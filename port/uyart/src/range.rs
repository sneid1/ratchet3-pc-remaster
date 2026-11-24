//! The port's own ranges, applied by the native culls as they read the game's values (port/uya `range.rs` says what each is
//! and why): the ties at their finest LOD and out to the level's limit, the props with their near meshes at every distance,
//! and the shrubs out to DRAW DISTANCE FAR's reach. The port sets them before each field (`set`); nothing is written into the
//! game's memory.
//!
//! They were written into the game's memory before each field (the classes' LOD distances, the instances' distance bytes, the
//! props' near distances, the level's shrub limit), at the addresses the level's globals named. On level 35 (R3-034) the tie
//! globals still named an array (0x152F880, 587 instances) in the buffer its next WAD streams into: the sound module's read
//! landed there at a vertical blank, the LZ unpacker (0x40BD20) was cut off by the field's end part way through it, and the
//! port's write before the next field raised 220 bytes of its unread stream to 60, so the unpacker met an invalid token and
//! trapped (0x40BE78). Applied where the cull reads each value, a range acts only on what the game itself is culling.
//!
//! Where each is applied (each the one read of its value: the renderer reads only whether a distance byte is 0, which no range
//! changes, and the LOD the cull chose, +0x1F):
//! - `tiecull` (0x4AFA28): the class's LOD distances (`lqc2 $vf16, 0x10($s0)`) and the instance's distance byte (`lbu $v0,
//!   0x3C($s6)`, the next instance's +0x1C) at 0x4AFCE0, before its PMINW with the level's limit;
//! - `mobydraw` (0x43EA78): the moby's near distance (`lbu $at, 0x62($fp)`) before the near and far meshes' test at 0x43F128;
//! - `shrubcull` (0x4A2200): the level's shrub limit (gp+0x45A8 on Veldin) at +0x44; the renderer's own shrub cull takes the
//!   same reach (port/uya `objects::Shrubs::visible`).
//!
//! A check of the tie cull or the moby draw path (UYA_NATIVE_CHECK) runs the translation with the values written into RAM as
//! they were (`for_translation`) and taken back after it (`translation_done`), so the native unit, which applies them as it
//! reads, is compared with the game's own code reading them from memory.

use crate::Cpu;
use std::collections::HashSet;
use std::sync::{Arc, Mutex, RwLock};

/// The LOD distances every tie class is culled with: past any depth the level's limit lets the cull list a tie at.
pub const LOD_DISTANCES: [f32; 3] = [65536.0, 131072.0, 196608.0];

/// A moby instance's size in the level's array.
const MOBY: u32 = 0x100;

/// The ranges for the next field. `None` fields are the game's own.
#[derive(Default)]
pub struct Ranges {
    /// The tie cull's: every class at its finest LOD, every instance's distance byte that is not 0 at least the level's limit.
    pub ties: Option<Ties>,
    /// The props' near distance, 255.
    pub props: Option<Props>,
    /// DRAW DISTANCE FAR's reach in world units: the level's shrub limit at least that.
    pub shrub_reach: Option<f32>,
}

/// The level's tie globals' addresses (`uyafmt::levelcode::TieAddrs`), for a check's translation run (`for_translation`).
pub struct Ties {
    pub class_count: u32,
    pub classes: u32,
    pub count: u32,
    pub instances: u32,
    pub limit: u32,
}

/// The level's moby array (the addresses of its start and end words) and its props: the classes in its class table with far
/// meshes (+5) and no joints (+8).
pub struct Props {
    pub mobies: u32,
    pub moby_end: u32,
    pub classes: HashSet<u32>,
}

static RANGES: RwLock<Option<Arc<Ranges>>> = RwLock::new(None);

/// The ranges from the next cull on.
pub fn set(r: Ranges) {
    *RANGES.write().expect("the ranges") = Some(Arc::new(r));
}

/// The ranges as set, the game's own before the first `set`.
pub(crate) fn get() -> Arc<Ranges> {
    RANGES.read().expect("the ranges").clone().unwrap_or_default()
}

/// The tie ranges for one call of the tie cull, from the level's globals as they stand: what they apply to is what the port
/// wrote, the level's instances (`instances` to `end`) and the classes of its class table.
pub(crate) struct TieView {
    /// The level's limit word (units).
    limit: u32,
    instances: u32,
    end: u32,
    classes: HashSet<u32>,
}

impl TieView {
    /// None while the level has no classes or no instances (nothing was written then).
    pub(crate) fn read(c: &mut Cpu, t: &Ties) -> Option<TieView> {
        // The class table is at `classes` itself; the instances' array at the word `instances` holds.
        let (count, table, n, instances) = (c.rd32(t.class_count), t.classes, c.rd32(t.count), c.rd32(t.instances));
        if count == 0 || n == 0 {
            return None;
        }
        let classes = (0..count).map(|k| c.rd32(table + 4 * k)).collect();
        Some(TieView { limit: c.rd32(t.limit), instances, end: instances + 32 * n, classes })
    }

    /// The distance byte read at `a`: an instance's (+0x1C) that is not 0 at least the level's limit in units over 16, rounded
    /// up, at most 255.
    pub(crate) fn byte(&self, a: u32, byte: u64) -> u64 {
        if a < self.instances || a >= self.end || (a - self.instances) % 32 != 0x1c {
            return byte;
        }
        let limit = u64::from(self.limit.div_ceil(16).min(255));
        if byte != 0 && byte < limit { limit } else { byte }
    }

    /// Whether `class` is culled with `LOD_DISTANCES`.
    pub(crate) fn lods(&self, class: u32) -> bool {
        self.classes.contains(&class)
    }
}

/// Whether moby `m` is a prop (`Props`) the draw loop does not skip, in the level's array: its near distance is 255.
pub(crate) fn prop(c: &mut Cpu, p: &Props, m: u32) -> bool {
    let (start, end) = (c.rd32(p.mobies), c.rd32(p.moby_end));
    m >= start && m < end && (m - start) % MOBY == 0 && c.rd8(m + 0x20) < 0x80 && c.rd16(m + 0x34) & 0x81 == 0 && p.classes.contains(&c.rd32(m + 0x24))
}

/// Which unit's values a check writes for its translation run.
#[derive(Clone, Copy)]
pub(crate) enum Unit {
    Ties,
    Props,
}

/// The bytes a check's translation run has written (address, the game's byte, the range's), by `Unit`.
static WRITTEN: Mutex<[Vec<(u32, u8, u8)>; 2]> = Mutex::new([Vec::new(), Vec::new()]);
/// By `Unit`: the checked calls whose translation read bytes the ranges changed, and those bytes in all.
static TOTALS: Mutex<[(u64, u64); 2]> = Mutex::new([(0, 0); 2]);

/// "; the port's ranges: N calls' translations read M bytes they changed", for a check's summary.
pub(crate) fn written(u: Unit) -> String {
    let (calls, bytes) = TOTALS.lock().expect("the check's writes")[u as usize];
    format!("; the port's ranges: {calls} calls' translations read {bytes} bytes they changed")
}

fn write(c: &mut Cpu, w: &mut Vec<(u32, u8, u8)>, a: u32, bytes: &[u8]) {
    for (k, &b) in bytes.iter().enumerate() {
        let a = a + k as u32;
        let was = c.rd8(a);
        if was != b {
            w.push((a, was, b));
            c.wr8(a, b);
        }
    }
}

/// Before a checked call's translation runs: unit `u`'s values written where the game reads them, as the port wrote them
/// before each field (the classes' LOD distances and the instances' distance bytes; the props' near distances).
pub(crate) fn for_translation(c: &mut Cpu, u: Unit) {
    let r = get();
    let mut w = Vec::new();
    match u {
        Unit::Ties => {
            if let Some(v) = r.ties.as_ref().and_then(|t| TieView::read(c, t)) {
                let lod: Vec<u8> = LOD_DISTANCES.iter().flat_map(|d| d.to_le_bytes()).collect();
                for &class in &v.classes {
                    write(c, &mut w, class + 0x10, &lod);
                }
                let mut a = v.instances + 0x1c;
                while a < v.end {
                    let b = v.byte(a, u64::from(c.rd8(a))) as u8;
                    write(c, &mut w, a, &[b]);
                    a += 32;
                }
            }
        }
        Unit::Props => {
            if let Some(p) = &r.props {
                let (start, end) = (c.rd32(p.mobies), c.rd32(p.moby_end));
                let mut m = start;
                while m < end {
                    if prop(c, p, m) {
                        write(c, &mut w, m + 0x62, &[255]);
                    }
                    m += MOBY;
                }
            }
        }
    }
    if !w.is_empty() {
        let mut t = TOTALS.lock().expect("the check's writes");
        t[u as usize].0 += 1;
        t[u as usize].1 += w.len() as u64;
    }
    WRITTEN.lock().expect("the check's writes")[u as usize] = w;
}

/// After a checked call's translation: the game's own bytes back where `for_translation` wrote, so the translation's state is
/// compared with the native unit's (which never writes them) and the game goes on with its own.
pub(crate) fn translation_done(c: &mut Cpu, u: Unit) {
    let w = std::mem::take(&mut WRITTEN.lock().expect("the check's writes")[u as usize]);
    // Last write first: a byte written twice gets the game's own back.
    for (a, was, ours) in w.into_iter().rev() {
        let now = c.rd8(a);
        if now != ours {
            panic!("UYA_NATIVE_CHECK: the translation wrote {now:#04x} at {a:08x}, where the check had written the port's range {ours:#04x} over the game's {was:#04x}");
        }
        c.wr8(a, was);
    }
}
