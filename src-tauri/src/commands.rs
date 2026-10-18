//! Tauri commands. Every command runs on a blocking worker thread, so starting or
//! stopping services never freezes the window. Errors reach the UI as strings.

use std::sync::Arc;

use serde::Deserialize;
use tauri::{AppHandle, State};

use crate::app::App;
use crate::dto::{AppSnapshot, Diagnostics, LatencyTestResult};
use crate::settings::{Role, Settings};
use moush_engine::logger::LogEntry;
use moush_engine::win::system;

type Res<T> = Result<T, String>;

async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| format!("internal error: {e}"))?
}

#[tauri::command]
pub async fn get_snapshot(app: State<'_, Arc<App>>) -> Res<AppSnapshot> {
    let app = app.inner().clone();
    blocking(move || Ok(app.snapshot())).await
}

#[tauri::command]
pub async fn set_role(app: State<'_, Arc<App>>, role: Option<Role>) -> Res<AppSnapshot> {
    let app = app.inner().clone();
    blocking(move || {
        app.set_role(role)?;
        Ok(app.snapshot())
    })
    .await
}

#[tauri::command]
pub async fn get_settings(app: State<'_, Arc<App>>) -> Res<Settings> {
    Ok(app.config().settings)
}

#[tauri::command]
pub async fn update_settings(app: State<'_, Arc<App>>, handle: AppHandle, settings: Settings) -> Res<Settings> {
    let app = app.inner().clone();
    let theme = settings.theme.clone();
    let r = blocking(move || app.update_settings(settings)).await?;
    crate::apply_theme(&handle, &theme);
    Ok(r)
}

#[tauri::command]
pub async fn host_start(app: State<'_, Arc<App>>) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || app.host_start()).await
}

#[tauri::command]
pub async fn host_stop(app: State<'_, Arc<App>>) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || {
        app.host_stop();
        Ok(())
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Placement {
    client_id: String,
    x: f64,
    y: f64,
}

#[tauri::command]
pub async fn host_set_layout(app: State<'_, Arc<App>>, placements: Vec<Placement>) -> Res<()> {
    if placements.iter().any(|p| !p.x.is_finite() || !p.y.is_finite() || p.x.abs() > 1e6 || p.y.abs() > 1e6) {
        return Err("invalid position".into());
    }
    app.with_host(|h| h.set_placements(placements.into_iter().map(|p| (p.client_id, p.x, p.y)).collect()))
}

#[tauri::command]
pub async fn host_test_switch(app: State<'_, Arc<App>>, client_id: String) -> Res<()> {
    app.with_host(|h| h.test_switch(&client_id))?
}

#[tauri::command]
pub async fn host_disconnect_client(app: State<'_, Arc<App>>, client_id: String) -> Res<()> {
    if app.with_host(|h| h.disconnect(&client_id))? { Ok(()) } else { Err("that computer is not connected".into()) }
}

#[tauri::command]
pub async fn refresh_monitors(app: State<'_, Arc<App>>) -> Res<()> {
    app.with_host(|h| h.refresh_monitors())?;
    app.notify();
    Ok(())
}

#[tauri::command]
pub async fn client_connect(app: State<'_, Arc<App>>, address: Option<String>, host_id: Option<String>) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || app.client_connect(address, host_id)).await
}

#[tauri::command]
pub async fn client_disconnect(app: State<'_, Arc<App>>) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || {
        app.client_disconnect();
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn discover_hosts(app: State<'_, Arc<App>>) -> Res<()> {
    app.discover();
    Ok(())
}

#[tauri::command]
pub async fn respond_pairing(app: State<'_, Arc<App>>, request_id: String, accept: bool) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || app.respond_pairing(&request_id, accept)).await
}

#[tauri::command]
pub async fn forget_device(app: State<'_, Arc<App>>, device_id: String) -> Res<()> {
    let app = app.inner().clone();
    blocking(move || app.forget_device(&device_id)).await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DevicePolicyArg {
    clipboard: bool,
    files: bool,
    text_share: bool,
}

/// Changes what a paired device may do (clipboard, files, shared text). Applies at once.
#[tauri::command]
pub async fn set_device_policy(app: State<'_, Arc<App>>, device_id: String, policy: DevicePolicyArg) -> Res<()> {
    let app = app.inner().clone();
    let p = moush_engine::core::features::DevicePolicy { clipboard: policy.clipboard, files: policy.files, text_share: policy.text_share };
    blocking(move || app.set_device_policy(&device_id, p)).await
}

/// Restores every setting to its default (keeps role, device name and the last host).
#[tauri::command]
pub async fn reset_settings(app: State<'_, Arc<App>>, handle: AppHandle) -> Res<Settings> {
    let app = app.inner().clone();
    let current = app.config().settings;
    let defaults = Settings {
        device_name: current.device_name,
        last_host_address: current.last_host_address,
        ..Settings::default()
    };
    let theme = defaults.theme.clone();
    let r = blocking(move || app.update_settings(defaults)).await?;
    crate::apply_theme(&handle, &theme);
    Ok(r)
}

#[tauri::command]
pub async fn get_diagnostics(app: State<'_, Arc<App>>) -> Res<Diagnostics> {
    let app = app.inner().clone();
    blocking(move || Ok(app.diagnostics())).await
}

#[tauri::command]
pub async fn get_logs(limit: Option<usize>) -> Res<Vec<LogEntry>> {
    Ok(moush_engine::logger::get().map(|l| l.recent(limit.unwrap_or(500).min(2000))).unwrap_or_default())
}

#[tauri::command]
pub async fn run_latency_test(app: State<'_, Arc<App>>) -> Res<LatencyTestResult> {
    let app = app.inner().clone();
    blocking(move || app.latency_test()).await
}

#[tauri::command]
pub async fn open_firewall_setup(app: State<'_, Arc<App>>) -> Res<()> {
    let exe = app.exe_path()?;
    blocking(move || system::add_firewall_rule(&exe).map_err(|e| e.to_string())).await
}

#[tauri::command]
pub async fn open_log_folder(app: State<'_, Arc<App>>) -> Res<()> {
    let dir = app.paths.log_dir.clone();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    system::open_folder(&dir).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn quit_app(handle: AppHandle) -> Res<()> {
    crate::quit(&handle);
    Ok(())
}
