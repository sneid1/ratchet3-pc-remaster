//! The port's own sound engine: the game's sound calls, played from its banks by voices of the engine's own, into 48 kHz stereo.
//!
//! What it stands in for is the game's sound library: the EE side (boot_elf 0x13B3F8..0x13D2B0) queues commands and sends them in
//! batches to the IOP module `989snd_Library` (WAD2 pair 1 entry 7), whose command server answers them and whose 240 Hz tick runs
//! the sounds. This engine does the module's job from the module's own code (the decompile is the source: each function here
//! names the module function it follows) and from the banks (`uyafmt::sbank`). It does not replay the SPU2's registers: a sound is
//! a list of commands run on the tick, a tone command starts a voice (a sample, a pitch, an envelope, a volume on each side), and
//! the voices are summed. It is not rac-rs's `snd989` or `spumix`.
//!
//! # Interface (for the runtime that calls it)
//!
//! - **A call.** The game's EE code calls `sceSifCallRpc` on two servers of the module; the runtime hands each call to
//!   [`Engine::call`] with the server id, the function number, the send buffer's bytes and a reply buffer:
//!   - server `0x123456`: function 0 initialises the engine (send: flags, tick source), 0x4D runs a batch (send: a count, then per
//!     command a 16-bit command number, a 16-bit length and the arguments padded to 4 bytes; the reply holds one word per
//!     command that returns a value, in command order), and the other numbers (0..=0x6A) are single commands with the same
//!     argument layout (`rpc.rs` lists them);
//!   - server `0x123457`: function 3 loads a bank from the disc (send: sector, count), 0x57 from EE memory (send: its address),
//!     0x59 from IOP memory (send: its IOP address; the bank the EE sent there before, `rpc::Host::read_iop`). The module's
//!     functions 2 and 0x6A (a bank in chunks, a bank by name) are called by no program on the disc and are refused.
//!   The reply of a call that has one is returned in the reply buffer, as the game reads it back.
//! - **Samples.** [`Engine::pull`] fills 48 kHz stereo frames; it advances the sound clock by as many samples as it fills (200
//!   samples a tick, 240 ticks a second: four ticks a field), so the game's time and the sound's are the same whatever the host's
//!   speed. The device output (`audio.rs`) pulls once per field (800.8 frames).
//!
//! What the module answers with an error code of its own and nothing else (a play of a bank or a sound that is not there, an
//! unload of a bank that is not loaded, a volume for voice group 15, a disc or EE-memory load of a version 1 bank), the engine
//! answers the same way. Anything else the engine does not do is an error: `call` returns it, and a fault met while the sounds
//! run is kept and returned by the next call. Its random numbers are the module's: the R250 generator of FUN_0000becc over the
//! table in the module's own data (`uyafmt::sbank::sound_modules`), drawn in the module's order.

pub mod reverb;
pub mod sinc;
pub mod rpc;
pub mod slide;
pub mod stream;
pub mod tool;
pub mod voice;

use std::collections::HashMap;
use std::sync::Arc;
use uyafmt::sbank::{Bank, Command, Tone};
use voice::{Pcm, Voice};

/// UYA_SNDLOG=1 (an instrument): every command the game sends and its answer (`rpc.rs`), and every voice taken from a sound or
/// refused to a tone (`Engine::alloc`).
pub(crate) fn log_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UYA_SNDLOG").is_some())
}

/// UYA_SNDWATCH=1 (an instrument, off by default on the player's word, 25 November 2026: "turn the census thing and other debug
/// stuff off"): the three short lines that hunt the plasma whip's repeating sound without UYA_SNDLOG's every command: a held
/// weapon sound the game starts again right after it ended and why it ended (`Ended`), a weapon sound started again and again
/// (`weapon_start`), a looping voice sounding for 20 s and whose it is (`voice_census`).
pub(crate) fn watch_on() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UYA_SNDWATCH").is_some())
}

/// The output rate.
pub const RATE: f64 = 48000.0;
/// Samples per driver tick: the tick is 240 Hz.
pub const SAMPLES_PER_TICK: usize = 200;
/// The chip's voices (989snd's 48: two cores of 24), the ones the game's voice groups name (commands 0xD and 0x4E).
pub const CHIP_VOICES: usize = 48;
/// Voices the engine has: the chip's, then 80 of its own that every voice group may take once its own range is busy (`alloc`),
/// mixed with the second core's. The chip's 48 were not enough for the Annihilation Nation arena's fights: a new tone took the
/// voice of a sound still playing, and the game restarted a held sound it lost (the plasma whip's, with its sharp start).
pub const VOICES: usize = 128;

/// A bank the engine holds: its data, its decoded samples (by the sample's first byte) and the mutable byte of the commands that
/// pick a variant (types 25 and 39 keep the last pick in their own entry); `live` until it is unloaded (the handle stays).
struct Loaded {
    live: bool,
    bank: Bank,
    pcm: HashMap<u32, Arc<Pcm>>,
    picks: HashMap<(usize, usize), i8>,
}

/// An LFO a sound runs (the module's 0x30-byte slot, FUN_00004f34 sets it up, FUN_00002574 runs it every second tick): a wave of
/// 2048 steps whose value, times a depth, moves the sound's volume (target 1), pan (2), transposition (3) or pitch bend (4), or
/// another of the sound's LFOs' rate (5) or depth (6), the one in slot `other`. `amp` (+6) is the amplitude now (what target 6
/// moves), `amp0` (+8) and `step0` (+0x14) the record's.
#[derive(Clone, Copy)]
struct LfoSlot {
    kind: u8,
    target: u8,
    other: usize,
    amp: i32,
    amp0: i32,
    step0: i32,
    invert: bool,
    /// The phase (27 bits, the wave's step is its top 11) and how far it moves each run, twice.
    phase: i32,
    step: i32,
    depth: i32,
    /// A square wave's duty; a random wave's held value and whether it is in its first half.
    duty: i32,
    held: i32,
    first_half: bool,
}

/// A playing sound (the module's 0x120-byte sound struct, FUN_00003fd0 builds it).
struct Inst {
    handle: u32,
    bank: usize,
    sound: usize,
    /// +0xC the sound's volume (0..127), +0x3A its master volume (10 bits, 0x400 is full), +0x38 an offset on the volume of the
    /// voices (7 bits).
    vol: i32,
    volmult: i32,
    voloff: i32,
    /// +0x3E the pan angle given at the start (degrees), +0x3C an offset on it.
    pan_base: i32,
    pan_off: i32,
    /// +0x14 a transposition in 1/128 semitone, +0x36 the pitch bend (-0x8000..0x7FFF), +0x34/+0x35 the note and its fine step.
    transpose: i32,
    bend: i32,
    note: i32,
    note_fine: i32,
    /// +0x4C..+0x4F the sound's four variables.
    vars: [i8; 4],
    /// +0x4A the next command (-1: none left), +0x48 the ticks until it runs.
    cmd: i32,
    timer: i32,
    /// +0x50, +0x52, +0x54: after a command that picks a variant, the entries left in the variant and the entries to skip past
    /// the others once it is over.
    skip: Option<(i32, i32)>,
    /// +0x16 bit 5: no new voices (the sound is being stopped). Bit 6: a loop went back this tick (the next one waits a tick).
    stopping: bool,
    loop_seen: bool,
    /// +0x16 bit 2: the sound has been let go once (`stop`).
    let_go: bool,
    /// +0x16 bit 1: the sound is paused (its voices hold, its commands wait).
    paused: bool,
    /// +0x5C: the sound's four LFOs, and what they add to the transposition (+0x44) and the bend (+0x40).
    lfos: [Option<LfoSlot>; 4],
    /// The pan slide running on the sound (slide.rs).
    slide: Option<slide::Slide>,
    transpose_mod: i32,
    bend_mod: i32,
    voices: Vec<usize>,
    children: Vec<u32>,
    born: u64,
    /// The tick of the game's last change to the sound (0x21, 0x1B): a sound the game keeps alive gets one every field.
    held: u64,
    /// Why the sound ends, once something has ended it (a let-go, its last voice taken, its polyphony), for `Ended`.
    end: Option<String>,
}

/// A sound the game was keeping alive when it ended (changed within the last field, playing for over half a second), kept a
/// moment so that the game's starting it again says why it had ended (`Engine::play`, the stall of a held sound: the plasma
/// whip's sharp start again and again on Annihilation Nation and Command Center, 24 November 2026).
struct Ended {
    bank: usize,
    sound: usize,
    handle: u32,
    born: u64,
    held: u64,
    at: u64,
    why: String,
}

/// How a sound is started (the PlaySound command's arguments, 989snd FUN_00003fd0).
#[derive(Clone, Copy, Debug)]
pub struct Play {
    /// The sound's volume (0..127), None for the sound table's.
    pub volume: Option<i32>,
    /// The pan angle, None for the sound table's.
    pub pan: Option<i32>,
    /// The sound's master volume, 10 bits (0x400 is full).
    pub volmult: i32,
    /// An angle that replaces the pan the voices are made from (None: the pan above).
    pub pan_override: Option<i32>,
    /// A transposition in 1/128 semitone and an initial pitch bend.
    pub transpose: i32,
    pub bend: i32,
}

impl Default for Play {
    fn default() -> Play {
        Play { volume: None, pan: None, volmult: 0x400, pan_override: None, transpose: 0, bend: 0 }
    }
}

pub struct Engine {
    banks: Vec<Loaded>,
    insts: Vec<Inst>,
    voices: Vec<Option<Voice>>,
    next_handle: u32,
    /// The EE address of the status area the start command gave (the disc channel's busy and status words), 0 before it.
    status_addr: u32,
    /// Samples until the next tick, and ticks run.
    phase: usize,
    /// The output's rate (48 kHz, or the device's own with AUDIO > MIXING: `set_rate`), the samples until the next tick at another
    /// rate than 48 kHz (a tick is the output's rate / 240 samples), and whether the voices read their samples with the
    /// windowed sinc (`set_hq`).
    rate: f64,
    tick_left: f64,
    hq: bool,
    pub ticks: u64,
    /// The engine's variables (989snd 0x1D618..), shared by every sound.
    globals: [i8; 16],
    rng: Rand,
    /// The LFOs running (989snd's list at 0x1D5F8, at most 10, in the order they were set up): the sound and the slot.
    lfo_list: Vec<(u32, usize)>,
    /// A fault met while the sounds ran, for the next call to return.
    fault: Option<String>,
    /// The master volume (the group 0x10, 0..0x400), the volumes of the other groups (FUN_0001ac0c) and the output mode
    /// (FUN_0001ad48: 1 mono, 2 stereo, 0 surround; surround is played as stereo).
    pub master: i32,
    pub group_vol: [i32; 16],
    pub out_mode: i32,
    /// The voices each voice group may use (FUN_00018be0): groups 0 and 2..15 the second core's 24..47, group 1 the first core's.
    pub group_voices: [(usize, usize); 16],
    /// The stream engine (stream.rs) and its four ducking records, with the scale each voice group is at (16.16, 0x10000 full).
    streams: stream::Streams,
    reverb: reverb::Reverb,
    duck: [Option<stream::Duck>; 4],
    duck_scale: [i32; 15],
    /// Held sounds of weapon banks that ended in the last moment (`Ended`), and how many restarts the log has told of (at most
    /// `RESTARTS_TOLD`).
    ended: Vec<Ended>,
    restarts_told: u32,
    /// The ticks a weapon bank's sound was started at lately, and when the log last told of it repeating (`weapon_start`).
    starts: HashMap<(usize, usize), (Vec<u64>, u64)>,
    /// Sounds that left the playing ones while voices of theirs still sounded (their bank, sound, tick and why), for the census
    /// of long voices (`voice_census`), the voices told of (index and start), and the lines both have written.
    gone: HashMap<u32, (usize, usize, u64, String)>,
    long_told: std::collections::HashSet<(usize, u64)>,
    census_told: u32,
}

/// The restarts of held sounds the log tells of in a run.
const RESTARTS_TOLD: u32 = 300;
/// The lines the census of long voices and the repeats of weapon sounds write in a run.
const CENSUS_TOLD: u32 = 500;
/// The SBlk id of the weapons' banks (the gadget banks the game sends to IOP memory, 0x59).
const WEAPON_BANK: u32 = 0x3030_6e6d;

/// The module's random numbers (989snd FUN_0000becc, an R250 lag generator): the u16[250] table at module 0x1D7EC and the index
/// (a u16) at 0x1D7E8, as the module's .data holds them; never reseeded.
struct Rand {
    t: [u16; 250],
    i: usize,
}

impl Rand {
    /// The generator as the module image `m` (module addresses from 0) starts it. Its rand code is checked first, so another build
    /// of the module fails here instead of drawing from the wrong bytes.
    fn from_module(m: &[u8]) -> Result<Rand, String> {
        let w = |a: usize| m.get(a..a + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")));
        let code: Vec<Option<u32>> = (0..4).map(|k| w(0xbecc + 4 * k)).collect();
        if code != [0x3c07_0002, 0x00e0_4821, 0x84e6_d7e8, 0x94e8_d7e8].map(Some) {
            return Err(format!("989snd: no R250 rand at 0xBECC ({code:x?})"));
        }
        let raw = m.get(0x1d7ec..0x1d7ec + 500).ok_or("989snd: the R250 table is past the module image")?;
        let i = m.get(0x1d7e8..0x1d7ea).map(|b| u16::from_le_bytes([b[0], b[1]]) as usize).ok_or("989snd: the R250 index is past the module image")?;
        if i >= 250 {
            return Err(format!("989snd: R250 index {i}"));
        }
        let mut t = [0u16; 250];
        for (k, c) in raw.chunks_exact(2).enumerate() {
            t[k] = u16::from_le_bytes([c[0], c[1]]);
        }
        Ok(Rand { t, i })
    }

    /// i' = i < 0x93 ? i + 0x67 : i - 0x93; t[i] ^= t[i']; the index wraps after 0xF9.
    fn next(&mut self) -> u16 {
        let i = self.i;
        let j = if i < 0x93 { i + 0x67 } else { i - 0x93 };
        let v = self.t[i] ^ self.t[j];
        self.t[i] = v;
        self.i = if i > 0xf8 { 0 } else { i + 1 };
        v
    }
}

impl Engine {
    /// An engine with the sound modules' images (`uyafmt::sbank::sound_modules`): its random numbers start where 989snd's do and
    /// its reverb has libsd's modes.
    pub fn new(module: &uyafmt::sbank::Modules) -> Result<Engine, String> {
        // Every group starts with all 48 voices: 989snd's start (0x18AB4..0x18B2C) sets the voice mode to 0 and each of the 16
        // ranges (0x1E1B8) to 0..0x2F, and only command 0xD with 1 splits them; the game never sends 0xD and narrows groups 1, 2,
        // 4, 5 and 6 to the second core's voices with 0x4E, so the sound effects of group 0 have both cores' voices. The port
        // started from 0xD's split (23 November 2026): group 0 had 24 voices, and in the Annihilation Nation arena (the player's
        // save, Counting Ducks) new sounds took 636 voices from sounds still playing in 70 seconds, the gun's own among them, 21
        // ticks into its firing sound.
        let g = [(0, 47); 16];
        Ok(Engine {
            banks: Vec::new(),
            insts: Vec::new(),
            voices: (0..VOICES).map(|_| None).collect(),
            next_handle: 1,
            status_addr: 0,
            phase: 0,
            rate: RATE,
            tick_left: 0.0,
            hq: false,
            ticks: 0,
            globals: [0; 16],
            rng: Rand::from_module(&module.snd)?,
            lfo_list: Vec::new(),
            fault: None,
            master: 0x400,
            group_vol: [0x400; 16],
            out_mode: 0,
            group_voices: g,
            streams: stream::Streams::default(),
            reverb: reverb::Reverb::new(&module.snd, &module.sd)?,
            duck: [None; 4],
            duck_scale: [0x10000; 15],
            ended: Vec::new(),
            restarts_told: 0,
            starts: HashMap::new(),
            gone: HashMap::new(),
            long_told: std::collections::HashSet::new(),
            census_told: 0,
        })
    }

    /// The module's rand (FUN_0000becc), 16 bits.
    fn rand(&mut self) -> u32 {
        self.rng.next() as u32
    }

    /// Adds a bank; returns its handle (what the module returns for a load: the game hands it back to play a sound).
    pub fn load_bank(&mut self, bank: Bank) -> u32 {
        self.banks.push(Loaded { live: true, bank, pcm: HashMap::new(), picks: HashMap::new() });
        self.banks.len() as u32
    }

    /// The bank a handle of -1 means (989snd FUN_0000C484: the head of its list of banks, the first loaded of those still loaded);
    /// 0 when none is.
    pub fn first_bank(&self) -> u32 {
        self.banks.iter().position(|b| b.live).map_or(0, |b| b as u32 + 1)
    }

    /// The fault met while the sounds ran, if any (taken: the caller fails with it).
    pub fn take_fault(&mut self) -> Option<String> {
        self.fault.take()
    }

    pub fn bank(&self, handle: u32) -> Option<&Bank> {
        self.banks.get((handle as usize).checked_sub(1)?).map(|l| &l.bank)
    }

    /// Whether the sound with this handle is still playing.
    pub fn playing(&self, handle: u32) -> bool {
        if stream::is_stream(handle) {
            return self.stream_find(handle).is_some();
        }
        self.insts.iter().any(|i| i.handle == handle)
    }

    pub fn active_sounds(&self) -> usize {
        self.insts.len()
    }

    /// The voices sounding now: the tone's sample offset and envelope words, the pitch step and the gain on each side.
    pub fn voices_now(&self) -> Vec<(u32, u16, u16, f64, [f32; 2])> {
        self.voices.iter().flatten().map(|v| (v.tone.sample, v.tone.adsr1, v.tone.adsr2, v.step, v.gain)).collect()
    }

    // ---- starting a sound (989snd FUN_00003fd0) ----

    /// Starts sound `sound` of bank `bank` (handles: `load_bank`). Returns the sound's handle, or 0 when nothing started (no such
    /// bank or sound, as the module's FUN_0000C484 and FUN_00003FD0 answer them; a sound with no commands, or one that finished at
    /// once without a voice).
    pub fn play(&mut self, bank: u32, sound: usize, p: Play) -> Result<u32, String> {
        let Some(bi) = (bank as usize).checked_sub(1).filter(|&b| b < self.banks.len() && self.banks[b].live) else { return Ok(0) };
        let Some(def) = self.banks[bi].bank.sounds.get(sound).cloned() else { return Ok(0) };
        if def.cmds.is_empty() {
            return Ok(0);
        }
        // A sound with flag bit 3 is limited to its polyphony (FUN_0000dcfc): the new one takes the place of the quietest (flag
        // bit 4) or the oldest (flag bit 5) playing one, else it is not started.
        if def.flags & 8 != 0 && def.polyphony > 0 {
            let same: Vec<usize> = (0..self.insts.len()).filter(|&k| self.insts[k].bank == bi && self.insts[k].sound == sound).collect();
            if same.len() >= def.polyphony as usize {
                let mut victim: Option<usize> = None;
                for &k in &same {
                    let better = match victim {
                        None => true,
                        Some(v) => (def.flags & 0x10 != 0 && self.insts[k].volmult < self.insts[v].volmult) || (def.flags & 0x20 != 0 && self.insts[k].born < self.insts[v].born),
                    };
                    if better {
                        victim = Some(k);
                    }
                }
                let Some(v) = victim else { return Ok(0) };
                if (def.flags & 0x10 == 0 || p.volmult <= self.insts[v].volmult) && def.flags & 0x20 == 0 {
                    return Ok(0);
                }
                let h = self.insts[v].handle;
                self.insts[v].end = Some(format!("replaced by a new one of the same sound (its polyphony of {})", def.polyphony));
                self.stop(h, true);
            }
        }
        let vol = p.volume.unwrap_or(def.volume as i32);
        let pan = p.pan.unwrap_or(def.pan as i32);
        let volmult = if p.volmult == 0x7fff_ffff { 0x400 } else { p.volmult };
        // A pan override of -1 or -2 is no override.
        let pan_base = match p.pan_override {
            Some(a) if a != -1 && a != -2 => a,
            _ => pan,
        };
        // A sound's handle has the shape of the module's: bit 31 set (the sound is live), 5 in bits 24-28 (a sound).
        let handle = 0x8500_0000 | (self.next_handle & 0xff_ffff);
        self.next_handle += 1;
        let inst = Inst {
            handle,
            bank: bi,
            sound,
            vol,
            volmult,
            voloff: 0,
            pan_base,
            pan_off: 0,
            transpose: p.transpose,
            bend: p.bend,
            note: 0x3c,
            note_fine: 0,
            vars: [0; 4],
            cmd: 0,
            timer: def.cmds[0].delay as i32,
            skip: None,
            stopping: false,
            loop_seen: false,
            let_go: false,
            paused: false,
            lfos: [None; 4],
            slide: None,
            transpose_mod: 0,
            bend_mod: 0,
            voices: Vec::new(),
            children: Vec::new(),
            born: self.ticks,
            held: 0,
            end: None,
        };
        self.weapon_start(bi, sound);
        // The same sound held by the game until a moment ago: the log says why that one ended (UYA_SNDWATCH).
        let ticks = self.ticks;
        if let Some(e) = self.ended.iter().position(|e| e.bank == bi && e.sound == sound && ticks - e.at <= 60).filter(|_| watch_on()) {
            let e = self.ended.remove(e);
            if self.restarts_told < RESTARTS_TOLD {
                self.restarts_told += 1;
                println!(
                    "sound: tick {ticks}: bank {} (SBlk id {:#x}) sound {sound} started again {} ticks after the one held by the game ({:#x}, started at tick {}, changed by the game to tick {}) ended at tick {}: {}",
                    bi + 1,
                    self.banks[bi].bank.id,
                    ticks - e.at,
                    e.handle,
                    e.born,
                    e.held,
                    e.at,
                    e.why
                );
            }
        }
        self.insts.push(inst);
        let k = self.insts.len() - 1;
        // The commands whose delay has come run now.
        let mut finished = false;
        while self.insts[k].cmd != -1 && self.insts[k].timer == 0 {
            if self.step(k)? {
                finished = true;
                break;
            }
        }
        let i = &self.insts[k];
        if finished || (i.timer == 0 && i.voices.is_empty() && i.children.is_empty()) {
            let h = self.insts[k].handle;
            if !self.insts[k].voices.is_empty() {
                self.gone.insert(h, (bi, sound, self.ticks, "its commands ended as it started".to_string()));
            }
            self.insts.remove(k);
            self.lfo_unlist(h);
            return Ok(0);
        }
        Ok(handle)
    }

    /// Runs the command a sound is on (FUN_00006240); true when the sound is over.
    fn step(&mut self, k: usize) -> Result<bool, String> {
        let (bi, si) = (self.insts[k].bank, self.insts[k].sound);
        let ci = self.insts[k].cmd as usize;
        let cmd = self.banks[bi].bank.sounds[si].cmds[ci];
        let count = self.banks[bi].bank.sounds[si].cmds.len() as i32;
        let extra = match self.run_cmd(k, cmd, ci)? {
            None => return Ok(true),
            Some(extra) => extra,
        };
        let inst = &mut self.insts[k];
        if let Some((n, jump)) = inst.skip {
            if n == 1 {
                inst.skip = None;
                inst.cmd += jump;
            } else {
                inst.skip = Some((n - 1, jump));
            }
        }
        inst.cmd += 1;
        if inst.cmd > count - 1 {
            inst.cmd = -1;
            return Ok(inst.voices.is_empty() && inst.children.is_empty());
        }
        inst.timer = self.banks[bi].bank.sounds[si].cmds[inst.cmd as usize].delay as i32 + extra;
        Ok(false)
    }

    /// One command. None: the sound ends here (type 24, or a command that cannot go on); Some(extra): ticks added to the next
    /// command's delay.
    fn run_cmd(&mut self, k: usize, cmd: Command, ci: usize) -> Result<Option<i32>, String> {
        let a = cmd.arg.map(|b| b as i8);
        match cmd.ty {
            1 | 9 => self.tone(k, cmd.offset())?,
            // 20, 21 and 35 do nothing; 22 jumps back to the previous 21 (FUN_0000506c), 23 forward to the next 22.
            20 | 21 | 35 => {}
            22 => {
                let (bi, si) = (self.insts[k].bank, self.insts[k].sound);
                let cmds = &self.banks[bi].bank.sounds[si].cmds;
                match (0..self.insts[k].cmd as usize).rev().find(|&j| cmds[j].ty == 21) {
                    Some(j) => {
                        self.insts[k].cmd = j as i32 - 1;
                        // A second jump back in one tick waits a tick (FUN_0000506c's flag 0x40).
                        let again = std::mem::replace(&mut self.insts[k].loop_seen, true);
                        return Ok(Some(again as i32));
                    }
                    None => return Ok(None),
                }
            }
            23 => {
                let (bi, si) = (self.insts[k].bank, self.insts[k].sound);
                let cmds = &self.banks[bi].bank.sounds[si].cmds;
                match (self.insts[k].cmd as usize + 1..cmds.len()).find(|&j| cmds[j].ty == 22) {
                    Some(j) => self.insts[k].cmd = j as i32,
                    None => return Ok(None),
                }
            }
            24 => return Ok(None),
            // 25 picks a variant at random (not the last one), 39 the next in turn: `a[0]` variants of `a[1]` entries each,
            // `a[2]` the last pick (kept in the bank's own entry).
            25 | 39 => {
                if self.insts[k].skip.is_some() {
                    return Ok(None);
                }
                let (bi, si) = (self.insts[k].bank, self.insts[k].sound);
                let (variants, size) = (a[0] as i32, a[1] as i32);
                if variants <= 0 {
                    return Err(format!("command {ci}: {variants} variants"));
                }
                let last = *self.banks[bi].picks.get(&(si, ci)).unwrap_or(&a[2]) as i32;
                let pick = if cmd.ty == 25 {
                    let mut p = (self.rand() % variants as u32) as i32;
                    if p == last {
                        p += 1;
                        if p == variants {
                            p = 0;
                        }
                    }
                    self.banks[bi].picks.insert((si, ci), p as i8);
                    p
                } else {
                    let next = if last + 1 == variants { 0 } else { last + 1 };
                    self.banks[bi].picks.insert((si, ci), next as i8);
                    last
                };
                let inst = &mut self.insts[k];
                inst.cmd += pick * size;
                inst.skip = Some((size + 1, (variants - 1 - pick) * size));
            }
            26 => return Ok(Some((self.rand() % (cmd.offset() + 1)) as i32)),
            27 => {
                let r = self.rand();
                let bend = ((((r % 0x7fff) * 0xffff) / 0x7fff) as i32 - 0x8000) * a[0] as i32 / 100;
                self.set_bend(k, bend as i16 as i32);
            }
            28 => self.set_bend(k, a[0] as i32 * 0x7fff / 0x7f),
            29 => {
                let b = (self.insts[k].bend + a[0] as i32 * 0x7fff / 0x7f).clamp(-0x8000, 0x7fff);
                self.set_bend(k, b);
            }
            30 => self.set_var(k, a[0] as i32, a[1]),
            31 => {
                let n = a[2] as i32 - a[1] as i32 + 1;
                let v = (self.rand() % n.max(1) as u32) as i32 + a[1] as i32;
                self.set_var(k, a[0] as i32, v as i8);
            }
            32 => self.add_var(k, a[0] as i32, 1),
            33 => self.add_var(k, a[0] as i32, -1),
            34 => {
                // Compare a variable: 0 less than, 1 equal to, 2 greater than the value; when it does not hold the next entry
                // is skipped.
                let v = self.get_var(k, a[0] as i32) as i32;
                let ok = match a[1] {
                    0 => v < a[2] as i32,
                    1 => v == a[2] as i32,
                    2 => v > a[2] as i32,
                    _ => false,
                };
                if !ok {
                    self.insts[k].cmd += 1;
                }
            }
            36 => {
                let (bi, si) = (self.insts[k].bank, self.insts[k].sound);
                let cmds = &self.banks[bi].bank.sounds[si].cmds;
                match (0..cmds.len()).find(|&j| cmds[j].ty == 35 && cmds[j].arg[0] as i8 == a[0]) {
                    Some(j) => self.insts[k].cmd = j as i32 - 1,
                    None => return Ok(None),
                }
            }
            // 38 waits until the sound's voices are over: the same command again next tick.
            38 => {
                if !self.insts[k].voices.is_empty() {
                    self.insts[k].cmd -= 1;
                    return Ok(Some(1));
                }
            }
            40 => self.add_var(k, a[1] as i32, a[0]),
            // 41 lets the voices go (they fall at their release rate), 42 stops them at once.
            41 => self.release_voices(k, false),
            42 => self.release_voices(k, true),
            43 => {
                let last = self.banks[self.insts[k].bank].bank.sounds[self.insts[k].sound].cmds.len() as i32 - 1;
                self.insts[k].cmd = last;
            }
            5 => self.child(k, cmd.offset())?,
            4 => self.lfo(k, cmd.offset())?,
            // Types 0, 2 and 3 return at once (989snd 0x4A24, 0x4F24, 0x4F2C); 10 to 19 report error 0x5F and end the sound
            // (FUN_00006210), as every type past the table's 45 does (FUN_00006240). Only the version 1 gadget banks, read with
            // version 3's 8-byte commands, reach them (uya sndbanks).
            0 | 2 | 3 => {}
            10..=19 | 0x2d..=0xff => return Ok(None),
            // 6, 7, 8, 37 and 44 are the module's (FUN_000060EC, FUN_00005B90, FUN_00005BE8, FUN_00005848, 0x5B2C); no bank on
            // the disc reaches them.
            t => return Err(format!("command {ci}: type {t}, which no bank on the disc uses")),
        }
        Ok(Some(0))
    }

    // ---- variables ----

    /// A variable named by a command's index: 0..3 the sound's, below 0 the engine's (index !i).
    fn get_var(&self, k: usize, i: i32) -> i8 {
        if i >= 0 {
            self.insts[k].vars[i as usize & 3]
        } else {
            self.globals[(!i) as usize & 15]
        }
    }

    fn set_var(&mut self, k: usize, i: i32, v: i8) {
        if i >= 0 {
            self.insts[k].vars[i as usize & 3] = v;
        } else {
            self.globals[(!i) as usize & 15] = v;
        }
    }

    fn add_var(&mut self, k: usize, i: i32, d: i8) {
        let v = (self.get_var(k, i) as i32 + d as i32).clamp(-128, 127) as i8;
        self.set_var(k, i, v);
    }

    /// A value a tone or child record gives that is a number or names a variable (FUN_00004a2c): from 0 a number, -1..-4 the sound's
    /// variables, -5 a random number, below that the engine's variables.
    fn value(&mut self, k: usize, v: i32) -> i32 {
        if v >= 0 {
            v
        } else if v == -5 {
            (self.rand() & 0x7f) as i32
        } else if v >= -4 {
            self.insts[k].vars[(!v) as usize] as i32
        } else {
            self.globals[((-6 - v) as usize) & 15] as i32
        }
    }

    // ---- a tone (989snd FUN_00004a2c) ----

    fn tone(&mut self, k: usize, off: u32) -> Result<(), String> {
        if self.insts[k].stopping {
            return Ok(());
        }
        let (bi, si) = (self.insts[k].bank, self.insts[k].sound);
        let tone = *self.banks[bi].bank.tones.get(&off).ok_or_else(|| format!("no tone record at +{off:#x}"))?;
        let group = self.banks[bi].bank.sounds[si].group as usize & 15;
        let priority = priority(self.insts[k].volmult, &tone);
        let Some(vi) = self.alloc(group, priority, tone.flags & 0x20 != 0) else { return Ok(()) };
        let tone_volume = self.value(k, tone.volume as i32).max(0);
        // A tone's pan is an angle, or names a variable (scaled to 360 degrees) or is random.
        let tp = tone.pan as i32;
        let tone_pan = if tp >= 0 {
            tp
        } else if tp == -5 {
            (self.rand() % 360) as i32
        } else {
            self.value(k, tp) * 360 / 0x7f
        }
        .rem_euclid(360);
        let pcm = self.pcm(bi, &tone)?;
        let handle = self.insts[k].handle;
        let mut v = Voice::new(pcm, tone, handle, priority, self.ticks, tone_volume, tone_pan);
        self.set_voice(k, &mut v);
        self.voices[vi] = Some(v);
        self.insts[k].voices.push(vi);
        Ok(())
    }

    /// The decoded sample of a tone.
    fn pcm(&mut self, bi: usize, tone: &Tone) -> Result<Arc<Pcm>, String> {
        let l = &mut self.banks[bi];
        if let Some(p) = l.pcm.get(&tone.sample) {
            return Ok(p.clone());
        }
        let p = Arc::new(voice::decode(&l.bank.samples, tone.sample as usize)?);
        l.pcm.insert(tone.sample, p.clone());
        Ok(p)
    }

    /// A voice for a tone of a sound in `group` (FUN_00019250): a free one in the group's range (the chip's voices the game
    /// gave it), else a free one of the engine's own past them (`CHIP_VOICES..VOICES`, every group's), else one taken from
    /// another sound: a fading one (let go: its sound is done with it) first, whatever its priority; else a playing one of the
    /// lowest priority, if that is not above the new tone's, the quietest of those (`Voice::loudness`), then the oldest.
    /// 989snd has the chip's 48 and takes the oldest of the lowest priority, however loud: in the Annihilation Nation arena (the
    /// player's sessions, 24 November 2026) all 48 voices were busy and the oldest were the plasma whip's held sound, at full
    /// volume beside Ratchet, while four voices were fading; the held sound lost both its voices and ended, and the game, which
    /// keeps it alive while the whip is out, started it again with its sharp start (the whip's equip sound), again and again
    /// through a fight. Taking the quietest of the lowest priority first was not enough there: the whip's tones are of the
    /// lowest priority, 0, and the fight used up the other voices of priority 0.
    fn alloc(&mut self, group: usize, priority: i32, _locked: bool) -> Option<usize> {
        let (lo, hi) = self.group_voices[group];
        let range = || (lo..=hi).chain(CHIP_VOICES..VOICES);
        if let Some(v) = range().find(|&v| self.voices[v].is_none()) {
            return Some(v);
        }
        let victim = range().filter(|&v| self.voices[v].as_ref().is_some_and(|x| !x.locked)).min_by_key(|&v| {
            let x = self.voices[v].as_ref().expect("a voice");
            (!x.env.releasing(), x.priority, x.loudness(), x.born)
        })?;
        let x = self.voices[victim].as_ref().expect("a voice");
        if !x.env.releasing() && (priority < x.priority || x.priority >= 0x7e) {
            if log_on() {
                println!("snd: tick {}: no voice in group {group} ({lo}..={hi} and {CHIP_VOICES}..{VOICES}) for a tone of priority {priority}", self.ticks);
            }
            return None;
        }
        let owner = x.owner;
        if log_on() {
            let fading = range().filter(|&v| self.voices[v].as_ref().is_some_and(|x| x.env.releasing())).count();
            let loud = range().filter_map(|v| self.voices[v].as_ref().filter(|x| !x.locked).map(|x| x.loudness()));
            let (quietest, loudest) = loud.fold((u32::MAX, 0), |(a, b), l| (a.min(l), b.max(l)));
            let ends = self.insts.iter().find(|i| i.handle == owner).is_some_and(|i| i.voices == [victim] && i.cmd == -1 && i.children.is_empty());
            println!(
                "snd: tick {}: voice {victim} of sound {owner:#x} (priority {}, {} at level {:#x}, loudness {:#x}, {} ticks old{}) taken for a tone of priority {priority}; {fading} of the group's voices fading, its quietest {quietest:#x}, its loudest {loudest:#x}",
                self.ticks,
                x.priority,
                if x.env.releasing() { "fading" } else { "playing" },
                x.env.envx(),
                x.loudness(),
                self.ticks - x.born,
                if ends { ", its sound ends" } else { "" }
            );
        }
        self.voices[victim] = None;
        let ticks = self.ticks;
        if let Some(i) = self.insts.iter_mut().find(|i| i.handle == owner) {
            i.voices.retain(|&v| v != victim);
            if i.voices.is_empty() && i.end.is_none() {
                i.end = Some(format!("its last voice was taken at tick {ticks} for a tone of priority {priority}"));
            }
        }
        Some(victim)
    }

    /// The volumes and pitch of a voice from its sound's present state (989snd FUN_00004a2c, FUN_0001b140, FUN_0001a874).
    fn set_voice(&mut self, k: usize, v: &mut Voice) {
        let inst = &self.insts[k];
        let flags = self.banks[inst.bank].bank.sounds[inst.sound].flags;
        // The angle of the sound plus the tone's (a sound with flag bit 2 draws the tone's toward the front as it gets quiet).
        let angle = (inst.pan_base + inst.pan_off).rem_euclid(360);
        let tone_angle = if flags & 4 != 0 { pan_fade(v.tone_pan, inst.volmult) } else { v.tone_pan };
        let vol_cur = (inst.voloff + 0x7f).clamp(0, 0x7f);
        let sq = |x: i32| x * x / 0x7f;
        let (v1, v2, v4, v6) = (sq(inst.volmult * 0x7f >> 10), sq(inst.vol), sq(vol_cur), sq(v.tone_volume));
        let level = ((((v2 * 0x102 * v6) / 0x7f) * v4) / 0x7f * v1 / 0x7f).min(0x7ffe);
        // The voice group's volume (989snd FUN_0001b82c): the square of its volume over 0x400, as a share of 0x400.
        let group_gain = self.group_gain(self.banks[inst.bank].bank.sounds[inst.sound].group as usize & 15);
        // A voice's volume register holds half of the volume it means (989snd writes the value >> 1 and the chip doubles it): the
        // gain is the value over 0x8000.
        v.gain = pan(level, (tone_angle + angle).rem_euclid(360)).map(|g| g as f32 / 16384.0 * group_gain);
        // The voice's priority follows the sound's volume as its volume does (989snd 0x5AAC writes it to the voice record with the
        // volumes): a tone whose priority falls as its sound gets quieter is the first a new sound takes once its sound is far.
        // The port kept the priority the voice started with.
        v.priority = priority(inst.volmult, &v.tone);
        // The pitch: the sound's note and fine step, its transposition and its bend (up to the tone's range).
        let b = (inst.bend + inst.bend_mod).clamp(-0x8000, 0x7fff);
        let bend = if b < 0 { v.tone.bend_down as i32 * b * 0x80 >> 15 } else { v.tone.bend_up as i32 * b * 0x80 / 0x7fff };
        let note_fine = inst.note * 128 + inst.note_fine + inst.transpose + inst.transpose_mod + bend;
        v.step = voice::pitch(v.tone.centre_note as i32, v.tone.fine_tune as i32, note_fine);
    }

    fn refresh(&mut self, k: usize) {
        for vi in self.insts[k].voices.clone() {
            if let Some(mut v) = self.voices[vi].take() {
                self.set_voice(k, &mut v);
                self.voices[vi] = Some(v);
            }
        }
    }

    fn set_bend(&mut self, k: usize, bend: i32) {
        self.insts[k].bend = bend;
        self.refresh(k);
    }

    // ---- LFOs (989snd FUN_00004f34, FUN_00002a70, FUN_00002574) ----

    /// A type 4 command: sets up (or, with no target, stops) one of the sound's four LFOs from the record.
    fn lfo(&mut self, k: usize, off: u32) -> Result<(), String> {
        let bi = self.insts[k].bank;
        let r = self.banks[bi].bank.lfos.get(&off).ok_or_else(|| format!("no LFO record at +{off:#x}"))?.raw;
        let slot = r[0] as usize;
        if slot > 3 {
            return Err(format!("LFO slot {slot}"));
        }
        if r[1] == 0 {
            self.insts[k].lfos[slot] = None;
            let h = self.insts[k].handle;
            self.lfo_list.retain(|&e| e != (h, slot));
            return Ok(());
        }
        let (kind, target) = (r[3], r[1]);
        if !(1..=6).contains(&target) {
            return Err(format!("LFO target {target}"));
        }
        if kind > 5 {
            return Err(format!("LFO wave {kind}"));
        }
        let other = r[2] as usize;
        if (5..=6).contains(&target) && other > 3 {
            return Err(format!("LFO target {target} on slot {other}"));
        }
        let amp = u16::from_le_bytes([r[6], r[7]]) as i32;
        let phase = if r[8] & 2 == 0 { (u16::from_le_bytes([r[10], r[11]]) as i32) << 16 } else { ((self.rand() & 0x7ff) as i32) << 16 };
        let held = if kind == 5 { -((self.rand() & 0x7fff) as i32) * (self.rand() & 1) as i32 } else { 0 };
        let duty = if kind == 2 { u16::from_le_bytes([r[4], r[5]]) as i32 } else { 0 };
        let step = i32::from_le_bytes([r[12], r[13], r[14], r[15]]);
        let mut l = LfoSlot { kind, target, other, amp, amp0: amp, step0: step, invert: r[8] & 1 != 0, phase, step, depth: 0, duty, held, first_half: true };
        l.depth = self.lfo_depth(k, &l);
        self.insts[k].lfos[slot] = Some(l);
        // FUN_00002378: into the running list (ten at most: past that it does not run) and, when it is in it, run once now.
        let h = self.insts[k].handle;
        if !self.lfo_list.contains(&(h, slot)) {
            if self.lfo_list.len() == 10 {
                return Ok(());
            }
            self.lfo_list.push((h, slot));
        }
        self.lfo_run(k, slot);
        Ok(())
    }

    /// An LFO's depth from its amplitude and target (989snd FUN_00002A70): the sound's volume times the amplitude over 1024 for the
    /// volume, scaled amplitudes for the pan, transposition and bend, none for the targets that move another LFO.
    fn lfo_depth(&self, k: usize, l: &LfoSlot) -> i32 {
        let amp = l.amp as u16 as i32;
        match l.target {
            1 => self.banks[self.insts[k].bank].bank.sounds[self.insts[k].sound].volume as i32 * amp >> 10,
            2 => amp * 0x2d >> 8,
            3 => amp * 0x17d >> 6,
            4 => amp * 0x7fff >> 10,
            _ => l.depth,
        }
    }

    /// Stops an LFO: out of the running list (989snd FUN_00002450).
    fn lfo_unlist(&mut self, handle: u32) {
        self.lfo_list.retain(|&(h, _)| h != handle);
    }

    /// The value of a wave at a step of its 2048 (989snd's wave functions at 0x217C..0x22E0): 0 none, 1 sine, 2 square, 3
    /// triangle, 4 sawtooth, 5 a random value held for each half.
    fn wave(&mut self, k: usize, slot: usize, step: i32) -> i32 {
        let l = self.insts[k].lfos[slot].expect("a running LFO");
        match l.kind {
            0 => 0,
            1 => {
                let q = |p: i32| (0x7fff as f64 * (p as f64 * std::f64::consts::FRAC_PI_2 / 512.0).sin()).round() as i32;
                match step {
                    0..=0x200 => q(step),
                    0x201..=0x400 => q(0x400 - step),
                    0x401..=0x600 => -q(step - 0x400),
                    _ => -q(0x800 - step),
                }
            }
            2 => {
                if step < l.duty {
                    -0x7fff
                } else {
                    0x7fff
                }
            }
            3 => {
                if step < 0x200 {
                    step * 0x7fff / 0x200
                } else if step < 0x600 {
                    0x7fff - (step - 0x200) * 0x7fff * 2 / 0x400
                } else {
                    (step - 0x600) * 0x7fff / 0x200 - 0x7fff
                }
            }
            4 => {
                if step < 0x400 {
                    step * 0x7fff / 0x3ff
                } else {
                    (step - 0x400) * 0x7fff / 0x400 - 0x7fff
                }
            }
            _ => {
                let mut l = l;
                if step < 0x400 {
                    if !l.first_half {
                        l.first_half = true;
                        l.held = -((self.rand() & 0x7fff) as i32) * (self.rand() & 1) as i32;
                    }
                } else if l.first_half {
                    l.first_half = false;
                    l.held = (((self.rand() & 0x7fff) as i32) - 0x3fff) * 2;
                }
                self.insts[k].lfos[slot] = Some(l);
                l.held
            }
        }
    }

    /// Runs one LFO once (989snd FUN_00002574): its wave steps on and moves what it targets. True when the sound's voices are to
    /// be set again (its volume, pan, transposition or bend moved).
    fn lfo_run(&mut self, k: usize, slot: usize) -> bool {
        let Some(l) = self.insts[k].lfos[slot] else { return false };
        let w = self.wave(k, slot, l.phase >> 16);
        let mut l = self.insts[k].lfos[slot].expect("a running LFO");
        l.phase = l.phase.wrapping_add(l.step.wrapping_mul(2));
        if l.phase > 0x7ff_ffff {
            l.phase -= 0x800_0000;
        }
        self.insts[k].lfos[slot] = Some(l);
        let w = if l.invert { -w } else { w };
        let inst = &mut self.insts[k];
        match l.target {
            1 => {
                let v = (l.depth.wrapping_mul(w - 0x7fff) >> 16) as i16 as i32;
                let moved = inst.voloff != v;
                inst.voloff = v;
                moved
            }
            2 => {
                let v = (l.depth.wrapping_mul(w) >> 15) as i16 as i32;
                let moved = inst.pan_off != v;
                inst.pan_off = v;
                moved
            }
            3 => {
                let v = (w.wrapping_mul(l.depth) >> 15) as i16 as i32;
                let moved = inst.transpose_mod != v;
                inst.transpose_mod = v;
                moved
            }
            4 => {
                let v = (w.wrapping_mul(l.depth) >> 15) as i16 as i32;
                let moved = inst.bend_mod != v;
                inst.bend_mod = v;
                moved
            }
            // The rate of the LFO in slot `other`: its record's step moved by this wave times the amplitude.
            5 => {
                if let Some(t) = inst.lfos[l.other].as_mut() {
                    let s0 = t.step0 as u32;
                    let f = ((s0 >> 8).wrapping_mul(l.amp as u16 as u32) >> 10) as i32;
                    t.step = s0.wrapping_add((f.wrapping_mul(w) >> 15).wrapping_mul(0x100) as u32) as i32;
                }
                false
            }
            // Its depth: the record's amplitude moved the same way, and the depth worked out again from it.
            _ => {
                if let Some(mut t) = inst.lfos[l.other] {
                    let f = ((t.amp0 as u32).wrapping_mul(l.amp as u16 as u32) >> 10) as i32;
                    t.amp = (t.amp0 as i16).wrapping_add((f.wrapping_mul(w) >> 15) as i16) as u16 as i32;
                    t.depth = self.lfo_depth(k, &t);
                    self.insts[k].lfos[l.other] = Some(t);
                }
                false
            }
        }
    }

    // ---- a child sound (989snd FUN_00005ddc) ----

    fn child(&mut self, k: usize, off: u32) -> Result<(), String> {
        if self.insts[k].stopping || self.insts[k].vol == 0 {
            return Ok(());
        }
        let bi = self.insts[k].bank;
        let kid = *self.banks[bi].bank.kids.get(&off).ok_or_else(|| format!("no child record at +{off:#x}"))?;
        let vol = self.value(k, kid.volume).abs().min(0x7f);
        let angle = if kid.pan >= 0 {
            kid.pan
        } else if kid.pan == -5 {
            (self.rand() % 360) as i32
        } else {
            self.value(k, kid.pan).abs().min(0x7f) * 360 / 0x7f
        };
        if kid.sound < 0 {
            // FUN_00005DDC looks the name at +0x10 up among the loaded banks; no bank on the disc has such a child (uya sndbanks).
            return Err(format!("a child sound named in another bank (record +{off:#x})"));
        }
        let inst = &self.insts[k];
        let p = Play { volume: Some(vol), pan: Some(angle), volmult: inst.volmult * inst.vol / 0x7f, pan_override: Some(inst.pan_base), transpose: inst.transpose, bend: inst.bend };
        let h = self.play(bi as u32 + 1, kid.sound as usize, p)?;
        if h != 0 {
            if let Some(c) = self.insts.iter().position(|i| i.handle == h) {
                self.insts[c].vars = self.insts[k].vars;
            }
            self.insts[k].children.push(h);
        }
        Ok(())
    }

    // ---- stopping ----

    fn release_voices(&mut self, k: usize, kill: bool) {
        for vi in std::mem::take(&mut self.insts[k].voices) {
            if kill {
                self.voices[vi] = None;
            } else if let Some(v) = self.voices[vi].as_mut() {
                v.env.release();
                // The voice stays with the sound until it has fallen silent.
                self.insts[k].voices.push(vi);
            }
        }
    }

    /// Stops a sound (FUN_0000e8a4, FUN_00004840): its voices are let go (`kill`: stopped at once), and a sound that has a release
    /// section (the first command of type 43 that is not the last) runs it. A sound let go a second time ends there: FUN_0000e8a4
    /// finds +0x16 bit 2 set by the first (0xE928..0xE93C), lets its voices go without the release section (FUN_00004840 with
    /// kill 1, 0xE9C8) and frees it while they fade (0xEA60 FUN_00019F4C, 0xEA80 FUN_0000DF50), so the next 0x19 answers 0. The
    /// port ran the release section again on every let-go: let go each field, a sound with one (the Flux Rifle's zoom, sound 2
    /// of WAD7 bank 3076) played its release tone over and over and never ended until its bank was unloaded.
    pub fn stop(&mut self, handle: u32, kill: bool) {
        if stream::is_stream(handle) {
            self.stream_stop(handle);
            return;
        }
        let Some(k) = self.insts.iter().position(|i| i.handle == handle) else { return };
        for c in self.insts[k].children.clone() {
            self.stop(c, kill);
        }
        let Some(k) = self.insts.iter().position(|i| i.handle == handle) else { return };
        if !kill && self.insts[k].let_go {
            self.note_end(k, "let go a second time (command 0x15)");
            self.release_voices(k, false);
            // The voices fade on their own (`tick` frees each once it is silent); the sound is gone now.
            self.insts.remove(k);
            self.lfo_unlist(handle);
            for i in &mut self.insts {
                i.children.retain(|&c| c != handle);
            }
            return;
        }
        self.insts[k].let_go = true;
        if self.insts[k].end.is_none() {
            self.insts[k].end = Some(if kill { "stopped at once".to_string() } else { "let go (command 0x15)".to_string() });
        }
        let (bi, si) = (self.insts[k].bank, self.insts[k].sound);
        let cmds = &self.banks[bi].bank.sounds[si].cmds;
        let release = cmds.iter().position(|c| c.ty == 43).filter(|&j| j != cmds.len() - 1);
        self.release_voices(k, kill);
        match release {
            Some(j) if !kill => {
                self.insts[k].cmd = j as i32 + 1;
                self.insts[k].timer = 0;
                self.insts[k].skip = None;
            }
            _ => {
                self.insts[k].cmd = -1;
                self.insts[k].stopping = true;
            }
        }
        if kill {
            self.insts[k].voices.clear();
        }
    }

    // ---- what the game's commands do to a playing sound (rpc.rs) ----

    fn index_of(&self, handle: u32) -> Option<usize> {
        self.insts.iter().position(|i| i.handle == handle)
    }

    /// Unloads a bank: its sounds stop and leave the playing sounds, and its data is dropped (the handle stays, empty). A handle
    /// that is no loaded bank does nothing (989snd FUN_00009500 reports error 0x14 or 0x15 and returns). The sounds leave at once,
    /// as a finished sound leaves at a tick (`tick`): left there, stopped, until the next tick, one was still found by the game's
    /// next command for it, which read the bank's sounds after they were dropped (the player's crash, 16 November 2026, on
    /// Florana: `stop`, "index out of bounds: the len is 0 but the index is 0"); a later command for it now finds no sound.
    pub fn unload_bank(&mut self, handle: u32) {
        let Some(bi) = (handle as usize).checked_sub(1).filter(|&b| b < self.banks.len() && self.banks[b].live) else {
            return;
        };
        self.banks[bi].live = false;
        let gone: Vec<u32> = self.insts.iter().filter(|i| i.bank == bi).map(|i| i.handle).collect();
        for &h in &gone {
            self.stop(h, true);
        }
        self.insts.retain(|i| i.bank != bi);
        for &h in &gone {
            self.lfo_unlist(h);
        }
        for i in &mut self.insts {
            i.children.retain(|c| !gone.contains(c));
        }
        if !gone.is_empty() {
            println!("sound: tick {}: bank {handle} unloaded with {} of its sounds playing", self.ticks, gone.len());
        }
        let b = &mut self.banks[bi].bank;
        b.sounds.clear();
        b.tones.clear();
        b.lfos.clear();
        b.kids.clear();
        b.samples.clear();
        self.banks[bi].pcm.clear();
    }

    /// The volume of a voice group, 0..0x400 (989snd FUN_0001ac0c); group 0x10 is the master volume. Group 15 does nothing (the
    /// module reports error 0x53); a group past 0x10 is an error (the module writes past its table of 17).
    pub fn set_group_volume(&mut self, group: usize, vol: i32) -> Result<(), String> {
        let vol = vol.clamp(0, 0x400);
        match group {
            0x10 => self.master = vol,
            0xf => {}
            g if g < 16 => {
                self.group_vol[g] = vol;
                self.refresh_group(g);
            }
            g => return Err(format!("a volume for voice group {g} (989snd writes it past its table of 17)")),
        }
        Ok(())
    }

    /// Pauses or resumes a sound and its children (989snd FUN_0000e490, FUN_0000e4fc).
    pub fn pause(&mut self, handle: u32, on: bool) {
        if stream::is_stream(handle) {
            self.stream_pause(handle, on);
            return;
        }
        let Some(k) = self.index_of(handle) else { return };
        for c in self.insts[k].children.clone() {
            self.pause(c, on);
        }
        let Some(k) = self.index_of(handle) else { return };
        self.insts[k].paused = on;
        for &vi in &self.insts[k].voices {
            if let Some(v) = self.voices[vi].as_mut() {
                v.paused = on;
            }
        }
    }

    /// Pauses or resumes every sound whose voice group is in `mask` (a bit a group).
    pub fn pause_groups(&mut self, mask: u32, on: bool) {
        let hs: Vec<u32> = self.insts.iter().filter(|i| mask >> (self.banks[i.bank].bank.sounds[i.sound].group & 31) & 1 != 0 && i.paused != on).map(|i| i.handle).collect();
        for h in hs {
            self.pause(h, on);
        }
        let ss: Vec<u32> = self.streams_in_groups(mask);
        for h in ss {
            self.stream_pause(h, on);
        }
    }

    /// Stops every sound and every voice at once (989snd FUN_0000e7a0).
    pub fn stop_all(&mut self) {
        self.stream_end_all();
        self.insts.clear();
        self.lfo_list.clear();
        for v in self.voices.iter_mut() {
            *v = None;
        }
    }

    /// A sound's master volume and pan (989snd FUN_000063b4): a negative volume is a master volume of its size, 0x7FFFFFFF leaves
    /// it, a pan of -1 is the sound's own, -2 leaves it. The sound's children follow.
    pub fn set_volume_pan(&mut self, handle: u32, vol: i32, pan: i32) {
        if stream::is_stream(handle) {
            self.stream_set(handle, vol, pan);
            return;
        }
        let Some(k) = self.index_of(handle) else { return };
        if vol < 0 {
            self.insts[k].volmult = -vol;
        } else if vol != 0x7fff_ffff {
            self.insts[k].volmult = vol;
        }
        if pan == -1 {
            self.insts[k].pan_base = self.banks[self.insts[k].bank].bank.sounds[self.insts[k].sound].pan as i32;
        } else if pan != -2 {
            self.insts[k].pan_base = pan;
        }
        let kids_vol = self.insts[k].volmult * self.insts[k].vol / 0x7f;
        for c in self.insts[k].children.clone() {
            self.set_volume_pan(c, kids_vol, pan);
        }
        if let Some(k) = self.index_of(handle) {
            self.refresh(k);
        }
    }

    /// A sound's transposition in 1/128 semitone (989snd FUN_0000ca64).
    pub fn set_transpose(&mut self, handle: u32, t: i32) {
        let Some(k) = self.index_of(handle) else { return };
        self.insts[k].transpose = t;
        for c in self.insts[k].children.clone() {
            self.set_transpose(c, t);
        }
        if let Some(k) = self.index_of(handle) {
            self.refresh(k);
        }
    }

    /// A sound's pitch bend (989snd FUN_000067d8): its children follow.
    pub fn set_bend_of(&mut self, handle: u32, bend: i32) {
        let Some(k) = self.index_of(handle) else { return };
        for c in self.insts[k].children.clone() {
            self.set_bend_of(c, bend);
        }
        if let Some(k) = self.index_of(handle) {
            self.set_bend(k, bend);
        }
    }

    // ---- the clock ----

    /// One tick (240 Hz): each sound's timer runs down and its commands run (989snd's per-tick update); finished sounds are
    /// dropped.
    fn tick(&mut self) {
        self.ticks += 1;
        // Voices that ended since the last tick leave their sounds.
        for i in &mut self.insts {
            i.voices.retain(|&v| self.voices[v].as_ref().is_some_and(|x| x.active));
        }
        // The running LFOs, on every second tick, in the list's order (989snd FUN_000029D4; a paused sound's too).
        if self.ticks % 2 == 0 {
            for (h, slot) in self.lfo_list.clone() {
                if let Some(k) = self.index_of(h) {
                    if self.lfo_run(k, slot) {
                        self.refresh(k);
                    }
                }
            }
        }
        for v in self.voices.iter_mut() {
            if v.as_ref().is_some_and(|x| !x.active) {
                *v = None;
            }
        }
        self.stream_tick();
        self.reverb.tick();
        if self.ticks % 240 == 0 && watch_on() {
            self.voice_census();
        }
        if self.ticks % 4 == 0 {
            self.duck_tick();
        }
        let mut k = 0;
        while k < self.insts.len() {
            let done = self.update(k);
            if !done && !self.insts[k].paused {
                self.slide_run(self.insts[k].handle);
            }
            if done {
                // A finished sound has no voices and no children left to wait for.
                let h = self.insts[k].handle;
                self.note_end(k, "its voices ended");
                self.insts.remove(k);
                self.lfo_unlist(h);
                for i in &mut self.insts {
                    i.children.retain(|&c| c != h);
                }
            } else {
                k += 1;
            }
        }
    }

    /// A weapon bank's sound started: four starts or more in the last 3 s at a beat (each at least an eighth of a second after
    /// the one before) are told once in 10 s, with the sound's commands (the plasma whip's sharp sound again and again, the
    /// player's sessions of 24 November 2026, through a cutscene too: whatever is starting it, and what it is).
    fn weapon_start(&mut self, bi: usize, sound: usize) {
        if !watch_on() || self.banks[bi].bank.id != WEAPON_BANK {
            return;
        }
        let ticks = self.ticks;
        let (at, told) = self.starts.entry((bi, sound)).or_insert((Vec::new(), 0));
        at.retain(|&t| ticks - t <= 720);
        at.push(ticks);
        let gaps: Vec<u64> = at.windows(2).map(|w| w[1] - w[0]).collect();
        if at.len() < 4 || gaps.iter().any(|&g| g < 30) || (*told != 0 && ticks - *told < 2400) || self.census_told >= CENSUS_TOLD {
            return;
        }
        *told = ticks;
        self.census_told += 1;
        let b = &self.banks[bi].bank;
        let cmds: Vec<String> = b.sounds[sound]
            .cmds
            .iter()
            .map(|c| match c.ty {
                1 | 9 => match b.tones.get(&c.offset()) {
                    Some(t) => format!("tone(sample {:#x} adsr {:04x}/{:04x} priority {} flags {:#x})/{}", t.sample, t.adsr1, t.adsr2, t.priority, t.flags, c.delay),
                    None => format!("tone(?)/{}", c.delay),
                },
                t => format!("{t}/{}", c.delay),
            })
            .collect();
        println!(
            "sound: tick {ticks}: weapon bank {} sound {sound} started {} times in {} ticks, {gaps:?} apart; flags {:#x}, polyphony {}: {}",
            bi + 1,
            gaps.len() + 1,
            ticks - at[0],
            b.sounds[sound].flags,
            b.sounds[sound].polyphony,
            cmds.join(" ")
        );
    }

    /// Once a second: a looping voice that has sounded for 20 s or more is told once, with its sound (playing, or how it ended).
    /// A voice a gone sound left sounding is in no sound to stop it.
    fn voice_census(&mut self) {
        let ticks = self.ticks;
        let live: std::collections::HashSet<u32> = self.voices.iter().flatten().map(|v| v.owner).collect();
        self.gone.retain(|h, _| live.contains(h));
        for n in 0..VOICES {
            let Some(v) = self.voices[n].as_ref() else { continue };
            if !v.active || !v.loops() || stream::is_stream(v.owner) || ticks - v.born < 4800 || self.census_told >= CENSUS_TOLD || !self.long_told.insert((n, v.born)) {
                continue;
            }
            self.census_told += 1;
            let whose = match (self.insts.iter().find(|i| i.handle == v.owner), self.gone.get(&v.owner)) {
                (Some(i), _) => format!("playing: bank {} (SBlk id {:#x}) sound {}{}", i.bank + 1, self.banks[i.bank].bank.id, i.sound, if i.let_go { ", let go" } else { "" }),
                (None, Some((b, s, at, why))) => format!("gone: bank {} (SBlk id {:#x}) sound {s}, ended at tick {at}: {why}", b + 1, self.banks[*b].bank.id),
                (None, None) => "gone, how unknown".to_string(),
            };
            println!(
                "sound: tick {ticks}: voice {n} has sounded its looping sample {:#x} (adsr {:04x}/{:04x}) for {} s, {} at level {:#x}, loudness {:#x}; its sound {:#x} {whose}",
                v.tone.sample,
                v.tone.adsr1,
                v.tone.adsr2,
                (ticks - v.born) / 240,
                if v.env.releasing() { "fading" } else { "playing" },
                v.env.envx(),
                v.loudness(),
                v.owner
            );
        }
        self.long_told.retain(|&(n, born)| self.voices[n].as_ref().is_some_and(|v| v.born == born));
    }

    /// A sound about to leave the playing ones: kept a moment (`Ended`) if it is a weapon bank's the game was keeping alive
    /// (changed within the last field, playing for over half a second), with why it ended (`Inst::end`, else `default`); and
    /// for the census of long voices (`gone`) if voices of its still sound.
    fn note_end(&mut self, k: usize, default: &str) {
        if !watch_on() {
            return;
        }
        let weapon = self.banks[self.insts[k].bank].bank.id == WEAPON_BANK;
        let i = &mut self.insts[k];
        let at = self.ticks;
        let why = i.end.take().unwrap_or_else(|| default.to_string());
        if !i.voices.is_empty() {
            self.gone.insert(i.handle, (i.bank, i.sound, at, why.clone()));
        }
        if weapon && i.held != 0 && at - i.held <= 8 && at - i.born > 120 {
            let e = Ended { bank: i.bank, sound: i.sound, handle: i.handle, born: i.born, held: i.held, at, why };
            self.ended.retain(|x| at - x.at <= 60);
            self.ended.push(e);
        }
    }

    /// The game changed a playing sound (0x21, 0x1B): it is keeping it alive.
    pub(crate) fn held(&mut self, handle: u32) {
        let ticks = self.ticks;
        if let Some(i) = self.insts.iter_mut().find(|i| i.handle == handle) {
            i.held = ticks;
        }
    }

    fn update(&mut self, k: usize) -> bool {
        if self.insts[k].paused {
            return false;
        }
        if self.insts[k].cmd == -1 {
            let i = &self.insts[k];
            return i.children.is_empty() && i.voices.is_empty();
        }
        self.insts[k].timer -= 1;
        self.insts[k].loop_seen = false;
        while self.insts[k].timer <= 0 {
            match self.step(k) {
                Ok(true) => return true,
                Ok(false) => {}
                Err(e) => {
                    let h = self.insts[k].handle;
                    self.fault.get_or_insert(format!("sound {h:#x}: {e}"));
                    self.insts[k].cmd = -1;
                    return false;
                }
            }
            if self.insts[k].cmd == -1 {
                return false;
            }
        }
        false
    }

    /// The output's rate (AUDIO > MIXING, PLAN 10: the device's own; 48 kHz the original), before the first frame is pulled:
    /// every tick's 1/240 s, every voice's step and envelope and the reverb follow it (`voice::Out`, `Reverb::set_rate`).
    pub fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
        self.reverb.set_rate(rate);
    }

    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// AUDIO > REVERB: the clean network's return (CLEAN) or the chip's (ORIGINAL), from the next frame on.
    pub fn set_clean_reverb(&mut self, on: bool) {
        self.reverb.clean = on;
    }

    /// The voices' samples read with the windowed sinc (AUDIO > MIXING) or the 4-point Hermite (the original), from the next
    /// frame on.
    pub fn set_hq(&mut self, hq: bool) {
        self.hq = hq;
    }

    /// Fills `out` with the next frames of the mix. The sound clock moves on by as many samples.
    pub fn pull(&mut self, out: &mut [[f32; 2]]) {
        // The master volume register is half of its meaning as well: 0x3FFF is full.
        let master = ((self.master.clamp(0, 0x400) * 0x3fff) >> 10) as f32 / 16384.0;
        let o_ = voice::Out { k: RATE / self.rate, hq: self.hq };
        for o in out.iter_mut() {
            if self.rate == RATE {
                if self.phase == 0 {
                    self.tick();
                    self.phase = SAMPLES_PER_TICK;
                }
                self.phase -= 1;
            } else {
                if self.tick_left <= 0.0 {
                    self.tick();
                    self.tick_left += self.rate / 240.0;
                }
                self.tick_left -= 1.0;
            }
            // What each core's voices put in its dry mix and send to its reverb (a tone with flag bit 0, or a stream part with
            // flag 2); the cores' mixing is the reverb's (`Reverb::mix`).
            let mut dry = [[0f32; 2]; 2];
            let mut send = [[0f32; 2]; 2];
            for (n, v) in self.voices.iter_mut().enumerate() {
                if let Some(v) = v {
                    let s = v.mix(&o_);
                    let core = usize::from(n >= 24);
                    if v.dry {
                        dry[core][0] += s[0];
                        dry[core][1] += s[1];
                    }
                    if v.reverb {
                        send[core][0] += s[0];
                        send[core][1] += s[1];
                    }
                }
            }
            let sum = self.reverb.mix(dry, send);
            *o = [(sum[0] * master).clamp(-1.0, 1.0), (sum[1] * master).clamp(-1.0, 1.0)];
        }
    }

    /// Frames as 16-bit samples.
    pub fn pull_i16(&mut self, out: &mut [[i16; 2]]) {
        let mut f = vec![[0f32; 2]; out.len()];
        self.pull(&mut f);
        for (o, s) in out.iter_mut().zip(f) {
            *o = [(s[0] * 32767.0) as i16, (s[1] * 32767.0) as i16];
        }
    }
}

/// The priority a tone starts with (989snd FUN_00006d30): its own, lowered toward a fifth or a half of it as the sound's master
/// volume (10 bits) falls, for a tone whose flags (bits 1-2) ask for it.
fn priority(volmult: i32, tone: &Tone) -> i32 {
    let p = tone.priority as i32;
    let mode = (tone.flags >> 1) & 3;
    if mode == 0 {
        return p;
    }
    let part = match mode {
        1 => p / 5,
        2 => p / 2,
        _ => p,
    };
    if part == 0 {
        return p;
    }
    if volmult > 0x3b6 {
        return p;
    }
    p - (part - part * volmult / 0x3b6)
}

/// A tone's angle drawn toward the front as the sound's master volume falls (989snd FUN_00006c9c).
fn pan_fade(angle: i32, volmult: i32) -> i32 {
    if volmult >= 0x356 {
        angle
    } else if volmult < 0xab {
        0
    } else if angle < 0xb5 {
        angle * (volmult - 0xaa) / 0x2ac
    } else {
        0x168 - (0x168 - angle) * (volmult - 0xaa) / 0x2ac
    }
}

/// The volume on each side for a level (0..0x7FFE) at an angle (989snd FUN_0001b140): from the front (0) round to the right (90),
/// behind (180) and the left (270), the two sides' shares following a quarter circle. Behind is played as in front (the game's
/// surround mode inverts a side there; the output has two channels). The result is what the voice's volume registers hold:
/// half of the share (as 989snd FUN_0001956c writes them; the chip doubles it).
fn pan(level: i32, angle: i32) -> [i32; 2] {
    if level == 0 {
        return [0, 0];
    }
    // The table of the module: (cos, sin) of half the index in degrees, scaled to 0x3FFF, index 0..179.
    let share = |i: i32| -> (f64, f64) {
        let t = (i as f64 / 2.0).to_radians();
        (t.cos(), t.sin())
    };
    let i = if angle >= 270 { angle - 270 } else { angle + 90 };
    let (l, r) = if i < 180 {
        share(i)
    } else {
        let (a, b) = share(i - 180);
        (b, a)
    };
    let side = |s: f64| ((level as f64 * s) as i32) >> 1;
    [side(l), side(r)]
}
