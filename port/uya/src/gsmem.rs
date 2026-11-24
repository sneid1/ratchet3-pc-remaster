//! GS memory across fields: the image transfers the game sent (`uyart::hw::GsUpload`, the frames' own and those of frames the
//! renderer did not draw), kept as the GS keeps them, so that a texture or a CLUT sampled by TBP / CBP that no transfer of the
//! frame's own chain covers (Clank's palettes, uploaded when he is loaded; the resident textures a level's start sends; the front
//! end's art) is read from where the GS holds it. Memory starts as the GS's after reset, every word 0. Every format's pages are
//! laid out texel by texel as the GS lays them out (GS manual, "Local Memory": PSMCT32, PSMCT24, PSMCT16, PSMCT16S, PSMT8,
//! PSMT4, the PSMT8H, PSMT4HL and PSMT4HH views of PSMCT32's words, and the Z formats; the same tables and arithmetic as rac-rs's
//! full GS, `engine/gsfull/gs_mem.rs`), and a texture of any of them is read back through its CLUT as the GS reads it.

const WORDS: usize = 1 << 20;

#[derive(Clone)]
pub(crate) struct GsMemory {
    words: Vec<u32>,
    /// Counts the transfers (a texture read from here is the same while this is).
    pub sent: u64,
}

/// The blocks of a page: PSMCT32's (and PSMT8's), PSMCT16's (and PSMT4's) and PSMCT16S's; a Z format's are its colour format's
/// with block number XOR 24.
const BLOCK: [[u32; 8]; 4] = [[0, 1, 4, 5, 16, 17, 20, 21], [2, 3, 6, 7, 18, 19, 22, 23], [8, 9, 12, 13, 24, 25, 28, 29], [10, 11, 14, 15, 26, 27, 30, 31]];
const BLOCK16: [[u32; 4]; 8] = [[0, 2, 8, 10], [1, 3, 9, 11], [4, 6, 12, 14], [5, 7, 13, 15], [16, 18, 24, 26], [17, 19, 25, 27], [20, 22, 28, 30], [21, 23, 29, 31]];
const BLOCK16S: [[u32; 4]; 8] = [[0, 2, 16, 18], [1, 3, 17, 19], [8, 10, 24, 26], [9, 11, 25, 27], [4, 6, 20, 22], [5, 7, 21, 23], [12, 14, 28, 30], [13, 15, 29, 31]];
/// A PSMCT32 block's words.
const COLUMN: [[u32; 8]; 8] = [
    [0, 1, 4, 5, 8, 9, 12, 13],
    [2, 3, 6, 7, 10, 11, 14, 15],
    [16, 17, 20, 21, 24, 25, 28, 29],
    [18, 19, 22, 23, 26, 27, 30, 31],
    [32, 33, 36, 37, 40, 41, 44, 45],
    [34, 35, 38, 39, 42, 43, 46, 47],
    [48, 49, 52, 53, 56, 57, 60, 61],
    [50, 51, 54, 55, 58, 59, 62, 63],
];

/// The bits a texel of format `psm` takes, or why `psm` is no GS format.
pub(crate) fn bits(psm: u64) -> Result<u32, String> {
    match psm {
        0x00 | 0x30 => Ok(32),
        0x01 | 0x31 => Ok(24),
        0x02 | 0x0a | 0x32 | 0x3a => Ok(16),
        0x13 | 0x1b => Ok(8),
        0x14 | 0x24 | 0x2c => Ok(4),
        _ => Err(format!("GS memory: PSM {psm:#x} is no GS format")),
    }
}

/// Where texel (x, y) of a buffer of format `psm` at block `bp`, `bw` x 64 texels wide, is: its word and the bit its value starts
/// at (PSMT8H, PSMT4HL and PSMT4HH: the top byte and nibbles of a PSMCT32 word).
fn place(psm: u64, bp: u32, bw: u32, x: u32, y: u32) -> (usize, u32) {
    Row::new(psm, bp, bw, y).at(x)
}

/// What `place` takes from a texel's row, so that a row's texels each cost their column's lookups only.
struct Row {
    /// `kind(psm)`.
    kind: u8,
    /// The bit a view of PSMCT32's words (PSMT8H, PSMT4HL, PSMT4HH) starts at; 0 for the others.
    vshift: u32,
    bp: u32,
    /// The Z formats' XOR on the block number.
    z: u32,
    /// The row's page row's first page: (y / page height) * pages across.
    ypage: u32,
    /// The row's blocks (PSMCT32's, PSMT8's, PSMT4's: a block row; PSMCT16's and PSMCT16S's: a block row's four).
    blocks: &'static [u32],
    /// The row's words of a block's column table.
    columns: &'static [u32; 8],
    swap: u32,
    yy: u32,
}

impl Row {
    fn new(psm: u64, bp: u32, bw: u32, y: u32) -> Row {
        let y = y & 0x7ff;
        let z = if psm >= 0x30 { 24 } else { 0 };
        let vshift = match psm {
            0x1b | 0x24 => 24,
            0x2c => 28,
            _ => 0,
        };
        let mut r = Row { kind: kind(psm), vshift, bp, z, ypage: 0, blocks: &BLOCK[0], columns: &COLUMN[0], swap: 0, yy: y & 15 };
        match psm {
            0x02 | 0x0a | 0x32 | 0x3a => {
                r.ypage = (y >> 6) * bw;
                r.blocks = if psm & 8 != 0 { &BLOCK16S[(y >> 3 & 7) as usize] } else { &BLOCK16[(y >> 3 & 7) as usize] };
                r.columns = &COLUMN[(y & 7) as usize];
            }
            0x13 => {
                r.ypage = (y >> 6) * (bw / 2).max(1);
                r.swap = ((r.yy + 2) >> 2 & 1) * 4;
                r.blocks = &BLOCK[(y >> 4 & 3) as usize];
                r.columns = &COLUMN[((((r.yy & !3) >> 1) + (r.yy & 1)) & 7) as usize];
            }
            0x14 => {
                r.ypage = (y >> 7) * (bw / 2).max(1);
                r.swap = ((r.yy + 2) >> 2 & 1) * 4;
                r.blocks = &BLOCK16[(y >> 4 & 7) as usize];
                r.columns = &COLUMN[((((r.yy & !3) >> 1) + (r.yy & 1)) & 7) as usize];
            }
            // PSMCT32, PSMCT24, PSMZ32, PSMZ24 and the views of PSMCT32's words.
            _ => {
                r.ypage = (y >> 5) * bw;
                r.blocks = &BLOCK[(y >> 3 & 3) as usize];
                r.columns = &COLUMN[(y & 7) as usize];
            }
        }
        r
    }

    #[inline(always)]
    fn at(&self, x: u32) -> (usize, u32) {
        match self.kind {
            1 => self.at_k::<1>(x),
            2 => self.at_k::<2>(x),
            3 => self.at_k::<3>(x),
            _ => self.at_k::<0>(x),
        }
    }

    /// `at` of the format kind `K` (`kind`).
    #[inline(always)]
    fn at_k<const K: u8>(&self, x: u32) -> (usize, u32) {
        let x = x & 0x7ff;
        let (word, shift) = match K {
            1 => {
                let page = self.ypage + (x >> 6);
                let hw = self.columns[(x & 7) as usize] * 2 + (x >> 3 & 1);
                ((self.bp + page * 32 + (self.blocks[(x >> 4 & 3) as usize] ^ self.z)) * 64 + hw / 2, 16 * (hw & 1))
            }
            2 => {
                let page = self.ypage + (x >> 7);
                let xx = x & 15;
                let w = (self.bp + page * 32 + self.blocks[(x >> 4 & 7) as usize]) * 64 + self.columns[((xx + self.swap) & 7) as usize];
                let byte = w * 4 + (xx >> 3 & 1) * 2 + (self.yy >> 1 & 1);
                (byte / 4, 8 * (byte % 4))
            }
            3 => {
                let page = self.ypage + (x >> 7);
                let xx = x & 31;
                let w = (self.bp + page * 32 + self.blocks[(x >> 5 & 3) as usize]) * 64 + self.columns[((xx + self.swap) & 7) as usize];
                let nibble = w * 8 + (xx >> 3 & 3) * 2 + (self.yy >> 1 & 1);
                (nibble / 8, 4 * (nibble % 8))
            }
            _ => {
                let page = self.ypage + (x >> 6);
                ((self.bp + page * 32 + (self.blocks[(x >> 3 & 7) as usize] ^ self.z)) * 64 + self.columns[(x & 7) as usize], self.vshift)
            }
        };
        (word as usize & (WORDS - 1), shift)
    }
}

/// The layout `Row::at_k` has for a format: 1 the 16-bit formats, 2 PSMT8, 3 PSMT4, 0 the rest (the 32- and 24-bit formats and the
/// views of PSMCT32's words).
fn kind(psm: u64) -> u8 {
    match psm {
        0x02 | 0x0a | 0x32 | 0x3a => 1,
        0x13 => 2,
        0x14 => 3,
        _ => 0,
    }
}

/// The colour of a CLUT entry or a texel of the 16- and 24-bit formats (`v`, `psm`'s bits): RGBA, alpha 0x80 opaque as the GS
/// has it; a 16-bit value's alpha TEXA's TA1 with its top bit set, else TA0, and a 24-bit value's TA0 (AEM: 0 where the colour is
/// black). Those take TEXA, which the jobs set (`texa`: None while none has).
pub(crate) fn colour(psm: u64, v: u32, texa: Option<u64>) -> Result<[u8; 4], String> {
    let texa = || texa.ok_or_else(|| format!("GS memory: a PSM {psm:#x} colour read before the jobs set TEXA"));
    Ok(match bits(psm)? {
        16 => {
            let t = texa()?;
            let c = |s: u32| ((v >> s & 0x1f) << 3) as u8;
            let a = if v & 0x8000 != 0 { t >> 32 } else if t >> 15 & 1 == 1 && v & 0x7fff == 0 { 0 } else { t };
            [c(0), c(5), c(10), a as u8]
        }
        24 => {
            let t = texa()?;
            let a = if t >> 15 & 1 == 1 && v & 0xff_ffff == 0 { 0 } else { t };
            [v as u8, (v >> 8) as u8, (v >> 16) as u8, a as u8]
        }
        _ => v.to_le_bytes(),
    })
}

impl GsMemory {
    pub fn new() -> GsMemory {
        GsMemory { words: vec![0; WORDS], sent: 0 }
    }

    /// The memory's words to `path` (the debugging switch `UYA_GSM_DUMP_AT`: a live run's GS memory for `uya native` to redraw a
    /// dumped field with, `UYA_GSM_LOAD`).
    pub fn dump(&self, path: &str) -> Result<(), String> {
        std::fs::write(path, bytemuck::cast_slice::<u32, u8>(&self.words)).map_err(|e| format!("{path}: {e}"))
    }

    pub fn load(&mut self, path: &str) -> Result<(), String> {
        let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        if b.len() != 4 * WORDS {
            return Err(format!("{path}: {} bytes, not GS memory's {}", b.len(), 4 * WORDS));
        }
        self.words.copy_from_slice(bytemuck::cast_slice(&b));
        self.sent += 1;
        Ok(())
    }

    /// One image transfer (host to local): BITBLTBUF, TRXPOS and TRXREG, and the data's quadwords, texel after texel from
    /// TRXPOS's corner row by row, each its format's bits (PSMCT24: three bytes); a transfer whose data ends short writes the
    /// texels it holds, as the GS does before the rest comes.
    pub fn upload(&mut self, bitblt: u64, trxpos: u64, trxreg: u64, data: &[u128]) -> Result<(), String> {
        self.sent += 1;
        let (dbp, bw, psm) = ((bitblt >> 32 & 0x3fff) as u32, (bitblt >> 48 & 0x3f) as u32, bitblt >> 56 & 0x3f);
        let (x0, y0) = ((trxpos >> 32 & 0x7ff) as u32, (trxpos >> 48 & 0x7ff) as u32);
        let (w, h) = ((trxreg & 0xfff) as u32, (trxreg >> 32 & 0xfff) as u32);
        let b = bits(psm)?;
        let bytes: &[u8] = bytemuck::cast_slice(data);
        let mask = if b == 32 { u32::MAX } else { (1u32 << b) - 1 };
        // The texels the data holds, of the w x h (a transfer whose data ends short writes the texels it holds).
        let held = match b {
            32 => bytes.len() / 4,
            24 => bytes.len() / 3,
            16 => bytes.len() / 2,
            8 => bytes.len(),
            _ => bytes.len() * 2,
        };
        let n = ((w * h) as usize).min(held);
        match b {
            32 => self.fill(psm, dbp, bw, (x0, y0, w), n, mask, |i| u32::from_le_bytes([bytes[4 * i], bytes[4 * i + 1], bytes[4 * i + 2], bytes[4 * i + 3]])),
            24 => self.fill(psm, dbp, bw, (x0, y0, w), n, mask, |i| u32::from_le_bytes([bytes[3 * i], bytes[3 * i + 1], bytes[3 * i + 2], 0])),
            16 => self.fill(psm, dbp, bw, (x0, y0, w), n, mask, |i| u16::from_le_bytes([bytes[2 * i], bytes[2 * i + 1]]) as u32),
            8 if psm == 0x13 => self.fill_t8(dbp, bw, (x0, y0, w), &bytes[..n]),
            8 => self.fill(psm, dbp, bw, (x0, y0, w), n, mask, |i| bytes[i] as u32),
            _ => self.fill(psm, dbp, bw, (x0, y0, w), n, mask, |i| (bytes[i / 2] >> (4 * (i % 2)) & 15) as u32),
        }
        Ok(())
    }

    /// The first `n` texels of a transfer `w` wide from (x0, y0), row by row, texel i's value `value(i)`, each written under `mask`.
    #[inline(always)]
    fn fill(&mut self, psm: u64, dbp: u32, bw: u32, at: (u32, u32, u32), n: usize, mask: u32, value: impl Fn(usize) -> u32) {
        match kind(psm) {
            1 => self.fill_k::<1>(psm, dbp, bw, at, n, mask, value),
            2 => self.fill_k::<2>(psm, dbp, bw, at, n, mask, value),
            3 => self.fill_k::<3>(psm, dbp, bw, at, n, mask, value),
            _ => self.fill_k::<0>(psm, dbp, bw, at, n, mask, value),
        }
    }

    /// `fill` of the format kind `K`. The 32-bit layout (0) and the 16-bit one (1) are written a column of eight texels at a time:
    /// the page and block are found once for the eight, and the words come from the row's column table.
    #[inline(always)]
    fn fill_k<const K: u8>(&mut self, psm: u64, dbp: u32, bw: u32, (x0, y0, w): (u32, u32, u32), n: usize, mask: u32, value: impl Fn(usize) -> u32) {
        let words: &mut [u32; WORDS] = (&mut self.words[..]).try_into().expect("GS memory is WORDS words");
        let (mut i, mut row) = (0usize, 0u32);
        while i < n {
            let r = Row::new(psm, dbp, bw, y0 + row);
            let end = n.min(i + w as usize);
            if K <= 1 {
                let mut col = x0;
                while i < end {
                    let x = col & 0x7ff;
                    let run = ((8 - (x & 7)) as usize).min(end - i);
                    let page = r.ypage + (x >> 6);
                    // The words a column's texels land in are within one 64-word block: the block's base (masked), the column's offset.
                    let (block, shift) = if K == 0 { (r.blocks[(x >> 3 & 7) as usize], r.vshift) } else { (r.blocks[(x >> 4 & 3) as usize], 16 * (x >> 3 & 1)) };
                    let base = ((r.bp + page * 32 + (block ^ r.z)) * 64) as usize & (WORDS - 1);
                    let columns = &r.columns[(x & 7) as usize..];
                    for (j, &c) in columns[..run].iter().enumerate() {
                        let k = base + c as usize;
                        let v = value(i + j);
                        words[k] = if mask == u32::MAX { v } else { words[k] & !(mask << shift) | v << shift };
                    }
                    i += run;
                    col += run as u32;
                }
            } else {
                for (col, i) in (x0..).zip(i..end) {
                    let (k, shift) = r.at_k::<K>(col);
                    words[k] = words[k] & !(mask << shift) | value(i) << shift;
                }
            }
            i = end;
            row += 1;
        }
    }

    /// `fill` of PSMT8 (the textures the game sends each field): a texel is a byte of its word. A block's sixteen texels of a row
    /// are written together: the block is found once and the byte offsets of the row's texels come from a table.
    fn fill_t8(&mut self, dbp: u32, bw: u32, (x0, y0, w): (u32, u32, u32), texels: &[u8]) {
        let bytes: &mut [u8; 4 * WORDS] = (&mut bytemuck::cast_slice_mut::<u32, u8>(&mut self.words)[..]).try_into().expect("GS memory is 4 * WORDS bytes");
        for (row, line) in texels.chunks(w as usize).enumerate() {
            let r = Row::new(0x13, dbp, bw, y0 + row as u32);
            let yo = r.yy >> 1 & 1;
            // The byte offset in a block's 64 words of the texel at x & 15.
            let mut off = [0usize; 16];
            for (xx, o) in off.iter_mut().enumerate() {
                let xx = xx as u32;
                *o = (r.columns[((xx + r.swap) & 7) as usize] * 4 + (xx >> 3 & 1) * 2 + yo) as usize;
            }
            let mut col = x0;
            let mut rest = line;
            while !rest.is_empty() {
                let x = col & 0x7ff;
                let run = ((16 - (x & 15)) as usize).min(rest.len());
                let page = r.ypage + (x >> 7);
                let base = (((dbp + page * 32 + r.blocks[(x >> 4 & 7) as usize]) * 64) as usize & (WORDS - 1)) * 4;
                for (&v, o) in rest[..run].iter().zip(&off[(x & 15) as usize..]) {
                    bytes[base + o] = v;
                }
                rest = &rest[run..];
                col += run as u32;
            }
        }
    }

    /// The value of texel (x, y) of the buffer of format `psm` at block `bp`, `bw` x 64 texels wide.
    fn value(&self, psm: u64, bp: u32, bw: u32, x: u32, y: u32) -> Result<u32, String> {
        let b = bits(psm)?;
        let (k, shift) = place(psm, bp, bw, x, y);
        Ok(if b == 32 { self.words[k] } else { self.words[k] >> shift & ((1u32 << b) - 1) })
    }

    /// The CLUT TEX0 samples with (CBP, CPSM PSMCT32, PSMCT16 or PSMCT16S, CSM1): 256 colours for an 8-bit texture (entry i
    /// at (i with bits 3 and 4 swapped) of the 16 x 16 at CBP), 16 for a 4-bit one (the 8 x 2 at CBP), RGBA in index order.
    pub fn clut(&self, tex0: u64, texa: Option<u64>) -> Result<Vec<[u8; 4]>, String> {
        let (cbp, cpsm, csm) = ((tex0 >> 37 & 0x3fff) as u32, tex0 >> 51 & 15, tex0 >> 55 & 1);
        if !matches!(cpsm, 0 | 2 | 0xa) || csm != 0 {
            return Err(format!("GS memory: TEX0 {tex0:#x}: a CLUT of CPSM {cpsm:#x}, CSM{}", csm + 1));
        }
        let entries = match bits(tex0 >> 20 & 0x3f)? {
            8 => 256,
            4 => 16,
            _ => return Err(format!("GS memory: TEX0 {tex0:#x}: no CLUT for a texture of its format")),
        };
        (0..entries)
            .map(|i: u32| {
                let (x, y) = if entries == 256 {
                    let p = (i & !0x18) | (i & 8) << 1 | (i & 0x10) >> 1;
                    (p % 16, p / 16)
                } else {
                    (i % 8, i / 8)
                };
                colour(cpsm, self.value(cpsm, cbp, 1, x, y)?, texa)
            })
            .collect()
    }

    /// The texture TEX0 samples (TBP, TBW, PSM, 2^TW x 2^TH; an indexed format through its CLUT, `clut`) as GS memory holds it:
    /// its texels row by row, RGBA, alpha 0x80 opaque; and an 8-bit texture with a PSMCT32 CLUT in its GS form (the indices and
    /// the CLUT in index order: `texnames::name` names it).
    pub fn texture(&self, tex0: u64, texa: Option<u64>) -> Result<(Vec<[u8; 4]>, Option<uyafmt::core::GsLevel>), String> {
        let (tbp, tbw, psm) = ((tex0 & 0x3fff) as u32, (tex0 >> 14 & 0x3f) as u32, tex0 >> 20 & 0x3f);
        let (lw, lh) = (tex0 >> 26 & 15, tex0 >> 30 & 15);
        if lw > 10 || lh > 10 {
            return Err(format!("GS memory: TEX0 {tex0:#x}: {} x {} texels, past the GS's 1024", 1u64 << lw, 1u64 << lh));
        }
        let (w, h) = (1u32 << lw, 1u32 << lh);
        let b = bits(psm)?;
        let mut values = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            let r = Row::new(psm, tbp, tbw, y);
            values.extend((0..w).map(|x| {
                let (k, shift) = r.at(x);
                if b == 32 { self.words[k] } else { self.words[k] >> shift & ((1u32 << b) - 1) }
            }));
        }
        match bits(psm)? {
            8 | 4 => {
                let clut = self.clut(tex0, texa)?;
                let px = values.iter().map(|&v| clut[v as usize]).collect();
                let gs = (bits(psm)? == 8 && tex0 >> 51 & 15 == 0).then(|| uyafmt::core::GsLevel { psm: psm as u32, texels: values.iter().map(|&v| v as u8).collect(), clut });
                Ok((px, gs))
            }
            _ => Ok((values.iter().map(|&v| colour(psm, v, texa)).collect::<Result<_, _>>()?, None)),
        }
    }
}
