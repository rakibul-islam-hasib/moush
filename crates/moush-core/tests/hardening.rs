//! Adversarial and overload behaviour of real connections on loopback: bulk traffic
//! competing with input, a receiver that stops reading, slow (dripping) peers before
//! authentication, and pairing floods.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use moush_core::crypto::Identity;
use moush_core::inject::{InjectState, Injector};
use moush_core::net::client::{run_session, ClientEvents, ClientLink, SessionConfig, SessionEnd};
use moush_core::net::host::{ClientHandle, HostEvents, HostServer};
use moush_core::net::{try_send, Outgoing, PeerInfo, PeerSender, MAX_BULK_QUEUED};
use moush_core::pairing::PairingBroker;
use moush_core::protocol::{key_flags, Message, MonitorDesc, MouseButton, WheelAxis, MAX_FEATURE_BODY};
use moush_core::stats::Stats;
use moush_core::trust::{TrustRole, TrustStore};

/// Counts injected key events.
struct Keys(Sender<u16>);

impl Injector for Keys {
    fn move_abs(&mut self, _: i32, _: i32) -> bool {
        true
    }
    fn button(&mut self, _: MouseButton, _: bool) -> bool {
        true
    }
    fn wheel(&mut self, _: WheelAxis, _: i16) -> bool {
        true
    }
    fn key(&mut self, vk: u16, _: u16, _: bool, _: bool, _: bool) -> bool {
        self.0.send(vk).is_ok()
    }
}

struct HostSide {
    connected: Mutex<Sender<ClientHandle>>,
    gone: Mutex<Sender<String>>,
}

impl HostEvents for HostSide {
    fn client_connected(&self, c: ClientHandle) {
        let _ = self.connected.lock().unwrap().send(c);
    }
    fn client_screens(&self, _: &str, _: u64, _: Vec<MonitorDesc>) {}
    fn client_disconnected(&self, id: &str, _: u64, _: &str) {
        let _ = self.gone.lock().unwrap().send(id.to_owned());
    }
}

/// Client events whose feature callback can stall the client's reader thread (the thread
/// that also injects input), simulating a receiver that stops reading.
struct Stalling {
    sender: Mutex<Option<PeerSender>>,
    stall: Duration,
    stalled: AtomicBool,
    features: AtomicU64,
}

impl ClientEvents for Stalling {
    fn connected(&self, _: &PeerInfo, sender: &PeerSender) {
        *self.sender.lock().unwrap() = Some(sender.clone());
    }
    fn active_changed(&self, _: bool) {}
    fn feature(&self, _: u8, _: Vec<u8>) {
        self.features.fetch_add(1, Ordering::Relaxed);
        if !self.stalled.swap(true, Ordering::SeqCst) {
            thread::sleep(self.stall);
        }
    }
}

fn never_answering_broker() -> Arc<PairingBroker> {
    PairingBroker::new(Arc::new(|| {}))
}

/// Broker that approves every prompt shortly after it opens.
fn approving_broker() -> Arc<PairingBroker> {
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

struct Host {
    server: HostServer,
    identity: Identity,
    trust: Arc<TrustStore>,
    broker: Arc<PairingBroker>,
    connected: Receiver<ClientHandle>,
    gone: Receiver<String>,
}

fn host(broker: Arc<PairingBroker>) -> Host {
    let (ctx, crx) = mpsc::channel();
    let (gtx, grx) = mpsc::channel();
    let identity = Identity::generate().unwrap();
    let trust = Arc::new(TrustStore::ephemeral());
    let server = HostServer::start(
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        "Desk PC".into(),
        Arc::new(identity.clone()),
        trust.clone(),
        broker.clone(),
        Arc::new(Stats::new()),
        Arc::new(HostSide { connected: Mutex::new(ctx), gone: Mutex::new(gtx) }),
    )
    .unwrap();
    Host { server, identity, trust, broker, connected: crx, gone: grx }
}

struct Client {
    keys: Receiver<u16>,
    stop: Arc<AtomicBool>,
    link: Arc<ClientLink>,
    session: thread::JoinHandle<SessionEnd>,
}

impl Client {
    fn end(self) -> SessionEnd {
        self.stop.store(true, Ordering::SeqCst);
        self.link.close();
        self.session.join().unwrap()
    }
}

/// Starts a client session. With `pretrusted`, both sides trust each other up front.
fn client(h: &Host, pretrusted: bool, events: Arc<dyn ClientEvents + Send + Sync>) -> Client {
    let id = Identity::generate().unwrap();
    let trust = Arc::new(TrustStore::ephemeral());
    if pretrusted {
        h.trust.add(id.public_key(), "Laptop", TrustRole::Client, None).unwrap();
        trust.add(h.identity.public_key(), "Desk PC", TrustRole::Host, None).unwrap();
    }
    let (ktx, krx) = mpsc::channel();
    let stop = Arc::new(AtomicBool::new(false));
    let link = ClientLink::new();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, h.server.port()));
    let session = {
        let (stop, link) = (stop.clone(), link.clone());
        thread::spawn(move || {
            let broker = approving_broker();
            let stats = Arc::new(Stats::new());
            let mut inject = InjectState::new(Keys(ktx));
            run_session(
                SessionConfig {
                    addr,
                    expected_host: None,
                    identity: &id,
                    name: "Laptop",
                    trust: &trust,
                    broker: &broker,
                    stats: &stats,
                    screens: vec![MonitorDesc { x: 0, y: 0, width: 1920, height: 1080, dpi: 96, primary: true, name: "D".into() }],
                    stop: &stop,
                },
                &mut inject,
                &link,
                events.as_ref(),
            )
        })
    };
    Client { keys: krx, stop, link, session }
}

fn key(i: u32) -> Message {
    Message::Key { vk: 0x41, scan: 0x1e, flags: if i & 1 == 0 { key_flags::DOWN } else { 0 } }
}

/// Floods the connection with bulk messages from several threads until `stop`.
fn bulk_flood(sender: &PeerSender, threads: usize, stop: &Arc<AtomicBool>) -> Vec<thread::JoinHandle<(u64, u64)>> {
    (0..threads)
        .map(|_| {
            let (sender, stop) = (sender.clone(), stop.clone());
            thread::spawn(move || {
                let (mut ok, mut refused) = (0, 0);
                while !stop.load(Ordering::Relaxed) {
                    if sender.send_feature(2, vec![0x5A; MAX_FEATURE_BODY], Duration::from_millis(20)) {
                        ok += 1;
                    } else {
                        refused += 1;
                        if sender.is_closed() {
                            break;
                        }
                    }
                }
                (ok, refused)
            })
        })
        .collect()
}

/// Sends `n` key events at ~1 kHz from this thread, like the host's hook thread does.
/// Returns (refused sends, slowest try_send, time of the first refusal).
fn input_at_1khz(c: &ClientHandle, n: u32) -> (u32, Duration, Option<Duration>) {
    let mut refused = 0;
    let mut first_refusal = None;
    let mut slowest = Duration::ZERO;
    let start = Instant::now();
    for i in 0..n {
        let t = Instant::now();
        if !try_send(&c.outbox, Outgoing::now(key(i))) {
            refused += 1;
            first_refusal.get_or_insert(start.elapsed());
        }
        slowest = slowest.max(t.elapsed());
        let next = start + Duration::from_micros(1000 * (i as u64 + 1));
        if let Some(d) = next.checked_duration_since(Instant::now()) {
            thread::sleep(d);
        }
    }
    (refused, slowest, first_refusal)
}

#[test]
fn bulk_contention_never_refuses_input() {
    let h = host(never_answering_broker());
    let c = client(&h, true, Arc::new(Stalling {
        sender: Mutex::new(None),
        stall: Duration::ZERO,
        stalled: AtomicBool::new(true),
        features: AtomicU64::new(0),
    }));
    let ch = h.connected.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(try_send(&ch.outbox, Outgoing::now(Message::Enter { x: 0, y: 0 })));

    // Eight feature workers compete for the bulk lane; the gate lets at most
    // MAX_BULK_QUEUED wait in the queue, so input always finds room.
    let stop = Arc::new(AtomicBool::new(false));
    let flood = bulk_flood(&ch.sender, 8, &stop);
    let (refused, slowest, _) = input_at_1khz(&ch, 2000);
    stop.store(true, Ordering::SeqCst);
    let (sent, waited): (u64, u64) = flood.into_iter().map(|t| t.join().unwrap()).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    for _ in 0..2000 {
        c.keys.recv_timeout(Duration::from_secs(10)).expect("every key arrives");
    }
    println!(
        "bulk contention: {sent} chunks sent ({:.0} MB), {waited} gate timeouts; input refused {refused}, slowest try_send {slowest:?}",
        sent as f64 * MAX_FEATURE_BODY as f64 / 1e6
    );
    assert_eq!(refused, 0, "bulk traffic never fills the queue input uses");
    assert!(slowest < Duration::from_millis(50), "try_send never blocks ({slowest:?})");
    assert!(sent > MAX_BULK_QUEUED as u64, "bulk traffic actually flowed");
    c.end();
}

#[test]
fn stalled_receiver_resumes_without_losing_input() {
    let h = host(never_answering_broker());
    let ev = Arc::new(Stalling {
        sender: Mutex::new(None),
        stall: Duration::from_millis(1200),
        stalled: AtomicBool::new(false),
        features: AtomicU64::new(0),
    });
    let c = client(&h, true, ev.clone());
    let ch = h.connected.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(try_send(&ch.outbox, Outgoing::now(Message::Enter { x: 0, y: 0 })));

    // The client's reader blocks for 1.2 s on the first chunk, while the host keeps
    // streaming bulk data and input (no flow control: worst case for socket buffers).
    let stop = Arc::new(AtomicBool::new(false));
    let flood = bulk_flood(&ch.sender, 2, &stop);
    let (refused, slowest, _) = input_at_1khz(&ch, 2500);
    stop.store(true, Ordering::SeqCst);
    for t in flood {
        t.join().unwrap();
    }
    for i in 0..2500 {
        c.keys.recv_timeout(Duration::from_secs(10)).unwrap_or_else(|_| panic!("key {i} arrives after the stall"));
    }
    println!("stall 1.2 s: refused {refused}, slowest try_send {slowest:?}, chunks received {}", ev.features.load(Ordering::Relaxed));
    assert!(ev.stalled.load(Ordering::SeqCst), "the stall happened");
    assert_eq!(refused, 0, "a short stall fits in the queue");
    assert!(slowest < Duration::from_millis(50), "try_send never blocks ({slowest:?})");
    assert!(h.gone.try_recv().is_err(), "the connection survived");
    c.end();
}

#[test]
fn receiver_that_stops_reading_is_disconnected_and_input_never_blocks() {
    let h = host(never_answering_broker());
    let ev = Arc::new(Stalling {
        sender: Mutex::new(None),
        stall: Duration::from_secs(8),
        stalled: AtomicBool::new(false),
        features: AtomicU64::new(0),
    });
    let c = client(&h, true, ev.clone());
    let ch = h.connected.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(try_send(&ch.outbox, Outgoing::now(Message::Enter { x: 0, y: 0 })));

    let stop = Arc::new(AtomicBool::new(false));
    let flood = bulk_flood(&ch.sender, 2, &stop);
    // Keep "typing" for 6 s; the hook thread must never wait, whatever the client does.
    let (refused, slowest, cut) = input_at_1khz(&ch, 6000);
    let gone = h.gone.recv_timeout(Duration::from_secs(10)).expect("host drops the stuck client");
    stop.store(true, Ordering::SeqCst);
    for t in flood {
        t.join().unwrap();
    }
    println!("stuck receiver: input refused from {cut:?} on ({refused} events); slowest try_send {slowest:?}");
    // The dead-peer check (3.5 s of silence) or the 3 s write timeout ends it.
    let cut = cut.expect("the connection was closed while typing");
    assert!(cut < Duration::from_millis(5500), "stuck peer detected in time ({cut:?})");
    assert_eq!(gone, ch.id);
    assert!(slowest < Duration::from_millis(50), "try_send never blocks ({slowest:?})");
    assert!(ch.sender.is_closed(), "feature sender is inert afterwards");
    assert!(!ch.sender.try_send_feature(2, vec![1]));
    assert!(matches!(c.end(), SessionEnd::Lost(_) | SessionEnd::Stopped));
}

#[test]
fn dripping_peer_is_cut_off_before_authentication() {
    let h = host(never_answering_broker());
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, h.server.port())).unwrap();
    s.set_read_timeout(Some(Duration::from_millis(500))).unwrap();
    let started = Instant::now();
    // A Noise frame header announcing 48 bytes, then one byte every 1.5 s: each read on
    // the host completes within its 5 s timeout, so only the overall deadline helps.
    let _ = s.write_all(&48u16.to_le_bytes());
    let closed_after = loop {
        let mut b = [0u8; 64];
        match s.read(&mut b) {
            Ok(0) => break started.elapsed(),
            Ok(_) => {}
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
            Err(_) => break started.elapsed(),
        }
        if started.elapsed() > Duration::from_secs(30) {
            panic!("host kept a dripping connection open for 30 s");
        }
        if s.write_all(&[0x42]).is_err() {
            break started.elapsed();
        }
        thread::sleep(Duration::from_millis(1000));
    };
    println!("dripping connection closed after {closed_after:?}");
    assert!(closed_after >= Duration::from_secs(8) && closed_after < Duration::from_secs(16), "{closed_after:?}");
}

#[test]
fn pairing_flood_cannot_lock_out_trusted_clients() {
    let h = host(never_answering_broker());
    // Two unpaired devices wait for a pairing decision nobody gives.
    let quiet = || -> Arc<dyn ClientEvents + Send + Sync> {
        Arc::new(Stalling { sender: Mutex::new(None), stall: Duration::ZERO, stalled: AtomicBool::new(true), features: AtomicU64::new(0) })
    };
    let a = client(&h, false, quiet());
    let b = client(&h, false, quiet());
    let t = Instant::now();
    while h.broker.list().len() < 2 {
        assert!(t.elapsed() < Duration::from_secs(10), "two pairing prompts open");
        thread::sleep(Duration::from_millis(20));
    }
    // A third pairing attempt is refused right away instead of taking a slot.
    let c = client(&h, false, quiet());
    let end = c.session.join().unwrap();
    assert!(matches!(&end, SessionEnd::Fatal(m) if m.contains("busy")), "{end:?}");
    assert_eq!(h.broker.list().len(), 2);
    // Trusted clients still get in while the pairings are pending.
    let d = client(&h, true, quiet());
    let dh = h.connected.recv_timeout(Duration::from_secs(10)).expect("trusted client connects");
    assert_eq!(dh.name, "Laptop");
    d.end();
    // A device that gives up while our user has not decided yet withdraws its prompt and
    // frees its slot at once (not after the 90 s pairing timeout).
    let t = Instant::now();
    a.end();
    b.end();
    assert!(t.elapsed() < Duration::from_secs(5), "abandoned pairing ends promptly ({:?})", t.elapsed());
    while !h.broker.list().is_empty() {
        assert!(t.elapsed() < Duration::from_secs(10), "abandoned prompts are withdrawn");
        thread::sleep(Duration::from_millis(20));
    }
}
