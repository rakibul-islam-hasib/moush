//! LAN discovery over UDP broadcast.
//!
//! A client broadcasts a small probe to every IPv4 interface; hosts answer with a unicast
//! announcement (device id, name, TCP port). Hosts never broadcast on their own, so
//! discovery costs nothing while nobody is looking. Announcements are only hints: the
//! identity of a host is proven by the encrypted handshake, never by this packet.

use std::collections::HashMap;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::protocol::{DISCOVERY_PORT, MAX_STRING, truncate_utf8};

const MAGIC: &[u8; 4] = b"MSHD";
const VERSION: u8 = 1;
const KIND_PROBE: u8 = 1;
const KIND_ANNOUNCE: u8 = 2;
const KIND_STOP: u8 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Announce {
    pub id: String,
    pub name: String,
    pub port: u16,
}

#[derive(Debug, PartialEq, Eq)]
enum Packet {
    Probe,
    Announce(Announce),
    Stop([u8; 8]),
}

fn encode_probe() -> Vec<u8> {
    let mut v = MAGIC.to_vec();
    v.extend([VERSION, KIND_PROBE]);
    v
}

fn encode_announce(a: &Announce) -> Vec<u8> {
    let mut v = MAGIC.to_vec();
    v.extend([VERSION, KIND_ANNOUNCE]);
    let id = truncate_utf8(&a.id, 32);
    v.push(id.len() as u8);
    v.extend_from_slice(id.as_bytes());
    let name = truncate_utf8(&a.name, MAX_STRING);
    v.push(name.len() as u8);
    v.extend_from_slice(name.as_bytes());
    v.extend_from_slice(&a.port.to_le_bytes());
    v
}

fn decode(buf: &[u8]) -> Option<Packet> {
    if buf.len() < 6 || &buf[..4] != MAGIC || buf[4] != VERSION {
        return None;
    }
    let body = &buf[6..];
    match buf[5] {
        KIND_PROBE => Some(Packet::Probe),
        KIND_STOP => Some(Packet::Stop(body.get(..8)?.try_into().ok()?)),
        KIND_ANNOUNCE => {
            let mut pos = 0;
            let mut string = |max: usize| -> Option<String> {
                let n = *body.get(pos)? as usize;
                if n > max {
                    return None;
                }
                let s = std::str::from_utf8(body.get(pos + 1..pos + 1 + n)?).ok()?;
                pos += 1 + n;
                (!s.chars().any(char::is_control)).then(|| s.to_owned())
            };
            let id = string(32)?;
            let name = string(MAX_STRING)?;
            if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit()) {
                return None;
            }
            let port = u16::from_le_bytes(body.get(pos..pos + 2)?.try_into().ok()?);
            (port != 0).then_some(Packet::Announce(Announce { id, name, port }))
        }
        _ => None,
    }
}

/// Token bucket for discovery replies: bursts of [`ReplyLimit::BURST`], refilled at
/// [`ReplyLimit::PER_SEC`]. A client scan sends one probe per interface, far below this.
struct ReplyLimit {
    tokens: f64,
    last: Instant,
}

impl ReplyLimit {
    const BURST: f64 = 32.0;
    const PER_SEC: f64 = 16.0;

    fn new() -> Self {
        Self { tokens: Self::BURST, last: Instant::now() }
    }

    fn allow(&mut self, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.last).as_secs_f64();
        self.last = now;
        self.tokens = (self.tokens + dt * Self::PER_SEC).min(Self::BURST);
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

/// Answers discovery probes until stopped.
pub struct Responder {
    stop: Arc<AtomicBool>,
    token: [u8; 8],
    port: u16,
    thread: Option<JoinHandle<()>>,
}

impl Responder {
    /// Binds the discovery port. Fails if another process (or instance) holds it.
    pub fn start(announce: Announce) -> io::Result<Self> {
        Self::start_on(announce, SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT))
    }

    pub fn start_on(announce: Announce, bind: SocketAddrV4) -> io::Result<Self> {
        let sock = UdpSocket::bind(bind)?;
        let port = sock.local_addr()?.port();
        let stop = Arc::new(AtomicBool::new(false));
        let token: [u8; 8] = crate::util::mono_us().to_le_bytes();
        let reply = encode_announce(&announce);
        let stop2 = stop.clone();
        let thread = thread::Builder::new().name("discovery".into()).spawn(move || {
            let mut buf = [0u8; 512];
            let mut limit = ReplyLimit::new();
            loop {
                let (n, from) = match sock.recv_from(&mut buf) {
                    Ok(v) => v,
                    // Windows reports ICMP "port unreachable" from earlier sends as an error here.
                    Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
                    Err(e) => {
                        log::warn!("discovery socket error: {e}");
                        break;
                    }
                };
                match decode(&buf[..n]) {
                    // Probes are unauthenticated and their source can be spoofed; the
                    // reply is larger than the probe, so cap the reply rate to keep the
                    // host from being used to flood another machine.
                    Some(Packet::Probe) if limit.allow(Instant::now()) => {
                        if let Err(e) = sock.send_to(&reply, from) {
                            log::debug!("discovery reply to {from} failed: {e}");
                        }
                    }
                    Some(Packet::Stop(t)) if t == token && from.ip().is_loopback() => break,
                    _ => {}
                }
                if stop2.load(Ordering::Relaxed) {
                    break;
                }
            }
        })?;
        Ok(Self { stop, token, port, thread: Some(thread) })
    }
}

impl Drop for Responder {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        // Wake the blocking recv with a loopback packet carrying our secret token.
        if let Ok(s) = UdpSocket::bind("127.0.0.1:0") {
            let mut p = MAGIC.to_vec();
            p.extend([VERSION, KIND_STOP]);
            p.extend_from_slice(&self.token);
            let _ = s.send_to(&p, (Ipv4Addr::LOCALHOST, self.port));
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub announce: Announce,
    /// Host address with the announced TCP port.
    pub addr: SocketAddr,
}

/// IPv4 broadcast addresses of all up, non-loopback interfaces plus the limited broadcast.
pub fn broadcast_targets() -> Vec<Ipv4Addr> {
    let mut v = vec![Ipv4Addr::BROADCAST];
    if let Ok(ifaces) = if_addrs::get_if_addrs() {
        for i in ifaces {
            if let if_addrs::IfAddr::V4(a) = i.addr
                && !a.ip.is_loopback()
            {
                let b = a.broadcast.unwrap_or_else(|| Ipv4Addr::from(u32::from(a.ip) | !u32::from(a.netmask)));
                if !v.contains(&b) {
                    v.push(b);
                }
            }
        }
    }
    v
}

/// Local IPv4 addresses, for diagnostics.
pub fn local_addresses() -> Vec<(String, IpAddr)> {
    if_addrs::get_if_addrs()
        .map(|v| {
            v.into_iter().filter(|i| !i.is_loopback() && i.ip().is_ipv4()).map(|i| (i.name.clone(), i.ip())).collect()
        })
        .unwrap_or_default()
}

/// Broadcasts a probe and collects answers for `wait`.
pub fn scan(wait: Duration) -> io::Result<Vec<Found>> {
    scan_on(wait, DISCOVERY_PORT, &broadcast_targets())
}

pub fn scan_on(wait: Duration, port: u16, targets: &[Ipv4Addr]) -> io::Result<Vec<Found>> {
    let sock = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0))?;
    sock.set_broadcast(true)?;
    let probe = encode_probe();
    let mut sent_any = false;
    for t in targets {
        match sock.send_to(&probe, SocketAddrV4::new(*t, port)) {
            Ok(_) => sent_any = true,
            Err(e) => log::debug!("probe to {t} failed: {e}"),
        }
    }
    if !sent_any {
        return Err(io::Error::new(
            io::ErrorKind::NetworkUnreachable,
            "no network interface could send a discovery probe",
        ));
    }
    let deadline = Instant::now() + wait;
    let mut found: HashMap<String, Found> = HashMap::new();
    let mut buf = [0u8; 512];
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        sock.set_read_timeout(Some(left))?;
        match sock.recv_from(&mut buf) {
            Ok((n, from)) => {
                if let Some(Packet::Announce(a)) = decode(&buf[..n]) {
                    let addr = SocketAddr::new(from.ip(), a.port);
                    found.entry(a.id.clone()).or_insert(Found { announce: a, addr });
                }
            }
            Err(e) if matches!(e.kind(), io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut) => break,
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => continue,
            Err(e) => return Err(e),
        }
    }
    let mut v: Vec<Found> = found.into_values().collect();
    v.sort_by(|a, b| a.announce.name.cmp(&b.announce.name));
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_rate_is_limited() {
        let mut l = ReplyLimit::new();
        let t0 = Instant::now();
        let burst = (0..1000).filter(|_| l.allow(t0)).count();
        assert_eq!(burst, ReplyLimit::BURST as usize, "a flood gets at most one burst");
        assert!(!l.allow(t0));
        // One second later the bucket has refilled at the steady rate.
        let t1 = t0 + Duration::from_secs(1);
        let refill = (0..1000).filter(|_| l.allow(t1)).count();
        assert_eq!(refill, ReplyLimit::PER_SEC as usize);
    }

    #[test]
    fn packet_roundtrip_and_validation() {
        let a = Announce { id: "0123456789abcdef".into(), name: "Desk PC".into(), port: 24800 };
        assert_eq!(decode(&encode_announce(&a)), Some(Packet::Announce(a)));
        assert_eq!(decode(&encode_probe()), Some(Packet::Probe));
        assert_eq!(decode(b"XXXX\x01\x01"), None);
        assert_eq!(decode(b"MSHD\x01\x02\x05abc"), None, "truncated");
        let bad_id = Announce { id: "not-hex!".into(), name: "x".into(), port: 1 };
        assert_eq!(decode(&encode_announce(&bad_id)), None);
    }

    #[test]
    fn responder_answers_probe_on_loopback() {
        let r = Responder::start_on(
            Announce { id: "abcd".into(), name: "Host".into(), port: 4242 },
            SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
        )
        .unwrap();
        let found = scan_on(Duration::from_millis(500), r.port, &[Ipv4Addr::LOCALHOST]).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].announce.name, "Host");
        assert_eq!(found[0].addr.port(), 4242);
        drop(r); // must not hang
    }
}
