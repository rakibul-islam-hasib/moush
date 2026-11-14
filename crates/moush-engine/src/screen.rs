//! Remote screen viewing for paired phones: captures a display (or a zoomed part of it),
//! encodes JPEG frames and streams them over the feature channel
//! [`SCREEN`](moush_core::features::channel::SCREEN).
//!
//! One worker thread per viewing phone. It never blocks input: frames are sent through the
//! connection's bulk gate (a full gate ends the stream), at most
//! [`MAX_IN_FLIGHT`](moush_core::screen::MAX_IN_FLIGHT) frames are unacknowledged, and a
//! static screen costs no bandwidth (unchanged pictures are not re-sent).
//!
//! Only devices paired as phone remotes may watch, on top of the hub's capability check
//! (the per-device "Screen viewing" permission).

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, Weak};
use std::thread;
use std::time::{Duration, Instant};

use moush_core::features::{cap, channel};
use moush_core::net::PeerSender;
use moush_core::screen::{self, DisplayInfo, MAX_IN_FLIGHT, NO_CURSOR, ScreenMsg, View};
use moush_core::trust::{TrustRole, TrustStore};

use crate::features::{FeatureHub, FeatureService};
use crate::win::dpi::make_thread_dpi_aware;
use crate::win::monitors::{self, Monitor};
use crate::win::screen_grab::{Grabber, cursor_pos};

const SEND_TIMEOUT: Duration = Duration::from_millis(400);
/// Frame numbers keep increasing across streams, so a phone can drop stale frames by `seq`
/// even when it restarts a view.
static NEXT_BASE: AtomicU32 = AtomicU32::new(0);

struct Viewer {
    stop: AtomicBool,
    view: Mutex<Option<View>>,
    wake: Condvar,
    acked: AtomicU32,
    base: u32,
}

impl Viewer {
    fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}

pub struct ScreenService {
    trust: Arc<TrustStore>,
    hub: OnceLock<Weak<FeatureHub>>,
    viewers: Mutex<HashMap<String, Arc<Viewer>>>,
}

impl ScreenService {
    pub fn new(trust: Arc<TrustStore>) -> Arc<Self> {
        Arc::new(Self { trust, hub: OnceLock::new(), viewers: Mutex::new(HashMap::new()) })
    }

    /// Registers the service with `hub` (once).
    pub fn attach(self: &Arc<Self>, hub: &Arc<FeatureHub>) {
        if self.hub.set(Arc::downgrade(hub)).is_ok() {
            hub.register(self.clone());
        }
    }

    fn sender(&self, peer: &str) -> Option<PeerSender> {
        self.hub.get()?.upgrade()?.sender(peer)
    }

    fn stop_viewer(&self, peer: &str) {
        if let Some(v) = self.viewers.lock().unwrap().remove(peer) {
            v.stop.store(true, Ordering::Relaxed);
            v.wake.notify_all();
        }
    }

    fn is_remote(&self, peer: &str) -> bool {
        self.trust.get(peer).is_some_and(|p| p.role == TrustRole::Remote)
    }
}

impl FeatureService for ScreenService {
    fn channel(&self) -> u8 {
        channel::SCREEN
    }

    fn peer_down(&self, peer_id: &str) {
        self.stop_viewer(peer_id);
    }

    fn caps_changed(&self, peer_id: &str, effective: u32) {
        if effective & cap::SCREEN == 0 {
            self.stop_viewer(peer_id);
        }
    }

    fn on_message(&self, peer_id: &str, body: Vec<u8>) {
        let Ok(msg) = ScreenMsg::decode(&body) else {
            log::debug!("ignoring a malformed screen message");
            return;
        };
        if !self.is_remote(peer_id) {
            return;
        }
        match msg {
            ScreenMsg::List => {
                if let Some(sender) = self.sender(peer_id) {
                    // Enumerating monitors takes a few ms: keep it off the reader thread.
                    let _ = thread::Builder::new().name("screen-list".into()).spawn(move || {
                        make_thread_dpi_aware();
                        let displays = display_infos(&monitors::enumerate());
                        sender.send_feature(channel::SCREEN, ScreenMsg::Displays { displays }.encode(), SEND_TIMEOUT);
                    });
                }
            }
            ScreenMsg::View(view) => {
                let mut viewers = self.viewers.lock().unwrap();
                if let Some(v) = viewers.get(peer_id).filter(|v| !v.stopped()) {
                    *v.view.lock().unwrap() = Some(view);
                    v.wake.notify_all();
                    return;
                }
                let Some(sender) = self.sender(peer_id) else { return };
                let base = NEXT_BASE.fetch_add(1 << 20, Ordering::Relaxed);
                let viewer = Arc::new(Viewer {
                    stop: AtomicBool::new(false),
                    view: Mutex::new(Some(view)),
                    wake: Condvar::new(),
                    acked: AtomicU32::new(base),
                    base,
                });
                viewers.insert(peer_id.to_owned(), viewer.clone());
                let name = format!("screen-{peer_id}");
                if thread::Builder::new().name(name).spawn(move || stream(viewer, sender)).is_err() {
                    viewers.remove(peer_id);
                }
            }
            ScreenMsg::Stop => self.stop_viewer(peer_id),
            ScreenMsg::Ack { seq } => {
                if let Some(v) = self.viewers.lock().unwrap().get(peer_id) {
                    v.acked.fetch_max(seq, Ordering::Relaxed);
                    v.wake.notify_all();
                }
            }
            _ => {}
        }
    }
}

fn display_infos(monitors: &[Monitor]) -> Vec<DisplayInfo> {
    monitors
        .iter()
        .take(screen::MAX_DISPLAYS)
        .map(|m| DisplayInfo {
            x: m.rect.x,
            y: m.rect.y,
            width: m.rect.w,
            height: m.rect.h,
            primary: m.primary,
            name: m.name.clone(),
        })
        .collect()
}

/// Cheap change detector: folds every 61st 8-byte word (a prime stride, so thin horizontal
/// patterns cannot hide) plus the buffer length.
fn fingerprint(buf: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ buf.len() as u64;
    let words = buf.len() / 8;
    let mut i = 0;
    while i < words {
        let w = u64::from_le_bytes(buf[i * 8..i * 8 + 8].try_into().unwrap());
        h = (h ^ w).wrapping_mul(0x0100_0000_01b3);
        h ^= h >> 29;
        i += 61;
    }
    h
}

fn stream(viewer: Arc<Viewer>, sender: PeerSender) {
    make_thread_dpi_aware();
    let Some(mut grab) = Grabber::new() else {
        let stopped = ScreenMsg::Stopped { reason: "Cannot capture the screen.".into() };
        sender.send_feature(channel::SCREEN, stopped.encode(), SEND_TIMEOUT);
        return;
    };
    let mut seq: u32 = viewer.base;
    let mut jpeg: Vec<u8> = Vec::with_capacity(256 * 1024);
    let mut last_view: Option<View> = None;
    let mut last_print: u64 = 0;
    let mut last_cursor: Option<(i32, i32)> = None;
    let mut idle_frames = 0u32;
    let mut next = Instant::now();
    let mut stalled_since: Option<Instant> = None;
    log::info!("screen stream started");
    loop {
        if viewer.stopped() {
            break;
        }
        // Wait for the pacing deadline and for room in the in-flight window.
        {
            let guard = viewer.view.lock().unwrap();
            let now = Instant::now();
            let in_flight = seq.saturating_sub(viewer.acked.load(Ordering::Relaxed));
            if now < next || in_flight >= MAX_IN_FLIGHT {
                let wait = if now < next { next - now } else { Duration::from_millis(30) };
                let _ = viewer.wake.wait_timeout(guard, wait.min(Duration::from_millis(100)));
                if in_flight >= MAX_IN_FLIGHT {
                    // A dead phone never acknowledges; keepalive closes the connection, this
                    // is only a safety net.
                    if stalled_since.get_or_insert(now).elapsed() > Duration::from_secs(8) {
                        log::info!("screen viewer stopped acknowledging; ending the stream");
                        break;
                    }
                } else {
                    stalled_since = None;
                }
                continue;
            }
            stalled_since = None;
        }
        let Some(view) = *viewer.view.lock().unwrap() else { continue };
        let mons = monitors::enumerate();
        let Some(mon) = mons.get(view.display as usize).or_else(|| mons.first()) else { continue };
        let display = mons.iter().position(|m| m == mon).unwrap_or(0) as u8;

        // Crop (display-relative), clamped to the display; 0 width = the whole display.
        let (dw, dh) = (mon.rect.w.max(1), mon.rect.h.max(1));
        let (mut cx, mut cy, mut cw, mut ch) = (view.crop_x, view.crop_y, view.crop_w as i32, view.crop_h as i32);
        if cw <= 0 || ch <= 0 {
            (cx, cy, cw, ch) = (0, 0, dw, dh);
        }
        cw = cw.clamp(16.min(dw), dw);
        ch = ch.clamp(16.min(dh), dh);
        cx = cx.clamp(0, dw - cw);
        cy = cy.clamp(0, dh - ch);
        // Fit the crop into the requested output size, never enlarging.
        let scale = (view.out_w as f64 / cw as f64).min(view.out_h as f64 / ch as f64).min(1.0);
        let (ow, oh) = (((cw as f64 * scale) as i32).max(2) & !1, ((ch as f64 * scale) as i32).max(2) & !1);

        let cursor = cursor_pos().and_then(|(x, y)| {
            let (lx, ly) = (x - mon.rect.x, y - mon.rect.y);
            (lx >= 0 && ly >= 0 && lx < dw && ly < dh).then_some((lx, ly))
        });
        let view_changed = last_view != Some(view);
        let interval = Duration::from_millis(if idle_frames > 12 { 200 } else { 1000 / view.fps as u64 });
        next = Instant::now() + interval;

        let Some(pixels) = grab.grab((mon.rect.x + cx, mon.rect.y + cy, cw, ch), (ow, oh)) else {
            // Locked screen, secure desktop, display change: try again shortly.
            next = Instant::now() + Duration::from_millis(500);
            continue;
        };
        let print = fingerprint(pixels);
        if !view_changed && print == last_print {
            idle_frames = idle_frames.saturating_add(1);
            if cursor != last_cursor {
                last_cursor = cursor;
                let (x, y) = cursor.unwrap_or((NO_CURSOR, NO_CURSOR));
                if !sender.send_feature(channel::SCREEN, ScreenMsg::Cursor { display, x, y }.encode(), SEND_TIMEOUT) {
                    break;
                }
            }
            continue;
        }
        idle_frames = 0;
        jpeg.clear();
        let enc = jpeg_encoder::Encoder::new(&mut jpeg, view.quality);
        if enc.encode(pixels, ow as u16, oh as u16, jpeg_encoder::ColorType::Bgra).is_err() {
            log::warn!("JPEG encoding failed");
            break;
        }
        seq += 1;
        let (x, y) = cursor.unwrap_or((NO_CURSOR, NO_CURSOR));
        let crop = (cx, cy, cw as u32, ch as u32);
        let chunks = screen::frame_chunks(seq, display, crop, (x, y), (ow as u16, oh as u16), &jpeg);
        // A saturated connection ends the stream: a partial frame is useless.
        if !chunks.into_iter().all(|c| sender.send_feature(channel::SCREEN, c, SEND_TIMEOUT)) {
            break;
        }
        last_view = Some(view);
        last_print = print;
        last_cursor = cursor;
    }
    log::info!("screen stream ended");
}
