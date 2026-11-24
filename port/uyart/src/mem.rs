//! Every address the fast paths in `Cpu::rd32` and friends do not take: RAM through its other windows (kseg0/kseg1 at 0x8000_0000
//! and 0xA000_0000, the uncached 0x2000_0000 and accelerated 0x3000_0000 mirrors), the hardware registers (`hw`), VU0's and VU1's
//! memories as the EE sees them (0x1100_0000), the GS's privileged registers (0x1200_0000). Anything else stops the game.

use crate::Cpu;

/// Where an address lands.
enum At {
    Ram(u32),
    Spr(u32),
    Hw(u32),
}

fn at(a: u32) -> At {
    if a >> 28 == 7 {
        return At::Spr(a & 0x3fff);
    }
    let p = a & 0x1fff_ffff;
    if p < crate::RAM_SIZE {
        // kseg0 / kseg1 / the uncached mirrors (0x2000_0000 / 0x3000_0000 reach RAM through the TLB the kernel sets up).
        At::Ram(p)
    } else {
        At::Hw(p)
    }
}

/// The slow load path (`uycpu::Rt::rd_slow`).
pub(crate) extern "C-unwind" fn rd_slow(c: &mut Cpu, a: u32, n: u32) -> u64 {
    match at(a) {
        At::Ram(p) => {
            let mut v = 0u64;
            for k in 0..n {
                v |= (unsafe { *c.ram.add((p + k) as usize) } as u64) << (8 * k);
            }
            v
        }
        At::Spr(p) => {
            let mut v = 0u64;
            for k in 0..n {
                v |= (unsafe { *c.spr.add(((p + k) & 0x3fff) as usize) } as u64) << (8 * k);
            }
            v
        }
        At::Hw(p) => crate::hw::read(c, p, n),
    }
}

/// The slow store path (`uycpu::Rt::wr_slow`).
pub(crate) extern "C-unwind" fn wr_slow(c: &mut Cpu, a: u32, n: u32, v: u64) {
    match at(a) {
        At::Ram(p) => {
            for k in 0..n {
                unsafe { *c.ram.add((p + k) as usize) = (v >> (8 * k)) as u8 };
            }
        }
        At::Spr(p) => {
            for k in 0..n {
                unsafe { *c.spr.add(((p + k) & 0x3fff) as usize) = (v >> (8 * k)) as u8 };
            }
        }
        At::Hw(p) => crate::hw::write(c, p, n, v),
    }
}

/// LQ's slow path (`uycpu::Rt::rd128_slow`).
pub(crate) extern "C-unwind" fn rd128_slow(c: &mut Cpu, a: u32, q: &mut [u64; 2]) {
    let (lo, hi) = match at(a) {
        At::Hw(p) => crate::hw::read128(c, p),
        _ => (rd_slow(c, a, 8), rd_slow(c, a + 8, 8)),
    };
    *q = [lo, hi];
}

/// SQ's slow path (`uycpu::Rt::wr128_slow`).
pub(crate) extern "C-unwind" fn wr128_slow(c: &mut Cpu, a: u32, lo: u64, hi: u64) {
    match at(a) {
        At::Hw(p) => crate::hw::write128(c, p, lo, hi),
        _ => {
            wr_slow(c, a, 8, lo);
            wr_slow(c, a + 8, 8, hi);
        }
    }
}
