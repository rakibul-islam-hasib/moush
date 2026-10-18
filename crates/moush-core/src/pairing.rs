//! Pending pairing requests waiting for a user decision.
//!
//! A connection thread registers a request and blocks on its [`PairingTicket`]; the UI
//! lists requests and resolves them with [`PairingBroker::respond`]. Dropping the ticket
//! (connection closed, timeout) removes the request.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;

use crate::trust::TrustRole;
use crate::util::now_ms;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PairingRequest {
    pub request_id: String,
    pub peer_id: String,
    pub peer_name: String,
    pub peer_address: String,
    pub peer_role: TrustRole,
    pub code: String,
    pub needs_my_approval: bool,
    pub expires_at: u64,
}

struct Pending {
    req: PairingRequest,
    tx: Option<Sender<bool>>,
    /// Aborts the connection behind this request (user cancelled while waiting).
    abort: Option<Box<dyn FnOnce() + Send>>,
}

type Notify = Arc<dyn Fn() + Send + Sync>;

pub struct PairingBroker {
    pending: Mutex<Vec<Pending>>,
    next: AtomicU64,
    notify: Notify,
}

impl PairingBroker {
    pub fn new(notify: Notify) -> Arc<Self> {
        Arc::new(Self { pending: Mutex::new(Vec::new()), next: AtomicU64::new(1), notify })
    }

    /// Registers a request. `req.request_id` and `expires_at` are filled in.
    pub fn open(self: &Arc<Self>, mut req: PairingRequest, timeout: Duration) -> PairingTicket {
        let id = format!("p{}", self.next.fetch_add(1, Ordering::Relaxed));
        req.request_id = id.clone();
        req.expires_at = now_ms() + timeout.as_millis() as u64;
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().push(Pending { req, tx: Some(tx), abort: None });
        (self.notify)();
        PairingTicket { broker: self.clone(), id, rx }
    }

    pub fn list(&self) -> Vec<PairingRequest> {
        self.pending.lock().unwrap().iter().map(|p| p.req.clone()).collect()
    }

    /// Delivers the user's decision. Rejecting a request that is only waiting for the
    /// other computer cancels the connection attempt. Returns false if the request no
    /// longer exists or there is nothing to decide.
    pub fn respond(&self, request_id: &str, accept: bool) -> bool {
        let mut pending = self.pending.lock().unwrap();
        let Some(p) = pending.iter_mut().find(|p| p.req.request_id == request_id) else {
            return false;
        };
        if !p.req.needs_my_approval {
            if accept {
                return false;
            }
            let abort = p.abort.take();
            drop(pending);
            if let Some(f) = abort {
                f();
            }
            return true;
        }
        if let Some(tx) = p.tx.take() {
            let _ = tx.send(accept);
        }
        // Keep the entry (now non-actionable) until the connection finishes pairing.
        p.req.needs_my_approval = false;
        drop(pending);
        (self.notify)();
        true
    }

    /// Withdraws a prompt without a decision (the peer disconnected); the waiting
    /// ticket returns `None`.
    pub fn cancel(&self, request_id: &str) {
        let mut pending = self.pending.lock().unwrap();
        if let Some(p) = pending.iter_mut().find(|p| p.req.request_id == request_id) {
            p.tx = None;
            p.req.needs_my_approval = false;
        }
    }

    fn remove(&self, request_id: &str) {
        self.pending.lock().unwrap().retain(|p| p.req.request_id != request_id);
        (self.notify)();
    }
}

pub struct PairingTicket {
    broker: Arc<PairingBroker>,
    id: String,
    rx: Receiver<bool>,
}

impl PairingTicket {
    /// Waits for the user's decision; `None` on timeout.
    pub fn wait(&self, timeout: Duration) -> Option<bool> {
        self.rx.recv_timeout(timeout).ok()
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    /// Sets how to abort the connection if the user cancels while waiting.
    pub fn set_abort(&self, f: impl FnOnce() + Send + 'static) {
        if let Some(p) = self.broker.pending.lock().unwrap().iter_mut().find(|p| p.req.request_id == self.id) {
            p.abort = Some(Box::new(f));
        }
    }
}

impl Drop for PairingTicket {
    fn drop(&mut self) {
        self.broker.remove(&self.id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(needs: bool) -> PairingRequest {
        PairingRequest {
            request_id: String::new(),
            peer_id: "abc".into(),
            peer_name: "Laptop".into(),
            peer_address: "10.0.0.2:1".into(),
            peer_role: TrustRole::Client,
            code: "123456".into(),
            needs_my_approval: needs,
            expires_at: 0,
        }
    }

    #[test]
    fn respond_resolves_ticket_and_drop_removes() {
        let b = PairingBroker::new(Arc::new(|| {}));
        let t = b.open(req(true), Duration::from_secs(60));
        assert_eq!(b.list().len(), 1);
        assert!(b.respond(t.id(), true));
        assert_eq!(t.wait(Duration::from_millis(10)), Some(true));
        assert!(!b.respond(t.id(), true), "already answered");
        drop(t);
        assert!(b.list().is_empty());
    }

    #[test]
    fn waiting_entries_are_not_actionable() {
        let b = PairingBroker::new(Arc::new(|| {}));
        let t = b.open(req(false), Duration::from_secs(60));
        assert!(!b.respond(t.id(), true));
        assert_eq!(t.wait(Duration::from_millis(5)), None);
        // Rejecting a waiting entry runs its abort hook.
        let aborted = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let a2 = aborted.clone();
        t.set_abort(move || a2.store(true, std::sync::atomic::Ordering::SeqCst));
        assert!(b.respond(t.id(), false));
        assert!(aborted.load(std::sync::atomic::Ordering::SeqCst));
    }
}
