//! Remote screen viewing: the wire format of channel
//! [`features::channel::SCREEN`](crate::features::channel::SCREEN).
//!
//! A paired phone asks which displays the computer has, then asks for a *view*: one display,
//! optionally cropped to a rectangle (that is how zooming stays sharp: the computer sends
//! only the visible part, scaled to the phone's pixels), at a given JPEG quality and frame
//! rate. The computer answers with `Frame` messages. A frame is larger than one feature
//! body, so it is split into chunks that the phone reassembles by `seq`/`offset`.
//!
//! Flow control: the phone acknowledges every completed frame (`Ack`); the computer keeps
//! at most [`MAX_IN_FLIGHT`] unacknowledged frames, so a slow phone never builds a queue.
//!
//! All coordinates are physical pixels. Crops and cursor positions are relative to the
//! top-left corner of the chosen display; displays are listed with their position in the
//! virtual desktop, which is what absolute mouse messages use.

use crate::protocol::{DecodeError, MAX_FEATURE_BODY, Reader, wire};

pub const MAX_DISPLAYS: usize = 16;
/// Most unacknowledged frames the computer keeps in flight.
pub const MAX_IN_FLIGHT: u32 = 2;
/// Largest frame image accepted (bytes).
pub const MAX_FRAME_BYTES: u32 = 8 << 20;
/// Largest output edge a viewer may request.
pub const MAX_OUT_EDGE: u16 = 2400;
/// Bytes of a `Frame` chunk before the payload.
pub const FRAME_HEADER: usize = 1 + 4 + 1 + 16 + 8 + 4 + 4 + 4 + 4;
/// Largest payload of one `Frame` chunk.
pub const CHUNK: usize = MAX_FEATURE_BODY - FRAME_HEADER - 4;
/// `cursor_x`/`cursor_y` value meaning "the cursor is not on this display".
pub const NO_CURSOR: i32 = i32::MIN;

mod kind {
    pub const LIST: u8 = 1;
    pub const VIEW: u8 = 2;
    pub const STOP: u8 = 3;
    pub const ACK: u8 = 4;
    pub const DISPLAYS: u8 = 10;
    pub const FRAME: u8 = 11;
    pub const STOPPED: u8 = 12;
    pub const CURSOR: u8 = 13;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayInfo {
    /// Position in the virtual desktop.
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
    pub name: String,
}

/// What the viewer wants to see.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct View {
    pub display: u8,
    /// Crop in display pixels; `crop_w == 0` means the whole display.
    pub crop_x: i32,
    pub crop_y: i32,
    pub crop_w: u32,
    pub crop_h: u32,
    /// Size the frame is scaled to (the computer keeps the aspect ratio of the crop).
    pub out_w: u16,
    pub out_h: u16,
    /// JPEG quality 20..=95.
    pub quality: u8,
    /// Frames per second 1..=30.
    pub fps: u8,
}

impl View {
    pub fn clamped(mut self) -> Self {
        self.quality = self.quality.clamp(20, 95);
        self.fps = self.fps.clamp(1, 30);
        self.out_w = self.out_w.clamp(64, MAX_OUT_EDGE);
        self.out_h = self.out_h.clamp(64, MAX_OUT_EDGE);
        self.crop_w = self.crop_w.min(1 << 16);
        self.crop_h = self.crop_h.min(1 << 16);
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenMsg {
    List,
    View(View),
    Stop,
    Ack {
        seq: u32,
    },
    Displays {
        displays: Vec<DisplayInfo>,
    },
    /// One chunk of a frame. `crop_*` is the (clamped) region of the display the image shows.
    Frame {
        seq: u32,
        display: u8,
        crop_x: i32,
        crop_y: i32,
        crop_w: u32,
        crop_h: u32,
        cursor_x: i32,
        cursor_y: i32,
        img_w: u16,
        img_h: u16,
        total: u32,
        offset: u32,
        data: Vec<u8>,
    },
    Stopped {
        reason: String,
    },
    /// The cursor moved while the picture did not change (display-relative; NO_CURSOR = off display).
    Cursor {
        display: u8,
        x: i32,
        y: i32,
    },
}

impl ScreenMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        match self {
            ScreenMsg::List => out.push(kind::LIST),
            ScreenMsg::View(v) => {
                out.push(kind::VIEW);
                out.push(v.display);
                wire::put_u32(&mut out, v.crop_x as u32);
                wire::put_u32(&mut out, v.crop_y as u32);
                wire::put_u32(&mut out, v.crop_w);
                wire::put_u32(&mut out, v.crop_h);
                wire::put_u16(&mut out, v.out_w);
                wire::put_u16(&mut out, v.out_h);
                out.push(v.quality);
                out.push(v.fps);
            }
            ScreenMsg::Stop => out.push(kind::STOP),
            ScreenMsg::Ack { seq } => {
                out.push(kind::ACK);
                wire::put_u32(&mut out, *seq);
            }
            ScreenMsg::Displays { displays } => {
                out.push(kind::DISPLAYS);
                let n = displays.len().min(MAX_DISPLAYS);
                out.push(n as u8);
                for d in &displays[..n] {
                    wire::put_u32(&mut out, d.x as u32);
                    wire::put_u32(&mut out, d.y as u32);
                    wire::put_u32(&mut out, d.width as u32);
                    wire::put_u32(&mut out, d.height as u32);
                    out.push(d.primary as u8);
                    wire::put_str8(&mut out, &d.name);
                }
            }
            ScreenMsg::Frame {
                seq,
                display,
                crop_x,
                crop_y,
                crop_w,
                crop_h,
                cursor_x,
                cursor_y,
                img_w,
                img_h,
                total,
                offset,
                data,
            } => {
                out.reserve(FRAME_HEADER + data.len());
                out.push(kind::FRAME);
                wire::put_u32(&mut out, *seq);
                out.push(*display);
                wire::put_u32(&mut out, *crop_x as u32);
                wire::put_u32(&mut out, *crop_y as u32);
                wire::put_u32(&mut out, *crop_w);
                wire::put_u32(&mut out, *crop_h);
                wire::put_u32(&mut out, *cursor_x as u32);
                wire::put_u32(&mut out, *cursor_y as u32);
                wire::put_u16(&mut out, *img_w);
                wire::put_u16(&mut out, *img_h);
                wire::put_u32(&mut out, *total);
                wire::put_u32(&mut out, *offset);
                wire::put_bytes(&mut out, data);
            }
            ScreenMsg::Stopped { reason } => {
                out.push(kind::STOPPED);
                wire::put_str8(&mut out, reason);
            }
            ScreenMsg::Cursor { display, x, y } => {
                out.push(kind::CURSOR);
                out.push(*display);
                wire::put_u32(&mut out, *x as u32);
                wire::put_u32(&mut out, *y as u32);
            }
        }
        out
    }

    /// Decodes and validates one body. Never panics; trailing bytes are an error.
    pub fn decode(body: &[u8]) -> Result<ScreenMsg, DecodeError> {
        let mut r = Reader::new(body);
        let msg = match r.u8()? {
            kind::LIST => ScreenMsg::List,
            kind::VIEW => {
                let display = r.u8()?;
                let crop_x = r.u32()? as i32;
                let crop_y = r.u32()? as i32;
                let crop_w = r.u32()?;
                let crop_h = r.u32()?;
                let out_w = r.u16()?;
                let out_h = r.u16()?;
                let quality = r.u8()?;
                let fps = r.u8()?;
                ScreenMsg::View(View { display, crop_x, crop_y, crop_w, crop_h, out_w, out_h, quality, fps }.clamped())
            }
            kind::STOP => ScreenMsg::Stop,
            kind::ACK => ScreenMsg::Ack { seq: r.u32()? },
            kind::DISPLAYS => {
                let n = r.u8()? as usize;
                if n > MAX_DISPLAYS {
                    return Err(DecodeError::Invalid("display count"));
                }
                let mut displays = Vec::with_capacity(n);
                for _ in 0..n {
                    displays.push(DisplayInfo {
                        x: r.u32()? as i32,
                        y: r.u32()? as i32,
                        width: r.u32()? as i32,
                        height: r.u32()? as i32,
                        primary: r.bool()?,
                        name: r.str8()?,
                    });
                }
                ScreenMsg::Displays { displays }
            }
            kind::FRAME => {
                let seq = r.u32()?;
                let display = r.u8()?;
                let crop_x = r.u32()? as i32;
                let crop_y = r.u32()? as i32;
                let crop_w = r.u32()?;
                let crop_h = r.u32()?;
                let cursor_x = r.u32()? as i32;
                let cursor_y = r.u32()? as i32;
                let img_w = r.u16()?;
                let img_h = r.u16()?;
                let total = r.u32()?;
                let offset = r.u32()?;
                let data = r.bytes(CHUNK)?.to_vec();
                if total == 0
                    || total > MAX_FRAME_BYTES
                    || data.is_empty()
                    || offset.checked_add(data.len() as u32).is_none_or(|e| e > total)
                {
                    return Err(DecodeError::Invalid("frame chunk"));
                }
                ScreenMsg::Frame {
                    seq,
                    display,
                    crop_x,
                    crop_y,
                    crop_w,
                    crop_h,
                    cursor_x,
                    cursor_y,
                    img_w,
                    img_h,
                    total,
                    offset,
                    data,
                }
            }
            kind::STOPPED => ScreenMsg::Stopped { reason: r.str8()? },
            kind::CURSOR => ScreenMsg::Cursor { display: r.u8()?, x: r.u32()? as i32, y: r.u32()? as i32 },
            _ => return Err(DecodeError::Invalid("screen message kind")),
        };
        r.finish()?;
        Ok(msg)
    }
}

/// Splits one encoded JPEG into `Frame` chunk bodies.
#[allow(clippy::too_many_arguments)]
pub fn frame_chunks(
    seq: u32,
    display: u8,
    crop: (i32, i32, u32, u32),
    cursor: (i32, i32),
    img: (u16, u16),
    jpeg: &[u8],
) -> Vec<Vec<u8>> {
    let total = jpeg.len() as u32;
    jpeg.chunks(CHUNK)
        .enumerate()
        .map(|(i, part)| {
            ScreenMsg::Frame {
                seq,
                display,
                crop_x: crop.0,
                crop_y: crop.1,
                crop_w: crop.2,
                crop_h: crop.3,
                cursor_x: cursor.0,
                cursor_y: cursor.1,
                img_w: img.0,
                img_h: img.1,
                total,
                offset: (i * CHUNK) as u32,
                data: part.to_vec(),
            }
            .encode()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(m: ScreenMsg) {
        assert_eq!(ScreenMsg::decode(&m.encode()).unwrap(), m);
    }

    #[test]
    fn messages_roundtrip() {
        roundtrip(ScreenMsg::List);
        roundtrip(ScreenMsg::Stop);
        roundtrip(ScreenMsg::Ack { seq: 7 });
        roundtrip(ScreenMsg::Stopped { reason: "bye".into() });
        roundtrip(ScreenMsg::Cursor { display: 2, x: -4, y: 9 });
        roundtrip(ScreenMsg::View(View {
            display: 1,
            crop_x: -5,
            crop_y: 10,
            crop_w: 800,
            crop_h: 450,
            out_w: 1280,
            out_h: 720,
            quality: 60,
            fps: 15,
        }));
        roundtrip(ScreenMsg::Displays {
            displays: vec![
                DisplayInfo { x: 0, y: 0, width: 1920, height: 1080, primary: true, name: "Main".into() },
                DisplayInfo { x: -1280, y: 100, width: 1280, height: 1024, primary: false, name: "Side".into() },
            ],
        });
    }

    #[test]
    fn view_is_clamped_on_decode() {
        let v = View {
            display: 0,
            crop_x: 0,
            crop_y: 0,
            crop_w: 0,
            crop_h: 0,
            out_w: 5,
            out_h: 60000,
            quality: 1,
            fps: 200,
        };
        match ScreenMsg::decode(&ScreenMsg::View(v).encode()).unwrap() {
            ScreenMsg::View(c) => {
                assert_eq!((c.out_w, c.out_h, c.quality, c.fps), (64, MAX_OUT_EDGE, 20, 30));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn frame_chunks_reassemble_and_fit_a_feature_body() {
        let jpeg: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let chunks = frame_chunks(3, 0, (0, 0, 1920, 1080), (10, 20), (1280, 720), &jpeg);
        assert!(chunks.len() > 2);
        let mut got = vec![0u8; jpeg.len()];
        for c in &chunks {
            assert!(c.len() <= MAX_FEATURE_BODY);
            match ScreenMsg::decode(c).unwrap() {
                ScreenMsg::Frame { seq, offset, data, total, .. } => {
                    assert_eq!((seq, total as usize), (3, jpeg.len()));
                    got[offset as usize..offset as usize + data.len()].copy_from_slice(&data);
                }
                _ => panic!(),
            }
        }
        assert_eq!(got, jpeg);
    }

    #[test]
    fn malformed_frames_are_rejected() {
        let mut c = frame_chunks(1, 0, (0, 0, 10, 10), (0, 0), (10, 10), &[1, 2, 3]).remove(0);
        c.push(0); // trailing byte
        assert!(ScreenMsg::decode(&c).is_err());
        assert!(ScreenMsg::decode(&[99]).is_err());
        assert!(ScreenMsg::decode(&[kind::FRAME, 1]).is_err());
    }
}
