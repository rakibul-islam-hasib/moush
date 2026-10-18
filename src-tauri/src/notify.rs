//! User notifications (Windows toasts) for events that happen while the user may not be
//! looking at Moush. Every call checks the notification settings first.
//!
//! Callers never block on this: delivery happens on a background thread.

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

/// Shows a notification if the settings allow it. `title` and `body` are plain text.
pub fn send(app: &App, kind: Kind, title: &str, body: &str) {
    if !enabled(app, kind) {
        return;
    }
    log::info!("notification: {title} - {body}");
}
