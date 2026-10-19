//! Robustness of the wire decoder and the size bound used for frame splitting.
//!
//! Deterministic pseudo-random fuzzing (no external crates): random buffers, mutated valid
//! encodings and hostile feature envelopes must never panic, invalid input must be
//! rejected, and whatever decodes must re-encode to exactly the same bytes (the encoding
//! is canonical, so a decoder bug that "accepts and reinterprets" shows up as a mismatch).

use moush_core::protocol::{
    DecodeError, MAX_COORD, MAX_FEATURE_BODY, MAX_MONITORS, MAX_STRING, Message, MonitorDesc, MouseButton, PeerRole,
    Reader, WheelAxis, decode_all, encode, encoded_len, key_flags, wire,
};

/// xorshift64*: small, fast and reproducible.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    fn byte(&mut self) -> u8 {
        self.next() as u8
    }
    fn bool(&mut self) -> bool {
        self.next() & 1 == 1
    }
    fn i32_in(&mut self, lo: i32, hi: i32) -> i32 {
        lo + self.below((hi as i64 - lo as i64 + 1) as u64) as i32
    }
    fn string(&mut self, max_chars: usize) -> String {
        const PARTS: [&str; 8] = ["a", "Z", " ", "é", "文", "😀", "-", "Laptop"];
        let n = self.below(max_chars as u64 + 1) as usize;
        (0..n).map(|_| PARTS[self.below(PARTS.len() as u64) as usize]).collect()
    }
}

/// A random *valid* message of any variant (strings may exceed the wire limit; they are
/// truncated on encode).
fn random_message(r: &mut Rng) -> Message {
    let coord = |r: &mut Rng| r.i32_in(-MAX_COORD, MAX_COORD);
    match r.below(15) {
        0 => Message::Hello {
            version: r.next() as u16,
            role: if r.bool() { PeerRole::Host } else { PeerRole::Client },
            name: r.string(80),
            trusts_peer: r.bool(),
        },
        1 => Message::Authorize { accept: r.bool() },
        2 => Message::Bye { reason: r.string(200) },
        3 => {
            let n = 1 + r.below(MAX_MONITORS as u64) as usize;
            Message::Screens {
                monitors: (0..n)
                    .map(|_| MonitorDesc {
                        x: coord(r),
                        y: coord(r),
                        width: r.i32_in(1, MAX_COORD),
                        height: r.i32_in(1, MAX_COORD),
                        dpi: r.i32_in(48, 960) as u16,
                        primary: r.bool(),
                        name: r.string(60),
                    })
                    .collect(),
            }
        }
        4 => Message::Enter { x: coord(r), y: coord(r) },
        5 => Message::Leave,
        6 => Message::MouseMove { x: coord(r), y: coord(r) },
        7 => Message::MouseButton { button: MouseButton::ALL[r.below(5) as usize], down: r.bool() },
        8 => Message::Wheel {
            axis: if r.bool() { WheelAxis::Vertical } else { WheelAxis::Horizontal },
            delta: r.next() as i16,
        },
        9 => Message::Key { vk: 1 + r.below(0xFE) as u16, scan: r.next() as u16, flags: r.byte() & key_flags::ALL },
        10 => Message::ReleaseAll,
        11 => Message::Ping { id: r.next() as u32, t_us: r.next() },
        12 => Message::Pong { id: r.next() as u32, t_us: r.next() },
        13 => Message::Capabilities { features: r.next() as u32 },
        _ => {
            let len = match r.below(4) {
                0 => 0,
                1 => MAX_FEATURE_BODY,
                _ => r.below(4096) as usize,
            };
            Message::Feature { channel: 1 + r.below(255) as u8, body: (0..len).map(|_| r.byte()).collect() }
        }
    }
}

fn encoded(msgs: &[Message]) -> Vec<u8> {
    let mut buf = Vec::new();
    for m in msgs {
        encode(m, &mut buf);
    }
    buf
}

/// Decodes `buf`; on success the messages must re-encode to the identical bytes.
fn check(buf: &[u8]) -> Result<Vec<Message>, DecodeError> {
    let mut out = Vec::new();
    decode_all(buf, &mut out)?;
    assert_eq!(encoded(&out), buf, "decoded messages re-encode differently: {out:?}");
    Ok(out)
}

#[test]
fn random_buffers_never_panic() {
    let mut r = Rng(0x9E37_79B9_7F4A_7C15);
    let mut accepted = 0;
    for i in 0..200_000 {
        let len = r.below(if i % 100 == 0 { 70_000 } else { 64 }) as usize;
        let mut buf: Vec<u8> = (0..len).map(|_| r.byte()).collect();
        // Bias the first byte towards real tags so the decoder gets past the type check.
        if let Some(b) = buf.first_mut()
            && r.bool()
        {
            *b = [1, 2, 3, 4, 10, 11, 12, 13, 14, 15, 16, 20, 21, 30, 31][r.below(15) as usize];
        }
        if check(&buf).is_ok() {
            accepted += 1;
        }
    }
    // Some short random buffers are valid by chance (e.g. a lone Leave); most are not.
    assert!(accepted > 0 && accepted < 200_000, "accepted {accepted}");
}

#[test]
fn mutated_valid_frames_never_panic() {
    let mut r = Rng(42);
    for _ in 0..20_000 {
        let n = 1 + r.below(6) as usize;
        let msgs: Vec<Message> = (0..n).map(|_| random_message(&mut r)).collect();
        let mut buf = encoded(&msgs);
        // The unmodified encoding always decodes.
        check(&buf).expect("valid encoding decodes");
        for _ in 0..1 + r.below(4) {
            if buf.is_empty() {
                break;
            }
            let at = r.below(buf.len() as u64) as usize;
            match r.below(6) {
                0 => buf[at] ^= 1 << r.below(8),
                1 => buf[at] = r.byte(),
                2 => buf.truncate(at),
                3 => buf.insert(at, r.byte()),
                4 => {
                    buf.remove(at);
                }
                // Extreme values where length fields usually live.
                _ => buf[at] = [0, 0xFF, 0x80, 0x7F][r.below(4) as usize],
            }
        }
        let _ = check(&buf);
    }
}

#[test]
fn hostile_feature_envelopes_are_rejected() {
    let feature = |channel: u8, len: u32, actual: usize| {
        let mut b = vec![31, channel];
        b.extend_from_slice(&len.to_le_bytes());
        b.resize(b.len() + actual, 0xAB);
        b
    };
    let mut out = Vec::new();
    // Reserved channel.
    assert!(decode_all(&feature(0, 0, 0), &mut out).is_err());
    // Length over the limit, with and without the bytes present.
    assert!(decode_all(&feature(1, MAX_FEATURE_BODY as u32 + 1, MAX_FEATURE_BODY + 1), &mut out).is_err());
    assert!(decode_all(&feature(1, u32::MAX, 16), &mut out).is_err());
    // Length larger than what follows.
    assert_eq!(decode_all(&feature(1, 100, 99), &mut out), Err(DecodeError::Truncated));
    // Header cut short.
    assert_eq!(decode_all(&[31, 1, 0, 0], &mut out), Err(DecodeError::Truncated));
    assert_eq!(decode_all(&[31], &mut out), Err(DecodeError::Truncated));
    // Exactly at the limit is fine; anything after must be a whole message.
    out.clear();
    decode_all(&feature(255, MAX_FEATURE_BODY as u32, MAX_FEATURE_BODY), &mut out).unwrap();
    assert_eq!(out.len(), 1);
    let mut b = feature(2, 3, 3);
    b.push(31);
    assert_eq!(decode_all(&b, &mut Vec::new()), Err(DecodeError::Truncated));
    // Every body length from 0 to 300 around a valid message round-trips.
    for len in 0..300usize {
        let msgs = [Message::Leave, Message::Feature { channel: 7, body: vec![len as u8; len] }, Message::ReleaseAll];
        assert_eq!(check(&encoded(&msgs)).unwrap(), msgs);
    }
}

#[test]
fn reader_never_panics_on_random_operations() {
    let mut r = Rng(7);
    for _ in 0..50_000 {
        let len = r.below(48) as usize;
        let buf: Vec<u8> = (0..len).map(|_| r.byte()).collect();
        let mut rd = Reader::new(&buf);
        for _ in 0..8 {
            let before = rd.remaining();
            let ok = match r.below(10) {
                0 => rd.u8().is_ok(),
                1 => rd.bool().is_ok(),
                2 => rd.u16().is_ok(),
                3 => rd.u32().is_ok(),
                4 => rd.u64().is_ok(),
                5 => rd.str8().is_ok(),
                6 => rd.bytes(r.below(64) as usize).is_ok(),
                7 => rd.take(r.below(64) as usize).is_ok(),
                8 => rd.take(usize::MAX).is_ok(),
                _ => rd.finish().is_ok(),
            };
            assert!(rd.remaining() <= before, "reader never moves backwards");
            if !ok {
                // Truncation leaves the position untouched for fixed reads; either way,
                // further reads must keep failing or succeed without panicking.
                let _ = rd.u8();
            }
        }
    }
}

#[test]
fn reader_rejects_bad_strings_and_runs() {
    let mut b = Vec::new();
    wire::put_bytes(&mut b, &[1, 2, 3]);
    assert!(Reader::new(&b).bytes(2).is_err(), "run longer than the caller's max");
    let mut rd = Reader::new(&b);
    assert_eq!(rd.bytes(3).unwrap(), &[1, 2, 3]);
    rd.finish().unwrap();
    let mut bad = vec![3, b'a', 0xFF, b'b'];
    assert!(Reader::new(&bad).str8().is_err(), "invalid UTF-8");
    bad = vec![2, b'a', b'\n'];
    assert!(Reader::new(&bad).str8().is_err(), "control character");
    let mut rd = Reader::new(&[1, 0]);
    rd.u8().unwrap();
    assert!(rd.finish().is_err(), "trailing bytes rejected by finish");
}

#[test]
fn encoded_len_bounds_every_variant() {
    let worst_name = "😀".repeat(200); // 800 bytes, truncated to MAX_STRING on encode
    let monitor = |name: &str| MonitorDesc {
        x: -MAX_COORD,
        y: MAX_COORD,
        width: MAX_COORD,
        height: 1,
        dpi: 960,
        primary: true,
        name: name.into(),
    };
    let mut cases = vec![
        Message::Hello { version: u16::MAX, role: PeerRole::Host, name: worst_name.clone(), trusts_peer: true },
        Message::Hello { version: 0, role: PeerRole::Client, name: String::new(), trusts_peer: false },
        Message::Authorize { accept: true },
        Message::Bye { reason: worst_name.clone() },
        Message::Bye { reason: "é".repeat(MAX_STRING) },
        Message::Screens { monitors: vec![monitor(&worst_name); MAX_MONITORS] },
        // More monitors than the wire allows: encode keeps MAX_MONITORS.
        Message::Screens { monitors: vec![monitor(&worst_name); MAX_MONITORS + 9] },
        Message::Screens { monitors: vec![monitor("")] },
        Message::Enter { x: i32::MIN, y: i32::MAX },
        Message::Leave,
        Message::MouseMove { x: -1, y: 1 },
        Message::MouseButton { button: MouseButton::X2, down: true },
        Message::Wheel { axis: WheelAxis::Horizontal, delta: i16::MIN },
        Message::Key { vk: 0xFE, scan: u16::MAX, flags: key_flags::ALL },
        Message::ReleaseAll,
        Message::Ping { id: u32::MAX, t_us: u64::MAX },
        Message::Pong { id: 0, t_us: 0 },
        Message::Capabilities { features: u32::MAX },
        Message::Feature { channel: 255, body: Vec::new() },
        Message::Feature { channel: 1, body: vec![0; MAX_FEATURE_BODY] },
    ];
    let mut r = Rng(1234);
    cases.extend((0..20_000).map(|_| random_message(&mut r)));
    for m in &cases {
        let mut buf = Vec::new();
        encode(m, &mut buf);
        assert!(encoded_len(m) >= buf.len(), "encoded_len {} < actual {} for {m:?}", encoded_len(m), buf.len());
    }
}

#[test]
fn debug_output_never_contains_feature_bodies() {
    let secret = b"hunter2 clipboard secret".to_vec();
    let m = Message::Feature { channel: 1, body: secret.clone() };
    let s = format!("{m:?}");
    assert!(!s.contains("hunter2") && !s.contains("104, 117"), "{s}");
    assert!(s.contains("24 bytes"), "{s}");
    assert_eq!(format!("{:?}", Message::Leave), "Leave");
    assert!(format!("{:?}", Message::MouseMove { x: 1, y: 2 }).contains("x: 1"));
}
