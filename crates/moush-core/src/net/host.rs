//! Host side: accepts client connections, authenticates and authorizes them, and keeps
//! one writer thread per client fed by an outbox the input router writes into.

use std::collections::HashMap;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::sync_channel;
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use super::{
    hello_and_authorize, handle_keepalive, run_writer, split_channel, AuthCtx, BulkGate, ConnShared, Outbox, Outgoing,
    PeerInfo, PeerSender, HANDSHAKE_TIMEOUT, OUTBOX_CAPACITY, WRITE_TIMEOUT,
};
use crate::crypto::{handshake, Identity};
use crate::pairing::PairingBroker;
use crate::protocol::{Message, MonitorDesc, PeerRole};
use crate::stats::{LatencySummary, Stats};
use crate::trust::{TrustRole, TrustStore};
use crate::util::now_ms;

const MAX_PENDING: usize = 4;
const MAX_CLIENTS: usize = 8;

/// A client that finished pairing and is ready to receive input.
#[derive(Clone)]
pub struct ClientHandle {
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub screens: Vec<MonitorDesc>,
    pub outbox: Outbox,
    /// Feature and control traffic to this client.
    pub sender: PeerSender,
    /// Increases with every connection; lets the receiver ignore stale disconnects
    /// after a client reconnected.
    pub conn_no: u64,
}

/// Callbacks from connection threads. Must not block.
pub trait HostEvents: Send + Sync + 'static {
    fn client_connected(&self, client: ClientHandle);
    fn client_screens(&self, id: &str, conn_no: u64, screens: Vec<MonitorDesc>);
    fn client_disconnected(&self, id: &str, conn_no: u64, reason: &str);
    /// The client announced which optional features it allows.
    fn client_capabilities(&self, _id: &str, _conn_no: u64, _features: u32) {}
    /// Feature traffic from the client (runs on its reader thread).
    fn client_feature(&self, _id: &str, _conn_no: u64, _channel: u8, _body: Vec<u8>) {}
}

#[derive(Clone, Debug)]
pub struct ClientInfo {
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub screens: Vec<MonitorDesc>,
    pub connected_at: u64,
    pub rtt_ms: Option<f64>,
}

struct Entry {
    info: ClientInfo,
    conn_no: u64,
    outbox: Outbox,
    shared: Arc<ConnShared>,
}

struct Ctx {
    identity: Arc<Identity>,
    name: String,
    trust: Arc<TrustStore>,
    broker: Arc<PairingBroker>,
    stats: Arc<Stats>,
    events: Arc<dyn HostEvents>,
    clients: Mutex<HashMap<String, Entry>>,
    pending: AtomicUsize,
    next_conn: AtomicU64,
    stopping: AtomicBool,
}

pub struct HostServer {
    ctx: Arc<Ctx>,
    addr: SocketAddr,
    accept: Option<JoinHandle<()>>,
}

impl HostServer {
    /// Binds `bind` (normally `0.0.0.0:port`; tests use loopback) and starts accepting clients.
    pub fn start(
        bind: SocketAddrV4,
        name: String,
        identity: Arc<Identity>,
        trust: Arc<TrustStore>,
        broker: Arc<PairingBroker>,
        stats: Arc<Stats>,
        events: Arc<dyn HostEvents>,
    ) -> io::Result<Self> {
        let listener = TcpListener::bind(bind).map_err(|e| {
            if e.kind() == io::ErrorKind::AddrInUse {
                io::Error::new(e.kind(), format!("port {} is already in use by another program", bind.port()))
            } else {
                e
            }
        })?;
        let addr = listener.local_addr()?;
        let ctx = Arc::new(Ctx {
            identity,
            name,
            trust,
            broker,
            stats,
            events,
            clients: Mutex::new(HashMap::new()),
            pending: AtomicUsize::new(0),
            next_conn: AtomicU64::new(1),
            stopping: AtomicBool::new(false),
        });
        let ctx2 = ctx.clone();
        let accept = thread::Builder::new().name("host-accept".into()).spawn(move || accept_loop(listener, ctx2))?;
        log::info!("host listening on {addr}");
        Ok(Self { ctx, addr, accept: Some(accept) })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn clients(&self) -> Vec<ClientInfo> {
        let clients = self.ctx.clients.lock().unwrap();
        let mut v: Vec<ClientInfo> = clients
            .values()
            .map(|e| ClientInfo { rtt_ms: e.shared.rtt_ms(), ..e.info.clone() })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn disconnect(&self, id: &str) -> bool {
        let clients = self.ctx.clients.lock().unwrap();
        match clients.get(id) {
            Some(e) => {
                let _ = super::try_send(&e.outbox, Outgoing::untimed(Message::Bye { reason: super::client::BYE_KICKED.into() }));
                // Give the writer a moment to flush the goodbye, then close.
                let shared = e.shared.clone();
                thread::spawn(move || {
                    thread::sleep(Duration::from_millis(150));
                    shared.close();
                });
                true
            }
            None => false,
        }
    }

    /// Measures round-trip time to a client with `count` pings.
    pub fn latency_test(&self, id: &str, count: u32) -> io::Result<(String, u32, LatencySummary)> {
        let (name, outbox, shared) = {
            let clients = self.ctx.clients.lock().unwrap();
            let e = clients.get(id).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "client is not connected"))?;
            (e.info.name.clone(), e.outbox.clone(), e.shared.clone())
        };
        let (n, s) = super::latency_test(&outbox, &shared, count, Duration::from_millis(10))?;
        Ok((name, n, s))
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if self.ctx.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        for e in self.ctx.clients.lock().unwrap().values() {
            let _ = super::try_send(&e.outbox, Outgoing::untimed(Message::Bye { reason: "the host stopped sharing".into() }));
        }
        thread::sleep(Duration::from_millis(50));
        for e in self.ctx.clients.lock().unwrap().values() {
            e.shared.close();
        }
        // Wake the blocking accept().
        let _ = TcpStream::connect_timeout(&SocketAddr::from((Ipv4Addr::LOCALHOST, self.addr.port())), Duration::from_millis(500));
        if let Some(t) = self.accept.take() {
            let _ = t.join();
        }
        log::info!("host stopped");
    }
}

impl Drop for HostServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn accept_loop(listener: TcpListener, ctx: Arc<Ctx>) {
    for conn in listener.incoming() {
        if ctx.stopping.load(Ordering::SeqCst) {
            break;
        }
        let stream = match conn {
            Ok(s) => s,
            Err(e) => {
                log::warn!("accept failed: {e}");
                thread::sleep(Duration::from_millis(100));
                continue;
            }
        };
        let addr = match stream.peer_addr() {
            Ok(a) => a,
            Err(_) => continue,
        };
        if ctx.pending.load(Ordering::SeqCst) >= MAX_PENDING {
            log::warn!("dropping connection from {addr}: too many pending handshakes");
            continue;
        }
        ctx.pending.fetch_add(1, Ordering::SeqCst);
        let ctx2 = ctx.clone();
        let spawned = thread::Builder::new().name(format!("host-conn-{addr}")).spawn(move || {
            serve(stream, addr, ctx2);
        });
        if let Err(e) = spawned {
            ctx.pending.fetch_sub(1, Ordering::SeqCst);
            log::error!("cannot spawn connection thread: {e}");
        }
    }
}

/// Decrements the pending-handshake counter once, however setup ends.
struct PendingGuard<'a>(&'a AtomicUsize, bool);

impl PendingGuard<'_> {
    fn done(&mut self) {
        if !self.1 {
            self.1 = true;
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

impl Drop for PendingGuard<'_> {
    fn drop(&mut self) {
        self.done();
    }
}

fn serve(mut stream: TcpStream, addr: SocketAddr, ctx: Arc<Ctx>) {
    let mut guard = PendingGuard(&ctx.pending, false);
    log::info!("connection from {addr}");
    let setup = (|| -> Result<_, String> {
        stream.set_nodelay(true).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(|e| e.to_string())?;
        let ch = handshake(&mut stream, &ctx.identity, false).map_err(|e| format!("handshake failed: {e}"))?;
        let remote = *ch.remote_static();
        let code = ch.verification_code();
        let (mut r, w) = split_channel(ch, &stream, &ctx.stats).map_err(|e| e.to_string())?;
        let auth = AuthCtx { trust: &ctx.trust, broker: &ctx.broker, my_role: PeerRole::Host, my_name: &ctx.name, code };
        let (peer, mut w, newly) = hello_and_authorize(&mut r, w, remote, addr, &auth).map_err(|e| e.to_string())?;
        if newly {
            ctx.trust.add(&peer.public, &peer.name, TrustRole::Client, Some(addr.to_string())).map_err(|e| e.to_string())?;
            log::info!("paired with {} ({})", peer.name, peer.id);
        } else {
            let _ = ctx.trust.touch(&peer.id, &peer.name, Some(addr.to_string()));
        }
        // The client describes its monitors right after authorization.
        stream.set_read_timeout(Some(HANDSHAKE_TIMEOUT)).map_err(|e| e.to_string())?;
        let screens = loop {
            match r.next_msg().map_err(|e| format!("waiting for screen info: {e}"))? {
                Message::Screens { monitors } => break monitors,
                Message::Ping { .. } | Message::Pong { .. } => continue,
                other => return Err(format!("unexpected {other:?} instead of screen info")),
            }
        };
        stream.set_read_timeout(None).map_err(|e| e.to_string())?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT)).map_err(|e| e.to_string())?;
        let full = {
            let clients = ctx.clients.lock().unwrap();
            clients.len() >= MAX_CLIENTS && !clients.contains_key(&peer.id)
        };
        if full {
            let _ = w.send(&[Message::Bye { reason: "the host has too many connected computers".into() }]);
            return Err("too many clients".into());
        }
        Ok((peer, r, w, screens))
    })();
    guard.done();
    let (peer, mut r, w, screens) = match setup {
        Ok(v) => v,
        Err(e) => {
            log::warn!("connection from {addr} failed: {e}");
            return;
        }
    };

    let shared = match stream.try_clone() {
        Ok(s) => ConnShared::new(s),
        Err(e) => {
            log::warn!("{e}");
            return;
        }
    };
    let (tx, rx) = sync_channel::<Outgoing>(OUTBOX_CAPACITY);
    let gate = Arc::new(BulkGate::default());
    let writer = {
        let shared = shared.clone();
        let stats = ctx.stats.clone();
        let gate = gate.clone();
        thread::Builder::new()
            .name(format!("host-tx-{}", peer.name))
            .spawn(move || run_writer(w, rx, shared, gate, stats, true))
    };
    if writer.is_err() {
        log::error!("cannot spawn writer thread");
        return;
    }
    let conn_no = ctx.next_conn.fetch_add(1, Ordering::SeqCst);
    let info = ClientInfo {
        id: peer.id.clone(),
        name: peer.name.clone(),
        addr,
        screens: screens.clone(),
        connected_at: now_ms(),
        rtt_ms: None,
    };
    let old = ctx
        .clients
        .lock()
        .unwrap()
        .insert(peer.id.clone(), Entry { info, conn_no, outbox: tx.clone(), shared: shared.clone() });
    if let Some(old) = old {
        log::info!("{} reconnected; closing its previous connection", peer.name);
        old.shared.close();
    }
    log::info!("{} connected from {addr} with {} monitor(s)", peer.name, screens.len());
    ctx.events.client_connected(ClientHandle {
        id: peer.id.clone(),
        name: peer.name.clone(),
        addr,
        screens,
        outbox: tx.clone(),
        sender: PeerSender::new(tx.clone(), gate, shared.clone()),
        conn_no,
    });

    let reason = read_loop(&mut r, &peer, conn_no, &shared, &tx, &ctx);
    shared.close();
    {
        let mut clients = ctx.clients.lock().unwrap();
        if clients.get(&peer.id).is_some_and(|e| e.conn_no == conn_no) {
            clients.remove(&peer.id);
        }
    }
    log::info!("{} disconnected: {reason}", peer.name);
    ctx.events.client_disconnected(&peer.id, conn_no, &reason);
}

fn read_loop(
    r: &mut super::MsgReader,
    peer: &PeerInfo,
    conn_no: u64,
    shared: &Arc<ConnShared>,
    outbox: &Outbox,
    ctx: &Ctx,
) -> String {
    loop {
        let batch = match r.read_batch() {
            Ok((b, _)) => b.to_vec(),
            Err(e) => {
                return if shared.is_closed() { "connection closed".into() } else { format!("connection lost ({e})") };
            }
        };
        shared.on_recv();
        for msg in batch {
            if handle_keepalive(&msg, shared, outbox, &ctx.stats) {
                continue;
            }
            match msg {
                Message::Screens { monitors } => {
                    log::info!("{} reported {} monitor(s)", peer.name, monitors.len());
                    if let Some(e) = ctx.clients.lock().unwrap().get_mut(&peer.id) {
                        e.info.screens = monitors.clone();
                    }
                    ctx.events.client_screens(&peer.id, conn_no, monitors);
                }
                Message::Bye { reason } => return format!("client said goodbye: {reason}"),
                Message::Capabilities { features } => ctx.events.client_capabilities(&peer.id, conn_no, features),
                Message::Feature { channel, body } => ctx.events.client_feature(&peer.id, conn_no, channel, body),
                other => {
                    // A client never sends input; anything else is a protocol violation.
                    log::warn!("{} sent unexpected {:?}; disconnecting", peer.name, other);
                    return "protocol violation".into();
                }
            }
        }
    }
}
