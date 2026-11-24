//! The VIF1 side of the VU1 programs the port does itself (`vuwater.rs`, `vusea.rs`): a pass's stream cut at its microcode
//! uploads, the program each piece uploads known by its microcode, and a piece walked as VIF1 would walk it (7.3 of the VIF's
//! manual: STCYCL, OFFSET and BASE's double buffer, STMOD, STMASK, STROW and STCOL, UNPACK into VU memory, DIRECT packets to the
//! GIF), each MSCAL handed to the program.

/// The VU1 programs the port does itself: the microcode's FNV-1a 64 and size.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Program {
    /// The water's, slime's and swamp's surfaces (VIF packet at 0x10F500 in the main program).
    Water,
    /// The open sea (VIF packet at 0x116070).
    Sea,
}

const PROGRAMS: [(Program, u64, usize); 2] = [(Program::Water, 0x3dc7_39c2_8ad6_ece5, 4704), (Program::Sea, 0x543a_eb36_7bbe_1acd, 3760)];

/// What a piece's program does with VU memory when the VIF starts it.
pub trait Micro {
    /// A run from `entry` (a byte address) with VIF1's TOPS `top`; the GIF packets it kicks appended to `out`. `clip` is the VU's
    /// clip flags (CLIP's judgements: the latest in bits 0 to 5, each older one six bits up, 24 bits), which carry from run to run
    /// and from program to program. A run is made under the rounding `vif::run` holds (toward zero, with flush-to-zero and
    /// denormals-are-zero: `vufloat::rz::Mode`), which `vusea.rs` leaves to the host's arithmetic.
    fn call(&mut self, entry: u32, mem: &mut VuMem, top: usize, clip: &mut u32, out: &mut Vec<[u32; 4]>) -> Result<(), String>;
}

/// VU1's data memory: 1,024 quadwords (an address masked to 10 bits is always inside it).
pub type VuMem = [[u32; 4]; 1024];

/// One piece of a pass's stream: from an upload of microcode to the next (`range`), and the program it uploads when it is one the
/// port does.
pub struct Piece {
    pub range: std::ops::Range<usize>,
    pub program: Option<Program>,
}

/// A pass's stream cut at its microcode uploads (a run of MPG codes, with only NOPs and FLUSHes between them, is one upload).
/// A pass can upload more than one program (level 14's second pass runs the sea's program, then the water's).
pub fn pieces(stream: &[u32]) -> Result<Vec<Piece>, String> {
    let r = VifRegs::default();
    // Each upload's start.
    let mut uploads: Vec<usize> = Vec::new();
    let (mut i, mut in_upload) = (0, false);
    while i < stream.len() {
        let cmd = (stream[i] >> 24) & 0x7f;
        let next = i + 1 + skip(stream, i, &r).ok_or("vif: a VIF code past the stream")?;
        match cmd {
            0x4a => {
                if !in_upload {
                    uploads.push(i);
                }
                in_upload = true;
            }
            0x00 | 0x10 | 0x11 | 0x13 => {}
            _ => in_upload = false,
        }
        i = next;
    }
    if uploads.is_empty() {
        uploads.push(0);
    }
    // What comes before the first upload (the chain's FLUSH) belongs to its piece.
    uploads[0] = 0;
    let mut out = Vec::new();
    for (k, &a) in uploads.iter().enumerate() {
        let b = uploads.get(k + 1).copied().unwrap_or(stream.len());
        let program = microcode(&stream[a..b]).and_then(|m| PROGRAMS.iter().find(|p| m.len() == p.2 && fnv(&m) == p.1).map(|p| p.0));
        out.push(Piece { range: a..b, program });
    }
    Ok(out)
}

fn fnv(b: &[u8]) -> u64 {
    b.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &x| (h ^ x as u64).wrapping_mul(0x100_0000_01b3))
}

/// The microcode the stream's MPG packets upload, from micro address 0.
fn microcode(stream: &[u32]) -> Option<Vec<u8>> {
    let mut img: Vec<u8> = Vec::new();
    let mut i = 0;
    while i < stream.len() {
        let c = stream[i];
        let (cmd, num, imm) = ((c >> 24) & 0x7f, (c >> 16) & 0xff, (c & 0xffff) as usize);
        i += 1;
        match cmd {
            0x4a => {
                let n = if num == 0 { 256 } else { num as usize };
                let at = imm * 8;
                let words = stream.get(i..i + 2 * n)?;
                if img.len() < at + 8 * n {
                    img.resize(at + 8 * n, 0);
                }
                for (k, w) in words.iter().enumerate() {
                    img[at + 4 * k..at + 4 * k + 4].copy_from_slice(&w.to_le_bytes());
                }
                i += 2 * n;
            }
            _ => i += skip(stream, i - 1, &VifRegs::default())?,
        }
    }
    (!img.is_empty()).then_some(img)
}

/// The VIF registers the stream sets (STCYCL's CL and WL, STMASK, STCOL, STROW, STMOD, BASE, OFFSET) and the double buffer's
/// TOPS and DBF.
#[derive(Clone, Copy, Default)]
pub(crate) struct VifRegs {
    pub cl: u32,
    pub wl: u32,
    pub mask: u32,
    pub col: [u32; 4],
    pub row: [u32; 4],
    pub mode: u32,
    pub base: u32,
    pub offset: u32,
    pub tops: u32,
    pub dbf: bool,
}

impl VifRegs {
    /// An MSCAL's start: the run's TOP (TOPS now), and the next buffer the other one.
    pub fn start(&mut self) -> usize {
        let top = self.tops as usize;
        self.dbf = !self.dbf;
        self.tops = if self.dbf { self.base + self.offset } else { self.base };
        top
    }
}

/// The words of data a VIF code at `at` carries (after the code itself); None past the stream.
fn skip(stream: &[u32], at: usize, r: &VifRegs) -> Option<usize> {
    let c = *stream.get(at)?;
    let (cmd, num, imm) = ((c >> 24) & 0x7f, (c >> 16) & 0xff, c & 0xffff);
    Some(match cmd {
        0x20 => 1,
        0x30 | 0x31 => 4,
        0x4a => 2 * if num == 0 { 256 } else { num as usize },
        0x50 | 0x51 => 4 * if imm == 0 { 65536 } else { imm as usize },
        c if c >= 0x60 => unpack_words(c, num, r),
        _ => 0,
    })
}

/// The data words of an UNPACK (`cmd` its code) of `num` vectors with the cycle `r` has.
fn unpack_words(cmd: u32, num: u32, r: &VifRegs) -> usize {
    let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
    let n = if num == 0 { 256 } else { num } as usize;
    let bits = if vn == 3 && vl == 3 { 16 } else { (32 >> vl) * (vn as usize + 1) };
    let (cl, wl) = (r.cl.max(1) as usize, r.wl.max(1) as usize);
    let nd = if wl <= cl { n } else { (n / wl) * cl + (n % wl).min(cl) };
    (nd * bits).div_ceil(32)
}

/// Walks a piece of a pass's stream with `program` (the one its upload put in micro memory) and the VU's clip flags as the pass's
/// pieces before it left them: the GIF output, quadword by quadword, as the GS would receive it (the DIRECT packets and the
/// program's packets in their order), appended to `out` (the caller's, so that its room is kept from pass to pass and frame to
/// frame: a pass gives megabytes).
pub fn run(stream: &[u32], program: &mut dyn Micro, clip: &mut u32, out: &mut Vec<[u32; 4]>) -> Result<(), String> {
    // The VU's arithmetic in the host's single precision (`vufloat::rz`): rounding toward zero while the programs run.
    let _mode = crate::vufloat::rz::Mode::toward_zero();
    let mut mem: VuMem = [[0u32; 4]; 1024];
    let mut r = VifRegs { cl: 1, wl: 1, ..VifRegs::default() };
    let mut i = 0;
    let mut uploaded = false;
    while i < stream.len() {
        let c = stream[i];
        let (cmd, num, imm) = ((c >> 24) & 0x7f, (c >> 16) & 0xff, c & 0xffff);
        let data = i + 1;
        let n = skip(stream, i, &r).ok_or("vif: a VIF code past the stream")?;
        let words = stream.get(data..data + n).ok_or_else(|| format!("vif: VIF code {c:#010x} at word {i}: its data runs past the stream"))?;
        match cmd {
            0x00 | 0x07 | 0x10 | 0x11 | 0x13 | 0x06 => {}
            0x01 => (r.cl, r.wl) = (imm & 0xff, (imm >> 8) & 0xff),
            0x02 => {
                r.offset = imm & 0x3ff;
                r.dbf = false;
                r.tops = r.base;
            }
            0x03 => r.base = imm & 0x3ff,
            0x05 => r.mode = imm & 3,
            0x20 => r.mask = words[0],
            0x30 => r.row.copy_from_slice(words),
            0x31 => r.col.copy_from_slice(words),
            0x4a => uploaded = true,
            0x50 | 0x51 => out.extend(words.chunks_exact(4).map(|q| [q[0], q[1], q[2], q[3]])),
            0x14 | 0x15 | 0x17 => {
                if !uploaded {
                    return Err("vif: a program run before the microcode".into());
                }
                if cmd == 0x17 {
                    return Err("vif: MSCNT".into());
                }
                // The run's TOP is the buffer TOPS named; the next buffer is the other one.
                let top = r.start();
                program.call(imm * 8, &mut mem, top, clip, out)?;
            }
            c if c >= 0x60 => unpack(&mut mem, c, num, imm, words, &mut r)?,
            c => return Err(format!("vif: VIF command {c:#x} at word {i}")),
        }
        i = data + n;
    }
    Ok(())
}

/// UNPACK: `num` vectors of the format in `cmd` from `words` into VU memory (7.3 of the VIF's manual: the cycle, the mask, the
/// column and row registers, STMOD's addition). The components a format does not give are as rac-rs's VIF1 (the emulated
/// reference's) fills them: an S vector's one value in all four, a V2's X and Y again as Z and W, a V3's W left as it was. STMOD
/// 1 (offset) writes a value read from the data plus ROW's, 2 (difference) that sum and keeps it in ROW.
pub(crate) fn unpack(mem: &mut [[u32; 4]], cmd: u32, num: u32, imm: u32, words: &[u32], r: &mut VifRegs) -> Result<(), String> {
    let (vn, vl, masked) = ((cmd >> 2) & 3, cmd & 3, cmd & 0x10 != 0);
    let usn = imm & 0x4000 != 0;
    let addr = (imm & 0x3ff) as usize + if imm & 0x8000 != 0 { r.tops as usize } else { 0 };
    let n = if num == 0 { 256 } else { num as usize };
    let (cl, wl) = (r.cl.max(1) as usize, r.wl.max(1) as usize);
    if wl > cl {
        return Err("vif: an UNPACK with WL over CL".into());
    }
    if r.mode == 3 {
        return Err("vif: STMOD 3".into());
    }
    let comps = vn as usize + 1;
    let size = 32 >> vl;
    if vn == 3 && vl == 3 {
        return Err("vif: an UNPACK V4-5".into());
    }
    let elem = |j: usize| -> u32 {
        let bit = j * size;
        let w = words[bit / 32];
        let v = if size == 32 { w } else { (w >> (bit % 32)) & ((1 << size) - 1) };
        if usn || size == 32 { v } else { ((v << (32 - size)) as i32 >> (32 - size)) as u32 }
    };
    for v in 0..n {
        let at = addr + (v / wl) * cl + v % wl;
        let q = mem.get_mut(at).ok_or_else(|| format!("vif: an UNPACK to {at:#x}"))?;
        let row = (v % wl).min(3);
        let e = |x: usize| elem(v * comps + x);
        let data = match comps {
            1 => [e(0); 4],
            2 => [e(0), e(1), e(0), e(1)],
            3 => [e(0), e(1), e(2), 0],
            _ => [e(0), e(1), e(2), e(3)],
        };
        for x in 0..4 {
            if comps == 3 && x == 3 {
                continue;
            }
            let m = if masked { (r.mask >> (8 * row + 2 * x)) & 3 } else { 0 };
            match m {
                0 => {
                    q[x] = if r.mode == 0 { data[x] } else { data[x].wrapping_add(r.row[x]) };
                    if r.mode == 2 {
                        r.row[x] = q[x];
                    }
                }
                1 => q[x] = r.row[x],
                2 => q[x] = r.col[row],
                _ => {}
            }
        }
    }
    Ok(())
}

/// One XGKICK's packet as the GIF reads it from VU memory at `addr`: tags and their data (PACKED NLOOP x NREG quadwords, REGLIST
/// half that rounded up, IMAGE NLOOP) up to the tag with EOP, addresses wrapping at the end of memory.
pub fn kick(mem: &VuMem, addr: usize, out: &mut Vec<[u32; 4]>) -> Result<(), String> {
    let mut a = addr & 0x3ff;
    let mut read = 0;
    loop {
        let t = mem[a];
        let nloop = (t[0] & 0x7fff) as usize;
        let nreg = match (t[1] >> 28) & 15 {
            0 => 16,
            n => n as usize,
        };
        let len = match (t[1] >> 26) & 3 {
            0 => nloop * nreg,
            1 => (nloop * nreg).div_ceil(2),
            _ => nloop,
        };
        read += len + 1;
        if read > mem.len() {
            return Err(format!("vif: an XGKICK at {addr:#x} reads past all of VU memory without a tag with EOP"));
        }
        if a + len < mem.len() {
            out.extend_from_slice(&mem[a..=a + len]);
        } else {
            out.extend((0..=len).map(|k| mem[(a + k) & 0x3ff]));
        }
        a = (a + len + 1) & 0x3ff;
        if t[0] >> 15 & 1 != 0 {
            return Ok(());
        }
    }
}
