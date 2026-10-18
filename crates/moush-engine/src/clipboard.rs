//! Clipboard synchronization, clipboard history and manual text sharing with the
//! connected peers (feature channel `CLIPBOARD`).
//!
//! Threads:
//! * the connection reader threads call [`FeatureService::on_message`]: it only decodes,
//!   releases flow-control credit on acks and hands everything else to the worker with a
//!   bounded `try_send`;
//! * one worker thread owns the [`SyncState`], reassembles incoming clips, decompresses,
//!   writes them to the clipboard through the backend and relays them;
//! * one short-lived sender thread per outgoing clip and peer streams the payload in
//!   flow-controlled chunks (at most one per peer: a newer clip aborts the older one);
//! * the backend (the Win32 clipboard thread in production) reads and writes the system
//!   clipboard and reports changes.
//!
//! Wire format, ordering and loop-prevention rules are documented in
//! [`moush_core::clipboard`]. Clips are never logged; the history lives in memory only.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use moush_core::clipboard::{
    as_link, codec, decode_payload, encode_payload, reject, share_messages, validate_share_text, ChunkResult, Clip, ClipKind,
    ClipMsg, Decision, FormatFilter, Incoming, Offer, ShareAssembler, ShareResult, SyncState, Version, MAX_CHUNK_DATA,
    MAX_CLIP_BYTES,
};
use moush_core::features::{cap, channel, ClipboardSettings};
use moush_core::flow::{Acquire, Window};
use moush_core::net::PeerSender;
use moush_core::protocol::truncate_utf8;
use moush_core::util::now_ms;

use crate::features::{FeatureHub, FeatureService, PeerSummary};

/// Bounded queue between the reader threads / backend and the worker.
const WORK_QUEUE: usize = 512;
/// How long a sender waits for room in the connection's bulk queue.
const SEND_TIMEOUT: Duration = Duration::from_secs(5);
/// How long a sender waits for the receiver to acknowledge data before giving up.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(20);
/// An incoming clip that receives nothing for this long is dropped.
const INCOMING_IDLE: Duration = Duration::from_secs(30);
/// Housekeeping period (settings changes, ack retries, stale transfers).
const TICK: Duration = Duration::from_millis(250);
/// Memory bound of the clipboard history, whatever its entry limit.
const HISTORY_MAX_BYTES: usize = 64 * 1024 * 1024;
/// Shared texts kept (newest first).
const MAX_SHARED: usize = 20;
/// Preview of a shared text sent to the UI.
const SHARE_PREVIEW_BYTES: usize = 4096;
/// Attempts to read a clipboard that stayed busy (another application holding it).
const READ_ATTEMPTS: u32 = 4;

// ---------------------------------------------------------------- backend

/// Access to the system clipboard. The production backend is
/// [`crate::win::clipboard::WinClipboard`]; tests use an in-memory fake.
pub trait ClipboardBackend: Send + Sync + 'static {
    /// Start or stop reporting clipboard changes.
    fn set_listening(&self, on: bool);
    /// The system clipboard sequence number (changes with every clipboard update).
    fn sequence(&self) -> u32;
    /// Reads the formats `opts` allows.
    fn read(&self, opts: &ReadOptions) -> Result<LocalRead, String>;
    /// Replaces the clipboard contents; returns the sequence number after the write.
    fn write(&self, clip: Arc<Clip>) -> Result<u32, String>;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ReadOptions {
    pub filter: FormatFilter,
    pub skip_sensitive: bool,
    /// Formats larger than this are not read.
    pub max_bytes: usize,
}

#[derive(Debug)]
pub enum LocalRead {
    /// `oversized`: a format was left out because it exceeded `max_bytes`.
    Clip { seq: u32, clip: Clip, oversized: bool },
    /// The source marked the content as sensitive (password managers).
    Sensitive { seq: u32 },
}

/// Called by the backend whenever the clipboard may have changed.
pub type ChangeCallback = Box<dyn Fn() + Send + Sync>;

// ---------------------------------------------------------------- configuration

/// Settings the service reads live (at the moment it needs them).
#[derive(Clone, Debug)]
pub struct ClipboardConfig {
    pub clipboard: ClipboardSettings,
    /// Flow-control window per transfer (`TransferSettings::window_bytes`).
    pub window_bytes: usize,
    /// This device's display name (sent as the origin of local copies).
    pub device_name: String,
}

impl ClipboardConfig {
    pub fn max_bytes(&self) -> usize {
        (self.clipboard.max_size_kb as usize * 1024).min(MAX_CLIP_BYTES)
    }

    fn filter(&self) -> FormatFilter {
        FormatFilter::from_settings(&self.clipboard)
    }
}

/// Things the app may want to show.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipboardEvent {
    /// Status, history or shared texts changed (refresh the UI).
    Changed,
    /// A clip from another computer was placed on this clipboard.
    ClipReceived { from: String, kind: ClipKind },
    /// Text or a link was shared with this computer.
    TextShared { from: String, chars: usize, is_link: bool },
}

pub struct ServiceOptions {
    /// This device's id (origin of local copies, tie-break of concurrent copies).
    pub device_id: String,
    pub settings: Box<dyn Fn() -> ClipboardConfig + Send + Sync>,
    pub events: Box<dyn Fn(ClipboardEvent) + Send + Sync>,
}

// ---------------------------------------------------------------- UI views

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoryEntryView {
    pub id: String,
    pub kind: ClipKind,
    pub preview: String,
    pub bytes: u64,
    /// "local", "sent" or "received".
    pub origin: &'static str,
    /// Device the clip came from (`None` = this computer).
    pub source: Option<String>,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SharedTextInfo {
    pub id: String,
    pub incoming: bool,
    pub peer_name: String,
    pub preview: String,
    pub chars: u32,
    pub is_link: bool,
    pub at: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatusView {
    pub last_sync_at: Option<u64>,
    /// "sent" or "received".
    pub last_direction: Option<&'static str>,
    pub last_peer: Option<String>,
    pub last_kind: Option<ClipKind>,
    pub history_count: u32,
    pub history_revision: u64,
    pub error: Option<String>,
}

struct HistoryItem {
    id: u64,
    hash: u64,
    kind: ClipKind,
    preview: String,
    bytes: usize,
    origin: &'static str,
    source: Option<String>,
    at: u64,
    clip: Arc<Clip>,
}

struct SharedItem {
    id: u64,
    incoming: bool,
    peer_name: String,
    text: String,
    is_link: bool,
    at: u64,
}

#[derive(Default)]
struct UiState {
    history: VecDeque<HistoryItem>,
    history_bytes: usize,
    revision: u64,
    shared: VecDeque<SharedItem>,
    status: StatusView,
    next_id: u64,
}

impl UiState {
    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    fn add_history(&mut self, s: &ClipboardSettings, clip: Arc<Clip>, hash: u64, origin: &'static str, source: Option<String>) {
        if !s.history {
            return;
        }
        self.history.retain(|h| h.hash != hash);
        let id = self.next_id();
        self.history.push_front(HistoryItem {
            id,
            hash,
            kind: clip.kind(),
            preview: clip.preview(),
            bytes: clip.size(),
            origin,
            source,
            at: now_ms(),
            clip,
        });
        self.trim(s);
        self.revision += 1;
    }

    /// Applies the history settings (cleared when disabled, trimmed to the limits).
    fn trim(&mut self, s: &ClipboardSettings) {
        let before = self.history.len();
        let limit = if s.history { s.history_limit.clamp(1, 200) as usize } else { 0 };
        self.history.truncate(limit);
        self.history_bytes = self.history.iter().map(|h| h.bytes).sum();
        while self.history_bytes > HISTORY_MAX_BYTES && self.history.len() > 1 {
            let h = self.history.pop_back().unwrap();
            self.history_bytes -= h.bytes;
        }
        if self.history.len() != before {
            self.revision += 1;
        }
    }

    fn add_shared(&mut self, incoming: bool, peer_name: String, text: String) -> u64 {
        let id = self.next_id();
        let is_link = as_link(&text).is_some();
        self.shared.push_front(SharedItem { id, incoming, peer_name, text, is_link, at: now_ms() });
        self.shared.truncate(MAX_SHARED);
        id
    }

    fn synced(&mut self, direction: &'static str, peer: String, kind: ClipKind) {
        let s = &mut self.status;
        s.last_sync_at = Some(now_ms());
        s.last_direction = Some(direction);
        s.last_peer = Some(peer);
        s.last_kind = Some(kind);
        s.error = None;
    }
}

fn parse_id(id: &str, prefix: char) -> Option<u64> {
    id.strip_prefix(prefix)?.parse().ok()
}

// ---------------------------------------------------------------- service

/// An outgoing clip to one peer.
struct OutXfer {
    xfer: u32,
    hash: u64,
    window: Arc<Window>,
}

enum Work {
    LocalChanged,
    Remote { peer: String, msg: ClipMsg },
    CapsChanged { peer: String, caps: u32 },
    PeerDown(String),
    /// A send ended without the peer receiving the clip.
    SendFailed { peer: String, hash: u64 },
    /// Put a clip on the clipboard on the user's request and sync it as a local copy.
    Write { clip: Arc<Clip>, reply: SyncSender<Result<(), String>> },
}

pub struct ClipboardService {
    hub: Weak<FeatureHub>,
    opts: ServiceOptions,
    backend: OnceLock<Arc<dyn ClipboardBackend>>,
    tx: SyncSender<Work>,
    change_pending: Arc<AtomicBool>,
    outgoing: Mutex<HashMap<String, OutXfer>>,
    ui: Mutex<UiState>,
    next_xfer: AtomicU32,
    next_share: AtomicU32,
}

impl ClipboardService {
    /// Creates the service, its worker and its backend (`make_backend` receives the
    /// change callback), and registers it with `hub`.
    pub fn start(
        hub: &Arc<FeatureHub>,
        opts: ServiceOptions,
        make_backend: impl FnOnce(ChangeCallback) -> io::Result<Arc<dyn ClipboardBackend>>,
    ) -> io::Result<Arc<Self>> {
        let (tx, rx) = mpsc::sync_channel(WORK_QUEUE);
        let change_pending = Arc::new(AtomicBool::new(false));
        let svc = Arc::new(Self {
            hub: Arc::downgrade(hub),
            opts,
            backend: OnceLock::new(),
            tx: tx.clone(),
            change_pending: change_pending.clone(),
            outgoing: Mutex::new(HashMap::new()),
            ui: Mutex::new(UiState::default()),
            next_xfer: AtomicU32::new(1),
            next_share: AtomicU32::new(1),
        });
        // Coalesce change notifications: one queued `LocalChanged` is enough.
        let backend = make_backend(Box::new(move || {
            if !change_pending.swap(true, Ordering::SeqCst) && tx.try_send(Work::LocalChanged).is_err() {
                change_pending.store(false, Ordering::SeqCst);
            }
        }))?;
        let _ = svc.backend.set(backend);
        let worker = Worker {
            svc: Arc::downgrade(&svc),
            state: SyncState::new(svc.opts.device_id.clone()),
            current: None,
            incoming: HashMap::new(),
            shares: HashMap::new(),
            acks: HashMap::new(),
            listening: false,
            enabled: false,
            read_failures: 0,
        };
        thread::Builder::new().name("clipboard-sync".into()).spawn(move || worker.run(rx))?;
        hub.register(svc.clone());
        Ok(svc)
    }

    /// Starts the service on the Windows clipboard.
    pub fn start_windows(hub: &Arc<FeatureHub>, opts: ServiceOptions) -> io::Result<Arc<Self>> {
        Self::start(hub, opts, |cb| Ok(Arc::new(crate::win::clipboard::WinClipboard::start(cb)?) as Arc<dyn ClipboardBackend>))
    }

    fn config(&self) -> ClipboardConfig {
        (self.opts.settings)()
    }

    fn backend(&self) -> &Arc<dyn ClipboardBackend> {
        self.backend.get().expect("backend is set in start")
    }

    fn changed(&self) {
        (self.opts.events)(ClipboardEvent::Changed);
    }

    // ------------------------------------------------------------- UI API

    pub fn status(&self) -> StatusView {
        let ui = self.ui.lock().unwrap();
        StatusView { history_count: ui.history.len() as u32, history_revision: ui.revision, ..ui.status.clone() }
    }

    /// Clips currently being sent (one per peer at most).
    pub fn sending(&self) -> usize {
        self.outgoing.lock().unwrap().len()
    }

    pub fn history(&self) -> Vec<HistoryEntryView> {
        if !self.config().clipboard.history {
            return Vec::new();
        }
        let ui = self.ui.lock().unwrap();
        ui.history
            .iter()
            .map(|h| HistoryEntryView {
                id: format!("c{}", h.id),
                kind: h.kind,
                preview: h.preview.clone(),
                bytes: h.bytes as u64,
                origin: h.origin,
                source: h.source.clone(),
                at: h.at,
            })
            .collect()
    }

    /// Puts a history entry back on the clipboard (and syncs it like a local copy).
    pub fn copy_entry(&self, id: &str) -> Result<(), String> {
        let clip = {
            let ui = self.ui.lock().unwrap();
            parse_id(id, 'c').and_then(|n| ui.history.iter().find(|h| h.id == n)).map(|h| h.clip.clone())
        };
        self.write_clip(clip.ok_or("that clipboard entry no longer exists")?)
    }

    pub fn delete_entry(&self, id: &str) -> Result<(), String> {
        {
            let mut ui = self.ui.lock().unwrap();
            let n = parse_id(id, 'c').ok_or("unknown clipboard entry")?;
            let before = ui.history.len();
            ui.history.retain(|h| h.id != n);
            if ui.history.len() == before {
                return Err("that clipboard entry no longer exists".into());
            }
            ui.history_bytes = ui.history.iter().map(|h| h.bytes).sum();
            ui.revision += 1;
        }
        self.changed();
        Ok(())
    }

    pub fn clear_history(&self) {
        {
            let mut ui = self.ui.lock().unwrap();
            ui.history.clear();
            ui.history_bytes = 0;
            ui.revision += 1;
        }
        self.changed();
    }

    pub fn shared_texts(&self) -> Vec<SharedTextInfo> {
        let ui = self.ui.lock().unwrap();
        ui.shared
            .iter()
            .map(|s| SharedTextInfo {
                id: format!("s{}", s.id),
                incoming: s.incoming,
                peer_name: s.peer_name.clone(),
                preview: truncate_utf8(&s.text, SHARE_PREVIEW_BYTES).to_owned(),
                chars: s.text.chars().count() as u32,
                is_link: s.is_link,
                at: s.at,
            })
            .collect()
    }

    fn shared_text(&self, id: &str) -> Result<String, String> {
        let ui = self.ui.lock().unwrap();
        parse_id(id, 's')
            .and_then(|n| ui.shared.iter().find(|s| s.id == n))
            .map(|s| s.text.clone())
            .ok_or_else(|| "that shared text no longer exists".into())
    }

    /// Places a shared text on this computer's clipboard (only ever on request).
    pub fn copy_shared_text(&self, id: &str) -> Result<(), String> {
        let text = self.shared_text(id)?;
        self.write_clip(Arc::new(Clip::text(text)))
    }

    /// The link of a shared text, validated again (http/https only).
    pub fn shared_link(&self, id: &str) -> Result<String, String> {
        let text = self.shared_text(id)?;
        as_link(&text).map(str::to_owned).ok_or_else(|| "this shared text is not a web link".into())
    }

    pub fn dismiss_shared_text(&self, id: &str) -> Result<(), String> {
        {
            let mut ui = self.ui.lock().unwrap();
            let n = parse_id(id, 's').ok_or("unknown shared text")?;
            ui.shared.retain(|s| s.id != n);
        }
        self.changed();
        Ok(())
    }

    /// Sends text to `peer` (`None`: every connected peer that accepts shared text, which
    /// on a client is the host). Blocks while the text is queued; call from a worker thread.
    pub fn share_text(&self, peer: Option<&str>, text: &str) -> Result<(), String> {
        validate_share_text(text)?;
        let hub = self.hub.upgrade().ok_or("not running")?;
        let targets: Vec<PeerSummary> = match peer {
            Some(id) => {
                let p = hub.peer(id).ok_or("that computer is not connected")?;
                if p.effective_caps & cap::TEXT_SHARE == 0 {
                    return Err(format!("{} does not accept shared text", p.name));
                }
                vec![p]
            }
            None => hub.peers().into_iter().filter(|p| p.effective_caps & cap::TEXT_SHARE != 0).collect(),
        };
        if targets.is_empty() {
            return Err("no connected computer accepts shared text".into());
        }
        let msgs: Vec<Vec<u8>> = share_messages(self.next_share.fetch_add(1, Ordering::Relaxed), text).iter().map(ClipMsg::encode).collect();
        let mut delivered = Vec::new();
        for p in &targets {
            let Some(sender) = hub.sender(&p.id) else { continue };
            if msgs.iter().all(|m| sender.send_feature(channel::CLIPBOARD, m.clone(), SEND_TIMEOUT)) {
                delivered.push(p.name.clone());
            } else {
                log::warn!("could not share text with {}", p.name);
            }
        }
        if delivered.is_empty() {
            return Err("the text could not be sent".into());
        }
        log::info!("shared {} characters with {}", text.chars().count(), delivered.join(", "));
        self.ui.lock().unwrap().add_shared(false, delivered.join(", "), text.to_owned());
        self.changed();
        Ok(())
    }

    fn write_clip(&self, clip: Arc<Clip>) -> Result<(), String> {
        let (reply, rx) = mpsc::sync_channel(1);
        self.tx.try_send(Work::Write { clip, reply }).map_err(|_| "the clipboard is busy; try again".to_string())?;
        rx.recv_timeout(Duration::from_secs(10)).map_err(|_| "the clipboard did not respond".to_string())?
    }

    // ------------------------------------------------------------- sending

    /// Starts streaming `payload` to `peer`, superseding an unfinished clip to it.
    fn start_send(self: &Arc<Self>, peer: &str, mut offer: Offer, payload: Arc<Vec<u8>>, window_bytes: usize) -> bool {
        let Some(sender) = self.hub.upgrade().and_then(|h| h.sender(peer)) else { return false };
        offer.xfer = self.next_xfer.fetch_add(1, Ordering::Relaxed);
        let window = Arc::new(Window::new(window_bytes));
        let old = self.outgoing.lock().unwrap().insert(peer.to_owned(), OutXfer { xfer: offer.xfer, hash: offer.hash, window: window.clone() });
        if let Some(old) = old {
            old.window.close();
            sender.try_send_feature(channel::CLIPBOARD, ClipMsg::Abort { xfer: old.xfer }.encode());
        }
        let svc = Arc::downgrade(self);
        let peer = peer.to_owned();
        let spawned = thread::Builder::new()
            .name("clipboard-send".into())
            .spawn(move || send_clip(svc, peer, sender, offer, payload, window));
        spawned.is_ok()
    }

    fn cancel_outgoing(&self, peer: &str) {
        if let Some(o) = self.outgoing.lock().unwrap().remove(peer) {
            o.window.close();
        }
    }

    fn cancel_all_outgoing(&self) {
        let all: Vec<(String, OutXfer)> = self.outgoing.lock().unwrap().drain().collect();
        let hub = self.hub.upgrade();
        for (peer, o) in all {
            o.window.close();
            if let Some(s) = hub.as_ref().and_then(|h| h.sender(&peer)) {
                s.try_send_feature(channel::CLIPBOARD, ClipMsg::Abort { xfer: o.xfer }.encode());
            }
        }
    }

    fn reply(&self, peer: &str, msg: ClipMsg) -> bool {
        self.hub.upgrade().and_then(|h| h.sender(peer)).is_some_and(|s| s.try_send_feature(channel::CLIPBOARD, msg.encode()))
    }
}

/// Streams one clip to one peer: offer, then chunks as the flow-control window allows.
fn send_clip(svc: Weak<ClipboardService>, peer: String, sender: PeerSender, offer: Offer, payload: Arc<Vec<u8>>, window: Arc<Window>) {
    let xfer = offer.xfer;
    let hash = offer.hash;
    let mut delivered = sender.send_feature(channel::CLIPBOARD, ClipMsg::Offer(offer).encode(), SEND_TIMEOUT);
    let mut offset = 0usize;
    while delivered && offset < payload.len() {
        let end = (offset + MAX_CHUNK_DATA).min(payload.len());
        match window.acquire(end - offset, ACQUIRE_TIMEOUT) {
            Acquire::Ok => {}
            Acquire::Closed => return, // superseded, cancelled or disconnected: someone else cleaned up
            Acquire::TimedOut => {
                log::warn!("clipboard transfer to {peer} stalled; giving up");
                sender.try_send_feature(channel::CLIPBOARD, ClipMsg::Abort { xfer }.encode());
                delivered = false;
                break;
            }
        }
        let chunk = ClipMsg::Chunk { xfer, offset: offset as u32, data: payload[offset..end].to_vec() };
        delivered = sender.send_feature(channel::CLIPBOARD, chunk.encode(), SEND_TIMEOUT);
        offset = end;
    }
    let Some(svc) = svc.upgrade() else { return };
    {
        let mut out = svc.outgoing.lock().unwrap();
        if out.get(&peer).is_some_and(|o| o.xfer == xfer) {
            out.remove(&peer);
        }
    }
    if !delivered && !window.is_closed() {
        let _ = svc.tx.try_send(Work::SendFailed { peer, hash });
    }
}

impl FeatureService for ClipboardService {
    fn channel(&self) -> u8 {
        channel::CLIPBOARD
    }

    fn peer_down(&self, peer_id: &str) {
        self.cancel_outgoing(peer_id);
        let _ = self.tx.try_send(Work::PeerDown(peer_id.to_owned()));
    }

    fn caps_changed(&self, peer_id: &str, effective: u32) {
        if effective & cap::CLIPBOARD == 0 {
            self.cancel_outgoing(peer_id);
        }
        let _ = self.tx.try_send(Work::CapsChanged { peer: peer_id.to_owned(), caps: effective });
    }

    fn on_message(&self, peer_id: &str, body: Vec<u8>) {
        let msg = match ClipMsg::decode(&body) {
            Ok(m) => m,
            Err(e) => {
                log::debug!("ignoring a malformed clipboard message from {peer_id}: {e}");
                return;
            }
        };
        match msg {
            ClipMsg::Ack { xfer, bytes } => {
                if let Some(o) = self.outgoing.lock().unwrap().get(peer_id).filter(|o| o.xfer == xfer) {
                    o.window.release(bytes as usize);
                }
            }
            ClipMsg::Reject { xfer, reason } => {
                let removed = {
                    let mut out = self.outgoing.lock().unwrap();
                    if out.get(peer_id).is_some_and(|o| o.xfer == xfer) { out.remove(peer_id) } else { None }
                };
                if let Some(o) = removed {
                    o.window.close();
                    log::debug!("{peer_id} declined a clip: {}", reject::name(reason));
                    if !matches!(reason, reject::DUPLICATE | reject::STALE | reject::ECHO) {
                        let _ = self.tx.try_send(Work::SendFailed { peer: peer_id.to_owned(), hash: o.hash });
                    }
                }
            }
            msg => {
                if let Err(e) = self.tx.try_send(Work::Remote { peer: peer_id.to_owned(), msg }) {
                    log::warn!("clipboard queue full; dropping a message from {peer_id}");
                    // Stop the sender instead of letting it stall on a broken transfer.
                    if let TrySendError::Full(Work::Remote { msg: ClipMsg::Offer(Offer { xfer, .. }) | ClipMsg::Chunk { xfer, .. }, .. }) = e {
                        self.reply(peer_id, ClipMsg::Reject { xfer, reason: reject::BUSY });
                    }
                }
            }
        }
    }
}

// ---------------------------------------------------------------- worker

struct Worker {
    svc: Weak<ClipboardService>,
    state: SyncState,
    /// The clip the sync currently holds (what a stale sender gets back) and its origin name.
    current: Option<(Arc<Clip>, String)>,
    incoming: HashMap<String, (Incoming, Instant)>,
    shares: HashMap<String, Option<ShareAssembler>>,
    /// Acknowledgements not sent yet (bulk queue was full): peer -> (xfer, bytes).
    acks: HashMap<String, (u32, u32)>,
    listening: bool,
    enabled: bool,
    /// Failed reads of the latest change so far (retried from housekeeping).
    read_failures: u32,
}

impl Worker {
    fn run(mut self, rx: Receiver<Work>) {
        let mut last_tick = Instant::now().checked_sub(TICK).unwrap_or_else(Instant::now);
        loop {
            let work = rx.recv_timeout(TICK);
            let Some(svc) = self.svc.upgrade() else { break };
            match work {
                Ok(w) => self.handle(&svc, w),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }
            if last_tick.elapsed() >= TICK {
                last_tick = Instant::now();
                self.housekeeping(&svc);
            }
        }
        if let Some(svc) = self.svc.upgrade() {
            svc.backend().set_listening(false);
        }
    }

    fn handle(&mut self, svc: &Arc<ClipboardService>, w: Work) {
        match w {
            Work::LocalChanged => self.local_changed(svc),
            Work::Remote { peer, msg } => self.remote(svc, &peer, msg),
            Work::CapsChanged { peer, caps } => {
                if caps & cap::CLIPBOARD != 0 {
                    svc.reply(&peer, ClipMsg::Clock { stamp: self.state.clock() });
                } else {
                    self.incoming.remove(&peer);
                }
            }
            Work::PeerDown(peer) => {
                self.incoming.remove(&peer);
                self.shares.remove(&peer);
                self.acks.remove(&peer);
                self.state.forget_peer(&peer);
            }
            Work::SendFailed { peer, hash } => self.state.unsent(&peer, hash),
            Work::Write { clip, reply } => {
                let r = svc.backend().write(clip.clone());
                let r = r.map(|seq| {
                    let cfg = svc.config();
                    if cfg.clipboard.enabled {
                        let filtered = clip.filtered(cfg.filter()).fit(cfg.max_bytes());
                        match filtered {
                            Some(f) => self.local_copy(svc, &cfg, f, Some(seq), false),
                            None => {
                                let _ = self.state.local_write(seq, clip.hash(), now_ms());
                            }
                        }
                    } else {
                        self.state.should_read(seq);
                    }
                });
                if let Err(e) = &r {
                    log::warn!("cannot write the clipboard: {e}");
                }
                let _ = reply.try_send(r.map_err(|e| format!("could not update the clipboard: {e}")));
            }
        }
    }

    /// Applies setting changes, retries acknowledgements and drops stalled transfers.
    fn housekeeping(&mut self, svc: &Arc<ClipboardService>) {
        let cfg = svc.config();
        let enabled = cfg.clipboard.enabled;
        if enabled != self.enabled {
            self.enabled = enabled;
            if enabled {
                // Whatever is on the clipboard now was not copied now: do not send it.
                let seq = svc.backend().sequence();
                self.state.should_read(seq);
            } else {
                for (peer, (inc, _)) in self.incoming.drain() {
                    svc.reply(&peer, ClipMsg::Reject { xfer: inc.offer.xfer, reason: reject::DISABLED });
                }
                svc.cancel_all_outgoing();
            }
            log::info!("clipboard sync {}", if enabled { "enabled" } else { "disabled" });
        }
        if enabled != self.listening {
            svc.backend().set_listening(enabled);
            self.listening = enabled;
        }
        let trimmed = {
            let mut ui = svc.ui.lock().unwrap();
            let rev = ui.revision;
            ui.trim(&cfg.clipboard);
            ui.revision != rev
        };
        if trimmed {
            svc.changed();
        }
        if self.read_failures > 0 && enabled {
            self.local_changed(svc);
        }
        self.flush_acks(svc);
        let stalled: Vec<String> = self.incoming.iter().filter(|(_, (_, t))| t.elapsed() > INCOMING_IDLE).map(|(p, _)| p.clone()).collect();
        for peer in stalled {
            if let Some((inc, _)) = self.incoming.remove(&peer) {
                log::warn!("clipboard transfer from {peer} stalled; dropped");
                svc.reply(&peer, ClipMsg::Reject { xfer: inc.offer.xfer, reason: reject::BUSY });
            }
        }
    }

    fn flush_acks(&mut self, svc: &Arc<ClipboardService>) {
        self.acks.retain(|peer, (xfer, bytes)| !svc.reply(peer, ClipMsg::Ack { xfer: *xfer, bytes: *bytes }));
    }

    fn ack(&mut self, svc: &Arc<ClipboardService>, peer: &str, xfer: u32, bytes: u32) {
        let e = self.acks.entry(peer.to_owned()).or_insert((xfer, 0));
        if e.0 != xfer {
            *e = (xfer, 0);
        }
        e.1 += bytes;
        let (x, b) = *e;
        if svc.reply(peer, ClipMsg::Ack { xfer: x, bytes: b }) {
            self.acks.remove(peer);
        }
    }

    // ------------------------------------------------------------- local copies

    fn local_changed(&mut self, svc: &Arc<ClipboardService>) {
        svc.change_pending.store(false, Ordering::SeqCst);
        let cfg = svc.config();
        if !cfg.clipboard.enabled {
            return;
        }
        let backend = svc.backend();
        if !self.state.should_read(backend.sequence()) {
            return;
        }
        let opts = ReadOptions { filter: cfg.filter(), skip_sensitive: cfg.clipboard.skip_sensitive, max_bytes: cfg.max_bytes() };
        let read = backend.read(&opts);
        if read.is_ok() {
            self.read_failures = 0;
        }
        match read {
            Err(e) => {
                self.read_failures += 1;
                if self.read_failures < READ_ATTEMPTS {
                    self.state.read_failed(); // retried from housekeeping
                    return;
                }
                self.read_failures = 0;
                log::warn!("cannot read the clipboard: {e}");
                svc.ui.lock().unwrap().status.error = Some(format!("Could not read the clipboard: {e}"));
                svc.changed();
            }
            Ok(LocalRead::Sensitive { seq }) => {
                self.state.should_read(seq);
                log::debug!("clipboard content marked as sensitive was not synchronized");
            }
            Ok(LocalRead::Clip { seq, clip, oversized }) => match clip.filtered(cfg.filter()).fit(cfg.max_bytes()) {
                Some(clip) => self.local_copy(svc, &cfg, clip, Some(seq), true),
                None => {
                    self.state.should_read(seq);
                    if oversized {
                        log::info!("clipboard content above the {} KB limit was not synchronized", cfg.clipboard.max_size_kb);
                        svc.ui.lock().unwrap().status.error =
                            Some(format!("The last copy was larger than {} KB and was not synchronized", cfg.clipboard.max_size_kb));
                        svc.changed();
                    }
                }
            },
        }
    }

    /// A new local clip: becomes the current clip and goes to every peer.
    /// `seq`: the sequence number it was read at (`read`) or written at by us (`!read`).
    fn local_copy(&mut self, svc: &Arc<ClipboardService>, cfg: &ClipboardConfig, clip: Clip, seq: Option<u32>, read: bool) {
        let hash = clip.hash();
        let version = match (seq, read) {
            (Some(s), false) => self.state.local_write(s, hash, now_ms()),
            _ => self.state.local_change(seq, hash, now_ms()),
        };
        let Some(version) = version else { return };
        let clip = Arc::new(clip);
        self.current = Some((clip.clone(), cfg.device_name.clone()));
        // Unfinished incoming clips are older than this copy by construction.
        for (peer, (inc, _)) in self.incoming.drain() {
            svc.reply(&peer, ClipMsg::Reject { xfer: inc.offer.xfer, reason: reject::STALE });
        }
        let sent = self.send(svc, cfg, &clip, &version, &cfg.device_name, hash, None);
        let mut ui = svc.ui.lock().unwrap();
        ui.add_history(&cfg.clipboard, clip.clone(), hash, if sent.is_empty() { "local" } else { "sent" }, None);
        if !sent.is_empty() {
            log::info!("sent a {} clip ({} bytes) to {}", clip.kind().as_str(), clip.size(), sent.join(", "));
            ui.synced("sent", sent.join(", "), clip.kind());
        }
        drop(ui);
        svc.changed();
    }

    /// Sends a clip to every peer that should get it; returns their names.
    #[allow(clippy::too_many_arguments)]
    fn send(
        &mut self,
        svc: &Arc<ClipboardService>,
        cfg: &ClipboardConfig,
        clip: &Clip,
        version: &Version,
        origin_name: &str,
        hash: u64,
        from: Option<&str>,
    ) -> Vec<String> {
        let Some(hub) = svc.hub.upgrade() else { return Vec::new() };
        let peers: Vec<PeerSummary> = hub.peers().into_iter().filter(|p| p.effective_caps & cap::CLIPBOARD != 0).collect();
        let ids: Vec<String> = peers.iter().map(|p| p.id.clone()).collect();
        let targets = self.state.targets(&ids, from, version, hash);
        if targets.is_empty() {
            return Vec::new();
        }
        let payload = Arc::new(encode_payload(clip, crate::win::clipboard::compress));
        let offer = Offer {
            xfer: 0,
            version: version.clone(),
            origin_name: origin_name.to_owned(),
            hash,
            kind: clip.kind(),
            raw_len: clip.size() as u32,
            wire_len: payload.len() as u32,
        };
        let mut names = Vec::new();
        for peer in targets {
            if svc.start_send(&peer, offer.clone(), payload.clone(), cfg.window_bytes) {
                self.state.sent(&peer, hash);
                names.push(peers.iter().find(|p| p.id == peer).map(|p| p.name.clone()).unwrap_or(peer));
            }
        }
        names
    }

    /// Our clip is newer than what `peer` offered: make sure the peer gets ours.
    fn send_back(&mut self, svc: &Arc<ClipboardService>, peer: &str) {
        let (Some((clip, origin_name)), Some(cur)) = (self.current.clone(), self.state.current().cloned()) else { return };
        let Some(hub) = svc.hub.upgrade() else { return };
        if hub.effective_caps(peer) & cap::CLIPBOARD == 0 || cur.version.origin == peer {
            return;
        }
        let cfg = svc.config();
        let hash = clip.hash();
        let payload = Arc::new(encode_payload(&clip, crate::win::clipboard::compress));
        let offer = Offer {
            xfer: 0,
            version: cur.version,
            origin_name,
            hash,
            kind: clip.kind(),
            raw_len: clip.size() as u32,
            wire_len: payload.len() as u32,
        };
        if svc.start_send(peer, offer, payload, cfg.window_bytes) {
            self.state.sent(peer, hash);
        }
    }

    // ------------------------------------------------------------- remote traffic

    fn remote(&mut self, svc: &Arc<ClipboardService>, peer: &str, msg: ClipMsg) {
        match msg {
            ClipMsg::Offer(offer) => self.offer(svc, peer, offer),
            ClipMsg::Chunk { xfer, offset, data } => {
                let Some((inc, at)) = self.incoming.get_mut(peer).filter(|(i, _)| i.offer.xfer == xfer) else { return };
                *at = Instant::now();
                match inc.push(offset, &data) {
                    ChunkResult::More => self.ack(svc, peer, xfer, data.len() as u32),
                    ChunkResult::Complete => {
                        self.ack(svc, peer, xfer, data.len() as u32);
                        if let Some((inc, _)) = self.incoming.remove(peer) {
                            self.complete(svc, peer, inc);
                        }
                    }
                    ChunkResult::Invalid => {
                        self.incoming.remove(peer);
                        log::warn!("clipboard data from {peer} arrived out of order; transfer dropped");
                        svc.reply(peer, ClipMsg::Reject { xfer, reason: reject::INVALID });
                    }
                }
            }
            ClipMsg::Abort { xfer } => {
                if self.incoming.get(peer).is_some_and(|(i, _)| i.offer.xfer == xfer) {
                    self.incoming.remove(peer);
                }
            }
            ClipMsg::Share { id, total, offset, data, .. } => {
                let slot = self.shares.entry(peer.to_owned()).or_default();
                match ShareAssembler::push(slot, id, total, offset, &data) {
                    ShareResult::More => {}
                    ShareResult::Invalid => log::debug!("dropped an invalid shared text from {peer}"),
                    ShareResult::Done(text) => self.shared_text_received(svc, peer, text),
                }
            }
            ClipMsg::Clock { stamp } => self.state.observe(stamp),
            ClipMsg::Ack { .. } | ClipMsg::Reject { .. } => {} // handled on the reader thread
        }
    }

    /// Whether this side currently accepts clips from `peer`.
    fn accepts(&self, svc: &Arc<ClipboardService>, cfg: &ClipboardConfig, peer: &str) -> bool {
        cfg.clipboard.enabled && svc.hub.upgrade().is_some_and(|h| h.local_caps(peer) & cap::CLIPBOARD != 0)
    }

    fn offer(&mut self, svc: &Arc<ClipboardService>, peer: &str, offer: Offer) {
        // A new offer supersedes the peer's unfinished clip.
        self.incoming.remove(peer);
        let cfg = svc.config();
        let xfer = offer.xfer;
        if !self.accepts(svc, &cfg, peer) {
            svc.reply(peer, ClipMsg::Reject { xfer, reason: reject::DISABLED });
            return;
        }
        if offer.raw_len as usize > cfg.max_bytes() {
            log::info!("a clip from {peer} ({} bytes) is above the size limit", offer.raw_len);
            svc.reply(peer, ClipMsg::Reject { xfer, reason: reject::TOO_LARGE });
            return;
        }
        match self.state.check(peer, &offer.version, offer.hash) {
            Decision::Accept => {
                self.incoming.insert(peer.to_owned(), (Incoming::new(offer), Instant::now()));
            }
            Decision::Reject { reason, send_back } => {
                svc.reply(peer, ClipMsg::Reject { xfer, reason });
                if send_back {
                    self.send_back(svc, peer);
                }
            }
        }
    }

    fn complete(&mut self, svc: &Arc<ClipboardService>, peer: &str, inc: Incoming) {
        let Incoming { offer, buf } = inc;
        let cfg = svc.config();
        let refuse = |reason| svc.reply(peer, ClipMsg::Reject { xfer: offer.xfer, reason });
        if !self.accepts(svc, &cfg, peer) {
            refuse(reject::DISABLED);
            return;
        }
        let decompress = |c: u8, data: &[u8], n: usize| (c == codec::XPRESS_HUFF).then(|| crate::win::clipboard::decompress(data, n)).flatten();
        let clip = match decode_payload(&buf, offer.raw_len as usize, cfg.max_bytes(), decompress) {
            Ok(c) if c.hash() == offer.hash => c,
            Ok(_) => {
                log::warn!("a clip from {peer} does not match its hash; dropped");
                refuse(reject::INVALID);
                return;
            }
            Err(e) => {
                log::warn!("invalid clip from {peer}: {e}");
                refuse(reject::INVALID);
                return;
            }
        };
        drop(buf);
        match self.state.check(peer, &offer.version, offer.hash) {
            Decision::Accept => {}
            Decision::Reject { reason, send_back } => {
                refuse(reason);
                if send_back {
                    self.send_back(svc, peer);
                }
                return;
            }
        }
        let clip = clip.filtered(cfg.filter());
        if clip.is_empty() {
            log::debug!("a clip from {peer} has no format enabled on this computer");
            return;
        }
        let written = clip.hash();
        let clip = Arc::new(clip);
        let seq = match svc.backend().write(clip.clone()) {
            Ok(seq) => seq,
            Err(e) => {
                log::warn!("cannot write the clipboard: {e}");
                svc.ui.lock().unwrap().status.error = Some(format!("Could not update the clipboard: {e}"));
                svc.changed();
                return;
            }
        };
        self.state.applied(peer, offer.version.clone(), offer.hash, written, seq);
        let origin_name = if offer.origin_name.is_empty() { peer.to_owned() } else { offer.origin_name.clone() };
        self.current = Some((clip.clone(), origin_name.clone()));
        log::info!("received a {} clip ({} bytes) from {origin_name}", clip.kind().as_str(), clip.size());
        let relayed = self.send(svc, &cfg, &clip, &offer.version, &origin_name, written, Some(peer));
        if !relayed.is_empty() {
            log::info!("relayed it to {}", relayed.join(", "));
        }
        {
            let mut ui = svc.ui.lock().unwrap();
            ui.add_history(&cfg.clipboard, clip.clone(), written, "received", Some(origin_name.clone()));
            ui.synced("received", origin_name.clone(), clip.kind());
        }
        (svc.opts.events)(ClipboardEvent::ClipReceived { from: origin_name, kind: clip.kind() });
        svc.changed();
    }

    fn shared_text_received(&mut self, svc: &Arc<ClipboardService>, peer: &str, text: String) {
        let Some(hub) = svc.hub.upgrade() else { return };
        if hub.local_caps(peer) & cap::TEXT_SHARE == 0 {
            log::info!("ignored text shared by {peer}: not allowed for this device");
            return;
        }
        let name = hub.peer(peer).map(|p| p.name).unwrap_or_else(|| peer.to_owned());
        let chars = text.chars().count();
        let is_link = as_link(&text).is_some();
        log::info!("received shared {} ({chars} characters) from {name}", if is_link { "link" } else { "text" });
        svc.ui.lock().unwrap().add_shared(true, name.clone(), text);
        (svc.opts.events)(ClipboardEvent::TextShared { from: name, chars, is_link });
        svc.changed();
    }
}
