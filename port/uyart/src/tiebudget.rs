//! The room the port's tie ranges take in a frame's packets (the v0.1.6 player whose game stopped as Ratchet came down the
//! Aquatos sewer pipe, report 157911, and GitHub issues #4 and #5: "level 28: native: a tie program run with no instance record
//! unpacked before it", 46 and 49 fields into level 28).
//!
//! The game builds each frame's DMA chain in one of its two frame packet buffers, `BUFFER` bytes each (the words at `map + 0xC`
//! and `map + 0x10` hold their starts, `uyafmt::scenecode`), and nothing in it holds a frame to that end. The port's tie ranges
//! (`range`: every tie at its finest LOD and out to the level's limit, the player's fix of level 28's pop-in) and its wider view
//! (WIDESCREEN, FIELD OF VIEW) build far more than the PS2's 4:3 frame did. Level 28's arrival (UYA_PACKET_LOG): at 16:9 1,429
//! KB of 1,536 (the cull's records 46 KB, the chain the game builds from its lists 1,199 KB, after the tie pass 172 KB), against
//! 615 KB in all with the game's own values; at FIELD OF VIEW 110 1,735 KB, and at 32:9 more: the frame ran past its buffer into
//! the other one, over the chain the DMA was still sending, and the game stopped (`hw`'s "not a VIF code"; the renderer's
//! missing instance record). With the game's own values the widest view tried, 32:9 at FIELD OF VIEW 110, took 1,240 KB.
//!
//! The chain's bytes are the ties at their finest LOD near the camera (the arrival's are all within 256 units: with the finest
//! LOD to 128 units the frame took 1,254 KB), so the ranges keep to the room by distance, as the eye least misses them:
//! - Each level's frames keep each tie at its finest LOD out to `Costs::finest` units (`range::lod_distances`: the class's LOD
//!   bands moved out to it) and list the instances out to the level's limit (`Costs::extend`: the small ties seen far off),
//!   which follow the frames' packets (`Costs::frame`). A level begins with the finest LOD to `FINEST_START` units and the
//!   instances at their own distances, about the game's own frame (the widest view tried took the game's own 1,240 KB). While
//!   the frames take under `GROW_BELOW` of their buffer the instances' distances come back, then the reach grows; a frame over
//!   `SHRINK_ABOVE` (or one whose ties were cut, below) shrinks the reach at once, down to none (the classes' own LOD distances,
//!   the game's own frame), and with none the instances go back to their
//!   own distances for `HOLD` frames.
//! - A frame whose ties jump past the room before the reach has followed (a cut, a fast turn) has its last ties not listed
//!   (`Budget`): at the cull's start the room is the buffer's end less where the chain stands (`gp+0x4838`), less the most the
//!   level's recent frames built after the tie pass and `MARGIN`; each tie listed takes what a tie took in the level's last
//!   passes (`Costs::per_tie`: a pass's bytes, the cull's records and the chain, over its ties), so a frame like the last ones is
//!   never cut.

use std::collections::VecDeque;

/// A frame packet buffer's bytes.
pub const BUFFER: u32 = 0x18_0000;
/// Left free at a frame's end: a frame's packets after its tie pass may come out more than the level's recent most.
const MARGIN: i64 = 48 << 10;
/// The recent frames the packets after the tie pass are the most of (two seconds of play), and the recent passes a tie's cost
/// is the most of.
const WINDOW: usize = 120;
const PASSES: usize = 8;
/// Before a level's frames have measured them (its first pass: about the game's own frame): what the packets after the tie pass
/// take (the most measured, level 28 at 32:9 and FIELD OF VIEW 110: 580 KB) and what a tie takes (the game's own there: 1.4
/// KB).
const AFTER_FIRST: i64 = 600 << 10;
const PER_TIE_FIRST: i64 = 1536;
/// Frames the instances keep to their own distances once a frame's ties did not fit at the least reach.
const HOLD: u32 = 120;
/// The finest LOD's reach (units): as a level begins, and its most (at 0 the classes keep their own LOD distances: the game's
/// own frame, which the widest view tried holds).
const FINEST_START: f32 = 32.0;
const FINEST_MOST: f32 = crate::range::FINEST_FULL;
/// The shares of the buffer a frame's packets may take before `finest` shrinks (by `SHRINK`; under `FINEST_STEP` to 0), and under
/// which it grows (by `GROW`, at least `FINEST_STEP`, each frame).
const SHRINK_ABOVE: u32 = BUFFER / 100 * 88;
const GROW_BELOW: u32 = BUFFER / 100 * 75;
const SHRINK: f32 = 0.7;
const GROW: f32 = 1.15;
const FINEST_STEP: f32 = 8.0;

/// The most of the last `N` values.
#[derive(Default)]
struct Recent<const N: usize>(VecDeque<i64>);

impl<const N: usize> Recent<N> {
    fn push(&mut self, v: i64) {
        self.0.push_back(v);
        if self.0.len() > N {
            self.0.pop_front();
        }
    }

    fn most(&self) -> Option<i64> {
        self.0.iter().copied().max()
    }
}

/// A tie pass: the chain pointer before the cull and after it (the cull's records), the ties it listed and left out, and the
/// pointer as the shrub pass began (with the chain the game built from the cull's lists).
#[derive(Clone, Copy)]
struct Pass {
    before: u32,
    cull_end: u32,
    listed: u32,
    out: u32,
    finest: f32,
    extend: bool,
    end: Option<u32>,
}

/// A level's measures (bytes), from its frames (`tie_pass`, `shrub_pass`, `frame`), and its finest LOD's reach.
pub(crate) struct Costs {
    level: Option<u32>,
    /// The chain pointer's word (gp+0x4838), as the last tie cull named it.
    ptr: Option<u32>,
    per_tie: Recent<PASSES>,
    after: Recent<WINDOW>,
    finest: f32,
    /// Whether the instances are listed out to the level's limit, and the frames left before they may be again.
    extend: bool,
    hold: u32,
    passes: VecDeque<Pass>,
}

impl Default for Costs {
    fn default() -> Costs {
        Costs { level: None, ptr: None, per_tie: Recent::default(), after: Recent::default(), finest: FINEST_START, extend: false, hold: 0, passes: VecDeque::new() }
    }
}

impl Costs {
    /// The level the next tie pass is in: a new level's measures are taken again.
    fn level(&mut self, level: u32) {
        if self.level != Some(level) {
            *self = Costs { level: Some(level), ..Costs::default() };
        }
    }

    /// The chain pointer's word, once a tie cull has named it.
    pub(crate) fn ptr(&self) -> Option<u32> {
        self.ptr
    }

    /// A tie cull ran with the budget `b`, the chain pointer at word `ptr` `before` it and `after` it.
    pub(crate) fn tie_pass(&mut self, ptr: u32, before: u32, after: u32, b: &Budget) {
        self.ptr = Some(ptr);
        self.passes.push_back(Pass { before, cull_end: after, listed: b.listed, out: b.out_count, finest: b.finest, extend: b.extend, end: None });
        if self.passes.len() > 4 {
            self.passes.pop_front();
        }
    }

    /// The shrub pass begins with the chain pointer at `at`: the last tie pass's end, and from its bytes a tie's.
    pub(crate) fn shrub_pass(&mut self, at: u32) {
        let Some(p) = self.passes.back_mut().filter(|p| p.end.is_none()) else { return };
        p.end = Some(at);
        if p.listed > 0 {
            let n = i64::from(p.listed);
            self.per_tie.push((i64::from(at.wrapping_sub(p.before)) + n - 1) / n);
        }
    }

    /// A frame's own chain words ran from `lo` to `hi` (`hw`): what was built after its tie pass, and the finest LOD's reach for
    /// the frames after it.
    ///
    /// Only a tie pass that ran in the frame (its start and its end inside the frame's words) measures it, once. A frame with
    /// none of its own (a pause's: its menus run no tie cull) was measured by the last pass before the pause whose start its
    /// words spanned: "after the tie pass" was the pause frame's whole packets, or with the pass's end past the frame's, a
    /// wrapped 4 GB. That stayed the most of `after` for its 120 frames, every tie cull's room was below nothing, no tie was
    /// listed, and the game drew no ties for two seconds after the pause menu closed (the player on Daxx, 28 November 2026:
    /// "When I click menu and then close it objects dissapear", "Half the world objects are gone"; the frame builder had 0 ties
    /// after the close, and they came back by themselves 1 to 4 s later).
    pub(crate) fn frame(&mut self, lo: u32, hi: u32) {
        let Some(k) = self.passes.iter().rposition(|p| p.before >= lo && p.before <= hi && p.end.is_none_or(|e| e >= p.before && e <= hi)) else { return };
        let p = self.passes.remove(k).expect("found above");
        if let Some(end) = p.end {
            self.after.push(i64::from(hi - end));
        }
        let used = hi.wrapping_sub(lo);
        self.hold = self.hold.saturating_sub(1);
        if used > SHRINK_ABOVE || p.out > 0 {
            if p.finest > 0.0 {
                let less = p.finest * SHRINK;
                self.finest = if less < FINEST_STEP { 0.0 } else { less };
            } else if p.extend {
                (self.extend, self.hold) = (false, HOLD);
            }
        } else if used < GROW_BELOW {
            if !self.extend {
                self.extend = self.hold == 0;
            } else {
                self.finest = (self.finest * GROW).max(self.finest + FINEST_STEP).min(FINEST_MOST);
            }
        }
    }

    /// UYA_PACKET_LOG: the tie pass of the frame from `lo` to `hi`, split.
    pub(crate) fn split(&self, lo: u32, hi: u32) -> String {
        let Some(p) = self.passes.iter().rev().find(|p| p.before >= lo && p.before <= hi) else { return String::new() };
        let kb = |x: u32, y: u32| y.wrapping_sub(x) as i32 / 1024;
        let end = p.end.map_or("?".to_string(), |e| format!("{} KB, after {} KB", kb(p.cull_end, e), kb(e, hi)));
        format!(": before the tie cull {} KB, the cull {} KB ({} ties, the finest LOD to {:.0} units, {}, {} left out), the tie chain {end}", kb(lo, p.before), kb(p.before, p.cull_end), p.listed, p.finest, if p.extend { "out to the level's limit" } else { "at their own distances" }, p.out)
    }
}

/// One tie cull's room (`tiecull`).
pub(crate) struct Budget {
    left: i64,
    per_tie: i64,
    /// The finest LOD's reach this pass (`range::lod_distances`), and whether the instances are listed out to the level's limit
    /// (`range::TieView::byte`).
    pub(crate) finest: f32,
    pub(crate) extend: bool,
    listed: u32,
    out_count: u32,
}

impl Budget {
    /// At a tie cull's start, in `level`, the chain pointer at `fp`: the room left in the frame packet buffer it lies in (`starts`:
    /// the two buffers'), None where it lies in neither.
    pub(crate) fn new(costs: &mut Costs, level: u32, starts: [u32; 2], fp: u32) -> Option<Budget> {
        costs.level(level);
        let start = starts.into_iter().find(|&s| fp >= s && fp - s < BUFFER)?;
        let left = i64::from(start + BUFFER - fp) - costs.after.most().unwrap_or(AFTER_FIRST) - MARGIN;
        Some(Budget { left, per_tie: costs.per_tie.most().unwrap_or(PER_TIE_FIRST), finest: costs.finest, extend: costs.extend, listed: 0, out_count: 0 })
    }

    /// Whether the next tie the cull would list is passed over: the room cannot hold it.
    pub(crate) fn next_out(&mut self) -> bool {
        i64::from(self.listed + 1) * self.per_tie > self.left
    }

    /// A tie listed.
    pub(crate) fn listed(&mut self) {
        self.listed += 1;
    }

    /// A tie passed over (`next_out`).
    pub(crate) fn left_out(&mut self) {
        self.out_count += 1;
    }
}
