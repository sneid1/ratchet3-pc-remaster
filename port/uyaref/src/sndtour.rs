//! `uyaref sndtour DISC [--levels L,...] [--first F] [--stay S] [--press ...] [--wav OUT]`: an instrument, not part of the engine.
//!
//! The emulated machine runs the game through the tour's levels: the press script (the tour's own when none is given), then at
//! field F the first level asked for with the game's own request_level_change (`ee_oracle::rc3::warp`), and each next one S
//! fields after the last was reached. The game's sound calls are answered by the emulated 989snd driver, as in every emulated
//! run; beside it, every batch of commands the game's library sends goes to the port's engine too, so that what the game asks for
//! on each level meets the engine. Each batch is copied as it goes out: a host stub at the library's send (func_0013C908, which
//! lets the function run on) takes the buffer it is about to send (the index at gp - 0x7AD0, the buffers' addresses at gp -
//! 0x7AF0, the replies' at gp - 0x7AD8), and the copies are fed in order at the end of the field. A batch's reply lands inside
//! the next send, in its wait for the server (sceSifCheckStatRpc, 0x13C970..0x13C980): a second stub just after it (0x13C988)
//! copies the reply of the batch before.
//!
//! Handles differ between the two drivers, so the batch is translated: a bank handle to the engine's handle of the same bank (the
//! emulated driver's bank is found among the disc's by its sample data, `uya::snd::tool::disc_banks`, and loaded into the engine the
//! first time), a sound or stream handle to the engine's answer to the command that made it (from the emulated driver's reply,
//! which lands a field later). The engine is pulled a field's 800.8 frames each field (`--wav` keeps them). What is counted, per
//! level: batches and commands fed, every error the engine answers (its call, or a fault the sounds met), handles that could not
//! be translated.

use uya::snd::{rpc::Host, Engine};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// EE memory as the machine has it after a field; disc reads the engine makes into EE memory are dropped (the emulated driver
/// made the game's own), so the machine is never written.
struct Shadow<'a> {
    ram: &'a [u8],
    iop: &'a [u8],
    disc: &'a mut uyafmt::disc::Disc,
}

impl Host for Shadow<'_> {
    fn read_ee(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        let a = (addr & 0x01ff_ffff) as usize;
        self.ram.get(a..a + len).map(|b| b.to_vec()).ok_or_else(|| format!("EE memory {addr:#x}+{len:#x}"))
    }
    fn write_ee(&mut self, _addr: u32, _bytes: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn read_disc(&mut self, lba: u32, sectors: u32) -> Result<Vec<u8>, String> {
        self.disc.sectors(lba as u64, sectors as u64)
    }
    fn disc_sectors(&mut self) -> u64 {
        self.disc.sectors
    }
    fn read_iop(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        let a = addr as usize;
        self.iop.get(a..a + len).map(|b| b.to_vec()).ok_or_else(|| format!("IOP memory {addr:#x}+{len:#x}"))
    }
}

/// One level's counts.
#[derive(Default)]
struct Stats {
    fields: u64,
    batches: u64,
    commands: u64,
    cids: BTreeSet<u16>,
    errors: Vec<String>,
    error_count: u64,
    untranslated_banks: u64,
    untranslated_plays: u64,
    untranslated_handles: u64,
    /// The plays the emulated driver started and the engine did not (its answer 0), by (bank label, sound).
    unstarted: BTreeMap<(String, u32), u64>,
    /// The reverb's commands (0x10, 0x50, 0x51, 0x64) with their arguments, counted.
    reverb: BTreeMap<(u16, Vec<u32>), u64>,
}

/// What the stubs copied since the last field: the batches sent and the replies landed, each with the batch's number in the run
/// (`sent` counts them).
struct Seen {
    sent: u64,
    batches: Vec<(u64, Vec<u8>)>,
    replies: Vec<(u64, Vec<u32>)>,
}

static SEEN: std::sync::Mutex<Seen> = std::sync::Mutex::new(Seen { sent: 0, batches: Vec::new(), replies: Vec::new() });

/// The library's batch send (func_0013C908) as the game reaches it: the batch copied, the function left to run.
fn capture(c: &mut ee_oracle::cpu::Cpu) -> bool {
    let gp = c.g(28) as u32;
    let ram = &c.mem.ram;
    let k = rd32(ram, gp.wrapping_sub(0x7ad0)) & 1;
    let base = rd32(ram, gp.wrapping_sub(0x7af0) + 4 * k);
    let mut seen = SEEN.lock().expect("the stubs' copies");
    let n = seen.sent;
    seen.batches.push((n, batch_bytes(ram, base)));
    seen.sent += 1;
    false
}

/// The same send past its wait for the server (0x13C988): the reply of the batch before (in the other buffer's reply area, its
/// count still in that buffer) has landed.
fn capture_reply(c: &mut ee_oracle::cpu::Cpu) -> bool {
    let gp = c.g(28) as u32;
    let ram = &c.mem.ram;
    let prev = (rd32(ram, gp.wrapping_sub(0x7ad0)) & 1) ^ 1;
    let (base, recv) = (rd32(ram, gp.wrapping_sub(0x7af0) + 4 * prev), rd32(ram, gp.wrapping_sub(0x7ad8) + 4 * prev));
    let n = rd32(ram, base).min(64);
    let mut seen = SEEN.lock().expect("the stubs' copies");
    if seen.sent >= 2 {
        let at = seen.sent - 2;
        seen.replies.push((at, (0..n + 2).map(|j| rd32(ram, recv + 4 * j)).collect()));
    }
    false
}

/// Commands whose first word is a sound or stream handle.
const HANDLE_ARG0: [u16; 14] = [0x13, 0x14, 0x15, 0x19, 0x1b, 0x21, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x4f];

pub fn run(a: &[String]) -> Result<(), String> {
    let iso = a.get(1).ok_or("usage: uyaref sndtour DISC [--levels L,...] [--first F] [--stay S] [--press ...] [--wav OUT]")?;
    let text = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let levels: Vec<u32> = text("--levels")
        .unwrap_or_else(|| "2,3,4,5,6,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,7,31".into())
        .split(',')
        .map(|l| l.parse().map_err(|_| format!("--levels: {l}")))
        .collect::<Result<_, _>>()?;
    let first: u64 = text("--first").map_or(Ok(2400), |v| v.parse().map_err(|_| "--first FIELD"))?;
    let stay: u64 = text("--stay").map_or(Ok(900), |v| v.parse().map_err(|_| "--stay FIELDS"))?;
    let mut presses = uya::presses(a);
    if presses.is_empty() {
        presses = uya::presses(&["--press".to_string(), "480:4000:4,540:40:4,700:10:4,900:40:4".to_string()]);
    }
    let wav_path = text("--wav");
    let mut disc = uyafmt::disc::Disc::open(iso)?;
    let module = uyafmt::sbank::sound_modules(iso)?;
    let mut e = Engine::new(&module)?;
    // Every bank of the disc, decoded, to find the emulated driver's banks among.
    let candidates: Vec<(String, uyafmt::sbank::Bank)> = uya::snd::tool::disc_banks(iso)?.into_iter().filter_map(|(l, b)| uyafmt::sbank::decode(&b).ok().map(|d| (l, d))).chain(std::iter::once(("front end".to_string(), uyafmt::sbank::front_bank(iso)?))).collect();
    let mut m = ee_oracle::Machine::cold(iso, "")?;
    m.cpu.add_host_stub(0x13_c908, "sndtour: the sound library's batch send", [0x27bd_ffd0, 0x8f82_8530, 0xffb1_0018, 0xffb0_0010], capture);
    m.cpu.add_host_stub(0x13_c988, "sndtour: the batch send past its wait", [0x8f82_8530, 0x240c_1000, 0x3c04_001d, 0x2484_4900], capture_reply);
    // The engine's own start, as the boot's: the status area and the stream engine with its seven readers.
    {
        let mut h = Shadow { ram: &m.cpu.mem.ram, iop: &m.cpu.iop, disc: &mut disc };
        e.call(&mut h, 0x123456, 0, &[0x80, 0x49, 0x1d, 0, 0, 0, 0, 0])?;
        let start: Vec<u8> = [7u32, 0x4000, 0, 0].iter().flat_map(|w| w.to_le_bytes()).collect();
        e.call(&mut h, 0x123456, 0x2a, &start)?;
    }
    let (mut bank_map, mut snd_map): (HashMap<u32, u32>, HashMap<u32, u32>) = (HashMap::new(), HashMap::new());
    // Batches waiting for the emulated driver's reply, by number: (index, the engine's answer) for the commands that make a handle.
    let mut pending: HashMap<u64, Vec<(usize, u32, u32, u32)>> = HashMap::new();
    let mut labels: HashMap<u32, String> = HashMap::new();
    let mut stats: BTreeMap<(usize, u32), Stats> = BTreeMap::new();
    let (mut toured, mut reached, mut asked) = (0usize, None::<u64>, 0u64);
    let mut wav: Vec<[i16; 2]> = Vec::new();
    let mut f = 0u64;
    loop {
        f += 1;
        m.set_pad(uya::pad_at(&presses, f), [0x80; 4]);
        m.frame().map_err(|err| format!("field {f}: pc {:08x}: {err}", m.cpu.pc))?;
        let level = m.level();
        // The tour.
        if toured > 0 && reached.is_none() && level == levels[toured - 1] {
            reached = Some(f);
            println!("field {f}: level {level} reached");
        }
        if toured > 0 && reached.is_none() && f >= asked + 20_000 {
            return Err(format!("field {f}: level {} asked for at field {asked} is not reached", levels[toured - 1]));
        }
        let due = if toured == 0 { f >= first } else { reached.is_some_and(|r| f >= r + stay) };
        if due {
            if toured == levels.len() {
                break;
            }
            match ee_oracle::rc3::warp::warp(&mut m.cpu, levels[toured]) {
                Ok(w) => {
                    println!("field {f}: warp from level {} to {}", w.from, levels[toured]);
                    (toured, reached, asked) = (toured + 1, None, f);
                }
                Err(err) => println!("field {f}: {err}; asking again"),
            }
        }
        let st = stats.entry((toured, level)).or_default();
        st.fields += 1;
        // The batches the library sent this field.
        let (sent, replies) = {
            let mut seen = SEEN.lock().expect("the stubs' copies");
            (std::mem::take(&mut seen.batches), std::mem::take(&mut seen.replies))
        };
        let mut replies = replies.into_iter().peekable();
        for (seq, raw) in sent {
            // The replies landed before this batch went out: the handles the emulated driver gave, set beside the engine's.
            while let Some((r, words)) = replies.next_if(|(r, _)| *r < seq) {
                for (j, mine, bank, sound) in pending.remove(&r).unwrap_or_default() {
                    let theirs = words.get(j + 1).copied().unwrap_or(0);
                    if theirs != 0 && theirs != u32::MAX && mine != 0 {
                        snd_map.insert(theirs, mine);
                    } else if theirs != 0 && theirs != u32::MAX && bank != u32::MAX {
                        let label = labels.get(&bank).cloned().unwrap_or_else(|| format!("bank {bank:#x}"));
                        *st.unstarted.entry((label, sound)).or_default() += 1;
                    }
                }
            }
            let cmds = parse(&raw);
            if cmds.is_empty() {
                continue;
            }
            st.batches += 1;
            st.commands += cmds.len() as u64;
            let mut out = (cmds.len() as u32).to_le_bytes().to_vec();
            for (cid, len, args) in &cmds {
                st.cids.insert(*cid);
                if matches!(cid, 0x10 | 0x50 | 0x51 | 0x64) {
                    *st.reverb.entry((*cid, args.clone())).or_default() += 1;
                }
                let mut args = args.clone();
                if matches!(cid, 0x06 | 0x11 | 0x12) && args.first().is_some_and(|&b| b != u32::MAX) {
                    let theirs = args[0];
                    let mine = match bank_map.get(&theirs) {
                        Some(&h) => Some(h),
                        None => m.cpu.snd.inner.as_ref().and_then(|i| i.banks.get(theirs)).and_then(|ob| candidates.iter().find(|(_, b)| b.samples == ob.data)).map(|(l, b)| {
                            let h = e.load_bank(b.clone());
                            bank_map.insert(theirs, h);
                            labels.insert(theirs, l.clone());
                            h
                        }),
                    };
                    match mine {
                        Some(h) => args[0] = h,
                        None if *cid == 0x06 => st.untranslated_banks += 1,
                        None => st.untranslated_plays += 1,
                    }
                }
                let handle_at = if HANDLE_ARG0.contains(cid) { Some(0) } else if *cid == 0x2c { Some(5) } else { None };
                if let Some(i) = handle_at {
                    if let Some(&h) = args.get(i).filter(|&&h| h != 0 && h != u32::MAX) {
                        match snd_map.get(&h) {
                            Some(&mine) => args[i] = mine,
                            None => st.untranslated_handles += 1,
                        }
                    }
                }
                out.extend_from_slice(&cid.to_le_bytes());
                out.extend_from_slice(&len.to_le_bytes());
                let mut bytes: Vec<u8> = args.iter().flat_map(|w| w.to_le_bytes()).collect();
                bytes.resize(((*len).max(0) as usize + 3) & !3, 0);
                out.extend_from_slice(&bytes);
            }
            let mut h = Shadow { ram: &m.cpu.mem.ram, iop: &m.cpu.iop, disc: &mut disc };
            match e.call(&mut h, 0x123456, 0x4d, &out) {
                Ok(reply) => {
                    let w: Vec<u32> = reply.chunks(4).map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes"))).collect();
                    let made: Vec<(usize, u32, u32, u32)> = cmds.iter().enumerate().filter(|(_, c)| matches!(c.0, 0x11 | 0x2c)).map(|(j, c)| (j, w.get(j + 1).copied().unwrap_or(0), if c.0 == 0x11 { c.2.first().copied().unwrap_or(0) } else { u32::MAX }, c.2.get(1).copied().unwrap_or(0))).collect();
                    if !made.is_empty() {
                        pending.insert(seq, made);
                    }
                }
                Err(err) => {
                    st.error_count += 1;
                    if st.errors.len() < 5 {
                        st.errors.push(format!("field {f}: {err}"));
                    }
                }
            }
        }
        for (r, words) in replies {
            for (j, mine, bank, sound) in pending.remove(&r).unwrap_or_default() {
                let theirs = words.get(j + 1).copied().unwrap_or(0);
                if theirs != 0 && theirs != u32::MAX && mine != 0 {
                    snd_map.insert(theirs, mine);
                } else if theirs != 0 && theirs != u32::MAX && bank != u32::MAX {
                    let label = labels.get(&bank).cloned().unwrap_or_else(|| format!("bank {bank:#x}"));
                    *st.unstarted.entry((label, sound)).or_default() += 1;
                }
            }
        }
        // A field's sound from the engine (800.8 frames: 801 and 800 in turn, as audio.rs pulls).
        let mut p = vec![[0i16; 2]; if f % 5 == 0 { 800 } else { 801 }];
        e.pull_i16(&mut p);
        if wav_path.is_some() {
            wav.extend_from_slice(&p);
        }
    }
    let mut total_errors = 0;
    for ((_, level), s) in &stats {
        total_errors += s.error_count;
        println!(
            "level {level}: {} fields, {} batches, {} commands {:x?}, {} errors; not translated: {} unloads of a bank the engine never had, {} plays of a bank not found, {} sound handles",
            s.fields, s.batches, s.commands, s.cids, s.error_count, s.untranslated_banks, s.untranslated_plays, s.untranslated_handles
        );
        for err in &s.errors {
            println!("  {err}");
        }
        for ((l, snd), n) in &s.unstarted {
            println!("  started by the emulated driver, not by the engine: {l} sound {snd} ({n} times)");
        }
        for ((cid, args), n) in &s.reverb {
            println!("  reverb command {cid:#x} {args:x?} ({n} times)");
        }
    }
    if let Some(p) = wav_path {
        uya::snd::tool::write_wav(&p, &wav)?;
        println!("the engine's sound through the tour: {p} ({:.1} s)", wav.len() as f64 / 48000.0);
    }
    println!("{f} fields, {total_errors} errors");
    if total_errors > 0 {
        return Err(format!("{total_errors} sound calls failed in the engine"));
    }
    Ok(())
}

/// A word of EE memory (its physical address).
fn rd32(ram: &[u8], a: u32) -> u32 {
    let a = (a & 0x01ff_ffff) as usize;
    u32::from_le_bytes(ram[a..a + 4].try_into().expect("4 bytes"))
}

/// A batch buffer's bytes: its count and commands (nothing when the count is not a batch's).
fn batch_bytes(ram: &[u8], base: u32) -> Vec<u8> {
    let count = rd32(ram, base);
    if count == 0 || count > 64 {
        return Vec::new();
    }
    let mut at = base + 4;
    for _ in 0..count {
        let len = (rd32(ram, at) >> 16) as u16 as i16 as i32;
        at += 4 + ((len.max(0) as u32 + 3) & !3);
    }
    let (a, b) = ((base & 0x01ff_ffff) as usize, (at & 0x01ff_ffff) as usize);
    ram[a..b].to_vec()
}

/// The commands of a batch's bytes: number, length, argument words.
fn parse(raw: &[u8]) -> Vec<(u16, i16, Vec<u32>)> {
    if raw.len() < 4 {
        return Vec::new();
    }
    let count = u32::from_le_bytes(raw[0..4].try_into().expect("4 bytes")) as usize;
    let mut at = 4;
    let mut out = Vec::new();
    for _ in 0..count {
        let (cid, len) = (u16::from_le_bytes([raw[at], raw[at + 1]]), i16::from_le_bytes([raw[at + 2], raw[at + 3]]));
        let n = (len.max(0) as usize + 3) & !3;
        let args = raw[at + 4..at + 4 + n].chunks(4).map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes"))).collect();
        out.push((cid, len, args));
        at += 4 + n;
    }
    out
}
