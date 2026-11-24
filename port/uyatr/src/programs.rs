//! The programs whose code is translated, read from the player's disc through `uyafmt`: the loader the disc boots
//! (`SCUS_973.53`, rac3-decomp's i5bootn, an ELF at 0x800000), the main program it unpacks (rac3-decomp's `boot_elf`: the resident
//! core, `core.text`, and its own front end, `.text` from 0x381180), frontbin (the front end the game reloads after Quit Game,
//! WAD2 pair 3) and the story levels' overlays (each LEVEL file's block 0, `uyafmt::levelcode::overlay`).
//!
//! Function bounds come from rac3-decomp's configuration through `scripts/uya_bounds.py` (`entry end level` lines). The loader
//! has none for this disc (rac3-decomp's i5bootn table is the PAL file's, which differs), so its functions are found from its
//! entry point and every `jal` target in its code.

use uyafmt::disc::{Disc, SECTOR, u32_at};

pub struct Program {
    /// Short name, used for the crate and in the runtime's messages: `loader`, `boot`, `fe100`, `fe101`, `l01` ...
    pub name: String,
    /// The image the code and its data are read from: (address, bytes) records as the program lies in RAM.
    pub records: Vec<(u32, Vec<u8>)>,
    /// The code the runtime checks is in RAM before using the translation, [lo, hi).
    pub text: (u32, u32),
    /// Function bounds [entry, end).
    pub funcs: Vec<(u32, u32)>,
    /// The program's own entry point (the loader's ELF entry; 0 for the others).
    pub entry: u32,
}

impl Program {
    pub fn word(&self, a: u32) -> Option<u32> {
        for (base, b) in &self.records {
            if a >= *base && ((a - *base) as usize) + 4 <= b.len() {
                let o = (a - *base) as usize;
                return Some(u32::from_le_bytes(b[o..o + 4].try_into().expect("4 bytes")));
            }
        }
        None
    }
}

/// `entry end level` lines from scripts/uya_bounds.py.
pub fn bounds(path: &str, level: &str) -> Result<Vec<(u32, u32)>, String> {
    let t = std::fs::read_to_string(path).map_err(|e| format!("{path}: {e}"))?;
    let mut v = Vec::new();
    for line in t.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let p: Vec<&str> = line.split('\t').collect();
        if p.len() != 3 {
            return Err(format!("{path}: bad line {line:?}"));
        }
        if p[2] == level {
            let a = u32::from_str_radix(p[0], 16).map_err(|e| format!("{path}: {line:?}: {e}"))?;
            let b = u32::from_str_radix(p[1], 16).map_err(|e| format!("{path}: {line:?}: {e}"))?;
            v.push((a, b));
        }
    }
    if v.is_empty() {
        return Err(format!("{path}: no functions of {level}"));
    }
    v.sort();
    Ok(v)
}

fn span(funcs: &[(u32, u32)]) -> (u32, u32) {
    (funcs.first().expect("functions").0, funcs.iter().map(|f| f.1).max().expect("functions"))
}

/// The loader: SCUS_973.53's PT_LOAD segments; functions from the entry and the `jal` targets in its executable segment.
pub fn loader(iso: &str) -> Result<Program, String> {
    let mut d = Disc::open(iso)?;
    let v = uyafmt::iso::volume(&mut d)?;
    let f = v.files.iter().find(|f| f.path == "/SCUS_973.53").ok_or("no /SCUS_973.53 on the disc")?;
    let elf = d.bytes_at(f.sector * SECTOR, f.size)?;
    if elf.get(..4) != Some(&b"\x7fELF"[..]) {
        return Err("SCUS_973.53 is not an ELF".into());
    }
    let entry = u32_at(&elf, 0x18)?;
    let (phoff, phnum) = (u32_at(&elf, 0x1c)? as usize, u16::from_le_bytes([elf[0x2c], elf[0x2d]]) as usize);
    let mut records = Vec::new();
    let mut text = None;
    for k in 0..phnum {
        let p = phoff + 32 * k;
        let (ty, off, vaddr, filesz, memsz, flags) = (u32_at(&elf, p)?, u32_at(&elf, p + 4)? as usize, u32_at(&elf, p + 8)?, u32_at(&elf, p + 16)? as usize, u32_at(&elf, p + 20)? as usize, u32_at(&elf, p + 24)?);
        if ty != 1 {
            continue;
        }
        let mut b = elf.get(off..off + filesz).ok_or("SCUS_973.53: a segment past the file's end")?.to_vec();
        b.resize(memsz, 0);
        if flags & 1 != 0 {
            text = Some((vaddr, vaddr + filesz as u32));
        }
        records.push((vaddr, b));
    }
    let _ = text.ok_or("SCUS_973.53: no executable segment")?;
    // The code is the executable section (the segment holds the data after it, the main program's packed image among it).
    let (shoff, shentsize, shnum) = (u32_at(&elf, 0x20)? as usize, u16::from_le_bytes([elf[0x2e], elf[0x2f]]) as usize, u16::from_le_bytes([elf[0x30], elf[0x31]]) as usize);
    let mut text = None;
    for k in 0..shnum {
        let s = shoff + shentsize * k;
        let (flags, addr, size) = (u32_at(&elf, s + 8)?, u32_at(&elf, s + 12)?, u32_at(&elf, s + 20)?);
        if flags & 4 != 0 && size > 0 {
            if text.is_some() {
                return Err("SCUS_973.53: more than one executable section".into());
            }
            text = Some((addr, addr + size));
        }
    }
    let text = text.ok_or("SCUS_973.53: no executable section")?;
    let mut p = Program { name: "loader".into(), records, text, funcs: Vec::new(), entry };
    let mut starts = std::collections::BTreeSet::new();
    starts.insert(text.0);
    starts.insert(entry);
    for a in (text.0..text.1).step_by(4) {
        let w = p.word(a).ok_or("loader text outside its records")?;
        let i = crate::decode::decode(w);
        if i.m == crate::decode::M::Jal {
            let t = i.jump_target(a);
            if t >= text.0 && t < text.1 {
                starts.insert(t);
            }
        }
    }
    let s: Vec<u32> = starts.into_iter().collect();
    p.funcs = s.iter().enumerate().map(|(k, &a)| (a, s.get(k + 1).copied().unwrap_or(text.1))).collect();
    Ok(p)
}

/// The main program as the loader lays it out from 0x100000, with the functions of `level` ("boot" or "100").
pub fn main_program(iso: &str, bounds_path: &str, level: &str, name: &str) -> Result<Program, String> {
    let b = uyafmt::boot::boot(iso)?;
    let funcs = bounds(bounds_path, level)?;
    let text = span(&funcs);
    Ok(Program { name: name.into(), records: vec![(uyafmt::boot::BASE, b.ram)], text, funcs, entry: 0 })
}

/// Records {address, size, kind, entry} + bytes, until the entry word changes (the overlay loader's rule).
fn records(b: &[u8], what: &str) -> Result<Vec<(u32, Vec<u8>)>, String> {
    let mut out = Vec::new();
    let mut p = 0usize;
    let mut entry = None;
    while p + 16 <= b.len() {
        let (address, size, e) = (u32_at(b, p)?, u32_at(b, p + 4)? as usize, u32_at(b, p + 12)?);
        if *entry.get_or_insert(e) != e {
            break;
        }
        let data = b.get(p + 16..p + 16 + size).ok_or_else(|| format!("{what}: record at {p:#x} runs past the end"))?;
        out.push((address, data.to_vec()));
        p += 16 + size;
    }
    if out.is_empty() {
        return Err(format!("{what}: no records"));
    }
    Ok(out)
}

/// frontbin: WAD2 pair 3's records (uyafmt::wads: "Pair 3: frontbin, an EE program (records), loaded by func_L01_00428008").
pub fn frontbin(iso: &str, bounds_path: &str) -> Result<Program, String> {
    let mut d = Disc::open(iso)?;
    let tc = uyafmt::toc::read(&mut d)?;
    let w = &tc.wads[1];
    let (s, n) = *w.pairs.get(3).ok_or("WAD2 has no pair 3")?;
    let b = d.sectors(w.sector + s as u64, n as u64)?;
    let records = records(&b, "frontbin")?;
    let funcs = bounds(bounds_path, "101")?;
    let text = span(&funcs);
    Ok(Program { name: "fe101".into(), records, text, funcs, entry: 0 })
}

/// Story level `level`'s overlay.
pub fn level(iso: &str, bounds_path: &str, level: usize) -> Result<Program, String> {
    let mut d = Disc::open(iso)?;
    let tc = uyafmt::toc::read(&mut d)?;
    let ov = uyafmt::levelcode::overlay(&mut d, &tc, level)?;
    let records = ov.records.into_iter().map(|r| (r.address, r.data)).collect();
    let funcs = bounds(bounds_path, &level.to_string())?;
    let text = span(&funcs);
    Ok(Program { name: format!("l{level:02}"), records, text, funcs, entry: 0 })
}
