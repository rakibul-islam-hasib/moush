use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext, SetThreadDpiAwarenessContext,
};

/// Makes the process per-monitor DPI aware (v2) so every coordinate is in physical
/// pixels. Tauri already does this for the GUI; the CLI calls it at startup.
pub fn make_process_dpi_aware() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

/// Same for the calling thread only; used by threads that read or set cursor positions.
pub fn make_thread_dpi_aware() {
    unsafe {
        SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}
