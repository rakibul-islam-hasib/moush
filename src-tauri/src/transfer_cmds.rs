//! File transfer commands, the UI view of transfers and their notifications.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use moush_engine::transfer::{self as xfer, Event, TransferInfo};
use moush_engine::win::dialogs;

use crate::app::App;
use crate::dto::{TransferFile, TransferView};
use crate::notify::{self, Kind};

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("internal error: {e}"))?
}

pub fn view(t: TransferInfo) -> TransferView {
    TransferView {
        id: t.id,
        direction: if t.outgoing { "send" } else { "receive" },
        peer_id: t.peer_id,
        peer_name: t.peer_name,
        files: t.files.into_iter().map(|f| TransferFile { name: f.name, size: f.size }).collect(),
        total_bytes: t.total_bytes,
        done_bytes: t.done_bytes,
        bytes_per_sec: t.bytes_per_sec,
        state: t.state.as_str(),
        error: t.error,
        started_at: t.started_at,
        finished_at: t.finished_at,
        destination: t.destination.map(|p| p.display().to_string()),
        can_retry: t.can_retry,
    }
}

/// Events from the transfer service: snapshot requests and notifications.
pub fn on_event(app: &App, e: Event) {
    match e {
        Event::Changed => app.notify(),
        Event::Incoming { peer_name, files, bytes, needs_approval: true, .. } => {
            let what = if files == 1 { "a file".to_string() } else { format!("{files} files") };
            notify::send(
                app,
                Kind::Transfer,
                &format!("{peer_name} wants to send you {what}"),
                &format!("{}. Open Moush to accept or decline.", size(bytes)),
            );
        }
        Event::Incoming { .. } => {}
        Event::Finished(t) => finished(app, &t),
    }
}

fn finished(app: &App, t: &TransferInfo) {
    use xfer::State;
    let what = match t.files.as_slice() {
        [one] => format!("\"{}\"", one.name),
        files => format!("{} files", files.len()),
    };
    let peer = &t.peer_name;
    let (title, body) = match (t.state, t.outgoing) {
        (State::Done, true) => (format!("Sent {what} to {peer}"), size(t.total_bytes)),
        (State::Done, false) => {
            let place = t.destination.as_ref().map(|d| format!("Saved in {}", d.display())).unwrap_or_default();
            (format!("Received {what} from {peer}"), place)
        }
        (State::Failed, true) => (format!("Sending to {peer} failed"), t.error.clone().unwrap_or_default()),
        (State::Failed, false) => (format!("Receiving from {peer} failed"), t.error.clone().unwrap_or_default()),
        (State::Rejected, true) => (format!("{peer} did not accept {what}"), t.error.clone().unwrap_or_default()),
        // Declined or cancelled here: the user knows. Expired, refused or cancelled remotely: tell.
        (State::Rejected, false) | (State::Cancelled, _) => match &t.error {
            Some(e) => (format!("Transfer with {peer} stopped"), e.clone()),
            None => return,
        },
        (State::Pending | State::Active, _) => return,
    };
    notify::send(app, Kind::Transfer, &title, &body);
}

fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} bytes");
    }
    let mut v = bytes as f64 / 1024.0;
    let mut u = 0;
    while v >= 1024.0 && u + 1 < UNITS.len() {
        v /= 1024.0;
        u += 1;
    }
    format!("{v:.1} {}", UNITS[u])
}

/// The peer to send to: the given one, or the only connected one.
fn target_peer(app: &App, peer_id: Option<String>) -> Res<String> {
    let peers = app.hub.peers();
    match peer_id {
        Some(id) if peers.iter().any(|p| p.id == id) => Ok(id),
        Some(_) => Err("That computer is not connected.".into()),
        None => match peers.as_slice() {
            [] => Err("No computer is connected.".into()),
            [one] => Ok(one.id.clone()),
            _ => Err("Choose which computer to send the files to.".into()),
        },
    }
}

/// The main window's handle, so pickers are modal to it.
fn owner(handle: &AppHandle) -> Option<isize> {
    handle.get_webview_window("main").and_then(|w| w.hwnd().ok()).map(|h| h.0 as isize)
}

#[tauri::command]
pub async fn send_files(app: State<'_, Arc<App>>, peer_id: Option<String>, paths: Vec<String>) -> Res<String> {
    let app = app.inner().clone();
    blocking(move || {
        let peer = target_peer(&app, peer_id)?;
        let paths: Vec<PathBuf> = paths.into_iter().map(PathBuf::from).collect();
        app.transfers.send(&peer, &paths)
    })
    .await
}

#[tauri::command]
pub async fn pick_files(handle: AppHandle) -> Res<Vec<String>> {
    let owner = owner(&handle);
    blocking(move || Ok(dialogs::pick_files(owner)?.into_iter().map(|p| p.display().to_string()).collect())).await
}

#[tauri::command]
pub async fn pick_folder(handle: AppHandle) -> Res<Option<String>> {
    let owner = owner(&handle);
    blocking(move || Ok(dialogs::pick_folder(owner)?.map(|p| p.display().to_string()))).await
}

#[tauri::command]
pub async fn respond_transfer(app: State<'_, Arc<App>>, id: String, accept: bool) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || app.transfers.respond(&id, accept)).await
}

#[tauri::command]
pub async fn cancel_transfer(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || app.transfers.cancel(&id)).await
}

#[tauri::command]
pub async fn retry_transfer(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || app.transfers.retry(&id)).await
}

#[tauri::command]
pub async fn clear_finished_transfers(app: State<'_, Arc<App>>) -> Res<()> {
    app.transfers.clear_finished();
    Ok(())
}

#[tauri::command]
pub async fn open_transfer_location(app: State<'_, Arc<App>>, id: String) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || {
        let path = app.transfers.location(&id).ok_or("The files are no longer there.")?;
        dialogs::reveal(&path);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn default_download_dir() -> Res<String> {
    blocking(|| Ok(xfer::default_download_dir().display().to_string())).await
}
