//! File transfer between paired computers: the wire format of channel
//! [`features::channel::FILES`](crate::features::channel::FILES), file-name sanitisation,
//! destination naming, resume bookkeeping and the per-file receive state. The disk and
//! thread work lives in `moush-engine`; everything here is platform independent.
//!
//! Protocol (S = sender, R = receiver). Every body starts with its kind byte and the
//! sender-chosen 64-bit transfer id:
//!
//! 1. S → R `Offer` parts: the file list (bare names and sizes), split across as many
//!    messages as needed; parts arrive in order. Folder transfers use `OfferV2` parts,
//!    whose entries carry a relative path (`Photos/2024/a.jpg`) and a directory flag.
//! 2. R → S `Accept` parts ending with `last = true` (a sparse list of files R skips or
//!    already holds a prefix of), or `Reject`.
//! 3. S → R `Data` chunks in file order with at most `window` bytes unacknowledged;
//!    R → S `Ack` once a chunk is written to disk, so a slow disk slows the sender down.
//!    S → R `FileDone` with the file's BLAKE2s-256 after its last chunk.
//! 4. R → S `Complete` once every file is verified and renamed into place, or `Error`.
//!
//! Either side may send `Cancel` at any time.
//!
//! Resume: R keeps partial data in `<name>.<tag>.moushpart` in the destination folder, where
//! the tag derives from the transfer id. When S offers the same transfer id again (retry),
//! R reports how many bytes of each file it holds and the hash of that prefix; S hashes its
//! own prefix and continues from there when they match, or restarts the file at offset 0
//! (R truncates when a file's data restarts at 0). The whole-file hash in `FileDone` covers
//! resumed and new bytes alike, so a bad resume can never produce a corrupt file.

use std::collections::HashSet;
use std::io::{self, Read};

use blake2::{Blake2s256, Digest};

use crate::protocol::{DecodeError, MAX_FEATURE_BODY, Reader, wire};

pub type Hash = [u8; 32];
/// Running BLAKE2s-256 state of a file being sent or received.
pub type HashState = Blake2s256;

/// Message kinds (first byte of every body).
pub mod kind {
    pub const OFFER: u8 = 1;
    pub const ACCEPT: u8 = 2;
    pub const REJECT: u8 = 3;
    pub const DATA: u8 = 4;
    pub const FILE_DONE: u8 = 5;
    pub const ACK: u8 = 6;
    pub const COMPLETE: u8 = 7;
    pub const CANCEL: u8 = 8;
    pub const ERROR: u8 = 9;
    /// Offer whose entries carry relative paths and an empty-folder flag.
    pub const OFFER_V2: u8 = 10;
}

/// Most files one offer may contain.
pub const MAX_FILES: u32 = 10_000;
/// Largest total size one offer may announce (receivers also check free space).
pub const MAX_TOTAL_BYTES: u64 = 4 << 40;
/// Bytes of a `Data` body before the payload: kind, id, index, offset, length.
pub const DATA_HEADER: usize = 1 + 8 + 4 + 8 + 4;
/// Largest payload of one `Data` message.
pub const CHUNK: usize = MAX_FEATURE_BODY - DATA_HEADER;
/// `Resume::offset` value meaning "the receiver skips this file".
pub const SKIP: u64 = u64::MAX;
/// Longest file name kept, in UTF-16 units (leaves room for " (n)" and the part suffix
/// within the 255-unit NTFS limit).
pub const MAX_NAME_UNITS: usize = 180;
/// Longest relative path of one entry, in UTF-16 units. Long paths work because the
/// receiver writes below a canonical (`\\?\`) folder.
pub const MAX_PATH_UNITS: usize = 512;
/// Longest encoded relative path: [`MAX_PATH_UNITS`] four-byte UTF-8 characters.
pub const MAX_PATH_BYTES: usize = MAX_PATH_UNITS * 4;
pub const PART_SUFFIX: &str = ".moushpart";
const OFFER_HEADER: usize = 1 + 8 + 4 + 8 + 4 + 2;
const ACCEPT_HEADER: usize = 1 + 8 + 1 + 2;
const RESUME_LEN: usize = 4 + 8 + 32;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Path relative to the transfer root, `/`-separated. A single file's path is its
    /// name; a folder transfer starts with the selected folder's name.
    pub path: String,
    pub size: u64,
    /// An empty folder to create on the receiving side. It carries no data.
    pub is_dir: bool,
}

impl FileEntry {
    /// The last path component, for display.
    pub fn name(&self) -> &str {
        self.path.rsplit('/').next().unwrap_or(&self.path)
    }
}

/// A file the receiver skips (`offset == SKIP`) or already holds `offset` bytes of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resume {
    pub index: u32,
    pub offset: u64,
    /// BLAKE2s-256 of the first `offset` bytes (zero when skipping).
    pub prefix: Hash,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Msg {
    Offer { id: u64, total_files: u32, total_bytes: u64, first: u32, files: Vec<FileEntry> },
    OfferV2 { id: u64, total_files: u32, total_bytes: u64, first: u32, files: Vec<FileEntry> },
    Accept { id: u64, last: bool, resume: Vec<Resume> },
    Reject { id: u64, reason: String },
    Data { id: u64, index: u32, offset: u64, data: Vec<u8> },
    FileDone { id: u64, index: u32, size: u64, hash: Hash },
    Ack { id: u64, bytes: u32 },
    Complete { id: u64 },
    Cancel { id: u64, reason: String },
    Error { id: u64, reason: String },
}

impl Msg {
    pub fn id(&self) -> u64 {
        match self {
            Msg::Offer { id, .. }
            | Msg::OfferV2 { id, .. }
            | Msg::Accept { id, .. }
            | Msg::Reject { id, .. }
            | Msg::Data { id, .. }
            | Msg::FileDone { id, .. }
            | Msg::Ack { id, .. }
            | Msg::Complete { id }
            | Msg::Cancel { id, .. }
            | Msg::Error { id, .. } => *id,
        }
    }

    /// Encodes one message. `Offer` and `Accept` lists must fit one body; use
    /// [`offer_parts`] and [`accept_parts`] to split long lists.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        match self {
            Msg::Offer { id, total_files, total_bytes, first, files } => {
                head(&mut out, kind::OFFER, *id);
                wire::put_u32(&mut out, *total_files);
                wire::put_u64(&mut out, *total_bytes);
                wire::put_u32(&mut out, *first);
                wire::put_u16(&mut out, files.len() as u16);
                for f in files {
                    wire::put_str8(&mut out, &f.path);
                    wire::put_u64(&mut out, f.size);
                }
            }
            Msg::OfferV2 { id, total_files, total_bytes, first, files } => {
                head(&mut out, kind::OFFER_V2, *id);
                wire::put_u32(&mut out, *total_files);
                wire::put_u64(&mut out, *total_bytes);
                wire::put_u32(&mut out, *first);
                wire::put_u16(&mut out, files.len() as u16);
                for f in files {
                    wire::put_bytes(&mut out, f.path.as_bytes());
                    wire::put_u64(&mut out, f.size);
                    wire::put_bool(&mut out, f.is_dir);
                }
            }
            Msg::Accept { id, last, resume } => {
                head(&mut out, kind::ACCEPT, *id);
                wire::put_bool(&mut out, *last);
                wire::put_u16(&mut out, resume.len() as u16);
                for r in resume {
                    wire::put_u32(&mut out, r.index);
                    wire::put_u64(&mut out, r.offset);
                    out.extend_from_slice(&r.prefix);
                }
            }
            Msg::Reject { id, reason } => text(&mut out, kind::REJECT, *id, reason),
            Msg::Data { id, index, offset, data } => return encode_data(*id, *index, *offset, data),
            Msg::FileDone { id, index, size, hash } => {
                head(&mut out, kind::FILE_DONE, *id);
                wire::put_u32(&mut out, *index);
                wire::put_u64(&mut out, *size);
                out.extend_from_slice(hash);
            }
            Msg::Ack { id, bytes } => {
                head(&mut out, kind::ACK, *id);
                wire::put_u32(&mut out, *bytes);
            }
            Msg::Complete { id } => head(&mut out, kind::COMPLETE, *id),
            Msg::Cancel { id, reason } => text(&mut out, kind::CANCEL, *id, reason),
            Msg::Error { id, reason } => text(&mut out, kind::ERROR, *id, reason),
        }
        out
    }

    /// Decodes and validates one body. Never panics; trailing bytes are an error.
    pub fn decode(body: &[u8]) -> Result<Msg, DecodeError> {
        let mut r = Reader::new(body);
        let k = r.u8()?;
        let id = r.u64()?;
        let msg = match k {
            kind::OFFER => {
                let total_files = r.u32()?;
                let total_bytes = r.u64()?;
                let first = r.u32()?;
                let count = r.u16()? as u32;
                check_offer_header(total_files, total_bytes, first, count)?;
                let mut files = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let path = r.str8()?;
                    let size = r.u64()?;
                    if path.is_empty() || size > MAX_TOTAL_BYTES {
                        return Err(DecodeError::Invalid("file entry"));
                    }
                    files.push(FileEntry { path, size, is_dir: false });
                }
                Msg::Offer { id, total_files, total_bytes, first, files }
            }
            kind::OFFER_V2 => {
                let total_files = r.u32()?;
                let total_bytes = r.u64()?;
                let first = r.u32()?;
                let count = r.u16()? as u32;
                check_offer_header(total_files, total_bytes, first, count)?;
                let mut files = Vec::with_capacity(count as usize);
                for _ in 0..count {
                    let raw = r.bytes(MAX_PATH_BYTES)?;
                    let path = std::str::from_utf8(raw).map_err(|_| DecodeError::Invalid("path utf-8"))?;
                    let size = r.u64()?;
                    let is_dir = r.bool()?;
                    if path.is_empty()
                        || path.chars().any(char::is_control)
                        || path_units(path) > MAX_PATH_UNITS
                        || size > MAX_TOTAL_BYTES
                        || (is_dir && size != 0)
                    {
                        return Err(DecodeError::Invalid("file entry"));
                    }
                    files.push(FileEntry { path: path.to_owned(), size, is_dir });
                }
                Msg::OfferV2 { id, total_files, total_bytes, first, files }
            }
            kind::ACCEPT => {
                let last = r.bool()?;
                let count = r.u16()? as usize;
                if count * RESUME_LEN != r.remaining() {
                    return Err(DecodeError::Invalid("accept length"));
                }
                let mut resume = Vec::with_capacity(count);
                for _ in 0..count {
                    let index = r.u32()?;
                    let offset = r.u64()?;
                    let prefix: Hash = r.take(32)?.try_into().unwrap();
                    resume.push(Resume { index, offset, prefix });
                }
                Msg::Accept { id, last, resume }
            }
            kind::REJECT => Msg::Reject { id, reason: r.str8()? },
            kind::DATA => {
                let index = r.u32()?;
                let offset = r.u64()?;
                let data = r.bytes(CHUNK)?;
                if data.is_empty() || offset.checked_add(data.len() as u64).is_none() {
                    return Err(DecodeError::Invalid("data chunk"));
                }
                Msg::Data { id, index, offset, data: data.to_vec() }
            }
            kind::FILE_DONE => {
                let index = r.u32()?;
                let size = r.u64()?;
                let hash: Hash = r.take(32)?.try_into().unwrap();
                Msg::FileDone { id, index, size, hash }
            }
            kind::ACK => {
                let bytes = r.u32()?;
                if bytes == 0 || bytes as usize > CHUNK {
                    return Err(DecodeError::Invalid("ack size"));
                }
                Msg::Ack { id, bytes }
            }
            kind::COMPLETE => Msg::Complete { id },
            kind::CANCEL => Msg::Cancel { id, reason: r.str8()? },
            kind::ERROR => Msg::Error { id, reason: r.str8()? },
            _ => return Err(DecodeError::Invalid("transfer message kind")),
        };
        r.finish()?;
        Ok(msg)
    }
}

fn head(out: &mut Vec<u8>, k: u8, id: u64) {
    wire::put_u8(out, k);
    wire::put_u64(out, id);
}

fn check_offer_header(total_files: u32, total_bytes: u64, first: u32, count: u32) -> Result<(), DecodeError> {
    if total_files == 0 || total_files > MAX_FILES {
        return Err(DecodeError::Invalid("file count"));
    }
    if total_bytes > MAX_TOTAL_BYTES {
        return Err(DecodeError::Invalid("total size"));
    }
    if count == 0 || first.checked_add(count).is_none_or(|end| end > total_files) {
        return Err(DecodeError::Invalid("offer part range"));
    }
    Ok(())
}

fn text(out: &mut Vec<u8>, k: u8, id: u64, reason: &str) {
    head(out, k, id);
    let clean: String = reason.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    wire::put_str8(out, &clean);
}

/// Encodes a data chunk without copying it into a [`Msg`] first.
pub fn encode_data(id: u64, index: u32, offset: u64, data: &[u8]) -> Vec<u8> {
    debug_assert!(!data.is_empty() && data.len() <= CHUNK);
    let mut out = Vec::with_capacity(DATA_HEADER + data.len());
    head(&mut out, kind::DATA, id);
    wire::put_u32(&mut out, index);
    wire::put_u64(&mut out, offset);
    wire::put_bytes(&mut out, data);
    out
}

/// Splits a file list into `Offer` bodies that each fit [`MAX_FEATURE_BODY`]. Plain file
/// lists use the original `Offer` format; anything with a path separator, an empty folder
/// or a name longer than 255 bytes uses `OfferV2` (`path`, `size`, `is_dir` per entry).
pub fn offer_parts(id: u64, files: &[FileEntry]) -> Vec<Vec<u8>> {
    let total_files = files.len() as u32;
    let total_bytes = files.iter().map(|f| f.size).sum();
    let v2 = files.iter().any(|f| f.is_dir || f.path.contains('/') || f.path.len() > 255);
    let mut parts = Vec::new();
    let mut first = 0usize;
    while first < files.len() {
        let mut len = OFFER_HEADER;
        let mut end = first;
        while end < files.len() && end - first < u16::MAX as usize {
            let entry = if v2 { 4 + files[end].path.len() + 8 + 1 } else { 1 + files[end].path.len().min(255) + 8 };
            if len + entry > MAX_FEATURE_BODY {
                break;
            }
            len += entry;
            end += 1;
        }
        let chunk = files[first..end].to_vec();
        let msg = if v2 {
            Msg::OfferV2 { id, total_files, total_bytes, first: first as u32, files: chunk }
        } else {
            Msg::Offer { id, total_files, total_bytes, first: first as u32, files: chunk }
        };
        parts.push(msg.encode());
        first = end;
    }
    parts
}

/// Splits a resume list into `Accept` bodies; the last one has `last = true`. Always
/// returns at least one body.
pub fn accept_parts(id: u64, resume: &[Resume]) -> Vec<Vec<u8>> {
    let per = (MAX_FEATURE_BODY - ACCEPT_HEADER) / RESUME_LEN;
    let chunks: Vec<&[Resume]> = if resume.is_empty() { vec![&[]] } else { resume.chunks(per).collect() };
    let n = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(i, c)| Msg::Accept { id, last: i + 1 == n, resume: c.to_vec() }.encode())
        .collect()
}

// ---------------------------------------------------------------- offer / accept assembly

/// Collects the parts of an incoming offer.
#[derive(Debug)]
pub struct OfferAssembler {
    pub id: u64,
    total_files: u32,
    total_bytes: u64,
    files: Vec<FileEntry>,
}

impl OfferAssembler {
    pub fn new(id: u64, total_files: u32, total_bytes: u64) -> Self {
        Self { id, total_files, total_bytes, files: Vec::new() }
    }

    pub fn total_files(&self) -> u32 {
        self.total_files
    }

    pub fn total_bytes(&self) -> u64 {
        self.total_bytes
    }

    /// Adds a decoded part. Returns `Ok(true)` once the list is complete.
    pub fn push(
        &mut self,
        total_files: u32,
        total_bytes: u64,
        first: u32,
        files: Vec<FileEntry>,
    ) -> Result<bool, &'static str> {
        if total_files != self.total_files || total_bytes != self.total_bytes {
            return Err("offer parts disagree");
        }
        if first as usize != self.files.len() || self.files.len() + files.len() > self.total_files as usize {
            return Err("offer parts out of order");
        }
        self.files.extend(files);
        if self.files.len() < self.total_files as usize {
            return Ok(false);
        }
        let sum = self.files.iter().try_fold(0u64, |a, f| a.checked_add(f.size));
        if sum != Some(self.total_bytes) {
            return Err("file sizes do not add up");
        }
        Ok(true)
    }

    pub fn into_files(self) -> Vec<FileEntry> {
        self.files
    }
}

/// Where the sender starts each file, from the receiver's `Accept` parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Start {
    Skip,
    /// Continue at `offset` if the sender's own prefix hashes to `prefix` (or 0 when
    /// `offset == 0`).
    At {
        offset: u64,
        prefix: Hash,
    },
}

/// Collects `Accept` parts on the sending side.
#[derive(Debug)]
pub struct AcceptAssembler {
    starts: Vec<Start>,
    next_index: u32,
    done: bool,
}

impl AcceptAssembler {
    pub fn new(file_count: usize) -> Self {
        Self { starts: vec![Start::At { offset: 0, prefix: [0; 32] }; file_count], next_index: 0, done: false }
    }

    /// Adds a part, validated against the offered `files`. Returns `Ok(true)` after the last part.
    pub fn push(&mut self, last: bool, resume: &[Resume], files: &[FileEntry]) -> Result<bool, &'static str> {
        if self.done {
            return Err("accept after the last part");
        }
        for r in resume {
            if r.index < self.next_index {
                return Err("accept entries out of order");
            }
            let Some(file) = files.get(r.index as usize) else { return Err("accept entry out of range") };
            self.starts[r.index as usize] = if r.offset == SKIP {
                Start::Skip
            } else if r.offset <= file.size {
                Start::At { offset: r.offset, prefix: r.prefix }
            } else {
                return Err("resume offset beyond the end of the file");
            };
            self.next_index = r.index + 1;
        }
        self.done = last;
        Ok(last)
    }

    pub fn starts(&self) -> &[Start] {
        &self.starts
    }
}

/// The offset the sender continues a file at: the receiver's offset when both prefixes
/// match, otherwise 0 (the receiver truncates when data restarts at 0).
pub fn resume_from(offset: u64, receiver_prefix: &Hash, own_prefix: &Hash) -> u64 {
    if offset > 0 && receiver_prefix == own_prefix { offset } else { 0 }
}

/// Hashes the first `len` bytes of `r` in 64 KiB steps, stopping early when `cancelled()`
/// returns true. Returns the running hash state so the caller can continue with the rest
/// of the file. Fails if `r` ends before `len` bytes.
pub fn hash_prefix<R: Read>(r: &mut R, len: u64, cancelled: impl Fn() -> bool) -> io::Result<Blake2s256> {
    let mut hasher = Blake2s256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut left = len;
    while left > 0 {
        if cancelled() {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
        let n = r.read(&mut buf[..want])?;
        if n == 0 {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "file is shorter than expected"));
        }
        hasher.update(&buf[..n]);
        left -= n as u64;
    }
    Ok(hasher)
}

pub fn hash_update(hasher: &mut HashState, data: &[u8]) {
    hasher.update(data);
}

pub fn finish_hash(hasher: &Blake2s256) -> Hash {
    hasher.clone().finalize().into()
}

// ---------------------------------------------------------------- receiving one file

/// How the receiver must apply a data chunk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteAt {
    /// Append at the current end of the partial file.
    Append,
    /// The sender restarted this file: truncate the partial file first.
    Restart,
}

/// Receive-side state of one file: how much is written and the hash of those bytes.
#[derive(Clone, Debug)]
pub struct ReceiveFile {
    size: u64,
    written: u64,
    hasher: Blake2s256,
}

impl ReceiveFile {
    pub fn new(size: u64) -> Self {
        Self { size, written: 0, hasher: Blake2s256::new() }
    }

    /// Continues after `written` bytes already on disk whose hash state is `hasher`.
    pub fn resumed(size: u64, written: u64, hasher: Blake2s256) -> Self {
        Self { size, written: written.min(size), hasher }
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    /// Validates a chunk before it is written. Data must continue exactly where the file
    /// ends, or restart at 0, and never run past the announced size.
    pub fn check(&self, offset: u64, len: usize) -> Result<WriteAt, &'static str> {
        if len == 0 {
            return Err("empty data chunk");
        }
        let at = if offset == self.written {
            WriteAt::Append
        } else if offset == 0 {
            WriteAt::Restart
        } else {
            return Err("data out of order");
        };
        match offset.checked_add(len as u64) {
            Some(end) if end <= self.size => Ok(at),
            _ => Err("more data than announced"),
        }
    }

    /// Records a chunk written to disk (after [`check`](Self::check) approved it).
    pub fn commit(&mut self, at: WriteAt, data: &[u8]) {
        if at == WriteAt::Restart {
            self.written = 0;
            self.hasher = Blake2s256::new();
        }
        self.hasher.update(data);
        self.written += data.len() as u64;
    }

    /// Checks the sender's `FileDone` against what was received.
    pub fn verify(&self, size: u64, hash: &Hash) -> Result<(), &'static str> {
        if size != self.size || self.written != self.size {
            return Err("file is incomplete");
        }
        if finish_hash(&self.hasher) != *hash {
            return Err("file was corrupted in transit (hash mismatch)");
        }
        Ok(())
    }
}

/// Where the receiver resumes a file whose partial copy holds `part_len` bytes: there if
/// it is not longer than the file, otherwise 0 (start over).
pub fn receive_offset(part_len: u64, size: u64) -> u64 {
    if part_len <= size { part_len } else { 0 }
}

// ---------------------------------------------------------------- names

/// Characters Windows does not allow in file names.
fn forbidden(c: char) -> bool {
    c.is_control() || matches!(c, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
}

/// Invisible formatting characters that can disguise a name (e.g. right-to-left override
/// turning "photo\u{202E}gpj.exe" into "photoexe.jpg").
fn invisible(c: char) -> bool {
    matches!(c, '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2069}' | '\u{FEFF}')
}

fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or("").trim_end_matches(' ').to_uppercase();
    if matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") {
        return true;
    }
    let mut chars = stem.chars();
    let prefix: String = chars.by_ref().take(3).collect();
    let rest: Vec<char> = chars.collect();
    (prefix == "COM" || prefix == "LPT")
        && rest.len() == 1
        && (rest[0].is_ascii_digit() || matches!(rest[0], '¹' | '²' | '³'))
}

/// Splits "name.ext" into ("name", ".ext"). A leading dot is part of the name; extensions
/// longer than 16 characters are not treated as extensions.
fn split_ext(name: &str) -> (&str, &str) {
    match name.rfind('.') {
        Some(i) if i > 0 && name.len() - i <= 17 => (&name[..i], &name[i..]),
        _ => (name, ""),
    }
}

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

fn limit_length(name: &str, max_units: usize) -> String {
    if utf16_len(name) <= max_units {
        return name.to_owned();
    }
    let (stem, ext) = split_ext(name);
    let budget = max_units.saturating_sub(utf16_len(ext));
    let mut out = String::new();
    let mut used = 0;
    for c in stem.chars() {
        if used + c.len_utf16() > budget {
            break;
        }
        used += c.len_utf16();
        out.push(c);
    }
    let out = out.trim_end_matches(['.', ' ']);
    format!("{out}{ext}")
}

/// Files that Explorer reads when it merely *displays* a folder; their icon or location
/// fields can point at a remote share and leak the user's NTLM credentials without anything
/// being opened. Received ones get `.txt` appended so they stay inert.
const SHELL_ACTIVE_EXTENSIONS: [&str; 6] = ["scf", "url", "lnk", "library-ms", "searchconnector-ms", "website"];
const SHELL_ACTIVE_NAMES: [&str; 3] = ["desktop.ini", "autorun.inf", "folder.htt"];

fn is_shell_active(name: &str) -> bool {
    let lower = name.to_lowercase();
    SHELL_ACTIVE_NAMES.contains(&lower.as_str())
        || lower.rsplit_once('.').is_some_and(|(_, ext)| SHELL_ACTIVE_EXTENSIONS.contains(&ext))
}

/// Turns any string into a safe bare file name for Windows, or `None` if nothing usable
/// remains. Keeps only the last path component, replaces characters Windows forbids
/// (including `:`, which would address an alternate data stream), drops invisible
/// formatting characters, trims trailing dots and spaces (Windows strips them, which could
/// otherwise alias another name), renames reserved device names (`con.txt` →
/// `_con.txt`) and limits the length while keeping the extension.
pub fn sanitize_name(raw: &str) -> Option<String> {
    sanitize_component(raw.rsplit(['/', '\\']).next().unwrap_or(""))
}

/// Like [`sanitize_name`] for one component of a relative path (no separators are kept).
fn sanitize_component(raw: &str) -> Option<String> {
    let replaced: String = raw.chars().filter(|c| !invisible(*c)).map(|c| if forbidden(c) { '_' } else { c }).collect();
    let trimmed = replaced.trim_start_matches(' ').trim_end_matches(['.', ' ']);
    if trimmed.is_empty() {
        return None;
    }
    let mut name = trimmed.to_owned();
    if is_reserved(&name) {
        name.insert(0, '_');
    }
    // Never let a received name look like one of our partial files.
    if name.to_lowercase().ends_with(PART_SUFFIX) {
        name.push('_');
    }
    if is_shell_active(&name) {
        name.push_str(".txt");
    }
    let name = limit_length(&name, MAX_NAME_UNITS);
    (!name.is_empty()).then_some(name)
}

/// Turns a relative path ("Photos/2024/a.jpg") into a safe path, sanitizing every
/// component like a file name. Returns `None` when a component leaves nothing usable.
pub fn sanitize_path(raw: &str) -> Option<String> {
    let mut out = String::with_capacity(raw.len());
    for part in raw.split('/') {
        let name = sanitize_component(part)?;
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&name);
    }
    (!out.is_empty()).then_some(out)
}

/// Validates a relative path from an offer. Paths that try to leave the destination folder
/// (backslashes, absolute paths, drive letters, alternate data streams, empty or `.`/`..`
/// components) refuse the whole offer; anything else is made safe with [`sanitize_path`].
pub fn check_offered_path(raw: &str) -> Result<String, &'static str> {
    if raw.contains('\\') {
        return Err("a file name contains a folder path");
    }
    if raw.contains(':') {
        return Err("a file name contains a drive or stream name");
    }
    if raw.starts_with('/')
        || raw.ends_with('/')
        || raw.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err("a file name contains a folder path");
    }
    let path = sanitize_path(raw).ok_or("a file name is not valid")?;
    if path_units(&path) > MAX_PATH_UNITS {
        return Err("a file path is too long");
    }
    Ok(path)
}

/// Length of a relative path in UTF-16 units.
pub fn path_units(s: &str) -> usize {
    utf16_len(s)
}

/// "name (n).ext".
pub fn numbered_name(name: &str, n: u32) -> String {
    let (stem, ext) = split_ext(name);
    format!("{stem} ({n}){ext}")
}

fn numbered_path(path: &str, n: u32) -> String {
    match path.rsplit_once('/') {
        Some((dir, name)) => format!("{dir}/{}", numbered_name(name, n)),
        None => numbered_name(path, n),
    }
}

/// Replaces the last component of a relative path.
pub fn replace_name(path: &str, name: &str) -> String {
    match path.rsplit_once('/') {
        Some((dir, _)) => format!("{dir}/{name}"),
        None => name.to_owned(),
    }
}

/// Makes paths unique within one transfer (case-insensitively, like NTFS), numbering the
/// last component of later duplicates.
pub fn dedupe_paths(files: &mut [FileEntry]) {
    let mut seen: HashSet<String> = HashSet::with_capacity(files.len());
    for f in files.iter_mut() {
        if seen.insert(f.path.to_lowercase()) {
            continue;
        }
        let original = f.path.clone();
        for n in 1.. {
            let candidate = numbered_path(&original, n);
            if seen.insert(candidate.to_lowercase()) {
                f.path = candidate;
                break;
            }
        }
    }
}

/// True when an entry is both a file and a folder, or a file sits where another entry needs
/// a folder (which would make receiving impossible).
fn has_path_conflict(files: &[FileEntry]) -> bool {
    let mut file_paths = HashSet::new();
    let mut dir_paths = HashSet::new();
    for f in files {
        if f.is_dir {
            dir_paths.insert(f.path.to_lowercase());
        } else {
            file_paths.insert(f.path.to_lowercase());
        }
    }
    if !file_paths.is_disjoint(&dir_paths) {
        return true;
    }
    for f in files {
        let lower = f.path.to_lowercase();
        let mut prefix = String::new();
        let mut parts = lower.split('/').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                break;
            }
            if !prefix.is_empty() {
                prefix.push('/');
            }
            prefix.push_str(part);
            if file_paths.contains(&prefix) {
                return true;
            }
        }
    }
    false
}

/// Sanitizes and validates the entries of an offer before anything is accepted: every path
/// is made safe, duplicates are numbered, and impossible file/folder layouts are refused.
pub fn prepare_offer(files: &[FileEntry]) -> Result<Vec<FileEntry>, &'static str> {
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let path = check_offered_path(&f.path)?;
        if f.is_dir && f.size != 0 {
            return Err("a folder entry has a size");
        }
        out.push(FileEntry { path, size: f.size, is_dir: f.is_dir });
    }
    dedupe_paths(&mut out);
    if has_path_conflict(&out) {
        return Err("an item is both a file and a folder");
    }
    Ok(out)
}

/// What to do when a received file's name already exists in the destination folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Overwrite {
    /// Save as "name (1).ext", "name (2).ext", ...
    Rename,
    /// Replace the existing file.
    Replace,
    /// Keep the existing file and do not receive this one.
    Skip,
}

impl Overwrite {
    /// From `TransferSettings::overwrite` ("rename", "overwrite", "skip").
    pub fn from_setting(s: &str) -> Self {
        match s {
            "overwrite" => Overwrite::Replace,
            "skip" => Overwrite::Skip,
            _ => Overwrite::Rename,
        }
    }
}

/// The name a received file is saved under, or `None` to skip it. `taken(name)` tells
/// whether a name already exists in the destination folder.
pub fn destination_name(name: &str, policy: Overwrite, taken: impl Fn(&str) -> bool) -> Option<String> {
    if !taken(name) {
        return Some(name.to_owned());
    }
    match policy {
        Overwrite::Replace => Some(name.to_owned()),
        Overwrite::Skip => None,
        Overwrite::Rename => (1..=9999).map(|n| numbered_name(name, n)).find(|c| !taken(c)),
    }
}

/// Name of the partial file a transfer writes `rel_path` into. The full relative path is
/// hashed, so two files with the same name in different folders never share a part file.
pub fn part_name(rel_path: &str, id: u64) -> String {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let mut hasher = Blake2s256::new();
    hasher.update(id.to_le_bytes());
    hasher.update(rel_path.as_bytes());
    let tag = u64::from_le_bytes(hasher.finalize()[..8].try_into().unwrap());
    format!("{name}.{tag:016x}{PART_SUFFIX}")
}

// ---------------------------------------------------------------- progress

/// Smoothed transfer rate in bytes per second.
#[derive(Clone, Debug, Default)]
pub struct RateMeter {
    last_bytes: u64,
    last_ms: u64,
    rate: f64,
}

impl RateMeter {
    /// Feeds the total byte count at `now_ms`; returns the smoothed rate.
    pub fn update(&mut self, bytes: u64, now_ms: u64) -> f64 {
        if self.last_ms == 0 || bytes < self.last_bytes {
            *self = Self { last_bytes: bytes, last_ms: now_ms.max(1), rate: 0.0 };
            return 0.0;
        }
        let dt = now_ms.saturating_sub(self.last_ms);
        if dt < 200 {
            return self.rate;
        }
        let inst = (bytes - self.last_bytes) as f64 * 1000.0 / dt as f64;
        self.rate = if self.rate == 0.0 { inst } else { self.rate * 0.7 + inst * 0.3 };
        self.last_bytes = bytes;
        self.last_ms = now_ms;
        self.rate
    }

    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files(n: usize, name_len: usize) -> Vec<FileEntry> {
        (0..n)
            .map(|i| FileEntry { path: format!("{i:05}{}", "x".repeat(name_len)), size: i as u64 * 10, is_dir: false })
            .collect()
    }

    #[test]
    fn every_message_round_trips() {
        let msgs = vec![
            Msg::Offer {
                id: 7,
                total_files: 2,
                total_bytes: 3,
                first: 0,
                files: vec![FileEntry { path: "a.txt".into(), size: 1, is_dir: false }],
            },
            Msg::OfferV2 {
                id: 7,
                total_files: 2,
                total_bytes: 3,
                first: 0,
                files: vec![
                    FileEntry { path: "Photos/2024/a.jpg".into(), size: 3, is_dir: false },
                    FileEntry { path: "Photos/empty".into(), size: 0, is_dir: true },
                ],
            },
            Msg::Accept { id: 7, last: true, resume: vec![Resume { index: 1, offset: 5, prefix: [9; 32] }] },
            Msg::Accept { id: 7, last: false, resume: vec![] },
            Msg::Reject { id: 7, reason: "no".into() },
            Msg::Data { id: 7, index: 3, offset: 1 << 40, data: vec![1, 2, 3] },
            Msg::FileDone { id: 7, index: 3, size: 99, hash: [4; 32] },
            Msg::Ack { id: u64::MAX, bytes: 100 },
            Msg::Complete { id: 1 },
            Msg::Cancel { id: 2, reason: "stop".into() },
            Msg::Error { id: 3, reason: "disk full".into() },
        ];
        for m in msgs {
            let body = m.encode();
            assert!(body.len() <= MAX_FEATURE_BODY);
            assert_eq!(Msg::decode(&body).unwrap(), m);
        }
    }

    #[test]
    fn largest_data_chunk_fits_one_body() {
        let body = encode_data(1, 0, 0, &vec![0xAB; CHUNK]);
        assert_eq!(body.len(), MAX_FEATURE_BODY);
        assert!(matches!(Msg::decode(&body).unwrap(), Msg::Data { data, .. } if data.len() == CHUNK));
    }

    #[test]
    fn hostile_bodies_are_rejected_without_panicking() {
        let good = Msg::OfferV2 {
            id: 1,
            total_files: 1,
            total_bytes: 5,
            first: 0,
            files: vec![FileEntry { path: "Photos/a".into(), size: 5, is_dir: false }],
        }
        .encode();
        // Every truncation and trailing garbage fail cleanly.
        for n in 0..good.len() {
            assert!(Msg::decode(&good[..n]).is_err(), "truncated at {n}");
        }
        let mut long = good.clone();
        long.push(0);
        assert!(Msg::decode(&long).is_err());
        // Zero and too many files, ranges past the total, unknown kinds.
        let bad = [
            Msg::Offer { id: 1, total_files: 0, total_bytes: 0, first: 0, files: vec![] }.encode(),
            Msg::Offer { id: 1, total_files: MAX_FILES + 1, total_bytes: 0, first: 0, files: files(1, 1) }.encode(),
            Msg::Offer { id: 1, total_files: 2, total_bytes: 0, first: 2, files: files(1, 1) }.encode(),
            Msg::Offer { id: 1, total_files: u32::MAX, total_bytes: 0, first: u32::MAX, files: files(1, 1) }.encode(),
            Msg::Offer { id: 1, total_files: 1, total_bytes: MAX_TOTAL_BYTES + 1, first: 0, files: files(1, 1) }
                .encode(),
            Msg::Offer {
                id: 1,
                total_files: 1,
                total_bytes: 0,
                first: 0,
                files: vec![FileEntry { path: String::new(), size: 0, is_dir: false }],
            }
            .encode(),
            Msg::OfferV2 {
                id: 1,
                total_files: 1,
                total_bytes: 0,
                first: 0,
                files: vec![FileEntry { path: "a".into(), size: 1, is_dir: true }],
            }
            .encode(),
            vec![99, 0, 0, 0, 0, 0, 0, 0, 0],
            Msg::Ack { id: 1, bytes: 0 }.encode(),
            Msg::Ack { id: 1, bytes: CHUNK as u32 + 1 }.encode(),
        ];
        for b in &bad {
            assert!(Msg::decode(b).is_err(), "{b:?}");
        }
        // Control characters in names, a data run longer than the chunk limit, a lying accept count.
        let mut ctl = Vec::new();
        head(&mut ctl, kind::REJECT, 1);
        ctl.extend_from_slice(&[3, b'a', 0x07, b'b']);
        assert!(Msg::decode(&ctl).is_err());
        let mut big = Vec::new();
        head(&mut big, kind::DATA, 1);
        wire::put_u32(&mut big, 0);
        wire::put_u64(&mut big, 0);
        wire::put_u32(&mut big, CHUNK as u32 + 1);
        big.extend(std::iter::repeat_n(0, CHUNK + 1));
        assert!(Msg::decode(&big).is_err());
        let mut acc =
            Msg::Accept { id: 1, last: true, resume: vec![Resume { index: 0, offset: 0, prefix: [0; 32] }] }.encode();
        acc[10] = 2; // claims two entries
        assert!(Msg::decode(&acc).is_err());
        // Data whose end overflows u64.
        let overflow = encode_data(1, 0, u64::MAX - 1, &[1, 2, 3]);
        assert!(Msg::decode(&overflow).is_err());
        // Random garbage never panics.
        let mut x: u32 = 12345;
        for len in 0..400 {
            let body: Vec<u8> = (0..len)
                .map(|_| {
                    x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    (x >> 16) as u8
                })
                .collect();
            let _ = Msg::decode(&body);
        }
    }

    #[test]
    fn long_lists_split_into_parts_and_reassemble() {
        let list = files(3000, 200);
        let parts = offer_parts(42, &list);
        assert!(parts.len() > 10);
        let mut asm: Option<OfferAssembler> = None;
        let mut complete = false;
        for p in &parts {
            assert!(p.len() <= MAX_FEATURE_BODY);
            let (id, total_files, total_bytes, first, files) = match Msg::decode(p).unwrap() {
                Msg::Offer { id, total_files, total_bytes, first, files }
                | Msg::OfferV2 { id, total_files, total_bytes, first, files } => {
                    (id, total_files, total_bytes, first, files)
                }
                other => panic!("unexpected {other:?}"),
            };
            let a = asm.get_or_insert_with(|| OfferAssembler::new(id, total_files, total_bytes));
            complete = a.push(total_files, total_bytes, first, files).unwrap();
        }
        assert!(complete);
        assert_eq!(asm.unwrap().into_files(), list);

        // Folder lists use OfferV2 and split the same way.
        let tree: Vec<FileEntry> = (0..3000)
            .map(|i| FileEntry { path: format!("Photos/{i:05}{}", "x".repeat(180)), size: 10, is_dir: false })
            .collect();
        let parts = offer_parts(43, &tree);
        assert!(parts.len() > 10);
        let mut asm: Option<OfferAssembler> = None;
        let mut complete = false;
        for p in &parts {
            assert!(p.len() <= MAX_FEATURE_BODY);
            let Msg::OfferV2 { id, total_files, total_bytes, first, files } = Msg::decode(p).unwrap() else { panic!() };
            let a = asm.get_or_insert_with(|| OfferAssembler::new(id, total_files, total_bytes));
            complete = a.push(total_files, total_bytes, first, files).unwrap();
        }
        assert!(complete);
        assert_eq!(asm.unwrap().into_files(), tree);

        let resume: Vec<Resume> = (0..2000).map(|i| Resume { index: i, offset: 1, prefix: [1; 32] }).collect();
        let parts = accept_parts(42, &resume);
        assert!(parts.len() > 1);
        let mut acc = AcceptAssembler::new(2000);
        let entries: Vec<FileEntry> =
            (0..2000).map(|i| FileEntry { path: i.to_string(), size: 10, is_dir: false }).collect();
        let mut last = false;
        for p in &parts {
            let Msg::Accept { last: l, resume, .. } = Msg::decode(p).unwrap() else { panic!() };
            last = acc.push(l, &resume, &entries).unwrap();
        }
        assert!(last);
        assert!(acc.starts().iter().all(|s| *s == Start::At { offset: 1, prefix: [1; 32] }));
        assert_eq!(accept_parts(1, &[]).len(), 1);
    }

    #[test]
    fn offer_assembly_rejects_inconsistent_parts() {
        let mut a = OfferAssembler::new(1, 2, 30);
        assert_eq!(a.push(2, 30, 1, files(1, 1)), Err("offer parts out of order"));
        assert_eq!(a.push(3, 30, 0, files(1, 1)), Err("offer parts disagree"));
        assert_eq!(a.push(2, 30, 0, files(1, 1)), Ok(false));
        // Sizes 0 + 10 != 30.
        assert_eq!(a.push(2, 30, 1, files(2, 1)[1..].to_vec()), Err("file sizes do not add up"));
    }

    #[test]
    fn accept_assembly_validates_entries() {
        let entries = vec![
            FileEntry { path: "a".into(), size: 10, is_dir: false },
            FileEntry { path: "b".into(), size: 10, is_dir: false },
        ];
        let r = |index, offset| Resume { index, offset, prefix: [0; 32] };
        assert!(AcceptAssembler::new(2).push(true, &[r(2, 0)], &entries).is_err(), "out of range");
        assert!(AcceptAssembler::new(2).push(true, &[r(0, 11)], &entries).is_err(), "beyond the end");
        assert!(AcceptAssembler::new(2).push(true, &[r(1, 1), r(0, 1)], &entries).is_err(), "out of order");
        let mut a = AcceptAssembler::new(2);
        assert_eq!(a.push(false, &[r(0, SKIP)], &entries), Ok(false));
        assert_eq!(a.push(true, &[r(1, 10)], &entries), Ok(true));
        assert_eq!(a.starts()[0], Start::Skip);
        assert_eq!(a.starts()[1], Start::At { offset: 10, prefix: [0; 32] });
        assert!(a.push(true, &[], &entries).is_err(), "nothing after the last part");
    }

    #[test]
    fn sanitizer_strips_paths_and_dangerous_names() {
        let s = |x: &str| sanitize_name(x);
        assert_eq!(s(r"..\..\x").as_deref(), Some("x"));
        assert_eq!(s("../../etc/passwd").as_deref(), Some("passwd"));
        assert_eq!(s(r"C:\Windows\system32\x.dll").as_deref(), Some("x.dll"));
        assert_eq!(s("C:x").as_deref(), Some("C_x"));
        assert_eq!(s("a:b").as_deref(), Some("a_b"), "no alternate data streams");
        assert_eq!(s("con.txt").as_deref(), Some("_con.txt"));
        assert_eq!(s("CON").as_deref(), Some("_CON"));
        assert_eq!(s("Lpt1.tar.gz").as_deref(), Some("_Lpt1.tar.gz"));
        assert_eq!(s("com¹").as_deref(), Some("_com¹"));
        assert_eq!(s("aux .txt").as_deref(), Some("_aux .txt"));
        assert_eq!(s("console.txt").as_deref(), Some("console.txt"));
        assert_eq!(s("COM10").as_deref(), Some("COM10"));
        assert_eq!(s("report.pdf. . ").as_deref(), Some("report.pdf"));
        assert_eq!(s("..").as_deref(), None);
        assert_eq!(s(".").as_deref(), None);
        assert_eq!(s("   ").as_deref(), None);
        assert_eq!(s("dir/").as_deref(), None);
        assert_eq!(s(".gitignore").as_deref(), Some(".gitignore"));
        assert_eq!(s("a\u{7}b<c>|?*\".txt").as_deref(), Some("a_b_c_____.txt"));
        assert_eq!(s("photo\u{202E}gpj.exe").as_deref(), Some("photogpj.exe"));
        assert_eq!(s("Prüfung — 日本語 😀.txt").as_deref(), Some("Prüfung — 日本語 😀.txt"));
        assert_eq!(s("x.moushpart").as_deref(), Some("x.moushpart_"));
        // Files Explorer parses while showing the folder are kept inert.
        assert_eq!(s("Desktop.ini").as_deref(), Some("Desktop.ini.txt"));
        assert_eq!(s("share.SCF").as_deref(), Some("share.SCF.txt"));
        assert_eq!(s("site.url").as_deref(), Some("site.url.txt"));
        assert_eq!(s("x.searchConnector-ms").as_deref(), Some("x.searchConnector-ms.txt"));
        assert_eq!(s("notes.url.md").as_deref(), Some("notes.url.md"));
        // Length limit keeps the extension and counts UTF-16 units.
        let long = format!("{}.docx", "😀".repeat(200));
        let out = s(&long).unwrap();
        assert!(utf16_len(&out) <= MAX_NAME_UNITS);
        assert!(out.ends_with(".docx"));
        assert!(utf16_len(&part_name(&numbered_name(&out, 9999), u64::MAX)) <= 255);

        // Relative paths keep their folders, sanitize every component, and refuse
        // anything that could escape the destination.
        assert_eq!(sanitize_path("Photos/2024/a.jpg").as_deref(), Some("Photos/2024/a.jpg"));
        assert_eq!(sanitize_path("a b/con.txt/sub.url").as_deref(), Some("a b/_con.txt/sub.url.txt"));
        assert_eq!(sanitize_path("a//b"), None);
        assert_eq!(sanitize_path("a/../b"), None);
        assert_eq!(sanitize_path("a/."), None);
        assert_eq!(sanitize_path("/a"), None);
        assert_eq!(sanitize_path("a/"), None);
    }

    #[test]
    fn offered_paths_with_traversal_refuse_the_offer() {
        assert!(check_offered_path(r"..\..\x").is_err());
        assert!(check_offered_path("../x").is_err());
        assert!(check_offered_path("Photos/../../x").is_err());
        assert!(check_offered_path(r"C:\x").is_err());
        assert!(check_offered_path("C:x").is_err());
        assert!(check_offered_path("a:b").is_err());
        assert!(check_offered_path("..").is_err());
        assert!(check_offered_path(". .").is_err());
        assert!(check_offered_path("Photos//a").is_err());
        assert!(check_offered_path("/etc/passwd").is_err());
        assert!(check_offered_path("Photos/").is_err());
        let long = vec!["x".repeat(MAX_NAME_UNITS); 4].join("/");
        assert!(check_offered_path(&long).is_err(), "{} units", path_units(&long));
        assert_eq!(check_offered_path("con.txt").unwrap(), "_con.txt");
        assert_eq!(check_offered_path("trailing.").unwrap(), "trailing");
        assert_eq!(check_offered_path("ok.txt").unwrap(), "ok.txt");
        assert_eq!(check_offered_path("Photos/con.txt/ok.md").unwrap(), "Photos/_con.txt/ok.md");
        assert_eq!(check_offered_path("Photos/2024/x.jpg").unwrap(), "Photos/2024/x.jpg");
    }

    #[test]
    fn duplicate_paths_in_one_transfer_are_numbered() {
        let mut files = vec![
            FileEntry { path: "a.txt".into(), size: 1, is_dir: false },
            FileEntry { path: "A.TXT".into(), size: 1, is_dir: false },
            FileEntry { path: "a (1).txt".into(), size: 1, is_dir: false },
            FileEntry { path: "dir/a.txt".into(), size: 1, is_dir: false },
            FileEntry { path: "dir/A.TXT".into(), size: 1, is_dir: false },
            FileEntry { path: "b".into(), size: 1, is_dir: false },
        ];
        dedupe_paths(&mut files);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["a.txt", "A (1).TXT", "a (1) (1).txt", "dir/a.txt", "dir/A (1).TXT", "b"]);
    }

    #[test]
    fn offer_preparation_sanitizes_and_rejects_impossible_trees() {
        let f = |path: &str, size, is_dir| FileEntry { path: path.into(), size, is_dir };
        let prepared =
            prepare_offer(&[f("Photos", 0, true), f("Photos/a b.txt", 3, false), f("photos/../evil", 1, false)]);
        assert_eq!(prepared, Err("a file name contains a folder path"));

        let prepared = prepare_offer(&[f("Photos", 0, true), f("Photos/a.txt", 3, false)]).unwrap();
        assert_eq!(prepared[0].path, "Photos");
        assert_eq!(prepared[1].path, "Photos/a.txt");

        assert_eq!(prepare_offer(&[f("a", 1, false), f("a/b", 1, false)]), Err("an item is both a file and a folder"));
        let mixed = prepare_offer(&[f("a", 1, false), f("A", 0, true)]).unwrap();
        assert_eq!(mixed[1].path, "A (1)");
        assert_eq!(prepare_offer(&[f("a", 1, true)]), Err("a folder entry has a size"));
        assert_eq!(prepare_offer(&[f("a/b", 0, true), f("a/B", 0, true)]).unwrap()[1].path, "a/B (1)");
        assert!(prepare_offer(&[f("Photos/2024/a.jpg", 5, false)]).is_ok());
    }

    #[test]
    fn overwrite_policies() {
        let taken = |n: &str| ["report.pdf", "report (1).pdf", ".bashrc"].contains(&n);
        assert_eq!(destination_name("new.pdf", Overwrite::Skip, taken).as_deref(), Some("new.pdf"));
        assert_eq!(destination_name("report.pdf", Overwrite::Rename, taken).as_deref(), Some("report (2).pdf"));
        assert_eq!(destination_name("report.pdf", Overwrite::Replace, taken).as_deref(), Some("report.pdf"));
        assert_eq!(destination_name("report.pdf", Overwrite::Skip, taken), None);
        assert_eq!(destination_name(".bashrc", Overwrite::Rename, taken).as_deref(), Some(".bashrc (1)"));
        assert_eq!(numbered_name("archive.tar.gz", 3), "archive.tar (3).gz");
        assert_eq!(numbered_name("noext", 1), "noext (1)");
        assert_eq!(Overwrite::from_setting("overwrite"), Overwrite::Replace);
        assert_eq!(Overwrite::from_setting("skip"), Overwrite::Skip);
        assert_eq!(Overwrite::from_setting("bogus"), Overwrite::Rename);
    }

    #[test]
    fn receive_state_enforces_order_size_and_hash() {
        let data = b"hello world, this is a file";
        let mut f = ReceiveFile::new(data.len() as u64);
        assert_eq!(f.check(5, 3), Err("data out of order"));
        assert_eq!(f.check(0, data.len() + 1), Err("more data than announced"));
        assert_eq!(f.check(0, 0), Err("empty data chunk"));
        f.commit(f.check(0, 10).unwrap(), &data[..10]);
        assert_eq!(f.check(5, 1), Err("data out of order"));
        assert_eq!(f.verify(data.len() as u64, &[0; 32]), Err("file is incomplete"));
        f.commit(f.check(10, data.len() - 10).unwrap(), &data[10..]);
        let good: Hash = Blake2s256::digest(data).into();
        assert_eq!(f.verify(data.len() as u64, &good), Ok(()));
        assert!(f.verify(data.len() as u64, &[0; 32]).is_err());
        assert!(f.verify(data.len() as u64 + 1, &good).is_err());
        // A restart at 0 resets the hash.
        let mut g = ReceiveFile::new(data.len() as u64);
        g.commit(WriteAt::Append, b"garbage");
        assert_eq!(g.check(0, data.len()), Ok(WriteAt::Restart));
        g.commit(WriteAt::Restart, data);
        assert_eq!(g.verify(data.len() as u64, &good), Ok(()));
    }

    #[test]
    fn resume_continues_only_from_a_matching_prefix() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i * 7) as u8).collect();
        let partial = &data[..40_000];
        // The receiver hashes its partial file and keeps the hash state.
        let off = receive_offset(partial.len() as u64, data.len() as u64);
        assert_eq!(off, 40_000);
        let rx_state = hash_prefix(&mut &partial[..], off, || false).unwrap();
        let rx_prefix = finish_hash(&rx_state);
        // The sender hashes the same prefix of its file: match, continue at the offset.
        let tx_prefix = finish_hash(&hash_prefix(&mut &data[..], off, || false).unwrap());
        let from = resume_from(off, &rx_prefix, &tx_prefix);
        assert_eq!(from, 40_000);
        let mut f = ReceiveFile::resumed(data.len() as u64, off, rx_state.clone());
        for chunk in data[from as usize..].chunks(CHUNK) {
            let at = f.check(f.written(), chunk.len()).unwrap();
            assert_eq!(at, WriteAt::Append);
            f.commit(at, chunk);
        }
        assert_eq!(f.verify(data.len() as u64, &Blake2s256::digest(&data).into()), Ok(()));

        // A different prefix (the file changed): the sender restarts at 0.
        let mut other = data.clone();
        other[10] ^= 1;
        let tx_prefix = finish_hash(&hash_prefix(&mut &other[..], off, || false).unwrap());
        assert_eq!(resume_from(off, &rx_prefix, &tx_prefix), 0);
        let mut f = ReceiveFile::resumed(other.len() as u64, off, rx_state.clone());
        let at = f.check(0, CHUNK).unwrap();
        assert_eq!(at, WriteAt::Restart);
        for (i, chunk) in other.chunks(CHUNK).enumerate() {
            let at = f.check((i * CHUNK) as u64, chunk.len()).unwrap();
            f.commit(at, chunk);
        }
        assert_eq!(f.verify(other.len() as u64, &Blake2s256::digest(&other).into()), Ok(()));

        // A partial file longer than the offered file starts over; a complete one sends nothing.
        assert_eq!(receive_offset(200, 100), 0);
        assert_eq!(receive_offset(100, 100), 100);
        assert_eq!(resume_from(0, &[0; 32], &[1; 32]), 0);
        // Hashing a prefix of a short reader fails, and cancellation stops it.
        assert!(hash_prefix(&mut &data[..10], 11, || false).is_err());
        assert!(hash_prefix(&mut &data[..], 10, || true).is_err());
    }

    #[test]
    fn rate_meter_smooths() {
        let mut m = RateMeter::default();
        assert_eq!(m.update(0, 1000), 0.0);
        assert_eq!(m.update(500, 1100), 0.0, "too soon to measure");
        assert_eq!(m.update(1000, 2000), 1000.0);
        let r = m.update(3000, 3000);
        assert!(r > 1000.0 && r < 2000.0);
    }
}
