//! The game's movies, played into the emulated machine's headless capture (`uyaref frames --movies`, `uyaref sndcheck --movies`).
//! The movie player (front end 0x3afaa8) is a host stub in the oracle (rc3/movie.rs) that calls the service attached here with
//! the player's arguments: a0 = the PSS stream's disc sector, a1 = its length in bytes, a2 = the language (rac-engine
//! `track_for_language`), t0 = 1 for the attract movie at boot. The service plays the PSS with the port's decoders (`uya::mpeg`:
//! program stream, MPEG-2 video, the SShd/SSbd ADPCM track): every `every`-th picture goes to `<out>/mNN_fFFFFF.png`, the 48 kHz
//! stereo audio to the run's WAV (`record.rs` takes it with `take_audio`), placed at the audio's own presentation time against
//! the first picture. The movie runs on its own clock while the game waits in the player, as on the console. The press script
//! runs on the run's clock (the fields the machine ran plus the movies' time in fields, `at_field`): Start held during a movie
//! ends it as skipped (1), else it plays to its end (0).
use uya::cinema::frame_rate;
use uya::mpeg::{audio::Track, pss::{Demuxer, Packet}, video::{Decoder, Frame}};
use std::io::{Read, Seek, SeekFrom};
use std::sync::Mutex;

const START: u16 = 0x800;
const FIELDS_PER_SECOND: f64 = 60000.0 / 1001.0;

struct Capture {
    iso: String,
    out: String,
    every: u64,
    presses: Vec<(u64, u16, u64)>,
    /// The run's clock in fields: the fields the machine ran plus the movies' time (set before each field).
    field: u64,
    /// Fields of movie time so far.
    extra: u64,
    movies: u32,
    /// The last movie's audio, 48 kHz stereo, from its first picture on.
    audio: Vec<[i16; 2]>,
}

static CAPTURE: Mutex<Option<Capture>> = Mutex::new(None);

/// Plays the game's movies into `out` (every `every`-th picture) and the run's WAV.
pub fn capture(iso: &str, out: &str, every: u64, presses: &[(u64, u16, u64)]) {
    *CAPTURE.lock().unwrap() = Some(Capture { iso: iso.into(), out: out.into(), every, presses: presses.to_vec(), field: 0, extra: 0, movies: 0, audio: Vec::new() });
    ee_oracle::rc3::movie::set_service(service);
}

/// The frames loop is about to run field `f`: the run's clock for the press script (`f` plus the movies' time so far).
pub fn at_field(f: u64) -> u64 {
    match CAPTURE.lock().unwrap().as_mut() {
        Some(c) => {
            c.field = f + c.extra;
            c.field
        }
        None => f,
    }
}

/// The audio of the movie that just played (empty when none did).
pub fn take_audio() -> Vec<[i16; 2]> {
    CAPTURE.lock().unwrap().as_mut().map(|c| std::mem::take(&mut c.audio)).unwrap_or_default()
}

fn service(_cpu: &mut ee_oracle::cpu::Cpu, a: [u32; 4]) -> bool {
    let mut g = CAPTURE.lock().unwrap();
    let c = g.as_mut().expect("movie service attached without a capture");
    c.movies += 1;
    match play(c, a) {
        Ok(skipped) => skipped,
        Err(e) => panic!("movie {} (sector {:#x}, {} bytes): {e}", c.movies, a[0], a[1]),
    }
}

/// The pictures of one movie as they come out of the decoder (display order).
struct Pictures {
    n: u64,
    /// The first picture's time stamp (90 kHz) and frame_rate_code.
    pts0: Option<u64>,
    rate: u8,
    /// The press script's field at the movie's start.
    field0: u64,
}

impl Pictures {
    /// Saves every `every`-th picture; true when the press script holds Start at this picture's time.
    fn take(&mut self, p: Frame, c: &Capture) -> Result<bool, String> {
        let pts = p.pts.unwrap_or(0);
        let t0 = *self.pts0.get_or_insert(pts);
        if self.n == 0 {
            self.rate = p.frame_rate_code;
        }
        if self.n % c.every == 0 {
            let path = format!("{}/m{:02}_f{:05}.png", c.out, c.movies, self.n);
            std::fs::write(&path, uya::pic::png(&rgb(&p))).map_err(|e| format!("{path}: {e}"))?;
        }
        self.n += 1;
        let field = self.field0 + (pts.saturating_sub(t0) as f64 / 90000.0 * FIELDS_PER_SECOND) as u64;
        Ok(uya::pad_at(&c.presses, field) & START != 0)
    }

    fn seconds(&self) -> f64 {
        self.n as f64 / frame_rate(self.rate)
    }
}

fn play(c: &mut Capture, a: [u32; 4]) -> Result<bool, String> {
    let (lba, bytes, lang) = (a[0], a[1] as u64, a[2]);
    if bytes == 0 {
        // An empty slot of the movie table (the story intro's chain 0x3bd29c asks for six, the sixth is empty): nothing
        // to stream, the player is done at once.
        println!("movie {}: sector {lba:#x}, empty, ends at once", c.movies);
        return Ok(false);
    }
    let mut f =std::fs::File::open(&c.iso).map_err(|e| format!("{}: {e}", c.iso))?;
    f.seek(SeekFrom::Start(lba as u64 * 2048)).map_err(|e| e.to_string())?;
    let mut dm = Demuxer::new(f.take(bytes));
    let (mut dec, mut track) = (Decoder::new(), Track::new(lang as u8));
    let mut pcm = Vec::<[i16; 2]>::new();
    let mut pics = Pictures { n: 0, pts0: None, rate: 0, field0: c.field };
    println!("movie {}: sector {lba:#x}, {bytes} bytes, track {}, flag {}, at field {}", c.movies, track.id(), a[3], c.field);
    let mut skipped = false;
    while !skipped {
        match dm.next_packet()? {
            Some(Packet::Video { pts, data }) => {
                dec.push(&data, pts)?;
                while let Some(p) = dec.next_frame() {
                    skipped |= pics.take(p, c)?;
                }
            }
            Some(Packet::Audio { track: t, pts, data }) => track.push(t, pts, &data, &mut pcm)?,
            None => break,
        }
    }
    if !skipped {
        dec.flush()?;
        while let Some(p) = dec.next_frame() {
            pics.take(p, c)?;
        }
    }
    // The audio starts at its own time stamp against the first picture's. A movie lasts until both its pictures and its
    // sound are over (the last picture stays up through the sound's tail, as rac-engine's player ends one); a skipped
    // movie ends at the picture where Start came.
    let v0 = pics.pts0.ok_or("no picture in the stream")?;
    let lead = (track.pts0.ok_or("no audio on the language track")?.saturating_sub(v0) as f64 / 90000.0 * 48000.0) as usize;
    let mut audio = vec![[0i16; 2]; lead];
    audio.extend_from_slice(&pcm);
    let pictures = (pics.seconds() * 48000.0) as usize;
    if skipped {
        audio.truncate(pictures);
    }
    audio.resize(audio.len().max(pictures), [0, 0]);
    println!(
        "movie {}: {} pictures ({:.2}s, frame rate code {}), audio {:.2}s from {:.3}s, {}",
        c.movies,
        pics.n,
        pics.seconds(),
        pics.rate,
        pcm.len() as f64 / 48000.0,
        lead as f64 / 48000.0,
        if skipped { "skipped with Start" } else { "played to its end" }
    );
    c.extra += (audio.len() as f64 / 48000.0 * FIELDS_PER_SECOND).round() as u64;
    c.audio = audio;
    Ok(skipped)
}

/// BT.601 limited-range YCbCr 4:2:0 -> RGB.
fn rgb(p: &Frame) -> uya::pic::Image {
    let (w, h) = (p.width as usize, p.height as usize);
    let cw = w.div_ceil(2);
    let mut out = Vec::with_capacity(w * h * 3);
    for y in 0..h {
        for x in 0..w {
            let l = (p.y[y * w + x] as f32 - 16.0) * 1.164;
            let (u, v) = (p.cb[(y / 2) * cw + x / 2] as f32 - 128.0, p.cr[(y / 2) * cw + x / 2] as f32 - 128.0);
            let px = [l + 1.596 * v, l - 0.392 * u - 0.813 * v, l + 2.017 * u];
            out.extend(px.map(|c| c.round().clamp(0.0, 255.0) as u8));
        }
    }
    uya::pic::Image { w, h, rgb: out }
}
