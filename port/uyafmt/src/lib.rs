//! Up Your Arsenal's disc and data formats (NTSC-U SCUS_973.53), worked out from R&C3's own data and code: the recompiled EE
//! code, the level overlays, rac3-decomp's bounds, names and structs, the VU1 programs, RAM dumps and the VIF1 trace. No R&C1 or
//! R&C2 format code is used, and this crate depends on no rac-rs crate.
//!
//! Game functions are named by their NTSC-U address: `func_XXXXXXXX` in boot_elf, `func_LNN_XXXXXXXX` in level NN's overlay.

pub mod anim;
pub mod boot;
pub mod collision;
pub mod core;
pub mod coremisc;
pub mod disc;
pub mod frontwad;
pub mod fxcode;
pub mod gameplay;
pub mod headers;
pub mod hud;
pub mod iso;
pub mod levelcode;
pub mod menucode;
pub mod lz;
pub mod moby;
pub mod movies;
pub mod occl;
pub mod pif;
pub mod prog6;
pub mod rd;
pub mod sbank;
pub mod scene;
pub mod scenecode;
pub mod subs;
pub mod terrain;
pub mod shrub;
pub mod tie;
pub mod toc;
pub mod track;
pub mod vag;
pub mod volume;
pub mod walk;
pub mod wad38;
pub mod wads;
