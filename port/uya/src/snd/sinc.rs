//! The high-quality resampler of AUDIO > MIXING (PLAN 10): band-limited interpolation with a Kaiser-windowed sinc, used where
//! the original mix interpolates with a 4-point Hermite (a voice reading its sample at its pitch, `voice.rs`; the device output
//! following the ring at the device's clock, `audio.rs`; a movie's 48 kHz sound and the original reverb's 48 kHz network when the
//! device runs at another rate, `Stream`).
//!
//! The kernel is `HALF` zero crossings each side (`TAPS` = 32 taps at unity), windowed by a Kaiser window of beta `BETA` (about
//! 90 dB of stopband), tabulated at `RES` points a zero crossing and read with linear interpolation between them. Reading a
//! signal faster than its own rate (a voice pitched up: `step` above 1) widens the kernel by `step` and lowers its cutoff to
//! `1 / step` of the source's Nyquist, so what the faster reading would fold back below the output's Nyquist is filtered out
//! (with the Hermite it aliases); the width is capped at `MAX_STRETCH`.

const HALF: usize = 16;
pub const TAPS: usize = 2 * HALF;
const RES: usize = 512;
const BETA: f64 = 9.0;
const MAX_STRETCH: f64 = 4.0;

/// The tabulated kernel: sinc(x) times the Kaiser window, for x from 0 to `HALF` in steps of 1 / `RES`.
pub struct Kernel {
    table: Vec<f32>,
}

fn bessel_i0(x: f64) -> f64 {
    // The power series of I0; 40 terms reach f64's precision for the arguments here.
    let (mut sum, mut term) = (1.0, 1.0);
    for k in 1..40 {
        term *= (x / 2.0) * (x / 2.0) / (k * k) as f64;
        sum += term;
    }
    sum
}

impl Kernel {
    pub fn new() -> Kernel {
        let n = HALF * RES;
        let table = (0..=n + 1)
            .map(|i| {
                let x = i as f64 / RES as f64;
                if x >= HALF as f64 {
                    return 0.0;
                }
                let sinc = if x == 0.0 { 1.0 } else { (std::f64::consts::PI * x).sin() / (std::f64::consts::PI * x) };
                let r = x / HALF as f64;
                (sinc * bessel_i0(BETA * (1.0 - r * r).sqrt()) / bessel_i0(BETA)) as f32
            })
            .collect();
        Kernel { table }
    }

    /// The kernel at `x` zero crossings from its centre.
    #[inline]
    fn at(&self, x: f64) -> f32 {
        let a = x.abs() * RES as f64;
        let i = a as usize;
        if i >= HALF * RES {
            return 0.0;
        }
        let t = (a - i as f64) as f32;
        self.table[i] + (self.table[i + 1] - self.table[i]) * t
    }

    /// The signal `s` (sample `i`, any integer: the caller answers past its ends) read at the fractional position `pos`, moving on
    /// `step` source samples an output sample: the weights normalised to sum to 1, so a constant stays exact.
    #[inline]
    pub fn read(&self, s: impl Fn(i64) -> f32, pos: f64, step: f64) -> f32 {
        let stretch = step.clamp(1.0, MAX_STRETCH);
        let fc = 1.0 / stretch;
        let radius = (HALF as f64 * stretch).ceil() as i64;
        let c = pos.floor() as i64;
        let (mut acc, mut wsum) = (0f32, 0f32);
        for i in c - radius + 1..=c + radius {
            let w = self.at((i as f64 - pos) * fc);
            acc += s(i) * w;
            wsum += w;
        }
        if wsum != 0.0 { acc / wsum } else { 0.0 }
    }
}

/// The one kernel every user shares.
pub fn kernel() -> &'static Kernel {
    static K: std::sync::OnceLock<Kernel> = std::sync::OnceLock::new();
    K.get_or_init(Kernel::new)
}

/// A stereo stream resampled from one rate to another, a block at a time: the input pushed in, the output pulled at the ratio.
pub struct Stream {
    /// Input frames per output frame.
    ratio: f64,
    /// The input kept: what the kernel still reaches back to, and what is not read yet; `base` is the index of `buf[0]`.
    buf: std::collections::VecDeque<[f32; 2]>,
    base: i64,
    /// The read position, in input frames.
    pos: f64,
}

impl Stream {
    pub fn new(from: f64, to: f64) -> Stream {
        Stream { ratio: from / to, buf: std::collections::VecDeque::new(), base: -(HALF as i64), pos: 0.0 }
    }

    pub fn push(&mut self, f: [f32; 2]) {
        if self.buf.is_empty() {
            // The history before the first frame is silence.
            self.buf.extend(std::iter::repeat_n([0.0; 2], HALF));
        }
        self.buf.push_back(f);
    }

    /// The next output frame, if the input reaches far enough past it.
    pub fn pull(&mut self) -> Option<[f32; 2]> {
        let need = self.pos.floor() as i64 + (HALF as f64 * self.ratio.max(1.0)).ceil() as i64;
        if need >= self.base + self.buf.len() as i64 {
            return None;
        }
        let k = kernel();
        let out = [0, 1].map(|ch| {
            k.read(
                |i| {
                    let j = i - self.base;
                    if j < 0 { 0.0 } else { self.buf.get(j as usize).map_or(0.0, |f| f[ch]) }
                },
                self.pos,
                self.ratio,
            )
        });
        self.pos += self.ratio;
        // What the kernel no longer reaches is dropped.
        let keep_from = self.pos.floor() as i64 - (HALF as f64 * self.ratio.max(1.0)).ceil() as i64 - 1;
        while self.base < keep_from && !self.buf.is_empty() {
            self.buf.pop_front();
            self.base += 1;
        }
        Some(out)
    }
}
