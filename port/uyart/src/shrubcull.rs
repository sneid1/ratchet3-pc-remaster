//! The shrubs' cull and their lighting (level 1 0x4A2198 with everything under it, and 0x4A3D80), done natively in front of
//! their translations (port/uyatr `JOBS`). The translated wrapper at 0x4A2198 asks `start` (`Rt::native` 16), the one at
//! 0x4A3D80 asks with 18; both answer `after` (17, 19) once the translation ran when `start` let it run. `c.ret` brings the
//! function's address with 1 in its low bit and 2 when the program's view is widened (a story level: `VIEW_LOADS` "the shrub
//! cull" +0x178; the front ends' is the game's own).
//!
//! - 0x4A2198 (0x4010D0 calls it at 0x401214 when 0x1DF9C4 & 8): the chain pointer kept (0x1E10E8 into 0x1E0E60 and, plus 16,
//!   gp+0x4838), FlushCache (0x11F0A0), the cull and chain builder 0x4A2200, the list of shrubs whose light changed copied
//!   from scratchpad 0x3200 to 0x23A070 (0x40BDE0, SPR_FROM), and the chain's end (0x4A1F88: NEXT tags around the light
//!   patches' packets of 0x4A3AD8, the global packet's CALL 0x4C21C0).
//! - 0x4A2200 is hand-written (its registers kept at scratchpad 0x3FA8, ra the scratchpad's base, the high halves of t9 and
//!   s3 carrying the chain's start): for each shrub class of the list at 0x2384F0 its instances come in by SPR_TO (128 at a
//!   time, to scratchpad 0x1000), each one tested against the view's frustum and its draw distance (the far and near fade,
//!   0x4A2538 to 0x4A27F0); a drawn one's matrix goes into the VIF1 packets built in the scratchpad at sp (0x0000 to 0x1000,
//!   two halves of 0x800 sent to the chain in RAM at fp by SPR_FROM as each fills), the near ones lit per vertex group
//!   (0x4A2930); then the classes' packet lists, the light patches (0x4A3098) and the second pass of the near ones
//!   (0x4A35F8), the class table at scratchpad 0x3000 and the NEXT tags that join the packets.
//! - 0x4A3D80 (0x4010D0 calls it at 0x40179C with the list 0x23A070; the level's setup 0x4252F0 at 0x4262D4 with lists of up
//!   to 4000 at 0x70000000): each listed shrub's colours from the level's lights, four vertices at a time by the shrub VU0
//!   program (tag 0x100AE0, `p100af0` in uyatr's vu0mp.rs): its runs from micro 0x0 and 0x160 are done here
//!   (`p100af0_0`, `p100af0_160`), the same arithmetic in the same order with its MAC and status flags as the run leaves
//!   them, so no VCALLMS is made. The program's other entries (0x2C0, 0x420, 0x580) are the tie light patches' (0x4B3368),
//!   which stay with the translated program.
//!
//! The functions are the same code in every program (uyatr finds them by size and shape: one of each in each story level and
//! front end); what relocation changes in them, the addresses of the globals and of boot data, the gp offsets and the call
//! targets, is read from the code in RAM (`F`), named by level 1's address of the instruction holding it. Each is its
//! translation's statements in order on a register file of its own, so what the unit leaves is what the translation leaves:
//! every GPR (both halves), VU0's registers and flags, RAM, the scratchpad, the DMA channels' registers, D_STAT and the end
//! interrupts as the port's kicks leave them. What goes away is the hardware's traffic: the stores to SPR_FROM's and SPR_TO's
//! registers become the transfers they kick (`dw`), the polls read the channel as the port keeps it, the VU0 program's
//! starts are its arithmetic, the loops' safe points (the unit is one block of the thread's time, as the moby unit is).
//!
//! With UYA_NATIVE_CHECK=<job>:N (job `shrub`: the calls of 0x4A2198; `shrub2`: the calls of 0x4A3D80; `moby`, `moby2` and
//! `sweep` are other units' and leave these alone) the first N calls run twice from the same state: the translation, then
//! the native unit on the state as it was before; every difference in the machine's state they leave (`differences`) is
//! reported with what the calls covered, and the game goes on from the translation's state. Both runs go with the thread's
//! interrupts held (`Check::ints`), as the moby unit's check does.

use crate::{Cpu, Ext};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use uycpu::mmi;
use uycpu::vu0::{m_fmac, m_itof, m_mac, m_put};

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
        assert!(a < uycpu::RAM_SIZE - 4, "the shrub unit's code at {a:08x}: not in RAM");
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
    /// This program's address of level 1's `pc` (a return address, a resume point).
    #[inline(always)]
    fn pc(self, pc: u32) -> u32 {
        self.at.wrapping_add(pc.wrapping_sub(self.l1))
    }
}

// ---- registers, 32-bit arithmetic as the R5900 keeps it, VU0's macro instructions -----------------------------------------

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
const GP: usize = 28;
const SP: usize = 29;
const FP: usize = 30;
const RA: usize = 31;

/// A 32-bit result as the R5900 keeps it in 64 bits.
#[inline(always)]
fn sx(v: u32) -> u64 {
    v as i32 as i64 as u64
}
/// ADD / ADDU / ADDI / ADDIU.
#[inline(always)]
fn add32(a: u64, b: u64) -> u64 {
    sx((a as u32).wrapping_add(b as u32))
}
/// SUB / SUBU.
#[inline(always)]
fn sub32(a: u64, b: u64) -> u64 {
    sx((a as u32).wrapping_sub(b as u32))
}
/// SLL.
#[inline(always)]
fn sll(a: u64, s: u32) -> u64 {
    sx((a as u32) << s)
}
/// SRL.
#[inline(always)]
fn srl(a: u64, s: u32) -> u64 {
    sx((a as u32) >> s)
}

#[inline(always)]
fn w4(q: Q) -> [u32; 4] {
    [q.0 as u32, (q.0 >> 32) as u32, q.1 as u32, (q.1 >> 32) as u32]
}
#[inline(always)]
fn q4(v: [u32; 4]) -> Q {
    (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32)
}
/// A lane in every lane (the BC forms).
#[inline(always)]
fn b4(x: u32) -> [u32; 4] {
    [x; 4]
}
/// LQC2 (VF0 keeps its constant).
#[inline(always)]
fn lqc2(c: &mut Cpu, v: usize, a: u32) {
    let q = c.rd128(a);
    if v != 0 {
        c.vf[v] = w4(q);
    }
}
/// SQC2.
#[inline(always)]
fn sqc2(c: &mut Cpu, v: usize, a: u32) {
    let (lo, hi) = q4(c.vf[v]);
    c.wr128(a, lo, hi);
}
/// QMTC2.
#[inline(always)]
fn qmtc2(c: &mut Cpu, v: usize, x: Q) {
    if v != 0 {
        c.vf[v] = w4(x);
    }
}

const ACC: usize = 32;

/// An FMAC macro instruction (K: 0 ADD, 1 SUB, 2 MADD, 3 MSUB, 4 MAX, 5 MINI, 6 MUL) on lanes D into VF `fd` or ACC (`ACC`;
/// `fd` 0 writes nothing), with the MAC and status flags it makes: `uycpu::vu0::fmac`'s result on four lanes at once
/// (`m_fmac` under the game's MXCSR is the VU's arithmetic, `fin` on each operand by DAZ and the exponent-255 clamp).
#[inline(always)]
fn fm<const K: u8, const D: u32>(c: &mut Cpu, fd: usize, fs: usize, t: [u32; 4]) {
    let r = m_fmac::<K, D>(c.vf[fs], t, c.vacc);
    if K != 4 && K != 5 {
        flags(c, m_mac::<D>(r));
    }
    if fd == ACC {
        m_put::<D>(&mut c.vacc, r);
    } else if fd != 0 {
        m_put::<D>(&mut c.vf[fd], r);
    }
}

/// A macro instruction's MAC flags (VI17) and the status flag's zero and sign bits with their sticky copies (VI16), as
/// `uycpu::vu0` sets them.
#[inline(always)]
fn flags(c: &mut Cpu, mac: u32) {
    c.vi[17] = mac;
    let st = ((mac & 0xf != 0) as u32) | (((mac & 0xf0 != 0) as u32) << 1);
    c.vi[16] = (c.vi[16] & !0x3) | st | (st << 6);
}

// ---- the scratchpad's DMA channels, as the port's kicks do them -----------------------------------------------------------

const SPR: u32 = 0x7000_0000;

/// A 32-bit store, the scratchpad channels' registers by their meaning (0x40BDE0's and 0x4A2200's stores to 0x1000D000 and
/// 0x1000D400 and their offsets).
#[inline(always)]
fn dw(c: &mut Cpu, a: u32, v: u32) {
    if a & 0xffff_fb00 == 0x1000_d000 { dma_reg(c, a, v) } else { c.wr32(a, v) }
}

/// A store to SPR_FROM's (0x1000D000) or SPR_TO's (0x1000D400) registers: kept, as `hw::write` keeps it; CHCR with STR
/// makes the transfer, as the port's kick would.
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
        (8, 0) => spr_from(c, v, h[1], h[2], h[8]),
        (_, mode) => panic!("the shrub unit's DMA on channel {ch} in mode {mode} (CHCR {v:#x}): the unit makes normal transfers only"),
    }
}

/// SPR_TO (channel 9) in normal mode with these register values: the copy into the scratchpad, the skin recording of a
/// transfer to 0x1C00 (`hw::SkinJoints`), the registers as the kick leaves them, the channel's end and its interrupt.
fn spr_to(c: &mut Cpu, chcr: u32, madr: u32, qwc: u32, sadr: u32) {
    let mp = c.mp();
    let m = c.mach();
    let h = &mut m.hw.ch[9];
    (h[0], h[1], h[2], h[8]) = (chcr, madr, qwc, sadr);
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

/// 0x4A2200's poll of SPR_FROM before a transfer (`lw v1, 0(a0)` of D8_CHCR, `andi v1, v1, 0x100`, a spin while set): the
/// port's transfers end at their kick, so STR is never set when the game reads it.
#[inline(always)]
fn poll(c: &mut Cpu, r: &mut [u64; 32]) {
    r[V1] = sx(c.mach().hw.ch[8][0]) & 0x100;
    if r[V1] != 0 {
        panic!("the shrub cull's poll of SPR_FROM found it busy (CHCR {:#x}): the port's transfers end at their kick", c.mach().hw.ch[8][0]);
    }
}

/// 0x4A2200's SPR_FROM of the packets built in the current half (a0 0x1000D000, at = sp & 0x7FF, v0 = sp - at): SADR, MADR
/// fp - at, QWC at / 16, CHCR 0x100.
#[inline(always)]
fn kick_from(c: &mut Cpu, r: &mut [u64; 32]) {
    dw(c, (r[A0] as u32).wrapping_add(0x80), r[V0] as u32);
    r[V0] = sub32(r[FP], r[AT]);
    dw(c, (r[A0] as u32).wrapping_add(0x10), r[V0] as u32);
    r[V0] = srl(r[AT], 4);
    dw(c, (r[A0] as u32).wrapping_add(0x20), r[V0] as u32);
    r[V0] = 0x100;
    dw(c, r[A0] as u32, r[V0] as u32);
}

/// The head of 0x4A2200's flushes: at, a0, v0 and the poll.
#[inline(always)]
fn flush_head(c: &mut Cpu, r: &mut [u64; 32]) {
    r[AT] = r[SP] & 0x7ff;
    r[A0] = 0x1000_d000;
    r[V0] = sub32(r[SP], r[AT]);
    poll(c, r);
}

/// The packets of the half sp is in sent to the chain at fp (seven copies of one code in 0x4A2200: 0x4A288C, 0x4A2C5C,
/// 0x4A2EC4, 0x4A2F6C, 0x4A33A4, 0x4A375C, 0x4A38C4), and sp to the other half: sp -= at, sp ^= 0x800.
fn flush(c: &mut Cpu, r: &mut [u64; 32]) {
    flush_head(c, r);
    kick_from(c, r);
    r[SP] = sub32(r[SP], r[AT]);
    r[SP] ^= 0x800;
}

/// 0x4A3460's flush: none when sp is at its half's start.
fn flush_some(c: &mut Cpu, r: &mut [u64; 32]) {
    flush_head(c, r);
    if r[AT] != 0 {
        kick_from(c, r);
        r[SP] = sub32(r[SP], r[AT]);
        r[SP] ^= 0x800;
    }
}

/// 0x4A3958's last flush (none when sp is at its half's start), sp left where it is, then the wait for its end (0x4A39B0:
/// STR read clear, v1 = 16 in the branch's delay slot).
fn last_flush(c: &mut Cpu, r: &mut [u64; 32]) {
    flush_head(c, r);
    if r[AT] != 0 {
        kick_from(c, r);
        poll(c, r);
        r[V1] = 0x10;
    }
}

// ---- what the calls covered (the check's report) ---------------------------------------------------------------------------

/// Counts of the paths the native calls took: shrub classes culled, instances tested, instances drawn, near instances lit
/// per vertex group, shrubs lit by 0x4A3D80, runs of the shrub program done natively.
const N: usize = 6;
static DONE: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
const NAMES: [&str; N] = ["shrub classes", "instances tested", "instances drawn", "near instances shaded", "shrubs lit", "program runs"];

// ---- the check -------------------------------------------------------------------------------------------------------------

/// The machine's state a call of the unit can change.
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
    ret: u32,
    ch: [[u32; 9]; 10],
    d_stat: u32,
    intc_stat: u32,
    parked: Option<usize>,
    records: Vec<(u32, u32, usize)>,
    rec_bytes: Vec<u8>,
    captured: bool,
    outputs: Vec<(usize, u32, Vec<u8>, Vec<[f32; 3]>)>,
    normals: Vec<[f32; 3]>,
}

impl State {
    fn take(c: &mut Cpu) -> State {
        let (r, rh, hilo, sa, f, facc, fcr31) = (c.r, c.rh, [c.lo, c.hi, c.lo1, c.hi1], c.sa, c.f, c.facc, c.fcr31);
        let (vf, vacc, vi, view_h, ret) = (c.vf, c.vacc, c.vi, c.view_h, c.ret);
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
            ret,
            ch: m.hw.ch,
            d_stat: m.hw.d_stat,
            intc_stat: m.hw.intc_stat,
            parked: m.vu0_parked,
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
        (c.vf, c.vacc, c.vi, c.view_h, c.ret) = (self.vf, self.vacc, self.vi, self.view_h, self.ret);
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
        m.vu0_parked = self.parked;
        m.hw.skin.records = self.records.clone();
        m.hw.skin.bytes = self.rec_bytes.clone();
        m.hw.skin.captured = self.captured;
        m.hw.skin.outputs = self
            .outputs
            .iter()
            .map(|(record, dest, bytes, normals)| crate::hw::SkinOutput {
                record: *record,
                dest: *dest,
                bytes: bytes.clone(),
                normals: normals.clone(),
            })
            .collect();
        m.hw.skin_normals = self.normals.clone();
    }
}

/// The byte ranges where `a` and `b` differ, as (start, end) pairs, at most `most`, and how many there are.
fn ranges(a: &[u8], b: &[u8], base: u32, most: usize) -> (Vec<String>, usize) {
    let (mut out, mut n, mut i) = (Vec::new(), 0, 0);
    while i < a.len() {
        if a[i] == b[i] {
            i += 1;
            continue;
        }
        let s = i;
        while i < a.len() && (a[i] != b[i] || (i + 16 < a.len() && a[i..i + 16] != b[i..i + 16])) {
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

/// What differs between the translation's state `want` and the native unit's `got`. Nothing is left out: the unit is its
/// translation's statements on the same state (the module's notes), so every register, both halves, and every byte of the
/// memories, the frames below sp among them (the glue's saves at sp - 16 and 0x4A1F88's at sp - 48), is what the
/// translation leaves.
fn differences(want: &State, got: &State) -> Vec<String> {
    let mut d = Vec::new();
    for (what, a, b, base) in [
        ("RAM", &want.ram, &got.ram, 0u32),
        ("scratchpad", &want.spr, &got.spr, SPR),
        ("VU0 data", &want.vu0d, &got.vu0d, 0),
        ("VU0 code", &want.vu0m, &got.vu0m, 0),
    ] {
        let (r, n) = ranges(a, b, base, 12);
        if n > 0 {
            d.push(format!("{what}: {n} ranges {}", r.join(" ")));
        }
    }
    let regs: Vec<String> = (1..32)
        .filter(|&k| want.r[k] != got.r[k] || want.rh[k] != got.rh[k])
        .map(|k| format!("r{k} {:x}:{:x}/{:x}:{:x}", want.rh[k], want.r[k], got.rh[k], got.r[k]))
        .collect();
    if !regs.is_empty() || want.sa != got.sa || want.hilo != got.hilo || want.ret != got.ret {
        d.push(format!(
            "registers: {} SA {}/{} HI/LO {:x?}/{:x?} ret {:08x}/{:08x}",
            regs.join(" "),
            want.sa,
            got.sa,
            want.hilo,
            got.hilo,
            want.ret,
            got.ret
        ));
    }
    let fregs: Vec<String> = (0..32).filter(|&k| want.f[k] != got.f[k]).map(|k| format!("f{k} {:x}/{:x}", want.f[k], got.f[k])).collect();
    if !fregs.is_empty() || want.facc != got.facc || want.fcr31 != got.fcr31 {
        d.push(format!("FPU: {} ACC {:x}/{:x} FCR31 {:x}/{:x}", fregs.join(" "), want.facc, got.facc, want.fcr31, got.fcr31));
    }
    let vf: Vec<String> = (0..32).filter(|&k| want.vf[k] != got.vf[k]).map(|k| format!("vf{k} {:08x?}/{:08x?}", want.vf[k], got.vf[k])).collect();
    let vi: Vec<String> = (0..32).filter(|&k| want.vi[k] != got.vi[k]).map(|k| format!("vi{k} {:x}/{:x}", want.vi[k], got.vi[k])).collect();
    if !vf.is_empty() || !vi.is_empty() || want.vacc != got.vacc || want.parked != got.parked {
        d.push(format!("VU0: {} {} ACC {:08x?}/{:08x?} waiting {:?}/{:?}", vf.join(" "), vi.join(" "), want.vacc, got.vacc, want.parked, got.parked));
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
        d.push(format!(
            "skin outputs: {} / {} (normals pending {} / {})",
            want.outputs.len(),
            got.outputs.len(),
            want.normals.len(),
            got.normals.len()
        ));
    }
    d
}

/// The two entries: 0x4A2198 (`shrub`) and 0x4A3D80 (`shrub2`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Job {
    Cull,
    Light,
}

struct Check {
    job: Job,
    left: u64,
    done: u64,
    differ: u64,
    kinds: std::collections::BTreeSet<String>,
    before: Option<State>,
    /// Entries of this unit reached inside the translation being checked: they run their translations too.
    inner: u32,
    /// Whether the thread had its interrupts on at the checked call: both runs go with them held (DI), and EI after the
    /// comparison takes what waited (`mobydraw`'s `Check::ints`: FlushCache's syscall at 0x4A21C8 runs the timers'
    /// interrupts that came due by the host's clock, and a loop's back edge in the translation past the field's time gives
    /// the field up; held, each lands once, after both runs).
    ints: bool,
}

static CHECK: std::sync::Mutex<Option<Check>> = std::sync::Mutex::new(None);

/// UYA_NATIVE_CHECK=<job>:N for this unit, read once; another unit's job (`moby`, `moby2`, `sweep`, any other) leaves this one unchecked.
fn checked() -> Option<(Job, u64)> {
    static C: std::sync::OnceLock<Option<(Job, u64)>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_NATIVE_CHECK").ok()?;
        let (job, n) = v.split_once(':').unwrap_or_else(|| panic!("UYA_NATIVE_CHECK={v}: <job>:N"));
        let job = match job {
            "shrub" => Job::Cull,
            "shrub2" => Job::Light,
            _ => return None,
        };
        Some((job, n.parse().unwrap_or_else(|_| panic!("UYA_NATIVE_CHECK={v}: N is a number of calls"))))
    })
}

thread_local! {
    /// The function's address and view bit of the call being checked, for its native run after the translation's.
    static CALL: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((0, false)) };
}

/// `Rt::native` 16 and 18: the unit natively (`c.ret` the return address), or (checking) the state saved and `c.ret` 0 so
/// the translation runs.
pub(crate) fn start(c: &mut Cpu, k: u32) {
    let job = if k == 16 { Job::Cull } else { Job::Light };
    let (at, wide) = (c.ret & !3, c.ret & 2 != 0);
    if let Some((cj, n)) = checked() {
        let mut g = CHECK.lock().expect("the shrub check");
        let ck = g.get_or_insert(Check { job: cj, left: n, done: 0, differ: 0, kinds: Default::default(), before: None, inner: 0, ints: false });
        if ck.before.is_some() {
            ck.inner += 1;
            c.ret = 0;
            return;
        }
        if ck.job == job && ck.left > 0 {
            CALL.with(|w| w.set((at, wide)));
            // Interrupts on as the kernel sees them (`kernel::K::ints_on`): EIE and IE, not in an exception.
            ck.ints = c.status & 0x1_0001 == 0x1_0001 && c.status & 6 == 0;
            if ck.ints {
                c.di();
            }
            ck.before = Some(State::take(c));
            c.ret = 0;
            return;
        }
    }
    run(c, job, at, wide);
    c.ret = c.r[RA] as u32;
}

/// `Rt::native` 17 and 19: the translation ran; the unit runs natively from the state before it and the two are compared.
pub(crate) fn after(c: &mut Cpu, k: u32) {
    let job = if k == 17 { Job::Cull } else { Job::Light };
    let (Some(before), ints) = ({
        let mut g = CHECK.lock().expect("the shrub check");
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
    let want = State::take(c);
    before.put(c);
    let (at, wide) = CALL.with(|w| w.get());
    let sp = c.r[SP] as u32;
    run(c, job, at, wide);
    c.ret = c.r[RA] as u32;
    let got = State::take(c);
    let d = differences(&want, &got);
    want.put(c);
    c.ret = ret;
    let mut g = CHECK.lock().expect("the shrub check");
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
        println!("UYA_NATIVE_CHECK: {job:?}: {} calls checked, {} differ ({})", ck.done, ck.differ, n.join(", "));
    }
    drop(g);
    // The interrupts held for the two runs (`Check::ints`): what came due runs now, as at the game's own EI.
    if ints {
        c.ei();
        c.ret = ret;
    }
}

/// The unit from either entry, natively.
fn run(c: &mut Cpu, job: Job, at: u32, wide: bool) {
    let mut n = [0u64; N];
    match job {
        Job::Cull => {
            // The tie ranges' room (`tiebudget`): the chain pointer as the shrub pass begins is the tie pass's end.
            if let Some(ptr) = c.mach().hw.tie_costs.ptr() {
                let at_shrubs = c.rd32(ptr);
                c.mach().hw.tie_costs.shrub_pass(at_shrubs);
            }
            f4a2198(c, F { at, l1: 0x4a_2198 }, wide, &mut n)
        }
        Job::Light => f4a3d80(c, F { at, l1: 0x4a_3d80 }, &mut n),
    }
    for (a, v) in DONE.iter().zip(n) {
        if v != 0 {
            a.fetch_add(v, Relaxed);
        }
    }
}

// ---- the glue: 0x4A2198, 0x40BDE0, 0x4A1F88, 0x4C21C0 -------------------------------------------------------------------------
//
// Compiled C, on the thread's registers as their translations leave them (a lui whose register a load or store then replaces
// is left out where nothing reads its value).

/// 0x4A2198: the chain pointer kept for the chain's end (0x1E10E8 into 0x1E0E60 with 0x1D4BB4 into 0x1D4BB0, gp+0x4838 the
/// pointer plus 16), FlushCache (0x11F0A0: syscall 100), the cull and chain builder 0x4A2200, the list of shrubs to light
/// from scratchpad 0x3200 to 0x23A070 (0x40BDE0, 64 quadwords), the chain's end (0x4A1F88).
fn f4a2198(c: &mut Cpu, k: F, wide: bool, n: &mut [u64; N]) {
    let gp = c.r[GP] as u32;
    c.r[V0] = sx(c.rd32(k.a(c, 0x4a_2198, 0x4a_219c)));
    c.r[SP] = add32(c.r[SP], k.lo(c, 0x4a_21a0) as u64);
    c.r[V1] = sx(c.rd32(k.a(c, 0x4a_21a4, 0x4a_21a8)));
    c.r[A0] = 0;
    c.r[AT] = sx(k.hi(c, 0x4a_21b0));
    c.wr32(k.a(c, 0x4a_21b0, 0x4a_21b4), c.r[V0] as u32);
    c.r[V0] = add32(c.r[V0], 16);
    c.wr64(c.r[SP] as u32, c.r[RA]);
    c.r[AT] = sx(k.hi(c, 0x4a_21c0));
    c.wr32(k.a(c, 0x4a_21c0, 0x4a_21c4), c.r[V1] as u32);
    // jal 0x11F0A0, its delay slot first: FlushCache(0), `addiu v1, zero, 100; syscall`.
    c.r[RA] = k.pc(0x4a_21d0) as u64;
    c.wr32(k.gp(c, gp, 0x4a_21cc), c.r[V0] as u32);
    let flush = k.call(c, 0x4a_21c8, 0x11_f0a0);
    c.r[V1] = 100;
    c.syscall(flush.pc(0x11_f0a4));
    c.r[RA] = k.pc(0x4a_21d8) as u64;
    {
        let kk = k.call(c, 0x4a_21d0, 0x4a_2200);
        f4a2200(c, kk, wide, n);
    }
    c.r[A0] = sx(k.hi(c, 0x4a_21d8));
    c.r[A1] = sx(k.lo(c, 0x4a_21dc));
    c.r[A2] = sx(k.lo(c, 0x4a_21e0));
    c.r[RA] = k.pc(0x4a_21ec) as u64;
    c.r[A0] = add32(c.r[A0], k.lo(c, 0x4a_21e8) as u64);
    {
        let kk = k.call(c, 0x4a_21e4, 0x40_bde0);
        f40bde0(c, kk);
    }
    c.r[RA] = k.pc(0x4a_21f4) as u64;
    {
        let kk = k.call(c, 0x4a_21ec, 0x4a_1f88);
        f4a1f88(c, kk);
    }
    c.r[RA] = c.rd64(c.r[SP] as u32);
    c.r[SP] = add32(c.r[SP], 16);
}

/// 0x40BDE0: SPR_FROM of a2 quadwords from scratchpad a1 to a0 (SADR, MADR, QWC, CHCR 0x100), then an uncached read of
/// 0x20100000 (the write buffer drained), or'ed with 1 into v1.
fn f40bde0(c: &mut Cpu, k: F) {
    c.r[AT] = 0x1000_d000;
    dw(c, 0x1000_d080, c.r[A1] as u32);
    dw(c, 0x1000_d010, c.r[A0] as u32);
    c.r[V0] = 0x100;
    dw(c, 0x1000_d020, c.r[A2] as u32);
    dw(c, 0x1000_d000, c.r[V0] as u32);
    c.r[V1] = sx(k.hi(c, 0x40_be00));
    c.r[V1] = sx(c.rd32(c.r[V1] as u32));
    c.r[V1] |= 1;
}

/// 0x4A1F88: NEXT tags (0x20000000) from the chain's mark (0x1E0E60) to the light patches' packets that 0x4A3AD8 makes
/// (a0 from gp-0x7D00), the global packet's CALL (0x4C21C0), the largest count it returned kept at gp+0x45B4 when 0x1E0E64
/// is below it, then NEXT tags joining the chain on from the pointer 0x4A2198 kept.
fn f4a1f88(c: &mut Cpu, k: F) {
    let gp = c.r[GP] as u32;
    c.r[SP] = add32(c.r[SP], 0xffff_ffe0);
    let sp = c.r[SP] as u32;
    c.r[V1] = sx(c.rd32(k.a(c, 0x4a_1f8c, 0x4a_1f90)));
    c.wr64(sp.wrapping_add(8), c.r[S1]);
    c.r[S1] = sx(c.rd32(k.a(c, 0x4a_1f98, 0x4a_1f9c)));
    c.wr64(sp.wrapping_add(0x10), c.r[S2]);
    c.r[V0] = add32(c.r[S1], 16);
    c.wr64(sp, c.r[S0]);
    c.wr64(sp.wrapping_add(0x18), c.r[RA]);
    c.r[S2] = sx(k.hi(c, 0x4a_1fb0));
    c.r[AT] = sx(k.hi(c, 0x4a_1fb4));
    c.wr32(k.a(c, 0x4a_1fb4, 0x4a_1fb8), c.r[V0] as u32);
    c.wr32(c.r[V1] as u32, c.r[S2] as u32);
    c.r[A0] = sx(c.rd32(k.a(c, 0x4a_1fc0, 0x4a_1fc4)));
    c.r[V0] = sx(c.rd32(k.a(c, 0x4a_1fc8, 0x4a_1fcc)));
    c.wr32((c.r[A0] as u32).wrapping_add(4), c.r[V0] as u32);
    c.r[V1] = sx(c.rd32(k.a(c, 0x4a_1fd4, 0x4a_1fd8)));
    c.wr32((c.r[V1] as u32).wrapping_add(8), 0);
    c.r[V0] = sx(c.rd32(k.a(c, 0x4a_1fe0, 0x4a_1fe4)));
    c.wr32((c.r[V0] as u32).wrapping_add(0xc), 0);
    c.r[RA] = k.pc(0x4a_1ff4) as u64;
    c.r[A0] = sx(c.rd32(k.gp(c, gp, 0x4a_1ff0)));
    {
        let kk = k.call(c, 0x4a_1fec, 0x4a_3ad8);
        f4a3ad8(c, kk);
    }
    c.r[RA] = k.pc(0x4a_1ffc) as u64;
    c.r[S0] = c.r[V0];
    {
        let kk = k.call(c, 0x4a_1ff4, 0x4c_21c0);
        f4c21c0(c, kk);
    }
    c.r[V0] = sx(c.rd32(k.a(c, 0x4a_1ffc, 0x4a_2000)));
    c.r[V0] = ((c.r[V0] as i64) < (c.r[S0] as i64)) as u64;
    if c.r[V0] != 0 {
        c.wr32(k.gp(c, gp, 0x4a_200c), c.r[S0] as u32);
    }
    c.r[V1] = sx(c.rd32(k.a(c, 0x4a_2010, 0x4a_2014)));
    c.r[S0] = c.rd64(sp);
    c.wr32(c.r[V1] as u32, c.r[S2] as u32);
    c.r[RA] = c.rd64(sp.wrapping_add(0x18));
    c.r[V0] = sx(c.rd32(k.a(c, 0x4a_2024, 0x4a_2028)));
    c.r[A0] = sx(c.rd32(k.a(c, 0x4a_202c, 0x4a_2030)));
    c.r[V0] = add32(c.r[V0], 16);
    c.wr32((c.r[A0] as u32).wrapping_add(4), c.r[V0] as u32);
    c.r[V1] = sx(c.rd32(k.a(c, 0x4a_203c, 0x4a_2040)));
    c.wr32((c.r[V1] as u32).wrapping_add(8), 0);
    c.r[A0] = sx(c.rd32(k.a(c, 0x4a_2048, 0x4a_204c)));
    c.wr32((c.r[A0] as u32).wrapping_add(0xc), 0);
    c.r[V0] = sx(c.rd32(k.a(c, 0x4a_2054, 0x4a_2058)));
    c.r[V0] = add32(c.r[V0], 16);
    c.r[AT] = sx(k.hi(c, 0x4a_2060));
    c.wr32(k.a(c, 0x4a_2060, 0x4a_2064), c.r[V0] as u32);
    c.wr32(c.r[S1] as u32, c.r[S2] as u32);
    c.r[S2] = c.rd64(sp.wrapping_add(0x10));
    c.r[V0] = sx(c.rd32(k.a(c, 0x4a_2070, 0x4a_2074)));
    c.wr32((c.r[S1] as u32).wrapping_add(0xc), 0);
    c.wr32((c.r[S1] as u32).wrapping_add(4), c.r[V0] as u32);
    c.wr32((c.r[S1] as u32).wrapping_add(8), 0);
    c.r[S1] = c.rd64(sp.wrapping_add(8));
    c.r[SP] = add32(c.r[SP], 0x20);
}

/// 0x4C21C0: a CALL tag (0x30000003) to the global packet at 0x1DA2F0 with its DIRECT code (0x50000003) at the chain pointer
/// (0x1E10E8), which steps on (gp+0x4838).
fn f4c21c0(c: &mut Cpu, k: F) {
    let gp = c.r[GP] as u32;
    let p = k.a(c, 0x4c_21c0, 0x4c_21c4);
    c.r[A0] = sx(c.rd32(p));
    c.r[V0] = sx(k.hi(c, 0x4c_21c8) | (k.w(c, 0x4c_21cc) & 0xffff));
    c.r[V1] = sx(k.hi(c, 0x4c_21d0));
    c.wr32(c.r[A0] as u32, c.r[V0] as u32);
    c.r[V1] = add32(c.r[V1], k.lo(c, 0x4c_21d8) as u64);
    c.r[A0] = sx(k.hi(c, 0x4c_21dc));
    c.r[A1] = sx(c.rd32(k.a(c, 0x4c_21e0, 0x4c_21e4)));
    c.r[A0] |= (k.w(c, 0x4c_21e8) & 0xffff) as u64;
    c.wr32((c.r[A1] as u32).wrapping_add(4), c.r[V1] as u32);
    c.r[V0] = sx(c.rd32(k.a(c, 0x4c_21f0, 0x4c_21f4)));
    c.wr32((c.r[V0] as u32).wrapping_add(8), 0);
    c.r[V1] = sx(c.rd32(k.a(c, 0x4c_21fc, 0x4c_2200)));
    c.wr32((c.r[V1] as u32).wrapping_add(0xc), c.r[A0] as u32);
    c.r[V0] = sx(c.rd32(k.a(c, 0x4c_2208, 0x4c_220c)));
    c.r[V0] = add32(c.r[V0], 16);
    c.wr32(k.gp(c, gp, 0x4c_2218), c.r[V0] as u32);
}

// ---- 0x4A2200: the cull and chain builder ------------------------------------------------------------------------------------
//
// Hand-written: its translation's statements in order on its own register file (`r`, `h` the high halves), the blocks by
// level 1's labels. The class's instances come in by SPR_TO at 0x4A24E8 (`dw`); the packets go out by the SPR_FROM flushes
// (`flush`, `flush_some`, `last_flush`); 0x4A2AA8's `jr a3` goes back to the resume left at scratchpad 0x3FA0 (0x4A2820 by
// the first pass, 0x4A36E8 by the third).

fn f4a2200(c: &mut Cpu, k: F, wide: bool, n: &mut [u64; N]) {
    let (mut r, mut h) = (c.r, c.rh);
    let gp = r[GP] as u32;
    let mut l: u32 = 0x004a2200;
    loop {
        match l {
            0x004a2200 => {
                r[AT] = 0x0000000070000000;
                c.wr64((r[AT] as u32).wrapping_add(0x00003ff8), r[RA]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003ff0), r[FP]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fe8), r[SP]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fe0), r[S7]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fd8), r[S6]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fd0), r[S5]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fc8), r[S4]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fc0), r[S3]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fb8), r[S2]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fb0), r[S1]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fa8), r[S0]);
                r[RA] = 0x0000000070000000;
                r[S3] = sx(c.rd32(k.gp(c, gp, 0x004a2234)));
                r[AT] = sx(c.rd32(k.gp(c, gp, 0x004a2238)));
                r[V0] = sx(k.hi(c, 0x004a223c));
                r[V0] = add32(r[V0], k.lo(c, 0x004a2240) as u64);
                r[V1] = sx(c.rd32(k.gp(c, gp, 0x004a2244)));
                r[A0] = sx(c.rd32(k.gp(c, gp, 0x004a2248)));
                r[A1] = add32(r[RA], 0x00003700);
                r[A2] = add32(r[RA], 0x00003d00);
                r[A3] = add32(r[RA], 0x00002000);
                r[T0] = add32(r[RA], 0x00003200);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f60), r[AT] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f64), r[V0] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003fa4), r[V1] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f88), r[A0] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f70), r[A1] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f74), r[A2] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f6c), r[A3] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f8c), r[T0] as u32);
                r[S3] = add32(r[S3], 0xffffffc0);
                r[T9] = sx(0x00000000);
                r[V0] = add32(r[RA], 0x00003b00);
                r[AT] = sx(k.hi(c, 0x004a228c));
                r[AT] = add32(r[AT], k.lo(c, 0x004a2290) as u64);
                r[V1] = add32(r[V0], 0x00000100);
                l = 0x004a2298;
                continue;
            }
            0x004a2298 => {
                (r[A0], h[A0]) = c.rd128(r[AT] as u32);
                (r[A1], h[A1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                c.wr128(r[V0] as u32, r[A0], h[A0]);
                r[AT] = add32(r[AT], 0x00000020);
                c.wr128((r[V0] as u32).wrapping_add(0x00000010), r[A1], h[A1]);
                r[V0] = add32(r[V0], 0x00000020);
                let t = r[V0] != r[V1];
                if t {
                    l = 0x004a2298;
                    continue;
                }
                r[AT] = sx(k.hi(c, 0x004a22b8));
                r[AT] = add32(r[AT], k.lo(c, 0x004a22bc) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                c.wr128((r[RA] as u32).wrapping_add(0x00003c00), r[V0], h[V0]);
                c.wr128((r[RA] as u32).wrapping_add(0x00003c10), r[V1], h[V1]);
                r[V0] = sx(c.rd32(k.gp(c, gp, 0x004a22d0)));
                c.wr32((r[RA] as u32).wrapping_add(0x00003c20), r[V0] as u32);
                r[V0] = sx(k.hi(c, 0x004a22d8));
                r[V0] = add32(r[V0], k.lo(c, 0x004a22dc) as u64);
                c.wr32((r[RA] as u32).wrapping_add(0x00003fa0), r[V0] as u32);
                r[AT] = sx(0x00000002);
                r[V0] = sx(0x000000ee);
                r[V1] = sx(0x00000011);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f7c), r[AT] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f80), r[V0] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f84), r[V1] as u32);
                r[FP] = sx(c.rd32(k.gp(c, gp, 0x004a2300)));
                r[SP] = add32(r[RA], 0x00000000);
                r[AT] = add32(r[RA], 0x00001000);
                r[V0] = add32(r[RA], 0x00002000);
                l = 0x004a2310;
                continue;
            }
            0x004a2310 => {
                c.wr128(r[AT] as u32, 0u64, 0u64);
                c.wr128((r[AT] as u32).wrapping_add(0x00000010), 0u64, 0u64);
                r[V1] = r[AT];
                c.wr128((r[AT] as u32).wrapping_add(0x00000020), 0u64, 0u64);
                r[AT] = add32(r[AT], 0x00000040);
                let t = r[AT] != r[V0];
                c.wr128((r[V1] as u32).wrapping_add(0x00000030), 0u64, 0u64);
                if t {
                    l = 0x004a2310;
                    continue;
                }
                r[V0] = 0x0000000044800000;
                r[AT] = sx(k.hi(c, 0x004a2334));
                r[AT] = add32(r[AT], k.lo(c, 0x004a2338) as u64);
                qmtc2(c, 1, (r[V0], h[V0]));
                lqc2(c, 24, (r[AT] as u32).wrapping_add(0x00000140));
                lqc2(c, 25, r[AT] as u32);
                lqc2(c, 26, (r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 27, (r[AT] as u32).wrapping_add(0x00000020));
                lqc2(c, 28, (r[AT] as u32).wrapping_add(0x00000080));
                lqc2(c, 29, (r[AT] as u32).wrapping_add(0x00000090));
                lqc2(c, 30, (r[AT] as u32).wrapping_add(0x000000a0));
                lqc2(c, 31, (r[AT] as u32).wrapping_add(0x000000b0));
                fm::<6, 1>(c, 24, 0, b4(c.vf[1][0]));
                r[AT] = sx(k.hi(c, 0x004a2368));
                r[AT] = add32(r[AT], k.lo(c, 0x004a236c) as u64);
                lqc2(c, 18, (r[AT] as u32).wrapping_add(0x00000230));
                lqc2(c, 19, (r[AT] as u32).wrapping_add(0x000000a0));
                {
                    let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000b0));
                    let (hh, vv) = if wide { (c.widen(a as u32), c.fov((a >> 32) as u32)) } else { (a as u32, (a >> 32) as u32) };
                    c.vf[20] = [hh, vv, b as u32, (b >> 32) as u32];
                }
                lqc2(c, 21, (r[AT] as u32).wrapping_add(0x000001e0));
                lqc2(c, 22, (r[AT] as u32).wrapping_add(0x000001f0));
                lqc2(c, 23, (r[AT] as u32).wrapping_add(0x00000200));
                r[AT] = 0x000000003a800000;
                qmtc2(c, 1, (r[AT], h[AT]));
                fm::<6, 12>(c, 19, 19, b4(c.vf[1][0]));
                fm::<6, 12>(c, 23, 23, b4(c.vf[1][0]));
                (r[AT], h[AT]) = c.rd128(k.gp(c, gp, 0x004a2398));
                (r[V0], h[V0]) = c.rd128(k.gp(c, gp, 0x004a239c));
                (r[V1], h[V1]) = c.rd128(k.gp(c, gp, 0x004a23a0));
                (r[A0], h[A0]) = c.rd128(k.gp(c, gp, 0x004a23a4));
                (r[A1], h[A1]) = c.rd128(k.gp(c, gp, 0x004a23a8));
                (r[A2], h[A2]) = c.rd128(k.gp(c, gp, 0x004a23ac));
                c.wr128((r[RA] as u32).wrapping_add(0x00003f00), r[AT], h[AT]);
                c.wr128((r[RA] as u32).wrapping_add(0x00003f10), r[V0], h[V0]);
                c.wr128((r[RA] as u32).wrapping_add(0x00003f20), r[V1], h[V1]);
                c.wr128((r[RA] as u32).wrapping_add(0x00003f30), r[A0], h[A0]);
                c.wr128((r[RA] as u32).wrapping_add(0x00003f40), r[A1], h[A1]);
                c.wr128((r[RA] as u32).wrapping_add(0x00003f50), r[A2], h[A2]);
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[V0] = sx(k.hi(c, 0x004a23cc));
                r[V0] = add32(r[V0], k.lo(c, 0x004a23d0) as u64);
                r[AT] = sx(k.hi(c, 0x004a23d4));
                r[AT] = add32(r[AT], k.lo(c, 0x004a23d8) as u64);
                r[V0] = sx(c.rd32(r[V0] as u32));
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
                r[AT] = 0x0000000030000000;
                c.wr32(r[SP] as u32, r[AT] as u32);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16(r[SP] as u32, r[V0] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                r[AT] = sx(k.hi(c, 0x004a23f8));
                r[AT] = add32(r[AT], k.lo(c, 0x004a23fc) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                (r[A0], h[A0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000020));
                (r[A1], h[A1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000030));
                (r[A2], h[A2]) = c.rd128((r[AT] as u32).wrapping_add(0x00000040));
                c.wr128(r[SP] as u32, r[V0], h[V0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), r[A2], h[A2]);
                r[SP] = add32(r[SP], 0x00000050);
                r[FP] = add32(r[FP], 0x00000050);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f90), r[FP] as u32);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[SP] = add32(r[SP], 0x00000010);
                r[AT] = sx(k.hi(c, 0x004a2440));
                r[AT] = add32(r[AT], k.lo(c, 0x004a2444) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                (r[A0], h[A0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000020));
                (r[A1], h[A1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000030));
                c.wr128(r[SP] as u32, r[V0], h[V0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
                r[SP] = add32(r[SP], 0x00000040);
                r[FP] = add32(r[FP], 0x00000040);
                (r[T9], h[T9]) = mmi::pcpyld((r[FP], h[FP]), (r[T9], h[T9]));
                l = 0x004a2478;
                continue;
            }
            0x004a2478 => {
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f60)));
                r[V0] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f64)));
                let t = r[AT] == r[T9];
                if t {
                    l = 0x004a2d28;
                    continue;
                }
                n[0] += 1;
                r[T8] = sx(c.rd32(r[V0] as u32));
                r[V0] = add32(r[V0], 0x00000004);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f64), r[V0] as u32);
                r[V1] = 0x0000000050000000;
                qmtc2(c, 17, (r[V1], h[V1]));
                r[S0] = sx(0x00000000);
                r[AT] = c.rd16((r[T8] as u32).wrapping_add(0x00000014)) as u64;
                r[S1] = sx(0x00000000);
                r[S6] = sx(c.rd32((r[T8] as u32).wrapping_add(0x00000018)));
                r[S2] = sx(0x00000000);
                r[S7] = c.rd16((r[T8] as u32).wrapping_add(0x00000016)) as u64;
                r[SP] = add32(r[SP], 0x00000010);
                r[V0] = r[AT] & 0x1;
                r[AT] = r[AT] & 0x6;
                let t = r[V0] != 0u64;
                r[FP] = add32(r[FP], 0x00000010);
                if t {
                    l = 0x004a2cd0;
                    continue;
                }
                r[T6] = sx(c.rd32((r[T8] as u32).wrapping_add(0x0000001c)));
                r[AT] = srl(r[AT], 1);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f9c), r[AT] as u32);
                r[AT] = add32(r[RA], 0x00002c00);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f68), r[AT] as u32);
                (r[S3], h[S3]) = mmi::pcpyld((r[FP], h[FP]), (r[S3], h[S3]));
                l = 0x004a24e8;
                continue;
            }
            0x004a24e8 => {
                let t = r[S7] == 0u64;
                r[T2] = 0x0000000010000000;
                if t {
                    l = 0x004a2ab0;
                    continue;
                }
                r[T2] = r[T2] | 0xd400;
                r[T7] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003fa4)));
                r[V0] = sx(0x00001000);
                r[AT] = sx(0x00000080);
                dw(c, (r[T2] as u32).wrapping_add(0x00000080), r[V0] as u32);
                (r[AT], h[AT]) = mmi::pminw((r[AT], h[AT]), (r[S7], h[S7]));
                dw(c, (r[T2] as u32).wrapping_add(0x00000010), r[S6] as u32);
                r[V0] = sll(r[AT], 1);
                dw(c, (r[T2] as u32).wrapping_add(0x00000020), r[V0] as u32);
                r[V0] = 0u64 | 0x100;
                dw(c, r[T2] as u32, r[V0] as u32);
                r[S7] = sub32(r[S7], r[AT]);
                r[S4] = add32(r[RA], 0x00001000);
                r[S5] = sll(r[AT], 5);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f78), r[S6] as u32);
                r[S6] = add32(r[S6], r[S5]);
                r[S5] = add32(r[S4], r[S5]);
                r[S4] = add32(r[S4], 0xffffffe0);
                l = 0x004a2538;
                continue;
            }
            0x004a2538 => {
                r[V0] = sx(c.rd32((r[S4] as u32).wrapping_add(0x00000030)));
                r[S4] = add32(r[S4], 0x00000020);
                let t = r[S4] == r[S5];
                lqc2(c, 1, r[S4] as u32);
                if t {
                    l = 0x004a2928;
                    continue;
                }
                n[1] += 1;
                (r[V0], h[V0]) = mmi::pminw((r[V0], h[V0]), (r[T7], h[T7]));
                r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000016)) as u64;
                fm::<1, 14>(c, 2, 1, c.vf[24]);
                fm::<0, 8>(c, 3, 0, b4(c.vf[1][3]));
                r[V0] = r[V0] << 32;
                qmtc2(c, 4, (r[V0], h[V0]));
                let t = r[V0] == 0u64;
                fm::<1, 4>(c, 3, 0, b4(c.vf[1][3]));
                if t {
                    l = 0x004a2918;
                    continue;
                }
                fm::<6, 14>(c, 32, 25, b4(c.vf[2][0]));
                fm::<2, 14>(c, 32, 26, b4(c.vf[2][1]));
                r[S3] = add32(r[S3], 0x00000040);
                fm::<2, 14>(c, 2, 27, b4(c.vf[2][2]));
                c.wr32((r[S4] as u32).wrapping_add(0x00000010), 0u64 as u32);
                fm::<1, 4>(c, 19, 4, b4(c.vf[1][3]));
                fm::<6, 12>(c, 8, 22, b4(c.vf[1][3]));
                r[V1] = r[V1] & 0x1;
                let t = r[V1] != 0u64;
                if t {
                    l = 0x004a2538;
                    continue;
                }
                fm::<0, 12>(c, 1, 3, b4(c.vf[2][2]));
                fm::<6, 12>(c, 5, 20, b4(c.vf[2][2]));
                uycpu::vu0::abs::<12>(c, 2, 2);
                fm::<6, 12>(c, 7, 8, b4(c.vf[22][2]));
                fm::<1, 12>(c, 3, 19, c.vf[1]);
                fm::<6, 12>(c, 9, 5, c.vf[21]);
                fm::<1, 12>(c, 6, 2, c.vf[8]);
                fm::<0, 12>(c, 8, 2, c.vf[7]);
                fm::<1, 12>(c, 4, 1, c.vf[23]);
                fm::<4, 2>(c, 1, 0, c.vf[2]);
                (r[AT], h[AT]) = q4(c.vf[3]);
                fm::<1, 12>(c, 7, 5, c.vf[6]);
                r[A1] = 0x0000000040000000;
                fm::<1, 12>(c, 8, 9, c.vf[8]);
                r[V0] = r[AT] << 32;
                let t = (r[AT] as i64) < 0;
                (r[A0], h[A0]) = q4(c.vf[4]);
                if t {
                    l = 0x004a2538;
                    continue;
                }
                let t = (r[V0] as i64) >= 0;
                {
                    let v = c.vf[1];
                    uycpu::vu0::mov::<12>(c, 1, [v[1], v[2], v[3], v[0]]);
                }
                if t {
                    l = 0x004a2538;
                    continue;
                }
                uycpu::vu0::ftoi::<2, 12>(c, 3, 3);
                r[T2] = c.rd16((r[S4] as u32).wrapping_add(0x00000018)) as u64;
                (r[V0], h[V0]) = q4(c.vf[7]);
                r[A1] = r[A1].wrapping_add(r[A1]);
                (r[V1], h[V1]) = q4(c.vf[8]);
                r[A0] = r[A0] ^ r[A1];
                let t = (r[V0] as i64) < 0;
                r[V0] = r[V0] << 32;
                if t {
                    l = 0x004a2538;
                    continue;
                }
                let t = (r[V0] as i64) < 0;
                r[V1] = r[V1] | r[A0];
                if t {
                    l = 0x004a2538;
                    continue;
                }
                fm::<6, 8>(c, 2, 1, b4(c.vf[24][3]));
                lqc2(c, 4, (r[S3] as u32).wrapping_add(0x00000030));
                r[AT] = c.rd8((r[S4] as u32).wrapping_add(0x0000001b)) as u64;
                r[V0] = c.rd16((r[S4] as u32).wrapping_add(0x0000001e)) as u64;
                let t = r[AT] != 0u64;
                r[AT] = 0u64 | 0xffff;
                if t {
                    l = 0x004a2668;
                    continue;
                }
                let t = r[AT] == r[V0];
                if t {
                    l = 0x004a2688;
                    continue;
                }
                l = 0x004a2668;
                continue;
            }
            0x004a2668 => {
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f8c)));
                r[V0] = add32(r[RA], 0x000035fe);
                let t = r[AT] == r[V0];
                if t {
                    l = 0x004a2688;
                    continue;
                }
                c.wr16(r[AT] as u32, r[T2] as u16);
                r[AT] = add32(r[AT], 0x00000002);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f8c), r[AT] as u32);
                l = 0x004a2688;
                continue;
            }
            0x004a2688 => {
                let t = (r[V1] as i64) < 0;
                r[V1] = r[V1] << 32;
                if t {
                    l = 0x004a28e0;
                    continue;
                }
                let t = (r[V1] as i64) < 0;
                uycpu::vu0::ftoi::<2, 4>(c, 5, 1);
                if t {
                    l = 0x004a28e0;
                    continue;
                }
                let t = r[T6] == 0u64;
                (r[V1], h[V1]) = q4(c.vf[3]);
                if t {
                    l = 0x004a2750;
                    continue;
                }
                r[AT] = c.rd8((r[S4] as u32).wrapping_add(0x00000017)) as u64;
                r[V1] = 0u64 | 0x8000;
                r[A2] = 0u64 | 0x8000;
                let t = r[AT] == 0u64;
                r[V1] = r[V1] << 32;
                if t {
                    l = 0x004a26e0;
                    continue;
                }
                (r[V0], h[V0]) = q4(c.vf[5]);
                r[AT] = r[AT] << 44;
                r[A0] = 0u64 | 0x8000;
                r[V1] = r[V1].wrapping_add(r[AT]);
                r[A2] = r[V0].wrapping_sub(r[AT]);
                r[V1] = r[V1].wrapping_sub(r[V0]);
                let t = (r[A2] as i64) < 0;
                r[A2] = ((r[A2] as i64) >> 32) as u64;
                if t {
                    l = 0x004a2750;
                    continue;
                }
                let t = 0u64 != 0u64;
                (r[A2], h[A2]) = mmi::pminw((r[A2], h[A2]), (r[A0], h[A0]));
                if t {
                    l = 0x004a2538;
                    continue;
                }
                l = 0x004a26e0;
                continue;
            }
            0x004a26e0 => {
                r[A2] = srl(r[A2], 8);
                r[V0] = sx(0x00000080);
                let t = r[A2] != r[V0];
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f6c)));
                if t {
                    l = 0x004a2728;
                    continue;
                }
                (r[V1], h[V1]) = q4(c.vf[3]);
                r[V0] = add32(r[RA], 0x00002c00);
                let t = r[AT] == r[V0];
                r[V1] = ((r[V1] as i64) >> 33) as u64;
                if t {
                    l = 0x004a2538;
                    continue;
                }
                r[V0] = 0u64 | 0x8000;
                r[S0] = add32(r[S0], 0x00000001);
                (r[V1], h[V1]) = mmi::pminw((r[V1], h[V1]), (r[V0], h[V0]));
                c.wr16(r[AT] as u32, r[T2] as u16);
                r[V1] = srl(r[V1], 8);
                c.wr16((r[AT] as u32).wrapping_add(0x00000002), r[V1] as u16);
                r[AT] = add32(r[AT], 0x00000004);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f6c), r[AT] as u32);
                l = 0x004a2538;
                continue;
            }
            0x004a2728 => {
                r[V0] = add32(r[RA], 0x00002c00);
                let t = r[AT] == r[V0];
                if t {
                    l = 0x004a2538;
                    continue;
                }
                c.wr16((r[AT] as u32).wrapping_add(0x00000002), r[A2] as u16);
                r[S0] = add32(r[S0], 0x00000001);
                c.wr16(r[AT] as u32, r[T2] as u16);
                r[AT] = add32(r[AT], 0x00000004);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f6c), r[AT] as u32);
                l = 0x004a2750;
                continue;
            }
            0x004a2750 => {
                (r[A0], h[A0]) = mmi::pcpyud((r[T9], h[T9]), (0u64, 0u64));
                fm::<5, 8>(c, 17, 2, c.vf[17]);
                r[A0] = sub32(r[FP], r[A0]);
                r[V1] = ((r[V1] as i64) >> 32) as u64;
                r[V0] = 0u64 | 0x8000;
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), 0u64, 0u64);
                (r[V1], h[V1]) = mmi::pminw((r[V1], h[V1]), (r[V0], h[V0]));
                let t = r[V1] != r[V0];
                r[A1] = sll(r[T2], 19);
                if t {
                    l = 0x004a2798;
                    continue;
                }
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f68)));
                r[V0] = add32(r[RA], 0x00003000);
                let t = r[AT] == r[V0];
                r[A0] = r[A0] | r[A1];
                if t {
                    l = 0x004a2538;
                    continue;
                }
                c.wr32(r[AT] as u32, r[A0] as u32);
                r[AT] = add32(r[AT], 0x00000004);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f68), r[AT] as u32);
                l = 0x004a27f0;
                continue;
            }
            0x004a2798 => {
                r[A0] = r[A0] | r[A1];
                r[A1] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f88)));
                r[A2] = sll(r[T2], 6);
                r[A3] = sll(r[T2], 5);
                r[A2] = add32(r[A2], r[A3]);
                r[V1] = add32(r[V1], 0x000000ff);
                r[A1] = add32(r[A1], r[A2]);
                r[V1] = srl(r[V1], 8);
                r[A1] = sx(c.rd32(r[A1] as u32));
                let t = 0u64 != 0u64;
                (r[A1], h[A1]) = mmi::pextlb((0u64, 0u64), (r[A1], h[A1]));
                if t {
                    l = 0x004a2538;
                    continue;
                }
                (r[A1], h[A1]) = mmi::pextlh((0u64, 0u64), (r[A1], h[A1]));
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f70)));
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), r[A1], h[A1]);
                r[V0] = add32(r[RA], 0x00003b00);
                let t = r[AT] == r[V0];
                c.wr32((r[SP] as u32).wrapping_add(0x0000004c), r[V1] as u32);
                if t {
                    l = 0x004a2538;
                    continue;
                }
                c.wr32(r[AT] as u32, r[A0] as u32);
                r[AT] = add32(r[AT], 0x00000004);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f70), r[AT] as u32);
                r[S2] = add32(r[S2], 0x00000001);
                l = 0x004a27f0;
                continue;
            }
            0x004a27f0 => {
                n[2] += 1;
                lqc2(c, 1, r[S3] as u32);
                r[A3] = add32(r[S3], 0x00000000);
                lqc2(c, 2, (r[S3] as u32).wrapping_add(0x00000010));
                lqc2(c, 3, (r[S3] as u32).wrapping_add(0x00000020));
                fm::<6, 14>(c, 1, 1, b4(c.vf[4][3]));
                fm::<6, 14>(c, 2, 2, b4(c.vf[4][3]));
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f9c)));
                fm::<6, 14>(c, 3, 3, b4(c.vf[4][3]));
                r[SP] = add32(r[SP], 0x00000050);
                let t = r[AT] != 0u64;
                fm::<1, 14>(c, 4, 4, c.vf[24]);
                if t {
                    l = 0x004a2930;
                    continue;
                }
                l = 0x004a2820;
                continue;
            }
            0x004a2820 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[1][0]));
                l = 0x004a2824;
                continue;
            }
            0x004a2824 => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[1][1]));
                l = 0x004a2828;
                continue;
            }
            0x004a2828 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[1][2]));
                l = 0x004a282c;
                continue;
            }
            0x004a282c => {
                fm::<2, 15>(c, 1, 31, b4(c.vf[0][0]));
                l = 0x004a2830;
                continue;
            }
            0x004a2830 => {
                fm::<6, 14>(c, 4, 4, b4(c.vf[24][3]));
                l = 0x004a2834;
                continue;
            }
            0x004a2834 => {
                r[FP] = add32(r[FP], 0x00000050);
                l = 0x004a2838;
                continue;
            }
            0x004a2838 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[2][0]));
                l = 0x004a283c;
                continue;
            }
            0x004a283c => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[2][1]));
                l = 0x004a2840;
                continue;
            }
            0x004a2840 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[2][2]));
                l = 0x004a2844;
                continue;
            }
            0x004a2844 => {
                fm::<2, 15>(c, 2, 31, b4(c.vf[0][0]));
                l = 0x004a2848;
                continue;
            }
            0x004a2848 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[3][0]));
                l = 0x004a284c;
                continue;
            }
            0x004a284c => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[3][1]));
                l = 0x004a2850;
                continue;
            }
            0x004a2850 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[3][2]));
                l = 0x004a2854;
                continue;
            }
            0x004a2854 => {
                fm::<2, 15>(c, 3, 31, b4(c.vf[0][0]));
                l = 0x004a2858;
                continue;
            }
            0x004a2858 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[4][0]));
                l = 0x004a285c;
                continue;
            }
            0x004a285c => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[4][1]));
                l = 0x004a2860;
                continue;
            }
            0x004a2860 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[4][2]));
                l = 0x004a2864;
                continue;
            }
            0x004a2864 => {
                fm::<2, 15>(c, 4, 31, b4(c.vf[0][3]));
                l = 0x004a2868;
                continue;
            }
            0x004a2868 => {
                sqc2(c, 1, (r[SP] as u32).wrapping_add(0xffffffb0));
                l = 0x004a286c;
                continue;
            }
            0x004a286c => {
                l = 0x004a2870;
                continue;
            }
            0x004a2870 => {
                sqc2(c, 2, (r[SP] as u32).wrapping_add(0xffffffc0));
                l = 0x004a2874;
                continue;
            }
            0x004a2874 => {
                r[AT] = r[SP] & 0x600;
                l = 0x004a2878;
                continue;
            }
            0x004a2878 => {
                sqc2(c, 3, (r[SP] as u32).wrapping_add(0xffffffd0));
                l = 0x004a287c;
                continue;
            }
            0x004a287c => {
                r[V0] = sx(0x00000600);
                l = 0x004a2880;
                continue;
            }
            0x004a2880 => {
                let t = r[AT] != r[V0];
                sqc2(c, 4, (r[SP] as u32).wrapping_add(0xffffffe0));
                if t {
                    l = 0x004a2538;
                    continue;
                }
                flush(c, &mut r);
                l = 0x004a2538;
                continue;
            }
            0x004a28e0 => {
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f74)));
                r[V0] = add32(r[RA], 0x00003f00);
                fm::<5, 8>(c, 17, 2, c.vf[17]);
                let t = r[AT] == r[V0];
                r[V1] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f78)));
                if t {
                    l = 0x004a2538;
                    continue;
                }
                r[A0] = r[S4] & 0xfff;
                r[S1] = add32(r[S1], 0x00000001);
                r[V1] = add32(r[V1], r[A0]);
                c.wr32(r[AT] as u32, r[V1] as u32);
                r[AT] = add32(r[AT], 0x00000004);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f74), r[AT] as u32);
                l = 0x004a2538;
                continue;
            }
            0x004a2918 => {
                r[S4] = add32(r[S4], 0xffffffe0);
                l = 0x004a2538;
                continue;
            }
            0x004a2928 => {
                l = 0x004a24e8;
                continue;
            }
            0x004a2930 => {
                n[3] += 1;
                r[A2] = r[RA] | 0x3c00;
                lqc2(c, 9, (r[A2] as u32).wrapping_add(0x00000010));
                lqc2(c, 8, r[A2] as u32);
                r[A1] = sx(c.rd32((r[A2] as u32).wrapping_add(0x00000020)));
                r[AT] = srl(r[AT], 1);
                fm::<0, 8>(c, 11, 0, b4(c.vf[0][3]));
                fm::<6, 14>(c, 12, 4, c.vf[4]);
                r[A2] = r[RA] | 0x3b00;
                fm::<0, 8>(c, 32, 12, b4(c.vf[12][1]));
                fm::<2, 8>(c, 12, 11, b4(c.vf[12][2]));
                fm::<1, 4>(c, 0, 9, b4(c.vf[12][0]));
                r[A0] = sx(c.vi[17]);
                fm::<6, 8>(c, 12, 12, b4(c.vf[9][2]));
                r[A0] = r[A0] & 0x40;
                let t = r[A0] != 0u64;
                fm::<1, 1>(c, 12, 0, b4(c.vf[12][0]));
                if t {
                    l = 0x004a2aa0;
                    continue;
                }
                fm::<6, 15>(c, 5, 0, b4(c.vf[0][0]));
                r[V0] = sll(r[A3], 6);
                fm::<6, 15>(c, 6, 0, b4(c.vf[0][0]));
                r[A0] = sll(r[A3], 1);
                {
                    let v = c.vf[0];
                    uycpu::vu0::mov::<15>(c, 7, [v[1], v[2], v[3], v[0]]);
                }
                r[V0] = add32(r[V0], r[A0]);
                fm::<0, 8>(c, 5, 5, b4(c.vf[0][3]));
                r[V0] = add32(r[V0], r[A3]);
                fm::<0, 4>(c, 6, 6, b4(c.vf[0][3]));
                r[V1] = sll(r[A3], 7);
                r[A0] = sll(r[A3], 2);
                r[V1] = sub32(r[V1], r[A0]);
                r[V1] = sub32(r[V1], r[A3]);
                r[V0] = add32(r[V0], r[A1]);
                let t = r[AT] == 0u64;
                r[V1] = add32(r[V1], r[A1]);
                if t {
                    l = 0x004a29c8;
                    continue;
                }
                r[V1] = add32(r[V1], r[A1]);
                l = 0x004a29c8;
                continue;
            }
            0x004a29c8 => {
                r[A0] = r[V0] & 0xff;
                r[A0] = add32(r[A0], r[A2]);
                r[A0] = c.rd8(r[A0] as u32) as i8 as i64 as u64;
                r[A0] = sll(r[A0], 5);
                qmtc2(c, 10, (r[A0], h[A0]));
                uycpu::vu0::itof::<2, 15>(c, 10, 10);
                r[A0] = srl(r[V0], 1);
                r[A0] = r[A0] & 0xff;
                r[A0] = add32(r[A0], r[A2]);
                r[A0] = c.rd8(r[A0] as u32) as i8 as i64 as u64;
                r[A0] = sll(r[A0], 5);
                qmtc2(c, 11, (r[A0], h[A0]));
                uycpu::vu0::itof::<2, 15>(c, 11, 11);
                fm::<6, 8>(c, 13, 10, c.vf[11]);
                r[A0] = add32(r[V1], 0x00000040);
                fm::<1, 1>(c, 13, 0, b4(c.vf[9][0]));
                r[A0] = r[A0] & 0xff;
                fm::<6, 8>(c, 13, 13, b4(c.vf[9][0]));
                r[A0] = add32(r[A0], r[A2]);
                fm::<0, 8>(c, 13, 13, b4(c.vf[13][3]));
                r[A0] = c.rd8(r[A0] as u32) as i8 as i64 as u64;
                r[A0] = sll(r[A0], 5);
                qmtc2(c, 10, (r[A0], h[A0]));
                uycpu::vu0::itof::<2, 15>(c, 10, 10);
                r[A0] = srl(r[V1], 1);
                r[A0] = r[A0] & 0xff;
                r[A0] = add32(r[A0], r[A2]);
                r[A0] = c.rd8(r[A0] as u32) as i8 as i64 as u64;
                r[A0] = sll(r[A0], 5);
                qmtc2(c, 11, (r[A0], h[A0]));
                uycpu::vu0::itof::<2, 15>(c, 11, 11);
                fm::<6, 8>(c, 13, 13, b4(c.vf[8][0]));
                fm::<6, 8>(c, 32, 10, b4(c.vf[8][1]));
                fm::<2, 8>(c, 10, 13, b4(c.vf[8][2]));
                fm::<6, 8>(c, 10, 10, b4(c.vf[12][3]));
                fm::<6, 8>(c, 32, 11, b4(c.vf[8][1]));
                fm::<2, 8>(c, 11, 13, b4(c.vf[8][3]));
                let t = r[AT] != 0u64;
                fm::<6, 8>(c, 11, 11, b4(c.vf[12][3]));
                if t {
                    l = 0x004a2a70;
                    continue;
                }
                fm::<6, 8>(c, 10, 10, b4(c.vf[9][3]));
                fm::<6, 8>(c, 11, 11, b4(c.vf[9][3]));
                l = 0x004a2a70;
                continue;
            }
            0x004a2a70 => {
                fm::<0, 8>(c, 7, 0, b4(c.vf[10][0]));
                fm::<0, 4>(c, 7, 0, b4(c.vf[11][0]));
                fm::<6, 14>(c, 32, 5, b4(c.vf[1][0]));
                fm::<2, 14>(c, 32, 6, b4(c.vf[1][1]));
                fm::<2, 14>(c, 1, 7, b4(c.vf[1][2]));
                fm::<6, 14>(c, 32, 5, b4(c.vf[2][0]));
                fm::<2, 14>(c, 32, 6, b4(c.vf[2][1]));
                fm::<2, 14>(c, 2, 7, b4(c.vf[2][2]));
                fm::<6, 14>(c, 32, 5, b4(c.vf[3][0]));
                fm::<2, 14>(c, 32, 6, b4(c.vf[3][1]));
                fm::<2, 14>(c, 3, 7, b4(c.vf[3][2]));
                l = 0x004a2aa0;
                continue;
            }
            0x004a2aa0 => {
                r[A3] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003fa0)));
                let t = r[A3] as u32;
                l = if t == k.pc(0x004a2820) {
                    0x004a2820
                } else if t == k.pc(0x004a36e8) {
                    0x004a36e8
                } else {
                    panic!("the shrub cull's jump at {:08x} to {t:08x}: neither of its own resumes", k.pc(0x004a2aa8))
                };
                continue;
            }
            0x004a2ab0 => {
                let t = 0u64 != 0u64;
                (r[A0], h[A0]) = mmi::pcpyud((r[S3], h[S3]), (0u64, 0u64));
                if t {
                    l = 0x004a2cd8;
                    continue;
                }
                let t = r[A0] == r[FP];
                uycpu::vu0::ftoi::<0, 8>(c, 17, 17);
                if t {
                    l = 0x004a2cd8;
                    continue;
                }
                r[AT] = srl(r[A0], 4);
                r[V0] = sub32(r[FP], r[A0]);
                r[V0] = sll(r[V0], 16);
                r[S6] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f68)));
                r[V1] = add32(r[V0], r[AT]);
                r[V0] = sll(r[T9], 1);
                r[V0] = add32(r[RA], r[V0]);
                c.wr16((r[V0] as u32).wrapping_add(0x00003600), r[S0] as u16);
                r[AT] = sll(r[T9], 2);
                c.wr16((r[V0] as u32).wrapping_add(0x00003c80), r[S1] as u16);
                r[AT] = add32(r[RA], r[AT]);
                c.wr16((r[V0] as u32).wrapping_add(0x00003680), r[S2] as u16);
                r[T9] = add32(r[T9], 0x00000001);
                (r[V0], h[V0]) = q4(c.vf[17]);
                r[S6] = sub32(r[S6], r[RA]);
                c.wr32((r[AT] as u32).wrapping_add(0x00003000), r[V1] as u32);
                r[S6] = add32(r[S6], 0xffffd400);
                c.wr32((r[AT] as u32).wrapping_add(0x00003100), r[V0] as u32);
                r[S6] = srl(r[S6], 2);
                c.wr16((r[T8] as u32).wrapping_add(0x00000038), r[S0] as u16);
                r[AT] = add32(r[S2], r[S6]);
                c.wr16((r[T8] as u32).wrapping_add(0x00000036), r[S1] as u16);
                c.wr16((r[T8] as u32).wrapping_add(0x00000034), r[AT] as u16);
                let t = r[S6] == 0u64;
                r[S5] = c.rd16((r[T8] as u32).wrapping_add(0x00000028)) as u64;
                if t {
                    l = 0x004a2478;
                    continue;
                }
                r[S4] = add32(r[T8], 0x00000040);
                r[S5] = sll(r[S5], 3);
                r[S5] = add32(r[S4], r[S5]);
                r[A3] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f7c)));
                r[T0] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f80)));
                r[T1] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f84)));
                (r[T2], h[T2]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f00));
                (r[T3], h[T3]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f10));
                (r[T4], h[T4]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f20));
                (r[T5], h[T5]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f30));
                (r[T6], h[T6]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f40));
                (r[T7], h[T7]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f50));
                l = 0x004a2b60;
                continue;
            }
            0x004a2b60 => {
                r[AT] = sx(c.rd32(r[S4] as u32));
                r[V0] = sx(c.rd32((r[S4] as u32).wrapping_add(0x00000004)));
                r[S4] = add32(r[S4], 0x00000008);
                c.wr128(r[SP] as u32, r[T2], h[T2]);
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
                r[V0] = srl(r[V0], 4);
                c.wr16(r[SP] as u32, r[V0] as u16);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16((r[SP] as u32).wrapping_add(0x00000008), r[A3] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                c.wr128(r[SP] as u32, r[T3], h[T3]);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16((r[SP] as u32).wrapping_add(0x0000000c), r[T1] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                r[A2] = add32(r[RA], 0x00002c00);
                r[S7] = add32(r[S6], 0x00000000);
                l = 0x004a2ba8;
                continue;
            }
            0x004a2ba8 => {
                r[AT] = sx(0x00000005);
                r[A1] = add32(r[T0], 0x00000001);
                let t = r[S7] == 0u64;
                (r[A0], h[A0]) = mmi::pminw((r[S7], h[S7]), (r[AT], h[AT]));
                if t {
                    l = 0x004a2cb0;
                    continue;
                }
                c.wr128(r[SP] as u32, r[T4], h[T4]);
                r[S7] = sub32(r[S7], r[A0]);
                c.wr16((r[SP] as u32).wrapping_add(0x00000008), r[T0] as u16);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr32((r[SP] as u32).wrapping_add(0x0000000c), r[A0] as u32);
                r[SP] = add32(r[SP], 0x00000010);
                l = 0x004a2bd0;
                continue;
            }
            0x004a2bd0 => {
                r[AT] = sx(c.rd32(r[A2] as u32));
                r[A2] = add32(r[A2], 0x00000004);
                c.wr128(r[SP] as u32, r[T5], h[T5]);
                (r[V1], h[V1]) = mmi::pcpyud((r[T9], h[T9]), (0u64, 0u64));
                r[A0] = add32(r[A0], 0xffffffff);
                r[V0] = sll(r[AT], 13);
                r[AT] = srl(r[AT], 19);
                r[V0] = srl(r[V0], 13);
                c.wr16((r[SP] as u32).wrapping_add(0x0000000c), r[A1] as u16);
                r[V0] = add32(r[V0], r[V1]);
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[V0] as u32);
                r[V1] = add32(r[A1], 0x00000004);
                r[V0] = r[AT] << 38;
                r[AT] = r[AT] << 37;
                r[AT] = r[AT].wrapping_add(r[V0]);
                r[A1] = add32(r[A1], 0x0000001c);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T7], h[T7]);
                r[AT] = r[AT].wrapping_add(r[T7]);
                r[V1] = r[V1] | 0x4000;
                c.wr64((r[SP] as u32).wrapping_add(0x00000010), r[AT]);
                c.wr16((r[SP] as u32).wrapping_add(0x0000001c), r[V1] as u16);
                r[FP] = add32(r[FP], 0x00000020);
                let t = r[A0] != 0u64;
                r[SP] = add32(r[SP], 0x00000020);
                if t {
                    l = 0x004a2bd0;
                    continue;
                }
                c.wr128(r[SP] as u32, r[T6], h[T6]);
                r[SP] = add32(r[SP], 0x00000010);
                r[AT] = sx(0x00000269);
                r[V0] = sx(0x00000026);
                r[T0] = sub32(r[AT], r[T0]);
                r[T1] = sub32(r[V0], r[T1]);
                r[AT] = r[SP] & 0x600;
                r[V0] = sx(0x00000600);
                let t = r[AT] != r[V0];
                r[FP] = add32(r[FP], 0x00000010);
                if t {
                    l = 0x004a2ba8;
                    continue;
                }
                flush(c, &mut r);
                l = 0x004a2ba8;
                continue;
            }
            0x004a2cb0 => {
                r[AT] = sx(0x0000007a);
                let t = r[S4] != r[S5];
                r[A3] = sub32(r[AT], r[A3]);
                if t {
                    l = 0x004a2b60;
                    continue;
                }
                c.wr32((r[RA] as u32).wrapping_add(0x00003f7c), r[A3] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f80), r[T0] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f84), r[T1] as u32);
                l = 0x004a2478;
                continue;
            }
            0x004a2cd0 => {
                r[S7] = sll(r[S7], 6);
                r[S3] = add32(r[S3], r[S7]);
                l = 0x004a2cd8;
                continue;
            }
            0x004a2cd8 => {
                r[SP] = add32(r[SP], 0xfffffff0);
                r[V0] = sll(r[T9], 1);
                r[V0] = add32(r[RA], r[V0]);
                r[FP] = add32(r[FP], 0xfffffff0);
                c.wr16((r[T8] as u32).wrapping_add(0x00000038), r[S0] as u16);
                c.wr16((r[T8] as u32).wrapping_add(0x00000036), r[S1] as u16);
                c.wr16((r[T8] as u32).wrapping_add(0x00000034), 0u64 as u16);
                c.wr16((r[V0] as u32).wrapping_add(0x00003600), r[S0] as u16);
                r[AT] = sll(r[T9], 2);
                c.wr16((r[V0] as u32).wrapping_add(0x00003c80), r[S1] as u16);
                r[AT] = add32(r[RA], r[AT]);
                c.wr16((r[V0] as u32).wrapping_add(0x00003680), r[S2] as u16);
                r[T9] = add32(r[T9], 0x00000001);
                c.wr32((r[AT] as u32).wrapping_add(0x00003000), 0u64 as u32);
                (r[V0], h[V0]) = q4(c.vf[17]);
                c.wr32((r[AT] as u32).wrapping_add(0x00003100), r[V0] as u32);
                l = 0x004a2478;
                continue;
            }
            0x004a2d28 => {
                r[AT] = sx(k.hi(c, 0x004a2d28));
                r[AT] = add32(r[AT], k.lo(c, 0x004a2d2c) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                (r[A0], h[A0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000020));
                c.wr128(r[SP] as u32, r[V0], h[V0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
                r[SP] = add32(r[SP], 0x00000030);
                r[FP] = add32(r[FP], 0x00000030);
                r[T9] = sx(0x00000000);
                r[S3] = sx(k.hi(c, 0x004a2d54));
                r[S3] = add32(r[S3], k.lo(c, 0x004a2d58) as u64);
                r[S2] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f60)));
                r[S1] = add32(r[RA], 0x00003680);
                r[S0] = add32(r[RA], 0x00003700);
                r[A3] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f7c)));
                r[T0] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f80)));
                r[T1] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f84)));
                (r[T2], h[T2]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f00));
                (r[T3], h[T3]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f10));
                (r[T4], h[T4]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f20));
                (r[T5], h[T5]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f30));
                (r[T6], h[T6]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f40));
                (r[T7], h[T7]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f50));
                l = 0x004a2d90;
                continue;
            }
            0x004a2d90 => {
                let t = r[T9] == r[S2];
                r[S6] = c.rd16(r[S1] as u32) as u64;
                if t {
                    l = 0x004a2f38;
                    continue;
                }
                r[T9] = add32(r[T9], 0x00000001);
                r[S1] = add32(r[S1], 0x00000002);
                let t = r[S6] == 0u64;
                r[S3] = add32(r[S3], 0x00000004);
                if t {
                    l = 0x004a2d90;
                    continue;
                }
                r[T8] = sx(c.rd32((r[S3] as u32).wrapping_add(0xfffffffc)));
                r[S4] = add32(r[T8], 0x00000040);
                r[S5] = c.rd16((r[T8] as u32).wrapping_add(0x00000028)) as u64;
                r[S5] = sll(r[S5], 3);
                r[S5] = add32(r[S4], r[S5]);
                l = 0x004a2dc8;
                continue;
            }
            0x004a2dc8 => {
                r[AT] = sx(c.rd32(r[S4] as u32));
                r[V0] = sx(c.rd32((r[S4] as u32).wrapping_add(0x00000004)));
                r[S4] = add32(r[S4], 0x00000008);
                c.wr128(r[SP] as u32, r[T2], h[T2]);
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
                r[V0] = srl(r[V0], 4);
                c.wr16(r[SP] as u32, r[V0] as u16);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16((r[SP] as u32).wrapping_add(0x00000008), r[A3] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                c.wr128(r[SP] as u32, r[T3], h[T3]);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16((r[SP] as u32).wrapping_add(0x0000000c), r[T1] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                r[S7] = add32(r[S6], 0x00000000);
                r[A2] = add32(r[S0], 0x00000000);
                l = 0x004a2e10;
                continue;
            }
            0x004a2e10 => {
                r[AT] = sx(0x00000005);
                r[A1] = add32(r[T0], 0x00000001);
                let t = r[S7] == 0u64;
                (r[A0], h[A0]) = mmi::pminw((r[S7], h[S7]), (r[AT], h[AT]));
                if t {
                    l = 0x004a2f18;
                    continue;
                }
                c.wr128(r[SP] as u32, r[T4], h[T4]);
                r[S7] = sub32(r[S7], r[A0]);
                c.wr16((r[SP] as u32).wrapping_add(0x00000008), r[T0] as u16);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr32((r[SP] as u32).wrapping_add(0x0000000c), r[A0] as u32);
                r[SP] = add32(r[SP], 0x00000010);
                l = 0x004a2e38;
                continue;
            }
            0x004a2e38 => {
                r[AT] = sx(c.rd32(r[A2] as u32));
                r[A2] = add32(r[A2], 0x00000004);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T5], h[T5]);
                (r[V1], h[V1]) = mmi::pcpyud((r[T9], h[T9]), (0u64, 0u64));
                r[A0] = add32(r[A0], 0xffffffff);
                r[V0] = sll(r[AT], 13);
                r[AT] = srl(r[AT], 19);
                r[V0] = srl(r[V0], 13);
                c.wr16((r[SP] as u32).wrapping_add(0x0000001c), r[A1] as u16);
                r[V0] = add32(r[V0], r[V1]);
                c.wr32((r[SP] as u32).wrapping_add(0x00000014), r[V0] as u32);
                r[V1] = add32(r[A1], 0x00000004);
                r[V0] = r[AT] << 38;
                r[AT] = r[AT] << 37;
                r[AT] = r[AT].wrapping_add(r[V0]);
                r[A1] = add32(r[A1], 0x0000001c);
                c.wr128(r[SP] as u32, r[T7], h[T7]);
                r[AT] = r[AT].wrapping_add(r[T7]);
                r[V1] = r[V1] | 0x4000;
                c.wr64(r[SP] as u32, r[AT]);
                c.wr16((r[SP] as u32).wrapping_add(0x0000000c), r[V1] as u16);
                r[FP] = add32(r[FP], 0x00000020);
                let t = r[A0] != 0u64;
                r[SP] = add32(r[SP], 0x00000020);
                if t {
                    l = 0x004a2e38;
                    continue;
                }
                c.wr128(r[SP] as u32, r[T6], h[T6]);
                r[SP] = add32(r[SP], 0x00000010);
                r[AT] = sx(0x00000269);
                r[V0] = sx(0x00000026);
                r[T0] = sub32(r[AT], r[T0]);
                r[T1] = sub32(r[V0], r[T1]);
                r[AT] = r[SP] & 0x600;
                r[V0] = sx(0x00000600);
                let t = r[AT] != r[V0];
                r[FP] = add32(r[FP], 0x00000010);
                if t {
                    l = 0x004a2e10;
                    continue;
                }
                flush(c, &mut r);
                l = 0x004a2e10;
                continue;
            }
            0x004a2f18 => {
                r[AT] = sx(0x0000007a);
                let t = r[S4] != r[S5];
                r[A3] = sub32(r[AT], r[A3]);
                if t {
                    l = 0x004a2dc8;
                    continue;
                }
                r[AT] = sll(r[S6], 2);
                r[S0] = add32(r[S0], r[AT]);
                l = 0x004a2d90;
                continue;
            }
            0x004a2f38 => {
                c.wr32((r[RA] as u32).wrapping_add(0x00003f7c), r[A3] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f80), r[T0] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f84), r[T1] as u32);
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[AT] = 0x0000000010000000;
                c.wr32(r[SP] as u32, r[AT] as u32);
                r[AT] = 0x0000000013000000;
                c.wr32((r[SP] as u32).wrapping_add(0x00000008), r[AT] as u32);
                r[SP] = add32(r[SP], 0x00000010);
                r[FP] = add32(r[FP], 0x00000010);
                flush(c, &mut r);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f94), r[FP] as u32);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[SP] = add32(r[SP], 0x00000010);
                r[AT] = sx(k.hi(c, 0x004a2fd0));
                r[AT] = add32(r[AT], k.lo(c, 0x004a2fd4) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                (r[A0], h[A0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000020));
                (r[A1], h[A1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000030));
                (r[A2], h[A2]) = c.rd128((r[AT] as u32).wrapping_add(0x00000040));
                c.wr128(r[SP] as u32, r[V0], h[V0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), r[A2], h[A2]);
                r[SP] = add32(r[SP], 0x00000050);
                r[FP] = add32(r[FP], 0x00000050);
                r[AT] = sx(k.hi(c, 0x004a3008));
                r[AT] = add32(r[AT], k.lo(c, 0x004a300c) as u64);
                lqc2(c, 28, (r[AT] as u32).wrapping_add(0x00000040));
                lqc2(c, 29, (r[AT] as u32).wrapping_add(0x00000050));
                lqc2(c, 30, (r[AT] as u32).wrapping_add(0x00000060));
                lqc2(c, 31, (r[AT] as u32).wrapping_add(0x00000070));
                r[AT] = sx(k.hi(c, 0x004a3020));
                r[AT] = add32(r[AT], k.lo(c, 0x004a3024) as u64);
                lqc2(c, 21, (r[AT] as u32).wrapping_add(0x00000230));
                lqc2(c, 22, (r[AT] as u32).wrapping_add(0x00000220));
                lqc2(c, 23, (r[AT] as u32).wrapping_add(0x000001a0));
                r[T7] = sx(c.rd32(k.gp(c, gp, 0x004a3038)));
                r[S7] = sx(0x00000080);
                r[AT] = sx(k.hi(c, 0x004a3040));
                r[AT] = add32(r[AT], k.lo(c, 0x004a3044) as u64);
                r[V0] = add32(r[AT], 0x00000080);
                r[V1] = add32(r[RA], 0x00003800);
                l = 0x004a3050;
                continue;
            }
            0x004a3050 => {
                (r[A0], h[A0]) = c.rd128(r[AT] as u32);
                (r[A1], h[A1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                r[AT] = add32(r[AT], 0x00000020);
                c.wr128(r[V1] as u32, r[A0], h[A0]);
                c.wr128((r[V1] as u32).wrapping_add(0x00000010), r[A1], h[A1]);
                let t = r[AT] != r[V0];
                r[V1] = add32(r[V1], 0x00000020);
                if t {
                    l = 0x004a3050;
                    continue;
                }
                (r[T3], h[T3]) = c.rd128(k.gp(c, gp, 0x004a3070));
                (r[T4], h[T4]) = c.rd128(k.gp(c, gp, 0x004a3074));
                (r[T5], h[T5]) = c.rd128(k.gp(c, gp, 0x004a3078));
                (r[T6], h[T6]) = c.rd128(k.gp(c, gp, 0x004a307c));
                r[T9] = sx(0x00000000);
                r[S3] = sx(k.hi(c, 0x004a3084));
                r[S3] = add32(r[S3], k.lo(c, 0x004a3088) as u64);
                r[S2] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f60)));
                r[S1] = add32(r[RA], 0x00003600);
                r[S0] = add32(r[RA], 0x00002000);
                l = 0x004a3098;
                continue;
            }
            0x004a3098 => {
                let t = r[T9] == r[S2];
                r[S6] = c.rd16(r[S1] as u32) as u64;
                if t {
                    l = 0x004a33f8;
                    continue;
                }
                r[T9] = add32(r[T9], 0x00000001);
                r[S1] = add32(r[S1], 0x00000002);
                let t = r[S6] == 0u64;
                r[S3] = add32(r[S3], 0x00000004);
                if t {
                    l = 0x004a3098;
                    continue;
                }
                r[T8] = sx(c.rd32((r[S3] as u32).wrapping_add(0xfffffffc)));
                r[AT] = sx(c.rd32((r[T8] as u32).wrapping_add(0x0000001c)));
                lqc2(c, 16, r[AT] as u32);
                r[S4] = r[AT];
                r[S5] = add32(r[AT], 0x00000040);
                l = 0x004a30d8;
                continue;
            }
            0x004a30d8 => {
                let t = r[S6] == 0u64;
                r[V0] = c.rd16(r[S0] as u32) as u64;
                if t {
                    l = 0x004a3098;
                    continue;
                }
                r[V1] = c.rd16((r[S0] as u32).wrapping_add(0x00000002)) as u64;
                r[S0] = add32(r[S0], 0x00000004);
                r[V0] = sll(r[V0], 6);
                r[V0] = add32(r[V0], r[T7]);
                r[A0] = r[V1] & r[S7];
                let t = r[A0] == 0u64;
                r[S6] = add32(r[S6], 0xffffffff);
                if t {
                    l = 0x004a30d8;
                    continue;
                }
                r[T2] = sx(c.rd32((r[V0] as u32).wrapping_add(0x0000001c)));
                r[V1] = sll(r[V1], 24);
                let t = r[S4] == 0u64;
                r[A0] = sx(c.rd32((r[V0] as u32).wrapping_add(0x0000002c)));
                if t {
                    l = 0x004a3140;
                    continue;
                }
                (r[A1], h[A1]) = c.rd128((r[S4] as u32).wrapping_add(0x00000010));
                (r[A2], h[A2]) = c.rd128((r[S4] as u32).wrapping_add(0x00000020));
                (r[A3], h[A3]) = c.rd128((r[S4] as u32).wrapping_add(0x00000030));
                r[S4] = sx(0x00000000);
                c.wr128(r[SP] as u32, r[T3], h[T3]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T4], h[T4]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A1], h[A1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A2], h[A2]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), r[A3], h[A3]);
                r[SP] = add32(r[SP], 0x00000050);
                r[FP] = add32(r[FP], 0x00000050);
                l = 0x004a3140;
                continue;
            }
            0x004a3140 => {
                lqc2(c, 1, (r[V0] as u32).wrapping_add(0x00000030));
                r[T2] = r[T2] | r[V1];
                (r[A0], h[A0]) = mmi::pextlh((0u64, 0u64), (r[A0], h[A0]));
                c.wr128(r[SP] as u32, r[T5], h[T5]);
                (r[T2], h[T2]) = mmi::pextlb((0u64, 0u64), (r[T2], h[T2]));
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T6], h[T6]);
                (r[T2], h[T2]) = mmi::pextlh((0u64, 0u64), (r[T2], h[T2]));
                qmtc2(c, 3, (r[A0], h[A0]));
                fm::<1, 14>(c, 1, 1, c.vf[24]);
                r[SP] = add32(r[SP], 0x00000020);
                uycpu::vu0::itof::<2, 12>(c, 3, 3);
                r[FP] = add32(r[FP], 0x00000020);
                let t = r[A0] == 0u64;
                if t {
                    l = 0x004a31d8;
                    continue;
                }
                fm::<0, 14>(c, 4, 0, b4(c.vf[0][3]));
                fm::<6, 14>(c, 2, 1, c.vf[1]);
                fm::<0, 8>(c, 32, 2, b4(c.vf[2][1]));
                fm::<2, 8>(c, 2, 4, b4(c.vf[2][2]));
                uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[2][0]);
                fm::<6, 4>(c, 15, 16, b4(c.vf[3][0]));
                fm::<6, 15>(c, 4, 0, b4(c.vf[0][0]));
                fm::<6, 15>(c, 5, 0, b4(c.vf[0][0]));
                {
                    let v = c.vf[0];
                    uycpu::vu0::mov::<15>(c, 6, [v[1], v[2], v[3], v[0]]);
                }
                uycpu::vu0::mov::<15>(c, 7, c.vf[0]);
                fm::<0, 8>(c, 4, 4, b4(c.vf[0][3]));
                r[AT] = add32(r[RA], 0x00003800);
                fm::<0, 4>(c, 5, 5, b4(c.vf[0][3]));
                r[V1] = add32(r[RA], 0x00003840);
                fm::<6, 3>(c, 15, 16, b4(c.vf[3][1]));
                fm::<6, 14>(c, 2, 1, b4(c.vi[22]));
                fm::<6, 7>(c, 15, 15, b4(c.vf[1][3]));
                fm::<0, 12>(c, 4, 0, c.vf[2]);
                fm::<0, 8>(c, 5, 0, b4(c.vf[2][1]));
                fm::<1, 4>(c, 5, 0, b4(c.vf[2][0]));
                fm::<6, 14>(c, 7, 1, b4(c.vf[24][3]));
                l = 0x004a3200;
                continue;
            }
            0x004a31d8 => {
                lqc2(c, 4, r[V0] as u32);
                lqc2(c, 5, (r[V0] as u32).wrapping_add(0x00000010));
                lqc2(c, 6, (r[V0] as u32).wrapping_add(0x00000020));
                r[AT] = r[S5];
                fm::<6, 14>(c, 4, 4, b4(c.vf[1][3]));
                fm::<6, 14>(c, 5, 5, b4(c.vf[1][3]));
                fm::<6, 14>(c, 6, 6, b4(c.vf[1][3]));
                fm::<6, 14>(c, 7, 1, b4(c.vf[24][3]));
                uycpu::vu0::mov::<15>(c, 15, c.vf[16]);
                r[V1] = add32(r[RA], 0x00003840);
                l = 0x004a3200;
                continue;
            }
            0x004a3200 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[4][0]));
                fm::<2, 15>(c, 32, 29, b4(c.vf[4][1]));
                fm::<2, 15>(c, 32, 30, b4(c.vf[4][2]));
                fm::<2, 15>(c, 10, 31, b4(c.vf[0][0]));
                fm::<6, 15>(c, 32, 28, b4(c.vf[5][0]));
                fm::<2, 15>(c, 32, 29, b4(c.vf[5][1]));
                fm::<2, 15>(c, 32, 30, b4(c.vf[5][2]));
                fm::<2, 15>(c, 11, 31, b4(c.vf[0][0]));
                fm::<6, 15>(c, 32, 28, b4(c.vf[6][0]));
                fm::<2, 15>(c, 32, 29, b4(c.vf[6][1]));
                fm::<2, 15>(c, 32, 30, b4(c.vf[6][2]));
                fm::<2, 15>(c, 12, 31, b4(c.vf[0][0]));
                fm::<6, 15>(c, 32, 28, b4(c.vf[7][0]));
                fm::<2, 15>(c, 32, 29, b4(c.vf[7][1]));
                fm::<2, 15>(c, 32, 30, b4(c.vf[7][2]));
                fm::<2, 15>(c, 13, 31, b4(c.vf[0][3]));
                lqc2(c, 1, r[AT] as u32);
                lqc2(c, 2, (r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 3, (r[AT] as u32).wrapping_add(0x00000020));
                lqc2(c, 4, (r[AT] as u32).wrapping_add(0x00000030));
                fm::<6, 6>(c, 1, 1, c.vf[15]);
                fm::<6, 6>(c, 2, 2, c.vf[15]);
                fm::<6, 6>(c, 3, 3, c.vf[15]);
                fm::<6, 6>(c, 4, 4, c.vf[15]);
                fm::<0, 2>(c, 1, 1, b4(c.vf[15][3]));
                fm::<0, 2>(c, 2, 2, b4(c.vf[15][3]));
                fm::<0, 2>(c, 3, 3, b4(c.vf[15][3]));
                fm::<0, 2>(c, 4, 4, b4(c.vf[15][3]));
                fm::<6, 15>(c, 32, 10, b4(c.vf[1][0]));
                fm::<2, 15>(c, 32, 11, b4(c.vf[1][1]));
                fm::<2, 15>(c, 32, 12, b4(c.vf[1][2]));
                fm::<2, 15>(c, 1, 13, b4(c.vf[0][3]));
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T2], h[T2]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), r[T2], h[T2]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000070), r[T2], h[T2]);
                c.wr128((r[SP] as u32).wrapping_add(0x000000a0), r[T2], h[T2]);
                uycpu::vu0::qop::<0>(c, c.vf[22][0], c.vf[1][3]);
                lqc2(c, 5, r[V1] as u32);
                fm::<0, 1>(c, 1, 1, c.vf[23]);
                fm::<6, 15>(c, 32, 10, b4(c.vf[2][0]));
                fm::<2, 15>(c, 32, 11, b4(c.vf[2][1]));
                fm::<2, 15>(c, 32, 12, b4(c.vf[2][2]));
                fm::<2, 15>(c, 2, 13, b4(c.vf[0][3]));
                fm::<6, 14>(c, 1, 1, b4(c.vi[22]));
                fm::<6, 14>(c, 5, 5, b4(c.vi[22]));
                uycpu::vu0::qop::<0>(c, c.vf[22][0], c.vf[2][3]);
                lqc2(c, 6, (r[V1] as u32).wrapping_add(0x00000010));
                fm::<0, 1>(c, 2, 2, c.vf[23]);
                fm::<6, 15>(c, 32, 10, b4(c.vf[3][0]));
                fm::<2, 15>(c, 32, 11, b4(c.vf[3][1]));
                fm::<2, 15>(c, 32, 12, b4(c.vf[3][2]));
                fm::<2, 15>(c, 3, 13, b4(c.vf[0][3]));
                fm::<6, 14>(c, 2, 2, b4(c.vi[22]));
                fm::<6, 14>(c, 6, 6, b4(c.vi[22]));
                fm::<4, 1>(c, 1, 1, c.vf[21]);
                uycpu::vu0::qop::<0>(c, c.vf[22][0], c.vf[3][3]);
                lqc2(c, 7, (r[V1] as u32).wrapping_add(0x00000020));
                fm::<0, 1>(c, 3, 3, c.vf[23]);
                fm::<6, 15>(c, 32, 10, b4(c.vf[4][0]));
                fm::<2, 15>(c, 32, 11, b4(c.vf[4][1]));
                fm::<2, 15>(c, 32, 12, b4(c.vf[4][2]));
                fm::<2, 15>(c, 4, 13, b4(c.vf[0][3]));
                fm::<6, 14>(c, 3, 3, b4(c.vi[22]));
                fm::<6, 14>(c, 7, 7, b4(c.vi[22]));
                fm::<4, 1>(c, 2, 2, c.vf[21]);
                uycpu::vu0::qop::<0>(c, c.vf[22][0], c.vf[4][3]);
                lqc2(c, 8, (r[V1] as u32).wrapping_add(0x00000030));
                fm::<0, 1>(c, 4, 4, c.vf[23]);
                sqc2(c, 5, r[SP] as u32);
                sqc2(c, 6, (r[SP] as u32).wrapping_add(0x00000030));
                fm::<5, 1>(c, 1, 1, b4(c.vf[21][2]));
                fm::<5, 1>(c, 2, 2, b4(c.vf[21][2]));
                fm::<4, 1>(c, 3, 3, c.vf[21]);
                fm::<6, 14>(c, 4, 4, b4(c.vi[22]));
                fm::<6, 14>(c, 8, 8, b4(c.vi[22]));
                sqc2(c, 7, (r[SP] as u32).wrapping_add(0x00000060));
                fm::<5, 1>(c, 3, 3, b4(c.vf[21][2]));
                fm::<4, 1>(c, 4, 4, c.vf[21]);
                sqc2(c, 8, (r[SP] as u32).wrapping_add(0x00000090));
                fm::<5, 1>(c, 4, 4, b4(c.vf[21][2]));
                fm::<0, 14>(c, 1, 1, c.vf[23]);
                fm::<0, 14>(c, 2, 2, c.vf[23]);
                fm::<0, 14>(c, 3, 3, c.vf[23]);
                fm::<0, 14>(c, 4, 4, c.vf[23]);
                uycpu::vu0::ftoi::<1, 15>(c, 1, 1);
                uycpu::vu0::ftoi::<1, 15>(c, 2, 2);
                uycpu::vu0::ftoi::<1, 15>(c, 3, 3);
                uycpu::vu0::ftoi::<1, 15>(c, 4, 4);
                sqc2(c, 1, (r[SP] as u32).wrapping_add(0x00000020));
                sqc2(c, 2, (r[SP] as u32).wrapping_add(0x00000050));
                sqc2(c, 3, (r[SP] as u32).wrapping_add(0x00000080));
                sqc2(c, 4, (r[SP] as u32).wrapping_add(0x000000b0));
                r[SP] = add32(r[SP], 0x000000c0);
                r[FP] = add32(r[FP], 0x000000c0);
                r[AT] = r[SP] & 0x600;
                r[V0] = sx(0x00000600);
                let t = r[AT] != r[V0];
                if t {
                    l = 0x004a30d8;
                    continue;
                }
                flush(c, &mut r);
                l = 0x004a30d8;
                continue;
            }
            0x004a33f8 => {
                r[AT] = 0u64 | 0x7f;
                let t = r[AT] == r[S7];
                if t {
                    l = 0x004a3450;
                    continue;
                }
                r[T9] = sx(0x00000000);
                r[S3] = sx(k.hi(c, 0x004a340c));
                r[S3] = add32(r[S3], k.lo(c, 0x004a3410) as u64);
                r[S1] = add32(r[RA], 0x00003600);
                r[S0] = add32(r[RA], 0x00002000);
                r[S7] = 0u64 | 0x7f;
                r[AT] = sx(k.hi(c, 0x004a3420));
                r[AT] = add32(r[AT], k.lo(c, 0x004a3424) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                (r[A0], h[A0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000020));
                c.wr128(r[SP] as u32, r[V0], h[V0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
                r[SP] = add32(r[SP], 0x00000030);
                r[FP] = add32(r[FP], 0x00000030);
                l = 0x004a3098;
                continue;
            }
            0x004a3450 => {
                c.wr32((r[RA] as u32).wrapping_add(0x00003f98), r[FP] as u32);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[SP] = add32(r[SP], 0x00000010);
                flush_some(c, &mut r);
                l = 0x004a34c0;
                continue;
            }
            0x004a34c0 => {
                r[AT] = sx(k.hi(c, 0x004a34c0));
                r[AT] = add32(r[AT], k.lo(c, 0x004a34c4) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                (r[A0], h[A0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000020));
                (r[A1], h[A1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000030));
                c.wr128(r[SP] as u32, r[V0], h[V0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
                r[SP] = add32(r[SP], 0x00000040);
                r[FP] = add32(r[FP], 0x00000040);
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f74)));
                r[V0] = add32(r[RA], 0x00003d00);
                let t = r[AT] == r[V0];
                if t {
                    l = 0x004a3958;
                    continue;
                }
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[V0] = sx(k.hi(c, 0x004a3504));
                r[V0] = add32(r[V0], k.lo(c, 0x004a3508) as u64);
                r[AT] = sx(k.hi(c, 0x004a350c));
                r[AT] = add32(r[AT], k.lo(c, 0x004a3510) as u64);
                r[V0] = sx(c.rd32(r[V0] as u32));
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
                r[AT] = 0x0000000030000000;
                c.wr32(r[SP] as u32, r[AT] as u32);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16(r[SP] as u32, r[V0] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                r[AT] = sx(k.hi(c, 0x004a3530));
                r[AT] = add32(r[AT], k.lo(c, 0x004a3534) as u64);
                (r[V0], h[V0]) = c.rd128(r[AT] as u32);
                (r[V1], h[V1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000010));
                (r[A0], h[A0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000020));
                (r[A1], h[A1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000030));
                (r[A2], h[A2]) = c.rd128((r[AT] as u32).wrapping_add(0x00000040));
                (r[A3], h[A3]) = c.rd128((r[AT] as u32).wrapping_add(0x00000050));
                (r[T0], h[T0]) = c.rd128((r[AT] as u32).wrapping_add(0x00000060));
                (r[T1], h[T1]) = c.rd128((r[AT] as u32).wrapping_add(0x00000070));
                c.wr128(r[SP] as u32, r[V0], h[V0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), r[A2], h[A2]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000050), r[A3], h[A3]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000060), r[T0], h[T0]);
                c.wr128((r[SP] as u32).wrapping_add(0x00000070), r[T1], h[T1]);
                r[SP] = add32(r[SP], 0x00000080);
                r[FP] = add32(r[FP], 0x00000080);
                r[AT] = sx(k.hi(c, 0x004a3580));
                r[AT] = add32(r[AT], k.lo(c, 0x004a3584) as u64);
                lqc2(c, 28, (r[AT] as u32).wrapping_add(0x000000c0));
                lqc2(c, 29, (r[AT] as u32).wrapping_add(0x000000d0));
                lqc2(c, 30, (r[AT] as u32).wrapping_add(0x000000e0));
                lqc2(c, 31, (r[AT] as u32).wrapping_add(0x000000f0));
                r[V0] = sx(k.hi(c, 0x004a3598));
                r[V0] = add32(r[V0], k.lo(c, 0x004a359c) as u64);
                c.wr32((r[RA] as u32).wrapping_add(0x00003fa0), r[V0] as u32);
                r[AT] = sx(0x00000002);
                r[V0] = sx(0x000000ee);
                r[V1] = sx(0x00000011);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f7c), r[AT] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f80), r[V0] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f84), r[V1] as u32);
                (r[T2], h[T2]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f00));
                (r[T3], h[T3]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f10));
                (r[T4], h[T4]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f20));
                (r[T5], h[T5]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f30));
                (r[T6], h[T6]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f40));
                (r[T7], h[T7]) = c.rd128((r[RA] as u32).wrapping_add(0x00003f50));
                r[T9] = sx(0x00000000);
                r[S3] = sx(k.hi(c, 0x004a35dc));
                r[S3] = add32(r[S3], k.lo(c, 0x004a35e0) as u64);
                r[S2] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f60)));
                r[S1] = add32(r[RA], 0x00003c80);
                r[S0] = add32(r[RA], 0x00003d00);
                r[AT] = sx(c.rd32(k.gp(c, gp, 0x004a35f0)));
                qmtc2(c, 16, (r[AT], h[AT]));
                l = 0x004a35f8;
                continue;
            }
            0x004a35f8 => {
                let t = r[T9] == r[S2];
                r[S6] = c.rd16(r[S1] as u32) as u64;
                if t {
                    l = 0x004a3938;
                    continue;
                }
                r[T9] = add32(r[T9], 0x00000001);
                r[S1] = add32(r[S1], 0x00000002);
                let t = r[S6] == 0u64;
                r[S3] = add32(r[S3], 0x00000004);
                if t {
                    l = 0x004a35f8;
                    continue;
                }
                r[A0] = sx(c.rd32((r[S3] as u32).wrapping_add(0xfffffffc)));
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[AT] = 0x0000000020000000;
                c.wr32(r[SP] as u32, r[AT] as u32);
                r[AT] = sll(r[S6], 6);
                r[V0] = sll(r[S6], 4);
                r[FP] = add32(r[FP], 0x00000010);
                r[AT] = add32(r[AT], r[V0]);
                r[AT] = add32(r[AT], r[FP]);
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
                r[SP] = add32(r[SP], 0x00000010);
                r[S4] = add32(r[A0], 0x00000040);
                r[S5] = c.rd16((r[A0] as u32).wrapping_add(0x00000028)) as u64;
                r[AT] = c.rd16((r[A0] as u32).wrapping_add(0x00000014)) as u64;
                r[S5] = sll(r[S5], 3);
                r[S5] = add32(r[S4], r[S5]);
                r[AT] = r[AT] & 0x6;
                r[T8] = add32(r[S0], 0x00000000);
                r[AT] = srl(r[AT], 1);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f9c), r[AT] as u32);
                l = 0x004a3678;
                continue;
            }
            0x004a3678 => {
                let t = r[S6] == 0u64;
                r[AT] = sx(c.rd32(r[S0] as u32));
                if t {
                    l = 0x004a37b0;
                    continue;
                }
                r[S0] = add32(r[S0], 0x00000004);
                r[S6] = add32(r[S6], 0xffffffff);
                r[V1] = c.rd16((r[AT] as u32).wrapping_add(0x00000018)) as u64;
                (r[A0], h[A0]) = q4(c.vf[16]);
                r[V0] = sll(r[V1], 6);
                r[V0] = add32(r[V0], r[A0]);
                (r[A0], h[A0]) = mmi::pcpyud((r[T9], h[T9]), (0u64, 0u64));
                lqc2(c, 4, (r[V0] as u32).wrapping_add(0x00000030));
                r[A0] = sub32(r[FP], r[A0]);
                lqc2(c, 1, r[V0] as u32);
                r[V1] = sll(r[V1], 19);
                lqc2(c, 2, (r[V0] as u32).wrapping_add(0x00000010));
                r[V1] = r[V1] | r[A0];
                lqc2(c, 3, (r[V0] as u32).wrapping_add(0x00000020));
                c.wr32((r[S0] as u32).wrapping_add(0xfffffffc), r[V1] as u32);
                fm::<1, 14>(c, 4, 4, c.vf[24]);
                fm::<6, 14>(c, 1, 1, b4(c.vf[4][3]));
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f9c)));
                fm::<6, 14>(c, 2, 2, b4(c.vf[4][3]));
                r[A3] = add32(r[V0], 0x00000000);
                let t = r[AT] != 0u64;
                fm::<6, 14>(c, 3, 3, b4(c.vf[4][3]));
                if t {
                    l = 0x004a2930;
                    continue;
                }
                l = 0x004a36e8;
                continue;
            }
            0x004a36e8 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[1][0]));
                l = 0x004a36ec;
                continue;
            }
            0x004a36ec => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[1][1]));
                l = 0x004a36f0;
                continue;
            }
            0x004a36f0 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[1][2]));
                l = 0x004a36f4;
                continue;
            }
            0x004a36f4 => {
                fm::<2, 15>(c, 1, 31, b4(c.vf[0][0]));
                l = 0x004a36f8;
                continue;
            }
            0x004a36f8 => {
                fm::<6, 14>(c, 4, 4, b4(c.vf[24][3]));
                l = 0x004a36fc;
                continue;
            }
            0x004a36fc => {
                l = 0x004a3700;
                continue;
            }
            0x004a3700 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[2][0]));
                l = 0x004a3704;
                continue;
            }
            0x004a3704 => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[2][1]));
                l = 0x004a3708;
                continue;
            }
            0x004a3708 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[2][2]));
                l = 0x004a370c;
                continue;
            }
            0x004a370c => {
                fm::<2, 15>(c, 2, 31, b4(c.vf[0][0]));
                l = 0x004a3710;
                continue;
            }
            0x004a3710 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[3][0]));
                l = 0x004a3714;
                continue;
            }
            0x004a3714 => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[3][1]));
                l = 0x004a3718;
                continue;
            }
            0x004a3718 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[3][2]));
                l = 0x004a371c;
                continue;
            }
            0x004a371c => {
                fm::<2, 15>(c, 3, 31, b4(c.vf[0][0]));
                l = 0x004a3720;
                continue;
            }
            0x004a3720 => {
                fm::<6, 15>(c, 32, 28, b4(c.vf[4][0]));
                l = 0x004a3724;
                continue;
            }
            0x004a3724 => {
                fm::<2, 15>(c, 32, 29, b4(c.vf[4][1]));
                l = 0x004a3728;
                continue;
            }
            0x004a3728 => {
                fm::<2, 15>(c, 32, 30, b4(c.vf[4][2]));
                l = 0x004a372c;
                continue;
            }
            0x004a372c => {
                fm::<2, 15>(c, 4, 31, b4(c.vf[0][3]));
                l = 0x004a3730;
                continue;
            }
            0x004a3730 => {
                sqc2(c, 1, r[SP] as u32);
                l = 0x004a3734;
                continue;
            }
            0x004a3734 => {
                sqc2(c, 2, (r[SP] as u32).wrapping_add(0x00000010));
                l = 0x004a3738;
                continue;
            }
            0x004a3738 => {
                sqc2(c, 3, (r[SP] as u32).wrapping_add(0x00000020));
                l = 0x004a373c;
                continue;
            }
            0x004a373c => {
                sqc2(c, 4, (r[SP] as u32).wrapping_add(0x00000030));
                l = 0x004a3740;
                continue;
            }
            0x004a3740 => {
                c.wr128((r[SP] as u32).wrapping_add(0x00000040), 0u64, 0u64);
                l = 0x004a3744;
                continue;
            }
            0x004a3744 => {
                r[SP] = add32(r[SP], 0x00000050);
                l = 0x004a3748;
                continue;
            }
            0x004a3748 => {
                r[AT] = r[SP] & 0x600;
                l = 0x004a374c;
                continue;
            }
            0x004a374c => {
                r[V0] = sx(0x00000600);
                l = 0x004a3750;
                continue;
            }
            0x004a3750 => {
                let t = r[AT] != r[V0];
                r[FP] = add32(r[FP], 0x00000050);
                if t {
                    l = 0x004a3678;
                    continue;
                }
                flush(c, &mut r);
                l = 0x004a3678;
                continue;
            }
            0x004a37b0 => {
                r[S6] = c.rd16((r[S1] as u32).wrapping_add(0xfffffffe)) as u64;
                r[A3] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f7c)));
                r[T0] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f80)));
                r[T1] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f84)));
                l = 0x004a37c8;
                continue;
            }
            0x004a37c8 => {
                r[AT] = sx(c.rd32(r[S4] as u32));
                r[V0] = sx(c.rd32((r[S4] as u32).wrapping_add(0x00000004)));
                r[S4] = add32(r[S4], 0x00000008);
                c.wr128(r[SP] as u32, r[T2], h[T2]);
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
                r[V0] = srl(r[V0], 4);
                c.wr16(r[SP] as u32, r[V0] as u16);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16((r[SP] as u32).wrapping_add(0x00000008), r[A3] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                c.wr128(r[SP] as u32, r[T3], h[T3]);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr16((r[SP] as u32).wrapping_add(0x0000000c), r[T1] as u16);
                r[SP] = add32(r[SP], 0x00000010);
                r[S7] = add32(r[S6], 0x00000000);
                r[A2] = add32(r[T8], 0x00000000);
                l = 0x004a3810;
                continue;
            }
            0x004a3810 => {
                r[AT] = sx(0x00000005);
                r[A1] = add32(r[T0], 0x00000001);
                let t = r[S7] == 0u64;
                (r[A0], h[A0]) = mmi::pminw((r[S7], h[S7]), (r[AT], h[AT]));
                if t {
                    l = 0x004a3918;
                    continue;
                }
                c.wr128(r[SP] as u32, r[T4], h[T4]);
                r[S7] = sub32(r[S7], r[A0]);
                c.wr16((r[SP] as u32).wrapping_add(0x00000008), r[T0] as u16);
                r[FP] = add32(r[FP], 0x00000010);
                c.wr32((r[SP] as u32).wrapping_add(0x0000000c), r[A0] as u32);
                r[SP] = add32(r[SP], 0x00000010);
                l = 0x004a3838;
                continue;
            }
            0x004a3838 => {
                r[AT] = sx(c.rd32(r[A2] as u32));
                r[A2] = add32(r[A2], 0x00000004);
                c.wr128(r[SP] as u32, r[T5], h[T5]);
                (r[V1], h[V1]) = mmi::pcpyud((r[T9], h[T9]), (0u64, 0u64));
                r[A0] = add32(r[A0], 0xffffffff);
                r[V0] = sll(r[AT], 13);
                r[AT] = srl(r[AT], 19);
                r[V0] = srl(r[V0], 13);
                c.wr16((r[SP] as u32).wrapping_add(0x0000000c), r[A1] as u16);
                r[V0] = add32(r[V0], r[V1]);
                c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[V0] as u32);
                r[V1] = add32(r[A1], 0x00000004);
                r[V0] = r[AT] << 38;
                r[AT] = r[AT] << 37;
                r[AT] = r[AT].wrapping_add(r[V0]);
                r[A1] = add32(r[A1], 0x0000001c);
                c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T7], h[T7]);
                r[AT] = r[AT].wrapping_add(r[T7]);
                r[V1] = r[V1] | 0x4000;
                c.wr64((r[SP] as u32).wrapping_add(0x00000010), r[AT]);
                c.wr16((r[SP] as u32).wrapping_add(0x0000001c), r[V1] as u16);
                r[FP] = add32(r[FP], 0x00000020);
                let t = r[A0] != 0u64;
                r[SP] = add32(r[SP], 0x00000020);
                if t {
                    l = 0x004a3838;
                    continue;
                }
                c.wr128(r[SP] as u32, r[T6], h[T6]);
                r[SP] = add32(r[SP], 0x00000010);
                r[AT] = sx(0x00000269);
                r[V0] = sx(0x00000026);
                r[T0] = sub32(r[AT], r[T0]);
                r[T1] = sub32(r[V0], r[T1]);
                r[AT] = r[SP] & 0x600;
                r[V0] = sx(0x00000600);
                let t = r[AT] != r[V0];
                r[FP] = add32(r[FP], 0x00000010);
                if t {
                    l = 0x004a3810;
                    continue;
                }
                flush(c, &mut r);
                l = 0x004a3810;
                continue;
            }
            0x004a3918 => {
                r[AT] = sx(0x0000007a);
                let t = r[S4] != r[S5];
                r[A3] = sub32(r[AT], r[A3]);
                if t {
                    l = 0x004a37c8;
                    continue;
                }
                c.wr32((r[RA] as u32).wrapping_add(0x00003f7c), r[A3] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f80), r[T0] as u32);
                c.wr32((r[RA] as u32).wrapping_add(0x00003f84), r[T1] as u32);
                l = 0x004a35f8;
                continue;
            }
            0x004a3938 => {
                c.wr128(r[SP] as u32, 0u64, 0u64);
                r[AT] = 0x0000000010000000;
                c.wr32(r[SP] as u32, r[AT] as u32);
                r[AT] = 0x0000000013000000;
                c.wr32((r[SP] as u32).wrapping_add(0x00000008), r[AT] as u32);
                r[SP] = add32(r[SP], 0x00000010);
                r[FP] = add32(r[FP], 0x00000010);
                l = 0x004a3958;
                continue;
            }
            0x004a3958 => {
                last_flush(c, &mut r);
                l = 0x004a39f8;
                continue;
            }
            0x004a39f8 => {
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f8c)));
                r[V0] = sx(0xffffffff);
                c.wr16(r[AT] as u32, r[V0] as u16);
                r[V0] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f60)));
                r[AT] = add32(r[RA], 0x00003000);
                r[V0] = sll(r[V0], 2);
                r[V0] = add32(r[AT], r[V0]);
                l = 0x004a3a18;
                continue;
            }
            0x004a3a18 => {
                let t = r[AT] == r[V0];
                r[V1] = sx(c.rd32(r[AT] as u32));
                if t {
                    l = 0x004a3a60;
                    continue;
                }
                r[AT] = add32(r[AT], 0x00000004);
                let t = r[V1] == 0u64;
                r[A0] = srl(r[V1], 16);
                if t {
                    l = 0x004a3a18;
                    continue;
                }
                r[A0] = r[A0] & 0xfff0;
                r[V1] = sll(r[V1], 12);
                r[V1] = srl(r[V1], 8);
                r[A1] = 0x0000000020000000;
                c.wr128((r[V1] as u32).wrapping_add(0xfffffff0), 0u64, 0u64);
                r[A0] = add32(r[V1], r[A0]);
                c.wr32((r[V1] as u32).wrapping_add(0xfffffff0), r[A1] as u32);
                c.wr32((r[V1] as u32).wrapping_add(0xfffffff4), r[A0] as u32);
                l = 0x004a3a18;
                continue;
            }
            0x004a3a60 => {
                r[AT] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f90)));
                r[V0] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f94)));
                r[V1] = sx(c.rd32((r[RA] as u32).wrapping_add(0x00003f98)));
                r[A0] = 0x0000000020000000;
                c.wr32(r[AT] as u32, r[A0] as u32);
                c.wr32(r[V0] as u32, r[A0] as u32);
                c.wr32(r[V1] as u32, r[A0] as u32);
                r[A0] = add32(r[V0], 0x00000010);
                c.wr32((r[AT] as u32).wrapping_add(0x00000004), r[A0] as u32);
                r[A0] = add32(r[V1], 0x00000010);
                c.wr32((r[V0] as u32).wrapping_add(0x00000004), r[A0] as u32);
                r[A0] = add32(r[AT], 0x00000010);
                c.wr32((r[V1] as u32).wrapping_add(0x00000004), r[A0] as u32);
                c.wr32(k.gp(c, gp, 0x004a3a98), r[FP] as u32);
                r[AT] = 0x0000000070000000;
                r[RA] = c.rd64((r[AT] as u32).wrapping_add(0x00003ff8));
                r[FP] = c.rd64((r[AT] as u32).wrapping_add(0x00003ff0));
                r[SP] = c.rd64((r[AT] as u32).wrapping_add(0x00003fe8));
                r[S7] = c.rd64((r[AT] as u32).wrapping_add(0x00003fe0));
                r[S6] = c.rd64((r[AT] as u32).wrapping_add(0x00003fd8));
                r[S5] = c.rd64((r[AT] as u32).wrapping_add(0x00003fd0));
                r[S4] = c.rd64((r[AT] as u32).wrapping_add(0x00003fc8));
                r[S3] = c.rd64((r[AT] as u32).wrapping_add(0x00003fc0));
                r[S2] = c.rd64((r[AT] as u32).wrapping_add(0x00003fb8));
                r[S1] = c.rd64((r[AT] as u32).wrapping_add(0x00003fb0));
                r[S0] = c.rd64((r[AT] as u32).wrapping_add(0x00003fa8));
                break;
            }
            _ => unreachable!("0x4A2200: no block at {l:08x}"),
        }
    }
    c.r = r;
    c.rh = h;
}

// ---- 0x4A3AD8: the light patches' packets ------------------------------------------------------------------------------------
//
// Hand-written, a leaf on t registers: the 0x100 flag bytes at scratchpad 0x3600 cleared; for each of the gp+0x4590 entries
// of the cull's words at scratchpad 0x3100 (0x7FFFFFFF: none) against the words at 0x239E70, its index into 0x239F70 and 1
// or 3 or'ed into the flag bytes its byte list at 0x23A470 names (0xFF its end); then for each of the gp+0x45AC flag bytes
// that is set, with its word at 0x239670 and its slot at 0x239A70, one packet of 0x70 bytes at the chain pointer (gp+0x4838)
// from the five templates at gp-0x2820, two when the flag has bit 1; v0 the packets' quadwords times 256.

fn f4a3ad8(c: &mut Cpu, k: F) {
    let (mut r, mut h) = (c.r, c.rh);
    let gp = r[GP] as u32;
    let mut l: u32 = 0x004a3ad8;
    loop {
        match l {
            0x004a3ad8 => {
                r[AT] = 0x0000000070000000;
                r[T6] = sx(k.hi(c, 0x004a3adc));
                r[T6] = add32(r[T6], k.lo(c, 0x004a3ae0) as u64);
                r[T7] = add32(r[AT], 0x00003100);
                r[T5] = add32(r[AT], 0x00003600);
                r[T4] = sx(k.hi(c, 0x004a3aec));
                r[T4] = add32(r[T4], k.lo(c, 0x004a3af0) as u64);
                r[T2] = sx(c.rd32(k.gp(c, gp, 0x004a3af8)));
                r[T3] = sx(k.hi(c, 0x004a3afc));
                r[T3] = add32(r[T3], k.lo(c, 0x004a3b00) as u64);
                r[T1] = sx(0x00000000);
                r[T8] = sx(c.rd32(k.gp(c, gp, 0x004a3b08)));
                r[T9] = srl(r[A0], 8);
                r[T0] = 0x000000007fff0000;
                r[T0] = r[T0] | 0xffff;
                r[AT] = add32(r[T5], 0x00000000);
                r[V0] = add32(r[T5], 0x00000100);
                l = 0x004a3b20;
                continue;
            }
            0x004a3b20 => {
                c.wr128(r[AT] as u32, 0u64, 0u64);
                r[AT] = add32(r[AT], 0x00000020);
                let t = r[AT] != r[V0];
                c.wr128((r[AT] as u32).wrapping_add(0xfffffff0), 0u64, 0u64);
                if t {
                    l = 0x004a3b20;
                    continue;
                }
                l = 0x004a3b38;
                continue;
            }
            0x004a3b38 => {
                let t = r[T1] == r[T2];
                r[T1] = add32(r[T1], 0x00000001);
                if t {
                    l = 0x004a3bb0;
                    continue;
                }
                r[AT] = sx(c.rd32(r[T7] as u32));
                r[T7] = add32(r[T7], 0x00000004);
                r[V0] = sx(c.rd32(r[T6] as u32));
                r[T6] = add32(r[T6], 0x00000004);
                let t = r[AT] == r[T0];
                r[T3] = add32(r[T3], 0x00000010);
                if t {
                    l = 0x004a3b38;
                    continue;
                }
                r[V1] = sll(r[V0], 1);
                r[A0] = add32(r[T1], 0xffffffff);
                r[V1] = sub32(r[V1], r[AT]);
                c.wr32(r[T4] as u32, r[A0] as u32);
                let t = (r[V1] as i64) <= 0;
                r[V1] = sub32(r[V0], r[AT]);
                if t {
                    l = 0x004a3b38;
                    continue;
                }
                let t = (r[V1] as i64) <= 0;
                r[V1] = sx(0x00000001);
                if t {
                    l = 0x004a3b80;
                    continue;
                }
                r[V1] = sx(0x00000003);
                l = 0x004a3b80;
                continue;
            }
            0x004a3b80 => {
                r[T4] = add32(r[T4], 0x00000004);
                r[AT] = add32(r[T3], 0xfffffff0);
                r[V0] = sx(0x000000ff);
                l = 0x004a3b90;
                continue;
            }
            0x004a3b90 => {
                r[A0] = c.rd8(r[AT] as u32) as u64;
                r[AT] = add32(r[AT], 0x00000001);
                let t = r[A0] == r[V0];
                r[A0] = add32(r[A0], r[T5]);
                if t {
                    l = 0x004a3b38;
                    continue;
                }
                r[A1] = c.rd8(r[A0] as u32) as u64;
                r[A1] = r[A1] | r[V1];
                c.wr8(r[A0] as u32, r[A1] as u8);
                l = 0x004a3b90;
                continue;
            }
            0x004a3bb0 => {
                r[AT] = sx(0xffffffff);
                c.wr32(r[T4] as u32, r[AT] as u32);
                (r[T3], h[T3]) = c.rd128(k.gp(c, gp, 0x004a3bb8));
                (r[T4], h[T4]) = c.rd128(k.gp(c, gp, 0x004a3bbc));
                (r[T5], h[T5]) = c.rd128(k.gp(c, gp, 0x004a3bc0));
                (r[T6], h[T6]) = c.rd128(k.gp(c, gp, 0x004a3bc4));
                (r[T7], h[T7]) = c.rd128(k.gp(c, gp, 0x004a3bc8));
                r[T2] = sx(k.hi(c, 0x004a3bcc));
                r[T2] = add32(r[T2], k.lo(c, 0x004a3bd0) as u64);
                r[A3] = sx(k.hi(c, 0x004a3bd4));
                r[A3] = add32(r[A3], k.lo(c, 0x004a3bd8) as u64);
                r[T0] = 0x0000000070000000;
                r[T1] = sx(c.rd32(k.gp(c, gp, 0x004a3be4)));
                r[T0] = add32(r[T0], 0x00003600);
                r[T1] = add32(r[T1], r[T0]);
                l = 0x004a3bf0;
                continue;
            }
            0x004a3bf0 => {
                let t = r[T0] == r[T1];
                r[AT] = c.rd8(r[T0] as u32) as u64;
                if t {
                    l = 0x004a3d70;
                    continue;
                }
                r[T0] = add32(r[T0], 0x00000001);
                r[A0] = sx(c.rd32(r[A3] as u32));
                r[A3] = add32(r[A3], 0x00000004);
                c.wr32(r[T2] as u32, 0u64 as u32);
                let t = r[AT] == 0u64;
                r[T2] = add32(r[T2], 0x00000004);
                if t {
                    l = 0x004a3bf0;
                    continue;
                }
                r[V0] = r[AT] & 0x2;
                r[V1] = srl(r[A0], 28);
                r[A0] = sll(r[A0], 4);
                let t = r[V0] == 0u64;
                r[A0] = srl(r[A0], 4);
                if t {
                    l = 0x004a3cc0;
                    continue;
                }
                c.wr128(r[T8] as u32, r[T3], h[T3]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000010), r[T4], h[T4]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000020), r[T5], h[T5]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000030), r[T6], h[T6]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000040), 0u64, 0u64);
                r[AT] = sx(0x00000053);
                c.wr128((r[T8] as u32).wrapping_add(0x00000050), r[T7], h[T7]);
                c.wr32((r[T8] as u32).wrapping_add(0x00000048), r[AT] as u32);
                c.wr16((r[T8] as u32).wrapping_add(0x00000024), r[T9] as u16);
                r[V0] = add32(r[V1], 0xfffffffa);
                let t = (r[V0] as i64) <= 0;
                r[AT] = sx(0x00000001);
                if t {
                    l = 0x004a3c60;
                    continue;
                }
                r[V0] = sx((r[AT] as u32).wrapping_shl(r[V0] as u32 & 31));
                c.wr8((r[T8] as u32).wrapping_add(0x00000026), r[V0] as u8);
                l = 0x004a3c60;
                continue;
            }
            0x004a3c60 => {
                r[AT] = sx((r[AT] as u32).wrapping_shl(r[V1] as u32 & 31));
                r[V0] = sx(0x00000001);
                c.wr32((r[T8] as u32).wrapping_add(0x00000030), r[AT] as u32);
                r[V0] = sx((r[V0] as u32).wrapping_shl(r[V1] as u32 & 31));
                c.wr32((r[T8] as u32).wrapping_add(0x00000034), r[AT] as u32);
                r[V0] = sx((r[V0] as u32).wrapping_shl(r[V1] as u32 & 31));
                r[AT] = 0u64 | 0x8000;
                r[V0] = srl(r[V0], 4);
                r[AT] = add32(r[AT], r[V0]);
                c.wr32((r[T8] as u32).wrapping_add(0x00000050), r[AT] as u32);
                r[AT] = 0x0000000030000000;
                c.wr128((r[T8] as u32).wrapping_add(0x00000060), 0u64, 0u64);
                r[AT] = add32(r[AT], r[V0]);
                c.wr32((r[T8] as u32).wrapping_add(0x00000064), r[A0] as u32);
                c.wr32((r[T8] as u32).wrapping_add(0x00000060), r[AT] as u32);
                r[AT] = 0x0000000050000000;
                r[AT] = add32(r[AT], r[V0]);
                r[V0] = srl(r[V0], 4);
                c.wr16((r[T2] as u32).wrapping_add(0xfffffffc), r[T9] as u16);
                r[T9] = add32(r[T9], r[V0]);
                c.wr32((r[T8] as u32).wrapping_add(0x0000006c), r[AT] as u32);
                r[V0] = add32(r[T9], 0xffffc005);
                let t = (r[V0] as i64) > 0;
                r[T8] = add32(r[T8], 0x00000070);
                if t {
                    l = 0x004a3d68;
                    continue;
                }
                l = 0x004a3cc0;
                continue;
            }
            0x004a3cc0 => {
                c.wr128(r[T8] as u32, r[T3], h[T3]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000010), r[T4], h[T4]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000020), r[T5], h[T5]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000030), r[T6], h[T6]);
                c.wr128((r[T8] as u32).wrapping_add(0x00000040), 0u64, 0u64);
                r[AT] = sx(0x00000053);
                c.wr128((r[T8] as u32).wrapping_add(0x00000050), r[T7], h[T7]);
                r[V1] = add32(r[V1], 0xffffffff);
                c.wr32((r[T8] as u32).wrapping_add(0x00000048), r[AT] as u32);
                c.wr16((r[T8] as u32).wrapping_add(0x00000024), r[T9] as u16);
                r[V0] = add32(r[V1], 0xfffffffa);
                let t = (r[V0] as i64) <= 0;
                r[AT] = sx(0x00000001);
                if t {
                    l = 0x004a3d00;
                    continue;
                }
                r[V0] = sx((r[AT] as u32).wrapping_shl(r[V0] as u32 & 31));
                c.wr8((r[T8] as u32).wrapping_add(0x00000026), r[V0] as u8);
                l = 0x004a3d00;
                continue;
            }
            0x004a3d00 => {
                r[AT] = sx((r[AT] as u32).wrapping_shl(r[V1] as u32 & 31));
                r[V0] = sx(0x00000001);
                c.wr32((r[T8] as u32).wrapping_add(0x00000030), r[AT] as u32);
                r[V0] = sx((r[V0] as u32).wrapping_shl(r[V1] as u32 & 31));
                c.wr32((r[T8] as u32).wrapping_add(0x00000034), r[AT] as u32);
                r[V0] = sx((r[V0] as u32).wrapping_shl(r[V1] as u32 & 31));
                r[AT] = sll(r[V0], 2);
                r[V0] = srl(r[V0], 4);
                r[A0] = add32(r[A0], r[AT]);
                r[AT] = 0u64 | 0x8000;
                r[AT] = add32(r[AT], r[V0]);
                c.wr32((r[T8] as u32).wrapping_add(0x00000050), r[AT] as u32);
                r[AT] = 0x0000000030000000;
                c.wr128((r[T8] as u32).wrapping_add(0x00000060), 0u64, 0u64);
                r[AT] = add32(r[AT], r[V0]);
                c.wr32((r[T8] as u32).wrapping_add(0x00000064), r[A0] as u32);
                c.wr32((r[T8] as u32).wrapping_add(0x00000060), r[AT] as u32);
                r[AT] = 0x0000000050000000;
                r[AT] = add32(r[AT], r[V0]);
                r[V0] = srl(r[V0], 4);
                c.wr16((r[T2] as u32).wrapping_add(0xfffffffe), r[T9] as u16);
                r[T9] = add32(r[T9], r[V0]);
                c.wr32((r[T8] as u32).wrapping_add(0x0000006c), r[AT] as u32);
                r[V0] = add32(r[T9], 0xffffc005);
                let t = (r[V0] as i64) <= 0;
                r[T8] = add32(r[T8], 0x00000070);
                if t {
                    l = 0x004a3bf0;
                    continue;
                }
                l = 0x004a3d68;
                continue;
            }
            0x004a3d68 => {
                r[T8] = add32(r[T8], 0xffffff90);
                l = 0x004a3d70;
                continue;
            }
            0x004a3d70 => {
                c.wr32(k.gp(c, gp, 0x004a3d70), r[T8] as u32);
                r[V0] = sll(r[T9], 8);
                break;
            }
            _ => unreachable!("0x4A3AD8: no block at {l:08x}"),
        }
    }
    c.r = r;
    c.rh = h;
}

// ---- 0x4A3D80: the shrubs' lighting --------------------------------------------------------------------------------------------
//
// Hand-written: for each shrub of the list at a0 (u16 indices, 0xFFFF its end) the light directions of its two light slots
// (0x1F17C0, 0x40 bytes each, faded by the shrub's +0x1C) and the point lights of its +0x1E nibbles (0x1F1BC0), turned into
// the shrub's frame (its matrix at gp+0x45A0's table), then its colours, four vertices at a time: the normals of its class's
// +0x2C list as s16 quadwords into vf1 to vf4 and vf5 to vf8, the shrub program run on each four (`program`), the colours'
// bytes packed from vf9 to vf12 and vf13 to vf16 into the shrub's colour table (gp+0x45A4, 0x60 bytes a shrub).

fn f4a3d80(c: &mut Cpu, k: F, n: &mut [u64; N]) {
    let (mut r, mut h) = (c.r, c.rh);
    let gp = r[GP] as u32;
    let mut l: u32 = 0x004a3d80;
    let mut resident = false;
    loop {
        match l {
            0x004a3d80 => {
                r[AT] = 0x0000000070000000;
                c.wr64((r[AT] as u32).wrapping_add(0x00003ff8), r[FP]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003ff0), r[S0]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fe8), r[S1]);
                c.wr64((r[AT] as u32).wrapping_add(0x00003fe0), r[S2]);
                r[T2] = sx(c.rd32(k.gp(c, gp, 0x004a3d94)));
                r[FP] = 0x0000000070000000;
                r[T3] = sx(c.rd32(k.gp(c, gp, 0x004a3d9c)));
                r[S0] = 0u64 | 0xffff;
                r[T4] = sx(c.rd32(k.gp(c, gp, 0x004a3da4)));
                r[T5] = sx(k.hi(c, 0x004a3da8));
                r[T5] = add32(r[T5], k.lo(c, 0x004a3dac) as u64);
                r[S2] = 0x0000000047800000;
                (r[S2], h[S2]) = mmi::pextlw((r[S2], h[S2]), (r[S2], h[S2]));
                r[FP] = r[A0];
                (r[S2], h[S2]) = mmi::pextlw((r[S2], h[S2]), (r[S2], h[S2]));
                r[T6] = sx(k.hi(c, 0x004a3dc0));
                r[T6] = add32(r[T6], k.lo(c, 0x004a3dc4) as u64);
                r[S1] = sx(k.hi(c, 0x004a3dc8));
                r[S1] = add32(r[S1], k.lo(c, 0x004a3dcc) as u64);
                r[AT] = sx(k.hi(c, 0x004a3dd0));
                r[AT] = add32(r[AT], k.lo(c, 0x004a3dd4) as u64);
                lqc2(c, 21, r[AT] as u32);
                l = 0x004a3ddc;
                continue;
            }
            0x004a3ddc => {
                r[A2] = c.rd16(r[FP] as u32) as u64;
                r[FP] = add32(r[FP], 0x00000002);
                let t = r[A2] == r[S0];
                if t {
                    l = 0x004a4314;
                    continue;
                }
                n[4] += 1;
                r[AT] = sll(r[A2], 5);
                r[A3] = add32(r[T2], r[AT]);
                r[A0] = c.rd16((r[A3] as u32).wrapping_add(0x0000001c)) as u64;
                r[AT] = r[A0] & 0xff00;
                let t = r[AT] == 0u64;
                if t {
                    l = 0x004a3ebc;
                    continue;
                }
                r[AT] = srl(r[A0], 4);
                r[AT] = r[AT] & 0xff0;
                qmtc2(c, 1, (r[AT], h[AT]));
                uycpu::vu0::itof::<2, 8>(c, 1, 1);
                r[AT] = r[A0] & 0xf;
                r[AT] = sll(r[AT], 6);
                fm::<1, 1>(c, 1, 0, b4(c.vf[1][0]));
                r[AT] = add32(r[AT], r[T6]);
                lqc2(c, 2, r[AT] as u32);
                r[V0] = srl(r[A0], 4);
                lqc2(c, 3, (r[AT] as u32).wrapping_add(0x00000010));
                r[V0] = r[V0] & 0xf;
                lqc2(c, 4, (r[AT] as u32).wrapping_add(0x00000020));
                r[V0] = sll(r[V0], 6);
                lqc2(c, 5, (r[AT] as u32).wrapping_add(0x00000030));
                r[V0] = add32(r[V0], r[T6]);
                lqc2(c, 6, r[V0] as u32);
                fm::<6, 14>(c, 2, 2, b4(c.vf[1][3]));
                lqc2(c, 7, (r[V0] as u32).wrapping_add(0x00000010));
                fm::<6, 14>(c, 3, 3, b4(c.vf[1][3]));
                lqc2(c, 8, (r[V0] as u32).wrapping_add(0x00000020));
                fm::<6, 14>(c, 4, 4, b4(c.vf[1][3]));
                lqc2(c, 9, (r[V0] as u32).wrapping_add(0x00000030));
                fm::<6, 14>(c, 5, 5, b4(c.vf[1][3]));
                fm::<6, 14>(c, 7, 7, b4(c.vf[1][0]));
                fm::<6, 14>(c, 9, 9, b4(c.vf[1][0]));
                fm::<6, 14>(c, 6, 6, b4(c.vf[1][0]));
                fm::<6, 14>(c, 8, 8, b4(c.vf[1][0]));
                fm::<0, 15>(c, 24, 3, c.vf[7]);
                fm::<0, 15>(c, 25, 5, c.vf[9]);
                fm::<0, 15>(c, 27, 2, c.vf[6]);
                fm::<0, 15>(c, 28, 4, c.vf[8]);
                fm::<6, 15>(c, 1, 24, c.vf[24]);
                fm::<0, 8>(c, 32, 1, b4(c.vf[1][1]));
                fm::<2, 8>(c, 1, 21, b4(c.vf[1][2]));
                uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[1][0]);
                fm::<6, 15>(c, 1, 25, c.vf[25]);
                fm::<0, 8>(c, 32, 1, b4(c.vf[1][1]));
                fm::<2, 8>(c, 1, 21, b4(c.vf[1][2]));
                fm::<6, 14>(c, 24, 24, b4(c.vi[22]));
                uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[1][0]);
                fm::<6, 14>(c, 25, 25, b4(c.vi[22]));
                let t = 0u64 == 0u64;
                if t {
                    l = 0x004a3edc;
                    continue;
                }
                l = 0x004a3ebc;
                continue;
            }
            0x004a3ebc => {
                r[AT] = r[A0] & 0xf;
                r[AT] = sll(r[AT], 6);
                r[AT] = add32(r[AT], r[T6]);
                lqc2(c, 27, r[AT] as u32);
                lqc2(c, 24, (r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 28, (r[AT] as u32).wrapping_add(0x00000020));
                lqc2(c, 25, (r[AT] as u32).wrapping_add(0x00000030));
                l = 0x004a3edc;
                continue;
            }
            0x004a3edc => {
                fm::<0, 8>(c, 30, 0, b4(c.vf[27][3]));
                fm::<0, 4>(c, 30, 0, b4(c.vf[28][3]));
                fm::<1, 1>(c, 27, 0, b4(c.vf[0][3]));
                fm::<1, 1>(c, 28, 0, b4(c.vf[0][3]));
                r[AT] = 0x00000000000f0000;
                r[V1] = sx(0xfffffffe);
                r[A1] = c.rd16((r[A3] as u32).wrapping_add(0x0000001e)) as u64;
                r[A1] = r[A1] | r[AT];
                lqc2(c, 10, r[A3] as u32);
                fm::<1, 15>(c, 26, 0, c.vf[0]);
                fm::<1, 15>(c, 29, 0, c.vf[0]);
                l = 0x004a3f0c;
                continue;
            }
            0x004a3f0c => {
                r[V0] = 0u64 | 0xf;
                r[AT] = r[A1] & r[V0];
                let t = r[AT] == r[V0];
                r[A1] = sx((r[A1] as u32 as i32 >> 4) as u32);
                if t {
                    l = 0x004a3f9c;
                    continue;
                }
                r[AT] = sll(r[AT], 5);
                r[AT] = add32(r[AT], r[S1]);
                lqc2(c, 2, (r[AT] as u32).wrapping_add(0x00000010));
                lqc2(c, 1, r[AT] as u32);
                uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[2][3]);
                fm::<1, 14>(c, 2, 10, c.vf[2]);
                fm::<6, 15>(c, 4, 2, c.vf[2]);
                fm::<0, 8>(c, 32, 4, b4(c.vf[4][1]));
                fm::<2, 8>(c, 4, 21, b4(c.vf[4][2]));
                fm::<6, 1>(c, 11, 0, b4(c.vi[22]));
                fm::<1, 1>(c, 0, 4, b4(c.vf[4][0]));
                r[AT] = sx(c.vi[17]);
                uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[4][0]);
                r[AT] = r[AT] & 0x10;
                let t = r[AT] != 0u64;
                if t {
                    l = 0x004a3f0c;
                    continue;
                }
                fm::<6, 1>(c, 11, 11, b4(c.vi[22]));
                fm::<6, 1>(c, 4, 0, b4(c.vi[22]));
                fm::<1, 1>(c, 11, 0, b4(c.vf[11][3]));
                uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[4][3]);
                fm::<6, 15>(c, 1, 1, b4(c.vf[11][3]));
                fm::<6, 14>(c, 32, 2, b4(c.vi[22]));
                fm::<2, 14>(c, 26, 26, b4(c.vf[0][3]));
                fm::<0, 15>(c, 29, 29, c.vf[1]);
                let t = 0u64 == 0u64;
                r[V1] = add32(r[V1], 0x00000001);
                if t {
                    l = 0x004a3f0c;
                    continue;
                }
                l = 0x004a3f9c;
                continue;
            }
            0x004a3f9c => {
                let t = (r[V1] as i64) < 0;
                fm::<6, 15>(c, 1, 26, c.vf[26]);
                if t {
                    l = 0x004a3fbc;
                    continue;
                }
                fm::<0, 8>(c, 32, 1, b4(c.vf[1][1]));
                fm::<2, 8>(c, 1, 21, b4(c.vf[1][2]));
                uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[1][0]);
                fm::<6, 14>(c, 26, 26, b4(c.vi[22]));
                l = 0x004a3fbc;
                continue;
            }
            0x004a3fbc => {
                fm::<1, 1>(c, 26, 0, b4(c.vf[0][3]));
                fm::<0, 2>(c, 30, 0, b4(c.vf[29][3]));
                fm::<1, 1>(c, 29, 0, b4(c.vf[0][3]));
                fm::<1, 1>(c, 24, 0, c.vf[0]);
                r[A1] = sll(r[A2], 6);
                fm::<1, 1>(c, 25, 0, c.vf[0]);
                r[A1] = add32(r[T3], r[A1]);
                fm::<1, 1>(c, 26, 0, c.vf[0]);
                lqc2(c, 1, r[A1] as u32);
                r[AT] = sx(c.rd32((r[A1] as u32).wrapping_add(0x0000000c)));
                lqc2(c, 2, (r[A1] as u32).wrapping_add(0x00000010));
                (r[AT], h[AT]) = mmi::pextlb((0u64, 0u64), (r[AT], h[AT]));
                lqc2(c, 3, (r[A1] as u32).wrapping_add(0x00000020));
                (r[AT], h[AT]) = mmi::pextlh((0u64, 0u64), (r[AT], h[AT]));
                (r[AT], h[AT]) = mmi::padduw((r[AT], h[AT]), (r[S2], h[S2]));
                qmtc2(c, 31, (r[AT], h[AT]));
                fm::<6, 14>(c, 4, 1, c.vf[1]);
                fm::<6, 14>(c, 5, 2, c.vf[2]);
                fm::<6, 14>(c, 6, 3, c.vf[3]);
                fm::<0, 8>(c, 32, 4, b4(c.vf[4][1]));
                fm::<2, 8>(c, 4, 21, b4(c.vf[4][2]));
                uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[4][0]);
                fm::<0, 8>(c, 32, 5, b4(c.vf[5][1]));
                fm::<2, 8>(c, 5, 21, b4(c.vf[5][2]));
                fm::<6, 14>(c, 1, 1, b4(c.vi[22]));
                uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[5][0]);
                fm::<0, 8>(c, 32, 6, b4(c.vf[6][1]));
                fm::<2, 8>(c, 6, 21, b4(c.vf[6][2]));
                fm::<6, 14>(c, 2, 2, b4(c.vi[22]));
                uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[6][0]);
                fm::<6, 14>(c, 3, 3, b4(c.vi[22]));
                fm::<1, 8>(c, 4, 0, c.vf[1]);
                fm::<1, 4>(c, 5, 0, c.vf[2]);
                fm::<1, 2>(c, 6, 0, c.vf[3]);
                fm::<1, 8>(c, 5, 0, b4(c.vf[1][1]));
                fm::<1, 8>(c, 6, 0, b4(c.vf[1][2]));
                fm::<1, 4>(c, 4, 0, b4(c.vf[2][0]));
                r[AT] = c.rd8((r[A3] as u32).wrapping_add(0x0000001b)) as i8 as i64 as u64;
                fm::<1, 4>(c, 6, 0, b4(c.vf[2][2]));
                fm::<1, 2>(c, 4, 0, b4(c.vf[3][0]));
                let t = r[AT] == 0u64;
                fm::<1, 2>(c, 5, 0, b4(c.vf[3][1]));
                if t {
                    l = 0x004a407c;
                    continue;
                }
                c.wr8((r[A3] as u32).wrapping_add(0x0000001b), 0u64 as u8);
                l = 0x004a407c;
                continue;
            }
            0x004a407c => {
                fm::<6, 14>(c, 32, 4, b4(c.vf[24][0]));
                fm::<2, 14>(c, 32, 5, b4(c.vf[24][1]));
                fm::<2, 14>(c, 1, 6, b4(c.vf[24][2]));
                fm::<6, 14>(c, 32, 4, b4(c.vf[25][0]));
                fm::<2, 14>(c, 32, 5, b4(c.vf[25][1]));
                fm::<2, 14>(c, 2, 6, b4(c.vf[25][2]));
                fm::<6, 14>(c, 32, 4, b4(c.vf[26][0]));
                fm::<2, 14>(c, 32, 5, b4(c.vf[26][1]));
                fm::<2, 14>(c, 3, 6, b4(c.vf[26][2]));
                r[AT] = c.rd8((r[A3] as u32).wrapping_add(0x0000001a)) as u64;
                fm::<0, 8>(c, 24, 0, b4(c.vf[1][0]));
                r[AT] = sll(r[AT], 2);
                fm::<0, 8>(c, 25, 0, b4(c.vf[1][1]));
                r[AT] = add32(r[T5], r[AT]);
                fm::<0, 8>(c, 26, 0, b4(c.vf[1][2]));
                r[T7] = sx(c.rd32(r[AT] as u32));
                fm::<0, 4>(c, 24, 0, b4(c.vf[2][0]));
                r[T7] = sx(c.rd32((r[T7] as u32).wrapping_add(0x0000002c)));
                fm::<0, 4>(c, 25, 0, b4(c.vf[2][1]));
                r[AT] = sll(r[A2], 6);
                fm::<0, 4>(c, 26, 0, b4(c.vf[2][2]));
                r[V0] = sll(r[A2], 5);
                fm::<0, 2>(c, 24, 0, b4(c.vf[3][0]));
                r[AT] = add32(r[AT], r[V0]);
                fm::<0, 2>(c, 25, 0, b4(c.vf[3][1]));
                r[T8] = add32(r[T4], r[AT]);
                fm::<0, 2>(c, 26, 0, b4(c.vf[3][2]));
                r[T9] = add32(r[T8], 0x00000060);
                r[AT] = c.rd64(r[T7] as u32);
                r[V0] = c.rd64((r[T7] as u32).wrapping_add(0x00000008));
                r[V1] = c.rd64((r[T7] as u32).wrapping_add(0x00000010));
                r[A0] = c.rd64((r[T7] as u32).wrapping_add(0x00000018));
                (r[AT], h[AT]) = mmi::pextlh((r[AT], h[AT]), (0u64, 0u64));
                (r[AT], h[AT]) = mmi::psraw((r[AT], h[AT]), 16);
                qmtc2(c, 1, (r[AT], h[AT]));
                (r[V0], h[V0]) = mmi::pextlh((r[V0], h[V0]), (0u64, 0u64));
                (r[V0], h[V0]) = mmi::psraw((r[V0], h[V0]), 16);
                qmtc2(c, 2, (r[V0], h[V0]));
                (r[V1], h[V1]) = mmi::pextlh((r[V1], h[V1]), (0u64, 0u64));
                (r[V1], h[V1]) = mmi::psraw((r[V1], h[V1]), 16);
                qmtc2(c, 3, (r[V1], h[V1]));
                (r[A0], h[A0]) = mmi::pextlh((r[A0], h[A0]), (0u64, 0u64));
                (r[A0], h[A0]) = mmi::psraw((r[A0], h[A0]), 16);
                qmtc2(c, 4, (r[A0], h[A0]));
                r[T7] = add32(r[T7], 0x00000020);
                program(c, k, &mut resident, 0x004a4144, n);
                p100af0_0(c);
                r[AT] = c.rd64(r[T7] as u32);
                r[V0] = c.rd64((r[T7] as u32).wrapping_add(0x00000008));
                r[V1] = c.rd64((r[T7] as u32).wrapping_add(0x00000010));
                r[A0] = c.rd64((r[T7] as u32).wrapping_add(0x00000018));
                (r[AT], h[AT]) = mmi::pextlh((r[AT], h[AT]), (0u64, 0u64));
                (r[AT], h[AT]) = mmi::psraw((r[AT], h[AT]), 16);
                qmtc2(c, 5, (r[AT], h[AT]));
                (r[V0], h[V0]) = mmi::pextlh((r[V0], h[V0]), (0u64, 0u64));
                (r[V0], h[V0]) = mmi::psraw((r[V0], h[V0]), 16);
                qmtc2(c, 6, (r[V0], h[V0]));
                (r[V1], h[V1]) = mmi::pextlh((r[V1], h[V1]), (0u64, 0u64));
                (r[V1], h[V1]) = mmi::psraw((r[V1], h[V1]), 16);
                qmtc2(c, 7, (r[V1], h[V1]));
                (r[A0], h[A0]) = mmi::pextlh((r[A0], h[A0]), (0u64, 0u64));
                (r[A0], h[A0]) = mmi::psraw((r[A0], h[A0]), 16);
                qmtc2(c, 8, (r[A0], h[A0]));
                r[T7] = add32(r[T7], 0x00000020);
                l = 0x004a41a4;
                continue;
            }
            0x004a41a4 => {
                program(c, k, &mut resident, 0x004a41a4, n);
                p100af0_160(c);
                (r[AT], h[AT]) = q4(c.vf[9]);
                r[T8] = add32(r[T8], 0x00000010);
                (r[AT], h[AT]) = mmi::ppach((0u64, 0u64), (r[AT], h[AT]));
                (r[V0], h[V0]) = q4(c.vf[10]);
                (r[AT], h[AT]) = mmi::ppacb((0u64, 0u64), (r[AT], h[AT]));
                (r[V1], h[V1]) = q4(c.vf[11]);
                (r[V0], h[V0]) = mmi::ppach((0u64, 0u64), (r[V0], h[V0]));
                (r[A0], h[A0]) = q4(c.vf[12]);
                (r[V0], h[V0]) = mmi::ppacb((0u64, 0u64), (r[V0], h[V0]));
                (r[V1], h[V1]) = mmi::ppach((0u64, 0u64), (r[V1], h[V1]));
                c.wr32((r[T8] as u32).wrapping_add(0xfffffff0), r[AT] as u32);
                (r[V1], h[V1]) = mmi::ppacb((0u64, 0u64), (r[V1], h[V1]));
                c.wr32((r[T8] as u32).wrapping_add(0xfffffff4), r[V0] as u32);
                (r[A0], h[A0]) = mmi::ppach((0u64, 0u64), (r[A0], h[A0]));
                c.wr32((r[T8] as u32).wrapping_add(0xfffffff8), r[V1] as u32);
                (r[A0], h[A0]) = mmi::ppacb((0u64, 0u64), (r[A0], h[A0]));
                let t = r[T8] == r[T9];
                c.wr32((r[T8] as u32).wrapping_add(0xfffffffc), r[A0] as u32);
                if t {
                    l = 0x004a430c;
                    continue;
                }
                r[AT] = c.rd64(r[T7] as u32);
                r[V0] = c.rd64((r[T7] as u32).wrapping_add(0x00000008));
                r[V1] = c.rd64((r[T7] as u32).wrapping_add(0x00000010));
                r[A0] = c.rd64((r[T7] as u32).wrapping_add(0x00000018));
                (r[AT], h[AT]) = mmi::pextlh((r[AT], h[AT]), (0u64, 0u64));
                (r[AT], h[AT]) = mmi::psraw((r[AT], h[AT]), 16);
                qmtc2(c, 1, (r[AT], h[AT]));
                (r[V0], h[V0]) = mmi::pextlh((r[V0], h[V0]), (0u64, 0u64));
                (r[V0], h[V0]) = mmi::psraw((r[V0], h[V0]), 16);
                qmtc2(c, 2, (r[V0], h[V0]));
                (r[V1], h[V1]) = mmi::pextlh((r[V1], h[V1]), (0u64, 0u64));
                (r[V1], h[V1]) = mmi::psraw((r[V1], h[V1]), 16);
                qmtc2(c, 3, (r[V1], h[V1]));
                (r[A0], h[A0]) = mmi::pextlh((r[A0], h[A0]), (0u64, 0u64));
                (r[A0], h[A0]) = mmi::psraw((r[A0], h[A0]), 16);
                qmtc2(c, 4, (r[A0], h[A0]));
                r[T7] = add32(r[T7], 0x00000020);
                program(c, k, &mut resident, 0x004a4254, n);
                p100af0_0(c);
                (r[AT], h[AT]) = q4(c.vf[13]);
                r[T8] = add32(r[T8], 0x00000010);
                (r[AT], h[AT]) = mmi::ppach((0u64, 0u64), (r[AT], h[AT]));
                (r[V0], h[V0]) = q4(c.vf[14]);
                (r[AT], h[AT]) = mmi::ppacb((0u64, 0u64), (r[AT], h[AT]));
                (r[V1], h[V1]) = q4(c.vf[15]);
                (r[V0], h[V0]) = mmi::ppach((0u64, 0u64), (r[V0], h[V0]));
                (r[A0], h[A0]) = q4(c.vf[16]);
                (r[V0], h[V0]) = mmi::ppacb((0u64, 0u64), (r[V0], h[V0]));
                (r[V1], h[V1]) = mmi::ppach((0u64, 0u64), (r[V1], h[V1]));
                c.wr32((r[T8] as u32).wrapping_add(0xfffffff0), r[AT] as u32);
                (r[V1], h[V1]) = mmi::ppacb((0u64, 0u64), (r[V1], h[V1]));
                c.wr32((r[T8] as u32).wrapping_add(0xfffffff4), r[V0] as u32);
                (r[A0], h[A0]) = mmi::ppach((0u64, 0u64), (r[A0], h[A0]));
                c.wr32((r[T8] as u32).wrapping_add(0xfffffff8), r[V1] as u32);
                (r[A0], h[A0]) = mmi::ppacb((0u64, 0u64), (r[A0], h[A0]));
                let t = r[T8] == r[T9];
                c.wr32((r[T8] as u32).wrapping_add(0xfffffffc), r[A0] as u32);
                if t {
                    l = 0x004a430c;
                    continue;
                }
                r[AT] = c.rd64(r[T7] as u32);
                r[V0] = c.rd64((r[T7] as u32).wrapping_add(0x00000008));
                r[V1] = c.rd64((r[T7] as u32).wrapping_add(0x00000010));
                r[A0] = c.rd64((r[T7] as u32).wrapping_add(0x00000018));
                (r[AT], h[AT]) = mmi::pextlh((r[AT], h[AT]), (0u64, 0u64));
                (r[AT], h[AT]) = mmi::psraw((r[AT], h[AT]), 16);
                qmtc2(c, 5, (r[AT], h[AT]));
                (r[V0], h[V0]) = mmi::pextlh((r[V0], h[V0]), (0u64, 0u64));
                (r[V0], h[V0]) = mmi::psraw((r[V0], h[V0]), 16);
                qmtc2(c, 6, (r[V0], h[V0]));
                (r[V1], h[V1]) = mmi::pextlh((r[V1], h[V1]), (0u64, 0u64));
                (r[V1], h[V1]) = mmi::psraw((r[V1], h[V1]), 16);
                qmtc2(c, 7, (r[V1], h[V1]));
                (r[A0], h[A0]) = mmi::pextlh((r[A0], h[A0]), (0u64, 0u64));
                (r[A0], h[A0]) = mmi::psraw((r[A0], h[A0]), 16);
                qmtc2(c, 8, (r[A0], h[A0]));
                r[T7] = add32(r[T7], 0x00000020);
                let t = 0u64 == 0u64;
                if t {
                    l = 0x004a41a4;
                    continue;
                }
                l = 0x004a430c;
                continue;
            }
            0x004a430c => {
                let t = 0u64 == 0u64;
                if t {
                    l = 0x004a3ddc;
                    continue;
                }
                l = 0x004a4314;
                continue;
            }
            0x004a4314 => {
                r[AT] = 0x0000000070000000;
                r[FP] = c.rd64((r[AT] as u32).wrapping_add(0x00003ff8));
                r[S0] = c.rd64((r[AT] as u32).wrapping_add(0x00003ff0));
                r[S1] = c.rd64((r[AT] as u32).wrapping_add(0x00003fe8));
                r[S2] = c.rd64((r[AT] as u32).wrapping_add(0x00003fe0));
                break;
            }
            _ => unreachable!("0x4A3D80: no block at {l:08x}"),
        }
    }
    c.r = r;
    c.rh = h;
}

// ---- the shrub VU0 program (tag 0x100AE0, uyatr's `p100af0`) -------------------------------------------------------------------

/// A VCALLMS of the shrub program by 0x4A3D80 (at 0x4A4144, 0x4A41A4, 0x4A4254), whose run is done here. What the
/// translation's VCALLMS would run is checked once a call of 0x4A3D80, at its first start: no microprogram waits for the EE
/// (a start would let it go on first, `vu0::vcallms`) and micro memory holds the shrub program at 0x0 and 0x160
/// (0x4010D0 sends it at 0x401684 before its call at 0x40179C, the level's setup 0x4252F0 at 0x4256D0 before 0x4262D4).
fn program(c: &mut Cpu, k: F, resident: &mut bool, pc: u32, n: &mut [u64; N]) {
    n[5] += 1;
    if *resident {
        return;
    }
    let m = c.mach();
    if let Some(p) = m.vu0_parked {
        panic!("the shrub lighting's VCALLMS at {:08x}: the VU0 microprogram at {p:#x} still waits for the EE", k.pc(pc));
    }
    let held = [0usize, 0x160].map(|a| m.vu0_index(a).map(|(_, p)| p.name));
    if held != [Some("p100af0"); 2] {
        panic!(
            "the shrub lighting's VCALLMS at {:08x}: micro memory {} holds no shrub program (tag 0x100AE0) at 0x0 and 0x160",
            k.pc(pc),
            m.vu0_census()
        );
    }
    *resident = true;
}

/// The program from micro 0x0 (its pairs 0x0 to 0x158, the E bit at 0x150): the normals of vf1 to vf4 (ITOF15 of x, y, z;
/// LOI 0x478000F3 with the first) turned by the light directions' rows vf24 to vf26 (MULAx, MADDAy, MADDz on every lane),
/// each with its product by vf30 (MUL.xyz into vf17 to vf20) taken where larger (MAX.xyz), then the colours vf9 to vf12 =
/// vf31 w + vf27 x + vf28 y + vf29 z of each (MULAw by vf0, MADDAx, MADDAy, MADDz), x, y and z no more than I (MINIi).
fn p100af0_0(c: &mut Cpu) {
    shrub_program(c, [1, 2, 3, 4], [9, 10, 11, 12]);
}

/// The program from micro 0x160 (its pairs 0x160 to 0x2B8, the E bit at 0x2B0): the same for the normals of vf5 to vf8, the
/// colours into vf13 to vf16.
fn p100af0_160(c: &mut Cpu) {
    shrub_program(c, [5, 6, 7, 8], [13, 14, 15, 16]);
}

/// One run of the shrub program on the normals of VF `v` into the colours of VF `o`, pair by pair as its translation
/// (uyatr's vu0mp.rs, `p100af0`) does it: every result in its pair's order (the program is straight code whose every
/// reading pair the VU's interlocks hold until its operands land, so each reads the value before it), ACC, vf17 to vf20 and
/// I as the run leaves them; Q as it was (no DIV in the run); the MAC flags of its last MAC-writing pair (the colours' last
/// MADDz) and the status flag's zero and sign bits from them, their sticky copies or'ed with every MAC write's, as the run's
/// end lands them.
#[inline(always)]
fn shrub_program(c: &mut Cpu, v: [usize; 4], o: [usize; 4]) {
    let (mut mlast, mut mso) = (0u32, 0u32);
    let mut mac = |f: u32| {
        mlast = f;
        mso |= f;
    };
    for (j, &n) in v.iter().enumerate() {
        let u = m_itof::<3>(c.vf[n]);
        m_put::<14>(&mut c.vf[n], u);
        if j == 0 {
            c.vi[21] = 0x4780_00f3;
        }
    }
    for &n in &v {
        let u = m_fmac::<6, 15>(c.vf[24], [c.vf[n][0]; 4], c.vacc);
        m_put::<15>(&mut c.vacc, u);
        mac(m_mac::<15>(u));
        let u = m_fmac::<2, 15>(c.vf[25], [c.vf[n][1]; 4], c.vacc);
        m_put::<15>(&mut c.vacc, u);
        mac(m_mac::<15>(u));
        let u = m_fmac::<2, 15>(c.vf[26], [c.vf[n][2]; 4], c.vacc);
        m_put::<15>(&mut c.vf[n], u);
        mac(m_mac::<15>(u));
    }
    for (j, &n) in v.iter().enumerate() {
        let u = m_fmac::<6, 14>(c.vf[n], c.vf[30], c.vacc);
        m_put::<14>(&mut c.vf[17 + j], u);
        mac(m_mac::<14>(u));
    }
    for (j, &n) in v.iter().enumerate() {
        let u = m_fmac::<4, 14>(c.vf[n], c.vf[17 + j], c.vacc);
        m_put::<14>(&mut c.vf[n], u);
    }
    for (j, &n) in v.iter().enumerate() {
        let u = m_fmac::<6, 15>(c.vf[31], [c.vf[0][3]; 4], c.vacc);
        m_put::<15>(&mut c.vacc, u);
        mac(m_mac::<15>(u));
        let u = m_fmac::<2, 15>(c.vf[27], [c.vf[n][0]; 4], c.vacc);
        m_put::<15>(&mut c.vacc, u);
        mac(m_mac::<15>(u));
        let u = m_fmac::<2, 15>(c.vf[28], [c.vf[n][1]; 4], c.vacc);
        m_put::<15>(&mut c.vacc, u);
        mac(m_mac::<15>(u));
        let u = m_fmac::<2, 15>(c.vf[29], [c.vf[n][2]; 4], c.vacc);
        m_put::<15>(&mut c.vf[o[j]], u);
        mac(m_mac::<15>(u));
    }
    for &n in &o {
        let u = m_fmac::<5, 14>(c.vf[n], [c.vi[21]; 4], c.vacc);
        m_put::<14>(&mut c.vf[n], u);
    }
    // The run's end lands the flags (the translation's `macs`: the run wrote MAC flags).
    let st = ((mlast & 0xf != 0) as u32) | (((mlast & 0xf0 != 0) as u32) << 1);
    let so = ((mso & 0xf != 0) as u32) | (((mso & 0xf0 != 0) as u32) << 1);
    c.vi[17] = mlast;
    c.vi[16] = (c.vi[16] & !0xf) | st | (so << 6);
}
