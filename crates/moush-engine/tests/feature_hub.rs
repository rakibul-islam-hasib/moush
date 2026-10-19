//! FeatureHub behaviour with fake services: peer lifecycle across reconnects, stale
//! connection numbers, capability negotiation and announcement, and dropping traffic that
//! must not reach a service. Uses one real loopback connection so the hub's capability
//! announcements can be observed on the other end.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use moush_engine::core::crypto::Identity;
use moush_engine::core::features::{cap, channel};
use moush_engine::core::inject::{InjectState, Injector};
use moush_engine::core::net::client::{ClientEvents, ClientLink, SessionConfig, run_session};
use moush_engine::core::net::host::{ClientHandle, HostEvents, HostServer};
use moush_engine::core::net::{PeerInfo, PeerSender};
use moush_engine::core::pairing::PairingBroker;
use moush_engine::core::protocol::{MonitorDesc, MouseButton, WheelAxis};
use moush_engine::core::stats::Stats;
use moush_engine::core::trust::{TrustRole, TrustStore};
use moush_engine::features::{FeatureHub, FeatureService, PeerSummary};

/// Records every callback as a line of text.
struct FakeService {
    channel: u8,
    log: Mutex<Vec<String>>,
}

impl FakeService {
    fn new(channel: u8) -> Arc<Self> {
        Arc::new(Self { channel, log: Mutex::new(Vec::new()) })
    }
    fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.log.lock().unwrap())
    }
}

impl FeatureService for FakeService {
    fn channel(&self) -> u8 {
        self.channel
    }
    fn peer_up(&self, p: &PeerSummary) {
        self.log.lock().unwrap().push(format!("up {} {}", p.id, p.effective_caps));
    }
    fn peer_down(&self, id: &str) {
        self.log.lock().unwrap().push(format!("down {id}"));
    }
    fn caps_changed(&self, id: &str, eff: u32) {
        self.log.lock().unwrap().push(format!("caps {id} {eff}"));
    }
    fn on_message(&self, id: &str, body: Vec<u8>) {
        self.log.lock().unwrap().push(format!("msg {id} {body:?}"));
    }
}

struct NoInput;
impl Injector for NoInput {
    fn move_abs(&mut self, _: i32, _: i32) -> bool {
        true
    }
    fn button(&mut self, _: MouseButton, _: bool) -> bool {
        true
    }
    fn wheel(&mut self, _: WheelAxis, _: i16) -> bool {
        true
    }
    fn key(&mut self, _: u16, _: u16, _: bool, _: bool, _: bool) -> bool {
        true
    }
}

struct HostSide(Mutex<Sender<ClientHandle>>);
impl HostEvents for HostSide {
    fn client_connected(&self, c: ClientHandle) {
        let _ = self.0.lock().unwrap().send(c);
    }
    fn client_screens(&self, _: &str, _: u64, _: Vec<MonitorDesc>) {}
    fn client_disconnected(&self, _: &str, _: u64, _: &str) {}
}

/// The far end: records the capability announcements it receives.
struct Remote(Mutex<Sender<u32>>);
impl ClientEvents for Remote {
    fn connected(&self, _: &PeerInfo, _: &PeerSender) {}
    fn active_changed(&self, _: bool) {}
    fn capabilities(&self, features: u32) {
        let _ = self.0.lock().unwrap().send(features);
    }
}

struct Link {
    _server: HostServer,
    sender: PeerSender,
    announced: Receiver<u32>,
    stop: Arc<AtomicBool>,
    link: Arc<ClientLink>,
    session: Option<thread::JoinHandle<()>>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.link.close();
        if let Some(s) = self.session.take() {
            let _ = s.join();
        }
    }
}

/// A real connection; `sender` sends to the far end, which reports `Capabilities`.
fn link() -> Link {
    let host_id = Identity::generate().unwrap();
    let client_id = Identity::generate().unwrap();
    let host_trust = Arc::new(TrustStore::ephemeral());
    let client_trust = Arc::new(TrustStore::ephemeral());
    host_trust.add(client_id.public_key(), "Laptop", TrustRole::Client, None).unwrap();
    client_trust.add(host_id.public_key(), "Desk", TrustRole::Host, None).unwrap();
    let (ctx, crx) = mpsc::channel();
    let server = HostServer::start(
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        "Desk".into(),
        Arc::new(host_id),
        host_trust,
        PairingBroker::new(Arc::new(|| {})),
        Arc::new(Stats::new()),
        Arc::new(HostSide(Mutex::new(ctx))),
    )
    .unwrap();
    let (atx, arx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let link = ClientLink::new();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port()));
    let session = {
        let (stop, link) = (stop.clone(), link.clone());
        thread::spawn(move || {
            let broker = PairingBroker::new(Arc::new(|| {}));
            let stats = Arc::new(Stats::new());
            let mut inject = InjectState::new(NoInput);
            run_session(
                SessionConfig {
                    addr,
                    expected_host: None,
                    identity: &client_id,
                    name: "Laptop",
                    trust: &client_trust,
                    broker: &broker,
                    stats: &stats,
                    screens: vec![MonitorDesc {
                        x: 0,
                        y: 0,
                        width: 800,
                        height: 600,
                        dpi: 96,
                        primary: true,
                        name: "D".into(),
                    }],
                    stop: &stop,
                },
                &mut inject,
                &link,
                &Remote(Mutex::new(atx)),
            );
        })
    };
    let c = crx.recv_timeout(Duration::from_secs(10)).expect("loopback connection");
    Link { _server: server, sender: c.sender, announced: arx, stop, link, session: Some(session) }
}

/// A hub whose policy is `caps` for "A" and "B" and nothing for anyone else (unpaired).
fn hub(caps: Arc<AtomicU32>) -> (Arc<FeatureHub>, Arc<FakeService>, Arc<FakeService>) {
    let hub = FeatureHub::new();
    hub.set_policy(move |id| if id == "A" || id == "B" { caps.load(Ordering::SeqCst) } else { 0 });
    let clip = FakeService::new(channel::CLIPBOARD);
    let files = FakeService::new(channel::FILES);
    hub.register(clip.clone());
    hub.register(files.clone());
    (hub, clip, files)
}

fn announced(l: &Link) -> u32 {
    l.announced.recv_timeout(Duration::from_secs(5)).expect("capabilities announced")
}

fn nothing_announced(l: &Link) {
    assert!(l.announced.recv_timeout(Duration::from_millis(150)).is_err(), "unexpected announcement");
}

const ALL: u32 = cap::CLIPBOARD | cap::FILES | cap::TEXT_SHARE;

#[test]
fn peer_lifecycle_survives_reconnects_and_ignores_stale_disconnects() {
    let l = link();
    let (hub, clip, files) = hub(Arc::new(AtomicU32::new(ALL)));
    hub.peer_up("A", "Laptop", 1, l.sender.clone());
    assert_eq!(announced(&l), ALL, "our capabilities are announced when the peer comes up");
    assert_eq!(clip.take(), ["up A 0"], "effective caps are unknown until the peer announces");
    files.take();

    // Reconnect: the old connection is replaced (services abort work with it first).
    hub.peer_up("A", "Laptop", 2, l.sender.clone());
    assert_eq!(announced(&l), ALL);
    assert_eq!(clip.take(), ["down A", "up A 0"]);
    assert_eq!(files.take(), ["down A", "up A 0"], "every service sees the reconnect");
    // The old connection's late disconnect must not remove the new one.
    hub.peer_down("A", 1);
    assert!(clip.take().is_empty());
    assert!(hub.peer("A").is_some() && hub.sender("A").is_some());
    // Traffic and announcements from the old connection are ignored.
    hub.on_capabilities("A", 1, ALL);
    hub.on_feature("A", 1, channel::CLIPBOARD, vec![1]);
    assert!(clip.take().is_empty());
    assert_eq!(hub.effective_caps("A"), 0);
    // The current connection works.
    hub.on_feature("A", 2, channel::CLIPBOARD, vec![2]);
    hub.on_feature("A", 2, channel::FILES, vec![3]);
    assert_eq!(clip.take(), ["msg A [2]"]);
    assert_eq!(files.take(), ["msg A [3]"]);
    // Its disconnect removes the peer.
    hub.peer_down("A", 2);
    assert_eq!(clip.take(), ["down A"]);
    assert!(hub.peer("A").is_none() && hub.sender("A").is_none());
    assert_eq!(hub.effective_caps("A"), 0);
    hub.peer_down("A", 2);
    assert!(clip.take().is_empty(), "a second disconnect is a no-op");
}

#[test]
fn effective_caps_are_local_and_remote() {
    let l = link();
    let (hub, clip, _files) = hub(Arc::new(AtomicU32::new(cap::CLIPBOARD | cap::FILES)));
    hub.peer_up("A", "Laptop", 7, l.sender.clone());
    announced(&l);
    clip.take();
    hub.on_capabilities("A", 7, cap::FILES | cap::TEXT_SHARE);
    assert_eq!(hub.effective_caps("A"), cap::FILES);
    assert_eq!(hub.local_caps("A"), cap::CLIPBOARD | cap::FILES);
    assert_eq!(hub.peers()[0].effective_caps, cap::FILES);
    assert_eq!(clip.take(), [format!("caps A {}", cap::FILES)]);
    // Repeating (or flooding) the same announcement changes nothing and calls nobody.
    for _ in 0..100 {
        hub.on_capabilities("A", 7, cap::FILES | cap::TEXT_SHARE);
    }
    assert!(clip.take().is_empty());
    // A different announcement with the same effective result is silent too.
    hub.on_capabilities("A", 7, cap::FILES);
    assert!(clip.take().is_empty());
    hub.on_capabilities("A", 7, 0);
    assert_eq!(clip.take(), ["caps A 0"]);
    assert_eq!(hub.effective_caps("A"), 0);
}

#[test]
fn refresh_caps_announces_only_changes() {
    let l = link();
    let policy = Arc::new(AtomicU32::new(ALL));
    let (hub, clip, _files) = hub(policy.clone());
    hub.peer_up("A", "Laptop", 3, l.sender.clone());
    assert_eq!(announced(&l), ALL);
    hub.on_capabilities("A", 3, ALL);
    clip.take();

    hub.refresh_caps();
    nothing_announced(&l);
    assert!(clip.take().is_empty());

    // The user turns clipboard sharing off for this device.
    policy.store(cap::FILES | cap::TEXT_SHARE, Ordering::SeqCst);
    hub.refresh_caps();
    assert_eq!(announced(&l), cap::FILES | cap::TEXT_SHARE);
    assert_eq!(clip.take(), [format!("caps A {}", cap::FILES | cap::TEXT_SHARE)]);
    assert_eq!(hub.effective_caps("A"), cap::FILES | cap::TEXT_SHARE);
    hub.refresh_caps();
    nothing_announced(&l);

    // Concurrent refreshes never leave an older evaluation in place.
    let hubs: Vec<_> = (0..8)
        .map(|i| {
            let (hub, policy) = (hub.clone(), policy.clone());
            thread::spawn(move || {
                for j in 0..50u32 {
                    policy.store(if (i + j) % 2 == 0 { ALL } else { cap::FILES }, Ordering::SeqCst);
                    hub.refresh_caps();
                }
            })
        })
        .collect();
    for h in hubs {
        h.join().unwrap();
    }
    policy.store(cap::TEXT_SHARE, Ordering::SeqCst);
    hub.refresh_caps();
    assert_eq!(hub.effective_caps("A"), cap::TEXT_SHARE);
    // The last announcement the peer received is the current policy.
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut last = None;
    while let Ok(v) =
        l.announced.recv_timeout(deadline.saturating_duration_since(Instant::now()).min(Duration::from_millis(300)))
    {
        last = Some(v);
    }
    assert_eq!(last, Some(cap::TEXT_SHARE));
}

#[test]
fn traffic_that_must_not_reach_services_is_dropped() {
    let l = link();
    let policy = Arc::new(AtomicU32::new(cap::FILES));
    let (hub, clip, files) = hub(policy.clone());

    // Unknown peer: nothing is delivered and nothing panics.
    hub.on_feature("A", 1, channel::FILES, vec![1]);
    hub.on_capabilities("A", 1, ALL);
    assert!(files.take().is_empty());

    hub.peer_up("A", "Laptop", 1, l.sender.clone());
    announced(&l);
    clip.take();
    files.take();
    // Unknown channel: ignored.
    hub.on_feature("A", 1, 99, vec![9]);
    // Clipboard is not allowed with this peer: its traffic never reaches the service,
    // whatever the peer claims.
    hub.on_capabilities("A", 1, ALL);
    hub.on_feature("A", 1, channel::CLIPBOARD, b"secret".to_vec());
    hub.on_feature("A", 1, channel::FILES, vec![4]);
    assert_eq!(clip.take(), [format!("caps A {}", cap::FILES)]);
    assert_eq!(files.take(), [format!("caps A {}", cap::FILES), "msg A [4]".to_string()]);

    // Text sharing alone opens the clipboard channel (shared text rides on it).
    policy.store(cap::TEXT_SHARE, Ordering::SeqCst);
    hub.refresh_caps();
    announced(&l);
    clip.take();
    files.take();
    hub.on_feature("A", 1, channel::CLIPBOARD, vec![5]);
    hub.on_feature("A", 1, channel::FILES, vec![6]);
    assert_eq!(clip.take(), ["msg A [5]"]);
    assert!(files.take().is_empty(), "file transfer was turned off");

    // A device that is not paired gets no capabilities at all.
    hub.peer_up("stranger", "?", 9, l.sender.clone());
    assert_eq!(announced(&l), 0);
    assert_eq!(hub.local_caps("stranger"), 0);
    hub.on_capabilities("stranger", 9, ALL);
    assert_eq!(hub.effective_caps("stranger"), 0);
    hub.on_feature("stranger", 9, channel::CLIPBOARD, vec![7]);
    hub.on_feature("stranger", 9, channel::FILES, vec![8]);
    assert!(clip.take().iter().all(|l| !l.starts_with("msg")));
    assert!(files.take().iter().all(|l| !l.starts_with("msg")));
}

#[test]
fn concurrent_lifecycle_calls_do_not_deadlock_or_leak_peers() {
    let l = link();
    let (hub, _clip, _files) = hub(Arc::new(AtomicU32::new(ALL)));
    // Reconnect storms from several "reader threads" racing with refreshes and traffic.
    let workers: Vec<_> = (0..6u64)
        .map(|t| {
            let (hub, sender) = (hub.clone(), l.sender.clone());
            thread::spawn(move || {
                let id = if t % 2 == 0 { "A" } else { "B" };
                for n in 0..200u64 {
                    let conn = t * 1000 + n;
                    hub.peer_up(id, id, conn, sender.clone());
                    hub.on_capabilities(id, conn, ALL);
                    hub.on_feature(id, conn, channel::CLIPBOARD, vec![1]);
                    if n % 3 == 0 {
                        hub.refresh_caps();
                    }
                    hub.peer_down(id, conn);
                }
            })
        })
        .collect();
    for w in workers {
        w.join().unwrap();
    }
    // Every connection that came up went down; whichever was last is gone.
    assert!(hub.peers().is_empty(), "{:?}", hub.peers());
}
