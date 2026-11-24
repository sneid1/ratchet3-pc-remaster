//! Shrub classes (LEVEL core): the class data the shrub class loader func_L01_00424850 relocates and the shrub renderer
//! func_L01_004A2200 reads. `shrub` decodes one shrub class at `at` in the unpacked core root `parent`.
//!
//! A shrub class: 0x40 header bytes, a table of (pointer, size) pairs at +0x40, 16-byte aligned VIF packets, an optional texture
//! block (+0x1C), 24 hull points (+0x2C) and zero padding to 0x40.
//!
//! Each block (pointer table entry) is one DMA'd VIF packet for VU1 program 4 or 5 (func_L01_004A2200 draws a block once per
//! instance, with the instance's matrix and colour table in VU1 memory). The programs turn it into one GIF packet per instance
//! whose layout the block fixes: see `ShrubBlock`. Programs 4 and 5 read the same block data. Program 4 transforms with the
//! screen-space matrix and kicks the packet (no clipping); program 5 (clip space matrix) runs the same slot-by-slot transform, then
//! clips every triangle against the guard band: it sets ADC on the vertices of triangles it rejects or clips and kicks the clipped
//! polygons as separate small packets (an A+D tag with the entry's registers and one PRIM 0x7D triangle fan). Which program
//! draws is decided per instance (func_L01_004A2200 draws its instance lists in two passes with different matrix constants:
//! the first from 0x4A2820 with the screen matrix at 0x1E2900+0x80, the second from 0x4A36E8 with the clip-space matrix at
//! 0x1E2900+0xC0), not per class; in the traced Veldin frame program 5 draws only instances of class 21 whose vertices cross the
//! camera's near plane or frustum. The geometry is the same either way.

use crate::disc::{u16_at, u32_at};
use crate::tie::{chk, flush, Acc};
use crate::track::{Node, Space, Tracker, NOTHING};

const GS: &str = "LEVEL core shrub class (unpacked)";

/// One shrub class, as the shrub class loader func_L01_00424850 relocates it and the shrub renderer func_L01_004A2200 draws it.
/// Vertices are class-local: the instance matrix (built by func_L01_004252F0 and func_L01_004A2200 from the gameplay block's
/// +0x40 table; VU1 reads it as vf18 to vf21) multiplies a position as `(x, y, z, 1)`.
#[derive(Clone, Debug)]
pub struct ShrubClass {
    /// Bounding sphere centre, class +0x0 to +0xB (three f32): func_L01_004252F0 loads the quadword at +0 with lqc2 (0x426210,
    /// through func_L01_0040C2D0) and transforms it into the instance's bounding sphere.
    pub centre: [f32; 3],
    /// Bounding sphere radius, class +0xC (f32): func_L01_004252F0 multiplies it by the longest matrix axis (lwc1 at 0x426244).
    pub radius: f32,
    /// Class +0x10 (f32): func_L01_00424850 multiplies it by 1024.0, converts it to an integer and stores it in its class table
    /// at 0x249E70 (lwc1 at 0x4248DC; 5.0 or 10.0 in Veldin).
    pub range: f32,
    /// Class +0x14 (u16): 0 or 4. func_L01_004A2200 tests bit 0 (skips the class, 0x4A24C0) and bits 1 and 2 (stored at its
    /// scratch word 0x3F9C, tested at 0x4A280C).
    pub flags: u16,
    /// Class +0x20 (f32): func_L01_004252F0 copies it to the instance's +0x3C (lwc1 at 0x426124) and passes it to func_40BEC0
    /// (0x426254).
    pub f20: f32,
    /// The blocks of the +0x40 table, in order. Each is one DMA'd VIF packet and draws one GIF packet per instance.
    pub blocks: Vec<ShrubBlock>,
    /// The class texture block (+0x1C), if the class has one.
    pub texture_block: Option<ClassTexture>,
    /// The 24 hull points, class +0x2C: four i16 each (x, y, z, w), as func_L01_004A3D80 loads them (ld, pextlh, psraw into VU0
    /// registers, transformed by the VU0 microprograms started with vcallms 0x0 and 0x160).
    pub hull: Vec<[i16; 4]>,
}

/// One block of a shrub class: the VIF packet's data for VU1 programs 4 and 5. VU1 builds one GIF packet from it per instance
/// (XGKICK at micro 0x980 and 0xB50 in program 4). The packet is `packet_qwords` quadwords; each item sits at its `slot` (a
/// quadword index from the packet's start). Items in slot order: for each texture entry five quadwords (an A+D GIF tag the
/// program supplies, then the entry's TEX1_1, CLAMP_1, MIPTBP1_1 and TEX0_1 writes); for each strip a GIF tag, then one
/// three-quadword vertex (ST, RGBAQ, XYZF2) per vertex of the strip. A strip uses the texture entry whose registers come last
/// before it in the packet.
#[derive(Clone, Debug)]
pub struct ShrubBlock {
    /// The N texture entries (the first V4-32 UNPACK: after the header quadword and the M extra quadwords), in slot order.
    pub textures: Vec<ShrubTexture>,
    /// The M extra quadwords (GIF tags), in slot order.
    pub strips: Vec<ShrubStrip>,
    /// The vertices in upload order (the two V4-16 UNPACKs, which the programs walk together).
    pub vertices: Vec<ShrubVertex>,
    /// Quadwords in the GIF packet: the slot after the last strip's last vertex.
    pub packet_qwords: u32,
}

/// A texture entry of a block (0x40 bytes, four quadwords: the GS registers TEX1_1, CLAMP_1, MIPTBP1_1 and TEX0_1 as A+D data).
/// func_L01_00424850 (the loop at 0x424BD8 to 0x424DA4) rebuilds the four registers' data from these fields and the shrub
/// texture record; VU1 programs 4 and 5 copy the four quadwords (the loop at micro 0x1B8) into the GIF packet.
#[derive(Clone, Debug)]
pub struct ShrubTexture {
    /// Packet quadword index of the A+D GIF tag the programs write before the four registers: entry +0xC (MTIR vi04 from vf01w
    /// at micro 0x1D8 in program 4, vf18w in program 5).
    pub slot: u32,
    /// TEX1_1 K (entry +0x0, u32): func_L01_00424850 shifts it to bit 32 (`s1` loaded at 0x424BE8, dsll32 at 0x424CE0 and
    /// 0x424D74). The GS reads its low 12 bits as signed 7.4 fixed point (0xFF92 is -6.875).
    pub k: u32,
    /// TEX1_1 MMIN (entry +0x4, u32): func_L01_00424850 shifts it left by 6 (`t5` at 0x424BEC, dsll at 0x424C1C and 0x424D30) and
    /// sets MMAG = 1 (0x20, `s7`).
    pub mmin: u32,
    /// CLAMP_1 WMS (entry +0x10, u32): ored into bits 0 to 1 (`s0` loaded at 0x424BF4, ored at 0x424CD4).
    pub wms: u32,
    /// CLAMP_1 WMT (entry +0x14, u32): ored in shifted left by 2 (`t9` loaded at 0x424BFC, dsll at 0x424CC4).
    pub wmt: u32,
    /// The class-local texture index j (entry +0x20, u32): func_L01_00424850 loads byte j of the class record's 16 texture index
    /// bytes (`lbu` at 0x424BF0; it copied the 16 bytes to its table at 0x24A470 at 0x424B68 to 0x424B78). That byte is the
    /// record of the core index's shrub texture table (words 18 and 19) that builds TEX0_1, MIPTBP1_1 and CLAMP_1 bits 24 to 33
    /// (the record index itself, `t8` shifted by 24 at 0x424CE4).
    pub index: u32,
}

/// One extra quadword of a block: a GIF tag with PRE set (so it writes PRIM, which restarts the primitive), the three registers
/// ST, RGBAQ and XYZF2 in packed mode, NLOOP vertices. VU1 stores it at its `slot` (the quadword's +0xC; micro 0x150) and the GS
/// draws the vertices after it as one primitive.
#[derive(Clone, Debug)]
pub struct ShrubStrip {
    /// Packet quadword index of the tag: extra quadword +0xC.
    pub slot: u32,
    /// The PRIM register the tag writes: tag word 1 bits 15 to 25. 0x7C in every block of Veldin: triangle strip (4), Gouraud
    /// (+8), textured (+0x10), fogged (+0x20), alpha blended (+0x40), ST coordinates (FST 0).
    pub prim: u16,
    /// The tag's EOP bit (set on the block's last strip only).
    pub eop: bool,
    /// The texture entry (index into `ShrubBlock::textures`) whose registers precede the tag in the packet.
    pub texture: usize,
    /// The primitive's vertices in GS order: indices into `ShrubBlock::vertices`, one per slot after the tag (NLOOP of them, the
    /// tag's word 0). Triangle strip: vertices k, k+1, k+2 form a triangle. Program 4 never sets ADC (bit 15 of the XYZF2
    /// quadword's word 3 stays 0), so every vertex from the third on completes a triangle.
    pub vertices: Vec<usize>,
}

/// One vertex of a block: the same index in the two V4-16 UNPACKs (positions; texture coordinates and colour index). Micro 0x298
/// to 0x308 converts both quadwords to floats in place; the transform loop reads them from micro 0x3A8 on in program 4.
#[derive(Clone, Debug)]
pub struct ShrubVertex {
    /// Class-local position: three halfwords of the first UNPACK (ITOF0 at micro 0x2B8: the integers as f32, no scaling).
    /// The instance matrix multiplies it (MULAw, MADDAx, MADDAy, MADDz at micro 0x3D0 to 0x3E8 in program 4).
    pub pos: [f32; 3],
    /// The packet quadword index this vertex's ST, RGBAQ and XYZF2 quadwords are stored at: halfword 3 of the first UNPACK's
    /// quadword (ADD vf07 = vf01 + vf17 at micro 0x3C8, MTIR vi02 from vf07w at 0x468: vf17 holds the packet's base address).
    pub slot: u16,
    /// The S and T the GS gets before VU1 multiplies them by Q: halfwords 0 to 2 of the second UNPACK (ITOF12 at micro 0x2C0,
    /// 0x2D8: 4096 is 1.0, so 1.0 is the texture's size), [s, t, third]. The third is 1.0 on every vertex on the disc: it
    /// becomes the ST register's Q after the multiplication by 1/w.
    pub st: [f32; 3],
    /// Index into the instance's colour table (quadword +5 of the 28-quadword instance record, 22 more colours after it; index 23, which
    /// 464 of the 20881 vertices drawn in the traced Veldin frame use, reads the next quadword in VU1 memory): the low 15 bits of
    /// halfword 3 of the second UNPACK (IAND with 0x7FFF at micro 0x488, LQ.xyz vf04, 0(vi07) at 0x498; vi07 adds the table's
    /// address). That quadword's x, y and z are the RGBAQ quadword's R, G and B; the A comes from the instance's alpha
    /// (the +0x5 quadword's w, MOVE.w vf04 = vf22 at micro 0x398). Bit 15 of that halfword is the loop end marker (IBLTZ at
    /// micro 0x5B0, 0x6E0, 0x748 and 0x7A8): set on the fourth vertex from the end of every block and nowhere else.
    pub colour: u16,
}

/// The class texture block (+0x1C): func_L01_004A2200 sends it with the class (lqc2 of the first quadword at 0x4A30C8, lq of the
/// TEX1, TEX0 and MIPTBP1 quadwords at 0x4A3110 to 0x4A3118) and func_L01_004252F0 reads +0 (0x4260AC). func_L01_00424850 builds
/// TEX1_1 (+0x10), TEX0_1 (+0x20) and MIPTBP1_1 (+0x30) from the class record's eight s16 at +0x20: [0] width, [1] height,
/// [2] level count (MXL = [2] - 1), [3] palette GS block (TEX0 CBP), [4] TEX0 TBP0, [5] to [7] MIPTBP1 TBP1 to TBP3, each block
/// number plus the word at 0x1A1EF0 shifted right by 8 (0x424A00 to 0x424A9C).
#[derive(Clone, Debug)]
pub struct ClassTexture {
    /// The first quadword: word 0 is an f32 (func_L01_004252F0 converts it to an integer and compares its low byte plus 24.0
    /// with the instance's +0x10, 0x4260B8 to 0x4260D8); the whole quadword is loaded into VU0's vf16 (0x4A30C8).
    pub first: [u32; 4],
    /// TEX1_1 K (+0x10, u32), as `ShrubTexture::k`.
    pub k: u32,
    /// TEX1_1 MMIN (+0x14, u32), as `ShrubTexture::mmin`.
    pub mmin: u32,
    /// The second 0x40 bytes of an 0x80 block (four quadwords loaded with lqc2 at 0x4A3240 to 0x4A324C), if the block has them.
    pub second: Option<[[u32; 4]; 4]>,
}

fn f32_at(b: &[u8], at: usize) -> Result<f32, String> {
    Ok(f32::from_bits(u32_at(b, at)?))
}

fn i16_at(b: &[u8], at: usize) -> Result<i16, String> {
    Ok(u16_at(b, at)? as i16)
}

/// Decodes the class at `at` in the unpacked core root `parent` (`b` is the class's bytes, up to the next structure) and marks
/// the bytes it reads. Returns the class's geometry, textures and bounding data.
pub fn shrub(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<ShrubClass, String> {
    let size = b.len() as u64;
    chk(b, 0, 0x40, "shrub class header")?;
    let hdr = t.line(GS, "shrub class header (0x40 bytes)", "shrub::shrub", "func_L01_00424850 (+0x10, +0x1C, +0x28, +0x2C; overwrites +0x16, +0x18, +0x26), func_L01_004252F0 (+0x0 to +0xF: lqc2 at +0 through func_L01_0040C2D0 at 0x426210, lwc1 +0xC at 0x426244; +0x20: lwc1 at 0x426124; +0x1C at 0x426134; writes +0x16, +0x18), func_L01_004A2200 (+0x14, +0x28; writes +0x34, +0x36, +0x38); +0x24: no reader found (func_L01_00424850, func_L01_004252F0, func_L01_004A2090, func_L01_004A2200, func_L01_004A3D80 and func_L01_0044C1D0, the code that uses the class table at 0x2384F0, read nothing there)", Space::Unpacked);
    let tab = t.line(GS, "shrub class block table (8 bytes: pointer, size)", "shrub::shrub", "func_L01_00424850 (relocates the pointer), func_L01_004A2200 (DMA ref of the block: size / 16 quadwords)", Space::Unpacked);
    let gap = t.line(GS, "shrub class padding", "shrub::shrub", NOTHING, Space::Unpacked);
    let vif = t.line(GS, "shrub block VIF codes", "shrub::shrub", "VIF1 (the block is DMA'd as it lies: STCYCL, NOP, STMOD, UNPACK words; the three UNPACKs have the TOPS flag, V4-32 at TOPS+0, V4-16 at TOPS+N and TOPS+N+count)", Space::Unpacked);
    let bh = t.line(GS, "shrub block header quadword (count, extra quadwords, two words)", "shrub::shrub", "func_L01_00424850 (reads words 0 and 1: the texture entry count N and the extra quadword count M); VU1 programs 4 and 5 (ILWR.x at micro 0xD8 and 0x1B0: N; ILWR.y at 0xD0: M; ILWR.z at 0x260: word 2; ILWR.w at 0x268, 0x328, 0x7D0 in program 4 and 0x268, 0x358 in program 5: word 3)", Space::Unpacked);
    let ex = t.line(GS, "shrub block extra quadwords (M of 16 bytes)", "shrub::shrub", "VU1 programs 4 and 5 (the M quadwords after the header quadword in the first V4-32 UNPACK: loaded and stored into the GIF packet that XGKICK at micro 0x980 and 0xB50 (program 4) and 0x608, 0xF58 and 0x10B8 (program 5) send to the GS)", Space::Unpacked);
    let te = t.line(GS, "shrub block texture register entries (0x40 bytes)", "shrub::shrub", "func_L01_00424850 (reads +0x0, +0x4, +0x10, +0x14, +0x20; overwrites +0x0, +0x10, +0x20, +0x30 with TEX1_1, CLAMP_1, MIPTBP1_1, TEX0_1); VU1 programs 4 and 5 (the 4 quadwords of each entry are loaded and stored into the GIF packet sent to the GS by the XGKICKs of micro 0x980, 0xB50, 0x608, 0xF58, 0x10B8: data and register descriptor of each)", Space::Unpacked);
    let vx = t.line(GS, "shrub block vertex quadwords (V4-16: four halfwords)", "shrub::shrub", "VU1 programs 4 and 5 (the two V4-16 UNPACKs of a block, double buffered: first LQ at micro 0x2B8, 0x2D0, 0x2D8 and following, second at 0x2C0, 0x2D0, 0x2D8 and following; halfword 3 of the second block's quadwords is the colour index in its low 15 bits and, in bit 15, the loop end marker the programs test with IBLTZ: set on the fourth quadword from the end only, so the game's loop stops after the last vertex)", Space::Unpacked);
    let tb = t.line(GS, "shrub class texture block (+0x1C)", "shrub::shrub", "func_L01_00424850 (reads +0x10, +0x14; overwrites +0x10, +0x20, +0x30 with TEX1_1, TEX0_1, MIPTBP1_1), func_L01_004A2200 (lqc2 of the first quadword at 0x4A30C8, lq of +0x10, +0x20, +0x30 at 0x4A3110 to 0x4A3118), func_L01_004252F0 (lwc1 +0 at 0x4260AC); the second 0x40 bytes of an 0x80 block: func_L01_004A2200 (lqc2 of its four quadwords at 0x4A3240 to 0x4A324C)", Space::Unpacked);
    let hp = t.line(GS, "shrub class hull points (24 of 8 bytes: four halfwords)", "shrub::shrub", "func_L01_004A3D80 (class +0x2C pointer at 0x4A40C0; 24 points as ld of 8 bytes at 0x4A40EC to 0x4A4300, halfwords sign extended by pextlh and psraw into VU0 registers, transformed by the VU0 microprograms started with vcallms 0x0 and 0x160)", Space::Unpacked);

    let (mut a_h, mut a_t, mut a_g, mut a_v, mut a_bh, mut a_ex, mut a_te, mut a_vx, mut a_tb, mut a_hp) = (Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default(), Acc::default());

    let n = u16_at(b, 0x28)? as i16;
    if n < 0 {
        return Err(format!("shrub block count {n}"));
    }
    let n = n as u64;
    let p1c = u32_at(b, 0x1c)? as u64;
    let p2c = u32_at(b, 0x2c)? as u64;
    chk(b, 0x40, 8 * n, "shrub block table")?;

    a_h.add(0, 0x40);
    a_h.mark(0, 0x10);
    a_h.mark(0x10, 4);
    a_h.mark(0x14, 2);
    a_h.zero(b, 0x16, 2, "overwritten by func_L01_00424850");
    a_h.zero(b, 0x18, 4, "overwritten by func_L01_00424850");
    a_h.mark(0x1c, 4);
    a_h.mark(0x20, 4);
    a_h.skip(0x24, 2);
    a_h.zero(b, 0x26, 2, "overwritten by func_L01_00424850");
    a_h.mark(0x28, 2);
    a_h.zero(b, 0x2a, 2, "shrub header");
    a_h.mark(0x2c, 4);
    a_h.zero(b, 0x30, 16, "written by func_L01_004A2200 and func_L01_004252F0");
    let flags = u16_at(b, 0x14)?;
    if flags != 0 && flags != 4 {
        a_h.fail(format!("+0x14 flags {flags:#x}, expected 0 or 4"));
    }
    a_t.add(0x40, 8 * n);
    a_t.mark(0x40, 8 * n);

    let mut blocks: Vec<ShrubBlock> = Vec::new();
    let mut pos = 0x40 + 8 * n;
    let padl = (16 - pos % 16) % 16;
    a_g.add(pos, padl);
    chk(b, pos, padl, "shrub padding")?;
    a_g.zero(b, pos, padl, "block alignment padding");
    pos += padl;
    for i in 0..n {
        let (p, s) = (u32_at(b, (0x40 + 8 * i) as usize)? as u64, u32_at(b, (0x44 + 8 * i) as usize)? as u64);
        if p != pos {
            return Err(format!("shrub block {i} at {p:#x}, expected {pos:#x}"));
        }
        chk(b, p, s, "shrub block")?;
        // The VIF stream: STCYCL, NOP, STMOD and three UNPACKs (V4-32, V4-16, V4-16): (word, count, payload at, payload bytes).
        let mut at_w = p;
        let end = p + s;
        let mut ups: Vec<(u32, u64, u64, u64)> = Vec::new();
        while at_w < end {
            let w = u32_at(b, at_w as usize)?;
            let cmd = (w >> 24) & 0x7f;
            let num = (w >> 16) & 0xff;
            a_v.add(at_w, 4);
            a_v.mark(at_w, 4);
            match cmd {
                0 => at_w += 4,
                1 => {
                    if w & 0xff != (w >> 8) & 0xff {
                        a_v.fail(format!("block at {p:#x}: STCYCL cycle {} and write length {} differ", w & 0xff, (w >> 8) & 0xff));
                    }
                    at_w += 4;
                }
                5 => at_w += 4,
                0x60..=0x7f => {
                    let (vn, vl) = (((cmd >> 2) & 3) as u64, (cmd & 3) as u64);
                    let per = match (vn, vl) {
                        (3, 3) => 2,
                        _ => (vn + 1) * [4u64, 2, 1][vl.min(2) as usize],
                    };
                    let pay = (num as u64 * per + 3) / 4 * 4;
                    if at_w + 4 + pay > end {
                        return Err(format!("shrub block at {p:#x}: unpack at {at_w:#x} of {pay} bytes past the block"));
                    }
                    ups.push((w, num as u64, at_w + 4, pay));
                    at_w += 4 + pay;
                }
                _ => return Err(format!("shrub block at {p:#x}: VIF code {w:#010x} at {at_w:#x}")),
            }
        }
        // A V4-32 header block, then two V4-16 vertex blocks of the same count, one after the other in VU1 memory (TOPS-relative).
        let ok = ups.len() == 3 && {
            let (u0, u1, u2) = (ups[0], ups[1], ups[2]);
            (u0.0 >> 24) & 0x7f == 0x6c
                && u0.0 & 0xffff == 0x8000
                && (u1.0 >> 24) & 0x7f == 0x6d
                && (u2.0 >> 24) & 0x7f == 0x6d
                && u1.1 == u2.1
                && u1.0 & 0xffff == 0x8000 + u0.1 as u32
                && u2.0 & 0xffff == 0x8000 + (u0.1 + u1.1) as u32
        };
        if !ok {
            return Err(format!("shrub block at {p:#x}: unpacks {:x?} are not a V4-32 header and two equal V4-16 blocks", ups.iter().map(|u| (u.0, u.1)).collect::<Vec<_>>()));
        }
        let (u0, u1, u2) = (ups[0], ups[1], ups[2]);
        let d0 = u0.2;
        let nn = u32_at(b, d0 as usize)? as u64;
        let mm = u32_at(b, (d0 + 4) as usize)? as u64;
        if u0.1 != 1 + mm + 4 * nn {
            return Err(format!("shrub block at {p:#x}: first unpack x{} with N {nn}, M {mm}", u0.1));
        }
        // Words 2 and 3 of the header quadword: the vertex count (the second UNPACK's offset, ILWR.z at micro 0x260) and the
        // quadword offset of the vertices (ILWR.w at 0x268, 0x328, 0x7D0: past the header, the extras and the entries).
        let (cnt, voff) = (u32_at(b, (d0 + 8) as usize)? as u64, u32_at(b, (d0 + 12) as usize)? as u64);
        if cnt != u1.1 || voff != 1 + mm + 4 * nn {
            return Err(format!("shrub block at {p:#x}: header quadword words 2 and 3 are {cnt} and {voff}, the unpacks have {} vertices and the vertices at quadword {}", u1.1, 1 + mm + 4 * nn));
        }
        a_bh.add(d0, 16);
        a_bh.mark(d0, 16);
        a_ex.add(d0 + 16, 16 * mm);
        a_ex.mark(d0 + 16, 16 * mm);
        let te0 = d0 + 16 + 16 * mm;
        let mut textures = Vec::new();
        for e in 0..nn {
            let q = te0 + 0x40 * e;
            a_te.add(q, 0x40);
            a_te.mark(q, 0x40);
            a_te.eq32(b, q + 8, 0x14, "GIF register number TEX1_1")?;
            a_te.eq32(b, q + 0x18, 8, "GIF register number CLAMP_1")?;
            a_te.eq32(b, q + 0x28, 0x34, "GIF register number MIPTBP1_1")?;
            a_te.eq32(b, q + 0x38, 6, "GIF register number TEX0_1")?;
            let w = |o: u64| u32_at(b, (q + o) as usize);
            textures.push(ShrubTexture { slot: w(0xc)?, k: w(0)?, mmin: w(4)?, wms: w(0x10)?, wmt: w(0x14)?, index: w(0x20)? });
        }
        for u in [u1, u2] {
            a_vx.add(u.2, u.3);
            a_vx.mark(u.2, u.3);
        }
        // The vertices: position quadword k and texture quadword k.
        let mut vertices = Vec::new();
        for k in 0..u1.1 {
            let (pa, ta) = ((u1.2 + 8 * k) as usize, (u2.2 + 8 * k) as usize);
            let (slot, cw) = (i16_at(b, pa + 6)?, i16_at(b, ta + 6)?);
            if slot < 0 {
                return Err(format!("shrub block at {p:#x} vertex {k}: negative packet slot {slot}"));
            }
            // The loop end marker: bit 15 of the texture quadword's halfword 3, on the fourth vertex from the end only.
            if (cw < 0) != (k + 4 == u1.1) {
                return Err(format!("shrub block at {p:#x} vertex {k} of {}: end marker {}", u1.1, cw < 0));
            }
            vertices.push(ShrubVertex {
                pos: [i16_at(b, pa)? as f32, i16_at(b, pa + 2)? as f32, i16_at(b, pa + 4)? as f32],
                slot: slot as u16,
                st: [i16_at(b, ta)? as f32 / 4096.0, i16_at(b, ta + 2)? as f32 / 4096.0, i16_at(b, ta + 4)? as f32 / 4096.0],
                colour: (cw as u16) & 0x7fff,
            });
        }
        // The extra quadwords: GIF tags.
        let mut strips: Vec<ShrubStrip> = Vec::new();
        for m in 0..mm {
            let q = (d0 + 16 + 16 * m) as usize;
            let (w0, w1, w2, w3) = (u32_at(b, q)?, u32_at(b, q + 4)?, u32_at(b, q + 8)?, u32_at(b, q + 12)?);
            // PRE set, FLG 0 (packed), NREG 3, the registers ST (2), RGBAQ (1) and XYZF2 (4); bits 0 to 13 of word 1 are zero.
            if w0 & 0x7fff == 0 || w1 & 0x3fff != 0 || w1 & 0x4000 == 0 || (w1 >> 26) & 3 != 0 || w1 >> 28 != 3 || w2 != 0x412 {
                return Err(format!("shrub block at {p:#x} extra quadword {m}: GIF tag {w0:#x} {w1:#x} {w2:#x}"));
            }
            let nloop = (w0 & 0x7fff) as usize;
            strips.push(ShrubStrip { slot: w3, prim: ((w1 >> 15) & 0x7ff) as u16, eop: w0 >> 15 & 1 == 1, texture: 0, vertices: vec![0; nloop] });
        }
        if strips.last().is_some_and(|s| !s.eop) || strips.iter().rev().skip(1).any(|s| s.eop) {
            return Err(format!("shrub block at {p:#x}: EOP is not set on the last extra quadword only"));
        }
        // The packet: texture entries and strips tile it from slot 0 without gaps.
        let mut items: Vec<(u32, u32, bool, usize)> = Vec::new();
        for (i, tx) in textures.iter().enumerate() {
            items.push((tx.slot, 5, true, i));
        }
        for (i, s) in strips.iter().enumerate() {
            items.push((s.slot, 1 + 3 * s.vertices.len() as u32, false, i));
        }
        items.sort();
        let mut at_slot = 0;
        let mut tex_now: Option<usize> = None;
        let mut by_slot: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for (k, v) in vertices.iter().enumerate() {
            by_slot.insert(v.slot as u32, k);
        }
        for &(slot, len, is_tex, i) in &items {
            if slot != at_slot {
                return Err(format!("shrub block at {p:#x}: item at packet quadword {slot}, expected {at_slot}"));
            }
            at_slot += len;
            if is_tex {
                tex_now = Some(i);
            } else {
                let tx = tex_now.ok_or_else(|| format!("shrub block at {p:#x}: strip at quadword {slot} before any texture entry"))?;
                strips[i].texture = tx;
                for j in 0..strips[i].vertices.len() {
                    let vs = slot + 1 + 3 * j as u32;
                    strips[i].vertices[j] = *by_slot.get(&vs).ok_or_else(|| format!("shrub block at {p:#x}: no vertex for slot {vs}"))?;
                }
            }
        }
        if !textures.windows(2).all(|w| w[0].slot < w[1].slot) {
            return Err(format!("shrub block at {p:#x}: texture entries are not in slot order"));
        }
        if !strips.windows(2).all(|w| w[0].slot < w[1].slot) {
            return Err(format!("shrub block at {p:#x}: extra quadwords are not in slot order"));
        }
        blocks.push(ShrubBlock { textures, strips, vertices, packet_qwords: at_slot });
        pos = end;
    }
    let mut texture_block = None;
    if p1c != 0 {
        let tl = p2c.wrapping_sub(p1c);
        if p1c != pos || (tl != 0x40 && tl != 0x80) {
            return Err(format!("shrub texture block at {p1c:#x} (blocks end {pos:#x}) of {tl:#x} bytes"));
        }
        chk(b, p1c, tl, "shrub texture block")?;
        a_tb.add(p1c, tl);
        a_tb.mark(p1c, tl);
        a_tb.eq32(b, p1c + 0x18, 0x14, "GIF register number TEX1_1")?;
        a_tb.eq32(b, p1c + 0x28, 6, "GIF register number TEX0_1")?;
        a_tb.eq32(b, p1c + 0x38, 0x34, "GIF register number MIPTBP1_1")?;
        let quad = |at: u64| -> Result<[u32; 4], String> { Ok([u32_at(b, at as usize)?, u32_at(b, at as usize + 4)?, u32_at(b, at as usize + 8)?, u32_at(b, at as usize + 12)?]) };
        let second = if tl == 0x80 { Some([quad(p1c + 0x40)?, quad(p1c + 0x50)?, quad(p1c + 0x60)?, quad(p1c + 0x70)?]) } else { None };
        texture_block = Some(ClassTexture { first: quad(p1c)?, k: u32_at(b, p1c as usize + 0x10)?, mmin: u32_at(b, p1c as usize + 0x14)?, second });
        pos = p2c;
    }
    if p2c != pos {
        return Err(format!("shrub hull points at {p2c:#x}, expected {pos:#x}"));
    }
    chk(b, p2c, 0xc0, "shrub hull points")?;
    a_hp.add(p2c, 0xc0);
    a_hp.mark(p2c, 0xc0);
    let nat = (p2c + 0xc0 + 0x3f) & !0x3f;
    if nat != size {
        return Err(format!("shrub class of {size:#x} bytes, its data ends at {nat:#x}"));
    }
    a_g.add(p2c + 0xc0, nat - p2c - 0xc0);
    a_g.zero(b, p2c + 0xc0, nat - p2c - 0xc0, "class padding");
    let mut hull = Vec::new();
    for k in 0..24usize {
        let a = p2c as usize + 8 * k;
        hull.push([i16_at(b, a)?, i16_at(b, a + 2)?, i16_at(b, a + 4)?, i16_at(b, a + 6)?]);
    }

    flush(t, parent, hdr, at, a_h);
    flush(t, parent, tab, at, a_t);
    flush(t, parent, gap, at, a_g);
    flush(t, parent, vif, at, a_v);
    flush(t, parent, bh, at, a_bh);
    flush(t, parent, ex, at, a_ex);
    flush(t, parent, te, at, a_te);
    flush(t, parent, vx, at, a_vx);
    flush(t, parent, tb, at, a_tb);
    flush(t, parent, hp, at, a_hp);
    Ok(ShrubClass {
        centre: [f32_at(b, 0)?, f32_at(b, 4)?, f32_at(b, 8)?],
        radius: f32_at(b, 0xc)?,
        range: f32_at(b, 0x10)?,
        flags,
        f20: f32_at(b, 0x20)?,
        blocks,
        texture_block,
        hull,
    })
}
