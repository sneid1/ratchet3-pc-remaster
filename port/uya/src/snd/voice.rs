//! A voice of the sound engine: a tone's sample played at a pitch, shaped by an envelope, at a volume on each side.
//!
//! The sample is PS-ADPCM as the banks hold it (16-byte frames: +0 shift in the low nibble and filter in the high one, +1 flags
//! (bit 0 the sample's last frame, bit 1 it repeats, bit 2 a frame the repeat returns to), 28 samples of 4 bits); it is decoded
//! once per sample (`decode`) and played from that. The envelope's four parts and their rates are the tone record's two words
//! (989snd FUN_0001956c copies them to the voice): attack (rising to full), decay (falling to the sustain level), sustain (held,
//! or slowly rising or falling) and release (falling to silence after the voice is let go), run in the chip's own integers
//! (`Env`).

use super::RATE;
use std::cell::RefCell;
use std::sync::Arc;

/// A sample: 16-bit mono PCM at the rate it was recorded at (the tone's centre note says which), and where it goes back to when it
/// reaches its end (None: it stops). A bank's tone is decoded whole; a stream's clip stays the disc's PS-ADPCM and the voice decodes
/// the part it reads (`Adpcm`).
pub struct Pcm {
    src: Source,
    pub loop_to: Option<usize>,
}

enum Source {
    Decoded(Vec<i16>),
    Adpcm(Adpcm),
}

impl Pcm {
    pub fn decoded(s: Vec<i16>, loop_to: Option<usize>) -> Pcm {
        Pcm { src: Source::Decoded(s), loop_to }
    }

    /// A stream's clip: `frames` (a whole number of 16-byte frames) read as `len` samples, the stream engine's length for it: a
    /// longer clip ends there and a shorter one is silent for the rest.
    pub fn adpcm(frames: Vec<u8>, len: usize, loop_to: Option<usize>) -> Pcm {
        Pcm { src: Source::Adpcm(Adpcm::new(frames, len)), loop_to }
    }

    /// The sample's length in samples.
    pub fn len(&self) -> usize {
        match &self.src {
            Source::Decoded(s) => s.len(),
            Source::Adpcm(a) => a.len,
        }
    }

    fn at(&self, cache: &RefCell<Cache>, j: usize) -> i16 {
        match &self.src {
            Source::Decoded(s) => s[j],
            Source::Adpcm(a) => a.at(&mut cache.borrow_mut(), j),
        }
    }
}

/// A clip's frames in segments of `SEG` frames, with the filter history (the two samples before it) at each segment's start,
/// found once when the clip is made: a voice decodes the segment it reads, whatever order it reads them in (the window of the
/// resampler, the loop's wrap), and every sample is the one the clip's decode in a row gave.
struct Adpcm {
    frames: Vec<u8>,
    hist: Vec<(i32, i32)>,
    /// The samples the clip counts (the stream engine's length) and how many of them the frames give: the rest are silent.
    len: usize,
    decoded: usize,
}

const SEG: usize = 64;
const SEG_SAMPLES: usize = SEG * 28;
const CACHE: usize = 8;

impl Adpcm {
    fn new(frames: Vec<u8>, len: usize) -> Adpcm {
        let mut hist = Vec::with_capacity(frames.len() / 16 / SEG + 1);
        let (mut a, mut b) = (0i32, 0i32);
        let mut scratch = Vec::with_capacity(28);
        for (i, f) in frames.chunks_exact(16).enumerate() {
            if i % SEG == 0 {
                hist.push((a, b));
            }
            scratch.clear();
            frame(f, &mut a, &mut b, &mut scratch);
        }
        let decoded = (frames.len() / 16 * 28).min(len);
        Adpcm { frames, hist, len, decoded }
    }

    fn at(&self, c: &mut Cache, j: usize) -> i16 {
        if j >= self.decoded {
            return 0;
        }
        let k = j / SEG_SAMPLES;
        let slot = match c.segs.iter().position(|(s, _)| *s == k) {
            Some(p) => p,
            None => {
                let d = self.segment(k);
                if c.segs.len() < CACHE {
                    c.segs.push((k, d));
                    c.segs.len() - 1
                } else {
                    let p = c.next;
                    c.next = (p + 1) % CACHE;
                    c.segs[p] = (k, d);
                    p
                }
            }
        };
        c.segs[slot].1[j - k * SEG_SAMPLES]
    }

    fn segment(&self, k: usize) -> Vec<i16> {
        let (mut a, mut b) = self.hist[k];
        let mut s = Vec::with_capacity(SEG_SAMPLES);
        for f in self.frames[k * SEG * 16..].chunks_exact(16).take(SEG) {
            frame(f, &mut a, &mut b, &mut s);
        }
        s
    }
}

/// The segments of a clip a voice has decoded and still holds.
#[derive(Default)]
struct Cache {
    segs: Vec<(usize, Vec<i16>)>,
    next: usize,
}

/// One 16-byte frame's 28 samples appended to `s`; `a` and `b` are the two samples before it (the filters' taps, in 1/64).
/// The SPU2 decodes every header: a shift past 12 shifts the nibble further (13 to 15: smaller samples), and a filter past 4
/// takes no taps (its coefficient rows are zero), as PCSX2's SPU2 decodes them (Mixer.cpp XA_decode_block: the header's shift
/// plus 16 on the nibble in the top bits, tbl_XA_Factor's rows 5 to 15 zero). Such a frame stopped the game (the player,
/// 18 November 2026, on level 3: "game crashed"; frame 0x5E20 of a bank: shift 13, filter 6).
fn frame(f: &[u8], a: &mut i32, b: &mut i32, s: &mut Vec<i16>) {
    const F: [(i32, i32); 16] = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0), (0, 0)];
    let (shift, filter) = ((f[0] & 15) as u32, (f[0] >> 4) as usize);
    for k in 0..28 {
        let nib = if k % 2 == 0 { f[2 + k / 2] & 15 } else { f[2 + k / 2] >> 4 } as i32;
        let raw = ((nib << 12) as i16 as i32) >> shift;
        let v = (raw + ((*a * F[filter].0 + *b * F[filter].1) >> 6)).clamp(-32768, 32767);
        *b = *a;
        *a = v;
        s.push(v as i16);
    }
}

/// The sample whose first frame is at byte `start` of `frames`, to its last frame.
pub fn decode(frames: &[u8], start: usize) -> Result<Pcm, String> {
    let mut s = Vec::new();
    let (mut a, mut b) = (0i32, 0i32);
    let mut loop_start = 0usize;
    let mut p = start;
    loop {
        let f = frames.get(p..p + 16).ok_or_else(|| format!("the sample at {start:#x} runs past the bank's {:#x} bytes without an end frame", frames.len()))?;
        let flags = f[1];
        if flags & 4 != 0 {
            loop_start = s.len();
        }
        frame(f, &mut a, &mut b, &mut s);
        p += 16;
        if flags & 1 != 0 {
            return Ok(Pcm::decoded(s, (flags & 2 != 0).then_some(loop_start)));
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Stage {
    Attack,
    Decay,
    Sustain,
    Release,
    Off,
}

/// One part of an envelope as the SPU2 runs it (PCSX2 V_ADSR::UpdateCache): falling or rising, exponential or linear, the rate's
/// shift and step, and the level that ends the part (the sustain part has none).
#[derive(Clone, Copy)]
struct Part {
    falling: bool,
    exp: bool,
    shift: u32,
    step: i32,
    target: i32,
}

/// The four parts of a tone's envelope words.
fn parts(adsr1: u16, adsr2: u16) -> [Part; 4] {
    let (a1, a2) = (adsr1 as i32, adsr2 as i32);
    let attack = Part { falling: false, exp: a1 & 0x8000 != 0, shift: (a1 >> 10 & 31) as u32, step: 7 - (a1 >> 8 & 3), target: 0x7fff };
    let decay = Part { falling: true, exp: true, shift: (a1 >> 4 & 15) as u32, step: -8, target: ((a1 & 15) + 1) << 11 };
    let sustain_falls = a2 & 0x4000 != 0;
    let sustain_step = 7 - (a2 >> 6 & 3);
    let sustain = Part { falling: sustain_falls, exp: a2 & 0x8000 != 0, shift: (a2 >> 8 & 31) as u32, step: if sustain_falls { !sustain_step } else { sustain_step }, target: 0 };
    let release = Part { falling: true, exp: a2 & 0x20 != 0, shift: (a2 & 31) as u32, step: -8, target: 0 };
    [attack, decay, sustain, release]
}

/// The envelope of a tone (`Tone::adsr1`, `Tone::adsr2`) as the SPU2 runs it, in its own integers: the level (ENVX, 0..=0x7FFF)
/// moves by the part's step once the part's counter fills (PCSX2 V_ADSR::Calculate). An exponential fall is the step times the
/// level over 0x8000 rounded down, so it never shrinks below one: a falling voice reaches 0 and stops, and 989snd frees it at
/// its next tick. The port's envelope was continuous before (23 November 2026) and fell exponentially until 1/10000 of full
/// scale: an exponential release at shift 11 held its voice 608 ms from 1/8 of full scale, the chip's 85 ms, and voices the
/// chip had freed were taken from sounds that needed them (in the Annihilation Nation arena 14 % more of them).
#[derive(Clone, Copy)]
pub struct Env {
    parts: [Part; 4],
    stage: Stage,
    counter: u32,
    level: i32,
    /// 48 kHz samples the envelope still owes the output (a mix at another rate: `next_at`).
    owed: f64,
}

impl Env {
    pub fn new(adsr1: u16, adsr2: u16) -> Env {
        Env { parts: parts(adsr1, adsr2), stage: Stage::Attack, counter: 0, level: 0, owed: 0.0 }
    }

    /// The voice is let go (KOFF): the envelope falls at its release rate from where it is.
    pub fn release(&mut self) {
        if self.stage != Stage::Off {
            self.stage = Stage::Release;
            self.counter = 0;
        }
    }

    pub fn done(&self) -> bool {
        self.stage == Stage::Off
    }

    pub fn releasing(&self) -> bool {
        self.stage == Stage::Release
    }

    /// ENVX.
    pub fn envx(&self) -> u16 {
        self.level as u16
    }

    /// One 48 kHz sample of the chip's envelope.
    fn step(&mut self) {
        if self.stage == Stage::Off {
            return;
        }
        let p = self.parts[self.stage as usize];
        let mut counter_inc = 0x8000u32 >> p.shift.saturating_sub(11);
        let mut level_inc = p.step << 11u32.saturating_sub(p.shift);
        if p.exp {
            if !p.falling && self.level > 0x6000 {
                counter_inc >>= 2;
            }
            if p.falling {
                level_inc = ((level_inc * self.level) >> 15) as i16 as i32;
            }
        }
        self.counter += counter_inc.max(1);
        if self.counter >= 0x8000 {
            self.counter = 0;
            self.level = (self.level + level_inc).clamp(0, 0x7fff);
        }
        if self.stage == Stage::Sustain {
            // The sustain part has no end level: one that falls to 0 stops the voice.
            if self.level == 0 {
                self.stage = Stage::Off;
            }
            return;
        }
        if (!p.falling && self.level >= p.target) || (p.falling && self.level <= p.target) {
            self.stage = match self.stage {
                Stage::Attack => Stage::Decay,
                Stage::Decay => Stage::Sustain,
                _ => Stage::Off,
            };
        }
        if self.stage == Stage::Off {
            self.level = 0;
        }
    }

    /// The level (0..1) for the next output sample of a mix at `k` x 48 kHz's sample period (`Out::k`): the envelope runs the
    /// chip's 48 kHz samples that fall due by then.
    pub fn next_at(&mut self, k: f64) -> f32 {
        self.owed += k;
        while self.owed >= 1.0 {
            self.owed -= 1.0;
            self.step();
        }
        self.level as f32 / 32768.0
    }
}

/// How the engine's output is made (AUDIO > MIXING, PLAN 10): `k`, 48 kHz over the output's rate (1.0 the original mix: the
/// voices' steps and envelopes are the chip's per 48 kHz sample, and move `k` times as far an output sample), and `hq`, each
/// voice's sample read with the windowed sinc (`sinc.rs`) instead of the 4-point Hermite.
#[derive(Clone, Copy)]
pub struct Out {
    pub k: f64,
    pub hq: bool,
}

/// The original mix: 48 kHz, the Hermite.
pub const ORIGINAL: Out = Out { k: 1.0, hq: false };

/// A sounding voice.
pub struct Voice {
    pcm: Arc<Pcm>,
    /// The segments of `pcm` decoded for the reads (a stream's clip).
    cache: RefCell<Cache>,
    /// The read position in the sample (fractional) and how far it moves per 48 kHz output sample (`Out::k` scales it).
    pos: f64,
    pub step: f64,
    pub env: Env,
    /// The volume on each side, as a fraction of full scale.
    pub gain: [f32; 2],
    /// Noise instead of the sample (a tone with flag bit 3).
    pub noise: bool,
    noise_val: f32,
    noise_seed: u32,
    /// The sound instance the voice belongs to (its handle), the priority it was given and the tick it started on.
    pub owner: u32,
    pub priority: i32,
    pub born: u64,
    /// A tone with flag bit 5 is not stolen by a sound of lower or equal priority.
    pub locked: bool,
    /// Bit 0 of the tone's flags: sent to the reverb. Bit 4: not heard dry.
    pub reverb: bool,
    pub dry: bool,
    pub active: bool,
    /// The sound is paused: the voice holds where it is.
    pub paused: bool,
    /// The tone and the values its pitch and volume were made from, kept so that a bend or a volume change can redo them.
    pub tone: uyafmt::sbank::Tone,
    pub tone_volume: i32,
    pub tone_pan: i32,
    /// A stream's next part: the voice goes on with it, and at the rate it gives, as its sample ends (the gapless join of a
    /// queued part); `advanced` counts the joins.
    pub chain: Option<(Arc<Pcm>, f64)>,
    pub advanced: u32,
}

impl Voice {
    #[allow(clippy::too_many_arguments)]
    pub fn new(pcm: Arc<Pcm>, tone: uyafmt::sbank::Tone, owner: u32, priority: i32, born: u64, tone_volume: i32, tone_pan: i32) -> Voice {
        Voice {
            pcm,
            cache: RefCell::default(),
            pos: 0.0,
            step: 1.0,
            env: Env::new(tone.adsr1, tone.adsr2),
            gain: [0.0; 2],
            noise: tone.flags & 8 != 0,
            noise_val: 0.0,
            noise_seed: 0x1234_5678 ^ born as u32,
            owner,
            priority,
            born,
            locked: tone.flags & 0x20 != 0,
            reverb: tone.flags & 1 != 0,
            dry: tone.flags & 0x10 == 0,
            active: true,
            paused: false,
            tone,
            tone_volume,
            tone_pan,
            chain: None,
            advanced: 0,
        }
    }

    /// A stream's voice: the clip at `step` (its rate over the output's), keyed as the stream engine keys one (989snd FUN_00016b50:
    /// the ADSR words 0x000F and 0x1FC0, the top priority, never stolen). Its volumes are the stream's to set.
    pub fn stream(pcm: Arc<Pcm>, step: f64, owner: u32, reverb: bool, born: u64) -> Voice {
        let tone = uyafmt::sbank::Tone {
            priority: 0x7f,
            volume: 0x7f,
            centre_note: 0,
            fine_tune: 0,
            pan: 0,
            key_low: 0,
            key_high: 127,
            bend_down: 0,
            bend_up: 0,
            adsr1: 0x000f,
            adsr2: 0x1fc0,
            flags: 0x20 | reverb as u16,
            sample: 0,
        };
        let mut v = Voice::new(pcm, tone, owner, 0x7f, born, 0x7f, 0);
        v.step = step;
        v
    }

    /// How loud the voice is now: its envelope's level times its louder side's volume (0 silent, 0x7FFF a full voice at full
    /// volume).
    pub fn loudness(&self) -> u32 {
        (self.gain[0].max(self.gain[1]).min(1.0) * self.env.envx() as f32) as u32
    }

    /// Whether the voice's sample goes back to a loop point at its end (it sounds until it is let go or stopped).
    pub fn loops(&self) -> bool {
        self.pcm.loop_to.is_some()
    }

    /// How far into its sample the voice is, in the sample's own samples.
    pub fn position(&self) -> f64 {
        self.pos
    }

    /// The next output sample: the sample at the read position (4-point Hermite, or the windowed sinc with `o.hq`), times the
    /// envelope and the volumes.
    pub fn mix(&mut self, o: &Out) -> [f32; 2] {
        if self.paused {
            return [0.0; 2];
        }
        let e = self.env.next_at(o.k);
        let step = if o.k == 1.0 { self.step } else { self.step * o.k };
        if self.env.done() {
            self.active = false;
            return [0.0; 2];
        }
        let v = if self.noise {
            let i = self.pos as u64;
            self.pos += step;
            if self.pos as u64 != i {
                self.noise_seed = self.noise_seed.wrapping_mul(1664525).wrapping_add(1013904223);
                self.noise_val = ((self.noise_seed >> 16) as i16) as f32 / 32768.0;
            }
            self.noise_val
        } else {
            let (pcm, cache) = (&*self.pcm, &self.cache);
            let n = pcm.len();
            let at = |i: i64| -> f32 {
                let n = n as i64;
                let j = if i < 0 {
                    0
                } else if i < n {
                    i
                } else {
                    match self.pcm.loop_to {
                        Some(l) => l as i64 + (i - n) % (n - l as i64).max(1),
                        None => return 0.0,
                    }
                };
                pcm.at(cache, j as usize) as f32 / 32768.0
            };
            let v = if o.hq {
                super::sinc::kernel().read(at, self.pos, step)
            } else {
                let i = self.pos.floor() as i64;
                let t = (self.pos - i as f64) as f32;
                let y = [at(i - 1), at(i), at(i + 1), at(i + 2)];
                let (c1, c2, c3) = (0.5 * (y[2] - y[0]), y[0] - 2.5 * y[1] + 2.0 * y[2] - 0.5 * y[3], 0.5 * (y[3] - y[0]) + 1.5 * (y[1] - y[2]));
                ((c3 * t + c2) * t + c1) * t + y[1]
            };
            let (n, loop_to) = (n as f64, self.pcm.loop_to);
            self.pos += step;
            if self.pos >= n {
                if let Some(l) = loop_to {
                    self.pos = l as f64 + (self.pos - n) % (n - l as f64).max(1.0);
                } else if let Some((next, step)) = self.chain.take() {
                    self.pos -= n;
                    self.pcm = next;
                    self.cache.get_mut().segs.clear();
                    self.step = step;
                    self.advanced += 1;
                } else {
                    self.active = false;
                }
            }
            v
        };
        [v * e * self.gain[0], v * e * self.gain[1]]
    }
}

/// The pitch step of a tone: output samples' worth of sample per output sample. A note is 128 fine steps of 1/128 semitone; the
/// sample plays at its recorded 44.1 kHz when the sound's note is the tone's centre note (989snd FUN_0001a7d4: the note's pitch
/// times 44100 / 48000 for a centre note that is not negative).
pub fn pitch(centre_note: i32, fine_tune: i32, note_fine: i32) -> f64 {
    let (c, scale) = if centre_note >= 0 { (centre_note, 44100.0 / RATE) } else { (-centre_note, 1.0) };
    2f64.powf((note_fine - (c * 128 + fine_tune)) as f64 / (12.0 * 128.0)) * scale
}
