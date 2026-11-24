//! The collision sweeps, done natively in front of their translation (port/uyatr `JOBS`): level 1's 0x3F8448 (coll_line: from
//! a0's point to a1's, a2 the mask, a3 a moby to leave out, t0 the caller's record for the hit ring), 0x3F9498 (coll_sphere_push:
//! the sphere of radius f12 at a0's point), 0x3FA2D8 (the same with the radii f12 and f13, called by the hero's 0x4EC868
//! alone) and
//! 0x3FB1D0 (ball_damage: the mobies a ball of radius f12 touches, into the list at 0x1EEB00), and the grid lookup 0x3F83A8 the
//! first three call (no other code calls it, in any program). The translated wrapper at each entry asks `start` (`Rt::native`
//! 8, 10, 12, 14), and `after` (9, 11, 13, 15) once the translation ran when `start` let it run; `c.ret` brings the function's
//! address with 1 in its low bit.
//!
//! The five are hand-written (128-bit registers, VU0's macro instructions, the scratchpad as their work memory) and the same
//! code in every story level and, coll_line, ball_damage and the grid lookup, the front end (uyatr finds them by size and
//! shape). What relocation changes in them is read from the code in RAM (`F`), named by level 1's address of the instruction
//! holding it: the globals' addresses (the collision state at level 1's 0x1EAA80, the hit ring 0x1EED00, ball_damage's list
//! 0x1EEB00, the table 0x201DE0, the gp words +0x37EC, the moby list, and +0x33D8), the links of the calls, and the two call
//! targets outside the unit. The rest of every word is the same in all 35 programs.
//!
//! Each function here is its code's statements in order, as level 1's translation has them (`translated/uya/l01_051`: every
//! instruction, every path), on a register file of the unit's own (the 128-bit GPRs, `r` and `h`), which goes back to the
//! thread when the entry returns. What goes away is the translated code's machinery and the PS2's traffic:
//! - the scratchpad DMA: the grid's cells come into scratchpad 0x0000 and 0x1000 in turn by SPR_TO in normal mode; its
//!   registers are stored as the port's kick stores them and the transfer is made when CHCR is written with STR, as the kick
//!   makes it (`spr_to_reg`), and its polls read CHCR, which the finished transfer left without STR (`spr_to_chcr`);
//! - the calls: the grid lookup's directly (`grid`); the two functions outside the unit through the call table, with the
//!   thread's registers as the code has them (`out`): the pose reader (level 1's 0x43C028, for a moby with joints; level 6's
//!   is another function) and 0x4388A0 (`jr ra`, at coll_sphere_push's way out at 0x3FA170);
//! - the loops' back edges: the unit has no safe point, the thread meets the field's deadline after it returns.
//! VU0's operations are the translation's (`uycpu::vu0`), the FMAC ones with `m_fmac` and the MAC and status flags the
//! translation's `fmac` makes (`fm`): the code reads the MAC flags itself (its CFC2s of VI17, which the runtime answers with
//! the register).
//!
//! So the unit leaves what the translation leaves, register for register and byte for byte (the output contract, which the
//! check compares whole): the GPRs (s0 to s7 and ra as the caller had them, their high halves 0: each entry's PCPYLDs at its
//! start and PCPYUDs at its ways out, 0x3F9438, 0x3FA118, 0x3FA144, 0x3FB1A8 and 0x3FBA90; the other registers, high halves
//! too (coll_sphere_push keeps gp+0x33D8's word and others in t8's and t9's: 0x3F9590, 0x3FA238 to 0x3FA2BC), as the code
//! leaves them; v0 the result), FPU f12 (ra's low half, 0x3FA168), VU0's registers (VF1 to VF31, ACC, Q, the MAC and status
//! flags), RAM (the collision state: +0x10 the calls' count, +0x14 the hit ring's slot, +0x18 to +0x7F the result, 0x3F9394 to
//! 0x3F93AC; the hit ring's records of 0x40 bytes, 0x3F93E8 to 0x3F9434; a moby's +0xA0, the count it was last tested at,
//! 0x3F8D30, 0x3F9C4C, 0x3FACAC, 0x3FB3C4, and its +0xA8 ring slot, 0x3F93F4, 0x3FBA70; ball_damage's list, ended by 0 at
//! 0x3FB9D0), the scratchpad (its 16 KB are the work memory: the cells, the triangles from 0x1FE0,
//! the lists at 0x3000 to 0x3FFF), SPR_TO's registers, D_STAT's bit 9 and the DMAC interrupt the kick raises, and what the pose
//! reader does (it runs as itself).
//!
//! With UYA_NATIVE_CHECK=sweep:N the first N calls of the four entries run twice from the same state: the translation, then
//! the native unit on the state as it was before; every difference in that state afterwards (`differences`: all of it, nothing
//! left out) is reported with what the calls covered, and the game goes on from the translation's state. Both runs go with the
//! thread's interrupts held, as the moby draw path's check holds them (`uyart::mobydraw`), so that the field's end and the
//! timers land after them, once. The moby draw path's jobs (`moby`, `moby2`) are its own and left to it.

use crate::{Cpu, Ext};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use uycpu::vu0::{m_fmac, m_mac, m_put};

// ---- the code in RAM ------------------------------------------------------------------------------------------------------

/// An entry of the unit as it lies in RAM: its address there and level 1's, by which this file names its instructions.
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
        assert!(a < uycpu::RAM_SIZE - 4, "the collision sweeps' code at {a:08x}: not in RAM");
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
    /// The function the JAL at `pc` calls.
    #[inline(always)]
    fn jal(self, c: &Cpu, pc: u32) -> u32 {
        (self.w(c, pc) & 0x03ff_ffff) << 2
    }
    /// This program's address of level 1's `pc` (a return address).
    #[inline(always)]
    fn pc(self, pc: u32) -> u32 {
        self.at.wrapping_add(pc.wrapping_sub(self.l1))
    }
}

/// A 32-bit result as the R5900 keeps it in 64 bits.
#[inline(always)]
fn sx(v: u32) -> u64 {
    v as i32 as i64 as u64
}

/// The entries, by level 1's address, in the order of their native numbers (8, 10, 12, 14).
const L1: [u32; 4] = [0x3f_8448, 0x3f_9498, 0x3f_a2d8, 0x3f_b1d0];
const ENTRY: [&str; 4] = ["coll_line", "coll_sphere_push", "0x3FA2D8", "ball_damage"];

// ---- VU0, the calls out, the scratchpad DMA ---------------------------------------------------------------------------------

/// An FMAC macro instruction (K: 0 ADD, 1 SUB, 2 MADD, 3 MSUB, 4 MAX, 5 MINI, 6 MUL) on lanes D into VF `dst` or ACC (32), as
/// `uycpu::vu0::fmac` makes it: the arithmetic on four lanes (`m_fmac`, under the game thread's MXCSR, `uycpu::fpu::MXCSR`),
/// the lanes outside D kept, VF0 never written, and but for MAX and MINI the MAC flags of the lanes in D into VI17 and the
/// status flag's Z and S (and their sticky bits) into VI16.
#[inline(always)]
fn fm<const K: u8, const D: u32>(c: &mut Cpu, dst: usize, fs: usize, t: [u32; 4]) {
    let v = m_fmac::<K, D>(c.vf[fs], t, c.vacc);
    if dst == 32 {
        m_put::<D>(&mut c.vacc, v);
    } else if dst != 0 {
        m_put::<D>(&mut c.vf[dst], v);
    }
    if K != 4 && K != 5 {
        let mac = m_mac::<D>(v);
        c.vi[17] = mac;
        let st = ((mac & 0xf != 0) as u32) | (((mac & 0xf0 != 0) as u32) << 1);
        c.vi[16] = (c.vi[16] & !0x3) | st | (st << 6);
    }
}

/// A call of a game function outside the unit (`t`, by the JAL at `from`) through the call table, with the thread's registers
/// as the unit's register file has them, and back with what it left; it returns to `ret` (neither the pose reader nor 0x4388A0
/// returns anywhere else).
#[inline(always)]
fn out(c: &mut Cpu, r: &mut [u64; 32], h: &mut [u64; 32], t: u32, from: u32, ret: u32) {
    c.r = *r;
    c.rh = *h;
    c.call(t, from);
    assert!(c.ret == ret, "the collision sweeps' call of {t:08x} at {from:08x} came back to {:08x}, not {ret:08x}", c.ret);
    *r = c.r;
    *h = c.rh;
    count(5, 1);
}

/// A store to SPR_TO's registers (0x1000D400 CHCR, +0x10 MADR, +0x20 QWC, +0x80 SADR), as `hw::write` stores it: kept; CHCR
/// with STR makes the transfer, as the port's kick makes it (`hw::kick`'s channel 9 in normal mode, the only mode the unit
/// writes: 0x100), and ends the channel as the kick ends it: STR and QWC cleared, D_STAT's bit 9, the DMAC interrupt raised.
/// The kick's skin recording (a transfer to 0x1C00) is not made: the unit's SADR is 0 or 0x1000 (0x70000000 or 0x70001000,
/// s7 at 0x3F8734 and its XORs).
fn spr_to_reg(c: &mut Cpu, a: u32, v: u32) {
    let reg = match a {
        0x1000_d400 => 0,
        0x1000_d410 => 1,
        0x1000_d420 => 2,
        0x1000_d480 => 8,
        _ => panic!("the collision sweeps' store of {v:#x} to {a:08x}: not one of SPR_TO's registers"),
    };
    let mp = c.mp();
    let m = c.mach();
    m.hw.ch[9][reg] = v;
    if reg != 0 || v & 0x100 == 0 {
        return;
    }
    assert!((v >> 2) & 3 == 0, "the collision sweeps' SPR_TO in mode {} (CHCR {v:#x}): the unit's transfers are normal", (v >> 2) & 3);
    let (madr, qwc) = (m.hw.ch[9][1], m.hw.ch[9][2] & 0xffff);
    let mut s = m.hw.ch[9][8] & 0x3ff0;
    put(m, &mut s, madr, qwc);
    let h = &mut m.hw.ch[9];
    h[1] = madr.wrapping_add(qwc * 16);
    h[8] = s;
    h[0] &= !0x100;
    h[2] = 0;
    m.hw.d_stat |= 1 << 9;
    crate::kernel::raise(mp, Some(c), crate::kernel::Int::Dmac(9));
    count(4, 1);
}

/// A load of SPR_TO's CHCR (0x1000D400, as `hw::read` answers it). The unit polls it for STR to clear before its next
/// transfer and before it reads the cells; every transfer is finished when CHCR's store returns (`spr_to_reg`).
fn spr_to_chcr(c: &mut Cpu, a: u32) -> u32 {
    assert!(a == 0x1000_d400, "the collision sweeps' load of {a:08x}: not SPR_TO's CHCR");
    let v = c.mach().hw.ch[9][0];
    assert!(v & 0x100 == 0, "the collision sweeps' poll of SPR_TO: a transfer under way (CHCR {v:#x})");
    v
}

/// A quadword of DMA source: RAM, or the scratchpad when bit 31 is set (`hw::qw`).
fn qw(m: &crate::Machine, a: u32) -> u128 {
    if a & 0x8000_0000 != 0 {
        let o = (a & 0x3ff0) as usize;
        u128::from_le_bytes(m.spr[o..o + 16].try_into().expect("16 bytes"))
    } else {
        let o = (a & 0x01ff_fff0) as usize;
        u128::from_le_bytes(m.ram[o..o + 16].try_into().expect("16 bytes"))
    }
}

/// `q` quadwords from `a` into the scratchpad at `*sadr`, which steps on (wrapping at 16 KB), as the kick copies them.
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

// ---- the entries ------------------------------------------------------------------------------------------------------------

/// Counts of what the native calls did: the calls of each entry, those returning other than 0, the grid's cells fetched
/// (SPR_TO transfers), the calls out (the pose reader, 0x4388A0).
static DONE: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
const NAMES: [&str; 6] = ["coll_line calls", "coll_sphere_push calls", "0x3FA2D8 calls", "ball_damage calls", "cells fetched", "calls out"];
static RETURNED: AtomicU64 = AtomicU64::new(0);

#[inline(always)]
fn count(k: usize, n: u32) {
    DONE[k].fetch_add(n as u64, Relaxed);
}

/// Entry `e` (0 coll_line, 1 coll_sphere_push, 2 0x3FA2D8, 3 ball_damage) at `at` in this program, natively, with the thread's
/// registers.
fn run(c: &mut Cpu, e: usize, at: u32) {
    let k = F { at, l1: L1[e] };
    match e {
        0 => e3f8448(c, k),
        1 => e3f9498(c, k),
        2 => e3fa2d8(c, k),
        _ => e3fb1d0(c, k),
    }
    count(e, 1);
    if c.r[2] != 0 {
        RETURNED.fetch_add(1, Relaxed);
    }
}

/// Each entry on the unit's register file: the thread's GPRs in, and back when it returns.
macro_rules! entry {
    ($name:ident, $body:ident) => {
        #[inline(never)]
        fn $name(c: &mut Cpu, k: F) {
            let (mut r, mut h) = (c.r, c.rh);
            $body(c, k, &mut r, &mut h);
            c.r = r;
            c.rh = h;
        }
    };
}
entry!(e3f8448, b3f8448);
entry!(e3f9498, b3f9498);
entry!(e3fa2d8, b3fa2d8);
entry!(e3fb1d0, b3fb1d0);

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

/// What differs between the translation's state `want` and the native unit's `got`: all of the state, every register (both
/// halves of each GPR), every byte. The unit makes the translation's statements, so nothing is left out.
fn differences(want: &State, got: &State) -> Vec<String> {
    let mut d = Vec::new();
    for (what, a, b, base) in [("RAM", &want.ram, &got.ram, 0u32), ("scratchpad", &want.spr, &got.spr, 0x7000_0000), ("VU0 data", &want.vu0d, &got.vu0d, 0), ("VU0 code", &want.vu0m, &got.vu0m, 0)] {
        let (r, n) = ranges(a, b, base, 12);
        if n > 0 {
            d.push(format!("{what}: {n} ranges {}", r.join(" ")));
        }
    }
    let regs: Vec<String> = (0..32).filter(|&k| want.r[k] != got.r[k] || want.rh[k] != got.rh[k]).map(|k| format!("r{k} {:x}:{:x}/{:x}:{:x}", want.rh[k], want.r[k], got.rh[k], got.r[k])).collect();
    if !regs.is_empty() || want.hilo != got.hilo || want.sa != got.sa {
        d.push(format!("registers: {} HI/LO {:x?}/{:x?} SA {}/{}", regs.join(" "), want.hilo, got.hilo, want.sa, got.sa));
    }
    let fregs: Vec<String> = (0..32).filter(|&k| want.f[k] != got.f[k]).map(|k| format!("f{k} {:x}/{:x}", want.f[k], got.f[k])).collect();
    if !fregs.is_empty() || want.facc != got.facc || want.fcr31 != got.fcr31 {
        d.push(format!("FPU: {} ACC {:x}/{:x} FCR31 {:x}/{:x}", fregs.join(" "), want.facc, got.facc, want.fcr31, got.fcr31));
    }
    let vf: Vec<String> = (0..32).filter(|&k| want.vf[k] != got.vf[k]).map(|k| format!("vf{k} {:x?}/{:x?}", want.vf[k], got.vf[k])).collect();
    let vi: Vec<String> = (0..32).filter(|&k| want.vi[k] != got.vi[k]).map(|k| format!("vi{k} {:x}/{:x}", want.vi[k], got.vi[k])).collect();
    if !vf.is_empty() || !vi.is_empty() || want.vacc != got.vacc {
        d.push(format!("VU0: {} {} ACC {:x?}/{:x?}", vf.join(" "), vi.join(" "), want.vacc, got.vacc));
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

struct Check {
    left: u64,
    done: u64,
    differ: u64,
    kinds: std::collections::BTreeSet<String>,
    /// The state before the call being checked, its entry and its address in this program.
    before: Option<(State, usize, u32)>,
    /// Entries of the unit called inside the translation being checked: they run their translations too.
    inner: u32,
    /// Whether the thread had its interrupts on at the checked call: both runs go with them held, and EI after the
    /// comparison takes what waited (as `mobydraw`'s check: a loop's back edge past the field's time would give the field
    /// up in the translation's run, and the timers that came due would run in whichever run reached a syscall first).
    ints: bool,
}

static CHECK: std::sync::Mutex<Option<Check>> = std::sync::Mutex::new(None);

/// UYA_NATIVE_CHECK=sweep:N, read once: N calls of the unit's entries. Another unit's job (the moby draw path's `moby`,
/// `moby2`, the shrub cull's `shrub`) leaves this unit unchecked, as mobydraw.rs does for this one's.
fn checked() -> Option<u64> {
    static C: std::sync::OnceLock<Option<u64>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_NATIVE_CHECK").ok()?;
        let (job, n) = v.split_once(':').unwrap_or_else(|| panic!("UYA_NATIVE_CHECK={v}: <job>:N (job sweep, moby, moby2 or shrub)"));
        match job {
            "sweep" => Some(n.parse().unwrap_or_else(|_| panic!("UYA_NATIVE_CHECK={v}: N is a number of calls"))),
            _ => None,
        }
    })
}

/// `Rt::native` 8, 10, 12, 14: the entry natively (`c.ret` its return address), or (checking) the state saved and `c.ret` 0
/// so the translation runs.
pub(crate) fn start(c: &mut Cpu, k: u32) {
    let e = ((k - 8) / 2) as usize;
    let at = c.ret & !3;
    if let Some(n) = checked() {
        let mut g = CHECK.lock().expect("the native check");
        let ck = g.get_or_insert(Check { left: n, done: 0, differ: 0, kinds: Default::default(), before: None, inner: 0, ints: false });
        if ck.before.is_some() {
            ck.inner += 1;
            c.ret = 0;
            return;
        }
        if ck.left > 0 {
            // Interrupts on as the kernel sees them (`kernel::K::ints_on`): EIE and IE, not in an exception.
            ck.ints = c.status & 0x1_0001 == 0x1_0001 && c.status & 6 == 0;
            if ck.ints {
                c.di();
            }
            ck.before = Some((State::take(c), e, at));
            c.ret = 0;
            return;
        }
    }
    run(c, e, at);
    c.ret = c.r[31] as u32;
}

/// `Rt::native` 9, 11, 13, 15: the translation ran; the entry runs natively from the state before it and the two are compared.
pub(crate) fn after(c: &mut Cpu, _k: u32) {
    let (Some((before, e, at)), ints) = ({
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
    let want = State::take(c);
    before.put(c);
    run(c, e, at);
    let got = State::take(c);
    let d = differences(&want, &got);
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
        if ck.differ <= 6 || ck.kinds.insert(format!("{}: {kind}", ENTRY[e])) {
            println!("UYA_NATIVE_CHECK: sweep call {} ({} at {at:08x}, ra {:08x}): {}", ck.done, ENTRY[e], want.r[31] as u32, d.join("; "));
        }
    }
    if ck.left == 0 || ck.done % 100 == 0 {
        let n: Vec<String> = DONE.iter().zip(NAMES).map(|(a, name)| format!("{name} {}", a.load(Relaxed))).collect();
        println!("UYA_NATIVE_CHECK: sweep: {} calls checked, {} differ ({}, returned not 0 {})", ck.done, ck.differ, n.join(", "), RETURNED.load(Relaxed));
    }
    drop(g);
    // The interrupts held for the two runs: what came due runs now, as at the game's own EI.
    if ints {
        c.ei();
        c.ret = ret;
    }
}

// ---- the functions, each its code's statements in order ---------------------------------------------------------------------
//
// Level 1's translation of each (uyatr's `emit`, `translated/uya/l01_051`), on the unit's register file: `r[n]` and `h[n]` the
// low and high 64 bits of GPR n. Changed from it, and nothing else: the relocated words read from the code (`k.hi`, `k.lo`,
// `k.pc`, `k.jal`: the LUI and ADDIU of each global, the gp offsets, the JALs' links and targets), the calls (`grid`, `out`),
// SPR_TO's registers (`spr_to_reg`, `spr_to_chcr`), the FMAC operations (`fm`), the CFC2s of VI17 (the register), and the
// back edges' safe points and the polls' waits (none: no transfer is under way when the unit polls).

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
#[inline(always)]
fn grid(c: &mut Cpu, r: &mut [u64; 32]) {
let mut l: u32 = 0x003f83a8;
loop { match l {
0x003f83a8 => {
r[4] = c.rd16((r[24] as u32)) as u64;
r[5] = c.rd16((r[24] as u32).wrapping_add(0x00000002)) as u64;
r[3] = (((r[3] as u32).wrapping_sub((r[4] as u32))) as i32 as i64 as u64);
let t = (r[3] as i64) < 0;  r[5] = (((r[5] as u32).wrapping_sub((r[3] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
let t = (r[5] as i64) <= 0;  r[3] = (((r[3] as u32) << 1) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
r[3] = (((r[3] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
r[3] = c.rd16((r[3] as u32).wrapping_add(0x00000004)) as u64;
r[3] = (((r[3] as u32) << 2) as i32 as i64 as u64);
let t = r[3] == 0u64;  r[3] = (((r[3] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
r[4] = c.rd16((r[3] as u32)) as u64;
r[5] = c.rd16((r[3] as u32).wrapping_add(0x00000002)) as u64;
r[2] = (((r[2] as u32).wrapping_sub((r[4] as u32))) as i32 as i64 as u64);
let t = (r[2] as i64) < 0;  r[5] = (((r[5] as u32).wrapping_sub((r[2] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
let t = (r[5] as i64) <= 0;  r[2] = (((r[2] as u32) << 2) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
r[2] = (((r[2] as u32).wrapping_add((r[3] as u32))) as i32 as i64 as u64);
r[2] = c.rd32((r[2] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
r[4] = c.rd16((r[2] as u32)) as u64;
r[5] = c.rd16((r[2] as u32).wrapping_add(0x00000002)) as u64;
r[1] = (((r[1] as u32).wrapping_sub((r[4] as u32))) as i32 as i64 as u64);
let t = (r[1] as i64) < 0;  r[5] = (((r[5] as u32).wrapping_sub((r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
let t = (r[5] as i64) <= 0;  r[1] = (((r[1] as u32) << 2) as i32 as i64 as u64);
 if t { l = 0x003f8440; continue; }
r[1] = (((r[1] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
r[1] = c.rd32((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
let t = (r[31] as u32);  c.ret = t; return;
}
0x003f8440 => {
let t = (r[31] as u32); r[1] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 c.ret = t; return;
}
_ => unreachable!("0x3f83a8: no label {l:08x} in it"), } } }

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
#[inline(always)]
fn b3f8448(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x003f8448;
loop { match l {
0x003f8448 => {
{ let (a, b) = uycpu::mmi::pcpyld((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
{ let (a, b) = c.rd128((r[4] as u32)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pcpyld((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
{ let (a, b) = c.rd128((r[5] as u32)); c.vf[29] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 1>(c, 28, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[15] = sx(k.hi(c, 0x3f845c));
r[15] = (((r[15] as u32).wrapping_add(k.lo(c, 0x3f8460))) as i32 as i64 as u64);
fm::<6, 1>(c, 29, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[1] = 0x0000000044800000;
{ let (a, b) = uycpu::mmi::pcpyld((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
r[13] = c.rd32((r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyld((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
fm::<5, 14>(c, 3, 28, c.vf[29]);
r[19] = (((r[6] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<4, 14>(c, 4, 28, c.vf[29]);
r[18] = (((r[7] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<6, 15>(c, 30, 28, uycpu::vu0::bcast(c.vf[1][0]));
r[17] = (((r[8] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<6, 15>(c, 31, 29, uycpu::vu0::bcast(c.vf[1][0]));
r[20] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<1, 14>(c, 3, 3, c.vf[0]);
r[16] = 0x0000000070000000;
{ let (a, b) = uycpu::mmi::pcpyld((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
let t = (r[13] as i64) < 0;  { let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
 if t { l = 0x003f9368; continue; }
r[1] = c.vi[17] as i32 as i64 as u64;
r[13] = (((r[13] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
fm::<1, 14>(c, 4, 4, uycpu::vu0::bcast(c.vf[1][0]));
c.wr32((r[15] as u32).wrapping_add(0x00000010), r[13] as u32);
uycpu::vu0::ftoi::<0, 15>(c, 28, 30);
r[1] = r[1] & 0xe0;
let t = r[1] != 0u64;  r[13] = r[13] << 32;
 if t { l = 0x003f9368; continue; }
r[24] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[19] = r[19] | r[13];
uycpu::vu0::ftoi::<0, 15>(c, 29, 31);
r[14] = 0x0000000010000000;
r[2] = 0u64 | 0xe0;
r[1] = c.vi[17] as i32 as i64 as u64;
r[4] = r[19] & 0x1;
{ let (a, b) = { let v = c.vf[28]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[12] = a; h[12] = b; }
r[22] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[1] = r[1] & 0xe0;
fm::<1, 14>(c, 13, 31, c.vf[30]);
let t = r[1] != r[2];  { let (a, b) = uycpu::mmi::psrlw((r[12], h[12]), 12); r[12] = a; h[12] = b; }
 if t { l = 0x003f9368; continue; }
{ let (a, b) = { let v = c.vf[29]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[13] = a; h[13] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[12], h[12])); r[9] = a; h[9] = b; }
c.wr32((r[16] as u32).wrapping_add(0x00003800), 0u64 as u32);
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[9], h[9])); r[9] = a; h[9] = b; }
c.wr32((r[16] as u32).wrapping_add(0x00003804), r[9] as u32);
{ let (a, b) = uycpu::mmi::psrlw((r[13], h[13]), 12); r[13] = a; h[13] = b; }
let t = (r[4] as i64) > 0;  r[8] = (((r[16] as u32).wrapping_add(0x00003ffc)) as i32 as i64 as u64);
 if t { l = 0x003f8588; continue; }
r[1] = r[12] & 0xffff;
r[2] = r[12] >> 32;
r[31] = k.pc(0x3f8558) as u64; { let (a, b) = uycpu::mmi::pcpyud((r[12], h[12]), (0u64, 0u64)); r[3] = a; h[3] = b; }
 grid(c, r);
let t = r[1] == 0u64;  c.wr64((r[16] as u32), 0u64);
 if t { l = 0x003f8588; continue; }
r[2] = r[1] & 0xff;
r[14] = r[14] | 0xd400;
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000020), r[2] as u32);
r[1] = (((r[1] as u32) >> 8) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000080), 0u64 as u32);
r[1] = (((r[1] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000010), r[1] as u32);
r[1] = 0u64 | 0x100;
spr_to_reg(c, (r[14] as u32), r[1] as u32);
l = 0x003f8588; continue; }
0x003f8588 => {
{ let (a, b) = uycpu::mmi::psubw((r[12], h[12]), (r[13], h[13])); r[10] = a; h[10] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[10], h[10])); r[10] = a; h[10] = b; }
let t = r[10] == 0u64;  r[11] = r[16] | 0x3000;
 if t { l = 0x003f86f8; continue; }
fm::<1, 14>(c, 1, 31, c.vf[30]);
r[14] = ((0u32.wrapping_add(0x00000003)) as i32 as i64 as u64);
uycpu::vu0::mov::<14>(c, 2, c.vf[30]);
r[15] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003f85a8; continue; }
0x003f85a8 => {
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[1][0]);
r[14] = (((r[14] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[1] = (((((r[12] as u32) as i32) >> 0) as u32) as i32 as i64 as u64);
r[2] = (((((r[13] as u32) as i32) >> 0) as u32) as i32 as i64 as u64);
r[4] = (((r[11] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[3] = (((r[2] as u32).wrapping_sub((r[1] as u32))) as i32 as i64 as u64);
let t = (r[3] as i64) >= 0;  r[3] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003f86a0; continue; }
r[1] = (((r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[3] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
 l = 0x003f86a0; continue;
}
0x003f85e0 => {
{ let v = c.vf[1]; uycpu::vu0::mov::<15>(c, 1, [v[1], v[2], v[3], v[0]]); }
c.wr8((r[8] as u32), r[3] as u8);
{ let v = c.vf[2]; uycpu::vu0::mov::<15>(c, 2, [v[1], v[2], v[3], v[0]]); }
r[8] = (((r[8] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::prot3w((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::prot3w((r[13], h[13])); r[13] = a; h[13] = b; }
let t = r[14] != 0u64;  r[11] = (((r[11] as u32).wrapping_add(0x00000200)) as i32 as i64 as u64);
 if t { l = 0x003f85a8; continue; }
r[10] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
r[6] = c.rd8((r[16] as u32).wrapping_add(0x00003ffc)) as i8 as i64 as u64;
r[11] = (((r[16] as u32).wrapping_add(0x00003000)) as i32 as i64 as u64);
r[7] = c.rd8((r[16] as u32).wrapping_add(0x00003ffd)) as i8 as i64 as u64;
r[12] = (((r[16] as u32).wrapping_add(0x00003200)) as i32 as i64 as u64);
r[8] = c.rd8((r[16] as u32).wrapping_add(0x00003ffe)) as i8 as i64 as u64;
r[13] = (((r[16] as u32).wrapping_add(0x00003400)) as i32 as i64 as u64);
r[7] = (((r[7] as u32) << 8) as i32 as i64 as u64);
r[8] = (((r[8] as u32) << 16) as i32 as i64 as u64);
l = 0x003f8628; continue; }
0x003f8628 => {
let t = (r[15] as i64) <= 0;  r[1] = c.rd32((r[11] as u32)) as i32 as i64 as u64;
 if t { l = 0x003f8718; continue; }
r[2] = c.rd32((r[12] as u32)) as i32 as i64 as u64;
r[10] = (((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[3] = c.rd32((r[13] as u32)) as i32 as i64 as u64;
r[15] = (((r[15] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[4] = (((r[2] as u32).wrapping_sub((r[1] as u32))) as i32 as i64 as u64);
let t = (r[4] as i64) < 0;  r[4] = (((r[3] as u32).wrapping_sub((r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8668; continue; }
let t = (r[4] as i64) < 0;  c.wr32((r[10] as u32), r[1] as u32);
 if t { l = 0x003f8688; continue; }
r[9] = (((r[9] as u32).wrapping_add((r[6] as u32))) as i32 as i64 as u64);
r[11] = (((r[11] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.wr32((r[10] as u32).wrapping_add(0x00000004), r[9] as u32);
 l = 0x003f8628; continue;
}
0x003f8668 => {
r[4] = (((r[3] as u32).wrapping_sub((r[2] as u32))) as i32 as i64 as u64);
let t = (r[4] as i64) < 0;  c.wr32((r[10] as u32), r[2] as u32);
 if t { l = 0x003f8688; continue; }
r[9] = (((r[9] as u32).wrapping_add((r[7] as u32))) as i32 as i64 as u64);
r[12] = (((r[12] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
c.wr32((r[10] as u32).wrapping_add(0x00000004), r[9] as u32);
 l = 0x003f8628; continue;
}
0x003f8688 => {
c.wr32((r[10] as u32), r[3] as u32);
r[9] = (((r[9] as u32).wrapping_add((r[8] as u32))) as i32 as i64 as u64);
c.wr32((r[10] as u32).wrapping_add(0x00000004), r[9] as u32);
r[13] = (((r[13] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 l = 0x003f8628; continue;
}
0x003f86a0 => {
let t = r[1] == r[2];  r[1] = (((r[1] as u32).wrapping_add((r[3] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f86e8; continue; }
l = 0x003f86a8; continue; }
0x003f86a8 => {
r[5] = (((r[1] as u32) << 12) as i32 as i64 as u64);
r[6] = 0x000000003f800000;
c.vf[3] = [r[5] as u32, (r[5] >> 32) as u32, h[5] as u32, (h[5] >> 32) as u32];
uycpu::vu0::itof::<0, 8>(c, 3, 3);
fm::<1, 8>(c, 3, 3, c.vf[2]);
fm::<6, 8>(c, 3, 3, uycpu::vu0::bcast(c.vi[22]));
r[15] = (((r[15] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[5] = a; h[5] = b; }
{ let (a, b) = uycpu::mmi::pminw((r[5], h[5]), (r[6], h[6])); r[5] = a; h[5] = b; }
{ let (a, b) = uycpu::mmi::pmaxw((r[5], h[5]), (0u64, 0u64)); r[5] = a; h[5] = b; }
c.wr32((r[4] as u32), r[5] as u32);
r[4] = (((r[4] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = r[1] != r[2];  r[1] = (((r[1] as u32).wrapping_add((r[3] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f86a8; continue; }
l = 0x003f86e8; continue; }
0x003f86e8 => {
r[1] = 0x0000000050000000;
c.wr32((r[4] as u32), r[1] as u32);
 l = 0x003f85e0; continue;
}
0x003f86f8 => {
{ let (a, b) = { let v = c.vf[28]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[10] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
{ let (a, b) = { let v = c.vf[29]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
{ let (a, b) = uycpu::mmi::psubw((r[1], h[1]), (r[2], h[2])); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::prot3w((r[1], h[1])); r[2] = a; h[2] = b; }
r[1] = r[1] | r[2];
let t = r[1] == 0u64;  r[14] = 0x0000000010010000;
 if t { l = 0x003f9348; continue; }
l = 0x003f8718; continue; }
0x003f8718 => {
fm::<0, 14>(c, 12, 0, uycpu::vu0::bcast(c.vf[0][3]));
r[1] = 0x000000003f800000;
fm::<0, 8>(c, 24, 0, uycpu::vu0::bcast(c.vf[0][3]));
c.wr64((r[10] as u32).wrapping_add(0x00000008), r[1]);
r[1] = r[19] & 0x1;
r[25] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
let t = r[1] != 0u64;  r[23] = (((r[16] as u32).wrapping_add(0x00001000)) as i32 as i64 as u64);
 if t { l = 0x003f8bb0; continue; }
l = 0x003f8738; continue; }
0x003f8738 => {
r[1] = 0x000000003f800000;
r[10] = c.rd32((r[25] as u32)) as i32 as i64 as u64;
let t = r[1] == r[10];  r[11] = c.rd32((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
 if t { l = 0x003f8bb0; continue; }
let t = r[22] != 0u64;  r[12] = c.rd32((r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
 if t { l = 0x003f8b78; continue; }
l = 0x003f8750; continue; }
0x003f8750 => {
r[3] = c.rd32((r[25] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[23] = r[23] ^ 0x1000;
r[14] = 0x0000000010000000;
r[14] = r[14] | 0xd400;
r[25] = (((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
l = 0x003f8768; continue; }
0x003f8768 => {
r[2] = spr_to_chcr(c, (r[14] as u32)) as i32 as i64 as u64;
r[2] = r[2] & 0x100;
c.vf[10] = [r[10] as u32, (r[10] >> 32) as u32, h[10] as u32, (h[10] >> 32) as u32];
c.vf[11] = [r[11] as u32, (r[11] >> 32) as u32, h[11] as u32, (h[11] >> 32) as u32];
let t = r[2] != 0u64;   if t { l = 0x003f8768; continue; }
let t = r[1] == r[11];  r[1] = r[3] & 0xff;
 if t { l = 0x003f87d0; continue; }
r[2] = (((r[3] as u32) >> 8) as i32 as i64 as u64);
r[3] = (((r[3] as u32) >> 16) as i32 as i64 as u64);
r[31] = k.pc(0x3f87a0) as u64; r[2] = r[2] & 0xff;
 grid(c, r);
r[2] = r[1] & 0xff;
r[3] = r[23] ^ 0x1000;
let t = r[1] == 0u64;  c.wr64((r[3] as u32), 0u64);
 if t { l = 0x003f87d0; continue; }
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000020), r[2] as u32);
r[1] = (((r[1] as u32) >> 8) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000080), r[3] as u32);
r[1] = (((r[1] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000010), r[1] as u32);
r[1] = 0u64 | 0x100;
spr_to_reg(c, (r[14] as u32), r[1] as u32);
l = 0x003f87d0; continue; }
0x003f87d0 => {
r[15] = c.rd8((r[23] as u32).wrapping_add(0x00000002)) as u64;
fm::<6, 14>(c, 32, 13, uycpu::vu0::bcast(c.vf[10][0]));
r[14] = (((r[23] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
fm::<2, 14>(c, 10, 30, uycpu::vu0::bcast(c.vf[0][3]));
let t = r[15] == 0u64;  fm::<6, 14>(c, 32, 13, uycpu::vu0::bcast(c.vf[11][0]));
 if t { l = 0x003f8738; continue; }
fm::<2, 14>(c, 11, 30, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::ftoi::<0, 14>(c, 1, 10);
r[8] = 0x0000000000010000;
uycpu::vu0::ftoi::<0, 14>(c, 2, 11);
r[8] = r[8] | 0x101;
{ let (a, b) = uycpu::mmi::pextlb((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pextlh((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psllw((r[8], h[8]), 11); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psllw((r[8], h[8]), 15); r[10] = a; h[10] = b; }
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlb((0u64, 0u64), (r[12], h[12])); r[3] = a; h[3] = b; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
{ let (a, b) = uycpu::mmi::pextlh((0u64, 0u64), (r[3], h[3])); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::psllw((r[3], h[3]), 12); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::paddw((r[3], h[3]), (r[9], h[9])); r[3] = a; h[3] = b; }
c.vf[16] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[10], h[10])); r[10] = a; h[10] = b; }
{ let (a, b) = uycpu::mmi::psubw((r[1], h[1]), (r[3], h[3])); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::psubw((r[2], h[2]), (r[3], h[3])); r[2] = a; h[2] = b; }
uycpu::vu0::itof::<0, 14>(c, 16, 16);
r[13] = (((r[16] as u32).wrapping_add(0x00001fe0)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pminw((r[1], h[1]), (r[2], h[2])); r[11] = a; h[11] = b; }
{ let (a, b) = uycpu::mmi::pmaxw((r[1], h[1]), (r[2], h[2])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::psubw((r[11], h[11]), (r[8], h[8])); r[11] = a; h[11] = b; }
{ let (a, b) = uycpu::mmi::paddw((r[12], h[12]), (r[8], h[8])); r[12] = a; h[12] = b; }
fm::<1, 14>(c, 14, 30, c.vf[16]);
fm::<1, 14>(c, 15, 31, c.vf[16]);
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[11], h[11])); r[11] = a; h[11] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[12], h[12])); r[12] = a; h[12] = b; }
r[1] = c.rd32((r[14] as u32)) as i32 as i64 as u64;
r[7] = 0x00000000000f0000;
r[2] = c.rd32((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[7] = r[7] | 0xfc00;
l = 0x003f8870; continue; }
0x003f8870 => {
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[14] = (((r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[3] = r[1] & 0x3ff;
r[5] = r[2] & 0x3ff;
r[4] = r[1] & r[7];
r[6] = r[2] & r[7];
r[3] = (((r[3] as u32) << 6) as i32 as i64 as u64);
r[5] = (((r[5] as u32) << 6) as i32 as i64 as u64);
r[4] = r[4] << 12;
r[6] = r[6] << 12;
r[1] = (((r[1] as u32) >> 20) as i32 as i64 as u64);
r[2] = (((r[2] as u32) >> 20) as i32 as i64 as u64);
r[1] = r[1] << 36;
r[2] = r[2] << 36;
r[3] = r[3] | r[4];
r[5] = r[5] | r[6];
r[1] = r[1] | r[3];
r[2] = r[2] | r[5];
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[4] = a; h[4] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[3], h[3]), 16); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[4], h[4]), 16); r[4] = a; h[4] = b; }
{ let (a, b) = uycpu::mmi::psubh((r[12], h[12]), (r[1], h[1])); r[5] = a; h[5] = b; }
c.vf[1] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psubh((r[12], h[12]), (r[2], h[2])); r[6] = a; h[6] = b; }
c.vf[2] = [r[4] as u32, (r[4] >> 32) as u32, h[4] as u32, (h[4] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psubh((r[1], h[1]), (r[11], h[11])); r[3] = a; h[3] = b; }
r[1] = c.rd32((r[14] as u32)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psubh((r[2], h[2]), (r[11], h[11])); r[4] = a; h[4] = b; }
r[2] = c.rd32((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
uycpu::vu0::itof::<0, 14>(c, 1, 1);
r[3] = r[3] & r[10];
uycpu::vu0::itof::<0, 14>(c, 2, 2);
r[4] = r[4] & r[10];
r[5] = r[5] & r[10];
r[6] = r[6] & r[10];
r[3] = r[3] >> 15;
r[4] = r[4] >> 15;
r[5] = r[5] >> 14;
r[6] = r[6] >> 14;
r[3] = r[3] | r[5];
r[4] = r[4] | r[6];
{ let v = c.vf[1]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[3], h[3])); r[3] = a; h[3] = b; }
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[4], h[4])); r[4] = a; h[4] = b; }
c.wr32((r[13] as u32).wrapping_add(0x0000000c), r[3] as u32);
r[15] = (((r[15] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[15] as i64) > 0;  c.wr32((r[13] as u32).wrapping_add(0x0000001c), r[4] as u32);
 if t { l = 0x003f8870; continue; }
r[1] = c.rd8((r[23] as u32).wrapping_add(0x00000002)) as u64;
r[13] = (((r[16] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
r[14] = c.rd8((r[23] as u32)) as u64;
r[15] = (((r[23] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
r[1] = (((r[1] as u32) << 2) as i32 as i64 as u64);
r[11] = 0x0000000000020000;
r[15] = (((r[15] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[14] = (((r[14] as u32) << 2) as i32 as i64 as u64);
r[5] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[11] = r[11] | 0x202;
r[12] = 0u64 | 0x700;
{ let (a, b) = uycpu::mmi::pcpyh((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
r[10] = (((r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003f8978; continue; }
0x003f8978 => {
let t = r[15] == r[14];  { let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (r[5], h[5])); r[1] = a; h[1] = b; }
 if t { l = 0x003f8b20; continue; }
{ let (a, b) = uycpu::mmi::psllw((r[1], h[1]), 4); r[1] = a; h[1] = b; }
r[6] = c.rd32((r[1] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[2] = r[1] >> 32;
r[7] = c.rd32((r[2] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[1], h[1]), (0u64, 0u64)); r[3] = a; h[3] = b; }
r[8] = c.rd32((r[3] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[4] = r[5] >> 48;
r[5] = c.rd32((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[6] = r[6] & r[7];
r[6] = r[6] & r[8];
r[15] = (((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = (r[6] as i64) > 0;  { let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
 if t { l = 0x003f8978; continue; }
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[19] & 0x80;
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[6] & r[4];
let t = (r[6] as i64) > 0;  { let (a, b) = c.rd128((r[3] as u32)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x003f8978; continue; }
fm::<1, 14>(c, 2, 2, c.vf[1]);
r[6] = r[19] & 0x20;
fm::<1, 14>(c, 3, 3, c.vf[1]);
r[6] = ((r[6] as i64) < 1) as u64;
fm::<1, 14>(c, 4, 1, c.vf[14]);
r[7] = (((r[19] as u32) >> 8) as i32 as i64 as u64);
fm::<1, 14>(c, 5, 1, c.vf[15]);
r[7] = r[7] ^ r[4];
uycpu::vu0::opmula(c, 3, 2);
r[7] = r[7] & 0x1f;
uycpu::vu0::opmsub(c, 6, 2, 3);
r[6] = r[6] | r[7];
let t = r[6] == 0u64;  fm::<6, 14>(c, 9, 13, c.vf[6]);
 if t { l = 0x003f8978; continue; }
fm::<6, 14>(c, 7, 4, c.vf[6]);
fm::<6, 14>(c, 8, 5, c.vf[6]);
fm::<1, 14>(c, 10, 14, c.vf[1]);
fm::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
fm::<2, 8>(c, 9, 12, uycpu::vu0::bcast(c.vf[9][2]));
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 12, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 8, 12, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::qop::<0>(c, c.vf[7][0], c.vf[9][0]);
r[9] = r[19] & 0x10;
fm::<6, 14>(c, 32, 10, uycpu::vu0::bcast(c.vf[0][3]));
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
r[7] = r[7] << 32;
r[8] = r[8] << 32;
let t = (r[9] as i64) > 0;  r[6] = r[7] ^ r[8];
 if t { l = 0x003f8a68; continue; }
let t = (r[7] as i64) >= 0;   if t { l = 0x003f8978; continue; }
let t = (r[8] as i64) <= 0;   if t { l = 0x003f8978; continue; }
l = 0x003f8a68; continue; }
0x003f8a68 => {
let t = (r[6] as i64) >= 0;   if t { l = 0x003f8978; continue; }
fm::<2, 14>(c, 4, 13, uycpu::vu0::bcast(c.vi[22]));
fm::<0, 14>(c, 1, 1, c.vf[16]);
fm::<1, 14>(c, 9, 3, c.vf[2]);
fm::<1, 14>(c, 10, 4, c.vf[2]);
uycpu::vu0::opmula(c, 4, 2);
uycpu::vu0::opmsub(c, 7, 2, 4);
uycpu::vu0::opmula(c, 3, 4);
uycpu::vu0::opmsub(c, 8, 4, 3);
uycpu::vu0::opmula(c, 10, 9);
uycpu::vu0::opmsub(c, 9, 9, 10);
fm::<6, 14>(c, 7, 7, c.vf[6]);
fm::<6, 14>(c, 8, 8, c.vf[6]);
fm::<6, 14>(c, 9, 9, c.vf[6]);
fm::<1, 8>(c, 10, 24, uycpu::vu0::bcast(c.vi[22]));
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 12, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 8, 12, uycpu::vu0::bcast(c.vf[8][2]));
fm::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
fm::<2, 8>(c, 9, 12, uycpu::vu0::bcast(c.vf[9][2]));
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = { let v = c.vf[10]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
r[7] = r[7] << 32;
r[8] = r[8] << 32;
let t = (r[7] as i64) < 0;  r[9] = r[9] << 32;
 if t { l = 0x003f8978; continue; }
let t = (r[8] as i64) < 0;  r[6] = r[6] << 32;
 if t { l = 0x003f8978; continue; }
let t = (r[9] as i64) < 0;   if t { l = 0x003f8978; continue; }
let t = (r[6] as i64) <= 0;   if t { l = 0x003f8978; continue; }
r[22] = (((r[4] as u32).wrapping_add(0x00001000)) as i32 as i64 as u64);
r[21] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<0, 8>(c, 24, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<0, 14>(c, 25, 4, c.vf[1]);
fm::<0, 14>(c, 26, 6, c.vf[0]);
fm::<0, 14>(c, 27, 0, c.vf[1]);
fm::<0, 14>(c, 28, 2, c.vf[1]);
fm::<0, 14>(c, 29, 3, c.vf[1]);
 l = 0x003f8978; continue;
}
0x003f8b20 => {
let t = r[20] != 0u64;   if t { l = 0x003f8f60; continue; }
let t = r[10] == 0u64;  r[3] = c.rd8((r[23] as u32).wrapping_add(0x00000003)) as u64;
 if t { l = 0x003f8738; continue; }
r[1] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[2] = (((r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = r[3] == 0u64;  r[14] = (((r[3] as u32) << 2) as i32 as i64 as u64);
 if t { l = 0x003f8738; continue; }
l = 0x003f8b40; continue; }
0x003f8b40 => {
r[4] = c.rd8((r[1] as u32).wrapping_add(0x00000002)) as u64;
r[3] = (((r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[5] = c.rd8((r[2] as u32)) as u64;
r[2] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr8((r[1] as u32).wrapping_add(0x00000001), r[4] as u8);
c.wr8((r[1] as u32).wrapping_add(0x00000002), r[5] as u8);
let t = r[3] != 0u64;  r[1] = (((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 if t { l = 0x003f8b40; continue; }
r[5] = c.rd32((r[10] as u32)) as i32 as i64 as u64;
r[15] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[10] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
 l = 0x003f8978; continue;
}
0x003f8b78 => {
{ let (a, b) = { let v = c.vf[24]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
r[3] = r[10] << 32;
r[2] = r[2] << 32;
r[2] = r[2].wrapping_sub(r[3]);
let t = (r[2] as i64) > 0;  r[14] = 0x0000000010010000;
 if t { l = 0x003f8750; continue; }
l = 0x003f8b90; continue; }
0x003f8b90 => {
r[2] = spr_to_chcr(c, (r[14] as u32).wrapping_add(0xffffd400)) as i32 as i64 as u64;
r[2] = r[2] & 0x100;
let t = r[2] != 0u64;   if t { l = 0x003f8b90; continue; }
l = 0x003f8bb0; continue; }
0x003f8bb0 => {
r[15] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x3f8bb0))) as i32 as i64 as u64;
fm::<6, 14>(c, 3, 13, c.vf[13]);
r[3] = c.rd16((r[16] as u32).wrapping_add(0x00003804)) as u64;
r[1] = (((r[16] as u32).wrapping_add(0x00003808)) as i32 as i64 as u64);
r[2] = (((r[16] as u32).wrapping_add(0x00003804)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlb((0u64, 0u64), (r[3], h[3])); r[3] = a; h[3] = b; }
r[21] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<0, 8>(c, 32, 3, uycpu::vu0::bcast(c.vf[3][1]));
fm::<2, 8>(c, 3, 12, uycpu::vu0::bcast(c.vf[3][2]));
{ let (a, b) = uycpu::mmi::psrah((r[3], h[3]), 2); r[3] = a; h[3] = b; }
r[5] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[3][0]);
c.wr32((r[16] as u32).wrapping_add(0x00003800), r[3] as u32);
l = 0x003f8be8; continue; }
0x003f8be8 => {
r[4] = c.rd16((r[1] as u32).wrapping_add(0x00000004)) as u64;
r[6] = 0x000000003f800000;
let t = r[5] == r[6];  r[1] = (((r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
 if t { l = 0x003f8c18; continue; }
{ let (a, b) = uycpu::mmi::pextlb((0u64, 0u64), (r[4], h[4])); r[4] = a; h[4] = b; }
{ let (a, b) = uycpu::mmi::psrah((r[4], h[4]), 2); r[4] = a; h[4] = b; }
let t = r[3] == r[4];  r[5] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
 if t { l = 0x003f8be8; continue; }
c.wr32((r[2] as u32), r[4] as u32);
r[3] = r[4] | 0u64;
r[2] = (((r[2] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 l = 0x003f8be8; continue;
}
0x003f8c18 => {
r[25] = (((r[16] as u32).wrapping_add(0x000037fc)) as i32 as i64 as u64);
r[1] = ((0u32.wrapping_add(0xffffffff)) as i32 as i64 as u64);
fm::<6, 14>(c, 21, 13, uycpu::vu0::bcast(c.vi[22]));
c.wr32((r[2] as u32), r[1] as u32);
{ let (a, b) = uycpu::mmi::pcpyld((r[15], h[15]), (0u64, 0u64)); r[15] = a; h[15] = b; }
l = 0x003f8c30; continue; }
0x003f8c30 => {
r[3] = c.rd32((r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[23] = sx(k.hi(c, 0x3f8c34));
r[23] = (((r[23] as u32).wrapping_add(k.lo(c, 0x3f8c38))) as i32 as i64 as u64);
r[25] = (((r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
r[2] = (((r[3] as u32) >> 8) as i32 as i64 as u64);
r[1] = r[3] & 0xffff;
r[2] = (((r[2] as u32).wrapping_add((r[23] as u32))) as i32 as i64 as u64);
r[1] = (((r[1] as u32) << 2) as i32 as i64 as u64);
let t = (r[3] as i64) < 0;  r[3] = (((r[2] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f9368; continue; }
r[1] = c.rd16((r[3] as u32)) as u64;
r[23] = (((r[23] as u32).wrapping_add(0x00004000)) as i32 as i64 as u64);
r[2] = c.rd8((r[3] as u32).wrapping_add(0x00000002)) as u64;
r[1] = (((r[1] as u32) << 5) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 1) as i32 as i64 as u64);
r[23] = (((r[23] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add((r[23] as u32))) as i32 as i64 as u64);
c.wr32((r[25] as u32), r[2] as u32);
l = 0x003f8c78; continue; }
0x003f8c78 => {
r[24] = c.rd16((r[23] as u32)) as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[15], h[15]), (0u64, 0u64)); r[2] = a; h[2] = b; }
r[1] = c.rd32((r[25] as u32)) as i32 as i64 as u64;
r[24] = (((r[24] as u32) << 8) as i32 as i64 as u64);
let t = r[23] == r[1];  r[24] = (((r[24] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8c30; continue; }
{ let (a, b) = c.rd128((r[24] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[23] = (((r[23] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
let t = r[24] == r[18];  r[1] = r[19] & 0x9;
 if t { l = 0x003f8c78; continue; }
fm::<1, 14>(c, 2, 1, c.vf[30]);
fm::<6, 1>(c, 3, 1, c.vf[1]);
r[2] = c.rd16((r[24] as u32).wrapping_add(0x00000034)) as u64;
r[1] = ((r[1] as i64) < 1) as u64;
r[20] = c.rd32((r[24] as u32).wrapping_add(0x00000098)) as i32 as i64 as u64;
r[2] = r[2] & 0x4000;
fm::<6, 14>(c, 2, 2, c.vf[21]);
r[1] = r[1] | r[2];
fm::<0, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[2][1]));
let t = r[1] == 0u64;  fm::<2, 8>(c, 2, 12, uycpu::vu0::bcast(c.vf[2][2]));
 if t { l = 0x003f8c78; continue; }
fm::<4, 8>(c, 2, 2, c.vf[0]);
fm::<5, 8>(c, 2, 2, c.vf[24]);
fm::<0, 14>(c, 32, 30, c.vf[0]);
fm::<2, 14>(c, 2, 13, uycpu::vu0::bcast(c.vf[2][0]));
fm::<1, 14>(c, 2, 2, c.vf[1]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
fm::<1, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[3][3]));
fm::<2, 8>(c, 32, 12, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 12, uycpu::vu0::bcast(c.vf[2][2]));
r[5] = r[19] & 0x4;
r[5] = ((r[5] as i64) < 1) as u64;
r[10] = sx(k.hi(c, 0x3f8cfc));
r[10] = (((r[10] as u32).wrapping_add(k.lo(c, 0x3f8d00))) as i32 as i64 as u64);
r[5] = (((r[5] as u32) << 1) as i32 as i64 as u64);
let t = r[20] == 0u64;  r[5] = (((r[5] as u32).wrapping_add((r[20] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f8c78; continue; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[7] = r[19] >> 32;
r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000a0)) as i32 as i64 as u64;
r[1] = r[1] << 32;
let t = (r[1] as i64) > 0;   if t { l = 0x003f8c78; continue; }
c.wr32((r[24] as u32).wrapping_add(0x000000a0), r[7] as u32);
let t = r[3] == r[7];  r[5] = c.rd16((r[5] as u32)) as u64;
 if t { l = 0x003f8c78; continue; }
r[2] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[1] = r[19] & 0x2;
let t = r[5] == 0u64;  r[1] = ((r[1] as i64) < 1) as u64;
 if t { l = 0x003f8dd0; continue; }
let t = r[1] == r[2];  r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000ac)) as i32 as i64 as u64;
 if t { l = 0x003f8c78; continue; }
r[1] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[2] = ((0u32.wrapping_add(0x00000008)) as i32 as i64 as u64);
r[16] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
l = 0x003f8d68; continue; }
0x003f8d68 => {
r[6] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
r[1] = (((r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[6] == r[3];   if t { l = 0x003f8dd0; continue; }
let t = (r[2] as i64) > 0;  r[16] = (((r[16] as u32).wrapping_add(0x00000800)) as i32 as i64 as u64);
 if t { l = 0x003f8d68; continue; }
r[2] = c.rd32((r[10] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[6] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[1] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[4] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[1] = r[1] & 0x7;
c.wr32((r[10] as u32).wrapping_add(0x0000000c), r[1] as u32);
r[1] = (((r[2] as u32) << 3) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 11) as i32 as i64 as u64);
r[4] = (((r[4] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[6] = (((r[6] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
c.wr32((r[4] as u32), r[3] as u32);
c.wr32((r[4] as u32).wrapping_add(0x00000004), r[7] as u32);
r[31] = k.pc(0x3f8dc0) as u64; r[4] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 out(c, r, h, k.jal(c, 0x3f8db8), k.pc(0x3f8db8), k.pc(0x3f8dc0));
fm::<1, 14>(c, 13, 31, c.vf[30]);
fm::<0, 14>(c, 12, 0, uycpu::vu0::bcast(c.vf[0][3]));
r[16] = 0x0000000070000000;
l = 0x003f8dd0; continue; }
0x003f8dd0 => {
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x00000010)); c.vf[16] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000c0)); c.vf[17] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000d0)); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000e0)); c.vf[19] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[2] = c.rd32((r[24] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
r[15] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[1] = 0x0000000044800000;
r[3] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[22] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
let t = r[15] == 0u64;  fm::<6, 14>(c, 16, 16, uycpu::vu0::bcast(c.vf[1][0]));
 if t { l = 0x003f8f60; continue; }
let t = (r[3] as i64) < 0;   if t { l = 0x003f8f60; continue; }
fm::<1, 14>(c, 14, 30, c.vf[16]);
r[13] = 0x0000000070000000;
fm::<1, 14>(c, 15, 31, c.vf[16]);
r[13] = r[13] | 0x1fe0;
fm::<6, 14>(c, 3, 17, uycpu::vu0::bcast(c.vf[22][0]));
r[11] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
fm::<6, 14>(c, 4, 18, uycpu::vu0::bcast(c.vf[22][0]));
r[12] = 0x0000000000800000;
fm::<6, 14>(c, 5, 19, uycpu::vu0::bcast(c.vf[22][0]));
r[12] = r[12] | 0x8080;
fm::<5, 14>(c, 10, 14, c.vf[15]);
r[11] = (((r[11] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
fm::<4, 14>(c, 11, 14, c.vf[15]);
l = 0x003f8e44; continue; }
0x003f8e44 => {
r[14] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlb((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
r[1] = c.rd64((r[14] as u32));
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000008));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
l = 0x003f8e70; continue; }
0x003f8e70 => {
uycpu::vu0::itof::<0, 14>(c, 1, 1);
r[1] = c.rd64((r[14] as u32).wrapping_add(0x00000010));
uycpu::vu0::itof::<0, 14>(c, 2, 2);
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000018));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 1, 5, uycpu::vu0::bcast(c.vf[1][2]));
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 2, 5, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 14>(c, 6, 1, c.vf[10]);
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
fm::<1, 14>(c, 7, 11, c.vf[1]);
r[14] = (((r[14] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let v = c.vf[1]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
fm::<1, 14>(c, 8, 2, c.vf[10]);
fm::<1, 14>(c, 9, 11, c.vf[2]);
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = { let v = c.vf[6]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pand((r[6], h[6]), (r[12], h[12])); r[6] = a; h[6] = b; }
{ let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::pand((r[7], h[7]), (r[12], h[12])); r[7] = a; h[7] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[8], h[8]), (r[12], h[12])); r[8] = a; h[8] = b; }
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[9], h[9]), (r[12], h[12])); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[6], h[6]), 31); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[7], h[7]), 30); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[8], h[8]), 31); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 30); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::por((r[6], h[6]), (r[7], h[7])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::por((r[8], h[8]), (r[9], h[9])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
c.wr32((r[13] as u32).wrapping_add(0x0000000c), r[6] as u32);
r[15] = (((r[15] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
let t = (r[15] as i64) > 0;  c.wr32((r[13] as u32).wrapping_add(0x0000001c), r[8] as u32);
 if t { l = 0x003f8e70; continue; }
r[1] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[13] = 0x0000000070000000;
r[14] = c.rd32((r[20] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[15] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
r[15] = (((r[15] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[11] = 0x0000000000020000;
r[5] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[11] = r[11] | 0x202;
r[12] = 0u64 | 0x700;
{ let (a, b) = uycpu::mmi::pcpyh((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
r[13] = r[13] | 0x2000;
 l = 0x003f8978; continue;
}
0x003f8f60 => {
r[1] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[2] = r[19] & 0x2;
let t = (r[2] as i64) > 0;  r[15] = (((r[20] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 if t { l = 0x003f8c78; continue; }
let t = r[1] == 0u64;  r[14] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x003f8c78; continue; }
fm::<1, 1>(c, 12, 0, c.vf[0]);
r[13] = r[19] & 0x4;
r[13] = ((r[13] as i64) < 1) as u64;
r[12] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
r[13] = (((r[13] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
l = 0x003f8f90; continue; }
0x003f8f90 => {
let t = (r[14] as i64) < 0;  r[1] = r[12] & 0x1;
 if t { l = 0x003f8c78; continue; }
r[12] = (((r[12] as u32) >> 1) as i32 as i64 as u64);
r[14] = c.rd16((r[15] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
r[2] = c.rd8((r[15] as u32)) as i8 as i64 as u64;
r[15] = (((r[15] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
let t = (r[1] as i64) > 0;  r[1] = r[13] & r[14];
 if t { l = 0x003f8f90; continue; }
let t = r[1] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f8f90; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f9230; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f91f8; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f91c8; continue; }
r[1] = c.rd16((r[15] as u32).wrapping_add(0xffffffe4)) as i16 as i64 as u64;
r[2] = c.rd16((r[15] as u32).wrapping_add(0xffffffe6)) as i16 as i64 as u64;
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[16] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[2] = (((r[16] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 14>(c, 1, 1, uycpu::vu0::bcast(c.vf[22][0]));
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[22][0]));
fm::<6, 1>(c, 10, 10, uycpu::vu0::bcast(c.vf[22][0]));
l = 0x003f9000; continue; }
0x003f9000 => {
fm::<6, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 1, 19, uycpu::vu0::bcast(c.vf[1][2]));
fm::<0, 8>(c, 10, 0, uycpu::vu0::bcast(c.vf[10][3]));
fm::<6, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 2, 19, uycpu::vu0::bcast(c.vf[2][2]));
fm::<6, 8>(c, 10, 10, c.vf[10]);
fm::<1, 14>(c, 2, 2, c.vf[1]);
fm::<0, 14>(c, 1, 1, c.vf[16]);
l = 0x003f9028; continue; }
0x003f9028 => {
uycpu::vu0::opmula(c, 2, 13);
uycpu::vu0::opmsub(c, 3, 13, 2);
uycpu::vu0::opmula(c, 13, 2);
uycpu::vu0::opmsub(c, 4, 2, 13);
fm::<6, 14>(c, 9, 3, c.vf[3]);
uycpu::vu0::opmula(c, 13, 3);
uycpu::vu0::opmsub(c, 7, 3, 13);
uycpu::vu0::opmula(c, 2, 4);
uycpu::vu0::opmsub(c, 8, 4, 2);
fm::<1, 14>(c, 5, 9, c.vf[12]);
r[1] = c.vi[17] as i32 as i64 as u64;
r[1] = r[1] & 0xe0;
r[2] = 0u64 | 0xe0;
let t = r[1] == r[2];  uycpu::vu0::mov::<14>(c, 6, c.vf[30]);
 if t { l = 0x003f9188; continue; }
fm::<1, 14>(c, 5, 30, c.vf[1]);
fm::<1, 14>(c, 6, 1, c.vf[30]);
fm::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
fm::<2, 8>(c, 9, 12, uycpu::vu0::bcast(c.vf[9][2]));
fm::<6, 14>(c, 7, 7, c.vf[5]);
fm::<6, 14>(c, 8, 8, c.vf[6]);
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 12, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 8, 12, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::qop::<0>(c, c.vf[7][0], c.vf[9][0]);
fm::<0, 14>(c, 32, 1, c.vf[0]);
fm::<2, 14>(c, 3, 2, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<0>(c, c.vf[8][0], c.vf[9][0]);
fm::<0, 14>(c, 32, 30, c.vf[0]);
fm::<2, 14>(c, 4, 13, uycpu::vu0::bcast(c.vi[22]));
fm::<6, 14>(c, 6, 2, c.vf[2]);
fm::<6, 14>(c, 7, 13, c.vf[13]);
fm::<1, 14>(c, 5, 4, c.vf[3]);
fm::<1, 4>(c, 10, 0, uycpu::vu0::bcast(c.vf[10][0]));
fm::<0, 8>(c, 32, 6, uycpu::vu0::bcast(c.vf[6][1]));
fm::<2, 8>(c, 6, 12, uycpu::vu0::bcast(c.vf[6][2]));
fm::<6, 14>(c, 5, 5, c.vf[5]);
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 12, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 4>(c, 32, 10, uycpu::vu0::bcast(c.vf[5][0]));
fm::<2, 4>(c, 32, 12, c.vf[5]);
fm::<2, 4>(c, 5, 12, uycpu::vu0::bcast(c.vf[5][2]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[6][0]);
{ let (a, b) = { let v = c.vf[5]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
let t = (r[1] as i64) >= 0;  fm::<1, 4>(c, 5, 0, c.vf[5]);
 if t { l = 0x003f8f90; continue; }
fm::<6, 14>(c, 6, 2, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[7][0]);
fm::<6, 14>(c, 7, 13, uycpu::vu0::bcast(c.vi[22]));
fm::<6, 14>(c, 6, 6, c.vf[7]);
fm::<0, 8>(c, 32, 6, uycpu::vu0::bcast(c.vf[6][1]));
fm::<2, 8>(c, 6, 12, uycpu::vu0::bcast(c.vf[6][2]));
fm::<6, 8>(c, 6, 6, c.vf[6]);
fm::<1, 8>(c, 6, 12, c.vf[6]);
uycpu::vu0::qop::<0>(c, c.vf[5][1], c.vf[6][0]);
fm::<0, 8>(c, 6, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 6, 6, c.vf[0]);
uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[6][0]);
fm::<6, 14>(c, 7, 7, uycpu::vu0::bcast(c.vi[22]));
fm::<1, 14>(c, 6, 4, c.vf[7]);
fm::<1, 14>(c, 3, 6, c.vf[30]);
fm::<6, 14>(c, 3, 3, c.vf[13]);
fm::<0, 8>(c, 32, 3, uycpu::vu0::bcast(c.vf[3][1]));
fm::<2, 8>(c, 3, 12, uycpu::vu0::bcast(c.vf[3][2]));
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) >= 0;   if t { l = 0x003f9188; continue; }
uycpu::vu0::mov::<14>(c, 6, c.vf[30]);
l = 0x003f9188; continue; }
0x003f9188 => {
fm::<1, 14>(c, 3, 6, c.vf[1]);
fm::<6, 14>(c, 4, 3, c.vf[2]);
fm::<6, 14>(c, 5, 2, c.vf[2]);
fm::<0, 8>(c, 32, 4, uycpu::vu0::bcast(c.vf[4][1]));
fm::<2, 8>(c, 4, 12, uycpu::vu0::bcast(c.vf[4][2]));
fm::<0, 8>(c, 32, 5, uycpu::vu0::bcast(c.vf[5][1]));
fm::<2, 8>(c, 5, 12, uycpu::vu0::bcast(c.vf[5][2]));
uycpu::vu0::qop::<0>(c, c.vf[4][0], c.vf[5][0]);
fm::<0, 8>(c, 4, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 4, 4, c.vf[0]);
fm::<5, 8>(c, 4, 4, c.vf[12]);
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[4][0]));
fm::<0, 14>(c, 1, 1, c.vf[2]);
fm::<0, 1>(c, 1, 10, uycpu::vu0::bcast(c.vf[0][0]));
 l = 0x003f9248; continue;
}
0x003f91c8 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 10, 1, uycpu::vu0::bcast(c.vf[22][0]));
fm::<6, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[22][0]));
fm::<6, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[10][0]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 14>(c, 32, 19, uycpu::vu0::bcast(c.vf[10][2]));
fm::<2, 14>(c, 1, 16, uycpu::vu0::bcast(c.vf[0][3]));
fm::<0, 8>(c, 10, 0, uycpu::vu0::bcast(c.vf[10][3]));
fm::<6, 14>(c, 2, 19, uycpu::vu0::bcast(c.vf[2][1]));
fm::<6, 8>(c, 10, 10, c.vf[10]);
 l = 0x003f9028; continue;
}
0x003f91f8 => {
r[1] = c.rd32((r[15] as u32).wrapping_add(0xffffffe4)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[16] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::mov::<1>(c, 1, c.vf[2]);
fm::<0, 14>(c, 1, 1, c.vf[3]);
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[22][0]));
fm::<6, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 1, 19, uycpu::vu0::bcast(c.vf[1][2]));
fm::<0, 14>(c, 1, 1, c.vf[16]);
 l = 0x003f9248; continue;
}
0x003f9230 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[22][0]));
fm::<6, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 1, 19, uycpu::vu0::bcast(c.vf[1][2]));
fm::<0, 14>(c, 1, 1, c.vf[16]);
l = 0x003f9248; continue; }
0x003f9248 => {
fm::<1, 14>(c, 2, 31, c.vf[30]);
fm::<1, 14>(c, 3, 1, c.vf[30]);
fm::<6, 14>(c, 5, 2, c.vf[2]);
fm::<6, 14>(c, 6, 3, c.vf[2]);
fm::<6, 14>(c, 7, 3, c.vf[3]);
fm::<6, 1>(c, 8, 1, c.vf[1]);
fm::<0, 8>(c, 32, 5, uycpu::vu0::bcast(c.vf[5][1]));
fm::<2, 8>(c, 5, 12, uycpu::vu0::bcast(c.vf[5][2]));
fm::<0, 8>(c, 32, 6, uycpu::vu0::bcast(c.vf[6][1]));
fm::<2, 8>(c, 6, 12, uycpu::vu0::bcast(c.vf[6][2]));
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 12, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::qop::<0>(c, c.vf[6][0], c.vf[5][0]);
fm::<0, 8>(c, 9, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 9, 9, c.vf[0]);
fm::<5, 8>(c, 9, 9, c.vf[12]);
fm::<0, 14>(c, 32, 30, c.vf[0]);
fm::<2, 14>(c, 9, 2, uycpu::vu0::bcast(c.vf[9][0]));
fm::<1, 14>(c, 9, 9, c.vf[1]);
fm::<6, 14>(c, 9, 9, c.vf[9]);
fm::<1, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[8][3]));
fm::<2, 8>(c, 32, 12, uycpu::vu0::bcast(c.vf[9][1]));
fm::<2, 8>(c, 9, 12, uycpu::vu0::bcast(c.vf[9][2]));
{ let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) > 0;   if t { l = 0x003f8f90; continue; }
fm::<0, 8>(c, 6, 6, c.vf[6]);
fm::<1, 8>(c, 7, 7, uycpu::vu0::bcast(c.vf[8][3]));
r[1] = 0x0000000040800000;
c.vf[10] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
fm::<6, 8>(c, 9, 6, c.vf[6]);
fm::<6, 8>(c, 10, 10, c.vf[5]);
fm::<6, 8>(c, 10, 10, c.vf[7]);
fm::<1, 8>(c, 9, 9, c.vf[10]);
fm::<4, 8>(c, 9, 9, c.vf[0]);
uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[9][0]);
fm::<1, 8>(c, 6, 0, c.vf[6]);
fm::<0, 8>(c, 5, 5, c.vf[5]);
fm::<0, 8>(c, 6, 6, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<0>(c, c.vf[6][0], c.vf[5][0]);
fm::<1, 8>(c, 5, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 5, 5, c.vf[0]);
fm::<5, 8>(c, 5, 5, c.vf[12]);
fm::<0, 14>(c, 32, 30, c.vf[0]);
fm::<2, 14>(c, 6, 2, uycpu::vu0::bcast(c.vf[5][0]));
fm::<1, 14>(c, 7, 6, c.vf[1]);
fm::<1, 8>(c, 8, 24, c.vf[5]);
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) <= 0;   if t { l = 0x003f8f90; continue; }
r[21] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[22] = (((r[15] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
r[22] = ((0u32.wrapping_sub((r[22] as u32))) as i32 as i64 as u64);
fm::<0, 8>(c, 24, 0, c.vf[5]);
fm::<0, 14>(c, 25, 0, c.vf[6]);
fm::<0, 14>(c, 26, 0, c.vf[7]);
 l = 0x003f8f90; continue;
}
0x003f9348 => {
r[1] = spr_to_chcr(c, (r[14] as u32).wrapping_add(0xffffd400)) as i32 as i64 as u64;
r[1] = r[1] & 0x100;
let t = r[1] != 0u64;   if t { l = 0x003f9348; continue; }
l = 0x003f9368; continue; }
0x003f9368 => {
let t = r[22] == 0u64;  r[2] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x003f9438; continue; }
r[2] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
r[10] = sx(k.hi(c, 0x3f9374));
r[10] = (((r[10] as u32).wrapping_add(k.lo(c, 0x3f9378))) as i32 as i64 as u64);
r[3] = 0x000000003a800000;
c.vf[1] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
fm::<6, 14>(c, 25, 25, uycpu::vu0::bcast(c.vf[1][0]));
fm::<6, 14>(c, 27, 27, uycpu::vu0::bcast(c.vf[1][0]));
fm::<6, 14>(c, 28, 28, uycpu::vu0::bcast(c.vf[1][0]));
fm::<6, 14>(c, 29, 29, uycpu::vu0::bcast(c.vf[1][0]));
{ let v = c.vf[25]; c.wr128((r[10] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[26]; c.wr128((r[10] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[27]; c.wr128((r[10] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[28]; c.wr128((r[10] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[29]; c.wr128((r[10] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((r[10] as u32).wrapping_add(0x00000018), r[21] as u32);
c.wr32((r[10] as u32).wrapping_add(0x0000001c), r[22] as u32);
let t = r[21] == 0u64;  r[3] = c.rd32((r[10] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
 if t { l = 0x003f9438; continue; }
let t = r[17] == 0u64;  r[1] = c.rd16((r[21] as u32).wrapping_add(0x00000034)) as u64;
 if t { l = 0x003f9438; continue; }
r[4] = (((r[3] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[5] = (((r[3] as u32) << 6) as i32 as i64 as u64);
r[1] = r[1] & 0x4000;
r[6] = c.rd8((r[21] as u32).wrapping_add(0x000000a8)) as i8 as i64 as u64;
let t = r[1] == 0u64;  r[4] = r[4] & 0x3f;
 if t { l = 0x003f9438; continue; }
r[8] = 0u64 | 0x0;
let t = (r[6] as i64) >= 0;  r[6] = (((r[6] as u32) << 6) as i32 as i64 as u64);
 if t { l = 0x003f9460; continue; }
l = 0x003f93e8; continue; }
0x003f93e8 => {
c.wr32((r[10] as u32).wrapping_add(0x00000014), r[4] as u32);
r[4] = sx(k.hi(c, 0x3f93ec));
r[4] = (((r[4] as u32).wrapping_add(k.lo(c, 0x3f93f0))) as i32 as i64 as u64);
c.wr8((r[21] as u32).wrapping_add(0x000000a8), r[3] as u8);
r[3] = (((r[4] as u32).wrapping_add((r[5] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[17] as u32)); r[4] = a; h[4] = b; }
{ let (a, b) = c.rd128((r[17] as u32).wrapping_add(0x00000010)); r[5] = a; h[5] = b; }
r[6] = c.rd64((r[17] as u32).wrapping_add(0x00000020));
r[7] = c.rd32((r[17] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
{ let v = c.vf[25]; c.wr128((r[3] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr128((r[3] as u32).wrapping_add(0x00000010), r[4], h[4]);
c.wr128((r[3] as u32).wrapping_add(0x00000020), r[5], h[5]);
c.wr64((r[3] as u32).wrapping_add(0x00000030), r[6]);
r[7] = r[7] | r[8];
c.wr32((r[3] as u32).wrapping_add(0x00000024), r[7] as u32);
c.wr32((r[3] as u32).wrapping_add(0x00000038), r[21] as u32);
r[20] = c.rd32((r[21] as u32).wrapping_add(0x00000098)) as i32 as i64 as u64;
r[1] = c.rd32((r[17] as u32).wrapping_add(0x00000028)) as i32 as i64 as u64;
c.wr32((r[3] as u32).wrapping_add(0x0000003c), r[1] as u32);
l = 0x003f9438; continue; }
0x003f9438 => {
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
let t = (r[31] as u32); { let (a, b) = uycpu::mmi::pcpyud((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
 c.ret = t; return;
}
0x003f9460 => {
r[7] = sx(k.hi(c, 0x3f9460));
r[7] = (((r[7] as u32).wrapping_add(k.lo(c, 0x3f9464))) as i32 as i64 as u64);
r[6] = (((r[6] as u32).wrapping_add((r[7] as u32))) as i32 as i64 as u64);
r[7] = c.rd32((r[6] as u32).wrapping_add(0x00000038)) as i32 as i64 as u64;
let t = r[7] != r[21];  r[7] = c.rd32((r[6] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
 if t { l = 0x003f93e8; continue; }
r[8] = c.rd32((r[17] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
r[7] = (((r[7] as u32).wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
let t = (r[7] as i64) < 0;  r[8] = c.rd32((r[6] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
 if t { l = 0x003f93e8; continue; }
r[7] = c.rd32((r[17] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
r[7] = r[7] | r[8];
c.wr32((r[6] as u32).wrapping_add(0x00000024), r[7] as u32);
 l = 0x003f9438; continue;
}
_ => unreachable!("0x3f8448: no label {l:08x} in it"), } } }

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
#[inline(always)]
fn b3f9498(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x003f9498;
loop { match l {
0x003f9498 => {
fm::<6, 1>(c, 19, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[15] = sx(k.hi(c, 0x3f949c));
r[15] = (((r[15] as u32).wrapping_add(k.lo(c, 0x3f94a0))) as i32 as i64 as u64);
fm::<6, 1>(c, 20, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[1] = 0x0000000044800000;
r[2] = 0x0000000041200000;
{ let (a, b) = uycpu::mmi::pcpyld((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
{ let (a, b) = c.rd128((r[4] as u32)); c.vf[31] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pcpyld((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
r[3] = c.f[12] as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyld((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
r[17] = c.rd32((r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pminw((r[3], h[3]), (r[2], h[2])); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
c.vf[30] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
fm::<1, 14>(c, 19, 31, uycpu::vu0::bcast(c.vf[30][0]));
r[19] = (((r[5] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<0, 14>(c, 20, 31, uycpu::vu0::bcast(c.vf[30][0]));
r[18] = (((r[6] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<6, 14>(c, 31, 31, uycpu::vu0::bcast(c.vf[1][0]));
r[1] = 0x000000003f7f0000;
fm::<6, 8>(c, 30, 30, c.vf[1]);
r[1] = r[1] | 0xdf3b;
fm::<1, 14>(c, 3, 19, c.vf[0]);
r[16] = 0x0000000070000000;
{ let (a, b) = uycpu::mmi::pcpyld((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
c.vf[21] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
let t = (r[17] as i64) < 0;  { let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
 if t { l = 0x003fa0a0; continue; }
r[20] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[22] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[1] = c.vi[17] as i32 as i64 as u64;
r[17] = (((r[17] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
fm::<6, 8>(c, 24, 30, c.vf[30]);
c.wr32((r[15] as u32).wrapping_add(0x00000010), r[17] as u32);
fm::<1, 14>(c, 2, 20, uycpu::vu0::bcast(c.vf[1][0]));
r[1] = r[1] & 0xe0;
let t = r[1] != 0u64;  uycpu::vu0::ftoi::<0, 15>(c, 3, 19);
 if t { l = 0x003fa0a0; continue; }
uycpu::vu0::ftoi::<0, 15>(c, 4, 20);
r[24] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[15] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
let t = (r[3] as i64) <= 0;   if t { l = 0x003fa0a0; continue; }
r[1] = c.vi[17] as i32 as i64 as u64;
r[2] = 0u64 | 0xe0;
r[1] = r[1] & 0xe0;
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[12] = a; h[12] = b; }
let t = r[1] != r[2];  { let (a, b) = { let v = c.vf[4]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[13] = a; h[13] = b; }
 if t { l = 0x003fa0a0; continue; }
fm::<6, 14>(c, 19, 19, uycpu::vu0::bcast(c.vf[1][0]));
r[1] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x3f9584))) as i32 as i64 as u64;
fm::<6, 14>(c, 20, 20, uycpu::vu0::bcast(c.vf[1][0]));
r[2] = 0u64 | 0x8000;
{ let (a, b) = uycpu::mmi::pcpyld((r[1], h[1]), (r[24], h[24])); r[24] = a; h[24] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[2], h[2]), (r[25], h[25])); r[25] = a; h[25] = b; }
fm::<1, 4>(c, 14, 0, uycpu::vu0::bcast(c.vf[24][0]));
r[1] = 0x0000000045000000;
fm::<6, 1>(c, 21, 0, uycpu::vu0::bcast(c.vf[21][0]));
r[2] = 0x0000000045800000;
{ let (a, b) = uycpu::mmi::psrlw((r[12], h[12]), 2); r[12] = a; h[12] = b; }
c.vf[12] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psrlw((r[13], h[13]), 2); r[13] = a; h[13] = b; }
c.vf[13] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
fm::<6, 8>(c, 24, 24, uycpu::vu0::bcast(c.vf[21][3]));
r[1] = r[19] & 0x1;
let t = (r[1] as i64) > 0;  fm::<0, 14>(c, 21, 0, uycpu::vu0::bcast(c.vf[0][3]));
 if t { l = 0x003f9b10; continue; }
{ let (a, b) = uycpu::mmi::psllw((r[12], h[12]), 12); r[2] = a; h[2] = b; }
c.wr64((r[16] as u32), 0u64);
c.vf[10] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyud((r[12], h[12]), (0u64, 0u64)); r[8] = a; h[8] = b; }
c.vf[11] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyud((r[13], h[13]), (0u64, 0u64)); r[11] = a; h[11] = b; }
uycpu::vu0::itof::<0, 14>(c, 10, 10);
r[10] = r[13] >> 32;
uycpu::vu0::itof::<0, 14>(c, 11, 11);
r[9] = (((r[13] as u32) >> 0) as i32 as i64 as u64);
l = 0x003f95f0; continue; }
0x003f95f0 => {
r[7] = r[12] >> 32;
uycpu::vu0::mov::<4>(c, 10, c.vf[11]);
l = 0x003f95f8; continue; }
0x003f95f8 => {
r[6] = (((r[12] as u32) >> 0) as i32 as i64 as u64);
uycpu::vu0::mov::<8>(c, 10, c.vf[11]);
l = 0x003f9600; continue; }
0x003f9600 => {
fm::<0, 14>(c, 2, 10, uycpu::vu0::bcast(c.vf[13][0]));
fm::<4, 14>(c, 1, 10, c.vf[31]);
fm::<5, 14>(c, 1, 1, c.vf[2]);
fm::<0, 14>(c, 3, 10, uycpu::vu0::bcast(c.vf[12][0]));
fm::<1, 14>(c, 1, 1, c.vf[31]);
fm::<6, 14>(c, 1, 1, c.vf[1]);
fm::<0, 4>(c, 32, 14, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 4>(c, 32, 21, c.vf[1]);
fm::<2, 4>(c, 1, 21, uycpu::vu0::bcast(c.vf[1][2]));
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[4] = a; h[4] = b; }
r[1] = (((r[6] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[2] = (((r[7] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = (r[4] as i64) >= 0;  r[3] = (((r[8] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x003f9680; continue; }
r[31] = k.pc(0x3f9640) as u64; { let v = c.vf[3]; c.wr128((r[15] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 grid(c, r);
let t = r[1] == 0u64;  c.wr32((r[15] as u32).wrapping_add(0x0000000c), r[1] as u32);
 if t { l = 0x003f9680; continue; }
let t = r[14] != 0u64;  r[15] = (((r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 if t { l = 0x003f9680; continue; }
r[14] = 0x0000000010000000;
r[2] = r[1] & 0xff;
r[14] = r[14] | 0xd400;
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000020), r[2] as u32);
r[1] = (((r[1] as u32) >> 8) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000080), 0u64 as u32);
r[1] = (((r[1] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000010), r[1] as u32);
r[1] = 0u64 | 0x100;
spr_to_reg(c, (r[14] as u32), r[1] as u32);
l = 0x003f9680; continue; }
0x003f9680 => {
fm::<0, 8>(c, 10, 10, c.vf[13]);
let t = r[6] != r[9];  r[6] = (((r[6] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003f9600; continue; }
fm::<0, 4>(c, 10, 10, uycpu::vu0::bcast(c.vf[13][0]));
let t = r[7] != r[10];  r[7] = (((r[7] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003f95f8; continue; }
fm::<0, 2>(c, 10, 10, uycpu::vu0::bcast(c.vf[13][0]));
let t = r[8] != r[11];  r[8] = (((r[8] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003f95f0; continue; }
r[25] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
r[23] = (((r[16] as u32).wrapping_add(0x00001000)) as i32 as i64 as u64);
c.wr32((r[15] as u32).wrapping_add(0x0000000c), 0u64 as u32);
l = 0x003f96c0; continue; }
0x003f96c0 => {
r[2] = c.rd32((r[25] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[14] = 0x0000000010000000;
r[1] = c.rd32((r[25] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
r[14] = r[14] | 0xd400;
let t = r[2] == 0u64;  { let (a, b) = c.rd128((r[25] as u32)); c.vf[23] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x003f9b10; continue; }
r[23] = r[23] ^ 0x1000;
r[25] = (((r[25] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
l = 0x003f96e0; continue; }
0x003f96e0 => {
r[2] = spr_to_chcr(c, (r[14] as u32)) as i32 as i64 as u64;
r[2] = r[2] & 0x100;
let t = r[2] != 0u64;   if t { l = 0x003f96e0; continue; }
let t = r[1] == 0u64;  r[3] = r[23] ^ 0x1000;
 if t { l = 0x003f9730; continue; }
r[2] = r[1] & 0xff;
c.wr64((r[3] as u32), 0u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000020), r[2] as u32);
r[1] = (((r[1] as u32) >> 8) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000080), r[3] as u32);
r[1] = (((r[1] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000010), r[1] as u32);
r[1] = 0u64 | 0x100;
spr_to_reg(c, (r[14] as u32), r[1] as u32);
l = 0x003f9730; continue; }
0x003f9730 => {
r[15] = c.rd8((r[23] as u32).wrapping_add(0x00000002)) as u64;
fm::<1, 14>(c, 1, 19, c.vf[23]);
r[14] = (((r[23] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
fm::<1, 14>(c, 2, 20, c.vf[23]);
let t = r[15] == 0u64;  fm::<1, 14>(c, 22, 31, c.vf[23]);
 if t { l = 0x003f96c0; continue; }
r[8] = 0x0000000000010000;
uycpu::vu0::ftoi::<0, 14>(c, 1, 1);
r[8] = r[8] | 0x101;
uycpu::vu0::ftoi::<0, 14>(c, 2, 2);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[11] = a; h[11] = b; }
{ let (a, b) = uycpu::mmi::pextlb((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlh((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[11], h[11])); r[11] = a; h[11] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::psllw((r[8], h[8]), 11); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psllw((r[8], h[8]), 15); r[10] = a; h[10] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[10], h[10])); r[10] = a; h[10] = b; }
r[13] = (((r[16] as u32).wrapping_add(0x00001fe0)) as i32 as i64 as u64);
r[1] = c.rd32((r[14] as u32)) as i32 as i64 as u64;
r[7] = 0x00000000000f0000;
r[2] = c.rd32((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[7] = r[7] | 0xfc00;
l = 0x003f9790; continue; }
0x003f9790 => {
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[14] = (((r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[3] = r[1] & 0x3ff;
r[5] = r[2] & 0x3ff;
r[4] = r[1] & r[7];
r[6] = r[2] & r[7];
r[3] = (((r[3] as u32) << 6) as i32 as i64 as u64);
r[5] = (((r[5] as u32) << 6) as i32 as i64 as u64);
r[4] = r[4] << 12;
r[6] = r[6] << 12;
r[1] = (((r[1] as u32) >> 20) as i32 as i64 as u64);
r[2] = (((r[2] as u32) >> 20) as i32 as i64 as u64);
r[1] = r[1] << 36;
r[2] = r[2] << 36;
r[3] = r[3] | r[4];
r[5] = r[5] | r[6];
r[1] = r[1] | r[3];
r[2] = r[2] | r[5];
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[4] = a; h[4] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[3], h[3]), 16); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[4], h[4]), 16); r[4] = a; h[4] = b; }
{ let (a, b) = uycpu::mmi::psubh((r[12], h[12]), (r[1], h[1])); r[5] = a; h[5] = b; }
c.vf[1] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psubh((r[12], h[12]), (r[2], h[2])); r[6] = a; h[6] = b; }
c.vf[2] = [r[4] as u32, (r[4] >> 32) as u32, h[4] as u32, (h[4] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psubh((r[1], h[1]), (r[11], h[11])); r[3] = a; h[3] = b; }
r[1] = c.rd32((r[14] as u32)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psubh((r[2], h[2]), (r[11], h[11])); r[4] = a; h[4] = b; }
r[2] = c.rd32((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
uycpu::vu0::itof::<0, 14>(c, 1, 1);
r[3] = r[3] & r[10];
uycpu::vu0::itof::<0, 14>(c, 2, 2);
r[4] = r[4] & r[10];
r[5] = r[5] & r[10];
r[6] = r[6] & r[10];
r[3] = r[3] >> 15;
r[4] = r[4] >> 15;
r[5] = r[5] >> 14;
r[6] = r[6] >> 14;
r[3] = r[3] | r[5];
r[4] = r[4] | r[6];
{ let v = c.vf[1]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[3], h[3])); r[3] = a; h[3] = b; }
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[4], h[4])); r[4] = a; h[4] = b; }
c.wr32((r[13] as u32).wrapping_add(0x0000000c), r[3] as u32);
r[15] = (((r[15] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[15] as i64) > 0;  c.wr32((r[13] as u32).wrapping_add(0x0000001c), r[4] as u32);
 if t { l = 0x003f9790; continue; }
r[1] = c.rd8((r[23] as u32).wrapping_add(0x00000002)) as u64;
r[13] = (((r[16] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
r[14] = c.rd8((r[23] as u32)) as u64;
r[15] = (((r[23] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
r[1] = (((r[1] as u32) << 2) as i32 as i64 as u64);
r[11] = 0x0000000000020000;
r[15] = (((r[15] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[14] = (((r[14] as u32) << 2) as i32 as i64 as u64);
r[5] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[11] = r[11] | 0x202;
r[12] = 0u64 | 0x700;
{ let (a, b) = uycpu::mmi::pcpyh((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
r[10] = (((r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003f9898; continue; }
0x003f9898 => {
let t = r[15] == r[14];  { let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (r[5], h[5])); r[1] = a; h[1] = b; }
 if t { l = 0x003f9ab8; continue; }
{ let (a, b) = uycpu::mmi::psllw((r[1], h[1]), 4); r[1] = a; h[1] = b; }
r[6] = c.rd32((r[1] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[2] = r[1] >> 32;
r[7] = c.rd32((r[2] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[1], h[1]), (0u64, 0u64)); r[3] = a; h[3] = b; }
r[8] = c.rd32((r[3] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[4] = r[5] >> 48;
r[5] = c.rd32((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[6] = r[6] & r[7];
r[6] = r[6] & r[8];
r[15] = (((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = (r[6] as i64) > 0;  { let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
 if t { l = 0x003f9898; continue; }
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[19] & 0x80;
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[6] & r[4];
let t = (r[6] as i64) > 0;  { let (a, b) = c.rd128((r[3] as u32)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x003f9898; continue; }
fm::<1, 14>(c, 2, 2, c.vf[1]);
r[6] = r[19] & 0x20;
fm::<1, 14>(c, 3, 3, c.vf[1]);
r[6] = ((r[6] as i64) < 1) as u64;
fm::<1, 14>(c, 4, 22, c.vf[1]);
r[7] = (((r[19] as u32) >> 8) as i32 as i64 as u64);
uycpu::vu0::opmula(c, 3, 2);
r[7] = r[7] ^ r[4];
uycpu::vu0::opmsub(c, 6, 2, 3);
r[7] = r[7] & 0x1f;
fm::<6, 14>(c, 7, 4, c.vf[6]);
r[6] = r[6] | r[7];
let t = r[6] == 0u64;  fm::<6, 14>(c, 8, 6, c.vf[6]);
 if t { l = 0x003f9898; continue; }
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::qop::<0>(c, c.vf[7][0], c.vf[8][0]);
r[6] = r[19] & 0x10;
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
fm::<1, 14>(c, 5, 0, c.vf[6]);
fm::<0, 14>(c, 32, 4, c.vf[0]);
let t = (r[6] as i64) > 0;  r[7] = r[7] << 32;
 if t { l = 0x003f9960; continue; }
let t = (r[7] as i64) < 0;   if t { l = 0x003f9898; continue; }
l = 0x003f9960; continue; }
0x003f9960 => {
fm::<6, 14>(c, 7, 6, uycpu::vu0::bcast(c.vi[22]));
fm::<2, 14>(c, 4, 5, uycpu::vu0::bcast(c.vi[22]));
fm::<6, 14>(c, 7, 7, c.vf[7]);
fm::<1, 14>(c, 9, 3, c.vf[2]);
fm::<1, 14>(c, 10, 4, c.vf[2]);
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 5, 21, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::opmula(c, 4, 2);
uycpu::vu0::opmsub(c, 7, 2, 4);
uycpu::vu0::opmula(c, 3, 4);
uycpu::vu0::opmsub(c, 8, 4, 3);
fm::<1, 8>(c, 11, 24, c.vf[5]);
uycpu::vu0::opmula(c, 10, 9);
uycpu::vu0::opmsub(c, 9, 9, 10);
fm::<6, 14>(c, 7, 7, c.vf[6]);
fm::<6, 14>(c, 8, 8, c.vf[6]);
{ let (a, b) = { let v = c.vf[11]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
fm::<6, 14>(c, 9, 9, c.vf[6]);
fm::<0, 4>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][0]));
r[8] = r[8] << 32;
let t = (r[8] as i64) < 0;  fm::<2, 4>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
 if t { l = 0x003f9898; continue; }
fm::<0, 4>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][0]));
fm::<2, 4>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<0, 4>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][0]));
fm::<2, 4>(c, 9, 21, uycpu::vu0::bcast(c.vf[9][2]));
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
let t = (r[7] as i64) < 0;  { let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
 if t { l = 0x003f9a18; continue; }
let t = (r[8] as i64) < 0;   if t { l = 0x003f9a08; continue; }
let t = (r[9] as i64) >= 0;   if t { l = 0x003f9a88; continue; }
fm::<0, 14>(c, 7, 2, c.vf[0]);
fm::<0, 14>(c, 8, 3, c.vf[0]);
 l = 0x003f9a20; continue;
}
0x003f9a08 => {
fm::<0, 14>(c, 7, 3, c.vf[0]);
fm::<0, 14>(c, 8, 0, c.vf[0]);
 l = 0x003f9a20; continue;
}
0x003f9a18 => {
fm::<0, 14>(c, 7, 0, c.vf[0]);
fm::<0, 14>(c, 8, 2, c.vf[0]);
l = 0x003f9a20; continue; }
0x003f9a20 => {
fm::<1, 14>(c, 9, 4, c.vf[7]);
fm::<1, 14>(c, 8, 8, c.vf[7]);
fm::<6, 14>(c, 10, 9, c.vf[8]);
fm::<6, 14>(c, 11, 8, c.vf[8]);
fm::<0, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 8>(c, 10, 21, uycpu::vu0::bcast(c.vf[10][2]));
fm::<0, 8>(c, 32, 11, uycpu::vu0::bcast(c.vf[11][1]));
fm::<2, 8>(c, 11, 21, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::qop::<0>(c, c.vf[10][0], c.vf[11][0]);
fm::<0, 8>(c, 10, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 10, 10, c.vf[0]);
fm::<5, 8>(c, 10, 10, c.vf[21]);
fm::<0, 14>(c, 32, 7, c.vf[0]);
fm::<2, 14>(c, 4, 8, uycpu::vu0::bcast(c.vf[10][0]));
fm::<1, 14>(c, 7, 22, c.vf[1]);
fm::<1, 14>(c, 7, 4, c.vf[7]);
fm::<6, 14>(c, 8, 7, c.vf[7]);
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 5, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<1, 8>(c, 8, 24, c.vf[5]);
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
r[8] = r[8] << 32;
let t = (r[8] as i64) < 0;   if t { l = 0x003f9898; continue; }
l = 0x003f9a88; continue; }
0x003f9a88 => {
r[21] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<0, 14>(c, 1, 1, c.vf[23]);
r[22] = (((r[4] as u32).wrapping_add(0x00001000)) as i32 as i64 as u64);
fm::<6, 8>(c, 24, 5, uycpu::vu0::bcast(c.vf[21][3]));
r[1] = r[19] & 0x40;
fm::<0, 14>(c, 26, 6, c.vf[0]);
fm::<0, 14>(c, 27, 0, c.vf[1]);
fm::<0, 14>(c, 28, 2, c.vf[1]);
let t = r[1] != 0u64;  fm::<0, 14>(c, 29, 3, c.vf[1]);
 if t { l = 0x003fa1f8; continue; }
fm::<0, 14>(c, 25, 4, c.vf[1]);
 l = 0x003f9898; continue;
}
0x003f9ab8 => {
let t = r[20] != 0u64;   if t { l = 0x003f9e58; continue; }
let t = r[10] == 0u64;  r[3] = c.rd8((r[23] as u32).wrapping_add(0x00000003)) as u64;
 if t { l = 0x003f96c0; continue; }
r[1] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[2] = (((r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = r[3] == 0u64;  r[14] = (((r[3] as u32) << 2) as i32 as i64 as u64);
 if t { l = 0x003f96c0; continue; }
l = 0x003f9ad8; continue; }
0x003f9ad8 => {
r[4] = c.rd8((r[1] as u32).wrapping_add(0x00000002)) as u64;
r[3] = (((r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[5] = c.rd8((r[2] as u32)) as u64;
r[2] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr8((r[1] as u32).wrapping_add(0x00000001), r[4] as u8);
c.wr8((r[1] as u32).wrapping_add(0x00000002), r[5] as u8);
let t = r[3] != 0u64;  r[1] = (((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 if t { l = 0x003f9ad8; continue; }
r[5] = c.rd32((r[10] as u32)) as i32 as i64 as u64;
r[15] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[10] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
 l = 0x003f9898; continue;
}
0x003f9b10 => {
uycpu::vu0::ftoi::<0, 14>(c, 1, 19);
r[15] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x3f9b14))) as i32 as i64 as u64;
uycpu::vu0::ftoi::<0, 14>(c, 2, 20);
r[21] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[4] = a; h[4] = b; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[15], h[15]), (0u64, 0u64)); r[15] = a; h[15] = b; }
r[3] = (((r[4] as u32) >> 14) as i32 as i64 as u64);
r[4] = r[4] >> 46;
r[5] = (((r[6] as u32) >> 14) as i32 as i64 as u64);
r[6] = r[6] >> 46;
r[25] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
r[7] = sx(k.hi(c, 0x3f9b44));
r[7] = (((r[7] as u32).wrapping_add(k.lo(c, 0x3f9b48))) as i32 as i64 as u64);
r[2] = (((r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003f9b50; continue; }
0x003f9b50 => {
r[1] = (((r[3] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003f9b58; continue; }
0x003f9b58 => {
r[8] = (((r[1] as u32) << 2) as i32 as i64 as u64);
r[9] = (((r[2] as u32) << 8) as i32 as i64 as u64);
r[8] = (((r[8] as u32).wrapping_add((r[9] as u32))) as i32 as i64 as u64);
r[8] = (((r[8] as u32).wrapping_add((r[7] as u32))) as i32 as i64 as u64);
r[9] = c.rd16((r[8] as u32)) as u64;
r[10] = (((r[7] as u32).wrapping_add(0x00004000)) as i32 as i64 as u64);
r[8] = c.rd8((r[8] as u32).wrapping_add(0x00000002)) as u64;
r[9] = (((r[9] as u32) << 5) as i32 as i64 as u64);
r[8] = (((r[8] as u32) << 1) as i32 as i64 as u64);
r[9] = (((r[9] as u32).wrapping_add((r[10] as u32))) as i32 as i64 as u64);
c.wr32((r[25] as u32), r[9] as u32);
r[8] = (((r[8] as u32).wrapping_add((r[9] as u32))) as i32 as i64 as u64);
c.wr32((r[25] as u32).wrapping_add(0x00000004), r[8] as u32);
r[25] = (((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = r[1] != r[5];  r[1] = (((r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003f9b58; continue; }
let t = r[2] != r[6];  r[2] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003f9b50; continue; }
c.wr64((r[25] as u32), 0u64);
r[25] = (((r[16] as u32).wrapping_add(0x000037f8)) as i32 as i64 as u64);
l = 0x003f9ba8; continue; }
0x003f9ba8 => {
r[23] = c.rd32((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[25] = (((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = r[23] == 0u64;   if t { l = 0x003fa0a0; continue; }
l = 0x003f9bb8; continue; }
0x003f9bb8 => {
r[24] = c.rd16((r[23] as u32)) as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[15], h[15]), (0u64, 0u64)); r[2] = a; h[2] = b; }
r[1] = c.rd32((r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[24] = (((r[24] as u32) << 8) as i32 as i64 as u64);
let t = r[23] == r[1];  r[24] = (((r[24] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
 if t { l = 0x003f9ba8; continue; }
{ let (a, b) = c.rd128((r[24] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[23] = (((r[23] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
let t = r[24] == r[18];   if t { l = 0x003f9bb8; continue; }
fm::<0, 8>(c, 3, 30, uycpu::vu0::bcast(c.vf[1][3]));
fm::<1, 14>(c, 2, 1, c.vf[31]);
r[20] = c.rd32((r[24] as u32).wrapping_add(0x00000098)) as i32 as i64 as u64;
fm::<6, 8>(c, 3, 3, c.vf[3]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
r[5] = r[19] & 0x4;
r[5] = ((r[5] as i64) < 1) as u64;
r[10] = sx(k.hi(c, 0x3f9bfc));
r[10] = (((r[10] as u32).wrapping_add(k.lo(c, 0x3f9c00))) as i32 as i64 as u64);
r[5] = (((r[5] as u32) << 1) as i32 as i64 as u64);
fm::<1, 8>(c, 32, 2, c.vf[3]);
fm::<2, 8>(c, 32, 21, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000a0)) as i32 as i64 as u64;
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[5] = (((r[5] as u32).wrapping_add((r[20] as u32))) as i32 as i64 as u64);
let t = r[3] == r[17];   if t { l = 0x003f9bb8; continue; }
let t = r[20] == 0u64;  r[1] = r[1] << 32;
 if t { l = 0x003f9bb8; continue; }
let t = (r[1] as i64) > 0;  r[1] = r[19] & 0x2;
 if t { l = 0x003f9bb8; continue; }
r[5] = c.rd16((r[5] as u32)) as u64;
r[2] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[1] = ((r[1] as i64) < 1) as u64;
let t = r[5] == 0u64;  c.wr32((r[24] as u32).wrapping_add(0x000000a0), r[17] as u32);
 if t { l = 0x003f9cc8; continue; }
let t = r[1] == r[2];  r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000ac)) as i32 as i64 as u64;
 if t { l = 0x003f9bb8; continue; }
r[1] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[2] = ((0u32.wrapping_add(0x00000008)) as i32 as i64 as u64);
r[16] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
l = 0x003f9c68; continue; }
0x003f9c68 => {
r[6] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
r[1] = (((r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[6] == r[3];   if t { l = 0x003f9cc8; continue; }
let t = (r[2] as i64) > 0;  r[16] = (((r[16] as u32).wrapping_add(0x00000800)) as i32 as i64 as u64);
 if t { l = 0x003f9c68; continue; }
r[2] = c.rd32((r[10] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[6] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[1] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[4] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[1] = r[1] & 0x7;
c.wr32((r[10] as u32).wrapping_add(0x0000000c), r[1] as u32);
r[1] = (((r[2] as u32) << 3) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 11) as i32 as i64 as u64);
r[4] = (((r[4] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[6] = (((r[6] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
c.wr32((r[4] as u32), r[3] as u32);
c.wr32((r[4] as u32).wrapping_add(0x00000004), r[7] as u32);
r[31] = k.pc(0x3f9cc0) as u64; r[4] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 out(c, r, h, k.jal(c, 0x3f9cb8), k.pc(0x3f9cb8), k.pc(0x3f9cc0));
r[16] = 0x0000000070000000;
l = 0x003f9cc8; continue; }
0x003f9cc8 => {
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x00000010)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000c0)); c.vf[16] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000d0)); c.vf[17] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000e0)); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[2] = c.rd32((r[24] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
r[15] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[1] = 0x0000000044800000;
r[3] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[13] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
let t = r[15] == 0u64;  fm::<6, 14>(c, 15, 15, uycpu::vu0::bcast(c.vf[1][0]));
 if t { l = 0x003f9e58; continue; }
let t = (r[3] as i64) < 0;   if t { l = 0x003f9e58; continue; }
r[13] = 0x0000000070000000;
fm::<6, 14>(c, 3, 16, uycpu::vu0::bcast(c.vf[13][0]));
r[13] = r[13] | 0x1fe0;
fm::<6, 14>(c, 4, 17, uycpu::vu0::bcast(c.vf[13][0]));
r[11] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
fm::<6, 14>(c, 5, 18, uycpu::vu0::bcast(c.vf[13][0]));
r[12] = 0x0000000000800000;
fm::<1, 14>(c, 22, 31, c.vf[15]);
r[12] = r[12] | 0x8080;
uycpu::vu0::mov::<14>(c, 23, c.vf[15]);
{ let (a, b) = uycpu::mmi::pextlb((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
fm::<1, 14>(c, 11, 22, uycpu::vu0::bcast(c.vf[30][0]));
r[11] = (((r[11] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
fm::<0, 14>(c, 12, 22, uycpu::vu0::bcast(c.vf[30][0]));
r[14] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
r[1] = c.rd64((r[14] as u32));
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000008));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
l = 0x003f9d68; continue; }
0x003f9d68 => {
uycpu::vu0::itof::<0, 14>(c, 1, 1);
r[1] = c.rd64((r[14] as u32).wrapping_add(0x00000010));
uycpu::vu0::itof::<0, 14>(c, 2, 2);
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000018));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 1, 5, uycpu::vu0::bcast(c.vf[1][2]));
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 2, 5, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 14>(c, 6, 1, c.vf[11]);
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
fm::<1, 14>(c, 7, 12, c.vf[1]);
r[14] = (((r[14] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let v = c.vf[1]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
fm::<1, 14>(c, 8, 2, c.vf[11]);
fm::<1, 14>(c, 9, 12, c.vf[2]);
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = { let v = c.vf[6]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pand((r[6], h[6]), (r[12], h[12])); r[6] = a; h[6] = b; }
{ let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::pand((r[7], h[7]), (r[12], h[12])); r[7] = a; h[7] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[8], h[8]), (r[12], h[12])); r[8] = a; h[8] = b; }
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[9], h[9]), (r[12], h[12])); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[6], h[6]), 31); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[7], h[7]), 30); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[8], h[8]), 31); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 30); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::por((r[6], h[6]), (r[7], h[7])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::por((r[8], h[8]), (r[9], h[9])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
c.wr32((r[13] as u32).wrapping_add(0x0000000c), r[6] as u32);
r[15] = (((r[15] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
let t = (r[15] as i64) > 0;  c.wr32((r[13] as u32).wrapping_add(0x0000001c), r[8] as u32);
 if t { l = 0x003f9d68; continue; }
r[1] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[13] = 0x0000000070000000;
r[14] = c.rd32((r[20] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[15] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
r[15] = (((r[15] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[11] = 0x0000000000020000;
r[5] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[11] = r[11] | 0x202;
r[12] = 0u64 | 0x700;
{ let (a, b) = uycpu::mmi::pcpyh((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
r[13] = r[13] | 0x2000;
 l = 0x003f9898; continue;
}
0x003f9e58 => {
r[1] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[2] = r[19] & 0x2;
let t = (r[2] as i64) > 0;  r[15] = (((r[20] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 if t { l = 0x003f9bb8; continue; }
let t = r[1] == 0u64;  r[13] = r[19] & 0x4;
 if t { l = 0x003f9bb8; continue; }
r[14] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[13] = ((r[13] as i64) < 1) as u64;
r[12] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
r[13] = (((r[13] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
l = 0x003f9e80; continue; }
0x003f9e80 => {
let t = (r[14] as i64) < 0;  r[1] = r[12] & 0x1;
 if t { l = 0x003f9bb8; continue; }
r[12] = (((r[12] as u32) >> 1) as i32 as i64 as u64);
r[14] = c.rd16((r[15] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
r[2] = c.rd8((r[15] as u32)) as i8 as i64 as u64;
r[15] = (((r[15] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
let t = (r[1] as i64) > 0;  r[1] = r[13] & r[14];
 if t { l = 0x003f9e80; continue; }
let t = r[1] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f9e80; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f9fd0; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f9f98; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003f9f68; continue; }
r[1] = c.rd16((r[15] as u32).wrapping_add(0xffffffe4)) as i16 as i64 as u64;
r[2] = c.rd16((r[15] as u32).wrapping_add(0xffffffe6)) as i16 as i64 as u64;
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[1] as u32).wrapping_add((r[16] as u32))) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add((r[16] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 14>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 1>(c, 3, 3, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[2][2]));
fm::<2, 14>(c, 2, 15, uycpu::vu0::bcast(c.vf[21][0]));
fm::<1, 14>(c, 2, 2, c.vf[1]);
fm::<1, 14>(c, 4, 31, c.vf[1]);
l = 0x003f9f18; continue; }
0x003f9f18 => {
fm::<6, 14>(c, 6, 4, c.vf[2]);
fm::<6, 14>(c, 7, 2, c.vf[2]);
fm::<0, 8>(c, 32, 6, uycpu::vu0::bcast(c.vf[6][1]));
fm::<2, 8>(c, 6, 21, uycpu::vu0::bcast(c.vf[6][2]));
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::qop::<0>(c, c.vf[6][0], c.vf[7][0]);
uycpu::vu0::mov::<14>(c, 11, c.vf[1]);
fm::<0, 14>(c, 12, 1, c.vf[2]);
uycpu::vu0::mov::<1>(c, 11, c.vf[3]);
fm::<0, 8>(c, 6, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 6, 6, c.vf[0]);
fm::<5, 8>(c, 6, 6, c.vf[21]);
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[6][0]));
fm::<0, 14>(c, 1, 1, c.vf[2]);
uycpu::vu0::mov::<1>(c, 1, c.vf[3]);
 l = 0x003f9fe8; continue;
}
0x003f9f68 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[0][3]));
fm::<6, 14>(c, 2, 18, uycpu::vu0::bcast(c.vf[2][1]));
uycpu::vu0::mov::<1>(c, 3, c.vf[1]);
fm::<1, 14>(c, 4, 31, c.vf[1]);
 l = 0x003f9f18; continue;
}
0x003f9f98 => {
r[1] = c.rd32((r[15] as u32).wrapping_add(0xffffffe4)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[1] as u32).wrapping_add((r[16] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::mov::<1>(c, 1, c.vf[2]);
fm::<0, 14>(c, 1, 1, c.vf[3]);
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
 l = 0x003f9fe8; continue;
}
0x003f9fd0 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
l = 0x003f9fe8; continue; }
0x003f9fe8 => {
fm::<0, 8>(c, 3, 30, uycpu::vu0::bcast(c.vf[1][3]));
fm::<1, 14>(c, 2, 1, c.vf[31]);
fm::<6, 8>(c, 3, 3, c.vf[3]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
fm::<0, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
l = 0x003fa000; continue; }
0x003fa000 => {
fm::<1, 8>(c, 5, 3, c.vf[2]);
uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[2][0]);
{ let (a, b) = { let v = c.vf[5]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) < 0;   if t { l = 0x003f9e80; continue; }
fm::<0, 8>(c, 2, 0, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[2][0]);
fm::<6, 8>(c, 2, 2, c.vf[21]);
fm::<5, 8>(c, 3, 2, uycpu::vu0::bcast(c.vf[1][3]));
fm::<0, 8>(c, 2, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<1, 14>(c, 5, 31, c.vf[1]);
fm::<6, 14>(c, 5, 5, uycpu::vu0::bcast(c.vf[2][0]));
fm::<6, 14>(c, 5, 5, uycpu::vu0::bcast(c.vf[3][0]));
fm::<0, 14>(c, 6, 1, c.vf[5]);
fm::<1, 14>(c, 2, 6, c.vf[31]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
fm::<0, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 8>(c, 3, 24, c.vf[2]);
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
r[3] = 0x000000003e000000;
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[2] = (((r[2] as u32) >> 0) as i32 as i64 as u64);
r[1] = r[1] << 32;
r[2] = r[2].wrapping_sub(r[3]);
let t = (r[1] as i64) < 0;  r[1] = r[19] & 0x40;
 if t { l = 0x003f9e80; continue; }
let t = r[1] != 0u64;  r[22] = (((r[15] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
 if t { l = 0x003fa260; continue; }
r[21] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[22] = ((0u32.wrapping_sub((r[22] as u32))) as i32 as i64 as u64);
let t = (r[2] as i64) < 0;  fm::<6, 8>(c, 24, 2, c.vf[21]);
 if t { l = 0x003fa188; continue; }
fm::<0, 14>(c, 25, 0, c.vf[6]);
fm::<0, 14>(c, 26, 0, c.vf[5]);
 l = 0x003f9e80; continue;
}
0x003fa0a0 => {
let t = r[22] == 0u64;  r[2] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x003fa118; continue; }
uycpu::vu0::qop::<2>(c, c.vf[30][0], c.vf[24][0]);
r[2] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
fm::<1, 14>(c, 2, 31, c.vf[25]);
r[1] = 0x000000003a800000;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
r[3] = r[19] & 0x40;
let t = r[3] != 0u64;  fm::<6, 14>(c, 25, 25, uycpu::vu0::bcast(c.vf[1][0]));
 if t { l = 0x003fa2c0; continue; }
fm::<6, 14>(c, 27, 27, uycpu::vu0::bcast(c.vf[1][0]));
fm::<6, 14>(c, 28, 28, uycpu::vu0::bcast(c.vf[1][0]));
fm::<6, 14>(c, 29, 29, uycpu::vu0::bcast(c.vf[1][0]));
r[1] = sx(k.hi(c, 0x3fa0e0));
r[1] = (((r[1] as u32).wrapping_add(k.lo(c, 0x3fa0e4))) as i32 as i64 as u64);
{ let v = c.vf[25]; c.wr128((r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[26]; c.wr128((r[1] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[27]; c.wr128((r[1] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[28]; c.wr128((r[1] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[29]; c.wr128((r[1] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((r[1] as u32).wrapping_add(0x00000018), r[21] as u32);
c.wr32((r[1] as u32).wrapping_add(0x0000001c), r[22] as u32);
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vi[22]));
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[1][0]));
fm::<0, 14>(c, 2, 2, c.vf[25]);
{ let v = c.vf[2]; c.wr128((r[1] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
l = 0x003fa118; continue; }
0x003fa118 => {
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
let t = (r[31] as u32); { let (a, b) = uycpu::mmi::pcpyud((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
 c.ret = t; return;
}
0x003fa140 => {
r[4] = r[18].wrapping_add(0u64);
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
c.f[12] = (r[31] as u32);
r[31] = k.pc(0x3fa178) as u64;  out(c, r, h, k.jal(c, 0x3fa170), k.pc(0x3fa170), k.pc(0x3fa178));
r[31] = c.f[12] as i32 as i64 as u64;
r[2] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = (r[31] as u32);  c.ret = t; return;
}
0x003fa188 => {
fm::<1, 14>(c, 2, 31, c.vf[1]);
fm::<6, 14>(c, 3, 2, c.vf[2]);
fm::<0, 8>(c, 32, 3, uycpu::vu0::bcast(c.vf[3][1]));
fm::<2, 8>(c, 3, 21, uycpu::vu0::bcast(c.vf[3][2]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[3][0]);
r[2] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
fm::<0, 1>(c, 2, 1, uycpu::vu0::bcast(c.vf[30][0]));
r[1] = 0x000000003a800000;
c.vf[5] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
r[1] = sx(k.hi(c, 0x3fa1ac));
r[1] = (((r[1] as u32).wrapping_add(k.lo(c, 0x3fa1b0))) as i32 as i64 as u64);
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[5][0]));
fm::<6, 1>(c, 2, 2, uycpu::vu0::bcast(c.vf[5][0]));
{ let v = c.vf[0]; c.wr128((r[1] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[0]; c.wr128((r[1] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[0]; c.wr128((r[1] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((r[1] as u32).wrapping_add(0x00000018), r[21] as u32);
c.wr32((r[1] as u32).wrapping_add(0x0000001c), r[22] as u32);
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vi[22]));
fm::<6, 14>(c, 3, 2, uycpu::vu0::bcast(c.vf[1][3]));
fm::<6, 14>(c, 4, 2, uycpu::vu0::bcast(c.vf[2][3]));
fm::<0, 14>(c, 3, 3, c.vf[1]);
fm::<0, 14>(c, 4, 4, c.vf[1]);
{ let v = c.vf[2]; c.wr128((r[1] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[3]; c.wr128((r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[4]; c.wr128((r[1] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x003fa118; continue;
}
0x003fa1f8 => {
fm::<6, 14>(c, 6, 26, c.vf[26]);
fm::<6, 8>(c, 24, 30, c.vf[30]);
r[2] = 0x000000003a800000;
{ let (a, b) = uycpu::mmi::pcpyud((r[25], h[25]), (0u64, 0u64)); r[3] = a; h[3] = b; }
c.vf[3] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
let t = r[3] == 0u64;  { let (a, b) = uycpu::mmi::pcpyud((r[24], h[24]), (0u64, 0u64)); r[1] = a; h[1] = b; }
 if t { l = 0x003fa140; continue; }
fm::<0, 8>(c, 32, 6, uycpu::vu0::bcast(c.vf[6][1]));
fm::<2, 8>(c, 6, 21, uycpu::vu0::bcast(c.vf[6][2]));
fm::<6, 14>(c, 1, 27, uycpu::vu0::bcast(c.vf[3][0]));
r[3] = (((r[3] as u32).wrapping_add(0xffffffc0)) as i32 as i64 as u64);
fm::<6, 14>(c, 2, 28, uycpu::vu0::bcast(c.vf[3][0]));
fm::<6, 14>(c, 3, 29, uycpu::vu0::bcast(c.vf[3][0]));
fm::<6, 1>(c, 26, 0, uycpu::vu0::bcast(c.vf[6][0]));
{ let (a, b) = uycpu::mmi::pcpyld((r[3], h[3]), (r[25], h[25])); r[25] = a; h[25] = b; }
{ let v = c.vf[1]; c.wr128((r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[2]; c.wr128((r[1] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[3]; c.wr128((r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[26]; c.wr128((r[1] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((r[1] as u32).wrapping_add(0x0000000c), r[22] as u32);
c.wr32((r[1] as u32).wrapping_add(0x0000001c), r[24] as u32);
r[1] = (((r[1] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pcpyld((r[1], h[1]), (r[24], h[24])); r[24] = a; h[24] = b; }
 l = 0x003f9898; continue;
}
0x003fa260 => {
r[2] = (((r[15] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
r[1] = 0x000000003a800000;
{ let (a, b) = uycpu::mmi::pcpyud((r[25], h[25]), (0u64, 0u64)); r[3] = a; h[3] = b; }
c.vf[3] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
r[2] = c.rd8((r[2] as u32)) as i8 as i64 as u64;
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[3][0]));
let t = r[3] == 0u64;  { let (a, b) = uycpu::mmi::pcpyud((r[24], h[24]), (0u64, 0u64)); r[1] = a; h[1] = b; }
 if t { l = 0x003fa140; continue; }
{ let v = c.vf[1]; c.wr128((r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[3] = (((r[3] as u32).wrapping_add(0xffffffc0)) as i32 as i64 as u64);
c.wr128((r[1] as u32).wrapping_add(0x00000010), 0u64, 0u64);
r[2] = (((r[2] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
c.wr128((r[1] as u32).wrapping_add(0x00000020), 0u64, 0u64);
{ let (a, b) = uycpu::mmi::pcpyld((r[3], h[3]), (r[25], h[25])); r[25] = a; h[25] = b; }
let t = (r[2] as i64) <= 0;  c.wr128((r[1] as u32).wrapping_add(0x00000030), 0u64, 0u64);
 if t { l = 0x003fa2b0; continue; }
fm::<6, 15>(c, 11, 11, uycpu::vu0::bcast(c.vf[3][0]));
fm::<6, 14>(c, 12, 12, uycpu::vu0::bcast(c.vf[3][0]));
{ let v = c.vf[11]; c.wr128((r[1] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[12]; c.wr128((r[1] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
l = 0x003fa2b0; continue; }
0x003fa2b0 => {
c.wr32((r[1] as u32).wrapping_add(0x0000001c), r[24] as u32);
r[1] = (((r[1] as u32).wrapping_add(0x00000040)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pcpyld((r[1], h[1]), (r[24], h[24])); r[24] = a; h[24] = b; }
 l = 0x003f9e80; continue;
}
0x003fa2c0 => {
{ let (a, b) = uycpu::mmi::pcpyud((r[24], h[24]), (0u64, 0u64)); r[1] = a; h[1] = b; }
r[2] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x3fa2c8))) as i32 as i64 as u64;
r[2] = (((r[1] as u32).wrapping_sub((r[2] as u32))) as i32 as i64 as u64);
r[2] = (((r[2] as u32) >> 6) as i32 as i64 as u64);
 l = 0x003fa118; continue;
}
_ => unreachable!("0x3f9498: no label {l:08x} in it"), } } }

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
#[inline(always)]
fn b3fa2d8(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x003fa2d8;
loop { match l {
0x003fa2d8 => {
fm::<6, 1>(c, 19, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[15] = sx(k.hi(c, 0x3fa2dc));
r[15] = (((r[15] as u32).wrapping_add(k.lo(c, 0x3fa2e0))) as i32 as i64 as u64);
fm::<6, 1>(c, 20, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[1] = 0x0000000044800000;
r[10] = c.f[12] as i32 as i64 as u64;
r[11] = c.f[13] as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyld((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
{ let (a, b) = c.rd128((r[4] as u32)); c.vf[31] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pcpyld((r[11], h[11]), (r[10], h[10])); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
r[17] = c.rd32((r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyld((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
c.vf[30] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
fm::<0, 14>(c, 20, 31, uycpu::vu0::bcast(c.vf[30][0]));
r[19] = (((r[5] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<1, 14>(c, 19, 31, uycpu::vu0::bcast(c.vf[30][0]));
r[18] = (((r[6] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<6, 14>(c, 31, 31, uycpu::vu0::bcast(c.vf[1][0]));
r[1] = 0x000000003f7f0000;
{ let (a, b) = uycpu::mmi::pcpyld((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
fm::<0, 2>(c, 20, 20, c.vf[30]);
r[1] = r[1] | 0xdf3b;
fm::<6, 10>(c, 30, 30, uycpu::vu0::bcast(c.vf[1][0]));
fm::<1, 14>(c, 3, 19, c.vf[0]);
r[16] = 0x0000000070000000;
{ let (a, b) = uycpu::mmi::pcpyld((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
c.vf[21] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
let t = (r[17] as i64) < 0;  { let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
 if t { l = 0x003fb128; continue; }
r[20] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[22] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[1] = c.vi[17] as i32 as i64 as u64;
r[17] = (((r[17] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
fm::<6, 8>(c, 24, 30, c.vf[30]);
c.wr32((r[15] as u32).wrapping_add(0x00000010), r[17] as u32);
fm::<1, 14>(c, 2, 20, uycpu::vu0::bcast(c.vf[1][0]));
r[1] = r[1] & 0xe0;
let t = r[1] != 0u64;  uycpu::vu0::ftoi::<0, 15>(c, 3, 19);
 if t { l = 0x003fb128; continue; }
uycpu::vu0::ftoi::<0, 15>(c, 4, 20);
r[24] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[15] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
let t = (r[10] as i64) <= 0;   if t { l = 0x003fb128; continue; }
r[1] = c.vi[17] as i32 as i64 as u64;
r[2] = 0u64 | 0xe0;
r[1] = r[1] & 0xe0;
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[12] = a; h[12] = b; }
let t = r[1] != r[2];  { let (a, b) = { let v = c.vf[4]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[13] = a; h[13] = b; }
 if t { l = 0x003fb128; continue; }
let t = (r[11] as i64) <= 0;  fm::<6, 14>(c, 19, 19, uycpu::vu0::bcast(c.vf[1][0]));
 if t { l = 0x003fb128; continue; }
fm::<6, 14>(c, 20, 20, uycpu::vu0::bcast(c.vf[1][0]));
fm::<1, 4>(c, 14, 0, uycpu::vu0::bcast(c.vf[24][0]));
r[1] = 0x0000000045000000;
fm::<6, 1>(c, 21, 0, uycpu::vu0::bcast(c.vf[21][0]));
r[2] = 0x0000000045800000;
{ let (a, b) = uycpu::mmi::psrlw((r[12], h[12]), 2); r[12] = a; h[12] = b; }
c.vf[12] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psrlw((r[13], h[13]), 2); r[13] = a; h[13] = b; }
c.vf[13] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
fm::<6, 8>(c, 24, 24, uycpu::vu0::bcast(c.vf[21][3]));
r[1] = r[19] & 0x1;
let t = (r[1] as i64) > 0;  fm::<0, 14>(c, 21, 0, uycpu::vu0::bcast(c.vf[0][3]));
 if t { l = 0x003fab48; continue; }
{ let (a, b) = uycpu::mmi::psllw((r[12], h[12]), 12); r[2] = a; h[2] = b; }
c.wr64((r[16] as u32), 0u64);
c.vf[10] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyud((r[12], h[12]), (0u64, 0u64)); r[8] = a; h[8] = b; }
c.vf[11] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyud((r[13], h[13]), (0u64, 0u64)); r[11] = a; h[11] = b; }
uycpu::vu0::itof::<0, 14>(c, 10, 10);
r[10] = r[13] >> 32;
uycpu::vu0::itof::<0, 14>(c, 11, 11);
r[9] = (((r[13] as u32) >> 0) as i32 as i64 as u64);
l = 0x003fa438; continue; }
0x003fa438 => {
r[7] = r[12] >> 32;
uycpu::vu0::mov::<4>(c, 10, c.vf[11]);
l = 0x003fa440; continue; }
0x003fa440 => {
r[6] = (((r[12] as u32) >> 0) as i32 as i64 as u64);
uycpu::vu0::mov::<8>(c, 10, c.vf[11]);
l = 0x003fa448; continue; }
0x003fa448 => {
fm::<0, 14>(c, 2, 10, uycpu::vu0::bcast(c.vf[13][0]));
fm::<4, 14>(c, 1, 10, c.vf[31]);
fm::<0, 2>(c, 5, 31, c.vf[30]);
fm::<0, 14>(c, 4, 31, c.vf[0]);
fm::<5, 14>(c, 1, 1, c.vf[2]);
fm::<5, 2>(c, 4, 5, c.vf[1]);
fm::<4, 2>(c, 4, 31, c.vf[1]);
fm::<0, 14>(c, 3, 10, uycpu::vu0::bcast(c.vf[12][0]));
fm::<1, 14>(c, 1, 1, c.vf[4]);
fm::<6, 14>(c, 1, 1, c.vf[1]);
fm::<0, 4>(c, 32, 14, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 4>(c, 32, 21, c.vf[1]);
fm::<2, 4>(c, 1, 21, uycpu::vu0::bcast(c.vf[1][2]));
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[4] = a; h[4] = b; }
r[1] = (((r[6] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[2] = (((r[7] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = (r[4] as i64) >= 0;  r[3] = (((r[8] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x003fa4d8; continue; }
r[31] = k.pc(0x3fa498) as u64; { let v = c.vf[3]; c.wr128((r[15] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 grid(c, r);
let t = r[1] == 0u64;  c.wr32((r[15] as u32).wrapping_add(0x0000000c), r[1] as u32);
 if t { l = 0x003fa4d8; continue; }
let t = r[14] != 0u64;  r[15] = (((r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 if t { l = 0x003fa4d8; continue; }
r[14] = 0x0000000010000000;
r[2] = r[1] & 0xff;
r[14] = r[14] | 0xd400;
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000020), r[2] as u32);
r[1] = (((r[1] as u32) >> 8) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000080), 0u64 as u32);
r[1] = (((r[1] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000010), r[1] as u32);
r[1] = 0u64 | 0x100;
spr_to_reg(c, (r[14] as u32), r[1] as u32);
l = 0x003fa4d8; continue; }
0x003fa4d8 => {
fm::<0, 8>(c, 10, 10, c.vf[13]);
let t = r[6] != r[9];  r[6] = (((r[6] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003fa448; continue; }
fm::<0, 4>(c, 10, 10, uycpu::vu0::bcast(c.vf[13][0]));
let t = r[7] != r[10];  r[7] = (((r[7] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003fa440; continue; }
fm::<0, 2>(c, 10, 10, uycpu::vu0::bcast(c.vf[13][0]));
let t = r[8] != r[11];  r[8] = (((r[8] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003fa438; continue; }
r[25] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
r[23] = (((r[16] as u32).wrapping_add(0x00001000)) as i32 as i64 as u64);
c.wr32((r[15] as u32).wrapping_add(0x0000000c), 0u64 as u32);
l = 0x003fa518; continue; }
0x003fa518 => {
r[2] = c.rd32((r[25] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[14] = 0x0000000010000000;
r[1] = c.rd32((r[25] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
r[14] = r[14] | 0xd400;
let t = r[2] == 0u64;  { let (a, b) = c.rd128((r[25] as u32)); c.vf[23] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x003fab48; continue; }
r[23] = r[23] ^ 0x1000;
r[25] = (((r[25] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
l = 0x003fa538; continue; }
0x003fa538 => {
r[2] = spr_to_chcr(c, (r[14] as u32)) as i32 as i64 as u64;
r[2] = r[2] & 0x100;
let t = r[2] != 0u64;   if t { l = 0x003fa538; continue; }
let t = r[1] == 0u64;  r[3] = r[23] ^ 0x1000;
 if t { l = 0x003fa588; continue; }
r[2] = r[1] & 0xff;
c.wr64((r[3] as u32), 0u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000020), r[2] as u32);
r[1] = (((r[1] as u32) >> 8) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000080), r[3] as u32);
r[1] = (((r[1] as u32).wrapping_add((r[24] as u32))) as i32 as i64 as u64);
spr_to_reg(c, (r[14] as u32).wrapping_add(0x00000010), r[1] as u32);
r[1] = 0u64 | 0x100;
spr_to_reg(c, (r[14] as u32), r[1] as u32);
l = 0x003fa588; continue; }
0x003fa588 => {
r[15] = c.rd8((r[23] as u32).wrapping_add(0x00000002)) as u64;
fm::<1, 14>(c, 1, 19, c.vf[23]);
r[14] = (((r[23] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
fm::<1, 14>(c, 2, 20, c.vf[23]);
let t = r[15] == 0u64;  fm::<1, 14>(c, 22, 31, c.vf[23]);
 if t { l = 0x003fa518; continue; }
r[8] = 0x0000000000010000;
uycpu::vu0::ftoi::<0, 14>(c, 1, 1);
r[8] = r[8] | 0x101;
uycpu::vu0::ftoi::<0, 14>(c, 2, 2);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[11] = a; h[11] = b; }
{ let (a, b) = uycpu::mmi::pextlb((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlh((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[11], h[11])); r[11] = a; h[11] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::psllw((r[8], h[8]), 11); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psllw((r[8], h[8]), 15); r[10] = a; h[10] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[10], h[10])); r[10] = a; h[10] = b; }
r[13] = (((r[16] as u32).wrapping_add(0x00001fe0)) as i32 as i64 as u64);
r[1] = c.rd32((r[14] as u32)) as i32 as i64 as u64;
r[7] = 0x00000000000f0000;
r[2] = c.rd32((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[7] = r[7] | 0xfc00;
l = 0x003fa5e8; continue; }
0x003fa5e8 => {
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
r[14] = (((r[14] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[3] = r[1] & 0x3ff;
r[5] = r[2] & 0x3ff;
r[4] = r[1] & r[7];
r[6] = r[2] & r[7];
r[3] = (((r[3] as u32) << 6) as i32 as i64 as u64);
r[5] = (((r[5] as u32) << 6) as i32 as i64 as u64);
r[4] = r[4] << 12;
r[6] = r[6] << 12;
r[1] = (((r[1] as u32) >> 20) as i32 as i64 as u64);
r[2] = (((r[2] as u32) >> 20) as i32 as i64 as u64);
r[1] = r[1] << 36;
r[2] = r[2] << 36;
r[3] = r[3] | r[4];
r[5] = r[5] | r[6];
r[1] = r[1] | r[3];
r[2] = r[2] | r[5];
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[4] = a; h[4] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[3], h[3]), 16); r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[4], h[4]), 16); r[4] = a; h[4] = b; }
{ let (a, b) = uycpu::mmi::psubh((r[12], h[12]), (r[1], h[1])); r[5] = a; h[5] = b; }
c.vf[1] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psubh((r[12], h[12]), (r[2], h[2])); r[6] = a; h[6] = b; }
c.vf[2] = [r[4] as u32, (r[4] >> 32) as u32, h[4] as u32, (h[4] >> 32) as u32];
{ let (a, b) = uycpu::mmi::psubh((r[1], h[1]), (r[11], h[11])); r[3] = a; h[3] = b; }
r[1] = c.rd32((r[14] as u32)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::psubh((r[2], h[2]), (r[11], h[11])); r[4] = a; h[4] = b; }
r[2] = c.rd32((r[14] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
uycpu::vu0::itof::<0, 14>(c, 1, 1);
r[3] = r[3] & r[10];
uycpu::vu0::itof::<0, 14>(c, 2, 2);
r[4] = r[4] & r[10];
r[5] = r[5] & r[10];
r[6] = r[6] & r[10];
r[3] = r[3] >> 15;
r[4] = r[4] >> 15;
r[5] = r[5] >> 14;
r[6] = r[6] >> 14;
r[3] = r[3] | r[5];
r[4] = r[4] | r[6];
{ let v = c.vf[1]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[3], h[3])); r[3] = a; h[3] = b; }
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[4], h[4])); r[4] = a; h[4] = b; }
c.wr32((r[13] as u32).wrapping_add(0x0000000c), r[3] as u32);
r[15] = (((r[15] as u32).wrapping_add(0xfffffffe)) as i32 as i64 as u64);
let t = (r[15] as i64) > 0;  c.wr32((r[13] as u32).wrapping_add(0x0000001c), r[4] as u32);
 if t { l = 0x003fa5e8; continue; }
r[1] = c.rd8((r[23] as u32).wrapping_add(0x00000002)) as u64;
r[13] = (((r[16] as u32).wrapping_add(0x00002000)) as i32 as i64 as u64);
r[14] = c.rd8((r[23] as u32)) as u64;
r[15] = (((r[23] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
r[1] = (((r[1] as u32) << 2) as i32 as i64 as u64);
r[11] = 0x0000000000020000;
r[15] = (((r[15] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[14] = (((r[14] as u32) << 2) as i32 as i64 as u64);
r[5] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[11] = r[11] | 0x202;
r[12] = 0u64 | 0x700;
{ let (a, b) = uycpu::mmi::pcpyh((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
r[10] = (((r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003fa6f0; continue; }
0x003fa6f0 => {
let t = r[15] == r[14];  { let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (r[5], h[5])); r[1] = a; h[1] = b; }
 if t { l = 0x003faaf0; continue; }
{ let (a, b) = uycpu::mmi::psllw((r[1], h[1]), 4); r[1] = a; h[1] = b; }
r[6] = c.rd32((r[1] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[2] = r[1] >> 32;
r[7] = c.rd32((r[2] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[1], h[1]), (0u64, 0u64)); r[3] = a; h[3] = b; }
r[8] = c.rd32((r[3] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[4] = r[5] >> 48;
r[5] = c.rd32((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[6] = r[6] & r[7];
r[6] = r[6] & r[8];
r[15] = (((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = (r[6] as i64) > 0;  { let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
 if t { l = 0x003fa6f0; continue; }
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[19] & 0x80;
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[6] & r[4];
let t = (r[6] as i64) > 0;  { let (a, b) = c.rd128((r[3] as u32)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x003fa6f0; continue; }
fm::<1, 14>(c, 2, 2, c.vf[1]);
r[6] = r[19] & 0x20;
fm::<1, 14>(c, 3, 3, c.vf[1]);
r[6] = ((r[6] as i64) < 1) as u64;
fm::<1, 14>(c, 5, 22, c.vf[1]);
r[7] = (((r[19] as u32) >> 8) as i32 as i64 as u64);
fm::<1, 14>(c, 4, 22, c.vf[1]);
r[7] = r[7] ^ r[4];
uycpu::vu0::opmula(c, 3, 2);
r[7] = r[7] & 0x1f;
uycpu::vu0::opmsub(c, 6, 2, 3);
r[6] = r[6] | r[7];
let t = r[6] == 0u64;  fm::<0, 2>(c, 5, 5, c.vf[30]);
 if t { l = 0x003fa6f0; continue; }
fm::<6, 14>(c, 8, 4, c.vf[6]);
fm::<6, 14>(c, 9, 5, c.vf[6]);
fm::<6, 2>(c, 10, 30, c.vf[6]);
uycpu::vu0::abs::<2>(c, 13, 6);
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
fm::<2, 8>(c, 9, 21, uycpu::vu0::bcast(c.vf[9][2]));
{ let (a, b) = { let v = c.vf[13]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
fm::<1, 2>(c, 11, 4, c.vf[2]);
r[8] = 0x000000003f800000;
fm::<1, 2>(c, 12, 4, c.vf[3]);
r[6] = r[19] & 0x10;
let t = (r[6] as i64) > 0;  { let (a, b) = uycpu::mmi::prot3w((r[9], h[9])); r[9] = a; h[9] = b; }
 if t { l = 0x003fa7d8; continue; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
{ let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
r[6] = r[6] & r[7];
r[6] = r[6] << 32;
let t = (r[6] as i64) < 0;   if t { l = 0x003fa6f0; continue; }
l = 0x003fa7d8; continue; }
0x003fa7d8 => {
fm::<1, 2>(c, 13, 5, c.vf[2]);
r[9] = r[9] >> 32;
fm::<1, 2>(c, 14, 5, c.vf[3]);
r[9] = (((r[9] as u32).wrapping_sub((r[8] as u32))) as i32 as i64 as u64);
let t = (r[9] as i64) < 0;  uycpu::vu0::mov::<2>(c, 7, c.vf[0]);
 if t { l = 0x003fa928; continue; }
uycpu::vu0::qop::<0>(c, c.vf[8][0], c.vf[10][2]);
{ let (a, b) = { let v = c.vf[4]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[11]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let (a, b) = { let v = c.vf[12]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = { let v = c.vf[5]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::por((r[7], h[7]), (r[8], h[8])); r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[13]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
{ let (a, b) = uycpu::mmi::por((r[7], h[7]), (r[9], h[9])); r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[14]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[3] = a; h[3] = b; }
{ let (a, b) = uycpu::mmi::prot3w((r[7], h[7])); r[7] = a; h[7] = b; }
let t = (r[7] as i64) >= 0;  { let (a, b) = uycpu::mmi::pand((r[1], h[1]), (r[2], h[2])); r[7] = a; h[7] = b; }
 if t { l = 0x003fa988; continue; }
{ let (a, b) = uycpu::mmi::pand((r[7], h[7]), (r[3], h[3])); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::prot3w((r[7], h[7])); r[7] = a; h[7] = b; }
let t = (r[7] as i64) < 0;   if t { l = 0x003fa968; continue; }
fm::<6, 2>(c, 7, 30, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::mov::<12>(c, 5, c.vf[4]);
fm::<1, 2>(c, 5, 4, c.vf[7]);
fm::<1, 14>(c, 9, 3, c.vf[2]);
fm::<1, 14>(c, 10, 5, c.vf[2]);
uycpu::vu0::opmula(c, 5, 2);
uycpu::vu0::opmsub(c, 7, 2, 5);
uycpu::vu0::opmula(c, 3, 5);
uycpu::vu0::opmsub(c, 8, 5, 3);
uycpu::vu0::opmula(c, 10, 9);
uycpu::vu0::opmsub(c, 9, 9, 10);
fm::<6, 14>(c, 7, 7, c.vf[6]);
fm::<6, 14>(c, 8, 8, c.vf[6]);
fm::<6, 14>(c, 9, 9, c.vf[6]);
fm::<0, 4>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][0]));
fm::<2, 4>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 4>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][0]));
fm::<2, 4>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<0, 4>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][0]));
fm::<2, 4>(c, 9, 21, uycpu::vu0::bcast(c.vf[9][2]));
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
let t = (r[7] as i64) < 0;  { let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
 if t { l = 0x003fa8e0; continue; }
let t = (r[8] as i64) < 0;   if t { l = 0x003fa8d0; continue; }
let t = (r[9] as i64) < 0;   if t { l = 0x003fa8c0; continue; }
uycpu::vu0::mov::<14>(c, 7, c.vf[5]);
 l = 0x003fa928; continue;
}
0x003fa8c0 => {
fm::<0, 14>(c, 7, 2, c.vf[0]);
fm::<0, 14>(c, 8, 3, c.vf[0]);
 l = 0x003fa8e8; continue;
}
0x003fa8d0 => {
fm::<0, 14>(c, 7, 3, c.vf[0]);
fm::<0, 14>(c, 8, 0, c.vf[0]);
 l = 0x003fa8e8; continue;
}
0x003fa8e0 => {
fm::<0, 14>(c, 7, 0, c.vf[0]);
fm::<0, 14>(c, 8, 2, c.vf[0]);
l = 0x003fa8e8; continue; }
0x003fa8e8 => {
fm::<1, 14>(c, 9, 5, c.vf[7]);
fm::<1, 14>(c, 8, 8, c.vf[7]);
fm::<6, 14>(c, 10, 9, c.vf[8]);
fm::<6, 14>(c, 11, 8, c.vf[8]);
fm::<0, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 8>(c, 10, 21, uycpu::vu0::bcast(c.vf[10][2]));
fm::<0, 8>(c, 32, 11, uycpu::vu0::bcast(c.vf[11][1]));
fm::<2, 8>(c, 11, 21, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::qop::<0>(c, c.vf[10][0], c.vf[11][0]);
fm::<0, 8>(c, 10, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 10, 10, c.vf[0]);
fm::<5, 8>(c, 10, 10, c.vf[21]);
fm::<0, 14>(c, 32, 7, c.vf[0]);
fm::<2, 14>(c, 7, 8, uycpu::vu0::bcast(c.vf[10][0]));
l = 0x003fa928; continue; }
0x003fa928 => {
fm::<1, 2>(c, 13, 7, c.vf[4]);
fm::<5, 2>(c, 13, 13, c.vf[30]);
fm::<4, 2>(c, 13, 13, c.vf[0]);
fm::<0, 2>(c, 13, 13, c.vf[4]);
uycpu::vu0::mov::<12>(c, 13, c.vf[4]);
fm::<6, 14>(c, 8, 13, c.vf[6]);
fm::<6, 14>(c, 10, 6, c.vf[6]);
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<0, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 8>(c, 10, 21, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::qop::<0>(c, c.vf[8][0], c.vf[10][0]);
fm::<6, 14>(c, 8, 6, uycpu::vu0::bcast(c.vi[22]));
 l = 0x003fa9a8; continue;
}
0x003fa968 => {
fm::<6, 14>(c, 10, 6, c.vf[6]);
fm::<0, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 8>(c, 10, 21, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::qop::<0>(c, c.vf[9][0], c.vf[10][0]);
uycpu::vu0::mov::<14>(c, 13, c.vf[5]);
fm::<6, 14>(c, 8, 6, uycpu::vu0::bcast(c.vi[22]));
 l = 0x003fa9a8; continue;
}
0x003fa988 => {
fm::<6, 14>(c, 10, 6, c.vf[6]);
fm::<0, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 8>(c, 10, 21, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::qop::<0>(c, c.vf[8][0], c.vf[10][0]);
uycpu::vu0::mov::<14>(c, 13, c.vf[4]);
fm::<6, 14>(c, 8, 6, uycpu::vu0::bcast(c.vi[22]));
l = 0x003fa9a8; continue; }
0x003fa9a8 => {
fm::<6, 14>(c, 9, 8, c.vf[8]);
fm::<1, 14>(c, 4, 13, c.vf[8]);
fm::<0, 8>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][1]));
fm::<2, 8>(c, 5, 21, uycpu::vu0::bcast(c.vf[9][2]));
uycpu::vu0::opmula(c, 4, 2);
uycpu::vu0::opmsub(c, 7, 2, 4);
fm::<1, 14>(c, 10, 3, c.vf[2]);
fm::<1, 14>(c, 11, 4, c.vf[2]);
fm::<1, 8>(c, 12, 24, c.vf[5]);
uycpu::vu0::opmula(c, 3, 4);
uycpu::vu0::opmsub(c, 8, 4, 3);
uycpu::vu0::opmula(c, 11, 10);
uycpu::vu0::opmsub(c, 9, 10, 11);
{ let (a, b) = { let v = c.vf[12]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
fm::<6, 14>(c, 7, 7, c.vf[6]);
r[7] = r[7] << 32;
let t = (r[7] as i64) < 0;  fm::<6, 14>(c, 8, 8, c.vf[6]);
 if t { l = 0x003fa6f0; continue; }
fm::<6, 14>(c, 9, 9, c.vf[6]);
fm::<0, 4>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][0]));
fm::<2, 4>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 4>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][0]));
fm::<2, 4>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<0, 4>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][0]));
fm::<2, 4>(c, 9, 21, uycpu::vu0::bcast(c.vf[9][2]));
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
let t = (r[7] as i64) < 0;  { let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
 if t { l = 0x003faa58; continue; }
let t = (r[8] as i64) < 0;   if t { l = 0x003faa48; continue; }
let t = (r[9] as i64) >= 0;   if t { l = 0x003faac8; continue; }
fm::<0, 14>(c, 7, 2, c.vf[0]);
fm::<0, 14>(c, 8, 3, c.vf[0]);
 l = 0x003faa60; continue;
}
0x003faa48 => {
fm::<0, 14>(c, 7, 3, c.vf[0]);
fm::<0, 14>(c, 8, 0, c.vf[0]);
 l = 0x003faa60; continue;
}
0x003faa58 => {
fm::<0, 14>(c, 7, 0, c.vf[0]);
fm::<0, 14>(c, 8, 2, c.vf[0]);
l = 0x003faa60; continue; }
0x003faa60 => {
fm::<1, 14>(c, 9, 4, c.vf[7]);
fm::<1, 14>(c, 8, 8, c.vf[7]);
fm::<6, 14>(c, 10, 9, c.vf[8]);
fm::<6, 14>(c, 11, 8, c.vf[8]);
fm::<0, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 8>(c, 10, 21, uycpu::vu0::bcast(c.vf[10][2]));
fm::<0, 8>(c, 32, 11, uycpu::vu0::bcast(c.vf[11][1]));
fm::<2, 8>(c, 11, 21, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::qop::<0>(c, c.vf[10][0], c.vf[11][0]);
fm::<0, 8>(c, 10, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 10, 10, c.vf[0]);
fm::<5, 8>(c, 10, 10, c.vf[21]);
fm::<0, 14>(c, 32, 7, c.vf[0]);
fm::<2, 14>(c, 4, 8, uycpu::vu0::bcast(c.vf[10][0]));
fm::<1, 14>(c, 7, 4, c.vf[13]);
fm::<6, 14>(c, 8, 7, c.vf[7]);
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 5, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<1, 8>(c, 8, 24, c.vf[5]);
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
r[8] = r[8] << 32;
let t = (r[8] as i64) < 0;   if t { l = 0x003fa6f0; continue; }
l = 0x003faac8; continue; }
0x003faac8 => {
r[21] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<0, 14>(c, 1, 1, c.vf[23]);
r[22] = (((r[4] as u32).wrapping_add(0x00001000)) as i32 as i64 as u64);
fm::<6, 8>(c, 24, 5, uycpu::vu0::bcast(c.vf[21][3]));
fm::<0, 14>(c, 26, 6, c.vf[0]);
fm::<0, 14>(c, 25, 4, c.vf[1]);
fm::<0, 14>(c, 27, 0, c.vf[1]);
fm::<0, 14>(c, 28, 2, c.vf[1]);
fm::<0, 14>(c, 29, 3, c.vf[1]);
 l = 0x003fa6f0; continue;
}
0x003faaf0 => {
let t = r[20] != 0u64;   if t { l = 0x003faec0; continue; }
let t = r[10] == 0u64;  r[3] = c.rd8((r[23] as u32).wrapping_add(0x00000003)) as u64;
 if t { l = 0x003fa518; continue; }
r[1] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[2] = (((r[15] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
let t = r[3] == 0u64;  r[14] = (((r[3] as u32) << 2) as i32 as i64 as u64);
 if t { l = 0x003fa518; continue; }
l = 0x003fab10; continue; }
0x003fab10 => {
r[4] = c.rd8((r[1] as u32).wrapping_add(0x00000002)) as u64;
r[3] = (((r[3] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
r[5] = c.rd8((r[2] as u32)) as u64;
r[2] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr8((r[1] as u32).wrapping_add(0x00000001), r[4] as u8);
c.wr8((r[1] as u32).wrapping_add(0x00000002), r[5] as u8);
let t = r[3] != 0u64;  r[1] = (((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 if t { l = 0x003fab10; continue; }
r[5] = c.rd32((r[10] as u32)) as i32 as i64 as u64;
r[15] = (((r[10] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[10] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
 l = 0x003fa6f0; continue;
}
0x003fab48 => {
uycpu::vu0::ftoi::<0, 14>(c, 1, 19);
r[15] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x3fab4c))) as i32 as i64 as u64;
uycpu::vu0::ftoi::<0, 14>(c, 2, 20);
r[21] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[4] = a; h[4] = b; }
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[15], h[15]), (0u64, 0u64)); r[15] = a; h[15] = b; }
r[3] = (((r[4] as u32) >> 14) as i32 as i64 as u64);
r[4] = r[4] >> 46;
r[5] = (((r[6] as u32) >> 14) as i32 as i64 as u64);
r[6] = r[6] >> 46;
r[25] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
r[7] = sx(k.hi(c, 0x3fab7c));
r[7] = (((r[7] as u32).wrapping_add(k.lo(c, 0x3fab80))) as i32 as i64 as u64);
r[2] = (((r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003fab88; continue; }
0x003fab88 => {
r[1] = (((r[3] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003fab90; continue; }
0x003fab90 => {
r[8] = (((r[1] as u32) << 2) as i32 as i64 as u64);
r[9] = (((r[2] as u32) << 8) as i32 as i64 as u64);
r[8] = (((r[8] as u32).wrapping_add((r[9] as u32))) as i32 as i64 as u64);
r[8] = (((r[8] as u32).wrapping_add((r[7] as u32))) as i32 as i64 as u64);
r[9] = c.rd16((r[8] as u32)) as u64;
r[10] = (((r[7] as u32).wrapping_add(0x00004000)) as i32 as i64 as u64);
r[8] = c.rd8((r[8] as u32).wrapping_add(0x00000002)) as u64;
r[9] = (((r[9] as u32) << 5) as i32 as i64 as u64);
r[8] = (((r[8] as u32) << 1) as i32 as i64 as u64);
r[9] = (((r[9] as u32).wrapping_add((r[10] as u32))) as i32 as i64 as u64);
c.wr32((r[25] as u32), r[9] as u32);
r[8] = (((r[8] as u32).wrapping_add((r[9] as u32))) as i32 as i64 as u64);
c.wr32((r[25] as u32).wrapping_add(0x00000004), r[8] as u32);
r[25] = (((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = r[1] != r[5];  r[1] = (((r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003fab90; continue; }
let t = r[2] != r[6];  r[2] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003fab88; continue; }
c.wr64((r[25] as u32), 0u64);
r[25] = (((r[16] as u32).wrapping_add(0x000037f8)) as i32 as i64 as u64);
l = 0x003fabe0; continue; }
0x003fabe0 => {
r[23] = c.rd32((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[25] = (((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = r[23] == 0u64;   if t { l = 0x003fb128; continue; }
l = 0x003fabf0; continue; }
0x003fabf0 => {
r[24] = c.rd16((r[23] as u32)) as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[15], h[15]), (0u64, 0u64)); r[2] = a; h[2] = b; }
r[1] = c.rd32((r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[24] = (((r[24] as u32) << 8) as i32 as i64 as u64);
let t = r[23] == r[1];  r[24] = (((r[24] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
 if t { l = 0x003fabe0; continue; }
{ let (a, b) = c.rd128((r[24] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[23] = (((r[23] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
let t = r[24] == r[18];   if t { l = 0x003fabf0; continue; }
fm::<0, 8>(c, 3, 30, uycpu::vu0::bcast(c.vf[1][3]));
fm::<1, 12>(c, 2, 1, c.vf[31]);
r[20] = c.rd32((r[24] as u32).wrapping_add(0x00000098)) as i32 as i64 as u64;
fm::<1, 2>(c, 4, 1, uycpu::vu0::bcast(c.vf[3][0]));
fm::<0, 2>(c, 5, 1, uycpu::vu0::bcast(c.vf[3][0]));
fm::<6, 8>(c, 3, 3, c.vf[3]);
fm::<6, 12>(c, 2, 2, c.vf[2]);
r[5] = r[19] & 0x4;
fm::<1, 2>(c, 4, 4, c.vf[31]);
r[5] = ((r[5] as i64) < 1) as u64;
fm::<1, 2>(c, 5, 5, c.vf[31]);
r[5] = (((r[5] as u32) << 1) as i32 as i64 as u64);
fm::<1, 8>(c, 32, 2, c.vf[3]);
r[10] = sx(k.hi(c, 0x3fac4c));
r[10] = (((r[10] as u32).wrapping_add(k.lo(c, 0x3fac50))) as i32 as i64 as u64);
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][1]));
fm::<1, 2>(c, 4, 4, c.vf[30]);
r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000a0)) as i32 as i64 as u64;
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[5] = (((r[5] as u32).wrapping_add((r[20] as u32))) as i32 as i64 as u64);
let t = r[3] == r[17];  { let (a, b) = { let v = c.vf[5]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[2] = a; h[2] = b; }
 if t { l = 0x003fabf0; continue; }
let t = r[20] == 0u64;  r[1] = r[1] << 32;
 if t { l = 0x003fabf0; continue; }
{ let (a, b) = { let v = c.vf[4]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[4] = a; h[4] = b; }
let t = (r[1] as i64) > 0;  { let (a, b) = uycpu::mmi::prot3w((r[2], h[2])); r[2] = a; h[2] = b; }
 if t { l = 0x003fabf0; continue; }
let t = (r[2] as i64) <= 0;  { let (a, b) = uycpu::mmi::prot3w((r[4], h[4])); r[4] = a; h[4] = b; }
 if t { l = 0x003fabf0; continue; }
let t = (r[4] as i64) >= 0;  r[1] = r[19] & 0x2;
 if t { l = 0x003fabf0; continue; }
r[5] = c.rd16((r[5] as u32)) as u64;
r[2] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[1] = ((r[1] as i64) < 1) as u64;
let t = r[5] == 0u64;  c.wr32((r[24] as u32).wrapping_add(0x000000a0), r[17] as u32);
 if t { l = 0x003fad28; continue; }
let t = r[1] == r[2];  r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000ac)) as i32 as i64 as u64;
 if t { l = 0x003fabf0; continue; }
r[1] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[2] = ((0u32.wrapping_add(0x00000008)) as i32 as i64 as u64);
r[16] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
l = 0x003facc8; continue; }
0x003facc8 => {
r[6] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
r[1] = (((r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[6] == r[3];   if t { l = 0x003fad28; continue; }
let t = (r[2] as i64) > 0;  r[16] = (((r[16] as u32).wrapping_add(0x00000800)) as i32 as i64 as u64);
 if t { l = 0x003facc8; continue; }
r[2] = c.rd32((r[10] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[6] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[1] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[4] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[1] = r[1] & 0x7;
c.wr32((r[10] as u32).wrapping_add(0x0000000c), r[1] as u32);
r[1] = (((r[2] as u32) << 3) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 11) as i32 as i64 as u64);
r[4] = (((r[4] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[6] = (((r[6] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
c.wr32((r[4] as u32), r[3] as u32);
c.wr32((r[4] as u32).wrapping_add(0x00000004), r[7] as u32);
r[31] = k.pc(0x3fad20) as u64; r[4] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 out(c, r, h, k.jal(c, 0x3fad18), k.pc(0x3fad18), k.pc(0x3fad20));
r[16] = 0x0000000070000000;
l = 0x003fad28; continue; }
0x003fad28 => {
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x00000010)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000c0)); c.vf[16] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000d0)); c.vf[17] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000e0)); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[2] = c.rd32((r[24] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
r[15] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[1] = 0x0000000044800000;
r[3] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[13] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
let t = r[15] == 0u64;  fm::<6, 14>(c, 15, 15, uycpu::vu0::bcast(c.vf[1][0]));
 if t { l = 0x003faec8; continue; }
let t = (r[3] as i64) < 0;   if t { l = 0x003faec8; continue; }
r[13] = 0x0000000070000000;
fm::<6, 14>(c, 3, 16, uycpu::vu0::bcast(c.vf[13][0]));
r[13] = r[13] | 0x1fe0;
fm::<6, 14>(c, 4, 17, uycpu::vu0::bcast(c.vf[13][0]));
r[11] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
fm::<6, 14>(c, 5, 18, uycpu::vu0::bcast(c.vf[13][0]));
r[12] = 0x0000000000800000;
fm::<1, 14>(c, 22, 31, c.vf[15]);
r[12] = r[12] | 0x8080;
uycpu::vu0::mov::<14>(c, 23, c.vf[15]);
{ let (a, b) = uycpu::mmi::pextlb((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
fm::<0, 14>(c, 12, 22, uycpu::vu0::bcast(c.vf[30][0]));
r[11] = (((r[11] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
fm::<1, 14>(c, 11, 22, uycpu::vu0::bcast(c.vf[30][0]));
r[14] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
fm::<0, 2>(c, 12, 12, c.vf[30]);
r[1] = c.rd64((r[14] as u32));
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000008));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
l = 0x003fadd0; continue; }
0x003fadd0 => {
uycpu::vu0::itof::<0, 14>(c, 1, 1);
r[1] = c.rd64((r[14] as u32).wrapping_add(0x00000010));
uycpu::vu0::itof::<0, 14>(c, 2, 2);
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000018));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 1, 5, uycpu::vu0::bcast(c.vf[1][2]));
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 2, 5, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 14>(c, 6, 1, c.vf[11]);
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
fm::<1, 14>(c, 7, 12, c.vf[1]);
r[14] = (((r[14] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let v = c.vf[1]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
fm::<1, 14>(c, 8, 2, c.vf[11]);
fm::<1, 14>(c, 9, 12, c.vf[2]);
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = { let v = c.vf[6]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pand((r[6], h[6]), (r[12], h[12])); r[6] = a; h[6] = b; }
{ let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::pand((r[7], h[7]), (r[12], h[12])); r[7] = a; h[7] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[8], h[8]), (r[12], h[12])); r[8] = a; h[8] = b; }
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[9], h[9]), (r[12], h[12])); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[6], h[6]), 31); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[7], h[7]), 30); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[8], h[8]), 31); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 30); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::por((r[6], h[6]), (r[7], h[7])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::por((r[8], h[8]), (r[9], h[9])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
c.wr32((r[13] as u32).wrapping_add(0x0000000c), r[6] as u32);
r[15] = (((r[15] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
let t = (r[15] as i64) > 0;  c.wr32((r[13] as u32).wrapping_add(0x0000001c), r[8] as u32);
 if t { l = 0x003fadd0; continue; }
r[1] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[13] = 0x0000000070000000;
r[14] = c.rd32((r[20] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[15] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
r[15] = (((r[15] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[11] = 0x0000000000020000;
r[5] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[11] = r[11] | 0x202;
r[12] = 0u64 | 0x700;
{ let (a, b) = uycpu::mmi::pcpyh((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
r[13] = r[13] | 0x2000;
 l = 0x003fa6f0; continue;
}
0x003faec0 => {
r[2] = c.rd32((r[24] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
c.vf[13] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
l = 0x003faec8; continue; }
0x003faec8 => {
r[1] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[2] = r[19] & 0x2;
let t = (r[2] as i64) > 0;  r[15] = (((r[20] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
 if t { l = 0x003fabf0; continue; }
let t = r[1] == 0u64;  r[13] = r[19] & 0x4;
 if t { l = 0x003fabf0; continue; }
r[14] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[13] = ((r[13] as i64) < 1) as u64;
r[12] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
r[13] = (((r[13] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
l = 0x003faef0; continue; }
0x003faef0 => {
let t = (r[14] as i64) < 0;  r[1] = r[12] & 0x1;
 if t { l = 0x003fabf0; continue; }
r[12] = (((r[12] as u32) >> 1) as i32 as i64 as u64);
r[14] = c.rd16((r[15] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
r[2] = c.rd8((r[15] as u32)) as i8 as i64 as u64;
r[15] = (((r[15] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
let t = (r[1] as i64) > 0;  r[1] = r[13] & r[14];
 if t { l = 0x003faef0; continue; }
let t = r[1] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003faef0; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003fb058; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003fb020; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003faff0; continue; }
r[1] = c.rd16((r[15] as u32).wrapping_add(0xffffffe4)) as i16 as i64 as u64;
r[2] = c.rd16((r[15] as u32).wrapping_add(0xffffffe6)) as i16 as i64 as u64;
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[16] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[2] = (((r[16] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 14>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 1>(c, 3, 3, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[2][2]));
fm::<2, 14>(c, 2, 15, uycpu::vu0::bcast(c.vf[21][0]));
fm::<1, 14>(c, 2, 2, c.vf[1]);
fm::<1, 14>(c, 4, 31, c.vf[1]);
l = 0x003faf88; continue; }
0x003faf88 => {
fm::<6, 12>(c, 6, 4, c.vf[2]);
fm::<6, 12>(c, 7, 2, c.vf[2]);
fm::<0, 8>(c, 6, 6, uycpu::vu0::bcast(c.vf[6][1]));
fm::<0, 8>(c, 7, 7, uycpu::vu0::bcast(c.vf[7][1]));
uycpu::vu0::qop::<0>(c, c.vf[6][0], c.vf[7][0]);
uycpu::vu0::abs::<8>(c, 7, 7);
fm::<1, 8>(c, 7, 21, c.vf[7]);
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
r[7] = r[7] << 32;
let t = (r[7] as i64) >= 0;   if t { l = 0x003fafd8; continue; }
fm::<0, 8>(c, 6, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 6, 6, c.vf[0]);
fm::<5, 8>(c, 6, 6, c.vf[21]);
fm::<0, 14>(c, 32, 1, c.vf[0]);
fm::<2, 14>(c, 1, 2, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::mov::<1>(c, 1, c.vf[3]);
 l = 0x003fb070; continue;
}
0x003fafd8 => {
fm::<0, 14>(c, 2, 2, c.vf[1]);
fm::<4, 2>(c, 1, 1, c.vf[31]);
fm::<5, 2>(c, 1, 2, c.vf[1]);
uycpu::vu0::mov::<1>(c, 1, c.vf[3]);
 l = 0x003fb070; continue;
}
0x003faff0 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[13][0]));
l = 0x003fb000; continue; }
0x003fb000 => {
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[0][3]));
fm::<6, 14>(c, 2, 18, uycpu::vu0::bcast(c.vf[2][1]));
uycpu::vu0::mov::<1>(c, 3, c.vf[1]);
fm::<1, 14>(c, 4, 31, c.vf[1]);
 l = 0x003faf88; continue;
}
0x003fb020 => {
r[1] = c.rd32((r[15] as u32).wrapping_add(0xffffffe4)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[1] as u32).wrapping_add((r[16] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::mov::<1>(c, 1, c.vf[2]);
fm::<0, 14>(c, 1, 1, c.vf[3]);
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
 l = 0x003fb070; continue;
}
0x003fb058 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
l = 0x003fb070; continue; }
0x003fb070 => {
fm::<1, 2>(c, 4, 1, c.vf[31]);
uycpu::vu0::mov::<12>(c, 6, c.vf[31]);
fm::<0, 8>(c, 3, 30, uycpu::vu0::bcast(c.vf[1][3]));
fm::<5, 2>(c, 4, 4, c.vf[30]);
fm::<4, 2>(c, 4, 4, c.vf[0]);
fm::<0, 2>(c, 6, 31, c.vf[4]);
fm::<1, 14>(c, 2, 1, c.vf[6]);
fm::<6, 8>(c, 3, 3, c.vf[3]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
fm::<0, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 8>(c, 5, 3, c.vf[2]);
uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[2][0]);
{ let (a, b) = { let v = c.vf[5]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) < 0;   if t { l = 0x003faef0; continue; }
fm::<0, 8>(c, 2, 0, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[2][0]);
fm::<6, 8>(c, 2, 2, c.vf[21]);
fm::<5, 8>(c, 3, 2, uycpu::vu0::bcast(c.vf[1][3]));
fm::<0, 8>(c, 2, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<1, 14>(c, 5, 6, c.vf[1]);
fm::<6, 14>(c, 5, 5, uycpu::vu0::bcast(c.vf[2][0]));
fm::<6, 14>(c, 5, 5, uycpu::vu0::bcast(c.vf[3][0]));
fm::<0, 14>(c, 1, 1, c.vf[5]);
fm::<1, 14>(c, 2, 1, c.vf[6]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
fm::<0, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 8>(c, 3, 24, c.vf[2]);
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) < 0;   if t { l = 0x003faef0; continue; }
r[21] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[22] = (((r[15] as u32).wrapping_add(0xffffffe0)) as i32 as i64 as u64);
r[22] = ((0u32.wrapping_sub((r[22] as u32))) as i32 as i64 as u64);
fm::<6, 8>(c, 24, 2, c.vf[21]);
fm::<0, 14>(c, 25, 0, c.vf[1]);
fm::<0, 14>(c, 26, 0, c.vf[5]);
 l = 0x003faef0; continue;
}
0x003fb128 => {
let t = r[22] == 0u64;  r[2] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x003fb1a8; continue; }
uycpu::vu0::qop::<2>(c, c.vf[30][0], c.vf[24][0]);
r[2] = ((0u32.wrapping_add(0x00000001)) as i32 as i64 as u64);
fm::<1, 14>(c, 2, 31, c.vf[25]);
r[1] = 0x000000003a800000;
c.vf[3] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
fm::<1, 14>(c, 1, 25, c.vf[31]);
fm::<6, 14>(c, 25, 25, uycpu::vu0::bcast(c.vf[3][0]));
fm::<6, 14>(c, 27, 27, uycpu::vu0::bcast(c.vf[3][0]));
fm::<6, 14>(c, 28, 28, uycpu::vu0::bcast(c.vf[3][0]));
fm::<6, 14>(c, 29, 29, uycpu::vu0::bcast(c.vf[3][0]));
r[1] = sx(k.hi(c, 0x3fb160));
r[1] = (((r[1] as u32).wrapping_add(k.lo(c, 0x3fb164))) as i32 as i64 as u64);
{ let v = c.vf[25]; c.wr128((r[1] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[26]; c.wr128((r[1] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[27]; c.wr128((r[1] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[28]; c.wr128((r[1] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[29]; c.wr128((r[1] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr32((r[1] as u32).wrapping_add(0x00000018), r[21] as u32);
c.wr32((r[1] as u32).wrapping_add(0x0000001c), r[22] as u32);
fm::<5, 2>(c, 1, 1, c.vf[30]);
fm::<4, 2>(c, 1, 1, c.vf[0]);
fm::<0, 2>(c, 2, 2, c.vf[1]);
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vi[22]));
fm::<1, 2>(c, 2, 2, c.vf[1]);
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[3][0]));
fm::<0, 14>(c, 2, 2, c.vf[25]);
{ let v = c.vf[2]; c.wr128((r[1] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
l = 0x003fb1a8; continue; }
0x003fb1a8 => {
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
let t = (r[31] as u32); { let (a, b) = uycpu::mmi::pcpyud((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
 c.ret = t; return;
}
_ => unreachable!("0x3fa2d8: no label {l:08x} in it"), } } }

#[allow(unused_mut, unused_variables, unreachable_code, unused_assignments, unused_parens, clippy::all)]
#[inline(always)]
fn b3fb1d0(c: &mut Cpu, k: F, r: &mut [u64; 32], h: &mut [u64; 32]) {
let mut l: u32 = 0x003fb1d0;
loop { match l {
0x003fb1d0 => {
r[1] = 0x0000000044800000;
r[15] = sx(k.hi(c, 0x3fb1d4));
r[15] = (((r[15] as u32).wrapping_add(k.lo(c, 0x3fb1d8))) as i32 as i64 as u64);
r[2] = 0x0000000044800000;
{ let (a, b) = uycpu::mmi::pcpyld((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
{ let (a, b) = c.rd128((r[4] as u32)); c.vf[31] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pcpyld((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
r[3] = c.f[12] as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyld((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
r[17] = c.rd32((r[15] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyld((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
c.vf[30] = [r[3] as u32, (r[3] >> 32) as u32, h[3] as u32, (h[3] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pcpyld((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
fm::<1, 14>(c, 19, 31, uycpu::vu0::bcast(c.vf[30][0]));
r[18] = (((r[6] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<0, 14>(c, 20, 31, uycpu::vu0::bcast(c.vf[30][0]));
r[19] = (((r[5] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<6, 14>(c, 31, 31, uycpu::vu0::bcast(c.vf[1][0]));
r[20] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<6, 8>(c, 30, 30, c.vf[1]);
r[21] = (((r[7] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
fm::<1, 14>(c, 3, 19, c.vf[0]);
r[16] = 0x0000000070000000;
{ let (a, b) = uycpu::mmi::pcpyld((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyld((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
let t = (r[17] as i64) < 0;  { let (a, b) = uycpu::mmi::pcpyld((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
 if t { l = 0x003fbab8; continue; }
r[22] = sx(k.hi(c, 0x3fb250));
r[22] = (((r[22] as u32).wrapping_add(k.lo(c, 0x3fb254))) as i32 as i64 as u64);
r[1] = c.vi[17] as i32 as i64 as u64;
r[17] = (((r[17] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
c.wr32((r[15] as u32).wrapping_add(0x00000010), r[17] as u32);
fm::<1, 14>(c, 2, 20, uycpu::vu0::bcast(c.vf[2][0]));
r[1] = r[1] & 0xe0;
uycpu::vu0::ftoi::<0, 15>(c, 19, 19);
let t = r[1] != 0u64;  uycpu::vu0::ftoi::<0, 15>(c, 20, 20);
 if t { l = 0x003fbab8; continue; }
let t = (r[3] as i64) <= 0;   if t { l = 0x003fbab8; continue; }
r[1] = c.vi[17] as i32 as i64 as u64;
r[2] = 0u64 | 0xe0;
fm::<6, 8>(c, 24, 30, c.vf[30]);
fm::<0, 14>(c, 21, 0, uycpu::vu0::bcast(c.vf[0][3]));
r[1] = r[1] & 0xe0;
{ let (a, b) = { let v = c.vf[19]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[12] = a; h[12] = b; }
let t = r[1] != r[2];  { let (a, b) = { let v = c.vf[20]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[13] = a; h[13] = b; }
 if t { l = 0x003fbab8; continue; }
r[15] = c.rd32((r[28] as u32).wrapping_add(k.lo(c, 0x3fb2a8))) as i32 as i64 as u64;
r[25] = (((r[16] as u32).wrapping_add(0x00003800)) as i32 as i64 as u64);
r[3] = (((r[12] as u32) >> 4) as i32 as i64 as u64);
r[4] = r[12] >> 36;
r[5] = (((r[13] as u32) >> 4) as i32 as i64 as u64);
r[6] = r[13] >> 36;
r[2] = (((r[4] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
r[7] = sx(k.hi(c, 0x3fb2c4));
r[7] = (((r[7] as u32).wrapping_add(k.lo(c, 0x3fb2c8))) as i32 as i64 as u64);
{ let (a, b) = uycpu::mmi::pcpyld((r[15], h[15]), (0u64, 0u64)); r[15] = a; h[15] = b; }
l = 0x003fb2d0; continue; }
0x003fb2d0 => {
r[1] = (((r[3] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
l = 0x003fb2d8; continue; }
0x003fb2d8 => {
r[8] = (((r[1] as u32) << 2) as i32 as i64 as u64);
r[9] = (((r[2] as u32) << 8) as i32 as i64 as u64);
r[8] = (((r[8] as u32).wrapping_add((r[9] as u32))) as i32 as i64 as u64);
r[8] = (((r[8] as u32).wrapping_add((r[7] as u32))) as i32 as i64 as u64);
r[9] = c.rd16((r[8] as u32)) as u64;
r[10] = (((r[7] as u32).wrapping_add(0x00004000)) as i32 as i64 as u64);
r[8] = c.rd8((r[8] as u32).wrapping_add(0x00000002)) as u64;
r[9] = (((r[9] as u32) << 5) as i32 as i64 as u64);
r[8] = (((r[8] as u32) << 1) as i32 as i64 as u64);
r[9] = (((r[9] as u32).wrapping_add((r[10] as u32))) as i32 as i64 as u64);
c.wr32((r[25] as u32), r[9] as u32);
r[8] = (((r[8] as u32).wrapping_add((r[9] as u32))) as i32 as i64 as u64);
c.wr32((r[25] as u32).wrapping_add(0x00000004), r[8] as u32);
r[25] = (((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = r[1] != r[5];  r[1] = (((r[1] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003fb2d8; continue; }
let t = r[2] != r[6];  r[2] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
 if t { l = 0x003fb2d0; continue; }
c.wr64((r[25] as u32), 0u64);
r[25] = (((r[16] as u32).wrapping_add(0x000037f8)) as i32 as i64 as u64);
l = 0x003fb328; continue; }
0x003fb328 => {
r[23] = c.rd32((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[25] = (((r[25] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
let t = r[23] == 0u64;   if t { l = 0x003fb9d0; continue; }
l = 0x003fb338; continue; }
0x003fb338 => {
r[24] = c.rd16((r[23] as u32)) as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[15], h[15]), (0u64, 0u64)); r[2] = a; h[2] = b; }
r[1] = c.rd32((r[25] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[24] = (((r[24] as u32) << 8) as i32 as i64 as u64);
let t = r[23] == r[1];  r[24] = (((r[24] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
 if t { l = 0x003fb328; continue; }
{ let (a, b) = c.rd128((r[24] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[23] = (((r[23] as u32).wrapping_add(0x00000002)) as i32 as i64 as u64);
let t = r[24] == r[18];   if t { l = 0x003fb338; continue; }
fm::<0, 8>(c, 3, 30, uycpu::vu0::bcast(c.vf[1][3]));
fm::<1, 14>(c, 2, 1, c.vf[31]);
r[20] = c.rd32((r[24] as u32).wrapping_add(0x00000098)) as i32 as i64 as u64;
fm::<6, 8>(c, 3, 3, c.vf[3]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
r[5] = r[19] & 0x4;
r[5] = ((r[5] as i64) < 1) as u64;
r[10] = sx(k.hi(c, 0x3fb37c));
r[10] = (((r[10] as u32).wrapping_add(k.lo(c, 0x3fb380))) as i32 as i64 as u64);
r[5] = (((r[5] as u32) << 1) as i32 as i64 as u64);
fm::<1, 8>(c, 32, 2, c.vf[3]);
fm::<2, 8>(c, 32, 21, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000a0)) as i32 as i64 as u64;
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[5] = (((r[5] as u32).wrapping_add((r[20] as u32))) as i32 as i64 as u64);
let t = r[3] == r[17];   if t { l = 0x003fb338; continue; }
let t = r[20] == 0u64;  r[1] = r[1] << 32;
 if t { l = 0x003fb338; continue; }
let t = (r[1] as i64) > 0;   if t { l = 0x003fb338; continue; }
r[5] = c.rd16((r[5] as u32)) as u64;
r[3] = c.rd32((r[24] as u32).wrapping_add(0x000000ac)) as i32 as i64 as u64;
let t = r[5] == 0u64;  c.wr32((r[24] as u32).wrapping_add(0x000000a0), r[17] as u32);
 if t { l = 0x003fb438; continue; }
r[1] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[2] = ((0u32.wrapping_add(0x00000008)) as i32 as i64 as u64);
r[16] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
l = 0x003fb3d8; continue; }
0x003fb3d8 => {
r[6] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
r[1] = (((r[1] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
let t = r[6] == r[3];   if t { l = 0x003fb438; continue; }
let t = (r[2] as i64) > 0;  r[16] = (((r[16] as u32).wrapping_add(0x00000800)) as i32 as i64 as u64);
 if t { l = 0x003fb3d8; continue; }
r[2] = c.rd32((r[10] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[6] = c.rd32((r[10] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[1] = (((r[2] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[4] = c.rd32((r[10] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[1] = r[1] & 0x7;
c.wr32((r[10] as u32).wrapping_add(0x0000000c), r[1] as u32);
r[1] = (((r[2] as u32) << 3) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 11) as i32 as i64 as u64);
r[4] = (((r[4] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[6] = (((r[6] as u32).wrapping_add((r[2] as u32))) as i32 as i64 as u64);
c.wr32((r[4] as u32), r[3] as u32);
c.wr32((r[4] as u32).wrapping_add(0x00000004), r[7] as u32);
r[31] = k.pc(0x3fb430) as u64; r[4] = (((r[24] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 out(c, r, h, k.jal(c, 0x3fb428), k.pc(0x3fb428), k.pc(0x3fb430));
r[16] = 0x0000000070000000;
l = 0x003fb438; continue; }
0x003fb438 => {
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x00000010)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000c0)); c.vf[16] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000d0)); c.vf[17] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[24] as u32).wrapping_add(0x000000e0)); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[2] = c.rd32((r[24] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
r[15] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[1] = 0x0000000044800000;
r[3] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[13] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
let t = r[15] == 0u64;  fm::<6, 14>(c, 15, 15, uycpu::vu0::bcast(c.vf[1][0]));
 if t { l = 0x003fb7c8; continue; }
let t = (r[3] as i64) < 0;   if t { l = 0x003fb7c8; continue; }
r[13] = 0x0000000070000000;
fm::<6, 14>(c, 3, 16, uycpu::vu0::bcast(c.vf[13][0]));
r[13] = r[13] | 0x1fe0;
fm::<6, 14>(c, 4, 17, uycpu::vu0::bcast(c.vf[13][0]));
r[11] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
fm::<6, 14>(c, 5, 18, uycpu::vu0::bcast(c.vf[13][0]));
r[12] = 0x0000000000800000;
fm::<1, 14>(c, 22, 31, c.vf[15]);
r[12] = r[12] | 0x8080;
uycpu::vu0::mov::<14>(c, 23, c.vf[15]);
{ let (a, b) = uycpu::mmi::pextlb((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (0u64, 0u64)); r[12] = a; h[12] = b; }
fm::<1, 14>(c, 11, 22, uycpu::vu0::bcast(c.vf[30][0]));
r[11] = (((r[11] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
fm::<0, 14>(c, 12, 22, uycpu::vu0::bcast(c.vf[30][0]));
r[14] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
r[1] = c.rd64((r[14] as u32));
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000008));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
l = 0x003fb4d8; continue; }
0x003fb4d8 => {
uycpu::vu0::itof::<0, 14>(c, 1, 1);
r[1] = c.rd64((r[14] as u32).wrapping_add(0x00000010));
uycpu::vu0::itof::<0, 14>(c, 2, 2);
r[2] = c.rd64((r[14] as u32).wrapping_add(0x00000018));
{ let (a, b) = uycpu::mmi::pextlh((r[1], h[1]), (0u64, 0u64)); r[1] = a; h[1] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[2], h[2]), (0u64, 0u64)); r[2] = a; h[2] = b; }
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 1, 5, uycpu::vu0::bcast(c.vf[1][2]));
fm::<6, 14>(c, 32, 3, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 4, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 2, 5, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 14>(c, 6, 1, c.vf[11]);
r[13] = (((r[13] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
fm::<1, 14>(c, 7, 12, c.vf[1]);
r[14] = (((r[14] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
{ let v = c.vf[1]; c.wr128((r[13] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
fm::<1, 14>(c, 8, 2, c.vf[11]);
fm::<1, 14>(c, 9, 12, c.vf[2]);
{ let v = c.vf[2]; c.wr128((r[13] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let (a, b) = { let v = c.vf[6]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[1], h[1]), 16); r[1] = a; h[1] = b; }
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[2], h[2]), 16); r[2] = a; h[2] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::pand((r[6], h[6]), (r[12], h[12])); r[6] = a; h[6] = b; }
{ let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::pand((r[7], h[7]), (r[12], h[12])); r[7] = a; h[7] = b; }
c.vf[1] = [r[1] as u32, (r[1] >> 32) as u32, h[1] as u32, (h[1] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[8], h[8]), (r[12], h[12])); r[8] = a; h[8] = b; }
c.vf[2] = [r[2] as u32, (r[2] >> 32) as u32, h[2] as u32, (h[2] >> 32) as u32];
{ let (a, b) = uycpu::mmi::pand((r[9], h[9]), (r[12], h[12])); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[6], h[6]), 31); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[7], h[7]), 30); r[7] = a; h[7] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[8], h[8]), 31); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::psrlw((r[9], h[9]), 30); r[9] = a; h[9] = b; }
{ let (a, b) = uycpu::mmi::por((r[6], h[6]), (r[7], h[7])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::por((r[8], h[8]), (r[9], h[9])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppach((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[6], h[6])); r[6] = a; h[6] = b; }
{ let (a, b) = uycpu::mmi::ppacb((0u64, 0u64), (r[8], h[8])); r[8] = a; h[8] = b; }
c.wr32((r[13] as u32).wrapping_add(0x0000000c), r[6] as u32);
r[15] = (((r[15] as u32).wrapping_add(0xfffffff0)) as i32 as i64 as u64);
let t = (r[15] as i64) > 0;  c.wr32((r[13] as u32).wrapping_add(0x0000001c), r[8] as u32);
 if t { l = 0x003fb4d8; continue; }
r[1] = c.rd32((r[20] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[13] = 0x0000000070000000;
r[14] = c.rd32((r[20] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[15] = (((r[20] as u32).wrapping_add((r[11] as u32))) as i32 as i64 as u64);
r[15] = (((r[15] as u32).wrapping_add((r[1] as u32))) as i32 as i64 as u64);
r[11] = 0x0000000000020000;
r[5] = c.rd32((r[15] as u32)) as i32 as i64 as u64;
r[14] = (((r[14] as u32).wrapping_add((r[15] as u32))) as i32 as i64 as u64);
r[11] = r[11] | 0x202;
r[12] = 0u64 | 0x700;
{ let (a, b) = uycpu::mmi::pcpyh((r[12], h[12])); r[12] = a; h[12] = b; }
{ let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
r[13] = r[13] | 0x2000;
l = 0x003fb5c8; continue; }
0x003fb5c8 => {
let t = r[15] == r[14];  { let (a, b) = uycpu::mmi::pextlh((r[12], h[12]), (r[5], h[5])); r[1] = a; h[1] = b; }
 if t { l = 0x003fb7c8; continue; }
{ let (a, b) = uycpu::mmi::psllw((r[1], h[1]), 4); r[1] = a; h[1] = b; }
r[6] = c.rd32((r[1] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[2] = r[1] >> 32;
r[7] = c.rd32((r[2] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
{ let (a, b) = uycpu::mmi::pcpyud((r[1], h[1]), (0u64, 0u64)); r[3] = a; h[3] = b; }
r[8] = c.rd32((r[3] as u32).wrapping_add(0x0000000c)) as i32 as i64 as u64;
r[4] = r[5] >> 48;
r[5] = c.rd32((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[6] = r[6] & r[7];
r[6] = r[6] & r[8];
r[15] = (((r[15] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
let t = (r[6] as i64) > 0;  { let (a, b) = uycpu::mmi::pextlb((r[11], h[11]), (r[5], h[5])); r[5] = a; h[5] = b; }
 if t { l = 0x003fb5c8; continue; }
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[19] & 0x80;
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[6] = r[6] & r[4];
let t = (r[6] as i64) > 0;  { let (a, b) = c.rd128((r[3] as u32)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x003fb5c8; continue; }
fm::<1, 14>(c, 2, 2, c.vf[1]);
r[6] = r[19] & 0x20;
fm::<1, 14>(c, 3, 3, c.vf[1]);
r[6] = ((r[6] as i64) < 1) as u64;
fm::<1, 14>(c, 4, 22, c.vf[1]);
r[7] = (((r[19] as u32) >> 8) as i32 as i64 as u64);
uycpu::vu0::opmula(c, 3, 2);
r[7] = r[7] ^ r[4];
uycpu::vu0::opmsub(c, 6, 2, 3);
r[7] = r[7] & 0x1f;
fm::<6, 14>(c, 7, 4, c.vf[6]);
r[6] = r[6] | r[7];
let t = r[6] == 0u64;  fm::<6, 14>(c, 8, 6, c.vf[6]);
 if t { l = 0x003fb5c8; continue; }
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::qop::<0>(c, c.vf[7][0], c.vf[8][0]);
r[6] = r[19] & 0x10;
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
fm::<1, 14>(c, 5, 0, c.vf[6]);
fm::<0, 14>(c, 32, 4, c.vf[0]);
let t = (r[6] as i64) > 0;  r[7] = r[7] << 32;
 if t { l = 0x003fb690; continue; }
let t = (r[7] as i64) < 0;   if t { l = 0x003fb5c8; continue; }
l = 0x003fb690; continue; }
0x003fb690 => {
fm::<6, 14>(c, 7, 6, uycpu::vu0::bcast(c.vi[22]));
fm::<2, 14>(c, 4, 5, uycpu::vu0::bcast(c.vi[22]));
fm::<6, 14>(c, 7, 7, c.vf[7]);
fm::<1, 14>(c, 9, 3, c.vf[2]);
fm::<1, 14>(c, 10, 4, c.vf[2]);
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 5, 21, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::opmula(c, 4, 2);
uycpu::vu0::opmsub(c, 7, 2, 4);
uycpu::vu0::opmula(c, 3, 4);
uycpu::vu0::opmsub(c, 8, 4, 3);
fm::<1, 8>(c, 11, 24, c.vf[5]);
uycpu::vu0::opmula(c, 10, 9);
uycpu::vu0::opmsub(c, 9, 9, 10);
fm::<6, 14>(c, 7, 7, c.vf[6]);
fm::<6, 14>(c, 8, 8, c.vf[6]);
{ let (a, b) = { let v = c.vf[11]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
fm::<6, 14>(c, 9, 9, c.vf[6]);
fm::<0, 4>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][0]));
r[8] = r[8] << 32;
let t = (r[8] as i64) < 0;  fm::<2, 4>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
 if t { l = 0x003fb5c8; continue; }
fm::<0, 4>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][0]));
fm::<2, 4>(c, 8, 21, uycpu::vu0::bcast(c.vf[8][2]));
fm::<0, 4>(c, 32, 9, uycpu::vu0::bcast(c.vf[9][0]));
fm::<2, 4>(c, 9, 21, uycpu::vu0::bcast(c.vf[9][2]));
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[7] = a; h[7] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
let t = (r[7] as i64) < 0;  { let (a, b) = { let v = c.vf[9]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[9] = a; h[9] = b; }
 if t { l = 0x003fb748; continue; }
let t = (r[8] as i64) < 0;   if t { l = 0x003fb738; continue; }
let t = (r[9] as i64) >= 0;   if t { l = 0x003fb7b8; continue; }
fm::<0, 14>(c, 7, 2, c.vf[0]);
fm::<0, 14>(c, 8, 3, c.vf[0]);
 l = 0x003fb750; continue;
}
0x003fb738 => {
fm::<0, 14>(c, 7, 3, c.vf[0]);
fm::<0, 14>(c, 8, 0, c.vf[0]);
 l = 0x003fb750; continue;
}
0x003fb748 => {
fm::<0, 14>(c, 7, 0, c.vf[0]);
fm::<0, 14>(c, 8, 2, c.vf[0]);
l = 0x003fb750; continue; }
0x003fb750 => {
fm::<1, 14>(c, 9, 4, c.vf[7]);
fm::<1, 14>(c, 8, 8, c.vf[7]);
fm::<6, 14>(c, 10, 9, c.vf[8]);
fm::<6, 14>(c, 11, 8, c.vf[8]);
fm::<0, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[10][1]));
fm::<2, 8>(c, 10, 21, uycpu::vu0::bcast(c.vf[10][2]));
fm::<0, 8>(c, 32, 11, uycpu::vu0::bcast(c.vf[11][1]));
fm::<2, 8>(c, 11, 21, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::qop::<0>(c, c.vf[10][0], c.vf[11][0]);
fm::<0, 8>(c, 10, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 10, 10, c.vf[0]);
fm::<5, 8>(c, 10, 10, c.vf[21]);
fm::<0, 14>(c, 32, 7, c.vf[0]);
fm::<2, 14>(c, 4, 8, uycpu::vu0::bcast(c.vf[10][0]));
fm::<1, 14>(c, 7, 22, c.vf[1]);
fm::<1, 14>(c, 7, 4, c.vf[7]);
fm::<6, 14>(c, 8, 7, c.vf[7]);
fm::<0, 8>(c, 32, 8, uycpu::vu0::bcast(c.vf[8][1]));
fm::<2, 8>(c, 5, 21, uycpu::vu0::bcast(c.vf[8][2]));
l = 0x003fb7a4; continue; }
0x003fb7a4 => {
fm::<1, 8>(c, 8, 24, c.vf[5]);
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[8] = a; h[8] = b; }
r[8] = r[8] << 32;
let t = (r[8] as i64) < 0;   if t { l = 0x003fb5c8; continue; }
l = 0x003fb7b8; continue; }
0x003fb7b8 => {
c.wr32((r[22] as u32), r[24] as u32);
r[22] = (((r[22] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 l = 0x003fb338; continue;
}
0x003fb7c8 => {
r[1] = c.rd32((r[20] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[15] = (((r[20] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64);
let t = r[1] == 0u64;  r[13] = r[19] & 0x4;
 if t { l = 0x003fb338; continue; }
r[14] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
r[13] = ((r[13] as i64) < 1) as u64;
r[12] = c.rd32((r[24] as u32).wrapping_add(0x0000009c)) as i32 as i64 as u64;
r[13] = (((r[13] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
l = 0x003fb7e8; continue; }
0x003fb7e8 => {
let t = (r[14] as i64) < 0;  r[1] = r[12] & 0x1;
 if t { l = 0x003fb338; continue; }
r[12] = (((r[12] as u32) >> 1) as i32 as i64 as u64);
r[14] = c.rd16((r[15] as u32).wrapping_add(0x00000002)) as i16 as i64 as u64;
r[2] = c.rd8((r[15] as u32)) as i8 as i64 as u64;
r[15] = (((r[15] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64);
let t = (r[1] as i64) > 0;  r[1] = r[13] & r[14];
 if t { l = 0x003fb7e8; continue; }
let t = r[1] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003fb7e8; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003fb928; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003fb8f0; continue; }
let t = r[2] == 0u64;  r[2] = (((r[2] as u32).wrapping_add(0xffffffff)) as i32 as i64 as u64);
 if t { l = 0x003fb8c0; continue; }
r[1] = c.rd16((r[15] as u32).wrapping_add(0xffffffe4)) as i16 as i64 as u64;
r[2] = c.rd16((r[15] as u32).wrapping_add(0xffffffe6)) as i16 as i64 as u64;
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[2] = (((r[2] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[1] as u32).wrapping_add((r[16] as u32))) as i32 as i64 as u64);
r[2] = (((r[2] as u32).wrapping_add((r[16] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[2] as u32)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 14>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 1>(c, 3, 3, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[2][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[2][2]));
fm::<2, 14>(c, 2, 15, uycpu::vu0::bcast(c.vf[21][0]));
fm::<1, 14>(c, 2, 2, c.vf[1]);
fm::<1, 14>(c, 4, 31, c.vf[1]);
l = 0x003fb880; continue; }
0x003fb880 => {
fm::<6, 14>(c, 6, 4, c.vf[2]);
fm::<6, 14>(c, 7, 2, c.vf[2]);
fm::<0, 8>(c, 32, 6, uycpu::vu0::bcast(c.vf[6][1]));
fm::<2, 8>(c, 6, 21, uycpu::vu0::bcast(c.vf[6][2]));
fm::<0, 8>(c, 32, 7, uycpu::vu0::bcast(c.vf[7][1]));
fm::<2, 8>(c, 7, 21, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::qop::<0>(c, c.vf[6][0], c.vf[7][0]);
fm::<0, 8>(c, 6, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<4, 8>(c, 6, 6, c.vf[0]);
fm::<5, 8>(c, 6, 6, c.vf[21]);
fm::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[6][0]));
fm::<0, 14>(c, 1, 1, c.vf[2]);
uycpu::vu0::mov::<1>(c, 1, c.vf[3]);
 l = 0x003fb940; continue;
}
0x003fb8c0 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 4>(c, 2, 2, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[0][3]));
fm::<6, 14>(c, 2, 18, uycpu::vu0::bcast(c.vf[2][1]));
uycpu::vu0::mov::<1>(c, 3, c.vf[1]);
fm::<1, 14>(c, 4, 31, c.vf[1]);
 l = 0x003fb880; continue;
}
0x003fb8f0 => {
r[1] = c.rd32((r[15] as u32).wrapping_add(0xffffffe4)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xffffffe0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[1] = (((r[1] as u32) << 4) as i32 as i64 as u64);
r[1] = (((r[1] as u32).wrapping_add((r[16] as u32))) as i32 as i64 as u64);
{ let (a, b) = c.rd128((r[1] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::mov::<1>(c, 1, c.vf[2]);
fm::<0, 14>(c, 1, 1, c.vf[3]);
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
 l = 0x003fb940; continue;
}
0x003fb928 => {
{ let (a, b) = c.rd128((r[15] as u32).wrapping_add(0xfffffff0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
fm::<6, 15>(c, 1, 1, uycpu::vu0::bcast(c.vf[13][0]));
fm::<6, 14>(c, 32, 16, uycpu::vu0::bcast(c.vf[1][0]));
fm::<2, 14>(c, 32, 17, uycpu::vu0::bcast(c.vf[1][1]));
fm::<2, 14>(c, 32, 18, uycpu::vu0::bcast(c.vf[1][2]));
fm::<2, 14>(c, 1, 15, uycpu::vu0::bcast(c.vf[21][0]));
l = 0x003fb940; continue; }
0x003fb940 => {
fm::<0, 8>(c, 3, 30, uycpu::vu0::bcast(c.vf[1][3]));
fm::<1, 14>(c, 2, 1, c.vf[31]);
fm::<6, 8>(c, 3, 3, c.vf[3]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
fm::<0, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 8>(c, 5, 3, c.vf[2]);
uycpu::vu0::qop::<1>(c, c.vf[0][0], c.vf[2][0]);
{ let (a, b) = { let v = c.vf[5]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) < 0;   if t { l = 0x003fb7e8; continue; }
fm::<0, 8>(c, 2, 0, uycpu::vu0::bcast(c.vi[22]));
uycpu::vu0::qop::<0>(c, c.vf[0][3], c.vf[2][0]);
fm::<6, 8>(c, 2, 2, c.vf[21]);
fm::<5, 8>(c, 3, 2, uycpu::vu0::bcast(c.vf[1][3]));
fm::<0, 8>(c, 2, 0, uycpu::vu0::bcast(c.vi[22]));
fm::<1, 14>(c, 5, 31, c.vf[1]);
fm::<6, 14>(c, 5, 5, uycpu::vu0::bcast(c.vf[2][0]));
fm::<6, 14>(c, 5, 5, uycpu::vu0::bcast(c.vf[3][0]));
fm::<0, 14>(c, 1, 1, c.vf[5]);
fm::<1, 14>(c, 2, 1, c.vf[31]);
fm::<6, 14>(c, 2, 2, c.vf[2]);
fm::<0, 8>(c, 32, 2, uycpu::vu0::bcast(c.vf[2][1]));
fm::<2, 8>(c, 2, 21, uycpu::vu0::bcast(c.vf[2][2]));
fm::<1, 8>(c, 3, 24, c.vf[2]);
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[1] = a; h[1] = b; }
r[1] = r[1] << 32;
let t = (r[1] as i64) < 0;   if t { l = 0x003fb7e8; continue; }
c.wr32((r[22] as u32), r[24] as u32);
r[22] = (((r[22] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 l = 0x003fb338; continue;
}
0x003fb9d0 => {
c.wr32((r[22] as u32), 0u64 as u32);
r[23] = sx(k.hi(c, 0x3fb9d4));
r[23] = (((r[23] as u32).wrapping_add(k.lo(c, 0x3fb9d8))) as i32 as i64 as u64);
r[12] = sx(k.hi(c, 0x3fb9dc));
r[12] = (((r[12] as u32).wrapping_add(k.lo(c, 0x3fb9e0))) as i32 as i64 as u64);
r[1] = c.rd32((r[23] as u32)) as i32 as i64 as u64;
{ let v = c.vf[0]; c.wr128((r[12] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[2] = ((0u32.wrapping_add(0x0000003f)) as i32 as i64 as u64);
c.wr32((r[12] as u32).wrapping_add(0x0000001c), r[2] as u32);
c.wr32((r[12] as u32).wrapping_add(0x00000018), r[1] as u32);
let t = r[21] == 0u64;   if t { l = 0x003fba88; continue; }
let t = r[23] == r[22];  r[1] = (((r[23] as u32).wrapping_add(0x00000000)) as i32 as i64 as u64);
 if t { l = 0x003fba88; continue; }
r[13] = sx(k.hi(c, 0x3fba08));
r[13] = (((r[13] as u32).wrapping_add(k.lo(c, 0x3fba0c))) as i32 as i64 as u64);
r[14] = c.rd32((r[12] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[21] as u32)); r[5] = a; h[5] = b; }
{ let (a, b) = c.rd128((r[21] as u32).wrapping_add(0x00000010)); r[6] = a; h[6] = b; }
r[7] = c.rd64((r[21] as u32).wrapping_add(0x00000020));
l = 0x003fba20; continue; }
0x003fba20 => {
let t = r[1] == r[22];  r[2] = c.rd32((r[1] as u32)) as i32 as i64 as u64;
 if t { l = 0x003fba80; continue; }
r[3] = c.rd16((r[2] as u32).wrapping_add(0x00000034)) as u64;
r[4] = (((r[14] as u32) << 6) as i32 as i64 as u64);
r[10] = c.rd8((r[2] as u32).wrapping_add(0x000000a8)) as i8 as i64 as u64;
r[3] = r[3] & 0x4000;
let t = r[3] == 0u64;  r[1] = (((r[1] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64);
 if t { l = 0x003fba20; continue; }
r[8] = c.rd32((r[21] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
r[9] = 0u64 | 0x0;
let t = (r[10] as i64) >= 0;  r[4] = (((r[4] as u32).wrapping_add((r[13] as u32))) as i32 as i64 as u64);
 if t { l = 0x003fbac0; continue; }
l = 0x003fba50; continue; }
0x003fba50 => {
{ let v = c.vf[0]; c.wr128((r[4] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
c.wr128((r[4] as u32).wrapping_add(0x00000010), r[5], h[5]);
c.wr128((r[4] as u32).wrapping_add(0x00000020), r[6], h[6]);
c.wr64((r[4] as u32).wrapping_add(0x00000030), r[7]);
r[8] = r[8] | r[9];
c.wr32((r[4] as u32).wrapping_add(0x00000024), r[8] as u32);
c.wr32((r[4] as u32).wrapping_add(0x00000038), r[2] as u32);
c.wr8((r[2] as u32).wrapping_add(0x000000a8), r[14] as u8);
r[14] = (((r[14] as u32).wrapping_add(0x00000001)) as i32 as i64 as u64);
r[14] = r[14] & 0x3f;
 l = 0x003fba20; continue;
}
0x003fba80 => {
c.wr32((r[12] as u32).wrapping_add(0x00000014), r[14] as u32);
l = 0x003fba88; continue; }
0x003fba88 => {
r[2] = (((r[22] as u32).wrapping_sub((r[23] as u32))) as i32 as i64 as u64);
r[2] = (((r[2] as u32) >> 2) as i32 as i64 as u64);
l = 0x003fba90; continue; }
0x003fba90 => {
{ let (a, b) = uycpu::mmi::pcpyud((r[31], h[31]), (0u64, 0u64)); r[31] = a; h[31] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[23], h[23]), (0u64, 0u64)); r[23] = a; h[23] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[22], h[22]), (0u64, 0u64)); r[22] = a; h[22] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[21], h[21]), (0u64, 0u64)); r[21] = a; h[21] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[20], h[20]), (0u64, 0u64)); r[20] = a; h[20] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[19], h[19]), (0u64, 0u64)); r[19] = a; h[19] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[18], h[18]), (0u64, 0u64)); r[18] = a; h[18] = b; }
{ let (a, b) = uycpu::mmi::pcpyud((r[17], h[17]), (0u64, 0u64)); r[17] = a; h[17] = b; }
let t = (r[31] as u32); { let (a, b) = uycpu::mmi::pcpyud((r[16], h[16]), (0u64, 0u64)); r[16] = a; h[16] = b; }
 c.ret = t; return;
}
0x003fbab8 => {
r[2] = ((0u32.wrapping_add(0x00000000)) as i32 as i64 as u64);
 l = 0x003fba90; continue;
}
0x003fbac0 => {
r[10] = (((r[10] as u32) << 6) as i32 as i64 as u64);
r[10] = (((r[10] as u32).wrapping_add((r[13] as u32))) as i32 as i64 as u64);
r[11] = c.rd32((r[10] as u32).wrapping_add(0x00000038)) as i32 as i64 as u64;
let t = r[11] != r[2];  r[11] = c.rd32((r[10] as u32).wrapping_add(0x0000002c)) as i32 as i64 as u64;
 if t { l = 0x003fba50; continue; }
r[9] = c.rd32((r[21] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
r[11] = (((r[11] as u32).wrapping_sub((r[9] as u32))) as i32 as i64 as u64);
let t = (r[11] as i64) < 0;  r[9] = c.rd32((r[10] as u32).wrapping_add(0x00000024)) as i32 as i64 as u64;
 if t { l = 0x003fba50; continue; }
r[11] = c.rd32((r[21] as u32).wrapping_add(0x00000014)) as i32 as i64 as u64;
r[9] = r[9] | r[11];
c.wr32((r[10] as u32).wrapping_add(0x00000024), r[9] as u32);
 l = 0x003fba20; continue;
}
_ => unreachable!("0x3fb1d0: no label {l:08x} in it"), } } }
