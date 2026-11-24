//! LEVEL block 3 (30 story levels; absent in 24, 30, 34 and 35).
//!
//! The block is a byte-identical copy of the gameplay block's +0x90 table (three counts, a zero word, then that many (key, id)
//! pairs of 8 bytes in three groups; checked on all 30 levels; the levels without the block are exactly those whose gameplay +0x90
//! is zero), followed by zero fill to the sector boundary. Nothing reads the block: no code in boot_elf, the 34 level overlays or
//! the two front ends reads the header pair at 0x168608/0x16860C (block 3's sector offset and count; xrefs over 0x168608..0x168610
//! and over the +0x79C8/+0x79CC offsets from 0x160C40 find none, and the loaders' block reads (func_L01_004252F0 block 2,
//! func_L01_004282C0 blocks 4-6 by index, func_L01_0045B378/func_L01_0045B3D0 blocks 1 and 7-9) never name it); in the Veldin
//! gameplay RAM dump the block's bytes sit at 0x818800 but its last sector is overwritten by later data and no code or pointer
//! refers to the address. The table's real reader is func_L01_004252F0 reading the gameplay block's copy (gameplay.rs).

use crate::track::{Space, Tracker};
use crate::track::Node;

/// Block 3 at `at` in the LEVEL file (disc space), `b` its bytes.
pub fn block3(t: &mut Tracker, file: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let l = t.ignored_line(
        "LEVEL block 3",
        "block 3: copy of the gameplay +0x90 table (three counts, a zero word, (key, id) pairs, zero fill)",
        "no reader of the block or of its header pair (+0x28/+0x2C, 0x168608) in boot_elf, the 34 level overlays or the front ends (xrefs of the pair and of the +0x79C8 offset, every block loader read); byte-identical to the gameplay block's +0x90 table, which func_L01_004252F0 reads",
        Space::Disc,
    );
    let c = t.child(file, l, at, b.len() as u64);
    t.close(c);
    Ok(())
}
