//! Logger: keeps recent entries in memory for the diagnostics view, appends to a log
//! file, and forwards entries to an optional listener (the UI).

use std::collections::VecDeque;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use log::{Level, LevelFilter, Log, Metadata, Record};
use serde::Serialize;

use moush_core::util::now_ms;

const RING: usize = 2000;
const MAX_FILE: u64 = 2 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct LogEntry {
    pub ts: u64,
    pub level: &'static str,
    pub target: String,
    pub message: String,
}

type Listener = Box<dyn Fn(&LogEntry) + Send + Sync>;

pub struct Logger {
    ring: Mutex<VecDeque<LogEntry>>,
    file: Mutex<Option<File>>,
    path: Option<PathBuf>,
    listener: Mutex<Option<Listener>>,
    level: LevelFilter,
    echo: bool,
}

static LOGGER: OnceLock<Logger> = OnceLock::new();

/// Installs the global logger. Our own crates log at `level`; dependencies at warn.
/// `echo` also prints entries to stderr (CLI and debug builds).
pub fn init(log_file: Option<&Path>, level: LevelFilter, echo: bool) -> &'static Logger {
    let logger = LOGGER.get_or_init(|| {
        let file = log_file.and_then(|p| {
            if let Some(dir) = p.parent() {
                let _ = fs::create_dir_all(dir);
            }
            if fs::metadata(p).map(|m| m.len() > MAX_FILE).unwrap_or(false) {
                let _ = fs::rename(p, p.with_extension("old.log"));
            }
            OpenOptions::new().create(true).append(true).open(p).ok()
        });
        Logger {
            ring: Mutex::new(VecDeque::with_capacity(RING)),
            file: Mutex::new(file),
            path: log_file.map(Path::to_path_buf),
            listener: Mutex::new(None),
            level,
            echo,
        }
    });
    if log::set_logger(logger).is_ok() {
        log::set_max_level(level.max(LevelFilter::Warn));
    }
    logger
}

pub fn get() -> Option<&'static Logger> {
    LOGGER.get()
}

impl Logger {
    pub fn recent(&self, limit: usize) -> Vec<LogEntry> {
        let ring = self.ring.lock().unwrap();
        ring.iter().skip(ring.len().saturating_sub(limit)).cloned().collect()
    }

    pub fn set_listener(&self, f: impl Fn(&LogEntry) + Send + Sync + 'static) {
        *self.listener.lock().unwrap() = Some(Box::new(f));
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }
}

fn ours(target: &str) -> bool {
    let t = target.as_bytes();
    let starts = |p: &str| t.len() >= p.len() && t[..p.len()].eq_ignore_ascii_case(p.as_bytes());
    starts("moush")
}

impl Log for Logger {
    fn enabled(&self, m: &Metadata<'_>) -> bool {
        if ours(m.target()) { m.level() <= self.level } else { m.level() <= Level::Warn }
    }

    fn log(&self, record: &Record<'_>) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let entry = LogEntry {
            ts: now_ms(),
            level: match record.level() {
                Level::Error => "error",
                Level::Warn => "warn",
                Level::Info => "info",
                Level::Debug | Level::Trace => "debug",
            },
            target: record.target().rsplit("::").next().unwrap_or("").to_owned(),
            message: record.args().to_string(),
        };
        if let Some(f) = self.file.lock().unwrap().as_mut() {
            let _ = writeln!(f, "{} {:5} [{}] {}", format_ts(entry.ts), entry.level, record.target(), entry.message);
        }
        if self.echo {
            eprintln!("{:5} [{}] {}", entry.level, entry.target, entry.message);
        }
        {
            let mut ring = self.ring.lock().unwrap();
            if ring.len() == RING {
                ring.pop_front();
            }
            ring.push_back(entry.clone());
        }
        if record.level() <= Level::Info
            && let Some(l) = self.listener.lock().unwrap().as_ref()
        {
            l(&entry);
        }
    }

    fn flush(&self) {
        if let Some(f) = self.file.lock().unwrap().as_mut() {
            let _ = f.flush();
        }
    }
}

/// `YYYY-MM-DD HH:MM:SS.mmmZ` (UTC) without pulling in a date crate.
fn format_ts(ms: u64) -> String {
    let secs = ms / 1000;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let days = (secs / 86400) as i64;
    // Civil-from-days (H. Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if mo <= 2 { 1 } else { 0 };
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{m:02}:{s:02}.{:03}Z", ms % 1000)
}

#[cfg(test)]
mod tests {
    #[test]
    fn timestamp_format() {
        assert_eq!(super::format_ts(0), "1970-01-01 00:00:00.000Z");
        assert_eq!(super::format_ts(1_760_000_000_123), "2025-10-09 08:53:20.123Z");
    }
}
