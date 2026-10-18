//! Win32 clipboard access for clipboard sync.
//!
//! A dedicated thread owns a message-only window registered with
//! `AddClipboardFormatListener`, so changes arrive as `WM_CLIPBOARDUPDATE` (no polling).
//! Reads and writes are requests executed on that thread; `OpenClipboard` is retried with
//! a short backoff while another application holds the clipboard.
//!
//! Formats: CF_UNICODETEXT, "HTML Format", "Rich Text Format", the registered "PNG"
//! format and CF_DIB (Windows synthesizes CF_DIB from CF_BITMAP/CF_DIBV5 and the other
//! bitmap formats from CF_DIB on the receiving side). Images travel as the bitmap, which
//! every application can paste, plus the source's PNG when it offered one (keeps
//! transparency for applications that read PNG); payloads are compressed with
//! [`compress`] (Xpress Huffman from the Windows Compression API, no extra dependency).
//!
//! Content marked by its source as not for clipboard monitors or clipboard history
//! (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory = 0`,
//! `CanUploadToCloudClipboard = 0`; set by password managers) is reported as sensitive
//! without reading it.

use std::io;
use std::mem::size_of;
use std::sync::mpsc::{self, Receiver, Sender, SyncSender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Storage::Compression::{
    CloseCompressor, CloseDecompressor, Compress, CreateCompressor, CreateDecompressor, Decompress, COMPRESSOR_HANDLE,
    COMPRESS_ALGORITHM_XPRESS_HUFF, DECOMPRESSOR_HANDLE,
};
use windows::Win32::System::DataExchange::{
    AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
    IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW, RemoveClipboardFormatListener, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowLongPtrW, PostMessageW,
    RegisterClassExW, SetWindowLongPtrW, GWLP_USERDATA, HWND_MESSAGE, MSG, SW_SHOWNORMAL, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_APP, WM_CLIPBOARDUPDATE, WNDCLASSEXW,
};

use moush_core::clipboard::{as_link, dib_len, png_len, Clip};

use crate::clipboard::{ChangeCallback, ClipboardBackend, LocalRead, ReadOptions};

const CF_DIB: u32 = 8;
const CF_UNICODETEXT: u32 = 13;
/// Posted to the window when requests are queued.
const WM_REQUEST: u32 = WM_APP + 1;
/// Backoff between attempts to open a clipboard that another application holds.
const OPEN_RETRY_MS: [u64; 8] = [0, 5, 10, 20, 40, 80, 120, 200];
/// How long a caller waits for the clipboard thread.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

enum Request {
    Listen(bool),
    Read(ReadOptions, SyncSender<Result<LocalRead, String>>),
    Write(Arc<Clip>, SyncSender<Result<u32, String>>),
    Quit,
}

/// The clipboard thread and its message-only window.
pub struct WinClipboard {
    /// The window handle as an integer (`HWND` is not `Send`).
    hwnd: isize,
    tx: std::sync::Mutex<Sender<Request>>,
    join: Option<JoinHandle<()>>,
}

impl WinClipboard {
    /// Starts the clipboard thread. `on_change` runs on that thread after every clipboard
    /// change while listening (see [`ClipboardBackend::set_listening`]); it must not block.
    pub fn start(on_change: ChangeCallback) -> io::Result<Self> {
        let (tx, rx) = mpsc::channel::<Request>();
        let (ready_tx, ready_rx) = mpsc::channel();
        let join = thread::Builder::new().name("clipboard-win32".into()).spawn(move || run(on_change, rx, ready_tx))?;
        match ready_rx.recv() {
            Ok(Ok(hwnd)) => Ok(Self { hwnd, tx: std::sync::Mutex::new(tx), join: Some(join) }),
            Ok(Err(e)) => Err(io::Error::other(e)),
            Err(_) => Err(io::Error::other("clipboard thread failed to start")),
        }
    }

    fn hwnd(&self) -> HWND {
        HWND(self.hwnd as *mut _)
    }

    fn post(&self, r: Request) -> bool {
        if self.tx.lock().unwrap().send(r).is_err() {
            return false;
        }
        unsafe { PostMessageW(Some(self.hwnd()), WM_REQUEST, WPARAM(0), LPARAM(0)) }.is_ok()
    }

    fn call<T>(&self, make: impl FnOnce(SyncSender<Result<T, String>>) -> Request) -> Result<T, String> {
        let (tx, rx) = mpsc::sync_channel(1);
        if !self.post(make(tx)) {
            return Err("the clipboard thread is not running".into());
        }
        rx.recv_timeout(REQUEST_TIMEOUT).map_err(|_| "the clipboard thread did not respond".to_string())?
    }
}

impl ClipboardBackend for WinClipboard {
    fn set_listening(&self, on: bool) {
        self.post(Request::Listen(on));
    }

    fn sequence(&self) -> u32 {
        unsafe { GetClipboardSequenceNumber() }
    }

    fn read(&self, opts: &ReadOptions) -> Result<LocalRead, String> {
        let opts = *opts;
        self.call(|tx| Request::Read(opts, tx))
    }

    fn write(&self, clip: Arc<Clip>) -> Result<u32, String> {
        self.call(|tx| Request::Write(clip, tx))
    }
}

impl Drop for WinClipboard {
    fn drop(&mut self) {
        self.post(Request::Quit);
        if let Some(j) = self.join.take() {
            let _ = j.join();
        }
    }
}

/// Registered clipboard format ids.
struct Formats {
    html: u32,
    rtf: u32,
    png: u32,
    exclude: u32,
    can_history: u32,
    can_cloud: u32,
}

impl Formats {
    fn register() -> Self {
        unsafe {
            Self {
                html: RegisterClipboardFormatW(w!("HTML Format")),
                rtf: RegisterClipboardFormatW(w!("Rich Text Format")),
                png: RegisterClipboardFormatW(w!("PNG")),
                exclude: RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")),
                can_history: RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory")),
                can_cloud: RegisterClipboardFormatW(w!("CanUploadToCloudClipboard")),
            }
        }
    }
}

fn run(on_change: ChangeCallback, rx: Receiver<Request>, ready: Sender<Result<isize, String>>) {
    let hinst: HINSTANCE = unsafe { GetModuleHandleW(None).map(|h| h.into()).unwrap_or_default() };
    let class = w!("MoushClipboardWindow");
    let wc = WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(proc_), hInstance: hinst, lpszClassName: class, ..Default::default() };
    let hwnd = unsafe {
        RegisterClassExW(&wc);
        CreateWindowExW(WINDOW_EX_STYLE(0), class, PCWSTR::null(), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, Some(hinst), None)
    };
    let hwnd = match hwnd {
        Ok(h) => h,
        Err(e) => {
            let _ = ready.send(Err(format!("cannot create the clipboard window: {e}")));
            return;
        }
    };
    let cb = Box::into_raw(Box::new(on_change));
    unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, cb as isize) };
    let formats = Formats::register();
    let _ = ready.send(Ok(hwnd.0 as isize));
    let mut listening = false;
    let mut msg = MSG::default();
    'outer: while unsafe { GetMessageW(&mut msg, None, 0, 0) }.0 > 0 {
        if msg.message != WM_REQUEST {
            unsafe { DispatchMessageW(&msg) };
            continue;
        }
        while let Ok(r) = rx.try_recv() {
            match r {
                Request::Listen(on) if on != listening => {
                    let r = unsafe { if on { AddClipboardFormatListener(hwnd) } else { RemoveClipboardFormatListener(hwnd) } };
                    match r {
                        Ok(()) => listening = on,
                        Err(e) => log::warn!("cannot {} clipboard changes: {e}", if on { "watch" } else { "stop watching" }),
                    }
                }
                Request::Listen(_) => {}
                Request::Read(opts, reply) => {
                    let _ = reply.send(read(hwnd, &formats, &opts));
                }
                Request::Write(clip, reply) => {
                    let _ = reply.send(write(hwnd, &formats, &clip));
                }
                Request::Quit => break 'outer,
            }
        }
    }
    unsafe {
        if listening {
            let _ = RemoveClipboardFormatListener(hwnd);
        }
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
        drop(Box::from_raw(cb));
    }
}

unsafe extern "system" fn proc_(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_CLIPBOARDUPDATE {
        let p = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const ChangeCallback;
        if !p.is_null() {
            unsafe { (*p)() };
        }
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// Holds the clipboard open; closes it when dropped.
struct OpenGuard;

impl OpenGuard {
    fn open(hwnd: HWND) -> Result<Self, String> {
        for delay in OPEN_RETRY_MS {
            if delay > 0 {
                thread::sleep(Duration::from_millis(delay));
            }
            if unsafe { OpenClipboard(Some(hwnd)) }.is_ok() {
                return Ok(OpenGuard);
            }
        }
        Err("another application is using the clipboard".into())
    }
}

impl Drop for OpenGuard {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

fn available(format: u32) -> bool {
    format != 0 && unsafe { IsClipboardFormatAvailable(format) }.is_ok()
}

/// Copies the data of a clipboard format (at most `max` bytes, else `Err(true)`).
/// Must be called with the clipboard open.
fn data(format: u32, max: usize) -> Result<Vec<u8>, bool> {
    let h = unsafe { GetClipboardData(format) }.map_err(|_| false)?;
    let hg = HGLOBAL(h.0);
    let size = unsafe { GlobalSize(hg) };
    if size == 0 {
        return Err(false);
    }
    if size > max {
        return Err(true);
    }
    let p = unsafe { GlobalLock(hg) } as *const u8;
    if p.is_null() {
        return Err(false);
    }
    let v = unsafe { std::slice::from_raw_parts(p, size) }.to_vec();
    let _ = unsafe { GlobalUnlock(hg) };
    Ok(v)
}

fn until_nul(mut v: Vec<u8>) -> Vec<u8> {
    if let Some(n) = v.iter().position(|b| *b == 0) {
        v.truncate(n);
    }
    v
}

/// True if the source asked monitors and clipboard history to leave this content alone.
fn is_sensitive(f: &Formats) -> bool {
    if available(f.exclude) {
        return true;
    }
    let dword_zero = |format| available(format) && data(format, 64).is_ok_and(|d| d.len() >= 4 && d[..4] == [0, 0, 0, 0]);
    dword_zero(f.can_history) || dword_zero(f.can_cloud)
}

fn read(hwnd: HWND, f: &Formats, opts: &ReadOptions) -> Result<LocalRead, String> {
    let _open = OpenGuard::open(hwnd)?;
    let seq = unsafe { GetClipboardSequenceNumber() };
    if opts.skip_sensitive && is_sensitive(f) {
        return Ok(LocalRead::Sensitive { seq });
    }
    let max = opts.max_bytes;
    let mut clip = Clip::default();
    let oversized = std::cell::Cell::new(false);
    let take = |format: u32, limit: usize| match data(format, limit) {
        Ok(v) => Some(v),
        Err(big) => {
            oversized.set(oversized.get() | big);
            None
        }
    };
    let has_text = available(CF_UNICODETEXT);
    if opts.filter.text && has_text {
        // UTF-16: twice the bytes of ASCII text at most; checked again after conversion.
        clip.text = take(CF_UNICODETEXT, max.saturating_mul(2)).map(|v| {
            let units: Vec<u16> = (0..v.len() / 2).map(|i| u16::from_le_bytes([v[2 * i], v[2 * i + 1]])).take_while(|u| *u != 0).collect();
            String::from_utf16_lossy(&units)
        });
        if clip.text.as_ref().is_some_and(|t| t.len() > max) {
            clip.text = None;
            oversized.set(true);
        }
    }
    if opts.filter.rich && clip.text.is_some() {
        clip.html = available(f.html).then(|| take(f.html, max)).flatten().map(until_nul).filter(|v| !v.is_empty());
        clip.rtf = available(f.rtf).then(|| take(f.rtf, max)).flatten().map(until_nul).filter(|v| !v.is_empty());
    }
    // Images only when there is no text: some applications offer a rendering of copied text.
    if opts.filter.images && !has_text {
        clip.png = available(f.png).then(|| take(f.png, max)).flatten().and_then(|mut v| {
            v.truncate(png_len(&v)?);
            Some(v)
        });
        clip.dib = available(CF_DIB).then(|| take(CF_DIB, max)).flatten().and_then(|mut v| {
            v.truncate(dib_len(&v)?);
            Some(v)
        });
    }
    Ok(LocalRead::Clip { seq, clip, oversized: oversized.get() })
}

fn write(hwnd: HWND, f: &Formats, clip: &Clip) -> Result<u32, String> {
    let open = OpenGuard::open(hwnd)?;
    unsafe { EmptyClipboard() }.map_err(|e| format!("cannot empty the clipboard: {e}"))?;
    if let Some(t) = &clip.text {
        let wide: Vec<u8> = t.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect();
        set(CF_UNICODETEXT, &wide)?;
    }
    let with_nul = |d: &[u8]| d.iter().copied().chain(Some(0)).collect::<Vec<u8>>();
    if let Some(d) = &clip.html {
        set(f.html, &with_nul(d))?;
    }
    if let Some(d) = &clip.rtf {
        set(f.rtf, &with_nul(d))?;
    }
    if let Some(d) = &clip.dib {
        set(CF_DIB, d)?;
    }
    if let Some(d) = &clip.png {
        set(f.png, d)?;
    }
    drop(open);
    Ok(unsafe { GetClipboardSequenceNumber() })
}

/// Hands a copy of `data` to the clipboard (which then owns the memory).
fn set(format: u32, data: &[u8]) -> Result<(), String> {
    if format == 0 {
        return Err("clipboard format not registered".into());
    }
    unsafe {
        let hg = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)).map_err(|e| format!("out of memory: {e}"))?;
        let p = GlobalLock(hg) as *mut u8;
        if p.is_null() {
            let _ = GlobalFree(Some(hg));
            return Err("cannot lock clipboard memory".into());
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), p, data.len());
        let _ = GlobalUnlock(hg);
        if let Err(e) = SetClipboardData(format, Some(HANDLE(hg.0))) {
            let _ = GlobalFree(Some(hg));
            return Err(format!("cannot set clipboard data: {e}"));
        }
    }
    Ok(())
}

/// Xpress Huffman compression (Windows Compression API). `None` unless the result is
/// smaller than the input.
pub fn compress(data: &[u8]) -> Option<Vec<u8>> {
    let mut h = COMPRESSOR_HANDLE(std::ptr::null_mut());
    unsafe { CreateCompressor(COMPRESS_ALGORITHM_XPRESS_HUFF, None, &mut h) }.ok()?;
    let mut out = vec![0u8; data.len()];
    let mut n = 0usize;
    let r = unsafe { Compress(h, Some(data.as_ptr().cast()), data.len(), Some(out.as_mut_ptr().cast()), out.len(), &mut n) };
    let _ = unsafe { CloseCompressor(h) };
    r.ok()?;
    (n < data.len()).then(|| {
        out.truncate(n);
        out
    })
}

/// Reverses [`compress`]; `None` unless the data decompresses to exactly `raw_len` bytes.
pub fn decompress(data: &[u8], raw_len: usize) -> Option<Vec<u8>> {
    let mut h = DECOMPRESSOR_HANDLE(std::ptr::null_mut());
    unsafe { CreateDecompressor(COMPRESS_ALGORITHM_XPRESS_HUFF, None, &mut h) }.ok()?;
    let mut out = vec![0u8; raw_len];
    let mut n = 0usize;
    let r = unsafe { Decompress(h, Some(data.as_ptr().cast()), data.len(), Some(out.as_mut_ptr().cast()), out.len(), Some(&mut n)) };
    let _ = unsafe { CloseDecompressor(h) };
    r.ok()?;
    (n == raw_len).then_some(out)
}

/// Opens an http(s) link in the default browser. The text is validated again here.
pub fn open_url(text: &str) -> io::Result<()> {
    let url = as_link(text).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "only http and https links can be opened"))?;
    let wide: Vec<u16> = url.encode_utf16().chain(Some(0)).collect();
    let r = unsafe { ShellExecuteW(None, w!("open"), PCWSTR(wide.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if r.0 as isize <= 32 {
        return Err(io::Error::other("the link could not be opened"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use moush_core::clipboard::{FormatFilter, MAX_CLIP_BYTES};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn compression_roundtrips_and_bounds_output() {
        let text: Vec<u8> = "Zeile \u{1F600} \u{4E2D}\u{6587}\r\n".repeat(5000).into_bytes();
        let c = compress(&text).expect("compressible");
        assert!(c.len() < text.len() / 5);
        assert_eq!(decompress(&c, text.len()).as_deref(), Some(&text[..]));
        assert_eq!(decompress(&c, text.len() - 1), None, "wrong announced size");
        assert_eq!(decompress(&c, text.len() + 1), None, "wrong announced size");
        assert_eq!(decompress(&c[..c.len() / 2], text.len()), None, "truncated stream");
        // Incompressible data is not "compressed".
        let mut x = 0x1234_5678u32;
        let noise: Vec<u8> = (0..100_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                x as u8
            })
            .collect();
        assert!(compress(&noise).is_none());
    }

    /// Uses the real clipboard of this session (overwrites it), so it is opt-in:
    /// `cargo test -p moush-engine win::clipboard -- --ignored`.
    #[test]
    #[ignore]
    fn listener_sees_writes_and_text_reads_back() {
        let changes = Arc::new(AtomicUsize::new(0));
        let c2 = changes.clone();
        let cb = WinClipboard::start(Box::new(move || {
            c2.fetch_add(1, Ordering::SeqCst);
        }))
        .unwrap();
        cb.set_listening(true);
        thread::sleep(Duration::from_millis(100));
        let text = "Moush test \u{1F600}\r\n\u{4E2D}\u{6587}\r\nline 3";
        let clip = Clip { text: Some(text.into()), html: Some(b"Version:0.9\r\n<b>x</b>".to_vec()), ..Default::default() };
        let seq = cb.write(Arc::new(clip.clone())).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        while changes.load(Ordering::SeqCst) == 0 && std::time::Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert!(changes.load(Ordering::SeqCst) > 0, "WM_CLIPBOARDUPDATE received");
        assert_eq!(cb.sequence(), seq, "nobody else wrote meanwhile");
        let opts = ReadOptions { filter: FormatFilter { text: true, rich: true, images: true }, skip_sensitive: true, max_bytes: MAX_CLIP_BYTES };
        let LocalRead::Clip { seq: read_seq, clip: back, oversized } = cb.read(&opts).unwrap() else { panic!("not sensitive") };
        assert_eq!(read_seq, seq);
        assert!(!oversized);
        assert_eq!(back.text.as_deref(), Some(text));
        assert_eq!(back.html, clip.html);
        assert_eq!(back.hash(), clip.hash(), "a re-read hashes like the written clip");
        // A bitmap round-trips exactly too (Windows synthesizes the other bitmap formats).
        let mut dib = vec![0u8; 40 + 4 * 3 * 2];
        dib[0] = 40;
        dib[4] = 3;
        dib[8] = 2;
        dib[12] = 1;
        dib[14] = 32;
        for (i, b) in dib[40..].iter_mut().enumerate() {
            *b = i as u8;
        }
        let img = Clip { dib: Some(dib), ..Default::default() };
        cb.write(Arc::new(img.clone())).unwrap();
        let LocalRead::Clip { clip: back, .. } = cb.read(&opts).unwrap() else { panic!() };
        assert_eq!(back.dib, img.dib);
        // Content marked the way password managers do is reported without being read.
        for (name, value) in [("ExcludeClipboardContentFromMonitorProcessing", 1u32), ("CanIncludeInClipboardHistory", 0)] {
            {
                let _open = OpenGuard::open(cb.hwnd()).unwrap();
                unsafe { EmptyClipboard() }.unwrap();
                let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
                let marker = unsafe { RegisterClipboardFormatW(PCWSTR(wide.as_ptr())) };
                set(CF_UNICODETEXT, &[b's', 0, 0, 0]).unwrap();
                set(marker, &value.to_le_bytes()).unwrap();
            }
            assert!(matches!(cb.read(&opts).unwrap(), LocalRead::Sensitive { .. }), "{name}");
            let LocalRead::Clip { clip, .. } = cb.read(&ReadOptions { skip_sensitive: false, ..opts }).unwrap() else { panic!() };
            assert_eq!(clip.text.as_deref(), Some("s"), "read when the user allows it");
        }
        cb.write(Arc::new(Clip::text(""))).unwrap();
        cb.set_listening(false);
    }
}
