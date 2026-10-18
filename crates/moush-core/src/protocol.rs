//! Wire protocol.
//!
//! Every message is encoded as `[type: u8][payload]` with fixed-size little-endian
//! fields, so decoding never needs a length prefix except for the few variable
//! messages (strings, monitor lists). Several messages can be concatenated into one
//! encrypted frame; the decoder walks the buffer message by message.
//!
//! Mouse positions are *absolute* coordinates in the client's virtual desktop. The host
//! is authoritative for where the shared cursor is, so a client only has to inject what
//! it receives, and a burst of moves can be coalesced to the latest one.
//!
//! Optional features (clipboard, file transfer, ...) travel inside [`Message::Feature`]:
//! the core only checks the envelope (channel and size); the body is encoded and
//! validated by the module that owns the channel, using the helpers in [`wire`].

use std::fmt;

pub const PROTOCOL_VERSION: u16 = 2;
pub const DEFAULT_PORT: u16 = 24800;
pub const DISCOVERY_PORT: u16 = 24801;

/// Maximum monitors a peer may report. Real machines have far fewer.
pub const MAX_MONITORS: usize = 16;
/// Maximum UTF-8 length of strings on the wire.
pub const MAX_STRING: usize = 128;
/// Largest coordinate accepted from a peer (well beyond any real virtual desktop).
pub const MAX_COORD: i32 = 1 << 20;
/// Largest body of a [`Message::Feature`]. Bulk data is sent in chunks of at most this
/// size, so one message always fits in one encrypted frame.
pub const MAX_FEATURE_BODY: usize = 32 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PeerRole {
    Host = 1,
    Client = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left = 1,
    Right = 2,
    Middle = 3,
    X1 = 4,
    X2 = 5,
}

impl MouseButton {
    pub const ALL: [MouseButton; 5] = [Self::Left, Self::Right, Self::Middle, Self::X1, Self::X2];

    pub fn bit(self) -> u8 {
        1 << (self as u8)
    }

    fn from_u8(v: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|b| *b as u8 == v)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WheelAxis {
    Vertical = 0,
    Horizontal = 1,
}

/// Key flags carried with [`Message::Key`].
pub mod key_flags {
    pub const DOWN: u8 = 0x01;
    pub const EXTENDED: u8 = 0x02;
    pub const ALL: u8 = DOWN | EXTENDED;
}

/// A monitor as reported by a peer, in that peer's physical-pixel virtual desktop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MonitorDesc {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub dpi: u16,
    pub primary: bool,
    pub name: String,
}

/// `Debug` is implemented by hand: it never prints the contents of a feature body
/// (clipboard text, file data), only its channel and length, so logging a message for
/// diagnostics cannot leak user data.
#[derive(Clone, PartialEq)]
pub enum Message {
    /// First message after the secure handshake, both directions. `trusts_peer` tells
    /// the other side whether the sender already trusts its key (no pairing prompt needed).
    Hello { version: u16, role: PeerRole, name: String, trusts_peer: bool },
    /// Pairing decision of the sender's user (sent immediately when the peer is trusted).
    Authorize { accept: bool },
    /// Orderly shutdown with a human-readable reason.
    Bye { reason: String },
    /// Client -> host: the client's monitors. Sent on connect and when displays change.
    Screens { monitors: Vec<MonitorDesc> },
    /// Host -> client: shared cursor enters this client at the given position.
    Enter { x: i32, y: i32 },
    /// Host -> client: shared cursor left; the client must release everything it holds.
    Leave,
    MouseMove { x: i32, y: i32 },
    MouseButton { button: MouseButton, down: bool },
    Wheel { axis: WheelAxis, delta: i16 },
    Key { vk: u16, scan: u16, flags: u8 },
    /// Release every key and button the receiver has injected.
    ReleaseAll,
    Ping { id: u32, t_us: u64 },
    Pong { id: u32, t_us: u64 },
    /// Both directions, after authorization and whenever local settings change: the
    /// optional features (`features::cap` bits) the sender currently allows with this peer.
    Capabilities { features: u32 },
    /// Both directions: traffic of an optional feature. `channel` is one of
    /// `features::channel`; the body is opaque here and at most [`MAX_FEATURE_BODY`] bytes.
    Feature { channel: u8, body: Vec<u8> },
}

impl fmt::Debug for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Message::Hello { version, role, name, trusts_peer } => f
                .debug_struct("Hello")
                .field("version", version)
                .field("role", role)
                .field("name", name)
                .field("trusts_peer", trusts_peer)
                .finish(),
            Message::Authorize { accept } => f.debug_struct("Authorize").field("accept", accept).finish(),
            Message::Bye { reason } => f.debug_struct("Bye").field("reason", reason).finish(),
            Message::Screens { monitors } => f.debug_struct("Screens").field("monitors", monitors).finish(),
            Message::Enter { x, y } => f.debug_struct("Enter").field("x", x).field("y", y).finish(),
            Message::Leave => f.write_str("Leave"),
            Message::MouseMove { x, y } => f.debug_struct("MouseMove").field("x", x).field("y", y).finish(),
            Message::MouseButton { button, down } => {
                f.debug_struct("MouseButton").field("button", button).field("down", down).finish()
            }
            Message::Wheel { axis, delta } => f.debug_struct("Wheel").field("axis", axis).field("delta", delta).finish(),
            Message::Key { vk, scan, flags } => {
                f.debug_struct("Key").field("vk", vk).field("scan", scan).field("flags", flags).finish()
            }
            Message::ReleaseAll => f.write_str("ReleaseAll"),
            Message::Ping { id, t_us } => f.debug_struct("Ping").field("id", id).field("t_us", t_us).finish(),
            Message::Pong { id, t_us } => f.debug_struct("Pong").field("id", id).field("t_us", t_us).finish(),
            Message::Capabilities { features } => {
                f.debug_struct("Capabilities").field("features", &format_args!("{features:#x}")).finish()
            }
            Message::Feature { channel, body } => f
                .debug_struct("Feature")
                .field("channel", channel)
                .field("body", &format_args!("<{} bytes>", body.len()))
                .finish(),
        }
    }
}

impl Message {
    /// True for messages that only carry position and can be superseded by a later one.
    pub fn is_coalescable_move(&self) -> bool {
        matches!(self, Message::MouseMove { .. })
    }

    /// True for low-priority bulk traffic that must never delay input.
    pub fn is_bulk(&self) -> bool {
        matches!(self, Message::Feature { .. })
    }
}

mod tag {
    pub const HELLO: u8 = 1;
    pub const AUTHORIZE: u8 = 2;
    pub const BYE: u8 = 3;
    pub const SCREENS: u8 = 4;
    pub const ENTER: u8 = 10;
    pub const LEAVE: u8 = 11;
    pub const MOUSE_MOVE: u8 = 12;
    pub const MOUSE_BUTTON: u8 = 13;
    pub const WHEEL: u8 = 14;
    pub const KEY: u8 = 15;
    pub const RELEASE_ALL: u8 = 16;
    pub const PING: u8 = 20;
    pub const PONG: u8 = 21;
    pub const CAPABILITIES: u8 = 30;
    pub const FEATURE: u8 = 31;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    Truncated,
    UnknownType(u8),
    Invalid(&'static str),
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Truncated => write!(f, "truncated message"),
            DecodeError::UnknownType(t) => write!(f, "unknown message type {t}"),
            DecodeError::Invalid(what) => write!(f, "invalid message: {what}"),
        }
    }
}

impl std::error::Error for DecodeError {}

/// Appends the encoding of `msg` to `out`.
pub fn encode(msg: &Message, out: &mut Vec<u8>) {
    match msg {
        Message::Hello { version, role, name, trusts_peer } => {
            out.push(tag::HELLO);
            out.extend_from_slice(&version.to_le_bytes());
            out.push(*role as u8);
            put_str(out, name);
            out.push(*trusts_peer as u8);
        }
        Message::Authorize { accept } => {
            out.push(tag::AUTHORIZE);
            out.push(*accept as u8);
        }
        Message::Bye { reason } => {
            out.push(tag::BYE);
            put_str(out, reason);
        }
        Message::Screens { monitors } => {
            out.push(tag::SCREENS);
            let n = monitors.len().min(MAX_MONITORS);
            out.push(n as u8);
            for m in &monitors[..n] {
                out.extend_from_slice(&m.x.to_le_bytes());
                out.extend_from_slice(&m.y.to_le_bytes());
                out.extend_from_slice(&m.width.to_le_bytes());
                out.extend_from_slice(&m.height.to_le_bytes());
                out.extend_from_slice(&m.dpi.to_le_bytes());
                out.push(m.primary as u8);
                put_str(out, &m.name);
            }
        }
        Message::Enter { x, y } => {
            out.push(tag::ENTER);
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
        Message::Leave => out.push(tag::LEAVE),
        Message::MouseMove { x, y } => {
            out.push(tag::MOUSE_MOVE);
            out.extend_from_slice(&x.to_le_bytes());
            out.extend_from_slice(&y.to_le_bytes());
        }
        Message::MouseButton { button, down } => {
            out.push(tag::MOUSE_BUTTON);
            out.push(*button as u8);
            out.push(*down as u8);
        }
        Message::Wheel { axis, delta } => {
            out.push(tag::WHEEL);
            out.push(*axis as u8);
            out.extend_from_slice(&delta.to_le_bytes());
        }
        Message::Key { vk, scan, flags } => {
            out.push(tag::KEY);
            out.extend_from_slice(&vk.to_le_bytes());
            out.extend_from_slice(&scan.to_le_bytes());
            out.push(*flags);
        }
        Message::ReleaseAll => out.push(tag::RELEASE_ALL),
        Message::Ping { id, t_us } => {
            out.push(tag::PING);
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&t_us.to_le_bytes());
        }
        Message::Pong { id, t_us } => {
            out.push(tag::PONG);
            out.extend_from_slice(&id.to_le_bytes());
            out.extend_from_slice(&t_us.to_le_bytes());
        }
        Message::Capabilities { features } => {
            out.push(tag::CAPABILITIES);
            out.extend_from_slice(&features.to_le_bytes());
        }
        Message::Feature { channel, body } => {
            debug_assert!(body.len() <= MAX_FEATURE_BODY, "feature body too large");
            out.push(tag::FEATURE);
            out.push(*channel);
            out.extend_from_slice(&(body.len() as u32).to_le_bytes());
            out.extend_from_slice(body);
        }
    }
}

/// Upper bound of the encoded size of `msg` (used to split batches into frames).
pub fn encoded_len(msg: &Message) -> usize {
    match msg {
        Message::Feature { body, .. } => 6 + body.len(),
        Message::Screens { monitors } => 2 + monitors.len() * (20 + 1 + MAX_STRING),
        Message::Hello { .. } | Message::Bye { .. } => 8 + MAX_STRING,
        _ => 16,
    }
}

/// Decodes every message in `buf`. Fails on the first malformed message; a peer that
/// sends malformed data is treated as hostile and disconnected by the caller.
pub fn decode_all(buf: &[u8], out: &mut Vec<Message>) -> Result<(), DecodeError> {
    let mut r = Reader { buf, pos: 0 };
    while r.pos < buf.len() {
        out.push(decode_one(&mut r)?);
    }
    Ok(())
}

fn decode_one(r: &mut Reader<'_>) -> Result<Message, DecodeError> {
    let t = r.u8()?;
    let msg = match t {
        tag::HELLO => {
            let version = r.u16()?;
            let role = match r.u8()? {
                1 => PeerRole::Host,
                2 => PeerRole::Client,
                _ => return Err(DecodeError::Invalid("role")),
            };
            Message::Hello { version, role, name: r.string()?, trusts_peer: r.bool()? }
        }
        tag::AUTHORIZE => Message::Authorize { accept: r.bool()? },
        tag::BYE => Message::Bye { reason: r.string()? },
        tag::SCREENS => {
            let n = r.u8()? as usize;
            if n == 0 || n > MAX_MONITORS {
                return Err(DecodeError::Invalid("monitor count"));
            }
            let mut monitors = Vec::with_capacity(n);
            for _ in 0..n {
                let m = MonitorDesc {
                    x: r.coord()?,
                    y: r.coord()?,
                    width: r.i32()?,
                    height: r.i32()?,
                    dpi: r.u16()?,
                    primary: r.bool()?,
                    name: r.string()?,
                };
                if !(1..=MAX_COORD).contains(&m.width) || !(1..=MAX_COORD).contains(&m.height) {
                    return Err(DecodeError::Invalid("monitor size"));
                }
                if !(48..=960).contains(&m.dpi) {
                    return Err(DecodeError::Invalid("monitor dpi"));
                }
                monitors.push(m);
            }
            Message::Screens { monitors }
        }
        tag::ENTER => Message::Enter { x: r.coord()?, y: r.coord()? },
        tag::LEAVE => Message::Leave,
        tag::MOUSE_MOVE => Message::MouseMove { x: r.coord()?, y: r.coord()? },
        tag::MOUSE_BUTTON => {
            let button = MouseButton::from_u8(r.u8()?).ok_or(DecodeError::Invalid("button"))?;
            Message::MouseButton { button, down: r.bool()? }
        }
        tag::WHEEL => {
            let axis = match r.u8()? {
                0 => WheelAxis::Vertical,
                1 => WheelAxis::Horizontal,
                _ => return Err(DecodeError::Invalid("wheel axis")),
            };
            Message::Wheel { axis, delta: r.i16()? }
        }
        tag::KEY => {
            let vk = r.u16()?;
            let scan = r.u16()?;
            let flags = r.u8()?;
            if vk == 0 || vk > 0xFE || flags & !key_flags::ALL != 0 {
                return Err(DecodeError::Invalid("key"));
            }
            Message::Key { vk, scan, flags }
        }
        tag::RELEASE_ALL => Message::ReleaseAll,
        tag::PING => Message::Ping { id: r.u32()?, t_us: r.u64()? },
        tag::PONG => Message::Pong { id: r.u32()?, t_us: r.u64()? },
        tag::CAPABILITIES => Message::Capabilities { features: r.u32()? },
        tag::FEATURE => {
            let channel = r.u8()?;
            if channel == 0 {
                return Err(DecodeError::Invalid("feature channel"));
            }
            let n = r.u32()? as usize;
            if n > MAX_FEATURE_BODY {
                return Err(DecodeError::Invalid("feature body too large"));
            }
            Message::Feature { channel, body: r.take(n)?.to_vec() }
        }
        other => return Err(DecodeError::UnknownType(other)),
    };
    Ok(msg)
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    let s = truncate_utf8(s, MAX_STRING);
    out.push(s.len() as u8);
    out.extend_from_slice(s.as_bytes());
}

/// Encoding helpers for feature bodies: little-endian fixed fields, length-prefixed
/// strings and byte runs. Decoding ([`Reader`]) is bounds-checked and never panics.
pub mod wire {
    pub use super::{DecodeError, Reader};

    pub fn put_u8(out: &mut Vec<u8>, v: u8) {
        out.push(v);
    }
    pub fn put_bool(out: &mut Vec<u8>, v: bool) {
        out.push(v as u8);
    }
    pub fn put_u16(out: &mut Vec<u8>, v: u16) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn put_u32(out: &mut Vec<u8>, v: u32) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    pub fn put_u64(out: &mut Vec<u8>, v: u64) {
        out.extend_from_slice(&v.to_le_bytes());
    }
    /// Short string (at most 255 bytes, truncated on a character boundary).
    pub fn put_str8(out: &mut Vec<u8>, s: &str) {
        let s = super::truncate_utf8(s, 255);
        out.push(s.len() as u8);
        out.extend_from_slice(s.as_bytes());
    }
    /// Byte run with a u32 length prefix. The caller keeps the whole body within
    /// `MAX_FEATURE_BODY`.
    pub fn put_bytes(out: &mut Vec<u8>, b: &[u8]) {
        out.extend_from_slice(&(b.len() as u32).to_le_bytes());
        out.extend_from_slice(b);
    }
}

/// Truncates to at most `max` bytes on a character boundary.
pub fn truncate_utf8(s: &str, max: usize) -> &str {
    if s.len() <= max {
        return s;
    }
    let mut end = max;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

/// Bounds-checked little-endian reader over a message or feature body.
pub struct Reader<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }
    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.buf.len() - self.pos
    }
    /// Fails unless the whole buffer was consumed (trailing garbage is an error).
    pub fn finish(&self) -> Result<(), DecodeError> {
        if self.pos == self.buf.len() { Ok(()) } else { Err(DecodeError::Invalid("trailing bytes")) }
    }
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], DecodeError> {
        let end = self.pos.checked_add(n).ok_or(DecodeError::Truncated)?;
        let s = self.buf.get(self.pos..end).ok_or(DecodeError::Truncated)?;
        self.pos = end;
        Ok(s)
    }
    pub fn u8(&mut self) -> Result<u8, DecodeError> {
        Ok(self.take(1)?[0])
    }
    pub fn bool(&mut self) -> Result<bool, DecodeError> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::Invalid("bool")),
        }
    }
    pub fn u16(&mut self) -> Result<u16, DecodeError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn i16(&mut self) -> Result<i16, DecodeError> {
        Ok(i16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    pub fn u32(&mut self) -> Result<u32, DecodeError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn i32(&mut self) -> Result<i32, DecodeError> {
        Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    pub fn u64(&mut self) -> Result<u64, DecodeError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }
    pub fn coord(&mut self) -> Result<i32, DecodeError> {
        let v = self.i32()?;
        if v.abs() > MAX_COORD {
            return Err(DecodeError::Invalid("coordinate out of range"));
        }
        Ok(v)
    }
    pub fn string(&mut self) -> Result<String, DecodeError> {
        let n = self.u8()? as usize;
        if n > MAX_STRING {
            return Err(DecodeError::Invalid("string length"));
        }
        let bytes = self.take(n)?;
        let s = std::str::from_utf8(bytes).map_err(|_| DecodeError::Invalid("utf-8"))?;
        if s.chars().any(|c| c.is_control()) {
            return Err(DecodeError::Invalid("control character in string"));
        }
        Ok(s.to_owned())
    }
    /// String written by [`wire::put_str8`]; control characters are rejected.
    pub fn str8(&mut self) -> Result<String, DecodeError> {
        let n = self.u8()? as usize;
        let s = std::str::from_utf8(self.take(n)?).map_err(|_| DecodeError::Invalid("utf-8"))?;
        if s.chars().any(|c| c.is_control()) {
            return Err(DecodeError::Invalid("control character in string"));
        }
        Ok(s.to_owned())
    }
    /// Byte run written by [`wire::put_bytes`], at most `max` bytes.
    pub fn bytes(&mut self, max: usize) -> Result<&'a [u8], DecodeError> {
        let n = self.u32()? as usize;
        if n > max {
            return Err(DecodeError::Invalid("byte run too long"));
        }
        self.take(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msgs: &[Message]) {
        let mut buf = Vec::new();
        for m in msgs {
            encode(m, &mut buf);
        }
        let mut out = Vec::new();
        decode_all(&buf, &mut out).expect("decode");
        assert_eq!(out, msgs);
    }

    #[test]
    fn roundtrips_every_message() {
        roundtrip(&[
            Message::Hello { version: 1, role: PeerRole::Client, name: "Laptop é".into(), trusts_peer: true },
            Message::Authorize { accept: true },
            Message::Bye { reason: "bye".into() },
            Message::Screens {
                monitors: vec![MonitorDesc {
                    x: -1920,
                    y: 0,
                    width: 1920,
                    height: 1200,
                    dpi: 144,
                    primary: true,
                    name: "Built-in".into(),
                }],
            },
            Message::Enter { x: 10, y: -20 },
            Message::Leave,
            Message::MouseMove { x: 1919, y: 1199 },
            Message::MouseButton { button: MouseButton::X2, down: true },
            Message::Wheel { axis: WheelAxis::Horizontal, delta: -120 },
            Message::Key { vk: 0x41, scan: 0x1e, flags: key_flags::DOWN | key_flags::EXTENDED },
            Message::ReleaseAll,
            Message::Ping { id: 7, t_us: u64::MAX },
            Message::Pong { id: 7, t_us: 1 },
            Message::Capabilities { features: 0b101 },
            Message::Feature { channel: 1, body: vec![1, 2, 3] },
            Message::Feature { channel: 2, body: vec![0; MAX_FEATURE_BODY] },
        ]);
    }

    #[test]
    fn rejects_bad_feature_envelopes() {
        let mut buf = vec![tag::FEATURE, 0];
        buf.extend_from_slice(&0u32.to_le_bytes());
        assert!(decode_all(&buf, &mut Vec::new()).is_err(), "channel 0 is reserved");
        let mut buf = vec![tag::FEATURE, 1];
        buf.extend_from_slice(&((MAX_FEATURE_BODY + 1) as u32).to_le_bytes());
        buf.resize(buf.len() + MAX_FEATURE_BODY + 1, 0);
        assert!(decode_all(&buf, &mut Vec::new()).is_err(), "oversized body");
        let mut buf = vec![tag::FEATURE, 1];
        buf.extend_from_slice(&10u32.to_le_bytes());
        assert_eq!(decode_all(&buf, &mut Vec::new()), Err(DecodeError::Truncated));
    }

    #[test]
    fn wire_helpers_roundtrip() {
        let mut b = Vec::new();
        wire::put_u8(&mut b, 9);
        wire::put_bool(&mut b, true);
        wire::put_u16(&mut b, 513);
        wire::put_u32(&mut b, 70000);
        wire::put_u64(&mut b, u64::MAX - 1);
        wire::put_str8(&mut b, "na\u{ef}ve \u{6587}\u{5b57}");
        wire::put_bytes(&mut b, &[5, 6]);
        let mut r = Reader::new(&b);
        assert_eq!(r.u8().unwrap(), 9);
        assert!(r.bool().unwrap());
        assert_eq!(r.u16().unwrap(), 513);
        assert_eq!(r.u32().unwrap(), 70000);
        assert_eq!(r.u64().unwrap(), u64::MAX - 1);
        assert_eq!(r.str8().unwrap(), "na\u{ef}ve \u{6587}\u{5b57}");
        assert_eq!(r.bytes(2).unwrap(), &[5, 6]);
        r.finish().unwrap();
        let mut r = Reader::new(&b);
        r.take(b.len() - 6).unwrap();
        assert!(r.bytes(1).is_err(), "length above max");
    }

    #[test]
    fn rejects_truncated_and_unknown() {
        let mut buf = Vec::new();
        encode(&Message::MouseMove { x: 1, y: 2 }, &mut buf);
        buf.pop();
        assert_eq!(decode_all(&buf, &mut Vec::new()), Err(DecodeError::Truncated));
        assert_eq!(decode_all(&[0xEE], &mut Vec::new()), Err(DecodeError::UnknownType(0xEE)));
    }

    #[test]
    fn rejects_hostile_values() {
        // Out-of-range coordinate.
        let mut buf = vec![tag::MOUSE_MOVE];
        buf.extend_from_slice(&(MAX_COORD + 1).to_le_bytes());
        buf.extend_from_slice(&0i32.to_le_bytes());
        assert!(decode_all(&buf, &mut Vec::new()).is_err());
        // Unknown key flag bits.
        let buf = [tag::KEY, 0x41, 0, 0x1e, 0, 0x80];
        assert!(decode_all(&buf, &mut Vec::new()).is_err());
        // Bad bool.
        assert!(decode_all(&[tag::AUTHORIZE, 2], &mut Vec::new()).is_err());
        // Control characters in strings.
        assert!(decode_all(&[tag::BYE, 1, b'\n'], &mut Vec::new()).is_err());
    }

    #[test]
    fn long_strings_are_truncated_on_char_boundary() {
        let name = "é".repeat(100); // 200 bytes
        let mut buf = Vec::new();
        encode(&Message::Bye { reason: name }, &mut buf);
        let mut out = Vec::new();
        decode_all(&buf, &mut out).unwrap();
        let Message::Bye { reason } = &out[0] else { panic!() };
        assert_eq!(reason.len(), MAX_STRING);
    }
}
