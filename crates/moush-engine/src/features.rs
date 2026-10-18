//! Feature hub: connects optional features (clipboard, file transfer, ...) to whichever
//! peers are connected, independently of the host/client role.
//!
//! The hub outlives engines: the app registers its services once, and the running host
//! or client engine reports peers coming and going and forwards their feature traffic.
//! On a host the peers are the connected clients; on a client the only peer is the host.
//!
//! Capabilities: when a peer comes up, and whenever [`FeatureHub::refresh_caps`] is
//! called after a settings change, the hub tells the peer which features this side
//! allows with it (`features::cap` bits from the policy callback). Services must only
//! *send* when [`FeatureHub::effective_caps`] has the bit, and must re-check their own
//! settings and [`FeatureHub::local_caps`] before acting on anything they *receive*.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use moush_core::net::PeerSender;
use moush_core::protocol::Message;

/// A feature that exchanges messages with peers on one channel.
pub trait FeatureService: Send + Sync + 'static {
    /// The channel this service owns (`moush_core::features::channel::*`).
    fn channel(&self) -> u8;
    /// A peer connected. Its capabilities are not known yet (see `caps_changed`).
    fn peer_up(&self, _peer: &PeerSummary) {}
    /// A peer disconnected: abort anything in progress with it.
    fn peer_down(&self, _peer_id: &str) {}
    /// The effective capabilities with a peer changed (either side's announcement).
    fn caps_changed(&self, _peer_id: &str, _effective: u32) {}
    /// A message on this service's channel. Runs on the connection's reader thread (on a
    /// client, the thread that injects input): must return within microseconds; hand
    /// anything slow (disk, clipboard, UI) to a worker thread.
    fn on_message(&self, peer_id: &str, body: Vec<u8>);
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerSummary {
    pub id: String,
    pub name: String,
    /// Capabilities both sides allow.
    pub effective_caps: u32,
}

struct PeerEntry {
    name: String,
    conn_no: u64,
    sender: PeerSender,
    local_caps: u32,
    remote_caps: u32,
}

type Policy = Box<dyn Fn(&str) -> u32 + Send + Sync>;

pub struct FeatureHub {
    services: RwLock<Vec<Arc<dyn FeatureService>>>,
    peers: Mutex<HashMap<String, PeerEntry>>,
    /// Capability bits this side allows with a peer id.
    policy: RwLock<Policy>,
    /// Serializes `refresh_caps`, so two concurrent refreshes cannot apply an older
    /// evaluation of the policy after a newer one.
    refresh: Mutex<()>,
}

impl Default for FeatureHub {
    fn default() -> Self {
        Self {
            services: RwLock::new(Vec::new()),
            peers: Mutex::new(HashMap::new()),
            policy: RwLock::new(Box::new(|_| 0)),
            refresh: Mutex::new(()),
        }
    }
}

impl FeatureHub {
    /// A hub that allows nothing until [`set_policy`](Self::set_policy) is called.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn set_policy(&self, policy: impl Fn(&str) -> u32 + Send + Sync + 'static) {
        *self.policy.write().unwrap() = Box::new(policy);
    }

    pub fn register(&self, service: Arc<dyn FeatureService>) {
        self.services.write().unwrap().push(service);
    }

    pub fn peers(&self) -> Vec<PeerSummary> {
        let peers = self.peers.lock().unwrap();
        let mut v: Vec<PeerSummary> = peers
            .iter()
            .map(|(id, p)| PeerSummary { id: id.clone(), name: p.name.clone(), effective_caps: p.local_caps & p.remote_caps })
            .collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn peer(&self, id: &str) -> Option<PeerSummary> {
        let peers = self.peers.lock().unwrap();
        peers.get(id).map(|p| PeerSummary { id: id.to_owned(), name: p.name.clone(), effective_caps: p.local_caps & p.remote_caps })
    }

    pub fn sender(&self, id: &str) -> Option<PeerSender> {
        self.peers.lock().unwrap().get(id).map(|p| p.sender.clone())
    }

    /// Capabilities both sides allow with `id` (0 when not connected).
    pub fn effective_caps(&self, id: &str) -> u32 {
        self.peers.lock().unwrap().get(id).map(|p| p.local_caps & p.remote_caps).unwrap_or(0)
    }

    /// Capabilities this side allows with `id` right now (from the policy, not cached).
    pub fn local_caps(&self, id: &str) -> u32 {
        (self.policy.read().unwrap())(id)
    }

    /// Recomputes what this side allows with every peer and re-announces it. Call after
    /// settings or device policies change.
    pub fn refresh_caps(&self) {
        // The policy reads settings and the trust store: evaluate it without holding the
        // peers lock, which reader threads (the client's injection thread) take per message.
        let _serial = self.refresh.lock().unwrap();
        let ids: Vec<String> = self.peers.lock().unwrap().keys().cloned().collect();
        let wanted: Vec<(u32, String)> = ids.into_iter().map(|id| (self.local_caps(&id), id)).collect();
        let mut changed = Vec::new();
        {
            let mut peers = self.peers.lock().unwrap();
            for (caps, id) in wanted {
                let Some(p) = peers.get_mut(&id) else { continue };
                if caps != p.local_caps {
                    let before = p.local_caps & p.remote_caps;
                    p.local_caps = caps;
                    p.sender.send_control(Message::Capabilities { features: caps });
                    if before != caps & p.remote_caps {
                        changed.push((id, caps & p.remote_caps));
                    }
                }
            }
        }
        for (id, eff) in changed {
            for s in self.services() {
                s.caps_changed(&id, eff);
            }
        }
    }

    fn services(&self) -> Vec<Arc<dyn FeatureService>> {
        self.services.read().unwrap().clone()
    }

    // ------------------------------------------------------------- engine side

    /// A peer finished authorization. Announces our capabilities to it.
    pub fn peer_up(&self, id: &str, name: &str, conn_no: u64, sender: PeerSender) {
        let local = self.local_caps(id);
        sender.send_control(Message::Capabilities { features: local });
        let old = self.peers.lock().unwrap().insert(
            id.to_owned(),
            PeerEntry { name: name.to_owned(), conn_no, sender, local_caps: local, remote_caps: 0 },
        );
        let services = self.services();
        if old.is_some() {
            for s in &services {
                s.peer_down(id);
            }
        }
        let summary = PeerSummary { id: id.to_owned(), name: name.to_owned(), effective_caps: 0 };
        for s in &services {
            s.peer_up(&summary);
        }
        // Settings may have changed between computing `local` and inserting the peer (a
        // concurrent refresh_caps would not have seen it yet).
        if self.local_caps(id) != local {
            self.refresh_caps();
        }
    }

    /// A peer disconnected. Ignored if it already reconnected (`conn_no` is stale).
    pub fn peer_down(&self, id: &str, conn_no: u64) {
        let removed = {
            let mut peers = self.peers.lock().unwrap();
            if peers.get(id).is_some_and(|p| p.conn_no == conn_no) {
                peers.remove(id);
                true
            } else {
                false
            }
        };
        if removed {
            for s in self.services() {
                s.peer_down(id);
            }
        }
    }

    pub fn on_capabilities(&self, id: &str, conn_no: u64, features: u32) {
        let eff = {
            let mut peers = self.peers.lock().unwrap();
            let Some(p) = peers.get_mut(id).filter(|p| p.conn_no == conn_no) else { return };
            let before = p.local_caps & p.remote_caps;
            p.remote_caps = features;
            let after = p.local_caps & features;
            // A peer repeating the same announcement (or flooding it) causes no callbacks.
            if after == before {
                return;
            }
            after
        };
        for s in self.services() {
            s.caps_changed(id, eff);
        }
    }

    /// Feature traffic from a peer. Dropped when it comes from a connection that is not
    /// the peer's current one, or on a channel whose features this side does not allow
    /// with that peer (services still re-check their own settings).
    pub fn on_feature(&self, id: &str, conn_no: u64, channel: u8, body: Vec<u8>) {
        let local = match self.peers.lock().unwrap().get(id).filter(|p| p.conn_no == conn_no) {
            Some(p) => p.local_caps,
            None => return,
        };
        if let Some(needed) = channel_caps(channel)
            && local & needed == 0
        {
            log::debug!("dropping feature traffic on channel {channel}: not allowed with this peer");
            return;
        }
        let service = self.services.read().unwrap().iter().find(|s| s.channel() == channel).cloned();
        match service {
            Some(s) => s.on_message(id, body),
            None => log::debug!("ignoring feature traffic on unknown channel {channel}"),
        }
    }
}

/// Capability bits that make traffic on a known channel acceptable (any one of them).
/// `None` for channels the core does not know; their services decide alone.
fn channel_caps(channel: u8) -> Option<u32> {
    use moush_core::features::{cap, channel as ch};
    match channel {
        ch::CLIPBOARD => Some(cap::CLIPBOARD | cap::TEXT_SHARE),
        ch::FILES => Some(cap::FILES),
        _ => None,
    }
}
