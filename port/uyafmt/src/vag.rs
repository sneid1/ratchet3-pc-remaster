//! VAG clips: Sony's VAGp header (big-endian: version, data size, sample rate; a 16-byte name) and SPU2 ADPCM frames (16
//! bytes: shift (low nibble, 0-12) and filter (high nibble, 0-4), flags (0-7: end, repeat, start), 28 four-bit samples). The
//! game streams them from the disc to the IOP, whose player reads the header; the SPU2 plays the frames.

use crate::track::{Node, Space, Tracker};

/// One clip of `b.len()` bytes at `at` in `parent`: header line and frames line under `group`. `game` names the EE function
/// that sends the clip to the IOP.
pub fn clip(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], game: &'static str) {
    let be = |o: usize| b.get(o..o + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]]));
    let lh = t.line(group, "VAGp header (tag, version 0x20, data size, sample rate, name)", "vag::clip", game, Space::Disc);
    let mut h = t.child(parent, lh, at, 0x30.min(b.len() as u64));
    let size = be(0xc).unwrap_or(0) as usize;
    let head = (|| -> Result<(), String> {
        if b.len() < 0x30 || &b[0..4] != b"VAGp" {
            return Err(format!("no VAGp header: {:02x?}", &b[..b.len().min(8)]));
        }
        if be(4) != Some(0x20) || be(8) != Some(0) || b[0x14..0x20].iter().any(|&v| v != 0) {
            return Err(format!("VAGp header {:02x?}", &b[..0x20]));
        }
        let rate = be(0x10).unwrap_or(0);
        if !matches!(rate, 11025 | 22050 | 24000 | 32000 | 44100 | 48000) {
            return Err(format!("sample rate {rate}"));
        }
        if 0x30 + size != b.len() || size % 16 != 0 {
            return Err(format!("data size {size:#x} for a clip of {:#x} bytes", b.len()));
        }
        // The name: printable bytes, then NUL to the end of its 16 bytes.
        let name = &b[0x20..0x30];
        let z = name.iter().position(|&c| c == 0).unwrap_or(16);
        if name[..z].iter().any(|&c| !(0x20..0x7f).contains(&c)) || name[z..].iter().any(|&c| c != 0) {
            return Err(format!("name {name:02x?}"));
        }
        Ok(())
    })();
    match head {
        Ok(()) => h.mark(0, 0x30),
        Err(e) => h.fail(e),
    }
    t.close(h);
    if b.len() <= 0x30 || h_failed(b, size) {
        return;
    }
    let lf = t.line(group, "SPU2 ADPCM frames (16 bytes: shift and filter, flags, 28 samples)", "vag::clip", "the SPU2 plays them (the IOP's stream player feeds them)", Space::Disc);
    let mut f = t.child(parent, lf, at + 0x30, size as u64);
    let data = &b[0x30..];
    match (0..size).step_by(16).find(|&k| data[k] >> 4 > 4 || data[k] & 15 > 12 || data[k + 1] > 7) {
        None => f.mark(0, size as u64),
        Some(k) => f.fail(format!("frame at +{:#x}: {:02x?}", 0x30 + k, &data[k..k + 2])),
    }
    t.close(f);
}

fn h_failed(b: &[u8], size: usize) -> bool {
    &b[0..4] != b"VAGp" || 0x30 + size != b.len() || size % 16 != 0
}
