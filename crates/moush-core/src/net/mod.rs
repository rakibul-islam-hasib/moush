//! Connection plumbing shared by host and client: message framing over the secure
//! channel, the pairing/authorization exchange, the writer thread with keepalive and
//! move coalescing, and latency probing.
//!
//! Transport choice: a single TCP connection with `TCP_NODELAY`. Key and button
//! transitions must arrive exactly once and in order relative to cursor moves (a click
//! must land where the cursor is), which TCP gives for free. On a LAN, a lost segment is
//! repaired by fast retransmit within a few milliseconds while input is flowing, and the
//! writer coalesces queued moves so a stall never replays a backlog of stale positions.
//! UDP would need its own ordering and retransmission for exactly the events that matter.
//!
//! Bulk feature traffic (clipboard contents, file chunks) shares the connection but never
//! delays input: at most [`MAX_BULK_QUEUED`] bulk messages may wait in a connection's
//! queue, the writer always sends pending input first and writes bulk messages one at a
//! time in between, and feature senders bound the bytes in flight with
//! [`crate::flow::Window`].

pub mod client;
pub mod host;

use std::collections::VecDeque;
use std::io::{self, BufReader};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::crypto::{device_id, FrameReader, FrameWriter, SecureChannel, MAX_FRAME_PLAINTEXT};
use crate::pairing::{PairingBroker, PairingRequest};
use crate::protocol::{self, Message, PeerRole, PROTOCOL_VERSION};
use crate::stats::{LatencySummary, Stats};
use crate::trust::{TrustRole, TrustStore};
use crate::util::{mono_ms, mono_us};

pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
pub const PAIRING_TIMEOUT: Duration = Duration::from_secs(90);
pub const PING_INTERVAL: Duration = Duration::from_secs(1);
/// A peer that sends nothing (not even a ping reply) for this long is considered gone.
pub const DEAD_AFTER: Duration = Duration::from_millis(3500);
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(3);
pub const OUTBOX_CAPACITY: usize = 4096;
/// Ping ids at or above this value belong to an explicit latency test.
const PROBE_ID_BASE: u32 = 0x8000_0000;
/// Bulk (feature) messages that may wait in one connection's queue at a time.
pub const MAX_BULK_QUEUED: usize = 16;

/// A message queued for sending, stamped with the time it was captured.
pub struct Outgoing {
    pub msg: Message,
    /// `mono_us()` at capture; 0 = do not measure.
    pub t_us: u64,
}

impl Outgoing {
    pub fn now(msg: Message) -> Self {
        Self { msg, t_us: mono_us() }
    }
    pub fn untimed(msg: Message) -> Self {
        Self { msg, t_us: 0 }
    }
}

pub type Outbox = SyncSender<Outgoing>;

/// Limits how many bulk messages wait in a connection's queue, so feature traffic can
/// never fill the queue that input depends on.
#[derive(Debug, Default)]
pub struct BulkGate {
    state: Mutex<(usize, bool)>,
    cv: Condvar,
}

impl BulkGate {
    fn enter(&self, timeout: Option<Duration>) -> bool {
        let deadline = timeout.map(|t| Instant::now() + t);
        let mut s = self.state.lock().unwrap();
        loop {
            if s.1 {
                return false;
            }
            if s.0 < MAX_BULK_QUEUED {
                s.0 += 1;
                return true;
            }
            let left = match deadline {
                Some(d) => d.saturating_duration_since(Instant::now()),
                None => return false,
            };
            if left.is_zero() {
                return false;
            }
            s = self.cv.wait_timeout(s, left).unwrap().0;
        }
    }

    fn leave(&self) {
        let mut s = self.state.lock().unwrap();
        s.0 = s.0.saturating_sub(1);
        self.cv.notify_one();
    }

    fn close(&self) {
        self.state.lock().unwrap().1 = true;
        self.cv.notify_all();
    }
}

/// Sends feature and control traffic to one connected peer. Cheap to clone; becomes
/// inert when the connection closes.
#[derive(Clone)]
pub struct PeerSender {
    outbox: Outbox,
    gate: Arc<BulkGate>,
    shared: Arc<ConnShared>,
}

impl PeerSender {
    pub(crate) fn new(outbox: Outbox, gate: Arc<BulkGate>, shared: Arc<ConnShared>) -> Self {
        Self { outbox, gate, shared }
    }

    /// Queues a small control message (e.g. `Capabilities`) without blocking.
    pub fn send_control(&self, msg: protocol::Message) -> bool {
        debug_assert!(!msg.is_bulk());
        try_send(&self.outbox, Outgoing::untimed(msg))
    }

    /// Queues a feature message, waiting up to `timeout` while the connection already
    /// has [`MAX_BULK_QUEUED`] bulk messages waiting. Call from worker threads only.
    /// Returns false on timeout or when the connection is gone.
    pub fn send_feature(&self, channel: u8, body: Vec<u8>, timeout: Duration) -> bool {
        self.queue_feature(channel, body, Some(timeout))
    }

    /// Like [`send_feature`](Self::send_feature) but never blocks (safe on reader threads).
    pub fn try_send_feature(&self, channel: u8, body: Vec<u8>) -> bool {
        self.queue_feature(channel, body, None)
    }

    fn queue_feature(&self, channel: u8, body: Vec<u8>, timeout: Option<Duration>) -> bool {
        if body.len() > protocol::MAX_FEATURE_BODY {
            log::error!("feature body of {} bytes exceeds the limit", body.len());
            return false;
        }
        if self.shared.is_closed() || !self.gate.enter(timeout) {
            return false;
        }
        match self.outbox.try_send(Outgoing::untimed(Message::Feature { channel, body })) {
            Ok(()) => true,
            Err(_) => {
                self.gate.leave();
                false
            }
        }
    }

    pub fn is_closed(&self) -> bool {
        self.shared.is_closed()
    }
}

/// Queues a message without blocking. A full queue means the peer stopped reading.
pub fn try_send(outbox: &Outbox, msg: Outgoing) -> bool {
    match outbox.try_send(msg) {
        Ok(()) => true,
        Err(TrySendError::Full(_)) => {
            log::warn!("outgoing queue full; peer is not keeping up");
            false
        }
        Err(TrySendError::Disconnected(_)) => false,
    }
}

/// State shared between the reader, the writer and the owner of a connection.
pub struct ConnShared {
    stream: TcpStream,
    closed: AtomicBool,
    last_recv_ms: AtomicU64,
    /// Smoothed RTT in µs; `u64::MAX` until the first pong.
    srtt_us: AtomicU64,
    probe: Mutex<Option<mpsc::Sender<(u32, u64)>>>,
}

impl ConnShared {
    fn new(stream: TcpStream) -> Arc<Self> {
        Arc::new(Self {
            stream,
            closed: AtomicBool::new(false),
            last_recv_ms: AtomicU64::new(mono_ms()),
            srtt_us: AtomicU64::new(u64::MAX),
            probe: Mutex::new(None),
        })
    }

    /// Closes the socket; blocked reads and writes on other threads return immediately.
    pub fn close(&self) {
        if !self.closed.swap(true, Ordering::SeqCst) {
            let _ = self.stream.shutdown(Shutdown::Both);
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    pub fn rtt_ms(&self) -> Option<f64> {
        match self.srtt_us.load(Ordering::Relaxed) {
            u64::MAX => None,
            v => Some(v as f64 / 1000.0),
        }
    }

    fn on_recv(&self) {
        self.last_recv_ms.store(mono_ms(), Ordering::Relaxed);
    }

    fn on_pong(&self, id: u32, t_us: u64, stats: &Stats) {
        let rtt = mono_us().saturating_sub(t_us);
        if id >= PROBE_ID_BASE {
            if let Some(tx) = &*self.probe.lock().unwrap() {
                let _ = tx.send((id, rtt));
            }
            return;
        }
        Stats::record(&stats.rtt_us, rtt);
        let prev = self.srtt_us.load(Ordering::Relaxed);
        let next = if prev == u64::MAX { rtt } else { (prev * 7 + rtt) / 8 };
        self.srtt_us.store(next, Ordering::Relaxed);
    }
}

/// Reads and decodes messages from the secure channel.
pub struct MsgReader {
    fr: FrameReader<BufReader<TcpStream>>,
    queue: VecDeque<Message>,
    batch: Vec<Message>,
    stats: Arc<Stats>,
}

impl MsgReader {
    /// Reads one frame and returns its messages plus the time the frame was complete.
    pub fn read_batch(&mut self) -> io::Result<(&[Message], u64)> {
        let (plain, wire) = self.fr.read_frame()?;
        let t = mono_us();
        self.batch.clear();
        protocol::decode_all(plain, &mut self.batch).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Stats::add(&self.stats.frames_received, 1);
        Stats::add(&self.stats.bytes_received, wire as u64);
        Ok((&self.batch, t))
    }

    /// Next single message (setup phases).
    pub fn next_msg(&mut self) -> io::Result<Message> {
        loop {
            if let Some(m) = self.queue.pop_front() {
                return Ok(m);
            }
            let (batch, _) = self.read_batch()?;
            let batch = batch.to_vec();
            self.queue.extend(batch);
        }
    }

    fn stream(&self) -> &TcpStream {
        self.fr.get_ref().get_ref()
    }
}

/// Encodes and writes messages to the secure channel.
pub struct MsgWriter {
    fw: FrameWriter<TcpStream>,
    buf: Vec<u8>,
    stats: Arc<Stats>,
}

impl MsgWriter {
    /// Sends `msgs` in as few frames as possible. A message never spans two frames,
    /// because the reader decodes each frame on its own.
    pub fn send(&mut self, msgs: &[Message]) -> io::Result<()> {
        self.buf.clear();
        for m in msgs {
            if !self.buf.is_empty() && self.buf.len() + protocol::encoded_len(m) > MAX_FRAME_PLAINTEXT {
                self.flush()?;
            }
            protocol::encode(m, &mut self.buf);
        }
        self.flush()
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.buf.is_empty() {
            return Ok(());
        }
        let n = self.fw.write_frame(&self.buf)?;
        self.buf.clear();
        Stats::add(&self.stats.frames_sent, 1);
        Stats::add(&self.stats.bytes_sent, n as u64);
        Ok(())
    }
}

pub(crate) fn split_channel(ch: SecureChannel, stream: &TcpStream, stats: &Arc<Stats>) -> io::Result<(MsgReader, MsgWriter)> {
    let (fr, fw) = ch.split(BufReader::with_capacity(64 * 1024, stream.try_clone()?), stream.try_clone()?);
    Ok((
        MsgReader { fr, queue: VecDeque::new(), batch: Vec::with_capacity(64), stats: stats.clone() },
        MsgWriter { fw, buf: Vec::with_capacity(1024), stats: stats.clone() },
    ))
}

/// Merges runs of consecutive cursor moves into the last one. Never reorders.
pub fn coalesce(batch: &mut Vec<Outgoing>) {
    if batch.len() < 2 {
        return;
    }
    let mut out: Vec<Outgoing> = Vec::with_capacity(batch.len());
    for m in batch.drain(..) {
        if let Some(last) = out.last_mut()
            && last.msg.is_coalescable_move()
            && m.msg.is_coalescable_move()
        {
            // Keep the oldest capture time so latency stats stay honest.
            let t = if last.t_us != 0 { last.t_us } else { m.t_us };
            *last = Outgoing { msg: m.msg, t_us: t };
            continue;
        }
        out.push(m);
    }
    *batch = out;
}

/// Sends queued messages, pings the peer every second and closes the connection if the
/// peer goes silent. Runs until the connection closes or every sender is dropped.
///
/// Input and control messages always go first; bulk messages are held back and written
/// one per loop iteration, so input queued meanwhile overtakes the rest of the bulk data.
pub(crate) fn run_writer(
    mut w: MsgWriter,
    rx: Receiver<Outgoing>,
    shared: Arc<ConnShared>,
    gate: Arc<BulkGate>,
    stats: Arc<Stats>,
    measure: bool,
) {
    let mut next_ping = Instant::now() + PING_INTERVAL;
    let mut ping_id: u32 = 0;
    let mut batch: Vec<Outgoing> = Vec::with_capacity(64);
    let mut msgs: Vec<Message> = Vec::with_capacity(64);
    let mut bulk: VecDeque<Message> = VecDeque::new();
    loop {
        let wait = next_ping.saturating_duration_since(Instant::now());
        let first = if bulk.is_empty() {
            rx.recv_timeout(wait)
        } else {
            rx.try_recv().map_err(|e| match e {
                mpsc::TryRecvError::Empty => RecvTimeoutError::Timeout,
                mpsc::TryRecvError::Disconnected => RecvTimeoutError::Disconnected,
            })
        };
        match first {
            Ok(first) => {
                batch.push(first);
                while batch.len() < 256 {
                    match rx.try_recv() {
                        Ok(m) => batch.push(m),
                        Err(_) => break,
                    }
                }
                // Hold bulk messages back; they are written below, one at a time.
                let mut i = 0;
                while i < batch.len() {
                    if batch[i].msg.is_bulk() {
                        bulk.push_back(batch.remove(i).msg);
                    } else {
                        i += 1;
                    }
                }
                coalesce(&mut batch);
                msgs.clear();
                msgs.extend(batch.iter().map(|o| o.msg.clone()));
                if let Err(e) = w.send(&msgs) {
                    if !shared.is_closed() {
                        log::info!("send failed: {e}");
                    }
                    break;
                }
                if measure {
                    let now = mono_us();
                    let mut input = 0;
                    for o in &batch {
                        if o.t_us != 0 {
                            Stats::record(&stats.send_us, now.saturating_sub(o.t_us));
                            input += 1;
                        }
                    }
                    Stats::add(&stats.events_forwarded, input);
                }
                batch.clear();
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
        if shared.is_closed() {
            break;
        }
        if let Some(m) = bulk.pop_front() {
            gate.leave();
            if let Err(e) = w.send(std::slice::from_ref(&m)) {
                if !shared.is_closed() {
                    log::info!("send failed: {e}");
                }
                break;
            }
        }
        if Instant::now() >= next_ping {
            let silent = mono_ms().saturating_sub(shared.last_recv_ms.load(Ordering::Relaxed));
            if silent > DEAD_AFTER.as_millis() as u64 {
                log::warn!("peer silent for {silent} ms; closing connection");
                break;
            }
            ping_id = (ping_id + 1) % PROBE_ID_BASE;
            if let Err(e) = w.send(&[Message::Ping { id: ping_id, t_us: mono_us() }]) {
                if !shared.is_closed() {
                    log::info!("keepalive failed: {e}");
                }
                break;
            }
            next_ping = Instant::now() + PING_INTERVAL;
        }
    }
    shared.close();
    gate.close();
}

/// Sends `count` pings `interval` apart through `outbox` and measures their RTT.
pub fn latency_test(outbox: &Outbox, shared: &ConnShared, count: u32, interval: Duration) -> io::Result<(u32, LatencySummary)> {
    let (tx, rx) = mpsc::channel();
    *shared.probe.lock().unwrap() = Some(tx);
    let mut ring = crate::stats::LatencyRing::default();
    let mut received = 0;
    for i in 0..count {
        if !try_send(outbox, Outgoing::untimed(Message::Ping { id: PROBE_ID_BASE + i, t_us: mono_us() })) {
            break;
        }
        thread::sleep(interval);
        while let Ok((_, rtt)) = rx.try_recv() {
            ring.record(rtt.min(u32::MAX as u64) as u32);
            received += 1;
        }
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    while received < count {
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok((_, rtt)) => {
                ring.record(rtt.min(u32::MAX as u64) as u32);
                received += 1;
            }
            Err(_) => break,
        }
    }
    *shared.probe.lock().unwrap() = None;
    if shared.is_closed() {
        return Err(io::Error::new(io::ErrorKind::NotConnected, "connection closed during the test"));
    }
    Ok((received, ring.summary().scaled(0.001)))
}

/// Handles a keepalive message. Returns true if `msg` was one.
pub(crate) fn handle_keepalive(msg: &Message, shared: &ConnShared, outbox: &Outbox, stats: &Stats) -> bool {
    match *msg {
        Message::Ping { id, t_us } => {
            try_send(outbox, Outgoing::untimed(Message::Pong { id, t_us }));
            true
        }
        Message::Pong { id, t_us } => {
            shared.on_pong(id, t_us, stats);
            true
        }
        _ => false,
    }
}

/// Who we are talking to, established by the handshake and hello exchange.
#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub public: [u8; 32],
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
}

pub(crate) struct AuthCtx<'a> {
    pub trust: &'a TrustStore,
    pub broker: &'a Arc<PairingBroker>,
    pub my_role: PeerRole,
    pub my_name: &'a str,
    pub code: String,
}

#[derive(Debug)]
pub enum AuthError {
    Io(io::Error),
    /// The peer is not what we expect (wrong role or protocol version).
    Incompatible(String),
    RejectedByPeer(String),
    RejectedLocally,
    TimedOut,
}

impl std::fmt::Display for AuthError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AuthError::Io(e) => write!(f, "{e}"),
            AuthError::Incompatible(s) => write!(f, "{s}"),
            AuthError::RejectedByPeer(s) => write!(f, "{s}"),
            AuthError::RejectedLocally => write!(f, "pairing was rejected on this computer"),
            AuthError::TimedOut => write!(f, "pairing was not approved in time"),
        }
    }
}

impl From<io::Error> for AuthError {
    fn from(e: io::Error) -> Self {
        AuthError::Io(e)
    }
}

/// Exchanges `Hello`, then runs pairing if either side does not yet trust the other.
/// Returns the peer and whether it was newly paired.
pub(crate) fn hello_and_authorize(
    r: &mut MsgReader,
    w: MsgWriter,
    ch_remote: [u8; 32],
    addr: SocketAddr,
    ctx: &AuthCtx<'_>,
) -> Result<(PeerInfo, MsgWriter, bool), AuthError> {
    let w = Arc::new(Mutex::new(w));
    let i_trust = ctx.trust.is_trusted(&ch_remote);
    w.lock().unwrap().send(&[Message::Hello {
        version: PROTOCOL_VERSION,
        role: ctx.my_role,
        name: ctx.my_name.to_owned(),
        trusts_peer: i_trust,
    }])?;
    let (name, role, version, peer_trusts) = match r.next_msg()? {
        Message::Hello { version, role, name, trusts_peer } => (name, role, version, trusts_peer),
        Message::Bye { reason } => return Err(AuthError::RejectedByPeer(reason)),
        other => return Err(AuthError::Incompatible(format!("unexpected {other:?} before hello"))),
    };
    let expected = if ctx.my_role == PeerRole::Host { PeerRole::Client } else { PeerRole::Host };
    let bye = |w: &Arc<Mutex<MsgWriter>>, reason: &str| {
        let _ = w.lock().unwrap().send(&[Message::Bye { reason: reason.to_owned() }]);
    };
    if version != PROTOCOL_VERSION {
        let msg = format!("protocol version mismatch (ours {PROTOCOL_VERSION}, theirs {version}); update Moush on both computers");
        bye(&w, &msg);
        return Err(AuthError::Incompatible(msg));
    }
    if role != expected {
        let msg = match role {
            PeerRole::Host => "both computers are set up as host; set one of them to client".to_owned(),
            PeerRole::Client => "both computers are set up as client; set one of them to host".to_owned(),
        };
        bye(&w, &msg);
        return Err(AuthError::Incompatible(msg));
    }
    let peer = PeerInfo { public: ch_remote, id: device_id(&ch_remote), name, addr };

    // Pairing UI is needed on both screens unless both sides already trust each other.
    let mut ticket = None;
    let mut decision = None;
    if i_trust && peer_trusts {
        w.lock().unwrap().send(&[Message::Authorize { accept: true }])?;
    } else {
        let t = ctx.broker.open(
            PairingRequest {
                request_id: String::new(),
                peer_id: peer.id.clone(),
                peer_name: peer.name.clone(),
                peer_address: addr.to_string(),
                peer_role: if role == PeerRole::Host { TrustRole::Host } else { TrustRole::Client },
                code: ctx.code.clone(),
                needs_my_approval: !i_trust,
                expires_at: 0,
            },
            PAIRING_TIMEOUT,
        );
        let sock = r.stream().try_clone()?;
        t.set_abort(move || {
            let _ = sock.shutdown(std::net::Shutdown::Both);
        });
        if i_trust {
            w.lock().unwrap().send(&[Message::Authorize { accept: true }])?;
            ticket = Some(t);
        } else {
            // Wait for the local user on a helper thread while this thread reads the peer.
            let w2 = w.clone();
            let tid = t.id().to_owned();
            decision = Some((tid, thread::spawn(move || {
                let d = t.wait(PAIRING_TIMEOUT);
                let accept = d == Some(true);
                let mut w = w2.lock().unwrap();
                let _ = w.send(&[Message::Authorize { accept }]);
                if !accept {
                    let _ = w.send(&[Message::Bye { reason: "pairing was rejected on the other computer".into() }]);
                }
                (d, t)
            })));
        }
        r.stream().set_read_timeout(Some(PAIRING_TIMEOUT + Duration::from_secs(5)))?;
    }

    // Wait for the peer's decision.
    let peer_result = loop {
        match r.next_msg() {
            Ok(Message::Authorize { accept: true }) => break Ok(()),
            Ok(Message::Authorize { accept: false }) => {
                break Err(AuthError::RejectedByPeer(format!("{} rejected the pairing", peer.name)));
            }
            Ok(Message::Bye { reason }) => break Err(AuthError::RejectedByPeer(reason)),
            Ok(Message::Ping { .. } | Message::Pong { .. }) => continue,
            Ok(other) => break Err(AuthError::Incompatible(format!("unexpected {other:?} during pairing"))),
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => {
                break Err(AuthError::TimedOut);
            }
            Err(e) => break Err(AuthError::Io(e)),
        }
    };
    let mut newly_paired = false;
    if let Some((tid, handle)) = decision {
        let (d, t) = match peer_result {
            Ok(()) => handle.join().map_err(|_| AuthError::RejectedLocally)?,
            Err(e) => {
                // The peer gave up: withdraw the prompt so the helper thread returns.
                ctx.broker.cancel(&tid);
                let _ = handle.join();
                return Err(e);
            }
        };
        drop(t);
        match d {
            Some(true) => newly_paired = true,
            Some(false) => return Err(AuthError::RejectedLocally),
            None => return Err(AuthError::TimedOut),
        }
    } else {
        peer_result?;
    }
    drop(ticket);
    r.stream().set_read_timeout(None)?;
    let w = Arc::try_unwrap(w).map_err(|_| AuthError::Incompatible("writer still shared".into()))?;
    Ok((peer, w.into_inner().unwrap(), newly_paired))
}
