//! scenecheck DISC.iso RAM.bin LEVEL: compares the decoded meshes of a level's SCENE block and hero body (`uyafmt::scene::load`,
//! `uyafmt::scenecode::hero_body`) with the game's RAM on a frame, for every program 0 run of the frame's moby chain that is neither
//! a core class nor the hero's armor or the wrench (the runs of `captures/agent-scene/noncore.py`; Starship Phoenix, level 3,
//! captures/dbg1/ram/ram04800.bin).
//!
//! The runs (as armorcheck): the chain from the record list's head variable, every block that sends a strip list names its packet A's
//! RAM address. A run is the core's when the address lies in `[core, core + 0x1400000)`, the armor's or the wrench's from the lower of the
//! two buffers the variables `armor_buffer` and `wrench_block` hold upward (the armor blocks run past 0x8000 bytes).
//!
//! The sources (`uyafmt::scenecode`): the SCENE block is the record `[S + 0x30]` of the level's SCENE file, its RAM base `[S + 0x58]`;
//! class `c`, mesh `m`: packet A at `[S + 0x58] + c.offset + m.a_at`. The hero body is WAD 4's pair `body_pair + [0x16C598]`, at
//! `[S2 + 0x70]`; its mesh's packet A is at that plus `m.a_at`.
//!
//! Per run, equal means (1) the packet A decoded from the RAM bytes (`decode_packet_a`) has the same cull byte, strips, texture
//! coordinates and setup word 2 as the decoded mesh, (2) every byte of the packet except words 0, 1, 4, 5, 8, 9, 12, 13 of each
//! 64-byte texture setup (rewritten by func_L01_00423700 / func_L01_004239F8) is the same on the disc and in RAM, (3) each setup's
//! rewritten TEX0 (words 8 and 9) in RAM names the mapped image's size: PSM 0x13, TW and TH = log2 of the image's width and height
//! (func_L01_00423700 computes them from the 16-byte texture record the loader wrote from the image header at +8 and +0xC); a
//! setup of the hero body also has the TEX0 the variable `body_tex0` holds. Setups with a negative number are counted, not mapped.
//!
//! What it would print if the decoder or a base were wrong: a wrong base or wrong block (another record `k`, another variant) finds
//! no mesh at the run's address ("no mesh"), or a mesh of another packet whose strips and bytes differ; a wrong class offset or
//! `a_at` finds no mesh; a wrong strip, coordinate or setup decode makes (1) differ; a block that is not the one in RAM makes (2)
//! differ in the first bytes; a wrong texture mapping (the wrong image, so another size) makes (3) differ.

use uyafmt::core::Texture;
use uyafmt::disc::{Disc, u32_at};
use uyafmt::levelcode;
use uyafmt::moby::{self, MobyClass};
use uyafmt::scene;
use uyafmt::scenecode;
use uyafmt::toc::{self, FileKind};

fn rd(ram: &[u8], a: usize, n: usize) -> Result<&[u8], String> {
    ram.get(a..a + n).ok_or_else(|| format!("RAM read {a:#x}+{n:#x} past the dump"))
}

fn rd32(ram: &[u8], a: usize) -> Result<u32, String> {
    u32_at(rd(ram, a, 4)?, 0)
}

fn rd64(ram: &[u8], a: usize) -> Result<u64, String> {
    Ok(u64::from_le_bytes(rd(ram, a, 8)?.try_into().expect("8 bytes")))
}

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

enum Block {
    StripList,
    Mpg,
    Other,
}

fn classify(words: &[u32]) -> Block {
    let mut i = 0;
    while i < words.len() {
        let code = words[i];
        let (cmd, imm, num) = ((code >> 24) & 0x7f, code & 0xffff, ((code >> 16) & 0xff) as usize);
        i += 1;
        match cmd {
            0x20 => i += 1,
            0x30 | 0x31 => i += 4,
            0x50 | 0x51 => i = 2 + i.saturating_sub(2).div_ceil(4) * 4 + 4 * if imm == 0 { 0x10000 } else { imm as usize },
            0x4a => return Block::Mpg,
            0x60..=0x7f => {
                if (cmd & 0x6f, imm & 0x3ff) == (0x6e, 0x12d) {
                    return Block::StripList;
                }
                i += unpack_words(cmd, num);
            }
            _ => {}
        }
    }
    Block::Other
}

fn packet_addresses(ram: &[u8], jobs: usize) -> Result<Vec<usize>, String> {
    let head = rd32(ram, jobs)? as usize;
    let first_output = rd32(ram, head + 0xC4)? as usize;
    let start = first_output - 0x150;
    if rd32(ram, start)? != 0x1000_0006 {
        return Err(format!("no moby chain at {start:#x}"));
    }
    let mut out = Vec::new();
    let mut at = start;
    for _ in 0..100_000 {
        let tag = rd32(ram, at)?;
        let (id, qwc) = ((tag >> 28) & 7, (tag & 0xffff) as usize);
        let (data_at, next) = match id {
            1 => (at + 16, at + 16 + 16 * qwc),
            3 | 0 => (rd32(ram, at + 4)? as usize, at + 16),
            2 => (at + 16, rd32(ram, at + 4)? as usize),
            7 => (at + 16, 0),
            _ => return Err(format!("chain tag {tag:#x} at {at:#x}")),
        };
        let mut words = vec![rd32(ram, at + 8)?, rd32(ram, at + 12)?];
        for k in 0..4 * qwc {
            words.push(rd32(ram, data_at + 4 * k)?);
        }
        match classify(&words) {
            Block::StripList => out.push(data_at),
            Block::Mpg => return Ok(out),
            Block::Other => {}
        }
        if id == 7 || id == 0 {
            return Ok(out);
        }
        at = next;
    }
    Err("the moby chain has no end".to_string())
}

/// Where one source's class sits: its bytes as the disc has them (from the class's start on), its RAM address, its decoded class and
/// the texture mapping.
struct Src<'a> {
    name: String,
    /// The class's bytes: the class's offset in `bytes`.
    bytes: &'a [u8],
    at: usize,
    ram: usize,
    class: &'a MobyClass,
    /// The image of a setup's number.
    image: Box<dyn Fn(i32) -> Result<&'a Texture, String> + 'a>,
    /// A TEX0 every setup with a non-negative number must have (the hero body's, class 10's) and the PSM it is uploaded as.
    tex0: Option<u64>,
    psm: u64,
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() == 2 {
        return all_levels(&args[1]);
    }
    if args.len() != 4 {
        return Err("usage: scenecheck DISC.iso RAM.bin LEVEL | scenecheck DISC.iso".to_string());
    }
    let level: usize = args[3].parse().map_err(|e| format!("level: {e}"))?;
    let ram = std::fs::read(&args[2]).map_err(|e| format!("{}: {e}", args[2]))?;
    let lg = levelcode::globals(&args[1], level)?;
    let sg = scenecode::globals(&args[1], level)?;
    println!("{sg:x?}");
    let mut d = Disc::open(&args[1])?;
    let tc = toc::read(&mut d)?;

    // The variables, in RAM.
    let k = rd32(&ram, sg.scene_index as usize)? as usize;
    let scene_base = rd32(&ram, sg.scene_base as usize)? as usize;
    let body_base = rd32(&ram, sg.body_base as usize)? as usize;
    let variant = rd32(&ram, sg.body_variant as usize)? as usize;
    let tex_n = rd32(&ram, sg.body_texture as usize)? as usize;
    let body_tex0 = rd64(&ram, sg.body_tex0 as usize)?;
    println!("scene record {k} at {scene_base:#x}; hero body variant {variant} texture {tex_n} at {body_base:#x}, TEX0 {body_tex0:#x}");

    let file = tc.files.iter().find(|f| f.level == level && f.kind == FileKind::Scene).ok_or_else(|| format!("no SCENE file for level {level}"))?;
    let loaded = scene::load(&mut d, file, k)?.ok_or_else(|| format!("scene record {k} has no block"))?;
    for c in &loaded.block.classes {
        println!("  scene class id {} at +{:#x}: {} meshes, {} images", c.id, c.offset, c.class.meshes.len(), c.images.len());
    }
    let c10_n = rd32(&ram, sg.class10_texture as usize)? as usize;
    let c10_tex0 = rd64(&ram, sg.class10_tex0 as usize)?;
    let c10_image = scenecode::class10_image(&mut d, &tc, &sg, c10_n)?;
    println!("  class 10 texture {c10_n}, TEX0 {c10_tex0:#x}, image {} x {}", c10_image.width, c10_image.height);
    let body = scenecode::hero_body(&mut d, &tc, &sg, variant, tex_n)?;
    println!("  hero body: {} meshes, image {} x {}", body.class.meshes.len(), body.image.width, body.image.height);

    // Every setup number of the three body variants and of the scene's class 10 (the remap table at gp - 0x5BA0 is in the level's
    // .bss: its bytes are not read at run time here, so the numbers must all be 0).
    let mut locals = std::collections::BTreeSet::new();
    for v in 0..3 {
        let b = scenecode::hero_body(&mut d, &tc, &sg, v, tex_n)?;
        for m in &b.class.meshes {
            for t in m.packet_a.iter().flat_map(|p| &p.textures) {
                locals.insert(t.local);
            }
        }
    }
    println!("  setup numbers of the 3 hero body variants: {locals:?}");
    let mut srcs: Vec<Src> = Vec::new();
    let (cg, ci) = (&sg, &c10_image);
    for c in &loaded.block.classes {
        let ten = c.id == 10;
        srcs.push(Src {
            name: format!("scene class {}", c.id),
            bytes: &loaded.bytes,
            at: c.offset,
            ram: scene_base + c.offset,
            class: &c.class,
            image: if ten {
                Box::new(move |l| if cg.body_remap.get(l as usize) == Some(&0) { Ok(ci) } else { Err(format!("class 10: texture number {l}")) })
            } else {
                Box::new(move |l| c.image(l))
            },
            tex0: if ten { Some(c10_tex0) } else { None },
            psm: if ten { 0x1b } else { 0x13 },
        });
    }
    let (bg, bb) = (&sg, &body);
    srcs.push(Src { name: "hero body".to_string(), bytes: &body.bytes, at: 0, ram: body_base, class: &body.class, image: Box::new(move |l| bb.image(bg, l)), tex0: Some(body_tex0), psm: 0x1b });

    let addrs = packet_addresses(&ram, lg.moby_jobs as usize)?;
    let core = rd32(&ram, lg.core as usize)? as usize;
    let (wb, hb) = (rd32(&ram, lg.wrench_block as usize)? as usize, rd32(&ram, lg.armor_buffer as usize)? as usize);
    let alo = wb.min(hb);
    println!("chain: {} program 0 runs, core at {core:#x}, armor and above from {alo:#x}", addrs.len());

    let (mut equal, mut different, mut total, mut negative) = (0, 0, 0, 0);
    for &addr in &addrs {
        if (core..core + 0x140_0000).contains(&addr) || addr >= alo {
            continue;
        }
        total += 1;
        let found = srcs.iter().find_map(|s| s.class.meshes.iter().position(|m| m.packet_a.is_some() && s.ram + m.a_at == addr).map(|i| (s, i)));
        let Some((s, mi)) = found else {
            println!("{addr:#09x}  no mesh (no source's base + a_at)");
            different += 1;
            continue;
        };
        let mesh = &s.class.meshes[mi];
        let dec = mesh.packet_a.as_ref().expect("selected for packet_a");
        let rec = u32_at(&s.bytes[s.at..], 0)? as usize + 16 * mesh.record;
        let w1 = u32_at(&s.bytes[s.at..], rec + 4)? as usize;
        let (size, tail) = (16 * (w1 & 0xff), w1 >> 16);
        let disc_p = &s.bytes[s.at + mesh.a_at..s.at + mesh.a_at + size];
        let ram_p = rd(&ram, addr, size)?;
        let mut why: Vec<String> = Vec::new();
        match moby::decode_packet_a(ram_p, tail) {
            Err(e) => why.push(format!("RAM packet does not decode: {e}")),
            Ok(r) => {
                if r.cull != dec.cull || r.prepass_count != dec.prepass_count {
                    why.push("cull byte".to_string());
                }
                if format!("{:?}", r.strips) != format!("{:?}", dec.strips) {
                    why.push("strips".to_string());
                }
                if format!("{:?}", r.uvs) != format!("{:?}", dec.uvs) {
                    why.push("texture coordinates".to_string());
                }
                if r.textures.len() != dec.textures.len() || r.textures.iter().zip(&dec.textures).any(|(a, b)| a.a_count != b.a_count) {
                    why.push("texture setups".to_string());
                }
            }
        }
        let setups = size - 16 * tail;
        let mut bad = 0;
        for o in 0..size {
            let rewritten = o >= setups && matches!(((o - setups) % 64) / 4, 0 | 1 | 4 | 5 | 8 | 9 | 12 | 13);
            if !rewritten && disc_p[o] != ram_p[o] {
                bad += 1;
            }
        }
        if bad != 0 {
            why.push(format!("{bad} bytes differ"));
        }
        for (j, t) in dec.textures.iter().enumerate() {
            if t.local < 0 {
                negative += 1;
                continue;
            }
            let tex0 = rd64(&ram, addr + setups + 64 * j + 0x20)?;
            let (psm, tw, th) = ((tex0 >> 20) & 0x3f, (tex0 >> 26) & 0xf, (tex0 >> 30) & 0xf);
            match (s.image)(t.local) {
                Err(e) => why.push(format!("setup {j}: {e}")),
                Ok(img) => {
                    if psm != s.psm || 1usize << tw != img.width || 1usize << th != img.height {
                        why.push(format!("setup {j}: local {} TEX0 {tex0:#x} is PSM {psm:#x} {} x {}, the image is {} x {}", t.local, 1u32 << tw, 1u32 << th, img.width, img.height));
                    }
                }
            }
            if let Some(want) = s.tex0 {
                if tex0 != want {
                    why.push(format!("setup {j}: TEX0 {tex0:#x}, the body's is {want:#x}"));
                }
            }
        }
        if why.is_empty() {
            equal += 1;
            println!("{addr:#09x}  {} mesh {mi}  equal ({} strips, {} setups)", s.name, dec.strips.len(), dec.textures.len());
        } else {
            different += 1;
            println!("{addr:#09x}  {} mesh {mi}  different: {}", s.name, why.join("; "));
        }
    }
    println!("total: {total} runs outside the core and the armor, {equal} equal, {different} different ({negative} setups with a negative number not mapped)");
    if different != 0 || total == 0 {
        return Err("scenecheck failed".to_string());
    }
    Ok(())
}

/// The 34 story levels: the variables read from each level's code, and every SCENE record's block decoded: its classes, meshes and
/// images (class 10's image comes from WAD 5, not from the block).
fn all_levels(iso: &str) -> Result<(), String> {
    let mut d = Disc::open(iso)?;
    let tc = toc::read(&mut d)?;
    let all = scenecode::all(iso)?;
    let reference = all.iter().find(|g| g.level == 1).ok_or("no level 1")?;
    let (mut blocks, mut classes, mut meshes, mut images) = (0, 0, 0, 0);
    for g in &all {
        // The offsets within the state are the same in every level; the addresses are the level's own.
        let same = (g.scene_buffer - g.scene_index, g.scene_base - g.scene_index, g.body_pair, g.texture_pair, g.class10_pair)
            == (reference.scene_buffer - reference.scene_index, reference.scene_base - reference.scene_index, reference.body_pair, reference.texture_pair, reference.class10_pair);
        println!("L{:02} S+0x30 {:#x} S+0x58 {:#x} S+0x68 {:#x} body {:#x} variant {:#x} texture {:#x} tex0 {:#x} slots {:#x} c10 {:#x} {:#x} remap {:02x?} pairs {} {} {} same-shape {same}", g.level, g.scene_index, g.scene_base, g.scene_buffer, g.body_base, g.body_variant, g.body_texture, g.body_tex0, g.slots, g.class10_texture, g.class10_tex0, g.body_remap, g.body_pair, g.texture_pair, g.class10_pair);
        if !same {
            return Err(format!("L{:02}: the scene variables are not laid out as level 1's", g.level));
        }
        let file = tc.files.iter().find(|f| f.level == g.level && f.kind == FileKind::Scene).ok_or_else(|| format!("no SCENE file for level {}", g.level))?;
        for k in 0..30 {
            let Some(b) = scene::load(&mut d, file, k)? else { continue };
            blocks += 1;
            let ids: Vec<u32> = b.block.classes.iter().map(|c| c.id).collect();
            classes += b.block.classes.len();
            let m: usize = b.block.classes.iter().map(|c| c.class.meshes.len()).sum();
            let i: usize = b.block.classes.iter().map(|c| c.images.len()).sum();
            // Every setup number of a class names an image of its list (class 10: slot 0; negative: a special set).
            for c in &b.block.classes {
                for t in c.class.meshes.iter().flat_map(|m| m.packet_a.iter()).flat_map(|p| &p.textures) {
                    if t.local >= 0 && c.id != 10 {
                        c.image(t.local)?;
                    }
                }
            }
            meshes += m;
            images += i;
            println!("  record {k}: {} bytes, classes {ids:?}, {m} meshes, {i} images", b.bytes.len());
        }
    }
    println!("{} levels, {blocks} blocks, {classes} classes, {meshes} meshes, {images} images", all.len());
    Ok(())
}
