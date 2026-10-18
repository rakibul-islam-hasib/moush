//! Calls back when the display configuration changes (monitor added, resolution or
//! scaling changed). Uses a hidden top-level window, because `WM_DISPLAYCHANGE` is only
//! broadcast to top-level windows. Fully event-driven: the thread sleeps in GetMessage.

use std::io;
use std::mem::size_of;
use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GWLP_USERDATA, GetMessageW, GetWindowLongPtrW,
    MSG, PostThreadMessageW, RegisterClassExW, SetWindowLongPtrW, WM_DISPLAYCHANGE, WM_DPICHANGED, WM_QUIT,
    WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_OVERLAPPED,
};
use windows::core::{PCWSTR, w};

use super::dpi::make_thread_dpi_aware;

type Callback = Box<dyn Fn() + Send + 'static>;

pub struct DisplayWatcher {
    thread_id: u32,
    join: Option<JoinHandle<()>>,
}

impl DisplayWatcher {
    pub fn start(on_change: impl Fn() + Send + 'static) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel();
        let cb: Callback = Box::new(on_change);
        let join = thread::Builder::new().name("display-watch".into()).spawn(move || {
            make_thread_dpi_aware();
            let hinst: HINSTANCE = unsafe { GetModuleHandleW(None).map(|h| h.into()).unwrap_or_default() };
            let class = w!("MoushDisplayWatch");
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc_),
                hInstance: hinst,
                lpszClassName: class,
                ..Default::default()
            };
            let hwnd = unsafe {
                RegisterClassExW(&wc);
                CreateWindowExW(
                    WS_EX_TOOLWINDOW,
                    class,
                    PCWSTR::null(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    0,
                    0,
                    None,
                    None,
                    Some(hinst),
                    None,
                )
            };
            let hwnd = match hwnd {
                Ok(h) => h,
                Err(e) => {
                    let _ = tx.send(Err(e.to_string()));
                    return;
                }
            };
            let cb = Box::into_raw(Box::new(cb));
            unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, cb as isize) };
            let _ = tx.send(Ok(unsafe { GetCurrentThreadId() }));
            let mut msg = MSG::default();
            while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 && msg.message != WM_QUIT {
                unsafe { DispatchMessageW(&msg) };
            }
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
                drop(Box::from_raw(cb));
            }
        })?;
        match rx.recv() {
            Ok(Ok(thread_id)) => Ok(Self { thread_id, join: Some(join) }),
            Ok(Err(e)) => Err(io::Error::other(e)),
            Err(_) => Err(io::Error::other("display watcher failed to start")),
        }
    }
}

impl Drop for DisplayWatcher {
    fn drop(&mut self) {
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_DISPLAYCHANGE || msg == WM_DPICHANGED {
        let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const Callback;
        if !p.is_null() {
            unsafe { (*p)() };
        }
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}
