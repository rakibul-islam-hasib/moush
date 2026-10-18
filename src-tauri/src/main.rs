// No console window in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod commands;
mod dto;
mod settings;
mod tray;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Manager, RunEvent, Theme, WindowEvent};

use moush_engine::paths::AppPaths;

use crate::app::App;

static QUITTING: AtomicBool = AtomicBool::new(false);

fn main() {
    // Physical-pixel coordinates everywhere (monitor layout, cursor positions).
    moush_engine::win::dpi::make_process_dpi_aware();
    let paths = AppPaths::from_env();
    let level = if cfg!(debug_assertions) { log::LevelFilter::Debug } else { log::LevelFilter::Info };
    let logger = moush_engine::logger::init(Some(&paths.log_file()), level, cfg!(debug_assertions));
    log::info!("Moush {} starting", env!("CARGO_PKG_VERSION"));
    let launched_minimized = std::env::args().any(|a| a == "--autostart" || a == "--minimized");

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| show_main_window(app)))
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::set_role,
            commands::get_settings,
            commands::update_settings,
            commands::host_start,
            commands::host_stop,
            commands::host_set_layout,
            commands::host_test_switch,
            commands::host_disconnect_client,
            commands::refresh_monitors,
            commands::client_connect,
            commands::client_disconnect,
            commands::discover_hosts,
            commands::respond_pairing,
            commands::forget_device,
            commands::get_diagnostics,
            commands::get_logs,
            commands::run_latency_test,
            commands::open_firewall_setup,
            commands::open_log_folder,
            commands::quit_app,
        ])
        .setup(move |tauri_app| {
            let handle = tauri_app.handle().clone();
            let app = App::new(handle.clone(), paths).map_err(|e| {
                log::error!("startup failed: {e}");
                e
            })?;
            tauri_app.manage(app.clone());

            let handle2 = handle.clone();
            logger.set_listener(move |entry| {
                use tauri::Emitter;
                let _ = handle2.emit("log", entry);
            });

            let start_hidden = launched_minimized || app.config().settings.start_minimized;
            if !start_hidden {
                create_main_window(&handle);
            }
            tray::create(&handle)?;
            app.autostart();
            app.notify();
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let close_to_tray = window
                    .app_handle()
                    .try_state::<Arc<App>>()
                    .map(|a| a.config().settings.close_to_tray)
                    .unwrap_or(false);
                // Closing to the tray destroys the window and its WebView2 processes
                // (several hundred MB); the engine keeps running and the window is
                // recreated on demand. Exit is prevented in `RunEvent::ExitRequested`.
                if !close_to_tray || QUITTING.load(Ordering::SeqCst) {
                    api.prevent_close();
                    quit(window.app_handle());
                }
            }
        })
        .build(tauri::generate_context!());

    let app = match result {
        Ok(a) => a,
        Err(e) => {
            log::error!("failed to start the window: {e}");
            show_fatal(&format!("Moush could not start:\n\n{e}"));
            std::process::exit(1);
        }
    };
    app.run(|handle, event| match event {
        RunEvent::ExitRequested { api, code, .. } => {
            // Keep running in the tray when the last window closes, unless quitting.
            if code.is_none() && !QUITTING.load(Ordering::SeqCst) {
                api.prevent_exit();
            }
        }
        RunEvent::Exit => shutdown(handle),
        _ => {}
    });
}

/// Shows the main window, creating it if it was closed to the tray. Safe from any thread.
pub fn show_main_window(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || match handle.get_webview_window("main") {
        Some(w) => {
            let _ = w.unminimize();
            let _ = w.show();
            let _ = w.set_focus();
        }
        None => create_main_window(&handle),
    });
}

/// Builds the main window from its configuration (Mica on Windows 11, saved theme).
fn create_main_window(app: &AppHandle) {
    let Some(cfg) = app.config().app.windows.iter().find(|w| w.label == "main").cloned() else { return };
    let window = match tauri::WebviewWindowBuilder::from_config(app, &cfg).and_then(|b| b.build()) {
        Ok(w) => w,
        Err(e) => {
            log::error!("cannot open the window: {e}");
            return;
        }
    };
    if let Some(state) = app.try_state::<Arc<App>>() {
        // Native Mica backdrop on Windows 11; Windows 10 keeps an opaque window.
        if state.platform.lock().unwrap().windows11 {
            let ok = window.set_effects(EffectsBuilder::new().effect(Effect::Mica).build()).is_ok();
            state.platform.lock().unwrap().mica = ok;
        }
        apply_theme(app, &state.config().settings.theme);
    }
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn apply_theme(app: &AppHandle, theme: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let t = match theme {
            "light" => Some(Theme::Light),
            "dark" => Some(Theme::Dark),
            _ => None,
        };
        let _ = w.set_theme(t);
    }
}

/// Stops services cleanly (cursor home, keys released, connections closed) and exits.
pub fn quit(app: &AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    shutdown(app);
    app.exit(0);
}

fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<Arc<App>>() {
        state.stop_runtime();
    }
    log::logger().flush();
}

fn show_fatal(msg: &str) {
    use std::os::windows::ffi::OsStrExt;
    let text: Vec<u16> = std::ffi::OsStr::new(msg).encode_wide().chain(Some(0)).collect();
    let title: Vec<u16> = std::ffi::OsStr::new("Moush").encode_wide().chain(Some(0)).collect();
    unsafe {
        windows_sys_message_box(text.as_ptr(), title.as_ptr());
    }
}

#[link(name = "user32")]
unsafe extern "system" {
    #[link_name = "MessageBoxW"]
    fn message_box_w(hwnd: isize, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

unsafe fn windows_sys_message_box(text: *const u16, title: *const u16) {
    const MB_ICONERROR: u32 = 0x10;
    unsafe { message_box_w(0, text, title, MB_ICONERROR) };
}
