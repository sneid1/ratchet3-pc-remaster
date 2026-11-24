//! The eight global WADs: the header copy at each one's first sector, and the entries its header gives, as the game's readers
//! use them (headers.rs names the readers). An entry's insides are not decoded here.

use crate::disc::{Disc, SECTOR, u16_at, u32_at, u32_be};
use crate::rd::Fields;
use crate::toc::WadHeader;
use crate::track::{NOTHING, Node, Space, Tracker};
use std::collections::BTreeSet;

pub fn wad(t: &mut Tracker, d: &mut Disc, root: &mut Node, k: usize, w: &WadHeader, toc_copy: &[u8], region: u64, recompiled: &BTreeSet<usize>) -> Result<(), String> {
    let g = format!("WAD{}", k + 1);
    // The header copy: the table of contents' header with +4 (the sector) zero. Nothing reads it.
    let first = d.sectors(w.sector, (w.size as u64).div_ceil(SECTOR))?;
    let l = t.line(&g, "header copy (the TOC's header with +4 zero)", "wads::wad", NOTHING, Space::Disc);
    let mut n = t.child(root, l, 0, w.size as u64);
    let mut want = toc_copy.to_vec();
    want[4..8].fill(0);
    if let Err(e) = Fields::new(&first[..w.size], &mut n).same(0, &want, "header copy") {
        n.fail(e);
    }
    t.close(n);
    let word = |i: usize| u32_at(toc_copy, 8 + 4 * i);
    let words = (w.size - 8) / 4;
    let entry = |t: &mut Tracker, root: &mut Node, name: &str, sector: u64, bytes: u64| {
        let at = sector * SECTOR;
        if sector == 0 || at + bytes > region * SECTOR || root.overlaps(at, bytes) {
            return;
        }
        let l = t.line(&g, name, "", "", Space::Disc);
        let n = t.child(root, l, at, bytes);
        t.close(n);
    };
    // A 2FIP image entry (pif.rs): the image's own bytes; the rest of its sectors is left to the zero-padding pass.
    let image = |t: &mut Tracker, d: &mut Disc, root: &mut Node, sector: u64, sectors: u64, game: &'static str| -> Result<(), String> {
        let at = sector * SECTOR;
        if sector == 0 || sectors == 0 || sector + sectors > region || root.overlaps(at, sectors * SECTOR) {
            return Ok(());
        }
        let b = d.sectors(w.sector + sector, sectors)?;
        // A bad value fails the image's node (the tracker lists it); the walk goes on.
        let _ = crate::pif::image(t, root, &g, at, &b, game);
        Ok(())
    };
    match k {
        // 100 movie records: the subtitle table (info block) and the program stream.
        0 => {
            for r in 0..words / 4 {
                let (info, info_bytes, movie, movie_bytes) = (word(4 * r)? as u64, word(4 * r + 1)? as u64, word(4 * r + 2)? as u64, word(4 * r + 3)? as u64);
                if info != 0 && info_bytes != 0 && !root.overlaps(info * SECTOR, info_bytes) {
                    let b = d.bytes_at((w.sector + info) * SECTOR, info_bytes)?;
                    crate::subs::table(t, root, "WAD1", info * SECTOR, &b);
                }
                if movie != 0 && movie_bytes != 0 && !root.overlaps(movie * SECTOR, movie_bytes) {
                    let b = d.bytes_at((w.sector + movie) * SECTOR, movie_bytes)?;
                    crate::movies::movie(t, root, movie * SECTOR, &b);
                }
            }
        }
        1 => {
            image(t, d, root, word(0)? as u64, word(1)? as u64, "front end func_003826C8 (through func_003824F0)")?;
            let (s, c) = (word(2)? as u64, word(3)? as u64);
            if s != 0 && c != 0 && s + c <= region && !root.overlaps(s * SECTOR, c * SECTOR) {
                let b = d.sectors(w.sector + s, c)?;
                iop_modules(t, root, s * SECTOR, &b)?;
            }
            let names = [
                "pair 0 (front end func_003826C8 loads it)",
                "pair 1 (front end func_003982E0 loads it)",
            ];
            for (p, name) in names.iter().enumerate() {
                entry(t, root, name, word(2 * p)? as u64, word(2 * p + 1)? as u64 * SECTOR);
            }
            crate::frontwad::block(t, d, root, w.sector, region, 1, 2, word(4)? as u64, word(5)? as u64)?;
            crate::frontwad::block(t, d, root, w.sector, region, 1, 7, word(14)? as u64, word(15)? as u64)?;
            // Pair 3: frontbin, an EE program (records), loaded by func_L01_00428008; pair 4: another EE program in the same
            // form that no story code loads.
            for (p, what, rec) in [(3usize, "frontbin (WAD2 pair 3)", recompiled.contains(&101)), (4, "WAD2 pair 4: an EE program no story code loads", false)] {
                let (s, c) = (word(2 * p)? as u64, word(2 * p + 1)? as u64);
                if s != 0 && !root.overlaps(s * SECTOR, c * SECTOR) {
                    let l = t.code_line("Code", what, rec);
                    let n = t.child(root, l, s * SECTOR, c * SECTOR);
                    t.close(n);
                }
            }
        }
        // 57 armor records: a model block and a texture block. A texture block is a list: u32 count (1-3), three u32 offsets
        // (unused ones zero), then the 2FIP images at those offsets, each right after the previous one. Record 56 holds bare
        // images in both slots.
        4 => {
            for r in 0..words / 4 {
                let (ms, mc, ts, tc) = (word(4 * r)? as u64, word(4 * r + 1)? as u64, word(4 * r + 2)? as u64, word(4 * r + 3)? as u64);
                let head = |d: &mut Disc, s: u64| d.bytes_at((w.sector + s) * SECTOR, 4).map(|b| &b[..] == b"2FIP");
                if head(d, ms)? {
                    image(t, d, root, ms, mc, "func_L01_00423238 (record 56, bare images), func_L01_0042A598 and frontbin func_00393A18 / func_L01_00422838 (image lists)")?;
                } else {
                    if ms != 0 && mc != 0 && ms + mc <= region && !root.overlaps(ms * SECTOR, mc * SECTOR) {
                        let b = d.sectors(w.sector + ms, mc)?;
                        // moby.rs: a block that fits neither layout fails its line (the tracker lists it).
                        let _ = crate::moby::armor(t, root, &g, ms * SECTOR, &b);
                    }
                }
                if head(d, ts)? {
                    image(t, d, root, ts, tc, "func_L01_00423238 (record 56, bare images), func_L01_0042A598 and frontbin func_00393A18 / func_L01_00422838 (image lists)")?;
                } else if ts != 0 && tc != 0 && ts + tc <= region && !root.overlaps(ts * SECTOR, tc * SECTOR) {
                    let b = d.sectors(w.sector + ts, tc)?;
                    let at = ts * SECTOR;
                    let l = t.line(&g, "armor texture list (u32 count, three u32 image offsets)", "wads::wad", "func_L01_0042A598 and frontbin func_00393A18 (func_L01_00422838)", Space::Disc);
                    let mut n = t.child(root, l, at, 16);
                    let count = u32_at(&b, 0)? as usize;
                    let offs: Vec<usize> = (0..3).map(|i| u32_at(&b, 4 + 4 * i).map(|v| v as usize)).collect::<Result<_, _>>()?;
                    let ok = (1..=3).contains(&count) && offs[0] == 16 && offs[count..].iter().all(|&o| o == 0) && offs[..count].iter().all(|&o| o >= 16 && o < b.len());
                    if ok {
                        n.mark(0, 16);
                    } else {
                        n.fail(format!("texture list {count} {offs:x?}"));
                    }
                    t.close(n);
                    if ok {
                        for i in 0..count {
                            let end = if i + 1 < count { offs[i + 1] } else { b.len() };
                            let _ = crate::pif::image(t, root, &g, at + offs[i] as u64, &b[offs[i]..end], "func_L01_00423238 (record 56, bare images), func_L01_0042A598 and frontbin func_00393A18 / func_L01_00422838 (image lists)");
                        }
                    }
                }
            }
        }
        5 => {
            let mut seen = BTreeSet::new();
            for i in 0..words {
                let s = word(i)? as u64;
                if s == 0 || !seen.insert(s) {
                    continue;
                }
                // A clip is a VAGp header (its data size, big-endian, at +0xC) and its data.
                let h = d.bytes_at((w.sector + s) * SECTOR, 0x30)?;
                if &h[0..4] != b"VAGp" {
                    continue;
                }
                let size = u32_be(&h, 0xc)? as u64;
                if (s * SECTOR + 0x30 + size) > region * SECTOR || root.overlaps(s * SECTOR, 0x30 + size) {
                    continue;
                }
                let bytes = d.bytes_at((w.sector + s) * SECTOR, 0x30 + size)?;
                crate::vag::clip(t, root, "WAD6", s * SECTOR, &bytes, "func_L01_0045B5A0 resolves its sector, func_0013CB20 streams it to the IOP");
            }
        }
        // WAD3 and WAD8: (sector, sectors) pairs; WAD4: the pair ranges that are (sector, sectors) entries (pairs 15-20, 56-116
        // and the 83-word rows from word 478 have other forms, not split yet).
        // WAD4 words 30-693: eight records in the SCENE record form (83 words), sound ids 4000-4007 (the resolver reads
        // 0x1DBC0 + id * 0x14C): words 0-1 and 4-11 VAG clips by language, 2-3 the subtitle block, 12-13 an LZ block,
        // 14 on chunk boundaries (LZ chunks).
        3 => {
            for r in 0..8usize {
                let at = 30 + 83 * r;
                let rw = |i: usize| word(at + i).map(|v| v as u64);
                for i in [0usize, 1, 4, 5, 6, 7, 8, 9, 10, 11] {
                    let s = rw(i)?;
                    if s == 0 || s >= region {
                        continue;
                    }
                    let h = d.bytes_at((w.sector + s) * SECTOR, 0x30)?;
                    let size = 0x30 + u32_be(&h, 0xc)? as u64;
                    if &h[0..4] == b"VAGp" && s * SECTOR + size <= region * SECTOR && !root.overlaps(s * SECTOR, size) {
                        let b = d.bytes_at((w.sector + s) * SECTOR, size)?;
                        crate::vag::clip(t, root, &g, s * SECTOR, &b, "func_L01_0045B5A0 / frontbin func_0039C2E8 resolve them (sound ids 4000-4007 from the records by language, 5000 on from pairs 382-388: pair 382 + id - 5000), func_0013CB20 streams them");
                    }
                }
                let (ss, sc) = (rw(2)?, rw(3)?);
                if ss != 0 && sc != 0 && ss + sc <= region && !root.overlaps(ss * SECTOR, sc * SECTOR) {
                    let b = d.sectors(w.sector + ss, sc)?;
                    crate::subs::table(t, root, &g, ss * SECTOR, &b);
                }
                let mut blocks = vec![(rw(12)?, rw(13)?)];
                for i in 14..82 {
                    let (a, b) = (rw(i)?, rw(i + 1)?);
                    if a == 0 || b <= a {
                        break;
                    }
                    blocks.push((a, b - a));
                }
                for (s, c) in blocks {
                    if s == 0 || c == 0 || s + c > region || root.overlaps(s * SECTOR, c * SECTOR) {
                        continue;
                    }
                    let b = d.sectors(w.sector + s, c)?;
                    if b.iter().all(|&v| v == 0) {
                        // As in SCENE chunk lists (func_L01_00429A70 hands comic chunks to the same loader): read by nothing.
                        let l = t.ignored_line(&g, "zero chunk-list entry", crate::walk::ZERO_ENTRY, Space::Disc);
                        let z = t.child(root, l, s * SECTOR, c * SECTOR);
                        t.close(z);
                        continue;
                    }
                    let l = t.line(&g, "comic record chunk: LZ stream", "lz::unpack", "func_L01_00433EA0 (each program's copy)", Space::Disc);
                    // Unpacked and parsed as SCENE chunks are (func_L01_00429A70 hands them to func_L01_00429740).
                    crate::walk::lz_block(t, root, l, &b, s * SECTOR, c * SECTOR, "WAD4 comic chunk (unpacked)", |t, r, d| crate::scene::chunk(t, r, d))?;
                }
            }
            for p in (0..=14).chain(347..=349) {
                crate::frontwad::block(t, d, root, w.sector, region, 3, p, word(2 * p)? as u64, word(2 * p + 1)? as u64)?;
            }
            for p in 350..=381 {
                image(t, d, root, word(2 * p)? as u64, word(2 * p + 1)? as u64, "frontbin func_00393E90 (func_L01_00423068), front end func_00398F48")?;
            }
            for p in 382..=388 {
                let s = word(2 * p)? as u64;
                let h = d.bytes_at((w.sector + s) * SECTOR, 0x30)?;
                let size = 0x30 + u32_be(&h, 0xc)? as u64;
                if &h[0..4] == b"VAGp" && s * SECTOR + size <= region * SECTOR && !root.overlaps(s * SECTOR, size) {
                    let b = d.bytes_at((w.sector + s) * SECTOR, size)?;
                    crate::vag::clip(t, root, &g, s * SECTOR, &b, "func_L01_0045B5A0 / frontbin func_0039C2E8 resolve them (sound ids 4000-4007 from the records by language, 5000 on from pairs 382-388: pair 382 + id - 5000), func_0013CB20 streams them");
                }
            }
        }
        2 | 7 => {
            for p in 0..words / 2 {
                let (s, c) = (word(2 * p)? as u64, word(2 * p + 1)? as u64);
                let game = match (k, p) {
                    (2, 329..=337) => Some("func_L01_0047C910 (pairs 329-337), func_L01_0048FE88 (pairs 349-379), func_L01_00497028 (pair 380); uploaded by func_L01_0042AA88 (frontbin func_00395EB8)"),
                    (2, 349..=379) => Some("func_L01_0047C910 (pairs 329-337), func_L01_0048FE88 (pairs 349-379), func_L01_00497028 (pair 380); uploaded by func_L01_0042AA88 (frontbin func_00395EB8)"),
                    (2, 380) => Some("func_L01_0047C910 (pairs 329-337), func_L01_0048FE88 (pairs 349-379), func_L01_00497028 (pair 380); uploaded by func_L01_0042AA88 (frontbin func_00395EB8)"),
                    (7, 71) => Some("func_L01_0049CA58 loads it (its descriptor's base pair 71, index 0); uploaded by func_L01_0042AA88 (frontbin func_00395EB8)"),
                    _ => None,
                };
                if let Some(game) = game {
                    image(t, d, root, s, c, game)?;
                } else if k == 7 && matches!(p, 0..=70 | 72..=73 | 400..=409 | 783..=784) && s != 0 && c != 0 && s + c <= region && !root.overlaps(s * SECTOR, c * SECTOR) {
                    // Raw 2FIP images no loader reaches: every WAD8 reader loads base pair + index (func_L01_00490B98's
                    // descriptors); these pairs lie below the lowest base (71), past pair 71's one image, below base 410, or
                    // past base 782's one entry (headers.rs).
                    let l = t.ignored_line(&g, "raw 2FIP image no reader's index range reaches (pairs 0-70, 72-73, 400-409, 783-784)", "read by nothing: no WAD8 reader's base pair and index range reaches these pairs (bases 71, 410, 782 load index 0 only below the next base; see the WAD8 header line)", Space::Disc);
                    let n = t.child(root, l, s * SECTOR, c * SECTOR);
                    t.close(n);
                }
            }
            if k == 2 {
                crate::wad38::wad3(t, d, root, toc_copy, w.sector, region)?;
            } else {
                crate::wad38::wad8(t, d, root, toc_copy, w.sector, region)?;
            }
        }
        // WAD7: 24 gadget records of ten words; words 1-2, 3-4, 6-7 and 8-9 are (sector, sectors) blocks (frontbin
        // func_003959A8 loads the first).
        6 => {
            for r in 0..words / 10 {
                for (a, b) in [(1usize, 2usize), (3, 4), (6, 7), (8, 9)] {
                    let (s, c) = (word(10 * r + a)? as u64, word(10 * r + b)? as u64);
                    if s == 0 || c == 0 || s + c > region || root.overlaps(s * SECTOR, c * SECTOR) {
                        continue;
                    }
                    if a == 3 || a == 8 {
                        let bytes = d.sectors(w.sector + s, c)?;
                        crate::sbank::bank(t, root, "WAD7", s * SECTOR, &bytes, "frontbin func_00395AC8 reads the bank into EE memory (func_0039D5A8), frontbin func_0039C1C8 / func_0013BC30 sends it to the IOP (RPC 0x57); its 989snd module parses it in FUN_000081c0").map_err(|e| format!("WAD7 record {r} block ({a}, {b}): {e}"))?;
                    } else {
                        crate::frontwad::block(t, d, root, w.sector, region, 6, 2 * r + usize::from(a == 6), s, c)?;
                    }
                }
            }
        }
        _ => {}
    }
    Ok(())
}

/// WAD2 pair 1, the game's own IOP modules: an LZ stream that front end func_003982E0 unpacks to EE 0x1000000 (func_003A0A20).
/// Unpacked: 24 (offset, size) u32 pairs, then 22 IRX images (ELF type 0xFF80), each at the previous one's end rounded up to
/// 0x40, zero between them. func_003982E0 reads entry 8 (+0x40, +0x44), sends that image (IOP_stash_daemon) to the IOP and
/// starts it (func_003980F0, func_001240E0); the daemon loads entries 1-7 from fixed EE addresses with fixed sizes (its code
/// at +0x14C to +0x2CC: the address in $a1, the size in $a3, for its EE-to-IOP copy and module load). Entries 9-22 are the
/// online-play modules (network stack, modem, USB headset and keyboard), which no story or front-end code loads; their
/// entries are checked against the images' own ELF extents. Entries 0 and 23 are zero.
fn iop_modules(t: &mut Tracker, root: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    const G: &str = "WAD2 IOP modules (unpacked)";
    /// The stash daemon's fixed loads: (EE address, size), in its call order.
    const STASH: [(u32, u32); 7] = [(0x10000C0, 0x19F1), (0x1026280, 0x2A65), (0x1001AC0, 0x177B5), (0x1019280, 0x1CD9), (0x101AF80, 0xB2E5), (0x1028D00, 0x6FF5), (0x102FD00, 0x2A298)];
    let l = t.line("WAD2", "pair 1: LZ stream", "lz::unpack", "func_003A0A20 (front end func_003982E0 unpacks it to 0x1000000)", Space::Disc);
    let size = b.len() as u64;
    crate::walk::lz_block(t, root, l, b, at, size, G, |t, root, data| {
        let lt = t.line(G, "module table (24 (offset, size) u32 pairs; entries 0 and 23 zero)", "wads::iop_modules", "front end func_003982E0 reads entry 8; the IOP stash daemon (entry 8) loads entries 1-7 from fixed EE addresses and sizes (checked); entries 9-22 (online-play modules, loaded by no story code) checked against the images' ELF extents", Space::Unpacked);
        let mut n = t.child(root, lt, 0, 0xC0);
        let mut bad = Vec::new();
        let mut images = Vec::new();
        let mut prev = 0xC0usize;
        for i in 0..24 {
            let (o, s) = (u32_at(data, 8 * i)? as usize, u32_at(data, 8 * i + 4)? as usize);
            if i == 0 || i == 23 {
                if o != 0 || s != 0 {
                    bad.push(format!("entry {i} ({o:#x}, {s:#x}) is not zero"));
                }
                continue;
            }
            let want = prev.div_ceil(0x40) * 0x40;
            if o != want || o + s > data.len() || data.get(o..o + 4) != Some(b"\x7fELF".as_slice()) || u16_at(data, o + 0x10)? != 0xFF80 {
                bad.push(format!("entry {i} ({o:#x}, {s:#x}): not an IRX image at {want:#x}"));
                break;
            }
            let ext = elf_extent(&data[o..o + s]).map_err(|e| format!("entry {i}: {e}"))?;
            if ext != s {
                bad.push(format!("entry {i}: size {s:#x}, the image's ELF extent is {ext:#x}"));
            }
            if (1..=7).contains(&i) && !STASH.contains(&((0x1000000 + o) as u32, s as u32)) {
                bad.push(format!("entry {i} ({o:#x}, {s:#x}) is none of the stash daemon's loads"));
            }
            images.push((i, o, s, prev));
            prev = o + s;
        }
        n.mark(0, 0xC0);
        for e in &bad {
            n.fail(e.clone());
        }
        t.close(n);
        if !bad.is_empty() {
            return Ok(());
        }
        if prev != data.len() {
            return Err(format!("the last image ends at {prev:#x}, the unpacked data at {:#x}", data.len()));
        }
        let lz = t.line(G, "zero padding before each image (to 0x40)", "wads::iop_modules", NOTHING, Space::Unpacked);
        let ls = t.unpacked_code_line("Code", "IOP modules loaded by the stash daemon (WAD2 pair 1 entries 1-7: sio2man, mcman, mcserv, padman, multitap_manager, Sound_Device_Library, 989snd_Library)", false);
        let ld = t.unpacked_code_line("Code", "IOP stash daemon (WAD2 pair 1 entry 8; front end func_003982E0 sends and starts it)", false);
        let lo = t.unpacked_code_line("Code", "online-play IOP modules (WAD2 pair 1 entries 9-22: network stack, dev9, modem, USB, headset, keyboard; no story code loads them)", false);
        for &(i, o, s, gap_at) in &images {
            if o > gap_at {
                let mut g = t.child(root, lz, gap_at as u64, (o - gap_at) as u64);
                if data[gap_at..o].iter().all(|&v| v == 0) {
                    g.mark(0, (o - gap_at) as u64);
                } else {
                    g.fail(format!("non-zero padding at {gap_at:#x}"));
                }
                t.close(g);
            }
            let l = match i {
                1..=7 => ls,
                8 => ld,
                _ => lo,
            };
            let c = t.child(root, l, o as u64, s as u64);
            t.close(c);
        }
        Ok(())
    })
}

/// An ELF32 image's extent: the end of its header tables and of every section with file bytes.
fn elf_extent(e: &[u8]) -> Result<usize, String> {
    let (phoff, shoff) = (u32_at(e, 0x1C)? as usize, u32_at(e, 0x20)? as usize);
    let (phes, phn, shes, shn) = (u16_at(e, 0x2A)? as usize, u16_at(e, 0x2C)? as usize, u16_at(e, 0x2E)? as usize, u16_at(e, 0x30)? as usize);
    let mut ext = (phoff + phes * phn).max(shoff + shes * shn);
    for k in 0..shn {
        let sh = shoff + shes * k;
        if u32_at(e, sh + 4)? != 8 {
            ext = ext.max(u32_at(e, sh + 0x10)? as usize + u32_at(e, sh + 0x14)? as usize);
        }
    }
    for k in 0..phn {
        let ph = phoff + phes * k;
        ext = ext.max(u32_at(e, ph + 4)? as usize + u32_at(e, ph + 0x10)? as usize);
    }
    Ok(ext)
}
