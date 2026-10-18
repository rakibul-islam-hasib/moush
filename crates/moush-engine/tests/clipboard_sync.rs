//! Clipboard sync end to end over real TCP on loopback: a host and clients, each with a
//! feature hub and a `ClipboardService` on an in-memory clipboard. Checks exact delivery
//! of large and Unicode clips, relay through the host, loop and echo prevention,
//! convergence of concurrent copies, size limits, the master switch, text sharing, and
//! that input keeps flowing while a large clip is in transit.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use moush_engine::clipboard::{
    ChangeCallback, ClipboardBackend, ClipboardConfig, ClipboardEvent, ClipboardService, LocalRead, ReadOptions, ServiceOptions,
};
use moush_engine::core::clipboard::Clip;
use moush_engine::core::crypto::Identity;
use moush_engine::core::features::{cap, ClipboardSettings};
use moush_engine::core::inject::{InjectState, Injector};
use moush_engine::core::net::client::{run_session, ClientEvents, ClientLink, SessionConfig};
use moush_engine::core::net::host::{ClientHandle, HostEvents, HostServer};
use moush_engine::core::net::{Outgoing, PeerInfo, PeerSender};
use moush_engine::core::pairing::PairingBroker;
use moush_engine::core::protocol::{key_flags, Message, MonitorDesc, MouseButton, WheelAxis};
use moush_engine::core::stats::{LatencyRing, Stats};
use moush_engine::core::trust::TrustStore;
use moush_engine::core::util::mono_us;
use moush_engine::features::FeatureHub;

// ---------------------------------------------------------------- fake clipboard

/// In-memory clipboard that behaves like the Win32 one: every write (ours or the user's)
/// bumps the sequence number and notifies the listener.
#[derive(Default)]
struct FakeClipboard {
    state: Mutex<(u32, Option<Clip>)>,
    on_change: Mutex<Option<ChangeCallback>>,
    listening: AtomicBool,
    /// Writes made by the sync service.
    writes: AtomicUsize,
}

impl FakeClipboard {
    /// The user copies something.
    fn copy(&self, clip: Clip) {
        {
            let mut s = self.state.lock().unwrap();
            s.0 += 1;
            s.1 = Some(clip);
        }
        self.notify();
    }

    fn get(&self) -> Option<Clip> {
        self.state.lock().unwrap().1.clone()
    }

    fn notify(&self) {
        if self.listening.load(Ordering::SeqCst)
            && let Some(cb) = &*self.on_change.lock().unwrap()
        {
            cb();
        }
    }
}

impl ClipboardBackend for FakeClipboard {
    fn set_listening(&self, on: bool) {
        self.listening.store(on, Ordering::SeqCst);
    }
    fn sequence(&self) -> u32 {
        self.state.lock().unwrap().0
    }
    fn read(&self, opts: &ReadOptions) -> Result<LocalRead, String> {
        let s = self.state.lock().unwrap();
        let clip = s.1.clone().unwrap_or_default().filtered(opts.filter);
        let oversized = clip.size() > opts.max_bytes;
        Ok(LocalRead::Clip { seq: s.0, clip: if oversized { Clip::default() } else { clip }, oversized })
    }
    fn write(&self, clip: Arc<Clip>) -> Result<u32, String> {
        let seq = {
            let mut s = self.state.lock().unwrap();
            s.0 += 1;
            s.1 = Some((*clip).clone());
            s.0
        };
        self.writes.fetch_add(1, Ordering::SeqCst);
        self.notify();
        Ok(seq)
    }
}

// ---------------------------------------------------------------- nodes

struct Node {
    id: String,
    name: String,
    hub: Arc<FeatureHub>,
    clipboard: Arc<FakeClipboard>,
    svc: Arc<ClipboardService>,
    settings: Arc<Mutex<ClipboardSettings>>,
    events: Receiver<ClipboardEvent>,
}

fn node(identity: &Identity, name: &str) -> Node {
    let hub = FeatureHub::new();
    hub.set_policy(|_| cap::CLIPBOARD | cap::TEXT_SHARE);
    let settings = Arc::new(Mutex::new(ClipboardSettings::default()));
    let clipboard = Arc::new(FakeClipboard::default());
    let (etx, erx) = mpsc::channel();
    let etx = Mutex::new(etx);
    let s2 = settings.clone();
    let name = name.to_owned();
    let device_name = name.clone();
    let fake = clipboard.clone();
    let svc = ClipboardService::start(
        &hub,
        ServiceOptions {
            device_id: identity.device_id(),
            settings: Box::new(move || ClipboardConfig {
                clipboard: s2.lock().unwrap().clone(),
                window_bytes: 128 * 1024,
                device_name: name.clone(),
            }),
            events: Box::new(move |e| {
                if e != ClipboardEvent::Changed {
                    let _ = etx.lock().unwrap().send(e);
                }
            }),
        },
        move |cb| {
            *fake.on_change.lock().unwrap() = Some(cb);
            Ok(fake as Arc<dyn ClipboardBackend>)
        },
    )
    .unwrap();
    Node { id: identity.device_id(), name: device_name, hub, clipboard, svc, settings, events: erx }
}

/// Forwards host connection events to the host's hub.
struct HostAdapter {
    hub: Arc<FeatureHub>,
    connected: Mutex<Sender<ClientHandle>>,
}

impl HostEvents for HostAdapter {
    fn client_connected(&self, c: ClientHandle) {
        self.hub.peer_up(&c.id, &c.name, c.conn_no, c.sender.clone());
        let _ = self.connected.lock().unwrap().send(c);
    }
    fn client_screens(&self, _: &str, _: u64, _: Vec<MonitorDesc>) {}
    fn client_disconnected(&self, id: &str, conn_no: u64, _: &str) {
        self.hub.peer_down(id, conn_no);
    }
    fn client_capabilities(&self, id: &str, conn_no: u64, features: u32) {
        self.hub.on_capabilities(id, conn_no, features);
    }
    fn client_feature(&self, id: &str, conn_no: u64, channel: u8, body: Vec<u8>) {
        self.hub.on_feature(id, conn_no, channel, body);
    }
}

/// Forwards client session events to the client's hub.
struct ClientAdapter {
    hub: Arc<FeatureHub>,
    host: Mutex<Option<String>>,
}

impl ClientEvents for ClientAdapter {
    fn connected(&self, host: &PeerInfo, sender: &PeerSender) {
        *self.host.lock().unwrap() = Some(host.id.clone());
        self.hub.peer_up(&host.id, &host.name, 1, sender.clone());
    }
    fn active_changed(&self, _: bool) {}
    fn disconnected(&self) {
        if let Some(id) = self.host.lock().unwrap().take() {
            self.hub.peer_down(&id, 1);
        }
    }
    fn capabilities(&self, features: u32) {
        if let Some(id) = self.host.lock().unwrap().clone() {
            self.hub.on_capabilities(&id, 1, features);
        }
    }
    fn feature(&self, channel: u8, body: Vec<u8>) {
        let id = self.host.lock().unwrap().clone();
        if let Some(id) = id {
            self.hub.on_feature(&id, 1, channel, body);
        }
    }
}

/// Records injected key events with the time they were injected.
struct Recorder(Sender<u64>);

impl Injector for Recorder {
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
        self.0.send(mono_us()).is_ok()
    }
}

/// Broker that approves every pairing prompt.
fn auto_broker() -> Arc<PairingBroker> {
    let slot: Arc<Mutex<Option<Arc<PairingBroker>>>> = Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    let b = PairingBroker::new(Arc::new(move || {
        let slot = slot2.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            if let Some(b) = slot.lock().unwrap().clone() {
                for r in b.list() {
                    if r.needs_my_approval {
                        b.respond(&r.request_id, true);
                    }
                }
            }
        });
    }));
    *slot.lock().unwrap() = Some(b.clone());
    b
}

struct Client {
    node: Node,
    handle: ClientHandle,
    keys: Receiver<u64>,
    stop: Arc<AtomicBool>,
    link: Arc<ClientLink>,
    session: thread::JoinHandle<()>,
}

struct Rig {
    host: Node,
    _server: HostServer,
    clients: Vec<Client>,
}

static NEXT: AtomicU64 = AtomicU64::new(0);

fn rig(n_clients: usize) -> Rig {
    let host_identity = Arc::new(Identity::generate().unwrap());
    let host = node(&host_identity, "Desk PC");
    let (ctx, crx) = mpsc::channel();
    let server = HostServer::start(
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        "Desk PC".into(),
        host_identity,
        Arc::new(TrustStore::ephemeral()),
        auto_broker(),
        Arc::new(Stats::new()),
        Arc::new(HostAdapter { hub: host.hub.clone(), connected: Mutex::new(ctx) }),
    )
    .unwrap();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, server.port()));
    let mut clients = Vec::new();
    for i in 0..n_clients {
        let identity = Identity::generate().unwrap();
        let name = format!("Laptop {}", NEXT.fetch_add(1, Ordering::Relaxed));
        let node = node(&identity, &name);
        let adapter = Arc::new(ClientAdapter { hub: node.hub.clone(), host: Mutex::new(None) });
        let stop = Arc::new(AtomicBool::new(false));
        let link = ClientLink::new();
        let (ktx, krx) = mpsc::channel();
        let session = {
            let (stop, link) = (stop.clone(), link.clone());
            thread::spawn(move || {
                let trust = TrustStore::ephemeral();
                let broker = auto_broker();
                let stats = Arc::new(Stats::new());
                let mut inject = InjectState::new(Recorder(ktx));
                let screens = vec![MonitorDesc { x: 0, y: 0, width: 1920, height: 1080, dpi: 96, primary: true, name: "M".into() }];
                run_session(
                    SessionConfig {
                        addr,
                        expected_host: None,
                        identity: &identity,
                        name: &name,
                        trust: &trust,
                        broker: &broker,
                        stats: &stats,
                        screens,
                        stop: &stop,
                    },
                    &mut inject,
                    &link,
                    adapter.as_ref(),
                );
            })
        };
        let handle = crx.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| panic!("client {i} connects"));
        clients.push(Client { node, handle, keys: krx, stop, link, session });
    }
    let rig = Rig { host, _server: server, clients };
    // Both sides announce the clipboard feature before the tests start copying.
    wait("capabilities", || {
        rig.clients.iter().all(|c| {
            rig.host.hub.effective_caps(&c.node.id) & cap::CLIPBOARD != 0 && c.node.hub.effective_caps(&rig.host.id) & cap::CLIPBOARD != 0
        })
    });
    // Let the services start listening (first housekeeping tick).
    thread::sleep(Duration::from_millis(400));
    rig
}

impl Rig {
    fn stop(self) {
        for c in self.clients {
            c.stop.store(true, Ordering::SeqCst);
            c.link.close();
            c.session.join().unwrap();
        }
    }
}

fn wait(what: &str, mut cond: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::sleep(Duration::from_millis(5));
    }
}

fn has(n: &Node, clip: &Clip) -> bool {
    n.clipboard.get().as_ref() == Some(clip)
}

/// A bitmap of about `bytes` bytes with noisy (poorly compressible) pixels.
fn big_bitmap(bytes: usize) -> Clip {
    let w = 1024usize;
    let h = bytes / (w * 4);
    let mut d = vec![0u8; 40 + w * h * 4];
    d[0..4].copy_from_slice(&40u32.to_le_bytes());
    d[4..8].copy_from_slice(&(w as i32).to_le_bytes());
    d[8..12].copy_from_slice(&(h as i32).to_le_bytes());
    d[12..14].copy_from_slice(&1u16.to_le_bytes());
    d[14..16].copy_from_slice(&32u16.to_le_bytes());
    let mut x = 0x9E37_79B9u32;
    for (i, b) in d[40..].iter_mut().enumerate() {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        *b = if i % 4 == 3 { 0xFF } else { (x >> 24) as u8 };
    }
    Clip { dib: Some(d), ..Default::default() }
}

const UNICODE: &str = "Grüße \u{1F600}\u{1F468}\u{200D}\u{1F469}\u{200D}\u{1F467}\r\n\u{4E2D}\u{6587}\u{6F22}\u{5B57} \u{65E5}\u{672C}\u{8A9E}\r\n\tindented\nunix line\r\n\r\nlast line without newline";

#[test]
fn large_and_unicode_clips_arrive_intact_while_input_flows() {
    let rig = rig(1);
    let client = &rig.clients[0];

    // Client -> host: Unicode, multiline, CRLF and LF mixed.
    let text = Clip { text: Some(UNICODE.into()), html: Some(b"Version:0.9\r\n<p>Gr\xc3\xbc\xc3\x9fe</p>".to_vec()), ..Default::default() };
    client.node.clipboard.copy(text.clone());
    wait("text on the host", || has(&rig.host, &text));
    assert!(matches!(rig.host.events.recv_timeout(Duration::from_secs(1)), Ok(ClipboardEvent::ClipReceived { .. })));

    // Host -> client: a 5 MB bitmap while the host keeps sending input to the client.
    client.handle.outbox.send(Outgoing::now(Message::Enter { x: 0, y: 0 })).unwrap();
    let image = big_bitmap(5 * 1024 * 1024);
    let started = Instant::now();
    rig.host.clipboard.copy(image.clone());
    let mut ring = LatencyRing::default();
    let mut i = 0u32;
    while !has(&client.node, &image) {
        assert!(started.elapsed() < Duration::from_secs(20), "image did not arrive");
        let t = mono_us();
        let flags = if i & 1 == 0 { key_flags::DOWN } else { 0 };
        client.handle.outbox.send(Outgoing { msg: Message::Key { vk: 0x41, scan: 0x1e, flags }, t_us: t }).unwrap();
        let at = client.keys.recv_timeout(Duration::from_secs(5)).expect("key injected");
        ring.record(at.saturating_sub(t) as u32);
        i += 1;
        thread::sleep(Duration::from_micros(500));
    }
    let secs = started.elapsed().as_secs_f64();
    let s = ring.summary();
    println!(
        "5 MB clip in {:.0} ms ({:.0} MB/s); {} keys meanwhile: p50 {:.0} us, p95 {:.0} us, max {:.0} us",
        secs * 1000.0,
        5.0 / secs,
        s.count,
        s.p50,
        s.p95,
        s.max
    );
    assert!(s.count == 0 || s.p50 < 20_000.0, "input keeps flowing during a clip transfer");

    // No echo, no loop: each side wrote exactly once, and nothing else travels.
    thread::sleep(Duration::from_millis(500));
    assert_eq!(rig.host.clipboard.writes.load(Ordering::SeqCst), 1);
    assert_eq!(client.node.clipboard.writes.load(Ordering::SeqCst), 1);
    assert!(has(&rig.host, &image), "the host keeps its own copy");

    // History records both directions.
    let h = rig.host.svc.history();
    assert_eq!(h.iter().map(|e| (e.kind.as_str(), e.origin)).collect::<Vec<_>>(), [("image", "sent"), ("rich", "received")]);
    assert_eq!(h[0].preview, format!("Image 1024 x {}", 5 * 1024 * 1024 / 4096));
    assert_eq!(client.node.svc.status().last_direction, Some("received"));

    // Copying a history entry puts it back and syncs it.
    rig.host.svc.copy_entry(&h[1].id).unwrap();
    wait("history entry synced", || has(&client.node, &text));
    rig.stop();
}

#[test]
fn host_relays_between_clients_without_echo_and_concurrent_copies_converge() {
    let rig = rig(2);
    let (a, b) = (&rig.clients[0], &rig.clients[1]);

    let clip = Clip::text("from A \u{2192} everyone");
    a.node.clipboard.copy(clip.clone());
    wait("relay to B", || has(&b.node, &clip) && has(&rig.host, &clip));
    thread::sleep(Duration::from_millis(300));
    assert_eq!(a.node.clipboard.writes.load(Ordering::SeqCst), 0, "never echoed to its origin");
    assert_eq!(b.node.clipboard.writes.load(Ordering::SeqCst), 1);
    let h = b.node.svc.history();
    assert_eq!(h[0].source.as_deref(), Some(a.node.name.as_str()), "origin name travels with the relay");

    // Copies on A, B and the host at the same moment: everyone ends with the same clip.
    for round in 0..5 {
        let ca = Clip::text(format!("A {round}"));
        let cb = Clip::text(format!("B {round}"));
        let ch = Clip::text(format!("H {round}"));
        a.node.clipboard.copy(ca.clone());
        b.node.clipboard.copy(cb.clone());
        rig.host.clipboard.copy(ch.clone());
        wait("convergence", || {
            let x = rig.host.clipboard.get();
            x.is_some() && a.node.clipboard.get() == x && b.node.clipboard.get() == x && [&ca, &cb, &ch].iter().any(|c| x.as_ref() == Some(*c))
        });
    }
    // Quiet afterwards: no ping-pong.
    let writes = |n: &Node| n.clipboard.writes.load(Ordering::SeqCst);
    let before = (writes(&rig.host), writes(&a.node), writes(&b.node));
    thread::sleep(Duration::from_millis(600));
    assert_eq!(before, (writes(&rig.host), writes(&a.node), writes(&b.node)), "sync is idle once converged");
    rig.stop();
}

#[test]
fn limits_switches_and_text_sharing() {
    let rig = rig(1);
    let client = &rig.clients[0];

    // The receiver enforces its own size limit.
    client.node.settings.lock().unwrap().max_size_kb = 64;
    let big = Clip::text("x".repeat(200 * 1024));
    rig.host.clipboard.copy(big.clone());
    thread::sleep(Duration::from_millis(800));
    assert_eq!(client.node.clipboard.writes.load(Ordering::SeqCst), 0, "oversized clip rejected");
    let small = Clip::text("small");
    rig.host.clipboard.copy(small.clone());
    wait("small clip", || has(&client.node, &small));

    // Per-format toggles: rich text stripped by the receiver, plain text kept.
    client.node.settings.lock().unwrap().rich_text = false;
    let rich = Clip { text: Some("bold".into()), html: Some(b"<b>bold</b>".to_vec()), rtf: Some(br"{\rtf1 \b bold}".to_vec()), ..Default::default() };
    rig.host.clipboard.copy(rich);
    wait("plain part of a rich clip", || client.node.clipboard.get() == Some(Clip::text("bold")));

    // The master switch stops sending and receiving at once.
    rig.host.settings.lock().unwrap().enabled = false;
    let off = Clip::text("not synced");
    rig.host.clipboard.copy(off.clone());
    client.node.clipboard.copy(Clip::text("not synced either"));
    thread::sleep(Duration::from_millis(800));
    assert!(!has(&client.node, &off));
    assert!(!has(&rig.host, &Clip::text("not synced either")));
    rig.host.settings.lock().unwrap().enabled = true;
    thread::sleep(Duration::from_millis(400));

    // History off clears it.
    assert!(!rig.host.svc.history().is_empty());
    rig.host.settings.lock().unwrap().history = false;
    wait("history cleared", || rig.host.svc.status().history_count == 0);

    // Shared text: arrives in the list, never on the clipboard; long texts are chunked.
    let before = client.node.clipboard.get();
    let long = format!("{}\r\nend", UNICODE.repeat(400));
    assert!(long.len() > 32 * 1024);
    rig.host.svc.share_text(None, &long).unwrap();
    rig.host.svc.share_text(Some(&client.node.id), "  https://example.com/path?q=1 ").unwrap();
    wait("shared texts", || client.node.svc.shared_texts().len() == 2);
    let shared = client.node.svc.shared_texts();
    assert!(shared[0].is_link && shared[0].incoming);
    assert!(!shared[1].is_link);
    assert_eq!(shared[1].chars as usize, long.chars().count());
    assert_eq!(client.node.clipboard.get(), before, "shared text is never placed on the clipboard by itself");
    assert_eq!(client.node.svc.shared_link(&shared[0].id).unwrap(), "https://example.com/path?q=1");
    assert!(client.node.svc.shared_link(&shared[1].id).is_err());
    client.node.svc.copy_shared_text(&shared[1].id).unwrap();
    assert_eq!(client.node.clipboard.get(), Some(Clip::text(long.clone())));
    wait("copied shared text syncs like a copy", || rig.host.clipboard.get() == Some(Clip::text(long.clone())));
    assert!(rig.host.svc.share_text(None, "").is_err());
    assert!(rig.host.svc.share_text(Some("nobody"), "x").is_err());
    let mut kinds = Vec::new();
    while let Ok(e) = client.node.events.try_recv() {
        kinds.push(e);
    }
    assert!(kinds.iter().any(|e| matches!(e, ClipboardEvent::TextShared { is_link: true, .. })));

    // A disconnect mid-transfer is cleaned up; the next connection works.
    rig.stop();
}

#[test]
fn disconnect_mid_transfer_cleans_up() {
    let rig = rig(1);
    for n in [&rig.host, &rig.clients[0].node] {
        n.settings.lock().unwrap().max_size_kb = 64 * 1024;
    }
    rig.host.clipboard.copy(big_bitmap(40 * 1024 * 1024));
    wait("transfer started", || rig.host.svc.sending() == 1);
    let host = rig.host;
    for c in rig.clients {
        c.stop.store(true, Ordering::SeqCst);
        c.link.close();
        c.session.join().unwrap();
        assert_eq!(c.node.clipboard.writes.load(Ordering::SeqCst), 0, "a partial clip is never applied");
    }
    wait("sender cleaned up", || host.svc.sending() == 0);
    assert!(host.hub.peers().is_empty());
}
