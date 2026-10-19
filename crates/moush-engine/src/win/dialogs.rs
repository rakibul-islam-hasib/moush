//! Native shell dialogs: the file picker (multi-select), the folder picker and "show in
//! Explorer". Each runs on its own short-lived COM single-threaded-apartment thread, so it
//! never blocks the UI thread, input threads or the caller's apartment.

use std::ffi::c_void;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::thread;

use windows::Win32::Foundation::{ERROR_CANCELLED, HWND};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoCreateInstance, CoInitializeEx,
    CoTaskMemFree, CoUninitialize,
};
use windows::Win32::UI::Shell::{
    FOS_ALLOWMULTISELECT, FOS_FILEMUSTEXIST, FOS_FORCEFILESYSTEM, FOS_PATHMUSTEXIST, FOS_PICKFOLDERS, FileOpenDialog,
    IFileOpenDialog, ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems, SIGDN_FILESYSPATH, ShellExecuteW,
};
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, Result as WinResult};

/// Runs `f` on a new STA thread and waits for it.
fn on_sta<T: Send + 'static>(f: impl FnOnce() -> WinResult<T> + Send + 'static) -> Result<T, String> {
    thread::Builder::new()
        .name("shell-dialog".into())
        .spawn(move || unsafe {
            let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
            let r = f();
            if init.is_ok() {
                CoUninitialize();
            }
            r
        })
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "the dialog crashed".to_string())?
        .map_err(|e| e.message())
}

/// Shows the open dialog; returns the chosen paths (empty when cancelled).
fn open_dialog(owner: Option<isize>, title: &str, folders: bool) -> WinResult<Vec<PathBuf>> {
    unsafe {
        let dlg: IFileOpenDialog = CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)?;
        let mut opts = dlg.GetOptions()? | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST;
        opts |= if folders { FOS_PICKFOLDERS } else { FOS_FILEMUSTEXIST | FOS_ALLOWMULTISELECT };
        dlg.SetOptions(opts)?;
        dlg.SetTitle(&HSTRING::from(title))?;
        let owner = owner.map(|h| HWND(h as *mut c_void));
        if let Err(e) = dlg.Show(owner) {
            if e.code() == ERROR_CANCELLED.to_hresult() {
                return Ok(Vec::new());
            }
            return Err(e);
        }
        let items = dlg.GetResults()?;
        let mut out = Vec::new();
        for i in 0..items.GetCount()? {
            let name = items.GetItemAt(i)?.GetDisplayName(SIGDN_FILESYSPATH)?;
            let path = name.to_string();
            CoTaskMemFree(Some(name.0 as *const c_void));
            if let Ok(p) = path {
                out.push(PathBuf::from(p));
            }
        }
        Ok(out)
    }
}

/// Native multi-select file picker. `owner` is the window handle the dialog is modal to.
pub fn pick_files(owner: Option<isize>) -> Result<Vec<PathBuf>, String> {
    on_sta(move || open_dialog(owner, "Choose files to send", false))
}

/// Native folder picker; `None` when cancelled.
pub fn pick_folder(owner: Option<isize>) -> Result<Option<PathBuf>, String> {
    on_sta(move || open_dialog(owner, "Choose a folder", true)).map(|v| v.into_iter().next())
}

fn wide(p: &Path) -> Vec<u16> {
    p.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Opens Explorer with `path` selected (a file) or showing it (a folder). Never opens or
/// runs the file itself. Returns immediately.
pub fn reveal(path: &Path) {
    let path = path.to_path_buf();
    let _ = thread::Builder::new().name("reveal".into()).spawn(move || unsafe {
        let init = CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE);
        let w = wide(&path);
        if path.is_dir() {
            ShellExecuteW(None, &HSTRING::from("explore"), PCWSTR(w.as_ptr()), None, None, SW_SHOWNORMAL);
        } else {
            let pidl = ILCreateFromPathW(PCWSTR(w.as_ptr()));
            if pidl.is_null() {
                log::warn!("cannot locate the file to show in Explorer");
            } else {
                if let Err(e) = SHOpenFolderAndSelectItems(pidl, None, 0) {
                    log::warn!("cannot show the file in Explorer: {e}");
                }
                ILFree(Some(pidl));
            }
        }
        if init.is_ok() {
            CoUninitialize();
        }
    });
}
