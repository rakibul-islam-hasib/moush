//! Persistent list of paired devices.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::crypto::{device_id, fingerprint, hex, unhex};
use crate::util::now_ms;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TrustRole {
    Host,
    Client,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrustedPeer {
    pub id: String,
    pub public_key: String,
    pub name: String,
    pub role: TrustRole,
    pub paired_at: u64,
    pub last_seen: Option<u64>,
    pub last_address: Option<String>,
}

impl TrustedPeer {
    pub fn fingerprint(&self) -> String {
        unhex(&self.public_key).map(|k| fingerprint(&k)).unwrap_or_default()
    }
}

pub struct TrustStore {
    path: Option<PathBuf>,
    peers: Mutex<Vec<TrustedPeer>>,
}

impl TrustStore {
    /// In-memory store (tests, CLI dry runs).
    pub fn ephemeral() -> Self {
        Self { path: None, peers: Mutex::new(Vec::new()) }
    }

    /// Loads the store from `path`. A missing file is an empty store; a corrupt one is
    /// reported so the user is not silently un-paired.
    pub fn load(path: PathBuf) -> io::Result<Self> {
        let peers = match fs::read_to_string(&path) {
            Ok(s) => serde_json::from_str::<Vec<TrustedPeer>>(&s)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, format!("{}: {e}", path.display())))?
                .into_iter()
                // Never trust an entry whose id does not match its key.
                .filter(|p| unhex(&p.public_key).is_some_and(|k| k.len() == 32 && device_id(&k) == p.id))
                .collect(),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e),
        };
        Ok(Self { path: Some(path), peers: Mutex::new(peers) })
    }

    pub fn list(&self) -> Vec<TrustedPeer> {
        self.peers.lock().unwrap().clone()
    }

    pub fn get(&self, id: &str) -> Option<TrustedPeer> {
        self.peers.lock().unwrap().iter().find(|p| p.id == id).cloned()
    }

    /// True when exactly this key was paired.
    pub fn is_trusted(&self, public_key: &[u8]) -> bool {
        let id = device_id(public_key);
        let key = hex(public_key);
        self.peers.lock().unwrap().iter().any(|p| p.id == id && p.public_key == key)
    }

    pub fn add(&self, public_key: &[u8], name: &str, role: TrustRole, address: Option<String>) -> io::Result<()> {
        let id = device_id(public_key);
        {
            let mut peers = self.peers.lock().unwrap();
            peers.retain(|p| p.id != id);
            peers.push(TrustedPeer {
                id,
                public_key: hex(public_key),
                name: name.to_owned(),
                role,
                paired_at: now_ms(),
                last_seen: Some(now_ms()),
                last_address: address,
            });
        }
        self.save()
    }

    /// Records a successful connection from a trusted peer.
    pub fn touch(&self, id: &str, name: &str, address: Option<String>) -> io::Result<()> {
        {
            let mut peers = self.peers.lock().unwrap();
            let Some(p) = peers.iter_mut().find(|p| p.id == id) else { return Ok(()) };
            p.last_seen = Some(now_ms());
            p.name = name.to_owned();
            if address.is_some() {
                p.last_address = address;
            }
        }
        self.save()
    }

    pub fn remove(&self, id: &str) -> io::Result<bool> {
        let removed = {
            let mut peers = self.peers.lock().unwrap();
            let before = peers.len();
            peers.retain(|p| p.id != id);
            peers.len() != before
        };
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    fn save(&self) -> io::Result<()> {
        let Some(path) = &self.path else { return Ok(()) };
        let json = serde_json::to_string_pretty(&*self.peers.lock().unwrap()).map_err(io::Error::other)?;
        crate::util::write_atomic(path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_check_remove_persist() {
        let dir = std::env::temp_dir().join(format!("moush-trust-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("trusted.json");
        let _ = fs::remove_file(&path);
        let key = [7u8; 32];
        let other = [8u8; 32];
        {
            let t = TrustStore::load(path.clone()).unwrap();
            assert!(!t.is_trusted(&key));
            t.add(&key, "Laptop", TrustRole::Client, Some("10.0.0.2:5000".into())).unwrap();
            assert!(t.is_trusted(&key));
            assert!(!t.is_trusted(&other));
        }
        let t = TrustStore::load(path.clone()).unwrap();
        assert!(t.is_trusted(&key));
        assert_eq!(t.list()[0].name, "Laptop");
        assert!(t.remove(&device_id(&key)).unwrap());
        assert!(!TrustStore::load(path).unwrap().is_trusted(&key));
        let _ = fs::remove_dir_all(dir);
    }
}
