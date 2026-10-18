//! User notifications (Windows toasts) for events that happen while the user may not be
//! looking at Moush. Every call checks the notification settings first.
//!
//! Callers never block on this: delivery happens on a background thread
//! (`moush_engine::win::toast`). Clicking a toast opens the main window.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use moush_engine::paths::AppPaths;
use moush_engine::win::toast::Toaster;

use crate::app::App;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// A computer connected or disconnected.
    Connection,
    /// Incoming transfer request, completed or failed transfer.
    Transfer,
    /// Text or a link shared from another computer.
    SharedText,
    /// A clipboard synchronization.
    Clipboard,
}

/// True when the user wants notifications of this kind.
pub fn enabled(app: &App, kind: Kind) -> bool {
    let n = app.config().settings.notifications;
    n.enabled
        && match kind {
            Kind::Connection => n.connections,
            Kind::Transfer => n.transfers,
            Kind::SharedText => n.shared_text,
            Kind::Clipboard => n.clipboard,
        }
}

static TOASTER: OnceLock<Toaster> = OnceLock::new();

/// Shows a notification if the settings allow it. `title` and `body` are plain text.
pub fn send(app: &App, kind: Kind, title: &str, body: &str) {
    if !enabled(app, kind) {
        return;
    }
    // The body may quote shared text or file names: never log it.
    log::debug!("notification ({kind:?})");
    let toaster = TOASTER.get_or_init(|| {
        let handle = app.handle().clone();
        let aumid = handle.config().identifier.clone();
        Toaster::start(&aumid, "Moush", icon_file(&app.paths), move || crate::show_main_window(&handle))
    });
    if !toaster.show(title, body) {
        log::debug!("notification dropped (too many at once)");
    }
}

/// The app icon as a file, shown in toasts when Moush runs without being installed.
fn icon_file(paths: &AppPaths) -> Option<PathBuf> {
    const ICON: &[u8] = include_bytes!("../icons/128x128.png");
    let path = paths.config_dir.join("notification-icon.png");
    if std::fs::read(&path).ok().as_deref() != Some(ICON) {
        std::fs::create_dir_all(&paths.config_dir).ok()?;
        std::fs::write(&path, ICON).ok()?;
    }
    Some(path)
}

/// A computer that disconnects and comes back within this time is not announced (Wi-Fi
/// hiccups, sleep/wake reconnect storms).
const GRACE: Duration = Duration::from_secs(5);

/// Announces computers connecting and disconnecting, from successive peer lists.
#[derive(Default)]
pub struct ConnectionWatch {
    known: HashMap<String, String>,
    /// Disconnected peers not announced yet, with the time they left.
    gone: HashMap<String, (String, Instant)>,
}

impl ConnectionWatch {
    /// `peers` are the connected (id, name) pairs; `running` is false when sharing or the
    /// connection was stopped on purpose, which is never announced.
    pub fn update(&mut self, app: &Arc<App>, peers: Vec<(String, String)>, running: bool) {
        if !running {
            self.known.clear();
            self.gone.clear();
            return;
        }
        let now = Instant::now();
        for (id, name) in &peers {
            if !self.known.contains_key(id) && self.gone.remove(id).is_none() {
                send(app, Kind::Connection, &format!("{name} connected"), "Mouse and keyboard sharing is ready.");
            }
        }
        for (id, name) in &self.known {
            if !peers.iter().any(|(p, _)| p == id) {
                self.gone.insert(id.clone(), (name.clone(), now));
                // Look again once the grace period is over.
                let weak = Arc::downgrade(app);
                let _ = thread::Builder::new().name("connection-grace".into()).spawn(move || {
                    thread::sleep(GRACE + Duration::from_millis(100));
                    if let Some(app) = weak.upgrade() {
                        app.notify();
                    }
                });
            }
        }
        self.known = peers.into_iter().collect();
        let due: Vec<String> = self.gone.iter().filter(|(_, (_, at))| now.duration_since(*at) >= GRACE).map(|(id, _)| id.clone()).collect();
        for id in due {
            if let Some((name, _)) = self.gone.remove(&id) {
                send(app, Kind::Connection, &format!("{name} disconnected"), "Moush reconnects automatically when it is back.");
            }
        }
    }
}
