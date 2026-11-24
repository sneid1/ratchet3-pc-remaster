//! VU1 as the game's translated VU1 microprograms run on it (written by port/uyatr from its vu1rt.rs; the programs are the
//! modules beside this one): the registers, 16 KB of data memory, VIF1's TOP, and the operations the translated code calls
//! besides `uycpu::vu0`'s whole-vector ones. Its owner walks the VIF1 chain (UNPACK into `mem`, MPG into micro memory) and
//! starts a program at an MSCAL's entry (`Prog::entry`, then `Prog::run`). The arithmetic is the host's single precision under
//! the game's MXCSR (`uycpu::fpu::MXCSR`: toward zero, DAZ, FTZ), which the caller sets on its thread while programs run.

/// VU1's state: VF, ACC, VI (16 the status flag, 17 the MAC flags, 18 the clip flag, 21 I, 22 Q, 23 P), data memory, the TOP the
/// running program reads (XTOP), and the address of each XGKICK since the owner last cleared them (`packet` reads one) with the
/// packet the GIF got from it (`sent`: read at the kick, as a program that kicks a buffer goes on to fill it again; with
/// `defer_packets`, at the next kick or `finish`).
pub struct Vu1 {
    pub vf: [[u32; 4]; 32],
    pub vacc: [u32; 4],
    pub vi: [u32; 32],
    pub mem: Box<[[u32; 4]; 1024]>,
    pub top: u32,
    pub kicks: Vec<u32>,
    pub sent: Vec<Result<Vec<[u32; 4]>, String>>,
    /// Capture packet bytes at each kick. Program 6's consumer reads its completed buffer after the run instead.
    pub capture_packets: bool,
    /// With `capture_packets`: a kicked packet is read when the next XGKICK comes or `finish` is called, not at its own XGKICK
    /// (the GIF reads a PATH1 packet while the program runs on, and an XGKICK waits for the one before to be sent: program 6
    /// kicks a packet and then writes it).
    pub defer_packets: bool,
    pending: Option<u32>,
}

impl Default for Vu1 {
    fn default() -> Self {
        Self::new()
    }
}

impl Vu1 {
    /// A VU1 that has run nothing: every register 0 but vf00's w (1.0), memory 0.
    pub fn new() -> Vu1 {
        let mut vf = [[0u32; 4]; 32];
        vf[0] = [0, 0, 0, 0x3f80_0000];
        Vu1 { vf, vacc: [0; 4], vi: [0; 32], mem: Box::new([[0; 4]; 1024]), top: 0, kicks: Vec::new(), sent: Vec::new(), capture_packets: true, defer_packets: false, pending: None }
    }

    /// XGKICK at `a`: the address onto `kicks`, the packet as data memory holds it now onto `sent`.
    pub fn kick(&mut self, a: u32) {
        self.kicks.push(a);
        if self.capture_packets && self.defer_packets {
            if let Some(b) = self.pending.replace(a) {
                let p = self.packet(b);
                self.sent.push(p);
            }
        } else if self.capture_packets {
            let p = self.packet(a);
            self.sent.push(p);
        }
    }

    /// The run's end with `defer_packets`: its last kicked packet onto `sent`, as data memory holds it now.
    pub fn finish(&mut self) {
        if let Some(b) = self.pending.take() {
            let p = self.packet(b);
            self.sent.push(p);
        }
    }

    /// The GIF packet an XGKICK at `a` sends, as the GIF reads it from data memory: each tag and its data (PACKED NLOOP x NREG
    /// quadwords, REGLIST half that rounded up, IMAGE NLOOP) up to the tag with EOP, addresses wrapping at the end of memory.
    pub fn packet(&self, a: u32) -> Result<Vec<[u32; 4]>, String> {
        let mut a = a as usize & 0x3ff;
        let mut out = Vec::new();
        loop {
            let t = self.mem[a];
            let nloop = (t[0] & 0x7fff) as usize;
            let nreg = match (t[1] >> 28) & 15 {
                0 => 16,
                n => n as usize,
            };
            let len = match (t[1] >> 26) & 3 {
                0 => nloop * nreg,
                1 => (nloop * nreg).div_ceil(2),
                _ => nloop,
            };
            out.extend((0..=len).map(|k| self.mem[(a + k) & 0x3ff]));
            if out.len() > 1024 {
                return Err(format!("VU1: the GIF packet an XGKICK sent from {a:#x} reads past all of data memory without a tag with EOP"));
            }
            a = (a + len + 1) & 0x3ff;
            if t[0] >> 15 & 1 != 0 {
                return Ok(out);
            }
        }
    }

    /// A VI write: vi00 stays 0, vi01 to vi15 keep 16 bits.
    #[inline(always)]
    pub fn set_vi(&mut self, i: usize, v: u32) {
        if i != 0 {
            self.vi[i] = if i < 16 { v & 0xffff } else { v };
        }
    }
}

/// A translated VU1 program: its microcode from micro address 0 (the program a run starts is the one whose code micro memory
/// holds), its entries and its code.
pub struct Prog {
    pub name: &'static str,
    pub code: &'static [u8],
    /// The native code's node for a start at a micro address (MSCAL's entry, or where the last run ended for MSCNT).
    pub entry: fn(usize) -> Option<u32>,
    /// Runs from a node to the E bit: the micro address of the E bit's delay slot.
    pub run: fn(&mut Vu1, u32) -> usize,
}

/// What a run keeps besides VU1's state, handed from one of its program's functions to the next (`drive`): Q and P on their way
/// (`qp`, `pv`), a VI register's value before the pair before a branch (`hv`), the branch taken (`tk`, `btk`), a JR's target, the
/// MAC results in flight and the newest (`ms`, `mlast`, `mso` their union, `macs` whether any was made), the pairs run, the CLIP
/// judgements in flight (`cs`).
#[derive(Clone, Copy)]
pub struct Run {
    pub qp: u32,
    pub pv: u32,
    pub hv: u32,
    pub tk: bool,
    pub btk: bool,
    pub jrt: usize,
    pub ms: [u32; 8],
    pub mlast: u32,
    pub mso: u32,
    pub macs: bool,
    pub pairs: u64,
    pub cs: [u32; 8],
}

/// One function of a translated program: its nodes from `n` on, until the run leaves them (the node it goes to) or ends (0x80000000
/// with the address of the E bit's delay slot).
pub type Chunk = fn(&mut Vu1, &mut Run, u32) -> u32;

/// A run of a translated program from node `n` (its functions `chunks`, each `per` nodes) to its E bit: the address of the E bit's
/// delay slot. What is in flight lands as the run ends.
pub fn drive(c: &mut Vu1, mut n: u32, chunks: &[Chunk], per: u32) -> usize {
    let mut r = Run { qp: c.vi[22], pv: c.vi[23], hv: 0, tk: false, btk: false, jrt: 0, ms: [0; 8], mlast: 0, mso: 0, macs: false, pairs: 0, cs: [0; 8] };
    while n & 0x8000_0000 == 0 {
        n = chunks[(n / per) as usize](c, &mut r, n);
    }
    c.vi[22] = r.qp;
    c.vi[23] = r.pv;
    if r.macs {
        c.vi[17] = r.mlast;
        let st = ((r.mlast & 0xf != 0) as u32) | (((r.mlast & 0xf0 != 0) as u32) << 1);
        let so = ((r.mso & 0xf != 0) as u32) | (((r.mso & 0xf0 != 0) as u32) << 1);
        c.vi[16] = (c.vi[16] & !0xf) | st | (so << 6);
    }
    (n & 0x7fff_ffff) as usize
}

/// CLIP fs.xyz, ft.w: the six judgements (+x, -x, +y, -y, +z, -z in bits 0 to 5) of fs against |ft.w|.
#[inline(always)]
pub fn m_clip(s: [u32; 4], w: u32) -> u32 {
    let w = uycpu::fpu::fin(w).abs();
    let mut f = 0;
    for l in 0..3 {
        let v = uycpu::fpu::fin(s[l]);
        if v > w {
            f |= 1 << (2 * l);
        }
        if v < -w {
            f |= 2 << (2 * l);
        }
    }
    f
}

/// ESUM: P = ((x + y) + z) + w.
#[inline(always)]
pub fn m_esum(v: [u32; 4]) -> u32 {
    use uycpu::fpu::add;
    add(add(add(v[0], v[1]), v[2]), v[3])
}

/// ESADD: P = (x x + y y) + z z.
#[inline(always)]
pub fn m_esadd(v: [u32; 4]) -> u32 {
    use uycpu::fpu::{add, mul};
    add(add(mul(v[0], v[0]), mul(v[1], v[1])), mul(v[2], v[2]))
}

/// DIV (0), SQRT (1), RSQRT (2): the value Q gets when it lands; the status flag's D and I (and their sticky bits) are set now.
#[inline(always)]
pub fn m_q<const K: u8>(c: &mut Vu1, a: u32, b: u32) -> u32 {
    let (x, y) = (uycpu::fpu::fin(a), uycpu::fpu::fin(b));
    let maxv = |s: u32| (s & 0x8000_0000) | 0x7f7f_ffff;
    match K {
        0 => {
            if y == 0.0 {
                c.vi[16] |= if x == 0.0 { 0x410 } else { 0x820 };
                maxv(a ^ b)
            } else {
                (x / y).to_bits()
            }
        }
        1 => {
            if y < 0.0 {
                c.vi[16] |= 0x410;
            }
            if y == 0.0 { b & 0x8000_0000 } else { y.abs().sqrt().to_bits() }
        }
        _ => {
            if y == 0.0 {
                c.vi[16] |= 0x820;
                maxv(a ^ b)
            } else {
                if y < 0.0 {
                    c.vi[16] |= 0x410;
                }
                (x / y.abs().sqrt()).to_bits()
            }
        }
    }
}
