//! The game's movie container: an MPEG-2 program stream (ISO 13818-1) as the PS2's libmpeg demultiplexes it (`uyafmt::movies` has
//! the layout and the functions that read it). A stream is a run of packs (start code 0x000001BA, an MPEG-2 pack header of 14 bytes
//! and its stuffing), a system header (0xBB) and PES packets: video on stream 0xE0, audio on private stream 1 (0xBD) whose payload
//! starts with a four-byte header `FF A1 00 <track>` (five tracks, one per language, interleaved packet by packet), and padding on
//! 0xBE. The stream ends with the end code 0x000001B9.

use std::io::{self, Read};

/// One packet of a movie that matters: a piece of the video elementary stream or of an audio track.
pub enum Packet {
    /// `pts` is the time stamp of the first picture that starts in `data`, if the packet has one (90 kHz).
    Video { pts: Option<u64>, data: Vec<u8> },
    /// `data` is the track's bytes without the four-byte header. The first packet of a track begins with its SShd and SSbd headers.
    Audio { track: u8, pts: Option<u64>, data: Vec<u8> },
}

/// The audio track of a front-end language (0 English, 1 French, 2 German, 3 Spanish, 4 Italian). Track 1 is unused on the disc.
pub fn track_for_language(lang: u8) -> u8 {
    [0u8, 2, 3, 4, 5][lang.min(4) as usize]
}

/// The program stream's packets, read as they come from `R`.
pub struct Demuxer<R: Read> {
    r: R,
}

/// A 33-bit time stamp from its five bytes (the marker bits dropped).
fn stamp(b: &[u8]) -> u64 {
    (((b[0] as u64 >> 1) & 7) << 30) | ((b[1] as u64) << 22) | (((b[2] as u64) >> 1) << 15) | ((b[3] as u64) << 7) | (b[4] as u64 >> 1)
}

impl<R: Read> Demuxer<R> {
    pub fn new(r: R) -> Demuxer<R> {
        Demuxer { r }
    }

    /// Fills `buf`; false at the end of the data.
    fn fill(&mut self, buf: &mut [u8]) -> io::Result<bool> {
        match self.r.read_exact(buf) {
            Ok(()) => Ok(true),
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => Ok(false),
            Err(e) => Err(e),
        }
    }

    /// The next video or audio packet, None at the end code or the end of the data.
    pub fn next_packet(&mut self) -> Result<Option<Packet>, String> {
        let io = |e: io::Error| e.to_string();
        loop {
            let mut h = [0u8; 4];
            if !self.fill(&mut h).map_err(io)? {
                return Ok(None);
            }
            if h[..3] != [0, 0, 1] {
                return Err(format!("program stream: {h:02x?} where a start code should be"));
            }
            match h[3] {
                // A pack header: 10 more bytes, the last one's low three bits the stuffing length.
                0xba => {
                    let mut p = [0u8; 10];
                    if !self.fill(&mut p).map_err(io)? {
                        return Ok(None);
                    }
                    if p[0] >> 6 != 1 {
                        return Err("program stream: a pack header that is not MPEG-2's".into());
                    }
                    let mut stuffing = [0u8; 7];
                    let n = (p[9] & 7) as usize;
                    if !self.fill(&mut stuffing[..n]).map_err(io)? {
                        return Ok(None);
                    }
                }
                0xb9 => return Ok(None),
                id => {
                    let mut l = [0u8; 2];
                    if !self.fill(&mut l).map_err(io)? {
                        return Ok(None);
                    }
                    let mut body = vec![0u8; u16::from_be_bytes(l) as usize];
                    if !self.fill(&mut body).map_err(io)? {
                        return Ok(None);
                    }
                    // The system header and the padding are not for the player.
                    if id != 0xe0 && id != 0xbd {
                        continue;
                    }
                    // A PES packet: two flag bytes, the header's length, then the header (time stamps) and the payload.
                    if body.len() < 3 || body[0] >> 6 != 2 {
                        return Err(format!("PES packet {id:#x}: no MPEG-2 header"));
                    }
                    let (flags, hl) = (body[1], body[2] as usize);
                    if body.len() < 3 + hl {
                        return Err(format!("PES packet {id:#x}: its header runs past it"));
                    }
                    let pts = (flags & 0x80 != 0 && hl >= 5).then(|| stamp(&body[3..8]));
                    let payload = &body[3 + hl..];
                    if id == 0xe0 {
                        return Ok(Some(Packet::Video { pts, data: payload.to_vec() }));
                    }
                    if payload.len() < 4 || payload[0] != 0xff {
                        return Err("private stream 1: a payload without the FF A1 00 <track> header".into());
                    }
                    return Ok(Some(Packet::Audio { track: payload[3], pts, data: payload[4..].to_vec() }));
                }
            }
        }
    }
}
