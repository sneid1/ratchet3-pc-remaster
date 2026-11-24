//! The game's VU microprograms, translated ahead of time into Rust: VU0's for `uycpu::vu0` (the runtime starts them from the
//! EE's VCALLMS), VU1's for the renderer's own VU1 state (`vu1mp.rs`: the programs the VIF1 chain uploads).
//!
//! The four VU0 programs the game uploads (found in its VIF0 MPG transfers; all in the boot program's data, the same in every
//! level): the moby skinning program (0x100090 and 0x100898, micro 0x000-0xA3F), the program of tag 0x100AE0 (0x100AF0, micro
//! 0x000-0x647), the tie second-part program (0x1159C0, micro 0x000-0x69F) and the math routines at micro 0xC80 (0x10F1B0).
//!
//! What a VU program computes depends on its pipeline's timing in three places: Q (a DIV, SQRT or RSQRT result lands 7 or 13
//! cycles after it issues, and an upper instruction reading Q before then reads the old one), the MAC flags and the status
//! flag's zero and sign bits (FMAND and its kin, FSAND, read the flags of the result that landed last, four cycles after its
//! instruction) and the clip flag (CLIP's judgement is shifted into it four cycles after the CLIP: FCAND, FCOR and FCGET read
//! the judgements that have landed). All three are fixed for each path through the program, so the translator follows the
//! timing itself: each node is a pair at one pipeline state (the VF results still in flight, the Q in flight, the MAC results and
//! CLIP judgements in flight, a pending branch, the E bit's slot, a known BAL return), with the stalls of the VU's register
//! interlocks; the native code does the arithmetic and lands Q, the flags and the judgements at the pairs the timing names.
//! Everything else is sequential, as the interlocks make it: upper and lower of a pair read the state before the pair, the upper
//! wins a write both make, LOI's I is seen from the next pair.
//!
//! VU1 differs from VU0 in its memories (16 KB of data, addresses wrapping at 0x400 quadwords; 16 KB of micro memory), in what
//! starts a program (VIF1's MSCAL at an entry, MSCNT where the last run ended: a program's start and the code after each
//! unconditional B's or E bit's delay slot), in the instructions only VU1 has (XTOP, VIF1's TOP; XGKICK, a packet to the GS:
//! its address is kept, `Vu1::kicks`, and the packet as data memory holds it then, `Vu1::sent`, nothing in VU1's state changes;
//! the EFU's ESADD and ESUM into P, 11 and 12 cycles, MFP)
//! and in what its programs rely on:
//! - the branch hazard: a branch or jump reads a VI register a cycle early, so after a pair that changed it (a flag
//!   instruction's result excepted) it reads the value from before that pair (program 4's MTIR then IBLTZ of the same
//!   register; program 0's IADDIU vi01 then JALR through vi01, which on a path that found nothing goes to the old vi01, 0: the
//!   program's start, which ends the run);
//! - the clip flag's writes (CLIP's judgement shifted in, FCSET's value) land four cycles after they issue, in the program's
//!   order (program 1's FCSET three pairs after a CLIP comes after it);
//! - computed jumps: a JR or JALR goes to whatever its register holds, which for a register only BAL, JALR or an IADDIU from
//!   vi00 set is one of their addresses (`IADDIU vi15, vi00, ...` in a B's delay slot is a hand-made return), and for one the
//!   program computes or loads any address code starts at after an unconditional jump or an E bit's slot, or the program's
//!   start (the tables of small routines: program 0 jumps to the table at vi03 + 5, vi03 read from data).
//! No return address is followed through a VU1 program (each would copy the subroutine's nodes per caller), and the MAC results
//! and judgements in flight are slots by their place in the queue, so equal queues are one state; a program's nodes are emitted
//! as functions of 256 nodes each (`vu1rt.rs`, `drive`): LLVM took over ten minutes on one function of thousands.
//!
//! The interlock is per field, as this translator has it for VU0 and PCSX2 models the VU (an instruction waits for the lanes it
//! reads); rac-rs's VU1 waits for the whole register, which on level 8's tour dump makes 16 runs of program 0 write bits 14 and 15 of some XYZF2 words differently
//! (a per-register build of this translator gives rac-rs's words there).

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write;

pub struct Prog {
    pub name: &'static str,
    /// (EE address of the code, micro address, pairs).
    pub parts: &'static [(u32, usize, usize)],
}

pub const PROGS: &[Prog] = &[
    Prog { name: "skin", parts: &[(0x10_0090, 0x000, 256), (0x10_0898, 0x800, 72)] },
    Prog { name: "p100af0", parts: &[(0x10_0af0, 0x000, 201)] },
    Prog { name: "tie", parts: &[(0x11_59c0, 0x000, 212)] },
    Prog { name: "math", parts: &[(0x10_f1b0, 0xc80, 96)] },
];

const NONE: usize = 99;
/// VU1: a chunk function's result that is the run's end (with the E bit's delay slot's address), not a node.
const END: u32 = 0x8000_0000;
/// VU1: the nodes of one function of a program's code (rustc and LLVM take minutes over one function of thousands of arms).
const CHUNK: u32 = 256;
/// A clip flag write in flight that is a CLIP's judgement (`St::cf`).
const JUDGEMENT: u32 = u32::MAX;
const ACC: usize = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum UK {
    Fmac { k: u8, acc: bool, t: TSel },
    Opmula,
    Opmsub,
    Itof(u8),
    Ftoi(u8),
    Abs,
    Clip,
    Nop,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TSel {
    Full,
    Bc(u8),
    Q,
    I,
}

#[derive(Clone, Copy, Debug)]
struct Up {
    kind: UK,
    dest: u32,
    ft: usize,
    fs: usize,
    fd: usize,
}

impl Up {
    /// (register, lane mask) read; register 32 is ACC.
    fn reads(&self) -> Vec<(usize, u32)> {
        match self.kind {
            UK::Fmac { k, t, .. } => {
                let mut v = vec![(self.fs, self.dest)];
                match t {
                    TSel::Full => v.push((self.ft, self.dest)),
                    TSel::Bc(b) => v.push((self.ft, 8 >> b)),
                    _ => {}
                }
                if k == 2 || k == 3 {
                    v.push((ACC, self.dest));
                }
                v
            }
            UK::Opmula => vec![(self.fs, 0xe), (self.ft, 0xe)],
            UK::Opmsub => vec![(self.fs, 0xe), (self.ft, 0xe), (ACC, 0xe)],
            UK::Itof(_) | UK::Ftoi(_) | UK::Abs => vec![(self.fs, self.dest)],
            UK::Clip => vec![(self.fs, 0xe), (self.ft, 1)],
            UK::Nop => vec![],
        }
    }
    /// (register, lanes) written.
    fn write(&self) -> Option<(usize, u32)> {
        match self.kind {
            UK::Fmac { acc, .. } => Some((if acc { ACC } else { self.fd }, self.dest)),
            UK::Opmula => Some((ACC, 0xe)),
            UK::Opmsub => Some((self.fd, 0xe)),
            UK::Itof(_) | UK::Ftoi(_) | UK::Abs => Some((self.ft, self.dest)),
            UK::Clip | UK::Nop => None,
        }
    }
    fn mac(&self) -> bool {
        match self.kind {
            UK::Fmac { k, .. } => k != 4 && k != 5,
            UK::Opmula | UK::Opmsub => true,
            _ => false,
        }
    }
}

fn decode_up(hi: u32) -> Result<Up, String> {
    let op = hi & 0x07ff_ffff;
    let dest = (op >> 21) & 15;
    let (ft, fs, fd) = (((op >> 16) & 31) as usize, ((op >> 11) & 31) as usize, ((op >> 6) & 31) as usize);
    let bc = (op & 3) as u8;
    let f = op & 63;
    const BCK: [u8; 7] = [0, 1, 2, 3, 4, 5, 6];
    let kind = if f < 0x3c {
        match f {
            0x00..=0x1b => UK::Fmac { k: BCK[(f >> 2) as usize], acc: false, t: TSel::Bc(bc) },
            0x1c => UK::Fmac { k: 6, acc: false, t: TSel::Q },
            0x1d => UK::Fmac { k: 4, acc: false, t: TSel::I },
            0x1e => UK::Fmac { k: 6, acc: false, t: TSel::I },
            0x1f => UK::Fmac { k: 5, acc: false, t: TSel::I },
            0x20 => UK::Fmac { k: 0, acc: false, t: TSel::Q },
            0x21 => UK::Fmac { k: 2, acc: false, t: TSel::Q },
            0x22 => UK::Fmac { k: 0, acc: false, t: TSel::I },
            0x23 => UK::Fmac { k: 2, acc: false, t: TSel::I },
            0x24 => UK::Fmac { k: 1, acc: false, t: TSel::Q },
            0x25 => UK::Fmac { k: 3, acc: false, t: TSel::Q },
            0x26 => UK::Fmac { k: 1, acc: false, t: TSel::I },
            0x27 => UK::Fmac { k: 3, acc: false, t: TSel::I },
            0x28 => UK::Fmac { k: 0, acc: false, t: TSel::Full },
            0x29 => UK::Fmac { k: 2, acc: false, t: TSel::Full },
            0x2a => UK::Fmac { k: 6, acc: false, t: TSel::Full },
            0x2b => UK::Fmac { k: 4, acc: false, t: TSel::Full },
            0x2c => UK::Fmac { k: 1, acc: false, t: TSel::Full },
            0x2d => UK::Fmac { k: 3, acc: false, t: TSel::Full },
            0x2e => UK::Opmsub,
            0x2f => UK::Fmac { k: 5, acc: false, t: TSel::Full },
            _ => return Err(format!("upper {hi:08x}")),
        }
    } else {
        let x = (op & 3) | ((op >> 4) & 0x7c);
        match x {
            0x00..=0x0f => UK::Fmac { k: BCK[(x >> 2) as usize], acc: true, t: TSel::Bc(bc) },
            0x10..=0x13 => UK::Itof((x - 0x10) as u8),
            0x14..=0x17 => UK::Ftoi((x - 0x14) as u8),
            0x18..=0x1b => UK::Fmac { k: 6, acc: true, t: TSel::Bc(bc) },
            0x1c => UK::Fmac { k: 6, acc: true, t: TSel::Q },
            0x1d => UK::Abs,
            0x1e => UK::Fmac { k: 6, acc: true, t: TSel::I },
            0x1f => UK::Clip,
            0x20 => UK::Fmac { k: 0, acc: true, t: TSel::Q },
            0x21 => UK::Fmac { k: 2, acc: true, t: TSel::Q },
            0x22 => UK::Fmac { k: 0, acc: true, t: TSel::I },
            0x23 => UK::Fmac { k: 2, acc: true, t: TSel::I },
            0x24 => UK::Fmac { k: 1, acc: true, t: TSel::Q },
            0x25 => UK::Fmac { k: 3, acc: true, t: TSel::Q },
            0x26 => UK::Fmac { k: 1, acc: true, t: TSel::I },
            0x27 => UK::Fmac { k: 3, acc: true, t: TSel::I },
            0x28 => UK::Fmac { k: 0, acc: true, t: TSel::Full },
            0x29 => UK::Fmac { k: 2, acc: true, t: TSel::Full },
            0x2a => UK::Fmac { k: 6, acc: true, t: TSel::Full },
            0x2c => UK::Fmac { k: 1, acc: true, t: TSel::Full },
            0x2d => UK::Fmac { k: 3, acc: true, t: TSel::Full },
            0x2e => UK::Opmula,
            0x2f => UK::Nop,
            _ => return Err(format!("upper {hi:08x}")),
        }
    };
    Ok(Up { kind, dest, ft, fs, fd })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum LK {
    Nop,
    Lq,
    Sq,
    Lqi,
    Sqi,
    Lqd,
    Sqd,
    Ilw,
    Isw,
    Ilwr,
    Iswr,
    Iaddiu,
    Isubiu,
    Iadd,
    Isub,
    Iaddi,
    Iand,
    Ior,
    Move,
    Mr32,
    Mtir,
    Mfir,
    Div,
    Sqrt,
    Rsqrt,
    Waitq,
    Fmeq,
    Fmand,
    Fmor,
    Fsand,
    Fcand,
    Fcor,
    Fceq,
    Fcset,
    Fcget,
    Xtop,
    Xgkick,
    Esadd,
    Esum,
    Mfp,
    B,
    Bal,
    Jr,
    Jalr,
    Ibeq,
    Ibne,
    Ibltz,
    Ibgtz,
    Iblez,
    Ibgez,
}

#[derive(Clone, Copy, Debug)]
struct Lo {
    kind: LK,
    w: u32,
}

impl Lo {
    fn dest(&self) -> u32 {
        (self.w >> 21) & 15
    }
    fn it(&self) -> usize {
        ((self.w >> 16) & 31) as usize
    }
    fn is(&self) -> usize {
        ((self.w >> 11) & 31) as usize
    }
    fn id(&self) -> usize {
        ((self.w >> 6) & 31) as usize
    }
    fn imm11(&self) -> i32 {
        ((self.w & 0x7ff) as i32) << 21 >> 21
    }
    fn imm15(&self) -> u32 {
        ((self.w >> 10) & 0x7800) | (self.w & 0x7ff)
    }
    /// FSAND's 12 bits: bit 21 is the top one.
    fn imm12(&self) -> u32 {
        ((self.w >> 10) & 0x800) | (self.w & 0x7ff)
    }
    fn imm24(&self) -> u32 {
        self.w & 0xff_ffff
    }
    fn fsf(&self) -> usize {
        ((self.w >> 21) & 3) as usize
    }
    fn ftf(&self) -> usize {
        ((self.w >> 23) & 3) as usize
    }
    /// The one lane of an ILW or ILWR (its dest field).
    fn lane(&self) -> usize {
        (0..4).find(|&l| self.dest() & (8 >> l) != 0).unwrap_or(0)
    }
    fn reads(&self) -> Vec<(usize, u32)> {
        match self.kind {
            LK::Sq | LK::Sqi | LK::Sqd => vec![(self.is(), self.dest())],
            LK::Move => vec![(self.is(), self.dest())],
            LK::Mr32 => {
                let d = self.dest();
                let mut m = 0;
                for l in 0..4 {
                    if d & (8 >> l) != 0 {
                        m |= 8 >> ((l + 1) & 3);
                    }
                }
                vec![(self.is(), m)]
            }
            LK::Mtir => vec![(self.is(), 8 >> self.fsf())],
            LK::Div | LK::Rsqrt => vec![(self.is(), 8 >> self.fsf()), (self.it(), 8 >> self.ftf())],
            LK::Sqrt => vec![(self.it(), 8 >> self.ftf())],
            LK::Esum => vec![(self.is(), 15)],
            LK::Esadd => vec![(self.is(), 14)],
            _ => vec![],
        }
    }
    fn wvf(&self) -> Option<(usize, u32)> {
        match self.kind {
            LK::Lq | LK::Lqi | LK::Lqd | LK::Move | LK::Mr32 | LK::Mfir | LK::Mfp => Some((self.it(), self.dest())),
            _ => None,
        }
    }
    /// The VI register it writes (a return address in it is lost).
    fn wvi(&self) -> Option<usize> {
        match self.kind {
            LK::Iaddiu | LK::Isubiu | LK::Iaddi | LK::Mtir | LK::Fmeq | LK::Fmand | LK::Fmor | LK::Fsand | LK::Fcget | LK::Xtop | LK::Ilw | LK::Ilwr => Some(self.it()),
            LK::Iadd | LK::Isub | LK::Iand | LK::Ior => Some(self.id()),
            LK::Fcand | LK::Fcor | LK::Fceq => Some(1),
            LK::Lqi | LK::Lqd => Some(self.is()),
            LK::Sqi | LK::Sqd => Some(self.it()),
            _ => None,
        }
    }
    fn is_branch(&self) -> bool {
        matches!(self.kind, LK::B | LK::Bal | LK::Jr | LK::Jalr | LK::Ibeq | LK::Ibne | LK::Ibltz | LK::Ibgtz | LK::Iblez | LK::Ibgez)
    }
    /// A jump through a register.
    fn is_jump(&self) -> bool {
        matches!(self.kind, LK::Jr | LK::Jalr)
    }
    fn target(&self, pc: usize, pmask: usize) -> usize {
        ((pc as i64 + 8 + self.imm11() as i64 * 8) as usize) & pmask
    }
}

fn decode_lo(lo: u32) -> Result<Lo, String> {
    if lo == 0x8000_033c {
        return Ok(Lo { kind: LK::Nop, w: lo });
    }
    let opc = lo >> 25;
    let kind = match opc {
        0x00 => LK::Lq,
        0x01 => LK::Sq,
        0x04 => LK::Ilw,
        0x05 => LK::Isw,
        0x08 => LK::Iaddiu,
        0x09 => LK::Isubiu,
        0x10 => LK::Fceq,
        0x11 => LK::Fcset,
        0x12 => LK::Fcand,
        0x13 => LK::Fcor,
        0x16 => LK::Fsand,
        0x18 => LK::Fmeq,
        0x1a => LK::Fmand,
        0x1b => LK::Fmor,
        0x1c => LK::Fcget,
        0x20 => LK::B,
        0x21 => LK::Bal,
        0x24 => LK::Jr,
        0x25 => LK::Jalr,
        0x28 => LK::Ibeq,
        0x29 => LK::Ibne,
        0x2c => LK::Ibltz,
        0x2d => LK::Ibgtz,
        0x2e => LK::Iblez,
        0x2f => LK::Ibgez,
        0x40 => match lo & 0x3f {
            0x30 => LK::Iadd,
            0x31 => LK::Isub,
            0x32 => LK::Iaddi,
            0x34 => LK::Iand,
            0x35 => LK::Ior,
            0x3c..=0x3f => match (lo & 3) | ((lo >> 4) & 0x7c) {
                0x30 => LK::Move,
                0x31 => LK::Mr32,
                0x34 => LK::Lqi,
                0x35 => LK::Sqi,
                0x36 => LK::Lqd,
                0x37 => LK::Sqd,
                0x38 => LK::Div,
                0x39 => LK::Sqrt,
                0x3a => LK::Rsqrt,
                0x3b => LK::Waitq,
                0x3c => LK::Mtir,
                0x3d => LK::Mfir,
                0x3e => LK::Ilwr,
                0x3f => LK::Iswr,
                0x64 => LK::Mfp,
                0x68 => LK::Xtop,
                0x6c => LK::Xgkick,
                0x70 => LK::Esadd,
                0x76 => LK::Esum,
                x => return Err(format!("lower special {x:#x} ({lo:08x})")),
            },
            _ => return Err(format!("lower {lo:08x}")),
        },
        _ => return Err(format!("lower {lo:08x}")),
    };
    Ok(Lo { kind, w: lo })
}

struct Pair {
    up: Up,
    lo: Option<Lo>,
    ibit: bool,
    ebit: bool,
    lw: u32,
    hw: u32,
}

/// The pipeline state a pair starts in.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
struct St {
    /// VF/ACC lanes still in flight: ((reg, lane), cycles left > 0).
    rd: Vec<((usize, usize), u32)>,
    /// Q in flight: cycles left.
    q: Option<u32>,
    /// P in flight (an EFU result, VU1): cycles left.
    p: Option<u32>,
    /// MAC results in flight (cycles left, slot), oldest first; the slot of the newest landed one (None: as before the run).
    mf: Vec<(u32, u8)>,
    mlast: Option<u8>,
    /// Writes of the clip flag in flight, oldest first: (cycles left, slot, what): a CLIP's judgement in `slot` (what is
    /// `JUDGEMENT`), shifted in as it lands, or FCSET's value.
    cf: Vec<(u32, u8, u32)>,
    /// The branch of the pair before (taken-if-flag, target) or an unconditional one.
    pend: Option<(bool, usize)>,
    /// This pair is the E bit's slot: the run ends after it.
    eslot: bool,
    /// The return address a BAL left (VU0): (VI register, micro address). VU1 follows none: a JR goes to whichever of its
    /// register's possible targets the register holds (`P::tables`), which keeps a subroutine one set of nodes however many
    /// places call it.
    ret: Vec<(usize, usize)>,
    /// This pair is a JR's delay slot: the targets the JR can go to (it jumps by `jrt` after the slot).
    jrt: Vec<usize>,
    /// VU1: this pair's branch reads these VI registers (a mask) as they were before the pair before it, which wrote them (the
    /// VU's branch hazard: a branch reads the integer registers a cycle early); their old values are in `hv`.
    hz: u32,
}

type Node = (usize, St);

struct Info {
    stall: u32,
    q_lands: bool,
    p_lands: bool,
    landed: Vec<u8>,
    mac_slot: Option<u8>,
    /// VU1: the MAC results left in flight after those landing, moved down to slots 0, 1, ...
    mshift: usize,
    /// Clip flag writes landing before this pair's lower op reads the clip flag (slot, what), and this pair's CLIP's slot.
    clanded: Vec<(u8, u32)>,
    clip_slot: Option<u8>,
    /// The clip flag writes left in flight after those landing, moved down to slots 0, 1, ...
    cshift: usize,
    next: St,
    park: bool,
}

struct P<'a> {
    skin: bool,
    base: usize,
    code: &'a [u8],
    flags: bool,
    /// VU1: 16 KB memories, no wait loops, computed jumps.
    vu1: bool,
    /// Micro addresses wrap at the micro memory's size (pairs: 0xFF8 VU0, 0x3FF8 VU1).
    pmask: usize,
    /// The links a JR can return through: (VI register, return address) of every BAL and JALR.
    bal_sites: Vec<(usize, usize)>,
    /// Where a jump through a register can go besides the return addresses BAL and JALR leave in it (VU1): the code addresses
    /// an IADDIU from vi00 puts in that register (the programs' hand-made returns, `IADDIU vi15, vi00, ...` in a B's delay
    /// slot), and for a register the program computes or loads, the program's start and every address that starts code after an
    /// unconditional jump or an E bit's slot (the tables of small routines: program 0 jumps to the table vi03 + 5 names, vi03
    /// read from data).
    tables: HashMap<usize, Vec<usize>>,
    /// The registers a JR or JALR jumps through (VU1).
    jump_regs: BTreeSet<usize>,
    spins: BTreeSet<usize>,
}

impl P<'_> {
    fn pair(&self, pc: usize) -> Result<Pair, String> {
        let o = pc - self.base;
        let lw = u32::from_le_bytes(self.code[o..o + 4].try_into().unwrap());
        let hw = u32::from_le_bytes(self.code[o + 4..o + 8].try_into().unwrap());
        let ibit = hw & 0x8000_0000 != 0;
        let up = decode_up(hw).map_err(|e| format!("{pc:04x}: {e}"))?;
        let lo = if ibit { None } else { Some(decode_lo(lw).map_err(|e| format!("{pc:04x}: {e}"))?) };
        Ok(Pair { up, lo, ibit, ebit: hw & 0x4000_0000 != 0, lw, hw })
    }

    /// The branch hazard between pair `p` and the pair at `t` after it (VU1): the VI register `p` changes, when `t`'s branch or
    /// jump reads it (a flag instruction's result is no hazard: the branch sees it).
    fn hazard(&self, p: &Pair, t: usize) -> u32 {
        let Some(l) = p.lo.filter(|_| self.vu1) else { return 0 };
        let Some(w) = l.wvi().filter(|&w| w != 0 && w < 16 && !(0x10..=0x1c).contains(&(l.w >> 25))) else { return 0 };
        if t < self.base || t >= self.base + self.code.len() {
            return 0;
        }
        let Some(b) = self.pair(t).ok().and_then(|q| q.lo) else { return 0 };
        // Jumps too: program 0's IADDIU vi01, vi03, 5 then JALR vi15, vi01 jumps to the vi01 of before (0 on a path that found
        // nothing to draw: the program's start, which ends the run; with the new vi01 the run never ends).
        let reads = match b.kind {
            LK::Ibeq | LK::Ibne => vec![b.is(), b.it()],
            LK::Ibltz | LK::Ibgtz | LK::Iblez | LK::Ibgez | LK::Jr | LK::Jalr => vec![b.is()],
            _ => vec![],
        };
        if reads.contains(&w) { 1 << w } else { 0 }
    }

    /// Whether the code from `e` keeps to the program until its runs end (every path, ignoring the pipeline: fall-through,
    /// branches, jumps to their possible targets, an E bit's slot).
    fn stays(&self, e: usize) -> bool {
        let end = self.base + self.code.len();
        let mut seen: BTreeSet<(usize, Vec<usize>, bool)> = BTreeSet::new();
        // (pc, where the code goes after this pair when it is a delay slot, whether it is the E bit's slot)
        let mut work = vec![(e, Vec::new(), false)];
        while let Some((pc, then, eslot)) = work.pop() {
            if pc < self.base || pc >= end {
                return false;
            }
            if !seen.insert((pc, then.clone(), eslot)) {
                continue;
            }
            let Ok(p) = self.pair(pc) else { return false };
            if eslot {
                continue;
            }
            if !then.is_empty() {
                work.extend(then.into_iter().map(|t| (t, Vec::new(), false)));
                continue;
            }
            let next = pc + 8;
            if p.ebit {
                work.push((next, Vec::new(), true));
                continue;
            }
            let then = match p.lo {
                Some(l) if matches!(l.kind, LK::B | LK::Bal) => vec![l.target(pc, self.pmask)],
                Some(l) if l.is_jump() => {
                    let mut v: Vec<usize> = self.bal_sites.iter().filter(|b| b.0 == l.is()).map(|b| b.1).collect();
                    v.extend(self.tables.get(&l.is()).into_iter().flatten());
                    v
                }
                Some(l) if l.is_branch() => vec![l.target(pc, self.pmask), (pc + 16) & self.pmask],
                _ => Vec::new(),
            };
            work.push((next, then, false));
        }
        true
    }

    /// The state of the pair at `t` after pair `p`: `st` with the branch hazard between them.
    fn after(&self, p: &Pair, t: usize, mut st: St) -> St {
        st.hz = self.hazard(p, t);
        st
    }

    /// A pure wait loop from `t` to the branch at `pc` (and its delay slot): upper NOPs, lower NOPs or branches.
    fn is_spin(&self, t: usize, pc: usize) -> bool {
        if t > pc || pc + 8 - t > 64 {
            return false;
        }
        (t..=pc + 8).step_by(8).all(|a| {
            let Ok(p) = self.pair(a) else { return false };
            matches!(p.up.kind, UK::Nop) && !p.ibit && p.lo.is_some_and(|l| l.kind == LK::Nop || l.is_branch() && l.kind != LK::Bal && l.kind != LK::Jr)
        })
    }

    fn step(&self, n: &Node) -> Result<(Info, Pair), String> {
        let (pc, s) = n;
        let p = self.pair(*pc)?;
        if (s.pend.is_some() || !s.jrt.is_empty()) && p.lo.is_some_and(|l| l.is_branch()) {
            return Err(format!("{pc:04x}: a branch in a branch's delay slot"));
        }
        if s.eslot && p.lo.is_some_and(|l| l.is_branch()) {
            return Err(format!("{pc:04x}: a branch in the E bit's delay slot"));
        }
        let mut stall = 0u32;
        let rd: HashMap<(usize, usize), u32> = s.rd.iter().copied().collect();
        let mut reads = p.up.reads();
        if let Some(l) = &p.lo {
            reads.extend(l.reads());
        }
        for (r, m) in reads {
            for l in 0..4 {
                if m & (8 >> l) != 0
                    && let Some(&c) = rd.get(&(r, l))
                {
                    stall = stall.max(c);
                }
            }
        }
        let waits_q = p.lo.is_some_and(|l| matches!(l.kind, LK::Waitq | LK::Div | LK::Sqrt | LK::Rsqrt));
        if waits_q && let Some(q) = s.q {
            stall = stall.max(q);
        }
        // An EFU instruction waits for the one before it (P in flight).
        if p.lo.is_some_and(|l| matches!(l.kind, LK::Esum | LK::Esadd))
            && let Some(q) = s.p
        {
            stall = stall.max(q);
        }
        let q_lands = s.q.is_some_and(|q| q <= stall);
        let p_lands = s.p.is_some_and(|q| q <= stall);
        // MAC results landing before this pair's lower op reads them.
        let mut landed = Vec::new();
        let mut mf = Vec::new();
        for &(c, slot) in &s.mf {
            if c <= stall {
                landed.push(slot);
            } else {
                mf.push((c - stall - 1, slot));
            }
        }
        let mut mlast = landed.last().copied().or(s.mlast);
        // VU1: the results in flight are slots 0, 1, ... in their order (the oldest land first), so equal queues are one state;
        // a flag instruction reads the newest landed result from the MAC flag register, where its landing put it.
        if self.vu1 {
            for m in mf.iter_mut() {
                m.1 -= landed.len() as u8;
            }
            mlast = None;
        }
        let mshift = mf.len();
        let mut mac_slot = None;
        if self.flags && p.up.mac() {
            if self.vu1 {
                mac_slot = Some(mf.len() as u8);
                mf.push((3, mf.len() as u8));
            } else {
                let used: BTreeSet<u8> = mf.iter().map(|m| m.1).chain(mlast).collect();
                let free = (0..8u8).find(|k| !used.contains(k)).ok_or("more than 8 MAC results in flight")?;
                mac_slot = Some(free);
                mf.push((3, free));
            }
        }
        // The clip flag's writes (CLIP's judgements, FCSET), the same way, in the program's order: the CLIP of the pair, then its
        // lower op's FCSET.
        let mut clanded = Vec::new();
        let mut cf = Vec::new();
        for &(c, slot, what) in &s.cf {
            if c <= stall {
                clanded.push((slot, what));
            } else {
                cf.push((c - stall - 1, slot, what));
            }
        }
        // Slots by place in the queue, as VU1's MAC results.
        for m in cf.iter_mut() {
            m.1 -= clanded.len() as u8;
        }
        let cshift = cf.len();
        let mut clip_slot = None;
        if matches!(p.up.kind, UK::Clip) {
            clip_slot = Some(cf.len() as u8);
            cf.push((3, cf.len() as u8, JUDGEMENT));
        }
        if let Some(l) = p.lo.filter(|l| l.kind == LK::Fcset) {
            cf.push((3, cf.len() as u8, l.imm24()));
        }
        // The state after this pair, if it completes.
        let mut nrd: BTreeMap<(usize, usize), u32> = BTreeMap::new();
        for (&k, &c) in &rd {
            if c > stall + 1 {
                nrd.insert(k, c - stall - 1);
            }
        }
        let mut put = |r: usize, m: u32| {
            if r != 0 && r != NONE {
                for l in 0..4 {
                    if m & (8 >> l) != 0 {
                        nrd.insert((r, l), 3);
                    }
                }
            }
        };
        if let Some(l) = &p.lo
            && let Some((r, m)) = l.wvf()
        {
            put(r, m);
        }
        if let Some((r, m)) = p.up.write() {
            put(r, m);
        }
        let mut nq = match s.q {
            Some(q) if q > stall => Some(q - stall - 1),
            _ => None,
        };
        let mut np = match s.p {
            Some(q) if q > stall => Some(q - stall - 1),
            _ => None,
        };
        if let Some(l) = &p.lo {
            match l.kind {
                LK::Div | LK::Sqrt => nq = Some(6),
                LK::Rsqrt => nq = Some(12),
                // The EFU's latencies: ESADD 11 cycles, ESUM 12.
                LK::Esadd => np = Some(10),
                LK::Esum => np = Some(11),
                _ => {}
            }
        }
        if !self.flags {
            mf.clear();
            mlast = None;
        }
        // Branch, park, return address.
        let mut ret = s.ret.clone();
        let mut jr_targets: Vec<usize> = Vec::new();
        let mut park = false;
        let mut pend = None;
        if let Some(l) = &p.lo {
            match l.kind {
                LK::Bal => {
                    if !self.vu1 {
                        ret = vec![(l.it(), *pc + 16)];
                    }
                    pend = Some((false, l.target(*pc, self.pmask)));
                }
                LK::Jr | LK::Jalr => {
                    jr_targets = match s.ret.iter().find(|r| r.0 == l.is()) {
                        Some(&(_, a)) => vec![a],
                        _ => {
                            let mut v: Vec<usize> = self.bal_sites.iter().filter(|b| b.0 == l.is()).map(|b| b.1).collect();
                            if self.vu1 {
                                v.extend(self.tables.get(&l.is()).into_iter().flatten());
                                v.sort();
                                v.dedup();
                            }
                            if v.is_empty() {
                                return Err(format!("{pc:04x}: JR vi{} with no BAL in the program", l.is()));
                            }
                            v
                        }
                    };
                    ret.clear();
                }
                LK::B => pend = Some((false, l.target(*pc, self.pmask))),
                k if l.is_branch() => {
                    let _ = k;
                    pend = Some((true, l.target(*pc, self.pmask)));
                }
                _ => {}
            }
            if let Some((_, t)) = pend
                && t <= *pc
                && s.pend.is_none()
                && !s.eslot
                && self.spins.contains(&t)
                && self.is_spin(t, *pc)
            {
                park = true;
            }
            // A VI write clobbers a known return address in that register.
            if let Some(w) = l.wvi() {
                ret.retain(|r| r.0 != w);
            }
        }
        let next = St { rd: nrd.into_iter().collect(), q: nq, p: np, mf, mlast, cf, pend, eslot: p.ebit, ret, jrt: jr_targets, hz: 0 };
        Ok((Info { stall, q_lands, p_lands, landed, mac_slot, mshift, clanded, clip_slot, cshift, next, park }, p))
    }
}

fn hx(v: u32) -> String {
    format!("0x{v:08x}")
}

fn vfr(r: usize) -> String {
    format!("c.vf[{r}]")
}

fn upper_expr(u: &Up) -> String {
    let s = vfr(u.fs);
    match u.kind {
        UK::Fmac { k, t, .. } => {
            let tv = match t {
                TSel::Full => vfr(u.ft),
                TSel::Bc(b) => format!("[c.vf[{}][{b}]; 4]", u.ft),
                TSel::Q => "[c.vi[22]; 4]".into(),
                TSel::I => "[c.vi[21]; 4]".into(),
            };
            format!("uycpu::vu0::m_fmac::<{k}, {}>({s}, {tv}, c.vacc)", u.dest)
        }
        UK::Opmula => format!("uycpu::vu0::m_opmula({s}, {})", vfr(u.ft)),
        UK::Opmsub => format!("uycpu::vu0::m_opmsub({s}, {}, c.vacc)", vfr(u.ft)),
        UK::Itof(n) => format!("uycpu::vu0::m_itof::<{n}>({s})"),
        UK::Ftoi(n) => format!("uycpu::vu0::m_ftoi::<{n}>({s})"),
        UK::Abs => format!("uycpu::vu0::m_abs({s})"),
        UK::Clip | UK::Nop => String::new(),
    }
}

/// A clip flag write landing: a CLIP's judgement (in `cs[sl]`) shifted in, or FCSET's value.
fn land_clip(out: &mut String, sl: u8, what: u32) {
    if what == JUDGEMENT {
        writeln!(out, "c.vi[18] = ((c.vi[18] << 6) | cs[{sl}]) & 0xff_ffff;").unwrap();
    } else {
        writeln!(out, "c.vi[18] = {what:#x};").unwrap();
    }
}

/// The code of one pair at one state.
fn emit_pair(prog: &P, pc: usize, p: &Pair, inf: &Info, id: &HashMap<Node, u32>, n: &Node, out: &mut String) -> Result<(), String> {
    let s = &n.1;
    let pmask = prog.pmask;
    writeln!(out, "// {pc:04x} {:08x} {:08x} stall {}{}", p.lw, p.hw, inf.stall, if s.eslot { " (E slot)" } else { "" }).unwrap();
    if inf.q_lands {
        writeln!(out, "c.vi[22] = qp;").unwrap();
    }
    if inf.p_lands {
        writeln!(out, "c.vi[23] = pv;").unwrap();
    }
    for sl in &inf.landed {
        writeln!(out, "c.vi[17] = ms[{sl}];").unwrap();
    }
    if prog.vu1 && !inf.landed.is_empty() {
        for j in 0..inf.mshift {
            writeln!(out, "ms[{j}] = ms[{}];", j + inf.landed.len()).unwrap();
        }
    }
    for &(sl, what) in &inf.clanded {
        land_clip(out, sl, what);
    }
    if !inf.clanded.is_empty() {
        for j in 0..inf.cshift {
            writeln!(out, "cs[{j}] = cs[{}];", j + inf.clanded.len()).unwrap();
        }
    }
    let ue = upper_expr(&p.up);
    if !ue.is_empty() {
        writeln!(out, "let u: [u32; 4] = {ue};").unwrap();
    }
    if matches!(p.up.kind, UK::Clip) {
        if !prog.vu1 {
            return Err(format!("{pc:04x}: CLIP in a microprogram"));
        }
        writeln!(out, "let cj = crate::rt::m_clip(c.vf[{}], c.vf[{}][3]);", p.up.fs, p.up.ft).unwrap();
    }
    // The lower operation, on the state before the pair.
    if let Some(l) = &p.lo {
        let (it, is, idr) = (l.it(), l.is(), l.id());
        // A branch after this pair reads the register this lower op changes as it was (the branch hazard).
        let nexts: Vec<usize> = if !s.jrt.is_empty() {
            s.jrt.clone()
        } else if let Some((_, t)) = s.pend {
            vec![t, (pc + 8) & pmask]
        } else {
            vec![(pc + 8) & pmask]
        };
        if !s.eslot && nexts.iter().any(|&t| prog.hazard(p, t) != 0) {
            writeln!(out, "hv = c.vi[{}];", l.wvi().expect("a hazard's writer")).unwrap();
        }
        // A VI register a branch reads: as the hazard has it.
        let bvi = |r: usize| if s.hz & (1 << r) != 0 { "hv".to_string() } else { format!("c.vi[{r}]") };
        let dmask = if prog.vu1 { 0x3ff } else { 0xff };
        let dq = |r: usize, off: i32| format!("(((c.vi[{r}] as i32).wrapping_add({off}) as u32 & {dmask:#x}) as usize * 16)");
        // The MAC flags a flag instruction reads: the newest landed result's.
        let mac = || match s.mlast {
            _ if prog.vu1 => "c.vi[17]".to_string(),
            _ if inf.landed.last().is_some() => format!("ms[{}]", inf.landed.last().unwrap()),
            Some(k) => format!("ms[{k}]"),
            None => "c.vi[17]".into(),
        };
        match l.kind {
            LK::Nop | LK::Waitq => {}
            // The packet the GS gets: its address (program 6's owner reads the last one's after the run) and its quadwords as data
            // memory holds them at the kick (a program that kicks a buffer and then fills it again, program 0's).
            LK::Xgkick => writeln!(out, "c.kick(c.vi[{is}] & 0x3ff);").unwrap(),
            LK::Lq => {
                if it != 0 {
                    writeln!(out, "{{ let a = {}; let v = uycpu::vu0::m_ld(dm, a); uycpu::vu0::m_put::<{}>(&mut c.vf[{it}], v); }}", dq(is, l.imm11()), l.dest()).unwrap();
                }
            }
            LK::Sq => writeln!(out, "{{ let a = {}; uycpu::vu0::m_st::<{}>(dm, a, c.vf[{is}]); }}", dq(it, l.imm11()), l.dest()).unwrap(),
            LK::Lqi | LK::Lqd => {
                let pre = if l.kind == LK::Lqd { format!("c.set_vi({is}, c.vi[{is}].wrapping_sub(1)); ") } else { String::new() };
                let post = if l.kind == LK::Lqi { format!(" c.set_vi({is}, c.vi[{is}].wrapping_add(1));") } else { String::new() };
                let put = if it != 0 { format!(" uycpu::vu0::m_put::<{}>(&mut c.vf[{it}], v);", l.dest()) } else { String::new() };
                writeln!(out, "{{ {pre}let a = {}; let v = uycpu::vu0::m_ld(dm, a);{put}{post} }}", dq(is, 0)).unwrap();
            }
            LK::Sqi | LK::Sqd => {
                let pre = if l.kind == LK::Sqd { format!("c.set_vi({it}, c.vi[{it}].wrapping_sub(1)); ") } else { String::new() };
                let post = if l.kind == LK::Sqi { format!(" c.set_vi({it}, c.vi[{it}].wrapping_add(1));") } else { String::new() };
                writeln!(out, "{{ {pre}let a = {}; uycpu::vu0::m_st::<{}>(dm, a, c.vf[{is}]);{post} }}", dq(it, 0), l.dest()).unwrap();
            }
            LK::Ilw | LK::Ilwr => {
                let off = if l.kind == LK::Ilw { l.imm11() } else { 0 };
                writeln!(out, "{{ let a = {}; let v = uycpu::vu0::m_ld(dm, a); c.set_vi({it}, v[{}] & 0xffff); }}", dq(is, off), l.lane()).unwrap();
            }
            LK::Isw | LK::Iswr => {
                let off = if l.kind == LK::Isw { l.imm11() } else { 0 };
                writeln!(out, "{{ let a = {}; let v = c.vi[{it}] & 0xffff; uycpu::vu0::m_st::<{}>(dm, a, [v; 4]); }}", dq(is, off), l.dest()).unwrap();
            }
            LK::Iaddiu => writeln!(out, "c.set_vi({it}, c.vi[{is}].wrapping_add({}));", l.imm15()).unwrap(),
            LK::Isubiu => writeln!(out, "c.set_vi({it}, c.vi[{is}].wrapping_sub({}));", l.imm15()).unwrap(),
            LK::Iadd => writeln!(out, "c.set_vi({idr}, c.vi[{is}].wrapping_add(c.vi[{it}]));").unwrap(),
            LK::Isub => writeln!(out, "c.set_vi({idr}, c.vi[{is}].wrapping_sub(c.vi[{it}]));").unwrap(),
            LK::Iaddi => writeln!(out, "c.set_vi({it}, c.vi[{is}].wrapping_add({}));", ((((l.w >> 6) & 31) as i32) << 27 >> 27) as u32).unwrap(),
            LK::Iand => writeln!(out, "c.set_vi({idr}, c.vi[{is}] & c.vi[{it}]);").unwrap(),
            LK::Ior => writeln!(out, "c.set_vi({idr}, c.vi[{is}] | c.vi[{it}]);").unwrap(),
            LK::Move => {
                if it != 0 {
                    writeln!(out, "{{ let v = c.vf[{is}]; uycpu::vu0::m_put::<{}>(&mut c.vf[{it}], v); }}", l.dest()).unwrap()
                }
            }
            LK::Mr32 => {
                if it != 0 {
                    writeln!(out, "{{ let v = c.vf[{is}]; uycpu::vu0::m_put::<{}>(&mut c.vf[{it}], [v[1], v[2], v[3], v[0]]); }}", l.dest()).unwrap()
                }
            }
            LK::Mtir => writeln!(out, "c.set_vi({it}, c.vf[{is}][{}] & 0xffff);", l.fsf()).unwrap(),
            LK::Mfir => {
                if it != 0 {
                    writeln!(out, "{{ let v = c.vi[{is}] as u16 as i16 as i32 as u32; uycpu::vu0::m_put::<{}>(&mut c.vf[{it}], [v; 4]); }}", l.dest()).unwrap()
                }
            }
            LK::Div | LK::Sqrt | LK::Rsqrt => {
                let f = if prog.vu1 { "crate::rt::m_q" } else { "uycpu::vu0::m_q" };
                match l.kind {
                    LK::Div => writeln!(out, "qp = {f}::<0>(c, c.vf[{is}][{}], c.vf[{it}][{}]);", l.fsf(), l.ftf()).unwrap(),
                    LK::Sqrt => writeln!(out, "qp = {f}::<1>(c, 0, c.vf[{it}][{}]);", l.ftf()).unwrap(),
                    _ => writeln!(out, "qp = {f}::<2>(c, c.vf[{is}][{}], c.vf[{it}][{}]);", l.fsf(), l.ftf()).unwrap(),
                }
            }
            LK::Fmeq | LK::Fmand | LK::Fmor => {
                let mac = mac();
                let e = match l.kind {
                    LK::Fmeq => format!("(({mac} & 0xffff) == (c.vi[{is}] & 0xffff)) as u32"),
                    LK::Fmand => format!("{mac} & c.vi[{is}]"),
                    _ => format!("{mac} | c.vi[{is}]"),
                };
                writeln!(out, "c.set_vi({it}, {e});").unwrap();
            }
            LK::Fsand => {
                // The status flag's zero and sign bits are the landed MAC flags' (no program reads its other bits).
                if l.imm12() & !3 != 0 {
                    return Err(format!("{pc:04x}: FSAND {:#x} reads status bits other than zero and sign", l.imm12()));
                }
                let mac = mac();
                writeln!(out, "{{ let m = {mac}; c.set_vi({it}, ((((m & 0xf) != 0) as u32) | ((((m & 0xf0) != 0) as u32) << 1)) & {}); }}", l.imm12()).unwrap();
            }
            LK::Fcand => writeln!(out, "c.set_vi(1, ((c.vi[18] & {:#x}) != 0) as u32);", l.imm24()).unwrap(),
            LK::Fcor => writeln!(out, "c.set_vi(1, (((c.vi[18] | {:#x}) & 0xff_ffff) == 0xff_ffff) as u32);", l.imm24()).unwrap(),
            LK::Fceq => writeln!(out, "c.set_vi(1, ((c.vi[18] & 0xff_ffff) == {:#x}) as u32);", l.imm24()).unwrap(),
            // FCSET's value lands with the clip flag's other writes (`St::cf`).
            LK::Fcset => {}
            LK::Fcget => writeln!(out, "c.set_vi({it}, c.vi[18] & 0xfff);").unwrap(),
            LK::Xtop => writeln!(out, "c.set_vi({it}, c.top);").unwrap(),
            LK::Esum => writeln!(out, "pv = crate::rt::m_esum(c.vf[{is}]);").unwrap(),
            LK::Esadd => writeln!(out, "pv = crate::rt::m_esadd(c.vf[{is}]);").unwrap(),
            LK::Mfp => {
                if it != 0 {
                    writeln!(out, "{{ let v = c.vi[23]; uycpu::vu0::m_put::<{}>(&mut c.vf[{it}], [v; 4]); }}", l.dest()).unwrap()
                }
            }
            LK::Bal => {
                writeln!(out, "c.set_vi({it}, {});", (pc + 16) / 8).unwrap();
            }
            LK::Jr | LK::B | LK::Jalr => {}
            LK::Ibeq | LK::Ibne | LK::Ibltz | LK::Ibgtz | LK::Iblez | LK::Ibgez => {
                let a = format!("({} as u16 as i16)", bvi(is));
                let b = format!("({} as u16 as i16)", bvi(it));
                let cond = match l.kind {
                    LK::Ibeq => format!("{a} == {b}"),
                    LK::Ibne => format!("{a} != {b}"),
                    LK::Ibltz => format!("{a} < 0"),
                    LK::Ibgtz => format!("{a} > 0"),
                    LK::Iblez => format!("{a} <= 0"),
                    _ => format!("{a} >= 0"),
                };
                writeln!(out, "tk = {cond};").unwrap();
            }
        }
        if l.is_jump() {
            writeln!(out, "jrt = ({} as usize * 8) & {pmask:#x};", bvi(is)).unwrap();
        }
        if l.kind == LK::Jalr {
            writeln!(out, "c.set_vi({it}, {});", (pc + 16) / 8).unwrap();
        }
    }
    // The upper's result, after the lower (the upper wins a register both write).
    if let Some((r, m)) = p.up.write()
        && r != 0
    {
        if r == ACC {
            writeln!(out, "uycpu::vu0::m_put::<{m}>(&mut c.vacc, u);").unwrap();
        } else {
            writeln!(out, "uycpu::vu0::m_put::<{m}>(&mut c.vf[{r}], u);").unwrap();
        }
    }
    if p.up.mac() {
        let lanes = p.up.write().map(|w| w.1).unwrap_or(0);
        writeln!(out, "{{ let f = uycpu::vu0::m_mac::<{lanes}>(u); mlast = f; mso |= f; macs = true;{} }}", inf.mac_slot.map(|k| format!(" ms[{k}] = f;")).unwrap_or_default()).unwrap();
    }
    // Keep the normal the game's skin job already transformed. The native renderer
    // consumes it with the job's output instead of repeating the joint transforms.
    if prog.skin {
        match pc {
            0xb0 | 0x188 | 0x2b0 | 0x3d8 | 0x520 | 0x668 => writeln!(out, "c.native(2);").unwrap(),
            0x9a0 => writeln!(out, "c.native(3);").unwrap(),
            _ => {}
        }
    }
    if let Some(k) = inf.clip_slot {
        writeln!(out, "cs[{k}] = cj;").unwrap();
    }
    if p.ibit {
        writeln!(out, "c.vi[21] = {};", hx(p.lw)).unwrap();
    }
    writeln!(out, "pairs += 1;").unwrap();
    let go = |t: usize, st: &St| -> Result<String, String> {
        let k = (t, prog.after(p, t, st.clone()));
        let i = id.get(&k).ok_or_else(|| format!("{pc:04x}: no node for {t:04x}"))?;
        Ok(format!("n = {i}; continue;"))
    };
    // Where the program goes next.
    if inf.park {
        let t = p.lo.expect("a branch").target(pc, pmask);
        writeln!(out, "if tk || {} {{ break 'run ({t}, true); }}", matches!(p.lo.map(|l| l.kind), Some(LK::B))).unwrap();
    }
    if s.eslot {
        // The judgements still in flight land as the run ends, in their order.
        for &(_, sl, what) in &inf.next.cf {
            land_clip(out, sl, what);
        }
        if prog.vu1 {
            writeln!(out, "break 'run {:#x};", END | pc as u32).unwrap();
        } else {
            writeln!(out, "break 'run ({pc}, false);").unwrap();
        }
        return Ok(());
    }
    if let Some((cond, t)) = s.pend {
        // This pair is the delay slot of the branch before.
        let nt = (pc + 8) & pmask;
        let mut st = inf.next.clone();
        st.pend = None;
        if cond {
            writeln!(out, "if btk {{ {} }} else {{ {} }}", go(t, &st)?, go(nt, &st)?).unwrap();
        } else {
            writeln!(out, "{}", go(t, &st)?).unwrap();
        }
        return Ok(());
    }
    if !s.jrt.is_empty() {
        // A JR's delay slot: the jump goes to the address the JR read.
        let unit = if prog.vu1 { "VU1" } else { "VU0" };
        let cases: Vec<String> = s.jrt.iter().map(|&t| go(t, &inf.next).map(|g| format!("{t} => {{ {g} }}"))).collect::<Result<_, _>>()?;
        writeln!(out, "match jrt {{ {} _ => panic!(\"{unit} microprogram: JR to {{jrt:#x}}, where no BAL returns\"), }}", cases.join(" ")).unwrap();
        return Ok(());
    }
    if p.lo.is_some_and(|l| l.is_branch() && !l.is_jump()) {
        writeln!(out, "btk = tk;").unwrap();
    }
    writeln!(out, "{}", go((pc + 8) & pmask, &inf.next)?).unwrap();
    Ok(())
}

/// Translates one program: Rust source of `pub mod <name>`. `vu1`: a VU1 program (`vu1mp.rs`); `calls`: the micro addresses
/// the EE's VCALLMS start (VU0).
pub fn translate(name: &str, base: usize, code: &[u8], calls: &BTreeSet<usize>, vu1: bool) -> Result<String, String> {
    let end = base + code.len();
    let pmask = if vu1 { 0x3ff8 } else { 0xff8 };
    let mut pp = P { skin: !vu1 && name == "skin", base, code, flags: false, vu1, pmask, bal_sites: Vec::new(), tables: HashMap::new(), jump_regs: BTreeSet::new(), spins: BTreeSet::new() };
    // The program reads the MAC flags (FMEQ, FMAND, FMOR, FSAND): their timing is followed.
    let mut entries: BTreeSet<usize> = BTreeSet::new();
    entries.insert(base);
    // VU1: code after an unconditional jump or an E bit's slot (where a computed jump can go), the code addresses the program
    // builds from vi00 by register, the registers it computes or loads (anything but an IADDIU or IADDI from vi00).
    let (mut after, mut by_reg, mut computed) = (BTreeSet::new(), HashMap::<usize, BTreeSet<usize>>::new(), BTreeSet::new());
    for pc in (base..end).step_by(8) {
        let p = pp.pair(pc)?;
        if let Some(l) = &p.lo {
            if matches!(l.kind, LK::Fmeq | LK::Fmand | LK::Fmor | LK::Fsand) {
                pp.flags = true;
            }
            if l.kind == LK::Bal || l.kind == LK::Jalr {
                pp.bal_sites.push((l.it(), pc + 16));
            }
            if l.kind == LK::Iaddiu && l.is() == 0 && l.imm15() as usize * 8 >= base && (l.imm15() as usize * 8) < end {
                by_reg.entry(l.it()).or_default().insert(l.imm15() as usize * 8);
            }
            if vu1 {
                if l.is_jump() {
                    pp.jump_regs.insert(l.is());
                }
                if let Some(w) = l.wvi().filter(|_| !(matches!(l.kind, LK::Iaddiu | LK::Iaddi) && l.is() == 0)) {
                    computed.insert(w);
                }
                if matches!(l.kind, LK::B | LK::Jr) && pc + 16 < end {
                    after.insert(pc + 16);
                }
            } else {
                if l.is_branch() && l.kind != LK::Jr {
                    let t = l.target(pc, pmask);
                    if t >= base && t < end {
                        entries.insert(t);
                        if t <= pc && pp.is_spin(t, pc) {
                            pp.spins.insert(t);
                        }
                    }
                }
                if matches!(l.kind, LK::B | LK::Bal | LK::Jr) && pc + 16 < end {
                    entries.insert(pc + 16);
                }
            }
        }
        if p.ebit && pc + 16 < end {
            if vu1 {
                after.insert(pc + 16);
            } else {
                entries.insert(pc + 16);
            }
        }
    }
    if vu1 {
        for &r in &pp.jump_regs {
            let mut v: Vec<usize> = by_reg.get(&r).into_iter().flatten().copied().collect();
            if computed.contains(&r) {
                v.push(base);
                v.extend(after.iter().copied().filter(|&a| a >= base && a < end));
            }
            v.sort();
            v.dedup();
            pp.tables.insert(r, v);
        }
        // MSCAL's entries: the program's start and the code after each unconditional B's or E bit's delay slot (the entry
        // tables, a program's second entry after its first's code, where the next MSCNT goes on), each whose code stays in the
        // program to an E bit (not the filler after a program's last jump).
        for pc in (base..end).step_by(8) {
            let p = pp.pair(pc)?;
            if (p.lo.is_some_and(|l| l.kind == LK::B) || p.ebit) && pc + 16 < end && pp.stays(pc + 16) {
                entries.insert(pc + 16);
            }
        }
    }
    for &a in calls.range(base..end) {
        entries.insert(a);
    }
    let start = |pc: usize| (pc, St { rd: vec![], q: None, p: None, mf: vec![], mlast: None, cf: vec![], pend: None, eslot: false, ret: vec![], jrt: vec![], hz: 0 });
    // Every node reachable from the entries.
    let mut graph: BTreeMap<u32, (Node, Info)> = BTreeMap::new();
    let mut id: HashMap<Node, u32> = HashMap::new();
    let mut work: Vec<Node> = entries.iter().map(|&e| start(e)).collect();
    let mut pairs: HashMap<usize, Pair> = HashMap::new();
    while let Some(n) = work.pop() {
        if id.contains_key(&n) {
            continue;
        }
        if n.0 < base || n.0 >= end {
            return Err(format!("{name}: the flow leaves the program at {:04x}", n.0));
        }
        let (inf, p) = pp.step(&n)?;
        let k = id.len() as u32;
        id.insert(n.clone(), k);
        let s = &n.1;
        let mut succ: Vec<Node> = Vec::new();
        if !s.jrt.is_empty() {
            for &t in &s.jrt {
                succ.push((t, pp.after(&p, t, inf.next.clone())));
            }
        } else if !s.eslot {
            if let Some((cond, t)) = s.pend {
                let mut st = inf.next.clone();
                st.pend = None;
                succ.push((t, pp.after(&p, t, st.clone())));
                if cond {
                    succ.push(((n.0 + 8) & pmask, pp.after(&p, (n.0 + 8) & pmask, st)));
                }
            } else {
                succ.push(((n.0 + 8) & pmask, pp.after(&p, (n.0 + 8) & pmask, inf.next.clone())));
            }
        }
        for s2 in succ {
            if !id.contains_key(&s2) {
                work.push(s2);
            }
        }
        pairs.insert(n.0, p);
        graph.insert(k, (n, inf));
    }
    let mut body = String::new();
    // VU1: each CHUNK nodes' arms a function of their own.
    let mut chunks: Vec<String> = Vec::new();
    for (k, (n, inf)) in &graph {
        let p = pp.pair(n.0)?;
        let mut arm = String::new();
        emit_pair(&pp, n.0, &p, inf, &id, n, &mut arm)?;
        if vu1 {
            if *k % CHUNK == 0 {
                chunks.push(String::new());
            }
            writeln!(chunks.last_mut().expect("pushed at node 0"), "{k} => {{
{arm}}}").unwrap();
        } else {
            writeln!(body, "{k} => {{
{arm}}}").unwrap();
        }
    }
    let entry_arms: Vec<String> = entries.iter().map(|&e| format!("{e} => Some({}),", id[&start(e)])).collect();
    let park_arms: Vec<String> = pp
        .spins
        .iter()
        .map(|&t| {
            let p = pp.pair(t).expect("decoded");
            let l = p.lo.expect("a lower op");
            if l.kind != LK::Ibeq || l.is() == 0 && l.it() == 0 {
                return Err(format!("{name}: the wait loop at {t:04x} is not an IBEQ viN, vi00 head"));
            }
            let r = if l.is() == 0 { l.it() } else { l.is() };
            Ok(format!("{t} => Some({r}),"))
        })
        .collect::<Result<_, String>>()?;
    let code_lit: Vec<String> = code.iter().map(|b| b.to_string()).collect();
    let mut o = String::new();
    writeln!(o, "/// {name}: {} pairs at micro {base:#x}, {} nodes.", code.len() / 8, graph.len()).unwrap();
    writeln!(o, "#[allow(unused_mut, unused_variables, unused_assignments, unreachable_code, clippy::all)]").unwrap();
    if vu1 {
        // VU1: its own state (`rt::Vu1`), started by MSCAL or MSCNT (no wait loops).
        writeln!(o, "pub mod {name} {{\nuse crate::rt::Vu1;\npub static CODE: [u8; {}] = [{}];", code.len(), code_lit.join(",")).unwrap();
        writeln!(o, "pub fn entry(a: usize) -> Option<u32> {{ match a {{ {} _ => None }} }}", entry_arms.join(" ")).unwrap();
        let names: Vec<String> = (0..chunks.len()).map(|k| format!("c{k}")).collect();
        writeln!(o, "pub fn run(c: &mut Vu1, n: u32) -> usize {{ crate::rt::drive(c, n, &CHUNKS, {CHUNK}) }}").unwrap();
        writeln!(o, "static CHUNKS: [crate::rt::Chunk; {}] = [{}];", chunks.len(), names.join(", ")).unwrap();
        for (k, arms) in chunks.iter().enumerate() {
            writeln!(
                o,
                "fn c{k}(c: &mut Vu1, r: &mut crate::rt::Run, mut n: u32) -> u32 {{\nlet dm: *mut u8 = c.mem.as_mut_ptr() as *mut u8;\nlet crate::rt::Run {{ mut qp, mut pv, mut hv, mut tk, mut btk, mut jrt, mut ms, mut mlast, mut mso, mut macs, mut pairs, mut cs }} = *r;\nlet out = 'run: loop {{ if pairs > 5_000_000 {{ panic!(\"VU1 {name}: 5,000,000 pairs without ending\"); }} match n {{\n{arms}_ => break 'run n,\n}} }};\n*r = crate::rt::Run {{ qp, pv, hv, tk, btk, jrt, ms, mlast, mso, macs, pairs, cs }};\nout\n}}"
            )
            .unwrap();
        }
        o.push_str("}\n");
        return Ok(o);
    }
    writeln!(o, "pub mod {name} {{\nuse uycpu::Cpu;\npub static CODE: [u8; {}] = [{}];", code.len(), code_lit.join(",")).unwrap();
    writeln!(o, "pub fn entry(a: usize) -> Option<u32> {{ match a {{ {} _ => None }} }}", entry_arms.join(" ")).unwrap();
    writeln!(o, "pub fn park(a: usize) -> Option<usize> {{ match a {{ {} _ => None }} }}", park_arms.join(" ")).unwrap();
    writeln!(
        o,
        "pub fn run(c: &mut Cpu, mut n: u32) -> (usize, bool) {{\nlet dm: *mut u8 = c.vu0d;\nlet mut qp: u32 = c.vi[22]; let mut tk = false; let mut btk = false; let mut jrt = 0usize;\nlet mut ms = [0u32; 8]; let mut mlast = 0u32; let mut mso = 0u32; let mut macs = false; let mut pairs = 0u64;\nlet r = 'run: loop {{ if pairs > 5_000_000 {{ panic!(\"VU0 {name}: 5,000,000 pairs without ending or waiting\"); }} match n {{\n{body}_ => unreachable!(),\n}} }};\n// What is in flight lands as the run ends or waits.\nc.vi[22] = qp;\nif macs {{ c.vi[17] = mlast; let st = ((mlast & 0xf != 0) as u32) | (((mlast & 0xf0 != 0) as u32) << 1); let so = ((mso & 0xf != 0) as u32) | (((mso & 0xf0 != 0) as u32) << 1); c.vi[16] = (c.vi[16] & !0xf) | st | (so << 6); }}\nr\n}}\n}}"
    )
    .unwrap();
    Ok(o)
}
