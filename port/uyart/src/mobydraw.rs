//! The moby draw path (level 1 0x438120 and everything under it), done natively in front of its translation (port/uyatr
//! `JOBS`): the cull and draw list (0x43EA78), the joint matrices (0x439E00) and the skin job (0x3F6FF0) for each draw record
//! (0x43E998), the second list of 0x437B48 (0x4A0B88, the pose reader 0x43C028, 0x4A0830 and its packets), the class texture
//! uploads (0x437C88), and the glue between them. The translated wrapper at 0x438120 asks `start` (`Rt::native` 4), the one at
//! its second half 0x4380C0 (called on its own by 0x4009B8, 0x497828 and 0x4BCCE0) asks with 6; both answer `after` (5, 7)
//! once the translation ran when `start` let it run. `c.ret` brings the function's address with 1 in its low bit and 2 when
//! the program's view is widened (a story level: `VIEW_LOADS` "the moby cull", `DRAWN`; the front ends' is the game's own).
//!
//! The functions are the same code in every program (uyatr finds them by size and shape); what relocation changes in them, the
//! addresses of the globals and of boot data, the gp offsets and the call targets, is read from the code in RAM (`F`), named by
//! level 1's address of the instruction holding it.
//!
//! What crosses the unit's boundary is made as the game makes it: RAM (the draw records, the VIF1 chain, the skinned output
//! arrays, the globals, the moby cull's drawn byte), the scratchpad (the game's structures in it carry scratchpad addresses: the
//! joint table's parent matrices, the pose readers' lists), VU0's data memory (the skin job's matrices, which later rows and
//! jobs read), the DMA channels' registers, D_STAT and the end interrupts as the port's kicks leave them, and the renderer's
//! skin recordings (`hw::SkinJoints`). What goes away is the PS2's traffic: the DMA kicks through the hardware registers and
//! their polling (a copy is made where the kick was), the skin job's VU0 microprogram (its arithmetic is done here), the
//! flags of VU0's macro instructions no code reads.
//!
//! With UYA_NATIVE_CHECK=<job>:N (job `moby`: the calls of 0x438120; `moby2`: the calls of 0x4380C0 on its own) the first N
//! calls run twice from the same state: the translation, then the native unit on the state as it was before; every difference
//! in what the game reads afterwards (`differences`) is reported with what the calls covered, and the game goes on from the
//! translation's state. Both runs go with the thread's interrupts held (`Check::ints`), so that what the host's clock decides
//! (the field's end, the timers' interrupts) lands after them, once, and not inside one of them.
//!
//! DRAW DISTANCE FAR (`drawdist`, port/uya `settings.rs`): the moby cull's far test (0x43EE70's block) is the sign of vf3.y =
//! (D x 1024 - r) - (z - r), D = min(the moby's +0x32, gp+0x3820) (the PMINW with s7; the level's setup stores 960 there on
//! Veldin, and most mobies' own is 64, the gameplay record's +0x30), r the sphere's radius and z its centre's view depth (both
//! x 1024), read by the QMFC2 of vf3 and its branch: a moby whose nearest point is past D is culled. With FAR, vf19.y (D x
//! 1024 - r, the SUB.y after the ITOF0) is at least the reach, unless D is not above 0, so a moby is drawn while its nearest
//! point is within the reach. Not for every moby the game's own D culls, though, because the game leans on D where nothing
//! else would stop its code:
//! - a moby with no class (+0x24 zero: one with no model, or one being spawned) has a zero sphere, at the world origin, which
//!   D always culls from where the game is played. Drawn, its LOD table (0x43FBC8) and meshes come from address 0, and the
//!   mesh loop (0x43F680, until s1 reaches s0) never ends: the chain pointer walks through all of RAM into the hardware
//!   registers (on Veldin with the camera turned to the origin from the middle of the level). FAR leaves such a moby at D.
//! - what the cull fills, it fills for the mobies the game draws itself first: the moby chain up to gp+0x483C's end (tested
//!   before each moby, 0x43EFA0: past it the cull ends, the second pass's near mobies with it), the job list (from 0x1DFBF0's
//!   word) up to the second list's entries (gp+0x3344, 60 KB: the cull never tests it) and the near list's 128 entries
//!   (scratchpad 0x1000 to 0x1800: a near-flag moby past them is culled). FAR's extra mobies drawn in the game's own passes
//!   filled them first: on Obani Draco's corridor its lamps and robots took the near list and the chain before the second pass,
//!   and Ratchet, the crate's hologram and the walkway were not drawn (likewise on Mylon; a v0.1.4 player with FAR on, on
//!   Veldin and Florana: "ratchet is sometimes invisible"). So a moby inside the reach but past its own limit is culled in the game's own
//!   passes as the game culls it, and drawn after them in an extras' pass (`f43ea78`'s `extras`): each as the first pass draws
//!   a moby, a near-flag one from the near list's slot 0 as the second pass draws an entry (the game's near list kept aside
//!   and put back), while the job list has 2 KB left (a record in each pass) and until the chain's end ends the pass. The game's
//!   own mobies get the room they get at ORIGINAL. The chain's end is 72 KB short of the job list (on Mylon, 0x674000 and
//!   0x686000): the one moby a pre-draw test lets past it (23 KB at most seen there) stays inside the frame's buffer.
//! - the near list's entries with a drop-shadow mark go to 0x21F0E0 at the cull's end (0x43FA90), 48 of them before the
//!   texture list (0x21F260), which past them the game's code overwrites; with FAR the copy ends there (only the game's own
//!   mobies are in the list).
//!
//! The fade over the last 16 units before the limit (0x43EFA0: vf3.y's FTOI0 >> 7, at most 0x80,
//! times +0x23) comes with it, and the near list's fades (0x43FD30, 0x43FFB0: +0x32 x 1024 less the depth) read the draw
//! distance at least the reach (`far_distance`). The drawn byte (+0x31, which the game's code acts on: its targets, the drop
//! shadows, debris removed off screen) stays the game's own decision (`drawn`), as with the widened view. What is not the far
//! test stays the game's: the near and far meshes (+0x62 x 1024 against the depth, 0x43F128), the near and side tests, the
//! far clip (a moby across it is sent to program 0's clipping run; FAR's reach is never past it), and what bounds the cull:
//! the chain's end and the near list's 128 entries (above: FAR's extras get what the game's own leave). UYA_FAR_LOG sums what
//! the culls did (`FarLog`). A check of the cull (`moby`) stops the game while FAR is on
//! (`drawdist::refuse_check`); `moby2`, which has no cull, is the same at both.

use crate::{Cpu, Ext};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use uycpu::vu0::{m_fmac, m_ftoi, m_mac, m_put};

type Q = (u64, u64);

// ---- the code in RAM ------------------------------------------------------------------------------------------------------

/// A function of the unit as it lies in RAM: its address there and level 1's, by which this file names its instructions.
#[derive(Clone, Copy)]
struct F {
    at: u32,
    l1: u32,
}

impl F {
    /// The instruction at level 1's `pc`, in this program (code is in RAM).
    #[inline(always)]
    fn w(self, c: &Cpu, pc: u32) -> u32 {
        let a = self.at.wrapping_add(pc.wrapping_sub(self.l1));
        assert!(a < uycpu::RAM_SIZE - 4, "the moby draw path's code at {a:08x}: not in RAM");
        unsafe { (c.ram.add(a as usize) as *const u32).read_unaligned() }
    }
    /// LUI's value.
    #[inline(always)]
    fn hi(self, c: &Cpu, pc: u32) -> u32 {
        self.w(c, pc) << 16
    }
    /// The 16-bit immediate, sign-extended (an address's low part or a gp offset).
    #[inline(always)]
    fn lo(self, c: &Cpu, pc: u32) -> u32 {
        self.w(c, pc) as u16 as i16 as i32 as u32
    }
    /// A LUI's value plus the low part of the instruction at `low`.
    #[inline(always)]
    fn a(self, c: &Cpu, lui: u32, low: u32) -> u32 {
        self.hi(c, lui).wrapping_add(self.lo(c, low))
    }
    /// gp plus the offset of the instruction at `pc`.
    #[inline(always)]
    fn gp(self, c: &Cpu, gp: u32, pc: u32) -> u32 {
        gp.wrapping_add(self.lo(c, pc))
    }
    /// The function the JAL at `pc` calls, named by level 1's address `l1`.
    #[inline(always)]
    fn call(self, c: &Cpu, pc: u32, l1: u32) -> F {
        F { at: (self.w(c, pc) & 0x03ff_ffff) << 2, l1 }
    }
    /// This program's address of level 1's `pc` (a return address, a routine's address).
    #[inline(always)]
    fn pc(self, pc: u32) -> u32 {
        self.at.wrapping_add(pc.wrapping_sub(self.l1))
    }
}

// ---- 128-bit values, VU0's macro instructions ------------------------------------------------------------------------------

#[inline(always)]
fn w4(q: Q) -> [u32; 4] {
    [q.0 as u32, (q.0 >> 32) as u32, q.1 as u32, (q.1 >> 32) as u32]
}
#[inline(always)]
fn q4(v: [u32; 4]) -> Q {
    (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32)
}
/// A 32-bit result as the R5900 keeps it in 64 bits.
#[inline(always)]
fn sx(v: u32) -> u64 {
    v as i32 as i64 as u64
}

const ACC: usize = 32;

/// An FMAC macro instruction (K: 0 ADD, 1 SUB, 2 MADD, 3 MSUB, 4 MAX, 5 MINI, 6 MUL) on lanes D into VF `fd` or ACC (`ACC`);
/// `fd` 0 writes nothing. Its flags are made only where code reads them (`mac`).
#[inline(always)]
fn vop<const K: u8, const D: u32>(c: &mut Cpu, fd: usize, fs: usize, t: [u32; 4]) {
    let r = m_fmac::<K, D>(c.vf[fs], t, c.vacc);
    if fd == ACC {
        m_put::<D>(&mut c.vacc, r);
    } else if fd != 0 {
        m_put::<D>(&mut c.vf[fd], r);
    }
}
/// The same, with its MAC flags into VI17 (the instruction a CFC2 of the flags reads).
#[inline(always)]
fn vmac<const K: u8, const D: u32>(c: &mut Cpu, fd: usize, fs: usize, t: [u32; 4]) {
    let r = m_fmac::<K, D>(c.vf[fs], t, c.vacc);
    c.vi[17] = m_mac::<D>(r);
    if fd == ACC {
        m_put::<D>(&mut c.vacc, r);
    } else if fd != 0 {
        m_put::<D>(&mut c.vf[fd], r);
    }
}
/// Lane `l` of VF `ft` in every lane (the BC forms).
#[inline(always)]
fn bc(c: &Cpu, ft: usize, l: usize) -> [u32; 4] {
    [c.vf[ft][l]; 4]
}
#[inline(always)]
fn qq(c: &Cpu) -> [u32; 4] {
    [c.vi[22]; 4]
}
#[inline(always)]
fn lqc2(c: &mut Cpu, v: usize, a: u32) {
    let q = c.rd128(a);
    if v != 0 {
        c.vf[v] = w4(q);
    }
}
#[inline(always)]
fn sqc2(c: &mut Cpu, v: usize, a: u32) {
    let (lo, hi) = q4(c.vf[v]);
    c.wr128(a, lo, hi);
}
#[inline(always)]
fn qmtc2(c: &mut Cpu, v: usize, x: Q) {
    if v != 0 {
        c.vf[v] = w4(x);
    }
}
#[inline(always)]
fn qmfc2(c: &Cpu, v: usize) -> Q {
    q4(c.vf[v])
}
/// MOVE / MR32 / ABS / ITOF / FTOI on lanes D of VF `ft`.
#[inline(always)]
fn vmove<const D: u32>(c: &mut Cpu, ft: usize, v: [u32; 4]) {
    if ft != 0 {
        m_put::<D>(&mut c.vf[ft], v);
    }
}
#[inline(always)]
fn mr32(v: [u32; 4]) -> [u32; 4] {
    [v[1], v[2], v[3], v[0]]
}
/// DIV (0), SQRT (1), RSQRT (2) into Q.
#[inline(always)]
fn qop<const K: u8>(c: &mut Cpu, a: u32, b: u32) {
    uycpu::vu0::qop::<K>(c, a, b);
}
#[inline(always)]
fn stq(c: &mut Cpu, a: u32, v: Q) {
    c.wr128(a, v.0, v.1);
}

// ---- the scratchpad's DMA channels, as the port's kicks do them -----------------------------------------------------------

const SPR: u32 = 0x7000_0000;

/// SPR_TO (channel 9) in normal mode with these register values: the copy into the scratchpad, the skin recording of a
/// transfer to 0x1C00 (`hw::SkinJoints`), the registers as the kick leaves them, the channel's end and its interrupt.
fn spr_to(c: &mut Cpu, chcr: u32, madr: u32, qwc: u32, sadr: u32) {
    let mp = c.mp();
    let m = c.mach();
    let h = &mut m.hw.ch[9];
    (h[0], h[1], h[2], h[8]) = (chcr, madr, qwc, sadr);
    // The kick's QWC is the register's low 16 bits.
    let qwc = qwc & 0xffff;
    let mut s = sadr & 0x3ff0;
    if s == 0x1c00 && crate::SKIN_JOINTS.load(Relaxed) {
        m.hw.skin.captured = true;
        let (from, to) = ((madr & 0x01ff_fff0) as usize, (madr & 0x01ff_fff0) as usize + 16 * qwc as usize);
        if from >= 0x40 && to <= crate::RAM_SIZE as usize {
            let at = m.hw.skin.bytes.len();
            m.hw.skin.bytes.extend_from_slice(&m.ram[from - 0x40..to]);
            m.hw.skin.records.push((madr, qwc, at));
        }
    }
    put(m, &mut s, madr, qwc);
    let h = &mut m.hw.ch[9];
    h[1] = madr.wrapping_add(qwc * 16);
    h[8] = s;
    end(c, mp, 9);
}

/// SPR_TO in source chain mode (CHCR `chcr`, the tags from TADR `tadr`, MADR and QWC as they stand), as the kick walks it.
fn spr_to_chain(c: &mut Cpu, chcr: u32, tadr: u32, sadr: u32) {
    let mp = c.mp();
    let m = c.mach();
    let h = &mut m.hw.ch[9];
    (h[0], h[3], h[8]) = (chcr, tadr, sadr);
    let (madr, qwc) = (h[1], h[2] & 0xffff);
    let mut s = sadr & 0x3ff0;
    put(m, &mut s, madr, qwc);
    let mut tadr = tadr;
    let mut asr = [m.hw.ch[9][4], m.hw.ch[9][5]];
    let mut asp = ((chcr >> 4) & 3) as usize;
    let (mut madr, mut last);
    loop {
        let tag = qw(m, tadr);
        let q = (tag & 0xffff) as u32;
        let id = ((tag >> 28) & 7) as u32;
        last = (tag >> 16) as u32 & 0xffff;
        let addr = (tag >> 32) as u32 & 0xffff_fff0;
        let (data, next, stop) = match id {
            0 => (addr, tadr + 16, true),
            1 => (tadr + 16, tadr + 16 + q * 16, false),
            2 => (tadr + 16, addr, false),
            3 | 4 => (addr, tadr + 16, false),
            5 => {
                if asp < 2 {
                    asr[asp] = tadr + 16 + q * 16;
                    asp += 1;
                }
                (tadr + 16, addr, false)
            }
            6 => {
                if asp > 0 {
                    asp -= 1;
                    (tadr + 16, asr[asp], false)
                } else {
                    (tadr + 16, tadr + 16 + q * 16, true)
                }
            }
            _ => (tadr + 16, tadr + 16 + q * 16, true),
        };
        // CHCR's TTE is clear in the unit's chain (0x104): no tag words go to the scratchpad.
        put(m, &mut s, data, q);
        madr = data.wrapping_add(q * 16);
        tadr = next;
        if stop || ((tag >> 31) & 1 != 0 && chcr & 0x80 != 0) {
            break;
        }
    }
    let h = &mut m.hw.ch[9];
    h[1] = madr;
    h[3] = tadr;
    h[4] = asr[0];
    h[5] = asr[1];
    h[0] = (h[0] & 0xffcf) | (asp as u32) << 4 | last << 16;
    h[8] = s;
    end(c, mp, 9);
}

/// SPR_FROM (channel 8) in normal mode: the copy to RAM, the skin recording of an output from 0x3000 or 0x3400, the
/// registers, the end.
fn spr_from(c: &mut Cpu, chcr: u32, madr: u32, qwc: u32, sadr: u32) {
    let mp = c.mp();
    let m = c.mach();
    let h = &mut m.hw.ch[8];
    (h[0], h[1], h[2], h[8]) = (chcr, madr, qwc, sadr);
    let qwc = qwc & 0xffff;
    let s = sadr & 0x3ff0;
    let (s0, d0, n) = (s as usize, (madr & 0x01ff_fff0) as usize, 16 * qwc as usize);
    if matches!(s, 0x3000 | 0x3400) && m.hw.skin.captured && !m.hw.skin.records.is_empty() {
        let record = m.hw.skin.records.len() - 1;
        let bytes = m.spr[s0..s0 + n].to_vec();
        let normals = std::mem::take(&mut m.hw.skin_normals);
        m.hw.skin.outputs.push(crate::hw::SkinOutput { record, dest: madr, bytes, normals });
    }
    if s0 + n <= 0x4000 && d0 + n <= crate::RAM_SIZE as usize {
        m.ram[d0..d0 + n].copy_from_slice(&m.spr[s0..s0 + n]);
    } else {
        for k in 0..qwc {
            let sq = ((s + 16 * k) & 0x3ff0) as usize;
            let d = (madr.wrapping_add(16 * k) & 0x01ff_fff0) as usize;
            let v: [u8; 16] = m.spr[sq..sq + 16].try_into().expect("16 bytes");
            m.ram[d..d + 16].copy_from_slice(&v);
        }
    }
    let h = &mut m.hw.ch[8];
    h[1] = madr.wrapping_add(qwc * 16);
    h[8] = (s + qwc * 16) & 0x3ff0;
    end(c, mp, 8);
}

/// A quadword of DMA source: RAM, or the scratchpad when bit 31 is set.
fn qw(m: &crate::Machine, a: u32) -> u128 {
    if a & 0x8000_0000 != 0 {
        let o = (a & 0x3ff0) as usize;
        u128::from_le_bytes(m.spr[o..o + 16].try_into().expect("16 bytes"))
    } else {
        let o = (a & 0x01ff_fff0) as usize;
        u128::from_le_bytes(m.ram[o..o + 16].try_into().expect("16 bytes"))
    }
}

/// `q` quadwords from `a` into the scratchpad at `*sadr`, which steps on (wrapping at 16 KB).
fn put(m: &mut crate::Machine, sadr: &mut u32, a: u32, q: u32) {
    let (s0, o, n) = ((*sadr & 0x3ff0) as usize, (a & 0x01ff_fff0) as usize, 16 * q as usize);
    if a & 0x8000_0000 == 0 && s0 + n <= 0x4000 && o + n <= crate::RAM_SIZE as usize {
        m.spr[s0..s0 + n].copy_from_slice(&m.ram[o..o + n]);
        *sadr = (*sadr + 16 * q) & 0x3ff0;
        return;
    }
    for k in 0..q {
        let v = qw(m, a.wrapping_add(16 * k));
        let s = (*sadr & 0x3ff0) as usize;
        m.spr[s..s + 16].copy_from_slice(&v.to_le_bytes());
        *sadr = (*sadr + 16) & 0x3ff0;
    }
}

/// A channel's end as the kick ends it: STR and QWC cleared, D_STAT's bit, the DMAC interrupt.
fn end(c: &mut Cpu, mp: *mut crate::Machine, ch: usize) {
    let m = c.mach();
    m.hw.ch[ch][0] &= !0x100;
    m.hw.ch[ch][2] = 0;
    m.hw.d_stat |= 1 << ch;
    crate::kernel::raise(mp, Some(c), crate::kernel::Int::Dmac(ch as u32));
}

// ---- what the calls covered (the check's report) ---------------------------------------------------------------------------

/// Counts of the paths the native calls took: mobies culled, mobies drawn, draw records run, rows of skin groups 1 to 3,
/// rigid vertices, second-list entries (0x4A0B88), 0x4A0830's lists, texture uploads (0x437C88).
static DONE: [AtomicU64; 10] = [const { AtomicU64::new(0) }; 10];
const NAMES: [&str; 10] = ["mobies seen", "mobies drawn", "near-list mobies", "draw records", "group 1 rows", "group 2 rows", "group 3 rows", "rigid vertices", "second-list entries", "texture uploads"];

#[inline(always)]
fn count(k: usize, n: u32) {
    DONE[k].fetch_add(n as u64, Relaxed);
}

// ---- the check -------------------------------------------------------------------------------------------------------------

/// What a call of the unit can change that the game reads afterwards.
struct State {
    ram: Vec<u8>,
    spr: Vec<u8>,
    vu0d: Vec<u8>,
    vu0m: Vec<u8>,
    r: [u64; 32],
    rh: [u64; 32],
    hilo: [u64; 4],
    sa: u32,
    f: [u32; 32],
    facc: u32,
    fcr31: u32,
    vf: [[u32; 4]; 32],
    vacc: [u32; 4],
    vi: [u32; 32],
    view_h: u32,
    ch: [[u32; 9]; 10],
    d_stat: u32,
    intc_stat: u32,
    records: Vec<(u32, u32, usize)>,
    rec_bytes: Vec<u8>,
    captured: bool,
    outputs: Vec<(usize, u32, Vec<u8>, Vec<[f32; 3]>)>,
    normals: Vec<[f32; 3]>,
}

impl State {
    fn take(c: &mut Cpu) -> State {
        let (r, rh, hilo, sa, f, facc, fcr31) = (c.r, c.rh, [c.lo, c.hi, c.lo1, c.hi1], c.sa, c.f, c.facc, c.fcr31);
        let (vf, vacc, vi, view_h) = (c.vf, c.vacc, c.vi, c.view_h);
        let m = c.mach();
        State {
            ram: m.ram.to_vec(),
            spr: m.spr.clone(),
            vu0d: m.vu0d.clone(),
            vu0m: m.vu0m.clone(),
            r,
            rh,
            hilo,
            sa,
            f,
            facc,
            fcr31,
            vf,
            vacc,
            vi,
            view_h,
            ch: m.hw.ch,
            d_stat: m.hw.d_stat,
            intc_stat: m.hw.intc_stat,
            records: m.hw.skin.records.clone(),
            rec_bytes: m.hw.skin.bytes.clone(),
            captured: m.hw.skin.captured,
            outputs: m.hw.skin.outputs.iter().map(|o| (o.record, o.dest, o.bytes.clone(), o.normals.clone())).collect(),
            normals: m.hw.skin_normals.clone(),
        }
    }

    fn put(&self, c: &mut Cpu) {
        (c.r, c.rh, c.sa, c.f, c.facc, c.fcr31) = (self.r, self.rh, self.sa, self.f, self.facc, self.fcr31);
        [c.lo, c.hi, c.lo1, c.hi1] = self.hilo;
        (c.vf, c.vacc, c.vi, c.view_h) = (self.vf, self.vacc, self.vi, self.view_h);
        let m = c.mach();
        m.ram.copy_from_slice(&self.ram);
        m.spr.copy_from_slice(&self.spr);
        m.vu0d.copy_from_slice(&self.vu0d);
        if m.vu0m[..] != self.vu0m[..] {
            m.vu0m.copy_from_slice(&self.vu0m);
            m.vu0_gen += 1;
        }
        m.hw.ch = self.ch;
        m.hw.d_stat = self.d_stat;
        m.hw.intc_stat = self.intc_stat;
        m.hw.skin.records = self.records.clone();
        m.hw.skin.bytes = self.rec_bytes.clone();
        m.hw.skin.captured = self.captured;
        m.hw.skin.outputs = self.outputs.iter().map(|(record, dest, bytes, normals)| crate::hw::SkinOutput { record: *record, dest: *dest, bytes: bytes.clone(), normals: normals.clone() }).collect();
        m.hw.skin_normals = self.normals.clone();
    }
}

/// The byte ranges where `a` and `b` differ, as (start, end) pairs, at most `most`, and how many there are; `skip` (offsets
/// in `a`) is left out.
fn ranges(a: &[u8], b: &[u8], base: u32, most: usize, skip: std::ops::Range<usize>) -> (Vec<String>, usize) {
    let (mut out, mut n, mut i) = (Vec::new(), 0, 0);
    while i < a.len() {
        if a[i] == b[i] || skip.contains(&i) {
            i += 1;
            continue;
        }
        let s = i;
        while i < a.len() && !skip.contains(&i) && (a[i] != b[i] || (i + 16 < a.len() && a[i..i + 16] != b[i..i + 16])) {
            i += 1;
        }
        n += 1;
        if out.len() < most {
            let e = i.min(s + 16);
            out.push(format!("{:#x}..{:#x} [{:02x?} / {:02x?}]", base as usize + s, base as usize + i, &a[s..e], &b[s..e]));
        }
    }
    (out, n)
}

/// The unit's own stack frames below the caller's sp: the deepest chain (0x438120, 0x4380C0, 0x437B48, 0x4A0830, 0x4A0348)
/// is 208 bytes. Nothing reads them after the unit returns (the callers' next calls write their frames before reading them).
const FRAMES: u32 = 0x100;

/// What differs between the translation's state `want` and `got`, for a call entered with stack pointer `sp`.
///
/// Left out, with the evidence:
/// - RAM below the entry sp (`FRAMES`).
/// - Scratchpad 0x3700..0x3720: 0x43C028's save of the caller's t8 and t9 (128 bits each, which hold whatever the code before
///   left in them), read by nothing but its own restore (0x43C878, 0x43C884); the only other code that touches the range is
///   the shrub cull (func_L01_004A2200), which builds its list there (from 0x4A2250) before it reads it (0x4A2E38).
/// - The caller-saved registers (at, v0, v1, a0 to a3, t0 to t9, k0, k1, both halves; HI and LO; FPU f0 to f19, its
///   accumulator and FCR31): the callers of both entries (0x4010D0, 0x4009B8, 0x4D3460, 0x497828, 0x4BCCE0) are compiled C
///   that treats them as void calls; after each call site the next reads of these registers come after writes.
/// - The high halves of s0 to s7, gp, sp, fp and ra. The unit's hand-written functions use them as room (0x43EA78's PCPYLDs
///   into ra, s4 to s7 and sp), the skin job as scratch for its vertices (its PEXT, PPAC, QMFC2 into s0 to s7, gp and sp),
///   and give back only the low halves (0x43EA78 and 0x43E998 restore with LD), so what they leave there is a record's last
///   vertex, which the native skin job does not make. Over every program's code (boot, fe100, fe101 and the 34 levels), the
///   128-bit reads of these high halves that no 128-bit write comes before in the same function are, outside the unit,
///   (a) 13198 saves, SQ to a frame slot that only the epilogue's LQ into the same register reads back;
///   (b) PEXEW ra at the entry and exit of level 1's 0x406018 and 0x4060D8, which swap ra's words 0 and 2 around a JAL that
///   overwrites the word brought down; (c) PEXTL* and PCPYLD, which read the low halves only; (d) the shrub cull's PMINWs
///   of s7 (level 1 0x4A2504, 0x4A2BB4, 0x4A2E1C, 0x4A381C), whose upper words go only to at, a0 and VF16's z and w (read
///   next as a1's high half at 0x43C97C, whose sign test sees lane y); and (e) level 1 0x4AFA28's PMINW of s1 at 0x4AFCF0
///   (fe100 0x3DA360, level 7 0x4AEB18), whose upper words a word-level taint of the function carries into rendered lanes
///   only along 0x4B0028 (t5 not 0) to 0x4B01D4, 0x4B0284, 0x4B0354's J to 0x4B0030 and VF10.w's broadcast at 0x4B00B4, a
///   path that also reads VF9 from the scratchpad (0x3E10) where the main path loads the item's rows. 0x4AFA28 runs before
///   the unit in the frame (0x4011AC), and after the unit 0x4010D0 rewrites s1's high half at 0x45E810 (PCPYLD s1, s1, zero;
///   PCPYUD at its end) when 0x1DF9C4 & 0x20 is set, or else leaves the unit's. (e) is the one read the code does not settle.
///   The low halves are compared.
/// - VU0's registers (VF, ACC, Q, I, VI1 to VI15, the MAC, status and clip flags), which the skin job's microprogram and the
///   translation's EE side leave full of a job's last rows: after the unit returns in the frame (0x4012B8 in 0x4010D0) no
///   code reads one before writing it (a read-before-write pass over the rest of 0x4010D0 and all it calls; the only hits are
///   whole-register QMFC2s of registers whose other lanes are never used, 0x40C110 lane x, 0x45EA00 lane x, 0x45EA10 lane z,
///   and CLIP's shift at 0x405B78, whose CFC2 at 0x405BA8 follows four CLIPs), and 0x4010D0 writes vf1, vf2, vf4 to vf9
///   and ACC on every path before it calls the unit again; the status flag is never read (no CFC2 of VI16 in level 1 or the
///   boot program; the VU0 programs' FSAND reads the landed MAC flags, uyatr vu0mp.rs). Between 0x4009B8's call of 0x4380C0
///   and its call of 0x438120 the cull can read lanes the code before left (0x43F4E0's vf5.x when vf5.y is +0, 0x440168 to
///   0x440170's vf5.w, vf6.w, vf7.w): there the native skin job's registers are not the translation's.
fn differences(want: &State, got: &State, sp: u32) -> Vec<String> {
    let mut d = Vec::new();
    let frames = (sp.wrapping_sub(FRAMES) & 0x01ff_ffff) as usize..(sp & 0x01ff_ffff) as usize;
    for (what, a, b, base, skip) in [
        ("RAM", &want.ram, &got.ram, 0u32, frames),
        ("scratchpad", &want.spr, &got.spr, SPR, 0x3700..0x3720),
        ("VU0 data", &want.vu0d, &got.vu0d, 0, 0..0),
        ("VU0 code", &want.vu0m, &got.vu0m, 0, 0..0),
    ] {
        let (r, n) = ranges(a, b, base, 12, skip);
        if n > 0 {
            d.push(format!("{what}: {n} ranges {}", r.join(" ")));
        }
    }
    // The callee-saved registers' low halves: s0 to s7, gp, sp, fp, ra.
    let kept = [16, 17, 18, 19, 20, 21, 22, 23, 28, 29, 30, 31];
    let regs: Vec<String> = kept.iter().filter(|&&k| want.r[k] != got.r[k]).map(|&k| format!("r{k} {:x}/{:x}", want.r[k], got.r[k])).collect();
    if !regs.is_empty() || want.sa != got.sa {
        d.push(format!("registers: {} SA {}/{}", regs.join(" "), want.sa, got.sa));
    }
    let fregs: Vec<String> = (20..32).filter(|&k| want.f[k] != got.f[k]).map(|k| format!("f{k} {:x}/{:x}", want.f[k], got.f[k])).collect();
    if !fregs.is_empty() {
        d.push(format!("FPU: {}", fregs.join(" ")));
    }
    if want.view_h != got.view_h {
        d.push(format!("view_h {:#x}/{:#x}", want.view_h, got.view_h));
    }
    let ch: Vec<String> = (0..10).filter(|&k| want.ch[k] != got.ch[k]).map(|k| format!("channel {k} {:x?}/{:x?}", want.ch[k], got.ch[k])).collect();
    if !ch.is_empty() || want.d_stat != got.d_stat || want.intc_stat != got.intc_stat {
        d.push(format!("DMA: {} D_STAT {:#x}/{:#x} INTC {:#x}/{:#x}", ch.join(" "), want.d_stat, got.d_stat, want.intc_stat, got.intc_stat));
    }
    if want.records != got.records || want.rec_bytes != got.rec_bytes || want.captured != got.captured {
        d.push(format!("skin records: {} / {}", want.records.len(), got.records.len()));
    }
    if want.outputs != got.outputs || want.normals != got.normals {
        d.push(format!("skin outputs: {} / {} (normals pending {} / {})", want.outputs.len(), got.outputs.len(), want.normals.len(), got.normals.len()));
    }
    d
}

/// The two entries: 0x438120 (`moby`) and 0x4380C0 on its own (`moby2`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Job {
    Draw,
    Lists,
}

struct Check {
    job: Job,
    left: u64,
    done: u64,
    differ: u64,
    kinds: std::collections::BTreeSet<String>,
    before: Option<(State, u32)>,
    /// Jobs entered inside the translation being checked: they run their translations too.
    inner: u32,
    /// Whether the thread had its interrupts on at the checked call. Both runs go with them held (DI), and EI
    /// after the comparison takes what waited: inside a run, a loop's back edge past the field's time gives the field up
    /// (`kernel::preempt`: the vertical blank's handlers, the other threads and the IOP's writes run before the thread goes
    /// on, and the field takes its skin recordings), and FlushCache's syscall at 0x437EB0 runs the timers' interrupts that
    /// came due by the host's clock (`kernel::syscall`, `Hw::timer_events`), each once, in whichever run reached it first.
    /// That is what differed in the arena fight's check: the handlers' frames on the interrupt stack at 0xFEF30
    /// (`kernel::INT_STACK`), Timer 0's count and time at 0x1D4AC8 (the boot program's handler 0x13D208), Timer 1's count at
    /// 0x13F288 (0x125578), INTC's VBON, the skin recordings the field took, and in the calls where the field ended the sound
    /// module's writes that land at the vertical blank (`iop::land_sound_writes`: a disc read at 0x110B000, the sound
    /// library's busy word 0x1D4980 that its callback clears) and the other threads' RAM.
    ints: bool,
}

static CHECK: std::sync::Mutex<Option<Check>> = std::sync::Mutex::new(None);

/// UYA_NATIVE_CHECK=<job>:N, read once. Another unit's job (the collision sweeps' `sweep`, the shrub cull's `shrub`) leaves
/// this unit unchecked.
fn checked() -> Option<(Job, u64)> {
    static C: std::sync::OnceLock<Option<(Job, u64)>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_NATIVE_CHECK").ok()?;
        let (job, n) = v.split_once(':').unwrap_or_else(|| panic!("UYA_NATIVE_CHECK={v}: <job>:N"));
        let job = match job {
            "moby" => Job::Draw,
            "moby2" => Job::Lists,
            _ => return None,
        };
        Some((job, n.parse().unwrap_or_else(|_| panic!("UYA_NATIVE_CHECK={v}: N is a number of calls"))))
    })
}

/// `Rt::native` 4 and 6: the unit natively (`c.ret` the return address), or (checking) the state saved and `c.ret` 0 so the
/// translation runs.
pub(crate) fn start(c: &mut Cpu, k: u32) {
    let job = if k == 4 { Job::Draw } else { Job::Lists };
    let (at, wide) = (c.ret & !3, c.ret & 2 != 0);
    if let Some((cj, n)) = checked() {
        let mut g = CHECK.lock().expect("the native check");
        let ck = g.get_or_insert(Check { job: cj, left: n, done: 0, differ: 0, kinds: Default::default(), before: None, inner: 0, ints: false });
        if ck.before.is_some() {
            ck.inner += 1;
            c.ret = 0;
            return;
        }
        if ck.job == job && ck.left > 0 {
            if job == Job::Draw {
                crate::drawdist::refuse_check("moby");
            }
            CALL.with(|w| w.set((at, wide)));
            // Interrupts on as the kernel sees them (`kernel::K::ints_on`): EIE and IE, not in an exception.
            ck.ints = c.status & 0x1_0001 == 0x1_0001 && c.status & 6 == 0;
            if ck.ints {
                c.di();
            }
            ck.before = Some((State::take(c), c.r[29] as u32));
            // The translation reads the props' near distances from memory, as the port wrote them before each field (`range`).
            if job == Job::Draw {
                crate::range::for_translation(c, crate::range::Unit::Props);
            }
            c.ret = 0;
            return;
        }
    }
    run(c, job, at, wide);
    c.ret = c.r[31] as u32;
}

/// `Rt::native` 5 and 7: the translation ran; the unit runs natively from the state before it and the two are compared.
pub(crate) fn after(c: &mut Cpu, k: u32) {
    let job = if k == 5 { Job::Draw } else { Job::Lists };
    let (Some((before, sp)), ints) = ({
        let mut g = CHECK.lock().expect("the native check");
        let ck = g.as_mut().expect("checking");
        if ck.inner > 0 {
            ck.inner -= 1;
            return;
        }
        (ck.before.take(), ck.ints)
    }) else {
        return;
    };
    let ret = c.ret;
    if job == Job::Draw {
        crate::range::translation_done(c, crate::range::Unit::Props);
    }
    let want = State::take(c);
    before.put(c);
    // The wrapper's `c.ret` at the start: the function's address and the view bit, which the translation's run replaced.
    let (at, wide) = (CALL.with(|w| w.get()).0, CALL.with(|w| w.get()).1);
    if job == Job::Draw {
        crate::drawdist::refuse_check("moby");
    }
    run(c, job, at, wide);
    let got = State::take(c);
    let d = differences(&want, &got, sp);
    want.put(c);
    c.ret = ret;
    let mut g = CHECK.lock().expect("the native check");
    let ck = g.as_mut().expect("checking");
    ck.left -= 1;
    ck.done += 1;
    if !d.is_empty() {
        ck.differ += 1;
        // The first calls that differ, and the first of each other kind of difference (what differs, without the values).
        let kind: String = d.iter().map(|x| x.split(':').next().unwrap_or(x)).collect::<Vec<_>>().join("+");
        if ck.differ <= 6 || ck.kinds.insert(kind) {
            println!("UYA_NATIVE_CHECK: {job:?} call {} (at {at:08x}, sp {sp:08x}): {}", ck.done, d.join("; "));
        }
    }
    if ck.left == 0 || ck.done % 100 == 0 {
        let n: Vec<String> = DONE.iter().zip(NAMES).map(|(a, name)| format!("{name} {}", a.load(Relaxed))).collect();
        println!("UYA_NATIVE_CHECK: {job:?}: {} calls checked, {} differ ({}){}", ck.done, ck.differ, n.join(", "), crate::range::written(crate::range::Unit::Props));
    }
    drop(g);
    // The interrupts held for the two runs (`Check::ints`): what came due runs now, as at the game's own EI.
    if ints {
        c.ei();
        c.ret = ret;
    }
}

thread_local! {
    /// The function's address and view bit of the call being checked, for its native run after the translation's.
    static CALL: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((0, false)) };
}

/// The unit from either entry, natively: the callee-saved registers' low halves come back as the caller had them; their high
/// halves as the native cull and second list leave them (the translation's skin job leaves its last vertex there:
/// `differences`).
fn run(c: &mut Cpu, job: Job, at: u32, wide: bool) {
    CALL.with(|w| w.set((at, wide)));
    let sp = c.r[29] as u32;
    let ra = c.r[31];
    match job {
        Job::Draw => f438120(c, F { at, l1: 0x43_8120 }, sp, ra, wide),
        Job::Lists => f4380c0(c, F { at, l1: 0x43_80c0 }, sp, ra),
    }
}

// ---- the glue: 0x438120, 0x4380C0 and the small functions under them --------------------------------------------------------
//
// Compiled C. Their stack frames hold saved registers only (`FRAMES`), which the native code keeps in its own variables.

/// 0x438120: the chain's start and the VU0 program (0x437F80), the per-class table cleared (0x437EF8), the moby cull and draw
/// list over the moby array (0x43EA78; mode 1, or 3 once when 0x1E00D4 was set, which it clears), its list end kept at
/// gp+0x37E8, then the second half (0x4380C0).
fn f438120(c: &mut Cpu, k: F, sp: u32, _ra: u64, wide: bool) {
    let sp = sp.wrapping_sub(16);
    { let kk = k.call(c, 0x43_8128, 0x43_7f80); f437f80(c, kk); }
    f437ef8(c);
    let gp = c.r[28] as u32;
    let mut a3 = 1;
    if c.rd32(k.a(c, 0x43_8138, 0x43_813c)) != 0 {
        let a = k.a(c, 0x43_8148, 0x43_814c);
        c.wr32(a, 0);
        a3 = 3;
    }
    let a0 = c.rd32(k.a(c, 0x43_8154, 0x43_8158));
    let a1 = c.rd32(k.gp(c, gp, 0x43_8164));
    // The end of the draw records' room: where the second list's entries go (0x437B48's a0, gp+0x3344), for FAR's bound.
    let room = {
        let second = k.call(c, 0x43_8168, 0x43_80c0).call(c, 0x43_80e8, 0x43_7b48);
        c.rd32(second.gp(c, gp, 0x43_7bfc))
    };
    let v0 = { let kk = k.call(c, 0x43_8160, 0x43_ea78); f43ea78(c, kk, a0, a1, u32::MAX, a3, k.pc(0x43_8168) as u64, sp, wide, room) };
    let a = k.gp(c, gp, 0x43_816c);
    c.wr32(a, v0);
    { let kk = k.call(c, 0x43_8168, 0x43_80c0); f4380c0(c, kk, sp, k.pc(0x43_8170) as u64); }
}

/// 0x4380C0: the second chain (0x4378F0), the draw records run (0x437EB0), the second list (0x437B48) when the cull left
/// its chain mark at 0x1E00E4, the class texture uploads (0x437C88) when the cull's list (0x21F260) is not empty.
fn f4380c0(c: &mut Cpu, k: F, sp: u32, _ra: u64) {
    let sp = sp.wrapping_sub(16);
    { let kk = k.call(c, 0x43_80c8, 0x43_78f0); f4378f0(c, kk); }
    { let kk = k.call(c, 0x43_80d0, 0x43_7eb0); f437eb0(c, kk, sp, k.pc(0x43_80d8) as u64); }
    let gp = c.r[28] as u32;
    let v0 = c.rd32(k.a(c, 0x43_80d8, 0x43_80dc));
    let mut v1 = sx(c.rd32(k.gp(c, gp, 0x43_80e4)));
    if v0 != 0 {
        { let kk = k.call(c, 0x43_80e8, 0x43_7b48); f437b48(c, kk, sp, k.pc(0x43_80f0) as u64); }
        v1 = sx(c.rd32(k.a(c, 0x43_80f0, 0x43_80f4)));
    }
    if sx(k.a(c, 0x43_80f8, 0x43_80fc)) < v1 {
        { let kk = k.call(c, 0x43_810c, 0x43_7c88); f437c88(c, kk); }
    }
}

/// 0x437F80: the chain's first tags (0x4C1E98, 0x4C1AF0's VU0 program upload, 0x4C1FE8), the frame's chain globals.
fn f437f80(c: &mut Cpu, k: F) {
    let a1 = c.rd16(k.a(c, 0x43_7f80, 0x43_7f88)) as u32;
    let a0 = k.a(c, 0x43_7f8c, 0x43_7f98);
    { let kk = k.call(c, 0x43_7f94, 0x4c_1e98); f4c1e98(c, kk, a0, a1); }
    let a = k.a(c, 0x43_7fa4, 0x43_7fa8);
    c.wr32(a, 6);
    let a0 = k.a(c, 0x43_7fa0, 0x43_7fb0);
    f4c1af0(c, a0);
    { let kk = k.call(c, 0x43_7fbc, 0x4c_1fe8); f4c1fe8(c, kk, 71, 0x5_360b); }
    let v0 = c.rd32(k.a(c, 0x43_7fc4, 0x43_7fc8));
    let a0 = c.rd32(k.a(c, 0x43_7fd0, 0x43_7fd4)).wrapping_add(0xffff_0000);
    let v1 = k.a(c, 0x43_7fd8, 0x43_7fe4);
    let a2 = c.rd32(k.a(c, 0x43_7fdc, 0x43_7fe0));
    let a = k.a(c, 0x43_7fe8, 0x43_7fec);
    c.wr32(a, v0);
    let a3 = c.rd32(k.a(c, 0x43_7ff4, 0x43_7ff8));
    let a = k.a(c, 0x43_8004, 0x43_8008);
    c.wr32(a, a2);
    let a = k.a(c, 0x43_800c, 0x43_8010);
    c.wr32(a, v0.wrapping_add(16));
    let a = k.a(c, 0x43_8014, 0x43_8018);
    c.wr32(a, v1);
    let a = k.a(c, 0x43_801c, 0x43_8020);
    c.wr32(a, a0);
    let a = k.a(c, 0x43_8024, 0x43_8028);
    c.wr32(a, a3);
    let a = k.a(c, 0x43_802c, 0x43_8030);
    c.wr32(a, 0);
}

/// 0x4C1E98: a REF tag (0x30000000 | quadwords) to `a0` at the chain pointer (0x1E10E8), which steps on (gp+0x4838).
fn f4c1e98(c: &mut Cpu, k: F, a0: u32, a1: u32) {
    let p = k.a(c, 0x4c_1e98, 0x4c_1e9c);
    let t = c.rd32(p);
    c.wr32(t, a1 | 0x3000_0000);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(4), a0);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(8), 0);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(12), 0);
    let t = c.rd32(p);
    let gp = c.r[28] as u32;
    let a = k.gp(c, gp, 0x4c_1ee0);
    c.wr32(a, t.wrapping_add(16));
}

/// 0x4C1AF0: VIF0's chain from `a0` (the skin job's VU0 program, MPG codes), kicked through the hardware registers: the
/// port's VIF0 puts it into VU0's micro memory at once (the polls before and after find the channel idle).
fn f4c1af0(c: &mut Cpu, a0: u32) {
    c.wr32(0x1000_8020, 0);
    c.wr32(0x1000_8030, a0 & 0x0fff_ffff);
    c.wr32(0x1000_8000, 0x145);
}

/// 0x4C1FE8: an A+D packet of one register (`a0` the register, `a1` the 64-bit value) at the chain pointer.
fn f4c1fe8(c: &mut Cpu, k: F, a0: u32, a1: u64) {
    let p = k.a(c, 0x4c_1fe8, 0x4c_1fec);
    let gp = c.r[28] as u32;
    for (o, v) in [(0u32, 0x1000_0002u32), (4, 0), (8, 0), (12, 0x5000_0002), (16, 0x8001), (20, 0x1000_0000), (24, 14), (28, 0)] {
        let t = c.rd32(p);
        c.wr32(t.wrapping_add(o), v);
    }
    let t = c.rd32(p);
    c.wr64(t.wrapping_add(0x20), a1);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(0x28), a0);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(0x2c), 0);
    let t = c.rd32(p);
    let a = k.gp(c, gp, 0x4c_2098);
    c.wr32(a, t.wrapping_add(0x30));
}

/// 0x4C21C0: a CALL tag (0x30000003) to the global packet at 0x1DA2F0 with its DIRECT code (0x50000003).
fn f4c21c0(c: &mut Cpu, k: F) {
    let p = k.a(c, 0x4c_21c0, 0x4c_21c4);
    let v = k.a(c, 0x4c_21d0, 0x4c_21d8);
    let t = c.rd32(p);
    c.wr32(t, 0x3000_0003);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(4), v);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(8), 0);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(12), 0x5000_0003);
    let t = c.rd32(p);
    let gp = c.r[28] as u32;
    let a = k.gp(c, gp, 0x4c_2218);
    c.wr32(a, t.wrapping_add(16));
}

/// 0x437EF8: the per-class words at scratchpad 0x3A00 (240 of them, 0x40BBA8) set to 0x40000000, the cull's nearest depth.
fn f437ef8(c: &mut Cpu) {
    for k in 0..240 {
        c.wr32(SPR + 0x3a00 + 4 * k, 0x4000_0000);
    }
}

/// 0x4378F0: the second chain's NEXT tags around the classes' chain (0x43E6E8) and the global packet (0x4C21C0).
fn f4378f0(c: &mut Cpu, k: F) {
    let p = k.a(c, 0x43_78f4, 0x43_78f8);
    let q = k.a(c, 0x43_7908, 0x43_790c);
    let s0 = c.rd32(p);
    let s1 = 0x2000_0000u32;
    c.wr32(p, s0.wrapping_add(16));
    let t = c.rd32(q);
    c.wr32(t, s1);
    let t = c.rd32(q);
    let v = c.rd32(p);
    c.wr32(t.wrapping_add(4), v);
    let t = c.rd32(q);
    c.wr32(t.wrapping_add(8), 0);
    let t = c.rd32(q);
    c.wr32(t.wrapping_add(12), 0);
    let gp = c.r[28] as u32;
    let a0 = c.rd32(k.gp(c, gp, 0x43_7958));
    { let kk = k.call(c, 0x43_7954, 0x43_e6e8); f43e6e8(c, kk, a0); }
    { let kk = k.call(c, 0x43_795c, 0x4c_21c0); f4c21c0(c, kk); }
    let t = c.rd32(p);
    c.wr32(t, s1);
    let v = c.rd32(q).wrapping_add(16);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(4), v);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(8), 0);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(12), 0);
    let t = c.rd32(p);
    c.wr32(p, t.wrapping_add(16));
    c.wr32(s0, s1);
    let v = c.rd32(p);
    c.wr32(s0.wrapping_add(12), 0);
    c.wr32(s0.wrapping_add(4), v);
    c.wr32(s0.wrapping_add(8), 0);
}

/// 0x437EB0: FlushCache (the boot program's syscall 100), the (cos, sin) table into the scratchpad at 0x3800 (0x40BD58,
/// 0x800 bytes), then the draw records from the list head 0x1DFBF0 with the staging buffer at gp+0x3348 (0x43E998).
fn f437eb0(c: &mut Cpu, k: F, sp: u32, ra: u64) {
    let sp = sp.wrapping_sub(16);
    let flush = k.call(c, 0x43_7eb8, 0x11_f0a0);
    c.r[3] = 100;
    c.r[4] = 0;
    c.syscall(flush.pc(0x11_f0a4));
    let src = k.a(c, 0x43_7ec0, 0x43_7ecc);
    for q in 0..0x80 {
        let v = c.rd128(src + 16 * q);
        c.wr128(SPR + 0x3800 + 16 * q, v.0, v.1);
    }
    let gp = c.r[28] as u32;
    let a0 = c.rd32(k.a(c, 0x43_7ed8, 0x43_7edc));
    let a1 = c.rd32(k.gp(c, gp, 0x43_7ee4));
    let _ = ra;
    { let kk = k.call(c, 0x43_7ee0, 0x43_e998); f43e998(c, kk, a0, a1, sp, k.pc(0x43_7ee8) as u64); }
}

/// 0x437B48: the second list's chain: a NEXT tag at the cull's mark (0x1E00E4) to the list built by 0x4A0B88 (the entries at
/// gp+0x3344 from the cull's list at 0x21F0E0) and 0x4A0830, then a NEXT back; an END-free empty tag when 0x1E00E0 is 0.
fn f437b48(c: &mut Cpu, k: F, sp: u32, _ra: u64) {
    let mark = k.a(c, 0x43_7b4c, 0x43_7b50);
    if c.rd32(mark) == 0 {
        let m = k.a(c, 0x43_7b64, 0x43_7b68);
        let t = c.rd32(m);
        c.wr32(t, 0x1000_0000);
        for o in [4, 8, 12] {
            let t = c.rd32(m);
            c.wr32(t.wrapping_add(o), 0);
        }
        return;
    }
    let p = k.a(c, 0x43_7b9c, 0x43_7ba0);
    let m = k.a(c, 0x43_7ba8, 0x43_7bac);
    let s0 = c.rd32(p);
    let s1 = 0x2000_0000u32;
    let v1 = c.rd32(m);
    c.wr32(p, s0.wrapping_add(16));
    c.wr32(v1, s1);
    let t = c.rd32(m);
    let v = c.rd32(p);
    c.wr32(t.wrapping_add(4), v);
    let t = c.rd32(m);
    c.wr32(t.wrapping_add(8), 0);
    let t = c.rd32(m);
    c.wr32(t.wrapping_add(12), 0);
    let gp = c.r[28] as u32;
    let a1 = c.rd32(k.a(c, 0x43_7bf0, 0x43_7bf4));
    let a0 = c.rd32(k.gp(c, gp, 0x43_7bfc));
    // The second list's functions on one register file (0x437B48's s0 and s1, its frame's sp, the arguments).
    let mut r = c.r;
    let mut h = c.rh;
    (r[S0], r[S1], r[SP]) = (sx(s0), sx(s1), sx(sp.wrapping_sub(32)));
    (r[A0], r[A1], r[RA]) = (sx(a0), sx(a1), k.pc(0x43_7c00) as u64);
    let k2 = k.call(c, 0x43_7bf8, 0x4a_0b88);
    f004a0b88(c, k2, &mut r, &mut h);
    r[A0] = sx(c.rd32(k.gp(c, gp, 0x43_7c04)));
    r[RA] = k.pc(0x43_7c08) as u64;
    let k2 = k.call(c, 0x43_7c00, 0x4a_0830);
    f004a0830(c, k2, &mut r, &mut h);
    // s0, s1 and ra come back from 0x437B48's frame (their low halves); the high halves as the callees left them (0x4A0B88's
    // and 0x4A0FE8's PCPYUDs clear them).
    for n in [S0, S1, S2, S3, S4, S5, S6, S7, SP, FP, RA] {
        c.rh[n] = h[n];
    }
    let t = c.rd32(p);
    c.wr32(t, s1);
    let v = c.rd32(m).wrapping_add(16);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(4), v);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(8), 0);
    let t = c.rd32(p);
    c.wr32(t.wrapping_add(12), 0);
    let t = c.rd32(p);
    c.wr32(p, t.wrapping_add(16));
    c.wr32(s0, s1);
    let v = c.rd32(p);
    c.wr32(s0.wrapping_add(12), 0);
    c.wr32(s0.wrapping_add(4), v);
    c.wr32(s0.wrapping_add(8), 0);
}

/// 0x437C88: for each entry of the cull's texture list (from 0x21F260 to 0x1E0130's end: the chain mark, the moby), a NEXT
/// tag at the mark into a chain of 112-byte upload packets (the template at 0x2E9C10, 0x40BD58) per palette of the class
/// (class +0xB: high nibble times low nibble), with a CALL of the global packet, and a NEXT back.
fn f437c88(c: &mut Cpu, k: F) {
    let gp = c.r[28] as u32;
    let base = k.a(c, 0x43_7c8c, 0x43_7c98);
    let endp = k.a(c, 0x43_7c90, 0x43_7c94);
    if c.rd32(endp) == base {
        return;
    }
    let p = k.a(c, 0x43_7cc8, 0x43_7ccc);
    let fp = c.rd32(p);
    let mut a3 = base;
    let lim = c.rd32(endp);
    let a = k.gp(c, gp, 0x43_7ce0);
    c.wr32(a, fp.wrapping_add(16));
    if sx(a3) < sx(lim) {
        let template = k.a(c, 0x43_7d5c, 0x43_7d64);
        let global = k.a(c, 0x43_7dc8, 0x43_7dd0);
        loop {
            count(9, 1);
            let s4 = c.rd32(a3);
            let a0 = c.rd32(a3.wrapping_add(4));
            c.wr32(s4, 0x2000_0000);
            let a2 = a0 & 0xf;
            let s7 = a3.wrapping_add(8);
            let v1 = c.rd32(p);
            let a0 = a0 & 0xffff_fff0;
            c.wr32(s4.wrapping_add(12), 0x1100_0000);
            c.wr32(s4.wrapping_add(4), v1);
            let cls = c.rd32(a0.wrapping_add(0x24));
            let b = c.rd8(cls.wrapping_add(0xb)) as u32;
            let pal = c.rd32(cls.wrapping_add(0x20));
            let (a1, v1) = (b & 0xf, b >> 4);
            let a0 = pal.wrapping_sub((a1.wrapping_mul(v1)) << 10).wrapping_sub(16);
            if v1 != 0 {
                let (mut s3, mut s2, s5, mut s1) = (a0, a0.wrapping_add((a2 << 10) + 16), a1 << 10, v1);
                loop {
                    let s0 = c.rd16(s3);
                    let dst = c.rd32(p);
                    for q in 0..7 {
                        let v = c.rd128(template + 16 * q);
                        c.wr128(dst + 16 * q, v.0, v.1);
                    }
                    s3 = s3.wrapping_add(2);
                    s1 -= 1;
                    let t = c.rd32(p);
                    c.wr16(t.wrapping_add(0x24), s0);
                    let t = c.rd32(p);
                    c.wr32(t.wrapping_add(0x64), s2);
                    s2 = s2.wrapping_add(s5);
                    let v0 = c.rd32(p).wrapping_add(112);
                    let a = k.gp(c, gp, 0x43_7db4);
                    c.wr32(a, v0);
                    if s1 == 0 {
                        break;
                    }
                }
            }
            let t = c.rd32(p);
            c.wr32(t, 0x3000_0003);
            let t = c.rd32(p);
            c.wr32(t.wrapping_add(4), global);
            let t = c.rd32(p);
            c.wr32(t.wrapping_add(8), 0);
            let t = c.rd32(p);
            c.wr32(t.wrapping_add(12), 0x5000_0003);
            let t = c.rd32(p);
            c.wr32(p, t.wrapping_add(16));
            c.wr32(t.wrapping_add(16), 0x2000_0000);
            let t = c.rd32(p);
            c.wr32(t.wrapping_add(4), s4.wrapping_add(16));
            let t = c.rd32(p);
            c.wr32(t.wrapping_add(8), 0);
            let t = c.rd32(p);
            c.wr32(t.wrapping_add(12), 0);
            let v1 = c.rd32(p).wrapping_add(16);
            let lim = c.rd32(endp);
            a3 = s7;
            let a = k.gp(c, gp, 0x43_7e60);
            c.wr32(a, v1);
            if sx(a3) >= sx(lim) {
                break;
            }
        }
    }
    c.wr32(fp, 0x2000_0000);
    let v1 = c.rd32(p);
    c.wr32(fp.wrapping_add(12), 0);
    c.wr32(fp.wrapping_add(4), v1);
    c.wr32(fp.wrapping_add(8), 0);
}

/// 0x43E6E8: the classes' chain. From the cull's per-class nearest depths (scratchpad 0x3A00, 0x40000000 for a class not seen)
/// against each class's limit (0x2006F0): the class numbers of the drawn ones (0x200AB0, ended by -1) and their flags (1 near,
/// 3 within half the limit) into the scratchpad's per-texture bytes at 0x3E00 for each texture of the class (0x200E70, 16
/// bytes a class, ended by 0xFF); then per texture (gp+0x37DC of them, their records at 0x1FFEF0) one or two 112-byte texture
/// upload packets from the template at gp-0x2820 at the chain pointer, each texture's VRAM block (`a0` >> 8 on) into 0x2002F0.
fn f43e6e8(c: &mut Cpu, k: F, a0: u32) {
    let gp = c.r[28] as u32;
    let mut t6 = k.a(c, 0x43_e6ec, 0x43_e6f0);
    let mut t7 = SPR + 0x3a00;
    let t5 = SPR + 0x3e00;
    let mut t4 = k.a(c, 0x43_e6fc, 0x43_e700);
    let t2 = c.rd32(k.gp(c, gp, 0x43_e708));
    let mut t3 = k.a(c, 0x43_e70c, 0x43_e710);
    let mut t1 = 0u32;
    let mut t8 = c.rd32(k.gp(c, gp, 0x43_e71c));
    let mut t9 = a0 >> 8;
    for q in 0..16 {
        c.wr128(t5 + 16 * q, 0, 0);
    }
    loop {
        let done = t1 == t2;
        t1 = t1.wrapping_add(1);
        if done {
            break;
        }
        let at = c.rd32(t7);
        t7 += 4;
        let v0 = c.rd32(t6);
        t6 = t6.wrapping_add(4);
        t3 = t3.wrapping_add(16);
        if at == 0x4000_0000 {
            continue;
        }
        c.wr32(t4, t1.wrapping_sub(1));
        if ((v0 << 1).wrapping_sub(at) as i32) <= 0 {
            continue;
        }
        let v1 = if (v0.wrapping_sub(at) as i32) <= 0 { 1 } else { 3 };
        t4 = t4.wrapping_add(4);
        let mut p = t3.wrapping_sub(16);
        loop {
            let b = c.rd8(p) as u32;
            p = p.wrapping_add(1);
            if b == 255 {
                break;
            }
            let a = b.wrapping_add(t5);
            let v = c.rd8(a) | v1 as u8;
            c.wr8(a, v);
        }
    }
    c.wr32(t4, u32::MAX);
    let tpl: [Q; 5] = std::array::from_fn(|q| {
        let a = k.gp(c, gp, 0x43_e7d0 + 4 * q as u32);
        c.rd128(a)
    });
    let mut t2 = k.a(c, 0x43_e7e4, 0x43_e7e8);
    let mut a3 = k.a(c, 0x43_e7ec, 0x43_e7f0);
    let mut t0 = SPR + 0x3e00;
    let t1 = c.rd32(k.gp(c, gp, 0x43_e7fc)).wrapping_add(t0);
    // One packet: the template, its quadword count `v0` (from the texture's size class `v1`: 1 << v1 texels a side), the
    // texture's address. SLLV shifts its rt by rs (`sllv at, v1, at` at 0x43E878 is at << v1, the listing naming rs first).
    let packet = |c: &mut Cpu, t8: u32, v1: u32, a0: u32, half: u32, t9: &mut u32, t2: u32, path_b: bool| {
        for (q, &(lo, hi)) in tpl[..4].iter().enumerate() {
            c.wr128(t8 + 16 * q as u32, lo, hi);
        }
        c.wr128(t8 + 0x40, 0, 0);
        c.wr128(t8 + 0x50, tpl[4].0, tpl[4].1);
        c.wr32(t8 + 0x48, 83);
        c.wr16(t8 + 0x24, *t9 as u16);
        let v = v1.wrapping_sub(6);
        if (v as i32) > 0 {
            c.wr8(t8 + 0x26, 1u32.wrapping_shl(v) as u8);
        }
        let at = 1u32.wrapping_shl(v1);
        c.wr32(t8 + 0x30, at);
        c.wr32(t8 + 0x34, at);
        let big = at.wrapping_shl(v1);
        let a0 = if path_b { a0.wrapping_add(big << 2) } else { a0 };
        let v0 = big >> 4;
        c.wr32(t8 + 0x50, 0x8000u32.wrapping_add(v0));
        c.wr128(t8 + 0x60, 0, 0);
        c.wr32(t8 + 0x64, a0);
        c.wr32(t8 + 0x60, 0x3000_0000u32.wrapping_add(v0));
        c.wr16(t2.wrapping_sub(half), *t9 as u16);
        *t9 = t9.wrapping_add(v0 >> 4);
        c.wr32(t8 + 0x6c, 0x5000_0000u32.wrapping_add(v0));
    };
    loop {
        if t0 == t1 {
            break;
        }
        let at = c.rd8(t0) as u32;
        t0 += 1;
        let a0 = c.rd32(a3);
        a3 = a3.wrapping_add(4);
        c.wr32(t2, 0);
        t2 = t2.wrapping_add(4);
        if at == 0 {
            continue;
        }
        let v1 = a0 >> 28;
        let a0 = a0 & 0x0fff_ffff;
        if at & 2 != 0 {
            packet(c, t8, v1, a0, 4, &mut t9, t2, false);
            let over = (t9.wrapping_sub(16379) as i32) > 0;
            t8 = t8.wrapping_add(112);
            if over {
                t8 = t8.wrapping_sub(112);
                break;
            }
        }
        packet(c, t8, v1.wrapping_sub(1), a0, 2, &mut t9, t2, true);
        let over = (t9.wrapping_sub(16379) as i32) > 0;
        t8 = t8.wrapping_add(112);
        if over {
            t8 = t8.wrapping_sub(112);
            break;
        }
    }
    let a = k.gp(c, gp, 0x43_e988);
    c.wr32(a, t8);
}

/// 0x43E998: for each draw record of the list (to one whose first word is 0) the joint matrices (0x439E00) and the skin job
/// (0x3F6FF0), which returns the next record; the caller's registers saved at 0x201D80 as the game saves them.
fn f43e998(c: &mut Cpu, k: F, a0: u32, a1: u32, sp: u32, ra: u64) {
    let save = k.a(c, 0x43_e99c, 0x43_e9a0);
    let regs = [c.r[16], c.r[17], c.r[18], c.r[19], c.r[20], c.r[21], c.r[22], c.r[23], c.r[28], sx(sp), c.r[30], ra];
    for (n, &v) in regs.iter().enumerate() {
        c.wr64(save + 8 * n as u32, v);
    }
    let joints = k.call(c, 0x43_e9e8, 0x43_9e00);
    let skin = k.call(c, 0x43_e9f8, 0x3f_6ff0);
    let mut rec = a0;
    while c.rd32(rec) != 0 {
        count(3, 1);
        f439e00(c, joints, rec, a1);
        rec = skin_job(c, skin, rec, a1, k.pc(0x43_ea00) as u64);
    }
}

// ---- 0x439E00: the joint matrices -------------------------------------------------------------------------------------------

/// The four halfwords of `x` as words, each shifted left by 3 (PEXTLH with zero, then PSRLW 13): the pose buffers' unsigned
/// 4.12 values, which ITOF15 makes floats.
#[inline(always)]
fn hu(x: u64) -> [u32; 4] {
    std::array::from_fn(|i| ((x >> (16 * i)) as u16 as u32) << 3)
}
/// The four halfwords of `x` sign-extended (PEXTLH or PEXTUH with zero, then PSRAW 16).
#[inline(always)]
fn hs(x: u64) -> [u32; 4] {
    std::array::from_fn(|i| (x >> (16 * i)) as u16 as i16 as i32 as u32)
}
#[inline(always)]
fn itof<const N: usize, const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    uycpu::vu0::itof::<N, D>(c, ft, fs);
}

/// rows `m`, `m+1`, `m+2` of VF times lanes x, y, z of `v` (VMULAx, VMADDAy, VMADDz) into `fd`.
#[inline(always)]
fn rot3(c: &mut Cpu, fd: usize, m: usize, v: usize) {
    vop::<6, 15>(c, ACC, m, bc(c, v, 0));
    vop::<2, 15>(c, ACC, m + 1, bc(c, v, 1));
    vop::<2, 15>(c, fd, m + 2, bc(c, v, 2));
}
/// The same plus row `m+3` times vf0.w (VMADDAz, VMADDw).
#[inline(always)]
fn rot4(c: &mut Cpu, fd: usize, m: usize, v: usize) {
    vop::<6, 15>(c, ACC, m, bc(c, v, 0));
    vop::<2, 15>(c, ACC, m + 1, bc(c, v, 1));
    vop::<2, 15>(c, ACC, m + 2, bc(c, v, 2));
    vop::<2, 15>(c, fd, m + 3, bc(c, 0, 3));
}
/// A quaternion's squared length into lane `l` of `v` (lane `l` of `v` plus the other lanes, each of the last two times vf4's
/// lane `l`, which is 1.0: VADDA, VMADDA, VMADD).
#[inline(always)]
fn sum4y(c: &mut Cpu, v: usize) {
    vop::<0, 4>(c, ACC, v, bc(c, v, 0));
    vop::<2, 4>(c, ACC, 4, bc(c, v, 2));
    vop::<2, 4>(c, v, 4, bc(c, v, 3));
}
#[inline(always)]
fn sum4x(c: &mut Cpu, v: usize) {
    vop::<0, 8>(c, ACC, v, bc(c, v, 1));
    vop::<2, 8>(c, ACC, 4, bc(c, v, 2));
    vop::<2, 8>(c, v, 4, bc(c, v, 3));
}

/// 0x439E00: the record `rec`'s joint matrices into the scratchpad at 0x40 j. The joint table and the two pose buffers come
/// in by SPR_TO's chain (tags at the staging buffer's 0x1FC0); each joint's rotation (a quaternion, the two poses blended by
/// the record's +4 and the shorter way round, normalized, unless flag 0x400) and position and scale make its local matrix,
/// times its parent's (the table's scratchpad pointer); the attachments (+8) and the record's second list (+0xC) are mixed in;
/// then each times its bind matrix (class +0x14, by SPR_TO to 0x1C00).
fn f439e00(c: &mut Cpu, _k: F, rec: u32, staging: u32) {
    qmtc2(c, 28, (sx(staging), 0));
    let nj = c.rd16(rec + 0x10) as i16 as i32;
    let blend = c.rd32(rec + 4);
    if nj == 0 {
        vop::<6, 15>(c, 1, 0, bc(c, 0, 0));
        vop::<6, 15>(c, 2, 0, bc(c, 0, 0));
        let v = mr32(c.vf[0]);
        vmove::<15>(c, 3, v);
        vop::<0, 8>(c, 1, 1, bc(c, 0, 3));
        vop::<0, 4>(c, 2, 2, bc(c, 0, 3));
        for (q, v) in [1usize, 2, 3, 0].into_iter().enumerate() {
            sqc2(c, v, SPR + 16 * q as u32);
        }
        return;
    }
    let tags = staging.wrapping_add(0x1fc0);
    c.wr32(tags, 0x3000_0000 | nj as u32);
    let table = c.rd32(rec + 0x18);
    let t1 = c.rd32(rec + 0x24);
    c.wr32(tags + 4, table);
    let mut t9 = c.rd32(rec + 8);
    let mut t8 = c.rd32(rec + 0xc);
    let tag1 = (0x3000_0000 | t1) & if blend == 0 { 0xffff } else { u32::MAX };
    let pose1 = c.rd32(rec + 0x20);
    qmtc2(c, 29, (c.r[18], c.rh[18]));
    let flags = c.rd16(rec + 0x12) as i16 as i32;
    c.wr32(tags + 0x10, tag1);
    vop::<6, 15>(c, 1, 0, bc(c, 0, 0));
    c.wr32(tags + 0x14, pose1);
    vop::<6, 15>(c, 2, 0, bc(c, 0, 0));
    let q2 = c.rd32(rec + 0x34);
    let v = mr32(c.vf[0]);
    vmove::<15>(c, 3, v);
    let pose2 = c.rd32(rec + 0x30);
    vop::<0, 14>(c, 4, 0, bc(c, 0, 3));
    vop::<1, 1>(c, 4, 0, c.vf[0]);
    c.wr32(tags + 0x20, q2);
    vop::<0, 8>(c, 1, 1, bc(c, 0, 3));
    c.wr32(tags + 0x24, pose2);
    vop::<0, 4>(c, 2, 2, bc(c, 0, 3));
    lqc2(c, 7, rec);
    let at = SPR + 0x2000 + ((nj as u32) << 4);
    let a0q = c.rd32(rec + 0x24);
    let mut v0 = (c.rd16(rec + 0x28) as u32).wrapping_add(at);
    vop::<1, 8>(c, 6, 4, bc(c, 7, 1));
    let a1q = c.rd16(rec + 0x38) as u32;
    let v = mr32(c.vf[7]);
    vmove::<15>(c, 7, v);
    let mut v1 = (c.rd16(rec + 0x2c) as u32).wrapping_add(at);
    let a2q = c.rd16(rec + 0x3c) as u32;
    c.wr32(at - 4, u32::MAX);
    let a0 = (a0q << 4).wrapping_add(at);
    let t1 = c.rd32(rec + 0x34) << 4;
    c.wr32(a0.wrapping_sub(4), u32::MAX);
    let mut a1 = a1q.wrapping_add(a0);
    qmtc2(c, 30, (c.r[17], c.rh[17]));
    let s1 = t1.wrapping_add(a0);
    c.wr32(s1.wrapping_sub(4), u32::MAX);
    let mut a2 = a2q.wrapping_add(a0);
    let t5 = staging.wrapping_add(16);
    vop::<1, 8>(c, 5, 0, bc(c, 0, 3));
    c.wr128(staging, 0, 0);
    qmtc2(c, 31, (c.r[16], c.rh[16]));
    spr_to_chain(c, 0x104, tags, 0x2000);
    let t6 = SPR;
    let mut at = at;
    let mut a0 = a0;
    if blend != 0 {
        // Two poses.
        let (mut n, mut p, mut s) = (nj, t6.wrapping_sub(64), t5.wrapping_sub(32));
        loop {
            p = p.wrapping_add(64);
            sqc2(c, 4, p);
            s = s.wrapping_add(32);
            sqc2(c, 4, p + 16);
            n -= 1;
            sqc2(c, 4, s);
            sqc2(c, 4, s + 16);
            if n <= 0 {
                break;
            }
        }
        let (mut n, mut t0, mut p) = (nj, t6 | 0x2000, t6.wrapping_sub(256));
        loop {
            for k in 0..4 {
                lqc2(c, 16 + k, t0 + 16 * k as u32);
            }
            p = p.wrapping_add(256);
            n -= 4;
            for k in 0..4u32 {
                sqc2(c, 16 + k as usize, p + 0x20 + 0x40 * k);
            }
            for k in 0..4u32 {
                sqc2(c, 16 + k as usize, p + 0x30 + 0x40 * k);
            }
            for k in 0..4u32 {
                c.wr32(p + 0x3c + 0x40 * k, 0);
            }
            t0 = t0.wrapping_add(64);
            if n <= 0 {
                break;
            }
        }
        let (mut s0, mut t3) = (t5.wrapping_sub(16), t5.wrapping_sub(28));
        while v0 != v1 {
            let a3 = c.rd64(v0);
            let j = c.rd8(v0 + 6) as u32;
            v0 = v0.wrapping_add(8);
            c.vf[8] = hu(a3);
            let t4 = (j << 6).wrapping_add(t6);
            itof::<3, 14>(c, 8, 8);
            if (a3 as i64) < 0 {
                c.wr32(t3 + 0xc, t4);
                t3 = t4;
                sqc2(c, 8, t4);
            } else {
                let t1 = (j << 5).wrapping_add(t5);
                c.wr32(s0 + 0xc, t1);
                s0 = t1;
                sqc2(c, 8, t1);
            }
        }
        let mut n = c.rd16(rec + 0x2e) as i16 as i32;
        let mut t4 = t5.wrapping_sub(72);
        while n != 0 {
            let a3 = c.rd64(v1);
            let j = c.rd8(v1 + 6) as i8 as i32;
            n -= 1;
            c.vf[8] = hs(a3);
            v1 = v1.wrapping_add(8);
            itof::<0, 14>(c, 8, 8);
            let t0 = ((j << 6) as u32).wrapping_add(t6);
            let w = c.rd32(t0 + 0x2c);
            c.wr32(t4 + 0x3c, t0);
            t4 = t0;
            c.wr32(t0 + 0x3c, 0);
            sqc2(c, 8, t0 + 0x20);
            c.wr32(t0 + 0x2c, w);
        }
        while a1 != a2 {
            let a3 = c.rd64(a1);
            let j = c.rd8(a1 + 6) as u32;
            a1 = a1.wrapping_add(8);
            c.vf[8] = hu(a3);
            let v1 = (j << 6).wrapping_add(t6);
            itof::<3, 14>(c, 8, 8);
            if (a3 as i64) < 0 {
                let t0 = c.rd32(v1 + 0xc);
                sqc2(c, 8, v1 + 0x10);
                if t0 == 0 {
                    c.wr32(t3 + 0xc, v1);
                    t3 = v1;
                }
            } else {
                let t1 = (j << 5).wrapping_add(t5);
                let t0 = c.rd32(t1 + 0x1c);
                sqc2(c, 8, t1 + 0x10);
                if t0 == 0 {
                    c.wr32(s0 + 0xc, t1);
                    s0 = t1;
                }
            }
        }
        c.wr32(t3 + 0xc, 0);
        c.wr32(s0 + 0xc, 0);
        let mut n = c.rd16(rec + 0x3e) as i16 as i32;
        while n != 0 {
            let a3 = c.rd64(a2);
            let j = c.rd8(a2 + 6) as i8 as i32;
            n -= 1;
            c.vf[8] = hs(a3);
            a2 = a2.wrapping_add(8);
            itof::<0, 14>(c, 8, 8);
            let t0 = ((j << 6) as u32).wrapping_add(t6);
            let link = c.rd32(t0 + 0x3c);
            sqc2(c, 8, t0 + 0x30);
            c.wr32(t0 + 0x3c, link);
            if link == 0 {
                c.wr32(t4 + 0x3c, t0);
                t4 = t0;
                c.wr32(t0 + 0x3c, 0);
            }
        }
        // The two poses' positions and scales blended: the scratchpad list (staging +0) and the third list (+4).
        let mut v1 = c.rd32(t5.wrapping_sub(16));
        vmove::<1>(c, 10, c.vf[0]);
        while v1 != 0 {
            let v0 = v1;
            lqc2(c, 8, v0);
            lqc2(c, 9, v0 + 16);
            vop::<6, 14>(c, ACC, 8, bc(c, 6, 0));
            vop::<2, 14>(c, 10, 9, bc(c, 7, 0));
            v1 = c.rd32(v0 + 0xc);
            sqc2(c, 10, v0 + 16);
        }
        let mut v0 = c.rd32(t5.wrapping_sub(12));
        while v0 != 0 {
            lqc2(c, 8, v0 + 0x20);
            lqc2(c, 9, v0 + 0x30);
            vop::<6, 14>(c, ACC, 8, bc(c, 6, 0));
            vop::<2, 14>(c, 10, 9, bc(c, 7, 0));
            let w = c.rd32(v0 + 0x2c);
            sqc2(c, 10, v0 + 0x20);
            c.wr32(v0 + 0x2c, w);
            v0 = c.rd32(v0 + 0x3c);
        }
        // The rotations, two joints at a time: the poses' quaternions blended.
        let normalize = flags & 0x400 == 0;
        let mut n = nj;
        let mut t4 = t6.wrapping_sub(128);
        loop {
            let a3 = c.rd128(at);
            if n <= 0 {
                break;
            }
            n -= 2;
            let t0 = c.rd128(a0);
            c.vf[8] = hs(a3.0);
            c.vf[9] = hs(t0.0);
            c.vf[10] = hs(a3.1);
            c.vf[11] = hs(t0.1);
            itof::<3, 15>(c, 8, 8);
            if normalize {
                itof::<3, 15>(c, 9, 9);
                itof::<3, 15>(c, 10, 10);
                itof::<3, 15>(c, 11, 11);
                vop::<6, 15>(c, 12, 8, c.vf[9]);
                vop::<6, 15>(c, 13, 10, c.vf[11]);
                vop::<6, 15>(c, 14, 9, bc(c, 5, 0));
                vop::<6, 15>(c, 15, 11, bc(c, 5, 0));
                vop::<6, 15>(c, ACC, 8, bc(c, 6, 0));
                vop::<2, 15>(c, 8, 9, bc(c, 7, 0));
                vop::<2, 15>(c, 9, 14, bc(c, 7, 0));
                at = at.wrapping_add(16);
                sum4y(c, 12);
                a0 = a0.wrapping_add(16);
                sum4y(c, 13);
                vop::<6, 15>(c, ACC, 10, bc(c, 6, 0));
                let d0 = qmfc2(c, 12).0;
                vop::<2, 15>(c, 10, 11, bc(c, 7, 0));
                t4 = t4.wrapping_add(128);
                vop::<2, 15>(c, 11, 15, bc(c, 7, 0));
                let d1 = qmfc2(c, 13).0;
                if (d0 as i64) >= 0 {
                    vop::<6, 15>(c, 9, 8, c.vf[8]);
                } else {
                    vmove::<15>(c, 8, c.vf[9]);
                    vop::<6, 15>(c, 9, 9, c.vf[9]);
                }
                if (d1 as i64) >= 0 {
                    vop::<6, 15>(c, 11, 10, c.vf[10]);
                } else {
                    vmove::<15>(c, 10, c.vf[11]);
                    vop::<6, 15>(c, 11, 11, c.vf[11]);
                }
                sum4x(c, 9);
                sum4x(c, 11);
                qop::<2>(c, c.vf[0][3], c.vf[9][0]);
                vop::<6, 15>(c, 8, 8, qq(c));
                qop::<2>(c, c.vf[0][3], c.vf[11][0]);
                vop::<6, 15>(c, 10, 10, qq(c));
            } else {
                t4 = t4.wrapping_add(128);
                itof::<3, 15>(c, 9, 9);
                at = at.wrapping_add(16);
                itof::<3, 15>(c, 10, 10);
                itof::<3, 15>(c, 11, 11);
                vop::<6, 15>(c, ACC, 8, bc(c, 6, 0));
                vop::<2, 15>(c, 8, 9, bc(c, 7, 0));
                vop::<6, 15>(c, ACC, 10, bc(c, 6, 0));
                vop::<2, 15>(c, 10, 11, bc(c, 7, 0));
                a0 = a0.wrapping_add(16);
            }
            sqc2(c, 8, t4);
            sqc2(c, 10, t4 + 0x40);
        }
    } else {
        // One pose.
        let (mut n, mut p, mut s) = (nj, t6.wrapping_sub(64), t5.wrapping_sub(32));
        loop {
            p = p.wrapping_add(64);
            s = s.wrapping_add(32);
            sqc2(c, 4, p + 16);
            n -= 1;
            sqc2(c, 4, s);
            if n <= 0 {
                break;
            }
        }
        let (mut n, mut t0, mut p) = (nj, t6 | 0x2000, t6.wrapping_sub(256));
        loop {
            for k in 0..4 {
                lqc2(c, 16 + k, t0 + 16 * k as u32);
            }
            p = p.wrapping_add(256);
            n -= 4;
            for k in 0..4u32 {
                sqc2(c, 16 + k as usize, p + 0x20 + 0x40 * k);
            }
            t0 = t0.wrapping_add(64);
            if n <= 0 {
                break;
            }
        }
        let mut s0 = t5.wrapping_sub(16);
        while v0 != v1 {
            let a3 = c.rd64(v0);
            let j = c.rd8(v0 + 6) as u32;
            v0 = v0.wrapping_add(8);
            c.vf[8] = hu(a3);
            let t4 = (j << 6).wrapping_add(t6);
            itof::<3, 14>(c, 8, 8);
            if (a3 as i64) < 0 {
                sqc2(c, 8, t4 + 16);
            } else {
                let t1 = (j << 5).wrapping_add(t5);
                c.wr32(s0 + 0xc, t1);
                s0 = t1;
                sqc2(c, 8, t1);
            }
        }
        c.wr32(s0 + 0xc, 0);
        let mut n = c.rd16(rec + 0x2e) as i16 as i32;
        while n != 0 {
            let a3 = c.rd64(v1);
            let j = c.rd8(v1 + 6) as i8 as i32;
            n -= 1;
            c.vf[8] = hs(a3);
            v1 = v1.wrapping_add(8);
            itof::<0, 14>(c, 8, 8);
            let t0 = ((j << 6) as u32).wrapping_add(t6);
            let w = c.rd32(t0 + 0x2c);
            sqc2(c, 8, t0 + 0x20);
            c.wr32(t0 + 0x2c, w);
        }
        let mut n = nj;
        let mut t4 = t6.wrapping_sub(64);
        let a3 = c.rd64(at);
        at = at.wrapping_add(8);
        c.vf[8] = hs(a3);
        loop {
            let a3 = c.rd64(at);
            itof::<3, 15>(c, 9, 8);
            n -= 1;
            at = at.wrapping_add(8);
            c.vf[8] = hs(a3);
            t4 = t4.wrapping_add(64);
            sqc2(c, 9, t4);
            if n <= 0 {
                break;
            }
        }
    }
    // The attachments (+8): their joints' rotations, positions and scales mixed with the attachment's by its weight.
    while t9 != 0 {
        let cnt = c.rd16(t9 + 2) as u32;
        let madr = c.rd32(t9 + 0x10);
        let q = cnt + cnt + cnt;
        spr_to(c, 0x100, madr, q, 0x2000);
        let wgt = c.rd32(t9 + 8);
        let mut t1 = SPR + 0x2000;
        let t0 = (q << 4).wrapping_add(t1);
        c.vf[21] = [wgt, (sx(wgt) >> 32) as u32, 0, 0];
        t9 = c.rd32(t9 + 0x1c);
        vop::<1, 8>(c, 20, 4, c.vf[21]);
        loop {
            let at = c.rd32(t1 + 0x2c);
            t1 = t1.wrapping_add(0x30);
            lqc2(c, 13, t1 - 0x30);
            lqc2(c, 14, t1 - 0x20);
            lqc2(c, 15, t1 - 0x10);
            lqc2(c, 10, at);
            lqc2(c, 11, at + 16);
            lqc2(c, 12, at + 32);
            vop::<6, 15>(c, 10, 10, bc(c, 20, 0));
            vop::<6, 15>(c, 13, 13, bc(c, 21, 0));
            vop::<6, 14>(c, 11, 11, bc(c, 20, 0));
            vop::<6, 14>(c, 14, 14, bc(c, 21, 0));
            vop::<6, 14>(c, 12, 12, bc(c, 20, 0));
            vop::<6, 14>(c, 15, 15, bc(c, 21, 0));
            vop::<0, 15>(c, 16, 10, c.vf[13]);
            vop::<6, 15>(c, 18, 10, c.vf[13]);
            vop::<0, 14>(c, 11, 11, c.vf[14]);
            vop::<0, 14>(c, 12, 12, c.vf[15]);
            vop::<6, 15>(c, 17, 16, c.vf[16]);
            sum4y(c, 18);
            sum4x(c, 17);
            // QMFC2 v0, vf18 (0x43A674): v0 whole, whose high half the skin job keeps in VU0's quadword 255.
            (c.r[2], c.rh[2]) = qmfc2(c, 18);
            if (c.r[2] as i64) < 0 {
                vop::<1, 15>(c, 16, 10, c.vf[13]);
                vop::<6, 15>(c, 17, 16, c.vf[16]);
                sum4x(c, 17);
            }
            qop::<2>(c, c.vf[0][3], c.vf[17][0]);
            sqc2(c, 11, at + 16);
            sqc2(c, 12, at + 32);
            let had = c.rd32(at + 0x1c);
            let v1 = c.rd32(t1 - 0x14);
            if had == 0 {
                c.wr32(at + 0x1c, v1);
            }
            vop::<6, 15>(c, 10, 16, qq(c));
            sqc2(c, 10, at);
            if t1 == t0 {
                break;
            }
        }
    }
    // The record's second list (+0xC): a joint's rotation times the entry's (byte +3 0) or mixed with it by its weight.
    vmove::<15>(c, 19, c.vf[0]);
    while t8 != 0 {
        let v1 = t8;
        let at = c.rd32(v1 + 4);
        let kind = c.rd8(v1 + 3);
        lqc2(c, 8, at);
        lqc2(c, 10, at + 16);
        lqc2(c, 12, at + 32);
        lqc2(c, 9, v1 + 16);
        lqc2(c, 11, v1 + 32);
        lqc2(c, 13, v1 + 48);
        t8 = c.rd32(v1 + 8);
        if kind == 0 {
            vop::<6, 1>(c, 14, 9, c.vf[8]);
            vop::<6, 14>(c, 15, 9, c.vf[8]);
            vop::<6, 14>(c, 16, 9, bc(c, 8, 3));
            vop::<6, 14>(c, 17, 8, bc(c, 9, 3));
            uycpu::vu0::opmula(c, 8, 9);
            uycpu::vu0::opmsub(c, 18, 9, 8);
            vop::<0, 8>(c, ACC, 15, bc(c, 15, 1));
            vop::<2, 8>(c, 15, 4, bc(c, 15, 2));
            vop::<0, 14>(c, 8, 16, c.vf[17]);
            vop::<0, 14>(c, 8, 8, c.vf[18]);
            vop::<1, 1>(c, 8, 14, bc(c, 15, 0));
            vop::<6, 14>(c, 19, 10, c.vf[11]);
            vop::<0, 14>(c, 12, 12, c.vf[13]);
        } else {
            let wv = c.rd32(v1 + 0xc);
            c.vf[20] = [wv, (sx(wv) >> 32) as u32, 0, 0];
            vop::<1, 1>(c, 20, 0, bc(c, 20, 0));
            vop::<6, 15>(c, 8, 8, bc(c, 20, 3));
            vop::<6, 15>(c, 9, 9, bc(c, 20, 0));
            vop::<6, 14>(c, 10, 10, bc(c, 20, 3));
            vop::<6, 14>(c, 11, 11, bc(c, 20, 0));
            vop::<6, 14>(c, 12, 12, bc(c, 20, 3));
            vop::<6, 14>(c, 13, 13, bc(c, 20, 0));
            vop::<0, 15>(c, 16, 8, c.vf[9]);
            vop::<6, 15>(c, 18, 8, c.vf[9]);
            vop::<0, 14>(c, 19, 10, c.vf[11]);
            vop::<0, 14>(c, 12, 12, c.vf[13]);
            vop::<6, 15>(c, 17, 16, c.vf[16]);
            sum4y(c, 18);
            sum4x(c, 17);
            // QMFC2 v0, vf18 (0x43A7A4), as above.
            (c.r[2], c.rh[2]) = qmfc2(c, 18);
            if (c.r[2] as i64) < 0 {
                vop::<1, 15>(c, 16, 8, c.vf[9]);
                vop::<6, 15>(c, 17, 16, c.vf[16]);
                sum4x(c, 17);
            }
            qop::<2>(c, c.vf[0][3], c.vf[17][0]);
            vop::<6, 15>(c, 8, 16, qq(c));
        }
        sqc2(c, 8, at);
        sqc2(c, 19, at + 16);
        sqc2(c, 12, at + 32);
    }
    // The bind matrices to 0x1C00 (the skin recording takes this transfer too), then each joint's local matrix.
    let bind = c.rd32(rec + 0x1c);
    let mark = SPR + ((nj as u32) << 6) + 0x1bfc;
    c.wr32(mark, u32::MAX);
    spr_to(c, 0x1d00, bind, (nj as u32) << 2, 0x1c00);
    let end = SPR + ((nj as u32) << 6);
    let mut a3 = SPR;
    loop {
        lqc2(c, 10, a3);
        vmove::<15>(c, 16, c.vf[1]);
        vop::<0, 15>(c, 11, 10, c.vf[10]);
        vmove::<15>(c, 17, c.vf[2]);
        vmove::<15>(c, 18, c.vf[3]);
        vmove::<15>(c, 19, c.vf[0]);
        vop::<6, 14>(c, 12, 11, bc(c, 10, 3));
        vop::<6, 14>(c, 13, 11, bc(c, 10, 0));
        vop::<6, 6>(c, 14, 11, bc(c, 10, 1));
        vop::<6, 2>(c, 15, 11, bc(c, 10, 2));
        vop::<0, 8>(c, 17, 0, bc(c, 12, 2));
        vop::<1, 8>(c, 18, 0, bc(c, 12, 1));
        vop::<0, 4>(c, 18, 0, bc(c, 12, 0));
        vop::<1, 8>(c, 16, 16, bc(c, 14, 1));
        vop::<1, 4>(c, 17, 17, bc(c, 13, 0));
        vop::<1, 2>(c, 18, 18, bc(c, 13, 0));
        vop::<1, 4>(c, 16, 13, bc(c, 12, 2));
        vop::<0, 2>(c, 16, 13, bc(c, 12, 1));
        vop::<1, 2>(c, 17, 14, bc(c, 12, 0));
        vop::<0, 8>(c, 17, 17, bc(c, 13, 1));
        vop::<0, 8>(c, 18, 18, bc(c, 13, 2));
        vop::<0, 4>(c, 18, 18, bc(c, 14, 2));
        a3 = a3.wrapping_add(64);
        vop::<1, 8>(c, 16, 16, bc(c, 15, 2));
        let t0 = c.rd32(a3 - 0x24);
        vop::<1, 4>(c, 17, 17, bc(c, 15, 2));
        let t1 = c.rd32(a3 - 0x14);
        vop::<1, 2>(c, 18, 18, bc(c, 14, 1));
        lqc2(c, 8, a3 - 0x30);
        lqc2(c, 9, a3 - 0x20);
        if t0 != 0 {
            vop::<6, 14>(c, 16, 16, bc(c, 8, 0));
            vop::<6, 14>(c, 17, 17, bc(c, 8, 1));
            vop::<6, 14>(c, 18, 18, bc(c, 8, 2));
        }
        vmove::<15>(c, 12, c.vf[1]);
        if t1 == 0 {
            vmove::<15>(c, 13, c.vf[2]);
            vmove::<15>(c, 14, c.vf[3]);
            vmove::<15>(c, 15, c.vf[0]);
        } else {
            lqc2(c, 12, t1);
            lqc2(c, 13, t1 + 16);
            lqc2(c, 14, t1 + 32);
            lqc2(c, 15, t1 + 48);
        }
        vop::<0, 14>(c, 19, 19, c.vf[9]);
        rot3(c, 20, 12, 16);
        rot3(c, 21, 12, 17);
        rot3(c, 22, 12, 18);
        vop::<6, 15>(c, ACC, 12, bc(c, 19, 0));
        vop::<2, 15>(c, ACC, 13, bc(c, 19, 1));
        vop::<2, 15>(c, ACC, 14, bc(c, 19, 2));
        vop::<2, 15>(c, 23, 15, bc(c, 0, 3));
        for k in 0..4u32 {
            sqc2(c, 20 + k as usize, a3 - 0x40 + 16 * k);
        }
        if a3 == end {
            break;
        }
    }
    // The scale list (staging +0xC): each joint's rows times its scale (the two poses' blended).
    let mut t4 = c.rd32(t5.wrapping_sub(4));
    let two = blend != 0;
    loop {
        let t3 = (t4.wrapping_sub(t5) << 1).wrapping_add(t6);
        if t4 == 0 {
            break;
        }
        lqc2(c, 1, t4);
        if two {
            lqc2(c, 8, t4 + 16);
            vop::<6, 14>(c, ACC, 1, bc(c, 6, 0));
            vop::<2, 14>(c, 1, 8, bc(c, 7, 0));
        }
        lqc2(c, 2, t3);
        lqc2(c, 3, t3 + 16);
        lqc2(c, 4, t3 + 32);
        vop::<6, 14>(c, 2, 2, bc(c, 1, 0));
        t4 = c.rd32(t4 + 0xc);
        vop::<6, 14>(c, 3, 3, bc(c, 1, 1));
        vop::<6, 14>(c, 4, 4, bc(c, 1, 2));
        sqc2(c, 2, t3);
        sqc2(c, 3, t3 + 16);
        sqc2(c, 4, t3 + 32);
    }
    // Each joint's matrix times its bind matrix.
    let (mut t6, mut t5) = (SPR, SPR | 0x1c00);
    loop {
        lqc2(c, 15, t5 + 0x30);
        lqc2(c, 14, t5 + 0x20);
        lqc2(c, 13, t5 + 0x10);
        lqc2(c, 12, t5);
        for k in 0..4 {
            lqc2(c, 20 + k, t6 + 16 * k as u32);
        }
        rot4(c, 19, 20, 15);
        rot3(c, 16, 20, 12);
        rot3(c, 17, 20, 13);
        rot3(c, 18, 20, 14);
        for k in 0..4u32 {
            sqc2(c, 16 + k as usize, t6 + 16 * k);
        }
        t5 = t5.wrapping_add(64);
        t6 = t6.wrapping_add(64);
        if t6 == end {
            break;
        }
    }
    let _ = s1;
}

// ---- 0x3F6FF0: the skin job (`uyafmt::moby`, "The skin job"; level 1 0x3F6FF0, func_003D00F8) -------------------------------
//
// The translation's reads and writes in its order (the scratchpad's variables, VU0's data memory, the staging array, the
// output buffers), the VU program's arithmetic on each row with `uycpu::vu0`'s operations (the same results bit for bit: the
// game's thread has the VU's rounding mode, `uycpu::fpu::MXCSR`); its SPR_TO and SPR_FROM transfers made where it kicks them.
// A record with a second group goes on into the rigid layout (`rigid`) as the function does.

type V4 = [u32; 4];

/// 0x3F6FF0 for the record `rec` with the staging buffer `staging`, called from the record runner with return address `ra`:
/// the next record.
fn skin_job(c: &mut Cpu, _k: F, rec: u32, staging: u32, ra: u64) -> u32 {
    skinned(c, rec, staging, ra)
}


fn rd_q(c: &mut Cpu, a: u32) -> V4 {
    let (lo, hi) = c.rd128(a);
    [lo as u32, (lo >> 32) as u32, hi as u32, (hi >> 32) as u32]
}

/// VU0's data memory, quadword `q` (wrapping at 256).
#[inline(always)]
fn vu0_get(c: &Cpu, q: u32) -> V4 {
    let a = (q as usize & 0xff) * 16;
    unsafe { (c.vu0d.add(a) as *const V4).read_unaligned() }
}

#[inline(always)]
fn vu0_put(c: &mut Cpu, q: u32, v: V4) {
    let a = (q as usize & 0xff) * 16;
    unsafe { (c.vu0d.add(a) as *mut V4).write_unaligned(v) }
}

/// A 4 x 4 matrix of VU0 data memory from quadword `q`.
#[inline(always)]
fn vu0_mat(c: &Cpu, q: u32) -> [V4; 4] {
    std::array::from_fn(|k| vu0_get(c, q + k as u32))
}

/// Joint matrix `j` (the scratchpad's 0x40 * j).
fn joint(c: &mut Cpu, j: u32) -> [V4; 4] {
    std::array::from_fn(|k| rd_q(c, SPR + 64 * (j & 0xff) + 16 * k as u32))
}

/// The 8 bytes of a packed position at a 2-byte aligned address (`sdr`, `sdl`).
fn wr_pos(c: &mut Cpu, a: u32, p: u64) {
    for k in 0..4 {
        c.wr16(a + 2 * k, (p >> (16 * k)) as u16);
    }
}

fn bcx(x: u32) -> V4 {
    [x; 4]
}

/// rows[0] * w0 + rows[1] * w1 (+ rows[2] * w2), row by row: VMULA then VMADDA (and VMADD).
fn blend(m: &[[V4; 4]], w: &[u32]) -> [V4; 4] {
    std::array::from_fn(|r| {
        let mut acc = m_fmac::<6, 15>(m[0][r], bcx(w[0]), [0; 4]);
        for k in 1..m.len() {
            acc = m_fmac::<2, 15>(m[k][r], bcx(w[k]), acc);
        }
        acc
    })
}

/// v (x, y, z) through the matrix's rows 0 to 2, plus row 3 times `t` when given: VMULAx, VMADDAy, VMADDAz (, VMADDw).
fn transform(m: &[V4], v: [u32; 3], t: Option<u32>) -> V4 {
    let acc = m_fmac::<6, 15>(m[0], bcx(v[0]), [0; 4]);
    let acc = m_fmac::<2, 15>(m[1], bcx(v[1]), acc);
    let acc = m_fmac::<2, 15>(m[2], bcx(v[2]), acc);
    match t {
        Some(t) => m_fmac::<2, 15>(m[3], bcx(t), acc),
        None => acc,
    }
}

fn f(x: f32) -> u32 {
    x.to_bits()
}

/// A vertex's normal for the renderer, as the VU programs' hooks give it (`hw::skin_normal`: while it takes the joints).
fn normal(c: &mut Cpu, n: V4) {
    if crate::SKIN_JOINTS.load(std::sync::atomic::Ordering::Relaxed) {
        c.mach().hw.skin_normals.push([f32::from_bits(n[0]), f32::from_bits(n[1]), f32::from_bits(n[2])]);
    }
}

/// The (cos, sin) pair `i` of the scratchpad's table at 0x3800.
fn trig(c: &mut Cpu, i: u8) -> (u32, u32) {
    let a = SPR + 0x3800 + 8 * i as u32;
    (c.rd32(a), c.rd32(a + 4))
}

/// A row's normal from its two angle indices: (cos A cos B, sin A cos B, sin B).
fn angles(c: &mut Cpu, la: u8, lb: u8) -> [u32; 3] {
    let ((ca, sa), (cb, sb)) = (trig(c, la), trig(c, lb));
    let p = m_fmac::<6, 15>([ca, sa, 0, 0], [cb, cb, 0, 0], [0; 4]);
    [p[0], p[1], sb]
}

/// The low halves of four integers (a position's FTOI0: x, y, z, w) as the 8 bytes `ppach` makes.
fn pack(i: V4) -> u64 {
    (i[0] & 0xffff) as u64 | ((i[1] & 0xffff) as u64) << 16 | ((i[2] & 0xffff) as u64) << 32 | ((i[3] & 0xffff) as u64) << 48
}

/// A lit colour's four bytes.
fn colour_of(lit: V4) -> u32 {
    (0..4).fold(0u32, |v, k| v | colour_byte(lit[k]) << (8 * k))
}

/// The skin job's colour byte of a lit colour lane: the low 16 bits of the float as an s16, times 0x80 saturated, shifted
/// right by 7, low byte (`ppach`, `pmulth`, `pmfhl.sh`, `psrah`, `ppacb`).
fn colour_byte(bits: u32) -> u32 {
    let v = (bits & 0xffff) as u16 as i16 as i32;
    ((v * 128).clamp(-32768, 32767) >> 7) as u8 as u32
}

/// The skinned layout (func_003D00F8 to 0x3F8044): the record `rec`, the staging buffer, ra as the job's caller passes it (the
/// job keeps it at scratchpad 0x1FF0 with ra's high half); returns the next record (v0).
fn skinned(c: &mut Cpu, rec: u32, staging: u32, ra: u64) -> u32 {
    c.wr128(SPR + 0x1ff0, ra, c.rh[31]);
    let f14 = c.rd32(rec + 0x14);
    let n = c.rd16(rec) as u32;
    let src = rec + 64;
    c.wr32(SPR + 0x1fd8, f14);
    c.wr32(SPR + 0x1fc8, SPR + 0x2000);
    c.wr32(SPR + 0x1fcc, SPR + 0x3000);
    c.wr32(SPR + 0x1fc0, SPR + 0x1c80);
    c.wr64(SPR + 0x1c78 + 8 * n, 0);
    spr_to(c, 256, src, (n >> 1) + 9, SPR + 0x1c00);
    let group = (((n >> 1) + 9) << 4) + src;
    c.wr32(SPR + 0x1fc4, staging);
    c.wr32(SPR + 0x1fd4, group);
    // The first mesh's packet B to 0x2000.
    let e = c.rd32(SPR + 0x1c80);
    spr_to(c, 256, (e >> 8) << 4, e & 0xff, 0x2000);
    // The light section into VF and VU0's data memory 251 to 255 (VSQI): the wrap factors' minimum with 0 (w times 0), the
    // three direction rows, and v0 (-1, its high half as it was).
    let lights = SPR + 0x1c00;
    let mut vf1 = rd_q(c, lights + 0x60);
    let low = m_fmac::<5, 14>(vf1, [0; 4], [0; 4]);
    m_put::<14>(&mut vf1, low);
    let w0 = m_fmac::<6, 1>(vf1, [0; 4], [0; 4]);
    m_put::<1>(&mut vf1, w0);
    let dirs = [rd_q(c, lights), rd_q(c, lights + 0x10), rd_q(c, lights + 0x20)];
    let cols = [rd_q(c, lights + 0x30), rd_q(c, lights + 0x40), rd_q(c, lights + 0x50)];
    let base = rd_q(c, lights + 0x70);
    let minus = [u32::MAX, u32::MAX, c.rh[2] as u32, (c.rh[2] >> 32) as u32];
    for (k, v) in [vf1, dirs[0], dirs[1], dirs[2], minus].into_iter().enumerate() {
        vu0_put(c, 251 + k as u32, v);
    }
    // Each mesh's setup (0x3F71AC to 0x3F71BC: ORI 0x80, PCPYH, PCPYLD) leaves v0 0x80 in every halfword, both halves: the next
    // job's quadword 255 has that high half.
    c.rh[2] = 0x0080_0080_0080_0080;
    let shared = Lights::new(&dirs, &cols, base, vf1);
    let mut entry_at = SPR + 0x1c80;
    loop {
        // The next mesh's packet B to the other buffer while this one is skinned.
        let next = c.rd32(entry_at + 8);
        let buf = c.rd32(SPR + 0x1fc8);
        let other = buf ^ 0x800;
        if next != 0 {
            spr_to(c, 256, (next >> 8) << 4, next & 0xff, other);
        }
        c.wr32(SPR + 0x1fc8, other);
        let out = c.rd32(SPR + 0x1fcc);
        mesh(c, buf, out, staging, &shared);
        let dest = c.rd32(entry_at + 4);
        let qwc = c.rd32(SPR + 0x1fd0);
        spr_from(c, 256, dest, qwc, out);
        entry_at += 8;
        c.wr32(SPR + 0x1fc0, entry_at);
        c.wr32(SPR + 0x1fcc, out ^ 0x400);
        if c.rd32(entry_at) == 0 {
            break;
        }
    }
    let f14 = c.rd32(SPR + 0x1fd8);
    let group = c.rd32(SPR + 0x1fd4);
    if f14 != 0 { rigid(c, f14, group, &dirs, &cols) } else { group }
}

/// The rigid layout (the function's second entry, +0x1058, which the skinned layout goes on into when the record's +0x14 word
/// is not zero): its section (the word's high half in quadwords, from the group) into the scratchpad at 0x1C00, then each entry's
/// packet: each vertex through one to three joint matrices, lit by the light section's directions (`dirs`, VF25 to VF27) and a
/// quarter of its colours (`cols`, VF28 to VF30) over a grey of 0x70, its normal through the section's first three rows; the
/// outputs DMA'd from 0x3000. Returns the next group (v0).
fn rigid(c: &mut Cpu, f14: u32, group: u32, dirs: &[V4; 3], cols: &[V4; 3]) -> u32 {
    let qwc = f14 >> 16;
    let alpha = f14 & 0xff;
    let mark = SPR + 0x1c00 + 16 * qwc - 8;
    c.wr64(mark, mark as u64);
    spr_to(c, 256, group, qwc, SPR + 0x1c00);
    let next = group.wrapping_add(16 * qwc);
    // The colours at a quarter (VF28 to VF30's x, y and z), the grey (VF31: 0x47800070 in x, y and z, 0x47800080 in w).
    let quarter = f(0.25);
    let cols: [V4; 3] = std::array::from_fn(|k| {
        let mut v = cols[k];
        m_put::<14>(&mut v, m_fmac::<6, 14>(cols[k], bcx(quarter), [0; 4]));
        v
    });
    let mut grey = [0, 0, 0, 0x4780_0080];
    m_put::<14>(&mut grey, m_fmac::<0, 14>([0, 0, 0, f(1.0)], bcx(0x4780_0070), [0; 4]));
    let mut entry = SPR + 0x1c40;
    let first = c.rd32(entry);
    let mut buf = SPR + 0x2000;
    let mut end = buf + 16 * (first & 0xff) - 16;
    c.wr64(end, 0);
    spr_to(c, 256, (first >> 8) << 4, first & 0xff, buf);
    let rows = [rd_q(c, SPR + 0x1c00), rd_q(c, SPR + 0x1c10), rd_q(c, SPR + 0x1c20)];
    let one = f(1.0);
    while end != 0 {
        let other = buf ^ 0x800;
        let following = c.rd32(entry + 8);
        let dest = c.rd32(entry + 4);
        end = 0;
        if following != 0 {
            end = other + 16 * (following & 0xff) - 16;
            c.wr64(end, 0);
            spr_to(c, 256, (following >> 8) << 4, following & 0xff, other);
        }
        let out = SPR + 0x3000;
        let n = c.rd32(buf);
        let (mut pos, mut col) = (out.wrapping_add(c.rd32(buf + 4)), out.wrapping_add(c.rd32(buf + 8)));
        let size = c.rd32(buf + 0xc);
        let verts = buf + 16;
        count(7, n);
        c.wr128(pos.wrapping_sub(16), 0, 0);
        c.wr128(col.wrapping_sub(16), 0, 0);
        c.wr128(out.wrapping_add(size).wrapping_sub(16), 0, 0);
        c.wr128(verts + 16 * n, 0, 0);
        entry += 8;
        let mut nrm_at = out;
        for k in 0..n {
            let v = verts + 16 * k;
            let b: [u8; 16] = {
                let (lo, hi) = c.rd128(v);
                let mut b = [0u8; 16];
                b[..8].copy_from_slice(&lo.to_le_bytes());
                b[8..].copy_from_slice(&hi.to_le_bytes());
                b
            };
            let joints = b[11] as i32 - 2;
            let mats = [b[8], b[9], b[10]].map(|j| joint(c, j as u32));
            let m = match joints {
                j if j < 0 => mats[0],
                0 => blend(&mats[..2], &[f(b[12] as f32 / 256.0), f(b[13] as f32 / 256.0)]),
                _ => blend(&mats, &[f(b[12] as f32 / 256.0), f(b[13] as f32 / 256.0), f(b[14] as f32 / 256.0)]),
            };
            let ((ca, sa), (cb, sb)) = (trig(c, b[6]), trig(c, b[7]));
            let xy = m_fmac::<6, 15>([ca, sa, 0, 0], [cb, cb, 0, 0], [0; 4]);
            let z = m_fmac::<0, 15>([0; 4], bcx(sb), [0; 4]);
            let nrm = transform(&m, [xy[0], xy[1], z[2]], None);
            normal(c, nrm);
            let p = [i16::from_le_bytes([b[0], b[1]]), i16::from_le_bytes([b[2], b[3]]), i16::from_le_bytes([b[4], b[5]])];
            let ints = m_ftoi::<0>(transform(&m, p.map(|x| f(x as f32)), Some(one)));
            let d = transform(dirs, [nrm[0], nrm[1], nrm[2]], None);
            let s = m_fmac::<4, 15>(d, [0, 0, 0, one], [0; 4]);
            let acc = m_fmac::<6, 15>(grey, bcx(one), [0; 4]);
            let acc = m_fmac::<2, 15>(cols[0], bcx(s[0]), acc);
            let acc = m_fmac::<2, 15>(cols[1], bcx(s[1]), acc);
            let lit = m_fmac::<2, 15>(cols[2], bcx(s[2]), acc);
            // The normal through the section's rows, x and y plus 1 as FTOI12, halved.
            let mut on = transform(&rows, [nrm[0], nrm[1], nrm[2]], None);
            let plus = m_fmac::<0, 12>(on, bcx(one), [0; 4]);
            m_put::<12>(&mut on, plus);
            let fixed = m_ftoi::<2>(on);
            m_put::<12>(&mut on, fixed);
            for (q, &i) in ints[..3].iter().enumerate() {
                c.wr16(pos + 2 * q as u32, i as u16);
            }
            pos += 6;
            c.wr32(col, (lit[0] & 0xff) | (lit[1] & 0xff) << 8 | (lit[2] & 0xff) << 16 | (lit[3] & 0xff) << 24);
            c.wr8(col + 3, alpha as u8);
            c.wr32(nrm_at, ((on[0] as i32 >> 1) as u32 & 0xffff) | ((on[1] as i32 >> 1) as u32) << 16);
            nrm_at += 4;
            col += 4;
        }
        spr_from(c, 256, dest, size >> 4, 0x3000);
        buf = other;
    }
    next
}

/// A record's light section as its meshes' arithmetic reads it: `dirs`, `cols`, `base` and `wrap` are the registers as the VU
/// holds them (the scalar row path takes them so); the `s*` fields are the same values as `m_fmac` reads them (an operand with
/// exponent 255 as the largest number of its sign), each lane splat over the four rows of a group.
struct Lights {
    dirs: [V4; 3],
    cols: [V4; 3],
    base: V4,
    wrap: V4,
    sd: [[std::arch::x86_64::__m128; 3]; 3],
    sc: [[std::arch::x86_64::__m128; 4]; 3],
    sw: [std::arch::x86_64::__m128; 3],
    sb: [std::arch::x86_64::__m128; 4],
}

impl Lights {
    fn new(dirs: &[V4; 3], cols: &[V4; 3], base: V4, wrap: V4) -> Lights {
        let sp = |x: u32| unsafe { std::arch::x86_64::_mm_set1_ps(uycpu::fpu::fin(x)) };
        // The base colour as every row's `VMUL base, 1.0` reads it.
        let lit0 = m_fmac::<6, 15>(base, bcx(f(1.0)), [0; 4]);
        Lights {
            dirs: *dirs,
            cols: *cols,
            base,
            wrap,
            sd: std::array::from_fn(|k| std::array::from_fn(|l| sp(dirs[k][l]))),
            sc: std::array::from_fn(|k| std::array::from_fn(|l| sp(cols[k][l]))),
            sw: std::array::from_fn(|l| sp(wrap[l])),
            sb: std::array::from_fn(|l| sp(lit0[l])),
        }
    }
}

/// Whether some word of the matrix has exponent 255 (which `m_fmac` reads as the largest number of its sign).
#[inline(always)]
fn m_255(m: &[V4; 4]) -> bool {
    use std::arch::x86_64::*;
    unsafe {
        let e = _mm_set1_epi32(0x7f80_0000);
        let mut o = _mm_setzero_si128();
        for q in m {
            o = _mm_or_si128(o, _mm_cmpeq_epi32(_mm_and_si128(_mm_loadu_si128(q.as_ptr() as *const _), e), e));
        }
        _mm_movemask_epi8(o) != 0
    }
}

type Soa = [[std::arch::x86_64::__m128; 4]; 4];

/// One matrix for all four rows: every word splat.
#[inline(always)]
fn soa_one(m: &[V4; 4]) -> Soa {
    use std::arch::x86_64::*;
    unsafe { std::array::from_fn(|k| std::array::from_fn(|l| _mm_set1_ps(f32::from_bits(m[k][l])))) }
}

/// Each row's own matrix: [k][l] holds word l of matrix row k for the four rows.
#[inline(always)]
fn soa_four(ms: &[[V4; 4]; 4]) -> Soa {
    use std::arch::x86_64::*;
    unsafe {
        std::array::from_fn(|k| {
            let ld = |i: usize| _mm_loadu_ps(ms[i][k].as_ptr() as *const f32);
            let (mut a, mut b, mut c, mut d) = (ld(0), ld(1), ld(2), ld(3));
            _MM_TRANSPOSE4_PS(&mut a, &mut b, &mut c, &mut d);
            [a, b, c, d]
        })
    }
}

/// What four rows of a mesh come to: the normals (x, y, z), the lit colours, the positions as integers, the colour words and
/// the packed positions for the staging records, and the status flags VRSQRT sets (as `qop`).
struct Rows4 {
    nrm: [std::arch::x86_64::__m128; 3],
    lit: [std::arch::x86_64::__m128; 4],
    ints: [std::arch::x86_64::__m128i; 4],
    col: std::arch::x86_64::__m128i,
    pack: [std::arch::x86_64::__m128i; 2],
    flags: u32,
}

/// The arithmetic of `mesh`'s row loop on four rows at once (the lanes of each vector are the rows): the same operations in the
/// same order as the scalar path, so the same bits, given operands without exponent 255 (the caller sends those rows to the
/// scalar path). `m` is the rows' matrices, `p` the positions' coordinates, `t` the four trig words (cos A, sin A, cos B, sin B).
#[inline(always)]
fn rows4(lt: &Lights, m: &Soa, p: [std::arch::x86_64::__m128; 3], t: [std::arch::x86_64::__m128; 4]) -> Rows4 {
    use std::arch::x86_64::*;
    unsafe {
        let one = _mm_set1_ps(1.0);
        let mul = |a, b| _mm_mul_ps(a, b);
        let add = |a, b| _mm_add_ps(a, b);
        // The position: VMULAx, VMADDAy, VMADDAz, VMADDw (the matrix's row 3 times 1).
        let pos: [__m128; 4] = std::array::from_fn(|l| add(add(add(mul(m[0][l], p[0]), mul(m[1][l], p[1])), mul(m[2][l], p[2])), mul(m[3][l], one)));
        // The normal from the angles (cos A cos B, sin A cos B, sin B) through rows 0 to 2.
        let (a, b, z) = (mul(t[0], t[2]), mul(t[1], t[2]), t[3]);
        let nrm: [__m128; 3] = std::array::from_fn(|l| add(add(mul(m[0][l], a), mul(m[1][l], b)), mul(m[2][l], z)));
        // 1 / sqrt(x^2 + y^2 + z^2): VRSQRT's quotient, the largest number for a zero, flags D (0x820) and I (0x410).
        let len = add(add(mul(nrm[0], nrm[0]), mul(nrm[1], nrm[1])), mul(nrm[2], nrm[2]));
        let zero = _mm_setzero_ps();
        let is0 = _mm_cmpeq_ps(len, zero);
        let neg = _mm_cmplt_ps(len, zero);
        let root = _mm_sqrt_ps(_mm_and_ps(len, _mm_castsi128_ps(_mm_set1_epi32(0x7fff_ffff))));
        let quot = _mm_div_ps(one, root);
        let big = _mm_or_si128(
            _mm_and_si128(_mm_xor_si128(_mm_castps_si128(one), _mm_castps_si128(len)), _mm_set1_epi32(0x8000_0000u32 as i32)),
            _mm_set1_epi32(0x7f7f_ffff),
        );
        let q = _mm_castsi128_ps(_mm_or_si128(_mm_and_si128(_mm_castps_si128(is0), big), _mm_andnot_si128(_mm_castps_si128(is0), _mm_castps_si128(quot))));
        let flags = if _mm_movemask_ps(is0) != 0 { 0x820 } else { 0 } | if _mm_movemask_ps(neg) != 0 { 0x410 } else { 0 };
        // The lights: the directions through the normal, the larger of that and it times the wrap factors, the colours through
        // that, the base added to those times 1 / length.
        let d: [__m128; 3] = std::array::from_fn(|l| add(add(mul(lt.sd[0][l], nrm[0]), mul(lt.sd[1][l], nrm[1])), mul(lt.sd[2][l], nrm[2])));
        let s: [__m128; 3] = std::array::from_fn(|l| {
            let dw = mul(d[l], lt.sw[l]);
            let ge = _mm_castps_si128(_mm_cmpge_ps(d[l], dw));
            _mm_castsi128_ps(_mm_or_si128(_mm_and_si128(ge, _mm_castps_si128(d[l])), _mm_andnot_si128(ge, _mm_castps_si128(dw))))
        });
        let lit: [__m128; 4] = std::array::from_fn(|l| add(lt.sb[l], mul(add(add(mul(lt.sc[0][l], s[0]), mul(lt.sc[1][l], s[1])), mul(lt.sc[2][l], s[2])), q)));
        // FTOI0 with the saturation at 2^31 (CVTTPS2DQ gives 0x80000000 for it).
        let top = _mm_set1_ps(2147483648.0);
        let ints: [__m128i; 4] = std::array::from_fn(|l| _mm_xor_si128(_mm_cvttps_epi32(pos[l]), _mm_castps_si128(_mm_cmpge_ps(pos[l], top))));
        // The colour bytes: the low 16 bits of each lane as an s16, limited to -256..255, low byte.
        let byte = |v: __m128| {
            let x = _mm_srai_epi32(_mm_slli_epi32(_mm_castps_si128(v), 16), 16);
            _mm_and_si128(_mm_min_epi16(_mm_max_epi16(x, _mm_set1_epi16(-256)), _mm_set1_epi16(255)), _mm_set1_epi32(0xff))
        };
        let col = _mm_or_si128(
            _mm_or_si128(byte(lit[0]), _mm_slli_epi32(byte(lit[1]), 8)),
            _mm_or_si128(_mm_slli_epi32(byte(lit[2]), 16), _mm_slli_epi32(byte(lit[3]), 24)),
        );
        // The positions' halfwords as the 8 bytes of each row's `pack`.
        let lo = _mm_set1_epi32(0xffff);
        let xy = _mm_or_si128(_mm_and_si128(ints[0], lo), _mm_slli_epi32(ints[1], 16));
        let zw = _mm_or_si128(_mm_and_si128(ints[2], lo), _mm_slli_epi32(ints[3], 16));
        Rows4 { nrm, lit, ints, col, pack: [_mm_unpacklo_epi32(xy, zw), _mm_unpackhi_epi32(xy, zw)], flags }
    }
}

/// One row on the scalar path (`m_fmac` and `m_q`: the operands' exponent 255 read as the largest number): the matrix `m`, the
/// row's 16 bytes `b`; returns the lit colour, the positions as integers and the normal.
fn row_scalar(c: &mut Cpu, lt: &Lights, m: &[V4; 4], b: &[u8; 16]) -> (V4, V4, V4) {
    let p = [i16::from_le_bytes([b[10], b[11]]), i16::from_le_bytes([b[12], b[13]]), i16::from_le_bytes([b[14], b[15]])];
    let pos = transform(m, p.map(|x| f(x as f32)), Some(f(1.0)));
    let nrm = transform(m, angles(c, b[8], b[9]), None);
    // The normal's length: 1 / sqrt(x^2 + y^2 + z^2) (VRSQRT).
    let sq = m_fmac::<6, 15>(nrm, nrm, [0; 4]);
    let len = m_fmac::<0, 15>(m_fmac::<0, 15>(bcx(sq[0]), bcx(sq[1]), [0; 4]), bcx(sq[2]), [0; 4]);
    let q = uycpu::vu0::m_q::<2>(c, f(1.0), len[0]);
    let d = transform(&lt.dirs, [nrm[0], nrm[1], nrm[2]], None);
    let s = m_fmac::<4, 15>(d, m_fmac::<6, 15>(d, lt.wrap, [0; 4]), [0; 4]);
    let a = transform(&lt.cols, [s[0], s[1], s[2]], None);
    let lit = m_fmac::<2, 15>(a, bcx(q), m_fmac::<6, 15>(lt.base, bcx(f(1.0)), [0; 4]));
    (lit, m_ftoi::<0>(pos), nrm)
}

/// One mesh of the skinned layout: packet B at `buf`, its output arrays at `out`.
///
/// The rows go through in groups of four, in the order the VU program takes them: each row's matrix through VU0's data memory as
/// before (a group 3 row that repeats the previous row's matrix words and joint leaves the memory as it is and keeps its
/// matrix), then the four rows' arithmetic at once (`rows4`), or, when some operand of the group has exponent 255, row by row
/// on the scalar path.
fn mesh(c: &mut Cpu, buf: u32, out: u32, staging: u32, lt: &Lights) {
    use std::arch::x86_64::*;
    let h = |c: &mut Cpu, o: u32| c.rd16(buf + o) as u32;
    let (n0, g1, g2, g3, copies, vc, roff, cn) = (h(c, 0), h(c, 2), h(c, 4), h(c, 6), h(c, 8), h(c, 0xa), h(c, 0xc), h(c, 0xe));
    let rows = g1 + g2 + g3;
    count(4, g1);
    count(5, g2);
    count(6, g3);
    let pad = |x: u32| (x + 15) & !15;
    let positions = out + pad(4 * vc);
    let end = positions + pad(6 * vc);
    c.wr128(positions - 16, 0, 0);
    c.wr128(end - 16, 0, 0);
    c.wr32(SPR + 0x1fd0, (end - out) >> 4);
    // The pairs' joint matrices into VU0's data memory.
    for k in 0..n0 {
        let (j, a) = (c.rd8(buf + 0x10 + 2 * k) as u32, c.rd8(buf + 0x11 + 2 * k) as u32);
        let m = joint(c, j);
        for (r, row) in m.into_iter().enumerate() {
            vu0_put(c, a + r as u32, row);
        }
    }
    let copies_at = buf + 0x10 + ((2 * n0 + 7) & !7);
    let row_at = (buf + roff) & !15;
    let row = |c: &mut Cpu, r: u32| -> [u8; 16] {
        let (lo, hi) = c.rd128(row_at + 16 * r);
        let mut b = [0u8; 16];
        b[..8].copy_from_slice(&lo.to_le_bytes());
        b[8..].copy_from_slice(&hi.to_le_bytes());
        b
    };
    let w = |b: u8| f(b as f32 / 256.0);
    let normals = crate::SKIN_JOINTS.load(Relaxed);
    // Each row's packed position and colour word (a mesh has under 128 rows: packet B is 0x800 bytes).
    let mut pk = [0u64; 136];
    let mut cl = [0u32; 136];
    // The VU program writes row r's results two hand-offs later, even rows into VF11/VF12 and odd ones into VF09/VF10, and
    // ends at hand-off rows + 2; the EE reads row r after hand-off r + 3, so a row past the end it reads finds the last real row
    // of the same parity: the last two rows' registers are the ones that stay.
    let to_vf = |c: &mut Cpu, r: u32, lit: V4, ints: V4| {
        if r + 2 >= rows {
            let pair = if r % 2 == 0 { 11 } else { 9 };
            (c.vf[pair], c.vf[pair + 1]) = (lit, ints);
        }
    };
    // The matrix of the last group 3 row, and the words that named it ((b2, b3, w0 >> 9)): the next row with the same ones leaves
    // VU0's data memory as it is (its put writes what the last row's put did, and its matrix is the last row's).
    let mut cur_key: Option<(u32, u32, u32)> = None;
    let (mut cur_m, mut cur_255, mut cur_id) = ([[0u32; 4]; 4], false, 0u32);
    let (mut ms, mut soa_id, mut soa) = ([[[0u32; 4]; 4]; 4], u32::MAX, unsafe { [[_mm_setzero_ps(); 4]; 4] });
    let mut start = 0u32;
    while start < rows {
        let n = (rows - start).min(4) as usize;
        let mut bs = [[0u8; 16]; 4];
        let mut ids = [0u32; 4];
        let mut any_255 = false;
        for i in 0..n {
            let r = start + i as u32;
            let b = row(c, r);
            bs[i] = b;
            let w0 = u16::from_le_bytes([b[0], b[1]]) as u32;
            let (b2, b3) = (b[2] as u32, b[3] as u32);
            // A row's own joint (VF05 to VF08, the joint its halfword names) goes into VU0's data memory before the matrices it
            // uses are loaded (groups 1 and 3), its blended matrix after them (groups 1 and 2).
            if r < g1 {
                let own = joint(c, w0 >> 9);
                for k in 0..4 {
                    vu0_put(c, b[6] as u32 + k, own[k as usize]);
                }
                let m = blend(&[vu0_mat(c, b2), vu0_mat(c, b3)], &[w(b[4]), w(b[5])]);
                for k in 0..4 {
                    vu0_put(c, b[7] as u32 + k, m[k as usize]);
                }
                (cur_key, cur_m, cur_255, cur_id) = (None, m, m_255(&m), cur_id + 1);
            } else if r < g1 + g2 {
                let m = blend(&[vu0_mat(c, b2), vu0_mat(c, b3), vu0_mat(c, 2 * (w0 >> 9))], &[w(b[4]), w(b[5]), w(b[6])]);
                for k in 0..4 {
                    vu0_put(c, b[7] as u32 + k, m[k as usize]);
                }
                (cur_key, cur_m, cur_255, cur_id) = (None, m, m_255(&m), cur_id + 1);
            } else if cur_key != Some((b2, b3, w0 >> 9)) {
                let own = joint(c, w0 >> 9);
                for k in 0..4 {
                    vu0_put(c, b3 + k, own[k as usize]);
                }
                let m = vu0_mat(c, b2);
                (cur_key, cur_m, cur_255, cur_id) = (Some((b2, b3, w0 >> 9)), m, m_255(&m), cur_id + 1);
            }
            ids[i] = cur_id;
            ms[i] = cur_m;
            any_255 |= cur_255;
        }
        // The trig words of the rows' angles (the missing rows of the last group repeat the last row's).
        let mut tg = [[0u32; 4]; 4];
        for i in 0..4 {
            let b = &bs[i.min(n - 1)];
            let ((ca, sa), (cb, sb)) = (trig(c, b[8]), trig(c, b[9]));
            tg[i] = [ca, sa, cb, sb];
        }
        let t = unsafe { std::array::from_fn::<__m128i, 4, _>(|l| _mm_setr_epi32(tg[0][l] as i32, tg[1][l] as i32, tg[2][l] as i32, tg[3][l] as i32)) };
        let e = unsafe { _mm_set1_epi32(0x7f80_0000) };
        let t_255 = unsafe {
            let o = |v: __m128i| _mm_cmpeq_epi32(_mm_and_si128(v, e), e);
            _mm_movemask_epi8(_mm_or_si128(_mm_or_si128(o(t[0]), o(t[1])), _mm_or_si128(o(t[2]), o(t[3])))) != 0
        };
        if any_255 || t_255 {
            for i in 0..n {
                let r = start + i as u32;
                let (lit, ints, nrm) = row_scalar(c, lt, &ms[i], &bs[i]);
                normal(c, nrm);
                to_vf(c, r, lit, ints);
                (pk[r as usize], cl[r as usize]) = (pack(ints), colour_of(lit));
            }
        } else {
            if ids[..n].iter().all(|&i| i == ids[0]) {
                if soa_id != ids[0] {
                    soa = soa_one(&cur_m);
                    soa_id = ids[0];
                }
            } else {
                for i in n..4 {
                    ms[i] = ms[n - 1];
                }
                soa = soa_four(&ms);
                soa_id = u32::MAX;
            }
            let p: [__m128; 3] = unsafe {
                std::array::from_fn(|l| {
                    let x = |i: usize| i16::from_le_bytes([bs[i.min(n - 1)][10 + 2 * l], bs[i.min(n - 1)][11 + 2 * l]]) as i32;
                    _mm_cvtepi32_ps(_mm_setr_epi32(x(0), x(1), x(2), x(3)))
                })
            };
            let res = rows4(lt, &soa, p, unsafe { t.map(|v| _mm_castsi128_ps(v)) });
            c.vi[16] |= res.flags;
            let r0 = start as usize;
            unsafe {
                _mm_storeu_si128(cl.as_mut_ptr().add(r0) as *mut __m128i, res.col);
                _mm_storeu_si128(pk.as_mut_ptr().add(r0) as *mut __m128i, res.pack[0]);
                _mm_storeu_si128(pk.as_mut_ptr().add(r0 + 2) as *mut __m128i, res.pack[1]);
            }
            if normals || start + n as u32 + 1 >= rows {
                let mut lanes = [[[0u32; 4]; 4]; 3];
                unsafe {
                    for l in 0..3 {
                        _mm_storeu_ps(lanes[0][l].as_mut_ptr() as *mut f32, res.nrm[l]);
                    }
                    for l in 0..4 {
                        _mm_storeu_ps(lanes[1][l].as_mut_ptr() as *mut f32, res.lit[l]);
                        _mm_storeu_si128(lanes[2][l].as_mut_ptr() as *mut __m128i, res.ints[l]);
                    }
                }
                for i in 0..n {
                    normal(c, [lanes[0][0][i], lanes[0][1][i], lanes[0][2][i], 0]);
                    to_vf(c, start + i as u32, std::array::from_fn(|l| lanes[1][l][i]), std::array::from_fn(|l| lanes[2][l][i]));
                }
            }
        }
        start += 4;
    }
    // The EE's last pass (fp, (rows - 3) % 4 rows after its loop of four): row r's outputs at index r and in the staging record
    // the row 7 after it names (the last rows': the halfwords 2 to 7 of the row its loop read last).
    let fp = (rows - 3) % 4;
    let named = |c: &mut Cpu, r: u32| -> u32 {
        let j = r + 7;
        let last = rows + 3 - fp;
        if j <= last {
            c.rd16(row_at + 16 * j) as u32 & 0x1ff
        } else {
            let lane = (j - last + 1) as u32;
            c.rd16(row_at + 16 * last + 2 * lane) as u32 & 0x1ff
        }
    };
    for r in 0..rows {
        let (p, col) = (pk[r as usize], cl[r as usize]);
        c.wr32(out + 4 * r, col);
        // The last row's position: x, y and z alone when it ends at a doubleword's end (fp 1), else with its w.
        if r + 1 == rows && fp == 1 {
            for k in 0..3 {
                c.wr16(positions + 6 * r + 2 * k, (p >> (16 * k)) as u16);
            }
        } else {
            c.wr64(positions + 6 * r, p);
        }
        let at = staging + 16 * named(c, r);
        c.wr64(at, p);
        c.wr32(at + 8, col);
    }
    // With fp 0 or 2 the EE stores its colours in pairs one row past the end: the colour of row rows - 2 (VF09's).
    if fp % 2 == 0 {
        let col = cl[rows as usize - 2];
        c.wr32(out + 4 * rows, col);
    }
    // The vertex copies from the staging records.
    for k in 0..copies {
        let slot = (c.rd16(copies_at + 2 * k) as u32 >> 7) & 0x1ff;
        let at = staging + 16 * slot;
        let (p, col) = (c.rd64(at), c.rd32(at + 8));
        wr_pos(c, positions + 6 * (rows + k), p);
        c.wr32(out + 4 * (rows + k), col);
    }
    let colours_end = out + 4 * vc;
    if colours_end & 7 != 0 {
        c.wr32(colours_end, 0);
    }
    // The last `cn` colours: the record's flat colour.
    let flat = c.rd32(SPR + 0x1c6c);
    for k in 1..=cn {
        c.wr32(colours_end - 4 * k, flat);
    }
}

// ---- 0x43EA78: the moby cull and draw list --------------------------------------------------------------------------------

// Register numbers, for the hand-written functions' register files.
const AT: usize = 1;
const V0: usize = 2;
const V1: usize = 3;
const A0: usize = 4;
const A1: usize = 5;
const A2: usize = 6;
const A3: usize = 7;
const T0: usize = 8;
const T1: usize = 9;
const T2: usize = 10;
const T3: usize = 11;
const T4: usize = 12;
const T5: usize = 13;
const T6: usize = 14;
const T7: usize = 15;
const S0: usize = 16;
const S1: usize = 17;
const S2: usize = 18;
const S3: usize = 19;
const S4: usize = 20;
const S5: usize = 21;
const S6: usize = 22;
const S7: usize = 23;
const T8: usize = 24;
const T9: usize = 25;
const SP: usize = 29;
const FP: usize = 30;
const RA: usize = 31;

/// A 128-bit register of a register file.
#[inline(always)]
fn rq(r: &[u64; 32], h: &[u64; 32], n: usize) -> Q {
    (r[n], h[n])
}
#[inline(always)]
fn setq(r: &mut [u64; 32], h: &mut [u64; 32], n: usize, v: Q) {
    r[n] = v.0;
    h[n] = v.1;
}
/// A 32-bit ADD / ADDU / ADDI as the R5900 keeps it.
#[inline(always)]
fn add32(a: u64, b: u64) -> u64 {
    sx((a as u32).wrapping_add(b as u32))
}

/// 0x43EA78 (a0 the moby array, a1 the job list's RAM pointer, a2 -1, a3 the mode: bit 0 a second pass over the near mobies,
/// bit 1 the drawn byte kept on culled mobies), called with return address `ra` and the caller's sp. Hand-written: it keeps
/// its registers at scratchpad 0x3FA8, works on 128-bit registers (the high halves of ra, sp, s4 to s7 carry its state), and
/// builds each drawn moby's draw record in the scratchpad (0x2800 and 0x2C00 in turn, SPR_FROM to the job list) and its VIF1
/// chain at gp+0x4838. Returns the job list's end (v0). The program's view is widened when `wide` (`VIEW_LOADS` "the moby cull"
/// +0x214, `DRAWN`); `room` is the end of the job list's room, which DRAW DISTANCE FAR keeps its extra mobies inside.
#[allow(clippy::too_many_arguments)]
fn f43ea78(c: &mut Cpu, k: F, a0: u32, a1: u32, a2: u32, a3: u32, ra: u64, sp: u32, wide: bool, room: u32) -> u32 {
    let gp = c.r[28] as u32;
    let g = |c: &mut Cpu, pc: u32| k.gp(c, gp, pc);
    // The relocated addresses.
    let list_q = k.a(c, 0x43_ead8, 0x43_eadc);
    let list_a = k.a(c, 0x43_eae0, 0x43_eae4);
    let tex_list = k.a(c, 0x43_eb1c, 0x43_eb20);
    let vis_table = k.a(c, 0x43_eb5c, 0x43_eb60);
    let lod_table = k.a(c, 0x43_eb8c, 0x43_eb90);
    let lights = k.a(c, 0x43_ebdc, 0x43_ebe0);
    let camera = k.a(c, 0x43_ec48, 0x43_ec4c);
    let view = k.a(c, 0x43_ec80, 0x43_ec84);
    let templates = k.a(c, 0x43_eca0, 0x43_eca4);
    let global_pkt = k.a(c, 0x43_ed3c, 0x43_ed40);
    let view2 = k.a(c, 0x43_ed64, 0x43_ed68);
    let special = k.a(c, 0x43_fa94, 0x43_fa98);
    let flags_word = k.a(c, 0x43_fb84, 0x43_fb88);
    let (g3880, g4838, g27d0, g483c, g4d30, g3820) = (g(c, 0x43_eb18), g(c, 0x43_ec08), g(c, 0x43_ec10), g(c, 0x43_ec18), g(c, 0x43_ec28), g(c, 0x43_ec30));
    let gtpl: [u32; 7] = std::array::from_fn(|q| if q < 6 { k.gp(c, gp, 0x43_ecd8 + 4 * q as u32) } else { k.gp(c, gp, 0x43_ed08) });
    let (g3830, g3880b, g4838b, g3834) = (g(c, 0x43_fad0), g(c, 0x43_fadc), g(c, 0x43_faf8), g(c, 0x43_fc78));
    // DRAW DISTANCE FAR's reach for the view struct (`drawdist`), in the cull's units (view depth x 1024) as f32 bits, and in
    // whole world units for the near list's draw distances; whether the moby being culled passed the game's own far test.
    let far = crate::drawdist::reach_of(c, view).map(f32::to_bits);
    let far_units = far.map(|d| (f32::from_bits(d) / 1024.0).ceil() as i64);
    // The port's props (`range`).
    let ranges = crate::range::get();
    let props = ranges.props.as_ref();
    let mut game_far = true;
    // DRAW DISTANCE FAR's extra mobies (the module doc): those the game's own far test culls and FAR's reach keeps, left out of
    // the game's own passes and drawn after them (`extras`, their addresses); `extra_pass`, the next one while their pass runs;
    // the near list as the game's passes left it and its end (the pass draws a near-flag extra from slot 0); `ra_first`, the
    // first pass's RA (the pass draws as the first pass does). With UYA_FAR_LOG, what the cull did with them (`FarLog`).
    let mut extras: Vec<u32> = Vec::new();
    let mut extra_pass: Option<usize> = None;
    let mut near_saved: Vec<(u64, u64)> = Vec::new();
    let mut near_end_saved = (0u64, 0u64);
    let (mut cursor_saved, mut ra_saved, mut ra_first) = (0u32, 0u64, 0u64);
    let mut far_log = FarLog::default();
    let chain_start = c.rd32(g4838);

    let mut r = c.r;
    let mut h = c.rh;
    r[A0] = sx(a0);
    r[A1] = sx(a1);
    r[A2] = sx(a2);
    r[A3] = sx(a3);
    r[RA] = ra;
    r[SP] = sx(sp);
    // The caller's registers into the scratchpad (0x3FA8 to 0x3FF8), as the function saves them.
    for (o, n) in [(0x3ff8u32, RA), (0x3ff0, FP), (0x3fe8, SP), (0x3fe0, S7), (0x3fd8, S6), (0x3fd0, S5), (0x3fc8, S4), (0x3fc0, S3), (0x3fb8, S2), (0x3fb0, S1), (0x3fa8, S0)] {
        c.wr64(SPR + o, r[n]);
    }
    // The function's FPU registers f10 to f12 (the fog, the moby's +0x40 word, the "nothing drawn" flag).
    let (mut f10, mut f11) = (0u32, 0u32);
    r[FP] = add32(r[A0], 0xffff_ff00);
    r[S5] = sx((r[A2] as u32) << 8);
    r[T9] = add32(r[A1], 0);
    r[S5] = add32(r[S5], r[A0]);
    r[RA] = 0;
    r[AT] = 0x2c00;
    r[V0] = r[A3] & 1;
    r[V1] = r[A3] & 2;
    if r[V0] != 0 {
        r[RA] = r[AT];
    }
    { let v = uycpu::mmi::pcpyld(rq(&r, &h, V1), rq(&r, &h, RA)); setq(&mut r, &mut h, RA, v); }
    r[V1] = sx(c.rd32(list_q));
    r[V0] = sx(c.rd32(list_a));
    spr_to(c, 0x100, r[V0] as u32, (r[V1] as u32).wrapping_add(0x80), 0);
    r[S6] = SPR as u64;
    r[AT] = sx(c.rd32(g3880));
    r[V0] = sx(tex_list.wrapping_add(0x100));
    c.wr32(SPR + 0x3e00, r[AT] as u32);
    c.wr32(SPR + 0x3e04, r[V0] as u32);
    for a in (SPR + 0x2600..SPR + 0x2800).step_by(16) {
        c.wr128(a, 0, 0);
    }
    for q in 0..8 {
        let v = c.rd128(vis_table + 16 * q);
        c.wr128(SPR + 0x3800 + 16 * q, v.0, v.1);
        setq(&mut r, &mut h, if q % 2 == 0 { A0 } else { A1 }, v);
    }
    for q in 0..4 {
        let v = c.rd128(lod_table + 16 * q);
        c.wr128(SPR + 0x3300 + 16 * q, v.0, v.1);
        setq(&mut r, &mut h, if q % 2 == 0 { A0 } else { A1 }, v);
    }
    spr_to(c, 0x100, lights, 0x50, 0x2000);
    r[SP] = sx(c.rd32(g4838));
    vop::<0, 8>(c, 11, 0, bc(c, 0, 3));
    { let v = c.rd128(g27d0); setq(&mut r, &mut h, AT, v); }
    vop::<1, 4>(c, 11, 0, bc(c, 0, 3));
    r[V0] = sx(c.rd32(g483c));
    { let v = uycpu::mmi::pcpyld(rq(&r, &h, V0), rq(&r, &h, SP)); setq(&mut r, &mut h, SP, v); }
    stq(c, SPR + 0x3f60, rq(&r, &h, AT));
    r[AT] = sx(c.rd32(g4d30) << 10);
    r[S7] = sx(c.rd32(g3820));
    { let v = uycpu::mmi::pcpyld(rq(&r, &h, AT), rq(&r, &h, S7)); setq(&mut r, &mut h, S7, v); }
    r[AT] = 0x4480_0000;
    r[V0] = (SPR + 0x1000) as u64;
    qmtc2(c, 1, rq(&r, &h, AT));
    { let v = uycpu::mmi::pcpyld(rq(&r, &h, V0), rq(&r, &h, S5)); setq(&mut r, &mut h, S5, v); }
    lqc2(c, 24, camera + 0x140);
    for (n, o) in [(25usize, 0u32), (26, 0x10), (27, 0x20), (28, 0x40), (29, 0x50), (30, 0x60), (31, 0x70)] {
        lqc2(c, n, camera + o);
    }
    vop::<6, 14>(c, 24, 24, bc(c, 1, 0));
    r[T8] = (SPR + 0x2c00) as u64;
    vop::<6, 1>(c, 24, 0, bc(c, 1, 0));
    lqc2(c, 19, view + 0xa0);
    {
        let (a, b) = c.rd128(view + 0xb0);
        let (hh, vv) = if wide { (c.widen(a as u32), c.fov((a >> 32) as u32)) } else { (a as u32, (a >> 32) as u32) };
        c.vf[20] = [hh, vv, b as u32, (b >> 32) as u32];
    }
    lqc2(c, 21, view + 0x1e0);
    lqc2(c, 22, view + 0x1f0);
    lqc2(c, 23, view + 0x200);
    for (q, n) in [V1, A0, A1, A2, A3, T0].into_iter().enumerate() {
        let v = c.rd128(templates + 16 * q as u32);
        { let v = v; setq(&mut r, &mut h, n, v); }
    }
    for (q, n) in [V1, A0, A1, A2, A3, T0].into_iter().enumerate() {
        stq(c, SPR + 0x3f00 + 16 * q as u32, rq(&r, &h, n));
    }
    for (q, n) in [AT, V0, V1, A0, A1, A2].into_iter().enumerate() {
        let v = c.rd128(gtpl[q]);
        { let v = v; setq(&mut r, &mut h, n, v); }
    }
    for (q, n) in [AT, V0, V1, A0, A1, A2].into_iter().enumerate() {
        stq(c, SPR + 0x3e80 + 16 * q as u32, rq(&r, &h, n));
    }
    { let v = c.rd128(gtpl[6]); setq(&mut r, &mut h, AT, v); }
    r[A1] = 1;
    stq(c, SPR + 0x3ee0, rq(&r, &h, AT));
    let mut f12 = r[A1] as u32;
    // The chain's head: a NEXT tag, a CALL of the global packet, program 0's constants (twice, 0x123 and the other buffer).
    let mut spv = r[SP] as u32;
    c.wr128(spv, 0, 0);
    c.wr32(spv, 0x1000_0000);
    c.wr16(spv + 0xe, 0x1400);
    c.wr16(spv + 0xc, 0);
    spv += 16;
    c.wr128(spv, 0, 0);
    c.wr32(spv + 4, global_pkt);
    c.wr32(spv, 0x3000_0004);
    spv += 16;
    { let v = c.rd128(SPR + 0x3f60); setq(&mut r, &mut h, V0, v); }
    stq(c, spv, rq(&r, &h, V0));
    stq(c, spv + 0x70, rq(&r, &h, V0));
    r[V0] = 6;
    r[V1] = 0x123;
    c.wr8(spv, 6);
    c.wr8(spv + 0xe, 6);
    c.wr8(spv + 0x70, 6);
    c.wr8(spv + 0x7e, 6);
    c.wr16(spv + 0xc, 0x123);
    r[V1] = 0x287;
    c.wr16(spv + 0x7c, 0x287);
    spv += 0xe0;
    let pk: [Q; 6] = std::array::from_fn(|q| c.rd128(view2 + 16 * q as u32));
    for (q, n) in [A0, A1, A2, A3, T0, T1].into_iter().enumerate() {
        { let v = pk[q]; setq(&mut r, &mut h, n, v); }
    }
    for q in 0..6u32 {
        stq(c, spv.wrapping_sub(0xd0) + 16 * q, pk[q as usize]);
    }
    for q in 0..6u32 {
        stq(c, spv.wrapping_sub(0x60) + 16 * q, pk[q as usize]);
    }
    r[SP] = sx(spv);
    // The point lights (scratchpad 0x2400, 8 of 0x20 bytes): those with a radius kept, packed, radius times 1024 and 1/1024 / r.
    qmtc2(c, 3, (0x4480_0000, h[AT]));
    qmtc2(c, 4, (0x3a80_0000, h[AT]));
    r[AT] = 0x4480_0000;
    r[AT] = 0x3a80_0000;
    r[AT] = (SPR + 0x2400) as u64;
    r[V0] = (SPR + 0x2500) as u64;
    r[V1] = (SPR + 0x2400) as u64;
    r[A0] = 0;
    loop {
        let done = r[AT] == r[V0];
        r[A1] = c.rd64(r[AT] as u32 + 0x10);
        if done {
            break;
        }
        let skip = r[A1] == 0;
        r[AT] = add32(r[AT], 0x20);
        if skip {
            continue;
        }
        lqc2(c, 1, r[AT] as u32 - 0x10);
        lqc2(c, 2, r[AT] as u32 - 0x20);
        qop::<0>(c, c.vf[4][0], c.vf[1][3]);
        vop::<6, 15>(c, 1, 1, bc(c, 3, 0));
        sqc2(c, 1, r[V1] as u32 + 0x10);
        vop::<6, 1>(c, 2, 0, qq(c));
        sqc2(c, 2, r[V1] as u32);
        r[A0] = add32(r[A0], 1);
        r[V1] = add32(r[V1], 0x20);
    }
    { let v = uycpu::mmi::pcpyld(rq(&r, &h, A0), rq(&r, &h, S6)); setq(&mut r, &mut h, S6, v); }
    let (mut lo, mut vp) = (0u64, 0u64);
    let _ = &mut vp;
    let mut l: u32 = 0x43_ee70;
    loop {
        match l {
            0x43_ee70 => {
                count(0, 1);
                if let Some(next) = extra_pass {
                    // DRAW DISTANCE FAR's extras' pass (`extras`): a near-flag extra the last one put into the near list's slot 0
                    // is drawn as the second pass draws an entry (0x43FD30); then the next extra, culled and drawn as the first
                    // pass does a moby; after the last, the pass ends (0x43FA88).
                    let end = h[S5] as u32;
                    if end != SPR + 0x1000 {
                        if c.rd32(SPR + 0x3ef0) != end {
                            l = 0x43_fd30;
                            continue;
                        }
                        h[S5] = sx(SPR + 0x1000);
                        c.wr32(SPR + 0x3ef0, SPR + 0x1000);
                    }
                    let Some(&fp) = extras.get(next) else {
                        l = 0x43_fa88;
                        continue;
                    };
                    extra_pass = Some(next + 1);
                    r[V0] = c.rd16(fp + 0x32) as i16 as i64 as u64;
                    r[FP] = sx(fp);
                    lqc2(c, 1, fp);
                } else {
                    r[V0] = c.rd16(r[FP] as u32 + 0x132) as i16 as i64 as u64;
                    r[FP] = add32(r[FP], 0x100);
                    let t = r[FP] == r[S5];
                    lqc2(c, 1, r[FP] as u32);
                    if t {
                        l = 0x43_fa88;
                        continue;
                    }
                }
                r[AT] = c.rd8(r[FP] as u32 + 0x20) as i8 as i64 as u64;
                { let v = uycpu::mmi::pminw(rq(&r, &h, V0), rq(&r, &h, S7)); setq(&mut r, &mut h, V0, v); }
                r[V0] <<= 42;
                vop::<1, 14>(c, 2, 1, c.vf[24]);
                let t = (r[AT] as i64) < 0;
                qmtc2(c, 4, rq(&r, &h, V0));
                if t {
                    l = 0x43_fa78;
                    continue;
                }
                vop::<6, 12>(c, 3, 11, bc(c, 1, 3));
                itof::<0, 4>(c, 19, 4);
                r[A2] = c.rd8(r[FP] as u32 + 0x37) as u64;
                vop::<6, 14>(c, ACC, 25, bc(c, 2, 0));
                r[A3] = c.rd8(r[FP] as u32 + 0x36) as u64;
                vop::<2, 14>(c, ACC, 26, bc(c, 2, 1));
                r[A2] = add32(r[A2], r[S6]);
                vop::<2, 14>(c, 2, 27, bc(c, 2, 2));
                r[A2] = c.rd8(r[A2] as u32 + 0x3800) as u64;
                vop::<1, 4>(c, 19, 19, bc(c, 1, 3));
                // DRAW DISTANCE FAR (the module doc): the draw distance less the radius at least the reach, unless the distance
                // is not above 0 (vf4.y, the PMINW's word times 1024) or the moby has no class (+0x24); in the extras' pass
                // (`extras`) only while the job list has room left for its record in both passes (1 KB each at most, the
                // scratchpad buffer it is built in). In the game's own passes the reach is the limit of the mobies the game
                // draws itself (their fade is FAR's, not their own limit's); one past its own limit waits for the extras' pass.
                let game_limit = c.vf[19][1];
                if let Some(d) = far {
                    let drawable = c.rd32(r[FP] as u32 + 0x24) != 0 && (extra_pass.is_none() || r[T9] as u32 + 0x800 <= room);
                    if (c.vf[4][1] as i32) > 0 && uycpu::fpu::lt(game_limit, d) && drawable {
                        c.vf[19][1] = d;
                    }
                }
                r[T0] = c.rd8(r[FP] as u32 + 0x31) as u64;
                vop::<6, 12>(c, 8, 22, bc(c, 1, 3));
                r[A3] &= r[A2];
                let t = r[A3] == 0;
                r[T6] = c.rd16(r[FP] as u32 + 0x34) as u64;
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                vop::<0, 12>(c, 1, 3, bc(c, 2, 2));
                r[A3] = c.rd8(r[FP] as u32 + 0x22) as u64;
                vop::<6, 12>(c, 5, 20, bc(c, 2, 2));
                r[V0] = r[T6] & 0x81;
                let t = r[V0] != 0;
                vmove::<12>(c, 2, uycpu::vu0::m_abs(c.vf[2]));
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                vop::<6, 12>(c, 7, 8, bc(c, 22, 2));
                r[T1] = sx((r[A3] as u32) << 1);
                vop::<1, 12>(c, 3, 19, c.vf[1]);
                // The game's own far test, vf3.y's sign from its own limit: the drawn byte follows it (`drawn`).
                if far.is_some() {
                    game_far = uycpu::fpu::sub(game_limit, c.vf[1][1]) >> 31 == 0;
                }
                r[T2] = c.rd8(r[FP] as u32 + 0x23) as u64;
                vop::<6, 12>(c, 9, 5, c.vf[21]);
                r[A3] = sx((r[A3] as u32) << 2);
                vop::<1, 12>(c, 6, 2, c.vf[8]);
                r[T2] = ((r[T2] as i64) < 8) as u64;
                vop::<0, 12>(c, 8, 2, c.vf[7]);
                r[A1] = 0x4000_0000;
                vop::<1, 12>(c, 4, 1, c.vf[23]);
                r[A1] = r[A1].wrapping_add(r[A1]);
                let t = r[T2] != 0;
                vop::<4, 2>(c, 1, 0, c.vf[2]);
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                { let v = qmfc2(c, 3); setq(&mut r, &mut h, AT, v); }
                r[A2] = r[T6] & r[RA];
                vop::<1, 12>(c, 7, 5, c.vf[6]);
                r[S3] = 10;
                vop::<1, 12>(c, 8, 9, c.vf[8]);
                r[V0] = r[AT] << 32;
                let t = (r[AT] as i64) < 0;
                { let v = qmfc2(c, 4); setq(&mut r, &mut h, A0, v); }
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                // DRAW DISTANCE FAR: a moby inside the reach but past its own limit is culled here as the game culls it, and
                // drawn after the game's own passes, in whatever room they leave (`extras`).
                if far.is_some() && extra_pass.is_none() && !game_far {
                    extras.push(r[FP] as u32);
                    l = 0x43_fa60;
                    continue;
                }
                let t = (r[V0] as i64) >= 0;
                vmove::<12>(c, 1, mr32(c.vf[1]));
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                let t = r[A2] != 0;
                { let v = qmfc2(c, 7); setq(&mut r, &mut h, V0, v); }
                if t {
                    l = 0x43_fd80;
                    continue;
                }
                { let v = qmfc2(c, 8); setq(&mut r, &mut h, V1, v); }
                r[A0] ^= r[A1];
                let t = (r[V0] as i64) < 0;
                r[V0] <<= 32;
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                let t = (r[V0] as i64) < 0;
                uycpu::vu0::ftoi::<0, 12>(c, 1, 1);
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                r[V1] |= r[A0];
                uycpu::vu0::ftoi::<0, 4>(c, 3, 3);
                let t = (r[V1] as i64) < 0;
                r[V1] <<= 32;
                if !t && (r[V1] as i64) >= 0 {
                    r[S3] = 14;
                }
                l = 0x43_ef90;
            }
            0x43_ef90 => {
                // The drawn byte: set when it was clear; with the view widened or FAR, only when the game's own view and
                // distance would draw it.
                drawn(c, r[FP] as u32 + 0x31, r[T0], h[RA], wide, game_far);
                r[AT] = 1;
                l = 0x43_efa0;
            }
            0x43_efa0 => {
                count(1, 1);
                lqc2(c, 18, r[FP] as u32 + 0x10);
                r[A3] = add32(r[A3], r[S6]);
                r[AT] = sx(c.rd32(r[A3] as u32 + 0x3a00));
                r[V0] = 0x1400_0000;
                r[A0] = sx(c.rd32(r[FP] as u32 + 0x2c));
                r[S3] |= r[V0];
                r[S2] = 2;
                { let v = qmfc2(c, 1); setq(&mut r, &mut h, V0, v); }
                vop::<6, 14>(c, 18, 18, bc(c, 24, 3));
                r[T8] ^= 0x400;
                qmtc2(c, 10, rq(&r, &h, A0));
                { let v = uycpu::mmi::pminw(rq(&r, &h, V0), rq(&r, &h, AT)); setq(&mut r, &mut h, V0, v); }
                c.wr32(r[A3] as u32 + 0x3a00, r[V0] as u32);
                r[T2] = c.rd64(r[FP] as u32 + 0x38);
                { let v = qmfc2(c, 3); setq(&mut r, &mut h, AT, v); }
                vop::<1, 14>(c, 18, 18, c.vf[24]);
                r[V1] = c.rd8(r[FP] as u32 + 0x23) as u64;
                r[AT] >>= 39;
                r[V0] = 0x80;
                { let v = uycpu::mmi::pminw(rq(&r, &h, AT), rq(&r, &h, V0)); setq(&mut r, &mut h, AT, v); }
                lo = sx(((r[AT] as i32 as i64) * (r[V1] as i32 as i64)) as i32 as u32);
                r[A0] = 0x4780_0000;
                let t8 = r[T8] as u32;
                for o in [0x40u32, 0x50, 0x60, 0x70, 0x80, 0x90, 0xa0] {
                    c.wr128(t8 + o, 0, 0);
                }
                r[T6] &= 0x2208;
                r[AT] = ((r[AT] as i64) < 128) as u64;
                r[AT] = sx((r[AT] as u32) << 3);
                r[T6] |= r[AT];
                { let v = uycpu::mmi::pextlw(rq(&r, &h, A0), rq(&r, &h, A0)); setq(&mut r, &mut h, A0, v); }
                { let v = uycpu::mmi::pextlw(rq(&r, &h, A0), rq(&r, &h, A0)); setq(&mut r, &mut h, A0, v); }
                r[AT] = lo;
                r[AT] = sx((r[AT] as u32) >> 7);
                r[V0] = r[T2] >> 32;
                r[V1] = sx((r[AT] as u32) << 24);
                f10 = r[AT] as u32;
                { let v = uycpu::mmi::por(rq(&r, &h, V0), rq(&r, &h, V1)); setq(&mut r, &mut h, V0, v); }
                { let v = uycpu::mmi::pextlb((0, 0), rq(&r, &h, V0)); setq(&mut r, &mut h, V0, v); }
                { let v = uycpu::mmi::pextlh((0, 0), rq(&r, &h, V0)); setq(&mut r, &mut h, V0, v); }
                { let v = uycpu::mmi::por(rq(&r, &h, V0), rq(&r, &h, A0)); setq(&mut r, &mut h, V0, v); }
                stq(c, t8 + 0xb0, rq(&r, &h, V0));
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, SP), (0, 0)); setq(&mut r, &mut h, AT, v); }
                lqc2(c, 15, r[FP] as u32 + 0xc0);
                lqc2(c, 16, r[FP] as u32 + 0xd0);
                r[AT] = sx((r[AT] as u32).wrapping_sub(r[SP] as u32));
                let t = (r[AT] as i64) < 0;
                lqc2(c, 17, r[FP] as u32 + 0xe0);
                if t {
                    l = 0x43_fa88;
                    continue;
                }
                // The per-instance constants of program 0 (`uyafmt::moby`, "The draw job and the skin job").
                vop::<1, 8>(c, 12, 0, c.vf[15]);
                vop::<1, 4>(c, 13, 0, c.vf[16]);
                vop::<1, 2>(c, 14, 0, c.vf[17]);
                vop::<1, 8>(c, 13, 0, bc(c, 15, 1));
                vop::<1, 8>(c, 14, 0, bc(c, 15, 2));
                vop::<1, 4>(c, 12, 0, bc(c, 16, 0));
                vop::<1, 4>(c, 14, 0, bc(c, 16, 2));
                vop::<1, 2>(c, 12, 0, bc(c, 17, 0));
                vop::<1, 2>(c, 13, 0, bc(c, 17, 1));
                vop::<6, 14>(c, 15, 15, bc(c, 10, 0));
                vop::<6, 14>(c, 16, 16, bc(c, 10, 0));
                vop::<6, 14>(c, 17, 17, bc(c, 10, 0));
                for n in [15usize, 16, 17] {
                    vop::<6, 15>(c, ACC, 28, bc(c, n, 0));
                    vop::<2, 15>(c, ACC, 29, bc(c, n, 1));
                    vop::<2, 15>(c, ACC, 30, bc(c, n, 2));
                    vop::<2, 15>(c, n, 31, bc(c, n, 3));
                }
                vop::<6, 15>(c, ACC, 28, bc(c, 18, 0));
                vop::<2, 15>(c, ACC, 29, bc(c, 18, 1));
                vop::<2, 15>(c, ACC, 30, bc(c, 18, 2));
                vop::<2, 15>(c, 18, 31, bc(c, 0, 3));
                r[S1] = sx(c.rd32(r[FP] as u32 + 0x24));
                r[T7] = c.rd8(r[FP] as u32 + 0x63) as u64;
                r[T1] = add32(r[T1], r[S6]);
                r[AT] = c.rd8(r[FP] as u32 + 0x62) as u64;
                // A prop's near distance is the port's range, 255 (`range`): its near meshes at every distance.
                if let Some(p) = props && crate::range::prop(c, p, r[FP] as u32) {
                    r[AT] = 255;
                }
                r[T1] = add32(r[T1], 0x2600);
                let t = r[T7] != 0;
                { let v = qmfc2(c, 1); setq(&mut r, &mut h, T0, v); }
                l = if t { 0x44_0038 } else { 0x43_f128 };
            }
            0x43_f128 => {
                r[V1] = sx(c.rd32(r[FP] as u32 + 0x44));
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, S7), (0, 0)); setq(&mut r, &mut h, A0, v); }
                { let v = uycpu::mmi::pminw(rq(&r, &h, T0), rq(&r, &h, A0)); setq(&mut r, &mut h, T0, v); }
                r[A0] = sx(c.rd32(r[FP] as u32 + 0x58));
                r[AT] = sx((r[AT] as u32) << 10);
                r[A1] = sx(c.rd32(r[FP] as u32 + 0x5c));
                { let v = uycpu::mmi::pmaxw(rq(&r, &h, T0), (0, 0)); setq(&mut r, &mut h, T0, v); }
                r[A2] = sx(c.rd32(r[FP] as u32 + 0x50));
                r[T0] = sx(r[T0] as u32);
                r[A3] = sx(c.rd32(r[FP] as u32 + 0x54));
                r[AT] = sx((r[T0] as u32).wrapping_sub(r[AT] as u32));
                let t8 = r[T8] as u32;
                c.wr32(t8 + 4, r[V1] as u32);
                c.wr32(t8 + 8, r[A2] as u32);
                c.wr32(t8 + 0xc, r[A3] as u32);
                let t = (r[AT] as i64) > 0;
                r[T3] = c.rd32(r[FP] as u32 + 0x40) as u64;
                l = if t { 0x43_f190 } else { 0x43_f170 };
            }
            0x43_f170 => {
                r[T0] = c.rd8(r[T1] as u32) as u64;
                { let v = uycpu::mmi::pcpyld((0, 0), rq(&r, &h, S4)); setq(&mut r, &mut h, S4, v); }
                r[AT] = 0;
                r[V0] = c.rd8(r[S1] as u32 + 4) as u64;
                r[T0] = add32(r[T0], 1);
                r[A2] = c.rd8(r[S1] as u32 + 8) as u64;
                c.wr8(r[T1] as u32, r[T0] as u8);
                l = 0x43_f1b8;
            }
            0x43_f190 => {
                r[V0] = c.rd8(r[S1] as u32 + 5) as u64;
                r[A3] = 1;
                let t = r[V0] == 0;
                r[T0] = c.rd8(r[T1] as u32 + 1) as u64;
                if t {
                    l = 0x43_f170;
                    continue;
                }
                r[AT] = c.rd8(r[S1] as u32 + 4) as u64;
                r[T0] = add32(r[T0], 1);
                r[A2] = c.rd8(r[S1] as u32 + 9) as u64;
                c.wr8(r[T1] as u32 + 1, r[T0] as u8);
                { let v = uycpu::mmi::pcpyld(rq(&r, &h, A3), rq(&r, &h, S4)); setq(&mut r, &mut h, S4, v); }
                l = 0x43_f1b8;
            }
            0x43_f1b8 => {
                r[T4] = sx((r[T3] as u32) >> 16);
                r[S4] = c.rd16(r[FP] as u32 + 0x7c) as i16 as i64 as u64;
                r[T0] = sx(c.rd32(r[S1] as u32 + 0x18));
                r[T4] &= 0xff;
                r[T1] = sx(c.rd32(r[S1] as u32 + 0x14));
                r[T5] = sx((r[T3] as u32) >> 24);
                r[S1] = sx(c.rd32(r[S1] as u32));
                let t8 = r[T8] as u32;
                c.wr32(t8, r[V0] as u32);
                r[AT] = sx((r[AT] as u32) << 4);
                c.wr32(t8 + 0x10, r[A2] as u32);
                r[V0] = sx((r[V0] as u32) << 4);
                c.wr32(t8 + 0x14, r[T7] as u32);
                r[S1] = add32(r[S1], r[AT]);
                c.wr32(t8 + 0x18, r[T0] as u32);
                r[S0] = add32(r[S1], r[V0]);
                c.wr32(t8 + 0x1c, r[T1] as u32);
                let t = r[A2] == 0;
                r[A2] = r[T3] & 0xff;
                if !t {
                    r[A3] = sx((r[T3] as u32) >> 8);
                    if r[T4] == r[T5] {
                        r[A3] &= 0xff;
                        r[A2] = add32(r[A2], 1);
                        let t = r[A2] != r[A3];
                        r[A2] = 0x400;
                        if !t {
                            c.wr16(t8 + 0x12, r[A2] as u16);
                        }
                    }
                    for (base, o) in [(A0, 0x20u32), (A1, 0x30)] {
                        let p = r[base] as u32;
                        r[A2] = c.rd16(p + 6) as u64;
                        r[A3] = sx(c.rd32(p + 8));
                        r[T0] = sx(c.rd32(p + 0xc));
                        r[base] = add32(r[base], 0x10);
                        c.wr32(t8 + o, r[base] as u32);
                        c.wr32(t8 + o + 4, r[A2] as u32);
                        c.wr32(t8 + o + 8, r[A3] as u32);
                        c.wr32(t8 + o + 0xc, r[T0] as u32);
                    }
                }
                l = 0x43_f260;
            }
            0x43_f260 => {
                // The light set (byte 2 of +0x38): one set, or two blended by byte 1; their directions through the moby's rows.
                r[T5] = c.rd16(r[FP] as u32 + 0x34) as u64;
                r[V1] = r[T2] >> 16;
                r[T3] = c.rd8(r[FP] as u32 + 0x7e) as i8 as i64 as u64;
                r[AT] = r[T2] & 0xf;
                r[V1] &= 0xff;
                r[AT] = sx((r[AT] as u32) << 6);
                r[T5] &= 0x10;
                r[T4] = r[T3] & 1;
                let t = r[V1] != 0;
                r[AT] = add32(r[AT], r[S6]);
                let at = r[AT] as u32;
                let t8 = r[T8] as u32;
                if !t {
                    lqc2(c, 1, at + 0x2010);
                    lqc2(c, 2, at + 0x2030);
                    vop::<6, 15>(c, ACC, 12, bc(c, 1, 0));
                    { let v = c.rd128(at + 0x2000); setq(&mut r, &mut h, A0, v); }
                    vop::<2, 15>(c, ACC, 13, bc(c, 1, 1));
                    { let v = c.rd128(at + 0x2020); setq(&mut r, &mut h, A1, v); }
                    { let v = uycpu::mmi::pmaxw(rq(&r, &h, A0), (0, 0)); setq(&mut r, &mut h, A0, v); }
                    { let v = uycpu::mmi::pmaxw(rq(&r, &h, A1), (0, 0)); setq(&mut r, &mut h, A1, v); }
                    vop::<2, 15>(c, 1, 14, bc(c, 1, 2));
                    stq(c, t8 + 0x70, rq(&r, &h, A0));
                    vop::<6, 15>(c, ACC, 12, bc(c, 2, 0));
                    stq(c, t8 + 0x80, rq(&r, &h, A1));
                    vop::<2, 15>(c, ACC, 13, bc(c, 2, 1));
                    r[A0] = sx(c.rd32(at + 0x200c));
                    vop::<2, 15>(c, 2, 14, bc(c, 2, 2));
                    r[A1] = sx(c.rd32(at + 0x202c));
                    c.wr32(t8 + 0xa0, r[A0] as u32);
                    c.wr32(t8 + 0xa4, r[A1] as u32);
                    { let v = qmfc2(c, 1); setq(&mut r, &mut h, A0, v); }
                    { let v = qmfc2(c, 2); setq(&mut r, &mut h, A2, v); }
                    c.wr32(t8 + 0x40, r[A0] as u32);
                    r[A1] = r[A0] >> 32;
                    c.wr32(t8 + 0x50, r[A1] as u32);
                    { let v = uycpu::mmi::pcpyud(rq(&r, &h, A0), rq(&r, &h, A0)); setq(&mut r, &mut h, A0, v); }
                    c.wr32(t8 + 0x60, r[A0] as u32);
                    c.wr32(t8 + 0x44, r[A2] as u32);
                    r[A3] = r[A2] >> 32;
                    c.wr32(t8 + 0x54, r[A3] as u32);
                    { let v = uycpu::mmi::pcpyud(rq(&r, &h, A2), rq(&r, &h, A2)); setq(&mut r, &mut h, A2, v); }
                    c.wr32(t8 + 0x64, r[A2] as u32);
                } else {
                    r[V1] = sx((r[V1] as u32) << 4);
                    r[V0] = r[T2] & 0xf00;
                    r[A0] = 0x1000;
                    r[V0] = sx((r[V0] as u32) >> 2);
                    r[A0] = sx((r[A0] as u32).wrapping_sub(r[V1] as u32));
                    r[V0] = add32(r[V0], r[S6]);
                    vop::<0, 14>(c, 10, 0, bc(c, 0, 3));
                    { let v = uycpu::mmi::pcpyld(rq(&r, &h, A0), rq(&r, &h, V1)); setq(&mut r, &mut h, V1, v); }
                    qmtc2(c, 9, rq(&r, &h, V1));
                    lqc2(c, 1, at + 0x2010);
                    lqc2(c, 2, at + 0x2030);
                    lqc2(c, 3, at + 0x2000);
                    lqc2(c, 4, at + 0x2020);
                    itof::<2, 10>(c, 9, 9);
                    let v0 = r[V0] as u32;
                    lqc2(c, 5, v0 + 0x2010);
                    lqc2(c, 6, v0 + 0x2030);
                    lqc2(c, 7, v0 + 0x2000);
                    lqc2(c, 8, v0 + 0x2020);
                    vop::<6, 14>(c, 1, 1, bc(c, 9, 2));
                    vop::<6, 14>(c, 2, 2, bc(c, 9, 2));
                    vop::<6, 15>(c, 3, 3, bc(c, 9, 2));
                    vop::<6, 15>(c, 4, 4, bc(c, 9, 2));
                    vop::<6, 14>(c, 5, 5, bc(c, 9, 0));
                    vop::<6, 14>(c, 6, 6, bc(c, 9, 0));
                    vop::<6, 15>(c, 7, 7, bc(c, 9, 0));
                    vop::<6, 15>(c, 8, 8, bc(c, 9, 0));
                    vop::<0, 14>(c, 1, 1, c.vf[5]);
                    vop::<0, 14>(c, 2, 2, c.vf[6]);
                    vop::<0, 15>(c, 3, 3, c.vf[7]);
                    vop::<0, 15>(c, 4, 4, c.vf[8]);
                    vop::<6, 14>(c, 5, 1, c.vf[1]);
                    vop::<6, 14>(c, 6, 2, c.vf[2]);
                    vop::<4, 14>(c, 3, 3, c.vf[0]);
                    vop::<4, 14>(c, 4, 4, c.vf[0]);
                    vop::<0, 8>(c, ACC, 5, bc(c, 5, 1));
                    vop::<2, 8>(c, 5, 10, bc(c, 5, 2));
                    vop::<0, 8>(c, ACC, 6, bc(c, 6, 1));
                    vop::<2, 8>(c, 6, 10, bc(c, 6, 2));
                    qop::<2>(c, c.vf[0][3], c.vf[5][0]);
                    sqc2(c, 3, t8 + 0x70);
                    sqc2(c, 4, t8 + 0x80);
                    rot3(c, 1, 12, 1);
                    r[AT] = sx(c.rd32(t8 + 0x7c));
                    r[V0] = sx(c.rd32(t8 + 0x8c));
                    c.wr32(t8 + 0x7c, 0);
                    c.wr32(t8 + 0x8c, 0);
                    c.wr32(t8 + 0xa0, r[AT] as u32);
                    c.wr32(t8 + 0xa4, r[V0] as u32);
                    vop::<6, 14>(c, 1, 1, qq(c));
                    qop::<2>(c, c.vf[0][3], c.vf[6][0]);
                    rot3(c, 2, 12, 2);
                    { let v = qmfc2(c, 1); setq(&mut r, &mut h, A0, v); }
                    c.wr32(t8 + 0x40, r[A0] as u32);
                    r[A1] = r[A0] >> 32;
                    c.wr32(t8 + 0x50, r[A1] as u32);
                    { let v = uycpu::mmi::pcpyud(rq(&r, &h, A0), rq(&r, &h, A0)); setq(&mut r, &mut h, A0, v); }
                    c.wr32(t8 + 0x60, r[A0] as u32);
                    vop::<6, 14>(c, 2, 2, qq(c));
                    { let v = qmfc2(c, 2); setq(&mut r, &mut h, A0, v); }
                    c.wr32(t8 + 0x44, r[A0] as u32);
                    r[A1] = r[A0] >> 32;
                    c.wr32(t8 + 0x54, r[A1] as u32);
                    { let v = uycpu::mmi::pcpyud(rq(&r, &h, A0), rq(&r, &h, A0)); setq(&mut r, &mut h, A0, v); }
                    c.wr32(t8 + 0x64, r[A0] as u32);
                }
                // The point lights within reach of the moby's sphere: their distance, scaled, into 0x3400 (two quadwords each).
                r[AT] = (SPR + 0x3400) as u64;
                r[A0] = 0;
                lqc2(c, 1, r[FP] as u32);
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, S6), (0, 0)); setq(&mut r, &mut h, V1, v); }
                let t = r[V1] == 0;
                r[T8] = add32(r[T8], 0xc0);
                if !t {
                    lqc2(c, 2, SPR + 0x2410);
                    r[V0] = (SPR + 0x2400) as u64;
                    vop::<1, 14>(c, 3, 2, c.vf[1]);
                    vop::<0, 14>(c, 6, 0, bc(c, 0, 3));
                    vop::<1, 14>(c, 7, 0, bc(c, 0, 3));
                    loop {
                        let t = r[V1] == 0;
                        vop::<6, 14>(c, 3, 3, c.vf[3]);
                        if t {
                            break;
                        }
                        vop::<6, 1>(c, 4, 2, c.vf[2]);
                        r[V0] = add32(r[V0], 0x20);
                        lqc2(c, 2, r[V0] as u32 + 0x10);
                        r[V1] = add32(r[V1], 0xffff_ffff);
                        vop::<0, 4>(c, ACC, 3, bc(c, 3, 0));
                        vop::<2, 4>(c, ACC, 6, bc(c, 3, 2));
                        vop::<2, 4>(c, 5, 7, bc(c, 4, 3));
                        vop::<2, 4>(c, 8, 0, bc(c, 0, 0));
                        vop::<1, 14>(c, 3, 2, c.vf[1]);
                        { let v = qmfc2(c, 5); setq(&mut r, &mut h, A1, v); }
                        qop::<1>(c, c.vf[0][0], c.vf[8][1]);
                        if (r[A1] as i64) > 0 {
                            continue;
                        }
                        let v0 = r[V0] as u32;
                        lqc2(c, 8, v0 - 0x10);
                        lqc2(c, 9, v0 - 0x20);
                        vop::<1, 14>(c, 8, 1, c.vf[8]);
                        vop::<6, 1>(c, 5, 0, qq(c));
                        vop::<6, 1>(c, 9, 9, qq(c));
                        qop::<0>(c, c.vf[0][3], c.vf[5][3]);
                        vop::<1, 1>(c, 9, 0, c.vf[9]);
                        rot3(c, 8, 12, 8);
                        vop::<6, 14>(c, 9, 9, bc(c, 9, 3));
                        vop::<6, 14>(c, 8, 8, qq(c));
                        sqc2(c, 9, r[AT] as u32);
                        r[A0] = add32(r[A0], 1);
                        sqc2(c, 8, r[AT] as u32 + 0x10);
                        r[AT] = add32(r[AT], 0x20);
                    }
                }
                l = if r[T4] != 0 { 0x44_0330 } else { 0x43_f568 };
            }
            0x43_f568 => {
                // The lights' average: none, the one, or their weighted sum normalized, into the record's light section.
                let t8 = r[T8] as u32;
                if r[A0] != 0 {
                    r[AT] = (SPR + 0x3400) as u64;
                    if r[A0] == 1 {
                        { let v = c.rd128(SPR + 0x3400); setq(&mut r, &mut h, V0, v); }
                        { let v = c.rd128(SPR + 0x3410); setq(&mut r, &mut h, V1, v); }
                        { let v = uycpu::mmi::pmaxw(rq(&r, &h, V0), (0, 0)); setq(&mut r, &mut h, V0, v); }
                        stq(c, t8.wrapping_sub(0x30), rq(&r, &h, V0));
                        c.wr32(t8.wrapping_sub(0x24), 0);
                        c.wr32(t8.wrapping_sub(0x78), r[V1] as u32);
                        r[A0] = r[V1] >> 32;
                        c.wr32(t8.wrapping_sub(0x68), r[A0] as u32);
                        { let v = uycpu::mmi::pcpyud(rq(&r, &h, V1), rq(&r, &h, V1)); setq(&mut r, &mut h, V1, v); }
                        c.wr32(t8.wrapping_sub(0x58), r[V1] as u32);
                    } else {
                        vop::<0, 14>(c, 1, 0, c.vf[0]);
                        r[V1] = 0x3eaa_0000;
                        vop::<0, 14>(c, 2, 0, c.vf[0]);
                        qmtc2(c, 5, rq(&r, &h, V1));
                        vop::<0, 4>(c, 5, 0, bc(c, 0, 3));
                        loop {
                            lqc2(c, 3, r[AT] as u32);
                            vop::<6, 14>(c, 6, 3, bc(c, 5, 0));
                            lqc2(c, 4, r[AT] as u32 + 0x10);
                            vop::<0, 14>(c, 1, 1, c.vf[3]);
                            r[AT] = add32(r[AT], 0x20);
                            vop::<0, 4>(c, ACC, 6, bc(c, 6, 0));
                            vop::<2, 4>(c, 6, 5, bc(c, 6, 2));
                            vop::<6, 14>(c, 4, 4, bc(c, 6, 1));
                            r[A0] = add32(r[A0], 0xffff_ffff);
                            let t = r[A0] != 0;
                            vop::<0, 14>(c, 2, 2, c.vf[4]);
                            if !t {
                                break;
                            }
                        }
                        vop::<6, 14>(c, 3, 2, c.vf[2]);
                        vop::<0, 4>(c, ACC, 3, bc(c, 3, 0));
                        vop::<2, 4>(c, 3, 5, bc(c, 3, 2));
                        qop::<2>(c, c.vf[0][3], c.vf[3][1]);
                        vop::<4, 14>(c, 1, 1, c.vf[0]);
                        vop::<6, 14>(c, 2, 2, qq(c));
                        sqc2(c, 1, t8.wrapping_sub(0x30));
                        c.wr32(t8.wrapping_sub(0x24), 0);
                        { let v = qmfc2(c, 2); setq(&mut r, &mut h, V1, v); }
                        c.wr32(t8.wrapping_sub(0x78), r[V1] as u32);
                        r[A0] = r[V1] >> 32;
                        c.wr32(t8.wrapping_sub(0x68), r[A0] as u32);
                        { let v = uycpu::mmi::pcpyud(rq(&r, &h, V1), rq(&r, &h, V1)); setq(&mut r, &mut h, V1, v); }
                        c.wr32(t8.wrapping_sub(0x58), r[V1] as u32);
                    }
                } else {
                    r[AT] = 1;
                }
                let t = (r[T3] as i64) < 0;
                f11 = r[T3] as u32;
                l = if t { 0x43_ffd8 } else { 0x43_f660 };
            }
            0x43_f660 => {
                let t = r[T5] != 0;
                { let v = c.rd128(SPR + 0x3f60); setq(&mut r, &mut h, T1, v); }
                l = if t { 0x44_0018 } else { 0x43_f668 };
            }
            0x43_f668 => {
                { let v = c.rd128(SPR + 0x3e80); setq(&mut r, &mut h, T2, v); }
                { let v = c.rd128(SPR + 0x3e90); setq(&mut r, &mut h, T3, v); }
                { let v = c.rd128(SPR + 0x3ec0); setq(&mut r, &mut h, T4, v); }
                let t = (r[S4] as i64) < 0;
                { let v = c.rd128(SPR + 0x3ed0); setq(&mut r, &mut h, T5, v); }
                l = if t { 0x43_fba0 } else { 0x43_f680 };
            }
            0x43_f680 => {
                // One mesh record: program 0's matrix rows (the first two), the UNPACKs of the positions and colours (the job's
                // output in RAM, by its entry), packet A by REF, the MSCAL; the skin job's entry (packet B, its output).
                let t = (r[S2] as i64) <= 0;
                r[AT] = 0x8129;
                if !t {
                    let sp = r[SP] as u32;
                    stq(c, sp, rq(&r, &h, T1));
                    r[S2] = add32(r[S2], 0xffff_ffff);
                    for (n, o) in [(15usize, 0x10u32), (16, 0x20), (17, 0x30), (18, 0x40)] {
                        sqc2(c, n, sp + o);
                    }
                    c.wr16(sp + 0xc, r[AT] as u16);
                    r[SP] = add32(r[SP], 0x50);
                }
                let sp = r[SP] as u32;
                let t8 = r[T8] as u32;
                r[AT] = sx(c.rd32(r[S1] as u32 + 8));
                r[V0] = sx(c.rd32(r[S1] as u32 + 0xc));
                r[A2] = add32(r[SP], 0x20);
                stq(c, sp, rq(&r, &h, T2));
                r[AT] = sx((r[AT] as u32) << 4);
                stq(c, sp + 0x10, rq(&r, &h, T3));
                r[V1] = r[V0] & 0xff;
                r[AT] |= r[V1];
                r[V1] = sx((r[V0] as u32) >> 24);
                c.wr32(t8, r[AT] as u32);
                r[A0] = sx((r[V0] as u32) >> 8);
                c.wr32(t8 + 4, r[A2] as u32);
                r[A0] &= 0xff;
                c.wr8(sp + 0xe, r[V1] as u8);
                r[A1] = sx((r[V0] as u32) >> 16);
                c.wr8(sp + 0x1e, r[V1] as u8);
                r[A1] &= 0xff;
                c.wr16(sp, r[A0] as u16);
                r[A0] = sx((r[A0] as u32) << 4);
                c.wr16(sp + 0x10, r[A1] as u16);
                r[A1] = sx((r[A1] as u32) << 4);
                r[T8] = add32(r[T8], 8);
                r[A2] = add32(r[A2], r[A1]);
                c.wr32(sp + 4, r[A2] as u32);
                r[A2] = add32(r[A2], r[A0]);
                c.wr32(sp + 0x14, r[A2] as u32);
                r[SP] = add32(r[A2], 0);
                r[V0] = c.rd64(r[S1] as u32);
                r[S1] = add32(r[S1], 0x10);
                stq(c, r[SP] as u32, rq(&r, &h, T4));
                r[SP] = add32(r[SP], 0x10);
                let sp = r[SP] as u32;
                c.wr32(sp - 0xc, r[V0] as u32);
                r[V1] = r[V0] >> 32;
                let t = (r[S2] as i64) <= 0;
                c.wr16(sp - 0x10, r[V1] as u16);
                if !t {
                    let t = r[T6] == 0;
                    r[A1] = sx(f11);
                    if !t {
                        let sp = r[SP] as u32;
                        c.wr128(sp, 0, 0);
                        c.wr32(sp, 0x1000_0000);
                        c.wr32(sp + 0xc, 0x1300_0000);
                        r[SP] = add32(r[SP], 0x10);
                        let sp = r[SP] as u32;
                        { let v = c.rd128(SPR + 0x3f00); setq(&mut r, &mut h, V0, v); }
                        r[AT] = r[T6] & 0x200;
                        { let v = c.rd128(SPR + 0x3f10); setq(&mut r, &mut h, V1, v); }
                        r[A2] = r[T7] & 0x2000;
                        stq(c, sp, rq(&r, &h, V0));
                        { let v = c.rd128(SPR + 0x3f20); setq(&mut r, &mut h, A0, v); }
                        stq(c, sp + 0x10, rq(&r, &h, V1));
                        let t = r[AT] != 0;
                        r[A0] = 0x48;
                        if !t {
                            let t = r[A2] != 0;
                            { let v = c.rd128(SPR + 0x3f40); setq(&mut r, &mut h, A0, v); }
                            if !t {
                                { let v = c.rd128(SPR + 0x3f30); setq(&mut r, &mut h, A0, v); }
                                r[V1] = add32(r[V1], 0xffff_fa00);
                                r[A0] = add32(r[A0], r[V1]);
                            }
                        }
                        stq(c, sp + 0x20, rq(&r, &h, A0));
                        r[SP] = add32(r[SP], 0x30);
                    }
                    r[A1] &= 8;
                    r[AT] = sx(f10);
                    let t = r[A1] == 0;
                    { let v = c.rd128(SPR + 0x3f50); setq(&mut r, &mut h, V1, v); }
                    if !t {
                        let t = r[T6] != 0;
                        { let v = c.rd128(SPR + 0x3f00); setq(&mut r, &mut h, A2, v); }
                        if !t {
                            let sp = r[SP] as u32;
                            c.wr128(sp, 0, 0);
                            c.wr32(sp, 0x1000_0000);
                            c.wr32(sp + 0xc, 0x1300_0000);
                            r[SP] = add32(r[SP], 0x10);
                        }
                        let sp = r[SP] as u32;
                        { let v = c.rd128(SPR + 0x3f10); setq(&mut r, &mut h, V0, v); }
                        r[AT] <<= 32;
                        stq(c, sp, rq(&r, &h, A2));
                        r[V1] |= r[AT];
                        stq(c, sp + 0x10, rq(&r, &h, V0));
                        f12 = 0;
                        stq(c, sp + 0x20, rq(&r, &h, V1));
                        r[SP] = add32(r[SP], 0x30);
                    }
                }
                let sp = r[SP] as u32;
                stq(c, sp, rq(&r, &h, T5));
                r[SP] = add32(r[SP], 0x10);
                let t = r[S1] != r[S0];
                c.wr16(r[SP] as u32 - 4, r[S3] as u16);
                l = if t { 0x43_f680 } else { 0x43_f800 };
            }
            0x43_f800 => {
                // The next LOD entry, or the chain's tail: the fog and colour setups (0x3F00 to 0x3F50) as the moby's flags ask.
                if r[S4] != 0 {
                    l = 0x43_fbc8;
                    continue;
                }
                let t = r[T6] == 0;
                r[A1] = sx(f11);
                if !t {
                    let sp = r[SP] as u32;
                    c.wr128(sp, 0, 0);
                    c.wr32(sp, 0x1000_0000);
                    c.wr32(sp + 0xc, 0x1300_0000);
                    r[SP] = add32(r[SP], 0x10);
                    let sp = r[SP] as u32;
                    { let v = c.rd128(SPR + 0x3f00); setq(&mut r, &mut h, V0, v); }
                    r[AT] = r[T6] & 0x200;
                    { let v = c.rd128(SPR + 0x3f10); setq(&mut r, &mut h, V1, v); }
                    stq(c, sp, rq(&r, &h, V0));
                    r[A0] = r[T6] & 0x2000;
                    let t = r[A0] != 0;
                    stq(c, sp + 0x10, rq(&r, &h, V1));
                    if t {
                        l = 0x43_f910;
                        continue;
                    }
                    let t = r[AT] != 0;
                    { let v = c.rd128(SPR + 0x3f20); setq(&mut r, &mut h, A0, v); }
                    if !t {
                        { let v = c.rd128(SPR + 0x3f30); setq(&mut r, &mut h, A0, v); }
                    }
                    stq(c, sp + 0x20, rq(&r, &h, A0));
                    r[SP] = add32(r[SP], 0x30);
                }
                l = 0x43_f860;
            }
            0x43_f860 => {
                let t = r[A1] == 0;
                { let v = c.rd128(SPR + 0x3f00); setq(&mut r, &mut h, V1, v); }
                if !t {
                    let t = r[T6] != 0;
                    { let v = c.rd128(SPR + 0x3f10); setq(&mut r, &mut h, A2, v); }
                    if !t {
                        let sp = r[SP] as u32;
                        c.wr128(sp, 0, 0);
                        c.wr32(sp, 0x1000_0000);
                        c.wr32(sp + 0xc, 0x1300_0000);
                        r[SP] = add32(r[SP], 0x10);
                    }
                    let sp = r[SP] as u32;
                    { let v = c.rd128(SPR + 0x3f20); setq(&mut r, &mut h, V0, v); }
                    stq(c, sp, rq(&r, &h, V1));
                    stq(c, sp + 0x10, rq(&r, &h, A2));
                    stq(c, sp + 0x20, rq(&r, &h, V0));
                    r[SP] = add32(r[SP], 0x30);
                }
                l = 0x43_f8a0;
            }
            0x43_f8a0 => {
                let t = r[T7] != 0;
                c.wr64(r[T8] as u32, 0);
                if t {
                    l = 0x44_0070;
                    continue;
                }
                r[AT] = 0x1000_d000;
                r[T8] = add32(r[T8], 0x10);
                l = 0x43_f8b8;
            }
            0x43_f8b8 => {
                // The record to the job list in RAM (SPR_FROM from its 1 KB buffer), the next moby.
                r[A0] = r[T8] & 0x3ff;
                c.wr64(r[T8] as u32 - 8, 0);
                r[T8] &= 0x2c00;
                r[A0] = sx((r[A0] as u32) >> 4);
                spr_from(c, 0x100, r[T9] as u32, r[A0] as u32, r[T8] as u32);
                if extra_pass.is_some() {
                    far_log.drawn += 1;
                }
                r[A0] = sx((r[A0] as u32) << 4);
                r[V0] = 0x100;
                r[T9] = add32(r[T9], r[A0]);
                let t = (r[RA] as i64) >= 0;
                r[T8] = add32(r[T8], r[S6]);
                l = if t { 0x43_ee70 } else { 0x43_fd30 };
            }
            0x43_f910 => {
                // Flag 0x2000: the record's entries read back from the record (the moby drawn again with the chain's tags).
                r[T0] = r[T8] & 0x2c00;
                r[T0] = add32(r[T0], r[S6]);
                r[T0] = add32(r[T0], 0xc0);
                r[AT] = sx((r[T8] as u32).wrapping_sub(r[T0] as u32));
                r[AT] = sx((r[AT] as u32) << 1);
                r[SP] = add32(r[SP], 0xffff_fff0);
                r[S1] = sx((r[S1] as u32).wrapping_sub(r[AT] as u32));
                r[S2] = 2;
                loop {
                    let t = (r[S2] as i64) <= 0;
                    r[AT] = 0x8129;
                    if !t {
                        let sp = r[SP] as u32;
                        stq(c, sp, rq(&r, &h, T1));
                        r[S2] = add32(r[S2], 0xffff_ffff);
                        for (n, o) in [(15usize, 0x10u32), (16, 0x20), (17, 0x30), (18, 0x40)] {
                            sqc2(c, n, sp + o);
                        }
                        c.wr16(sp + 0xc, r[AT] as u16);
                        r[SP] = add32(r[SP], 0x50);
                    }
                    let sp = r[SP] as u32;
                    r[V0] = sx(c.rd32(r[S1] as u32 + 0xc));
                    r[AT] = sx(c.rd32(r[T0] as u32 + 4));
                    r[T0] = add32(r[T0], 8);
                    stq(c, sp, rq(&r, &h, T2));
                    r[V1] = sx((r[V0] as u32) >> 24);
                    stq(c, sp + 0x10, rq(&r, &h, T3));
                    r[A0] = sx((r[V0] as u32) >> 8);
                    c.wr8(sp + 0xe, r[V1] as u8);
                    r[A0] &= 0xff;
                    c.wr16(sp, r[A0] as u16);
                    r[A1] = sx((r[V0] as u32) >> 16);
                    c.wr32(sp + 0x14, r[AT] as u32);
                    r[A1] &= 0xff;
                    c.wr16(sp + 0x10, r[A1] as u16);
                    r[A1] = sx((r[A1] as u32) << 4);
                    c.wr8(sp + 0x1e, r[V1] as u8);
                    r[AT] = add32(r[AT], r[A1]);
                    c.wr32(sp + 4, r[AT] as u32);
                    r[AT] = 0x3000;
                    c.wr16(sp + 0x12, 0x3000);
                    r[SP] = add32(r[SP], 0x20);
                    r[V0] = c.rd64(r[S1] as u32);
                    r[S1] = add32(r[S1], 0x10);
                    stq(c, r[SP] as u32, rq(&r, &h, T4));
                    r[SP] = add32(r[SP], 0x10);
                    let sp = r[SP] as u32;
                    c.wr32(sp - 0xc, r[V0] as u32);
                    r[V1] = r[V0] >> 32;
                    let t = (r[S2] as i64) <= 0;
                    c.wr16(sp - 0x10, r[V1] as u16);
                    if !t {
                        c.wr128(sp, 0, 0);
                        c.wr32(sp, 0x1000_0000);
                        c.wr32(sp + 0xc, 0x1300_0000);
                        r[SP] = add32(r[SP], 0x10);
                        let sp = r[SP] as u32;
                        { let v = c.rd128(SPR + 0x3f00); setq(&mut r, &mut h, V0, v); }
                        { let v = c.rd128(SPR + 0x3f10); setq(&mut r, &mut h, V1, v); }
                        { let v = c.rd128(SPR + 0x3f40); setq(&mut r, &mut h, A0, v); }
                        stq(c, sp, rq(&r, &h, V0));
                        stq(c, sp + 0x10, rq(&r, &h, V1));
                        stq(c, sp + 0x20, rq(&r, &h, A0));
                        r[A1] = 0x1001;
                        c.wr16(sp + 0x20, 0x1001);
                        r[SP] = add32(r[SP], 0x30);
                    }
                    let sp = r[SP] as u32;
                    stq(c, sp, rq(&r, &h, T5));
                    r[SP] = add32(r[SP], 0x10);
                    let t = r[S1] != r[S0];
                    c.wr16(r[SP] as u32 - 4, r[S3] as u16);
                    if !t {
                        break;
                    }
                }
                let sp = r[SP] as u32;
                c.wr128(sp, 0, 0);
                c.wr32(sp, 0x1000_0000);
                c.wr32(sp + 0xc, 0x1300_0000);
                r[SP] = add32(r[SP], 0x10);
                let sp = r[SP] as u32;
                { let v = c.rd128(SPR + 0x3f00); setq(&mut r, &mut h, V0, v); }
                { let v = c.rd128(SPR + 0x3f10); setq(&mut r, &mut h, V1, v); }
                { let v = c.rd128(SPR + 0x3f30); setq(&mut r, &mut h, A0, v); }
                stq(c, sp, rq(&r, &h, V0));
                stq(c, sp + 0x10, rq(&r, &h, V1));
                stq(c, sp + 0x20, rq(&r, &h, A0));
                r[SP] = add32(r[SP], 0x30);
                l = 0x43_f860;
            }
            0x43_fa60 => {
                // Culled: the drawn byte cleared, unless it was clear or the mode keeps it (ra's high half positive).
                let t = r[T0] == 0;
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, RA), (0, 0)); setq(&mut r, &mut h, AT, v); }
                if !t && (r[AT] as i64) <= 0 {
                    c.wr8(r[FP] as u32 + 0x31, 0);
                }
                l = 0x43_ee70;
            }
            0x43_fa78 => {
                // A moby whose +0x20 byte is negative: skipped; -1 ends the array.
                r[AT] = add32(r[AT], 1);
                l = if r[AT] != 0 { 0x43_ee70 } else { 0x43_fa88 };
            }
            0x43_fa88 => {
                // The first pass's end (or the chain full): the second pass when the mode asks for it (RA positive), else the
                // end. With DRAW DISTANCE FAR: the extras' pass after the first pass of a cull with no second pass; its own end
                // when it runs (its last extra, or the chain full); the chain full in the second pass ends the cull.
                l = if extra_pass.is_some() {
                    EXTRAS_END
                } else if (r[RA] as i64) > 0 {
                    ra_first = r[RA];
                    0x43_fc60
                } else if r[RA] == 0 && far.is_some() && !extras.is_empty() {
                    ra_first = r[RA];
                    EXTRAS_START
                } else {
                    0x43_fa90
                };
            }
            0x43_fa90 => {
                // The end: the near list's entries with a second pass mark into 0x21F0E0 (their count at gp+0x3830), the texture
                // list's end, the chain's last tag, the job list's zero quadword, the "nothing drawn" flag.
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, S5), (0, 0)); setq(&mut r, &mut h, V0, v); }
                r[A0] = sx(special);
                r[AT] = (SPR + 0x1000) as u64;
                r[V1] = 0;
                while r[AT] != r[V0] {
                    // DRAW DISTANCE FAR: the list ends where the texture list starts (48 entries), however many near mobies
                    // the game's own passes drew (FAR's extras are not in the list: `extras`).
                    if far.is_some() && r[A0] as u32 == tex_list {
                        break;
                    }
                    r[A1] = c.rd64(r[AT] as u32);
                    r[AT] = add32(r[AT], 0x10);
                    r[A2] = r[A1] >> 32;
                    c.wr64(r[A0] as u32, r[A1]);
                    if r[A2] != 0 {
                        r[A0] = add32(r[A0], 8);
                        r[V1] = add32(r[V1], 1);
                    }
                }
                if far.is_some() {
                    let chain = (r[SP] as u32).wrapping_sub(chain_start);
                    let chain_room = (h[SP] as u32).wrapping_sub(chain_start);
                    far_log.end(extras.len(), (r[T9] as u32).wrapping_sub(a1), room.wrapping_sub(a1), chain, chain_room);
                }
                c.wr32(g3830, r[V1] as u32);
                r[AT] = sx(c.rd32(SPR + 0x3e00));
                c.wr32(g3880b, r[AT] as u32);
                let sp = r[SP] as u32;
                c.wr128(sp, 0, 0);
                c.wr32(sp, 0x1000_0000);
                c.wr32(sp + 0xc, 0x1300_0000);
                r[SP] = add32(r[SP], 0x10);
                c.wr32(g4838b, r[SP] as u32);
                r[V1] = add32(r[T9], 0);
                c.wr128(SPR, 0, 0);
                spr_from(c, 0x100, r[V1] as u32, 1, 0);
                if f12 == 0 {
                    let t0 = ((f12 as i32) < 1) as u32;
                    let v = c.rd32(flags_word) | (t0 << 3);
                    c.wr32(flags_word, v);
                }
                break;
            }
            0x43_fba0 => {
                // The LOD table (+0x7C's mask negative): its entries from the highest set bit down.
                r[S4] &= 0x7fff;
                r[AT] = r[T8] & 0x2c00;
                let t = r[S4] != 0;
                r[AT] = add32(r[AT], r[S6]);
                if !t {
                    r[S4] = 1;
                }
                c.wr32(r[AT] as u32, 0);
                r[T7] = 0;
                c.wr32(r[AT] as u32 + 0x14, 0);
                l = 0x43_fbc8;
            }
            0x43_fbc8 => {
                count(2, 0);
                r[AT] = uycpu::mmi::plzcw(r[S4]);
                r[A0] = sx(c.rd32(r[FP] as u32 + 0x24));
                r[V0] = 30;
                r[V1] = 1;
                r[AT] = sx((r[V0] as u32).wrapping_sub(r[AT] as u32));
                r[A1] = c.rd8(r[A0] as u32 + 0x2c) as u64;
                r[V1] = sx((r[V1] as u32).wrapping_shl(r[AT] as u32 & 31));
                r[AT] = sx((r[AT] as u32) << 2);
                r[S4] ^= r[V1];
                let t = r[A1] == 0;
                r[A1] = sx((r[A1] as u32) << 4);
                if t {
                    l = 0x43_f800;
                    continue;
                }
                r[S1] = sx(c.rd32(r[A0] as u32));
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, S4), (0, 0)); setq(&mut r, &mut h, V0, v); }
                r[A0] = add32(r[A0], r[A1]);
                r[V1] = r[T8] & 0x2c00;
                r[A0] = add32(r[A0], r[AT]);
                r[V1] = add32(r[V1], r[S6]);
                let mut alt = false;
                if r[V0] != 0 {
                    r[AT] = c.rd8(r[A0] as u32 + 6) as u64;
                    r[S0] = c.rd8(r[A0] as u32 + 7) as u64;
                    alt = r[AT] != 0;
                }
                if !alt {
                    r[AT] = c.rd8(r[A0] as u32 + 4) as u64;
                    r[S0] = c.rd8(r[A0] as u32 + 5) as u64;
                }
                r[V0] = c.rd16(r[V1] as u32) as u64;
                r[AT] = sx((r[AT] as u32) << 4);
                let t = r[S0] == 0;
                r[S1] = add32(r[S1], r[AT]);
                if t {
                    l = 0x43_f800;
                    continue;
                }
                r[V0] = add32(r[V0], r[S0]);
                r[S0] = sx((r[S0] as u32) << 4);
                r[A0] = add32(r[V0], 0xffff_ff9a);
                r[S0] = add32(r[S0], r[S1]);
                if (r[A0] as i64) >= 0 {
                    l = 0x43_f800;
                    continue;
                }
                c.wr16(r[V1] as u32, r[V0] as u16);
                l = 0x43_f680;
            }
            0x43_fc60 => {
                // The second pass (mode bit 0): the chain's mark for 0x437B48 (0x1E00E4), the near list sorted (drawn ones
                // first) and drawn again from 0x43EFA0.
                let sp = r[SP] as u32;
                c.wr128(sp, 0, 0);
                c.wr32(sp, 0x1000_0000);
                c.wr32(sp + 0xc, 0x1300_0000);
                r[SP] = add32(r[SP], 0x10);
                c.wr32(g3834, r[SP] as u32);
                r[SP] = add32(r[SP], 0x10);
                r[RA] = u64::MAX;
                r[AT] = (SPR + 0x1000) as u64;
                r[V1] = (SPR + 0x1800) as u64;
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, S5), (0, 0)); setq(&mut r, &mut h, V0, v); }
                if r[AT] == r[V0] {
                    l = 0x43_fa90;
                    continue;
                }
                loop {
                    r[A0] = c.rd16(r[AT] as u32 + 4) as u64;
                    { let v = c.rd128(r[AT] as u32); setq(&mut r, &mut h, A1, v); }
                    r[AT] = add32(r[AT], 0x10);
                    r[A0] = ((r[A0] as i64) < 1) as u64;
                    r[A0] ^= 1;
                    stq(c, r[V1] as u32, rq(&r, &h, A1));
                    r[A0] = sx((r[A0] as u32) << 4);
                    let t = r[AT] != r[V0];
                    r[V1] = add32(r[V1], r[A0]);
                    if !t {
                        break;
                    }
                }
                r[AT] = (SPR + 0x1000) as u64;
                loop {
                    r[A0] = c.rd16(r[AT] as u32 + 4) as u64;
                    { let v = c.rd128(r[AT] as u32); setq(&mut r, &mut h, A1, v); }
                    r[AT] = add32(r[AT], 0x10);
                    r[A0] = ((r[A0] as i64) < 1) as u64;
                    stq(c, r[V1] as u32, rq(&r, &h, A1));
                    r[A0] = sx((r[A0] as u32) << 4);
                    let t = r[AT] != r[V0];
                    r[V1] = add32(r[V1], r[A0]);
                    if !t {
                        break;
                    }
                }
                r[AT] = (SPR + 0x1000) as u64;
                r[V1] = (SPR + 0x1800) as u64;
                loop {
                    { let v = c.rd128(r[V1] as u32); setq(&mut r, &mut h, A0, v); }
                    r[V1] = add32(r[V1], 0x10);
                    stq(c, r[AT] as u32, rq(&r, &h, A0));
                    r[AT] = add32(r[AT], 0x10);
                    if r[AT] == r[V0] {
                        break;
                    }
                }
                r[T5] = (SPR + 0x1000) as u64;
                c.wr32(SPR + 0x3ef0, r[T5] as u32);
                { let v = uycpu::mmi::pcpyld(rq(&r, &h, V0), rq(&r, &h, S5)); setq(&mut r, &mut h, S5, v); }
                l = 0x43_fd30;
            }
            0x43_fd30 => {
                r[T5] = sx(c.rd32(SPR + 0x3ef0));
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, S5), (0, 0)); setq(&mut r, &mut h, AT, v); }
                let t = r[T5] == r[AT];
                r[FP] = sx(c.rd32(r[T5] as u32));
                if t {
                    // The list's end: the cull's end, or with DRAW DISTANCE FAR the extras' pass after the second pass, or its
                    // next extra once it has drawn (or skipped) the one in slot 0.
                    l = if extra_pass.is_some() {
                        0x43_ee70
                    } else if far.is_some() && !extras.is_empty() {
                        EXTRAS_START
                    } else {
                        0x43_fa90
                    };
                    continue;
                }
                count(2, 1);
                let t5 = r[T5] as u32;
                r[T1] = c.rd16(t5 + 8) as i16 as i64 as u64;
                r[T3] = sx(c.rd32(t5 + 0xc));
                r[T6] = c.rd16(r[FP] as u32 + 0x34) as u64;
                r[A3] = sx((r[T1] as u32) << 1);
                r[AT] = c.rd16(r[FP] as u32 + 0x32) as i16 as i64 as u64;
                r[AT] = far_distance(r[AT], far_units);
                r[S3] = c.rd16(t5 + 0xa) as i16 as i64 as u64;
                r[T5] = add32(r[T5], 0x10);
                c.wr32(SPR + 0x3ef0, r[T5] as u32);
                r[AT] = sx((r[AT] as u32) << 10);
                let t = r[S3] == 0;
                r[AT] = sx((r[AT] as u32).wrapping_sub(r[T3] as u32));
                if t {
                    continue;
                }
                r[AT] <<= 32;
                qmtc2(c, 1, rq(&r, &h, T3));
                qmtc2(c, 3, rq(&r, &h, AT));
                l = 0x43_efa0;
            }
            0x43_fd80 => {
                // A moby with a near flag (+0x34 and the mode): into the near list (scratchpad 0x1000) with its depth; drawn in
                // the second pass, or now when it is far enough (0x43FE40's test against its parent's sphere).
                r[S3] = 0;
                uycpu::vu0::ftoi::<0, 4>(c, 3, 3);
                { let v = qmfc2(c, 8); setq(&mut r, &mut h, V1, v); }
                r[A0] ^= r[A1];
                let t = (r[V0] as i64) < 0;
                r[V0] <<= 32;
                let mut culled = t;
                if !culled {
                    let t = (r[V0] as i64) < 0;
                    uycpu::vu0::ftoi::<0, 12>(c, 1, 1);
                    culled = t;
                }
                if culled {
                    let t = r[T0] == 0;
                    { let v = uycpu::mmi::pcpyud(rq(&r, &h, RA), (0, 0)); setq(&mut r, &mut h, V0, v); }
                    if !t && (r[V0] as i64) <= 0 {
                        c.wr8(r[FP] as u32 + 0x31, 0);
                    }
                } else {
                    r[S3] = 10;
                    drawn(c, r[FP] as u32 + 0x31, r[T0], h[RA], wide, game_far);
                    let t = r[T0] != 0;
                    r[V1] |= r[A0];
                    if !t {
                        r[AT] = 1;
                    }
                    let t = (r[V1] as i64) < 0;
                    r[V1] <<= 32;
                    if !t && (r[V1] as i64) >= 0 {
                        r[S3] = 14;
                    }
                }
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, S5), (0, 0)); setq(&mut r, &mut h, T5, v); }
                { let v = qmfc2(c, 1); setq(&mut r, &mut h, T3, v); }
                r[V0] = (SPR + 0x1800) as u64;
                let t = r[T5] == r[V0];
                r[A2] = c.rd16(r[FP] as u32 + 0x34) as u64;
                if t {
                    l = 0x43_fa60;
                    continue;
                }
                let t5 = r[T5] as u32;
                c.wr32(t5, r[FP] as u32);
                c.wr16(t5 + 8, r[T1] as u16);
                c.wr16(t5 + 0xa, r[S3] as u16);
                r[V0] = r[A2] & 0x400;
                let t = r[V0] == 0;
                c.wr32(t5 + 0xc, r[T3] as u32);
                if t {
                    l = 0x43_ff98;
                    continue;
                }
                r[A0] = sx(c.rd32(r[FP] as u32 + 0x74));
                r[V1] = sx(c.rd32(r[FP] as u32 + 0x70));
                r[V0] = c.rd8(r[FP] as u32 + 0xbd) as i8 as i64 as u64;
                let t = r[A0] == 0;
                qmtc2(c, 4, rq(&r, &h, V1));
                if t {
                    l = 0x43_ffb0;
                    continue;
                }
                qmtc2(c, 9, rq(&r, &h, A0));
                r[V0] = sx((r[V0] as u32) << 4);
                l = if (r[V0] as i64) < 0 { 0x43_ff80 } else { 0x43_fe40 };
            }
            0x43_fe40 => {
                r[AT] = c.rd8(r[FP] as u32 + 0x6f) as u64;
                r[V0] = add32(r[V0], r[S6]);
                vop::<6, 14>(c, 4, 4, bc(c, 24, 3));
                vop::<6, 14>(c, 9, 9, bc(c, 24, 3));
                lqc2(c, 5, r[V0] as u32 + 0x3300);
                lqc2(c, 6, r[FP] as u32);
                lqc2(c, 7, r[FP] as u32 + 0xe0);
                lqc2(c, 8, r[FP] as u32 + 0x10);
                vop::<6, 14>(c, ACC, 8, bc(c, 24, 3));
                vop::<2, 14>(c, 4, 7, bc(c, 4, 0));
                vop::<0, 1>(c, 6, 6, bc(c, 9, 0));
                vop::<1, 14>(c, 9, 4, c.vf[6]);
                vop::<6, 14>(c, 8, 5, c.vf[7]);
                vop::<6, 14>(c, 9, 9, c.vf[7]);
                vop::<0, 8>(c, 8, 8, bc(c, 8, 1));
                vop::<0, 8>(c, 9, 9, bc(c, 9, 1));
                vop::<0, 8>(c, 8, 8, bc(c, 8, 2));
                vop::<0, 8>(c, 9, 9, bc(c, 9, 2));
                qop::<0>(c, c.vf[9][0], c.vf[8][0]);
                vop::<6, 14>(c, ACC, 5, qq(c));
                vop::<2, 14>(c, 1, 6, bc(c, 0, 3));
                r[AT] <<= 42;
                vop::<1, 14>(c, 2, 1, c.vf[24]);
                let t = r[AT] == 0;
                qmtc2(c, 4, rq(&r, &h, AT));
                if t {
                    l = 0x43_ffb0;
                    continue;
                }
                vop::<6, 12>(c, 3, 11, bc(c, 1, 3));
                itof::<0, 4>(c, 19, 4);
                vop::<6, 14>(c, ACC, 25, bc(c, 2, 0));
                vop::<2, 14>(c, ACC, 26, bc(c, 2, 1));
                vop::<2, 14>(c, 2, 27, bc(c, 2, 2));
                vop::<1, 4>(c, 19, 19, bc(c, 1, 3));
                vop::<6, 12>(c, 8, 22, bc(c, 1, 3));
                vop::<0, 12>(c, 1, 3, bc(c, 2, 2));
                vop::<6, 12>(c, 5, 20, bc(c, 2, 2));
                vmove::<12>(c, 2, uycpu::vu0::m_abs(c.vf[2]));
                vop::<6, 12>(c, 7, 8, bc(c, 22, 2));
                vop::<1, 12>(c, 3, 19, c.vf[1]);
                vop::<6, 12>(c, 9, 5, c.vf[21]);
                vop::<1, 12>(c, 6, 2, c.vf[8]);
                vop::<0, 12>(c, 8, 2, c.vf[7]);
                r[A1] = 0x4000_0000;
                vop::<1, 12>(c, 4, 1, c.vf[23]);
                r[A1] = r[A1].wrapping_add(r[A1]);
                { let v = qmfc2(c, 3); setq(&mut r, &mut h, AT, v); }
                vop::<1, 12>(c, 7, 5, c.vf[6]);
                vop::<1, 12>(c, 8, 9, c.vf[8]);
                r[V0] = r[AT] << 32;
                r[T2] = 1;
                let t = (r[AT] as i64) < 0;
                { let v = qmfc2(c, 4); setq(&mut r, &mut h, A0, v); }
                if t {
                    l = 0x43_ffb0;
                    continue;
                }
                let t = (r[V0] as i64) >= 0;
                { let v = qmfc2(c, 7); setq(&mut r, &mut h, V0, v); }
                if t {
                    l = 0x43_ffb0;
                    continue;
                }
                { let v = qmfc2(c, 8); setq(&mut r, &mut h, V1, v); }
                r[A0] ^= r[A1];
                let t = (r[V0] as i64) < 0;
                r[V0] <<= 32;
                if t {
                    l = 0x43_ffb0;
                    continue;
                }
                let t = (r[V0] as i64) < 0;
                r[V1] |= r[A0];
                if t {
                    l = 0x43_ffb0;
                    continue;
                }
                uycpu::vu0::ftoi::<0, 4>(c, 3, 3);
                let t = (r[V1] as i64) < 0;
                r[V1] <<= 32;
                if !t && (r[V1] as i64) >= 0 {
                    r[T2] = 0;
                }
                { let v = qmfc2(c, 3); setq(&mut r, &mut h, A0, v); }
                r[A0] = ((r[A0] as i64) >> 33) as u64;
                r[V1] = 0x1000;
                { let v = uycpu::mmi::pminw(rq(&r, &h, V1), rq(&r, &h, A0)); setq(&mut r, &mut h, V1, v); }
                let t5 = r[T5] as u32;
                c.wr16(t5 + 6, r[T2] as u16);
                c.wr16(t5 + 4, r[V1] as u16);
                r[T5] = add32(r[T5], 0x10);
                { let v = uycpu::mmi::pcpyld(rq(&r, &h, T5), rq(&r, &h, S5)); setq(&mut r, &mut h, S5, v); }
                l = 0x43_ee70;
            }
            0x43_ff80 => {
                lqc2(c, 7, r[FP] as u32 + 0xe0);
                r[V0] = 0x30;
                vop::<1, 15>(c, 7, 0, c.vf[7]);
                sqc2(c, 7, SPR + 0x3330);
                l = 0x43_fe40;
            }
            0x43_ff98 => {
                let t5 = r[T5] as u32;
                let t = r[S3] == 0;
                c.wr16(t5 + 6, 0);
                if t {
                    l = 0x43_ee70;
                    continue;
                }
                c.wr16(t5 + 4, 0);
                r[T5] = add32(r[T5], 0x10);
                { let v = uycpu::mmi::pcpyld(rq(&r, &h, T5), rq(&r, &h, S5)); setq(&mut r, &mut h, S5, v); }
                l = 0x43_ee70;
            }
            0x43_ffb0 => {
                let t = r[S3] == 0;
                r[AT] = c.rd16(r[FP] as u32 + 0x32) as i16 as i64 as u64;
                r[AT] = far_distance(r[AT], far_units);
                if t {
                    l = 0x43_ee70;
                    continue;
                }
                r[A2] &= 0x800;
                let t = r[A2] != 0;
                r[AT] <<= 42;
                if t {
                    l = 0x43_ff98;
                    continue;
                }
                r[AT] = r[AT].wrapping_sub(r[T3]);
                qmtc2(c, 1, rq(&r, &h, T3));
                qmtc2(c, 3, rq(&r, &h, AT));
                l = 0x43_efa0;
            }
            0x43_ffd8 => {
                // +0x40 negative: the moby's class textures into the texture list (0x437C88), a NEXT tag for them in the chain.
                r[AT] = sx(c.rd32(SPR + 0x3e00));
                r[V0] = sx(c.rd32(SPR + 0x3e04));
                r[T3] = sx((r[T3] as u32) >> 4);
                let t = r[AT] == r[V0];
                r[T3] &= 7;
                if t {
                    l = 0x43_f660;
                    continue;
                }
                r[T3] |= r[FP];
                let at = r[AT] as u32;
                c.wr32(at, r[SP] as u32);
                c.wr32(at + 4, r[T3] as u32);
                r[AT] = add32(r[AT], 8);
                let sp = r[SP] as u32;
                c.wr128(sp, 0, 0);
                r[V0] = 0x1000_0000;
                c.wr32(sp, 0x1000_0000);
                r[SP] = add32(r[SP], 0x10);
                c.wr32(SPR + 0x3e00, r[AT] as u32);
                l = 0x43_f660;
            }
            0x44_0018 => {
                // +0x34 bit 4: the moby's own colour (+0x78) into the light section, the alpha kept.
                let t8 = r[T8] as u32;
                r[A2] = sx(c.rd32(r[FP] as u32 + 0x78));
                r[A3] = c.rd8(t8 - 4) as u64;
                c.wr32(t8 - 0x14, r[A2] as u32);
                c.wr8(t8 - 0x11, r[A3] as u8);
                l = 0x43_f668;
            }
            0x44_0038 => {
                // +0x63: the second group's alpha, faded by depth from the class's +6 (the far fade).
                r[V1] = sx((r[T7] as u32) << 10);
                r[T7] = c.rd8(r[S1] as u32 + 6) as u64;
                r[A0] = sx(r[T0] as u32);
                r[A1] = c.rd8(r[FP] as u32 + 0x23) as u64;
                let t = (r[T7] as i64) <= 0;
                r[V1] = sx((r[V1] as u32).wrapping_sub(r[A0] as u32));
                if !t {
                    let t = (r[V1] as i64) <= 0;
                    r[T7] = 0;
                    if !t {
                        r[V1] = sx((r[V1] as u32) >> 7);
                        r[A0] = 0x80;
                        { let v = uycpu::mmi::pminw(rq(&r, &h, V1), rq(&r, &h, A0)); setq(&mut r, &mut h, V1, v); }
                        lo = sx(((r[V1] as i32 as i64) * (r[A1] as i32 as i64)) as i32 as u32);
                        r[T7] = lo;
                        r[T7] = sx((r[T7] as u32) >> 7);
                    }
                }
                l = 0x43_f128;
            }
            0x44_0070 => {
                // The second group (+0x63): the class's second mesh run in the record's own section (a rigid group), with its
                // own rows (the moby's rotation through the light directions) and program 0's constants.
                lqc2(c, 9, r[FP] as u32);
                r[A0] = r[T8] & 0x3ff;
                r[AT] = sx(c.rd32(r[FP] as u32 + 0x24));
                r[A0] = sx((r[T8] as u32).wrapping_sub(r[A0] as u32));
                r[V0] = c.rd8(r[AT] as u32 + 7) as u64;
                r[V1] = c.rd8(r[AT] as u32 + 6) as u64;
                r[T8] = add32(r[T8], 0x10);
                r[S1] = sx(c.rd32(r[AT] as u32));
                r[V0] = sx((r[V0] as u32) << 4);
                r[A1] = add32(r[V1], 0xa);
                r[V1] = sx((r[V1] as u32) << 4);
                vop::<1, 14>(c, 9, 9, c.vf[24]);
                r[A1] = sx((r[A1] as u32) >> 1);
                r[A2] = r[T8] & 0x3ff;
                r[A3] = sx((r[A1] as u32) << 4);
                r[A2] = add32(r[A2], r[A3]);
                r[S1] = add32(r[S1], r[V0]);
                r[A2] = add32(r[A2], 0xffff_fc10);
                r[S0] = add32(r[S1], r[V1]);
                if (r[A2] as i64) >= 0 {
                    // No room in the buffer: the group is dropped.
                    r[AT] = r[T8] & 0x3ff;
                    r[AT] = sx((r[T8] as u32).wrapping_sub(r[AT] as u32));
                    c.wr32(r[AT] as u32 + 0x14, 0);
                    r[T8] = add32(r[T8], 0xffff_fff0);
                    c.wr64(r[T8] as u32, 0);
                    r[AT] = 0x1000_d000;
                    r[T8] = add32(r[T8], 0x10);
                    l = 0x43_f8b8;
                    continue;
                }
                c.wr16(r[A0] as u32 + 0x16, r[A1] as u16);
                r[T8] = sx((r[T8] as u32) >> 4);
                { let v = c.rd128(SPR + 0x3e80); setq(&mut r, &mut h, T2, v); }
                r[T8] = sx((r[T8] as u32) << 4);
                { let v = c.rd128(SPR + 0x3ea0); setq(&mut r, &mut h, T3, v); }
                vop::<6, 14>(c, 5, 9, c.vf[9]);
                { let v = c.rd128(SPR + 0x3eb0); setq(&mut r, &mut h, T4, v); }
                vop::<0, 14>(c, 6, 0, bc(c, 0, 3));
                { let v = c.rd128(SPR + 0x3ec0); setq(&mut r, &mut h, T5, v); }
                { let v = c.rd128(SPR + 0x3ed0); setq(&mut r, &mut h, T6, v); }
                vop::<0, 8>(c, ACC, 5, bc(c, 5, 1));
                vop::<2, 8>(c, 5, 6, bc(c, 5, 2));
                lqc2(c, 1, r[FP] as u32 + 0xc0);
                lqc2(c, 2, r[FP] as u32 + 0xd0);
                lqc2(c, 3, r[FP] as u32 + 0xe0);
                qop::<2>(c, c.vf[0][3], c.vf[5][0]);
                rot3(c, 1, 25, 1);
                rot3(c, 2, 25, 2);
                rot3(c, 3, 25, 3);
                vmove::<15>(c, 4, c.vf[0]);
                vop::<0, 14>(c, 5, 0, c.vf[0]);
                vop::<0, 14>(c, 6, 0, c.vf[0]);
                vop::<0, 14>(c, 7, 0, c.vf[0]);
                vop::<6, 14>(c, 9, 9, qq(c));
                rot3(c, 9, 25, 9);
                vop::<0, 8>(c, 5, 0, bc(c, 0, 3));
                vop::<0, 4>(c, 6, 0, bc(c, 9, 2));
                vop::<1, 4>(c, 7, 0, bc(c, 9, 1));
                vop::<0, 2>(c, 6, 0, bc(c, 9, 1));
                vop::<0, 2>(c, 7, 0, bc(c, 9, 2));
                rot3(c, 1, 5, 1);
                rot3(c, 2, 5, 2);
                rot3(c, 3, 5, 3);
                vop::<0, 8>(c, 5, 0, bc(c, 9, 2));
                vop::<0, 14>(c, 6, 0, c.vf[0]);
                vop::<1, 8>(c, 7, 0, bc(c, 9, 0));
                vop::<0, 2>(c, 5, 0, bc(c, 9, 0));
                vop::<0, 4>(c, 6, 0, bc(c, 0, 3));
                vop::<0, 4>(c, 7, 0, bc(c, 0, 1));
                vop::<0, 2>(c, 7, 0, bc(c, 9, 2));
                rot3(c, 1, 5, 1);
                rot3(c, 2, 5, 2);
                rot3(c, 3, 5, 3);
                let t8 = r[T8] as u32;
                for k in 0..4u32 {
                    sqc2(c, 1 + k as usize, t8 + 16 * k);
                }
                r[T8] = add32(r[T8], 0x40);
                lqc2(c, 1, SPR + 0x3ee0);
                { let v = c.rd128(SPR + 0x3f60); setq(&mut r, &mut h, T1, v); }
                vop::<0, 15>(c, 18, 18, c.vf[1]);
                r[S2] = 2;
                r[S3] = 0x1400_000a;
                loop {
                    let t = (r[S2] as i64) <= 0;
                    r[AT] = 0x8129;
                    if !t {
                        let sp = r[SP] as u32;
                        stq(c, sp, rq(&r, &h, T1));
                        r[S2] = add32(r[S2], 0xffff_ffff);
                        for (n, o) in [(15usize, 0x10u32), (16, 0x20), (17, 0x30), (18, 0x40)] {
                            sqc2(c, n, sp + o);
                        }
                        c.wr16(sp + 0xc, r[AT] as u16);
                        r[SP] = add32(r[SP], 0x50);
                    }
                    let sp = r[SP] as u32;
                    let t8 = r[T8] as u32;
                    r[AT] = sx(c.rd32(r[S1] as u32 + 8));
                    r[V0] = sx(c.rd32(r[S1] as u32 + 0xc));
                    stq(c, sp, rq(&r, &h, T2));
                    r[AT] = sx((r[AT] as u32) << 4);
                    stq(c, sp + 0x10, rq(&r, &h, T3));
                    r[V1] = r[V0] & 0xff;
                    stq(c, sp + 0x20, rq(&r, &h, T4));
                    r[A2] = add32(r[SP], 0x30);
                    r[AT] |= r[V1];
                    r[V1] = sx((r[V0] as u32) >> 24);
                    c.wr32(t8, r[AT] as u32);
                    r[A0] = sx((r[V0] as u32) >> 8);
                    c.wr32(t8 + 4, r[A2] as u32);
                    r[A0] &= 0xff;
                    c.wr8(sp + 0xe, r[V1] as u8);
                    r[A1] = sx((r[V0] as u32) >> 16);
                    c.wr8(sp + 0x1e, r[V1] as u8);
                    r[A1] &= 0xff;
                    c.wr8(sp + 0x2e, r[V1] as u8);
                    c.wr16(sp, r[A0] as u16);
                    r[A0] = sx((r[A0] as u32) << 4);
                    c.wr16(sp + 0x10, r[A1] as u16);
                    c.wr16(sp + 0x20, r[A1] as u16);
                    r[A1] = sx((r[A1] as u32) << 4);
                    r[T8] = add32(r[T8], 8);
                    r[A2] = add32(r[A2], r[A1]);
                    c.wr32(sp + 4, r[A2] as u32);
                    r[A2] = add32(r[A2], r[A0]);
                    c.wr32(sp + 0x14, r[A2] as u32);
                    r[A2] = add32(r[A2], r[A1]);
                    c.wr32(sp + 0x24, r[A2] as u32);
                    r[SP] = add32(r[A2], 0);
                    r[V0] = c.rd64(r[S1] as u32);
                    r[S1] = add32(r[S1], 0x10);
                    stq(c, r[SP] as u32, rq(&r, &h, T5));
                    r[SP] = add32(r[SP], 0x10);
                    let sp = r[SP] as u32;
                    c.wr32(sp - 0xc, r[V0] as u32);
                    r[V1] = r[V0] >> 32;
                    c.wr16(sp - 0x10, r[V1] as u16);
                    stq(c, sp, rq(&r, &h, T6));
                    r[SP] = add32(r[SP], 0x10);
                    let t = r[S1] != r[S0];
                    c.wr16(r[SP] as u32 - 4, r[S3] as u16);
                    if !t {
                        break;
                    }
                }
                c.wr64(r[T8] as u32, 0);
                r[AT] = 0x1000_d000;
                r[T8] = add32(r[T8], 0x10);
                l = 0x43_f8b8;
            }
            0x44_0330 => {
                // +0x7E bit 0: the moby's visibility cells (the bitmaps at scratchpad 0 and 0x400 from the first transfer, the cell
                // lights at 0x800): the lights of the cells it overlaps added to the point lights (0x3400).
                r[A3] = 0xf;
                r[V1] = 0x3a80_0000;
                r[V0] = (SPR + 0x400) as u64;
                qmtc2(c, 2, rq(&r, &h, V1));
                c.sa = 32;
                vop::<6, 14>(c, 3, 1, bc(c, 2, 0));
                r[V1] = SPR as u64;
                uycpu::vu0::ftoi::<0, 15>(c, 4, 3);
                { let v = qmfc2(c, 4); setq(&mut r, &mut h, A1, v); }
                r[A3] = !r[A3];
                { let v = uycpu::mmi::pextlw((0, 0), rq(&r, &h, A1)); setq(&mut r, &mut h, A2, v); }
                r[A1] >>= 32;
                r[A2] &= r[A3];
                r[V1] = add32(r[V1], r[A2]);
                r[A1] &= r[A3];
                { let v = c.rd128(r[V1] as u32); setq(&mut r, &mut h, V1, v); }
                r[V0] = add32(r[V0], r[A1]);
                { let v = c.rd128(r[V0] as u32); setq(&mut r, &mut h, V0, v); }
                r[A1] = (SPR + 0x800) as u64;
                { let v = uycpu::mmi::pand(rq(&r, &h, V0), rq(&r, &h, V1)); setq(&mut r, &mut h, V0, v); }
                { let v = uycpu::mmi::pcpyud(rq(&r, &h, V0), (0, 0)); setq(&mut r, &mut h, A2, v); }
                r[A2] |= r[V0];
                r[V1] = 4;
                if r[A2] != 0 {
                    loop {
                        { let v = uycpu::mmi::pextlw((0, 0), rq(&r, &h, V0)); setq(&mut r, &mut h, A2, v); }
                        let t = r[A2] == 0;
                        r[V1] = add32(r[V1], 0xffff_ffff);
                        if t {
                            r[A1] = add32(r[A1], 0x200);
                        } else {
                            r[A3] = 0x20;
                            loop {
                                r[T0] = r[A2] & 1;
                                if r[T0] != 0 {
                                    r[T0] = c.rd64(r[A1] as u32);
                                    vop::<0, 4>(c, 2, 0, bc(c, 0, 3));
                                    { let v = uycpu::mmi::pextlh((0, 0), rq(&r, &h, T0)); setq(&mut r, &mut h, T0, v); }
                                    { let v = uycpu::mmi::psllw(rq(&r, &h, T0), 6); setq(&mut r, &mut h, T0, v); }
                                    qmtc2(c, 4, rq(&r, &h, T0));
                                    itof::<2, 15>(c, 4, 4);
                                    qop::<0>(c, c.vf[0][3], c.vf[4][3]);
                                    vop::<1, 15>(c, 5, 4, c.vf[3]);
                                    vop::<6, 14>(c, 5, 5, c.vf[5]);
                                    vop::<6, 1>(c, 6, 4, c.vf[4]);
                                    vop::<0, 4>(c, ACC, 5, bc(c, 5, 0));
                                    vop::<2, 4>(c, ACC, 2, bc(c, 5, 2));
                                    vop::<3, 4>(c, 5, 2, bc(c, 6, 3));
                                    vop::<2, 4>(c, 6, 0, bc(c, 0, 0));
                                    vop::<6, 1>(c, 2, 0, qq(c));
                                    qop::<1>(c, c.vf[0][0], c.vf[6][1]);
                                    { let v = qmfc2(c, 5); setq(&mut r, &mut h, T0, v); }
                                    let t = (r[T0] as i64) > 0;
                                    vop::<1, 14>(c, 5, 3, c.vf[4]);
                                    if !t {
                                        r[T0] = c.rd64(r[A1] as u32 + 8);
                                        { let v = uycpu::mmi::pextlh((0, 0), rq(&r, &h, T0)); setq(&mut r, &mut h, T0, v); }
                                        qmtc2(c, 9, rq(&r, &h, T0));
                                        vop::<6, 1>(c, 5, 0, qq(c));
                                        vop::<6, 1>(c, 2, 2, qq(c));
                                        qop::<0>(c, c.vf[0][3], c.vf[5][3]);
                                        itof::<2, 15>(c, 9, 9);
                                        vop::<1, 1>(c, 2, 0, c.vf[2]);
                                        vop::<6, 14>(c, ACC, 12, bc(c, 5, 0));
                                        vop::<2, 14>(c, ACC, 13, bc(c, 5, 1));
                                        vop::<2, 14>(c, 5, 14, bc(c, 5, 2));
                                        vop::<6, 14>(c, 9, 9, bc(c, 2, 3));
                                        sqc2(c, 9, r[AT] as u32);
                                        vop::<6, 14>(c, 5, 5, qq(c));
                                        r[A0] = add32(r[A0], 1);
                                        sqc2(c, 5, r[AT] as u32 + 0x10);
                                        r[AT] = add32(r[AT], 0x20);
                                    }
                                }
                                r[A3] = add32(r[A3], 0xffff_ffff);
                                r[A2] = sx((r[A2] as u32) >> 1);
                                let t = r[A2] != 0;
                                r[A1] = add32(r[A1], 0x10);
                                if !t {
                                    break;
                                }
                            }
                            r[A3] = sx((r[A3] as u32) << 4);
                            r[A1] = add32(r[A1], r[A3]);
                        }
                        let t = r[V1] != 0;
                        { let v = uycpu::mmi::qfsrv(rq(&r, &h, V0), (0, 0), c.sa); setq(&mut r, &mut h, V0, v); }
                        if !t {
                            break;
                        }
                    }
                }
                l = 0x43_f568;
            }
            EXTRAS_START => {
                // DRAW DISTANCE FAR's extras' pass (`extras`), after the game's own passes: the near list, its end (S5's high
                // half), the second pass's cursor and RA kept as those passes left them; the pass draws with the first pass's RA
                // (a record's next block is 0x43EE70) and the near list's slot 0 for a near-flag extra.
                near_saved = (0..128).map(|i| c.rd128(SPR + 0x1000 + 16 * i)).collect();
                near_end_saved = (r[S5], h[S5]);
                cursor_saved = c.rd32(SPR + 0x3ef0);
                ra_saved = r[RA];
                h[S5] = sx(SPR + 0x1000);
                c.wr32(SPR + 0x3ef0, SPR + 0x1000);
                r[RA] = ra_first;
                extra_pass = Some(0);
                l = 0x43_ee70;
            }
            EXTRAS_END => {
                // The extras' pass ends (every extra culled or drawn, or the chain full): the game's passes' near list, its end,
                // the cursor and RA back, for the cull's end (0x43FA90) and the caller.
                far_log.cut = extras.len() - extra_pass.expect("the extras' pass ends only while it runs").min(extras.len());
                for (i, q) in near_saved.iter().enumerate() {
                    c.wr128(SPR + 0x1000 + 16 * i as u32, q.0, q.1);
                }
                (r[S5], h[S5]) = near_end_saved;
                c.wr32(SPR + 0x3ef0, cursor_saved);
                r[RA] = ra_saved;
                l = 0x43_fa90;
            }
            _ => unreachable!("0x43EA78: no block at {l:08x}"),
        }
    }
    let _ = (vp, lo);
    // s0 to s7, fp, sp and ra come back from the scratchpad as the caller had them; their high halves as the function left them.
    // So does v0's (PCPYUD v0, s5, zero at 0x43FA90: 0), which reaches the first record's skin job (VU0's quadword 255).
    for n in [V0, S0, S1, S2, S3, S4, S5, S6, S7, SP, FP, RA] {
        c.rh[n] = h[n];
    }
    add32(r[T9], 0x10) as u32
}

/// UYA_FAR_LOG: what DRAW DISTANCE FAR's culls did, summed over each 300 of them (`FarLog::end`): the extra mobies left for the
/// extras' pass (`f43ea78`'s `extras`), the records that pass made, the extras it never reached (the chain full), and the
/// fullest job list (its bytes against the room up to the second list, `room`) and moby chain (against gp+0x483C's end).
#[derive(Default)]
struct FarLog {
    drawn: u32,
    cut: usize,
}

static FAR_LOG_ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
static FAR_LOG: std::sync::Mutex<[u64; 8]> = std::sync::Mutex::new([0; 8]);

impl FarLog {
    /// The cull's end: `extras` left for the extras' pass; the job list `job` bytes of `room`, the chain `chain` of `chain_room`.
    fn end(&self, extras: usize, job: u32, room: u32, chain: u32, chain_room: u32) {
        if !*FAR_LOG_ON.get_or_init(|| std::env::var_os("UYA_FAR_LOG").is_some()) {
            return;
        }
        let mut s = FAR_LOG.lock().expect("far log lock");
        let [calls, deferred, drawn, cut, job_max, room_min, chain_max, chain_room_min] = &mut *s;
        *calls += 1;
        *deferred += extras as u64;
        *drawn += self.drawn as u64;
        *cut += self.cut as u64;
        *job_max = (*job_max).max(job as u64);
        *room_min = if *room_min == 0 { room as u64 } else { (*room_min).min(room as u64) };
        *chain_max = (*chain_max).max(chain as u64);
        *chain_room_min = if *chain_room_min == 0 { chain_room as u64 } else { (*chain_room_min).min(chain_room as u64) };
        if *calls == 300 {
            println!("far: 300 culls: {deferred} extra mobies left for the extras' pass, {drawn} records made in it, {cut} never reached (the chain full); the fullest job list {job_max} of {room_min} bytes, moby chain {chain_max} of {chain_room_min}");
            *s = [0; 8];
        }
    }
}

/// `f43ea78`'s own blocks for DRAW DISTANCE FAR's extras' pass: no address of the game's (its blocks are at 0x43xxxx and 0x44xxxx).
const EXTRAS_START: u32 = 1;
const EXTRAS_END: u32 = 2;

/// A moby's draw distance (its +0x32 as the near list's fades read it, sign-extended) with DRAW DISTANCE FAR's reach in whole
/// units (`far`): at least the reach when it is above 0, as the far test's limit is (the module doc); itself at ORIGINAL.
#[inline(always)]
fn far_distance(d: u64, far: Option<i64>) -> u64 {
    match far {
        Some(f) if (d as i64) > 0 => (d as i64).max(f) as u64,
        _ => d,
    }
}

/// The drawn byte (moby +0x31, `flag`; `was` the byte as the cull read it) of a moby that passed the cull's tests, as the
/// game's own cull leaves it: set when it was clear and the game would draw the moby, and where the game would not (outside
/// its own view when the view is widened, `wide`: `uycpu::Cpu::in_original_view`, uyatr's `DRAWN`; past its own far limit,
/// `game_far` false, with DRAW DISTANCE FAR) what the culled path makes it: cleared, unless it was clear or the cull's mode
/// (ra's high half, positive) keeps it. With the game's own view and ORIGINAL, the byte as the translation leaves it.
fn drawn(c: &mut Cpu, flag: u32, was: u64, ra_hi: u64, wide: bool, game_far: bool) {
    let seen = (!wide || c.in_original_view()) && game_far;
    if !seen && was as u8 != 0 && ra_hi as i64 <= 0 {
        c.wr8(flag, 0);
    }
    if was as u8 == 0 && seen {
        c.wr8(flag, 1);
    }
}

// ---- 0x437B48's second list: 0x4A0B88 (with the pose reader 0x43C028) and 0x4A0830's packets ------------------------------
//
// Hand-written and compiled functions with their own register conventions (0x43C028 keeps the caller's t8 and t9, 0x4A0B88
// parks s0 to s7 and ra in their high halves, 0x4A0FE8 and 0x4A19A0 park ra there, 0x4A1C40 parks a0 and ra): they share one
// register file, as on the EE, and each is its code's statements in order. Their 32-bit stores go through `dw` (a store to
// SPR_TO's or SPR_FROM's registers is the register; CHCR with STR makes the transfer, as the port's kick would).

/// A lane in every lane.
#[inline(always)]
fn b4(x: u32) -> [u32; 4] {
    [x; 4]
}

/// A 32-bit store, the scratchpad channels' registers by their meaning.
#[inline(always)]
fn dw(c: &mut Cpu, a: u32, v: u32) {
    if a & 0xffff_fb00 == 0x1000_d000 {
        dma_reg(c, a, v)
    } else {
        c.wr32(a, v)
    }
}

/// A store to SPR_FROM's (0x1000D000) or SPR_TO's (0x1000D400) registers: kept; CHCR with STR makes the transfer.
#[cold]
fn dma_reg(c: &mut Cpu, a: u32, v: u32) {
    let ch = if a & 0x400 != 0 { 9 } else { 8 };
    let reg = ((a & 0xff) >> 4) as usize;
    if reg != 0 || v & 0x100 == 0 {
        c.mach().hw.ch[ch][reg] = v;
        return;
    }
    let h = c.mach().hw.ch[ch];
    match (ch, (v >> 2) & 3) {
        (9, 0) => spr_to(c, v, h[1], h[2], h[8]),
        (9, 1) => spr_to_chain(c, v, h[3], h[8]),
        (8, 0) => spr_from(c, v, h[1], h[2], h[8]),
        (_, mode) => panic!("the moby draw path's DMA on channel {ch} in mode {mode} (CHCR {v:#x}): the unit makes normal and chain transfers only"),
    }
}

/// 0x4C1FE8 with its arguments in the register file.
fn f004c1fe8(c: &mut Cpu, k: F, r: &mut [u64; 32], _h: &mut [u64; 32]) {
    f4c1fe8(c, k, r[A0] as u32, r[A1]);
}

/// The clip-plane routines at 0x4A17B4 (12 bytes each, 0x4A16F8's JALR): the planes' distances of the edge's two ends, w plus
/// or minus z, y, x.
fn clip_plane(c: &mut Cpu, n: u32) {
    let lane = [2usize, 2, 1, 1, 0, 0][n as usize];
    if n % 2 == 0 {
        vop::<0, 1>(c, 4, 1, bc(c, 1, lane));
        vop::<0, 1>(c, 5, 2, bc(c, 2, lane));
    } else {
        vop::<1, 1>(c, 4, 1, bc(c, 1, lane));
        vop::<1, 1>(c, 5, 2, bc(c, 2, lane));
    }
}

/// 0x4A16E8: 0x4A16F8 from 0x4A1704 against two planes (the routines from 0x4A17C0, 16 the planes' flag).
fn f004a16e8(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
    r[T0] = sx(k.a(c, 0x4a_16e8, 0x4a_16ec));
    r[A2] = 16;
    f004a16f8_at(c, F { at: k.pc(0x4a_16f8), l1: 0x4a_16f8 }, r, h, 0x4a_1704);
}

/// The states of 0x4A1C40's near-plane clipper (its JALR to a3): which ends of the last edge were in front.
#[derive(Clone, Copy)]
enum Clip {
    S1e3c,
    S1e74,
    S1eac,
    S1edc,
    S1f14,
    S1f4c,
}

/// One of the clipper's emitters (0x4A1E60, 0x4A1E98, 0x4A1EC8, 0x4A1F00, 0x4A1F38, 0x4A1F68): the point where the edge
/// between the ends `a` and `b` meets the plane, w plus z of each as the weights, out at t5.
fn clip_emit(c: &mut Cpu, r: &mut [u64; 32], e: u32) {
    let (wa, wb, m, s) = match e {
        0x4a_1e60 => (9, 2, 2, 9),
        0x4a_1e98 => (8, 1, 8, 1),
        0x4a_1ec8 => (1, 2, 1, 2),
        0x4a_1f00 => (9, 2, 9, 2),
        0x4a_1f38 => (8, 1, 1, 8),
        _ => (1, 2, 2, 1),
    };
    // vf4.w and vf5.w: the two weights (each code's order); ACC = m * (the other's weight); vf6 = ACC - s * (its own).
    match e {
        0x4a_1e60 | 0x4a_1f38 | 0x4a_1f68 => {
            vop::<0, 1>(c, 4, wa, bc(c, wa, 2));
            vop::<0, 1>(c, 5, wb, bc(c, wb, 2));
            vop::<6, 15>(c, ACC, m, bc(c, 4, 3));
            vop::<3, 15>(c, 6, s, bc(c, 5, 3));
        }
        _ => {
            vop::<0, 1>(c, 5, wb, bc(c, wb, 2));
            vop::<0, 1>(c, 4, wa, bc(c, wa, 2));
            vop::<6, 15>(c, ACC, m, bc(c, 5, 3));
            vop::<3, 15>(c, 6, s, bc(c, 4, 3));
        }
    }
    r[T5] = add32(r[T5], 0x10);
    sqc2(c, 6, r[T5] as u32 - 0x10);
}

/// 0x4A1C40: the shadow's strip (gp+0x4580 pairs at the scratchpad's 0, in front of the near plane per CLIP's z-): each
/// edge through the clipper's states, then 0x4A16E8 against the side planes and the perspective divide into the chain.
fn f004a1c40(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
    let gp = r[28] as u32;
    r[T3] = sx(c.rd32(k.gp(c, gp, 0x4a_1c40)));
    r[T2] = SPR as u64;
    r[T7] = sx(c.rd32(k.gp(c, gp, 0x4a_1c48)));
    lqc2(c, 1, SPR);
    { let v = uycpu::mmi::pcpyld(rq(r, h, RA), (0, 0)); setq(r, h, RA, v); }
    lqc2(c, 2, SPR + 0x10);
    r[T3] = add32(r[T3], 0xffff_ffff);
    uycpu::vu0::clip(c, 1, 1);
    let t = (r[T3] as i64) <= 0;
    uycpu::vu0::clip(c, 2, 2);
    if !t {
        r[T5] = add32(r[T7], 0x30);
        vmac::<1, 1>(c, 0, 1, c.vf[2]);
        { let v = uycpu::mmi::pcpyld(rq(r, h, A0), (0, 0)); setq(r, h, A0, v); }
        r[T2] = add32(r[T2], 0x20);
        r[V0] = sx(c.cfc2(18));
        r[V1] = sx(c.cfc2(17));
        r[AT] = r[V0] & 0x800;
        r[V0] &= 0x20;
        let mut s = if r[AT] != 0 {
            if r[V0] == 0 {
                Clip::S1eac
            } else if r[V1] & 0x10 == 0 {
                Clip::S1edc
            } else {
                Clip::S1e74
            }
        } else if r[V0] != 0 {
            Clip::S1f4c
        } else if r[V1] & 0x10 != 0 {
            Clip::S1e3c
        } else {
            Clip::S1f14
        };
        loop {
            lqc2(c, 4, r[T2] as u32);
            lqc2(c, 5, r[T2] as u32 + 0x10);
            r[T2] = add32(r[T2], 0x20);
            uycpu::vu0::clip(c, 4, 4);
            uycpu::vu0::clip(c, 5, 5);
            vmove::<15>(c, 8, c.vf[1]);
            vmove::<15>(c, 9, c.vf[2]);
            vmove::<15>(c, 1, c.vf[4]);
            vmove::<15>(c, 2, c.vf[5]);
            r[V0] = sx(c.cfc2(18));
            r[T3] = add32(r[T3], 0xffff_ffff);
            r[AT] = r[V0] & 0x800;
            r[V0] &= 0x20;
            r[A2] = k.pc(0x4a_1d40) as u64;
            let (at, v0) = (r[AT] != 0, r[V0] != 0);
            let e = |c: &mut Cpu, r: &mut [u64; 32], x: u32| clip_emit(c, r, x);
            s = match s {
                Clip::S1e3c if !at => s,
                Clip::S1e3c => {
                    e(c, r, 0x4a_1f38);
                    if !v0 {
                        e(c, r, 0x4a_1ec8);
                        Clip::S1eac
                    } else {
                        e(c, r, 0x4a_1e60);
                        Clip::S1e74
                    }
                }
                Clip::S1e74 if v0 => s,
                Clip::S1e74 => {
                    e(c, r, 0x4a_1f00);
                    if at {
                        e(c, r, 0x4a_1ec8);
                        Clip::S1eac
                    } else {
                        e(c, r, 0x4a_1e98);
                        Clip::S1e3c
                    }
                }
                Clip::S1eac => {
                    if !at {
                        e(c, r, 0x4a_1e98);
                        Clip::S1e3c
                    } else if v0 {
                        e(c, r, 0x4a_1e60);
                        Clip::S1e74
                    } else {
                        e(c, r, 0x4a_1ec8);
                        Clip::S1eac
                    }
                }
                Clip::S1edc if at => s,
                Clip::S1edc => {
                    e(c, r, 0x4a_1e98);
                    if v0 {
                        e(c, r, 0x4a_1f68);
                        Clip::S1f4c
                    } else {
                        e(c, r, 0x4a_1f00);
                        Clip::S1f14
                    }
                }
                Clip::S1f14 if !v0 => s,
                Clip::S1f14 => {
                    e(c, r, 0x4a_1e60);
                    if !at {
                        e(c, r, 0x4a_1f68);
                        Clip::S1f4c
                    } else {
                        e(c, r, 0x4a_1f38);
                        Clip::S1edc
                    }
                }
                Clip::S1f4c => {
                    if at {
                        e(c, r, 0x4a_1f38);
                        Clip::S1edc
                    } else if !v0 {
                        e(c, r, 0x4a_1f00);
                        Clip::S1f14
                    } else {
                        e(c, r, 0x4a_1f68);
                        Clip::S1f4c
                    }
                }
            };
            if r[T3] == 0 {
                break;
            }
        }
        r[A0] = add32(r[T7], 0x30);
        r[A1] = sx((r[T5] as u32).wrapping_sub(r[A0] as u32) >> 4);
        r[AT] = add32(r[A1], 0xffff_fffd);
        if (r[AT] as i64) >= 0 {
            r[RA] = k.pc(0x4a_1d68) as u64;
            let k2 = k.call(c, 0x4a_1d60, 0x4a_16e8);
            f004a16e8(c, k2, r, h);
            r[AT] = add32(r[V0], 0xffff_fffd);
            if (r[AT] as i64) >= 0 {
                let t7 = r[T7] as u32;
                c.wr32(t7, 1);
                c.wr64(t7 + 8, 1);
                c.wr32(t7 + 4, 0x2400_0000);
                { let v = uycpu::mmi::pcpyud(rq(r, h, A0), (0, 0)); setq(r, h, A0, v); }
                c.wr32(t7 + 0x10, r[A0] as u32);
                c.wr32(t7 + 0x14, 0x4400_0000);
                c.wr64(t7 + 0x18, 0x55);
                c.wr32(t7 + 0x20, r[V0] as u32);
                c.wr32(t7 + 0x24, 0x2400_0000);
                c.wr64(t7 + 0x28, 0x52);
                let view = k.a(c, 0x4a_1db4, 0x4a_1db8);
                lqc2(c, 28, view + 0x190);
                lqc2(c, 29, view + 0x1a0);
                let a = k.gp(c, gp, 0x4a_1dc4);
                lqc2(c, 18, a);
                r[AT] = 0x493f_fff8;
                { let v = uycpu::mmi::pextlw(rq(r, h, AT), rq(r, h, AT)); setq(r, h, AT, v); }
                qmtc2(c, 31, rq(r, h, AT));
                r[T5] = add32(r[T7], 0x30);
                loop {
                    lqc2(c, 1, r[T5] as u32);
                    qop::<0>(c, c.vf[0][3], c.vf[1][3]);
                    vop::<6, 14>(c, 1, 28, c.vf[1]);
                    r[T5] = add32(r[T5], 0x10);
                    r[V0] = add32(r[V0], 0xffff_ffff);
                    vop::<6, 14>(c, ACC, 29, bc(c, 0, 3));
                    vop::<2, 14>(c, ACC, 1, qq(c));
                    vop::<2, 14>(c, 1, 18, bc(c, 0, 3));
                    vop::<2, 12>(c, 2, 31, bc(c, 0, 3));
                    uycpu::vu0::ftoi::<0, 2>(c, 3, 1);
                    { let v = qmfc2(c, 2); setq(r, h, T0, v); }
                    sqc2(c, 1, r[T5] as u32 - 0x10);
                    { let v = qmfc2(c, 3); setq(r, h, T1, v); }
                    { let v = uycpu::mmi::pexch(rq(r, h, T0)); setq(r, h, T0, v); }
                    { let v = uycpu::mmi::pcpyud(rq(r, h, T1), (0, 0)); setq(r, h, T1, v); }
                    c.wr32(r[T5] as u32 - 8, r[T0] as u32);
                    c.wr32(r[T5] as u32 - 4, r[T1] as u32);
                    if r[V0] == 0 {
                        break;
                    }
                }
                let a = k.gp(c, gp, 0x4a_1e2c);
                c.wr32(a, r[T5] as u32);
            }
        }
    }
    { let v = uycpu::mmi::pcpyud(rq(r, h, RA), (0, 0)); setq(r, h, RA, v); }
}

// The second list's functions, each its translation's statements in order on the shared register file.
#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a0b88(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a0b88;
loop { match l {
0x004a0b88 => {
{ let (a, b) = uycpu::mmi::pcpyld((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
r[23] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a0bac))) as i32 as i64 as u64;
r[21] = r[4].wrapping_add(0u64);
r[19] = r[5].wrapping_add(0u64);
r[20] = sx(k.a(c, 0x4a0bb8, 0x4a0bbc).wrapping_sub(0xfffff0e0));
r[20] = (((r[20] as u32).wrapping_add(0xfffff0e0)) as i32 as i64 as u64);
r[22] = sx(k.a(c, 0x4a0bc0, 0x4a0bc4).wrapping_sub(0xf0));
r[22] = (((r[22] as u32).wrapping_add(0x000000f0)) as i32 as i64 as u64);
r[19] = (((r[21] as u32).wrapping_add((r[19] as u32))) as i32 as i64 as u64);
r[19] = (((r[19] as u32).wrapping_add(0xfffff000)) as i32 as i64 as u64);
l = 0x004a0bd0; continue; }
0x004a0bd0 => {
r[16] = c.rd32((r[20] as u32)) as i32 as i64 as u64;
r[23] = (((r[23] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[17] = c.rd32((r[16] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
r[6] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[4] = (((r[16] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[5] = c.rd8((r[17] as u32).wrapping_add(0x00000008)) as u64;
r[31] = k.pc(0x4a0bf0) as u64;  { let k2 = k.call(c, 0x4a0be8, 0x43c028); f0043c028(c, k2, r, h); }
r[1] = c.rd32((r[16] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
r[2] = 0x000000003a800000;
r[3] = c.rd16((r[20] as u32).wrapping_add(0x00000004)) as u64;
r[4] = c.rd16((r[20] as u32).wrapping_add(0x00000006)) as u64;
r[20] = (((r[20] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
c.vf[10] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[1] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
c.vf[11] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
vop::<6, 8>(c, 10, 10, c.vf[1]);
uycpu::vu0::itof::<2, 8>(c, 11, 11);
r[2] = c.rd8((r[17] as u32).wrapping_add(0x0000000f)) as u64;
r[1] = c.rd32((r[17] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
r[2] = (((r[2] as u32) << 4) as i32 as i64 as u64);
r[15] = (((r[1] as u32).wrapping_sub((r[2] as u32))) as i32 as i64 as u64);
vop::<6, 8>(c, 10, 10, c.vf[11]);
{ let (a, b) = c.rd128((r[16] as u32).wrapping_add(0x000000c0)); c.vf[12] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[16] as u32).wrapping_add(0x000000d0)); c.vf[13] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[16] as u32).wrapping_add(0x000000e0)); c.vf[14] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = (((r[21] as u32).wrapping_sub((r[19] as u32))) as i32 as i64 as u64);
let t = (r[1] as i64) > 0;  { let (a, b) = c.rd128((r[16] as u32).wrapping_add(0x00000010)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x004a0e70; continue; }
r[1] = c.rd8((r[16] as u32).wrapping_add(0x000000bd)) as i8 as i64 as u64;
r[14] = (((r[21] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[10] = c.rd8((r[16] as u32).wrapping_add(0x000000bf)) as u64;
vop::<6, 14>(c, 1, 14, c.vf[15]);
r[9] = 0u64 | 0x1;
vop::<0, 14>(c, 2, 0, b4(c.vf[0][3]));
r[2] = c.rd32((r[16] as u32).wrapping_add(0x00000070)) as i32 as i64 as u64;
c.vf[3] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
vop::<0, 8>(c, ACC, 1, b4(c.vf[1][1]));
vop::<2, 8>(c, ACC, 2, b4(c.vf[1][2]));
vop::<2, 8>(c, 1, 2, c.vf[3]);
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[3] = c.rd32((r[16] as u32).wrapping_add(0x00000074)) as i32 as i64 as u64;
r[13] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let v = c.vf[14]; c.wr128((r[21] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[25] = 0x0000000070000000;
let t = (r[1] as i64) < 0;  r[1] = (((r[1] as u32).wrapping_add((r[22] as u32))) as i32 as i64 as u64);
 if t { l = 0x004a0e58; continue; }
{ let (a, b) = c.rd128((r[1] as u32)); r[1] = a; h[1] = b; }
l = 0x004a0c98; continue; }
0x004a0c98 => {
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
dw(c, (r[21] as u32).wrapping_add(0x00000004), r[4] as u32);
r[12] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
dw(c, (r[21] as u32).wrapping_add(0x00000008), r[2] as u32);
dw(c, (r[21] as u32).wrapping_add(0x0000000c), r[3] as u32);
c.wr128((r[21] as u32).wrapping_add(0x00000020), r[1], h[1]);
r[21] = (((r[21] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
l = 0x004a0cb8; continue; }
0x004a0cb8 => {
let t = r[12] != 0u64;  r[11] = c.rd8((r[15] as u32)) as u64;
 if t { l = 0x004a0e68; continue; }
r[12] = c.rd8((r[15] as u32).wrapping_add(0x00000001)) as u64;
r[1] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = r[11] != r[1];  r[2] = r[9] & r[10];
 if t { l = 0x004a0d48; continue; }
r[1] = c.rd32((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[9] = (((r[9] as u32) << 1) as i32 as i64 as u64);
let t = (r[2] as i64) > 0;  r[2] = c.rd32((r[15] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
 if t { l = 0x004a0d40; continue; }
r[1] = (((r[1] as u32) << 6) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0x00000010)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = (((r[1] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[13] = (((r[13] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000020)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000030)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, ACC, 4, b4(c.vf[1][0]));
vop::<2, 14>(c, ACC, 5, b4(c.vf[1][1]));
vop::<2, 14>(c, ACC, 6, b4(c.vf[1][2]));
vop::<2, 14>(c, 1, 7, b4(c.vf[0][3]));
l = 0x004a0d10; continue; }
0x004a0d10 => {
vop::<6, 15>(c, 1, 1, b4(c.vf[10][0]));
vop::<6, 14>(c, ACC, 12, b4(c.vf[1][0]));
vop::<2, 14>(c, ACC, 13, b4(c.vf[1][1]));
vop::<2, 14>(c, ACC, 14, b4(c.vf[1][2]));
vop::<2, 14>(c, 1, 15, b4(c.vf[0][3]));
dw(c, (r[21] as u32), r[11] as u32);
dw(c, (r[21] as u32).wrapping_add(0x00000004), r[2] as u32);
c.wr64((r[21] as u32).wrapping_add(0x00000008), 0u64);
{ let v = c.vf[1]; c.wr128((r[21] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[21] = (((r[21] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
l = 0x004a0d40; continue; }
0x004a0d40 => {
r[15] = (((r[15] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 l = 0x004a0cb8; continue;
}
0x004a0d48 => {
r[1] = c.rd16((r[15] as u32).wrapping_add(0x00000004)) as u64;
r[9] = (((r[9] as u32) << 1) as i32 as i64 as u64);
let t = (r[2] as i64) > 0;  r[2] = c.rd16((r[15] as u32).wrapping_add(0x00000006)) as u64;
 if t { l = 0x004a0e10; continue; }
r[1] = (((r[1] as u32) << 6) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0x00000010)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[2] = (((r[2] as u32) << 6) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0x00000020)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = (((r[1] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000020)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000030)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, ACC, 4, b4(c.vf[1][0]));
vop::<2, 14>(c, ACC, 5, b4(c.vf[1][1]));
vop::<2, 14>(c, ACC, 6, b4(c.vf[1][2]));
vop::<2, 14>(c, 1, 7, b4(c.vf[0][3]));
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000010)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000020)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000030)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, ACC, 4, b4(c.vf[2][0]));
vop::<2, 14>(c, ACC, 5, b4(c.vf[2][1]));
vop::<2, 14>(c, ACC, 6, b4(c.vf[2][2]));
vop::<2, 14>(c, 2, 7, b4(c.vf[0][3]));
vop::<6, 15>(c, 1, 1, b4(c.vf[10][0]));
vop::<6, 15>(c, 2, 2, b4(c.vf[10][0]));
r[3] = c.rd32((r[15] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[4] = c.rd32((r[15] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
vop::<6, 14>(c, ACC, 12, b4(c.vf[1][0]));
vop::<2, 14>(c, ACC, 13, b4(c.vf[1][1]));
vop::<2, 14>(c, ACC, 14, b4(c.vf[1][2]));
vop::<2, 14>(c, 1, 15, b4(c.vf[0][3]));
vop::<6, 14>(c, ACC, 12, b4(c.vf[2][0]));
vop::<2, 14>(c, ACC, 13, b4(c.vf[2][1]));
vop::<2, 14>(c, ACC, 14, b4(c.vf[2][2]));
vop::<2, 14>(c, 2, 15, b4(c.vf[0][3]));
let t = (r[3] as i64) < 0;  r[1] = ((0u32.wrapping_sub((r[3] as u32))) as i32 as i64 as u64);
 if t { l = 0x004a0e18; continue; }
l = 0x004a0de8; continue; }
0x004a0de8 => {
let t = (r[4] as i64) < 0;  r[1] = ((0u32.wrapping_sub((r[4] as u32))) as i32 as i64 as u64);
 if t { l = 0x004a0e38; continue; }
l = 0x004a0df0; continue; }
0x004a0df0 => {
dw(c, (r[21] as u32), r[11] as u32);
dw(c, (r[21] as u32).wrapping_add(0x0000000c), 0u64 as u32);
dw(c, (r[21] as u32).wrapping_add(0x00000004), r[3] as u32);
dw(c, (r[21] as u32).wrapping_add(0x00000008), r[4] as u32);
{ let v = c.vf[1]; c.wr128((r[21] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[2]; c.wr128((r[21] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[21] = (((r[21] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
r[13] = (((r[13] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
l = 0x004a0e10; continue; }
0x004a0e10 => {
r[15] = (((r[15] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
 l = 0x004a0cb8; continue;
}
0x004a0e18 => {
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
c.vf[4] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vop::<1, 14>(c, 3, 1, c.vf[2]);
uycpu::vu0::itof::<2, 8>(c, 4, 4);
vop::<6, 14>(c, 3, 3, b4(c.vf[4][0]));
vop::<0, 14>(c, 1, 1, c.vf[3]);
r[3] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 l = 0x004a0de8; continue;
}
0x004a0e38 => {
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
c.vf[4] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vop::<1, 14>(c, 3, 2, c.vf[1]);
uycpu::vu0::itof::<2, 8>(c, 4, 4);
vop::<6, 14>(c, 3, 3, b4(c.vf[4][0]));
vop::<0, 14>(c, 2, 2, c.vf[3]);
r[4] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 l = 0x004a0df0; continue;
}
0x004a0e58 => {
vop::<1, 15>(c, 2, 0, c.vf[14]);
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
 l = 0x004a0c98; continue;
}
0x004a0e68 => {
let t = (r[23] as i64) > 0;  dw(c, (r[14] as u32), r[13] as u32);
 if t { l = 0x004a0bd0; continue; }
l = 0x004a0e70; continue; }
0x004a0e70 => {
c.wr128((r[21] as u32), 0u64, 0u64);
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f0043c028(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x43c028;
loop { match l {
0x0043c028 => {
r[7] = c.rd32((r[4] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
r[1] = 0x0000000070000000;
c.wr128((r[1] as u32).wrapping_add(0x00003700), r[24], h[24]);
r[10] = r[5].wrapping_add(0u64);
c.wr128((r[1] as u32).wrapping_add(0x00003710), r[25], h[25]);
r[24] = (((r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
dw(c, (r[1] as u32).wrapping_add(0x00003720), r[6] as u32);
r[25] = 0x0000000070000000;
r[12] = c.rd32((r[24] as u32).wrapping_add(0x00000058)) as i32 as i64 as u64;
r[8] = 0x0000000010000000;
r[13] = c.rd32((r[24] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
r[8] = r[8] | 0xd400;
r[11] = c.rd32((r[7] as u32).wrapping_add(0x00000018)) as i32 as i64 as u64;
r[1] = 0u64 | 0x3000;
dw(c, (r[8] as u32).wrapping_add(0x00000080), r[1] as u32);
vop::<0, 14>(c, 5, 0, b4(c.vf[0][3]));
dw(c, (r[8] as u32).wrapping_add(0x00000010), r[11] as u32);
r[15] = (((r[10] as u32) << 4) as i32 as i64 as u64);
dw(c, (r[8] as u32).wrapping_add(0x00000020), r[10] as u32);
r[15] = (((r[15] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
c.wr64((r[15] as u32).wrapping_add(0x00002ff8), 0u64);
r[1] = 0u64 | 0x100;
dw(c, (r[8] as u32), r[1] as u32);
r[12] = (((r[12] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[4] = c.rd16((r[12] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
r[13] = (((r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[9] = c.rd32((r[24] as u32).wrapping_add(0x00000044)) as i32 as i64 as u64;
vop::<6, 1>(c, 5, 0, b4(c.vf[0][0]));
r[7] = c.rd8((r[7] as u32).wrapping_add(0x00000008)) as u64;
r[14] = (((r[4] as u32) << 4) as i32 as i64 as u64);
r[2] = c.rd16((r[12] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
vop::<6, 15>(c, 1, 0, b4(c.vf[0][0]));
r[7] = (((r[7] as u32) << 3) as i32 as i64 as u64);
vop::<6, 15>(c, 2, 0, b4(c.vf[0][0]));
r[14] = (((r[14] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
{ let v = c.vf[0]; uycpu::vu0::mov::<15>(c, 3, [v[1], v[2], v[3], v[0]]); }
r[3] = c.rd16((r[12] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
vop::<0, 8>(c, 1, 1, b4(c.vf[0][3]));
c.wr128((r[14] as u32).wrapping_add(0x00001ff0), 0u64, 0u64);
vop::<0, 4>(c, 2, 2, b4(c.vf[0][3]));
dw(c, (r[25] as u32).wrapping_add(0x00003728), r[7] as u32);
r[5] = (((r[25] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
r[6] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x0043c0d8; continue; }
0x0043c0d8 => {
{ let v = c.vf[5]; c.wr128((r[5] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[5]; c.wr128((r[5] as u32).wrapping_add(0x00000090), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[5] = (((r[5] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((r[5] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[6] = (((r[6] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[6] as i64) > 0;  { let v = c.vf[5]; c.wr128((r[5] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c0d8; continue; }
l = 0x0043c0f8; continue; }
0x0043c0f8 => {
r[1] = c.rd64((r[15] as u32).wrapping_add(0x00002ff8));
r[5] = (((r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
r[6] = 0u64 | 0x100;
let t = r[1] == 0u64;   /* the DMA ended at its kick */
dw(c, (r[8] as u32).wrapping_add(0x00000080), r[5] as u32);
vop::<6, 1>(c, 9, 0, b4(c.vf[0][0]));
dw(c, (r[8] as u32).wrapping_add(0x00000010), r[12] as u32);
vop::<6, 1>(c, 10, 0, b4(c.vf[0][0]));
dw(c, (r[8] as u32).wrapping_add(0x00000020), r[4] as u32);
vop::<6, 1>(c, 11, 0, b4(c.vf[0][0]));
dw(c, (r[8] as u32), r[6] as u32);
vop::<6, 1>(c, 12, 0, b4(c.vf[0][0]));
r[5] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[6] = (((r[25] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
r[7] = (((r[25] as u32).wrapping_add(0xffffff00)) as i32 as i64 as u64);
l = 0x0043c148; continue; }
0x0043c148 => {
{ let (a, b) = c.rd128((r[6] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[7] = (((r[7] as u32).wrapping_add(0x00000100)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[6] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[5] = (((r[5] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[6] as u32).wrapping_add(0x00000020)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, 9, 5, b4(c.vf[0][3]));
{ let (a, b) = c.rd128((r[6] as u32).wrapping_add(0x00000030)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, 10, 6, b4(c.vf[0][3]));
{ let v = c.vf[5]; c.wr128((r[7] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vop::<6, 14>(c, 11, 7, b4(c.vf[0][3]));
{ let v = c.vf[6]; c.wr128((r[7] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vop::<6, 14>(c, 12, 8, b4(c.vf[0][3]));
{ let v = c.vf[7]; c.wr128((r[7] as u32).wrapping_add(0x000000a0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((r[7] as u32).wrapping_add(0x000000e0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[9]; c.wr128((r[7] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[10]; c.wr128((r[7] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[11]; c.wr128((r[7] as u32).wrapping_add(0x000000b0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[6] = (((r[6] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
let t = (r[5] as i64) > 0;  { let v = c.vf[12]; c.wr128((r[7] as u32).wrapping_add(0x000000f0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c148; continue; }
let t = r[9] == 0u64;   if t { l = 0x0043c570; continue; }
r[7] = c.rd8((r[24] as u32).wrapping_add(0x00000042)) as u64;
r[8] = c.rd8((r[24] as u32).wrapping_add(0x00000043)) as u64;
r[5] = c.rd8((r[24] as u32).wrapping_add(0x00000040)) as u64;
r[6] = c.rd8((r[24] as u32).wrapping_add(0x00000041)) as u64;
r[1] = (((r[7] as u32).wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
r[7] = c.rd16((r[13] as u32).wrapping_add(0xfffffff6)) as i16 as i64 as u64;
r[5] = (((r[5] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.vf[4] = [r[9] as u32, (r[9] >> 32) as u32, h[9] as u32, (h[9] >> 32) as u32];
r[5] = (((r[5] as u32).wrapping_sub((r[6] as u32))) as i32 as i64 as u64);
r[1] = r[1] | r[5];
r[5] = c.rd16((r[13] as u32).wrapping_add(0xfffffffa)) as i16 as i64 as u64;
dw(c, (r[25] as u32).wrapping_add(0x00003724), r[1] as u32);
r[15] = (((r[7] as u32) << 4) as i32 as i64 as u64);
r[15] = (((r[15] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[6] = c.rd16((r[13] as u32).wrapping_add(0xfffffffe)) as i16 as i64 as u64;
c.wr128((r[15] as u32).wrapping_add(0x000025f0), 0u64, 0u64);
vop::<1, 1>(c, 4, 0, b4(c.vf[4][0]));
l = 0x0043c1e8; continue; }
0x0043c1e8 => {
r[1] = c.rd64((r[14] as u32).wrapping_add(0x00001ff0));
r[8] = 0x0000000010000000;
r[8] = r[8] | 0xd400;
let t = r[1] == 0u64;   /* the DMA ended at its kick */
dw(c, (r[8] as u32).wrapping_add(0x00000010), r[13] as u32);
r[1] = (((r[25] as u32).wrapping_add(0x00002600)) as i32 as i64 as u64);
dw(c, (r[8] as u32).wrapping_add(0x00000080), r[1] as u32);
r[14] = (((r[25] as u32).wrapping_add(0x00002d00)) as i32 as i64 as u64);
dw(c, (r[8] as u32).wrapping_add(0x00000020), r[7] as u32);
r[1] = 0u64 | 0x100;
dw(c, (r[8] as u32), r[1] as u32);
r[13] = (((r[25] as u32).wrapping_add(0x00002c00)) as i32 as i64 as u64);
r[7] = c.rd32((r[25] as u32).wrapping_add(0x00003728)) as i32 as i64 as u64;
r[1] = (((r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
vop::<6, 15>(c, 5, 0, b4(c.vf[0][0]));
r[1] = (((r[1] as u32).wrapping_add((r[7] as u32))) as i32 as i64 as u64);
r[4] = (((r[2] as u32) << 3) as i32 as i64 as u64);
let t = r[2] == 0u64;  r[4] = (((r[4] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043c2b0; continue; }
r[7] = c.rd64((r[1] as u32));
r[8] = c.rd8((r[1] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 13); r[9] = a; h[9] = b; }
l = 0x0043c258; continue; }
0x0043c258 => {
let t = (r[2] as i64) <= 0;   if t { l = 0x0043c2b0; continue; }
c.vf[6] = [r[9] as u32, (r[9] >> 32) as u32, h[9] as u32, (h[9] >> 32) as u32];
r[9] = (((r[10] as u32).wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
let t = (r[9] as i64) <= 0;  r[12] = r[7] >> 62;
 if t { l = 0x0043c2b0; continue; }
r[11] = (((r[8] as u32) << 6) as i32 as i64 as u64);
r[7] = c.rd64((r[1] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
r[11] = (((r[11] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[1] = (((r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[8] = c.rd8((r[1] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[9] = a; h[9] = b; }
let t = r[12] == 0u64;  { let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 13); r[9] = a; h[9] = b; }
 if t { l = 0x0043c258; continue; }
dw(c, (r[13] as u32), r[11] as u32);
r[13] = (((r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((r[11] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[12] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (r[2] as i64) > 0;  dw(c, (r[11] as u32).wrapping_add(0x0000000c), r[12] as u32);
 if t { l = 0x0043c258; continue; }
l = 0x0043c2b0; continue; }
0x0043c2b0 => {
let t = r[3] == 0u64;  r[7] = c.rd64((r[4] as u32));
 if t { l = 0x0043c318; continue; }
r[8] = c.rd8((r[4] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
l = 0x0043c2c8; continue; }
0x0043c2c8 => {
c.vf[6] = [r[7] as u32, (r[7] >> 32) as u32, h[7] as u32, (h[7] >> 32) as u32];
r[9] = (((r[10] as u32).wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
let t = (r[9] as i64) <= 0;  r[12] = (((r[8] as u32) << 6) as i32 as i64 as u64);
 if t { l = 0x0043c318; continue; }
r[12] = (((r[12] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[7] = c.rd64((r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
r[4] = (((r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[8] = c.rd8((r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[7] = a; h[7] = b; }
dw(c, (r[14] as u32), r[12] as u32);
r[14] = (((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
r[11] = c.rd32((r[12] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
{ let v = c.vf[5]; c.wr128((r[12] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
dw(c, (r[12] as u32).wrapping_add(0x0000002c), r[11] as u32);
r[11] = ((0u32.wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (r[3] as i64) > 0;  dw(c, (r[12] as u32).wrapping_add(0x0000003c), r[11] as u32);
 if t { l = 0x0043c2c8; continue; }
l = 0x0043c318; continue; }
0x0043c318 => {
r[1] = c.rd64((r[15] as u32).wrapping_add(0x000025f0));
r[7] = c.rd32((r[25] as u32).wrapping_add(0x00003728)) as i32 as i64 as u64;
r[4] = (((r[25] as u32).wrapping_add(0x00002600)) as i32 as i64 as u64);
let t = r[1] == 0u64;  r[3] = (((r[5] as u32) << 3) as i32 as i64 as u64);
 /* the DMA ended at its kick */
vop::<6, 15>(c, 5, 0, b4(c.vf[0][0]));
r[4] = (((r[4] as u32).wrapping_add((r[7] as u32))) as i32 as i64 as u64);
let t = r[5] == 0u64;  r[3] = (((r[3] as u32).wrapping_add((r[4] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043c3b0; continue; }
r[7] = c.rd64((r[4] as u32));
r[8] = c.rd8((r[4] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 13); r[9] = a; h[9] = b; }
l = 0x0043c358; continue; }
0x0043c358 => {
let t = (r[5] as i64) <= 0;   if t { l = 0x0043c3b0; continue; }
c.vf[6] = [r[9] as u32, (r[9] >> 32) as u32, h[9] as u32, (h[9] >> 32) as u32];
r[1] = (((r[10] as u32).wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
let t = (r[1] as i64) <= 0;  r[12] = r[7] >> 62;
 if t { l = 0x0043c3b0; continue; }
r[11] = (((r[8] as u32) << 6) as i32 as i64 as u64);
r[7] = c.rd64((r[4] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
r[11] = (((r[11] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[8] = c.rd8((r[4] as u32).wrapping_add(0x0000000e)) as u64;
r[4] = (((r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[9] = a; h[9] = b; }
dw(c, (r[13] as u32), r[11] as u32);
{ let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 13); r[9] = a; h[9] = b; }
r[1] = c.rd32((r[11] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
let t = r[12] == 0u64;  r[5] = (((r[5] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x0043c358; continue; }
{ let v = c.vf[5]; c.wr128((r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[13] = (((r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = r[5] != 0u64;  r[13] = (((r[13] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043c358; continue; }
l = 0x0043c3b0; continue; }
0x0043c3b0 => {
let t = (r[6] as i64) <= 0;  r[7] = c.rd64((r[3] as u32));
 if t { l = 0x0043c410; continue; }
r[8] = c.rd8((r[3] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
l = 0x0043c3c8; continue; }
0x0043c3c8 => {
c.vf[6] = [r[7] as u32, (r[7] >> 32) as u32, h[7] as u32, (h[7] >> 32) as u32];
r[1] = (((r[10] as u32).wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
let t = (r[1] as i64) <= 0;  r[12] = (((r[8] as u32) << 6) as i32 as i64 as u64);
 if t { l = 0x0043c410; continue; }
r[12] = (((r[12] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[7] = c.rd64((r[3] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
r[3] = (((r[3] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[8] = c.rd8((r[3] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[7], h[7]), (0u64, 0u64)); r[7] = a; h[7] = b; }
dw(c, (r[14] as u32), r[12] as u32);
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
r[1] = c.rd32((r[12] as u32).wrapping_add(0x0000003c)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let v = c.vf[5]; c.wr128((r[12] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[6] = (((r[6] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (r[6] as i64) > 0;  r[14] = (((r[14] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043c3c8; continue; }
l = 0x0043c410; continue; }
0x0043c410 => {
r[1] = (((r[25] as u32).wrapping_add(0x00002c00)) as i32 as i64 as u64);
r[2] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
r[3] = (((r[25] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
let t = r[1] == r[13];  dw(c, (r[13] as u32), r[3] as u32);
 if t { l = 0x0043c460; continue; }
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043c430; continue; }
0x0043c430 => {
vop::<6, 14>(c, ACC, 5, b4(c.vf[4][3]));
r[3] = (((r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
vop::<2, 14>(c, 7, 6, b4(c.vf[4][0]));
r[4] = c.rd32((r[2] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
r[2] = c.rd32((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[1] = (((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((r[3] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = r[1] != r[13];  dw(c, (r[3] as u32).wrapping_add(0x0000001c), r[4] as u32);
 if t { l = 0x0043c430; continue; }
l = 0x0043c460; continue; }
0x0043c460 => {
r[1] = (((r[25] as u32).wrapping_add(0x00002d00)) as i32 as i64 as u64);
r[2] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
r[3] = (((r[25] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
let t = r[1] == r[14];  dw(c, (r[14] as u32), r[3] as u32);
 if t { l = 0x0043c4b0; continue; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043c480; continue; }
0x0043c480 => {
vop::<6, 14>(c, ACC, 5, b4(c.vf[4][3]));
r[3] = (((r[2] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
vop::<2, 14>(c, 7, 6, b4(c.vf[4][0]));
r[4] = c.rd32((r[2] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
r[2] = c.rd32((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[1] = (((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((r[3] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = r[1] != r[14];  dw(c, (r[3] as u32).wrapping_add(0x0000002c), r[4] as u32);
 if t { l = 0x0043c480; continue; }
l = 0x0043c4b0; continue; }
0x0043c4b0 => {
r[1] = (((r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
r[2] = (((r[25] as u32).wrapping_add(0x00002600)) as i32 as i64 as u64);
r[7] = c.rd32((r[25] as u32).wrapping_add(0x00003724)) as i32 as i64 as u64;
r[3] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); r[6] = a; h[6] = b; }
r[4] = (((r[25] as u32).wrapping_add(0xffffff80)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[2] as u32)); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[6], h[6]), (0u64, 0u64)); r[5] = a; h[5] = b; }
let t = r[7] != 0u64;  { let (a, b) = uycpu::mmi::psraw((r[5], h[5]), 16); r[5] = a; h[5] = b; }
 if t { l = 0x0043c8a8; continue; }
c.vf[5] = [r[5] as u32, (r[5] >> 32) as u32, h[5] as u32, (h[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[6], h[6]), (0u64, 0u64)); r[6] = a; h[6] = b; }
l = 0x0043c4e0; continue; }
0x0043c4e0 => {
{ let (a, b) = uycpu::mmi::psraw((r[6], h[6]), 16); r[6] = a; h[6] = b; }
c.vf[6] = [r[6] as u32, (r[6] >> 32) as u32, h[6] as u32, (h[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((r[8], h[8]), (0u64, 0u64)); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
c.vf[7] = [r[7] as u32, (r[7] >> 32) as u32, h[7] as u32, (h[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[8], h[8]), (0u64, 0u64)); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[8], h[8]), 16); r[8] = a; h[8] = b; }
c.vf[8] = [r[8] as u32, (r[8] >> 32) as u32, h[8] as u32, (h[8] >> 32) as u32];
r[4] = (((r[4] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 5, 5);
r[1] = (((r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
r[2] = (((r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
uycpu::vu0::itof::<3, 15>(c, 8, 8);
vop::<6, 15>(c, ACC, 5, b4(c.vf[4][3]));
vop::<2, 15>(c, 9, 7, b4(c.vf[4][0]));
vop::<6, 15>(c, ACC, 6, b4(c.vf[4][3]));
{ let (a, b) = c.rd128((r[1] as u32)); r[6] = a; h[6] = b; }
vop::<2, 15>(c, 10, 8, b4(c.vf[4][0]));
{ let (a, b) = c.rd128((r[2] as u32)); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[6], h[6]), (0u64, 0u64)); r[5] = a; h[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[5], h[5]), 16); r[5] = a; h[5] = b; }
c.vf[5] = [r[5] as u32, (r[5] >> 32) as u32, h[5] as u32, (h[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[6], h[6]), (0u64, 0u64)); r[6] = a; h[6] = b; }
{ let v = c.vf[9]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[3] as i64) > 0;  { let v = c.vf[10]; c.wr128((r[4] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c4e0; continue; }
 l = 0x0043c6e8; continue;
}
0x0043c570 => {
r[1] = c.rd64((r[14] as u32).wrapping_add(0x00001ff0));
r[5] = c.rd32((r[25] as u32).wrapping_add(0x00003728)) as i32 as i64 as u64;
r[12] = (((r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
let t = r[1] == 0u64;  r[13] = (((r[2] as u32) << 3) as i32 as i64 as u64);
 /* the DMA ended at its kick */
vop::<6, 15>(c, 5, 0, b4(c.vf[0][0]));
r[12] = (((r[12] as u32).wrapping_add((r[5] as u32))) as i32 as i64 as u64);
let t = r[2] == 0u64;  r[13] = (((r[13] as u32).wrapping_add((r[12] as u32))) as i32 as i64 as u64);
 if t { l = 0x0043c5f8; continue; }
r[5] = c.rd64((r[12] as u32));
r[6] = c.rd8((r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[5], h[5]), (0u64, 0u64)); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[7], h[7]), 13); r[7] = a; h[7] = b; }
l = 0x0043c5b0; continue; }
0x0043c5b0 => {
let t = (r[2] as i64) <= 0;   if t { l = 0x0043c5f8; continue; }
c.vf[6] = [r[7] as u32, (r[7] >> 32) as u32, h[7] as u32, (h[7] >> 32) as u32];
r[1] = (((r[10] as u32).wrapping_sub((r[6] as u32))) as i32 as i64 as u64);
let t = (r[1] as i64) <= 0;  r[9] = r[5] >> 62;
 if t { l = 0x0043c5f8; continue; }
r[8] = (((r[6] as u32) << 6) as i32 as i64 as u64);
r[5] = c.rd64((r[12] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<3, 14>(c, 5, 6);
r[8] = (((r[8] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[12] = (((r[12] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[6] = c.rd8((r[12] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[5], h[5]), (0u64, 0u64)); r[7] = a; h[7] = b; }
let t = r[9] == 0u64;  { let (a, b) = uycpu::mmi::psrlw((r[7], h[7]), 13); r[7] = a; h[7] = b; }
 if t { l = 0x0043c5b0; continue; }
let t = (r[2] as i64) > 0;  { let v = c.vf[5]; c.wr128((r[8] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c5b0; continue; }
l = 0x0043c5f8; continue; }
0x0043c5f8 => {
let t = r[3] == 0u64;  r[5] = c.rd64((r[13] as u32));
 if t { l = 0x0043c650; continue; }
r[6] = c.rd8((r[13] as u32).wrapping_add(0x00000006)) as u64;
uycpu::vu0::mov::<1>(c, 5, c.vf[0]);
{ let (a, b) = uycpu::mmi::pextlh((r[5], h[5]), (0u64, 0u64)); r[5] = a; h[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[5], h[5]), 16); r[5] = a; h[5] = b; }
l = 0x0043c610; continue; }
0x0043c610 => {
c.vf[6] = [r[5] as u32, (r[5] >> 32) as u32, h[5] as u32, (h[5] >> 32) as u32];
r[1] = (((r[10] as u32).wrapping_sub((r[6] as u32))) as i32 as i64 as u64);
let t = (r[1] as i64) <= 0;  r[8] = (((r[6] as u32) << 6) as i32 as i64 as u64);
 if t { l = 0x0043c650; continue; }
r[8] = (((r[8] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[5] = c.rd64((r[13] as u32).wrapping_add(0x00000008));
uycpu::vu0::itof::<0, 14>(c, 5, 6);
r[13] = (((r[13] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[6] = c.rd8((r[13] as u32).wrapping_add(0x00000006)) as u64;
{ let (a, b) = uycpu::mmi::pextlh((r[5], h[5]), (0u64, 0u64)); r[5] = a; h[5] = b; }
r[7] = c.rd32((r[8] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psraw((r[5], h[5]), 16); r[5] = a; h[5] = b; }
{ let v = c.vf[5]; c.wr128((r[8] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (r[3] as i64) > 0;  dw(c, (r[8] as u32).wrapping_add(0x0000002c), r[7] as u32);
 if t { l = 0x0043c610; continue; }
l = 0x0043c650; continue; }
0x0043c650 => {
r[1] = (((r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
r[2] = (((r[25] as u32).wrapping_add(0xffffff00)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); r[6] = a; h[6] = b; }
r[3] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); r[8] = a; h[8] = b; }
l = 0x0043c668; continue; }
0x0043c668 => {
{ let (a, b) = uycpu::mmi::pextlh((r[6], h[6]), (0u64, 0u64)); r[5] = a; h[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[5], h[5]), 16); r[5] = a; h[5] = b; }
c.vf[5] = [r[5] as u32, (r[5] >> 32) as u32, h[5] as u32, (h[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[6], h[6]), (0u64, 0u64)); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[6], h[6]), 16); r[6] = a; h[6] = b; }
c.vf[6] = [r[6] as u32, (r[6] >> 32) as u32, h[6] as u32, (h[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((r[8], h[8]), (0u64, 0u64)); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
c.vf[7] = [r[7] as u32, (r[7] >> 32) as u32, h[7] as u32, (h[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[8], h[8]), (0u64, 0u64)); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[8], h[8]), 16); r[8] = a; h[8] = b; }
c.vf[8] = [r[8] as u32, (r[8] >> 32) as u32, h[8] as u32, (h[8] >> 32) as u32];
uycpu::vu0::itof::<3, 15>(c, 5, 5);
r[1] = (((r[1] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
r[2] = (((r[2] as u32).wrapping_add(0x00000100)) as i32 as i64 as u64);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
{ let (a, b) = c.rd128((r[1] as u32)); r[6] = a; h[6] = b; }
uycpu::vu0::itof::<3, 15>(c, 8, 8);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); r[8] = a; h[8] = b; }
{ let v = c.vf[5]; c.wr128((r[2] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[6]; c.wr128((r[2] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[7]; c.wr128((r[2] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0xfffffffc)) as i32 as i64 as u64);
let t = (r[3] as i64) > 0;  { let v = c.vf[8]; c.wr128((r[2] as u32).wrapping_add(0x000000c0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c668; continue; }
l = 0x0043c6e8; continue; }
0x0043c6e8 => {
r[15] = c.rd64((r[24] as u32).wrapping_add(0x00000050));
let t = r[15] != 0u64;  r[14] = (((r[15] as u32) >> 0) as i32 as i64 as u64);
 if t { l = 0x0043ca28; continue; }
l = 0x0043c6f8; continue; }
0x0043c6f8 => {
r[12] = (((r[10] as u32) << 6) as i32 as i64 as u64);
r[11] = (((r[25] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[12] = (((r[12] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
uycpu::vu0::mov::<15>(c, 4, c.vf[1]);
uycpu::vu0::mov::<15>(c, 5, c.vf[2]);
dw(c, (r[12] as u32).wrapping_add(0x0000002c), r[25] as u32);
uycpu::vu0::mov::<15>(c, 6, c.vf[3]);
uycpu::vu0::mov::<15>(c, 7, c.vf[0]);
l = 0x0043c718; continue; }
0x0043c718 => {
{ let (a, b) = c.rd128((r[11] as u32)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::mov::<15>(c, 16, c.vf[1]);
vop::<0, 15>(c, 11, 10, c.vf[10]);
uycpu::vu0::mov::<15>(c, 17, c.vf[2]);
uycpu::vu0::mov::<15>(c, 18, c.vf[3]);
r[13] = (((r[11] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
uycpu::vu0::mov::<15>(c, 19, c.vf[0]);
r[11] = (((r[11] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[13] as u32).wrapping_add(0x00000010)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, 12, 11, b4(c.vf[10][3]));
vop::<6, 14>(c, 13, 11, b4(c.vf[10][0]));
vop::<6, 6>(c, 14, 11, b4(c.vf[10][1]));
vop::<6, 2>(c, 15, 11, b4(c.vf[10][2]));
vop::<0, 8>(c, 17, 0, b4(c.vf[12][2]));
vop::<1, 8>(c, 18, 0, b4(c.vf[12][1]));
vop::<0, 4>(c, 18, 0, b4(c.vf[12][0]));
vop::<1, 8>(c, 16, 16, b4(c.vf[14][1]));
vop::<1, 4>(c, 17, 17, b4(c.vf[13][0]));
vop::<1, 2>(c, 18, 18, b4(c.vf[13][0]));
vop::<1, 4>(c, 16, 13, b4(c.vf[12][2]));
vop::<0, 2>(c, 16, 13, b4(c.vf[12][1]));
vop::<1, 2>(c, 17, 14, b4(c.vf[12][0]));
vop::<0, 8>(c, 17, 17, b4(c.vf[13][1]));
vop::<0, 8>(c, 18, 18, b4(c.vf[13][2]));
vop::<0, 4>(c, 18, 18, b4(c.vf[14][2]));
vop::<1, 8>(c, 16, 16, b4(c.vf[15][2]));
vop::<1, 4>(c, 17, 17, b4(c.vf[15][2]));
vop::<1, 2>(c, 18, 18, b4(c.vf[14][1]));
{ let (a, b) = c.rd128((r[13] as u32).wrapping_add(0x00000020)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, 16, 16, b4(c.vf[8][0]));
vop::<6, 14>(c, 17, 17, b4(c.vf[8][1]));
vop::<6, 14>(c, 18, 18, b4(c.vf[8][2]));
r[9] = c.rd32((r[11] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
vop::<0, 14>(c, 19, 19, c.vf[9]);
vop::<6, 15>(c, ACC, 4, b4(c.vf[16][0]));
vop::<2, 15>(c, ACC, 5, b4(c.vf[16][1]));
vop::<2, 15>(c, 16, 6, b4(c.vf[16][2]));
vop::<6, 15>(c, ACC, 4, b4(c.vf[17][0]));
vop::<2, 15>(c, ACC, 5, b4(c.vf[17][1]));
vop::<2, 15>(c, 17, 6, b4(c.vf[17][2]));
vop::<6, 15>(c, ACC, 4, b4(c.vf[18][0]));
vop::<2, 15>(c, ACC, 5, b4(c.vf[18][1]));
vop::<2, 15>(c, 18, 6, b4(c.vf[18][2]));
vop::<6, 15>(c, ACC, 4, b4(c.vf[19][0]));
vop::<2, 15>(c, ACC, 5, b4(c.vf[19][1]));
vop::<2, 15>(c, ACC, 6, b4(c.vf[19][2]));
vop::<2, 15>(c, 19, 7, b4(c.vf[0][3]));
{ let v = c.vf[16]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[17]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[18]; c.wr128((r[13] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = r[9] == 0u64;  { let v = c.vf[19]; c.wr128((r[13] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c888; continue; }
{ let (a, b) = c.rd128((r[9] as u32)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[9] as u32).wrapping_add(0x00000010)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[9] as u32).wrapping_add(0x00000020)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
let t = r[11] != r[12];  { let (a, b) = c.rd128((r[9] as u32).wrapping_add(0x00000030)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x0043c718; continue; }
l = 0x0043c808; continue; }
0x0043c808 => {
r[13] = c.rd32((r[25] as u32).wrapping_add(0x00003720)) as i32 as i64 as u64;
r[11] = (((r[25] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = (r[13] as i64) > 0;  r[12] = (((r[25] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
 if t { l = 0x0043c848; continue; }
let t = (r[13] as i64) < 0;   if t { l = 0x0043c878; continue; }
l = 0x0043c820; continue; }
0x0043c820 => {
{ let (a, b) = c.rd128((r[11] as u32).wrapping_add(0x00000030)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[12] = (((r[12] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[11] as u32).wrapping_add(0x00000070)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[11] = (((r[11] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[1]; c.wr128((r[12] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[10] = (((r[10] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[10] as i64) > 0;  { let v = c.vf[2]; c.wr128((r[12] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c820; continue; }
 l = 0x0043c878; continue;
}
0x0043c848 => {
{ let (a, b) = c.rd128((r[11] as u32).wrapping_add(0x00000030)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[12] = (((r[12] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[11] as u32).wrapping_add(0x00000070)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[11] = (((r[11] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[1]; c.wr128((r[12] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
{ let v = c.vf[2]; c.wr128((r[12] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[10] = (((r[10] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[10] as i64) < 0;  { let v = c.vf[1]; c.wr128((r[13] as u32).wrapping_add(0xffffffe0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c878; continue; }
let t = (r[10] as i64) > 0;  { let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0xfffffff0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c848; continue; }
l = 0x0043c878; continue; }
0x0043c878 => {
{ let (a, b) = c.rd128((r[25] as u32).wrapping_add(0x00003700)); r[24] = a; h[24] = b; }
{ let (a, b) = c.rd128((r[25] as u32).wrapping_add(0x00003710)); r[25] = a; h[25] = b; }
 return;
}
0x0043c888 => {
uycpu::vu0::mov::<15>(c, 4, c.vf[1]);
uycpu::vu0::mov::<15>(c, 5, c.vf[2]);
uycpu::vu0::mov::<15>(c, 6, c.vf[3]);
let t = r[11] != r[12];  uycpu::vu0::mov::<15>(c, 7, c.vf[0]);
 if t { l = 0x0043c718; continue; }
 l = 0x0043c808; continue;
}
0x0043c8a8 => {
c.vf[5] = [r[5] as u32, (r[5] >> 32) as u32, h[5] as u32, (h[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[6], h[6]), (0u64, 0u64)); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[6], h[6]), 16); r[6] = a; h[6] = b; }
c.vf[6] = [r[6] as u32, (r[6] >> 32) as u32, h[6] as u32, (h[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((r[8], h[8]), (0u64, 0u64)); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
c.vf[7] = [r[7] as u32, (r[7] >> 32) as u32, h[7] as u32, (h[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[8], h[8]), (0u64, 0u64)); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[8], h[8]), 16); r[8] = a; h[8] = b; }
c.vf[8] = [r[8] as u32, (r[8] >> 32) as u32, h[8] as u32, (h[8] >> 32) as u32];
l = 0x0043c8e0; continue; }
0x0043c8e0 => {
uycpu::vu0::itof::<3, 15>(c, 5, 5);
uycpu::vu0::itof::<3, 15>(c, 7, 7);
uycpu::vu0::itof::<3, 15>(c, 6, 6);
uycpu::vu0::itof::<3, 15>(c, 8, 8);
vop::<6, 15>(c, 5, 5, b4(c.vf[4][3]));
vop::<6, 15>(c, 7, 7, b4(c.vf[4][0]));
vop::<6, 15>(c, 6, 6, b4(c.vf[4][3]));
vop::<6, 15>(c, 8, 8, b4(c.vf[4][0]));
vop::<0, 15>(c, 9, 5, c.vf[7]);
vop::<6, 15>(c, 10, 5, c.vf[7]);
vop::<0, 15>(c, 11, 6, c.vf[8]);
vop::<6, 15>(c, 12, 6, c.vf[8]);
vop::<6, 15>(c, 13, 9, c.vf[9]);
vop::<0, 4>(c, ACC, 10, b4(c.vf[10][0]));
vop::<2, 4>(c, ACC, 2, b4(c.vf[10][2]));
vop::<2, 4>(c, 14, 2, b4(c.vf[10][3]));
vop::<0, 8>(c, ACC, 13, b4(c.vf[13][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[13][2]));
vop::<2, 8>(c, 13, 1, b4(c.vf[13][3]));
{ let (a, b) = { let v = c.vf[14]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[5] = a; h[5] = b; }
let t = (r[5] as i64) >= 0;   if t { l = 0x0043c950; continue; }
vop::<1, 15>(c, 9, 5, c.vf[7]);
vop::<6, 15>(c, 13, 9, c.vf[9]);
vop::<0, 8>(c, ACC, 13, b4(c.vf[13][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[13][2]));
vop::<2, 8>(c, 13, 1, b4(c.vf[13][3]));
l = 0x0043c950; continue; }
0x0043c950 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[13][0]);
vop::<6, 15>(c, 15, 11, c.vf[11]);
vop::<0, 4>(c, ACC, 12, b4(c.vf[12][0]));
vop::<2, 4>(c, ACC, 2, b4(c.vf[12][2]));
vop::<2, 4>(c, 16, 2, b4(c.vf[12][3]));
vop::<0, 8>(c, ACC, 15, b4(c.vf[15][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[15][2]));
vop::<2, 8>(c, 15, 1, b4(c.vf[15][3]));
{ let (a, b) = { let v = c.vf[16]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[5] = a; h[5] = b; }
let t = (r[5] as i64) >= 0;   if t { l = 0x0043c9a0; continue; }
vop::<1, 15>(c, 11, 6, c.vf[8]);
vop::<6, 15>(c, 15, 11, c.vf[11]);
vop::<0, 8>(c, ACC, 15, b4(c.vf[15][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[15][2]));
vop::<2, 8>(c, 15, 1, b4(c.vf[15][3]));
l = 0x0043c9a0; continue; }
0x0043c9a0 => {
vop::<6, 15>(c, 9, 9, b4(c.vi[22]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[15][0]);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); r[6] = a; h[6] = b; }
r[1] = (((r[1] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[2] as u32).wrapping_add(0x00000010)); r[8] = a; h[8] = b; }
r[2] = (((r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlh((r[6], h[6]), (0u64, 0u64)); r[5] = a; h[5] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[5], h[5]), 16); r[5] = a; h[5] = b; }
c.vf[5] = [r[5] as u32, (r[5] >> 32) as u32, h[5] as u32, (h[5] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[6], h[6]), (0u64, 0u64)); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[6], h[6]), 16); r[6] = a; h[6] = b; }
c.vf[6] = [r[6] as u32, (r[6] >> 32) as u32, h[6] as u32, (h[6] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextlh((r[8], h[8]), (0u64, 0u64)); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[7], h[7]), 16); r[7] = a; h[7] = b; }
c.vf[7] = [r[7] as u32, (r[7] >> 32) as u32, h[7] as u32, (h[7] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pextuh((r[8], h[8]), (0u64, 0u64)); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[8], h[8]), 16); r[8] = a; h[8] = b; }
c.vf[8] = [r[8] as u32, (r[8] >> 32) as u32, h[8] as u32, (h[8] >> 32) as u32];
vop::<6, 15>(c, 11, 11, b4(c.vi[22]));
r[4] = (((r[4] as u32).wrapping_add(0x00000080)) as i32 as i64 as u64);
{ let v = c.vf[9]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[3] as i64) > 0;  { let v = c.vf[11]; c.wr128((r[4] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043c8e0; continue; }
 l = 0x0043c6e8; continue;
}
0x0043ca28 => {
let t = r[14] == 0u64;  r[9] = 0x0000000010000000;
 if t { l = 0x0043cba0; continue; }
r[13] = c.rd16((r[14] as u32).wrapping_add(0x00000002)) as u64;
r[9] = r[9] | 0xd400;
r[12] = c.rd32((r[14] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
r[1] = ((0u32.wrapping_add(0x00002000)) as i32 as i64 as u64);
dw(c, (r[9] as u32).wrapping_add(0x00000080), r[1] as u32);
r[1] = (((r[13] as u32).wrapping_add((r[13] as u32))) as i32 as i64 as u64);
dw(c, (r[9] as u32).wrapping_add(0x00000010), r[12] as u32);
r[13] = (((r[13] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
dw(c, (r[9] as u32).wrapping_add(0x00000020), r[13] as u32);
r[1] = ((0u32.wrapping_add(0x00000100)) as i32 as i64 as u64);
dw(c, (r[9] as u32), r[1] as u32);
r[13] = (((r[13] as u32) << 4) as i32 as i64 as u64);
r[1] = c.rd32((r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[12] = (((r[25] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
c.vf[4] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
r[13] = (((r[13] as u32).wrapping_add((r[12] as u32))) as i32 as i64 as u64);
r[14] = c.rd32((r[14] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
r[1] = (((r[25] as u32).wrapping_add(0x00001e00)) as i32 as i64 as u64);
dw(c, (r[13] as u32).wrapping_add(0x0000002c), r[1] as u32);
vop::<1, 1>(c, 4, 0, b4(c.vf[4][0]));
l = 0x0043ca80; continue; }
0x0043ca80 => {
r[2] = c.rd32((r[9] as u32)) as i32 as i64 as u64;
r[2] = r[2] & 0x100;
let t = r[2] != 0u64;   if t { l = 0x0043ca80; continue; }
r[11] = c.rd32((r[12] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[12] as u32)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[12] as u32).wrapping_add(0x00000010)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[12] as u32).wrapping_add(0x00000020)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[11] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[11] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[11] as u32).wrapping_add(0x00000020)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x0043cac0; continue; }
0x0043cac0 => {
vop::<6, 15>(c, 6, 6, b4(c.vf[4][3]));
r[2] = (((r[10] as u32) << 6) as i32 as i64 as u64);
vop::<6, 15>(c, 9, 9, b4(c.vf[4][0]));
r[2] = (((r[2] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
vop::<6, 14>(c, 7, 7, b4(c.vf[4][3]));
r[2] = (((r[2] as u32).wrapping_sub((r[11] as u32))) as i32 as i64 as u64);
let t = (r[2] as i64) <= 0;  vop::<6, 14>(c, 10, 10, b4(c.vf[4][0]));
 if t { l = 0x0043cb98; continue; }
vop::<6, 14>(c, 8, 8, b4(c.vf[4][3]));
vop::<6, 14>(c, 11, 11, b4(c.vf[4][0]));
vop::<0, 15>(c, 12, 6, c.vf[9]);
vop::<6, 15>(c, 14, 6, c.vf[9]);
vop::<0, 14>(c, 7, 7, c.vf[10]);
vop::<0, 14>(c, 8, 8, c.vf[11]);
vop::<6, 15>(c, 13, 12, c.vf[12]);
vop::<0, 4>(c, ACC, 14, b4(c.vf[14][0]));
vop::<2, 4>(c, ACC, 2, b4(c.vf[14][2]));
vop::<2, 4>(c, 14, 2, b4(c.vf[14][3]));
vop::<0, 8>(c, ACC, 13, b4(c.vf[13][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[13][2]));
vop::<2, 8>(c, 13, 1, b4(c.vf[13][3]));
{ let (a, b) = { let v = c.vf[14]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
let t = (r[2] as i64) >= 0;   if t { l = 0x0043cb38; continue; }
vop::<1, 15>(c, 12, 6, c.vf[9]);
vop::<6, 15>(c, 13, 12, c.vf[12]);
vop::<0, 8>(c, ACC, 13, b4(c.vf[13][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[13][2]));
vop::<2, 8>(c, 13, 1, b4(c.vf[13][3]));
l = 0x0043cb38; continue; }
0x0043cb38 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[13][0]);
r[3] = c.rd32((r[12] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
r[1] = c.rd32((r[12] as u32).wrapping_add(0x0000005c)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[12] as u32).wrapping_add(0x00000030)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[12] as u32).wrapping_add(0x00000040)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[12] as u32).wrapping_add(0x00000050)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; c.wr128((r[11] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((r[11] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000020)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[2] = c.rd32((r[11] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
let t = r[2] != 0u64;   if t { l = 0x0043cb80; continue; }
dw(c, (r[11] as u32).wrapping_add(0x0000001c), r[3] as u32);
l = 0x0043cb80; continue; }
0x0043cb80 => {
vop::<6, 15>(c, 12, 12, b4(c.vi[22]));
r[12] = (((r[12] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
{ let v = c.vf[12]; c.wr128((r[11] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[11] = (((r[1] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = r[12] != r[13];   if t { l = 0x0043cac0; continue; }
l = 0x0043cb98; continue; }
0x0043cb98 => {
let t = r[14] != 0u64;   if t { l = 0x0043ca28; continue; }
l = 0x0043cba0; continue; }
0x0043cba0 => {
r[15] = r[15] >> 32;
vop::<0, 14>(c, 5, 0, b4(c.vf[0][3]));
l = 0x0043cba8; continue; }
0x0043cba8 => {
let t = r[15] == 0u64;  r[14] = (((r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x0043ccc8; continue; }
r[1] = c.rd32((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[3] = (((r[10] as u32) << 6) as i32 as i64 as u64);
r[2] = c.rd8((r[14] as u32).wrapping_add(0x00000003)) as u64;
r[3] = (((r[3] as u32).wrapping_add((r[25] as u32))) as i32 as i64 as u64);
r[15] = c.rd32((r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[3] = (((r[3] as u32).wrapping_sub((r[1] as u32))) as i32 as i64 as u64);
let t = (r[3] as i64) <= 0;   if t { l = 0x0043cba8; continue; }
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000020)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[14] as u32).wrapping_add(0x00000010)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[14] as u32).wrapping_add(0x00000020)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[14] as u32).wrapping_add(0x00000030)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
let t = r[2] != 0u64;   if t { l = 0x0043cc28; continue; }
vop::<6, 1>(c, 12, 7, c.vf[6]);
vop::<6, 14>(c, 13, 7, c.vf[6]);
vop::<6, 14>(c, 14, 7, b4(c.vf[6][3]));
vop::<6, 14>(c, 15, 6, b4(c.vf[7][3]));
uycpu::vu0::opmula(c, 6, 7);
uycpu::vu0::opmsub(c, 16, 7, 6);
vop::<0, 8>(c, ACC, 13, b4(c.vf[13][1]));
vop::<2, 8>(c, 13, 5, b4(c.vf[13][2]));
vop::<0, 14>(c, 6, 14, c.vf[15]);
vop::<0, 14>(c, 6, 6, c.vf[16]);
vop::<1, 1>(c, 6, 12, b4(c.vf[13][0]));
vop::<6, 14>(c, 8, 8, c.vf[9]);
vop::<0, 14>(c, 10, 10, c.vf[11]);
 l = 0x0043ccb8; continue;
}
0x0043cc28 => {
r[3] = c.rd32((r[14] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
c.vf[18] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
vop::<1, 1>(c, 18, 0, b4(c.vf[18][0]));
vop::<6, 15>(c, 6, 6, b4(c.vf[18][3]));
vop::<6, 15>(c, 7, 7, b4(c.vf[18][0]));
vop::<6, 14>(c, 8, 8, b4(c.vf[18][3]));
vop::<6, 14>(c, 9, 9, b4(c.vf[18][0]));
vop::<6, 14>(c, 10, 10, b4(c.vf[18][3]));
vop::<6, 14>(c, 11, 11, b4(c.vf[18][0]));
vop::<0, 15>(c, 14, 6, c.vf[7]);
vop::<6, 15>(c, 16, 6, c.vf[7]);
vop::<0, 14>(c, 8, 8, c.vf[9]);
vop::<0, 14>(c, 10, 10, c.vf[11]);
vop::<6, 15>(c, 15, 14, c.vf[14]);
vop::<0, 4>(c, ACC, 16, b4(c.vf[16][0]));
vop::<2, 4>(c, ACC, 2, b4(c.vf[16][2]));
vop::<2, 4>(c, 16, 2, b4(c.vf[16][3]));
vop::<0, 8>(c, ACC, 15, b4(c.vf[15][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[15][2]));
vop::<2, 8>(c, 15, 1, b4(c.vf[15][3]));
{ let (a, b) = { let v = c.vf[16]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
let t = (r[2] as i64) >= 0;   if t { l = 0x0043cca0; continue; }
vop::<1, 15>(c, 14, 6, c.vf[7]);
vop::<6, 15>(c, 15, 14, c.vf[14]);
vop::<0, 8>(c, ACC, 15, b4(c.vf[15][1]));
vop::<2, 8>(c, ACC, 1, b4(c.vf[15][2]));
vop::<2, 8>(c, 15, 1, b4(c.vf[15][3]));
l = 0x0043cca0; continue; }
0x0043cca0 => {
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[15][0]);
vop::<6, 15>(c, 6, 14, b4(c.vi[22]));
l = 0x0043ccb8; continue; }
0x0043ccb8 => {
{ let v = c.vf[6]; c.wr128((r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[8]; c.wr128((r[1] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = r[15] != 0u64;  { let v = c.vf[10]; c.wr128((r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x0043cba8; continue; }
l = 0x0043ccc8; continue; }
0x0043ccc8 => {
 l = 0x0043c6f8; continue;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a0830(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a0830;
loop { match l {
0x004a0830 => {
r[29] = (((r[29] as u32).wrapping_add(0xffffffa0)) as i32 as i64 as u64);
c.wr64((r[29] as u32).wrapping_add(0x00000038), r[19]);
r[19] = r[4].wrapping_add(0u64);
c.wr64((r[29] as u32).wrapping_add(0x00000028), r[17]);
c.wr64((r[29] as u32).wrapping_add(0x00000050), r[22]);
c.wr64((r[29] as u32).wrapping_add(0x00000020), r[16]);
c.wr64((r[29] as u32).wrapping_add(0x00000030), r[18]);
c.wr64((r[29] as u32).wrapping_add(0x00000040), r[20]);
c.wr64((r[29] as u32).wrapping_add(0x00000048), r[21]);
c.wr64((r[29] as u32).wrapping_add(0x00000058), r[31]);
r[31] = k.pc(0x4a0860) as u64; r[17] = r[19].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0858, 0x4a04b0); f004a04b0(c, k2, r, h); }
r[4] = sx(k.a(c, 0x4a0860, 0x4a0864).wrapping_sub(0x10e8));
r[4] = c.rd32((r[4] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[2] = 0x0000000030000000;
r[2] = r[2] | 0x7;
r[3] = sx(k.a(c, 0x4a0870, 0x4a0878).wrapping_sub(0x20b0));
dw(c, (r[4] as u32), r[2] as u32);
r[3] = (((r[3] as u32).wrapping_add(0x000020b0)) as i32 as i64 as u64);
r[9] = 0x0000000013000000;
r[4] = 0x0000000050000000;
r[2] = sx(k.a(c, 0x4a0884, 0x4a0888).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[4] = r[4] | 0x7;
r[6] = 0x0000000030000000;
r[7] = sx(k.a(c, 0x4a0894, 0x4a08a0).wrapping_sub(0xffffde30));
dw(c, (r[2] as u32).wrapping_add(0x00000004), r[3] as u32);
r[6] = r[6] | 0x3;
r[7] = (((r[7] as u32).wrapping_add(0xffffde30)) as i32 as i64 as u64);
r[8] = 0x0000000050000000;
r[2] = sx(k.a(c, 0x4a08a8, 0x4a08ac).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[8] = r[8] | 0x3;
r[5] = sx(k.a(c, 0x4a08b4, 0x4a08bc).wrapping_sub(0xfffffec0));
dw(c, (r[2] as u32).wrapping_add(0x00000008), r[9] as u32);
r[5] = (((r[5] as u32).wrapping_add(0xfffffec0)) as i32 as i64 as u64);
r[2] = sx(k.a(c, 0x4a08c0, 0x4a08c4).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[2] as u32).wrapping_add(0x0000000c), r[4] as u32);
r[3] = sx(k.a(c, 0x4a08cc, 0x4a08d0).wrapping_sub(0x10e8));
r[3] = c.rd32((r[3] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[2] = (((r[3] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a08d8, 0x4a08dc).wrapping_sub(0x10e8));
dw(c, (r[1] as u32).wrapping_add(0x000010e8), r[2] as u32);
dw(c, (r[3] as u32).wrapping_add(0x00000010), r[6] as u32);
r[2] = sx(k.a(c, 0x4a08e4, 0x4a08e8).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[2] as u32).wrapping_add(0x00000004), r[7] as u32);
r[3] = sx(k.a(c, 0x4a08f0, 0x4a08f4).wrapping_sub(0x10e8));
r[3] = c.rd32((r[3] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[3] as u32).wrapping_add(0x00000008), r[9] as u32);
r[4] = sx(k.a(c, 0x4a08fc, 0x4a0900).wrapping_sub(0x10e8));
r[4] = c.rd32((r[4] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[4] as u32).wrapping_add(0x0000000c), r[8] as u32);
r[2] = c.rd16((r[5] as u32).wrapping_add(0x00000152)) as u64;
r[3] = c.rd16((r[5] as u32).wrapping_add(0x00000150)) as u64;
r[2] = (((r[2] as u32) << 16) as i32 as i64 as u64);
r[4] = sx(k.a(c, 0x4a0914, 0x4a0918).wrapping_sub(0x10e8));
r[4] = c.rd32((r[4] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[3] = (((r[3] as u32) << 16) as i32 as i64 as u64);
r[2] = (((((r[2] as u32) as i32) >> 17) as u32) as i32 as i64 as u64);
r[3] = (((((r[3] as u32) as i32) >> 17) as u32) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xfffff800)) as i32 as i64 as u64);
r[3] = (((r[3] as u32).wrapping_add(0xfffff800)) as i32 as i64 as u64);
c.f[0] = (r[2] as u32);
c.f[0] = uycpu::fpu::cvt_s(c.f[0]);
c.f[1] = (r[3] as u32);
c.f[1] = uycpu::fpu::cvt_s(c.f[1]);
r[22] = (((r[4] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[4] = (((r[4] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[2] = sx(k.a(c, 0x4a0950, 0x4a0954).wrapping_sub(0x5477));
r[2] = c.rd8((r[2] as u32).wrapping_add(0x00005477)) as u64;
r[1] = sx(k.a(c, 0x4a0958, 0x4a095c).wrapping_sub(0xe24));
dw(c, (r[1] as u32).wrapping_add(0x00000e24), c.f[0]);
r[1] = sx(k.a(c, 0x4a0960, 0x4a0964).wrapping_sub(0xe20));
dw(c, (r[1] as u32).wrapping_add(0x00000e20), c.f[1]);
let t = r[2] == 0u64;  dw(c, (r[28] as u32).wrapping_add(k.lo(c, 0x4a096c)), r[4] as u32);
 if t { l = 0x004a097c; continue; }
r[21] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = 0u64 == 0u64;  r[20] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
 if t { l = 0x004a0984; continue; }
l = 0x004a097c; continue; }
0x004a097c => {
r[21] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
r[20] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
l = 0x004a0984; continue; }
0x004a0984 => {
r[2] = c.rd32((r[19] as u32)) as i32 as i64 as u64;
let t = r[2] == 0u64;  r[3] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a098c))) as i32 as i64 as u64;
 if t { l = 0x004a0ac0; continue; }
l = 0x004a0990; continue; }
0x004a0990 => {
r[1] = 0x00000000447a0000;
c.f[12] = (r[1] as u32);
r[4] = (((r[17] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[19] = (((r[19] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
r[31] = k.pc(0x4a09a8) as u64; r[18] = 0u64.wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a09a0, 0x4a0348); f004a0348(c, k2, r, h); }
c.f[0] = c.rd32((r[17] as u32).wrapping_add(0x00000010));
dw(c, (r[29] as u32).wrapping_add(0x00000010), c.f[0]);
dw(c, (r[29] as u32), c.f[0]);
c.f[1] = c.rd32((r[17] as u32).wrapping_add(0x00000014));
dw(c, (r[29] as u32).wrapping_add(0x00000014), c.f[1]);
dw(c, (r[29] as u32).wrapping_add(0x00000004), c.f[1]);
c.f[2] = c.rd32((r[17] as u32).wrapping_add(0x00000018));
dw(c, (r[29] as u32).wrapping_add(0x00000018), c.f[2]);
dw(c, (r[29] as u32).wrapping_add(0x00000008), c.f[2]);
c.f[0] = c.rd32((r[17] as u32).wrapping_add(0x00000008));
c.f[1] = c.rd32((r[17] as u32).wrapping_add(0x0000000c));
c.f[0] = c.f[0] ^ 0x8000_0000;
c.f[0] = uycpu::fpu::sub(c.f[0], c.f[1]);
dw(c, (r[29] as u32).wrapping_add(0x0000000c), c.f[0]);
c.f[1] = c.rd32((r[17] as u32).wrapping_add(0x00000008));
c.f[0] = c.rd32((r[17] as u32).wrapping_add(0x0000000c));
c.f[1] = c.f[1] ^ 0x8000_0000;
c.f[1] = uycpu::fpu::add(c.f[1], c.f[0]);
dw(c, (r[29] as u32).wrapping_add(0x0000001c), c.f[1]);
r[2] = c.rd32((r[17] as u32)) as i32 as i64 as u64;
let t = (r[2] as i64) <= 0;  if t { r[17] = r[19].wrapping_add(0u64);
 l = 0x004a0ab4; continue; }
l = 0x004a0a00; continue; }
0x004a0a00 => {
r[31] = k.pc(0x4a0a08) as u64; r[4] = r[19].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0a00, 0x4a0458); f004a0458(c, k2, r, h); }
r[19] = r[2].wrapping_add(0u64);
r[4] = r[29].wrapping_add(0u64);
r[31] = k.pc(0x4a0a18) as u64; r[5] = (((r[29] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a0a10, 0x4a12a8); f004a12a8(c, k2, r, h); }
r[2] = c.rd32((r[17] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
let t = r[2] == 0u64;  r[5] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a0a20))) as i32 as i64 as u64;
 if t { l = 0x004a0a34; continue; }
r[31] = k.pc(0x4a0a2c) as u64; r[4] = 0x0000000070000000;
 { let k2 = k.call(c, 0x4a0a24, 0x4a1530); f004a1530(c, k2, r, h); }
let t = 0u64 == 0u64;  r[16] = r[2].wrapping_add(0u64);
 if t { l = 0x004a0a40; continue; }
l = 0x004a0a34; continue; }
0x004a0a34 => {
r[4] = 0x0000000070000000;
r[31] = k.pc(0x4a0a40) as u64; r[16] = 0u64.wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0a38, 0x4a1340); f004a1340(c, k2, r, h); }
l = 0x004a0a40; continue; }
0x004a0a40 => {
let t = r[16] == 0u64;   if t { l = 0x004a0a80; continue; }
r[5] = sx(k.a(c, 0x4a0a48, 0x4a0a4c).wrapping_sub(0xffff976c));
r[5] = c.rd32((r[5] as u32).wrapping_add(0xffff976c)) as i32 as i64 as u64;
r[31] = k.pc(0x4a0a58) as u64; r[4] = r[20].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0a50, 0x4a19a0); f004a19a0(c, k2, r, h); }
r[5] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a0a58))) as i32 as i64 as u64;
r[31] = k.pc(0x4a0a64) as u64; r[4] = r[21].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0a5c, 0x4a19a0); f004a19a0(c, k2, r, h); }
r[2] = r[16] & 0x20;
let t = r[2] == 0u64;  if t { r[2] = c.rd32((r[17] as u32)) as i32 as i64 as u64;
 l = 0x004a0aa0; continue; }
r[31] = k.pc(0x4a0a78) as u64; r[4] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a0a74))) as i32 as i64 as u64;
 { let k2 = k.call(c, 0x4a0a70, 0x4a1c40); f004a1c40(c, k2, r, h); }
let t = 0u64 == 0u64;  r[2] = c.rd32((r[17] as u32)) as i32 as i64 as u64;
 if t { l = 0x004a0aa0; continue; }
l = 0x004a0a80; continue; }
0x004a0a80 => {
r[5] = sx(k.a(c, 0x4a0a80, 0x4a0a84).wrapping_sub(0xffff976c));
r[5] = c.rd32((r[5] as u32).wrapping_add(0xffff976c)) as i32 as i64 as u64;
r[31] = k.pc(0x4a0a90) as u64; r[4] = r[20].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0a88, 0x4a1800); f004a1800(c, k2, r, h); }
r[5] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a0a90))) as i32 as i64 as u64;
r[31] = k.pc(0x4a0a9c) as u64; r[4] = r[21].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0a94, 0x4a1800); f004a1800(c, k2, r, h); }
r[2] = c.rd32((r[17] as u32)) as i32 as i64 as u64;
l = 0x004a0aa0; continue; }
0x004a0aa0 => {
r[18] = (((r[18] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[2] = ((r[18] as i64) < (r[2] as i64)) as u64;
let t = r[2] != 0u64;   if t { l = 0x004a0a00; continue; }
r[17] = r[19].wrapping_add(0u64);
l = 0x004a0ab4; continue; }
0x004a0ab4 => {
r[2] = c.rd32((r[17] as u32)) as i32 as i64 as u64;
let t = r[2] != 0u64;  r[3] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a0abc))) as i32 as i64 as u64;
 if t { l = 0x004a0990; continue; }
l = 0x004a0ac0; continue; }
0x004a0ac0 => {
r[2] = (((r[3] as u32).wrapping_sub((r[22] as u32))) as i32 as i64 as u64);
r[2] = (((((r[2] as u32) as i32) >> 4) as u32) as i32 as i64 as u64);
r[4] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = (r[4] as i64) <= 0;  r[2] = 0x0000000050000000;
 if t { l = 0x004a0af4; continue; }
r[3] = 0x0000000010000000;
r[2] = r[4] | r[2];
r[3] = r[4] | r[3];
dw(c, (r[22] as u32).wrapping_add(0x0000000c), r[2] as u32);
dw(c, (r[22] as u32), r[3] as u32);
dw(c, (r[22] as u32).wrapping_add(0x00000004), 0u64 as u32);
let t = 0u64 == 0u64;  dw(c, (r[22] as u32).wrapping_add(0x00000008), 0u64 as u32);
 if t { l = 0x004a0b00; continue; }
l = 0x004a0af4; continue; }
0x004a0af4 => {
r[2] = (((r[3] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a0af8, 0x4a0afc).wrapping_sub(0x10e8));
dw(c, (r[1] as u32).wrapping_add(0x000010e8), r[2] as u32);
l = 0x004a0b00; continue; }
0x004a0b00 => {
r[2] = sx(k.a(c, 0x4a0b00, 0x4a0b08).wrapping_sub(0x1ed0));
r[3] = 0x0000000000080000;
r[2] = (((r[2] as u32).wrapping_add(0x00001ed0)) as i32 as i64 as u64);
r[4] = ((0u32.wrapping_add(0x0000004c)) as i32 as i64 as u64);
r[5] = c.rd32((r[2] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[5] = (((((r[5] as u32) as i32) >> 13) as u32) as i32 as i64 as u64);
r[31] = k.pc(0x4a0b20) as u64; r[5] = r[3] | r[5];
 { let k2 = k.call(c, 0x4a0b18, 0x4c1fe8); f004c1fe8(c, k2, r, h); }
r[5] = 0u64 | 0x8000;
r[5] = r[5] << 22;
r[5] = r[5] | 0x64;
r[31] = k.pc(0x4a0b34) as u64; r[4] = ((0u32.wrapping_add(0x00000042)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a0b2c, 0x4c1fe8); f004c1fe8(c, k2, r, h); }
r[31] = k.pc(0x4a0b3c) as u64; r[4] = 0u64.wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0b34, 0x4a0660); f004a0660(c, k2, r, h); }
r[5] = 0x0000000000050000;
r[5] = r[5] | 0x360b;
r[31] = k.pc(0x4a0b4c) as u64; r[4] = ((0u32.wrapping_add(0x00000047)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a0b44, 0x4c1fe8); f004c1fe8(c, k2, r, h); }
r[5] = 0u64 | 0x8000;
r[5] = r[5] << 24;
r[5] = r[5] | 0x44;
r[31] = k.pc(0x4a0b60) as u64; r[4] = ((0u32.wrapping_add(0x00000042)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a0b58, 0x4c1fe8); f004c1fe8(c, k2, r, h); }
r[16] = c.rd64((r[29] as u32).wrapping_add(0x00000020));
r[17] = c.rd64((r[29] as u32).wrapping_add(0x00000028));
r[18] = c.rd64((r[29] as u32).wrapping_add(0x00000030));
r[19] = c.rd64((r[29] as u32).wrapping_add(0x00000038));
r[20] = c.rd64((r[29] as u32).wrapping_add(0x00000040));
r[21] = c.rd64((r[29] as u32).wrapping_add(0x00000048));
r[22] = c.rd64((r[29] as u32).wrapping_add(0x00000050));
r[31] = c.rd64((r[29] as u32).wrapping_add(0x00000058));
r[29] = (((r[29] as u32).wrapping_add(0x00000060)) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a04b0(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a04b0;
loop { match l {
0x004a04b0 => {
r[29] = (((r[29] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
r[2] = sx(k.a(c, 0x4a04b4, 0x4a04bc).wrapping_sub(0xfffffec0));
c.wr64((r[29] as u32), r[16]);
r[2] = (((r[2] as u32).wrapping_add(0xfffffec0)) as i32 as i64 as u64);
c.wr64((r[29] as u32).wrapping_add(0x00000010), r[18]);
r[3] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr64((r[29] as u32).wrapping_add(0x00000018), r[31]);
r[4] = ((0u32.wrapping_add(0x00000042)) as i32 as i64 as u64);
c.wr64((r[29] as u32).wrapping_add(0x00000008), r[17]);
r[5] = ((0u32.wrapping_add(0x00000064)) as i32 as i64 as u64);
r[17] = c.rd16((r[2] as u32).wrapping_add(0x00000150)) as i16 as i64 as u64;
r[18] = c.rd16((r[2] as u32).wrapping_add(0x00000152)) as i16 as i64 as u64;
r[3] = ((r[3] as i64) < (r[17] as i64)) as u64;
r[2] = (((r[17] as u32).wrapping_add(0x0000001f)) as i32 as i64 as u64);
if r[3] != 0 { r[2] = r[17]; }
r[31] = k.pc(0x4a04f4) as u64; r[16] = (((((r[2] as u32) as i32) >> 5) as u32) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a04ec, 0x4c1fe8); f004c1fe8(c, k2, r, h); }
r[5] = sx(k.a(c, 0x4a04f4, 0x4a04f8).wrapping_sub(0x10e8));
r[5] = c.rd32((r[5] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[4] = (((r[16] as u32).wrapping_add(0x00000005)) as i32 as i64 as u64);
r[2] = 0x0000000010000000;
r[3] = 0x0000000050000000;
r[2] = r[4] | r[2];
r[13] = 0u64 | 0x8000;
r[13] = r[13] << 45;
r[13] = r[13] | 0x1;
dw(c, (r[5] as u32), r[2] as u32);
r[4] = r[4] | r[3];
r[2] = 0u64 | 0x9000;
r[2] = r[2] << 46;
r[5] = r[16] | 0x8000;
r[3] = sx(k.a(c, 0x4a052c, 0x4a0530).wrapping_sub(0x10e8));
r[3] = c.rd32((r[3] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[5] = r[5] | r[2];
r[11] = ((0u32.wrapping_add(0x0000000e)) as i32 as i64 as u64);
r[12] = 0x0000000000030000;
r[12] = r[12] | 0x5001;
dw(c, (r[3] as u32).wrapping_add(0x00000004), 0u64 as u32);
r[8] = ((0u32.wrapping_add(0x00000047)) as i32 as i64 as u64);
r[9] = 0u64 | 0x9000;
r[9] = r[9] << 46;
r[9] = r[9] | 0x8001;
r[10] = ((0u32.wrapping_add(0x00000010)) as i32 as i64 as u64);
r[2] = sx(k.a(c, 0x4a055c, 0x4a0560).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[6] = ((0u32.wrapping_add(0x00000146)) as i32 as i64 as u64);
r[7] = 0u64 | 0x8080;
r[7] = r[7] << 16;
r[7] = r[7] | 0x8080;
r[3] = ((0u32.wrapping_add(0x00000044)) as i32 as i64 as u64);
dw(c, (r[2] as u32).wrapping_add(0x00000008), 0u64 as u32);
r[14] = 0u64.wrapping_add(0u64);
r[2] = sx(k.a(c, 0x4a0580, 0x4a0584).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[2] as u32).wrapping_add(0x0000000c), r[4] as u32);
r[15] = sx(k.a(c, 0x4a058c, 0x4a0590).wrapping_sub(0x10e8));
r[15] = c.rd32((r[15] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[2] = (((r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a0598, 0x4a059c).wrapping_sub(0x10e8));
dw(c, (r[1] as u32).wrapping_add(0x000010e8), r[2] as u32);
c.wr64((r[15] as u32).wrapping_add(0x00000010), r[13]);
c.wr64((r[2] as u32).wrapping_add(0x00000048), r[3]);
c.wr64((r[2] as u32).wrapping_add(0x00000008), r[11]);
c.wr64((r[2] as u32).wrapping_add(0x00000010), r[12]);
c.wr64((r[2] as u32).wrapping_add(0x00000018), r[8]);
c.wr64((r[2] as u32).wrapping_add(0x00000020), r[9]);
c.wr64((r[2] as u32).wrapping_add(0x00000028), r[10]);
c.wr64((r[2] as u32).wrapping_add(0x00000030), r[6]);
c.wr64((r[2] as u32).wrapping_add(0x00000038), r[7]);
let t = (r[16] as i64) <= 0;  c.wr64((r[2] as u32).wrapping_add(0x00000040), r[5]);
 if t { l = 0x004a062c; continue; }
r[2] = (((r[18] as u32) << 3) as i32 as i64 as u64);
r[5] = 0u64 | 0x8000;
r[4] = (((r[2] as u32).wrapping_add(0x00007ff0)) as i32 as i64 as u64);
r[3] = (((r[17] as u32) << 3) as i32 as i64 as u64);
r[2] = (((r[5] as u32).wrapping_sub((r[2] as u32))) as i32 as i64 as u64);
r[3] = ((0u32.wrapping_sub((r[3] as u32))) as i32 as i64 as u64);
r[8] = r[2] << 16;
r[7] = (((r[3] as u32).wrapping_add((r[5] as u32))) as i32 as i64 as u64);
r[2] = 0u64 | 0x8200;
r[4] = r[4] << 16;
r[6] = (((r[3] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
r[5] = (((r[15] as u32).wrapping_add(0x00000060)) as i32 as i64 as u64);
l = 0x004a0600; continue; }
0x004a0600 => {
r[2] = r[7] | r[8];
r[3] = r[6] | r[4];
c.wr64((r[5] as u32), r[2]);
r[14] = (((r[14] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[5] = (((r[5] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[6] = (((r[6] as u32).wrapping_add(0x00000200)) as i32 as i64 as u64);
c.wr64((r[5] as u32), r[3]);
r[7] = (((r[7] as u32).wrapping_add(0x00000200)) as i32 as i64 as u64);
r[2] = ((r[14] as i64) < (r[16] as i64)) as u64;
let t = r[2] != 0u64;  r[5] = (((r[5] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x004a0600; continue; }
l = 0x004a062c; continue; }
0x004a062c => {
r[3] = sx(k.a(c, 0x4a062c, 0x4a0630).wrapping_sub(0x10e8));
r[3] = c.rd32((r[3] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[2] = (((r[16] as u32) << 4) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0x00000050)) as i32 as i64 as u64);
r[16] = c.rd64((r[29] as u32));
r[3] = (((r[3] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
r[17] = c.rd64((r[29] as u32).wrapping_add(0x00000008));
r[18] = c.rd64((r[29] as u32).wrapping_add(0x00000010));
r[31] = c.rd64((r[29] as u32).wrapping_add(0x00000018));
r[1] = sx(k.a(c, 0x4a0650, 0x4a0654).wrapping_sub(0x10e8));
dw(c, (r[1] as u32).wrapping_add(0x000010e8), r[3] as u32);
r[29] = (((r[29] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a0660(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a0660;
loop { match l {
0x004a0660 => {
r[29] = (((r[29] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
r[2] = sx(k.a(c, 0x4a0664, 0x4a066c).wrapping_sub(0xfffffec0));
c.wr64((r[29] as u32).wrapping_add(0x00000008), r[17]);
r[2] = (((r[2] as u32).wrapping_add(0xfffffec0)) as i32 as i64 as u64);
c.wr64((r[29] as u32), r[16]);
r[3] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[16] = c.rd16((r[2] as u32).wrapping_add(0x00000150)) as i16 as i64 as u64;
r[6] = 0x0000000010000000;
r[17] = c.rd16((r[2] as u32).wrapping_add(0x00000152)) as i16 as i64 as u64;
r[8] = 0x0000000050000000;
r[3] = ((r[3] as i64) < (r[16] as i64)) as u64;
r[2] = (((r[16] as u32).wrapping_add(0x0000001f)) as i32 as i64 as u64);
if r[3] != 0 { r[2] = r[16]; }
r[10] = sx(k.a(c, 0x4a0694, 0x4a0698).wrapping_sub(0x10e8));
r[10] = c.rd32((r[10] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[25] = (((((r[2] as u32) as i32) >> 5) as u32) as i32 as i64 as u64);
r[9] = 0u64 | 0x9000;
r[9] = r[9] << 46;
r[5] = (((r[25] as u32).wrapping_add(0x00000005)) as i32 as i64 as u64);
r[7] = r[25] | 0x8000;
r[6] = r[5] | r[6];
r[7] = r[7] | r[9];
dw(c, (r[10] as u32), r[6] as u32);
r[5] = r[5] | r[8];
r[14] = 0u64 | 0x8000;
r[14] = r[14] << 45;
r[14] = r[14] | 0x1;
r[13] = ((0u32.wrapping_add(0x0000000e)) as i32 as i64 as u64);
r[2] = sx(k.a(c, 0x4a06d0, 0x4a06d4).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[12] = 0x0000000000030000;
r[12] = r[12] | 0x5801;
r[10] = ((0u32.wrapping_add(0x00000047)) as i32 as i64 as u64);
r[11] = 0u64 | 0x9000;
r[11] = r[11] << 46;
r[11] = r[11] | 0x1;
dw(c, (r[2] as u32).wrapping_add(0x00000004), 0u64 as u32);
r[8] = ((0u32.wrapping_add(0x00000010)) as i32 as i64 as u64);
r[9] = ((0u32.wrapping_add(0x00000146)) as i32 as i64 as u64);
r[6] = ((0u32.wrapping_add(0x00000044)) as i32 as i64 as u64);
r[2] = sx(k.a(c, 0x4a0700, 0x4a0704).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[15] = 0u64.wrapping_add(0u64);
dw(c, (r[2] as u32).wrapping_add(0x00000008), 0u64 as u32);
r[3] = sx(k.a(c, 0x4a0710, 0x4a0714).wrapping_sub(0x10e8));
r[3] = c.rd32((r[3] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[3] as u32).wrapping_add(0x0000000c), r[5] as u32);
r[24] = sx(k.a(c, 0x4a071c, 0x4a0720).wrapping_sub(0x10e8));
r[24] = c.rd32((r[24] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[2] = (((r[24] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a0728, 0x4a072c).wrapping_sub(0x10e8));
dw(c, (r[1] as u32).wrapping_add(0x000010e8), r[2] as u32);
c.wr64((r[24] as u32).wrapping_add(0x00000010), r[14]);
c.wr64((r[2] as u32).wrapping_add(0x00000048), r[6]);
c.wr64((r[2] as u32).wrapping_add(0x00000008), r[13]);
c.wr64((r[2] as u32).wrapping_add(0x00000010), r[12]);
c.wr64((r[2] as u32).wrapping_add(0x00000018), r[10]);
c.wr64((r[2] as u32).wrapping_add(0x00000020), r[11]);
c.wr64((r[2] as u32).wrapping_add(0x00000028), r[8]);
c.wr64((r[2] as u32).wrapping_add(0x00000030), r[9]);
c.wr64((r[2] as u32).wrapping_add(0x00000038), r[4]);
let t = (r[25] as i64) <= 0;  c.wr64((r[2] as u32).wrapping_add(0x00000040), r[7]);
 if t { l = 0x004a07bc; continue; }
r[2] = (((r[17] as u32) << 3) as i32 as i64 as u64);
r[5] = 0u64 | 0x8000;
r[4] = (((r[2] as u32).wrapping_add(0x00007ff0)) as i32 as i64 as u64);
r[3] = (((r[16] as u32) << 3) as i32 as i64 as u64);
r[2] = (((r[5] as u32).wrapping_sub((r[2] as u32))) as i32 as i64 as u64);
r[3] = ((0u32.wrapping_sub((r[3] as u32))) as i32 as i64 as u64);
r[8] = r[2] << 16;
r[7] = (((r[3] as u32).wrapping_add((r[5] as u32))) as i32 as i64 as u64);
r[2] = 0u64 | 0x8200;
r[4] = r[4] << 16;
r[6] = (((r[3] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
r[5] = (((r[24] as u32).wrapping_add(0x00000060)) as i32 as i64 as u64);
l = 0x004a0790; continue; }
0x004a0790 => {
r[2] = r[7] | r[8];
r[3] = r[6] | r[4];
c.wr64((r[5] as u32), r[2]);
r[15] = (((r[15] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[5] = (((r[5] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[6] = (((r[6] as u32).wrapping_add(0x00000200)) as i32 as i64 as u64);
c.wr64((r[5] as u32), r[3]);
r[7] = (((r[7] as u32).wrapping_add(0x00000200)) as i32 as i64 as u64);
r[2] = ((r[15] as i64) < (r[25] as i64)) as u64;
let t = r[2] != 0u64;  r[5] = (((r[5] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x004a0790; continue; }
l = 0x004a07bc; continue; }
0x004a07bc => {
r[3] = sx(k.a(c, 0x4a07bc, 0x4a07c0).wrapping_sub(0x10e8));
r[3] = c.rd32((r[3] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[2] = (((r[25] as u32) << 4) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0x00000050)) as i32 as i64 as u64);
r[4] = 0x0000000010000000;
r[3] = (((r[3] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
r[5] = 0x0000000013000000;
r[1] = sx(k.a(c, 0x4a07d8, 0x4a07dc).wrapping_sub(0x10e8));
dw(c, (r[1] as u32).wrapping_add(0x000010e8), r[3] as u32);
dw(c, (r[3] as u32), r[4] as u32);
r[16] = c.rd64((r[29] as u32));
r[2] = sx(k.a(c, 0x4a07e8, 0x4a07ec).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[17] = c.rd64((r[29] as u32).wrapping_add(0x00000008));
dw(c, (r[2] as u32).wrapping_add(0x00000004), 0u64 as u32);
r[3] = sx(k.a(c, 0x4a07f8, 0x4a07fc).wrapping_sub(0x10e8));
r[3] = c.rd32((r[3] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[3] as u32).wrapping_add(0x00000008), r[5] as u32);
r[4] = sx(k.a(c, 0x4a0804, 0x4a0808).wrapping_sub(0x10e8));
r[4] = c.rd32((r[4] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
dw(c, (r[4] as u32).wrapping_add(0x0000000c), 0u64 as u32);
r[2] = sx(k.a(c, 0x4a0810, 0x4a0814).wrapping_sub(0x10e8));
r[2] = c.rd32((r[2] as u32).wrapping_add(0x000010e8)) as i32 as i64 as u64;
r[2] = (((r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a081c, 0x4a0820).wrapping_sub(0x10e8));
dw(c, (r[1] as u32).wrapping_add(0x000010e8), r[2] as u32);
r[29] = (((r[29] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a0348(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a0348;
loop { match l {
0x004a0348 => {
r[29] = (((r[29] as u32).wrapping_add(0xffffffd0)) as i32 as i64 as u64);
r[5] = r[4].wrapping_add(0u64);
c.wr64((r[29] as u32).wrapping_add(0x00000010), r[16]);
dw(c, (r[29] as u32).wrapping_add(0x00000020), c.f[20]);
r[16] = sx(k.a(c, 0x4a0358, 0x4a035c).wrapping_sub(0xdf0));
r[16] = (((r[16] as u32).wrapping_add(0x00000df0)) as i32 as i64 as u64);
c.wr64((r[29] as u32).wrapping_add(0x00000018), r[31]);
c.f[20] = c.f[12];
{ let (a, b) = c.rd128((r[4] as u32)); r[2] = a; h[2] = b; }
r[4] = r[29].wrapping_add(0u64);
r[31] = k.pc(0x4a0378) as u64; c.wr128((r[16] as u32), r[2], h[2]);
 { let k2 = k.call(c, 0x4a0370, 0x40be48); f0040be48(c, k2, r, h); }
r[5] = r[16].wrapping_add(0u64);
r[4] = sx(k.a(c, 0x4a037c, 0x4a0380).wrapping_sub(0xe10));
r[4] = (((r[4] as u32).wrapping_add(0x00000e10)) as i32 as i64 as u64);
r[31] = k.pc(0x4a038c) as u64; c.f[12] = c.f[20];
 { let k2 = k.call(c, 0x4a0384, 0x40be90); f0040be90(c, k2, r, h); }
c.f[2] = c.rd32((r[29] as u32));
c.f[1] = c.rd32((r[29] as u32).wrapping_add(0x00000004));
c.set_fc(uycpu::fpu::lt(c.f[2], c.f[1]));
let t = c.fcr31 & 0x0080_0000 == 0;  c.f[0] = c.rd32((r[29] as u32).wrapping_add(0x00000008));
 if t { l = 0x004a03d4; continue; }
c.set_fc(uycpu::fpu::lt(c.f[2], c.f[0]));
let t = c.fcr31 & 0x0080_0000 == 0;  if t { dw(c, (r[28] as u32).wrapping_add(k.lo(c, 0x4a03b0)), c.f[1]);
 l = 0x004a0404; continue; }
r[1] = sx(k.a(c, 0x4a03b4, 0x4a03b8).wrapping_sub(0xe00));
dw(c, (r[1] as u32).wrapping_add(0x00000e00), c.f[2]);
r[1] = sx(k.a(c, 0x4a03bc, 0x4a03c0).wrapping_sub(0xe04));
dw(c, (r[1] as u32).wrapping_add(0x00000e04), c.f[0]);
r[1] = sx(k.a(c, 0x4a03c4, 0x4a03c8).wrapping_sub(0xe08));
dw(c, (r[1] as u32).wrapping_add(0x00000e08), c.f[1]);
let t = 0u64 == 0u64;   if t { l = 0x004a0414; continue; }
l = 0x004a03d4; continue; }
0x004a03d4 => {
c.set_fc(uycpu::fpu::lt(c.f[1], c.f[0]));
let t = c.fcr31 & 0x0080_0000 == 0;  if t { dw(c, (r[28] as u32).wrapping_add(k.lo(c, 0x4a03e0)), c.f[1]);
 l = 0x004a0404; continue; }
r[1] = sx(k.a(c, 0x4a03e4, 0x4a03e8).wrapping_sub(0xe00));
dw(c, (r[1] as u32).wrapping_add(0x00000e00), c.f[0]);
r[1] = sx(k.a(c, 0x4a03ec, 0x4a03f0).wrapping_sub(0xe04));
dw(c, (r[1] as u32).wrapping_add(0x00000e04), c.f[1]);
r[1] = sx(k.a(c, 0x4a03f4, 0x4a03f8).wrapping_sub(0xe08));
dw(c, (r[1] as u32).wrapping_add(0x00000e08), c.f[2]);
let t = 0u64 == 0u64;   if t { l = 0x004a0414; continue; }
l = 0x004a0404; continue; }
0x004a0404 => {
r[1] = sx(k.a(c, 0x4a0404, 0x4a0408).wrapping_sub(0xe04));
dw(c, (r[1] as u32).wrapping_add(0x00000e04), c.f[2]);
r[1] = sx(k.a(c, 0x4a040c, 0x4a0410).wrapping_sub(0xe08));
dw(c, (r[1] as u32).wrapping_add(0x00000e08), c.f[0]);
l = 0x004a0414; continue; }
0x004a0414 => {
r[4] = sx(k.a(c, 0x4a0414, 0x4a0418).wrapping_sub(0xe00));
r[4] = (((r[4] as u32).wrapping_add(0x00000e00)) as i32 as i64 as u64);
r[6] = sx(k.a(c, 0x4a041c, 0x4a0420).wrapping_sub(0xdf0));
r[6] = (((r[6] as u32).wrapping_add(0x00000df0)) as i32 as i64 as u64);
r[31] = k.pc(0x4a042c) as u64; r[5] = r[4].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a0424, 0x40bf20); f0040bf20(c, k2, r, h); }
r[4] = sx(k.a(c, 0x4a042c, 0x4a0430).wrapping_sub(0xe00));
r[4] = (((r[4] as u32).wrapping_add(0x00000e00)) as i32 as i64 as u64);
r[1] = 0x000000003f800000;
c.f[12] = (r[1] as u32);
r[31] = k.pc(0x4a0444) as u64; r[5] = r[4].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a043c, 0x40c0f0); f0040c0f0(c, k2, r, h); }
r[16] = c.rd64((r[29] as u32).wrapping_add(0x00000010));
r[31] = c.rd64((r[29] as u32).wrapping_add(0x00000018));
c.f[20] = c.rd32((r[29] as u32).wrapping_add(0x00000020));
r[29] = (((r[29] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a0458(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a0458;
loop { match l {
0x004a0458 => {
r[29] = (((r[29] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
c.wr64((r[29] as u32), r[16]);
c.wr64((r[29] as u32).wrapping_add(0x00000008), r[31]);
r[16] = r[4].wrapping_add(0u64);
r[3] = c.rd16((r[16] as u32)) as i16 as i64 as u64;
let t = r[3] != 0u64;  r[2] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x004a0484; continue; }
r[31] = k.pc(0x4a047c) as u64; r[16] = (((r[16] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a0474, 0x4a0ea0); f004a0ea0(c, k2, r, h); }
let t = 0u64 == 0u64;  r[2] = r[16].wrapping_add(0u64);
 if t { l = 0x004a049c; continue; }
l = 0x004a0484; continue; }
0x004a0484 => {
let t = r[3] != r[2];  r[2] = r[16].wrapping_add(0u64);
 if t { l = 0x004a049c; continue; }
r[31] = k.pc(0x4a0494) as u64; r[4] = r[16].wrapping_add(0u64);
 { let k2 = k.call(c, 0x4a048c, 0x4a0fe8); f004a0fe8(c, k2, r, h); }
r[16] = (((r[16] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
r[2] = r[16].wrapping_add(0u64);
l = 0x004a049c; continue; }
0x004a049c => {
r[31] = c.rd64((r[29] as u32).wrapping_add(0x00000008));
r[16] = c.rd64((r[29] as u32));
r[29] = (((r[29] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a0ea0(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a0ea0;
loop { match l {
0x004a0ea0 => {
r[12] = c.rd32((r[4] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[4] as u32).wrapping_add(0x00000010)); c.vf[27] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = sx(k.a(c, 0x4a0ea8, 0x4a0eac).wrapping_sub(0xe10));
r[1] = (((r[1] as u32).wrapping_add(0x00000e10)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[10] = 0x0000000070000000;
r[11] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a0ebc, 0x4a0ec0).wrapping_sub(0xdf0));
r[1] = (((r[1] as u32).wrapping_add(0x00000df0)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[30] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<1, 14>(c, 30, 0, c.vf[30]);
c.f[2] = (r[12] as u32);
c.f[2] = uycpu::fpu::cvt_s(c.f[2]);
r[1] = 0x0000000040c80000;
r[1] = r[1] | 0xf5c3;
c.f[1] = (r[1] as u32);
c.f[0] = uycpu::fpu::div(c, c.f[1], c.f[2]);
r[1] = c.f[0] as i32 as i64 as u64;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vcallms(0xdb0, 0x004a0eec);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
vop::<0, 8>(c, 31, 0, b4(c.vf[1][0]));
r[1] = c.f[0] as i32 as i64 as u64;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vcallms(0xda0, 0x004a0f00);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
vop::<0, 4>(c, 31, 0, b4(c.vf[1][0]));
vop::<1, 1>(c, 31, 0, b4(c.vf[31][1]));
vop::<6, 14>(c, 29, 30, b4(c.vf[31][0]));
vop::<6, 8>(c, 1, 30, b4(c.vf[30][0]));
vop::<6, 8>(c, 1, 1, b4(c.vf[31][3]));
vop::<0, 8>(c, 1, 1, b4(c.vf[31][1]));
vop::<6, 4>(c, 1, 30, b4(c.vf[30][0]));
vop::<6, 4>(c, 1, 1, b4(c.vf[31][3]));
vop::<1, 4>(c, 1, 1, b4(c.vf[29][2]));
vop::<6, 2>(c, 1, 30, b4(c.vf[30][0]));
vop::<6, 2>(c, 1, 1, b4(c.vf[31][3]));
vop::<0, 2>(c, 1, 1, b4(c.vf[29][1]));
vop::<6, 8>(c, 2, 30, b4(c.vf[30][1]));
vop::<6, 8>(c, 2, 2, b4(c.vf[31][3]));
vop::<0, 8>(c, 2, 2, b4(c.vf[29][2]));
vop::<6, 4>(c, 2, 30, b4(c.vf[30][1]));
vop::<6, 4>(c, 2, 2, b4(c.vf[31][3]));
vop::<0, 4>(c, 2, 2, b4(c.vf[31][1]));
vop::<6, 2>(c, 2, 30, b4(c.vf[30][1]));
vop::<6, 2>(c, 2, 2, b4(c.vf[31][3]));
vop::<1, 2>(c, 2, 2, b4(c.vf[29][0]));
vop::<6, 8>(c, 3, 30, b4(c.vf[30][2]));
vop::<6, 8>(c, 3, 3, b4(c.vf[31][3]));
vop::<1, 8>(c, 3, 3, b4(c.vf[29][1]));
vop::<6, 4>(c, 3, 30, b4(c.vf[30][2]));
vop::<6, 4>(c, 3, 3, b4(c.vf[31][3]));
vop::<0, 4>(c, 3, 3, b4(c.vf[29][0]));
vop::<6, 2>(c, 3, 30, b4(c.vf[30][2]));
vop::<6, 2>(c, 3, 3, b4(c.vf[31][3]));
vop::<0, 2>(c, 3, 3, b4(c.vf[31][1]));
r[1] = sx(k.a(c, 0x4a0f80, 0x4a0f84).wrapping_sub(0xe00));
r[1] = (((r[1] as u32).wrapping_add(0x00000e00)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[26] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 14>(c, 26, 26, b4(c.vf[27][3]));
l = 0x004a0f90; continue; }
0x004a0f90 => {
vop::<6, 14>(c, ACC, 1, b4(c.vf[26][0]));
vop::<2, 14>(c, ACC, 2, b4(c.vf[26][1]));
vop::<2, 14>(c, 26, 3, b4(c.vf[26][2]));
vop::<0, 14>(c, 5, 26, c.vf[27]);
{ let v = c.vf[5]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vop::<0, 14>(c, 5, 5, c.vf[28]);
{ let v = c.vf[5]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[12] = (((r[12] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[12] != 0u64;  r[11] = (((r[11] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x004a0f90; continue; }
r[1] = 0x0000000070000000;
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[5]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[5]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[11] = (((r[11] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[1] = (((r[28] as u32).wrapping_add(k.lo(c, 0x4a0fdc))) as i32 as i64 as u64);
dw(c, (r[1] as u32), r[11] as u32);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a0fe8(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a0fe8;
loop { match l {
0x004a0fe8 => {
{ let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
r[12] = c.rd32((r[4] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[13] = c.rd32((r[4] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[4] as u32).wrapping_add(0x00000010)); c.vf[27] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[4] as u32).wrapping_add(0x00000020)); c.vf[25] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = sx(k.a(c, 0x4a0ffc, 0x4a1000).wrapping_sub(0xe10));
r[1] = (((r[1] as u32).wrapping_add(0x00000e10)) as i32 as i64 as u64);
l = 0x004a1004; continue; }
0x004a1004 => {
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vmac::<1, 15>(c, 2, 27, c.vf[25]);
r[10] = 0x0000000070000000;
r[11] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a1014, 0x4a1018).wrapping_sub(0xdf0));
l = 0x004a1018; continue; }
0x004a1018 => {
r[1] = (((r[1] as u32).wrapping_add(0x00000df0)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[30] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x004a1020; continue; }
0x004a1020 => {
uycpu::vu0::opmula(c, 30, 2);
l = 0x004a1024; continue; }
0x004a1024 => {
uycpu::vu0::opmsub(c, 24, 2, 30);
vmac::<1, 14>(c, 30, 0, c.vf[30]);
vmac::<0, 14>(c, 20, 0, b4(c.vf[0][3]));
vmac::<6, 14>(c, 1, 24, c.vf[24]);
vmac::<0, 8>(c, ACC, 1, b4(c.vf[1][1]));
vmac::<2, 8>(c, 21, 20, b4(c.vf[1][2]));
uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[21][0]);
vmac::<0, 8>(c, 21, 0, b4(c.vi[22]));
vmac::<1, 1>(c, 0, 2, b4(c.vf[21][0]));
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[21][0]);
r[1] = c.cfc2(17) as i32 as i64 as u64;
vmac::<1, 1>(c, 1, 27, c.vf[25]);
r[1] = r[1] & 0x10;
let t = r[1] == 0u64;  vmac::<0, 8>(c, 1, 0, b4(c.vf[1][3]));
 if t { l = 0x004a11b8; continue; }
r[2] = 0x000000003fc80000;
r[2] = r[2] | 0xf5c3;
c.vf[16] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
r[8] = c.cfc2(17) as i32 as i64 as u64;
r[8] = r[8] & 0x1;
let t = r[8] != 0u64;  vmac::<6, 14>(c, 24, 24, b4(c.vi[22]));
 if t { l = 0x004a10a4; continue; }
vmac::<6, 8>(c, 1, 1, b4(c.vi[22]));
c.vcallms(0xf10, 0x004a1098);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
vmac::<1, 8>(c, 16, 16, c.vf[1]);
l = 0x004a10a4; continue; }
0x004a10a4 => {
r[1] = 0x0000000040480000;
r[1] = r[1] | 0xf5c3;
c.vf[2] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vmac::<1, 8>(c, 1, 2, c.vf[16]);
vmac::<0, 8>(c, 17, 1, c.vf[1]);
vmac::<0, 8>(c, 18, 16, c.vf[17]);
vmac::<0, 8>(c, 19, 16, c.vf[16]);
vmac::<6, 14>(c, 22, 24, b4(c.vf[27][3]));
vmac::<6, 14>(c, 23, 24, b4(c.vf[25][3]));
let t = r[12] == 0u64;  r[1] = 0x000000003fc80000;
 if t { l = 0x004a1114; continue; }
c.vf[5] = [r[12] as u32, (r[12] >> 32) as u32, h[12] as u32, (h[12] >> 32) as u32];
uycpu::vu0::itof::<0, 15>(c, 5, 5);
r[1] = r[1] | 0xf5c3;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vmac::<0, 8>(c, 1, 1, c.vf[16]);
r[31] = k.pc(0x4a10ec) as u64; uycpu::vu0::qop::<0>(c, c.vf[17][0], c.vf[5][0]);
 { let k2 = k.call(c, 0x4a10e4, 0x4a1228); f004a1228(c, k2, r, h); }
vmac::<6, 14>(c, ACC, 3, b4(c.vf[22][2]));
vmac::<2, 14>(c, ACC, 2, b4(c.vf[22][1]));
vmac::<2, 14>(c, 26, 1, b4(c.vf[22][0]));
r[31] = k.pc(0x4a1100) as u64; vmac::<0, 8>(c, 1, 0, b4(c.vi[22]));
 { let k2 = k.call(c, 0x4a10f8, 0x4a1228); f004a1228(c, k2, r, h); }
uycpu::vu0::mov::<15>(c, 4, c.vf[27]);
r[31] = k.pc(0x4a110c) as u64; r[8] = (((r[12] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a1104, 0x4a1274); f004a1274(c, k2, r, h); }
let t = 0u64 == 0u64;   if t { l = 0x004a113c; continue; }
l = 0x004a1114; continue; }
0x004a1114 => {
vmac::<1, 14>(c, 1, 27, c.vf[22]);
{ let v = c.vf[1]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vmac::<0, 14>(c, 1, 1, c.vf[28]);
{ let v = c.vf[1]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vmac::<0, 14>(c, 1, 27, c.vf[22]);
{ let v = c.vf[1]; c.wr128((r[10] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vmac::<0, 14>(c, 1, 1, c.vf[28]);
{ let v = c.vf[1]; c.wr128((r[10] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[10] = (((r[10] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
r[11] = (((r[11] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
l = 0x004a113c; continue; }
0x004a113c => {
let t = r[13] == 0u64;  r[1] = 0x0000000040960000;
 if t { l = 0x004a118c; continue; }
c.vf[5] = [r[13] as u32, (r[13] >> 32) as u32, h[13] as u32, (h[13] >> 32) as u32];
uycpu::vu0::itof::<0, 15>(c, 5, 5);
r[1] = r[1] | 0xcbe3;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vmac::<1, 8>(c, 1, 18, c.vf[1]);
r[31] = k.pc(0x4a1160) as u64; uycpu::vu0::qop::<0>(c, c.vf[19][0], c.vf[5][0]);
 { let k2 = k.call(c, 0x4a1158, 0x4a1228); f004a1228(c, k2, r, h); }
vmac::<6, 14>(c, ACC, 3, b4(c.vf[23][2]));
vmac::<2, 14>(c, ACC, 2, b4(c.vf[23][1]));
vmac::<2, 14>(c, 26, 1, b4(c.vf[23][0]));
vmac::<0, 8>(c, 1, 0, b4(c.vi[22]));
r[31] = k.pc(0x4a1178) as u64;  { let k2 = k.call(c, 0x4a1170, 0x4a1228); f004a1228(c, k2, r, h); }
uycpu::vu0::mov::<15>(c, 4, c.vf[25]);
r[31] = k.pc(0x4a1184) as u64; r[8] = (((r[13] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a117c, 0x4a1274); f004a1274(c, k2, r, h); }
let t = 0u64 == 0u64;   if t { l = 0x004a1200; continue; }
l = 0x004a118c; continue; }
0x004a118c => {
vmac::<0, 14>(c, 1, 25, c.vf[23]);
{ let v = c.vf[1]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vmac::<0, 14>(c, 1, 1, c.vf[28]);
{ let v = c.vf[1]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vmac::<1, 14>(c, 1, 25, c.vf[23]);
{ let v = c.vf[1]; c.wr128((r[10] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vmac::<0, 14>(c, 1, 1, c.vf[28]);
{ let v = c.vf[1]; c.wr128((r[10] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[10] = (((r[10] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
let t = 0u64 == 0u64;  r[11] = (((r[11] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
 if t { l = 0x004a1200; continue; }
l = 0x004a11b8; continue; }
0x004a11b8 => {
let t = r[12] == 0u64;   if t { l = 0x004a1200; continue; }
c.f[2] = (r[12] as u32);
c.f[2] = uycpu::fpu::cvt_s(c.f[2]);
r[1] = 0x0000000040c80000;
r[1] = r[1] | 0xf5c3;
c.f[1] = (r[1] as u32);
c.f[0] = uycpu::fpu::div(c, c.f[1], c.f[2]);
r[2] = c.f[0] as i32 as i64 as u64;
r[31] = k.pc(0x4a11e4) as u64; c.vf[1] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
 { let k2 = k.call(c, 0x4a11dc, 0x4a1228); f004a1228(c, k2, r, h); }
r[1] = sx(k.a(c, 0x4a11e4, 0x4a11e8).wrapping_sub(0xe00));
r[1] = (((r[1] as u32).wrapping_add(0x00000e00)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[26] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vmac::<6, 14>(c, 26, 26, b4(c.vf[27][3]));
uycpu::vu0::mov::<15>(c, 4, c.vf[27]);
r[31] = k.pc(0x4a1200) as u64; r[8] = (((r[12] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a11f8, 0x4a1274); f004a1274(c, k2, r, h); }
l = 0x004a1200; continue; }
0x004a1200 => {
r[1] = 0x0000000070000000;
{ let (a, b) = c.rd128((r[1] as u32)); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000010)); r[9] = a; h[9] = b; }
r[11] = (((r[11] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr128((r[10] as u32), r[8], h[8]);
c.wr128((r[10] as u32).wrapping_add(0x00000010), r[9], h[9]);
r[1] = (((r[28] as u32).wrapping_add(k.lo(c, 0x4a121c))) as i32 as i64 as u64);
dw(c, (r[1] as u32), r[11] as u32);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1228(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a1228;
loop { match l {
0x004a1228 => {
c.vcallms(0xda0, 0x004a1228);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
vop::<6, 14>(c, 29, 30, b4(c.vf[1][1]));
vop::<0, 14>(c, ACC, 0, b4(c.vf[0][3]));
vop::<3, 14>(c, 5, 30, b4(c.vf[1][0]));
vop::<0, 2>(c, ACC, 0, b4(c.vf[1][0]));
vop::<1, 8>(c, ACC, 0, b4(c.vf[29][1]));
vop::<0, 4>(c, ACC, 0, b4(c.vf[29][0]));
vop::<2, 14>(c, 3, 5, b4(c.vf[30][2]));
vop::<0, 4>(c, ACC, 0, b4(c.vf[1][0]));
vop::<0, 8>(c, ACC, 0, b4(c.vf[29][2]));
vop::<1, 2>(c, ACC, 0, b4(c.vf[29][0]));
vop::<2, 14>(c, 2, 5, b4(c.vf[30][1]));
vop::<0, 8>(c, ACC, 0, b4(c.vf[1][0]));
vop::<1, 4>(c, ACC, 0, b4(c.vf[29][2]));
vop::<0, 2>(c, ACC, 0, b4(c.vf[29][1]));
vop::<2, 14>(c, 1, 5, b4(c.vf[30][0]));
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1274(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a1274;
loop { match l {
0x004a1274 => {
vop::<0, 14>(c, 5, 26, c.vf[4]);
vop::<6, 14>(c, ACC, 1, b4(c.vf[26][0]));
r[11] = (((r[11] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
vop::<2, 14>(c, ACC, 2, b4(c.vf[26][1]));
vop::<2, 14>(c, 26, 3, b4(c.vf[26][2]));
vop::<0, 14>(c, 6, 5, c.vf[28]);
{ let v = c.vf[5]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[8] = (((r[8] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[8] != 0u64;  { let v = c.vf[6]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64); l = 0x004a1274; continue; }
r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a12a8(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a12a8;
loop { match l {
0x004a12a8 => {
{ let (a, b) = c.rd128((r[4] as u32)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[10] = 0x0000000070000000;
{ let (a, b) = c.rd128((r[5] as u32)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[10] = (((r[10] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
r[1] = (((r[28] as u32).wrapping_add(k.lo(c, 0x4a12b8))) as i32 as i64 as u64);
r[11] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
vop::<0, 8>(c, 12, 0, b4(c.vf[11][1]));
vop::<0, 8>(c, 13, 0, b4(c.vf[11][2]));
l = 0x004a12c8; continue; }
0x004a12c8 => {
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000030)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
vop::<1, 14>(c, 3, 2, c.vf[1]);
vop::<6, 8>(c, ACC, 11, b4(c.vf[1][0]));
vop::<2, 8>(c, ACC, 12, b4(c.vf[1][1]));
vop::<2, 8>(c, 4, 13, b4(c.vf[1][2]));
vop::<6, 8>(c, ACC, 11, b4(c.vf[3][0]));
vop::<2, 8>(c, ACC, 12, b4(c.vf[3][1]));
vop::<2, 8>(c, 7, 13, b4(c.vf[3][2]));
vop::<6, 8>(c, ACC, 11, b4(c.vf[2][0]));
vop::<2, 8>(c, ACC, 12, b4(c.vf[2][1]));
vop::<2, 8>(c, 5, 13, b4(c.vf[2][2]));
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[7][0]);
vop::<6, 14>(c, ACC, 3, b4(c.vf[10][3]));
vop::<2, 14>(c, 4, 3, b4(c.vf[4][0]));
vop::<6, 14>(c, ACC, 3, b4(c.vf[11][3]));
vop::<2, 14>(c, 5, 3, b4(c.vf[5][0]));
vop::<0, 14>(c, ACC, 1, b4(c.vf[0][0]));
vop::<3, 14>(c, 1, 4, b4(c.vi[22]));
vop::<0, 14>(c, ACC, 2, b4(c.vf[0][0]));
vop::<3, 14>(c, 2, 5, b4(c.vi[22]));
{ let v = c.vf[1]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
let t = r[11] != 0u64;  { let v = c.vf[2]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x004a12c8; continue; }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1340(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
    f004a1340_at(c, k, r, h, 0x4a1340)
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1340_at(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32], at: u32) {
let mut l: u32 = at;
loop { match l {
0x004a1340 => {
r[1] = sx(k.a(c, 0x4a1340, 0x4a1344).wrapping_sub(0x2900));
r[1] = (((r[1] as u32).wrapping_add(0x00002900)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000c0)); c.vf[20] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000d0)); c.vf[21] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000e0)); c.vf[22] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000f0)); c.vf[23] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000140)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = sx(k.a(c, 0x4a135c, 0x4a1360).wrapping_sub(0x7600));
r[1] = (((r[1] as u32).wrapping_add(0x00007600)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000190)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000001a0)); c.vf[29] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[28] as u32).wrapping_add(k.lo(c, 0x4a136c))); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = 0x0000000044800000;
c.vf[11] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vop::<6, 15>(c, 24, 20, b4(c.vf[11][0]));
vop::<6, 15>(c, 25, 21, b4(c.vf[11][0]));
vop::<6, 15>(c, 26, 22, b4(c.vf[11][0]));
vop::<0, 15>(c, ACC, 23, b4(c.vf[0][0]));
vop::<3, 15>(c, ACC, 24, b4(c.vf[10][0]));
vop::<3, 15>(c, ACC, 25, b4(c.vf[10][1]));
vop::<3, 15>(c, 27, 26, b4(c.vf[10][2]));
vop::<6, 14>(c, ACC, 28, c.vf[24]);
vop::<2, 14>(c, 24, 29, b4(c.vf[24][3]));
vop::<6, 14>(c, ACC, 28, c.vf[25]);
vop::<2, 14>(c, 25, 29, b4(c.vf[25][3]));
vop::<6, 14>(c, ACC, 28, c.vf[26]);
vop::<2, 14>(c, 26, 29, b4(c.vf[26][3]));
vop::<6, 14>(c, ACC, 28, c.vf[27]);
vop::<2, 14>(c, 27, 29, b4(c.vf[27][3]));
r[11] = r[5].wrapping_add(0u64);
r[10] = r[4].wrapping_add(0u64);
r[10] = (((r[10] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
vop::<6, 15>(c, ACC, 27, b4(c.vf[0][3]));
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<2, 15>(c, ACC, 26, b4(c.vf[5][2]));
vop::<2, 15>(c, ACC, 25, b4(c.vf[5][1]));
vop::<2, 15>(c, 7, 24, b4(c.vf[5][0]));
vop::<6, 15>(c, ACC, 27, b4(c.vf[0][3]));
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[7][3]);
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000030)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<2, 15>(c, ACC, 26, b4(c.vf[6][2]));
vop::<2, 15>(c, ACC, 25, b4(c.vf[6][1]));
vop::<2, 15>(c, 8, 24, b4(c.vf[6][0]));
l = 0x004a13f0; continue; }
0x004a13f0 => {
vop::<6, 15>(c, ACC, 27, b4(c.vf[0][3]));
vop::<6, 14>(c, 9, 7, b4(c.vi[22]));
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000040)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[8][3]);
vop::<2, 15>(c, ACC, 26, b4(c.vf[5][2]));
vop::<2, 15>(c, ACC, 25, b4(c.vf[5][1]));
vop::<2, 15>(c, 7, 24, b4(c.vf[5][0]));
{ let v = c.vf[9]; c.wr128((r[10] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vop::<6, 15>(c, ACC, 27, b4(c.vf[0][3]));
vop::<6, 14>(c, 9, 8, b4(c.vi[22]));
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000050)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[7][3]);
vop::<2, 15>(c, ACC, 26, b4(c.vf[6][2]));
vop::<2, 15>(c, ACC, 25, b4(c.vf[6][1]));
r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
vop::<2, 15>(c, 8, 24, b4(c.vf[6][0]));
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[11] != 0u64;  { let v = c.vf[9]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x004a13f0; continue; }
l = 0x004a1450; continue; }
0x004a1450 => {
r[10] = r[4].wrapping_add(0u64);
r[11] = r[5].wrapping_add(0u64);
r[12] = 0x0000000070000000;
r[12] = r[12] | 0x3000;
{ let (a, b) = c.rd128((r[10] as u32)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000040)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000010)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000030)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000050)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<1, 12>(c, 2, 2, c.vf[7]);
vop::<1, 12>(c, 3, 3, c.vf[7]);
vop::<1, 12>(c, 5, 5, c.vf[4]);
vop::<1, 12>(c, 6, 6, c.vf[4]);
vop::<6, 8>(c, ACC, 3, b4(c.vf[2][1]));
vop::<3, 8>(c, 1, 2, b4(c.vf[3][1]));
vop::<6, 4>(c, ACC, 5, b4(c.vf[6][0]));
vop::<3, 4>(c, 1, 6, b4(c.vf[5][0]));
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
c.wr64((r[28] as u32).wrapping_add(k.lo(c, 0x4a149c)), r[1]);
r[1] = 0x00000000493f0000;
r[1] = r[1] | 0xfff8;
c.vf[31] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
l = 0x004a14ac; continue; }
0x004a14ac => {
uycpu::vu0::mov::<15>(c, 5, c.vf[7]);
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
vop::<1, 15>(c, 8, 5, c.vf[6]);
vop::<1, 15>(c, 9, 6, c.vf[7]);
vop::<6, 1>(c, 5, 0, b4(c.vf[5][2]));
vop::<6, 1>(c, 6, 0, b4(c.vf[6][2]));
vop::<6, 4>(c, ACC, 8, b4(c.vf[9][0]));
vop::<3, 4>(c, 10, 9, b4(c.vf[8][0]));
vop::<0, 12>(c, 3, 5, c.vf[18]);
vop::<0, 12>(c, 4, 6, c.vf[18]);
vop::<0, 12>(c, 1, 5, b4(c.vf[31][0]));
vop::<0, 12>(c, 2, 6, b4(c.vf[31][0]));
{ let (a, b) = { let v = c.vf[10]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
uycpu::vu0::ftoi::<0, 1>(c, 3, 5);
r[1] = ((r[1] as i64) < (0u64 as i64)) as u64;
uycpu::vu0::ftoi::<0, 1>(c, 4, 6);
r[1] = (((r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr8((r[12] as u32), r[1] as u8);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
r[12] = (((r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pexch((r[1], h[1])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pexch((r[2], h[2])); r[9] = a; h[9] = b; }
{ let v = c.vf[3]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[4]; c.wr128((r[10] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
dw(c, (r[10] as u32).wrapping_add(0x00000008), r[8] as u32);
dw(c, (r[10] as u32).wrapping_add(0x00000018), r[9] as u32);
let t = r[11] != 0u64;  r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 if t { l = 0x004a14ac; continue; }
r[2] = ((0u32.wrapping_add(0u32)) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1530(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a1530;
loop { match l {
0x004a1530 => {
r[1] = sx(k.a(c, 0x4a1530, 0x4a1534).wrapping_sub(0x2900));
r[1] = (((r[1] as u32).wrapping_add(0x00002900)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000c0)); c.vf[20] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000d0)); c.vf[21] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000e0)); c.vf[22] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000000f0)); c.vf[23] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000140)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = 0x0000000044800000;
c.vf[11] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vop::<6, 15>(c, 24, 20, b4(c.vf[11][0]));
vop::<6, 15>(c, 25, 21, b4(c.vf[11][0]));
vop::<6, 15>(c, 26, 22, b4(c.vf[11][0]));
vop::<0, 15>(c, ACC, 23, b4(c.vf[0][0]));
vop::<3, 15>(c, ACC, 24, b4(c.vf[10][0]));
vop::<3, 15>(c, ACC, 25, b4(c.vf[10][1]));
vop::<3, 15>(c, 27, 26, b4(c.vf[10][2]));
r[10] = r[4].wrapping_add(0u64);
r[11] = (((r[5] as u32).wrapping_add((r[5] as u32))) as i32 as i64 as u64);
r[2] = ((0u32.wrapping_add(0u32)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[10] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<6, 15>(c, ACC, 24, b4(c.vf[1][0]));
vop::<2, 15>(c, ACC, 25, b4(c.vf[1][1]));
vop::<2, 15>(c, ACC, 26, b4(c.vf[1][2]));
vop::<2, 15>(c, 2, 27, b4(c.vf[0][3]));
l = 0x004a1590; continue; }
0x004a1590 => {
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000010)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::clip(c, 2, 2);
{ let v = c.vf[2]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
vop::<6, 15>(c, ACC, 24, b4(c.vf[1][0]));
vop::<2, 15>(c, ACC, 25, b4(c.vf[1][1]));
vop::<2, 15>(c, ACC, 26, b4(c.vf[1][2]));
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
vop::<2, 15>(c, 2, 27, b4(c.vf[0][3]));
r[10] = (((r[10] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[1] = c.cfc2(18) as i32 as i64 as u64;
let t = r[11] != 0u64;  r[2] = r[2] | r[1];
 if t { l = 0x004a1590; continue; }
r[2] = r[2] & 0x3f;
let t = r[2] == 0u64;   if t { l = 0x004a1690; continue; }
r[10] = r[4].wrapping_add(0u64);
r[11] = r[5].wrapping_add(0u64);
r[12] = 0x0000000070000000;
r[12] = r[12] | 0x3000;
{ let (a, b) = c.rd128((r[10] as u32)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000040)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000010)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000030)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000050)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[7]; uycpu::vu0::mov::<2>(c, 7, [v[1], v[2], v[3], v[0]]); }
{ let v = c.vf[2]; uycpu::vu0::mov::<2>(c, 2, [v[1], v[2], v[3], v[0]]); }
{ let v = c.vf[3]; uycpu::vu0::mov::<2>(c, 3, [v[1], v[2], v[3], v[0]]); }
{ let v = c.vf[4]; uycpu::vu0::mov::<2>(c, 4, [v[1], v[2], v[3], v[0]]); }
{ let v = c.vf[5]; uycpu::vu0::mov::<2>(c, 5, [v[1], v[2], v[3], v[0]]); }
{ let v = c.vf[6]; uycpu::vu0::mov::<2>(c, 6, [v[1], v[2], v[3], v[0]]); }
uycpu::vu0::opmula(c, 2, 7);
uycpu::vu0::opmsub(c, 1, 7, 2);
uycpu::vu0::opmula(c, 5, 4);
uycpu::vu0::opmsub(c, 2, 4, 5);
vop::<6, 15>(c, 1, 1, c.vf[3]);
vop::<6, 15>(c, 2, 2, c.vf[6]);
vop::<4, 15>(c, 11, 0, b4(c.vf[0][3]));
vop::<0, 8>(c, ACC, 1, b4(c.vf[1][1]));
vop::<2, 8>(c, 1, 11, b4(c.vf[1][2]));
vop::<0, 4>(c, ACC, 2, b4(c.vf[2][0]));
vop::<2, 4>(c, 1, 11, b4(c.vf[2][2]));
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
c.wr64((r[28] as u32).wrapping_add(k.lo(c, 0x4a163c)), r[1]);
l = 0x004a1640; continue; }
0x004a1640 => {
uycpu::vu0::mov::<15>(c, 5, c.vf[7]);
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000010)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let v = c.vf[6]; uycpu::vu0::mov::<2>(c, 6, [v[1], v[2], v[3], v[0]]); }
{ let v = c.vf[7]; uycpu::vu0::mov::<2>(c, 7, [v[1], v[2], v[3], v[0]]); }
uycpu::vu0::opmula(c, 6, 5);
uycpu::vu0::opmsub(c, 1, 5, 6);
vop::<6, 15>(c, 1, 1, c.vf[7]);
vop::<0, 4>(c, ACC, 1, b4(c.vf[1][0]));
vop::<2, 4>(c, 1, 11, b4(c.vf[1][2]));
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = ((r[1] as i64) < (0u64 as i64)) as u64;
r[1] = (((r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr8((r[12] as u32), r[1] as u8);
r[12] = (((r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = r[11] != 0u64;  r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 if t { l = 0x004a1640; continue; }
 return;
}
0x004a1690 => {
r[1] = sx(k.a(c, 0x4a1690, 0x4a1694).wrapping_sub(0x7600));
r[1] = (((r[1] as u32).wrapping_add(0x00007600)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000190)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000001a0)); c.vf[29] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[28] as u32).wrapping_add(k.lo(c, 0x4a16a0))); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[10] = r[4].wrapping_add(0u64);
r[11] = (((r[5] as u32).wrapping_add((r[5] as u32))) as i32 as i64 as u64);
l = 0x004a16ac; continue; }
0x004a16ac => {
{ let (a, b) = c.rd128((r[10] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[1][3]);
vop::<6, 14>(c, 1, 1, b4(c.vi[22]));
vop::<6, 14>(c, ACC, 29, b4(c.vf[0][3]));
vop::<2, 14>(c, 1, 1, c.vf[28]);
{ let v = c.vf[1]; c.wr128((r[10] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[10] = (((r[10] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
let t = r[11] != 0u64;   if t { l = 0x004a16ac; continue; }
f004a1340_at(c, F { at: k.pc(0x4a1340), l1: 0x4a1340 }, r, h, 0x4a1450); return; }
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a16f8(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
    f004a16f8_at(c, k, r, h, 0x4a16f8)
}

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a16f8_at(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32], at: u32) {
let mut l: u32 = at;
loop { match l {
0x004a16f8 => {
r[8] = sx(k.a(c, 0x4a16f8, 0x4a16fc).wrapping_sub(0x17b4));
r[8] = (((r[8] as u32).wrapping_add(0x000017b4)) as i32 as i64 as u64);
r[6] = ((0u32.wrapping_add(0x00000020)) as i32 as i64 as u64);
l = 0x004a1704; continue; }
0x004a1704 => {
r[5] = (((r[5] as u32) << 4) as i32 as i64 as u64);
r[5] = (((r[5] as u32).wrapping_add((r[4] as u32))) as i32 as i64 as u64);
l = 0x004a170c; continue; }
0x004a170c => {
{ let (a, b) = c.rd128((r[5] as u32).wrapping_add(0xfffffff0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::clip(c, 2, 2);
{ let (a, b) = c.rd128((r[4] as u32)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[2] = (((r[4] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
r[3] = (((r[4] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
r[1] = c.cfc2(18) as i32 as i64 as u64;
r[7] = r[1] & r[6];
l = 0x004a173c; continue; }
0x004a173c => {
uycpu::vu0::clip(c, 3, 3);
uycpu::vu0::mov::<15>(c, 1, c.vf[2]);
uycpu::vu0::mov::<15>(c, 2, c.vf[3]);
r[2] = (((r[2] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
let t = (r[8] as u32); r[9] = k.pc(0x4a1754) as u64; { let (a, b) = c.rd128((r[2] as u32)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 clip_plane(c, t.wrapping_sub(k.pc(0x4a17b4)) / 12);
let t = r[7] == 0u64;  r[1] = c.cfc2(18) as i32 as i64 as u64;
 if t { l = 0x004a1768; continue; }
vop::<6, 15>(c, ACC, 1, b4(c.vf[5][3]));
let t = 0u64 == 0u64;  vop::<3, 15>(c, 6, 2, b4(c.vf[4][3]));
 if t { l = 0x004a1778; continue; }
l = 0x004a1768; continue; }
0x004a1768 => {
{ let v = c.vf[1]; c.wr128((r[3] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
vop::<6, 15>(c, ACC, 2, b4(c.vf[4][3]));
vop::<3, 15>(c, 6, 1, b4(c.vf[5][3]));
l = 0x004a1778; continue; }
0x004a1778 => {
r[1] = r[1] & r[6];
let t = r[1] == r[7];  r[7] = (((r[1] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
 if t { l = 0x004a178c; continue; }
{ let v = c.vf[6]; c.wr128((r[3] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
l = 0x004a178c; continue; }
0x004a178c => {
let t = r[2] != r[5];   if t { l = 0x004a173c; continue; }
let t = r[3] == r[4];  r[5] = (((r[3] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x004a17a8; continue; }
r[6] = (((r[6] as u32) >> 1) as i32 as i64 as u64);
let t = r[6] != 0u64;  r[8] = (((r[8] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64);
 if t { l = 0x004a170c; continue; }
l = 0x004a17a8; continue; }
0x004a17a8 => {
r[2] = (((r[3] as u32).wrapping_sub((r[4] as u32))) as i32 as i64 as u64);
r[2] = (((r[2] as u32) >> 4) as i32 as i64 as u64);
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1800(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a1800;
loop { match l {
0x004a1800 => {
{ let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
r[11] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1804))) as i32 as i64 as u64;
r[13] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1808))) as i32 as i64 as u64;
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[10] = 0x0000000070000000;
let t = (r[11] as i64) <= 0;   if t { l = 0x004a191c; continue; }
r[1] = 0u64 | 0x1;
dw(c, (r[13] as u32), r[1] as u32);
r[1] = 0x0000000010000000;
dw(c, (r[13] as u32).wrapping_add(0x00000004), r[1] as u32);
r[1] = ((0u32.wrapping_add(0x0000000e)) as i32 as i64 as u64);
dw(c, (r[13] as u32).wrapping_add(0x00000008), r[1] as u32);
r[1] = ((0u32.wrapping_add(0x0000003f)) as i32 as i64 as u64);
dw(c, (r[13] as u32).wrapping_add(0x00000018), r[1] as u32);
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[15] = r[13].wrapping_add(0u64);
r[13] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
r[12] = 0x0000000070000000;
r[1] = c.rd8((r[12] as u32).wrapping_add(0x00003000)) as i8 as i64 as u64;
r[11] = (((r[12] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
l = 0x004a1854; continue; }
0x004a1818 => {
r[1] = 0u64 | 0x1;
dw(c, (r[13] as u32), r[1] as u32);
r[1] = 0x0000000010000000;
dw(c, (r[13] as u32).wrapping_add(0x00000004), r[1] as u32);
r[1] = ((0u32.wrapping_add(0x0000000e)) as i32 as i64 as u64);
dw(c, (r[13] as u32).wrapping_add(0x00000008), r[1] as u32);
r[1] = ((0u32.wrapping_add(0x0000003f)) as i32 as i64 as u64);
dw(c, (r[13] as u32).wrapping_add(0x00000018), r[1] as u32);
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[15] = r[13].wrapping_add(0u64);
r[13] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
r[12] = 0x0000000070000000;
r[1] = c.rd8((r[12] as u32).wrapping_add(0x00003000)) as i8 as i64 as u64;
r[11] = (((r[12] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
l = 0x004a1854; continue; }
0x004a1854 => {
let t = r[1] != r[4];  { let (a, b) = c.rd128((r[10] as u32)); r[2] = a; h[2] = b; }
 if t { l = 0x004a187c; continue; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000010)); r[3] = a; h[3] = b; }
c.wr128((r[13] as u32), r[2], h[2]);
c.wr128((r[13] as u32).wrapping_add(0x00000010), r[3], h[3]);
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); r[2] = a; h[2] = b; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000030)); r[3] = a; h[3] = b; }
c.wr128((r[13] as u32).wrapping_add(0x00000020), r[2], h[2]);
c.wr128((r[13] as u32).wrapping_add(0x00000030), r[3], h[3]);
r[13] = (((r[13] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
l = 0x004a187c; continue; }
0x004a187c => {
r[1] = c.rd8((r[12] as u32).wrapping_add(0x00003001)) as i8 as i64 as u64;
r[12] = (((r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = r[12] != r[11];  r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 if t { l = 0x004a1854; continue; }
r[8] = (((r[13] as u32).wrapping_sub((r[15] as u32))) as i32 as i64 as u64);
r[8] = (((r[8] as u32).wrapping_add(0xfffffff4)) as i32 as i64 as u64);
r[8] = (((r[8] as u32) >> 6) as i32 as i64 as u64);
r[1] = 0u64 | 0x1;
dw(c, (r[15] as u32), r[1] as u32);
c.wr64((r[15] as u32).wrapping_add(0x00000008), r[1]);
r[1] = 0x0000000024000000;
dw(c, (r[15] as u32).wrapping_add(0x00000004), r[1] as u32);
dw(c, (r[15] as u32).wrapping_add(0x00000010), r[5] as u32);
r[1] = 0x0000000044000000;
dw(c, (r[15] as u32).wrapping_add(0x00000014), r[1] as u32);
r[1] = 0u64 | 0x54;
c.wr64((r[15] as u32).wrapping_add(0x00000018), r[1]);
dw(c, (r[15] as u32).wrapping_add(0x00000020), r[8] as u32);
r[1] = 0xffffffff84000000;
dw(c, (r[15] as u32).wrapping_add(0x00000024), r[1] as u32);
r[1] = 0x0000000052520000;
r[1] = r[1] | 0xd2d2;
c.wr64((r[15] as u32).wrapping_add(0x00000028), r[1]);
r[8] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a18d8))) as i32 as i64 as u64;
r[9] = sx(k.a(c, 0x4a18dc, 0x4a18e0).wrapping_sub(0xe3c));
r[9] = c.rd32((r[9] as u32).wrapping_add(0x00000e3c)) as i32 as i64 as u64;
r[2] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
let t = r[4] != r[2];   if t { l = 0x004a18f8; continue; }
r[8] = ((0u32.wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
r[9] = ((0u32.wrapping_sub((r[9] as u32))) as i32 as i64 as u64);
l = 0x004a18f8; continue; }
0x004a18f8 => {
let t = (r[8] as i64) < 0;  r[10] = 0x0000000070000000;
 if t { l = 0x004a1908; continue; }
r[31] = k.pc(0x4a1908) as u64;  { let k2 = k.call(c, 0x4a1900, 0x4a1928); f004a1928(c, k2, r, h); }
l = 0x004a1908; continue; }
0x004a1908 => {
let t = (r[9] as i64) >= 0;  r[10] = 0x0000000070000000;
 if t { l = 0x004a1918; continue; }
r[31] = k.pc(0x4a1918) as u64; r[10] = (((r[10] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a1910, 0x4a1928); f004a1928(c, k2, r, h); }
l = 0x004a1918; continue; }
0x004a1918 => {
dw(c, (r[28] as u32).wrapping_add(k.lo(c, 0x4a1918)), r[13] as u32);
l = 0x004a191c; continue; }
0x004a191c => {
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1928(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a1928;
loop { match l {
0x004a1928 => {
r[11] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1928))) as i32 as i64 as u64;
r[15] = r[13].wrapping_add(0u64);
r[13] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
r[14] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x004a1938; continue; }
0x004a1938 => {
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[10] as u32)); r[2] = a; h[2] = b; }
r[14] = (((r[14] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[13] = (((r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
let t = r[11] != 0u64;  c.wr128((r[13] as u32).wrapping_add(0xfffffff0), r[2], h[2]);
 if t { l = 0x004a1938; continue; }
let t = r[14] == 0u64;   if t { l = 0x004a1998; continue; }
r[2] = 0u64 | 0x1;
dw(c, (r[15] as u32), r[2] as u32);
r[3] = 0x0000000024000000;
dw(c, (r[15] as u32).wrapping_add(0x00000004), r[3] as u32);
c.wr64((r[15] as u32).wrapping_add(0x00000008), r[2]);
dw(c, (r[15] as u32).wrapping_add(0x00000010), r[5] as u32);
r[1] = 0x0000000044000000;
dw(c, (r[15] as u32).wrapping_add(0x00000014), r[1] as u32);
r[1] = 0u64 | 0x55;
c.wr64((r[15] as u32).wrapping_add(0x00000018), r[1]);
r[1] = r[14] | 0x0;
dw(c, (r[15] as u32).wrapping_add(0x00000020), r[14] as u32);
dw(c, (r[15] as u32).wrapping_add(0x00000024), r[3] as u32);
r[1] = 0u64 | 0x52;
c.wr64((r[15] as u32).wrapping_add(0x00000028), r[1]);
l = 0x004a1998; continue; }
0x004a1998 => {
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a19a0(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a19a0;
loop { match l {
0x004a19a0 => {
{ let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
r[1] = sx(k.a(c, 0x4a19a4, 0x4a19a8).wrapping_sub(0x7600));
r[1] = (((r[1] as u32).wrapping_add(0x00007600)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x00000190)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[1] as u32).wrapping_add(0x000001a0)); c.vf[29] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[28] as u32).wrapping_add(k.lo(c, 0x4a19b4))); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = 0x00000000493f0000;
r[1] = r[1] | 0xfff8;
{ let (a, b) = uycpu::mmi::pextlw((r[1], h[1]), (r[1], h[1])); r[1] = a; h[1] = b; }
c.vf[31] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
r[24] = 0u64 | 0x1;
{ let (a, b) = uycpu::mmi::pcpyld((r[24], h[24]), (0u64, 0u64)); r[24] = a; h[24] = b; }
r[24] = 0x0000000024000000;
r[24] = r[24] << 32;
r[24] = r[24] | 0x1;
r[25] = 0u64 | 0x55;
{ let (a, b) = uycpu::mmi::pcpyld((r[25], h[25]), (0u64, 0u64)); r[25] = a; h[25] = b; }
r[25] = 0x0000000044000000;
r[25] = r[25] << 32;
{ let (a, b) = uycpu::mmi::pextlw((0u64, 0u64), (r[5], h[5])); r[5] = a; h[5] = b; }
r[25] = r[25] | r[5];
r[15] = 0u64 | 0x52;
{ let (a, b) = uycpu::mmi::pcpyld((r[15], h[15]), (0u64, 0u64)); r[15] = a; h[15] = b; }
r[15] = 0x0000000024000000;
r[15] = r[15] << 32;
r[15] = r[15].wrapping_add(0x0000000000000004);
r[13] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1a08))) as i32 as i64 as u64;
r[11] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1a0c))) as i32 as i64 as u64;
r[14] = r[4].wrapping_add(0u64);
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[11] == 0u64;   if t { l = 0x004a1b78; continue; }
r[1] = 0u64 | 0x1;
dw(c, (r[13] as u32), r[1] as u32);
r[1] = 0x0000000010000000;
dw(c, (r[13] as u32).wrapping_add(0x00000004), r[1] as u32);
r[1] = ((0u32.wrapping_add(0x0000000e)) as i32 as i64 as u64);
dw(c, (r[13] as u32).wrapping_add(0x00000008), r[1] as u32);
r[1] = ((0u32.wrapping_add(0x0000003f)) as i32 as i64 as u64);
dw(c, (r[13] as u32).wrapping_add(0x00000018), r[1] as u32);
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[10] = 0x0000000070000000;
r[12] = 0x0000000070000000;
r[11] = (((r[12] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
l = 0x004a1a50; continue; }
0x004a1a50 => {
r[1] = c.rd8((r[12] as u32).wrapping_add(0x00003000)) as i8 as i64 as u64;
r[12] = (((r[12] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
let t = r[1] != r[14];   if t { l = 0x004a1b1c; continue; }
{ let (a, b) = c.rd128((r[10] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[4] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000010)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[5] = ((0u32.wrapping_add(0x00000004)) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000030)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[10] as u32).wrapping_add(0x00000020)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::clip(c, 1, 1);
c.wr128((r[13] as u32), r[24], h[24]);
uycpu::vu0::clip(c, 2, 2);
c.wr128((r[13] as u32).wrapping_add(0x00000010), r[25], h[25]);
uycpu::vu0::clip(c, 3, 3);
c.wr128((r[13] as u32).wrapping_add(0x00000020), r[15], h[15]);
uycpu::vu0::clip(c, 4, 4);
{ let v = c.vf[1]; c.wr128((r[13] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[3]; c.wr128((r[13] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[4]; c.wr128((r[13] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[1] = c.cfc2(18) as i32 as i64 as u64;
let t = r[1] == 0u64;  r[2] = ((0u32.wrapping_add(0x00000004)) as i32 as i64 as u64);
 if t { l = 0x004a1ac8; continue; }
r[31] = k.pc(0x4a1abc) as u64;  { let k2 = k.call(c, 0x4a1ab4, 0x4a16f8); f004a16f8(c, k2, r, h); }
r[1] = (((r[2] as u32).wrapping_add(0xfffffffd)) as i32 as i64 as u64);
let t = (r[1] as i64) < 0;  dw(c, (r[13] as u32).wrapping_add(0x00000020), r[2] as u32);
 if t { l = 0x004a1b1c; continue; }
l = 0x004a1ac8; continue; }
0x004a1ac8 => {
r[13] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
l = 0x004a1acc; continue; }
0x004a1acc => {
{ let (a, b) = c.rd128((r[13] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[1][3]);
vop::<6, 14>(c, 1, 28, c.vf[1]);
r[13] = (((r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
vop::<6, 14>(c, ACC, 29, b4(c.vf[0][3]));
vop::<2, 14>(c, ACC, 1, b4(c.vi[22]));
vop::<2, 14>(c, 1, 18, b4(c.vf[0][3]));
vop::<2, 12>(c, 2, 31, b4(c.vf[0][3]));
uycpu::vu0::ftoi::<0, 2>(c, 3, 1);
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let v = c.vf[1]; c.wr128((r[13] as u32).wrapping_add(0xfffffff0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::pexch((r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[9], h[9]), (0u64, 0u64)); r[9] = a; h[9] = b; }
dw(c, (r[13] as u32).wrapping_add(0xfffffff8), r[8] as u32);
let t = r[2] != 0u64;  dw(c, (r[13] as u32).wrapping_add(0xfffffffc), r[9] as u32);
 if t { l = 0x004a1acc; continue; }
l = 0x004a1b1c; continue; }
0x004a1b1c => {
let t = r[12] != r[11];  r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
 if t { l = 0x004a1a50; continue; }
r[15] = r[15].wrapping_add(0xfffffffffffffffc);
r[2] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
r[1] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1b2c))) as i32 as i64 as u64;
let t = r[14] != r[2];   if t { l = 0x004a1b3c; continue; }
r[1] = ((0u32.wrapping_sub((r[1] as u32))) as i32 as i64 as u64);
l = 0x004a1b3c; continue; }
0x004a1b3c => {
let t = (r[1] as i64) < 0;  r[10] = 0x0000000070000000;
 if t { l = 0x004a1b4c; continue; }
r[31] = k.pc(0x4a1b4c) as u64;  { let k2 = k.call(c, 0x4a1b44, 0x4a1b84); f004a1b84(c, k2, r, h); }
l = 0x004a1b4c; continue; }
0x004a1b4c => {
r[2] = ((0u32.wrapping_add(0x00000002)) as i32 as i64 as u64);
r[1] = sx(k.a(c, 0x4a1b50, 0x4a1b54).wrapping_sub(0xe3c));
r[1] = c.rd32((r[1] as u32).wrapping_add(0x00000e3c)) as i32 as i64 as u64;
let t = r[14] != r[2];   if t { l = 0x004a1b64; continue; }
r[1] = ((0u32.wrapping_sub((r[1] as u32))) as i32 as i64 as u64);
l = 0x004a1b64; continue; }
0x004a1b64 => {
let t = (r[1] as i64) >= 0;  r[10] = 0x0000000070000000;
 if t { l = 0x004a1b74; continue; }
r[31] = k.pc(0x4a1b74) as u64; r[10] = (((r[10] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 { let k2 = k.call(c, 0x4a1b6c, 0x4a1b84); f004a1b84(c, k2, r, h); }
l = 0x004a1b74; continue; }
0x004a1b74 => {
dw(c, (r[28] as u32).wrapping_add(k.lo(c, 0x4a1b74)), r[13] as u32);
l = 0x004a1b78; continue; }
0x004a1b78 => {
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f004a1b84(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x4a1b84;
loop { match l {
0x004a1b84 => {
{ let (a, b) = uycpu::mmi::pcpyld((r[30], h[30]), (0u64, 0u64)); r[30] = a; h[30] = b; }
r[30] = (((r[31] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
r[11] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1b8c))) as i32 as i64 as u64;
r[3] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
l = 0x004a1b94; continue; }
0x004a1b94 => {
{ let (a, b) = c.rd128((r[10] as u32)); r[1] = a; h[1] = b; }
r[11] = (((r[11] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
c.wr128((r[3] as u32), r[1], h[1]);
r[10] = (((r[10] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
let t = r[11] != 0u64;  r[3] = (((r[3] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 if t { l = 0x004a1b94; continue; }
r[4] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
r[5] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x4a1bb0))) as i32 as i64 as u64;
r[31] = k.pc(0x4a1bbc) as u64;  { let k2 = k.call(c, 0x4a1bb4, 0x4a16f8); f004a16f8(c, k2, r, h); }
r[1] = (((r[2] as u32).wrapping_add(0xfffffffd)) as i32 as i64 as u64);
let t = (r[1] as i64) < 0;   if t { l = 0x004a1c30; continue; }
c.wr128((r[13] as u32), r[24], h[24]);
c.wr128((r[13] as u32).wrapping_add(0x00000010), r[25], h[25]);
{ let (a, b) = uycpu::mmi::pcpyud((0u64, 0u64), (r[15], h[15])); r[1] = a; h[1] = b; }
r[1] = r[15] | r[2];
c.wr128((r[13] as u32).wrapping_add(0x00000020), r[1], h[1]);
r[13] = (((r[13] as u32).wrapping_add(0x00000030)) as i32 as i64 as u64);
l = 0x004a1be0; continue; }
0x004a1be0 => {
{ let (a, b) = c.rd128((r[13] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[1][3]);
vop::<6, 14>(c, 1, 28, c.vf[1]);
r[13] = (((r[13] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
vop::<6, 14>(c, ACC, 29, b4(c.vf[0][3]));
vop::<2, 14>(c, ACC, 1, b4(c.vi[22]));
vop::<2, 14>(c, 1, 18, b4(c.vf[0][3]));
vop::<2, 12>(c, 2, 31, b4(c.vf[0][3]));
uycpu::vu0::ftoi::<0, 2>(c, 3, 1);
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let v = c.vf[1]; c.wr128((r[13] as u32).wrapping_add(0xfffffff0), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::pexch((r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[9], h[9]), (0u64, 0u64)); r[9] = a; h[9] = b; }
dw(c, (r[13] as u32).wrapping_add(0xfffffff8), r[8] as u32);
let t = r[2] != 0u64;  dw(c, (r[13] as u32).wrapping_add(0xfffffffc), r[9] as u32);
 if t { l = 0x004a1be0; continue; }
l = 0x004a1c30; continue; }
0x004a1c30 => {
r[31] = (((r[30] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pcpyud((r[30], h[30]), (0u64, 0u64)); r[30] = a; h[30] = b; }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f0040be48(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x40be48;
loop { match l {
0x0040be48 => {
{ let (a, b) = c.rd128((r[5] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::abs::<15>(c, 1, 1);
{ let v = c.vf[1]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f0040be90(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x40be90;
loop { match l {
0x0040be90 => {
r[1] = c.f[12] as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[5] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.vf[2] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
vop::<6, 14>(c, 1, 1, b4(c.vf[2][0]));
{ let v = c.vf[1]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f0040bf20(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x40bf20;
loop { match l {
0x0040bf20 => {
{ let (a, b) = c.rd128((r[5] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[6] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::opmula(c, 2, 1);
uycpu::vu0::opmsub(c, 3, 1, 2);
{ let v = c.vf[3]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 return;
}
_ => unreachable!(), } } }


#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
fn f0040c0f0(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x40c0f0;
loop { match l {
0x0040c0f0 => {
{ let (a, b) = c.rd128((r[5] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
vop::<0, 14>(c, 3, 0, b4(c.vf[0][3]));
vop::<6, 14>(c, 2, 1, c.vf[1]);
vop::<0, 8>(c, ACC, 2, b4(c.vf[2][1]));
vop::<2, 8>(c, 2, 3, b4(c.vf[2][2]));
r[1] = c.f[12] as i32 as i64 as u64;
c.vf[3] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
uycpu::vu0::qop::<2>(c, c.vf[3][0], c.vf[2][0]);
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = r[1] == 0u64;   if t { l = 0x0040c130; continue; }
vop::<6, 14>(c, 1, 1, b4(c.vi[22]));
{ let v = c.vf[1]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 return;
}
0x0040c130 => {
vop::<0, 14>(c, 1, 0, c.vf[0]);
{ let v = c.vf[1]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 return;
}
_ => unreachable!(), } } }


