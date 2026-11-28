//! Which tie triangles the game's own tie programs draw this frame, decided here from the frame's VIF1 jobs and the instance
//! records, as VU1 programs 2 and 3 decide them (the microcode's arithmetic, read from the programs; `captures/agent-tie/prog2.s`
//! and `prog3.s` in the main checkout are the listings).
//!
//! The chain sends each drawn tie instance's record (UNPACK V4-32 of 6 quadwords to VU1 0xD6, TOPS-relative: the 6 quadwords at
//! `TieGlobals::records` + 16 x the list word's record index), its colours (UNPACK V4-8 to 0xDC) and then one run per packet of the
//! LOD it draws: MSCAL 0x0, or 0x2 (entry 0x10, the back-face test) when func_L01_004B0990 finds the instance nearer than the
//! packet's LOD entry +5, of program 2, or of program 3 for the ties its cull sends to clipping (`Call`, `tie_draws`). The
//! record's quadwords 0 to 3 are the model-to-screen matrix (columns x, y, z and the translation), quadword 4's x the morph factor
//! (a vertex with `sway` is at `sway * k + pos`), quadword 5's z the instance's winding word.
//!
//! What the programs leave undrawn is the ADC bit of a strip vertex: the GS draws no triangle there (`decide`). Each triangle's
//! decision rests on its own three vertices and the instance's record alone, so the same triangle seen the same way is decided
//! the same way in every frame.
//!
//! - **Program 2, entry 0x10** (micro 0x1DE8 to 0x1FE0): for every strip triangle the z of the cross product of its screen
//!   edges, the 12.4 X and Y words read back as floats (ITOF4): (v1 - v0) x (v2 - v1) for the strip's even triangles, the
//!   operands swapped for the odd ones (the strip's winding alternates); its MAC flags (0x20 negative, 0x02 zero) equal to the
//!   strip's `side` XOR the record's winding word set ADC at the triangle's third vertex. Entry 0x0 draws every triangle.
//! - **Program 3**: the transform leaves each vertex's clip coordinates (x, y, z, w). Entry 0x10 (micro 0x1DE8 to 0x20B0, and
//!   the pass at 0x26E8) flags a triangle when the sign of det(v0, v1, v2) over (x, y, w), taken in the same alternating order,
//!   equals the strip's `side` XOR the winding word (MAC flag 0x20 only): the triangle faces away from the eye. A flagged
//!   triangle gets ADC unless the low half of the strip's second vertex's w (the word the flag is stored as) is 0. Then for
//!   every triangle (micro 0x2198 on): one with a vertex outside the guard band (CLIP of x, y, z against w: the clip space is 4
//!   times the screen) gets ADC at its third vertex; unless it is flagged (the clipper sends no fan for it, micro 0x3260 to
//!   0x32A0) or all three vertices lie outside one plane of the screen (CLIP of x, y, z scaled by `Call::screen`, vf17, against
//!   w), the program sends it clipped to the band as a fan of its own (L_3258 to the clipper at L_3620). The native picture
//!   draws such a triangle whole: the GPU clips it.
//!
//! The determinant is the program's test as the console runs it. For a day (015b3a6 to this change, 14 November 2026) the port
//! read the flag the way rac-rs's emulated VU1 does: the MAC flags of the OPMULA the OPMSUB stalls for, the next triangle's first
//! partial products, with what VU1 memory held past a strip's end (it followed VU1 memory through the whole chain for that).
//! That made the emulated game's picture and dropped front-facing triangles the console draws: on level 28 the sewer pipe's
//! walls (their second draws' shine painted over the empty wall: a pink and purple swirl), the dark green faces inside the
//! riveted wheel rims over the slime channel (left out on the tour's field 37800, and decided differently from one field to the
//! next in play: a flicker); the let's-play footage of the console shows them whole (16:08 to 16:29 of "Let's Play Ratchet &
//! Clank 3: Up Your Arsenal Part 11: Sewer Slimes"). The determinant draws them as the console does.
//!
//! The signs rest on coordinate differences that can be 0 or one unit (a strip whose vertices pair up at one clip x), so the
//! clip coordinates and the determinant are computed with the VU's arithmetic: each product and sum rounded toward zero, in the
//! program's order (`clip_coords`, `det`).

use crate::objects::FastMap;

/// One run of a tie program drawing an instance's packet (its first draw).
#[derive(Clone, Debug)]
pub(crate) struct Call {
    /// Program 2, or 3 (the instance is clipped).
    pub program: u8,
    /// Entry 0x10: the back-face test.
    pub test: bool,
    /// Program 3's vf17 (VU1 0x3CF, sent by the chain): the screen's scale of its clip space.
    pub screen: [f32; 4],
    /// The instance record's 6 quadwords as the chain sent them (the UNPACK's data).
    pub record: [[u32; 4]; 6],
}

/// Per tie instance record (the EE address of the 6 quadwords the chain unpacks to VU1 0xD6), its runs at entry 0x0 or 0x10 in
/// the order the chain sends them: one per packet of the LOD it draws the instance at.
pub(crate) struct TieDraws {
    pub by_record: FastMap<u32, Vec<Call>>,
}

/// The frame's tie runs (`TieDraws`), read from the VIF1 jobs' codes.
pub(crate) fn tie_draws(jobs: &crate::jobs::FrameJobs) -> Result<TieDraws, String> {
    let mut by_record: FastMap<u32, Vec<Call>> = FastMap::default();
    // The program the jobs uploaded last (`mobies::PROGRAMS`, by the main program's address of its first piece).
    let mut resident: Option<u8> = None;
    let mut screen: Option<[f32; 4]> = None;
    for job in &jobs.jobs {
        let crate::jobs::Job::Vif1 { words, marks, blocks, .. } = job else { continue };
        // The EE address of a data word of the job (its DMA block's source plus its place in the block). The blocks and marks are
        // in the chain's order, by word: a word's is the last that starts at or before it.
        let source = |at: usize| -> Option<u32> {
            let k = blocks.partition_point(|b| b.0 <= at).checked_sub(1)?;
            Some(blocks[k].1 + 4 * (at - blocks[k].0) as u32)
        };
        // A word's place in its quadword as the VIF gets it: a tag's two VIF words are the upper half of the tag.
        let phase = |at: usize| -> usize {
            match marks.partition_point(|m| m.0 <= at).checked_sub(1) {
                Some(k) if marks[k].1 == 2 => 2 + (at - marks[k].0),
                Some(k) => (at - marks[k].0) % 4,
                None => at % 4,
            }
        };
        let mut record = None;
        // Whether the instance's colours were unpacked since the last run (UNPACK V4-8 to 0xDC): a packet's first draw takes them,
        // its second draw does not (its coordinates, or nothing: a packet whose second part has none of its own, entry flags
        // 16, is set up at micro 0x70 and drawn again at entry 0x0 with the record alone; level 8 field 12600).
        let mut colours = false;
        let (mut cl, mut wl) = (1usize, 1usize);
        let mut i = 0;
        while i < words.len() {
            let code = words[i];
            let (cmd, imm, num) = ((code >> 24) & 0x7f, code & 0xffff, ((code >> 16) & 0xff) as usize);
            i += 1;
            match cmd {
                0x00 | 0x02..=0x07 | 0x10 | 0x11 | 0x13 => {}
                0x01 => (cl, wl) = ((imm & 0xff) as usize, (imm >> 8 & 0xff) as usize),
                0x20 => i += 1,
                0x30 | 0x31 => i += 4,
                // MSCAL, MSCALF: a run of program 2 or 3 drawing an instance's packet (its first draw).
                0x14 | 0x15 => {
                    let entry = imm * 8;
                    if matches!(resident, Some(2 | 3)) && matches!(entry, 0x0 | 0x10) && std::mem::take(&mut colours) {
                        let (rec, words) = record.ok_or("native: a tie program run with no instance record unpacked before it")?;
                        let program = resident.expect("matched above");
                        let screen = match program {
                            3 => screen.ok_or("native: a program 3 run before the chain sent its vf17 (VU1 0x3CF)")?,
                            _ => [0.0; 4],
                        };
                        by_record.entry(rec).or_default().push(Call { program, test: entry == 0x10, screen, record: words });
                    }
                    colours = false;
                }
                0x17 => colours = false,
                // MPG to micro address 0: the program it uploads.
                0x4a => {
                    if imm == 0 {
                        let at = source(i);
                        // By the packet's address, else by its microcode (`mobies::program_of`: a chain read through a copy).
                        resident = match crate::mobies::PROGRAMS.iter().find(|(_, pieces)| Some(pieces[0]) == at).map(|&(p, _)| p) {
                            Some(p) => Some(p),
                            None => crate::mobies::program_of(None, words.get(i..).unwrap_or(&[])),
                        };
                    }
                    i += 2 * if num == 0 { 256 } else { num };
                }
                // DIRECT: its GIF data starts at the next quadword.
                0x50 | 0x51 => {
                    i += (4 - phase(i)) % 4;
                    i += 4 * if imm == 0 { 0x10000 } else { imm as usize };
                }
                0x60..=0x7f => {
                    let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
                    let n = if num == 0 { 256 } else { num };
                    // Its size: CL and WL of STCYCL (a write cycle longer than the read one skips no data).
                    let items = if wl <= cl || wl == 0 { n } else { (n / wl) * cl + (n % wl).min(cl) };
                    let bits = if vl == 3 { 16 } else { (vn as usize + 1) * (32 >> vl) };
                    let len = (items * bits).div_ceil(32);
                    match cmd & 0x6f {
                        0x6e => colours = true,
                        // UNPACK V4-32 of 6 quadwords (to 0xD6 of the buffer the run reads: TOPS-relative, so instances alternate
                        // buffers): the instance record, the chain's only such unpack.
                        0x6c if num == 6 => {
                            let at = source(i).ok_or("native: a tie record UNPACK outside the job's blocks")?;
                            let w = words.get(i..i + 24).ok_or("native: a tie record UNPACK past its job")?;
                            record = Some((at, std::array::from_fn(|q| std::array::from_fn(|k| w[4 * q + k]))));
                        }
                        // V4-32 to an absolute address over program 3's vf17 (0x3CF).
                        0x6c if imm & 0x8000 == 0 && (imm & 0x3ff) as usize <= 0x3cf && 0x3cf < (imm & 0x3ff) as usize + items => {
                            let q = i + 4 * (0x3cf - (imm & 0x3ff) as usize);
                            let w = words.get(q..q + 4).ok_or("native: a tie chain UNPACK past its job")?;
                            screen = Some(std::array::from_fn(|k| f32::from_bits(w[k])));
                        }
                        _ => {}
                    }
                    i += len;
                }
                c => return Err(format!("native: VIF code {c:#x} in the tie chain")),
            }
        }
    }
    Ok(TieDraws { by_record })
}

/// The 12.4 fixed point of FTOI4.
fn ftoi4(f: f32) -> i32 {
    (f * 16.0) as i32
}

/// A VU product, rounded toward zero to 24 bits of mantissa (the products here are of integers below 2^18, exact in f64).
fn vu_product(a: i64, b: i64) -> f64 {
    // Products below 2^36 are exact in f64 and `log2` floors them right: the 24 bits kept are the top 24 bits of the integer.
    if a.unsigned_abs() < 1 << 18 && b.unsigned_abs() < 1 << 18 {
        let p = a * b;
        let m = p.unsigned_abs();
        let drop = (64 - m.leading_zeros()).saturating_sub(24);
        let kept = (m >> drop) << drop;
        return if p < 0 { -(kept as f64) } else { kept as f64 };
    }
    let p = a as f64 * b as f64;
    if p == 0.0 {
        return 0.0;
    }
    let e = p.abs().log2().floor() as i32;
    let unit = 2f64.powi(e - 23);
    (p / unit).trunc() * unit
}

/// The VU's float arithmetic: each product, sum and quotient rounded toward zero to an f32, a result below the smallest normal
/// flushed to zero. (The f64 product of two f32 is exact; an f64 sum of two f32 is exact unless their exponents are more than
/// 29 apart, where the f32 result is the larger operand's either way.)
fn rz(x: f64) -> f32 {
    let r = x as f32;
    let r = if r != 0.0 && r.is_finite() && (r as f64).abs() > x.abs() { f32::from_bits(r.to_bits() - 1) } else { r };
    if r.abs() < f32::MIN_POSITIVE { 0.0f32.copysign(r) } else { r }
}

fn mul(a: f32, b: f32) -> f32 {
    rz(a as f64 * b as f64)
}

/// The record's matrix (columns x, y, z and the translation) and morph factor as floats.
fn matrix(rec: &[[u32; 4]; 6]) -> ([[f32; 4]; 4], f32) {
    ([0, 1, 2, 3].map(|q| rec[q].map(f32::from_bits)), f32::from_bits(rec[4][0]))
}

/// One vertex's clip coordinates (`clip_coords`), MXCSR already set to `VU_MXCSR`.
#[inline]
fn transform(m: &([[f32; 4]; 4], f32), v: &uyafmt::tie::TieVertex) -> [f32; 4] {
    let ([cx, cy, cz, ct], k) = *m;
    let p = match v.sway {
        Some(s) => [s[0] * k + v.pos[0], s[1] * k + v.pos[1], s[2] * k + v.pos[2]],
        None => v.pos,
    };
    [0, 1, 2, 3].map(|c| ct[c] + cx[c] * p[0] + cy[c] * p[1] + cz[c] * p[2])
}

/// A packet's clip coordinates, as the program's transform leaves them: the record's matrix times the class-local position, in
/// the program's order (MULAw of the translation, MADDAx, MADDAy, MADDz; a morph vertex's MULx by k, then ADD).
///
/// The VU's rounding is the x86's own with MXCSR set to round toward zero and flush results below the smallest normal to zero
/// (`VU_MXCSR`; no DAZ: an operand below the smallest normal is used as it is, as `mul` uses it), so the transform is the host's
/// f32 products and sums in the program's order, the mode set for it and put back after.
fn clip_coords(pk: &uyafmt::tie::TiePacket, rec: &[[u32; 4]; 6], out: &mut Vec<[f32; 4]>) {
    let m = matrix(rec);
    let old = uyart::fpu::set_mode(VU_MXCSR);
    out.clear();
    out.extend(pk.vertices.iter().map(|v| transform(&m, v)));
    uyart::fpu::set_mode(old);
}

/// MXCSR for the VU's arithmetic on the host: round toward zero, flush to zero, every exception masked.
const VU_MXCSR: u32 = 0x1f80 | 0x6000 | 0x8000;

/// CLIP's six flags of `v` against its w scaled by `s` (+x, -x, +y, -y, +z, -z).
fn clip_flags(v: [f32; 4], s: [f32; 4]) -> u32 {
    let w = (v[3] * s[3]).abs();
    (0..3).fold(0, |f, c| {
        let x = v[c] * s[c];
        f | ((x > w) as u32) << (2 * c) | ((x < -w) as u32) << (2 * c + 1)
    })
}

/// det(v0, v1, v2) over (x, y, w) of a strip triangle's clip coordinates, in the strip's alternating order (the edges' cross
/// product (v1 - v0) x (v2 - v1) for an even triangle, the operands swapped for an odd one, then its dot product with v2), each
/// difference, product and sum rounded toward zero as the VU rounds them. MXCSR already set to `VU_MXCSR`.
fn det(v0: [f32; 4], v1: [f32; 4], v2: [f32; 4], odd: bool) -> f32 {
    let h = |c: [f32; 4]| [c[0], c[1], c[3]];
    let (h0, h1, h2) = (h(v0), h(v1), h(v2));
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let cross = |a: [f32; 3], b: [f32; 3]| [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let (a, b) = (sub(h1, h0), sub(h2, h1));
    let c = if odd { cross(b, a) } else { cross(a, b) };
    c[2] * h2[2] + c[1] * h2[1] + c[0] * h2[0]
}

/// Room `decide` keeps from call to call: the packet's clip coordinates, a strip's screen coordinates and its vertices' clip flags.
#[derive(Default)]
pub(crate) struct DecideRoom {
    clip: Vec<[f32; 4]>,
    xy: Vec<[i64; 2]>,
    flags: Vec<(bool, u32)>,
}

/// The ADC flags program `call` leaves on `pk`'s strips for its instance record, into `adc`: each strip's in turn, one a vertex
/// (true: no triangle is drawn at that vertex).
pub(crate) fn decide(pk: &uyafmt::tie::TiePacket, call: &Call, adc: &mut Vec<bool>, room: &mut DecideRoom) {
    // Program 2 without the back-face test leaves no flag (of level 28's arrival's 4,489 packets a frame, 118 needed their clip
    // coordinates).
    if !(call.program == 3 || (call.program == 2 && call.test)) {
        adc.clear();
        adc.resize(pk.strips.iter().map(|st| st.vertices.len()).sum(), false);
        return;
    }
    let rec = &call.record;
    clip_coords(pk, rec, &mut room.clip);
    let clip = &room.clip;
    let winding = rec[5][2] & 0xffff;
    adc.clear();
    for st in &pk.strips {
        let (base, n) = (adc.len(), st.vertices.len());
        adc.resize(base + n, false);
        let v = |i: usize| clip[st.vertices[i] as usize];
        let expected = (st.side as u32 ^ winding) & 0xffff;
        match call.program {
            2 if call.test => {
                // Q = 1 / w (DIV), x and y times Q (MULq), then FTOI4.
                room.xy.clear();
                room.xy.extend((0..n).map(|i| {
                    let c = v(i);
                    let q = rz(1.0 / c[3] as f64);
                    [ftoi4(mul(c[0], q)) as i64, ftoi4(mul(c[1], q)) as i64]
                }));
                let xy = &room.xy;
                for i in 0..n.saturating_sub(2) {
                    let a = [xy[i + 1][0] - xy[i][0], xy[i + 1][1] - xy[i][1]];
                    let b = [xy[i + 2][0] - xy[i + 1][0], xy[i + 2][1] - xy[i + 1][1]];
                    let (u, w) = if i % 2 == 0 { (a, b) } else { (b, a) };
                    let (p, q) = (vu_product(u[0], w[1]), vu_product(w[0], u[1]));
                    let flags = if p < q { 0x20 } else if p == q { 0x02 } else { 0 };
                    adc[base + i + 2] = flags == expected;
                }
            }
            3 => {
                if call.test && n >= 2 && f32::to_bits(v(1)[3]) & 0xffff != 0 {
                    let old = uyart::fpu::set_mode(VU_MXCSR);
                    for i in 0..n - 2 {
                        let d = det(v(i), v(i + 1), v(i + 2), i % 2 == 1);
                        adc[base + i + 2] = (if d < 0.0 { 0x20 } else { 0 }) == expected;
                    }
                    uyart::fpu::set_mode(old);
                }
                // Per vertex: outside the clip volume, and its flags against the screen's.
                room.flags.clear();
                room.flags.extend((0..n).map(|i| (clip_flags(v(i), [1.0; 4]) != 0, clip_flags(v(i), call.screen))));
                let f = &room.flags;
                for i in 0..n.saturating_sub(2) {
                    if f[i].0 || f[i + 1].0 || f[i + 2].0 {
                        // The clipper sends no fan for a flagged (back-facing) triangle either (micro 0x3260 to 0x32A0).
                        adc[base + i + 2] |= f[i].1 & f[i + 1].1 & f[i + 2].1 != 0;
                    }
                }
            }
            _ => {}
        }
    }
}
