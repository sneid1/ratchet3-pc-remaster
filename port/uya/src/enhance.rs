//! The remaster's graphics options (PLAN item 8) on the renderer's side, process-wide like a game's options: the values of the
//! GRAPHICS tab's options (`settings::OPTIONS`), set by `play` when the player changes them and by `uya native --set`. Each is
//! off (0) by default, and with every one off the picture is the game's.
//!
//! - `texture_filtering`: off, the GS's own filtering of the game's textures (their levels picked from Q and TEX1 K for the
//!   PS2's 448 lines); 2x to 16x, every one of the game's textures sampled at the level its own texel density on screen picks,
//!   trilinear, anisotropic up to that many samples. A pack replacement (`texnames`) is always drawn trilinear at 16x.
//! - `ambient_occlusion` (`ao.rs`): low and high, on the level's static world and its mobies.
//! - `lighting` (`lighting.rs`): per pixel, the game's own light passes evaluated at every pixel.
//! - `rift_apart_surfaces` (`texnames`, `lighting::MATERIAL`): on, the surfaces the pack folder's `0-riftapart` pairs (PLAN 9.2)
//!   drawn with their Rift Apart material on the game's own texture (the pack's): its grain, its relief and its gloss, lit per
//!   pixel; off, the packs as before.

use std::sync::atomic::{AtomicU8, Ordering};

const NONE: AtomicU8 = AtomicU8::new(0);
static LEVELS: [AtomicU8; crate::settings::OPTIONS.len()] = [NONE; crate::settings::OPTIONS.len()];

/// The renderer's options from now on: those of `s`.
pub(crate) fn set(s: &crate::settings::Settings) {
    for (l, v) in LEVELS.iter().zip(s.values) {
        l.store(v, Ordering::Relaxed);
    }
    // The per-pixel lighting of the mobies takes the skin job's joint matrices (`mobies::Mobies::skin_pass`).
    uyart::SKIN_JOINTS.store(lighting(), Ordering::Relaxed);
}

/// Option `key`'s value now (its place in its `values`).
pub(crate) fn level(key: &str) -> u8 {
    LEVELS[crate::settings::index(key)].load(Ordering::Relaxed)
}

/// FRAME GENERATION: frames DLSS-G is to generate per picture the port draws (0 off, 1 for 2X, 2 for 3X, 3 for 4X). What it
/// is generating now is `play::FG_GENERATED`.
pub(crate) fn frame_generation() -> u32 {
    u32::from(level("frame_generation"))
}

/// FRAME PACING UNCAPPED (`play.rs` `Timeline`): the pictures drawn back to back, each shown once done.
pub(crate) fn uncapped() -> bool {
    level("frame_pacing") == 0
}

/// Whether the updated textures' surfaces are there to draw (`texnames::open`: the updated-textures folder found and read).
static RIFT_SURFACES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The updated textures' surfaces read, or not there (`texnames::open`); the skin job's joint matrices follow (`set`).
pub(crate) fn set_rift_surfaces(there: bool) {
    RIFT_SURFACES.store(there, Ordering::Relaxed);
    uyart::SKIN_JOINTS.store(lighting(), Ordering::Relaxed);
}

/// Whether the pack folder's `0-riftapart` replacements are drawn (`texnames`, `rift_apart_surfaces`): the option on and the
/// updated textures there. Without them (a release installed without the updated-textures volumes, the usual way) the option,
/// on by default, drew nothing new but still lit every pixel (`lighting`): the world's per-pixel lighting and the mobies' skin
/// job capture, three times the frame's jobs a field, at a cost a weak PC could not pay, with LIGHTING at VERTEX (two v0.1.3
/// players without the volumes: a Ryzen 5 4600G's integrated graphics at 20 to 22 pictures a second, a GTX 1650 Ti).
pub(crate) fn rift_apart() -> bool {
    level("rift_apart_surfaces") == 1 && RIFT_SURFACES.load(Ordering::Relaxed)
}

/// The anisotropy the game's own textures are sampled with (`texture_filtering`): None, the GS's own filtering.
pub(crate) fn aniso() -> Option<u16> {
    match level("texture_filtering") {
        0 => None,
        l => Some(1 << l),
    }
}

/// The water reflections' level (`reflections.rs`): 0 off, 1 low (the reflection at half the picture's size), 2 high.
pub(crate) fn reflections() -> u8 {
    level("water_reflections")
}

/// The light shafts' level (`shafts.rs`): 0 off, 1 low, 2 high.
pub(crate) fn shafts() -> u8 {
    level("light_shafts")
}

/// The ambient occlusion's level: 0 off, 1 low, 2 high.
pub(crate) fn ao() -> u8 {
    level("ambient_occlusion")
}

/// The real-time shadows' level: 0 off, 1 low, 2 medium, 3 high (`shadows.rs`: the maps' size), 4 ray traced
/// (`raytrace::RAY_TRACED`).
pub(crate) fn shadows() -> u8 {
    level("shadows")
}

/// The ray-traced lighting's level (`rtlight.rs`): 0 off, 1 low, 2 high.
pub(crate) fn rt_lighting() -> u8 {
    level("ray_traced_lighting")
}

/// The ray-traced reflections' level (`rtlight.rs`): 0 off, 1 low, 2 high.
pub(crate) fn rt_reflections() -> u8 {
    level("ray_traced_reflections")
}

/// Whether the FPS counter is shown (`play.rs` `Fps`, the DISPLAY tab's FPS COUNTER).
pub(crate) fn fps_counter() -> bool {
    level("fps_counter") == 1
}

/// DLSS's level (`dlss.rs`): 0 off, 1 DLAA, 2 quality, 3 balanced, 4 performance.
pub(crate) fn dlss() -> u8 {
    level("dlss")
}

/// Whether the world is lit per pixel (`lighting.rs`): the LIGHTING option at PER-PIXEL, or the Rift Apart surfaces on (their
/// materials' relief and gloss are lit at every pixel, `lighting::MATERIAL`).
pub(crate) fn lighting() -> bool {
    level("lighting") > 0 || rift_apart()
}
