//! The reverb: the effects the module's calls 0x10, 0x50, 0x51 and 0x64 set up on the SPU2's two cores. The module picks a mode
//! for a core (libsd's ten: off, room, studio A, B and C, hall, space, echo, delay, pipe), a depth for each side, and for the
//! echo and the delay a delay and a feedback; libsd's sceSdSetEffectAttr programs the core's effect unit with that mode's
//! registers, and the chip runs its effect network over a work area in its memory. The engine does both: the registers from
//! libsd's own table on the disc, and the network as the SPU's documentation gives it.
//!
//! The modes' settings (`Presets`, from libsd's image, WAD2 pair 1 entry 6: `uyafmt::sbank::sound_modules`): the work area's size
//! a mode (8-byte units, libsd 0x4A98) and its 32 effect registers (libsd 0x4AC4, 0x44 bytes a mode: dAPF1, dAPF2, vIIR, vCOMB1
//! to 4, vWALL, vAPF1, vAPF2, mLSAME, mRSAME, mLCOMB1, mRCOMB1, mLCOMB2, mRCOMB2, dLSAME, dRSAME, mLDIFF, mRDIFF, mLCOMB3,
//! mRCOMB3, mLCOMB4, mRCOMB4, dLDIFF, dRDIFF, mLAPF1, mRAPF1, mLAPF2, mRAPF2, vLIN, vRIN; the addresses in 8-byte units of the
//! work area). The sizes are checked against 989snd's own table of work areas (0x1DFC0, in bytes). For the echo and the delay
//! libsd moves some of them by the delay and sets the feedback (libsd 0xE34..0xF5C): with d = delay + 1, mLSAME = 64 d - dAPF1,
//! mRSAME = 32 d - dAPF2, mLCOMB1 = mRCOMB1 + 32 d, dLSAME = dRSAME + 32 d, mLAPF1 = mLAPF2 + 32 d, mRAPF1 = mRAPF2 + 32 d, and
//! vWALL = feedback * 0x102.
//!
//! The network (the SPU reverb formula of the PSX/SPU documentation, psx-spx "SPU Reverb Formula"; the SPU2 runs it at half its
//! 48 kHz rate): each sample computes one side, the left on one and the right on the next (the documentation's notes: not both
//! at once as its formula shows them), from that side's input through the 39-tap half-band FIR the documentation lists; the pair
//! reads and writes the work area around a position that moves one 16-bit word after the right side (24 kHz; wrapping in the
//! mode's area; `[m]` is the word m * 4 words on, `[m - 2]` the word before it):
//!
//! ```text
//! Lin = vLIN * L;  Rin = vRIN * R
//! [mLSAME] = (Lin + [dLSAME] vWALL - [mLSAME-2]) vIIR + [mLSAME-2]     (and mRSAME with Rin, dRSAME)
//! [mLDIFF] = (Lin + [dRDIFF] vWALL - [mLDIFF-2]) vIIR + [mLDIFF-2]     (and mRDIFF with Rin, dLDIFF)
//! Lout = vCOMB1 [mLCOMB1] + vCOMB2 [mLCOMB2] + vCOMB3 [mLCOMB3] + vCOMB4 [mLCOMB4]
//! Lout -= vAPF1 [mLAPF1-dAPF1]; [mLAPF1] = Lout; Lout = Lout vAPF1 + [mLAPF1-dAPF1]
//! Lout -= vAPF2 [mLAPF2-dAPF2]; [mLAPF2] = Lout; Lout = Lout vAPF2 + [mLAPF2-dAPF2]   (and the right side alike)
//! ```
//!
//! (products of two 16-bit values over 0x8000, what is written saturated to 16 bits); each side's outputs, a zero between each
//! two, go through the same FIR doubled back to 48 kHz, and the result times the depth (the core's EVOL) is the reverb's return.
//! The work area is the mode's own size: the module sets EEA to its area's last byte (area + size - 1) and libsd starts it
//! `size` bytes below the end of EEA's 128 KiB page, so the network's ring is the mode's size wherever the allocator put it.
//!
//! What the module does with its calls (989snd FUN_0000D4B0, FUN_0000D5B4, FUN_0000D9BC, FUN_0000D2D0, FUN_0000D278,
//! FUN_00003724, FUN_00003794, FUN_00003998, FUN_0000DA68):
//! - 0x50 `mask, mode, depth, delay, feedback`: the mode (with its delay and feedback) is set on each core of the mask (bit 0 the
//!   first, bit 1 the second) and then the depth of both sides. A mode that is already set only takes the new delay and feedback.
//!   A new mode switches the core's effect off (EFFECT_ENABLE 0: the network runs and writes nothing) with its EVOL at 0 and
//!   clears its work area; 10 ticks later the depth is applied and the effect switched on. A mode whose work area is larger than
//!   the one reserved for the core (0x51) is refused and the old one stays. The game's boot sets mode 0 on the second core.
//! - 0x51 `mask, mode`: reserves the work area of that mode for the core (0 gives it up), so that later modes up to its size
//!   need no new area.
//! - 0x10 `mask, depth, time, side`: moves the depth of side 1 (left), 2 (right) or 3 (both) to `depth` in `time` ticks: at once
//!   for 0, else by `depth / time` a tick (by 1 a tick when the distance is shorter than the time).
//! - 0x64 `on`: sets or clears bits 0 and 1 of the second core's MMIX: the first core's output goes into the second core's
//!   effect too. The module's start sets MMIX 0xF00 on the first core (its voices into its dry mix and its effect) and 0xF0C on
//!   the second (the same, and the first core's output into its dry mix): `Reverb::mix`.
//!
//! AUDIO > REVERB (PLAN 10): CLEAN runs the same network from the same settings (the mode's registers, the echo's and the
//! delay's delay and feedback, the depth and its fades, the cores' routing) in floating point at the mix's own rate (`Clean`):
//! both sides every sample instead of one a sample at 24 kHz, the delays the same times at the mix's rate, the reflections'
//! low-pass vIIR the same cut-off there, nothing truncated to 16 bits or saturated, and no half-band filter around it (the chip
//! runs at 24 kHz between two of them, so its tail stops near 11 kHz and carries its 16-bit grain). ORIGINAL is the chip's.
//! Both run all the time, so the setting switches at once with the tail intact.

/// Ticks a new mode's work area takes to clear (989snd FUN_0000D5B4 sets 10).
const SETTLE: u32 = 10;

/// Why a core in mode 0 cannot be given a depth: its effect is off then (EFFECT_ENABLE is only switched on after a mode's work
/// area is cleared) and mode 0's registers (every volume zero, the all-passes at the position itself) return the word at the
/// position of its ring (mode 0's work area) at the top of EEA's page: SPU memory the module never cleared (the end of the last mode's work
/// area, or what was there before any), which the engine does not have.
const OFF_DEPTH: &str = "the chip would return the stale words of its work area, which the engine does not have";

/// The half-band FIR of the reverb's input and output (psx-spx "SPU Reverb Formula": 39 taps, symmetric, the centre 0x4000).
const FIR: [i32; 39] = [
    -0x0001, 0, 0x0002, 0, -0x000a, 0, 0x0023, 0, -0x0067, 0, 0x010a, 0, -0x0268, 0, 0x0534, 0, -0x0b90, 0, 0x2806, 0x4000, 0x2806, 0, -0x0b90, 0,
    0x0534, 0, -0x0268, 0, 0x010a, 0, -0x0067, 0, 0x0023, 0, -0x000a, 0, 0x0002, 0, -0x0001,
];

/// The modes' work area sizes (bytes, 989snd's table) and effect registers (libsd's table).
#[derive(Clone)]
pub struct Presets {
    bytes: [usize; 10],
    regs: [[u16; 32]; 10],
}

impl Presets {
    /// From the module images: 989snd's (`snd`, its table of work areas at 0x1DFC0) and libsd's (`sd`). Mode 0 must be the off
    /// mode `Reverb` reads it as: every volume zero and the all-passes at the position itself (`Reverb::set_mode`).
    pub fn from_modules(snd: &[u8], sd: &[u8]) -> Result<Presets, String> {
        let w32 = |m: &[u8], a: usize| m.get(a..a + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes")));
        let w16 = |m: &[u8], a: usize| m.get(a..a + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
        let mut p = Presets { bytes: [0; 10], regs: [[0; 32]; 10] };
        for mode in 0..10 {
            let units = w32(sd, 0x4a98 + 4 * mode).ok_or("libsd: the effect sizes are past the module image")?;
            let bytes = w32(snd, 0x1dfc0 + 4 * mode).ok_or("989snd: the work area table is past the module image")?;
            if mode > 0 && units * 8 != bytes {
                return Err(format!("libsd's effect mode {mode} is {:#x} bytes, 989snd's work area for it {bytes:#x}: not the tables this reads", units * 8));
            }
            p.bytes[mode] = bytes as usize;
            for r in 0..32 {
                p.regs[mode][r] = w16(sd, 0x4ac4 + 0x44 * mode + 2 * r).ok_or("libsd: the effect registers are past the module image")?;
            }
        }
        let off = &p.regs[0];
        if off[..10].iter().chain(&off[26..]).any(|&v| v != 0) {
            return Err(format!("libsd's effect mode 0 has registers {off:x?}: not the off mode this reads"));
        }
        Ok(p)
    }

    /// The registers sceSdSetEffectAttr programs for `mode`, with the echo's and the delay's delay and feedback.
    pub fn regs(&self, mode: u32, delay: u32, feedback: u32) -> [u16; 32] {
        let mut r = self.regs[mode as usize];
        if mode == 7 || mode == 8 {
            let d = delay.wrapping_add(1);
            r[10] = ((d << 6) as u16).wrapping_sub(r[0]);
            r[11] = ((d << 5) as u16).wrapping_sub(r[1]);
            r[12] = r[13].wrapping_add((d << 5) as u16);
            r[16] = r[17].wrapping_add((d << 5) as u16);
            r[26] = r[28].wrapping_add((d << 5) as u16);
            r[27] = r[29].wrapping_add((d << 5) as u16);
            r[7] = feedback.wrapping_mul(0x102) as u16;
        }
        r
    }

    /// The work area's size of `mode` in bytes (989snd's: what 0x51 reserves and 0x50 compares).
    pub fn area(&self, mode: u32) -> usize {
        self.bytes[mode as usize]
    }
}

/// A product of two 16-bit values over 0x8000.
fn mul(a: i32, b: i32) -> i32 {
    (a * b) >> 15
}

fn sat(x: i32) -> i32 {
    x.clamp(-0x8000, 0x7fff)
}

/// The effect network of one core: its registers, its work area and position, the FIR's histories.
struct Effect {
    r: [i32; 32],
    ram: Vec<i16>,
    pos: usize,
    /// The input (48 kHz) and each side's outputs with a zero between each two (48 kHz), the newest last; whether the next sample
    /// computes the right side.
    input: [[i32; 39]; 2],
    output: [[i32; 39]; 2],
    right: bool,
    /// EFFECT_ENABLE: the network writes its work area.
    write: bool,
}

impl Effect {
    fn new(regs: [u16; 32], words: usize) -> Effect {
        Effect { r: regs.map(|v| v as i16 as i32), ram: vec![0; words], pos: 0, input: [[0; 39]; 2], output: [[0; 39]; 2], right: false, write: false }
    }

    /// New registers on the same work area (the echo's and the delay's delay and feedback).
    fn set_regs(&mut self, regs: [u16; 32]) {
        self.r = regs.map(|v| v as i16 as i32);
    }

    /// The word `units` 8-byte units on from the position, `back` words before it.
    fn at(&self, units: i32, back: usize) -> usize {
        let n = self.ram.len() as i64;
        ((self.pos as i64 + (units as u16 as i64) * 4 - back as i64).rem_euclid(n)) as usize
    }
    fn rd(&self, units: i32, back: usize) -> i32 {
        self.ram[self.at(units, back)] as i32
    }
    fn wr(&mut self, units: i32, v: i32) {
        if self.write {
            let a = self.at(units, 0);
            self.ram[a] = sat(v) as i16;
        }
    }

    /// One side's half of a 24 kHz cycle of the network (0 left, 1 right): its input in, its output out.
    fn side(&mut self, s: usize, x: i32) -> i32 {
        let g = self.r;
        let (d_apf1, d_apf2, v_iir, v_wall, v_apf1, v_apf2) = (g[0], g[1], g[2], g[7], g[8], g[9]);
        let v_comb = [g[3], g[4], g[5], g[6]];
        // This side's same-side write and read, its different-side write and the other side's read, its combs, its all-passes,
        // its input volume (the register order in `Presets`).
        let (m_same, d_same, m_diff, d_diff) = [(g[10], g[16], g[18], g[25]), (g[11], g[17], g[19], g[24])][s];
        let combs = [[g[12], g[14], g[20], g[22]], [g[13], g[15], g[21], g[23]]][s];
        let (m_apf1, m_apf2, v_in) = [(g[26], g[28], g[30]), (g[27], g[29], g[31])][s];
        let x = mul(v_in, x);
        let reflect = |e: &Effect, d: i32, m: i32| -> i32 {
            let prev = e.rd(m, 1);
            mul(x + mul(e.rd(d, 0), v_wall) - prev, v_iir) + prev
        };
        let same = reflect(self, d_same, m_same);
        self.wr(m_same, same);
        let diff = reflect(self, d_diff, m_diff);
        self.wr(m_diff, diff);
        let mut o = (0..4).map(|k| mul(v_comb[k], self.rd(combs[k], 0))).sum::<i32>();
        // Only what is written is saturated (the documentation's notes); the output is a 16-bit sample of the FIR's.
        for (m, d, v) in [(m_apf1, d_apf1, v_apf1), (m_apf2, d_apf2, v_apf2)] {
            let back = self.rd(m.wrapping_sub(d), 0);
            o -= mul(v, back);
            self.wr(m, o);
            o = mul(o, v) + back;
        }
        sat(o)
    }

    /// One 48 kHz sample: the input (16-bit scale) in, the network's output (16-bit scale, before the depth) out; `write` is
    /// EFFECT_ENABLE. A sample computes one side (left first), from its input's FIR; the position moves on after the right.
    fn run(&mut self, x: [i32; 2], write: bool) -> [i32; 2] {
        self.write = write;
        for s in 0..2 {
            self.input[s].copy_within(1.., 0);
            self.input[s][38] = x[s];
        }
        let s = usize::from(self.right);
        let down = sat((0..39).map(|k| FIR[k] * self.input[s][k]).sum::<i32>() >> 15);
        let o = self.side(s, down);
        let mut y = [0i32; 2];
        for c in 0..2 {
            self.output[c].copy_within(1.., 0);
            self.output[c][38] = if c == s { o } else { 0 };
            y[c] = sat((0..39).map(|k| FIR[k] * 2 * self.output[c][k]).sum::<i32>() >> 15);
        }
        if self.right {
            self.pos = (self.pos + 1) % self.ram.len();
        }
        self.right = !self.right;
        y
    }
}

/// The network of `Effect` in floating point at the mix's rate (AUDIO > REVERB, CLEAN): the same registers, the work area the same
/// time long (`scale` samples a 16-bit word of the chip's, a word being a 24 kHz cycle), both sides at each sample.
struct Clean {
    /// The registers as the chip reads them: volumes over 0x8000 (`g`), addresses in 8-byte units (`r`).
    g: [f32; 32],
    r: [i32; 32],
    ram: Vec<f32>,
    pos: usize,
    scale: f64,
    /// vIIR at the mix's rate: the same one-pole cut-off as the chip's at 24 kHz.
    iir: f32,
    write: bool,
}

impl Clean {
    fn new(regs: [u16; 32], words: usize, rate: f64) -> Clean {
        let scale = rate / 24000.0;
        let n = ((words as f64 * scale).round() as usize).max(1);
        let mut c = Clean { g: [0.0; 32], r: [0; 32], ram: vec![0.0; n], pos: 0, scale, iir: 0.0, write: false };
        c.set_regs(regs);
        c
    }

    fn set_regs(&mut self, regs: [u16; 32]) {
        self.r = regs.map(|v| v as i16 as i32);
        self.g = self.r.map(|v| v as f32 / 32768.0);
        // y += a (x - y) once a 24 kHz cycle is y += a' (x - y) once a sample with 1 - a' = (1 - a)^(24 kHz / rate).
        let a = self.g[2] as f64;
        self.iir = (1.0 - (1.0 - a).powf(1.0 / self.scale)) as f32;
    }

    /// The sample `units` 8-byte units (four words of the chip's: as many 24 kHz cycles) on from the position, `back` samples
    /// before it.
    fn at(&self, units: i32, back: usize) -> usize {
        let n = self.ram.len() as i64;
        let off = ((units as u16 as f64) * 4.0 * self.scale).round() as i64;
        ((self.pos as i64 + off - back as i64).rem_euclid(n)) as usize
    }
    fn rd(&self, units: i32, back: usize) -> f32 {
        self.ram[self.at(units, back)]
    }
    fn wr(&mut self, units: i32, v: f32) {
        if self.write {
            let a = self.at(units, 0);
            self.ram[a] = v;
        }
    }

    /// One side (0 left, 1 right): `Effect::side` in floating point.
    fn side(&mut self, s: usize, x: f32) -> f32 {
        let (g, r) = (self.g, self.r);
        let (d_apf1, d_apf2, v_wall, v_apf1, v_apf2) = (r[0], r[1], g[7], g[8], g[9]);
        let v_comb = [g[3], g[4], g[5], g[6]];
        let (m_same, d_same, m_diff, d_diff) = [(r[10], r[16], r[18], r[25]), (r[11], r[17], r[19], r[24])][s];
        let combs = [[r[12], r[14], r[20], r[22]], [r[13], r[15], r[21], r[23]]][s];
        let (m_apf1, m_apf2, v_in) = [(r[26], r[28], g[30]), (r[27], r[29], g[31])][s];
        let x = v_in * x;
        let iir = self.iir;
        let reflect = |e: &Clean, d: i32, m: i32| -> f32 {
            let prev = e.rd(m, 1);
            (x + e.rd(d, 0) * v_wall - prev) * iir + prev
        };
        let same = reflect(self, d_same, m_same);
        self.wr(m_same, same);
        let diff = reflect(self, d_diff, m_diff);
        self.wr(m_diff, diff);
        let mut o: f32 = (0..4).map(|k| v_comb[k] * self.rd(combs[k], 0)).sum();
        for (m, d, v) in [(m_apf1, d_apf1, v_apf1), (m_apf2, d_apf2, v_apf2)] {
            let back = self.rd(m.wrapping_sub(d), 0);
            o -= v * back;
            self.wr(m, o);
            o = o * v + back;
        }
        o
    }

    /// One sample at the mix's rate: the input (1.0 full scale) in, both sides' output out; `write` is EFFECT_ENABLE.
    fn run(&mut self, x: [f32; 2], write: bool) -> [f32; 2] {
        self.write = write;
        let y = [self.side(0, x[0]), self.side(1, x[1])];
        self.pos = (self.pos + 1) % self.ram.len();
        y
    }
}

/// One core's effect.
struct Unit {
    /// The mode set (0: off, or none yet), the mode whose work area is reserved (0: none), the depth the game set (left, right:
    /// the core's EVOL, signed 16-bit) and the ticks the work area has left to clear (989snd 0x1DFB4 + 4c).
    mode: u32,
    reserved: u32,
    depth: [i32; 2],
    settle: u32,
    /// The depth fades running (989snd 0x1E4A0 and 0x1E4C4 + 0x48c): the step a tick and the depth it stops at, per side.
    fade: [Option<(i32, i32)>; 2],
    effect: Option<Effect>,
    /// At an output rate other than 48 kHz (AUDIO > MIXING): the network runs at its own 48 kHz, its input resampled to it and
    /// its return back (`sinc::Stream`).
    conv: Option<(super::sinc::Stream, super::sinc::Stream)>,
    /// The same network in floating point at the mix's rate (AUDIO > REVERB, CLEAN), run beside it.
    clean: Option<Clean>,
}

impl Unit {
    fn new() -> Unit {
        Unit { mode: 0, reserved: 0, depth: [0; 2], settle: 0, fade: [None; 2], effect: None, conv: None, clean: None }
    }

    /// The clean network's sample (1.0 full scale, times the depth): nothing while the work area clears.
    fn run_clean(&mut self, x: [f32; 2]) -> [f32; 2] {
        let settled = self.settle == 0;
        let Some(c) = self.clean.as_mut() else { return [0.0; 2] };
        let y = c.run(x, settled);
        if !settled {
            return [0.0; 2];
        }
        [0, 1].map(|s| y[s] * self.depth[s] as f32 / 32768.0)
    }

    /// One output sample at the output's rate: `run` itself at 48 kHz, else the 48 kHz samples the input reaches, resampled in
    /// and out.
    fn run_at(&mut self, x: [i32; 2]) -> [i32; 2] {
        let Some((mut into, mut back)) = self.conv.take() else { return self.run(x) };
        into.push(x.map(|v| v as f32));
        while let Some(s) = into.pull() {
            let y = self.run(s.map(|v| v.round() as i32));
            back.push(y.map(|v| v as f32));
        }
        let y = back.pull().unwrap_or([0.0; 2]).map(|v| v.round() as i32);
        self.conv = Some((into, back));
        y
    }

    /// One sample: the core's effect input in (16-bit scale), its return out (16-bit scale, times the depth). While the work area
    /// clears the effect is switched off (it runs, writes nothing) and its EVOL is 0.
    fn run(&mut self, x: [i32; 2]) -> [i32; 2] {
        let Some(e) = self.effect.as_mut() else { return [0; 2] };
        let y = e.run(x, self.settle == 0);
        if self.settle > 0 {
            return [0; 2];
        }
        [0, 1].map(|s| (y[s] * self.depth[s]) >> 15)
    }
}

/// The two cores' effects and the routing between the cores.
pub struct Reverb {
    presets: Presets,
    unit: [Unit; 2],
    /// 0x64: the first core's output goes into the second core's effect too (core 1's MMIX bits 0 and 1).
    ext: bool,
    /// The mix's rate, and AUDIO > REVERB: the clean network's return instead of the chip's.
    rate: f64,
    pub clean: bool,
    /// What the engine was asked for and the module refuses too (a mode whose area is over the reserved one).
    pub refused: u64,
}

impl Reverb {
    /// With the module images (`Presets::from_modules`).
    pub fn new(snd: &[u8], sd: &[u8]) -> Result<Reverb, String> {
        Ok(Reverb { presets: Presets::from_modules(snd, sd)?, unit: [Unit::new(), Unit::new()], ext: false, rate: 48000.0, clean: false, refused: 0 })
    }

    /// The output's rate (`Engine::set_rate`): the networks are the chip's at 48 kHz, so at another rate each core's runs on its
    /// input resampled to 48 kHz and its return resampled back.
    pub fn set_rate(&mut self, rate: f64) {
        self.rate = rate;
        for u in &mut self.unit {
            u.conv = (rate != 48000.0).then(|| (super::sinc::Stream::new(rate, 48000.0), super::sinc::Stream::new(48000.0, rate)));
        }
    }

    /// 0x50 (989snd FUN_0000D4B0): the mode, its delay and feedback, and the depth of both sides, for each core of `mask`. An
    /// error for a depth on a core left in mode 0 (`OFF_DEPTH`).
    pub fn set(&mut self, mask: u32, mode: u32, depth: i32, delay: u32, feedback: u32) -> Result<(), String> {
        for c in 0..2 {
            if mask >> c & 1 == 0 {
                continue;
            }
            self.set_mode(c, mode, delay, feedback);
            let d = depth as i16 as i32;
            if self.unit[c].mode == 0 && d != 0 {
                return Err(format!("reverb: core {c} in mode 0 given the depth {d:#x}: {OFF_DEPTH}"));
            }
            self.unit[c].depth = [d; 2];
        }
        Ok(())
    }

    /// FUN_0000D5B4.
    fn set_mode(&mut self, c: usize, mode: u32, delay: u32, feedback: u32) {
        let mode = mode.min(9);
        let regs = self.presets.regs(mode, delay, feedback);
        let (area_reserved, area_mode) = (self.presets.area(self.unit[c].reserved), self.presets.area(mode));
        let u = &mut self.unit[c];
        if u.mode == mode {
            // sceSdSetEffectAttr again: the echo's and the delay's delay and feedback, on the same work area.
            if let Some(e) = u.effect.as_mut() {
                e.set_regs(regs);
            }
            if let Some(c) = u.clean.as_mut() {
                c.set_regs(regs);
            }
            return;
        }
        if u.reserved > 0 && area_reserved < area_mode {
            // 989snd error 0x57: the work area reserved is smaller than the mode's.
            self.refused += 1;
            return;
        }
        u.fade = [None; 2];
        u.mode = mode;
        // A new mode's work area is cleared (sceSdClearEffectWorkArea) and its effect switched on 10 ticks later; mode 0's
        // registers are all zero (`Presets::from_modules`), so an effect of mode 0 returns nothing.
        u.effect = (mode != 0).then(|| Effect::new(regs, self.presets.area(mode) / 2));
        u.clean = (mode != 0).then(|| Clean::new(regs, self.presets.area(mode) / 2, self.rate));
        u.settle = if mode != 0 { SETTLE } else { 0 };
    }

    /// 0x51 (989snd FUN_0000D2D0): reserves the work area of `mode` for each core of `mask` (0 gives it up; a mode already set
    /// is switched off first).
    pub fn reserve(&mut self, mask: u32, mode: u32) {
        for c in 0..2 {
            if mask >> c & 1 == 0 {
                continue;
            }
            if self.unit[c].mode > 0 {
                self.set_mode(c, 0, 0, 0);
            }
            self.unit[c].reserved = mode.min(9);
        }
    }

    /// 0x64 (989snd FUN_0000D278).
    pub fn enable(&mut self, on: bool) {
        self.ext = on;
    }

    /// 0x10 (989snd FUN_00003794): the depth of side `side` (1 left, 2 right, 3 both) of each core of `mask` moves to `depth` in
    /// `time` ticks. An error for a depth other than 0 on a core in mode 0, on the way or at the end (`OFF_DEPTH`).
    pub fn fade(&mut self, mask: u32, depth: i32, time: i32, side: u32) -> Result<(), String> {
        for c in 0..2 {
            if mask >> c & 1 == 0 {
                continue;
            }
            for s in 0..2 {
                if side >> s & 1 == 0 {
                    continue;
                }
                let u = &mut self.unit[c];
                u.fade[s] = None;
                let to = depth as i16 as i32;
                if u.mode == 0 && (to != 0 || (time != 0 && u.depth[s] != 0)) {
                    return Err(format!("reverb: core {c} in mode 0, its depth moved from {:#x} to {to:#x} in {time} ticks: {OFF_DEPTH}", u.depth[s]));
                }
                if time == 0 {
                    u.depth[s] = to;
                    continue;
                }
                let d = to - u.depth[s];
                if d == 0 {
                    continue;
                }
                let step = if d.abs() < time { d.signum() } else { d / time };
                u.fade[s] = Some((step, to));
            }
        }
        Ok(())
    }

    /// One tick (989snd FUN_0000DA68, FUN_00003998): the work area's clearing runs down and the depth fades step.
    pub fn tick(&mut self) {
        for u in self.unit.iter_mut() {
            if u.settle > 0 {
                u.settle -= 1;
            }
            for s in 0..2 {
                if let Some((step, to)) = u.fade[s] {
                    let d = u.depth[s] + step;
                    let d = if (step > 0 && d > to) || (step < 0 && d < to) { to } else { d };
                    u.depth[s] = d;
                    if d == to {
                        u.fade[s] = None;
                    }
                }
            }
        }
    }

    /// One frame of the two cores (1.0 the 16-bit full scale): what each core's voices put in its dry mix and send to its effect
    /// in, the second core's output out (before the master volume). The first core's output (its dry mix and its effect's return,
    /// at its MVOL 0x3FFF) is in the second core's dry mix, and with 0x64 in its effect's input too.
    pub fn mix(&mut self, dry: [[f32; 2]; 2], send: [[f32; 2]; 2]) -> [f32; 2] {
        let i16s = |v: [f32; 2]| v.map(|x| sat((x * 32768.0) as i32));
        let f32s = |v: [i32; 2]| v.map(|x| x as f32 / 32768.0);
        let clean = self.clean;
        // Both networks run (the setting switches at once, the tails intact); the setting picks the return.
        let run = |u: &mut Unit, x: [f32; 2]| {
            let original = f32s(u.run_at(i16s(x)));
            let c = u.run_clean(x);
            if clean { c } else { original }
        };
        let r0 = run(&mut self.unit[0], send[0]);
        let out0 = [0, 1].map(|s| (dry[0][s] + r0[s]) * (0x3fff as f32 / 16384.0));
        let ext = if self.ext { out0 } else { [0.0; 2] };
        let r1 = run(&mut self.unit[1], [send[1][0] + ext[0], send[1][1] + ext[1]]);
        [0, 1].map(|s| dry[1][s] + out0[s] + r1[s])
    }
}
