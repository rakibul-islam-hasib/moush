//! File transfer service on channel [`channel::FILES`]: sends files from disk and receives
//! them into the download folder, with progress, cancellation from either side, resume
//! after a failure, integrity checks and per-device authorization.
//!
//! Threads: `on_message` (a connection's reader thread) only decodes, releases flow-control
//! credit for acks and hands everything else to a bounded queue. One control thread
//! assembles incoming offers and runs timers; every active transfer has its own worker
//! thread (at most [`MAX_ACTIVE_PER_PEER`] per peer and direction; further outgoing
//! transfers wait in a queue). Files are streamed in [`tr::CHUNK`]-sized pieces; memory
//! use per transfer is one chunk buffer plus the bounded queues, whatever the file size.
//!
//! Receiving: data goes to `<name>.<tag>.moushpart` in the destination folder; after the
//! sender's `FileDone` the file is flushed to disk, its BLAKE2s-256 checked, renamed into
//! place (never replacing an existing file unless the overwrite policy says so) and marked
//! as downloaded from the internet (Zone.Identifier stream), so Windows applies the same
//! checks as for a browser download. Received files are never opened or executed.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::os::windows::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf, Prefix};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::sync::{Arc, Mutex, OnceLock, RwLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use moush_core::features::{cap, channel, TransferSettings};
use moush_core::flow::{Acquire, Window};
use moush_core::transfer::{self as tr, AcceptAssembler, FileEntry, Msg, OfferAssembler, RateMeter, ReceiveFile, Resume, Start, WriteAt};
use moush_core::util::{mono_ms, now_ms};

use crate::features::{FeatureHub, FeatureService};

/// Outgoing transfers that run at the same time with one peer; more wait in a queue.
pub const MAX_ACTIVE_PER_PEER: usize = 4;
/// Incoming transfers (waiting for approval or running) accepted from one peer at a time.
pub const MAX_INCOMING_PER_PEER: usize = 4;
/// Outgoing transfers that may be queued or running in total.
const MAX_OUTGOING: usize = 64;
/// Finished transfers kept for display.
const HISTORY: usize = 50;
/// An offer waiting for the user is declined after this long.
pub const APPROVAL_TIMEOUT: Duration = Duration::from_secs(120);
/// The sender gives up waiting for an answer after this long.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(150);
/// All parts of an offer must arrive within this time.
const ASSEMBLY_TIMEOUT: Duration = Duration::from_secs(30);
/// A transfer without progress for this long fails.
const IDLE_TIMEOUT: Duration = Duration::from_secs(60);
/// Longest wait to queue one message on the connection or to get flow-control credit.
const SEND_TIMEOUT: Duration = Duration::from_secs(30);
/// Messages waiting for one transfer's worker. A sender stays far below this: its window
/// allows at most 32 chunks in flight.
const INPUT_QUEUE: usize = 256;
/// Free space kept on the destination drive on top of the incoming data.
const DISK_RESERVE: u64 = 64 * 1024 * 1024;
/// Snapshot requests caused by progress alone (not state changes) per second, at most.
const PROGRESS_INTERVAL_MS: u64 = 250;
/// Partial files older than this are removed from the download folder.
const STALE_PART_AGE: Duration = Duration::from_secs(7 * 24 * 3600);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    /// Waiting for the user's approval (receive), for the receiver's answer or for a free
    /// slot (send).
    Pending,
    Active,
    Done,
    Failed,
    Cancelled,
    Rejected,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Pending => "pending",
            State::Active => "active",
            State::Done => "done",
            State::Failed => "failed",
            State::Cancelled => "cancelled",
            State::Rejected => "rejected",
        }
    }

    pub fn is_final(self) -> bool {
        !matches!(self, State::Pending | State::Active)
    }
}

/// A transfer as shown to the user.
#[derive(Clone, Debug)]
pub struct TransferInfo {
    pub id: String,
    pub outgoing: bool,
    pub peer_id: String,
    pub peer_name: String,
    /// Offered names (send) or the names the files are saved under (receive).
    pub files: Vec<FileEntry>,
    pub total_bytes: u64,
    pub done_bytes: u64,
    pub bytes_per_sec: f64,
    pub state: State,
    pub error: Option<String>,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    /// Receive: the folder the files are saved to.
    pub destination: Option<PathBuf>,
    pub can_retry: bool,
    /// Receive: waiting for the user to accept or decline.
    pub awaiting_approval: bool,
}

/// Something the app may want to tell the user about.
#[derive(Clone, Debug)]
pub enum Event {
    /// Any visible state changed (progress is throttled to 4 per second).
    Changed,
    /// A peer offered files. `needs_approval` is false when they were accepted automatically.
    Incoming { id: String, peer_name: String, files: usize, bytes: u64, needs_approval: bool },
    /// A transfer reached a final state.
    Finished(TransferInfo),
}

type SettingsFn = Box<dyn Fn() -> TransferSettings + Send + Sync>;
type EventFn = Box<dyn Fn(Event) + Send + Sync>;

/// Why a running transfer was stopped from outside its worker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stop {
    /// Cancelled by either side: partial files are deleted.
    Cancelled,
    /// Connection lost, rejected or failed: partial files are kept for a retry.
    Halted,
}

enum Input {
    Peer(Msg),
    Wake,
}

/// One run of a transfer's worker (a retry starts a new attempt).
struct Attempt {
    tx: SyncSender<Input>,
    window: Window,
    stop: Mutex<Option<Stop>>,
}

impl Attempt {
    fn stop(&self) -> Option<Stop> {
        *self.stop.lock().unwrap()
    }

    fn halt(&self, why: Stop) {
        self.stop.lock().unwrap().get_or_insert(why);
        self.window.close();
        let _ = self.tx.try_send(Input::Wake);
    }
}

struct Status {
    files: Vec<FileEntry>,
    /// Send: the files on disk.
    sources: Vec<PathBuf>,
    total: u64,
    state: State,
    error: Option<String>,
    finished_at: Option<u64>,
    destination: Option<PathBuf>,
    /// The file to select in Explorer.
    reveal: Option<PathBuf>,
    meter: RateMeter,
    /// Receive: the user has to answer before this instant.
    approval_deadline: Option<Instant>,
    /// Send: waiting for a free slot.
    queued: bool,
}

struct Transfer {
    id: u64,
    ui_id: String,
    outgoing: bool,
    peer_id: String,
    peer_name: String,
    started_at: u64,
    st: Mutex<Status>,
    /// Bytes acknowledged by the receiver (send) or written to disk (receive).
    done: AtomicU64,
    /// A worker thread owns this transfer.
    running: AtomicBool,
    attempt: Mutex<Option<Arc<Attempt>>>,
}

impl Transfer {
    fn state(&self) -> State {
        self.st.lock().unwrap().state
    }

    fn attempt(&self) -> Option<Arc<Attempt>> {
        self.attempt.lock().unwrap().clone()
    }

    fn info(&self) -> TransferInfo {
        let mut s = self.st.lock().unwrap();
        let done = self.done.load(Ordering::Relaxed).min(s.total);
        let rate = if s.state == State::Active { s.meter.update(done, mono_ms()) } else { 0.0 };
        let running = self.running.load(Ordering::SeqCst);
        TransferInfo {
            id: self.ui_id.clone(),
            outgoing: self.outgoing,
            peer_id: self.peer_id.clone(),
            peer_name: self.peer_name.clone(),
            files: s.files.clone(),
            total_bytes: s.total,
            done_bytes: done,
            bytes_per_sec: rate,
            state: s.state,
            error: s.error.clone(),
            started_at: self.started_at,
            finished_at: s.finished_at,
            destination: s.destination.as_deref().map(display_path),
            can_retry: self.outgoing && matches!(s.state, State::Failed | State::Cancelled) && !running,
            awaiting_approval: s.approval_deadline.is_some() && s.state == State::Pending,
        }
    }
}

struct Inner {
    hub: OnceLock<Weak<FeatureHub>>,
    default_dir: PathBuf,
    settings: RwLock<SettingsFn>,
    events: RwLock<EventFn>,
    /// Newest first: running, queued and the last [`HISTORY`] finished transfers.
    transfers: Mutex<Vec<Arc<Transfer>>>,
    /// Offers whose parts are still arriving, by (peer, id).
    assembling: Mutex<HashMap<(String, u64), (OfferAssembler, Instant)>>,
    control: SyncSender<Control>,
    last_progress_ms: AtomicU64,
}

enum Control {
    Offer(String, Msg),
    Event(Event),
}

/// File transfer with all connected peers: create it, set its handlers, then
/// [`attach`](Self::attach) it to the [`FeatureHub`].
pub struct TransferService {
    inner: Arc<Inner>,
}

impl TransferService {
    /// `default_dir` is used when `TransferSettings::download_dir` is empty.
    pub fn new(default_dir: PathBuf) -> Arc<Self> {
        let (control, rx) = mpsc::sync_channel(256);
        let inner = Arc::new(Inner {
            hub: OnceLock::new(),
            default_dir,
            settings: RwLock::new(Box::new(TransferSettings::default)),
            events: RwLock::new(Box::new(|_| {})),
            transfers: Mutex::new(Vec::new()),
            assembling: Mutex::new(HashMap::new()),
            control,
            last_progress_ms: AtomicU64::new(0),
        });
        let weak = Arc::downgrade(&inner);
        let _ = thread::Builder::new().name("transfer-control".into()).spawn(move || control_loop(weak, rx));
        Arc::new(Self { inner })
    }

    /// Registers the service with `hub` (once).
    pub fn attach(self: &Arc<Self>, hub: &Arc<FeatureHub>) {
        if self.inner.hub.set(Arc::downgrade(hub)).is_ok() {
            hub.register(self.clone());
        }
    }

    /// Where settings are read from (at the moment they are needed) and where events go.
    pub fn set_handlers(&self, settings: impl Fn() -> TransferSettings + Send + Sync + 'static, events: impl Fn(Event) + Send + Sync + 'static) {
        *self.inner.settings.write().unwrap() = Box::new(settings);
        *self.inner.events.write().unwrap() = Box::new(events);
    }

    /// Active, pending and recent transfers, newest first.
    pub fn list(&self) -> Vec<TransferInfo> {
        let list = self.inner.transfers.lock().unwrap().clone();
        list.iter().map(|t| t.info()).collect()
    }

    pub fn get(&self, id: &str) -> Option<TransferInfo> {
        self.inner.by_ui_id(id).map(|t| t.info())
    }

    /// Queues files for sending to `peer_id`; returns the transfer id.
    pub fn send(&self, peer_id: &str, paths: &[PathBuf]) -> Result<String, String> {
        let inner = &self.inner;
        let peer = inner.sendable_peer(peer_id)?;
        let (files, sources) = stat_sources(paths)?;
        let outgoing = inner.transfers.lock().unwrap().iter().filter(|t| t.outgoing && !t.state().is_final()).count();
        if outgoing >= MAX_OUTGOING {
            return Err("Too many transfers are waiting. Try again when some have finished.".into());
        }
        let id = loop {
            let id = new_id();
            if inner.find(peer_id, id).is_none() {
                break id;
            }
        };
        log::info!("transfer s{id:016x}: sending {} file(s), {} bytes", files.len(), files.iter().map(|f| f.size).sum::<u64>());
        let t = Arc::new(Transfer {
            id,
            ui_id: format!("s{id:016x}"),
            outgoing: true,
            peer_id: peer_id.to_owned(),
            peer_name: peer.name,
            started_at: now_ms(),
            st: Mutex::new(Status {
                total: files.iter().map(|f| f.size).sum(),
                files,
                sources,
                state: State::Pending,
                error: None,
                finished_at: None,
                destination: None,
                reveal: paths.first().cloned(),
                meter: RateMeter::default(),
                approval_deadline: None,
                queued: true,
            }),
            done: AtomicU64::new(0),
            running: AtomicBool::new(false),
            attempt: Mutex::new(None),
        });
        inner.insert(t.clone());
        inner.schedule();
        inner.changed();
        Ok(t.ui_id.clone())
    }

    /// Answers an incoming offer that waits for approval.
    pub fn respond(&self, id: &str, accept: bool) -> Result<(), String> {
        let t = self.inner.by_ui_id(id).ok_or("This transfer no longer exists.")?;
        let waiting = {
            let s = t.st.lock().unwrap();
            !t.outgoing && s.state == State::Pending && s.approval_deadline.is_some()
        };
        if !waiting {
            return Err("This transfer is no longer waiting for an answer.".into());
        }
        if accept {
            if !self.inner.receive_allowed(&t.peer_id) {
                self.inner.reject(&t, "File transfer is turned off on the receiving computer.", Some("File transfer is turned off.".into()));
                return Err("File transfer is turned off for this device.".into());
            }
            self.inner.start_receiver(&t);
        } else {
            self.inner.reject(&t, "The receiver declined the files.", None);
        }
        Ok(())
    }

    /// Cancels a transfer in either direction (declines one that waits for approval).
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let inner = &self.inner;
        let t = inner.by_ui_id(id).ok_or("This transfer no longer exists.")?;
        let (state, awaiting) = {
            let s = t.st.lock().unwrap();
            (s.state, s.approval_deadline.is_some())
        };
        if state.is_final() {
            return Ok(());
        }
        if !t.outgoing && state == State::Pending && awaiting {
            inner.reject(&t, "The receiver declined the files.", None);
            return Ok(());
        }
        if t.attempt().is_some() || !t.outgoing {
            inner.tell(&t.peer_id, Msg::Cancel { id: t.id, reason: "Cancelled by the other computer.".into() });
        }
        inner.stop(&t, State::Cancelled, None, Stop::Cancelled);
        Ok(())
    }

    /// Sends a failed or cancelled outgoing transfer again; the receiver continues from the
    /// data it already has.
    pub fn retry(&self, id: &str) -> Result<(), String> {
        let inner = &self.inner;
        let t = inner.by_ui_id(id).ok_or("This transfer no longer exists.")?;
        if !t.outgoing || t.running.load(Ordering::SeqCst) || !matches!(t.state(), State::Failed | State::Cancelled) {
            return Err("Only failed or cancelled transfers you sent can be retried.".into());
        }
        inner.sendable_peer(&t.peer_id)?;
        let sources = t.st.lock().unwrap().sources.clone();
        let (files, _) = stat_sources(&sources)?;
        {
            let mut s = t.st.lock().unwrap();
            if s.files.iter().map(|f| &f.name).ne(files.iter().map(|f| &f.name)) {
                return Err("The files changed; send them again.".into());
            }
            s.total = files.iter().map(|f| f.size).sum();
            s.files = files;
            s.state = State::Pending;
            s.error = None;
            s.finished_at = None;
            s.meter.reset();
            s.queued = true;
        }
        t.done.store(0, Ordering::Relaxed);
        log::info!("transfer {}: retrying", t.ui_id);
        inner.schedule();
        inner.changed();
        Ok(())
    }

    /// Removes finished transfers from the list.
    pub fn clear_finished(&self) {
        self.inner.transfers.lock().unwrap().retain(|t| !t.state().is_final() || t.running.load(Ordering::SeqCst));
        self.inner.changed();
    }

    /// The file or folder to show in Explorer for a transfer.
    pub fn location(&self, id: &str) -> Option<PathBuf> {
        let t = self.inner.by_ui_id(id)?;
        let s = t.st.lock().unwrap();
        s.reveal.clone().filter(|p| p.exists()).or_else(|| s.destination.clone()).map(|p| display_path(&p))
    }

    /// The folder files are received into right now (validated and created).
    pub fn download_dir(&self) -> Result<PathBuf, String> {
        resolve_download_dir(&self.inner.settings().download_dir, &self.inner.default_dir).map(|p| display_path(&p))
    }
}

impl FeatureService for TransferService {
    fn channel(&self) -> u8 {
        channel::FILES
    }

    fn peer_down(&self, peer_id: &str) {
        let inner = &self.inner;
        inner.assembling.lock().unwrap().retain(|(p, _), _| p != peer_id);
        for t in inner.with_peer(peer_id) {
            let msg = format!("Lost the connection to {}.", t.peer_name);
            inner.stop(&t, State::Failed, Some(msg), Stop::Halted);
        }
    }

    fn caps_changed(&self, peer_id: &str, effective: u32) {
        if effective & cap::FILES != 0 {
            return;
        }
        let inner = &self.inner;
        for t in inner.with_peer(peer_id) {
            inner.stop(&t, State::Failed, Some("File transfer was turned off.".into()), Stop::Halted);
        }
    }

    fn on_message(&self, peer_id: &str, body: Vec<u8>) {
        let msg = match Msg::decode(&body) {
            Ok(m) => m,
            Err(e) => {
                // Rate-limited: this runs on the connection's reader thread.
                static LAST_WARN_MS: AtomicU64 = AtomicU64::new(0);
                let (now, last) = (mono_ms(), LAST_WARN_MS.load(Ordering::Relaxed));
                if (last == 0 || now.saturating_sub(last) > 10_000)
                    && LAST_WARN_MS.compare_exchange(last, now.max(1), Ordering::Relaxed, Ordering::Relaxed).is_ok()
                {
                    log::warn!("ignoring a malformed file transfer message: {e:?}");
                }
                return;
            }
        };
        let inner = &self.inner;
        if let Msg::Offer { .. } = msg {
            if inner.control.try_send(Control::Offer(peer_id.to_owned(), msg)).is_err() {
                log::warn!("file transfer offers arrive too fast; dropping one");
            }
            return;
        }
        let Some(t) = inner.find(peer_id, msg.id()) else {
            log::debug!("file transfer message for an unknown transfer");
            return;
        };
        match msg {
            Msg::Ack { bytes, .. } if t.outgoing => {
                if let Some(a) = t.attempt() {
                    a.window.release(bytes as usize);
                    t.done.fetch_add(bytes as u64, Ordering::Relaxed);
                    inner.progress();
                }
            }
            // State changes are logged once by `finish`.
            Msg::Cancel { .. } => {
                let why = format!("Cancelled on {}.", t.peer_name);
                inner.stop(&t, State::Cancelled, Some(why), Stop::Cancelled);
            }
            Msg::Reject { reason, .. } if t.outgoing => {
                inner.stop(&t, State::Rejected, Some(peer_text(&t.peer_name, &reason)), Stop::Halted);
            }
            Msg::Error { reason, .. } => {
                inner.stop(&t, State::Failed, Some(peer_text(&t.peer_name, &reason)), Stop::Halted);
            }
            m @ (Msg::Accept { .. } | Msg::Complete { .. }) if t.outgoing => inner.route(&t, m),
            m @ (Msg::Data { .. } | Msg::FileDone { .. }) if !t.outgoing => inner.route(&t, m),
            _ => log::debug!("unexpected file transfer message"),
        }
    }
}

impl Drop for TransferService {
    fn drop(&mut self) {
        for t in self.inner.transfers.lock().unwrap().iter() {
            if let Some(a) = t.attempt() {
                a.halt(Stop::Halted);
            }
        }
    }
}

fn peer_text(peer: &str, reason: &str) -> String {
    if reason.trim().is_empty() { format!("{peer} stopped the transfer.") } else { format!("{peer}: {reason}") }
}

impl Inner {
    fn hub(&self) -> Option<Arc<FeatureHub>> {
        self.hub.get().and_then(Weak::upgrade)
    }

    fn settings(&self) -> TransferSettings {
        (self.settings.read().unwrap())()
    }

    fn emit(&self, e: Event) {
        (self.events.read().unwrap())(e)
    }

    /// Final-state and offer events go through the control thread, so reader threads never
    /// run the app's notification code.
    fn emit_later(&self, e: Event) {
        if self.control.try_send(Control::Event(e)).is_err() {
            log::debug!("transfer event dropped");
        }
    }

    fn changed(&self) {
        self.last_progress_ms.store(mono_ms(), Ordering::Relaxed);
        self.emit(Event::Changed);
    }

    /// Progress update, throttled to one snapshot request per [`PROGRESS_INTERVAL_MS`].
    fn progress(&self) {
        let now = mono_ms();
        let last = self.last_progress_ms.load(Ordering::Relaxed);
        if now.saturating_sub(last) >= PROGRESS_INTERVAL_MS
            && self.last_progress_ms.compare_exchange(last, now, Ordering::Relaxed, Ordering::Relaxed).is_ok()
        {
            self.emit(Event::Changed);
        }
    }

    fn find(&self, peer: &str, id: u64) -> Option<Arc<Transfer>> {
        self.transfers.lock().unwrap().iter().find(|t| t.id == id && t.peer_id == peer).cloned()
    }

    fn by_ui_id(&self, ui_id: &str) -> Option<Arc<Transfer>> {
        self.transfers.lock().unwrap().iter().find(|t| t.ui_id == ui_id).cloned()
    }

    fn with_peer(&self, peer: &str) -> Vec<Arc<Transfer>> {
        self.transfers.lock().unwrap().iter().filter(|t| t.peer_id == peer && !t.state().is_final()).cloned().collect()
    }

    fn insert(&self, t: Arc<Transfer>) {
        let mut list = self.transfers.lock().unwrap();
        list.insert(0, t);
        let mut finished = 0;
        list.retain(|t| {
            if !t.state().is_final() || t.running.load(Ordering::SeqCst) {
                return true;
            }
            finished += 1;
            finished <= HISTORY
        });
    }

    fn receive_allowed(&self, peer: &str) -> bool {
        let Some(hub) = self.hub() else { return false };
        self.settings().enabled && hub.local_caps(peer) & cap::FILES != 0
    }

    /// A connected peer that both sides allow file transfer with.
    fn sendable_peer(&self, peer_id: &str) -> Result<crate::features::PeerSummary, String> {
        let hub = self.hub().ok_or("Moush is shutting down.")?;
        if !self.settings().enabled {
            return Err("File transfer is turned off in Settings.".into());
        }
        let peer = hub.peer(peer_id).ok_or("That computer is not connected.")?;
        if hub.local_caps(peer_id) & cap::FILES == 0 {
            return Err(format!("File transfer is turned off for {}.", peer.name));
        }
        if hub.effective_caps(peer_id) & cap::FILES == 0 {
            return Err(format!("{} does not accept files right now.", peer.name));
        }
        Ok(peer)
    }

    /// Queues a message to a peer without ever blocking (safe on reader threads).
    fn tell_now(&self, peer: &str, msg: Msg) -> bool {
        self.hub().and_then(|h| h.sender(peer)).is_some_and(|s| s.try_send_feature(channel::FILES, msg.encode()))
    }

    /// Queues a message to a peer, waiting at most 2 s for room (worker and command threads).
    fn tell(&self, peer: &str, msg: Msg) -> bool {
        let Some(sender) = self.hub().and_then(|h| h.sender(peer)) else { return false };
        let body = msg.encode();
        sender.try_send_feature(channel::FILES, body.clone()) || sender.send_feature(channel::FILES, body, Duration::from_secs(2))
    }

    fn route(&self, t: &Arc<Transfer>, msg: Msg) {
        let Some(a) = t.attempt() else { return };
        if a.tx.try_send(Input::Peer(msg)).is_err() && a.stop().is_none() {
            log::warn!("transfer {}: peer is not respecting flow control", t.ui_id);
            self.tell_now(&t.peer_id, Msg::Error { id: t.id, reason: "Too much data in flight.".into() });
            self.stop(t, State::Failed, Some("The other computer sent too much data at once.".into()), Stop::Halted);
        }
    }

    /// Moves a transfer to a final state (first final state wins) and stops its worker.
    fn stop(&self, t: &Arc<Transfer>, state: State, error: Option<String>, why: Stop) {
        if self.finish(t, state, error)
            && let Some(a) = t.attempt()
        {
            a.halt(why);
        }
    }

    /// Sets a final state once. Returns false if the transfer had already finished.
    fn finish(&self, t: &Arc<Transfer>, state: State, error: Option<String>) -> bool {
        {
            let mut s = t.st.lock().unwrap();
            if s.state.is_final() {
                return false;
            }
            s.state = state;
            s.error = error;
            s.finished_at = Some(now_ms());
            s.approval_deadline = None;
            s.queued = false;
        }
        log::info!("transfer {}: {}", t.ui_id, state.as_str());
        self.changed();
        self.emit_later(Event::Finished(t.info()));
        true
    }

    fn reject(&self, t: &Arc<Transfer>, reason: &str, local_error: Option<String>) {
        self.tell(&t.peer_id, Msg::Reject { id: t.id, reason: reason.into() });
        self.finish(t, State::Rejected, local_error);
    }

    /// Starts queued outgoing transfers while their peer has free slots.
    fn schedule(self: &Arc<Self>) {
        let list = self.transfers.lock().unwrap().clone();
        let mut running: HashMap<&str, usize> = HashMap::new();
        for t in &list {
            if t.outgoing && t.running.load(Ordering::SeqCst) {
                *running.entry(&t.peer_id).or_default() += 1;
            }
        }
        // Oldest first.
        for t in list.iter().rev().filter(|t| t.outgoing) {
            let n = running.entry(&t.peer_id).or_default();
            if *n >= MAX_ACTIVE_PER_PEER {
                continue;
            }
            {
                let mut s = t.st.lock().unwrap();
                if !s.queued || s.state != State::Pending {
                    continue;
                }
                s.queued = false;
            }
            *n += 1;
            self.spawn_worker(t.clone(), self.settings().window_bytes(), true);
        }
    }

    fn spawn_worker(self: &Arc<Self>, t: Arc<Transfer>, window: usize, outgoing: bool) {
        let (tx, rx) = mpsc::sync_channel(INPUT_QUEUE);
        let a = Arc::new(Attempt { tx, window: Window::new(window), stop: Mutex::new(None) });
        *t.attempt.lock().unwrap() = Some(a.clone());
        t.running.store(true, Ordering::SeqCst);
        let inner = self.clone();
        let t2 = t.clone();
        let spawned = thread::Builder::new()
            .name(if outgoing { "transfer-send" } else { "transfer-receive" }.into())
            .spawn(move || {
                if outgoing {
                    send_worker(&inner, &t2, &a, rx);
                } else {
                    receive_worker(&inner, &t2, &a, rx);
                }
                t2.running.store(false, Ordering::SeqCst);
                inner.schedule();
                inner.changed();
            });
        if let Err(e) = spawned {
            t.running.store(false, Ordering::SeqCst);
            self.finish(&t, State::Failed, Some(format!("Could not start the transfer: {e}")));
        }
    }

    fn start_receiver(self: &Arc<Self>, t: &Arc<Transfer>) {
        {
            let mut s = t.st.lock().unwrap();
            if s.state != State::Pending {
                return;
            }
            s.approval_deadline = None;
            s.state = State::Active;
        }
        self.changed();
        self.spawn_worker(t.clone(), 0, false);
    }
}

// ---------------------------------------------------------------- control thread

fn control_loop(inner: Weak<Inner>, rx: Receiver<Control>) {
    loop {
        let msg = rx.recv_timeout(Duration::from_secs(1));
        let Some(inner) = inner.upgrade() else { return };
        match msg {
            Ok(Control::Offer(peer, msg)) => on_offer(&inner, &peer, msg),
            Ok(Control::Event(e)) => inner.emit(e),
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
        expire(&inner);
    }
}

/// Declines offers nobody answered and drops offers whose parts stopped arriving.
fn expire(inner: &Arc<Inner>) {
    let now = Instant::now();
    inner.assembling.lock().unwrap().retain(|_, (_, at)| now.duration_since(*at) < ASSEMBLY_TIMEOUT);
    let overdue: Vec<Arc<Transfer>> = inner
        .transfers
        .lock()
        .unwrap()
        .iter()
        .filter(|t| {
            let s = t.st.lock().unwrap();
            s.state == State::Pending && s.approval_deadline.is_some_and(|d| d <= now)
        })
        .cloned()
        .collect();
    for t in overdue {
        inner.reject(&t, "Nobody answered on the receiving computer.", Some("Not answered within 2 minutes.".into()));
    }
}

fn on_offer(inner: &Arc<Inner>, peer: &str, msg: Msg) {
    let Msg::Offer { id, total_files, total_bytes, first, files } = msg else { return };
    let key = (peer.to_owned(), id);
    let complete = {
        let mut asm = inner.assembling.lock().unwrap();
        if first == 0 {
            if let Some(t) = inner.find(peer, id)
                && (!t.state().is_final() || t.running.load(Ordering::SeqCst))
            {
                drop(asm);
                inner.tell(peer, Msg::Reject { id, reason: "This transfer is already in progress.".into() });
                return;
            }
            if !inner.receive_allowed(peer) {
                drop(asm);
                inner.tell(peer, Msg::Reject { id, reason: "File transfer is turned off on the receiving computer.".into() });
                return;
            }
            let open = inner.with_peer(peer).iter().filter(|t| !t.outgoing).count()
                + asm.keys().filter(|(p, _)| p == peer).count();
            if open >= MAX_INCOMING_PER_PEER {
                drop(asm);
                inner.tell(peer, Msg::Reject { id, reason: "Too many transfers at once; try again later.".into() });
                return;
            }
            asm.insert(key.clone(), (OfferAssembler::new(id, total_files, total_bytes), Instant::now()));
        }
        let Some((a, _)) = asm.get_mut(&key) else { return };
        match a.push(total_files, total_bytes, first, files) {
            Ok(false) => return,
            Ok(true) => asm.remove(&key).map(|(a, _)| a),
            Err(e) => {
                asm.remove(&key);
                drop(asm);
                refuse(inner, peer, id, e);
                return;
            }
        }
    };
    if let Some(a) = complete {
        offer_complete(inner, peer, id, a.into_files());
    }
}

/// Refuses an invalid offer and records it so the user can see what happened.
fn refuse(inner: &Arc<Inner>, peer: &str, id: u64, why: &str) {
    log::warn!("refusing a file transfer offer: {why}");
    inner.tell(peer, Msg::Reject { id, reason: format!("Refused: {why}.") });
    let Some(hub) = inner.hub() else { return };
    let name = hub.peer(peer).map(|p| p.name).unwrap_or_else(|| peer.to_owned());
    let t = new_incoming(peer, &name, id, Vec::new());
    {
        let mut s = t.st.lock().unwrap();
        s.state = State::Rejected;
        s.error = Some(format!("Refused files from {name}: {why}."));
        s.finished_at = Some(now_ms());
    }
    inner.remove_final(peer, id);
    inner.insert(t);
    inner.changed();
}

fn new_incoming(peer: &str, peer_name: &str, id: u64, files: Vec<FileEntry>) -> Arc<Transfer> {
    Arc::new(Transfer {
        id,
        ui_id: format!("r{id:016x}"),
        outgoing: false,
        peer_id: peer.to_owned(),
        peer_name: peer_name.to_owned(),
        started_at: now_ms(),
        st: Mutex::new(Status {
            total: files.iter().map(|f| f.size).sum(),
            files,
            sources: Vec::new(),
            state: State::Pending,
            error: None,
            finished_at: None,
            destination: None,
            reveal: None,
            meter: RateMeter::default(),
            approval_deadline: None,
            queued: false,
        }),
        done: AtomicU64::new(0),
        running: AtomicBool::new(false),
        attempt: Mutex::new(None),
    })
}

impl Inner {
    /// Forgets an earlier, finished record of the same transfer (it is being retried).
    fn remove_final(&self, peer: &str, id: u64) {
        self.transfers.lock().unwrap().retain(|t| !(t.id == id && t.peer_id == peer && t.state().is_final()));
    }
}

fn offer_complete(inner: &Arc<Inner>, peer: &str, id: u64, files: Vec<FileEntry>) {
    let mut names = Vec::with_capacity(files.len());
    for f in &files {
        match tr::check_offered_name(&f.name) {
            Ok(n) => names.push(n),
            Err(e) => return refuse(inner, peer, id, e),
        }
    }
    tr::dedupe_names(&mut names);
    let Some(hub) = inner.hub() else { return };
    let Some(summary) = hub.peer(peer) else { return };
    if !inner.receive_allowed(peer) {
        inner.tell(peer, Msg::Reject { id, reason: "File transfer is turned off on the receiving computer.".into() });
        return;
    }
    let ui_id = format!("r{id:016x}");
    if inner.by_ui_id(&ui_id).is_some_and(|t| t.peer_id != peer) {
        inner.tell(peer, Msg::Reject { id, reason: "Duplicate transfer id.".into() });
        return;
    }
    let files: Vec<FileEntry> = names.into_iter().zip(&files).map(|(name, f)| FileEntry { name, size: f.size }).collect();
    let count = files.len();
    let t = new_incoming(peer, &summary.name, id, files);
    let total = t.st.lock().unwrap().total;
    let ask = inner.settings().ask_before_receiving;
    if ask {
        t.st.lock().unwrap().approval_deadline = Some(Instant::now() + APPROVAL_TIMEOUT);
    }
    log::info!("transfer {}: {} offers {count} file(s), {total} bytes", t.ui_id, summary.name);
    inner.remove_final(peer, id);
    inner.insert(t.clone());
    inner.emit(Event::Incoming { id: t.ui_id.clone(), peer_name: summary.name.clone(), files: count, bytes: total, needs_approval: ask });
    if ask {
        inner.changed();
    } else {
        inner.start_receiver(&t);
    }
}

// ---------------------------------------------------------------- workers

/// How a worker ended, besides success.
enum Abort {
    /// Someone else already set the final state (cancel, disconnect, peer error).
    Stopped,
    /// Fail and tell the peer.
    Error(String),
    /// Fail without telling the peer (it is gone or already knows).
    Quiet(String),
    /// Receive: decline before anything was accepted.
    Refuse(String),
}

impl From<io::Error> for Abort {
    fn from(e: io::Error) -> Self {
        Abort::Error(e.to_string())
    }
}

/// Waits for the next message from the peer. Wakes for stops; times out after `timeout`.
fn next_input(a: &Attempt, rx: &Receiver<Input>, timeout: Duration, what: &str) -> Result<Msg, Abort> {
    let deadline = Instant::now() + timeout;
    loop {
        if a.stop().is_some() {
            return Err(Abort::Stopped);
        }
        match rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            Ok(Input::Peer(m)) => return Ok(m),
            Ok(Input::Wake) => {}
            Err(RecvTimeoutError::Timeout) => return Err(Abort::Error(what.to_owned())),
            Err(RecvTimeoutError::Disconnected) => return Err(Abort::Stopped),
        }
    }
}

fn end_worker(inner: &Arc<Inner>, t: &Arc<Transfer>, a: &Attempt, r: Result<(), Abort>) {
    match r {
        Ok(()) => {
            inner.finish(t, State::Done, None);
        }
        Err(Abort::Stopped) => {}
        Err(_) if a.stop().is_some() => {}
        Err(Abort::Error(msg)) => {
            inner.tell(&t.peer_id, Msg::Error { id: t.id, reason: msg.clone() });
            inner.finish(t, State::Failed, Some(msg));
        }
        Err(Abort::Quiet(msg)) => {
            inner.finish(t, State::Failed, Some(msg));
        }
        Err(Abort::Refuse(msg)) => {
            inner.tell(&t.peer_id, Msg::Reject { id: t.id, reason: msg.clone() });
            inner.finish(t, State::Rejected, Some(msg));
        }
    }
}

fn send_worker(inner: &Arc<Inner>, t: &Arc<Transfer>, a: &Arc<Attempt>, rx: Receiver<Input>) {
    let r = send(inner, t, a, &rx);
    if let Err(Abort::Stopped) = &r
        && a.stop() == Some(Stop::Cancelled)
    {
        log::info!("transfer {}: send cancelled", t.ui_id);
    }
    end_worker(inner, t, a, r);
}

fn send(inner: &Arc<Inner>, t: &Arc<Transfer>, a: &Attempt, rx: &Receiver<Input>) -> Result<(), Abort> {
    let lost = || Abort::Quiet(format!("Lost the connection to {}.", t.peer_name));
    let sender = inner.hub().and_then(|h| h.sender(&t.peer_id)).ok_or_else(lost)?;
    let (files, sources) = {
        let s = t.st.lock().unwrap();
        (s.files.clone(), s.sources.clone())
    };
    let send = |body: Vec<u8>| if sender.send_feature(channel::FILES, body, SEND_TIMEOUT) { Ok(()) } else { Err(lost()) };

    for part in tr::offer_parts(t.id, &files) {
        send(part)?;
    }
    let mut accept = AcceptAssembler::new(files.len());
    let deadline = Instant::now() + ANSWER_TIMEOUT;
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let no_answer = format!("{} did not answer.", t.peer_name);
        match next_input(a, rx, left, &no_answer)? {
            Msg::Accept { last, resume, .. } => match accept.push(last, &resume, &files) {
                Ok(true) => break,
                Ok(false) => {}
                Err(e) => return Err(Abort::Error(format!("Invalid answer from {}: {e}.", t.peer_name))),
            },
            _ => return Err(Abort::Error("Unexpected message before the transfer was accepted.".into())),
        }
    }
    {
        let mut s = t.st.lock().unwrap();
        if s.state != State::Pending {
            return Err(Abort::Stopped);
        }
        s.state = State::Active;
    }
    inner.changed();

    let mut buf = vec![0u8; tr::CHUNK];
    for (i, (f, path)) in files.iter().zip(&sources).enumerate() {
        let (offset, prefix) = match accept.starts()[i] {
            Start::Skip => {
                t.done.fetch_add(f.size, Ordering::Relaxed);
                continue;
            }
            Start::At { offset, prefix } => (offset, prefix),
        };
        let mut file = File::open(path).map_err(|e| Abort::Error(format!("Cannot read {}: {e}", f.name)))?;
        if file.metadata()?.len() != f.size {
            return Err(Abort::Error(format!("{} changed after it was offered.", f.name)));
        }
        let mut hasher = tr::HashState::default();
        let mut pos = 0;
        if offset > 0 {
            let own = tr::hash_prefix(&mut file, offset, || a.stop().is_some()).map_err(|e| Abort::Error(format!("Cannot read {}: {e}", f.name)))?;
            pos = tr::resume_from(offset, &prefix, &tr::finish_hash(&own));
            if pos > 0 {
                hasher = own;
                log::info!("transfer {}: resuming file {i} at {pos} bytes", t.ui_id);
            } else {
                file.seek(SeekFrom::Start(0))?;
            }
        }
        t.done.fetch_add(pos, Ordering::Relaxed);
        while pos < f.size {
            if a.stop().is_some() {
                return Err(Abort::Stopped);
            }
            // Nothing but Complete is expected now; drain stray input so the queue stays empty.
            while let Ok(Input::Peer(_) | Input::Wake) = rx.try_recv() {}
            let n = (f.size - pos).min(tr::CHUNK as u64) as usize;
            file.read_exact(&mut buf[..n]).map_err(|e| Abort::Error(format!("Cannot read {}: {e}", f.name)))?;
            tr::hash_update(&mut hasher, &buf[..n]);
            match a.window.acquire(n, SEND_TIMEOUT) {
                Acquire::Ok => {}
                Acquire::TimedOut => return Err(Abort::Error(format!("{} stopped responding.", t.peer_name))),
                Acquire::Closed => return Err(Abort::Stopped),
            }
            send(tr::encode_data(t.id, i as u32, pos, &buf[..n]))?;
            pos += n as u64;
        }
        send(Msg::FileDone { id: t.id, index: i as u32, size: f.size, hash: tr::finish_hash(&hasher) }.encode())?;
    }
    // The receiver verifies, flushes and renames, then confirms.
    let waiting = format!("{} did not confirm the transfer.", t.peer_name);
    loop {
        match next_input(a, rx, IDLE_TIMEOUT * 2, &waiting)? {
            Msg::Complete { .. } => return Ok(()),
            _ => continue,
        }
    }
}

/// Receive-side runtime state of one file.
struct RxFile {
    name: String,
    part: PathBuf,
    skip: bool,
    done: bool,
    state: ReceiveFile,
    file: Option<File>,
}

fn receive_worker(inner: &Arc<Inner>, t: &Arc<Transfer>, a: &Arc<Attempt>, rx: Receiver<Input>) {
    let mut files = Vec::new();
    let r = receive(inner, t, a, &rx, &mut files);
    // Close handles before deleting or keeping partial files.
    for f in &mut files {
        f.file = None;
    }
    if a.stop() == Some(Stop::Cancelled) {
        for f in files.iter().filter(|f| !f.done && !f.skip) {
            if let Err(e) = fs::remove_file(&f.part)
                && e.kind() != io::ErrorKind::NotFound
            {
                log::warn!("transfer {}: cannot remove a partial file: {e}", t.ui_id);
            }
        }
    }
    let ok = r.is_ok();
    end_worker(inner, t, a, r);
    if ok && inner.settings().open_folder_when_done {
        let reveal = t.st.lock().unwrap().reveal.clone();
        if let Some(p) = reveal {
            crate::win::dialogs::reveal(&display_path(&p));
        }
    }
}

fn receive(inner: &Arc<Inner>, t: &Arc<Transfer>, a: &Attempt, rx: &Receiver<Input>, rx_files: &mut Vec<RxFile>) -> Result<(), Abort> {
    let settings = inner.settings();
    let dir = resolve_download_dir(&settings.download_dir, &inner.default_dir).map_err(Abort::Refuse)?;
    t.st.lock().unwrap().destination = Some(dir.clone());
    remove_stale_parts(&dir);
    let policy = tr::Overwrite::from_setting(&settings.overwrite);
    let files = t.st.lock().unwrap().files.clone();
    let cancelled = || a.stop().is_some();

    // Plan: skip, resume from a partial file, or start fresh.
    let mut resume = Vec::new();
    let mut needed = 0u64;
    let mut have = 0u64;
    for (i, f) in files.iter().enumerate() {
        let part = dir.join(tr::part_name(&f.name, t.id));
        let skip = policy == tr::Overwrite::Skip && dir.join(&f.name).symlink_metadata().is_ok();
        let mut state = ReceiveFile::new(f.size);
        if skip {
            resume.push(Resume { index: i as u32, offset: tr::SKIP, prefix: [0; 32] });
            have += f.size;
        } else {
            let len = fs::metadata(&part).ok().filter(|m| m.is_file()).map(|m| m.len()).unwrap_or(0);
            let offset = tr::receive_offset(len, f.size);
            if offset > 0 {
                // Errors while stopped are ignored by `end_worker`.
                let unreadable = |e: io::Error| Abort::Refuse(format!("Cannot read the partial copy of {}: {e}", f.name));
                let mut file = File::open(&part).map_err(unreadable)?;
                let hasher = tr::hash_prefix(&mut file, offset, cancelled).map_err(unreadable)?;
                resume.push(Resume { index: i as u32, offset, prefix: tr::finish_hash(&hasher) });
                state = ReceiveFile::resumed(f.size, offset, hasher);
                have += offset;
            }
            needed += f.size - offset;
        }
        rx_files.push(RxFile { name: f.name.clone(), part, skip, done: skip, state, file: None });
    }
    if let Some(free) = free_space(&dir)
        && free < needed.saturating_add(DISK_RESERVE)
    {
        return Err(Abort::Refuse(format!(
            "Not enough free disk space on the receiving computer ({} needed, {} free).",
            human_bytes(needed),
            human_bytes(free)
        )));
    }
    t.done.store(have, Ordering::Relaxed);
    if !resume.is_empty() {
        log::info!("transfer {}: resuming {} file(s), {have} bytes already here", t.ui_id, resume.len());
    }
    let sender = inner.hub().and_then(|h| h.sender(&t.peer_id));
    let lost = || Abort::Quiet(format!("Lost the connection to {}.", t.peer_name));
    let sender = sender.ok_or_else(lost)?;
    for part in tr::accept_parts(t.id, &resume) {
        if !sender.send_feature(channel::FILES, part, SEND_TIMEOUT) {
            return Err(lost());
        }
    }
    inner.changed();

    let stalled = format!("{} stopped sending.", t.peer_name);
    let mut i = 0;
    loop {
        // Files arrive in order; skipped ones are already done.
        while rx_files.get(i).is_some_and(|f| f.done) {
            i += 1;
        }
        let Some(f) = rx_files.get_mut(i) else { break };
        match next_input(a, rx, IDLE_TIMEOUT, &stalled)? {
            Msg::Data { index, offset, data, .. } => {
                if index as usize != i {
                    return Err(Abort::Error("File data arrived out of order.".into()));
                }
                let at = f.state.check(offset, data.len()).map_err(|e| Abort::Error(format!("{}: {e}.", f.name)))?;
                let before = f.state.written();
                let file = open_part(&mut f.file, &f.part, before)?;
                if at == WriteAt::Restart {
                    file.set_len(0)?;
                    file.seek(SeekFrom::Start(0))?;
                    t.done.fetch_sub(before, Ordering::Relaxed);
                }
                file.write_all(&data).map_err(|e| Abort::Error(format!("Cannot save {}: {e}", f.name)))?;
                f.state.commit(at, &data);
                t.done.fetch_add(data.len() as u64, Ordering::Relaxed);
                // Acknowledge after the write: a slow disk slows the sender down.
                let ack = Msg::Ack { id: t.id, bytes: data.len() as u32 }.encode();
                if !sender.try_send_feature(channel::FILES, ack.clone()) && !sender.send_feature(channel::FILES, ack, SEND_TIMEOUT) {
                    return Err(lost());
                }
                inner.progress();
            }
            Msg::FileDone { index, size, hash, .. } => {
                if index as usize != i {
                    return Err(Abort::Error("File data arrived out of order.".into()));
                }
                let written = f.state.written();
                let mut file = match f.file.take() {
                    Some(file) => file,
                    None => {
                        let mut slot = None;
                        open_part(&mut slot, &f.part, written)?;
                        slot.unwrap()
                    }
                };
                if let Err(e) = f.state.verify(size, &hash) {
                    drop(file);
                    let _ = fs::remove_file(&f.part);
                    return Err(Abort::Error(format!("{}: {e}.", f.name)));
                }
                file.flush()?;
                file.sync_all().map_err(|e| Abort::Error(format!("Cannot save {}: {e}", f.name)))?;
                drop(file);
                match place_file(&f.part, &dir, &f.name, policy) {
                    Ok(Some(dest)) => {
                        let saved = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                        let mut s = t.st.lock().unwrap();
                        if let Some(entry) = s.files.get_mut(i) {
                            entry.name = saved;
                        }
                        s.reveal = Some(dest);
                    }
                    Ok(None) => log::info!("transfer {}: file {i} skipped (name exists)", t.ui_id),
                    Err(e) => return Err(Abort::Error(format!("Cannot save {}: {e}", f.name))),
                }
                f.done = true;
                inner.changed();
            }
            _ => return Err(Abort::Error("Unexpected file transfer message.".into())),
        }
    }
    if !sender.send_feature(channel::FILES, Msg::Complete { id: t.id }.encode(), SEND_TIMEOUT) {
        return Err(lost());
    }
    Ok(())
}

/// Opens (creating) a partial file positioned at `written`; a fresh file is truncated.
fn open_part<'a>(slot: &'a mut Option<File>, path: &Path, written: u64) -> Result<&'a mut File, Abort> {
    if slot.is_none() {
        let mut f = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)?;
        if written == 0 {
            f.set_len(0)?;
        }
        f.seek(SeekFrom::Start(written))?;
        *slot = Some(f);
    }
    Ok(slot.as_mut().unwrap())
}

/// Moves a verified partial file to its final name per the overwrite policy and marks it
/// as downloaded. Returns `None` when the file was skipped.
fn place_file(part: &Path, dir: &Path, name: &str, policy: tr::Overwrite) -> io::Result<Option<PathBuf>> {
    for _ in 0..8 {
        let taken = |n: &str| dir.join(n).symlink_metadata().is_ok();
        let Some(dest_name) = tr::destination_name(name, policy, taken) else {
            fs::remove_file(part)?;
            return Ok(None);
        };
        let dest = dir.join(dest_name);
        let moved = if policy == tr::Overwrite::Replace { fs::rename(part, &dest) } else { move_no_replace(part, &dest) };
        match moved {
            Ok(()) => {
                mark_downloaded(&dest);
                return Ok(Some(dest));
            }
            // Someone created that name in the meantime: pick the next one.
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "no free file name"))
}

/// Writes the Mark of the Web so Windows treats the file like a browser download
/// (SmartScreen, Office protected view, "unblock" prompt).
fn mark_downloaded(path: &Path) {
    let mut stream = path.as_os_str().to_owned();
    stream.push(":Zone.Identifier");
    if let Err(e) = fs::write(PathBuf::from(stream), b"[ZoneTransfer]\r\nZoneId=3\r\n") {
        // Volumes without alternate data streams (FAT32, exFAT).
        log::debug!("cannot mark a received file as downloaded: {e}");
    }
}

/// Removes partial files of transfers that were abandoned long ago.
fn remove_stale_parts(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        if !name.to_string_lossy().ends_with(tr::PART_SUFFIX) {
            continue;
        }
        let old = e.metadata().ok().and_then(|m| m.modified().ok()).and_then(|t| t.elapsed().ok()).is_some_and(|age| age > STALE_PART_AGE);
        if old {
            let _ = fs::remove_file(e.path());
        }
    }
}

// ---------------------------------------------------------------- helpers

/// Reads the size of each file to send. Folders are refused.
fn stat_sources(paths: &[PathBuf]) -> Result<(Vec<FileEntry>, Vec<PathBuf>), String> {
    if paths.is_empty() {
        return Err("Choose at least one file.".into());
    }
    if paths.len() > tr::MAX_FILES as usize {
        return Err(format!("At most {} files can be sent at once.", tr::MAX_FILES));
    }
    let mut files = Vec::with_capacity(paths.len());
    let mut total = 0u64;
    for p in paths {
        let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| format!("{} is not a file.", p.display()))?;
        let meta = fs::metadata(p).map_err(|e| format!("Cannot read {name}: {e}"))?;
        if meta.is_dir() {
            return Err(format!("{name} is a folder; only files can be sent."));
        }
        total = total.saturating_add(meta.len());
        files.push(FileEntry { name, size: meta.len() });
    }
    if total > tr::MAX_TOTAL_BYTES {
        return Err("These files are too large to send at once.".into());
    }
    Ok((files, paths.to_vec()))
}

fn new_id() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let mut h = RandomState::new().build_hasher();
    h.write_u64(now_ms());
    h.write_u64(COUNTER.fetch_add(1, Ordering::Relaxed));
    h.write_u32(std::process::id());
    h.finish().max(1)
}

/// `\\?\C:\x` → `C:\x` for display and for shell APIs.
fn display_path(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) if !rest.starts_with("UNC\\") => PathBuf::from(rest),
        _ => p.to_path_buf(),
    }
}

fn human_bytes(n: u64) -> String {
    const UNITS: [&str; 5] = ["bytes", "KB", "MB", "GB", "TB"];
    let mut v = n as f64;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 { format!("{n} bytes") } else { format!("{v:.1} {}", UNITS[u]) }
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// The folder files are received into: the setting, or `default` when it is empty. Must
/// be an absolute path on a local drive; it is created if missing. Returns the canonical
/// (`\\?\`) form, which also works beyond `MAX_PATH`.
pub fn resolve_download_dir(setting: &str, default: &Path) -> Result<PathBuf, String> {
    let setting = setting.trim();
    let dir = if setting.is_empty() { default.to_path_buf() } else { PathBuf::from(setting) };
    let local = match dir.components().next() {
        Some(Component::Prefix(p)) => matches!(p.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)),
        _ => false,
    };
    if !local || !dir.is_absolute() {
        return Err(format!("The download folder \"{}\" must be a full path on a drive of this computer.", dir.display()));
    }
    if is_network_drive(&dir) {
        return Err(format!("The download folder \"{}\" is on a network drive.", dir.display()));
    }
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create the download folder \"{}\": {e}", dir.display()))?;
    let canonical = fs::canonicalize(&dir).map_err(|e| format!("Cannot open the download folder \"{}\": {e}", dir.display()))?;
    if !canonical.is_dir() {
        return Err(format!("\"{}\" is not a folder.", dir.display()));
    }
    Ok(canonical)
}

/// `Downloads\Moush` in the user's Downloads folder (wherever it was moved to).
pub fn default_download_dir() -> PathBuf {
    use windows::Win32::System::Com::CoTaskMemFree;
    use windows::Win32::UI::Shell::{FOLDERID_Downloads, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let known = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Downloads, KF_FLAG_DEFAULT, None).ok().and_then(|p| {
            let s = p.to_string().ok();
            CoTaskMemFree(Some(p.0 as *const _));
            s
        })
    };
    let downloads = known.map(PathBuf::from).unwrap_or_else(|| {
        std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(std::env::temp_dir).join("Downloads")
    });
    downloads.join("Moush")
}

fn is_network_drive(dir: &Path) -> bool {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    const DRIVE_REMOTE: u32 = 4;
    let Some(Component::Prefix(p)) = dir.components().next() else { return false };
    let root = match p.kind() {
        Prefix::Disk(d) | Prefix::VerbatimDisk(d) => format!("{}:\\", d as char),
        _ => return true,
    };
    let w = wide(Path::new(&root));
    unsafe { GetDriveTypeW(PCWSTR(w.as_ptr())) == DRIVE_REMOTE }
}

fn free_space(dir: &Path) -> Option<u64> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let w = wide(dir);
    let mut free = 0u64;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(w.as_ptr()), Some(&mut free), None, None) }.ok().map(|_| free)
}

/// Renames `from` to `to`, failing with `AlreadyExists` instead of replacing a file.
fn move_no_replace(from: &Path, to: &Path) -> io::Result<()> {
    use windows::core::PCWSTR;
    use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS};
    use windows::Win32::Storage::FileSystem::{MoveFileExW, MOVEFILE_WRITE_THROUGH};
    let (f, t) = (wide(from), wide(to));
    match unsafe { MoveFileExW(PCWSTR(f.as_ptr()), PCWSTR(t.as_ptr()), MOVEFILE_WRITE_THROUGH) } {
        Ok(()) => Ok(()),
        Err(e) if e.code() == ERROR_ALREADY_EXISTS.to_hresult() || e.code() == ERROR_FILE_EXISTS.to_hresult() => {
            Err(io::Error::new(io::ErrorKind::AlreadyExists, "file exists"))
        }
        Err(e) => Err(io::Error::from_raw_os_error(e.code().0 & 0xFFFF)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("moush-transfer-{tag}-{}", new_id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn download_dir_must_be_local_and_absolute() {
        let base = temp_dir("dir");
        assert!(resolve_download_dir("relative\\dir", &base).is_err());
        assert!(resolve_download_dir(r"\\server\share\x", &base).is_err());
        assert!(resolve_download_dir(r"\\.\PhysicalDrive0", &base).is_err());
        let d = resolve_download_dir("", &base.join("sub")).unwrap();
        assert!(d.is_dir() && d.ends_with("sub"));
        assert_eq!(display_path(&d), base.join("sub"));
        assert!(default_download_dir().ends_with("Moush"));
        fs::remove_dir_all(&base).unwrap();
    }

    #[test]
    fn placing_files_follows_the_overwrite_policy_and_marks_them() {
        let dir = fs::canonicalize(temp_dir("place")).unwrap();
        fs::write(dir.join("a.txt"), b"old").unwrap();
        let part = |data: &[u8]| {
            let p = dir.join(tr::part_name("a.txt", 1));
            fs::write(&p, data).unwrap();
            p
        };
        let p = place_file(&part(b"new1"), &dir, "a.txt", tr::Overwrite::Rename).unwrap().unwrap();
        assert!(p.ends_with("a (1).txt"));
        assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"old");
        let zone = fs::read_to_string(format!("{}:Zone.Identifier", p.display())).unwrap();
        assert!(zone.contains("ZoneId=3"));
        assert!(place_file(&part(b"x"), &dir, "a.txt", tr::Overwrite::Skip).unwrap().is_none());
        assert!(!dir.join(tr::part_name("a.txt", 1)).exists(), "skipped part is removed");
        place_file(&part(b"new2"), &dir, "a.txt", tr::Overwrite::Replace).unwrap().unwrap();
        assert_eq!(fs::read(dir.join("a.txt")).unwrap(), b"new2");
        assert_eq!(move_no_replace(&dir.join("a (1).txt"), &dir.join("a.txt")).unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert!(free_space(&dir).unwrap() > 0);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn human_sizes() {
        assert_eq!(human_bytes(12), "12 bytes");
        assert_eq!(human_bytes(1536), "1.5 KB");
        assert_eq!(human_bytes(5 << 30), "5.0 GB");
    }
}
