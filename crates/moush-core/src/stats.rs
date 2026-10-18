//! Lightweight runtime statistics. Recording a sample is a short uncontended mutex
//! section; percentiles are only computed when the diagnostics view asks for them.

use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Serialize;

const RING: usize = 4096;

/// Fixed-size ring of the most recent samples.
pub struct LatencyRing {
    buf: Vec<u32>,
    pos: usize,
    len: usize,
}

impl Default for LatencyRing {
    fn default() -> Self {
        Self { buf: vec![0; RING], pos: 0, len: 0 }
    }
}

impl LatencyRing {
    pub fn record(&mut self, v: u32) {
        self.buf[self.pos] = v;
        self.pos = (self.pos + 1) % RING;
        self.len = (self.len + 1).min(RING);
    }

    pub fn clear(&mut self) {
        self.pos = 0;
        self.len = 0;
    }

    pub fn summary(&self) -> LatencySummary {
        if self.len == 0 {
            return LatencySummary::default();
        }
        let mut v: Vec<u32> = self.buf[..self.len].to_vec();
        v.sort_unstable();
        let pct = |p: f64| v[(((v.len() - 1) as f64) * p).round() as usize] as f64;
        LatencySummary {
            count: v.len() as u64,
            mean: v.iter().map(|x| *x as f64).sum::<f64>() / v.len() as f64,
            p50: pct(0.50),
            p95: pct(0.95),
            p99: pct(0.99),
            max: *v.last().unwrap() as f64,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Serialize, PartialEq)]
pub struct LatencySummary {
    pub count: u64,
    pub mean: f64,
    pub p50: f64,
    pub p95: f64,
    pub p99: f64,
    pub max: f64,
}

impl LatencySummary {
    /// Rescales every value (e.g. microseconds to milliseconds).
    pub fn scaled(self, f: f64) -> Self {
        Self { count: self.count, mean: self.mean * f, p50: self.p50 * f, p95: self.p95 * f, p99: self.p99 * f, max: self.max * f }
    }
}

#[derive(Default)]
pub struct Stats {
    /// Host: time spent inside the low-level hook callback (µs).
    pub hook_us: Mutex<LatencyRing>,
    /// Host: hook timestamp -> frame written to the socket (µs).
    pub send_us: Mutex<LatencyRing>,
    /// Client: frame received -> SendInput returned (µs).
    pub inject_us: Mutex<LatencyRing>,
    /// Round-trip time of keepalive pings (µs).
    pub rtt_us: Mutex<LatencyRing>,
    pub events_captured: AtomicU64,
    pub events_forwarded: AtomicU64,
    pub events_injected: AtomicU64,
    pub inject_failures: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub frames_sent: AtomicU64,
    pub frames_received: AtomicU64,
    pub reconnects: AtomicU64,
    /// Duration of the last successful reconnect in ms; `u64::MAX` when none yet.
    pub last_reconnect_ms: AtomicU64,
}

impl Stats {
    pub fn new() -> Self {
        let s = Self::default();
        s.last_reconnect_ms.store(u64::MAX, Ordering::Relaxed);
        s
    }

    pub fn record(ring: &Mutex<LatencyRing>, v: u64) {
        if let Ok(mut r) = ring.lock() {
            r.record(v.min(u32::MAX as u64) as u32);
        }
    }

    pub fn summary(ring: &Mutex<LatencyRing>) -> LatencySummary {
        ring.lock().map(|r| r.summary()).unwrap_or_default()
    }

    pub fn add(counter: &AtomicU64, n: u64) {
        counter.fetch_add(n, Ordering::Relaxed);
    }

    pub fn get(counter: &AtomicU64) -> u64 {
        counter.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles() {
        let mut r = LatencyRing::default();
        for v in 1..=100 {
            r.record(v);
        }
        let s = r.summary();
        assert_eq!(s.count, 100);
        assert_eq!(s.p50, 51.0);
        assert_eq!(s.p99, 99.0);
        assert_eq!(s.max, 100.0);
        assert!((s.mean - 50.5).abs() < 1e-9);
    }

    #[test]
    fn ring_keeps_latest() {
        let mut r = LatencyRing::default();
        for _ in 0..RING {
            r.record(1000);
        }
        for _ in 0..RING {
            r.record(1);
        }
        assert_eq!(r.summary().max, 1.0);
    }
}
