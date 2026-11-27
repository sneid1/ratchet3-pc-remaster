//! PCSX2 texture names of the native renderer's textures, so that a PCSX2 texture pack (a folder of `<name>.png` / `.dds` files,
//! the fan packs in the profile's `texture_replacements`) replaces them at their own resolution.
//!
//! PCSX2 names a texture by what GS local memory holds when a draw samples it: the XXH3-64 of the texture's level 0 (for a PSMT8
//! texture of at least one 16 x 16 block, the raw 256 bytes of each block, block rows top to bottom, each block's bytes in the GS's
//! own order within it; for a smaller PSMT8 texture and for PSMT8H, its rows of indices), the XXH3-64 of its CLUT's 256 colours in
//! index order, and its PSM and size (`bits`: PSM, log2 width << 6, log2 height << 10). The port's textures carry their level 0 in
//! GS terms (`core::GsLevel`), so the name is computed here from the indices and the palette (`name`, `xxh3_64`, `t8_block_byte`).
//! A block's 256 bytes are the texels of that block whatever the buffer width, so the name does not depend on where the game put
//! the texture. A pack's files are found here (`open`: `<tex0>-<clut>-<bits>.png` in the folder or any folder in it, the first in
//! sorted order) and decoded (`pic::read_png`), and `uya texnames --write` dumps the textures here (`dump`).
//!
//! Only the name of a draw without a clamp region is made here (CLAMP REGION_CLAMP / REGION_REPEAT smaller than the texture name
//! another rectangle); `region_free` tells such draws apart, and they draw the original.
//!
//! A replacement is drawn at its own resolution with the level its own texel density picks on screen (the GS's LOD from Q and K
//! picks levels for the original's size and the PS2's picture), trilinear and anisotropic.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// A texture's PCSX2 name with no clamp region: the XXH3-64 of its level 0 and of its palette, and `bits` (PSM, log2 width << 6,
/// log2 height << 10).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct TexName {
    tex0: u64,
    clut: u64,
    bits: u32,
}

impl TexName {
    /// The file stem PCSX2 dumps it under and a pack names its file by: `<tex0>-<clut>-<bits>` (hex, the bits eight digits).
    pub(crate) fn stem(&self) -> String {
        format!("{:x}-{:x}-{:08x}", self.tex0, self.clut, self.bits)
    }

    /// The name of a pack file `<tex0>-<clut>-<bits>.png` (PCSX2's scanf of `%llx-%llx-%08x.`: up to 16, 16 and 8 hex digits,
    /// bit 14 of the bits, the old TCC flag, cleared as PCSX2 clears it); None for any other file.
    fn of_file(file: &str) -> Option<TexName> {
        let (stem, ext) = file.rsplit_once('.')?;
        if !ext.eq_ignore_ascii_case("png") {
            return None;
        }
        let mut parts = stem.split('-');
        let mut hex = |max: usize| parts.next().filter(|p| !p.is_empty() && p.len() <= max && p.bytes().all(|c| c.is_ascii_hexdigit())).map(|p| u64::from_str_radix(p, 16).expect("hex digits"));
        let (tex0, clut, bits) = (hex(16)?, hex(16)?, hex(8)?);
        parts.next().is_none().then_some(TexName { tex0, clut, bits: bits as u32 & !(1 << 14) })
    }
}

/// The PCSX2 name of `t` sampled with no clamp region; None for a texture with no GS form (`core::Texture::gs`).
pub(crate) fn name(t: &uyafmt::core::Texture) -> Result<Option<TexName>, String> {
    let Some(g) = &t.gs else { return Ok(None) };
    let (w, h) = (t.width, t.height);
    if !w.is_power_of_two() || !h.is_power_of_two() || w > 1024 || h > 1024 || g.texels.len() != w * h || g.clut.len() != 256 {
        return Err(format!("texnames: a {w} x {h} texture of {} texels and {} palette colours", g.texels.len(), g.clut.len()));
    }
    let (tex0, bits) = indexed_key(g.psm, w, h, &g.texels)?;
    let clut: Vec<u8> = g.clut.iter().flatten().copied().collect();
    Ok(Some(TexName { tex0, clut: xxh3_64(&clut), bits }))
}

/// The image half of a name, shared by the draw path and load-time HUD/GS RAM discovery. The palette a sprite uses is
/// selected by the game's draw, so hashing images separately avoids constructing every image/palette combination.
fn indexed_key(psm: u32, w: usize, h: usize, indices: &[u8]) -> Result<(u64, u32), String> {
    if !w.is_power_of_two() || !h.is_power_of_two() || w > 1024 || h > 1024 || indices.len() != w * h {
        return Err(format!("texnames: a {w} x {h} indexed texture of {} texels", indices.len()));
    }
    let texels: Vec<u8> = match psm {
        // PSMT8 of at least one block: each 16 x 16 block's 256 bytes as GS memory holds them, blocks in rows.
        0x13 if w >= 16 && h >= 16 => {
            let mut b = vec![0u8; w * h];
            for by in 0..h / 16 {
                for bx in 0..w / 16 {
                    let block = &mut b[(by * (w / 16) + bx) * 256..][..256];
                    for y in 0..16 {
                        for x in 0..16 {
                            block[t8_block_byte(x, y)] = indices[(by * 16 + y) * w + bx * 16 + x];
                        }
                    }
                }
            }
            b
        }
        // A smaller PSMT8 texture, and PSMT8H (the index in a 32-bit texel's top byte): the rows of indices.
        0x13 | 0x1B => indices.to_vec(),
        p => return Err(format!("texnames: PSM {p:#x}")),
    };
    let bits = psm | w.trailing_zeros() << 6 | h.trailing_zeros() << 10;
    Ok((xxh3_64(&texels), bits))
}

/// Images and palettes present in a level whose pairing is chosen only when the EE draws. Only installed replacements
/// matching both actual image bytes and an actual palette are preloaded, not all files in the pack or fabricated pairings.
#[derive(Default)]
pub(crate) struct PalettedNames {
    images: HashSet<(u64, u32)>,
    palettes: HashSet<u64>,
}

impl PalettedNames {
    pub(crate) fn image(&mut self, psm: u32, w: usize, h: usize, indices: &[u8]) -> Result<(), String> {
        self.images.insert(indexed_key(psm, w, h, indices)?);
        Ok(())
    }

    /// A 256-colour palette in index order, exactly as `name` receives it from `core::GsLevel`.
    pub(crate) fn palette(&mut self, clut: &[[u8; 4]]) {
        self.palettes.insert(xxh3_64(&clut.iter().flatten().copied().collect::<Vec<_>>()));
    }

    pub(crate) fn preload(&self, device: &wgpu::Device, queue: &wgpu::Queue, label: &str) -> Result<(), String> {
        note_ui(self.images.iter().copied())?;
        let names: Vec<TexName> = {
            let g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
            let Some(hd) = g.as_ref() else { return Ok(()) };
            hd.files.keys().chain(hd.rift_files.keys()).chain(hd.rift.iter().flat_map(|r| r.keys()))
                .filter(|n| self.images.contains(&(n.tex0, n.bits)) && self.palettes.contains(&n.clut)).copied().collect()
        };
        preload_names(device, queue, &names, false, label)
    }
}

/// `t` as PCSX2 dumps it (its RGBA with the palette applied, alpha on the GS scale, 0x80 opaque), to `<dir>/<stem>.png` unless the
/// folder has that file already. Returns whether it was written.
fn dump(dir: &Path, t: &uyafmt::core::Texture, stem: &str) -> Result<bool, String> {
    let path = dir.join(format!("{stem}.png"));
    if path.exists() {
        return Ok(false);
    }
    let g = t.gs.as_ref().ok_or("texnames: a texture with no GS form")?;
    let rgba: Vec<u8> = g.texels.iter().flat_map(|&i| g.clut[i as usize]).collect();
    std::fs::write(&path, crate::pic::png_rgba(t.width, t.height, &rgba)).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(true)
}

/// The byte a PSMT8 texel (`x`, `y` within its 16 x 16 block) takes in the block's 256 bytes: the block is four columns of 16 x 4
/// texels, each column 16 32-bit words laid out as a PSMCT32 block's rows (word (u, v) of an 8 x 8 PSMCT32 block is
/// (v / 2) x 16 + (u / 2) x 4 + (v % 2) x 2 + u % 2); a texel's word is read at row (y / 4) x 2 + y % 2 with its x moved 4 on
/// for rows 2 to 5 of every 8 (the GS's "swizzle"), and its byte in the word is (x / 8) x 2 + (y / 2) % 2.
fn t8_block_byte(x: usize, y: usize) -> usize {
    let swap = ((y + 2) >> 2 & 1) * 4;
    let v = (((y & !3) >> 1) + (y & 1)) & 7;
    let u = (x + swap) & 7;
    let word = (v >> 1) * 16 + (u >> 1) * 4 + (v & 1) * 2 + (u & 1);
    word * 4 + (x >> 3 & 1) * 2 + (y >> 1 & 1)
}

/// XXH3-64 with seed 0 and the default secret (the hash PCSX2 names textures by), for the input sizes a texture or a palette has.
fn xxh3_64(input: &[u8]) -> u64 {
    const SECRET: [u8; 192] = [
        0xb8, 0xfe, 0x6c, 0x39, 0x23, 0xa4, 0x4b, 0xbe, 0x7c, 0x01, 0x81, 0x2c, 0xf7, 0x21, 0xad, 0x1c, 0xde, 0xd4, 0x6d, 0xe9, 0x83, 0x90, 0x97, 0xdb, 0x72, 0x40, 0xa4, 0xa4, 0xb7, 0xb3, 0x67, 0x1f, 0xcb, 0x79, 0xe6, 0x4e, 0xcc, 0xc0, 0xe5, 0x78, 0x82, 0x5a, 0xd0, 0x7d, 0xcc, 0xff, 0x72, 0x21, 0xb8, 0x08, 0x46, 0x74, 0xf7, 0x43, 0x24, 0x8e, 0xe0, 0x35, 0x90, 0xe6, 0x81, 0x3a, 0x26, 0x4c, 0x3c, 0x28, 0x52, 0xbb, 0x91, 0xc3, 0x00, 0xcb, 0x88, 0xd0, 0x65, 0x8b, 0x1b, 0x53, 0x2e, 0xa3, 0x71, 0x64, 0x48, 0x97, 0xa2, 0x0d, 0xf9, 0x4e, 0x38, 0x19, 0xef, 0x46, 0xa9, 0xde, 0xac, 0xd8, 0xa8, 0xfa, 0x76, 0x3f, 0xe3, 0x9c, 0x34, 0x3f, 0xf9, 0xdc, 0xbb, 0xc7, 0xc7, 0x0b, 0x4f, 0x1d, 0x8a, 0x51, 0xe0, 0x4b, 0xcd, 0xb4, 0x59, 0x31, 0xc8, 0x9f, 0x7e, 0xc9, 0xd9, 0x78, 0x73, 0x64, 0xea, 0xc5, 0xac, 0x83, 0x34, 0xd3, 0xeb, 0xc3, 0xc5, 0x81, 0xa0, 0xff, 0xfa, 0x13, 0x63, 0xeb, 0x17, 0x0d, 0xdd, 0x51, 0xb7, 0xf0, 0xda, 0x49, 0xd3, 0x16, 0x55, 0x26, 0x29, 0xd4, 0x68, 0x9e, 0x2b, 0x16, 0xbe, 0x58, 0x7d, 0x47, 0xa1, 0xfc, 0x8f, 0xf8, 0xb8, 0xd1, 0x7a, 0xd0, 0x31, 0xce, 0x45, 0xcb, 0x3a, 0x8f, 0x95, 0x16, 0x04, 0x28, 0xaf, 0xd7, 0xfb, 0xca, 0xbb, 0x4b, 0x40, 0x7e,
    ];
    const P32_1: u64 = 0x9E37_79B1;
    const P32_2: u64 = 0x85EB_CA77;
    const P32_3: u64 = 0xC2B2_AE3D;
    const P64_1: u64 = 0x9E37_79B1_85EB_CA87;
    const P64_2: u64 = 0xC2B2_AE3D_27D4_EB4F;
    const P64_3: u64 = 0x1656_67B1_9E37_79F9;
    const P64_4: u64 = 0x85EB_CA77_C2B2_AE63;
    const P64_5: u64 = 0x27D4_EB2F_1656_67C5;
    let r64 = |b: &[u8], o: usize| u64::from_le_bytes(b[o..o + 8].try_into().expect("8 bytes"));
    let r32 = |b: &[u8], o: usize| u32::from_le_bytes(b[o..o + 4].try_into().expect("4 bytes")) as u64;
    let fold = |a: u64, b: u64| {
        let m = a as u128 * b as u128;
        m as u64 ^ (m >> 64) as u64
    };
    let avalanche = |mut h: u64| {
        h ^= h >> 37;
        h = h.wrapping_mul(0x1656_6791_9E37_79F9);
        h ^ h >> 32
    };
    let mix16 = |i: usize, s: usize| fold(r64(input, i) ^ r64(&SECRET, s), r64(input, i + 8) ^ r64(&SECRET, s + 8));
    let len = input.len();
    if len == 0 {
        let mut h = r64(&SECRET, 56) ^ r64(&SECRET, 64);
        h ^= h >> 33;
        h = h.wrapping_mul(P64_2);
        h ^= h >> 29;
        h = h.wrapping_mul(P64_3);
        return h ^ h >> 32;
    }
    if len <= 3 {
        let combo = (input[0] as u64) << 16 | (input[len >> 1] as u64) << 24 | input[len - 1] as u64 | (len as u64) << 8;
        let mut h = combo ^ (r32(&SECRET, 0) ^ r32(&SECRET, 4));
        h ^= h >> 33;
        h = h.wrapping_mul(P64_2);
        h ^= h >> 29;
        h = h.wrapping_mul(P64_3);
        return h ^ h >> 32;
    }
    if len <= 8 {
        let flip = r64(&SECRET, 8) ^ r64(&SECRET, 16);
        let mut h = (r32(input, len - 4) + (r32(input, 0) << 32)) ^ flip;
        h ^= h.rotate_left(49) ^ h.rotate_left(24);
        h = h.wrapping_mul(0x9FB2_1C65_1E98_DF25);
        h ^= (h >> 35).wrapping_add(len as u64);
        h = h.wrapping_mul(0x9FB2_1C65_1E98_DF25);
        return h ^ h >> 28;
    }
    if len <= 16 {
        let lo = r64(input, 0) ^ (r64(&SECRET, 24) ^ r64(&SECRET, 32));
        let hi = r64(input, len - 8) ^ (r64(&SECRET, 40) ^ r64(&SECRET, 48));
        return avalanche((len as u64).wrapping_add(lo.swap_bytes()).wrapping_add(hi).wrapping_add(fold(lo, hi)));
    }
    if len <= 128 {
        let mut acc = (len as u64).wrapping_mul(P64_1);
        if len > 32 {
            if len > 64 {
                if len > 96 {
                    acc = acc.wrapping_add(mix16(48, 96)).wrapping_add(mix16(len - 64, 112));
                }
                acc = acc.wrapping_add(mix16(32, 64)).wrapping_add(mix16(len - 48, 80));
            }
            acc = acc.wrapping_add(mix16(16, 32)).wrapping_add(mix16(len - 32, 48));
        }
        acc = acc.wrapping_add(mix16(0, 0)).wrapping_add(mix16(len - 16, 16));
        return avalanche(acc);
    }
    if len <= 240 {
        let mut acc = (len as u64).wrapping_mul(P64_1);
        for i in 0..8 {
            acc = acc.wrapping_add(mix16(16 * i, 16 * i));
        }
        acc = avalanche(acc);
        for i in 8..len / 16 {
            acc = acc.wrapping_add(mix16(16 * i, 16 * (i - 8) + 3));
        }
        return avalanche(acc.wrapping_add(mix16(len - 16, 136 - 17)));
    }
    // The long input: 8 accumulators over 64-byte stripes, the secret stepping 8 bytes a stripe, scrambled after each block.
    let mut acc = [P32_3, P64_1, P64_2, P64_3, P64_4, P32_2, P64_5, P32_1];
    let accumulate = |acc: &mut [u64; 8], at: usize, s: usize| {
        for i in 0..8 {
            let v = r64(input, at + 8 * i);
            let k = v ^ r64(&SECRET, s + 8 * i);
            acc[i ^ 1] = acc[i ^ 1].wrapping_add(v);
            acc[i] = acc[i].wrapping_add((k & 0xFFFF_FFFF).wrapping_mul(k >> 32));
        }
    };
    let stripes = (192 - 64) / 8;
    let block = 64 * stripes;
    let blocks = (len - 1) / block;
    for b in 0..blocks {
        for n in 0..stripes {
            accumulate(&mut acc, b * block + n * 64, n * 8);
        }
        for (i, a) in acc.iter_mut().enumerate() {
            *a = ((*a ^ *a >> 47) ^ r64(&SECRET, 192 - 64 + 8 * i)).wrapping_mul(P32_1);
        }
    }
    for n in 0..((len - 1) - block * blocks) / 64 {
        accumulate(&mut acc, blocks * block + n * 64, n * 8);
    }
    accumulate(&mut acc, len - 64, 192 - 64 - 7);
    let mut h = (len as u64).wrapping_mul(P64_1);
    for i in 0..4 {
        h = h.wrapping_add(fold(acc[2 * i] ^ r64(&SECRET, 11 + 16 * i), acc[2 * i + 1] ^ r64(&SECRET, 11 + 16 * i + 8)));
    }
    avalanche(h)
}

/// The pack folder the native renderer draws replacements from, process-wide like a game setting (`open`): every texture the
/// renderer uploads (`objects::Materials`, the terrain, the sky) looks its replacement up here.
static HD: std::sync::Mutex<Option<Hd>> = std::sync::Mutex::new(None);

struct Hd {
    /// Every file the folder has for a name the port makes (the first in sorted order, folders included, as PCSX2 takes it),
    /// but those in `RIFT_APART`.
    files: HashMap<TexName, PathBuf>,
    /// The folder's `RIFT_APART` (PLAN 9.2), and the surfaces it pairs with a Rift Apart material when it is there (its
    /// `surfaces.tsv`): drawn with them while the `rift_apart_surfaces` option is on (`enhance::rift_apart`).
    rift_dir: PathBuf,
    rift: Option<HashMap<TexName, RiftSurface>>,
    /// The Rift Apart folder's `replacements` (the remaster's own art for what is not a surface, the HUD's and the menus'
    /// pictures: PCSX2-named files as a pack's), drawn ahead of `files` while the option is on.
    rift_files: HashMap<TexName, PathBuf>,
    /// The Rift Apart folder's `lights.tsv` (`LIGHTS_HEADER`): the textures that are lights, each with its boost and glow
    /// (`light`), drawn so while the option is on.
    lights: HashMap<TexName, [f32; 2]>,
    /// The Rift Apart folder's `relief.tsv` when there (`RELIEF_HEADER`): per texture its relief map (`relief/FILE`, a normal
    /// made from the texture's own generated height) and its strength, drawn while the option is on.
    relief: HashMap<TexName, (String, f32)>,
    /// The level's replacements on the GPU: those `preload` uploaded as it loaded, and those of textures first drawn after the
    /// load (`hd`), each decoded once; emptied when the next level loads.
    views: HashMap<TexName, HdView>,
    /// The level's Rift Apart material maps on the GPU, by material, the same way (`rift`).
    materials: HashMap<String, wgpu::TextureView>,
    /// Since the last `take_counts`: textures named, textures replaced, naming errors.
    named: usize,
    replaced: usize,
    errors: Vec<String>,
    /// Play only (`open_in_play`): the GPU the background worker uploads to, the worker's queue, what it has been asked for and not
    /// yet landed (`request`), and the first decode that failed (`poll` stops the game with it).
    gpu: Option<(wgpu::Device, wgpu::Queue)>,
    worker: Option<std::sync::mpsc::Sender<Job>>,
    inflight: HashSet<Want>,
    failed: Option<String>,
}

/// What the background worker is asked for: a pack file's replacement or a Rift Apart material's map (by its key).
#[derive(Clone, PartialEq, Eq, Hash)]
enum Want {
    Pack(TexName),
    Map(String),
}

/// A decode the worker is asked for: of the level `level_no` (`GENERATION`), a stale one is dropped.
struct Job {
    level_no: u64,
    want: Want,
    file: PathBuf,
}

/// A decode the worker finished: its replacement on the GPU or the file's error.
struct Landed {
    level_no: u64,
    want: Want,
    result: Result<Done, String>,
}

enum Done {
    Pack(HdView),
    Map(wgpu::TextureView),
}

/// Whether textures first drawn after their level's load are decoded by the background worker (`uya play`: `open_in_play`) rather
/// than where they are asked for (`uya native`, whose pictures wait for every replacement).
static BACKGROUND: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// The level the worker's decodes are for: each level's load (`preload`) makes a new one.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Counts the landings `poll` has taken into the level's textures: a `Materials` with a replacement to wait for looks again when it
/// changes.
static EPOCH: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Set when the worker has landed something `poll` has not taken.
static LANDED_FLAG: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static LANDED: std::sync::Mutex<Vec<Landed>> = std::sync::Mutex::new(Vec::new());
/// Set when a decode failed (`Hd::failed`).
static FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The result of a lookup by name that may be waiting for the worker.
enum Got<T> {
    /// The replacement, or none for the texture.
    Now(Option<T>),
    /// Asked of the worker, not landed yet: the game's own texture until it has.
    Later,
}

/// One level of a replacement: RGBA8, alpha on the GS scale (0x80 = opaque).
struct Level {
    w: u32,
    h: u32,
    rgba: Vec<u8>,
}

/// A replacement: the file's image as level 0, then each level half the size (`half`) down to 1 x 1, less the top levels the
/// UPDATED TEXTURES level's cap drops (`capped`).
struct Repl {
    levels: Vec<Level>,
    /// The least alpha of the file's own image (GS scale, 0x80 opaque), before any level is dropped: a hole a dropped level had
    /// stays one for the alpha test.
    min_alpha: u8,
}

/// A replacement on the GPU: its view, its level count and the least alpha of its level 0 (GS scale, 0x80 opaque).
#[derive(Clone)]
pub(crate) struct HdView {
    pub view: wgpu::TextureView,
    pub levels: usize,
    pub min_alpha: u8,
}

/// The folder in the pack folder that holds the Rift Apart surfaces the player plays with (`scripts/riftapart-pair.py`), unless
/// `open` is given another (a test's, kept apart from the player's profile): `surfaces.tsv`, each surface it pairs
/// (`RiftSurface`), and per material `materials/NAME.png`, its map (`lighting::MATERIAL`).
const RIFT_APART: &str = "0-riftapart";

/// The header of `surfaces.tsv`.
const SURFACES_HEADER: &str = "stem\tmaterial\ttiles_u\ttiles_v\treflectance\tgrain\tnormal\tspecular\tmetal";

/// A surface the Rift Apart folder pairs with Rift Apart materials (a row of its `surfaces.tsv` or `regions.tsv`): its material,
/// or for a surface of regions (a character's atlas: fur, cloth, leather, metal, skin in one texture) up to four materials and
/// its region map (`regions/<file>`, RGBA: each texel's weight of material 0 to 3; what the weights leave takes no material);
/// and the normal's strength (1: the map's own).
#[derive(Clone)]
struct RiftSurface {
    slots: Vec<RiftSlot>,
    region_map: Option<String>,
    normal: f32,
    /// Atlas maps (`pbr.tsv`): slot 0 normal XY (fur weight in A), slot 1 occlusion/roughness/metalness (lens weight
    /// in A), slot 2 when given emission RGB (glow in A).
    pbr: bool,
    /// A mixed atlas's original fur tiling, kept apart from the maps' one atlas repeat.
    fur_tiles: [f32; 2],
}

/// One of a surface's materials: its name, its tiles per 1.0 of u and of v, the square root of its reflectance at normal
/// incidence, the grain's power (1: Rift Apart's own contrast, the approved look) and the specular's strength (1: as the method
/// lights it; less on a glossy material whose highlight the game's lights blow out, `lighting::MATERIAL` `material_highlight`).
/// Each material of a surface of regions has its own: a character's polished metal beside its leather. A surface of regions'
/// material may be fur (`FUR`: no map; its tiles the strands' cells, `objects` `fs_fur`), its length in the world in `fur`; an
/// eye's has a catchlight of that strength (`lighting::MATERIAL` `material_catchlight`). `metal` is how much of a metal it is (0
/// a paint, a stone, a cloth; 1 a bare metal; between, a worn or thin paint over metal): a metal reflects at `METAL_F0` and in
/// its own colour, by its gloss (`lighting::MATERIAL` `material_reflection`, `raytrace`'s traced reflections).
#[derive(Clone)]
struct RiftSlot {
    material: String,
    tiles: [f32; 2],
    reflectance: f32,
    grain: f32,
    specular: f32,
    fur: f32,
    catchlight: f32,
    metal: f32,
}

/// A bare metal's reflectance at normal incidence (F0; steel to chrome is 0.5 to 0.7, the game's own chrome in the ray tracing 0.8).
pub(crate) const METAL_F0: f32 = 0.6;

impl RiftSlot {
    /// The material's reflectance at normal incidence: its own, its metalness of the way to a metal's.
    fn f0(&self) -> f32 {
        let own = self.reflectance * self.reflectance;
        own + (METAL_F0 - own) * self.metal
    }
}

/// The material name of fur in `regions.tsv` (a slot with no map of its own).
const FUR: &str = "fur";

/// A surface's Rift Apart materials on the GPU (`rift`): each material's map with its row (`RiftSlot`: its tiles and look), a
/// surface of regions' region map, and the normal's strength (`lighting::MATERIAL`, the material uniform's `rift`, `tiles`,
/// `tiles3`, `grains`, `specs` and `refls`, bindings 3 and 5 to 8).
#[derive(Clone)]
pub(crate) struct RiftMat {
    /// Each material's map (none for fur) and row.
    slots: Vec<(Option<wgpu::TextureView>, RiftSlot)>,
    /// The region map's view and its key (`material_path`).
    region: Option<(wgpu::TextureView, String)>,
    normal: f32,
    pbr: bool,
    fur_tiles: [f32; 2],
    /// The texture's relief map (`relief.tsv`) and its strength: a normal at the texture's own coordinates, one repeat, from
    /// its generated height, over its material's when it has one (`lighting::MATERIAL` `material_relief`).
    relief: Option<(wgpu::TextureView, f32)>,
}

impl RiftMat {
    /// The material uniform's `rift` (the first material's tiles, the normal's strength, 1 for one material or 2 for regions),
    /// `tiles` (the second and third materials'), `tiles3` (the fourth's) and per material its grain's power (`grains`), its
    /// specular's strength (`specs`), the root of its reflectance with its metalness (`refls`: `RiftSlot::f0`), its fur's length
    /// (`furs`), its catchlight's strength (`catches`) and its metalness (`metals`); zeros for a texture with no Rift Apart surface.
    pub(crate) fn uniform(r: Option<&RiftMat>) -> [[f32; 4]; 9] {
        let Some(r) = r.filter(|r| r.has_material()) else { return [[0.0; 4]; 9] };
        let t = |k: usize| r.slots.get(k).map_or([0.0; 2], |s| s.1.tiles);
        let each = |f: fn(&RiftSlot) -> f32| -> [f32; 4] { std::array::from_fn(|k| r.slots.get(k).map_or(0.0, |s| f(&s.1))) };
        [
            [t(0)[0], t(0)[1], r.normal, if r.pbr { if r.slots.len() > 2 { 4.0 } else { 3.0 } } else if r.region.is_some() { 2.0 } else { 1.0 }],
            [t(1)[0], t(1)[1], t(2)[0], t(2)[1]],
            [t(3)[0], t(3)[1], r.fur_tiles[0], r.fur_tiles[1]],
            each(|s| s.grain),
            each(|s| s.specular),
            each(|s| s.f0().sqrt()),
            each(|s| s.fur),
            each(|s| s.catchlight),
            each(|s| s.metal),
        ]
    }

    /// The surface for the ray tracing's reflections (`raytrace::Shade::mat`): per material its map (its key and view; none for
    /// fur), its tiles, its F0 (`RiftSlot::f0`) and its metalness; and a surface of regions' region map (its key and view).
    #[allow(clippy::type_complexity)]
    pub(crate) fn rt(&self) -> (Vec<(Option<(String, wgpu::TextureView)>, [f32; 2], f32, f32)>, Option<(String, wgpu::TextureView)>) {
        let slots = self.slots.iter().map(|(v, s)| (v.as_ref().map(|v| (s.material.clone(), v.clone())), s.tiles, s.f0(), s.metal)).collect();
        (slots, self.region.as_ref().map(|(v, k)| (k.clone(), v.clone())))
    }

    /// Whether the surface has fur (`objects` `fs_fur` draws its shells).
    pub(crate) fn has_fur(r: Option<&RiftMat>) -> bool {
        r.is_some_and(|r| r.slots.iter().any(|s| s.1.fur > 0.0))
    }

    pub(crate) fn is_pbr(&self) -> bool { self.pbr }

    /// Whether the texture has a Rift Apart material (a relief alone is drawn on the game's own surface).
    pub(crate) fn has_material(&self) -> bool { !self.slots.is_empty() }

    /// The material uniform's `relief`: x the relief's strength (0: none).
    pub(crate) fn relief_uniform(r: Option<&RiftMat>) -> [f32; 4] {
        [r.and_then(|r| r.relief.as_ref()).map_or(0.0, |x| x.1), 0.0, 0.0, 0.0]
    }

    /// Bindings 3 to 9 of a material's bind group: the first material's map, the materials' sampler, the region map, the
    /// other three materials' maps and the relief map (`flat` for what a texture has not).
    pub(crate) fn entries<'a>(r: Option<&'a RiftMat>, flat: &'a wgpu::TextureView, sampler: &'a wgpu::Sampler) -> [wgpu::BindGroupEntry<'a>; 7] {
        let view = |v: &'a wgpu::TextureView| wgpu::BindingResource::TextureView(v);
        let slot = |k: usize| r.and_then(|r| r.slots.get(k)).and_then(|s| s.0.as_ref()).unwrap_or(flat);
        [
            wgpu::BindGroupEntry { binding: 3, resource: view(slot(0)) },
            wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::Sampler(sampler) },
            wgpu::BindGroupEntry { binding: 5, resource: view(r.and_then(|r| r.region.as_ref()).map_or(flat, |g| &g.0)) },
            wgpu::BindGroupEntry { binding: 6, resource: view(slot(1)) },
            wgpu::BindGroupEntry { binding: 7, resource: view(slot(2)) },
            wgpu::BindGroupEntry { binding: 8, resource: view(slot(3)) },
            wgpu::BindGroupEntry { binding: 9, resource: view(r.and_then(|r| r.relief.as_ref()).map_or(flat, |x| &x.0)) },
        ]
    }
}

impl Hd {
    /// The Rift Apart surface of `n` while the option is on (`enhance::rift_apart`); the option on with no Rift Apart
    /// folder (a release installed without the textures volume) is drawn without.
    fn surface(&self, n: &TexName) -> Result<Option<&RiftSurface>, String> {
        if !crate::enhance::rift_apart() {
            return Ok(None);
        }
        Ok(self.rift.as_ref().and_then(|r| r.get(n)))
    }

    /// The relief of `n` while the option is on: its map's key (`material_path`, `relief:FILE`) and strength.
    fn relief(&self, n: &TexName) -> Option<(String, f32)> {
        if !crate::enhance::rift_apart() {
            return None;
        }
        self.relief.get(n).map(|(f, k)| (format!("relief:{f}"), *k))
    }
}

/// A map's file in the Rift Apart folder: a material's (`materials/NAME.png`) or, keyed `region:FILE`, a region map's
/// (`regions/FILE`).
fn material_path(rift_dir: &Path, key: &str) -> PathBuf {
    if let Some(file) = key.strip_prefix("relief:") {
        return rift_dir.join("relief").join(file);
    }
    match key.strip_prefix("region:") {
        Some(file) => rift_dir.join("regions").join(file),
        None => rift_dir.join("materials").join(format!("{key}.png")),
    }
}

/// Every map a surface draws with (`material_path`'s keys): its materials' and its region map.
fn surface_maps(s: &RiftSurface) -> Vec<String> {
    let mut v: Vec<String> = s.slots.iter().filter(|m| m.material != FUR).map(|m| m.material.clone()).collect();
    v.extend(s.region_map.iter().map(|f| format!("region:{f}")));
    v
}

/// The header of `regions.tsv`.
const REGIONS_HEADER: &str = "stem\tmap\tnormal\tmaterial\ttiles_u\ttiles_v\treflectance\tgrain\tspecular\tfur\tcatchlight\tmetal\tmaterial1\ttiles_u1\ttiles_v1\treflectance1\tgrain1\tspecular1\tfur1\tcatchlight1\tmetal1\tmaterial2\ttiles_u2\ttiles_v2\treflectance2\tgrain2\tspecular2\tfur2\tcatchlight2\tmetal2\tmaterial3\ttiles_u3\ttiles_v3\treflectance3\tgrain3\tspecular3\tfur3\tcatchlight3\tmetal3";

/// The Rift Apart folder's `surfaces.tsv` (`SURFACES_HEADER`: per surface its stem, material, tiles along u and v,
/// reflectance's root, grain's power, normal's strength, specular's strength and metalness) and, when there, its `regions.tsv`
/// (`REGIONS_HEADER`: per surface of regions its stem, region map file and normal's strength, then four materials each with its
/// tiles, reflectance's root, grain's power, specular's strength, fur's length, catchlight's strength and metalness, `-` and
/// zeros for one it has not; `fur` a material of fur), tab-separated (`#` lines are notes). A row that is not that, a stem that is no
/// replacement name, a stem twice or a map that is not there stops the game, naming it.
fn read_surfaces(rift_dir: &Path) -> Result<HashMap<TexName, RiftSurface>, String> {
    let mut out = HashMap::new();
    for (file, header) in [("surfaces.tsv", SURFACES_HEADER), ("regions.tsv", REGIONS_HEADER), ("pbr.tsv", "stem\tnormal_map\torm_map")] {
        let path = rift_dir.join(file);
        if file != "surfaces.tsv" && !path.exists() {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|e| format!("textures: {}: {e}", path.display()))?;
        let mut lines = text.lines().filter(|l| !l.starts_with('#'));
        let actual_header = lines.next();
        if actual_header != Some(header) && !(file == "pbr.tsv" && actual_header == Some("stem\tnormal_map\torm_map\temissive_map")) {
            return Err(format!("textures: {}: the header is not {header:?}", path.display()));
        }
        for line in lines {
            let f: Vec<&str> = line.split('\t').collect();
            let bad = || format!("textures: {}: the row {line:?}", path.display());
            let num = |v: &str| v.parse::<f32>().map_err(|_| bad());
            let (stem, mut s) = match (file, &f[..]) {
                ("pbr.tsv", [stem, _normal_map, _orm_map, rest @ ..]) if rest.len() <= 1 => {
                    let mut slots=Vec::new();
                    for (index, map) in f[1..].iter().enumerate() {
                        // The emission map is optional: empty or "-" for none.
                        if index == 2 && (map.is_empty() || *map == "-") { continue; }
                        if map.is_empty() || !map.bytes().all(|b|b.is_ascii_alphanumeric() || b==b'_' || b==b'-') { return Err(bad()); }
                        slots.push(RiftSlot { material: map.to_string(), tiles:[1.0;2], reflectance:0.2, grain:0.0, specular:1.0, fur:0.0, catchlight:0.0, metal:0.0 });
                    }
                    (*stem, RiftSurface { slots, region_map:None, normal:1.0, pbr:true, fur_tiles:[0.0;2] })
                }
                ("surfaces.tsv", [stem, material, tu, tv, r, grain, normal, specular, metal]) => {
                    let slot = RiftSlot { material: material.to_string(), tiles: [num(tu)?, num(tv)?], reflectance: num(r)?, grain: num(grain)?, specular: num(specular)?, fur: 0.0, catchlight: 0.0, metal: num(metal)? };
                    (*stem, RiftSurface { slots: vec![slot], region_map: None, normal: num(normal)?, pbr: false, fur_tiles: [0.0; 2] })
                }
                ("regions.tsv", [stem, map, normal, rest @ ..]) if rest.len() == 36 => {
                    let mut slots = Vec::new();
                    for c in rest.chunks(9) {
                        if c[0] != "-" {
                            let slot = RiftSlot { material: c[0].to_string(), tiles: [num(c[1])?, num(c[2])?], reflectance: num(c[3])?, grain: num(c[4])?, specular: num(c[5])?, fur: num(c[6])?, catchlight: num(c[7])?, metal: num(c[8])? };
                            if (slot.material == FUR) != (slot.fur > 0.0) {
                                return Err(bad());
                            }
                            slots.push(slot);
                        }
                    }
                    if slots.is_empty() {
                        return Err(bad());
                    }
                    (*stem, RiftSurface { slots, region_map: Some(map.to_string()), normal: num(normal)?, pbr: false, fur_tiles: [0.0; 2] })
                }
                _ => return Err(bad()),
            };
            let n = TexName::of_file(&format!("{stem}.png")).ok_or_else(bad)?;
            for key in surface_maps(&s) {
                let m = material_path(rift_dir, &key);
                if !m.is_file() {
                    return Err(format!("textures: {}: {stem}'s map {} is not there", path.display(), m.display()));
                }
            }
            // A fitted mixed atlas keeps the surface's original fur shell length, fur tiling and lens strength; its per-pixel
            // fur and lens weights are in the normal and ORM maps' alpha.
            if s.pbr {
                if let Some(old) = out.get(&n) {
                    let old: &RiftSurface = old;
                    s.fur_tiles = old.slots.iter().find(|slot| slot.fur > 0.0).map_or([0.0; 2], |slot| slot.tiles);
                    s.slots[0].fur = old.slots.iter().map(|slot| slot.fur).fold(0.0f32, f32::max);
                    s.slots[0].catchlight = old.slots.iter().map(|slot| slot.catchlight).fold(0.0f32, f32::max);
                }
            }
            // A PBR atlas deliberately replaces the old tiled material row.
            // Duplicate PBR rows are still an error.
            let previous=out.insert(n,s);
            if previous.is_some_and(|p|file!="pbr.tsv" || p.pbr) {
                return Err(format!("textures: {}: {stem} has two rows", path.display()));
            }
        }
    }
    Ok(out)
}

/// Draws the png files under `dir` (PCSX2's rules: each named by the key of the texture it replaces; folders in it too) in place of
/// the textures they name, at their own resolution, and while the Rift Apart surfaces are on the surfaces of the Rift Apart
/// folder: `rift`, else `dir`'s `RIFT_APART` (which the packs' scan leaves out either way).
pub(crate) fn open(dir: &Path, rift: Option<&Path>) -> Result<(), String> {
    if !dir.is_dir() {
        return Err(format!("texnames: {} is not a folder", dir.display()));
    }
    // Each folder's entries in sorted order, a folder in it read in its place.
    fn scan(d: &Path, files: &mut HashMap<TexName, PathBuf>, duplicates: &mut usize, other: &mut usize) -> Result<(), String> {
        let mut entries: Vec<_> = std::fs::read_dir(d).map_err(|e| format!("{}: {e}", d.display()))?.collect::<Result<_, _>>().map_err(|e| format!("{}: {e}", d.display()))?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let path = e.path();
            if path.is_dir() {
                if d.join(RIFT_APART) == path {
                    continue;
                }
                scan(&path, files, duplicates, other)?;
                continue;
            }
            match e.file_name().to_str().and_then(TexName::of_file) {
                Some(n) if files.contains_key(&n) => *duplicates += 1,
                Some(n) => {
                    files.insert(n, path);
                }
                None => *other += 1,
            }
        }
        Ok(())
    }
    let (mut files, mut duplicates, mut other) = (HashMap::new(), 0, 0);
    scan(dir, &mut files, &mut duplicates, &mut other)?;
    println!("textures: {} replacement names in {} ({duplicates} duplicates, {other} other files: names with a clamp region, which the port does not make, and anything else)", files.len(), dir.display());
    let rift_dir = match rift {
        // A folder named but not there (a release installed without the updated-textures volume): drawn without it, the
        // same as no folder given. Every use of the path below needs the folder there (line 458 names it when it is not).
        Some(r) if !r.is_dir() => {
            println!("textures: the updated-textures folder {} is not there; drawn without it", r.display());
            dir.join(RIFT_APART)
        }
        Some(r) => r.to_path_buf(),
        None => dir.join(RIFT_APART),
    };
    let lights = match rift_dir.is_dir() {
        true => read_lights(&rift_dir)?,
        false => HashMap::new(),
    };
    let rift = match rift_dir.is_dir() {
        true => {
            let r = read_surfaces(&rift_dir)?;
            let materials: HashSet<&str> = r.values().flat_map(|s| s.slots.iter().map(|m| m.material.as_str()).filter(|m| *m != FUR)).collect();
            println!("textures: {} surfaces paired with {} updated-texture materials in {}, drawn while UPDATED TEXTURES is on", r.len(), materials.len(), rift_dir.display());
            Some(r)
        }
        false => None,
    };
    // UPDATED TEXTURES draws, and lights per pixel, only with its surfaces there (`enhance::rift_apart`).
    if rift.is_none() {
        println!("textures: no updated textures: UPDATED TEXTURES draws nothing, and its per-pixel lighting is off");
    }
    crate::enhance::set_rift_surfaces(rift.is_some());
    let relief = match rift_dir.join("relief.tsv").is_file() {
        true => {
            let r = read_relief(&rift_dir)?;
            println!("textures: {} textures with a relief from their generated height in {}, drawn while UPDATED TEXTURES is on", r.len(), rift_dir.display());
            r
        }
        false => HashMap::new(),
    };
    let mut rift_files = HashMap::new();
    let replacements = rift_dir.join("replacements");
    if replacements.is_dir() {
        let (mut d, mut o) = (0, 0);
        scan(&replacements, &mut rift_files, &mut d, &mut o)?;
        println!("textures: {} remaster replacements in {} ({d} duplicates, {o} other files), drawn while UPDATED TEXTURES is on", rift_files.len(), replacements.display());
    }
    *HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")? = Some(Hd { files, rift_dir, rift, rift_files, lights, relief, views: HashMap::new(), materials: HashMap::new(), named: 0, replaced: 0, errors: Vec::new(), gpu: None, worker: None, inflight: HashSet::new(), failed: None });
    Ok(())
}

/// `open` for `uya play`: what is first drawn after its level's load is decoded and uploaded by a worker at below normal priority
/// (`replacements`), the game's own texture drawn until it has landed.
pub(crate) fn open_in_play(dir: &Path, rift: Option<&Path>) -> Result<(), String> {
    open(dir, rift)?;
    BACKGROUND.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// The images (a texture's `tex0` and `bits`, whatever its palette) the 2D layer draws: the HUD's and the menus' (`eedraw`'s
/// textures, noted by `dump_ui` as each is made) and the HUD's and resident images found as a level loads
/// (`PalettedNames::image`). UPDATED TEXTURES at HUD ONLY draws replacements for these alone.
static UI_IMAGES: std::sync::Mutex<Option<HashSet<(u64, u32)>>> = std::sync::Mutex::new(None);

fn note_ui(images: impl IntoIterator<Item = (u64, u32)>) -> Result<(), String> {
    UI_IMAGES.lock().map_err(|_| "texnames: the 2D layer's lock is poisoned")?.get_or_insert_with(HashSet::new).extend(images);
    Ok(())
}

fn is_ui(n: &TexName) -> bool {
    UI_IMAGES.lock().expect("texnames: the 2D layer's lock is poisoned").as_ref().is_some_and(|u| u.contains(&(n.tex0, n.bits)))
}

impl Hd {
    /// The file drawn in place of `n` at the UPDATED TEXTURES level now (`enhance::texture_level`): none at OFF; at HUD ONLY one
    /// for a texture of the 2D layer alone; else any. The remaster's own (`rift_files`) first, then the pack's.
    fn file(&self, n: &TexName) -> Option<&PathBuf> {
        use crate::enhance::{TEXTURES_HUD, TEXTURES_OFF};
        match crate::enhance::texture_level() {
            TEXTURES_OFF => None,
            TEXTURES_HUD if !is_ui(n) => None,
            _ => self.rift_files.get(n).or_else(|| self.files.get(n)),
        }
    }
}

/// `uya native --dump-textures DIR`: every texture named while drawing (the scene's, the HUD's, the menus') written to DIR as
/// PCSX2 dumps it (`dump`), once.
static DUMP: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub(crate) fn dump_to(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    *DUMP.lock().map_err(|_| "texnames: the dump lock is poisoned")? = Some(dir.to_path_buf());
    Ok(())
}

/// `uya native --dump-ui DIR`: the 2D layer's textures (the HUD's and the menus', `eedraw`) written to DIR as PCSX2 dumps them,
/// once each: the UI's pictures apart from the scene's.
static DUMP_UI: std::sync::Mutex<Option<PathBuf>> = std::sync::Mutex::new(None);

pub(crate) fn dump_ui_to(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    *DUMP_UI.lock().map_err(|_| "texnames: the dump lock is poisoned")? = Some(dir.to_path_buf());
    Ok(())
}

/// A 2D-layer texture made (`eedraw`), dumped when `--dump-ui` asked.
pub(crate) fn dump_ui(t: &uyafmt::core::Texture) -> Result<(), String> {
    // A texture of the 2D layer (UPDATED TEXTURES at HUD ONLY draws replacements for these alone): a name made here is noted
    // whether or not a dump is asked for; one the port cannot name has no replacement to draw.
    if let Ok(Some(n)) = name(t) {
        note_ui([(n.tex0, n.bits)])?;
    }
    if let Some(dir) = DUMP_UI.lock().map_err(|_| "texnames: the dump lock is poisoned")?.as_ref() {
        if let Some(n) = name(t)? {
            dump(dir, t, &n.stem())?;
        }
    }
    Ok(())
}

/// The replacement of `t` on the GPU when the pack folder has one: uploaded by `preload` as its level loaded, else (a texture
/// first drawn after the load, as Clank's skin is with the level's first frame) decoded and uploaded now, once for the level. A
/// file that cannot be read or decoded stops the game, naming it. (`replacements` is the lookup of a texture drawn while playing.)
pub(crate) fn hd(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> Result<Option<HdView>, String> {
    Ok(hd_get(device, queue, t, false)?.0)
}

/// `hd`, or with `background` the replacement asked of the worker when it is not there yet: the second is whether it is to come.
fn hd_get(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture, background: bool) -> Result<(Option<HdView>, bool), String> {
    // `uya native --surfaces`: the terrain's and the ties' textures drawn as their id colours (`surfaces`).
    if crate::surfaces::on() {
        return Ok((crate::surfaces::id_view(device, queue, t)?, false));
    }
    let mut g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
    let Some(hd) = g.as_mut() else { return Ok((None, false)) };
    let name = match name(t) {
        Ok(Some(n)) => n,
        Ok(None) => return Ok((None, false)),
        Err(e) => {
            hd.errors.push(e);
            return Ok((None, false));
        }
    };
    hd.named += 1;
    if let Some(dir) = DUMP.lock().map_err(|_| "texnames: the dump lock is poisoned")?.as_ref() {
        dump(dir, t, &name.stem())?;
    }
    let Some(file) = hd.file(&name).cloned() else { return Ok((None, false)) };
    hd.replaced += 1;
    if let Some(v) = hd.views.get(&name) {
        return Ok((Some(v.clone()), false));
    }
    if background {
        hd.gpu.get_or_insert_with(|| (device.clone(), queue.clone()));
        hd.request(Want::Pack(name), file)?;
        return Ok((None, true));
    }
    let t0 = std::time::Instant::now();
    let v = to_gpu(device, queue, &decode(&file)?, usize::MAX)?;
    println!("textures: {} decoded when first drawn, not with its level's load: {:.1} ms", file.display(), t0.elapsed().as_secs_f64() * 1e3);
    hd.views.insert(name, v.clone());
    Ok((Some(v), false))
}

/// `t`'s Rift Apart material on the GPU while UPDATED TEXTURES is on and the Rift Apart folder pairs it: its map uploaded by `preload`
/// with the level, else decoded and uploaded now, once for the level. A map that cannot be read or decoded stops the game,
/// naming it.
pub(crate) fn rift(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> Result<Option<RiftMat>, String> {
    if crate::surfaces::on() {
        return Ok(None);
    }
    let mut g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
    let Some(hd) = g.as_mut() else { return Ok(None) };
    // A texture with no name has no surface (`hd`, which every texture goes through, counts the naming errors).
    let Ok(Some(name)) = name(t) else { return Ok(None) };
    let surface = hd.surface(&name)?.cloned();
    let relief = hd.relief(&name);
    if surface.is_none() && relief.is_none() {
        return Ok(None);
    }
    for key in rift_maps(surface.as_ref(), relief.as_ref()) {
        if hd.materials.contains_key(&key) {
            continue;
        }
        let t0 = std::time::Instant::now();
        let mf = material_path(&hd.rift_dir, &key);
        let v = upload_levels(device, queue, "Rift Apart material", &decode_map(&mf)?)?;
        println!("textures: {} decoded when first drawn, not with its level's load: {:.1} ms", mf.display(), t0.elapsed().as_secs_f64() * 1e3);
        hd.materials.insert(key, v);
    }
    Ok(Some(hd.rift_mat(surface, relief)))
}

/// Every map a texture's Rift Apart material draws with: its relief's and its surface's (`surface_maps`).
fn rift_maps(surface: Option<&RiftSurface>, relief: Option<&(String, f32)>) -> Vec<String> {
    let mut keys: Vec<String> = relief.map(|r| r.0.clone()).into_iter().collect();
    keys.extend(surface.into_iter().flat_map(surface_maps));
    keys
}

impl Hd {
    /// The material of a surface and relief whose maps are all in `materials`.
    fn rift_mat(&self, surface: Option<RiftSurface>, relief: Option<(String, f32)>) -> RiftMat {
        let view = |key: &str| self.materials.get(key).expect("every map of the material is there").clone();
        let relief = relief.map(|(k, strength)| (view(&k), strength));
        let Some(s) = surface else {
            return RiftMat { slots: Vec::new(), region: None, normal: 1.0, pbr: false, fur_tiles: [0.0; 2], relief };
        };
        let slots = s.slots.iter().map(|m| (if m.material == FUR { None } else { Some(view(&m.material)) }, m.clone())).collect();
        let region = s.region_map.as_ref().map(|f| format!("region:{f}")).map(|k| (view(&k), k));
        RiftMat { slots, region, normal: s.normal, pbr: s.pbr, fur_tiles: s.fur_tiles, relief }
    }

    /// Asks the worker for `want`, once (the worker is started with the first ask).
    fn request(&mut self, want: Want, file: PathBuf) -> Result<(), String> {
        if !self.inflight.insert(want.clone()) {
            return Ok(());
        }
        if self.worker.is_none() {
            let (device, queue) = self.gpu.clone().ok_or("texnames: a background decode before any texture was looked up")?;
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            std::thread::Builder::new().name("texture decode".into()).spawn(move || decode_worker(rx, device, queue)).map_err(|e| format!("texnames: the decode thread: {e}"))?;
            self.worker = Some(tx);
        }
        let job = Job { level_no: GENERATION.load(std::sync::atomic::Ordering::Relaxed), want, file };
        self.worker.as_ref().expect("started above").send(job).map_err(|_| "texnames: the decode thread ended".to_string())
    }

    /// The replacement of `name`: its view when it is there, else asked of the worker.
    fn pack_by_name(&mut self, name: &TexName) -> Result<Got<HdView>, String> {
        let Some(file) = self.file(name).cloned() else { return Ok(Got::Now(None)) };
        if let Some(v) = self.views.get(name) {
            return Ok(Got::Now(Some(v.clone())));
        }
        self.request(Want::Pack(*name), file)?;
        Ok(Got::Later)
    }

    /// The Rift Apart material of `name`: when every map of it is there, else the missing ones asked of the worker.
    fn rift_by_name(&mut self, name: &TexName) -> Result<Got<RiftMat>, String> {
        let surface = self.surface(name)?.cloned();
        let relief = self.relief(name);
        if surface.is_none() && relief.is_none() {
            return Ok(Got::Now(None));
        }
        let mut later = false;
        for key in rift_maps(surface.as_ref(), relief.as_ref()) {
            if !self.materials.contains_key(&key) {
                self.request(Want::Map(key.clone()), material_path(&self.rift_dir, &key))?;
                later = true;
            }
        }
        Ok(if later { Got::Later } else { Got::Now(Some(self.rift_mat(surface, relief))) })
    }
}

/// What a `Materials` holds of a texture's replacements: its pack texture, its Rift Apart material, and whether either is still to
/// come from the worker (`replacements_named` asks again once `poll`'s count has moved on).
pub(crate) type Replacements = (Option<HdView>, Option<RiftMat>, bool);

/// `t`'s replacements while playing: as `hd` and `rift`, but what is not on the GPU yet is asked of the worker (decoded and uploaded
/// at below normal priority, off the render thread) and left out, the game's own texture drawn until it has landed. With no
/// worker (`uya native`) the same as `hd` and `rift`.
pub(crate) fn replacements(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> Result<Replacements, String> {
    if !BACKGROUND.load(std::sync::atomic::Ordering::Relaxed) || crate::surfaces::on() {
        return Ok((hd(device, queue, t)?, rift(device, queue, t)?, false));
    }
    poll()?;
    let (pack, pack_later) = hd_get(device, queue, t, true)?;
    let mut g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
    let Some(hd) = g.as_mut() else { return Ok((None, None, false)) };
    let Ok(Some(name)) = name(t) else { return Ok((pack, None, pack_later)) };
    let (material, material_later) = match hd.rift_by_name(&name)? {
        Got::Now(m) => (m, false),
        Got::Later => (None, true),
    };
    Ok((pack, material, pack_later || material_later))
}

/// `replacements` of the texture named `name`, once the worker has landed more (`poll`'s count changed).
pub(crate) fn replacements_named(name: &TexName) -> Result<Replacements, String> {
    poll()?;
    let mut g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
    let Some(hd) = g.as_mut() else { return Ok((None, None, false)) };
    let (pack, pack_later) = match hd.pack_by_name(name)? {
        Got::Now(v) => (v, false),
        Got::Later => (None, true),
    };
    let (material, material_later) = match hd.rift_by_name(name)? {
        Got::Now(m) => (m, false),
        Got::Later => (None, true),
    };
    Ok((pack, material, pack_later || material_later))
}

/// Takes what the worker has landed into the level's textures and returns the count of landings so far (a `Materials` waiting for
/// a replacement looks again when it has changed). A decode that failed stops the game, naming its file.
pub(crate) fn poll() -> Result<u64, String> {
    use std::sync::atomic::Ordering::{Acquire, Relaxed, Release};
    if LANDED_FLAG.swap(false, Acquire) {
        let landed = std::mem::take(&mut *LANDED.lock().map_err(|_| "texnames: the landing lock is poisoned")?);
        let mut g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
        let hd = g.as_mut().ok_or("texnames: a decode landed with no pack folder")?;
        let level_no = GENERATION.load(Relaxed);
        let mut any = false;
        for l in landed.into_iter().filter(|l| l.level_no == level_no) {
            hd.inflight.remove(&l.want);
            any = true;
            match (l.want, l.result) {
                (Want::Pack(n), Ok(Done::Pack(v))) => {
                    hd.views.entry(n).or_insert(v);
                }
                (Want::Map(k), Ok(Done::Map(v))) => {
                    hd.materials.entry(k).or_insert(v);
                }
                (_, Err(e)) => {
                    hd.failed.get_or_insert(e);
                    FAILED.store(true, Release);
                }
                _ => return Err("texnames: a decode landed as the other kind".into()),
            }
        }
        if any {
            EPOCH.fetch_add(1, Release);
        }
    }
    if FAILED.load(Acquire) {
        let g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
        if let Some(e) = g.as_ref().and_then(|h| h.failed.clone()) {
            return Err(e);
        }
    }
    Ok(EPOCH.load(Acquire))
}

/// The bytes of one texture write by the background worker (`upload`).
const UPLOAD_BAND: usize = 512 << 10;

/// The background worker: decodes and uploads what `Hd::request` sends, one at a time at below normal priority (a two-core PC must
/// not lose its frame builder to it), dropping what a new level made stale.
fn decode_worker(rx: std::sync::mpsc::Receiver<Job>, device: wgpu::Device, queue: wgpu::Queue) {
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_BELOW_NORMAL};
        // A thread that keeps its priority still decodes, only against the frame builder: the priority is no reason to stop.
        let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_BELOW_NORMAL) };
    }
    for job in rx {
        if job.level_no != GENERATION.load(std::sync::atomic::Ordering::Relaxed) {
            continue;
        }
        let t0 = std::time::Instant::now();
        let result = match &job.want {
            Want::Pack(_) => decode(&job.file).and_then(|r| to_gpu(&device, &queue, &r, UPLOAD_BAND)).map(Done::Pack),
            Want::Map(_) => decode_map(&job.file).and_then(|l| upload(&device, &queue, "Rift Apart material", &l.iter().map(|l| (l.w, l.h, &l.rgba[..])).collect::<Vec<_>>(), UPLOAD_BAND)).map(Done::Map),
        };
        if result.is_ok() {
            println!("textures: {} decoded in the background, not with its level's load: {:.1} ms", job.file.display(), t0.elapsed().as_secs_f64() * 1e3);
        }
        LANDED.lock().expect("texnames: the landing lock is poisoned").push(Landed { level_no: job.level_no, want: job.want, result });
        LANDED_FLAG.store(true, std::sync::atomic::Ordering::Release);
    }
}

/// A Rift Apart material's map decoded: its image as level 0 and the levels below it, as it is (no GS alpha), less the top
/// levels the UPDATED TEXTURES level drops (`capped`: the maps' cap).
fn decode_map(file: &Path) -> Result<Vec<Level>, String> {
    let (w, h, rgba) = std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| crate::pic::read_png(&b)).map_err(|e| format!("textures: {}: {e}", file.display()))?;
    Ok(capped(vec![Level { w, h, rgba }], crate::enhance::texture_caps().map(|c| c.1)))
}

/// `levels` (just level 0) with its chain made down to 1 x 1 and the top levels dropped until the larger side is at most `cap`
/// texels (`enhance::texture_caps`; None, no cap: the whole chain). A level that is no bigger than the cap is never dropped.
fn capped(mut levels: Vec<Level>, cap: Option<u32>) -> Vec<Level> {
    while let Some(l) = levels.last().filter(|l| l.w > 1 || l.h > 1) {
        levels.push(half(l));
    }
    if let Some(cap) = cap {
        let drop = levels.iter().take_while(|l| l.w.max(l.h) > cap).count().min(levels.len() - 1);
        levels.drain(..drop);
    }
    levels
}

fn upload_levels(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, levels: &[Level]) -> Result<wgpu::TextureView, String> {
    upload(device, queue, label, &levels.iter().map(|l| (l.w, l.h, &l.rgba[..])).collect::<Vec<_>>(), usize::MAX)
}

/// The header of `lights.tsv`.
const LIGHTS_HEADER: &str = "stem\tboost\tglow";

/// The Rift Apart folder's `lights.tsv` (`LIGHTS_HEADER`: per texture that is a light, a lamp's lit panel, a strip, neon, a
/// screen, its stem, its boost and its glow; `#` lines are notes). A row that is not that, or a stem twice, stops the game.
/// The header of `relief.tsv`.
const RELIEF_HEADER: &str = "stem\tmap\tstrength";

/// The Rift Apart folder's `relief.tsv` (`RELIEF_HEADER`: per texture its stem, its relief map's file in `relief/` and the
/// relief's strength, 1 the map's own slope), tab-separated (`#` lines are notes). A row that is not that, a stem that is no
/// replacement name, a stem twice or a map that is not there stops the game, naming it.
fn read_relief(rift_dir: &Path) -> Result<HashMap<TexName, (String, f32)>, String> {
    let path = rift_dir.join("relief.tsv");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("textures: {}: {e}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.starts_with('#') && !l.is_empty());
    if lines.next() != Some(RELIEF_HEADER) {
        return Err(format!("textures: {}: the header is not {RELIEF_HEADER:?}", path.display()));
    }
    let mut out = HashMap::new();
    for line in lines {
        let bad = || format!("textures: {}: the row {line:?}", path.display());
        let [stem, map, strength] = line.split('\t').collect::<Vec<_>>()[..] else { return Err(bad()) };
        let n = TexName::of_file(&format!("{stem}.png")).ok_or_else(bad)?;
        if map.is_empty() || !map.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-' || b == b'.') {
            return Err(bad());
        }
        if !rift_dir.join("relief").join(map).is_file() {
            return Err(format!("textures: {}: {stem}'s relief map relief/{map} is not there", path.display()));
        }
        if out.insert(n, (map.to_string(), strength.parse::<f32>().map_err(|_| bad())?)).is_some() {
            return Err(format!("textures: {}: {stem} has two rows", path.display()));
        }
    }
    Ok(out)
}

fn read_lights(rift_dir: &Path) -> Result<HashMap<TexName, [f32; 2]>, String> {
    let path = rift_dir.join("lights.tsv");
    let text = std::fs::read_to_string(&path).map_err(|e| format!("textures: {}: {e}", path.display()))?;
    let mut lines = text.lines().filter(|l| !l.starts_with('#'));
    if lines.next() != Some(LIGHTS_HEADER) {
        return Err(format!("textures: {}: the header is not {LIGHTS_HEADER:?}", path.display()));
    }
    let mut out = HashMap::new();
    for line in lines {
        let bad = || format!("textures: {}: the row {line:?}", path.display());
        let [stem, boost, glow] = line.split('\t').collect::<Vec<_>>()[..] else { return Err(bad()) };
        let n = TexName::of_file(&format!("{stem}.png")).ok_or_else(bad)?;
        let v = [boost.parse::<f32>().map_err(|_| bad())?, glow.parse::<f32>().map_err(|_| bad())?];
        if out.insert(n, v).is_some() {
            return Err(format!("textures: {}: {stem} has two rows", path.display()));
        }
    }
    Ok(out)
}

/// `t`'s light while UPDATED TEXTURES is on and the Rift Apart folder says it is one (`lights.tsv`): its boost (its texel at
/// that many times its colour whatever light falls on it), and no glow: the folder's glow (a share above 0x80 of the frame's alpha
/// it would write) is not taken. The game's glow extracts every pixel above 0x80 and spreads it, so a light written there
/// bloomed by how much of it the view held and what was bright around it, which the original never does to these textures (the
/// player, 16 November 2026, of Florana's green lights and green pads: "lights up super bright at certain angles but isnt at
/// others. Its not meant to light up"). Zeros for any other texture.
pub(crate) fn light(t: &uyafmt::core::Texture) -> Result<[f32; 2], String> {
    if !crate::enhance::rift_apart() || crate::surfaces::on() {
        return Ok([0.0; 2]);
    }
    let g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
    let Some(hd) = g.as_ref() else { return Ok([0.0; 2]) };
    let Ok(Some(name)) = name(t) else { return Ok([0.0; 2]) };
    Ok(hd.lights.get(&name).map_or([0.0; 2], |&[boost, _]| [boost, 0.0]))
}

/// The Rift Apart folder's water normal (`water_n.png`: Rift Apart's own tiling water normal, `reflections` ripples the water's
/// reflection with it) while UPDATED TEXTURES is on; None while it is off, or while the folder (a release installed without
/// the textures volume) or the file is not there: the water keeps the game's own.
pub(crate) fn water_normal(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Option<wgpu::TextureView>, String> {
    if !crate::enhance::rift_apart() {
        return Ok(None);
    }
    let dir = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?.as_ref().map(|hd| hd.rift_dir.clone());
    let Some(dir) = dir else { return Ok(None) };
    let f = dir.join("water_n.png");
    if !f.is_file() {
        return Ok(None);
    }
    Ok(Some(upload_levels(device, queue, "Rift Apart water normal", &decode_map(&f)?)?))
}

/// The material map every material without a Rift Apart surface binds (not read: the material uniform's `rift.w` is 0).
pub(crate) fn flat_material(device: &wgpu::Device, queue: &wgpu::Queue) -> Result<wgpu::TextureView, String> {
    upload(device, queue, "no Rift Apart material", &[(1, 1, &[128, 128, 0, 102][..])], usize::MAX)
}

/// A pack file decoded: its image as level 0 and the levels below it, less the top levels the UPDATED TEXTURES level drops
/// (`capped`: the textures' cap).
fn decode(file: &Path) -> Result<Repl, String> {
    let (w, h, mut rgba) = std::fs::read(file).map_err(|e| e.to_string()).and_then(|b| crate::pic::read_png(&b)).map_err(|e| format!("textures: {}: {e}", file.display()))?;
    // A pack's alpha is the GS's (0x80 opaque): above 0x80 is opaque.
    rgba.chunks_exact_mut(4).for_each(|p| p[3] = p[3].min(0x80));
    let min_alpha = rgba.chunks_exact(4).map(|p| p[3]).min().ok_or_else(|| format!("textures: {}: an empty image", file.display()))?;
    Ok(Repl { levels: capped(vec![Level { w, h, rgba }], crate::enhance::texture_caps().map(|c| c.0)), min_alpha })
}

/// A level's replacements (`textures`: every texture of its scene) decoded by at most four workers (the game runs on while
/// its level loads) and uploaded here as each is decoded, for `hd` to hand out. Decoded one after another, a level's 280 to 560
/// pack files took 1.5 to 3.9 s; all decoded before any was uploaded and all staged before one submit, they held 600 MB more
/// memory through the level.
pub(crate) fn preload(device: &wgpu::Device, queue: &wgpu::Queue, textures: &[&uyafmt::core::Texture]) -> Result<(), String> {
    let names: Vec<_> = textures.iter().filter_map(|t| name(t).ok().flatten()).collect();
    preload_names(device, queue, &names, true, "scene")
}

/// Extend the current level's cache when another authoritative asset inventory becomes available during loading. Only the
/// initial scene preload resets it; HUD/GS RAM preloading must retain the already uploaded scene and character materials.
fn preload_names(device: &wgpu::Device, queue: &wgpu::Queue, names: &[TexName], reset: bool, label: &str) -> Result<(), String> {
    // The level's pack files, then its Rift Apart materials' maps (`rift`).
    let (jobs, maps): (Vec<(TexName, PathBuf)>, Vec<(String, PathBuf)>) = {
        let mut g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
        let Some(hd) = g.as_mut() else { return Ok(()) };
        if reset {
            hd.views.clear();
            hd.materials.clear();
            // What the worker still holds for the level that went is dropped (`decode_worker`, `poll`).
            GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            hd.inflight.clear();
        }
        let mut seen = HashSet::new();
        let (mut jobs, mut maps) = (Vec::new(), Vec::new());
        let mut materials = HashSet::new();
        for n in names.iter().copied().filter(|n| seen.insert(*n)) {
            if !hd.views.contains_key(&n) {
                if let Some(f) = hd.file(&n) {
                    jobs.push((n, f.clone()));
                }
            }
            if let Some(s) = hd.surface(&n)? {
                for key in surface_maps(s) {
                    if !hd.materials.contains_key(&key) && materials.insert(key.clone()) {
                        maps.push((key.clone(), material_path(&hd.rift_dir, &key)));
                    }
                }
            }
            if let Some((key, _)) = hd.relief(&n) {
                if !hd.materials.contains_key(&key) && materials.insert(key.clone()) {
                    maps.push((key.clone(), material_path(&hd.rift_dir, &key)));
                }
            }
        }
        jobs.sort_unstable_by_key(|(n, _)| (n.tex0, n.clut, n.bits));
        maps.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        (jobs, maps)
    };
    let t0 = std::time::Instant::now();
    let mut uploaded = Vec::with_capacity(maps.len());
    let next = std::sync::atomic::AtomicUsize::new(0);
    // These are full-resolution material maps and mip chains. A queue per CPU plus one pending decode per worker can keep
    // dozens of large images alive while uploads wait for the GPU. Bound both the workers and the upload backlog.
    let threads = std::thread::available_parallelism().map_err(|e| format!("texnames: {e}"))?.get().saturating_sub(2).clamp(1, 4);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<(String, Vec<Level>), String>>(1);
    std::thread::scope(|s| -> Result<(), String> {
        for _ in 0..threads {
            let (tx, next, maps) = (tx.clone(), &next, &maps);
            s.spawn(move || loop {
                let Some((m, mf)) = maps.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) else { return };
                if tx.send(decode_map(mf).map(|l| (m.clone(), l))).is_err() {
                    return;
                }
            });
        }
        drop(tx);
        for r in rx {
            let (m, l) = r?;
            uploaded.push((m, upload_levels(device, queue, "Rift Apart material", &l)?));
        }
        Ok(())
    })?;
    HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?.as_mut().expect("open above").materials.extend(uploaded);
    let threads = std::thread::available_parallelism().map_err(|e| format!("texnames: {e}"))?.get().saturating_sub(2).clamp(1, 4);
    let next = std::sync::atomic::AtomicUsize::new(0);
    // One queued replacement, in addition to those held by the bounded decode workers.
    let (tx, rx) = std::sync::mpsc::sync_channel::<Result<(TexName, Repl), String>>(1);
    let mut views = Vec::with_capacity(jobs.len());
    std::thread::scope(|s| -> Result<(), String> {
        for _ in 0..threads {
            let (tx, next, jobs) = (tx.clone(), &next, &jobs);
            s.spawn(move || loop {
                let Some((n, file)) = jobs.get(next.fetch_add(1, std::sync::atomic::Ordering::Relaxed)) else { return };
                // A closed channel: the uploads stopped on an error, which they return.
                if tx.send(decode(file).map(|r| (*n, r))).is_err() {
                    return;
                }
            });
        }
        drop(tx);
        for r in rx {
            let (n, r) = r?;
            views.push((n, to_gpu(device, queue, &r, usize::MAX)?));
        }
        Ok(())
    })?;
    let mut g = HD.lock().map_err(|_| "texnames: the pack folder's lock is poisoned")?;
    g.as_mut().expect("open above").views.extend(views);
    if !jobs.is_empty() || !maps.is_empty() {
        println!("textures: {label}: {} replacements and {} Rift Apart materials preloaded: {:.2} s", jobs.len(), maps.len(), t0.elapsed().as_secs_f64());
    }
    Ok(())
}

/// Half the size as the GPU sizes a texture's levels (each side halved and rounded down, at least 1: a 2045-wide replacement
/// halved rounding up made one level more than the GPU allows), each texel the rounded mean of the texels below it (2 x 2, three
/// across the last of an odd side).
fn half(l: &Level) -> Level {
    let (w, h) = ((l.w / 2).max(1), (l.h / 2).max(1));
    // The texels of `l` under texel `i` of a side halved from `n`: two, three at the last of an odd side, one of a side of 1.
    let span = |i: u32, n: u32, out: u32| (2 * i).min(n - 1)..if i + 1 == out { n } else { 2 * i + 2 };
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let (mut sum, mut n) = ([0u32; 4], 0);
            for sy in span(y, l.h, h) {
                for sx in span(x, l.w, w) {
                    let o = ((sy * l.w + sx) * 4) as usize;
                    for k in 0..4 {
                        sum[k] += l.rgba[o + k] as u32;
                    }
                    n += 1;
                }
            }
            rgba.extend(sum.map(|s| ((s + n / 2) / n) as u8));
        }
    }
    Level { w, h, rgba }
}

/// The end of a level's load: what `hd` met since the last call (None with no pack folder): textures named, textures replaced,
/// naming errors.
pub(crate) fn take_counts() -> Option<(usize, usize, Vec<String>)> {
    let mut g = HD.lock().expect("texnames: the pack folder's lock is poisoned");
    let hd = g.as_mut()?;
    let out = (hd.named, hd.replaced, std::mem::take(&mut hd.errors));
    (hd.named, hd.replaced) = (0, 0);
    Some(out)
}

/// Whether a draw with CLAMP `clamp` of a `w` x `h` texture samples it with no clamp region (the name made here is that draw's).
/// (PCSX2's `SourceRegion::Create`: REGION_CLAMP with a range smaller than the texture or reaching past it, or REGION_REPEAT with a
/// mask that leaves a rectangle, names a region.)
pub(crate) fn region_free(w: usize, h: usize, clamp: u64) -> bool {
    let axis = |mode: u64, min: i64, max: i64, size: i64| match mode {
        2 => !(max >= min && (max - min + 1 < size || max >= size)),
        3 if min != 0 => {
            let r = ((min | max) - max) + 1;
            !(r < size || (max != 0 && r <= size))
        }
        _ => true,
    };
    let f = |sh: u32| (clamp >> sh & 0x3ff) as i64;
    axis(clamp & 3, f(4), f(14), w.min(1024) as i64) && axis(clamp >> 2 & 3, f(24), f(34), h.min(1024) as i64)
}

/// Texel bytes written to the GPU's staging since its last flush (`upload`).
static STAGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The staging is submitted and waited for once this many texel bytes are written, so that a level's hundreds of replacements
/// are never staged at once (level 1's 485, staged before one submit, kept about 600 MB of upload memory through the level).
const STAGING_FLUSH: u64 = 64 << 20;

/// A texture of `levels` (each its width, height and RGBA8 rows) on the GPU.
///
/// With `band` bytes a write, each level is written in bands of rows (the background worker's: one write of a 4096 x 4096 level
/// holds the GPU queue's staging for about 10 ms, and the render thread's own writes wait on it, so the worker writes in bands the
/// render thread's writes fit between).
fn upload(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, levels: &[(u32, u32, &[u8])], band: usize) -> Result<wgpu::TextureView, String> {
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: levels[0].0, height: levels[0].1, depth_or_array_layers: 1 },
        mip_level_count: levels.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (l, &(w, h, px)) in levels.iter().enumerate() {
        let rows = (band / (4 * w as usize)).clamp(1, h as usize) as u32;
        for y in (0..h).step_by(rows as usize) {
            let n = rows.min(h - y);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: l as u32, origin: wgpu::Origin3d { x: 0, y, z: 0 }, aspect: wgpu::TextureAspect::All },
                &px[(y * w * 4) as usize..((y + n) * w * 4) as usize],
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(n) },
                wgpu::Extent3d { width: w, height: n, depth_or_array_layers: 1 },
            );
            if rows < h {
                std::thread::sleep(std::time::Duration::from_micros(100));
            }
        }
    }
    let bytes: u64 = levels.iter().map(|l| l.2.len() as u64).sum();
    if STAGED.fetch_add(bytes, std::sync::atomic::Ordering::Relaxed) + bytes >= STAGING_FLUSH {
        STAGED.store(0, std::sync::atomic::Ordering::Relaxed);
        queue.submit([]);
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("texnames: GPU wait: {e}"))?;
    }
    Ok(tex.create_view(&Default::default()))
}

/// A one-level texture on the GPU as a replacement is (`surfaces`' id textures).
pub(crate) fn upload_level0(device: &wgpu::Device, queue: &wgpu::Queue, label: &str, w: u32, h: u32, rgba: Vec<u8>) -> Result<HdView, String> {
    let min_alpha = rgba.chunks_exact(4).map(|p| p[3]).min().ok_or("texnames: an empty texture")?;
    Ok(HdView { view: upload(device, queue, label, &[(w, h, &rgba[..])], usize::MAX)?, levels: 1, min_alpha })
}

/// A decoded replacement on the GPU.
fn to_gpu(device: &wgpu::Device, queue: &wgpu::Queue, r: &Repl, band: usize) -> Result<HdView, String> {
    let levels: Vec<(u32, u32, &[u8])> = r.levels.iter().map(|l| (l.w, l.h, &l.rgba[..])).collect();
    Ok(HdView { view: upload(device, queue, "pack texture", &levels, band)?, levels: levels.len(), min_alpha: r.min_alpha })
}

/// `t`'s replacement on the GPU, with its level count, when there is one (`hd`).
pub(crate) fn hd_view(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> Result<Option<(wgpu::TextureView, usize)>, String> {
    Ok(hd(device, queue, t)?.map(|h| (h.view, h.levels)))
}

/// The sampler of a texture sampled at the level its texel density on screen picks (a pack replacement, or any with the
/// texture filtering option): trilinear, `aniso` times anisotropic (16 for a replacement), `cs` / `ct` clamping each axis, level
/// 0 alone when `one_level`.
pub(crate) fn hd_sampler(device: &wgpu::Device, cs: bool, ct: bool, one_level: bool, aniso: u16) -> wgpu::Sampler {
    let mode = |c: bool| if c { wgpu::AddressMode::ClampToEdge } else { wgpu::AddressMode::Repeat };
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("pack texture"),
        address_mode_u: mode(cs),
        address_mode_v: mode(ct),
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: aniso,
        lod_max_clamp: if one_level { 0.0 } else { 32.0 },
        ..Default::default()
    })
}

/// `uya texnames <disc.iso> <level>[,<level>...] <dumps dir> [--write]`: names every texture of each level's native scene and
/// counts, per kind, the names a PCSX2 dump folder (the emulated GS's `--dump`) holds a file for; with `write`, dumps the others
/// there as the emulated GS would (the originals texture-select.py judges pack files against).
pub fn report(iso: &str, levels: &[u32], dumps: &str, write: bool) -> Result<(), String> {
    let dumped: HashSet<String> = std::fs::read_dir(dumps)
        .map_err(|e| format!("{dumps}: {e}"))?
        .filter_map(|e| e.ok()?.file_name().into_string().ok())
        .filter_map(|n| n.strip_suffix(".png").map(str::to_string))
        .collect();
    let (mut written, mut kept) = (0, 0);
    for &level in levels {
        println!("level {level}");
        let scene = crate::native::Scene::decode(iso, level)?;
        for (kind, textures) in scene.texture_sets() {
            let (mut named, mut hit, mut none) = (0, 0, 0);
            let mut misses = Vec::new();
            for t in &textures {
                match name(t)? {
                    Some(n) => {
                        named += 1;
                        let stem = n.stem();
                        if write {
                            if dump(Path::new(dumps), t, &stem)? {
                                written += 1;
                            } else {
                                kept += 1;
                            }
                        }
                        if dumped.contains(&stem) {
                            hit += 1;
                        } else {
                            misses.push(stem);
                        }
                    }
                    None => none += 1,
                }
            }
            println!("{kind}: {} textures, {named} named ({none} not), {hit} of them dumped", textures.len());
            for m in misses.iter().take(3) {
                println!("  not dumped: {m}");
            }
        }
    }
    if write {
        println!("{written} dumps written, {kept} there already");
    }
    Ok(())
}
