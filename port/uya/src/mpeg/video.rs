//! An MPEG-2 video decoder (ISO 13818-2, Main Profile, 4:2:0), written from the standard for the game's movies.
//!
//! The elementary stream is fed in pieces (`push`, each with the time stamp of the PES packet it came in); units are cut at the start
//! codes: the sequence header (0xB3) and its extensions (0xB5: the sequence extension, the quantiser matrix extension, the picture
//! coding extension), the group of pictures (0xB8), the picture header (0x00) and the slices (0x01..=0xAF). A slice is a run of
//! macroblocks: each has a type (intra, motion forward and backward, a coded block pattern, a new quantiser), its motion vectors
//! (differences from the slice's predictors, tables B.10), and up to six 8 x 8 blocks of coefficients (DC from the predictor, the
//! rest as run and level, tables B.14 and B.15, in zigzag or alternate order) that are dequantised with the picture's matrices and
//! quantiser, mismatch-controlled and transformed back (`idct`). A predicted macroblock is the reference frames' pixels at the
//! vector (half-sample positions averaged), forward, backward or both averaged. Pictures come out in display order: an I or P
//! picture is held until the next one is decoded, B pictures go out at once.
//!
//! What the disc's movies use is done, and nothing else: frame pictures with frame prediction and frame DCT. Refused with an
//! error where they first appear, never decoded wrong: field pictures (picture_structure 1 and 2), field prediction and dual-prime
//! vectors (frame_motion_type 1 and 3), field DCT (dct_type 1), chroma formats other than 4:2:0, MPEG-1 streams (no picture coding
//! extension), scalable extensions.

use super::bits::Bits;
use super::idct::idct;
use super::vlc::{self, BWD, EOB, ESCAPE, FWD, INTRA, PATTERN, QUANT};
use std::collections::VecDeque;
use std::rc::Rc;

/// A decoded picture, cropped to the sequence's size: luminance `width * height`, each chrominance plane
/// `width.div_ceil(2) * height.div_ceil(2)`.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub y: Vec<u8>,
    pub cb: Vec<u8>,
    pub cr: Vec<u8>,
    /// The time stamp of the PES packet the picture's start code came in (90 kHz), if it had one.
    pub pts: Option<u64>,
    pub frame_rate_code: u8,
    /// 1 I, 2 P, 3 B.
    pub coding_type: u8,
}

/// What the decoder met, for the tools.
#[derive(Default, Clone, Debug)]
pub struct Stats {
    /// Pictures by picture_coding_type (index 1 I, 2 P, 3 B).
    pub pictures: [u32; 4],
    /// B pictures that came before two reference pictures had been decoded (the start of an open group of pictures): not shown.
    pub dropped: u32,
    pub alternate_scan: u32,
    pub frame_dct_only: u32,
    pub q_scale_type: u32,
    pub concealment: u32,
    /// Predictions that reached outside the reference picture (the edge pixels were used).
    pub outside: u64,
}

/// Table 7-2 and figure 7-2: the zigzag scan and the alternate scan (position in the scan -> position in the block, row-major).
const ZIGZAG: [u8; 64] = [
    0, 1, 8, 16, 9, 2, 3, 10, 17, 24, 32, 25, 18, 11, 4, 5, 12, 19, 26, 33, 40, 48, 41, 34, 27, 20, 13, 6, 7, 14, 21, 28, 35, 42, 49, 56, 57, 50, 43, 36, 29, 22,
    15, 23, 30, 37, 44, 51, 58, 59, 52, 45, 38, 31, 39, 46, 53, 60, 61, 54, 47, 55, 62, 63,
];
const ALTERNATE: [u8; 64] = [
    0, 8, 16, 24, 1, 9, 2, 10, 17, 25, 32, 40, 48, 56, 57, 49, 41, 33, 26, 18, 3, 11, 4, 12, 19, 27, 34, 42, 50, 58, 35, 43, 51, 59, 20, 28, 5, 13, 6, 14, 21, 29,
    36, 44, 52, 60, 37, 45, 53, 61, 22, 30, 7, 15, 23, 31, 38, 46, 54, 62, 39, 47, 55, 63,
];

/// The default intra quantiser matrix (6.3.11), row-major.
const DEFAULT_INTRA: [u8; 64] = [
    8, 16, 19, 22, 26, 27, 29, 34, 16, 16, 22, 24, 27, 29, 34, 37, 19, 22, 26, 27, 29, 34, 34, 38, 22, 22, 26, 27, 29, 34, 37, 40, 22, 26, 27, 29, 32, 35, 40, 48,
    26, 27, 29, 32, 35, 40, 48, 58, 26, 27, 29, 34, 38, 46, 56, 69, 27, 29, 35, 38, 46, 56, 69, 83,
];

/// Table 7-6: quantiser_scale for q_scale_type 1, by quantiser_scale_code 1..=31.
const NONLINEAR: [i32; 31] = [1, 2, 3, 4, 5, 6, 7, 8, 10, 12, 14, 16, 18, 20, 22, 24, 28, 32, 36, 40, 44, 48, 52, 56, 64, 72, 80, 88, 96, 104, 112];

#[derive(Clone, Copy)]
struct Seq {
    w: usize,
    h: usize,
    mbw: usize,
    mbh: usize,
    frame_rate_code: u8,
}

#[derive(Clone, Copy, Default)]
struct Pic {
    ctype: u8,
    f_code: [[u8; 2]; 2],
    dc_prec: u8,
    structure: u8,
    fpfd: bool,
    cmv: bool,
    qst: bool,
    ivlc: bool,
    alt: bool,
    ext: bool,
    pts: Option<u64>,
    started: bool,
    skip: bool,
}

/// A frame store: the planes at the coded size (whole macroblocks).
struct FBuf {
    w: usize,
    h: usize,
    y: Vec<u8>,
    cb: Vec<u8>,
    cr: Vec<u8>,
}

/// The frame being decoded: its store and its picture's header.
struct Frm {
    buf: FBuf,
    pic: Pic,
}

/// A macroblock's prediction: the directions and each one's vector (`mv[s]`, in half samples).
#[derive(Clone, Copy)]
struct Motion {
    fwd: bool,
    bwd: bool,
    mv: [[i32; 2]; 2],
}

/// The state that runs through a slice: the quantiser code, the DC predictors, the motion vector predictors (`pmv[s][t]`) and
/// the last macroblock's prediction (what a skipped macroblock of a B picture repeats).
struct Sl {
    q: u32,
    dc: [i32; 3],
    pmv: [[i32; 2]; 2],
    last: Option<Motion>,
}

/// A macroblock's pixels before they go into the frame store: luminance 16 x 16, each chrominance 8 x 8, row-major.
struct Px {
    y: [u8; 256],
    c: [[u8; 64]; 2],
}

/// A plane to predict from.
struct View<'a> {
    p: &'a [u8],
    w: usize,
    h: usize,
}

impl FBuf {
    fn new(w: usize, h: usize) -> FBuf {
        FBuf { w, h, y: vec![0; w * h], cb: vec![0; w * h / 4], cr: vec![0; w * h / 4] }
    }

    /// Plane 0 luminance, 1 and 2 chrominance.
    fn view(&self, plane: usize) -> View<'_> {
        match plane {
            0 => View { p: &self.y, w: self.w, h: self.h },
            1 => View { p: &self.cb, w: self.w / 2, h: self.h / 2 },
            _ => View { p: &self.cr, w: self.w / 2, h: self.h / 2 },
        }
    }
}

/// The prediction of a `w` x `h` block at `(x2, y2)` (half samples) of `v` into `out` (rows `stride` apart), averaged with what
/// is there when `avg` (the second direction of a bidirectional prediction). Returns whether the block reached outside the view.
#[allow(clippy::too_many_arguments)]
fn mc(v: &View, x2: i32, y2: i32, w: usize, h: usize, out: &mut [u8], stride: usize, avg: bool) -> bool {
    let (ix, iy) = (x2 >> 1, y2 >> 1);
    let (hx, hy) = (x2 & 1 != 0, y2 & 1 != 0);
    let outside = ix < 0 || iy < 0 || ix as usize + w + hx as usize > v.w || iy as usize + h + hy as usize > v.h;
    let ww = w + 1;
    let mut win = [0u8; 17 * 17];
    for j in 0..=h {
        let sy = (iy + j as i32).clamp(0, v.h as i32 - 1) as usize;
        let row = &v.p[sy * v.w..];
        for i in 0..=w {
            let sx = (ix + i as i32).clamp(0, v.w as i32 - 1) as usize;
            win[j * ww + i] = row[sx];
        }
    }
    for j in 0..h {
        for i in 0..w {
            let a = win[j * ww + i] as u32;
            let p = match (hx, hy) {
                (false, false) => a,
                (true, false) => (a + win[j * ww + i + 1] as u32 + 1) >> 1,
                (false, true) => (a + win[(j + 1) * ww + i] as u32 + 1) >> 1,
                (true, true) => (a + win[j * ww + i + 1] as u32 + win[(j + 1) * ww + i] as u32 + win[(j + 1) * ww + i + 1] as u32 + 2) >> 2,
            };
            let o = &mut out[j * stride + i];
            *o = if avg { ((*o as u32 + p + 1) >> 1) as u8 } else { p as u8 };
        }
    }
    outside
}

/// The next start code at or after `i`: the index of its 00 00 01.
fn find_sc(b: &[u8], mut i: usize) -> Option<usize> {
    while i + 3 < b.len() {
        if b[i + 2] > 1 {
            i += 3;
        } else if b[i] == 0 && b[i + 1] == 0 && b[i + 2] == 1 {
            return Some(i);
        } else {
            i += 1;
        }
    }
    None
}

/// A motion vector component from its code and residual (7.6.3.1), added to the predictor and brought back into range.
fn motion_delta(b: &mut Bits, f_code: u8, pred: i32) -> Result<i32, String> {
    if f_code == 0 || f_code > 9 {
        return Err(format!("f_code {f_code}"));
    }
    let (mag, n) = vlc::motion_code().look(b).ok_or("motion_code: no such code")?;
    b.skip(n);
    let mut code = mag as i32;
    if code != 0 && b.get1() {
        code = -code;
    }
    let r_size = (f_code - 1) as u32;
    let delta = if code == 0 || r_size == 0 {
        code
    } else {
        let residual = b.get(r_size) as i32;
        let d = ((code.abs() - 1) << r_size) + residual + 1;
        if code < 0 { -d } else { d }
    };
    let f = 1 << r_size;
    let (low, high, range) = (-16 * f, 16 * f - 1, 32 * f);
    let mut v = pred + delta;
    if v < low {
        v += range;
    }
    if v > high {
        v -= range;
    }
    Ok(v)
}

pub struct Decoder {
    buf: Vec<u8>,
    /// The stream offset of `buf[0]`, and the PES packets' time stamps by the stream bytes they carried (start, end, stamp).
    base: usize,
    stamps: VecDeque<(usize, usize, u64)>,
    seq: Option<Seq>,
    intra_q: [u8; 64],
    inter_q: [u8; 64],
    pic: Pic,
    frm: Option<Frm>,
    /// The older and the newer reference frame.
    refs: [Option<Rc<FBuf>>; 2],
    /// The newest reference frame, waiting for its turn to be shown.
    held: Option<(Rc<FBuf>, Pic)>,
    out: VecDeque<Frame>,
    pub stats: Stats,
}

impl Default for Decoder {
    fn default() -> Decoder {
        Decoder::new()
    }
}

impl Decoder {
    pub fn new() -> Decoder {
        Decoder {
            buf: Vec::new(),
            base: 0,
            stamps: VecDeque::new(),
            seq: None,
            intra_q: DEFAULT_INTRA,
            inter_q: [16; 64],
            pic: Pic::default(),
            frm: None,
            refs: [None, None],
            held: None,
            out: VecDeque::new(),
            stats: Stats::default(),
        }
    }

    /// More of the elementary stream; `pts` belongs to the first picture that starts in it.
    pub fn push(&mut self, es: &[u8], pts: Option<u64>) -> Result<(), String> {
        let start = self.base + self.buf.len();
        self.buf.extend_from_slice(es);
        if let Some(p) = pts {
            self.stamps.push_back((start, start + es.len(), p));
        }
        self.units(false)
    }

    /// The end of the stream: the last units are decoded and the last reference picture is shown.
    pub fn flush(&mut self) -> Result<(), String> {
        self.units(true)?;
        self.end_picture()?;
        if let Some((b, p)) = self.held.take() {
            let f = self.frame(&b, &p)?;
            self.out.push_back(f);
        }
        Ok(())
    }

    /// The next picture in display order.
    pub fn next_frame(&mut self) -> Option<Frame> {
        self.out.pop_front()
    }

    /// Decodes every whole unit in the buffer (and, at the end, the last one).
    fn units(&mut self, last: bool) -> Result<(), String> {
        loop {
            let Some(s0) = find_sc(&self.buf, 0) else {
                if last {
                    self.base += self.buf.len();
                    self.buf.clear();
                }
                return Ok(());
            };
            let s1 = match find_sc(&self.buf, s0 + 4) {
                Some(s) => s,
                None if last => self.buf.len(),
                None => {
                    // Keep the unit being received from its start code on.
                    self.buf.drain(..s0);
                    self.base += s0;
                    return Ok(());
                }
            };
            let code = self.buf[s0 + 3];
            let unit = self.buf[s0 + 4..s1].to_vec();
            let at = self.base + s0;
            self.buf.drain(..s1);
            self.base += s1;
            self.unit(code, &unit, at)?;
            if self.buf.is_empty() {
                return Ok(());
            }
        }
    }

    fn unit(&mut self, code: u8, d: &[u8], at: usize) -> Result<(), String> {
        match code {
            0x00 => {
                self.end_picture()?;
                self.picture_header(d, at)
            }
            0x01..=0xaf => self.slice(code, d),
            0xb3 => {
                self.end_picture()?;
                self.sequence_header(d)
            }
            0xb5 => self.extension(d),
            0xb7 | 0xb8 => self.end_picture(),
            // User data and the reserved codes carry nothing the pictures need.
            _ => Ok(()),
        }
    }

    fn sequence_header(&mut self, d: &[u8]) -> Result<(), String> {
        let mut b = Bits::new(d);
        let (w, h) = (b.get(12) as usize, b.get(12) as usize);
        b.skip(4);
        let frame_rate_code = b.get(4) as u8;
        b.skip(18 + 1 + 10 + 1);
        self.intra_q = DEFAULT_INTRA;
        self.inter_q = [16; 64];
        if b.get1() {
            for &z in &ZIGZAG {
                self.intra_q[z as usize] = b.get(8) as u8;
            }
        }
        if b.get1() {
            for &z in &ZIGZAG {
                self.inter_q[z as usize] = b.get(8) as u8;
            }
        }
        if w == 0 || h == 0 || self.intra_q.contains(&0) || self.inter_q.contains(&0) {
            return Err(format!("sequence header: {w} x {h}, a matrix with a zero"));
        }
        self.seq = Some(Seq { w, h, mbw: w.div_ceil(16), mbh: h.div_ceil(16), frame_rate_code });
        Ok(())
    }

    fn extension(&mut self, d: &[u8]) -> Result<(), String> {
        let mut b = Bits::new(d);
        match b.get(4) {
            // The sequence extension: only 4:2:0 is done.
            1 => {
                b.skip(8 + 1);
                let chroma = b.get(2);
                if chroma != 1 {
                    return Err(format!("chroma_format {chroma}: only 4:2:0 is done"));
                }
                Ok(())
            }
            3 => {
                if b.get1() {
                    for &z in &ZIGZAG {
                        self.intra_q[z as usize] = b.get(8) as u8;
                    }
                }
                if b.get1() {
                    for &z in &ZIGZAG {
                        self.inter_q[z as usize] = b.get(8) as u8;
                    }
                }
                Ok(())
            }
            8 => {
                let p = &mut self.pic;
                for s in 0..2 {
                    for t in 0..2 {
                        p.f_code[s][t] = b.get(4) as u8;
                    }
                }
                p.dc_prec = b.get(2) as u8;
                p.structure = b.get(2) as u8;
                // top_field_first: the order of a frame's fields on an interlaced display; the pictures go out as whole frames.
                b.skip(1);
                p.fpfd = b.get1();
                p.cmv = b.get1();
                p.qst = b.get1();
                p.ivlc = b.get1();
                p.alt = b.get1();
                // repeat_first_field, chroma_420_type, progressive_frame: display timing and chroma siting, not decoding.
                p.ext = true;
                if p.structure != 3 {
                    return Err(format!("picture_structure {}: field pictures are not done (every movie of the disc has frame pictures)", p.structure));
                }
                Ok(())
            }
            // The sequence display, copyright and picture display extensions say nothing the decoding needs; the scalable ones
            // are refused.
            2 | 4 | 7 => Ok(()),
            e => Err(format!("extension {e}: a scalable stream")),
        }
    }

    fn picture_header(&mut self, d: &[u8], at: usize) -> Result<(), String> {
        let mut b = Bits::new(d);
        b.skip(10);
        let ctype = b.get(3) as u8;
        if !(1..=3).contains(&ctype) {
            return Err(format!("picture_coding_type {ctype}"));
        }
        // The time stamp of the packet the start code came in, if no earlier picture took it.
        while self.stamps.front().is_some_and(|s| s.1 <= at) {
            self.stamps.pop_front();
        }
        let pts = match self.stamps.front() {
            Some(&(s, e, p)) if s <= at && at < e => {
                self.stamps.pop_front();
                Some(p)
            }
            _ => None,
        };
        self.pic = Pic { ctype, pts, ..Pic::default() };
        Ok(())
    }

    /// The picture's first slice: a new frame.
    fn begin(&mut self) -> Result<(), String> {
        let seq = self.seq.ok_or("a picture before the sequence header")?;
        let p = self.pic;
        if !p.ext {
            return Err("a picture without a picture coding extension (MPEG-1): not done".into());
        }
        let need = match p.ctype {
            1 => 0,
            2 => 1,
            _ => 2,
        };
        let have = self.refs.iter().filter(|r| r.is_some()).count();
        if have < need {
            if p.ctype == 3 {
                self.pic.skip = true;
                self.stats.dropped += 1;
                return Ok(());
            }
            return Err(format!("a picture of type {} with {have} reference pictures", p.ctype));
        }
        self.stats.pictures[p.ctype as usize] += 1;
        self.stats.alternate_scan += p.alt as u32;
        self.stats.frame_dct_only += p.fpfd as u32;
        self.stats.q_scale_type += p.qst as u32;
        self.stats.concealment += p.cmv as u32;
        self.frm = Some(Frm { buf: FBuf::new(seq.mbw * 16, seq.mbh * 16), pic: p });
        self.pic.started = true;
        Ok(())
    }

    /// The end of a picture: its frame is complete.
    fn end_picture(&mut self) -> Result<(), String> {
        let p = std::mem::take(&mut self.pic);
        if !p.started {
            return Ok(());
        }
        let f = self.frm.take().expect("a started picture has its frame");
        let buf = Rc::new(f.buf);
        if f.pic.ctype == 3 {
            let fr = self.frame(&buf, &f.pic)?;
            self.out.push_back(fr);
        } else {
            if let Some((b, hp)) = self.held.take() {
                let fr = self.frame(&b, &hp)?;
                self.out.push_back(fr);
            }
            self.refs[0] = self.refs[1].take();
            self.refs[1] = Some(buf.clone());
            self.held = Some((buf, f.pic));
        }
        Ok(())
    }

    /// A frame store cropped to the picture's size.
    fn frame(&self, b: &FBuf, p: &Pic) -> Result<Frame, String> {
        let seq = self.seq.ok_or("no sequence header")?;
        let (w, h) = (seq.w, seq.h);
        let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
        let crop = |src: &[u8], sw: usize, w: usize, h: usize| -> Vec<u8> { (0..h).flat_map(|y| src[y * sw..y * sw + w].iter().copied()).collect() };
        Ok(Frame {
            width: w as u32,
            height: h as u32,
            y: crop(&b.y, b.w, w, h),
            cb: crop(&b.cb, b.w / 2, cw, ch),
            cr: crop(&b.cr, b.w / 2, cw, ch),
            pts: p.pts,
            frame_rate_code: seq.frame_rate_code,
            coding_type: p.ctype,
        })
    }

    // ---- slices and macroblocks ----

    fn slice(&mut self, code: u8, d: &[u8]) -> Result<(), String> {
        if !self.pic.started && !self.pic.skip {
            self.begin()?;
        }
        if self.pic.skip {
            return Ok(());
        }
        let seq = self.seq.expect("begin checked it");
        let p = self.pic;
        let rows = seq.mbh;
        let row = code as usize - 1;
        if row >= rows {
            return Err(format!("slice at row {row} of {rows}"));
        }
        let mut b = Bits::new(d);
        let q = b.get(5);
        if b.peek(1) == 1 {
            b.skip(9);
        }
        while b.get1() {
            b.skip(8);
        }
        let dc0 = 1 << (7 + p.dc_prec);
        let mut sl = Sl { q, dc: [dc0; 3], pmv: [[0; 2]; 2], last: None };
        let mut addr = (row * seq.mbw) as i64 - 1;
        let mut first = true;
        loop {
            let mut inc = 0i64;
            loop {
                let (v, n) = vlc::mb_increment().look(&b).ok_or_else(|| format!("macroblock_address_increment at row {row}, after macroblock {addr}"))?;
                b.skip(n);
                if v == 34 {
                    inc += 33;
                } else {
                    inc += v as i64;
                    break;
                }
            }
            if first {
                addr += inc;
                first = false;
            } else {
                for _ in 1..inc {
                    addr += 1;
                    self.skipped(addr as usize, &mut sl)?;
                }
                addr += 1;
            }
            self.macroblock(&mut b, addr as usize, &mut sl)?;
            if b.over() {
                return Err(format!("slice at row {row}: macroblock {addr} runs past the slice's data"));
            }
            if b.at_start_code() {
                return Ok(());
            }
        }
    }

    fn macroblock(&mut self, b: &mut Bits, addr: usize, sl: &mut Sl) -> Result<(), String> {
        let seq = self.seq.expect("begin checked it");
        let p = self.pic;
        let (mbx, mby) = (addr % seq.mbw, addr / seq.mbw);
        if mby >= seq.mbh {
            return Err(format!("macroblock {addr} below the picture"));
        }
        let table = match p.ctype {
            1 => vlc::mb_type_i(),
            2 => vlc::mb_type_p(),
            _ => vlc::mb_type_b(),
        };
        let (t, n) = table.look(b).ok_or_else(|| format!("macroblock_type at macroblock {addr}"))?;
        b.skip(n);
        let (quant, fwd, bwd, pattern, intra) = (t & QUANT != 0, t & FWD != 0, t & BWD != 0, t & PATTERN != 0, t & INTRA != 0);
        if (fwd || bwd) && !p.fpfd {
            match b.get(2) {
                2 => {}
                1 => return Err(format!("field prediction (frame_motion_type 1) at macroblock {addr}: not done")),
                3 => return Err(format!("dual-prime motion vectors at macroblock {addr}: not done")),
                _ => return Err(format!("frame_motion_type 0 at macroblock {addr}")),
            }
        }
        if !p.fpfd && (intra || pattern) && b.get1() {
            return Err(format!("field DCT (dct_type 1) at macroblock {addr}: not done"));
        }
        if quant {
            sl.q = b.get(5);
        }
        let mut m = Motion { fwd, bwd, mv: [[0; 2]; 2] };
        if fwd || (intra && p.cmv) {
            self.motion_vectors(b, 0, &mut m, sl)?;
        }
        if bwd {
            self.motion_vectors(b, 1, &mut m, sl)?;
        }
        if intra && p.cmv {
            b.skip(1);
        }
        let cbp = if pattern {
            let (v, n) = vlc::cbp().look(b).ok_or_else(|| format!("coded_block_pattern at macroblock {addr}"))?;
            b.skip(n);
            v as u32
        } else if intra {
            63
        } else {
            0
        };
        let mut px = Px { y: [0; 256], c: [[0; 64]; 2] };
        if intra {
            if !p.cmv {
                sl.pmv = [[0; 2]; 2];
            }
            sl.last = None;
        } else {
            sl.dc = [1 << (7 + p.dc_prec); 3];
            // A P macroblock without motion is predicted from the same place of the reference.
            if p.ctype == 2 && !fwd {
                sl.pmv = [[0; 2]; 2];
                m = Motion { fwd: true, bwd: false, mv: [[0; 2]; 2] };
            }
            sl.last = Some(m);
            self.predict(mbx, mby, &m, &mut px)?;
        }
        let qs = if sl.q == 0 {
            return Err(format!("quantiser_scale_code 0 at macroblock {addr}"));
        } else if p.qst {
            NONLINEAR[sl.q as usize - 1]
        } else {
            2 * sl.q as i32
        };
        for i in 0..6 {
            if cbp & (32 >> i) == 0 {
                continue;
            }
            let f = self.block(b, intra, if i < 4 { 0 } else { i - 3 }, sl, qs)?;
            add_block(&mut px, i, &f);
        }
        self.store(mbx, mby, &px);
        Ok(())
    }

    /// A skipped macroblock (7.6.6): in a P picture the reference's pixels at the same place, in a B picture the previous
    /// macroblock's prediction again.
    fn skipped(&mut self, addr: usize, sl: &mut Sl) -> Result<(), String> {
        let seq = self.seq.expect("begin checked it");
        let p = self.pic;
        let (mbx, mby) = (addr % seq.mbw, addr / seq.mbw);
        sl.dc = [1 << (7 + p.dc_prec); 3];
        let m = match p.ctype {
            2 => {
                sl.pmv = [[0; 2]; 2];
                Motion { fwd: true, bwd: false, mv: [[0; 2]; 2] }
            }
            3 => sl.last.ok_or_else(|| format!("a skipped macroblock ({addr}) after an intra one in a B picture"))?,
            _ => return Err(format!("a skipped macroblock ({addr}) in an I picture")),
        };
        let mut px = Px { y: [0; 256], c: [[0; 64]; 2] };
        self.predict(mbx, mby, &m, &mut px)?;
        self.store(mbx, mby, &px);
        Ok(())
    }

    /// motion_vectors(s) (6.2.5.2) for direction `s` of frame prediction (one vector), the predictor updated (7.6.3).
    fn motion_vectors(&mut self, b: &mut Bits, s: usize, m: &mut Motion, sl: &mut Sl) -> Result<(), String> {
        let p = self.pic;
        let x = motion_delta(b, p.f_code[s][0], sl.pmv[s][0])?;
        let y = motion_delta(b, p.f_code[s][1], sl.pmv[s][1])?;
        sl.pmv[s] = [x, y];
        m.mv[s] = [x, y];
        Ok(())
    }

    /// The reference frame of direction `s`: a P picture's is the newer reference, a B picture's forward the older and backward the
    /// newer.
    fn frame_ref(&self, s: usize) -> Result<&FBuf, String> {
        let i = if self.pic.ctype == 3 && s == 0 { 0 } else { 1 };
        self.refs[i].as_deref().ok_or_else(|| "a prediction without its reference picture".to_string())
    }

    /// The macroblock's prediction (7.6.4 to 7.6.7) into `px`.
    fn predict(&mut self, mbx: usize, mby: usize, m: &Motion, px: &mut Px) -> Result<(), String> {
        let (bx, by) = (mbx as i32, mby as i32);
        let mut avg = false;
        let mut outside = 0u64;
        for s in 0..2 {
            if !(if s == 0 { m.fwd } else { m.bwd }) {
                continue;
            }
            let fb = self.frame_ref(s)?;
            let mv = m.mv[s];
            outside += mc(&fb.view(0), 32 * bx + mv[0], 32 * by + mv[1], 16, 16, &mut px.y, 16, avg) as u64;
            for c in 0..2 {
                outside += mc(&fb.view(1 + c), 16 * bx + mv[0] / 2, 16 * by + mv[1] / 2, 8, 8, &mut px.c[c], 8, avg) as u64;
            }
            avg = true;
        }
        self.stats.outside += outside;
        Ok(())
    }

    /// One block's coefficients (6.2.6, 7.2 to 7.4), dequantised, mismatch-controlled and transformed: the block's samples (or the
    /// differences to add to the prediction). `cc` 0 luminance, 1 Cb, 2 Cr.
    fn block(&self, b: &mut Bits, intra: bool, cc: usize, sl: &mut Sl, qs: i32) -> Result<[i32; 64], String> {
        let p = self.pic;
        let scan = if p.alt { &ALTERNATE } else { &ZIGZAG };
        let w = if intra { &self.intra_q } else { &self.inter_q };
        let mut f = [0i32; 64];
        let mut sum = 0i32;
        let mut n = 0usize;
        if intra {
            let t = if cc == 0 { vlc::dc_size_luma() } else { vlc::dc_size_chroma() };
            let (size, len) = t.look(b).ok_or("dct_dc_size: no such code")?;
            b.skip(len);
            let size = size as u32;
            let diff = if size == 0 {
                0
            } else {
                let v = b.get(size) as i32;
                if v & (1 << (size - 1)) != 0 { v } else { v - (1 << size) + 1 }
            };
            sl.dc[cc] += diff;
            f[0] = (sl.dc[cc] << (3 - p.dc_prec)).clamp(-2048, 2047);
            sum = f[0];
            n = 1;
        }
        let table = vlc::coefficient(intra && p.ivlc);
        loop {
            let (run, level) = if !intra && n == 0 && b.peek(1) == 1 {
                // The first coefficient of a non-intra block: '1s' is run 0, level 1.
                b.skip(1);
                (0usize, if b.get1() { -1 } else { 1 })
            } else {
                let (v, len) = table.look(b).ok_or("DCT coefficient: no such code")?;
                b.skip(len);
                if v == EOB {
                    break;
                }
                if v == ESCAPE {
                    let run = b.get(6) as usize;
                    let l = b.get(12) as i32;
                    let level = if l >= 2048 { l - 4096 } else { l };
                    if level == 0 || level == -2048 {
                        return Err(format!("escaped level {level}"));
                    }
                    (run, level)
                } else {
                    let level = (v & 0xff) as i32;
                    ((v >> 8) as usize, if b.get1() { -level } else { level })
                }
            };
            n += run;
            if n > 63 {
                return Err("a block with more than 64 coefficients".into());
            }
            let pos = scan[n] as usize;
            let wq = w[pos] as i32 * qs;
            let v = if intra { level * wq * 2 / 32 } else { (2 * level + level.signum()) * wq / 32 };
            f[pos] = v.clamp(-2048, 2047);
            sum += f[pos];
            n += 1;
        }
        if sum & 1 == 0 {
            f[63] ^= 1;
        }
        idct(&mut f);
        Ok(f)
    }

    /// The macroblock into the frame store at its place.
    fn store(&mut self, mbx: usize, mby: usize, px: &Px) {
        let fb = &mut self.frm.as_mut().expect("a started picture has its frame").buf;
        let (w, cw) = (fb.w, fb.w / 2);
        for j in 0..16 {
            let at = (16 * mby + j) * w + 16 * mbx;
            fb.y[at..at + 16].copy_from_slice(&px.y[j * 16..j * 16 + 16]);
        }
        for j in 0..8 {
            let at = (8 * mby + j) * cw + 8 * mbx;
            fb.cb[at..at + 8].copy_from_slice(&px.c[0][j * 8..j * 8 + 8]);
            fb.cr[at..at + 8].copy_from_slice(&px.c[1][j * 8..j * 8 + 8]);
        }
    }
}

/// Block `i` of a macroblock (0..3 luminance, 4 Cb, 5 Cr) added to its prediction.
fn add_block(px: &mut Px, i: usize, f: &[i32; 64]) {
    if i < 4 {
        let (bx, by) = ((i & 1) * 8, i >> 1);
        for k in 0..8 {
            let row = by * 8 + k;
            for x in 0..8 {
                let d = &mut px.y[row * 16 + bx + x];
                *d = (*d as i32 + f[k * 8 + x]).clamp(0, 255) as u8;
            }
        }
    } else {
        let c = &mut px.c[i - 4];
        for k in 0..64 {
            c[k] = (c[k] as i32 + f[k]).clamp(0, 255) as u8;
        }
    }
}
