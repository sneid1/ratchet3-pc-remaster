//! A movie's sound: one language's track of the program stream's private stream 1, PS2 ADPCM (the SPU2's format, 16-byte frames of
//! 28 samples). The track starts with two headers (libmpeg-side code func_L01_004CF578 and func_L01_004CF648 copy them): `SShd`
//! (size 0x18: the codec, the sample rate, the channel count, the interleave in bytes, then eight 0xFF) and `SSbd` (the data size);
//! the frames follow, the channels' in turn in chunks of `interleave` bytes (left, right, left, ...). The decoder keeps each
//! channel's two last samples between packets.

use super::pss::track_for_language;

/// The SPU2's two-tap filters, in 1/64 (filter number from the frame's first byte's high nibble).
const FILTERS: [(i32, i32); 5] = [(0, 0), (60, 0), (115, -52), (98, -55), (122, -60)];

/// One language's sound of a movie, decoded to 48 kHz stereo frames as its packets come.
pub struct Track {
    id: u8,
    dec: Option<Dec>,
    /// The first packet's time stamp (90 kHz): where the sound starts against the pictures.
    pub pts0: Option<u64>,
}

struct Dec {
    interleave: usize,
    hist: [(i32, i32); 2],
    /// Per channel: a frame flagged as the last has been decoded; the rest of the channel is silence (the track ends with that
    /// frame and a terminator frame the SPU2 never plays).
    ended: [bool; 2],
    pending: Vec<u8>,
}

/// One frame's 28 samples appended to `out`; `h` the channel's two last samples.
fn frame(f: &[u8], h: &mut (i32, i32), out: &mut Vec<i16>) {
    let (shift, filter) = ((f[0] & 15) as u32, ((f[0] >> 4) as usize).min(4));
    // A shift over 12 is what the chip makes of a frame with a shift of 9.
    let shift = if shift > 12 { 9 } else { shift };
    let (c1, c2) = FILTERS[filter];
    for i in 0..28 {
        let b = f[2 + i / 2];
        let nib = if i & 1 == 0 { b & 15 } else { b >> 4 } as i32;
        let s = ((((nib << 12) as i16) as i32) >> shift) + ((h.0 * c1 + h.1 * c2 + 32) >> 6);
        let s = s.clamp(-32768, 32767);
        h.1 = h.0;
        h.0 = s;
        out.push(s as i16);
    }
}

impl Track {
    /// The track of front-end language `lang` (the movie player's third argument).
    pub fn new(lang: u8) -> Track {
        Track { id: track_for_language(lang), dec: None, pts0: None }
    }

    pub fn id(&self) -> u8 {
        self.id
    }

    /// An audio packet of the movie: its frames are appended to `out` (packets of the other tracks are left alone).
    pub fn push(&mut self, track: u8, pts: Option<u64>, data: &[u8], out: &mut Vec<[i16; 2]>) -> Result<(), String> {
        if track != self.id {
            return Ok(());
        }
        let mut body = data;
        if self.dec.is_none() {
            if body.len() < 40 || &body[0..4] != b"SShd" || &body[32..36] != b"SSbd" {
                return Err("audio track without its SShd and SSbd headers".into());
            }
            let w = |o: usize| u32::from_le_bytes([body[o], body[o + 1], body[o + 2], body[o + 3]]);
            let (rate, channels, interleave) = (w(12), w(16), w(20) as usize);
            if channels != 2 || rate != 48000 || interleave == 0 || interleave % 16 != 0 {
                return Err(format!("audio track {}: {channels} channels at {rate} Hz, interleave {interleave}", self.id));
            }
            body = &body[40..];
            self.dec = Some(Dec { interleave, hist: [(0, 0); 2], ended: [false; 2], pending: Vec::new() });
            self.pts0 = Some(pts.ok_or("the first audio packet has no time stamp")?);
        }
        let d = self.dec.as_mut().expect("made above");
        d.pending.extend_from_slice(body);
        let block = 2 * d.interleave;
        let mut at = 0;
        while d.pending.len() - at >= block {
            let mut per: [Vec<i16>; 2] = [Vec::new(), Vec::new()];
            for c in 0..2 {
                let chunk = &d.pending[at + c * d.interleave..at + (c + 1) * d.interleave];
                for f in chunk.chunks_exact(16) {
                    if d.ended[c] {
                        per[c].extend(std::iter::repeat_n(0i16, 28));
                        continue;
                    }
                    frame(f, &mut d.hist[c], &mut per[c]);
                    if f[1] & 1 != 0 {
                        d.ended[c] = true;
                    }
                }
            }
            out.extend(per[0].iter().zip(&per[1]).map(|(&l, &r)| [l, r]));
            at += block;
        }
        d.pending.drain(..at);
        Ok(())
    }
}
