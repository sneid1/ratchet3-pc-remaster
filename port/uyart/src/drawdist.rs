//! The DRAW DISTANCE setting's word to the culls (port/uya `settings.rs`): ORIGINAL, every cull as the game made it, or FAR,
//! each far limit that ends what is drawn raised to the farthest the game's own picture shows (`reach`). The port sets it
//! before each field (`set`); the units that read it are the tie cull (`tiecull`, 0x4AFA28) and the moby cull (`mobydraw`,
//! 0x43EA78), the culls whose lists the renderer draws. It lives here and not in `uycpu::Host`, whose layout the level libraries
//! are built against: no translated code reads it.
//!
//! The reach comes from the view struct (the tangents' struct, `uyafmt::levelcode::LevelGlobals::tangents` - 0xB0), the frame's
//! own values, in its units (view depth x 1024):
//! - the fog: func_L01_003FEBC8 copies it there each frame from the level's (the gameplay settings table +0x18 to +0x24, at
//!   0x1DFA68 on Veldin) or from the camera's own: the near and far depths at +0x228 and +0x22C, F at them at +0x238 and +0x23C,
//!   F linear between them and held beyond, the pixel (F x colour + (255 - F) x FOGCOL) / 256 (`uyafmt::gameplay::Fog`). A
//!   drawn thing is wholly the fog's colour where F is below 1 (F is truncated): past the depth where F reaches 1, when the far
//!   F is below 1 and the fog thickens with depth;
//! - the view's far clip at +0xA4 (1024 units as func_L01_003FF970 sets it): the plane the game's own projection ends at
//!   (view_projection_setup makes the projection at +0x100 from it and the near clip +0xA0, so a point past it is CLIP's +z).
//!   Past it the game draws nothing: the tie cull culls a tie whose class's 8 corners are all past it (0x4B039C) and program 3
//!   leaves a triangle with its three vertices past it undrawn (port/uya `tievu::decide`), a sprite with a corner past it is
//!   culled (0x405A58 at 0x405B8C).
//!
//! The reach is the nearer of the two. No story level's fog wholly hides anything: its far F is 33.15 (level 23) to 249.9, so
//! on every level the reach is the far clip.
//!
//! The other culls are the game's at FAR, because what they decide is not what the renderer draws, or their far limit is the
//! far clip itself: the terrain cull (0x4ADC98: the renderer draws every chunk that passes the zone test, at any depth); the
//! shrub cull (0x4A2200: the renderer culls the shrubs itself against the level's shrub limit, which it and the shrub cull
//! read as at least the reach, `range`); the effects' pool builder (0x45E810: the renderer runs its own copy of the cull on the pool,
//! `uyafmt::prog6::draw`); the sky (0x4A46C8: its clusters are culled by the view's sides alone); the sprites (0x405A58: their
//! far test is the far clip, never nearer than the reach).

use crate::Cpu;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

static FAR: AtomicBool = AtomicBool::new(false);

/// DRAW DISTANCE FAR (`true`) or ORIGINAL, from the next cull on.
pub fn set(far: bool) {
    FAR.store(far, Relaxed);
}

/// Whether the culls draw FAR.
pub fn far() -> bool {
    FAR.load(Relaxed)
}

/// The view struct's words the reach is made from: the far clip, the fog's near and far depths, F at each.
pub const WORDS: [u32; 5] = [0xa4, 0x228, 0x22c, 0x238, 0x23c];

/// FAR's reach from the view struct's `WORDS`, in its units (view depth x 1024): the depth past which the fog has wholly
/// hidden what is drawn, or the view's far clip where that is nearer or the fog never wholly hides anything. A word that is
/// no number stops the game: the view struct is not set.
pub fn reach(w: [u32; 5]) -> f32 {
    let [clip, near, far, near_f, far_f] = w.map(f32::from_bits);
    if ![clip, near, far, near_f, far_f].iter().all(|v| v.is_finite()) {
        panic!("draw distance: the view struct's far clip {clip}, fog depths {near} to {far} and F {near_f} to {far_f} are not numbers");
    }
    let hidden = if far_f < 1.0 && near_f > far_f && far >= near { near + (near_f - 1.0) / (near_f - far_f) * (far - near) } else { f32::INFINITY };
    hidden.min(clip)
}

/// FAR's reach for the view struct at `view`, None at ORIGINAL.
pub(crate) fn reach_of(c: &mut Cpu, view: u32) -> Option<f32> {
    if !far() {
        return None;
    }
    Some(reach(WORDS.map(|o| c.rd32(view.wrapping_add(o)))))
}

/// A check of a unit FAR changes (UYA_NATIVE_CHECK=`job`) about to run while FAR is on: it would compare FAR's lists with the
/// translation's, which are the game's own, so the game stops.
pub(crate) fn refuse_check(job: &str) {
    if far() {
        panic!("UYA_NATIVE_CHECK={job}: DRAW DISTANCE is FAR, which the native cull draws and its translation does not; the check compares the two only at ORIGINAL");
    }
}
