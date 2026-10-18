//! Small Windows system integrations: DPAPI, autostart, elevation, OS version, process
//! statistics, firewall rule management.

use std::ffi::OsStr;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::process::CommandExt;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, LocalFree, ERROR_SUCCESS, FILETIME, HANDLE, HLOCAL};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS};
use windows::Win32::System::ProcessStatus::{K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::System::SystemInformation::{ComputerNamePhysicalDnsHostname, GetComputerNameExW};
use windows::Win32::System::Threading::{GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, OpenProcessToken};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{SW_HIDE, SW_SHOWNORMAL};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const FIREWALL_RULE: &str = "Moush";

fn wide(s: impl AsRef<OsStr>) -> Vec<u16> {
    s.as_ref().encode_wide().chain(Some(0)).collect()
}

/// Encrypts `data` for the current Windows user (DPAPI).
pub fn protect(data: &[u8]) -> io::Result<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(&input, PCWSTR::null(), None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .map_err(|e| io::Error::other(format!("CryptProtectData: {e}")))?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(Some(HLOCAL(out.pbData.cast())));
        Ok(v)
    }
}

/// Decrypts data produced by [`protect`] for the same user.
pub fn unprotect(data: &[u8]) -> io::Result<Vec<u8>> {
    unsafe {
        let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_ptr() as *mut u8 };
        let mut out = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(&input, None, None, None, None, CRYPTPROTECT_UI_FORBIDDEN, &mut out)
            .map_err(|e| io::Error::other(format!("CryptUnprotectData: {e}")))?;
        let v = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
        LocalFree(Some(HLOCAL(out.pbData.cast())));
        Ok(v)
    }
}

pub fn computer_name() -> String {
    unsafe {
        let mut len = 0u32;
        let _ = GetComputerNameExW(ComputerNamePhysicalDnsHostname, None, &mut len);
        let mut buf = vec![0u16; len as usize + 1];
        let mut len2 = buf.len() as u32;
        if GetComputerNameExW(ComputerNamePhysicalDnsHostname, Some(PWSTR(buf.as_mut_ptr())), &mut len2).is_ok() {
            return String::from_utf16_lossy(&buf[..len2 as usize]);
        }
    }
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "This PC".into())
}

pub fn is_elevated() -> bool {
    unsafe {
        let mut token = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token).is_err() {
            return false;
        }
        let mut elev = TOKEN_ELEVATION::default();
        let mut len = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            Some(&mut elev as *mut _ as *mut _),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut len,
        )
        .is_ok();
        let _ = CloseHandle(token);
        ok && elev.TokenIsElevated != 0
    }
}

fn reg_get_string(hkey: windows::Win32::System::Registry::HKEY, subkey: &str, value: &str) -> Option<String> {
    let sk = wide(subkey);
    let v = wide(value);
    unsafe {
        let mut len = 0u32;
        if RegGetValueW(hkey, PCWSTR(sk.as_ptr()), PCWSTR(v.as_ptr()), RRF_RT_REG_SZ, None, None, Some(&mut len)) != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u16; (len as usize / 2) + 1];
        let mut len2 = (buf.len() * 2) as u32;
        if RegGetValueW(
            hkey,
            PCWSTR(sk.as_ptr()),
            PCWSTR(v.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr().cast()),
            Some(&mut len2),
        ) != ERROR_SUCCESS
        {
            return None;
        }
        Some(super::monitors::wide_to_string(&buf))
    }
}

/// Windows build number (22000+ is Windows 11).
pub fn os_build() -> u32 {
    reg_get_string(HKEY_LOCAL_MACHINE, r"SOFTWARE\Microsoft\Windows NT\CurrentVersion", "CurrentBuildNumber")
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(0)
}

/// Adds or removes the per-user "run at sign-in" entry.
pub fn set_launch_at_startup(enabled: bool, exe: &Path, args: &str) -> io::Result<()> {
    let key = wide(RUN_KEY);
    let name = wide("Moush");
    unsafe {
        let r = if enabled {
            let cmd = wide(format!("\"{}\" {args}", exe.display()).trim_end());
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                PCWSTR(key.as_ptr()),
                PCWSTR(name.as_ptr()),
                REG_SZ.0,
                Some(cmd.as_ptr().cast()),
                (cmd.len() * 2) as u32,
            )
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, PCWSTR(key.as_ptr()), PCWSTR(name.as_ptr()));
            if r.0 == 2 { ERROR_SUCCESS } else { r } // ERROR_FILE_NOT_FOUND: already absent
        };
        if r != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(r.0 as i32));
        }
    }
    Ok(())
}

pub fn launch_at_startup_enabled() -> bool {
    reg_get_string(HKEY_CURRENT_USER, RUN_KEY, "Moush").is_some()
}

/// Process CPU usage sampling and memory statistics.
pub struct ProcessMonitor {
    last: Option<(Instant, u64)>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessSample {
    pub cpu_percent: f64,
    pub memory_mb: f64,
    pub threads: u32,
}

fn filetime_100ns(f: FILETIME) -> u64 {
    ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
}

impl ProcessMonitor {
    pub fn new() -> Self {
        Self { last: None }
    }

    /// CPU is averaged since the previous call, as a percentage of one logical core
    /// divided by the number of cores (like Task Manager).
    pub fn sample(&mut self) -> ProcessSample {
        let mut s = ProcessSample::default();
        unsafe {
            let p = GetCurrentProcess();
            let (mut c, mut e, mut k, mut u) = Default::default();
            if GetProcessTimes(p, &mut c, &mut e, &mut k, &mut u).is_ok() {
                let busy = filetime_100ns(k) + filetime_100ns(u);
                let now = Instant::now();
                if let Some((t, b)) = self.last {
                    let wall = now.duration_since(t).as_secs_f64();
                    let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1) as f64;
                    if wall > 0.0 {
                        s.cpu_percent = ((busy - b) as f64 / 1e7) / wall / cores * 100.0;
                    }
                }
                self.last = Some((now, busy));
            }
            let mut mem = PROCESS_MEMORY_COUNTERS { cb: size_of::<PROCESS_MEMORY_COUNTERS>() as u32, ..Default::default() };
            if K32GetProcessMemoryInfo(p, &mut mem, mem.cb).as_bool() {
                s.memory_mb = mem.WorkingSetSize as f64 / (1024.0 * 1024.0);
            }
            s.threads = thread_count();
        }
        s
    }
}

impl Default for ProcessMonitor {
    fn default() -> Self {
        Self::new()
    }
}

fn thread_count() -> u32 {
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return 0 };
        let pid = GetCurrentProcessId();
        let mut e = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut n = 0;
        if Process32FirstW(snap, &mut e).is_ok() {
            loop {
                if e.th32ProcessID == pid {
                    n = e.cntThreads;
                    break;
                }
                if Process32NextW(snap, &mut e).is_err() {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        n
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FirewallState {
    Allowed,
    Missing,
    Unknown,
}

/// Checks for our inbound rule. Runs `netsh` (no window); takes ~100 ms.
pub fn firewall_state() -> FirewallState {
    let out = Command::new("netsh")
        .args(["advfirewall", "firewall", "show", "rule", &format!("name={FIREWALL_RULE}"), "dir=in"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    match out {
        Ok(o) if o.status.success() => FirewallState::Allowed,
        // netsh exits with 1 and "No rules match" when the rule does not exist.
        Ok(o) if String::from_utf8_lossy(&o.stdout).contains("No rules") => FirewallState::Missing,
        Ok(o) if o.status.code() == Some(1) => FirewallState::Missing,
        _ => FirewallState::Unknown,
    }
}

/// Adds an inbound allow rule for `exe` on private and domain networks. Shows the UAC
/// prompt; returns once the elevated `netsh` was launched.
pub fn add_firewall_rule(exe: &Path) -> io::Result<()> {
    let params = format!(
        "advfirewall firewall add rule name=\"{FIREWALL_RULE}\" dir=in action=allow program=\"{}\" enable=yes profile=private,domain",
        exe.display()
    );
    let verb = wide("runas");
    let file = wide("netsh.exe");
    let params = wide(params);
    let r = unsafe {
        ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(file.as_ptr()), PCWSTR(params.as_ptr()), PCWSTR::null(), SW_HIDE)
    };
    if r.0 as isize <= 32 {
        return Err(io::Error::other("the firewall rule was not added (administrator approval was declined or failed)"));
    }
    Ok(())
}

/// Opens a folder in Explorer.
pub fn open_folder(path: &Path) -> io::Result<()> {
    let verb = wide("open");
    let p = wide(path.as_os_str());
    let r = unsafe { ShellExecuteW(None, PCWSTR(verb.as_ptr()), PCWSTR(p.as_ptr()), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
    if r.0 as isize <= 32 {
        return Err(io::Error::other(format!("cannot open {}", path.display())));
    }
    Ok(())
}
