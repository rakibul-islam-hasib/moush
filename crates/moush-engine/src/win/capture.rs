//! Host input capture: low-level mouse and keyboard hooks on a dedicated thread.
//!
//! `WH_MOUSE_LL` / `WH_KEYBOARD_LL` see every physical input event before any window
//! does, and can drop it. The hook callbacks run on this thread's message loop; they
//! hand each event to the [`Router`] (pure logic, microseconds) and act on its verdict.
//! Windows removes a low-level hook whose callback is too slow, so the callback never
//! blocks: network sends are non-blocking pushes into per-client queues.
//!
//! The thread also owns a tiny invisible topmost window used to hide the host cursor
//! (the cursor shape is decided by the window under it) and to receive display-change
//! and session-lock notifications.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Power::{ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState};
use windows::Win32::System::RemoteDesktop::{
    NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification, WTSUnRegisterSessionNotification,
};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThread, GetCurrentThreadId, SetThreadPriority, THREAD_PRIORITY_HIGHEST,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, GetLastInputInfo, LASTINPUTINFO};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateCursor, CreateWindowExW, DefWindowProcW, DestroyCursor, DestroyWindow, DispatchMessageW,
    GetCursorPos, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId, HCURSOR, HHOOK, HWND_TOPMOST, IsWindow,
    KBDLLHOOKSTRUCT, KillTimer, LLKHF_EXTENDED, LLKHF_INJECTED, LLKHF_UP, LLMHF_INJECTED, LWA_ALPHA, MA_NOACTIVATE,
    MSG, MSLLHOOKSTRUCT, PostThreadMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SetCursor,
    SetCursorPos, SetForegroundWindow, SetLayeredWindowAttributes, SetTimer, SetWindowPos, SetWindowsHookExW,
    ShowWindow, TranslateMessage, UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_APP, WM_DISPLAYCHANGE,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEHWHEEL,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETCURSOR, WM_SYSKEYDOWN, WM_SYSKEYUP,
    WM_TIMER, WM_WTSSESSION_CHANGE, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP, WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::{PCWSTR, w};

use moush_core::geometry::{Layout, Point};
use moush_core::inject::Injector;
use moush_core::net::{Outbox, Outgoing, try_send};
use moush_core::protocol::{MouseButton, WheelAxis};
use moush_core::router::{Action, RawInput, Router, RouterSettings, Verdict};
use moush_core::stats::Stats;
use moush_core::util::{mono_ms, mono_us};

use super::dpi::make_thread_dpi_aware;
use super::inject::SendInputInjector;

const WM_CAPTURE_CMD: u32 = WM_APP + 1;
/// Posted to ourselves to re-install the hooks outside of a hook callback.
const WM_REHOOK: u32 = WM_APP + 2;
/// `WM_REHOOK` parameter: the hooks were bypassed, so input state must be re-synchronised.
const REHOOK_AFTER_GAP: usize = 1;
const WATCHDOG_TIMER: usize = 1;
const WATCHDOG_MS: u32 = 1000;
/// Input Windows saw this much later than our hooks did means the hooks were bypassed.
const BYPASS_MS: u32 = 300;

/// Testing aid: when `MOUSH_TEST_ACCEPT_INJECTED` is set, synthetic input (SendInput
/// from another program) is routed like physical input, so the whole pipeline can be
/// exercised by a script. Our own injected events are still ignored.
static ACCEPT_INJECTED: AtomicBool = AtomicBool::new(false);

fn is_foreign(injected: bool, extra: usize) -> bool {
    injected && (!ACCEPT_INJECTED.load(Ordering::Relaxed) || extra == super::inject::INJECT_MARKER)
}
const HIDER_SIZE: i32 = 64;

pub enum CaptureCmd {
    SetLayout(Layout),
    SetSettings(RouterSettings),
    AddClient { id: String, outbox: Outbox },
    RemoveClient { id: String },
    SwitchTo(String),
    ForceLocal,
    Stop,
}

/// Notifications from the capture thread. Called on the hook thread: must not block.
pub trait CaptureEvents: Send + Sync + 'static {
    fn active_changed(&self, client: Option<String>);
    fn lock_changed(&self, locked: bool);
    fn displays_changed(&self);
    /// A client's queue is full or closed: it should be disconnected.
    fn send_failed(&self, client: &str);
}

pub struct Capture {
    thread_id: u32,
    tx: Sender<CaptureCmd>,
    installed: Arc<AtomicBool>,
    join: Option<JoinHandle<()>>,
}

impl Capture {
    /// Starts the capture thread and installs the hooks. Fails if Windows refuses the hooks.
    pub fn start(
        layout: Layout,
        settings: RouterSettings,
        stats: Arc<Stats>,
        events: Arc<dyn CaptureEvents>,
    ) -> io::Result<Self> {
        if std::env::var_os("MOUSH_TEST_ACCEPT_INJECTED").is_some() {
            log::warn!("test mode: synthetic input is routed like physical input");
            ACCEPT_INJECTED.store(true, Ordering::Relaxed);
        }
        let (tx, rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::channel::<Result<u32, String>>();
        let installed = Arc::new(AtomicBool::new(false));
        let installed2 = installed.clone();
        let join = thread::Builder::new()
            .name("input-capture".into())
            .spawn(move || capture_thread(layout, settings, stats, events, rx, ready_tx, installed2))?;
        match ready_rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self { thread_id, tx, installed, join: Some(join) }),
            Ok(Err(e)) => {
                let _ = join.join();
                Err(io::Error::other(e))
            }
            Err(_) => Err(io::Error::other("input capture thread exited during startup")),
        }
    }

    pub fn hooks_installed(&self) -> bool {
        self.installed.load(Ordering::SeqCst)
    }

    pub fn send(&self, cmd: CaptureCmd) {
        if self.tx.send(cmd).is_ok() {
            unsafe {
                let _ = PostThreadMessageW(self.thread_id, WM_CAPTURE_CMD, WPARAM(0), LPARAM(0));
            }
        }
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if let Some(j) = self.join.take() {
            self.send(CaptureCmd::Stop);
            let _ = j.join();
        }
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        self.shutdown();
    }
}

struct HookState {
    router: Router,
    outboxes: HashMap<String, Outbox>,
    actions: Vec<Action>,
    stats: Arc<Stats>,
    events: Arc<dyn CaptureEvents>,
    hider: HWND,
    keep_awake: bool,
    /// Our own window that had keyboard focus when a client became active.
    refocus: Option<HWND>,
}

thread_local! {
    static HOOKS: std::cell::Cell<Option<(HHOOK, HHOOK)>> = const { std::cell::Cell::new(None) };
    /// GetTickCount() of the last event our hooks received.
    static LAST_EVENT: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    static STATE: RefCell<Option<HookState>> = const { RefCell::new(None) };
    static BLANK_CURSOR: RefCell<Option<HCURSOR>> = const { RefCell::new(None) };
}

fn capture_thread(
    layout: Layout,
    settings: RouterSettings,
    stats: Arc<Stats>,
    events: Arc<dyn CaptureEvents>,
    rx: Receiver<CaptureCmd>,
    ready: Sender<Result<u32, String>>,
    installed: Arc<AtomicBool>,
) {
    make_thread_dpi_aware();
    unsafe {
        let _ = SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST);
    }
    let thread_id = unsafe { GetCurrentThreadId() };
    let hinst: HINSTANCE = unsafe { GetModuleHandleW(None).map(|h| h.into()).unwrap_or_default() };

    let hider = match create_hider(hinst) {
        Ok(h) => h,
        Err(e) => {
            let _ = ready.send(Err(format!("cannot create helper window: {e}")));
            return;
        }
    };
    unsafe {
        let _ = WTSRegisterSessionNotification(hider, NOTIFY_FOR_THIS_SESSION);
    }
    STATE.with(|s| {
        *s.borrow_mut() = Some(HookState {
            router: Router::new(layout, settings),
            outboxes: HashMap::new(),
            actions: Vec::with_capacity(16),
            stats,
            events,
            hider,
            keep_awake: false,
            refocus: None,
        })
    });

    if let Err(e) = install_hooks(hinst) {
        let _ = ready.send(Err(e));
        cleanup(hider);
        return;
    }
    LAST_EVENT.with(|t| t.set(unsafe { GetTickCount() }));
    unsafe {
        SetTimer(Some(hider), WATCHDOG_TIMER, WATCHDOG_MS, None);
    }
    installed.store(true, Ordering::SeqCst);
    let _ = ready.send(Ok(thread_id));
    log::info!("input hooks installed");

    let mut msg = MSG::default();
    'outer: loop {
        let r = unsafe { GetMessageW(&mut msg, None, 0, 0) };
        if r.0 <= 0 || msg.message == WM_QUIT {
            break;
        }
        if msg.hwnd.is_invalid() && msg.message == WM_REHOOK {
            if let Err(e) = install_hooks(hinst) {
                log::error!("re-installing input hooks failed: {e}");
            }
            if msg.wParam.0 == REHOOK_AFTER_GAP {
                // Events were missed while the hooks were bypassed: key releases among them
                // would leave keys stuck. Release what went to the client and forget host
                // keys that Windows reports as up.
                with_state(|s| {
                    s.router.resync_after_gap(|vk| unsafe { GetAsyncKeyState(vk as i32) } < 0, &mut s.actions);
                    s.execute(mono_us());
                });
            }
            continue;
        }
        if msg.hwnd.is_invalid() && msg.message == WM_CAPTURE_CMD {
            while let Ok(cmd) = rx.try_recv() {
                if matches!(cmd, CaptureCmd::Stop) {
                    break 'outer;
                }
                with_state(|s| s.command(cmd));
            }
            continue;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    // Leave the system clean: cursor home and visible, client released, hooks gone.
    with_state(|s| {
        s.router.force_local(&mut s.actions);
        s.execute(0);
        s.set_keep_awake(false);
    });
    unsafe {
        let _ = KillTimer(Some(hider), WATCHDOG_TIMER);
        if let Some((m, k)) = HOOKS.with(|h| h.take()) {
            let _ = UnhookWindowsHookEx(m);
            let _ = UnhookWindowsHookEx(k);
        }
        let _ = WTSUnRegisterSessionNotification(hider);
    }
    installed.store(false, Ordering::SeqCst);
    STATE.with(|s| s.borrow_mut().take());
    cleanup(hider);
    log::info!("input hooks removed");
}

/// Installs (or re-installs) both hooks. New hooks go to the front of the system hook
/// chain, ahead of hooks other programs installed since. The new pair is installed
/// before the old one is removed; no callback can run in between because callbacks are
/// only delivered while this thread waits for messages.
fn install_hooks(hinst: HINSTANCE) -> Result<(), String> {
    let (m, k) = unsafe {
        let m = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinst), 0);
        let k = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(hinst), 0);
        match (m, k) {
            (Ok(m), Ok(k)) => (m, k),
            (m, k) => {
                if let Ok(m) = m {
                    let _ = UnhookWindowsHookEx(m);
                }
                if let Ok(k) = k {
                    let _ = UnhookWindowsHookEx(k);
                }
                return Err("Windows refused to install the input hooks".to_owned());
            }
        }
    };
    if let Some((om, ok)) = HOOKS.with(|h| h.replace(Some((m, k)))) {
        unsafe {
            let _ = UnhookWindowsHookEx(om);
            let _ = UnhookWindowsHookEx(ok);
        }
    }
    Ok(())
}

fn request_rehook(after_gap: bool) {
    let param = if after_gap { REHOOK_AFTER_GAP } else { 0 };
    unsafe {
        let _ = PostThreadMessageW(GetCurrentThreadId(), WM_REHOOK, WPARAM(param), LPARAM(0));
    }
}

/// Runs once a second. Windows silently drops a low-level hook whose callback was ever
/// too slow, and a hook installed later by another program that does not pass events
/// on hides them from us. Both show up as input that Windows registered but our hooks
/// never saw; re-installing puts our hooks back at the front of the chain.
fn watchdog() {
    let mut info = LASTINPUTINFO { cbSize: size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    if !unsafe { GetLastInputInfo(&mut info) }.as_bool() {
        return;
    }
    let ours = LAST_EVENT.with(|t| t.get());
    let behind = info.dwTime.wrapping_sub(ours) as i32;
    if behind > BYPASS_MS as i32 {
        LAST_EVENT.with(|t| t.set(info.dwTime));
        // Keys typed into Moush's own (WebView2) window skip our hook by design of
        // WebView2; input stays local then, which is correct, and re-hooking cannot help.
        if owned_by_us(unsafe { GetForegroundWindow() }) {
            return;
        }
        log::warn!("input reached Windows without passing our hooks ({behind} ms); re-installing them");
        request_rehook(true);
    }
}

fn owned_by_us(w: HWND) -> bool {
    if w.is_invalid() {
        return false;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(w, Some(&mut pid)) };
    pid == unsafe { GetCurrentProcessId() }
}

fn note_event() {
    LAST_EVENT.with(|t| t.set(unsafe { GetTickCount() }));
}

fn cleanup(hider: HWND) {
    unsafe {
        let _ = DestroyWindow(hider);
        BLANK_CURSOR.with(|c| {
            if let Some(cur) = c.borrow_mut().take() {
                let _ = DestroyCursor(cur);
            }
        });
    }
}

/// Runs `f` on the hook state. Returns None if the state is unavailable (re-entrant
/// call or shutting down) so callers fall back to passing the event through.
fn with_state<R>(f: impl FnOnce(&mut HookState) -> R) -> Option<R> {
    STATE.with(|s| s.try_borrow_mut().ok().and_then(|mut g| g.as_mut().map(f)))
}

impl HookState {
    fn command(&mut self, cmd: CaptureCmd) {
        match cmd {
            CaptureCmd::SetLayout(l) => self.router.set_layout(l, &mut self.actions),
            CaptureCmd::SetSettings(s) => {
                let was_locked = self.router.locked();
                self.router.set_settings(s);
                if was_locked != self.router.locked() {
                    self.events.lock_changed(self.router.locked());
                }
            }
            CaptureCmd::AddClient { id, outbox } => {
                self.outboxes.insert(id, outbox);
            }
            CaptureCmd::RemoveClient { id } => {
                self.outboxes.remove(&id);
                self.router.client_gone(&id, &mut self.actions);
            }
            CaptureCmd::SwitchTo(id) => {
                let cur = cursor_pos();
                if !self.router.switch_to(&id, cur, &mut self.actions) {
                    log::warn!("cannot switch to {id}: not in the layout");
                }
            }
            CaptureCmd::ForceLocal => self.router.force_local(&mut self.actions),
            CaptureCmd::Stop => {}
        }
        self.execute(mono_us());
    }

    fn handle(&mut self, input: RawInput) -> Verdict {
        let t0 = mono_us();
        let cursor = if matches!(input, RawInput::MouseMove { .. }) { cursor_pos() } else { Point::default() };
        let v = self.router.handle(input, cursor, mono_ms(), &mut self.actions);
        if !self.actions.is_empty() {
            self.execute(t0);
        }
        Stats::add(&self.stats.events_captured, 1);
        Stats::record(&self.stats.hook_us, mono_us().saturating_sub(t0));
        v
    }

    fn execute(&mut self, t_us: u64) {
        let mut failed: Vec<String> = Vec::new();
        let mut actions = std::mem::take(&mut self.actions);
        for a in actions.drain(..) {
            match a {
                Action::Send { client, msg } => match self.outboxes.get(&client) {
                    Some(o) => {
                        if !try_send(o, Outgoing { msg, t_us }) && !failed.contains(&client) {
                            failed.push(client);
                        }
                    }
                    None => log::debug!("no queue for {client}"),
                },
                Action::WarpCursor(p) => unsafe {
                    let _ = SetCursorPos(p.x, p.y);
                },
                Action::HideCursor(p) => unsafe {
                    let _ = SetWindowPos(
                        self.hider,
                        Some(HWND_TOPMOST),
                        p.x - HIDER_SIZE / 2,
                        p.y - HIDER_SIZE / 2,
                        HIDER_SIZE,
                        HIDER_SIZE,
                        SWP_NOACTIVATE,
                    );
                    let _ = ShowWindow(self.hider, SW_SHOWNOACTIVATE);
                    // While a WebView2 window of this process has keyboard focus, Windows
                    // does not call this process's keyboard hook, so keys would be typed
                    // into Moush instead of reaching the client. Move focus to the
                    // plain helper window (stray keys are dropped there) and give it back
                    // when the cursor returns. Other programs' focus is never touched.
                    // In mouse-only mode typing stays on the host, so focus is left alone.
                    let fg = GetForegroundWindow();
                    if self.router.settings().forward_keyboard
                        && owned_by_us(fg)
                        && fg != self.hider
                        && SetForegroundWindow(self.hider).as_bool()
                    {
                        self.refocus = Some(fg);
                    }
                },
                Action::ShowCursor => unsafe {
                    let _ = ShowWindow(self.hider, SW_HIDE);
                    if let Some(w) = self.refocus.take()
                        && IsWindow(Some(w)).as_bool()
                    {
                        let _ = SetForegroundWindow(w);
                    }
                },
                Action::KeepAwake(on) => self.set_keep_awake(on),
                Action::ActiveChanged(c) => {
                    if c.is_some() {
                        request_rehook(false);
                    }
                    match &c {
                        Some(id) => log::info!("input now goes to {id}"),
                        None => log::info!("input back on this computer"),
                    }
                    self.events.active_changed(c);
                }
                Action::LockChanged(l) => {
                    log::info!("cursor {}", if l { "locked to the current computer" } else { "unlocked" });
                    self.events.lock_changed(l);
                }
                Action::LocalKey { vk, scan, extended, down } => {
                    // Injected events are queued behind the event being processed and come
                    // back through our hooks tagged with INJECT_MARKER, which passes them on.
                    // Fails (UIPI) if the foreground window is elevated and Moush is not.
                    if !SendInputInjector.key(vk, scan, extended, down, true) {
                        log::debug!("could not inject key 0x{vk:02x} on this computer");
                    }
                }
            }
        }
        self.actions = actions; // keep the allocation
        for c in failed {
            self.outboxes.remove(&c);
            self.events.send_failed(&c);
        }
    }

    fn set_keep_awake(&mut self, on: bool) {
        if on == self.keep_awake {
            return;
        }
        self.keep_awake = on;
        unsafe {
            if on {
                SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED);
            } else {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        }
    }
}

fn cursor_pos() -> Point {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    Point::new(p.x, p.y)
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        note_event();
        let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        if !is_foreign(info.flags & LLMHF_INJECTED != 0, info.dwExtraInfo) {
            let hi = (info.mouseData >> 16) as u16;
            let input = match wparam.0 as u32 {
                WM_MOUSEMOVE => Some(RawInput::MouseMove { x: info.pt.x, y: info.pt.y }),
                WM_LBUTTONDOWN => Some(RawInput::MouseButton { button: MouseButton::Left, down: true }),
                WM_LBUTTONUP => Some(RawInput::MouseButton { button: MouseButton::Left, down: false }),
                WM_RBUTTONDOWN => Some(RawInput::MouseButton { button: MouseButton::Right, down: true }),
                WM_RBUTTONUP => Some(RawInput::MouseButton { button: MouseButton::Right, down: false }),
                WM_MBUTTONDOWN => Some(RawInput::MouseButton { button: MouseButton::Middle, down: true }),
                WM_MBUTTONUP => Some(RawInput::MouseButton { button: MouseButton::Middle, down: false }),
                WM_XBUTTONDOWN | WM_XBUTTONUP => {
                    let button = if hi == 2 { MouseButton::X2 } else { MouseButton::X1 };
                    Some(RawInput::MouseButton { button, down: wparam.0 as u32 == WM_XBUTTONDOWN })
                }
                WM_MOUSEWHEEL => Some(RawInput::Wheel { axis: WheelAxis::Vertical, delta: hi as i16 }),
                WM_MOUSEHWHEEL => Some(RawInput::Wheel { axis: WheelAxis::Horizontal, delta: hi as i16 }),
                _ => None,
            };
            if let Some(input) = input
                && route(input) == Verdict::Swallow
            {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        note_event();
        let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let msg = wparam.0 as u32;
        if is_foreign(info.flags & LLKHF_INJECTED == LLKHF_INJECTED, info.dwExtraInfo)
            || !(1..=0xFE).contains(&info.vkCode)
        {
            return unsafe { CallNextHookEx(None, code, wparam, lparam) };
        }
        let down = matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN) && info.flags & LLKHF_UP != LLKHF_UP;
        if matches!(msg, WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP) {
            let input = RawInput::Key {
                vk: info.vkCode as u16,
                scan: info.scanCode as u16,
                extended: info.flags & LLKHF_EXTENDED == LLKHF_EXTENDED,
                down,
            };
            if route(input) == Verdict::Swallow {
                return LRESULT(1);
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Routes an event; any failure (re-entrancy, panic) lets the event through so the
/// user never loses control of the host.
fn route(input: RawInput) -> Verdict {
    std::panic::catch_unwind(|| with_state(|s| s.handle(input)).unwrap_or(Verdict::Pass)).unwrap_or(Verdict::Pass)
}

fn create_hider(hinst: HINSTANCE) -> windows::core::Result<HWND> {
    let class = w!("MoushCursorHider");
    // Fully transparent 32x32 cursor: AND mask all ones, XOR mask all zeros.
    let and_mask = [0xFFu8; 128];
    let xor_mask = [0u8; 128];
    let blank = unsafe { CreateCursor(Some(hinst), 0, 0, 32, 32, and_mask.as_ptr().cast(), xor_mask.as_ptr().cast())? };
    BLANK_CURSOR.with(|c| *c.borrow_mut() = Some(blank));
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        lpfnWndProc: Some(hider_proc),
        hInstance: hinst,
        hCursor: blank,
        lpszClassName: class,
        ..Default::default()
    };
    unsafe {
        RegisterClassExW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_LAYERED,
            class,
            PCWSTR::null(),
            WS_POPUP,
            0,
            0,
            HIDER_SIZE,
            HIDER_SIZE,
            None,
            None,
            Some(hinst),
            None,
        )?;
        // Alpha 1 instead of 0: a fully transparent layered window is not hit-tested,
        // and the window must be under the cursor to control its shape.
        SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA)?;
        Ok(hwnd)
    }
}

use std::mem::size_of;

unsafe extern "system" fn hider_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_SETCURSOR => {
            BLANK_CURSOR.with(|c| {
                if let Some(cur) = *c.borrow() {
                    unsafe { SetCursor(Some(cur)) };
                }
            });
            LRESULT(1)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_TIMER if wparam.0 == WATCHDOG_TIMER => {
            watchdog();
            LRESULT(0)
        }
        WM_DISPLAYCHANGE => {
            with_state(|s| s.events.displays_changed());
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        WM_WTSSESSION_CHANGE => {
            let event = wparam.0 as u32;
            if event == WTS_SESSION_LOCK || event == WTS_SESSION_UNLOCK {
                // The secure desktop takes over input: bring the cursor home, and forget held
                // keys, whose releases (e.g. of Win+L) go to the secure desktop, not to us.
                with_state(|s| {
                    s.router.session_changed(&mut s.actions);
                    s.execute(mono_us());
                });
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::sync_channel;
    use std::time::{Duration, Instant};

    use moush_core::geometry::{ClientPlacement, Rect, Screen};
    use moush_core::keys::{self, ShortcutSet};
    use moush_core::protocol::{Message, key_flags};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY,
    };

    const VK_F23: u16 = 0x86;

    struct NoEvents;
    impl CaptureEvents for NoEvents {
        fn active_changed(&self, _: Option<String>) {}
        fn lock_changed(&self, _: bool) {}
        fn displays_changed(&self) {}
        fn send_failed(&self, _: &str) {}
    }

    /// Synthetic key without our marker: with ACCEPT_INJECTED the hooks route it like a
    /// physical key.
    fn tap(vk: u16, down: bool) {
        let input = INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk),
                    wScan: 0,
                    dwFlags: if down { KEYBD_EVENT_FLAGS(0) } else { KEYEVENTF_KEYUP },
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        };
        assert_eq!(unsafe { SendInput(&[input], size_of::<INPUT>() as i32) }, 1, "SendInput refused");
        thread::sleep(Duration::from_millis(30));
    }

    fn host_down(vk: u16) -> bool {
        unsafe { GetAsyncKeyState(vk as i32) < 0 }
    }

    /// Everything the fake client received in the last moments.
    fn drain(rx: &Receiver<Outgoing>) -> Vec<Message> {
        let deadline = Instant::now() + Duration::from_millis(150);
        let mut v = Vec::new();
        while let Ok(o) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            v.push(o.msg);
        }
        v
    }

    fn keys_of(msgs: &[Message]) -> Vec<(u16, bool)> {
        msgs.iter()
            .filter_map(|m| match m {
                Message::Key { vk, flags, .. } => Some((*vk, flags & key_flags::DOWN != 0)),
                _ => None,
            })
            .collect()
    }

    /// Drives the real low-level hooks, router and action executor with harmless synthetic
    /// keys (F23/F24, Ctrl, Shift, Ctrl+Alt+Shift+Home) and a fake client queue. Needs an
    /// unlocked interactive session; run by hand:
    /// `cargo test -p moush-engine real_hooks -- --ignored --nocapture --test-threads=1`
    #[test]
    #[ignore = "drives the real hooks with synthetic input on the interactive desktop"]
    fn real_hooks_route_keys_without_stuck_keys() {
        ACCEPT_INJECTED.store(true, Ordering::Relaxed);
        let mut mons: Vec<Rect> = super::super::monitors::enumerate().iter().map(|m| m.rect).collect();
        if mons.is_empty() {
            mons.push(Rect::new(0, 0, 1920, 1080));
        }
        let layout = Layout {
            host: Screen::new(mons),
            clients: vec![ClientPlacement {
                id: "c".into(),
                screen: Screen::new(vec![Rect::new(0, 0, 1920, 1080)]),
                origin_x: -100_000.0, // far away: never reached through an edge
                origin_y: 0.0,
                scale: 1.0,
            }],
        };
        let settings =
            RouterSettings { local_shortcuts: ShortcutSet::parse(&["Ctrl+Shift+F23"]).0, ..RouterSettings::default() };
        let capture = Capture::start(layout, settings, Arc::new(Stats::new()), Arc::new(NoEvents)).expect("hooks");
        let (tx, rx) = sync_channel(256);
        capture.send(CaptureCmd::AddClient { id: "c".into(), outbox: tx });
        let original = cursor_pos();
        capture.send(CaptureCmd::SwitchTo("c".into()));
        let enter = drain(&rx);
        assert!(matches!(enter.first(), Some(Message::Enter { .. })), "switch: {enter:?}");

        // 1. Plain key: forwarded, swallowed on the host.
        tap(keys::VK_F24, true);
        let host_saw_f24 = host_down(keys::VK_F24);
        tap(keys::VK_F24, false);
        assert_eq!(keys_of(&drain(&rx)), vec![(keys::VK_F24, true), (keys::VK_F24, false)]);
        assert!(!host_saw_f24, "F24 leaked to the host");

        // 2. Local shortcut typed while the client is active: modifiers move to the host.
        tap(keys::VK_LCONTROL, true);
        tap(keys::VK_LSHIFT, true);
        assert!(!host_down(keys::VK_LCONTROL) && !host_down(keys::VK_LSHIFT), "modifiers leaked to the host");
        tap(VK_F23, true);
        thread::sleep(Duration::from_millis(50));
        let host_has_combo = host_down(keys::VK_LCONTROL) && host_down(keys::VK_LSHIFT) && host_down(VK_F23);
        tap(VK_F23, false);
        tap(keys::VK_LSHIFT, false);
        tap(keys::VK_LCONTROL, false);
        assert_eq!(
            keys_of(&drain(&rx)),
            vec![
                (keys::VK_LCONTROL, true),
                (keys::VK_LSHIFT, true),
                (keys::VK_LSHIFT, false),
                (keys::VK_LCONTROL, false)
            ]
        );
        assert!(host_has_combo, "Ctrl+Shift+F23 was not re-pressed on the host");
        for vk in [keys::VK_LCONTROL, keys::VK_LSHIFT, VK_F23] {
            assert!(!host_down(vk), "vk {vk:#x} stuck on the host");
        }

        // 3. Switching hotkey back to the host: consumed, nothing stuck anywhere.
        for vk in [keys::VK_LCONTROL, keys::VK_LMENU, keys::VK_LSHIFT] {
            tap(vk, true);
        }
        tap(keys::VK_HOME, true);
        tap(keys::VK_HOME, false);
        for vk in [keys::VK_LSHIFT, keys::VK_LMENU, keys::VK_LCONTROL] {
            tap(vk, false);
        }
        let msgs = drain(&rx);
        assert!(msgs.contains(&Message::Leave), "hotkey did not return home: {msgs:?}");
        assert!(!keys_of(&msgs).iter().any(|(vk, _)| *vk == keys::VK_HOME), "Home leaked to the client");
        for vk in [keys::VK_LCONTROL, keys::VK_LMENU, keys::VK_LSHIFT, keys::VK_HOME] {
            assert!(!host_down(vk), "vk {vk:#x} stuck on the host");
        }

        capture.stop();
        unsafe {
            let _ = SetCursorPos(original.x, original.y);
        }
        ACCEPT_INJECTED.store(false, Ordering::Relaxed);
    }
}
