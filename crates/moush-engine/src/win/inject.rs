//! Client-side input injection with `SendInput`.
//!
//! Cursor moves are injected as absolute positions on the virtual desktop, which
//! bypasses the client's own pointer acceleration (the host already applied it).
//! Keys are injected by scan code so the client's keyboard layout interprets them,
//! exactly as if the physical keyboard were attached there.

use std::mem::size_of;

use windows::Win32::UI::Input::KeyboardAndMouse::{
    INPUT, INPUT_0, INPUT_KEYBOARD, INPUT_MOUSE, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_EXTENDEDKEY, KEYEVENTF_KEYUP,
    KEYEVENTF_SCANCODE, MOUSE_EVENT_FLAGS, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_HWHEEL, MOUSEEVENTF_LEFTDOWN,
    MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MIDDLEDOWN, MOUSEEVENTF_MIDDLEUP, MOUSEEVENTF_MOVE, MOUSEEVENTF_RIGHTDOWN,
    MOUSEEVENTF_RIGHTUP, MOUSEEVENTF_VIRTUALDESK, MOUSEEVENTF_WHEEL, MOUSEEVENTF_XDOWN, MOUSEEVENTF_XUP, MOUSEINPUT,
    SendInput, VIRTUAL_KEY,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN,
};

use moush_core::inject::Injector;
use moush_core::protocol::{MouseButton, WheelAxis};

/// Tag on every injected event so our own hooks can recognise them.
pub const INJECT_MARKER: usize = 0x4D53_4852; // "MSHR"

const XBUTTON1: u32 = 1;
const XBUTTON2: u32 = 2;

#[derive(Default)]
pub struct SendInputInjector;

impl SendInputInjector {
    pub fn new() -> Self {
        Self
    }
}

/// Maps a virtual-desktop pixel to SendInput's 0..=65535 absolute range. Windows maps
/// back with `pixel = normalized * size / 65536`, so rounding up hits the exact pixel.
pub fn normalize(p: i32, origin: i32, size: i32) -> i32 {
    if size <= 1 {
        return 0;
    }
    let rel = (p - origin).clamp(0, size - 1) as i64;
    (((rel * 65536) + size as i64 - 1) / size as i64).min(65535) as i32
}

fn send(input: INPUT) -> bool {
    unsafe { SendInput(&[input], size_of::<INPUT>() as i32) == 1 }
}

fn mouse(dx: i32, dy: i32, data: u32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data, dwFlags: flags, time: 0, dwExtraInfo: INJECT_MARKER },
        },
    }
}

impl Injector for SendInputInjector {
    fn move_abs(&mut self, x: i32, y: i32) -> bool {
        let (vx, vy, vw, vh) = unsafe {
            (
                GetSystemMetrics(SM_XVIRTUALSCREEN),
                GetSystemMetrics(SM_YVIRTUALSCREEN),
                GetSystemMetrics(SM_CXVIRTUALSCREEN),
                GetSystemMetrics(SM_CYVIRTUALSCREEN),
            )
        };
        send(mouse(
            normalize(x, vx, vw),
            normalize(y, vy, vh),
            0,
            MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | MOUSEEVENTF_VIRTUALDESK,
        ))
    }

    fn button(&mut self, button: MouseButton, down: bool) -> bool {
        let (flags, data) = match (button, down) {
            (MouseButton::Left, true) => (MOUSEEVENTF_LEFTDOWN, 0),
            (MouseButton::Left, false) => (MOUSEEVENTF_LEFTUP, 0),
            (MouseButton::Right, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
            (MouseButton::Right, false) => (MOUSEEVENTF_RIGHTUP, 0),
            (MouseButton::Middle, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
            (MouseButton::Middle, false) => (MOUSEEVENTF_MIDDLEUP, 0),
            (MouseButton::X1, true) => (MOUSEEVENTF_XDOWN, XBUTTON1),
            (MouseButton::X1, false) => (MOUSEEVENTF_XUP, XBUTTON1),
            (MouseButton::X2, true) => (MOUSEEVENTF_XDOWN, XBUTTON2),
            (MouseButton::X2, false) => (MOUSEEVENTF_XUP, XBUTTON2),
        };
        send(mouse(0, 0, data, flags))
    }

    fn wheel(&mut self, axis: WheelAxis, delta: i16) -> bool {
        let flags = match axis {
            WheelAxis::Vertical => MOUSEEVENTF_WHEEL,
            WheelAxis::Horizontal => MOUSEEVENTF_HWHEEL,
        };
        send(mouse(0, 0, delta as i32 as u32, flags))
    }

    fn key(&mut self, vk: u16, scan: u16, extended: bool, down: bool, by_vk: bool) -> bool {
        let mut flags = KEYBD_EVENT_FLAGS(0);
        if !by_vk {
            flags |= KEYEVENTF_SCANCODE;
        }
        if extended {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        if !down {
            flags |= KEYEVENTF_KEYUP;
        }
        send(INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(if by_vk { vk } else { 0 }),
                    wScan: scan,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: INJECT_MARKER,
                },
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::normalize;

    /// Windows converts back with `normalized * size >> 16`; every pixel must survive.
    #[test]
    fn normalization_roundtrips_every_pixel() {
        for size in [800, 1366, 1920, 2560, 3840, 5120, 7680] {
            for origin in [0, -1920] {
                for px in 0..size {
                    let n = normalize(origin + px, origin, size) as i64;
                    assert_eq!((n * size as i64) >> 16, px as i64, "size {size} px {px}");
                }
            }
        }
    }
}
