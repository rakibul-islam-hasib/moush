//! Credit-based flow control for bulk feature streams.
//!
//! A sender may have at most `capacity` bytes in flight (sent but not acknowledged by
//! the receiver). This bounds how much bulk data can sit in front of input in socket
//! buffers, so a transfer never adds more than `capacity / bandwidth` to input latency,
//! and gives end-to-end backpressure: a receiver that writes slowly to disk acknowledges
//! slowly, which slows the sender down.

use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

#[derive(Debug)]
struct State {
    in_flight: usize,
    closed: bool,
}

#[derive(Debug)]
pub struct Window {
    capacity: usize,
    state: Mutex<State>,
    cv: Condvar,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Acquire {
    Ok,
    TimedOut,
    Closed,
}

impl Window {
    pub fn new(capacity: usize) -> Self {
        Self { capacity: capacity.max(1), state: Mutex::new(State { in_flight: 0, closed: false }), cv: Condvar::new() }
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn in_flight(&self) -> usize {
        self.state.lock().unwrap().in_flight
    }

    /// Blocks until `n` more bytes may be sent. A request larger than the capacity is
    /// granted once nothing else is in flight.
    pub fn acquire(&self, n: usize, timeout: Duration) -> Acquire {
        let deadline = Instant::now() + timeout;
        let mut s = self.state.lock().unwrap();
        loop {
            if s.closed {
                return Acquire::Closed;
            }
            if s.in_flight == 0 || s.in_flight + n <= self.capacity {
                s.in_flight += n;
                return Acquire::Ok;
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Acquire::TimedOut;
            }
            s = self.cv.wait_timeout(s, left).unwrap().0;
        }
    }

    /// The receiver acknowledged `n` bytes.
    pub fn release(&self, n: usize) {
        let mut s = self.state.lock().unwrap();
        s.in_flight = s.in_flight.saturating_sub(n);
        self.cv.notify_all();
    }

    /// Wakes every waiter with [`Acquire::Closed`] (cancelled or disconnected).
    pub fn close(&self) {
        self.state.lock().unwrap().closed = true;
        self.cv.notify_all();
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().unwrap().closed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn limits_bytes_in_flight() {
        let w = Window::new(100);
        assert_eq!(w.acquire(60, Duration::ZERO), Acquire::Ok);
        assert_eq!(w.acquire(60, Duration::from_millis(10)), Acquire::TimedOut);
        w.release(60);
        assert_eq!(w.acquire(60, Duration::ZERO), Acquire::Ok);
        assert_eq!(w.in_flight(), 60);
    }

    #[test]
    fn oversized_request_waits_for_empty_window() {
        let w = Window::new(10);
        assert_eq!(w.acquire(50, Duration::ZERO), Acquire::Ok);
        assert_eq!(w.acquire(1, Duration::ZERO), Acquire::TimedOut);
    }

    #[test]
    fn release_and_close_wake_waiters() {
        let w = Arc::new(Window::new(10));
        w.acquire(10, Duration::ZERO);
        let w2 = w.clone();
        let t = thread::spawn(move || w2.acquire(5, Duration::from_secs(5)));
        thread::sleep(Duration::from_millis(20));
        w.release(10);
        assert_eq!(t.join().unwrap(), Acquire::Ok);
        let w2 = w.clone();
        let t = thread::spawn(move || w2.acquire(10, Duration::from_secs(5)));
        thread::sleep(Duration::from_millis(20));
        w.close();
        assert_eq!(t.join().unwrap(), Acquire::Closed);
    }
}
