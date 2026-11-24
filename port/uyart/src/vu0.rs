//! VU0's microprograms as the runtime starts them: the game's VU0 microprograms, translated ahead of time by `uyatr`
//! (`Vu0Prog`): VCALLMS runs one natively to its end, or to a wait loop on a VI register that only the EE ends (the
//! microprogram then waits there, VPU-STAT busy, and goes on when a CTC2 changes a VI register). The macro instructions and the
//! microprograms' operations are `uycpu::vu0`'s (the translated code is compiled against that crate). `p100af0`, `tie` and
//! `math` run as hand-written code instead (`vu0native`, with its check UYA_VU0_CHECK); `skin`'s runs are the moby draw
//! path's.

use crate::{Cpu, Ext};

pub use uycpu::vu0::*;

/// A translated VU0 microprogram: its bytes in micro memory from `base`, its entries and wait loops, its code.
pub struct Vu0Prog {
    pub name: &'static str,
    pub base: usize,
    pub code: &'static [u8],
    /// The native code's node for a start or resume at a micro address.
    pub entry: fn(usize) -> Option<u32>,
    /// A wait loop's head: the VI register whose zero ends it.
    pub park: fn(usize) -> Option<usize>,
    /// Runs from a node to the E bit or a wait loop: (the micro address it stopped at, waiting).
    pub run: fn(&mut Cpu, u32) -> (usize, bool),
}

/// COP2's condition: a microprogram is waiting for the EE (`uycpu::Rt::vu0_busy`).
pub(crate) extern "C-unwind" fn vu0_busy(c: &mut Cpu) -> bool {
    c.mach().vu0_parked.is_some()
}

/// CFC2 (`uycpu::Rt::cfc2`).
pub(crate) extern "C-unwind" fn cfc2(c: &mut Cpu, r: u32) -> u32 {
    let r = r as usize;
    match r {
        0..=15 => c.vi[r] & 0xffff,
        29 => c.mach().vu0_parked.is_some() as u32,
        _ => c.vi[r],
    }
}

/// CTC2 (`uycpu::Rt::ctc2`).
pub(crate) extern "C-unwind" fn ctc2(c: &mut Cpu, r: u32, v: u32, pc: u32) {
    let r = r as usize;
    match r {
        0 => {}
        1..=15 => {
            c.vi[r] = v & 0xffff;
            if c.mach().vu0_parked.is_some() {
                vu0_resume(c);
            }
        }
        // FBRST: bit 1 resets VU0 (a waiting microprogram ends); VU1's bits are the renderer's.
        28 => {
            if v & 2 != 0 {
                c.mach().vu0_parked = None;
            }
        }
        // VPU-STAT and TPC are read-only.
        26 | 29 => {}
        31 => panic!("CTC2 to CMSAR1 at {pc:08x}: the game starts a VU1 microprogram from the EE"),
        _ => c.vi[r] = v,
    }
}

/// VCALLMS / VCALLMSR: the microprogram at micro address `a` (`uycpu::Rt::vcallms`).
pub(crate) extern "C-unwind" fn vcallms(c: &mut Cpu, a: u32, pc: u32) {
    if c.mach().vu0_parked.is_some() {
        // A start while one waits: the waiting one gets its chance to go on first (it waits on a VI the EE has set).
        vu0_resume(c);
        if let Some(p) = c.mach().vu0_parked {
            panic!("VCALLMS {a:#x} at {pc:08x} while the microprogram at {p:#x} still waits for the EE");
        }
    }
    vu0_run(c, (a & 0xff8) as usize, pc);
}

fn vu0_resume(c: &mut Cpu) {
    if let Some(p) = c.mach().vu0_parked.take() {
        vu0_run(c, p, 0);
    }
}

fn vu0_run(c: &mut Cpu, a: usize, pc: u32) {
    let Some((i, p)) = c.mach().vu0_index(a) else {
        let m = c.mach();
        panic!("VU0 microprogram at {a:#x} (from {pc:08x}): no translated program holds it (micro memory {})", m.vu0_census());
    };
    let Some(node) = (p.entry)(a) else {
        panic!("VU0 microprogram {} entered at {a:#x} (from {pc:08x}), which is not one of its entries", p.name);
    };
    if let Some(r) = (p.park)(a) {
        // A resume at a wait loop whose register is still not zero: it goes on waiting.
        if c.vi[r] & 0xffff != 0 {
            c.mach().vu0_parked = Some(a);
            return;
        }
    }
    let t = crate::machine::tsc();
    let (end, parked) = match crate::vu0native::native(p.name) {
        Some(f) => crate::vu0native::run(c, f, p, node, a),
        None => (p.run)(c, node),
    };
    let (m, ticks) = (c.mach(), crate::machine::tsc() - t);
    m.prof[1] += ticks;
    m.vu0_runs[i].0 += 1;
    m.vu0_runs[i].1 += ticks;
    m.vu0_parked = if parked { Some(end) } else { None };
}
