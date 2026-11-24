//! The game's LZ streams ("WAD" header), unpacked as the game's unpacker does it.
//!
//! Written from func_L01_00433EA0 (NTSC-U level 1's copy; each program carries one). The unpacker DMAs the stream body in
//! 0x2000-byte chunks to the scratchpad, two at a time, and reads tokens from the chunk in front of it:
//!
//! - header: "WAD" (checked as u16 ^ 0x4157 against byte 2 ^ 0x44), the stream's size including the header as a u32 at +3; the
//!   body starts at +0x10 (bytes +7..+0x10 are not read).
//! - first byte > 0x11: a literal of (byte - 0x11) bytes, then state B; otherwise state A at that byte.
//! - state A, flag < 0x10: a literal of flag + 3 bytes (flag 0: next byte + 18), then state B.
//! - flag >= 0x40: a match of (flag >> 5) + 1 bytes from 1 + ((flag >> 2) & 7) + next * 8 back.
//! - 0x20..0x40: n = flag & 0x1f (0: next + 0x1f); two bytes b0 b1; a match of n + 2 bytes from 1 + (b0 >> 2) + (b1 << 6) back.
//! - 0x10..0x20: n = flag & 7 (0: next + 7); b0 b1; d = ((flag & 8) << 11) + (b0 >> 2) + (b1 << 6). d != 0: a match of n + 2
//!   bytes from 0x4000 + d back. d == 0 and n == 1: no copy. d == 0 and n != 1: the rest of this chunk is skipped and tokens go
//!   on at the next chunk's start in state A.
//! - after a match, the low two bits of the byte two back (the flag for >= 0x40, else b0) give 0-3 literal bytes: none goes to
//!   state A, any to state B.
//! - state B, flag < 0x10: the unpacker traps (teq).
//! - the stream ends when a token would start at the header's size (checked only in the last chunk).

use crate::disc::u32_at;

pub struct Stream {
    /// The unpacked bytes.
    pub data: Vec<u8>,
    /// The header's size: the stream's bytes, header included.
    pub size: usize,
    /// The chunk tails skipped after a switch token: (start, end) in the stream.
    pub skipped: Vec<(usize, usize)>,
}

const CHUNK: usize = 0x2000;
const BODY: usize = 0x10;

/// Unpacks the stream at the start of `src` (which may run on past the stream's end).
pub fn unpack(src: &[u8]) -> Result<Stream, String> {
    if src.len() < BODY || &src[0..3] != b"WAD" {
        return Err(format!("no WAD header: {:02x?}", &src[..src.len().min(4)]));
    }
    let size = u32_at(src, 3)? as usize;
    if size < BODY || size > src.len() {
        return Err(format!("stream size {size:#x} outside 0x10..{:#x}", src.len()));
    }
    let mut out: Vec<u8> = Vec::with_capacity(size * 3);
    let mut skipped = Vec::new();
    let mut chunk = BODY;
    let mut remaining = size - BODY;
    let mut end = if remaining <= 0x1fff { Some(chunk + remaining) } else { None };
    let mut p = BODY;

    // One byte of the current chunk, before the stream's end.
    let byte = |p: usize, chunk: usize, end: Option<usize>| -> Result<u8, String> {
        if p >= chunk + CHUNK || end.is_some_and(|e| p >= e) || p >= size {
            return Err(format!("token byte at {p:#x} outside its chunk {chunk:#x}..{:#x} or the stream's end {size:#x}", chunk + CHUNK));
        }
        Ok(src[p])
    };
    let literal = |out: &mut Vec<u8>, p: &mut usize, n: usize, chunk: usize, end: Option<usize>| -> Result<(), String> {
        for _ in 0..n {
            out.push(byte(*p, chunk, end)?);
            *p += 1;
        }
        Ok(())
    };
    let copy = |out: &mut Vec<u8>, dist: usize, n: usize, at: usize| -> Result<(), String> {
        if dist > out.len() {
            return Err(format!("match at {at:#x} reaches {dist:#x} back with {:#x} bytes out", out.len()));
        }
        let from = out.len() - dist;
        for k in 0..n {
            let b = out[from + k];
            out.push(b);
        }
        Ok(())
    };

    // false: state A (a literal may come next); true: state B (a match must).
    let mut after_literal;
    let first = byte(p, chunk, end)?;
    if first > 0x11 {
        p += 1;
        literal(&mut out, &mut p, (first - 0x11) as usize, chunk, end)?;
        after_literal = true;
    } else {
        after_literal = false;
    }
    loop {
        if end.is_some_and(|e| p >= e) {
            break;
        }
        let at = p;
        let flag = byte(p, chunk, end)?;
        p += 1;
        if flag < 0x10 {
            if after_literal {
                return Err(format!("literal token {flag:#x} at {at:#x} after a literal (the unpacker traps)"));
            }
            let n = if flag == 0 {
                let x = byte(p, chunk, end)?;
                p += 1;
                x as usize + 18
            } else {
                flag as usize + 3
            };
            literal(&mut out, &mut p, n, chunk, end)?;
            after_literal = true;
            continue;
        }
        let trail_from;
        if flag >= 0x40 {
            let b = byte(p, chunk, end)? as usize;
            p += 1;
            copy(&mut out, 1 + ((flag as usize >> 2) & 7) + b * 8, (flag as usize >> 5) + 1, at)?;
            trail_from = flag;
        } else if flag >= 0x20 {
            let mut n = (flag & 0x1f) as usize;
            if n == 0 {
                n = byte(p, chunk, end)? as usize + 0x1f;
                p += 1;
            }
            let (b0, b1) = (byte(p, chunk, end)?, byte(p + 1, chunk, end)? as usize);
            p += 2;
            copy(&mut out, 1 + (b0 as usize >> 2) + (b1 << 6), n + 2, at)?;
            trail_from = b0;
        } else {
            let mut n = (flag & 7) as usize;
            if n == 0 {
                n = byte(p, chunk, end)? as usize + 7;
                p += 1;
            }
            let (b0, b1) = (byte(p, chunk, end)?, byte(p + 1, chunk, end)? as usize);
            p += 2;
            let d = (((flag & 8) as usize) << 11) + (b0 as usize >> 2) + (b1 << 6);
            if d == 0 && n != 1 {
                // Switch: the rest of this chunk is skipped.
                skipped.push((p, (chunk + CHUNK).min(size)));
                chunk += CHUNK;
                if remaining < CHUNK {
                    return Err(format!("chunk switch at {at:#x} past the stream's last chunk"));
                }
                remaining -= CHUNK;
                end = if remaining <= 0x1fff { Some(chunk + remaining) } else { None };
                p = chunk;
                after_literal = false;
                continue;
            }
            if d != 0 {
                copy(&mut out, 0x4000 + d, n + 2, at)?;
            }
            trail_from = b0;
        }
        let t = (trail_from & 3) as usize;
        if t == 0 {
            after_literal = false;
        } else {
            literal(&mut out, &mut p, t, chunk, end)?;
            after_literal = true;
        }
    }
    Ok(Stream { data: out, size, skipped })
}
