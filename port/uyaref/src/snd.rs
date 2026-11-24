//! The emulated machine's sound beside the port's sound engine (`uya::snd`): instruments, not part of the engine or the game.
//! Each runs rac-rs's emulated PS2 (its 989snd driver, its SPU2 mixer and effect unit) and sets what it does beside the engine.
//!
//! - `uyaref sndtrace`: the commands the game's sound library queues, field by field.
//! - `uyaref sndemu`: the emulated mix of the game's streams alone.
//! - `uyaref sndcheck`: every voice the emulated driver starts, its pitch and volumes beside the engine's for the same tone.
//! - `uyaref sndlfo`: a sound with an LFO on another LFO, played by the engine and by the emulated driver, tick by tick.
//! - `uyaref sndrev`: the engine's reverb return beside rac-rs's emulated SPU2 effect unit on the same input.

use std::collections::BTreeSet;
use uya::snd::tool::{reverb_run, write_wav};
use uya::snd::{Engine, Play, RATE, SAMPLES_PER_TICK};

/// `uyaref sndtrace DISC [--fields N] [--press ...]`: an instrument, not part of the engine. The emulated machine runs the game and
/// every field the commands its sound library queued (the two batch buffers at 0x15C3C0 and 0x15D3C0 hold a count, then per
/// command a 16-bit number, a 16-bit length and the arguments) are printed as they appear, so that what the game asked for can be
/// set beside what the engine plays for it.
pub fn trace(a: &[String]) -> Result<(), String> {
    let iso = a.get(1).ok_or("usage: uyaref sndtrace DISC [--fields N] [--press ...]")?;
    let fields: u64 = a.iter().position(|x| x == "--fields").map(|i| a[i + 1].parse().expect("fields")).unwrap_or(1200);
    let presses = uya::presses(a);
    // --dump FROM,TO: the buffers are printed in every field of that range, changed or not.
    let dump: (u64, u64) = a.iter().position(|x| x == "--dump").map(|i| { let (x, y) = a[i + 1].split_once(',').expect("--dump FROM,TO"); (x.parse().expect("field"), y.parse().expect("field")) }).unwrap_or((1, 0));
    // --streams: the streams the emulated driver holds are printed when they appear (what the game's calls made of them: the group,
    // flags, volume, pan, sectors and header offsets, whether held) and when they are gone. An instrument: it shows the calls
    // that go to the module directly and so are not in the command buffers.
    let streams = a.iter().any(|x| x == "--streams");
    let mut seen = BTreeSet::new();
    let mut m = ee_oracle::Machine::cold(iso, "")?;
    let mut last: [Vec<u8>; 2] = [Vec::new(), Vec::new()];
    for f in 1..=fields {
        m.set_pad(uya::pad_at(&presses, f), [0x80; 4]);
        m.frame().map_err(|e| format!("field {f}: pc {:08x}: {e}", m.cpu.pc))?;
        if streams {
            if let Some(i) = m.cpu.snd.inner.as_ref() {
                for rec in 0..8u32 {
                    for serial in 1..64u32 {
                        let h = 0x8400_0000 | rec << 16 | serial;
                        match i.streams.stream(h) {
                            Some(s) if seen.insert(h) => println!(
                                "field {f}: stream {h:#x} group {} flags {:#x} volume {} pan {} held {} sectors {:x?} header offsets {:x?}",
                                s.group, s.flags, s.vol, s.pan, s.held, s.lba, s.ring
                            ),
                            None if seen.remove(&h) => println!("field {f}: stream {h:#x} gone"),
                            _ => {}
                        }
                    }
                }
            }
        }
        for (k, base) in [0x15C3C0u32, 0x15D3C0].into_iter().enumerate() {
            let count = m.cpu.mem.rd32(base);
            if count == 0 || count > 64 {
                last[k].clear();
                continue;
            }
            let mut at = base + 4;
            let mut raw = Vec::new();
            let mut cmds = Vec::new();
            for _ in 0..count {
                let (cid, len) = (m.cpu.mem.rd16(at), m.cpu.mem.rd16(at + 2) as i16 as i32);
                let n = ((len.max(0) as u32 + 3) & !3) as usize;
                let args: Vec<u32> = (0..n / 4).map(|w| m.cpu.mem.rd32(at + 4 + 4 * w as u32)).collect();
                raw.extend_from_slice(&(cid as u32).to_le_bytes());
                for w in &args {
                    raw.extend_from_slice(&w.to_le_bytes());
                }
                cmds.push((cid, len, args));
                at += 4 + n as u32;
            }
            if last[k] != raw || (dump.0..=dump.1).contains(&f) {
                for (cid, len, args) in &cmds {
                    println!("field {f}: buffer {k}: cmd {cid:#04x} len {len} args {}", args.iter().map(|w| format!("{w:08x}")).collect::<Vec<_>>().join(" "));
                }
                last[k] = raw;
            }
        }
    }
    Ok(())
}

/// `uyaref sndemu DISC OUT.wav [--fields N] [--press ...]`: an instrument. The emulated machine runs the game and the emulated mixer
/// renders the game's sound as `uyaref sound` does, but only the voices the emulated driver keyed to play a stream (the host event
/// `StreamSrc` names them) are kept: the other voices' key-ons, volumes and mix masks are dropped from the events. What is left is
/// the emulated mix of the streams alone, the reference the engine's streams are set against.
pub fn emu_streams(a: &[String]) -> Result<(), String> {
    use ee_oracle::snd989::voices::Reg;
    use rac_engine::spumix;
    let iso = a.get(1).ok_or("usage: uyaref sndemu DISC OUT.wav [--fields N] [--press ...]")?;
    let out = a.get(2).ok_or("sndemu: OUT.wav")?;
    let fields: u64 = a.iter().position(|x| x == "--fields").map(|i| a[i + 1].parse().expect("fields")).unwrap_or(1200);
    let presses = uya::presses(a);
    let mut m = ee_oracle::Machine::cold(iso, "")?;
    m.record_sound();
    let mut mixer = crate::record::Mixer::new();
    let mut pcm: Vec<[i16; 2]> = Vec::new();
    // The voices (core * 24 + voice in core) that play a stream now.
    let mut sv = 0u64;
    for f in 1..=fields {
        m.set_pad(uya::pad_at(&presses, f), [0x80; 4]);
        m.frame().map_err(|e| format!("field {f}: pc {:08x}: {e}", m.cpu.pc))?;
        if m.cpu.snd.inner.is_none() {
            pcm.extend(mixer.silence());
            continue;
        }
        let mut b = spumix::drain(&mut m);
        let mut kept = Vec::new();
        for mut e in std::mem::take(&mut b.ev) {
            let w = e.w;
            match w.reg {
                Reg::StreamSrc => {
                    sv |= 1 << w.index;
                    kept.push(e);
                }
                // A voice keyed from a bank is not a stream's.
                Reg::Ssa if e.bank.is_some() => sv &= !(1 << w.index),
                r if r.per_voice() => {
                    if sv >> w.index & 1 != 0 {
                        kept.push(e);
                    }
                }
                Reg::Kon | Reg::Koff | Reg::VmixL | Reg::VmixR | Reg::VmixEL | Reg::VmixER => {
                    e.w.value &= (sv >> (w.index as u32 * 24)) as u32 & 0xff_ffff;
                    kept.push(e);
                }
                _ => kept.push(e),
            }
        }
        b.ev = kept;
        pcm.extend(mixer.batch(b));
        if f % 600 == 0 {
            println!("field {f}: {:.1}s", pcm.len() as f64 / 48000.0);
        }
    }
    write_wav(out, &pcm)?;
    println!("{out}: the emulated mix of the streams alone, {:.1}s", pcm.len() as f64 / 48000.0);
    Ok(())
}

/// `uyaref sndcheck DISC LABELS [--fields N] [--press ...]`: an instrument. The emulated machine runs the game; every voice the
/// emulated driver starts (its pitch, its volumes, its envelope words and its sample's offset, the SPU2 registers it wrote) is
/// found among the tones of the banks named (LABELS: `fe`, `1`, ... joined with commas), and the engine's own pitch and volumes
/// for that tone, played with the sound's defaults, are set beside them.
pub fn check(a: &[String]) -> Result<(), String> {
    use rac_engine::spumix;
    use ee_oracle::snd989::voices::Reg;
    let iso = a.get(1).ok_or("usage: uyaref sndcheck DISC LABELS [--fields N] [--press ...]")?;
    let labels = a.get(2).ok_or("sndcheck: LABELS")?;
    let module = uyafmt::sbank::sound_modules(iso)?;
    let fields: u64 = a.iter().position(|x| x == "--fields").map(|i| a[i + 1].parse().expect("fields")).unwrap_or(1200);
    let presses = uya::presses(a);
    let mut banks = Vec::new();
    for l in labels.split(',') {
        banks.extend(uya::snd::tool::banks(iso, l)?);
    }
    // --movies: the game's movies play in the capture (headless, their pictures not kept) and the press script runs on the run's
    // clock with their time, so the run goes on past the intro into the level.
    let movies = a.iter().any(|x| x == "--movies");
    if movies {
        let dir = "captures/snd-out/movies";
        std::fs::create_dir_all(dir).map_err(|e| format!("{dir}: {e}"))?;
        crate::movie::capture(iso, dir, u64::MAX, &presses);
    }
    let mut m = ee_oracle::Machine::cold(iso, "")?;
    m.record_sound();
    // The registers of each voice as the emulated driver left them.
    let mut regs = [[0u32; 6]; 48];
    let mut starts: Vec<(u64, usize, [u32; 6], [i32; 9])> = Vec::new();
    for f in 1..=fields {
        m.set_pad(uya::pad_at(&presses, crate::movie::at_field(f)), [0x80; 4]);
        m.frame().map_err(|e| format!("field {f}: pc {:08x}: {e}", m.cpu.pc))?;
        if m.cpu.snd.inner.is_none() {
            continue;
        }
        if movies {
            // The movie's sound is the capture's, not the driver's.
            let _ = crate::movie::take_audio();
        }
        for ev in spumix::drain(&mut m).ev {
            let w = ev.w;
            let v = w.index as usize;
            match w.reg {
                Reg::VolL if v < 48 => {
                    regs[v][0] = w.value;
                    // The writes that follow a voice's start (a volume update): shown for the first starts.
                    if starts.iter().any(|s| s.1 == v && f - s.0 < 8) && starts.len() <= 3 {
                        println!("field {f} voice {v}: VOLL written {}", w.value as i16);
                    }
                }
                Reg::VolR if v < 48 => regs[v][1] = w.value,
                Reg::Pitch if v < 48 => regs[v][2] = w.value,
                Reg::Adsr1 if v < 48 => regs[v][3] = w.value,
                Reg::Adsr2 if v < 48 => regs[v][4] = w.value,
                Reg::Ssa if v < 48 => regs[v][5] = w.value,
                Reg::Kon => {
                    for k in 0..24 {
                        if w.value >> k & 1 != 0 {
                            let vv = w.index as usize * 24 + k;
                            let gv = m.cpu.snd.inner.as_ref().map(|i| i.drv.voices.group_vol).unwrap_or([0x400; 9]);
                            starts.push((f, vv, regs[vv], gv));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let (mut found, mut pitch_ok, mut vol_ok, mut shown) = (0, 0, 0, 0);
    for (f, v, r, gv) in &starts {
        let (ssa, a1, a2) = (r[5], r[3] as u16, r[4] as u16);
        // The tone with this sample and envelope, in the first bank that has one, and a sound that starts it.
        let mut hit = None;
        'outer: for (label, bank) in &banks {
            for (si, snd) in bank.sounds.iter().enumerate() {
                for c in &snd.cmds {
                    if c.ty == 1 || c.ty == 9 {
                        if let Some(t) = bank.tones.get(&c.offset()) {
                            if t.sample == ssa && t.adsr1 == a1 && t.adsr2 == a2 {
                                hit = Some((label.clone(), si));
                                break 'outer;
                            }
                        }
                    }
                }
            }
        }
        let Some((label, si)) = hit else {
            println!("field {f} voice {v}: sample {ssa:#x} adsr {a1:04x} {a2:04x}: in none of the banks named");
            continue;
        };
        found += 1;
        let mut e = Engine::new(&module)?;
        // The group volumes the emulated driver had when it started the voice.
        for (g, &vol) in gv.iter().enumerate() {
            e.set_group_volume(g, vol)?;
        }
        let handle = e.load_bank(banks.iter().find(|(l, _)| *l == label).expect("found above").1.clone());
        e.play(handle, si, Play::default())?;
        let Some(&(_, _, _, step, gain)) = e.voices_now().iter().find(|x| x.0 == ssa && x.1 == a1 && x.2 == a2) else { continue };
        let pitch = (step * 4096.0).round() as i64;
        // The gain is the register's value over 0x4000 (the chip doubles the register).
        let (gl, gr) = ((gain[0] * 16384.0).round() as i64, (gain[1] * 16384.0).round() as i64);
        let p_ok = (pitch - r[2] as i64).abs() <= 1;
        let v_ok = (gl - r[0] as i64).abs() <= 1 && (gr - r[1] as i64).abs() <= 1;
        pitch_ok += p_ok as usize;
        vol_ok += v_ok as usize;
        if (!p_ok || !v_ok) && shown < 25 {
            shown += 1;
            println!("field {f} voice {v}: {label} sound {si}: pitch emulated {:#x} engine {pitch:#x}; volumes emulated {}/{} engine {gl}/{gr} (group volumes {gv:x?})", r[2], r[0] as i16, r[1] as i16);
        }
    }
    println!("{} voices started by the emulated driver; {found} found in the banks; pitch equal (within 1) in {pitch_ok}, volumes equal (within 1) in {vol_ok}", starts.len());
    Ok(())
}

/// `uyaref sndlfo DISC LEVEL BLOCK [SOUND] [--ticks N]`: an instrument. A sound of a level's bank (block 1 or 7 to 9; without SOUND
/// the first whose LFO moves another LFO, target 5 or 6) played by the engine and by the emulated 989snd driver (`ee_oracle::snd989`)
/// from the same random numbers, and each voice's pitch register set side by side, tick by tick, where either changes.
pub fn lfo_compare(a: &[String]) -> Result<(), String> {
    use ee_oracle::snd989::{bank::{Bank as OBank, BankKind}, driver::Inner, rng::Rng250, voices::Reg};
    let iso = a.get(1).ok_or("usage: uyaref sndlfo DISC LEVEL BLOCK [SOUND] [--ticks N]")?;
    let level: usize = a.get(2).ok_or("sndlfo: LEVEL")?.parse().map_err(|_| "sndlfo: LEVEL is a number")?;
    let block: usize = a.get(3).ok_or("sndlfo: BLOCK")?.parse().map_err(|_| "sndlfo: BLOCK is a number")?;
    let ticks: usize = a.iter().position(|x| x == "--ticks").map(|i| a[i + 1].parse().expect("ticks")).unwrap_or(480);
    let mut d = uyafmt::disc::Disc::open(iso)?;
    let tc = uyafmt::toc::read(&mut d)?;
    let f = tc.files.iter().find(|f| f.level == level && f.kind == uyafmt::toc::FileKind::Level).ok_or("sndlfo: no such level")?;
    let w = |o: usize| u32::from_le_bytes(f.header[o..o + 4].try_into().expect("4 bytes")) as u64;
    let bytes = d.sectors(f.sector + w(0x10 + 8 * block), w(0x14 + 8 * block))?;
    let bank = uyafmt::sbank::decode(&bytes)?;
    let sound = match a.get(4).filter(|x| !x.starts_with("--")) {
        Some(s) => s.parse().map_err(|_| "sndlfo: SOUND is a number")?,
        None => bank.sounds.iter().position(|s| s.cmds.iter().any(|c| c.ty == 4 && bank.lfos.get(&c.offset()).is_some_and(|l| (5..=6).contains(&l.raw[1])))).ok_or("sndlfo: no sound of the bank has an LFO on another LFO")?,
    };
    println!("L{level:02} block {block} sound {sound}: commands {:?}", bank.sounds[sound].cmds.iter().map(|c| c.ty).collect::<Vec<_>>());
    for c in bank.sounds[sound].cmds.iter().filter(|c| c.ty == 4 || c.ty == 1) {
        if c.ty == 4 {
            println!("  LFO record +{:#x}: {:02x?}", c.offset(), bank.lfos[&c.offset()].raw);
        } else {
            let t = &bank.tones[&c.offset()];
            println!("  tone +{:#x}: centre note {} fine {} sample {:#x}", c.offset(), t.centre_note, t.fine_tune, t.sample);
        }
    }
    let module = uyafmt::sbank::sound_modules(iso)?;
    let mut e = Engine::new(&module)?;
    let h = e.load_bank(bank);
    e.play(h, sound, Play::default())?;
    let mut t = [0u16; 250];
    for (k, c) in module.snd[0x1d7ec..0x1d7ec + 500].chunks_exact(2).enumerate() {
        t[k] = u16::from_le_bytes([c[0], c[1]]);
    }
    let mut o = Inner::blank(Rng250::from_state(t, u16::from_le_bytes([module.snd[0x1d7e8], module.snd[0x1d7e9]]) as usize)?);
    o.record = true;
    o.module_init();
    let oh = o.add_bank(BankKind::Level, OBank::parse(&bytes)?, None);
    let t0 = o.tick();
    o.command(o.vb, 0x11, &[oh, sound as u32, 0x400, 0, 0, 0, 0], &mut None).ok_or("sndlfo: the emulated driver did not play it")?;
    let mut theirs: std::collections::BTreeMap<u8, u32> = std::collections::BTreeMap::new();
    let mut theirs_vol: std::collections::BTreeMap<u8, u32> = std::collections::BTreeMap::new();
    let mut mine_last: Vec<i64> = Vec::new();
    let mut buf = vec![[0i16; 2]; SAMPLES_PER_TICK];
    let (mut same, mut differ) = (0, 0);
    for k in 0..ticks {
        e.pull_i16(&mut buf);
        if k % 4 == 3 {
            o.vb += 1;
            o.advance_to(o.vb);
        }
        for wr in o.out.writes.drain(..) {
            match wr.reg {
                Reg::Pitch => {
                    theirs.insert(wr.index, wr.value);
                }
                Reg::VolL => {
                    theirs_vol.insert(wr.index, wr.value);
                }
                _ => {}
            }
        }
        if k % 4 != 3 {
            continue;
        }
        let mine: Vec<i64> = e.voices_now().iter().map(|v| (v.3 * 4096.0).round() as i64).collect();
        let th: Vec<i64> = theirs.values().map(|&v| v as i64).collect();
        let ok = mine == th;
        if ok { same += 1 } else { differ += 1 }
        let vols: Vec<i64> = e.voices_now().iter().map(|v| (v.4[0] * 16384.0).round() as i64).collect();
        if mine != mine_last || !ok || k < 64 {
            println!("tick {:4} (driver {}): engine pitch {mine:?} left {vols:?}, driver pitch {th:?} left {:?}{}", k + 1, o.tick() - t0, theirs_vol.values().collect::<Vec<_>>(), if ok { "" } else { "  DIFFER" });
        }
        mine_last = mine;
    }
    println!("{same} fields the same, {differ} different");
    match e.take_fault() {
        Some(f) => Err(f),
        None => Ok(()),
    }
}

/// `uyaref sndrev DISC [--mode M] [--depth D] [--delay N --feedback N] [--tail FIELDS] [--wav PREFIX]` (an instrument): the engine's reverb beside the SPU2
/// effect unit of rac-rs's emulated mix (`rac_engine::spurev`) on the same input. The engine mixes a stream sent to the reverb
/// twice, without a reverb and with mode M (default 5, the hall) at depth D (default 0x3000; the echo's and the delay's delay
/// and feedback, default 0) on the second core; their difference
/// is the engine's reverb return. The first mix is the effect's input (the stream is the only voice, dry and sent), which the
/// emulated unit, given the same mode's registers and a work area of the mode's size at the end of its page, runs into its
/// return (times the depth). Printed per side: correlation at the best lag within 8 samples and the level (RMS) of the engine's
/// return over the emulated one's, over the whole run and over the tail after the stream is let go. `--wav` writes
/// PREFIX-engine.wav (the engine's mix with the reverb) and PREFIX-emu.wav (the dry mix plus the emulated return).
pub fn reverb_compare(a: &[String]) -> Result<(), String> {
    let iso = a.get(1).ok_or("usage: uyaref sndrev DISC [--mode M] [--depth D] [--tail FIELDS] [--wav PREFIX]")?;
    let opt = |k: &str| a.iter().position(|x| x == k).and_then(|i| a.get(i + 1)).cloned();
    let num = |s: String| -> Result<u32, String> {
        match s.strip_prefix("0x") {
            Some(h) => u32::from_str_radix(h, 16),
            None => s.parse(),
        }
        .map_err(|e| format!("sndrev: {s}: {e}"))
    };
    let mode = opt("--mode").map(num).transpose()?.unwrap_or(5);
    let depth = opt("--depth").map(num).transpose()?.unwrap_or(0x3000);
    let tail = opt("--tail").map(num).transpose()?.unwrap_or(150) as usize;
    let (delay, feedback) = (opt("--delay").map(num).transpose()?.unwrap_or(0), opt("--feedback").map(num).transpose()?.unwrap_or(0));
    if !(1..=9).contains(&mode) {
        return Err(format!("sndrev: mode {mode}: 1 to 9"));
    }
    let module = uyafmt::sbank::sound_modules(iso)?;
    let presets = uya::snd::reverb::Presets::from_modules(&module.snd, &module.sd)?;
    let regs = presets.regs(mode, delay, feedback);
    let size = presets.area(mode) as u32;
    println!("mode {mode}: work area {size:#x} bytes, registers {regs:04x?}");
    let (dry, go) = reverb_run(iso, &module, None, tail, false)?;
    let (wet, _) = reverb_run(iso, &module, Some([mode, depth, delay, feedback]), tail, false)?;
    // The engine's master volume at 0x400 is 0x3FFF of the chip's: the mixes are the cores' output times that.
    let m = 0x3fff as f32 / 16384.0;
    let peak = dry.iter().chain(&wet).map(|f| f[0].abs().max(f[1].abs())).fold(0f32, f32::max);
    if peak >= 1.0 {
        return Err(format!("sndrev: the mix reaches {peak}: clipped, the difference is not the return"));
    }
    let mut rev = rac_engine::spurev::Reverb::new();
    rev.set_regs(&regs);
    let (eea, esa) = (0x1f_ffffu32, 0x20_0000 - size);
    rev.set_area_bytes(esa, eea);
    rev.clear(esa, eea);
    rev.fx_enable = true;
    let d = depth as i16 as i32;
    let mut engine = Vec::with_capacity(dry.len());
    let mut emu = Vec::with_capacity(dry.len());
    for (x, y) in dry.iter().zip(&wet) {
        let inp = x.map(|v| ((v / m) * 32768.0).round() as i32);
        let r = rev.process(inp);
        emu.push([0, 1].map(|s| ((r[s] * d) >> 15) as f32));
        engine.push([0, 1].map(|s| (y[s] - x[s]) / m * 32768.0));
    }
    let stats = |from: usize, what: &str| {
        for s in 0..2 {
            let (x, y): (Vec<f32>, Vec<f32>) = (engine[from..].iter().map(|f| f[s]).collect(), emu[from..].iter().map(|f| f[s]).collect());
            let rms = |v: &[f32]| (v.iter().map(|a| (*a as f64).powi(2)).sum::<f64>() / v.len() as f64).sqrt();
            let mut best = (f64::MIN, 0i32);
            for lag in -8i32..=8 {
                let (mut xy, mut xx, mut yy) = (0f64, 0f64, 0f64);
                for i in 0..x.len() {
                    let j = i as i64 + lag as i64;
                    if j < 0 || j >= y.len() as i64 {
                        continue;
                    }
                    let (p, q) = (x[i] as f64, y[j as usize] as f64);
                    xy += p * q;
                    xx += p * p;
                    yy += q * q;
                }
                let c = xy / (xx * yy).sqrt();
                if c > best.0 {
                    best = (c, lag);
                }
            }
            println!(
                "{what} {}: correlation {:.4} at lag {}, level {:.4} (engine RMS {:.1}, emulated {:.1}, of 32768)",
                ["left", "right"][s],
                best.0,
                best.1,
                rms(&x) / rms(&y),
                rms(&x),
                rms(&y)
            );
        }
    };
    println!("{} frames, the stream let go at {:.3} s", dry.len(), go as f64 / RATE);
    stats(0, "whole run");
    stats(go + 2 * 801, "tail");
    if let Some(p) = opt("--wav") {
        let q = |v: f32| (v * 32767.0).clamp(-32768.0, 32767.0) as i16;
        write_wav(&format!("{p}-engine.wav"), &wet.iter().map(|f| f.map(q)).collect::<Vec<_>>())?;
        write_wav(&format!("{p}-emu.wav"), &dry.iter().zip(&emu).map(|(x, r)| [0, 1].map(|s| q(x[s] + r[s] / 32768.0 * m))).collect::<Vec<_>>())?;
        println!("{p}-engine.wav, {p}-emu.wav");
    }
    Ok(())
}
