//! The game's movies played in the window (`play`): the boot logos, the story's cinematics. The game's movie player (front end
//! 0x3AFAA8, and each level program's own copy) is answered by the port's runtime (`uyart::machine::movie_player`), which hands the
//! movie to `play_movie` below and waits until it returns, as the console's player blocks the game.
//!
//! # The entry
//!
//! ```ignore
//! pub fn play_movie(sector: u32, bytes: u32, lang: u32) -> bool
//! ```
//!
//! `sector` is the PSS stream's first disc sector and `bytes` its length (the player's a0 and a1: WAD1's record, the WAD's sector
//! added), `lang` the front-end language whose sound track plays (0 English, 1 French, 2 German, 3 Spanish, 4 Italian). It
//! returns when the movie is over: true when it was skipped (the player's return value, 1 when Start ended it), false when it
//! played to its end. It plays into the window and the device that `play.rs` set up: `attach` (the window's channel, the input,
//! the press script and the game's field counter) and `set_audio` (the device's ring, `audio.rs`) must have been called. A movie
//! that cannot be played (a stream the decoder refuses) is reported on
//! stderr and answered as skipped, so the game goes on.
//!
//! # What it does
//!
//! It streams the PSS from the disc, decodes its MPEG-2 pictures (the port's decoder, `mpeg::video`) and the language's sound track (`mpeg::audio::Track`), converts each picture to RGB (BT.601, limited range; the 4:2:0 colour
//! brought to every pixel guided by its brightness, so colour edges follow the picture's edges instead of its 2 x 2 blocks) and
//! hands it to the renderer, which upscales it to the window (`play.rs`: edge-directed, then sharpened). Cross, Circle or Start
//! (any device: Space, Enter, a click, Esc, A, B, Menu; or the `--press` script) skips the movie, as on the console Start does.
//!
//! The sound is the movie's clock. Its frames go into the audio ring (`audio.rs`) a little ahead of the device, and a picture is
//! shown when the device has reached its time (`Link::heard`), so picture and sound stay together however the two clocks run.
//! A picture the clock has passed by more than one is dropped, never the sound. The `--press` script runs on the game's field
//! count at the movie's start plus the movie's own time in fields, so a press can skip a movie some seconds in.

use crate::audio::Link;
use crate::input::{Input, CIRCLE, CROSS, START};
use crate::mpeg::{audio::Track, pss::{Demuxer, Packet}, video::{Decoder, Frame}};
use std::collections::VecDeque;
use std::io::{Read, Seek, SeekFrom};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A movie's picture: RGBA, alpha opaque.
pub struct Picture {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

struct Player {
    iso: String,
    show: std::sync::mpsc::SyncSender<crate::play::Msg>,
    input: Arc<Mutex<Input>>,
    quit: Arc<AtomicBool>,
    /// The `--press` script and the field the game thread is in (a movie plays within one field).
    presses: Vec<(u64, u16, u64)>,
    field: Arc<AtomicU64>,
}

static PLAYER: Mutex<Option<Player>> = Mutex::new(None);
static AUDIO: Mutex<Option<Arc<Link>>> = Mutex::new(None);

/// Plays the game's movies into the window from now on.
pub(crate) fn attach(iso: &str, show: std::sync::mpsc::SyncSender<crate::play::Msg>, input: Arc<Mutex<Input>>, quit: Arc<AtomicBool>, presses: &[(u64, u16, u64)], field: Arc<AtomicU64>) {
    *PLAYER.lock().expect("movie lock") = Some(Player { iso: iso.into(), show, input, quit, presses: presses.to_vec(), field });
}

/// The window's channel let go (`play.rs`: the game thread's end), so that it closes with the game's own and the renderer ends.
pub(crate) fn detach() {
    *PLAYER.lock().expect("movie lock") = None;
}

/// Whether `attach` gave a window to play movies into (the port's runtime gives the game's player a movie service only then).
pub fn attached() -> bool {
    PLAYER.lock().expect("movie lock").is_some()
}

/// The audio output the movies' sound goes to (made at the top of the game thread, before the machine's first field).
pub fn set_audio(link: Arc<Link>) {
    *AUDIO.lock().expect("movie audio lock") = Some(link);
}

/// The entry: see the module doc.
pub fn play_movie(sector: u32, bytes: u32, lang: u32) -> bool {
    let audio = AUDIO.lock().expect("movie audio lock").clone().expect("a movie plays with the game's audio output (audio::Audio::open, at the top of the game thread)");
    let g = PLAYER.lock().expect("movie lock");
    let p = g.as_ref().expect("a movie plays into the window `attach` gave");
    match play(p, &audio, [sector, bytes, lang]) {
        Ok(skipped) => {
            // Seen, played through or skipped (`seen.rs`, CUTSCENE SKIP); the profile not written stops the game.
            crate::seen::saw(crate::seen::Cutscene::Movie { sector }).unwrap_or_else(|e| panic!("play: movie at sector {sector:#x}: {e}"));
            skipped
        }
        Err(e) => {
            eprintln!("play: movie at sector {sector:#x} ({bytes} bytes): {e}");
            true
        }
    }
}

const FIELDS_PER_SECOND: f64 = 60000.0 / 1001.0;

/// MPEG-2 frame_rate_code -> pictures per second.
pub fn frame_rate(code: u8) -> f64 {
    match code {
        1 => 24000.0 / 1001.0,
        2 => 24.0,
        3 => 25.0,
        4 => 30000.0 / 1001.0,
        5 => 30.0,
        6 => 50.0,
        7 => 60000.0 / 1001.0,
        8 => 60.0,
        _ => panic!("MPEG-2 frame_rate_code {code}"),
    }
}
/// Seconds of sound kept in the ring ahead of the device.
const AHEAD: f64 = 0.5;
/// Pictures decoded and waiting at most (the sound's packets come within a few pictures of their time), and pictures read at
/// most before the sound has started.
const PICTURES_CAP: usize = 240;
const RATE: f64 = 48000.0;

/// Plays one movie; true when it was skipped.
fn play(p: &Player, audio: &Link, a: [u32; 3]) -> Result<bool, String> {
    let (lba, bytes) = (a[0], a[1] as u64);
    if bytes == 0 {
        // An empty slot of the movie table: nothing to stream, the player is done at once.
        return Ok(false);
    }
    let mut f = std::fs::File::open(&p.iso).map_err(|e| format!("{}: {e}", p.iso))?;
    f.seek(SeekFrom::Start(lba as u64 * 2048)).map_err(|e| e.to_string())?;
    // The fields before the movie are in the ring: the movie's sound follows them.
    let base = audio.movie_begin();
    let r = run(p, audio, base, f.take(bytes), a);
    audio.movie_end();
    r
}

fn run(p: &Player, audio: &Link, base: u64, src: impl Read, a: [u32; 3]) -> Result<bool, String> {
    let (lba, bytes) = (a[0], a[1]);
    let mut dm = Demuxer::new(src);
    let (mut dec, mut track) = (Decoder::new(), Track::new(a[2] as u8));
    let field0 = p.field.load(Ordering::SeqCst);
    // The buttons held when the movie starts do not skip it; a new press does (a press of the script counts as new).
    let mut held = p.input.lock().expect("input lock").field(0, true).buttons;
    println!("play: field {field0}: movie at sector {lba:#x}, {bytes} bytes");
    // The pictures waiting for their time (seconds into the movie), the first picture's time stamp (90 kHz) and frame rate code.
    let (mut pics, mut v0, mut rate) = (VecDeque::<(f64, Frame)>::new(), None::<u64>, 0u8);
    // The sound decoded before its start against the pictures was known, and the frames of sound put in the ring (from the
    // movie's time 0: the lead of silence counts).
    let (mut pending, mut sound, mut started) = (Vec::<[i16; 2]>::new(), 0u64, false);
    let (mut ended, mut over) = (false, None::<Instant>);
    let (mut decoded, mut shown, mut dropped, mut last_due) = (0u64, 0u64, 0u64, 0f64);
    let (t0, mut moved) = (Instant::now(), (Instant::now(), 0f64));
    let mut chunk = Vec::new();
    loop {
        if p.quit.load(Ordering::SeqCst) {
            return Ok(true);
        }
        // The movie's time: what the device has played of its sound; once the sound is over, the wall clock goes on from there.
        let heard = audio.heard();
        let secs = match over {
            Some(at) => sound as f64 / RATE + at.elapsed().as_secs_f64(),
            None => (heard - base as f64) / audio.rate(),
        };
        if over.is_none() && ended && secs >= sound as f64 / RATE - 0.001 {
            over = Some(Instant::now());
        }
        if heard > moved.1 + 1.0 {
            moved = (Instant::now(), heard);
        } else if started && over.is_none() && moved.0.elapsed() > Duration::from_secs(3) {
            return Err(format!("the sound has not moved for 3 s (at {secs:.2} s of the movie)"));
        }
        // A press of the devices or the script skips.
        let scripted = crate::pad_at(&p.presses, field0 + (secs.max(0.0) * FIELDS_PER_SECOND) as u64);
        let b = p.input.lock().expect("input lock").field(0, true).buttons | scripted;
        let pressed = b & !held;
        held = b;
        if pressed & (CROSS | CIRCLE | START) != 0 && crate::seen::may_skip(crate::seen::Cutscene::Movie { sector: lba }) {
            println!("play: movie skipped at {secs:.1} s: {shown} pictures shown ({dropped} dropped of {decoded}), {:.2} s of sound at {:.3} s of the mix", sound as f64 / RATE, base as f64 / RATE);
            return Ok(true);
        }
        // The pictures whose time has come: the newest is shown, any older one it overtook is dropped.
        let mut due = None;
        while pics.front().is_some_and(|(t, _)| *t <= secs) {
            dropped += due.is_some() as u64;
            due = pics.pop_front();
        }
        if let Some((_, fr)) = due {
            p.show.send(crate::play::Msg::Movie(rgba(&fr))).map_err(|_| "the renderer stopped")?;
            shown += 1;
        }
        if ended && pics.is_empty() && secs >= (sound as f64 / RATE).max(last_due + 1.0 / frame_rate(rate)) {
            break;
        }
        // More of the movie when the sound or the pictures are running out.
        // The ring's frames (its rate the mix's, AUDIO > MIXING) against the movie's 48 kHz frames put in.
        let waiting = (base as f64 + sound as f64 * audio.rate() / RATE - heard) / audio.rate();
        if ended || (started && pics.len() >= 2 && waiting >= AHEAD) {
            std::thread::sleep(Duration::from_millis(2));
            continue;
        }
        if pics.len() >= PICTURES_CAP {
            return Err(format!("{PICTURES_CAP} pictures are waiting and the sound is {waiting:.2} s ahead of the device: the sound does not keep up with the pictures"));
        }
        let mut fresh = Vec::new();
        match dm.next_packet()? {
            Some(Packet::Video { pts, data }) => {
                dec.push(&data, pts)?;
                while let Some(fr) = dec.next_frame() {
                    fresh.push(fr);
                }
            }
            Some(Packet::Audio { track: t, pts, data }) => {
                chunk.clear();
                track.push(t, pts, &data, &mut chunk)?;
                if started {
                    audio.push_movie(&chunk);
                    sound += chunk.len() as u64;
                } else {
                    pending.extend_from_slice(&chunk);
                }
            }
            None => {
                dec.flush()?;
                while let Some(fr) = dec.next_frame() {
                    fresh.push(fr);
                }
                ended = true;
            }
        }
        for fr in fresh {
            let pts = fr.pts.unwrap_or(0);
            let first = *v0.get_or_insert(pts);
            if decoded == 0 {
                rate = fr.frame_rate_code;
            }
            decoded += 1;
            last_due = pts.saturating_sub(first) as f64 / 90000.0;
            pics.push_back((last_due, fr));
        }
        // The sound starts at its own time stamp against the first picture's (a movie's sound may lag its first picture).
        if !started {
            if let (Some(v), Some(a0)) = (v0, track.pts0) {
                let lead = (a0.saturating_sub(v) as f64 / 90000.0 * RATE) as usize;
                let mut first = vec![[0i16; 2]; lead];
                first.append(&mut pending);
                audio.push_movie(&first);
                sound = first.len() as u64;
                started = true;
            }
        }
        if ended {
            if decoded == 0 {
                return Err("no picture in the stream".into());
            }
            if !started {
                return Err("no sound on the language track".into());
            }
            audio.end_of_stream();
        }
    }
    println!("play: movie played, {shown} pictures shown ({dropped} dropped of {decoded}), {:.2} s of sound at {:.3} s of the mix, in {:.1} s", sound as f64 / RATE, base as f64 / RATE, t0.elapsed().as_secs_f64());
    Ok(false)
}

/// BT.601 limited-range YCbCr 4:2:0 -> RGBA. MPEG-2 sites each colour sample on the even column and between two rows; a pixel's
/// colour is its four nearest samples, weighted by distance and by how close the brightness at each sample is to the pixel's own
/// (joint bilateral), so a colour does not bleed across an edge the brightness shows.
fn rgba(p: &Frame) -> Picture {
    let (w, h) = (p.width as usize, p.height as usize);
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    // The brightness at each colour sample: the two pixels beside it on its column.
    let mut at = vec![0f32; cw * ch];
    for j in 0..ch {
        for i in 0..cw {
            let (x, y0, y1) = ((2 * i).min(w - 1), (2 * j).min(h - 1), (2 * j + 1).min(h - 1));
            at[j * cw + i] = (p.y[y0 * w + x] as f32 + p.y[y1 * w + x] as f32) * 0.5;
        }
    }
    // Brightness differences well under this keep the distance weights; larger ones fall away as 1 / (1 + (d / RANGE)^2).
    const RANGE: f32 = 12.0;
    let mut out = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let cy = ((y as f32 - 0.5) * 0.5).clamp(0.0, (ch - 1) as f32);
        let (j0, fy) = (cy.floor() as usize, cy.fract());
        let j1 = (j0 + 1).min(ch - 1);
        for x in 0..w {
            let cx = x as f32 * 0.5;
            let (i0, fx) = ((cx.floor() as usize).min(cw - 1), cx.fract());
            let i1 = (i0 + 1).min(cw - 1);
            let luma = p.y[y * w + x] as f32;
            let (mut u, mut v, mut sum) = (0f32, 0f32, 0f32);
            for (i, j, d) in [(i0, j0, (1.0 - fx) * (1.0 - fy)), (i1, j0, fx * (1.0 - fy)), (i0, j1, (1.0 - fx) * fy), (i1, j1, fx * fy)] {
                let k = j * cw + i;
                let r = (luma - at[k]) / RANGE;
                let wgt = d / (1.0 + r * r) + d * 1e-4;
                u += wgt * p.cb[k] as f32;
                v += wgt * p.cr[k] as f32;
                sum += wgt;
            }
            let l = (luma - 16.0) * 1.164;
            let (u, v) = (u / sum - 128.0, v / sum - 128.0);
            let px = [l + 1.596 * v, l - 0.392 * u - 0.813 * v, l + 2.017 * u];
            out.extend(px.map(|c| c.round().clamp(0.0, 255.0) as u8));
            out.push(255);
        }
    }
    Picture { w: w as u32, h: h as u32, rgba: out }
}
