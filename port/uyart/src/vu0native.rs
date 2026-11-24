//! The game's VU0 microprograms `p100af0`, `tie` and `math` (translated/uya/vu0, uyatr's vu0mp.rs) as hand-written code:
//! `vu0::vu0_run` runs these instead of their translations. Each start the game makes is its translation's statements in
//! order (every pair's upper result taken, its lower instruction done, the result put, as the translation orders them), so a
//! run leaves what the translation leaves: VF, ACC, VI (I, Q, the integer registers), the MAC flags of the run's last
//! MAC-writing instruction in VI17 and the status flag's zero and sign bits from them with their sticky copies or'ed with
//! every MAC write's (the translation's end of a run), the D and I bits of a DIV-unit instruction as it issues. What goes
//! away is the translation's machinery: the node dispatch, the pair count, the MAC flags' pipeline slots (the one reader,
//! FMAND, gets the flags of the instruction the translation gives it), the MAC flags' landings in VI17 that the run's end
//! overwrites.
//!
//! None of the three touches VU0's data memory or anything of the EE's, and none waits for the EE (no wait loops: `park` is
//! empty for all three). The starts, from every VCALLMS in the translated EE code (the same functions in every story level
//! and front end; level 1's addresses):
//! - `p100af0` (tag 0x100AE0), the shrubs' and the ties' vertex light: 0x0 and 0x160 by the shrub lighting 0x4A3D80 (at
//!   0x4A4144, 0x4A4254 and 0x4A41A4; `shrubcull` does those runs itself unless its check runs the translation), 0x2C0,
//!   0x420 and 0x580 by the tie light patches 0x4B3368 (at 0x4B3994, 0x4B3AF0, 0x4B3DD8; 0x580 for a tie whose instance
//!   names point lights in its +0x1A, levels 31 and 36 at their starts).
//! - `tie` (tag 0x1159B0), the ties' second-draw texture coordinates (uyafmt `tie.rs`): 0x0 (0x4B15C8) and 0x150 (0x4B14E8,
//!   0x4B16C0) by 0x4B13C8 for packets of entry flag 2, 0x2A0 (0x4B1A08) and 0x4A0 (0x4B1930, 0x4B1AE0) by 0x4B17E8 for flag 4
//!   (only level 1's tie class 7210 has such packets: its instances at (171, 449, 85), (338, 289, 77), (363, 275, 77),
//!   (270, 397, 84) and (193, 367, 80)).
//! - `math` (micro 0xC80), the library's: 0xC80 (the rotation matrix from three angles: 0x40C63C, 0x40C65C, 0x439644), 0xDA0
//!   and 0xDB0 (sine and cosine: 0x40C3D0, 0x40C3E8, 0x40CCE0, 0x40CCF0, the moby draw path's 0x4A0EEC, 0x4A0F00, 0x4A1228),
//!   0xE98 (vf14 to vf16 from vf8: 0x40CB84, 0x40CBA4) and 0xF10 (the arcsine of vf1 x: 0x4A1098).
//! The VCALLMSRs (CMSAR0 at 0x3F82B8) are the skin program's. Any other start of these programs is not one the game makes,
//! and panics (the translations' other entries are their branch targets and the code after each E bit's slot, places a run
//! passes through).
//!
//! With UYA_VU0_CHECK=N (read once) the first N runs of the three run twice from the same state: the translation, then this
//! code on the state as it was before; every difference in what they leave (VF, ACC, VI, VU0's data memory, where the run
//! stopped and whether it waits) is printed, with a summary of the runs checked and differing by program and start, and the
//! game goes on from the translation's state.

use crate::vu0::Vu0Prog;
use crate::{Cpu, Ext};
use std::collections::BTreeMap;
use std::sync::Mutex;
use uycpu::vu0::{m_abs, m_fmac, m_ftoi, m_itof, m_mac, m_opmula, m_put, m_q};

type V = [u32; 4];

/// A program's native run from a micro address: (where it stopped, waiting), as the translation's `run` gives them.
pub(crate) type Native = fn(&mut Cpu, usize) -> (usize, bool);

/// The native code of a translated program by its name (translated/uya/game `VU0`). `skin` has none here: the moby draw
/// path does its runs.
pub(crate) fn native(name: &str) -> Option<Native> {
    match name {
        "p100af0" => Some(p100af0),
        "tie" => Some(tie),
        "math" => Some(math),
        _ => None,
    }
}

// ---- the arithmetic, `uycpu::vu0`'s as the translation calls it ------------------------------------------------------------

/// An FMAC instruction's destination ACC (a VF number otherwise).
const ACC: usize = 32;

/// The MAC flags a run makes: its last MAC-writing instruction's, and every one's or'ed.
#[derive(Default)]
struct Mac {
    last: u32,
    or: u32,
}

impl Mac {
    /// The MAC flags of a result on the lanes D (the translation's `m_mac`), the run's last so far.
    #[inline(always)]
    fn put<const D: u32>(&mut self, u: V) -> u32 {
        let f = m_mac::<D>(u);
        self.last = f;
        self.or |= f;
        f
    }

    /// The run's end (every run of the three writes MAC flags): VI17 its last MAC flags, VI16's zero and sign bits from
    /// them and their sticky copies from all, as the translation lands them.
    #[inline(always)]
    fn land(&self, c: &mut Cpu) {
        let st = ((self.last & 0xf != 0) as u32) | (((self.last & 0xf0 != 0) as u32) << 1);
        let so = ((self.or & 0xf != 0) as u32) | (((self.or & 0xf0 != 0) as u32) << 1);
        c.vi[17] = self.last;
        c.vi[16] = (c.vi[16] & !0xf) | st | (so << 6);
    }
}

/// An upper FMAC instruction whose pair's lower one touches nothing it reads or writes: K as `m_fmac` (0 ADD, 1 SUB, 2 MADD,
/// 3 MSUB, 4 MAX, 5 MINI, 6 MUL) on the lanes D of VF `fs` and `t` (with ACC), into VF `fd` or ACC, with its MAC flags (MAX
/// and MINI make none).
#[inline(always)]
fn fm<const K: u8, const D: u32>(c: &mut Cpu, m: &mut Mac, fd: usize, fs: usize, t: V) {
    let u = m_fmac::<K, D>(c.vf[fs], t, c.vacc);
    if fd == ACC {
        m_put::<D>(&mut c.vacc, u);
    } else {
        m_put::<D>(&mut c.vf[fd], u);
    }
    if K != 4 && K != 5 {
        m.put::<D>(u);
    }
}

/// ITOF (N: 0, 4, 12, 15 as 0 to 3) of VF `fs` into the lanes D of VF `ft`.
#[inline(always)]
fn itof<const N: usize, const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    let u = m_itof::<N>(c.vf[fs]);
    m_put::<D>(&mut c.vf[ft], u);
}

/// FTOI (N as `itof`) of VF `fs` into the lanes D of VF `ft`.
#[inline(always)]
fn ftoi<const N: usize, const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    let u = m_ftoi::<N>(c.vf[fs]);
    m_put::<D>(&mut c.vf[ft], u);
}

/// MR32's rotation of a vector.
#[inline(always)]
fn rot(v: V) -> V {
    [v[1], v[2], v[3], v[0]]
}

/// IBNE vi0, `r`: whether the branch is taken.
#[inline(always)]
fn ibne0(c: &Cpu, r: usize) -> bool {
    (c.vi[0] as u16 as i16) != (c.vi[r] as u16 as i16)
}

// ---- p100af0 (tag 0x100AE0): the shrubs' and the ties' vertex light ------------------------------------------------------

fn p100af0(c: &mut Cpu, a: usize) -> (usize, bool) {
    let mut m = Mac::default();
    let end = match a {
        0x0 => {
            shrub_light(c, &mut m, [1, 2, 3, 4], [9, 10, 11, 12]);
            0x158
        }
        0x160 => {
            shrub_light(c, &mut m, [5, 6, 7, 8], [13, 14, 15, 16]);
            0x2b8
        }
        0x2c0 => {
            patch_light(c, &mut m, [1, 2, 3, 4], [9, 10, 11, 12]);
            0x418
        }
        0x420 => {
            patch_light(c, &mut m, [5, 6, 7, 8], [13, 14, 15, 16]);
            0x578
        }
        0x580 => {
            p100af0_580(c, &mut m);
            0x640
        }
        _ => panic!("VU0 microprogram p100af0 started at {a:#x}: not a start the game makes (0x0, 0x160, 0x2C0, 0x420, 0x580)"),
    };
    m.land(c);
    (end, false)
}

/// From 0x0 (to 0x158, the E bit at 0x150) on the normals of vf1 to vf4 into the colours vf9 to vf12, from 0x160 (to
/// 0x2B8) on vf5 to vf8 into vf13 to vf16: ITOF15.xyz of each normal (LOI 0x478000F3 with the first), each turned by the
/// light directions' rows vf24 to vf26 (MULAx, MADDAy, MADDz on every lane), MUL.xyz by vf30 into vf17 to vf20 and MAX.xyz
/// with that, then each colour = vf31 w + vf27 x + vf28 y + vf29 z of its normal (MULAw by vf0, MADDAx, MADDAy, MADDz),
/// MINIi.xyz.
#[inline(always)]
fn shrub_light(c: &mut Cpu, m: &mut Mac, v: [usize; 4], o: [usize; 4]) {
    for (j, &n) in v.iter().enumerate() {
        itof::<3, 14>(c, n, n);
        if j == 0 {
            c.vi[21] = 0x4780_00f3;
        }
    }
    for &n in &v {
        fm::<6, 15>(c, m, ACC, 24, [c.vf[n][0]; 4]);
        fm::<2, 15>(c, m, ACC, 25, [c.vf[n][1]; 4]);
        fm::<2, 15>(c, m, n, 26, [c.vf[n][2]; 4]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<6, 14>(c, m, 17 + j, n, c.vf[30]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<4, 14>(c, m, n, n, c.vf[17 + j]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<6, 15>(c, m, ACC, 31, [c.vf[0][3]; 4]);
        fm::<2, 15>(c, m, ACC, 27, [c.vf[n][0]; 4]);
        fm::<2, 15>(c, m, ACC, 28, [c.vf[n][1]; 4]);
        fm::<2, 15>(c, m, o[j], 29, [c.vf[n][2]; 4]);
    }
    for &n in &o {
        fm::<5, 14>(c, m, n, n, [c.vi[21]; 4]);
    }
}

/// From 0x2C0 (to 0x418, the E bit at 0x410) on the normals vf1 to vf4 with the base colours vf9 to vf12, from 0x420 (to
/// 0x578) on vf5 to vf8 with vf13 to vf16, the colours into the normals' registers: MULz.xy of each normal by its own z
/// (LOI 0x478000F3 with the first), each turned by the rows vf24 to vf26 from its x, y and w (MULAx, MADDAy, MADDw), MUL.xyz
/// by vf30 into vf17 to vf20 and MAX.xyz with that, then each = its base colour's w + vf27 x + vf28 y + vf29 z of it (MULAw
/// by vf0, MADDAx, MADDAy, MADDz), MINIi.xyz.
#[inline(always)]
fn patch_light(c: &mut Cpu, m: &mut Mac, v: [usize; 4], base: [usize; 4]) {
    for (j, &n) in v.iter().enumerate() {
        fm::<6, 12>(c, m, n, n, [c.vf[n][2]; 4]);
        if j == 0 {
            c.vi[21] = 0x4780_00f3;
        }
    }
    for &n in &v {
        fm::<6, 15>(c, m, ACC, 24, [c.vf[n][0]; 4]);
        fm::<2, 15>(c, m, ACC, 25, [c.vf[n][1]; 4]);
        fm::<2, 15>(c, m, n, 26, [c.vf[n][3]; 4]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<6, 14>(c, m, 17 + j, n, c.vf[30]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<4, 14>(c, m, n, n, c.vf[17 + j]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<6, 15>(c, m, ACC, base[j], [c.vf[0][3]; 4]);
        fm::<2, 15>(c, m, ACC, 27, [c.vf[n][0]; 4]);
        fm::<2, 15>(c, m, ACC, 28, [c.vf[n][1]; 4]);
        fm::<2, 15>(c, m, n, 29, [c.vf[n][2]; 4]);
    }
    for &n in &v {
        fm::<5, 14>(c, m, n, n, [c.vi[21]; 4]);
    }
}

/// From 0x580 (to 0x640, the E bit at 0x638), pair by pair; Q from the RSQRT at 0x580 lands at 0x5B8.
#[inline(always)]
fn p100af0_580(c: &mut Cpu, m: &mut Mac) {
    // 0580: MAXx.x vf13, vf11, vf0x | RSQRT Q, vf25x, vf9x
    let u = m_fmac::<4, 8>(c.vf[11], [c.vf[0][0]; 4], c.vacc);
    let q = m_q::<2>(c, c.vf[25][0], c.vf[9][0]);
    m_put::<8>(&mut c.vf[13], u);
    // 0588: ITOF0.xyz vf5, vf5
    itof::<0, 14>(c, 5, 5);
    // 0590: MULz.xy vf6, vf6, vf6z
    fm::<6, 12>(c, m, 6, 6, [c.vf[6][2]; 4]);
    // 0598: MUL.xyz vf11, vf10, vf4
    fm::<6, 14>(c, m, 11, 10, c.vf[4]);
    // 05A0: MULx.xyz vf2, vf8, vf13x
    fm::<6, 14>(c, m, 2, 8, [c.vf[13][0]; 4]);
    // 05A8: MULAx ACC, vf28, vf5x
    fm::<6, 15>(c, m, ACC, 28, [c.vf[5][0]; 4]);
    // 05B0: MADDAy ACC, vf29, vf5y
    fm::<2, 15>(c, m, ACC, 29, [c.vf[5][1]; 4]);
    // 05B8: Q lands; MADDAz ACC, vf30, vf5z
    c.vi[22] = q;
    fm::<2, 15>(c, m, ACC, 30, [c.vf[5][2]; 4]);
    // 05C0: FTOI0.xyz vf1, vf2
    ftoi::<0, 14>(c, 1, 2);
    // 05C8: MADDw vf3, vf31, vf0w
    fm::<2, 15>(c, m, 3, 31, [c.vf[0][3]; 4]);
    // 05D0: MULAx ACC, vf21, vf6x
    fm::<6, 15>(c, m, ACC, 21, [c.vf[6][0]; 4]);
    // 05D8: MADDAy ACC, vf22, vf6y
    fm::<2, 15>(c, m, ACC, 22, [c.vf[6][1]; 4]);
    // 05E0: MADDw vf4, vf23, vf6w
    fm::<2, 15>(c, m, 4, 23, [c.vf[6][3]; 4]);
    // 05E8: MULq.x vf9, vf9, Q
    fm::<6, 8>(c, m, 9, 9, [c.vi[22]; 4]);
    // 05F0: SUB.xyz vf10, vf26, vf3
    fm::<1, 14>(c, m, 10, 26, c.vf[3]);
    // 05F8: MULq vf11, vf11, Q
    fm::<6, 15>(c, m, 11, 11, [c.vi[22]; 4]);
    // 0600: SUBx.w vf9, vf26, vf9x
    fm::<1, 1>(c, m, 9, 26, [c.vf[9][0]; 4]);
    // 0608: MUL.xyz vf12, vf10, vf10
    fm::<6, 14>(c, m, 12, 10, c.vf[10]);
    // 0610: MULw.xyz vf8, vf27, vf9w
    fm::<6, 14>(c, m, 8, 27, [c.vf[9][3]; 4]);
    // 0618: MULw.w vf7, vf24, vf9w
    fm::<6, 1>(c, m, 7, 24, [c.vf[9][3]; 4]);
    // 0620: ADDAy.x ACC, vf11, vf11y
    fm::<0, 8>(c, m, ACC, 11, [c.vf[11][1]; 4]);
    // 0628: MADDz.x vf11, vf25, vf11z
    fm::<2, 8>(c, m, 11, 25, [c.vf[11][2]; 4]);
    // 0630: ADDAy.x ACC, vf12, vf12y
    fm::<0, 8>(c, m, ACC, 12, [c.vf[12][1]; 4]);
    // 0638: MADDz.x vf9, vf25, vf12z (the E bit)
    fm::<2, 8>(c, m, 9, 25, [c.vf[12][2]; 4]);
    // 0640: ADDw.x vf11, vf11, vf7w (its slot)
    fm::<0, 8>(c, m, 11, 11, [c.vf[7][3]; 4]);
}

// ---- tie (tag 0x1159B0): the ties' second-draw texture coordinates --------------------------------------------------------

fn tie(c: &mut Cpu, a: usize) -> (usize, bool) {
    let mut m = Mac::default();
    let end = match a {
        0x0 => {
            tie_vertices(c, &mut m, [5, 6, 7, 8]);
            0x148
        }
        0x150 => {
            tie_vertices(c, &mut m, [15, 16, 17, 18]);
            0x298
        }
        0x2a0 => {
            tie_light(c, &mut m, [4, 5, 6], [7, 8, 9]);
            0x498
        }
        0x4a0 => {
            tie_light(c, &mut m, [14, 15, 16], [17, 18, 19]);
            0x698
        }
        _ => panic!("VU0 microprogram tie started at {a:#x}: not a start the game makes (0x0, 0x150, 0x2A0, 0x4A0)"),
    };
    m.land(c);
    (end, false)
}

/// From 0x0 (to 0x148, the E bit at 0x140) on vf5 to vf8, from 0x150 (to 0x298) on vf15 to vf18: ITOF12 of each, each
/// turned by the rows vf1 to vf3 (MULAx, MADDAy, MADDz), its product by vf9 into vf11 to vf14 summed y + z + x into z
/// (ADDAy.z, MADDx.z by vf31), each scaled by that z (MULz), then ACC = vf0 + vf9 and each = ACC - itself times vf31 w
/// (MSUBw), then ACC = vf29 x and each = ACC + itself times I (LOI 0x3E99999A; MADDi).
#[inline(always)]
fn tie_vertices(c: &mut Cpu, m: &mut Mac, v: [usize; 4]) {
    for &n in &v {
        itof::<2, 15>(c, n, n);
    }
    for &n in &v {
        fm::<6, 15>(c, m, ACC, 1, [c.vf[n][0]; 4]);
        fm::<2, 15>(c, m, ACC, 2, [c.vf[n][1]; 4]);
        fm::<2, 15>(c, m, n, 3, [c.vf[n][2]; 4]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<6, 15>(c, m, 11 + j, 9, c.vf[n]);
    }
    for k in 11..15 {
        fm::<0, 2>(c, m, ACC, k, [c.vf[k][1]; 4]);
        fm::<2, 2>(c, m, k, 31, [c.vf[k][0]; 4]);
    }
    for (j, &n) in v.iter().enumerate() {
        fm::<6, 15>(c, m, n, n, [c.vf[11 + j][2]; 4]);
    }
    fm::<0, 15>(c, m, ACC, 0, c.vf[9]);
    for &n in &v {
        fm::<3, 15>(c, m, n, n, [c.vf[31][3]; 4]);
    }
    fm::<0, 15>(c, m, ACC, 0, [c.vf[29][0]; 4]);
    c.vi[21] = 0x3e99_999a;
    for &n in &v {
        fm::<2, 15>(c, m, n, n, [c.vi[21]; 4]);
    }
}

/// From 0x2A0 (to 0x498, the E bit at 0x490) on the positions vf4 to vf6 and normals vf7 to vf9, from 0x4A0 (to 0x698) on
/// vf14 to vf16 and vf17 to vf19: ITOF4 of the positions (LOI 1.5 with the first) less vf1 to vf3, their squared length
/// (MULA, MADDA, MADD into vf10), its reciprocal square root by the integer seed and one Newton step (ITOF0, MULw, MSUBw,
/// FTOI0, MUL, MUL, MULi, MSUB into vf10), the normals ITOF12, the positions scaled by it, their dot product with the
/// normals into vf10, the normals scaled by that, each less vf30 x times itself from vf28 + its position (ADDA, MSUBx), the
/// last's doubled plus vf30 (ADDA, MADDw) through the same integer step (ADDAz, ITOF0, MSUBw, FTOI0), the first two scaled by
/// it, and the results' x and y lanes from them by vf31 on ACC = vf28 + vf31 w (MADDx.x, MADDx.y to MADDw.y).
#[inline(always)]
fn tie_light(c: &mut Cpu, m: &mut Mac, p: [usize; 3], n: [usize; 3]) {
    for (j, &r) in p.iter().enumerate() {
        itof::<1, 15>(c, r, r);
        if j == 0 {
            c.vi[21] = 0x3fc0_0000;
        }
    }
    for (j, &r) in p.iter().enumerate() {
        fm::<1, 15>(c, m, r, r, c.vf[1 + j]);
    }
    fm::<6, 15>(c, m, ACC, p[0], c.vf[p[0]]);
    fm::<2, 15>(c, m, ACC, p[1], c.vf[p[1]]);
    fm::<2, 15>(c, m, 10, p[2], c.vf[p[2]]);
    fm::<0, 15>(c, m, ACC, 28, [c.vf[31][2]; 4]);
    itof::<0, 15>(c, 11, 10);
    fm::<6, 15>(c, m, 12, 10, [c.vf[31][3]; 4]);
    fm::<3, 15>(c, m, 11, 11, [c.vf[31][3]; 4]);
    ftoi::<0, 15>(c, 10, 11);
    fm::<6, 15>(c, m, 11, 10, c.vf[10]);
    fm::<6, 15>(c, m, 12, 10, c.vf[12]);
    fm::<6, 15>(c, m, ACC, 10, [c.vi[21]; 4]);
    fm::<3, 15>(c, m, 10, 11, c.vf[12]);
    for &r in &n {
        itof::<2, 15>(c, r, r);
    }
    for &r in &p {
        fm::<6, 15>(c, m, r, r, c.vf[10]);
    }
    fm::<6, 15>(c, m, ACC, p[0], c.vf[n[0]]);
    fm::<2, 15>(c, m, ACC, p[1], c.vf[n[1]]);
    fm::<2, 15>(c, m, 10, p[2], c.vf[n[2]]);
    for &r in &n {
        fm::<6, 15>(c, m, r, r, c.vf[10]);
    }
    for j in 0..3 {
        fm::<0, 15>(c, m, ACC, 28, c.vf[p[j]]);
        fm::<3, 15>(c, m, n[j], n[j], [c.vf[30][0]; 4]);
    }
    fm::<0, 15>(c, m, ACC, n[2], c.vf[n[2]]);
    fm::<2, 15>(c, m, n[2], 30, [c.vf[0][3]; 4]);
    fm::<0, 15>(c, m, ACC, 28, [c.vf[31][2]; 4]);
    itof::<0, 15>(c, n[2], n[2]);
    fm::<3, 15>(c, m, n[2], n[2], [c.vf[31][3]; 4]);
    ftoi::<0, 15>(c, n[2], n[2]);
    fm::<0, 15>(c, m, ACC, 28, [c.vf[31][3]; 4]);
    fm::<6, 15>(c, m, n[0], n[0], c.vf[n[2]]);
    fm::<6, 15>(c, m, n[1], n[1], c.vf[n[2]]);
    fm::<2, 8>(c, m, p[2], 31, [c.vf[n[0]][0]; 4]);
    fm::<2, 4>(c, m, p[2], 31, [c.vf[n[1]][0]; 4]);
    fm::<2, 8>(c, m, n[0], 31, [c.vf[n[0]][1]; 4]);
    fm::<2, 4>(c, m, n[0], 31, [c.vf[n[1]][1]; 4]);
    fm::<2, 8>(c, m, n[1], 31, [c.vf[n[0]][2]; 4]);
    fm::<2, 4>(c, m, n[1], 31, [c.vf[n[1]][2]; 4]);
    fm::<2, 8>(c, m, n[2], 31, [c.vf[n[0]][3]; 4]);
    fm::<2, 4>(c, m, n[2], 31, [c.vf[n[1]][3]; 4]);
}

// ---- math (micro 0xC80): the library's rotation, sine and cosine, and the rest -----------------------------------------------

fn math(c: &mut Cpu, a: usize) -> (usize, bool) {
    let mut m = Mac::default();
    let end = match a {
        0xc80 => {
            math_c80(c, &mut m);
            0xd60
        }
        0xda0 => {
            math_da0(c, &mut m);
            0xe48
        }
        0xdb0 => {
            math_db0(c, &mut m);
            0xe48
        }
        0xe98 => {
            math_e98(c, &mut m);
            0xf08
        }
        0xf10 => math_f10(c, &mut m),
        _ => panic!("VU0 microprogram math started at {a:#x}: not a start the game makes (0xC80, 0xDA0, 0xDB0, 0xE98, 0xF10)"),
    };
    m.land(c);
    (end, false)
}

/// From 0xC80 (to 0xD60, the E bit at 0xD58): the rotation matrix of the angles vf1 x, y, z into vf20 to vf22 (vf23 vf0).
/// Each angle that is not zero (FMAND of 0xC80's zero flags into vi11 to vi13; IBNE past it) has its sine and cosine made
/// (BAL vi15 to the series at 0xD68) and its rotation put in (the x one into vf21 and vf22 directly; the y and z ones by the
/// matrix product at 0xE50, also by BAL vi15). Each JR vi15 comes back to the pair after its BAL's slot, which is where its
/// BAL set vi15 (no pair between writes vi15).
#[inline(always)]
fn math_c80(c: &mut Cpu, m: &mut Mac) {
    // 0C80: ADD.xyz vf31, vf1, vf0 | MR32 vf22, vf0
    let u = m_fmac::<0, 14>(c.vf[1], c.vf[0], c.vacc);
    c.vf[22] = rot(c.vf[0]);
    m_put::<14>(&mut c.vf[31], u);
    let zero = m.put::<14>(u);
    // 0C88 to 0C98: IADDIU vi11, vi0, 8; vi12, 4; vi13, 2
    c.set_vi(11, c.vi[0].wrapping_add(8));
    c.set_vi(12, c.vi[0].wrapping_add(4));
    c.set_vi(13, c.vi[0].wrapping_add(2));
    // 0CA0: MULx.yzw vf20, vf0, vf0x | FMAND vi11, vi11 (0xC80's flags)
    let u = m_fmac::<6, 7>(c.vf[0], [c.vf[0][0]; 4], c.vacc);
    c.set_vi(11, zero & c.vi[11]);
    m_put::<7>(&mut c.vf[20], u);
    m.put::<7>(u);
    // 0CA8: MULx.xzw vf21, vf0, vf0x | FMAND vi12, vi12
    let u = m_fmac::<6, 11>(c.vf[0], [c.vf[0][0]; 4], c.vacc);
    c.set_vi(12, zero & c.vi[12]);
    m_put::<11>(&mut c.vf[21], u);
    m.put::<11>(u);
    // 0CB0: ADDw.x vf20, vf0, vf0w | IBNE vi0, vi11 to 0xCF0
    let u = m_fmac::<0, 8>(c.vf[0], [c.vf[0][3]; 4], c.vacc);
    let skip_x = ibne0(c, 11);
    m_put::<8>(&mut c.vf[20], u);
    m.put::<8>(u);
    // 0CB8 (its slot): ADDw.y vf21, vf0, vf0w | FMAND vi13, vi13
    let u = m_fmac::<0, 4>(c.vf[0], [c.vf[0][3]; 4], c.vacc);
    c.set_vi(13, zero & c.vi[13]);
    m_put::<4>(&mut c.vf[21], u);
    m.put::<4>(u);
    if !skip_x {
        // 0CC0: BAL vi15, 0xD68 (vi15 0xCD0); 0CC8, its slot, nothing
        c.set_vi(15, 410);
        sincos(c, m);
        // 0CD0: ADDx.y vf21, vf0, vf1x
        fm::<0, 4>(c, m, 21, 0, [c.vf[1][0]; 4]);
        // 0CD8: ADDy.z vf21, vf0, vf1y
        fm::<0, 2>(c, m, 21, 0, [c.vf[1][1]; 4]);
        // 0CE0: ADDx.z vf22, vf0, vf1x
        fm::<0, 2>(c, m, 22, 0, [c.vf[1][0]; 4]);
        // 0CE8: SUBy.y vf22, vf0, vf1y
        fm::<1, 4>(c, m, 22, 0, [c.vf[1][1]; 4]);
    }
    // 0CF0: ADDy.x vf1, vf0, vf31y | IBNE vi0, vi12 to 0xD28
    let u = m_fmac::<0, 8>(c.vf[0], [c.vf[31][1]; 4], c.vacc);
    let skip_y = ibne0(c, 12);
    m_put::<8>(&mut c.vf[1], u);
    m.put::<8>(u);
    // 0CF8 (its slot): SUB vf25, vf0, vf0 | MFIR vf24, vi0
    let u = m_fmac::<1, 15>(c.vf[0], c.vf[0], c.vacc);
    c.vf[24] = [c.vi[0] as u16 as i16 as i32 as u32; 4];
    m_put::<15>(&mut c.vf[25], u);
    m.put::<15>(u);
    if !skip_y {
        // 0D00: SUB vf26, vf0, vf0 | BAL vi15, 0xD68 (vi15 0xD10)
        let u = m_fmac::<1, 15>(c.vf[0], c.vf[0], c.vacc);
        c.set_vi(15, 418);
        m_put::<15>(&mut c.vf[26], u);
        m.put::<15>(u);
        // 0D08 (its slot): ADDw.y vf25, vf0, vf0w
        fm::<0, 4>(c, m, 25, 0, [c.vf[0][3]; 4]);
        sincos(c, m);
        // 0D10: ADDy.x vf26, vf0, vf1y | MOVE.x vf24, vf1
        let u = m_fmac::<0, 8>(c.vf[0], [c.vf[1][1]; 4], c.vacc);
        let v = c.vf[1];
        m_put::<8>(&mut c.vf[24], v);
        m_put::<8>(&mut c.vf[26], u);
        m.put::<8>(u);
        // 0D18: SUBy.z vf24, vf0, vf1y | BAL vi15, 0xE50 (vi15 0xD28)
        let u = m_fmac::<1, 2>(c.vf[0], [c.vf[1][1]; 4], c.vacc);
        c.set_vi(15, 421);
        m_put::<2>(&mut c.vf[24], u);
        m.put::<2>(u);
        // 0D20 (its slot): ADDx.z vf26, vf0, vf1x
        fm::<0, 2>(c, m, 26, 0, [c.vf[1][0]; 4]);
        rows(c, m);
    }
    // 0D28: ADDz.x vf1, vf0, vf31z | IBNE vi0, vi13 to 0xD58
    let u = m_fmac::<0, 8>(c.vf[0], [c.vf[31][2]; 4], c.vacc);
    let skip_z = ibne0(c, 13);
    m_put::<8>(&mut c.vf[1], u);
    m.put::<8>(u);
    // 0D30 (its slot): MR32 vf26, vf0
    c.vf[26] = rot(c.vf[0]);
    if !skip_z {
        // 0D38: MULx vf24, vf0, vf0x | BAL vi15, 0xD68 (vi15 0xD48)
        let u = m_fmac::<6, 15>(c.vf[0], [c.vf[0][0]; 4], c.vacc);
        c.set_vi(15, 425);
        m_put::<15>(&mut c.vf[24], u);
        m.put::<15>(u);
        // 0D40 (its slot): MULx vf25, vf0, vf0x
        fm::<6, 15>(c, m, 25, 0, [c.vf[0][0]; 4]);
        sincos(c, m);
        // 0D48: ADDx.y vf25, vf0, vf1x | BAL vi15, 0xE50 (vi15 0xD58)
        let u = m_fmac::<0, 4>(c.vf[0], [c.vf[1][0]; 4], c.vacc);
        c.set_vi(15, 427);
        m_put::<4>(&mut c.vf[25], u);
        m.put::<4>(u);
        // 0D50 (its slot): SUBy.x vf25, vf0, vf1y | MOVE.xy vf24, vf1
        let u = m_fmac::<1, 8>(c.vf[0], [c.vf[1][1]; 4], c.vacc);
        let v = c.vf[1];
        m_put::<12>(&mut c.vf[24], v);
        m_put::<8>(&mut c.vf[25], u);
        m.put::<8>(u);
        rows(c, m);
    }
    // 0D58: MOVE vf23, vf0 (the E bit); 0D60, its slot, nothing
    c.vf[23] = c.vf[0];
}

/// The subroutine at 0xD68, BAL vi15 to it, its JR vi15 at 0xE30: vf1 y = x and x + pi/2 (the cosine's angle; LOI pi/2,
/// then LOI pi), then (B 0xDC0, IADDIU vi1, vi0, 0 in its slot) the series with vi1 0, which does not branch at 0xE20.
#[inline(always)]
fn sincos(c: &mut Cpu, m: &mut Mac) {
    // 0D68: ADDx.y vf1, vf0, vf1x | LOI 0x3FC90FDB
    fm::<0, 4>(c, m, 1, 0, [c.vf[1][0]; 4]);
    c.vi[21] = 0x3fc9_0fdb;
    // 0D70: ADDi.x vf1, vf1, I | LOI 0x40490FDB
    fm::<0, 8>(c, m, 1, 1, [c.vi[21]; 4]);
    c.vi[21] = 0x4049_0fdb;
    // 0D78: B 0xDC0; 0D80 (its slot): ADDAi ACC, vf0, I | IADDIU vi1, vi0, 0
    let u = m_fmac::<0, 15>(c.vf[0], [c.vi[21]; 4], c.vacc);
    c.set_vi(1, c.vi[0].wrapping_add(0));
    m_put::<15>(&mut c.vacc, u);
    m.put::<15>(u);
    series(c, m);
}

/// From 0xDA0 (to 0xE48): vf1 y = x, x + pi/2 (LOI pi/2), then 0xDB0.
#[inline(always)]
fn math_da0(c: &mut Cpu, m: &mut Mac) {
    // 0DA0: ADDx.y vf1, vf0, vf1x | LOI 0x3FC90FDB
    fm::<0, 4>(c, m, 1, 0, [c.vf[1][0]; 4]);
    c.vi[21] = 0x3fc9_0fdb;
    // 0DA8: ADDi.x vf1, vf1, I
    fm::<0, 8>(c, m, 1, 1, [c.vi[21]; 4]);
    math_db0(c, m);
}

/// From 0xDB0 (to 0xE48, the E bit at 0xE40): LOI pi, ACC = pi and vi1 1, then the series, which branches at 0xE20 (vi1 is
/// not vi0) to the E bit.
#[inline(always)]
fn math_db0(c: &mut Cpu, m: &mut Mac) {
    // 0DB0: LOI 0x40490FDB
    c.vi[21] = 0x4049_0fdb;
    // 0DB8: ADDAi ACC, vf0, I | IADDIU vi1, vi0, 1
    let u = m_fmac::<0, 15>(c.vf[0], [c.vi[21]; 4], c.vacc);
    c.set_vi(1, c.vi[0].wrapping_add(1));
    m_put::<15>(&mut c.vacc, u);
    m.put::<15>(u);
    series(c, m);
}

/// 0xDC0 to 0xE28, then 0xE30 and 0xE38 (vi1 0: IBNE vi0, vi1 at 0xE20 not taken, JR vi15 at 0xE30) or 0xE40 and 0xE48
/// (vi1 1: taken, the E bit), the same two instructions: vf1 (ACC pi) brought into -pi..pi by MSUBw, SUBAi, MSUBw, MINI,
/// MAX, then the odd series in it (LOIs 0xB94FB21F, 0x362E9C14, 0xBE2AAAA4, 0x3C08873E) into vf1.
#[inline(always)]
fn series(c: &mut Cpu, m: &mut Mac) {
    // 0DC0: MSUBw vf2, vf1, vf0w
    fm::<3, 15>(c, m, 2, 1, [c.vf[0][3]; 4]);
    // 0DC8: SUBAi ACC, vf0, I
    fm::<1, 15>(c, m, ACC, 0, [c.vi[21]; 4]);
    // 0DD0: MSUBw vf3, vf1, vf0w
    fm::<3, 15>(c, m, 3, 1, [c.vf[0][3]; 4]);
    // 0DD8: MINI vf1, vf1, vf2
    fm::<5, 15>(c, m, 1, 1, c.vf[2]);
    // 0DE0: MAX vf1, vf1, vf3
    fm::<4, 15>(c, m, 1, 1, c.vf[3]);
    // 0DE8: MUL vf2, vf1, vf1 | LOI 0xB94FB21F
    fm::<6, 15>(c, m, 2, 1, c.vf[1]);
    c.vi[21] = 0xb94f_b21f;
    // 0DF0: MULi vf5, vf1, I | LOI 0x362E9C14
    fm::<6, 15>(c, m, 5, 1, [c.vi[21]; 4]);
    c.vi[21] = 0x362e_9c14;
    // 0DF8: MULi vf6, vf1, I | LOI 0xBE2AAAA4
    fm::<6, 15>(c, m, 6, 1, [c.vi[21]; 4]);
    c.vi[21] = 0xbe2a_aaa4;
    // 0E00: MULi vf3, vf1, I
    fm::<6, 15>(c, m, 3, 1, [c.vi[21]; 4]);
    // 0E08: MUL vf4, vf2, vf2 | LOI 0x3C08873E
    fm::<6, 15>(c, m, 4, 2, c.vf[2]);
    c.vi[21] = 0x3c08_873e;
    // 0E10: MULAi ACC, vf1, I
    fm::<6, 15>(c, m, ACC, 1, [c.vi[21]; 4]);
    // 0E18: MADDA ACC, vf5, vf2
    fm::<2, 15>(c, m, ACC, 5, c.vf[2]);
    // 0E20: MADD vf6, vf6, vf4 | IBNE vi0, vi1 to 0xE40
    fm::<2, 15>(c, m, 6, 6, c.vf[4]);
    // 0E28 (its slot): MULAw ACC, vf1, vf0w
    fm::<6, 15>(c, m, ACC, 1, [c.vf[0][3]; 4]);
    // 0E30 (JR vi15) or 0E40 (the E bit): MADDA ACC, vf3, vf2
    fm::<2, 15>(c, m, ACC, 3, c.vf[2]);
    // 0E38 or 0E48 (the slot): MADD vf1, vf6, vf4
    fm::<2, 15>(c, m, 1, 6, c.vf[4]);
}

/// The subroutine at 0xE50, BAL vi15 to it, its JR vi15 at 0xE88: the rows vf20 to vf22 turned by vf24 to vf26 (MULAx,
/// MADDAy, MADDz each).
#[inline(always)]
fn rows(c: &mut Cpu, m: &mut Mac) {
    for r in 20..23 {
        fm::<6, 15>(c, m, ACC, 24, [c.vf[r][0]; 4]);
        fm::<2, 15>(c, m, ACC, 25, [c.vf[r][1]; 4]);
        fm::<2, 15>(c, m, r, 26, [c.vf[r][2]; 4]);
    }
}

/// From 0xE98 (to 0xF08, the E bit at 0xF00), pair by pair.
#[inline(always)]
fn math_e98(c: &mut Cpu, m: &mut Mac) {
    // 0E98: MULA ACC, vf8, vf8 | LOI 0x3FB504F3
    fm::<6, 15>(c, m, ACC, 8, c.vf[8]);
    c.vi[21] = 0x3fb5_04f3;
    // 0EA0: MULi vf3, vf8, I | MR32.w vf14, vf0
    let u = m_fmac::<6, 15>(c.vf[8], [c.vi[21]; 4], c.vacc);
    let v = rot(c.vf[0]);
    m_put::<1>(&mut c.vf[14], v);
    m_put::<15>(&mut c.vf[3], u);
    m.put::<15>(u);
    // 0EA8: MADD vf2, vf8, vf8
    fm::<2, 15>(c, m, 2, 8, c.vf[8]);
    // 0EB0: ADDw vf4, vf0, vf0w
    fm::<0, 15>(c, m, 4, 0, [c.vf[0][3]; 4]);
    // 0EB8: OPMULA.xyz ACC, vf3, vf3
    let u = m_opmula(c.vf[3], c.vf[3]);
    m_put::<14>(&mut c.vacc, u);
    m.put::<14>(u);
    // 0EC0: MSUBw vf5, vf3, vf3w | MR32.w vf16, vf0
    let u = m_fmac::<3, 15>(c.vf[3], [c.vf[3][3]; 4], c.vacc);
    let v = rot(c.vf[0]);
    m_put::<1>(&mut c.vf[16], v);
    m_put::<15>(&mut c.vf[5], u);
    m.put::<15>(u);
    // 0EC8: MADDw vf6, vf3, vf3w | MR32.w vf15, vf0
    let u = m_fmac::<2, 15>(c.vf[3], [c.vf[3][3]; 4], c.vacc);
    let v = rot(c.vf[0]);
    m_put::<1>(&mut c.vf[15], v);
    m_put::<15>(&mut c.vf[6], u);
    m.put::<15>(u);
    // 0ED0: ADDAw.xyz ACC, vf0, vf0w
    fm::<0, 14>(c, m, ACC, 0, [c.vf[0][3]; 4]);
    // 0ED8: MSUBAx.yz ACC, vf4, vf2x
    fm::<3, 6>(c, m, ACC, 4, [c.vf[2][0]; 4]);
    // 0EE0: MSUBy.z vf16, vf4, vf2y | MR32 vf3, vf5
    let u = m_fmac::<3, 2>(c.vf[4], [c.vf[2][1]; 4], c.vacc);
    c.vf[3] = rot(c.vf[5]);
    m_put::<2>(&mut c.vf[16], u);
    m.put::<2>(u);
    // 0EE8: MSUBAy.xz ACC, vf4, vf2y | MR32 vf7, vf6
    let u = m_fmac::<3, 10>(c.vf[4], [c.vf[2][1]; 4], c.vacc);
    c.vf[7] = rot(c.vf[6]);
    m_put::<10>(&mut c.vacc, u);
    m.put::<10>(u);
    // 0EF0: MSUBz.y vf15, vf4, vf2z | MR32.y vf14, vf5
    let u = m_fmac::<3, 4>(c.vf[4], [c.vf[2][2]; 4], c.vacc);
    let v = rot(c.vf[5]);
    m_put::<4>(&mut c.vf[14], v);
    m_put::<4>(&mut c.vf[15], u);
    m.put::<4>(u);
    // 0EF8: MSUBz.x vf14, vf4, vf2z | MR32.x vf16, vf5
    let u = m_fmac::<3, 8>(c.vf[4], [c.vf[2][2]; 4], c.vacc);
    let v = rot(c.vf[5]);
    m_put::<8>(&mut c.vf[16], v);
    m_put::<8>(&mut c.vf[14], u);
    m.put::<8>(u);
    // 0F00: ADDy.z vf14, vf0, vf6y | MR32.z vf15, vf3 (the E bit)
    let u = m_fmac::<0, 2>(c.vf[0], [c.vf[6][1]; 4], c.vacc);
    let v = rot(c.vf[3]);
    m_put::<2>(&mut c.vf[15], v);
    m_put::<2>(&mut c.vf[14], u);
    m.put::<2>(u);
    // 0F08 (its slot): ADDx.y vf16, vf0, vf6x | MR32.x vf15, vf7
    let u = m_fmac::<0, 4>(c.vf[0], [c.vf[6][0]; 4], c.vacc);
    let v = rot(c.vf[7]);
    m_put::<8>(&mut c.vf[15], v);
    m_put::<4>(&mut c.vf[16], u);
    m.put::<4>(u);
}

/// From 0xF10 (to 0xF68, the E bit at 0xF60, or, vf1 x negative, IBNE vi0, vi1 at 0xF50 taken, to 0xF78, the E bit at
/// 0xF70): the polynomial in |vf1 x| times the square root of 1 - |x| (SQRT into Q at 0xF40, landing at 0xF58) taken from
/// 1.57 (LOI 0x3FC8F5C3) or added to -1.57, the sign from FMAND of 0xF18's sign flag into vi1.
#[inline(always)]
fn math_f10(c: &mut Cpu, m: &mut Mac) -> usize {
    // 0F10: ABS.x vf4, vf1 | LOI 0x3FC90DA4
    let u = m_abs(c.vf[1]);
    m_put::<8>(&mut c.vf[4], u);
    c.vi[21] = 0x3fc9_0da4;
    // 0F18: ADD.x vf0, vf1, vf0 (its flags only)
    let u = m_fmac::<0, 8>(c.vf[1], c.vf[0], c.vacc);
    let flags = m.put::<8>(u);
    // 0F20: ADDAi.x ACC, vf0, I | IADDIU vi1, vi0, 0x80
    let u = m_fmac::<0, 8>(c.vf[0], [c.vi[21]; 4], c.vacc);
    c.set_vi(1, c.vi[0].wrapping_add(128));
    m_put::<8>(&mut c.vacc, u);
    m.put::<8>(u);
    // 0F28: SUBx.w vf1, vf0, vf4x | LOI 0xBC996E30
    fm::<1, 1>(c, m, 1, 0, [c.vf[4][0]; 4]);
    c.vi[21] = 0xbc99_6e30;
    // 0F30: MUL.x vf2, vf1, vf1 | FMAND vi1, vi1 (0xF18's flags)
    let u = m_fmac::<6, 8>(c.vf[1], c.vf[1], c.vacc);
    c.set_vi(1, flags & c.vi[1]);
    m_put::<8>(&mut c.vf[2], u);
    m.put::<8>(u);
    // 0F38: MULi.x vf3, vf4, I | LOI 0xBE593484
    fm::<6, 8>(c, m, 3, 4, [c.vi[21]; 4]);
    c.vi[21] = 0xbe59_3484;
    // 0F40: SQRT Q, vf1w
    let q = m_q::<1>(c, 0, c.vf[1][3]);
    // 0F48: MADDAi.x ACC, vf4, I | LOI 0x3D981627
    fm::<2, 8>(c, m, ACC, 4, [c.vi[21]; 4]);
    c.vi[21] = 0x3d98_1627;
    // 0F50: MADDAi.x ACC, vf2, I | IBNE vi0, vi1 to 0xF70
    let u = m_fmac::<2, 8>(c.vf[2], [c.vi[21]; 4], c.vacc);
    let negative = ibne0(c, 1);
    m_put::<8>(&mut c.vacc, u);
    m.put::<8>(u);
    // 0F58 (its slot): Q lands; MADD.x vf1, vf2, vf3 | LOI 0x3FC8F5C3
    c.vi[22] = q;
    fm::<2, 8>(c, m, 1, 2, c.vf[3]);
    c.vi[21] = 0x3fc8_f5c3;
    if negative {
        // 0F70: SUBAi.x ACC, vf0, I (the E bit)
        fm::<1, 8>(c, m, ACC, 0, [c.vi[21]; 4]);
        // 0F78 (its slot): MADDq.x vf1, vf1, Q
        fm::<2, 8>(c, m, 1, 1, [c.vi[22]; 4]);
        0xf78
    } else {
        // 0F60: ADDAi.x ACC, vf0, I (the E bit)
        fm::<0, 8>(c, m, ACC, 0, [c.vi[21]; 4]);
        // 0F68 (its slot): MSUBq.x vf1, vf1, Q
        fm::<3, 8>(c, m, 1, 1, [c.vi[22]; 4]);
        0xf68
    }
}

// ---- UYA_VU0_CHECK -----------------------------------------------------------------------------------------------------

/// What a run can leave: VU0's registers, its data memory, where the run stopped and whether it waits.
struct State {
    vf: [[u32; 4]; 32],
    vacc: [u32; 4],
    vi: [u32; 32],
    vu0d: Vec<u8>,
    parked: Option<usize>,
    ret: (usize, bool),
}

impl State {
    fn take(c: &mut Cpu, ret: (usize, bool)) -> State {
        let (vf, vacc, vi) = (c.vf, c.vacc, c.vi);
        let m = c.mach();
        State { vf, vacc, vi, vu0d: m.vu0d.clone(), parked: m.vu0_parked, ret }
    }

    fn put(&self, c: &mut Cpu) {
        (c.vf, c.vacc, c.vi) = (self.vf, self.vacc, self.vi);
        let m = c.mach();
        m.vu0d.copy_from_slice(&self.vu0d);
        m.vu0_parked = self.parked;
    }
}

/// What differs between the translation's state `want` and the native run's `got`.
fn differences(want: &State, got: &State) -> Vec<String> {
    let mut d = Vec::new();
    for k in 0..32 {
        if want.vf[k] != got.vf[k] {
            d.push(format!("vf{k} {:08x?}/{:08x?}", want.vf[k], got.vf[k]));
        }
    }
    if want.vacc != got.vacc {
        d.push(format!("ACC {:08x?}/{:08x?}", want.vacc, got.vacc));
    }
    for k in 0..32 {
        if want.vi[k] != got.vi[k] {
            d.push(format!("vi{k} {:x}/{:x}", want.vi[k], got.vi[k]));
        }
    }
    if let Some(i) = (0..want.vu0d.len()).find(|&i| want.vu0d[i] != got.vu0d[i]) {
        let n = (0..want.vu0d.len()).filter(|&i| want.vu0d[i] != got.vu0d[i]).count();
        d.push(format!("VU0 data: {n} bytes from {i:#x}"));
    }
    if want.ret != got.ret || want.parked != got.parked {
        d.push(format!("stopped {:#x?}/{:#x?} waiting {:?}/{:?}", want.ret, got.ret, want.parked, got.parked));
    }
    d
}

struct Check {
    left: u64,
    done: u64,
    differ: u64,
    /// Runs checked and differing by program and start.
    runs: BTreeMap<(&'static str, usize), (u64, u64)>,
    last: std::time::Instant,
}

static CHECK: Mutex<Option<Check>> = Mutex::new(None);

/// UYA_VU0_CHECK=N, read once: N runs of the three programs checked.
fn checked() -> Option<u64> {
    static C: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_VU0_CHECK").ok()?;
        Some(v.parse().unwrap_or_else(|_| panic!("UYA_VU0_CHECK={v}: N is a number of runs")))
    })
}

fn summary(ck: &Check) {
    let per: Vec<String> = ck.runs.iter().map(|((name, a), (n, d))| format!("{name} {a:#x} {n}/{d}")).collect();
    println!("UYA_VU0_CHECK: {} runs checked, {} differ (checked/differing: {})", ck.done, ck.differ, per.join(", "));
}

/// A run of program `p` (native code `f`) from micro address `a`, its translation's node `node`: natively, or (checking) the
/// translation, then the native code from the same state, compared, the game going on from the translation's state.
pub(crate) fn run(c: &mut Cpu, f: Native, p: &Vu0Prog, node: u32, a: usize) -> (usize, bool) {
    if let Some(n) = checked() {
        let mut g = CHECK.lock().expect("the VU0 check");
        let ck = g.get_or_insert_with(|| Check { left: n, done: 0, differ: 0, runs: BTreeMap::new(), last: std::time::Instant::now() });
        if ck.left > 0 {
            ck.left -= 1;
            drop(g);
            return check(c, f, p, node, a);
        }
    }
    f(c, a)
}

fn check(c: &mut Cpu, f: Native, p: &Vu0Prog, node: u32, a: usize) -> (usize, bool) {
    let before = State::take(c, (0, false));
    let r = (p.run)(c, node);
    let want = State::take(c, r);
    before.put(c);
    let r2 = f(c, a);
    let got = State::take(c, r2);
    let d = differences(&want, &got);
    want.put(c);
    let mut g = CHECK.lock().expect("the VU0 check");
    let ck = g.as_mut().expect("checking");
    ck.done += 1;
    let e = ck.runs.entry((p.name, a)).or_default();
    e.0 += 1;
    if !d.is_empty() {
        e.1 += 1;
        ck.differ += 1;
        println!("UYA_VU0_CHECK: run {} ({} from {a:#x}) differs: {}", ck.done, p.name, d.join("; "));
    }
    if ck.left == 0 || ck.last.elapsed().as_secs() >= 2 {
        ck.last = std::time::Instant::now();
        summary(ck);
    }
    r
}
