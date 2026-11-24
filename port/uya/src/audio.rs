//! Live audio for `uya play` (plan item 3.4): the game's sound and its movies' sound through the default output device (cpal).
//!
//! What the device plays is the port's own sound engine's output (`snd::Engine`, which the runtime that runs the game's code
//! calls with the game's sound calls: `Audio::engine`) and, while a movie plays, the movie's sound. Each field the engine is pulled
//! for that field's frames, and they go into a ring (`Link`) that the device's callback empties. The movies' sound (`cinema.rs`,
//! `movie::Track`) goes into the same ring while the movie plays. The game's own volume settings act on the engine, as the
//! game sets them.
//!
//! Threads. The game thread (the EE's) only tells the mixer thread a field is over (`Audio::field`); the mixer thread pulls the
//! engine, so the voices' cost is not taken from the EE's field, and does not wait on the game thread to keep the ring filled.
//! The device's callback runs on cpal's thread.
//!
//! In step with the picture:
//! - A field makes 800.8 frames (200 per engine tick, 240 ticks a second): the mix is tied to the game's own time, but for a
//!   field that runs long (below). The device takes frames at the clock of the audio hardware; the game thread is paced by the system clock.
//!   The ring's fill is held near `TARGET` by changing the pace the callback reads at by at most `ADJ_MAX` (0.5 %, inaudible),
//!   so the two clocks do not drift apart and no latency builds up. A producer that gets ahead of the device (fields run flat out
//!   while the memory-card messages are answered) has what does not fit (`LIMIT`) dropped, counted: the sound never makes the
//!   game wait, and what is made while the game fast-forwards is not played.
//! - A field that runs long does not starve the device: between fields the mixer thread makes frames ahead of the game's fields
//!   when the ring runs low (`mix`), as the IOP's sound runs on through a long EE field, and the fields after it make theirs
//!   only once their count passes what was made ahead, of which no more than the ring holds (`LIMIT`) is owed: a field held
//!   for seconds (a level's first picture on a slow PC, the game standing between the control channel's commands) is time the
//!   game stood still, and owing it would leave the engine unpulled for as long once the ring ran dry, the game waiting on its
//!   sound (a travel's level load stalled on black). Should the ring still run dry (an underrun, counted), the device plays
//!   silence, smoothly faded, until `PRIME` frames are in the ring again, and then plays on.
//! - A movie plays inside one field. Its sound goes into the ring ahead of its pictures and the audio hardware's clock is the
//!   movie's clock: a picture is shown when `Link::heard` has reached its time (`cinema.rs`).
//!
//! AUDIO > MIXING (PLAN 10): with HIGH QUALITY the engine mixes at the output device's own rate (from the next start: the ring is
//! made at it, `Link::rate`), each voice reads its sample with the windowed sinc (`snd::sinc`), the device reads the ring with it
//! too, and a movie's 48 kHz sound comes into the ring resampled with it; ORIGINAL is the 48 kHz mix and the 4-point Hermite.
//!
//! `--mute`: the device gets silence and everything is still counted. `--wav FILE`: the frames go to the WAV as they go into the
//! ring (the mix, before the mute and before the resampling to the device's rate), less what a movie's skip threw away unplayed.
//! Its header is kept right after every write, so a run that is stopped leaves a WAV that plays. A ring that runs dry while the
//! last 100 ms it played were silent is not an underrun (nothing was lost).

use crate::snd::Engine;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::collections::VecDeque;
use std::io::{Seek, SeekFrom, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// One stereo 48 kHz frame of the mix.
pub type Frame = [i16; 2];

/// The 44 bytes that head a 16-bit stereo 48 kHz PCM WAV of `frames` frames (the device output's WAV, the engine tools', the
/// reference's recordings).
pub fn wav_header(frames: u64) -> [u8; 44] {
    wav_header_at(frames, 48000)
}

/// The same at `rate` frames a second.
pub fn wav_header_at(frames: u64, rate: u32) -> [u8; 44] {
    let data = (frames * 4) as u32;
    let mut b = [0u8; 44];
    b[0..4].copy_from_slice(b"RIFF");
    b[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    b[8..16].copy_from_slice(b"WAVEfmt ");
    for (i, v) in [16u32, 0x0002_0001, rate, rate * 4, 0x0010_0004].into_iter().enumerate() {
        b[16 + 4 * i..20 + 4 * i].copy_from_slice(&v.to_le_bytes());
    }
    b[36..40].copy_from_slice(b"data");
    b[40..44].copy_from_slice(&data.to_le_bytes());
    b
}

/// The original mix's rate (and a movie's sound's).
const RATE: f64 = 48000.0;
/// Frames in the ring before the device starts (or starts again after an underrun): about four fields.
const PRIME: usize = 3200;
/// The fill the rate control holds the ring's average at.
const TARGET: f64 = 2600.0;
/// The most frames the game's sound may have waiting: a field's frames that would take the ring above it are dropped.
const LIMIT: usize = 4800;
/// The rate control: the pace the ring is read at changes by `ADJ_GAIN` per frame the average fill is off the target, at most
/// `ADJ_MAX`; the fill's average runs over `AVG_SECS`.
const ADJ_MAX: f64 = 0.005;
const ADJ_GAIN: f64 = 4e-6;
const AVG_SECS: f64 = 0.5;
/// Frames the device's output fades in over when it starts after silence.
const FADE: f32 = 64.0;
/// A field that takes longer than this (ms of wall time from one `Audio::field` to the next) is "slow" in the log.
const SLOW_MS: f64 = 25.0;
/// Between fields the mixer looks at the ring this often, and makes frames ahead of the game's fields when the ring is below
/// `LOW` (25 ms of sound; in step with the game a field's frames come before the ring falls under about 1700).
const WAKE: Duration = Duration::from_millis(2);
const LOW: usize = 1200;

/// `--mute` and `--wav FILE` of `uya play`.
#[derive(Clone, Default)]
pub struct Options {
    pub mute: bool,
    pub wav: Option<String>,
    /// AUDIO > MIXING at the start: the mix at the device's own rate.
    pub device_rate: bool,
}

/// What `Link::stats` reports.
#[derive(Clone, Copy, Default)]
pub struct Stats {
    /// Frames of the mix handed over, and the ones the device has taken from the ring.
    pub pushed: u64,
    pub played: u64,
    /// Frames the device was given in all (the mix at the device's rate, silence included).
    pub device: u64,
    /// Times the ring ran dry while playing, and the device frames of silence that followed each until it was refilled.
    pub underruns: u64,
    pub gap: u64,
    /// Frames dropped because the ring was full (the game ran ahead of the device).
    pub overruns: u64,
    /// The ring's fill now, and its lowest and highest since the last report, in frames.
    pub fill: usize,
    pub min_fill: usize,
    pub max_fill: usize,
    /// The rate control's adjustment now.
    pub adj: f64,
}

/// Where the device was in the stream at its last callback.
#[derive(Clone, Copy)]
struct Anchor {
    at: Instant,
    /// The stream position (frames, fractional) at the callback's first output frame and after its last.
    pos: f64,
    end: f64,
    /// Seconds from the callback until its first frame is heard, and the stream frames per second.
    latency: f64,
    rate: f64,
}

struct Ring {
    /// The frames not yet passed; `buf[0]` is the one the read position is on, `frac` of the way to `buf[1]`.
    buf: VecDeque<Frame>,
    prev: [f32; 2],
    frac: f64,
    /// Frames passed and frames ever put in: `pushed - popped` is `buf.len()`.
    popped: u64,
    pushed: u64,
    playing: bool,
    /// Silence is expected (nothing has played yet, or a movie just ended): not an underrun.
    quiet: bool,
    /// No more frames are coming (the end of a movie's sound): the ring plays what it has and running dry is not an underrun.
    eof: bool,
    /// The last frames handed over were dropped (the game is running ahead of the device): running dry is not an underrun.
    skipping: bool,
    /// The rate control is on (the game's sound; off for a movie, which the picture follows).
    control: bool,
    gain: f32,
    last: [f32; 2],
    /// `popped` when the device last played a frame that was not silent.
    loud: u64,
    /// The frames last passed, for the windowed sinc's reach back (`snd::sinc::TAPS` / 2 of them).
    hist: VecDeque<[f32; 2]>,
    avg: f64,
    adj: f64,
    anchor: Anchor,
    heard: f64,
    stats: Stats,
}

/// The ring and what the game's threads share with the device's callback.
pub struct Link {
    ring: Mutex<Ring>,
    wav: Mutex<Option<Wav>>,
    /// Fields sent to the mixer thread and done by it (`barrier`).
    sent: AtomicU64,
    done: Mutex<u64>,
    done_cv: Condvar,
    /// Movies played so far (a field with a movie in it takes the movie's time).
    movies: AtomicU64,
    /// The ring's rate (the mix's: 48 kHz, or the device's own with AUDIO > MIXING), whether the device reads it with the windowed
    /// sinc, and a movie's sound on its way from its 48 kHz to the ring's rate.
    rate: f64,
    hq: std::sync::atomic::AtomicBool,
    movie_rs: Mutex<Option<crate::snd::sinc::Stream>>,
}

struct Wav {
    f: std::fs::File,
    frames: u64,
    rate: u32,
}

impl Wav {
    fn create(path: &str, rate: u32) -> Result<Wav, String> {
        let mut f = std::fs::File::create(path).map_err(|e| format!("audio: {path}: {e}"))?;
        f.write_all(&wav_header_at(0, rate)).map_err(|e| format!("audio: {path}: {e}"))?;
        Ok(Wav { f, frames: 0, rate })
    }

    /// Appends frames and puts the lengths in the header, so the file is a whole WAV after every write.
    fn write(&mut self, frames: &[Frame]) {
        let mut b = Vec::with_capacity(frames.len() * 4);
        for s in frames {
            b.extend_from_slice(&s[0].to_le_bytes());
            b.extend_from_slice(&s[1].to_le_bytes());
        }
        self.frames += frames.len() as u64;
        let r = self.f.write_all(&b).and_then(|_| self.header());
        r.expect("audio: writing the WAV");
    }

    /// Takes the last `frames` frames back out (they were never played).
    fn truncate(&mut self, frames: u64) {
        self.frames = self.frames.saturating_sub(frames);
        let r = self.f.set_len(44 + self.frames * 4).and_then(|_| self.header());
        r.expect("audio: writing the WAV");
    }

    fn header(&mut self) -> std::io::Result<()> {
        let h = wav_header_at(self.frames, self.rate);
        self.f.seek(SeekFrom::Start(0))?;
        self.f.write_all(&h)?;
        self.f.seek(SeekFrom::End(0)).map(|_| ())
    }
}

/// The 4-point Hermite (Catmull-Rom) interpolation of the four neighbouring samples `y` at `t` between `y[1]` and `y[2]`.
fn hermite(y: [f32; 4], t: f32) -> f32 {
    let c1 = 0.5 * (y[2] - y[0]);
    let c2 = y[0] - 2.5 * y[1] + 2.0 * y[2] - 0.5 * y[3];
    let c3 = 0.5 * (y[3] - y[0]) + 1.5 * (y[1] - y[2]);
    ((c3 * t + c2) * t + c1) * t + y[1]
}

fn unit(f: Frame) -> [f32; 2] {
    [f[0] as f32 / 32768.0, f[1] as f32 / 32768.0]
}

impl Ring {
    /// Whether the device played something in the last 100 ms (a ring that runs dry in silence loses nothing).
    fn heard_lately(&self) -> bool {
        self.popped.saturating_sub(self.loud) < 4800
    }

    /// Silence: what the device played last, faded out.
    fn silence(&mut self) -> [f32; 2] {
        if !self.quiet && !self.eof && !self.skipping && self.heard_lately() {
            self.stats.gap += 1;
        }
        self.last = self.last.map(|x| if x.abs() < 1e-5 { 0.0 } else { x * 0.93 });
        self.last
    }

    /// The device's next frame, the read position moving on by `step` frames of the mix (read with the windowed sinc with `hq`).
    fn next(&mut self, step: f64, hq: bool) -> [f32; 2] {
        if !self.playing {
            let need = if self.eof { 3 } else { PRIME };
            if self.buf.len() < need {
                return self.silence();
            }
            self.playing = true;
            self.quiet = false;
            self.gain = 0.0;
            self.avg = self.buf.len() as f64;
        }
        if self.buf.len() < 3 {
            // The ring ran dry.
            self.playing = false;
            if !self.eof && !self.skipping && self.heard_lately() {
                self.stats.underruns += 1;
            }
            return self.silence();
        }
        let (a, b, c) = (unit(self.buf[0]), unit(self.buf[1]), unit(self.buf[2]));
        let t = self.frac as f32;
        self.gain = (self.gain + 1.0 / FADE).min(1.0);
        let out = if hq {
            let (buf, hist) = (&self.buf, &self.hist);
            let k = crate::snd::sinc::kernel();
            [0, 1].map(|ch| {
                let s = |i: i64| -> f32 {
                    if i < 0 {
                        hist.get(hist.len().wrapping_sub((-i) as usize)).map_or(0.0, |f| f[ch])
                    } else {
                        buf.get(i as usize).map_or(0.0, |f| unit(*f)[ch])
                    }
                };
                k.read(s, self.frac, step) * self.gain
            })
        } else {
            [0, 1].map(|k| hermite([self.prev[k], a[k], b[k], c[k]], t) * self.gain)
        };
        self.last = out;
        if out[0].abs() > 1e-4 || out[1].abs() > 1e-4 {
            self.loud = self.popped;
        }
        self.frac += step;
        while self.frac >= 1.0 {
            match self.buf.pop_front() {
                Some(f) => {
                    self.prev = unit(f);
                    self.hist.push_back(self.prev);
                    if self.hist.len() > crate::snd::sinc::TAPS / 2 {
                        self.hist.pop_front();
                    }
                    self.popped += 1;
                    self.frac -= 1.0;
                }
                None => {
                    self.frac = 0.0;
                    break;
                }
            }
        }
        out
    }

    /// Everything waiting is dropped and the device falls silent, quietly (the end of a movie; a skipped movie). Returns the
    /// number of frames dropped.
    fn quiesce(&mut self) -> u64 {
        let dropped = self.buf.len() as u64;
        self.popped += dropped;
        self.buf.clear();
        self.frac = 0.0;
        self.prev = [0.0; 2];
        self.playing = false;
        self.quiet = true;
        self.heard = self.popped as f64;
        self.anchor = Anchor { at: Instant::now(), pos: self.heard, end: self.heard, ..self.anchor };
        dropped
    }
}

impl Link {
    fn new(wav: Option<Wav>, rate: f64) -> Link {
        let anchor = Anchor { at: Instant::now(), pos: 0.0, end: 0.0, latency: 0.0, rate };
        let ring = Ring {
            buf: VecDeque::with_capacity(1 << 16),
            prev: [0.0; 2],
            frac: 0.0,
            popped: 0,
            pushed: 0,
            playing: false,
            quiet: true,
            eof: false,
            skipping: false,
            control: true,
            gain: 0.0,
            last: [0.0; 2],
            loud: 0,
            hist: VecDeque::new(),
            avg: 0.0,
            adj: 0.0,
            anchor,
            heard: 0.0,
            stats: Stats { min_fill: usize::MAX, ..Stats::default() },
        };
        Link { ring: Mutex::new(ring), wav: Mutex::new(wav), sent: AtomicU64::new(0), done: Mutex::new(0), done_cv: Condvar::new(), movies: AtomicU64::new(0), rate, hq: std::sync::atomic::AtomicBool::new(false), movie_rs: Mutex::new(None) }
    }

    /// The ring's rate (frames a second): 48 kHz, or the device's own with AUDIO > MIXING.
    pub fn rate(&self) -> f64 {
        self.rate
    }

    /// A movie's sound (48 kHz) into the ring, at the ring's rate (resampled with the windowed sinc when that is another).
    pub fn push_movie(&self, frames: &[Frame]) {
        if self.rate == RATE {
            return self.push(frames, usize::MAX);
        }
        let mut out = Vec::with_capacity((frames.len() as f64 * self.rate / RATE) as usize + 2);
        {
            let mut g = self.movie_rs.lock().expect("movie resampler lock");
            let rs = g.get_or_insert_with(|| crate::snd::sinc::Stream::new(RATE, self.rate));
            for f in frames {
                rs.push(unit(*f));
                while let Some(o) = rs.pull() {
                    out.push(o.map(|x| (x * 32767.0).clamp(-32768.0, 32767.0) as i16));
                }
            }
        }
        self.push(&out, usize::MAX);
    }

    fn ring(&self) -> MutexGuard<'_, Ring> {
        self.ring.lock().expect("audio ring lock")
    }

    /// The device's callback: fills `out` (interleaved, `channels` a frame) with the mix at the device's `rate`.
    /// `latency` is the time until the buffer's first frame is heard.
    fn render(&self, out: &mut [f32], channels: usize, rate: f64, latency: f64, mute: bool) {
        let at = Instant::now();
        let mut g = self.ring();
        let r = &mut *g;
        let n = out.len() / channels;
        if r.control && r.playing {
            r.avg += (r.buf.len() as f64 - r.avg) * (n as f64 / rate / AVG_SECS).min(1.0);
            r.adj = ((r.avg - TARGET) * ADJ_GAIN).clamp(-ADJ_MAX, ADJ_MAX);
        } else {
            r.adj = 0.0;
        }
        let step = self.rate / rate * (1.0 + r.adj);
        let (start, popped) = (r.popped as f64 + r.frac, r.popped);
        let hq = self.hq.load(Ordering::Relaxed);
        out.fill(0.0);
        for frame in out.chunks_exact_mut(channels) {
            let s = r.next(step, hq);
            let s = if mute { [0.0, 0.0] } else { s };
            if channels == 1 {
                frame[0] = (s[0] + s[1]) * 0.5;
            } else {
                frame[0] = s[0];
                frame[1] = s[1];
            }
        }
        r.stats.device += n as u64;
        r.stats.played += r.popped - popped;
        // Only a buffer that took frames from the ring moves the clock: a buffer of silence after the ring ran dry would put the
        // anchor at the end with this callback's latency still to come, every callback again, and the last frames (one device
        // latency of them) would never count as heard.
        let end = r.popped as f64 + r.frac;
        if end != start {
            r.anchor = Anchor { at, pos: start, end, latency, rate: self.rate * (1.0 + r.adj) };
        }
        if r.playing {
            r.stats.min_fill = r.stats.min_fill.min(r.buf.len());
        }
    }

    /// Puts frames in the ring, unless more than `limit` frames would be waiting: then they are dropped (counted) and the ring
    /// plays what it has. The WAV gets them either way.
    pub fn push(&self, frames: &[Frame], limit: usize) {
        if frames.is_empty() {
            return;
        }
        {
            let mut g = self.ring();
            if g.buf.len() + frames.len() > limit {
                g.stats.overruns += frames.len() as u64;
                g.skipping = true;
            } else {
                g.skipping = false;
                g.buf.extend(frames.iter().copied());
                g.pushed += frames.len() as u64;
                g.stats.pushed += frames.len() as u64;
                let fill = g.buf.len();
                g.stats.max_fill = g.stats.max_fill.max(fill);
            }
        }
        self.note(frames);
    }

    /// Frames for the WAV only: the silence of the fields before the driver exists keeps the WAV in step with the game's time,
    /// and the device needs none of it (nothing is playing, so nothing can run dry).
    pub fn note(&self, frames: &[Frame]) {
        if let Some(w) = self.wav.lock().expect("audio wav lock").as_mut() {
            w.write(frames);
        }
    }

    /// While the game's sound plays (no movie), the frames that bring a ring below `low` back to `target`: what the mixer makes
    /// ahead of the game's fields while a field runs long.
    fn short_of(&self, low: usize, target: usize) -> Option<usize> {
        let r = self.ring();
        (r.control && r.playing && r.buf.len() < low).then(|| target - r.buf.len())
    }

    /// Times the ring ran dry while playing.
    fn underruns(&self) -> u64 {
        self.ring().stats.underruns
    }

    /// The ring's numbers (the lowest and highest fill start again).
    pub fn stats(&self) -> Stats {
        let mut r = self.ring();
        let s = Stats { fill: r.buf.len(), adj: r.adj, ..r.stats };
        r.stats.min_fill = usize::MAX;
        r.stats.max_fill = r.buf.len();
        s
    }

    /// Waits until the mixer thread has put every field it was sent into the ring.
    pub fn barrier(&self) {
        let sent = self.sent.load(Ordering::SeqCst);
        let mut d = self.done.lock().expect("audio done lock");
        let t0 = Instant::now();
        while *d < sent {
            assert!(t0.elapsed() < Duration::from_secs(10), "audio: the mixer thread has not finished its fields in 10 s");
            d = self.done_cv.wait_timeout(d, Duration::from_millis(50)).expect("audio done lock").0;
        }
    }

    /// A movie is about to play: the fields before it are in the ring, and the rate control is off (the pictures follow the
    /// device's clock). Returns the stream position (frames) its first frame will have.
    pub fn movie_begin(&self) -> u64 {
        self.barrier();
        *self.movie_rs.lock().expect("movie resampler lock") = None;
        let mut r = self.ring();
        r.control = false;
        r.eof = false;
        r.pushed
    }

    /// The movie's sound is all in the ring: the device plays what is there and may run dry.
    pub fn end_of_stream(&self) {
        self.ring().eof = true;
    }

    /// The movie is over or skipped: nothing waiting is played (and the WAV, which ends with those frames, loses them), and the
    /// game's sound starts again from its own priming.
    pub fn movie_end(&self) {
        let mut r = self.ring();
        let dropped = r.quiesce();
        r.control = true;
        r.eof = false;
        drop(r);
        if let Some(w) = self.wav.lock().expect("audio wav lock").as_mut() {
            w.truncate(dropped);
        }
        self.movies.fetch_add(1, Ordering::SeqCst);
    }

    /// The stream position (frames) being heard now: the device's clock, extrapolated between its callbacks.
    pub fn heard(&self) -> f64 {
        let mut r = self.ring();
        let a = r.anchor;
        let h = (a.pos + (a.at.elapsed().as_secs_f64() - a.latency) * a.rate).min(a.end).max(r.heard);
        r.heard = h;
        h
    }
}

/// The live audio output: made at the top of the game thread, one `field` per field the machine ran. Dropping it stops the device.
pub struct Audio {
    link: Arc<Link>,
    tx: Option<SyncSender<()>>,
    mixer: Option<std::thread::JoinHandle<()>>,
    engine: Arc<Mutex<Engine>>,
    _stream: cpal::Stream,
    fields: u64,
    /// The time of the last `field` call, the slowest gap and the number of gaps over `SLOW_MS`.
    last: Instant,
    slowest: f64,
    slow: u64,
    t0: Instant,
    /// The underruns and movies counted so far.
    underruns: u64,
    movies: u64,
}

impl Audio {
    /// Opens the default output device and the engine the game's sound calls go to (with the module image of the disc at `iso`).
    pub fn open(o: &Options, iso: &str) -> Result<Audio, String> {
        let module = uyafmt::sbank::sound_modules(iso)?;
        let host = cpal::default_host();
        let device = host.default_output_device().ok_or("audio: no default output device")?;
        let name = device.description().map(|d| d.to_string()).map_err(|e| format!("audio: the output device: {e}"))?;
        let supported = device.default_output_config().map_err(|e| format!("audio: {name}: {e}"))?;
        let format = supported.sample_format();
        let config: cpal::StreamConfig = supported.into();
        let (rate, channels) = (config.sample_rate as f64, config.channels as usize);
        // AUDIO > MIXING: the mix at the device's own rate, or the original 48 kHz.
        let mix_rate = if o.device_rate { rate } else { RATE };
        let wav = o.wav.as_deref().map(|w| Wav::create(w, mix_rate as u32)).transpose()?;
        let link = Arc::new(Link::new(wav, mix_rate));
        if channels == 0 {
            return Err(format!("audio: {name} has no channels"));
        }
        let stream = match format {
            cpal::SampleFormat::F32 => build::<f32>(&device, config, link.clone(), o.mute),
            cpal::SampleFormat::I16 => build::<i16>(&device, config, link.clone(), o.mute),
            cpal::SampleFormat::I32 => build::<i32>(&device, config, link.clone(), o.mute),
            cpal::SampleFormat::U16 => build::<u16>(&device, config, link.clone(), o.mute),
            f => return Err(format!("audio: {name}: the sample format {f:?} is not handled")),
        }
        .map_err(|e| format!("audio: {name}: {e}"))?;
        stream.play().map_err(|e| format!("audio: {name}: {e}"))?;
        println!(
            "audio: {name}: {rate} Hz, {channels} channels, {format:?}; the mix at {mix_rate} Hz{}{}",
            if o.mute { "; muted (the device gets silence, everything is counted)" } else { "" },
            o.wav.as_deref().map(|w| format!("; the mix goes to {w}")).unwrap_or_default()
        );
        // The mixer thread: the engine's voices are not mixed on the EE's thread. A few fields wait in the channel.
        let mut e = Engine::new(&module)?;
        e.set_rate(mix_rate);
        let engine = Arc::new(Mutex::new(e));
        let (tx, rx) = sync_channel::<()>(6);
        let (l, e) = (link.clone(), engine.clone());
        let mixer = std::thread::Builder::new().name("uya mixer".into()).spawn(move || mix(l, e, rx)).map_err(|e| format!("audio: mixer thread: {e}"))?;
        crate::cinema::set_audio(link.clone());
        let now = Instant::now();
        Ok(Audio { link, tx: Some(tx), mixer: Some(mixer), engine, _stream: stream, fields: 0, last: now, slowest: 0.0, slow: 0, t0: now, underruns: 0, movies: 0 })
    }

    /// AUDIO > REVERB, from the next frame on: the clean network's return or the chip's.
    pub fn set_clean_reverb(&self, on: bool) {
        self.engine.lock().expect("sound engine lock").set_clean_reverb(on);
    }

    /// AUDIO > MIXING's resampling, from the next frame on: the voices' and the device's read with the windowed sinc, or the
    /// original Hermite (the mix's rate is the one the game started with).
    pub fn set_hq(&self, hq: bool) {
        self.engine.lock().expect("sound engine lock").set_hq(hq);
        self.link.hq.store(hq, Ordering::Relaxed);
    }

    /// The engine the game's sound calls go to (`snd::Engine::call`): the runtime locks it for each call.
    pub fn engine(&self) -> Arc<Mutex<Engine>> {
        self.engine.clone()
    }

    /// After every field the game ran: the engine is pulled for the field's frames (800.8 a field, whatever the field took in wall
    /// time) and they go to the device.
    pub fn field(&mut self) -> Result<(), String> {
        self.link.sent.fetch_add(1, Ordering::SeqCst);
        self.fields += 1;
        self.tx.as_ref().expect("the sender lives until drop").send(()).map_err(|_| "audio: the mixer thread stopped")?;
        let now = Instant::now();
        let mut gap = now.duration_since(self.last).as_secs_f64() * 1000.0;
        self.last = now;
        // A field with a movie in it took the movie's time: it is not a slow field.
        let movies = self.link.movies.load(Ordering::SeqCst);
        if movies != self.movies {
            self.movies = movies;
            gap = 0.0;
        }
        self.slowest = self.slowest.max(gap);
        self.slow += (gap > SLOW_MS) as u64;
        let u = self.link.underruns();
        if u > self.underruns {
            if self.underruns < 12 {
                println!("audio: field {}: the ring ran dry (underrun {u}) after a field of {gap:.1} ms", self.fields);
            }
            self.underruns = u;
        }
        if self.fields % 300 == 0 {
            let s = self.link.stats();
            println!(
                "audio: field {}: {} frames of the mix ({:.2} s) handed over, {} played, {} device frames; underruns {} ({} frames of gap), dropped {} frames; \
                 buffer {} frames (lowest {}, highest {}), rate adjustment {:+.3} %; fields over {SLOW_MS} ms: {}, slowest {:.1} ms",
                self.fields,
                s.pushed,
                s.pushed as f64 / self.link.rate,
                s.played,
                s.device,
                s.underruns,
                s.gap,
                s.overruns,
                s.fill,
                if s.min_fill == usize::MAX { 0 } else { s.min_fill },
                s.max_fill,
                s.adj * 100.0,
                self.slow,
                self.slowest
            );
            self.slowest = 0.0;
        }
        Ok(())
    }
}

impl Drop for Audio {
    fn drop(&mut self) {
        self.tx.take();
        if let Some(h) = self.mixer.take() {
            let _ = h.join();
        }
        let s = self.link.stats();
        println!("audio: {:.1} s: {} frames of the mix handed over, {} played, underruns {} ({} frames of gap), dropped {} frames", self.t0.elapsed().as_secs_f64(), s.pushed, s.played, s.underruns, s.gap, s.overruns);
    }
}

/// The mixer thread: each field's frames, pulled from the engine and put in the ring (a field is 1001 / 60000 s: 800.8 frames);
/// and between fields, every `WAKE`, the frames a ring below `LOW` needs to be back at `TARGET` while the game's sound plays. So a
/// field that runs long (the game thread held by the renderer loading a level's data for its first picture) does not starve the
/// device: the engine runs on, as the IOP's sound runs through a long EE field. What was made ahead counts against the fields
/// that follow, up to what the ring holds (`LIMIT`): a field's frames are made only once the fields' count passes what is
/// owed. Past `LIMIT` the game's time stood still while the device was fed, and nothing is owed for it: owed seconds would
/// leave the engine unpulled for as many seconds of fields once the ring ran dry, and the game, waiting on its sound, stalls.
fn mix(link: Arc<Link>, engine: Arc<Mutex<Engine>>, rx: Receiver<()>) {
    raise_priority();
    let (mut fields, mut made) = (0u64, 0u64);
    let mut buf: Vec<Frame> = Vec::new();
    let make = |n: usize, buf: &mut Vec<Frame>| {
        buf.resize(n, [0, 0]);
        engine.lock().expect("sound engine lock").pull_i16(buf);
        link.push(buf, LIMIT);
    };
    loop {
        match rx.recv_timeout(WAKE) {
            Ok(()) => {
                fields += 1;
                let to = (fields as f64 * link.rate * 1001.0 / 60000.0) as u64;
                if to > made {
                    make((to - made) as usize, &mut buf);
                    made = to;
                }
                *link.done.lock().expect("audio done lock") += 1;
                link.done_cv.notify_all();
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if let Some(n) = link.short_of(LOW, TARGET as usize) {
                    make(n, &mut buf);
                    let due = (fields as f64 * link.rate * 1001.0 / 60000.0) as u64;
                    made = (made + n as u64).min(due + LIMIT as u64);
                }
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

/// The device's stream for sample type `T`.
fn build<T>(device: &cpal::Device, config: cpal::StreamConfig, link: Arc<Link>, mute: bool) -> Result<cpal::Stream, cpal::Error>
where
    T: cpal::SizedSample + cpal::FromSample<f32>,
{
    let (rate, channels) = (config.sample_rate as f64, config.channels as usize);
    let mut scratch: Vec<f32> = Vec::new();
    let mut promoted = false;
    device.build_output_stream(
        config,
        move |data: &mut [T], info: &cpal::OutputCallbackInfo| {
            if !promoted {
                promoted = true;
                eprintln!("audio: the device's thread: {}; first buffer {} frames", raise_priority(), data.len() / channels);
            }
            let ts = info.timestamp();
            let latency = ts.playback.saturating_duration_since(ts.callback).as_secs_f64().min(0.25);
            scratch.resize(data.len(), 0.0);
            link.render(&mut scratch, channels, rate, latency, mute);
            for (d, s) in data.iter_mut().zip(&scratch) {
                *d = T::from_sample(*s);
            }
        },
        |e| eprintln!("audio: {e}"),
        None,
    )
}

/// The calling thread into the Windows multimedia scheduler's "Pro Audio" class (else above-normal priority): a normal-priority
/// thread loses the CPU to the game's busy threads for longer than the device's 10 ms period, and the device then plays a gap.
/// Returns what was set.
#[cfg(windows)]
fn raise_priority() -> &'static str {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LoadLibraryA(name: *const u8) -> *mut c_void;
        fn GetProcAddress(module: *mut c_void, name: *const u8) -> *mut c_void;
        fn GetCurrentThread() -> *mut c_void;
        fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
    }
    // SAFETY: plain Win32 calls on the current thread; avrt.dll is a system library and its export has this signature.
    unsafe {
        let m = LoadLibraryA(c"avrt.dll".as_ptr().cast());
        let f = if m.is_null() { std::ptr::null_mut() } else { GetProcAddress(m, c"AvSetMmThreadCharacteristicsW".as_ptr().cast()) };
        if !f.is_null() {
            type SetCharacteristics = unsafe extern "system" fn(*const u16, *mut u32) -> *mut c_void;
            let f = std::mem::transmute::<*mut c_void, SetCharacteristics>(f);
            let task: Vec<u16> = "Pro Audio\0".encode_utf16().collect();
            let mut index = 0u32;
            if !f(task.as_ptr(), &mut index).is_null() {
                return "MMCSS Pro Audio";
            }
        }
        // THREAD_PRIORITY_ABOVE_NORMAL
        if SetThreadPriority(GetCurrentThread(), 1) != 0 {
            return "above normal";
        }
    }
    "normal (could not be raised)"
}

#[cfg(not(windows))]
fn raise_priority() -> &'static str {
    "normal"
}
