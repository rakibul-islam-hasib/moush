//! Application controller: owns configuration, the running host or client engine and
//! the shared services, builds UI snapshots and emits them when anything changes.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use moush_engine::client::{ClientEngine, ClientOptions, ClientState, ConnectTarget};
use moush_engine::core::crypto::Identity;
use moush_engine::core::discovery;
use moush_engine::core::pairing::PairingBroker;
use moush_engine::core::protocol::MonitorDesc;
use moush_engine::core::stats::Stats;
use moush_engine::core::trust::{TrustRole, TrustStore};
use moush_engine::core::util::now_ms;
use moush_engine::core::features::{self as feat, cap};
use moush_engine::discovered::DiscoveryCache;
use moush_engine::features::FeatureHub;
use moush_engine::host::{HostEngine, HostOptions, Notify};
use moush_engine::paths::AppPaths;
use moush_engine::win::monitors::{self, Monitor};
use moush_engine::win::system::{self, FirewallState, ProcessMonitor};

use crate::dto::*;
use crate::settings::{Config, Role, Settings};

/// What is running right now.
#[derive(Default)]
enum Runtime {
    #[default]
    Idle,
    Host(HostEngine),
    Client(ClientEngine),
}

pub struct App {
    pub paths: AppPaths,
    identity: Arc<Identity>,
    trust: Arc<TrustStore>,
    broker: Arc<PairingBroker>,
    stats: Arc<Stats>,
    discovered: Arc<DiscoveryCache>,
    config: Mutex<Config>,
    runtime: Mutex<Runtime>,
    host_error: Mutex<Option<String>>,
    process: Mutex<ProcessMonitor>,
    started: Instant,
    pub platform: Mutex<PlatformInfo>,
    notify: Notify,
    /// Connects optional features (clipboard, file transfer) to the connected peers.
    pub hub: Arc<FeatureHub>,
    handle: AppHandle,
}

impl App {
    pub fn new(handle: AppHandle, paths: AppPaths) -> Result<Arc<Self>, String> {
        let identity = moush_engine::identity::load_or_create(&paths.identity()).map_err(|e| e.to_string())?;
        let trust = TrustStore::load(paths.trusted()).map_err(|e| format!("cannot read paired devices: {e}"))?;
        let config = Config::load(&paths.settings(), &system::computer_name());
        let os_build = system::os_build();

        let (tx, rx) = mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        let notify: Notify = Arc::new(move || {
            let _ = tx.lock().unwrap().send(());
        });
        let app = Arc::new(Self {
            paths,
            identity: Arc::new(identity),
            trust: Arc::new(trust),
            broker: PairingBroker::new(notify.clone()),
            stats: Arc::new(Stats::new()),
            discovered: DiscoveryCache::new(),
            config: Mutex::new(config),
            runtime: Mutex::new(Runtime::Idle),
            host_error: Mutex::new(None),
            process: Mutex::new(ProcessMonitor::new()),
            started: Instant::now(),
            platform: Mutex::new(PlatformInfo { os_build, windows11: os_build >= 22000, mica: false, elevated: system::is_elevated() }),
            notify,
            hub: FeatureHub::new(),
            handle: handle.clone(),
        });
        let weak = Arc::downgrade(&app);
        app.hub.set_policy(move |id| weak.upgrade().map(|a| a.local_caps(id)).unwrap_or(0));
        // Emit snapshots from one thread, coalescing bursts of changes.
        let weak = Arc::downgrade(&app);
        thread::Builder::new()
            .name("ui-snapshots".into())
            .spawn(move || emit_loop(rx, weak, handle))
            .map_err(|e| e.to_string())?;
        Ok(app)
    }

    pub fn notify(&self) {
        (self.notify)();
    }

    pub fn handle(&self) -> &AppHandle {
        &self.handle
    }

    /// Feature bits this computer allows with a peer: global settings combined with the
    /// paired device's policy. Unpaired devices get nothing.
    pub fn local_caps(&self, peer_id: &str) -> u32 {
        let Some(policy) = self.trust.policy(peer_id) else { return 0 };
        let s = self.config().settings;
        feat::local_caps(&s.clipboard, &s.transfer, &policy)
    }

    pub fn set_device_policy(&self, id: &str, policy: feat::DevicePolicy) -> Result<(), String> {
        if !self.trust.set_policy(id, policy).map_err(|e| e.to_string())? {
            return Err("that device is not paired".into());
        }
        self.hub.refresh_caps();
        self.notify();
        Ok(())
    }

    pub fn config(&self) -> Config {
        self.config.lock().unwrap().clone()
    }

    fn save_config(&self, c: &Config) {
        if let Err(e) = c.save(&self.paths.settings()) {
            log::error!("cannot save settings: {e}");
        }
    }

    /// Starts whatever the saved role says should run at launch.
    pub fn autostart(&self) {
        let c = self.config();
        if !c.settings.auto_start {
            return;
        }
        match c.role {
            Some(Role::Host) => {
                if let Err(e) = self.host_start() {
                    log::error!("could not start sharing: {e}");
                }
            }
            Some(Role::Client) => {
                if let Some(target) = self.last_host_target(&c) {
                    let _ = self.start_client(target);
                }
            }
            None => {}
        }
    }

    fn last_host_target(&self, c: &Config) -> Option<ConnectTarget> {
        match (&c.last_host_id, &c.settings.last_host_address) {
            (Some(id), addr) if self.trust.get(id).is_some() => {
                Some(ConnectTarget::Host { id: id.clone(), hint: addr.as_ref().and_then(|a| a.parse().ok()) })
            }
            (_, Some(addr)) => Some(ConnectTarget::Address(addr.clone())),
            _ => None,
        }
    }

    // ------------------------------------------------------------- role

    pub fn set_role(&self, role: Option<Role>) -> Result<(), String> {
        self.stop_runtime();
        let mut c = self.config();
        c.role = role;
        self.save_config(&c);
        *self.config.lock().unwrap() = c;
        log::info!("role set to {}", role.map(Role::as_str).unwrap_or("none"));
        if role == Some(Role::Host) {
            self.host_start()?;
        }
        if role == Some(Role::Client) {
            self.discovered.scan_async(self.notify.clone());
        }
        self.notify();
        Ok(())
    }

    pub fn stop_runtime(&self) {
        let rt = std::mem::take(&mut *self.runtime.lock().unwrap());
        match rt {
            Runtime::Host(h) => h.stop(),
            Runtime::Client(c) => c.stop(),
            Runtime::Idle => {}
        }
        self.notify();
    }

    // ------------------------------------------------------------- host

    pub fn host_start(&self) -> Result<(), String> {
        let c = self.config();
        if c.role != Some(Role::Host) {
            return Err("this computer is not set up as the host".into());
        }
        if matches!(*self.runtime.lock().unwrap(), Runtime::Host(_)) {
            return Ok(());
        }
        self.stop_runtime();
        let result = HostEngine::start(
            HostOptions {
                port: c.settings.port,
                name: c.settings.device_name.clone(),
                discovery: c.settings.discovery_enabled,
                router: c.settings.router(),
                layout_path: Some(self.paths.layout()),
            },
            self.identity.clone(),
            self.trust.clone(),
            self.broker.clone(),
            self.stats.clone(),
            self.hub.clone(),
            self.notify.clone(),
        );
        let r = match result {
            Ok(engine) => {
                *self.runtime.lock().unwrap() = Runtime::Host(engine);
                *self.host_error.lock().unwrap() = None;
                Ok(())
            }
            Err(e) => {
                let msg = format!("Could not start sharing: {e}");
                *self.host_error.lock().unwrap() = Some(msg.clone());
                Err(msg)
            }
        };
        self.notify();
        r
    }

    pub fn host_stop(&self) {
        if matches!(*self.runtime.lock().unwrap(), Runtime::Host(_)) {
            self.stop_runtime();
        }
    }

    pub fn with_host<R>(&self, f: impl FnOnce(&HostEngine) -> R) -> Result<R, String> {
        match &*self.runtime.lock().unwrap() {
            Runtime::Host(h) => Ok(f(h)),
            _ => Err("sharing is not running".into()),
        }
    }

    /// Returns the cursor to the host (tray menu, shutdown).
    pub fn force_local(&self) {
        let _ = self.with_host(|h| h.force_local());
    }

    // ------------------------------------------------------------- client

    pub fn client_connect(&self, address: Option<String>, host_id: Option<String>) -> Result<(), String> {
        if self.config().role != Some(Role::Client) {
            return Err("this computer is not set up as a client".into());
        }
        let target = match (host_id, address) {
            (Some(id), _) => {
                let hint = self.discovered.get(&id).map(|d| d.addr);
                ConnectTarget::Host { id, hint }
            }
            (None, Some(a)) if !a.trim().is_empty() => {
                // Validate early so the user gets an immediate error for typos.
                moush_engine::client::parse_address(&a, self.config().settings.port)?;
                ConnectTarget::Address(a.trim().to_owned())
            }
            _ => return Err("choose a host or enter its address".into()),
        };
        self.start_client(target)
    }

    fn start_client(&self, target: ConnectTarget) -> Result<(), String> {
        self.stop_runtime();
        let c = self.config();
        let engine = ClientEngine::start(
            target,
            ClientOptions {
                name: c.settings.device_name.clone(),
                default_port: c.settings.port,
                auto_reconnect: true,
                dry_run: std::env::var_os("MOUSH_DRY_RUN").is_some(),
            },
            self.identity.clone(),
            self.trust.clone(),
            self.broker.clone(),
            self.stats.clone(),
            self.hub.clone(),
            self.notify.clone(),
            |_| {}, // the host is remembered in `remember_host` once connected
        )
        .map_err(|e| e.to_string())?;
        *self.runtime.lock().unwrap() = Runtime::Client(engine);
        self.notify();
        Ok(())
    }

    pub fn client_disconnect(&self) {
        if matches!(*self.runtime.lock().unwrap(), Runtime::Client(_)) {
            self.stop_runtime();
        }
    }

    /// Called after a successful client connection: remember the host for next launch.
    pub fn remember_host(&self) {
        let host = match &*self.runtime.lock().unwrap() {
            Runtime::Client(c) => {
                let v = c.view();
                v.host_id.zip(v.host_address)
            }
            _ => None,
        };
        if let Some((id, addr)) = host {
            let mut c = self.config.lock().unwrap();
            if c.last_host_id.as_deref() != Some(&id) || c.settings.last_host_address.as_deref() != Some(&addr) {
                c.last_host_id = Some(id);
                c.settings.last_host_address = Some(addr);
                let snapshot = c.clone();
                drop(c);
                self.save_config(&snapshot);
            }
        }
    }

    pub fn discover(&self) {
        self.discovered.scan_async(self.notify.clone());
    }

    // ------------------------------------------------------------- pairing / trust

    pub fn respond_pairing(&self, request_id: &str, accept: bool) -> Result<(), String> {
        let req = self.broker.list().into_iter().find(|r| r.request_id == request_id);
        let Some(req) = req else { return Err("this pairing request has expired".into()) };
        let waiting = !req.needs_my_approval;
        self.broker.respond(request_id, accept);
        // Cancelling our own attempt to reach a host also stops the retries.
        if waiting && !accept && req.peer_role == TrustRole::Host {
            self.client_disconnect();
        }
        self.notify();
        Ok(())
    }

    pub fn forget_device(&self, id: &str) -> Result<(), String> {
        self.trust.remove(id).map_err(|e| e.to_string())?;
        match &*self.runtime.lock().unwrap() {
            Runtime::Host(h) => h.forget(id),
            Runtime::Client(c) if c.view().host_id.as_deref() == Some(id) => c.cancel_pending(),
            _ => {}
        }
        let mut c = self.config.lock().unwrap();
        if c.last_host_id.as_deref() == Some(id) {
            c.last_host_id = None;
            let snapshot = c.clone();
            drop(c);
            self.save_config(&snapshot);
        }
        log::info!("forgot device {id}");
        self.notify();
        Ok(())
    }

    // ------------------------------------------------------------- settings

    pub fn update_settings(&self, new: Settings) -> Result<Settings, String> {
        let mut new = new.validated()?;
        let old = self.config();
        // The UI never owns this field; keep what the backend recorded.
        new.last_host_address = old.settings.last_host_address.clone();
        if new.launch_at_startup != old.settings.launch_at_startup || new.launch_at_startup != system::launch_at_startup_enabled() {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            system::set_launch_at_startup(new.launch_at_startup, &exe, "--autostart")
                .map_err(|e| format!("could not change the startup setting: {e}"))?;
        }
        let mut c = old.clone();
        c.settings = new.clone();
        self.save_config(&c);
        *self.config.lock().unwrap() = c;
        self.hub.refresh_caps();

        let restart_host = new.port != old.settings.port
            || new.discovery_enabled != old.settings.discovery_enabled
            || new.device_name != old.settings.device_name;
        let host_running = matches!(*self.runtime.lock().unwrap(), Runtime::Host(_));
        if host_running {
            if restart_host {
                log::info!("restarting sharing to apply new network settings");
                self.stop_runtime();
                self.host_start()?;
            } else {
                let _ = self.with_host(|h| h.set_settings(new.router()));
            }
        }
        self.notify();
        Ok(new)
    }

    // ------------------------------------------------------------- snapshot

    pub fn snapshot(&self) -> AppSnapshot {
        let c = self.config();
        let pairing = self.broker.list();
        let trusted = self.trust.list();
        let (host, client) = match &*self.runtime.lock().unwrap() {
            Runtime::Host(h) => (Some(self.host_snapshot(Some(h))), None),
            Runtime::Client(cl) => (None, Some(self.client_snapshot(Some(cl), &pairing))),
            Runtime::Idle => match c.role {
                Some(Role::Host) => (Some(self.host_snapshot(None)), None),
                Some(Role::Client) => (None, Some(self.client_snapshot(None, &pairing))),
                None => (None, None),
            },
        };
        let discovered = if c.role == Some(Role::Client) {
            self.discovered
                .list()
                .into_iter()
                .filter(|d| d.id != self.identity.device_id())
                .map(|d| DiscoveredHost {
                    trusted: trusted.iter().any(|t| t.id == d.id),
                    id: d.id,
                    name: d.name,
                    address: d.addr.to_string(),
                    last_seen: d.last_seen,
                })
                .collect()
        } else {
            Vec::new()
        };
        AppSnapshot {
            role: c.role.map(Role::as_str),
            device_name: c.settings.device_name.clone(),
            device_id: self.identity.device_id(),
            fingerprint: self.identity.fingerprint(),
            version: env!("CARGO_PKG_VERSION"),
            platform: *self.platform.lock().unwrap(),
            host,
            client,
            pairing,
            trusted: trusted
                .iter()
                .map(|t| TrustedDevice {
                    id: t.id.clone(),
                    name: t.name.clone(),
                    role: t.role,
                    fingerprint: t.fingerprint(),
                    last_address: t.last_address.clone(),
                    paired_at: t.paired_at,
                    last_seen: t.last_seen,
                    policy: t.policy,
                    online: self.hub.peer(&t.id).is_some(),
                })
                .collect(),
            discovered,
            features: self.features_snapshot(),
        }
    }

    fn features_snapshot(&self) -> FeaturesSnapshot {
        let s = self.config().settings;
        FeaturesSnapshot {
            peers: self
                .hub
                .peers()
                .into_iter()
                .map(|p| FeaturePeer {
                    clipboard: p.effective_caps & cap::CLIPBOARD != 0,
                    files: p.effective_caps & cap::FILES != 0,
                    text_share: p.effective_caps & cap::TEXT_SHARE != 0,
                    id: p.id,
                    name: p.name,
                })
                .collect(),
            clipboard: ClipboardStatus { enabled: s.clipboard.enabled, ..Default::default() },
            transfers: Vec::new(),
            shared_texts: Vec::new(),
        }
    }

    fn host_snapshot(&self, h: Option<&HostEngine>) -> HostSnapshot {
        let error = self.host_error.lock().unwrap().clone();
        let Some(h) = h else {
            let host_monitors = monitors::enumerate();
            return HostSnapshot {
                running: false,
                listen_port: self.config().settings.port,
                capture_active: false,
                active_target: None,
                locked: false,
                clients: vec![],
                monitors: host_monitors.iter().map(monitor_info).collect(),
                layout: vec![],
                error,
            };
        };
        let v = h.view();
        let clients = h.clients();
        HostSnapshot {
            running: true,
            listen_port: v.port,
            capture_active: h.hooks_installed(),
            active_target: v.active.clone(),
            locked: v.locked,
            clients: clients
                .iter()
                .map(|c| ConnectedClient {
                    id: c.id.clone(),
                    name: c.name.clone(),
                    address: c.addr.to_string(),
                    active: v.active.as_deref() == Some(&c.id),
                    rtt_ms: c.rtt_ms,
                    monitors: c.screens.iter().enumerate().map(|(i, m)| desc_info(i, m)).collect(),
                    connected_at: c.connected_at,
                })
                .collect(),
            monitors: v.monitors.iter().map(monitor_info).collect(),
            layout: v
                .layout
                .iter()
                .map(|l| ClientLayoutView {
                    client_id: l.id.clone(),
                    name: l.name.clone(),
                    online: l.online,
                    x: l.rect.x,
                    y: l.rect.y,
                    width: l.rect.w,
                    height: l.rect.h,
                    resolution_width: l.resolution.0,
                    resolution_height: l.resolution.1,
                    scale: l.scale,
                    monitors: l.monitors.iter().enumerate().map(|(i, m)| desc_info(i, m)).collect(),
                    adjacent: l.adjacent,
                })
                .collect(),
            error: error.or(v.warning),
        }
    }

    fn client_snapshot(&self, cl: Option<&ClientEngine>, pairing: &[PairingRequest]) -> ClientSnapshot {
        let Some(cl) = cl else {
            return ClientSnapshot {
                state: "idle",
                target: None,
                host_id: None,
                host_name: None,
                host_address: None,
                active: false,
                rtt_ms: None,
                last_error: None,
                retry_in_ms: None,
                monitors: monitors::enumerate().iter().enumerate().map(|(i, m)| desc_info(i, &m.to_desc())).collect(),
            };
        };
        let v = cl.view();
        let pairing_with_host = pairing.iter().any(|p| p.peer_role == TrustRole::Host);
        let state = match v.state {
            ClientState::Connecting | ClientState::Reconnecting if pairing_with_host => "pairing",
            ClientState::Connecting => "connecting",
            ClientState::Connected => "connected",
            ClientState::Reconnecting => "reconnecting",
            ClientState::Error => "error",
            ClientState::Stopped => "idle",
        };
        ClientSnapshot {
            state,
            target: Some(v.target.clone()),
            host_id: v.host_id.clone(),
            host_name: v.host_name.clone(),
            host_address: v.host_address.clone(),
            active: v.active,
            rtt_ms: cl.rtt_ms(),
            last_error: v.last_error.clone(),
            retry_in_ms: v.retry_at.map(|t| t.saturating_sub(now_ms())),
            monitors: v.monitors.iter().enumerate().map(|(i, m)| desc_info(i, m)).collect(),
        }
    }

    // ------------------------------------------------------------- diagnostics

    pub fn diagnostics(&self) -> Diagnostics {
        let p = self.process.lock().unwrap().sample();
        let s = &self.stats;
        let hook_installed = matches!(&*self.runtime.lock().unwrap(), Runtime::Host(h) if h.hooks_installed());
        let last_reconnect = s.last_reconnect_ms.load(std::sync::atomic::Ordering::Relaxed);
        Diagnostics {
            uptime_secs: self.started.elapsed().as_secs(),
            process: ProcessStats { cpu_percent: p.cpu_percent, memory_mb: p.memory_mb, threads: p.threads },
            input: InputStats {
                hook_installed,
                events_captured: Stats::get(&s.events_captured),
                events_forwarded: Stats::get(&s.events_forwarded),
                events_injected: Stats::get(&s.events_injected),
                inject_failures: Stats::get(&s.inject_failures),
                hook_latency_us: Stats::summary(&s.hook_us),
                send_latency_us: Stats::summary(&s.send_us),
                inject_latency_us: Stats::summary(&s.inject_us),
            },
            network: NetworkStats {
                rtt_ms: Stats::summary(&s.rtt_us).scaled(0.001),
                bytes_sent: Stats::get(&s.bytes_sent),
                bytes_received: Stats::get(&s.bytes_received),
                frames_sent: Stats::get(&s.frames_sent),
                frames_received: Stats::get(&s.frames_received),
                reconnects: Stats::get(&s.reconnects),
                last_reconnect_ms: (last_reconnect != u64::MAX).then_some(last_reconnect),
                interfaces: discovery::local_addresses()
                    .into_iter()
                    .map(|(name, ip)| InterfaceInfo { name, address: ip.to_string() })
                    .collect(),
            },
            firewall: match system::firewall_state() {
                FirewallState::Allowed => "allowed",
                FirewallState::Missing => "missing",
                FirewallState::Unknown => "unknown",
            },
            elevated: self.platform.lock().unwrap().elevated,
            log_path: self.paths.log_file().display().to_string(),
        }
    }

    pub fn latency_test(&self) -> Result<LatencyTestResult, String> {
        const COUNT: u32 = 100;
        let r = match &*self.runtime.lock().unwrap() {
            Runtime::Host(h) => h.latency_test(None, COUNT),
            Runtime::Client(c) => c.latency_test(COUNT),
            Runtime::Idle => return Err("not connected".into()),
        };
        let (peer_name, received, rtt) = r.map_err(|e| e.to_string())?;
        Ok(LatencyTestResult { peer_name, sent: COUNT, received, rtt_ms: rtt })
    }

    pub fn exe_path(&self) -> Result<PathBuf, String> {
        std::env::current_exe().map_err(|e| e.to_string())
    }

    /// Tray tooltip text.
    pub fn status_line(&self) -> String {
        let s = self.snapshot();
        if let Some(h) = &s.host {
            if !h.running {
                return "Moush — sharing is off".into();
            }
            return match (&h.active_target, h.clients.len()) {
                (Some(id), _) => {
                    let name = h.clients.iter().find(|c| &c.id == id).map(|c| c.name.as_str()).unwrap_or("client");
                    format!("Moush — controlling {name}")
                }
                (None, 0) => "Moush — waiting for computers".into(),
                (None, n) => format!("Moush — {n} computer(s) connected"),
            };
        }
        if let Some(c) = &s.client {
            return match c.state {
                "connected" => format!("Moush — connected to {}", c.host_name.as_deref().unwrap_or("host")),
                "idle" => "Moush — not connected".into(),
                other => format!("Moush — {other}"),
            };
        }
        "Moush".into()
    }
}

fn monitor_info(m: &Monitor) -> MonitorInfo {
    MonitorInfo {
        id: m.device.clone(),
        name: m.name.clone(),
        x: m.rect.x,
        y: m.rect.y,
        width: m.rect.w,
        height: m.rect.h,
        dpi: m.dpi,
        scale: m.dpi as f64 / 96.0,
        primary: m.primary,
    }
}

fn desc_info(i: usize, m: &MonitorDesc) -> MonitorInfo {
    MonitorInfo {
        id: format!("monitor-{i}"),
        name: m.name.clone(),
        x: m.x,
        y: m.y,
        width: m.width,
        height: m.height,
        dpi: m.dpi as u32,
        scale: m.dpi as f64 / 96.0,
        primary: m.primary,
    }
}

fn emit_loop(rx: Receiver<()>, app: std::sync::Weak<App>, handle: AppHandle) {
    let mut last_client_state: Option<&'static str> = None;
    let mut prompted: Vec<String> = Vec::new();
    while rx.recv().is_ok() {
        // Coalesce a burst of changes into one snapshot.
        thread::sleep(Duration::from_millis(25));
        while rx.try_recv().is_ok() {}
        let Some(app) = app.upgrade() else { break };
        let snap = app.snapshot();
        let client_state = snap.client.as_ref().map(|c| c.state);
        if client_state == Some("connected") && last_client_state != Some("connected") {
            app.remember_host();
        }
        last_client_state = client_state;
        // A pairing prompt must not go unseen while the window sits in the tray.
        let new_prompt = snap.pairing.iter().any(|p| p.needs_my_approval && !prompted.contains(&p.request_id));
        prompted = snap.pairing.iter().map(|p| p.request_id.clone()).collect();
        if new_prompt {
            crate::show_main_window(&handle);
        }
        if let Err(e) = handle.emit("snapshot", &snap) {
            log::debug!("snapshot emit failed: {e}");
        }
        crate::tray::update_tooltip(&handle, &app.status_line());
    }
}
