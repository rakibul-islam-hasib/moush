//! Screen capture with GDI: copies a rectangle of the virtual desktop into a top-down BGRA
//! buffer, scaled with the system's HALFTONE filter. Fast enough for 15–30 fps JPEG
//! streaming and works on every Windows version; protected (DRM) content and the secure
//! desktop come out black, like in any other screen-sharing tool.

use std::ffi::c_void;
use std::mem::size_of;

use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC,
    DeleteObject, GetDC, HALFTONE, HBITMAP, HDC, HGDIOBJ, ReleaseDC, SRCCOPY, SelectObject, SetBrushOrgEx,
    SetStretchBltMode, StretchBlt,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

/// Reusable capture surface. Not `Send`-safe across threads in general: create and use it
/// on one thread.
pub struct Grabber {
    screen: HDC,
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
    bits: *mut c_void,
    w: i32,
    h: i32,
}

// The GDI handles are only used by the thread that owns the grabber.
unsafe impl Send for Grabber {}

impl Grabber {
    pub fn new() -> Option<Self> {
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return None;
            }
            let mem = CreateCompatibleDC(Some(screen));
            if mem.is_invalid() {
                ReleaseDC(None, screen);
                return None;
            }
            Some(Self {
                screen,
                mem,
                bmp: HBITMAP::default(),
                old: HGDIOBJ::default(),
                bits: std::ptr::null_mut(),
                w: 0,
                h: 0,
            })
        }
    }

    fn ensure_surface(&mut self, w: i32, h: i32) -> bool {
        if self.w == w && self.h == h && !self.bits.is_null() {
            return true;
        }
        unsafe {
            if !self.bmp.is_invalid() {
                SelectObject(self.mem, self.old);
                let _ = DeleteObject(HGDIOBJ(self.bmp.0));
                self.bmp = HBITMAP::default();
                self.bits = std::ptr::null_mut();
            }
            let bmi = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: w,
                    biHeight: -h,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB.0,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut bits: *mut c_void = std::ptr::null_mut();
            let Ok(bmp) = CreateDIBSection(Some(self.mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0) else {
                return false;
            };
            if bits.is_null() {
                let _ = DeleteObject(HGDIOBJ(bmp.0));
                return false;
            }
            self.old = SelectObject(self.mem, HGDIOBJ(bmp.0));
            self.bmp = bmp;
            self.bits = bits;
            self.w = w;
            self.h = h;
        }
        true
    }

    /// Copies `src` = (x, y, w, h) of the virtual desktop, scaled to `out` = (w, h) pixels.
    /// Returns the BGRA bytes (`out.0 * out.1 * 4`), valid until the next call.
    pub fn grab(&mut self, src: (i32, i32, i32, i32), out: (i32, i32)) -> Option<&[u8]> {
        let (sx, sy, sw, sh) = src;
        let (ow, oh) = out;
        if sw < 1 || sh < 1 || ow < 1 || oh < 1 || !self.ensure_surface(ow, oh) {
            return None;
        }
        unsafe {
            SetStretchBltMode(self.mem, HALFTONE);
            let _ = SetBrushOrgEx(self.mem, 0, 0, None);
            StretchBlt(self.mem, 0, 0, ow, oh, Some(self.screen), sx, sy, sw, sh, SRCCOPY | CAPTUREBLT).ok().ok()?;
            Some(std::slice::from_raw_parts(self.bits as *const u8, (ow as usize) * (oh as usize) * 4))
        }
    }
}

impl Drop for Grabber {
    fn drop(&mut self) {
        unsafe {
            if !self.bmp.is_invalid() {
                SelectObject(self.mem, self.old);
                let _ = DeleteObject(HGDIOBJ(self.bmp.0));
            }
            let _ = DeleteDC(self.mem);
            ReleaseDC(None, self.screen);
        }
    }
}

/// Mouse cursor position in virtual-desktop pixels (the calling thread must be DPI aware).
pub fn cursor_pos() -> Option<(i32, i32)> {
    let mut p = POINT::default();
    unsafe { GetCursorPos(&mut p).ok()? };
    Some((p.x, p.y))
}
