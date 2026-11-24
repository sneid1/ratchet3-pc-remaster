//! VU1 program 5 (the main program's VU1 microcode uploaded from 0x1023F8, 589 instruction pairs: the clip-space shrub program
//! of `uyafmt::shrub`), done by the port's own code from its microcode on `vu1::Vu1` (the module doc there: the arithmetic, the
//! pipeline and the check against the translation).
//!
//! - Entry 0x00 loads its constants: vf31 (a GIF tag), vf30, vf29 (the fog's bounds), vf22 and vf23 (the screen's scale and
//!   offset), vf24 (the fans' GIF tag) from quadwords 0 to 5, and vf17 the integers (856, 520, 856, 520) as floats plus
//!   vf30.y (2^23: the integer is then the float's low bits, which MTIR takes): the two output buffers' starts.
//! - Entries 0x88 and 0xA8 unpack a block: vi11 turns to the other input buffer (122 - vi11), vf17's x and z are made the
//!   buffers' starts less the other's (and turned for the next block), quadword 1 y gets 379 or 238; the block's texture setups
//!   (its header's x of them, five quadwords each after vf31) and its strip heads are copied to both output buffers at their
//!   own offsets (each record's w), and its positions and texture coordinates turned to floats (ITOF0, ITOF12) in place.
//! - Entry 0x318 draws: quadword 1 y turns between the two unpacked blocks (617 less it); for each of the block's batches (vi14
//!   of them, 28 quadwords apart: a matrix, the fog's terms and the colours' table) every vertex goes through the matrix to clip
//!   space with Q = 1 / w, its colour looked up (its position's w, masked to 15 bits, indexes the batch's table) and stored with
//!   its ST times Q at its place in the output buffer (vf16 + vf17's offset), then each strip of the batch (its record: count,
//!   first vertex) is made screen positions (vf22 vf16.xyz Q + vf23, FTOI4, the fog the batch's) and judged against the
//!   frustum, three judgements at a time: a triangle with a vertex outside is drawn clipped instead (its last vertex gets ADC)
//!   unless all three are outside one plane of -z, -y, +y, -x, +x: its three corners are clipped against -z, -x, +x, -y and +y
//!   (Sutherland-Hodgman, the subroutines at 0x1130, 0x11C0 and 0x1238 returning through vi02) and, with three or more left,
//!   kicked as a fan after a GIF tag with the texture setup of the strip's group. The batch's strips are kicked from the
//!   buffer, which turns (MR32 of vf17) for the next.

use crate::vu1::{bc, s16, sx, Vu1, ACC, W, X, XW, XY, XYZ, XYZW, XZ, Y, YW, Z, ZW};

/// More vertices, edges or strips than this in one run is data the program loops on (the translation stops at 5,000,000
/// pairs).
const BUDGET: u32 = 2_000_000;

pub(crate) fn run(v: &mut Vu1, entry: usize) -> Result<usize, String> {
    let mut budget = BUDGET;
    match entry {
        0x00 => Ok(constants(v)),
        0x88 => unpack(v, 379, &mut budget),
        0xa8 => unpack(v, 238, &mut budget),
        0x318 => draw(v, &mut budget),
        e => Err(format!("VU1 program 5 started at {e:#x} (its runs start at 0x0, 0x88, 0xA8 and 0x318)")),
    }
}

/// One more step of a loop, or the run stops.
fn step(budget: &mut u32) -> Result<(), String> {
    *budget = budget.checked_sub(1).ok_or("VU1 program 5: more than 2,000,000 loop steps without ending")?;
    Ok(())
}

/// Entry 0x00 (to the E bit at 0x78 and its slot).
fn constants(v: &mut Vu1) -> usize {
    v.lq::<XYZW>(31, 0);
    v.lq::<XYZW>(30, 1);
    v.lq::<XYZW>(29, 2);
    v.set_vi(11, 120);
    v.set_vi(1, 856);
    v.vf[17][0] = sx(v.vi[1]);
    v.vf[17][2] = sx(v.vi[1]);
    v.set_vi(1, 520);
    v.vf[17][1] = sx(v.vi[1]);
    v.vf[17][3] = sx(v.vi[1]);
    v.lq::<XYZW>(22, 3);
    v.lq::<XYZW>(23, 4);
    v.itof::<0, XYZW>(17, v.vf[17]);
    v.lq::<XYZW>(24, 5);
    v.add::<XYZW>(17, v.vf[17], bc(v.vf[30][1]));
    0x80
}

/// Entries 0x88 and 0xA8: the block at the input buffer vi11 turns to (its header: x texture setups, y strip heads, z vertices,
/// w the vertices' offset), quadword 1 y `mark` (to the E bit at 0x308 and its slot).
fn unpack(v: &mut Vu1, mark: u32, budget: &mut u32) -> Result<usize, String> {
    v.sub::<XZ>(17, v.vf[0], bc(v.vf[17][0]));
    v.set_vi(1, 122);
    v.add::<XYZW>(28, v.vf[0], bc(v.vf[30][1]));
    v.set_vi(11, v.vi[1].wrapping_sub(v.vi[11]));
    v.set_vi(13, mark);
    // 0x0C0..0x118.
    v.isw::<Y>(13, 1);
    v.sub::<XZ>(17, v.vf[17], bc(v.vf[17][1]));
    v.set_vi(9, v.vi[11].wrapping_add(1));
    v.ilw(5, v.vi[11], 1);
    v.ilw(15, v.vi[11], 0);
    v.lq::<XYZW>(1, v.vi[9]);
    v.set_vi(9, v.vi[9].wrapping_add(1));
    v.add::<XZ>(17, v.vf[17], bc(v.vf[30][0]));
    v.set_vi(15, v.vi[15].wrapping_sub(1));
    v.set_vi(4, v.vf[1][3] & 0xffff);
    v.set_vi(2, v.vf[17][0] & 0xffff);
    let more = v.vi[15] != 0;
    v.set_vi(3, v.vf[17][1] & 0xffff);
    if !more {
        // 0x120: a block with one texture setup.
        v.add::<Y>(28, v.vf[28], bc(v.vf[31][2]));
    }
    // 0x128..0x1A0: the strip heads (two at a time) to both buffers at each one's offset.
    loop {
        step(budget)?;
        v.lq::<XYZW>(2, v.vi[9]);
        v.set_vi(9, v.vi[9].wrapping_add(1));
        v.set_vi(1, v.vi[4].wrapping_add(v.vi[2]));
        v.set_vi(14, v.vi[4].wrapping_add(v.vi[3]));
        v.set_vi(5, v.vi[5].wrapping_sub(1));
        v.set_vi(4, v.vf[2][3] & 0xffff);
        v.st::<XYZW>(v.vi[1], v.vf[1]);
        let last = v.vi[5] == 0;
        v.st::<XYZW>(v.vi[14], v.vf[1]);
        if last {
            break;
        }
        v.lq::<XYZW>(1, v.vi[9]);
        v.set_vi(9, v.vi[9].wrapping_add(1));
        v.set_vi(1, v.vi[4].wrapping_add(v.vi[2]));
        v.set_vi(14, v.vi[4].wrapping_add(v.vi[3]));
        v.set_vi(5, v.vi[5].wrapping_sub(1));
        v.set_vi(4, v.vf[1][3] & 0xffff);
        v.st::<XYZW>(v.vi[1], v.vf[2]);
        let more = v.vi[5] != 0;
        v.st::<XYZW>(v.vi[14], v.vf[2]);
        if !more {
            break;
        }
    }
    // 0x1A8..0x258: the texture setups (four quadwords after vf31's tag) to both buffers.
    v.set_vi(9, v.vi[9].wrapping_sub(1));
    v.ilw(5, v.vi[11], 0);
    loop {
        step(budget)?;
        for r in [18, 19, 20, 21] {
            v.lq::<XYZW>(r, v.vi[9]);
            v.set_vi(9, v.vi[9].wrapping_add(1));
        }
        v.set_vi(4, v.vf[18][3] & 0xffff);
        v.set_vi(5, v.vi[5].wrapping_sub(1));
        v.set_vi(1, v.vi[4].wrapping_add(v.vi[2]));
        for (k, r) in [31, 18, 19, 20, 21].into_iter().enumerate() {
            v.st::<XYZW>(v.vi[1].wrapping_add(k as u32), v.vf[r]);
        }
        v.set_vi(1, v.vi[4].wrapping_add(v.vi[3]));
        for (k, r) in [31, 18, 19, 20].into_iter().enumerate() {
            v.st::<XYZW>(v.vi[1].wrapping_add(k as u32), v.vf[r]);
        }
        let more = v.vi[5] != 0;
        v.st::<XYZW>(v.vi[1].wrapping_add(4), v.vf[21]);
        if !more {
            break;
        }
    }
    // 0x260..0x300: the positions (ITOF0) and texture coordinates (ITOF12, xyz), two vertices at a time, in place.
    v.ilw(12, v.vi[11], 2);
    v.ilw(6, v.vi[11], 3);
    v.set_vi(15, 1);
    v.set_vi(5, 1);
    v.set_vi(5, v.vi[5].wrapping_add(v.vi[12]));
    v.set_vi(6, v.vi[6].wrapping_add(v.vi[11]));
    v.set_vi(8, v.vi[6].wrapping_add(v.vi[12]));
    loop {
        step(budget)?;
        v.lq::<XYZW>(1, v.vi[6]);
        v.lq::<XYZ>(5, v.vi[8]);
        v.lq::<XYZW>(18, v.vi[6].wrapping_add(1));
        v.set_vi(5, v.vi[5].wrapping_sub(2));
        v.itof::<0, XYZW>(1, v.vf[1]);
        v.lq::<XYZ>(19, v.vi[8].wrapping_add(1));
        v.itof::<2, XYZ>(5, v.vf[5]);
        v.itof::<0, XYZW>(18, v.vf[18]);
        v.itof::<2, XYZ>(19, v.vf[19]);
        v.st::<XYZW>(v.vi[6], v.vf[1]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        let done = v.vi[5] == 0;
        v.st::<XYZ>(v.vi[8], v.vf[5]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        if done {
            break;
        }
        v.st::<XYZW>(v.vi[6], v.vf[18]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        let more = s16(v.vi[15]) != s16(v.vi[5]);
        v.st::<XYZ>(v.vi[8], v.vf[19]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        if !more {
            break;
        }
    }
    Ok(0x310)
}

/// vf1.x = vf30.x - vf17.x - vf17.y: the clipper's scratch (its integer in the float's low bits).
fn scratch(v: &mut Vu1) {
    v.sub::<X>(1, v.vf[0], bc(v.vf[17][0]));
    v.sub::<X>(1, v.vf[1], bc(v.vf[17][1]));
    v.add::<X>(1, v.vf[1], bc(v.vf[30][0]));
}

/// Entry 0x318: the unpacked block drawn (to the E bit at 0x610 and its slot).
fn draw(v: &mut Vu1, budget: &mut u32) -> Result<usize, String> {
    v.ilw(13, 1, 1);
    v.set_vi(1, 617);
    v.set_vi(13, v.vi[1].wrapping_sub(v.vi[13]));
    v.isw::<Y>(13, 1);
    v.ilw(14, v.vi[13], 0);
    v.set_vi(13, v.vi[13].wrapping_add(5));
    loop {
        // 0x350..0x3C8: the batch's matrix (vf18..vf21) and fog (vf2.w = vf21.w vf30.z + vf30.w, between vf29.x and vf29.y).
        v.lq::<XYZW>(21, v.vi[13].wrapping_sub(1));
        v.ilw(6, v.vi[11], 3);
        v.lq::<XYZW>(18, v.vi[13].wrapping_sub(4));
        v.lq::<XYZW>(19, v.vi[13].wrapping_sub(3));
        v.mul::<W>(ACC, v.vf[21], bc(v.vf[30][2]));
        v.lq::<XYZW>(20, v.vi[13].wrapping_sub(2));
        v.madd::<W>(2, v.vf[30], bc(v.vf[0][3]));
        v.lq::<W>(4, v.vi[13]);
        v.set_vi(6, v.vi[6].wrapping_add(v.vi[11]));
        v.set_vi(10, 32767);
        v.set_vi(8, v.vi[6].wrapping_add(v.vi[12]));
        v.max::<W>(2, v.vf[2], bc(v.vf[29][0]));
        v.set_vi(10, 32767);
        v.set_vi(3, v.vi[12]);
        v.mini::<W>(2, v.vf[2], bc(v.vf[29][1]));
        v.lq::<XYZW>(14, v.vi[6]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        v.lq::<XYZW>(15, v.vi[8]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        transform(v, budget)?;
        // 0x4C0..0x4F8: the fog as FTOI4 (vf2.w), vf3.w the same with ADC; the first strip's record (vf8).
        v.ftoi::<1, W>(2, v.vf[2]);
        v.set_vi(2, v.vf[17][3] & 0xffff);
        v.set_vi(9, v.vi[11].wrapping_add(1));
        v.ilw(5, v.vi[11], 1);
        v.set_vi(1, v.vi[10].wrapping_add(1));
        v.lq::<XYZW>(8, v.vi[9]);
        v.set_vi(9, v.vi[9].wrapping_add(1));
        v.set_vi(4, v.vf[2][3] & 0xffff);
        v.set_vi(4, v.vi[4] | v.vi[1]);
        v.vf[3][3] = sx(v.vi[4]);
        strips(v, budget)?;
        // 0x5D0..0x608: the buffer's packet kicked, the buffers turned for the next batch.
        v.set_vi(15, v.vf[17][1] & 0xffff);
        let f = v.vf[17];
        v.vf[17] = [f[1], f[2], f[3], f[0]];
        v.set_vi(9, v.vf[28][0] & 0xffff);
        v.set_vi(14, v.vi[14].wrapping_sub(1));
        v.set_vi(13, v.vi[13].wrapping_add(28));
        v.set_vi(15, v.vi[15].wrapping_add(v.vi[9]));
        v.add::<X>(28, v.vf[0], bc(v.vf[28][1]));
        let more = v.vi[14] != 0;
        v.kick(v.vi[15]);
        if !more {
            return Ok(0x618);
        }
        step(budget)?;
    }
}

/// 0x3D8..0x4B8: the batch's vertices, two at a time, to clip space (vf6, vf1), Q = 1 / w; each stored at its place in the
/// output buffer (its position plus vf17, w: vf16.w, vf7.w) with its colour (its ST's w, masked to 15 bits, indexes the batch's
/// table) and its ST with z times Q.
fn transform(v: &mut Vu1, budget: &mut u32) -> Result<(), String> {
    loop {
        step(budget)?;
        v.mul::<XYZW>(ACC, v.vf[21], bc(v.vf[0][3]));
        v.add::<XYZW>(16, v.vf[14], v.vf[17]);
        v.lq::<XYZW>(1, v.vi[6]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[14][0]));
        v.set_vi(4, v.vf[15][3] & 0xffff);
        v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[14][1]));
        v.lq::<XYZW>(5, v.vi[8]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        v.madd::<XYZW>(6, v.vf[20], bc(v.vf[14][2]));
        v.mul::<XYZW>(ACC, v.vf[21], bc(v.vf[0][3]));
        v.lq::<XYZW>(14, v.vi[6]);
        v.set_vi(6, v.vi[6].wrapping_add(1));
        v.add::<XYZW>(7, v.vf[1], v.vf[17]);
        v.madd::<XYZW>(ACC, v.vf[18], bc(v.vf[1][0]));
        v.set_vi(1, v.vf[5][3] & 0xffff);
        v.madd::<XYZW>(ACC, v.vf[19], bc(v.vf[1][1]));
        v.div(v.vf[0][3], v.vf[6][3]);
        v.madd::<XYZW>(1, v.vf[20], bc(v.vf[1][2]));
        v.set_vi(7, v.vf[16][3] & 0xffff);
        v.set_vi(4, v.vi[4] & v.vi[10]);
        v.set_vi(4, v.vi[4].wrapping_add(v.vi[13]));
        v.set_vi(1, v.vi[1] & v.vi[10]);
        v.lq::<XYZ>(4, v.vi[4]);
        v.set_vi(3, v.vi[3].wrapping_sub(1));
        v.set_vi(2, v.vf[7][3] & 0xffff);
        v.mul::<Z>(15, v.vf[15], v.q());
        v.div(v.vf[0][3], v.vf[1][3]);
        v.st::<XYZW>(v.vi[7].wrapping_add(1), v.vf[4]);
        v.st::<XYZW>(v.vi[7].wrapping_add(2), v.vf[6]);
        let done = v.vi[3] == 0;
        v.st::<XYZW>(v.vi[7], v.vf[15]);
        if done {
            return Ok(());
        }
        v.set_vi(1, v.vi[1].wrapping_add(v.vi[13]));
        v.lq::<XYZ>(4, v.vi[1]);
        v.set_vi(3, v.vi[3].wrapping_sub(1));
        v.mul::<Z>(5, v.vf[5], v.q());
        v.lq::<XYZW>(15, v.vi[8]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        v.st::<XYZW>(v.vi[2].wrapping_add(2), v.vf[1]);
        v.st::<XYZW>(v.vi[2].wrapping_add(1), v.vf[4]);
        let more = v.vi[3] != 0;
        v.st::<XYZW>(v.vi[2], v.vf[5]);
        if !more {
            return Ok(());
        }
    }
}

/// 0x500..0x5C8: the batch's strips (vi5 of them; each record vf8: x its vertex count, w its first vertex's place).
fn strips(v: &mut Vu1, budget: &mut u32) -> Result<(), String> {
    loop {
        v.set_vi(4, v.vf[8][3] & 0xffff);
        v.set_vi(3, v.vf[8][0] & 0xffff);
        v.set_vi(5, v.vi[5].wrapping_sub(1));
        v.set_vi(7, 0u32.wrapping_sub(3));
        v.set_vi(6, v.vi[4].wrapping_add(v.vi[2]));
        v.lq::<XYZW>(16, v.vi[6].wrapping_add(3));
        v.lq::<XYZW>(15, v.vi[6].wrapping_add(1));
        v.set_vi(3, v.vi[3] & v.vi[10]);
        // FCSET 0.
        v.vi[crate::vu1::CLIPF] = 0;
        v.set_vi(6, v.vi[6].wrapping_add(1));
        loop {
            step(budget)?;
            // 0x550..0x598: the vertex's screen position (vf14: FTOI4, the fog in w), judged with the two before it.
            v.mul::<XYZ>(2, v.vf[16], bc(v.vf[15][2]));
            v.vf[27] = v.vf[26];
            let j = crate::vu1::clip(v.vf[16], v.vf[16][3]);
            v.clip_in(j);
            v.vf[26] = v.vf[25];
            v.mul::<XY>(15, v.vf[15], bc(v.vf[15][2]));
            v.vf[25] = v.vf[16];
            v.set_vi(3, v.vi[3].wrapping_sub(1));
            v.mul::<XYZ>(ACC, v.vf[2], v.vf[22]);
            v.vf[14][3] = v.vf[2][3];
            v.madd::<XYZ>(2, v.vf[23], bc(v.vf[0][3]));
            v.set_vi(1, v.fcand(0x3ffff));
            v.st::<XYZW>(v.vi[6], v.vf[15]);
            v.set_vi(6, v.vi[6].wrapping_add(3));
            let outside = v.vi[1] != 0;
            v.ftoi::<1, XYZ>(14, v.vf[2]);
            v.set_vi(7, v.vi[7].wrapping_add(1));
            if outside {
                clipper(v, budget)?;
            }
            // 0x5A0..0x5B8: the next vertex; this one stored.
            v.lq::<XYZW>(15, v.vi[6]);
            v.lq::<XYZW>(16, v.vi[6].wrapping_add(2));
            let more = v.vi[3] != 0;
            v.st::<XYZW>(v.vi[6].wrapping_sub(1), v.vf[14]);
            if !more {
                break;
            }
        }
        let more = v.vi[5] != 0;
        v.lq::<XYZW>(8, v.vi[9]);
        v.set_vi(9, v.vi[9].wrapping_add(1));
        if !more {
            return Ok(());
        }
    }
}

/// The five planes the clipper takes in turn: its return address (vi02), and its three FCOR masks (both corners outside, the
/// first outside, the second outside).
#[derive(Clone, Copy)]
enum Plane {
    NearZ,
    NearX,
    FarX,
    NearY,
    FarY,
}

impl Plane {
    fn ret(self) -> u32 {
        match self {
            Plane::NearZ => 281,
            Plane::NearX => 327,
            Plane::FarX => 374,
            Plane::NearY => 420,
            Plane::FarY => 467,
        }
    }
    fn masks(self) -> [u32; 3] {
        match self {
            Plane::NearZ => [0xfff7df, 0xfff7ff, 0xffffdf],
            Plane::NearX => [0xffff7d, 0xffff7f, 0xfffffd],
            Plane::FarX => [0xffffbe, 0xffffbf, 0xfffffe],
            Plane::NearY => [0xfffdf7, 0xfffdff, 0xfffff7],
            Plane::FarY => [0xfffefb, 0xfffeff, 0xfffffb],
        }
    }
}

/// 0x620..0x10B8: the triangle the vertex ends clipped and kicked as a fan, unless it is outside one plane whole; then (0x10C0)
/// the strip's registers and its last three judgements back.
fn clipper(v: &mut Vu1, budget: &mut u32) -> Result<(), String> {
    // 0x620..0x6A8: the vertex gets ADC; under three vertices, or all three outside one of -z, -y, +y, -x, +x, nothing more.
    v.vf[14][3] = v.vf[3][3];
    let few = s16(v.vi[7]) < 0;
    v.set_vi(1, v.fcor(0xfdf7df));
    if few {
        return Ok(());
    }
    for m in [0xff7df7, 0xffbefb, 0xffdf7d, 0xffefbe] {
        let out = v.vi[1] != 0;
        v.set_vi(1, v.fcor(m));
        if out {
            return Ok(());
        }
    }
    if v.vi[1] != 0 {
        return Ok(());
    }
    // 0x6B0..0x7A0: the strip's registers saved at the scratch; its three corners there (clip position vf25..vf27, ST over
    // its z, colour as floats).
    scratch(v);
    v.set_vi(15, v.vf[1][0] & 0xffff);
    v.isw::<X>(2, v.vi[15]);
    v.isw::<Y>(3, v.vi[15]);
    v.isw::<Z>(9, v.vi[15]);
    v.isw::<W>(5, v.vi[15]);
    v.isw::<X>(7, v.vi[15].wrapping_add(1));
    v.set_vi(15, v.vi[15].wrapping_add(4));
    for (k, p) in [25usize, 26, 27].into_iter().enumerate() {
        let k = k as u32;
        v.lq::<XYZW>(5, v.vi[6].wrapping_sub(3 + 3 * k));
        v.div(v.vf[0][3], v.vf[5][2]);
        v.lq::<XYZW>(1, v.vi[6].wrapping_sub(2 + 3 * k));
        v.itof::<0, XYZW>(1, v.vf[1]);
        v.st::<XYZW>(v.vi[15].wrapping_add(3 * k), v.vf[p]);
        v.mul::<XYZ>(5, v.vf[5], v.q());
        v.st::<XYZW>(v.vi[15].wrapping_add(2 + 3 * k), v.vf[1]);
        v.st::<XYZW>(v.vi[15].wrapping_add(1 + 3 * k), v.vf[5]);
    }
    // 0x7A8..0x7E8: three corners at vi15, the output from vi15 + 36.
    v.set_vi(7, 3);
    v.set_vi(9, 0);
    v.set_vi(8, v.vi[15].wrapping_add(36));
    v.set_vi(5, v.vi[8]);
    let mut kept = true;
    for (n, plane) in [Plane::NearZ, Plane::NearX, Plane::FarX, Plane::NearY, Plane::FarY].into_iter().enumerate() {
        if n > 0 {
            // 0x8F0..0x930 (and after each plane): the output becomes the input; under three corners, nothing to draw.
            turn(v);
            if s16(v.vi[2]) < 0 {
                kept = false;
                break;
            }
        }
        v.set_vi(4, v.vi[7].wrapping_sub(1));
        v.set_vi(3, v.vi[4].wrapping_add(v.vi[4]));
        v.set_vi(3, v.vi[3].wrapping_add(v.vi[4]));
        v.set_vi(3, v.vi[3].wrapping_add(v.vi[15]));
        v.set_vi(4, v.vi[15]);
        loop {
            step(budget)?;
            edge(v, plane);
            // The return (0x8C8 and its kin): the next edge.
            v.set_vi(7, v.vi[7].wrapping_sub(1));
            v.set_vi(3, v.vi[4]);
            v.set_vi(4, v.vi[4].wrapping_add(3));
            if v.vi[7] == 0 {
                break;
            }
        }
    }
    if kept {
        // 0xEC0..0xEF8 after the last plane.
        turn(v);
        if s16(v.vi[2]) >= 0 {
            fan(v, budget)?;
        }
    }
    // 0x10C0..0x1108: the strip's registers back, its last three corners judged again; back to 0x5A0.
    scratch(v);
    v.set_vi(15, v.vf[1][0] & 0xffff);
    v.ilw(2, v.vi[15], 0);
    v.ilw(3, v.vi[15], 1);
    let j = crate::vu1::clip(v.vf[27], v.vf[27][3]);
    v.clip_in(j);
    v.ilw(9, v.vi[15], 2);
    let j = crate::vu1::clip(v.vf[26], v.vf[26][3]);
    v.clip_in(j);
    v.ilw(5, v.vi[15], 3);
    let j = crate::vu1::clip(v.vf[25], v.vf[25][3]);
    v.clip_in(j);
    v.ilw(7, v.vi[15].wrapping_add(1), 0);
    Ok(())
}

/// After a plane (0x8F0..0x918 and its kin): the plane's output is the next's input, vi7 its corners, vi2 their count less 3.
fn turn(v: &mut Vu1) {
    v.set_vi(8, v.vi[15]);
    v.set_vi(15, v.vi[5]);
    v.set_vi(5, v.vi[8]);
    v.set_vi(7, v.vi[9]);
    v.set_vi(9, 0);
    v.set_vi(2, v.vi[7].wrapping_sub(3));
}

/// One edge of the polygon against `plane` (0x7F0..0x8C0 and its kin): from the corner at vi3 (P: vf1, vf18, vf19) to the one at
/// vi4 (C: vf5, vf20, vf21), the crossing's fraction in Q.
fn edge(v: &mut Vu1, plane: Plane) {
    v.set_vi(2, plane.ret());
    v.lq::<XYZW>(1, v.vi[3]);
    v.lq::<XYZW>(5, v.vi[4]);
    v.lq::<XYZW>(18, v.vi[3].wrapping_add(1));
    v.lq::<XYZW>(19, v.vi[3].wrapping_add(2));
    v.lq::<XYZW>(20, v.vi[4].wrapping_add(1));
    let j = crate::vu1::clip(v.vf[1], v.vf[1][3]);
    v.clip_in(j);
    v.lq::<XYZW>(21, v.vi[4].wrapping_add(2));
    let (zero, p) = (v.vf[0], v.vf[1]);
    match plane {
        Plane::NearZ => {
            v.sub::<ZW>(6, v.vf[5], p);
            v.add::<W>(6, v.vf[6], bc(v.vf[6][2]));
            v.sub::<Z>(6, zero, bc(p[3]));
            v.sub::<Z>(6, v.vf[6], bc(p[2]));
        }
        Plane::NearX => {
            v.sub::<XW>(6, v.vf[5], p);
            v.add::<W>(6, v.vf[6], bc(v.vf[6][0]));
            v.sub::<X>(6, zero, bc(p[3]));
            v.sub::<X>(6, v.vf[6], bc(p[0]));
        }
        Plane::FarX => {
            v.sub::<X>(6, v.vf[5], p);
            v.sub::<W>(6, p, v.vf[5]);
            v.add::<W>(6, v.vf[6], bc(v.vf[6][0]));
            v.add::<X>(6, zero, bc(p[3]));
            v.sub::<X>(6, v.vf[6], bc(p[0]));
        }
        Plane::NearY => {
            v.sub::<YW>(6, v.vf[5], p);
            v.add::<W>(6, v.vf[6], bc(v.vf[6][1]));
            v.sub::<Y>(6, zero, bc(p[3]));
            v.sub::<Y>(6, v.vf[6], bc(p[1]));
        }
        Plane::FarY => {
            v.sub::<Y>(6, v.vf[5], p);
            v.sub::<W>(6, p, v.vf[5]);
            v.add::<W>(6, v.vf[6], bc(v.vf[6][1]));
            v.add::<Y>(6, zero, bc(p[3]));
            v.sub::<Y>(6, v.vf[6], bc(p[1]));
        }
    }
    let j = crate::vu1::clip(v.vf[5], v.vf[5][3]);
    v.clip_in(j);
    let lane = match plane {
        Plane::NearZ => 2,
        Plane::NearX | Plane::FarX => 0,
        Plane::NearY | Plane::FarY => 1,
    };
    v.div(v.vf[6][lane], v.vf[6][3]);
    // The judgements of P and C: both outside, nothing; P outside, the crossing and C (0x1130); C outside, the crossing
    // (0x11C0); else C (0x1238). Each FCOR in a branch's delay slot is made either way.
    let [both, first, second] = plane.masks();
    let out = v.fcor(both);
    v.set_vi(1, out);
    v.set_vi(1, v.fcor(first));
    if out != 0 {
        return;
    }
    let entering = v.vi[1] != 0;
    v.set_vi(1, v.fcor(second));
    if entering {
        crossing(v);
        corner(v);
        v.set_vi(9, v.vi[9].wrapping_add(2));
    } else if v.vi[1] != 0 {
        crossing(v);
        v.set_vi(9, v.vi[9].wrapping_add(1));
    } else {
        corner(v);
        v.set_vi(9, v.vi[9].wrapping_add(1));
    }
}

/// P + (C - P) Q for the corner's three quadwords (SUB, MULAq, MADDw), each stored (SQI to vi8).
fn crossing(v: &mut Vu1) {
    for (c, p) in [(5usize, 1usize), (20, 18), (21, 19)] {
        v.sub::<XYZW>(6, v.vf[c], v.vf[p]);
        v.mul::<XYZW>(ACC, v.vf[6], v.q());
        v.madd::<XYZW>(6, v.vf[p], bc(v.vf[0][3]));
        v.st::<XYZW>(v.vi[8], v.vf[6]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
    }
}

/// C's three quadwords stored (SQI to vi8).
fn corner(v: &mut Vu1) {
    for r in [5, 20, 21] {
        v.st::<XYZW>(v.vi[8], v.vf[r]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
    }
}

/// 0xF08..0x10B8: the clipped polygon (vi7 corners at vi15) kicked as a fan: vf24's tag kicked alone, then the strip's group's
/// texture setup (the last whose first vertex is at or before the strip's, vf8.w), the fan's tag (vf24 with NLOOP the count) and
/// its corners (ST times Q, the colour by FTOI0, the screen position by FTOI4).
fn fan(v: &mut Vu1, budget: &mut u32) -> Result<(), String> {
    scratch(v);
    v.set_vi(2, v.vf[1][0] & 0xffff);
    v.set_vi(8, v.vi[2].wrapping_add(76));
    v.set_vi(3, v.vi[8]);
    v.st::<XYZW>(v.vi[8], v.vf[24]);
    v.set_vi(8, v.vi[8].wrapping_add(1));
    v.kick(v.vi[3]);
    v.ilw(5, v.vi[11], 1);
    v.ilw(3, v.vi[11], 0);
    v.set_vi(9, v.vi[11].wrapping_add(1));
    v.set_vi(9, v.vi[9].wrapping_add(v.vi[5]));
    loop {
        step(budget)?;
        v.lq::<XYZW>(18, v.vi[9]);
        v.set_vi(4, v.vf[8][3] & 0xffff);
        v.set_vi(2, v.vf[18][3] & 0xffff);
        v.set_vi(4, v.vi[4].wrapping_sub(v.vi[2]));
        v.set_vi(3, v.vi[3].wrapping_sub(1));
        if s16(v.vi[4]) < 0 {
            break;
        }
        let more = v.vi[3] != 0;
        v.set_vi(9, v.vi[9].wrapping_add(4));
        if !more {
            break;
        }
    }
    v.set_vi(9, v.vi[9].wrapping_sub(4));
    for r in [18, 19, 20, 21] {
        v.lq::<XYZW>(r, v.vi[9]);
        v.set_vi(9, v.vi[9].wrapping_add(1));
    }
    v.set_vi(3, v.vi[8]);
    for r in [31, 18, 19, 20, 21] {
        v.st::<XYZW>(v.vi[8], v.vf[r]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
    }
    let t = v.vf[24];
    v.vf[1][1..].copy_from_slice(&t[1..]);
    v.set_vi(2, v.vf[24][0] & 0xffff);
    v.set_vi(2, v.vi[2].wrapping_add(v.vi[7]));
    v.vf[1][0] = sx(v.vi[2]);
    v.st::<XYZW>(v.vi[8], v.vf[1]);
    v.set_vi(8, v.vi[8].wrapping_add(1));
    loop {
        step(budget)?;
        for r in [1, 18, 19] {
            v.lq::<XYZW>(r, v.vi[15]);
            v.set_vi(15, v.vi[15].wrapping_add(1));
        }
        v.div(v.vf[0][3], v.vf[1][3]);
        v.mul::<XYZ>(18, v.vf[18], v.q());
        v.mul::<XYZ>(2, v.vf[1], v.q());
        v.mul::<XYZ>(ACC, v.vf[2], v.vf[22]);
        v.madd::<XYZ>(2, v.vf[23], bc(v.vf[0][3]));
        v.ftoi::<1, XYZ>(2, v.vf[2]);
        v.set_vi(7, v.vi[7].wrapping_sub(1));
        v.ftoi::<0, XYZ>(4, v.vf[19]);
        v.st::<XYZW>(v.vi[8], v.vf[18]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        v.st::<XYZW>(v.vi[8], v.vf[4]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        let more = v.vi[7] != 0;
        v.st::<XYZW>(v.vi[8], v.vf[2]);
        v.set_vi(8, v.vi[8].wrapping_add(1));
        if !more {
            break;
        }
    }
    v.kick(v.vi[3]);
    Ok(())
}
