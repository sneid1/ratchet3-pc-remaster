//! The stream engine: music, speech and stingers read from the disc as VAG clips and played on voices of their own. It is the second
//! half of the module (989snd 0x10000..0x13400, the calls 0x2A..0x37, 0x4F and 0x60; the EE side is func_0013CA30..0013CD00), and
//! this file does what that code does, function by function, with the disc read at once and the clip decoded whole instead of
//! streamed through the SPU2's two half rings (the port's disc is a file; nothing the game can ask of a stream depends on the
//! ring).
//!
//! # What a stream is
//!
//! `0x2A` starts the engine with a count of channel readers (the boot asks for 7). `0x2C` starts a stream: the first sector of a
//! clip (a VAGp file: a 0x30-byte header, big-endian data size at +0xC and sample rate at +0x10, then PS-ADPCM frames) and the byte
//! offset of its header in that sector, for the left channel and, for a stereo stream, a second clip for the right; a volume
//! (0..0x400), a pan angle, a voice group, the handle of a stream to queue this part behind (0: a new stream) and flags: 1 starts
//! it held (paused), 2 sends it to the reverb, 4 loops it, 8 and 0x40000 are the module's preload and timing flags (the port loads
//! a part whole, so they change nothing it does), 0x10 fails the call when the stream to queue behind is gone, 0x20 plays a mono
//! clip on two voices (the same sample on both sides at full), 0x40 gives the part its own volume and pan when it starts. The
//! answer is the stream's handle, `0x84000000 | record << 16 | serial` (989snd FUN_00017C64 and FUN_0000DEAC), 0 if it could not
//! start (no reader or record or voice free, or the engine not started), and 1 when it queued a part.
//!
//! A mono stream plays on one voice at the stream's pan; a mono stream with flag 0x20 on two (270 degrees on one, the other with
//! its sides swapped: both ears at the full volume); a stereo stream's left channel at 270 degrees and its right at 90 (FUN_00016B50,
//! FUN_00012BEx). Their volumes are FUN_0001B140's with the stream's volume (0..0x7F) as the only squared-free factor: `volume *
//! 0x102` as the level, then the pan table and the voice group's volume, like a sound's. A voice is keyed with the envelope words
//! 0x000F and 0x1FC0 and the highest priority, in the stream's voice group; a voice of a sound with a lower priority is taken if the
//! group is full.
//!
//! A part queued behind a playing one joins it without a gap (the voice goes on with the next clip as its own ends); `0x30` counts
//! the parts the stream has (the playing one and those behind it), `0x31`, `0x32` and `0x33` are the time played, left and the total
//! of the playing part in 1/1024 s (`0x32` is -1 for a looping one), `0x4F` is whether the stream has started (the first data is in:
//! the module's record flag 8; 1 once a tick has passed since the start, -1 for a handle that is gone), `0x2D` and `0x2E` pause and
//! resume it, `0x2F` and the sound stop (0x15) mute it and end it, `0x34` ends every stream and `0x35` shuts the engine down.
//!
//! `0x60` sets one of four ducking records (989snd FUN_0001BD48 and FUN_0001BAD8, run every fourth tick): while a sound or stream
//! of a trigger group plays, a mask of other groups' volumes falls to a floor, and rises back when it is over.

use super::rpc::Host;
use super::voice::{Pcm, Voice};
use super::{pan, Engine, RATE};
use std::collections::VecDeque;
use std::sync::Arc;

/// The handle type of a stream (bits 24..28).
pub const HANDLE_TYPE: u32 = 4;

/// Whether a handle is a stream's.
pub fn is_stream(handle: u32) -> bool {
    handle >> 24 & 0x1f == HANDLE_TYPE
}

/// One part of a stream: its clips decoded (the left channel's first), the sample rate they were recorded at, their length in
/// samples, and what the call that made the part asked for.
pub(super) struct Part {
    pcm: Vec<Arc<Pcm>>,
    rate: u32,
    samples: u32,
    looping: bool,
    reverb: bool,
    /// The volume (0..0x7F) and pan the part sets when it starts, when its call gave them (flag 0x40).
    vol: Option<i32>,
    pan: Option<i32>,
}

/// How a stream's voices are laid out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Mode {
    Mono,
    Doubled,
    Stereo,
}

/// A stream (the module's 0x78-byte record and its readers).
pub(super) struct Stream {
    handle: u32,
    group: usize,
    /// +0x10 the volume (0..0x7F) and +0x12 the pan angle.
    vol: i32,
    pan: i32,
    /// Record flag 2: held or paused. Flag 8: the first data is in. Flag 4: ended by a stop.
    paused: bool,
    ready: bool,
    stopped: bool,
    mode: Mode,
    voices: Vec<usize>,
    /// Channel readers the stream holds (989snd FUN_000173A8): one, or two for a stereo stream.
    readers: usize,
    /// The playing part's rate, length and loop flag, and the parts behind it (their clips decoded already).
    rate: u32,
    samples: u32,
    looping: bool,
    queue: VecDeque<Part>,
    /// How many joins of a queued part the first voice had when the stream last looked.
    joined: u32,
    /// The pan slide running on the stream (slide.rs).
    slide: Option<super::slide::Slide>,
}

/// The stream engine's state: the count of readers it was started with (0: not started), each record's serial and the streams.
#[derive(Default)]
pub struct Streams {
    count: usize,
    serial: Vec<u16>,
    list: Vec<Stream>,
}

/// A ducking record (989snd 0x25C28 + 0x18 * n): while a sound or stream of group `trigger` plays, the groups in `mask` fall by
/// `fall` a step (every fourth tick) to `floor` (16.16: 0x10000 is full), and rise back by `rise`.
#[derive(Clone, Copy)]
pub struct Duck {
    trigger: u32,
    mask: u32,
    floor: i32,
    fall: i32,
    rise: i32,
    cur: i32,
}

/// The clip at sector `lba`, its header `ring` bytes into the sector: its PS-ADPCM frames and the rate (989snd reads the header
/// from the first sector, then the data).
fn read_clip(host: &mut dyn Host, lba: u32, ring: u16) -> Result<(Vec<u8>, u32), String> {
    let head = host.read_disc(lba, 1)?;
    let h = head.get(ring as usize..ring as usize + 0x30).ok_or_else(|| format!("stream clip at sector {lba}: its header at +{ring:#x} runs past the sector"))?;
    if &h[0..4] != b"VAGp" {
        return Err(format!("stream clip at sector {lba} +{ring:#x}: {:x?} is not a VAGp header", &h[0..4]));
    }
    let data = u32::from_be_bytes([h[0xc], h[0xd], h[0xe], h[0xf]]) as usize;
    let rate = u32::from_be_bytes([h[0x10], h[0x11], h[0x12], h[0x13]]);
    let end = ring as usize + 0x30 + data;
    let bytes = host.read_disc(lba, end.div_ceil(2048) as u32)?;
    let frames = &bytes[ring as usize + 0x30..end];
    Ok((frames[..frames.len() / 16 * 16].to_vec(), rate))
}

/// The pan table's angle of a stream's channel: the stream's own plus the channel's (FUN_00016B50: a doubled voice and a stereo
/// stream's left channel 0x10E, its right 0x5A).
fn chan_angle(mode: Mode, chan: usize) -> i32 {
    match (mode, chan) {
        (Mode::Mono, _) => 0,
        (Mode::Doubled, _) | (Mode::Stereo, 0) => 0x10e,
        (Mode::Stereo, _) => 0x5a,
    }
}

impl Engine {
    /// The stream engine's group gain (989snd FUN_0001B82C): the group's volume squared over 0x400 times the ducking's square.
    pub(super) fn group_gain(&self, g: usize) -> f32 {
        if g > 14 {
            return 1.0;
        }
        let d = self.duck_scale[g] as i64;
        let dsq = if (1..0x10000).contains(&d) { d * d >> 16 } else { d };
        let gv = self.group_vol[g] as i64;
        ((((gv * gv) >> 10) * dsq) >> 16) as f32 / 1024.0
    }

    /// 0x2A: starts the engine with `count` channel readers (989snd FUN_00010124). It fails when it is already started or has no
    /// readers to make; every other failure of the module's is a memory it did not get.
    pub(super) fn stream_engine_start(&mut self, count: u32) -> u32 {
        if self.streams.count != 0 || count == 0 {
            return 0;
        }
        self.streams.count = count as usize;
        self.streams.serial = vec![1; count as usize];
        self.streams.list.clear();
        1
    }

    /// 0x35: shuts the engine down (989snd FUN_00017B00): every stream ends.
    pub(super) fn stream_engine_stop(&mut self) {
        self.stream_end_all();
        self.streams.count = 0;
    }

    pub(super) fn stream_find(&self, handle: u32) -> Option<usize> {
        self.streams.list.iter().position(|s| s.handle == handle)
    }

    /// A stream's pan angle, and the slide running on it (slide.rs).
    pub(super) fn stream_pan(&self, handle: u32) -> Option<i32> {
        self.stream_find(handle).map(|si| self.streams.list[si].pan)
    }

    pub(super) fn stream_slide(&mut self, handle: u32) -> Option<&mut Option<super::slide::Slide>> {
        let si = self.stream_find(handle)?;
        Some(&mut self.streams.list[si].slide)
    }

    /// A part of a stream from the clips at `lbas` (sector, header offset): read, decoded and joined to the call's flags.
    fn stream_part(&mut self, host: &mut dyn Host, clips: &[(u32, u16)], flags: u32, vol: Option<i32>, pan: Option<i32>) -> Result<Part, String> {
        let mut pcm: Vec<Arc<Pcm>> = Vec::new();
        let mut rate = 0;
        for &(lba, ring) in clips {
            let (frames, r) = read_clip(host, lba, ring)?;
            if rate != 0 && rate != r {
                return Err(format!("stream clips at sectors {} and {lba} have rates {rate} and {r}", clips[0].0));
            }
            // 989snd 0x14fe0..0x1500c gives the linked reader the head reader's length
            // and rate. Both voices change parts together. A shorter VAG has keyed off and
            // contributes silence for the rest of that part (level 7, scene 3).
            let len = pcm.first().map_or(frames.len() / 16 * 28, |head| head.len());
            rate = r;
            pcm.push(Arc::new(Pcm::adpcm(frames, len, (flags & 4 != 0).then_some(0))));
        }
        let samples = pcm[0].len() as u32;
        Ok(Part { pcm, rate, samples, looping: flags & 4 != 0, reverb: flags & 2 != 0, vol, pan })
    }

    /// 0x2C (989snd FUN_00010A00): starts a stream, or queues a part behind one. The words are the EE wrapper's eight: the first
    /// sector of the left clip and of the right (0: mono), the left header's offset with the volume (0..0x400) above it, the right's
    /// with the pan above it, the voice group, the stream to queue behind, a byte the module keeps for its class volumes and the
    /// flags. The answer is the stream's handle, 1 for a queued part and 0 for nothing started.
    pub(super) fn stream_start(&mut self, host: &mut dyn Host, w: &[u32]) -> Result<u32, String> {
        let g = |i: usize| w.get(i).copied().unwrap_or(0);
        if self.streams.count == 0 {
            // 989snd error 0x2F: the stream engine is not started.
            return Ok(0);
        }
        let (lba, lba_r) = (g(0), g(1));
        let ring = [g(2) as u16, g(3) as u16];
        let vol = (g(2) >> 16) as i32 * 0x7f >> 10;
        let pan = (g(3) >> 16) as u16 as i16 as i32;
        let group = (g(4) as i8 as i32).rem_euclid(16) as usize;
        let (after, flags) = (g(5), g(7));
        let own = |v: i32, p: i32| (flags & 0x40 != 0).then_some((v, p));
        // 989snd reads a stream's clips on its stream thread (FUN_00014864): a clip whose first sector is past the disc's end is a
        // sceCdRead the CD driver refuses, and the reader reports error 0x3E and ends the stream (record flag 4), unheard. The
        // EE's dialog lookup (level 35 0x429F90) can hand it such a sector from a table word that is not one.
        let unread = |host: &mut dyn Host, clips: &[(u32, u16)]| {
            let end = host.disc_sectors();
            clips.iter().find(|c| c.0 as u64 >= end).map(|c| c.0)
        };
        if after != 0 {
            if let Some(si) = self.stream_find(after) {
                let stereo = self.streams.list[si].mode == Mode::Stereo;
                if stereo && lba_r == 0 {
                    // FUN_00010A00 queues it on the left channel alone; the engine keeps a part's channels together.
                    return Err(format!("a stream part with one channel queued behind the stereo stream {after:#x}"));
                }
                let clips: Vec<(u32, u16)> = if stereo { vec![(lba, ring[0]), (lba_r, ring[1])] } else { vec![(lba, ring[0])] };
                if let Some(bad) = unread(host, &clips) {
                    println!("sound: tick {}: a part queued behind stream {after:#x} at sector {bad:#x}, past the disc's end: 989snd error 0x3E, the stream ends", self.ticks);
                    self.stream_stop(after);
                    return Ok(1);
                }
                let (v, p) = own(vol, pan).map_or((None, None), |(v, p)| (Some(v), Some(p.rem_euclid(360))));
                let part = self.stream_part(host, &clips, flags, v, p)?;
                let s = &mut self.streams.list[si];
                let first = s.queue.is_empty();
                s.queue.push_back(part);
                if first {
                    self.stream_chain(si);
                }
                return Ok(1);
            }
            if flags & 0x10 != 0 {
                return Ok(0);
            }
        }
        // A new stream: readers, a record and voices.
        let stereo = lba_r != 0;
        let mode = if stereo {
            Mode::Stereo
        } else if flags & 0x20 != 0 {
            Mode::Doubled
        } else {
            Mode::Mono
        };
        let readers = if stereo { 2 } else { 1 };
        let used: usize = self.streams.list.iter().map(|s| s.readers).sum();
        if used + readers > self.streams.count {
            return Ok(0);
        }
        let Some(rec) = (0..self.streams.count).find(|&r| !self.streams.list.iter().any(|s| s.handle >> 16 & 0xff == r as u32)) else { return Ok(0) };
        let clips: Vec<(u32, u16)> = if stereo { vec![(lba, ring[0]), (lba_r, ring[1])] } else { vec![(lba, ring[0])] };
        if let Some(bad) = unread(host, &clips) {
            // The record and its readers are the stream's until the next tick drops it, never started.
            let serial = self.streams.serial[rec].wrapping_add(1);
            let handle = 0x8400_0000 | (rec as u32) << 16 | serial as u32;
            println!("sound: tick {}: stream {handle:#x} at sector {bad:#x}, past the disc's end: 989snd error 0x3E, the stream ends unheard", self.ticks);
            self.streams.serial[rec] = serial;
            self.streams.list.push(Stream {
                handle,
                group,
                vol: 0,
                pan: 0,
                paused: flags & 1 != 0,
                ready: false,
                stopped: true,
                mode,
                voices: Vec::new(),
                readers,
                rate: 0,
                samples: 0,
                looping: false,
                queue: VecDeque::new(),
                joined: 0,
                slide: None,
            });
            return Ok(handle);
        }
        let part = self.stream_part(host, &clips, flags, None, None)?;
        let nvoices = if mode == Mode::Mono { 1 } else { 2 };
        let serial = self.streams.serial[rec].wrapping_add(1);
        let handle = 0x8400_0000 | (rec as u32) << 16 | serial as u32;
        let step = part.rate as f64 / RATE;
        let mut voices = Vec::new();
        for c in 0..nvoices {
            let Some(vi) = self.alloc(group, 0x7f, true) else {
                for vi in voices {
                    self.voices[vi] = None;
                }
                return Ok(0);
            };
            let mut v = Voice::stream(part.pcm[c.min(part.pcm.len() - 1)].clone(), step, handle, part.reverb, self.ticks);
            // Not heard until the first data is in (the next tick) and not while the stream is held.
            v.paused = true;
            self.voices[vi] = Some(v);
            voices.push(vi);
        }
        self.streams.serial[rec] = serial;
        self.streams.list.push(Stream {
            handle,
            group,
            vol,
            pan: pan.rem_euclid(360),
            paused: flags & 1 != 0,
            ready: false,
            stopped: false,
            mode,
            voices,
            readers,
            rate: part.rate,
            samples: part.samples,
            looping: part.looping,
            queue: VecDeque::new(),
            joined: 0,
            slide: None,
        });
        let si = self.streams.list.len() - 1;
        self.stream_refresh(si);
        Ok(handle)
    }

    /// The volumes of a stream's voices from its volume, pan and group (989snd FUN_00016B50, FUN_0001B140 with only the channel's
    /// volume squared: the level is the stream's volume times 0x102, at most 0x7FFE).
    fn stream_refresh(&mut self, si: usize) {
        let s = &self.streams.list[si];
        let level = (s.vol.max(0) * 0x102).min(0x7ffe);
        let gg = self.group_gain(s.group);
        let gains: Vec<[f32; 2]> = (0..s.voices.len())
            .map(|c| {
                let [l, r] = pan(level, (s.pan + chan_angle(s.mode, c)).rem_euclid(360));
                // The doubling voice has the other's sides swapped.
                let [l, r] = if s.mode == Mode::Doubled && c == 1 { [r, l] } else { [l, r] };
                [l as f32 / 16384.0 * gg, r as f32 / 16384.0 * gg]
            })
            .collect();
        let vs = s.voices.clone();
        for (c, vi) in vs.into_iter().enumerate() {
            if let Some(v) = self.voices[vi].as_mut() {
                v.gain = gains[c];
            }
        }
    }

    /// Hands the voices the first queued part, to go on with as their clips end.
    fn stream_chain(&mut self, si: usize) {
        let s = &self.streams.list[si];
        let Some(p) = s.queue.front() else { return };
        let step = p.rate as f64 / RATE;
        for (c, &vi) in s.voices.iter().enumerate() {
            if let Some(v) = self.voices[vi].as_mut() {
                v.chain = Some((p.pcm[c.min(p.pcm.len() - 1)].clone(), step));
            }
        }
    }

    /// 0x2D and 0x2E (989snd FUN_000117E8, FUN_00011A3C): a stream held or let go. A stream held before its first data is in
    /// stays held when it arrives.
    pub(super) fn stream_pause(&mut self, handle: u32, on: bool) {
        let Some(si) = self.stream_find(handle) else { return };
        let s = &mut self.streams.list[si];
        if s.paused == on {
            return;
        }
        s.paused = on;
        if s.ready {
            for &vi in &s.voices.clone() {
                if let Some(v) = self.voices[vi].as_mut() {
                    v.paused = on;
                }
            }
        }
    }

    /// 0x2F and the stop of a stream by 0x15 (989snd FUN_00011D74): muted and ended; the handle is still its own until the next
    /// tick (flag 4), and the voices are free.
    pub(super) fn stream_stop(&mut self, handle: u32) {
        let Some(si) = self.stream_find(handle) else { return };
        let s = &mut self.streams.list[si];
        s.vol = 0;
        s.pan = 0;
        s.stopped = true;
        for vi in std::mem::take(&mut s.voices) {
            self.voices[vi] = None;
        }
    }

    /// Ends every stream at once (989snd FUN_000115C4 waits until each is gone).
    pub(super) fn stream_end_all(&mut self) {
        for s in std::mem::take(&mut self.streams.list) {
            for vi in s.voices {
                self.voices[vi] = None;
            }
        }
    }

    /// A stream's volume and pan (989snd FUN_0001811C): a negative volume is a 7-bit one of its size, 0x7FFFFFFF leaves it, else
    /// 0..0x400; the pan -1 is 0, -2 leaves it.
    pub(super) fn stream_set(&mut self, handle: u32, vol: i32, pan: i32) {
        let Some(si) = self.stream_find(handle) else { return };
        let s = &mut self.streams.list[si];
        if vol < 0 {
            s.vol = -vol;
        } else if vol != 0x7fff_ffff {
            s.vol = vol.wrapping_mul(0x7f) >> 10;
        }
        s.vol = s.vol.min(0x7f);
        match pan {
            -1 => s.pan = 0,
            -2 => {}
            p => s.pan = p.rem_euclid(360),
        }
        self.stream_refresh(si);
    }

    /// The volumes on each side of a stream's voices now (for the tools).
    pub fn stream_gains(&self, handle: u32) -> Vec<[f32; 2]> {
        let Some(si) = self.stream_find(handle) else { return Vec::new() };
        self.streams.list[si].voices.iter().filter_map(|&vi| self.voices[vi].as_ref()).map(|v| v.gain).collect()
    }

    /// 0x30 (989snd FUN_0001108C): the parts the stream has: the playing one, the first queued one (loaded ahead) and the records of
    /// the others, a channel's each.
    pub(super) fn stream_parts(&self, handle: u32) -> u32 {
        let Some(si) = self.stream_find(handle) else { return 0 };
        let s = &self.streams.list[si];
        let rec = if s.mode == Mode::Stereo { 2 } else { 1 };
        let queued = s.queue.len();
        (1 + queued.min(1) + queued.saturating_sub(1) * rec) as u32
    }

    /// 0x31 (989snd FUN_00012C44): the time the playing part has played, in 1/1024 s.
    pub(super) fn stream_elapsed(&self, handle: u32) -> u32 {
        let Some(si) = self.stream_find(handle) else { return 0 };
        let s = &self.streams.list[si];
        let at = s.voices.first().and_then(|&vi| self.voices[vi].as_ref()).map_or(0.0, |v| v.position());
        (at as u64 * 1024 / s.rate.max(1) as u64) as u32
    }

    /// 0x33 (989snd FUN_00012DC4): the playing part's length in 1/1024 s.
    pub(super) fn stream_total(&self, handle: u32) -> u32 {
        let Some(si) = self.stream_find(handle) else { return 0 };
        let s = &self.streams.list[si];
        (s.samples as u64 * 1024 / s.rate.max(1) as u64) as u32
    }

    /// 0x32 (989snd FUN_00012D20): the time left, -1 for a looping part.
    pub(super) fn stream_remaining(&self, handle: u32) -> u32 {
        let Some(si) = self.stream_find(handle) else { return 0 };
        if self.streams.list[si].looping {
            return u32::MAX;
        }
        self.stream_total(handle).wrapping_sub(self.stream_elapsed(handle))
    }

    /// 0x4F (989snd FUN_00012E84): 1 once the stream has started, 0 before, -1 for a handle that is gone.
    pub(super) fn stream_started(&self, handle: u32) -> u32 {
        match self.stream_find(handle) {
            Some(si) => self.streams.list[si].ready as u32,
            None => u32::MAX,
        }
    }

    /// 0x60 (989snd FUN_0001BD48): ducking record `n`; a first word of -1 clears it. The two rates are made into steps: the
    /// distance to the floor over (rate * 30 >> 16) runs of the update.
    pub(super) fn stream_duck(&mut self, n: u32, w: &[u32]) {
        let Some(slot) = self.duck.get_mut(n as usize) else { return };
        let g = |i: usize| w.get(i).copied().unwrap_or(0);
        if g(0) == 0xffff_ffff {
            *slot = None;
            return;
        }
        let floor = g(2) as i32;
        let dist = 0x10000 - floor;
        let step = |rate: u32| (dist / (((rate as i32).wrapping_mul(0x1e)) >> 16).max(1)).max(1);
        *slot = Some(Duck { trigger: g(0), mask: g(1), floor, fall: step(g(3)), rise: step(g(4)), cur: 0x10000 });
    }

    /// Every fourth tick (989snd FUN_0001BAD8): each ducking record moves toward its floor while a sound or stream of its trigger
    /// group plays (and is not paused), else back to full; the groups' scales are the lowest of those that name them.
    pub(super) fn duck_tick(&mut self) {
        let mut active = 0u32;
        for i in &self.insts {
            if !i.paused && !i.voices.is_empty() {
                active |= 1 << (self.banks[i.bank].bank.sounds[i.sound].group as u32 & 31);
            }
        }
        for s in &self.streams.list {
            if !s.paused && !s.voices.is_empty() {
                active |= 1 << s.group;
            }
        }
        let mut scale = [0x10000i32; 15];
        for d in self.duck.iter_mut().flatten() {
            let mask = if active >> (d.trigger & 31) & 1 == 0 {
                if d.cur != 0x10000 {
                    d.cur = (d.cur + d.rise).min(0x10000);
                    d.mask
                } else {
                    0
                }
            } else {
                if d.cur != d.floor {
                    d.cur = (d.cur - d.fall).max(d.floor);
                }
                d.mask
            };
            for (g, s) in scale.iter_mut().enumerate() {
                if mask >> g & 1 != 0 && d.cur < *s {
                    *s = d.cur;
                }
            }
        }
        let changed: Vec<usize> = (0..15).filter(|&g| self.duck_scale[g] != scale[g]).collect();
        self.duck_scale = scale;
        for g in changed {
            self.refresh_group(g);
        }
    }

    /// The handles of the streams whose voice group is in `mask` (a bit a group).
    pub(super) fn streams_in_groups(&self, mask: u32) -> Vec<u32> {
        self.streams.list.iter().filter(|s| mask >> s.group & 1 != 0).map(|s| s.handle).collect()
    }

    /// Redoes the volumes of every sound and stream of voice group `g`.
    pub(super) fn refresh_group(&mut self, g: usize) {
        for k in 0..self.insts.len() {
            if self.banks[self.insts[k].bank].bank.sounds[self.insts[k].sound].group as usize & 15 == g {
                self.refresh(k);
            }
        }
        for si in 0..self.streams.list.len() {
            if self.streams.list[si].group == g {
                self.stream_refresh(si);
            }
        }
    }

    /// One tick of the streams: a stream whose first data is in starts (unless it is held), a part that joined becomes the playing
    /// one (its own volume and pan apply, the next queued part is handed to the voices), and a stream that has ended or been
    /// stopped is dropped.
    pub(super) fn stream_tick(&mut self) {
        let mut si = 0;
        while si < self.streams.list.len() {
            if self.streams.list[si].stopped {
                self.streams.list.remove(si);
                continue;
            }
            // A stream whose voices are gone has played its last part.
            let gone = self.streams.list[si].voices.iter().any(|&vi| self.voices[vi].is_none());
            if gone {
                let s = self.streams.list.remove(si);
                for vi in s.voices {
                    self.voices[vi] = None;
                }
                continue;
            }
            if !self.streams.list[si].ready {
                let s = &mut self.streams.list[si];
                s.ready = true;
                if !s.paused {
                    for &vi in &s.voices.clone() {
                        if let Some(v) = self.voices[vi].as_mut() {
                            v.paused = false;
                        }
                    }
                }
            }
            if !self.streams.list[si].paused {
                self.slide_run(self.streams.list[si].handle);
            }
            let first = self.streams.list[si].voices.first().copied();
            let advanced = first.and_then(|vi| self.voices[vi].as_ref()).map_or(0, |v| v.advanced);
            while self.streams.list[si].joined < advanced {
                let s = &mut self.streams.list[si];
                s.joined += 1;
                let Some(p) = s.queue.pop_front() else { break };
                s.rate = p.rate;
                s.samples = p.samples;
                s.looping = p.looping;
                if let Some(v) = p.vol {
                    s.vol = v;
                }
                if let Some(a) = p.pan {
                    s.pan = a;
                }
                for &vi in &s.voices.clone() {
                    if let Some(v) = self.voices[vi].as_mut() {
                        v.reverb = p.reverb;
                    }
                }
                self.stream_refresh(si);
                self.stream_chain(si);
            }
            si += 1;
        }
    }
}
