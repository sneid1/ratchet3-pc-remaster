//! The addresses program 6's inputs have in a level's RAM, read from the level's own code (`levelcode`'s method).
//!
//! func_L01_0045E810 (frontbin func_003C7E68; 0xC28 bytes; the same function in all 34 story levels) reads every input of the
//! effect draw. Offsets from its start (Veldin's addresses in brackets):
//! - gp-relative loads: +0x0 `lw` the pool base (gp+0x3A48), +0x14 the highest index (gp+0x3A50), +0x54 `lqc2 $vf25` (gp-0x2850),
//!   +0x58 the far limit (gp+0x3A58), +0x48C the VRAM allocator (gp-0x7D00), +0x4AC `lqc2 $vf20` the TEX0 template (gp-0x2860),
//!   +0x5AC `lq` the DMA tag at gp-0x28A0, whose second word (gp-0x289C) is the address of the 11 constant quadwords.
//! - the camera struct (`lui` +0x4, `addiu` +0x8, 0x1E2900): `lqc2` at +0x18 the camera position (+0x140), +0x20/+0x28/+0x30 the
//!   rotation rows (+0, +0x10, +0x20), +0x38..+0x44 the clip columns (+0xC0..+0xF0); the same struct is formed again at +0x5D4
//!   (`lui`), +0x5D8 (`addiu`) and read at +0x5E0..+0x5EC (+0x40..+0x70, the program's first four quadwords).
//! - the view struct (`lui` +0xC, `addiu` +0x10, 0x1E7600): `lqc2` at +0x48 vf24 (+0x1C0), +0x4C vf26 (+0xB0), +0x50 vf27 (+0x1F0);
//!   formed again at +0x5B8 / +0x5BC and read at +0x5C0 (+0x220) and +0x5C4 (+0x1A0).
//! - the texture table: `lui` +0x4C4, `addiu` +0x4C8 (0x220AC0); the sine table: `lui` +0x524, `addiu` +0x528 (0x2BFDA0).
//!
//! Each field address is formed from the instructions of the level's copy of the function (the function is found by its words,
//! `levelcode::locate_function`); the struct bases formed twice must agree, and every load's base register must be the one the
//! base was formed in.

use crate::levelcode::{self, Overlay};

/// Every story level's program 6 input addresses (`STORY_LEVELS` order), each read from the level's own code.
pub fn all(iso_path: &str) -> Result<Vec<(usize, FxAddrs)>, String> {
    let mut d = crate::disc::Disc::open(iso_path)?;
    let tc = crate::toc::read(&mut d)?;
    let reference = levelcode::overlay(&mut d, &tc, 1)?;
    levelcode::STORY_LEVELS
        .iter()
        .map(|&l| {
            let ov = levelcode::overlay(&mut d, &tc, l)?;
            Ok((l, addresses(&reference, &ov).map_err(|e| format!("L{l:02}: {e}"))?))
        })
        .collect()
}
use crate::prog6::FxAddrs;

const NAME: &str = "func_L01_0045E810";
const ADDRESS: u32 = 0x45_E810;
const SIZE: u32 = 0xC28;

fn sext(w: u32) -> i64 {
    (w & 0xFFFF) as u16 as i16 as i64
}

/// The addresses of level `ov`'s program 6 inputs. `reference` is level 1's overlay.
pub fn addresses(reference: &Overlay, ov: &Overlay) -> Result<FxAddrs, String> {
    let start = levelcode::locate_function(reference, ov, NAME, ADDRESS, SIZE)?;
    let gp = levelcode::from_overlay(reference, ov)?.gp;
    let w = |off: u32| ov.word(start + off).ok_or_else(|| format!("L{:02} {NAME} +{off:#x}: no word", ov.level));
    // lw / lqc2 / lq on gp at `off`: the address gp + imm, `op` the required opcode.
    let gp_at = |off: u32, op: u32| -> Result<u32, String> {
        let u = w(off)?;
        if u >> 26 != op || (u >> 21) & 31 != 28 {
            return Err(format!("L{:02} {NAME} +{off:#x}: {u:08x} is not opcode {op:#x} on gp", ov.level));
        }
        Ok((gp as i64 + sext(u)) as u32)
    };
    // The base formed by `lui` at `lui`, `addiu` at `add`.
    let base = |lui: u32, add: u32| -> Result<(u32, u32), String> {
        let (l, a) = (w(lui)?, w(add)?);
        if l >> 26 != 0x0F || a >> 26 != 0x09 || (a >> 21) & 31 != (l >> 16) & 31 {
            return Err(format!("L{:02} {NAME} +{lui:#x}/+{add:#x}: {l:08x} {a:08x} are not a lui and an addiu on its register", ov.level));
        }
        Ok(((((l & 0xFFFF) << 16) as i64 + sext(a)) as u32, (a >> 16) & 31))
    };
    // The field a memory instruction at `off` reads from the base `(address, register)`; `rel` is the field's offset in the struct.
    let field = |b: (u32, u32), off: u32, op: u32, rel: i64| -> Result<u32, String> {
        let u = w(off)?;
        if u >> 26 != op || (u >> 21) & 31 != b.1 {
            return Err(format!("L{:02} {NAME} +{off:#x}: {u:08x} is not opcode {op:#x} on the base register {}", ov.level, b.1));
        }
        if sext(u) != rel {
            return Err(format!("L{:02} {NAME} +{off:#x}: field offset {:#x}, expected {rel:#x}", ov.level, sext(u)));
        }
        Ok((b.0 as i64 + rel) as u32)
    };
    let (lw, lqc2, lq) = (0x23, 0x36, 0x1E);
    let cam = base(0x4, 0x8)?;
    let view = base(0xC, 0x10)?;
    let cam2 = base(0x5D4, 0x5D8)?;
    let view2 = base(0x5B8, 0x5BC)?;
    if cam.0 != cam2.0 || view.0 != view2.0 {
        return Err(format!("L{:02} {NAME}: the camera struct is {:#x} and {:#x}, the view struct {:#x} and {:#x}", ov.level, cam.0, cam2.0, view.0, view2.0));
    }
    let consecutive = |what: &str, offs: &[u32], rels: &[i64], b: (u32, u32), first: u32| -> Result<u32, String> {
        let mut a = first;
        for (k, (&o, &r)) in offs.iter().zip(rels).enumerate() {
            let x = field(b, o, lqc2, r)?;
            if k == 0 {
                a = x;
            } else if x != a + 16 * k as u32 {
                return Err(format!("L{:02} {NAME}: {what} quadword {k} at {x:#x}, not after {a:#x}", ov.level));
            }
        }
        Ok(a)
    };
    let table = |lui: u32, add: u32| -> Result<u32, String> {
        let (l, a) = (w(lui)?, w(add)?);
        if l >> 26 != 0x0F || a >> 26 != 0x09 || (a >> 21) & 31 != (l >> 16) & 31 {
            return Err(format!("L{:02} {NAME} +{lui:#x}/+{add:#x}: not a lui and an addiu on its register", ov.level));
        }
        Ok((((l & 0xFFFF) << 16) as i64 + sext(a)) as u32)
    };
    let a = FxAddrs {
        gp,
        pool: gp_at(0x0, lw)?,
        max_index: gp_at(0x14, lw)?,
        limit: gp_at(0x58, lw)?,
        vf25: gp_at(0x54, lqc2)?,
        vram: gp_at(0x48C, lw)?,
        tex0_template: gp_at(0x4AC, lqc2)?,
        static_ptr: gp_at(0x5AC, lq)? + 4,
        cam: field(cam, 0x18, lqc2, 0x140)?,
        rot: consecutive("rotation", &[0x20, 0x28, 0x30], &[0, 0x10, 0x20], cam, 0)?,
        clip: consecutive("clip", &[0x38, 0x3C, 0x40, 0x44], &[0xC0, 0xD0, 0xE0, 0xF0], cam, 0)?,
        vf24: field(view, 0x48, lqc2, 0x1C0)?,
        vf26: field(view, 0x4C, lqc2, 0xB0)?,
        vf27: field(view, 0x50, lqc2, 0x1F0)?,
        vu_matrix: consecutive("vu matrix", &[0x5E0, 0x5E4, 0x5E8, 0x5EC], &[0x40, 0x50, 0x60, 0x70], cam2, 0)?,
        vu_q4: field(view2, 0x5C0, lq, 0x220)?,
        vu_q5: field(view2, 0x5C4, lq, 0x1A0)?,
        textures: table(0x4C4, 0x4C8)?,
        sine: table(0x524, 0x528)?,
    };
    Ok(a)
}
