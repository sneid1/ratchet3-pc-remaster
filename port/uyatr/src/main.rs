//! `uyatr DISC BOUNDS OUT PROGRAM...`: translates the named programs of Up Your Arsenal into Rust crates under OUT, for the port's
//! runtime (`port/uyart`), and the game's VU0 and VU1 microprograms (`OUT/vu0`, `OUT/vu1`: `vu0mp.rs`, `vu1mp.rs`; `uyatr DISC
//! BOUNDS vu1 OUT` writes `OUT/vu1` alone). PROGRAM is `loader`, `boot`, `fe100`, `fe101`, `lNN` (a story level) or `levels`
//! (every story level: 1-14, 16-24, 26-36). BOUNDS is the function-bounds file of `scripts/uya_bounds.py`. The output is game
//! code: it stays on this PC (OUT is gitignored), and the port depends on it. Run from `port/`.
//!
//! The crates are compiled against `port/uycpu` only. `OUT/game` (`uya-game`, a dependency of the game's executable) links the
//! loader, the boot program and the front ends and lists every program the run translated; each story level is a library of
//! its own, `OUT/levels/lNN` (`tr-lNN`, a cdylib in the levels' own workspace `OUT/levels`, built by `scripts/build-levels.ps1`
//! into the port's target folder: `tr_lNN.dll` beside the executable),
//! which the runtime loads when the level's overlay is in RAM (`uyart::machine`). All 34 levels in the one executable would be
//! about 1,700 crates and past 2 GB of code (PLAN 0.3, 5.3).
//!
//! Every function in the bounds becomes Rust (`emit.rs`); the gaps between them that hold decodable code become functions too (a
//! piece the bounds split off runs on into the next). The runtime's table maps every label to its function: the entries, every
//! branch or call target, every code address a data word or a `lui`/`addiu` pair holds (jump tables, callbacks). A call the table
//! has no translation for stops the game with the address.

mod decode;
mod emit;
mod programs;
mod vu0mp;
mod vu1mp;

use programs::Program;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The code of a crate: the functions that start in one 0x5000-byte window of the program's text (about 5,000 instructions),
/// crate N the N-th window (`lNN_NNN`). Crates build in parallel and in bounded memory (a rustc on one peaks near 0.5 GB), and the
/// cut does not move when the translation of one function changes, so a translator fix rebuilds only the crates it touches.
const CHUNK_BYTES: u32 = 0x5000;

/// The levels' workspace (`OUT/levels/Cargo.toml`), before its members line.
const LEVELS_WORKSPACE: &str = "# Written by port/uyatr: the story levels' libraries, a workspace of their own (game code from the player's disc, never
# committed). scripts\\build-levels.ps1 builds them all in one cargo run into the port's target folder, beside the executable that
# loads them.
[workspace]
resolver = \"3\"
";

/// The levels' build profile (PLAN 5.3). The translated code is about 1,800 crates and most of what the port compiles.
const LEVELS_PROFILE: &str = "# opt-level 3: the game's own speed (59.94 fields a second). One codegen unit a crate: every function of a crate, and the
# generic helpers it instantiates (uycpu's MMI and VU0 operations and the std array helpers inside them), in one LLVM module, so
# they inline; with 16 units and no LTO they did not (separate try_map and closure calls on every operation), and Veldin's game
# code took 13 ms a field instead of 6.8 (a CPU profile of the library). One unit also leaves no LTO pass to run, and the
# crates' number keeps every job busy. debug 0: line tables for every inlined load and store are most of the debug info, its
# LLVM time, memory and PDB merging; a stop names game addresses. strip none: with debug 0 cargo would strip, and rustc would
# pass /DEBUG:NONE; the PDB's public symbols keep function names in profiles and crash stacks. panic unwind: a stop raised in
# the runtime unwinds through a library's frames to the executable's catch_unwind (abort would end the process there).
[profile.release]
opt-level = 3
codegen-units = 1
debug = 0
strip = \"none\"
incremental = false
panic = \"unwind\"
";

/// The story levels (`levels`): PLAN.md, "The disc map" (38 has an AUDIO file only; 39-55 are multiplayer, out of scope).
const LEVELS: [u32; 34] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36];

/// The game functions the runtime answers natively instead of running their code: the EE's side of the IOP (whose own
/// processor and modules the port does not run; the servers are native, `uyart::iop`) and the hardware the port has no model of
/// (the IPU's movie player). Each is (what, its size in words, its `shape`, its number for `uycpu::Rt::native`): found by its
/// code wherever it is linked (the boot program, a front end, every level's own copy).
const NATIVE: &[(&str, usize, u64, u32)] = &[
    // libkernel InitSys's search of the BIOS kernel's code (FindAddress over 0x80000000..0x80080000 until two searches agree,
    // 0x124a80): the port's kernel is native, there is no BIOS image to find, and the address it stores (0x13E2F8) is read by
    // no code of the game.
    ("InitSys's BIOS scan", 63, 0xa2f3b341a15b0be9, 0),
    // The movie player (front end 0x3afaa8 and each level's copy): it plays an MPEG-2 stream on the IPU and returns 1 when
    // Start ended it, 0 when it played to its end, clearing bit 0x80 of 0x1D4D40 on the way out.
    ("the movie player", 113, 0x6116a51b29014915, 1),
];

/// The game functions the runtime does natively in front of their translation (`uyart::mobydraw`): (what, size in words,
/// `shape`, the runtime's number for `Rt::native`). The translation stays as `f…_body`: the wrapper at the function's entry asks
/// the runtime with `Rt::native` N (`c.ret` the function's address with 1 in its low bit, and 2 when the program's view is
/// widened: a story level), which runs the function and leaves its return address in `c.ret`, or leaves 0 there for the
/// translation to run; after the translation ran it asks with N + 1 (UYA_NATIVE_CHECK: the runtime runs its own from the
/// state before and compares).
const JOBS: &[(&str, usize, u64, u32)] = &[
    // The moby draw path (level 1 0x438120, func_003BE340): the chain's start, the moby cull and draw list (0x43EA78), then
    // its second half. In the Annihilation Nation arena's fight its translated code under it took 0.91 ms a field besides the
    // skin job (captures/native-port/plan.md).
    ("the moby draw path", 23, 0x8da6_98fe_09bd_a31f, 4),
    // Its second half (level 1 0x4380C0, func_003BE2E0), which 0x4009B8, 0x497828 and 0x4BCCE0 call on their own: the classes'
    // chain, the draw records (0x43E998: the joint matrices 0x439E00 and the skin job 0x3F6FF0 for each, whose only caller this
    // is), the second list (0x437B48), the class texture uploads (0x437C88).
    ("the moby draw path's second half", 24, 0x6f64_e46f_2904_70c6, 6),
    // The collision sweeps (`uyart::collide`), with the grid lookup they call (level 1 0x3F83A8, 40 words, which no code
    // outside them calls) inside: coll_line (level 1 0x3F8448, fe101 0x3CE740), coll_sphere_push (0x3F9498), its two-radius
    // form (0x3FA2D8) and ball_damage (0x3FB1D0, fe101 0x3CF790); in every story level, the front end has the first and last.
    // 0.31 ms a field of the arena fight's translated time (captures/native-port/plan.md, unit 4).
    ("coll_line", 1044, 0xd6e5_ae9f_ba65_4b91, 8),
    ("coll_sphere_push", 912, 0x1f1d_ca39_eb73_292d, 10),
    ("the two-radius sphere push", 958, 0x111f_fe5f_eb21_ec38, 12),
    ("ball_damage", 586, 0x011a_dd0e_952d_41d5, 14),
    // The shrub cull (level 1 0x4A2198, called by the frame at 0x401214, and all it calls: FlushCache, the cull and chain
    // builder 0x4A2200, the light list's copy 0x40BDE0, the chain's end 0x4A1F88 with 0x4A3AD8 and 0x4C21C0; `uyart::shrubcull`).
    // One in each story level and front end, and nothing else of its size and shape.
    ("the shrub cull", 26, 0xcc74_8a72_21a7_57ae, 16),
    // The shrubs' lighting (level 1 0x4A3D80, called by the frame at 0x40179C and by the level's setup at 0x4262D4), with the
    // shrub VU0 program's runs from micro 0x0 and 0x160 done natively.
    ("the shrubs' lighting", 364, 0x5cf1_879f_62cc_3fdf, 18),
    // The tie cull (level 1 0x4AFA28, `uyart::tiecull`; its caller 0x4AF6D0 is the whole tie pass, so the entry is the cull) and
    // the terrain cull (level 1 0x4ADC98; its caller 0x4ADBC0 also makes the terrain's matrix and handles the clipped chunks),
    // hand-written leaves with `VIEW_LOADS` loads at +0x198 and +0x10c. One of each in each story level and front end (the same
    // sizes and shapes as their `VIEW_LOADS` entries).
    ("the tie cull", 975, 0xf402_bbd6_ce91_d229, 20),
    ("the terrain cull", 770, 0x773c_dcd8_44c6_2420, 24),
    // The effects' pool builder (level 1 0x45E810, frontbin func_003C7E68, with the texture placer 0x45F438 at its end, which
    // only its three BGEZALs call; `uyart::fxpool`): the effects layer's cull, depth sort and VU1 program 6's chain. The frame
    // calls it (0x4010D0 at 0x401354, 0x4D3460 at 0x4D3564). One in each story level and front end, and nothing else of its
    // size and shape; its view load (+0x4C) is `VIEW_LOADS`', and the native unit widens it alike.
    ("the effects' pool builder", 778, 0xf36e_a424_d839_ae4d, 28),
    // The sky's frame (level 1 0x4A46C8, frontbin func_003C8C40; called by 0x405768 and its copy 0x4D36B8 between the chain's
    // start and end): each layer's rotation (0x4A45B0, 0x40C678) and draw (0x4A48F8: 0x4A4958 or 0x4A4B00, with the cluster
    // cull 0x4A5220, the clip matrix 0x4A4C98, the vertices 0x4A4D40, the triangle lists 0x4A4F38 and 0x4A4E38, the GS
    // registers 0x4C1FE8; `uyart::skycull`). One in each story level and front end, and nothing else of its size and shape.
    ("the sky", 38, 0xb022_e302_7abc_056d, 32),
    // The sprite's cull and draw (level 1 0x405A58, frontbin func_003CD830, hand-written; 231 calls in level 1's code): the
    // corners' guard-band cull, the strip, or the near-plane clip (0x4060D8 with 0x4057C0, 0x405938, 0x4059F8) and its fan,
    // the GIF packets (0x406098, 0x406018). One in each story level and front end, and nothing else of its size and shape.
    ("the sprites", 368, 0x12ac_93c1_2098_005c, 36),
    // Animation (`uyart::anim`, captures/native-port/plan.md unit 3): the layer update (level 1 0x436D30), which calls the frame
    // unpack (0x4397F8) and the pose build (0x43B7D0), and the six leaves the game calls on their own: 0x43AAF8, 0x43CF90,
    // 0x43D9D8, 0x4389D8 and the frame readers 0x436500 and 0x4365E0. Only the layer update calls anything (the two above,
    // 0x437200, 0x43E558 and boot's FlushCache 0x11F0A0); each of the others is a leaf. The same code in every story level.
    ("the layer update", 246, 0x3dcc_c7f3_ebf9_b164, 40),
    ("the frame unpack", 386, 0x4f9c_f6a7_117c_af85, 42),
    ("the pose build", 534, 0xe7a9_9517_9b8d_a1b8, 44),
    ("the animation leaf 0x43AAF8", 822, 0x5817_dab6_58f8_f8ed, 46),
    ("the animation leaf 0x43CF90", 658, 0x6d7d_fa2f_c761_dcd0, 48),
    ("the animation leaf 0x43D9D8", 672, 0x4184_854f_87c1_e929, 50),
    ("the animation leaf 0x4389D8", 196, 0x5ad0_1fa2_575b_d1fb, 52),
    ("the frame reader 0x436500", 55, 0x48db_4beb_3ea1_0fd2, 54),
    ("the frame reader 0x4365E0", 50, 0xf92e_35e2_925a_2b7a, 56),
];

/// The loads of the view's horizontal tangent (the view struct's +0xB0, `uyafmt::levelcode::LevelGlobals::tangents`) that make
/// the picture and decide what is drawn: each reads the tangent times the host's `view_k` (`uycpu::Host`: the screen's width
/// over the original picture's) and its field of view factor `fov`, so on a wider screen the game's own view, its frustum and
/// its culling reach further to the sides at the same vertical view, and the game's field of view itself (what its camera,
/// cutscene and menu code set and restore) is left as it is. Each entry is (what, the function's size in words, its `shape`,
/// the loads' offsets in it): found by its code in every program, the same function in every level and both front ends. The
/// vertical tangent (+0xB4) is never widened (the view widens at the same vertical view): a quadword load here reads it times
/// `fov` alone, and its own loads where the view is made are `FOV_LOADS`. Which loads are here and why, and which are not:
/// PLAN 4.2's notes in docs/HANDOVER.md and docs/widescreen-loads.md.
const VIEW_LOADS: &[(&str, usize, u64, &[u32])] = &[
    // view_projection_setup (frontbin func_003830E8, func_L01_003FFAC8): the loads it makes the frustum from (+0xB8 =
    // 1/cos(atan h) for the frustum quadword +0x1F0, and its guard band factor +0x1F8 from h times +0xA8), its copy of (h, v,
    // +0xB8, +0xBC) to 0x1E78C0, and the projection (+0xC0 = +0x210 / (h near), with 0x1DA06C from the same load). Not +0x70,
    // which v = 0.775 h is made from (so v stays the game's), nor +0x1C4, its test of h against 0.62 for the zoomed view's
    // factor (the game's field of view, not the view).
    ("view_projection_setup", 469, 0xa23f323b7aa3bc57, &[0xf4, 0x138, 0x278, 0x3bc]),
    // The culls, each loading (h, v, +0xB8, +0xBC) into a VU0 register for its frustum test: the moby cull and draw list
    // (func_L01_0043EA78; its drawn byte: `DRAWN`), the effects' pool builder (func_L01_0045E810), the shrub cull
    // (func_L01_004A2200), the terrain's (func_L01_004ADC98) and the tie cull (func_L01_004AFA28).
    ("the moby cull", 1680, 0xff82413af542ecfa, &[0x214]),
    ("the effects' pool builder", 778, 0xf36ea424d839ae4d, &[0x4c]),
    ("the shrub cull", 1590, 0x5ec90fa54ad347ba, &[0x178]),
    ("the terrain cull", 770, 0x773cdcd844c62420, &[0x10c]),
    ("the tie cull", 975, 0xf402bbd6ce91d229, &[0x198]),
    // The water's (func_L02_00310500, in levels 2, 4, 5, 9, 12, 14, 17 to 20, 22 and 29): the part of the water plane drawn, from
    // the frustum's corner rays (+-h, +-v, 1) in the camera's octant. Not func_L02_0030FC10's h, its texture's mip level K (the
    // texel size on screen, which the vertical view sets).
    ("the water's extent", 903, 0xba79f58bf3b7f642, &[0x198, 0x1b0, 0x1d4, 0x1f0, 0x2c4]),
];

/// The loads of a view tangent read times the field of view's factor alone (`uycpu::Host::fov`, `Cpu::fov`; PLAN 10, the field
/// of view setting): with `VIEW_LOADS` (whose horizontal loads read it too) the game's view is made at the player's field of
/// view, both tangents scaled alike, and what the game decides from the view follows it, so nothing pops in at the edges and
/// what is drawn is what is targeted. Its field of view state is not touched: the loads that save, set and restore it, the
/// zoomed view's test (view_projection_setup +0x1C4) and the 2D layer's models (0x4C7BF8, 0x4C7FF0) read the tangents as
/// written. Each entry as `VIEW_LOADS`'s, each load an `lwc1`.
const FOV_LOADS: &[(&str, usize, u64, &[u32])] = &[
    // view_projection_setup's loads of v (+0xB4, v = 0.775 h as it wrote it at +0x78): +0x11C for +0xBC = 1/cos(atan v), +0x158
    // with +0xAC for the guard band's factor +0x1F8, +0x3D0 for the projection's y.
    ("view_projection_setup", 469, 0xa23f323b7aa3bc57, &[0x11c, 0x158, 0x3d0]),
    // The water's extent's v (its corner rays (+-h, +-v, 1); the h loads are `VIEW_LOADS`').
    ("the water's extent", 903, 0xba79f58bf3b7f642, &[0x194, 0x1b4, 0x1d0, 0x1ec, 0x2a4]),
    // The level of detail's zoom (func_L01_004AD960: 0.62 / h clamped to 1..4, 0x1E0F28): the size of things on screen.
    ("the level of detail's zoom", 44, 0xa59de5dfb678f84f, &[0x10]),
    // The targeting cone (func_L01_003444B0 +0x61C: 2 atan h, kept with its list of drawn targets).
    ("the targeting cone", 935, 0x6a359ed034066466, &[0x61c]),
    // The water texture's mip level (func_L02_0030FC10 +0x250: TEX1 K from h x near, the texel's size on screen).
    ("the water's mip level", 215, 0xaf6d1a5fed8b1cac, &[0x250]),
];

/// The loads of the game's camera speed option (camera_speed_options, 0x143AD0: a word per camera setting and player, 50 in a new
/// game; the options menu sets it) where its cameras turn by the right stick: each reads the word times the camera speed
/// setting's factor (`uycpu::Host::cam_speed`, `Cpu::cam_speed`; PLAN 10), so the right stick turns the camera that much faster
/// or slower than the game's own option makes it, its top speed too. The options menu's and the level start's reads (0x49C54C,
/// 0x49D7B8, 0x4DBD08 on Veldin) are the option itself and read it as written. Each an `lwc1` of the integer word, converted
/// after (`cvt.s.w`).
const CAMERA_SPEED: &[(&str, usize, u64, &[u32])] = &[
    // The follow camera's update (Veldin 0x3E8850, by pointer, each frame): speed / 100 x 0.0567 + 0.0218 into 0x1D5FF0 + 4 x
    // player, which camera_stick_input (0x3E7478) takes as its turn speed on a push of the stick.
    ("the follow camera's turn speed", 180, 0xb9509fde839e4d2f, &[0x68]),
    // fps_cam_update (0x410B08, the lock-strafe and first person cameras' turn, from hero_camera_update each frame): its turn
    // rates times lerp(0.25, 1.25, speed / 64).
    ("the free-look camera's turn", 617, 0x16be66ff6b40c487, &[0xec]),
];

/// The hero's and its camera's reads of the camera option (camera_setting, 0x1439F4 + player: 0 third person, 1 lock-strafe, 2
/// first person; the options menu sets it): each reads lock-strafe while the keyboard and mouse play (`uycpu::Host::aim`,
/// `Cpu::camera_setting`; PLAN 10, the mouse aim: the player, 15 November 2026, "Aiming fixes shouldnt be optional"), so the
/// game runs its own lock-strafe camera, the hero facing where the camera looks and his guns bent to the point its ray hits
/// (aim_bend_to_point, Veldin 0x4C7010), and the mouse turns that camera (`rtgame.rs`). The button remaps read it the same way
/// (pad_button_for 0x45E3B8 and hero_pad_button_for 0x50EE20: in lock-strafe R1's actions move to R2, L1's to L2, and Cross's
/// also take L1), so the keyboard and mouse play the game's own lock-strafe layout: R2 crouches, R1 and Circle fire, L2 is the
/// first-person aiming view (the bindings' defaults, uya's `bindings.rs`). With them read as written (until 15 November 2026) the hero's code was
/// lock-strafe and the remaps third person, so R1 both crouched (pad_button_for(R1) is R1 in third person) and fired
/// (hero_attack_start reads R1 as fire in lock-strafe): Shift crouched and shot at once (the player: "SHFT shoots gun instead of
/// just being crouch"; with every read lock-strafe R2 crouched with no shot, R1 fired, L2 aimed, L1 jumped: captures/s2 of the
/// input-menus worktree). Read as written: the options menu's (the player's own setting, for a pad), and the follow camera's
/// (camera_stick_input 0x3E7478 and its update 0x3E8850, which pick their invert and speed options by it: the follow camera
/// runs where the hero's camera may not, and keeps its own options). Each an `lbu` of 0x1439F4 or of the options' base 0x143950
/// (plus the player) + 0xA4: every one in the 34 story levels' code but those (captures/tools/camsites.py lists them, function
/// by function); the hero's code is not the same function in every level (the levels each function is in follow it).
const CAMERA_SETTING: &[(&str, usize, u64, &[u32])] = &[
    // The button remaps (Veldin's addresses; the same code in every level that has them).
    ("pad_button_for (Veldin 0x45e3b8)", 30, 0x2f994a400a345038, &[0xc]),
    ("hero_pad_button_for (Veldin 0x50ee20)", 28, 0x57cbeddcbc684651, &[0x4]),
    ("flux_scope_update (Veldin 0x3444b0)", 935, 0x6a359ed034066466, &[0x7f0]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("Veldin 0x3b1618", 91, 0x4b6b954758f4a1f9, &[0x38, 0xf4]), // levels 1,4,9,12,16,18,20,22,26,29
    ("Veldin 0x496498", 138, 0x1b3926defaea2a21, &[0x10]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("Veldin 0x49ba68", 308, 0x2b5886beb19489f7, &[0x40, 0x408]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("Veldin 0x4a93c0", 101, 0x35b27d28281f3c05, &[0x94]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("(sets hero_camera_mode) (Veldin 0x4a9f80)", 1506, 0x00d9ded42c3c36df, &[0x1590]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("Veldin 0x4cd1d0", 540, 0x052a984683780fde, &[0x68]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("hero_level_start (Veldin 0x4dbb60)", 289, 0x81a80477dba6b67e, &[0x160, 0x170]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("hero_fire_anim (Veldin 0x4e6a08)", 169, 0xd05ce139d9b816b1, &[0x148]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("the hero's update (Veldin 0x4f2090)", 8648, 0xc5bb0c8185e7b11b, &[0x7054]), // levels 1,3,5,6,7,10,11,12,13,14,16,17,18,20,21,22,24,26,28,29
    ("hero_timers_update (Veldin 0x4faa68)", 683, 0xff33e35d6f33376a, &[0x3d8]), // levels 1,3,4,5,6,7,8,10,11,12,13,14,16,17,18,19,20,21,22,24,26,27,28,29
    ("Veldin 0x4fc518", 4733, 0x021340372d2cb298, &[0x1f4c, 0x27fc, 0x2bf4]), // levels 1,3,5,6,7,10,11,12,13,14,16,17,18,20,21,22,24,26,28,29
    ("hero_attack_start (Veldin 0x501678)", 827, 0x0a4922c1ed4bc65b, &[0x94]), // levels 1,2,3,4,5,6,7,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,28,29,30,31,32,33,34,35,36
    ("hero_transitions (Veldin 0x503b68)", 7701, 0x47f6fee11f580403, &[0x348, 0x6ec, 0xacc, 0x1cdc, 0x2a7c, 0x2c78, 0x5bfc, 0x6d74]), // levels 1,3,5,6,7,10,11,12,13,14,16,17,18,20,21,22,24,26,28,29
    ("hero_camera_update (Veldin 0x50ee90)", 352, 0x81c1de82af3d83e1, &[0x70, 0xbc, 0x2c8, 0x3f4, 0x4e8]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("hero_camera_allowed (Veldin 0x50ff18)", 57, 0xa1f16a3bedd2be67, &[0xc, 0xe0]), // levels 1,2,3,4,5,6,7,8,9,10,11,12,13,14,16,17,18,19,20,21,22,23,24,26,27,28,29,30,31,32,33,34,35,36
    ("level 2's 0x307298", 414, 0x1e837b4f2f87280e, &[0x3c8]), // levels 2,5,12,14
    ("level 18's 0x351750", 487, 0xa568515a9dab0da5, &[0xf4]), // levels 18,20,26,29
    ("level 18's 0x352128", 248, 0xf29db6c9839c79b6, &[0x13c]), // levels 18,20,26,29
    ("level 4's 0x35a890", 52, 0x2d54d5bd06cd6c3d, &[0x34]), // levels 4,6,11,16,19,22
    ("level 4's 0x3cf6c8", 1694, 0x1168430fc0d7c487, &[0x84c, 0xbf8]), // levels 4,26
    ("level 9's 0x405b60", 269, 0x983504bf33a75018, &[0xb8]), // levels 9,18,20,24,26,29
    ("level 30's 0x4c05f8", 9414, 0xede2e18592073265, &[0x6ff4]), // levels 30,31,32,33,34,35,36
    ("level 2's 0x4c6cd0", 6772, 0xa86ede014f32e312, &[0x5d70]), // levels 2
    ("level 27's 0x4c6e30", 5467, 0x3cc64dad114787e5, &[0x3fcc]), // levels 27
    ("level 30's 0x4c9bc8", 709, 0x0e8eea626c032d32, &[0x3ec]), // levels 30,31,32,33,34,35,36
    ("level 30's 0x4cb6e0", 5208, 0x0c4f5a59c7677107, &[0x1f4c, 0x2a30, 0x2e30]), // levels 30,31,32,33,34,35,36
    ("level 2's 0x4cd958", 663, 0x2bb6e5aaf367b755, &[0x3dc]), // levels 2,9
    ("level 27's 0x4cd958", 3861, 0x3087693ff7ab9e37, &[0x16e8, 0x1d44, 0x213c]), // levels 27
    ("level 2's 0x4cf3b8", 3968, 0xa29c5a01f662724e, &[0x1d08, 0x23e4, 0x275c]), // levels 2
    ("level 8's 0x4d22a8", 5475, 0x7d1121d9efd135bf, &[0x4094]), // levels 8
    ("level 30's 0x4d34b0", 8816, 0x0c2bb450a17d847f, &[0x348, 0x6ec, 0xacc, 0x1cdc, 0x2a7c, 0x2c78, 0x5bb8, 0x7ee0]), // levels 30,31,32,33,34,35,36
    ("level 27's 0x4d3fe8", 6107, 0x5181a85fa127acbf, &[0x324, 0x6b8, 0xa5c, 0x1bdc, 0x225c, 0x244c, 0x4348, 0x54b4]), // levels 27
    ("level 23's 0x4d43b8", 9483, 0x3874b678101c2175, &[0x7058]), // levels 23
    ("level 2's 0x4d5de0", 5652, 0xe2e648f03b2feaee, &[0x2d4, 0x674, 0xa48, 0x1c58, 0x28b0, 0x2aa0, 0x4d98]), // levels 2
    ("level 8's 0x4d8d78", 3770, 0xbcc3603adf7d0af1, &[0x16c8, 0x1c7c, 0x2078]), // levels 8
    ("level 8's 0x4dcfc8", 705, 0x5bdee993f14c0f68, &[0x94]), // levels 8,27
    ("level 23's 0x4ddaa0", 706, 0x5715cb29f0d00744, &[0x3d8]), // levels 23
    ("level 8's 0x4df298", 6157, 0x8d2c840824e609fa, &[0x324, 0x6b8, 0xa5c, 0x1c50, 0x2324, 0x2514, 0x4410, 0x557c]), // levels 8
    ("level 23's 0x4df5a8", 4922, 0xdf49e04d465cdc30, &[0x1f4c, 0x28c4, 0x2cc0]), // levels 23
    ("level 19's 0x4e5d40", 7310, 0x527f195ad9dfafa1, &[0x5c14]), // levels 19
    ("level 23's 0x4e6f00", 8399, 0xae66f5523e0a1ef2, &[0x348, 0x6ec, 0xacc, 0x1cdc, 0x2a7c, 0x2c78, 0x5bfc, 0x6dbc, 0x785c]), // levels 23
    ("level 19's 0x4eece0", 4216, 0xee275dedaa928b23, &[0x19b0, 0x20a8, 0x24a0]), // levels 19
    ("level 19's 0x4f5708", 7245, 0x7be8aba01883696f, &[0x328, 0x6cc, 0xaac, 0x1680, 0x2358, 0x2554, 0x54dc, 0x6654]), // levels 19
    ("level 4's 0x5003d0", 6662, 0x180724e334026692, &[0x51f8]), // levels 4
    ("level 4's 0x508950", 4118, 0xb669fb2336ec4bea, &[0x19cc, 0x2028, 0x2420]), // levels 4
    ("level 9's 0x50a150", 6503, 0xdb3443ea5b3f5505, &[0x59c0]), // levels 9
    ("level 4's 0x50f5c8", 6338, 0x27dfdcc7fba6aea0, &[0x324, 0x6b8, 0xa5c, 0x1bdc, 0x229c, 0x248c, 0x46e4, 0x5850]), // levels 4
    ("level 9's 0x512408", 3809, 0x078896f4d367a1e2, &[0x1c18, 0x21c4, 0x2540]), // levels 9
    ("level 9's 0x518bb8", 5583, 0xf58c5d10fd3a0d17, &[0x2d4, 0x674, 0xa48, 0x1c58, 0x2878, 0x2a68, 0x4c84]), // levels 9
];

/// The moby cull's drawn byte (moby +0x31): func_L01_0043EA78 sets it when a moby passes its tests and clears it when the moby
/// is culled, and the game's code reads it to act on what is on screen: the lists of targets (func_L01_0031E488, 0x3444B0,
/// 0x3D48B8, 0x3DE4F0 pick drawn, living enemies from 0x1EEB00), the drop shadows (0x444070, 0x444188), debris removed off
/// screen (0x3004E0), post-draw callbacks (0x4026A0). With the cull widened (`VIEW_LOADS`), each (branch on the byte as the cull
/// read it, the store that sets it) on the passed path makes the byte the original view's decision (`uycpu::Cpu::drawn_side`,
/// `in_original_view`), so what happens stays as it was while the moby is drawn.
const DRAWN: &[(&str, usize, u64, &[(u32, u32)])] = &[("the moby cull", 1680, 0xff82413af542ecfa, &[(0x518, 0x520), (0x1330, 0x133c)])];

/// The HUD at the wide screen's edges (PLAN 4.2). The game's UI element system (the HUD, and the newer screens of the pause
/// family) lays its elements out in shares of the original picture's width. At the entry of its draw of the current screen
/// (func_L01_00511BA8, a0 its manager) the runtime works out which groups of the HUD's elements sit at the left or the right
/// (`uycpu::Cpu::ui_screen`); each load of an element's x (+0x10) where an element kind lays itself out, or a picture element
/// is drawn at its own place, reads it plus its group's shift (`Cpu::ui_shift`: none for any other element, and none at all on
/// an original-shaped screen). In every level; not in the front ends, whose UI screens are menus (drawn centred).
const UI_SCREEN: &[(&str, usize, u64)] = &[("the UI's screen draw", 20, 0xfd08b1eb7fadfb71)];
const UI_LOADS: &[(&str, usize, u64, &[u32])] = &[
    // The container's layout (func_L01_00519AC0: its rect, its scissor's).
    ("the container's layout", 150, 0xb47bb88dd31e77de, &[0xe4]),
    // One code three times: the layouts of the text (kind 2, func_L01_0051B4C8) and of kinds 4 and 6 (0x51A738, 0x51D250).
    ("the text's layout", 152, 0x4e41992cdfa9ad2d, &[0xe0]),
    // Kind 3's layout (func_L01_0051BB08: a scaled text, placed in shares of the screen when its parent is the whole screen).
    ("kind 3's layout", 274, 0xe77694b767760a8e, &[0x210, 0x264]),
    // Kind 4's draw (func_L01_0051A158).
    ("kind 4's draw", 375, 0x469dde854cb024d9, &[0x8c, 0x210]),
    // The picture's draw at its own place (func_L01_0051D658, flag 0x100: x + 0x1DC120's offset, times 512) and its layout
    // (func_L01_0051D7E0).
    ("the picture's draw", 98, 0xbabc96afb5ac0875, &[0x58]),
    ("the picture's layout", 136, 0x05888eb997415845, &[0x74]),
];

/// The 2D drawers' screen test (func_L01_00403E40, 0x4040D8, 0x4042B8, 0x404498: the text's glyphs and the sprites): a quad is
/// dropped when its left edge is past GS x 0x9000 (12.4, the frame's right edge: XYOFFSET 0x7000 + 512 pixels; the same bound
/// for its top) or its right edge before 0x7000. On a wider screen the HUD's groups are placed past the frame's edges
/// (`UI_LOADS`), so the bound (`ori`, +offset) and the right edge's test (the first `slti`) take the wide screen's edges
/// (`Cpu::ui_margin`, 0 on an original-shaped screen). In every level, as `UI_LOADS`.
const UI_EDGES: &[(&str, usize, u64, &[(u32, bool)])] = &[
    ("the glyph test A", 164, 0xecd1171d39b5a1c4, &[(0x70, true), (0xa0, false)]),
    ("the glyph test B", 119, 0x87b787fa48385755, &[(0x84, true), (0xb4, false)]),
    ("the glyph test C", 120, 0xa577a73264b20b40, &[(0x78, true), (0xa0, false)]),
    ("the glyph test D", 169, 0xbff81b5c3c6e8b6f, &[(0xb0, true), (0xf0, false)]),
];

/// A function's code with what relocation changes left out (jump targets, the 16-bit immediates of I-type instructions): the same
/// library function linked into another program or level has the same shape.
fn shape(words: &[u32]) -> u64 {
    fnv(words.iter().map(|&w| match w >> 26 {
        0x02 | 0x03 => w & 0xfc00_0000,
        0x00 | 0x1c | 0x10 | 0x11 | 0x12 => w,
        _ => w & 0xffff_0000,
    }))
}

fn fnv(words: impl Iterator<Item = u32>) -> u64 {
    words.fold(0xcbf2_9ce4_8422_2325u64, |h, w| (h ^ w as u64).wrapping_mul(0x100_0000_01b3))
}

struct Out {
    name: String,
    text: (u32, u32),
    hash: u64,
    sample: Vec<(u32, u32)>,
    crates: Vec<String>,
    /// The micro addresses its VCALLMS instructions start.
    calls: BTreeSet<usize>,
}

/// Writes a file only when its text changed, so cargo rebuilds only the crates a translator run changed.
fn put(path: impl AsRef<std::path::Path>, text: impl AsRef<[u8]>) -> std::io::Result<()> {
    match std::fs::read(&path) {
        Ok(old) if old == text.as_ref() => Ok(()),
        _ => std::fs::write(path, text),
    }
}

/// Every address a program's code calls or jumps to and every word of its records: the places other programs' code reaches.
fn targets(p: &Program) -> BTreeSet<u32> {
    let mut t = BTreeSet::new();
    for a in (p.text.0..p.text.1).step_by(4) {
        let Some(w) = p.word(a) else { continue };
        let i = decode::decode(w);
        if i.m == decode::M::Jal || i.m == decode::M::J {
            t.insert(i.jump_target(a));
        }
    }
    for (_, b) in &p.records {
        for c in b.chunks_exact(4) {
            t.insert(u32::from_le_bytes(c.try_into().expect("4 bytes")));
        }
    }
    t
}

fn translate(p: &Program, out_dir: &str, cpu_path: &str, outside: &BTreeSet<u32>) -> Result<Out, String> {
    let word = |a: u32| p.word(a).ok_or_else(|| format!("{}: {a:08x} is in none of the program's records", p.name));
    // Functions: the bounds, then the gaps holding decodable code.
    let mut funcs: Vec<(u32, u32)> = Vec::new();
    let mut at = p.text.0;
    for &(s, e) in &p.funcs {
        if s > at {
            let gap: Result<Vec<u32>, String> = (at..s).step_by(4).map(word).collect();
            if gap.map(|g| g.iter().all(|&w| decode::decode(w).m != decode::M::Unknown)).unwrap_or(false) {
                funcs.push((at, s));
            }
        }
        funcs.push((s, e));
        at = at.max(e);
    }
    let starts: BTreeSet<u32> = funcs.iter().map(|f| f.0).collect();
    let owner = |a: u32| -> Option<(u32, u32)> {
        let s = *starts.range(..=a).next_back()?;
        funcs.iter().find(|f| f.0 == s).copied().filter(|f| a < f.1)
    };
    // Labels.
    let mut labels: BTreeSet<u32> = starts.clone();
    let in_text = |v: u32| v >= p.text.0 && v < p.text.1 && v % 4 == 0;
    // What the other programs reach in this one (the front ends call into the core, past an entry the bounds put a word early).
    for &v in outside.range(p.text.0..p.text.1) {
        if in_text(v) && owner(v).is_some() {
            labels.insert(v);
        }
    }
    for (_, b) in &p.records {
        for c in b.chunks_exact(4) {
            let v = u32::from_le_bytes(c.try_into().expect("4 bytes"));
            if in_text(v) && owner(v).is_some() {
                labels.insert(v);
            }
        }
    }
    for &(s, e) in &funcs {
        let mut hi: [Option<u32>; 32] = [None; 32];
        // Addresses this function builds inside itself, and whether it jumps through a register other than ra.
        let (mut own, mut computed) = (Vec::new(), false);
        // The function's own branch targets, and the code after each jump that does not come back (`j`, `jr`) and its delay slot.
        let (mut inner, mut after) = (BTreeSet::new(), Vec::new());
        for a in (s..e).step_by(4) {
            let i = decode::decode(word(a)?);
            if (i.m == decode::M::J || i.m == decode::M::Jr) && a + 8 < e {
                after.push(a + 8);
            }
            if i.is_cond_branch() || i.m == decode::M::J {
                let t = if i.m == decode::M::J { i.jump_target(a) } else { i.branch_target(a) };
                if t >= s && t < e {
                    inner.insert(t);
                }
            }
            match i.m {
                decode::M::Lui => hi[i.rt()] = Some(i.uimm() << 16),
                decode::M::Addiu | decode::M::Ori => {
                    if let Some(h) = hi[i.rs()] {
                        let v = if i.m == decode::M::Ori { h | i.uimm() } else { h.wrapping_add(i.simm() as u32) };
                        if in_text(v) && owner(v).is_some() {
                            labels.insert(v);
                            if v >= s && v < e {
                                own.push(v);
                            }
                        }
                    }
                }
                decode::M::Jr if i.rs() != 31 => computed = true,
                _ => {}
            }
            if i.m == decode::M::Jal || i.m == decode::M::J {
                let t = i.jump_target(a);
                if in_text(t) && owner(t).is_some() && (i.m == decode::M::Jal || !(t >= s && t < e)) {
                    labels.insert(t);
                }
            }
            if i.is_cond_branch() {
                let t = i.branch_target(a);
                if in_text(t) && owner(t).is_some() && (i.links() || !(t >= s && t < e)) {
                    labels.insert(t);
                }
            }
        }
        // Code after a jump that does not come back, which no branch of the function reaches, is entered from outside: an entry of a
        // table of small routines (level 4's VU0 entries from 0x4b0e2c, 12 bytes apart: `jalr t1, t0` with t0 stepped by 12).
        for x in after {
            if !inner.contains(&x) && owner(x).is_some() {
                labels.insert(x);
            }
        }
        // A jump into an unrolled run at an offset from an address the function built (fe100 0x3d37a0: `jr` to 0x3d37a8 plus
        // a multiple of 4, into five VU0 multiplies): every word of the straight run from that address can be entered.
        if computed {
            for &v in &own {
                for a in (v..e).step_by(4).take(256) {
                    labels.insert(a);
                    if decode::decode(word(a)?).has_delay() {
                        break;
                    }
                }
            }
        }
    }
    let mut natives = Vec::new();
    // The functions whose view loads are widened (`VIEW_LOADS`), for the report.
    let mut viewed = Vec::new();
    // Emit, chunked.
    let mut crates = Vec::new();
    let mut src = String::new();
    let mut table = String::new();
    // The window the code in `src` belongs to.
    let mut window = 0u32;
    let mut total = 0usize;
    let mut spins = Vec::new();
    let mut failed = Vec::new();
    let flush = |src: &mut String, table: &mut String, crates: &mut Vec<String>, window: u32| -> Result<(), String> {
        let name = format!("{}_{window:03}", p.name);
        let dir = format!("{out_dir}/{name}");
        std::fs::create_dir_all(format!("{dir}/src")).map_err(|e| format!("{dir}: {e}"))?;
        put(
            format!("{dir}/Cargo.toml"),
            format!("# Written by port/uyatr from the player's disc. Game code: never committed.\n[package]\nname = \"tr-{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nuycpu = {{ path = \"{cpu_path}\" }}\n", name.replace('_', "-")),
        )
        .map_err(|e| format!("{dir}: {e}"))?;
        let lib = format!("//! Written by port/uyatr from the player's disc: {} ({name}). Game code, never committed.\n#![allow(clippy::all, non_snake_case)]\nuse uycpu::Cpu;\n\npub static FUNCS: &[(u32, uycpu::Fn)] = &[\n{table}];\n\n{src}", p.name);
        put(format!("{dir}/src/lib.rs"), lib).map_err(|e| format!("{dir}: {e}"))?;
        crates.push(name);
        src.clear();
        table.clear();
        Ok(())
    };
    for &(s, e) in &funcs {
        let w = (s - p.text.0) / CHUNK_BYTES;
        if w != window && !src.is_empty() {
            flush(&mut src, &mut table, &mut crates, window)?;
        }
        window = w;
        let words: Vec<u32> = (s..e).step_by(4).map(word).collect::<Result<_, _>>()?;
        total += words.len();
        let sh = shape(&words);
        if let Some(&(what, _, _, k)) = NATIVE.iter().find(|n| n.1 == words.len() && n.2 == sh) {
            natives.push(format!("{what} at {s:08x}"));
            writeln!(src, "pub extern \"C-unwind\" fn f{s:08x}(c: &mut Cpu, l: u32) {{ if l != 0x{s:08x} {{ c.bad_label(0x{s:08x}, l); }} c.native({k}); c.ret = c.r[31] as u32; }}").unwrap();
            writeln!(table, "(0x{s:08x}, f{s:08x}),").unwrap();
            continue;
        }
        let fl: BTreeSet<u32> = labels.range(s..e).copied().collect();
        let next = p.word(e);
        let f = match emit::func(s, e, &words, next, &fl) {
            Ok(f) => f,
            Err(err) => {
                failed.push(err);
                continue;
            }
        };
        // The front ends' frames are shown as the 2D layer at the original picture's shape (port/uya `front.rs`: the menus in
        // the middle, a space scene scaled to the screen's width), so their view is made at the original shape: not widened,
        // and their moby cull's drawn byte is its own.
        let wide_view = !p.name.starts_with("fe");
        let widen = VIEW_LOADS.iter().filter(|v| wide_view && v.1 == words.len() && v.2 == sh).flat_map(|v| v.3.iter().map(move |&o| s + o)).collect();
        let fov = FOV_LOADS.iter().filter(|v| v.1 == words.len() && v.2 == sh).flat_map(|v| v.3.iter().map(move |&o| s + o)).collect();
        for (what, ..) in FOV_LOADS.iter().filter(|v| v.1 == words.len() && v.2 == sh) {
            viewed.push(format!("{what} (the field of view's) at {s:08x}"));
        }
        let cam_setting = CAMERA_SETTING.iter().filter(|v| v.1 == words.len() && v.2 == sh).flat_map(|v| v.3.iter().map(move |&o| s + o)).collect();
        for (what, ..) in CAMERA_SETTING.iter().filter(|v| v.1 == words.len() && v.2 == sh) {
            viewed.push(format!("{what} (the mouse aim's camera option) at {s:08x}"));
        }
        let cam_speed = CAMERA_SPEED.iter().filter(|v| v.1 == words.len() && v.2 == sh).flat_map(|v| v.3.iter().map(move |&o| s + o)).collect();
        for (what, ..) in CAMERA_SPEED.iter().filter(|v| v.1 == words.len() && v.2 == sh) {
            viewed.push(format!("{what} (the camera speed's) at {s:08x}"));
        }
        let drawn = DRAWN.iter().filter(|v| wide_view && v.1 == words.len() && v.2 == sh).flat_map(|v| v.3.iter().map(move |&(b, st)| (s + b, s + st))).collect();
        for (what, ..) in VIEW_LOADS.iter().filter(|v| wide_view && v.1 == words.len() && v.2 == sh) {
            let flag = if DRAWN.iter().any(|d| d.1 == words.len() && d.2 == sh) { " (its drawn byte the original view's)" } else { "" };
            viewed.push(format!("{what}{flag} at {s:08x}"));
        }
        let hud = !p.name.starts_with("fe");
        let ui_screen = UI_SCREEN.iter().filter(|v| hud && v.1 == words.len() && v.2 == sh).map(|_| s).collect();
        let ui_loads = UI_LOADS.iter().filter(|v| hud && v.1 == words.len() && v.2 == sh).flat_map(|v| v.3.iter().map(move |&o| s + o)).collect();
        for (what, ..) in UI_SCREEN.iter().filter(|v| hud && v.1 == words.len() && v.2 == sh) {
            viewed.push(format!("{what} (the HUD's groups placed) at {s:08x}"));
        }
        for (what, ..) in UI_LOADS.iter().filter(|v| hud && v.1 == words.len() && v.2 == sh) {
            viewed.push(format!("{what} (the HUD's x) at {s:08x}"));
        }
        let ui_edges = UI_EDGES.iter().filter(|v| hud && v.1 == words.len() && v.2 == sh).flat_map(|v| v.3.iter().map(move |&(o, bound)| (s + o, bound))).collect();
        for (what, ..) in UI_EDGES.iter().filter(|v| hud && v.1 == words.len() && v.2 == sh) {
            viewed.push(format!("{what} (the wide screen's edges) at {s:08x}"));
        }
        let mut ctx = emit::Ctx { spins: Vec::new(), ra_copies: [false; 32], unwritten: [false; 32], widen, fov, cam_speed, cam_setting, drawn, ui_screen, ui_loads, ui_edges };
        let mut body = String::new();
        if let Err(err) = emit::emit(&f, &mut ctx, &mut body) {
            failed.push(err);
            continue;
        }
        spins.extend(ctx.spins);
        if let Some(&(what, _, _, k)) = JOBS.iter().find(|j| j.1 == words.len() && j.2 == sh) {
            natives.push(format!("{what} at {s:08x} (in front of its translation)"));
            let named = format!("fn f{s:08x}(");
            if !body.contains(&named) {
                return Err(format!("{}: {what} at {s:08x}: its translation has no fn f{s:08x}", p.name));
            }
            src.push_str(&body.replacen(&named, &format!("fn f{s:08x}_body("), 1));
            let entry = s | 1 | (wide_view as u32) << 1;
            writeln!(
                src,
                "/// {what}: written by port/uyatr (`JOBS`), the runtime's native code (`uyart::mobydraw`), or this translation then the runtime's check.\npub extern \"C-unwind\" fn f{s:08x}(c: &mut Cpu, l: u32) {{\nif l != 0x{s:08x} {{ c.bad_label(0x{s:08x}, l); }}\nc.ret = 0x{entry:08x};\nc.native({k});\nif c.ret != 0 {{ return; }}\nf{s:08x}_body(c, l);\nc.native({});\n}}",
                k + 1
            )
            .unwrap();
        } else {
            src.push_str(&body);
        }
        for &a in f.leaders.iter().filter(|a| labels.contains(a)) {
            writeln!(table, "(0x{a:08x}, f{s:08x}),").unwrap();
        }
    }
    if !failed.is_empty() {
        let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
        for f in &failed {
            let k = f.split(": ").nth(1).unwrap_or(f).split(' ').take(4).collect::<Vec<_>>().join(" ");
            *kinds.entry(k).or_default() += 1;
        }
        for (k, n) in &kinds {
            eprintln!("{}: {n} x {k}", p.name);
        }
        for f in failed.iter().take(20) {
            eprintln!("{}: {f}", p.name);
        }
        return Err(format!("{}: {} of {} functions not translated", p.name, failed.len(), funcs.len()));
    }
    if !src.is_empty() || crates.is_empty() {
        flush(&mut src, &mut table, &mut crates, window)?;
    }
    let text_words: Vec<u32> = (p.text.0..p.text.1).step_by(4).map(word).collect::<Result<_, _>>()?;
    let hash = fnv(text_words.iter().copied());
    let step = (text_words.len() / 64).max(1);
    let sample = (0..text_words.len()).step_by(step).map(|k| (p.text.0 + 4 * k as u32, text_words[k])).collect();
    println!("{}: answered natively: {}", p.name, if natives.is_empty() { "none".to_string() } else { natives.join(", ") });
    println!("{}: the view widened in: {}", p.name, if viewed.is_empty() { "none".to_string() } else { viewed.join(", ") });
    println!(
        "{}: text {:08x}..{:08x}, entry {:08x}, {} functions, {} instructions, {} labels, {} spin loops, {} crates, hash {hash:016x}",
        p.name,
        p.text.0,
        p.text.1,
        p.entry,
        funcs.len(),
        total,
        labels.len(),
        spins.len(),
        crates.len()
    );
    let calls = text_words.iter().filter(|&&w| w >> 26 == 0x12 && w & (1 << 25) != 0 && w & 0x3f == 0x38).map(|&w| (((w >> 6) & 0x7fff) * 8) as usize).collect();
    Ok(Out { name: p.name.clone(), text: p.text, hash, sample, crates, calls })
}

fn load(iso: &str, bounds: &str, name: &str) -> Result<Program, String> {
    match name {
        "loader" => programs::loader(iso),
        "boot" => programs::main_program(iso, bounds, "boot", "boot"),
        "fe100" => programs::main_program(iso, bounds, "100", "fe100"),
        "fe101" => programs::frontbin(iso, bounds),
        l if l.starts_with('l') && l != "levels" => programs::level(iso, bounds, l[1..].parse().map_err(|_| format!("{l}: not lNN"))?),
        other => Err(format!("{other}: not a program")),
    }
}

fn main() {
    if let Err(e) = run() {
        eprintln!("uyatr: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 5 {
        return Err("usage: uyatr DISC BOUNDS OUT PROGRAM... (loader, boot, fe100, fe101, lNN, levels)".into());
    }
    let (iso, bounds, out_dir) = (&a[1], &a[2], &a[3]);
    // `uyatr DISC BOUNDS dis PROGRAM ADDR[,ADDR...]`: the listing of the functions holding each address, with their code's FNV
    // (the value the NATIVE table checks).
    if out_dir == "dis" {
        let p = load(iso, bounds, a.get(4).ok_or("dis PROGRAM ADDR,...")?)?;
        let hex = |s: &str| u32::from_str_radix(s.trim_start_matches("0x"), 16).map_err(|e| format!("{s}: {e}"));
        let mut list = Vec::new();
        for s in a.get(5).ok_or("dis PROGRAM ADDR,...")?.split(',') {
            // ADDR, or LO-HI: every function overlapping the range.
            if let Some((lo, hi)) = s.split_once('-') {
                let (lo, hi) = (hex(lo)?, hex(hi)?);
                list.extend(p.funcs.iter().filter(|f| f.1 > lo && f.0 < hi).copied());
            } else {
                let x = hex(s)?;
                list.push(*p.funcs.iter().find(|f| x >= f.0 && x < f.1).ok_or_else(|| format!("{x:08x} is in no function of {}", p.name))?);
            }
        }
        for (f0, f1) in list {
            let words: Vec<u32> = (f0..f1).step_by(4).map(|a| p.word(a).ok_or(format!("{a:08x} not in the records"))).collect::<Result<_, _>>()?;
            println!("function {f0:08x}..{f1:08x} of {}: {} words, FNV {:016x}, shape {:016x}", p.name, words.len(), fnv(words.iter().copied()), shape(&words));
            for (k, &w) in words.iter().enumerate() {
                let pc = f0 + 4 * k as u32;
                println!("  {pc:08x} {w:08x}  {}", decode::text(&decode::decode(w), pc));
            }
        }
        return Ok(());
    }
    // The port's workspace (this folder): the runtime and the crate the code is compiled against.
    let port = std::fs::canonicalize(".").map_err(|e| format!("this folder: {e}"))?;
    if !port.join("uyart").is_dir() || !port.join("uycpu").is_dir() {
        return Err(format!("{}: run uyatr from port/ (no uyart and uycpu here)", port.display()));
    }
    let slash = |p: std::path::PathBuf| p.to_string_lossy().trim_start_matches(r"\\?\").replace('\\', "/");
    let (rt_path, cpu_path) = (slash(port.join("uyart")), slash(port.join("uycpu")));
    // `uyatr DISC BOUNDS vu1 OUT`: the VU1 microprograms alone (OUT/vu1), as a full run writes them.
    if out_dir == "vu1" {
        let out = a.get(4).ok_or("vu1 OUT")?;
        let boot = load(iso, bounds, "boot")?;
        return vu1mp::write(&boot, out, &cpu_path, |path, text| put(&path, text).map_err(|e| format!("{path}: {e}")));
    }
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{out_dir}: {e}"))?;
    let mut outs = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for n in &a[4..] {
        if n == "levels" {
            names.extend(LEVELS.iter().map(|l| format!("l{l:02}")));
        } else {
            names.push(n.clone());
        }
    }
    let progs: Vec<Program> = names.iter().map(|name| load(iso, bounds, name)).collect::<Result<_, _>>()?;
    // The places other programs' code reaches in a program: only from programs that can be in RAM with it (the levels'
    // overlays and the front ends occupy the same addresses: a level never calls another level's or a front end's code).
    let overlap = |p: &Program, q: &Program| p.text.0 < q.text.1 && q.text.0 < p.text.1;
    let reach: Vec<BTreeSet<u32>> = progs.iter().map(targets).collect();
    for (k, p) in progs.iter().enumerate() {
        let outside: BTreeSet<u32> = progs.iter().enumerate().filter(|(j, q)| *j != k && !overlap(p, q)).flat_map(|(j, _)| reach[j].iter().copied()).collect();
        outs.push(translate(p, out_dir, &cpu_path, &outside)?);
    }
    // The crate the port depends on: every program's table and the code checks the runtime makes before using it.
    let dir = format!("{out_dir}/game");
    std::fs::create_dir_all(format!("{dir}/src")).map_err(|e| format!("{dir}: {e}"))?;
    let mut toml = format!("# Written by port/uyatr. Game code: never committed.\n[package]\nname = \"uya-game\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nuyart = {{ path = \"{rt_path}\" }}\n");
    let mut lib = String::from("//! Written by port/uyatr: the translated programs. Game code, never committed.\n\npub static PROGRAMS: &[uyart::Program] = &[\n");
    for o in &outs {
        let tables: Vec<String> = o.crates.iter().map(|c| format!("tr_{c}::FUNCS")).collect();
        let sample: Vec<String> = o.sample.iter().map(|(a, w)| format!("(0x{a:08x}, 0x{w:08x})")).collect();
        let entry = |funcs: &str, lib: &str, ty: &str| {
            format!("{ty} {{ name: \"{}\", text: (0x{:08x}, 0x{:08x}), hash: 0x{:016x}, sample: &[{}], funcs: {funcs}, lib: \"{lib}\" }}", o.name, o.text.0, o.text.1, o.hash, sample.join(", "))
        };
        if !o.name.starts_with('l') || o.name == "loader" {
            for c in &o.crates {
                writeln!(toml, "tr-{} = {{ path = \"../{c}\" }}", c.replace('_', "-")).unwrap();
            }
            writeln!(lib, "    {},", entry(&format!("&[{}]", tables.join(", ")), "", "uyart::Program")).unwrap();
            continue;
        }
        // A story level: a library of its own (a member of the levels' workspace, below); the executable's list names it, with no
        // functions.
        let lib_name = format!("tr_{}", o.name);
        writeln!(lib, "    {},", entry("&[]", &lib_name, "uyart::Program")).unwrap();
        let dir = format!("{out_dir}/levels/{}", o.name);
        std::fs::create_dir_all(format!("{dir}/src")).map_err(|e| format!("{dir}: {e}"))?;
        let mut t = format!(
            "# Written by port/uyatr from the player's disc: {0}'s code, the library the runtime loads with its overlay. Game code: never committed.\n[package]\nname = \"tr-{0}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\ncrate-type = [\"cdylib\"]\n\n[dependencies]\nuycpu = {{ path = \"{cpu_path}\" }}\n",
            o.name
        );
        for c in &o.crates {
            writeln!(t, "tr-{} = {{ path = \"../../{c}\" }}", c.replace('_', "-")).unwrap();
        }
        put(format!("{dir}/Cargo.toml"), t).map_err(|e| format!("{dir}: {e}"))?;
        let l = format!(
            "//! Written by port/uyatr from the player's disc: {0}'s code, the library the runtime loads when the level's overlay is in RAM. Game code, never committed.\n#![allow(improper_ctypes_definitions)]\n\nstatic FUNCS: &[&[(u32, uycpu::Fn)]] = &[{1}];\nstatic PROGRAM: uycpu::Program = {2};\n\n/// The layout of `uycpu` this library was built against.\n#[unsafe(no_mangle)]\npub extern \"C\" fn uya_abi() -> u64 {{\n    uycpu::ABI\n}}\n\n/// The program this library holds.\n#[unsafe(no_mangle)]\npub extern \"C\" fn uya_program() -> &'static uycpu::Program {{\n    &PROGRAM\n}}\n",
            o.name,
            tables.join(", "),
            entry("FUNCS", &lib_name, "uycpu::Program")
        );
        put(format!("{dir}/src/lib.rs"), l).map_err(|e| format!("{dir}: {e}"))?;
    }
    lib.push_str("];\n");
    // The levels' workspace: every level library this run wrote, built in one cargo run by scripts\build-levels.ps1 into the port's
    // target folder (beside the executable that loads them).
    let levels: Vec<&str> = outs.iter().filter(|o| o.name.starts_with('l') && o.name != "loader").map(|o| o.name.as_str()).collect();
    if !levels.is_empty() {
        let members: Vec<String> = levels.iter().map(|n| format!("\"{n}\"")).collect();
        put(format!("{out_dir}/levels/Cargo.toml"), format!("{LEVELS_WORKSPACE}members = [{}]\n\n{LEVELS_PROFILE}", members.join(", "))).map_err(|e| format!("{out_dir}/levels: {e}"))?;
    }
    // The VU0 microprograms, cut from the boot program's data where the game's VIF0 transfers send them from.
    let boot = load(iso, bounds, "boot")?;
    let calls: BTreeSet<usize> = outs.iter().flat_map(|o| o.calls.iter().copied()).collect();
    let mut vsrc = String::from("//! Written by port/uyatr: the game's VU0 microprograms (vu0mp.rs). Game code, never committed.\n#![allow(clippy::all, unused_parens)]\n");
    lib.push_str("\npub static VU0: &[uyart::vu0::Vu0Prog] = &[\n");
    for vp in vu0mp::PROGS {
        let base = vp.parts[0].1;
        let mut code = Vec::new();
        for &(at, micro, pairs) in vp.parts {
            if micro != base + code.len() {
                return Err(format!("VU0 {}: part at micro {micro:#x} does not follow", vp.name));
            }
            for k in 0..pairs * 2 {
                code.extend(boot.word(at + 4 * k as u32).ok_or_else(|| format!("VU0 {}: {at:08x} not in the boot program", vp.name))?.to_le_bytes());
            }
        }
        vsrc.push_str(&vu0mp::translate(vp.name, base, &code, &calls, false).map_err(|e| format!("VU0 {}: {e}", vp.name))?);
        writeln!(lib, "    uyart::vu0::Vu0Prog {{ name: \"{0}\", base: {base:#x}, code: &tr_vu0::{0}::CODE, entry: tr_vu0::{0}::entry, park: tr_vu0::{0}::park, run: tr_vu0::{0}::run }},", vp.name).unwrap();
        println!("VU0 {}: micro {base:#x}..{:#x}", vp.name, base + code.len());
    }
    lib.push_str("];\n");
    let vdir = format!("{out_dir}/vu0");
    std::fs::create_dir_all(format!("{vdir}/src")).map_err(|e| format!("{vdir}: {e}"))?;
    put(format!("{vdir}/Cargo.toml"), format!("# Written by port/uyatr. Game code: never committed.\n[package]\nname = \"tr-vu0\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nuycpu = {{ path = \"{cpu_path}\" }}\n")).map_err(|e| format!("{vdir}: {e}"))?;
    put(format!("{vdir}/src/lib.rs"), vsrc).map_err(|e| format!("{vdir}: {e}"))?;
    writeln!(toml, "tr-vu0 = {{ path = \"../vu0\" }}").unwrap();
    // The VU1 microprograms the VIF1 chain uploads (vu1mp.rs): a crate of their own, which the renderer depends on.
    vu1mp::write(&boot, out_dir, &cpu_path, |path, text| put(&path, text).map_err(|e| format!("{path}: {e}")))?;
    put(format!("{dir}/Cargo.toml"), toml).map_err(|e| format!("{dir}: {e}"))?;
    put(format!("{dir}/src/lib.rs"), lib).map_err(|e| format!("{dir}: {e}"))?;
    Ok(())
}
