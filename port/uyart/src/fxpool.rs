//! The effects' pool builder (level 1 0x45E810, frontbin func_003C7E68, with the texture placer 0x45F438 it calls), done
//! natively in front of its translation (port/uyatr `JOBS`): the effects layer's cull, its depth sort and the chain VU1 program
//! 6 draws from (`uyafmt::prog6`, the renderer's `port/uya/src/effects.rs`). The translated wrapper at 0x45E810 asks `start`
//! (`Rt::native` 28) and answers `after` (29) once the translation ran when `start` let it run. `c.ret` brings the function's
//! address with 1 in its low bit and 2 when the program's view is widened (a story level: `VIEW_LOADS` "the effects' pool
//! builder" +0x4C; the front ends' is the game's own).
//!
//! The frame calls it (0x4010D0 at 0x401354 when 0x1DF9C4 & 0x20, between its FlushCache and TEST_1 writes; 0x4D3460 at
//! 0x4D3564 the same way). Neither caller is glue: each is a whole frame's draw sequence (457 and 150 words) calling many
//! parts, and what it does around the call (FlushCache(0) before, `v0 = 9`, `a0 = 71`, `a1 = 0x5360B` and 0x4C1FE8's TEST_1
//! after, which reads only a0 and a1) is the frame's own. So the unit is 0x45E810 and the one function it calls:
//! - 0x45E810 is hand-written (s0 to s3 and ra kept in their own high halves, PCPYLD at the entry, PCPYUD at the end): the
//!   pool's entries (64 bytes, `[gp+0x3A48]`, the highest index `[gp+0x3A50]`) come in by SPR_TO 32 at a time into the
//!   scratchpad's halves 0 and 0x800; each is tested against the view and its near and far distances (kinds 0 and 1 by its
//!   view-space depth and side, kinds 2 and 3 by CLIP of both ends) and a drawn one is put in one of 1024 depth buckets
//!   (heads at 0x3000, nodes from 0x1000); one culled with its drawn bit (flags 0x20) is listed at 0x3800 and the bit
//!   cleared in RAM; the buckets are read from the deepest into a sorted list at 0x0000; then the texture table (to 0x3000)
//!   and the sine table (to 0x3400) come in, and each listed entry's item (4, 5, 4 or 6 quadwords by its kind) is built in
//!   the scratchpad's halves 0x1000 and 0x1800, each sent by SPR_FROM to the chain at `[gp+0x4838]` when it fills, with the
//!   run's DMA tag and footer (`[gp-0x2890]`, `[gp-0x2880]`, `[gp-0x2870]`) around it.
//! - 0x45F438 (only the unit calls it: its three BGEZALs, in every program) gives a texture word not yet placed its VRAM slot
//!   (`[gp-0x7D00]`, s0), a record at 0x1E7900 + 16 x `[gp+0x314C]` (s2, s1) and its TEX0 word in the scratchpad's copy of
//!   the table.
//!
//! The functions are the same code in every program: uyatr finds the unit by size and shape, one in each of the 34 story
//! levels and the two front ends and nothing else, its callee at its end in each (the census of the 36 programs' functions
//! by `uyatr dis`). What relocation changes in it (the gp offsets, the lui/addiu pairs of the camera struct 0x1E2900, the view
//! struct 0x1E7600, the texture records 0x1E7900, the texture table 0x220AC0, the sine table 0x2BFDA0 and boot's program 6
//! tag 0x101140 / 0x101150, the BGEZALs' target) is read from the code in RAM (`F`), named by level 1's address of the
//! instruction holding it; every other word is the same in every program (the words of level 1's copy against those of
//! fe100, fe101 and levels 2, 7, 28 and 36 differ only there and in the J targets inside the function).
//!
//! Output contract: the unit is its translation's statements in order on the thread's own registers, so what it leaves is
//! what the translation leaves:
//! - RAM: the entries' flag bytes (+1: bit 0x20 cleared at 0x45EC14, set at 0x45EF28, 0x45F080, 0x45F140, 0x45F238); the
//!   chain from `[gp+0x4838]` (the header's SPR_FROM at 0x45EE58: a REF tag to program 6's upload and eight quadwords of
//!   its constants; each run's at 0x45F338 and the last at 0x45F3E8); `[gp-0x7D00]` (s0),
//!   `[gp+0x314C]` (s1), `[gp+0x4838]` (t9) at 0x45F410 to 0x45F418; the texture records (0x45F444 to 0x45F4AC).
//! - The scratchpad: 0x0000 to 0x1FFF (the pool's batches, then the sorted list; the bucket nodes, then the staging halves),
//!   0x3000 to 0x3FFF (the bucket heads, then the texture table with its TEX0 words, 0x45F4CC; the sine table; the list of
//!   cleared flags).
//! - Every GPR, both halves (s0 to s3 and ra their low halves with the high halves 0, 0x45F420 to 0x45F434; the rest as the
//!   last entry leaves them, their high halves from LQ, QMFC2 and the MMI operations), HI/LO and HI1/LO1 (MULT at 0x45EF6C
//!   and 0x45F0A4, PMULTUW at 0x45F264), the FPU's f0 to f5, VU0's vf1 to vf7 and vf17 to vf31, ACC, the MAC, status and clip
//!   flags and Q, `view_h` (the widened load), the DMA channels 8 and 9, D_STAT and the DMAC interrupts at each transfer's end.
//! - What the code after it reads: 0x40135C and 0x4D356C write v0, a0 and a1 before they read a register, and 0x4C1FE8 reads
//!   a0 and a1 only; the chain is read by the VIF1 kick that sends it, the flags by the next frame's call, the records and the
//!   count by the texture uploads. Everything above is produced, so no part of the state is left to the translation.
//!
//! What goes away is the hardware's traffic: the stores to SPR_FROM's and SPR_TO's registers become the transfers they kick
//! (`dw`, as `hw::kick` makes them: the code's scratchpad addresses are 0, 0x800, 0x3000 and 0x3400 for SPR_TO and 0x1000
//! and 0x1800 for SPR_FROM, so none is the skin job's), the polls read the channel as the port keeps it, BC0F's waits are
//! not made (`Cpu::cpcond0` is always true), the loops' safe points (the unit is one block of the thread's time, as the
//! moby and shrub units are).
//!
//! With UYA_NATIVE_CHECK=fxpool:N (any other unit's job leaves this one unchecked) the first N calls run twice from the same
//! state: the translation, then the native unit on the state as it was before; every difference in the machine's state they
//! leave (`differences`) is reported, with what the calls covered every 100 calls, and the game goes on from the
//! translation's state. Both runs go with the thread's interrupts held (`Check::ints`), as the moby unit's check does.

use crate::{Cpu, Ext};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use uycpu::vu0::{m_abs, m_fmac, m_ftoi, m_mac, m_put};
use uycpu::{fpu, mmi, vu0};

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
        let a = self.pc(pc);
        assert!(a < uycpu::RAM_SIZE - 4, "the effects' pool builder's code at {a:08x}: not in RAM");
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
    /// This program's address of level 1's `pc` (a return address).
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
const T8: usize = 24;
const T9: usize = 25;
const GP: usize = 28;
const SP: usize = 29;
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
/// SRA.
#[inline(always)]
fn sra(a: u64, s: u32) -> u64 {
    sx(((a as u32 as i32) >> s) as u32)
}
/// A register plus a load's or store's offset.
#[inline(always)]
fn ad(r: u64, off: u32) -> u32 {
    (r as u32).wrapping_add(off)
}
/// A GPR's 128 bits.
#[inline(always)]
fn g(c: &Cpu, r: usize) -> Q {
    (c.r[r], c.rh[r])
}
/// An MMI result into a GPR (both halves).
#[inline(always)]
fn s(c: &mut Cpu, r: usize, v: Q) {
    (c.r[r], c.rh[r]) = v;
}
/// $zero's 128 bits.
const Z: Q = (0, 0);

/// PCPYLD r, r, zero: the low half kept in the high half, the low half 0 (the unit's saves).
#[inline(always)]
fn keep(c: &mut Cpu, r: usize) {
    let v = mmi::pcpyld(g(c, r), Z);
    s(c, r, v);
}
/// PCPYUD r, r, zero: the high half back in the low half, the high half 0.
#[inline(always)]
fn give(c: &mut Cpu, r: usize) {
    let v = mmi::pcpyud(g(c, r), Z);
    s(c, r, v);
}
/// MULT zero, rs, rt: the 64-bit product's words into LO and HI.
#[inline(always)]
fn mult(c: &mut Cpu, rs: usize, rt: usize) {
    let p = ((c.r[rs] as u32 as i32 as i64) * (c.r[rt] as u32 as i32 as i64)) as u64;
    c.lo = sx(p as u32);
    c.hi = sx((p >> 32) as u32);
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
/// QMTC2 of GPR `r` (both halves) into VF `v`.
#[inline(always)]
fn qmtc2(c: &mut Cpu, v: usize, r: usize) {
    if v != 0 {
        c.vf[v] = w4(g(c, r));
    }
}
/// QMFC2 of VF `v` into GPR `r` (both halves).
#[inline(always)]
fn qmfc2(c: &mut Cpu, r: usize, v: usize) {
    let q = q4(c.vf[v]);
    s(c, r, q);
}

const ACC: usize = 32;

/// An FMAC macro instruction (K: 0 ADD, 1 SUB, 2 MADD, 3 MSUB, 4 MAX, 5 MINI, 6 MUL) on lanes D into VF `fd` or ACC (`ACC`;
/// `fd` 0 writes nothing), with the MAC and status flags it makes: `uycpu::vu0::fmac`'s result on four lanes at once
/// (`m_fmac` under the game thread's MXCSR, `uycpu::fpu::MXCSR`, is the VU's arithmetic: `fin` on each operand, and toward
/// zero a finite product never reaches exponent 255).
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

/// FTOI12 on lanes D (`uycpu::vu0::ftoi::<2, D>`).
#[inline(always)]
fn ftoi12<const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    if ft != 0 {
        let r = m_ftoi::<2>(c.vf[fs]);
        m_put::<D>(&mut c.vf[ft], r);
    }
}

/// ABS on lanes D (`uycpu::vu0::abs::<D>`).
#[inline(always)]
fn vabs<const D: u32>(c: &mut Cpu, ft: usize, fs: usize) {
    if ft != 0 {
        let r = m_abs(c.vf[fs]);
        m_put::<D>(&mut c.vf[ft], r);
    }
}

// ---- the scratchpad's DMA channels, as the port's kicks do them -----------------------------------------------------------

const SPR: u32 = 0x7000_0000;

/// A 32-bit store, the scratchpad channels' registers by their meaning (the unit's stores to 0x1000D000 and 0x1000D400 and
/// their offsets; D_STAT, 0x1000E010, is the hardware's store).
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
        (_, mode) => panic!("the effects' pool builder's DMA on channel {ch} in mode {mode} (CHCR {v:#x}): the unit makes normal transfers only"),
    }
}

/// SPR_TO (channel 9) in normal mode with these register values: the copy into the scratchpad, the registers as the kick
/// leaves them, the channel's end and its interrupt.
fn spr_to(c: &mut Cpu, chcr: u32, madr: u32, qwc: u32, sadr: u32) {
    let mp = c.mp();
    let m = c.mach();
    let h = &mut m.hw.ch[9];
    (h[0], h[1], h[2], h[8]) = (chcr, madr, qwc, sadr);
    let qwc = qwc & 0xffff;
    let mut s = sadr & 0x3ff0;
    put(m, &mut s, madr, qwc);
    let h = &mut m.hw.ch[9];
    h[1] = madr.wrapping_add(qwc * 16);
    h[8] = s;
    end(c, mp, 9);
}

/// SPR_FROM (channel 8) in normal mode: the copy to RAM, the registers, the end.
fn spr_from(c: &mut Cpu, chcr: u32, madr: u32, qwc: u32, sadr: u32) {
    let mp = c.mp();
    let m = c.mach();
    let h = &mut m.hw.ch[8];
    (h[0], h[1], h[2], h[8]) = (chcr, madr, qwc, sadr);
    let qwc = qwc & 0xffff;
    let s = sadr & 0x3ff0;
    let (s0, d0, n) = (s as usize, (madr & 0x01ff_fff0) as usize, 16 * qwc as usize);
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

/// The unit's poll of SPR_FROM (`lw v1, 0(a0)` of D8_CHCR, `andi v1, v1, 0x100`, a wait while set: 0x45F2F8, 0x45F3A8,
/// 0x45F3F0): the port's transfers end at their kick, so STR is never set when the game reads it.
#[inline(always)]
fn poll(c: &mut Cpu) {
    c.r[V1] = sx(c.mach().hw.ch[8][0]) & 0x100;
    if c.r[V1] != 0 {
        panic!("the effects' pool builder's poll of SPR_FROM found it busy (CHCR {:#x}): the port's transfers end at their kick", c.mach().hw.ch[8][0]);
    }
}

/// SPR_FROM of the staging half t8 is in (a0 0x1000D000, at = t8 & 0x7FF, t8 at the half's start): SADR t8, D_STAT's
/// bit 8 cleared, MADR t9 - at, QWC at / 16, CHCR 0x100 (0x45EE38, 0x45F318, 0x45F3C8).
#[inline(always)]
fn kick_from(c: &mut Cpu) {
    dw(c, ad(c.r[A0], 0x80), c.r[T8] as u32);
    c.r[V0] = 0x100;
    dw(c, ad(c.r[A0], 0x1010), c.r[V0] as u32);
    c.r[V0] = sub32(c.r[T9], c.r[AT]);
    dw(c, ad(c.r[A0], 0x10), c.r[V0] as u32);
    c.r[V0] = srl(c.r[AT], 4);
    dw(c, ad(c.r[A0], 0x20), c.r[V0] as u32);
    c.r[V0] = 0x100;
    dw(c, c.r[A0] as u32, c.r[V0] as u32);
}

/// SPR_TO of the pool's next 32 entries (128 quadwords from t8) into the scratchpad at `sadr` (t7 0x1000D400): MADR,
/// D_STAT's bit 9 cleared, QWC, SADR, CHCR 0x100 (0x45E8B8 with sadr 0 from at, 0x45E950 with sadr v0).
#[inline(always)]
fn pool_in(c: &mut Cpu, sadr: Option<usize>) {
    dw(c, ad(c.r[T7], 0x10), c.r[T8] as u32);
    c.r[AT] = 0x200;
    dw(c, ad(c.r[T7], 0xc10), c.r[AT] as u32);
    c.r[AT] = 0x80;
    dw(c, ad(c.r[T7], 0x20), c.r[AT] as u32);
    let r = match sadr {
        Some(r) => r,
        None => {
            c.r[AT] = 0;
            AT
        }
    };
    dw(c, ad(c.r[T7], 0x80), c.r[r] as u32);
    c.r[AT] = 0x100;
    dw(c, c.r[T7] as u32, c.r[AT] as u32);
    c.r[T8] = add32(c.r[T8], 0x800);
}

// ---- what the calls covered (the check's report) ---------------------------------------------------------------------------

/// Counts of the paths the native calls took: calls that built no chain (an empty pool, nothing drawn), entries read, entries
/// skipped (flags bit 7), entries culled, entries sorted into a bucket, drawn bits cleared, items of each kind, textures
/// placed (0x45F438), runs sent.
const N: usize = 12;
static DONE: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
const NAMES: [&str; N] =
    ["calls with no chain", "entries read", "entries skipped", "entries culled", "entries sorted", "drawn bits cleared", "sprites", "ground quads", "lines", "ribbons", "textures placed", "runs"];
const NOTHING: usize = 0;
const READ: usize = 1;
const SKIPPED: usize = 2;
const CULLED: usize = 3;
const SORTED: usize = 4;
const CLEARED: usize = 5;
const KIND: usize = 6;
const PLACED: usize = 10;
const RUNS: usize = 11;

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
/// memories (the chain, the pool's flags, the records, the scratchpad's every staging area) is what the translation leaves.
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
    d
}

struct Check {
    left: u64,
    done: u64,
    differ: u64,
    kinds: std::collections::BTreeSet<String>,
    before: Option<State>,
    /// Whether the thread had its interrupts on at the checked call: both runs go with them held (DI), and EI after the
    /// comparison takes what waited (`mobydraw`'s `Check::ints`: each transfer's end raises its DMAC interrupt, and a loop's
    /// back edge in the translation past the field's time gives the field up; held, each lands once, after both runs).
    ints: bool,
}

static CHECK: std::sync::Mutex<Option<Check>> = std::sync::Mutex::new(None);

/// UYA_NATIVE_CHECK=fxpool:N, read once: N calls of the unit; another unit's job leaves this one unchecked.
fn checked() -> Option<u64> {
    static C: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_NATIVE_CHECK").ok()?;
        let (job, n) = v.split_once(':').unwrap_or_else(|| panic!("UYA_NATIVE_CHECK={v}: <job>:N"));
        match job {
            "fxpool" => Some(n.parse().unwrap_or_else(|_| panic!("UYA_NATIVE_CHECK={v}: N is a number of calls"))),
            _ => None,
        }
    })
}

thread_local! {
    /// The function's address and view bit of the call being checked, for its native run after the translation's.
    static CALL: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((0, false)) };
}

/// `Rt::native` 28: the unit natively (`c.ret` the return address), or (checking) the state saved and `c.ret` 0 so the
/// translation runs.
pub(crate) fn start(c: &mut Cpu, _k: u32) {
    let (at, wide) = (c.ret & !3, c.ret & 2 != 0);
    if let Some(n) = checked() {
        let mut g = CHECK.lock().expect("the effects' pool check");
        let ck = g.get_or_insert(Check { left: n, done: 0, differ: 0, kinds: Default::default(), before: None, ints: false });
        if ck.left > 0 {
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
    run(c, at, wide);
    c.ret = c.r[RA] as u32;
}

/// `Rt::native` 29: the translation ran; the unit runs natively from the state before it and the two are compared.
pub(crate) fn after(c: &mut Cpu, _k: u32) {
    let (Some(before), ints) = ({
        let mut g = CHECK.lock().expect("the effects' pool check");
        let ck = g.as_mut().expect("checking");
        (ck.before.take(), ck.ints)
    }) else {
        return;
    };
    let ret = c.ret;
    let want = State::take(c);
    before.put(c);
    let (at, wide) = CALL.with(|w| w.get());
    let sp = c.r[SP] as u32;
    run(c, at, wide);
    c.ret = c.r[RA] as u32;
    let got = State::take(c);
    let d = differences(&want, &got);
    want.put(c);
    c.ret = ret;
    let mut g = CHECK.lock().expect("the effects' pool check");
    let ck = g.as_mut().expect("checking");
    ck.left -= 1;
    ck.done += 1;
    if !d.is_empty() {
        ck.differ += 1;
        // The first calls that differ, and the first of each other kind of difference (what differs, without the values).
        let kind: String = d.iter().map(|x| x.split(':').next().unwrap_or(x)).collect::<Vec<_>>().join("+");
        if ck.differ <= 6 || ck.kinds.insert(kind) {
            println!("UYA_NATIVE_CHECK: fxpool call {} (at {at:08x}, sp {sp:08x}): {}", ck.done, d.join("; "));
        }
    }
    if ck.left == 0 || ck.done % 100 == 0 {
        let n: Vec<String> = DONE.iter().zip(NAMES).map(|(a, name)| format!("{name} {}", a.load(Relaxed))).collect();
        println!("UYA_NATIVE_CHECK: fxpool: {} calls checked, {} differ ({})", ck.done, ck.differ, n.join(", "));
    }
    drop(g);
    // The interrupts held for the two runs (`Check::ints`): what came due runs now, as at the game's own EI.
    if ints {
        c.ei();
        c.ret = ret;
    }
}

/// The unit natively, with its counts.
fn run(c: &mut Cpu, at: u32, wide: bool) {
    let mut n = [0u64; N];
    f45e810(c, F { at, l1: 0x45_e810 }, wide, &mut n);
    for (a, v) in DONE.iter().zip(n) {
        if v != 0 {
            a.fetch_add(v, Relaxed);
        }
    }
}

// ---- 0x45E810 ---------------------------------------------------------------------------------------------------------------
//
// Its translation's statements in order, the delay slots where the translation puts them (a branch's condition is read
// before its delay slot runs), each named by level 1's address where the code turns.

/// 0x45E810: the cull, the depth sort and program 6's chain.
fn f45e810(c: &mut Cpu, k: F, wide: bool, n: &mut [u64; N]) {
    let gp = c.r[GP] as u32;
    // The pool (t8), the camera struct (at, 0x1E2900), the view struct (v0, 0x1E7600), the highest index (t9); s0 to s3 kept
    // in their high halves; the view in vf20 to vf31.
    c.r[T8] = sx(c.rd32(k.gp(c, gp, 0x45_e810)));
    c.r[AT] = sx(k.a(c, 0x45_e814, 0x45_e818));
    c.r[V0] = sx(k.a(c, 0x45_e81c, 0x45_e820));
    c.r[T9] = sx(c.rd32(k.gp(c, gp, 0x45_e824)));
    lqc2(c, 28, ad(c.r[AT], 0x140));
    keep(c, S0);
    lqc2(c, 29, c.r[AT] as u32);
    keep(c, S1);
    lqc2(c, 30, ad(c.r[AT], 0x10));
    keep(c, S2);
    lqc2(c, 31, ad(c.r[AT], 0x20));
    keep(c, S3);
    lqc2(c, 20, ad(c.r[AT], 0xc0));
    lqc2(c, 21, ad(c.r[AT], 0xd0));
    lqc2(c, 22, ad(c.r[AT], 0xe0));
    lqc2(c, 23, ad(c.r[AT], 0xf0));
    lqc2(c, 24, ad(c.r[V0], 0x1c0));
    // +0x4C: the view's tangents (h, v, 1/cos(atan h), 1/cos(atan v)); on a story level h times `view_k` and `fov`, v times
    // `fov` (`VIEW_LOADS`), as the translation's load.
    {
        let (a, b) = c.rd128(ad(c.r[V0], 0xb0));
        let (h, v) = if wide { (c.widen(a as u32), c.fov((a >> 32) as u32)) } else { (a as u32, (a >> 32) as u32) };
        c.vf[26] = [h, v, b as u32, (b >> 32) as u32];
    }
    lqc2(c, 27, ad(c.r[V0], 0x1f0));
    lqc2(c, 25, k.gp(c, gp, 0x45_e864));
    c.r[S3] = sx(c.rd32(k.gp(c, gp, 0x45_e868)));
    c.r[AT] = 0x4480_0000;
    c.r[T7] = 0x1000_0000;
    qmtc2(c, 1, AT);
    c.r[T7] |= 0xd400;
    c.r[T9] = add32(c.r[T9], 1);
    c.r[AT] = 0x3f70_0000;
    fm::<6, 15>(c, 20, 20, b4(c.vf[1][0]));
    fm::<6, 15>(c, 21, 21, b4(c.vf[1][0]));
    fm::<6, 15>(c, 22, 22, b4(c.vf[1][0]));
    qmtc2(c, 1, AT);
    fm::<6, 12>(c, 24, 24, b4(c.vf[1][0]));
    c.r[AT] = SPR as u64;
    c.r[T2] = add32(c.r[AT], 0x3800);
    c.r[T3] = add32(c.r[AT], 0x1000);
    c.r[T4] = add32(c.r[AT], 0x3000);
    c.r[T5] = add32(c.r[AT], 0x800);
    let none = c.r[T9] == 0;
    keep(c, RA);
    if none {
        n[NOTHING] += 1;
        give_back(c);
        return;
    }
    // 0x45E8B8: the first 32 entries to scratchpad 0; the bucket heads (0x3000 to 0x37FF) cleared.
    pool_in(c, None);
    c.r[AT] = add32(c.r[T4], 0xffff_ffc0);
    c.r[V0] = add32(c.r[T4], 0x7c0);
    loop {
        c.wr128(ad(c.r[AT], 0x40), 0, 0);
        c.r[AT] = add32(c.r[AT], 0x40);
        c.wr128(ad(c.r[AT], 0x10), 0, 0);
        c.wr128(ad(c.r[AT], 0x20), 0, 0);
        let more = c.r[AT] != c.r[V0];
        c.wr128(ad(c.r[AT], 0x30), 0, 0);
        if !more {
            break;
        }
    }
    // 0x45E908: each batch of 32 (t6 the count left after it); the next one comes in to the other half meanwhile.
    'batch: loop {
        let all = c.r[T9] == 0;
        c.r[V0] = sx(0xffff_f800);
        if all {
            break;
        }
        c.r[V0] &= c.r[T5];
        c.r[T6] = add32(c.r[T9], 0xffff_ffe0);
        // 0x45E920: BC0F's wait for the transfer (`Cpu::cpcond0`: ended at its kick).
        let v = mmi::pmaxw(g(c, T6), Z);
        s(c, T6, v);
        let last = c.r[T6] == 0;
        c.r[T5] = c.r[V0] ^ 0x800;
        if !last {
            pool_in(c, Some(V0));
        }
        c.r[T5] = add32(c.r[T5], 0xffff_ffc0);
        // 0x45E980: each entry of the batch.
        loop {
            let end = c.r[T9] == c.r[T6];
            lqc2(c, 1, ad(c.r[T5], 0x50));
            if end {
                continue 'batch;
            }
            n[READ] += 1;
            c.r[AT] = sx(c.rd32(ad(c.r[T5], 0x4c)));
            c.r[T5] = add32(c.r[T5], 0x40);
            fm::<1, 14>(c, 2, 1, c.vf[28]);
            c.r[T1] = c.rd8(ad(c.r[T5], 1)) as i8 as i64 as u64;
            qmtc2(c, 3, AT);
            c.r[T9] = add32(c.r[T9], 0xffff_ffff);
            let skip = (c.r[T1] as i64) < 0;
            c.r[AT] = c.r[T1] & 0x2;
            if skip {
                n[SKIPPED] += 1;
                continue;
            }
            let two = c.r[AT] != 0;
            fm::<6, 12>(c, 3, 25, b4(c.vf[3][0]));
            let seen = if two { cull_ends(c) } else { cull_point(c) };
            if seen {
                // Into its depth bucket (v0): the node (index t9, the bucket's last node) at t3, the head to it.
                n[SORTED] += 1;
                c.r[A0] = c.rd16(c.r[V0] as u32) as u64;
                c.wr16(c.r[V0] as u32, c.r[T3] as u16);
                c.wr16(c.r[T3] as u32, c.r[T9] as u16);
                c.wr16(ad(c.r[T3], 2), c.r[A0] as u16);
                c.r[T3] = add32(c.r[T3], 4);
                continue;
            }
            // 0x45EBC0: culled; one that was drawn (t0 its flags without 0x20) is listed at t2 while the list has room.
            n[CULLED] += 1;
            let same = c.r[T0] == c.r[T1];
            c.r[AT] = c.r[T2] & 0x4000;
            if same || c.r[AT] != 0 {
                continue;
            }
            n[CLEARED] += 1;
            c.wr16(c.r[T2] as u32, c.r[T9] as u16);
            c.wr16(ad(c.r[T2], 2), c.r[T0] as u16);
            c.r[T2] = add32(c.r[T2], 4);
        }
    }
    // 0x45EBE0: the culled entries' drawn bits cleared in RAM.
    c.r[T8] = sx(c.rd32(k.gp(c, gp, 0x45_ebe0)));
    c.r[T3] = SPR as u64;
    c.r[T7] = sx(c.rd32(k.gp(c, gp, 0x45_ebe8)));
    c.r[T3] = add32(c.r[T3], 0x3800);
    if c.r[T3] != c.r[T2] {
        loop {
            c.r[V0] = c.rd16(c.r[T3] as u32) as u64;
            c.r[V1] = c.rd16(ad(c.r[T3], 2)) as u64;
            c.r[T3] = add32(c.r[T3], 4);
            c.r[V0] = sub32(c.r[T7], c.r[V0]);
            c.r[V0] = sll(c.r[V0], 6);
            c.r[V0] = add32(c.r[V0], c.r[T8]);
            let more = c.r[T3] != c.r[T2];
            c.wr8(ad(c.r[V0], 1), c.r[V1] as u8);
            if !more {
                break;
            }
        }
    }
    // 0x45EC18: the buckets from the deepest (0x37FE) to the nearest (0x3002; 0x3000's is not read), each one's entries
    // (latest first) into the sorted list of indices at scratchpad 0 (t4), ended by 0.
    c.r[T3] = SPR as u64;
    c.r[T4] = add32(c.r[T3], 0);
    c.r[T5] = add32(c.r[T3], 0x37fe);
    c.r[T6] = add32(c.r[T3], 0x2ffe);
    loop {
        c.r[AT] = c.rd16(c.r[T5] as u32) as u64;
        c.r[T5] = add32(c.r[T5], 0xffff_fffe);
        if c.r[T5] == c.r[T6] {
            break;
        }
        let empty = c.r[AT] == 0;
        c.r[AT] |= c.r[T3];
        if empty {
            continue;
        }
        loop {
            c.r[V0] = c.rd16(c.r[AT] as u32) as u64;
            c.r[AT] = c.rd16(ad(c.r[AT], 2)) as u64;
            c.r[V0] = sub32(c.r[T7], c.r[V0]);
            c.r[AT] = add32(c.r[AT], c.r[T3]);
            c.wr16(c.r[T4] as u32, c.r[V0] as u16);
            c.r[T4] = add32(c.r[T4], 2);
            if c.r[AT] == c.r[T3] {
                break;
            }
        }
    }
    // 0x45EC70
    c.wr16(c.r[T4] as u32, 0);
    c.r[T2] = SPR as u64;
    c.r[T7] = add32(c.r[T4], 0);
    c.r[T6] = add32(c.r[T2], 0);
    c.r[T8] = add32(c.r[T2], 0x1000);
    let nothing = c.r[T6] == c.r[T7];
    c.r[T4] = 0;
    if nothing {
        n[NOTHING] += 1;
        give_back(c);
        return;
    }
    // 0x45EC90: the chain's pointer (t9), the pool (t3), the textures placed (s1), the VRAM allocator (s0), the next
    // texture record (s2); the run's tag, footer and end (vf17 to vf19), the TEX0 template (vf20); the texture table to
    // scratchpad 0x3000 and the sine table to 0x3400.
    c.r[T9] = sx(c.rd32(k.gp(c, gp, 0x45_ec90)));
    c.r[T3] = sx(c.rd32(k.gp(c, gp, 0x45_ec94)));
    c.r[S1] = sx(c.rd32(k.gp(c, gp, 0x45_ec98)));
    c.r[S0] = sx(c.rd32(k.gp(c, gp, 0x45_ec9c)));
    c.r[AT] = sll(c.r[S1], 4);
    c.r[S2] = sx(k.a(c, 0x45_eca4, 0x45_eca8));
    c.r[S2] = add32(c.r[S2], c.r[AT]);
    lqc2(c, 17, k.gp(c, gp, 0x45_ecb0));
    lqc2(c, 18, k.gp(c, gp, 0x45_ecb4));
    lqc2(c, 19, k.gp(c, gp, 0x45_ecb8));
    lqc2(c, 20, k.gp(c, gp, 0x45_ecbc));
    qmfc2(c, AT, 25);
    c.r[V0] = 0x4580_0000;
    c.f[4] = c.r[AT] as u32;
    c.f[5] = c.r[V0] as u32;
    c.r[A0] = 0x1000_0000;
    c.r[AT] = sx(k.a(c, 0x45_ecd4, 0x45_ecd8));
    c.r[A0] |= 0xd400;
    dw(c, ad(c.r[A0], 0x10), c.r[AT] as u32);
    c.r[AT] = 0x200;
    dw(c, ad(c.r[A0], 0xc10), c.r[AT] as u32);
    c.r[AT] = 0x40;
    dw(c, ad(c.r[A0], 0x20), c.r[AT] as u32);
    c.r[AT] = 0x3000;
    dw(c, ad(c.r[A0], 0x80), c.r[AT] as u32);
    c.r[AT] = 0x100;
    dw(c, c.r[A0] as u32, c.r[AT] as u32);
    // 0x45ED10: BC0F's wait.
    c.r[AT] = 0x200;
    c.r[V0] = sx(k.a(c, 0x45_ed34, 0x45_ed38));
    dw(c, ad(c.r[A0], 0xc10), c.r[AT] as u32);
    dw(c, ad(c.r[A0], 0x10), c.r[V0] as u32);
    c.r[AT] = 0x40;
    dw(c, ad(c.r[A0], 0x20), c.r[AT] as u32);
    c.r[AT] = 0x3400;
    dw(c, ad(c.r[A0], 0x80), c.r[AT] as u32);
    c.r[AT] = 0x100;
    dw(c, c.r[A0] as u32, c.r[AT] as u32);
    // 0x45ED68: BC0F's wait. The chain's head at t8 (0x1000), sent at once: a REF tag (0x30000000) to program 6's upload
    // (boot's 0x101150, its quadword count from 0x101140), then the quadwords at gp-0x28B0, the camera's rows (+0x40 to
    // +0x60 times 1024, +0x70), the view's +0x220 and +0x1A0, and gp-0x28A0's tag to the program's constants.
    c.wr128(c.r[T8] as u32, 0, 0);
    c.r[V0] = sx(k.a(c, 0x45_ed8c, 0x45_ed90));
    c.r[AT] = sx(k.a(c, 0x45_ed94, 0x45_ed98));
    c.r[V0] = sx(c.rd32(c.r[V0] as u32));
    c.wr32(ad(c.r[T8], 4), c.r[AT] as u32);
    c.r[AT] = 0x3000_0000;
    c.wr32(c.r[T8] as u32, c.r[AT] as u32);
    c.r[T9] = add32(c.r[T9], 0x10);
    c.wr16(c.r[T8] as u32, c.r[V0] as u16);
    c.r[T8] = add32(c.r[T8], 0x10);
    let q = c.rd128(k.gp(c, gp, 0x45_edb8));
    s(c, AT, q);
    let q = c.rd128(k.gp(c, gp, 0x45_edbc));
    s(c, V0, q);
    c.wr128(c.r[T8] as u32, c.r[AT], c.rh[AT]);
    c.wr128(ad(c.r[T8], 0x70), c.r[V0], c.rh[V0]);
    c.r[AT] = sx(k.a(c, 0x45_edc8, 0x45_edcc));
    let q = c.rd128(ad(c.r[AT], 0x220));
    s(c, V0, q);
    let q = c.rd128(ad(c.r[AT], 0x1a0));
    s(c, V1, q);
    c.wr128(ad(c.r[T8], 0x50), c.r[V0], c.rh[V0]);
    c.wr128(ad(c.r[T8], 0x60), c.r[V1], c.rh[V1]);
    c.r[V0] = 0x4480_0000;
    c.r[AT] = sx(k.a(c, 0x45_ede4, 0x45_ede8));
    qmtc2(c, 5, V0);
    lqc2(c, 1, ad(c.r[AT], 0x40));
    lqc2(c, 2, ad(c.r[AT], 0x50));
    lqc2(c, 3, ad(c.r[AT], 0x60));
    lqc2(c, 4, ad(c.r[AT], 0x70));
    fm::<6, 15>(c, 1, 1, b4(c.vf[5][0]));
    fm::<6, 15>(c, 2, 2, b4(c.vf[5][0]));
    fm::<6, 15>(c, 3, 3, b4(c.vf[5][0]));
    sqc2(c, 1, ad(c.r[T8], 0x10));
    sqc2(c, 2, ad(c.r[T8], 0x20));
    sqc2(c, 3, ad(c.r[T8], 0x30));
    sqc2(c, 4, ad(c.r[T8], 0x40));
    c.r[T8] = add32(c.r[T8], 0x80);
    c.r[T9] = add32(c.r[T9], 0x80);
    c.r[AT] = c.r[T8] & 0x7ff;
    c.r[A0] = 0x1000_0000;
    c.r[A0] |= 0xd000;
    c.r[T8] = sub32(c.r[T8], c.r[AT]);
    kick_from(c);
    c.r[T8] ^= 0x800;
    c.r[T8] = add32(c.r[T8], 0x10);
    c.r[T9] = add32(c.r[T9], 0x10);
    c.r[T5] = c.rd16(c.r[T6] as u32) as u64;
    c.r[T5] = sll(c.r[T5], 6);
    c.r[T5] = add32(c.r[T5], c.r[T3]);
    // 0x45EE78: each sorted entry's item (t5 the entry, t6 the list, t8 the staging half, t4 the run's items); a run is sent
    // when the half reaches its 0x700 mark.
    loop {
        let end = c.r[T6] == c.r[T7];
        c.r[V0] = c.rd8(ad(c.r[T5], 1)) as u64;
        if end {
            break;
        }
        lqc2(c, 1, ad(c.r[T5], 0x10));
        c.r[AT] = add32(c.r[T5], 0);
        c.r[T6] = add32(c.r[T6], 2);
        c.r[V1] = c.r[V0] & 0x3;
        let other = c.r[V1] != 0;
        c.r[T5] = c.rd16(c.r[T6] as u32) as u64;
        let more = if !other {
            n[KIND] += 1;
            sprite(c, k, n)
        } else {
            // 0x45EFB8
            c.r[V1] = add32(c.r[V1], 0xffff_fffe);
            fm::<1, 14>(c, 1, 1, c.vf[28]);
            let two = (c.r[V1] as i64) >= 0;
            c.r[A3] = sx(c.rd32(ad(c.r[AT], 4)));
            if !two {
                n[KIND + 1] += 1;
                ground(c, k, n)
            } else {
                // 0x45F0E8
                lqc2(c, 2, ad(c.r[AT], 0x20));
                c.r[T5] = sll(c.r[T5], 6);
                c.r[A2] = sx(c.rd32(ad(c.r[AT], 0xc)));
                c.r[T5] = add32(c.r[T5], c.r[T3]);
                let ribbon = c.r[V1] != 0;
                fm::<1, 14>(c, 2, 2, c.vf[28]);
                if !ribbon {
                    n[KIND + 2] += 1;
                    line(c)
                } else {
                    n[KIND + 3] += 1;
                    band(c, k, n)
                }
            }
        };
        if !more {
            // 0x45F2B8: the run closed (footer vf18, the half's tag vf17 with its quadword count and its items' count at
            // +0x3C) and sent; t8 to the other half.
            n[RUNS] += 1;
            sqc2(c, 18, c.r[T8] as u32);
            c.r[T8] = add32(c.r[T8], 0x10);
            c.r[T9] = add32(c.r[T9], 0x10);
            c.r[AT] = c.r[T8] & 0x7ff;
            c.r[A0] = 0x1000_0000;
            c.r[T8] = sub32(c.r[T8], c.r[AT]);
            c.r[A0] |= 0xd000;
            c.r[V0] = srl(c.r[AT], 4);
            sqc2(c, 17, c.r[T8] as u32);
            c.r[V0] = add32(c.r[V0], 0xffff_ffff);
            c.wr16(c.r[T8] as u32, c.r[V0] as u16);
            c.r[V0] = add32(c.r[V0], 0xffff_ffff);
            c.wr8(ad(c.r[T8], 0xe), c.r[V0] as u8);
            c.wr32(ad(c.r[T8], 0x3c), c.r[T4] as u32);
            c.r[T4] = 0;
            poll(c);
            kick_from(c);
            c.r[T8] ^= 0x800;
            c.r[T8] = add32(c.r[T8], 0x10);
            c.r[T9] = add32(c.r[T9], 0x10);
        }
    }
    // 0x45F350: the last run closed when it has items, the chain's end (vf19), the last half sent and waited for; the
    // allocator, the textures placed and the chain's pointer kept.
    let empty = c.r[T4] == 0;
    c.r[AT] = c.r[T8] & 0x7ff;
    if !empty {
        n[RUNS] += 1;
        sqc2(c, 18, c.r[T8] as u32);
        c.r[V0] = sub32(c.r[T8], c.r[AT]);
        c.r[T8] = add32(c.r[T8], 0x10);
        c.r[T9] = add32(c.r[T9], 0x10);
        c.r[AT] = srl(c.r[AT], 4);
        c.wr32(ad(c.r[V0], 0x3c), c.r[T4] as u32);
        sqc2(c, 17, c.r[V0] as u32);
        c.wr16(c.r[V0] as u32, c.r[AT] as u16);
        c.r[AT] = add32(c.r[AT], 0xffff_ffff);
        c.wr8(ad(c.r[V0], 0xe), c.r[AT] as u8);
        c.r[T8] = add32(c.r[T8], 0x10);
        c.r[T9] = add32(c.r[T9], 0x10);
    }
    // 0x45F390
    sqc2(c, 19, ad(c.r[T8], 0xffff_fff0));
    c.r[AT] = c.r[T8] & 0x7ff;
    c.r[A0] = 0x1000_0000;
    c.r[A0] |= 0xd000;
    c.r[T8] = sub32(c.r[T8], c.r[AT]);
    poll(c);
    kick_from(c);
    c.r[T8] ^= 0x800;
    poll(c);
    c.wr32(k.gp(c, gp, 0x45_f410), c.r[S0] as u32);
    c.wr32(k.gp(c, gp, 0x45_f414), c.r[S1] as u32);
    c.wr32(k.gp(c, gp, 0x45_f418), c.r[T9] as u32);
    give_back(c);
}

/// 0x45F420: ra and s3 to s0 back from their high halves (the high halves 0).
fn give_back(c: &mut Cpu) {
    give(c, RA);
    give(c, S3);
    give(c, S2);
    give(c, S1);
    give(c, S0);
}

/// 0x45E9B0: the cull of a kind 0 or 1 entry (vf2 its position in view space, vf3.xy its size times vf25.xy): its depth
/// (vf6.z, FTOI12) within its near (a2) and far (a0, s3's limit at most) distances, its x and y within the view's sides
/// (vf26's tangents, vf27). A drawn one leaves its bucket's address in v0.
fn cull_point(c: &mut Cpu) -> bool {
    fm::<6, 14>(c, ACC, 29, b4(c.vf[2][0]));
    c.r[T0] = c.r[T1] & 0xdf;
    fm::<2, 14>(c, ACC, 30, b4(c.vf[2][1]));
    c.r[A2] = c.rd8(ad(c.r[T5], 9)) as u64;
    fm::<2, 14>(c, 2, 31, b4(c.vf[2][2]));
    c.r[A1] = 0x001d_0000;
    fm::<6, 12>(c, 4, 27, b4(c.vf[3][0]));
    c.r[A0] = c.r[A2] & 0xf0;
    ftoi12::<8>(c, 7, 3);
    c.r[A0] = sll(c.r[A0], 13);
    c.r[A2] &= 0xf;
    c.r[A1] = (c.r[A1] < c.r[A0]) as u64;
    ftoi12::<2>(c, 6, 2);
    c.r[A2] = sll(c.r[A2], 10);
    vabs::<12>(c, 3, 2);
    c.r[A1] = sll(c.r[A1], 23);
    fm::<6, 12>(c, 5, 26, b4(c.vf[2][2]));
    c.r[A0] = add32(c.r[A0], c.r[A1]);
    let v = mmi::pminw(g(c, A0), g(c, S3));
    s(c, A0, v);
    qmfc2(c, A1, 7);
    fm::<1, 12>(c, 3, 3, c.vf[4]);
    qmfc2(c, V0, 6);
    c.r[A3] = 0x3ff;
    c.r[A1] = sra(c.r[A1], 0);
    let v = mmi::pextuw(Z, g(c, V0));
    s(c, V0, v);
    fm::<1, 12>(c, 3, 5, c.vf[3]);
    c.r[V0] = sra(c.r[V0], 0);
    c.r[V1] = c.r[V0].wrapping_sub(c.r[A2]);
    c.r[A1] = c.r[V0].wrapping_sub(c.r[A1]);
    let near = (c.r[V1] as i64) <= 0;
    c.r[V1] = c.r[V0].wrapping_sub(c.r[A0]);
    if near {
        return false;
    }
    let far = (c.r[V1] as i64) >= 0;
    c.r[A1] = ((c.r[A1] as i64) < (c.r[A2] as i64)) as u64;
    if far {
        return false;
    }
    c.r[A1] &= c.r[T1];
    c.r[V0] = sra(c.r[V0], 10);
    qmfc2(c, V1, 3);
    let v = mmi::pminw(g(c, V0), g(c, A3));
    s(c, V0, v);
    let close = c.r[A1] != 0;
    c.r[V0] = sll(c.r[V0], 1);
    if close {
        return false;
    }
    let side = (c.r[V1] as i64) < 0;
    c.r[V1] <<= 32;
    if side {
        return false;
    }
    let height = (c.r[V1] as i64) < 0;
    c.r[V0] = add32(c.r[V0], c.r[T4]);
    !height
}

/// 0x45EA88: the cull of a kind 2 or 3 entry (two points less the camera, vf2 and vf4): both ends in clip space (vf20 to
/// vf23) by CLIP, the flag's last two results read before the second pair (scaled by vf24) and then both ends outside one
/// plane; the depth of the first end in view space (vf2.z, FTOI12) within its near (a0) and far (a1, s3's limit at most)
/// distances. A drawn one leaves its bucket's address in v0.
fn cull_ends(c: &mut Cpu) -> bool {
    lqc2(c, 4, ad(c.r[T5], 0x20));
    c.r[T0] = c.r[T1] & 0xdf;
    fm::<6, 15>(c, ACC, 20, b4(c.vf[2][0]));
    c.r[A0] = c.rd8(ad(c.r[T5], 9)) as u64;
    fm::<1, 14>(c, 4, 4, c.vf[28]);
    fm::<2, 15>(c, ACC, 21, b4(c.vf[2][1]));
    c.r[A1] = c.r[A0] & 0xf0;
    fm::<2, 15>(c, ACC, 22, b4(c.vf[2][2]));
    c.r[A1] = sll(c.r[A1], 13);
    fm::<2, 15>(c, 1, 23, b4(c.vf[0][3]));
    c.r[A0] &= 0xf;
    fm::<6, 15>(c, ACC, 20, b4(c.vf[4][0]));
    c.r[A0] = sll(c.r[A0], 10);
    fm::<2, 15>(c, ACC, 21, b4(c.vf[4][1]));
    fm::<2, 15>(c, ACC, 22, b4(c.vf[4][2]));
    fm::<2, 15>(c, 3, 23, b4(c.vf[0][3]));
    fm::<6, 15>(c, ACC, 2, b4(c.vf[25][2]));
    fm::<2, 15>(c, 2, 4, b4(c.vf[25][2]));
    vu0::clip(c, 1, 1);
    vu0::clip(c, 3, 3);
    fm::<6, 12>(c, 1, 1, c.vf[24]);
    fm::<6, 12>(c, 3, 3, c.vf[24]);
    fm::<6, 14>(c, ACC, 29, b4(c.vf[2][0]));
    fm::<2, 14>(c, ACC, 30, b4(c.vf[2][1]));
    fm::<2, 14>(c, 2, 31, b4(c.vf[2][2]));
    // CFC2 of the clip flag (`uyart::vu0::cfc2`: VI18 as it is).
    c.r[AT] = sx(c.vi[18]);
    vu0::clip(c, 1, 1);
    vu0::clip(c, 3, 3);
    c.r[AT] &= 0xfff;
    let out = c.r[AT] != 0;
    ftoi12::<2>(c, 2, 2);
    if out {
        return false;
    }
    let v = mmi::pminw(g(c, A1), g(c, S3));
    s(c, A1, v);
    c.r[AT] = sx(c.vi[18]);
    qmfc2(c, V1, 2);
    c.r[A3] = 0x3ff;
    c.r[V0] = srl(c.r[AT], 6);
    c.r[AT] &= 0x3f;
    let v = mmi::pextuw(Z, g(c, V1));
    s(c, V1, v);
    c.r[V1] = sra(c.r[V1], 0);
    c.r[AT] &= c.r[V0];
    let out = c.r[AT] != 0;
    c.r[A0] = c.r[V1].wrapping_sub(c.r[A0]);
    if out {
        return false;
    }
    let near = (c.r[A0] as i64) <= 0;
    c.r[A1] = c.r[V1].wrapping_sub(c.r[A1]);
    if near {
        return false;
    }
    let far = (c.r[A1] as i64) >= 0;
    c.r[V0] = sra(c.r[V1], 10);
    if far {
        return false;
    }
    let v = mmi::pminw(g(c, V0), g(c, A3));
    s(c, V0, v);
    c.r[V0] = sll(c.r[V0], 1);
    c.r[V0] = add32(c.r[V0], c.r[T4]);
    true
}

/// The texture word t0 placed when it is not yet (bit 63 clear): BGEZAL at level 1's `pc` (t0's high half cleared or not in
/// its delay slot by the caller), ra the instruction after its delay slot.
#[inline(always)]
fn place(c: &mut Cpu, k: F, pc: u32, n: &mut [u64; N]) -> bool {
    let not_yet = (c.r[T0] as i64) >= 0;
    c.r[RA] = k.pc(pc + 8) as u64;
    if not_yet {
        n[PLACED] += 1;
    }
    not_yet
}

/// 0x45EE98: a kind 0 item (4 quadwords: the position less the camera with the item's tag in w, the colour with its faded
/// alpha, the sine table's pair at its angle times Q (its size over its depth) with its ALPHA_1 value, the TEX0 word).
/// Whether the half has room after it.
fn sprite(c: &mut Cpu, k: F, n: &mut [u64; N]) -> bool {
    fm::<1, 14>(c, 1, 1, c.vf[28]);
    c.r[T1] = c.rd8(ad(c.r[AT], 2)) as u64;
    c.r[A2] = c.rd8(ad(c.r[AT], 8)) as u64;
    c.r[T5] = sll(c.r[T5], 6);
    c.r[A3] = sx(c.rd32(ad(c.r[AT], 4)));
    c.r[T5] = add32(c.r[T5], c.r[T3]);
    lqc2(c, 3, c.r[AT] as u32);
    c.r[T1] = sll(c.r[T1], 3);
    c.r[A0] = c.rd8(ad(c.r[AT], 9)) as u64;
    c.r[T1] = add32(c.r[T1], c.r[T2]);
    fm::<6, 14>(c, ACC, 29, b4(c.vf[1][0]));
    fm::<2, 14>(c, ACC, 30, b4(c.vf[1][1]));
    c.r[A2] = sll(c.r[A2], 2);
    fm::<2, 14>(c, 2, 31, b4(c.vf[1][2]));
    c.r[A1] = add32(c.r[A2], 0x300);
    fm::<6, 1>(c, 3, 3, c.vf[25]);
    c.r[A1] &= 0x3ff;
    c.r[T0] = c.rd64(ad(c.r[T1], 0x3000));
    c.r[V1] = c.r[A0] & 0xf;
    ftoi12::<2>(c, 5, 2);
    c.r[A0] &= 0xf0;
    fm::<0, 2>(c, 2, 2, c.vf[25]);
    c.r[A2] = add32(c.r[A2], c.r[T2]);
    c.r[A1] = add32(c.r[A1], c.r[T2]);
    c.r[A0] = sll(c.r[A0], 13);
    c.r[A1] = c.rd32(ad(c.r[A1], 0x3400)) as u64;
    let v = mmi::pextlb(Z, g(c, A3));
    s(c, A3, v);
    c.r[A2] = c.rd32(ad(c.r[A2], 0x3400)) as u64;
    let v = mmi::pextlh(Z, g(c, A3));
    s(c, A3, v);
    vu0::qop::<0>(c, c.vf[3][3], c.vf[2][2]);
    c.r[A1] <<= 32;
    c.r[A1] |= c.r[A2];
    c.r[A2] = c.r[V0] | 0x20;
    let was = c.r[V0] == c.r[A2];
    qmtc2(c, 4, A1);
    if !was {
        c.wr8(ad(c.r[AT], 1), c.r[A2] as u8);
    }
    // 0x45EF30
    qmfc2(c, A2, 5);
    c.r[V1] = sll(c.r[V1], 10);
    c.r[A1] = c.rd8(ad(c.r[AT], 7)) as u64;
    let v = mmi::pminw(g(c, A0), g(c, S3));
    s(c, A0, v);
    sqc2(c, 1, c.r[T8] as u32);
    let v = mmi::pextuw(Z, g(c, A2));
    s(c, A2, v);
    c.r[V1] = sub32(c.r[A2], c.r[V1]);
    c.r[A0] = sub32(c.r[A0], c.r[A2]);
    c.r[A0] = srl(c.r[A0], 4);
    c.r[A2] = 0x1000;
    c.wr128(ad(c.r[T8], 0x10), c.r[A3], c.rh[A3]);
    let v = mmi::pminw(g(c, A0), g(c, V1));
    s(c, A0, v);
    c.wr32(ad(c.r[T8], 0xc), 0);
    let v = mmi::pminw(g(c, A0), g(c, A2));
    s(c, A0, v);
    fm::<6, 12>(c, 4, 4, b4(c.vi[22]));
    mult(c, A0, A1);
    let call = place(c, k, 0x45_ef70, n);
    let v = mmi::pcpyld(Z, g(c, T0));
    s(c, T0, v);
    if call {
        f45f438(c);
    }
    c.r[A0] = c.rd8(ad(c.r[AT], 3)) as u64;
    c.r[AT] = c.r[T8] & 0x700;
    c.wr128(ad(c.r[T8], 0x30), c.r[T0], c.rh[T0]);
    c.r[V0] = 0x700;
    sqc2(c, 4, ad(c.r[T8], 0x20));
    c.r[V1] = c.lo;
    c.wr64(ad(c.r[T8], 0x28), c.r[A0]);
    c.r[T9] = add32(c.r[T9], 0x40);
    c.r[V1] = srl(c.r[V1], 12);
    c.wr32(ad(c.r[T8], 0x1c), c.r[V1] as u32);
    c.r[T8] = add32(c.r[T8], 0x40);
    let more = c.r[AT] != c.r[V0];
    c.r[T4] = add32(c.r[T4], 4);
    more
}

/// 0x45EFC8: a kind 1 item (5 quadwords: the position less the camera with the tag, the colour with its faded alpha, the
/// sine table's pair at its angle times its size, its ALPHA_1 value, the TEX0 word).
fn ground(c: &mut Cpu, k: F, n: &mut [u64; N]) -> bool {
    c.r[T1] = c.rd8(ad(c.r[AT], 2)) as u64;
    c.r[T5] = sll(c.r[T5], 6);
    c.r[A2] = c.rd8(ad(c.r[AT], 8)) as u64;
    c.r[T5] = add32(c.r[T5], c.r[T3]);
    c.f[0] = c.rd32(ad(c.r[AT], 0xc));
    c.r[T1] = sll(c.r[T1], 3);
    fm::<6, 14>(c, ACC, 29, b4(c.vf[1][0]));
    c.r[A0] = c.rd8(ad(c.r[AT], 9)) as u64;
    fm::<2, 14>(c, ACC, 30, b4(c.vf[1][1]));
    fm::<2, 14>(c, 2, 31, b4(c.vf[1][2]));
    c.r[A2] = sll(c.r[A2], 2);
    c.f[0] = fpu::mul(c.f[0], c.f[4]);
    c.r[A1] = add32(c.r[A2], 0x300);
    c.r[T1] = add32(c.r[T1], c.r[T2]);
    c.r[T0] = c.rd64(ad(c.r[T1], 0x3000));
    c.r[V1] = c.r[A0] & 0xf;
    ftoi12::<2>(c, 2, 2);
    c.r[A0] &= 0xf0;
    c.f[3] = fpu::mul(c.f[0], c.f[5]);
    c.r[A1] &= 0x3ff;
    c.r[A1] = add32(c.r[A1], c.r[T2]);
    c.r[A2] = add32(c.r[A2], c.r[T2]);
    c.f[1] = c.rd32(ad(c.r[A2], 0x3400));
    let v = mmi::pextlb(Z, g(c, A3));
    s(c, A3, v);
    c.f[2] = c.rd32(ad(c.r[A1], 0x3400));
    let v = mmi::pextlh(Z, g(c, A3));
    s(c, A3, v);
    c.f[3] = fpu::cvt_w(c.f[3]);
    c.wr128(ad(c.r[T8], 0x10), c.r[A3], c.rh[A3]);
    qmfc2(c, A2, 2);
    c.r[V1] = sll(c.r[V1], 10);
    c.r[A1] = c.rd8(ad(c.r[AT], 7)) as u64;
    c.r[A0] = sll(c.r[A0], 13);
    sqc2(c, 1, c.r[T8] as u32);
    let v = mmi::pextuw(Z, g(c, A2));
    s(c, A2, v);
    let v = mmi::pminw(g(c, A0), g(c, S3));
    s(c, A0, v);
    c.r[A3] = sx(c.f[3]);
    c.f[1] = fpu::mul(c.f[1], c.f[0]);
    c.r[V1] = sub32(c.r[A2], c.r[V1]);
    c.f[2] = fpu::mul(c.f[2], c.f[0]);
    c.r[A0] = sub32(c.r[A0], c.r[A2]);
    c.r[A2] = c.r[V0] | 0x20;
    c.r[V1] = sub32(c.r[V1], c.r[A3]);
    let was = c.r[V0] == c.r[A2];
    c.r[A0] = srl(c.r[A0], 4);
    if !was {
        c.wr8(ad(c.r[AT], 1), c.r[A2] as u8);
    }
    // 0x45F088
    c.r[A2] = 0x1000;
    c.r[A3] = 1;
    c.wr32(ad(c.r[T8], 0x20), c.f[1]);
    let v = mmi::pminw(g(c, A0), g(c, V1));
    s(c, A0, v);
    c.wr32(ad(c.r[T8], 0x24), c.f[2]);
    let v = mmi::pminw(g(c, A0), g(c, A2));
    s(c, A0, v);
    c.wr32(ad(c.r[T8], 0xc), c.r[A3] as u32);
    mult(c, A0, A1);
    let call = place(c, k, 0x45_f0a8, n);
    let v = mmi::pcpyld(Z, g(c, T0));
    s(c, T0, v);
    if call {
        f45f438(c);
    }
    c.r[A0] = c.rd8(ad(c.r[AT], 3)) as u64;
    c.r[AT] = c.r[T8] & 0x700;
    c.wr128(ad(c.r[T8], 0x30), c.r[T0], c.rh[T0]);
    c.r[V0] = 0x700;
    c.wr64(ad(c.r[T8], 0x28), c.r[A0]);
    c.r[V1] = c.lo;
    c.r[V1] = srl(c.r[V1], 12);
    c.r[T9] = add32(c.r[T9], 0x50);
    c.wr32(ad(c.r[T8], 0x1c), c.r[V1] as u32);
    c.r[T8] = add32(c.r[T8], 0x50);
    let more = c.r[AT] != c.r[V0];
    c.r[T4] = add32(c.r[T4], 5);
    more
}

/// 0x45F100: a kind 2 item (4 quadwords: both points less the camera, both colours).
fn line(c: &mut Cpu) -> bool {
    let v = mmi::pextlb(Z, g(c, A3));
    s(c, A3, v);
    let v = mmi::pextlh(Z, g(c, A3));
    s(c, A3, v);
    sqc2(c, 1, c.r[T8] as u32);
    let v = mmi::pextlb(Z, g(c, A2));
    s(c, A2, v);
    c.wr128(ad(c.r[T8], 0x10), c.r[A3], c.rh[A3]);
    let v = mmi::pextlh(Z, g(c, A2));
    s(c, A2, v);
    c.wr128(ad(c.r[T8], 0x30), c.r[A2], c.rh[A2]);
    sqc2(c, 2, ad(c.r[T8], 0x20));
    c.r[A0] = sx(0xffff_ffff);
    c.wr32(ad(c.r[T8], 0xc), c.r[A0] as u32);
    c.r[A0] = c.r[V0] | 0x20;
    let was = c.r[V0] == c.r[A0];
    c.wr32(ad(c.r[T8], 0x2c), 0);
    if !was {
        c.wr8(ad(c.r[AT], 1), c.r[A0] as u8);
    }
    // 0x45F148
    c.r[AT] = c.r[T8] & 0x700;
    c.r[V0] = 0x700;
    c.r[T9] = add32(c.r[T9], 0x40);
    c.r[T8] = add32(c.r[T8], 0x40);
    let more = c.r[AT] != c.r[V0];
    c.r[T4] = add32(c.r[T4], 4);
    more
}

/// 0x45F168: a kind 3 item (6 quadwords: both points less the camera, the side across the view (the cross product of the
/// ribbon and the eye ray, times Q from RSQRT), both colours with their faded alphas, the ALPHA_1 value and TEX0 word).
fn band(c: &mut Cpu, k: F, n: &mut [u64; N]) -> bool {
    c.r[T1] = c.rd8(ad(c.r[AT], 2)) as u64;
    let v = mmi::pextlb(Z, g(c, A3));
    s(c, A3, v);
    c.r[A0] = c.rd8(ad(c.r[AT], 7)) as u64;
    let v = mmi::pextlh(Z, g(c, A3));
    s(c, A3, v);
    fm::<6, 14>(c, ACC, 1, b4(c.vf[25][2]));
    c.r[A1] = c.rd8(ad(c.r[AT], 0xf)) as u64;
    fm::<2, 14>(c, 4, 2, b4(c.vf[25][2]));
    c.r[T1] = sll(c.r[T1], 3);
    fm::<1, 14>(c, 3, 2, c.vf[1]);
    c.r[T1] = add32(c.r[T1], c.r[T2]);
    c.r[V1] = c.rd8(ad(c.r[AT], 9)) as u64;
    let v = mmi::pextlb(Z, g(c, A2));
    s(c, A2, v);
    c.r[T0] = c.rd64(ad(c.r[T1], 0x3000));
    let v = mmi::pextlh(Z, g(c, A2));
    s(c, A2, v);
    vu0::opmula(c, 3, 4);
    vu0::opmsub(c, 3, 4, 3);
    fm::<6, 14>(c, ACC, 29, b4(c.vf[4][0]));
    fm::<2, 14>(c, ACC, 30, b4(c.vf[4][1]));
    fm::<2, 14>(c, 4, 31, b4(c.vf[4][2]));
    fm::<6, 14>(c, 5, 3, c.vf[3]);
    fm::<0, 14>(c, 6, 0, b4(c.vf[0][3]));
    let v = mmi::pcpyld(g(c, A1), g(c, A0));
    s(c, A1, v);
    ftoi12::<2>(c, 4, 4);
    c.r[A0] = c.r[V1] & 0xf0;
    fm::<0, 8>(c, ACC, 5, b4(c.vf[5][1]));
    c.r[V1] &= 0xf;
    fm::<2, 8>(c, 5, 6, b4(c.vf[5][2]));
    c.wr128(ad(c.r[T8], 0x30), c.r[A3], c.rh[A3]);
    c.r[V1] = sll(c.r[V1], 10);
    c.wr128(ad(c.r[T8], 0x40), c.r[A2], c.rh[A2]);
    c.r[A0] = sll(c.r[A0], 13);
    qmfc2(c, A2, 4);
    let v = mmi::pminw(g(c, A0), g(c, S3));
    s(c, A0, v);
    vu0::qop::<2>(c, c.vf[1][3], c.vf[5][0]);
    sqc2(c, 1, c.r[T8] as u32);
    let v = mmi::pextuw(Z, g(c, A2));
    s(c, A2, v);
    sqc2(c, 2, ad(c.r[T8], 0x10));
    c.r[V1] = sub32(c.r[A2], c.r[V1]);
    c.r[A0] = sub32(c.r[A0], c.r[A2]);
    c.r[A2] = c.r[V0] | 0x20;
    let was = c.r[V0] == c.r[A2];
    c.r[A0] = srl(c.r[A0], 4);
    if !was {
        c.wr8(ad(c.r[AT], 1), c.r[A2] as u8);
    }
    // 0x45F240
    c.r[A2] = 0x1000;
    c.r[A3] = sx(0xffff_fffe);
    let v = mmi::pminw(g(c, A0), g(c, V1));
    s(c, A0, v);
    c.r[V1] = c.rd8(ad(c.r[AT], 3)) as u64;
    let v = mmi::pminw(g(c, A0), g(c, A2));
    s(c, A0, v);
    c.wr32(ad(c.r[T8], 0xc), c.r[A3] as u32);
    let v = mmi::pcpyld(g(c, A0), g(c, A0));
    s(c, A0, v);
    c.wr64(ad(c.r[T8], 0x58), c.r[V1]);
    let call = place(c, k, 0x45_f260, n);
    let v = mmi::pmultuw(c, g(c, A0), g(c, A1));
    s(c, A0, v);
    if call {
        f45f438(c);
    }
    c.wr64(ad(c.r[T8], 0x50), c.r[T0]);
    // 0x45F26C: WAITQ, then MULq.
    fm::<6, 14>(c, 3, 3, b4(c.vi[22]));
    let v = mmi::psrlw(g(c, A0), 12);
    s(c, A0, v);
    c.wr32(ad(c.r[T8], 0x3c), c.r[A0] as u32);
    let v = mmi::pcpyud(g(c, A0), Z);
    s(c, A0, v);
    c.wr32(ad(c.r[T8], 0x4c), c.r[A0] as u32);
    c.r[AT] = c.r[T8] & 0x700;
    c.r[V0] = 0x700;
    sqc2(c, 3, ad(c.r[T8], 0x20));
    c.r[T9] = add32(c.r[T9], 0x60);
    c.wr32(ad(c.r[T8], 0x2c), 0);
    c.r[T8] = add32(c.r[T8], 0x60);
    let more = c.r[AT] != c.r[V0];
    c.r[T4] = add32(c.r[T4], 6);
    more
}

// ---- 0x45F438 -----------------------------------------------------------------------------------------------------------------

/// 0x45F438: the texture word t0 (from the scratchpad's copy of the table at 0x3000 + t1) placed: its record at s2 (fields
/// of the word, the VRAM slot s0 before and after it), s0 moved past it, s1 counted; its TEX0 word (vf20's template with the
/// word's fields and the slot put in) into the table's copy.
fn f45f438(c: &mut Cpu) {
    c.r[A2] = srl(c.r[T0], 4);
    c.r[A3] = c.r[T0] & 0xf;
    c.r[A1] = c.r[T0] >> 36;
    c.wr32(c.r[S2] as u32, c.r[A2] as u32);
    c.wr32(ad(c.r[S2], 8), c.r[A1] as u32);
    c.r[A2] = c.r[T0] >> 32;
    qmfc2(c, T0, 20);
    c.r[A2] &= 0xf;
    c.wr8(ad(c.r[S2], 0xc), c.r[A2] as u8);
    c.wr8(ad(c.r[S2], 0xd), c.r[A2] as u8);
    c.r[A2] <<= 26;
    c.r[T0] |= c.r[A2];
    c.r[A2] <<= 4;
    c.r[T0] |= c.r[A2];
    c.r[A1] = 0x400;
    c.wr16(ad(c.r[S2], 4), c.r[A3] as u16);
    c.r[A3] = sll(c.r[A3], 8);
    c.r[A1] = sub32(c.r[A1], c.r[A3]);
    c.r[A3] <<= 43;
    c.r[T0] |= c.r[A3];
    c.r[A2] = srl(c.r[S0], 8);
    c.wr16(ad(c.r[S2], 6), c.r[A2] as u16);
    c.r[S0] = add32(c.r[S0], c.r[A1]);
    c.r[A2] <<= 37;
    c.r[A3] = c.rd8(ad(c.r[S2], 0xc)) as u64;
    c.r[T0] |= c.r[A2];
    c.r[A2] = srl(c.r[S0], 8);
    c.r[T0] |= c.r[A2];
    c.wr16(ad(c.r[S2], 0xe), c.r[A2] as u16);
    c.r[A2] = 1;
    c.r[S2] = add32(c.r[S2], 0x10);
    // SLLV a2, a2 by a3 (rd = rt << rs: the translation's `(a2 as u32).wrapping_shl(a3 & 31)`), twice.
    c.r[A2] = sx((c.r[A2] as u32).wrapping_shl(c.r[A3] as u32 & 31));
    c.r[S1] = add32(c.r[S1], 1);
    c.r[A2] = sx((c.r[A2] as u32).wrapping_shl(c.r[A3] as u32 & 31));
    c.r[S0] = add32(c.r[S0], c.r[A2]);
    let ret = c.r[RA] as u32;
    c.wr64(ad(c.r[T1], 0x3000), c.r[T0]);
    c.ret = ret;
}
