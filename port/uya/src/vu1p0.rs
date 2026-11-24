//! VU1 program 0, the mobies' (the main program's VU1 microcode uploaded from 0x110798, 1,335 instruction pairs), done by the
//! port's own code from its microcode on `vu1::Vu1` (the module doc there: the arithmetic, the pipeline and the check against
//! the translation). The moby chain's runs, the runs outside it (the stars, the front end's 3D) and the runs of classes the
//! port has no mesh for all run it.
//!
//! # Its entries (MSCAL's immediate times 8)
//!
//! - 0x00: vf27 = (712, 840, 712, 840), the two buffers of VU1 memory each run writes its GS packets into and kicks, in turn.
//! - 0x40, 0x50 (variant A: the fog per vertex, back-face culling and clipping) and 0x60, 0x70 (variant B: the fog for the whole
//!   run, nothing culled or clipped) run the mesh at TOP (0x50, 0x70) or at 356 (0x40, 0x60): vi13 is the buffer.
//! - 0xA0 (TOP) and 0xB0 (vi13 as the last run left it): the buffer's positions and colours made 12-bit fixed point (FTOI12)
//!   first, then variant A (a negative vertex count) or B.
//! - 0x10, 0x20, 0x30, 0x80, 0x90: an E bit at once (nothing).
//!
//! # A run of a mesh
//!
//! The buffer (vi13) holds the model-to-clip rows (vf17..vf20 at +653), the constants (+647..+652: vf24 the fog's floor, vf23
//! the GIF tag and Q's numerator, vf28 the guard band's scale, vf21 the w column for the clip test, vf22 the screen offset), the
//! vertex arrays (positions +355, texture coordinates +549, colours +452; a strip's restart vertices +483, +677, +580), the
//! strip list (from +657: one 16-bit index a vertex, a negative one a restart, 0 a texture group's end) and the texture setups.
//! Each vertex is transformed (ACC = vf20 + vf17 x + vf18 y + vf19 z), its screen position vf22 + clip.xyz vf23.x / clip.w
//! (FTOI4), its fog clip.w + vf22.w between vf24.x and 255 (A) or the run's (B), and stored with its ST times Q and its colour
//! into the GIF packet at vf29.w; the packet is kicked when the buffer is full and when the strip list ends, the two buffers in
//! turn (vf27, MR32). Variant A's loop is four copies of one slot, the registers turned by one each (`SlotA`): each vertex ends
//! a triangle of the strip, whose screen-space orientation (the z of the cross product of its two edges, MSUBy.x's sign as
//! FSAND reads it) against the run's cull flag (vi10) and whose clip judgements (FCAND: any of the three outside, then any
//! outside the near plane or the guard band) decide: a triangle outside goes to the clip handler (`clip_slot`), which tests
//! the guard band (the three corners scaled by vf28) and, unless all three are outside one plane of it, clips the triangle
//! (`clipper`, 0x1CD0) and kicks it as a fan of its own.
//!
//! The clip handler reads the clip flag (FCGET) while the judgements of its last two CLIPs are still on their way (each lands
//! four cycles after its CLIP), and FSAND reads the MAC flags of the result named at each (the orientation's MSUBy.x, or the
//! transform's last MADD on the loop's way in): the code says so where it does. The clipper's edge that leaves a plane jumps
//! through vi01 right after an IADDIU of it, so the jump takes vi01's old value, 0: the program's start, whose code ends the run
//! as entry 0's does (the translator's doc: the branch hazard). Every other value is the program's in program order.

use crate::vu1::{bc, clip, s16, sx, Vu1, ACC, I, W, X, XY, XYW, XYZ, XYZW, Y, Z, ZW, XW, YW};

/// More loop steps than this in one run is data the program loops on (the translation stops at 5,000,000 pairs).
const BUDGET: u32 = 2_000_000;

fn step(budget: &mut u32) -> Result<(), String> {
    *budget = budget.checked_sub(1).ok_or("VU1 program 0: more than 2,000,000 loop steps without ending")?;
    Ok(())
}

pub(crate) fn run(v: &mut Vu1, entry: usize) -> Result<usize, String> {
    let mut budget = BUDGET;
    match entry {
        0x00 => Ok(buffers(v)),
        0x10 | 0x20 | 0x30 | 0x80 | 0x90 => Ok(entry + 8),
        0x40 | 0x50 | 0x60 | 0x70 => {
            // B to 0x248 or 0x12E0, its delay slot vi13 = 356 or TOP.
            let t = if entry & 0x10 == 0 { 356 } else { v.top };
            v.set_vi(13, t);
            if entry < 0x60 { mesh_a(v, &mut budget) } else { mesh_b(v, &mut budget) }
        }
        0xa0 | 0xb0 => {
            if entry == 0xa0 {
                v.set_vi(13, v.top);
            }
            fixed(v, &mut budget)
        }
        e => Err(format!("VU1 program 0 started at {e:#x} (its runs start at 0x0 to 0xB0, by 0x10)")),
    }
}

/// Entry 0x00 (0x218..0x240): vf27 = (712, 840, 712, 840). The micro address of its E bit's slot.
fn buffers(v: &mut Vu1) -> usize {
    v.set_vi(1, 712);
    v.vf[27][0] = sx(v.vi[1]);
    v.vf[27][2] = sx(v.vi[1]);
    v.set_vi(1, 840);
    v.vf[27][1] = sx(v.vi[1]);
    v.vf[27][3] = sx(v.vi[1]);
    0x240
}

/// Entries 0xA0, 0xB0 (0x0B0..0x210): the positions (four quadwords at a time) and the colours made FTOI12 in place; then
/// variant A for a negative vertex count (made positive), B for another.
fn fixed(v: &mut Vu1, budget: &mut u32) -> Result<usize, String> {
    v.set_vi(1, v.vi[13].wrapping_add(291));
    v.ilw(2, v.vi[1].wrapping_add(10), 3);
    v.set_vi(6, 0);
    v.set_vi(3, v.vi[13]);
    let positive = s16(v.vi[2]) >= 0;
    v.set_vi(4, v.vi[13].wrapping_add(194));
    if !positive {
        v.set_vi(2, 0u32.wrapping_sub(v.vi[2]));
        v.set_vi(6, 1);
    }
    v.set_vi(5, v.vi[2]);
    loop {
        step(budget)?;
        v.set_vi(2, v.vi[2].wrapping_sub(4));
        v.lq::<XYZW>(1, v.vi[3]);
        let rest = s16(v.vi[2]) < 0;
        v.lq::<XYZW>(2, v.vi[3].wrapping_add(1));
        if rest {
            break;
        }
        v.lq::<XYZW>(3, v.vi[3].wrapping_add(2));
        v.lq::<XYZW>(4, v.vi[3].wrapping_add(3));
        for (k, r) in [1usize, 2, 3, 4].into_iter().enumerate() {
            v.ftoi::<2, XYZW>(r, v.vf[r]);
            v.lq::<XYZW>(r + 4, v.vi[4].wrapping_add(k as u32));
        }
        for r in [1usize, 2, 3, 4] {
            v.ftoi::<2, XY>(r + 4, v.vf[r + 4]);
            v.st::<XYZW>(v.vi[3], v.vf[r]);
            v.set_vi(3, v.vi[3].wrapping_add(1));
        }
        for r in [5usize, 6, 7, 8] {
            v.st::<XY>(v.vi[4], v.vf[r]);
            v.set_vi(4, v.vi[4].wrapping_add(1));
        }
    }
    // 0x190: the last ones (fewer than four).
    let none = s16(v.vi[2]) <= 0;
    v.set_vi(2, v.vi[2].wrapping_add(4));
    if !none {
        loop {
            step(budget)?;
            v.lq::<XYZW>(1, v.vi[3]);
            v.lq::<XYZW>(2, v.vi[4]);
            v.set_vi(2, v.vi[2].wrapping_sub(1));
            v.ftoi::<2, XYZW>(1, v.vf[1]);
            v.ftoi::<2, XYZW>(2, v.vf[2]);
            v.st::<XYZW>(v.vi[3], v.vf[1]);
            v.set_vi(3, v.vi[3].wrapping_add(1));
            let more = s16(v.vi[2]) > 0;
            v.st::<XYZW>(v.vi[4], v.vf[2]);
            v.set_vi(4, v.vi[4].wrapping_add(1));
            if !more {
                break;
            }
        }
    }
    // 0x1F8: IBGTZ vi6 to variant A, its delay slot XTOP vi13; else B.
    let a = s16(v.vi[6]) > 0;
    v.set_vi(13, v.top);
    if a { mesh_a(v, budget) } else { mesh_b(v, budget) }
}

/// The mesh's start, both variants (0x248..0x308 and 0x12E0..0x13A0): the strip list's head (vi8; vi11 the texture setups,
/// vi12 the next group, vi2..vi4 the first three indices, vi7 the buffer's room), the first vertex (vf1, vf9, vf5) and the
/// matrix (vf17..vf20).
fn head(v: &mut Vu1) {
    v.set_vi(13, v.vi[13].wrapping_sub(356));
    v.set_vi(8, v.vi[13].wrapping_add(659));
    v.ilw(11, v.vi[8].wrapping_sub(2), 1);
    v.ilw(12, v.vi[8].wrapping_sub(2), 2);
    v.ilw(2, v.vi[8].wrapping_sub(1), 0);
    v.set_vi(7, 115);
    v.set_vi(11, v.vi[11].wrapping_add(v.vi[8]));
    v.set_vi(11, v.vi[11].wrapping_sub(2));
    v.vf[10][3] = sx(v.vi[11]);
    // LOI 2^23 (I from the next pair).
    v.vi[I] = 0x4b00_0000;
    v.ilw(3, v.vi[8].wrapping_sub(1), 1);
    v.ilw(4, v.vi[8].wrapping_sub(1), 2);
    v.itof::<0, XYZW>(10, v.vf[10]);
    let restart = v.vi[2] != 0;
    v.set_vi(1, v.vi[2].wrapping_add(128));
    if !restart {
        v.set_vi(1, v.vi[12]);
        v.isw::<X>(12, v.vi[8].wrapping_sub(1));
        v.add::<W>(10, v.vf[10], v.vf[0]);
        v.ilw(12, v.vi[11], 3);
    }
    v.set_vi(1, v.vi[1].wrapping_add(v.vi[13]));
    v.lq::<XYZW>(1, v.vi[1].wrapping_add(355));
    v.add::<W>(10, v.vf[10], v.i());
    v.lq::<XYZ>(9, v.vi[1].wrapping_add(549));
    v.lq::<XYZW>(5, v.vi[1].wrapping_add(452));
    v.lq::<XYZW>(20, v.vi[13].wrapping_add(656));
    v.itof::<0, XYZW>(1, v.vf[1]);
    v.lq::<XYZW>(17, v.vi[13].wrapping_add(653));
    v.itof::<2, XYZ>(9, v.vf[9]);
    v.lq::<XYZW>(18, v.vi[13].wrapping_add(654));
    v.lq::<XYZW>(19, v.vi[13].wrapping_add(655));
}

/// The registers of one of variant A's four slots (0x418 + 0x90 s): positions a (the vertex before last's, being finished),
/// b (the last's), c (the new one's), d (the oldest's, stored now); texture coordinates and colours; the clip-space copies
/// (cl_c the new one's judged now, cl_b the last's, made now); the edge (vf29 or vf30) and the strip indices (ip the last's, ic
/// the new one's).
#[derive(Clone, Copy)]
struct SlotA {
    s: usize,
    a: usize,
    b: usize,
    c: usize,
    d: usize,
    st_a: usize,
    st_b: usize,
    st_c: usize,
    col_a: usize,
    col_c: usize,
    cl_b: usize,
    cl_c: usize,
    cl_d: usize,
    e: usize,
    ip: usize,
    ic: usize,
}

impl SlotA {
    fn of(s: usize) -> SlotA {
        let pos = |x: usize| 1 + (x - 1 + s) % 4;
        let st = |x: usize| 9 + (x - 9 + s) % 4;
        let col = |x: usize| 5 + (x - 5 + s) % 4;
        let cl = |x: usize| 13 + (x - 13 + s) % 4;
        let idx = |x: usize| 2 + (x - 2 + s) % 4;
        SlotA { s, a: pos(2), b: pos(3), c: pos(4), d: pos(1), st_a: st(9), st_b: st(11), st_c: st(12), col_a: col(5), col_c: col(8), cl_b: cl(15), cl_c: cl(13), cl_d: cl(16), e: 29 + s % 2, ip: idx(5), ic: idx(2) }
    }

    /// The quadword of the strip list ic is loaded from (vi08 + 0 for the first two slots, - 1 after slot 2's vi8 += 1) and its
    /// lane.
    fn ic_at(&self, v: &Vu1) -> (u32, usize) {
        (if self.s < 2 { v.vi[8] } else { v.vi[8].wrapping_sub(1) }, self.s)
    }
}

/// Where a slot of variant A resumes: its start (0x418), after the first pairs (0x438: the loop's way in), 0x448 (a strip's
/// restart), 0x490 (the end of a strip's restart) or 0x498 (after a strip's end or the clip handler).
#[derive(Clone, Copy, PartialEq, Eq)]
enum From {
    Top,
    Enter,
    At448,
    At490,
    At498,
}

/// What a slot of variant A ends in.
enum Out {
    Next,
    Restart,
    StripEnd,
    Clip,
}

/// Variant A (0x248..0xE58 and its handlers to 0x12D8).
fn mesh_a(v: &mut Vu1, budget: &mut u32) -> Result<usize, String> {
    head(v);
    // 0x310..0x410: the second and third vertices transformed, Q for the first two; the constants; the first tag (vf23 to vf29.w).
    v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
    v.set_vi(1, v.vi[3].wrapping_add(v.vi[13]));
    v.madd::<XYZW>(ACC, v.vf[17], bc(v.vf[1][0]));
    v.lq::<XYZW>(2, v.vi[1].wrapping_add(483));
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[1][1]));
    v.lq::<XYZ>(10, v.vi[1].wrapping_add(677));
    v.madd::<XYZW>(1, v.vf[19], bc(v.vf[1][2]));
    v.lq::<XYZW>(23, v.vi[13].wrapping_add(648));
    v.lq::<XYZW>(6, v.vi[1].wrapping_add(580));
    v.itof::<0, XYZW>(2, v.vf[2]);
    v.set_vi(1, v.vi[4].wrapping_add(v.vi[13]));
    v.itof::<2, XYZ>(10, v.vf[10]);
    v.lq::<XYZW>(3, v.vi[1].wrapping_add(355));
    v.div(v.vf[23][0], v.vf[1][3]);
    v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
    v.lq::<XYZ>(11, v.vi[1].wrapping_add(549));
    v.madd::<XYZW>(ACC, v.vf[17], bc(v.vf[2][0]));
    v.lq::<XYZW>(21, v.vi[13].wrapping_add(650));
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[2][1]));
    v.lq::<XYZW>(7, v.vi[1].wrapping_add(452));
    v.madd::<XYZW>(2, v.vf[19], bc(v.vf[2][2]));
    v.lq::<XYZW>(22, v.vi[13].wrapping_add(652));
    v.itof::<0, XYZW>(3, v.vf[3]);
    v.isw::<X>(13, 976);
    v.mul::<XYZW>(13, v.vf[1], v.vf[21]);
    v.vi[I] = 0x453f_f000;
    v.mul::<XYZ>(1, v.vf[1], v.q());
    v.lq::<XYZW>(24, v.vi[13].wrapping_add(647));
    v.mul::<XYZ>(9, v.vf[9], v.q());
    v.div(v.vf[23][0], v.vf[2][3]);
    v.itof::<2, XYZ>(11, v.vf[11]);
    v.set_vi(6, v.vf[27][0] & 0xffff);
    v.add::<W>(9, v.vf[0], v.i());
    v.vi[I] = 0x437e_0000;
    v.add::<XYZW>(1, v.vf[1], v.vf[22]);
    v.lq::<XYZW>(28, v.vi[13].wrapping_add(649));
    v.ilw(10, v.vi[8].wrapping_sub(2), 0);
    v.mul::<XYZW>(ACC, v.vf[17], bc(v.vf[3][0]));
    v.vf[29][3] = sx(v.vi[6]);
    v.add::<W>(11, v.vf[0], v.i());
    v.st::<XYZW>(v.vi[6], v.vf[23]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    v.max::<W>(1, v.vf[1], bc(v.vf[24][0]));
    v.ilw(5, v.vi[8].wrapping_sub(1), 3);
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[3][1]));
    v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[3][2]));
    v.set_vi(9, 0u32.wrapping_sub(1));
    v.mul::<XYZ>(10, v.vf[10], v.q());
    v.mul::<XYZW>(14, v.vf[2], v.vf[21]);
    v.mul::<XYZ>(2, v.vf[2], v.q());
    let group = v.vi[5] == 0;
    v.madd::<XYZW>(3, v.vf[20], bc(v.vf[0][3]));
    // The MAC flags the first FSAND reads: this MADD's (the orientation's MSUB has not run).
    let mut orient = v.last_mac();
    v.set_vi(1, v.vi[5].wrapping_add(v.vi[13]));
    let (mut s, mut from) = if group {
        (0, restart_a(v, &SlotA::of(0), 0x30, &mut orient))
    } else if s16(v.vi[5]) < 0 {
        (0, restart_a(v, &SlotA::of(0), 0x10, &mut orient))
    } else {
        v.lq::<XYZW>(4, v.vi[1].wrapping_add(355));
        (0, Some(From::Enter))
    };
    loop {
        step(budget)?;
        let Some(at) = from else {
            return Ok(strips_end(v));
        };
        let r = SlotA::of(s);
        from = match slot_a(v, &r, at, &mut orient) {
            Out::Next => {
                if s < 3 {
                    s += 1;
                    Some(From::Top)
                } else {
                    s = 0;
                    // Slot 4's IBGEZ vi7 back to 0x418; under it, the full buffer kicked first (0x658).
                    if s16(v.vi[7]) < 0 {
                        flush(v, 4);
                    }
                    Some(From::Top)
                }
            }
            Out::Restart => restart_a(v, &r, 0, &mut orient),
            Out::StripEnd => Some(strip_end_a(v, &r, budget)?),
            Out::Clip => match clip_slot(v, &r, budget)? {
                Some(end) => return Ok(end),
                None => Some(From::At498),
            },
        };
    }
}

/// One slot of variant A from `at` to its end or a branch out of it.
fn slot_a(v: &mut Vu1, r: &SlotA, at: From, orient: &mut u32) -> Out {
    if at == From::Top {
        // +0x00: IBLEZ ip to the restart handler, its delay slot the last vertex's transform finished (b).
        v.mul::<XYZ>(r.a, v.vf[r.a], v.q());
        let restart = s16(v.vi[r.ip]) <= 0;
        v.madd::<XYZW>(r.b, v.vf[20], bc(v.vf[0][3]));
        v.set_vi(1, v.vi[r.ip].wrapping_add(v.vi[13]));
        if restart {
            return Out::Restart;
        }
        // +0x10, +0x18: d stored; the orientation (edges vf29 and vf30: x29 y30 - x30 y29, its sign); the new vertex's position.
        v.mul::<X>(ACC, v.vf[29], bc(v.vf[30][1]));
        v.st::<XYZW>(v.vi[6], v.vf[r.c]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        v.msub::<X>(0, v.vf[30], bc(v.vf[29][1]));
        *orient = v.last_mac();
        v.lq::<XYZW>(r.c, v.vi[1].wrapping_add(355));
    }
    if matches!(at, From::Top | From::Enter) {
        // +0x20, +0x28: a's screen offset; the new vertex's texture coordinates and colour; cl_c judged.
        v.add::<XYZW>(r.a, v.vf[r.a], v.vf[22]);
        v.lq::<XYZ>(r.st_c, v.vi[1].wrapping_add(549));
        let j = clip(v.vf[r.cl_c], v.vf[r.cl_c][3]);
        v.clip_in(j);
        v.lq::<XYZW>(r.col_c, v.vi[1].wrapping_add(452));
    }
    if matches!(at, From::Top | From::Enter | From::At448) {
        // +0x30..+0x48: d's fog at most 255; Q for b; the triangle's orientation (FSAND); the next edge; a's fog at least vf24.x.
        v.mini::<W>(r.d, v.vf[r.d], v.vf[11]);
        v.div(v.vf[23][0], v.vf[r.b][3]);
        v.itof::<0, XYZW>(r.c, v.vf[r.c]);
        v.fsand_s(1, *orient);
        v.sub::<XY>(r.e, v.vf[r.a], v.vf[r.d]);
        let end = s16(v.vi[r.ic]) <= 0;
        v.max::<W>(r.a, v.vf[r.a], bc(v.vf[24][0]));
        v.set_vi(9, v.vi[9].wrapping_add(1));
        if end {
            return Out::StripEnd;
        }
        // +0x50..+0x60: a triangle facing the culled way (vi1 = vi10) is not tested; else its judgements, and the near plane's
        // and the guard band's (ADC on d when culled or outside).
        v.itof::<2, XYZ>(r.st_c, v.vf[r.st_c]);
        let skip = s16(v.vi[1]) != s16(v.vi[10]);
        v.mul::<XYZW>(ACC, v.vf[17], bc(v.vf[r.c][0]));
        v.set_vi(1, v.fcand(0x3ffff));
        if !skip {
            v.add::<W>(r.d, v.vf[r.d], v.vf[9]);
            v.set_vi(1, v.fcand(0x20820));
        }
        // +0x68, +0x70: the new vertex's transform; its index; IBNE vi1 to the clip handler.
        v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[r.c][1]));
        let (q, l) = r.ic_at(v);
        v.ilw(r.ic, q, l);
        v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[r.c][2]));
        let outside = v.vi[1] != 0;
        st_b(v, r);
        if outside {
            return Out::Clip;
        }
    } else if at == From::At490 {
        st_b(v, r);
    }
    // +0x80, +0x88: d's screen position (FTOI4); cl_b made; a's colour stored.
    v.ftoi::<1, XYZW>(r.d, v.vf[r.d]);
    match r.s {
        1 => v.set_vi(8, v.vi[8].wrapping_add(1)),
        2 => v.set_vi(7, v.vi[7].wrapping_sub(12)),
        _ => {}
    }
    v.mul::<XYZW>(r.cl_b, v.vf[r.b], v.vf[21]);
    v.st::<XYZW>(v.vi[6], v.vf[r.col_a]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    Out::Next
}

/// +0x78: st_b times Q; st_a stored.
fn st_b(v: &mut Vu1, r: &SlotA) {
    v.mul::<XYZ>(r.st_b, v.vf[r.st_b], v.q());
    v.st::<XYZ>(v.vi[6], v.vf[r.st_a]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
}

/// A slot's restart handler (0xDE0 for slot 1, 0xC60, 0xCE0, 0xD60), from its start (`from` 0), its restart (0x10: the strip's
/// restart vertex at +483) or its group's end (0x30: the next texture group's vertex): back to the slot at 0x448, or None at the
/// strip list's end.
fn restart_a(v: &mut Vu1, r: &SlotA, from: u32, orient: &mut u32) -> Option<From> {
    let mut from = from;
    if from == 0 {
        let group = v.vi[r.ip] == 0;
        v.mul::<X>(ACC, v.vf[29], bc(v.vf[30][1]));
        v.st::<XYZW>(v.vi[6], v.vf[r.c]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        from = if group { 0x30 } else { 0x10 };
    }
    if from == 0x10 {
        v.lq::<XYZW>(r.c, v.vi[1].wrapping_add(483));
        v.msub::<X>(0, v.vf[30], bc(v.vf[29][1]));
        *orient = v.last_mac();
        v.lq::<XYZ>(r.st_c, v.vi[1].wrapping_add(677));
        v.add::<XYZW>(r.a, v.vf[r.a], v.vf[22]);
        let j = clip(v.vf[r.cl_c], v.vf[r.cl_c][3]);
        v.clip_in(j);
        v.lq::<XYZW>(r.col_c, v.vi[1].wrapping_add(580));
        return Some(From::At448);
    }
    // +0x30: the group's end: the strip list's next word back (ISW at the index's place), the next group's first vertex.
    v.add::<XYZW>(r.a, v.vf[r.a], v.vf[22]);
    let (q, l) = match r.s {
        0 => (v.vi[8].wrapping_sub(1), 3),
        1 => (v.vi[8], 0),
        2 => (v.vi[8].wrapping_sub(1), 1),
        _ => (v.vi[8].wrapping_sub(1), 2),
    };
    let w = v.vi[12] & 0xffff;
    let mut m = v.ld(q);
    m[l] = w;
    v.mem[(q & 0x3ff) as usize] = m;
    v.set_vi(14, v.vi[12].wrapping_add(v.vi[13]));
    v.set_vi(1, v.vf[10][3] & 0xffff);
    v.add::<W>(10, v.vf[10], v.vf[0]);
    v.lq::<XYZW>(r.c, v.vi[14].wrapping_add(355));
    v.lq::<XYZ>(r.st_c, v.vi[14].wrapping_add(549));
    v.msub::<X>(0, v.vf[30], bc(v.vf[29][1]));
    *orient = v.last_mac();
    v.lq::<XYZW>(r.col_c, v.vi[14].wrapping_add(452));
    let more = v.vi[12] != 0;
    let j = clip(v.vf[r.cl_c], v.vf[r.cl_c][3]);
    v.clip_in(j);
    v.ilw(12, v.vi[1], 3);
    if more { Some(From::At448) } else { None }
}

/// 0x658..0x720 (variant A, `d` vf4) and 0x1640..0x1708 (B, vf14): the full buffer's tag (NLOOP the vertices so far, EOP) and
/// kick; the other buffer's tag, and the strip's last two vertices again in it.
fn flush(v: &mut Vu1, d: usize) {
    v.set_vi(1, v.vf[29][3] & 0xffff);
    v.set_vi(15, v.vf[27][0] & 0xffff);
    v.st::<XYZW>(v.vi[6], v.vf[d]);
    v.set_vi(14, v.vi[9].wrapping_add(1));
    v.set_vi(14, v.vi[14].wrapping_add(16384));
    v.set_vi(14, v.vi[14].wrapping_add(16384));
    v.isw::<X>(14, v.vi[1]);
    v.set_vi(1, v.vf[27][1] & 0xffff);
    let f = v.vf[27];
    v.vf[27] = [f[1], f[2], f[3], f[0]];
    v.set_vi(7, 109);
    v.kick(v.vi[15]);
    v.vf[29][3] = sx(v.vi[1]);
    v.st::<XYZW>(v.vi[1], v.vf[23]);
    v.set_vi(1, v.vi[1].wrapping_add(1));
    let none = v.vi[9] == 0;
    v.lq::<XYZW>(31, v.vi[6].wrapping_sub(5));
    if !none {
        v.lq::<XYZW>(25, v.vi[6].wrapping_sub(4));
        v.lq::<XYZW>(17, v.vi[6].wrapping_sub(3));
        for r in [31, 25, 17] {
            v.st::<XYZW>(v.vi[1], v.vf[r]);
            v.set_vi(1, v.vi[1].wrapping_add(1));
        }
        v.lq::<XYZW>(17, v.vi[13].wrapping_add(653));
        v.set_vi(9, 1);
    }
    v.st::<XYZW>(v.vi[1], v.vf[12]);
    v.set_vi(1, v.vi[1].wrapping_add(1));
    v.st::<XYZW>(v.vi[1], v.vf[8]);
    v.set_vi(1, v.vi[1].wrapping_add(1));
    v.set_vi(6, v.vi[1]);
}

/// 0x728..0x770: the strip list's end: the packet's tag (NLOOP, EOP) and kick; the buffers turned. The E bit's slot.
fn strips_end(v: &mut Vu1) -> usize {
    v.set_vi(9, v.vi[9].wrapping_add(1));
    v.set_vi(1, v.vf[29][3] & 0xffff);
    v.set_vi(15, v.vf[27][0] & 0xffff);
    v.set_vi(9, v.vi[9].wrapping_add(16384));
    v.set_vi(9, v.vi[9].wrapping_add(16384));
    v.isw::<X>(9, v.vi[1]);
    let f = v.vf[27];
    v.vf[27] = [f[1], f[2], f[3], f[0]];
    v.kick(v.vi[15]);
    0x770
}

/// A slot's strip-end handler (0x778 for slot 1, 0x8B0, 0x9E8, 0xB20): a restart (ic negative) goes on with the new vertex
/// (0x490); a strip's end (0) starts the next strip in the packet: its texture setups (vi15 of them from vi11, the buffer
/// kicked first when they do not fit), the strip's tag; back at 0x498.
fn strip_end_a(v: &mut Vu1, r: &SlotA, budget: &mut u32) -> Result<From, String> {
    v.add::<W>(r.d, v.vf[r.d], v.vf[9]);
    let end = v.vi[r.ic] == 0;
    v.itof::<2, XYZ>(r.st_c, v.vf[r.st_c]);
    let (q, l) = r.ic_at(v);
    v.ilw(r.ic, q, l);
    if !end {
        v.mul::<XYZW>(ACC, v.vf[17], bc(v.vf[r.c][0]));
        v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[r.c][1]));
        v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[r.c][2]));
        return Ok(From::At490);
    }
    v.mul::<XYZW>(ACC, v.vf[17], bc(v.vf[r.c][0]));
    v.ilw(15, v.vi[11], 2);
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[r.c][1]));
    v.set_vi(1, v.vf[29][3] & 0xffff);
    v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[r.c][2]));
    v.lq::<XYZW>(31, v.vi[11]);
    v.set_vi(11, v.vi[11].wrapping_add(1));
    v.mul::<XYZ>(r.st_b, v.vf[r.st_b], v.q());
    v.set_vi(7, v.vi[7].wrapping_sub(v.vi[15]));
    let some = s16(v.vi[15]) >= 0;
    v.set_vi(7, v.vi[7].wrapping_sub(2));
    if !some {
        v.set_vi(15, v.vi[15].wrapping_add(256));
        v.set_vi(7, v.vi[7].wrapping_sub(256));
        let culling = v.vi[10] != 0;
        v.set_vi(10, v.vi[10].wrapping_sub(2));
        if !culling {
            v.set_vi(10, 2);
        }
    }
    v.ftoi::<0, Z>(31, v.vf[31]);
    if r.s == 3 {
        v.set_vi(14, v.vi[7].wrapping_add(12));
    }
    v.isw::<X>(9, v.vi[1]);
    let room = if r.s == 3 { v.vi[14] } else { v.vi[7] };
    let fits = s16(room) >= 0;
    v.set_vi(9, v.vi[9].wrapping_add(16384));
    if !fits {
        v.set_vi(9, v.vi[9].wrapping_add(16384));
        v.isw::<X>(9, v.vi[1]);
        v.set_vi(1, v.vf[27][0] & 0xffff);
        v.set_vi(6, v.vf[27][1] & 0xffff);
        let f = v.vf[27];
        v.vf[27] = [f[1], f[2], f[3], f[0]];
        v.set_vi(7, [114, 117, 120, 111][r.s]);
        v.set_vi(7, v.vi[7].wrapping_sub(v.vi[15]));
        v.kick(v.vi[1]);
    }
    setups(v, budget)?;
    st_b_after(v, r);
    Ok(From::At498)
}

/// The strip's texture setups and tag (0x850..0x898 and its kin): vf24 with NLOOP vi15, the setups from vi11, the strip's tag.
fn setups(v: &mut Vu1, budget: &mut u32) -> Result<(), String> {
    v.st::<XYZW>(v.vi[6], v.vf[24]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    v.isw::<X>(15, v.vi[6].wrapping_sub(1));
    loop {
        step(budget)?;
        v.set_vi(15, v.vi[15].wrapping_sub(1));
        v.set_vi(9, 0);
        let more = v.vi[15] != 0;
        v.st::<XYZW>(v.vi[6], v.vf[31]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        if !more {
            break;
        }
        v.lq::<XYZW>(31, v.vi[11]);
        v.set_vi(11, v.vi[11].wrapping_add(1));
    }
    v.vf[29][3] = sx(v.vi[6]);
    v.st::<XYZW>(v.vi[6], v.vf[23]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    Ok(())
}

/// The strip-end handler's last delay slot: st_a stored, all four words (SQI.xyzw; its st_b was made times Q in the handler).
fn st_b_after(v: &mut Vu1, r: &SlotA) {
    v.st::<XYZW>(v.vi[6], v.vf[r.st_a]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
}

/// A slot's clip handler (0xE60 for slot 1, 0xF80, 0x10A0, 0x11C0): the triangle (cl_b, cl_d, cl_c) against the guard band
/// (scaled by vf28): unless all three are outside one plane of it, it is clipped and kicked as a fan (`clipper`). The strip's
/// judgements made again. None: back to the slot at 0x498; Some: the run ended in the clipper (its E bit's slot).
fn clip_slot(v: &mut Vu1, r: &SlotA, budget: &mut u32) -> Result<Option<usize>, String> {
    v.mul::<XYZW>(31, v.vf[r.cl_b], v.vf[28]);
    v.isw::<Y>(2, 976);
    v.mul::<XYZW>(25, v.vf[r.cl_d], v.vf[28]);
    v.isw::<Z>(3, 976);
    v.mul::<XYZW>(17, v.vf[r.cl_c], v.vf[28]);
    v.isw::<W>(4, 976);
    v.add::<W>(r.d, v.vf[r.d], v.vf[9]);
    v.isw::<Z>(12, 977);
    for g in [31, 25, 17] {
        let j = clip(v.vf[g], v.vf[g][3]);
        v.clip_in(j);
        match g {
            25 => v.isw::<Y>(11, 977),
            17 => v.lq::<XYZW>(17, v.vi[13].wrapping_add(653)),
            _ => {}
        }
    }
    v.set_vi(13, 63);
    // FCGET three times right after three CLIPs: the first sees the last two judgements not landed, the second the last.
    let cf = v.vi[crate::vu1::CLIPF];
    v.set_vi(1, (cf >> 12) & 0xfff);
    v.set_vi(14, (cf >> 6) & 0xfff);
    v.set_vi(15, cf & 0xfff);
    v.set_vi(1, v.vi[1] & v.vi[14]);
    for (g, n) in [(r.cl_b, 15usize), (r.cl_d, 13), (r.cl_c, 0)] {
        let j = clip(v.vf[g], v.vf[g][3]);
        v.clip_in(j);
        if n != 0 {
            v.set_vi(1, v.vi[1] & v.vi[n]);
        } else {
            v.ilw(13, 976, 0);
        }
    }
    let rejected = v.vi[1] != 0;
    let cf = v.vi[crate::vu1::CLIPF];
    v.set_vi(1, (cf >> 12) & 0xfff);
    if rejected {
        return Ok(None);
    }
    v.set_vi(14, (cf >> 6) & 0xfff);
    v.set_vi(13, cf & 0xfff);
    v.set_vi(11, if r.s == 3 { 12 } else { 0 });
    v.set_vi(12, [111, 114, 117, 108][r.s]);
    // The triangle's three strip indices (as the slots before loaded them).
    let at: [(u32, usize); 3] = match r.s {
        0 => [(2, 2), (2, 3), (1, 0)],
        1 => [(2, 3), (1, 0), (1, 1)],
        2 => [(2, 0), (2, 1), (2, 2)],
        _ => [(2, 1), (2, 2), (2, 3)],
    };
    for (k, (back, l)) in at.into_iter().enumerate() {
        v.ilw(2 + k, v.vi[8].wrapping_sub(back), l);
    }
    v.st::<XYZW>(979, v.vf[r.cl_b]);
    v.ftoi::<1, XYZW>(18, v.vf[r.d]);
    v.st::<XYZW>(984, v.vf[r.cl_d]);
    v.st::<XYZW>(989, v.vf[r.cl_c]);
    // vf12.w: the triangle's orientation in the strip (0 or 1).
    if r.s % 2 == 0 {
        v.mul::<W>(12, v.vf[0], bc(v.vf[0][0]));
    } else {
        v.add::<W>(12, v.vf[0], bc(v.vf[0][0]));
    }
    v.st::<XYZW>(991, v.vf[r.col_a]);
    v.st::<XYZW>(992, v.vf[r.st_a]);
    // BAL vi15 to 0x1CD0, its delay slot SQ vf18 to 993.
    let ret = [491u32, 527, 563, 599][r.s];
    v.set_vi(15, ret);
    v.st::<XYZW>(993, v.vf[18]);
    if let Some(end) = clipper(v, budget)? {
        return Ok(Some(end));
    }
    if v.vi[15] != ret {
        return Err(format!("VU1 program 0: the clipper returned to {:#x}, not {:#x}", v.vi[15] * 8, ret * 8));
    }
    // The return (0xF58 and its kin): c's transform made again, Q for b; the judgements; B to 0x498.
    v.mul::<XYZW>(ACC, v.vf[17], bc(v.vf[r.c][0]));
    v.div(v.vf[23][0], v.vf[r.b][3]);
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[r.c][1]));
    v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[r.c][2]));
    let j = clip(v.vf[r.cl_d], v.vf[r.cl_d][3]);
    v.clip_in(j);
    let j = clip(v.vf[r.cl_c], v.vf[r.cl_c][3]);
    v.clip_in(j);
    // SQI.xyzw (the slot's own store of st_a is SQI.xyz).
    v.st::<XYZW>(v.vi[6], v.vf[r.st_a]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    Ok(None)
}

/// The clipper (0x1CD0..0x27D0, BAL'd with the triangle at 979.., 984.., 989.., its strip indices in vi2..vi4, the buffer
/// tags' and the cull flag's registers): the registers saved; the triangle's corners (clip position, ST, colour, the
/// judgement's flags) from the vertex arrays; culled if it faces the culled way; else clipped against each plane its corners
/// are outside (the guard band's vf17.x bits, halved each plane), each edge through a plane routine (`plane`), the crossing
/// refined until it is inside; the polygon kicked as a fan in the packet (the buffer kicked first when it does not fit); the
/// registers back. Some: the run ended (an edge leaving a plane: the jump to address 0).
fn clipper(v: &mut Vu1, budget: &mut u32) -> Result<Option<usize>, String> {
    v.isw::<X>(15, 977);
    v.isw::<W>(5, 977);
    v.isw::<X>(10, 978);
    v.isw::<Y>(6, 978);
    v.isw::<Z>(11, 978);
    v.isw::<W>(12, 978);
    for (k, r) in [1usize, 2, 3, 4].into_iter().enumerate() {
        v.st::<XYZW>(968 + k as u32, v.vf[r]);
    }
    v.ilw(5, 976, 0);
    for (k, r) in [13usize, 14, 15, 16].into_iter().enumerate() {
        v.st::<XYZW>(972 + k as u32, v.vf[r]);
    }
    v.lq::<XYZW>(28, v.vi[5].wrapping_add(651));
    v.lq::<XYZW>(4, v.vi[6].wrapping_sub(7));
    v.lq::<XYZW>(21, v.vi[6].wrapping_sub(4));
    v.lq::<XYZW>(13, v.vi[6].wrapping_sub(5));
    v.st::<XYZW>(982, v.vf[4]);
    // The corners' texture coordinates: each index's array (positive: +549; a restart: +128 more).
    let pos2 = s16(v.vi[2]) > 0;
    v.lq::<XYZW>(24, v.vi[6].wrapping_sub(2));
    if !pos2 {
        v.set_vi(2, v.vi[2].wrapping_add(128));
    }
    v.set_vi(2, v.vi[2].wrapping_add(v.vi[5]));
    let pos3 = s16(v.vi[3]) > 0;
    v.lq::<XYZW>(2, v.vi[2].wrapping_add(549));
    if !pos3 {
        v.set_vi(3, v.vi[3].wrapping_add(128));
    }
    v.set_vi(3, v.vi[3].wrapping_add(v.vi[5]));
    let pos4 = s16(v.vi[4]) > 0;
    v.lq::<XYZW>(19, v.vi[3].wrapping_add(549));
    if !pos4 {
        v.set_vi(4, v.vi[4].wrapping_add(128));
    }
    v.set_vi(4, v.vi[4].wrapping_add(v.vi[5]));
    v.lq::<XYZW>(31, v.vi[4].wrapping_add(549));
    // The corners' colours (the packet's integers) as floats.
    let mfir4 = |v: &mut Vu1, d: usize| {
        let (a, b, c, e) = (sx(v.vi[15]), sx(v.vi[4]), sx(v.vi[5]), sx(v.vi[3]));
        v.vf[d] = [a, b, c, e];
    };
    for (r, l) in [(15usize, 0usize), (4, 1), (5, 2), (3, 3)] {
        v.ilw(r, 991, l);
    }
    mfir4(v, 3);
    for (r, l) in [(15usize, 0usize), (4, 1), (5, 2)] {
        v.ilw(r, v.vi[6].wrapping_sub(3), l);
    }
    v.itof::<0, XYZW>(3, v.vf[3]);
    v.ilw(3, v.vi[6].wrapping_sub(3), 3);
    mfir4(v, 20);
    v.st::<XYZW>(991, v.vf[3]);
    v.ilw(15, v.vi[6].wrapping_sub(6), 0);
    v.ilw(4, v.vi[6].wrapping_sub(6), 1);
    v.itof::<0, XYZW>(20, v.vf[20]);
    v.ilw(5, v.vi[6].wrapping_sub(6), 2);
    v.ilw(3, v.vi[6].wrapping_sub(6), 3);
    mfir4(v, 3);
    v.itof::<2, XYZW>(2, v.vf[2]);
    v.set_vi(5, v.vf[13][3] & 0xffff);
    v.set_vi(4, v.vf[24][3] & 0xffff);
    v.ilw(3, 993, 3);
    v.itof::<2, XYZW>(19, v.vf[19]);
    v.st::<XYZW>(987, v.vf[21]);
    v.itof::<0, XYZW>(3, v.vf[3]);
    v.set_vi(10, 4080);
    v.itof::<2, XYZW>(31, v.vf[31]);
    v.st::<XYZ>(980, v.vf[2]);
    v.set_vi(5, v.vi[5] & v.vi[10]);
    v.st::<XYZ>(985, v.vf[19]);
    v.vf[13][3] = sx(v.vi[5]);
    v.st::<XYZ>(990, v.vf[31]);
    v.set_vi(4, v.vi[4] & v.vi[10]);
    v.vf[24][3] = sx(v.vi[4]);
    v.set_vi(3, v.vi[3] & v.vi[10]);
    v.isw::<W>(3, 993);
    v.vf[25][3] = sx(v.vi[3]);
    v.st::<XYZW>(981, v.vf[3]);
    v.st::<XYZW>(983, v.vf[13]);
    v.itof::<1, XYZW>(13, v.vf[13]);
    v.st::<XYZW>(986, v.vf[20]);
    v.st::<XYZW>(988, v.vf[24]);
    v.itof::<1, XYZW>(24, v.vf[24]);
    v.set_vi(11, 32);
    v.itof::<1, W>(25, v.vf[25]);
    v.set_vi(10, v.vi[1] & v.vi[11]);
    v.st::<W>(980, v.vf[13]);
    v.st::<W>(985, v.vf[24]);
    // The corners' clip flags with the two of the guard band each one's judgement does not tell (`flags`).
    let near1 = v.vi[10] == 0;
    v.st::<W>(990, v.vf[25]);
    if !near1 {
        v.set_vi(15, 1007);
        v.set_vi(10, 3);
        flags(v);
    }
    v.set_vi(10, v.vi[14] & v.vi[11]);
    v.isw::<W>(1, 982);
    v.set_vi(2, v.vi[1]);
    let near2 = v.vi[10] == 0;
    v.set_vi(1, v.vi[14]);
    if !near2 {
        v.set_vi(15, 1014);
        v.set_vi(10, 3);
        flags(v);
    }
    v.set_vi(10, v.vi[13] & v.vi[11]);
    v.isw::<W>(1, 987);
    v.set_vi(2, v.vi[2] | v.vi[1]);
    let near3 = v.vi[10] == 0;
    v.set_vi(1, v.vi[13]);
    if !near3 {
        v.set_vi(15, 1034);
        v.set_vi(10, 3);
        flags(v);
    }
    // 0x2050..0x20B0: the planes' bits (vf17.x 32.5, halved each plane; vf17.y 0.01, the crossing's margin; vf29.z 0.5), the
    // planes the corners are outside (vi2: 512 + those of 63), vi3 the first plane's routine.
    v.mul::<W>(17, v.vf[0], bc(v.vf[0][0]));
    v.vi[I] = 0x4202_0000;
    v.isw::<W>(1, 992);
    v.set_vi(2, v.vi[2] | v.vi[1]);
    v.set_vi(1, 63);
    v.add::<X>(17, v.vf[0], v.i());
    v.vi[I] = 0x3c23_d70a;
    v.set_vi(2, v.vi[2] & v.vi[1]);
    v.set_vi(2, v.vi[2].wrapping_add(512));
    v.set_vi(1, 32);
    v.add::<Y>(17, v.vf[0], v.i());
    v.vi[I] = 0x3f00_0000;
    v.set_vi(1, v.vi[2] & v.vi[1]);
    v.set_vi(3, 1275);
    let not_near = v.vi[1] == 0;
    v.add::<Z>(29, v.vf[0], v.i());
    v.set_vi(13, 3);
    if !not_near {
        // 0x20B8..0x2100: the cull flag (978.x): none, or a triangle outside the near plane tested for its orientation.
        v.ilw(14, 978, 0);
        let none = v.vi[14] == 0;
        v.set_vi(14, v.vi[14].wrapping_sub(2));
        let test = none || v.vi[14] == 0;
        if test && cull(v) {
            v.set_vi(6, v.vi[6].wrapping_sub(1));
            return Ok(restore(v));
        }
    }
    // 0x21E8..0x2580: each plane.
    loop {
        step(budget)?;
        v.ftoi::<0, X>(31, v.vf[17]);
        v.set_vi(1, 32);
        v.set_vi(1, v.vi[1] & v.vi[2]);
        v.set_vi(2, v.vi[2].wrapping_add(v.vi[2]));
        let inside = v.vi[1] == 0;
        v.set_vi(11, v.vf[31][0] & 0xffff);
        if !inside {
            let empty = v.vi[13] == 0;
            v.set_vi(4, 989);
            if empty {
                break;
            }
            if let Some(end) = clip_plane(v, budget)? {
                return Ok(Some(end));
            }
        }
        // 0x2578: the next plane's bit; IBGEZ vi2, its delay slot vi3 += 10.
        v.mul::<X>(17, v.vf[17], bc(v.vf[29][2]));
        let more = s16(v.vi[2]) >= 0;
        v.set_vi(3, v.vi[3].wrapping_add(10));
        if !more {
            break;
        }
    }
    // 0x2588: no corner left, nothing drawn; else the fan.
    let empty = v.vi[13] == 0;
    v.set_vi(6, v.vi[6].wrapping_sub(1));
    if !empty {
        fan(v, budget)?;
    }
    Ok(restore(v))
}

/// The subroutine at 0x1FE8 (`flags`): vi1 gets bit 0 or 1 (3) when it has neither, bit 2 or 3 (12) when it has neither.
fn flags(v: &mut Vu1) {
    v.set_vi(10, v.vi[1] & v.vi[10]);
    if v.vi[10] == 0 {
        v.set_vi(1, v.vi[1].wrapping_add(3));
    }
    v.set_vi(10, 12);
    v.set_vi(10, v.vi[1] & v.vi[10]);
    if v.vi[10] == 0 {
        v.set_vi(1, v.vi[1].wrapping_add(12));
    }
}

/// 0x2108..0x21D8: the triangle's orientation from its corners' screen positions (FSAND of the cross product's z); true when it
/// faces the culled way (the run's flag in vf12.w and 978.x).
fn cull(v: &mut Vu1) -> bool {
    v.ftoi::<1, W>(12, v.vf[12]);
    v.lq::<XYZW>(31, 979);
    v.lq::<XYZW>(25, 984);
    v.lq::<XYZW>(14, 989);
    for r in [31, 25, 14] {
        v.vf[r][2] = v.vf[r][3];
    }
    v.sub::<XYZW>(25, v.vf[25], v.vf[31]);
    v.sub::<XYZW>(14, v.vf[14], v.vf[31]);
    v.opmula(v.vf[25], v.vf[14]);
    v.opmsub(25, v.vf[14], v.vf[25]);
    v.mul::<XYZW>(31, v.vf[31], v.vf[25]);
    v.add::<X>(31, v.vf[31], bc(v.vf[31][1]));
    v.ilw(14, 978, 0);
    v.add::<X>(31, v.vf[31], bc(v.vf[31][2]));
    v.set_vi(10, v.vf[12][3] & 0xffff);
    v.fsand_s(1, v.last_mac());
    if v.vi[10] != 0 {
        let flag = v.vi[14] != 0;
        v.set_vi(14, 0);
        if !flag {
            v.set_vi(14, 2);
        }
    }
    s16(v.vi[1]) == s16(v.vi[14])
}

/// One plane (0x2210..0x2570): the polygon at 979 (vi13 corners, five quadwords each: position, ST, colour, the screen
/// position's and the flags' quadwords) with its first corner again after its last, each edge from P (vi4 - 10) to C (vi4 - 5):
/// C kept when inside, the crossing (refined until inside) when the edge enters; an edge that leaves ends the run. Some: that.
fn clip_plane(v: &mut Vu1, budget: &mut u32) -> Result<Option<usize>, String> {
    v.set_vi(5, 979);
    v.set_vi(1, v.vi[13].wrapping_add(v.vi[13]));
    v.set_vi(1, v.vi[1].wrapping_add(v.vi[1]));
    v.set_vi(1, v.vi[1].wrapping_add(v.vi[13]));
    for (k, r) in [1usize, 2, 3, 15, 16].into_iter().enumerate() {
        v.lq::<XYZW>(r, 979 + k as u32);
    }
    for (k, r) in [1usize, 2, 3, 15, 16].into_iter().enumerate() {
        v.st::<XYZW>(v.vi[1].wrapping_add(979 + k as u32), v.vf[r]);
    }
    v.set_vi(12, v.vi[13].wrapping_sub(1));
    v.set_vi(13, 0);
    loop {
        step(budget)?;
        v.ilw(1, v.vi[4].wrapping_sub(7), 3);
        v.ilw(14, v.vi[4].wrapping_sub(2), 3);
        v.lq::<XYZW>(1, v.vi[4].wrapping_sub(10));
        v.lq::<XYZW>(18, v.vi[4].wrapping_sub(5));
        v.set_vi(1, v.vi[1] & v.vi[11]);
        v.set_vi(14, v.vi[14] & v.vi[11]);
        let p_in = v.vi[1] == 0;
        v.lq::<XYZW>(19, v.vi[4].wrapping_sub(4));
        if !p_in {
            let c_out = v.vi[14] != 0;
            v.lq::<XYZW>(20, v.vi[4].wrapping_sub(3));
            if !c_out {
                // Entering: JALR vi15 through vi3 to the plane's routine, its delay slot LQ vf2.
                v.sub::<XYZW>(14, v.vf[18], v.vf[1]);
                v.set_vi(15, 1120);
                v.lq::<XYZW>(2, v.vi[4].wrapping_sub(9));
                plane(v)?;
                crossing(v, budget)?;
            }
        } else {
            let c_out = v.vi[14] != 0;
            v.lq::<XYZW>(20, v.vi[4].wrapping_sub(3));
            if c_out {
                // Leaving: IADDIU vi1 = vi3 + 5, then JALR through vi1, which takes vi1's value from before (0, the IBEQ that
                // brought the edge here): the program's start, B to 0x218, whose E bit ends the run.
                let old = v.vi[1];
                v.set_vi(1, v.vi[3].wrapping_add(5));
                v.sub::<XYZW>(14, v.vf[1], v.vf[18]);
                v.set_vi(15, 1168);
                v.lq::<XYZW>(2, v.vi[4].wrapping_sub(9));
                if old != 0 {
                    return Err(format!("VU1 program 0: the clipper's JALR to {:#x}", old * 8));
                }
                return Ok(Some(buffers(v)));
            }
            v.lq::<XYZW>(21, v.vi[4].wrapping_sub(2));
            v.lq::<XYZW>(24, v.vi[4].wrapping_sub(1));
            corner(v);
        }
        // 0x2560: the next edge.
        v.set_vi(4, v.vi[4].wrapping_add(5));
        let more = v.vi[12] != 0;
        v.set_vi(12, v.vi[12].wrapping_sub(1));
        if !more {
            return Ok(None);
        }
    }
}

/// C's five quadwords (vf18, vf19, vf20, vf21, vf24) onto the output, vi13 one more (0x2430..0x2460 and 0x23D8..0x2408).
fn corner(v: &mut Vu1) {
    for r in [18, 19, 20, 21, 24] {
        v.st::<XYZW>(v.vi[5], v.vf[r]);
        v.set_vi(5, v.vi[5].wrapping_add(1));
    }
    v.set_vi(13, v.vi[13].wrapping_add(1));
}

/// The plane routine vi3 names (0x27D8 + 0x50 k for the plane k: -z, +z, -y, +y, -x, +x; JALR'd at 0x22F0, its JR back to
/// 0x2300 with its delay slot): vf25 the distances (P's and C's to the plane, w their difference), vf31.w P's, vf15 the
/// colour's step; one more output corner (vi5 += 5, vi13 += 1).
fn plane(v: &mut Vu1) -> Result<(), String> {
    let k = match v.vi[3] {
        1275 => 0,
        1285 => 1,
        1295 => 2,
        1305 => 3,
        1315 => 4,
        1325 => 5,
        n => return Err(format!("VU1 program 0: the clipper's JALR to {:#x}", n * 8)),
    };
    let lane = [2, 2, 1, 1, 0, 0][k];
    let (p, c) = (v.vf[1], v.vf[18]);
    // +0: the distances' lane and w (C - P, or P - C for the far side).
    match (k, lane) {
        (0, _) => v.sub::<ZW>(25, c, p),
        (1, _) => v.sub::<ZW>(25, p, c),
        (2, _) => v.sub::<YW>(25, c, p),
        (3, _) => v.sub::<YW>(25, p, c),
        (4, _) => v.sub::<XW>(25, c, p),
        _ => v.sub::<XW>(25, p, c),
    }
    v.lq::<XYZW>(3, v.vi[4].wrapping_sub(8));
    // +8, +16: vf31.w = 0 - P.w - P.lane (near) or P.w - P.lane (far).
    if k % 2 == 0 {
        v.sub::<W>(ACC, v.vf[17], v.vf[1]);
        v.lq::<XYZW>(21, v.vi[4].wrapping_sub(2));
        v.msub::<W>(31, v.vf[0], bc(v.vf[1][lane]));
        v.set_vi(5, v.vi[5].wrapping_add(5));
    } else {
        v.sub::<W>(31, v.vf[1], bc(v.vf[1][lane]));
        v.lq::<XYZW>(21, v.vi[4].wrapping_sub(2));
        v.set_vi(5, v.vi[5].wrapping_add(5));
    }
    // +24, +32 (the JR and its delay slot).
    v.sub::<XYW>(15, v.vf[19], v.vf[2]);
    if k % 2 == 0 {
        v.add::<W>(25, v.vf[25], bc(v.vf[25][lane]));
    } else {
        v.sub::<W>(25, v.vf[25], bc(v.vf[25][lane]));
    }
    v.set_vi(13, v.vi[13].wrapping_add(1));
    Ok(())
}

/// 0x2300..0x2408: the entering edge's crossing (P + (C - P) t, t = vf31.w / vf25.w, its ST, colour and screen position), made
/// again a margin further in (vf17.y) while it is still outside; stored at vi5 - 5.., then C after it unless it was the last.
fn crossing(v: &mut Vu1, budget: &mut u32) -> Result<(), String> {
    loop {
        step(budget)?;
        v.mul::<XYZW>(ACC, v.vf[1], v.vf[28]);
        v.div(v.vf[31][3], v.vf[25][3]);
        v.mul::<XYZW>(13, v.vf[14], v.vf[28]);
        v.lq::<XYZW>(2, v.vi[4].wrapping_sub(9));
        v.sub::<XYZW>(16, v.vf[20], v.vf[3]);
        v.lq::<XYZW>(3, v.vi[4].wrapping_sub(8));
        v.add::<W>(2, v.vf[2], bc(v.vf[17][1]));
        v.lq::<XYZW>(24, v.vi[4].wrapping_sub(1));
        v.add::<XYZW>(3, v.vf[3], bc(v.vf[17][1]));
        v.madd::<XYZW>(13, v.vf[13], v.q());
        v.add::<XYZW>(ACC, v.vf[1], bc(v.vf[0][0]));
        v.madd::<XYZW>(1, v.vf[14], v.q());
        v.mul::<W>(31, v.vf[0], v.q());
        v.add::<XYW>(ACC, v.vf[2], bc(v.vf[0][0]));
        // FSAND at 0x2360 reads this ADDA's flags (the two results after it have not landed).
        let f_acc = v.last_mac();
        v.madd::<XYW>(2, v.vf[15], v.q());
        v.mul::<XYZW>(16, v.vf[16], v.q());
        v.div(v.vf[23][0], v.vf[13][3]);
        v.sub::<W>(25, v.vf[0], v.vf[31]);
        let f_sub = v.last_mac();
        v.fsand_s(1, f_acc);
        v.max::<W>(31, v.vf[31], bc(v.vf[0][0]));
        v.st::<XYZW>(v.vi[5].wrapping_sub(5), v.vf[1]);
        let j = clip(v.vf[1], v.vf[1][3]);
        v.clip_in(j);
        v.lq::<XYZW>(1, v.vi[4].wrapping_sub(10));
        v.add::<XYZW>(3, v.vf[3], v.vf[16]);
        v.st::<XYZW>(v.vi[5].wrapping_sub(4), v.vf[2]);
        // FSAND at 0x2380: the SUB.w's flags.
        v.fsand_s(14, f_sub);
        v.add::<W>(13, v.vf[2], bc(v.vf[0][0]));
        v.mul::<XYZ>(13, v.vf[13], v.q());
        v.set_vi(10, v.vi[crate::vu1::CLIPF] & 0xfff);
        v.mini::<W>(31, v.vf[31], bc(v.vf[0][3]));
        v.set_vi(1, v.vi[1] | v.vi[14]);
        v.mul::<XYZW>(4, v.vf[2], v.q());
        v.st::<XYZW>(v.vi[5].wrapping_sub(3), v.vf[3]);
        v.isw::<W>(10, v.vi[5].wrapping_sub(2));
        v.add::<XYZ>(13, v.vf[13], v.vf[22]);
        v.vf[25][3] = v.vf[0][3];
        v.ftoi::<1, XYZW>(13, v.vf[13]);
        let again = v.vi[1] != 0;
        v.st::<XYZ>(v.vi[5].wrapping_sub(2), v.vf[4]);
        if !again {
            break;
        }
    }
    let last = s16(v.vi[4]) == s16(v.vi[5]);
    v.st::<XYZW>(v.vi[5].wrapping_sub(1), v.vf[13]);
    if !last {
        for r in [18, 19, 20, 21, 24] {
            v.st::<XYZW>(v.vi[5], v.vf[r]);
            v.set_vi(5, v.vi[5].wrapping_add(1));
        }
        v.set_vi(13, v.vi[13].wrapping_add(1));
    }
    Ok(())
}

/// 0x2598..0x2708: the clipped polygon as a fan in the packet: the packet's tag ended and kicked when the fan does not fit
/// (vi7, the room, less 5 and three a corner, with 978.z); the fan's tag (vf23 turned: its PRIM a fan), each corner's ST,
/// colour (FTOI0) and screen position; the strip's tag again with its last vertex's three quadwords.
fn fan(v: &mut Vu1, budget: &mut u32) -> Result<(), String> {
    v.ilw(10, 978, 2);
    v.set_vi(1, v.vf[29][3] & 0xffff);
    v.set_vi(15, v.vi[13].wrapping_add(v.vi[13]));
    v.set_vi(15, v.vi[15].wrapping_add(v.vi[13]));
    v.set_vi(7, v.vi[7].wrapping_sub(5));
    v.set_vi(7, v.vi[7].wrapping_sub(v.vi[15]));
    v.set_vi(14, v.vi[7].wrapping_add(v.vi[10]));
    v.isw::<X>(9, v.vi[1]);
    let fits = s16(v.vi[14]) >= 0;
    v.set_vi(9, v.vi[9].wrapping_add(16384));
    if !fits {
        v.set_vi(9, v.vi[9].wrapping_add(16384));
        v.isw::<X>(9, v.vi[1]);
        v.ilw(7, 978, 3);
        v.set_vi(1, v.vf[27][0] & 0xffff);
        v.set_vi(6, v.vf[27][1] & 0xffff);
        let f = v.vf[27];
        v.vf[27] = [f[1], f[2], f[3], f[0]];
        v.set_vi(7, v.vi[7].wrapping_sub(v.vi[15]));
        v.kick(v.vi[1]);
    }
    let t = v.vf[23];
    v.vf[31] = [t[1], t[2], t[3], t[0]];
    v.st::<XYZW>(v.vi[6], v.vf[23]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    v.isw::<X>(13, v.vi[6].wrapping_sub(1));
    v.set_vi(1, 979);
    let t = v.vf[31];
    v.vf[31] = [t[1], t[2], t[3], t[0]];
    v.lq::<XYZW>(3, v.vi[1].wrapping_add(2));
    v.ilw(14, 978, 1);
    // B to 0x2688, its delay slot SQ.y vf31 (the fan's PRIM) into the tag.
    v.st::<Y>(v.vi[6].wrapping_sub(1), v.vf[31]);
    let mut first = true;
    loop {
        step(budget)?;
        if !first {
            v.st::<XYZW>(v.vi[6], v.vf[3]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.lq::<XYZW>(3, v.vi[1].wrapping_add(2));
            v.st::<XYZW>(v.vi[6], v.vf[13]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
        }
        first = false;
        v.lq::<XYZW>(4, v.vi[1].wrapping_add(3));
        v.lq::<XYZW>(13, v.vi[1].wrapping_add(4));
        v.ftoi::<0, XYZW>(3, v.vf[3]);
        v.set_vi(13, v.vi[13].wrapping_sub(1));
        v.set_vi(1, v.vi[1].wrapping_add(5));
        let more = v.vi[13] != 0;
        v.st::<XYZW>(v.vi[6], v.vf[4]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        if !more {
            break;
        }
    }
    v.st::<XYZW>(v.vi[6], v.vf[3]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    v.st::<XYZW>(v.vi[6], v.vf[13]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    v.lq::<XYZW>(31, v.vi[14].wrapping_sub(4));
    v.lq::<XYZW>(25, v.vi[14].wrapping_sub(3));
    v.lq::<XYZW>(24, v.vi[14].wrapping_sub(2));
    v.vf[29][3] = sx(v.vi[6]);
    for r in [23, 31, 25, 24] {
        v.st::<XYZW>(v.vi[6], v.vf[r]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
    }
    v.set_vi(9, 1);
    Ok(())
}

/// 0x2710..0x27D0: the registers the clipper saved, back; JR vi15 (the caller checks it). None: the run goes on.
fn restore(v: &mut Vu1) -> Option<usize> {
    for (r, l) in [(13usize, 0usize), (2, 1), (3, 2), (4, 3)] {
        v.ilw(r, 976, l);
    }
    for (r, l) in [(15usize, 0usize), (11, 1), (12, 2), (5, 3)] {
        v.ilw(r, 977, l);
    }
    v.ilw(10, 978, 0);
    for (r, o) in [(21usize, 650u32), (24, 647), (28, 649), (17, 653), (18, 654), (19, 655), (20, 656)] {
        v.lq::<XYZW>(r, v.vi[13].wrapping_add(o));
    }
    for (k, r) in [1usize, 2, 3, 4, 13, 14, 15, 16].into_iter().enumerate() {
        v.lq::<XYZW>(r, 968 + k as u32);
    }
    None
}

/// The labels of variant B's loop (0x14A8..0x1638): where its handlers come back.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Lb {
    L14a8,
    L14c8,
    L14d8,
    L14f8,
    L1500,
    L1540,
    L1558,
    L1560,
    L15a0,
    L15b8,
    L15c8,
    L1608,
    L1620,
    L1630,
}

/// Variant B (0x12E0..0x1CC8): the same packets as A without culling or clipping, the fog the run's (vf30.w), its loop
/// scheduled by hand (its four slots not alike).
fn mesh_b(v: &mut Vu1, budget: &mut u32) -> Result<usize, String> {
    head(v);
    v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
    v.set_vi(1, v.vi[3].wrapping_add(v.vi[13]));
    v.madd::<XYZW>(ACC, v.vf[17], bc(v.vf[1][0]));
    v.lq::<XYZW>(2, v.vi[1].wrapping_add(483));
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[1][1]));
    v.lq::<XYZ>(10, v.vi[1].wrapping_add(677));
    v.madd::<XYZW>(1, v.vf[19], bc(v.vf[1][2]));
    v.lq::<XYZW>(23, v.vi[13].wrapping_add(648));
    v.lq::<XYZW>(6, v.vi[1].wrapping_add(580));
    v.itof::<0, XYZW>(2, v.vf[2]);
    v.set_vi(1, v.vi[4].wrapping_add(v.vi[13]));
    v.itof::<2, XYZ>(10, v.vf[10]);
    v.lq::<XYZW>(3, v.vi[1].wrapping_add(355));
    v.div(v.vf[23][0], v.vf[1][3]);
    v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
    v.lq::<XYZ>(11, v.vi[1].wrapping_add(549));
    v.madd::<XYZW>(ACC, v.vf[17], bc(v.vf[2][0]));
    v.lq::<XYZW>(21, v.vi[13].wrapping_add(650));
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[2][1]));
    v.lq::<XYZW>(7, v.vi[1].wrapping_add(452));
    v.madd::<XYZW>(2, v.vf[19], bc(v.vf[2][2]));
    v.lq::<XYZW>(22, v.vi[13].wrapping_add(652));
    v.itof::<0, XYZW>(3, v.vf[3]);
    v.isw::<X>(13, 976);
    v.mul::<XYZW>(13, v.vf[1], v.vf[21]);
    v.vi[I] = 0x453f_f000;
    v.mul::<XYZ>(1, v.vf[1], v.q());
    v.lq::<XYZW>(24, v.vi[13].wrapping_add(647));
    v.mul::<XYZ>(9, v.vf[9], v.q());
    v.div(v.vf[23][0], v.vf[2][3]);
    v.itof::<2, XYZ>(11, v.vf[11]);
    v.set_vi(6, v.vf[27][0] & 0xffff);
    v.add::<W>(9, v.vf[0], v.i());
    v.vi[I] = 0x437e_0000;
    v.add::<XYZ>(1, v.vf[1], v.vf[22]);
    v.lq::<XYZW>(28, v.vi[13].wrapping_add(649));
    v.add::<W>(30, v.vf[20], v.vf[22]);
    v.ilw(10, v.vi[8].wrapping_sub(2), 0);
    v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
    v.vf[29][3] = sx(v.vi[6]);
    v.add::<W>(11, v.vf[0], v.i());
    v.st::<XYZW>(v.vi[6], v.vf[23]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[3][2]));
    v.ilw(5, v.vi[8].wrapping_sub(1), 3);
    v.max::<W>(30, v.vf[30], bc(v.vf[24][0]));
    v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[3][1]));
    v.set_vi(9, 0u32.wrapping_sub(1));
    v.madd::<XYZW>(3, v.vf[17], bc(v.vf[3][0]));
    v.mul::<XYZ>(2, v.vf[2], v.q());
    let group = v.vi[5] == 0;
    v.mini::<W>(30, v.vf[30], v.vf[11]);
    v.set_vi(1, v.vi[5].wrapping_add(v.vi[13]));
    let mut at = if group {
        h1b(v, 1, 0x30)
    } else {
        v.mul::<W>(1, v.vf[30], v.vf[0]);
        let restart = s16(v.vi[5]) < 0;
        v.mul::<XYZ>(10, v.vf[10], v.q());
        if restart {
            h1b(v, 1, 0x10)
        } else {
            v.lq::<XYZW>(4, v.vi[1].wrapping_add(355));
            Some(Lb::L14c8)
        }
    };
    loop {
        step(budget)?;
        let Some(l) = at else {
            return Ok(strips_end(v));
        };
        at = b_from(v, l, budget)?;
    }
}

/// Variant B's loop from label `l` to the next label (or a handler's way back).
fn b_from(v: &mut Vu1, l: Lb, budget: &mut u32) -> Result<Option<Lb>, String> {
    Ok(match l {
        Lb::L14a8 => {
            v.madd::<XYZW>(3, v.vf[17], bc(v.vf[3][0]));
            let restart = s16(v.vi[5]) <= 0;
            v.mul::<XYZ>(2, v.vf[2], v.q());
            v.set_vi(1, v.vi[5].wrapping_add(v.vi[13]));
            if restart {
                return Ok(h1b(v, 1, 0));
            }
            v.mul::<W>(1, v.vf[30], v.vf[0]);
            v.lq::<XYZW>(4, v.vi[1].wrapping_add(355));
            v.mul::<XYZ>(10, v.vf[10], v.q());
            v.st::<XYZW>(v.vi[6], v.vf[14]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            Some(Lb::L14c8)
        }
        Lb::L14c8 => {
            v.lq::<XYZ>(12, v.vi[1].wrapping_add(549));
            v.add::<XYZ>(2, v.vf[2], v.vf[22]);
            v.lq::<XYZW>(8, v.vi[1].wrapping_add(452));
            Some(Lb::L14d8)
        }
        Lb::L14d8 => {
            v.itof::<0, XYZW>(4, v.vf[4]);
            v.lq::<XYZW>(26, v.vi[8]);
            v.set_vi(8, v.vi[8].wrapping_add(1));
            v.div(v.vf[23][0], v.vf[3][3]);
            v.itof::<2, XYZ>(12, v.vf[12]);
            let end = s16(v.vi[2]) <= 0;
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.set_vi(9, v.vi[9].wrapping_add(1));
            if end {
                return h2b(v, 1, budget);
            }
            Some(Lb::L14f8)
        }
        Lb::L14f8 => {
            v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[4][2]));
            v.st::<XYZ>(v.vi[6], v.vf[9]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            Some(Lb::L1500)
        }
        Lb::L1500 => {
            v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[4][1]));
            v.set_vi(2, v.vf[26][0] & 0xffff);
            v.ftoi::<1, XYZW>(14, v.vf[1]);
            v.st::<XYZW>(v.vi[6], v.vf[5]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.madd::<XYZW>(4, v.vf[17], bc(v.vf[4][0]));
            let restart = s16(v.vi[2]) <= 0;
            v.mul::<XYZ>(3, v.vf[3], v.q());
            v.set_vi(1, v.vi[2].wrapping_add(v.vi[13]));
            if restart {
                return Ok(h1b(v, 2, 0));
            }
            v.mul::<W>(2, v.vf[30], v.vf[0]);
            v.lq::<XYZW>(1, v.vi[1].wrapping_add(355));
            v.lq::<XYZ>(9, v.vi[1].wrapping_add(549));
            v.mul::<XYZ>(11, v.vf[11], v.q());
            v.st::<XYZW>(v.vi[6], v.vf[14]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.add::<XYZ>(3, v.vf[3], v.vf[22]);
            v.lq::<XYZW>(5, v.vi[1].wrapping_add(452));
            Some(Lb::L1540)
        }
        Lb::L1540 => {
            v.itof::<0, XYZW>(1, v.vf[1]);
            let end = s16(v.vi[3]) <= 0;
            v.itof::<2, XYZ>(9, v.vf[9]);
            v.div(v.vf[23][0], v.vf[4][3]);
            if end {
                return h2b(v, 2, budget);
            }
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.set_vi(9, v.vi[9].wrapping_add(1));
            Some(Lb::L1558)
        }
        Lb::L1558 => {
            v.st::<XYZ>(v.vi[6], v.vf[10]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            Some(Lb::L1560)
        }
        Lb::L1560 => {
            v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[1][2]));
            v.set_vi(3, v.vf[26][1] & 0xffff);
            v.ftoi::<1, XYZW>(14, v.vf[2]);
            v.st::<XYZW>(v.vi[6], v.vf[6]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[1][1]));
            let restart = s16(v.vi[3]) <= 0;
            v.mul::<W>(3, v.vf[30], v.vf[0]);
            v.set_vi(1, v.vi[3].wrapping_add(v.vi[13]));
            if restart {
                return Ok(h1b(v, 3, 0));
            }
            v.madd::<XYZW>(1, v.vf[17], bc(v.vf[1][0]));
            v.lq::<XYZW>(2, v.vi[1].wrapping_add(355));
            v.mul::<XYZ>(4, v.vf[4], v.q());
            v.lq::<XYZ>(10, v.vi[1].wrapping_add(549));
            v.mul::<XYZ>(12, v.vf[12], v.q());
            v.st::<XYZW>(v.vi[6], v.vf[14]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.lq::<XYZW>(6, v.vi[1].wrapping_add(452));
            Some(Lb::L15a0)
        }
        Lb::L15a0 => {
            v.itof::<0, XYZW>(2, v.vf[2]);
            let end = s16(v.vi[4]) <= 0;
            v.add::<XYZ>(4, v.vf[4], v.vf[22]);
            v.div(v.vf[23][0], v.vf[1][3]);
            if end {
                return h2b(v, 3, budget);
            }
            v.itof::<2, XYZ>(10, v.vf[10]);
            v.set_vi(9, v.vi[9].wrapping_add(1));
            Some(Lb::L15b8)
        }
        Lb::L15b8 => {
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.set_vi(4, v.vf[26][2] & 0xffff);
            v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[2][2]));
            v.st::<XYZ>(v.vi[6], v.vf[11]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            Some(Lb::L15c8)
        }
        Lb::L15c8 => {
            v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[2][1]));
            v.set_vi(7, v.vi[7].wrapping_sub(12));
            v.ftoi::<1, XYZW>(14, v.vf[3]);
            v.st::<XYZW>(v.vi[6], v.vf[7]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.madd::<XYZW>(2, v.vf[17], bc(v.vf[2][0]));
            let restart = s16(v.vi[4]) <= 0;
            v.mul::<XYZ>(1, v.vf[1], v.q());
            v.set_vi(1, v.vi[4].wrapping_add(v.vi[13]));
            if restart {
                return Ok(h1b(v, 4, 0));
            }
            v.mul::<W>(4, v.vf[30], v.vf[0]);
            v.lq::<XYZW>(3, v.vi[1].wrapping_add(355));
            v.mul::<XYZ>(9, v.vf[9], v.q());
            v.st::<XYZW>(v.vi[6], v.vf[14]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.lq::<XYZ>(11, v.vi[1].wrapping_add(549));
            v.add::<XYZ>(1, v.vf[1], v.vf[22]);
            v.lq::<XYZW>(7, v.vi[1].wrapping_add(452));
            Some(Lb::L1608)
        }
        Lb::L1608 => {
            v.itof::<0, XYZW>(3, v.vf[3]);
            let end = s16(v.vi[5]) <= 0;
            v.div(v.vf[23][0], v.vf[2][3]);
            if end {
                return h2b(v, 4, budget);
            }
            v.itof::<2, XYZ>(11, v.vf[11]);
            v.set_vi(9, v.vi[9].wrapping_add(1));
            Some(Lb::L1620)
        }
        Lb::L1620 => {
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.st::<XYZ>(v.vi[6], v.vf[12]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[3][2]));
            v.set_vi(5, v.vf[26][3] & 0xffff);
            Some(Lb::L1630)
        }
        Lb::L1630 => {
            v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[3][1]));
            let room = s16(v.vi[7]) >= 0;
            v.ftoi::<1, XYZW>(14, v.vf[4]);
            v.st::<XYZW>(v.vi[6], v.vf[8]);
            v.set_vi(6, v.vi[6].wrapping_add(1));
            if !room {
                flush(v, 14);
            }
            Some(Lb::L14a8)
        }
    })
}

/// Variant B's restart handler for slot `s` (1..4: 0x1C58, 0x1AF0, 0x1B68, 0x1BE0) from its start (`from` 0), its restart (0x10)
/// or its group's end (0x30): back into the loop, or None at the strip list's end.
fn h1b(v: &mut Vu1, s: usize, from: u32) -> Option<Lb> {
    // Each slot's registers: the new vertex's (position, ST, colour), the previous ST made times Q, a's screen offset
    // (none for slot 3) and the fog's MUL.w on b, and where it goes back.
    let (c, st, col, st_q, a, b, back) = match s {
        1 => (4usize, 12usize, 8usize, 10usize, Some(2usize), Some(1usize), Lb::L14d8),
        2 => (1, 9, 5, 11, Some(3), Some(2), Lb::L1540),
        3 => (2, 10, 6, 12, None, None, Lb::L15a0),
        _ => (3, 11, 7, 9, Some(1), Some(4), Lb::L1608),
    };
    let ip = [5usize, 2, 3, 4][s - 1];
    let mut from = from;
    if from == 0 {
        if s == 3 {
            v.madd::<XYZW>(1, v.vf[17], bc(v.vf[1][0]));
        }
        let group = v.vi[ip] == 0;
        if s == 3 {
            v.mul::<XYZ>(4, v.vf[4], v.q());
        }
        v.st::<XYZW>(v.vi[6], v.vf[14]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        from = if group { 0x30 } else { 0x10 };
    }
    if from == 0x10 {
        v.lq::<XYZW>(c, v.vi[1].wrapping_add(483));
        v.mul::<XYZ>(st_q, v.vf[st_q], v.q());
        v.lq::<XYZ>(st, v.vi[1].wrapping_add(677));
        if let Some(a) = a {
            v.add::<XYZ>(a, v.vf[a], v.vf[22]);
        }
        if let Some(b) = b {
            v.mul::<W>(b, v.vf[30], v.vf[0]);
        }
        v.lq::<XYZW>(col, v.vi[1].wrapping_add(580));
        return Some(back);
    }
    if let Some(a) = a {
        v.add::<XYZ>(a, v.vf[a], v.vf[22]);
    }
    v.set_vi(14, v.vi[12].wrapping_add(v.vi[13]));
    v.set_vi(1, v.vf[10][3] & 0xffff);
    v.add::<W>(10, v.vf[10], v.vf[0]);
    v.lq::<XYZW>(c, v.vi[14].wrapping_add(355));
    v.lq::<XYZ>(st, v.vi[14].wrapping_add(549));
    v.mul::<XYZ>(st_q, v.vf[st_q], v.q());
    v.lq::<XYZW>(col, v.vi[14].wrapping_add(452));
    let more = v.vi[12] != 0;
    if let Some(b) = b {
        v.mul::<W>(b, v.vf[30], v.vf[0]);
    }
    v.ilw(12, v.vi[1], 3);
    if more { Some(back) } else { None }
}

/// Variant B's strip-end handler for slot `s` (1..4: 0x1710, 0x1800, 0x18F8, 0x19F0): a restart goes back into the slot; a strip's
/// end starts the next strip (its four quadwords of texture setup from vi11, the buffer kicked first when they do not fit).
fn h2b(v: &mut Vu1, s: usize, budget: &mut u32) -> Result<Option<Lb>, String> {
    let ic = [2usize, 3, 4, 5][s - 1];
    let d = [1usize, 2, 3, 4][s - 1];
    v.add::<W>(d, v.vf[d], v.vf[9]);
    let restart = v.vi[ic] != 0;
    // The branch's delay slot.
    match s {
        1 => {}
        2 => {
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.set_vi(9, v.vi[9].wrapping_add(1));
        }
        3 => {
            v.itof::<2, XYZ>(10, v.vf[10]);
            v.set_vi(9, v.vi[9].wrapping_add(1));
        }
        _ => {
            v.itof::<2, XYZ>(11, v.vf[11]);
            v.set_vi(9, v.vi[9].wrapping_add(1));
        }
    }
    if restart {
        return Ok(Some([Lb::L14f8, Lb::L1558, Lb::L15b8, Lb::L1620][s - 1]));
    }
    v.set_vi(1, v.vf[29][3] & 0xffff);
    match s {
        1 => {
            v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[4][2]));
            v.lq::<XYZW>(31, v.vi[11]);
            v.set_vi(11, v.vi[11].wrapping_add(1));
        }
        2 => {
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.lq::<XYZW>(31, v.vi[11]);
            v.set_vi(11, v.vi[11].wrapping_add(1));
            v.set_vi(3, v.vf[26][1] & 0xffff);
        }
        3 => {
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.lq::<XYZW>(31, v.vi[11]);
            v.set_vi(11, v.vi[11].wrapping_add(1));
            v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[2][2]));
            v.set_vi(4, v.vf[26][2] & 0xffff);
        }
        _ => {
            v.mul::<XYZW>(ACC, v.vf[20], bc(v.vf[0][3]));
            v.lq::<XYZW>(31, v.vi[11]);
            v.set_vi(11, v.vi[11].wrapping_add(1));
            v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[3][2]));
            v.set_vi(5, v.vf[26][3] & 0xffff);
        }
    }
    v.ftoi::<0, Z>(31, v.vf[31]);
    v.set_vi(7, v.vi[7].wrapping_sub(6));
    if s == 4 {
        v.set_vi(14, v.vi[7].wrapping_add(12));
    }
    v.isw::<X>(9, v.vi[1]);
    let room = if s == 4 { v.vi[14] } else { v.vi[7] };
    let fits = s16(room) >= 0;
    v.set_vi(9, v.vi[9].wrapping_add(16384));
    if !fits {
        v.set_vi(9, v.vi[9].wrapping_add(16384));
        v.isw::<X>(9, v.vi[1]);
        v.set_vi(1, v.vf[27][0] & 0xffff);
        v.set_vi(6, v.vf[27][1] & 0xffff);
        let f = v.vf[27];
        v.vf[27] = [f[1], f[2], f[3], f[0]];
        v.set_vi(7, [110, 113, 116, 107][s - 1]);
        v.kick(v.vi[1]);
    }
    step(budget)?;
    v.lq::<XYZW>(13, v.vi[11]);
    v.set_vi(11, v.vi[11].wrapping_add(1));
    v.lq::<XYZW>(15, v.vi[11]);
    v.set_vi(11, v.vi[11].wrapping_add(1));
    v.st::<XYZ>(v.vi[6], v.vf[24]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    v.lq::<XYZW>(16, v.vi[11]);
    v.set_vi(11, v.vi[11].wrapping_add(1));
    v.set_vi(15, 4);
    v.set_vi(9, 0);
    v.isw::<X>(15, v.vi[6].wrapping_sub(1));
    for r in [31, 13, 15, 16] {
        v.st::<XYZW>(v.vi[6], v.vf[r]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
    }
    v.vf[29][3] = sx(v.vi[6]);
    v.st::<XYZW>(v.vi[6], v.vf[23]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    // SQI.xyzw (the loop's own store of it is SQI.xyz).
    let st = [9usize, 10, 11, 12][s - 1];
    v.st::<XYZW>(v.vi[6], v.vf[st]);
    v.set_vi(6, v.vi[6].wrapping_add(1));
    Ok(Some([Lb::L1500, Lb::L1560, Lb::L15c8, Lb::L1630][s - 1]))
}
