//! Cache of hosts found on the LAN, refreshed on demand by a background scan.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use moush_core::discovery;
use moush_core::util::now_ms;

use crate::host::Notify;

/// Entries not seen for this long are dropped.
const EXPIRE_MS: u64 = 20_000;

#[derive(Clone, Debug)]
pub struct DiscoveredHost {
    pub id: String,
    pub name: String,
    pub addr: SocketAddr,
    pub last_seen: u64,
}

#[derive(Default)]
pub struct DiscoveryCache {
    hosts: Mutex<HashMap<String, DiscoveredHost>>,
    scanning: AtomicBool,
    last_error: Mutex<Option<String>>,
}

impl DiscoveryCache {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn list(&self) -> Vec<DiscoveredHost> {
        let now = now_ms();
        let mut hosts = self.hosts.lock().unwrap();
        hosts.retain(|_, h| now.saturating_sub(h.last_seen) < EXPIRE_MS);
        let mut v: Vec<DiscoveredHost> = hosts.values().cloned().collect();
        v.sort_by(|a, b| a.name.cmp(&b.name));
        v
    }

    pub fn get(&self, id: &str) -> Option<DiscoveredHost> {
        self.hosts.lock().unwrap().get(id).cloned()
    }

    pub fn last_error(&self) -> Option<String> {
        self.last_error.lock().unwrap().clone()
    }

    /// Starts a scan in the background unless one is running; `notify` fires when done.
    pub fn scan_async(self: &Arc<Self>, notify: Notify) {
        if self.scanning.swap(true, Ordering::SeqCst) {
            return;
        }
        let me = self.clone();
        let spawned = thread::Builder::new().name("discovery-scan".into()).spawn(move || {
            match discovery::scan(Duration::from_millis(1200)) {
                Ok(found) => {
                    let now = now_ms();
                    let mut hosts = me.hosts.lock().unwrap();
                    for f in found {
                        hosts.insert(
                            f.announce.id.clone(),
                            DiscoveredHost { id: f.announce.id, name: f.announce.name, addr: f.addr, last_seen: now },
                        );
                    }
                    *me.last_error.lock().unwrap() = None;
                }
                Err(e) => {
                    log::warn!("discovery scan failed: {e}");
                    *me.last_error.lock().unwrap() = Some(e.to_string());
                }
            }
            me.scanning.store(false, Ordering::SeqCst);
            notify();
        });
        if spawned.is_err() {
            self.scanning.store(false, Ordering::SeqCst);
        }
    }
}
