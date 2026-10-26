//! File transfer between two `TransferService`s over a real loopback connection (host
//! server + client session with Noise encryption and pairing), each behind its own
//! `FeatureHub`, exactly as the app wires them.
//!
//! Sizes: `MOUSH_TRANSFER_TEST_MB` sets the large file (default 200 MB in release builds,
//! 48 MB in debug builds).
//! Throughput and input latency: `cargo test --release -p moush-engine --test
//! transfer_loopback -- --ignored --nocapture`.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use moush_engine::core::crypto::Identity;
use moush_engine::core::features::{TransferSettings, cap, channel};
use moush_engine::core::inject::{InjectState, Injector};
use moush_engine::core::net::client::{ClientEvents, ClientLink, SessionConfig, SessionEnd, run_session};
use moush_engine::core::net::host::{ClientHandle, HostEvents, HostServer};
use moush_engine::core::net::{Outgoing, PeerInfo, PeerSender};
use moush_engine::core::pairing::PairingBroker;
use moush_engine::core::protocol::{Message, MonitorDesc, MouseButton, WheelAxis, key_flags};
use moush_engine::core::stats::{LatencyRing, Stats};
use moush_engine::core::transfer::{self as tr, FileEntry, Msg};
use moush_engine::core::trust::TrustStore;
use moush_engine::core::util::mono_us;
use moush_engine::features::FeatureHub;
use moush_engine::transfer::{Event, State, TransferInfo, TransferService};
use moush_engine::win::system::ProcessMonitor;

/// The tests share the machine's disk and CPU; run them one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

const WAIT: Duration = Duration::from_secs(120);

/// Formats every log record (the app always runs with a logger, so log arguments must be
/// evaluated in tests too) and keeps warnings for the test output.
struct TestLogger;

impl log::Log for TestLogger {
    fn enabled(&self, _: &log::Metadata) -> bool {
        true
    }
    fn log(&self, record: &log::Record) {
        let line = format!("{}", record.args());
        if record.level() <= log::Level::Warn {
            eprintln!("[{}] {line}", record.level());
        }
    }
    fn flush(&self) {}
}

fn init_logging() {
    static LOGGER: TestLogger = TestLogger;
    if log::set_logger(&LOGGER).is_ok() {
        log::set_max_level(log::LevelFilter::Trace);
    }
}

// ---------------------------------------------------------------- rig

struct Side {
    hub: Arc<FeatureHub>,
    svc: Arc<TransferService>,
    settings: Arc<Mutex<TransferSettings>>,
    allow: Arc<AtomicBool>,
    events: Receiver<Event>,
    dir: PathBuf,
}

impl Side {
    fn new(dir: PathBuf) -> Self {
        let hub = FeatureHub::new();
        let allow = Arc::new(AtomicBool::new(true));
        let a = allow.clone();
        hub.set_policy(move |_| if a.load(Ordering::SeqCst) { cap::FILES } else { 0 });
        let svc = TransferService::new(dir.clone());
        let settings = Arc::new(Mutex::new(TransferSettings { ask_before_receiving: false, ..Default::default() }));
        let (tx, events) = mpsc::channel();
        let tx = Mutex::new(tx);
        let s = settings.clone();
        svc.set_handlers(
            move || s.lock().unwrap().clone(),
            move |e| {
                if !matches!(e, Event::Changed) {
                    let _ = tx.lock().unwrap().send(e);
                }
            },
        );
        svc.attach(&hub);
        Self { hub, svc, settings, allow, events, dir }
    }

    fn set(&self, f: impl FnOnce(&mut TransferSettings)) {
        f(&mut self.settings.lock().unwrap());
    }

    fn peer(&self) -> String {
        self.hub.peers().first().expect("connected").id.clone()
    }

    fn info(&self, id: &str) -> TransferInfo {
        self.svc.get(id).expect("transfer exists")
    }

    /// The incoming transfer (there is only one at a time in these tests).
    fn incoming(&self) -> Option<TransferInfo> {
        self.svc.list().into_iter().find(|t| !t.outgoing)
    }

    fn parts(&self) -> Vec<PathBuf> {
        fs::read_dir(&self.dir)
            .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.to_string_lossy().ends_with(tr::PART_SUFFIX)).collect())
            .unwrap_or_default()
    }
}

struct HostSide {
    hub: Arc<FeatureHub>,
    handles: Mutex<Sender<ClientHandle>>,
}

impl HostEvents for HostSide {
    fn client_connected(&self, c: ClientHandle) {
        self.hub.peer_up(&c.id, &c.name, c.conn_no, c.sender.clone());
        let _ = self.handles.lock().unwrap().send(c);
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

struct ClientSide {
    hub: Arc<FeatureHub>,
    session: Mutex<Option<(String, u64)>>,
    sessions: AtomicU32,
}

impl ClientEvents for ClientSide {
    fn connected(&self, host: &PeerInfo, sender: &PeerSender) {
        let n = self.sessions.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        *self.session.lock().unwrap() = Some((host.id.clone(), n));
        self.hub.peer_up(&host.id, &host.name, n, sender.clone());
    }
    fn active_changed(&self, _: bool) {}
    fn disconnected(&self) {
        if let Some((id, n)) = self.session.lock().unwrap().take() {
            self.hub.peer_down(&id, n);
        }
    }
    fn capabilities(&self, features: u32) {
        if let Some((id, n)) = self.session.lock().unwrap().clone() {
            self.hub.on_capabilities(&id, n, features);
        }
    }
    fn feature(&self, channel: u8, body: Vec<u8>) {
        let s = self.session.lock().unwrap().clone();
        if let Some((id, n)) = s {
            self.hub.on_feature(&id, n, channel, body);
        }
    }
}

/// Records the injection time of key events (input latency during transfers).
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

struct Rig {
    _host_server: HostServer,
    host_port: u16,
    host: Side,
    client: Side,
    client_events: Arc<ClientSide>,
    client_id: Identity,
    client_trust: Arc<TrustStore>,
    handles: Receiver<ClientHandle>,
    host_stats: Arc<Stats>,
    session: Option<(Arc<AtomicBool>, Arc<ClientLink>, thread::JoinHandle<SessionEnd>)>,
    injected: Option<Receiver<u64>>,
    root: PathBuf,
}

fn temp_root(tag: &str) -> PathBuf {
    static N: AtomicU32 = AtomicU32::new(0);
    let d = std::env::temp_dir().join(format!(
        "moush-xfer-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    d
}

impl Rig {
    fn new(tag: &str) -> Self {
        init_logging();
        let root = temp_root(tag);
        let host = Side::new(root.join("host-downloads"));
        let client = Side::new(root.join("client-downloads"));
        let (htx, handles) = mpsc::channel();
        let host_stats = Arc::new(Stats::new());
        let server = HostServer::start(
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
            "Desk PC".into(),
            Arc::new(Identity::generate().unwrap()),
            Arc::new(TrustStore::ephemeral()),
            auto_broker(),
            host_stats.clone(),
            Arc::new(HostSide { hub: host.hub.clone(), handles: Mutex::new(htx) }),
        )
        .unwrap();
        let mut rig = Rig {
            host_port: server.port(),
            _host_server: server,
            client_events: Arc::new(ClientSide {
                hub: client.hub.clone(),
                session: Mutex::new(None),
                sessions: AtomicU32::new(0),
            }),
            host,
            client,
            client_id: Identity::generate().unwrap(),
            client_trust: Arc::new(TrustStore::ephemeral()),
            handles,
            host_stats,
            session: None,
            injected: None,
            root,
        };
        rig.connect();
        rig
    }

    /// Starts a client session and waits until both sides allow files.
    fn connect(&mut self) -> ClientHandle {
        let stop = Arc::new(AtomicBool::new(false));
        let link = ClientLink::new();
        let (itx, irx) = mpsc::channel();
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, self.host_port));
        let (id, trust, events) = (self.client_id.clone(), self.client_trust.clone(), self.client_events.clone());
        let (stop2, link2) = (stop.clone(), link.clone());
        let h = thread::spawn(move || {
            let broker = auto_broker();
            let stats = Arc::new(Stats::new());
            let mut inject = InjectState::new(Recorder(itx));
            run_session(
                SessionConfig {
                    addr,
                    expected_host: None,
                    identity: &id,
                    name: "Laptop",
                    trust: &trust,
                    broker: &broker,
                    stats: &stats,
                    screens: vec![MonitorDesc {
                        x: 0,
                        y: 0,
                        width: 1920,
                        height: 1080,
                        dpi: 96,
                        primary: true,
                        name: "M".into(),
                    }],
                    stop: &stop2,
                },
                &mut inject,
                &link2,
                events.as_ref(),
            )
        });
        self.session = Some((stop, link, h));
        self.injected = Some(irx);
        let handle = self.handles.recv_timeout(Duration::from_secs(20)).expect("client connected");
        assert!(
            wait_until(Duration::from_secs(10), || {
                let (h, c) = (self.host.hub.peers(), self.client.hub.peers());
                h.len() == 1
                    && c.len() == 1
                    && h[0].effective_caps & cap::FILES != 0
                    && c[0].effective_caps & cap::FILES != 0
            }),
            "capabilities exchanged"
        );
        handle
    }

    fn disconnect(&mut self) {
        if let Some((stop, link, h)) = self.session.take() {
            stop.store(true, Ordering::SeqCst);
            link.close();
            h.join().unwrap();
        }
        assert!(wait_until(Duration::from_secs(10), || self.host.hub.peers().is_empty()
            && self.client.hub.peers().is_empty()));
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        if let Some((stop, link, h)) = self.session.take() {
            stop.store(true, Ordering::SeqCst);
            link.close();
            let _ = h.join();
        }
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn wait_until(timeout: Duration, mut f: impl FnMut() -> bool) -> bool {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if f() {
            return true;
        }
        thread::sleep(Duration::from_millis(10));
    }
    f()
}

fn wait_state(side: &Side, id: &str, state: State) -> TransferInfo {
    let mut last = None;
    let ok = wait_until(WAIT, || {
        let t = side.svc.get(id);
        let done = t.as_ref().is_some_and(|t| t.state == state);
        last = t;
        done
    });
    let t = last.expect("transfer exists");
    assert!(ok, "expected {state:?}, got {:?} ({:?})", t.state, t.error);
    t
}

fn incoming_state(side: &Side, state: State) -> TransferInfo {
    let mut last = None;
    let ok = wait_until(WAIT, || {
        last = side.incoming();
        last.as_ref().is_some_and(|t| t.state == state)
    });
    assert!(ok, "incoming transfer should be {state:?}, is {:?}", last.map(|t| (t.state, t.error)));
    last.unwrap()
}

/// Writes `size` bytes of a deterministic pattern seeded by `seed`, 1 MB at a time.
fn make_file(path: &Path, size: u64, seed: u64) {
    let mut f = File::create(path).unwrap();
    let mut x = seed | 1;
    let mut buf = vec![0u8; 1 << 20];
    let mut left = size;
    while left > 0 {
        let n = left.min(buf.len() as u64) as usize;
        for c in buf[..n].chunks_mut(8) {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            c.copy_from_slice(&x.to_le_bytes()[..c.len()]);
        }
        f.write_all(&buf[..n]).unwrap();
        left -= n as u64;
    }
}

fn file_hash(path: &Path) -> tr::Hash {
    let mut f = File::open(path).unwrap();
    let len = f.metadata().unwrap().len();
    let h = tr::hash_prefix(&mut f, len, || false).unwrap();
    let mut rest = Vec::new();
    f.read_to_end(&mut rest).unwrap();
    assert!(rest.is_empty());
    tr::finish_hash(&h)
}

/// 200 MB in release builds; 48 MB in debug builds, where encryption runs ~20x slower.
fn large_mb() -> u64 {
    let default = if cfg!(debug_assertions) { 48 } else { 200 };
    std::env::var("MOUSH_TRANSFER_TEST_MB").ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

// ---------------------------------------------------------------- tests

#[test]
fn sends_several_files_including_a_large_one_with_verified_hashes() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rig = Rig::new("many");
    let src = rig.root.join("src");
    fs::create_dir_all(&src).unwrap();
    let big = large_mb() << 20;
    let specs: Vec<(&str, u64)> = vec![
        ("empty.txt", 0),
        ("one.bin", 1),
        ("exact-chunk.bin", tr::CHUNK as u64),
        ("chunk-plus-one.bin", tr::CHUNK as u64 + 1),
        ("Prüfung 日本語.pdf", 300_000),
        ("large.iso", big),
    ];
    let mut paths = Vec::new();
    for (i, (name, size)) in specs.iter().enumerate() {
        let p = src.join(name);
        make_file(&p, *size, i as u64 + 7);
        paths.push(p);
    }
    // An existing file with the same name: the default policy keeps both.
    fs::create_dir_all(&rig.host.dir).unwrap();
    fs::write(rig.host.dir.join("one.bin"), b"keep me").unwrap();

    let mut mon = ProcessMonitor::new();
    let base_mb = mon.sample().memory_mb;
    let sent_before = Stats::get(&rig.host_stats.bytes_received);
    let started = Instant::now();
    let id = rig.client.svc.send(&rig.client.peer(), &paths).unwrap();
    let mut peak_mb = base_mb;
    let ok = wait_until(WAIT * 4, || {
        peak_mb = peak_mb.max(mon.sample().memory_mb);
        rig.client.info(&id).state == State::Done
    });
    assert!(ok, "send finished: {:?}", rig.client.info(&id));
    let secs = started.elapsed().as_secs_f64();
    let out = rig.client.info(&id);
    assert_eq!(out.done_bytes, out.total_bytes);
    let inc = incoming_state(&rig.host, State::Done);
    assert_eq!(inc.done_bytes, inc.total_bytes);
    assert!(rig.host.parts().is_empty(), "no partial files are left behind");

    // Every file arrived intact; the clash was renamed and the original kept.
    for (name, _) in &specs {
        let saved = if *name == "one.bin" { "one (1).bin".to_string() } else { name.to_string() };
        assert_eq!(file_hash(&rig.host.dir.join(&saved)), file_hash(&src.join(name)), "{saved}");
        let zone = fs::read_to_string(format!("{}:Zone.Identifier", rig.host.dir.join(&saved).display())).unwrap();
        assert!(zone.contains("ZoneId=3"), "{saved} carries the Mark of the Web");
    }
    assert_eq!(fs::read(rig.host.dir.join("one.bin")).unwrap(), b"keep me");
    assert!(inc.files.iter().any(|f| f.name == "one (1).bin"), "the list shows the saved name");

    // Streaming: memory does not grow with the file size, and the wire carried the data
    // once in chunks (plus encryption and framing overhead).
    let wire = Stats::get(&rig.host_stats.bytes_received) - sent_before;
    let total: u64 = specs.iter().map(|s| s.1).sum();
    let chunks = specs.iter().map(|s| s.1.div_ceil(tr::CHUNK as u64)).sum::<u64>();
    println!(
        "sent {} files, {:.0} MB in {secs:.1} s ({:.0} MB/s), {chunks} chunks, {:.1} MB on the wire, working set {base_mb:.0} -> peak {peak_mb:.0} MB",
        specs.len(),
        total as f64 / 1e6,
        total as f64 / 1e6 / secs,
        wire as f64 / 1e6
    );
    assert!(wire >= total && wire < total + total / 50 + (1 << 20), "data crossed the wire once");
    assert!(peak_mb - base_mb < 64.0, "memory stays bounded while streaming {} MB", big >> 20);

    // Events for the app: an automatic accept and completions on both sides.
    let host_events: Vec<Event> = rig.host.events.try_iter().collect();
    assert!(host_events.iter().any(|e| matches!(e, Event::Incoming { needs_approval: false, files: 6, .. })));
    assert!(host_events.iter().any(|e| matches!(e, Event::Finished(t) if t.state == State::Done)));
}

#[test]
fn cancelling_mid_transfer_from_either_side_removes_partial_files() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rig = Rig::new("cancel");
    let src = rig.root.join("big.bin");
    make_file(&src, 96 << 20, 3);

    for receiver_cancels in [false, true] {
        let id = rig.client.svc.send(&rig.client.peer(), std::slice::from_ref(&src)).unwrap();
        assert!(wait_until(WAIT, || rig.host.incoming().is_some_and(|t| t.done_bytes > 4 << 20)), "data is flowing");
        assert!(!rig.host.parts().is_empty(), "receiving into a partial file");
        let inc = rig.host.incoming().unwrap();
        if receiver_cancels {
            rig.host.svc.cancel(&inc.id).unwrap();
        } else {
            rig.client.svc.cancel(&id).unwrap();
        }
        let out = wait_state(&rig.client, &id, State::Cancelled);
        let inc = wait_state(&rig.host, &inc.id, State::Cancelled);
        assert!(wait_until(Duration::from_secs(10), || rig.host.parts().is_empty()), "partial file removed");
        assert!(!rig.host.dir.join("big.bin").exists());
        // The side that did not cancel says who did.
        let other = if receiver_cancels { &out } else { &inc };
        assert!(other.error.as_deref().unwrap_or("").contains("Cancelled on"), "{other:?}");
        rig.host.svc.clear_finished();
        rig.client.svc.clear_finished();
        assert!(wait_until(Duration::from_secs(5), || {
            rig.client.svc.clear_finished();
            rig.client.svc.list().is_empty()
        }));
    }
}

#[test]
fn disconnect_mid_transfer_then_retry_resumes_from_the_partial_file() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rig = Rig::new("resume");
    let src = rig.root.join("video.mp4");
    let size = 64u64 << 20;
    make_file(&src, size, 11);

    let id = rig.client.svc.send(&rig.client.peer(), std::slice::from_ref(&src)).unwrap();
    assert!(wait_until(WAIT, || rig.host.incoming().is_some_and(|t| t.done_bytes > 16 << 20)));
    rig.disconnect();
    let out = wait_state(&rig.client, &id, State::Failed);
    assert!(out.error.as_deref().unwrap().contains("Lost the connection"), "{out:?}");
    assert!(
        wait_until(Duration::from_secs(10), || rig.client.info(&id).can_retry),
        "retryable once the worker stopped"
    );
    let inc = incoming_state(&rig.host, State::Failed);
    assert!(!inc.can_retry, "only the sender retries");
    let parts = rig.host.parts();
    assert_eq!(parts.len(), 1, "partial file kept for resuming");
    let partial = fs::metadata(&parts[0]).unwrap().len();
    assert!(partial > 16 << 20 && partial < size, "partial length {partial}");
    assert!(rig.client.svc.retry(&id).is_err(), "cannot retry while disconnected");

    rig.connect();
    let before = Stats::get(&rig.host_stats.bytes_received);
    rig.client.svc.retry(&id).unwrap();
    wait_state(&rig.client, &id, State::Done);
    incoming_state(&rig.host, State::Done);
    let resent = Stats::get(&rig.host_stats.bytes_received) - before;
    println!("resume: {partial} of {size} bytes were already there; {resent} bytes crossed the wire on retry");
    assert!(resent < size - partial + (2 << 20), "only the missing part was sent again");
    assert_eq!(file_hash(&rig.host.dir.join("video.mp4")), file_hash(&src));
    assert!(rig.host.parts().is_empty());
    assert_eq!(rig.host.svc.list().iter().filter(|t| !t.outgoing).count(), 1, "the retry replaced the failed record");
}

#[test]
fn resume_restarts_a_file_that_changed_since_the_partial_copy() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rig = Rig::new("changed");
    let src = rig.root.join("data.bin");
    let size = 48u64 << 20;
    make_file(&src, size, 5);

    let id = rig.client.svc.send(&rig.client.peer(), std::slice::from_ref(&src)).unwrap();
    assert!(wait_until(WAIT, || rig.host.incoming().is_some_and(|t| t.done_bytes > 8 << 20)));
    rig.disconnect();
    wait_state(&rig.client, &id, State::Failed);
    assert!(wait_until(Duration::from_secs(10), || rig.client.info(&id).can_retry));
    // Change the beginning of the file, keeping its size.
    {
        let mut f = fs::OpenOptions::new().write(true).open(&src).unwrap();
        f.write_all(b"changed!").unwrap();
    }
    rig.connect();
    let before = Stats::get(&rig.host_stats.bytes_received);
    rig.client.svc.retry(&id).unwrap();
    wait_state(&rig.client, &id, State::Done);
    incoming_state(&rig.host, State::Done);
    let resent = Stats::get(&rig.host_stats.bytes_received) - before;
    assert!(resent >= size, "the whole file was sent again ({resent} bytes)");
    assert_eq!(file_hash(&rig.host.dir.join("data.bin")), file_hash(&src), "the new content arrived, not a mix");
}

#[test]
fn offers_need_approval_and_can_be_declined() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rig = Rig::new("approval");
    rig.host.set(|s| s.ask_before_receiving = true);
    let src = rig.root.join("notes.txt");
    make_file(&src, 1000, 1);

    // Declined.
    let id = rig.client.svc.send(&rig.client.peer(), std::slice::from_ref(&src)).unwrap();
    let inc = incoming_state(&rig.host, State::Pending);
    assert!(inc.awaiting_approval);
    assert_eq!(inc.files, vec![FileEntry { name: "notes.txt".into(), size: 1000 }]);
    assert!(
        rig.host
            .events
            .recv_timeout(Duration::from_secs(5))
            .is_ok_and(|e| matches!(e, Event::Incoming { needs_approval: true, .. }))
    );
    assert_eq!(rig.client.info(&id).state, State::Pending, "the sender waits for the answer");
    rig.host.svc.respond(&inc.id, false).unwrap();
    let out = wait_state(&rig.client, &id, State::Rejected);
    assert!(out.error.unwrap().contains("declined"));
    assert_eq!(rig.host.info(&inc.id).state, State::Rejected);
    assert!(!rig.host.dir.join("notes.txt").exists());
    assert!(rig.host.svc.respond(&inc.id, true).is_err(), "cannot answer twice");

    // Accepted.
    rig.host.svc.clear_finished();
    let id = rig.client.svc.send(&rig.client.peer(), std::slice::from_ref(&src)).unwrap();
    let inc = incoming_state(&rig.host, State::Pending);
    rig.host.svc.respond(&inc.id, true).unwrap();
    wait_state(&rig.client, &id, State::Done);
    assert_eq!(fs::read(rig.host.dir.join("notes.txt")).unwrap(), fs::read(&src).unwrap());
}

/// Sends raw offers from the client's connection, bypassing its `TransferService`.
fn raw_offer(rig: &Rig, id: u64, files: &[FileEntry]) {
    let sender = rig.client.hub.sender(&rig.client.peer()).unwrap();
    for part in tr::offer_parts(id, files) {
        assert!(sender.send_feature(channel::FILES, part, Duration::from_secs(5)));
    }
}

fn refused(rig: &Rig, id: u64) -> TransferInfo {
    let ui = format!("r{id:016x}");
    let mut last = None;
    assert!(wait_until(Duration::from_secs(10), || {
        last = rig.host.svc.get(&ui);
        last.as_ref().is_some_and(|t| t.state == State::Rejected)
    }));
    last.unwrap()
}

#[test]
fn hostile_and_oversized_offers_are_refused() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rig = Rig::new("hostile");
    let f = |name: &str, size| FileEntry { name: name.into(), size };

    for (i, name) in [r"..\..\evil.exe", "../x", r"C:\Windows\x.dll", "a:b", ".."].iter().enumerate() {
        let id = 100 + i as u64;
        raw_offer(&rig, id, &[f("fine.txt", 1), f(name, 1)]);
        let t = refused(&rig, id);
        assert!(t.error.as_deref().unwrap().contains("Refused"), "{name}: {t:?}");
    }
    // More than the free space on the drive.
    raw_offer(&rig, 200, &[f("huge.bin", tr::MAX_TOTAL_BYTES)]);
    let t = refused(&rig, 200);
    assert!(t.error.as_deref().unwrap().contains("disk space"), "{t:?}");
    // Sizes that do not add up are refused; too many files never decode.
    let mut part = Msg::Offer { id: 300, total_files: 1, total_bytes: 5, first: 0, files: vec![f("a", 6)] }.encode();
    let sender = rig.client.hub.sender(&rig.client.peer()).unwrap();
    assert!(sender.send_feature(channel::FILES, part.clone(), Duration::from_secs(5)));
    refused(&rig, 300);
    part[9..13].copy_from_slice(&(tr::MAX_FILES + 1).to_le_bytes());
    assert!(sender.send_feature(channel::FILES, part, Duration::from_secs(5)));
    // Receiving turned off locally: refused without a record.
    rig.host.allow.store(false, Ordering::SeqCst);
    raw_offer(&rig, 400, &[f("x.txt", 1)]);
    thread::sleep(Duration::from_millis(300));
    rig.host.allow.store(true, Ordering::SeqCst);
    assert!(rig.host.svc.get(&format!("r{:016x}", 400u64)).is_none());
    assert!(rig.host.svc.list().iter().all(|t| t.state == State::Rejected), "nothing was accepted");
    // Reserved and unusual names are accepted but renamed safely.
    raw_offer(&rig, 500, &[f("con.txt", 0), f("report.pdf. ", 0)]);
    let ui = format!("r{:016x}", 500u64);
    assert!(wait_until(Duration::from_secs(5), || rig.host.svc.get(&ui).is_some()));
    let names: Vec<String> = rig.host.info(&ui).files.into_iter().map(|f| f.name).collect();
    assert_eq!(names, ["_con.txt", "report.pdf"]);
    assert!(fs::read_dir(&rig.host.dir).unwrap().all(|e| e.unwrap().path().parent() == Some(rig.host.dir.as_path())));
}

#[test]
fn sending_requires_the_feature_on_both_sides() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let rig = Rig::new("caps");
    let src = rig.root.join("a.txt");
    make_file(&src, 10, 1);
    let peer = rig.client.peer();
    rig.client.set(|s| s.enabled = false);
    assert!(rig.client.svc.send(&peer, std::slice::from_ref(&src)).is_err());
    rig.client.set(|s| s.enabled = true);
    // The host turns files off for this device: the client learns it from capabilities.
    rig.host.allow.store(false, Ordering::SeqCst);
    rig.host.hub.refresh_caps();
    assert!(wait_until(Duration::from_secs(5), || rig.client.hub.effective_caps(&peer) & cap::FILES == 0));
    let err = rig.client.svc.send(&peer, std::slice::from_ref(&src)).unwrap_err();
    assert!(err.contains("does not accept files"), "{err}");
    assert!(rig.client.svc.send(&peer, std::slice::from_ref(&rig.root)).is_err(), "folders are refused");
}

/// Throughput per speed setting and input latency while a transfer runs host -> client
/// (the direction input travels). Run in release with `--ignored --nocapture`.
#[test]
#[ignore]
fn throughput_and_input_latency() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let mut rig = Rig::new("speed");
    let src = rig.root.join("speed.bin");
    let size = large_mb() << 20;
    make_file(&src, size, 9);
    for speed in ["balanced", "fast"] {
        rig.host.set(|s| s.speed = speed.into());
        let handle = rig.connect_handle();
        let injected = rig.injected.take().unwrap();
        handle.outbox.send(Outgoing::now(Message::Enter { x: 10, y: 10 })).unwrap();
        let started = Instant::now();
        let id = rig.host.svc.send(&rig.host.peer(), std::slice::from_ref(&src)).unwrap();
        let mut ring = LatencyRing::default();
        let mut i = 0u32;
        while rig.host.info(&id).state != State::Done {
            assert!(!rig.host.info(&id).state.is_final(), "{:?}", rig.host.info(&id));
            let t = mono_us();
            let flags = if i % 2 == 0 { key_flags::DOWN } else { 0 };
            handle.outbox.send(Outgoing { msg: Message::Key { vk: 0x41, scan: 0x1e, flags }, t_us: t }).unwrap();
            if let Ok(at) = injected.recv_timeout(Duration::from_secs(5)) {
                ring.record(at.saturating_sub(t) as u32);
            }
            i += 1;
            thread::sleep(Duration::from_millis(2));
        }
        let secs = started.elapsed().as_secs_f64();
        let s = ring.summary();
        println!(
            "{speed}: {:.0} MB in {secs:.2} s = {:.0} MB/s; input latency during the transfer p50 {:.0} us, p95 {:.0} us, max {:.0} us ({} keys)",
            size as f64 / 1e6,
            size as f64 / 1e6 / secs,
            s.p50,
            s.p95,
            s.max,
            i
        );
        assert_eq!(
            file_hash(&rig.client.dir.join(if speed == "balanced" { "speed.bin" } else { "speed (1).bin" })),
            file_hash(&src)
        );
        rig.injected = Some(injected);
        rig.disconnect();
    }
}

impl Rig {
    /// Connects if needed and returns the host's handle to the client.
    fn connect_handle(&mut self) -> ClientHandle {
        if self.session.is_some() {
            self.disconnect();
        }
        self.connect()
    }
}
