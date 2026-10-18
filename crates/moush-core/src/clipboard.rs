//! Clipboard synchronization and manual text sharing, platform independent: the clip
//! model, the wire format of [`channel::CLIPBOARD`](crate::features::channel::CLIPBOARD)
//! and the sync state machine that decides what to apply and where to send it. The
//! Windows clipboard access and the service that moves clips between peers live in
//! `moush-engine` (`clipboard.rs`, `win/clipboard.rs`).
//!
//! # Wire format
//! Every feature body starts with a kind byte ([`kind`]). A clip travels as one `OFFER`
//! (version, content hash, sizes) followed by `CHUNK`s that carry its serialized payload
//! ([`encode_payload`]). The receiver acknowledges consumed chunk bytes with `ACK` (flow
//! control, see [`crate::flow::Window`]) and may refuse a clip at any time with `REJECT`;
//! the sender abandons an unfinished clip with `ABORT` when a newer one supersedes it.
//! At most one clip is in flight per peer and direction. Manually shared text travels
//! as `SHARE` parts (at most [`MAX_SHARE_TEXT`] bytes, never placed on the clipboard
//! automatically). `CLOCK` advances the peer's logical clock when a connection starts.
//!
//! # Ordering and convergence
//! Every clip carries a [`Version`]: a hybrid logical timestamp plus the id of the device
//! it was copied on. Versions are totally ordered by `(stamp, origin)`: the larger stamp
//! wins and equal stamps are broken by the byte-wise larger origin device id, so every
//! device orders two clips the same way. A local copy gets `stamp = max(now_ms, clock + 1)`
//! and every version seen from a peer advances `clock`, so a copy made after a device
//! has seen clip X always orders after X; only truly concurrent copies are decided by
//! the tie-break. A device applies a received clip only if its version is greater than
//! the version of what it holds; when it refuses a stale clip it sends its own (newer)
//! clip back to that peer, so both ends end up with the same content.
//!
//! # Loop prevention and dedupe
//! 1. A write made by the sync itself is recognized by the clipboard sequence number
//!    recorded right after writing, and never treated as a local copy.
//! 2. A local change whose content hash equals what the sync already holds is ignored.
//! 3. A relayed clip is never sent to the peer it came from nor to its origin device.
//! 4. A clip whose hash equals the last one exchanged with a peer is not sent to it again.

use std::collections::HashMap;

use blake2::{Blake2s256, Digest};

use crate::features::ClipboardSettings;
use crate::protocol::wire::{put_bool, put_bytes, put_str8, put_u8, put_u32, put_u64};
use crate::protocol::{DecodeError, MAX_FEATURE_BODY, Reader};

/// Largest clip (sum of its uncompressed formats) ever accepted, whatever the settings.
pub const MAX_CLIP_BYTES: usize = 64 * 1024 * 1024;
/// Largest manually shared text in UTF-8 bytes.
pub const MAX_SHARE_TEXT: usize = 64 * 1024;
/// Largest data run in one `CHUNK` or `SHARE` body (leaves room for the header).
pub const MAX_CHUNK_DATA: usize = MAX_FEATURE_BODY - 32;
/// Serialization overhead of a payload over its formats' bytes (count + per-format headers).
pub const MAX_PAYLOAD_OVERHEAD: usize = 1 + FORMAT_COUNT * 10;
/// Longest accepted device id in a version.
const MAX_ORIGIN: usize = 64;
/// Formats of at least this size are offered to the compressor.
const COMPRESS_MIN: usize = 1024;
/// Characters of text kept as a preview.
pub const PREVIEW_CHARS: usize = 300;
/// Longest text recognised as a link.
const MAX_LINK: usize = 2048;

/// Message kinds (first body byte).
pub mod kind {
    /// Sender -> receiver: a clip follows.
    pub const OFFER: u8 = 1;
    /// Sender -> receiver: a run of the clip's payload.
    pub const CHUNK: u8 = 2;
    /// Receiver -> sender: payload bytes consumed (flow control).
    pub const ACK: u8 = 3;
    /// Sender -> receiver: the sender abandoned this clip (superseded or cancelled).
    pub const ABORT: u8 = 4;
    /// Receiver -> sender: the receiver refuses this clip.
    pub const REJECT: u8 = 5;
    /// A part of manually shared text.
    pub const SHARE: u8 = 6;
    /// The sender's logical clock (sent when the feature becomes available).
    pub const CLOCK: u8 = 7;
}

/// Why a receiver refused a clip (diagnostics only).
pub mod reject {
    pub const DISABLED: u8 = 1;
    pub const TOO_LARGE: u8 = 2;
    pub const STALE: u8 = 3;
    pub const DUPLICATE: u8 = 4;
    pub const INVALID: u8 = 5;
    pub const BUSY: u8 = 6;
    pub const ECHO: u8 = 7;

    pub fn name(r: u8) -> &'static str {
        match r {
            DISABLED => "disabled",
            TOO_LARGE => "too large",
            STALE => "older than the current clip",
            DUPLICATE => "duplicate",
            INVALID => "invalid",
            BUSY => "busy",
            ECHO => "echo",
            _ => "unknown",
        }
    }
}

/// Clipboard formats in a payload.
pub mod format {
    /// UTF-8 text (CF_UNICODETEXT on Windows).
    pub const TEXT: u8 = 1;
    /// The "HTML Format" clipboard format (UTF-8 with its offset header).
    pub const HTML: u8 = 2;
    /// "Rich Text Format".
    pub const RTF: u8 = 3;
    /// Device-independent bitmap (CF_DIB: BITMAPINFO header, color table, pixels).
    pub const DIB: u8 = 4;
    /// The registered "PNG" format.
    pub const PNG: u8 = 5;
}
const FORMAT_COUNT: usize = 5;

/// How a format's bytes are encoded in a payload.
pub mod codec {
    pub const NONE: u8 = 0;
    /// Xpress Huffman (MS-XCA) through the Windows Compression API. Both ends run Windows.
    pub const XPRESS_HUFF: u8 = 1;
}

// ---------------------------------------------------------------- clip model

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClipKind {
    Text,
    Rich,
    Image,
}

impl ClipKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ClipKind::Text => "text",
            ClipKind::Rich => "rich",
            ClipKind::Image => "image",
        }
    }

    fn to_u8(self) -> u8 {
        match self {
            ClipKind::Text => 1,
            ClipKind::Rich => 2,
            ClipKind::Image => 3,
        }
    }

    fn from_u8(v: u8) -> Option<Self> {
        match v {
            1 => Some(ClipKind::Text),
            2 => Some(ClipKind::Rich),
            3 => Some(ClipKind::Image),
            _ => None,
        }
    }
}

/// Clipboard contents in the formats Moush synchronizes. Byte formats hold exactly the
/// format's data (no trailing NUL or allocation padding).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Clip {
    pub text: Option<String>,
    pub html: Option<Vec<u8>>,
    pub rtf: Option<Vec<u8>>,
    pub dib: Option<Vec<u8>>,
    pub png: Option<Vec<u8>>,
}

/// Which formats to take from the clipboard (from the settings).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FormatFilter {
    pub text: bool,
    pub rich: bool,
    pub images: bool,
}

impl FormatFilter {
    pub fn from_settings(s: &ClipboardSettings) -> Self {
        Self { text: s.text, rich: s.text && s.rich_text, images: s.images }
    }
}

impl Clip {
    pub fn text(s: impl Into<String>) -> Self {
        Self { text: Some(s.into()), ..Default::default() }
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_none() && self.html.is_none() && self.rtf.is_none() && self.dib.is_none() && self.png.is_none()
    }

    /// Sum of the formats' sizes in bytes.
    pub fn size(&self) -> usize {
        self.formats().map(|(_, d)| d.len()).sum()
    }

    /// Present formats in canonical order.
    pub fn formats(&self) -> impl Iterator<Item = (u8, &[u8])> {
        [
            (format::TEXT, self.text.as_ref().map(|t| t.as_bytes())),
            (format::HTML, self.html.as_deref()),
            (format::RTF, self.rtf.as_deref()),
            (format::DIB, self.dib.as_deref()),
            (format::PNG, self.png.as_deref()),
        ]
        .into_iter()
        .filter_map(|(f, d)| d.map(|d| (f, d)))
    }

    pub fn kind(&self) -> ClipKind {
        if self.text.is_none() && (self.dib.is_some() || self.png.is_some()) {
            ClipKind::Image
        } else if self.html.is_some() || self.rtf.is_some() {
            ClipKind::Rich
        } else {
            ClipKind::Text
        }
    }

    /// Content hash over every present format (stable across encodings and compression).
    pub fn hash(&self) -> u64 {
        let mut h = Blake2s256::new();
        for (f, d) in self.formats() {
            h.update([f]);
            h.update((d.len() as u64).to_le_bytes());
            h.update(d);
        }
        u64::from_le_bytes(h.finalize()[..8].try_into().unwrap())
    }

    /// Keeps the formats the filter allows. Rich text only travels alongside plain text,
    /// and images only when the clip has no text: apps such as Word offer a rendered
    /// bitmap of copied text, which must not turn a text copy into an image.
    pub fn filtered(&self, f: FormatFilter) -> Clip {
        let text = if f.text { self.text.clone() } else { None };
        let rich = f.rich && text.is_some();
        let images = f.images && self.text.is_none();
        Clip {
            html: if rich { self.html.clone() } else { None },
            rtf: if rich { self.rtf.clone() } else { None },
            dib: if images { self.dib.clone() } else { None },
            png: if images { self.png.clone() } else { None },
            text,
        }
    }

    /// Fits the clip into `max` bytes by dropping optional formats (the PNG when a bitmap
    /// is present, then RTF, then HTML). `None` when it cannot fit or nothing is left.
    pub fn fit(mut self, max: usize) -> Option<Clip> {
        if self.size() > max && self.dib.is_some() {
            self.png = None;
        }
        if self.size() > max {
            self.rtf = None;
        }
        if self.size() > max {
            self.html = None;
        }
        (self.size() <= max && !self.is_empty()).then_some(self)
    }

    /// Image dimensions from the PNG or bitmap header.
    pub fn image_size(&self) -> Option<(u32, u32)> {
        self.png.as_deref().and_then(png_size).or_else(|| self.dib.as_deref().and_then(dib_size))
    }

    /// Short description for the history: the start of the text or the image size.
    pub fn preview(&self) -> String {
        match (&self.text, self.image_size()) {
            (Some(t), _) => t.chars().take(PREVIEW_CHARS).collect(),
            (None, Some((w, h))) => format!("Image {w} x {h}"),
            (None, None) if self.dib.is_some() || self.png.is_some() => "Image".into(),
            _ => String::new(),
        }
    }

    /// Checks every format the way a receiver must before handing it to the system.
    pub fn validate(&self) -> Result<(), DecodeError> {
        if self.is_empty() {
            return Err(DecodeError::Invalid("empty clip"));
        }
        if self.text.as_ref().is_some_and(|t| t.contains('\0')) {
            return Err(DecodeError::Invalid("NUL in text"));
        }
        if self.html.as_ref().is_some_and(|d| d.is_empty() || d.contains(&0)) {
            return Err(DecodeError::Invalid("html"));
        }
        if self.rtf.as_ref().is_some_and(|d| d.is_empty() || d.contains(&0)) {
            return Err(DecodeError::Invalid("rtf"));
        }
        if let Some(d) = &self.dib
            && dib_len(d) != Some(d.len())
        {
            return Err(DecodeError::Invalid("bitmap"));
        }
        if let Some(d) = &self.png
            && png_len(d) != Some(d.len())
        {
            return Err(DecodeError::Invalid("png"));
        }
        Ok(())
    }
}

/// Exact size of a packed DIB (BITMAPINFOHEADER or later, color table or masks, pixels,
/// embedded color profile), or `None` if the header is malformed or `data` is too short.
pub fn dib_len(data: &[u8]) -> Option<usize> {
    let u32_at = |o: usize| data.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let i32_at = |o: usize| data.get(o..o + 4).map(|b| i32::from_le_bytes(b.try_into().unwrap()));
    let u16_at = |o: usize| data.get(o..o + 2).map(|b| u16::from_le_bytes(b.try_into().unwrap()));
    let header = u32_at(0)? as usize;
    if !(40..=1024).contains(&header) || header > data.len() {
        return None;
    }
    let width = i32_at(4)?;
    let height = i32_at(8)?;
    let planes = u16_at(12)?;
    let bits = u16_at(14)? as u64;
    let compression = u32_at(16)?;
    let size_image = u32_at(20)? as u64;
    let clr_used = u32_at(32)? as u64;
    if width <= 0 || width > 100_000 || height == 0 || height.unsigned_abs() > 100_000 || planes != 1 {
        return None;
    }
    if !matches!(bits, 1 | 4 | 8 | 16 | 24 | 32) {
        return None;
    }
    const BI_RGB: u32 = 0;
    const BI_RLE8: u32 = 1;
    const BI_RLE4: u32 = 2;
    const BI_BITFIELDS: u32 = 3;
    const BI_ALPHABITFIELDS: u32 = 6;
    let masks = match compression {
        BI_BITFIELDS if header == 40 => 12,
        BI_ALPHABITFIELDS if header == 40 => 16,
        _ => 0,
    };
    let colors = if clr_used > 0 {
        if clr_used > 256 {
            return None;
        }
        clr_used * 4
    } else if bits <= 8 {
        (1u64 << bits) * 4
    } else {
        0
    };
    let pixels = match compression {
        BI_RGB | BI_BITFIELDS | BI_ALPHABITFIELDS => {
            let stride = (width as u64 * bits).div_ceil(32) * 4;
            stride * height.unsigned_abs() as u64
        }
        BI_RLE8 | BI_RLE4 if size_image > 0 => size_image,
        _ => return None,
    };
    let mut end = header as u64 + masks + colors + pixels;
    // BITMAPV5HEADER with an embedded ICC profile (offset relative to the header).
    const PROFILE_EMBEDDED: u32 = 0x4D42_4544; // 'MBED'
    const PROFILE_LINKED: u32 = 0x4C49_4E4B; // 'LINK'
    if header >= 124 && matches!(u32_at(56)?, PROFILE_EMBEDDED | PROFILE_LINKED) {
        let offset = u32_at(112)? as u64;
        let size = u32_at(116)? as u64;
        end = end.max(offset + size);
    }
    let end = usize::try_from(end).ok()?;
    (end <= data.len()).then_some(end)
}

fn dib_size(data: &[u8]) -> Option<(u32, u32)> {
    dib_len(data)?;
    let w = i32::from_le_bytes(data[4..8].try_into().ok()?);
    let h = i32::from_le_bytes(data[8..12].try_into().ok()?);
    Some((w.unsigned_abs(), h.unsigned_abs()))
}

const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// Exact size of a PNG stream (signature through the IEND chunk), or `None` if malformed.
pub fn png_len(data: &[u8]) -> Option<usize> {
    if data.get(..8)? != PNG_SIGNATURE {
        return None;
    }
    let mut pos = 8usize;
    let mut first = true;
    loop {
        let len = u32::from_be_bytes(data.get(pos..pos + 4)?.try_into().ok()?) as usize;
        if len > 0x7FFF_FFFF {
            return None;
        }
        let ty = data.get(pos + 4..pos + 8)?;
        if first && (ty != b"IHDR" || len != 13) {
            return None;
        }
        first = false;
        let end = pos.checked_add(12)?.checked_add(len)?;
        if end > data.len() {
            return None;
        }
        if ty == b"IEND" {
            return Some(end);
        }
        pos = end;
    }
}

fn png_size(data: &[u8]) -> Option<(u32, u32)> {
    png_len(data)?;
    let w = u32::from_be_bytes(data.get(16..20)?.try_into().ok()?);
    let h = u32::from_be_bytes(data.get(20..24)?.try_into().ok()?);
    Some((w, h))
}

// ---------------------------------------------------------------- payload

/// Serializes a clip: `[count u8]` then per format `[format u8][codec u8][raw_len u32][bytes]`.
/// `compress` (for [`codec::XPRESS_HUFF`]) is tried on formats of at least 1 KiB and
/// used only when it makes them smaller.
pub fn encode_payload(clip: &Clip, compress: impl Fn(&[u8]) -> Option<Vec<u8>>) -> Vec<u8> {
    let mut out = Vec::with_capacity(clip.size() / 2 + MAX_PAYLOAD_OVERHEAD);
    put_u8(&mut out, clip.formats().count() as u8);
    for (f, data) in clip.formats() {
        put_u8(&mut out, f);
        let packed = if data.len() >= COMPRESS_MIN { compress(data).filter(|c| c.len() < data.len()) } else { None };
        put_u8(&mut out, if packed.is_some() { codec::XPRESS_HUFF } else { codec::NONE });
        put_u32(&mut out, data.len() as u32);
        put_bytes(&mut out, packed.as_deref().unwrap_or(data));
    }
    out
}

/// Parses and validates a payload whose formats add up to exactly `raw_len` bytes (at
/// most `max_raw`). `decompress(codec, data, raw_len)` must return exactly `raw_len`
/// bytes or `None`.
pub fn decode_payload(
    buf: &[u8],
    raw_len: usize,
    max_raw: usize,
    decompress: impl Fn(u8, &[u8], usize) -> Option<Vec<u8>>,
) -> Result<Clip, DecodeError> {
    if raw_len > max_raw.min(MAX_CLIP_BYTES) {
        return Err(DecodeError::Invalid("clip too large"));
    }
    let mut r = Reader::new(buf);
    let count = r.u8()? as usize;
    if count == 0 || count > FORMAT_COUNT {
        return Err(DecodeError::Invalid("format count"));
    }
    let mut clip = Clip::default();
    let mut total = 0usize;
    for _ in 0..count {
        let f = r.u8()?;
        let c = r.u8()?;
        let n = r.u32()? as usize;
        total = total.checked_add(n).filter(|t| *t <= raw_len).ok_or(DecodeError::Invalid("format sizes"))?;
        let data = r.bytes(n)?;
        let data = match c {
            codec::NONE => data.to_vec(),
            codec::XPRESS_HUFF if data.len() < n => {
                decompress(c, data, n).filter(|d| d.len() == n).ok_or(DecodeError::Invalid("compressed data"))?
            }
            _ => return Err(DecodeError::Invalid("codec")),
        };
        let slot_taken = match f {
            format::TEXT => {
                let s = String::from_utf8(data).map_err(|_| DecodeError::Invalid("utf-8"))?;
                clip.text.replace(s).is_some()
            }
            format::HTML => clip.html.replace(data).is_some(),
            format::RTF => clip.rtf.replace(data).is_some(),
            format::DIB => clip.dib.replace(data).is_some(),
            format::PNG => clip.png.replace(data).is_some(),
            _ => return Err(DecodeError::Invalid("format")),
        };
        if slot_taken {
            return Err(DecodeError::Invalid("duplicate format"));
        }
    }
    r.finish()?;
    if total != raw_len {
        return Err(DecodeError::Invalid("clip size"));
    }
    clip.validate()?;
    Ok(clip)
}

// ---------------------------------------------------------------- messages

/// Identifies one clip across all devices; ordered by stamp, then origin device id.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub stamp: u64,
    pub origin: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offer {
    /// Sender-chosen id of this transfer on this connection.
    pub xfer: u32,
    pub version: Version,
    /// Display name of the origin device.
    pub origin_name: String,
    /// [`Clip::hash`] of the payload's clip.
    pub hash: u64,
    pub kind: ClipKind,
    /// Sum of the uncompressed formats.
    pub raw_len: u32,
    /// Bytes of payload that follow in chunks.
    pub wire_len: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClipMsg {
    Offer(Offer),
    Chunk { xfer: u32, offset: u32, data: Vec<u8> },
    Ack { xfer: u32, bytes: u32 },
    Abort { xfer: u32 },
    Reject { xfer: u32, reason: u8 },
    /// Part of a shared text: `data` goes at `offset` of a `total`-byte UTF-8 text.
    Share { id: u32, total: u32, offset: u32, is_last: bool, data: Vec<u8> },
    Clock { stamp: u64 },
}

impl ClipMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(match self {
            ClipMsg::Chunk { data, .. } | ClipMsg::Share { data, .. } => data.len() + 32,
            _ => 128,
        });
        match self {
            ClipMsg::Offer(o) => {
                put_u8(&mut b, kind::OFFER);
                put_u32(&mut b, o.xfer);
                put_u64(&mut b, o.version.stamp);
                put_str8(&mut b, &o.version.origin);
                let name: String = o.origin_name.chars().filter(|c| !c.is_control()).collect();
                put_str8(&mut b, &name);
                put_u64(&mut b, o.hash);
                put_u8(&mut b, o.kind.to_u8());
                put_u32(&mut b, o.raw_len);
                put_u32(&mut b, o.wire_len);
            }
            ClipMsg::Chunk { xfer, offset, data } => {
                put_u8(&mut b, kind::CHUNK);
                put_u32(&mut b, *xfer);
                put_u32(&mut b, *offset);
                put_bytes(&mut b, data);
            }
            ClipMsg::Ack { xfer, bytes } => {
                put_u8(&mut b, kind::ACK);
                put_u32(&mut b, *xfer);
                put_u32(&mut b, *bytes);
            }
            ClipMsg::Abort { xfer } => {
                put_u8(&mut b, kind::ABORT);
                put_u32(&mut b, *xfer);
            }
            ClipMsg::Reject { xfer, reason } => {
                put_u8(&mut b, kind::REJECT);
                put_u32(&mut b, *xfer);
                put_u8(&mut b, *reason);
            }
            ClipMsg::Share { id, total, offset, is_last, data } => {
                put_u8(&mut b, kind::SHARE);
                put_u32(&mut b, *id);
                put_u32(&mut b, *total);
                put_u32(&mut b, *offset);
                put_bool(&mut b, *is_last);
                put_bytes(&mut b, data);
            }
            ClipMsg::Clock { stamp } => {
                put_u8(&mut b, kind::CLOCK);
                put_u64(&mut b, *stamp);
            }
        }
        debug_assert!(b.len() <= MAX_FEATURE_BODY);
        b
    }

    /// Decodes and validates one body; trailing bytes are an error.
    pub fn decode(body: &[u8]) -> Result<ClipMsg, DecodeError> {
        let mut r = Reader::new(body);
        let msg = match r.u8()? {
            kind::OFFER => {
                let xfer = r.u32()?;
                let stamp = r.u64()?;
                let origin = r.str8()?;
                if origin.is_empty() || origin.len() > MAX_ORIGIN {
                    return Err(DecodeError::Invalid("origin"));
                }
                let origin_name = r.str8()?;
                let hash = r.u64()?;
                let kind = ClipKind::from_u8(r.u8()?).ok_or(DecodeError::Invalid("clip kind"))?;
                let raw_len = r.u32()?;
                let wire_len = r.u32()?;
                if raw_len == 0 || raw_len as usize > MAX_CLIP_BYTES {
                    return Err(DecodeError::Invalid("clip size"));
                }
                if wire_len == 0 || wire_len as usize > raw_len as usize + MAX_PAYLOAD_OVERHEAD {
                    return Err(DecodeError::Invalid("payload size"));
                }
                ClipMsg::Offer(Offer { xfer, version: Version { stamp, origin }, origin_name, hash, kind, raw_len, wire_len })
            }
            kind::CHUNK => {
                let xfer = r.u32()?;
                let offset = r.u32()?;
                let data = r.bytes(MAX_CHUNK_DATA)?;
                if data.is_empty() || offset as usize + data.len() > MAX_CLIP_BYTES + MAX_PAYLOAD_OVERHEAD {
                    return Err(DecodeError::Invalid("chunk"));
                }
                ClipMsg::Chunk { xfer, offset, data: data.to_vec() }
            }
            kind::ACK => ClipMsg::Ack { xfer: r.u32()?, bytes: r.u32()? },
            kind::ABORT => ClipMsg::Abort { xfer: r.u32()? },
            kind::REJECT => ClipMsg::Reject { xfer: r.u32()?, reason: r.u8()? },
            kind::SHARE => {
                let id = r.u32()?;
                let total = r.u32()?;
                let offset = r.u32()?;
                let is_last = r.bool()?;
                let data = r.bytes(MAX_CHUNK_DATA)?;
                let end = offset as usize + data.len();
                if total == 0 || total as usize > MAX_SHARE_TEXT || data.is_empty() || end > total as usize || is_last != (end == total as usize) {
                    return Err(DecodeError::Invalid("shared text part"));
                }
                ClipMsg::Share { id, total, offset, is_last, data: data.to_vec() }
            }
            kind::CLOCK => ClipMsg::Clock { stamp: r.u64()? },
            _ => return Err(DecodeError::Invalid("clipboard message kind")),
        };
        r.finish()?;
        Ok(msg)
    }
}

// ---------------------------------------------------------------- reassembly

/// A clip being received: payload bytes arrive strictly in order.
#[derive(Debug)]
pub struct Incoming {
    pub offer: Offer,
    pub buf: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ChunkResult {
    More,
    Complete,
    /// Out of order, wrong transfer or more data than announced: drop the transfer.
    Invalid,
}

impl Incoming {
    pub fn new(offer: Offer) -> Self {
        // Grow as data arrives; never trust the announced size for an up-front allocation.
        let cap = (offer.wire_len as usize).min(1024 * 1024);
        Self { offer, buf: Vec::with_capacity(cap) }
    }

    pub fn push(&mut self, offset: u32, data: &[u8]) -> ChunkResult {
        let end = self.buf.len() + data.len();
        if offset as usize != self.buf.len() || end > self.offer.wire_len as usize {
            return ChunkResult::Invalid;
        }
        self.buf.extend_from_slice(data);
        if end == self.offer.wire_len as usize { ChunkResult::Complete } else { ChunkResult::More }
    }
}

/// A shared text being received.
#[derive(Debug)]
pub struct ShareAssembler {
    id: u32,
    total: u32,
    buf: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ShareResult {
    More,
    Done(String),
    Invalid,
}

impl ShareAssembler {
    /// Adds a part to `slot` (one text in progress per peer; a new id replaces it).
    pub fn push(slot: &mut Option<ShareAssembler>, id: u32, total: u32, offset: u32, data: &[u8]) -> ShareResult {
        if slot.as_ref().is_none_or(|s| s.id != id) {
            *slot = (offset == 0).then(|| ShareAssembler { id, total, buf: Vec::with_capacity(total as usize) });
        }
        let Some(s) = slot.as_mut() else { return ShareResult::Invalid };
        if s.total != total || s.buf.len() != offset as usize || s.buf.len() + data.len() > total as usize {
            *slot = None;
            return ShareResult::Invalid;
        }
        s.buf.extend_from_slice(data);
        if s.buf.len() < total as usize {
            return ShareResult::More;
        }
        let s = slot.take().unwrap();
        match String::from_utf8(s.buf) {
            Ok(t) if validate_share_text(&t).is_ok() => ShareResult::Done(t),
            _ => ShareResult::Invalid,
        }
    }
}

/// Splits a shared text into `SHARE` messages.
pub fn share_messages(id: u32, text: &str) -> Vec<ClipMsg> {
    let bytes = text.as_bytes();
    let total = bytes.len() as u32;
    bytes
        .chunks(MAX_CHUNK_DATA)
        .enumerate()
        .map(|(i, c)| {
            let offset = (i * MAX_CHUNK_DATA) as u32;
            ClipMsg::Share { id, total, offset, is_last: offset as usize + c.len() == bytes.len(), data: c.to_vec() }
        })
        .collect()
}

/// Checks text before sharing it (and again after receiving it).
pub fn validate_share_text(text: &str) -> Result<(), &'static str> {
    if text.trim().is_empty() {
        return Err("there is no text to share");
    }
    if text.len() > MAX_SHARE_TEXT {
        return Err("the text is too long to share (at most 64 KB)");
    }
    if text.contains('\0') {
        return Err("the text contains a NUL character");
    }
    Ok(())
}

/// The link if the whole text (ignoring surrounding whitespace) is a single http(s) URL
/// without whitespace or control characters.
pub fn as_link(text: &str) -> Option<&str> {
    let t = text.trim();
    let lower = t.chars().take(8).collect::<String>().to_ascii_lowercase();
    let rest = if lower.starts_with("https://") {
        &t[8..]
    } else if lower.starts_with("http://") {
        &t[7..]
    } else {
        return None;
    };
    let host_ok = rest.chars().next().is_some_and(|c| c != '/' && c != '?' && c != '#');
    let clean = !t.chars().any(|c| c.is_whitespace() || c.is_control() || c == '"' || c == '<' || c == '>');
    (host_ok && clean && t.len() <= MAX_LINK).then_some(t)
}

// ---------------------------------------------------------------- sync state

/// What the sync currently holds on this device's clipboard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Current {
    pub version: Version,
    /// Hash of the clip as received/copied and of what was written to the clipboard
    /// (they differ when local settings filtered formats out).
    pub hashes: [u64; 2],
}

#[derive(Debug, PartialEq, Eq)]
pub enum Decision {
    Accept,
    /// Refuse with a [`reject`] reason. `send_back`: our clip is newer, send it to the peer.
    Reject { reason: u8, send_back: bool },
}

/// The clipboard sync state machine of one device (see the module docs for the rules).
#[derive(Debug)]
pub struct SyncState {
    me: String,
    clock: u64,
    current: Option<Current>,
    /// Clipboard sequence number right after our last own write.
    own_seq: Option<u32>,
    /// Last sequence number we read (or skipped).
    last_seq: Option<u32>,
    /// Hash of the last clip exchanged with each peer (connected peers only).
    peer_hash: HashMap<String, u64>,
}

impl SyncState {
    pub fn new(me: impl Into<String>) -> Self {
        Self { me: me.into(), clock: 0, current: None, own_seq: None, last_seq: None, peer_hash: HashMap::new() }
    }

    pub fn me(&self) -> &str {
        &self.me
    }

    pub fn clock(&self) -> u64 {
        self.clock
    }

    pub fn current(&self) -> Option<&Current> {
        self.current.as_ref()
    }

    /// A version stamp seen from a peer advances our clock.
    pub fn observe(&mut self, stamp: u64) {
        self.clock = self.clock.max(stamp);
    }

    fn next_version(&mut self, now_ms: u64) -> Version {
        self.clock = now_ms.max(self.clock.saturating_add(1));
        Version { stamp: self.clock, origin: self.me.clone() }
    }

    /// Whether a clipboard change with sequence number `seq` needs reading at all.
    pub fn should_read(&mut self, seq: u32) -> bool {
        if self.own_seq == Some(seq) || self.last_seq == Some(seq) {
            return false;
        }
        self.last_seq = Some(seq);
        true
    }

    /// Reading the clipboard failed: the next change notification (or a retry) reads again.
    pub fn read_failed(&mut self) {
        self.last_seq = None;
    }

    /// The local clipboard now holds content with `hash` (read at sequence `seq`).
    /// Returns the new clip's version, or `None` if this is not a new local copy.
    pub fn local_change(&mut self, seq: Option<u32>, hash: u64, now_ms: u64) -> Option<Version> {
        if let Some(s) = seq {
            self.last_seq = Some(s);
            if self.own_seq == Some(s) {
                return None;
            }
        }
        if self.current.as_ref().is_some_and(|c| c.hashes.contains(&hash)) {
            return None;
        }
        let version = self.next_version(now_ms);
        self.current = Some(Current { version: version.clone(), hashes: [hash, hash] });
        Some(version)
    }

    /// The sync itself put a local clip on the clipboard on the user's request (e.g. from
    /// the history): it is a new local copy, unless it is what we already hold.
    pub fn local_write(&mut self, seq: u32, hash: u64, now_ms: u64) -> Option<Version> {
        self.own_seq = Some(seq);
        self.local_change(None, hash, now_ms)
    }

    /// Decides on a clip offered by `peer` (called on the offer and again before applying).
    pub fn check(&mut self, peer: &str, version: &Version, hash: u64) -> Decision {
        self.observe(version.stamp);
        if version.origin == self.me {
            return Decision::Reject { reason: reject::ECHO, send_back: false };
        }
        let Some(c) = &mut self.current else { return Decision::Accept };
        if c.hashes.contains(&hash) {
            // Same content: keep it, adopt the larger version.
            if *version > c.version {
                c.version = version.clone();
            }
            self.peer_hash.insert(peer.to_owned(), hash);
            return Decision::Reject { reason: reject::DUPLICATE, send_back: false };
        }
        if c.version >= *version {
            let ours = c.hashes[1];
            let send_back = self.peer_hash.get(peer) != Some(&ours) && c.version.origin != peer;
            return Decision::Reject { reason: reject::STALE, send_back };
        }
        Decision::Accept
    }

    /// A clip received from `from` was written to the clipboard (sequence `seq` after the
    /// write). `written` is the hash of what was written.
    pub fn applied(&mut self, from: &str, version: Version, received: u64, written: u64, seq: u32) {
        self.observe(version.stamp);
        self.current = Some(Current { version, hashes: [received, written] });
        self.own_seq = Some(seq);
        self.peer_hash.insert(from.to_owned(), received);
    }

    /// Peers a clip goes to: every candidate with the clipboard feature except the peer it
    /// came from, its origin device and peers that already have this content.
    pub fn targets(&self, candidates: &[String], from: Option<&str>, version: &Version, hash: u64) -> Vec<String> {
        candidates
            .iter()
            .filter(|p| Some(p.as_str()) != from && **p != version.origin && self.peer_hash.get(*p) != Some(&hash))
            .cloned()
            .collect()
    }

    /// Records that a clip with `hash` is being sent to `peer`.
    pub fn sent(&mut self, peer: &str, hash: u64) {
        self.peer_hash.insert(peer.to_owned(), hash);
    }

    /// A send failed or was refused: the peer may not have this content.
    pub fn unsent(&mut self, peer: &str, hash: u64) {
        if self.peer_hash.get(peer) == Some(&hash) {
            self.peer_hash.remove(peer);
        }
    }

    pub fn forget_peer(&mut self, peer: &str) {
        self.peer_hash.remove(peer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dib(w: i32, h: i32, bits: u16) -> Vec<u8> {
        let stride = ((w as usize * bits as usize).div_ceil(32)) * 4;
        let colors = if bits <= 8 { (1usize << bits) * 4 } else { 0 };
        let mut d = vec![0u8; 40 + colors + stride * h.unsigned_abs() as usize];
        d[0..4].copy_from_slice(&40u32.to_le_bytes());
        d[4..8].copy_from_slice(&w.to_le_bytes());
        d[8..12].copy_from_slice(&h.to_le_bytes());
        d[12..14].copy_from_slice(&1u16.to_le_bytes());
        d[14..16].copy_from_slice(&bits.to_le_bytes());
        for (i, b) in d[40 + colors..].iter_mut().enumerate() {
            *b = (i * 7 % 251) as u8;
        }
        d
    }

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut d = PNG_SIGNATURE.to_vec();
        let chunk = |d: &mut Vec<u8>, ty: &[u8], data: &[u8]| {
            d.extend_from_slice(&(data.len() as u32).to_be_bytes());
            d.extend_from_slice(ty);
            d.extend_from_slice(data);
            d.extend_from_slice(&[0, 0, 0, 0]);
        };
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&w.to_be_bytes());
        ihdr.extend_from_slice(&h.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut d, b"IHDR", &ihdr);
        chunk(&mut d, b"IDAT", &[1, 2, 3]);
        chunk(&mut d, b"IEND", &[]);
        d
    }

    /// Test "compressor": run-length pairs, only useful to exercise the codec path.
    fn rle(data: &[u8]) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        for run in data.chunk_by(|a, b| a == b) {
            for part in run.chunks(255) {
                out.extend_from_slice(&[part.len() as u8, part[0]]);
            }
        }
        Some(out)
    }

    fn unrle(c: u8, data: &[u8], n: usize) -> Option<Vec<u8>> {
        assert_eq!(c, codec::XPRESS_HUFF);
        let mut out = Vec::new();
        for p in data.chunks(2) {
            if p.len() != 2 || out.len() + p[0] as usize > n {
                return None;
            }
            out.extend(std::iter::repeat_n(p[1], p[0] as usize));
        }
        Some(out)
    }

    const TRICKY: &str = "Zeile 1 \u{1F600}\r\n\u{4E2D}\u{6587} \u{65E5}\u{672C}\u{8A9E}\r\n\ttab\nLF only\r\n\u{200D}\u{1F469}\u{200D}\u{1F4BB} end";

    fn roundtrip(clip: &Clip) -> Clip {
        let p = encode_payload(clip, rle);
        decode_payload(&p, clip.size(), MAX_CLIP_BYTES, unrle).expect("decode")
    }

    #[test]
    fn payload_roundtrips_every_format_exactly() {
        let clip = Clip {
            text: Some(TRICKY.into()),
            html: Some(b"Version:0.9\r\nStartHTML:0\r\n<b>x</b>".to_vec()),
            rtf: Some(br"{\rtf1 hi}".to_vec()),
            ..Default::default()
        };
        assert_eq!(roundtrip(&clip), clip);
        assert_eq!(clip.kind(), ClipKind::Rich);
        let img = Clip { dib: Some(dib(64, -32, 32)), png: Some(png(64, 32)), ..Default::default() };
        assert_eq!(roundtrip(&img), img);
        assert_eq!(img.kind(), ClipKind::Image);
        assert_eq!(img.preview(), "Image 64 x 32");
        // Compressible data takes the codec path and still round-trips.
        let big = Clip::text("a".repeat(100_000));
        let p = encode_payload(&big, rle);
        assert!(p.len() < 2000, "compressed");
        assert_eq!(roundtrip(&big), big);
        assert_eq!(roundtrip(&Clip::text(TRICKY)).text.as_deref(), Some(TRICKY));
    }

    #[test]
    fn messages_roundtrip() {
        let msgs = [
            ClipMsg::Offer(Offer {
                xfer: 7,
                version: Version { stamp: 1_700_000_000_000, origin: "a1b2c3d4e5f6a7b8".into() },
                origin_name: "Laptop \u{e9}".into(),
                hash: u64::MAX - 3,
                kind: ClipKind::Image,
                raw_len: 5_000_000,
                wire_len: 5_000_010,
            }),
            ClipMsg::Chunk { xfer: 7, offset: 32_000, data: vec![9; MAX_CHUNK_DATA] },
            ClipMsg::Ack { xfer: 7, bytes: 32_000 },
            ClipMsg::Abort { xfer: 7 },
            ClipMsg::Reject { xfer: 7, reason: reject::TOO_LARGE },
            ClipMsg::Clock { stamp: 42 },
        ];
        for m in msgs {
            let b = m.encode();
            assert!(b.len() <= MAX_FEATURE_BODY);
            assert_eq!(ClipMsg::decode(&b).unwrap(), m);
        }
        let text = TRICKY.repeat(1000);
        let parts = share_messages(3, &text);
        assert!(parts.len() > 1);
        let mut slot = None;
        let mut done = None;
        for p in parts {
            let ClipMsg::Share { id, total, offset, data, .. } = ClipMsg::decode(&p.encode()).unwrap() else { panic!() };
            match ShareAssembler::push(&mut slot, id, total, offset, &data) {
                ShareResult::More => {}
                ShareResult::Done(t) => done = Some(t),
                ShareResult::Invalid => panic!("invalid"),
            }
        }
        assert_eq!(done.as_deref(), Some(text.as_str()));
    }

    #[test]
    fn hostile_messages_are_rejected() {
        let offer = ClipMsg::Offer(Offer {
            xfer: 1,
            version: Version { stamp: 1, origin: "abc".into() },
            origin_name: "x".into(),
            hash: 1,
            kind: ClipKind::Text,
            raw_len: 10,
            wire_len: 20,
        })
        .encode();
        // Every truncation fails, never panics.
        for n in 0..offer.len() {
            assert!(ClipMsg::decode(&offer[..n]).is_err(), "truncated at {n}");
        }
        let mut trailing = offer.clone();
        trailing.push(0);
        assert!(ClipMsg::decode(&trailing).is_err(), "trailing bytes");
        // Payload far larger than the clip.
        let mut bad = ClipMsg::Offer(Offer {
            xfer: 1,
            version: Version { stamp: 1, origin: "abc".into() },
            origin_name: "x".into(),
            hash: 1,
            kind: ClipKind::Text,
            raw_len: 10,
            wire_len: 10 + MAX_PAYLOAD_OVERHEAD as u32 + 1,
        });
        assert!(ClipMsg::decode(&bad.encode()).is_err());
        if let ClipMsg::Offer(o) = &mut bad {
            o.wire_len = 10;
            o.raw_len = MAX_CLIP_BYTES as u32 + 1;
        }
        assert!(ClipMsg::decode(&bad.encode()).is_err(), "oversized clip");
        // Empty origin, unknown kind, oversized chunk, bad share parts.
        let mut b = offer.clone();
        b[13] = 0; // origin length -> 0, rest misaligned
        assert!(ClipMsg::decode(&b).is_err());
        assert!(ClipMsg::decode(&[99]).is_err());
        assert!(ClipMsg::decode(&[]).is_err());
        let mut chunk = vec![kind::CHUNK];
        chunk.extend_from_slice(&1u32.to_le_bytes());
        chunk.extend_from_slice(&0u32.to_le_bytes());
        chunk.extend_from_slice(&((MAX_CHUNK_DATA + 1) as u32).to_le_bytes());
        chunk.resize(chunk.len() + MAX_CHUNK_DATA + 1, 0);
        assert!(ClipMsg::decode(&chunk).is_err(), "oversized chunk");
        let share = ClipMsg::Share { id: 1, total: 4, offset: 2, is_last: true, data: vec![1, 2, 3] };
        assert!(ClipMsg::decode(&share.encode()).is_err(), "part beyond total");
        let share = ClipMsg::Share { id: 1, total: MAX_SHARE_TEXT as u32 + 1, offset: 0, is_last: false, data: vec![1] };
        assert!(ClipMsg::decode(&share.encode()).is_err(), "share too long");
        // Bad UTF-8 in a shared text.
        let mut slot = None;
        assert_eq!(ShareAssembler::push(&mut slot, 1, 2, 0, &[0xC3, 0x28]), ShareResult::Invalid);
        // A part that does not start a text, or skips ahead.
        assert_eq!(ShareAssembler::push(&mut slot, 2, 10, 5, b"abc"), ShareResult::Invalid);
        assert_eq!(ShareAssembler::push(&mut slot, 3, 10, 0, b"abc"), ShareResult::More);
        assert_eq!(ShareAssembler::push(&mut slot, 3, 10, 4, b"abc"), ShareResult::Invalid);
        assert!(slot.is_none());
    }

    #[test]
    fn hostile_payloads_are_rejected() {
        let clip = Clip { text: Some("hello".into()), dib: None, ..Default::default() };
        let p = encode_payload(&clip, |_| None);
        let ok = |p: &[u8], raw: usize| decode_payload(p, raw, MAX_CLIP_BYTES, unrle);
        assert!(ok(&p, 5).is_ok());
        for n in 0..p.len() {
            assert!(ok(&p[..n], 5).is_err(), "truncated at {n}");
        }
        let mut t = p.clone();
        t.push(1);
        assert!(ok(&t, 5).is_err(), "trailing bytes");
        assert!(ok(&p, 6).is_err(), "size mismatch with the offer");
        assert!(decode_payload(&p, 5, 4, unrle).is_err(), "over the receiver's limit");
        // Bad UTF-8.
        let mut bad = p.clone();
        let last = bad.len() - 1;
        bad[last] = 0xFF;
        assert!(ok(&bad, 5).is_err());
        // NUL inside text.
        let nul = encode_payload(&Clip::text("a\0b"), |_| None);
        assert!(ok(&nul, 3).is_err());
        // Duplicate format.
        let mut dup = vec![2];
        for _ in 0..2 {
            dup.extend_from_slice(&[format::TEXT, codec::NONE]);
            dup.extend_from_slice(&1u32.to_le_bytes());
            dup.extend_from_slice(&1u32.to_le_bytes());
            dup.push(b'x');
        }
        assert!(ok(&dup, 2).is_err());
        // Unknown codec / raw length mismatch / decompression bomb.
        let mut c = vec![1, format::TEXT, 9];
        c.extend_from_slice(&1u32.to_le_bytes());
        c.extend_from_slice(&1u32.to_le_bytes());
        c.push(b'x');
        assert!(ok(&c, 1).is_err());
        let mut bomb = vec![1, format::TEXT, codec::XPRESS_HUFF];
        bomb.extend_from_slice(&100u32.to_le_bytes());
        bomb.extend_from_slice(&2u32.to_le_bytes());
        bomb.extend_from_slice(&[255, b'a']);
        assert!(ok(&bomb, 100).is_err(), "decompresses to more than announced");
        // Malformed bitmap: header claims more pixels than present.
        let mut d = dib(10, 10, 24);
        d.truncate(d.len() - 1);
        let img = Clip { dib: Some(d), ..Default::default() };
        let p = encode_payload(&img, |_| None);
        assert!(ok(&p, img.size()).is_err());
    }

    #[test]
    fn exact_image_lengths() {
        for (w, h, bits) in [(1, 1, 1), (3, 5, 4), (7, -3, 8), (5, 5, 16), (33, 2, 24), (4, 4, 32)] {
            let d = dib(w, h, bits);
            assert_eq!(dib_len(&d), Some(d.len()), "{w}x{h}x{bits}");
            let mut padded = d.clone();
            padded.extend_from_slice(&[0; 16]);
            assert_eq!(dib_len(&padded), Some(d.len()), "allocation padding is ignored");
        }
        assert_eq!(dib_len(&[0; 39]), None);
        let mut huge = dib(1, 1, 32);
        huge[4..8].copy_from_slice(&1_000_000i32.to_le_bytes());
        assert_eq!(dib_len(&huge), None);
        let p = png(3, 4);
        let mut padded = p.clone();
        padded.extend_from_slice(&[0; 7]);
        assert_eq!(png_len(&padded), Some(p.len()));
        assert_eq!(png_len(&p[..p.len() - 1]), None);
        assert_eq!(png_size(&p), Some((3, 4)));
    }

    #[test]
    fn filtering_and_fitting() {
        let all = Clip {
            text: Some("t".into()),
            html: Some(b"h".to_vec()),
            rtf: Some(b"r".to_vec()),
            dib: Some(dib(2, 2, 32)),
            png: None,
        };
        let f = |text, rich, images| FormatFilter { text, rich, images };
        let t = all.filtered(f(true, true, true));
        assert!(t.dib.is_none(), "images never accompany text");
        assert!(t.html.is_some() && t.rtf.is_some());
        let plain = all.filtered(f(true, false, true));
        assert_eq!(plain, Clip::text("t"));
        assert!(all.filtered(f(false, true, true)).is_empty(), "rich text needs text");
        let img = Clip { dib: Some(dib(100, 100, 32)), png: Some(png(100, 100)), ..Default::default() };
        assert!(img.filtered(f(true, true, false)).is_empty());
        let fitted = img.clone().fit(img.dib.as_ref().unwrap().len()).unwrap();
        assert!(fitted.png.is_none() && fitted.dib.is_some(), "png dropped first");
        assert!(img.fit(100).is_none());
        let rich = Clip { text: Some("x".repeat(10)), html: Some(vec![b'h'; 50]), rtf: Some(vec![b'r'; 50]), ..Default::default() };
        assert_eq!(rich.clone().fit(65).unwrap().rtf, None);
        assert_eq!(rich.clone().fit(10).unwrap(), Clip::text("x".repeat(10)));
        assert!(rich.fit(9).is_none());
        let s = ClipboardSettings { rich_text: false, ..Default::default() };
        assert_eq!(FormatFilter::from_settings(&s), f(true, false, true));
    }

    #[test]
    fn links() {
        assert_eq!(as_link("  https://example.com/a?b=c#d \n"), Some("https://example.com/a?b=c#d"));
        assert_eq!(as_link("HTTP://Example.com"), Some("HTTP://Example.com"));
        for bad in ["ftp://x", "https://", "https:///path", "https://a b", "see https://a.com", "javascript:alert(1)", "file:///c:/x", "https://a\"b"] {
            assert_eq!(as_link(bad), None, "{bad}");
        }
        assert!(validate_share_text("  ").is_err());
        assert!(validate_share_text(&"x".repeat(MAX_SHARE_TEXT + 1)).is_err());
        assert!(validate_share_text(TRICKY).is_ok());
    }

    // ------------------------------------------------------------ state machine

    #[test]
    fn own_writes_and_unchanged_content_are_not_copies() {
        let mut s = SyncState::new("me");
        let v = s.local_change(Some(10), 111, 1000).expect("a new local copy");
        assert_eq!(v, Version { stamp: 1000, origin: "me".into() });
        assert!(s.local_change(Some(11), 111, 2000).is_none(), "same content again is not a copy");
        assert!(!s.should_read(11), "already read");
        // A received clip written by the sync is recognised by its sequence number...
        let incoming = Version { stamp: 5000, origin: "peer".into() };
        assert_eq!(s.check("peer", &incoming, 222), Decision::Accept);
        s.applied("peer", incoming.clone(), 222, 223, 12);
        assert!(!s.should_read(12));
        assert!(s.local_change(Some(12), 999, 6000).is_none(), "own write by sequence number");
        // ...and by content if another app re-sets the same data.
        assert!(s.local_change(Some(13), 223, 6000).is_none(), "own write by content");
        // A later real copy gets a version after everything seen, even with a slow wall clock.
        let v = s.local_change(Some(14), 333, 10).unwrap();
        assert!(v > incoming);
    }

    #[test]
    fn dedupe_and_echo() {
        let mut s = SyncState::new("me");
        let v = s.local_change(Some(1), 1, 100).unwrap();
        // Our own clip coming back is never applied.
        assert_eq!(s.check("p", &v, 1), Decision::Reject { reason: reject::ECHO, send_back: false });
        // Same content from a peer: duplicate, and the larger version is adopted.
        let newer = Version { stamp: 200, origin: "p".into() };
        assert_eq!(s.check("p", &newer, 1), Decision::Reject { reason: reject::DUPLICATE, send_back: false });
        assert_eq!(s.current().unwrap().version, newer);
        // Content already exchanged with a peer is not sent to it again.
        let peers = vec!["p".to_string(), "q".to_string()];
        let v2 = s.local_change(Some(2), 2, 300).unwrap();
        assert_eq!(s.targets(&peers, None, &v2, 2), peers);
        s.sent("p", 2);
        assert_eq!(s.targets(&peers, None, &v2, 2), vec!["q".to_string()]);
        s.unsent("p", 2);
        assert_eq!(s.targets(&peers, None, &v2, 2).len(), 2);
    }

    #[test]
    fn relay_skips_source_and_origin() {
        let mut host = SyncState::new("host");
        let v = Version { stamp: 10, origin: "a".into() };
        assert_eq!(host.check("a", &v, 5), Decision::Accept);
        host.applied("a", v.clone(), 5, 5, 1);
        let peers: Vec<String> = ["a", "b", "c"].iter().map(|s| s.to_string()).collect();
        assert_eq!(host.targets(&peers, Some("a"), &v, 5), vec!["b".to_string(), "c".to_string()]);
        // A clip relayed through another path never goes back to its origin device.
        let vb = Version { stamp: 11, origin: "c".into() };
        assert_eq!(host.targets(&peers, Some("b"), &vb, 6), vec!["a".to_string()]);
    }

    #[test]
    fn newer_clip_supersedes_and_stale_gets_answered() {
        let mut s = SyncState::new("b");
        let old = Version { stamp: 10, origin: "a".into() };
        let new = Version { stamp: 20, origin: "a".into() };
        assert_eq!(s.check("a", &new, 2), Decision::Accept);
        s.applied("a", new.clone(), 2, 2, 1);
        // An older clip arriving later (or finishing later) is stale; the peer already has ours.
        assert_eq!(s.check("a", &old, 1), Decision::Reject { reason: reject::STALE, send_back: false });
        // A different peer with an older clip gets ours sent back.
        assert_eq!(s.check("c", &Version { stamp: 15, origin: "c".into() }, 3), Decision::Reject { reason: reject::STALE, send_back: true });
        // A local copy during a transfer makes the transfer stale when it completes.
        s.local_change(Some(5), 9, 0).unwrap();
        assert!(matches!(s.check("a", &Version { stamp: 21, origin: "a".into() }, 4), Decision::Reject { reason: reject::STALE, .. }));
    }

    /// Simulated star network: offers are delivered in a chosen order; every node runs
    /// the same rules as the engine (accept -> apply + relay, stale -> send back).
    struct Node {
        s: SyncState,
        content: Option<(u64, Version)>,
        peers: Vec<String>,
    }

    type Msg = (String, String, Version, u64); // from, to, version, hash

    fn deliver(nodes: &mut HashMap<String, Node>, (from, to, v, h): Msg, out: &mut Vec<Msg>) {
        let n = nodes.get_mut(&to).unwrap();
        match n.s.check(&from, &v, h) {
            Decision::Accept => {
                n.s.applied(&from, v.clone(), h, h, 0);
                n.content = Some((h, v.clone()));
                for p in n.s.targets(&n.peers, Some(&from), &v, h) {
                    n.s.sent(&p, h);
                    out.push((to.clone(), p, v.clone(), h));
                }
            }
            Decision::Reject { send_back: true, .. } => {
                let (ch, cv) = n.content.clone().unwrap();
                n.s.sent(&from, ch);
                out.push((to.clone(), from, cv, ch));
            }
            Decision::Reject { .. } => {}
        }
    }

    fn copy(nodes: &mut HashMap<String, Node>, who: &str, hash: u64, now: u64, out: &mut Vec<Msg>) {
        let n = nodes.get_mut(who).unwrap();
        let v = n.s.local_change(None, hash, now).unwrap();
        n.content = Some((hash, v.clone()));
        for p in n.s.targets(&n.peers, None, &v, hash) {
            n.s.sent(&p, hash);
            out.push((who.to_owned(), p, v.clone(), hash));
        }
    }

    fn star() -> HashMap<String, Node> {
        let mut nodes = HashMap::new();
        nodes.insert("h".into(), Node { s: SyncState::new("h"), content: None, peers: vec!["a".into(), "b".into(), "c".into()] });
        for c in ["a", "b", "c"] {
            nodes.insert(c.into(), Node { s: SyncState::new(c), content: None, peers: vec!["h".into()] });
        }
        nodes
    }

    fn run(nodes: &mut HashMap<String, Node>, mut queue: Vec<Msg>, lifo: bool) -> usize {
        let mut delivered = 0;
        while !queue.is_empty() {
            let m = if lifo { queue.pop().unwrap() } else { queue.remove(0) };
            let mut out = Vec::new();
            deliver(nodes, m, &mut out);
            queue.extend(out);
            delivered += 1;
            assert!(delivered < 1000, "sync loop");
        }
        delivered
    }

    #[test]
    fn concurrent_copies_converge_deterministically() {
        for lifo in [false, true] {
            // Two clients copy at the same instant: equal stamps, origin id breaks the tie.
            let mut nodes = star();
            let mut q = Vec::new();
            copy(&mut nodes, "a", 100, 5000, &mut q);
            copy(&mut nodes, "c", 300, 5000, &mut q);
            run(&mut nodes, q, lifo);
            for (id, n) in &nodes {
                assert_eq!(n.content.as_ref().unwrap().0, 300, "{id} converges on the larger origin id (lifo={lifo})");
            }
            // Host and a client copy concurrently with different clocks: the larger stamp wins.
            let mut q = Vec::new();
            copy(&mut nodes, "h", 400, 9000, &mut q);
            copy(&mut nodes, "b", 500, 9500, &mut q);
            run(&mut nodes, q, lifo);
            for n in nodes.values() {
                assert_eq!(n.content.as_ref().unwrap().0, 500);
            }
            // Same content copied concurrently on two machines: nothing to fight over.
            let mut q = Vec::new();
            copy(&mut nodes, "a", 600, 20_000, &mut q);
            copy(&mut nodes, "b", 600, 20_000, &mut q);
            let n = run(&mut nodes, q, lifo);
            assert!(n <= 6, "no ping-pong of identical content ({n} deliveries)");
            for n in nodes.values() {
                assert_eq!(n.content.as_ref().unwrap().0, 600);
            }
        }
    }

    #[test]
    fn a_copy_reaches_everyone_once_and_never_returns() {
        let mut nodes = star();
        let mut q = Vec::new();
        copy(&mut nodes, "a", 7, 1, &mut q);
        let mut log = Vec::new();
        while !q.is_empty() {
            let m = q.remove(0);
            log.push((m.0.clone(), m.1.clone()));
            let mut out = Vec::new();
            deliver(&mut nodes, m, &mut out);
            q.extend(out);
        }
        assert_eq!(log, [("a", "h"), ("h", "b"), ("h", "c")].map(|(a, b)| (a.to_string(), b.to_string())));
        for n in nodes.values() {
            assert_eq!(n.content.as_ref().unwrap().0, 7);
        }
    }
}
