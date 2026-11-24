//! The game's sound as the emulated PS2 plays it, recorded to a WAV: an instrument (`uyaref sound`, `uyaref frames --wav`), not
//! what `uya play` plays. Each field the oracle's 989snd driver hands over its SPU2 register writes (rac-engine `spumix::drain`), which
//! rac-engine's SPU2 voice mixer replays on the 48 voices, offline, at the driver's own 240 Hz tick (200 samples of 48 kHz per
//! tick). Before the driver exists (the boot before 989snd's first call) the output is silence at the field rate, so the sound
//! stays in step with the pictures of the same run. The port's own sound engine (`uya::snd`) is what the device plays; this is the
//! emulated sound the engine's output is set against.
use rac_engine::spumix::{self, SpuVoices};
use uya::audio::{wav_header, Frame};

/// 48 kHz samples per NTSC field (59.94 Hz).
const SAMPLES_PER_FIELD: f64 = 48000.0 * 1001.0 / 60000.0;

/// The emulated mix of the game's sound, field by field. The mixer's clock is the driver's tick, not the number of fields: each field
/// renders as many frames as the driver's ticks since the mixer started (200 each) come to.
pub struct Mixer {
    /// The mixer, the driver tick it started at and the frames it has rendered.
    mix: Option<(SpuVoices, u32, u64)>,
    /// Fields and frames of silence made before the driver existed.
    silent: (u64, u64),
}

impl Mixer {
    pub fn new() -> Mixer {
        Mixer { mix: None, silent: (0, 0) }
    }

    /// A field in which the driver does not exist yet: silence at the field rate (800.8 frames a field).
    pub fn silence(&mut self) -> Vec<Frame> {
        self.silent.0 += 1;
        let to = (self.silent.0 as f64 * SAMPLES_PER_FIELD) as u64;
        let n = to - self.silent.1;
        self.silent.1 = to;
        vec![[0, 0]; n as usize]
    }

    /// A field of the driver: its SPU2 writes replayed on the voices, rendered up to the driver's tick. The first field only
    /// starts the mixer (it renders no frames), so that the sound is as late as the driver's first tick.
    pub fn batch(&mut self, b: spumix::Batch) -> Vec<Frame> {
        let (mix, t0, rendered) = self.mix.get_or_insert_with(|| (SpuVoices::new(b.tick, false), b.tick, 0));
        mix.sync_core(&b.core, false);
        mix.push(b.ev);
        let to = spumix::SAMPLES_PER_TICK * b.tick.wrapping_sub(*t0) as u64;
        let mut out = Vec::with_capacity(to.saturating_sub(*rendered) as usize);
        while *rendered < to {
            let f = mix.frame();
            let side = |k: usize| ((f.dry[k].clamp(-32768, 32767) * f.master[k]) >> 15) as i16;
            out.push([side(0), side(1)]);
            *rendered += 1;
        }
        out
    }
}

pub struct Recorder {
    mixer: Mixer,
    pcm: Vec<Frame>,
}

impl Recorder {
    /// Starts recording the driver's SPU2 writes (before the first field).
    pub fn new(m: &mut ee_oracle::Machine) -> Recorder {
        m.record_sound();
        Recorder { mixer: Mixer::new(), pcm: Vec::new() }
    }

    /// After every field the machine ran.
    pub fn field(&mut self, m: &mut ee_oracle::Machine) {
        if m.cpu.snd.inner.is_none() {
            let s = self.mixer.silence();
            self.pcm.extend_from_slice(&s);
            return;
        }
        let b = spumix::drain(m);
        // A movie played in this field (movie.rs): its sound comes first, the game's goes on after it.
        let movie = crate::movie::take_audio();
        if !movie.is_empty() {
            println!("movie sound at {:.3}s of the WAV, {:.3}s long", self.seconds(), movie.len() as f64 / 48000.0);
        }
        self.pcm.extend_from_slice(&movie);
        let game = self.mixer.batch(b);
        self.pcm.extend_from_slice(&game);
    }

    pub fn seconds(&self) -> f64 {
        self.pcm.len() as f64 / 48000.0
    }

    /// 16-bit stereo 48 kHz PCM WAV.
    pub fn write(&self, path: &str) -> Result<(), String> {
        let mut b = Vec::with_capacity(44 + self.pcm.len() * 4);
        b.extend_from_slice(&wav_header(self.pcm.len() as u64));
        for s in &self.pcm {
            b.extend_from_slice(&s[0].to_le_bytes());
            b.extend_from_slice(&s[1].to_le_bytes());
        }
        std::fs::write(path, b).map_err(|e| format!("{path}: {e}"))?;
        println!("{path}: {:.1}s of 48 kHz stereo", self.seconds());
        Ok(())
    }
}
