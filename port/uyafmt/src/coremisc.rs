//! The LEVEL core's sky, the regions at index words 0x19, 0x1A and 0x29-0x2E, and the core index tables that core.rs leaves
//! undecoded, as the level core loader func_L01_00428750 (the same function in all 34 story levels) and the code that uses the
//! data read them. Every structure here was checked against all 34 story levels; a value the decoder does not expect fails the
//! structure's line.
//!
//! Sky (core word 4, func_L01_00422698): a 0x20-byte header, s16 mesh-count mesh offsets (u32, at +0x20), the texture records (16
//! bytes: u32 CLUT offset, u32 texel offset, u32 width, u32 height; offsets from the texture pool), the texture pool (a 0x400-byte
//! CLUT then the texels, per record) at the 0x40-aligned offset after the records, then the meshes. A mesh is a 0x10-byte header
//! (s16 record count, u16 flags, three s16 angles and three s16 angle steps, which func_L01_004A45B0 accumulates every frame),
//! then count 0x20-byte records (four f32 bounding sphere read by func_L01_004A5220, the u32 offset of the record's data, u16
//! vertex count A, u16 triangle count B, four u16 section offsets), then the records' data blocks: A vertices of 8 bytes (s16 x,
//! y, z and an s16 passed on; func_L01_004A4D40), A u32 per-vertex words (a colour, or two s16 texture coordinates), B triangles
//! of 4 bytes (u8 i0, i1, i2 and a u8 texture index; func_L01_004A4E38, func_L01_004A4F38), zero bytes up to a multiple of 16.
//!
//! The regions at words 0x19 and 0x1A are pools of 8-bit textures. Word 0x14/0x15's records (func_L01_004224C8) address the
//! region at 0x19: u32 CLUT offset, u32 zero, u32 texel offset, u32 size (width and height). Word 0x16/0x17's records
//! (func_L01_004225E8, uploaded by func_L01_00401CD0 from the region at 0x1A) are u32 CLUT offset, u32 texel offset, u32 width,
//! u32 height. A CLUT is 256 RGBA32 entries (0x400 bytes); the records' blocks tile each region exactly, and the region at 0x1A
//! ends with 0x40 zero bytes that word 0x28 points at.
//!
//! The regions at words 0x2A, 0x2C and 0x2E (func_L01_003FEE38) are 0x80-byte bit sets after a 16-byte header (f32 x, y, z of a
//! split point, u32 zero), picked by the side of the split point the camera is on (0x2A: 8 octants) or by its direction from it
//! (0x2C and 0x2E: 0x800 directions); the region's last 0x30 bytes are zero.

use crate::disc::{u16_at, u32_at};
use crate::track::{Node, Space, Tracker, NOTHING};
use std::collections::BTreeSet;

const LOADER: &str = "func_L01_00428750 (same in all 34 story levels)";

fn bad(what: &str, at: u64, v: u64, want: &str) -> String {
    format!("{what} at {at:#x}: {v:#x}, expected {want}")
}

fn s16_at(b: &[u8], at: usize) -> Result<i64, String> {
    Ok(u16_at(b, at)? as i16 as i64)
}

/// A decoder's plan: the byte ranges (offset, length) of each line kind, checked against the region.
struct Plan {
    r: Vec<Vec<(u64, u64)>>,
}

impl Plan {
    fn new(kinds: usize) -> Plan {
        Plan { r: vec![Vec::new(); kinds] }
    }

    fn add(&mut self, k: usize, at: u64, len: u64, n: u64, what: &str) -> Result<(), String> {
        if at + len > n {
            return Err(format!("{what} at {at:#x}+{len:#x}: past the region's {n:#x} bytes"));
        }
        if len > 0 {
            self.r[k].push((at, len));
        }
        Ok(())
    }

    /// No two structures overlap; every byte between them is zero and goes to line kind `pad`.
    fn finish(&mut self, b: &[u8], pad: usize) -> Result<(), String> {
        let n = b.len() as u64;
        let mut all: Vec<(u64, u64)> = self.r.iter().flatten().copied().collect();
        all.sort_unstable();
        let mut at = 0u64;
        let mut gaps = Vec::new();
        for &(a, l) in &all {
            if a < at {
                return Err(format!("structures overlap at {a:#x} (the previous one ends at {at:#x})"));
            }
            if a > at {
                gaps.push((at, a - at));
            }
            at = a + l;
        }
        if at < n {
            gaps.push((at, n - at));
        }
        for &(a, l) in &gaps {
            if let Some(k) = b[a as usize..(a + l) as usize].iter().position(|&v| v != 0) {
                return Err(bad("byte between structures", a + k as u64, b[a as usize + k] as u64, "zero"));
            }
        }
        self.r[pad].extend(gaps);
        for r in self.r.iter_mut() {
            r.sort_unstable();
        }
        Ok(())
    }
}

/// One line per kind with ranges; every byte of an instance is a named field the decoder read, so each is marked whole.
fn emit(t: &mut Tracker, region: &mut Node, group: &str, decoder: &'static str, lines: &[(&'static str, &'static str)], p: &Plan) {
    for (k, (name, game)) in lines.iter().enumerate() {
        if p.r[k].is_empty() {
            continue;
        }
        let l = t.line(group, name, decoder, game, Space::Unpacked);
        let mut n = t.pieces(region, l, &p.r[k]);
        let total = n.size;
        n.mark(0, total);
        t.close(n);
    }
}

/// The whole region as one failed line, and the error.
fn fail_region<T>(t: &mut Tracker, mut region: Node, group: &str, name: &str, decoder: &'static str, game: &'static str, len: u64, e: String) -> Result<T, String> {
    let l = t.line(group, name, decoder, game, Space::Unpacked);
    let mut n = t.child(&mut region, l, 0, len);
    n.fail(e.clone());
    t.close(n);
    t.close(region);
    Err(e)
}

// ---------------------------------------------------------------------------------------------------------------------------
// Sky

const SKY_G: &str = "LEVEL core sky (unpacked)";

/// The level's sky as func_L01_004A46C8 draws it each frame (the meshes in order, each through func_L01_004A48F8): around the eye,
/// with the camera's rotation and projection and no translation (the matrix at 0x1E2A00 that func_L01_004A4C98 combines with the
/// mesh's rotation), before the world.
pub struct Sky {
    pub meshes: Vec<SkyMesh>,
    /// The texture records, as the GS samples them (func_L01_004A4F38 uploads each record's CLUT as a 16 x 16 PSMCT32 image,
    /// read with CSM1, and its 8-bit texels): RGBA, alpha 0x80 = 1.0, one level.
    pub textures: Vec<crate::core::Texture>,
}

/// A sky mesh (its header and records).
pub struct SkyMesh {
    /// The flags as the loader leaves them (see `sky_plan`). Bit 0: the vertex words are colours (func_L01_004A4B00, func_L01_004A4E38: GIF template 0x1422E0, PRIM 0x4B Gouraud
    /// and blended, ALPHA_1 0x44, TEST_1 ZTST ALWAYS); clear: they are texture coordinates (func_L01_004A4958, func_L01_004A4F38:
    /// PRIM 0x5B textured and blended, TEST_1 0x3308B (alpha GEQUAL 8), no Z write). Bit 1 (textured meshes): template 0x1423C0,
    /// ALPHA_1 0xA8 (the colour replaces the frame's) instead of 0x142350's 0x44.
    pub flags: u16,
    /// +4, +6, +8: the angles about x, y and z (2 pi / 65536 units) func_L01_004A45B0 builds the mesh's rotation from (func_0040C678:
    /// Rz Ry Rx, applied to a vertex as a row vector); +0xA, +0xC, +0xE: what it adds to them each frame.
    pub angles: [i16; 3],
    pub steps: [i16; 3],
    pub parts: Vec<SkyPart>,
}

/// One record of a sky mesh: its data block.
pub struct SkyPart {
    /// +0: the bounding sphere func_L01_004A5220 tests.
    pub sphere: [f32; 4],
    /// x, y, z (s16, func_L01_004A4D40 converts them with vitof0) and a fourth s16 whose low byte is the vertex's alpha in a
    /// textured mesh (func_L01_004A4F38 at 0x4A4FDC: lbu +0xC of the transformed vertex, shifted into RGBAQ's A; R, G, B are 0x80).
    pub vertices: Vec<[i16; 4]>,
    /// One word per vertex: an RGBA colour (0x80 = 1.0) in a coloured mesh; two s16 texture coordinates in units of 1/4096 of the
    /// texture (vitof12 at 0x4A5050) in a textured one, sent as ST with Q 1.0.
    pub words: Vec<u32>,
    /// Vertex indices i0, i1, i2 and a texture index into `Sky::textures` (read in textured meshes only).
    pub triangles: Vec<[u8; 4]>,
}
const SKY_D: &str = "coremisc::sky";
const SK_HDR: usize = 0;
const SK_MOFF: usize = 1;
const SK_TREC: usize = 2;
const SK_CLUT: usize = 3;
const SK_TEXEL: usize = 4;
const SK_MHDR: usize = 5;
const SK_MREC: usize = 6;
const SK_VERT: usize = 7;
const SK_VWORD: usize = 8;
const SK_TRI: usize = 9;
const SK_PAD: usize = 10;

const SKY_LINES: [(&str, &str); 11] = [
    (
        "sky header (u32 zero, u16 zero, s16 mesh count, u32 zero, s16 texture record count, u16 zero, u32 texture records offset, u32 texture pool offset, u32 offset equal to the texture records offset, u32 zero)",
        "func_L01_00422698 (same in all 34 story levels) stores 1 over the u16 at +4 and relocates the offsets at +0x10, +0x14, +0x18 and +0x1C (+0x1C only when not zero) and reads the counts at +6 and +0xC; func_L01_004A46C8 reads the mesh count and func_L01_004A4768 the texture record count; +0, +8 and +0xE are read by nothing and zero, the u16 at +4 is zero on the disc and +0x18 equals +0x10",
    ),
    (
        "sky mesh offsets (u32 offset of each mesh)",
        "func_L01_00422698 (same in all 34 story levels) relocates them; func_L01_004A45B0 and func_L01_004A48F8 read them",
    ),
    (
        "sky texture records (16 bytes: u32 CLUT offset, u32 texel offset, u32 width, u32 height; offsets from the texture pool)",
        "func_L01_00422698 (same in all 34 story levels) turns each into a texture table entry: the offsets divided by 16 into +0xA and +8, the log2 of the width and height (plzcw) into +0xC and +0xE",
    ),
    (
        "sky texture CLUTs (256 RGBA32 entries of 4 u8)",
        "func_L01_004A4F38 (same in all 34 story levels) builds the GS upload of the texture table entry's CLUT",
    ),
    (
        "sky texture texels (8-bit, width x height)",
        "func_L01_004A4F38 (same in all 34 story levels) builds the GS upload of the texture table entry's texels",
    ),
    (
        "sky mesh headers (s16 record count, u16 flags, 3 x s16 angle, 3 x s16 angle step)",
        "func_L01_00422698 (same in all 34 story levels) reads the count and flags (and moves a flags value from +4 to +2 when bit 0 is clear); func_L01_004A4958 and func_L01_004A4B00 read the count and the flags; func_L01_004A45B0 adds the steps at +0xA, +0xC and +0xE to the angles at +4, +6 and +8 each frame and reads the angles",
    ),
    (
        "sky mesh records (4 x f32 bounding sphere x, y, z, radius; u32 data offset; u16 vertex count; u16 triangle count; 4 x u16 section offsets)",
        "func_L01_004A5220 (same in all 34 story levels) reads the sphere; func_L01_00422698 relocates the data offset; func_L01_004A4958 and func_L01_004A4B00 read the data offset, the counts and the offsets at +0x18, +0x1A and +0x1C; the offset at +0x1E (the block's size) is read by nothing and equals the sections rounded up to 16",
    ),
    (
        "sky mesh vertices (s16 x, y, z and an s16 passed on to the transformed vertex)",
        "func_L01_004A4D40 (same in all 34 story levels)",
    ),
    (
        "sky mesh vertex words (u32 per vertex: a colour, or two s16 texture coordinates)",
        "func_L01_004A4E38 copies them into the draw packet (meshes with flags bit 0 set); func_L01_004A4F38 converts them with vitof12 (bit 0 clear) (same in all 34 story levels)",
    ),
    (
        "sky mesh triangles (u8 i0, i1, i2 and a u8 texture index)",
        "func_L01_004A4E38 and func_L01_004A4F38 (same in all 34 story levels); func_L01_004A4F38 reads the texture index and looks it up in the texture table",
    ),
    ("zero padding (header to the texture pool, blocks to 16 bytes, the region's end)", NOTHING),
];

fn sky_plan(b: &[u8]) -> Result<(Plan, Sky), String> {
    let n = b.len() as u64;
    let mut p = Plan::new(SKY_LINES.len());
    if n < 0x40 {
        return Err(format!("sky region of {n:#x} bytes is shorter than its 0x40-byte header"));
    }
    for (at, len, what) in [(0usize, 4usize, "header +0"), (4, 2, "header +4"), (8, 4, "header +8"), (0xe, 2, "header +0xE")] {
        if let Some(k) = b[at..at + len].iter().position(|&v| v != 0) {
            return Err(bad(what, (at + k) as u64, b[at + k] as u64, "zero"));
        }
    }
    let (nm, nt) = (s16_at(b, 6)?, s16_at(b, 0xc)?);
    if !(1..=8).contains(&nm) {
        return Err(bad("mesh count", 6, nm as u64, "1 to 8 (the offsets fit before +0x40)"));
    }
    if nt < 1 {
        return Err(bad("texture record count", 0xc, nt as u64, "at least 1"));
    }
    let (toff, poff, o18, o1c) = (u32_at(b, 0x10)? as u64, u32_at(b, 0x14)? as u64, u32_at(b, 0x18)? as u64, u32_at(b, 0x1c)? as u64);
    if toff != 0x40 || o18 != 0x40 || o1c != 0 {
        return Err(format!("sky header offsets +0x10 {toff:#x}, +0x18 {o18:#x}, +0x1C {o1c:#x}: expected 0x40, 0x40 and 0"));
    }
    let (nm, nt) = (nm as u64, nt as u64);
    if poff != (0x40 + 16 * nt + 0x3f) & !0x3f {
        return Err(bad("texture pool offset", 0x14, poff, "the texture records' end rounded up to 0x40"));
    }
    p.add(SK_HDR, 0, 0x20, n, "header")?;
    p.add(SK_MOFF, 0x20, 4 * nm, n, "mesh offsets")?;
    p.add(SK_TREC, toff, 16 * nt, n, "texture records")?;
    // The texture pool: per record a CLUT and the texels.
    let mut pool: BTreeSet<(u64, u64, bool)> = BTreeSet::new();
    let mut sky = Sky { meshes: Vec::with_capacity(nm as usize), textures: Vec::with_capacity(nt as usize) };
    for i in 0..nt {
        let r = (toff + 16 * i) as usize;
        let (w0, w1, w2, w3) = (u32_at(b, r)? as u64, u32_at(b, r + 4)? as u64, u32_at(b, r + 8)? as u64, u32_at(b, r + 12)? as u64);
        if w0 % 16 != 0 || w1 % 16 != 0 {
            return Err(format!("sky texture record {i}: offsets {w0:#x} and {w1:#x} are not multiples of 16"));
        }
        if !w2.is_power_of_two() || !w3.is_power_of_two() {
            return Err(format!("sky texture record {i}: {w2} x {w3} texels"));
        }
        pool.insert((poff + w0, 0x400, true));
        pool.insert((poff + w1, w2 * w3, false));
        sky.textures.push(sky_texture(b, (poff + w0) as usize, (poff + w1) as usize, w2 as usize, w3 as usize).map_err(|e| format!("sky texture record {i}: {e}"))?);
    }
    for &(a, l, clut) in &pool {
        p.add(if clut { SK_CLUT } else { SK_TEXEL }, a, l, n, "texture pool block")?;
    }
    // The meshes and their data blocks.
    for i in 0..nm {
        let mo = u32_at(b, 0x20 + 4 * i as usize)? as u64;
        if mo % 16 != 0 {
            return Err(bad("mesh offset", 0x20 + 4 * i, mo, "a multiple of 16"));
        }
        if mo + 0x10 > n {
            return Err(format!("sky mesh {i} at {mo:#x}: past the region's {n:#x} bytes"));
        }
        let (cnt, flags) = (s16_at(b, mo as usize)?, u16_at(b, mo as usize + 2)?);
        if cnt < 0 || flags > 3 {
            return Err(format!("sky mesh {i} at {mo:#x}: record count {cnt}, flags {flags:#x}"));
        }
        p.add(SK_MHDR, mo, 0x10, n, "mesh header")?;
        p.add(SK_MREC, mo + 0x10, 0x20 * cnt as u64, n, "mesh records")?;
        let h = |o: usize| s16_at(b, mo as usize + o).map(|v| v as i16);
        let mut angles = [h(4)?, h(6)?, h(8)?];
        // func_L01_00422698 (0x4227B8): a mesh whose flags have bit 0 clear and whose +4 is not zero takes +4 as its flags, and +4
        // becomes 0.
        let mut flags = flags;
        if flags & 1 == 0 && angles[0] != 0 {
            flags = angles[0] as u16;
            angles[0] = 0;
            if flags > 3 {
                return Err(format!("sky mesh {i} at {mo:#x}: flags {flags:#x} moved from +4"));
            }
        }
        let mut mesh = SkyMesh { flags, angles, steps: [h(0xa)?, h(0xc)?, h(0xe)?], parts: Vec::with_capacity(cnt as usize) };
        for k in 0..cnt as u64 {
            let r = (mo + 0x10 + 0x20 * k) as usize;
            let (part, ptr, s) = sky_part(b, r, flags, nt).map_err(|e| format!("sky mesh {i} record {k}: {e}"))?;
            let (a, bb) = (part.vertices.len() as u64, part.triangles.len() as u64);
            p.add(SK_VERT, ptr, 8 * a, n, "vertices")?;
            p.add(SK_VWORD, ptr + s[1], 4 * a, n, "vertex words")?;
            p.add(SK_TRI, ptr + s[2], 4 * bb, n, "triangles")?;
            p.add(SK_PAD, ptr + s[2] + 4 * bb, s[3] - s[2] - 4 * bb, n, "block padding")?;
            mesh.parts.push(part);
        }
        sky.meshes.push(mesh);
    }
    p.finish(b, SK_PAD)?;
    Ok((p, sky))
}

/// A sky texture: the 0x400-byte CLUT at `clut` and the `width` x `height` 8-bit texels at `texels` in `b`, as the GS samples them
/// (func_L01_004A4F38 uploads the CLUT as a 16 x 16 PSMCT32 image, read with CSM1).
fn sky_texture(b: &[u8], clut: usize, texels: usize, width: usize, height: usize) -> Result<crate::core::Texture, String> {
    let (cl, tx) = (b.get(clut..clut + 0x400), b.get(texels..texels + width * height));
    let (Some(cl), Some(tx)) = (cl, tx) else { return Err(format!("the CLUT at {clut:#x} or the {width} x {height} texels at {texels:#x} past the data")) };
    // CSM1: entry i of the 16 x 16 CLUT image is at index i with bits 3 and 4 swapped.
    let colour = |i: usize| -> [u8; 4] {
        let j = (i & !0x18) | ((i & 8) << 1) | ((i & 0x10) >> 1);
        [cl[4 * j], cl[4 * j + 1], cl[4 * j + 2], cl[4 * j + 3]]
    };
    let clut: Vec<[u8; 4]> = (0..256).map(colour).collect();
    Ok(crate::core::Texture { width, height, levels: vec![tx.iter().map(|&v| colour(v as usize)).collect()], gs: Some(crate::core::GsLevel { psm: 0x13, texels: tx.to_vec(), clut }) })
}

/// A sky mesh record at `r` in `b` and its data block, whose offset is an index into `b` (on the disc the region's own offset; in
/// RAM the address func_L01_00422698 relocates it to), in a mesh with `flags` of a sky with `nt` texture records: the part, its
/// data block's offset and its four section offsets.
fn sky_part(b: &[u8], r: usize, flags: u16, nt: u64) -> Result<(SkyPart, u64, [u64; 4]), String> {
    let ptr = u32_at(b, r + 0x10)? as u64;
    let (a, bb) = (u16_at(b, r + 0x14)? as u64, u16_at(b, r + 0x16)? as u64);
    let s = [u16_at(b, r + 0x18)? as u64, u16_at(b, r + 0x1a)? as u64, u16_at(b, r + 0x1c)? as u64, u16_at(b, r + 0x1e)? as u64];
    if s[0] != 0 || s[1] != 8 * a || s[2] != s[1] + 4 * a || s[3] != (s[2] + 4 * bb + 15) & !15 {
        return Err(format!("at {r:#x}: {a} vertices, {bb} triangles, section offsets {s:x?}"));
    }
    if ptr + s[3] > b.len() as u64 {
        return Err(format!("data block {ptr:#x}+{:#x} past the data", s[3]));
    }
    let tri = (ptr + s[2]) as usize;
    for q in 0..bb as usize {
        for c in 0..3 {
            let x = b[tri + 4 * q + c] as u64;
            if x >= a {
                return Err(bad("sky triangle index", (tri + 4 * q + c) as u64, x, &format!("below the vertex count {a}")));
            }
        }
        if flags & 1 == 0 && b[tri + 4 * q + 3] as u64 >= nt {
            return Err(bad("sky triangle texture index", (tri + 4 * q + 3) as u64, b[tri + 4 * q + 3] as u64, &format!("below the texture record count {nt}")));
        }
    }
    let f = |o: usize| u32_at(b, r + o).map(f32::from_bits);
    let v = ptr as usize;
    let vertices = (0..a as usize).map(|j| -> Result<[i16; 4], String> { Ok([s16_at(b, v + 8 * j)?, s16_at(b, v + 8 * j + 2)?, s16_at(b, v + 8 * j + 4)?, s16_at(b, v + 8 * j + 6)?].map(|x| x as i16)) }).collect::<Result<Vec<_>, String>>()?;
    let words = (0..a as usize).map(|j| u32_at(b, v + s[1] as usize + 4 * j)).collect::<Result<Vec<_>, String>>()?;
    let triangles = (0..bb as usize).map(|q| [b[tri + 4 * q], b[tri + 4 * q + 1], b[tri + 4 * q + 2], b[tri + 4 * q + 3]]).collect();
    Ok((SkyPart { sphere: [f(0)?, f(4)?, f(8)?, f(0xc)?], vertices, words, triangles }, ptr, s))
}

/// The sky func_L01_00422698 has relocated in EE RAM at `at`: the one its pointer (0x1E0E70 on Veldin) names and func_L01_004A46C8
/// draws each frame. The loader stores 1 over the header's u16 at +4, turns the offsets at +0x10, +0x14 and +0x18, the mesh
/// offsets and the records' data offsets into addresses, moves a textured mesh's flags from +4 to +2, and makes each texture record
/// a texture table entry: +8 the texel offset / 16, +0xA the CLUT offset / 16 (from the texture pool), +0xC and +0xE the log2 of
/// the width and height (0x1E - plzcw), +0 the GS setting func_L01_004A4F38 keeps there for the frame. The angles at +4, +6 and +8
/// of a mesh's header are where func_L01_004A45B0 has advanced them to.
pub fn sky_in_ram(ram: &[u8], at: u32) -> Result<Sky, String> {
    let h = at as usize;
    let word = |o: usize| u32_at(ram, h + o).map(|v| v as usize);
    let (nm, nt) = (s16_at(ram, h + 6)?, s16_at(ram, h + 0xc)?);
    // func_L01_004A46C8 draws meshes while their index is below the s16 at +6: none when it is not positive. Level 27 entered from
    // Veldin by the game's own level change: Veldin's code loads level 27's core and points Veldin's variable at its sky, and
    // level 27's own variable stays 0 (its sky the zeros at address 0).
    if nm <= 0 {
        return Ok(Sky { meshes: Vec::new(), textures: Vec::new() });
    }
    if u16_at(ram, h + 4)? != 1 {
        return Err(format!("the sky at {at:#x}: header +4 is {:#x}, not the 1 its loader stores", u16_at(ram, h + 4)?));
    }
    if nm > 8 || nt < 1 {
        return Err(format!("the sky at {at:#x}: {nm} meshes (1 to 8) and {nt} texture records (at least 1)"));
    }
    let (table, pool) = (word(0x10)?, word(0x14)?);
    if table != h + 0x40 || word(0x18)? != table || word(0x1c)? != 0 || pool != h + ((0x40 + 16 * nt as usize + 0x3f) & !0x3f) {
        return Err(format!("the sky at {at:#x}: texture table at {table:#x}, pool at {pool:#x}, +0x18 {:#x}, +0x1C {:#x}", word(0x18)?, word(0x1c)?));
    }
    let mut sky = Sky { meshes: Vec::with_capacity(nm as usize), textures: Vec::with_capacity(nt as usize) };
    for i in 0..nt as usize {
        let e = table + 16 * i;
        let (texels, clut) = (16 * u16_at(ram, e + 8)? as usize, 16 * u16_at(ram, e + 0xa)? as usize);
        let (lw, lh) = (s16_at(ram, e + 0xc)?, s16_at(ram, e + 0xe)?);
        // The GS's largest texture is 1024 x 1024 (TEX0's TW and TH at most 10).
        if !(0..=10).contains(&lw) || !(0..=10).contains(&lh) {
            return Err(format!("the sky at {at:#x}: texture {i} of 2^{lw} x 2^{lh} texels"));
        }
        sky.textures.push(sky_texture(ram, pool + clut, pool + texels, 1 << lw, 1 << lh).map_err(|e| format!("the sky at {at:#x}: texture {i}: {e}"))?);
    }
    for i in 0..nm as usize {
        let m = word(0x20 + 4 * i)?;
        let (cnt, flags) = (s16_at(ram, m)?, u16_at(ram, m + 2)?);
        if m % 16 != 0 || cnt < 0 || flags > 3 {
            return Err(format!("the sky at {at:#x}: mesh {i} at {m:#x}: record count {cnt}, flags {flags:#x}"));
        }
        let a = |o: usize| s16_at(ram, m + o).map(|v| v as i16);
        let mut mesh = SkyMesh { flags, angles: [a(4)?, a(6)?, a(8)?], steps: [a(0xa)?, a(0xc)?, a(0xe)?], parts: Vec::with_capacity(cnt as usize) };
        for k in 0..cnt as usize {
            mesh.parts.push(sky_part(ram, m + 0x10 + 0x20 * k, flags, nt as u64).map_err(|e| format!("the sky at {at:#x}: mesh {i} record {k}: {e}"))?.0);
        }
        sky.meshes.push(mesh);
    }
    Ok(sky)
}

/// The sky (core index word 4; func_L01_00422698) at `at` in `parent` (the unpacked core's root), `b` its bytes up to the next core offset.
pub fn sky(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<Sky, String> {
    let mut region = t.region(parent, SKY_G, at, b.len() as u64);
    match sky_plan(b) {
        Err(e) => {
            fail_region(t, region, SKY_G, "sky (not decoded: a value is not what the decoder requires)", SKY_D, "func_L01_00422698 (same in all 34 story levels)", b.len() as u64, e.clone())?;
            Err(e)
        }
        Ok((p, sky)) => {
            emit(t, &mut region, SKY_G, SKY_D, &SKY_LINES, &p);
            t.close(region);
            Ok(sky)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// The texture pools at words 0x19 and 0x1A

const P_CLUT: usize = 0;
const P_TEXEL: usize = 1;
const P_PAD: usize = 2;

const P19_G: &str = "LEVEL core region 0x19 (unpacked)";
const P19_D: &str = "coremisc::region_19";
const P19_LINES: [(&str, &str); 3] = [
    (
        "region 0x19 CLUTs (256 RGBA32 entries of 4 u8)",
        "func_L01_004224C8 (same in all 34 story levels) turns each word 0x14/0x15 record into the word pair at 0x220AC0 that addresses its CLUT (record +0) and its texels (record +8); the reader of the entries' bytes is not found",
    ),
    (
        "region 0x19 texels (8-bit, size x size)",
        "func_L01_004224C8 (same in all 34 story levels) addresses them through the record's +8 and +0xC (the size, a power of two); the reader of the texels is not found",
    ),
    ("zero padding", NOTHING),
];

const P1A_G: &str = "LEVEL core region 0x1A (unpacked)";
const P1A_D: &str = "coremisc::region_1a";
const P1A_LINES: [(&str, &str); 3] = [
    (
        "region 0x1A CLUTs (256 RGBA32 entries of 4 u8)",
        "func_L01_004225E8 (same in all 34 story levels) reads the word 0x16/0x17 records; func_L01_00401CD0 (same in all 34 story levels) addresses the CLUT at the region (0x1DFA04) plus the record's +0 and uploads it",
    ),
    (
        "region 0x1A texels (8-bit, width x height)",
        "func_L01_004225E8 (same in all 34 story levels) reads the word 0x16/0x17 records; func_L01_00401CD0 (same in all 34 story levels) addresses the texels at the region (0x1DFA04) plus the record's +4 and uploads them",
    ),
    ("zero padding (the region's last 0x40 bytes, core index word 0x28, and any other gap)", NOTHING),
];

fn pool_plan(b: &[u8], idx: &[u8], at: u64, is_1a: bool) -> Result<Plan, String> {
    let n = b.len() as u64;
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as u64);
    let (cnt, off) = if is_1a { (w(0x16)?, w(0x17)?) } else { (w(0x14)?, w(0x15)?) };
    let mut p = Plan::new(3);
    let mut blocks: BTreeSet<(u64, u64, bool)> = BTreeSet::new();
    for i in 0..cnt {
        let r = (off + 16 * i) as usize;
        let rec = [u32_at(idx, r)? as u64, u32_at(idx, r + 4)? as u64, u32_at(idx, r + 8)? as u64, u32_at(idx, r + 12)? as u64];
        if is_1a {
            let (wd, ht) = (rec[2], rec[3]);
            if rec[0] % 0x40 != 0 || rec[1] % 0x40 != 0 || !wd.is_power_of_two() || !ht.is_power_of_two() || wd > 0x7fff_ffff || ht > 0x7fff_ffff {
                return Err(format!("word 0x16/0x17 record {i}: offsets {:#x}, {:#x}, size {wd} x {ht}", rec[0], rec[1]));
            }
            blocks.insert((rec[0], 0x400, true));
            blocks.insert((rec[1], wd * ht, false));
        } else {
            if rec[1] != 0 || !rec[3].is_power_of_two() {
                return Err(format!("word 0x14/0x15 record {i}: +4 {:#x}, size {}", rec[1], rec[3]));
            }
            blocks.insert((rec[0], 0x400, true));
            blocks.insert((rec[2], rec[3] * rec[3], false));
        }
    }
    for &(a, l, clut) in &blocks {
        p.add(if clut { P_CLUT } else { P_TEXEL }, a, l, n, "texture block")?;
    }
    if is_1a {
        // Word 0x28 points at the region's last 0x40 bytes.
        if n < 0x40 || w(0x28)? != at + n - 0x40 {
            return Err(format!("index word 0x28 is {:#x}, expected the region's last 0x40 bytes at {:#x}", w(0x28)?, (at + n).wrapping_sub(0x40)));
        }
        p.add(P_PAD, n - 0x40, 0x40, n, "region end")?;
    }
    p.finish(b, P_PAD)?;
    Ok(p)
}

/// The effect textures of the region at core index word 0x19, indexed as the effect texture table (0x220AC0) and an effect entry's
/// byte +2 index them: texture i is the word 0x14/0x15 record i (func_L01_004224C8 zeroes the counter at gp+0x3A5C, then for each
/// record stores table entry [counter] = ((region RAM address + record +0) << 4) + record +4 (the CLUT: 256 PSMCT32 entries, the
/// PSM nibble is record +4, zero) and ((region RAM address + record +8) << 4) + log2 of record +0xC (the texels: 8-bit, size x
/// size, one level), and increments the counter). Pixels as the GS samples them: each texel the palette colour of its index, the
/// palette in the order a 16 x 16 CSM1 CLUT read leaves it (as `core::texture`), alpha 0x80 = 1.0. `b` is the region, `idx` the
/// core index.
pub fn effect_textures(b: &[u8], idx: &[u8]) -> Result<Vec<crate::core::Texture>, String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as usize);
    let (cnt, off) = (w(0x14)?, w(0x15)?);
    if cnt > 128 {
        return Err(format!("word 0x14: {cnt} effect textures, the table at 0x220AC0 holds 128"));
    }
    (0..cnt)
        .map(|i| {
            let r = off + 16 * i;
            let (clut, texels, size) = (u32_at(idx, r)? as usize, u32_at(idx, r + 8)? as usize, u32_at(idx, r + 12)? as usize);
            let pal = b.get(clut..clut + 0x400).ok_or_else(|| format!("effect texture {i}: CLUT {clut:#x} past the region"))?;
            let px = b.get(texels..texels + size * size).ok_or_else(|| format!("effect texture {i}: texels {texels:#x}+{:#x} past the region", size * size))?;
            let colour = |p: u8| {
                let p = p as usize;
                let j = (p & !0x18) | ((p & 8) << 1) | ((p & 0x10) >> 1);
                [pal[4 * j], pal[4 * j + 1], pal[4 * j + 2], pal[4 * j + 3]]
            };
            let clut: Vec<[u8; 4]> = (0..=255u8).map(colour).collect();
            Ok(crate::core::Texture { width: size, height: size, levels: vec![px.iter().map(|&p| colour(p)).collect()], gs: Some(crate::core::GsLevel { psm: 0x13, texels: px.to_vec(), clut }) })
        })
        .collect()
}

/// The region at core index word 0x19 (textures addressed by the word 0x14/0x15 records, func_L01_004224C8): returns them as
/// `effect_textures` does. `idx` is the core index.
pub fn region_19(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8], idx: &[u8]) -> Result<Vec<crate::core::Texture>, String> {
    let mut region = t.region(parent, P19_G, at, b.len() as u64);
    match pool_plan(b, idx, at, false).and_then(|p| effect_textures(b, idx).map(|x| (p, x))) {
        Err(e) => fail_region(t, region, P19_G, "region 0x19 (not decoded: a value is not what the decoder requires)", P19_D, "func_L01_004224C8 (same in all 34 story levels)", b.len() as u64, e),
        Ok((p, textures)) => {
            emit(t, &mut region, P19_G, P19_D, &P19_LINES, &p);
            t.close(region);
            Ok(textures)
        }
    }
}

/// A texture of the region at core index word 0x1A: its CLUT's and texels' offsets in the region (what an upload's source
/// address minus the region's RAM address is) and its pixels.
pub struct QuadTexture {
    pub clut: u32,
    pub texels: u32,
    pub texture: crate::core::Texture,
}

/// The textures of the region at core index word 0x1A, in record order: record i (word 0x16/0x17, 16 bytes: CLUT offset, texel
/// offset, width, height) is 8-bit texels, width x height, and a CLUT of 256 PSMCT32 entries; func_L01_00401CD0 uploads both
/// from the region's RAM address (0x1DFA04 on Veldin) plus the offsets, for the primitives the EE writes into the moby chain
/// itself (weapon fire, lights). Pixels as the GS samples them: each texel the palette colour of its index, the palette in the
/// order a 16 x 16 CSM1 CLUT read leaves it, alpha 0x80 = 1.0 (as `effect_textures`). `b` is the region, `idx` the core index.
pub fn quad_textures(b: &[u8], idx: &[u8]) -> Result<Vec<QuadTexture>, String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as usize);
    let (cnt, off) = (w(0x16)?, w(0x17)?);
    (0..cnt)
        .map(|i| {
            let r = off + 16 * i;
            let (clut, texels, wd, ht) = (u32_at(idx, r)? as usize, u32_at(idx, r + 4)? as usize, u32_at(idx, r + 8)? as usize, u32_at(idx, r + 12)? as usize);
            let pal = b.get(clut..clut + 0x400).ok_or_else(|| format!("region 0x1A texture {i}: CLUT {clut:#x} past the region"))?;
            let px = b.get(texels..texels + wd * ht).ok_or_else(|| format!("region 0x1A texture {i}: texels {texels:#x}+{:#x} past the region", wd * ht))?;
            let colour = |p: u8| {
                let p = p as usize;
                let j = (p & !0x18) | ((p & 8) << 1) | ((p & 0x10) >> 1);
                [pal[4 * j], pal[4 * j + 1], pal[4 * j + 2], pal[4 * j + 3]]
            };
            let gs = crate::core::GsLevel { psm: 0x13, texels: px.to_vec(), clut: (0..=255u8).map(colour).collect() };
            Ok(QuadTexture { clut: clut as u32, texels: texels as u32, texture: crate::core::Texture { width: wd, height: ht, levels: vec![px.iter().map(|&p| colour(p)).collect()], gs: Some(gs) } })
        })
        .collect()
}

/// The region at core index word 0x1A (textures addressed by the word 0x16/0x17 records, func_L01_004225E8): returns them as
/// `quad_textures` does. `idx` is the core index.
pub fn region_1a(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8], idx: &[u8]) -> Result<Vec<QuadTexture>, String> {
    let mut region = t.region(parent, P1A_G, at, b.len() as u64);
    match pool_plan(b, idx, at, true).and_then(|p| quad_textures(b, idx).map(|x| (p, x))) {
        Err(e) => fail_region(t, region, P1A_G, "region 0x1A (not decoded: a value is not what the decoder requires)", P1A_D, "func_L01_004225E8 (same in all 34 story levels)", b.len() as u64, e),
        Ok((p, textures)) => {
            emit(t, &mut region, P1A_G, P1A_D, &P1A_LINES, &p);
            t.close(region);
            Ok(textures)
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// The regions at words 0x29, 0x2A, 0x2C, 0x2D and 0x2E

const R_G: &str = "LEVEL core regions 0x29-0x2E (unpacked)";

/// The bit-set regions: `sets` entries of 0x80 bytes after the 16-byte header, then 0x30 zero bytes.
fn bitsets(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8], sets: u64, decoder: &'static str, head: &'static str, set: &'static str, pad: &'static str, game: &'static str) -> Result<(), String> {
    let n = b.len() as u64;
    let mut region = t.region(parent, R_G, at, n);
    let r = (|| -> Result<(), String> {
        if n != 0x10 + 0x80 * sets + 0x30 {
            return Err(format!("region of {n:#x} bytes, expected {:#x} (a 0x10-byte header, {sets} bit sets of 0x80 bytes, 0x30 zero bytes)", 0x10 + 0x80 * sets + 0x30));
        }
        if u32_at(b, 12)? != 0 {
            return Err(bad("header +0xC", 12, u32_at(b, 12)? as u64, "zero"));
        }
        if let Some(k) = b[(0x10 + 0x80 * sets) as usize..].iter().position(|&v| v != 0) {
            return Err(bad("byte after the bit sets", 0x10 + 0x80 * sets + k as u64, b[(0x10 + 0x80 * sets) as usize + k] as u64, "zero"));
        }
        Ok(())
    })();
    if let Err(e) = r {
        return fail_region(t, region, R_G, "region (not decoded: a value is not what the decoder requires)", decoder, game, n, e);
    }
    let mut plan = Plan::new(3);
    plan.r[0].push((0, 0xc));
    plan.r[0].push((0xc, 4));
    plan.r[1].push((0x10, 0x80 * sets));
    plan.r[2].push((0x10 + 0x80 * sets, 0x30));
    let lines = [(head, game), (set, game), (pad, NOTHING)];
    emit(t, &mut region, R_G, decoder, &lines, &plan);
    t.close(region);
    Ok(())
}

const BITSET_GAME: &str = "func_L01_003FEE38 (same in all 34 story levels) reads the f32 x, y, z of the header and copies the picked 0x80-byte set to 0x1EA8C0 (func_L01_0040BD58); +0xC is read by nothing and zero";

/// The region at core index word 0x2A: a split point and 8 bit sets (func_L01_003FEE38 case 2).
pub fn region_2a(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    bitsets(
        t,
        parent,
        at,
        b,
        8,
        "coremisc::region_2a",
        "region 0x2A header (f32 x, y, z of the split point, u32 zero)",
        "region 0x2A bit sets (8 x 0x80 bytes, picked by the camera's side of the split point on x, y and z)",
        "region 0x2A zero padding (the last 0x30 bytes)",
        BITSET_GAME,
    )
}

/// The region at core index word 0x2C: a split point and 0x800 bit sets (func_L01_003FEE38 case 3).
pub fn region_2c(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    bitsets(
        t,
        parent,
        at,
        b,
        0x800,
        "coremisc::region_2c",
        "region 0x2C header (f32 x, y, z of the split point, u32 zero)",
        "region 0x2C bit sets (0x800 x 0x80 bytes, picked by the camera's direction from the split point)",
        "region 0x2C zero padding (the last 0x30 bytes)",
        BITSET_GAME,
    )
}

/// The region at core index word 0x2E: a split point and 0x800 bit sets (func_L01_003FEE38 case 4).
pub fn region_2e(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    bitsets(
        t,
        parent,
        at,
        b,
        0x800,
        "coremisc::region_2e",
        "region 0x2E header (f32 x, y, z of the split point, u32 zero)",
        "region 0x2E bit sets (0x800 x 0x80 bytes, picked by the camera's direction from the split point)",
        "region 0x2E zero padding (the last 0x30 bytes)",
        BITSET_GAME,
    )
}

/// The region at core index word 0x29 (func_L01_00422470): four 16-byte entries whose first 8 bytes it loads to 0x1DFF40,
/// 0x1DFF50, 0x1DFF60 and 0x1DFF70, then data from +0x40 (0x1DFF80). No level has it (word 0x29 is zero in all 34), so no
/// instance exists on the disc; the bytes the loader does not read are left unread.
pub fn region_29(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let n = b.len() as u64;
    let mut region = t.region(parent, R_G, at, n);
    if n < 0x40 {
        return fail_region(t, region, R_G, "region 0x29 (not decoded: shorter than its 0x40-byte header)", "coremisc::region_29", "func_L01_00422470", n, format!("region 0x29 of {n:#x} bytes"));
    }
    let l = t.line(R_G, "region 0x29 header (4 x: u64 loaded to 0x1DFF40.., u64 not read)", "coremisc::region_29", "func_L01_00422470 (same in all 34 story levels) loads the u64 at +0, +0x10, +0x20 and +0x30", Space::Unpacked);
    let mut h = t.child(&mut region, l, 0, 0x40);
    for k in 0..4 {
        h.mark(0x10 * k, 8);
    }
    t.close(h);
    let l = t.line(R_G, "region 0x29 data (from +0x40; stored at 0x1DFF80)", "", "", Space::Unpacked);
    let d = t.child(&mut region, l, 0x40, n - 0x40);
    t.close(d);
    t.close(region);
    Ok(())
}

/// The region at core index word 0x2D (func_L01_00428750, func_L01_003EDBF8): u32 entry count, u32 data base, `count` 16-byte
/// entries (u32 packed dimensions: x, y and z in 10 bits each from bit 0, 10 and 20; u32 data offset), then the data: x * y * z
/// bytes (func_L01_003EDBF8 samples them as a voxel volume: a byte at x + y * dx + z * dx * dy style index, interpolated), then
/// zero bytes. The one level that has it (L02) has one entry (128 x 128 x 128) whose data starts at +0x10, inside the entry
/// (its last two words are the data's first bytes).
pub fn region_2d(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let n = b.len() as u64;
    let mut region = t.region(parent, R_G, at, n);
    let r = (|| -> Result<u64, String> {
        let (count, base, dims, w1) = (u32_at(b, 0)?, u32_at(b, 4)?, u32_at(b, 8)?, u32_at(b, 12)?);
        if count != 1 || base != 0x10 || w1 != 0 || n < 0x18 {
            return Err(format!("region 0x2D: count {count}, data base {base:#x}, entry data offset {w1:#x}: expected 1, 0x10 and 0"));
        }
        let (x, y, z) = ((dims & 0x3ff) as u64, ((dims >> 10) & 0x3ff) as u64, ((dims >> 20) & 0x3ff) as u64);
        let len = x * y * z;
        if len == 0 || 0x10 + len > n {
            return Err(format!("region 0x2D: dimensions {x} x {y} x {z} ({dims:#x}) past the region's {n:#x} bytes"));
        }
        if let Some(k) = b[(0x10 + len) as usize..].iter().position(|&v| v != 0) {
            return Err(bad("byte after the volume", 0x10 + len + k as u64, b[(0x10 + len) as usize + k] as u64, "zero"));
        }
        Ok(len)
    })();
    let len = match r {
        Ok(l) => l,
        Err(e) => return fail_region(t, region, R_G, "region 0x2D (not decoded: a value is not what the decoder requires)", "coremisc::region_2d", LOADER, n, e),
    };
    let l = t.line(
        R_G,
        "region 0x2D header and entry (u32 entry count, u32 data base, u32 packed dimensions, u32 data offset)",
        "coremisc::region_2d",
        "func_L01_00428750 (same in all 34 story levels) reads the count and the base and relocates each entry's offset (+4 of the entry) to region + offset + base; func_L01_003EDBF8 (same in all 34 story levels) reads the entry's first u32 (10-bit x, y and z) and the data pointer",
        Space::Unpacked,
    );
    let mut h = t.child(&mut region, l, 0, 0x10);
    h.mark(0, 0x10);
    t.close(h);
    let l = t.line(R_G, "region 0x2D volume (x * y * z bytes from +0x10)", "coremisc::region_2d", "func_L01_003EDBF8 (same in all 34 story levels) reads the bytes (lbu) at the indices its trilinear lookup computes from the entry's dimensions", Space::Unpacked);
    let mut d = t.child(&mut region, l, 0x10, len);
    d.mark(0, len);
    t.close(d);
    if 0x10 + len < n {
        let l = t.line(R_G, "region 0x2D zero padding after the volume", "coremisc::region_2d", LOADER, Space::Unpacked);
        let mut p = t.child(&mut region, l, 0x10 + len, n - 0x10 - len);
        let pl = p.size;
        p.mark(0, pl);
        t.close(p);
    }
    t.close(region);
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------------
// Index tables

const GI: &str = "LEVEL core index";
const ID: &str = "coremisc::index_tables";

fn fail_table(t: &mut Tracker, region: &mut Node, at: u64, len: u64, name: &str, game: &'static str, e: String) -> String {
    let l = t.line(GI, &format!("{name} (not decoded: a value is not what the decoder requires)"), ID, game, Space::Disc);
    let mut n = t.child(region, l, at, len);
    n.fail(e.clone());
    t.close(n);
    e
}

/// A table whose records all get one line: `check` validates record `i` (at `rec` bytes of the table), then every byte is marked.
fn records(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], off: u64, count: u64, size: u64, name: &str, game: &'static str, check: &dyn Fn(u64, &[u8]) -> Result<(), String>) -> Result<(), String> {
    let len = count * size;
    if off + len > idx.len() as u64 {
        return Err(fail_table(t, region, at + off.min(idx.len() as u64), 0, name, game, format!("{count} records at {off:#x} past the index's {:#x} bytes", idx.len())));
    }
    let tab = &idx[off as usize..(off + len) as usize];
    let mut err = None;
    for i in 0..count {
        if let Err(e) = check(i, &tab[(i * size) as usize..((i + 1) * size) as usize]) {
            err = Some(e);
            break;
        }
    }
    if let Some(e) = err {
        return Err(fail_table(t, region, at + off, len, name, game, e));
    }
    let l = t.line(GI, name, ID, game, Space::Disc);
    let mut n = t.child(region, l, at + off, len);
    n.mark(0, len);
    t.close(n);
    Ok(())
}

/// The core offsets the index gives (as core.rs's `index` collects them), sorted.
fn core_offsets(idx: &[u8]) -> Result<Vec<u64>, String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as u64);
    let mut v = Vec::new();
    for i in [2usize, 3, 4, 5, 0x18, 0x19, 0x1a, 0x29, 0x2a, 0x2c, 0x2d, 0x2e] {
        if w(i)? != 0 || i == 2 {
            v.push(w(i)?);
        }
    }
    for (cw, ow, size) in [(6usize, 7usize, 32u64), (8, 9, 32), (10, 11, 48)] {
        for r in 0..w(cw)? {
            let c = u32_at(idx, (w(ow)? + r * size) as usize)? as u64;
            if c != 0 {
                v.push(c);
            }
        }
    }
    v.sort_unstable();
    v.dedup();
    Ok(v)
}

/// The index header words the loader does not read, checked against what they are on every level: word 0x22 is the core LZ
/// stream's size (`lz_size`, the part table's size of part 9), 0x23 the core's unpacked size (`unpacked`), 0x28 the core offset
/// of the last 0x40 bytes of the region at word 0x1A (which end where the next core offset starts), 0x2F zero, 0x1F the lowest
/// offset of the word 0x1E table. Word 0x1D is not settled (it has no reader and no relation to the data was found) and stays
/// unread. Marks the words on `h`, the index header line's node (offsets in the index).
pub fn header_words(h: &mut Node, idx: &[u8], lz_size: u64, unpacked: u64) -> Result<(), String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as u64);
    if w(0x22)? != lz_size {
        return Err(bad("index word 0x22", 0x88, w(0x22)?, &format!("the core LZ stream's size {lz_size:#x}")));
    }
    if w(0x23)? != unpacked {
        return Err(bad("index word 0x23", 0x8c, w(0x23)?, &format!("the core's unpacked size {unpacked:#x}")));
    }
    let offs = core_offsets(idx)?;
    let a = w(0x1a)?;
    let next = offs.iter().copied().find(|&o| o > a).ok_or("no core offset after the region at word 0x1A")?;
    if w(0x28)? + 0x40 != next {
        return Err(bad("index word 0x28", 0xa0, w(0x28)?, &format!("the region at word 0x1A's end {next:#x} minus 0x40")));
    }
    if w(0x2f)? != 0 {
        return Err(bad("index word 0x2F", 0xbc, w(0x2f)?, "zero"));
    }
    let e = w(0x1e)?;
    let low = (0..127u64).map(|k| u32_at(idx, (e + 4 * k) as usize).map(|v| v as u64)).collect::<Result<Vec<_>, _>>()?.into_iter().filter(|&v| v != 0).min();
    if low != Some(w(0x1f)?) {
        return Err(bad("index word 0x1F", 0x7c, w(0x1f)?, &format!("the lowest offset in the word 0x1E table, {low:x?}")));
    }
    for i in [0x22u64, 0x23, 0x28, 0x2f, 0x1f] {
        h.mark(4 * i, 4);
    }
    // Word 0x1D: func_L01_00428750 and the code that keeps the index never read it (no xref found in boot_elf, the 34 overlays or the
    // front ends), and it relates to nothing: skipped.
    h.skip(4 * 0x1d, 4);
    Ok(())
}

/// The core index's tables that core.rs leaves undecoded, at `at` in `region` (the level data part), `idx` their bytes: the
/// texture records at words 0x14/0x15 and 0x16/0x17, the GS RAM table (words 0, 1 and 0x21), and the tables at words 0x1B, 0x1C,
/// 0x2B and 0x1E. Returns the first error (each failing table is also a failed line of exact extent).
pub fn index_tables(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8]) -> Result<(), String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as u64);
    let mut first: Option<String> = None;
    let mut keep = |r: Result<(), String>| {
        if let Err(e) = r {
            first.get_or_insert(e);
        }
    };
    keep(tex14(t, region, at, idx, w(0x14)?, w(0x15)?));
    keep(tex16(t, region, at, idx, w(0x16)?, w(0x17)?));
    keep(gs_table(t, region, at, idx, w(0)?, w(0x21)?, w(1)?));
    keep(table_1b(t, region, at, idx, w(0x1b)?, w(0x1c)?));
    keep(table_1c(t, region, at, idx, w(0x1c)?, w(0x2b)?));
    keep(table_2b(t, region, at, idx, w(0x2b)?, w(0x1e)?));
    keep(table_1e(t, region, at, idx, w(0x1e)?));
    match first {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

fn tex14(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], n: u64, off: u64) -> Result<(), String> {
    records(
        t,
        region,
        at,
        idx,
        off,
        n,
        16,
        "texture records, words 0x14/0x15 (16 bytes: u32 CLUT offset, u32 zero, u32 texel offset, u32 size)",
        "func_L01_004224C8 (same in all 34 story levels) reads all four words: (region 0x19's address + +0) * 16 + +4, and (region 0x19's address + +8) * 16 + (0x1E - plzcw of +0xC) into the pair at 0x220AC0; the offsets are into the region at word 0x19, +0xC is the width and the height of the texels",
        &|i, r| {
            let (w1, d) = (u32_at(r, 4)?, u32_at(r, 12)?);
            if w1 != 0 || !d.is_power_of_two() {
                return Err(format!("record {i}: +4 {w1:#x}, size {d}"));
            }
            Ok(())
        },
    )
}

fn tex16(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], n: u64, off: u64) -> Result<(), String> {
    records(
        t,
        region,
        at,
        idx,
        off,
        n,
        16,
        "texture records, words 0x16/0x17 (16 bytes: u32 CLUT offset, u32 texel offset, s32 width, s32 height)",
        "func_L01_004225E8 (same in all 34 story levels) reads all four words: +0 and +4 divided by 64 into the entry's +0xA and +8, the sign of +8 (negative: format 0x14, else 0x13), and the log2 of the absolute values of +8 and +0xC (plzcw) into +0xC and +0xD; the offsets are into the region at word 0x1A",
        &|i, r| {
            let (a, b, wd, ht) = (u32_at(r, 0)?, u32_at(r, 4)?, u32_at(r, 8)? as i32, u32_at(r, 12)? as i32);
            if a % 0x40 != 0 || b % 0x40 != 0 || wd <= 0 || ht <= 0 || !(wd as u32).is_power_of_two() || !(ht as u32).is_power_of_two() {
                return Err(format!("record {i}: offsets {a:#x} and {b:#x}, size {wd} x {ht}"));
            }
            Ok(())
        },
    )
}

fn gs_table(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], n1: u64, n2: u64, off: u64) -> Result<(), String> {
    let len = 16 * (n1 + n2);
    let name1 = "GS RAM table, entries counted by word 0 (16 bytes: u32 format 0 or 0x13, u16 width, s16 height, u32 equal to +0xC, u32 offset in the GS RAM image part)";
    let game1: &'static str = "func_L01_00423B48 (same in all 34 story levels) reads +0 (the format), +4 (width and height) and +0xC (the offset); +8 is read by nothing and equals +0xC";
    let name2 = "GS RAM table, entries counted by word 0x21 (16 bytes: u32 0x13, u16 width, s16 height, u32 offset in the GS RAM image part, u32 offset from the first of these entries)";
    let game2: &'static str = "func_L01_00423B48 (same in all 34 story levels) reads +4 (width and height) and +8 (the offset) of the entries after the first word-0 entries; +0 is read by nothing and is 0x13, +0xC is read by nothing and equals +8 minus the first of these entries' +8";
    if off + len > idx.len() as u64 {
        return Err(fail_table(t, region, at + off.min(idx.len() as u64), 0, name1, game1, format!("GS RAM table of {len:#x} bytes at {off:#x} past the index's {:#x} bytes", idx.len())));
    }
    let tab = &idx[off as usize..(off + len) as usize];
    let rec = |i: u64| -> Result<[u64; 4], String> { Ok([u32_at(tab, (16 * i) as usize)? as u64, u32_at(tab, (16 * i) as usize + 4)? as u64, u32_at(tab, (16 * i) as usize + 8)? as u64, u32_at(tab, (16 * i) as usize + 12)? as u64]) };
    let mut res = Ok(());
    for i in 0..n1 {
        let r = rec(i)?;
        if !(r[0] == 0 || r[0] == 0x13) || r[2] != r[3] {
            res = Err(format!("GS RAM table entry {i}: format {:#x}, +8 {:#x}, +0xC {:#x}", r[0], r[2], r[3]));
            break;
        }
    }
    if res.is_ok() && n2 > 0 {
        let base = rec(n1)?[2];
        for i in n1..n1 + n2 {
            let r = rec(i)?;
            if r[0] != 0x13 || r[3] != r[2].wrapping_sub(base) {
                res = Err(format!("GS RAM table entry {i}: +0 {:#x}, +8 {:#x}, +0xC {:#x} (first +8 {base:#x})", r[0], r[2], r[3]));
                break;
            }
        }
    }
    if let Err(e) = res {
        return Err(fail_table(t, region, at + off, len, name1, game1, e));
    }
    let l = t.line(GI, name1, ID, game1, Space::Disc);
    let mut n = t.child(region, l, at + off, 16 * n1);
    n.mark(0, 16 * n1);
    t.close(n);
    if n2 > 0 {
        let l = t.line(GI, name2, ID, game2, Space::Disc);
        let mut n = t.child(region, l, at + off + 16 * n1, 16 * n2);
        n.mark(0, 16 * n2);
        t.close(n);
    }
    Ok(())
}

/// Lines of a table made of several structures: `ranges` per line (offsets in the index), zero bytes between them checked.
fn multi(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], off: u64, len: u64, lines: &[(&str, &'static str)], ranges: &[Vec<(u64, u64)>], pad: usize) -> Result<(), String> {
    let mut all: Vec<(u64, u64)> = ranges.iter().flatten().copied().collect();
    all.sort_unstable();
    let mut cur = off;
    let mut gaps = Vec::new();
    for &(a, l) in &all {
        if a < cur {
            return Err(format!("table structures overlap at {a:#x}"));
        }
        if a > cur {
            gaps.push((cur, a - cur));
        }
        cur = a + l;
    }
    if cur > off + len {
        return Err(format!("table structures run to {cur:#x}, past the table's end {:#x}", off + len));
    }
    if cur < off + len {
        gaps.push((cur, off + len - cur));
    }
    for &(a, l) in &gaps {
        if let Some(k) = idx[a as usize..(a + l) as usize].iter().position(|&v| v != 0) {
            return Err(bad("index byte between structures", a + k as u64, idx[a as usize + k] as u64, "zero"));
        }
    }
    let mut ranges: Vec<Vec<(u64, u64)>> = ranges.to_vec();
    ranges[pad].extend(gaps);
    for (k, (name, game)) in lines.iter().enumerate() {
        if ranges[k].is_empty() {
            continue;
        }
        ranges[k].sort_unstable();
        let l = t.line(GI, name, ID, game, Space::Disc);
        let rs: Vec<(u64, u64)> = ranges[k].iter().map(|&(a, n)| (at + a, n)).collect();
        let mut n = t.pieces(region, l, &rs);
        let total = n.size;
        n.mark(0, total);
        t.close(n);
    }
    Ok(())
}

fn table_1b(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], off: u64, end: u64) -> Result<(), String> {
    let len = end.wrapping_sub(off);
    let lines: [(&str, &'static str); 4] = [
        (
            "table at word 0x1B, header (u32 entry count, u32 texture count, u32 offset of the byte lists, u32 size of the byte lists)",
            "func_L01_004224C8 (same in all 34 story levels) reads the count, the lists' offset and size; +4 is read by nothing and equals word 0x14 (the texture record count)",
        ),
        (
            "table at word 0x1B, entries (u32 offset of a byte list in the table, 0 for none)",
            "func_L01_004224C8 (same in all 34 story levels) turns each into the pointer at 0x221118 + 4 * entry number (0x221370 for none)",
        ),
        (
            "table at word 0x1B, byte lists (u8 texture numbers below the texture count)",
            "func_L01_004224C8 (same in all 34 story levels) copies them to 0x221370; effect code reads them through the pointers at 0x221118 (e.g. func_L01_002FAAC8)",
        ),
        ("zero padding (entries to the byte lists)", NOTHING),
    ];
    let r = (|| -> Result<Vec<Vec<(u64, u64)>>, String> {
        if end < off || end > idx.len() as u64 {
            return Err(format!("table at word 0x1B {off:#x} to {end:#x}"));
        }
        let tab = &idx[off as usize..end as usize];
        let (n, x, lo, ls) = (u32_at(tab, 0)? as u64, u32_at(tab, 4)? as u64, u32_at(tab, 8)? as u64, u32_at(tab, 12)? as u64);
        let w14 = u32_at(idx, 4 * 0x14)? as u64;
        if x != w14 || lo != (16 + 4 * n + 15) & !15 || lo + ls != len {
            return Err(format!("header: entries {n}, +4 {x} (expected {w14}), lists at {lo:#x} size {ls:#x} (table {len:#x} bytes)"));
        }
        for i in 0..n {
            let e = u32_at(tab, (16 + 4 * i) as usize)? as u64;
            if e != 0 && !(lo <= e && e < lo + ls) {
                return Err(format!("entry {i}: {e:#x} is not inside the byte lists {lo:#x}..{:#x}", lo + ls));
            }
        }
        if let Some(k) = tab[lo as usize..].iter().position(|&v| v as u64 >= x) {
            return Err(format!("byte list byte {k:#x}: {:#x} is not below the texture count {x}", tab[lo as usize + k]));
        }
        Ok(vec![vec![(off, 16)], vec![(off + 16, 4 * n)], vec![(off + lo, ls)], Vec::new()])
    })();
    match r {
        Err(e) => Err(fail_table(t, region, at + off, len, "table at word 0x1B", lines[0].1, e)),
        Ok(ranges) => multi(t, region, at, idx, off, len, &lines, &ranges, 3).map_err(|e| fail_table(t, region, at + off, len, "table at word 0x1B", lines[0].1, e)),
    }
}

fn table_1c(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], off: u64, end: u64) -> Result<(), String> {
    let len = end.wrapping_sub(off);
    let lines: [(&str, &'static str); 6] = [
        (
            "table at word 0x1C, header (4 x s16: offset of the records, record count, offset of the remap entries, remap entry count)",
            "func_L01_00428750 (same in all 34 story levels) reads all four",
        ),
        (
            "table at word 0x1C, class entries, one per moby class (u16 offset of the class's list, u16 length of that list in u32s)",
            "func_L01_00428750 (same in all 34 story levels) reads each offset (as s16); the length is read by nothing and equals the list's size (the lists tile the table)",
        ),
        (
            "table at word 0x1C, records (0x20 bytes: f32, f32, s32, s32, s32, s32, u8, u8, s16 remap index, u32 zero)",
            "func_L01_00428750 (same in all 34 story levels) copies them to 0x1DFBEC, replaces the s16 at +0x1A by the remap entry (when below the remap entry count, else 0xFFFF) and zeroes +0x1C (zero on the disc); func_L01_003ECAC8 and func_L01_003EE398 read +0, +4, +0xC and +0x18; func_L01_003EC940 reads +0x18; func_L01_004A6F58 reads +0x10, +0x14, +0x18 and +0x1A and passes +4; func_L01_004A53D0 reads +8, +0xC and +0x19 (bit 0)",
        ),
        (
            "table at word 0x1C, remap entries (s32 0 to 0x7FFF)",
            "func_L01_00428750 (same in all 34 story levels) reads the low s16 of the entry a record's +0x1A names",
        ),
        (
            "table at word 0x1C, class lists (u32 values)",
            "func_L01_00428750 (same in all 34 story levels): the first u32 of a class's list plus 1 goes to each of the class's parts' +0x1C, the following u32s' low s16 to the parts' +0x1A",
        ),
        ("zero padding (lists to the next table)", NOTHING),
    ];
    let r = (|| -> Result<Vec<Vec<(u64, u64)>>, String> {
        if end < off || end > idx.len() as u64 {
            return Err(format!("table at word 0x1C {off:#x} to {end:#x}"));
        }
        let nclass = u32_at(idx, 4 * 6)? as u64;
        let tab = &idx[off as usize..end as usize];
        let h = [s16_at(tab, 0)?, s16_at(tab, 2)?, s16_at(tab, 4)?, s16_at(tab, 6)?];
        if h.iter().any(|&v| v < 0) {
            return Err(format!("header {h:?}"));
        }
        let (rec, nrec, remap, nremap) = (h[0] as u64, h[1] as u64, h[2] as u64, h[3] as u64);
        let lists = remap + 4 * nremap;
        if rec != 8 + 4 * nclass || rec + 32 * nrec != remap || lists > len {
            return Err(format!("header {h:?}: {nclass} classes, table {len:#x} bytes"));
        }
        for i in 0..nrec {
            if u32_at(tab, (rec + 32 * i + 0x1c) as usize)? != 0 {
                return Err(format!("record {i} +0x1C: {:#x}, expected zero", u32_at(tab, (rec + 32 * i + 0x1c) as usize)?));
            }
        }
        for i in 0..nremap {
            let v = u32_at(tab, (remap + 4 * i) as usize)?;
            if v > 0x7fff {
                return Err(format!("remap entry {i}: {v:#x}, expected 0 to 0x7FFF"));
            }
        }
        // The class lists tile the rest of the table up to a zero padding.
        let mut starts: Vec<(u64, u64)> = Vec::new();
        let mut empties: Vec<u64> = Vec::new();
        for c in 0..nclass {
            let (o, l) = (u16_at(tab, (8 + 4 * c) as usize)? as u64, u16_at(tab, (8 + 4 * c) as usize + 2)? as u64);
            if l == 0 {
                empties.push(o);
            } else {
                starts.push((o, l));
            }
        }
        starts.sort_unstable();
        starts.dedup();
        let mut cur = lists;
        for &(o, l) in &starts {
            if o != cur {
                return Err(format!("class list at {o:#x}: expected {cur:#x} (the lists tile the table)"));
            }
            cur = o + 4 * l;
        }
        if starts.windows(2).any(|p| p[0].0 == p[1].0) {
            return Err("two class entries give one list two lengths".to_string());
        }
        if cur > len {
            return Err(format!("class lists end at {cur:#x}, past the table's {len:#x} bytes"));
        }
        for &o in &empties {
            if o != cur && !starts.iter().any(|s| s.0 == o) {
                return Err(format!("empty class list at {o:#x}: not at a list start"));
            }
        }
        Ok(vec![
            vec![(off, 8)],
            vec![(off + 8, 4 * nclass)],
            vec![(off + rec, 32 * nrec)],
            vec![(off + remap, 4 * nremap)],
            vec![(off + lists, cur - lists)],
            Vec::new(),
        ])
    })();
    match r {
        Err(e) => Err(fail_table(t, region, at + off, len, "table at word 0x1C", lines[0].1, e)),
        Ok(ranges) => multi(t, region, at, idx, off, len, &lines, &ranges, 5).map_err(|e| fail_table(t, region, at + off, len, "table at word 0x1C", lines[0].1, e)),
    }
}

fn table_2b(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], off: u64, end: u64) -> Result<(), String> {
    let len = end.wrapping_sub(off);
    let lines: [(&str, &'static str); 2] = [
        ("table at word 0x2B (s16 moby class ids ended by a negative s16)", "func_L01_00428750 (same in all 34 story levels) reads s16 values up to the first negative one"),
        ("zero padding (list to the next table)", NOTHING),
    ];
    let r = (|| -> Result<Vec<Vec<(u64, u64)>>, String> {
        if end < off || end > idx.len() as u64 {
            return Err(format!("table at word 0x2B {off:#x} to {end:#x}"));
        }
        let tab = &idx[off as usize..end as usize];
        let mut p = 0usize;
        loop {
            let v = s16_at(tab, p)?;
            p += 2;
            if v < 0 {
                break;
            }
        }
        Ok(vec![vec![(off, p as u64)], Vec::new()])
    })();
    match r {
        Err(e) => Err(fail_table(t, region, at + off, len, "table at word 0x2B", lines[0].1, e)),
        Ok(ranges) => multi(t, region, at, idx, off, len, &lines, &ranges, 1).map_err(|e| fail_table(t, region, at + off, len, "table at word 0x2B", lines[0].1, e)),
    }
}

fn table_1e(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], off: u64) -> Result<(), String> {
    let len = idx.len() as u64 - off;
    let lines: [(&str, &'static str); 2] = [
        (
            "table at word 0x1E (u32 core offsets, 0 for none; the first 127 entries)",
            "func_L01_00428750 (same in all 34 story levels) reads one entry per byte at +0xC of the class at 0x1FD200 (127 on every level), adds the core's address to each non-zero one and relocates the data at it (func_L01_00423DB8)",
        ),
        ("table at word 0x1E, entries 127 to 255 (u32 zero)", NOTHING),
    ];
    let r = (|| -> Result<Vec<Vec<(u64, u64)>>, String> {
        if len != 0x400 {
            return Err(format!("table of {len:#x} bytes, expected 0x400 (256 entries)"));
        }
        let unpacked = u32_at(idx, 4 * 0x23)? as u64;
        for i in 0..127u64 {
            let v = u32_at(idx, (off + 4 * i) as usize)? as u64;
            if v >= unpacked {
                return Err(format!("entry {i}: core offset {v:#x} past the core's {unpacked:#x} bytes"));
            }
        }
        for i in 127..256u64 {
            let v = u32_at(idx, (off + 4 * i) as usize)?;
            if v != 0 {
                return Err(format!("entry {i}: {v:#x}, expected zero (only 127 entries are read)"));
            }
        }
        Ok(vec![vec![(off, 4 * 127)], vec![(off + 4 * 127, 4 * 129)]])
    })();
    match r {
        Err(e) => Err(fail_table(t, region, at + off, len, "table at word 0x1E", lines[0].1, e)),
        Ok(ranges) => multi(t, region, at, idx, off, len, &lines, &ranges, 1).map_err(|e| fail_table(t, region, at + off, len, "table at word 0x1E", lines[0].1, e)),
    }
}

// ---------------------------------------------------------------------------------------------------------------------------
// Class record tables (index words 6/7, 8/9 and 10/11)

/// One class record table: `count` records of `size` bytes at `off` in the index. `word_ok` validates a record (its bytes), then
/// every byte the game reads is marked and the words nothing reads (`unread`: byte ranges inside a record) are skipped.
fn class_table(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8], off: u64, count: u64, size: u64, name: &str, game: &'static str, unread: &[(u64, u64)], mode_max: Option<u32>) -> Result<(), String> {
    let len = count * size;
    if off + len > idx.len() as u64 {
        return Err(fail_table(t, region, at + off.min(idx.len() as u64), 0, name, game, format!("{count} records at {off:#x} past the index's {:#x} bytes", idx.len())));
    }
    let core_size = u32_at(idx, 4 * 0x23)? as u64;
    let mut err = None;
    for i in 0..count {
        let r = &idx[(off + i * size) as usize..(off + (i + 1) * size) as usize];
        let (core, id) = (u32_at(r, 0)? as u64, u32_at(r, 4)? as u64);
        if core >= core_size {
            err = Some(bad(&format!("record {i} core offset"), off + i * size, core, &format!("below the core's {core_size:#x} bytes")));
        } else if id > 0xffff {
            err = Some(bad(&format!("record {i} class id"), off + i * size + 4, id, "at most 0xFFFF (stored as a halfword)"));
        } else if let Some(m) = mode_max {
            let mode = u32_at(r, 8)?;
            if mode > m {
                err = Some(bad(&format!("record {i} mode"), off + i * size + 8, mode as u64, &format!("at most {m}")));
            }
        }
        for &(a, l) in unread {
            if r[a as usize..(a + l) as usize].iter().any(|&v| v != 0) {
                err = Some(bad(&format!("record {i} word at +{a:#x}"), off + i * size + a, u32_at(r, a as usize)? as u64, "zero (read by nothing)"));
            }
        }
        if err.is_some() {
            break;
        }
    }
    if let Some(e) = err {
        return Err(fail_table(t, region, at + off, len, name, game, e));
    }
    let l = t.line(GI, name, "coremisc::class_records", game, Space::Disc);
    let mut n = t.child(region, l, at + off, len);
    for i in 0..count {
        let base = i * size;
        let mut p = 0;
        for &(a, l) in unread {
            if a > p {
                n.mark(base + p, a - p);
            }
            n.skip(base + a, l);
            p = a + l;
        }
        if size > p {
            n.mark(base + p, size - p);
        }
    }
    t.close(n);
    Ok(())
}

/// The core index's three class record tables, at `at` in `region` (the level data part), `idx` their bytes: the moby class
/// records (words 6/7), the tie class records (words 8/9) and the shrub class records (words 10/11). Returns the first error (each
/// failing table is also a failed line of exact extent).
pub fn class_records(t: &mut Tracker, region: &mut Node, at: u64, idx: &[u8]) -> Result<(), String> {
    let w = |i: usize| u32_at(idx, 4 * i).map(|v| v as u64);
    let mut first = Ok(());
    let mut keep = |r: Result<(), String>| {
        if first.is_ok() {
            first = r;
        }
    };
    let (n, o) = (w(6)?, w(7)?);
    if n != 0 {
        keep(class_table(
            t, region, at, idx, o, n, 32,
            "moby class records (32 bytes: u32 core offset, u32 class id, u32 mode, u32 zero, 16 texture index bytes)",
            "func_L01_00428750 (same in all 34 story levels) reads +0 (the class data's core offset, 0 for none), +4 (the class id, passed to func_L01_00424368 which stores it as a halfword), +8 (0, 1 or other: selects which 64-bit constant func_L01_00424368 is given, 0x23C580+0x38 or the word at 0x1D4BA8, or none for 0) and passes +0x10 on (func_L01_00424048, func_L01_00423E10 read its 16 bytes as texture indices by lbu); func_L01_00425170 reads +0 and +4; +0xC: no reader (zero in every level)",
            &[(0xc, 4)],
            Some(2),
        ));
    }
    let (n, o) = (w(8)?, w(9)?);
    if n != 0 {
        keep(class_table(
            t, region, at, idx, o, n, 32,
            "tie class records (32 bytes: u32 core offset, u32 class id, 8 bytes zero, 16 texture index bytes)",
            "func_L01_00428750 (same in all 34 story levels) passes +0 (the class data), +4 (the class id) and +0x10 to func_L01_00424478, which maps the id (func_L01_004252F0 maps tie class ids through 0x23C580) and gives the 16 texture index bytes to func_L01_00423E10 (lbu); +8 and +0xC: no reader (zero in every level)",
            &[(8, 8)],
            None,
        ));
    }
    let (n, o) = (w(10)?, w(11)?);
    if n != 0 {
        keep(class_table(
            t, region, at, idx, o, n, 48,
            "shrub class records (48 bytes: u32 core offset, u32 class id, 8 bytes zero, 16 texture index bytes, 8 s16)",
            "func_L01_00428750 (same in all 34 story levels) passes +0 (the class data), +4 (the class id), +0x10 and +0x20 to func_L01_00424850, which reads the 16 texture index bytes (func_L01_00423E10) and the eight s16 at +0x20 (lh at +0, +2, +4, +6, +8, +0xA, +0xC, +0xE of that block); func_L01_004252F0 maps shrub class ids through 0x2384F0; +8 and +0xC: no reader (zero in every level)",
            &[(8, 8)],
            None,
        ));
    }
    first
}
