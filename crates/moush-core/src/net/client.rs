//! Client side: one session = connect, handshake, pair, then inject everything the host
//! sends until the connection ends. Reconnection policy lives in the caller.

use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use super::{
    AuthCtx, AuthError, BulkGate, ConnShared, HANDSHAKE_TIMEOUT, OUTBOX_CAPACITY, Outbox, Outgoing, PeerInfo,
    PeerSender, SETUP_DEADLINE, SetupDeadline, WRITE_TIMEOUT, handle_keepalive, hello_and_authorize, run_writer,
    split_channel, try_send,
};
use crate::crypto::{Identity, handshake};
use crate::inject::{InjectState, Injector};
use crate::pairing::PairingBroker;
use crate::protocol::{Message, MonitorDesc, PeerRole};
use crate::stats::{LatencySummary, Stats};
use crate::trust::{TrustRole, TrustStore};
use crate::util::mono_us;

pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
/// Goodbye reason the host uses when the user disconnects a client on purpose.
pub const BYE_KICKED: &str = "disconnected by the host";

/// Notifications from a running session. Must not block: they run on the thread that
/// injects input.
pub trait ClientEvents: Send + Sync {
    /// The session is up; `sender` carries feature and control traffic to the host.
    fn connected(&self, host: &PeerInfo, sender: &PeerSender);
    fn active_changed(&self, active: bool);
    /// The session that `connected` announced has ended.
    fn disconnected(&self) {}
    /// The host announced which optional features it allows.
    fn capabilities(&self, _features: u32) {}
    /// Feature traffic from the host.
    fn feature(&self, _channel: u8, _body: Vec<u8>) {}
}

/// Handle to the current session for other threads (UI, display-change watcher).
#[derive(Default)]
pub struct ClientLink {
    conn: Mutex<Option<(Outbox, Arc<ConnShared>)>>,
    /// Socket of a session still in setup, so `close` can interrupt a pairing wait.
    setup: Mutex<Option<TcpStream>>,
}

impl ClientLink {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn is_connected(&self) -> bool {
        self.conn.lock().unwrap().as_ref().is_some_and(|(_, s)| !s.is_closed())
    }

    pub fn rtt_ms(&self) -> Option<f64> {
        self.conn.lock().unwrap().as_ref().and_then(|(_, s)| s.rtt_ms())
    }

    pub fn send_screens(&self, monitors: Vec<MonitorDesc>) -> bool {
        match &*self.conn.lock().unwrap() {
            Some((outbox, _)) => try_send(outbox, Outgoing::untimed(Message::Screens { monitors })),
            None => false,
        }
    }

    pub fn latency_test(&self, count: u32) -> io::Result<(u32, LatencySummary)> {
        let (outbox, shared) = self
            .conn
            .lock()
            .unwrap()
            .clone()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "not connected to a host"))?;
        super::latency_test(&outbox, &shared, count, Duration::from_millis(10))
    }

    /// Ends the current session (or interrupts one being set up).
    pub fn close(&self) {
        if let Some((outbox, shared)) = &*self.conn.lock().unwrap() {
            let _ = try_send(outbox, Outgoing::untimed(Message::Bye { reason: "the client disconnected".into() }));
            let shared = shared.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                shared.close();
            });
        }
        if let Some(s) = &*self.setup.lock().unwrap() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionEnd {
    /// The connection ended; retrying makes sense.
    Lost(String),
    /// Retrying would not help (pairing rejected, incompatible peer, identity changed).
    Fatal(String),
    /// Stopped on request.
    Stopped,
}

pub struct SessionConfig<'a> {
    pub addr: SocketAddr,
    /// Device id we expect to reach (reconnecting to a known host). A different
    /// device at that address is refused.
    pub expected_host: Option<String>,
    pub identity: &'a Identity,
    pub name: &'a str,
    pub trust: &'a TrustStore,
    pub broker: &'a Arc<PairingBroker>,
    pub stats: &'a Arc<Stats>,
    pub screens: Vec<MonitorDesc>,
    pub stop: &'a AtomicBool,
}

pub fn run_session<I: Injector>(
    cfg: SessionConfig<'_>,
    inject: &mut InjectState<I>,
    link: &ClientLink,
    events: &dyn ClientEvents,
) -> SessionEnd {
    let stopped = || cfg.stop.load(Ordering::SeqCst);
    let mut stream = match TcpStream::connect_timeout(&cfg.addr, CONNECT_TIMEOUT) {
        Ok(s) => s,
        Err(e) => return SessionEnd::Lost(describe_connect_error(&e, cfg.addr)),
    };
    if stopped() {
        return SessionEnd::Stopped;
    }
    *link.setup.lock().unwrap() = stream.try_clone().ok();
    let result = setup(&mut stream, &cfg);
    *link.setup.lock().unwrap() = None;
    let (peer, mut r, w) = match result {
        Ok(v) => v,
        Err(_) if stopped() => return SessionEnd::Stopped,
        Err(e) => return e,
    };

    let shared = match stream.try_clone() {
        Ok(s) => ConnShared::new(s),
        Err(e) => return SessionEnd::Lost(e.to_string()),
    };
    let (tx, rx) = sync_channel::<Outgoing>(OUTBOX_CAPACITY);
    let gate = Arc::new(BulkGate::default());
    {
        let shared = shared.clone();
        let stats = cfg.stats.clone();
        let gate = gate.clone();
        if let Err(e) =
            thread::Builder::new().name("client-tx".into()).spawn(move || run_writer(w, rx, shared, gate, stats, false))
        {
            return SessionEnd::Lost(format!("cannot start writer: {e}"));
        }
    }
    *link.conn.lock().unwrap() = Some((tx.clone(), shared.clone()));
    if stopped() {
        shared.close();
    }
    log::info!("connected to {} at {}", peer.name, cfg.addr);
    events.connected(&peer, &PeerSender::new(tx.clone(), gate, shared.clone()));

    let end = loop {
        let (batch, t0) = match r.read_batch() {
            Ok(v) => v,
            Err(e) => {
                break if stopped() {
                    SessionEnd::Stopped
                } else if shared.is_closed() {
                    SessionEnd::Lost("connection timed out".into())
                } else {
                    SessionEnd::Lost(format!("connection lost ({e})"))
                };
            }
        };
        shared.on_recv();
        let mut input = false;
        let mut violation = None;
        let mut bye = None;
        for msg in batch {
            if handle_keepalive(msg, &shared, &tx, cfg.stats) {
                continue;
            }
            match msg {
                Message::Enter { .. }
                | Message::Leave
                | Message::MouseMove { .. }
                | Message::MouseButton { .. }
                | Message::Wheel { .. }
                | Message::Key { .. }
                | Message::ReleaseAll => {
                    let was_active = inject.active();
                    let res = inject.apply(msg);
                    Stats::add(&cfg.stats.events_injected, res.injected as u64);
                    if res.failed > 0 {
                        Stats::add(&cfg.stats.inject_failures, res.failed as u64);
                    }
                    if inject.active() != was_active {
                        events.active_changed(inject.active());
                    }
                    input = true;
                }
                Message::Bye { reason } => bye = Some(reason.clone()),
                Message::Capabilities { features } => events.capabilities(*features),
                Message::Feature { channel, body } => events.feature(*channel, body.clone()),
                other => violation = Some(format!("{other:?}")),
            }
        }
        if input {
            Stats::record(&cfg.stats.inject_us, mono_us().saturating_sub(t0));
        }
        if let Some(v) = violation {
            log::warn!("host sent unexpected {v}; disconnecting");
            break SessionEnd::Fatal("the host sent an invalid message".into());
        }
        if let Some(reason) = bye {
            break if reason == BYE_KICKED { SessionEnd::Fatal(reason) } else { SessionEnd::Lost(reason) };
        }
    };
    shared.close();
    *link.conn.lock().unwrap() = None;
    let was_active = inject.active();
    inject.reset();
    if was_active {
        events.active_changed(false);
    }
    events.disconnected();
    end
}

fn setup(
    stream: &mut TcpStream,
    cfg: &SessionConfig<'_>,
) -> Result<(PeerInfo, super::MsgReader, super::MsgWriter), SessionEnd> {
    let lost = |e: io::Error| SessionEnd::Lost(e.to_string());
    stream.set_nodelay(true).map_err(lost)?;
    stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(lost)?;
    stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(lost)?;
    let deadline = SetupDeadline::arm(stream, SETUP_DEADLINE).map_err(lost)?;
    let ch =
        handshake(stream, cfg.identity, true).map_err(|e| SessionEnd::Lost(format!("secure handshake failed: {e}")))?;
    let remote = *ch.remote_static();
    let remote_id = crate::crypto::device_id(&remote);
    if let Some(expected) = &cfg.expected_host
        && *expected != remote_id
    {
        return Err(SessionEnd::Fatal(format!(
            "a different computer answered at {} (expected device {expected}, got {remote_id})",
            cfg.addr
        )));
    }
    let code = ch.verification_code();
    let (mut r, w) = split_channel(ch, stream, cfg.stats).map_err(lost)?;
    let auth = AuthCtx {
        trust: cfg.trust,
        broker: cfg.broker,
        my_role: PeerRole::Client,
        my_name: cfg.name,
        code,
        deadline: Some(&deadline),
        on_hello: None,
    };
    let (peer, mut w, newly) = hello_and_authorize(&mut r, w, remote, cfg.addr, &auth).map_err(|e| match e {
        AuthError::Io(e) => SessionEnd::Lost(e.to_string()),
        other => SessionEnd::Fatal(other.to_string()),
    })?;
    if newly {
        cfg.trust
            .add(&peer.public, &peer.name, TrustRole::Host, Some(cfg.addr.to_string()))
            .map_err(|e| SessionEnd::Fatal(format!("cannot save the paired host: {e}")))?;
        log::info!("paired with host {} ({})", peer.name, peer.id);
    } else {
        let _ = cfg.trust.touch(&peer.id, &peer.name, Some(cfg.addr.to_string()));
    }
    w.send(&[Message::Screens { monitors: cfg.screens.clone() }]).map_err(lost)?;
    stream.set_read_timeout(None).map_err(lost)?;
    stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(lost)?;
    Ok((peer, r, w))
}

fn describe_connect_error(e: &io::Error, addr: SocketAddr) -> String {
    match e.kind() {
        io::ErrorKind::ConnectionRefused => {
            format!("{addr} refused the connection; is Moush running in host mode there?")
        }
        io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock => format!(
            "no answer from {addr}; check that both computers are on the same network and that Windows Firewall allows Moush on the host"
        ),
        io::ErrorKind::HostUnreachable | io::ErrorKind::NetworkUnreachable => {
            format!("{addr} is unreachable from this network")
        }
        _ => format!("cannot connect to {addr}: {e}"),
    }
}
