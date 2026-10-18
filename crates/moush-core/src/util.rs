use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Wall-clock time in milliseconds since the Unix epoch.
pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn epoch() -> Instant {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    *EPOCH.get_or_init(Instant::now)
}

/// Monotonic microseconds since first use; for latency measurements.
pub fn mono_us() -> u64 {
    epoch().elapsed().as_micros() as u64
}

/// Monotonic milliseconds since first use.
pub fn mono_ms() -> u64 {
    epoch().elapsed().as_millis() as u64
}

/// Writes a file via a temporary sibling and rename, so a crash never leaves it half-written.
pub fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    {
        let mut f = fs::File::create(&tmp)?;
        f.write_all(data)?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path)
}
