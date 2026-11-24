//! The R5900's instruction encoding (the EE core: MIPS III/IV subset, the R5900's own instructions, MMI, the COP1 FPU and the
//! COP2 VU0 macro instructions), decoded into a mnemonic and its fields. An encoding outside these tables is `M::Unknown`; the
//! translator refuses a function holding one.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum M {
    // SPECIAL
    Sll, Srl, Sra, Sllv, Srlv, Srav, Jr, Jalr, Movz, Movn, Syscall, Break, Sync, Mfhi, Mthi, Mflo, Mtlo, Dsllv, Dsrlv, Dsrav,
    Mult, Multu, Div, Divu, Add, Addu, Sub, Subu, And, Or, Xor, Nor, Mfsa, Mtsa, Slt, Sltu, Dadd, Daddu, Dsub, Dsubu,
    Tge, Tgeu, Tlt, Tltu, Teq, Tne, Dsll, Dsrl, Dsra, Dsll32, Dsrl32, Dsra32,
    // REGIMM
    Bltz, Bgez, Bltzl, Bgezl, Bltzal, Bgezal, Bltzall, Bgezall, Mtsab, Mtsah, Tgei, Tgeiu, Tlti, Tltiu, Teqi, Tnei,
    // primary
    J, Jal, Beq, Bne, Blez, Bgtz, Addi, Addiu, Slti, Sltiu, Andi, Ori, Xori, Lui, Beql, Bnel, Blezl, Bgtzl, Daddi, Daddiu,
    Ldl, Ldr, Lq, Sq, Lb, Lh, Lwl, Lw, Lbu, Lhu, Lwr, Lwu, Sb, Sh, Swl, Sw, Sdl, Sdr, Swr, Cache, Lwc1, Pref, Lqc2, Ld,
    Swc1, Sqc2, Sd,
    // COP0
    Mfc0, Mtc0, Bc0f, Bc0t, Bc0fl, Bc0tl, Tlbr, Tlbwi, Tlbwr, Tlbp, Eret, Ei, Di,
    // COP1
    Mfc1, Cfc1, Mtc1, Ctc1, Bc1f, Bc1t, Bc1fl, Bc1tl, AddS, SubS, MulS, DivS, SqrtS, AbsS, MovS, NegS, RsqrtS, AddaS, SubaS,
    MulaS, MaddS, MsubS, MaddaS, MsubaS, CvtWS, MaxS, MinS, CF, CEq, CLt, CLe, CvtSW,
    // COP2 transfers and branches
    Qmfc2, Cfc2, Qmtc2, Ctc2, Bc2f, Bc2t, Bc2fl, Bc2tl,
    /// A VU0 macro instruction (COP2 with the CO bit): decoded further by `vu`.
    Vu,
    // MMI
    Madd, Maddu, Plzcw, Mfhi1, Mthi1, Mflo1, Mtlo1, Mult1, Multu1, Div1, Divu1, Madd1, Maddu1, Pmfhl, Pmthl, Psllh, Psrlh,
    Psrah, Psllw, Psrlw, Psraw,
    // MMI0
    Paddw, Psubw, Pcgtw, Pmaxw, Paddh, Psubh, Pcgth, Pmaxh, Paddb, Psubb, Pcgtb, Paddsw, Psubsw, Pextlw, Ppacw, Paddsh,
    Psubsh, Pextlh, Ppach, Paddsb, Psubsb, Pextlb, Ppacb, Pext5, Ppac5,
    // MMI1
    Pabsw, Pceqw, Pminw, Padsbh, Pabsh, Pceqh, Pminh, Pceqb, Padduw, Psubuw, Pextuw, Padduh, Psubuh, Pextuh, Paddub, Psubub,
    Pextub, Qfsrv,
    // MMI2
    Pmaddw, Psllvw, Psrlvw, Pmsubw, Pmfhi, Pmflo, Pinth, Pmultw, Pdivw, Pcpyld, Pmaddh, Phmadh, Pand, Pxor, Pmsubh, Phmsbh,
    Pexeh, Prevh, Pmulth, Pdivbw, Pexew, Prot3w,
    // MMI3
    Pmadduw, Psravw, Pmthi, Pmtlo, Pinteh, Pmultuw, Pdivuw, Pcpyud, Por, Pnor, Pexch, Pcpyh, Pexcw,
    Unknown,
}

#[derive(Clone, Copy, Debug)]
pub struct I {
    pub w: u32,
    pub m: M,
}

impl I {
    pub fn rs(&self) -> usize {
        ((self.w >> 21) & 31) as usize
    }
    pub fn rt(&self) -> usize {
        ((self.w >> 16) & 31) as usize
    }
    pub fn rd(&self) -> usize {
        ((self.w >> 11) & 31) as usize
    }
    pub fn sa(&self) -> u32 {
        (self.w >> 6) & 31
    }
    /// The sign-extended 16-bit immediate.
    pub fn simm(&self) -> i32 {
        self.w as u16 as i16 as i32
    }
    pub fn uimm(&self) -> u32 {
        self.w & 0xffff
    }
    /// A branch's target from the instruction at `pc`.
    pub fn branch_target(&self, pc: u32) -> u32 {
        pc.wrapping_add(4).wrapping_add((self.simm() << 2) as u32)
    }
    /// A J / JAL target from the instruction at `pc`.
    pub fn jump_target(&self, pc: u32) -> u32 {
        (pc.wrapping_add(4) & 0xf000_0000) | ((self.w & 0x03ff_ffff) << 2)
    }

    /// The instruction has a delay slot: every branch and jump.
    pub fn has_delay(&self) -> bool {
        use M::*;
        matches!(
            self.m,
            Jr | Jalr | J | Jal | Beq | Bne | Blez | Bgtz | Beql | Bnel | Blezl | Bgtzl | Bltz | Bgez | Bltzl | Bgezl | Bltzal | Bgezal
                | Bltzall | Bgezall | Bc0f | Bc0t | Bc0fl | Bc0tl | Bc1f | Bc1t | Bc1fl | Bc1tl | Bc2f | Bc2t | Bc2fl | Bc2tl
        )
    }

    /// A conditional branch (both successors), likely or not.
    pub fn is_cond_branch(&self) -> bool {
        self.has_delay() && !matches!(self.m, M::Jr | M::Jalr | M::J | M::Jal)
    }

    /// A likely branch: the delay slot runs only when taken.
    pub fn is_likely(&self) -> bool {
        use M::*;
        matches!(self.m, Beql | Bnel | Blezl | Bgtzl | Bltzl | Bgezl | Bltzall | Bgezall | Bc0fl | Bc0tl | Bc1fl | Bc1tl | Bc2fl | Bc2tl)
    }

    /// A branch that links ($ra = pc + 8).
    pub fn links(&self) -> bool {
        matches!(self.m, M::Bltzal | M::Bgezal | M::Bltzall | M::Bgezall)
    }

    /// Writes memory.
    pub fn is_store(&self) -> bool {
        use M::*;
        matches!(self.m, Sb | Sh | Sw | Sd | Sq | Swl | Swr | Sdl | Sdr | Swc1 | Sqc2)
    }

    pub fn is_load(&self) -> bool {
        use M::*;
        matches!(self.m, Lb | Lbu | Lh | Lhu | Lw | Lwu | Ld | Lq | Lwl | Lwr | Ldl | Ldr | Lwc1 | Lqc2)
    }
}

pub fn decode(w: u32) -> I {
    use M::*;
    let op = w >> 26;
    let rs = (w >> 21) & 31;
    let rt = (w >> 16) & 31;
    let sa = (w >> 6) & 31;
    let f = w & 63;
    let m = match op {
        0x00 => match f {
            0x00 => Sll,
            0x02 => Srl,
            0x03 => Sra,
            0x04 => Sllv,
            0x06 => Srlv,
            0x07 => Srav,
            0x08 => Jr,
            0x09 => Jalr,
            0x0a => Movz,
            0x0b => Movn,
            0x0c => Syscall,
            0x0d => Break,
            0x0f => Sync,
            0x10 => Mfhi,
            0x11 => Mthi,
            0x12 => Mflo,
            0x13 => Mtlo,
            0x14 => Dsllv,
            0x16 => Dsrlv,
            0x17 => Dsrav,
            0x18 => Mult,
            0x19 => Multu,
            0x1a => Div,
            0x1b => Divu,
            0x20 => Add,
            0x21 => Addu,
            0x22 => Sub,
            0x23 => Subu,
            0x24 => And,
            0x25 => Or,
            0x26 => Xor,
            0x27 => Nor,
            0x28 => Mfsa,
            0x29 => Mtsa,
            0x2a => Slt,
            0x2b => Sltu,
            0x2c => Dadd,
            0x2d => Daddu,
            0x2e => Dsub,
            0x2f => Dsubu,
            0x30 => Tge,
            0x31 => Tgeu,
            0x32 => Tlt,
            0x33 => Tltu,
            0x34 => Teq,
            0x36 => Tne,
            0x38 => Dsll,
            0x3a => Dsrl,
            0x3b => Dsra,
            0x3c => Dsll32,
            0x3e => Dsrl32,
            0x3f => Dsra32,
            _ => Unknown,
        },
        0x01 => match rt {
            0x00 => Bltz,
            0x01 => Bgez,
            0x02 => Bltzl,
            0x03 => Bgezl,
            0x08 => Tgei,
            0x09 => Tgeiu,
            0x0a => Tlti,
            0x0b => Tltiu,
            0x0c => Teqi,
            0x0e => Tnei,
            0x10 => Bltzal,
            0x11 => Bgezal,
            0x12 => Bltzall,
            0x13 => Bgezall,
            0x18 => Mtsab,
            0x19 => Mtsah,
            _ => Unknown,
        },
        0x02 => J,
        0x03 => Jal,
        0x04 => Beq,
        0x05 => Bne,
        0x06 => Blez,
        0x07 => Bgtz,
        0x08 => Addi,
        0x09 => Addiu,
        0x0a => Slti,
        0x0b => Sltiu,
        0x0c => Andi,
        0x0d => Ori,
        0x0e => Xori,
        0x0f => Lui,
        0x10 => match rs {
            0x00 => Mfc0,
            0x04 => Mtc0,
            0x08 => match rt {
                0 => Bc0f,
                1 => Bc0t,
                2 => Bc0fl,
                3 => Bc0tl,
                _ => Unknown,
            },
            0x10 => match f {
                0x01 => Tlbr,
                0x02 => Tlbwi,
                0x06 => Tlbwr,
                0x08 => Tlbp,
                0x18 => Eret,
                0x38 => Ei,
                0x39 => Di,
                _ => Unknown,
            },
            _ => Unknown,
        },
        0x11 => match rs {
            0x00 => Mfc1,
            0x02 => Cfc1,
            0x04 => Mtc1,
            0x06 => Ctc1,
            0x08 => match rt {
                0 => Bc1f,
                1 => Bc1t,
                2 => Bc1fl,
                3 => Bc1tl,
                _ => Unknown,
            },
            0x10 => match f {
                0x00 => AddS,
                0x01 => SubS,
                0x02 => MulS,
                0x03 => DivS,
                0x04 => SqrtS,
                0x05 => AbsS,
                0x06 => MovS,
                0x07 => NegS,
                0x16 => RsqrtS,
                0x18 => AddaS,
                0x19 => SubaS,
                0x1a => MulaS,
                0x1c => MaddS,
                0x1d => MsubS,
                0x1e => MaddaS,
                0x1f => MsubaS,
                0x24 => CvtWS,
                0x28 => MaxS,
                0x29 => MinS,
                0x30 => CF,
                0x32 => CEq,
                0x34 => CLt,
                0x36 => CLe,
                _ => Unknown,
            },
            0x14 => match f {
                0x20 => CvtSW,
                _ => Unknown,
            },
            _ => Unknown,
        },
        0x12 => match rs {
            0x01 => Qmfc2,
            0x02 => Cfc2,
            0x05 => Qmtc2,
            0x06 => Ctc2,
            0x08 => match rt {
                0 => Bc2f,
                1 => Bc2t,
                2 => Bc2fl,
                3 => Bc2tl,
                _ => Unknown,
            },
            0x10..=0x1f => Vu,
            _ => Unknown,
        },
        0x14 => Beql,
        0x15 => Bnel,
        0x16 => Blezl,
        0x17 => Bgtzl,
        0x18 => Daddi,
        0x19 => Daddiu,
        0x1a => Ldl,
        0x1b => Ldr,
        0x1c => match f {
            0x00 => Madd,
            0x01 => Maddu,
            0x04 => Plzcw,
            0x08 => match sa {
                0x00 => Paddw,
                0x01 => Psubw,
                0x02 => Pcgtw,
                0x03 => Pmaxw,
                0x04 => Paddh,
                0x05 => Psubh,
                0x06 => Pcgth,
                0x07 => Pmaxh,
                0x08 => Paddb,
                0x09 => Psubb,
                0x0a => Pcgtb,
                0x10 => Paddsw,
                0x11 => Psubsw,
                0x12 => Pextlw,
                0x13 => Ppacw,
                0x14 => Paddsh,
                0x15 => Psubsh,
                0x16 => Pextlh,
                0x17 => Ppach,
                0x18 => Paddsb,
                0x19 => Psubsb,
                0x1a => Pextlb,
                0x1b => Ppacb,
                0x1e => Pext5,
                0x1f => Ppac5,
                _ => Unknown,
            },
            0x09 => match sa {
                0x00 => Pmaddw,
                0x02 => Psllvw,
                0x03 => Psrlvw,
                0x04 => Pmsubw,
                0x08 => Pmfhi,
                0x09 => Pmflo,
                0x0a => Pinth,
                0x0c => Pmultw,
                0x0d => Pdivw,
                0x0e => Pcpyld,
                0x10 => Pmaddh,
                0x11 => Phmadh,
                0x12 => Pand,
                0x13 => Pxor,
                0x14 => Pmsubh,
                0x15 => Phmsbh,
                0x1a => Pexeh,
                0x1b => Prevh,
                0x1c => Pmulth,
                0x1d => Pdivbw,
                0x1e => Pexew,
                0x1f => Prot3w,
                _ => Unknown,
            },
            0x10 => Mfhi1,
            0x11 => Mthi1,
            0x12 => Mflo1,
            0x13 => Mtlo1,
            0x18 => Mult1,
            0x19 => Multu1,
            0x1a => Div1,
            0x1b => Divu1,
            0x20 => Madd1,
            0x21 => Maddu1,
            0x28 => match sa {
                0x01 => Pabsw,
                0x02 => Pceqw,
                0x03 => Pminw,
                0x04 => Padsbh,
                0x05 => Pabsh,
                0x06 => Pceqh,
                0x07 => Pminh,
                0x0a => Pceqb,
                0x10 => Padduw,
                0x11 => Psubuw,
                0x12 => Pextuw,
                0x14 => Padduh,
                0x15 => Psubuh,
                0x16 => Pextuh,
                0x18 => Paddub,
                0x19 => Psubub,
                0x1a => Pextub,
                0x1b => Qfsrv,
                _ => Unknown,
            },
            0x29 => match sa {
                0x00 => Pmadduw,
                0x03 => Psravw,
                0x08 => Pmthi,
                0x09 => Pmtlo,
                0x0a => Pinteh,
                0x0c => Pmultuw,
                0x0d => Pdivuw,
                0x0e => Pcpyud,
                0x12 => Por,
                0x13 => Pnor,
                0x1a => Pexch,
                0x1b => Pcpyh,
                0x1e => Pexcw,
                _ => Unknown,
            },
            0x30 => Pmfhl,
            0x31 => Pmthl,
            0x34 => Psllh,
            0x36 => Psrlh,
            0x37 => Psrah,
            0x3c => Psllw,
            0x3e => Psrlw,
            0x3f => Psraw,
            _ => Unknown,
        },
        0x1e => Lq,
        0x1f => Sq,
        0x20 => Lb,
        0x21 => Lh,
        0x22 => Lwl,
        0x23 => Lw,
        0x24 => Lbu,
        0x25 => Lhu,
        0x26 => Lwr,
        0x27 => Lwu,
        0x28 => Sb,
        0x29 => Sh,
        0x2a => Swl,
        0x2b => Sw,
        0x2c => Sdl,
        0x2d => Sdr,
        0x2e => Swr,
        0x2f => Cache,
        0x31 => Lwc1,
        0x33 => Pref,
        0x36 => Lqc2,
        0x37 => Ld,
        0x39 => Swc1,
        0x3e => Sqc2,
        0x3f => Sd,
        _ => Unknown,
    };
    I { w, m }
}

/// The VU0 macro instructions (COP2, CO bit set): upper and integer operations in macro mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum V {
    /// FMAC with a broadcast field (x y z w = 0..3), `k` the kind, `acc` writes ACC instead of fd.
    Bc { k: K, acc: bool, bc: u8 },
    /// FMAC with Q or I as the second operand.
    Qi { k: K, acc: bool, i: bool },
    /// FMAC lane by lane.
    Full { k: K, acc: bool },
    Opmula,
    Opmsub,
    Itof(u8),
    Ftoi(u8),
    Abs,
    Clip,
    Nop,
    Move,
    Mr32,
    Lqi,
    Sqi,
    Lqd,
    Sqd,
    Div,
    Sqrt,
    Rsqrt,
    Waitq,
    Mtir,
    Mfir,
    Ilwr,
    Iswr,
    Rnext,
    Rget,
    Rinit,
    Rxor,
    Iadd,
    Isub,
    Iaddi,
    Iand,
    Ior,
    Callms,
    Callmsr,
    Unknown,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum K {
    Add,
    Sub,
    Madd,
    Msub,
    Max,
    Mini,
    Mul,
}

pub fn decode_vu(w: u32) -> V {
    let f = w & 63;
    let bc = (w & 3) as u8;
    const BCK: [K; 7] = [K::Add, K::Sub, K::Madd, K::Msub, K::Max, K::Mini, K::Mul];
    if f < 0x3c {
        return match f {
            0x00..=0x1b => V::Bc { k: BCK[(f >> 2) as usize], acc: false, bc },
            0x1c => V::Qi { k: K::Mul, acc: false, i: false },
            0x1d => V::Qi { k: K::Max, acc: false, i: true },
            0x1e => V::Qi { k: K::Mul, acc: false, i: true },
            0x1f => V::Qi { k: K::Mini, acc: false, i: true },
            0x20 => V::Qi { k: K::Add, acc: false, i: false },
            0x21 => V::Qi { k: K::Madd, acc: false, i: false },
            0x22 => V::Qi { k: K::Add, acc: false, i: true },
            0x23 => V::Qi { k: K::Madd, acc: false, i: true },
            0x24 => V::Qi { k: K::Sub, acc: false, i: false },
            0x25 => V::Qi { k: K::Msub, acc: false, i: false },
            0x26 => V::Qi { k: K::Sub, acc: false, i: true },
            0x27 => V::Qi { k: K::Msub, acc: false, i: true },
            0x28 => V::Full { k: K::Add, acc: false },
            0x29 => V::Full { k: K::Madd, acc: false },
            0x2a => V::Full { k: K::Mul, acc: false },
            0x2b => V::Full { k: K::Max, acc: false },
            0x2c => V::Full { k: K::Sub, acc: false },
            0x2d => V::Full { k: K::Msub, acc: false },
            0x2e => V::Opmsub,
            0x2f => V::Full { k: K::Mini, acc: false },
            0x30 => V::Iadd,
            0x31 => V::Isub,
            0x32 => V::Iaddi,
            0x34 => V::Iand,
            0x35 => V::Ior,
            0x38 => V::Callms,
            0x39 => V::Callmsr,
            _ => V::Unknown,
        };
    }
    let x = (w & 3) | ((w >> 4) & 0x7c);
    match x {
        0x00..=0x0f => V::Bc { k: BCK[(x >> 2) as usize], acc: true, bc },
        0x10..=0x13 => V::Itof((x - 0x10) as u8),
        0x14..=0x17 => V::Ftoi((x - 0x14) as u8),
        0x18..=0x1b => V::Bc { k: K::Mul, acc: true, bc },
        0x1c => V::Qi { k: K::Mul, acc: true, i: false },
        0x1d => V::Abs,
        0x1e => V::Qi { k: K::Mul, acc: true, i: true },
        0x1f => V::Clip,
        0x20 => V::Qi { k: K::Add, acc: true, i: false },
        0x21 => V::Qi { k: K::Madd, acc: true, i: false },
        0x22 => V::Qi { k: K::Add, acc: true, i: true },
        0x23 => V::Qi { k: K::Madd, acc: true, i: true },
        0x24 => V::Qi { k: K::Sub, acc: true, i: false },
        0x25 => V::Qi { k: K::Msub, acc: true, i: false },
        0x26 => V::Qi { k: K::Sub, acc: true, i: true },
        0x27 => V::Qi { k: K::Msub, acc: true, i: true },
        0x28 => V::Full { k: K::Add, acc: true },
        0x29 => V::Full { k: K::Madd, acc: true },
        0x2a => V::Full { k: K::Mul, acc: true },
        0x2c => V::Full { k: K::Sub, acc: true },
        0x2d => V::Full { k: K::Msub, acc: true },
        0x2e => V::Opmula,
        0x2f => V::Nop,
        0x30 => V::Move,
        0x31 => V::Mr32,
        0x34 => V::Lqi,
        0x35 => V::Sqi,
        0x36 => V::Lqd,
        0x37 => V::Sqd,
        0x38 => V::Div,
        0x39 => V::Sqrt,
        0x3a => V::Rsqrt,
        0x3b => V::Waitq,
        0x3c => V::Mtir,
        0x3d => V::Mfir,
        0x3e => V::Ilwr,
        0x3f => V::Iswr,
        0x40 => V::Rnext,
        0x41 => V::Rget,
        0x42 => V::Rinit,
        0x43 => V::Rxor,
        _ => V::Unknown,
    }
}

const REG: [&str; 32] = [
    "zero", "at", "v0", "v1", "a0", "a1", "a2", "a3", "t0", "t1", "t2", "t3", "t4", "t5", "t6", "t7", "s0", "s1", "s2", "s3", "s4", "s5",
    "s6", "s7", "t8", "t9", "k0", "k1", "gp", "sp", "fp", "ra",
];

/// A listing line for the instruction at `pc` (for reading the game's code: `uyatr ... dis`).
pub fn text(i: &I, pc: u32) -> String {
    use M::*;
    let (rs, rt, rd) = (REG[i.rs()], REG[i.rt()], REG[i.rd()]);
    let name = format!("{:?}", i.m).to_lowercase();
    match i.m {
        Lb | Lbu | Lh | Lhu | Lw | Lwu | Ld | Lq | Lwl | Lwr | Ldl | Ldr | Sb | Sh | Sw | Sd | Sq | Swl | Swr | Sdl | Sdr => format!("{name} {rt}, {:#x}({rs})", i.simm()),
        Lwc1 | Swc1 => format!("{name} $f{}, {:#x}({rs})", i.rt(), i.simm()),
        Lqc2 | Sqc2 => format!("{name} vf{}, {:#x}({rs})", i.rt(), i.simm()),
        Addi | Addiu | Slti | Sltiu | Daddi | Daddiu => format!("{name} {rt}, {rs}, {}", i.simm()),
        Andi | Ori | Xori => format!("{name} {rt}, {rs}, {:#x}", i.uimm()),
        Lui => format!("lui {rt}, {:#x}", i.uimm()),
        Beq | Bne | Beql | Bnel => format!("{name} {rs}, {rt}, {:08x}", i.branch_target(pc)),
        Blez | Bgtz | Bltz | Bgez | Blezl | Bgtzl | Bltzl | Bgezl | Bltzal | Bgezal | Bltzall | Bgezall => format!("{name} {rs}, {:08x}", i.branch_target(pc)),
        Bc0f | Bc0t | Bc0fl | Bc0tl | Bc1f | Bc1t | Bc1fl | Bc1tl | Bc2f | Bc2t | Bc2fl | Bc2tl => format!("{name} {:08x}", i.branch_target(pc)),
        J | Jal => format!("{name} {:08x}", i.jump_target(pc)),
        Jr => format!("jr {rs}"),
        Jalr => format!("jalr {rd}, {rs}"),
        Sll | Srl | Sra | Dsll | Dsrl | Dsra | Dsll32 | Dsrl32 | Dsra32 => {
            if i.w == 0 { "nop".into() } else { format!("{name} {rd}, {rt}, {}", i.sa()) }
        }
        Syscall => "syscall".into(),
        Mfc0 | Mtc0 => format!("{name} {rt}, ${}", i.rd()),
        Mfc1 | Mtc1 | Cfc1 | Ctc1 => format!("{name} {rt}, $f{}", i.rd()),
        Qmfc2 | Qmtc2 | Cfc2 | Ctc2 => format!("{name} {rt}, vi/vf{}", i.rd()),
        Vu => format!("vu {:?} (dest {:x}, ft {}, fs {}, fd {})", decode_vu(i.w), (i.w >> 21) & 15, i.rt(), i.rd(), i.sa()),
        _ if i.w >> 26 == 0x11 => format!("{name} $f{}, $f{}, $f{}", i.sa(), i.rd(), i.rt()),
        _ => format!("{name} {rd}, {rs}, {rt}"),
    }
}
