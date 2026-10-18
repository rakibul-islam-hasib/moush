//! End-to-end tests over real TCP on loopback: handshake, pairing, input delivery,
//! keepalive, reconnect with stored trust, rejection, stuck-key release, and feature
//! traffic sharing the connection with input.

use std::net::{Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use moush_core::crypto::Identity;
use moush_core::flow::{Acquire, Window};
use moush_core::inject::{InjectState, Injector};
use moush_core::net::client::{run_session, ClientEvents, ClientLink, SessionConfig, SessionEnd};
use moush_core::net::host::{ClientHandle, HostEvents, HostServer};
use moush_core::net::{Outgoing, PeerInfo, PeerSender};
use moush_core::pairing::PairingBroker;
use moush_core::protocol::{key_flags, Message, MonitorDesc, MouseButton, MAX_FEATURE_BODY};
use moush_core::stats::{LatencyRing, Stats};
use moush_core::trust::TrustStore;
use moush_core::util::mono_us;

#[derive(Debug, Clone, PartialEq)]
enum Ev {
    Move(i32, i32),
    Button(MouseButton, bool),
    Key(u16, bool),
}

/// Records injected events with the time they were injected.
#[derive(Clone)]
struct Recorder(Sender<(Ev, u64)>);

impl Injector for Recorder {
    fn move_abs(&mut self, x: i32, y: i32) -> bool {
        self.0.send((Ev::Move(x, y), mono_us())).is_ok()
    }
    fn button(&mut self, b: MouseButton, down: bool) -> bool {
        self.0.send((Ev::Button(b, down), mono_us())).is_ok()
    }
    fn wheel(&mut self, _: moush_core::protocol::WheelAxis, _: i16) -> bool {
        true
    }
    fn key(&mut self, vk: u16, _: u16, _: bool, down: bool, _: bool) -> bool {
        self.0.send((Ev::Key(vk, down), mono_us())).is_ok()
    }
}

struct HostSide {
    connected: Mutex<Sender<ClientHandle>>,
    gone: Mutex<Sender<String>>,
    features: Mutex<Sender<(u8, Vec<u8>)>>,
    caps: Mutex<Sender<u32>>,
}

impl HostEvents for HostSide {
    fn client_connected(&self, c: ClientHandle) {
        let _ = self.connected.lock().unwrap().send(c);
    }
    fn client_screens(&self, _: &str, _: u64, _: Vec<MonitorDesc>) {}
    fn client_disconnected(&self, id: &str, _: u64, _: &str) {
        let _ = self.gone.lock().unwrap().send(id.to_owned());
    }
    fn client_capabilities(&self, _: &str, _: u64, features: u32) {
        let _ = self.caps.lock().unwrap().send(features);
    }
    fn client_feature(&self, _: &str, _: u64, channel: u8, body: Vec<u8>) {
        let _ = self.features.lock().unwrap().send((channel, body));
    }
}

/// Client events that expose the session's sender and record feature traffic.
struct FeatureEvents {
    sender: Mutex<Option<PeerSender>>,
    features: Mutex<Sender<(u8, Vec<u8>)>>,
    caps: Mutex<Sender<u32>>,
}

impl ClientEvents for FeatureEvents {
    fn connected(&self, _: &PeerInfo, sender: &PeerSender) {
        *self.sender.lock().unwrap() = Some(sender.clone());
    }
    fn active_changed(&self, _: bool) {}
    fn capabilities(&self, features: u32) {
        let _ = self.caps.lock().unwrap().send(features);
    }
    fn feature(&self, channel: u8, body: Vec<u8>) {
        // Acknowledge bulk chunks the way real features do (flow control).
        if channel == 2
            && let Some(s) = &*self.sender.lock().unwrap()
        {
            s.try_send_feature(3, (body.len() as u32).to_le_bytes().to_vec());
        }
        let _ = self.features.lock().unwrap().send((channel, body));
    }
}

struct NoEvents;
impl ClientEvents for NoEvents {
    fn connected(&self, _: &PeerInfo, _: &moush_core::net::PeerSender) {}
    fn active_changed(&self, _: bool) {}
}

/// Broker whose prompts are answered automatically with `answer`.
fn auto_broker(answer: bool) -> Arc<PairingBroker> {
    let slot: Arc<Mutex<Option<Arc<PairingBroker>>>> = Arc::new(Mutex::new(None));
    let slot2 = slot.clone();
    let b = PairingBroker::new(Arc::new(move || {
        let slot = slot2.clone();
        // Respond from another thread: the notify callback runs under the broker's caller.
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(20));
            if let Some(b) = slot.lock().unwrap().clone() {
                for r in b.list() {
                    if r.needs_my_approval {
                        b.respond(&r.request_id, answer);
                    }
                }
            }
        });
    }));
    *slot.lock().unwrap() = Some(b.clone());
    b
}

fn screens() -> Vec<MonitorDesc> {
    vec![MonitorDesc { x: 0, y: 0, width: 1920, height: 1200, dpi: 144, primary: true, name: "Built-in".into() }]
}

struct Rig {
    host: HostServer,
    connected: Receiver<ClientHandle>,
    gone: Receiver<String>,
    host_features: Receiver<(u8, Vec<u8>)>,
    host_caps: Receiver<u32>,
    host_trust: Arc<TrustStore>,
    client_trust: Arc<TrustStore>,
    client_id: Identity,
    stats: Arc<Stats>,
}

fn rig(host_answer: bool) -> Rig {
    let (ctx, crx) = mpsc::channel();
    let (gtx, grx) = mpsc::channel();
    let (ftx, frx) = mpsc::channel();
    let (captx, caprx) = mpsc::channel();
    let host_trust = Arc::new(TrustStore::ephemeral());
    let stats = Arc::new(Stats::new());
    let host = HostServer::start(
        std::net::SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        "Desk PC".into(),
        Arc::new(Identity::generate().unwrap()),
        host_trust.clone(),
        auto_broker(host_answer),
        stats.clone(),
        Arc::new(HostSide {
            connected: Mutex::new(ctx),
            gone: Mutex::new(gtx),
            features: Mutex::new(ftx),
            caps: Mutex::new(captx),
        }),
    )
    .unwrap();
    Rig {
        host,
        connected: crx,
        gone: grx,
        host_features: frx,
        host_caps: caprx,
        host_trust,
        client_trust: Arc::new(TrustStore::ephemeral()),
        client_id: Identity::generate().unwrap(),
        stats,
    }
}

/// Starts a client session on a thread; returns its injected events and final result.
fn start_client(rig: &Rig, stop: Arc<AtomicBool>, link: Arc<ClientLink>) -> (Receiver<(Ev, u64)>, thread::JoinHandle<SessionEnd>) {
    start_client_with(rig, stop, link, Arc::new(NoEvents))
}

fn start_client_with(
    rig: &Rig,
    stop: Arc<AtomicBool>,
    link: Arc<ClientLink>,
    client_events: Arc<dyn ClientEvents + Send + Sync>,
) -> (Receiver<(Ev, u64)>, thread::JoinHandle<SessionEnd>) {
    let (etx, erx) = mpsc::channel();
    let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, rig.host.port()));
    let id = rig.client_id.clone();
    let trust = rig.client_trust.clone();
    let stats = Arc::new(Stats::new());
    let h = thread::spawn(move || {
        let broker = auto_broker(true);
        let mut inject = InjectState::new(Recorder(etx));
        run_session(
            SessionConfig {
                addr,
                expected_host: None,
                identity: &id,
                name: "Laptop",
                trust: &trust,
                broker: &broker,
                stats: &stats,
                screens: screens(),
                stop: &stop,
            },
            &mut inject,
            &link,
            client_events.as_ref(),
        )
    });
    (erx, h)
}

fn recv_ev(rx: &Receiver<(Ev, u64)>) -> Ev {
    rx.recv_timeout(Duration::from_secs(5)).expect("event").0
}

#[test]
fn pairing_input_and_disconnect() {
    let rig = rig(true);
    let stop = Arc::new(AtomicBool::new(false));
    let link = ClientLink::new();
    let (events, session) = start_client(&rig, stop.clone(), link.clone());

    let client = rig.connected.recv_timeout(Duration::from_secs(10)).expect("client connects after pairing");
    assert_eq!(client.name, "Laptop");
    assert_eq!(client.screens, screens());
    assert_eq!(rig.host_trust.list().len(), 1, "host remembers the client");
    assert_eq!(rig.client_trust.list().len(), 1, "client remembers the host");

    let send = |m: Message| assert!(client.outbox.send(Outgoing::now(m)).is_ok());
    // Input before Enter is ignored by the client.
    send(Message::MouseMove { x: 5, y: 5 });
    send(Message::Enter { x: 1919, y: 600 });
    assert_eq!(recv_ev(&events), Ev::Move(1919, 600));
    send(Message::MouseMove { x: 1900, y: 610 });
    assert_eq!(recv_ev(&events), Ev::Move(1900, 610));
    send(Message::MouseButton { button: MouseButton::Left, down: true });
    send(Message::MouseButton { button: MouseButton::Left, down: false });
    assert_eq!(recv_ev(&events), Ev::Button(MouseButton::Left, true));
    assert_eq!(recv_ev(&events), Ev::Button(MouseButton::Left, false));

    // A key held when the connection dies is released by the client.
    send(Message::Key { vk: 0x41, scan: 0x1e, flags: key_flags::DOWN });
    assert_eq!(recv_ev(&events), Ev::Key(0x41, true));
    assert!(rig.host.disconnect(&client.id));
    assert_eq!(recv_ev(&events), Ev::Key(0x41, false), "held key released on disconnect");
    let end = session.join().unwrap();
    assert!(matches!(end, SessionEnd::Fatal(_)), "kicked clients do not auto-reconnect: {end:?}");
    assert_eq!(rig.gone.recv_timeout(Duration::from_secs(5)).unwrap(), client.id);
}

#[test]
fn trusted_client_reconnects_without_prompt() {
    let rig = rig(true);
    let link = ClientLink::new();
    let stop = Arc::new(AtomicBool::new(false));
    let (_, session) = start_client(&rig, stop.clone(), link.clone());
    let first = rig.connected.recv_timeout(Duration::from_secs(10)).unwrap();
    stop.store(true, Ordering::SeqCst);
    link.close();
    assert_eq!(session.join().unwrap(), SessionEnd::Stopped);
    rig.gone.recv_timeout(Duration::from_secs(5)).unwrap();

    // Second session: both sides already trust each other, so no prompt is shown and the
    // connection completes without the 20 ms auto-answer delay.
    let started = Instant::now();
    let stop = Arc::new(AtomicBool::new(false));
    let (_, session) = start_client(&rig, stop.clone(), link.clone());
    let second = rig.connected.recv_timeout(Duration::from_secs(10)).unwrap();
    let reconnect_ms = started.elapsed().as_millis();
    assert_eq!(first.id, second.id);
    assert!(second.conn_no > first.conn_no);
    println!("reconnect with stored trust took {reconnect_ms} ms");
    assert!(reconnect_ms < 1000);
    stop.store(true, Ordering::SeqCst);
    link.close();
    session.join().unwrap();
}

#[test]
fn rejected_pairing_is_fatal_and_not_trusted() {
    let rig = rig(false);
    let (_, session) = start_client(&rig, Arc::new(AtomicBool::new(false)), ClientLink::new());
    let end = session.join().unwrap();
    assert!(matches!(end, SessionEnd::Fatal(_)), "{end:?}");
    assert!(rig.host_trust.list().is_empty());
    assert!(rig.client_trust.list().is_empty());
    assert!(rig.connected.try_recv().is_err());
}

#[test]
fn latency_over_loopback() {
    let rig = rig(true);
    let link = ClientLink::new();
    let stop = Arc::new(AtomicBool::new(false));
    let (events, session) = start_client(&rig, stop.clone(), link.clone());
    let client = rig.connected.recv_timeout(Duration::from_secs(10)).unwrap();
    client.outbox.send(Outgoing::now(Message::Enter { x: 0, y: 0 })).unwrap();
    recv_ev(&events);

    // 1000 Hz key events (not coalescable), measuring enqueue -> injected.
    let mut ring = LatencyRing::default();
    for i in 0..1000u32 {
        let t = mono_us();
        let down = i % 2 == 0;
        client
            .outbox
            .send(Outgoing { msg: Message::Key { vk: 0x41, scan: 0x1e, flags: if down { key_flags::DOWN } else { 0 } }, t_us: t })
            .unwrap();
        let (_, at) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        ring.record(at.saturating_sub(t) as u32);
        thread::sleep(Duration::from_micros(1000));
    }
    let s = ring.summary();
    println!(
        "loopback enqueue->inject latency over {} events: mean {:.0} us, p50 {:.0} us, p95 {:.0} us, p99 {:.0} us, max {:.0} us",
        s.count, s.mean, s.p50, s.p95, s.p99, s.max
    );
    let send = Stats::summary(&rig.stats.send_us);
    println!("host send path: p50 {:.0} us, p99 {:.0} us", send.p50, send.p99);
    assert!(s.p50 < 2000.0, "median loopback latency should be well under 2 ms");

    let (_, n, rtt) = rig.host.latency_test(&client.id, 50).unwrap();
    println!("ping test: {n}/50 replies, rtt p50 {:.3} ms p99 {:.3} ms", rtt.p50, rtt.p99);
    assert_eq!(n, 50);

    stop.store(true, Ordering::SeqCst);
    link.close();
    session.join().unwrap();
}

#[test]
fn feature_traffic_flows_both_ways_without_starving_input() {
    let rig = rig(true);
    let link = ClientLink::new();
    let stop = Arc::new(AtomicBool::new(false));
    let (ftx, frx) = mpsc::channel();
    let (ctx, crx) = mpsc::channel();
    let fe = Arc::new(FeatureEvents { sender: Mutex::new(None), features: Mutex::new(ftx), caps: Mutex::new(ctx) });
    let (events, session) = start_client_with(&rig, stop.clone(), link.clone(), fe.clone());
    let client = rig.connected.recv_timeout(Duration::from_secs(10)).unwrap();
    // The host can see the connection before the client thread reports it.
    let waited = Instant::now();
    let to_host = loop {
        if let Some(s) = fe.sender.lock().unwrap().clone() {
            break s;
        }
        assert!(waited.elapsed() < Duration::from_secs(5), "client sender");
        thread::sleep(Duration::from_millis(2));
    };

    // Capabilities in both directions.
    assert!(client.sender.send_control(Message::Capabilities { features: 0b11 }));
    assert!(to_host.send_control(Message::Capabilities { features: 0b10 }));
    assert_eq!(crx.recv_timeout(Duration::from_secs(5)).unwrap(), 0b11);
    assert_eq!(rig.host_caps.recv_timeout(Duration::from_secs(5)).unwrap(), 0b10);

    // Client -> host feature message.
    assert!(to_host.send_feature(1, b"hello host".to_vec(), Duration::from_secs(1)));
    assert_eq!(rig.host_features.recv_timeout(Duration::from_secs(5)).unwrap(), (1, b"hello host".to_vec()));

    // Host streams 8 MB of maximum-size chunks, flow-controlled by the client's acks,
    // while input keeps flowing.
    client.outbox.send(Outgoing::now(Message::Enter { x: 0, y: 0 })).unwrap();
    recv_ev(&events);
    const CHUNKS: usize = 256;
    let window = Arc::new(Window::new(128 * 1024));
    let acks = {
        let window = window.clone();
        let host_features = rig.host_features;
        thread::spawn(move || {
            let mut acked = 0;
            while acked < CHUNKS * MAX_FEATURE_BODY {
                let (ch, body) = host_features.recv_timeout(Duration::from_secs(10)).expect("ack");
                assert_eq!(ch, 3);
                let n = u32::from_le_bytes(body[..4].try_into().unwrap()) as usize;
                window.release(n);
                acked += n;
            }
        })
    };
    let sender = client.sender.clone();
    let started = Instant::now();
    let bulk = {
        let window = window.clone();
        thread::spawn(move || {
            for i in 0..CHUNKS {
                let mut body = vec![(i % 251) as u8; MAX_FEATURE_BODY];
                body[..4].copy_from_slice(&(i as u32).to_le_bytes());
                assert_eq!(window.acquire(body.len(), Duration::from_secs(10)), Acquire::Ok);
                assert!(sender.send_feature(2, body, Duration::from_secs(10)), "chunk {i} queued");
            }
        })
    };
    let mut ring = LatencyRing::default();
    for i in 0..200u32 {
        let t = mono_us();
        let down = i % 2 == 0;
        client
            .outbox
            .send(Outgoing { msg: Message::Key { vk: 0x41, scan: 0x1e, flags: if down { key_flags::DOWN } else { 0 } }, t_us: t })
            .unwrap();
        let (_, at) = events.recv_timeout(Duration::from_secs(5)).unwrap();
        ring.record(at.saturating_sub(t) as u32);
        thread::sleep(Duration::from_micros(500));
    }
    bulk.join().unwrap();
    acks.join().unwrap();
    let mb_s = (CHUNKS * MAX_FEATURE_BODY) as f64 / started.elapsed().as_secs_f64() / 1e6;
    for i in 0..CHUNKS {
        let (ch, body) = frx.recv_timeout(Duration::from_secs(10)).expect("chunk arrives");
        assert_eq!(ch, 2);
        assert_eq!(body.len(), MAX_FEATURE_BODY);
        assert_eq!(u32::from_le_bytes(body[..4].try_into().unwrap()), i as u32, "chunks arrive in order");
    }
    let s = ring.summary();
    println!(
        "input latency during an 8 MB bulk stream: p50 {:.0} us, p95 {:.0} us, max {:.0} us; bulk throughput {mb_s:.0} MB/s",
        s.p50, s.p95, s.max
    );
    assert!(s.p50 < 20_000.0, "input keeps flowing during bulk transfers");

    stop.store(true, Ordering::SeqCst);
    link.close();
    session.join().unwrap();
    assert!(!client.sender.try_send_feature(1, vec![1]), "sender is inert after disconnect");
}
