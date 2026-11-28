//! The hardware registers the game's code touches, answered by native code for what each was for. Nothing here keeps time in
//! cycles or models a unit's pipeline:
//!
//! - A DMA kick does its transfer at once. VIF1 and GIF path 3 chains are walked as the DMA walks them (the tags, the TTE words,
//!   REF data in RAM or scratchpad) and handed to the renderer as jobs (`Job`), which draws them (a frame's draw lists with
//!   their blocks only, their words read from the frame's RAM copy off the game thread: `Chain`); VIF0's codes put data and
//!   microprograms into VU0's memories; the scratchpad transfers copy. The channel then reads as ended, and its DMAC interrupt is
//!   raised.
//! - INTC and DMAC status and masks; the vertical blank's bits (raised by the frame loop).
//! - The timers count from the host's clock; COP0 Count too.
//! - The GS's privileged registers keep what the game writes (DISPFB goes with each job); CSR reports drawing done.
//!
//! A register outside these stops the game with its address.

use crate::{Cpu, Ext, Machine};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// One job for the renderer, in kick order: a VIF1 chain (its words in DMA order with the TTE words, a mark per block start
/// (phase 0) and per TTE pair (phase 2), and per block (first data word, EE address with bit 31 for scratchpad, quadwords, tag
/// id; 99 a normal transfer)), or a GIF path 3 transfer's quadwords; each with the DISPFB being shown.
/// The original skin program's output for a field, captured before another draw reuses its buffers. Each input record keeps
/// its RAM header and mesh entries; each DMA8 output keeps the resulting packed positions and normals tapped from VU0.
/// Legacy v3 saved frames instead contain a 0x1C00-byte joint-matrix prefix per record and the trig table, which the renderer
/// can reconstruct offline. Live fields omit these redundant inputs and consume the original outputs directly.
#[derive(Default)]
pub struct SkinJoints {
    pub records: Vec<(u32, u32, usize)>,
    pub bytes: Vec<u8>,
    pub trig: Vec<u8>,
    /// Live jobs have the original skin outputs. Version 3 saved frames only have inputs.
    pub captured: bool,
    pub outputs: Vec<SkinOutput>,
}

pub struct SkinOutput {
    pub record: usize,
    pub dest: u32,
    pub bytes: Vec<u8>,
    pub normals: Vec<[f32; 3]>,
}

impl SkinJoints {
    /// A vblank can interrupt the EE between two mesh DMAs of one skin job. Its
    /// remaining SPR_FROM transfers still belong to the same input record.
    /// Keep that input with the EE until it has sent all of its outputs.
    pub(crate) fn take_field(&mut self, interrupted: bool) -> Self {
        let mut field = std::mem::take(self);
        if !interrupted || !field.captured || field.records.is_empty() {
            return field;
        }
        let last = field.records.len() - 1;
        let primary = |i: usize| {
            let (src, qwc, at) = field.records[i];
            let data = &field.bytes[at..at + 0x40 + 16 * qwc as usize];
            let count = u32::from_le_bytes(data[..4].try_into().unwrap()) as usize;
            // The primary transfer holds the lights and count mesh entries. A
            // nonzero high half at +0x14 names a second group immediately after it.
            (matches!(count, 1..=200) && qwc as usize == 9 + count / 2)
                .then(|| (count, u16::from_le_bytes([data[0x16], data[0x17]]) != 0, src + 16 * qwc))
        };
        let (src, qwc, at) = field.records[last];
        let sent = field.outputs.iter().rev().take_while(|o| o.record == last).count();
        let keep = if last > 0 && primary(last - 1).is_some_and(|(_, second, next)| second && next == src) {
            // Group two has eight-byte entries at +0x80, ending at a zero pair.
            let data = &field.bytes[at..at + 0x40 + 16 * qwc as usize];
            let count = data.get(0x80..).and_then(|d| d.chunks_exact(8).position(|e| e == [0; 8]));
            count.filter(|&n| sent < n).map(|_| last - 1)
        } else {
            primary(last).and_then(|(count, second, _)| (sent < count || second).then_some(last))
        };
        if let Some(first) = keep {
            let offset = field.records[first].2;
            self.records = field.records.split_off(first);
            self.bytes = field.bytes.split_off(offset);
            self.outputs = field.outputs.split_off(field.outputs.partition_point(|o| o.record < first));
            self.captured = true;
            for (_, _, at) in &mut self.records { *at -= offset; }
            for output in &mut self.outputs { output.record -= first; }
        }
        field
    }
}

#[cfg(test)]
mod skin_tests {
    use super::*;

    #[test]
    fn interrupted_skin_keeps_only_the_unfinished_job() {
        let mut skin = SkinJoints { captured: true, ..Default::default() };
        let mut primary = |src, count: u32, second| {
            let qwc = 9 + count / 2;
            let at = skin.bytes.len();
            skin.bytes.resize(at + 64 + 16 * qwc as usize, 0);
            skin.bytes[at..at + 4].copy_from_slice(&count.to_le_bytes());
            skin.bytes[at + 0x16] = u8::from(second);
            skin.records.push((src, qwc, at));
        };
        primary(0x1000, 1, false);
        primary(0x2000, 2, true);
        let output = |record, dest| SkinOutput { record, dest, bytes: vec![], normals: vec![] };
        skin.outputs.extend([output(0, 0x4000), output(1, 0x5000)]);
        let done = skin.take_field(true);
        assert_eq!(done.records.len(), 1);
        assert_eq!(done.outputs[0].dest, 0x4000);
        assert_eq!(skin.records, [(0x2000, 10, 0)]);
        assert_eq!((skin.outputs[0].record, skin.outputs[0].dest), (0, 0x5000));

        skin.outputs.push(output(0, 0x5100));
        assert!(skin.take_field(true).records.is_empty());
        let at = skin.bytes.len();
        skin.records.push((0x20a0, 6, at));
        skin.bytes.resize(at + 64 + 6 * 16, 0);
        skin.bytes[at + 0x80] = 1;
        assert!(skin.take_field(true).records.is_empty());
        skin.outputs.push(output(1, 0x6000));
        let done = skin.take_field(true);
        assert_eq!(done.records.len(), 2);
        assert_eq!(done.outputs.len(), 3);
        assert!(skin.records.is_empty());
    }
}

/// Called only at the normal-producing instructions in the translated skin program.
pub(crate) fn skin_normal(c: &mut Cpu, rigid: bool) {
    if !crate::SKIN_JOINTS.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // The rigid loop evaluates one zero look-ahead row while storing its last real row.
    if rigid && c.r[2] as u32 >= c.r[25] as u32 {
        return;
    }
    let v = c.vf[if rigid { 4 } else { 15 }];
    c.mach().hw.skin_normals.push([f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2])]);
}

/// One job for the renderer, in kick order: a VIF1 chain (its words in DMA order with the TTE words, a mark per block start
/// (phase 0) and per TTE pair (phase 2), and per block (first data word, EE address with bit 31 for scratchpad, quadwords, tag
/// id; 99 a normal transfer)), or a VIF1 chain whose words are read from the frame's RAM copy when the renderer takes it
/// (`Chain`), or a GIF path 3 transfer's quadwords; each with the DISPFB being shown.
pub enum Job {
    Vif1 { words: Vec<u32>, marks: Vec<(usize, u32)>, blocks: Vec<(usize, u32, u32, u32)>, dispfb: u64 },
    Gif { qs: Vec<u128>, dispfb: u64 },
    Chain(Chain),
}

/// A VIF1 job's words, marks and blocks (`Job::Vif1`).
type Vif1Words = (Vec<u32>, Vec<(usize, u32)>, Vec<(usize, u32, u32, u32)>);

/// A tag's block of a VIF1 chain as the DMA sent it (`chain`): its tag's two VIF words (with CHCR's TTE), its data's address (bit
/// 31: the scratchpad), quadwords and tag id.
#[derive(Clone, Copy)]
pub(crate) struct Block {
    tte: Option<[u32; 2]>,
    data: u32,
    q: u32,
    id: u32,
}

/// How far each frame's own chain words reach (the tags and the data inline after them: CNT, NEXT, CALL, RET and END tags, not
/// the data a REF points to elsewhere), from the frame's first VIF1 kick on, in RAM: the game builds a frame's packets in one
/// of its two frame packet buffers (`tiebudget::BUFFER` bytes each) and nothing in it holds them to the buffer's end. Each
/// frame's reach goes to the tie ranges' room (`tiebudget::Costs::frame`); with UYA_PACKET_LOG a frame that reaches farther than
/// any before is logged, with its tie pass's share, and one past a buffer's size at once.
#[derive(Default)]
struct Packets {
    /// UYA_PACKET_LOG=N: every Nth frame logged too (120 when it names none).
    log: Option<Option<u64>>,
    frame: Option<(u32, u32)>,
    most: u32,
    frames: u64,
}

impl Packets {
    fn log(&mut self) -> Option<u64> {
        *self.log.get_or_insert_with(|| std::env::var("UYA_PACKET_LOG").ok().map(|v| v.parse().unwrap_or(120).max(1)))
    }

    /// A frame's first kick: the frame before ends (logged when it reached farther than any before, and every Nth).
    fn begin(&mut self, field: u64, costs: &mut crate::tiebudget::Costs) {
        let Some((lo, hi)) = self.frame.take() else { return };
        costs.frame(lo, hi);
        self.frames += 1;
        if let Some(every) = self.log()
            && (hi - lo > self.most || self.frames % every == 0)
        {
            self.most = self.most.max(hi - lo);
            println!("packets: field {field}: the frame before's chain words from {lo:#x} to {hi:#x}, {} KB (a frame packet buffer holds {} KB){}", (hi - lo) / 1024, crate::tiebudget::BUFFER / 1024, costs.split(lo, hi));
        }
    }

    /// The kick's blocks: their words in RAM, when they are the chain's own.
    fn blocks(&mut self, field: u64, blocks: &[Block]) {
        for b in blocks.iter().filter(|b| matches!(b.id, 1 | 2 | 5 | 6 | 7) && b.data & 0x8000_0000 == 0) {
            let (lo, hi) = (b.data - 16, b.data + 16 * b.q);
            let f = self.frame.get_or_insert((lo, hi));
            let was = f.1 - f.0;
            *f = (f.0.min(lo), f.1.max(hi));
            if was <= crate::tiebudget::BUFFER && f.1 - f.0 > crate::tiebudget::BUFFER && self.log.flatten().is_some() {
                println!("packets: field {field}: a frame's chain words from {:#x} to {:#x}, {} KB, past a frame packet buffer's {} KB", f.0, f.1, (f.1 - f.0) / 1024, crate::tiebudget::BUFFER / 1024);
            }
        }
    }
}

/// A VIF1 chain whose words the renderer reads from the frame's RAM copy (`Machine::frame_ram`) as it takes the frame
/// (`build`), off the game thread. The copy holds what the kick read: a thread's kick reads the RAM right after the copy is
/// made, and a kick its VIF1 end handler makes inside it (`Burst`) reads it after only that handler ran, which stores nothing
/// but its flag word and D1_CHCR. The chain's blocks are kept in the copy (`RamCopy::chains`), with the scratchpad as it stood
/// (`RamCopy::spr`).
pub struct Chain {
    /// The copy's RAM and generation at the kick (`RamCopy::key`).
    copy: (u64, u64),
    /// MADR and QWC: the normal transfer before the tags.
    head: (u32, u32),
    blocks: std::ops::Range<usize>,
    dispfb: u64,
    /// The field and the job's place in it, for UYA_KICK_CHECK's report, and its words as copied at the kick, to compare.
    field: u64,
    job: usize,
    check: Option<Box<Vif1Words>>,
}

impl Chain {
    /// The chain's words, marks and blocks, and its DISPFB, as `Job::Vif1` holds them: read from `ram`, the copy it was kicked
    /// with (another copy, or this one brought up to date since, is an error).
    pub fn build(&self, ram: &crate::RamCopy) -> Result<(Vec<u32>, Vec<(usize, u32)>, Vec<(usize, u32, u32, u32)>, u64), String> {
        if ram.key() != self.copy {
            return Err(format!("field {} job {}: a VIF1 chain kicked with the RAM copy {:?} built with the copy {:?}", self.field, self.job, self.copy, ram.key()));
        }
        let (madr, qwc) = self.head;
        let blocks = ram.chains.get(self.blocks.clone()).ok_or_else(|| format!("field {} job {}: the RAM copy holds {} chain blocks, not {:?}", self.field, self.job, ram.chains.len(), self.blocks))?;
        if ram.spr.len() < 0x4000 && (touches_spr(madr, qwc) || blocks.iter().any(|b| touches_spr(b.data, b.q))) {
            return Err(format!("field {} job {}: a VIF1 chain reads the scratchpad and its RAM copy has none", self.field, self.job));
        }
        let built = assemble(ram, &ram.spr, madr, qwc, blocks);
        if let Some(kick) = &self.check {
            check_chain(self.field, self.job, kick, &built);
        }
        let (words, marks, blocks) = built;
        Ok((words, marks, blocks, self.dispfb))
    }
}

/// UYA_KICK_CHECK=1: every chain read from the frame's copy is also copied at its kick, and the two are compared when it is built
/// (`Chain::build`); every VIF1 kick's GS events are found both from its stream where it lies (`vif1_events`) and from its words
/// as copied (`vif1_events_words`, the way before), and compared. The counts: chains compared, those that differed; kicks
/// compared, those that differed.
static CHECKS: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

fn checking() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("UYA_KICK_CHECK").is_ok_and(|v| v == "1"))
}

/// UYA_KICK_CHECK's counts so far (`CHECKS`); None when it is off.
pub fn kick_check() -> Option<[u64; 4]> {
    checking().then(|| CHECKS.each_ref().map(|a| a.load(Ordering::Relaxed)))
}

/// One comparison of kind `k` (0 chains, 2 GS events) counted; whether to print its differences (the first 20 of the kind).
fn checked(k: usize, differ: bool) -> bool {
    CHECKS[k].fetch_add(1, Ordering::Relaxed);
    differ && CHECKS[k + 1].fetch_add(1, Ordering::Relaxed) < 20
}

/// UYA_KICK_CHECK: a chain built from the frame's copy against its words as copied at the kick; the first differing word, by
/// block.
fn check_chain(field: u64, job: usize, kick: &Vif1Words, copy: &Vif1Words) {
    let mut d = Vec::new();
    if kick.0 != copy.0 {
        let n = kick.0.len().min(copy.0.len());
        let differ = (0..n).filter(|&i| kick.0[i] != copy.0[i]).count();
        if let Some(i) = (0..n).find(|&i| kick.0[i] != copy.0[i]) {
            let at = match copy.2.partition_point(|b| b.0 <= i).checked_sub(1) {
                Some(b) if i - copy.2[b].0 < 4 * copy.2[b].2 as usize => format!("block {b} ({:08x}) word {}", copy.2[b].1, i - copy.2[b].0),
                Some(b) => format!("the tag's words after block {b}"),
                None => "the tag's words before the first block".to_string(),
            };
            d.push(format!("word {i} ({at}): {:08x} from the copy, {:08x} at the kick; {differ} words differ", copy.0[i], kick.0[i]));
        }
        if kick.0.len() != copy.0.len() {
            d.push(format!("{} words from the copy, {} at the kick", copy.0.len(), kick.0.len()));
        }
    }
    if kick.1 != copy.1 {
        d.push(format!("marks: {} from the copy, {} at the kick, the first differing {:?}", copy.1.len(), kick.1.len(), copy.1.iter().zip(&kick.1).position(|(a, b)| a != b)));
    }
    if kick.2 != copy.2 {
        d.push(format!("blocks: {} from the copy, {} at the kick, the first differing {:?}", copy.2.len(), kick.2.len(), copy.2.iter().zip(&kick.2).position(|(a, b)| a != b)));
    }
    if checked(0, !d.is_empty()) {
        println!("UYA_KICK_CHECK: field {field} job {job}: {}", d.join("; "));
    }
}

/// An image transfer into GS memory a job's GS packets make: BITBLTBUF, TRXPOS and TRXREG as the packets left them, the IMAGE
/// packet's quadwords, and the job it is in (its index in `Hw::jobs`). The renderer's 2D layer keeps GS memory from them for the
/// frames it does not draw itself (port/uya `front.rs`). Transfers by the EE's stores into the GIF FIFO, a quadword a job, are
/// not read here.
#[derive(Clone, PartialEq)]
pub struct GsUpload {
    pub job: usize,
    pub bitbltbuf: u64,
    pub trxpos: u64,
    pub trxreg: u64,
    pub data: Vec<u128>,
}

pub struct Hw {
    /// DMA channel registers by channel (0 VIF0 .. 9 SPR_TO): CHCR, MADR, QWC, TADR, ASR0, ASR1, -, -, SADR (by offset / 16).
    pub ch: [[u32; 9]; 10],
    pub d_ctrl: u32,
    pub d_stat: u32,
    pub d_pcr: u32,
    pub d_sqwc: u32,
    pub d_rbsr: u32,
    pub d_rbor: u32,
    pub d_stadr: u32,
    pub d_enable: u32,
    pub intc_stat: u32,
    pub intc_mask: u32,
    /// The four timers.
    timers: [Timer; 4],
    t0: Instant,
    /// GS privileged registers by offset / 16 (0x1200_0000 .. 0x1200_00F0) and CSR, IMR, BUSDIR, SIGLBLID.
    pub gs: [u64; 16],
    pub imr: u64,
    pub busdir: u64,
    /// VIF0's unpack state: CYCLE (CL, WL), MASK, MODE, ROW, COL, and its other registers by offset / 16.
    pub vif0: Vif0,
    /// GS CSR, SIGLBLID, the field's image transfers and the walk of the GS events that changes them (`gsstate`): reached only
    /// through its accessors, which wait for a walk still running on the worker thread.
    pub(crate) walk: gsstate::GsState,
    pub vif_regs: [[u32; 16]; 2],
    pub gif_regs: [u32; 16],
    pub sbus: [u32; 16],
    pub jobs: Vec<Job>,
    /// The skin job's joint matrices this field (`SkinJoints`).
    pub skin: SkinJoints,
    pub(crate) skin_normals: Vec<[f32; 3]>,
    /// The call sites of this field's VIF1 kicks made by a thread, and where its last frame starts in `jobs` (`kick`).
    vif1_sites: Vec<u32>,
    pub frame_from: usize,
    /// The frame's own chain words so far (`Packets`), and the level's tie costs measured from its frames (`tiebudget`).
    packets: Packets,
    pub(crate) tie_costs: crate::tiebudget::Costs,
    /// Whether a thread kicked a VIF1 list this field (a frame's first; `Machine::frame_ram` holds the RAM at the last).
    pub kicked: bool,
    /// The VIF1 end handler's burst under way (`Burst`).
    burst: Option<Burst>,
    /// The blocks of a VIF1 kick whose words are copied at the kick: kept from kick to kick.
    blocks: Vec<Block>,
    /// The debug serial port's line (SIO TX), printed whole.
    sio: Vec<u8>,
}

/// A thread's VIF1 kick whose end handler is a copy of the one every program has (`END_HANDLERS`) and runs alone: the kicks
/// that handler makes inside the thread's kick, the frame's other draw lists, read the RAM as the thread's kick copied it, but
/// for the handler's flag word (`flag`, its address).
#[derive(Clone, Copy)]
struct Burst {
    flag: u32,
}

/// An EE timer: it counts from `base` at the time `at` (when it was last written) at its clock's rate; `seen` is the count up to
/// which its events (reaching COMP, wrapping at 0x10000) have been raised.
#[derive(Clone, Copy)]
struct Timer {
    base: u64,
    at: Instant,
    mode: u32,
    comp: u32,
    hold: u32,
    seen: u64,
}

#[derive(Default, Clone, Copy)]
pub struct Vif0 {
    pub cl: u32,
    pub wl: u32,
    pub mask: u32,
    pub mode: u32,
    pub row: [u32; 4],
    pub col: [u32; 4],
    pub itop: u32,
}

impl Default for Hw {
    fn default() -> Hw {
        let now = Instant::now();
        Hw {
            ch: [[0; 9]; 10],
            d_ctrl: 0,
            d_stat: 0,
            d_pcr: 0,
            d_sqwc: 0,
            d_rbsr: 0,
            d_rbor: 0,
            d_stadr: 0,
            d_enable: 0x1201,
            intc_stat: 0,
            intc_mask: 0,
            timers: [Timer { base: 0, at: now, mode: 0, comp: 0, hold: 0, seen: 0 }; 4],
            t0: now,
            gs: [0; 16],
            imr: 0x7f00,
            busdir: 0,
            walk: gsstate::GsState::new(),
            vif0: Vif0 { cl: 1, wl: 1, ..Default::default() },
            vif_regs: [[0; 16]; 2],
            gif_regs: [0; 16],
            sbus: [0; 16],
            jobs: Vec::new(),
            skin: SkinJoints::default(),
            skin_normals: Vec::new(),
            vif1_sites: Vec::new(),
            frame_from: 0,
            packets: Packets::default(),
            tie_costs: crate::tiebudget::Costs::default(),
            kicked: false,
            burst: None,
            blocks: Vec::new(),
            sio: Vec::new(),
        }
    }
}

/// The EE's clock (294.912 MHz) and the bus clock (half of it), for Count and the timers.
const EE_HZ: f64 = 294_912_000.0;
const BUS_HZ: f64 = 147_456_000.0;
/// NTSC hsyncs a second.
const HSYNC_HZ: f64 = 15_734.264;

impl Hw {
    pub fn cop0_count(&self) -> u32 {
        (self.t0.elapsed().as_secs_f64() * EE_HZ) as u64 as u32
    }

    /// Timer i's ticks since it was last set (its whole count, before the 16-bit counter wraps or ZRET returns it to 0).
    fn timer_ticks(&self, i: usize) -> u64 {
        let t = &self.timers[i];
        if t.mode & 0x80 == 0 {
            return t.base;
        }
        let hz = match t.mode & 3 {
            0 => BUS_HZ,
            1 => BUS_HZ / 16.0,
            2 => BUS_HZ / 256.0,
            _ => HSYNC_HZ,
        };
        t.base + (t.at.elapsed().as_secs_f64() * hz) as u64
    }

    /// The counter as T_COUNT reads it: with ZRET (MODE bit 6) it returns to 0 on reaching COMP, else it wraps at 0x10000.
    fn timer_count(&self, i: usize) -> u32 {
        let t = &self.timers[i];
        let n = self.timer_ticks(i);
        if t.mode & 0x40 != 0 && t.comp != 0 { (n % (t.comp as u64 + 1)) as u32 } else { (n & 0xffff) as u32 }
    }

    /// The timers' events since the last look: each timer that reached COMP (with CMPE) or wrapped (with OVFE) since then gets
    /// its flag in MODE (EQUF, OVFF) and its INTC cause (9 + timer) is returned.
    pub fn timer_events(&mut self) -> Vec<u32> {
        let mut out = Vec::new();
        for i in 0..4 {
            let mode = self.timers[i].mode;
            if mode & 0x80 == 0 || mode & 0x300 == 0 {
                continue;
            }
            let now = self.timer_ticks(i);
            let t = &mut self.timers[i];
            let (a, b) = (t.seen, now);
            t.seen = now;
            let period = if t.mode & 0x40 != 0 && t.comp != 0 { t.comp as u64 + 1 } else { 0x1_0000 };
            // Reaching COMP: the count passes comp within a period; with ZRET that is every period.
            let hits = |x: u64| if t.mode & 0x40 != 0 { x / period } else { (x + 0x1_0000 - t.comp as u64) / 0x1_0000 };
            let compared = mode & 0x100 != 0 && hits(b) > hits(a);
            let wrapped = mode & 0x200 != 0 && mode & 0x40 == 0 && b / 0x1_0000 > a / 0x1_0000;
            if compared {
                t.mode |= 0x400;
            }
            if wrapped {
                t.mode |= 0x800;
            }
            if compared || wrapped {
                out.push(9 + i as u32);
            }
        }
        out
    }

    pub fn dmac_mask(&self) -> u32 {
        self.d_stat >> 16
    }

    /// _EnableIntc / _DisableIntc: whether the bit changed.
    pub fn set_intc_mask(&mut self, cause: u32, on: bool) -> bool {
        let old = self.intc_mask;
        if on {
            self.intc_mask |= 1 << cause;
        } else {
            self.intc_mask &= !(1 << cause);
        }
        old != self.intc_mask
    }
    pub fn set_dmac_mask(&mut self, ch: u32, on: bool) -> bool {
        let old = self.d_stat;
        if on {
            self.d_stat |= 1 << (16 + ch);
        } else {
            self.d_stat &= !(1 << (16 + ch));
        }
        old != self.d_stat
    }

    /// The vertical blank starts: INTC VBON, GS CSR VSINT, the field bit flips.
    pub fn vblank_start(&mut self) {
        self.intc_stat |= 1 << 2;
        let csr = self.walk.csr_mut();
        *csr |= 1 << 3;
        *csr ^= 1 << 13;
    }
    pub fn vblank_end(&mut self) {
        self.intc_stat |= 1 << 3;
    }

    /// A field begins: no kicks yet, its frame starts at its first job.
    pub fn field_start(&mut self) {
        self.vif1_sites.clear();
        self.frame_from = 0;
        self.kicked = false;
    }

    pub fn gs_csr(&mut self) -> u64 {
        self.walk.csr()
    }

    /// DISPFB of the circuit being shown (PMODE: circuit 2 alone uses DISPFB2).
    fn dispfb(&self) -> u64 {
        if self.gs[0] & 3 == 2 { self.gs[9] } else { self.gs[7] }
    }

    /// A job for the renderer. A field that has handed over a million has gone round a loop: the game stops with what sent it
    /// (`what`, said only then) and the return address of the code that did (`ra`).
    fn job(&mut self, j: Job, what: impl FnOnce() -> String, ra: u32) {
        if self.jobs.len() >= 1 << 20 {
            panic!("this field has handed the renderer {} jobs, the last from {} (ra {ra:08x}): a loop that does not end", self.jobs.len(), what());
        }
        self.jobs.push(j);
    }
}

const DMA_BASE: [u32; 10] = [0x8000, 0x9000, 0xa000, 0xb000, 0xb400, 0xc000, 0xc400, 0xc800, 0xd000, 0xd400];

fn channel(off: u32) -> Option<(usize, usize)> {
    DMA_BASE.iter().position(|&b| off >= b && off < b + 0x100).map(|c| (c, ((off - DMA_BASE[c]) / 0x10) as usize))
}

pub fn read(c: &mut Cpu, p: u32, n: u32) -> u64 {
    let at = At::of(c);
    let m = c.mach();
    let h = &mut m.hw;
    let v: u64 = match p {
        0x1000_0000..=0x1000_1fff => {
            let i = ((p >> 11) & 3) as usize;
            match p & 0x7ff {
                0x00 => h.timer_count(i) as u64,
                0x10 => h.timers[i].mode as u64,
                0x20 => h.timers[i].comp as u64,
                0x30 => h.timers[i].hold as u64,
                _ => unknown(p, n, "read", at),
            }
        }
        0x1000_3000..=0x1000_30ff => h.gif_regs[((p >> 4) & 15) as usize] as u64,
        0x1000_3800..=0x1000_38ff => h.vif_regs[0][((p >> 4) & 15) as usize] as u64,
        0x1000_3c00..=0x1000_3cff => h.vif_regs[1][((p >> 4) & 15) as usize] as u64,
        0x1000_8000..=0x1000_d4ff => match channel(p & 0xffff) {
            Some((ch, r)) if r < 9 => h.ch[ch][r] as u64,
            _ => unknown(p, n, "read", at),
        },
        0x1000_e000 => h.d_ctrl as u64,
        0x1000_e010 => h.d_stat as u64,
        0x1000_e020 => h.d_pcr as u64,
        0x1000_e030 => h.d_sqwc as u64,
        0x1000_e040 => h.d_rbsr as u64,
        0x1000_e050 => h.d_rbor as u64,
        0x1000_e060 => h.d_stadr as u64,
        0x1000_f000 => h.intc_stat as u64,
        0x1000_f010 => h.intc_mask as u64,
        0x1000_f200..=0x1000_f2ff => h.sbus[((p >> 4) & 15) as usize] as u64,
        0x1000_f520 => h.d_enable as u64,
        // The debug serial port: transmit always ready.
        0x1000_f100..=0x1000_f1ff => 0,
        0x1100_0000..=0x1100_ffff => {
            let (mem, o) = vu_mem(m, p);
            let mut v = 0u64;
            for k in 0..n as usize {
                v |= (mem[(o + k) % mem.len()] as u64) << (8 * k);
            }
            v
        }
        0x1200_0000..=0x1200_00ff => h.gs[((p >> 4) & 15) as usize],
        0x1200_1000 => h.walk.csr(),
        0x1200_1010 => h.imr,
        0x1200_1040 => h.busdir,
        0x1200_1080 => h.walk.siglblid(),
        // Past the scratchpad the space is unmapped: the console reads nothing, so does the port.
        0x0200_4000..=0x0fff_ffff => 0,
        _ => unknown(p, n, "read", at),
    };
    let sh = (p & 7) * 8;
    if n < 8 { (v >> sh) & ((1u64 << (8 * n)) - 1) } else { v }
}

fn vu_mem(m: &mut crate::Machine, p: u32) -> (&mut Vec<u8>, usize) {
    match p & 0xffff {
        0x0000..=0x3fff => (&mut m.vu0m, (p & 0xfff) as usize),
        0x4000..=0x7fff => (&mut m.vu0d, (p & 0xfff) as usize),
        0x8000..=0xbfff => (&mut m.vu1m, (p & 0x3fff) as usize),
        _ => (&mut m.vu1d, (p & 0x3fff) as usize),
    }
}

/// Where the game was at a hardware access: its return address register, the last return taken (`Cpu::ret`), its stack pointer, and
/// every GPR (low and high 64 bits: hand-written code keeps values in the high halves).
#[derive(Clone, Copy)]
struct At {
    ra: u32,
    ret: u32,
    sp: u32,
    r: [u64; 32],
    rh: [u64; 32],
}

impl At {
    fn of(c: &Cpu) -> At {
        At { ra: c.r[31] as u32, ret: c.ret, sp: c.r[29] as u32, r: c.r, rh: c.rh }
    }
}

#[cold]
fn unknown(p: u32, n: u32, what: &str, at: At) -> u64 {
    let regs: Vec<String> = (1..32).map(|k| format!("r{k} {:x}/{:x}", at.r[k] as u32, at.rh[k] as u32)).collect();
    panic!("hardware {what} of {n} bytes at {p:08x} (ra {:08x}, last return to {:08x}, sp {:08x}; low/high GPRs: {}): a register the port does not answer", at.ra, at.ret, at.sp, regs.join(" "))
}

pub fn read128(c: &mut Cpu, p: u32) -> (u64, u64) {
    match p {
        // VIF1's FIFO read back (a GS -> EE download): the port's GS has no picture in RAM to give.
        0x1000_5000 => (0, 0),
        0x1100_0000..=0x1100_ffff => (read(c, p, 8), read(c, p + 8, 8)),
        // The unmapped physical space from the scratchpad's end (0x0200_4000) to the devices (0x1000_0000): the console ignores what is written and
        // reads nothing, so does the port.
        0x0200_4000..=0x0fff_ffff => (0, 0),
        _ => (read(c, p, 8), read(c, p + 8, 8)),
    }
}

pub fn write128(c: &mut Cpu, p: u32, lo: u64, hi: u64) {
    match p {
        0x1100_0000..=0x1100_ffff => {
            write(c, p, 8, lo);
            write(c, p + 8, 8, hi);
        }
        // VIF1's FIFO (VIF codes by the EE's own stores, sceGsResetPath's among them): a job of one quadword for the renderer, as a
        // chain's words are.
        0x1000_5000 => {
            let (d, ra) = (c.mach().hw.dispfb(), c.r[31] as u32);
            let words = vec![lo as u32, (lo >> 32) as u32, hi as u32, (hi >> 32) as u32];
            c.mach().hw.job(Job::Vif1 { words, marks: vec![(0, 0)], blocks: vec![(0, p, 1, 99)], dispfb: d }, || "the EE's stores into VIF1's FIFO".into(), ra);
        }
        // VIF0's FIFO: its codes into VU0's memories.
        0x1000_4000 => {
            let words = [lo as u32, (lo >> 32) as u32, hi as u32, (hi >> 32) as u32];
            vif0(c, &words);
        }
        // The GIF FIFO (path 3 by the EE's own stores): handed on as a job of one quadword.
        0x1000_6000 => {
            let (d, ra) = (c.mach().hw.dispfb(), c.r[31] as u32);
            c.mach().hw.job(Job::Gif { qs: vec![(hi as u128) << 64 | lo as u128], dispfb: d }, || "the EE's stores into the GIF FIFO".into(), ra);
        }
        // The unmapped physical space between the scratchpad and the devices: the console ignores what is written there, so does the port.
        0x0200_4000..=0x0fff_ffff => {}
        _ => panic!("hardware 16-byte write at {p:08x} (ra {:08x}, sp {:08x}): a register the port does not answer", c.r[31] as u32, c.r[29] as u32),
    }
}

pub fn write(c: &mut Cpu, p: u32, n: u32, v: u64) {
    let at = At::of(c);
    let mp = c.mp();
    let m = c.mach();
    let w = v as u32;
    match p {
        0x1000_0000..=0x1000_1fff => {
            let i = ((p >> 11) & 3) as usize;
            let h = &mut m.hw;
            match p & 0x7ff {
                0x00 => {
                    let t = &mut h.timers[i];
                    (t.base, t.at, t.seen) = ((w & 0xffff) as u64, Instant::now(), (w & 0xffff) as u64);
                }
                // MODE: writing 1 to EQUF or OVFF clears it; the count goes on from where it is.
                0x10 => {
                    let n = h.timer_ticks(i);
                    let t = &mut h.timers[i];
                    let flags = t.mode & 0xc00 & !(w & 0xc00);
                    (t.base, t.at, t.seen) = (n, Instant::now(), n);
                    t.mode = (w & 0x3ff) | flags;
                }
                0x20 => h.timers[i].comp = w & 0xffff,
                0x30 => h.timers[i].hold = w & 0xffff,
                _ => {
                    unknown(p, n, "write", at);
                }
            }
        }
        0x1000_3000..=0x1000_30ff => m.hw.gif_regs[((p >> 4) & 15) as usize] = w,
        0x1000_3800..=0x1000_38ff => m.hw.vif_regs[0][((p >> 4) & 15) as usize] = w,
        0x1000_3c00..=0x1000_3cff => m.hw.vif_regs[1][((p >> 4) & 15) as usize] = w,
        0x1000_8000..=0x1000_d4ff => match channel(p & 0xffff) {
            Some((ch, r)) if r < 9 => {
                m.hw.ch[ch][r] = w;
                if r == 0 && w & 0x100 != 0 {
                    kick_timed(c, ch);
                }
            }
            _ => {
                unknown(p, n, "write", at);
            }
        },
        0x1000_e000 => m.hw.d_ctrl = w,
        // D_STAT: writing 1 clears a channel's end bit (0..15), toggles its mask bit (16..31).
        0x1000_e010 => m.hw.d_stat = (m.hw.d_stat & !(w & 0xffff)) ^ (w & 0xffff_0000),
        0x1000_e020 => m.hw.d_pcr = w,
        0x1000_e030 => m.hw.d_sqwc = w,
        0x1000_e040 => m.hw.d_rbsr = w,
        0x1000_e050 => m.hw.d_rbor = w,
        0x1000_e060 => m.hw.d_stadr = w,
        // I_STAT: writing 1 clears; I_MASK: writing 1 toggles.
        0x1000_f000 => m.hw.intc_stat &= !w,
        0x1000_f010 => m.hw.intc_mask ^= w,
        0x1000_f200..=0x1000_f2ff => m.hw.sbus[((p >> 4) & 15) as usize] = w,
        0x1000_f590 => m.hw.d_enable = w,
        0x1000_f180 => {
            let h = &mut m.hw;
            if w as u8 == b'\n' {
                println!("ee sio: {}", String::from_utf8_lossy(&h.sio));
                h.sio.clear();
            } else {
                h.sio.push(w as u8);
            }
        }
        0x1000_f100..=0x1000_f1ff => {}
        0x1100_0000..=0x1100_ffff => {
            let (mem, o) = vu_mem(m, p);
            for k in 0..n as usize {
                let l = mem.len();
                mem[(o + k) % l] = (v >> (8 * k)) as u8;
            }
            if p & 0xffff < 0x4000 {
                m.vu0_gen += 1;
            }
        }
        0x1200_0000..=0x1200_00ff => m.hw.gs[((p >> 4) & 15) as usize] = v,
        // CSR: writing 1 to SIGNAL, FINISH, HSINT, VSINT, EDWINT clears them; RESET clears the rest.
        0x1200_1000 => {
            let csr = m.hw.walk.csr_mut();
            *csr &= !(v & 0x1f);
            if v & 0x200 != 0 {
                *csr &= !0x1f;
            }
        }
        0x1200_1010 => m.hw.imr = v,
        0x1200_1040 => m.hw.busdir = v,
        0x1200_1080 => *m.hw.walk.siglblid_mut() = v,
        // The unmapped physical space between the scratchpad and the devices: the console ignores what is written there, so does the port.
        0x0200_4000..=0x0fff_ffff => {}
        _ => {
            unknown(p, n, "write", at);
        }
    }
    let _ = mp;
}

/// A kick, timed for the host's report (`Machine::prof`, `kicks`): its own work (the transfer, and what it hands the renderer)
/// apart from the interrupt handlers its channel's end runs (`kernel::raise`), which may kick again. For the outermost kick on
/// this host thread, the handlers' game code is the raise's time less the kicks inside it; a raise that passed the turn to
/// another thread (`machine::turns`) held that thread's time too, and is not counted.
fn kick_timed(c: &mut Cpu, ch: usize) {
    thread_local! {
        /// This host thread's kicks under way, and the ticks of those inside the outermost.
        static KICKING: std::cell::Cell<(u32, u64)> = const { std::cell::Cell::new((0, 0)) };
    }
    let depth = KICKING.with(|k| {
        let (d, inner) = k.get();
        k.set((d + 1, inner));
        d
    });
    let t0 = crate::machine::tsc();
    let began = kick(c, ch);
    let (t1, turns) = (crate::machine::tsc(), crate::machine::turns());
    crate::kernel::raise(c.mp(), Some(&mut *c), crate::kernel::Int::Dmac(ch as u32));
    let t2 = crate::machine::tsc();
    let m = c.mach();
    if began {
        m.hw.burst = None;
    }
    m.prof[2] += t1 - t0;
    m.kicks[ch].0 += 1;
    m.kicks[ch].1 += t1 - t0;
    let inner = KICKING.with(|k| {
        let (d, inner) = k.get();
        if depth == 0 {
            k.set((0, 0));
            inner
        } else {
            k.set((d - 1, inner + (t2 - t0)));
            0
        }
    });
    if depth == 0 && crate::machine::turns() == turns {
        m.prof[4] += (t2 - t1).saturating_sub(inner);
    }
}

/// A quadword of DMA source: RAM, or scratchpad when bit 31 is set.
fn qw(m: &Machine, a: u32) -> u128 {
    qw_in(&m.ram, &m.spr, a)
}

/// A quadword of DMA source in `ram` or (bit 31) `spr`: the machine's (`qw`), or a frame's copy of them (`Chain::build`).
fn qw_in(ram: &[u8], spr: &[u8], a: u32) -> u128 {
    if a & 0x8000_0000 != 0 {
        let o = (a & 0x3ff0) as usize;
        u128::from_le_bytes(spr[o..o + 16].try_into().unwrap())
    } else {
        let o = (a & 0x01ff_fff0) as usize;
        u128::from_le_bytes(ram[o..o + 16].try_into().unwrap())
    }
}

/// Word `o` of the data the DMA reads from `a` as `words_in` gives it: of its quadword `o / 4` (`qw_in`). Within RAM and not
/// wrapping, `words_in`'s one copy holds the same bytes: (a + 16 k) & 0x01FFFFF0 is (a & 0x01FFFFF0) + 16 k there.
fn word_in(ram: &[u8], spr: &[u8], a: u32, o: usize) -> u32 {
    let q = a.wrapping_add(16 * (o / 4) as u32);
    let b = 4 * (o % 4);
    let at = if q & 0x8000_0000 != 0 { &spr[(q & 0x3ff0) as usize + b..] } else { &ram[(q & 0x01ff_fff0) as usize + b..] };
    u32::from_le_bytes([at[0], at[1], at[2], at[3]])
}

fn words(m: &Machine, a: u32, q: u32, out: &mut Vec<u32>) {
    words_in(&m.ram, &m.spr, a, q, out)
}

fn words_in(ram: &[u8], spr: &[u8], a: u32, q: u32, out: &mut Vec<u32>) {
    // From RAM, not wrapping past its end: the words as they lie (read a quadword at a time, and each GIF block of the chain
    // scanned from a vector of its own, Veldin's kicks took 6.4 ms a field with every tie drawn at its finest, `range.rs`; 5.9).
    let o = (a & 0x01ff_fff0) as usize;
    if a & 0x8000_0000 == 0 && o + 16 * q as usize <= crate::RAM_SIZE as usize {
        out.extend(ram[o..o + 16 * q as usize].chunks_exact(4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])));
        return;
    }
    for k in 0..q {
        let v = qw_in(ram, spr, a.wrapping_add(16 * k));
        out.extend([v as u32, (v >> 32) as u32, (v >> 64) as u32, (v >> 96) as u32]);
    }
}

/// Whether the quadwords the DMA reads from `a` (`q` of them, `qw_in`) reach the scratchpad (bit 31).
fn touches_spr(a: u32, q: u32) -> bool {
    q > 0 && (a & 0x8000_0000 != 0 || a as u64 + 16 * (q as u64 - 1) >= 0x8000_0000)
}

/// Whether the quadwords the DMA reads from `a` (`q` of them) hold RAM's word at `w`: those reaching the scratchpad or wrapping
/// at RAM's end are taken to.
fn holds(a: u32, q: u32, w: u32) -> bool {
    let o = (a & 0x01ff_fff0) as u64;
    if q == 0 {
        return false;
    }
    if touches_spr(a, q) || o + 16 * q as u64 > crate::RAM_SIZE as u64 {
        return true;
    }
    (o..o + 16 * q as u64).contains(&((w & 0x01ff_fff0) as u64))
}

/// A VIF1 kick's job as the renderer reads it (`Job::Vif1`), from `ram` and `spr` (the machine's at the kick, or the frame's copy
/// of them, `Chain::build`): the normal transfer's words from MADR (`qwc` quadwords), then per block its tag's words and its
/// data's; a mark at the start (phase 0), at each tag's words (phase 2) and at each block's data (phase 0); per block its first
/// data word, address, quadwords and tag id (99 the normal transfer, when it sends any).
fn assemble(ram: &[u8], spr: &[u8], madr: u32, qwc: u32, tagged: &[Block]) -> Vif1Words {
    // The words into a vector made once at their size: grown block by block, its reallocations were half of the copy
    // (Metropolis: 955 of 1809 samples of `words`).
    let total = 4 * qwc as usize + tagged.iter().map(|t| 2 * t.tte.is_some() as usize + 4 * t.q as usize).sum::<usize>();
    let mut words_v = Vec::with_capacity(total);
    let mut marks = Vec::with_capacity(1 + 2 * tagged.len());
    marks.push((0usize, 0u32));
    let mut blocks = Vec::with_capacity(1 + tagged.len());
    if qwc > 0 {
        blocks.push((0, madr, qwc, 99));
    }
    words_in(ram, spr, madr, qwc, &mut words_v);
    for t in tagged {
        if let Some(u) = t.tte {
            marks.push((words_v.len(), 2));
            words_v.extend(u);
        }
        blocks.push((words_v.len(), t.data, t.q, t.id));
        marks.push((words_v.len(), 0));
        words_in(ram, spr, t.data, t.q, &mut words_v);
    }
    (words_v, marks, blocks)
}

/// Every program's copy of the VIF1 end handler (level 1's 0x4C2908; the catalogue's func_003A45F0, fingerprint 762f2942cfb93f10:
/// one in each story level and each front end) and the gp offset of its flag word. Each copy's translation is level 1's with
/// its own gp offset: from its entry, with no loop and no call but EI (a handler's EI only sets the status bit:
/// `kernel::ei`), it reads D1_CHCR, D1_TADR and the flag word and stores only the flag word (gp + offset, ANDed) and D1_CHCR
/// (0x10009000, the channel restarted on the chain's next tag: `kick` again); it writes none of gp, sp and ra.
const END_HANDLERS: [(&str, u32, u32); 36] = [
    ("fe100", 0x003a_9db0, 0xffff_b764),
    ("fe101", 0x003a_45f0, 0xffff_b764),
    ("l01", 0x004c_2908, 0xffff_e5e0),
    ("l02", 0x0049_89e0, 0xffff_e4a0),
    ("l03", 0x004a_ff70, 0xffff_e620),
    ("l04", 0x004d_2640, 0xffff_e460),
    ("l05", 0x004b_b6e8, 0xffff_e520),
    ("l06", 0x004a_62e0, 0xffff_e460),
    ("l07", 0x004c_1730, 0xffff_e520),
    ("l08", 0x004a_4800, 0xffff_e460),
    ("l09", 0x004d_c7a8, 0xffff_e5a0),
    ("l10", 0x004a_5448, 0xffff_e660),
    ("l11", 0x0049_8948, 0xffff_e460),
    ("l12", 0x004c_3e38, 0xffff_e4e0),
    ("l13", 0x004a_2548, 0xffff_e4a0),
    ("l14", 0x004a_d1a0, 0xffff_e560),
    ("l16", 0x004d_3e08, 0xffff_e5e0),
    ("l17", 0x0049_bb90, 0xffff_e4e0),
    ("l18", 0x004d_5110, 0xffff_e4a0),
    ("l19", 0x004b_6d18, 0xffff_e520),
    ("l20", 0x004e_2350, 0xffff_e5e0),
    ("l21", 0x0049_97c0, 0xffff_e5e0),
    ("l22", 0x004c_9498, 0xffff_e4a0),
    ("l23", 0x004a_4260, 0xffff_e5a0),
    ("l24", 0x004a_76f8, 0xffff_e460),
    ("l26", 0x004c_b388, 0xffff_e4a0),
    ("l27", 0x0049_9098, 0xffff_e560),
    ("l28", 0x0048_a140, 0xffff_e460),
    ("l29", 0x004c_dd58, 0xffff_e4a0),
    ("l30", 0x0049_05c0, 0xffff_e460),
    ("l31", 0x0048_fd78, 0xffff_e460),
    ("l32", 0x0049_91a8, 0xffff_e460),
    ("l33", 0x0049_18f0, 0xffff_e460),
    ("l34", 0x0049_6c98, 0xffff_e460),
    ("l35", 0x0049_0f18, 0xffff_e460),
    ("l36", 0x0049_6a78, 0xffff_e460),
];

/// A thread's VIF1 kick, its RAM copied for the frame: its burst (`Burst`) when what its interrupt runs inside it is one of
/// `END_HANDLERS` alone. A raise from the thread runs the channel's handlers, then the interrupts waiting (`kernel::raise`,
/// `drain`): with one handler and none waiting (`kernel::dmac_alone`), that handler runs, and each kick it makes queues only
/// VIF1's interrupt again (`raise` inside a handler), which `drain` runs next; the handler makes no kernel call, so no other
/// thread runs before the raise returns (`kernel::reschedule`). Whether it began one.
fn burst(m: &mut Machine) -> bool {
    let Some((func, gp)) = crate::kernel::dmac_alone(m, 1) else { return false };
    let Some(&(_, _, off)) = m.program_at(func).and_then(|p| END_HANDLERS.iter().find(|h| h.0 == p && h.1 == func)) else { return false };
    let flag = gp.wrapping_add(off);
    // A flag word in RAM (not the scratchpad, which the chains may read: `RamCopy::spr`).
    if flag >= crate::RAM_SIZE {
        return false;
    }
    m.hw.burst = Some(Burst { flag });
    true
}

/// The chains read from the frame's RAM copy so far (`Job::Chain`: an earlier frame of the field's) built from it now, before the
/// copy is brought up to date for a thread's next kick.
fn settle(m: &mut Machine) {
    for k in 0..m.hw.jobs.len() {
        let Job::Chain(c) = &m.hw.jobs[k] else { continue };
        let (words, marks, blocks, dispfb) = c.build(&m.frame_ram).unwrap_or_else(|e| panic!("{e}"));
        m.hw.jobs[k] = Job::Vif1 { words, marks, blocks, dispfb };
    }
}

/// A VIF1 kick's stream as the DMA sends it, read where it lies (`word_in`): the normal transfer's quadwords from MADR, then per
/// block its tag's two VIF words (with CHCR's TTE) and its data. Its pieces, in order: 0 the normal transfer, 2k + 1 block k's
/// tag words, 2k + 2 its data. `vif1_events` reads it from its start on, never back, so the piece it is at only moves on.
struct Stream<'a> {
    ram: &'a [u8],
    spr: &'a [u8],
    head: (u32, u32),
    blocks: &'a [Block],
    /// Its words.
    len: usize,
    /// The piece read last, and its first word.
    piece: usize,
    from: usize,
    /// That piece's words (`enter`), and its data's bytes when they lie in RAM in one run (not the scratchpad, not wrapping at
    /// RAM's end: `words_in`'s one copy); a tag's words and other data are read by `word_in`.
    plen: usize,
    at: Option<&'a [u8]>,
}

/// The data blocks ahead of the one entered whose first line is fetched (`Stream::enter`): on a CPU whose cache does not hold the
/// frame's lists (a 4 MB L3), each block's first code was a wait on RAM.
const PREFETCH_BLOCKS: usize = 8;

impl<'a> Stream<'a> {
    fn new(ram: &'a [u8], spr: &'a [u8], head: (u32, u32), blocks: &'a [Block]) -> Stream<'a> {
        let len = 4 * head.1 as usize + blocks.iter().map(|b| 2 * b.tte.is_some() as usize + 4 * b.q as usize).sum::<usize>();
        let mut s = Stream { ram, spr, head, blocks, len, piece: 0, from: 0, plen: 0, at: None };
        s.enter();
        s
    }

    /// Data at `a`, `q` quadwords, as one run of RAM's bytes when it lies there so (`words_in`: (a + 16 k) & 0x01FFFFF0 is
    /// (a & 0x01FFFFF0) + 16 k there, and bit 31 is not reached).
    fn run(ram: &'a [u8], a: u32, q: u32) -> Option<&'a [u8]> {
        let o = (a & 0x01ff_fff0) as usize;
        (a & 0x8000_0000 == 0 && o + 16 * q as usize <= crate::RAM_SIZE as usize).then(|| &ram[o..o + 16 * q as usize])
    }

    /// The piece `piece` taken up: its words, its data's run in RAM, and the first line of the data `PREFETCH_BLOCKS` blocks on.
    fn enter(&mut self) {
        let p = self.piece;
        if p % 2 == 1 {
            (self.plen, self.at) = (2 * self.blocks[p / 2].tte.is_some() as usize, None);
            return;
        }
        let (a, q) = if p == 0 { self.head } else { (self.blocks[p / 2 - 1].data, self.blocks[p / 2 - 1].q) };
        (self.plen, self.at) = (4 * q as usize, Self::run(self.ram, a, q));
        if let Some(b) = self.blocks.get((p / 2).saturating_sub(1) + PREFETCH_BLOCKS) {
            if let Some(ahead) = Self::run(self.ram, b.data, b.q.min(1)) {
                // SAFETY: a hint only; the address is inside `ram`.
                unsafe { std::arch::x86_64::_mm_prefetch::<{ std::arch::x86_64::_MM_HINT_T0 }>(ahead.as_ptr() as *const i8) };
            }
        }
    }

    /// To the piece holding word `i` (below `len`, not before the piece read last).
    #[inline]
    fn seek(&mut self, i: usize) {
        while i >= self.from + self.plen {
            self.from += self.plen;
            self.piece += 1;
            self.enter();
        }
    }

    /// The address of the data the stream is at (the normal transfer's or a block's).
    fn addr(&self) -> u32 {
        if self.piece == 0 { self.head.0 } else { self.blocks[self.piece / 2 - 1].data }
    }

    #[inline]
    fn word(&mut self, i: usize) -> u32 {
        self.seek(i);
        let o = i - self.from;
        if let Some(at) = self.at {
            u32::from_le_bytes([at[4 * o], at[4 * o + 1], at[4 * o + 2], at[4 * o + 3]])
        } else if self.piece % 2 == 1 {
            // A tag's piece holds words only with TTE (`seek` passes over empty pieces).
            self.blocks[self.piece / 2].tte.expect("a tag's two words")[o]
        } else {
            word_in(self.ram, self.spr, self.addr(), o)
        }
    }

    /// Where a DIRECT whose code is word `c` has its data: the next quadword of the DMA stream after its code (the word after a
    /// tag's pair, or the next fourth word of a block).
    fn data_from(&mut self, c: usize) -> usize {
        self.seek(c);
        if self.piece % 2 == 1 { self.from + 2 } else { self.from + (c - self.from + 1).div_ceil(4) * 4 }
    }
}

/// What a walk of GS events (`vif1_events`, `gif_events`) keeps and changes: VIF1's write cycle (STCYCL's CL and WL, for the
/// lengths of its UNPACKs), each GIF path's packet its data ended inside of (`GifCarry`: 0 PATH2, 1 PATH3), the GS's BITBLTBUF,
/// TRXPOS and TRXREG as the packets so far left them, and what it has done to the GS registers the game reads and not yet handed
/// over (`GsState`): the CSR bits it set, SIGLBLID as the pair `(keep, set)` of the function `s -> (s & keep) | set` the packets'
/// SIGNAL and LABEL writes made of it, and the image transfers. `job` is the job the packets go with; `runs` a DIRECT's runs of
/// quadwords, kept from walk to walk.
#[derive(Clone)]
pub(crate) struct Walk {
    cycle: (u32, u32),
    carry: [GifCarry; 2],
    trx: [u64; 3],
    csr: u64,
    sig: (u64, u64),
    uploads: Vec<GsUpload>,
    runs: Vec<(usize, u32)>,
    job: usize,
}

impl Default for Walk {
    fn default() -> Walk {
        Walk { cycle: (1, 1), carry: Default::default(), trx: [0; 3], csr: 0, sig: (!0, 0), uploads: Vec::new(), runs: Vec::new(), job: 0 }
    }
}

/// The count of the worker's walks compared with UYA_KICK_CHECK (`gsstate::GsState::join`) and of those that differed.
static WORKER_CHECKS: [AtomicU64; 2] = [const { AtomicU64::new(0) }; 2];

/// GS CSR, SIGLBLID and the field's image transfers, and the walk of the VIF1 kicks' GS events that sets them, which runs on a
/// worker thread for the kicks read from the frame's RAM copy (the thread's kick and its end handler's burst). The state is in
/// a module of its own so that nothing else in the file can reach it but through its accessors, each of which waits for a
/// walk still running first: a reader never sees the state before the walk finished (the walk only sets CSR's SIGNAL and
/// FINISH bits and composes SIGLBLID's writes, so applying its result at the first read or write in place of at the kick is the
/// same as doing it at the kick).
mod gsstate {
    use super::{Block, GsUpload, Stream, Walk, checked, checking, vif1_events, Events, WORKER_CHECKS};
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering, fence};
    use std::sync::{Arc, Mutex, MutexGuard};
    use std::thread::{JoinHandle, Thread};
    use std::time::{Duration, Instant};

    /// How long the idle worker spins for a next kick before it parks: a thread's kick and its end handler's burst come within
    /// microseconds of each other.
    const SPIN: Duration = Duration::from_micros(200);

    /// A kick's walk to do: its stream's head, a copy of its blocks, the frame copy's bytes (valid until the next copy, which
    /// waits for the walk) and the job.
    struct Task {
        head: (u32, u32),
        blocks: Vec<Block>,
        ram: (*const u8, usize),
        job: usize,
    }

    // SAFETY: `ram` is the frame copy's bytes, read only here and not changed until the game thread has joined the walk.
    unsafe impl Send for Task {}

    #[derive(Default)]
    struct Queue {
        tasks: VecDeque<Task>,
        /// Block vectors given back by the worker, to be filled by the next kick.
        pool: Vec<Vec<Block>>,
    }

    struct Shared {
        /// The walk's state: the worker holds it for each task, the game thread at its own walks and at the join.
        walk: Mutex<Walk>,
        queue: Mutex<Queue>,
        /// The tasks given and the tasks done (the worker's counts are `done`; the game thread's `submitted`).
        submitted: AtomicU64,
        done: AtomicU64,
        asleep: AtomicBool,
        quit: AtomicBool,
        /// The panic of a task (the walk's own: a VIF code it does not know), raised again at the join.
        panic: Mutex<Option<Box<dyn std::any::Any + Send>>>,
        /// UYA_KICK_CHECK: the walk after each task.
        snaps: Mutex<Vec<Events>>,
    }

    fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
        m.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) struct GsState {
        csr: u64,
        siglblid: u64,
        uploads: Vec<GsUpload>,
        shared: Arc<Shared>,
        worker: Option<(JoinHandle<()>, Thread)>,
        /// Tasks given since the last join.
        pending: bool,
        /// Time stamp counter ticks the joins waited for the worker, for the host's line on a slow field
        /// (`machine::FieldWaits::walk_wait`).
        pub(crate) waited: u64,
        /// UYA_KICK_CHECK: the same walks done inline on the RAM as it lies, from the walk's state at the first of the tasks
        /// pending, and the walk after each.
        shadow: Walk,
        shadow_snaps: Vec<Events>,
    }

    impl GsState {
        pub(crate) fn new() -> GsState {
            GsState {
                csr: 0x5500,
                siglblid: 0,
                uploads: Vec::new(),
                shared: Arc::new(Shared {
                    walk: Mutex::new(Walk::default()),
                    queue: Mutex::new(Queue::default()),
                    submitted: AtomicU64::new(0),
                    done: AtomicU64::new(0),
                    asleep: AtomicBool::new(false),
                    quit: AtomicBool::new(false),
                    panic: Mutex::new(None),
                    snaps: Mutex::new(Vec::new()),
                }),
                worker: None,
                pending: false,
                waited: 0,
                shadow: Walk::default(),
                shadow_snaps: Vec::new(),
            }
        }

        pub(crate) fn csr(&mut self) -> u64 {
            self.join();
            self.csr
        }

        pub(crate) fn csr_mut(&mut self) -> &mut u64 {
            self.join();
            &mut self.csr
        }

        pub(crate) fn siglblid(&mut self) -> u64 {
            self.join();
            self.siglblid
        }

        pub(crate) fn siglblid_mut(&mut self) -> &mut u64 {
            self.join();
            &mut self.siglblid
        }

        /// The field's image transfers so far, taken.
        pub(crate) fn take_uploads(&mut self) -> Vec<GsUpload> {
            self.join();
            std::mem::take(&mut self.uploads)
        }

        /// A walk done now, on the game thread, its results handed over at once.
        pub(crate) fn inline<R>(&mut self, f: impl FnOnce(&mut Walk) -> R) -> R {
            self.join();
            let mut w = lock(&self.shared.walk);
            let r = f(&mut w);
            apply(&mut w, &mut self.csr, &mut self.siglblid, &mut self.uploads);
            r
        }

        /// Whether kicks' walks go to the worker thread: when the game's threads keep a core of their own (`play`'s
        /// `keep_game_core` sets `kernel::THREAD_START` only then); on a PC with too few cores for a spare one, every thread
        /// shares them and the walk is done at the kick as before.
        pub(crate) fn offload(&self) -> bool {
            crate::kernel::THREAD_START.get().is_some()
        }

        /// A kick's walk handed to the worker, from the frame copy's bytes `ram`.
        pub(crate) fn submit(&mut self, head: (u32, u32), blocks: &[Block], ram: (*const u8, usize), job: usize) {
            if self.worker.is_none() {
                let shared = self.shared.clone();
                let h = std::thread::Builder::new().name("uya walk events".into()).spawn(move || worker(shared)).expect("the walk's worker thread");
                let t = h.thread().clone();
                self.worker = Some((h, t));
            }
            let s = &self.shared;
            {
                let mut q = lock(&s.queue);
                let mut v = q.pool.pop().unwrap_or_default();
                v.clear();
                v.extend_from_slice(blocks);
                q.tasks.push_back(Task { head, blocks: v, ram, job });
            }
            self.pending = true;
            s.submitted.fetch_add(1, Ordering::SeqCst);
            fence(Ordering::SeqCst);
            if s.asleep.load(Ordering::SeqCst) {
                self.worker.as_ref().expect("spawned").1.unpark();
            }
        }

        /// UYA_KICK_CHECK: the walk of a kick given to the worker, done inline too on the RAM as it lies, to compare at the join.
        /// Called before `submit`.
        pub(crate) fn shadow(&mut self, job: usize, f: impl FnOnce(&mut Walk)) {
            if !self.pending {
                self.shadow = lock(&self.shared.walk).clone();
                self.shadow_snaps.clear();
            }
            self.shadow.job = job;
            f(&mut self.shadow);
            self.shadow_snaps.push(Events::of(&self.shadow));
        }

        /// Waits for the walks given to the worker and hands over their results; a panic of theirs is raised here as it was
        /// raised there.
        pub(crate) fn join(&mut self) {
            if !self.pending {
                return;
            }
            self.pending = false;
            let s = &self.shared;
            let target = s.submitted.load(Ordering::SeqCst);
            let mut spins = 0u32;
            let t = crate::machine::tsc();
            while s.done.load(Ordering::Acquire) < target {
                spins += 1;
                if spins < 4000 {
                    std::hint::spin_loop();
                } else {
                    // A wake-up the worker missed would hang here: wake it again.
                    self.worker.as_ref().expect("spawned").1.unpark();
                    std::thread::yield_now();
                }
            }
            self.waited += crate::machine::tsc() - t;
            if let Some(p) = lock(&s.panic).take() {
                std::panic::resume_unwind(p);
            }
            let mut w = lock(&s.walk);
            apply(&mut w, &mut self.csr, &mut self.siglblid, &mut self.uploads);
            if checking() {
                let theirs = std::mem::take(&mut *lock(&s.snaps));
                assert_eq!(theirs.len(), self.shadow_snaps.len(), "UYA_KICK_CHECK: the worker's walks and the inline walks differ in number");
                for (k, (a, b)) in self.shadow_snaps.iter().zip(&theirs).enumerate() {
                    let d = a.differences(b);
                    let n = WORKER_CHECKS[0].fetch_add(1, Ordering::Relaxed) + 1;
                    let bad = if d.is_empty() { WORKER_CHECKS[1].load(Ordering::Relaxed) } else { WORKER_CHECKS[1].fetch_add(1, Ordering::Relaxed) + 1 };
                    if !d.is_empty() && checked(2, true) {
                        println!("UYA_KICK_CHECK: the worker's walk {k} of {} (job {}): {}", theirs.len(), a.uploads.len(), d.join("; "));
                    }
                    if n == 1 || n % 500 == 0 {
                        println!("UYA_KICK_CHECK: the GS events walked by the worker thread: {n} compared with the inline walk, {bad} differ");
                    }
                }
                self.shadow_snaps.clear();
            }
        }
    }

    impl Drop for GsState {
        fn drop(&mut self) {
            let Some((h, t)) = self.worker.take() else { return };
            let s = &self.shared;
            while s.done.load(Ordering::Acquire) < s.submitted.load(Ordering::SeqCst) {
                t.unpark();
                std::thread::yield_now();
            }
            s.quit.store(true, Ordering::SeqCst);
            t.unpark();
            let _ = h.join();
        }
    }

    /// What a walk did, handed over to CSR, SIGLBLID and the transfers.
    fn apply(w: &mut Walk, csr: &mut u64, siglblid: &mut u64, uploads: &mut Vec<GsUpload>) {
        *csr |= std::mem::take(&mut w.csr);
        let (keep, set) = std::mem::replace(&mut w.sig, (!0, 0));
        *siglblid = (*siglblid & keep) | set;
        uploads.append(&mut w.uploads);
    }

    /// The worker thread: walks the tasks in order, spinning a while for the next and then parking.
    fn worker(s: Arc<Shared>) {
        loop {
            let task = lock(&s.queue).tasks.pop_front();
            let Some(t) = task else {
                if s.quit.load(Ordering::SeqCst) {
                    return;
                }
                let t0 = Instant::now();
                while s.submitted.load(Ordering::SeqCst) == s.done.load(Ordering::Relaxed) && t0.elapsed() < SPIN && !s.quit.load(Ordering::Relaxed) {
                    std::hint::spin_loop();
                }
                if s.submitted.load(Ordering::SeqCst) == s.done.load(Ordering::Relaxed) && !s.quit.load(Ordering::SeqCst) {
                    s.asleep.store(true, Ordering::SeqCst);
                    fence(Ordering::SeqCst);
                    if s.submitted.load(Ordering::SeqCst) == s.done.load(Ordering::Relaxed) && !s.quit.load(Ordering::SeqCst) {
                        std::thread::park();
                    }
                    s.asleep.store(false, Ordering::SeqCst);
                }
                continue;
            };
            if lock(&s.panic).is_none() {
                let mut w = lock(&s.walk);
                // SAFETY: the frame copy's bytes, unchanged until the game thread has joined this walk.
                let ram = unsafe { std::slice::from_raw_parts(t.ram.0, t.ram.1) };
                w.job = t.job;
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| vif1_events(&mut w, Stream::new(ram, &[], t.head, &t.blocks))));
                if checking() && r.is_ok() {
                    lock(&s.snaps).push(Events::of(&w));
                }
                drop(w);
                if let Err(p) = r {
                    *lock(&s.panic) = Some(p);
                }
            }
            lock(&s.queue).pool.push(t.blocks);
            s.done.fetch_add(1, Ordering::Release);
        }
    }
}

/// A kick's GS events (`vif1_events`) from its stream where it lies, done at once (`GsState::inline`); with UYA_KICK_CHECK also
/// from its words as copied at the kick (`copied`: `vif1_events_words`, the way before), the two compared, the machine going on
/// from the first.
fn events(m: &mut Machine, head: (u32, u32), tagged: &[Block], copied: Option<&Vif1Words>, job: usize) {
    let (ram, spr, field) = (&m.ram, &m.spr, m.field);
    m.hw.walk.inline(|w| {
        w.job = job;
        let Some((words, marks, _)) = copied else {
            vif1_events(w, Stream::new(ram, spr, head, tagged));
            return;
        };
        let before = w.clone();
        let tags: Vec<usize> = marks.iter().filter(|m| m.1 == 2).map(|m| m.0).collect();
        vif1_events_words(w, words, &tags);
        let old = Events::of(w);
        *w = before;
        vif1_events(w, Stream::new(ram, spr, head, tagged));
        let d = Events::of(w).differences(&old);
        if checked(2, !d.is_empty()) {
            println!("UYA_KICK_CHECK: field {field} job {job}: the GS events: {}", d.join("; "));
        }
    });
}

/// What a VIF1 stream's GS events change (`vif1_events`), as a walk holds it: for UYA_KICK_CHECK.
#[derive(Clone)]
struct Events {
    cycle: (u32, u32),
    carry: GifCarry,
    sig: (u64, u64),
    csr: u64,
    trx: [u64; 3],
    uploads: Vec<GsUpload>,
}

impl Events {
    fn of(w: &Walk) -> Events {
        Events { cycle: w.cycle, carry: w.carry[0].clone(), sig: w.sig, csr: w.csr, trx: w.trx, uploads: w.uploads.clone() }
    }

    /// What differs, these read where the stream lies, `old` from the words copied (or, for the worker's walk, from the frame's
    /// copy).
    fn differences(&self, old: &Events) -> Vec<String> {
        let mut d = Vec::new();
        if self.cycle != old.cycle {
            d.push(format!("STCYCL {:?} in place, {:?} from the copy", self.cycle, old.cycle));
        }
        if self.carry != old.carry {
            d.push("the GIF packet carried on differs".to_string());
        }
        if (self.sig, self.csr) != (old.sig, old.csr) {
            d.push(format!("SIGLBLID (keep, set) {:x?} and CSR {:x} in place, {:x?} and {:x} from the copy", self.sig, self.csr, old.sig, old.csr));
        }
        if self.trx != old.trx {
            d.push(format!("BITBLTBUF, TRXPOS, TRXREG {:x?} in place, {:x?} from the copy", self.trx, old.trx));
        }
        if self.uploads != old.uploads {
            d.push(format!("image transfers: {} in place, {} from the copy, the first differing {:?}", self.uploads.len(), old.uploads.len(), self.uploads.iter().zip(&old.uploads).position(|(a, b)| a != b)));
        }
        d
    }
}

/// A source chain walked as the DMA walks it: per tag the TTE words (when CHCR's TTE is set), then its data; to an END tag or a
/// tag with IRQ when TIE is set. `f(tag's upper words or None, data address, quadwords, id)` for each.
fn chain(m: &mut crate::Machine, ch: usize, mut f: impl FnMut(&crate::Machine, Option<[u32; 2]>, u32, u32, u32)) {
    let chcr = m.hw.ch[ch][0];
    let tte = chcr & 0x40 != 0;
    let mut tadr = m.hw.ch[ch][3];
    let mut asr = [m.hw.ch[ch][4], m.hw.ch[ch][5]];
    let mut asp = ((chcr >> 4) & 3) as usize;
    let mut madr = m.hw.ch[ch][1];
    // CHCR's TAG field: the upper 16 bits of the last tag read (the game's VIF1 end handler 0x3a9db0 reads its ID and its own
    // marker bits to tell an END from an IRQ tag it restarts the channel after).
    let mut last = chcr >> 16;
    // A chain that sends more than all of EE RAM (2M quadwords) in one kick has gone round a loop.
    let mut sent = 0u64;
    for n in 0.. {
        if n > 1_000_000 {
            panic!("DMA channel {ch}: the chain from {:08x} does not end", m.hw.ch[ch][3]);
        }
        let tag = qw(m, tadr);
        let q = (tag & 0xffff) as u32;
        sent += q as u64;
        if sent > 2 << 20 {
            panic!("DMA channel {ch}: the chain from {:08x} has sent {sent} quadwords at tag {n}, {tadr:08x} ({tag:032x}): it goes round a loop", m.hw.ch[ch][3]);
        }
        let id = ((tag >> 28) & 7) as u32;
        last = (tag >> 16) as u32 & 0xffff;
        let addr = (tag >> 32) as u32 & 0xffff_fff0;
        let (data, next, end) = match id {
            0 => (addr, tadr + 16, true),
            1 => (tadr + 16, tadr + 16 + q * 16, false),
            2 => (tadr + 16, addr, false),
            3 | 4 => (addr, tadr + 16, false),
            5 => {
                if asp < 2 {
                    asr[asp] = tadr + 16 + q * 16;
                    asp += 1;
                }
                (tadr + 16, addr, false)
            }
            6 => {
                if asp > 0 {
                    asp -= 1;
                    (tadr + 16, asr[asp], false)
                } else {
                    (tadr + 16, tadr + 16 + q * 16, true)
                }
            }
            _ => (tadr + 16, tadr + 16 + q * 16, true),
        };
        let upper = tte.then(|| [(tag >> 64) as u32, (tag >> 96) as u32]);
        f(m, upper, data, q, id);
        madr = data.wrapping_add(q * 16);
        tadr = next;
        if end || ((tag >> 31) & 1 != 0 && chcr & 0x80 != 0) {
            break;
        }
    }
    let h = &mut m.hw.ch[ch];
    h[1] = madr;
    h[3] = tadr;
    h[4] = asr[0];
    h[5] = asr[1];
    h[0] = (h[0] & 0xffcf) | (asp as u32) << 4 | last << 16;
}

/// The GS events a VIF1 stream's GIF packets (DIRECT, DIRECTHL) raise (`gif_events`), read where the stream lies (`Stream`):
/// what `vif1_events_words` finds in the stream's words copied. The rest of its codes are walked over by their lengths.
fn vif1_events(h: &mut Walk, mut s: Stream) {
    // A DIRECT's data starts at the next quadword of the DMA stream after its code (`Stream::data_from`) and steps over the tags'
    // words it runs into; its quadwords go to the GIF from where they lie, by runs within one block (`runs`: each run's first
    // quadword of the DIRECT and its address). The data is at a tag's words (`seek` there) only at their first: it starts at a
    // block's quadword or a tag's words, and goes on by whole quadwords of blocks and by tags' pairs.
    let mut runs = std::mem::take(&mut h.runs);
    let mut i = 0;
    while i < s.len {
        let code = s.word(i);
        let (cmd, imm, num) = ((code >> 24) & 0x7f, code & 0xffff, (code >> 16) & 0xff);
        i += 1;
        match cmd {
            0x00 | 0x02..=0x07 | 0x10 | 0x11 | 0x13 | 0x14 | 0x15 | 0x17 => {}
            0x01 => h.cycle = (imm & 0xff, (imm >> 8) & 0xff),
            0x20 => i += 1,
            0x30 | 0x31 => i += 4,
            0x4a => i += 2 * if num == 0 { 256 } else { num } as usize,
            0x50 | 0x51 => {
                let n = if imm == 0 { 0x1_0000 } else { imm } as usize;
                let mut p = s.data_from(i - 1);
                runs.clear();
                let mut got = 0;
                while got < n && p + 4 <= s.len {
                    s.seek(p);
                    if s.piece % 2 == 1 {
                        p += 2;
                        continue;
                    }
                    let k = (p - s.from) / 4;
                    let take = (n - got).min(s.plen / 4 - k);
                    runs.push((got, s.addr().wrapping_add(16 * k as u32)));
                    got += take;
                    p += 4 * take;
                }
                let (ram, spr) = (s.ram, s.spr);
                // The data in one run of RAM (a DIRECT within one block, the usual case) is read from its bytes; across blocks, or
                // in the scratchpad, each quadword from its run.
                match runs[..] {
                    [(0, a)] if Stream::run(ram, a, got as u32).is_some() => {
                        let at = Stream::run(ram, a, got as u32).expect("tested");
                        gif_events(h, 0, got, |k| u128::from_le_bytes(at[16 * k..16 * k + 16].try_into().expect("16 bytes")));
                    }
                    _ => gif_events(h, 0, got, |k| {
                        let (k0, a) = runs[runs.partition_point(|r| r.0 <= k) - 1];
                        qw_in(ram, spr, a.wrapping_add(16 * (k - k0) as u32))
                    }),
                }
                i = p;
            }
            0x60..=0x7f => {
                // The vectors read: all `num` of them, or with WL > CL (filling writes) CL of every WL.
                let num = if num == 0 { 256 } else { num };
                let (cl, wl) = (h.cycle.0.max(1), h.cycle.1.max(1));
                let read = if wl <= cl { num } else { num / wl * cl + (num % wl).min(cl) };
                let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
                let bits = if vl == 3 { 16 } else { [32, 16, 8][vl as usize] * (vn + 1) };
                i += (read * bits).div_ceil(32) as usize;
            }
            _ => {
                s.seek(i - 1);
                // The word's block: the head's (piece 0), a tag's words (odd pieces) or a tag's data (even pieces).
                let at = match s.piece {
                    0 => format!("{:08x}", s.head.0.wrapping_add(4 * (i - 1) as u32)),
                    p if p % 2 == 1 => format!("the words of tag {}", p / 2),
                    p => format!("{:08x} (tag {}'s data, ID {})", s.blocks[p / 2 - 1].data.wrapping_add(4 * (i - 1 - s.from) as u32), p / 2 - 1, s.blocks[p / 2 - 1].id),
                };
                panic!("VIF1 code {code:08x} at word {} of a transfer from {:08x}: not a VIF code (at {at})", i - 1, s.head.0)
            }
        }
    }
    h.runs = runs;
}

/// `vif1_events` from a stream's words copied (`assemble`) and its tags' places, as the kicks found them before: for
/// UYA_KICK_CHECK.
fn vif1_events_words(h: &mut Walk, w: &[u32], tags: &[usize]) {
    // A DMA tag's two VIF words (at each of `tags`, ascending) sit in the upper half of its quadword: a DIRECT's data starts at the
    // next quadword of the DMA stream after its code (the word after a tag's pair, or the next fourth word of a block) and steps
    // over the tags it runs into.
    let data_from = |c: usize| -> usize {
        let j = tags.partition_point(|&t| t + 1 < c);
        if j < tags.len() && tags[j] <= c {
            return tags[j] + 2;
        }
        let b = if j == 0 { 0 } else { tags[j - 1] + 2 };
        b + (c - b + 1).div_ceil(4) * 4
    };
    let mut i = 0;
    while i < w.len() {
        let code = w[i];
        let (cmd, imm, num) = ((code >> 24) & 0x7f, code & 0xffff, (code >> 16) & 0xff);
        i += 1;
        match cmd {
            0x00 | 0x02..=0x07 | 0x10 | 0x11 | 0x13 | 0x14 | 0x15 | 0x17 => {}
            0x01 => h.cycle = (imm & 0xff, (imm >> 8) & 0xff),
            0x20 => i += 1,
            0x30 | 0x31 => i += 4,
            0x4a => i += 2 * if num == 0 { 256 } else { num } as usize,
            0x50 | 0x51 => {
                let n = if imm == 0 { 0x1_0000 } else { imm } as usize;
                let mut p = data_from(i - 1);
                // The tags from `p` on, passed in step with the data: a DIRECT of a texture's image runs to thousands of
                // quadwords, and a search of every tag for each of them was most of a kick (Daxx: the game thread's 3.4 ms of
                // VIF1 kicks a field).
                let mut t = tags.partition_point(|&x| x < p);
                let mut d: Vec<u128> = Vec::with_capacity(n);
                while d.len() < n && p + 4 <= w.len() {
                    while t < tags.len() && tags[t] < p {
                        t += 1;
                    }
                    if t < tags.len() && tags[t] == p {
                        p += 2;
                        continue;
                    }
                    d.push(w[p] as u128 | (w[p + 1] as u128) << 32 | (w[p + 2] as u128) << 64 | (w[p + 3] as u128) << 96);
                    p += 4;
                }
                gif_events(h, 0, d.len(), |k| d[k]);
                i = p;
            }
            0x60..=0x7f => {
                // The vectors read: all `num` of them, or with WL > CL (filling writes) CL of every WL.
                let num = if num == 0 { 256 } else { num };
                let (cl, wl) = (h.cycle.0.max(1), h.cycle.1.max(1));
                let read = if wl <= cl { num } else { num / wl * cl + (num % wl).min(cl) };
                let (vn, vl) = ((cmd >> 2) & 3, cmd & 3);
                let bits = if vl == 3 { 16 } else { [32, 16, 8][vl as usize] * (vn + 1) };
                i += (read * bits).div_ceil(32) as usize;
            }
            _ => panic!("VIF1 code {code:08x} at word {} of a transfer: not a VIF code", i - 1),
        }
    }
}

/// The GS events in GIF packets, as the GS raises them on drawing them (the drawing is the renderer's; the events are the
/// EE's to see at once): an A+D write to SIGNAL (0x60) or LABEL (0x62) updates SIGLBLID, SIGNAL and FINISH (0x61) set their
/// CSR flags (libgraph's sceGsExecStoreImage at 0x12d14c waits for FINISH before it reads the GS back). And the image
/// transfers (`GsUpload`): A+D writes of BITBLTBUF, TRXPOS and TRXREG, and IMAGE packets, of the job about to be handed over.
/// A GIF packet a path's data ended inside of: the path's next data (its next DIRECT, its next DMA transfer) goes on with it, as
/// the GIF takes it. A texture's image data often comes in a DIRECT of its own after its tag's, and a transfer cut at a
/// DIRECT's end lost its last quadwords and read the next data as tags (the player's crash, 19 November 2026: a 16 x 16
/// palette handed over as 1008 of its 1024 bytes).
#[derive(Default, Clone, PartialEq)]
enum GifCarry {
    #[default]
    None,
    /// A PACKED packet with an A+D register: its tag, register count, the loops left and the register the loop is at.
    Packed { tag: u128, nreg: usize, loops: usize, r: usize },
    /// Quadwords of a packet with nothing to read (PACKED with no A+D register, REGLIST) still to pass over.
    Skip(usize),
    /// An IMAGE transfer: what came of it so far (handed over, `Hw::uploads`, when its last quadword comes) and the quadwords left.
    Image(GsUpload, usize),
}

/// The GS events of a path's GIF data (`path` 0: VIF1's DIRECT, PATH2; 1: the GIF DMA, PATH3): SIGNAL, FINISH and LABEL, the
/// transfer registers, and each IMAGE transfer (`GsUpload`). A packet the data ends inside of is carried to the path's next data
/// (`GifCarry`).
fn gif_events(h: &mut Walk, path: usize, n: usize, q: impl Fn(usize) -> u128) {
    let job = h.job;
    let mut i = 0;
    match std::mem::take(&mut h.carry[path]) {
        GifCarry::None => {}
        GifCarry::Skip(k) => {
            if k > n {
                h.carry[path] = GifCarry::Skip(k - n);
                return;
            }
            i = k;
        }
        GifCarry::Image(mut up, k) => {
            let m = k.min(n);
            up.data.extend((0..m).map(&q));
            if m < k {
                h.carry[path] = GifCarry::Image(up, k - m);
                return;
            }
            h.uploads.push(up);
            i = m;
        }
        GifCarry::Packed { tag, nreg, loops, r } => {
            if let Some(c) = gif_packed(h, tag, nreg, loops, r, &mut i, n, &q) {
                h.carry[path] = c;
                return;
            }
        }
    }
    while i < n {
        let tag = q(i);
        i += 1;
        let nloop = (tag & 0x7fff) as usize;
        let flg = (tag >> 58) & 3;
        let nreg = match ((tag >> 60) & 15) as usize {
            0 => 16,
            n => n,
        };
        let skip = |h: &mut Walk, i: &mut usize, k: usize| -> bool {
            if *i + k > n {
                h.carry[path] = GifCarry::Skip(*i + k - n);
                return false;
            }
            *i += k;
            true
        };
        match flg {
            // Only an A+D register writes SIGNAL, FINISH or LABEL: a packet with none is passed over whole.
            0 if !(0..nreg).any(|r| (tag >> (64 + 4 * r)) & 15 == 0xe) => {
                if !skip(h, &mut i, nloop * nreg) {
                    return;
                }
            }
            0 => {
                if let Some(c) = gif_packed(h, tag, nreg, nloop, 0, &mut i, n, &q) {
                    h.carry[path] = c;
                    return;
                }
            }
            1 => {
                if !skip(h, &mut i, (nloop * nreg).div_ceil(2)) {
                    return;
                }
            }
            _ => {
                let m = nloop.min(n - i);
                let up = GsUpload { job, bitbltbuf: h.trx[0], trxpos: h.trx[1], trxreg: h.trx[2], data: (i..i + m).map(&q).collect() };
                if m < nloop {
                    h.carry[path] = GifCarry::Image(up, nloop - m);
                    return;
                }
                if m > 0 {
                    h.uploads.push(up);
                }
                i += nloop;
            }
        }
    }
}

/// A PACKED packet's registers from loop `loops` left and register `r` on, the A+D writes read (SIGNAL, FINISH, LABEL and the
/// transfer registers); its carry when the data ends first.
#[allow(clippy::too_many_arguments)]
fn gif_packed(h: &mut Walk, tag: u128, nreg: usize, mut loops: usize, mut r: usize, i: &mut usize, n: usize, q: &impl Fn(usize) -> u128) -> Option<GifCarry> {
    while loops > 0 {
        while r < nreg {
            if *i >= n {
                return Some(GifCarry::Packed { tag, nreg, loops, r });
            }
            let d = q(*i);
            *i += 1;
            if (tag >> (64 + 4 * r)) & 15 == 0xe {
                let (addr, v) = ((d >> 64) as u8, d as u64);
                let (lo, mask) = (v & 0xffff_ffff, v >> 32);
                match addr {
                    0x60 => {
                        h.sig = (h.sig.0 & !mask, (h.sig.1 & !mask) | (lo & mask));
                        h.csr |= 1;
                    }
                    0x61 => h.csr |= 2,
                    0x62 => h.sig = (h.sig.0 & !(mask << 32), (h.sig.1 & !(mask << 32)) | ((lo & mask) << 32)),
                    0x50..=0x52 => h.trx[(addr - 0x50) as usize] = v,
                    _ => {}
                }
            }
            r += 1;
        }
        r = 0;
        loops -= 1;
    }
    None
}

/// A DMA kick (CHCR with STR): the transfer, then the channel ends (its interrupt is raised by `kick_timed`). Whether it began
/// a VIF1 burst (`burst`).
fn kick(c: &mut Cpu, ch: usize) -> bool {
    let ra = c.r[31] as u32;
    let thread = c.thread;
    let m = c.mach();
    let chcr = m.hw.ch[ch][0];
    let mode = (chcr >> 2) & 3;
    let qwc = m.hw.ch[ch][2] & 0xffff;
    let madr = m.hw.ch[ch][1];
    let mut began = false;
    match ch {
        // VIF1 (to the GS: DIR 1): the chain's words to the renderer. DIR 0 is a GS -> EE download, which has nothing to give.
        1 => {
            if chcr & 1 != 0 {
                // The game kicks a frame's first draw list from its thread and the DMA end handler chains the rest (level 28: one
                // kick from 0x489644, four from the handler). Its loop is: the chain, the cull for the next frame, the wait for the
                // vertical blank; when it runs late a field can end between a chain and the cull after it, and the next field
                // culls and then builds its chain (Veldin, a --fast run's field 913: the RAM at its start held field 912's list,
                // its chain the next one's table), or hold two frames (level 28's arrival, field 412). So the renderer draws a
                // field's last frame with the RAM as it stood at that frame's first kick: the state its lists were built from
                // (`Machine::frame_ram`). A thread's second VIF1 kick from one call site in a field is the field's next frame.
                // Before the copy is brought up to date, the chains read from it so far are built from it (`settle`).
                if thread != 0 {
                    // The frame's copy is about to change: the walk of the last kicks' GS events reads it.
                    m.hw.walk.join();
                    m.hw.packets.begin(m.field, &mut m.hw.tie_costs);
                    if m.hw.vif1_sites.contains(&ra) {
                        m.hw.vif1_sites.clear();
                        m.hw.frame_from = m.hw.jobs.len();
                    }
                    m.hw.vif1_sites.push(ra);
                    m.hw.kicked = true;
                    settle(m);
                    let t = crate::machine::tsc();
                    m.ram.copy_to(&mut m.frame_ram).expect("the RAM for the frame");
                    m.prof[3] += crate::machine::tsc() - t;
                    m.frame_ram.chains.clear();
                    m.frame_ram.spr.clear();
                    began = burst(m);
                }
                // The words of a thread's kick, and of the kicks its end handler makes in its burst, are read from the frame's
                // copy by the renderer (`Chain`): its blocks are kept in the copy. Any other kick's are copied now, as the DMA
                // reads them (`assemble`).
                let from_copy = thread != 0 || m.hw.burst.is_some();
                let mut rec = std::mem::take(if from_copy { &mut m.frame_ram.chains } else { &mut m.hw.blocks });
                if !from_copy {
                    rec.clear();
                }
                let first = rec.len();
                if mode == 1 {
                    chain(m, 1, |_, tte, data, q, id| rec.push(Block { tte, data, q, id }));
                } else {
                    m.hw.ch[1][1] = madr.wrapping_add(qwc * 16);
                }
                let tagged = &rec[first..];
                m.hw.packets.blocks(m.field, tagged);
                // The end handler stored its flag word since the copy: a kick of the burst that reads it is copied now.
                let read = thread == 0 && m.hw.burst.is_some_and(|b| holds(madr, qwc, b.flag) || tagged.iter().any(|t| holds(t.data, t.q, b.flag)));
                let check = checking().then(|| assemble(&m.ram, &m.spr, madr, qwc, tagged));
                let job = m.hw.jobs.len();
                // The scratchpad is read by these (and only the RAM copy's bytes are shared with the worker, not the scratchpad).
                let spr_read = touches_spr(madr, qwc) || tagged.iter().any(|t| touches_spr(t.data, t.q));
                if from_copy && !read && !spr_read && m.hw.walk.offload() {
                    // The walk of this kick's GS events is read from the frame's copy, whose bytes stay as they are until the next
                    // thread kick's copy (it joins first), off the game thread (`gsstate`).
                    if checking() {
                        let (ram, spr) = (&m.ram, &m.spr);
                        m.hw.walk.shadow(job, |w| vif1_events(w, Stream::new(ram, spr, (madr, qwc), tagged)));
                    }
                    m.hw.walk.submit((madr, qwc), tagged, m.frame_ram.raw(), job);
                } else {
                    events(m, (madr, qwc), tagged, check.as_ref(), job);
                }
                let (dispfb, tadr) = (m.hw.dispfb(), m.hw.ch[1][3]);
                let j = if from_copy && !read {
                    if m.frame_ram.spr.is_empty() && spr_read {
                        m.frame_ram.spr.extend_from_slice(&m.spr);
                    }
                    Job::Chain(Chain { copy: m.frame_ram.key(), head: (madr, qwc), blocks: first..rec.len(), dispfb, field: m.field, job, check: check.map(Box::new) })
                } else {
                    // The check copied the same words at this kick.
                    let (words, marks, blocks) = match check {
                        Some(w) => w,
                        None => assemble(&m.ram, &m.spr, madr, qwc, tagged),
                    };
                    rec.truncate(first);
                    Job::Vif1 { words, marks, blocks, dispfb }
                };
                if from_copy {
                    m.frame_ram.chains = rec;
                } else {
                    m.hw.blocks = rec;
                }
                m.hw.job(j, || format!("VIF1 DMA from {madr:08x} (chain {tadr:08x})"), ra);
            }
        }
        // GIF path 3.
        2 => {
            // As VIF1's: the tags, then the words into a vector made at their size.
            let mut tagged: Vec<(u32, u32)> = Vec::new();
            if mode == 1 {
                chain(m, 2, |_, _, data, q, _| tagged.push((data, q)));
            } else {
                m.hw.ch[2][1] = madr.wrapping_add(qwc * 16);
            }
            let mut w = Vec::with_capacity(4 * (qwc as usize + tagged.iter().map(|t| t.1 as usize).sum::<usize>()));
            words(m, madr, qwc, &mut w);
            for (data, q) in tagged {
                words(m, data, q, &mut w);
            }
            let qs: Vec<u128> = w.chunks_exact(4).map(|x| x[0] as u128 | (x[1] as u128) << 32 | (x[2] as u128) << 64 | (x[3] as u128) << 96).collect();
            let job = m.hw.jobs.len();
            m.hw.walk.inline(|w| {
                w.job = job;
                gif_events(w, 1, qs.len(), |k| qs[k]);
            });
            let d = m.hw.dispfb();
            m.hw.job(Job::Gif { qs, dispfb: d }, || format!("GIF DMA from {madr:08x}"), ra);
        }
        // VIF0: its codes into VU0's memories.
        0 => {
            let mut w = Vec::new();
            words(m, madr, qwc, &mut w);
            if mode == 1 {
                chain(m, 0, |m, upper, data, q, _| {
                    if let Some(u) = upper {
                        w.extend(u);
                    }
                    words(m, data, q, &mut w);
                });
            } else {
                m.hw.ch[0][1] = madr.wrapping_add(qwc * 16);
            }
            vif0(c, &w);
        }
        // From the scratchpad to RAM (normal), from RAM to the scratchpad (normal or source chain).
        8 => {
            let sadr = m.hw.ch[8][8] & 0x3ff0;
            if mode != 0 {
                panic!("SPR_FROM in mode {mode}: the port copies normal transfers only");
            }
            // Neither side wrapping (the scratchpad at 16 KB, RAM at its end): one copy.
            let (s0, d0, n) = (sadr as usize, (madr & 0x01ff_fff0) as usize, 16 * qwc as usize);
            if matches!(sadr, 0x3000 | 0x3400) && m.hw.skin.captured && !m.hw.skin.records.is_empty() {
                m.hw.skin.outputs.push(SkinOutput {
                    record: m.hw.skin.records.len() - 1,
                    dest: madr,
                    bytes: m.spr[s0..s0 + n].to_vec(),
                    normals: std::mem::take(&mut m.hw.skin_normals),
                });
            }
            if s0 + n <= 0x4000 && d0 + n <= crate::RAM_SIZE as usize {
                m.ram[d0..d0 + n].copy_from_slice(&m.spr[s0..s0 + n]);
            } else {
                for k in 0..qwc {
                let s = ((sadr + 16 * k) & 0x3ff0) as usize;
                let d = (madr.wrapping_add(16 * k) & 0x01ff_fff0) as usize;
                let q: [u8; 16] = m.spr[s..s + 16].try_into().unwrap();
                m.ram[d..d + 16].copy_from_slice(&q);
                }
            }
            m.hw.ch[8][1] = madr.wrapping_add(qwc * 16);
            m.hw.ch[8][8] = (sadr + qwc * 16) & 0x3ff0;
        }
        9 => {
            let mut sadr = m.hw.ch[9][8] & 0x3ff0;
            // The skin job's light section (or second group) to 0x1C00: its joint matrices are in the scratchpad (`SkinJoints`).
            if mode == 0 && sadr == 0x1c00 && crate::SKIN_JOINTS.load(std::sync::atomic::Ordering::Relaxed) {
                m.hw.skin.captured = true;
                let (from, to) = ((madr & 0x01ff_fff0) as usize, (madr & 0x01ff_fff0) as usize + 16 * qwc as usize);
                if from >= 0x40 && to <= crate::RAM_SIZE as usize {
                    let at = m.hw.skin.bytes.len();
                    m.hw.skin.bytes.extend_from_slice(&m.ram[from - 0x40..to]);
                    m.hw.skin.records.push((madr, qwc, at));
                }
            }
            fn put(m: &mut crate::Machine, sadr: &mut u32, a: u32, q: u32) {
                // From RAM, neither side wrapping (the scratchpad at 16 KB, RAM at its end): one copy.
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
            put(m, &mut sadr, madr, qwc);
            if mode == 1 {
                let mut blocks = Vec::new();
                chain(m, 9, |_, upper, data, q, _| blocks.push((upper, data, q)));
                for (upper, data, q) in blocks {
                    if let Some(u) = upper {
                        let tte = (sadr & 0x3ff0) as usize;
                        m.spr[tte..tte + 8].copy_from_slice(&[u[0].to_le_bytes(), u[1].to_le_bytes()].concat());
                        sadr = (sadr + 16) & 0x3ff0;
                    }
                    put(m, &mut sadr, data, q);
                }
            } else {
                m.hw.ch[9][1] = madr.wrapping_add(qwc * 16);
            }
            m.hw.ch[9][8] = sadr;
        }
        _ => panic!("DMA channel {ch} kicked (CHCR {chcr:#x}, MADR {madr:08x}, QWC {qwc:#x}, TADR {:08x}): the port does not answer it", m.hw.ch[ch][3]),
    }
    let m = c.mach();
    m.hw.ch[ch][0] &= !0x100;
    m.hw.ch[ch][2] = 0;
    m.hw.d_stat |= 1 << ch;
    began
}

/// VIF0's codes: UNPACK into VU0's data memory, MPG into its micro memory, the setup codes; MSCAL runs a microprogram.
fn vif0(c: &mut Cpu, w: &[u32]) {
    let mut i = 0;
    while i < w.len() {
        let code = w[i];
        let cmd = (code >> 24) & 0x7f;
        let imm = code & 0xffff;
        let num = (code >> 16) & 0xff;
        i += 1;
        if matches!(cmd, 0x14 | 0x15 | 0x17) {
            // MSCAL, MSCALF, MSCNT: a microprogram starts (MSCNT at the address the last one stopped).
            let a = if cmd == 0x17 { c.mach().vu0_parked.unwrap_or(0) } else { (imm as usize) * 8 };
            c.vcallms(a as u32, 0);
            continue;
        }
        let m = c.mach();
        let v = &mut m.hw.vif0;
        match cmd {
            // NOP, FLUSHE, FLUSH, FLUSHA, MARK: nothing waits here.
            0x00 | 0x07 | 0x10 | 0x11 | 0x13 => {}
            0x01 => {
                v.cl = imm & 0xff;
                v.wl = (imm >> 8) & 0xff;
            }
            0x04 => v.itop = imm & 0x3ff,
            0x05 => v.mode = imm & 3,
            0x20 => {
                v.mask = w[i];
                i += 1;
            }
            0x30 | 0x31 => {
                let q = [w[i], w[i + 1], w[i + 2], w[i + 3]];
                if cmd == 0x30 { v.row = q } else { v.col = q }
                i += 4;
            }
            0x4a => {
                let n = if num == 0 { 256 } else { num } as usize;
                let mut changed = false;
                for k in 0..n * 2 {
                    let a = (imm as usize * 8 + k * 4) & 0xfff;
                    let b = w[i + k].to_le_bytes();
                    changed |= m.vu0m[a..a + 4] != b;
                    m.vu0m[a..a + 4].copy_from_slice(&b);
                }
                if changed {
                    m.vu0_gen += 1;
                }
                i += n * 2;
            }
            0x60..=0x7f => i += unpack(m, cmd, imm, num, &w[i..]),
            _ => panic!("VIF0 code {code:08x}: not one the port answers"),
        }
    }
}

/// UNPACK into VU0's data memory: the data words consumed.
fn unpack(m: &mut crate::Machine, cmd: u32, imm: u32, num: u32, w: &[u32]) -> usize {
    let vn = (cmd >> 2) & 3;
    let vl = cmd & 3;
    let masked = cmd & 0x10 != 0;
    let unsigned = imm & 0x4000 != 0;
    let num = if num == 0 { 256 } else { num };
    let v = m.hw.vif0;
    let (cl, wl) = (v.cl.max(1), v.wl.max(1));
    // Elements and their bits.
    let comps = (vn + 1) as usize;
    let bits = [32u32, 16, 8, 5][vl as usize];
    let total_bits = if vl == 3 { 16 * num } else { bits * comps as u32 * num };
    let nwords = total_bits.div_ceil(32) as usize;
    let bytes: Vec<u8> = w[..nwords.min(w.len())].iter().flat_map(|x| x.to_le_bytes()).collect();
    let mut addr = (imm & 0x3ff) as usize;
    let mut rd = 0usize;
    let elem = |rd: &mut usize| -> u32 {
        let x = match bits {
            32 => u32::from_le_bytes(bytes[*rd..*rd + 4].try_into().unwrap()),
            16 => {
                let h = u16::from_le_bytes(bytes[*rd..*rd + 2].try_into().unwrap());
                if unsigned { h as u32 } else { h as i16 as i32 as u32 }
            }
            _ => {
                let b = bytes[*rd];
                if unsigned { b as u32 } else { b as i8 as i32 as u32 }
            }
        };
        *rd += (bits / 8) as usize;
        x
    };
    for n in 0..num {
        let cyc = (n % wl) as usize;
        let mut vals = [0u32; 4];
        if vl == 3 {
            let h = u16::from_le_bytes(bytes[rd..rd + 2].try_into().unwrap()) as u32;
            rd += 2;
            vals = [(h & 0x1f) << 3, ((h >> 5) & 0x1f) << 3, ((h >> 10) & 0x1f) << 3, ((h >> 15) & 1) << 7];
        } else {
            for (k, val) in vals.iter_mut().enumerate().take(comps) {
                let _ = k;
                *val = elem(&mut rd);
            }
            if comps == 1 {
                vals = [vals[0]; 4];
            }
        }
        let qa = (addr & 0xff) * 16;
        for l in 0..4 {
            let mode = if masked { (v.mask >> (2 * (l + 4 * cyc.min(3)))) & 3 } else { 0 };
            let present = comps == 1 || l < comps || vl == 3;
            let val = match mode {
                0 => {
                    if !present {
                        continue;
                    }
                    match v.mode {
                        1 => vals[l].wrapping_add(m.hw.vif0.row[l]),
                        2 => {
                            let s = vals[l].wrapping_add(m.hw.vif0.row[l]);
                            m.hw.vif0.row[l] = s;
                            s
                        }
                        _ => vals[l],
                    }
                }
                1 => m.hw.vif0.row[l],
                2 => v.col[cyc.min(3)],
                _ => continue,
            };
            m.vu0d[qa + 4 * l..qa + 4 * l + 4].copy_from_slice(&val.to_le_bytes());
        }
        addr += 1;
        if cl > wl && cyc as u32 == wl - 1 {
            addr += (cl - wl) as usize;
        }
    }
    nwords
}
