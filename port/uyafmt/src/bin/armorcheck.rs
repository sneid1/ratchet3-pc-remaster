//! armorcheck DISC.iso RAM.bin: compares `uyafmt::moby::armor`'s decoded armor model blocks with the game's RAM on the traced Veldin
//! frame (field 2400, captures/trace/ram02400.bin).
//!
//! The runs: the frame's moby DMA chain (the head variable `moby_jobs`, the first record's first output at head + 0xC4, the chain
//! 0x150 bytes before it; `port/uya/src/mobies.rs` explains it) is walked and every block that sends a strip list (UNPACK 0x6E to
//! TOPS + 0x12D: packet A of a program 0 run) names its packet A's RAM address. Those outside the core (the core's base is the
//! variable `core`) are the armor runs.
//!
//! The blocks: the hero's armor is the WAD5 entry skin_worn (byte 0x1427AB; func_510800) when it is not zero, else armor_worn (s16
//! 0x1426A0), and func_L01_00422838 reads its mesh block into the buffer the variable armor_buffer (0x1D4B64) holds; the wrench is
//! entry 31 (func_L01_00422D70), in the buffer the variable at 0x1EAA34 holds. The RAM base of a block is that buffer's address.
//!
//! Per run: the mesh is the block's mesh whose `base + a_at` is the run's address. Equal means (1) the packet A decoded from the RAM
//! bytes (`decode_packet_a`) has the same cull byte, strips (texture group, vertex indices, ADC bits), texture coordinates and
//! texture setups' word 2 (low half) as the mesh the disc block decoded to, (2) every byte of the packet that the load does not
//! rewrite is the same on the disc and in RAM (the rewritten words are 0, 1, 4, 5, 8, 9, 12 and 13 of each 64-byte texture setup),
//! and (3) each setup's rewritten TEX0 (words 8 and 9) in RAM is the TEX0 of the texture slot the decoded setup's word 8 names
//! (func_L01_00423E10 maps it through the 16 bytes at gp - 0x5B90, then func_L01_004239F8 stores the slot's TEX0: the array at
//! 0x1D4B70 for the hero, the word at 0x1D4B98 for the wrench).
//!
//! What it would print if the decoder or the base were wrong: a wrong base (or a wrong entry) finds no mesh at the run's address
//! ("no mesh"), or a mesh of another packet whose strips and bytes differ; a wrong strip, coordinate or setup decode makes (1) differ;
//! a block that is not the one in RAM makes (2) differ in the first bytes; a wrong texture mapping makes (3) differ.

use uyafmt::disc::{Disc, SECTOR, u32_at};
use uyafmt::levelcode;
use uyafmt::moby::{self, Armor};
use uyafmt::toc;
use uyafmt::track::{Space, Tracker};

fn rd(ram: &[u8], a: usize, n: usize) -> Result<&[u8], String> {
    ram.get(a..a + n).ok_or_else(|| format!("RAM read {a:#x}+{n:#x} past the dump"))
}

fn rd32(ram: &[u8], a: usize) -> Result<u32, String> {
    u32_at(rd(ram, a, 4)?, 0)
}

fn rd64(ram: &[u8], a: usize) -> Result<u64, String> {
    let b = rd(ram, a, 8)?;
    Ok(u64::from_le_bytes(b.try_into().expect("8 bytes")))
}

/// The size in 32-bit words of the data of UNPACK `cmd` of `num` elements.
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

/// What a block's VIF stream (the tag's two VIF words, then its data) holds: an UNPACK to the strip list's address 0x12D, the MPG of
/// the next pass's program, or neither.
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

/// The RAM addresses of packet A of every program 0 run of the frame's chain, in order.
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

struct Loaded {
    name: String,
    entry: usize,
    base: usize,
    len: usize,
    bytes: Vec<u8>,
    armor: Armor,
}

fn load(d: &mut Disc, w: &toc::WadHeader, name: &str, entry: usize, base: usize) -> Result<Loaded, String> {
    let (ms, mc) = w.pairs[2 * entry];
    let bytes = d.sectors(w.sector + ms as u64, mc as u64)?;
    let mut t = Tracker::new();
    let mut root = t.root("check", "armor block", bytes.len() as u64, Space::Disc);
    let armor = moby::armor(&mut t, &mut root, "check", 0, &bytes)?.ok_or_else(|| format!("WAD5 entry {entry} is a class-form record"))?;
    Ok(Loaded { name: format!("{name} (entry {entry})"), entry, base, len: bytes.len(), bytes, armor })
}

fn main() -> Result<(), String> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        return Err("usage: armorcheck DISC.iso RAM.bin".to_string());
    }
    let ram = std::fs::read(&args[2]).map_err(|e| format!("{}: {e}", args[2]))?;
    let g = levelcode::globals(&args[1], 1)?;
    let mut d = Disc::open(&args[1])?;
    let tc = toc::read(&mut d)?;
    let w = &tc.wads[4];
    let _ = SECTOR;

    // Veldin's variables (level 1's func_L01_00422838, func_L01_00422D70, func_L01_00422BD0 and func_510800 read them).
    let skin = rd(&ram, 0x1427AB, 1)?[0] as usize;
    let armor_worn = u16::from_le_bytes(rd(&ram, 0x1426A0, 2)?.try_into().expect("2 bytes")) as usize;
    let hero_entry = if skin > 0 { skin } else { armor_worn };
    let hero_base = rd32(&ram, 0x1D4B64)? as usize;
    let wrench_base = rd32(&ram, 0x1EAA34)? as usize;
    let blocks = [load(&mut d, w, "hero armor", hero_entry, hero_base)?, load(&mut d, w, "wrench", 31, wrench_base)?];
    for b in &blocks {
        println!("block {}: RAM {:#x} to {:#x}, {} meshes (counts {:?}), {} texture-id records", b.name, b.base, b.base + b.len, b.armor.meshes.len(), b.armor.counts, b.armor.texture_ids.len());
    }

    let addrs = packet_addresses(&ram, g.moby_jobs as usize)?;
    let core = rd32(&ram, g.core as usize)? as usize;
    println!("chain: {} program 0 runs, core at {core:#x}", addrs.len());
    let tex0_hero = 0x1D4B70usize;
    let tex0_wrench = 0x1D4B98usize;
    let remap = 0x1D6D20usize; // gp (0x1DC8B0, func_0013ADA8) - 0x5B90, as func_L01_00422AB8 and func_L01_00422F28 pass it
    let (mut equal, mut different, mut total) = (0, 0, 0);
    for &addr in &addrs {
        // The core's classes are the other runs (core + the class record's offset, at most the core's size).
        if addr < 0x1F00000 {
            continue;
        }
        total += 1;
        let Some((blk, mi)) = blocks.iter().find_map(|b| b.armor.meshes.iter().position(|m| m.packet_a.is_some() && b.base + m.a_at == addr).map(|i| (b, i))) else {
            println!("{addr:#09x}  no mesh (no block's base + a_at)");
            different += 1;
            continue;
        };
        let mesh = &blk.armor.meshes[mi];
        let dec = mesh.packet_a.as_ref().expect("selected for packet_a");
        let rec = 16 + 16 * mi;
        let (w1, _) = (u32_at(&blk.bytes, rec + 4)? as usize, 0);
        let (size, tail) = (16 * (w1 & 0xff), w1 >> 16);
        let disc_p = &blk.bytes[mesh.a_at..mesh.a_at + size];
        let ram_p = rd(&ram, addr, size)?;
        let mut why: Vec<String> = Vec::new();
        // (1) the decode of RAM against the decode of the disc block.
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
        // (2) the bytes the load does not rewrite.
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
        // (3) the texture mapping.
        for (k, s) in dec.textures.iter().enumerate() {
            // A negative number is not remapped (func_L01_00423E10 0x423E84 `bltz`); func_L01_004239F8 (0x423A98 to 0x423AB4) takes the
            // TEX0 of -2 from the 8 bytes at 0x1DFF10 and of -3 from 0x1DFF28 (-1 stores a constant TEX1/CLAMP set: not in this frame).
            let slot = if s.local < 0 { None } else { Some(rd(&ram, remap + s.local as usize, 1)?[0] as usize) };
            let want = match (s.local, slot, blk.name.starts_with("wrench")) {
                (-2, _, _) => Some(rd64(&ram, 0x1DFF10)?),
                (-3, _, _) => Some(rd64(&ram, 0x1DFF28)?),
                (_, Some(0), true) => Some(rd64(&ram, tex0_wrench)?),
                (_, Some(s), false) => Some(rd64(&ram, tex0_hero + 8 * s)?),
                _ => None,
            };
            let got = rd64(&ram, addr + setups + 64 * k + 0x20)?;
            if want != Some(got) {
                why.push(format!("setup {k}: local {} slot {slot:?} TEX0 {got:#x}, expected {want:x?}", s.local));
            }
        }
        if why.is_empty() {
            equal += 1;
            println!("{addr:#09x}  {} mesh {mi}  equal ({} strips, {} setups)", blk.name, dec.strips.len(), dec.textures.len());
        } else {
            different += 1;
            println!("{addr:#09x}  {} mesh {mi}  different: {}", blk.name, why.join("; "));
        }
        let _ = blk.entry;
    }
    println!("total: {total} non-core runs, {equal} equal, {different} different");
    if different != 0 || total == 0 {
        return Err("armorcheck failed".to_string());
    }
    Ok(())
}
