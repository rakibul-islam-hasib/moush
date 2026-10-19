//! Client option "focus on enter": when the shared cursor enters this computer, the
//! top-level window under it comes to the front, so typing goes where the user points.
//!
//! Windows only lets a process take the foreground under the foreground-lock rules
//! (`SetForegroundWindow` documentation); having injected input with `SendInput` is not
//! one of the documented conditions, so Moush does not rely on it. Instead it attaches to
//! the foreground thread's input state (`AttachThreadInput`), which shares that thread's
//! activation rights, activates the window, and detaches. That can wait on a hung
//! foreground thread, so it runs on its own worker thread, never on the thread that
//! injects input.
//!
//! Focus is never taken from a secure desktop (lock screen, UAC prompt: there is no
//! foreground window then) or from a full-screen window, and the desktop and taskbar are
//! never activated (that would just steal focus from the window the user was in).

use std::io;
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::thread;
use std::time::Duration;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
use windows::Win32::UI::WindowsAndMessaging::{
    BringWindowToTop, GA_ROOT, GWL_EXSTYLE, GWL_STYLE, GetAncestor, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetWindowLongW, GetWindowRect, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow, WS_DISABLED,
    WS_EX_NOACTIVATE, WS_EX_TRANSPARENT, WindowFromPoint,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FocusOutcome {
    Focused,
    AlreadyInFront,
    Skipped(&'static str),
    /// Windows refused the activation (foreground lock).
    Refused,
}

/// Brings the top-level window under the cursor to the foreground, within the rules above.
pub fn focus_window_under_cursor() -> FocusOutcome {
    let mut p = POINT::default();
    if unsafe { GetCursorPos(&mut p) }.is_err() {
        return FocusOutcome::Skipped("cursor position unavailable");
    }
    let hit = unsafe { WindowFromPoint(p) };
    if hit.is_invalid() {
        return FocusOutcome::Skipped("no window under the cursor");
    }
    focus_window(unsafe { GetAncestor(hit, GA_ROOT) })
}

/// Activates `target` (a top-level window) unless that would be inappropriate.
pub fn focus_window(target: HWND) -> FocusOutcome {
    if target.is_invalid() {
        return FocusOutcome::Skipped("no window under the cursor");
    }
    let fg = unsafe { GetForegroundWindow() };
    if fg.is_invalid() {
        return FocusOutcome::Skipped("no foreground window (lock screen or secure desktop)");
    }
    if fg == target {
        return FocusOutcome::AlreadyInFront;
    }
    if is_shell_window(target) {
        return FocusOutcome::Skipped("desktop or taskbar");
    }
    let style = unsafe { GetWindowLongW(target, GWL_STYLE) } as u32;
    let ex = unsafe { GetWindowLongW(target, GWL_EXSTYLE) } as u32;
    if !unsafe { IsWindowVisible(target) }.as_bool()
        || style & WS_DISABLED.0 != 0
        || ex & (WS_EX_NOACTIVATE.0 | WS_EX_TRANSPARENT.0) != 0
    {
        return FocusOutcome::Skipped("window cannot be activated");
    }
    if !is_shell_window(fg) && is_full_screen(fg) {
        return FocusOutcome::Skipped("a full-screen window is in front");
    }
    unsafe {
        let me = GetCurrentThreadId();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let attached = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = BringWindowToTop(target);
        let _ = SetForegroundWindow(target);
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
    }
    if unsafe { GetForegroundWindow() } == target { FocusOutcome::Focused } else { FocusOutcome::Refused }
}

fn class_name(w: HWND) -> String {
    let mut buf = [0u16; 64];
    let n = unsafe { GetClassNameW(w, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n])
}

/// The desktop (Progman/WorkerW) and the taskbars.
fn is_shell_window(w: HWND) -> bool {
    matches!(class_name(w).as_str(), "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd")
}

/// True when `w` covers its whole monitor (games, videos, presentations).
fn is_full_screen(w: HWND) -> bool {
    let mut r = RECT::default();
    if unsafe { GetWindowRect(w, &mut r) }.is_err() {
        return false;
    }
    let mon = unsafe { MonitorFromWindow(w, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(mon, &mut info) }.as_bool() {
        return false;
    }
    let m = info.rcMonitor;
    r.left <= m.left && r.top <= m.top && r.right >= m.right && r.bottom >= m.bottom
}

/// Runs focus requests on a dedicated thread. Requests made while one is pending are
/// merged; the thread ends when the worker is dropped.
pub struct FocusWorker {
    tx: SyncSender<()>,
}

impl FocusWorker {
    pub fn start() -> io::Result<Self> {
        let (tx, rx) = mpsc::sync_channel::<()>(1);
        thread::Builder::new().name("focus".into()).spawn(move || run(rx))?;
        Ok(Self { tx })
    }

    /// Never blocks.
    pub fn request(&self) {
        match self.tx.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => {}
            Err(TrySendError::Disconnected(())) => log::debug!("focus worker gone"),
        }
    }
}

fn run(rx: Receiver<()>) {
    while rx.recv().is_ok() {
        // Let Windows process the injected entry move before asking what is under it.
        thread::sleep(Duration::from_millis(15));
        let outcome = focus_window_under_cursor();
        log::debug!("focus on enter: {outcome:?}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, MSG, PostThreadMessageW,
        RegisterClassExW, SW_SHOWNOACTIVATE, ShowWindow, WINDOW_EX_STYLE, WM_QUIT, WNDCLASSEXW, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    unsafe extern "system" fn proc_(h: HWND, m: u32, w: WPARAM, l: LPARAM) -> LRESULT {
        unsafe { DefWindowProcW(h, m, w, l) }
    }

    /// Run by hand (`cargo test -p moush-engine focus -- --ignored`): it briefly takes the
    /// foreground from whatever window has it, then gives it back. Verifies that a
    /// background process can activate another window with `focus_window`.
    #[test]
    #[ignore = "interactive: briefly changes the foreground window"]
    fn background_process_can_activate_a_window() {
        let previous = unsafe { GetForegroundWindow() };
        let (tx, rx) = mpsc::channel();
        let ui = thread::spawn(move || unsafe {
            let hinst = GetModuleHandleW(None).unwrap();
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: hinst.into(),
                lpszClassName: w!("MoushFocusTest"),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            let hwnd = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("MoushFocusTest"),
                w!("Moush focus test"),
                WS_OVERLAPPEDWINDOW,
                100,
                100,
                300,
                200,
                None,
                None,
                Some(hinst.into()),
                None,
            )
            .unwrap();
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            tx.send((hwnd.0 as isize, GetCurrentThreadId())).unwrap();
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                DispatchMessageW(&msg);
            }
            let _ = DestroyWindow(hwnd);
        });
        let (raw, ui_thread) = rx.recv().unwrap();
        let target = HWND(raw as *mut _);
        thread::sleep(Duration::from_millis(200));
        let before = unsafe { GetForegroundWindow() };
        let outcome = focus_window(target);
        thread::sleep(Duration::from_millis(100));
        let after = unsafe { GetForegroundWindow() };
        // Give the foreground back to where it was.
        let restored = if previous.is_invalid() { FocusOutcome::Skipped("none") } else { focus_window(previous) };
        unsafe {
            let _ = PostThreadMessageW(ui_thread, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        ui.join().unwrap();
        println!(
            "foreground before {before:?}, target {target:?}: {outcome:?}, after {after:?}; restore: {restored:?}"
        );
        assert_ne!(before, target, "the test window must start in the background");
        assert_eq!(outcome, FocusOutcome::Focused);
        assert_eq!(after, target);
    }
}
