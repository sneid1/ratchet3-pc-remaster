//! Tie classes (LEVEL core): the class data the tie class loader func_L01_00424478 relocates and the renderers func_L01_004AFA28
//! (tie culling and queueing), func_L01_004B0990 and func_L01_004B1D60 (tie DMA chain), func_L01_004B3368 (tie light patches)
//! read. `tie` decodes one tie class at `at` in the unpacked core root `parent`. Shrub classes are `shrub.rs`.
//!
//! A tie class: 0x80 header bytes, 8 bounding corners, then for each of the three LODs (counts at +0xC, +0xD, +0xE) a table of
//! 16-byte entries followed by the packets they point to, then the class's light data (+0x34: vertex positions, three variant
//! tables, colour patch lists), the texture registers (+0x1C, 0x50 bytes each) and zero padding to 0x40.
//!
//! # Second-part texture coordinates
//!
//! A packet whose LOD list entry flags (+0xE) have bit 2 or 4 is drawn a second time with coordinates the EE computes every frame
//! and sends to VU1 (UNPACK V2-32, `4 * ff` elements of S and T, one element per vertex in the packet's vertex order, then padding).
//! The port takes them from the chain, where the EE sent them (the UNPACK V2-32 to 0xDC of VU1's record buffer before the
//! second draw: `objects::second_coordinates` in `uya`), not from a run of the routines this section records.
//! func_L01_004B0990 (0x4B0E44 `andi $v0, flags, 6`, 0x4B1128 `bnez $a0` on flags & 2) puts 0x4B13C8 (flag 2) or 0x4B17E8
//! (flag 4) in $a3 (0x4B1118, 0x4B1130), sets $s5 to the second part + 0x30 (0x4B11AC), $a1 to `ff` (entry +0xF, 0x4B1210),
//! $sp to the DMA packet's data and $v1 to the instance record, and calls it with `jalr $a3` at 0x4B1238. Both routines are
//! inside func_L01_004B1280 (rac3-decomp's catalogue: 2704 bytes, 0x4B1280 to 0x4B1D10, at its level's address in all 34 story
//! levels); the instructions of 0x4B13C8 to 0x4B1BC0 are the same words in all 34 levels except the `lqc2` offsets from $gp at
//! 0x4B13F0 and 0x4B1848 (the gp-relative offset differs per level: the constants sit at a per-level gp, so a level other than 1
//! reads them at its own gp + that immediate) and the `lw` at 0x4B1280.
//!
//! Inputs, read each frame (Veldin, level 1, gp = 0x1DC8B0 = 0x1E0F54 - 0x46A4):
//! - the instance's matrix record ($v1): the table at [gp + 0x46B8] (0x1DC8B0 + 0x46B8 = 0x1E0F68 holds 0x526120 at field 2400)
//!   plus 16 x ((list word >> 12) & 0x1FFF) (0x4B0BD8 to 0x4B0C04, stored in the table at 0x70002000 + 16 n by 0x4B0C0C and loaded back as $v1 at 0x4B1200), the record the packet sends to VU1 as 6 quadwords;
//!   the routines load `lqc2` from its +0x60 (vf1 in flag 2, vf4 in flag 4: 0x4B13D0, 0x4B17F0), +0x70 (vf2, 0x4B13D8), +0x80
//!   (vf3, 0x4B13E0) and +0x90 (vf9, 0x4B13E8, flag 2 only). A caller that does not have the list word finds the record by the
//!   first 6 quadwords it sends to VU1;
//! - vf30 of flag 2: the quadword at gp - 0x1DA0 = 0x1DAB10 (`lqc2 $vf30, -0x1DA0($gp)`, 0x4B13F0): (0.25, 0.25, -0.5, 0.5) on
//!   Veldin, of which the routine uses the w;
//! - vf31 of flag 4: the quadword at gp - 0x1D90 = 0x1DAB20 (`lqc2 $vf31, -0x1D90($gp)`, 0x4B1848): (0.48, -0.48, 1598029824.0,
//!   0.5) on Veldin;
//! - the packet's second part +0x30 (`raw`), and `ff`.
//! The routines also set vf28 to 0, vf29 and vf30 (flag 4: 1 and 2 in every lane), vf31 (flag 2: 1, 1, 1, 2; 0x4B13CC, 0x4B1404)
//! and vf29 (flag 2: vf30.w in every lane; 0x4B13FC, 0x4B140C).
//!
//! The VU0 microprogram (straight-line code, entries 0x0, 0x150, 0x2A0, 0x4A0, every lower instruction a NOP or LOI) is the
//! program of the DMA tag at RAM 0x1159B0 (QWC 0x6A, `0x7000006A 0 0 0x4AD40000`: one MPG of 0xD4 pairs to micro address 0; the
//! pairs are at 0x1159C0 to 0x116060, 1696 bytes, in the boot data, the same in all 34 levels). func_L01_004C1AF0 sends
//! the tag to VIF0 (DMA channel 0, CHCR 0x145 at 0x4C1B68) and func_L01_004AF6D0 calls it with `lui $a0, 0x11; addiu $a0, $a0,
//! 0x59B0` at 0x4AF720, before func_L01_004B0990 (the same call is in every level's frame function; 0x4AF724 on Veldin) and
//! sends the shrub VU0 program (tag 0x100AE0) again after func_L01_004B1D60 (0x4AF73C). Its pairs, 0x0 to
//! 0x69F:
//! - 0x0 to 0x148 (flag 2, vcallms 0x0, quadwords of vf5 to vf8; E bit at 0x140): ITOF12 vf5 to vf8 (0x0 to 0x18), vf5 to vf8 =
//!   vf1 x + vf2 y + vf3 z of each (MULAx, MADDAy, MADDz, 0x20 to 0x78: the normal turned by the record's rows), vf11 to vf14 =
//!   vf9 x vf5 to vf8 (0x80 to 0x98), their z = x + y + z (ADDAy.z, MADDx.z with vf31 z = 1, 0xA0 to 0xD8), vf5 to vf8 x= that z
//!   (0xE0 to 0xF8), vf5 to vf8 = vf9 - 2 vf31.w x (the same) (ADDA ACC = vf0 + vf9 and MSUBw, 0x100 to 0x120: the view vector
//!   reflected about the normal), then vf5 to vf8 = vf29.x + I x vf5 to vf8 (ADDAx at 0x128 with LOI 0.3, MADDi 0x130 to
//!   0x148): S, T = 0.5 + 0.3 x the reflection's x and y;
//! - 0x150 to 0x298 (the same for vf15 to vf18, vcallms 0x150; E bit at 0x290);
//! - 0x2A0 to 0x498 (flag 4, vcallms 0x2A0, inputs vf4 to vf9, results in vf6 to vf9, x and y; E bit at 0x490): LOI 1.5 and ITOF4
//!   vf4 to vf6 (0x2A0 to 0x2B0), minus vf1 to vf3 (SUB, 0x2B8 to 0x2C8), the squared length in vf10 (MULA, MADDA, MADD, 0x2D0
//!   to 0x2E0), the reciprocal square root of it by the magic number in vf31 z (ITOF0 of the float's bits 0x2F0, MULw 0x2F8,
//!   MSUBw 0x310, FTOI0 0x330, one Newton step MUL 0x338, 0x340, MULAi 0x348, MSUB 0x350), ITOF12 vf7 to vf9 (0x358 to 0x368),
//!   vf4 to vf6 x= it (0x370 to 0x380), the dot of that with vf7 to vf9 (0x388 to 0x398), vf7 to vf9 reflected about the
//!   direction (x= it 0x3A0 to 0x3B0, ADDA, MSUBx with vf30 x = 2, 0x3B8 to 0x3E0), 2 vf9 + vf30 (0x3E8, 0x3F0), a second magic
//!   number reciprocal (0x3F8 to 0x440), and the products into x and y of vf6 to vf9 with vf31 x, y (0x448 to 0x498);
//! - 0x4A0 to 0x698 (the same for vf14 to vf19, vcallms 0x4A0, results in vf16 to vf19; E bit at 0x690).
//! Flag 2 groups: the quadwords of the normals alternate vcallms 0x150 (quadword 0, 2, ...) and 0x0 (1, 3, ...); each quadword's
//! four results are the x and y of vf15 to vf18 or vf5 to vf8, stored `sd` at $sp in vertex order (0x4B15E8 to 0x4B1610). Flag 4
//! groups: 32 bytes per four vertices, vcallms 0x4A0 for groups 0, 2, ... and 0x2A0 for 1, 3, ... (0x4B1930, 0x4B1A08); the results
//! are vf16 to vf19 or vf6 to vf9 stored at 0x4B1A20 to 0x4B1A48 and 0x4B1AF8 to 0x4B1B20. In the chain the coordinates come before
//! the second draw: entry 0x90 takes them as floats, a second pass set up at entry 0x70 as integers (ITOF12). Level 20's
//! mission start sends 173,262 packets' coordinates in 75 s of play, equal bit for bit to what a VU0 interpreter of the routines
//! gave (that interpreter, run every frame inside the renderer, is gone).

use crate::disc::{u16_at, u32_at};
use crate::track::{Node, Space, Tracker, NOTHING};

const GT: &str = "LEVEL core tie class (unpacked)";

/// One texture register of a tie class (the 0x50-byte records at the class's +0x1C): the GS registers the packets send in their
/// A+D header. `index` is the register's class-local index j.
///
/// The tie texture record of register j is byte j of the 16 texture index bytes at +0x10 of the class's record in the core index
/// (words 8/9, 32 bytes per class): func_L01_00424478 copies those 16 bytes (0x424624 to 0x424634) and, for register j (the loop at
/// 0x424658), reads the byte `t` (0x424660), takes the 16-byte record `table2[t]` of the core index's tie texture table (words
/// 16/17; no base is added: `s6 + (t << 4)` at 0x42467C, `lhu 4`, `lh 6` to `lh 0xE` fields at 0x424684 to 0x4246B8) and overwrites
/// the register's quadwords: +0x00 TEX0_1 (built from the record), +0x10 TEX1_1 = (levels - 1) << 2 | 0x20 | `mmin` << 6 |
/// `k_word` << 32 (0x424748 to 0x424784; levels = the record's u16 at +8), +0x20 MIPTBP1_1 (from the record), +0x30 CLAMP_1 =
/// `wms` | `wmt` << 2 | t << 24 (0x424754, 0x42477C). The file holds the inputs below; ALPHA_1 is the file's data unchanged.
#[derive(Clone, Debug)]
pub struct TieTexture {
    pub index: usize,
    /// File +0x10: the TEX1_1 high word the game sends (K in bits 0 to 11 of it, signed, 4 fraction bits).
    pub k_word: u32,
    /// TEX1_1.K in mip levels: the signed 12-bit value of `k_word` / 16.
    pub k: f32,
    /// File +0x14: TEX1_1.MMIN (bits 6 to 8 of the doubleword the game sends).
    pub mmin: u32,
    /// File +0x30 / +0x34: CLAMP_1.WMS (bits 0, 1) and WMT (bits 2, 3): 0 repeat, 1 clamp, 2 region clamp, 3 region repeat.
    pub wms: u32,
    pub wmt: u32,
    /// ALPHA_1 data (file +0x40; register 0x42), sent as is: A, B, C, D (bits 0 to 7, two bits each) and FIX (bits 32 to 39).
    pub alpha: u64,
    pub alpha_a: u8,
    pub alpha_b: u8,
    pub alpha_c: u8,
    pub alpha_d: u8,
    pub alpha_fix: u8,
}

/// One vertex of a packet, as VU1 program 2 reads it.
#[derive(Clone, Debug)]
pub struct TieVertex {
    /// Class-local position: the s16 triple converted by ITOF0 (micro 0x2C0, 0x3B0 and the loops at 0xA08 to 0x1D98), which the
    /// instance matrix (the +0x34 table of the gameplay block, VU1 quads 0 to 3 of the instance record) multiplies. For a vertex with
    /// `sway`, the rest position (the second quadword of its 24 bytes).
    pub pos: [f32; 3],
    /// The vertices of the packet's second group (VU1 loops from micro 0x1A60; tab vector 0 y = 0, tab vector 2 w of them): the first
    /// quadword of their 24 bytes, converted by ITOF0. Program 2 computes the position that the matrix multiplies as
    /// `sway * k + pos` (MULx.xyz by vf27x then ADD.xyz of the second quadword, micro 0x1A80 and 0x1AA0), k being the x of the
    /// instance record's quad 4 (LQ vf27, 4(vi03) at micro 0xA68).
    pub sway: Option<[f32; 3]>,
    /// The texture coordinates the GS gets: s / q and t / q of the vertex's third s16 triple (ITOF12 at micro 0x2C8, 0x7B8; program 2
    /// multiplies by the reciprocal of w and the GS divides by the stored q, so what is drawn is s / q, t / q): 1.0 is the texture's
    /// size.
    pub uv: [f32; 2],
    /// The index of this vertex's colour in the packet's colours: program 2 reads the instance's colour quadwords in vertex order
    /// (LQI vf11 to vf14, (vi03++) at micro 0xAD8 and in every loop), one per vertex, as RGBA bytes with 0x80 = 1.0.
    pub colour: u32,
}

/// A triangle strip of a packet: the vertices in the order the GS gets them (GS TRISTRIP: triangle i is vertices i, i + 1, i + 2).
#[derive(Clone, Debug)]
pub struct TieStrip {
    /// Which of the packet's textures (index into `TiePacket::textures`) the strip is drawn with: the texture group of the
    /// output buffer the strip's GIF tag (tab vector 3 on, z = its quadword in the buffer) lies in.
    pub texture: usize,
    /// Tab vector w of the strip (0 or 0x20 in every packet of the disc): program 2's strip pass (micro 0x4E8) copies y, z, w of each
    /// vector to VU1 address 0x1F on, and entry 0x10's pass (micro 0x1E58 to 0x1FE0) sets the ADC bit on the strip's vertices from
    /// the third on, which draws no triangle of the strip. In the traced frame that happened to 317 of 327 strips with side 0 and
    /// 4 of 461 with side 0x20 on entry 0x10, and never on entry 0x0; the test depends on the view, so it is not class data.
    pub side: u8,
    /// Indexes into `TiePacket::vertices`.
    pub vertices: Vec<u32>,
}

/// A triangle as the GS draws it from program 2's output.
#[derive(Clone, Debug)]
pub struct TieTriangle {
    /// Indexes into `TiePacket::vertices`, in GS order (the winding of odd triangles of a strip is the opposite of the even ones').
    pub v: [u32; 3],
    /// Index into `TiePacket::textures`.
    pub texture: usize,
    /// Index into `TiePacket::strips`.
    pub strip: usize,
}

/// The second part of a packet (entry flags 2, 4, 10 or 16, `h` != 0): the packet is drawn a second time, VU1 program 2 entry 0x60,
/// 0x70 / 0x80, 0x90 (micro 0x2048 to 0x23B0): the colour quadword at +0x10 replaces every vertex colour, the first `uv_counts.0`
/// vertices (2-quadword ones) and then `uv_counts.1` vertices (3-quadword ones) get the texture coordinates the EE computes from
/// the packed normals (func_L01_004B13C8 for flag 2, func_L01_004B17E8 for flag 4: 4 normals per quadword, vcallms 0x150 and 0x0 on
/// VU0), and the packet's second texture list is used.
#[derive(Clone, Debug)]
pub struct TieSecond {
    /// Class-local register index per second-part texture (the second list of `a` offsets).
    pub registers: Vec<usize>,
    /// The quadword at +0x10 as RGBA (each word of it, 0x80 = 1.0), only when flag 2 or 4 is set.
    pub colour: Option<[u8; 4]>,
    /// Words x and y of the quadword at +0x20 (ILW.x / ILW.y 2(vi00) at micro 0x2060, 0x2068): the number of 2-quadword and of
    /// 3-quadword vertices that get new coordinates; equal to the packet's counts of the two vertex kinds.
    pub uv_counts: Option<(u32, u32)>,
    /// Flag 2 only: the packed normals at +0x30, `4 * ff` entries (the last ones may pad), per word x = (w << 21) >> 19,
    /// y = (w << 10) >> 19, z = (w >> 22) << 3 as the EE code does (0x4B1420 to 0x4B1448). Empty for flag 4, whose data is not
    /// packed normals.
    pub normals: Vec<[i32; 3]>,
    /// The bytes at +0x30 as the EE routine reads them: 16 x ff for flag 2 (ff quadwords of four packed normals), 32 x ff for
    /// flag 4 (ff groups of two quadwords: four vertices' x, y, z signed bytes in words 0 to 2 of the first, the same of the
    /// second; word 3 of each unused). Empty without flag 2 or 4. The EE's routines read these.
    pub raw: Vec<u8>,
}

/// One packet (one 16-byte LOD list entry and the data it points to).
#[derive(Clone, Debug)]
pub struct TiePacket {
    /// Class-relative offset of the packet's header quadword.
    pub offset: u32,
    /// Entry +0xE: bit 2 and 4 second part with normals, 8 (VU1 constants at 0x3E80 / 0x3EA0 sent by func_L01_004B0990 at 0x4B0D18
    /// to 0x4B0D54), 16 second part without data.
    pub flags: u8,
    /// Class-local register index (`TieClass::textures`) of each of the packet's textures (offsets at +0x10 divided by 0x50).
    pub textures: Vec<usize>,
    pub vertices: Vec<TieVertex>,
    pub strips: Vec<TieStrip>,
    pub triangles: Vec<TieTriangle>,
    /// Entry +0xA: the number of colours (RGBA, 4 bytes each) the packet takes from the instance's colour buffer
    /// (func_L01_004B0990 at 0x4B0DB8 to 0x4B0DF0: UNPACK V4-8 of (n + 3) / 4 quadwords, n quadwords out); equal to the vertex count.
    pub colour_count: u32,
    /// The byte offset of the packet's colours from the start of the instance's colour buffer ([0x1E0F64] + 16 x the u16 at the
    /// instance record's +0x14, func_L01_004B0990 at 0x4B0BBC to 0x4B0C0C): the instance's pointer starts there for each LOD and is
    /// advanced by (colour_count + 3) / 4 x 16 after each entry of the LOD list (0x4B0DF4, 0x4B0E10).
    pub colour_offset: u32,
    /// Entry +5 (func_L01_004B0990 at 0x4B0C3C: compared with the instance's byte +8 of its scratchpad record to set a VIF code
    /// byte at 0x4B0E08): not interpreted.
    pub entry_byte5: u8,
    pub second: Option<TieSecond>,
}

/// A decoded tie class.
#[derive(Clone, Debug)]
pub struct TieClass {
    /// Header +0x40 (f32): func_L01_004252F0 copies it to row 3, w (+0x3C) of its copy of the instance matrix (0x425C1C, 0x425C20)
    /// and scales the instance's culling centre by it (func_40BEC0 at 0x425C88); VU1 program 2 does not read it.
    pub scale: f32,
    /// Header +0x50 (quad, xyz) and +0x5C (f32): the class's culling centre and radius (func_L01_004AF7D0).
    pub centre: [f32; 3],
    pub radius: f32,
    /// The 8 bounding corners (+0x80 to +0xFF, 4 floats each) func_L01_004AFA28 tests.
    pub bounds: [[f32; 4]; 8],
    /// The class's texture registers, in the class's order (+0x1C, 0x50 bytes each).
    pub textures: Vec<TieTexture>,
    /// The three LOD lists (counts +0xC, +0xD, +0xE), each in packet order.
    pub lods: [Vec<TiePacket>; 3],
    /// Header +0x10, +0x14, +0x18 (f32, lqc2 vf16 at 0x4AFCB4): the view depths func_L01_004AFA28 picks the LOD by. At a depth d
    /// (the instance's culling centre, at least 0): LOD 0 up to the first, LOD 0 morphing (the morph vertices moved by
    /// (d - first) / (second - first)) up to the second, LOD 1 morphing by (d - second) / (third - second) up to the third, LOD 2
    /// beyond it.
    pub lod_distances: [f32; 3],
    /// Header +0x3C (u16): func_L01_004AFA28 skips a class with bit 0 or 3 set (0x4AFCB0).
    pub flags: u16,
    /// The light positions (+0x34, +0x38 of them, 8 bytes each): the class-local x, y, z (s16, the vertices' units) and the
    /// normal's two byte indices into the (cos, sin) table (u16: a low, b high; the normal is (cos a cos b, sin a cos b, sin b)),
    /// which the tie light pass func_L01_004B3368 lights each instance's colours at (`ld` and `psraw 16` of the position at
    /// 0x4B3C90, the indices by `lhu +6`, `pextlb`, `pextlh` and `psllw 3` at 0x4B3890 to 0x4B38E0).
    pub light_positions: Vec<([i16; 3], u16)>,
    /// Per LOD (the light tables at +0x2A, +0x2C, +0x2E from +0x34, which func_L01_004B3368 picks by the instance's +0x1F, 0x4B3E9C):
    /// each colour word of the instance's colour buffer (a vertex's is its packet's `colour_offset` / 4 plus its `colour`) as the
    /// pass makes it from the lit light positions: (light position, weight in quarters) of each source. A block's lists (DMA'd to
    /// scratchpad 0x3000, run at 0x4B3F58 to 0x4B43BC into its output at scratchpad 0x2800, the blocks' outputs one after the
    /// other) write list 1 a copy of one source, 2 the mean of two, 3 (a + 3 b) / 4 (a the source in bits 23 to 31, b in 14 to
    /// 22), 4 (2 a + b + c) / 4, 5 the mean of four, 6 zero (no source); a later list's write to a word wins. Empty: a word no list
    /// writes or a zero.
    pub light_slots: [Vec<Vec<(u16, u8)>>; 3],
}

fn f32_at(b: &[u8], at: usize) -> Result<f32, String> {
    Ok(f32::from_bits(u32_at(b, at)?))
}

fn u64_at(b: &[u8], at: usize) -> Result<u64, String> {
    Ok(u32_at(b, at)? as u64 | (u32_at(b, at + 4)? as u64) << 32)
}

fn packed_normal(w: u32) -> [i32; 3] {
    [((w << 21) as i32) >> 19, ((w << 10) as i32) >> 19, (w as i32 >> 22) << 3]
}

/// The pieces of one structure kind in a class and the bytes read in them, all by class-relative offset.
#[derive(Default)]
pub(crate) struct Acc {
    r: Vec<(u64, u64)>,
    m: Vec<(u64, u64)>,
    sk: Vec<(u64, u64)>,
    err: Option<String>,
}

impl Acc {
    pub(crate) fn add(&mut self, at: u64, len: u64) {
        if len == 0 {
            return;
        }
        if let Some(l) = self.r.last_mut() {
            if l.0 + l.1 == at {
                l.1 += len;
                return;
            }
        }
        self.r.push((at, len));
    }
    pub(crate) fn mark(&mut self, at: u64, len: u64) {
        if len == 0 {
            return;
        }
        self.m.push((at, len));
    }
    /// Bytes nothing reads.
    pub(crate) fn skip(&mut self, at: u64, len: u64) {
        if len == 0 {
            return;
        }
        self.sk.push((at, len));
    }
    pub(crate) fn fail(&mut self, e: String) {
        if self.err.is_none() {
            self.err = Some(e);
        }
    }
    /// Bytes that must all be zero (padding, or values the game's code overwrites).
    pub(crate) fn zero(&mut self, b: &[u8], at: u64, len: u64, what: &str) {
        let Some(s) = b.get(at as usize..(at + len) as usize) else {
            self.fail(format!("+{at:#x}+{len:#x} {what}: past the class's {:#x} bytes", b.len()));
            return;
        };
        match s.iter().position(|&v| v != 0) {
            None => self.mark(at, len),
            Some(k) => self.fail(format!("+{:#x} {what}: byte {:#04x}, expected 0", at as usize + k, s[k])),
        }
    }
    /// A u32 that must equal `want`.
    pub(crate) fn eq32(&mut self, b: &[u8], at: u64, want: u32, what: &str) -> Result<(), String> {
        let v = u32_at(b, at as usize)?;
        if v == want {
            self.mark(at, 4);
        } else {
            self.fail(format!("+{at:#x} {what}: {v:#x}, expected {want:#x}"));
        }
        Ok(())
    }
}

/// Creates the node of `line` over the accumulated pieces and applies the marks.
pub(crate) fn flush(t: &mut Tracker, parent: &mut Node, line: usize, base: u64, a: Acc) {
    if a.r.is_empty() {
        return;
    }
    let abs: Vec<(u64, u64)> = a.r.iter().map(|&(s, l)| (base + s, l)).collect();
    let mut n = t.pieces(parent, line, &abs);
    for &(at, len) in &a.m {
        let mut cum = 0;
        for &(s, l) in &a.r {
            if at >= s && at + len <= s + l {
                n.mark(cum + (at - s), len);
                break;
            }
            cum += l;
        }
    }
    for &(at, len) in &a.sk {
        let mut cum = 0;
        for &(s, l) in &a.r {
            if at >= s && at + len <= s + l {
                n.skip(cum + (at - s), len);
                break;
            }
            cum += l;
        }
    }
    if let Some(e) = a.err {
        n.fail(e);
    }
    t.close(n);
}

pub(crate) fn chk(b: &[u8], at: u64, len: u64, what: &str) -> Result<(), String> {
    if at + len > b.len() as u64 {
        return Err(format!("{what}: {at:#x}+{len:#x} past the class's {:#x} bytes", b.len()));
    }
    Ok(())
}

pub fn tie(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<TieClass, String> {
    let size = b.len() as u64;
    chk(b, 0, 0x100, "tie class header")?;
    let hdr = t.line(GT, "tie class header (0x80 bytes)", "tie::tie", "func_L01_00424478 (+0 to +0x1F, +0x34, +0x46, +0x48; overwrites +0x20, +0x3E), func_L01_004AFA28 (+0x10 to +0x18, +0x24, +0x3C), func_L01_004B3368 (+0x2A, +0x38, +0x3A, +0x4C: lw at 0x4B3628 when the +0x3C flag 0x10 is set, else zero), func_L01_004252F0 (+0x40, +0x46), func_L01_004B4480 (+0x78), func_L01_004AF7D0 (+0x50 to +0x5F: func_L01_0040C2D0 transforms the quadword at +0x50 with lqc2, lwc1 +0x5C at 0x4AF8CC); +0x44 and +0x60 to +0x77: no reader found (func_L01_004AF7D0, func_L01_004AFA28, func_L01_004B0990, func_L01_004B1D60, func_L01_004B3368, func_L01_004B4480, func_L01_004252F0 and func_L01_00424478 read nothing there)", Space::Unpacked);
    let bbox = t.line(GT, "tie class bounding box (8 corners of 4 floats)", "tie::tie", "func_L01_004AFA28 (the visibility test reads +0x80 to +0xFF)", Space::Unpacked);
    let ent = t.line(GT, "tie LOD list entries (16 bytes)", "tie::tie", "func_L01_00424478 (relocates the pointers), func_L01_004B0990 and func_L01_004B1D60 (build the DMA chain from the fields)", Space::Unpacked);
    let ph = t.line(GT, "tie packet header quadwords", "tie::tie", "func_L01_004B0990 (UNPACK V4-32 x1 to VU1 address 0, one quadword); VU1 program 2 (word 3: ILW.w vi03, 0(vi00) at micro 0x4E8, the strip table offset; words 0 to 2: LQI vf07 at micro 0xD8 then MTIR vi04, vf07x at 0x188, rotated by MR32.xyz at 0x1C0: the vertex count at which each further texture of the packet starts) and program 3 (ILW.w at 0x1E18 and 0x2700, MTIR vi04, vf07x at 0x220)", Space::Unpacked);
    let pt = t.line(GT, "tie packet texture offsets", "tie::tie", "func_L01_004B0990 (offsets into the class's texture registers, one 5-quadword upload each)", Space::Unpacked);
    let ptab = t.line(GT, "tie packet strip tables (V4-8)", "tie::tie", "func_L01_004B0990 (UNPACK V4-8 from the packet to VU1 address 6, 0xB or 0x10); VU1 program 2 (the tab vectors 0, 1, 2 are loaded as vf02, vf03, vf04: MTIR vi05, vf02x at micro 0x488, vi14 from vf02y at 0x490, vi09 from vf02w at 0x1C8 and ILW.w at 0x518 (the strip count); vi04, vi06, vi07, vi08 from vf03 x, y, z, w at 0x498, 0x4A0, 0x4A8, 0x4B0; vi06 from vf04x at 0x2A0 and vi07 from vf04y at 0x2E8), program 3 reads the same fields at 0x550 to 0x578, 0x338, 0x340; vectors 3 on: x at 0x1D8 and 0x240, z at 0x1E0 and 0x248, w at 0x110, 0x1DD0, 0x1EE8 and in the strip pass at 0x4E8, which a packet with word 3 of its header zero skips, so its vector w are skipped (the strip pass at 0x4E8 copies y, z, w with LQI.yzw and SQI.yzw, and ILW.x, ILW.z read x and z); vector 0 z, vector 2 z and w and the y of vectors 3 on: no instruction consumes them in any of the 2817 vectors of the 338 packets VU1 ran in the traced Veldin frame, and changing them changes no output packet in the mutation runs", Space::Unpacked);
    let pv = t.line(GT, "tie packet vertices (16 bytes)", "tie::tie", "func_L01_004B0990 (UNPACK V4-16), VU1 program 2 (position, slot, ST: ITOF0 at micro 0x2C0, ITOF12 for ST, MTIR for the slot)", Space::Unpacked);
    let ps = t.line(GT, "tie packet strip vectors (V4-8)", "tie::tie", "func_L01_004B1D60 (UNPACK V4-8 from the packet to VU1 address 0x1E, 4 vectors per quadword, program 3 chain only: the program 2 chain of func_L01_004B0990 builds this list itself in the strip pass at micro 0x4E8); VU1 program 3 (LQ vf13, 30(vi00) at micro 0x2130, MTIR vi02, vf13x at 0x2158 and the strip loop): every component of every vector up to the last nonzero one is read (the 3 packets VU1 ran), the vectors after it are zero padding", Space::Unpacked);
    let p2t = t.line(GT, "tie packet second-part texture offsets", "tie::tie", "func_L01_004B0990 (offsets into the class's texture registers)", Space::Unpacked);
    let p2 = t.line(GT, "tie packet second-part data", "tie::tie", "when the entry's flags have bit 2 or 4: func_L01_004B0990 refs +0x10 and +0x20 (UNPACK V4-32 to VU1 address 1; VU1 program 2 LQ vf02, 1(vi00) at micro 0x78 and 0x2310, ILW.x/ILW.y 2(vi00) at micro 0x2060, 0x2068, 0x2308, 0x2320: z and w of +0x20 are zero) and reads +0x30 on the EE (func_L01_004B13C8 for bit 2, func_L01_004B17E8 for bit 4, which also reads the second set of quadwords); a part whose flags have neither bit 2 nor bit 4 (flags 16): func_L01_004B0990 (0x4B0E44 to 0x4B0E9C) and func_L01_004B1D60 (0x4B2210 to 0x4B2284) read only its texture offsets, nothing reads the rest", Space::Unpacked);
    let lpos = t.line(GT, "tie class light positions (8 bytes: x, y, z s16, w)", "tie::tie", "func_L01_004B3368 (DMA to the scratchpad from +0x34, +0x38 entries; x, y, z: ld, psraw 16 and vitof0 at 0x4B3C90, 0x4B3D00, 0x4B3D88; w: lhu +6 of four entries at 0x4B3890 and the two byte indexes into the 256-entry table at scratchpad 0x3800 at 0x4B3C98 (pextlb, psllw 3, ld))", Space::Unpacked);
    let lhdr = t.line(GT, "tie class light patch table headers (32 bytes)", "tie::tie", "func_L01_004B3368 (lbu of the block quadword counts until a zero byte)", Space::Unpacked);
    let lblk = t.line(GT, "tie class light patch blocks", "tie::tie", "func_L01_004B3368 (the block is DMA'd to scratchpad 0x3000 and its header read by lq at 0x4B3F38: six list byte lengths, the output quadword count at +0xC; lists 0x4B3F58 to 0x4B43BC); the halfword at +0xE: no reader found", Space::Unpacked);
    let plst = t.line(GT, "tie class colour patch lists", "tie::tie", "func_L01_004B4480 (count, then halfword offsets; the lists start at the class's +0x78 offsets)", Space::Unpacked);
    let tex = t.line(GT, "tie class texture registers (0x50 bytes)", "tie::tie", "func_L01_00424478 (reads +0x10, +0x14, +0x30, +0x34; overwrites +0x0, +0x10, +0x20, +0x30 with TEX0_1, TEX1_1, MIPTBP1_1, CLAMP_1; the GS reads the register numbers and ALPHA_1)", Space::Unpacked);
    let pad = t.line(GT, "tie class padding", "tie::tie", NOTHING, Space::Unpacked);

    let (mut a_h, mut a_b, mut a_e, mut a_ph, mut a_pt, mut a_tab, mut a_v, mut a_s, mut a_2t, mut a_2) = (Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default());
    let (mut a_lp, mut a_lh, mut a_lb, mut a_pl, mut a_tx, mut a_pd) = (Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default());

    // Header.
    a_h.add(0, 0x80);
    a_h.mark(0, 0x20);
    a_h.zero(b, 0x20, 4, "overwritten by func_L01_00424478");
    a_h.mark(0x24, 6);
    a_h.mark(0x2a, 6);
    a_h.zero(b, 0x30, 4, "written by func_L01_004252F0");
    a_h.mark(0x34, 0xa);
    a_h.mark(0x3c, 2);
    a_h.zero(b, 0x3e, 2, "overwritten by func_L01_00424478");
    a_h.mark(0x40, 4);
    a_h.zero(b, 0x46, 2, "overwritten by func_L01_00424478");
    a_h.mark(0x48, 4);
    if u16_at(b, 0x3c)? & 0x10 != 0 {
        a_h.mark(0x4c, 4);
    } else {
        a_h.zero(b, 0x4c, 4, "+0x4C is read only when +0x3C has flag 0x10");
    }
    a_h.mark(0x50, 0x10);
    a_h.mark(0x78, 8);
    a_h.skip(0x44, 2);
    a_h.skip(0x60, 0x18);
    a_b.add(0x80, 0x80);
    a_b.mark(0x80, 0x80);

    let lists = [u32_at(b, 0)? as u64, u32_at(b, 4)? as u64, u32_at(b, 8)? as u64];
    let cnt = [b[0xc] as u64, b[0xd] as u64, b[0xe] as u64];
    let ntex = b[0xf] as u64;
    let tex_at = u32_at(b, 0x1c)? as u64;
    let p34 = u32_at(b, 0x34)? as u64;
    let n38 = u16_at(b, 0x38)? as u64;

    // The LODs: entry tables and packets, in order.
    let mut pos = 0x100u64;
    let mut lods: [Vec<TiePacket>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    for l in 0..3 {
        if cnt[l] == 0 {
            if lists[l] != 0 {
                return Err(format!("LOD {l}: list at {:#x} with a zero count", lists[l]));
            }
            continue;
        }
        if lists[l] != pos {
            return Err(format!("LOD {l}: list at {:#x}, expected {pos:#x}", lists[l]));
        }
        let mut col_off = 0u32;
        chk(b, pos, 16 * cnt[l], "tie LOD list")?;
        a_e.add(pos, 16 * cnt[l]);
        a_e.mark(pos, 16 * cnt[l]);
        let mut at_e = pos;
        pos += 16 * cnt[l];
        for _ in 0..cnt[l] {
            let e = at_e as usize;
            at_e += 16;
            let rel = u32_at(b, e)? as u64;
            let p = lists[l] + rel;
            if p != pos {
                return Err(format!("LOD {l}: packet at {p:#x}, expected {pos:#x}"));
            }
            let (a, c, d, ee, f) = (b[e + 4] as u64, b[e + 6] as u64, b[e + 7] as u64, b[e + 8] as u64, b[e + 9] as u64);
            let (h, cc, dd, fl, ff) = (b[e + 0xb] as u64, b[e + 0xc] as u64, b[e + 0xd] as u64, b[e + 0xe], b[e + 0xf] as u64);
            if !(1..=4).contains(&a) || c < 4 || d != (c + 3) / 4 || ee != d + 2 || cc != ee + f || (h != 0 && h != cc + dd) {
                a_e.fail(format!("entry at {e:#x}: fields {:02x?} do not fit a packet", &b[e..e + 16]));
                return Err(format!("entry at {e:#x} does not fit"));
            }
            let mut len = 16 * (cc + dd);
            if h != 0 {
                len += 16 * (3 + ff);
                if fl & 4 != 0 {
                    len += 16 * ff;
                }
            } else if ff != 0 {
                a_e.fail(format!("entry at {e:#x}: second-part count {ff} without a second part"));
                return Err(format!("entry at {e:#x} does not fit"));
            }
            chk(b, p, len, "tie packet")?;
            // Quadword 0 and the texture offsets.
            a_ph.add(p, 16);
            a_ph.mark(p, 16);
            let w = u32_at(b, (p + 12) as usize)? as i32;
            if w != 0 && w != -((4 + 5 * a) as i32) {
                a_ph.fail(format!("packet at {p:#x}: word 3 is {w}, expected 0 or -{}", 4 + 5 * a));
            }
            // Words 0 to 2: increasing positions for the first a - 1 textures after the first, zero after.
            let mut prev = 0;
            for i in 0..3u64 {
                let v = u32_at(b, (p + 4 * i) as usize)? as u64;
                let bad = if i + 1 < a { v <= prev } else { v != 0 };
                if bad {
                    a_ph.fail(format!("packet at {p:#x}: word {i} is {v}, texture count {a}"));
                }
                prev = v;
            }
            a_pt.add(p + 0x10, 16);
            a_pt.mark(p + 0x10, 4 * a);
            for i in 0..a {
                let to = u32_at(b, (p + 0x10 + 4 * i) as usize)? as u64;
                if to % 0x50 != 0 || to / 0x50 >= ntex {
                    a_pt.fail(format!("packet at {p:#x}: texture offset {to:#x} outside the class's {ntex} registers"));
                }
            }
            a_pt.zero(b, p + 0x10 + 4 * a, 16 - 4 * a, "texture offset padding");
            // The V4-8 strip table: c vectors of 4 bytes, padded to d quadwords.
            a_tab.add(p + 0x20, 16 * d);
            if b[(p + 0x20 + 3) as usize] as u64 != c - 3 {
                a_tab.fail(format!("packet at {p:#x}: strip count {} but {} vectors", b[(p + 0x23) as usize], c));
            }
            // Vector 0: x, y, w read (z not); vector 1: all four; vector 2: x, y (z, w not); vectors 3 on: x, z, w (y not).
            a_tab.mark(p + 0x20, 2);
            a_tab.mark(p + 0x23, 5);
            a_tab.mark(p + 0x28, 2);
            a_tab.skip(p + 0x22, 1);
            a_tab.skip(p + 0x2a, 2);
            for j in 3..c {
                a_tab.mark(p + 0x20 + 4 * j, 1);
                a_tab.skip(p + 0x20 + 4 * j + 1, 1);
                a_tab.mark(p + 0x20 + 4 * j + 2, 1);
                if w != 0 {
                    a_tab.mark(p + 0x20 + 4 * j + 3, 1);
                } else {
                    a_tab.skip(p + 0x20 + 4 * j + 3, 1);
                }
            }
            a_tab.zero(b, p + 0x20 + 4 * c, 16 * d - 4 * c, "strip table padding");
            // Vertices.
            a_v.add(p + 16 * ee, 16 * f);
            a_v.mark(p + 16 * ee, 16 * f);
            // Strip vectors: the used vectors (x, y, z, w each), then zero padding.
            a_s.add(p + 16 * cc, 16 * dd);
            let sv = &b[(p + 16 * cc) as usize..(p + 16 * (cc + dd)) as usize];
            let used = sv.iter().rposition(|&x| x != 0).map_or(0, |k| (k as u64 + 4) / 4);
            a_s.mark(p + 16 * cc, 4 * used);
            a_s.zero(b, p + 16 * cc + 4 * used, 16 * dd - 4 * used, "strip vector padding");
            // Second part.
            if (h == 0 && fl != 0 && fl != 8) || (h != 0 && ![2, 4, 10, 16].contains(&fl)) {
                a_e.fail(format!("entry at {e:#x}: flags {fl:#x} with second-part offset {h}"));
                return Err(format!("entry at {e:#x} does not fit"));
            }
            if h != 0 {
                let q = p + 16 * h;
                a_2t.add(q, 16);
                a_2t.mark(q, 4 * a);
                a_2t.zero(b, q + 4 * a, 16 - 4 * a, "texture offset padding");
                for i in 0..a {
                    let to = u32_at(b, (q + 4 * i) as usize)? as u64;
                    if to % 0x50 != 0 || to / 0x50 >= ntex {
                        a_2t.fail(format!("packet at {p:#x}: second-part texture offset {to:#x} outside the class's {ntex} registers"));
                    }
                }
                a_2.add(q + 0x10, len - 16 * h - 16);
                if fl & 6 == 0 {
                    a_2.skip(q + 0x10, len - 16 * h - 16);
                } else {
                    a_2.mark(q + 0x10, 16);
                    a_2.mark(q + 0x20, 8);
                    a_2.zero(b, q + 0x28, 8, "second-part quadword +0x20, words 2 and 3");
                    a_2.mark(q + 0x30, 16 * ff * if fl & 4 != 0 { 2 } else { 1 });
                }
            }
            // The packet's content.
            let mut textures = Vec::new();
            for i in 0..a {
                textures.push((u32_at(b, (p + 0x10 + 4 * i) as usize)? / 0x50) as usize);
            }
            let tab = |j: u64| -> [u8; 4] { let o = (p + 0x20 + 4 * j) as usize; [b[o], b[o + 1], b[o + 2], b[o + 3]] };
            let (t0, t2) = (tab(0), tab(2));
            let (zab, zc) = (t2[2] as u64, t2[3] as u64);
            let vbytes = 16 * zab + 24 * zc;
            if vbytes > 16 * f || 16 * f - vbytes >= 16 {
                return Err(format!("packet at {p:#x}: {zab} two-quadword and {zc} three-quadword vertices in {f} vertex quadwords"));
            }
            if b[e + 0xa] as u64 != zab + zc {
                return Err(format!("packet at {p:#x}: entry byte +0xA is {}, the packet has {} vertices", b[e + 0xa], zab + zc));
            }
            if t0[1] == 1 && zc != 0 {
                return Err(format!("packet at {p:#x}: tab vector 0 y is {} with {zc} three-quadword vertices", t0[1]));
            }
            let mut vertices = Vec::new();
            let vq = (p + 16 * ee) as usize;
            let s16 = |o: usize| -> f32 { i16::from_le_bytes([b[o], b[o + 1]]) as f32 };
            let slot16 = |o: usize| -> u64 { i16::from_le_bytes([b[o], b[o + 1]]) as u16 as u64 };
            let uv_of = |o: usize| -> Result<[f32; 2], String> {
                let q = s16(o + 4);
                if q == 0.0 {
                    return Err(format!("packet at {p:#x}: texture coordinate q is 0 at {o:#x}"));
                }
                Ok([s16(o) / q, s16(o + 2) / q])
            };
            let mut slots: Vec<(u64, u32)> = Vec::new();
            for i in 0..zab {
                let o = vq + 16 * i as usize;
                let (w1, w2) = (slot16(o + 6), slot16(o + 14));
                slots.push((w1, i as u32));
                if w2 != 0 && w2 != w1 {
                    slots.push((w2, i as u32));
                }
                vertices.push(TieVertex { pos: [s16(o), s16(o + 2), s16(o + 4)], sway: None, uv: uv_of(o + 8)?, colour: i as u32 });
            }
            let cq = vq + 16 * zab as usize;
            for i in 0..zc {
                let o = cq + 24 * i as usize;
                let (w1, w2) = (slot16(o + 6), slot16(o + 22));
                let k = (zab + i) as u32;
                slots.push((w1, k));
                if w2 != 0 && w2 != w1 {
                    slots.push((w2, k));
                }
                vertices.push(TieVertex { pos: [s16(o + 8), s16(o + 10), s16(o + 12)], sway: Some([s16(o), s16(o + 2), s16(o + 4)]), uv: uv_of(o + 16)?, colour: k });
            }
            // A vertex is written to its slot (and to its second slot when that is set); the last write to a slot stays.
            let mut by_slot: Vec<Option<u32>> = vec![None; 1024];
            for &(s, v) in &slots {
                *by_slot.get_mut(s as usize).ok_or_else(|| format!("packet at {p:#x}: slot {s}"))? = Some(v);
            }
            let mut strips = Vec::new();
            let mut triangles = Vec::new();
            for j in 3..c {
                let tv = tab(j);
                let (n, z) = (tv[0] as u64, tv[2] as u64);
                let mut tex = 0;
                for i in 0..3u64 {
                    if i + 1 < a && (u32_at(b, (p + 4 * i) as usize)? as u64) < z {
                        tex = i as usize + 1;
                    }
                }
                let mut vs = Vec::new();
                for i in 0..n {
                    let s = (z + 1 + 3 * i) as usize;
                    vs.push(by_slot.get(s).copied().flatten().ok_or_else(|| format!("packet at {p:#x}: strip {} vertex {i}: no vertex writes slot {s}", j - 3))?);
                }
                let si = strips.len();
                for i in 2..vs.len() {
                    triangles.push(TieTriangle { v: [vs[i - 2], vs[i - 1], vs[i]], texture: tex, strip: si });
                }
                strips.push(TieStrip { texture: tex, side: tv[3], vertices: vs });
            }
            let second = if h != 0 {
                let q = p + 16 * h;
                let mut registers = Vec::new();
                for i in 0..a {
                    registers.push((u32_at(b, (q + 4 * i) as usize)? / 0x50) as usize);
                }
                let mut colour = None;
                let mut uv_counts = None;
                let mut normals = Vec::new();
                let mut raw = Vec::new();
                if fl & 6 != 0 {
                    let mut c4 = [0u8; 4];
                    for (k, v) in c4.iter_mut().enumerate() {
                        let w = u32_at(b, (q + 0x10 + 4 * k as u64) as usize)?;
                        if w > 255 {
                            return Err(format!("packet at {p:#x}: second-part colour word {w:#x}"));
                        }
                        *v = w as u8;
                    }
                    colour = Some(c4);
                    let (x, y) = (u32_at(b, (q + 0x20) as usize)? as u64, u32_at(b, (q + 0x24) as usize)? as u64);
                    if x != zab || y != zc {
                        return Err(format!("packet at {p:#x}: second part gives {x} and {y} vertices, the packet has {zab} and {zc}"));
                    }
                    uv_counts = Some((x as u32, y as u32));
                    let n = (16 * ff * if fl & 4 != 0 { 2 } else { 1 }) as usize;
                    raw.extend_from_slice(&b[(q + 0x30) as usize..(q + 0x30) as usize + n]);
                    if fl & 2 != 0 {
                        for k in 0..4 * ff {
                            normals.push(packed_normal(u32_at(b, (q + 0x30 + 4 * k) as usize)?));
                        }
                    }
                }
                Some(TieSecond { registers, colour, uv_counts, normals, raw })
            } else {
                None
            };
            let nc = b[e + 0xa] as u32;
            lods[l].push(TiePacket { offset: p as u32, flags: fl, textures, vertices, strips, triangles, colour_count: nc, colour_offset: col_off, entry_byte5: b[e + 5], second });
            col_off += (nc + 3) / 4 * 16;
            pos = p + len;
        }
    }
    if pos != p34 {
        return Err(format!("the LOD packets end at {pos:#x}, the light data starts at {p34:#x}"));
    }

    // Light data: positions, three variant tables, colour patch lists.
    if tex_at < p34 || tex_at + 0x50 * ntex > size {
        return Err(format!("texture registers at {tex_at:#x} outside the class"));
    }
    let reg = tex_at - p34;
    let o = [u16_at(b, 0x2a)? as u64, u16_at(b, 0x2c)? as u64, u16_at(b, 0x2e)? as u64];
    let pl = [u16_at(b, 0x78)? as u64, u16_at(b, 0x7a)? as u64, u16_at(b, 0x7c)? as u64, u16_at(b, 0x7e)? as u64];
    let mut pls: Vec<u64> = pl.iter().copied().filter(|&x| x != 0).collect();
    pls.sort_unstable();
    pls.dedup();
    let pos_end = (n38 + 1) / 2 * 16;
    let vt_end = pls.first().copied().unwrap_or(reg);
    if o[0] != pos_end || !(o[0] <= o[1] && o[1] <= o[2] && o[2] <= vt_end) || vt_end > reg {
        return Err(format!("light data: positions end {pos_end:#x}, tables at {o:x?}, patch lists from {vt_end:#x}, region {reg:#x}"));
    }
    a_lp.add(p34, pos_end);
    a_lp.mark(p34, 8 * n38);
    if n38 % 2 != 0 {
        a_lp.zero(b, p34 + 8 * n38, 8, "position padding");
    }
    let bounds = [o[0], o[1], o[2], vt_end];
    let mut light_slots: [Vec<Vec<(u16, u8)>>; 3] = Default::default();
    for i in 0..3 {
        let (s, e) = (bounds[i], bounds[i + 1]);
        if s == e {
            continue;
        }
        let slots = &mut light_slots[i];
        // The colour word the block's output starts at.
        let mut first_slot = 0usize;
        let hs = p34 + s;
        let mut ns = Vec::new();
        for k in 0..32 {
            let v = b[(hs + k) as usize];
            if v == 0 {
                break;
            }
            ns.push(v as u64);
        }
        a_lh.add(hs, 32);
        a_lh.mark(hs, (ns.len() as u64 + 1).min(32));
        if ns.len() < 32 {
            a_lh.zero(b, hs + ns.len() as u64 + 1, 31 - ns.len() as u64, "table header after the terminator");
        }
        let total: u64 = ns.iter().sum();
        if 0x20 + 16 * total != e - s {
            return Err(format!("light table {i}: header gives {:#x} bytes, the table has {:#x}", 0x20 + 16 * total, e - s));
        }
        let mut at_b = hs + 0x20;
        for n in ns {
            let blen = 16 * n;
            chk(b, at_b, blen, "light block")?;
            a_lb.add(at_b, blen);
            let h16 = |k: u64| u16_at(b, (at_b + k) as usize).map(|v| v as u64);
            let (n1, n2, n3, n4, n5, n6, oq, bl) = (h16(0)?, h16(2)?, h16(4)?, h16(6)?, h16(8)?, h16(0xa)?, h16(0xc)?, h16(0xe)?);
            a_lb.mark(at_b, 0xe);
            a_lb.skip(at_b + 0xe, 2);
            let tot = 0x10 + n1 + n2 + n3 + n4 + n5 + n6;
            if bl != blen || n1 % 4 != 0 || n2 % 4 != 0 || n3 % 4 != 0 || n4 % 8 != 0 || n5 % 8 != 0 || n6 % 2 != 0 || tot > blen || blen - tot >= 16 {
                a_lb.fail(format!("light block at {at_b:#x}: lists {n1:#x} {n2:#x} {n3:#x} {n4:#x} {n5:#x} {n6:#x} bytes, output {oq} quadwords, size field {bl:#x} in {blen:#x}"));
                at_b += blen;
                continue;
            }
            let dmax = 16 * oq;
            let base = first_slot;
            first_slot += 4 * oq as usize;
            let mut put = |dst: u64, src: Vec<(u16, u8)>| {
                let s = base + dst as usize / 4;
                if slots.len() <= s {
                    slots.resize(s + 1, Vec::new());
                }
                slots[s] = src;
            };
            let at = |o: u64| (o / 4) as u16;
            let mut p = at_b + 0x10;
            // List 1: (source, destination) halfword pairs; sources are offsets in the 0x800 bytes of lit colours at scratchpad 0x2000.
            for k in 0..n1 / 4 {
                let (src, dst) = (h16(0x10 + 4 * k)?, h16(0x12 + 4 * k)?);
                if src >= 0x800 || src % 4 != 0 || dst >= dmax || dst % 4 != 0 {
                    a_lb.fail(format!("light block at {at_b:#x}: list 1 entry {k} source {src:#x} destination {dst:#x}, output {dmax:#x} bytes"));
                }
                put(dst, vec![(at(src), 4)]);
            }
            a_lb.mark(p, n1);
            p += n1;
            // Lists 2 and 3: words of two 9-bit source offsets and a destination (bits 2 to 13 and 14 to 22, 23 to 31).
            for list in 0..2 {
                let nn = if list == 0 { n2 } else { n3 };
                for k in 0..nn / 4 {
                    let w = u32_at(b, (p + 4 * k) as usize)?;
                    let dst = (w & 0x3ffc) as u64;
                    if w & 3 != 0 || dst >= dmax {
                        a_lb.fail(format!("light block at {:#x}: list {} word {w:#x}, output {dmax:#x} bytes", p + 4 * k, list + 2));
                    }
                    let (a, b) = (at((w >> 21 & 0x7fc) as u64), at((w >> 12 & 0x7fc) as u64));
                    put(dst, if list == 0 { vec![(a, 2), (b, 2)] } else { vec![(a, 1), (b, 3)] });
                }
                a_lb.mark(p, nn);
                p += nn;
            }
            // List 4: three source halfwords and a destination.
            for k in 0..n4 / 8 {
                let v: Vec<u64> = (0..4).map(|j| h16(p - at_b + 8 * k + 2 * j)).collect::<Result<_, _>>()?;
                if v.iter().any(|x| x % 4 != 0) || v[..3].iter().any(|&x| x >= 0x800) || v[3] >= dmax {
                    a_lb.fail(format!("light block at {:#x}: list 4 entry {k}: {v:x?}, output {dmax:#x} bytes", p + 8 * k));
                }
                put(v[3], vec![(at(v[0]), 2), (at(v[1]), 1), (at(v[2]), 1)]);
            }
            a_lb.mark(p, n4);
            p += n4;
            // List 5: a word of three 9-bit source offsets (bits 2 to 10, 11 to 19, 20 to 28), a source and a destination halfword.
            for k in 0..n5 / 8 {
                let w = u32_at(b, (p + 8 * k) as usize)?;
                let (s4, dst) = (h16(p - at_b + 8 * k + 4)?, h16(p - at_b + 8 * k + 6)?);
                if w & 3 != 0 || w >> 29 != 0 || s4 >= 0x800 || s4 % 4 != 0 || dst >= dmax || dst % 4 != 0 {
                    a_lb.fail(format!("light block at {:#x}: list 5 entry {k}: {w:#x} {s4:#x} {dst:#x}, output {dmax:#x} bytes", p + 8 * k));
                }
                let w = w as u64;
                put(dst, vec![(at(w & 0x7fc), 1), (at(w >> 9 & 0x7fc), 1), (at(w >> 18 & 0x7fc), 1), (at(s4), 1)]);
            }
            a_lb.mark(p, n5);
            p += n5;
            // List 6: destination halfwords that are set to zero.
            for k in 0..n6 / 2 {
                let dst = h16(p - at_b + 2 * k)?;
                if dst >= dmax || dst % 4 != 0 {
                    a_lb.fail(format!("light block at {:#x}: list 6 entry {k} destination {dst:#x}, output {dmax:#x} bytes", p + 2 * k));
                }
                put(dst, Vec::new());
            }
            a_lb.mark(p, n6);
            p += n6;
            a_lb.zero(b, p, at_b + blen - p, "light block padding");
            at_b += blen;
        }
    }
    for (j, &s) in pls.iter().enumerate() {
        let e = pls.get(j + 1).copied().unwrap_or(reg);
        let n = u16_at(b, (p34 + s) as usize)? as u64;
        if s % 16 != 0 || s + 2 + 2 * n > e {
            return Err(format!("patch list at {s:#x} with {n} entries before {e:#x}"));
        }
        a_pl.add(p34 + s, e - s);
        a_pl.mark(p34 + s, 2 + 2 * n);
        a_pl.zero(b, p34 + s + 2 + 2 * n, e - s - 2 - 2 * n, "patch list padding");
    }
    // Positions pad: when the count is odd the last 8 bytes of the final quadword are padding.
    // Texture registers.
    for k in 0..ntex {
        let r = tex_at + 0x50 * k;
        a_tx.add(r, 0x50);
        a_tx.zero(b, r, 8, "overwritten by func_L01_00424478 (TEX0_1)");
        a_tx.eq32(b, r + 8, 6, "GIF register number TEX0_1")?;
        a_tx.zero(b, r + 0xc, 4, "register number high word");
        a_tx.mark(r + 0x10, 8);
        a_tx.eq32(b, r + 0x18, 0x14, "GIF register number TEX1_1")?;
        a_tx.zero(b, r + 0x1c, 4, "register number high word");
        a_tx.zero(b, r + 0x20, 8, "overwritten by func_L01_00424478 (MIPTBP1_1)");
        a_tx.eq32(b, r + 0x28, 0x34, "GIF register number MIPTBP1_1")?;
        a_tx.zero(b, r + 0x2c, 4, "register number high word");
        a_tx.mark(r + 0x30, 8);
        a_tx.eq32(b, r + 0x38, 8, "GIF register number CLAMP_1")?;
        a_tx.zero(b, r + 0x3c, 4, "register number high word");
        a_tx.mark(r + 0x40, 8);
        a_tx.eq32(b, r + 0x48, 0x42, "GIF register number ALPHA_1")?;
        a_tx.zero(b, r + 0x4c, 4, "register number high word");
    }
    let tend = tex_at + 0x50 * ntex;
    if size - tend >= 0x40 || (size % 0x40) != 0 {
        return Err(format!("class of {size:#x} bytes ends {:#x} after its texture registers", size - tend));
    }
    a_pd.add(tend, size - tend);
    a_pd.zero(b, tend, size - tend, "class padding");

    flush(t, parent, hdr, at, a_h);
    flush(t, parent, bbox, at, a_b);
    flush(t, parent, ent, at, a_e);
    flush(t, parent, ph, at, a_ph);
    flush(t, parent, pt, at, a_pt);
    flush(t, parent, ptab, at, a_tab);
    flush(t, parent, pv, at, a_v);
    flush(t, parent, ps, at, a_s);
    flush(t, parent, p2t, at, a_2t);
    flush(t, parent, p2, at, a_2);
    flush(t, parent, lpos, at, a_lp);
    flush(t, parent, lhdr, at, a_lh);
    flush(t, parent, lblk, at, a_lb);
    flush(t, parent, plst, at, a_pl);
    flush(t, parent, tex, at, a_tx);
    flush(t, parent, pad, at, a_pd);
    let mut textures = Vec::new();
    for k in 0..ntex {
        let r = (tex_at + 0x50 * k) as usize;
        let k_word = u32_at(b, r + 0x10)?;
        let alpha = u64_at(b, r + 0x40)?;
        textures.push(TieTexture {
            index: k as usize,
            k_word,
            k: (((k_word & 0xfff) as i32) << 20 >> 20) as f32 / 16.0,
            mmin: u32_at(b, r + 0x14)?,
            wms: u32_at(b, r + 0x30)?,
            wmt: u32_at(b, r + 0x34)?,
            alpha,
            alpha_a: (alpha & 3) as u8,
            alpha_b: (alpha >> 2 & 3) as u8,
            alpha_c: (alpha >> 4 & 3) as u8,
            alpha_d: (alpha >> 6 & 3) as u8,
            alpha_fix: (alpha >> 32) as u8,
        });
    }
    let mut bounds = [[0f32; 4]; 8];
    for (i, c) in bounds.iter_mut().enumerate() {
        for (k, v) in c.iter_mut().enumerate() {
            *v = f32_at(b, 0x80 + 16 * i + 4 * k)?;
        }
    }
    Ok(TieClass {
        scale: f32_at(b, 0x40)?,
        centre: [f32_at(b, 0x50)?, f32_at(b, 0x54)?, f32_at(b, 0x58)?],
        radius: f32_at(b, 0x5c)?,
        bounds,
        textures,
        lods,
        lod_distances: [f32_at(b, 0x10)?, f32_at(b, 0x14)?, f32_at(b, 0x18)?],
        flags: u16_at(b, 0x3c)?,
        light_positions: (0..n38)
            .map(|k| -> Result<([i16; 3], u16), String> {
                let at = (p34 + 8 * k) as usize;
                Ok(([u16_at(b, at)? as i16, u16_at(b, at + 2)? as i16, u16_at(b, at + 4)? as i16], u16_at(b, at + 6)?))
            })
            .collect::<Result<_, _>>()?,
        light_slots,
    })
}
