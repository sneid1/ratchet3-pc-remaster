//! The LEVEL core's terrain (the core index's word 2; a core offset, 0 in all 34 story levels) and the region at word 3.
//!
//! The terrain (func_L01_00424DE8 sets it up at level load; func_L01_004ADBC0 draws it each frame through func_L01_004ADC98):
//! - a 0x40-byte header: +0 the chunk table's offset (0x40), +4 the chunk count, +8 an f32 scale (0x1E0F1C), +0xC a count
//!   (0x1F3FD8); the rest is zero;
//! - the chunk table, 0x40 bytes per chunk: a bounding sphere (4 f32, read by func_L01_004ADC98 as the cull sphere), +0x10 the
//!   chunk's data (an offset from the table that func_L01_00424DE8 makes absolute), +0x14/+0x16/+0x1A/+0x1E and the byte
//!   counts at +0x20 to +0x26 the DMA references func_L01_004ADC98 builds for the three detail levels, +0x1C and +0x28 the
//!   GS register records and their number, +0x2C/+0x2E the sphere list, +0x30 the light table, +0x38 the box corners, +0x3A/+0x3B
//!   the mask and zone of the visibility test, +0x3C/+0x3D/+0x3E, and +0x34 to +0x36 slots the game writes (func_L01_004252F0 at
//!   load, func_L01_004AEC50, func_L01_004AF4D8);
//! - the chunks' data, one after the other, each: a VIF1 stream (STROW, STMOD, STCYCL, NOP and UNPACK codes with their payloads;
//!   VU1 programs 1 and 7 consume it) that ends at +0x1E; the vertex colours (+0x1E, RGBA bytes); the light table (+0x30: a
//!   16-byte header, then 8-byte entries, func_L01_004AEC50); the sphere list (+0x2E, 16-byte entries, func_L01_004AE8A0); the
//!   8 box corners (+0x38, 4 s16 each, func_L01_004ADC98);
//! - zero padding up to the next core offset (4 levels have 0x48430 to 0xFD2C0 bytes of it).
//!
//! The stream's UNPACK payloads (except the GS register records) are VU1 data read by microprograms 1 and 7: the 5-vector block
//! header (counts and VU addresses of the arrays), the vertex positions, UV records (s, t, vertex pointers), the strip vertex and
//! morph lists (UV record indexes) and the strip table; `payloads` checks every count, address and index against the arrays the
//! header names, and skips the few bytes no instruction reads.
//!
//! What `terrain` returns, and how (func_L01_004ADC98 builds one DMA packet per chunk and detail level; VU1 program 1 draws it):
//! - the three detail levels' VIF1 pieces, each sent after a STCYCL 4/4 (the tag template at 0x1DA320): far (code 6) the
//!   +0x21 quadwords at +0x14; mid (codes 8, 0xA) the +0x22 quadwords at +0x16; near (codes 0xE, 0x10, 0x14) the +0x20
//!   quadwords at +0x16, then the +0x23 at +0x1A. Then the colours: (+0x24, +0x25, +0x26) rounded down to 4 RGBA words from
//!   +0x1E, a V4-8 unsigned UNPACK to VU address +0x2A after STCYCL 2/1 (far, near: template 0x1DA330) or STMOD 0 (mid: template
//!   0x1DA340, whose stream left STCYCL 2/1), so colour i lands beside position i;
//! - every address is TOP-relative (UNPACK flag bit), so this decoder unpacks each detail level into a VU memory of its own
//!   and reads it as the microcode does: positions (V3-16 plus the STROW origin, VU1 ITOF0, the 1/1024 matrix: world units are
//!   the integer / 1024) at the header's +0xC address, two quadwords apart with their colour in the second; UV records whose
//!   s and t are the STROW row (0x45000000) plus the s16, read as f32 minus the first GS record's qword 1 w (2048.0); strip
//!   tables and lists whose entries plus the STROW row are UV record addresses;
//! - a strip-table entry starts a new triangle strip (the GIF tag template at VU 0x292 has PRE set and PRIM 0x7C, a triangle
//!   strip): x < 0 gives x + 128 vertices and, when z >= 0, the GS register record z / 5; x > 0 gives x vertices under the same
//!   record; x = 0 ends the table. The strips take the list's entries in order;
//! - the morph passes (micro 0x450 for B, 0x498 for A) place morph vertex i between its record's position and the mean of two
//!   parents (the position of the record its list entry names, and the record's +4 position) by depth; the extra passes
//!   (micro 0x13C0 for B, 0x13E8 for A) overwrite extra record j with the record its list entry names when both positions are
//!   past the band (SQ at micro 0x1600). Those are returned as lists; the full-detail vertex is the record's own position;
//! - the GS register records as func_L01_00424DE8 builds them: +0 the terrain texture record, +0x10 K (TEX1 bits 32-43, signed
//!   1/16), +0x14 MMIN, +0x20 WMS and +0x24 WMT (CLAMP_1: 0 repeat, 1 clamp).
//!
//! Checked by use (13 November 2026, Veldin field 2400): the game drawing only its terrain programs (VU1 1 and 7; the emulated
//! GS with RAC_GSFULL_ONLY, captures/terrainonly2) shows the same shapes, in the same place, as these positions, strips, UVs and
//! textures drawn natively (captures/terrain1). The colours are not what the game draws:
//!
//! The colours at +0x1E are not what the game draws: func_L01_004AEC50 (the terrain light pass) overwrites them. The level
//! loader func_L01_004252F0 calls it at 0x425860 for every chunk (its argument is a list of chunk indices ended by 0xFFFF), after
//! clearing the 0x400 bytes at 0x1F17C0 (0x4256F0) and copying the gameplay block's light sets there (gameplay::light_sets, at
//! most 12). For each chunk (the table at [gp+0x4660]) it reads +0x10, +0x1E, +0x29, +0x30, +0x34, +0x36 and +0x3C:
//! - it first sends the 256-entry table of (cos, sin) f32 pairs at 0x2BF5A0 (boot_elf's data) to the scratchpad at 0x70003800
//!   (D_TOSPR at 0x4AECD0, 0x80 quadwords). The pairs are cos and sin of 2 pi i / 256 (Veldin's RAM at field 2400 holds these
//!   values to within 3.5e-7; `cos_sin` computes them);
//! - +0x34 is 0xFF on the disc in every chunk, so the per-chunk branch (one light set for the whole chunk, 0x4AED58) is not taken
//!   at load. Each of the first +0x3C (LB) light table entries, 8 bytes each from +0x10, is lit by the per-entry branch
//!   (0x4AEEE8): +2 and +3 are two byte indices a, b into the (cos, sin) table (LH, then PEXTLB/PEXTLH/PSLLW 3 at 0x4AEFF0), and
//!   the normal is -(cos a cos b, sin a cos b, sin b); +4 (LH) is an RGB555 base colour (PEXT5, then 65536.0 + c / 128 by PADDUW
//!   of 0x47800000, so 1.0 = 0x80); +6 (LH) names the light sets: low nibble set A; when the high byte (the weight w / 256 of
//!   set B: VITOF12 of it shifted 4) is not zero, the next nibble is set B and the sets are blended row by row (A (1 - w) + B w)
//!   and the two directions renormalised (VRSQRT); otherwise set A alone;
//! - the colour is base + colour0 max(d0, d0 colour0.w) + colour1 max(d1, d1 colour1.w), d the normal dotted with the set's
//!   direction; red, green and blue are clamped to 65536 + 255/128 (VMINIX with 0x478000FF), alpha is the base's; each float's
//!   low byte (PPACH, PPACB) is colour i of the chunk (D_FROMSPR back to +0x1E, +0x29 quadwords);
//! - then a point-light pass (0x4AF0C8) adds, for each nibble of +0x36 up to 0xF, the point light record at 0x1F1BC0 + 32 n:
//!   entry +0 is the offset from the chunk's data to that vertex's three s16 in the stream (the first offset used as is, the
//!   others with bit 0 cleared), the header the origin added to them. +0x36 is 0xFFFF on the disc and still at Veldin field 2400
//!   in every chunk; point lights are the game's runtime lights (func_L01_004AF4D8), not the level's data.
//!
//! The arithmetic runs on VU0 in macro mode: every product and sum is rounded toward zero to single precision, as `light`
//! reproduces (`vu_mul`, `vu_add`).
//!
//! The region at word 3 (absent in 7 levels; func_L01_00428750 stores it at 0x1EA9E0 and 0x1DFC00) is the terrain's
//! visibility grid: func_L01_003FECA8 looks a cell up in three levels (the third coordinate, then the second, then the first; each
//! level a u16 first cell, a u16 count and u16 entries) and returns a 0x80-byte zone record, func_L01_003FEE38 copies the records of
//! the camera's cell and its neighbours to 0x1EA8C0 and func_L01_004ADC98 copies that to the scratchpad (0x70003B00) and keeps a
//! chunk when its +0x3A mask ANDed with the record's byte at the chunk's +0x3B zone is not zero.

use crate::disc::{u16_at, u32_at};
use crate::rd::Fields;
use crate::track::{LineId, Node, Space, Tracker, NOTHING};

const GT: &str = "LEVEL core terrain (unpacked)";
const GV: &str = "LEVEL core terrain visibility grid (unpacked)";
const DT: &str = "terrain::terrain";
const DV: &str = "terrain::region3";

fn done(t: &mut Tracker, mut n: Node, r: Result<(), String>) -> Result<(), String> {
    match r {
        Ok(()) => {
            t.close(n);
            Ok(())
        }
        Err(e) => {
            n.fail(e.clone());
            t.close(n);
            Err(e)
        }
    }
}

/// A line of the unpacked core.
fn line(t: &mut Tracker, group: &str, name: &str, decoder: &'static str, game: &'static str) -> usize {
    t.line(group, name, decoder, game, Space::Unpacked)
}

// ---- what the terrain decoder returns ----

/// A terrain (the core's, or a level chunk's stream A): its LOD scale and its chunks.
pub struct Terrain {
    /// Header +8: the LOD scale B. func_L01_00424DE8 stores the band depths 6B, 4B and 2B (0x1E0F20, 0x1E0F24, 0x1E0F28).
    pub lod_scale: f32,
    /// Header +0xC: the count func_L01_00424DE8 stores at 0x1F3FD8, which func_L01_004252F0 compares with the gameplay block's
    /// chunk pairs in a level with level chunks (0x427958).
    pub header_count: u32,
    pub chunks: Vec<TerrainChunk>,
}

/// One chunk: its vertices at full detail, the strips of each detail level, and the morph and snap lists that move vertices
/// between detail levels.
pub struct TerrainChunk {
    /// The cull sphere (+0: centre x, y, z and radius), in world units.
    pub sphere: [f32; 4],
    /// The 8 box corners (+0x38), in world units: func_L01_004ADC98 widens each s16 by 64 (PEXTLH, PSRAW 10) under the 1/1024
    /// matrix.
    pub corners: [[f32; 3]; 8],
    /// +0x27: the chunk is drawn at the far detail level at any depth (func_L01_004ADC98 branches to code 6).
    pub force_far: bool,
    /// The mask and zone byte the game gives the chunk at load (+0x3A, +0x3B in RAM; `ZoneKeys`): the chunk is drawn when the mask
    /// ANDed with the zone's byte of the frame's zone record is not zero.
    pub visibility: (u8, u8),
    /// Every position, in world units: the base vertices, then the A morph vertices, then the B morph vertices.
    pub positions: Vec<[f32; 3]>,
    /// Each position's colour as the game draws it (RGBA, 0x80 = 1.0): the terrain light pass func_L01_004AEC50 at level load,
    /// from the chunk's light table and the level's light sets (see the module doc).
    pub colours: Vec<[u8; 4]>,
    /// Each position's normal as the light pass builds it from its light table entry: -(cos a cos b, sin a cos b, sin b).
    pub normals: Vec<[f32; 3]>,
    /// Each position's light table entry's other inputs to the light pass: its base colour and its light sets.
    pub lights: Vec<TerrainLight>,
    /// The UV records in VU order: base, A (morph then extra), B (morph then extra). Strips index this list.
    pub vertices: Vec<TerrainVertex>,
    /// Far, mid and near: each uses a prefix of `vertices` (base; base and A; all).
    pub lods: [TerrainLod; 3],
    /// The GS register records the strips select.
    pub materials: Vec<TerrainMaterial>,
    /// Per morph vertex [its position, parent position, parent position]: A (mid and near), B (near).
    pub morph: [Vec<[u32; 3]>; 2],
    /// Per extra record [its vertex, the position whose depth is tested with the other vertex's, the vertex it is overwritten
    /// with]: A (mid, code 8), B (near, code 0xE).
    pub snap: [Vec<[u32; 3]>; 2],
}

/// What the terrain light pass lights a position with besides its normal (`light`): the colour is
/// `base + colour0 max(d0, d0 colour0.w) + colour1 max(d1, d1 colour1.w)` of the sets `sets` names, d the normal dotted with the
/// set's directions, red, green and blue held at 255.
#[derive(Clone, Copy, Debug)]
pub struct TerrainLight {
    /// The entry's +4 RGB555 base colour as bytes (each 5-bit field to bits 3 to 7, the alpha bit to bit 7; 0x80 = 1.0).
    pub base: [u8; 4],
    /// The entry's +6: low nibble set A; the next nibble set B and the high byte its weight w / 256 when that byte is not zero
    /// (the sets blended row by row, A (1 - w) + B w, the directions renormalised), else set A alone.
    pub sets: u16,
}

pub struct TerrainVertex {
    pub uv: [f32; 2],
    /// Index into `positions`.
    pub position: u32,
}

pub struct TerrainLod {
    /// How many of the chunk's `vertices` this level's strips may use.
    pub vertices: usize,
    pub strips: Vec<TerrainStrip>,
}

pub struct TerrainStrip {
    /// Index into the chunk's `materials`.
    pub material: usize,
    /// Indices into the chunk's `vertices`; triangle k is vertices k, k+1, k+2.
    pub vertices: Vec<u32>,
}

pub struct TerrainMaterial {
    /// Index into the core's terrain texture records.
    pub texture: u32,
    /// TEX1 K, in mip levels.
    pub k: f32,
    /// TEX1 MMIN.
    pub mmin: u32,
    /// CLAMP_1 WMS and WMT: true = clamp, false = repeat.
    pub clamp: [bool; 2],
}

// ---- the terrain ----

/// One chunk table entry, as far as the decoder needs it.
struct PayLines {
    hdr: LineId,
    pos: LineId,
    rec0: LineId,
    rec1: LineId,
    strips: LineId,
    morph: LineId,
    table: LineId,
}

struct Chunk {
    /// The chunk data's offset in the terrain.
    a: usize,
    o16: usize,
    o1a: usize,
    o1c: usize,
    o1e: usize,
    o2c: usize,
    o2e: usize,
    o30: usize,
    o38: usize,
    n28: usize,
    /// Quadwords of the near level's first and second pieces, the far and the mid level's (+0x20 to +0x23).
    q: [usize; 4],
    /// Colour words of the far, mid and near levels (+0x24 to +0x26) and their VU address (+0x2A).
    ncol: [usize; 3],
    col_addr: usize,
    sphere: [f32; 4],
    force_far: bool,
    /// +0x3A (u16): the index of the chunk's pair in the gameplay block's +0x90 table; +0x3D: that pair's id.
    zone_pair: usize,
    id: u8,
    /// +0x3C (LB): how many light table entries the light pass lights, one per position.
    lit: usize,
}

/// What a chunk's VIF stream holds: the ranges of its code words, of its payloads by kind, the GS register records' payload
/// and the rows its STROW codes carry.
struct Stream {
    codes: Vec<(u64, u64)>,
    ups: Vec<Up>,
    gs: Option<(usize, usize)>,
    rows: Vec<usize>,
}

/// One UNPACK of a chunk stream other than the GS register records: its kind (the VIF command), the flag bits, the VU1 address,
/// the vector count, where its payload sits in the terrain, its byte size and the STROW row in effect.
struct Up {
    kind: u32,
    usn: bool,
    addr: usize,
    num: usize,
    at: usize,
    nb: usize,
    row: [u32; 4],
}

/// The UNPACK kinds the terrain streams use, by the command (low bits) and the flag bits of the immediate.
const KINDS: [(u32, u32); 5] = [(0x6e, 3), (0x6e, 2), (0x6d, 3), (0x6d, 2), (0x69, 2)];

/// The VIF1 codes of the stream [a, a + len): every code word and the payload of every UNPACK.
fn stream(b: &[u8], a: usize, len: usize) -> Result<Stream, String> {
    let mut s = Stream { codes: Vec::new(), ups: Vec::new(), gs: None, rows: Vec::new() };
    let mut row = [0u32; 4];
    let mut p = a;
    while p < a + len {
        let w = u32_at(b, p)?;
        let (cmd, num, imm) = ((w >> 24) & 0x7f, (w >> 16) & 0xff, w & 0xffff);
        if cmd >= 0x60 {
            let kind = cmd & 0x6f;
            let bits = match kind {
                0x6e => 32,
                0x6d => 64,
                0x69 => 48,
                0x6c => 128,
                _ => return Err(format!("+{:#x}: UNPACK code {w:#010x}", p - a)),
            };
            if num == 0 {
                return Err(format!("+{:#x}: UNPACK code {w:#010x} of no vectors", p - a));
            }
            let nb = ((bits * num as usize + 31) / 32) * 4;
            if p + 4 + nb > a + len {
                return Err(format!("+{:#x}: UNPACK code {w:#010x} runs past the stream's {len:#x} bytes", p - a));
            }
            s.codes.push((p as u64, 4));
            if kind == 0x6c {
                if imm >> 14 != 2 || s.gs.is_some() {
                    return Err(format!("+{:#x}: UNPACK code {w:#010x}: a second or unflagged V4-32", p - a));
                }
                s.gs = Some((p + 4, num as usize));
            } else {
                KINDS.iter().position(|&x| x == (kind, imm >> 14)).ok_or_else(|| format!("+{:#x}: UNPACK code {w:#010x}", p - a))?;
                s.ups.push(Up { kind, usn: imm >> 14 & 1 == 1, addr: (imm & 0x3ff) as usize, num: num as usize, at: p + 4, nb, row });
            }
            p += 4 + nb;
        } else if cmd == 0x30 && w == 0x3000_0000 {
            s.codes.push((p as u64, 20));
            s.rows.push(p + 4);
            for i in 0..4 {
                row[i] = u32_at(b, p + 4 + 4 * i)?;
            }
            p += 20;
        } else if (cmd == 0 && w == 0) || (cmd == 1 && num == 0 && (imm == 0x102 || imm == 0x404)) || (cmd == 5 && num == 0 && imm <= 1) {
            s.codes.push((p as u64, 4));
            p += 4;
        } else {
            return Err(format!("+{:#x}: VIF code {w:#010x}", p - a));
        }
    }
    if p != a + len {
        return Err(format!("the stream overruns its {len:#x} bytes by {:#x}", p - a - len));
    }
    Ok(s)
}

fn header(b: &[u8], n: &mut Node) -> Result<usize, String> {
    let mut f = Fields::new(&b[..0x40], n);
    let off = f.u32(0)?;
    let count = f.u32(4)? as usize;
    f.u32(8)?;
    f.u32(0xc)?;
    f.fill(0x10, 0x30, 0, "terrain header padding")?;
    if off != 0x40 {
        return Err(format!("+0 the chunk table is at {off:#x}, expected 0x40"));
    }
    Ok(count)
}

/// The chunk table at 0x40: every field is read by the game or checked against what its builder derived it from.
fn table(b: &[u8], count: usize, n: &mut Node) -> Result<Vec<Chunk>, String> {
    let mut out = Vec::with_capacity(count);
    for k in 0..count {
        let r = 0x40 * k;
        let h = |o: usize| u16_at(b, r + o).map(|v| v as usize);
        let by = |o: usize| b.get(r + o).map(|&v| v as usize).ok_or_else(|| format!("chunk {k}: byte +{o:#x} past the table"));
        let (o14, o18, o1a, o1c, o1e) = (h(0x14)?, h(0x18)?, h(0x1a)?, h(0x1c)?, h(0x1e)?);
        let (o2c, o2e, o30, o32, o38) = (h(0x2c)?, h(0x2e)?, h(0x30)?, h(0x32)?, h(0x38)?);
        let (q21, q23, q26, q28, q29, q2b, q34, q35, o36) = (by(0x21)?, by(0x23)?, by(0x26)?, by(0x28)?, by(0x29)?, by(0x2b)?, by(0x34)?, by(0x35)?, h(0x36)?);
        let a = 0x40 + u32_at(b, r + 0x10)? as usize;
        // +0x14 is the first detail level's data offset (0: the chunk data starts with it) and +0x18 its end (16 bytes per
        // quadword of +0x21); +0x1E is where +0x1A's data (+0x23 quadwords) ends; the colours (+0x1E to +0x30) are +0x29 quadwords
        // (and +0x26 / 4); +0x32 repeats +0x2E; the sphere list is 16 bytes per +0x2C; +0x2B is 0, +0x34 is 0xFF, +0x35 and
        // +0x36 are the slots func_L01_004252F0 and func_L01_004AEC50 initialise (0 and 0xFFFF).
        let bad = o14 != 0 || o18 != 16 * q21 || o1e != o1a + 16 * q23 || o30 != o1e + 16 * q29 || q26 / 4 != q29 || o32 != o2e || q2b != 0 || q34 != 0xff || q35 != 0 || o36 != 0xffff || o38 != o2e + 16 * o2c || o2e < o30 + 16;
        if bad {
            return Err(format!(
                "chunk {k}: +0x14 {o14:#x}, +0x18 {o18:#x}, +0x1A {o1a:#x}, +0x1E {o1e:#x}, +0x21 {q21:#x}, +0x23 {q23:#x}, +0x26 {q26:#x}, +0x29 {q29:#x}, +0x2B {q2b:#x}, +0x2C {o2c:#x}, +0x2E {o2e:#x}, +0x30 {o30:#x}, +0x32 {o32:#x}, +0x34 {q34:#x}, +0x35 {q35:#x}, +0x36 {o36:#x}, +0x38 {o38:#x} are not the values the fields derive from"
            ));
        }
        // +0x27 is func_L01_004ADC98's force-far flag: 0 or 1 on the disc.
        let q27 = by(0x27)?;
        if q27 > 1 {
            return Err(format!("chunk {k}: +0x27 {q27:#x}, expected 0 or 1"));
        }
        let f = |o: usize| u32_at(b, r + o).map(f32::from_bits);
        n.mark(r as u64, 0x40);
        out.push(Chunk {
            a,
            o16: h(0x16)?,
            o1a,
            o1c,
            o1e,
            o2c,
            o2e,
            o30,
            o38,
            n28: q28,
            q: [by(0x20)?, q21, by(0x22)?, q23],
            ncol: [by(0x24)?, by(0x25)?, q26],
            col_addr: by(0x2a)?,
            sphere: [f(0)? / 1024.0, f(4)? / 1024.0, f(8)? / 1024.0, f(0xc)? / 1024.0],
            force_far: q27 == 1,
            zone_pair: h(0x3a)?,
            id: by(0x3d)? as u8,
            lit: by(0x3c)?,
        });
    }
    Ok(out)
}

/// The GS register records (5 qwords each: TEX0_1, TEX1_1, CLAMP_1, MIPTBP1_1, MIPTBP2_1 as A+D data and register addresses).
/// func_L01_00424DE8 reads the texture index at +0 and the words at +0x10, +0x14, +0x20 and +0x24 and builds the register values
/// in place; the register addresses and the other data words are fixed.
fn gs_records(b: &[u8], at: usize, count: usize, n: &mut Node) -> Result<Vec<TerrainMaterial>, String> {
    let mut out = Vec::with_capacity(count);
    for r in 0..count {
        let e = at + 80 * r;
        let q = |o: usize| b.get(e + o..e + o + 8).map(|s| u64::from_le_bytes(s.try_into().unwrap())).ok_or_else(|| format!("GS record {r} past the end"));
        let ok = u32_at(b, e + 4)? == 0 && q(8)? == 6 && q(0x18)? == 0x4500_0000_0000_0014 && q(0x28)? == 8 && q(0x30)? == 0 && q(0x38)? == 0x34 && q(0x40)? == 0 && q(0x48)? == 0x36;
        if !ok {
            return Err(format!("GS record {r}: +4 {:#x}, +8 {:#x}, +0x18 {:#x}, +0x28 {:#x}, +0x30 {:#x}, +0x38 {:#x}, +0x40 {:#x}, +0x48 {:#x}", u32_at(b, e + 4)?, q(8)?, q(0x18)?, q(0x28)?, q(0x30)?, q(0x38)?, q(0x40)?, q(0x48)?));
        }
        let (k, mmin, wms, wmt) = (u32_at(b, e + 0x10)?, u32_at(b, e + 0x14)?, u32_at(b, e + 0x20)?, u32_at(b, e + 0x24)?);
        if mmin > 5 || wms > 1 || wmt > 1 {
            return Err(format!("GS record {r}: MMIN {mmin}, WMS {wms}, WMT {wmt}"));
        }
        // K is TEX1 bits 32-43: the low 12 bits of +0x10, a signed count of 1/16 mip levels.
        let k12 = (k & 0xfff) as i32;
        out.push(TerrainMaterial { texture: u32_at(b, e)?, k: (if k12 >= 0x800 { k12 - 0x1000 } else { k12 }) as f32 / 16.0, mmin, clamp: [wms == 1, wmt == 1] });
    }
    n.mark(0, 80 * count as u64);
    Ok(out)
}

/// Vertex colours: RGBA bytes, alpha 0x80; an all-zero entry is a vertex slot nothing uses.
fn colours(b: &[u8], at: usize, len: usize, n: &mut Node) -> Result<(), String> {
    for i in (0..len).step_by(4) {
        let e = b.get(at + i..at + i + 4).ok_or_else(|| "colour past the end".to_string())?;
        if e[3] != 0x80 && e != [0, 0, 0, 0] {
            return Err(format!("+{i:#x}: colour {:02x}{:02x}{:02x}{:02x}, alpha neither 0x80 nor an unused slot", e[0], e[1], e[2], e[3]));
        }
    }
    n.mark(0, len as u64);
    Ok(())
}

/// One light table entry (8 bytes), as func_L01_004AEC50 reads it.
struct LightEntry {
    /// +0: the offset from the chunk's data to this vertex's three s16 in the stream (the point-light pass).
    at: usize,
    /// +2, +3: indices into the (cos, sin) table.
    angles: [u8; 2],
    /// +4: the RGB555 base colour (bit 15 the alpha bit).
    base: u16,
    /// +6: low nibble set A, next nibble set B, high byte the weight of B (0: set A alone).
    sets: u16,
}

/// The light table: a 16-byte header (the chunk's origin, the row of one of the stream's STROW codes; the point-light pass adds it
/// to each vertex's s16, LQ at 0x4AF124) and 8-byte entries of four u16, one per position, that func_L01_004AEC50 reads.
fn lights(b: &[u8], at: usize, len: usize, rows: &[usize], n: &mut Node) -> Result<([i32; 3], Vec<LightEntry>), String> {
    let head = b.get(at..at + 16).ok_or_else(|| "light table header past the end".to_string())?;
    if !rows.iter().any(|&r| &b[r..r + 16] == head) {
        return Err("the header is no STROW row of the chunk's stream".to_string());
    }
    if (len - 16) % 8 != 0 {
        return Err(format!("{len:#x} bytes are not a header and whole entries"));
    }
    let origin = [u32_at(b, at)? as i32, u32_at(b, at + 4)? as i32, u32_at(b, at + 8)? as i32];
    let mut out = Vec::with_capacity((len - 16) / 8);
    for e in (at + 16..at + len).step_by(8) {
        out.push(LightEntry { at: u16_at(b, e)? as usize, angles: [b[e + 2], b[e + 3]], base: u16_at(b, e + 4)?, sets: u16_at(b, e + 6)? });
    }
    n.mark(0, len as u64);
    Ok((origin, out))
}

/// The table func_L01_004AEC50 sends to the scratchpad at 0x70003800: cos and sin of 2 pi i / 256.
fn cos_sin(i: u8) -> [f32; 2] {
    let a = std::f64::consts::TAU * i as f64 / 256.0;
    [a.cos() as f32, a.sin() as f32]
}

/// A VU0 result: the exact value rounded toward zero to single precision.
fn vu_round(x: f64) -> f32 {
    let y = x as f32;
    if (y as f64).abs() > x.abs() { f32::from_bits(y.to_bits() - 1) } else { y }
}

fn vu_mul(a: f32, b: f32) -> f32 {
    vu_round(a as f64 * b as f64)
}

fn vu_add(a: f32, b: f32) -> f32 {
    vu_round(a as f64 + b as f64)
}

/// VADDAY.X then VMADDZ.X with vf21.x (1.0, from 0x1D6CC0): the sum of the products `p`.
fn vu_sum3(p: [f32; 3]) -> f32 {
    vu_add(vu_add(p[0], p[1]), vu_mul(1.0, p[2]))
}

fn vu_dot(a: [f32; 3], b: [f32; 4]) -> f32 {
    vu_sum3([vu_mul(a[0], b[0]), vu_mul(a[1], b[1]), vu_mul(a[2], b[2])])
}

/// Entry `e`'s normal and colour as func_L01_004AEC50's per-entry branch computes them from the level's light sets `sets` (as
/// copied to 0x1F17C0).
fn light(e: &LightEntry, sets: &[crate::gameplay::LightSet]) -> Result<([f32; 3], [u8; 4]), String> {
    // A nibble names one of 16 slots. Before the light pass the loader clears all 16 (0x400 bytes at 0x1F17C0, 0x4256F0) and
    // copies the level's sets into the first ones; the runtime lights later written into slots 13 and 14 (func_L01_00422048,
    // func_L01_004009B8, func_L01_004B7948) are not called from the loader. So a slot past the level's sets is all zero here.
    const CLEARED: crate::gameplay::LightSet = crate::gameplay::LightSet { colour: [[0.0; 4]; 2], direction: [[0.0; 4]; 2] };
    let set = |i: u16| if (i as usize) < sets.len() { &sets[i as usize] } else { &CLEARED };
    let a = set(e.sets & 0xf);
    let (mut colour, mut direction) = (a.colour, a.direction);
    let weight = e.sets >> 8;
    if weight != 0 {
        // 0x4AEF00: w = (weight << 4) / 4096; rows A x (1 - w) plus rows B x w; the directions renormalised.
        let b = set((e.sets >> 4) & 0xf);
        let w = weight as f32 / 256.0;
        let v = vu_add(1.0, -w);
        for l in 0..2 {
            for k in 0..4 {
                colour[l][k] = vu_add(vu_mul(colour[l][k], v), vu_mul(b.colour[l][k], w));
            }
            for k in 0..3 {
                direction[l][k] = vu_add(vu_mul(direction[l][k], v), vu_mul(b.direction[l][k], w));
            }
            let d = direction[l];
            let len2 = vu_sum3([vu_mul(d[0], d[0]), vu_mul(d[1], d[1]), vu_mul(d[2], d[2])]);
            // VRSQRT of 0 gives the largest float on the VU (no infinity), which leaves a zero direction zero.
            let q = if len2 == 0.0 { f32::MAX } else { vu_round(1.0 / (len2 as f64).sqrt()) };
            for k in 0..3 {
                direction[l][k] = vu_mul(direction[l][k], q);
            }
        }
    }
    let [ca, sa] = cos_sin(e.angles[0]);
    let [cb, sb] = cos_sin(e.angles[1]);
    let normal = [-vu_mul(ca, cb), -vu_mul(sa, cb), -sb];
    let mut s = [0f32; 2];
    for l in 0..2 {
        let d = vu_dot(normal, direction[l]);
        s[l] = d.max(vu_mul(d, colour[l][3]));
    }
    // PEXT5: each 5-bit field to bits 3-7, the alpha bit to bit 7; PADDUW 0x47800000 makes 65536 + k / 128.
    let k = [(e.base & 0x1f) << 3, ((e.base >> 5) & 0x1f) << 3, ((e.base >> 10) & 0x1f) << 3, (e.base >> 15) << 7];
    let top = f32::from_bits(0x4780_00ff);
    let mut out = [0u8; 4];
    for c in 0..4 {
        let base = f32::from_bits(0x4780_0000 + k[c] as u32);
        // The w rows were zeroed (VSUBW.W) before the sums: alpha stays the base's.
        let (c0, c1) = if c < 3 { (colour[0][c], colour[1][c]) } else { (0.0, 0.0) };
        let mut v = vu_add(vu_add(base, vu_mul(c0, s[0])), vu_mul(c1, s[1]));
        if c < 3 {
            v = v.min(top);
        }
        if !(65536.0..131072.0).contains(&v) {
            return Err(format!("colour channel {c} comes to {v} (base {:#06x}, sets {:#06x})", e.base, e.sets));
        }
        out[c] = v.to_bits() as u8;
    }
    Ok((normal, out))
}

fn corners(b: &[u8], at: usize, n: &mut Node) -> Result<(), String> {
    for i in 0..8 {
        let w = u16_at(b, at + 8 * i + 6)?;
        if w != 0 {
            return Err(format!("corner {i}: fourth s16 is {w:#x}, expected 0"));
        }
    }
    n.mark(0, 0x40);
    Ok(())
}

/// The terrain at `at` in the unpacked core `parent`; `b` are its bytes.
/// Which visibility key each chunk of a terrain gets at load (func_L01_004252F0 0x427910 to 0x427E20, func_L01_00428088), the u16
/// (key >> 3) << 8 | 1 << (key & 7) of a gameplay +0x90 pair at the chunk's +0x3A:
/// - `Cleared`: the level has no visibility grid (core index word 3) or no +0x90 table: every chunk gets 0x7F80 (mask 0x80, zone
///   0x7F);
/// - `Direct`: the level has both and no level chunks (settings +0x68 is 0): chunk i gets pair i when the pairs are as many as the
///   chunks and pair i's id is chunk i's +0x3D (0x4279B0); otherwise every chunk gets 0x7F80 (0x427B58);
/// - `Indexed`: the level has both and level chunks, and the core terrain's header +0xC equals the pair count (0x427958): each
///   chunk gets the pair its disc +0x3A names (func_L01_00428088 after the core and after each level chunk); when the counts
///   differ the caller passes `Cleared` (func_L01_00428088 writes 0x7F80).
pub enum ZoneKeys<'a> {
    Cleared,
    Direct(&'a [crate::gameplay::ChunkZone]),
    Indexed(&'a [crate::gameplay::ChunkZone]),
}

fn zone_key(p: &crate::gameplay::ChunkZone) -> (u8, u8) {
    (1u8 << (p.key & 7), (p.key >> 3) as u8)
}

/// `sets` are the level's light sets (the gameplay block's), which light the terrain (func_L01_004AEC50); `zones` says which
/// visibility key each chunk gets.
pub fn terrain(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8], sets: &[crate::gameplay::LightSet], zones: ZoneKeys) -> Result<Terrain, String> {
    let mut region = t.region(parent, GT, at, b.len() as u64);
    let place = region.place.clone();
    let r = terrain_in(t, &mut region, b, sets, zones).map_err(|e| format!("{place}: {e}"));
    t.close(region);
    r
}

fn terrain_in(t: &mut Tracker, region: &mut Node, b: &[u8], sets: &[crate::gameplay::LightSet], zones: ZoneKeys) -> Result<Terrain, String> {
    let size = b.len();
    let l_head = line(t, GT, "terrain header (64 bytes)", DT, "func_L01_00424DE8 (same in all 34 story levels): +0 the chunk table's offset, +4 the chunk count, +8 an f32 scale, +0xC a count; +0x10 to +0x3F are zero padding");
    let l_table = line(
        t,
        GT,
        "terrain chunk table (0x40 bytes per chunk)",
        DT,
        "func_L01_004ADC98 (same in all 34 story levels) reads +0 (sphere), +0x10 to +0x2A, +0x2C to +0x2E, +0x35, +0x36, +0x38 to +0x3B and +0x3E; func_L01_004AEC50 +0x10, +0x1E, +0x29, +0x30, +0x34 to +0x36 and +0x3C; func_L01_004252F0 +0x3D; func_L01_00424DE8 makes +0x10 absolute; +0x18, +0x2B, +0x32 are values derived from other fields, +0x35 and +0x36 are slots the game writes at load",
    );
    let l_codes = line(t, GT, "terrain chunk VIF codes (UNPACK, STROW, STMOD, STCYCL, NOP words)", DT, "VIF1 reads them from the DMA references func_L01_004ADC98 builds (chunk +0x14 to +0x26)");
    let l_gs = line(
        t,
        GT,
        "terrain chunk GS register records (5 A+D qwords: TEX0_1, TEX1_1, CLAMP_1, MIPTBP1_1, MIPTBP2_1)",
        DT,
        "func_L01_00424DE8 (same in all 34 story levels) builds the registers from the texture index at +0 and the words at +0x10, +0x14, +0x20, +0x24; func_L01_004ADB20 patches TEX0 and MIPTBP2; the register addresses and the other data words are fixed",
    );
    let l_pay = PayLines {
        hdr: line(
            t,
            GT,
            "terrain chunk VU1 block header (UNPACK V4-16 unsigned, 5 vectors: 17 read u16 fields, 3 no reader)",
            DT,
            "VU1 programs 1 and 7 (UNPACK to VU address 0, TOP-relative, ILW/ILWR through vi14): +0 u16 base vertex count (program 1 micro 0x288 ILWR.x), +4 A morph count (0x290, 0x498, 0xB00), +6 A extra count (0x13E8), +8 B morph count (0x450, 0xAB0), +0xA B extra count (0x13C0), +0xC VU address of the vertex array (0x278), +0x12 A record array (0x4A0), +0x14 A extra records (0x13F0), +0x16 B record array (0x458), +0x18 B extra records (0x13C8), +0x1A strip vertex list (0x1C50, 0x2308, 0x32D0; program 7 0x3F8), +0x1C A morph list (0x4A8), +0x1E A extra list (0x13F8), +0x20 B morph list (0x460), +0x22 B extra list (0x13D0), +0x24 strip table (0x1C48; program 7 0x3F0), +0x26 GS register records (0x1C10; program 7 0x3B0); no reader in program 1 or 7 (every ILW/LQ of VU 0-4 searched; changing them leaves the VU1 output packets unchanged): +2, +0xE, +0x10",
        ),
        pos: line(
            t,
            GT,
            "terrain chunk vertex positions (V3-16 s16 x, y, z, VU1 vertex array)",
            DT,
            "VU1 program 1 (micro 0x2F0-0x440 LQI vertex pairs, ITOF0 at 0x310 then MADDA with the matrix at VU 5-8; morph passes micro 0x498-0x7C8 ITOF0 at 0x540; program 7 micro 0x4B0 ITOF0 + MADDA); the STROW origin (the light table header) is added by VIF; padding to 4 bytes is zero",
        ),
        rec0: line(
            t,
            GT,
            "terrain chunk base UV records (V4-16: s, t, vertex pointer; third halfword no reader)",
            DT,
            "VU1 programs 1 and 7: s, t (+0, +2) read by LQ.xy (program 1 micro 0x1CD0, 0x1D30, 0x2008, 0x23B8; program 7 0x450), the vertex pointer (+6, added to the STROW base) by ILWR.w (0x1CC8, 0x23B0); +4 is read by no instruction (a mutation of every base record's +4 leaves the VU1 output packets unchanged)",
        ),
        rec1: line(
            t,
            GT,
            "terrain chunk morph and extra UV records (V4-16: s, t, second vertex pointer, vertex pointer)",
            DT,
            "VU1 program 1 (A and B morph passes micro 0x498-0x7C8 and 0x450: +4 is the second parent vertex, ILW.z at 0x548; extra passes micro 0x13C0-0x1990, 0x13E8: MTIR vi09, vf06z at 0x14E0), s, t by the draw loops' LQ.xy (0x1CD0), +6 vertex pointer by ILWR.w",
        ),
        strips: line(
            t,
            GT,
            "terrain chunk strip vertex lists (V4-8 unsigned: UV record indexes)",
            DT,
            "VU1 program 1 draw loop (micro 0x1C10-0x1F90, 0x1FB0-0x22D8, 0x22E8-0x25F0: ILWR.x, .y, .z, .w (vi03) of 4 entries per vector, each plus the STROW base is a UV record address); program 7 micro 0x430-0x5E0; entries beyond the strips' vertex total are zero padding",
        ),
        morph: line(
            t,
            GT,
            "terrain chunk morph lists (V4-8 unsigned: UV record indexes)",
            DT,
            "VU1 program 1 morph passes (micro 0x450, 0x498, 0xAB0, 0xB00: ILWR.x, .y, .z, .w (vi04) 0x510-0x6C0, 0xBA0-0xE18; extra passes 0x13C0, 0x13E8: ILWR (vi03) 0x1438-0x14F8); entries beyond the list's count are zero padding",
        ),
        table: line(
            t,
            GT,
            "terrain chunk strip table (V4-8: strip length and register flag x, flush flag y, GS register record z)",
            DT,
            "VU1 program 1 (micro 0x1C48-0x1CA0 first entry ILWR.x, ILWR.z vi09; 0x1990-0x1BD8 next entries ILWR.x at 0x19E0, ILW.y at 0x19E8, ILW.z at 0x19F0); x>0 continues the strip with x vertices, x<0 gives x+128 vertices and (z >= 0) a new GS register record 5*z qwords from VU 9, y<0 flushes the packet first, x=0 ends. Not read: w of every entry, y and z of continuation entries (x>0), y of the first entry (no instruction reads them; changing them leaves the VU1 output packets unchanged)",
        ),
    };
    let l_col = line(t, GT, "terrain chunk vertex colours (RGBA bytes)", DT, "func_L01_004ADC98 (same in all 34 story levels) references them (chunk +0x1E) as a V4-8 UNPACK for VU1 programs 1 and 7");
    let l_light = line(t, GT, "terrain chunk light table (16-byte header, 8-byte entries)", DT, "func_L01_004AEC50 (same in all 34 story levels): its per-entry light branch reads +2, +3 (cos, sin table indices), +4 (RGB555 base colour) and +6 (light sets and weight) of the first chunk +0x3C entries; its point-light pass the header (the chunk's origin, a STROW row of the stream) and +0 (the offset of the vertex's s16 in the stream)");
    let l_sph = line(t, GT, "terrain chunk sphere list (16 bytes: 3 f32, u16, u8, u8)", DT, "func_L01_004AE8A0 (same in all 34 story levels)");
    let l_box = line(t, GT, "terrain chunk box corners (8 x 4 s16)", DT, "func_L01_004ADC98 (same in all 34 story levels)");
    let l_pad = line(t, GT, "terrain end padding (zeros)", DT, NOTHING);

    if size < 0x40 {
        return Err(format!("{size:#x} bytes cannot hold the terrain header"));
    }
    let mut n = t.child(region, l_head, 0, 0x40);
    let r = header(b, &mut n);
    let count = match r {
        Ok(c) => {
            t.close(n);
            c
        }
        Err(e) => {
            n.fail(e.clone());
            t.close(n);
            return Err(e);
        }
    };
    let lod_scale = f32::from_bits(u32_at(b, 8)?);
    let header_count = u32_at(b, 0xc)?;
    if count == 0 && size == 0x40 {
        // A level chunk's terrain with no chunks (func_L01_00424DE8's loop does not run).
        return Ok(Terrain { lod_scale, header_count, chunks: Vec::new() });
    }
    if count == 0 || 0x40 + 0x40 * count > size {
        return Err(format!("{count} chunks cannot fit in the terrain's {size:#x} bytes"));
    }
    let mut n = t.child(region, l_table, 0x40, 0x40 * count as u64);
    let chunks = match table(&b[0x40..], count, &mut n) {
        Ok(c) => {
            t.close(n);
            c
        }
        Err(e) => {
            n.fail(e.clone());
            t.close(n);
            return Err(e);
        }
    };
    let cleared = vec![(0x80u8, 0x7fu8); count];
    let keys: Vec<(u8, u8)> = match zones {
        ZoneKeys::Cleared => cleared,
        ZoneKeys::Direct(p) => {
            if p.len() == count && chunks.iter().zip(p).all(|(c, p)| c.id == p.id) {
                p.iter().map(zone_key).collect()
            } else {
                cleared
            }
        }
        ZoneKeys::Indexed(p) => chunks
            .iter()
            .enumerate()
            .map(|(k, c)| p.get(c.zone_pair).map(zone_key).ok_or_else(|| format!("chunk {k}: +0x3A names visibility pair {} of {}", c.zone_pair, p.len())))
            .collect::<Result<_, String>>()?,
    };
    if chunks[0].a != 0x40 + 0x40 * count {
        return Err(format!("the first chunk's data is at {:#x}, the table ends at {:#x}", chunks[0].a, 0x40 + 0x40 * count));
    }
    let mut out = Terrain { lod_scale, header_count, chunks: Vec::with_capacity(count) };
    for k in 0..count {
        let c = &chunks[k];
        let end = if k + 1 < count { chunks[k + 1].a } else { size };
        if end < c.a + c.o38 + 0x40 || end > size || (k + 1 < count && chunks[k + 1].a <= c.a) {
            return Err(format!("chunk {k}: data {:#x} to {end:#x} cannot hold its parts (box corners end at {:#x})", c.a, c.a + c.o38 + 0x40));
        }
        let s = stream(b, c.a, c.o1e).map_err(|e| format!("chunk {k} at {:#x}: {e}", c.a))?;
        let (gs_at, gs_n) = s.gs.ok_or_else(|| format!("chunk {k}: no GS register records"))?;
        if gs_at - c.a != c.o1c || gs_n != 5 * c.n28 {
            return Err(format!("chunk {k}: GS records at +{:#x} of {gs_n} vectors, the table says +{:#x} and {}", gs_at - c.a, c.o1c, c.n28));
        }
        let mut n = t.pieces(region, l_codes, &s.codes);
        let total = n.size;
        n.mark(0, total);
        t.close(n);
        let mut n = t.child(region, l_gs, gs_at as u64, 80 * c.n28 as u64);
        let materials = match gs_records(b, gs_at, c.n28, &mut n) {
            Ok(m) => {
                t.close(n);
                m
            }
            Err(e) => {
                n.fail(e.clone());
                t.close(n);
                return Err(e);
            }
        };
        let base = payloads(t, region, b, k, c, &s, &l_pay)?;
        let mut n = t.child(region, l_col, (c.a + c.o1e) as u64, (c.o30 - c.o1e) as u64);
        let r = colours(b, c.a + c.o1e, c.o30 - c.o1e, &mut n);
        done(t, n, r)?;
        let mut n = t.child(region, l_light, (c.a + c.o30) as u64, (c.o2e - c.o30) as u64);
        let (origin, entries) = match lights(b, c.a + c.o30, c.o2e - c.o30, &s.rows, &mut n) {
            Ok(v) => {
                t.close(n);
                v
            }
            Err(e) => {
                n.fail(e.clone());
                t.close(n);
                return Err(format!("chunk {k}: light table: {e}"));
            }
        };
        out.chunks.push(chunk_data(b, c, base, materials, origin, &entries, sets, keys[k]).map_err(|e| format!("chunk {k} at {:#x}: {e}", c.a))?);
        if c.o2c > 0 {
            let mut n = t.child(region, l_sph, (c.a + c.o2e) as u64, 16 * c.o2c as u64);
            n.mark(0, 16 * c.o2c as u64);
            t.close(n);
        }
        let mut n = t.child(region, l_box, (c.a + c.o38) as u64, 0x40);
        let r = corners(b, c.a + c.o38, &mut n);
        done(t, n, r)?;
        let tail = end - (c.a + c.o38 + 0x40);
        if tail > 0 {
            let mut n = t.child(region, l_pad, (c.a + c.o38 + 0x40) as u64, tail as u64);
            let r = Fields::new(&b[c.a + c.o38 + 0x40..end], &mut n).fill(0, tail, 0, "terrain end padding");
            done(t, n, r)?;
        }
    }
    Ok(out)
}

// ---- the chunk as VU1 program 1 reads it ----

/// VU1 data memory: 1024 quadwords.
type Vu = Vec<[u32; 4]>;

/// The VIF1 state the chunk's codes change: STCYCL's CL and WL, STMOD's mode, STROW's row.
struct Vif {
    cl: usize,
    wl: usize,
    mode: u32,
    row: [u32; 4],
}

/// One UNPACK into `vu`: `kind` 0x6E (V4-8), 0x6D (V4-16), 0x69 (V3-16) or 0x6C (V4-32) with the flag bits of `imm` (TOP-relative
/// addresses are taken from 0), `num` vectors from `b[at..]`, written as VIF1 writes them under CL/WL and the offset mode.
fn unpack(vu: &mut Vu, st: &Vif, b: &[u8], at: usize, kind: u32, num: usize, imm: u32) -> Result<(), String> {
    let (addr, usn) = ((imm & 0x3ff) as usize, imm >> 14 & 1 == 1);
    if st.cl < st.wl || st.wl == 0 || st.mode > 1 {
        return Err(format!("UNPACK under CL {}, WL {}, mode {}", st.cl, st.wl, st.mode));
    }
    let (fields, width) = match kind {
        0x6e => (4, 1),
        0x6d => (4, 2),
        0x69 => (3, 2),
        0x6c => (4, 4),
        _ => return Err(format!("UNPACK kind {kind:#x}")),
    };
    for i in 0..num {
        let q = (addr + (i / st.wl) * st.cl + i % st.wl) & 0x3ff;
        for f in 0..fields {
            let p = at + (i * fields + f) * width;
            let v = match (width, usn) {
                (1, true) => b[p] as u32,
                (1, false) => b[p] as i8 as i32 as u32,
                (2, true) => u16::from_le_bytes([b[p], b[p + 1]]) as u32,
                (2, false) => i16::from_le_bytes([b[p], b[p + 1]]) as i32 as u32,
                _ => u32_at(b, p)?,
            };
            vu[q][f] = if st.mode == 1 { v.wrapping_add(st.row[f]) } else { v };
        }
    }
    Ok(())
}

/// The VIF1 codes of `b[a..a + len]` run into `vu`. `stream` has already checked every code.
fn run(vu: &mut Vu, st: &mut Vif, b: &[u8], a: usize, len: usize) -> Result<(), String> {
    let mut p = a;
    while p < a + len {
        let w = u32_at(b, p)?;
        let (cmd, num, imm) = ((w >> 24) & 0x7f, ((w >> 16) & 0xff) as usize, w & 0xffff);
        if cmd >= 0x60 {
            let kind = cmd & 0x6f;
            let bits = match kind {
                0x6e => 32,
                0x6d => 64,
                0x69 => 48,
                _ => 128,
            };
            unpack(vu, st, b, p + 4, kind, num, imm)?;
            p += 4 + ((bits * num + 31) / 32) * 4;
        } else if cmd == 0x30 {
            for i in 0..4 {
                st.row[i] = u32_at(b, p + 4 + 4 * i)?;
            }
            p += 20;
        } else {
            match cmd {
                1 => (st.cl, st.wl) = ((imm & 0xff) as usize, (imm >> 8) as usize),
                5 => st.mode = imm & 3,
                _ => {}
            }
            p += 4;
        }
    }
    if p != a + len {
        return Err(format!("a detail level's piece {a:#x}+{len:#x} ends inside a VIF code"));
    }
    Ok(())
}

/// The VU memory of one detail level (0 far, 1 mid, 2 near) of chunk `c`: its pieces, each after a STCYCL 4/4, then the colours.
fn level_vu(b: &[u8], c: &Chunk, lod: usize) -> Result<Vu, String> {
    let mut vu: Vu = vec![[0; 4]; 1024];
    let mut st = Vif { cl: 4, wl: 4, mode: 0, row: [0; 4] };
    let pieces: Vec<(usize, usize)> = match lod {
        0 => vec![(0, 16 * c.q[1])],
        1 => vec![(c.o16, 16 * c.q[2])],
        _ => vec![(c.o16, 16 * c.q[0]), (c.o1a, 16 * c.q[3])],
    };
    for (o, len) in pieces {
        if o + len > c.o1e {
            return Err(format!("detail level {lod}: piece +{o:#x}+{len:#x} runs past the stream's {:#x} bytes", c.o1e));
        }
        (st.cl, st.wl) = (4, 4);
        run(&mut vu, &mut st, b, c.a + o, len)?;
    }
    // The colour tag: STCYCL 2/1 for far and near, STMOD 0 for mid (whose stream sets STCYCL 2/1 before it ends).
    if lod == 1 {
        st.mode = 0;
    } else {
        (st.cl, st.wl) = (2, 1);
    }
    if (st.cl, st.wl, st.mode) != (2, 1, 0) {
        return Err(format!("detail level {lod}: the colours unpack under CL {}, WL {}, mode {}", st.cl, st.wl, st.mode));
    }
    let n = c.ncol[lod] / 4 * 4;
    if 4 * n > c.o30 - c.o1e {
        return Err(format!("detail level {lod}: {n} colours past the {} at +0x1E", (c.o30 - c.o1e) / 4));
    }
    unpack(&mut vu, &st, b, c.a + c.o1e, 0x6e, n, 0xc000 | c.col_addr as u32)?;
    Ok(vu)
}

/// The VU1 block header's u16 fields (vector v, field f) as program 1 reads them (ILW: the low 16 bits).
fn hw(vu: &Vu, v: usize, f: usize) -> usize {
    (vu[v][f] & 0xffff) as usize
}

/// Chunk `c` as program 1 builds it: every vertex at full detail, the strips of each detail level, the morph and snap lists.
/// `base` is the VU address of the base UV records.
fn chunk_data(b: &[u8], c: &Chunk, base: usize, materials: Vec<TerrainMaterial>, origin: [i32; 3], entries: &[LightEntry], sets: &[crate::gameplay::LightSet], visibility: (u8, u8)) -> Result<TerrainChunk, String> {
    let vus = [level_vu(b, c, 0)?, level_vu(b, c, 1)?, level_vu(b, c, 2)?];
    let near = &vus[2];
    // Every level uploads the same block header and GS records.
    for (lod, vu) in vus.iter().enumerate() {
        if vu[..9 + 5 * c.n28] != near[..9 + 5 * c.n28] {
            return Err(format!("detail level {lod}'s block header or GS records differ from the near level's"));
        }
    }
    let (n0, na, xa, nb, xb, vbase) = (hw(near, 0, 0), hw(near, 0, 2), hw(near, 0, 3), hw(near, 1, 0), hw(near, 1, 1), hw(near, 1, 2));
    let (rec_a, rec_ax, rec_b, rec_bx) = (hw(near, 2, 1), hw(near, 2, 2), hw(near, 2, 3), hw(near, 3, 0));
    let (s_addr, l_a, l_xa, l_b, l_xb, t_addr) = (hw(near, 3, 1), hw(near, 3, 2), hw(near, 3, 3), hw(near, 4, 0), hw(near, 4, 1), hw(near, 4, 2));
    let nvert = n0 + na + nb;
    let rec_end = rec_b + nb + xb;
    // The draw loop's STQ: s and t (f32) minus the first GS record's qword 1 w (VU 9 + 1).
    let st_base = f32::from_bits(near[10][3]);
    let pos_of = |vu: &Vu, rec: usize, f: usize| -> Result<u32, String> {
        let p = hw(vu, rec, f);
        if p < vbase || (p - vbase) % 2 != 0 || (p - vbase) / 2 >= nvert {
            return Err(format!("UV record at VU {rec:#x}: vertex pointer {p:#x} ({nvert} vertices from {vbase:#x})"));
        }
        Ok(((p - vbase) / 2) as u32)
    };
    // The light pass lights entry i into colour i, which lands beside position i: one entry per position.
    if c.lit != nvert || entries.len() < nvert || c.ncol[2] / 4 * 4 < nvert {
        return Err(format!("{nvert} positions, +0x3C {} lit entries, {} light table entries, {} near colours", c.lit, entries.len(), c.ncol[2]));
    }
    let mut positions = Vec::with_capacity(nvert);
    let mut colours = Vec::with_capacity(nvert);
    let mut normals = Vec::with_capacity(nvert);
    let mut lights_out = Vec::with_capacity(nvert);
    for i in 0..nvert {
        let pq = near[vbase + 2 * i];
        let raw = [pq[0] as i32, pq[1] as i32, pq[2] as i32];
        positions.push(raw.map(|v| v as f32 / 1024.0));
        // Entry i's +0 names this vertex's s16 in the stream; the point-light pass adds the origin to them (bit 0 cleared after
        // the first entry).
        let e = &entries[i];
        let at = c.a + if i == 0 { e.at } else { e.at & !1 };
        let s16 = [u16_at(b, at)?, u16_at(b, at + 2)?, u16_at(b, at + 4)?].map(|v| v as i16 as i32);
        if (i == 0 && e.at & 1 != 0) || (0..3).any(|k| s16[k].wrapping_add(origin[k]) != raw[k]) {
            return Err(format!("light entry {i}: +0 {:#x} names {s16:?}, position {i} is {raw:?} from the origin {origin:?}", e.at));
        }
        let (n, col) = light(e, sets).map_err(|m| format!("light entry {i}: {m}"))?;
        normals.push(n);
        colours.push(col);
        let k = |sh: u16| ((e.base >> sh) & 0x1f) as u8 * 8;
        lights_out.push(TerrainLight { base: [k(0), k(5), k(10), (e.base >> 15) as u8 * 0x80], sets: e.sets });
    }
    let mut vertices = Vec::with_capacity(rec_end - base);
    for r in base..rec_end {
        let q = near[r];
        vertices.push(TerrainVertex { uv: [f32::from_bits(q[0]) - st_base, f32::from_bits(q[1]) - st_base], position: pos_of(near, r, 3)? });
    }
    // A list entry (V4-8 unsigned plus the STROW row) is a UV record's VU address.
    let entry = |vu: &Vu, list: usize, i: usize, limit: usize| -> Result<u32, String> {
        let a = (vu[list + i / 4][i % 4] & 0xffff) as usize;
        if a < base || a >= limit {
            return Err(format!("list at VU {list:#x} entry {i}: UV record {a:#x}, the records run {base:#x} to {limit:#x}"));
        }
        Ok((a - base) as u32)
    };
    let mut morph: [Vec<[u32; 3]>; 2] = Default::default();
    let mut snap: [Vec<[u32; 3]>; 2] = Default::default();
    for (stage, (n, rec, list, x, xrec, xlist)) in [(na, rec_a, l_a, xa, rec_ax, l_xa), (nb, rec_b, l_b, xb, rec_bx, l_xb)].into_iter().enumerate() {
        for i in 0..n {
            let parent = entry(near, list, i, rec_end)? as usize;
            morph[stage].push([pos_of(near, rec + i, 3)?, vertices[parent].position, pos_of(near, rec + i, 2)?]);
        }
        for j in 0..x {
            snap[stage].push([(xrec + j - base) as u32, pos_of(near, xrec + j, 2)?, entry(near, xlist, j, rec_end)?]);
        }
    }
    // The strips of each level: the table at the header's +0x24, the list at +0x1A.
    let limits = [rec_a - base, rec_b - base, rec_end - base];
    let mut lods: Vec<TerrainLod> = Vec::with_capacity(3);
    for (lod, vu) in vus.iter().enumerate() {
        let mut strips = Vec::new();
        let (mut at, mut material) = (0usize, None);
        for e in 0.. {
            if t_addr + e >= 1024 {
                return Err(format!("detail level {lod}: the strip table runs past VU memory"));
            }
            let q = vu[t_addr + e];
            let (x, z) = (q[0] as i32, q[2] as i32);
            if x == 0 {
                break;
            }
            if x < 0 && z >= 0 {
                if z % 5 != 0 || z as usize / 5 >= materials.len() {
                    return Err(format!("detail level {lod}: strip table entry {e}: GS record {z}"));
                }
                material = Some(z as usize / 5);
            }
            let material = material.ok_or_else(|| format!("detail level {lod}: strip table entry {e} before any GS record"))?;
            let count = if x < 0 { (x + 128) as usize } else { x as usize };
            let vertices = (at..at + count).map(|i| entry(vu, s_addr, i, base + limits[lod])).collect::<Result<Vec<u32>, String>>()?;
            at += count;
            strips.push(TerrainStrip { material, vertices });
        }
        lods.push(TerrainLod { vertices: limits[lod], strips });
    }
    let corner = |i: usize| -> Result<[f32; 3], String> {
        let s = |k: usize| u16_at(b, c.a + c.o38 + 8 * i + 2 * k).map(|v| v as i16 as f32 * 64.0 / 1024.0);
        Ok([s(0)?, s(1)?, s(2)?])
    };
    Ok(TerrainChunk {
        sphere: c.sphere,
        corners: [corner(0)?, corner(1)?, corner(2)?, corner(3)?, corner(4)?, corner(5)?, corner(6)?, corner(7)?],
        force_far: c.force_far,
        visibility,
        positions,
        colours,
        normals,
        lights: lights_out,
        vertices,
        lods: lods.try_into().map_err(|_| "three detail levels".to_string())?,
        materials,
        morph,
        snap,
    })
}

// ---- the region at word 3 ----

/// The grid's nodes: `at` its offset, 4 + 2 x count bytes (a u16 first cell, a u16 count, count u16 entries); returns its count.
fn node(b: &[u8], at: usize, what: &str) -> Result<usize, String> {
    let count = u16_at(b, at + 2)? as usize;
    if at + 4 + 2 * count > b.len() {
        return Err(format!("{what} at {at:#x} with {count} entries runs past the region's {:#x} bytes", b.len()));
    }
    Ok(count)
}

/// The region at word 3 at `at` in the unpacked core `parent`; `b` are its bytes.
pub fn region3(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let mut region = t.region(parent, GV, at, b.len() as u64);
    let place = region.place.clone();
    let r = region3_in(t, &mut region, b).map_err(|e| format!("{place}: {e}"));
    t.close(region);
    r
}

fn region3_in(t: &mut Tracker, region: &mut Node, b: &[u8]) -> Result<(), String> {
    let size = b.len();
    let l_z = line(t, GV, "terrain visibility grid, first level (zone record offset, first cell, count, y node offsets)", DV, "func_L01_003FECA8 (same in all 34 story levels): +0 the zone records' offset, +4 the first cell of the third coordinate, +6 the cell count, +8 one u16 per cell, the second-level node's offset in words (0 = none)");
    let l_y = line(t, GV, "terrain visibility grid, second-level nodes (first cell, count, x node offsets)", DV, "func_L01_003FECA8 (same in all 34 story levels): a u16 first cell of the second coordinate, a u16 count, then one u16 per cell, the third-level node's offset in words (0 = none)");
    let l_x = line(t, GV, "terrain visibility grid, third-level nodes (first cell, count, zone record indices)", DV, "func_L01_003FECA8 (same in all 34 story levels): a u16 first cell of the first coordinate, a u16 count, then one u16 per cell, a zone record's index (0xFFFF = none)");
    let l_rec = line(
        t,
        GV,
        "terrain visibility zone records (0x80 mask bytes each)",
        DV,
        "func_L01_003FEE38 (same in all 34 story levels) copies the camera cell's record (and combines its neighbours') to 0x1EA8C0; func_L01_004ADC98 copies that to the scratchpad and ANDs chunk +0x3A with the byte at chunk +0x3B",
    );
    let l_pad = line(t, GV, "terrain visibility grid padding (zeros)", DV, NOTHING);

    if size < 8 {
        return Err(format!("{size:#x} bytes cannot hold the grid's first level"));
    }
    let leaf = u32_at(b, 0)? as usize;
    let zc = u16_at(b, 6)? as usize;
    if 8 + 2 * zc > size {
        return Err(format!("the first level's {zc} cells run past the region's {size:#x} bytes"));
    }
    let mut n = t.child(region, l_z, 0, (8 + 2 * zc) as u64);
    n.mark(0, (8 + 2 * zc) as u64);
    t.close(n);
    // The nodes, each placed once: the offsets are in words.
    let mut y_nodes = std::collections::BTreeSet::new();
    for z in 0..zc {
        let v = u16_at(b, 8 + 2 * z)? as usize;
        if v != 0 {
            y_nodes.insert(4 * v);
        }
    }
    let mut x_nodes = std::collections::BTreeSet::new();
    let mut y_ranges = Vec::new();
    for &y in &y_nodes {
        let c = node(b, y, "second-level node")?;
        y_ranges.push((y as u64, (4 + 2 * c) as u64));
        for i in 0..c {
            let v = u16_at(b, y + 4 + 2 * i)? as usize;
            if v != 0 {
                x_nodes.insert(4 * v);
            }
        }
    }
    let mut x_ranges = Vec::new();
    let mut records = std::collections::BTreeSet::new();
    for &x in &x_nodes {
        let c = node(b, x, "third-level node")?;
        x_ranges.push((x as u64, (4 + 2 * c) as u64));
        for i in 0..c {
            let v = u16_at(b, x + 4 + 2 * i)?;
            if v != 0xffff {
                records.insert(v as usize);
            }
        }
    }
    let nrec = records.len();
    if records.iter().copied().ne(0..nrec) {
        return Err("the zone record indices are not 0 to the highest without gaps".to_string());
    }
    if leaf + 0x80 * nrec > size || leaf < 8 + 2 * zc {
        return Err(format!("{nrec} zone records at {leaf:#x} do not fit the region's {size:#x} bytes"));
    }
    // The nodes must not overlap each other or the records.
    let mut all: Vec<(u64, u64)> = y_ranges.iter().chain(x_ranges.iter()).copied().chain([(leaf as u64, 0x80 * nrec as u64)]).chain([(0, (8 + 2 * zc) as u64)]).collect();
    all.sort_unstable();
    for w in all.windows(2) {
        if w[0].0 + w[0].1 > w[1].0 {
            return Err(format!("grid parts at {:#x}+{:#x} and {:#x} overlap", w[0].0, w[0].1, w[1].0));
        }
    }
    if !y_ranges.is_empty() {
        let mut n = t.pieces(region, l_y, &y_ranges);
        let total = n.size;
        n.mark(0, total);
        t.close(n);
    }
    if !x_ranges.is_empty() {
        let mut n = t.pieces(region, l_x, &x_ranges);
        let total = n.size;
        n.mark(0, total);
        t.close(n);
    }
    if nrec > 0 {
        let mut n = t.child(region, l_rec, leaf as u64, 0x80 * nrec as u64);
        n.mark(0, 0x80 * nrec as u64);
        t.close(n);
    }
    for (a, e) in region.holes() {
        let (a, e) = (a as usize, e as usize);
        let mut n = t.child(region, l_pad, a as u64, (e - a) as u64);
        let r = Fields::new(&b[a..e], &mut n).fill(0, e - a, 0, "terrain visibility grid padding");
        done(t, n, r)?;
    }
    Ok(())
}

// ---- the VU1 payloads of a chunk ----

fn i8_(v: u8) -> i32 {
    v as i8 as i32
}

/// The UNPACK payloads of chunk `k` by role: the block header (VU address 0) gives the counts and VU addresses the microprograms
/// loop over; every payload must be the vector array one of them names, with the sizes the counts give.
/// Returns the VU address of the base UV records.
fn payloads(t: &mut Tracker, region: &mut Node, b: &[u8], k: usize, c: &Chunk, s: &Stream, l: &PayLines) -> Result<usize, String> {
    let e = |m: String| format!("chunk {k}: {m}");
    let hs: Vec<&Up> = s.ups.iter().filter(|u| u.kind == 0x6d && u.usn && u.addr == 0).collect();
    if hs.len() != 1 || hs[0].num != 5 {
        return Err(e(format!("{} VU1 block headers", hs.len())));
    }
    let hu = hs[0];
    let hw = |v: usize, f: usize| -> Result<usize, String> { Ok(u16_at(b, hu.at + 8 * v + 2 * f)? as usize) };
    let (n0, na, xa) = (hw(0, 0)?, hw(0, 2)?, hw(0, 3)?);
    let (nb_, xb, vbase) = (hw(1, 0)?, hw(1, 1)?, hw(1, 2)?);
    let (rec_a, rec_ax, rec_b, rec_bx) = (hw(2, 1)?, hw(2, 2)?, hw(2, 3)?, hw(3, 0)?);
    let (s_addr, l_a, l_xa, l_b, l_xb, t_addr, gs_addr) = (hw(3, 1)?, hw(3, 2)?, hw(3, 3)?, hw(4, 0)?, hw(4, 1)?, hw(4, 2)?, hw(4, 3)?);
    let nvert = n0 + na + nb_;
    let (mut pos, mut rec0, mut rec1, mut strips, mut morph, mut tables): (Vec<&Up>, Vec<&Up>, Vec<&Up>, Vec<&Up>, Vec<(&Up, usize)>, Vec<&Up>) = Default::default();
    let (mut pos_n, mut recs_a, mut recs_b) = ([0usize; 3], 0usize, 0usize);
    let mut lists = [0usize; 4];
    for u in &s.ups {
        match (u.kind, u.usn) {
            (0x6d, true) => {
                if !std::ptr::eq(u, hu) {
                    return Err(e(format!("V4-16 unsigned UNPACK at VU {:#x}", u.addr)));
                }
            }
            (0x69, _) => {
                let r = if u.addr == vbase {
                    0
                } else if u.addr == vbase + 2 * n0 && na > 0 {
                    1
                } else if u.addr == vbase + 2 * (n0 + na) && nb_ > 0 {
                    2
                } else {
                    return Err(e(format!("V3-16 UNPACK at VU {:#x}", u.addr)));
                };
                pos_n[r] += 1;
                if u.num != [n0, na, nb_][r] {
                    return Err(e(format!("vertex array {r} has {} vertices, the header says {}", u.num, [n0, na, nb_][r])));
                }
                pos.push(u);
            }
            (0x6d, false) => {
                if u.addr == rec_a && na + xa > 0 {
                    recs_a += 1;
                    if u.num != na + xa {
                        return Err(e(format!("A records: {} vectors for {} + {}", u.num, na, xa)));
                    }
                    rec1.push(u);
                } else if u.addr == rec_b && nb_ + xb > 0 {
                    recs_b += 1;
                    if u.num != nb_ + xb {
                        return Err(e(format!("B records: {} vectors for {} + {}", u.num, nb_, xb)));
                    }
                    rec1.push(u);
                } else {
                    rec0.push(u);
                }
            }
            (0x6e, true) => {
                if u.addr == s_addr {
                    strips.push(u);
                } else {
                    let r = if u.addr == l_a && na > 0 {
                        0
                    } else if u.addr == l_xa && xa > 0 {
                        1
                    } else if u.addr == l_b && nb_ > 0 {
                        2
                    } else if u.addr == l_xb && xb > 0 {
                        3
                    } else {
                        return Err(e(format!("V4-8 unsigned UNPACK at VU {:#x}", u.addr)));
                    };
                    lists[r] += 1;
                    if u.num != [na, xa, nb_, xb][r].div_ceil(4) {
                        return Err(e(format!("morph list {r}: {} vectors for {} entries", u.num, [na, xa, nb_, xb][r])));
                    }
                    morph.push((u, [na, xa, nb_, xb][r]));
                }
            }
            (0x6e, false) => {
                if u.addr != t_addr {
                    return Err(e(format!("V4-8 UNPACK at VU {:#x}, the strip table is at {t_addr:#x}", u.addr)));
                }
                tables.push(u);
            }
            _ => return Err(e(format!("UNPACK kind {:#x}", u.kind))),
        }
    }
    if pos_n[0] != 1 || (na > 0) as usize != pos_n[1] || (nb_ > 0) as usize != pos_n[2] || rec0.len() != 1 || recs_a != (na + xa > 0) as usize || recs_b != (nb_ + xb > 0) as usize {
        return Err(e("the vertex or record arrays do not match the header's counts".to_string()));
    }
    if lists != [(na > 0) as usize, (xa > 0) as usize, (nb_ > 0) as usize, (xb > 0) as usize] {
        return Err(e("the morph lists do not match the header's counts".to_string()));
    }
    let r0 = rec0[0];
    if rec_a != r0.addr + r0.num || rec_ax != rec_a + na || rec_b != rec_ax + xa || rec_bx != rec_b + nb_ {
        return Err(e(format!("record array addresses {rec_a} {rec_ax} {rec_b} {rec_bx} do not follow the base records at {} + {}", r0.addr, r0.num)));
    }
    if l_xa != l_a + na.div_ceil(4) || l_b != l_xa + xa.div_ceil(4) || l_xb != l_b + nb_.div_ceil(4) || s_addr != l_xb + xb.div_ceil(4) {
        return Err(e("list addresses do not follow each other".to_string()));
    }
    if gs_addr != 9 || s.gs.is_none() {
        return Err(e(format!("GS register records at VU {gs_addr}, expected 9")));
    }
    let rec_end = rec_b + nb_ + xb;
    let ptr = |v: i32, what: &str, i: usize| -> Result<(), String> {
        if v < 0 || v & 1 != 0 || v as usize / 2 >= nvert {
            return Err(e(format!("{what} {i}: vertex pointer {v} ({nvert} vertices)")));
        }
        Ok(())
    };
    // The block header: 17 u16 fields the microcode reads, 3 nothing reads.
    let mut n = t.child(region, l.hdr, hu.at as u64, hu.nb as u64);
    for j in 0..20u64 {
        if j == 1 || j == 7 || j == 8 {
            n.skip(2 * j, 2);
        } else {
            n.mark(2 * j, 2);
        }
    }
    t.close(n);
    for u in pos {
        let mut n = t.child(region, l.pos, u.at as u64, u.nb as u64);
        let r = if b[u.at + 6 * u.num..u.at + u.nb].iter().any(|&v| v != 0) { Err(e("vertex array padding is not zero".to_string())) } else { Ok(()) };
        if r.is_ok() {
            n.mark(0, u.nb as u64);
        }
        done(t, n, r)?;
    }
    // Base records: s, t and the vertex pointer are read, the third halfword is not.
    let mut n = t.child(region, l.rec0, r0.at as u64, r0.nb as u64);
    let r = (|| {
        if r0.row[3] as usize != vbase {
            return Err(e(format!("base records' STROW vertex base {:#x}, header {vbase:#x}", r0.row[3])));
        }
        for i in 0..r0.num {
            ptr(u16_at(b, r0.at + 8 * i + 6)? as i16 as i32, "base record", i)?;
            n.mark(8 * i as u64, 4);
            n.skip(8 * i as u64 + 4, 2);
            n.mark(8 * i as u64 + 6, 2);
        }
        Ok(())
    })();
    done(t, n, r)?;
    for u in rec1 {
        let mut n = t.child(region, l.rec1, u.at as u64, u.nb as u64);
        let r = (|| {
            if u.row[2] as usize != vbase || u.row[3] as usize != vbase {
                return Err(e(format!("morph records' STROW vertex bases {:#x} {:#x}, header {vbase:#x}", u.row[2], u.row[3])));
            }
            for i in 0..u.num {
                ptr(u16_at(b, u.at + 8 * i + 4)? as i16 as i32, "morph record parent", i)?;
                ptr(u16_at(b, u.at + 8 * i + 6)? as i16 as i32, "morph record", i)?;
            }
            n.mark(0, u.nb as u64);
            Ok(())
        })();
        done(t, n, r)?;
    }
    // Entries are UV record indexes: base + entry must name a record of the three arrays; entries past `used` are zero padding.
    let entries = |u: &Up, used: usize, what: &str| -> Result<(), String> {
        if u.row != [r0.addr as u32; 4] {
            return Err(e(format!("{what} STROW row {:x?}, the base records are at {}", u.row, r0.addr)));
        }
        for (i, &v) in b[u.at..u.at + 4 * u.num].iter().enumerate() {
            if i < used {
                if r0.addr + v as usize >= rec_end {
                    return Err(e(format!("{what} entry {i} is {v}, the records end at +{}", rec_end - r0.addr)));
                }
            } else if v != 0 {
                return Err(e(format!("{what} padding entry {i} is {v}")));
            }
        }
        Ok(())
    };
    for (u, used) in morph {
        let mut n = t.child(region, l.morph, u.at as u64, u.nb as u64);
        let r = entries(u, used, "morph list");
        if r.is_ok() {
            n.mark(0, u.nb as u64);
        }
        done(t, n, r)?;
    }
    if strips.len() != tables.len() {
        return Err(e(format!("{} strip lists and {} strip tables", strips.len(), tables.len())));
    }
    for (su, tu) in strips.into_iter().zip(tables) {
        let mut n = t.child(region, l.table, tu.at as u64, tu.nb as u64);
        let mut total = 0usize;
        let r = (|| {
            let d = &b[tu.at..tu.at + 4 * tu.num];
            let mut term = None;
            for i in 0..tu.num {
                let (x, y, z) = (i8_(d[4 * i]), i8_(d[4 * i + 1]), i8_(d[4 * i + 2]));
                let o = 4 * i as u64;
                n.mark(o, 1);
                if x == 0 {
                    if y != -1 || z != -1 {
                        return Err(e(format!("strip table terminator {i} has y {y}, z {z}")));
                    }
                    n.mark(o + 1, 2);
                    n.skip(o + 3, 1);
                    term = Some(i);
                    break;
                }
                if x < 0 {
                    total += (x + 128) as usize;
                    if i == 0 && z < 0 {
                        return Err(e("the first strip has no GS register record".to_string()));
                    }
                    if i > 0 {
                        if y != 0 && y != -128 {
                            return Err(e(format!("strip table entry {i}: flush flag {y}")));
                        }
                        n.mark(o + 1, 1);
                    } else {
                        n.skip(o + 1, 1);
                    }
                    if (z >= 0 && (z % 5 != 0 || z as usize / 5 >= c.n28)) || z < -1 {
                        return Err(e(format!("strip table entry {i}: register record {z} of {} records", c.n28)));
                    }
                    n.mark(o + 2, 1);
                    n.skip(o + 3, 1);
                } else {
                    total += x as usize;
                    n.skip(o + 1, 3);
                }
            }
            if term != Some(tu.num - 1) {
                return Err(e(format!("strip table terminator at {term:?} of {} entries", tu.num)));
            }
            Ok(())
        })();
        done(t, n, r)?;
        if su.num != total.div_ceil(4) {
            return Err(e(format!("{total} strip vertices in {} list vectors", su.num)));
        }
        let mut n = t.child(region, l.strips, su.at as u64, su.nb as u64);
        let r = entries(su, total, "strip list");
        if r.is_ok() {
            n.mark(0, su.nb as u64);
        }
        done(t, n, r)?;
    }
    Ok(r0.addr)
}
