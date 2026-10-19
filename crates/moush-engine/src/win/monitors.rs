//! Monitor enumeration in physical pixels with per-monitor DPI and friendly names.

use std::collections::HashMap;
use std::mem::size_of;

use windows::Win32::Devices::Display::{
    DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_MODE_INFO,
    DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME,
    DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QDC_ONLY_ACTIVE_PATHS, QueryDisplayConfig,
};
use windows::Win32::Foundation::{ERROR_SUCCESS, LPARAM, RECT};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::MONITORINFOF_PRIMARY;
use windows::core::BOOL;

use moush_core::geometry::Rect;
use moush_core::protocol::MonitorDesc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Monitor {
    /// GDI device name, e.g. `\\.\DISPLAY1`.
    pub device: String,
    pub name: String,
    pub rect: Rect,
    pub dpi: u32,
    pub primary: bool,
}

impl Monitor {
    pub fn to_desc(&self) -> MonitorDesc {
        MonitorDesc {
            x: self.rect.x,
            y: self.rect.y,
            width: self.rect.w,
            height: self.rect.h,
            dpi: self.dpi.clamp(48, 960) as u16,
            primary: self.primary,
            name: self.name.clone(),
        }
    }
}

/// Lists monitors, primary first. The calling thread must be per-monitor DPI aware
/// (see [`super::dpi::make_thread_dpi_aware`]) to get physical-pixel coordinates.
pub fn enumerate() -> Vec<Monitor> {
    let mut handles: Vec<HMONITOR> = Vec::new();
    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        unsafe { (*(data.0 as *mut Vec<HMONITOR>)).push(h) };
        BOOL(1)
    }
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(collect), LPARAM(&mut handles as *mut _ as isize));
    }
    let names = friendly_names();
    let mut out: Vec<Monitor> = handles
        .into_iter()
        .filter_map(|h| {
            let mut info = MONITORINFOEXW::default();
            info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
            if !unsafe { GetMonitorInfoW(h, &mut info as *mut _ as *mut MONITORINFO) }.as_bool() {
                return None;
            }
            let r = info.monitorInfo.rcMonitor;
            let device = wide_to_string(&info.szDevice);
            let (mut dx, mut dy) = (96u32, 96u32);
            if unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) }.is_err() {
                dx = 96;
            }
            let name = names.get(&device).cloned().unwrap_or_else(|| fallback_name(&device));
            Some(Monitor {
                device,
                name,
                rect: Rect::new(r.left, r.top, r.right - r.left, r.bottom - r.top),
                dpi: dx,
                primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            })
        })
        .collect();
    out.sort_by_key(|m| (!m.primary, m.rect.x, m.rect.y));
    out
}

fn fallback_name(device: &str) -> String {
    match device.rsplit("DISPLAY").next().and_then(|n| n.parse::<u32>().ok()) {
        Some(n) => format!("Display {n}"),
        None => device.trim_start_matches(r"\\.\").to_owned(),
    }
}

/// Maps GDI device names to monitor model names via the display configuration API.
fn friendly_names() -> HashMap<String, String> {
    let mut map = HashMap::new();
    unsafe {
        let (mut np, mut nm) = (0u32, 0u32);
        if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm) != ERROR_SUCCESS {
            return map;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
        if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None)
            != ERROR_SUCCESS
        {
            return map;
        }
        paths.truncate(np as usize);
        for p in &paths {
            let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            src.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
            src.header.size = size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
            src.header.adapterId = p.sourceInfo.adapterId;
            src.header.id = p.sourceInfo.id;
            if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                continue;
            }
            let mut tgt = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            tgt.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
            tgt.header.size = size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
            tgt.header.adapterId = p.targetInfo.adapterId;
            tgt.header.id = p.targetInfo.id;
            if DisplayConfigGetDeviceInfo(&mut tgt.header) != 0 {
                continue;
            }
            let name = wide_to_string(&tgt.monitorFriendlyDeviceName);
            if !name.is_empty() {
                map.insert(wide_to_string(&src.viewGdiDeviceName), name);
            }
        }
    }
    map
}

pub(crate) fn wide_to_string(w: &[u16]) -> String {
    let end = w.iter().position(|c| *c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}
