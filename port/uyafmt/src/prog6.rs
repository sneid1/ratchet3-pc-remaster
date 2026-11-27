//! What VU1 program 6 draws: the game's depth-sorted effect sprites and beams (explosions, dust and smoke clouds, sparks, ground
//! rings, laser beams). Program 6 draws nothing else; no water or sea mesh is made by it, those are 64-byte effect entries that
//! game code creates at run time (see "Where the entries come from").
//!
//! # The path from an entry to the GS
//!
//! `func_L01_0045E810` (frontbin `func_003C7E68`, the same function in all 34 story levels; called by the frame draw
//! `func_L01_004010D0` at 0x401354 between TEST_1 = 0x53001 and 0x5360B) is the only code that uploads program 6 (the MPG tag
//! at 0x101140 refers to the microcode at 0x101158, 1776 bytes). It reads the effect pool and writes the DMA chain:
//!
//! 1. Inputs (Veldin's addresses; gp = 0x1DC8B0): the pool base `[gp+0x3A48]` (0x1E02F8, 0x800 entries of 0x40 bytes), the
//!    highest used index `[gp+0x3A50]` (0x1E0300; the count is that plus one), the far limit `[gp+0x3A58]` (0x1E0308, `s3`), the
//!    camera position (0x1E2A40), the world-to-view rotation rows (0x1E2900, 0x1E2910, 0x1E2920), the world-to-clip columns
//!    (0x1E29C0..0x1E29F0; the first three scaled by 1024), `vf24` (0x1E77C0, x and y scaled by 0.9375), `vf25` (`gp-0x2850`),
//!    `vf26`/`vf27` (0x1E76B0, 0x1E77F0), the effect texture table (0x220AC0, 128 pairs of u32, filled by `func_L01_004224C8`
//!    from the core's region at word 0x19), the sine table (0x2BFDA0, 256 f32, in the level overlay's data), the VRAM
//!    allocator `s0` (`[gp-0x7D00]`, 0x1D4BB0, before the call), and the 17 quadwords the program starts with (see `Frame`).
//! 2. Each entry (in index order) is culled and put in one of 1024 depth buckets (depth >> 10, later entries first within a
//!    bucket); the buckets are then read from the deepest to the nearest into a sorted list. Entries seen get flag 0x20.
//! 3. For each entry in that order the item the microcode reads is written (4, 5, 4 or 6 quadwords for entry kinds 0 to 3), the
//!    entry's texture word is turned into a GS TEX0 on first use (`0x45F438`: the texture is given the next VRAM slot, `s0`),
//!    and every time the 0x800-byte staging buffer reaches its 0x700 mark a run is closed: DMA tag, UNPACK of the items to
//!    VU1 address TOPS+0 (the two buffers 0 and 0x200 alternate), the footer, MSCAL at 0xA8.
//! 4. The run's items are drawn by program 6 (entry 0xA8): one GIF packet per item with XGKICK. `draw` has the caller run the
//!    program (`Vu1Run`): the port's renderer runs it as the port's translator translated it.
//!
//! # Entry layout (64 bytes; the fields func_L01_0045E810 reads)
//!
//! +0 effect type (read by the effect's own update code), +1 flags (bit 7 skip, bits 0-1 the kind: 0 camera-facing sprite,
//! 1 ground-plane quad, 2 line, 3 textured ribbon; 0x20 drawn last frame), +2 effect texture index, +3 ALPHA_1 value, +4 R, G, B,
//! A bytes, +7 is A (the alpha the fade scales), +8 angle byte (index into the sine table), +9 near (low nibble << 10) and far
//! (high nibble << 13, 0xF is "no limit") distances in 1/4096 units of view depth, +0xC size (kind 0, 1), +0x10 position (x, y,
//! z, w = ribbon width), kinds 2 and 3: +0x20 second point (w = second width), +0xC the second colour, +0xF its alpha.
//!
//! # Program 6 (444 words at 0x101158; entries are byte addresses of instruction pairs)
//!
//! - 0x00: the run that follows the upload. It loads the 17 constant quadwords into vf28..31 (the view-projection matrix, rows
//!   1024 times larger than the EE's, row 3 the translation), vf25 (-0.02391, 293.25 = fog, 51200, 204800), vf26 (the screen
//!   centre 2048, 2048, the z offset 8388112, fog 293.25), vf27 (1, 1.0625: the aspect of a quad's second axis), vf21..24 (the
//!   ST corners (0,0,1) (1,0,1) (0,1,1) (1,1,1)) and vf15..20 (GIF tag words and the TEX0 register's address 6 and the
//!   ALPHA_1's address 0x42), sets vi14 = 0x3FF, vi15 = 0x200, vi08 = -2 (kind 3's tag) and ends (E bit at 0x98).
//! - 0xA8: one run: vi15 = (vi15 + 0x200) & 0x3FF (the TOPS buffer), vi12 = the item quadword count (w of quadword 2), the
//!   first item's tag (w of quadword 0: 0 kind 0, 1 kind 1, -1 kind 2, -2 kind 3), output at vi15 + 0x80, XGKICK there.
//! - kind 0 (tag 0, 4 qw: position relative to the camera with w = 0, RGBAQ words, (dx, dy, ALPHA_1 value), TEX0 word): clip
//!   position = matrix * (position, 1), Q = vf25.x / w, corners = vf26 + (dx, 1.0625 dy), vf26 + (-dy, 1.0625 dx), and the
//!   negatives, plus (clip.xyz * Q), FTOI4; a 12-quadword packet: tag (NLOOP 1, PRE, PRIM 0x54 = textured alpha-blended
//!   triangle strip, 11 registers A+D, A+D, RGBAQ, then ST/XYZF2 x 4), ALPHA_1, TEX0_1, the RGBAQ, and the four corners with
//!   ST (0,0,1) (1,0,1) (0,1,1) (1,1,1) and fog F = vf26.w.
//! - kind 1 (tag 1, 5 qw): the quad's corners are (x ± dx, y ± dy) and (x ∓ dy, y ± dx) (the position's x and y, world
//!   units relative to the camera) at the position's z, each projected by the matrix; ST (0,0,1)*Q ... for perspective-correct
//!   texturing; 15 quadwords.
//! - kind 2 (tag -1, 4 qw): a Gouraud line between two projected points, alpha register 0x44; 6 quadwords.
//! - kind 3 (tag -2, 6 qw): a textured ribbon: p1 ± side, p2 ± side * (w of p2), 13 quadwords.
//! - the last item's tag word has EOP (0x8001).
//!
//! # Where the entries come from
//!
//! Entries are made at run time: `func_L01_0045E440` (through the wrapper 0x45E438) hands out 64-byte slots of the pool; its callers are
//! the 60 or so effect spawners in each level overlay (0x45FD70..0x472700 and the three at 0x4D4FA8, 0x4D5A98, 0x4D6400, each
//! the same function in all 34 story levels, plus level-specific ones in levels 5, 7, 19, 23, 30, 36). Each spawner fills the
//! entry from its arguments (positions, sizes, colours, a texture index) and a random-number function. Nothing on the disc
//! describes a water surface or a cloud sea as data for program 6; the effect textures (table 0x220AC0) come from the level
//! core (core region 0x19, index words 0x14/0x15, `coremisc::region_19`).

use crate::boot::Boot;

/// RAM address of program 6's microcode and its size in bytes (`scripts/uya_vugen.py` PROGS[6]).
pub const MICROCODE: u32 = 0x101158;
pub const MICROCODE_SIZE: usize = 1776;

/// The entry kinds (the low two bits of an entry's flags).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Sprite,
    Ground,
    Line,
    Ribbon,
}

/// Everything func_L01_0045E810 reads, as RAM holds it when the frame draw calls it.
#[derive(Clone)]
pub struct Frame {
    /// The pool's entries, index 0 to the highest used index.
    pub pool: Vec<[u8; 64]>,
    /// `[gp+0x3A58]`: the far limit (1/4096 view-depth units).
    pub limit: i32,
    /// 0x1E2A40: the camera position.
    pub cam: [f32; 4],
    /// 0x1E2900, 0x1E2910, 0x1E2920: the world-to-view rotation (vf29, vf30, vf31).
    pub rot: [[f32; 4]; 3],
    /// 0x1E29C0..0x1E29F0: the world-to-clip columns (vf20..vf23), as RAM holds them (the first three are scaled by 1024 here).
    pub clip: [[f32; 4]; 4],
    /// 0x1E77C0 (vf24; x and y are scaled by 0.9375 here), 0x1E76B0 (vf26), 0x1E77F0 (vf27), `gp-0x2850` (vf25).
    pub vf24: [f32; 4],
    pub vf26: [f32; 4],
    pub vf27: [f32; 4],
    pub vf25: [f32; 4],
    /// The effect texture table, 128 pairs (low word, high word) at 0x220AC0.
    pub textures: Vec<[u32; 2]>,
    /// The sine table at 0x2BFDA0.
    pub sine: Vec<f32>,
    /// The VRAM allocator `[gp-0x7D00]` before the call (a GS block number << 8).
    pub vram: u32,
    /// `gp-0x2860` (vf20): the TEX0 word the resolved texture words start from.
    pub tex0_template: u64,
    /// The program's first 17 quadwords: 0x1E2940..0x1E2970 (rows 0 to 2 scaled by 1024 here), 0x1E7820, 0x1E77A0 and the 11
    /// quadwords at the address `[0x1DA014]` holds (0x2EEEE0).
    pub vu_matrix: [[f32; 4]; 4],
    pub vu_q4: [u32; 4],
    pub vu_q5: [u32; 4],
    pub vu_static: [[u32; 4]; 11],
}

/// Veldin's addresses of the values `Frame` holds.
pub mod veldin {
    pub const GP: u32 = 0x1DC8B0;
    pub const POOL: u32 = 0x1E02F8;
    pub const MAX_INDEX: u32 = 0x1E0300;
    pub const LIMIT: u32 = 0x1E0308;
    pub const CAM: u32 = 0x1E2A40;
    pub const ROT: u32 = 0x1E2900;
    pub const CLIP: u32 = 0x1E29C0;
    pub const VF24: u32 = 0x1E77C0;
    pub const VF26: u32 = 0x1E76B0;
    pub const VF27: u32 = 0x1E77F0;
    pub const VF25: u32 = GP - 0x2850;
    pub const TEX0_TEMPLATE: u32 = GP - 0x2860;
    pub const TEXTURES: u32 = 0x220AC0;
    pub const SINE: u32 = 0x2BFDA0;
    pub const VRAM: u32 = GP - 0x7D00;
    pub const VU_MATRIX: u32 = 0x1E2940;
    pub const VU_Q4: u32 = 0x1E7820;
    pub const VU_Q5: u32 = 0x1E77A0;
    pub const STATIC_PTR: u32 = 0x1DA014;
}

/// The addresses of the values `Frame` holds, in one level's RAM: `fxcode::addresses` reads them from the level's own code,
/// `veldin_addrs` is Veldin's (the `veldin` constants). Each `lqc2`/`lq` field is one quadword; `pool`, `max_index`, `limit`,
/// `vram` and `static_ptr` are variables (u32), `textures` is 128 pairs of u32, `sine` 256 f32.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FxAddrs {
    pub gp: u32,
    pub pool: u32,
    pub max_index: u32,
    pub limit: u32,
    pub cam: u32,
    pub rot: u32,
    pub clip: u32,
    pub vf24: u32,
    pub vf26: u32,
    pub vf27: u32,
    pub vf25: u32,
    pub tex0_template: u32,
    pub textures: u32,
    pub sine: u32,
    pub vram: u32,
    pub vu_matrix: u32,
    pub vu_q4: u32,
    pub vu_q5: u32,
    /// The variable (gp-0x289C on Veldin: the second word of the quadword at gp-0x28A0, a DMA tag func_L01_0045E810 +0x5AC copies
    /// into the program's first run) holding the address of the 11 constant quadwords.
    pub static_ptr: u32,
}

/// Veldin's addresses.
pub fn veldin_addrs() -> FxAddrs {
    use veldin::*;
    FxAddrs { gp: GP, pool: POOL, max_index: MAX_INDEX, limit: LIMIT, cam: CAM, rot: ROT, clip: CLIP, vf24: VF24, vf26: VF26, vf27: VF27, vf25: VF25, tex0_template: TEX0_TEMPLATE, textures: TEXTURES, sine: SINE, vram: VRAM, vu_matrix: VU_MATRIX, vu_q4: VU_Q4, vu_q5: VU_Q5, static_ptr: STATIC_PTR }
}

fn u32_ram(ram: &[u8], a: u32) -> Result<u32, String> {
    let a = a as usize;
    let b = ram.get(a..a + 4).ok_or_else(|| format!("prog6: RAM address {a:#x} is past the dump"))?;
    Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

fn qw_ram(ram: &[u8], a: u32) -> Result<[u32; 4], String> {
    Ok([u32_ram(ram, a)?, u32_ram(ram, a + 4)?, u32_ram(ram, a + 8)?, u32_ram(ram, a + 12)?])
}

fn qf(q: [u32; 4]) -> [f32; 4] {
    [f32::from_bits(q[0]), f32::from_bits(q[1]), f32::from_bits(q[2]), f32::from_bits(q[3])]
}

impl Frame {
    /// The frame's inputs from an EE RAM dump (address 0 first) with Veldin's addresses (`veldin`). The per-level addresses of the
    /// level-owned values (0x1E2900.., 0x1E77xx, 0x220AC0, 0x2BFDA0, 0x2EEEE0) are the same instructions' in each level's overlay
    /// (levelcode.rs finds a function's instructions by their words); the gp-relative ones are the same in every level.
    pub fn from_ram(ram: &[u8]) -> Result<Frame, String> {
        Frame::from_ram_at(ram, &veldin_addrs())
    }

    /// The frame's inputs from an EE RAM dump (address 0 first) of a level whose addresses are `a` (`fxcode::addresses`).
    pub fn from_ram_at(ram: &[u8], a: &FxAddrs) -> Result<Frame, String> {
        let pool_base = u32_ram(ram, a.pool)?;
        // The highest index in use; -1 when the pool is empty (the count is the index plus one).
        let max_index = u32_ram(ram, a.max_index)? as i32;
        if !(-1..0x800).contains(&max_index) {
            return Err(format!("prog6: highest effect index {max_index:#x}"));
        }
        let mut pool = Vec::new();
        for i in 0..(max_index + 1) as usize {
            let a = pool_base as usize + i * 64;
            let b = ram.get(a..a + 64).ok_or_else(|| format!("prog6: effect {i} at {a:#x} is past the dump"))?;
            let mut e = [0u8; 64];
            e.copy_from_slice(b);
            pool.push(e);
        }
        let mut rot = [[0f32; 4]; 3];
        for (k, r) in rot.iter_mut().enumerate() {
            *r = qf(qw_ram(ram, a.rot + 16 * k as u32)?);
        }
        let mut clip = [[0f32; 4]; 4];
        for (k, c) in clip.iter_mut().enumerate() {
            *c = qf(qw_ram(ram, a.clip + 16 * k as u32)?);
        }
        for c in clip.iter_mut().take(3) {
            for v in c.iter_mut() {
                *v *= 1024.0;
            }
        }
        let mut vf24 = qf(qw_ram(ram, a.vf24)?);
        vf24[0] *= 0.9375;
        vf24[1] *= 0.9375;
        let mut vu_matrix = [[0f32; 4]; 4];
        for (k, c) in vu_matrix.iter_mut().enumerate() {
            *c = qf(qw_ram(ram, a.vu_matrix + 16 * k as u32)?);
        }
        for c in vu_matrix.iter_mut().take(3) {
            for v in c.iter_mut() {
                *v *= 1024.0;
            }
        }
        // The matrix's w column is the clip columns' w times q4's x (Q = q4.x / clip.w): the two are made together, so the screen
        // the program puts a point at is the clip columns' (1 / their w) whatever q4.x is (-0.000125 to -0.064 on the levels'
        // frames, every one with this ratio to the float's last bit). The column is taken from them: on level 10's self-destruct
        // countdown the camera struct's w column is 1/40.8 of it in the frame's RAM and q4 is not, and a frame put back in the
        // world through that had its 2D layer squeezed into a point in the middle (no countdown shown) and its effects 40 times
        // off the screen (the player, 22 November 2026: "the autodestruct timer doesnt appear properly on screen, causing a bunch
        // of other level texture issues"); the game's own picture there has the countdown, its effects and its red light.
        let q4 = qw_ram(ram, a.vu_q4)?;
        let q4x = f32::from_bits(q4[0]);
        for r in 0..4 {
            vu_matrix[r][3] = q4x * clip[r][3];
        }
        let sp = u32_ram(ram, a.static_ptr)?;
        let mut vu_static = [[0u32; 4]; 11];
        for (k, q) in vu_static.iter_mut().enumerate() {
            *q = qw_ram(ram, sp + 16 * k as u32)?;
        }
        let mut textures = Vec::new();
        for i in 0..128u32 {
            textures.push([u32_ram(ram, a.textures + 8 * i)?, u32_ram(ram, a.textures + 8 * i + 4)?]);
        }
        let mut sine = Vec::new();
        for i in 0..256u32 {
            sine.push(f32::from_bits(u32_ram(ram, a.sine + 4 * i)?));
        }
        let t = qw_ram(ram, a.tex0_template)?;
        Ok(Frame {
            pool,
            limit: u32_ram(ram, a.limit)? as i32,
            cam: qf(qw_ram(ram, a.cam)?),
            rot,
            clip,
            vf24,
            vf26: qf(qw_ram(ram, a.vf26)?),
            vf27: qf(qw_ram(ram, a.vf27)?),
            vf25: qf(qw_ram(ram, a.vf25)?),
            textures,
            sine,
            vram: u32_ram(ram, a.vram)?,
            tex0_template: t[0] as u64 | (t[1] as u64) << 32,
            vu_matrix,
            vu_q4: q4,
            vu_q5: qw_ram(ram, a.vu_q5)?,
            vu_static,
        })
    }
}

/// An effect texture as the table at 0x220AC0 gives it: the core data's CLUT and texels (RAM addresses inside the core's region
/// at word 0x19; subtract the core's RAM base and that region's core offset to get the offsets `coremisc::region_19` knows).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectTexture {
    /// The table index (an entry's byte +2).
    pub index: usize,
    pub clut_address: u32,
    pub texel_address: u32,
    /// log2 of the (square) texture's side.
    pub log2_size: u32,
    /// The low nibble of the CLUT word (the CLUT's pixel storage mode for the upload).
    pub clut_psm: u32,
}

/// One vertex as the GS gets it: ST/Q, the 12.4 fixed-point position (the screen origin is 2048), the depth and the fog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub s: f32,
    pub t: f32,
    pub q: f32,
    /// XYZF2 packed words: x and y in 1/16 pixel (low 16 bits), z (>> 4 of the third word), fog (bits 4-11 of the fourth).
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub fog: u32,
}

/// One drawn effect: a GIF packet of program 6's output, decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Prim {
    pub kind: Kind,
    /// The pool index of the effect entry.
    pub entry: usize,
    /// GS PRIM (the tag's PRIM field: 0x54 triangle strip with texture and alpha blending, 0x4x for lines).
    pub prim: u32,
    /// ALPHA_1.
    pub alpha: u64,
    /// The TEX0_1 word (None for the untextured line), and the effect texture it names.
    pub tex0: Option<u64>,
    pub texture: Option<EffectTexture>,
    /// Where the packet's quadwords (as the GS receives them) are in its run's `raw`: the tag's quadword and the count.
    at: usize,
    len: usize,
}

/// One run of program 6 (one UNPACK + MSCAL): the packets program 6 made from the run's items, and the effects they hold.
#[derive(Clone, Debug)]
pub struct Run {
    /// The VU1 address the items were unpacked to (0 or 0x200).
    pub tops: u32,
    /// The run's GIF packets as the GS receives them, one after another.
    pub raw: Vec<[u32; 4]>,
    pub prims: Vec<Prim>,
}

impl Run {
    /// The quadwords of `prim`'s packet, its tag first.
    pub fn raw_of(&self, prim: &Prim) -> &[[u32; 4]] {
        &self.raw[prim.at..prim.at + prim.len]
    }
}

/// What program 6 draws for the frame, and the values the code leaves behind.
#[derive(Clone, Debug)]
pub struct Drawn {
    pub runs: Vec<Run>,
    /// The pool entries' flags after the call (0x20 set for each drawn entry, cleared for each that was drawn and is not now).
    pub flags: Vec<u8>,
    /// The VRAM allocator after the call (`[gp-0x7D00]`).
    pub vram: u32,
    /// The texture words the call resolved, in order of first use: (table index, TEX0).
    pub resolved: Vec<(usize, u64)>,
}

fn f32_at(e: &[u8; 64], o: usize) -> f32 {
    f32::from_le_bytes([e[o], e[o + 1], e[o + 2], e[o + 3]])
}

fn m3(rot: &[[f32; 4]; 3], v: [f32; 3]) -> [f32; 3] {
    let mut r = [0f32; 3];
    for c in 0..3 {
        r[c] = rot[0][c] * v[0] + rot[1][c] * v[1] + rot[2][c] * v[2];
    }
    r
}

fn m4(cols: &[[f32; 4]; 4], v: [f32; 3]) -> [f32; 4] {
    let mut r = [0f32; 4];
    for c in 0..4 {
        r[c] = cols[0][c] * v[0] + cols[1][c] * v[1] + cols[2][c] * v[2] + cols[3][c];
    }
    r
}

/// VCLIP: the six outside bits of (x, y, z) against |w| (x > w, x < -w, y > w, y < -w, z > w, z < -w).
fn clip_bits(v: [f32; 4]) -> u32 {
    let w = v[3].abs();
    (v[0] > w) as u32 | ((v[0] < -w) as u32) << 1 | ((v[1] > w) as u32) << 2 | ((v[1] < -w) as u32) << 3 | ((v[2] > w) as u32) << 4 | ((v[2] < -w) as u32) << 5
}

/// FTOI12 of a float: truncation toward zero of x * 4096.
fn ftoi12(x: f32) -> i32 {
    (x as f64 * 4096.0).trunc().clamp(i32::MIN as f64, i32::MAX as f64) as i32
}

fn near_far(b9: u8, limit: i32, special: bool) -> (i64, i64) {
    let near = ((b9 & 0xF) as i64) << 10;
    let mut far = ((b9 & 0xF0) as i64) << 13;
    if special && 0x1D0000 < far {
        far += 1 << 23;
    }
    (near, far.min(limit as i64))
}

/// The culling and sorting pass (0x45E980..0x45EC70): the sorted entry indices, deepest first.
fn sort(f: &Frame, flags: &mut [u8]) -> Vec<usize> {
    // Each entry seen: its depth bucket (depth >> 10) over its pool index.
    let mut seen: Vec<u32> = Vec::with_capacity(f.pool.len());
    for (i, e) in f.pool.iter().enumerate() {
        let fl = flags[i];
        if fl & 0x80 != 0 {
            continue;
        }
        let rel = [f32_at(e, 0x10) - f.cam[0], f32_at(e, 0x14) - f.cam[1], f32_at(e, 0x18) - f.cam[2]];
        let cleared = fl & 0xDF;
        let mut cull = false;
        let depth: i64;
        if fl & 2 == 0 {
            let size = f32_at(e, 0xC);
            let (near, far) = near_far(e[9], f.limit, true);
            let v = m3(&f.rot, rel);
            let s3x = f.vf25[0] * size;
            let s3y = f.vf25[1] * size;
            let size_i = ftoi12(s3x) as i64;
            depth = ftoi12(v[2]) as u32 as i64;
            let sx = f.vf26[0] * v[2] - (v[0].abs() - f.vf27[0] * s3x);
            let sy = f.vf26[1] * v[2] - (v[1].abs() - f.vf27[1] * s3x);
            let _ = s3y;
            if depth - near <= 0 || depth - far >= 0 {
                cull = true;
            } else {
                let near_edge = ((depth - size_i) < near) as u8 & fl;
                cull = near_edge != 0 || sy.is_sign_negative() || sx.is_sign_negative();
            }
        } else {
            let p2 = [f32_at(e, 0x20) - f.cam[0], f32_at(e, 0x24) - f.cam[1], f32_at(e, 0x28) - f.cam[2]];
            let (near, far) = near_far(e[9], f.limit, false);
            let c1 = m4(&f.clip, rel);
            let c3 = m4(&f.clip, p2);
            let first = clip_bits(c1) << 6 | clip_bits(c3);
            let mid = [(rel[0] + p2[0]) * f.vf25[2], (rel[1] + p2[1]) * f.vf25[2], (rel[2] + p2[2]) * f.vf25[2]];
            let mut s1 = c1;
            let mut s3 = c3;
            for k in 0..2 {
                s1[k] *= f.vf24[k];
                s3[k] *= f.vf24[k];
            }
            let v = m3(&f.rot, mid);
            let b1 = clip_bits(s1);
            let b3 = clip_bits(s3);
            depth = ftoi12(v[2]) as u32 as i64;
            if first & 0xFFF != 0 || b1 & b3 != 0 || depth - near <= 0 || depth - far >= 0 {
                cull = true;
            }
        }
        if cull {
            if fl != cleared {
                flags[i] = cleared;
            }
            continue;
        }
        seen.push(((depth >> 10).min(0x3FF) as u32) << 16 | i as u32);
    }
    // The buckets are read from the last to the first and each from its last entry to its first: the deepest bucket and, within a
    // bucket, the later entry first.
    seen.sort_unstable_by(|a, b| b.cmp(a));
    let order: Vec<usize> = seen.iter().map(|k| (k & 0xFFFF) as usize).collect();
    order
}

fn expand(b: [u8; 4]) -> [u32; 4] {
    [b[0] as u32, b[1] as u32, b[2] as u32, b[3] as u32]
}

fn bytes4(e: &[u8; 64], o: usize) -> [u8; 4] {
    [e[o], e[o + 1], e[o + 2], e[o + 3]]
}

fn fl(x: f32) -> u32 {
    x.to_bits()
}

/// The texture word of table entry `idx`, resolved the way 0x45F438 does on first use.
fn resolve(f: &Frame, table: &mut [Option<u64>], idx: usize, vram: &mut u32, resolved: &mut Vec<(usize, u64)>) -> Result<u64, String> {
    if let Some(t) = table[idx] {
        return Ok(t);
    }
    let [lo, hi] = *f.textures.get(idx).ok_or_else(|| format!("prog6: texture index {idx}"))?;
    let raw = lo as u64 | (hi as u64) << 32;
    if raw >> 63 != 0 {
        table[idx] = Some(raw);
        return Ok(raw);
    }
    let a3 = (lo & 0xF) as u64;
    let a2 = (hi & 0xF) as u64;
    let mut t0 = f.tex0_template;
    t0 |= a2 << 26;
    t0 |= a2 << 30;
    let a1 = 0x400u32.wrapping_sub((a3 as u32) << 8);
    t0 |= (a3 << 8) << 43;
    let cbp = (*vram >> 8) as u64;
    *vram = vram.wrapping_add(a1);
    t0 |= cbp << 37;
    let tbp = (*vram >> 8) as u64;
    t0 |= tbp;
    *vram = vram.wrapping_add((1u32 << a2) << a2);
    table[idx] = Some(t0);
    resolved.push((idx, t0));
    Ok(t0)
}

/// Builds the items of the sorted entries (0x45EE68..0x45F2B8) and closes a run each time the staging buffer fills.
fn build(f: &Frame, flags: &mut [u8], order: &[usize], vram: &mut u32, resolved: &mut Vec<(usize, u64)>) -> Result<Vec<Vec<([u32; 4], usize)>>, String> {
    let mut table: Vec<Option<u64>> = vec![None; f.textures.len()];
    let mut runs: Vec<Vec<([u32; 4], usize)>> = Vec::new();
    let mut cur: Vec<([u32; 4], usize)> = Vec::new();
    let mut t8: u32 = 0x10;
    for &i in order {
        let e = &f.pool[i];
        let fl_ = flags[i];
        let kind = fl_ & 3;
        let mut rel = [f32_at(e, 0x10) - f.cam[0], f32_at(e, 0x14) - f.cam[1], f32_at(e, 0x18) - f.cam[2], f32_at(e, 0x1C)];
        if flags[i] & 0x20 == 0 {
            flags[i] |= 0x20;
        }
        let (near, far_raw) = near_far(e[9], f.limit, false);
        let rgba = expand(bytes4(e, 4));
        let alpha_reg = e[3] as u32;
        let mut push = |qws: &[[u32; 4]]| cur.extend(qws.iter().map(|&q| (q, i)));
        let fade = |depth: i64, extra: i64| -> i64 {
            let a = ((far_raw - depth) as u32 >> 4) as i64;
            a.min(depth - near - extra).min(0x1000)
        };
        match kind {
            0 => {
                let v = m3(&f.rot, [rel[0], rel[1], rel[2]]);
                let depth = ftoi12(v[2]) as u32 as i64;
                let q = f32_at(e, 0xC) * f.vf25[3] / (v[2] + f.vf25[2]);
                let ang = (e[8] as usize) * 4;
                let sin = f.sine[(ang / 4) & 0xFF];
                let cos = f.sine[((ang + 0x300) & 0x3FF) / 4];
                let tex = resolve(f, &mut table, e[2] as usize, vram, resolved)?;
                let a = (fade(depth, 0).max(i64::MIN) * e[7] as i64) >> 12;
                let mut c = rgba;
                c[3] = a as u32;
                rel[3] = 0.0;
                push(&[
                    [fl(rel[0]), fl(rel[1]), fl(rel[2]), 0],
                    c,
                    [fl(sin * q), fl(cos * q), alpha_reg, 0],
                    [tex as u32, (tex >> 32) as u32, 0, 0],
                ]);
                t8 += 0x40;
            }
            1 => {
                let v = m3(&f.rot, [rel[0], rel[1], rel[2]]);
                let depth = ftoi12(v[2]) as u32 as i64;
                let f0 = f32_at(e, 0xC) * f.vf25[0];
                let size_i = (f0 * 4096.0) as i32 as i64;
                let ang = (e[8] as usize) * 4;
                let sin = f.sine[(ang / 4) & 0xFF] * f0;
                let cos = f.sine[((ang + 0x300) & 0x3FF) / 4] * f0;
                let tex = resolve(f, &mut table, e[2] as usize, vram, resolved)?;
                let a = (fade(depth, size_i) * e[7] as i64) >> 12;
                let mut c = rgba;
                c[3] = a as u32;
                push(&[
                    [fl(rel[0]), fl(rel[1]), fl(rel[2]), 1],
                    c,
                    [fl(sin), fl(cos), alpha_reg, 0],
                    [tex as u32, (tex >> 32) as u32, 0, 0],
                    [0, 0, 0, 0],
                ]);
                t8 += 0x50;
            }
            2 => {
                let p2 = [f32_at(e, 0x20) - f.cam[0], f32_at(e, 0x24) - f.cam[1], f32_at(e, 0x28) - f.cam[2], 0.0];
                let c2 = expand(bytes4(e, 0xC));
                push(&[[fl(rel[0]), fl(rel[1]), fl(rel[2]), 0xFFFF_FFFF], rgba, [fl(p2[0]), fl(p2[1]), fl(p2[2]), 0], c2]);
                t8 += 0x40;
            }
            _ => {
                let p2 = [f32_at(e, 0x20) - f.cam[0], f32_at(e, 0x24) - f.cam[1], f32_at(e, 0x28) - f.cam[2], f32_at(e, 0x2C)];
                let c2 = expand(bytes4(e, 0xC));
                let mid = [(rel[0] + p2[0]) * f.vf25[2], (rel[1] + p2[1]) * f.vf25[2], (rel[2] + p2[2]) * f.vf25[2]];
                let d = [p2[0] - rel[0], p2[1] - rel[1], p2[2] - rel[2]];
                let cr = [d[1] * mid[2] - mid[1] * d[2], d[2] * mid[0] - mid[2] * d[0], d[0] * mid[1] - mid[0] * d[1]];
                let vm = m3(&f.rot, mid);
                let depth = ftoi12(vm[2]) as u32 as i64;
                let len2 = cr[0] * cr[0] + cr[1] * cr[1] + cr[2] * cr[2];
                let qv = rel[3] / len2.sqrt();
                let tex = resolve(f, &mut table, e[2] as usize, vram, resolved)?;
                let fd = fade(depth, 0);
                let mut c1 = rgba;
                let mut c2w = c2;
                c1[3] = ((fd * e[7] as i64) >> 12) as u32;
                c2w[3] = ((fd * e[0xF] as i64) >> 12) as u32;
                push(&[
                    [fl(rel[0]), fl(rel[1]), fl(rel[2]), 0xFFFF_FFFE],
                    [fl(p2[0]), fl(p2[1]), fl(p2[2]), fl(p2[3])],
                    [fl(cr[0] * qv), fl(cr[1] * qv), fl(cr[2] * qv), 0],
                    c1,
                    c2w,
                    [tex as u32, (tex >> 32) as u32, alpha_reg, 0],
                ]);
                t8 += 0x60;
            }
        }
        if t8 & 0x700 == 0x700 {
            runs.push(std::mem::take(&mut cur));
            t8 = 0x10;
        }
    }
    if !cur.is_empty() {
        runs.push(cur);
    }
    Ok(runs)
}

/// What runs program 6 for `draw`: puts `data` in VU1 data memory from quadword `at`, runs the game's program 6 from micro address
/// `entry`, and returns the GIF packet of the run's last XGKICK (empty for a run that kicks none). The port's renderer runs it
/// translated by the port's translator (`uyatr`), on VU1 state of its own; `uyafmt` holds no VU1 of its own.
pub type Vu1Run<'a> = dyn FnMut(&[[u32; 4]], usize, usize) -> Result<Vec<[u32; 4]>, String> + 'a;

/// Program 6 on 17 constant quadwords (the run at entry 0x00 after the upload) and the items of each run (the UNPACK data of one
/// MSCAL at entry 0xA8 each, alternating the TOPS buffers 0 and 0x200 as VIF1 does): each run's GIF packet as the GS receives it.
fn run_vu(vu1: &mut Vu1Run, consts: &[[u32; 4]; 17], runs: &[Vec<[u32; 4]>]) -> Result<Vec<Vec<[u32; 4]>>, String> {
    if !vu1(consts, 0, 0x00)?.is_empty() {
        return Err("prog6: the program's first run kicked a packet".into());
    }
    let mut out = Vec::new();
    for (r, items) in runs.iter().enumerate() {
        let packet = vu1(items, if r % 2 == 0 { 0 } else { 0x200 }, 0xA8)?;
        if packet.is_empty() {
            return Err("prog6: the run did not XGKICK".into());
        }
        out.push(packet);
    }
    Ok(out)
}

/// Decodes a run's GIF packets: one per item, in order (`kinds`: each item's kind and pool index).
fn decode(raw: &[[u32; 4]], kinds: &[(Kind, usize)], f: &Frame) -> Result<Vec<Prim>, String> {
    let mut out = Vec::with_capacity(kinds.len());
    let mut a = 0;
    let mut k = 0;
    while a < raw.len() {
        let tag = raw[a];
        let nloop = (tag[0] & 0x7FFF) as usize;
        let pre = tag[1] >> 14 & 1;
        let prim = (tag[1] >> 15) & 0x7FF;
        let flg = tag[1] >> 26 & 3;
        let nreg = match tag[1] >> 28 {
            0 => 16,
            n => n as usize,
        };
        if flg != 0 || pre != 1 {
            return Err(format!("prog6: GIF tag {tag:08x?} is not a PACKED tag with PRE"));
        }
        let len = 1 + nloop * nreg;
        let packet = &raw[a..a + len];
        let (kind, entry) = *kinds.get(k).ok_or("prog6: more packets than items")?;
        let mut alpha = 0u64;
        let mut tex0 = None;
        for l in 0..nloop {
            for r in 0..nreg {
                let reg = if r < 8 { tag[2] >> (4 * r) & 15 } else { tag[3] >> (4 * (r - 8)) & 15 };
                let d = packet[1 + l * nreg + r];
                match reg {
                    0xE => {
                        let data = d[0] as u64 | (d[1] as u64) << 32;
                        match d[2] {
                            0x42 => alpha = data,
                            0x06 => tex0 = Some(data),
                            n => return Err(format!("prog6: A+D register {n:#x}")),
                        }
                    }
                    1 | 2 | 4 => {}
                    n => return Err(format!("prog6: GIF register {n:#x}")),
                }
            }
        }
        let texture = tex0.map(|_| {
            let idx = f.pool[entry][2] as usize;
            let [lo, hi] = f.textures[idx];
            EffectTexture { index: idx, clut_address: lo >> 4, texel_address: hi >> 4, log2_size: hi & 0xF, clut_psm: lo & 0xF }
        });
        out.push(Prim { kind, entry, prim, alpha, tex0, texture, at: a, len });
        k += 1;
        a += len;
    }
    Ok(out)
}

/// The 17 constant quadwords the program starts with.
pub fn constants(f: &Frame) -> [[u32; 4]; 17] {
    let mut q = [[0u32; 4]; 17];
    for r in 0..4 {
        q[r] = [fl(f.vu_matrix[r][0]), fl(f.vu_matrix[r][1]), fl(f.vu_matrix[r][2]), fl(f.vu_matrix[r][3])];
    }
    q[4] = f.vu_q4;
    q[5] = f.vu_q5;
    q[6..].copy_from_slice(&f.vu_static);
    q
}

/// Program 6's output for the frame's effect entries: culls, sorts and builds the items as `func_L01_0045E810` does and has `vu1`
/// run the game's own program 6 (`MICROCODE_SIZE` bytes from RAM 0x101158, `microcode`) on them.
pub fn draw(f: &Frame, vu1: &mut Vu1Run) -> Result<Drawn, String> {
    let mut flags: Vec<u8> = f.pool.iter().map(|e| e[1]).collect();
    let mut vram = f.vram;
    let mut resolved = Vec::new();
    let order = sort(f, &mut flags);
    let items = build(f, &mut flags, &order, &mut vram, &mut resolved)?;
    let qws: Vec<Vec<[u32; 4]>> = items
        .iter()
        .map(|run| {
            let mut v: Vec<[u32; 4]> = run.iter().map(|(q, _)| *q).collect();
            v[2][3] = v.len() as u32;
            v
        })
        .collect();
    let packets = run_vu(vu1, &constants(f), &qws)?;
    let mut runs = Vec::new();
    for (r, (run, raw)) in items.iter().zip(packets).enumerate() {
        let mut kinds = Vec::with_capacity(run.len() / 4);
        let mut k = 0;
        while k < run.len() {
            let (w, entry) = (run[k].0[3], run[k].1);
            let (kind, len) = match w {
                0 => (Kind::Sprite, 4),
                1 => (Kind::Ground, 5),
                0xFFFF_FFFF => (Kind::Line, 4),
                0xFFFF_FFFE => (Kind::Ribbon, 6),
                _ => return Err(format!("prog6: item tag {w:#x}")),
            };
            kinds.push((kind, entry));
            k += len;
        }
        let prims = decode(&raw, &kinds, f)?;
        runs.push(Run { tops: if r % 2 == 0 { 0 } else { 0x200 }, raw, prims });
    }
    Ok(Drawn { runs, flags, vram, resolved })
}

/// Reads program 6's microcode from the boot program.
pub fn microcode(boot: &Boot) -> Result<&[u8], String> {
    boot.bytes(MICROCODE, MICROCODE_SIZE)
}
