//! Client orchestration: resolve the host, run sessions, reconnect with backoff, keep
//! the host informed about display changes.

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use moush_core::crypto::Identity;
use moush_core::discovery;
use moush_core::inject::{InjectState, Injector};
use moush_core::net::client::{run_session, ClientEvents, ClientLink, SessionConfig, SessionEnd};
use moush_core::net::{PeerInfo, PeerSender};
use moush_core::pairing::PairingBroker;
use moush_core::protocol::{MonitorDesc, MouseButton, WheelAxis};
use moush_core::stats::{LatencySummary, Stats};
use moush_core::trust::TrustStore;
use moush_core::util::now_ms;

use crate::features::FeatureHub;
use crate::host::Notify;
use crate::win::display_watch::DisplayWatcher;
use crate::win::dpi::make_thread_dpi_aware;
use crate::win::focus::FocusWorker;
use crate::win::inject::SendInputInjector;
use crate::win::monitors;

const BACKOFF_MS: [u64; 6] = [250, 500, 1000, 2000, 3000, 5000];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConnectTarget {
    /// "ip", "ip:port" or a host name.
    Address(String),
    /// A host found by discovery or paired before.
    Host { id: String, hint: Option<SocketAddr> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientState {
    Connecting,
    Connected,
    Reconnecting,
    Error,
    Stopped,
}

#[derive(Clone, Debug)]
pub struct ClientView {
    pub state: ClientState,
    pub target: String,
    pub host_id: Option<String>,
    pub host_name: Option<String>,
    pub host_address: Option<String>,
    pub active: bool,
    pub last_error: Option<String>,
    /// Unix ms of the next reconnect attempt.
    pub retry_at: Option<u64>,
    pub monitors: Vec<MonitorDesc>,
}

pub struct ClientOptions {
    pub name: String,
    pub default_port: u16,
    pub auto_reconnect: bool,
    /// Log input instead of injecting it (testing on one machine).
    pub dry_run: bool,
    /// When the shared cursor enters, bring the window under it to the front.
    /// Can be changed while running with [`ClientEngine::set_focus_on_enter`].
    pub focus_on_enter: bool,
}

struct Shared {
    view: Mutex<ClientView>,
    notify: Notify,
    stats: Arc<Stats>,
    lost_at: Mutex<Option<Instant>>,
    /// Called with the address of every successful connection (to remember it).
    on_connected: Box<dyn Fn(&str) + Send + Sync>,
    hub: Arc<FeatureHub>,
    /// (host id, connection number) of the current session, for the feature hub.
    session: Mutex<Option<(String, u64)>>,
    sessions: std::sync::atomic::AtomicU64,
    focus_on_enter: AtomicBool,
    /// None in dry-run mode (nothing is injected, so nothing is focused either).
    focus: Option<FocusWorker>,
}

impl Shared {
    fn update(&self, f: impl FnOnce(&mut ClientView)) {
        f(&mut self.view.lock().unwrap());
        (self.notify)();
    }
}

impl ClientEvents for Shared {
    fn connected(&self, host: &PeerInfo, sender: &PeerSender) {
        let conn_no = self.sessions.fetch_add(1, Ordering::SeqCst) + 1;
        *self.session.lock().unwrap() = Some((host.id.clone(), conn_no));
        self.hub.peer_up(&host.id, &host.name, conn_no, sender.clone());
        if let Some(t) = self.lost_at.lock().unwrap().take() {
            Stats::add(&self.stats.reconnects, 1);
            self.stats.last_reconnect_ms.store(t.elapsed().as_millis() as u64, Ordering::Relaxed);
        }
        (self.on_connected)(&host.addr.to_string());
        self.update(|v| {
            v.state = ClientState::Connected;
            v.host_id = Some(host.id.clone());
            v.host_name = Some(host.name.clone());
            v.host_address = Some(host.addr.to_string());
            v.last_error = None;
            v.retry_at = None;
        });
    }

    fn active_changed(&self, active: bool) {
        log::info!("{}", if active { "shared cursor entered this computer" } else { "shared cursor left this computer" });
        if active
            && self.focus_on_enter.load(Ordering::Relaxed)
            && let Some(f) = &self.focus
        {
            f.request(); // never blocks: the work happens on the focus thread
        }
        self.update(|v| v.active = active);
    }

    fn disconnected(&self) {
        if let Some((id, conn_no)) = self.session.lock().unwrap().take() {
            self.hub.peer_down(&id, conn_no);
        }
    }

    fn capabilities(&self, features: u32) {
        if let Some((id, conn_no)) = self.session.lock().unwrap().clone() {
            self.hub.on_capabilities(&id, conn_no, features);
        }
    }

    fn feature(&self, channel: u8, body: Vec<u8>) {
        let session = self.session.lock().unwrap().clone();
        if let Some((id, conn_no)) = session {
            self.hub.on_feature(&id, conn_no, channel, body);
        }
    }
}

pub struct ClientEngine {
    shared: Arc<Shared>,
    link: Arc<ClientLink>,
    stop: Arc<AtomicBool>,
    wake: Sender<()>,
    thread: Option<JoinHandle<()>>,
    _watcher: Option<DisplayWatcher>,
}

impl ClientEngine {
    #[allow(clippy::too_many_arguments)]
    pub fn start(
        target: ConnectTarget,
        opts: ClientOptions,
        identity: Arc<Identity>,
        trust: Arc<TrustStore>,
        broker: Arc<PairingBroker>,
        stats: Arc<Stats>,
        hub: Arc<FeatureHub>,
        notify: Notify,
        on_connected: impl Fn(&str) + Send + Sync + 'static,
    ) -> std::io::Result<Self> {
        make_thread_dpi_aware();
        let target_text = match &target {
            ConnectTarget::Address(a) => a.clone(),
            ConnectTarget::Host { id, .. } => trust.get(id).map(|p| p.name).unwrap_or_else(|| id.clone()),
        };
        let shared = Arc::new(Shared {
            view: Mutex::new(ClientView {
                state: ClientState::Connecting,
                target: target_text,
                host_id: None,
                host_name: None,
                host_address: None,
                active: false,
                last_error: None,
                retry_at: None,
                monitors: current_monitors(),
            }),
            notify,
            stats: stats.clone(),
            lost_at: Mutex::new(None),
            on_connected: Box::new(on_connected),
            hub,
            session: Mutex::new(None),
            sessions: std::sync::atomic::AtomicU64::new(0),
            focus_on_enter: AtomicBool::new(opts.focus_on_enter),
            focus: if opts.dry_run {
                None
            } else {
                match FocusWorker::start() {
                    Ok(f) => Some(f),
                    Err(e) => {
                        log::warn!("focus on enter unavailable: {e}");
                        None
                    }
                }
            },
        });
        let link = ClientLink::new();
        let stop = Arc::new(AtomicBool::new(false));
        let (wake_tx, wake_rx) = mpsc::channel();

        let watcher = {
            let link = link.clone();
            let shared = shared.clone();
            match DisplayWatcher::start(move || {
                let m = current_monitors();
                log::info!("display configuration changed: {} monitor(s)", m.len());
                link.send_screens(m.clone());
                shared.update(|v| v.monitors = m);
            }) {
                Ok(w) => Some(w),
                Err(e) => {
                    log::warn!("display change notifications unavailable: {e}");
                    None
                }
            }
        };

        let ctx = Worker { target, opts, identity, trust, broker, stats, shared: shared.clone(), link: link.clone(), stop: stop.clone(), wake: wake_rx };
        let thread = thread::Builder::new().name("client".into()).spawn(move || ctx.run())?;
        Ok(Self { shared, link, stop, wake: wake_tx, thread: Some(thread), _watcher: watcher })
    }

    pub fn view(&self) -> ClientView {
        self.shared.view.lock().unwrap().clone()
    }

    pub fn rtt_ms(&self) -> Option<f64> {
        self.link.rtt_ms()
    }

    /// Applies the "focus on enter" option immediately (next time the cursor enters).
    pub fn set_focus_on_enter(&self, on: bool) {
        self.shared.focus_on_enter.store(on, Ordering::Relaxed);
    }

    /// Cancels a pairing that is waiting for the host (closes the connection attempt).
    pub fn cancel_pending(&self) {
        self.link.close();
    }

    pub fn latency_test(&self, count: u32) -> std::io::Result<(String, u32, LatencySummary)> {
        let name = self.view().host_name.unwrap_or_else(|| "host".into());
        let (n, s) = self.link.latency_test(count)?;
        Ok((name, n, s))
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let Some(t) = self.thread.take() else { return };
        self.stop.store(true, Ordering::SeqCst);
        let _ = self.wake.send(());
        self.link.close();
        let _ = t.join();
    }
}

impl Drop for ClientEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn current_monitors() -> Vec<MonitorDesc> {
    let mut m: Vec<MonitorDesc> = monitors::enumerate().iter().map(|m| m.to_desc()).collect();
    if m.is_empty() {
        m.push(MonitorDesc { x: 0, y: 0, width: 1920, height: 1080, dpi: 96, primary: true, name: "Display".into() });
    }
    m
}

/// Logs input instead of injecting it.
struct DryRunInjector;

impl Injector for DryRunInjector {
    fn move_abs(&mut self, x: i32, y: i32) -> bool {
        log::debug!("[dry-run] move {x},{y}");
        true
    }
    fn button(&mut self, b: MouseButton, down: bool) -> bool {
        log::info!("[dry-run] {b:?} {}", if down { "down" } else { "up" });
        true
    }
    fn wheel(&mut self, a: WheelAxis, d: i16) -> bool {
        log::info!("[dry-run] wheel {a:?} {d}");
        true
    }
    fn key(&mut self, vk: u16, scan: u16, ext: bool, down: bool, _by_vk: bool) -> bool {
        log::info!("[dry-run] key vk=0x{vk:02x} scan=0x{scan:02x}{} {}", if ext { " ext" } else { "" }, if down { "down" } else { "up" });
        true
    }
}

enum AnyInjector {
    Real(SendInputInjector),
    Dry(DryRunInjector),
}

impl Injector for AnyInjector {
    fn move_abs(&mut self, x: i32, y: i32) -> bool {
        match self {
            Self::Real(i) => i.move_abs(x, y),
            Self::Dry(i) => i.move_abs(x, y),
        }
    }
    fn button(&mut self, b: MouseButton, down: bool) -> bool {
        match self {
            Self::Real(i) => i.button(b, down),
            Self::Dry(i) => i.button(b, down),
        }
    }
    fn wheel(&mut self, a: WheelAxis, d: i16) -> bool {
        match self {
            Self::Real(i) => i.wheel(a, d),
            Self::Dry(i) => i.wheel(a, d),
        }
    }
    fn key(&mut self, vk: u16, scan: u16, ext: bool, down: bool, by_vk: bool) -> bool {
        match self {
            Self::Real(i) => i.key(vk, scan, ext, down, by_vk),
            Self::Dry(i) => i.key(vk, scan, ext, down, by_vk),
        }
    }
}

struct Worker {
    target: ConnectTarget,
    opts: ClientOptions,
    identity: Arc<Identity>,
    trust: Arc<TrustStore>,
    broker: Arc<PairingBroker>,
    stats: Arc<Stats>,
    shared: Arc<Shared>,
    link: Arc<ClientLink>,
    stop: Arc<AtomicBool>,
    wake: Receiver<()>,
}

impl Worker {
    fn run(self) {
        make_thread_dpi_aware();
        let injector = if self.opts.dry_run { AnyInjector::Dry(DryRunInjector) } else { AnyInjector::Real(SendInputInjector::new()) };
        let mut inject = InjectState::new(injector);
        // Once connected, we know who the host is: later attempts must reach the same device.
        let mut known_host = match &self.target {
            ConnectTarget::Host { id, .. } => Some(id.clone()),
            ConnectTarget::Address(_) => None,
        };
        let mut failures: usize = 0;
        while !self.stop.load(Ordering::SeqCst) {
            let addr = match self.resolve(known_host.as_deref(), failures) {
                Ok(a) => a,
                Err(e) => {
                    if !self.wait_retry(&mut failures, e) {
                        break;
                    }
                    continue;
                }
            };
            self.shared.update(|v| v.host_address = Some(addr.to_string()));
            let end = run_session(
                SessionConfig {
                    addr,
                    expected_host: known_host.clone(),
                    identity: &self.identity,
                    name: &self.opts.name,
                    trust: &self.trust,
                    broker: &self.broker,
                    stats: &self.stats,
                    screens: current_monitors(),
                    stop: &self.stop,
                },
                &mut inject,
                &self.link,
                self.shared.as_ref(),
            );
            let was_connected = self.shared.view.lock().unwrap().state == ClientState::Connected;
            if was_connected {
                known_host = self.shared.view.lock().unwrap().host_id.clone();
                failures = 0;
                *self.shared.lost_at.lock().unwrap() = Some(Instant::now());
            }
            match end {
                SessionEnd::Stopped => break,
                SessionEnd::Fatal(msg) => {
                    log::warn!("connection ended: {msg}");
                    self.shared.update(|v| {
                        v.state = ClientState::Error;
                        v.last_error = Some(msg);
                        v.active = false;
                        v.retry_at = None;
                    });
                    return;
                }
                SessionEnd::Lost(msg) => {
                    log::info!("connection lost: {msg}");
                    if !self.wait_retry(&mut failures, msg) {
                        break;
                    }
                }
            }
        }
        self.shared.update(|v| {
            v.state = ClientState::Stopped;
            v.active = false;
            v.retry_at = None;
        });
    }

    /// Records a failure and sleeps the backoff delay. Returns false when stopped or
    /// when automatic reconnection is disabled.
    fn wait_retry(&self, failures: &mut usize, error: String) -> bool {
        if !self.opts.auto_reconnect && *failures > 0 {
            self.shared.update(|v| {
                v.state = ClientState::Error;
                v.last_error = Some(error);
                v.retry_at = None;
            });
            return false;
        }
        let delay = BACKOFF_MS[(*failures).min(BACKOFF_MS.len() - 1)];
        *failures += 1;
        let ever_connected = self.shared.view.lock().unwrap().host_id.is_some();
        self.shared.update(|v| {
            v.state = if ever_connected { ClientState::Reconnecting } else { ClientState::Connecting };
            v.last_error = Some(error);
            v.active = false;
            v.retry_at = Some(now_ms() + delay);
        });
        match self.wake.recv_timeout(Duration::from_millis(delay)) {
            Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => false,
            Err(mpsc::RecvTimeoutError::Timeout) => !self.stop.load(Ordering::SeqCst),
        }
    }

    fn resolve(&self, known_host: Option<&str>, failures: usize) -> Result<SocketAddr, String> {
        match &self.target {
            ConnectTarget::Address(a) => {
                let direct = parse_address(a, self.opts.default_port);
                // After repeated failures, the host may have a new IP: look for it by id.
                if (direct.is_err() || failures >= 2)
                    && let Some(id) = known_host
                    && let Some(found) = discover(id)
                {
                    return Ok(found);
                }
                direct
            }
            ConnectTarget::Host { id, hint } => {
                if failures == 0
                    && let Some(h) = hint
                {
                    return Ok(*h);
                }
                if let Some(found) = discover(id) {
                    return Ok(found);
                }
                hint.or_else(|| self.trust.get(id).and_then(|p| p.last_address).and_then(|a| a.parse().ok()))
                    .ok_or_else(|| "the host was not found on the network".to_owned())
            }
        }
    }
}

fn discover(id: &str) -> Option<SocketAddr> {
    discovery::scan(Duration::from_millis(700))
        .ok()?
        .into_iter()
        .find(|f| f.announce.id == id)
        .map(|f| f.addr)
}

/// Parses "host", "host:port", "1.2.3.4", "[::1]:24800". Prefers IPv4.
pub fn parse_address(s: &str, default_port: u16) -> Result<SocketAddr, String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("enter the host's IP address or name".into());
    }
    let with_port = if s.parse::<SocketAddr>().is_ok() {
        s.to_owned()
    } else if s.parse::<std::net::Ipv6Addr>().is_ok() {
        format!("[{s}]:{default_port}")
    } else if s.rsplit_once(':').is_some_and(|(_, p)| p.parse::<u16>().is_ok()) && !s.contains('[') {
        s.to_owned()
    } else {
        format!("{s}:{default_port}")
    };
    let addrs: Vec<SocketAddr> = with_port.to_socket_addrs().map_err(|e| format!("cannot resolve \"{s}\": {e}"))?.collect();
    addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or(addrs.first())
        .copied()
        .ok_or_else(|| format!("\"{s}\" did not resolve to an address"))
}

#[cfg(test)]
mod tests {
    use super::parse_address;

    #[test]
    fn addresses() {
        assert_eq!(parse_address("192.168.1.20", 24800).unwrap().to_string(), "192.168.1.20:24800");
        assert_eq!(parse_address(" 10.0.0.5:9000 ", 24800).unwrap().to_string(), "10.0.0.5:9000");
        assert_eq!(parse_address("::1", 24800).unwrap().to_string(), "[::1]:24800");
        assert_eq!(parse_address("localhost", 24800).unwrap().port(), 24800);
        assert!(parse_address("", 24800).is_err());
    }
}
