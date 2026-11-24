//! Animation frames of moby sequences (SCENE chunks and SCENE blocks), as R&C3's own code reads them.
//!
//! Every entry point takes the `Space` its lines are in: the frames and base blocks of a class that sits on the disc (the front end's
//! model blocks) are decoded in `Space::Disc` with the same rules.
//!
//! A frame in these files is the unpacked form that func_L01_004397F8 (the frame unpacker, rac3-decomp's `anim_frame_unpack`)
//! writes and the pose builders read; a sequence whose header byte +0x13 is 0xFF keeps its frames in this form (func_L01_00436D30
//! and func_L01_004389D8 take the frame pointer straight from the sequence's offset table in that case; every sequence in the
//! SCENE chunks and blocks has +0x13 = 0xFF). A frame is:
//! - a 16-byte header: +0 f32, +4 s16, +6 u16 quadword count, +8 u16, +0xA u16 (n1), +0xC u16, +0xE u16 (n2);
//! - N joint rotations of 8 bytes (x, y, z, w as s16): N is +8 / 8 (func_L01_004397F8 writes +8 as 8 * N and +0xC as
//!   8 * N + 8 * n1; the pose builders get N from the object's joint table, not from the frame);
//! - n1 entries of 8 bytes (list 1) and n2 entries of 8 bytes (list 2);
//! - a zero doubleword when N + n1 + n2 is odd, so that the data is a whole number of quadwords: +6 is (N + n1 + n2 + 1) / 2, the
//!   quadword count func_L01_0043B7D0 hands to the DMA that copies the data after the header into the scratchpad.
//!
//! func_L01_0043B7D0 (rac3-decomp's `moby_pose_build`; func_L01_0043AAF8, func_L01_0043C028, func_L01_0043CF90 and
//! func_L01_0043D9D8 read the same fields) reads, with VU0 macro instructions:
//! - rotation i (found through the object's joint table, 8 bytes at scratchpad +0x3000 + 8 * i): `ld`, `pextlh`, `psraw 16`,
//!   `vitof15.xyzw`: four s16 divided by 32768;
//! - a list 1 entry: `pextlh`, `psrlw 13`, `vitof15.xyz`: x, y, z are u16 divided by 4096; `lbu` of byte +6 is the joint number
//!   (looked up in the object's joint table); `dsrl32 30` takes the top two bits of the +6 halfword, minus one, which go to the
//!   joint's matrix +0x1C; bits 8-13 of that halfword are read by nothing;
//! - a list 2 entry: `pextlh`, `psraw 16`, `vitof0.xyz`: x, y, z are s16 as they are; `lbu` of byte +6 is the joint number; the
//!   high byte of that halfword is read by nothing.
//!
//! func_L01_00436D30 (the animation layer update) reads +0 with `lwc1` and keeps it as the layer's rate; func_L01_004389D8 reads
//! it the same way when the sequence's +0x18 is 0. func_L01_00436500 and func_L01_004365E0 read +4 with `lh` and scale it by
//! 0.0625 (1 / 16), blending it between two frames.
//!
//! What no code reads: +8 and +0xC (the animation code in 0x436000-0x43E000 does not load them; the unpacker writes them), the
//! top bits 8-13 of a list 1 entry's +6 halfword, the high byte of a list 2 entry's +6 halfword and the zero doubleword the DMA
//! copies but nothing uses. The decoder requires each to be what the unpacker would have written (zero, or 8 * N and
//! 8 * N + 8 * n1) and fails the frame otherwise.
//!
//! The packed form (a sequence whose +0x13 is not 0xFF: the slot sequences of the core index's word 0x1E table, that is moby class 0's
//! animation sequences, and a few sequences of class 0 itself; +0x13 is 0 to 7 there and equals +0x12): func_L01_004397F8
//! (rac3-decomp's `anim_frame_unpack`; its callers func_L01_00436D30, func_L01_004DBFE8, func_L01_004A9F80 and func_L01_004BBDD8
//! pass the frame's offset word with the top four bits cut off, the output buffer, the class's joint count N (class +8) and the base
//! block pointer sequence + 0x1C + 4 * frame count + 4 * (+0x13)) unpacks a frame against its sequence's base block:
//! - the base block (packed_base): 4 * (+0x13) bytes of records (two u16 each, nothing reads them), then a u32 that holds the offsets
//!   of three streams of every frame (bits 0-9 the s16 stream, 10-20 the byte stream, 21-31 the sign stream, each counted from the
//!   frame's +4), N * 6 bytes (x, y, z of each joint's rotation as s16, whose low two bits are the component's code: 0 the value as
//!   it is, 1 a signed byte delta, 2 a signed 10-bit delta, 3 an s16 that replaces it), the list 1 and list 2 entry counts (two
//!   bytes) and 8 bytes per list entry (x, y, z as s16, then the halfword the unpacked entry's +6 receives);
//! - a frame (packed_frame): +0 u16 (the rate is 1 / value, 0 for a rate of 0), +2 s16, then the 10-bit stream (a u32 per three
//!   deltas, for the components coded 2), the s16 stream (components coded 3), the byte stream (components coded 1), the sign stream
//!   (one bit per joint, least significant first, for the joint's w = sqrt(1 - x^2 - y^2 - z^2) in the unpacked frame), then per list
//!   entry a control byte (2 bits each for x, y, z: 0 the base value, 1 a signed byte delta read as 4 * byte, 2 an s16 that replaces
//!   it; bits 6-7 zero) followed by the bytes the codes name. The stream sizes follow from the base block's codes, so every frame of
//!   a sequence has the same stream offsets and only the list streams vary in length. The frame ends with its last list byte; frames
//!   start on 4-byte boundaries and zero bytes pad to the next frame (0 to 3) or the next structure (0 to 15).
//! - the unpacked frame is then the 16-byte header (+0 rate, +4 the s16, +6 quadword count, +8 8 * N, +0xA n1, +0xC 8 * N + 8 * n1,
//!   +0xE n2), the N rotations (x, y, z decoded, w from the sign stream), and the list entries; the unpacked frame above is what
//!   func_L01_0043B7D0 reads.

use crate::track::{NOTHING, Node, Space, Tracker};

const DEC: &str = "anim::frame";
const RATE_FN: &str = "func_L01_00436D30 and func_L01_004389D8";
const VALUE_FN: &str = "func_L01_00436500 and func_L01_004365E0";
const COUNT_FN: &str = "func_L01_0043B7D0 (func_L01_0043AAF8, func_L01_0043C028, func_L01_0043CF90 and func_L01_0043D9D8 read the same fields)";
const OFFSETS_FN: &str = "func_L01_004397F8 writes them (8 * N and 8 * N + 8 * list 1 count); no reader found";
const ROT_FN: &str = "func_L01_0043B7D0 (rotation loads at scratchpad +0x3000; func_L01_0043AAF8, func_L01_0043C028, func_L01_0043CF90 and func_L01_0043D9D8 read the same)";
const LIST_FN: &str = "func_L01_0043B7D0 (func_L01_0043AAF8, func_L01_0043C028, func_L01_0043CF90 and func_L01_0043D9D8 read the same)";

fn u16_at(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}

/// Fails a frame: a node over `ranges` of the frame (offsets from the frame's start) takes the error, and the error is returned.
fn bad(t: &mut Tracker, parent: &mut Node, line: usize, at: u64, ranges: &[(u64, u64)], e: String) -> Result<(), String> {
    let r: Vec<(u64, u64)> = ranges.iter().map(|&(a, n)| (at + a, n)).collect();
    let mut n = t.pieces(parent, line, &r);
    let e = format!("{} +{:#x}: {e}", parent.place, at + ranges[0].0);
    n.fail(e.clone());
    t.close(n);
    Err(e)
}

/// Fails the open node `n` with `e`, closes it and returns the error.
fn stop(t: &mut Tracker, mut n: Node, e: String) -> Result<(), String> {
    n.fail(e.clone());
    t.close(n);
    Err(e)
}

/// One frame of `b.len()` bytes at `at` in `parent`, split into lines of `group` in `space` (the frames of a class on the disc, the
/// front end's model blocks, are in `Space::Disc`; the level cores' and the SCENE blocks' are in `Space::Unpacked`).
pub fn frame(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], space: Space) -> Result<(), String> {
    let l_rate = t.line(group, "animation frame rate (+0, f32)", DEC, RATE_FN, space);
    let l_value = t.line(group, "animation frame value (+4, s16 read as value * 0.0625)", DEC, VALUE_FN, space);
    let l_count = t.line(group, "animation frame quadword count and list counts (+6, +0xA, +0xE; u16)", DEC, COUNT_FN, space);
    let l_offs = t.line(group, "animation frame list offsets (+8, +0xC; u16: 8 * N and 8 * N + 8 * list 1 count)", DEC, OFFSETS_FN, space);
    let l_rot = t.line(group, "animation frame joint rotations (8 bytes per joint: x, y, z, w as s16, read as value / 32768)", DEC, ROT_FN, space);
    let l_one = t.line(
        group,
        "animation frame list 1 entries (8 bytes: x, y, z as u16 read as value / 4096; +6 halfword: joint number in the low byte, bits 14-15 a mode, bits 8-13 zero)",
        DEC,
        LIST_FN,
        space,
    );
    let l_two = t.line(
        group,
        "animation frame list 2 entries (8 bytes: x, y, z as s16 read as they are; +6 halfword: joint number in the low byte, high byte zero)",
        DEC,
        LIST_FN,
        space,
    );
    let l_tail = t.line(group, "animation frame tail (zero doubleword that completes the last quadword when the entry count is odd)", DEC, NOTHING, space);

    if b.len() < 0x10 {
        return bad(t, parent, l_count, at, &[(0, b.len() as u64)], format!("frame of {:#x} bytes, a header needs 0x10", b.len()));
    }
    let (qwc, off1, n1, off2, n2) = (u16_at(b, 6), u16_at(b, 8), u16_at(b, 0xa), u16_at(b, 0xc), u16_at(b, 0xe));
    let cnt = [(6, 2), (0xa, 2), (0xe, 2)];
    if off1 % 8 != 0 {
        return bad(t, parent, l_offs, at, &[(8, 2), (0xc, 2)], format!("+8 is {off1:#x}, expected 8 * N for a joint count N (0 for a class without joints)"));
    }
    let joints = off1 / 8;
    if off2 != off1 + 8 * n1 {
        return bad(t, parent, l_offs, at, &[(8, 2), (0xc, 2)], format!("+0xC is {off2:#x}, expected 8 * N + 8 * n1 = {:#x}", off1 + 8 * n1));
    }
    let entries = joints + n1 + n2;
    if qwc != (entries + 1) / 2 {
        return bad(t, parent, l_count, at, &cnt, format!("+6 is {qwc:#x}, expected (N + n1 + n2 + 1) / 2 = {:#x}", (entries + 1) / 2));
    }
    if b.len() != 0x10 + 16 * qwc {
        return bad(t, parent, l_count, at, &cnt, format!("frame of {:#x} bytes, expected 0x10 + 16 * {qwc} = {:#x}", b.len(), 0x10 + 16 * qwc));
    }

    let mut n = t.child(parent, l_rate, at, 4);
    n.mark(0, 4);
    t.close(n);
    let mut n = t.child(parent, l_value, at + 4, 2);
    n.mark(0, 2);
    t.close(n);
    let mut n = t.pieces(parent, l_count, &[(at + 6, 2), (at + 0xa, 2), (at + 0xe, 2)]);
    n.mark(0, 6);
    t.close(n);
    let mut n = t.pieces(parent, l_offs, &[(at + 8, 2), (at + 0xc, 2)]);
    n.mark(0, 4);
    t.close(n);

    let mut n = t.child(parent, l_rot, at + 0x10, 8 * joints as u64);
    n.mark(0, 8 * joints as u64);
    t.close(n);

    let p1 = 0x10 + off1;
    let mut n = t.child(parent, l_one, at + p1 as u64, 8 * n1 as u64);
    for i in 0..n1 {
        let h = u16_at(b, p1 + 8 * i + 6);
        if h & 0x3f00 != 0 {
            return stop(t, n, format!("{} +{:#x}: list 1 entry {i}: +6 halfword {h:#06x}, bits 8-13 must be zero", parent.place, at + (p1 + 8 * i + 6) as u64));
        }
    }
    n.mark(0, 8 * n1 as u64);
    t.close(n);

    let p2 = 0x10 + off2;
    let mut n = t.child(parent, l_two, at + p2 as u64, 8 * n2 as u64);
    for i in 0..n2 {
        let h = u16_at(b, p2 + 8 * i + 6);
        if h & 0xff00 != 0 {
            return stop(t, n, format!("{} +{:#x}: list 2 entry {i}: +6 halfword {h:#06x}, the high byte must be zero", parent.place, at + (p2 + 8 * i + 6) as u64));
        }
    }
    n.mark(0, 8 * n2 as u64);
    t.close(n);

    let end = p2 + 8 * n2;
    if b.len() > end {
        let mut n = t.child(parent, l_tail, at + end as u64, (b.len() - end) as u64);
        if let Some(k) = b[end..].iter().position(|&v| v != 0) {
            return stop(t, n, format!("{} +{:#x}: tail byte {:#04x}, expected zero", parent.place, at + (end + k) as u64, b[end + k]));
        }
        n.mark(0, (b.len() - end) as u64);
        t.close(n);
    }
    Ok(())
}

const PDEC: &str = "anim::packed_frame";
const BDEC: &str = "anim::packed_base";
const UNPACK_FN: &str = "func_L01_004397F8 (rac3-decomp's anim_frame_unpack)";
const DURATION_FN: &str = "func_L01_004397F8 (lhu; the unpacked rate is 1 / value, 0 for 0) and func_L01_00436D30 (lh; the layer's rate, the same rule)";
const PVALUE_FN: &str = "func_L01_004397F8 copies it to +4 of the unpacked frame, which func_L01_00436500 and func_L01_004365E0 read";
const RECORDS_FN: &str = "no reader: func_L01_00436D30, func_L01_004DBFE8, func_L01_004A9F80 and func_L01_004BBDD8 only step over them (the base block is at sequence + 0x1C + 4 * frame count + 4 * the sequence's +0x13)";

/// The base block of a packed sequence, as func_L01_004397F8 reads it (`joints` is the class's byte +8).
struct Base {
    n_a: usize,
    n_b: usize,
    n_s: usize,
    n1: usize,
    n2: usize,
    joints: usize,
    len: usize,
}

impl Base {
    /// Bytes of the 10-bit stream of every frame (a u32 per three deltas).
    fn size_a(&self) -> usize {
        4 * self.n_a.div_ceil(3)
    }
}

/// Reads the block at the start of `b` (a u32 of stream offsets, 3 * joints coded components, the two list counts, the list
/// entries); the stream offsets must be the sizes the components' codes give.
fn read_base(b: &[u8], joints: usize) -> Result<Base, String> {
    let head = 6 + 6 * joints;
    if b.len() < head {
        return Err(format!("base block of {:#x} bytes, {joints} joints need {head:#x}", b.len()));
    }
    let w = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize;
    let (off_s, off_b, off_w) = (w & 0x3ff, (w >> 10) & 0x7ff, w >> 21);
    let (mut n_a, mut n_b, mut n_s) = (0, 0, 0);
    for i in 0..3 * joints {
        match b[4 + 2 * i] & 3 {
            1 => n_b += 1,
            2 => n_a += 1,
            3 => n_s += 1,
            _ => {}
        }
    }
    let (n1, n2) = (b[4 + 6 * joints] as usize, b[5 + 6 * joints] as usize);
    let len = head + 8 * (n1 + n2);
    if b.len() < len {
        return Err(format!("base block of {:#x} bytes, {n1} + {n2} list entries need {len:#x}", b.len()));
    }
    let base = Base { n_a, n_b, n_s, n1, n2, joints, len };
    let (want_s, want_b) = (base.size_a(), base.size_a() + 2 * n_s);
    let want_w = want_b + n_b;
    if (off_s, off_b, off_w) != (want_s, want_b, want_w) {
        return Err(format!(
            "stream offsets {off_s:#x}, {off_b:#x}, {off_w:#x}: the components' codes ({n_a} coded 2, {n_s} coded 3, {n_b} coded 1) give {want_s:#x}, {want_b:#x}, {want_w:#x}"
        ));
    }
    Ok(base)
}

/// The size of the base block at the start of `b` (its u32, 6 bytes per joint, 6 more, 8 per list entry), for the structure planner.
pub fn base_len(b: &[u8], joints: usize) -> Result<usize, String> {
    read_base(b, joints).map(|x| x.len)
}

/// The records and base block of a packed sequence: `b` is `4 * records + base_len` bytes (the sequence's +0x13 records, then the
/// block) at `at`; the first frame follows it directly.
pub fn packed_base(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], records: usize, joints: usize, space: Space) -> Result<(), String> {
    let l_rec = t.line(group, "packed animation sequence records (4 bytes each, as many as the sequence's +0x13: two u16)", BDEC, RECORDS_FN, space);
    let l_offs = t.line(group, "packed animation base block stream offsets (+0, u32: bits 0-9 s16 stream, 10-20 byte stream, 21-31 sign stream, from a frame's +4)", BDEC, UNPACK_FN, space);
    let l_comp = t.line(group, "packed animation base block joint components (6 bytes per joint: x, y, z as s16; the low two bits code 0 as it is, 1 byte delta, 2 10-bit delta, 3 s16 that replaces it)", BDEC, UNPACK_FN, space);
    let l_cnt = t.line(group, "packed animation base block list entry counts (two bytes: list 1, list 2)", BDEC, UNPACK_FN, space);
    let l_ent = t.line(
        group,
        "packed animation base block list entries (8 bytes: x, y, z as s16, then the halfword the unpacked entry's +6 receives; list 1 bits 8-13 and list 2 high byte zero)",
        BDEC,
        UNPACK_FN,
        space,
    );
    let r = 4 * records;
    if b.len() < r + 4 {
        return bad(t, parent, l_offs, at, &[(0, b.len() as u64)], format!("base block area of {:#x} bytes, {records} records and the block need more", b.len()));
    }
    let bi = match read_base(&b[r..], joints) {
        Ok(v) => v,
        Err(e) => return bad(t, parent, l_offs, at, &[(r as u64, 4)], e),
    };
    if b.len() != r + bi.len {
        return bad(t, parent, l_offs, at, &[(r as u64, 4)], format!("{:#x} bytes, expected {r:#x} + {:#x}", b.len(), bi.len));
    }
    if r != 0 {
        let mut n = t.child(parent, l_rec, at, r as u64);
        n.skip(0, r as u64);
        t.close(n);
    }
    let mut n = t.child(parent, l_offs, at + r as u64, 4);
    n.mark(0, 4);
    t.close(n);
    if joints != 0 {
        let mut n = t.child(parent, l_comp, at + r as u64 + 4, 6 * joints as u64);
        n.mark(0, 6 * joints as u64);
        t.close(n);
    }
    let mut n = t.child(parent, l_cnt, at + r as u64 + 4 + 6 * joints as u64, 2);
    n.mark(0, 2);
    t.close(n);
    let p = r + 6 + 6 * joints;
    if bi.n1 + bi.n2 != 0 {
        let mut n = t.child(parent, l_ent, at + p as u64, 8 * (bi.n1 + bi.n2) as u64);
        for k in 0..bi.n1 + bi.n2 {
            let h = u16_at(b, p + 8 * k + 6);
            let (mask, what) = if k < bi.n1 { (0x3f00, "list 1 bits 8-13") } else { (0xff00, "list 2 high byte") };
            if h & mask != 0 {
                return stop(t, n, format!("{} +{:#x}: base list entry {k}: +6 halfword {h:#06x}, {what} must be zero", parent.place, at + (p + 8 * k + 6) as u64));
            }
        }
        n.mark(0, 8 * (bi.n1 + bi.n2) as u64);
        t.close(n);
    }
    Ok(())
}

/// One packed frame at `at`: `b` runs from the frame's start to the next structure (the frame, then zero padding), `base` starts at
/// the sequence's base block (4 * +0x13 bytes after its frame offset table) and `joints` is the class's byte +8. The frame's size is
/// 4 + the 10-bit stream (4 * ceil(components coded 2 / 3)) + 2 * components coded 3 + components coded 1 + ceil(joints / 8)
/// + for each of the n1 + n2 list entries 1 + one byte per x, y, z coded 1 and two per component coded 2; the rest of `b` is zero.
pub fn packed_frame(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], base: &[u8], joints: usize, space: Space) -> Result<(), String> {
    let l_dur = t.line(group, "packed animation frame duration (+0, u16; the rate is 1 / value, 0 for a rate of 0)", PDEC, DURATION_FN, space);
    let l_val = t.line(group, "packed animation frame value (+2, s16 read as value * 0.0625)", PDEC, PVALUE_FN, space);
    let l_a = t.line(group, "packed animation frame 10-bit deltas (u32 words of three signed 10-bit fields at bits 0, 10, 20, read as 4 * delta, for the components coded 2; bits 30-31 and unused fields zero)", PDEC, UNPACK_FN, space);
    let l_s = t.line(group, "packed animation frame s16 values (the components coded 3)", PDEC, UNPACK_FN, space);
    let l_b = t.line(group, "packed animation frame byte deltas (s8, read as 4 * byte, for the components coded 1)", PDEC, UNPACK_FN, space);
    let l_w = t.line(group, "packed animation frame sign bits (one per joint, least significant first: w of the unpacked rotation is negated; unused bits zero)", PDEC, UNPACK_FN, space);
    let l_list = t.line(
        group,
        "packed animation frame list entry deltas (per entry a control byte of 2-bit codes for x, y, z (0 base value, 1 byte delta read as 4 * byte, 2 s16 that replaces it; bits 6-7 zero), then the bytes the codes name)",
        PDEC,
        UNPACK_FN,
        space,
    );
    let l_pad = t.line(group, "zero padding after a packed animation frame (to the next frame or structure)", PDEC, NOTHING, space);

    let bi = match read_base(base, joints) {
        Ok(v) => v,
        Err(e) => return bad(t, parent, l_dur, at, &[(0, b.len() as u64)], format!("base block: {e}")),
    };
    let size_a = bi.size_a();
    let (p_a, p_s) = (4, 4 + size_a);
    let p_b = p_s + 2 * bi.n_s;
    let p_w = p_b + bi.n_b;
    let p_l = p_w + bi.joints.div_ceil(8);
    if b.len() < p_l {
        return bad(t, parent, l_dur, at, &[(0, b.len() as u64)], format!("frame area of {:#x} bytes, its streams reach {p_l:#x}", b.len()));
    }
    // The 10-bit stream: its unused fields and the two top bits of each word are zero.
    for wi in 0..size_a / 4 {
        let w = u32::from_le_bytes([b[p_a + 4 * wi], b[p_a + 4 * wi + 1], b[p_a + 4 * wi + 2], b[p_a + 4 * wi + 3]]);
        let used = (bi.n_a - 3 * wi).min(3);
        let mask: u32 = (0..used).map(|j| 0x3ffu32 << (10 * j)).sum();
        if w & !mask != 0 {
            return bad(t, parent, l_a, at, &[(p_a as u64, size_a as u64)], format!("word {wi} is {w:#010x}, only {used} fields of 10 bits are used"));
        }
    }
    if bi.joints % 8 != 0 {
        let last = b[p_l - 1];
        if last >> (bi.joints % 8) != 0 {
            return bad(t, parent, l_w, at, &[(p_w as u64, (p_l - p_w) as u64)], format!("last sign byte {last:#04x}, {} joints use {} of its bits", bi.joints, bi.joints % 8));
        }
    }
    // The list entries.
    let mut q = p_l;
    for k in 0..bi.n1 + bi.n2 {
        let fail = |t: &mut Tracker, parent: &mut Node, e: String| bad(t, parent, l_list, at, &[(p_l as u64, (b.len() - p_l) as u64)], e);
        if q >= b.len() {
            return fail(t, parent, format!("list entry {k}: control byte at {q:#x} past the {:#x} bytes", b.len()));
        }
        let c = b[q];
        q += 1;
        if c & 0xc0 != 0 {
            return fail(t, parent, format!("list entry {k}: control byte {c:#04x}, bits 6-7 must be zero"));
        }
        for j in 0..3 {
            match (c >> (2 * j)) & 3 {
                1 => q += 1,
                2 => q += 2,
                3 => return fail(t, parent, format!("list entry {k}: control byte {c:#04x}, code 3 for component {j}")),
                _ => {}
            }
        }
        if q > b.len() {
            return fail(t, parent, format!("list entry {k}: values run to {q:#x}, past the {:#x} bytes", b.len()));
        }
    }
    let end = q;
    if let Some(i) = b[end..].iter().position(|&v| v != 0) {
        return bad(t, parent, l_pad, at, &[(end as u64, (b.len() - end) as u64)], format!("padding byte {:#04x} at +{:#x}, expected zero", b[end + i], end + i));
    }

    let mut n = t.child(parent, l_dur, at, 2);
    n.mark(0, 2);
    t.close(n);
    let mut n = t.child(parent, l_val, at + 2, 2);
    n.mark(0, 2);
    t.close(n);
    for (line, from, to) in [(l_a, p_a, p_s), (l_s, p_s, p_b), (l_b, p_b, p_w), (l_w, p_w, p_l), (l_list, p_l, end)] {
        if to > from {
            let mut n = t.child(parent, line, at + from as u64, (to - from) as u64);
            n.mark(0, (to - from) as u64);
            t.close(n);
        }
    }
    if b.len() > end {
        let mut n = t.child(parent, l_pad, at + end as u64, (b.len() - end) as u64);
        n.mark(0, (b.len() - end) as u64);
        t.close(n);
    }
    Ok(())
}
