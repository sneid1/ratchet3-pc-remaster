//! Subtitle tables: a WAD1 movie's info block and a SCENE record's header block, as func_L01_00402D50 draws them (both are
//! loaded into the pointer at 0x1E6E4C: by func_L01_004AC588 / func_L01_004B4D48 for a movie, by func_L01_00429618 for a
//! scene).
//!
//! Entries of nine s16: start frame, end frame, five text offsets (the column the language picks: language byte 0x143A07 5 ->
//! 3, 4 -> 4, 2..3 -> byte - 1, else 0) and two more columns no code reads (0xCCCC, or text offsets in a few entries). The
//! entries run to the first entry's first text offset; the texts are NUL-terminated strings after them. Bytes after the last
//! string are zero.

use crate::rd::Fields;
use crate::track::{NOTHING, Node, Space, Tracker};

pub fn table(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8]) {
    let l = t.line(
        group,
        "subtitle table (entries and texts)",
        "subs::table",
        "func_L01_00402D50 (draws it; entries' columns 0-4 and the texts they point at); columns 5-6 and the zero tail are read by nothing",
        Space::Disc,
    );
    let mut n = t.child(parent, l, at, b.len() as u64);
    if let Err(e) = decode(&mut n, b) {
        n.fail(e);
    }
    t.close(n);
    let _ = NOTHING;
}

fn decode(n: &mut Node, b: &[u8]) -> Result<(), String> {
    let s16 = |o: usize| -> Result<i16, String> { b.get(o..o + 2).map(|x| i16::from_le_bytes([x[0], x[1]])).ok_or_else(|| format!("+{o:#x} past the table's end {:#x}", b.len())) };
    let end = s16(4)?;
    if end <= 0 || end as usize % 18 != 0 || end as usize > b.len() {
        return Err(format!("first text offset {end:#x}: not a whole number of 18-byte entries inside the block"));
    }
    let end = end as usize;
    let mut f = Fields::new(b, n);
    let mut strings = std::collections::BTreeSet::new();
    for e in (0..end).step_by(18) {
        // Start and end frame: shown while start <= frame <= end (an entry with start after end is never shown).
        f.mark(e, 4);
        for k in 0..7 {
            let o = s16(e + 4 + 2 * k)?;
            if k >= 5 && o as u16 == 0xcccc {
                f.mark(e + 4 + 2 * k, 2);
                continue;
            }
            if (o as usize) < end || o as usize >= b.len() {
                return Err(format!("entry +{e:#x} column {k}: text offset {o:#x} outside {end:#x}..{:#x}", b.len()));
            }
            strings.insert(o as usize);
            f.mark(e + 4 + 2 * k, 2);
        }
    }
    let mut last = end;
    for &o in &strings {
        let z = b[o..].iter().position(|&c| c == 0).ok_or_else(|| format!("text at {o:#x} has no NUL"))?;
        f.mark(o, z + 1);
        last = last.max(o + z + 1);
    }
    // Between the texts: nothing but a stray NUL; after the last text: zero to the block's end.
    for i in end..b.len() {
        if b[i] == 0 && !strings.iter().any(|&o| o <= i && i < o + b[o..].iter().position(|&c| c == 0).unwrap_or(0) + 1) {
            f.mark(i, 1);
        }
    }
    let _ = last;
    Ok(())
}
