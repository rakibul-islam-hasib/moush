//! Host orchestration: wires the network server, the input capture thread, discovery
//! and the layout together. A single control thread processes every event (client
//! joined/left, display changed, UI edits) in order, so no state is shared between the
//! hook thread and the network threads except through message queues.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use moush_core::crypto::Identity;
use moush_core::discovery::{Announce, Responder};
use moush_core::net::host::{ClientHandle, ClientInfo, HostEvents, HostServer};
use moush_core::pairing::PairingBroker;
use moush_core::protocol::MonitorDesc;
use moush_core::router::RouterSettings;
use moush_core::stats::{LatencySummary, Stats};
use moush_core::trust::TrustStore;

use crate::features::FeatureHub;
use crate::layout::{self, ClientView, LayoutStore};
use crate::win::capture::{Capture, CaptureCmd, CaptureEvents};
use crate::win::dpi::make_thread_dpi_aware;
use crate::win::monitors::{self, Monitor};

pub type Notify = Arc<dyn Fn() + Send + Sync>;

pub struct HostOptions {
    pub port: u16,
    pub name: String,
    pub discovery: bool,
    pub router: RouterSettings,
    pub layout_path: Option<PathBuf>,
}

/// What the UI shows about the host.
#[derive(Clone, Debug, Default)]
pub struct HostView {
    pub port: u16,
    pub active: Option<String>,
    pub locked: bool,
    pub monitors: Vec<Monitor>,
    pub layout: Vec<ClientView>,
    pub warning: Option<String>,
}

enum Ctl {
    Connected(ClientHandle),
    Screens(String, u64, Vec<MonitorDesc>),
    Gone(String, u64),
    Active(Option<String>),
    Locked(bool),
    Displays,
    SendFailed(String),
    Place(Vec<(String, f64, f64)>),
    Forget(String),
    Stop,
}

struct Events {
    ctl: Mutex<Sender<Ctl>>,
    hub: Arc<FeatureHub>,
}

impl Events {
    fn send(&self, c: Ctl) {
        let _ = self.ctl.lock().unwrap().send(c);
    }
}

impl HostEvents for Events {
    fn client_connected(&self, client: ClientHandle) {
        // Synchronously, before the client's reader starts: feature traffic may follow at once.
        self.hub.peer_up(&client.id, &client.name, client.conn_no, client.sender.clone());
        self.send(Ctl::Connected(client));
    }
    fn client_screens(&self, id: &str, conn_no: u64, screens: Vec<MonitorDesc>) {
        self.send(Ctl::Screens(id.to_owned(), conn_no, screens));
    }
    fn client_disconnected(&self, id: &str, conn_no: u64, _reason: &str) {
        self.hub.peer_down(id, conn_no);
        self.send(Ctl::Gone(id.to_owned(), conn_no));
    }
    fn client_capabilities(&self, id: &str, conn_no: u64, features: u32) {
        self.hub.on_capabilities(id, conn_no, features);
    }
    fn client_feature(&self, id: &str, conn_no: u64, channel: u8, body: Vec<u8>) {
        self.hub.on_feature(id, conn_no, channel, body);
    }
}

impl CaptureEvents for Events {
    fn active_changed(&self, client: Option<String>) {
        self.send(Ctl::Active(client));
    }
    fn lock_changed(&self, locked: bool) {
        self.send(Ctl::Locked(locked));
    }
    fn displays_changed(&self) {
        self.send(Ctl::Displays);
    }
    fn send_failed(&self, client: &str) {
        self.send(Ctl::SendFailed(client.to_owned()));
    }
}

// Field order matters: on drop the capture thread stops first (cursor home, the active
// client told to release everything), then the server closes the connections.
struct Shared {
    capture: Capture,
    server: HostServer,
    view: Mutex<HostView>,
    notify: Notify,
}

pub struct HostEngine {
    shared: Arc<Shared>,
    ctl: Sender<Ctl>,
    control: Option<JoinHandle<()>>,
    _responder: Option<Responder>,
}

impl HostEngine {
    pub fn start(
        opts: HostOptions,
        identity: Arc<Identity>,
        trust: Arc<TrustStore>,
        broker: Arc<PairingBroker>,
        stats: Arc<Stats>,
        hub: Arc<FeatureHub>,
        notify: Notify,
    ) -> io::Result<Self> {
        make_thread_dpi_aware();
        let (tx, rx) = mpsc::channel();
        let events = Arc::new(Events { ctl: Mutex::new(tx.clone()), hub });
        let mut store = LayoutStore::load(opts.layout_path.clone());
        // Drop layout entries for devices that are no longer paired.
        let trusted: Vec<String> = trust.list().into_iter().map(|p| p.id).collect();
        if store.clients.iter().any(|c| !trusted.contains(&c.id)) {
            store.clients.retain(|c| trusted.contains(&c.id));
            let _ = store.save();
        }
        let host_monitors = monitors::enumerate();
        if host_monitors.is_empty() {
            return Err(io::Error::other("no monitors detected"));
        }
        let (layout, views) = layout::build(&host_monitors, &[], &store);

        let capture = Capture::start(layout, opts.router, stats.clone(), events.clone())?;
        let server = match HostServer::start(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, opts.port), opts.name.clone(), identity.clone(), trust, broker, stats, events) {
            Ok(s) => s,
            Err(e) => {
                capture.stop();
                return Err(e);
            }
        };
        let mut warning = None;
        let responder = if opts.discovery {
            match Responder::start(Announce { id: identity.device_id(), name: opts.name.clone(), port: server.port() }) {
                Ok(r) => Some(r),
                Err(e) => {
                    log::warn!("LAN discovery unavailable: {e}");
                    warning = Some(format!(
                        "Automatic discovery is unavailable ({e}). Clients can still connect by IP address."
                    ));
                    None
                }
            }
        } else {
            None
        };
        let shared = Arc::new(Shared {
            view: Mutex::new(HostView {
                port: server.port(),
                active: None,
                locked: false,
                monitors: host_monitors.clone(),
                layout: views,
                warning,
            }),
            capture,
            server,
            notify,
        });
        let shared2 = shared.clone();
        let control = thread::Builder::new()
            .name("host-control".into())
            .spawn(move || control_loop(rx, shared2, store, host_monitors))?;
        Ok(Self { shared, ctl: tx, control: Some(control), _responder: responder })
    }

    pub fn view(&self) -> HostView {
        self.shared.view.lock().unwrap().clone()
    }

    pub fn clients(&self) -> Vec<ClientInfo> {
        self.shared.server.clients()
    }

    pub fn hooks_installed(&self) -> bool {
        self.shared.capture.hooks_installed()
    }

    pub fn set_placements(&self, placements: Vec<(String, f64, f64)>) {
        let _ = self.ctl.send(Ctl::Place(placements));
    }

    /// Removes a device from the layout (after it was unpaired).
    pub fn forget(&self, id: &str) {
        let _ = self.ctl.send(Ctl::Forget(id.to_owned()));
        self.shared.server.disconnect(id);
    }

    pub fn set_settings(&self, s: RouterSettings) {
        self.shared.capture.send(CaptureCmd::SetSettings(s));
    }

    pub fn test_switch(&self, id: &str) -> Result<(), String> {
        if !self.view().layout.iter().any(|c| c.id == id && c.online) {
            return Err("that computer is not connected".into());
        }
        self.shared.capture.send(CaptureCmd::SwitchTo(id.to_owned()));
        Ok(())
    }

    pub fn force_local(&self) {
        self.shared.capture.send(CaptureCmd::ForceLocal);
    }

    pub fn disconnect(&self, id: &str) -> bool {
        self.shared.server.disconnect(id)
    }

    pub fn refresh_monitors(&self) {
        let _ = self.ctl.send(Ctl::Displays);
    }

    /// Pings `id` (or the active / first client) and returns (name, replies, RTT ms).
    pub fn latency_test(&self, id: Option<&str>, count: u32) -> io::Result<(String, u32, LatencySummary)> {
        let target = match id {
            Some(id) => id.to_owned(),
            None => {
                let v = self.view();
                let clients = self.clients();
                v.active
                    .or_else(|| clients.first().map(|c| c.id.clone()))
                    .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "no computer is connected"))?
            }
        };
        self.shared.server.latency_test(&target, count)
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let Some(control) = self.control.take() else { return };
        let _ = self.ctl.send(Ctl::Stop);
        let _ = control.join();
        // Bring the cursor home (tells the active client to release everything) before
        // the connections close. Dropping `shared` then stops the server and the hooks.
        self.shared.capture.send(CaptureCmd::ForceLocal);
    }
}

impl Drop for HostEngine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct Online {
    name: String,
    conn_no: u64,
}

fn control_loop(rx: Receiver<Ctl>, shared: Arc<Shared>, mut store: LayoutStore, mut monitors: Vec<Monitor>) {
    make_thread_dpi_aware();
    let mut online: HashMap<String, Online> = HashMap::new();
    let rebuild = |store: &LayoutStore, monitors: &[Monitor], online: &HashMap<String, Online>| {
        let ids: Vec<String> = online.keys().cloned().collect();
        let (layout, views) = layout::build(monitors, &ids, store);
        shared.capture.send(CaptureCmd::SetLayout(layout));
        let mut v = shared.view.lock().unwrap();
        v.layout = views;
        v.monitors = monitors.to_vec();
    };
    while let Ok(msg) = rx.recv() {
        match msg {
            Ctl::Stop => break,
            Ctl::Connected(c) => {
                if store.remember(&c.id, &c.name, &c.screens) {
                    let _ = store.save();
                }
                online.insert(c.id.clone(), Online { name: c.name.clone(), conn_no: c.conn_no });
                // Queue first, then layout: the router may route to it immediately.
                shared.capture.send(CaptureCmd::AddClient { id: c.id.clone(), outbox: c.outbox });
                rebuild(&store, &monitors, &online);
            }
            Ctl::Screens(id, conn_no, screens) => {
                if let Some(o) = online.get(&id).filter(|o| o.conn_no == conn_no) {
                    if store.remember(&id, &o.name.clone(), &screens) {
                        let _ = store.save();
                    }
                    rebuild(&store, &monitors, &online);
                }
            }
            Ctl::Gone(id, conn_no) => {
                if online.get(&id).is_some_and(|o| o.conn_no == conn_no) {
                    online.remove(&id);
                    shared.capture.send(CaptureCmd::RemoveClient { id });
                    rebuild(&store, &monitors, &online);
                }
            }
            Ctl::Active(a) => shared.view.lock().unwrap().active = a,
            Ctl::Locked(l) => shared.view.lock().unwrap().locked = l,
            Ctl::Displays => {
                let m = monitors::enumerate();
                if !m.is_empty() && m != monitors {
                    log::info!("display configuration changed: {} monitor(s)", m.len());
                    monitors = m;
                    rebuild(&store, &monitors, &online);
                }
            }
            Ctl::SendFailed(id) => {
                log::warn!("{id} is not accepting input; disconnecting it");
                shared.server.disconnect(&id);
            }
            Ctl::Place(list) => {
                let mut changed = false;
                for (id, x, y) in list {
                    changed |= store.set_position(&id, x, y);
                }
                if changed {
                    if let Err(e) = store.save() {
                        log::error!("cannot save layout: {e}");
                    }
                    log::info!("layout updated");
                    rebuild(&store, &monitors, &online);
                }
            }
            Ctl::Forget(id) => {
                if store.forget(&id) {
                    let _ = store.save();
                    rebuild(&store, &monitors, &online);
                }
            }
        }
        (shared.notify)();
    }
}
