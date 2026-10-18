//! Clipboard sync glue: starts the clipboard service with live settings, maps its state
//! into UI snapshots, and the Tauri commands for clipboard history and text sharing.
//! Commands run on blocking worker threads (clipboard access and sends may wait briefly).

use std::sync::Arc;

use tauri::State;

use moush_engine::clipboard::{ClipboardConfig, ClipboardEvent, ClipboardService, ServiceOptions};
use moush_engine::core::clipboard::ClipKind;
use moush_engine::core::features::ClipboardSettings;
use moush_engine::win::clipboard::open_url;

use crate::app::App;
use crate::dto::{ClipboardEntry, ClipboardStatus, SharedTextView};
use crate::notify::{self, Kind};

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("internal error: {e}"))?
}

/// Starts the clipboard service on the Windows clipboard and registers it with the hub.
pub fn start(app: &Arc<App>, device_id: String) -> Result<Arc<ClipboardService>, String> {
    let settings = Arc::downgrade(app);
    let events = Arc::downgrade(app);
    ClipboardService::start_windows(
        &app.hub,
        ServiceOptions {
            device_id,
            settings: Box::new(move || match settings.upgrade() {
                Some(a) => {
                    let s = a.config().settings;
                    ClipboardConfig {
                        window_bytes: s.transfer.window_bytes(),
                        clipboard: s.clipboard,
                        device_name: s.device_name,
                    }
                }
                None => ClipboardConfig {
                    clipboard: ClipboardSettings { enabled: false, ..Default::default() },
                    window_bytes: 128 * 1024,
                    device_name: String::new(),
                },
            }),
            events: Box::new(move |e| {
                let Some(app) = events.upgrade() else { return };
                match e {
                    ClipboardEvent::Changed => app.notify(),
                    // Contents never go into notifications (they would end up in the log).
                    ClipboardEvent::ClipReceived { from, kind } => {
                        notify::send(
                            &app,
                            Kind::Clipboard,
                            "Clipboard updated",
                            &format!("{} copied on {from}", kind_label(kind)),
                        );
                    }
                    ClipboardEvent::TextShared { from, chars, is_link } => {
                        let (title, body) = if is_link {
                            (format!("Link from {from}"), "Open Moush to open or copy it".to_string())
                        } else {
                            (format!("Text from {from}"), format!("{chars} characters - open Moush to copy them"))
                        };
                        notify::send(&app, Kind::SharedText, &title, &body);
                    }
                }
            }),
        },
    )
    .map_err(|e| e.to_string())
}

fn kind_label(k: ClipKind) -> &'static str {
    match k {
        ClipKind::Text => "Text",
        ClipKind::Rich => "Formatted text",
        ClipKind::Image => "An image",
    }
}

/// `features.clipboard` of the UI snapshot.
pub fn status(app: &App, enabled: bool) -> ClipboardStatus {
    let Some(svc) = app.clipboard.get() else {
        return ClipboardStatus { enabled, error: Some("Clipboard sync is unavailable".into()), ..Default::default() };
    };
    let s = svc.status();
    ClipboardStatus {
        enabled,
        last_sync_at: s.last_sync_at,
        last_direction: s.last_direction,
        last_peer: s.last_peer,
        last_kind: s.last_kind.map(ClipKind::as_str),
        history_count: s.history_count,
        history_revision: s.history_revision,
        error: s.error,
    }
}

/// `features.shared_texts` of the UI snapshot.
pub fn shared_texts(app: &App) -> Vec<SharedTextView> {
    app.clipboard
        .get()
        .map(|svc| {
            svc.shared_texts()
                .into_iter()
                .map(|t| SharedTextView {
                    id: t.id,
                    direction: if t.incoming { "received" } else { "sent" },
                    peer_name: t.peer_name,
                    preview: t.preview,
                    chars: t.chars,
                    is_link: t.is_link,
                    at: t.at,
                })
                .collect()
        })
        .unwrap_or_default()
}

fn service(app: &State<'_, Arc<App>>) -> Res<Arc<ClipboardService>> {
    app.clipboard.get().cloned().ok_or_else(|| "clipboard sync is unavailable".into())
}

#[tauri::command]
pub async fn get_clipboard_history(app: State<'_, Arc<App>>) -> Res<Vec<ClipboardEntry>> {
    let svc = service(&app)?;
    Ok(svc
        .history()
        .into_iter()
        .map(|e| ClipboardEntry {
            id: e.id,
            kind: e.kind.as_str(),
            preview: e.preview,
            bytes: e.bytes,
            origin: e.origin,
            source: e.source.unwrap_or_else(|| "This PC".into()),
            at: e.at,
        })
        .collect())
}

#[tauri::command]
pub async fn copy_clipboard_entry(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    let svc = service(&app)?;
    blocking(move || svc.copy_entry(&id)).await
}

#[tauri::command]
pub async fn delete_clipboard_entry(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    service(&app)?.delete_entry(&id)
}

#[tauri::command]
pub async fn clear_clipboard_history(app: State<'_, Arc<App>>) -> Res<()> {
    service(&app)?.clear_history();
    Ok(())
}

/// Shares text with a peer (`peer_id` null: the host on a client, every peer on a host).
#[tauri::command]
pub async fn share_text(app: State<'_, Arc<App>>, peer_id: Option<String>, text: String) -> Res<()> {
    let svc = service(&app)?;
    blocking(move || svc.share_text(peer_id.as_deref(), &text)).await
}

#[tauri::command]
pub async fn copy_shared_text(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    let svc = service(&app)?;
    blocking(move || svc.copy_shared_text(&id)).await
}

/// Opens a shared link in the default browser (http/https only, validated twice).
#[tauri::command]
pub async fn open_shared_link(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    let link = service(&app)?.shared_link(&id)?;
    blocking(move || open_url(&link).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn dismiss_shared_text(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    service(&app)?.dismiss_shared_text(&id)
}
