//! Sound banks: LEVEL blocks 1 and 7-9 and WAD7's gadget banks, as R&C3's 989snd_Library IOP module (WAD2 pair 1 entry 7) parses them.
//!
//! The reader is the IOP's sound library, the module `989snd_Library` (entry 7 of the 22 IRX images that the LZ stream of WAD2
//! pair 1 unpacks to; boot_elf's front end func_003982E0 copies that image to the IOP and executes the modules). The EE sends
//! the bank's sector to the IOP (func_0013BAB8, RPC 3); the module's loader `FUN_00008bac` reads the bank from the disc (for
//! banks sent from EE memory `FUN_0000898c`). Function names below are the module's own addresses (module-relative, base 0).
//!
//! A bank is a 0x18-byte wrapper (+0 type 1 or 3, +4 version 2, +8 offset and +0xC size of the SBlk part, +0x10 offset and
//! +0x14 size of the sample data), the SBlk part and the sample data (SPU2 ADPCM). The SBlk part (version 3) is: a 0x40-byte
//! header, the sound table (12-byte entries), the command table (8-byte entries; the commands of one sound are consecutive) and
//! the records that commands of type 1 (a tone, 24 bytes) and type 4 (an LFO, 16 bytes) refer to. A version 1 SBlk (three of
//! WAD7's gadget banks, records 0, 1 and 5) has a 0x3C-byte header and 40-byte command entries (record 5's 17 fill its part).
//! The module's disc and EE-memory loads (`FUN_00008bac`, RPC 3 and 0x57) refuse it (error 0x71, handle 0); its load from IOP
//! memory (`FUN_0000898c`, RPC 0x59, how the levels load their gadget banks) does not look at the version and registers it with
//! version 3's reads (`FUN_00009b78`: 12-byte sounds at +0x1C, 8-byte commands at +0x20, records from +0x34, here 0), so its
//! sounds run whatever 8-byte commands those reads find. `decode` reads a bank that way; the walk keeps a version 1 body as one
//! extent.

use crate::track::{LineId, Node, Space, Tracker};

const WRAP_NAME: &str = "sound bank wrapper (+0 type 1 or 3, +4 version 2, SBlk offset and size, sample data offset and size)";
const HEAD3: &str = "SBlk header, version 3 (magic, version, flags, id, bank number, table counts and offsets, SPU2 address and size)";
const HEAD1: &str = "SBlk header, version 1 (magic, version, flags, id, bank number, counts and offsets, SPU2 address and size)";
const SOUNDS: &str = "SBlk sound table (12-byte entries: volume, voice group, pan, command count, polyphony, flags, command offset; a sound without commands has its first four bytes unread)";
const TONE: &str = "SBlk tone record (24 bytes: priority, volume, centre note and fine tune, pan, key range, bend range, ADSR, flags, sample address)";
const LFO: &str = "SBlk LFO record (16 bytes)";
const BODY1: &str = "SBlk body, version 1 (40-byte command entries)";
const CHILD: &str = "SBlk child-sound record (32 bytes: volume, pan, sound number, name)";
const FRAMES: &str = "SPU2 ADPCM frames (16 bytes: shift and filter, flags, 28 samples)";

/// A command type: its line name, the module function that runs it, and what the module does with the entry's bytes 0..2
/// ('r' read, 'z' unread by the module and required zero, 'u' unread by the module, kept zero where the disc has zero, 'o' the
/// 24-bit offset of a record).
struct Cmd {
    ty: u8,
    name: &'static str,
    game: &'static str,
    bytes: [u8; 3],
}

const CMDS: [Cmd; 26] = [
    Cmd { ty: 1, name: "SBlk command, type 1 and 9: play a tone (24-bit offset of a tone record)", game: "989snd FUN_00006240 dispatches, FUN_00004a2c; the delay is read by FUN_00006240 and FUN_00003fd0; nothing reads +6 (skipped: no reader)", bytes: *b"ooo" },
    Cmd { ty: 4, name: "SBlk command, type 4: set an LFO (24-bit offset of an LFO record)", game: "989snd FUN_00006240 dispatches, FUN_00004f34; the delay is read by FUN_00006240 and FUN_00003fd0; nothing reads +6 (skipped: no reader)", bytes: *b"ooo" },
    Cmd { ty: 5, name: "SBlk command, type 5: start a sound of this bank (24-bit offset of a child-sound record)", game: "989snd FUN_00006240 dispatches, FUN_00005ddc; nothing reads +6 (skipped: no reader)", bytes: *b"ooo" },
    Cmd { ty: 20, name: "SBlk command, type 20: no operation", game: "989snd FUN_00006240 dispatches, FUN_0000505c returns; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"zzz" },
    Cmd { ty: 21, name: "SBlk command, type 21: loop start marker", game: "989snd FUN_00006240 dispatches, FUN_00005064 returns, FUN_0000506c searches for it by type; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"uuu" },
    Cmd { ty: 22, name: "SBlk command, type 22: jump back to the previous type 21", game: "989snd FUN_00006240 dispatches, FUN_0000506c; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"uuu" },
    Cmd { ty: 23, name: "SBlk command, type 23: jump forward to the next type 22", game: "989snd FUN_00006240 dispatches, FUN_0000511c; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"zzz" },
    Cmd { ty: 24, name: "SBlk command, type 24: end of the sound (returns -1)", game: "989snd FUN_00006240 dispatches, FUN_000051b8; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"uuu" },
    Cmd { ty: 25, name: "SBlk command, type 25: random skip (+0 variants, +1 entries per variant, +2 last pick)", game: "989snd FUN_00006240 dispatches, FUN_00005204; nothing reads +6 (skipped: no reader)", bytes: *b"rrr" },
    Cmd { ty: 26, name: "SBlk command, type 26: random extra delay (24-bit maximum)", game: "989snd FUN_00006240 dispatches, FUN_00005314; nothing reads +6 (skipped: no reader)", bytes: *b"rrr" },
    Cmd { ty: 27, name: "SBlk command, type 27: random bend (+0 percent)", game: "989snd FUN_00006240 dispatches, FUN_00005360; nothing reads bytes 1-2 or +6 (skipped: no reader)", bytes: *b"rzz" },
    Cmd { ty: 28, name: "SBlk command, type 28: set the bend (+0)", game: "989snd FUN_00006240 dispatches, FUN_00005428; nothing reads bytes 1-2 or +6 (skipped: no reader)", bytes: *b"rzz" },
    Cmd { ty: 29, name: "SBlk command, type 29: add to the bend (+0)", game: "989snd FUN_00006240 dispatches, FUN_000054a0; nothing reads bytes 1-2 or +6 (skipped: no reader)", bytes: *b"rzz" },
    Cmd { ty: 30, name: "SBlk command, type 30: set a variable (+0 variable, +1 value)", game: "989snd FUN_00006240 dispatches, FUN_00005524; nothing reads byte 2 or +6 (skipped: no reader)", bytes: *b"rrz" },
    Cmd { ty: 31, name: "SBlk command, type 31: random variable (+0 variable, +1 minimum, +2 maximum)", game: "989snd FUN_00006240 dispatches, FUN_00005560; nothing reads +6 (skipped: no reader)", bytes: *b"rrr" },
    Cmd { ty: 32, name: "SBlk command, type 32: increment a variable (+0)", game: "989snd FUN_00006240 dispatches, FUN_000055f4; nothing reads bytes 1-2 or +6 (skipped: no reader)", bytes: *b"rzz" },
    Cmd { ty: 33, name: "SBlk command, type 33: decrement a variable (+0)", game: "989snd FUN_00006240 dispatches, FUN_00005664; nothing reads bytes 1-2 or +6 (skipped: no reader)", bytes: *b"rzz" },
    Cmd { ty: 34, name: "SBlk command, type 34: compare a variable and skip the next entry (+0 variable, +1 test, +2 value)", game: "989snd FUN_00006240 dispatches, FUN_000056d4; nothing reads +6 (skipped: no reader)", bytes: *b"rrr" },
    Cmd { ty: 35, name: "SBlk command, type 35: label (+0 label number)", game: "989snd FUN_00006240 dispatches, FUN_00005798 returns, FUN_000057a0 and FUN_00005848 search for it by +0; nothing reads bytes 1-2 or +6 (skipped: no reader)", bytes: *b"rzz" },
    Cmd { ty: 36, name: "SBlk command, type 36: jump to a label (+0 label number)", game: "989snd FUN_00006240 dispatches, FUN_000057a0; nothing reads bytes 1-2 or +6 (skipped: no reader)", bytes: *b"rzz" },
    Cmd { ty: 38, name: "SBlk command, type 38: wait for the sound's voices (no parameters)", game: "989snd FUN_00006240 dispatches, FUN_00005944; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"zzz" },
    Cmd { ty: 39, name: "SBlk command, type 39: cycling skip (+0 variants, +1 entries per variant, +2 next)", game: "989snd FUN_00006240 dispatches, FUN_00005980; nothing reads +6 (skipped: no reader)", bytes: *b"rrr" },
    Cmd { ty: 40, name: "SBlk command, type 40: add to a variable (+0 amount, +1 variable)", game: "989snd FUN_00006240 dispatches, FUN_00005a4c; nothing reads byte 2 or +6 (skipped: no reader)", bytes: *b"rrz" },
    Cmd { ty: 41, name: "SBlk command, type 41: voice command (no parameters)", game: "989snd FUN_00006240 dispatches, FUN_00005ab0; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"zzz" },
    Cmd { ty: 42, name: "SBlk command, type 42: voice command (no parameters)", game: "989snd FUN_00006240 dispatches, FUN_00005ad4; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"zzz" },
    Cmd { ty: 43, name: "SBlk command, type 43: jump to the last entry", game: "989snd FUN_00006240 dispatches, FUN_00005b0c; nothing reads bytes 0-2 or +6 (skipped: no reader)", bytes: *b"uuu" },
];

fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// The wrapper, the SBlk header and the offsets and counts the rest depends on.
struct Head {
    version: u32,
    n_sounds: usize,
    n_cmds: usize,
    n_samples: usize,
    sounds_at: usize,
    cmds_at: usize,
    records_at: usize,
}

/// One bank of `b.len()` bytes at `at` in `parent`; its own length comes from its wrapper and the rest of `b` is left to the
/// caller. `game` is the EE function(s) that send the sector to the IOP; every call of one `group` passes the same string.
/// A bank of R&C3's build (LEVEL blocks, WAD7).
pub fn bank(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], game: &'static str) -> Result<(), String> {
    bank_in(t, parent, group, at, b, game, Space::Disc)
}

/// The same bank in `space`: a bank the front end unpacked or copied into memory (WAD4 screen sound banks, WAD2 pack stream 12),
/// whose `at` is an offset in the unpacked block. `game` names the EE function that hands it to the IOP.
pub fn bank_in(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], game: &'static str, space: Space) -> Result<(), String> {
    // The wrapper: six words.
    let lw = t.line(group, WRAP_NAME, "sbank::bank", game, space);
    let mut w = t.child(parent, lw, at, 0x18.min(b.len() as u64));
    let wrap = (|| -> Result<(usize, usize, usize, usize), String> {
        if b.len() < 0x18 {
            return Err(format!("bank of {:#x} bytes", b.len()));
        }
        let word = |k: usize| u32_at(b, 4 * k);
        if word(0) != 1 && word(0) != 3 {
            return Err(format!("+0 type {:#x}", word(0)));
        }
        if word(1) != 2 {
            return Err(format!("+4 version {:#x}", word(1)));
        }
        if word(2) != 0x18 || word(4) != word(2) + word(3) {
            return Err(format!("SBlk at {:#x}+{:#x}, sample data at {:#x}", word(2), word(3), word(4)));
        }
        let end = word(4) as u64 + word(5) as u64;
        if end > b.len() as u64 {
            return Err(format!("bank ends at {end:#x}, past its {:#x} bytes", b.len()));
        }
        Ok((word(2) as usize, word(3) as usize, word(4) as usize, word(5) as usize))
    })();
    let (sb_at, sb_size, sd_at, sd_size) = match wrap {
        Ok(v) => {
            w.mark(0, 0x18);
            v
        }
        Err(e) => {
            w.fail(e);
            t.close(w);
            return Ok(());
        }
    };
    t.close(w);
    let sb = &b[sb_at..sb_at + sb_size];
    let sd = &b[sd_at..sd_at + sd_size];
    let base = at + sb_at as u64;

    // The SBlk header.
    let version = if sb.len() >= 8 { u32_at(sb, 4) } else { 0 };
    let hsize = if version == 1 { 0x3c } else { 0x40 };
    let lh = t.line(group, if version == 1 { HEAD1 } else { HEAD3 }, "sbank::bank", "989snd FUN_00008bac (magic, version), FUN_00009720 (flags, SPU2 address, sizes), FUN_00009b78 (offsets, counts), FUN_0000a2dc (id), FUN_0000a374 (bank number); nothing reads +0x14 and +0x1a (skipped: no reader)", space);
    let mut h = t.child(parent, lh, base, (hsize as u64).min(sb.len() as u64));
    let head = header(&mut h, sb, sd_size, hsize);
    t.close(h);
    let head = match head {
        Ok(v) => v,
        Err(_) => return Ok(()),
    };

    if head.version == 1 {
        let l = t.ignored_line(group, BODY1, "989snd reads a version 1 body only with version 3's reads, after a load from IOP memory (FUN_0000898C, RPC 0x59; FUN_00008BAC refuses it from the disc and EE memory, error 0x71): skipped on the claim, not settled, that no level loads WAD7 records 0, 1 and 5's banks into its gadget slots (PLAN, unproven claims)", space);
        let n = t.child(parent, l, base + hsize as u64, (sb.len() - hsize) as u64);
        t.close(n);
        frames(t, parent, group, at + sd_at as u64, sd, space);
        return Ok(());
    }

    // The command table first: which tone and LFO records exist.
    let cmd_at = head.cmds_at;
    let mut tones = std::collections::BTreeSet::new();
    let mut lfos = std::collections::BTreeSet::new();
    let mut kids = std::collections::BTreeSet::new();
    for i in 0..head.n_cmds {
        let e = &sb[cmd_at + 8 * i..cmd_at + 8 * i + 8];
        let off = e[0] as usize | (e[1] as usize) << 8 | (e[2] as usize) << 16;
        match e[3] {
            1 | 9 => {
                tones.insert(off);
            }
            4 => {
                lfos.insert(off);
            }
            5 => {
                kids.insert(off);
            }
            _ => {}
        }
    }

    // The sound table: one instance for the whole table.
    let ls = t.line(group, SOUNDS, "sbank::bank", "989snd FUN_00009b78 (command offsets), FUN_00003fd0 (volume, voice group, pan, command count), FUN_0000dcfc (polyphony, flags)", space);
    let mut s = t.child(parent, ls, base + head.sounds_at as u64, 12 * head.n_sounds as u64);
    match sounds(&mut s, &sb[head.sounds_at..head.sounds_at + 12 * head.n_sounds], head.n_cmds) {
        Ok(()) => {}
        Err(e) => s.fail(e),
    }
    t.close(s);

    // The commands, one instance each.
    let mut ids: [Option<LineId>; 256] = [None; 256];
    for i in 0..head.n_cmds {
        let e = &sb[cmd_at + 8 * i..cmd_at + 8 * i + 8];
        let ty = e[3];
        let Some(c) = CMDS.iter().find(|c| c.ty == if ty == 9 { 1 } else { ty }) else {
            // A command type the module's dispatch table lacks or that no bank on the disc uses.
            let l = t.line(group, "SBlk command, unknown type", "sbank::bank", "", space);
            let mut n = t.child(parent, l, base + (cmd_at + 8 * i) as u64, 8);
            n.fail(format!("command {i} type {ty}"));
            t.close(n);
            continue;
        };
        let l = *ids[ty as usize].get_or_insert_with(|| t.line(group, c.name, "sbank::bank", c.game, space));
        let mut n = t.child(parent, l, base + (cmd_at + 8 * i) as u64, 8);
        n.mark(3, 1);
        n.mark(4, 2);
        // Nothing reads +6.
        n.skip(6, 2);
        for k in 0..3 {
            match c.bytes[k] {
                b'r' | b'o' => n.mark(k as u64, 1),
                // Bytes the handler does not read.
                _ => n.skip(k as u64, 1),
            }
        }
        t.close(n);
    }

    // The records, tiling the rest of the SBlk part.
    let lt = t.line(group, TONE, "sbank::bank", "989snd FUN_00004a2c (priority, volume, pan, flags), FUN_00006d30 (priority), FUN_0001956c (centre note, ADSR, flags, sample address), FUN_0001a874 (bend range), FUN_0000d1b8 (key range); nothing reads +0x14 (skipped: no reader)", space);
    let ll = t.line(group, LFO, "sbank::bank", "989snd FUN_00004f34 (+1 is the depth: when 0 the LFO is stopped and +2..+0xF are unread, skipped; +9 is unread, skipped; +4 and +5 are read only when +3 is 2: skipped otherwise; +0xA and +0xB are read unless +8 bit 1 is set: skipped then)", space);
    let mut pos = 0usize;
    let mut addrs = std::collections::BTreeSet::new();
    let lk = t.line(group, CHILD, "sbank::bank", "989snd FUN_00005ddc (type 5: +0 volume, +4 pan, +0xC sound number; the name at +0x10 is read only when the number is negative)", space);
    // (offset, kind: 0 tone, 1 LFO, 2 child-sound record)
    let mut all: Vec<(usize, u8)> = tones.iter().map(|&o| (o, 0)).chain(lfos.iter().map(|&o| (o, 1))).chain(kids.iter().map(|&o| (o, 2))).collect();
    all.sort();
    for (o, kind) in all {
        let size = [24, 16, 32][kind as usize];
        let line = [lt, ll, lk][kind as usize];
        let abs = head.records_at + o;
        if o != pos || abs + size > sb.len() {
            // The records must follow one another without a gap.
            let mut n = t.child(parent, line, base + abs.min(sb.len().saturating_sub(size)) as u64, size as u64);
            n.fail(format!("record at +{o:#x}, the previous ends at +{pos:#x}, the part has {:#x} bytes of records", sb.len() - head.records_at));
            t.close(n);
            return Ok(());
        }
        pos = o + size;
        let r = &sb[abs..abs + size];
        let mut n = t.child(parent, line, base + abs as u64, size as u64);
        let res = match kind {
            0 => tone(&mut n, r, sd_size, &mut addrs),
            1 => lfo(&mut n, r),
            _ => child(&mut n, r, head.n_sounds),
        };
        if let Err(e) = res {
            n.fail(e);
        }
        t.close(n);
    }
    if head.records_at + pos != sb.len() {
        return Err(format!("{}: {:#x} bytes of the SBlk part follow its last record", parent.place, sb.len() - head.records_at - pos));
    }
    if addrs.len() != head.n_samples {
        return Err(format!("{}: header counts {} samples, the tones use {} sample addresses", parent.place, head.n_samples, addrs.len()));
    }
    frames(t, parent, group, at + sd_at as u64, sd, space);
    Ok(())
}

fn header(h: &mut Node, sb: &[u8], sd_size: usize, hsize: usize) -> Result<Head, String> {
    let r = (|| -> Result<Head, String> {
        if sb.len() < hsize {
            return Err(format!("SBlk part of {:#x} bytes", sb.len()));
        }
        if &sb[0..4] != b"SBlk" {
            return Err(format!("+0 magic {:02x?}", &sb[0..4]));
        }
        let version = u32_at(sb, 4);
        if version != 1 && version != 3 {
            return Err(format!("+4 version {version}"));
        }
        // The only flag value on the disc: bit 2 (the loader allocates SPU2 memory itself).
        if u32_at(sb, 8) != 4 {
            return Err(format!("+8 flags {:#x}", u32_at(sb, 8)));
        }
        if sb[0x11..0x14].iter().any(|&v| v != 0) {
            return Err(format!("+0x11 {:02x?}", &sb[0x11..0x14]));
        }
        if u16_at(sb, 0x14) != 0 {
            return Err(format!("+0x14 {:#x}", u16_at(sb, 0x14)));
        }
        let (n_sounds, n_cmds, n_samples) = (u16_at(sb, 0x16) as usize, u16_at(sb, 0x18) as usize, u16_at(sb, 0x1a) as usize);
        let (sounds_at, cmds_at, records_at) = (u32_at(sb, 0x1c) as usize, u32_at(sb, 0x20) as usize, u32_at(sb, 0x34) as usize);
        if sounds_at != hsize {
            return Err(format!("+0x1c sound table at {sounds_at:#x}, not {hsize:#x}"));
        }
        if u32_at(sb, 0x28) as usize != sd_size || u32_at(sb, 0x2c) as usize != sd_size {
            return Err(format!("+0x28 {:#x}, +0x2c {:#x}, sample data {sd_size:#x} bytes", u32_at(sb, 0x28), u32_at(sb, 0x2c)));
        }
        if u32_at(sb, 0x30) != 0 || u32_at(sb, 0x38) != 0 || (hsize == 0x40 && u32_at(sb, 0x3c) != 0) {
            return Err(format!("+0x30 {:#x}, +0x38 {:#x}", u32_at(sb, 0x30), u32_at(sb, 0x38)));
        }
        if version == 3 {
            if cmds_at != sounds_at + 12 * n_sounds {
                return Err(format!("+0x20 command table at {cmds_at:#x}, the sound table ends at {:#x}", sounds_at + 12 * n_sounds));
            }
            if records_at != cmds_at + 8 * n_cmds || records_at > sb.len() {
                return Err(format!("+0x34 records at {records_at:#x}, the command table ends at {:#x}", cmds_at + 8 * n_cmds));
            }
        }
        Ok(Head { version, n_sounds, n_cmds, n_samples, sounds_at, cmds_at, records_at })
    })();
    match r {
        Ok(v) => {
            // +0 magic, +4 version, +8 flags, +0xC id, +0x10 bank number (byte) and its three zero bytes, +0x14 zero, +0x16 sounds,
            // +0x18 commands, +0x1A samples (checked against the tones), +0x1C sound table, +0x20 command table, +0x24 SPU2 address,
            // +0x28 and +0x2C sizes, +0x30 zero (the loader's list link), +0x34 records, +0x38 and +0x3C zero (flags 0x100 and 0x200
            // clear).
            if v.version == 3 {
                h.mark(0, hsize as u64);
            } else {
                // The version 1 body is kept as one extent, so the sample count at +0x1A cannot be checked: skipped.
                h.mark(0, 0x1a);
                h.skip(0x1a, 2);
                h.mark(0x1c, (hsize - 0x1c) as u64);
            }
            Ok(v)
        }
        Err(e) => {
            h.fail(e.clone());
            Err(e)
        }
    }
}

fn sounds(n: &mut Node, s: &[u8], n_cmds: usize) -> Result<(), String> {
    let mut next = 0usize;
    for i in 0..s.len() / 12 {
        let e = &s[12 * i..12 * i + 12];
        let (count, flags, off) = (e[4] as usize, u16_at(e, 6), u32_at(e, 8) as usize);
        // Bit 0 (FUN_00006b28), bit 1 (FUN_00003fd0), bit 2 (FUN_00003f98's sound start), bit 3, 4 and 5 (FUN_0000dcfc, FUN_0000de38
        // polyphony), bit 15 (not tested by the module); bit 14 is set at run time and cleared by FUN_00009b78.
        if flags & !0x803f != 0 {
            return Err(format!("sound {i} flags {flags:#06x}"));
        }
        if count == 0 {
            // FUN_00003fd0 starts a sound only when its command count is not zero, and reads the volume, voice group and pan
            // inside that test: a sound without commands has them unread. The builder wrote the offset as -8 (the command
            // table's start minus 8 once FUN_00009b78 relocates it).
            if off != 0xffff_fff8 {
                return Err(format!("sound {i} has no commands and its offset is {off:#x}"));
            }
            n.skip(12 * i as u64, 4);
            n.mark(12 * i as u64 + 4, 8);
            continue;
        }
        if off != 8 * next {
            return Err(format!("sound {i} commands at +{off:#x}, expected +{:#x}", 8 * next));
        }
        next += count;
        n.mark(12 * i as u64, 12);
    }
    if next != n_cmds {
        return Err(format!("the sounds hold {next} commands, the header {n_cmds}"));
    }
    Ok(())
}

fn tone(n: &mut Node, r: &[u8], sd_size: usize, addrs: &mut std::collections::BTreeSet<u32>) -> Result<(), String> {
    // +0xE flags: bit 0 (FUN_0001956c: reverb), bits 1-2 (FUN_00006d30), bit 3 (noise), bit 4 and bit 5 (FUN_00004a2c).
    let flags = u16_at(r, 0xe);
    if flags & !0x3f != 0 {
        return Err(format!("+0xE flags {flags:#06x}"));
    }
    let addr = u32_at(r, 0x10);
    if addr % 16 != 0 || addr as usize >= sd_size {
        return Err(format!("+0x10 sample address {addr:#x}, the sample data has {sd_size:#x} bytes"));
    }
    addrs.insert(addr);
    n.mark(0, 0x14);
    // Nothing reads +0x14.
    n.skip(0x14, 4);
    Ok(())
}

fn child(n: &mut Node, r: &[u8], n_sounds: usize) -> Result<(), String> {
    // +0 volume and +4 pan (negative values name a variable or the random source), +8 unread, +0xC the sound to start in this bank
    // (negative: a name at +0x10 looked up in other banks), +0x10..+0x20 the name, read only for a negative number.
    let num = u32_at(r, 0xc) as i32;
    if num < 0 || num as usize >= n_sounds {
        return Err(format!("+0xC sound number {num}, the bank has {n_sounds} sounds"));
    }
    n.mark(0, 8);
    n.skip(8, 4);
    n.mark(0xc, 4);
    n.skip(0x10, 0x10);
    Ok(())
}

fn lfo(n: &mut Node, r: &[u8]) -> Result<(), String> {
    // +0 slot, +1 depth (0 stops the LFO: nothing else is read), +2, +3 kind, +4 (kind 2 only), +6, +8 flags, +9 unread, +0xA phase
    // (unless +8 bit 1), +0xC.
    n.mark(0, 2);
    if r[1] == 0 {
        n.skip(2, 14);
        return Ok(());
    }
    n.skip(9, 1);
    n.mark(2, 2);
    n.mark(6, 2);
    n.mark(8, 1);
    n.mark(0xc, 4);
    if r[3] == 2 {
        n.mark(4, 2);
    } else {
        n.skip(4, 2);
    }
    if r[8] & 2 != 0 {
        n.skip(0xa, 2);
    } else {
        n.mark(0xa, 2);
    }
    Ok(())
}

fn frames(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, sd: &[u8], space: Space) {
    let l = t.line(group, FRAMES, "sbank::bank", "989snd FUN_00009720 sends them to the SPU2", space);
    let mut f = t.child(parent, l, at, sd.len() as u64);
    if sd.len() % 16 != 0 {
        f.fail(format!("{:#x} bytes of sample data", sd.len()));
    } else {
        match (0..sd.len()).step_by(16).find(|&k| sd[k] >> 4 > 4 || sd[k] & 15 > 12 || sd[k + 1] > 7) {
            None => f.mark(0, sd.len() as u64),
            Some(k) => f.fail(format!("frame at +{k:#x}: {:02x?}", &sd[k..k + 2])),
        }
    }
    t.close(f);
}

// ---- the decoded bank: what the port's sound engine plays ----

/// A sound of a version 3 bank (the sound table's 12-byte entry: +0 volume, +1 voice group, +2 pan, +4 command count, +5
/// polyphony, +6 flags, +8 the commands' offset): 989snd FUN_00003fd0 reads the volume, group, pan and count when the sound starts.
#[derive(Clone, Debug)]
pub struct Sound {
    pub volume: i8,
    pub group: u8,
    /// A pan angle in degrees (0 front, 90 right, 180 behind, 270 left).
    pub pan: i16,
    pub polyphony: u8,
    pub flags: u16,
    pub cmds: Vec<Command>,
}

/// A command (the command table's 8-byte entry): +0..+2 its parameters (a 24-bit record offset for types 1, 4, 5 and 9), +3 its
/// type, +4 the delay in driver ticks before the next command, +6 unread.
#[derive(Clone, Copy, Debug)]
pub struct Command {
    pub ty: u8,
    pub arg: [u8; 3],
    pub delay: i16,
}

impl Command {
    /// The 24-bit offset of the record the command names.
    pub fn offset(&self) -> u32 {
        self.arg[0] as u32 | (self.arg[1] as u32) << 8 | (self.arg[2] as u32) << 16
    }
}

/// A tone record (24 bytes): what a command of type 1 or 9 plays. 989snd FUN_00004a2c, FUN_00006d30 (priority), FUN_0001956c
/// (the rest), FUN_0001a874 (bend range).
#[derive(Clone, Copy, Debug)]
pub struct Tone {
    pub priority: i8,
    /// 0..127; negative: a variable (-1..-4 the sound's, -5 random, below -5 the engine's).
    pub volume: i8,
    pub centre_note: i8,
    pub fine_tune: i8,
    /// A pan angle in degrees, or negative: a variable.
    pub pan: i16,
    pub key_low: i8,
    pub key_high: i8,
    /// The pitch bend's range in semitones, downwards and upwards.
    pub bend_down: i8,
    pub bend_up: i8,
    /// The envelope: attack, decay, sustain and release as two 16-bit words (see `snd::env`).
    pub adsr1: u16,
    pub adsr2: u16,
    /// Bit 0 reverb send, bits 1-2 priority fade, bit 3 noise, bit 4 no dry output, bit 5 not stolen.
    pub flags: u16,
    /// The first ADPCM frame's byte offset in the bank's sample data.
    pub sample: u32,
}

/// An LFO record (16 bytes, type 4 commands): kept as it is; FUN_00004f34 reads it.
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    pub raw: [u8; 16],
}

/// A child-sound record (32 bytes, type 5 commands): +0 volume, +4 pan (negative: variables), +0xC the sound to start.
#[derive(Clone, Copy, Debug)]
pub struct Kid {
    pub volume: i32,
    pub pan: i32,
    pub sound: i32,
}

/// A version 3 bank, decoded for the engine.
#[derive(Clone, Debug)]
pub struct Bank {
    /// The SBlk header's id (+0xC) and bank number (+0x10).
    pub id: u32,
    pub number: u8,
    /// The SBlk part's version (+4): 1 or 3.
    pub version: u32,
    pub sounds: Vec<Sound>,
    pub tones: std::collections::BTreeMap<u32, Tone>,
    pub lfos: std::collections::BTreeMap<u32, Lfo>,
    pub kids: std::collections::BTreeMap<u32, Kid>,
    /// The PS-ADPCM frames (16 bytes each) the tones' sample offsets point into.
    pub samples: Vec<u8>,
}

/// Decodes the bank at the start of `b` (the wrapper: `bank` above explains the layout) as the module reads it: each sound's
/// commands are the 8-byte entries at the command table plus its offset (FUN_00009B78, FUN_00006240), the records they name
/// from +0x34. A version 3 bank's sounds stay inside its command table; a version 1 bank's read its 40-byte entries as 8-byte
/// ones, as the module does (whether a load takes a version 1 bank at all is the loader's: see the module doc).
pub fn decode(b: &[u8]) -> Result<Bank, String> {
    if b.len() < 0x18 {
        return Err(format!("bank of {:#x} bytes", b.len()));
    }
    let word = |k: usize| u32_at(b, 4 * k);
    if (word(0) != 1 && word(0) != 3) || word(1) != 2 || word(2) != 0x18 || word(4) != word(2) + word(3) {
        return Err(format!("bank wrapper {:x?}", [word(0), word(1), word(2), word(3), word(4), word(5)]));
    }
    let (sb_at, sb_size, sd_at, sd_size) = (word(2) as usize, word(3) as usize, word(4) as usize, word(5) as usize);
    if sd_at + sd_size > b.len() {
        return Err(format!("bank ends at {:#x}, past its {:#x} bytes", sd_at + sd_size, b.len()));
    }
    let sb = &b[sb_at..sb_at + sb_size];
    if sb.len() < 0x3c || &sb[0..4] != b"SBlk" || (u32_at(sb, 4) != 3 && u32_at(sb, 4) != 1) {
        return Err(format!("SBlk part: magic {:02x?}, version {}", &sb[..4.min(sb.len())], u32_at(sb, 4)));
    }
    let n_sounds = u16_at(sb, 0x16) as usize;
    let (sounds_at, cmds_at, records_at) = (u32_at(sb, 0x1c) as usize, u32_at(sb, 0x20) as usize, u32_at(sb, 0x34) as usize);
    if sounds_at + 12 * n_sounds > sb.len() || records_at > sb.len() {
        return Err("SBlk tables run past the part".into());
    }
    let mut cmds: Vec<Command> = Vec::new();
    let mut sounds = Vec::with_capacity(n_sounds);
    for i in 0..n_sounds {
        let e = &sb[sounds_at + 12 * i..sounds_at + 12 * i + 12];
        let (count, off) = (e[4] as usize, u32_at(e, 8) as usize);
        let list = if count == 0 {
            Vec::new()
        } else {
            let at = cmds_at + off;
            let entries = sb.get(at..at + 8 * count).ok_or_else(|| format!("sound {i}: {count} commands at +{off:#x} run past the SBlk part"))?;
            entries.chunks_exact(8).map(|e| Command { ty: e[3], arg: [e[0], e[1], e[2]], delay: i16::from_le_bytes([e[4], e[5]]) }).collect()
        };
        cmds.extend_from_slice(&list);
        sounds.push(Sound { volume: e[0] as i8, group: e[1], pan: i16::from_le_bytes([e[2], e[3]]), polyphony: e[5], flags: u16_at(e, 6), cmds: list });
    }
    let record = |off: u32, size: usize| -> Result<&[u8], String> {
        let at = records_at + off as usize;
        sb.get(at..at + size).ok_or_else(|| format!("record at +{off:#x} runs past the SBlk part"))
    };
    let (mut tones, mut lfos, mut kids) = (std::collections::BTreeMap::new(), std::collections::BTreeMap::new(), std::collections::BTreeMap::new());
    for c in &cmds {
        let off = c.offset();
        match c.ty {
            1 | 9 => {
                let r = record(off, 24)?;
                tones.insert(
                    off,
                    Tone {
                        priority: r[0] as i8,
                        volume: r[1] as i8,
                        centre_note: r[2] as i8,
                        fine_tune: r[3] as i8,
                        pan: i16::from_le_bytes([r[4], r[5]]),
                        key_low: r[6] as i8,
                        key_high: r[7] as i8,
                        bend_down: r[8] as i8,
                        bend_up: r[9] as i8,
                        adsr1: u16_at(r, 0xa),
                        adsr2: u16_at(r, 0xc),
                        flags: u16_at(r, 0xe),
                        sample: u32_at(r, 0x10),
                    },
                );
            }
            4 => {
                let r = record(off, 16)?;
                lfos.insert(off, Lfo { raw: r.try_into().expect("16 bytes") });
            }
            5 => {
                let r = record(off, 32)?;
                kids.insert(off, Kid { volume: u32_at(r, 0) as i32, pan: u32_at(r, 4) as i32, sound: u32_at(r, 0xc) as i32 });
            }
            _ => {}
        }
    }
    Ok(Bank { id: u32_at(sb, 0xc), number: sb[0x10], version: u32_at(sb, 4), sounds, tones, lfos, kids, samples: b[sd_at..sd_at + sd_size].to_vec() })
}

/// The banks of story level `level`'s LEVEL file: blocks 1 and 7-9 (the header's (sector offset, sectors) pairs from +0x10),
/// each sent to the IOP by the level's code. Blocks that are empty are left out; each entry is (block number, bank).
pub fn level_banks(iso_path: &str, level: usize) -> Result<Vec<(usize, Bank)>, String> {
    use crate::disc::Disc;
    let mut d = Disc::open(iso_path)?;
    let tc = crate::toc::read(&mut d)?;
    let f = tc.files.iter().find(|f| f.level == level && f.kind == crate::toc::FileKind::Level).ok_or_else(|| format!("no LEVEL file for level {level}"))?;
    let mut out = Vec::new();
    for k in [1usize, 7, 8, 9] {
        let (off, n) = (u32_at(&f.header, 0x10 + 8 * k) as u64, u32_at(&f.header, 0x14 + 8 * k) as u64);
        if n == 0 {
            continue;
        }
        let bytes = d.sectors(f.sector + off, n)?;
        out.push((k, decode(&bytes).map_err(|e| format!("L{level:02} LEVEL block {k}: {e}"))?));
    }
    Ok(out)
}

/// The IOP's sound modules as it loads them: 989snd_Library (`snd`, WAD2 pair 1 entry 7) and libsd (`sd`, entry 6). What the
/// port's sound engine takes from their own data: 989snd's random numbers' table and work area sizes, libsd's effect modes.
pub struct Modules {
    pub snd: Vec<u8>,
    pub sd: Vec<u8>,
}

/// Both sound modules of the disc (`Modules`).
pub fn sound_modules(iso_path: &str) -> Result<Modules, String> {
    use crate::disc::Disc;
    let mut d = Disc::open(iso_path)?;
    let tc = crate::toc::read(&mut d)?;
    let w = tc.wads.get(1).ok_or("no WAD2")?;
    let &(sector, sectors) = w.pairs.get(1).ok_or("WAD2 has no pair 1")?;
    let b = d.sectors(w.sector + sector as u64, sectors as u64)?;
    let data = crate::lz::unpack(&b)?.data;
    if data.len() < 0xC0 {
        return Err("WAD2 pair 1: no module table".into());
    }
    Ok(Modules { snd: iop_image(&data, 7)?, sd: iop_image(&data, 6)? })
}

/// Entry `k` of WAD2 pair 1's module table (`wads::iop_modules`) as the IOP loads it: its PT_LOAD segments at their module
/// addresses, the rest zero.
fn iop_image(data: &[u8], k: usize) -> Result<Vec<u8>, String> {
    let (o, s) = (u32_at(data, 8 * k) as usize, u32_at(data, 8 * k + 4) as usize);
    let e = data.get(o..o + s).ok_or_else(|| format!("WAD2 pair 1 entry {k} runs past the modules"))?;
    if e.len() < 0x34 || &e[0..4] != b"\x7fELF" {
        return Err(format!("WAD2 pair 1 entry {k} is not an ELF image"));
    }
    let (phoff, phes, phn) = (u32_at(e, 0x1c) as usize, u16_at(e, 0x2a) as usize, u16_at(e, 0x2c) as usize);
    let mut img = Vec::new();
    for i in 0..phn {
        let ph = phoff + phes * i;
        if ph + 0x18 > e.len() {
            return Err(format!("WAD2 pair 1 entry {k}: a program header past the image"));
        }
        let f = |i: usize| u32_at(e, ph + 4 * i) as usize;
        if f(0) != 1 {
            continue;
        }
        let (off, vaddr, filesz, memsz) = (f(1), f(2), f(4), f(5));
        if img.len() < vaddr + memsz {
            img.resize(vaddr + memsz, 0);
        }
        img[vaddr..vaddr + filesz].copy_from_slice(e.get(off..off + filesz).ok_or_else(|| format!("WAD2 pair 1 entry {k}: a segment past the image"))?);
    }
    if img.is_empty() {
        return Err(format!("WAD2 pair 1 entry {k}: no PT_LOAD segment"));
    }
    Ok(img)
}

/// The front end's own sound bank: stream 12 of the pack in WAD2's pair 7 (frontbin func_0039C1C8 sends it to the IOP).
pub fn front_bank(iso_path: &str) -> Result<Bank, String> {
    use crate::disc::Disc;
    let mut d = Disc::open(iso_path)?;
    let tc = crate::toc::read(&mut d)?;
    let w = tc.wads.get(1).ok_or("no WAD2")?;
    let &(sector, sectors) = w.pairs.get(7).ok_or("WAD2 has no pair 7")?;
    let b = d.sectors(w.sector + sector as u64, sectors as u64)?;
    let (o, s) = (u32_at(&b, 4 * 24) as usize, u32_at(&b, 4 * 24 + 4) as usize);
    let stream = crate::lz::unpack(b.get(o..o + s).ok_or("the front end pack's sound stream runs past its entry")?)?;
    decode(&stream.data).map_err(|e| format!("front end sound bank: {e}"))
}
