//! The tie cull (level 1 0x4AFA28) and the terrain cull (level 1 0x4ADC98), done natively in front of their translations
//! (port/uyatr `JOBS`). The translated wrapper at 0x4AFA28 asks `start` (`Rt::native` 20), the one at 0x4ADC98 asks with 24;
//! both answer `after` (21, 25) once the translation ran when `start` let it run. `c.ret` brings the function's address with 1
//! in its low bit and 2 when the program's view is widened (a story level: `VIEW_LOADS` "the tie cull" +0x198 and "the terrain
//! cull" +0x10c; the front ends' is the game's own).
//!
//! The entries are the culls themselves, not their callers, because the callers are not glue:
//! - 0x4AF6D0 (the frame's tie pass) runs the whole tie pipeline around the cull: 0x4C1FE8, FlushCache, the cull, the class
//!   lists (0x4AF5E0: 0x4B27F0, 0x4C21C0), FlushCache, 0x4B2AA0, the tie VU0 program's upload (0x4C1AF0 of 0x1159B0), the tie
//!   chain (0x4B0990, whose 0x4B1280 runs that program, and 0x4B1D60), the shrub program's upload (0x4C1AF0 of 0x100AE0) and
//!   a copy of 0x1DA420 into the chain;
//! - 0x4ADBC0 (the frame's terrain pass) makes a matrix with the library's math (0x40C5D0, 0x40BE90, 0x40C9B0), sends it to
//!   VU1 twice (0x4C1EF0 at 5 and 0x14D), FlushCache, the cull, then the clipped chunks (0x4ADA10: 0x4AE8A0, 0x4AEA08,
//!   0x4C21C0) and the list at scratchpad 0x3000 to 0x23C180 (0x40BDE0).
//!
//! Both culls are hand-written leaves: the caller's registers kept at scratchpad 0x3FA8 to 0x3FFF and given back at the end
//! (s0 to s7, sp, fp, ra by SD/LD, so their high halves pass through), ra the scratchpad's base, no call but the tie cull's
//! wait for SPR_FROM (0x4B0964).
//! - 0x4AFA28: for each tie class of the list at 0x23C580 (gp+0x46A0 of them) its instances (0x20-byte records in the table
//!   gp+0x46A4 points to) are tested against the view's frustum (vf20, the tangents), the visibility grid's mask (scratchpad
//!   0x3C00, copied from 0x1EA8C0) and their distance (vf4.y from the instance's byte +0x1C, at most gp+0x46E4 >> 4), the ones
//!   that cross the view's planes by CLIP of the class's 8 bounding corners (0x4B039C); a drawn one gets a VIF1 packet built
//!   in the scratchpad at sp (sent to the chain at fp by SPR_FROM as it reaches 0x600) and entries in the lists at scratchpad
//!   0x800, 0x1000, 0x1800, 0x2000, 0x3000, 0x3400 and 0x3800; the end sends 0x800 to 0x242080, 0x2000 to 0x240880, 0x3800 to
//!   0x242880, 0x3400 to 0x242C80 and 0x1800 to 0x241880 (level 1's addresses), with their sizes at gp+0x46BC to gp+0x46C8,
//!   and stores the chain pointer (gp+0x4838) and the matrix records' table (gp+0x46B8).
//! - 0x4ADC98: the terrain's chunks (0x40-byte records from gp+0x4660, gp+0x4664 of them) come in by SPR_TO, 32 at a time into
//!   the scratchpad's 0x1000 and 0x1800 in turn; each is tested against the frustum (vf24) and the visibility grid's mask
//!   (scratchpad 0x3B00, copied from 0x1EA8C0), the ones that cross the view's planes by CLIP of their 8 box corners (0x4AE2A0)
//!   and listed at 0x3D00 for 0x4ADA10; a drawn chunk's packet (from the templates at gp-0x2590) is built at sp (halves of
//!   0x800 sent to the chain at fp by SPR_FROM), its entries at 0x3000, 0x3400 and 0x3780 (0x38 quadwords sent to 0x23BD80).
//! Neither runs a VU0 microprogram: the tie program's runs stay in 0x4B1280, the shrub program's tie entries in 0x4B3368.
//!
//! The functions are the same code in every program (uyatr finds them by size and shape: one of each in each story level and
//! front end). What relocation changes in them, the addresses of the level's globals and of boot data (every LUI of a RAM
//! address and the ADDIU after it), the gp offsets and the internal jumps, was compared over levels 1, 7, 28 and both front
//! ends, and is read from the code in RAM (`F`), named by level 1's address of the instruction holding it. Each function is
//! its translation's statements in order (uyatr's output for level 1, every instruction of every path) on a register file of
//! its own, so what the unit leaves is what the translation leaves: every GPR (both halves: the 128-bit loads, PMINW, PROT3W,
//! PEXTLH, PSRAW and the QMFC2s on the full registers, as the translation does them), VU0's registers, flags and Q, RAM, the
//! scratchpad, the DMA channels' registers, D_STAT and the end interrupts (the stores to SPR_FROM's and SPR_TO's registers go
//! through `hw::write`, as the translation's do). What goes away: the loops' safe points (the unit is one block of the
//! thread's time, as the other units are) and the polls' spins (the port's transfers end at their kick, so STR is never set
//! when a poll reads it; a set one stops the game: `busy`).
//!
//! The PMINW at 0x4AFCF0 (`pminw v0, v0, s1`, each instance) reads s1's high half: s1 is gp+0x46E4 >> 4 by LW, so its high
//! half is the caller's, passed through (the end's LD writes only the low half). Words 0 and 1 are the instance's byte +0x1C
//! against gp+0x46E4 >> 4, which DSLL32 and QMTC2 take into vf4.y; words 2 and 3, the minimum of v0's high half (the 0x1DA980
//! quadword's for the first instance, then what the instance before left: this PMINW's own, or the QMFC2 of vf7 or vf2) and
//! s1's, go to v0's high half, to vf4.z and vf4.w (QMTC2 at 0x4AFD08) and from there to a0's high half (QMFC2 at 0x4AFDB4).
//! In the unit they reach no memory, no branch and no low half: vf4 is written whole again (LQC2 at 0x4AFFF0 and 0x4B03A4, the
//! MINI.y, MAX.w and ADD.xyz on 0x4AFE50's paths) before its z or w is read or stored, and a0 and v0 are used by 64-bit
//! instructions only until the next QMFC2 writes them whole. So they can only stay in v0, a0 and vf4 at the return. The unit
//! does the PMINW on all four words of the registers as they are, so its v0, a0 and vf4 are the translation's from the same
//! state. The terrain cull's PMINW at 0x4ADE8C (`pminw at, at, t9`) reads t9's high half the same way, and is done the same way.
//!
//! With UYA_NATIVE_CHECK=<job>:N (job `tie`: the calls of 0x4AFA28; `terrain`: the calls of 0x4ADC98; any other job is another
//! unit's and leaves these alone) the first N calls run twice from the same state: the translation, then the native unit on
//! the state as it was before; every difference in the machine's state they leave (`differences`) is reported with what the
//! calls covered, a summary every 100 calls, and the game goes on from the translation's state. Both runs go with the
//! thread's interrupts held (`Check::ints`), as the other units' checks do.
//!
//! DRAW DISTANCE FAR (`drawdist`, port/uya `settings.rs`): the tie cull's far test is the sign of vf3.y = (L - r) - (z - r),
//! L = 16 x min(the instance's byte +0x1C, gp+0x46E4 >> 4) (vf4.y, ITOF0 of the PMINW at 0x4AFCF0 shifted), r its radius, z
//! its centre's view depth, read by the QMFC2 of vf3 and its branch in 0x4AFCE0's block: a tie whose nearest point is past
//! L is not listed. With FAR, vf19.y (L - r, the SUB.y after the ITOF0) is at least the reach, unless L is 0 (a byte of 0: a
//! tie the level hides), so a tie is listed while its nearest point is within the reach. The fade the cull gives a tie over
//! the last 8 units before its limit (vf19.y as FTOI4 of vf3.y, at most 0x80, at the record's +0x44 by 0x4B0010) comes with
//! it. What is not the far test stays the game's: the tie's LOD (its class's distances against its depth: the port's
//! `range` reads them as its finest LOD's), the near and side tests, the far clip (a tie whose corners are all past it is culled at 0x4B039C,
//! and FAR's reach is never past it), and the cull's caps (1023 ties a frame, `t4`; 256 a class, scratchpad 0x1000 to
//! 0x1800). The terrain cull is the game's at FAR too: the renderer draws every chunk that passes the zone test at any depth
//! (port/uya `native::terrain_pass`), so its far limit (vf23.y, gp+0x4690: 960 units x 1024 on Veldin) decides nothing seen.
//! A check of the tie cull stops the game while FAR is on (`drawdist::refuse_check`); the terrain cull's is the same at both.

use crate::{Cpu, Ext};
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

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
        assert!(a < uycpu::RAM_SIZE - 4, "the cull unit's code at {a:08x}: not in RAM");
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
    /// gp plus the offset of the instruction at `pc`.
    #[inline(always)]
    fn gp(self, c: &Cpu, gp: u32, pc: u32) -> u32 {
        gp.wrapping_add(self.lo(c, pc))
    }
}

// ---- registers, 32-bit arithmetic as the R5900 keeps it ---------------------------------------------------------------------

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
/// ADDU / ADDIU.
#[inline(always)]
fn add32(a: u64, b: u64) -> u64 {
    sx((a as u32).wrapping_add(b as u32))
}
/// SUBU.
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

// ---- the DMA polls ---------------------------------------------------------------------------------------------------------

/// A poll (level 1's `pc`, the loop's head) found its channel's STR set, where the translation would spin: the port's
/// transfers end at their kick (`hw::write`'s), so no store the game makes leaves one running.
#[cold]
#[inline(never)]
fn busy(k: F, pc: u32) -> ! {
    panic!("the cull at {:08x}: the DMA channel its poll at level 1's {pc:08x} reads is busy: the port's transfers end at their kick", k.at)
}

/// 0x4B0964, the tie cull's wait after each SPR_FROM kick (`lw at, -0x3000(a0)` of D8_CHCR with a0 = 0x10010000, `andi at,
/// at, 0x100`, a spin while set; `jr ra` with `lui ra, 0x7000` in its delay slot): at as the poll leaves it, ra the
/// scratchpad's base again (the JAL's return address is never read but by its `jr`).
#[inline(always)]
fn f4b0964(c: &mut Cpu, k: F, r: &mut [u64; 32]) {
    r[AT] = sx(c.rd32((r[A0] as u32).wrapping_add(0xffff_d000))) & 0x100;
    if r[AT] != 0 {
        busy(k, 0x004b_0964);
    }
    r[RA] = 0x7000_0000;
}

// ---- what the calls covered (the check's report) ---------------------------------------------------------------------------

/// Counts of the paths the native calls took.
const N: usize = 8;
static DONE: [AtomicU64; N] = [const { AtomicU64::new(0) }; N];
const NAMES: [&str; N] = [
    "tie classes",
    "tie instances tested",
    "tie instances in view",
    "tie instances clipped",
    "terrain batches",
    "terrain chunks tested",
    "terrain chunks drawn",
    "terrain chunks clipped",
];

// ---- the check -------------------------------------------------------------------------------------------------------------

const SPR: u32 = 0x7000_0000;

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

/// What differs between the translation's state `want` and the native unit's `got`: the whole output contract, nothing left
/// out (the unit is its translation's statements on the same state, the module's notes): every register, both halves, the
/// FPU, VU0's registers, flags and Q (vi22), the view's original tangent (`view_h`, which the widened load sets), every byte of
/// RAM (the chain, the lists the culls send out, the instances' and classes' bytes they write), the scratchpad (the saved
/// registers, the work words at 0x3F00 to 0x3FA8, the packets and lists), VU0's memories, the DMA channels, D_STAT, the
/// interrupts and the renderer's skin recordings (an SPR_FROM from 0x3400 makes one while a skin record is captured).
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

/// The two entries: 0x4AFA28 (`tie`) and 0x4ADC98 (`terrain`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Job {
    Tie,
    Terrain,
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
    /// comparison takes what waited (`mobydraw`'s `Check::ints`: a kick's DMAC interrupt, or a loop's back edge in the
    /// translation past the field's time, landing in one run only; held, each lands once, after both runs).
    ints: bool,
}

static CHECK: std::sync::Mutex<Option<Check>> = std::sync::Mutex::new(None);

/// UYA_NATIVE_CHECK=<job>:N for this unit, read once; another unit's job (`moby`, `sweep`, `shrub`, any other) leaves this one
/// unchecked.
fn checked() -> Option<(Job, u64)> {
    static C: std::sync::OnceLock<Option<(Job, u64)>> = std::sync::OnceLock::new();
    *C.get_or_init(|| {
        let v = std::env::var("UYA_NATIVE_CHECK").ok()?;
        let (job, n) = v.split_once(':').unwrap_or_else(|| panic!("UYA_NATIVE_CHECK={v}: <job>:N"));
        let job = match job {
            "tie" => Job::Tie,
            "terrain" => Job::Terrain,
            _ => return None,
        };
        Some((job, n.parse().unwrap_or_else(|_| panic!("UYA_NATIVE_CHECK={v}: N is a number of calls"))))
    })
}

thread_local! {
    /// The function's address and view bit of the call being checked, for its native run after the translation's.
    static CALL: std::cell::Cell<(u32, bool)> = const { std::cell::Cell::new((0, false)) };
}

/// `Rt::native` 20 and 24: the unit natively (`c.ret` the return address), or (checking) the state saved and `c.ret` 0 so
/// the translation runs.
pub(crate) fn start(c: &mut Cpu, k: u32) {
    let job = if k == 20 { Job::Tie } else { Job::Terrain };
    let (at, wide) = (c.ret & !3, c.ret & 2 != 0);
    if let Some((cj, n)) = checked() {
        let mut g = CHECK.lock().expect("the cull check");
        let ck = g.get_or_insert(Check { job: cj, left: n, done: 0, differ: 0, kinds: Default::default(), before: None, inner: 0, ints: false });
        if ck.before.is_some() {
            ck.inner += 1;
            c.ret = 0;
            return;
        }
        if ck.job == job && ck.left > 0 {
            if job == Job::Tie {
                crate::drawdist::refuse_check("tie");
            }
            CALL.with(|w| w.set((at, wide)));
            // Interrupts on as the kernel sees them (`kernel::K::ints_on`): EIE and IE, not in an exception.
            ck.ints = c.status & 0x1_0001 == 0x1_0001 && c.status & 6 == 0;
            if ck.ints {
                c.di();
            }
            ck.before = Some(State::take(c));
            // The translation reads the port's ranges from memory, as the port wrote them before each field (`range`).
            if job == Job::Tie {
                crate::range::for_translation(c, crate::range::Unit::Ties);
            }
            c.ret = 0;
            return;
        }
    }
    run(c, job, at, wide);
    c.ret = c.r[RA] as u32;
}

/// `Rt::native` 21 and 25: the translation ran; the unit runs natively from the state before it and the two are compared.
pub(crate) fn after(c: &mut Cpu, k: u32) {
    let job = if k == 21 { Job::Tie } else { Job::Terrain };
    let (Some(before), ints) = ({
        let mut g = CHECK.lock().expect("the cull check");
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
    if job == Job::Tie {
        crate::range::translation_done(c, crate::range::Unit::Ties);
    }
    let want = State::take(c);
    before.put(c);
    let (at, wide) = CALL.with(|w| w.get());
    if job == Job::Tie {
        crate::drawdist::refuse_check("tie");
    }
    let sp = c.r[SP] as u32;
    run(c, job, at, wide);
    c.ret = c.r[RA] as u32;
    let got = State::take(c);
    let d = differences(&want, &got);
    want.put(c);
    c.ret = ret;
    let mut g = CHECK.lock().expect("the cull check");
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
        println!("UYA_NATIVE_CHECK: {job:?}: {} calls checked, {} differ ({}){}", ck.done, ck.differ, n.join(", "), crate::range::written(crate::range::Unit::Ties));
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
        Job::Tie => f4afa28(c, F { at, l1: 0x4a_fa28 }, wide, &mut n),
        Job::Terrain => f4adc98(c, F { at, l1: 0x4a_dc98 }, wide, &mut n),
    }
    for (a, v) in DONE.iter().zip(n) {
        if v != 0 {
            a.fetch_add(v, Relaxed);
        }
    }
}

// ---- 0x4AFA28: the tie cull ---------------------------------------------------------------------------------------------------
//
// Its translation's statements in order on its own register file (`r`, `h` the high halves), the blocks by level 1's labels;
// the relocated words from the code (`k`), the safe points gone, 0x4B0964's polls `f4b0964`, the view's tangents widened when
// `wide` (+0x198: vf20 = (h view_k fov, v fov, +0xB8, +0xBC)).
#[allow(unused_assignments, unused_parens, clippy::all)]
fn f4afa28(c: &mut Cpu, k: F, wide: bool, n: &mut [u64; N]) {
let (mut r, mut h) = (c.r, c.rh);
let gp = r[GP] as u32;
let mut far: Option<u32> = None;
// The port's tie ranges (`range`): the level's classes culled at their finest LOD, its instances out to its limit.
let ranges = crate::range::get();
let ties = ranges.ties.as_ref().and_then(|t| crate::range::TieView::read(c, t));
let mut l: u32 = 0x004afa28;
loop { match l {
0x004afa28 => {
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
r[AT] = c.rd32(k.gp(c, gp, 0x004afa58)) as i32 as i64 as u64;
r[V0] = sx(k.hi(c, 0x004afa5c));
r[V0] = add32(r[V0], k.lo(c, 0x004afa60) as u64);
r[RA] = 0x0000000070000000;
r[V1] = c.rd32(k.gp(c, gp, 0x004afa68)) as i32 as i64 as u64;
r[A0] = c.rd32(k.gp(c, gp, 0x004afa6c)) as i32 as i64 as u64;
c.wr32((r[RA] as u32).wrapping_add(0x00003f80), r[AT] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003f84), r[V0] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003f88), r[V1] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003f8c), r[A0] as u32);
r[AT] = add32(r[RA], 0x00002000);
r[V0] = add32(r[RA], 0x00001800);
r[V1] = add32(r[RA], 0x00003400);
r[A0] = add32(r[RA], 0x00003800);
r[A1] = add32(r[RA], 0x00003000);
r[A2] = add32(r[RA], 0x00000800);
c.wr32((r[RA] as u32).wrapping_add(0x00003f90), r[AT] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003f94), r[V0] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003f98), r[V1] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003f9c), r[A0] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003fa4), r[A1] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003fa0), r[A2] as u32);
r[S1] = c.rd32(k.gp(c, gp, 0x004afab0)) as i32 as i64 as u64;
r[T8] = sx(0x00000000);
r[FP] = c.rd32(k.gp(c, gp, 0x004afab8)) as i32 as i64 as u64;
r[SP] = add32(r[RA], 0x00000000);
r[S1] = srl(r[S1], 4);
r[V0] = add32(r[RA], 0x00003c00);
r[AT] = sx(k.hi(c, 0x004afacc));
r[AT] = add32(r[AT], k.lo(c, 0x004afad0) as u64);
r[V1] = add32(r[V0], 0x00000080);
l = 0x004afad8; continue; }
0x004afad8 => {
{ let (a, b) = c.rd128((r[AT] as u32)); r[A0] = a; h[A0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); r[A1] = a; h[A1] = b; }
c.wr128((r[V0] as u32), r[A0], h[A0]);
r[AT] = add32(r[AT], 0x00000020);
c.wr128((r[V0] as u32).wrapping_add(0x00000010), r[A1], h[A1]);
r[V0] = add32(r[V0], 0x00000020);
let t = r[V0] != r[V1];   if t { l = 0x004afad8; continue; }
r[V0] = add32(r[RA], 0x00003d00);
r[AT] = sx(k.hi(c, 0x004afafc));
r[AT] = add32(r[AT], k.lo(c, 0x004afb00) as u64);
r[V1] = add32(r[V0], 0x00000100);
l = 0x004afb08; continue; }
0x004afb08 => {
{ let (a, b) = c.rd128((r[AT] as u32)); r[A0] = a; h[A0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); r[A1] = a; h[A1] = b; }
c.wr128((r[V0] as u32), r[A0], h[A0]);
r[AT] = add32(r[AT], 0x00000020);
c.wr128((r[V0] as u32).wrapping_add(0x00000010), r[A1], h[A1]);
r[V0] = add32(r[V0], 0x00000020);
let t = r[V0] != r[V1];   if t { l = 0x004afb08; continue; }
r[V0] = c.rd32(k.gp(c, gp, 0x004afb28)) as i32 as i64 as u64;
r[AT] = sx(k.hi(c, 0x004afb2c));
r[AT] = add32(r[AT], k.lo(c, 0x004afb30) as u64);
c.wr32((r[RA] as u32).wrapping_add(0x00003e20), r[V0] as u32);
{ let (a, b) = c.rd128((r[AT] as u32)); r[V0] = a; h[V0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); r[V1] = a; h[V1] = b; }
c.wr128((r[RA] as u32).wrapping_add(0x00003e00), r[V0], h[V0]);
c.wr128((r[RA] as u32).wrapping_add(0x00003e10), r[V1], h[V1]);
r[V0] = 0x0000000044800000;
r[AT] = sx(k.hi(c, 0x004afb4c));
r[AT] = add32(r[AT], k.lo(c, 0x004afb50) as u64);
c.vf[1] = [r[V0] as u32, (r[V0] >> 32) as u32, h[V0] as u32, (h[V0] >> 32) as u32];
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000140)); c.vf[24] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32)); c.vf[25] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); c.vf[26] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000020)); c.vf[27] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000080)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000090)); c.vf[29] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000a0)); c.vf[30] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000b0)); c.vf[31] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000100)); c.vf[12] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000110)); c.vf[13] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000120)); c.vf[14] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000130)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 1>(c, 24, 0, uycpu::vu0::bcast(c.vf[1][0]));
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000c0)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000d0)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000e0)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000f0)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[1]; c.wr128((r[RA] as u32).wrapping_add(0x00003f40), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[2]; c.wr128((r[RA] as u32).wrapping_add(0x00003f50), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[3]; c.wr128((r[RA] as u32).wrapping_add(0x00003f60), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
{ let v = c.vf[4]; c.wr128((r[RA] as u32).wrapping_add(0x00003f70), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[AT] = sx(k.hi(c, 0x004afbb0));
r[AT] = add32(r[AT], k.lo(c, 0x004afbb4) as u64);
// The view struct (at): DRAW DISTANCE FAR's reach for it (`drawdist`), in the cull's world units, as f32 bits.
far = crate::drawdist::reach_of(c, r[AT] as u32).map(|d| (d / 1024.0).to_bits());
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000230)); c.vf[18] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000a0)); c.vf[19] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000b0)); let (h, v) = if wide { (c.widen(a as u32), c.fov((a >> 32) as u32)) } else { (a as u32, (a >> 32) as u32) }; c.vf[20] = [h, v, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000001f0)); c.vf[22] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000200)); c.vf[23] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[AT] = 0x000000003a800000;
c.vf[1] = [r[AT] as u32, (r[AT] >> 32) as u32, h[AT] as u32, (h[AT] >> 32) as u32];
uycpu::vu0::fmac::<6, 12>(c, 19, 19, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<6, 12>(c, 23, 23, uycpu::vu0::bcast(c.vf[1][0]));
{ let (a, b) = c.rd128(k.gp(c, gp, 0x004afbdc)); c.vf[21] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
c.wr128((r[SP] as u32), 0u64, 0u64);
r[V0] = sx(k.hi(c, 0x004afbe4));
r[V0] = add32(r[V0], k.lo(c, 0x004afbe8) as u64);
r[AT] = sx(k.hi(c, 0x004afbec));
r[AT] = add32(r[AT], k.lo(c, 0x004afbf0) as u64);
r[V0] = c.rd32((r[V0] as u32)) as i32 as i64 as u64;
c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
r[AT] = 0x0000000030000000;
c.wr32((r[SP] as u32), r[AT] as u32);
r[FP] = add32(r[FP], 0x00000010);
c.wr16((r[SP] as u32), r[V0] as u16);
r[SP] = add32(r[SP], 0x00000010);
r[AT] = sx(k.hi(c, 0x004afc10));
r[AT] = add32(r[AT], k.lo(c, 0x004afc14) as u64);
{ let (a, b) = c.rd128((r[AT] as u32)); r[V0] = a; h[V0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); r[V1] = a; h[V1] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000020)); r[A0] = a; h[A0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000030)); r[A1] = a; h[A1] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000040)); r[A2] = a; h[A2] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000050)); r[A3] = a; h[A3] = b; }
c.wr128((r[SP] as u32), r[V0], h[V0]);
c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
c.wr128((r[SP] as u32).wrapping_add(0x00000040), r[A2], h[A2]);
c.wr128((r[SP] as u32).wrapping_add(0x00000050), r[A3], h[A3]);
r[SP] = add32(r[SP], 0x00000060);
r[FP] = add32(r[FP], 0x00000060);
c.wr32((r[RA] as u32).wrapping_add(0x00003fa8), r[FP] as u32);
r[FP] = add32(r[FP], 0x00000010);
c.wr32(k.gp(c, gp, 0x004afc58), r[FP] as u32);
r[SP] = add32(r[SP], 0x00000010);
r[T4] = sx(0x000003ff);
l = 0x004afc68; continue; }
0x004afc68 => {
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003f80)) as i32 as i64 as u64;
r[V0] = c.rd32((r[RA] as u32).wrapping_add(0x00003f84)) as i32 as i64 as u64;
let t = r[AT] == 0u64;  r[AT] = add32(r[AT], 0xffffffff);
 if t { l = 0x004b076c; continue; }
n[0] += 1;
r[S0] = c.rd32((r[V0] as u32)) as i32 as i64 as u64;
r[V0] = add32(r[V0], 0x00000004);
c.wr32((r[RA] as u32).wrapping_add(0x00003f80), r[AT] as u32);
c.wr32((r[RA] as u32).wrapping_add(0x00003f84), r[V0] as u32);
r[AT] = c.rd16((r[S0] as u32).wrapping_add(0x0000003c)) as u64;
r[V0] = 0x0000000050000000;
c.vf[17] = [r[V0] as u32, (r[V0] >> 32) as u32, h[V0] as u32, (h[V0] >> 32) as u32];
r[T3] = sx(0x00000000);
r[S6] = c.rd32((r[S0] as u32).wrapping_add(0x00000020)) as i32 as i64 as u64;
r[V0] = r[AT] & 0x9;
r[S7] = c.rd16((r[S0] as u32).wrapping_add(0x0000003e)) as u64;
r[T5] = r[AT] & 0x6;
r[S6] = sll(r[S6], 5);
r[V1] = c.rd32((r[RA] as u32).wrapping_add(0x00003f88)) as i32 as i64 as u64;
let t = r[V0] != 0u64;  { let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x00000010)); c.vf[16] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
if let Some(v) = &ties && v.lods(r[S0] as u32) { let [x, y, z] = crate::range::LOD_DISTANCES.map(f32::to_bits); c.vf[16][0] = x; c.vf[16][1] = y; c.vf[16][2] = z; }
 if t { l = 0x004b067c; continue; }
r[S6] = add32(r[S6], r[V1]);
r[T5] = srl(r[T5], 1);
r[S2] = 0;
r[S5] = add32(r[RA], 0x00001000);
r[S6] = add32(r[S6], 0xffffffe0);
r[S7] = sll(r[S7], 5);
r[S7] = add32(r[S7], r[S6]);
r[S3] = 0;
let t = r[S6] == r[S7];  r[S4] = 0;
 if t { l = 0x004b067c; continue; }
l = 0x004afce0; continue; }
0x004afce0 => {
let t = r[S6] == r[S7];  r[V0] = c.rd8((r[S6] as u32).wrapping_add(0x0000003c)) as u64;
if let Some(v) = &ties { r[V0] = v.byte((r[S6] as u32).wrapping_add(0x3c), r[V0]); }
 if t { l = 0x004b067c; continue; }
n[1] += 1;
{ let (a, b) = c.rd128((r[S6] as u32).wrapping_add(0x00000020)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[S6] = add32(r[S6], 0x00000020);
{ let (a, b) = uycpu::mmi::pminw((r[V0], h[V0]), (r[S1], h[S1])); r[V0] = a; h[V0] = b; }
r[AT] = c.rd8((r[S6] as u32).wrapping_add(0x00000011)) as u64;
uycpu::vu0::fmac::<1, 14>(c, 2, 1, c.vf[24]);
r[V0] = r[V0] << 36;
uycpu::vu0::fmac::<0, 8>(c, 3, 0, uycpu::vu0::bcast(c.vf[1][3]));
r[V1] = c.rd8((r[S6] as u32).wrapping_add(0x00000010)) as u64;
c.vf[4] = [r[V0] as u32, (r[V0] >> 32) as u32, h[V0] as u32, (h[V0] >> 32) as u32];
r[AT] = add32(r[AT], r[RA]);
uycpu::vu0::fmac::<1, 4>(c, 3, 0, uycpu::vu0::bcast(c.vf[1][3]));
r[AT] = c.rd8((r[AT] as u32).wrapping_add(0x00003c00)) as u64;
uycpu::vu0::itof::<0, 4>(c, 4, 4);
r[T9] = c.rd16((r[S6] as u32).wrapping_add(0x00000012)) as u64;
uycpu::vu0::fmac::<6, 14>(c, 32, 25, uycpu::vu0::bcast(c.vf[2][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 26, uycpu::vu0::bcast(c.vf[2][1]));
r[AT] = r[AT] & r[V1];
let t = r[AT] == 0u64;  uycpu::vu0::fmac::<2, 14>(c, 2, 27, uycpu::vu0::bcast(c.vf[2][2]));
 if t { l = 0x004afce0; continue; }
uycpu::vu0::fmac::<6, 12>(c, 8, 22, uycpu::vu0::bcast(c.vf[1][3]));
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003f8c)) as i32 as i64 as u64;
uycpu::vu0::fmac::<1, 4>(c, 19, 4, uycpu::vu0::bcast(c.vf[1][3]));
// DRAW DISTANCE FAR (the module doc): the tie's limit less its radius at least the reach, unless the limit is 0 (a tie the
// level hides).
if let Some(d) = far { if c.vf[4][1] != 0 && uycpu::fpu::lt(c.vf[19][1], d) { c.vf[19][1] = d; } }
r[T7] = sll(r[T9], 6);
r[T7] = add32(r[T7], r[AT]);
r[T6] = sx(0x00000001);
uycpu::vu0::fmac::<0, 12>(c, 1, 3, uycpu::vu0::bcast(c.vf[2][2]));
r[AT] = add32(r[RA], 0x00001800);
let t = r[AT] == r[S5];  uycpu::vu0::fmac::<6, 12>(c, 5, 20, uycpu::vu0::bcast(c.vf[2][2]));
 if t { l = 0x004afce0; continue; }
uycpu::vu0::abs::<12>(c, 2, 2);
r[AT] = c.rd8((r[S6] as u32).wrapping_add(0x0000001e)) as u64;
let t = (r[T4] as i64) <= 0;   if t { l = 0x004afce0; continue; }
uycpu::vu0::fmac::<1, 12>(c, 3, 19, c.vf[1]);
r[AT] = r[AT] & 0x4;
let t = r[AT] != 0u64;  uycpu::vu0::fmac::<1, 12>(c, 4, 1, c.vf[23]);
 if t { l = 0x004afce0; continue; }
uycpu::vu0::fmac::<1, 12>(c, 6, 2, c.vf[8]);
uycpu::vu0::fmac::<0, 12>(c, 8, 2, c.vf[8]);
uycpu::vu0::fmac::<4, 2>(c, 1, 0, c.vf[2]);
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[AT] = a; h[AT] = b; }
uycpu::vu0::fmac::<1, 12>(c, 7, 5, c.vf[6]);
r[A1] = 0x0000000040000000;
uycpu::vu0::fmac::<1, 12>(c, 8, 5, c.vf[8]);
r[V0] = r[AT] << 32;
let t = (r[AT] as i64) < 0;  { let (a, b) = { let v = c.vf[4]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[A0] = a; h[A0] = b; }
 if t { l = 0x004afce0; continue; }
let t = (r[V0] as i64) >= 0;  { let v = c.vf[1]; uycpu::vu0::mov::<12>(c, 1, [v[1], v[2], v[3], v[0]]); }
 if t { l = 0x004afce0; continue; }
uycpu::vu0::ftoi::<1, 4>(c, 19, 3);
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[V0] = a; h[V0] = b; }
r[A1] = r[A1].wrapping_add(r[A1]);
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[V1] = a; h[V1] = b; }
r[A0] = r[A0] ^ r[A1];
let t = (r[V0] as i64) < 0;  r[V0] = r[V0] << 32;
 if t { l = 0x004afce0; continue; }
let t = (r[V0] as i64) < 0;  r[V1] = r[V1] | r[A0];
 if t { l = 0x004afce0; continue; }
let t = (r[V1] as i64) < 0;  r[V1] = r[V1] << 32;
 if t { l = 0x004b039c; continue; }
let t = (r[V1] as i64) < 0;   if t { l = 0x004b039c; continue; }
l = 0x004afdf8; continue; }
0x004afdf8 => {
uycpu::vu0::ftoi::<0, 4>(c, 11, 1);
r[V1] = 0u64 | 0xffff;
uycpu::vu0::fmac::<1, 14>(c, 2, 16, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::fmac::<6, 8>(c, 3, 1, uycpu::vu0::bcast(c.vf[24][3]));
r[AT] = c.rd8((r[S6] as u32).wrapping_add(0x0000001e)) as u64;
uycpu::vu0::fmac::<0, 4>(c, 32, 0, c.vf[18]);
r[V0] = c.rd16((r[S6] as u32).wrapping_add(0x0000001a)) as u64;
uycpu::vu0::fmac::<2, 4>(c, 4, 1, uycpu::vu0::bcast(c.vf[18][0]));
r[AT] = r[AT] & 0x1;
let t = r[AT] != 0u64;  { let (a, b) = { let v = c.vf[11]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[T1] = a; h[T1] = b; }
 if t { l = 0x004afe30; continue; }
let t = r[V0] == r[V1];   if t { l = 0x004afe50; continue; }
l = 0x004afe30; continue; }
0x004afe30 => {
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003f98)) as i32 as i64 as u64;
r[V0] = add32(r[RA], 0x000037fe);
let t = r[AT] == r[V0];  r[V0] = add32(r[AT], 0x00000002);
 if t { l = 0x004afe50; continue; }
c.wr16((r[AT] as u32), r[T9] as u16);
c.wr32((r[RA] as u32).wrapping_add(0x00003f98), r[V0] as u32);
l = 0x004afe50; continue; }
0x004afe50 => {
n[2] += 1;
{ let (a, b) = { let v = c.vf[2]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[V0] = a; h[V0] = b; }
r[T4] = add32(r[T4], 0xffffffff);
r[A1] = c.rd8((r[S6] as u32).wrapping_add(0x0000001f)) as u64;
uycpu::vu0::fmac::<5, 8>(c, 17, 3, c.vf[17]);
{ let (a, b) = { let v = c.vf[19]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[T2] = a; h[T2] = b; }
r[AT] = r[V0] << 32;
let t = (r[AT] as i64) >= 0;  { let (a, b) = uycpu::mmi::prot3w((r[V0], h[V0])); r[V1] = a; h[V1] = b; }
 if t { l = 0x004aff40; continue; }
let t = (r[V1] as i64) < 0;   if t { l = 0x004aff80; continue; }
let t = (r[V0] as i64) >= 0;   if t { l = 0x004afee0; continue; }
uycpu::vu0::fmac::<1, 2>(c, 3, 16, uycpu::vu0::bcast(c.vf[16][1]));
r[T0] = r[S5];
uycpu::vu0::fmac::<1, 4>(c, 3, 0, c.vf[2]);
uycpu::vu0::fmac::<5, 4>(c, 4, 4, uycpu::vu0::bcast(c.vf[18][2]));
c.wr32((r[S5] as u32).wrapping_add(0x00000004), r[S3] as u32);
r[S3] = r[S5];
r[S5] = add32(r[S5], 0x00000008);
uycpu::vu0::qop::<0>(c, c.vf[3][1], c.vf[3][2]);
r[A1] = r[A1] ^ 0x1;
uycpu::vu0::fmac::<4, 1>(c, 4, 18, uycpu::vu0::bcast(c.vf[4][1]));
r[A2] = sx(0x00000001);
uycpu::vu0::fmac::<6, 7>(c, 3, 0, uycpu::vu0::bcast(c.vf[0][0]));
uycpu::vu0::fmac::<0, 14>(c, 4, 0, c.vf[0]);
uycpu::vu0::fmac::<0, 8>(c, 3, 0, uycpu::vu0::bcast(c.vi[22]));
let t = r[T6] == 0u64;  { let v = c.vf[4]; c.wr128((r[SP] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x004b05b4; continue; }
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x004affc0; continue;
}
0x004afee0 => {
uycpu::vu0::fmac::<1, 4>(c, 3, 16, uycpu::vu0::bcast(c.vf[16][0]));
r[T0] = r[S5];
uycpu::vu0::fmac::<1, 8>(c, 3, 0, c.vf[2]);
uycpu::vu0::fmac::<5, 4>(c, 4, 4, uycpu::vu0::bcast(c.vf[18][2]));
c.wr32((r[S5] as u32).wrapping_add(0x00000004), r[S2] as u32);
r[S2] = r[S5];
r[S5] = add32(r[S5], 0x00000008);
uycpu::vu0::qop::<0>(c, c.vf[3][0], c.vf[3][1]);
r[A1] = r[A1] ^ 0x0;
uycpu::vu0::fmac::<4, 1>(c, 4, 18, uycpu::vu0::bcast(c.vf[4][1]));
r[A2] = sx(0x00000000);
uycpu::vu0::fmac::<6, 7>(c, 3, 0, uycpu::vu0::bcast(c.vf[0][0]));
uycpu::vu0::fmac::<0, 14>(c, 4, 0, c.vf[0]);
uycpu::vu0::fmac::<0, 8>(c, 3, 0, uycpu::vu0::bcast(c.vi[22]));
let t = r[T6] == 0u64;  { let v = c.vf[4]; c.wr128((r[SP] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x004b0594; continue; }
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x004affc0; continue;
}
0x004aff40 => {
{ let (a, b) = uycpu::mmi::por((0u64, 0u64), (0u64, 0u64)); r[AT] = a; h[AT] = b; }
uycpu::vu0::fmac::<5, 4>(c, 4, 4, uycpu::vu0::bcast(c.vf[18][2]));
r[T0] = r[S5];
uycpu::vu0::fmac::<0, 14>(c, 3, 0, c.vf[0]);
c.wr32((r[S5] as u32).wrapping_add(0x00000004), r[S2] as u32);
uycpu::vu0::fmac::<4, 1>(c, 3, 18, uycpu::vu0::bcast(c.vf[4][1]));
r[A1] = r[A1] ^ 0x0;
r[A2] = sx(0x00000000);
r[S2] = r[S5];
r[S5] = add32(r[S5], 0x00000008);
let t = r[T6] == 0u64;  c.wr128((r[SP] as u32).wrapping_add(0x00000040), 0u64, 0u64);
 if t { l = 0x004b0574; continue; }
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x004affc0; continue;
}
0x004aff80 => {
{ let (a, b) = uycpu::mmi::por((0u64, 0u64), (0u64, 0u64)); r[AT] = a; h[AT] = b; }
uycpu::vu0::fmac::<5, 4>(c, 4, 4, uycpu::vu0::bcast(c.vf[18][2]));
r[T0] = r[S5];
uycpu::vu0::fmac::<0, 14>(c, 3, 0, c.vf[0]);
c.wr32((r[S5] as u32).wrapping_add(0x00000004), r[S4] as u32);
uycpu::vu0::fmac::<4, 1>(c, 3, 18, uycpu::vu0::bcast(c.vf[4][1]));
r[A1] = r[A1] ^ 0x2;
r[A2] = sx(0x00000002);
r[S4] = r[S5];
r[S5] = add32(r[S5], 0x00000008);
c.wr128((r[SP] as u32).wrapping_add(0x00000040), 0u64, 0u64);
let t = r[T6] == 0u64;  { let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x004b05d4; continue; }
l = 0x004affc0; continue; }
0x004affc0 => {
r[AT] = sll(r[T8], 12);
r[V0] = sx(0x0000007f);
r[T1] = r[T1] >> 32;
{ let (a, b) = uycpu::mmi::pminw((r[T1], h[T1]), (r[V0], h[V0])); r[T1] = a; h[T1] = b; }
r[AT] = r[AT] | r[T9];
r[T1] = sll(r[T1], 25);
r[AT] = r[AT] | r[T1];
r[V0] = c.rd8((r[S6] as u32).wrapping_add(0x0000001e)) as u64;
r[V0] = r[V0] & 0x8;
c.wr32((r[T0] as u32), r[AT] as u32);
{ let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000030)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[V0] = sll(r[V0], 2);
let t = r[A1] != 0u64;  c.wr32((r[SP] as u32).wrapping_add(0x00000058), r[V0] as u32);
 if t { l = 0x004b035c; continue; }
l = 0x004b0000; continue; }
0x004b0000 => {
{ let (a, b) = c.rd128((r[T7] as u32)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x004b0004; continue; }
0x004b0004 => {
r[AT] = sx(0x00000080);
l = 0x004b0008; continue; }
0x004b0008 => {
{ let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000010)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
l = 0x004b000c; continue; }
0x004b000c => {
r[T2] = ((r[T2] as i64) >> 32) as u64;
l = 0x004b0010; continue; }
0x004b0010 => {
{ let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000020)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = uycpu::mmi::pminw((r[T2], h[T2]), (r[AT], h[AT])); r[T2] = a; h[T2] = b; }
uycpu::vu0::fmac::<6, 14>(c, 1, 9, uycpu::vu0::bcast(c.vf[4][3]));
c.wr32((r[SP] as u32).wrapping_add(0x00000044), r[T2] as u32);
uycpu::vu0::fmac::<6, 14>(c, 2, 10, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 14>(c, 3, 11, uycpu::vu0::bcast(c.vf[4][3]));
let t = r[T5] != 0u64;  uycpu::vu0::fmac::<1, 14>(c, 4, 4, c.vf[24]);
 if t { l = 0x004b01d4; continue; }
l = 0x004b0030; continue; }
0x004b0030 => {
let t = r[T6] == 0u64;  r[FP] = add32(r[FP], 0x00000060);
 if t { l = 0x004b0624; continue; }
uycpu::vu0::fmac::<6, 15>(c, 32, 28, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 29, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::fmac::<2, 15>(c, 1, 30, uycpu::vu0::bcast(c.vf[1][2]));
l = 0x004b0044; continue; }
0x004b0044 => {
uycpu::vu0::fmac::<6, 14>(c, 4, 4, uycpu::vu0::bcast(c.vf[24][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 28, uycpu::vu0::bcast(c.vf[2][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 29, uycpu::vu0::bcast(c.vf[2][1]));
uycpu::vu0::fmac::<2, 15>(c, 2, 30, uycpu::vu0::bcast(c.vf[2][2]));
uycpu::vu0::fmac::<6, 15>(c, 32, 28, uycpu::vu0::bcast(c.vf[3][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 29, uycpu::vu0::bcast(c.vf[3][1]));
uycpu::vu0::fmac::<2, 15>(c, 3, 30, uycpu::vu0::bcast(c.vf[3][2]));
uycpu::vu0::fmac::<6, 15>(c, 32, 28, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 29, uycpu::vu0::bcast(c.vf[4][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 30, uycpu::vu0::bcast(c.vf[4][2]));
uycpu::vu0::fmac::<2, 15>(c, 4, 31, uycpu::vu0::bcast(c.vf[0][3]));
l = 0x004b0078; continue; }
0x004b0078 => {
r[V1] = c.rd16((r[S0] as u32).wrapping_add(0x0000003c)) as u64;
r[T8] = add32(r[T8], 0x00000006);
{ let v = c.vf[1]; c.wr128((r[SP] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[V1] = r[V1] & 0x60;
{ let v = c.vf[2]; c.wr128((r[SP] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[AT] = r[SP] & 0x600;
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000020), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[V0] = sx(0x00000600);
let t = r[V1] == 0u64;  { let v = c.vf[4]; c.wr128((r[SP] as u32).wrapping_add(0x00000030), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x004b0194; continue; }
{ let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000030)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[V1] = r[V1] & 0x40;
let t = r[V1] != 0u64;  { let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000030)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 if t { l = 0x004b0138; continue; }
uycpu::vu0::fmac::<6, 14>(c, 9, 9, uycpu::vu0::bcast(c.vf[9][3]));
uycpu::vu0::fmac::<6, 14>(c, 10, 10, uycpu::vu0::bcast(c.vf[10][3]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[11][3]));
uycpu::vu0::fmac::<6, 14>(c, 32, 25, uycpu::vu0::bcast(c.vf[9][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 26, uycpu::vu0::bcast(c.vf[9][1]));
l = 0x004b00c4; continue; }
0x004b00c4 => {
uycpu::vu0::fmac::<2, 14>(c, 1, 27, uycpu::vu0::bcast(c.vf[9][2]));
uycpu::vu0::fmac::<6, 14>(c, 32, 25, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 26, uycpu::vu0::bcast(c.vf[10][1]));
uycpu::vu0::fmac::<2, 14>(c, 2, 27, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::fmac::<6, 14>(c, 32, 25, uycpu::vu0::bcast(c.vf[11][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 26, uycpu::vu0::bcast(c.vf[11][1]));
uycpu::vu0::fmac::<2, 14>(c, 3, 27, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::fmac::<1, 15>(c, 4, 24, c.vf[4]);
uycpu::vu0::fmac::<4, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<6, 14>(c, 32, 25, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 26, uycpu::vu0::bcast(c.vf[4][1]));
uycpu::vu0::fmac::<2, 14>(c, 4, 27, uycpu::vu0::bcast(c.vf[4][2]));
uycpu::vu0::fmac::<1, 15>(c, 4, 0, c.vf[4]);
uycpu::vu0::fmac::<6, 15>(c, 6, 4, c.vf[4]);
uycpu::vu0::fmac::<0, 2>(c, 32, 6, uycpu::vu0::bcast(c.vf[6][1]));
uycpu::vu0::fmac::<2, 2>(c, 6, 5, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::qop::<2>(c, c.vf[0][3], c.vf[6][2]);
l = 0x004b0108; continue; }
0x004b0108 => {
{ let v = c.vf[1]; c.wr128((r[SP] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[T8] = add32(r[T8], 0x00000004);
{ let v = c.vf[2]; c.wr128((r[SP] as u32).wrapping_add(0x00000070), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[FP] = add32(r[FP], 0x00000040);
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000080), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<6, 15>(c, 4, 4, uycpu::vu0::bcast(c.vi[22]));
{ let v = c.vf[4]; c.wr128((r[SP] as u32).wrapping_add(0x00000090), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[SP] = add32(r[SP], 0x00000040);
 l = 0x004b0194; continue;
}
0x004b0138 => {
uycpu::vu0::fmac::<6, 14>(c, 9, 9, uycpu::vu0::bcast(c.vf[9][3]));
uycpu::vu0::fmac::<6, 14>(c, 10, 10, uycpu::vu0::bcast(c.vf[10][3]));
uycpu::vu0::fmac::<6, 14>(c, 11, 11, uycpu::vu0::bcast(c.vf[11][3]));
uycpu::vu0::fmac::<1, 14>(c, 4, 24, c.vf[4]);
uycpu::vu0::fmac::<0, 8>(c, 5, 0, uycpu::vu0::bcast(c.vf[9][0]));
uycpu::vu0::fmac::<0, 4>(c, 6, 0, uycpu::vu0::bcast(c.vf[10][1]));
uycpu::vu0::fmac::<0, 2>(c, 7, 0, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::fmac::<0, 4>(c, 5, 0, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<0, 8>(c, 6, 0, uycpu::vu0::bcast(c.vf[9][1]));
uycpu::vu0::fmac::<0, 8>(c, 7, 0, uycpu::vu0::bcast(c.vf[9][2]));
uycpu::vu0::fmac::<0, 2>(c, 5, 0, uycpu::vu0::bcast(c.vf[11][0]));
uycpu::vu0::fmac::<0, 2>(c, 6, 0, uycpu::vu0::bcast(c.vf[11][1]));
uycpu::vu0::fmac::<0, 4>(c, 7, 0, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 6, uycpu::vu0::bcast(c.vf[4][1]));
uycpu::vu0::fmac::<2, 14>(c, 4, 7, uycpu::vu0::bcast(c.vf[4][2]));
uycpu::vu0::fmac::<6, 8>(c, 4, 4, uycpu::vu0::bcast(c.vf[9][3]));
uycpu::vu0::fmac::<6, 4>(c, 4, 4, uycpu::vu0::bcast(c.vf[10][3]));
uycpu::vu0::fmac::<6, 2>(c, 4, 4, uycpu::vu0::bcast(c.vf[11][3]));
{ let v = c.vf[4]; c.wr128((r[SP] as u32).wrapping_add(0x00000060), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[T8] = add32(r[T8], 0x00000001);
r[FP] = add32(r[FP], 0x00000010);
r[SP] = add32(r[SP], 0x00000010);
l = 0x004b0194; continue; }
0x004b0194 => {
let t = r[AT] != r[V0];  r[SP] = add32(r[SP], 0x00000060);
 if t { l = 0x004afce0; continue; }
r[V0] = add32(r[RA], 0x00000000);
r[A0] = 0x0000000010010000;
r[AT] = sub32(r[SP], r[V0]);
c.wr32((r[A0] as u32).wrapping_add(0xffffd080), r[V0] as u32);
r[V1] = sub32(r[FP], r[AT]);
c.wr32((r[A0] as u32).wrapping_add(0xffffd010), r[V1] as u32);
r[V1] = srl(r[AT], 4);
c.wr32((r[A0] as u32).wrapping_add(0xffffd020), r[V1] as u32);
r[V1] = 0u64 | 0x100;
c.wr32((r[A0] as u32).wrapping_add(0xffffd000), r[V1] as u32);
f4b0964(c, k, &mut r);
r[SP] = r[V0];
 l = 0x004afce0; continue;
}
0x004b01d4 => {
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003e10)); c.vf[9] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003e00)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let v = c.vf[12]; c.wr128((r[RA] as u32).wrapping_add(0x00003e30), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
uycpu::vu0::fmac::<0, 8>(c, 11, 0, uycpu::vu0::bcast(c.vf[0][3]));
r[A1] = c.rd32((r[RA] as u32).wrapping_add(0x00003e20)) as i32 as i64 as u64;
r[AT] = srl(r[T5], 1);
uycpu::vu0::fmac::<6, 14>(c, 12, 4, c.vf[4]);
r[A2] = r[RA] | 0x3d00;
uycpu::vu0::fmac::<0, 8>(c, 32, 12, uycpu::vu0::bcast(c.vf[12][1]));
uycpu::vu0::fmac::<2, 8>(c, 12, 11, uycpu::vu0::bcast(c.vf[12][2]));
uycpu::vu0::fmac::<1, 4>(c, 0, 9, uycpu::vu0::bcast(c.vf[12][0]));
r[A0] = c.cfc2(17) as i32 as i64 as u64;
uycpu::vu0::fmac::<6, 8>(c, 12, 12, uycpu::vu0::bcast(c.vf[9][2]));
r[A0] = r[A0] & 0x40;
let t = r[A0] != 0u64;  uycpu::vu0::fmac::<1, 1>(c, 12, 0, uycpu::vu0::bcast(c.vf[12][0]));
 if t { l = 0x004b034c; continue; }
uycpu::vu0::fmac::<6, 15>(c, 5, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[A3] = srl(r[A3], 9);
uycpu::vu0::fmac::<6, 15>(c, 6, 0, uycpu::vu0::bcast(c.vf[0][0]));
r[A3] = r[A3] & 0x1ff;
{ let v = c.vf[0]; uycpu::vu0::mov::<15>(c, 7, [v[1], v[2], v[3], v[0]]); }
r[V0] = sll(r[A3], 6);
uycpu::vu0::fmac::<0, 8>(c, 5, 5, uycpu::vu0::bcast(c.vf[0][3]));
r[A0] = sll(r[A3], 1);
uycpu::vu0::fmac::<0, 4>(c, 6, 6, uycpu::vu0::bcast(c.vf[0][3]));
r[V0] = add32(r[V0], r[A0]);
r[V0] = add32(r[V0], r[A3]);
r[V1] = sll(r[A3], 7);
r[A0] = sll(r[A3], 2);
r[V1] = sub32(r[V1], r[A0]);
r[V1] = sub32(r[V1], r[A3]);
r[V0] = add32(r[V0], r[A1]);
let t = r[AT] == 0u64;  r[V1] = add32(r[V1], r[A1]);
 if t { l = 0x004b0274; continue; }
r[V1] = add32(r[V1], r[A1]);
l = 0x004b0274; continue; }
0x004b0274 => {
r[A0] = r[V0] & 0xff;
r[A0] = add32(r[A0], r[A2]);
r[A0] = c.rd8((r[A0] as u32)) as i8 as i64 as u64;
r[A0] = sll(r[A0], 5);
c.vf[10] = [r[A0] as u32, (r[A0] >> 32) as u32, h[A0] as u32, (h[A0] >> 32) as u32];
uycpu::vu0::itof::<2, 15>(c, 10, 10);
r[A0] = srl(r[V0], 1);
r[A0] = r[A0] & 0xff;
r[A0] = add32(r[A0], r[A2]);
r[A0] = c.rd8((r[A0] as u32)) as i8 as i64 as u64;
r[A0] = sll(r[A0], 5);
c.vf[11] = [r[A0] as u32, (r[A0] >> 32) as u32, h[A0] as u32, (h[A0] >> 32) as u32];
uycpu::vu0::itof::<2, 15>(c, 11, 11);
uycpu::vu0::fmac::<6, 8>(c, 7, 10, c.vf[11]);
r[A0] = add32(r[V1], 0x00000040);
uycpu::vu0::fmac::<1, 1>(c, 7, 0, uycpu::vu0::bcast(c.vf[9][0]));
r[A0] = r[A0] & 0xff;
uycpu::vu0::fmac::<6, 8>(c, 7, 7, uycpu::vu0::bcast(c.vf[9][0]));
r[A0] = add32(r[A0], r[A2]);
uycpu::vu0::fmac::<0, 8>(c, 7, 7, uycpu::vu0::bcast(c.vf[7][3]));
r[A0] = c.rd8((r[A0] as u32)) as i8 as i64 as u64;
r[A0] = sll(r[A0], 5);
c.vf[10] = [r[A0] as u32, (r[A0] >> 32) as u32, h[A0] as u32, (h[A0] >> 32) as u32];
uycpu::vu0::itof::<2, 15>(c, 10, 10);
r[A0] = srl(r[V1], 1);
r[A0] = r[A0] & 0xff;
r[A0] = add32(r[A0], r[A2]);
r[A0] = c.rd8((r[A0] as u32)) as i8 as i64 as u64;
r[A0] = sll(r[A0], 5);
c.vf[11] = [r[A0] as u32, (r[A0] >> 32) as u32, h[A0] as u32, (h[A0] >> 32) as u32];
uycpu::vu0::itof::<2, 15>(c, 11, 11);
uycpu::vu0::fmac::<6, 8>(c, 7, 7, uycpu::vu0::bcast(c.vf[8][0]));
uycpu::vu0::fmac::<6, 8>(c, 32, 10, uycpu::vu0::bcast(c.vf[8][1]));
uycpu::vu0::fmac::<2, 8>(c, 10, 7, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::fmac::<6, 8>(c, 10, 10, uycpu::vu0::bcast(c.vf[12][3]));
uycpu::vu0::fmac::<6, 8>(c, 32, 11, uycpu::vu0::bcast(c.vf[8][1]));
uycpu::vu0::fmac::<2, 8>(c, 11, 7, uycpu::vu0::bcast(c.vf[8][3]));
let t = r[AT] != 0u64;  uycpu::vu0::fmac::<6, 8>(c, 11, 11, uycpu::vu0::bcast(c.vf[12][3]));
 if t { l = 0x004b031c; continue; }
uycpu::vu0::fmac::<6, 8>(c, 10, 10, uycpu::vu0::bcast(c.vf[9][3]));
uycpu::vu0::fmac::<6, 8>(c, 11, 11, uycpu::vu0::bcast(c.vf[9][3]));
l = 0x004b031c; continue; }
0x004b031c => {
uycpu::vu0::fmac::<0, 8>(c, 7, 0, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<0, 4>(c, 7, 0, uycpu::vu0::bcast(c.vf[11][0]));
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 6, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::fmac::<2, 14>(c, 1, 7, uycpu::vu0::bcast(c.vf[1][2]));
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[2][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 6, uycpu::vu0::bcast(c.vf[2][1]));
uycpu::vu0::fmac::<2, 14>(c, 2, 7, uycpu::vu0::bcast(c.vf[2][2]));
uycpu::vu0::fmac::<6, 14>(c, 32, 5, uycpu::vu0::bcast(c.vf[3][0]));
uycpu::vu0::fmac::<2, 14>(c, 32, 6, uycpu::vu0::bcast(c.vf[3][1]));
uycpu::vu0::fmac::<2, 14>(c, 3, 7, uycpu::vu0::bcast(c.vf[3][2]));
l = 0x004b034c; continue; }
0x004b034c => {
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003e30)); c.vf[12] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
 l = 0x004b0030; continue;
}
0x004b035c => {
r[A1] = srl(r[A1], 7);
c.wr8((r[S6] as u32).wrapping_add(0x0000001f), r[A2] as u8);
r[A1] = ((r[A1] as i64) < 1) as u64;
r[A2] = sll(r[A2], 1);
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003f94)) as i32 as i64 as u64;
r[A2] = add32(r[A2], r[S0]);
r[A2] = c.rd16((r[A2] as u32).wrapping_add(0x00000024)) as u64;
r[A1] = sll(r[A1], 15);
r[V0] = add32(r[RA], 0x00001ffe);
r[A1] = r[A1] | r[T9];
let t = r[AT] == r[V0];  c.wr16((r[S6] as u32).wrapping_add(0x00000016), r[A2] as u16);
 if t { l = 0x004b0000; continue; }
c.wr16((r[AT] as u32), r[A1] as u16);
r[V0] = add32(r[AT], 0x00000002);
c.wr32((r[RA] as u32).wrapping_add(0x00003f94), r[V0] as u32);
 l = 0x004b0000; continue;
}
0x004b039c => {
n[3] += 1;
uycpu::vu0::fmac::<6, 15>(c, 9, 1, uycpu::vu0::bcast(c.vf[0][3]));
{ let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000030)); c.vf[4] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[T7] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000010)); c.vf[2] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[T7] as u32).wrapping_add(0x00000020)); c.vf[3] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x00000080)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x00000090)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 14>(c, 1, 1, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 14>(c, 2, 2, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<6, 14>(c, 3, 3, uycpu::vu0::bcast(c.vf[4][3]));
uycpu::vu0::fmac::<1, 14>(c, 4, 4, c.vf[24]);
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::fmac::<2, 15>(c, 1, 14, uycpu::vu0::bcast(c.vf[1][2]));
uycpu::vu0::fmac::<6, 14>(c, 4, 4, uycpu::vu0::bcast(c.vf[24][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[2][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[2][1]));
uycpu::vu0::fmac::<2, 15>(c, 2, 14, uycpu::vu0::bcast(c.vf[2][2]));
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x000000a0)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[3][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[3][1]));
uycpu::vu0::fmac::<2, 15>(c, 3, 14, uycpu::vu0::bcast(c.vf[3][2]));
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x000000b0)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[4][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 14, uycpu::vu0::bcast(c.vf[4][2]));
uycpu::vu0::fmac::<2, 15>(c, 4, 15, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[5][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[5][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[5][2]));
uycpu::vu0::fmac::<2, 15>(c, 5, 4, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[6][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[6][2]));
uycpu::vu0::fmac::<2, 15>(c, 6, 4, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[7][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::fmac::<2, 15>(c, 7, 4, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 6, 6);
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[8][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[8][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::fmac::<2, 15>(c, 8, 4, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 7, 7);
uycpu::vu0::fmac::<6, 12>(c, 5, 5, c.vf[21]);
uycpu::vu0::fmac::<6, 12>(c, 6, 6, c.vf[21]);
uycpu::vu0::clip(c, 8, 8);
uycpu::vu0::fmac::<6, 12>(c, 7, 7, c.vf[21]);
uycpu::vu0::fmac::<6, 12>(c, 8, 8, c.vf[21]);
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x000000c0)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x000000d0)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[AT] = c.cfc2(18) as i32 as i64 as u64;
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::clip(c, 6, 6);
uycpu::vu0::clip(c, 7, 7);
uycpu::vu0::clip(c, 8, 8);
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[10][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::fmac::<2, 15>(c, 5, 4, uycpu::vu0::bcast(c.vf[0][3]));
r[V1] = c.cfc2(18) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x000000e0)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[S0] as u32).wrapping_add(0x000000f0)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[11][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[11][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::fmac::<2, 15>(c, 6, 4, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[7][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::fmac::<2, 15>(c, 7, 4, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 6, 6);
uycpu::vu0::fmac::<6, 15>(c, 32, 1, uycpu::vu0::bcast(c.vf[8][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 2, uycpu::vu0::bcast(c.vf[8][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 3, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::fmac::<2, 15>(c, 8, 4, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 7, 7);
uycpu::vu0::fmac::<6, 12>(c, 5, 5, c.vf[21]);
uycpu::vu0::fmac::<6, 12>(c, 6, 6, c.vf[21]);
uycpu::vu0::clip(c, 8, 8);
uycpu::vu0::fmac::<6, 12>(c, 7, 7, c.vf[21]);
r[A1] = srl(r[AT], 6);
uycpu::vu0::fmac::<6, 12>(c, 8, 8, c.vf[21]);
r[A2] = srl(r[AT], 12);
r[A3] = srl(r[AT], 18);
r[AT] = r[AT] & r[A1];
r[A2] = r[A2] & r[A3];
r[V0] = c.cfc2(18) as i32 as i64 as u64;
r[AT] = r[AT] & r[A2];
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::clip(c, 6, 6);
r[A1] = srl(r[V0], 6);
uycpu::vu0::clip(c, 7, 7);
r[A2] = srl(r[V0], 12);
uycpu::vu0::clip(c, 8, 8);
r[A3] = srl(r[V0], 18);
r[V0] = r[V0] & r[A1];
r[A2] = r[A2] & r[A3];
r[V0] = r[V0] & r[A2];
r[AT] = r[AT] & r[V0];
r[AT] = r[AT] & 0x3f;
let t = r[AT] != 0u64;  r[A0] = c.cfc2(18) as i32 as i64 as u64;
 if t { l = 0x004afce0; continue; }
uycpu::vu0::fmac::<6, 15>(c, 1, 9, uycpu::vu0::bcast(c.vf[0][3]));
r[V1] = r[V1] | r[A0];
r[T6] = ((r[V1] as i64) < 1) as u64;
 l = 0x004afdf8; continue;
}
0x004b0574 => {
r[S2] = c.rd32((r[S2] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[S5] = add32(r[S5], 0xfffffff8);
r[V1] = c.rd32((r[S0] as u32)) as i32 as i64 as u64;
r[A0] = c.rd8((r[S0] as u32).wrapping_add(0x0000000c)) as u64;
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000050), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x004b05ec; continue;
}
0x004b0594 => {
r[S2] = c.rd32((r[S2] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[S5] = add32(r[S5], 0xfffffff8);
r[V1] = c.rd32((r[S0] as u32)) as i32 as i64 as u64;
r[A0] = c.rd8((r[S0] as u32).wrapping_add(0x0000000c)) as u64;
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x004b05ec; continue;
}
0x004b05b4 => {
r[S3] = c.rd32((r[S3] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[S5] = add32(r[S5], 0xfffffff8);
r[V1] = c.rd32((r[S0] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[A0] = c.rd8((r[S0] as u32).wrapping_add(0x0000000d)) as u64;
{ let v = c.vf[3]; c.wr128((r[SP] as u32).wrapping_add(0x00000040), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 l = 0x004b05ec; continue;
}
0x004b05d4 => {
r[S4] = c.rd32((r[S4] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
r[S5] = add32(r[S5], 0xfffffff8);
r[V1] = c.rd32((r[S0] as u32).wrapping_add(0x00000008)) as i32 as i64 as u64;
r[A0] = c.rd8((r[S0] as u32).wrapping_add(0x0000000e)) as u64;
l = 0x004b05ec; continue; }
0x004b05ec => {
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003f9c)) as i32 as i64 as u64;
r[V0] = add32(r[RA], 0x00003c00);
let t = r[AT] == r[V0];  r[V0] = c.rd32((r[S0] as u32).wrapping_add(0x0000001c)) as i32 as i64 as u64;
 if t { l = 0x004affc0; continue; }
c.wr32((r[AT] as u32).wrapping_add(0x00000008), r[V1] as u32);
r[T3] = add32(r[T3], 0x00000001);
c.wr32((r[AT] as u32).wrapping_add(0x0000000c), r[A0] as u32);
c.wr32((r[AT] as u32).wrapping_add(0x00000004), r[V0] as u32);
r[T0] = r[AT];
r[AT] = add32(r[AT], 0x00000010);
c.wr32((r[RA] as u32).wrapping_add(0x00003f9c), r[AT] as u32);
 l = 0x004affc0; continue;
}
0x004b0624 => {
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003f40)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003f50)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003f60)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003f70)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 32, 5, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 6, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::fmac::<2, 15>(c, 1, 7, uycpu::vu0::bcast(c.vf[1][2]));
uycpu::vu0::fmac::<6, 14>(c, 4, 4, uycpu::vu0::bcast(c.vf[24][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 5, uycpu::vu0::bcast(c.vf[2][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 6, uycpu::vu0::bcast(c.vf[2][1]));
uycpu::vu0::fmac::<2, 15>(c, 2, 7, uycpu::vu0::bcast(c.vf[2][2]));
uycpu::vu0::fmac::<6, 15>(c, 32, 5, uycpu::vu0::bcast(c.vf[3][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 6, uycpu::vu0::bcast(c.vf[3][1]));
uycpu::vu0::fmac::<2, 15>(c, 3, 7, uycpu::vu0::bcast(c.vf[3][2]));
uycpu::vu0::fmac::<6, 15>(c, 32, 5, uycpu::vu0::bcast(c.vf[4][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 6, uycpu::vu0::bcast(c.vf[4][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 7, uycpu::vu0::bcast(c.vf[4][2]));
uycpu::vu0::fmac::<2, 15>(c, 4, 8, uycpu::vu0::bcast(c.vf[0][3]));
 l = 0x004b0078; continue;
}
0x004b067c => {
uycpu::vu0::ftoi::<0, 8>(c, 17, 17);
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003fa0)) as i32 as i64 as u64;
r[V0] = c.rd32((r[RA] as u32).wrapping_add(0x00003f90)) as i32 as i64 as u64;
r[V1] = add32(r[S5], 0xfffff000);
r[A0] = c.rd32((r[RA] as u32).wrapping_add(0x00003fa4)) as i32 as i64 as u64;
r[V1] = r[V1] & 0xfff;
c.wr128((r[AT] as u32), 0u64, 0u64);
r[A1] = add32(r[AT], 0x00000010);
c.wr32((r[RA] as u32).wrapping_add(0x00003fa0), r[A1] as u32);
{ let (a, b) = { let v = c.vf[17]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[A1] = a; h[A1] = b; }
c.wr32((r[A0] as u32), r[A1] as u32);
r[A0] = add32(r[A0], 0x00000004);
c.wr32((r[RA] as u32).wrapping_add(0x00003fa4), r[A0] as u32);
r[A1] = add32(r[V0], 0xffffe000);
let t = r[V1] == 0u64;  c.wr32((r[AT] as u32), r[S0] as u32);
 if t { l = 0x004afc68; continue; }
r[A1] = r[A1] & 0xfff;
c.wr8((r[AT] as u32).wrapping_add(0x0000000f), r[T3] as u8);
c.wr16((r[AT] as u32).wrapping_add(0x00000008), r[A1] as u16);
let t = r[S2] == 0u64;  r[V1] = 0;
 if t { l = 0x004b0704; continue; }
l = 0x004b06dc; continue; }
0x004b06dc => {
r[A0] = c.rd32((r[S2] as u32)) as i32 as i64 as u64;
r[S2] = c.rd32((r[S2] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.wr32((r[V0] as u32), r[A0] as u32);
r[V0] = add32(r[V0], 0x00000004);
let t = r[S2] != 0u64;  r[V1] = add32(r[V1], 0x00000001);
 if t { l = 0x004b06dc; continue; }
c.wr8((r[AT] as u32).wrapping_add(0x0000000c), r[V1] as u8);
l = 0x004b0704; continue; }
0x004b0704 => {
let t = r[S3] == 0u64;  r[V1] = 0;
 if t { l = 0x004b0734; continue; }
l = 0x004b070c; continue; }
0x004b070c => {
r[A0] = c.rd32((r[S3] as u32)) as i32 as i64 as u64;
r[S3] = c.rd32((r[S3] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.wr32((r[V0] as u32), r[A0] as u32);
r[V0] = add32(r[V0], 0x00000004);
let t = r[S3] != 0u64;  r[V1] = add32(r[V1], 0x00000001);
 if t { l = 0x004b070c; continue; }
c.wr8((r[AT] as u32).wrapping_add(0x0000000d), r[V1] as u8);
l = 0x004b0734; continue; }
0x004b0734 => {
let t = r[S4] == 0u64;  r[V1] = 0;
 if t { l = 0x004b0764; continue; }
l = 0x004b073c; continue; }
0x004b073c => {
r[A0] = c.rd32((r[S4] as u32)) as i32 as i64 as u64;
r[S4] = c.rd32((r[S4] as u32).wrapping_add(0x00000004)) as i32 as i64 as u64;
c.wr32((r[V0] as u32), r[A0] as u32);
r[V0] = add32(r[V0], 0x00000004);
let t = r[S4] != 0u64;  r[V1] = add32(r[V1], 0x00000001);
 if t { l = 0x004b073c; continue; }
c.wr8((r[AT] as u32).wrapping_add(0x0000000e), r[V1] as u8);
l = 0x004b0764; continue; }
0x004b0764 => {
c.wr32((r[RA] as u32).wrapping_add(0x00003f90), r[V0] as u32);
 l = 0x004afc68; continue;
}
0x004b076c => {
r[V0] = add32(r[RA], 0x00000000);
r[A0] = 0x0000000010010000;
r[AT] = sub32(r[SP], r[V0]);
c.wr32((r[A0] as u32).wrapping_add(0xffffd080), r[V0] as u32);
r[V1] = sub32(r[FP], r[AT]);
c.wr32((r[A0] as u32).wrapping_add(0xffffd010), r[V1] as u32);
r[V1] = srl(r[AT], 4);
let t = r[V1] == 0u64;   if t { l = 0x004b07a4; continue; }
c.wr32((r[A0] as u32).wrapping_add(0xffffd020), r[V1] as u32);
r[V1] = 0u64 | 0x100;
c.wr32((r[A0] as u32).wrapping_add(0xffffd000), r[V1] as u32);
f4b0964(c, k, &mut r);
l = 0x004b07a4; continue; }
0x004b07a4 => {
r[AT] = add32(r[RA], 0x00003800);
r[V0] = c.rd32((r[RA] as u32).wrapping_add(0x00003f9c)) as i32 as i64 as u64;
let t = r[AT] == r[V0];  r[V1] = c.rd32((r[RA] as u32).wrapping_add(0x00003f90)) as i32 as i64 as u64;
 if t { l = 0x004b07dc; continue; }
l = 0x004b07b4; continue; }
0x004b07b4 => {
r[A0] = c.rd32((r[AT] as u32)) as i32 as i64 as u64;
r[AT] = add32(r[AT], 0x00000010);
c.wr32((r[V1] as u32), r[A0] as u32);
let t = r[AT] != r[V0];  r[V1] = add32(r[V1], 0x00000004);
 if t { l = 0x004b07b4; continue; }
c.wr32((r[RA] as u32).wrapping_add(0x00003f90), r[V1] as u32);
l = 0x004b07dc; continue; }
0x004b07dc => {
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003fa8)) as i32 as i64 as u64;
c.wr128((r[AT] as u32), 0u64, 0u64);
r[V0] = 0x0000000020000000;
c.wr32((r[AT] as u32), r[V0] as u32);
c.wr32((r[AT] as u32).wrapping_add(0x00000004), r[FP] as u32);
r[AT] = c.rd32((r[RA] as u32).wrapping_add(0x00003fa0)) as i32 as i64 as u64;
c.wr128((r[AT] as u32), 0u64, 0u64);
l = 0x004b0800; continue; }
0x004b0800 => {
c.wr32(k.gp(c, gp, 0x004b0804), r[FP] as u32);
r[A0] = 0x0000000010010000;
r[A1] = 0u64 | 0x100;
r[S0] = c.rd16((r[RA] as u32).wrapping_add(0x00003f90)) as u64;
r[S1] = c.rd16((r[RA] as u32).wrapping_add(0x00003f9c)) as u64;
r[S2] = c.rd16((r[RA] as u32).wrapping_add(0x00003f98)) as u64;
r[S3] = c.rd16((r[RA] as u32).wrapping_add(0x00003f94)) as u64;
r[S0] = add32(r[S0], 0xffffe000);
r[S1] = add32(r[S1], 0xffffc800);
r[S2] = add32(r[S2], 0xffffcc00);
r[S3] = add32(r[S3], 0xffffe800);
c.wr32(k.gp(c, gp, 0x004b0834), r[S0] as u32);
c.wr32(k.gp(c, gp, 0x004b0838), r[S1] as u32);
c.wr32(k.gp(c, gp, 0x004b083c), r[S2] as u32);
c.wr32(k.gp(c, gp, 0x004b0840), r[S3] as u32);
r[AT] = add32(r[RA], 0x00000800);
r[V0] = sx(k.hi(c, 0x004b0848));
r[V0] = add32(r[V0], k.lo(c, 0x004b084c) as u64);
r[V1] = sx(0x00000080);
c.wr32((r[A0] as u32).wrapping_add(0xffffd080), r[AT] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd010), r[V0] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd020), r[V1] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd000), r[A1] as u32);
f4b0964(c, k, &mut r);
r[AT] = add32(r[RA], 0x00002000);
r[V0] = sx(k.hi(c, 0x004b0870));
r[V0] = add32(r[V0], k.lo(c, 0x004b0874) as u64);
r[S0] = add32(r[S0], 0x0000000c);
c.wr32((r[A0] as u32).wrapping_add(0xffffd080), r[AT] as u32);
r[S0] = srl(r[S0], 4);
let t = r[S0] == 0u64;  c.wr32((r[A0] as u32).wrapping_add(0xffffd010), r[V0] as u32);
 if t { l = 0x004b089c; continue; }
c.wr32((r[A0] as u32).wrapping_add(0xffffd020), r[S0] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd000), r[A1] as u32);
f4b0964(c, k, &mut r);
l = 0x004b089c; continue; }
0x004b089c => {
r[AT] = add32(r[RA], 0x00003800);
r[V0] = sx(k.hi(c, 0x004b08a0));
r[V0] = add32(r[V0], k.lo(c, 0x004b08a4) as u64);
r[V1] = srl(r[S1], 4);
let t = r[V1] == 0u64;  c.wr32((r[A0] as u32).wrapping_add(0xffffd080), r[AT] as u32);
 if t { l = 0x004b08cc; continue; }
c.wr32((r[A0] as u32).wrapping_add(0xffffd010), r[V0] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd020), r[V1] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd000), r[A1] as u32);
f4b0964(c, k, &mut r);
l = 0x004b08cc; continue; }
0x004b08cc => {
r[AT] = add32(r[RA], 0x00003400);
r[V0] = sx(k.hi(c, 0x004b08d0));
r[V0] = add32(r[V0], k.lo(c, 0x004b08d4) as u64);
r[V1] = add32(r[S2], 0x0000000e);
c.wr32((r[A0] as u32).wrapping_add(0xffffd080), r[AT] as u32);
r[V1] = srl(r[V1], 4);
let t = r[V1] == 0u64;  c.wr32((r[A0] as u32).wrapping_add(0xffffd010), r[V0] as u32);
 if t { l = 0x004b08fc; continue; }
c.wr32((r[A0] as u32).wrapping_add(0xffffd020), r[V1] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd000), r[A1] as u32);
f4b0964(c, k, &mut r);
l = 0x004b08fc; continue; }
0x004b08fc => {
r[AT] = add32(r[RA], 0x00001800);
r[V0] = sx(k.hi(c, 0x004b0900));
r[V0] = add32(r[V0], k.lo(c, 0x004b0904) as u64);
r[V1] = add32(r[S3], 0x0000000e);
c.wr32((r[A0] as u32).wrapping_add(0xffffd080), r[AT] as u32);
r[V1] = srl(r[V1], 4);
let t = r[V1] == 0u64;  c.wr32((r[A0] as u32).wrapping_add(0xffffd010), r[V0] as u32);
 if t { l = 0x004b092c; continue; }
c.wr32((r[A0] as u32).wrapping_add(0xffffd020), r[V1] as u32);
c.wr32((r[A0] as u32).wrapping_add(0xffffd000), r[A1] as u32);
f4b0964(c, k, &mut r);
l = 0x004b092c; continue; }
0x004b092c => {
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
_ => unreachable!("0x4AFA28: no block at {l:08x}"),
}
}
c.r = r;
c.rh = h;
}

// ---- 0x4ADC98: the terrain cull --------------------------------------------------------------------------------------------------
//
// As the tie cull: its translation's statements in order on its own register file, the relocated words from the code, the
// safe points gone, the polls of SPR_TO (0x4ADE68) and SPR_FROM (0x4AE260, 0x4AE560, 0x4AE5B8, 0x4AE610, 0x4AE768, 0x4AE7E8,
// 0x4AE830) a stop when busy, the view's tangents widened when `wide` (+0x10c: vf24).
#[allow(unused_assignments, unused_parens, clippy::all)]
fn f4adc98(c: &mut Cpu, k: F, wide: bool, n: &mut [u64; N]) {
let (mut r, mut h) = (c.r, c.rh);
let gp = r[GP] as u32;
let mut l: u32 = 0x004adc98;
loop { match l {
0x004adc98 => {
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
r[T8] = c.rd32(k.gp(c, gp, 0x004adcc8)) as i32 as i64 as u64;
r[RA] = 0x0000000070000000;
r[T9] = c.rd32(k.gp(c, gp, 0x004adcd0)) as i32 as i64 as u64;
r[AT] = 0x0000000010000000;
c.wr32((r[RA] as u32).wrapping_add(0x00003f10), r[T8] as u32);
r[AT] = r[AT] | 0xd400;
r[V0] = sx(0x00001000);
r[S3] = add32(r[T8], 0xffffffc0);
c.wr32((r[AT] as u32).wrapping_add(0x00000080), r[V0] as u32);
r[V0] = sx(0x00000080);
c.wr32((r[AT] as u32).wrapping_add(0x00000020), r[V0] as u32);
r[V0] = 0u64 | 0x100;
c.wr32((r[AT] as u32).wrapping_add(0x00000010), r[T8] as u32);
r[T8] = add32(r[T8], 0x00000800);
c.wr32((r[AT] as u32), r[V0] as u32);
r[S5] = sx(0x00000800);
r[FP] = c.rd32(k.gp(c, gp, 0x004add08)) as i32 as i64 as u64;
r[SP] = add32(r[RA], 0x00000000);
r[V0] = add32(r[RA], 0x00003b00);
r[AT] = sx(k.hi(c, 0x004add14));
r[AT] = add32(r[AT], k.lo(c, 0x004add18) as u64);
r[V1] = add32(r[V0], 0x00000080);
l = 0x004add20; continue; }
0x004add20 => {
{ let (a, b) = c.rd128((r[AT] as u32)); r[A0] = a; h[A0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); r[A1] = a; h[A1] = b; }
c.wr128((r[V0] as u32), r[A0], h[A0]);
r[AT] = add32(r[AT], 0x00000020);
c.wr128((r[V0] as u32).wrapping_add(0x00000010), r[A1], h[A1]);
r[V0] = add32(r[V0], 0x00000020);
let t = r[V0] != r[V1];   if t { l = 0x004add20; continue; }
r[V0] = 0x0000000044800000;
r[AT] = sx(k.hi(c, 0x004add44));
r[AT] = add32(r[AT], k.lo(c, 0x004add48) as u64);
c.vf[1] = [r[V0] as u32, (r[V0] >> 32) as u32, h[V0] as u32, (h[V0] >> 32) as u32];
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000140)); c.vf[28] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32)); c.vf[29] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); c.vf[30] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000020)); c.vf[31] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 28, 28, uycpu::vu0::bcast(c.vf[1][0]));
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000100)); c.vf[12] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000110)); c.vf[13] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000120)); c.vf[14] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000130)); c.vf[15] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<1, 14>(c, 1, 0, c.vf[28]);
uycpu::vu0::fmac::<6, 15>(c, 18, 12, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<6, 15>(c, 19, 13, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<6, 15>(c, 20, 14, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 12, uycpu::vu0::bcast(c.vf[1][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 13, uycpu::vu0::bcast(c.vf[1][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 14, uycpu::vu0::bcast(c.vf[1][2]));
uycpu::vu0::fmac::<2, 15>(c, 21, 15, uycpu::vu0::bcast(c.vf[0][3]));
r[AT] = sx(k.hi(c, 0x004add98));
r[AT] = add32(r[AT], k.lo(c, 0x004add9c) as u64);
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000a0)); c.vf[23] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000000b0)); let (h, v) = if wide { (c.widen(a as u32), c.fov((a >> 32) as u32)) } else { (a as u32, (a >> 32) as u32) }; c.vf[24] = [h, v, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000001e0)); c.vf[25] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x000001f0)); c.vf[26] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000200)); c.vf[27] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128(k.gp(c, gp, 0x004addb4)); c.vf[22] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[AT] = c.rd32(k.gp(c, gp, 0x004addb8)) as i32 as i64 as u64;
c.vf[1] = [r[AT] as u32, (r[AT] >> 32) as u32, h[AT] as u32, (h[AT] >> 32) as u32];
uycpu::vu0::fmac::<0, 4>(c, 23, 0, uycpu::vu0::bcast(c.vf[1][0]));
{ let (a, b) = c.rd128(k.gp(c, gp, 0x004addc8)); r[AT] = a; h[AT] = b; }
c.wr128((r[RA] as u32).wrapping_add(0x00003f00), r[AT], h[AT]);
{ let (a, b) = c.rd128(k.gp(c, gp, 0x004addd0)); r[T4] = a; h[T4] = b; }
{ let (a, b) = c.rd128(k.gp(c, gp, 0x004addd4)); r[T5] = a; h[T5] = b; }
{ let (a, b) = c.rd128(k.gp(c, gp, 0x004addd8)); r[T6] = a; h[T6] = b; }
{ let (a, b) = c.rd128(k.gp(c, gp, 0x004adddc)); r[T7] = a; h[T7] = b; }
c.wr128((r[SP] as u32), 0u64, 0u64);
r[V0] = sx(k.hi(c, 0x004adde4));
r[V0] = add32(r[V0], k.lo(c, 0x004adde8) as u64);
r[AT] = sx(k.hi(c, 0x004addec));
r[AT] = add32(r[AT], k.lo(c, 0x004addf0) as u64);
r[V0] = c.rd32((r[V0] as u32)) as i32 as i64 as u64;
c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
r[AT] = 0x0000000030000000;
c.wr32((r[SP] as u32), r[AT] as u32);
r[FP] = add32(r[FP], 0x00000010);
c.wr16((r[SP] as u32), r[V0] as u16);
r[SP] = add32(r[SP], 0x00000010);
r[AT] = sx(k.hi(c, 0x004ade10));
r[AT] = add32(r[AT], k.lo(c, 0x004ade14) as u64);
r[V0] = sx(0x00000011);
l = 0x004ade20; continue; }
0x004ade20 => {
{ let (a, b) = c.rd128((r[AT] as u32)); r[V1] = a; h[V1] = b; }
r[AT] = add32(r[AT], 0x00000010);
r[V0] = add32(r[V0], 0xffffffff);
r[FP] = add32(r[FP], 0x00000010);
c.wr128((r[SP] as u32), r[V1], h[V1]);
r[SP] = add32(r[SP], 0x00000010);
let t = (r[V0] as i64) > 0;   if t { l = 0x004ade20; continue; }
r[S0] = add32(r[RA], 0x00003000);
r[S1] = add32(r[RA], 0x00003d00);
r[S2] = add32(r[RA], 0x00003400);
l = 0x004ade50; continue; }
0x004ade50 => {
let t = r[T9] == 0u64;   if t { l = 0x004ae520; continue; }
n[4] += 1;
r[S5] = r[S5] ^ 0x800;
r[V0] = 0x0000000010000000;
r[S6] = add32(r[RA], 0x00001000);
r[V0] = r[V0] | 0xd400;
l = 0x004ade68; continue; }
0x004ade68 => {
r[AT] = c.rd32((r[V0] as u32)) as i32 as i64 as u64;
r[AT] = r[AT] & 0x100;
if r[AT] != 0u64 { busy(k, 0x004ade68) }
r[S6] = add32(r[S6], r[S5]);
r[AT] = sx(0x00000020);
let t = 0u64 != 0u64;  { let (a, b) = uycpu::mmi::pminw((r[AT], h[AT]), (r[T9], h[T9])); r[AT] = a; h[AT] = b; }
 if t { l = 0x004ade50; continue; }
r[T9] = sub32(r[T9], r[AT]);
r[AT] = sll(r[AT], 6);
let t = r[T9] == 0u64;  r[S7] = add32(r[S6], r[AT]);
 if t { l = 0x004adec0; continue; }
r[V1] = r[S6] ^ 0x800;
c.wr32((r[V0] as u32).wrapping_add(0x00000010), r[T8] as u32);
c.wr32((r[V0] as u32).wrapping_add(0x00000080), r[V1] as u32);
r[V1] = sx(0x00000080);
c.wr32((r[V0] as u32).wrapping_add(0x00000020), r[V1] as u32);
r[V1] = 0u64 | 0x100;
c.wr32((r[V0] as u32), r[V1] as u32);
r[T8] = add32(r[T8], 0x00000800);
l = 0x004adec0; continue; }
0x004adec0 => {
{ let (a, b) = c.rd128((r[S6] as u32)); c.vf[1] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
let t = r[S6] == r[S7];  r[S4] = add32(r[S6], 0x00000000);
 if t { l = 0x004ade50; continue; }
n[5] += 1;
uycpu::vu0::fmac::<1, 14>(c, 2, 1, c.vf[28]);
r[S6] = add32(r[S6], 0x00000040);
uycpu::vu0::fmac::<0, 8>(c, 3, 0, uycpu::vu0::bcast(c.vf[1][3]));
r[S3] = add32(r[S3], 0x00000040);
uycpu::vu0::fmac::<1, 4>(c, 3, 0, uycpu::vu0::bcast(c.vf[1][3]));
r[T2] = c.rd16((r[S4] as u32).wrapping_add(0x0000003e)) as u64;
r[T3] = c.rd32((r[S4] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
uycpu::vu0::fmac::<6, 14>(c, 32, 29, uycpu::vu0::bcast(c.vf[2][0]));
r[A2] = c.rd8((r[S4] as u32).wrapping_add(0x0000003b)) as u64;
uycpu::vu0::fmac::<2, 14>(c, 32, 30, uycpu::vu0::bcast(c.vf[2][1]));
r[A3] = c.rd8((r[S4] as u32).wrapping_add(0x0000003a)) as u64;
uycpu::vu0::fmac::<2, 14>(c, 2, 31, uycpu::vu0::bcast(c.vf[2][2]));
r[A2] = add32(r[A2], r[RA]);
r[A2] = c.rd8((r[A2] as u32).wrapping_add(0x00003b00)) as u64;
uycpu::vu0::fmac::<6, 12>(c, 8, 26, uycpu::vu0::bcast(c.vf[1][3]));
uycpu::vu0::fmac::<0, 12>(c, 1, 3, uycpu::vu0::bcast(c.vf[2][2]));
r[A3] = r[A3] & r[A2];
let t = r[A3] == 0u64;  uycpu::vu0::fmac::<6, 12>(c, 5, 24, uycpu::vu0::bcast(c.vf[2][2]));
 if t { l = 0x004adec0; continue; }
uycpu::vu0::abs::<12>(c, 2, 2);
uycpu::vu0::fmac::<1, 12>(c, 3, 23, c.vf[1]);
uycpu::vu0::fmac::<1, 12>(c, 6, 2, c.vf[8]);
uycpu::vu0::fmac::<0, 12>(c, 8, 2, c.vf[8]);
uycpu::vu0::fmac::<1, 12>(c, 4, 1, c.vf[27]);
{ let (a, b) = { let v = c.vf[3]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[AT] = a; h[AT] = b; }
r[A2] = 0x0000000040000000;
uycpu::vu0::fmac::<1, 12>(c, 7, 5, c.vf[6]);
r[A2] = r[A2].wrapping_add(r[A2]);
uycpu::vu0::fmac::<1, 12>(c, 8, 5, c.vf[8]);
r[V0] = r[AT] << 32;
let t = (r[AT] as i64) < 0;  uycpu::vu0::ftoi::<0, 12>(c, 1, 1);
 if t { l = 0x004adec0; continue; }
let t = (r[V0] as i64) >= 0;  { let (a, b) = { let v = c.vf[4]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[A0] = a; h[A0] = b; }
 if t { l = 0x004adec0; continue; }
{ let (a, b) = { let v = c.vf[7]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[V0] = a; h[V0] = b; }
{ let (a, b) = { let v = c.vf[8]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[V1] = a; h[V1] = b; }
r[A0] = r[A0] ^ r[A2];
let t = (r[V0] as i64) < 0;  r[V0] = r[V0] << 32;
 if t { l = 0x004adec0; continue; }
let t = (r[V0] as i64) < 0;  r[V1] = r[V1] | r[A0];
 if t { l = 0x004adec0; continue; }
let t = (r[V1] as i64) < 0;  r[V1] = r[V1] << 32;
 if t { l = 0x004ae2a0; continue; }
let t = (r[V1] as i64) < 0;  { let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[AT] = a; h[AT] = b; }
 if t { l = 0x004ae2a0; continue; }
l = 0x004adfb8; continue; }
0x004adfb8 => {
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000035)) as u64;
r[A0] = c.rd16((r[S4] as u32).wrapping_add(0x00000036)) as u64;
r[A1] = add32(r[RA], 0x000033fe);
let t = r[V1] != 0u64;  r[V1] = 0u64 | 0xffff;
 if t { l = 0x004adfd8; continue; }
let t = r[V1] == r[A0];   if t { l = 0x004adff0; continue; }
l = 0x004adfd8; continue; }
0x004adfd8 => {
let t = r[S0] == r[A1];  r[V1] = c.rd32((r[RA] as u32).wrapping_add(0x00003f10)) as i32 as i64 as u64;
 if t { l = 0x004adff0; continue; }
r[V1] = sub32(r[S3], r[V1]);
r[V1] = srl(r[V1], 6);
c.wr16((r[S0] as u32), r[V1] as u16);
r[S0] = add32(r[S0], 0x00000002);
l = 0x004adff0; continue; }
0x004adff0 => {
n[6] += 1;
r[V1] = c.rd32((r[RA] as u32).wrapping_add(0x00003f00)) as i32 as i64 as u64;
r[A0] = c.rd32((r[RA] as u32).wrapping_add(0x00003f04)) as i32 as i64 as u64;
r[V0] = ((r[AT] as i64) >> 32) as u64;
r[A1] = c.rd32((r[RA] as u32).wrapping_add(0x00003f08)) as i32 as i64 as u64;
r[AT] = (((r[AT] as u32).wrapping_add(0u32)) as i32 as i64 as u64);
r[A2] = sub32(r[T2], r[V0]);
r[T1] = c.rd8((r[S4] as u32).wrapping_add(0x00000027)) as u64;
let t = (r[A2] as i64) < 0;  r[A2] = sub32(r[V1], r[V0]);
 if t { l = 0x004ae058; continue; }
r[A3] = c.rd16((r[S4] as u32).wrapping_add(0x0000002e)) as u64;
r[T0] = add32(r[RA], 0x00003778);
let t = r[T0] == r[S2];  r[T0] = c.rd16((r[S4] as u32).wrapping_add(0x0000002c)) as u64;
 if t { l = 0x004ae058; continue; }
r[A3] = add32(r[A3], r[T3]);
c.wr32((r[S2] as u32), r[A3] as u32);
c.wr32((r[S2] as u32).wrapping_add(0x00000004), r[T0] as u32);
r[A3] = c.rd16((r[S4] as u32).wrapping_add(0x0000001c)) as u64;
r[T0] = c.rd8((r[S4] as u32).wrapping_add(0x00000028)) as u64;
r[A3] = add32(r[A3], r[T3]);
c.wr32((r[S2] as u32).wrapping_add(0x00000380), r[A3] as u32);
c.wr32((r[S2] as u32).wrapping_add(0x00000384), r[T0] as u32);
r[S2] = add32(r[S2], 0x00000008);
l = 0x004ae058; continue; }
0x004ae058 => {
let t = r[T1] != 0u64;   if t { l = 0x004ae1d8; continue; }
let t = (r[A2] as i64) <= 0;  r[A2] = sub32(r[V1], r[AT]);
 if t { l = 0x004ae1d8; continue; }
let t = (r[A2] as i64) <= 0;  r[A2] = sub32(r[A0], r[V0]);
 if t { l = 0x004ae150; continue; }
let t = (r[A2] as i64) <= 0;  r[A2] = sub32(r[A0], r[AT]);
 if t { l = 0x004ae148; continue; }
let t = (r[A2] as i64) <= 0;  r[A2] = sub32(r[A1], r[V0]);
 if t { l = 0x004ae0a0; continue; }
let t = (r[A2] as i64) <= 0;  r[A2] = sub32(r[A1], r[AT]);
 if t { l = 0x004ae098; continue; }
let t = (r[A2] as i64) <= 0;   if t { l = 0x004ae098; continue; }
r[T2] = sx(0x00000014);
 l = 0x004ae0a8; continue;
}
0x004ae098 => {
r[T2] = sx(0x00000010);
 l = 0x004ae0a8; continue;
}
0x004ae0a0 => {
r[T2] = sx(0x0000000e);
l = 0x004ae0a8; continue; }
0x004ae0a8 => {
r[V0] = c.rd16((r[S4] as u32).wrapping_add(0x00000016)) as u64;
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000020)) as u64;
r[A0] = c.rd16((r[S4] as u32).wrapping_add(0x0000001e)) as u64;
r[A1] = c.rd8((r[S4] as u32).wrapping_add(0x00000026)) as u64;
r[A2] = c.rd8((r[S4] as u32).wrapping_add(0x0000002a)) as u64;
c.wr128((r[SP] as u32), r[T4], h[T4]);
c.wr8((r[SP] as u32), r[V1] as u8);
r[V0] = add32(r[V0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[V0] as u32);
r[V0] = c.rd16((r[S4] as u32).wrapping_add(0x0000001a)) as u64;
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000023)) as u64;
c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T4], h[T4]);
c.wr8((r[SP] as u32).wrapping_add(0x00000010), r[V1] as u8);
r[V0] = add32(r[V0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000014), r[V0] as u32);
c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[T5], h[T5]);
r[A1] = srl(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x00000020), r[A1] as u8);
r[A0] = add32(r[A0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000024), r[A0] as u32);
r[A1] = sll(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x0000002e), r[A1] as u8);
c.wr8((r[SP] as u32).wrapping_add(0x0000002c), r[A2] as u8);
c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[T7], h[T7]);
r[FP] = add32(r[FP], 0x00000040);
c.wr8((r[SP] as u32).wrapping_add(0x0000003c), r[T2] as u8);
r[SP] = add32(r[SP], 0x00000040);
r[AT] = r[SP] & 0x700;
r[V0] = sx(0x00000700);
let t = r[AT] != r[V0];   if t { l = 0x004adec0; continue; }
 l = 0x004ae250; continue;
}
0x004ae148 => {
r[T2] = sx(0x0000000a);
 l = 0x004ae158; continue;
}
0x004ae150 => {
r[T2] = sx(0x00000008);
l = 0x004ae158; continue; }
0x004ae158 => {
r[V0] = c.rd16((r[S4] as u32).wrapping_add(0x00000016)) as u64;
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000022)) as u64;
r[A0] = c.rd16((r[S4] as u32).wrapping_add(0x0000001e)) as u64;
r[A1] = c.rd8((r[S4] as u32).wrapping_add(0x00000025)) as u64;
r[A2] = c.rd8((r[S4] as u32).wrapping_add(0x0000002a)) as u64;
c.wr128((r[SP] as u32), r[T4], h[T4]);
c.wr8((r[SP] as u32), r[V1] as u8);
r[V0] = add32(r[V0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[V0] as u32);
c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T6], h[T6]);
r[A1] = srl(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x00000010), r[A1] as u8);
r[A0] = add32(r[A0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000014), r[A0] as u32);
r[A1] = sll(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x0000001e), r[A1] as u8);
c.wr8((r[SP] as u32).wrapping_add(0x0000001c), r[A2] as u8);
c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[T7], h[T7]);
r[FP] = add32(r[FP], 0x00000030);
c.wr8((r[SP] as u32).wrapping_add(0x0000002c), r[T2] as u8);
r[SP] = add32(r[SP], 0x00000030);
r[AT] = r[SP] & 0x700;
r[V0] = sx(0x00000700);
let t = r[AT] != r[V0];   if t { l = 0x004adec0; continue; }
 l = 0x004ae250; continue;
}
0x004ae1d8 => {
r[V0] = c.rd16((r[S4] as u32).wrapping_add(0x00000014)) as u64;
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000021)) as u64;
r[A0] = c.rd16((r[S4] as u32).wrapping_add(0x0000001e)) as u64;
r[A1] = c.rd8((r[S4] as u32).wrapping_add(0x00000024)) as u64;
r[A2] = c.rd8((r[S4] as u32).wrapping_add(0x0000002a)) as u64;
c.wr128((r[SP] as u32), r[T4], h[T4]);
r[T2] = sx(0x00000006);
c.wr8((r[SP] as u32), r[V1] as u8);
r[V0] = add32(r[V0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[V0] as u32);
c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T5], h[T5]);
r[A1] = srl(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x00000010), r[A1] as u8);
r[A0] = add32(r[A0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000014), r[A0] as u32);
r[A1] = sll(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x0000001e), r[A1] as u8);
c.wr8((r[SP] as u32).wrapping_add(0x0000001c), r[A2] as u8);
c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[T7], h[T7]);
r[FP] = add32(r[FP], 0x00000030);
c.wr8((r[SP] as u32).wrapping_add(0x0000002c), r[T2] as u8);
r[SP] = add32(r[SP], 0x00000030);
r[AT] = r[SP] & 0x700;
r[V0] = sx(0x00000700);
let t = r[AT] != r[V0];   if t { l = 0x004adec0; continue; }
l = 0x004ae250; continue; }
0x004ae250 => {
r[AT] = r[SP] & 0x7ff;
r[A0] = 0x0000000010000000;
r[A0] = r[A0] | 0xd000;
r[V0] = sub32(r[SP], r[AT]);
l = 0x004ae260; continue; }
0x004ae260 => {
r[V1] = c.rd32((r[A0] as u32)) as i32 as i64 as u64;
r[V1] = r[V1] & 0x100;
if r[V1] != 0u64 { busy(k, 0x004ae260) }
c.wr32((r[A0] as u32).wrapping_add(0x00000080), r[V0] as u32);
r[V0] = sub32(r[FP], r[AT]);
c.wr32((r[A0] as u32).wrapping_add(0x00000010), r[V0] as u32);
r[V0] = srl(r[AT], 4);
c.wr32((r[A0] as u32).wrapping_add(0x00000020), r[V0] as u32);
r[V0] = 0u64 | 0x100;
c.wr32((r[A0] as u32), r[V0] as u32);
r[SP] = sub32(r[SP], r[AT]);
r[SP] = r[SP] ^ 0x800;
 l = 0x004adec0; continue;
}
0x004ae2a0 => {
r[AT] = c.rd16((r[S4] as u32).wrapping_add(0x00000038)) as u64;
r[V0] = add32(r[RA], 0x00003be0);
r[AT] = add32(r[AT], r[T3]);
r[V1] = sx(0x00000008);
r[A0] = c.rd64((r[AT] as u32));
r[A1] = c.rd64((r[AT] as u32).wrapping_add(0x00000008));
{ let (a, b) = uycpu::mmi::pextlh((r[A0], h[A0]), (0u64, 0u64)); r[A0] = a; h[A0] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[A1], h[A1]), (0u64, 0u64)); r[A1] = a; h[A1] = b; }
l = 0x004ae2c0; continue; }
0x004ae2c0 => {
{ let (a, b) = uycpu::mmi::psraw((r[A0], h[A0]), 10); r[A0] = a; h[A0] = b; }
{ let (a, b) = uycpu::mmi::psraw((r[A1], h[A1]), 10); r[A1] = a; h[A1] = b; }
c.vf[4] = [r[A0] as u32, (r[A0] >> 32) as u32, h[A0] as u32, (h[A0] >> 32) as u32];
r[AT] = add32(r[AT], 0x00000010);
c.vf[5] = [r[A1] as u32, (r[A1] >> 32) as u32, h[A1] as u32, (h[A1] >> 32) as u32];
r[V0] = add32(r[V0], 0x00000020);
uycpu::vu0::itof::<0, 14>(c, 4, 4);
r[A0] = c.rd64((r[AT] as u32));
uycpu::vu0::itof::<0, 14>(c, 5, 5);
r[A1] = c.rd64((r[AT] as u32).wrapping_add(0x00000008));
{ let (a, b) = uycpu::mmi::pextlh((r[A0], h[A0]), (0u64, 0u64)); r[A0] = a; h[A0] = b; }
{ let (a, b) = uycpu::mmi::pextlh((r[A1], h[A1]), (0u64, 0u64)); r[A1] = a; h[A1] = b; }
{ let v = c.vf[4]; c.wr128((r[V0] as u32), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
r[V1] = add32(r[V1], 0xfffffffe);
let t = (r[V1] as i64) > 0;  { let v = c.vf[5]; c.wr128((r[V0] as u32).wrapping_add(0x00000010), v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32); }
 if t { l = 0x004ae2c0; continue; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c00)); c.vf[5] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c10)); c.vf[6] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c20)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c30)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[5][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[5][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[5][2]));
uycpu::vu0::fmac::<2, 15>(c, 5, 21, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[6][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[6][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[6][2]));
uycpu::vu0::fmac::<2, 15>(c, 6, 21, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[7][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::fmac::<2, 15>(c, 7, 21, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 6, 6);
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[8][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[8][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::fmac::<2, 15>(c, 8, 21, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 7, 7);
uycpu::vu0::fmac::<6, 12>(c, 5, 5, c.vf[22]);
uycpu::vu0::fmac::<6, 12>(c, 6, 6, c.vf[22]);
uycpu::vu0::clip(c, 8, 8);
uycpu::vu0::fmac::<6, 12>(c, 7, 7, c.vf[22]);
uycpu::vu0::fmac::<6, 12>(c, 8, 8, c.vf[22]);
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c40)); c.vf[10] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c50)); c.vf[11] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
r[AT] = c.cfc2(18) as i32 as i64 as u64;
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::clip(c, 6, 6);
uycpu::vu0::clip(c, 7, 7);
uycpu::vu0::clip(c, 8, 8);
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[10][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[10][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[10][2]));
uycpu::vu0::fmac::<2, 15>(c, 5, 21, uycpu::vu0::bcast(c.vf[0][3]));
r[V1] = c.cfc2(18) as i32 as i64 as u64;
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c60)); c.vf[7] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
{ let (a, b) = c.rd128((r[RA] as u32).wrapping_add(0x00003c70)); c.vf[8] = [a as u32, (a >> 32) as u32, b as u32, (b >> 32) as u32]; }
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[11][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[11][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[11][2]));
uycpu::vu0::fmac::<2, 15>(c, 6, 21, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[7][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[7][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[7][2]));
uycpu::vu0::fmac::<2, 15>(c, 7, 21, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 6, 6);
uycpu::vu0::fmac::<6, 15>(c, 32, 18, uycpu::vu0::bcast(c.vf[8][0]));
uycpu::vu0::fmac::<2, 15>(c, 32, 19, uycpu::vu0::bcast(c.vf[8][1]));
uycpu::vu0::fmac::<2, 15>(c, 32, 20, uycpu::vu0::bcast(c.vf[8][2]));
uycpu::vu0::fmac::<2, 15>(c, 8, 21, uycpu::vu0::bcast(c.vf[0][3]));
uycpu::vu0::clip(c, 7, 7);
uycpu::vu0::fmac::<6, 12>(c, 5, 5, c.vf[22]);
uycpu::vu0::fmac::<6, 12>(c, 6, 6, c.vf[22]);
uycpu::vu0::clip(c, 8, 8);
uycpu::vu0::fmac::<6, 12>(c, 7, 7, c.vf[22]);
r[A1] = srl(r[AT], 6);
uycpu::vu0::fmac::<6, 12>(c, 8, 8, c.vf[22]);
r[A2] = srl(r[AT], 12);
r[A3] = srl(r[AT], 18);
r[AT] = r[AT] & r[A1];
r[A2] = r[A2] & r[A3];
r[V0] = c.cfc2(18) as i32 as i64 as u64;
r[AT] = r[AT] & r[A2];
uycpu::vu0::clip(c, 5, 5);
uycpu::vu0::clip(c, 6, 6);
r[A1] = srl(r[V0], 6);
uycpu::vu0::clip(c, 7, 7);
r[A2] = srl(r[V0], 12);
uycpu::vu0::clip(c, 8, 8);
r[A3] = srl(r[V0], 18);
l = 0x004ae440; continue; }
0x004ae440 => {
r[V0] = r[V0] & r[A1];
r[A2] = r[A2] & r[A3];
r[V0] = r[V0] & r[A2];
r[AT] = r[AT] & r[V0];
r[AT] = r[AT] & 0x3f;
let t = r[AT] != 0u64;  r[A0] = c.cfc2(18) as i32 as i64 as u64;
 if t { l = 0x004adec0; continue; }
r[V1] = r[V1] | r[A0];
let t = r[V1] == 0u64;  { let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[AT] = a; h[AT] = b; }
 if t { l = 0x004adfb8; continue; }
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000035)) as u64;
r[A0] = c.rd16((r[S4] as u32).wrapping_add(0x00000036)) as u64;
r[A1] = add32(r[RA], 0x000033fe);
let t = r[V1] != 0u64;  r[V1] = 0u64 | 0xffff;
 if t { l = 0x004ae498; continue; }
let t = r[V1] == r[A0];   if t { l = 0x004ae4b0; continue; }
l = 0x004ae498; continue; }
0x004ae498 => {
let t = r[S0] == r[A1];  r[V1] = c.rd32((r[RA] as u32).wrapping_add(0x00003f10)) as i32 as i64 as u64;
 if t { l = 0x004ae4b0; continue; }
r[V1] = sub32(r[S3], r[V1]);
r[V1] = srl(r[V1], 6);
c.wr16((r[S0] as u32), r[V1] as u16);
r[S0] = add32(r[S0], 0x00000002);
l = 0x004ae4b0; continue; }
0x004ae4b0 => {
n[7] += 1;
{ let (a, b) = { let v = c.vf[1]; (v[0] as u64 | (v[1] as u64) << 32, v[2] as u64 | (v[3] as u64) << 32) }; r[AT] = a; h[AT] = b; }
r[V0] = add32(r[RA], 0x00003efc);
let t = r[V0] == r[S1];  c.wr32((r[S1] as u32), r[S3] as u32);
 if t { l = 0x004adec0; continue; }
r[S1] = add32(r[S1], 0x00000004);
r[V0] = ((r[AT] as i64) >> 32) as u64;
r[A2] = sub32(r[T2], r[V0]);
let t = (r[A2] as i64) < 0;   if t { l = 0x004ae518; continue; }
r[A3] = c.rd16((r[S4] as u32).wrapping_add(0x0000002e)) as u64;
r[T0] = add32(r[RA], 0x00003778);
let t = r[T0] == r[S2];  r[T0] = c.rd16((r[S4] as u32).wrapping_add(0x0000002c)) as u64;
 if t { l = 0x004ae518; continue; }
r[A3] = add32(r[A3], r[T3]);
c.wr32((r[S2] as u32), r[A3] as u32);
c.wr32((r[S2] as u32).wrapping_add(0x00000004), r[T0] as u32);
r[A3] = c.rd16((r[S4] as u32).wrapping_add(0x0000001c)) as u64;
r[T0] = c.rd8((r[S4] as u32).wrapping_add(0x00000028)) as u64;
r[A3] = add32(r[A3], r[T3]);
c.wr32((r[S2] as u32).wrapping_add(0x00000380), r[A3] as u32);
c.wr32((r[S2] as u32).wrapping_add(0x00000384), r[T0] as u32);
r[S2] = add32(r[S2], 0x00000008);
l = 0x004ae518; continue; }
0x004ae518 => {
 l = 0x004adec0; continue;
}
0x004ae520 => {
r[AT] = sx(k.hi(c, 0x004ae520));
r[AT] = add32(r[AT], k.lo(c, 0x004ae524) as u64);
{ let (a, b) = c.rd128((r[AT] as u32)); r[V0] = a; h[V0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); r[V1] = a; h[V1] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000020)); r[A0] = a; h[A0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000030)); r[A1] = a; h[A1] = b; }
c.wr128((r[SP] as u32), r[V0], h[V0]);
c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
r[SP] = add32(r[SP], 0x00000040);
r[FP] = add32(r[FP], 0x00000040);
r[AT] = r[SP] & 0x7ff;
r[A0] = 0x0000000010000000;
r[A0] = r[A0] | 0xd000;
r[V0] = sub32(r[SP], r[AT]);
l = 0x004ae560; continue; }
0x004ae560 => {
r[V1] = c.rd32((r[A0] as u32)) as i32 as i64 as u64;
r[V1] = r[V1] & 0x100;
if r[V1] != 0u64 { busy(k, 0x004ae560) }
c.wr32((r[A0] as u32).wrapping_add(0x00000080), r[V0] as u32);
r[V0] = sub32(r[FP], r[AT]);
c.wr32((r[A0] as u32).wrapping_add(0x00000010), r[V0] as u32);
r[V0] = srl(r[AT], 4);
c.wr32((r[A0] as u32).wrapping_add(0x00000020), r[V0] as u32);
r[V0] = 0u64 | 0x100;
c.wr32((r[A0] as u32), r[V0] as u32);
r[SP] = sub32(r[SP], r[AT]);
r[SP] = r[SP] ^ 0x800;
c.wr32((r[S2] as u32), 0u64 as u32);
c.wr32((r[S2] as u32).wrapping_add(0x00000380), 0u64 as u32);
r[AT] = sx(0xffffffff);
c.wr16((r[S0] as u32), r[AT] as u16);
r[A0] = 0x0000000010000000;
r[A0] = r[A0] | 0xd000;
l = 0x004ae5b8; continue; }
0x004ae5b8 => {
r[V1] = c.rd32((r[A0] as u32)) as i32 as i64 as u64;
r[V1] = r[V1] & 0x100;
let t = r[V1] == 0u64;  r[V1] = sx(0x00000010);
 if t { l = 0x004ae5e8; continue; }
l = 0x004ae5c8; continue; }
0x004ae5c8 => {
r[V1] = add32(r[V1], 0xffffffff);
let t = r[V1] != 0u64;   if t { l = 0x004ae5c8; continue; }
 busy(k, 0x004ae5b8)
}
0x004ae5e8 => {
r[V0] = sx(k.hi(c, 0x004ae5e8));
r[V0] = add32(r[V0], k.lo(c, 0x004ae5ec) as u64);
c.wr32((r[A0] as u32).wrapping_add(0x00000010), r[V0] as u32);
r[V0] = sx(0x00003780);
c.wr32((r[A0] as u32).wrapping_add(0x00000080), r[V0] as u32);
r[V0] = sx(0x00000038);
c.wr32((r[A0] as u32).wrapping_add(0x00000020), r[V0] as u32);
r[V0] = 0u64 | 0x100;
c.wr32((r[A0] as u32), r[V0] as u32);
l = 0x004ae610; continue; }
0x004ae610 => {
r[V1] = c.rd32((r[A0] as u32)) as i32 as i64 as u64;
r[V1] = r[V1] & 0x100;
let t = r[V1] == 0u64;  r[V1] = sx(0x00000010);
 if t { l = 0x004ae640; continue; }
l = 0x004ae620; continue; }
0x004ae620 => {
r[V1] = add32(r[V1], 0xffffffff);
let t = r[V1] != 0u64;   if t { l = 0x004ae620; continue; }
 busy(k, 0x004ae610)
}
0x004ae640 => {
r[S0] = add32(r[RA], 0x00003d00);
let t = r[S0] == r[S1];   if t { l = 0x004ae860; continue; }
c.wr128((r[SP] as u32), 0u64, 0u64);
r[V0] = sx(k.hi(c, 0x004ae654));
r[V0] = add32(r[V0], k.lo(c, 0x004ae658) as u64);
r[AT] = sx(k.hi(c, 0x004ae65c));
r[AT] = add32(r[AT], k.lo(c, 0x004ae660) as u64);
r[V0] = c.rd32((r[V0] as u32)) as i32 as i64 as u64;
c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[AT] as u32);
r[AT] = 0x0000000030000000;
c.wr32((r[SP] as u32), r[AT] as u32);
r[FP] = add32(r[FP], 0x00000010);
c.wr16((r[SP] as u32), r[V0] as u16);
r[SP] = add32(r[SP], 0x00000010);
r[AT] = sx(k.hi(c, 0x004ae680));
r[AT] = add32(r[AT], k.lo(c, 0x004ae684) as u64);
r[V0] = sx(0x00000011);
l = 0x004ae690; continue; }
0x004ae690 => {
{ let (a, b) = c.rd128((r[AT] as u32)); r[V1] = a; h[V1] = b; }
r[AT] = add32(r[AT], 0x00000010);
r[V0] = add32(r[V0], 0xffffffff);
r[FP] = add32(r[FP], 0x00000010);
c.wr128((r[SP] as u32), r[V1], h[V1]);
r[SP] = add32(r[SP], 0x00000010);
let t = (r[V0] as i64) > 0;   if t { l = 0x004ae690; continue; }
l = 0x004ae6b0; continue; }
0x004ae6b0 => {
let t = r[S0] == r[S1];  r[S4] = c.rd32((r[S0] as u32)) as i32 as i64 as u64;
 if t { l = 0x004ae7a8; continue; }
r[S0] = add32(r[S0], 0x00000004);
r[T2] = sx(0x00000002);
r[T3] = c.rd32((r[S4] as u32).wrapping_add(0x00000010)) as i32 as i64 as u64;
r[V0] = c.rd16((r[S4] as u32).wrapping_add(0x00000016)) as u64;
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000020)) as u64;
r[A0] = c.rd16((r[S4] as u32).wrapping_add(0x0000001e)) as u64;
r[A1] = c.rd8((r[S4] as u32).wrapping_add(0x00000026)) as u64;
r[A2] = c.rd8((r[S4] as u32).wrapping_add(0x0000002a)) as u64;
c.wr128((r[SP] as u32), r[T4], h[T4]);
c.wr8((r[SP] as u32), r[V1] as u8);
r[V0] = add32(r[V0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000004), r[V0] as u32);
r[V0] = c.rd16((r[S4] as u32).wrapping_add(0x0000001a)) as u64;
r[V1] = c.rd8((r[S4] as u32).wrapping_add(0x00000023)) as u64;
c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[T4], h[T4]);
c.wr8((r[SP] as u32).wrapping_add(0x00000010), r[V1] as u8);
r[V0] = add32(r[V0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000014), r[V0] as u32);
c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[T5], h[T5]);
r[A1] = srl(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x00000020), r[A1] as u8);
r[A0] = add32(r[A0], r[T3]);
c.wr32((r[SP] as u32).wrapping_add(0x00000024), r[A0] as u32);
r[A1] = sll(r[A1], 2);
c.wr8((r[SP] as u32).wrapping_add(0x0000002e), r[A1] as u8);
c.wr8((r[SP] as u32).wrapping_add(0x0000002c), r[A2] as u8);
c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[T7], h[T7]);
r[FP] = add32(r[FP], 0x00000040);
c.wr8((r[SP] as u32).wrapping_add(0x0000003c), r[T2] as u8);
r[SP] = add32(r[SP], 0x00000040);
r[AT] = r[SP] & 0x700;
r[V0] = sx(0x00000700);
let t = r[AT] != r[V0];   if t { l = 0x004ae6b0; continue; }
r[AT] = r[SP] & 0x7ff;
r[A0] = 0x0000000010000000;
r[A0] = r[A0] | 0xd000;
r[V0] = sub32(r[SP], r[AT]);
l = 0x004ae768; continue; }
0x004ae768 => {
r[V1] = c.rd32((r[A0] as u32)) as i32 as i64 as u64;
r[V1] = r[V1] & 0x100;
if r[V1] != 0u64 { busy(k, 0x004ae768) }
c.wr32((r[A0] as u32).wrapping_add(0x00000080), r[V0] as u32);
r[V0] = sub32(r[FP], r[AT]);
c.wr32((r[A0] as u32).wrapping_add(0x00000010), r[V0] as u32);
r[V0] = srl(r[AT], 4);
c.wr32((r[A0] as u32).wrapping_add(0x00000020), r[V0] as u32);
r[V0] = 0u64 | 0x100;
c.wr32((r[A0] as u32), r[V0] as u32);
r[SP] = sub32(r[SP], r[AT]);
r[SP] = r[SP] ^ 0x800;
 l = 0x004ae6b0; continue;
}
0x004ae7a8 => {
r[AT] = sx(k.hi(c, 0x004ae7a8));
r[AT] = add32(r[AT], k.lo(c, 0x004ae7ac) as u64);
{ let (a, b) = c.rd128((r[AT] as u32)); r[V0] = a; h[V0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000010)); r[V1] = a; h[V1] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000020)); r[A0] = a; h[A0] = b; }
{ let (a, b) = c.rd128((r[AT] as u32).wrapping_add(0x00000030)); r[A1] = a; h[A1] = b; }
c.wr128((r[SP] as u32), r[V0], h[V0]);
c.wr128((r[SP] as u32).wrapping_add(0x00000010), r[V1], h[V1]);
c.wr128((r[SP] as u32).wrapping_add(0x00000020), r[A0], h[A0]);
c.wr128((r[SP] as u32).wrapping_add(0x00000030), r[A1], h[A1]);
r[SP] = add32(r[SP], 0x00000040);
r[FP] = add32(r[FP], 0x00000040);
r[AT] = r[SP] & 0x7ff;
r[A0] = 0x0000000010000000;
r[A0] = r[A0] | 0xd000;
r[V0] = sub32(r[SP], r[AT]);
l = 0x004ae7e8; continue; }
0x004ae7e8 => {
r[V1] = c.rd32((r[A0] as u32)) as i32 as i64 as u64;
r[V1] = r[V1] & 0x100;
if r[V1] != 0u64 { busy(k, 0x004ae7e8) }
c.wr32((r[A0] as u32).wrapping_add(0x00000080), r[V0] as u32);
r[V0] = sub32(r[FP], r[AT]);
c.wr32((r[A0] as u32).wrapping_add(0x00000010), r[V0] as u32);
r[V0] = srl(r[AT], 4);
c.wr32((r[A0] as u32).wrapping_add(0x00000020), r[V0] as u32);
r[V0] = 0u64 | 0x100;
c.wr32((r[A0] as u32), r[V0] as u32);
r[SP] = sub32(r[SP], r[AT]);
r[SP] = r[SP] ^ 0x800;
r[A0] = 0x0000000010000000;
r[A0] = r[A0] | 0xd000;
l = 0x004ae830; continue; }
0x004ae830 => {
r[V1] = c.rd32((r[A0] as u32)) as i32 as i64 as u64;
r[V1] = r[V1] & 0x100;
let t = r[V1] == 0u64;  r[V1] = sx(0x00000010);
 if t { l = 0x004ae860; continue; }
l = 0x004ae840; continue; }
0x004ae840 => {
r[V1] = add32(r[V1], 0xffffffff);
let t = r[V1] != 0u64;   if t { l = 0x004ae840; continue; }
 busy(k, 0x004ae830)
}
0x004ae860 => {
c.wr32(k.gp(c, gp, 0x004ae860), r[FP] as u32);
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
_ => unreachable!("0x4ADC98: no block at {l:08x}"),
}
}
c.r = r;
c.rh = h;
}
