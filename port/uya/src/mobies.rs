//! The mobies (Ratchet, enemies, crates, everything VU1 program 0 draws), from the game's own moby chain of the frame and the
//! classes `uyafmt::moby` decodes from the disc.
//!
//! Each frame func_L01_0043EA78 builds the moby DMA chain for VIF1 and the draw records of the skin job (`uyafmt::moby`, "The
//! draw job and the skin job"); the skin job (func_003D00F8) writes each mesh's skinned positions, lit colours and, for the rigid
//! layout, texture coordinates into the chain's own data blocks. The DMA sends the chain in the next field: the frame's jobs
//! (`jobs.rs`, as the DMA read every block, from RAM or scratchpad) hold everything program 0 receives for each run, in the
//! order it runs, and the frame walk (`walk`) reads every job of the frame, the world's, the moby chain's and those after it
//! (the water's and other programs' passes, the effect pass, the EE's own packets: shadows, weapon fire, the glow, the HUD):
//! - the chain starts with a cnt tag of 6 quadwords (STCYCL 4/4, UNPACK V4-32 to 0x123: the program's constants, vf24 to vf22;
//!   0x150 bytes before the first record's first mesh output), a cnt of the same for the other
//!   buffer, then per mesh a cnt of 4 quadwords (UNPACK V4-32 to TOPS + 0x129: the model-to-clip rows vf17 to vf20), the
//!   skin job's arrays (cnt tags: UNPACK V4-8 colours to TOPS + 0x61, V3-16 positions to TOPS + 0, and for the rigid layout
//!   V2-16 texture coordinates to TOPS + 0xC2), a ref tag to the class's packet A (strip list, texture coordinates, texture
//!   setups), DIRECT GS packets (A+D: TEST_1, ALPHA_1, FRAME_1 when they change; and GS primitives the EE writes itself) and
//!   MSCAL 0xE (variant B) or 0xA (variant A); cnt, ref and next tags. The pass ends where the jobs upload the next pass's
//!   VU1 program (MPG; Veldin field 2400: a ref to program 6 at 0x101150) or at an end tag. Veldin field 2400: 375 runs, 364 B
//!   and 11 A, as the trace shows.
//! - the mesh is the class mesh whose packet A the ref names: the class's RAM address is the core's RAM base (func_L01_00428750
//!   loads it at +0x60 from its level-buffer struct, `levelcode::LevelGlobals::core`) plus the class record's core offset, and
//!   the packet is at the class plus `Mesh::a_at`; or a mesh of the hero's armor (the WAD5 entry at `armor_entry`, its block at
//!   the address `armor_buffer` holds) or of the wrench (WAD5 entry 31, its block at the address `wrench_block` holds), whose
//!   texture setups name slots through the level's remap bytes and the slots are the entry's images (`moby::armor_entry`); or a
//!   class of the SCENE record the cutscene loader has unpacked (record `[scene_index]` at `[scene_base]`, `scene::load`: a setup
//!   number indexes the class's own images, class 10's maps through the remap bytes to WAD 5's class 10 texture
//!   `[class10_texture]`); or the hero body (`[body_base]`, `scenecode::hero_body` for `[body_variant]` and `[body_texture]`). The
//!   variables are each level's own (`uyafmt::scenecode`). Other classes are counted each frame, not drawn.
//! - a vertex's world position: the rows U (vf17 to vf20) are the instance's rows (rotation times scale, and the position
//!   relative to the camera, both times 1024) multiplied by the camera's 4 x 4 C at 0x1E2940 (func_L01_003FE8F8 builds it at
//!   the view camera - 0x120, `addiu $a0, $s2, -0x120`), so (x, y, z, 1) . U . C^-1 / 1024 + the eye is the world position
//!   (Veldin field 2400: every instance's rows come out orthogonal with one scale, and the translations are the instances'
//!   positions);
//! - F: variant B: vf20.w + vf22.w for the whole run; variant A: clip.w + vf22.w per vertex, clip.w = (x, y, z, 1) . U's w column;
//!   then max(vf24.x), min(255), the integer part; ADC vertices draw no triangle;
//! - the colours are the skin job's bytes; the texture coordinates packet A's (s16 / 4096) or the rigid array's; the texture
//!   of a strip is its group's setup (TEX1 K and MMIN, CLAMP, the texture record of the class record's byte `local`), a strip
//!   without one keeps the texture bound before it (the previous run's when it is the first);
//! - the pixel: ALPHA_1 0x44 ((Cs - Cd) x As + Cd; 0x8044 in the GS stream of the traced frame) and the run's TEST_1: 0x5B00C
//!   (no alpha test, Z GEQUAL) or 0x5360B (alpha test GEQUAL 0x60, a failing pixel writes only its colour); 1/Q is 32 times the
//!   view depth (vf23.x / clip.w with the w row of C: 0.7606 per world unit / 0.02381);
//! - variant A with the strip list's byte +0 = 0 or 2 culls the triangles whose screen-space orientation flag equals it (the
//!   module doc of `uyafmt::moby`), from the program's own screen coordinates.

use crate::effects::GsVertex;
use crate::native::Camera;
use crate::objects::{Blend, Draw, Key, Materials, ObjVertex, tex1_k};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use uyafmt::moby::MobyClass;

/// The walk's time by part when UYA_NATIVE_TIMES is set (`native::Times`, an instrument): the GS packets read (DIRECTs, GIF
/// jobs), the moby chain's unpacks and runs, the passes' programs; nanoseconds since `walk_times` last took them.
static WALK_NS: [AtomicU64; 3] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

fn walk_timed() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UYA_NATIVE_TIMES").is_some())
}

/// `f`, its time added to part `k` of `WALK_NS` when the walk is timed.
#[inline(always)]
fn walk_part<T>(k: usize, f: impl FnOnce() -> T) -> T {
    if !walk_timed() {
        return f();
    }
    let t = std::time::Instant::now();
    let r = f();
    WALK_NS[k].fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
    r
}

/// The walk's parts' times (ms) since the last call, for `native::Times`.
pub(crate) fn walk_times() -> [(&'static str, f64); 3] {
    let ms = |k: usize| WALK_NS[k].swap(0, Ordering::Relaxed) as f64 / 1e6;
    [("walk: GS packets", ms(0)), ("walk: moby chain", ms(1)), ("walk: passes", ms(2))]
}

/// A primitive the EE writes into the frame's jobs itself (DIRECT GIF packets in REGLIST or PACKED mode: the shadows, the HUD),
/// with the GS state it draws with: the registers of the context its PRIM names (CTXT, bit 9).
#[derive(Clone, Debug)]
pub(crate) struct EePrim {
    /// The moby runs before it in the chain; `AFTER_EFFECTS` when it comes after the effect pass (program 6's runs).
    pub after: usize,
    pub prim: u64,
    pub frame: u64,
    pub zbuf: u64,
    pub test: u64,
    pub alpha: u64,
    pub tex0: u64,
    pub clamp: u64,
    pub tex1: u64,
    /// MIPTBP1 and MIPTBP2: the base and width of mip levels 1 to 6 (TEX1's MXL says how many).
    pub mip: [u64; 2],
    pub xyoffset: u64,
    pub scissor: u64,
    /// Context 1's XYOFFSET (the main frame's: the world is drawn with context 1).
    pub xyoffset1: u64,
    /// The IMAGE transfer earlier in the chain that uploaded the texels at TEX0's TBP0, if one did.
    pub texels: Option<Upload>,
    /// The one that uploaded the CLUT at TEX0's CBP, if one did.
    pub clut: Option<Upload>,
    /// TEXA (the alpha of the 16- and 24-bit formats' colours) as the jobs last set it (None: none has).
    pub texa: Option<u64>,
    /// The vertices as the GS kicks them: 1 for a point, 2 for a line or a sprite, 3 for a triangle.
    pub verts: Verts,
    /// The frame's back in a frame with no native world (`walk_into`: a trip's starfield): drawn across the picture's width, the
    /// PS2's frame scaled to it whole and its top and bottom cut, as a front end's space scene is (`Native::render_screen`).
    pub fill: bool,
}

/// A primitive's vertices (1 to 3), held in the primitive itself: the sea's passes give 20,000 primitives a frame, each of which
/// would otherwise allocate its own. It is the slice of them (`Deref`).
#[derive(Clone, Copy)]
pub(crate) struct Verts {
    n: u8,
    /// The vertices, and whatever was in the room past `n` (never read).
    v: [GsVertex; 3],
}

/// A vertex to fill the room of a queue that is not full.
const NO_VERTEX: GsVertex = GsVertex { s: 0.0, t: 0.0, q: 0.0, rgba: [0; 4], x: 0.0, y: 0.0, z: 0.0, f: 0, u: 0.0, v: 0.0 };

impl Verts {
    /// `vs` (1 to 3 vertices: a primitive's).
    pub fn new(vs: &[GsVertex]) -> Verts {
        assert!((1..=3).contains(&vs.len()), "a GS primitive of {} vertices", vs.len());
        Verts { n: vs.len() as u8, v: std::array::from_fn(|i| vs[i.min(vs.len() - 1)]) }
    }
}

impl std::ops::Deref for Verts {
    type Target = [GsVertex];
    fn deref(&self) -> &[GsVertex] {
        &self.v[..self.n as usize]
    }
}

impl std::ops::DerefMut for Verts {
    fn deref_mut(&mut self) -> &mut [GsVertex] {
        &mut self.v[..self.n as usize]
    }
}

impl<'a> IntoIterator for &'a Verts {
    type Item = &'a GsVertex;
    type IntoIter = std::slice::Iter<'a, GsVertex>;
    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl std::fmt::Debug for Verts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(self.iter()).finish()
    }
}

/// A transfer between the EE and GS memory the walk met (TRXDIR 0: host to local, an IMAGE; 1: local to host, a download): its
/// BITBLTBUF, TRXPOS and TRXREG, where it came among the EE's primitives (`index`: how many came before it; `after` their
/// place) and how many passes had started.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Transfer {
    pub dir: u64,
    pub bitbltbuf: u64,
    pub trxpos: u64,
    pub trxreg: u64,
    pub index: usize,
    pub after: usize,
    passes: usize,
    /// A host to local transfer's data: the IMAGE that followed it (its source address, format and size).
    pub image: Option<Upload>,
}

impl Transfer {
    /// The main frame's 64 x 32 pages (0 to 103, row by row: 8 to a row) the transfer's rectangle covers, a page each in order
    /// of the transfer, and where each sits in the transfer's own buffer; empty when it touches none. A PSMCT32 or PSMCT24
    /// rectangle of whole pages on a page boundary: the GS keeps such a rectangle page by page, so a transfer through a buffer
    /// of another width moves whole pages of the main frame.
    pub fn main_pages(&self) -> Result<Vec<u32>, String> {
        let (base, bw, psm) = if self.dir == 0 { (self.bitbltbuf >> 32 & 0x3fff, self.bitbltbuf >> 48 & 0x3f, self.bitbltbuf >> 56 & 0x3f) } else { (self.bitbltbuf & 0x3fff, self.bitbltbuf >> 16 & 0x3f, self.bitbltbuf >> 24 & 0x3f) };
        let (x, y) = if self.dir == 0 { (self.trxpos >> 32 & 0x7ff, self.trxpos >> 48 & 0x7ff) } else { (self.trxpos & 0x7ff, self.trxpos >> 16 & 0x7ff) };
        let (w, h) = (self.trxreg & 0xfff, self.trxreg >> 32 & 0xfff);
        // The main frame: FBP 0xF4 (page 0xF4), 8 pages wide, 416 rows (13 rows of pages).
        let (first, last) = (0xF4u64, 0xF4 + 8 * 13);
        let pages = |b: u64, n: u64| b / 32 < last && b / 32 + n > first;
        if !pages(base, bw.max(1) * (y + h).div_ceil(32)) {
            return Ok(Vec::new());
        }
        if !matches!(psm, 0 | 1) || base % 32 != 0 || x % 64 != 0 || y % 32 != 0 || w % 64 != 0 || h % 32 != 0 || bw == 0 {
            return Err(format!("native: a transfer touching the main frame: BITBLTBUF {:#x} TRXPOS {:#x} TRXREG {:#x}", self.bitbltbuf, self.trxpos, self.trxreg));
        }
        let mut out = Vec::new();
        for py in (y..y + h).step_by(32) {
            for px in (x..x + w).step_by(64) {
                let page = base / 32 + (py / 32) * bw + px / 64;
                if page < first || page >= last {
                    return Err(format!("native: a transfer of BITBLTBUF {:#x} runs past the main frame", self.bitbltbuf));
                }
                out.push((page - first) as u32);
            }
        }
        Ok(out)
    }
}

/// An IMAGE transfer of the chain: the RAM address of its data, BITBLTBUF's DPSM and TRXREG's width and height, and BITBLTBUF
/// and TRXPOS whole (where in GS memory it goes: `eedraw` writes it there when a texture is sampled beyond it); None for a picture
/// the port puts in a frame itself (`prompts`), which no transfer sends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Upload {
    pub src: u32,
    pub psm: u8,
    pub w: u32,
    pub h: u32,
    pub regs: Option<(u64, u64)>,
}

/// `EePrim::after` of a primitive the chain draws after the effect pass.
pub(crate) const AFTER_EFFECTS: usize = usize::MAX;

/// `EePrim::after` of a primitive the walk meets inside a pass of another VU1 program (`Pass`): the pass's own output, which
/// the emulated pass gives again with the program's, replaces these.
const IN_PASS: usize = usize::MAX - 1;


/// A pass of the moby chain that runs a VU1 program other than the eight of `scripts/uya_vugen.py` (Veldin has none): the water's,
/// slime's and swamp's (`vuwater.rs`, VIF packet at 0x10F500 in the main program) or the sea's (`vusea.rs`, at 0x116070), the
/// port's own code for both. The chain's VIF stream from the block that uploads it to the block that uploads program 6 (or the
/// chain's end), with the GS state when it starts.
pub(crate) struct Pass {
    stream: Vec<u32>,
    marks: Vec<(usize, u32)>,
    gs: EeGs,
    /// The IMAGE transfers of its stream, in order, as the walk read them (their destination block and source address): rac-rs's
    /// VIF1 gives the pass's GS output without addresses.
    images: Vec<(u32, Upload)>,
}

/// Emptied vectors kept from one pass to the next and frame to frame: a pass's VIF stream is most of a job and its GIF output
/// megabytes, and a vector of that size made anew each frame is memory the system takes back when it is freed and faults in
/// again, page by page, when it is made.
struct Spare<T>(std::sync::Mutex<Vec<Vec<T>>>);

impl<T> Spare<T> {
    /// How many a pool keeps (a frame has a pass or two).
    const KEPT: usize = 4;

    const fn new() -> Self {
        Spare(std::sync::Mutex::new(Vec::new()))
    }

    /// An empty vector with the room of one given back, or a new one.
    fn take(&self) -> Vec<T> {
        self.0.lock().expect("a spare vector's lock").pop().unwrap_or_default()
    }

    /// `v` emptied, kept for the next `take`.
    fn give(&self, mut v: Vec<T>) {
        v.clear();
        let mut kept = self.0.lock().expect("a spare vector's lock");
        if kept.len() < Self::KEPT {
            kept.push(v);
        }
    }
}

/// The passes' VIF streams (`PassAt::stream`) and GIF output (`Pass::prims`).
static STREAMS: Spare<u32> = Spare::new();
static QUADS: Spare<[u32; 4]> = Spare::new();

/// The debugging switches, read from the environment once (they are looked at per run, per primitive and per pass).
static EEDUMP: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
static PASS_DUMP: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// A check of each pass's output (`check_passes`): the pass's name (the programs it runs, joined by `+`), its VIF stream and marks,
/// and the port's GIF output, before the output is read.
pub type PassCheck = fn(name: &str, stream: &[u32], marks: &[(usize, u32)], port: &[[u32; 4]]) -> Result<(), String>;
static PASS_CHECK: std::sync::OnceLock<PassCheck> = std::sync::OnceLock::new();

/// A check of each run of program 0 the port replays on its translated VU1 (`check_runs`): VU1's data memory and registers as the
/// run starts, its start (the MSCAL's immediate times 8) and TOP, the port's GIF output, and program 0's microcode.
pub type RunCheck = fn(mem: &[[u32; 4]], vf: &[[u32; 4]; 32], vi: &[u32; 32], start: usize, top: usize, port: &[[u32; 4]], code: &[u8]);
static RUN_CHECK: std::sync::OnceLock<RunCheck> = std::sync::OnceLock::new();

/// Runs `check` on every replayed run of program 0 from now on (`uyaref vucheck`: the same run on rac-rs's VU1). The game sets none.
pub fn check_runs(check: RunCheck) {
    if RUN_CHECK.set(check).is_err() {
        panic!("mobies::check_runs: a run check is set already");
    }
}

/// Runs `check` on every pass from now on: the emulated reference's `uyaref vucheck` (port/uyaref) sets each pass's output beside
/// the same stream run on rac-rs's VIF1 and VU1. The game sets none.
pub fn check_passes(check: PassCheck) {
    if PASS_CHECK.set(check).is_err() {
        panic!("mobies::check_passes: a pass check is set already");
    }
}

fn env_once<'a>(v: &'a std::sync::OnceLock<Option<String>>, name: &str) -> Option<&'a str> {
    v.get_or_init(|| std::env::var(name).ok()).as_deref()
}

/// UYA_EEDUMP: the moby runs and the EE's primitives printed as they are read; UYA_EEDUMP_AT=F,G...: for those fields alone
/// (`dump_field`; every field's is too much output for the game to run on).
pub(crate) fn eedump() -> bool {
    DUMP_FIELD.load(Ordering::Relaxed) || env_once(&EEDUMP, "UYA_EEDUMP").is_some()
}

static DUMP_FIELD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static DUMP_AT: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// The field about to be drawn: `eedump` for it when UYA_EEDUMP_AT names it.
pub(crate) fn dump_field(field: u64) {
    let on = env_once(&DUMP_AT, "UYA_EEDUMP_AT").is_some_and(|v| v.split(',').any(|f| f.parse() == Ok(field)));
    DUMP_FIELD.store(on, Ordering::Relaxed);
}

static LIT_STATS: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();

/// UYA_LIT_STATS: each frame's moby runs lit per pixel and those not, and how near the light terms the per-pixel lighting takes
/// at their vertices are to the skin job's own (`moby_lit`).
fn lit_stats() -> bool {
    env_once(&LIT_STATS, "UYA_LIT_STATS").is_some()
}

/// The per-pixel lighting's count of a frame's runs (`moby_lit`).
#[derive(Default)]
struct LitStats {
    lit: usize,
    /// Runs whose mesh the skin job made no output for this field or the one before (`Mobies::skin_pass`), or whose output is not
    /// what the run uploaded, or whose model transform is singular.
    unskinned: usize,
    differ: usize,
    no_light: usize,
    /// With `lit_stats`: of the runs that differ, their positions more than a unit off, and the difference in vertex counts.
    differ_vertices: [usize; 2],
    /// With `lit_stats`: the light terms from the normals in the world against the skin job's own, summed over the vertices (and
    /// with the normals reversed), and their count.
    error: [f64; 2],
    vertices: usize,
}

#[derive(Default)]
struct PreparedRun {
    lit: Vec<crate::lighting::MobyLit>,
    world: Vec<[f32; 3]>,
    stats: LitStats,
}

/// What the skin job made of one mesh (`Mobies::skin_pass`, `uyafmt::moby::skin_fast`): the positions its run uploads (to within a
/// unit), each vertex's normal in the model's frame (`uyafmt::moby::Skinned::normals`) and its light terms there, the vertices
/// from `flat` on taking the instance's flat colour (packet B's constant normals).
struct SkinOut {
    positions: Vec<[i16; 3]>,
    normals: Vec<[f32; 3]>,
    terms: Vec<[f32; 2]>,
    flat: usize,
    /// The record's light section.
    lights: uyafmt::moby::JobLights,
}

/// Copy the actual positions and transformed normals from the original job. Packet B's
/// staging-copy order is still significant across meshes, even when no transforms are needed.
fn captured_skin(mesh: &uyafmt::moby::Mesh, output: &uyart::hw::SkinOutput, staging: &mut [[f32; 3]]) -> Result<(Vec<[i16; 3]>, Vec<[f32; 3]>), String> {
    use uyafmt::moby::PacketB;
    let mut normals = output.normals.clone();
    match mesh.packet_b.as_ref().expect("decoded packet B") {
        PacketB::Skinned(s) => {
            let n = s.rows.len();
            if normals.len() != n {
                return Err(format!("captured skin at {:#x}: {} normals for {n} rows", output.dest, normals.len()));
            }
            for (r, normal) in normals.iter().enumerate() {
                let j = r + 7;
                let slot = if j < n {
                    s.rows[j].slot as usize
                } else if j - n < s.tail.len() {
                    (s.tail[j - n][0] & 511) as usize
                } else {
                    let lane = j - n - s.tail.len() + 2;
                    let tail = s.tail.last().ok_or("captured skin: missing look-ahead row")?;
                    (*tail.get(lane).ok_or("captured skin: staging slot past look-ahead row")? & 511) as usize
                };
                staging[slot] = *normal;
            }
            normals.extend(s.copies.iter().map(|c| staging[(c >> 7) as usize & 511]));
        }
        PacketB::Rigid(r) if normals.len() != r.rows.len() => {
            return Err(format!("captured rigid skin at {:#x}: {} normals for {} rows", output.dest, normals.len(), r.rows.len()));
        }
        PacketB::Rigid(_) => {}
    }
    let count = mesh.vertices as usize;
    let offset = (4 * count).div_ceil(16) * 16;
    let positions = output.bytes.get(offset..offset + 6 * count).ok_or("captured skin: positions past output transfer")?
        .chunks_exact(6).map(|p| std::array::from_fn(|i| i16::from_le_bytes([p[2 * i], p[2 * i + 1]]))).collect();
    Ok((positions, normals))
}

/// How many skin job outputs are kept at one address (`Mobies::skinned`), newest first: a run takes the one that gives its own
/// positions (`moby_lit`), as an output for the next frame can already be at the address of this frame's (a field that runs two
/// frames, a list buffer met again).
const SKIN_KEPT: usize = 3;

/// How many skin passes an address's outputs stay after the last one written there (`Mobies::skinned`; about 10 s of play). The
/// game puts each field's outputs in its list buffers wherever they fall, so the addresses move from field to field, and kept
/// until the same address came again, the outputs held 369 MB a minute into Veldin and 530 MB after three (the heap census,
/// 487,105 and 702,337 blocks), growing on as long as the level ran. The oldest a frame draws is a few fields' (a scene's cut:
/// more than five), and a paused game runs no passes, so nothing ages while it stands.
const SKIN_AGE: u64 = 600;

/// How many fields' skin job transfers a frame's jobs carry (`rtgame`, `Mobies::skin_pass`): the renderer skins each field's once,
/// those of fields it drew no frame of too (a field's job skins the frame the next field sends, a later one when the game misses
/// fields).
pub(crate) const SKIN_FIELDS: u64 = 8;

/// The inverse of a 3 x 3.
fn invert3(m: [[f64; 3]; 3]) -> Option<[[f64; 3]; 3]> {
    let c = |r: usize, k: usize| m[(r + 1) % 3][(k + 1) % 3] * m[(r + 2) % 3][(k + 2) % 3] - m[(r + 1) % 3][(k + 2) % 3] * m[(r + 2) % 3][(k + 1) % 3];
    let det = m[0][0] * c(0, 0) + m[0][1] * c(0, 1) + m[0][2] * c(0, 2);
    (det.abs() > 1e-30).then(|| std::array::from_fn(|r| std::array::from_fn(|k| c(k, r) / det)))
}

/// The per-pixel lighting's inputs of a moby run's vertices (`lighting::MobyLit`, one per skinned position). The game lights a
/// moby's vertices in its skin job (func_003D00F8, `uyafmt::moby` "The skin job"): the skinned layout's colour is the instance's
/// base plus each light's colour times max(d, d min(wrap, 0)), the rigid layout's 0x70 plus a quarter of each light's colour times
/// max(d, 0), d the vertex's normal (the (cos, sin) table's through its joint matrices) dotted with the light's direction; the
/// lights are the moby's light choice (instance +0x38: sets A and B of the level's light sets and B's weight). `skin` is the skin
/// job's outputs at the run's positions address (`Mobies::skin_pass`, newest first), the normals the game lit it with; the run's
/// is the one whose positions are those the run uploaded (to within a unit: `uyafmt::moby::skin_fast`). The normals go into the world through the inverse of the model's 3 x 3 (the run's rows
/// through the camera's inverse: normals turn by the inverse transpose of what turns positions). The two lights come from the same skin job. No matched output, a singular model transform,
/// or a vertex of the mesh's flat colour, and the vertex is not
/// lit per pixel. They are written into `out`, which is emptied first, one per position.
#[allow(clippy::too_many_arguments)]
fn moby_lit(run_index: u32, light_rows: &mut [[f32; 4]; 4], stats: &mut LitStats, u: &[[f64; 4]; 4], ci: &[[f64; 4]; 4], run: &Run, skin: &[std::sync::Arc<SkinOut>], out: &mut Vec<crate::lighting::MobyLit>) {
    cpu_wide(|| moby_lit_of(run_index, light_rows, stats, u, ci, run, skin, out))
}

/// `f` compiled for the CPU's SSE4.1 and AVX2 when it has them (the build's baseline has neither: `trunc`, `floor` and `round`
/// are then calls into the C runtime, one per value). The same IEEE operations either way, so the same values.
#[inline(always)]
fn cpu_wide<R>(f: impl FnOnce() -> R) -> R {
    #[cfg(target_arch = "x86_64")]
    {
        #[target_feature(enable = "avx2")]
        unsafe fn wide<R>(f: impl FnOnce() -> R) -> R {
            f()
        }
        if std::arch::is_x86_feature_detected!("avx2") {
            // SAFETY: the CPU has AVX2 (checked above).
            return unsafe { wide(f) };
        }
    }
    f()
}

#[allow(clippy::too_many_arguments)]
#[inline(always)]
fn moby_lit_of(run_index: u32, light_rows: &mut [[f32; 4]; 4], stats: &mut LitStats, u: &[[f64; 4]; 4], ci: &[[f64; 4]; 4], run: &Run, skin: &[std::sync::Arc<SkinOut>], out: &mut Vec<crate::lighting::MobyLit>) {
    use crate::lighting::{LIT, MOBY, MOBY_RIGID, MobyLit};
    let n = run.vu.positions.len();
    out.clear();
    if skin.is_empty() {
        stats.unskinned += 1;
        out.resize(n, MobyLit::default());
        return;
    }
    // The run's own: every position the one it uploaded, to within the unit the VU's truncation can move it.
    let near = |a: &[i16; 3], b: &[i16; 3]| (0..3).all(|k| (a[k] as i32 - b[k] as i32).abs() <= 1);
    let fits = |sk: &SkinOut| sk.positions.len() == run.vu.positions.len() && sk.positions.iter().zip(run.vu.positions.iter()).all(|(a, b)| near(a, b));
    let Some(sk) = skin.iter().find(|sk| fits(sk)) else {
        stats.differ += 1;
        if lit_stats() {
            let sk = &skin[0];
            stats.differ_vertices[0] += sk.positions.iter().zip(run.vu.positions.iter()).filter(|(a, b)| !near(a, b)).count();
            stats.differ_vertices[1] += sk.positions.len().abs_diff(run.vu.positions.len());
        }
        out.resize(n, MobyLit::default());
        return;
    };
    // Model to world: the run's rows through the camera's inverse (`world_at` without the GS's snap), rows 0 to 2.
    let m: [[f64; 3]; 3] = std::array::from_fn(|j| std::array::from_fn(|k| (0..4).map(|q| u[j][q] * ci[q][k]).sum::<f64>() / 1024.0));
    let Some(inv) = invert3(m) else {
        stats.no_light += 1;
        out.resize(n, MobyLit::default());
        return;
    };
    // The skin job already contains the instance's two lights, including blends and runtime colours. Recover
    // their world directions from the draw transform. Inferring a level-table index here made all material
    // lighting switch off if either reconstructed direction differed by two degrees during animation.
    let lights = [0, 1].map(|k| {
        let mut colour = sk.lights.colours[k];
        colour[3] = sk.lights.wrap[k];
        let direction: [f32; 3] = std::array::from_fn(|q| {
            -(0..3).map(|j| m[j][q] * sk.lights.dirs[j][k] as f64).sum::<f64>() as f32
        });
        let [x, y, z] = crate::lighting::renormal(direction);
        light_rows[k * 2] = colour;
        light_rows[k * 2 + 1] = [x, y, z, 0.0];
        (colour, [x, y, z])
    });
    stats.lit += 1;
    let rigid = run.vu.uvs.is_some();
    let word = run_index | if rigid { MOBY_RIGID } else { MOBY } | LIT;
    let stats_on = lit_stats();
    // What `terms_of` makes of the lights for each vertex, made once: each light's wrap and its direction renormalised.
    let light_dirs = lights.map(|(c, d)| (c[3], crate::lighting::renormal(d)));
    let pass = word & (3 << 28);
    let term = |e: f32, wrap: f32| -> f32 {
        match pass {
            crate::lighting::TIE => e.max(e * wrap),
            MOBY => e.max(e * wrap.min(0.0)),
            _ => e.max(0.0),
        }
    };
    let count = sk.normals.len().min(sk.terms.len());
    let mut done = 0;
    // Four vertices at a time, each operation of the one-at-a-time code below in its order on each (so that the compiler can make
    // SIMD operations of them, which are the same IEEE operations on each lane).
    while !stats_on && done + 4 <= count {
        let v = &sk.normals[done..done + 4];
        let d: [[f64; 4]; 3] = std::array::from_fn(|c| std::array::from_fn(|j| v[j][c] as f64));
        let w: [[f32; 4]; 3] = std::array::from_fn(|k| std::array::from_fn(|j| -(inv[k][0] * d[0][j] + inv[k][1] * d[1][j] + inv[k][2] * d[2][j]) as f32));
        let len2: [f32; 4] = std::array::from_fn(|j| w[0][j] * w[0][j] + w[1][j] * w[1][j] + w[2][j] * w[2][j]);
        let root: [f32; 4] = std::array::from_fn(|j| len2[j].sqrt());
        let rn: [[f32; 4]; 3] = std::array::from_fn(|k| std::array::from_fn(|j| if len2[j] > 0.0 { w[k][j] / root[j] } else { 0.0 }));
        let s: [[f32; 4]; 2] = std::array::from_fn(|k| {
            let (wrap, dir) = light_dirs[k];
            std::array::from_fn(|j| term(rn[0][j] * dir[0] + rn[1][j] * dir[1] + rn[2][j] * dir[2], wrap))
        });
        let n: [[i8; 4]; 3] = std::array::from_fn(|c| std::array::from_fn(|j| (rn[c][j] * 127.0).round() as i8));
        let share = (1.0f32 * 127.0).round() as i8;
        for j in 0..4 {
            let zero = w[0][j] == 0.0 && w[1][j] == 0.0 && w[2][j] == 0.0;
            out.push(if done + j >= sk.flat || zero { MobyLit::default() } else { MobyLit { n: [n[0][j], n[1][j], n[2][j], share], s: [s[0][j], s[1][j]], light: word } });
        }
        done += 4;
    }
    out.extend(sk.normals[done..count].iter().zip(&sk.terms[done..count]).enumerate().map(|(i, (v, game))| {
        let i = done + i;
        // Into the world, reversed: the skin job dots the directions turned into the model's frame and negated.
        let w = [0, 1, 2].map(|k| -(inv[k][0] * v[0] as f64 + inv[k][1] * v[1] as f64 + inv[k][2] * v[2] as f64) as f32);
        if i >= sk.flat || w == [0.0; 3] {
            return MobyLit::default();
        }
        // `terms_of` and `snorm` of `w`, which renormalise it each: once.
        let rn = crate::lighting::renormal(w);
        let s: [f32; 2] = [0, 1].map(|k| {
            let (wrap, d) = light_dirs[k];
            term(rn[0] * d[0] + rn[1] * d[1] + rn[2] * d[2], wrap)
        });
        if stats_on {
            let back = crate::lighting::terms_of(&lights, word, w.map(|x| -x));
            for (e, s) in stats.error.iter_mut().zip([s, back]) {
                *e += ((s[0] - game[0]).abs() + (s[1] - game[1]).abs()) as f64 / 2.0;
            }
            stats.vertices += 1;
        }
        let share = 1.0f32;
        MobyLit { n: [(rn[0] * 127.0).round() as i8, (rn[1] * 127.0).round() as i8, (rn[2] * 127.0).round() as i8, (share * 127.0).round() as i8], s, light: word }
    }));
}

impl Pass {
    /// The pass's GS output, each piece of its stream run by the port's own code for the program it uploads (`vif.rs` walks it),
    /// read as `EeGs` reads the chain's own packets: its triangles, each `after` the given run count, onto `out`; and the held Q
    /// it leaves. A piece that uploads a program the port does not do is an error (what it put onto `out` before is the
    /// caller's to take off). The pass is spent: its stream's room goes back to `STREAMS`.
    fn prims(mut self, after: usize, out: &mut Vec<(usize, EePrim)>) -> Result<f32, String> {
        let mut qs = QUADS.take();
        let r = self.read(after, &mut qs, out);
        QUADS.give(qs);
        STREAMS.give(self.stream);
        r
    }

    /// `prims`, the pass's GIF output made in `qs` (a vector with room from the passes before).
    fn read(&mut self, after: usize, qs: &mut Vec<[u32; 4]>, out: &mut Vec<(usize, EePrim)>) -> Result<f32, String> {
        if let Some(dir) = env_once(&PASS_DUMP, "UYA_PASS_DUMP") {
            let n = std::fs::read_dir(&dir).map(|d| d.count()).unwrap_or(0);
            let words: Vec<u8> = self.stream.iter().flat_map(|w| w.to_le_bytes()).collect();
            std::fs::write(format!("{dir}/pass{n:03}.bin"), words).map_err(|e| format!("{dir}: {e}"))?;
        }
        let pieces = crate::vif::pieces(&self.stream)?;
        // The VU's clip flags through the pass: a VU1 that has judged nothing before it (what came before the pass is not the
        // port's; only a strip's second vertex's ADC in the water reads that far back, and the GS draws no triangle there).
        let mut clip = 0u32;
        for p in &pieces {
            let stream = &self.stream[p.range.clone()];
            match p.program {
                Some(crate::vif::Program::Water) => crate::vif::run(stream, &mut crate::vuwater::Water::default(), &mut clip, qs)?,
                Some(crate::vif::Program::Sea) => crate::vif::run(stream, &mut crate::vusea::Sea, &mut clip, qs)?,
                None => return Err(format!("native: a moby pass uploads a VU1 program the port does not do (words {}..{} of its stream)", p.range.start, p.range.end)),
            }
        }
        if let Some(check) = PASS_CHECK.get() {
            let names: Vec<String> = pieces.iter().map(|p| format!("{:?}", p.program.expect("matched above")).to_lowercase()).collect();
            check(&names.join("+"), &self.stream, &self.marks, qs)?;
        }
        let gs = &mut self.gs;
        gs.gif_out(qs, after, out, &mut self.images.iter().copied())?;
        Ok(gs.held_q)
    }
}

/// The GS registers a field's jobs leave for the next field's (the GS keeps them from frame to frame): the two contexts' and
/// TEXA (None: no job has set it).
#[derive(Clone, Copy)]
pub(crate) struct GsLeft {
    pub ctx: [Ctx; 2],
    pub texa: Option<u64>,
    /// FOGCOL (0x3D) as the jobs last wrote it: the colour the world programs' fogged pixels go to (`Walk::fog`).
    pub fogcol: Option<u64>,
}

impl GsLeft {
    /// What a field after fields the walk did not read starts from: the contexts as the moby pass starts them (`EeGs::start`),
    /// TEXA as the last field read left it.
    pub fn after_gap(self) -> GsLeft {
        GsLeft { ctx: EeGs::start().ctx, texa: self.texa, fogcol: self.fogcol }
    }
}

/// One drawing context's registers: FRAME, TEST, ALPHA, TEX0, CLAMP, TEX1, XYOFFSET, SCISSOR, and MIPTBP1 and MIPTBP2.
#[derive(Clone, Copy, Default)]
pub(crate) struct Ctx {
    pub frame: u64,
    pub zbuf: u64,
    pub test: u64,
    pub alpha: u64,
    pub tex0: u64,
    pub clamp: u64,
    pub tex1: u64,
    pub xyoffset: u64,
    pub scissor: u64,
    pub mip: [u64; 2],
}

/// The GS registers the chain's own packets write, and the vertex queue of the current primitive.
#[derive(Clone)]
pub(crate) struct EeGs {
    prim: u64,
    /// Contexts 1 and 2.
    ctx: [Ctx; 2],
    bitbltbuf: u64,
    trxpos: u64,
    trxreg: u64,
    /// The transfers so far, and how many passes the walk has started (`Transfer::passes`).
    transfers: Vec<Transfer>,
    passes: usize,
    /// Each IMAGE transfer, by its destination block (BITBLTBUF DBP), and every one in order.
    uploads: HashMap<u32, Upload>,
    /// The last primitive's TEX0 and the uploads its texels and CLUT came from (`vertex`), until an upload changes them.
    upload_cache: Option<(u64, Option<Upload>, Option<Upload>)>,
    images: Vec<(u32, Upload)>,
    /// An IMAGE transfer's data still to come (it may continue into the next DIRECT): quadwords left, destination block, whether
    /// its data has started, and the transfer.
    image_left: usize,
    image_dbp: u32,
    image_started: bool,
    image_up: Upload,
    rgba: [u8; 4],
    q: f32,
    texa: Option<u64>,
    fogcol: Option<u64>,
    /// PACKED ST keeps its Q for the next PACKED RGBAQ (the GIF's, whatever path writes next), and the PACKED ST writes so far.
    held_q: f32,
    st_writes: u64,
    st: [f32; 2],
    uv: [f32; 2],
    queue: [GsVertex; 3],
    /// How many of `queue` are the current primitive's so far.
    queued: usize,
}

impl EeGs {
    /// The GS state when the moby pass starts (the traced frame's stream): FRAME_1 0x800F4, ALPHA_1 0x8044 and TEST_1 0x5B00C.
    pub(crate) fn start() -> EeGs {
        let c1 = Ctx { frame: 0x800F4, test: TEST_OPEN, alpha: 0x8044, ..Ctx::default() };
        EeGs { prim: 0, ctx: [c1, Ctx::default()], bitbltbuf: 0, trxpos: 0, trxreg: 0, transfers: Vec::new(), passes: 0, uploads: HashMap::new(), upload_cache: None, images: Vec::new(), image_left: 0, image_dbp: 0, image_started: false, image_up: Upload { src: 0, psm: 0, w: 0, h: 0, regs: None }, rgba: [0; 4], q: 1.0, texa: None, fogcol: None, held_q: 1.0, st_writes: 0, st: [0.0; 2], uv: [0.0; 2], queue: [NO_VERTEX; 3], queued: 0 }
    }

    /// The GIF data of one DIRECT (`n` quadwords; `qw(q)` its quadword q, `addr(q)` its EE address): register writes, the EE's
    /// own primitives (into `out`, at `place`) and IMAGE transfers (whose data may continue into the next DIRECT). `ad` sees each
    /// A+D write.
    fn direct(&mut self, n: usize, qw: impl Fn(usize) -> Result<[u32; 4], String>, addr: impl Fn(usize) -> u32, place: usize, out: &mut Vec<(usize, EePrim)>, mut ad: impl FnMut(u64, u64)) -> Result<(), String> {
        // An IMAGE transfer's data from the DIRECT before: its source is this DIRECT's first quadword.
        if self.image_left > 0 && !self.image_started {
            self.upload(self.image_dbp, Upload { src: addr(0), ..self.image_up });
            self.images.push((self.image_dbp, Upload { src: addr(0), ..self.image_up }));
            self.image_of_transfer(Upload { src: addr(0), ..self.image_up });
            self.image_started = true;
        }
        let mut q = self.image_left.min(n);
        self.image_left -= q;
        while q < n {
            let t = qw(q)?;
            let (lo, hi) = (t[0] as u64 | (t[1] as u64) << 32, t[2] as u64 | (t[3] as u64) << 32);
            let (nloop, flg) = ((lo & 0x7fff) as usize, (lo >> 58) & 3);
            let nreg = match (lo >> 60) & 0xf {
                0 => 16,
                r => r as usize,
            };
            q += 1;
            let size = match flg {
                0 => nloop * nreg,
                1 => (nloop * nreg).div_ceil(2),
                _ => nloop,
            };
            if flg != 2 && q + size > n {
                return Err(format!("native: a GIF packet continues past its DIRECT at {:#x}", addr(q - 1)));
            }
            // PRE: the tag's PRIM field is written first.
            if lo >> 46 & 1 == 1 && flg != 2 {
                self.write(0x00, lo >> 47 & 0x7ff, place, out);
            }
            match flg {
                // REGLIST: a 64-bit register value each, two per quadword.
                1 => {
                    for l in 0..nloop {
                        for r in 0..nreg {
                            let k = l * nreg + r;
                            let d = qw(q + k / 2)?;
                            let v = if k % 2 == 0 { d[0] as u64 | (d[1] as u64) << 32 } else { d[2] as u64 | (d[3] as u64) << 32 };
                            self.write(hi >> (4 * r) & 0xf, v, place, out);
                        }
                    }
                    q += size;
                }
                // PACKED with registers other than A+D: PRIM, RGBAQ, ST, XYZ2 ...
                0 if (0..nreg).any(|r| (hi >> (4 * r)) & 0xf != 0xe) => {
                    for l in 0..nloop {
                        for r in 0..nreg {
                            self.write_packed(hi >> (4 * r) & 0xf, qw(q + l * nreg + r)?, place, out);
                        }
                    }
                    q += size;
                }
                0 => {
                    for _ in 0..nloop * nreg {
                        let d = qw(q)?;
                        let (val, reg) = (d[0] as u64 | (d[1] as u64) << 32, (d[2] & 0xff) as u64);
                        self.write(reg, val, place, out);
                        ad(reg, val);
                        q += 1;
                    }
                }
                // IMAGE: host to local transfer data to BITBLTBUF's DBP (TRXDIR 0), here or in the next DIRECT.
                _ => {
                    self.image_dbp = (self.bitbltbuf >> 32 & 0x3fff) as u32;
                    self.image_up = Upload { src: 0, psm: (self.bitbltbuf >> 56 & 0x3f) as u8, w: (self.trxreg & 0xfff) as u32, h: (self.trxreg >> 32 & 0xfff) as u32, regs: Some((self.bitbltbuf, self.trxpos)) };
                    self.image_started = q < n;
                    if self.image_started {
                        self.upload(self.image_dbp, Upload { src: addr(q), ..self.image_up });
                        self.images.push((self.image_dbp, Upload { src: addr(q), ..self.image_up }));
                        self.image_of_transfer(Upload { src: addr(q), ..self.image_up });
                    }
                    let here = nloop.min(n - q);
                    self.image_left = nloop - here;
                    q += here;
                }
            }
        }
        Ok(())
    }

    /// The GIF packets a VU1 program kicked (`qs`: the tags and their data, quadword by quadword), read as the GS reads them: the
    /// register writes and the primitives (`after` the given run count) onto `out`. An IMAGE packet takes the next of `images`
    /// (the walk's transfers of a pass's stream, whose output carries none of its own addresses).
    fn gif_out(&mut self, qs: &[[u32; 4]], after: usize, out: &mut Vec<(usize, EePrim)>, images: &mut dyn Iterator<Item = (u32, Upload)>) -> Result<(), String> {
        let mut k = 0;
        while k < qs.len() {
            let t = qs[k];
            let (lo, hi) = (t[0] as u64 | (t[1] as u64) << 32, t[2] as u64 | (t[3] as u64) << 32);
            let (nloop, flg) = ((lo & 0x7fff) as usize, (lo >> 58) & 3);
            let nreg = match (lo >> 60) & 0xf {
                0 => 16,
                r => r as usize,
            };
            k += 1;
            if lo >> 46 & 1 == 1 && flg != 2 {
                self.write(0x00, lo >> 47 & 0x7ff, after, out);
            }
            // The tag's register descriptors, one each.
            let regs: [u64; 16] = std::array::from_fn(|r| hi >> (4 * r) & 0xf);
            match flg {
                0 => {
                    let data = qs.get(k..k + nloop * nreg).ok_or("native: the VU1 pass's GIF packet past its output")?;
                    for loop_data in data.chunks_exact(nreg) {
                        for (&reg, &d) in regs.iter().zip(loop_data) {
                            self.write_packed(reg, d, after, out);
                        }
                    }
                    k += nloop * nreg;
                }
                1 => {
                    let data = qs.get(k..k + (nloop * nreg).div_ceil(2)).ok_or("native: the VU1 pass's GIF packet past its output")?;
                    let value = |j: usize| {
                        let d = data[j / 2];
                        if j % 2 == 0 { d[0] as u64 | (d[1] as u64) << 32 } else { d[2] as u64 | (d[3] as u64) << 32 }
                    };
                    for l in 0..nloop {
                        for r in 0..nreg {
                            self.write(regs[r], value(l * nreg + r), after, out);
                        }
                    }
                    k += (nloop * nreg).div_ceil(2);
                }
                // IMAGE: the walk's next transfer of the pass, to BITBLTBUF's DBP.
                _ => {
                    let (dbp, up) = images.next().ok_or("native: the VU1 pass's GS output has more IMAGE transfers than its stream")?;
                    if dbp != (self.bitbltbuf >> 32 & 0x3fff) as u32 {
                        return Err(format!("native: the VU1 pass's IMAGE transfer to block {:#x}, the walk's to {dbp:#x}", self.bitbltbuf >> 32 & 0x3fff));
                    }
                    self.upload(dbp, up);
                    k += nloop;
                }
            }
        }
        Ok(())
    }

    /// A register write (64-bit value as REGLIST and A+D give it); a drawing kick appends the primitive to `out`.
    fn write(&mut self, reg: u64, v: u64, after: usize, out: &mut Vec<(usize, EePrim)>) {
        match reg {
            0x00 => {
                self.prim = v & 0x7ff;
                self.queued = 0;
            }
            0x01 => {
                self.rgba = [v as u8, (v >> 8) as u8, (v >> 16) as u8, (v >> 24) as u8];
                self.q = f32::from_bits((v >> 32) as u32);
            }
            0x02 => self.st = [f32::from_bits(v as u32), f32::from_bits((v >> 32) as u32)],
            // UV: U and V in 1/16 texel (bits 0-13, 16-29).
            0x03 => self.uv = [(v & 0x3fff) as f32 / 16.0, (v >> 16 & 0x3fff) as f32 / 16.0],
            0x04 | 0x0c => self.vertex(v, ((v >> 32) & 0xff_ffff) as f64, (v >> 56) as u32, reg == 0x04, after, out),
            0x05 | 0x0d => self.vertex(v, (v >> 32) as u32 as f64, 0, reg == 0x05, after, out),
            // Each context's register pair: context 1's number, context 2's the next.
            0x06 | 0x07 => self.ctx[(reg - 0x06) as usize].tex0 = v,
            // TEX2: TEX0's PSM and CLUT fields (CBP, CPSM, CSM, CSA, CLD) alone.
            0x16 | 0x17 => {
                let mask = 0x3f << 20 | !0u64 << 37;
                let c = &mut self.ctx[(reg - 0x16) as usize];
                c.tex0 = c.tex0 & !mask | v & mask;
            }
            0x08 | 0x09 => self.ctx[(reg - 0x08) as usize].clamp = v,
            0x14 | 0x15 => self.ctx[(reg - 0x14) as usize].tex1 = v,
            0x34 | 0x35 => self.ctx[(reg - 0x34) as usize].mip[0] = v,
            0x36 | 0x37 => self.ctx[(reg - 0x36) as usize].mip[1] = v,
            0x18 | 0x19 => self.ctx[(reg - 0x18) as usize].xyoffset = v,
            0x40 | 0x41 => self.ctx[(reg - 0x40) as usize].scissor = v,
            0x50 => self.bitbltbuf = v,
            0x51 => self.trxpos = v,
            0x52 => self.trxreg = v,
            0x53 => self.transfers.push(Transfer { dir: v & 3, bitbltbuf: self.bitbltbuf, trxpos: self.trxpos, trxreg: self.trxreg, index: out.len(), after, passes: self.passes, image: None }),
            0x42 | 0x43 => self.ctx[(reg - 0x42) as usize].alpha = v,
            0x47 | 0x48 => self.ctx[(reg - 0x47) as usize].test = v,
            0x3b => self.texa = Some(v),
            0x3d => self.fogcol = Some(v),
            0x4c | 0x4d => self.ctx[(reg - 0x4c) as usize].frame = v,
            0x4e | 0x4f => self.ctx[(reg - 0x4e) as usize].zbuf = v,
            _ => {}
        }
    }

    /// A PACKED register write (`d` the quadword).
    fn write_packed(&mut self, reg: u64, d: [u32; 4], after: usize, out: &mut Vec<(usize, EePrim)>) {
        let xy = (d[0] & 0xffff) as u64 | ((d[1] & 0xffff) as u64) << 16;
        let adc = d[3] >> 15 & 1 == 1;
        match reg {
            0x0 => self.write(0x00, (d[0] & 0x7ff) as u64, after, out),
            0x1 => {
                let v = (d[0] & 0xff) as u64 | ((d[1] & 0xff) as u64) << 8 | ((d[2] & 0xff) as u64) << 16 | ((d[3] & 0xff) as u64) << 24 | (self.held_q.to_bits() as u64) << 32;
                self.write(0x01, v, after, out);
            }
            0x3 => self.write(0x03, (d[0] & 0x3fff) as u64 | ((d[1] & 0x3fff) as u64) << 16, after, out),
            0x2 => {
                self.held_q = f32::from_bits(d[2]);
                self.st_writes += 1;
                self.write(0x02, d[0] as u64 | (d[1] as u64) << 32, after, out);
            }
            0x4 => self.write(if adc { 0x0c } else { 0x04 }, xy | ((d[2] >> 4 & 0xff_ffff) as u64) << 32 | ((d[3] >> 4 & 0xff) as u64) << 56, after, out),
            0x5 => self.write(if adc { 0x0d } else { 0x05 }, xy | (d[2] as u64) << 32, after, out),
            0xe => self.write((d[2] & 0xff) as u64, d[0] as u64 | (d[1] as u64) << 32, after, out),
            _ => {}
        }
    }

    /// The IMAGE `up` as the data of the host to local transfer TRXDIR started last (`Transfer::image`).
    fn image_of_transfer(&mut self, up: Upload) {
        if let Some(t) = self.transfers.last_mut().filter(|t| t.dir == 0 && t.image.is_none()) {
            t.image = Some(up);
        }
    }

    /// An IMAGE transfer to block `dbp`.
    fn upload(&mut self, dbp: u32, up: Upload) {
        self.uploads.insert(dbp, up);
        self.upload_cache = None;
    }

    fn vertex(&mut self, v: u64, z: f64, f: u32, kick: bool, after: usize, out: &mut Vec<(usize, EePrim)>) {
        let ty = self.prim & 7;
        // A primitive has at most 3 vertices, and the queue is emptied (or cut to its last ones) each time one has them.
        self.queue[self.queued] = GsVertex { s: self.st[0], t: self.st[1], q: self.q, rgba: self.rgba, x: (v & 0xffff) as f64 / 16.0, y: (v >> 16 & 0xffff) as f64 / 16.0, z, f, u: self.uv[0], v: self.uv[1] };
        self.queued += 1;
        let need = match ty {
            0 => 1,
            1 | 2 | 6 => 2,
            _ => 3,
        };
        if self.queued < need {
            return;
        }
        if kick {
            let c = self.ctx[(self.prim >> 9 & 1) as usize];
            let (texels, clut) = match self.upload_cache {
                Some((t, a, b)) if t == c.tex0 => (a, b),
                _ => {
                    let a = self.uploads.get(&((c.tex0 & 0x3fff) as u32)).copied();
                    let b = self.uploads.get(&((c.tex0 >> 37 & 0x3fff) as u32)).copied();
                    self.upload_cache = Some((c.tex0, a, b));
                    (a, b)
                }
            };
            out.push((0, EePrim {
                after,
                prim: self.prim,
                frame: c.frame,
                zbuf: c.zbuf,
                test: c.test,
                alpha: c.alpha,
                tex0: c.tex0,
                clamp: c.clamp,
                tex1: c.tex1,
                mip: c.mip,
                xyoffset: c.xyoffset,
                scissor: c.scissor,
                xyoffset1: self.ctx[0].xyoffset,
                texels,
                clut,
                texa: self.texa,
                verts: Verts { n: self.queued as u8, v: self.queue },
                fill: false,
            }));
        }
        match ty {
            // A line strip and a triangle strip keep their last vertices, a fan its first and last.
            2 | 4 => {
                self.queue.copy_within(1.., 0);
                self.queued -= 1;
            }
            5 => {
                self.queue[1] = self.queue[2];
                self.queued -= 1;
            }
            _ => self.queued = 0,
        }
    }
}

/// The microcode addresses of the eight VU1 programs' MPG packets in the main program (`scripts/uya_vugen.py`; program 6's is
/// `uyafmt::prog6::MICROCODE`). An MPG that uploads any other microcode (the water's at 0x10F500, the one at 0x116074) starts a
/// pass the port runs on rac-rs's VU1 (`Pass`).
pub(crate) const PROGRAMS: [(u8, &[u32]); 8] = [
    (0, &[0x11_0798, 0x11_0FA0, 0x11_17A8, 0x11_1FB0, 0x11_27B8, 0x11_2FC0]),
    (1, &[0x10_3688, 0x10_3E90, 0x10_4698, 0x10_4EA0, 0x10_56A8, 0x10_5EB0, 0x10_66B8, 0x10_6EC0]),
    (2, &[0x10_8F38, 0x10_9740, 0x10_9F48, 0x10_A750, 0x10_AF58]),
    (3, &[0x10_B338, 0x10_BB40, 0x10_C348, 0x10_CB50, 0x10_D358, 0x10_DB60, 0x10_E368, 0x10_EB70]),
    (4, &[0x10_1868, 0x10_2070]),
    (5, &[0x10_23F8, 0x10_2C00, 0x10_3408]),
    (6, &[uyafmt::prog6::MICROCODE]),
    (7, &[0x10_7138, 0x10_7940, 0x10_8148, 0x10_8950]),
];

/// Each program piece's first bytes in the main program (`PROGRAMS`' packets and the microcode after their tags), for
/// `program_of` (`note_programs`, made once from the main program).
static PROGRAM_CODE: std::sync::OnceLock<Vec<(u8, Vec<u8>)>> = std::sync::OnceLock::new();

/// The bytes `program_of` looks for each piece's microcode in: its packet's first 96 bytes (the tag, then the code).
const PROGRAM_WINDOW: usize = 96;

/// Notes each program piece's first bytes from the main program `boot` (`program_of`), once.
pub(crate) fn note_programs(boot: &uyafmt::boot::Boot) {
    if PROGRAM_CODE.get().is_some() {
        return;
    }
    let mut v = Vec::new();
    for &(p, pieces) in &PROGRAMS {
        for &a in pieces {
            // A piece outside the main program's records (none is known to be) is matched by its address alone.
            if let Ok(b) = boot.bytes(a, PROGRAM_WINDOW) {
                v.push((p, b.to_vec()));
            }
        }
    }
    let _ = PROGRAM_CODE.set(v);
}

/// The program an MPG uploads: by its packet's address in the main program (`PROGRAMS`), else by its microcode (`code`, the
/// words after the MPG) found in a piece's first bytes. The game sends a program's upload from the main program's packet, but
/// a chain the DMA read through a copy (a kick neither a thread's nor its end handler's, as the first frames after the pause
/// menu closes can be) names the copy's addresses: by address alone the upload was no program, the runs after it were credited
/// to none, and the level's ties and effects pass were not drawn until an upload came from the main program again (the player
/// on Daxx, 28 November 2026: "When I click menu and then close it objects dissapear", "Half the world objects are gone"; the
/// frame builder had 0 ties for those frames, the effects target nothing, both back together seconds later).
pub(crate) fn program_of(packet: Option<u32>, code: &[u32]) -> Option<u8> {
    if let Some(p) = packet.and_then(|a| PROGRAMS.iter().find(|(_, c)| c.contains(&a)).map(|&(p, _)| p)) {
        return Some(p);
    }
    let need: Vec<u8> = code.iter().take(8).flat_map(|w| w.to_le_bytes()).collect();
    if need.len() < 32 {
        return None;
    }
    PROGRAM_CODE.get()?.iter().find(|(_, w)| w.windows(32).step_by(4).any(|x| x == &need[..])).map(|&(p, _)| p)
}

/// `EePrim::after` of a primitive before the frame's first program upload: the frame's clear, the copy of the frame before to
/// the display buffer and the sky, which the native frame draws itself.
const BEFORE_WORLD: usize = usize::MAX - 2;

/// What wrote the GIF's held Q (a PACKED ST's Q, which the next PACKED RGBAQ takes) last: program 6's first packet takes it
/// (`effects::Effects::frame`). The EE's own packets or a pass's output give it; a moby run's is its last vertex's.
#[derive(Clone, Copy, Debug)]
pub(crate) enum HeldQ {
    Value(f32),
    Run(usize),
    Pass(usize),
}

/// What the frame's jobs send, read in kick order for the native frame: the moby runs, the EE's own primitives and the output of
/// the passes of programs the port runs on rac-rs's VU1, in the order the GS gets them, and what could not be read.
pub(crate) struct Walk {
    runs: Vec<Run>,
    constants: [[f32; 4]; 6],
    /// Whether the moby chain's constants were read (`constants`; `fog`), and FOGCOL as the GS had it when the frame's first
    /// world program (any but 0 and 6) ran, or else when the chain began (a frame can set it again between them: Daxx's water).
    chain_constants: bool,
    world_fogcol: Option<u64>,
    /// The EE's own primitives and the passes' output, in the order the GS gets them, each with the moby draw it comes before
    /// (`Mobies::frame` sets it in place; the walk leaves 0): the pairs `eedraw::build` takes, made where the primitives are read
    /// (the sea's 20,000 a frame are not moved again).
    pub ee: Vec<(usize, EePrim)>,
    /// What stopped the reading of a job (the job's rest is not read).
    pub stopped: Vec<String>,
    /// The GIF's held Q when program 6's upload starts the effect pass (None: nothing the walk read wrote one).
    pub fx_q: Option<HeldQ>,
    /// Which of the eight programs ran (an MSCAL while it was the one uploaded last): the native frame draws a world layer only
    /// when its programs ran (a menu screen's frame runs none).
    pub ran: [bool; 8],
    /// The transfers into and out of the main frame (`Transfer::main_pages`), `index` among `ee` as drawn.
    pub transfers: Vec<Transfer>,
    /// The GS registers as the jobs leave them (the next field's `start`).
    pub gs_end: GsLeft,
    /// Each pass's output (`Pass`: the water's and the sea's programs), a range of `ee` (the water reflections' plane and
    /// surfaces, `reflections.rs`).
    pub passes: Vec<std::ops::Range<usize>>,
    /// Every IMAGE transfer of the frame's jobs in order, by destination block (`Run::images` counts them).
    images: Vec<(u32, Upload)>,
    /// The data words of the chain's UNPACKs (`RawUnpack::words`).
    unpack_words: Vec<u32>,
}

impl Walk {
    /// Whether the frame's jobs hold moby runs.
    pub fn has_runs(&self) -> bool {
        !self.runs.is_empty()
    }

    /// The fog the frame's world was drawn with, as the jobs gave it to the GS. The moby program's constants (the chain's
    /// first block) hold F's floor (vf24 x), its slope along the view depth (vf23 x) and F at depth 0 (vf19 w): the program
    /// fogs with F = clamp(F0 + slope x depth, floor, 255), as the world's other programs do. The view setup
    /// (func_L10_003E3160 in every level) makes them from the fog's near and far depths and F values in the view struct, the
    /// slope as (F far - F near) x the near clip (`near_clip`, the view struct's +0xA0) / (far - near) with the depths x 1024.
    /// FOGCOL is the GS register as the jobs (or the fields before) left it when the world's programs ran. A level's code can
    /// set a fog for the frame's drawing alone and put the level's back before the field ends: Zeldrin's self-destruct alarm
    /// (func_L10_003B8468) sets a red FOGCOL and an F pulsing over the whole view, and the view struct (`view`, read at the
    /// field's end) holds the level's fog again. `view` is the frame's fog where the jobs do not say: a frame without the moby
    /// chain, or FOGCOL before the jobs first set it.
    pub fn fog(&self, view: uyafmt::gameplay::Fog, near_clip: f32) -> uyafmt::gameplay::Fog {
        if !self.chain_constants {
            return view;
        }
        let colour = self.world_fogcol.map_or(view.colour, |c| [c as u8, (c >> 8) as u8, (c >> 16) as u8]);
        let (floor, f0) = (self.constants[0][0], self.constants[5][3]);
        let slope = self.constants[1][0] * 1024.0 / near_clip;
        let near_f = f0.min(255.0);
        if slope == 0.0 {
            let f = near_f.max(floor);
            return uyafmt::gameplay::Fog { colour, near: 0.0, far: 1.0, near_f: f, far_f: f };
        }
        uyafmt::gameplay::Fog { colour, near: (near_f - f0) / slope, far: (floor - f0) / slope, near_f, far_f: floor }
    }
}

/// A pass being read: its VIF stream and marks so far, the GS state when it starts, where its output goes in the walk's
/// primitives and at which place.
struct PassAt {
    stream: Vec<u32>,
    marks: Vec<(usize, u32)>,
    gs: EeGs,
    /// The walk's IMAGE transfers before the pass.
    images: usize,
    at: usize,
    place: usize,
}

/// The place of a primitive the walk meets now.
fn place(world: bool, in_pass: bool, after_fx: bool, runs: usize) -> usize {
    match (world, in_pass, after_fx) {
        (false, _, _) => BEFORE_WORLD,
        (true, true, _) => IN_PASS,
        (true, false, true) => AFTER_EFFECTS,
        (true, false, false) => runs,
    }
}

/// The frame's jobs read in kick order (`jobs.rs`), each VIF1 job block by block (the tag's two VIF words, two NOPs for a block
/// sent without them, then its data), each GIF job as the GIF reads it. With `mobies`, the moby chain is read from its first
/// block (a cnt of 6 quadwords whose tag VIF words are STCYCL 4/4 and UNPACK V4-32 of 6 to 0x123: the program's constants, the
/// first of the frame) to its job's end: the moby runs as program 0 receives them (the module doc). Everywhere the DIRECTs' GS
/// packets are read through one GS state (`EeGs`), and each program upload (MPG) tells which program the MSCALs that follow
/// run: the eight the port knows, or another (a `Pass`, from its upload to the next known program's). Program 6's upload
/// starts the effect pass: what the EE draws after it is drawn after the effects.
///
/// `start`: the two contexts' registers as the field before's jobs left them (the GS keeps its registers from frame to frame: a
/// menu frame draws with the XYOFFSET_1 and SCISSOR_1 set before it); without it, `EeGs::start`'s.
pub(crate) fn walk(jobs: &crate::jobs::FrameJobs, boot: &uyafmt::boot::Boot, mobies: bool, start: Option<GsLeft>) -> Result<Walk, String> {
    walk_into(jobs, boot, mobies, start, Vec::new(), &mut LevelVu::new())
}

/// VU1 and VIF1 as a level's frames leave them for the next (`walk_into`; `native::Walker` keeps one from frame to frame, as the
/// PS2 keeps them): the program the last upload made resident (None: none known, or a pass's), VU1 as the runs of program 0
/// outside the moby chain (the stars of a space shot) leave it (None: none has run yet; then VU1 as the chain's first run leaves
/// it, `chain_vu`), and VIF1's registers. A walk of one frame alone (`walk`) starts from a new one: nothing resident.
#[derive(Clone)]
pub(crate) struct LevelVu {
    resident: Option<u8>,
    vu: Option<crate::vu1::Vu1>,
    regs: crate::vif::VifRegs,
    /// Kept from frame to frame (`kept`): its walks are the ones UYA_STARS_LOG counts.
    kept: bool,
}

impl LevelVu {
    pub fn new() -> LevelVu {
        LevelVu { resident: None, vu: None, regs: crate::vif::VifRegs { cl: 1, wl: 1, ..Default::default() }, kept: false }
    }

    /// The one a walker keeps from frame to frame.
    pub fn kept() -> LevelVu {
        LevelVu { kept: true, ..LevelVu::new() }
    }
}

/// UYA_STARS_LOG: per frame with runs of program 0 outside the moby chain, or MSCALs with no program known resident, their
/// counts (an instrument).
fn stars_log() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UYA_STARS_LOG").is_some())
}

/// `walk`, its primitives (`Walk::ee`) written into `buf` (emptied first): the native frame hands back the last frame's, whose
/// room the sea's 20,000 primitives a frame then take without growing it again.
pub(crate) fn walk_into(jobs: &crate::jobs::FrameJobs, boot: &uyafmt::boot::Boot, mobies: bool, start: Option<GsLeft>, buf: Vec<(usize, EePrim)>, lv: &mut LevelVu) -> Result<Walk, String> {
    // The moby chain is found with or without `mobies`: without, its runs and their UNPACKs are passed over (no moby is drawn),
    // so that program 0 runs only outside it. Run there, on the VU1 memory the walk keeps for the runs outside the chain, the
    // chain's runs looped (the player's level 27, 20 November 2026: "VU1 p0: 5,000,000 pairs without ending" on the game
    // thread's read-back walk and on the renderer's walk without the mobies).
    let mut chain_found = false;
    let mut ran = [false; 8];
    // The program resident as the frame starts is the one the frame before left (`LevelVu`); `this_frame` the one this frame's
    // own uploads made (None before its first), for the counts UYA_STARS_LOG gives of a walk without the carry.
    let mut resident: Option<u8> = lv.resident;
    let mut this_frame: Option<u8> = None;
    let (mut star_runs, mut star_fresh, mut star_unknown, mut alone_runs, mut alone_skipped) = (0u32, 0u32, 0u32, 0u32, 0u32);
    let (mut star_qws, mut star_prims, mut star_entries) = (0usize, 0usize, Vec::new());
    let mut runs = Vec::new();
    // The words of every UNPACK the chain sent (`RawUnpack::words` ranges in it).
    let mut unpack_words: Vec<u32> = Vec::new();
    let mut constants = [[0f32; 4]; 6];
    let mut chain_constants = false;
    let mut world_fogcol: Option<Option<u64>> = None;
    let (mut other, mut textured) = (None, false);
    let mut gs = EeGs::start();
    if let Some(c) = start {
        (gs.ctx, gs.texa, gs.fogcol) = (c.ctx, c.texa, c.fogcol);
    }
    let mut ee: Vec<(usize, EePrim)> = Vec::new();
    // The two buffers and the one the VIF writes now (the first run's is TOPS 0).
    let mut bufs = [VuData::default(), VuData::default()];
    let mut tops = 0usize;
    // A known program's upload met (the world drawn natively from here); program 6's met (the effect pass started); the pass
    // being read and the passes read (each with where its output goes and its place).
    let (mut world, mut after_fx) = (false, false);
    let mut pass: Option<PassAt> = None;
    let mut passes: Vec<(Pass, usize, usize)> = Vec::new();
    let mut stopped = Vec::new();
    let mut in_chain = false;
    // VU1 and VIF1's registers for the runs of program 0 outside the moby chain (the stars): replayed as the PS2 does, from the
    // frame before's (`LevelVu`); the first from VU1 as the chain's first run leaves it (`chain_vu`).
    let mut zvu: Option<crate::vu1::Vu1> = lv.vu.take();
    let mut fresh = false;
    let mut zregs = lv.regs;
    // VIF memory a STMOD 2 UNPACK is replayed into for its ROW alone (an UNPACK never reads what it writes over).
    let mut scratch: Vec<[u32; 4]> = Vec::new();
    // The held Q's writer so far, and the one when program 6's upload came.
    let (mut held, mut fx_q): (Option<HeldQ>, Option<Option<HeldQ>>) = (None, None);
    for (jn, job) in jobs.jobs.iter().enumerate() {
        let (all, marks, blocks) = match job {
            crate::jobs::Job::Gif { qs, .. } => {
                // Path 3 sends no addresses: an IMAGE transfer from it has source 0.
                let qw = |q: usize| qs.get(q).map(|&d| [d as u32, (d >> 32) as u32, (d >> 64) as u32, (d >> 96) as u32]).ok_or_else(|| "native: a GIF job's packet past its data".to_string());
                gs.passes = passes.len() + pass.is_some() as usize;
                let pl = place(world, pass.is_some(), after_fx, runs.len());
                if let Err(e) = walk_part(0, || gs.direct(qs.len(), qw, |_| 0, pl, &mut ee, |_, _| {})) {
                    stopped.push(format!("GIF job {jn}: {e}"));
                }
                continue;
            }
            crate::jobs::Job::Vif1 { words, marks, blocks, .. } => (words, marks, blocks),
        };
        // Where the job's tags' two VIF words are (sorted, for the blocks' look-ups).
        let mut tte: Vec<usize> = marks.iter().filter(|m| m.1 == 2).map(|m| m.0).collect();
        tte.sort_unstable();
        let tagged = |idx: usize| idx >= 2 && tte.binary_search(&(idx - 2)).is_ok();
        // The moby chain's first block in this job, if it is the chain's.
        let chain_from = (!chain_found)
            .then(|| blocks.iter().position(|b| b.3 == 1 && b.2 == 6 && tagged(b.0) && all[b.0 - 2..b.0] == [0x0100_0404, 0x6C06_0123]))
            .flatten();
        chain_found |= chain_from.is_some();
        let walked = (|| -> Result<(), String> {
            for (k, b) in blocks.iter().enumerate() {
                let (idx, src, qwc, id) = *b;
                let qwc = qwc as usize;
                // The block's EE source address (bit 31: scratchpad).
                let (data_at, at) = (src as usize, src as usize);
                let data = all.get(idx..idx + 4 * qwc).ok_or_else(|| format!("native: VIF1 job {jn}'s block from {src:#x} past its words"))?;
                // The tag's VIF words and the data are one run of the job's words (no copy); a block without them gets two NOPs.
                let words: std::borrow::Cow<[u32]> = if tagged(idx) {
                    std::borrow::Cow::Borrowed(&all[idx - 2..idx + 4 * qwc])
                } else {
                    std::borrow::Cow::Owned([[0, 0].as_slice(), data].concat())
                };
                if chain_from == Some(k) {
                    for (n, c) in constants.iter_mut().enumerate() {
                        *c = std::array::from_fn(|e| f32::from_bits(words[2 + 4 * n + e]));
                    }
                    chain_constants = true;
                    world_fogcol.get_or_insert(gs.fogcol);
                    in_chain = true;
                    // The moby pass's ALPHA_1 when it starts (the traced frame's GS stream: the world programs' own packets leave
                    // it so): 0x8044. TEST_1 is the EE's last: only its packets write it, never a VU1 program's (level 28 field
                    // 6160 of the emulated PS2: 165 TEST_1 writes, all PATH2), and the one before the pass can be any (level 28's
                    // pipe: 0x5360B, the alpha test that keeps the splash ring about the hero, almost all clear texels, out of the
                    // depth the water is then tested against; set to 0x5B00C here, the ring hid the water about him). FRAME_1 is
                    // the EE's last too (a cutscene draws the moby pass into a texture). The registers the pass does not follow
                    // are counted from here.
                    gs.ctx[0].alpha = EeGs::start().ctx[0].alpha;
                    (other, textured) = (None, false);
                }
                // Packet A is the block that sends the strip list (UNPACK to 0x12D); its texture coordinates (UNPACK to 0xC2) are
                // the class's, and coordinates in any other block are the rigid layout's skin output.
                let is_packet_a = in_chain && sends_strip_list(&words);
                // Whether this block's MPG ends the pass being read (the block is then not the pass's).
                let mut ends_pass = false;
                let (mut cl, mut wl) = (1usize, 1usize);
                let mut i = 0;
                while i < words.len() {
                    let code = words[i];
                    let cmd = (code >> 24) & 0x7f;
                    let imm = code & 0xffff;
                    let num = ((code >> 16) & 0xff) as usize;
                    i += 1;
                    let moby = in_chain && !after_fx && pass.is_none();
                    match cmd {
                        // Validate a FLUSHE/MPG upload at the start of the DMA block, once FLUSHE is
                        // decoded as a command. A tag's UNPACK can consume those same words as vertex data.
                        0x11 if i == 3 && words.get(i).is_some_and(|w| (w >> 24) & 0x7f == 0x4a) => {
                            let disc = boot.bytes(data_at as u32, 16 * qwc)?;
                            for (k, (&word, original)) in data.iter().zip(disc.chunks_exact(4)).enumerate() {
                                if word != u32::from_le_bytes(original.try_into().expect("4 bytes")) {
                                    return Err(format!("native: the microcode packet at {data_at:#x} differs from the main program's at word {k}"));
                                }
                            }
                        }
                        0x00 | 0x04 | 0x06 | 0x07 | 0x10 | 0x11 | 0x13 => {}
                        0x01 => {
                            (cl, wl) = ((imm & 0xff) as usize, (imm >> 8 & 0xff) as usize);
                            (zregs.cl, zregs.wl) = (cl as u32, wl as u32);
                        }
                        0x02 => (zregs.offset, zregs.dbf, zregs.tops) = (imm & 0x3ff, false, zregs.base),
                        0x03 => zregs.base = imm & 0x3ff,
                        0x05 => zregs.mode = imm & 3,
                        0x20 => {
                            zregs.mask = *words.get(i).ok_or("native: STMASK past its block")?;
                            i += 1;
                        }
                        0x30 | 0x31 => {
                            let w = words.get(i..i + 4).ok_or("native: STROW or STCOL past its block")?;
                            if cmd == 0x30 { zregs.row.copy_from_slice(w) } else { zregs.col.copy_from_slice(w) }
                            i += 4;
                        }
                        0x14 | 0x15 | 0x17 if !moby => {
                            if pass.is_none() {
                                star_unknown += resident.is_none() as u32;
                                alone_skipped += this_frame.is_none() as u32;
                                alone_runs += (this_frame == Some(0)) as u32;
                            }
                            if let Some(p) = resident {
                                ran[p as usize] = true;
                                if p != 0 && p != 6 {
                                    world_fogcol.get_or_insert(gs.fogcol);
                                }
                            }
                            let top = zregs.start();
                            // A run of program 0 outside the moby chain (the stars of a space shot: the level's code draws them with the
                            // moby program's own entries): run on the VU1 memory the chain's UNPACKs built, its GIF packets read as the
                            // GS reads them.
                            if resident == Some(0) && pass.is_none() {
                                if cmd == 0x17 {
                                    return Err("native: MSCNT of program 0 outside the moby chain".into());
                                }
                                let vu = match &mut zvu {
                                    Some(v) => v,
                                    None => {
                                        fresh = true;
                                        zvu.insert(chain_vu()?)
                                    }
                                };
                                star_runs += 1;
                                star_fresh += fresh as u32;
                                if !star_entries.contains(&(imm * 8)) {
                                    star_entries.push(imm * 8);
                                }
                                let qs = run_program_0(vu, top, imm)?;
                                let pl = place(world, false, after_fx, runs.len());
                                let st0 = gs.st_writes;
                                gs.passes = passes.len();
                                let e0 = ee.len();
                                gs.gif_out(&qs, pl, &mut ee, &mut std::iter::empty())?;
                                (star_qws, star_prims) = (star_qws + qs.len(), star_prims + ee.len() - e0);
                                if gs.st_writes != st0 {
                                    held = Some(HeldQ::Value(gs.held_q));
                                }
                            }
                        }
                        0x14 | 0x15 | 0x17 => {
                            ran[0] = true;
                            if !mobies {
                                continue;
                            }
                            let variant_a = match imm {
                                0xA | 0x8 => true,
                                0xE | 0xC => false,
                                0x0 => continue,
                                m => return Err(format!("native: moby MSCAL {m:#x}")),
                            };
                            let vu = walk_part(1, || bufs[tops].clone());
                            runs.push(Run { vu, alpha: gs.ctx[0].alpha, frame: gs.ctx[0].frame, other, textured, variant_a, test: gs.ctx[0].test, xyoffset: gs.ctx[0].xyoffset, scissor: gs.ctx[0].scissor, ctx: gs.ctx[0], top: 0x164 * tops, mscal: imm, images: gs.images.len() });
                            held = Some(HeldQ::Run(runs.len() - 1));
                            textured = false;
                            tops ^= 1;
                        }
                        0x50 | 0x51 => {
                            // DIRECT: qword aligned GIF packets: A+D writes of TEST_1 and ALPHA_1, and the EE's own primitives.
                            let n = if imm == 0 { 0x10000 } else { imm as usize };
                            // The data starts at the next quadword of the stream (`words[2]` is the block's first data word).
                            i = 2 + i.saturating_sub(2).div_ceil(4) * 4;
                            let base = i;
                            let st0 = gs.st_writes;
                            gs.passes = passes.len() + pass.is_some() as usize;
                            let pl = place(world, pass.is_some(), after_fx, runs.len());
                            walk_part(0, || gs.direct(
                                n,
                                |q| words.get(base + 4 * q..base + 4 * q + 4).map(|w| [w[0], w[1], w[2], w[3]]).ok_or_else(|| format!("native: DIRECT past its block at {at:#x}")),
                                // The EE address of the DIRECT's quadword q.
                                |q| (data_at + 4 * (base - 2) + 16 * q) as u32,
                                pl,
                                &mut ee,
                                |reg, val| match reg {
                                    // TEST_1, ALPHA_1, FRAME_1, and XYOFFSET_1, SCISSOR_1 and ZBUF_1 (another frame buffer's window
                                    // and depth buffer, which the run takes; `Mobies::frame`).
                                    0x47 | 0x42 | 0x4c | 0x18 | 0x40 | 0x4e => {}
                                    // TEXFLUSH, and the image transfer's BITBLTBUF, TRXPOS, TRXREG, TRXDIR: no drawing state.
                                    0x3f | 0x50..=0x53 => {}
                                    // DTHE: the GS's dither of 16-bit frame buffers, which the native picture has not; COLCLAMP 1: the
                                    // colour clamped to 0..255, as the native picture's is.
                                    0x45 => {}
                                    0x46 if val == 1 => {}
                                    // PRMODECONT 1: the primitive's attributes are PRIM's, as the pass takes them.
                                    0x1a if val == 1 => {}
                                    0x06 | 0x14 | 0x08 | 0x34 => textured = true,
                                    0x4a if val == 0 => {}
                                    reg => other = Some((reg, val)),
                                },
                            ))?;
                            // Inside a pass its output (which carries these packets) gives the held Q.
                            if gs.st_writes != st0 && pass.is_none() {
                                held = Some(HeldQ::Value(gs.held_q));
                            }
                            i += 4 * n;
                        }
                        // MPG: a known program's upload ends the pass being read (program 6's starts the effect pass, whose runs
                        // `effects.rs` draws); any other's starts a pass.
                        0x4a => {
                            let microcode = (data_at + 4 * (i - 2)) as u32;
                            let program = program_of(Some(microcode), words.get(i..).unwrap_or(&[]));
                            resident = program;
                            this_frame = program;
                            match program {
                                Some(p) => {
                                    world = true;
                                    if let Some(pa) = pass.take() {
                                        passes.push((Pass { stream: pa.stream, marks: pa.marks, gs: pa.gs, images: gs.images[pa.images..].to_vec() }, pa.at, pa.place));
                                        held = Some(HeldQ::Pass(passes.len() - 1));
                                        ends_pass = true;
                                    }
                                    if p == 6 && fx_q.is_none() {
                                        fx_q = Some(held);
                                    }
                                    after_fx |= p == 6;
                                }
                                None if pass.is_none() => {
                                    world = true;
                                    pass = Some(PassAt { stream: STREAMS.take(), marks: Vec::new(), gs: gs.clone(), images: gs.images.len(), at: ee.len(), place: place(true, false, after_fx, runs.len()) });
                                }
                                None => {}
                            }
                            i += 2 * if num == 0 { 256 } else { num };
                        }
                        0x60..=0x7f => {
                            let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
                            let n = if num == 0 { 256 } else { num };
                            if !moby {
                                // Its size: CL and WL of STCYCL (a write cycle longer than the read one skips no data).
                                let items = if wl <= cl || wl == 0 { n } else { (n / wl) * cl + (n % wl).min(cl) };
                                let bits = if vl == 3 { 16 } else { (vn as usize + 1) * (32 >> vl) };
                                let len = (items * bits).div_ceil(32);
                                // Program 0 outside the moby chain reads what these build.
                                if resident == Some(0) && pass.is_none() {
                                    let w = words.get(i..i + len).ok_or("native: an UNPACK past its block")?;
                                    let vu = match &mut zvu {
                                        Some(v) => v,
                                        None => {
                                            fresh = true;
                                            zvu.insert(chain_vu()?)
                                        }
                                    };
                                    crate::vif::unpack(&mut vu.mem[..], cmd, num as u32, imm, w, &mut zregs).map_err(|e| format!("native: {e}"))?;
                                }
                                i += len;
                                continue;
                            }
                            if !mobies {
                                let size = if vl == 3 { n * 2 } else { n * (vn as usize + 1) * (4 >> vl) };
                                i += size.div_ceil(4);
                                continue;
                            }
                            let t_unpack = walk_timed().then(std::time::Instant::now);
                            let bytes_per = match vl {
                                0 => 4,
                                1 => 2,
                                2 => 1,
                                _ => 2,
                            };
                            let size = if vl == 3 { n * 2 } else { n * (vn as usize + 1) * bytes_per };
                            let len = size.div_ceil(4);
                            // The block's words as bytes, in place (the host is little-endian, as the words are laid out).
                            let data: &[u8] = bytemuck::cast_slice(words.get(i..i + len).ok_or("native: moby UNPACK past its block")?);
                            // FLG: the address is from TOPS (the buffer written now); without it, 0x164 and up is the second buffer.
                            let (b, addr) = match (imm & 0x8000 != 0, imm & 0x3ff) {
                                (true, a) => (tops, a),
                                (false, a) if a >= 0x164 => (1, a - 0x164),
                                (false, a) => (0, a),
                            };
                            let cur = &mut bufs[b];
                            match (cmd & 0x6f, addr) {
                                (0x6c, 0x129) => {
                                    for (r, row) in cur.rows.iter_mut().enumerate() {
                                        for (c, x) in row.iter_mut().enumerate() {
                                            *x = f32::from_le_bytes(data[16 * r + 4 * c..16 * r + 4 * c + 4].try_into().expect("4 bytes"));
                                        }
                                    }
                                }
                                (0x69, 0x000) => {
                                    cur.positions = std::sync::Arc::from(bytemuck::cast_slice::<u8, [i16; 3]>(&data[..6 * n]));
                                    // The data's address: the block's, plus where in it (its words follow the tag's two).
                                    cur.positions_at = Some(data_at as u32 + 4 * (i as u32 - 2));
                                }
                                (0x6e, 0x061) => cur.colours = std::sync::Arc::from(bytemuck::cast_slice::<u8, [u8; 4]>(&data[..4 * n])),
                                (0x65, 0x0c2) if !is_packet_a => cur.uvs = Some(std::sync::Arc::from(bytemuck::cast_slice::<u8, [i16; 2]>(&data[..4 * n]))),
                                // Packet A's own coordinates and its texture setups (at 0x12D + the strip list's quadword count).
                                (0x65, 0x0c2) if is_packet_a => cur.uvs = None,
                                (0x6c, a) if is_packet_a && a > 0x12d => {}
                                // Packet A: the block's address names the class mesh.
                                (0x6e, 0x12d) => cur.packet_a = Some(data_at as u32),
                                // The constants, to either buffer.
                                (0x6c, 0x123) => {}
                                (c, a) => return Err(format!("native: moby chain UNPACK {c:#x} to {a:#x} at {at:#x}")),
                            }
                            let raw = RawUnpack { cmd, num: num as u32, imm, regs: zregs, words: (unpack_words.len() as u32, len as u32) };
                            unpack_words.extend_from_slice(&words[i..i + len]);
                            // STMOD 2 (difference) keeps each value it writes in ROW, which the next UNPACK adds to: its replay
                            // (`run_vu`) starts from the ROW this one leaves.
                            if zregs.mode == 2 {
                                if scratch.is_empty() {
                                    scratch.resize(4096, [0u32; 4]);
                                }
                                crate::vif::unpack(&mut scratch, cmd, num as u32, imm, &words[i..i + len], &mut zregs).map_err(|e| format!("native: {e}"))?;
                            }
                            // A later UNPACK to the same address and format replaces the earlier, and every one is replayed in the
                            // order VIF1 got it: one replaced in place came before an UNPACK it overlaps that VIF1 got after it (the
                            // fall down level 27's pipe: a matrix at TOP + 0x14A written again after one at TOP + 0x147, replayed
                            // before it, put a ring's vertices all at one point, holes in the tube where the PS2 draws it whole).
                            let unpacks = std::sync::Arc::make_mut(&mut cur.unpacks);
                            unpacks.retain(|u| !(u.cmd == cmd && u.imm == imm));
                            unpacks.push(raw);
                            if let Some(t) = t_unpack {
                                WALK_NS[1].fetch_add(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                            }
                            i += len;
                        }
                        c => return Err(format!("native: VIF code {c:#x} at {at:#x}")),
                    }
                }
                if let Some(pa) = pass.as_mut().filter(|_| !ends_pass) {
                    // Room for the rest of the job's blocks at once (the sea's pass is most of a job).
                    pa.stream.reserve(all.len() - idx + 2 * (blocks.len() - k));
                    pa.marks.push((pa.stream.len(), 2));
                    pa.stream.extend_from_slice(&words[..2]);
                    pa.marks.push((pa.stream.len(), 0));
                    pa.stream.extend_from_slice(&words[2..]);
                }
                if in_chain && (id == 7 || id == 0) {
                    return Ok(());
                }
            }
            Ok(())
        })();
        if let Err(e) = walked {
            // A moby chain the walk cannot read draws no moby: the caller says why and walks again without it.
            if in_chain {
                return Err(e);
            }
            stopped.push(format!("VIF1 job {jn}: {e}"));
        }
        in_chain = false;
    }
    // A pass the walk did not see end ends with the frame.
    if let Some(pa) = pass.take() {
        passes.push((Pass { stream: pa.stream, marks: pa.marks, gs: pa.gs, images: gs.images[pa.images..].to_vec() }, pa.at, pa.place));
    }
    let mut fx_q = fx_q.flatten();
    (lv.resident, lv.vu, lv.regs) = (resident, zvu, zregs);
    if stars_log() && lv.kept && star_runs + star_unknown + alone_runs + alone_skipped > 0 {
        println!("native: field {}: program 0 outside the moby chain: {star_runs} runs (entries {star_entries:x?}; {star_fresh} on a VU1 made fresh this frame; {star_qws} quadwords kicked, {star_prims} primitives), {star_unknown} MSCALs with no program known resident; walked without the frames before: {alone_runs} runs, all on a fresh VU1, {alone_skipped} MSCALs skipped", jobs.field);
    }
    // The transfers into and out of the main frame (one inside a pass is not read).
    let mut transfers: Vec<Transfer> = Vec::new();
    for t in &gs.transfers {
        match t.main_pages() {
            Ok(p) if p.is_empty() => {}
            Ok(_) if t.after == IN_PASS => stopped.push("native: a transfer of the main frame inside a pass".into()),
            Ok(_) => transfers.push(*t),
            Err(e) => stopped.push(e),
        }
    }
    if eedump() {
        eprintln!("walk images: {:x?}", gs.images.iter().map(|(d, u)| (*d, u.src, u.psm, u.w, u.h)).collect::<Vec<_>>());
    }
    // The primitives drawn, in the order the GS gets them: the EE's own, each pass's output where the walk met the pass (run here,
    // in the walk's order, onto the primitives so far); what the walk met inside a pass is in its output, and what came before
    // the first program's upload is the native frame's own. A frame that uploads no program (a menu screen's) draws no world:
    // its EE packets are all its picture, after the effects. `own_kept[i]`: the EE's own primitives drawn before its primitive
    // i, and `lens[k]` pass k's output, for the transfers' places among them.
    // A frame whose programs drew no terrain, ties or shrubs (only mobies: a trip's space scene between planets, which the
    // level's code draws while the next level loads) has no native world or sky to stand for what came before its first upload:
    // that is its picture's back (the trip's starfield, 54 textured triangles over the clear's), drawn first, before the runs, across
    // the picture's width (`EePrim::fill`: at the PS2's shape the wide picture's sides showed the clear's blue past the stars).
    // Dropped, the trips showed the ship on black (the player, 24 November 2026: "The stars frequently didnt appear properly",
    // "The space shots are the loading screens between planets").
    let no_world = !(ran[1] || ran[7] || ran[2] || ran[3] || ran[4] || ran[5]);
    let mut prims = buf;
    prims.clear();
    prims.reserve(ee.len());
    let mut own_kept = Vec::with_capacity(ee.len() + 1);
    let mut lens = vec![0usize; passes.len()];
    let mut outputs = Vec::with_capacity(passes.len());
    let mut passes = passes.into_iter().enumerate().peekable();
    let mut own = ee.into_iter();
    let mut kept = 0;
    loop {
        while let Some((k, (pa, _, place))) = passes.next_if(|(_, (_, at, _))| *at == own_kept.len()) {
            let start = prims.len();
            match walk_part(2, || pa.prims(place, &mut prims)) {
                Ok(q) => {
                    lens[k] = prims.len() - start;
                    outputs.push(start..prims.len());
                    if matches!(fx_q, Some(HeldQ::Pass(j)) if j == k) {
                        fx_q = Some(HeldQ::Value(q));
                    }
                }
                Err(e) => {
                    prims.truncate(start);
                    stopped.push(e);
                }
            }
        }
        own_kept.push(kept);
        let Some(mut p) = own.next() else { break };
        if !world {
            p.1.after = AFTER_EFFECTS;
        } else if no_world && p.1.after == BEFORE_WORLD {
            p.1.after = 0;
            p.1.fill = true;
        }
        if p.1.after != IN_PASS && p.1.after != BEFORE_WORLD {
            kept += 1;
            prims.push(p);
        }
    }
    assert!(passes.next().is_none(), "a pass begins where the walk's primitives were then");
    // A transfer comes after the output of each pass begun before it.
    for t in &mut transfers {
        t.index = own_kept[t.index] + lens[..t.passes].iter().sum::<usize>();
        if !world {
            t.after = AFTER_EFFECTS;
        }
    }
    Ok(Walk { runs, constants, chain_constants, world_fogcol: world_fogcol.flatten(), ee: prims, stopped, fx_q, ran, transfers, gs_end: GsLeft { ctx: gs.ctx, texa: gs.texa, fogcol: gs.fogcol }, passes: outputs, images: gs.images, unpack_words })
}

/// A level's moby globals: the variable holding the core's RAM base, the view camera, the hero armor's entry and buffer
/// variables, the wrench block's variable and the armor texture remap bytes.
#[derive(Clone, Copy)]
pub(crate) struct MobyGlobals {
    pub core: u32,
    pub view_camera: u32,
    pub armor_entry: u32,
    pub armor_buffer: u32,
    pub wrench_block: u32,
    pub armor_remap: [u8; 16],
    /// The skin textures moby classes of mode 2 (Clank's) and mode 1 (the ship's) give their 0xFF setups.
    pub clank: uyafmt::levelcode::SkinTexture,
    pub ship: uyafmt::levelcode::SkinTexture,
    /// The level's scene and hero body variables.
    pub scene: uyafmt::scenecode::SceneGlobals,
    /// The held gadget's buffer and class table entry.
    pub gadget: uyafmt::levelcode::GadgetAddrs,
}

/// A gadget class's texture setup numbers: func_L01_0042A058 maps number 0 to the gadget's one texture and sets no other.
const GADGET_REMAP: [u8; 16] = [0, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];

/// The wrench's entry in the armor WAD (func_L01_00422D70 reads its sectors at 0x162CE8 and 0x162CF0: entry 31 of the table at
/// 0x162AF8).
const WRENCH_ENTRY: u32 = 31;

/// The armor entry variable's value while no armor is loaded (a level's first frames).
const NO_ARMOR: u32 = u32::MAX;

/// Where a run's mesh comes from: the core's class `n`, armor WAD entry `n`, class `c` of SCENE record `k`, the hero body
/// (variant, texture), the held gadget of class id `n`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Src {
    Core(usize),
    Armor(u32),
    Scene(u32, usize),
    Body(u32, u32),
    Gadget(u32),
}

/// The texture sources' materials ids: the core's moby textures 0, armor entry e 1 + e, SCENE record k's class c, the hero body's
/// texture, scene class 10's texture n, the gadget of class id n's texture.
fn armor_id(e: u32) -> u32 {
    1 + e
}
fn scene_id(k: u32, c: usize) -> u32 {
    0x1000 + 0x100 * k + c as u32
}
fn body_id(v: u32, t: u32) -> u32 {
    0x1_0000 + 0x100 * v + t
}
fn class10_id(n: u32) -> u32 {
    0x2_0000 + n
}
fn gadget_id(n: u32) -> u32 {
    0x3_0000 + n
}
/// The skin texture of WAD header `wad`'s pair `pair`.
fn skin_id(wad: u32, pair: u32) -> u32 {
    0x4000_0000 + (wad << 12) + pair
}

/// TEST_1 when the moby pass starts (the traced frame's GS stream): no alpha test, Z test GEQUAL.
const TEST_OPEN: u64 = 0x5_B00C;

pub(crate) struct Mobies {
    iso: String,
    /// The index of the first of the two special textures in the core's moby textures.
    specials: u32,
    /// The armor WAD entries met so far: the decoded block and its meshes by packet A offset.
    armors: HashMap<u32, (uyafmt::moby::Armor, HashMap<u32, usize>)>,
    /// The level's number (its SCENE file is the table of contents' entry for it).
    level: usize,
    /// The SCENE records met so far (None: the record holds no block) and their meshes by packet A offset in the block.
    scenes: HashMap<u32, Option<(uyafmt::scene::LoadedBlock, HashMap<u32, (usize, usize)>)>>,
    /// The hero bodies met so far, by (variant, texture), and their meshes by packet A offset.
    bodies: HashMap<(u32, u32), (uyafmt::scenecode::HeroBody, HashMap<u32, usize>)>,
    /// The gadget classes met so far, by class id, and their meshes by packet A offset.
    gadgets: HashMap<u32, (MobyClass, HashMap<u32, usize>)>,
    /// The pair counts of the WADs the skin textures are read from (`skin`), by header offset, read the first time one is needed.
    wad_pairs: HashMap<u32, usize>,
    classes: Vec<Option<MobyClass>>,
    records: Vec<[u8; 16]>,
    /// The core class records' modes (`ClassRecord::mode`).
    modes: Vec<u32>,
    /// Packet A's offset in the core (the class record's offset + `a_at`) -> (class, mesh).
    by_packet: HashMap<u32, (usize, usize)>,
    /// Per texture source (`Src::id`) and TEST_1 AREF (0 when the alpha test is off), the materials (the textures shared).
    pub materials: HashMap<(u32, u16), Materials>,
    /// The native picture's runs as the last frame drew them, kept when its vertices' places in the frame before are asked for
    /// (`frame`'s `track`): a picture between two frames draws each vertex between them (`replay.rs`).
    last: Option<LastFrame>,
    /// The per-pixel lighting's skin job (`skin_pass`): the state it keeps between meshes and frames (as the game's, nothing
    /// clears it), packet B decoded by its RAM address with the bytes it was decoded from, and each mesh's output by its
    /// positions' address.
    skin_state: uyafmt::moby::SkinState,
    packet_bs: HashMap<u32, (Vec<u8>, uyafmt::moby::Mesh, Vec<usize>)>,
    /// Each address's outputs with the skin pass that last wrote there (`skin_passes`): an address not written for `SKIN_AGE`
    /// passes is let go (`skin_pass`).
    skinned: HashMap<u32, (u64, Vec<std::sync::Arc<SkinOut>>)>,
    /// The fields' skin passes run so far (`skin_field`): the age of `skinned`'s addresses.
    skin_passes: u64,
    /// The last field whose transfers `skinned` holds the outputs of.
    skin_done: Option<u64>,
    /// The outputs of the frame before by packet B, with the inputs they were made from (the joint matrices the packet names, the
    /// light section, the second group's rows and alpha): a mesh skinned from the same inputs again (a moby that stands still)
    /// takes its output again. The run's check (`moby_lit`: the output is what the run uploaded) holds for these as for the rest.
    skin_kept: HashMap<u32, Vec<(Vec<u8>, std::sync::Arc<SkinOut>)>>,
    /// The buffers a run of `frame` fills (its per-pixel lighting, each vertex's screen point and its vertex as drawn), kept from
    /// run to run and frame to frame so that none is allocated and regrown again for each run.
    prepared: Vec<PreparedRun>,
    work: Vec<RunWork>,
}

/// What the frame's pass over the runs decided for one run (`Mobies::frame`).
enum Plan {
    Skip(String),
    /// A class loaded at run time: program 0 on the chain's VU1 memory.
    Vu,
    /// The run's `Task`, by index.
    Draw(usize),
    Fail(String),
}

/// What the vertex work of a drawn run needs.
struct Task<'a> {
    r: usize,
    run: &'a Run,
    a: &'a uyafmt::moby::PacketA,
    window: Option<(f32, f32, f32, f32)>,
    before: Option<&'a LastRun>,
    hero_run: bool,
    fb: u32,
}

/// A raw place in one of the frame's buffers that the crew's threads write disjoint parts of (`copy_runs`).
struct Dest<T>(*mut T);

// SAFETY: `copy_runs` gives each thread parts of the buffer no other writes, and reads none of it before the threads are done.
unsafe impl<T> Send for Dest<T> {}
unsafe impl<T> Sync for Dest<T> {}

impl<T> Dest<T> {
    fn at(&self, i: usize) -> *mut T {
        // SAFETY: `copy_runs` reserved room for every index it writes.
        unsafe { self.0.add(i) }
    }
}

/// Each drawn run's vertices, previous places and lighting (`work[task]`'s first `n` corners) put at `dest` of `out`, `prev` and
/// `lit` (`copies`: task, dest, n; in order and end to end from the buffers' length): what `extend_from_slice` of each did in turn,
/// shared out in chunks between the crew's threads and this one.
fn copy_runs(work: &[RunWork], copies: &[(usize, usize, usize)], out: &mut Vec<ObjVertex>, prev: &mut Vec<[f32; 3]>, lit: &mut Vec<crate::lighting::MobyLit>) {
    let Some(&(_, last, n)) = copies.last() else { return };
    let (base, end) = (out.len(), last + n);
    let total = end - base;
    out.reserve(total);
    prev.reserve(total);
    lit.reserve(total);
    let (po, pp, pl) = (Dest(out.as_mut_ptr()), Dest(prev.as_mut_ptr()), Dest(lit.as_mut_ptr()));
    let one = |&(ti, dest, n): &(usize, usize, usize)| {
        let w = &work[ti];
        assert!(w.out.len() >= n && w.prev.len() >= n && w.lit.len() >= n, "a run's corners are fewer than its strips' counts");
        // SAFETY: the room for `dest..dest + n` was reserved above and no other run's copy writes it (the destinations are the
        // runs' places end to end); the sources are `work`'s, read only.
        unsafe {
            std::ptr::copy_nonoverlapping(w.out.as_ptr(), po.at(dest), n);
            std::ptr::copy_nonoverlapping(w.prev.as_ptr(), pp.at(dest), n);
            std::ptr::copy_nonoverlapping(w.lit.as_ptr(), pl.at(dest), n);
        }
    };
    if total < 4096 {
        copies.iter().for_each(one);
    } else {
        let chunk = copies.len().div_ceil(16).max(1);
        crate::crew::run(copies.len().div_ceil(chunk), &|k| copies[k * chunk..copies.len().min((k + 1) * chunk)].iter().for_each(one));
    }
    // SAFETY: every slot below `end` is written: the runs' parts are end to end from the old length.
    unsafe {
        out.set_len(end);
        prev.set_len(end);
        lit.set_len(end);
    }
}

/// A drawn run's strips' (material set, key) and the vertices made for them: each strip's count, its corners, their previous
/// places and lighting. Kept from frame to frame so that none is allocated again.
const NO_CORNER: u32 = u32::MAX;

#[derive(Default)]
struct RunWork {
    strips: Vec<((u32, u16), Key)>,
    counts: Vec<usize>,
    out: Vec<ObjVertex>,
    prev: Vec<[f32; 3]>,
    lit: Vec<crate::lighting::MobyLit>,
    /// Each position's screen point (the cull's), when the run culls, and its fog.
    screens: Vec<[f32; 2]>,
    fogs: Vec<f32>,
    /// Each position's first corner in `out` (`NO_CORNER`: none yet).
    vertices: Vec<u32>,
    err: Option<String>,
}

/// World positions from a field of the native picture, for interpolation and motion vectors.
struct LastFrame {
    field: u64,
    runs: Vec<LastRun>,
}

struct LastRun {
    /// Packet A's address: the mesh.
    pa: u32,
    origin: [f32; 3],
    places: Vec<[f32; 3]>,
}

/// The farthest a moby run's origin moves in one field and is still taken for the same instance's run (`Mobies::frame`): a run
/// matched no nearer is drawn where this frame draws it in the pictures between frames.
const MATCH_UNITS: f32 = 4.0;

impl LastRun {
    /// The Phoenix hangar has reversed instances of the same mesh with nearly coincident origins.
    /// Rank candidates by corresponding vertex travel, and discard discontinuities even if only one candidate survived culling.
    fn match_score(&self, origin: [f32; 3], places: &[[f32; 3]]) -> Option<f32> {
        let distance2 = |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>();
        let limit = MATCH_UNITS * MATCH_UNITS;
        if places.len() != self.places.len() || !(distance2(self.origin, origin) <= limit) {
            return None;
        }
        let mut score = 0.0;
        for (&old, &new) in self.places.iter().zip(places) {
            let travel = distance2(old, new);
            if !(travel <= limit) {
                return None;
            }
            score += travel;
        }
        Some(score)
    }
}

#[test]
fn hangar_twins_match_the_same_side_across_fields() {
    let origin = [157.05, 95.647, 112.137];
    let opposite = [157.007, 95.635, 112.137];
    let left = LastRun { pa: 1, origin, places: vec![[149.0, 95.6, 112.0], [151.0, 95.6, 117.0]] };
    let right = LastRun { pa: 1, origin: opposite, places: vec![[165.0, 95.6, 112.0], [163.0, 95.6, 117.0]] };
    // The other instance has the nearer origin. It must also be rejected when the correct history is absent.
    assert!(right.match_score(opposite, &left.places).is_none());
    assert!(left.match_score(origin, &right.places).is_none());
    assert_eq!(left.match_score(opposite, &left.places), Some(0.0));
    assert_eq!(right.match_score(origin, &right.places), Some(0.0));
}

/// Where the GS puts a moby vertex (skinned position `p`, the run's rows `u`), back in the world: the program sends X =
/// FTOI4(clip.x x Q + vf22.x) and Y likewise (12.4 fixed point, truncated; `uyafmt::moby`, the per-vertex formula), so the world
/// position is the one whose screen point is that, clip x and y moved there with z and w kept (clip . C^-1 / 1024 + the eye).
/// Vertices the GS puts at one point are at one point natively: level 30's water is four instances whose shared edges are 0.015 GS
/// pixels apart in float and one 12.4 value on the GS (field 40500: 238.5034 and 238.5186, both 238.5).
fn world_at(u: &[[f64; 4]; 4], ci: &[[f64; 4]; 4], eye: [f32; 3], vf23x: f32, vf22: [f32; 4], p: [i16; 3]) -> [f32; 3] {
    let v = [p[0] as f64, p[1] as f64, p[2] as f64];
    let cl: [f64; 4] = [0, 1, 2, 3].map(|k| u[3][k] + v[0] * u[0][k] + v[1] * u[1][k] + v[2] * u[2][k]);
    let q = vf23x as f64 / cl[3];
    let snap = |c: f64, o: f32| (((c * q + o as f64) * 16.0).trunc() / 16.0 - o as f64) / q;
    let c = [snap(cl[0], vf22[0]), snap(cl[1], vf22[1]), cl[2], cl[3]];
    [0, 1, 2].map(|k| ((c[0] * ci[0][k] + c[1] * ci[1][k] + c[2] * ci[2][k] + c[3] * ci[3][k]) / 1024.0 + eye[k] as f64) as f32)
}

/// `world_at` of each of `ps`, appended to `out`: four at a time with every operation of `world_at` in its order on each, so that
/// the compiler can make them SIMD operations (which are the same IEEE operations on each lane), and the rest one at a time.
#[inline(always)]
fn world_all(u: &[[f64; 4]; 4], ci: &[[f64; 4]; 4], eye: [f32; 3], vf23x: f32, vf22: [f32; 4], ps: &[[i16; 3]], out: &mut Vec<[f32; 3]>) {
    out.reserve(ps.len());
    let mut quads = ps.chunks_exact(4);
    for p in &mut quads {
        let v: [[f64; 4]; 3] = std::array::from_fn(|c| std::array::from_fn(|j| p[j][c] as f64));
        let cl: [[f64; 4]; 4] = std::array::from_fn(|k| std::array::from_fn(|j| u[3][k] + v[0][j] * u[0][k] + v[1][j] * u[1][k] + v[2][j] * u[2][k]));
        let q: [f64; 4] = std::array::from_fn(|j| vf23x as f64 / cl[3][j]);
        let snap = |c: [f64; 4], o: f32| -> [f64; 4] { std::array::from_fn(|j| (((c[j] * q[j] + o as f64) * 16.0).trunc() / 16.0 - o as f64) / q[j]) };
        let c = [snap(cl[0], vf22[0]), snap(cl[1], vf22[1]), cl[2], cl[3]];
        let w: [[f32; 4]; 3] = std::array::from_fn(|k| std::array::from_fn(|j| ((c[0][j] * ci[0][k] + c[1][j] * ci[1][k] + c[2][j] * ci[2][k] + c[3][j] * ci[3][k]) / 1024.0 + eye[k] as f64) as f32));
        out.extend((0..4).map(|j| [w[0][j], w[1][j], w[2][j]]));
    }
    out.extend(quads.remainder().iter().map(|&p| world_at(u, ci, eye, vf23x, vf22, p)));
}

/// `rows` (a run's matrix) applied to the position `p` as the moby program does: x y z 1 times the four rows, the sum from the
/// last row up.
#[inline(always)]
fn clip_of(rows: &[[f32; 4]; 4], p: [i16; 3]) -> [f32; 4] {
    let v = [p[0] as f32, p[1] as f32, p[2] as f32];
    [0, 1, 2, 3].map(|k| rows[3][k] + v[0] * rows[0][k] + v[1] * rows[1][k] + v[2] * rows[2][k])
}

/// The screen point the program gives the position `p` (the cull's): clip x and y times Q = vf23.x / clip w, plus XYOFFSET's.
#[inline(always)]
fn screen_of(rows: &[[f32; 4]; 4], vf23x: f32, vf22: [f32; 4], p: [i16; 3]) -> [f32; 2] {
    let cl = clip_of(rows, p);
    let q = vf23x / cl[3];
    [cl[0] * q + vf22[0], cl[1] * q + vf22[1]]
}

/// The fog the program gives `p`: F at the position's clip w (variant A) or at the matrix's, floored to its minimum and at most 255.
#[inline(always)]
fn fog_of(rows: &[[f32; 4]; 4], variant_a: bool, vf22w: f32, vf24x: f32, p: [i16; 3]) -> f32 {
    let f = if variant_a { clip_of(rows, p)[3] + vf22w } else { rows[3][3] + vf22w };
    f.max(vf24x).min(255.0).floor()
}

/// `screen_of` of each of `ps` onto `out`, four at a time (every operation as `screen_of`'s, in its order, on each lane).
#[inline(always)]
fn screens_all(rows: &[[f32; 4]; 4], vf23x: f32, vf22: [f32; 4], ps: &[[i16; 3]], out: &mut Vec<[f32; 2]>) {
    out.reserve(ps.len());
    let mut quads = ps.chunks_exact(4);
    for p in &mut quads {
        let v: [[f32; 4]; 3] = std::array::from_fn(|c| std::array::from_fn(|j| p[j][c] as f32));
        let cl: [[f32; 4]; 4] = std::array::from_fn(|k| std::array::from_fn(|j| rows[3][k] + v[0][j] * rows[0][k] + v[1][j] * rows[1][k] + v[2][j] * rows[2][k]));
        let q: [f32; 4] = std::array::from_fn(|j| vf23x / cl[3][j]);
        out.extend((0..4).map(|j| [cl[0][j] * q[j] + vf22[0], cl[1][j] * q[j] + vf22[1]]));
    }
    out.extend(quads.remainder().iter().map(|&p| screen_of(rows, vf23x, vf22, p)));
}

/// `fog_of` of each of `ps` onto `out`, four at a time.
#[inline(always)]
fn fogs_all(rows: &[[f32; 4]; 4], variant_a: bool, vf22w: f32, vf24x: f32, ps: &[[i16; 3]], out: &mut Vec<f32>) {
    out.reserve(ps.len());
    if !variant_a {
        // The matrix's own w: the same for every position.
        out.resize(ps.len(), fog_of(rows, false, vf22w, vf24x, [0; 3]));
        return;
    }
    let mut quads = ps.chunks_exact(4);
    for p in &mut quads {
        let v: [[f32; 3]; 4] = std::array::from_fn(|j| [p[j][0] as f32, p[j][1] as f32, p[j][2] as f32]);
        let f: [f32; 4] = std::array::from_fn(|j| (rows[3][3] + v[j][0] * rows[0][3] + v[j][1] * rows[1][3] + v[j][2] * rows[2][3] + vf22w).max(vf24x).min(255.0).floor());
        out.extend_from_slice(&f);
    }
    out.extend(quads.remainder().iter().map(|&p| fog_of(rows, true, vf22w, vf24x, p)));
}

/// What one of program 0's two VU1 buffers holds (TOPS 0 and TOPS 0x164: the VIF writes the buffer of the next run, each MSCAL
/// runs it and swaps). The chain sends only what changed: a mesh drawn again with the same instance reuses the matrix rows, and the
/// same mesh its packet A, that its buffer still holds from two runs before (Veldin field 2400: with the two buffers kept, every
/// one of the 375 runs puts its vertices where the trace's GS packets do).
#[derive(Clone, Default)]
struct VuData {
    rows: [[f32; 4]; 4],
    /// Shared by the runs' copies of the buffer (`VuData`'s clone): an UNPACK replaces them whole, never writes into them.
    positions: std::sync::Arc<[[i16; 3]]>,
    colours: std::sync::Arc<[[u8; 4]]>,
    /// The texture coordinates at TOPS + 0xC2: the rigid layout's skin output, or None when packet A wrote them last (the class's).
    uvs: Option<std::sync::Arc<[[i16; 2]]>>,
    packet_a: Option<u32>,
    /// The EE address the positions came from (the skin job's output array of the mesh, `Mobies::skin_pass`).
    positions_at: Option<u32>,
    /// Every UNPACK the chain wrote into this buffer as VIF1 got it (a later one to the same address and format replaces the
    /// earlier): what a run of a class the port has no mesh for is run from (`run_vu`).
    unpacks: std::sync::Arc<Vec<RawUnpack>>,
}

/// One UNPACK as the chain sent it: the code's format and count and immediate, the cycle in force, and the data words.
#[derive(Clone)]
struct RawUnpack {
    cmd: u32,
    num: u32,
    imm: u32,
    /// VIF1's cycle, mask, row and column registers when it got the UNPACK.
    regs: crate::vif::VifRegs,
    /// Where its data words are in `Walk::unpack_words`: the start and the count.
    words: (u32, u32),
}

/// One run of program 0 read from the chain.
struct Run {
    vu: VuData,
    /// ALPHA_1 when the run is drawn.
    alpha: u64,
    /// FRAME_1 when the run is drawn (0x800F4: the frame buffer, every bit written).
    frame: u64,
    /// A GS register the chain wrote that the pass does not follow, with its value.
    other: Option<(u64, u64)>,
    /// Whether the chain wrote a texture register (TEX0_1, TEX1_1, CLAMP_1, MIPTBP1_1) since the last run: the texture a strip
    /// without a setup draws with is then the chain's, not the previous strip's.
    textured: bool,
    variant_a: bool,
    test: u64,
    /// XYOFFSET_1 and SCISSOR_1 when the run is drawn (a run into another frame buffer: its window).
    xyoffset: u64,
    scissor: u64,
    /// The drawing context 1 when the run starts (the GS state a run of program 0 draws with, TEX0 to SCISSOR).
    ctx: Ctx,
    /// VU1's TOPS when it starts (the buffer it reads), the MSCAL's immediate (its entry / 8), and how many of the walk's IMAGE
    /// transfers (`Walk::images`) came before it.
    top: usize,
    mscal: u32,
    images: usize,
}

fn rd_u32(ram: &[u8], a: usize) -> Result<u32, String> {
    ram.get(a..a + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or_else(|| format!("native: moby chain read at {a:#x} past RAM"))
}

fn rd_f32(ram: &[u8], a: usize) -> Result<f32, String> {
    rd_u32(ram, a).map(f32::from_bits)
}

/// The texture setting of a packet A texture setup, with the source of its texture's materials: `src` for a setup whose number
/// `rec` maps, the core's (0) for -2 and -3, which name the level's two special textures (`specials` and `specials + 1` in the
/// core's moby textures). For those func_L01_00423700 and func_L01_004239F8 store TEX1 with the setup's K and MMIN, CLAMP 5
/// (both axes clamped) and the TEX0 the level setup built (0x1DFF10, 0x1DFF28 on Veldin), single level.
fn moby_key(t: &uyafmt::moby::TextureSetup, rec: &[u8; 16], src: u32, specials: u32, own: Option<u32>) -> Result<(u32, Key), String> {
    match t.local {
        -2 | -3 => return Ok((0, (specials + (-2 - t.local) as u32, true, true, t.mmin, tex1_k(t.k).to_bits(), Blend::Alpha, 0))),
        // -1: TEX0 0x80000004CC007FFB, the 8 x 8 PSMCT32 texture at GS block 0x3FFB (zeros), after the two specials.
        -1 => return Ok((0, (specials + 2, true, true, t.mmin, tex1_k(t.k).to_bits(), Blend::Alpha, 0))),
        l if l < 0 => return Err(format!("native: moby texture setup with the special number {l}")),
        _ => {}
    }
    let byte = *rec.get(t.local as usize).ok_or_else(|| format!("native: moby texture number {} past the class record's 16 bytes", t.local))?;
    if byte == 0xff {
        // func_L01_00428750 gives a class of mode 1 the ship's and of mode 2 (and up) Clank's texture for these setups: `own` is
        // its materials id.
        return match own {
            Some(o) if t.wms <= 1 && t.wmt <= 1 => Ok((o, (0, t.wms == 1, t.wmt == 1, t.mmin, tex1_k(t.k).to_bits(), Blend::Alpha, 0))),
            _ => Err("native: moby texture with no record (0xFF)".into()),
        };
    }
    if t.wms > 1 || t.wmt > 1 {
        return Err(format!("native: moby CLAMP WMS {} WMT {}", t.wms, t.wmt));
    }
    Ok((src, (byte as u32, t.wms == 1, t.wmt == 1, t.mmin, tex1_k(t.k).to_bits(), Blend::Alpha, 0)))
}

impl Mobies {
    #[allow(clippy::too_many_arguments)]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, iso: &str, level: usize, classes: Vec<Option<MobyClass>>, records: &[uyafmt::core::ClassRecord], textures: &[uyafmt::core::Texture], specials: u32) -> Result<Mobies, String> {
        let mut by_packet = HashMap::new();
        let mut base = Materials::new(device, queue, textures, 5.0, 0)?;
        for (ci, (c, r)) in classes.iter().zip(records).enumerate() {
            let Some(c) = c else { continue };
            for (mi, m) in c.meshes.iter().enumerate() {
                let Some(a) = &m.packet_a else { continue };
                by_packet.insert((r.offset as usize + m.a_at) as u32, (ci, mi));
                for t in &a.textures {
                    // A setup the draw cannot use stops at its first use, with its reason.
                    if let Ok((_, key)) = moby_key(t, &r.textures, 0, specials, None) {
                        base.get(device, layout, key)?;
                    }
                }
            }
        }
        Ok(Mobies { iso: iso.to_string(), specials, armors: HashMap::new(), level, scenes: HashMap::new(), bodies: HashMap::new(), gadgets: HashMap::new(), wad_pairs: HashMap::new(), classes, records: records.iter().map(|r| r.textures).collect(), modes: records.iter().map(|r| r.mode).collect(), by_packet, materials: HashMap::from([((0, 0), base)]), last: None, skin_state: uyafmt::moby::SkinState::new(), packet_bs: HashMap::new(), skinned: HashMap::new(), skin_passes: 0, skin_done: None, skin_kept: HashMap::new(), prepared: Vec::new(), work: Vec::new() })
    }

    /// Armor WAD entry `entry`'s meshes and textures, decoded from the disc the first time a frame needs them.
    fn armor(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, entry: u32) -> Result<(), String> {
        if self.armors.contains_key(&entry) {
            return Ok(());
        }
        let (armor, textures) = uyafmt::moby::armor_entry(&self.iso, entry as usize)?;
        let by = armor.meshes.iter().enumerate().filter(|(_, m)| m.packet_a.is_some()).map(|(i, m)| (m.a_at as u32, i)).collect();
        self.materials.insert((armor_id(entry), 0), Materials::new(device, queue, &textures, 5.0, 0)?);
        self.armors.insert(entry, (armor, by));
        Ok(())
    }

    /// Skin texture `t` for the skin number its variable holds in `ram` (`uyafmt::moby::wad_image`), decoded the first time a
    /// frame needs it; its materials id, or None when the variable holds no skin: a number past the WAD's pictures (the loader
    /// reads the pair at skin 0's plus the number, whatever it is). The ship's colours are numbers past 15 (the vendor's Amoeboid
    /// Green is 23): read as no skin, the ship's wings, its parts drawn with the ship's own texture, were not drawn (the player,
    /// 23 November 2026: "your ship wings disappear when you select a different ship colour").
    fn skin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, ram: &[u8], t: &uyafmt::levelcode::SkinTexture) -> Result<Option<u32>, String> {
        let n = rd_u32(ram, t.skin as usize)?;
        let pairs = match self.wad_pairs.get(&t.wad) {
            Some(&k) => k,
            None => {
                let mut d = uyafmt::disc::Disc::open(&self.iso)?;
                let tc = uyafmt::toc::read(&mut d)?;
                let k = tc.wads.iter().find(|w| w.at == t.wad as usize).ok_or_else(|| format!("native: the table of contents has no WAD header at {:#x}", t.wad))?.pairs.len();
                self.wad_pairs.insert(t.wad, k);
                k
            }
        };
        if t.pair as u64 + n as u64 >= pairs as u64 {
            return Ok(None);
        }
        let id = skin_id(t.wad, t.pair + n);
        if !self.materials.contains_key(&(id, 0)) {
            let image = uyafmt::moby::wad_image(&self.iso, t.wad, (t.pair + n) as usize)?;
            self.materials.insert((id, 0), Materials::new(device, queue, &[image], 5.0, 0)?);
        }
        Ok(Some(id))
    }

    /// SCENE record `k`'s classes and their images (and scene class 10's WAD 5 texture `class10`), decoded the first time a frame
    /// needs them. A `class10` of -1 is the game's none (a level's first frames, before a cutscene sets it): no texture is loaded
    /// for it, and a scene class 10 drawn then finds no material and stops.
    fn scene(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, g: &uyafmt::scenecode::SceneGlobals, k: u32, class10: u32) -> Result<(), String> {
        let class10 = (class10 != u32::MAX).then_some(class10);
        // Both loaded already (every frame after the first): the disc is not opened.
        if self.scenes.contains_key(&k) && class10.is_none_or(|c| self.materials.contains_key(&(class10_id(c), 0))) {
            return Ok(());
        }
        let mut d = uyafmt::disc::Disc::open(&self.iso)?;
        let tc = uyafmt::toc::read(&mut d)?;
        if !self.scenes.contains_key(&k) {
            let file = tc.files.iter().find(|f| f.level == self.level && f.kind == uyafmt::toc::FileKind::Scene).ok_or_else(|| format!("native: level {} has no SCENE file", self.level))?;
            let loaded = uyafmt::scene::load(&mut d, file, k as usize)?;
            if let Some(l) = &loaded {
                for (c, sc) in l.block.classes.iter().enumerate() {
                    if !sc.images.is_empty() {
                        self.materials.insert((scene_id(k, c), 0), Materials::new(device, queue, &sc.images, 5.0, 0)?);
                    }
                }
            }
            let by = loaded.map(|l| {
                let by = l.block.classes.iter().enumerate().flat_map(|(c, sc)| sc.class.meshes.iter().enumerate().filter(|(_, m)| m.packet_a.is_some()).map(move |(m, me)| ((sc.offset + me.a_at) as u32, (c, m)))).collect();
                (l, by)
            });
            self.scenes.insert(k, by);
        }
        if let Some(c) = class10.filter(|c| !self.materials.contains_key(&(class10_id(*c), 0))) {
            let image = uyafmt::scenecode::class10_image(&mut d, &tc, g, c as usize)?;
            self.materials.insert((class10_id(c), 0), Materials::new(device, queue, std::slice::from_ref(&image), 5.0, 0)?);
        }
        Ok(())
    }

    /// The gadget of class id `id` (`uyafmt::moby::gadget`), decoded the first time a frame needs it.
    fn gadget(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, table: u32, id: u32) -> Result<(), String> {
        if self.gadgets.contains_key(&id) {
            return Ok(());
        }
        let (class, texture) = uyafmt::moby::gadget(&self.iso, table, id)?;
        let by = class.meshes.iter().enumerate().filter(|(_, m)| m.packet_a.is_some()).map(|(i, m)| (m.a_at as u32, i)).collect();
        self.materials.insert((gadget_id(id), 0), Materials::new(device, queue, std::slice::from_ref(&texture), 5.0, 0)?);
        self.gadgets.insert(id, (class, by));
        Ok(())
    }

    /// The hero body of `variant` and `texture`, decoded the first time a frame needs it.
    fn body(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, g: &uyafmt::scenecode::SceneGlobals, variant: u32, texture: u32) -> Result<(), String> {
        if self.bodies.contains_key(&(variant, texture)) {
            return Ok(());
        }
        let mut d = uyafmt::disc::Disc::open(&self.iso)?;
        let tc = uyafmt::toc::read(&mut d)?;
        let body = uyafmt::scenecode::hero_body(&mut d, &tc, g, variant as usize, texture as usize)?;
        let by = body.class.meshes.iter().enumerate().filter(|(_, m)| m.packet_a.is_some()).map(|(i, m)| (m.a_at as u32, i)).collect();
        self.materials.insert((body_id(variant, texture), 0), Materials::new(device, queue, std::slice::from_ref(&body.image), 5.0, 0)?);
        self.bodies.insert((variant, texture), (body, by));
        Ok(())
    }

    /// What the frame in `ram` draws that is decoded from the disc the first time it is needed: the hero's armor and the wrench,
    /// Clank's and the ship's skins (for the skin numbers their variables hold), the cutscene's SCENE record and the hero
    /// body (for the loaders' variables: record numbers 0 to 29; body variants 0 to 3 and texture numbers 0 to 31, the ranges the
    /// loaders' arguments take) and the held gadget. The level's load (`native`) calls it with its RAM, so that the arrival's
    /// first frame decodes none of it (level 28's took 15.5 ms). Returns the skins' materials ids and the held gadget's class id.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, ram: &[u8], g: &MobyGlobals) -> Result<(Option<u32>, Option<u32>, Option<u32>), String> {
        // The hero's armor: -1 is the game's none (a level's first frames, before its code loads the armor).
        match rd_u32(ram, g.armor_entry as usize)? {
            NO_ARMOR => {}
            e => self.armor(device, queue, e)?,
        }
        self.armor(device, queue, WRENCH_ENTRY)?;
        let (clank, ship) = (self.skin(device, queue, ram, &g.clank)?, self.skin(device, queue, ram, &g.ship)?);
        let s = &g.scene;
        let (scene_k, class10) = (rd_u32(ram, s.scene_index as usize)?, rd_u32(ram, s.class10_texture as usize)?);
        let (body_v, body_t) = (rd_u32(ram, s.body_variant as usize)?, rd_u32(ram, s.body_texture as usize)?);
        if scene_k < 30 {
            self.scene(device, queue, s, scene_k, class10)?;
        }
        if body_v < 4 && body_t < 32 {
            self.body(device, queue, s, body_v, body_t)?;
        }
        // The held gadget: the class table's entry at the index the level reserved for it (negative: none), when the class it
        // names is the gadget buffer's.
        let ga = &g.gadget;
        let reserved = rd_u32(ram, ga.index as usize)? as u16 as i16;
        let held = match usize::try_from(reserved) {
            Ok(i) => (rd_u32(ram, ga.classes as usize + 4 * i)? == ga.buffer).then_some(rd_u32(ram, ga.class_ids as usize + 2 * i)? & 0xFFFF),
            Err(_) => None,
        };
        if let Some(id) = held {
            self.gadget(device, queue, ga.table, id)?;
        }
        Ok((clank, ship, held))
    }

    /// The frame's runs, from the chain in RAM, with the program's constants and the primitives the EE writes into the chain
    /// itself. The walk reads on after the last run: the chain uploads the effect pass's program (MPG: program 6), runs it (its
    /// UNPACKs and MSCALs are the effect layer's, `effects.rs`) and goes on with more of the EE's own primitives (Veldin field
    /// 2400: the weapon fire, then screen-space passes), to its end. A failure after the last run (the chain's last `next` tag
    /// can point at memory the EE has used again since) leaves the runs and is returned as the last value.
    /// The walk ends after `expected` runs (the record list's mesh entries): past the moby pass the chain goes on into other
    /// passes, whose blocks may already be rebuilt (Kerwan field 22500: the 155th run is followed by a buffer of particle data).
    /// The frame's moby triangles, appended to `out`; each draw with its materials (0: no alpha test, 1: AREF 0x60). Returns the
    /// runs drawn and, per reason, the runs not drawn (a class outside the core, a texture setup the draw does not bind).
    /// The frame's moby triangles (the walk's runs), appended to `out`; each draw with its materials. Returns the runs drawn, per
    /// reason the runs not drawn (a class outside the core, a texture setup the draw does not bind), and the walk's EE primitives
    /// each with the draw it comes before (`AFTER_EFFECTS`: after the effects).
    ///
    /// `prev` gets, for each vertex of `out`, where field `field - 1` drew it: with `track`, a run of the native picture matched to
    /// that field's run of the same mesh with the least vertex travel (within `MATCH_UNITS`, each of its runs taken once) takes each
    /// vertex's place there; every other vertex (and every one without `track`) is its own.
    #[allow(clippy::too_many_arguments)]
    /// The skin job of the last fields' draw records run again (`uyart::hw::SkinJoints`: each record's joint matrices as
    /// func_003D00F8 had them), each field's once and in the order the game ran it, into `skinned` by each mesh's positions address
    /// (kept until a later output at that address, as RAM keeps the job's output: on Veldin a scene's cut drew a frame whose
    /// records were skinned more than five fields before): the entry's destination plus the
    /// first array padded to 16 bytes (the skinned layout's colours, the rigid one's texture coordinates). A record (the transfer's
    /// source less 0x40, as the transfer had it): +0 its mesh entry count n, +0x14 its second group's quadword count << 16 | alpha,
    /// its light section from +0x40 (`uyafmt::moby::JobLights`), its entries from +0xC0 (packet B's address << 4 | its quadword
    /// count, the destination); the second group at +0x40 + 0x10 (n / 2 + 9), its matrix rows (`RigidInputs`) and its entries from
    /// +0x40 ended by a zero word, which the next transfer (from the group) skins with the same joint matrices. The transfers are
    /// the last `SKIN_FIELDS` fields' (`rtgame`: a field's job skins the frame the next field sends, a later one when the game
    /// misses fields, or the field's own second frame). What a record cannot be skinned for is returned.
    fn skin_pass(&mut self, ram: &[u8], skin: &[(u64, std::sync::Arc<uyart::hw::SkinJoints>)]) -> Vec<String> {
        // A field before the last one done: another run's (`uya native` of another dump).
        if skin.last().is_some_and(|(f, _)| self.skin_done.is_some_and(|d| *f < d)) {
            (self.skin_done, self.skinned) = (None, HashMap::new());
        }
        let mut errors = Vec::new();
        for (f, sj) in skin {
            if self.skin_done.is_some_and(|d| *f <= d) {
                continue;
            }
            self.skin_done = Some(*f);
            self.skin_passes += 1;
            errors.extend(self.skin_field(ram, sj));
            // The addresses no pass has written for `SKIN_AGE` passes, looked for once every 60.
            if self.skin_passes % 60 == 0 {
                let now = self.skin_passes;
                self.skinned.retain(|_, (pass, _)| now - *pass < SKIN_AGE);
            }
        }
        errors
    }

    /// An output at `at`, the newest of the `SKIN_KEPT` kept there.
    fn put_skinned(&mut self, at: u32, out: std::sync::Arc<SkinOut>) {
        let (pass, v) = self.skinned.entry(at).or_default();
        *pass = self.skin_passes;
        v.insert(0, out);
        v.truncate(SKIN_KEPT);
    }

    /// `skin_pass` of one field's transfers.
    fn skin_field(&mut self, ram: &[u8], skin: &uyart::hw::SkinJoints) -> Vec<String> {
        use uyafmt::moby::{JobLights, PacketB, RigidInputs, SkinInputs};
        let mut kept = std::mem::take(&mut self.skin_kept);
        let mut errors = Vec::new();
        if skin.records.is_empty() {
            return errors;
        }
        let trig = if skin.captured { Vec::new() } else { match SkinInputs::trig_table(&skin.trig) {
            Ok(t) => t,
            Err(e) => return vec![format!("native: the skin job's (cos, sin) table: {e}")],
        } };
        // A mesh entry (packet B's address << 4 | its quadword count, the destination) at `o` of a transfer's bytes.
        let entry = |d: &[u8], o: usize| -> Option<(u32, u32, u32)> {
            let w = |o: usize| d.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")));
            let (w0, w1) = (w(o)?, w(o + 4)?);
            Some(((w0 & !0xff) >> 4, w0 & 0xff, w1))
        };
        // The inputs a mesh the field before did not skin the same is skinned from: made when the first needs them, its joint
        // matrices filled again (once) for each record that has one.
        let mut inputs: Option<SkinInputs> = None;
        // The cache key of a mesh entry, built again for each (`kept`).
        let mut key: Vec<u8> = Vec::new();
        // The second group the record before names, which the next transfer is: its address, the record's +0x14 and lights.
        let mut second: Option<(u32, u32, JobLights)> = None;
        let mut output_cursor = 0;
        for (record, &(src, qwc, at)) in skin.records.iter().enumerate() {
            let joint_len = if skin.captured { 0 } else { 0x1c00 };
            let Some(b) = skin.bytes.get(at..at + joint_len + 0x40 + 16 * qwc as usize) else {
                errors.push("native: the skin job's transfers past their bytes".into());
                break;
            };
            // The scratchpad's joint matrices, then RAM from 0x40 before the source (`uyart::hw::SkinJoints`).
            let (joint_bytes, d) = b.split_at(joint_len);
            let w = |o: usize| u32::from_le_bytes(d[o..o + 4].try_into().expect("4 bytes"));
            let group = second.take().filter(|g| g.0 == src);
            // The skin job's transfers: a record's light section and mesh entries (from +0x40: 8 quadwords and the n entries with
            // their zero one, 9 + n / 2 quadwords), or the second group the record before names; another transfer to 0x1C00 is
            // other code's.
            let n = w(0) as usize;
            if group.is_none() && (n == 0 || n > 200 || qwc as usize != 9 + n / 2) {
                continue;
            }
            let r = (|| -> Result<(), String> {
                let mut joints_filled = false;
                let ends = |from: usize| -> Result<Vec<(u32, u32, u32)>, String> {
                    let mut v = Vec::new();
                    let mut o = from;
                    while let Some(e) = entry(d, o).filter(|e| e.0 != 0 || e.1 != 0) {
                        v.push(e);
                        o += 8;
                    }
                    entry(d, o).map(|_| v).ok_or_else(|| format!("native: the skin job's transfer from {src:#x}: its mesh entries do not end"))
                };
                let (entries, lights, rigid) = match group {
                    Some((_, f14, lights)) => (ends(0x80)?, lights, Some(RigidInputs::read(d.get(0x40..0x70).ok_or("native: a draw record's second group past its transfer")?, f14)?)),
                    None => {
                        let lights = JobLights::read(&d[..0xc0])?;
                        let f14 = w(0x14);
                        if f14 >> 16 != 0 {
                            second = Some((src + 16 * ((n as u32 >> 1) + 9), f14, lights));
                        }
                        let entries = (0..n).map(|k| entry(d, 0xc0 + 8 * k)).collect::<Option<Vec<_>>>().ok_or("native: a draw record's mesh entries past its transfer")?;
                        (entries, lights, None)
                    }
                };
                let (wrap, dirs) = (lights.wrap.map(|w| w.min(0.0)), lights.dirs);
                for (pb, qwc, dest) in entries {
                    let bytes = ram.get(pb as usize..pb as usize + 16 * qwc as usize).ok_or_else(|| format!("native: packet B at {pb:#x} past RAM"))?;
                    if !matches!(self.packet_bs.get(&pb), Some((b, _, _)) if b.as_slice() == bytes) {
                        let p = uyafmt::moby::decode_packet_b(bytes).map_err(|e| format!("native: packet B at {pb:#x}: {e}"))?;
                        // The joint matrices the packet reads (its pairs' and its rows' own).
                        let mut used: Vec<usize> = match &p {
                            PacketB::Skinned(s) => s.pairs.iter().map(|&(j, _)| j as usize).chain(s.rows.iter().map(|r| r.joint as usize)).collect(),
                            PacketB::Rigid(r) => r.rows.iter().flat_map(|r| r.joints.iter().take((r.joint_count as usize).max(1)).map(|&j| j as usize)).collect(),
                        };
                        used.retain(|&j| j < 112);
                        used.sort_unstable();
                        used.dedup();
                        let vertices = match &p {
                            PacketB::Skinned(s) => s.rows.len() + s.copies.len(),
                            PacketB::Rigid(r) => r.rows.len(),
                        };
                        let mesh = uyafmt::moby::Mesh { record: 0, b_qwords: qwc as u8, position_qwords: 0, normal_qwords: 0, vertices: vertices as u8, a_at: 0, b_at: 0, packet_a: None, packet_b: Some(p) };
                        self.packet_bs.insert(pb, (bytes.to_vec(), mesh, used));
                    }
                    let (_, mesh, used) = &self.packet_bs[&pb];
                    if skin.captured {
                        let at = skin.outputs[output_cursor..].iter().position(|o| o.record == record && o.dest == dest)
                            .ok_or_else(|| format!("native: missing captured skin output for record {record} at {dest:#x}"))? + output_cursor;
                        output_cursor = at + 1;
                        let (positions, normals) = captured_skin(mesh, &skin.outputs[at], &mut self.skin_state.staging_normals)?;
                        let flat = match &mesh.packet_b {
                            Some(PacketB::Skinned(s)) => positions.len() - s.constant_normals as usize,
                            _ => positions.len(),
                        };
                        let is_rigid = rigid.is_some();
                        let terms = normals.iter().map(|v| {
                            let len = if is_rigid { 1.0 } else { (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(f32::MIN_POSITIVE) };
                            [0, 1].map(|k| {
                                let d = (dirs[0][k] * v[0] + dirs[1][k] * v[1] + dirs[2][k] * v[2]) / len;
                                if is_rigid { d.max(0.0) } else { d.max(d * wrap[k]) }
                            })
                        }).collect();
                        let positions_at = dest + (4 * mesh.vertices as usize).div_ceil(16) as u32 * 16;
                        self.put_skinned(positions_at, std::sync::Arc::new(SkinOut { positions, normals, terms, flat, lights }));
                        continue;
                    }
                    key.clear();
                    for &j in used {
                        key.extend_from_slice(&joint_bytes[64 * j..64 * j + 64]);
                    }
                    let l = &lights;
                    key.extend(l.dirs.iter().chain(&l.colours).flatten().chain(&l.wrap).chain(&l.base).flat_map(|x| x.to_le_bytes()));
                    key.extend_from_slice(&l.flat);
                    if let Some(g) = &rigid {
                        key.extend(g.view.iter().flatten().flat_map(|x| x.to_le_bytes()));
                        key.push(g.alpha);
                    }
                    let positions_at = dest + (4 * mesh.vertices as usize).div_ceil(16) as u32 * 16;
                    // The output the field made already, else the field before's (moved into this field's, a key being in one only).
                    let again = match self.skin_kept.get(&pb).and_then(|v| v.iter().find(|(k, _)| *k == key)) {
                        Some((_, o)) => Some(o.clone()),
                        None => kept.get_mut(&pb).and_then(|v| v.iter().position(|(k, _)| *k == key).map(|at| v.swap_remove(at))).map(|(k, o)| {
                            self.skin_kept.entry(pb).or_default().push((k, o.clone()));
                            o
                        }),
                    };
                    if let Some(out) = again {
                        self.put_skinned(positions_at, out);
                        continue;
                    }
                    // Not skinned the same before: the job's inputs, with this record's joint matrices.
                    let inputs = inputs.get_or_insert_with(|| SkinInputs { joints: Vec::with_capacity(112), lights, rigid, trig: trig.clone() });
                    inputs.lights = lights;
                    inputs.rigid = rigid;
                    if !joints_filled {
                        inputs.joints.clear();
                        inputs.joints.extend((0..112).map(|j| -> [[f32; 4]; 4] { std::array::from_fn(|r| std::array::from_fn(|c| f32::from_le_bytes(joint_bytes[64 * j + 16 * r + 4 * c..64 * j + 16 * r + 4 * c + 4].try_into().expect("4 bytes")))) }));
                        joints_filled = true;
                    }
                    let out = uyafmt::moby::skin_fast(mesh, inputs, &mut self.skin_state).map_err(|e| format!("native: the skin job of packet B at {pb:#x}: {e}"))?;
                    let n = out.positions.len();
                    let flat = match &mesh.packet_b {
                        Some(PacketB::Skinned(s)) => n - s.constant_normals as usize,
                        _ => n,
                    };
                    // The skin job's light terms: the normal dotted with the record's light rows (times 1 / its length for the
                    // skinned layout), max(d, d min(wrap, 0)) skinned, max(d, 0) rigid.
                    let rigid = out.uvs.is_some();
                    let terms = out
                        .normals
                        .iter()
                        .map(|v| {
                            let len = if rigid { 1.0 } else { (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(f32::MIN_POSITIVE) };
                            [0, 1].map(|k| {
                                let d = (dirs[0][k] * v[0] + dirs[1][k] * v[1] + dirs[2][k] * v[2]) / len;
                                if rigid { d.max(0.0) } else { d.max(d * wrap[k]) }
                            })
                        })
                        .collect();
                    let out = std::sync::Arc::new(SkinOut { positions: out.positions, normals: out.normals, terms, flat, lights });
                    self.skin_kept.entry(pb).or_default().push((key.clone(), out.clone()));
                    self.put_skinned(positions_at, out);
                }
                Ok(())
            })();
            if let Err(e) = r {
                errors.push(e);
            }
        }
        errors
    }

    pub fn frame(&mut self, _pool: &crate::native::Pool, device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, ram: &[u8], walk: Walk, g: &MobyGlobals, cam: &Camera, field: u64, out: &mut Vec<ObjVertex>, prev: &mut Vec<[f32; 3]>, lit: &mut Vec<crate::lighting::MobyLit>, light_rows: &mut Vec<[[f32; 4]; 4]>, skin: &[(u64, std::sync::Arc<uyart::hw::SkinJoints>)], track: bool, draws: &mut Vec<((u32, u16), Draw, u32)>, targets: &mut Vec<(u32, u32)>, fx_q: &mut Option<f32>, transfers: &mut [Transfer], passes: &mut [std::ops::Range<usize>]) -> Result<(usize, Vec<(String, usize)>, Vec<(usize, EePrim)>), String> {
        let Walk { runs, constants, mut ee, stopped, fx_q: held, images, unpack_words, .. } = walk;
        let mut vu: Option<crate::vu1::Vu1> = None;
        let mut vu_gs: Option<EeGs> = None;
        if eedump() {
            for (r, run) in runs.iter().enumerate() {
                eprintln!("run {r}: FRAME {:#x} TEST {:#x} ALPHA {:#x} packet A {:x?} other {:x?}", run.frame, run.test, run.alpha, run.vu.packet_a, run.other);
            }
        }
        if let Some(HeldQ::Value(q)) = held {
            *fx_q = Some(q);
        }
        // The run whose last vertex's Q program 6's first packet takes.
        let q_run = match held {
            Some(HeldQ::Run(r)) => Some(r),
            _ => None,
        };
        let core = rd_u32(ram, g.core as usize)?;
        let (hero, hero_at, wrench_at) = (rd_u32(ram, g.armor_entry as usize)?, rd_u32(ram, g.armor_buffer as usize)?, rd_u32(ram, g.wrench_block as usize)?);
        // What the frame decodes from the disc the first time it needs it (the level's load decoded what its RAM named), logged
        // when it took the frame long (`UYA_NATIVE_TIMES`).
        let loads = (std::time::Instant::now(), self.armors.len(), self.materials.len(), self.scenes.len(), self.bodies.len(), self.gadgets.len());
        let (clank, ship, held) = self.prepare(device, queue, ram, g)?;
        let ga = &g.gadget;
        if walk_timed() && loads.0.elapsed().as_secs_f64() > 0.002 {
            println!("native: the frame's first-use moby loads took {:.1} ms: armors {} -> {}, materials {} -> {}, scenes {} -> {}, bodies {} -> {}, gadgets {} -> {}", loads.0.elapsed().as_secs_f64() * 1e3, loads.1, self.armors.len(), loads.2, self.materials.len(), loads.3, self.scenes.len(), loads.4, self.bodies.len(), loads.5, self.gadgets.len());
        }
        let s = &g.scene;
        let (scene_k, scene_at, class10) = (rd_u32(ram, s.scene_index as usize)?, rd_u32(ram, s.scene_base as usize)?, rd_u32(ram, s.class10_texture as usize)?);
        let (body_v, body_t, body_at) = (rd_u32(ram, s.body_variant as usize)?, rd_u32(ram, s.body_texture as usize)?, rd_u32(ram, s.body_base as usize)?);
        let identity: [u8; 16] = std::array::from_fn(|i| i as u8);
        // The camera 4 x 4 (rows at the view camera - 0x120) and its inverse.
        let vc = g.view_camera as usize - 0x120;
        let mut c = [[0f64; 4]; 4];
        for (r, row) in c.iter_mut().enumerate() {
            for (k, x) in row.iter_mut().enumerate() {
                *x = rd_f32(ram, vc + 16 * r + 4 * k)? as f64;
            }
        }
        let ci = invert(c).ok_or("native: the camera 4 x 4 has no inverse")?;
        let (vf24x, vf23x, vf22) = (constants[0][0], constants[1][0], constants[5]);
        // Per-pixel lighting uses the light inputs and transformed normals from the field's skin jobs.
        let lighting = crate::enhance::lighting();
        let mut lit_count = LitStats::default();
        let skin_time = walk_timed().then(std::time::Instant::now);
        let skin_errors = if lighting { self.skin_pass(ram, skin) } else { Vec::new() };
        let skin_ms = skin_time.map_or(0.0, |t| t.elapsed().as_secs_f64() * 1e3);
        let mesh_time = walk_timed().then(std::time::Instant::now);
        // Shared skin staging remains serial. Once captured, each run's normal/light
        // preparation and world positions are independent; assembly below keeps draw order.
        let mut prepared = std::mem::take(&mut self.prepared);
        prepared.resize_with(runs.len(), PreparedRun::default);
        let skinned = &self.skinned;
        light_rows.resize(runs.len(), [[0.0; 4]; 4]);
        // The runs in small chunks the crew's threads and this one take as they finish (a hero's run costs many times a crate's).
        let chunk = runs.len().div_ceil(16).max(1);
        let parts: Vec<std::sync::Mutex<(&mut [PreparedRun], &mut [[[f32; 4]; 4]])>> = prepared.chunks_mut(chunk).zip(light_rows.chunks_mut(chunk)).map(std::sync::Mutex::new).collect();
        crate::crew::run(parts.len(), &|part| {
            let mut guard = parts[part].lock().expect("a chunk of the moby lighting");
            let (ps, ls) = &mut *guard;
            for (i, ((run, p), rows)) in runs[part * chunk..].iter().zip(ps.iter_mut()).zip(ls.iter_mut()).enumerate() {
                let u = run.vu.rows.map(|r| r.map(|x| x as f64));
                p.stats = LitStats::default();
                p.world.clear();
                if run.frame == 0x800f4 {
                    cpu_wide(|| world_all(&u, &ci, cam.eye, vf23x, vf22, &run.vu.positions, &mut p.world));
                }
                if lighting && run.frame == 0x800f4 {
                    let skin = run.vu.positions_at.and_then(|at| skinned.get(&at)).map_or(&[][..], |(_, v)| v.as_slice());
                    moby_lit((part * chunk + i) as u32, rows, &mut p.stats, &u, &ci, run, skin, &mut p.lit);
                } else {
                    p.lit.clear();
                    p.lit.resize(run.vu.positions.len(), crate::lighting::MobyLit::default());
                }
            }
        });
        drop(parts);
        let pick = crate::surfaces::Pick::new(cam);
        // The runs of the field before, by mesh, each taken by at most one run of this frame; and this frame's runs of the native
        // picture (index, origin) to keep for the next.
        let last = self.last.take().filter(|l| track && l.field + 1 == field);
        let mut by_mesh: HashMap<u32, Vec<usize>> = HashMap::new();
        if let Some(l) = &last {
            for (k, r) in l.runs.iter().enumerate() {
                by_mesh.entry(r.pa).or_default().push(k);
            }
        }
        let mut taken = vec![false; last.as_ref().map_or(0, |l| l.runs.len())];
        let mut kept = Vec::new();
        let mut drawn = 0;
        let mut skipped: Vec<(String, usize)> = skin_errors.into_iter().map(|e| (e, 1)).collect();
        let mut skip = |why: String| match skipped.iter_mut().find(|(w, _)| *w == why) {
            Some((_, n)) => *n += 1,
            None => skipped.push((why, 1)),
        };
        // The texture bound last, with the source of its materials.
        let mut last_key: Option<(u32, Key)> = None;
        // The EE's own primitives go between the draws of the runs they come between (each pair's draw index, set in place: `placed`
        // of them so far), and no draw spans them.
        let mut placed = 0;
        let mut barrier = usize::MAX;
        // One pass over the runs in order does what depends on the runs before it (the matching to the field before, the textures
        // bound last, the materials made); the vertices of the runs it draws are built on the crew's threads; a last pass in order
        // places the EE's primitives, runs the programs the port runs and puts each run's vertices into the draws.
        let mut work = std::mem::take(&mut self.work);
        let mut tasks: Vec<Task> = Vec::new();
        let mut plans: Vec<Plan> = Vec::with_capacity(runs.len());
        for (r, run) in runs.iter().enumerate() {
            let pa = match run.vu.packet_a {
                Some(pa) => pa,
                None => {
                    plans.push(Plan::Fail("native: a moby run without packet A".into()));
                    break;
                }
            };
            let armor_mesh = |entry: u32, at: u32| if entry == NO_ARMOR { None } else { pa.checked_sub(at).and_then(|o| self.armors[&entry].1.get(&o)).map(|&m| (Src::Armor(entry), m)) };
            let scene_mesh = || pa.checked_sub(scene_at).and_then(|o| self.scenes.get(&scene_k)?.as_ref()?.1.get(&o)).map(|&(c, m)| (Src::Scene(scene_k, c), m));
            let body_mesh = || pa.checked_sub(body_at).and_then(|o| self.bodies.get(&(body_v, body_t))?.1.get(&o)).map(|&m| (Src::Body(body_v, body_t), m));
            let gadget_mesh = || held.and_then(|id| pa.checked_sub(ga.buffer).and_then(|o| self.gadgets[&id].1.get(&o)).map(|&m| (Src::Gadget(id), m)));
            let found = pa.checked_sub(core).and_then(|o| self.by_packet.get(&o)).map(|&(c, m)| (Src::Core(c), m)).or_else(|| armor_mesh(hero, hero_at)).or_else(|| armor_mesh(WRENCH_ENTRY, wrench_at)).or_else(scene_mesh).or_else(body_mesh).or_else(gadget_mesh);
            let Some((src, mi)) = found else {
                // A class the game loaded at run time: the PS2's own way, program 0 on the chain's VU1 memory (the last pass).
                plans.push(match run.other {
                    Some((reg, val)) => Plan::Skip(format!("GS register {reg:#x} = {val:#x}")),
                    None => Plan::Vu,
                });
                continue;
            };
            let plan: Plan = 'plan: {
                // The mesh, the bytes that map its setups' numbers, and its textures' materials id.
                let (mesh, map, tex_src) = match src {
                    Src::Core(c) => (&self.classes[c].as_ref().expect("indexed classes are decoded").meshes[mi], &self.records[c], 0),
                    Src::Armor(e) => (&self.armors[&e].0.meshes[mi], &g.armor_remap, armor_id(e)),
                    Src::Scene(k, c) => {
                        let sc = &self.scenes[&k].as_ref().expect("indexed scenes are loaded").0.block.classes[c];
                        if sc.id == 10 {
                            (&sc.class.meshes[mi], &s.body_remap, class10_id(class10))
                        } else {
                            (&sc.class.meshes[mi], &identity, scene_id(k, c))
                        }
                    }
                    Src::Body(v, t) => (&self.bodies[&(v, t)].0.class.meshes[mi], &s.body_remap, body_id(v, t)),
                    Src::Gadget(id) => (&self.gadgets[&id].0.meshes[mi], &GADGET_REMAP, gadget_id(id)),
                };
                let a = mesh.packet_a.as_ref().expect("indexed meshes have packet A");
                // A core class of mode 1 gives its 0xFF setups the ship's texture, of mode 2 or more Clank's (func_L01_00428750).
                let own = match src {
                    Src::Core(c) if self.modes[c] == 1 => ship,
                    Src::Core(c) if self.modes[c] >= 2 => clank,
                    _ => None,
                };
                if eedump() {
                    eprintln!("run {r} draws {src:?} mesh {mi}: setups {:?} strips {:?}", a.textures.iter().map(|t| (t.local, map.get(t.local.max(0) as usize).copied())).collect::<Vec<_>>(), a.strips.iter().map(|s| s.texture).collect::<Vec<_>>());
                }
                let keys: Result<Vec<Option<(u32, Key)>>, String> = a.strips.iter().map(|s| s.texture.map(|k| a.textures.get(k).ok_or_else(|| "native: moby strip texture setup past the packet's".to_string()).and_then(|t| moby_key(t, map, tex_src, self.specials, own))).transpose()).collect();
                // A setup naming a texture its source does not have (a scene class's number past its images, a slot other than 0 of
                // the body's or class 10's single texture) is not drawn.
                let keys = keys.and_then(|ks| {
                    for (s, k) in ks.iter().flatten() {
                        let n = self.materials[&(*s, 0)].len();
                        if k.0 as usize >= n {
                            return Err(format!("native: moby texture {} of a source with {n}", k.0));
                        }
                    }
                    Ok(ks)
                });
                // The frame buffer: the native picture (FRAME_1 0x800F4), or another PSMCT32 one every bit written (a cutscene
                // draws a character into a texture a monitor shows): its key (FBP, FBW: `eedraw::MAIN`'s), its window (XYOFFSET_1
                // and SCISSOR_1's height) in GS pixels.
                let fb = (run.frame & 0x1ff | (run.frame >> 16 & 0x3f) << 16) as u32;
                let window = if run.frame == 0x800F4 {
                    None
                } else if run.frame >> 24 & 0x3f != 0 || run.frame >> 32 != 0 {
                    break 'plan Plan::Skip(format!("FRAME_1 {:#x}", run.frame));
                } else {
                    let rows = (run.scissor >> 48 & 0x7ff) as u32 + 1;
                    if !targets.contains(&(fb, rows)) {
                        targets.push((fb, rows));
                    }
                    Some(((run.frame >> 16 & 0x3f) as f32 * 64.0, rows as f32, (run.xyoffset & 0xffff) as f32 / 16.0, (run.xyoffset >> 32 & 0xffff) as f32 / 16.0))
                };
                if let Some((reg, val)) = run.other {
                    break 'plan Plan::Skip(format!("GS register {reg:#x} = {val:#x}"));
                }
                let a_ = |v: u64, s: u32| ((v >> s) & 3) as u8;
                let blend = match Blend::of(a_(run.alpha, 0), a_(run.alpha, 2), a_(run.alpha, 4), a_(run.alpha, 6), (run.alpha >> 32) as u8) {
                    Ok(b) => b,
                    Err(e) => break 'plan Plan::Skip(e),
                };
                let keys = match keys {
                    Ok(k) => k.into_iter().map(|k| k.map(|(s, k)| (s, (k.0, k.1, k.2, k.3, k.4, blend, k.6)))).collect::<Vec<_>>(),
                    Err(e) => break 'plan Plan::Skip(e),
                };
                // TEST_1: Z test GEQUAL; no alpha test, or GEQUAL AREF with a failing pixel writing only its colour (RGB_ONLY), or NEVER
                // with every pixel writing its colour and alpha but not its depth (FB_ONLY: `moby_set` 256 + 1024; 0x51001, level 28's
                // sewer pipe, class 144's sixteen meshes).
                let t = run.test;
                let set = match (t & 1, (t >> 1) & 7, ((t >> 4) & 0xff) as u16, (t >> 12) & 3, (t >> 16) & 7) {
                    (0, _, _, _, 0b101) => 0,
                    (1, 5, aref, 3, 0b101) => aref,
                    (1, 0, _, 1, 0b101) => 256 + 1024,
                    _ => break 'plan Plan::Skip(format!("TEST_1 {t:#x}")),
                };
                let aref = set;
                for (s, k) in keys.iter().flatten() {
                    if !self.materials.contains_key(&(*s, aref)) {
                        let m = moby_set(&self.materials[&(*s, 0)], aref);
                        self.materials.insert((*s, aref), m);
                    }
                    if let Err(e) = self.materials.get_mut(&(*s, aref)).expect("inserted above").get(device, layout, *k) {
                        break 'plan Plan::Fail(e);
                    }
                }
                if run.textured {
                    last_key = None;
                }
                if keys.first().is_some_and(|k| k.is_none()) && last_key.is_none() {
                    break 'plan Plan::Skip("the first strip draws with the texture the chain's own GS writes bound".into());
                }
                // The rows U (clip = (x, y, z, 1) . U).
                let u: [[f64; 4]; 4] = run.vu.rows.map(|r| r.map(|x| x as f64));
                let clip = |p: [i16; 3]| -> [f32; 4] {
                    let v = [p[0] as f32, p[1] as f32, p[2] as f32];
                    [0, 1, 2, 3].map(|k| run.vu.rows[3][k] + v[0] * run.vu.rows[0][k] + v[1] * run.vu.rows[1][k] + v[2] * run.vu.rows[2][k])
                };
                // The vertex where the GS puts it: the program sends X = FTOI4(clip.x x Q + vf22.x) and Y likewise (12.4 fixed point,
                // truncated; `uyafmt::moby`, the per-vertex formula), so the world position is the one whose screen point is that,
                // clip x and y moved there with z and w kept (clip . C^-1 / 1024 + the eye). Vertices the GS puts at one point are at
                // one point natively: level 30's water is four instances whose shared edges are 0.015 GS pixels apart in float and
                // one 12.4 value on the GS (field 40500: 238.5034 and 238.5186, both 238.5).
                let world = |p: [i16; 3]| -> [f32; 3] { world_at(&u, &ci, cam.eye, vf23x, vf22, p) };
                // The run of the field before this one continues (`MATCH_UNITS`), for the native picture's runs.
                let mut before: Option<&LastRun> = None;
                if window.is_none() && track {
                    let origin = world([0, 0, 0]);
                    let places = &prepared[r].world;
                    if let Some(l) = &last {
                        let best = by_mesh.get(&pa).into_iter().flatten().copied()
                            .filter(|&k| !taken[k])
                            .filter_map(|k| l.runs[k].match_score(origin, places).map(|score| (k, score)))
                            .min_by(|a, b| a.1.total_cmp(&b.1));
                        if let Some((k, _)) = best {
                            taken[k] = true;
                            before = Some(&l.runs[k]);
                        }
                    }
                    kept.push(LastRun { pa, origin, places: places.clone() });
                }
                if q_run == Some(r) {
                    // The program sends the last vertex's ST with Q = vf23.x / its clip w (the module doc's 1/Q).
                    if let Some(v) = a.strips.last().and_then(|s| s.vertices.last()) {
                        let Some(&p) = run.vu.positions.get(v.index as usize) else { break 'plan Plan::Fail("native: moby vertex past the skinned positions".into()) };
                        *fx_q = Some(vf23x / clip(p)[3]);
                    }
                }
                drawn += 1;
                let stats = &prepared[r].stats;
                lit_count.lit += stats.lit;
                lit_count.unskinned += stats.unskinned;
                lit_count.differ += stats.differ;
                lit_count.no_light += stats.no_light;
                lit_count.vertices += stats.vertices;
                for k in 0..2 { lit_count.error[k] += stats.error[k]; lit_count.differ_vertices[k] += stats.differ_vertices[k]; }
                // Each strip's texture and material, in order: a strip without a setup keeps the texture bound before it (its
                // source's materials), drawn with this run's blend.
                let ti = tasks.len();
                if work.len() <= ti {
                    work.resize_with(ti + 1, RunWork::default);
                }
                work[ti].strips.clear();
                for k in &keys {
                    if let Some(k) = k {
                        last_key = Some(*k);
                    }
                    let Some((tsrc, k)) = last_key else { break 'plan Plan::Fail("native: the first moby strip has no texture bound".into()) };
                    let key = (k.0, k.1, k.2, k.3, k.4, blend, k.6);
                    let set = (tsrc, aref);
                    if !self.materials.contains_key(&set) {
                        let m = moby_set(&self.materials[&(tsrc, 0)], aref);
                        self.materials.insert(set, m);
                    }
                    if let Err(e) = self.materials.get_mut(&set).expect("inserted above").get(device, layout, key) {
                        break 'plan Plan::Fail(e);
                    }
                    work[ti].strips.push((set, key));
                }
                // `uya native --surfaces`: the hero's armor (Ratchet's body) in world space, for the world's units per metre.
                let hero_run = crate::surfaces::on() && matches!(src, Src::Armor(e) if e == hero);
                tasks.push(Task { r, run, a, window, before, hero_run, fb });
                Plan::Draw(ti)
            };
            let failed = matches!(plan, Plan::Fail(_));
            plans.push(plan);
            if failed {
                break;
            }
        }
        // Each drawn run's vertices, in strip order, into its own buffers.
        let materials = &self.materials;
        let build = |task: &Task, w: &mut RunWork| -> Result<(), String> {
            let (r, run, a, window, before, hero_run) = (task.r, task.run, task.a, task.window, task.before, task.hero_run);
            let RunWork { strips, counts, out, prev, lit, screens, fogs, vertices, .. } = w;
            out.clear();
            prev.clear();
            lit.clear();
            counts.clear();
            let run_lit = &prepared[r].lit;
            let clip = |p: [i16; 3]| -> [f32; 4] {
                let v = [p[0] as f32, p[1] as f32, p[2] as f32];
                [0, 1, 2, 3].map(|k| run.vu.rows[3][k] + v[0] * run.vu.rows[0][k] + v[1] * run.vu.rows[1][k] + v[2] * run.vu.rows[2][k])
            };
            let uv = |i: usize| -> Result<[f32; 2], String> {
                match (&run.vu.uvs, &a.uvs) {
                    (Some(r), _) => r.get(i).map(|t| [t[0] as f32 / 4096.0, t[1] as f32 / 4096.0]).ok_or_else(|| "native: moby vertex past the rigid coordinates".into()),
                    (None, Some(p)) => p.get(i).copied().ok_or_else(|| "native: moby vertex past packet A's coordinates".into()),
                    (None, None) => Err("native: a moby run with no texture coordinates".into()),
                }
            };
            // Each position's screen point and fog, made for all of them at once: the same operations of the one-at-a-time
            // `screen_of` and `fog_of`, four at a time.
            let culls = run.variant_a && (a.cull == 0 || a.cull == 2);
            cpu_wide(|| {
                screens.clear();
                if culls {
                    screens_all(&run.vu.rows, vf23x, vf22, &run.vu.positions, screens);
                }
                fogs.clear();
                fogs_all(&run.vu.rows, run.variant_a, vf22[3], vf24x, &run.vu.positions, fogs);
            });
            vertices.clear();
            vertices.resize(run.vu.positions.len(), NO_CORNER);
            // The vertex's position in the mesh's stream (every strip's vertices in order): the orientation test's parity.
            let mut stream = 0;
            for (s, &((tsrc, aref), key)) in a.strips.iter().zip(strips.iter()) {
                let first = out.len();
                for k in 2..s.vertices.len() {
                    if s.vertices[k].skip {
                        continue;
                    }
                    let tri = [s.vertices[k - 2].index as usize, s.vertices[k - 1].index as usize, s.vertices[k].index as usize];
                    let pos = |i: usize| -> Result<[i16; 3], String> { run.vu.positions.get(i).copied().ok_or_else(|| "native: moby vertex past the skinned positions".into()) };
                    let screen_at = |i: usize| -> Result<[f32; 2], String> { screens.get(i).copied().ok_or_else(|| "native: moby vertex past the skinned positions".into()) };
                    if run.variant_a && (a.cull == 0 || a.cull == 2) {
                        let sc = [screen_at(tri[0])?, screen_at(tri[1])?, screen_at(tri[2])?];
                        let e1 = [sc[1][0] - sc[0][0], sc[1][1] - sc[0][1]];
                        let e2 = [sc[2][0] - sc[1][0], sc[2][1] - sc[1][1]];
                        let (p, q) = if (stream + k) % 2 == 0 { (e1, e2) } else { (e2, e1) };
                        let negative = p[0] * q[1] - q[0] * p[1] < 0.0;
                        if negative as i8 == a.cull / 2 {
                            continue;
                        }
                    }
                    for &i in &tri {
                        if let Some(at) = vertices.get(i).copied().filter(|&at| at != NO_CORNER) {
                            let (v, b) = (out[at as usize], prev[at as usize]);
                            out.push(v);
                            prev.push(b);
                            lit.push(run_lit[i]);
                            continue;
                        }
                        let p = pos(i)?;
                        let colour = *run.vu.colours.get(i).ok_or("native: moby vertex past the colours")?;
                        let t = uv(i)?;
                        let v = match window {
                            None => {
                                let w = prepared[r].world[i];
                                if hero_run {
                                    crate::surfaces::hero_point(w);
                                }
                                ObjVertex { pos: w, st: [t[0], t[1], 1.0], colour, f: fogs[i] }
                            }
                            // Another frame buffer: the program's own screen point in the window (the rows are its camera's), the
                            // view depth (1/Q is 32 times it) and ST as the GS gets them (S Q, T Q, Q), all linear on the screen.
                            Some((w, h, xo, yo)) => {
                                let cl = clip(p);
                                let q = vf23x / cl[3];
                                // X and Y as the GS gets them (FTOI4: 1/16 pixel, truncated).
                                let (sx, sy) = (((cl[0] * q + vf22[0]) * 16.0).trunc() / 16.0, ((cl[1] * q + vf22[1]) * 16.0).trunc() / 16.0);
                                // Half a GS pixel on: the GS's pixel centre is its integer coordinate (`eedraw`).
                                ObjVertex { pos: [(sx - xo + 0.5) / w * 2.0 - 1.0, 1.0 - (sy - yo + 0.5) / h * 2.0, cl[3] / (32.0 * vf23x)], st: [t[0] * q, t[1] * q, q], colour, f: fogs[i] }
                            }
                        };
                        let b = before.map_or(v.pos, |run| run.places[i]);
                        vertices[i] = out.len() as u32;
                        out.push(v);
                        prev.push(b);
                        lit.push(run_lit[i]);
                    }
                    // `uya native --surfaces`: the triangle in the world with its texture's name, for the texture's world size per
                    // repeat (`surfaces::object_triangle`).
                    if let Some(pick) = pick.as_ref().filter(|_| window.is_none()) {
                        let n = out.len();
                        if let Some(distance) = pick.hit([out[n - 3].pos, out[n - 2].pos, out[n - 1].pos]) {
                            let name = materials[&(tsrc, 0)].names[key.0 as usize].map(|n| n.stem());
                            println!("pick: moby run {r} distance {distance:.4} source {tsrc} texture {name:?} key {key:?} aref {aref} colours {:?}", [out[n - 3].colour, out[n - 2].colour, out[n - 1].colour]);
                        }
                    }
                    if crate::surfaces::on() && window.is_none() {
                        if let Some(name) = materials[&(tsrc, 0)].names.get(key.0 as usize).copied().flatten() {
                            let n = out.len();
                            let t = [out[n - 3], out[n - 2], out[n - 1]];
                            crate::surfaces::object_triangle(name, t.map(|v| v.pos), t.map(|v| [v.st[0], v.st[1]]));
                        }
                    }
                }
                counts.push((out.len() - first) as usize);
                stream += s.vertices.len();
            }
            Ok(())
        };
        // `build` has one call site, so that it is compiled with the CPU's wider instructions (`cpu_wide`).
        let build_wide = |t: &Task, w: &mut RunWork| cpu_wide(|| build(t, w).err());
        if pick.is_some() || crate::surfaces::on() {
            for (ti, t) in tasks.iter().enumerate() {
                work[ti].err = build_wide(t, &mut work[ti]);
            }
        } else {
            let slots: Vec<std::sync::Mutex<&mut RunWork>> = work[..tasks.len()].iter_mut().map(std::sync::Mutex::new).collect();
            crate::crew::run(slots.len(), &|k| {
                let mut w = slots[k].lock().expect("a run's work");
                w.err = build_wide(&tasks[k], &mut w);
            });
        }
        // The runs' vertices are copied into the frame's buffers after the pass over the plans (`copy_runs`, on the crew's threads):
        // the pass decides the draws from where each run's go (`n_out`, what the buffers will hold), and what it has put in them
        // when it stops on an error is copied first.
        let mut n_out = out.len();
        let mut copies: Vec<(usize, usize, usize)> = Vec::new();
        let placed_all = (|| -> Result<(), String> {
        for (r, plan) in plans.into_iter().enumerate() {
            while let Some((at, p)) = ee.get_mut(placed)
                && p.after <= r
            {
                *at = draws.len();
                barrier = draws.len();
                placed += 1;
            }
            let run = &runs[r];
            match plan {
                Plan::Skip(why) => skip(why),
                Plan::Fail(e) => return Err(e),
                Plan::Vu => {
                    let mut made = Vec::new();
                    let vu = match &mut vu {
                        Some(v) => v,
                        None => vu.insert(chain_vu()?),
                    };
                    if let Err(e) = run_vu(vu, &mut vu_gs, run, &constants, &images, &unpack_words, r, &mut made) {
                        skip(e);
                        continue;
                    }
                    if eedump() {
                        let mut t: Vec<_> = made.iter().map(|(_, p)| (p.tex0, p.prim & 0x10 != 0, p.texels.map(|u| u.src), p.clut.map(|u| u.src))).collect();
                        t.dedup();
                        eprintln!("vu run {r} (packet A {:#x}): {} prims, textures {t:x?}", run.vu.packet_a.expect("checked above"), made.len());
                    }
                    // Its primitives come where the GS gets them: after the EE's before this run, before the draws of the next.
                    let k = made.len();
                    for (at, _) in &mut made {
                        *at = draws.len();
                    }
                    barrier = draws.len();
                    ee.splice(placed..placed, made);
                    for t in transfers.iter_mut() {
                        if t.index > placed || t.index == placed && t.after > r {
                            t.index += k;
                        }
                    }
                    for p in passes.iter_mut() {
                        if p.start >= placed {
                            *p = p.start + k..p.end + k;
                        } else if p.end > placed {
                            p.end += k;
                        }
                    }
                    placed += k;
                    drawn += 1;
                }
                Plan::Draw(ti) => {
                    let (t, w) = (&tasks[ti], &mut work[ti]);
                    if let Some(e) = w.err.take() {
                        return Err(e);
                    }
                    let (dest, mut at) = (n_out, 0);
                    for (&(set, key), &n) in w.strips.iter().zip(&w.counts) {
                        if draws.len() == barrier || draws.last().is_none_or(|(st, dr, f)| *st != set || dr.key != key || *f != t.fb) {
                            draws.push((set, Draw { key, first: n_out as u32, count: 0 }, t.fb));
                        }
                        at += n;
                        n_out += n;
                        let dr = &mut draws.last_mut().expect("pushed above").1;
                        dr.count = n_out as u32 - dr.first;
                    }
                    copies.push((ti, dest, at));
                }
            }
        }
        Ok(())
        })();
        copy_runs(&work, &copies, out, prev, lit);
        placed_all?;
        (self.prepared, self.work) = (prepared, work);
        if lit_stats() && lighting {
            let e = |k: usize| lit_count.error[k] / lit_count.vertices.max(1) as f64;
            println!("native: lit mobies: {} runs lit; not: {} with no skin job output, {} whose output differs ({:?} positions, count), {} with a singular model transform; light terms {:.4} from the skin job's ({:.4} with the normals reversed, {} vertices)", lit_count.lit, lit_count.unskinned, lit_count.differ, lit_count.differ_vertices, lit_count.no_light, e(0), e(1), lit_count.vertices);
        }
        // The rest in the jobs' order: the EE's primitives (and the passes' output) after the last run, then after the effect pass.
        for (at, p) in &mut ee[placed..] {
            *at = if p.after == AFTER_EFFECTS { AFTER_EFFECTS } else { draws.len() };
        }
        if track {
            self.last = Some(LastFrame { field, runs: kept });
        }
        skipped.extend(stopped.into_iter().map(|e| (e, 1)));
        if let Some(t) = mesh_time {
            println!("native: moby CPU: skin {:.3} ms, geometry/light/materials {:.3} ms, {} corners, {} draws", skin_ms, t.elapsed().as_secs_f64() * 1e3, out.len(), draws.len());
        }
        free_later(runs);
        Ok((drawn, skipped, ee))
    }
}

/// A frame's moby runs freed on a thread of their own once the frame is built (`Mobies::frame`): their UNPACKs' words and their
/// positions are thousands of allocations a frame, and freeing them took the frame builder a tenth of its time (Florana with DRAW
/// DISTANCE FAR and the v0.1.4 player's settings: `drop_in_place<Vec<Run>>` 5.8 % and the UNPACK lists' 4.8 % of its samples).
fn free_later(runs: Vec<Run>) {
    static FREE: std::sync::OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<Vec<Run>>>> = std::sync::OnceLock::new();
    let tx = FREE.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<Vec<Run>>();
        std::thread::Builder::new()
            .name("uya free".into())
            .spawn(move || {
                for runs in rx {
                    drop(runs);
                }
            })
            .expect("the moby runs' freeing thread starts");
        std::sync::Mutex::new(tx)
    });
    tx.lock().expect("the moby runs' freeing channel").send(runs).expect("the moby runs' freeing thread runs");
}

/// The address a frame's GS data that has none of the EE's is given: a PATH3 transfer's and a VU1 program's packets (their bytes
/// `ScreenWalk::bytes`, from `FRAME_BYTES` on), which `eedraw::EeTextures` reads a texture uploaded from them out of.
pub(crate) const FRAME_BYTES: u32 = 0xC000_0000;

/// VU1 and VIF1 as a screen frame's chains leave them (`screen_walk`): the front end's chains upload a program once and run it
/// frame after frame, and keep VIF1's registers as the PS2 keeps them.
pub(crate) struct ScreenVu {
    vu: crate::vu1::Vu1,
    regs: crate::vif::VifRegs,
    /// Micro memory (16 KB) and the program it holds (None: not looked up since the last MPG).
    micro: Vec<u8>,
    program: Option<Option<Resident>>,
}

/// The program micro memory holds: one the port does itself (`vu1`), or another, as the port's translator translated it.
#[derive(Clone, Copy)]
enum Resident {
    Port(crate::vu1::Prog),
    Translated(&'static tr_vu1::rt::Prog),
}

impl ScreenVu {
    pub fn new() -> ScreenVu {
        ScreenVu { vu: crate::vu1::Vu1::new(), regs: crate::vif::VifRegs { cl: 1, wl: 1, ..Default::default() }, micro: vec![0; 0x4000], program: None }
    }
}

/// A screen frame's GS packets read in kick order (`screen_walk`): the primitives, the transfers into and out of the main frame,
/// the GS contexts the jobs leave, the bytes of the packets that have no EE address (`FRAME_BYTES`), and what stopped a job.
pub(crate) struct ScreenWalk {
    pub ee: Vec<(usize, EePrim)>,
    pub transfers: Vec<Transfer>,
    pub gs_end: GsLeft,
    pub bytes: Vec<u8>,
    pub stopped: Vec<String>,
}

/// The jobs of a frame with no level's world (the front end's) read as the GS gets them: each GIF job (PATH3), each VIF1 job's
/// DIRECT packets (their data at the EE's addresses, from the job's blocks) and the packets its VU1 programs kick, every program
/// run on VU1 as the PS2 runs it (`vu`: UNPACK into its data memory, MPG into its micro memory, MSCAL a run of the program the
/// port's translator translated from that microcode, under the game's MXCSR), all of it drawn (no layer of the level's draws any
/// of it). A program's packets and a PATH3 transfer's data are given addresses from `FRAME_BYTES` on, their bytes kept: an upload
/// of one of the HUD's glyph sets' parts (`prompts::Glyphs`, by its bytes) is given the main program's address of that part, as
/// the levels' chains upload them, so its letters and glyphs are the port's (`prompts::swap`). Z is put in the Z24 buffer's range
/// whatever the primitive's ZBUF format (`effects::Projection::at_rest`). A job the walk cannot read stops there, named.
pub(crate) fn screen_walk(jobs: &crate::jobs::FrameJobs, start: Option<GsLeft>, s: &mut ScreenVu, glyphs: &crate::prompts::Glyphs) -> ScreenWalk {
    let mut gs = EeGs::start();
    if let Some(c) = start {
        (gs.ctx, gs.texa, gs.fogcol) = (c.ctx, c.texa, c.fogcol);
    }
    let mut ee: Vec<(usize, EePrim)> = Vec::new();
    let mut bytes: Vec<u8> = Vec::new();
    let mut stopped = Vec::new();
    // UYA_STARS_LOG: the frame's MSCALs, by the program that ran (or none known), the entries of program 0's and the primitives its
    // packets made.
    let (mut runs_by, mut p0_entries, mut p0_prims, mut unknown) = (std::collections::BTreeMap::<&'static str, u32>::new(), Vec::new(), 0usize, 0u32);
    // Packets with no EE address: kept, and read through the GS at addresses from FRAME_BYTES on.
    let unaddressed = |gs: &mut EeGs, ee: &mut Vec<(usize, EePrim)>, bytes: &mut Vec<u8>, qs: &[[u32; 4]]| -> Result<(), String> {
        let base = bytes.len();
        for q in qs {
            for w in q {
                bytes.extend_from_slice(&w.to_le_bytes());
            }
        }
        gs.direct(qs.len(), |q| Ok(qs[q]), |q| FRAME_BYTES + (base + 16 * q) as u32, 0, ee, |_, _| {})
    };
    for (jn, job) in jobs.jobs.iter().enumerate() {
        let (all, marks, blocks) = match job {
            crate::jobs::Job::Gif { qs, .. } => {
                let qs: Vec<[u32; 4]> = qs.iter().map(|&d| [d as u32, (d >> 32) as u32, (d >> 64) as u32, (d >> 96) as u32]).collect();
                if let Err(e) = unaddressed(&mut gs, &mut ee, &mut bytes, &qs) {
                    stopped.push(format!("GIF job {jn}: {e}"));
                }
                continue;
            }
            crate::jobs::Job::Vif1 { words, marks, blocks, .. } => (words, marks, blocks),
        };
        let mut tte: Vec<usize> = marks.iter().filter(|m| m.1 == 2).map(|m| m.0).collect();
        tte.sort_unstable();
        let tagged = |idx: usize| idx >= 2 && tte.binary_search(&(idx - 2)).is_ok();
        let walked = (|| -> Result<(), String> {
            for &(idx, src, qwc, _) in blocks.iter() {
                let qwc = qwc as usize;
                let data = all.get(idx..idx + 4 * qwc).ok_or_else(|| format!("native: VIF1 job {jn}'s block from {src:#x} past its words"))?;
                // The tag's VIF words and the data (a block sent without them: two NOPs), word i at EE address src + 4 (i - 2).
                let words: std::borrow::Cow<[u32]> = if tagged(idx) { std::borrow::Cow::Borrowed(&all[idx - 2..idx + 4 * qwc]) } else { std::borrow::Cow::Owned([[0, 0].as_slice(), data].concat()) };
                let mut i = 0;
                while i < words.len() {
                    let code = words[i];
                    let (cmd, imm, num) = ((code >> 24) & 0x7f, code & 0xffff, (code >> 16) & 0xff);
                    i += 1;
                    let r = &mut s.regs;
                    match cmd {
                        0x00 | 0x04 | 0x06 | 0x07 | 0x10 | 0x11 | 0x13 => {}
                        0x01 => (r.cl, r.wl) = (imm & 0xff, imm >> 8 & 0xff),
                        0x02 => (r.offset, r.dbf, r.tops) = (imm & 0x3ff, false, r.base),
                        0x03 => r.base = imm & 0x3ff,
                        0x05 => r.mode = imm & 3,
                        0x20 => {
                            r.mask = *words.get(i).ok_or("native: STMASK past its block")?;
                            i += 1;
                        }
                        0x30 | 0x31 => {
                            let w = words.get(i..i + 4).ok_or("native: STROW or STCOL past its block")?;
                            if cmd == 0x30 { r.row.copy_from_slice(w) } else { r.col.copy_from_slice(w) }
                            i += 4;
                        }
                        0x14 | 0x15 | 0x17 => {
                            let top = r.start();
                            if cmd == 0x17 {
                                return Err("native: VU1 MSCNT (a run continued where the last ended)".into());
                            }
                            let micro = &s.micro;
                            let prog = *s.program.get_or_insert_with(|| {
                                crate::vu1::Prog::of_micro(micro).map(Resident::Port).or_else(|| tr_vu1::PROGRAMS.iter().find(|p| micro.get(..p.code.len()) == Some(p.code)).map(Resident::Translated))
                            });
                            unknown += prog.is_none() as u32;
                            let prog = prog.ok_or("native: VU1 microcode the port has no program for")?;
                            let entry = imm as usize * 8;
                            s.vu.top = top as u32;
                            s.vu.kicks.clear();
                            s.vu.sent.clear();
                            // Program 6 kicks a packet and then writes it (`effects.rs` reads it after the run): its packets are
                            // read when the next is kicked or the run ends; the other programs' at their kicks (each kicks a buffer
                            // and goes on to fill it again).
                            let name = match prog {
                                Resident::Port(p) => p.name(),
                                Resident::Translated(t) => t.name,
                            };
                            s.vu.defer_packets = name == "p6";
                            let ran = match prog {
                                Resident::Port(p) => crate::vu1::run(&mut s.vu, p, entry).map(|_| ()),
                                Resident::Translated(t) => crate::vu1::run_translated(&mut s.vu, t, entry),
                            };
                            s.vu.finish();
                            ran.map_err(|e| format!("VU1 program {name} from {entry:#x}, TOP {top:#x}: {e}"))?;
                            *runs_by.entry(name).or_insert(0) += 1;
                            let e0 = ee.len();
                            for sent in std::mem::take(&mut s.vu.sent) {
                                let sent = sent.map_err(|e| format!("VU1 program {name} from {entry:#x}, TOP {top:#x}: {e}"))?;
                                unaddressed(&mut gs, &mut ee, &mut bytes, &sent)?;
                            }
                            if name == "p0" {
                                p0_prims += ee.len() - e0;
                                if !p0_entries.contains(&entry) {
                                    p0_entries.push(entry);
                                }
                            }
                        }
                        0x50 | 0x51 => {
                            let n = if imm == 0 { 0x10000 } else { imm as usize };
                            i = 2 + i.saturating_sub(2).div_ceil(4) * 4;
                            let base = i;
                            gs.direct(n, |q| words.get(base + 4 * q..base + 4 * q + 4).map(|w| [w[0], w[1], w[2], w[3]]).ok_or_else(|| format!("native: DIRECT past its block at {src:#x}")), |q| src + 4 * (base as u32 - 2) + 16 * q as u32, 0, &mut ee, |_, _| {})?;
                            i += 4 * n;
                        }
                        // MPG: micro memory is 16 KB; an upload past its end wraps.
                        0x4a => {
                            let n = 2 * if num == 0 { 256 } else { num as usize };
                            let w = words.get(i..i + n).ok_or("native: MPG past its block")?;
                            for (k, x) in w.iter().enumerate() {
                                let at = (imm as usize * 8 + 4 * k) & 0x3fff;
                                s.micro[at..at + 4].copy_from_slice(&x.to_le_bytes());
                            }
                            s.program = None;
                            i += n;
                        }
                        0x60..=0x7f => {
                            let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
                            let n = if num == 0 { 256 } else { num };
                            // Its size: CL and WL of STCYCL (a write cycle longer than the read one skips no data).
                            let (cl, wl) = (r.cl, r.wl);
                            let items = if wl <= cl || wl == 0 { n } else { (n / wl) * cl + (n % wl).min(cl) };
                            let bits = if vl == 3 { 16 } else { (vn + 1) * (32 >> vl) };
                            let len = (items * bits).div_ceil(32) as usize;
                            let w = words.get(i..i + len).ok_or("native: an UNPACK past its block")?;
                            crate::vif::unpack(&mut s.vu.mem[..], cmd, num, imm, w, r).map_err(|e| format!("native: {e}"))?;
                            i += len;
                        }
                        c => return Err(format!("native: VIF code {c:#x} at {src:#x}")),
                    }
                }
            }
            Ok(())
        })();
        if let Err(e) = walked {
            stopped.push(format!("VIF1 job {jn}: {e}"));
        }
    }
    if stars_log() && (!runs_by.is_empty() || unknown > 0 || !stopped.is_empty()) {
        println!("native: field {}: the front end's walk: VU1 runs {runs_by:?} (program 0's entries {p0_entries:x?}, {p0_prims} primitives from its packets), {unknown} MSCALs with no program known resident, {} jobs stopped{}", jobs.field, stopped.len(), stopped.first().map_or(String::new(), |e| format!(" (the first: {e})")));
    }
    let mut transfers = Vec::new();
    for t in &gs.transfers {
        match t.main_pages() {
            Ok(p) if p.is_empty() => {}
            Ok(_) => transfers.push(*t),
            Err(e) => stopped.push(e),
        }
    }
    // The glyph sets' parts uploaded from the frame's own bytes, by their bytes (each upload once).
    let mut known: HashMap<u32, Option<crate::prompts::Held>> = HashMap::new();
    let mut held = |u: Upload| {
        *known.entry(u.src).or_insert_with(|| {
            let n = match u.psm {
                0x14 => (u.w * u.h / 2) as usize,
                0x13 => (u.w * u.h) as usize,
                0 => (4 * u.w * u.h) as usize,
                _ => return None,
            };
            let at = (u.src - FRAME_BYTES) as usize;
            bytes.get(at..at + n).and_then(|b| glyphs.held(u.psm as u64, u.w as u64, u.h as u64, b))
        })
    };
    for (_, p) in &mut ee {
        if let Some(t) = p.texels.filter(|t| t.src >= FRAME_BYTES) {
            match held(t) {
                Some(h @ (crate::prompts::Held::Font(set) | crate::prompts::Held::Sheet(set))) => {
                    p.texels = Some(Upload { src: crate::prompts::home(h, set), ..t });
                    if let Some(c) = p.clut.filter(|c| c.src >= FRAME_BYTES)
                        && let Some(h @ crate::prompts::Held::Palette(_)) = held(c)
                    {
                        p.clut = Some(Upload { src: crate::prompts::home(h, set), ..c });
                    }
                }
                _ => {}
            }
        }
        // ZBUF's PSM: PSMZ32, PSMZ24, PSMZ16 and PSMZ16S.
        if p.test >> 16 & 1 == 1 {
            let scale = match p.zbuf >> 24 & 15 {
                0 => 1.0 / 256.0,
                2 | 0xa => 256.0,
                _ => 1.0,
            };
            for v in p.verts.iter_mut() {
                v.z *= scale;
            }
        }
    }
    ScreenWalk { ee, transfers, gs_end: GsLeft { ctx: gs.ctx, texa: gs.texa, fogcol: gs.fogcol }, bytes, stopped }
}

/// A translated VU1 program's run (`run`), its stop an error: the translator's guard stops a run that never ends (5,000,000
/// instruction pairs: the program fed VU1 memory it loops on), which ended the renderer's thread (the player's Aquatos, 20
/// November 2026, "VU1 p0: 5,000,000 pairs without ending"); the run is not drawn and its reason is named instead.
pub(crate) fn run_contained(run: impl FnOnce()) -> Result<(), String> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(run)).map_err(|p| p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_else(|| "the run stopped".into()))
}

/// VU1 program 0 (the moby program, the port's own code of it: `vu1p0`) started at MSCAL immediate `imm` with TOPS `top` on
/// `vu`'s memory: the GIF packets its XGKICKs sent, quadword by quadword.
fn run_program_0(vu: &mut crate::vu1::Vu1, top: usize, imm: u32) -> Result<Vec<[u32; 4]>, String> {
    let entry = imm as usize * 8;
    vu.top = top as u32;
    vu.kicks.clear();
    vu.sent.clear();
    let before = RUN_CHECK.get().map(|_| (vu.mem.to_vec(), vu.vf, vu.vi));
    crate::vu1::run(vu, crate::vu1::Prog::P0, entry).map_err(|e| format!("native: VU1 program 0 from {entry:#x}, TOP {top:#x}: {e}"))?;
    let mut qs = Vec::new();
    for sent in vu.sent.drain(..) {
        qs.extend(sent?);
    }
    if let (Some(check), Some((mem, vf, vi))) = (RUN_CHECK.get(), before) {
        check(&mem, &vf, &vi, entry, top, &qs, &tr_vu1::p0::CODE);
    }
    Ok(qs)
}

/// A moby run's materials of set `set` from its source's (set 0): TEST_1 AREF (0 to 255: GEQUAL AREF, a failing pixel writing its
/// colour), or ATST NEVER (256) with AFAIL FB_ONLY (+ 1024: every pixel writes its colour and alpha, not its depth), as the EE's
/// triangles' sets are numbered (`eedraw`).
fn moby_set(m: &Materials, set: u16) -> Materials {
    match set {
        s if s < 256 => m.with_aref(s),
        1280 => m.with_aref(0).never_passing().fb_only(),
        s => unreachable!("moby material set {s} is none `Mobies::frame` makes"),
    }
}

/// VU1 as the moby chain's first run leaves it for the others: MSCAL 0x0, program 0's entry 0 (micro 0x218: vf27 = 712, 840, 712,
/// 840, the two buffers of VU1 memory each run writes its GS packets into and kicks, in turn). Without it a run wrote its packets
/// over its own vertices at 0 (vf27 0): the fall down level 27's pipe, the tube's runs, drew triangles from vertices its own output
/// had overwritten (shards across the picture, Z at the Z24 buffer's largest, which the emulated PS2's GS stream has none of) and
/// some ran on without end ("VU1 p0: 5,000,000 pairs without ending").
fn chain_vu() -> Result<crate::vu1::Vu1, String> {
    let mut vu = crate::vu1::Vu1::new();
    let kicked = run_program_0(&mut vu, 0, 0)?;
    if !kicked.is_empty() {
        return Err(format!("native: program 0's entry 0 kicked {} quadwords", kicked.len()));
    }
    Ok(vu)
}

/// A moby run whose class the port has no mesh for (one the game loads at run time: Clank's in the cutscene of the flight to
/// Aquatos), run as the PS2 runs it: program 0 on the VU1 memory the chain's UNPACKs built for it (the program's constants, and
/// this buffer's matrix, positions, colours, texture coordinates, strip list and texture setups, whose GS registers the game wrote
/// into the packet), its GIF packets read through the GS state the run starts with (the chain's IMAGE transfers before it:
/// `EeGs`), the primitives onto `out` after `r` moby runs, to be drawn as the EE's own are, in the world.
fn run_vu(vu: &mut crate::vu1::Vu1, gs: &mut Option<EeGs>, run: &Run, constants: &[[f32; 4]; 6], images: &[(u32, Upload)], unpack_words: &[u32], r: usize, out: &mut Vec<(usize, EePrim)>) -> Result<(), String> {
    vu.mem.fill([0; 4]);
    for (n, c) in constants.iter().enumerate() {
        vu.mem[0x123 + n] = c.map(f32::to_bits);
    }
    for u in run.vu.unpacks.iter() {
        let mut regs = crate::vif::VifRegs { tops: run.top as u32, ..u.regs };
        crate::vif::unpack(&mut vu.mem[..], u.cmd, u.num, u.imm, &unpack_words[u.words.0 as usize..(u.words.0 + u.words.1) as usize], &mut regs).map_err(|e| format!("native: {e}"))?;
    }
    let qs = run_program_0(vu, run.top, run.mscal)?;
    // The GS keeps its texture registers from run to run: a strip without a setup draws with the texture the run before bound,
    // unless the chain wrote one since.
    let g = gs.get_or_insert_with(|| {
        let mut g = EeGs::start();
        g.ctx[0] = run.ctx;
        g
    });
    let (tex0, clamp, tex1) = (g.ctx[0].tex0, g.ctx[0].clamp, g.ctx[0].tex1);
    g.ctx[0] = run.ctx;
    if !run.textured && g.st_writes != 0 {
        (g.ctx[0].tex0, g.ctx[0].clamp, g.ctx[0].tex1) = (tex0, clamp, tex1);
    }
    for &(dbp, up) in &images[..run.images] {
        g.upload(dbp, up);
    }
    g.gif_out(&qs, r, out, &mut std::iter::empty())
}

/// The size in words of UNPACK `cmd` of `num` elements.
fn unpack_words(cmd: u32, num: usize) -> usize {
    let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
    let n = if num == 0 { 256 } else { num };
    let size = match vl {
        0 => n * (vn as usize + 1) * 4,
        1 => n * (vn as usize + 1) * 2,
        2 => n * (vn as usize + 1),
        _ => n * 2,
    };
    size.div_ceil(4)
}

/// Whether a block's VIF stream (the tag's two VIF words, then its data) has an UNPACK to the strip list's address 0x12D.
fn sends_strip_list(words: &[u32]) -> bool {
    let mut i = 0;
    while i < words.len() {
        let code = words[i];
        let (cmd, imm, num) = ((code >> 24) & 0x7f, code & 0xffff, ((code >> 16) & 0xff) as usize);
        i += 1;
        match cmd {
            0x20 => i += 1,
            0x30 | 0x31 => i += 4,
            0x50 | 0x51 => i = 2 + i.saturating_sub(2).div_ceil(4) * 4 + 4 * if imm == 0 { 0x10000 } else { imm as usize },
            0x4a => return false,
            0x60..=0x7f => {
                if (cmd & 0x6f, imm & 0x3ff) == (0x6e, 0x12d) {
                    return true;
                }
                i += unpack_words(cmd, num);
            }
            _ => {}
        }
    }
    false
}

/// The inverse of a 4 x 4 (Gauss-Jordan with partial pivoting), None when it is singular.
pub(crate) fn invert(m: [[f64; 4]; 4]) -> Option<[[f64; 4]; 4]> {
    let mut a = m;
    let mut inv = [[0f64; 4]; 4];
    for (i, row) in inv.iter_mut().enumerate() {
        row[i] = 1.0;
    }
    for col in 0..4 {
        let p = (col..4).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))?;
        if a[p][col].abs() < 1e-30 {
            return None;
        }
        a.swap(col, p);
        inv.swap(col, p);
        let d = a[col][col];
        for k in 0..4 {
            a[col][k] /= d;
            inv[col][k] /= d;
        }
        for r in 0..4 {
            if r != col {
                let f = a[r][col];
                for k in 0..4 {
                    a[r][k] -= f * a[col][k];
                    inv[r][k] -= f * inv[col][k];
                }
            }
        }
    }
    Some(inv)
}
