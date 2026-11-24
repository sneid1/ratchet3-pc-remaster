//! `uya sounds DISC LEVEL OUT [--sound BANK:SOUND,...]`: the engine plays a bank's sounds into WAVs, to be listened to.
//!
//! LEVEL is a story level (its LEVEL file's banks 1 and 7-9, labelled by the block that holds them) or `fe` (the front end's own
//! bank). Without `--sound` every sound of every bank is played one after another into one WAV per bank (`OUT/<label>.wav`, with
//! 0.4 s between sounds) and `OUT/<label>.txt` lists each sound's start time, length and commands; with it the named sounds
//! (`BANK` the bank's label as printed, `SOUND` its index) go to `OUT/<label>-<sound>.wav`, one each. A sound that is still playing
//! after 4 s is let go (a sustained or looping one), and what it leaves is rendered for another 2 s at most.

use super::{Engine, Play, RATE, SAMPLES_PER_TICK};
use std::fmt::Write as _;

/// Renders one sound alone; returns its frames and whether it had to be let go.
pub fn render(e: &mut Engine, bank: u32, sound: usize) -> Result<(Vec<[i16; 2]>, bool), String> {
    let h = e.play(bank, sound, Play::default())?;
    let mut out = Vec::new();
    if h == 0 {
        return Ok((out, false));
    }
    let chunk = 480;
    let mut released = false;
    let mut buf = vec![[0i16; 2]; chunk];
    loop {
        e.pull_i16(&mut buf);
        out.extend_from_slice(&buf);
        let t = out.len() as f64 / RATE;
        if !e.playing(h) {
            break;
        }
        if !released && t >= 4.0 {
            e.stop(h, false);
            released = true;
        }
        if t >= 6.0 {
            e.stop(h, true);
            break;
        }
    }
    Ok((out, released))
}

/// `uya sndrev DISC MODE DEPTH OUT_DIR`: a stream of the game's (`reverb_run`) through libsd's reverb mode `MODE` at depth `DEPTH`
/// (hex), let go and its tail rendered, with the chip's network (`OUT_DIR/original.wav`) and AUDIO > REVERB's clean one
/// (`clean.wav`), from the same calls: the A/B of the reverb alone.
pub fn sndrev(a: &[String]) -> Result<(), String> {
    let num = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|_| format!("sndrev: {s} is not hex"));
    let (iso, mode, depth, out) = (&a[1], num(&a[2])?, num(&a[3])?, &a[4]);
    let module = uyafmt::sbank::sound_modules(iso)?;
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    for (name, clean) in [("original", false), ("clean", true)] {
        let (mix, go) = reverb_run(iso, &module, Some([mode, depth, 0, 0]), 150, clean)?;
        let pcm: Vec<[i16; 2]> = mix.iter().map(|f| f.map(|x| (x * 32767.0).clamp(-32768.0, 32767.0) as i16)).collect();
        write_wav(&format!("{out}/{name}.wav"), &pcm)?;
        println!("sndrev: mode {mode} depth {depth:#x} {name}: {} frames, the stream let go at frame {go}", pcm.len());
    }
    Ok(())
}

pub fn write_wav(path: &str, pcm: &[[i16; 2]]) -> Result<(), String> {
    let mut b = Vec::with_capacity(44 + pcm.len() * 4);
    b.extend_from_slice(&crate::audio::wav_header(pcm.len() as u64));
    for s in pcm {
        b.extend_from_slice(&s[0].to_le_bytes());
        b.extend_from_slice(&s[1].to_le_bytes());
    }
    std::fs::write(path, b).map_err(|e| format!("{path}: {e}"))
}

/// The banks of `level` ("fe": the front end's) with a label each.
pub fn banks(iso: &str, level: &str) -> Result<Vec<(String, uyafmt::sbank::Bank)>, String> {
    if level == "fe" {
        return Ok(vec![("fe".to_string(), uyafmt::sbank::front_bank(iso)?)]);
    }
    let n: usize = level.parse().map_err(|_| format!("sounds: LEVEL is a story level or fe, not {level}"))?;
    Ok(uyafmt::sbank::level_banks(iso, n)?.into_iter().map(|(k, b)| (format!("L{n:02}-b{k}"), b)).collect())
}

pub fn sounds(a: &[String]) -> Result<(), String> {
    let (iso, level, out) = (a.get(1).ok_or("usage: uya sounds DISC LEVEL OUT [--sound LABEL:SOUND,...]")?, a.get(2).ok_or("sounds: LEVEL")?, a.get(3).ok_or("sounds: OUT")?);
    let only: Vec<(String, usize)> = match a.iter().position(|x| x == "--sound") {
        Some(i) => a
            .get(i + 1)
            .ok_or("--sound LABEL:SOUND,...")?
            .split(',')
            .map(|p| {
                let (l, s) = p.rsplit_once(':').ok_or("--sound LABEL:SOUND,...")?;
                Ok((l.to_string(), s.parse::<usize>().map_err(|_| "--sound: SOUND is a number")?))
            })
            .collect::<Result<_, String>>()?,
        None => Vec::new(),
    };
    std::fs::create_dir_all(out).map_err(|e| format!("{out}: {e}"))?;
    let mut e = Engine::new(&uyafmt::sbank::sound_modules(iso)?)?;
    for (label, bank) in banks(iso, level)? {
        let nsounds = bank.sounds.len();
        println!("{label}: bank id {:#x}, number {}, {nsounds} sounds, {} tones, {:.1} s of ADPCM", bank.id, bank.number, bank.tones.len(), bank.samples.len() as f64 / 16.0 * 28.0 / 44100.0);
        let handle = e.load_bank(bank);
        if !only.is_empty() {
            for (l, s) in only.iter().filter(|(l, _)| *l == label) {
                let (pcm, released) = render(&mut e, handle, *s)?;
                let path = format!("{out}/{l}-{s}.wav");
                write_wav(&path, &pcm)?;
                println!("{path}: {:.2} s{}", pcm.len() as f64 / RATE, if released { " (let go at 4 s)" } else { "" });
            }
            continue;
        }
        let (mut all, mut index) = (Vec::<[i16; 2]>::new(), String::new());
        for s in 0..nsounds {
            let def = e.bank(handle).expect("loaded above").sounds[s].clone();
            let at = all.len() as f64 / RATE;
            let (pcm, released) = match render(&mut e, handle, s) {
                Ok(r) => r,
                Err(err) => {
                    let _ = writeln!(index, "{s:4} {at:8.2}s  ERROR {err}");
                    continue;
                }
            };
            let peak = pcm.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0);
            let types: Vec<String> = def.cmds.iter().map(|c| c.ty.to_string()).collect();
            let _ = writeln!(index, "{s:4} {at:8.2}s {:6.2}s peak {peak:5} vol {:3} pan {:3} group {} flags {:#x} polyphony {} cmds [{}]{}", pcm.len() as f64 / RATE, def.volume, def.pan, def.group, def.flags, def.polyphony, types.join(" "), if released { " let go at 4 s" } else { "" });
            all.extend_from_slice(&pcm);
            all.resize(all.len() + (0.4 * RATE) as usize, [0, 0]);
        }
        let path = format!("{out}/{label}.wav");
        write_wav(&path, &all)?;
        std::fs::write(format!("{out}/{label}.txt"), index).map_err(|e| format!("{out}/{label}.txt: {e}"))?;
        println!("{path}: {:.1} s", all.len() as f64 / RATE);
    }
    match e.take_fault() {
        Some(f) => Err(f),
        None => Ok(()),
    }
}

/// `uya sndlog PLAYLOG DISC FROM TO`: an instrument. The sound calls a game's play.log recorded with UYA_SNDLOG (each `snd:
/// field F sid S fno N send [...] (B bytes) reply [...]` line, made at the tick of the engine's `snd: tick T:` line before it)
/// replayed into a new engine over a scratch EE memory and the disc, at the same ticks from tick FROM to tick TO; run with
/// UYA_SNDLOG the engine prints its own lines as the game's did (every command, each voice taken or refused), to compare a
/// change of the engine on a session's real calls. Before FROM only the settings run (the start, the voice groups' ranges and
/// volumes, the reverb, the bank loads and unloads), without the clock: no sound starts and nothing is mixed. Banks the game
/// loaded from the disc (function 3) load again; banks it sent from EE or IOP memory (0x57, 0x59) are not in the log, and the
/// commands naming them or one of their sounds are left out (counted). Bank, sound and stream handles are the replay's own,
/// matched to the log's by the replies.
pub fn sndlog(a: &[String]) -> Result<(), String> {
    use std::collections::{HashMap, HashSet};
    use std::io::BufRead;
    let usage = "usage: uya sndlog PLAYLOG DISC FROM TO";
    let (log, iso) = (a.get(1).ok_or(usage)?, a.get(2).ok_or(usage)?);
    let tick_arg = |i: usize| a.get(i).and_then(|t| t.parse::<u64>().ok()).ok_or(usage);
    let (from, to) = (tick_arg(3)?, tick_arg(4)?);
    let module = uyafmt::sbank::sound_modules(iso)?;
    let mut e = Engine::new(&module)?;
    let mut h = ReplayHost { ram: vec![0u8; 32 << 20], iop: vec![0u8; 2 << 20], disc: uyafmt::disc::Disc::open(iso)? };
    let file = std::fs::File::open(log).map_err(|x| format!("{log}: {x}"))?;
    let hex = |s: &str| u32::from_str_radix(s, 16).map_err(|_| format!("sndlog: {s} is not hex"));
    let list = |s: &str| -> Result<Vec<u32>, String> { s.split_whitespace().map(hex).collect() };
    let (mut banks, mut sounds): (HashMap<u32, u32>, HashMap<u32, u32>) = (HashMap::new(), HashMap::new());
    let mut absent: HashSet<u32> = HashSet::new();
    let (mut tick, mut calls, mut left_out) = (0u64, 0u64, 0u64);
    let mut buf = vec![[0f32; 2]; SAMPLES_PER_TICK];
    for (n, line) in std::io::BufReader::new(file).lines().enumerate() {
        let line = line.map_err(|x| format!("{log}: line {}: {x}", n + 1))?;
        if let Some(rest) = line.strip_prefix("snd: tick ") {
            tick = rest.split(':').next().and_then(|t| t.parse().ok()).ok_or_else(|| format!("{log}: line {}: no tick", n + 1))?;
            continue;
        }
        let Some(rest) = line.strip_prefix("snd: field ") else { continue };
        let bad = || format!("{log}: line {}: {line}", n + 1);
        let field = |key: &str| rest.split(key).nth(1).and_then(|s| s.split_whitespace().next()).ok_or_else(bad);
        let (sid, fno) = (hex(field("sid 0x")?)?, hex(field("fno 0x")?)?);
        let inside = |open: &str| rest.split(open).nth(1).and_then(|s| s.split(']').next()).ok_or_else(bad);
        let (send, reply) = (list(inside("send [")?)?, list(inside("reply [")?)?);
        let bytes: usize = rest.split(") reply").next().and_then(|s| s.rsplit('(').next()).and_then(|s| s.trim_end_matches(" bytes").parse().ok()).ok_or_else(bad)?;
        if bytes > 48 || send.len() * 4 != bytes {
            return Err(format!("{}: the log holds {} of the call's {bytes} bytes", bad(), send.len() * 4));
        }
        if tick > to {
            break;
        }
        // The engine as it stood when the game made the call: its clock at the call's tick (from FROM on).
        let live = tick >= from;
        if live && e.ticks < from {
            e.ticks = from;
        }
        while live && e.ticks < tick {
            e.pull(&mut buf);
        }
        calls += 1;
        if sid == 0x123457 {
            let logged = *reply.first().ok_or_else(bad)?;
            if fno == 3 {
                let r = words_of(&e.call(&mut h, sid, fno, &le(&send))?);
                banks.insert(logged, *r.first().ok_or_else(bad)?);
            } else {
                absent.insert(logged);
                left_out += 1;
            }
            continue;
        }
        // One command's arguments in the replay's handles, or None when it names a bank or sound the replay does not have.
        let map = |cid: u32, args: &[u32]| -> Option<Vec<u32>> {
            let mut v = args.to_vec();
            let handle = |x: u32| if x == 0 || x == 0xffff_ffff { Some(x) } else { sounds.get(&x).copied() };
            match cid {
                0x11 | 0x12 | 0x06 => v[0] = if v[0] == 0xffff_ffff { v[0] } else { *banks.get(&v[0])? },
                0x13 | 0x14 | 0x15 | 0x19 | 0x1b | 0x21 | 0x2d..=0x33 | 0x4f => v[0] = handle(v[0])?,
                0x2c => {
                    let w = v.get_mut(5)?;
                    *w = handle(*w)?;
                }
                _ => {}
            }
            Some(v)
        };
        // The commands of the call (a batch, function 0x4D, or one), each with its logged result.
        let mut cmds: Vec<(u32, Vec<u32>, u32)> = Vec::new();
        if fno == 0x4d {
            let (mut at, count) = (1usize, *send.first().ok_or_else(bad)? as usize);
            for j in 0..count {
                let head = *send.get(at).ok_or_else(bad)?;
                let len = (head >> 16) as usize / 4;
                cmds.push((head & 0xffff, send.get(at + 1..at + 1 + len).ok_or_else(bad)?.to_vec(), *reply.get(j + 1).ok_or_else(bad)?));
                at += 1 + len;
            }
        } else {
            cmds.push((fno, send.clone(), *reply.get(1).ok_or_else(bad)?));
        }
        // Before FROM only the settings: the start, the voice groups, the volumes, the output mode, the reverb, the unloads.
        let setting = |cid: u32| matches!(cid, 0x00 | 0x06 | 0x08 | 0x09 | 0x0b | 0x0d | 0x10 | 0x2a | 0x4e | 0x50 | 0x51 | 0x64);
        let kept: Vec<(u32, Vec<u32>, u32)> =
            cmds.iter().filter(|(cid, _, _)| live || setting(*cid)).filter_map(|(cid, args, r)| map(*cid, args).map(|v| (*cid, v, *r))).collect();
        left_out += (cmds.len() - kept.len()) as u64;
        if kept.is_empty() {
            continue;
        }
        let results = if fno == 0x4d {
            let refs: Vec<(u16, &[u32])> = kept.iter().map(|(c, v, _)| (*c as u16, v.as_slice())).collect();
            results(&e.call(&mut h, sid, fno, &batch(&refs))?, kept.len())?
        } else {
            results(&e.call(&mut h, sid, fno, &le(&kept[0].1))?, 1)?
        };
        for ((cid, _, logged), r) in kept.iter().zip(results) {
            if matches!(cid, 0x11 | 0x2c) && *logged != 0 && r != 0 {
                sounds.insert(*logged, r);
            }
        }
    }
    if let Some(f) = e.take_fault() {
        return Err(f);
    }
    println!("sndlog: {calls} calls replayed to tick {}; {left_out} commands and bank loads left out (banks from EE or IOP memory: {})", e.ticks, absent.len());
    Ok(())
}

/// A scratch EE memory and the disc, for the engine's calls to read and write.
struct ReplayHost {
    ram: Vec<u8>,
    iop: Vec<u8>,
    disc: uyafmt::disc::Disc,
}

impl ReplayHost {
    fn span(&self, addr: u32, len: usize) -> Result<std::ops::Range<usize>, String> {
        let a = (addr & 0x01ff_ffff) as usize;
        if a + len > self.ram.len() {
            return Err(format!("EE memory {addr:#x}+{len:#x}"));
        }
        Ok(a..a + len)
    }
}

impl super::rpc::Host for ReplayHost {
    fn read_ee(&mut self, addr: u32, len: usize) -> Result<Vec<u8>, String> {
        Ok(self.ram[self.span(addr, len)?].to_vec())
    }
    fn write_ee(&mut self, addr: u32, bytes: &[u8]) -> Result<(), String> {
        let r = self.span(addr, bytes.len())?;
        self.ram[r].copy_from_slice(bytes);
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

fn batch(cmds: &[(u16, &[u32])]) -> Vec<u8> {
    let mut b = (cmds.len() as u32).to_le_bytes().to_vec();
    for (cid, args) in cmds {
        b.extend_from_slice(&cid.to_le_bytes());
        b.extend_from_slice(&((args.len() * 4) as i16).to_le_bytes());
        for a in *args {
            b.extend_from_slice(&a.to_le_bytes());
        }
    }
    b
}

fn words_of(b: &[u8]) -> Vec<u32> {
    b.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn le(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The reply of a call with `n` results: 0xFFFFFFFF, the results, 0xFFFFFFFF (the game's receive size is 4 * (n + 2)).
fn results(r: &[u8], n: usize) -> Result<Vec<u32>, String> {
    let w = words_of(r);
    if w.len() != n + 2 || w[0] != 0xffff_ffff || w[n + 1] != 0xffff_ffff {
        return Err(format!("reply {w:08x?}, expected {n} results between two 0xFFFFFFFF"));
    }
    Ok(w[1..=n].to_vec())
}

/// `uya sndreplay DISC`: the sound calls the game's boot makes (the servers, function numbers and arguments its EE code sent, as the
/// emulated run recorded them: `uyaref sndtrace` and the oracle's `UYA_SNDLOG` and `ORACLE_RPC` logs), replayed into the engine over a
/// scratch EE memory and the disc. Each reply is checked against what the game's code reads back: its length (the call's receive
/// size), the 0xFFFFFFFF words round the results, the results themselves, the status words a disc read must clear, the bytes it must
/// put in EE memory, and a bank handle that plays.
pub fn replay(a: &[String]) -> Result<(), String> {
    use super::rpc::Host;
    let iso = a.get(1).ok_or("usage: uya sndreplay DISC")?;
    let mut h = ReplayHost { ram: vec![0u8; 32 << 20], iop: vec![0u8; 2 << 20], disc: uyafmt::disc::Disc::open(iso)? };
    let module = uyafmt::sbank::sound_modules(iso)?;
    let mut e = Engine::new(&module)?;
    let (mut ok, mut bad) = (0, 0);
    let mut check = |what: &str, good: bool, detail: String| {
        if good {
            ok += 1;
            println!("ok   {what}: {detail}");
        } else {
            bad += 1;
            println!("FAIL {what}: {detail}");
        }
    };
    const STATUS: u32 = 0x1d_4980;
    // 1. func_0013B3F8: the start, sent alone: the status area's address and the flags.
    let r = e.call(&mut h, 0x123456, 0, &le(&[STATUS, 0]))?;
    check("server 0x123456 function 0 (start)", results(&r, 1).is_ok(), format!("{} bytes, receive size 12", r.len()));
    // The status area is set busy by the EE before it queues a disc read (func_0013CD28).
    h.write_ee(STATUS, &1u32.to_le_bytes())?;
    h.write_ee(STATUS + 0x10, &0xffff_ffffu32.to_le_bytes())?;
    // 2. The first batch of the boot (fields 10 to 17): three disc reads of WAD2 and the loads of the front end, the voice groups'
    // volumes, a stop of every sound, a reverb setting.
    let reads = [(675_462u32, 9u32, 0x2d_2180u32), (676_026, 0x310, 0xc7_3000), (1_153_075, 0x2d, 0x1f_bf090)];
    let mut cmds: Vec<(u16, Vec<u32>)> = reads.iter().map(|&(l, n, d)| (0x38u16, vec![l, n, d])).collect();
    for (g, v) in [(0u32, 0x2ccu32), (3, 0x266), (4, 0x266), (2, 0x400), (6, 0x400), (5, 0x320), (1, 0x300)] {
        cmds.push((0x09, vec![g, v]));
    }
    cmds.push((0x18, vec![]));
    cmds.push((0x50, vec![2, 0, 0, 0, 0]));
    let refs: Vec<(u16, &[u32])> = cmds.iter().map(|(c, v)| (*c, v.as_slice())).collect();
    let r = e.call(&mut h, 0x123456, 0x4d, &batch(&refs))?;
    check("server 0x123456 function 0x4D (the boot's first batch)", results(&r, cmds.len()).is_ok(), format!("{} commands, {} bytes, receive size {}", cmds.len(), r.len(), cmds.len() * 4 + 8));
    for &(lba, n, dest) in &reads {
        let want = h.disc.sectors(lba as u64, n as u64)?;
        let got = h.read_ee(dest, want.len())?;
        check(&format!("disc read of {n} sectors at {lba}"), want == got, format!("{} bytes in EE memory at {dest:#x}", got.len()));
    }
    let st = (words_of(&h.read_ee(STATUS, 4)?)[0], words_of(&h.read_ee(STATUS + 0x10, 4)?)[0]);
    check("status area after the reads", st == (0, 0), format!("busy {:#x}, status {:#x} (the EE polls busy == 0)", st.0, st.1));
    // 3. The direct calls of the boot: the stream engine's start, the streams' stop and wait, the stop of every sound, the settle.
    let r = e.call(&mut h, 0x123456, 0x2a, &le(&[7, 0x4000, 0, 0]))?;
    check("function 0x2A (stream engine start)", results(&r, 1).map(|v| v[0] == 1).unwrap_or(false), "result 1, the game takes it as started".to_string());
    for (f, arg, want) in [(0x34u32, vec![], 0u32), (0x36, vec![1u32], 1), (0x18, vec![], 0), (0x08, vec![], 0)] {
        let r = e.call(&mut h, 0x123456, f, &le(&arg))?;
        let v = results(&r, 1);
        check(&format!("function {f:#x}"), v.as_ref().map(|v| v[0] == want).unwrap_or(false), format!("{v:x?}, receive size 12"));
    }
    // 4. The bank loads: the front end's bank from EE memory (server 0x123457 function 0x57), a level's from the disc (function 3).
    let tc = uyafmt::toc::read(&mut h.disc)?;
    let w = tc.wads.get(1).ok_or("no WAD2")?;
    let &(sector, sectors) = w.pairs.get(7).ok_or("no WAD2 pair 7")?;
    let pack = h.disc.sectors(w.sector + sector as u64, sectors as u64)?;
    let (o, s) = (u32::from_le_bytes(pack[96..100].try_into().expect("4 bytes")) as usize, u32::from_le_bytes(pack[100..104].try_into().expect("4 bytes")) as usize);
    let bank = uyafmt::lz::unpack(&pack[o..o + s])?.data;
    h.write_ee(0xe7_3000, &bank)?;
    let r = e.call(&mut h, 0x123457, 0x57, &0xe7_3000u32.to_le_bytes())?;
    let fe = words_of(&r);
    check("server 0x123457 function 0x57 (the front end's bank, EE 0xE73000)", fe.len() == 1 && fe[0] != 0 && fe[0] != 0xffff_ffff, format!("handle {:#x}, receive size 4", fe.first().copied().unwrap_or(0)));
    let r = e.call(&mut h, 0x123457, 3, &le(&[0x11_bc44, 0]))?;
    let lv = words_of(&r);
    check("server 0x123457 function 3 (Veldin's bank, sector 0x11BC44)", lv.len() == 1 && lv[0] != 0 && lv[0] != 0xffff_ffff, format!("handle {:#x}", lv.first().copied().unwrap_or(0)));
    // 4b. The gadget banks a story level loads from IOP memory (function 0x59, func_L01_0042A300): a version 3 one and a version 1
    // one (WAD7 record 0 entry 0, id 2653), each at the start of an IOP slot; the version 1 bank from EE memory (function 0x57) is
    // refused as the module refuses it (handle 0). The version 1 bank's sound 0, read as the module reads it (one 8-byte command
    // of type 0, a no-op), ends at once: no handle, no sound.
    let w7 = tc.wads.get(6).ok_or("no WAD7")?;
    let words: Vec<u32> = w7.pairs.iter().flat_map(|&(x, y)| [x, y]).collect();
    let mut gadgets: Vec<(usize, usize, Vec<u8>, u32)> = Vec::new();
    for r in 0..words.len() / 10 {
        for k in 0..2 {
            let (s, c) = (words[10 * r + 3 + 5 * k], words[10 * r + 4 + 5 * k]);
            if s != 0 && c != 0 {
                let b = h.disc.sectors(w7.sector + s as u64, c as u64)?;
                let v = uyafmt::sbank::decode(&b)?.version;
                gadgets.push((r, k, b, v));
            }
        }
    }
    let v1 = gadgets.iter().find(|g| g.3 == 1).ok_or("no version 1 gadget bank")?.clone();
    let v3 = gadgets.iter().find(|g| g.3 == 3).ok_or("no version 3 gadget bank")?.clone();
    for (slot, g) in [(0u32, &v3), (1, &v1)] {
        let at = 0x10_0000 + slot * 0xc800;
        h.iop[at as usize..at as usize + g.2.len()].copy_from_slice(&g.2);
        let r = e.call(&mut h, 0x123457, 0x59, &at.to_le_bytes())?;
        let hd = words_of(&r)[0];
        check(&format!("server 0x123457 function 0x59 (WAD7 record {} entry {}, version {}, IOP {at:#x})", g.0, g.1, g.3), hd != 0 && hd != u32::MAX, format!("handle {hd:#x}"));
        if g.3 == 1 {
            let snd = results(&e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x11, &[hd, 0, 0x400, 0, 0, 0])]))?, 1)?[0];
            let mut pcm = vec![[0i16; 2]; 9600];
            e.pull_i16(&mut pcm);
            let peak = pcm.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0);
            check("sound 0 of the version 1 gadget bank", snd == 0 && peak == 0, format!("handle {snd:#x}, peak {peak} in 0.2 s"));
            e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x18, &[])]))?;
            h.write_ee(0xe7_3000, &g.2)?;
            let r = e.call(&mut h, 0x123457, 0x57, &0xe7_3000u32.to_le_bytes())?;
            check("the version 1 gadget bank from EE memory (function 0x57)", words_of(&r) == [0], format!("handle {:#x}: refused, as 989snd FUN_00008BAC refuses it", words_of(&r)[0]));
        }
    }

    // 5. A sound: the play command (0x11) with the arguments the boot's first sound gave it (sound 3 of the front end's bank at
    // full master volume), the query (0x19), a volume update (0x21) and a stop (0x15).
    let hd = fe[0];
    let r = e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x11, &[hd, 3, 0x400, 0, 0, 0])]))?;
    let snd = results(&r, 1)?[0];
    check("command 0x11 (play)", snd >> 24 == 0x85, format!("handle {snd:#x}"));
    let r = e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x19, &[snd]), (0x21, &[snd, 1, 0x400, 0, 0, 0])]))?;
    let q = results(&r, 2)?;
    check("commands 0x19 and 0x21 on it", q == [snd, snd], format!("{q:x?}"));
    let mut pcm = vec![[0i16; 2]; 4800];
    e.pull_i16(&mut pcm);
    let peak = pcm.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0);
    let mut e2 = Engine::new(&module)?;
    let hh = e2.load_bank(uyafmt::sbank::decode(&bank)?);
    let (direct, _) = render(&mut e2, hh, 3)?;
    let want_peak = direct.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0);
    // Sound 3 is in voice group 4, whose volume the boot's batch set to 0x266: the sound is that group's share of the sound played
    // directly ((0x266 ^ 2 >> 10) / 1024 of it, 989snd FUN_0001B82C); with the group's volume back at 0x400 it is the same.
    let scaled = want_peak as i64 * ((0x266 * 0x266) >> 10) / 1024;
    check("the played sound, at the group volume the boot set", (peak as i64 - scaled).abs() <= 2 && peak > 0, format!("peak {peak}, the sound played directly {want_peak} times the group's share {scaled}"));
    e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x18, &[]), (0x09, &[4, 0x400])]))?;
    let r = e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x11, &[hd, 3, 0x400, 0, 0, 0])]))?;
    let snd2 = results(&r, 1)?[0];
    let mut pcm = vec![[0i16; 2]; 4800];
    e.pull_i16(&mut pcm);
    let peak2 = pcm.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0);
    check("the same sound with the group's volume reset to 0x400", snd2 >> 24 == 0x85 && peak2 == want_peak, format!("peak {peak2}, played directly {want_peak}"));
    let r = e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x15, &[snd])]))?;
    check("command 0x15 (stop)", results(&r, 1).is_ok(), format!("{} bytes", r.len()));

    // 6. The streams and the reverb, with the calls the game made in the milestone run (field 1477 to 1887 of
    // `--press 480:4000:4,540:40:4,700:10:4,900:40:4`: the command buffers in `uyaref sndtrace --streams`, the arguments of the starts
    // that go to the module directly from the game's code, func_0045BB48 and the like): the first stream is held and stereo (group
    // 1), a stinger is held (group 2), the looping music queues behind the first and the first is let go.
    let wav_path = a.iter().position(|x| x == "--wav").map(|i| a[i + 1].clone());
    let mut wav: Vec<[i16; 2]> = Vec::new();
    let one = |e: &mut Engine, h: &mut ReplayHost, cid: u16, args: &[u32]| -> Result<u32, String> { Ok(results(&e.call(h, 0x123456, 0x4d, &batch(&[(cid, args)]))?, 1)?[0]) };
    // Runs `fields` fields (800.8 frames each: 801, 800, 801, ...) and gives the loudest sample of them.
    let run = |e: &mut Engine, wav: &mut Vec<[i16; 2]>, fields: usize| -> u16 {
        let mut peak = 0;
        for _ in 0..fields {
            let mut p = vec![[0i16; 2]; 801];
            e.pull_i16(&mut p);
            peak = peak.max(p.iter().map(|f| f[0].unsigned_abs().max(f[1].unsigned_abs())).max().unwrap_or(0));
            wav.extend_from_slice(&p);
        }
        peak
    };
    e.stop_all();
    let r = e.call(&mut h, 0x123456, 0x2a, &le(&[7, 0x4000, 0, 0]))?;
    check("function 0x2A when the engine is started", results(&r, 1).map(|v| v[0] == 0).unwrap_or(false), "result 0: the module answers an error for a second start".to_string());
    let s1 = one(&mut e, &mut h, 0x2c, &[0xbc2d5, 0xbc311, 0x0400_0000, 0, 1, 0, 0, 1])?;
    check("command 0x2C: the first stream, held (stereo, group 1)", s1 == 0x8400_0002, format!("handle {s1:#x}"));
    let started = one(&mut e, &mut h, 0x4f, &[s1])?;
    check("command 0x4F before the first data is in", started == 0, format!("{started}"));
    let quiet = run(&mut e, &mut wav, 1);
    let started = one(&mut e, &mut h, 0x4f, &[s1])?;
    check("command 0x4F a field later; a held stream is silent", started == 1 && quiet == 0, format!("started {started}, peak {quiet}"));
    let (parts, total, elapsed, left) = (one(&mut e, &mut h, 0x30, &[s1])?, one(&mut e, &mut h, 0x33, &[s1])?, one(&mut e, &mut h, 0x31, &[s1])?, one(&mut e, &mut h, 0x32, &[s1])?);
    check("commands 0x30 0x33 0x31 0x32 on a held stream", parts == 1 && total > 0 && elapsed == 0 && left == total, format!("parts {parts}, total {} ms, played {elapsed}, left {} ms", total * 1000 / 1024, left * 1000 / 1024));
    let s2 = one(&mut e, &mut h, 0x2c, &[0xae8fb, 0, 0x0400_0000, 0, 2, 0, 0, 1])?;
    check("command 0x2C: the stinger, held (mono, group 2)", s2 == 0x8401_0002, format!("handle {s2:#x}"));
    let t0 = std::time::Instant::now();
    let queued = one(&mut e, &mut h, 0x2c, &[0xbc8fa, 0xbc34d, 0x0400_0000, 0, 1, s1, 0, 4])?;
    let took = t0.elapsed();
    let parts = one(&mut e, &mut h, 0x30, &[s1])?;
    check("command 0x2C: the looping music queued behind the first stream", queued == 1 && parts == 2, format!("answer {queued}, parts {parts}, read and decoded in {} ms", took.as_millis()));
    let miss = one(&mut e, &mut h, 0x2c, &[0xae8fb, 0, 0x0400_0000, 0, 2, 0x8405_0001, 0, 0x10])?;
    let gone = one(&mut e, &mut h, 0x4f, &[0x8405_0001])?;
    check("a queue behind a stream that is gone, with flag 0x10, and 0x4F on it", miss == 0 && gone == u32::MAX, format!("answers {miss:#x}, {gone:#x}"));
    one(&mut e, &mut h, 0x2e, &[s1])?;
    let loud = run(&mut e, &mut wav, 50);
    let el = one(&mut e, &mut h, 0x31, &[s1])?;
    let want = 50 * 801 * 1024 / 48000;
    check("resumed: it plays, its time follows the clock", loud > 0 && el.abs_diff(want) <= 12, format!("peak {loud}, played {el} (1/1024 s), after 50 fields {want}"));
    // The first part's end: the looping part joins, the parts left are one and the time left is -1.
    let mut fields = 50;
    while one(&mut e, &mut h, 0x30, &[s1])? == 2 && fields < 20000 {
        run(&mut e, &mut wav, 1);
        fields += 1;
    }
    let (parts, left, el) = (one(&mut e, &mut h, 0x30, &[s1])?, one(&mut e, &mut h, 0x32, &[s1])?, one(&mut e, &mut h, 0x31, &[s1])?);
    let loud = run(&mut e, &mut wav, 20);
    let looped = one(&mut e, &mut h, 0x33, &[s1])?;
    check(
        "the first part ends: the looping part joins without a gap",
        parts == 1 && left == u32::MAX && loud > 0 && el < 100,
        format!("after {} s: parts {parts}, left {left:#x}, the new part ({} s long) played {el}, peak {loud}", fields * 801 / 48000, looped / 1024),
    );
    let before = e.stream_gains(s2);
    one(&mut e, &mut h, 0x2e, &[s2])?;
    run(&mut e, &mut wav, 20);
    let g0 = e.stream_gains(s2).first().map(|g| g[0].max(g[1])).unwrap_or(0.0);
    check("the stinger resumed: it has a voice and a volume", before.len() == 1 && g0 > 0.5, format!("volume {g0:.3}"));
    // The pan slide (0x21 with bits 0 and 2, as the game sends it each field): the stinger at the centre slides to 90 degrees (the
    // right) in 15 ticks, and back.
    let side = |e: &Engine| e.stream_gains(s2).first().copied().unwrap_or([0.0; 2]);
    let (c0, vol) = (side(&e), 0x400);
    one(&mut e, &mut h, 0x21, &[s2, 7, vol, 90, 0, 0])?;
    run(&mut e, &mut wav, 5);
    let r90 = side(&e);
    one(&mut e, &mut h, 0x21, &[s2, 7, vol, 0, 0, 0])?;
    run(&mut e, &mut wav, 5);
    let c1 = side(&e);
    check(
        "command 0x21 with bit 2: the pan slides to the right and back",
        (c0[0] - c0[1]).abs() < 0.01 && r90[1] > 20.0 * r90[0] && (c1[0] - c0[0]).abs() < 0.01,
        format!("centre {:.3}/{:.3}, at 90 degrees {:.3}/{:.3}, back {:.3}/{:.3}", c0[0], c0[1], r90[0], r90[1], c1[0], c1[1]),
    );
    // Ducking: group 1 (the music) plays, so group 2 falls to a quarter of its volume (its volume goes by the square) in half a
    // second, and comes back when the music is let go.
    one(&mut e, &mut h, 0x60, &[0, 1, 1 << 2, 0x4000, 0x10000, 0x10000])?;
    run(&mut e, &mut wav, 60);
    let g1 = e.stream_gains(s2).first().map(|g| g[0].max(g[1])).unwrap_or(0.0);
    check("command 0x60: the music ducks the stinger's group", (g1 / g0 - 1.0 / 16.0).abs() < 0.01, format!("volume {g1:.4} of {g0:.3}: {:.4} (the floor 0x4000 squared is 0.0625)", g1 / g0));
    one(&mut e, &mut h, 0x2f, &[s1])?;
    let ended = one(&mut e, &mut h, 0x19, &[s1])?;
    run(&mut e, &mut wav, 60);
    let g2 = e.stream_gains(s2).first().map(|g| g[0].max(g[1])).unwrap_or(0.0);
    let after = one(&mut e, &mut h, 0x19, &[s1])?;
    check("command 0x2F ends the music; the duck lifts; the handle goes after a tick", ended == s1 && after == 0 && (g2 / g0 - 1.0).abs() < 0.01, format!("handle {ended:#x} then {after:#x}, volume back to {:.3}", g2 / g0));
    one(&mut e, &mut h, 0x60, &[0, 0xffff_ffff])?;
    one(&mut e, &mut h, 0x15, &[s2])?;
    run(&mut e, &mut wav, 2);
    let (q, st) = (one(&mut e, &mut h, 0x19, &[s2])?, one(&mut e, &mut h, 0x4f, &[s2])?);
    check("command 0x15 on a stream", q == 0 && st == u32::MAX, format!("0x19 {q:#x}, 0x4F {st:#x}"));
    // The reverb: a stream with flag 2 is sent to it. With none set, nothing is heard after it is let go; with a hall of depth
    // 0x3000 switched on, a tail is; with the depth moved to 0 it is not.
    let tail = |e: &mut Engine, h: &mut ReplayHost, wav: &mut Vec<[i16; 2]>| -> Result<u16, String> {
        let s = one(e, h, 0x2c, &[0xb0ab3, 0, 0x0400_0000, 0, 6, 0, 0, 2])?;
        run(e, wav, 20);
        one(e, h, 0x2f, &[s])?;
        run(e, wav, 2);
        Ok(run(e, wav, 30))
    };
    let dry = tail(&mut e, &mut h, &mut wav)?;
    one(&mut e, &mut h, 0x50, &[2, 5, 0x3000, 0, 0])?;
    one(&mut e, &mut h, 0x64, &[1])?;
    run(&mut e, &mut wav, 4);
    let wet = tail(&mut e, &mut h, &mut wav)?;
    one(&mut e, &mut h, 0x10, &[2, 0, 0, 3])?;
    let off = tail(&mut e, &mut h, &mut wav)?;
    check("commands 0x50 0x64 0x10: the reverb's tail after a stream is let go", dry == 0 && wet > 0 && off == 0, format!("no reverb {dry}, hall at depth 0x3000 {wet}, depth moved to 0 {off}"));
    // 0x51 reserves the work area of studio B (switching the hall off): a hall (a larger area) is refused and the core stays in
    // mode 0, where a depth is an error (the chip would return its work area's stale words); a studio B is not refused.
    one(&mut e, &mut h, 0x51, &[2, 3])?;
    let before = e.reverb.refused;
    let off = e.call(&mut h, 0x123456, 0x4d, &batch(&[(0x50, &[2, 5, 0x3000, 0, 0])]));
    let refused = e.reverb.refused - before;
    run(&mut e, &mut wav, 4);
    let silent = tail(&mut e, &mut h, &mut wav)?;
    one(&mut e, &mut h, 0x50, &[2, 3, 0x3000, 0, 0])?;
    run(&mut e, &mut wav, 4);
    let fits = tail(&mut e, &mut h, &mut wav)?;
    check(
        "command 0x51: a mode over the reserved work area is refused",
        refused == 1 && off.is_err() && silent == 0 && e.reverb.refused == before + 1 && fits > 0,
        format!("hall after reserving studio B: refused {refused}, {}, tail {silent}; studio B {fits}", off.err().unwrap_or_else(|| "no error".into())),
    );
    one(&mut e, &mut h, 0x34, &[])?;
    if let Some(p) = wav_path {
        write_wav(&p, &wav)?;
        println!("the streams, the ducking and the reverb as the engine mixes them: {p} ({:.1} s)", wav.len() as f64 / 48000.0);
    }
    if let Some(f) = e.take_fault() {
        return Err(f);
    }
    println!("{ok} checks passed, {bad} failed");
    if bad > 0 {
        return Err(format!("{bad} replies differ from what the game's code reads back"));
    }
    Ok(())
}

/// `uya sndbanks DISC`: an instrument. Every sound bank the disc holds (each story level's LEVEL blocks 1 and 7-9, the front end's
/// bank, WAD7's gadget records' two banks each) with its wrapper and SBlk version, and in the banks the engine decodes the records
/// the engine's open cases depend on: LFOs whose target is not 1-4 (another LFO's rate or depth) and child sounds named by a
/// bank name (a negative sound number).
pub fn bank_census(a: &[String]) -> Result<(), String> {
    let iso = a.get(1).ok_or("usage: uya sndbanks DISC")?;
    let all = disc_banks(iso)?;
    let fe = uyafmt::sbank::front_bank(iso)?;
    println!("front end bank: id {:#x}, {} sounds", fe.id, fe.sounds.len());
    let (mut lfo_other, mut named_kids) = (0, 0);
    for (label, b) in &all {
        let w = |o: usize| b.get(o..o + 4).map_or(0, |x| u32::from_le_bytes(x.try_into().expect("4 bytes")));
        let sb = w(8) as usize;
        let (magic, version) = (b.get(sb..sb + 4).map(|m| String::from_utf8_lossy(m).into_owned()).unwrap_or_default(), w(sb + 4));
        match uyafmt::sbank::decode(b) {
            Ok(bank) => {
                let lfos: Vec<u8> = bank.lfos.values().map(|l| l.raw[1]).filter(|&t| t > 4).collect();
                let kids = bank.kids.values().filter(|k| k.sound < 0).count();
                lfo_other += lfos.len();
                named_kids += kids;
                println!("{label}: wrapper {} {}, {magic} version {version}, {} sounds{}{}", w(0), w(4), bank.sounds.len(),
                    if lfos.is_empty() { String::new() } else { format!(", LFO targets {lfos:?}") },
                    if kids == 0 { String::new() } else { format!(", {kids} child sounds named in another bank") });
                if version == 1 {
                    // A version 1 bank's sounds as the module runs them (8-byte commands): each command's type, arguments and delay.
                    for (i, snd) in bank.sounds.iter().enumerate() {
                        let cmds: Vec<String> = snd.cmds.iter().map(|c| format!("{}:{:02x?}/{}", c.ty, c.arg, c.delay)).collect();
                        println!("  sound {i}: {}", cmds.join(" "));
                    }
                }
            }
            Err(e) => {
                // What the module's registration (FUN_00009b78) reads from the header: the counts at +0x16 and +0x18, the
                // offsets at +0x1C, +0x20 and +0x34, the flags at +8.
                let h = |o: usize| w(sb + o);
                println!("{label}: wrapper {} {}, {magic} version {version}: not decoded ({e}); header +8 {:#x}, +0x16/+0x18 counts {} {}, +0x1C {:#x} +0x20 {:#x} +0x34 {:#x}, SBlk {:#x} bytes",
                    w(0), w(4), h(8), h(0x14) >> 16, h(0x18) & 0xffff, h(0x1c), h(0x20), h(0x34), w(12));
            }
        }
    }
    println!("{} banks; LFOs with another target {lfo_other}; child sounds named in another bank {named_kids}", all.len());
    Ok(())
}

/// Every sound bank the disc holds but the front end's (each story level's LEVEL blocks 1 and 7-9, WAD7's gadget records' two
/// banks each), labelled, as the bytes the loads read.
pub fn disc_banks(iso: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut d = uyafmt::disc::Disc::open(iso)?;
    let tc = uyafmt::toc::read(&mut d)?;
    let mut all: Vec<(String, Vec<u8>)> = Vec::new();
    for f in tc.files.iter().filter(|f| f.kind == uyafmt::toc::FileKind::Level) {
        for k in [1usize, 7, 8, 9] {
            let w = |o: usize| u32::from_le_bytes(f.header[o..o + 4].try_into().expect("4 bytes")) as u64;
            let (off, n) = (w(0x10 + 8 * k), w(0x14 + 8 * k));
            if n != 0 {
                all.push((format!("L{:02} LEVEL block {k}", f.level), d.sectors(f.sector + off, n)?));
            }
        }
    }
    let w7 = tc.wads.get(6).ok_or("no WAD7")?;
    let words: Vec<u32> = w7.pairs.iter().flat_map(|&(a, b)| [a, b]).collect();
    for r in 0..words.len() / 10 {
        for (x, y, e) in [(3usize, 4usize, 0), (8, 9, 1)] {
            let (s, c) = (words[10 * r + x] as u64, words[10 * r + y] as u64);
            if s != 0 && c != 0 {
                all.push((format!("WAD7 record {r} entry {e} (id {})", words[10 * r + 5 * e]), d.sectors(w7.sector + s, c)?));
            }
        }
    }
    Ok(all)
}

/// One run for the reference's `uyaref sndrev` (port/uyaref) and `uya sndrev`: the engine started as the boot starts it, the reverb set on the second core
/// when `reverb` is given (mode, depth, delay, feedback), 4 fields, a mono stream with flag 2 (the replay's: sector 0xB0AB3, group 6) for 20 fields, then
/// let go and `tail` fields more; `clean`: AUDIO > REVERB's clean network's return. The mix (f32 frames) and the frame where the stream was let go.
pub fn reverb_run(iso: &str, module: &uyafmt::sbank::Modules, reverb: Option<[u32; 4]>, tail: usize, clean: bool) -> Result<(Vec<[f32; 2]>, usize), String> {
    let mut h = ReplayHost { ram: vec![0u8; 32 << 20], iop: vec![0u8; 2 << 20], disc: uyafmt::disc::Disc::open(iso)? };
    let mut e = Engine::new(module)?;
    e.set_clean_reverb(clean);
    results(&e.call(&mut h, 0x123456, 0, &le(&[0x1d_4980, 0]))?, 1)?;
    results(&e.call(&mut h, 0x123456, 0x2a, &le(&[7, 0x4000, 0, 0]))?, 1)?;
    let mut one = |e: &mut Engine, cid: u16, args: &[u32]| -> Result<u32, String> { Ok(results(&e.call(&mut h, 0x123456, 0x4d, &batch(&[(cid, args)]))?, 1)?[0]) };
    if let Some([mode, depth, delay, feedback]) = reverb {
        one(&mut e, 0x50, &[2, mode, depth, delay, feedback])?;
    }
    let mut out = Vec::new();
    let fields = |e: &mut Engine, out: &mut Vec<[f32; 2]>, n: usize| {
        for _ in 0..n {
            let mut p = vec![[0f32; 2]; 801];
            e.pull(&mut p);
            out.extend_from_slice(&p);
        }
    };
    fields(&mut e, &mut out, 4);
    let s = one(&mut e, 0x2c, &[0xb0ab3, 0, 0x0400_0000, 0, 6, 0, 0, 2])?;
    if s == 0 {
        return Err("sndrev: the stream did not start".into());
    }
    fields(&mut e, &mut out, 20);
    one(&mut e, 0x2f, &[s])?;
    let go = out.len();
    fields(&mut e, &mut out, tail);
    if let Some(f) = e.take_fault() {
        return Err(f);
    }
    Ok((out, go))
}
