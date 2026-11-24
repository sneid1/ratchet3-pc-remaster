//! The addresses of a level's globals, read from the level's own code.
//!
//! Each level overlay (the LEVEL file's level data, part 0: records {address, size, kind, entry} then bytes, copied to their
//! addresses until the entry word changes) is a separately linked copy of the game's level code, so the variables it owns
//! (camera, view, terrain zones, sky, shrubs, ties, mobies, light sets) sit at different addresses in each level. The code
//! forms each address with a `lui` and a load, store or `addiu` (or `gp` plus an offset), so each address is read from the
//! instruction that forms it, in the game function that does so on Veldin (NTSC-U level 1).
//!
//! A function is found in a level by its code: the words of the function in level 1, with each immediate field of an
//! address-bearing instruction (lui, addiu, ori, loads, stores) and each jump target taken out (the linker changes exactly
//! those between levels' copies, rac3-decomp's level catalogue fingerprints the same way), must equal the words at exactly one
//! place in the level's records. The instruction at the same offset from the function's start is then read: its register
//! fields were part of the match, so it is the same instruction, and its immediate (with the `lui`'s) is the level's address.
//!
//! Functions (all in the catalogue, extern/rac3-decomp/config/us/levels/functions.tsv, as the same function in all 34 story
//! levels; Veldin's addresses):
//! - func_L01_004252F0 (the level's data setup; 0x2D14 bytes): stores the shrub and tie array pointers, counts and limits,
//!   the moby array pointer and forms the light sets' address, and reads each also through gp.
//! - func_L01_003F3660 (camera_manager_update): forms GameCamera's address.
//! - func_L01_003FFAC8 (view_projection_setup): reads the view tangents.
//! - func_L01_003FEE38: forms the terrain zone record's address.
//! - func_L01_004A46C8: reads the sky pointer.
//! - func_L01_004A2200 (shrub cull), func_L01_004AFA28 (tie cull), func_L01_004B0990 (tie DMA chain): read the shrub and tie
//!   globals through gp; they confirm both the field addresses and gp.
//!
//! gp is the one value the boot program's start code sets (func_0013ADA8 at 0x13AF38..0x13AF60: lui/addiu into a0, then
//! `daddu $gp, $a0, $zero`) before any level runs. The level's own code shows it too: a variable that func_L01_004252F0 stores
//! through lui and through gp (the tie count, the moby array pointer) gives gp = lui address - gp offset; both variables must
//! give the same gp, and every gp-relative read of the three culling functions must land on its field.

use crate::disc::{Disc, SECTOR, u32_at};
use crate::toc::{self, FileKind};

/// One overlay record: where the loader copies `data`, and its kind word.
pub struct Record {
    pub address: u32,
    pub kind: u32,
    pub data: Vec<u8>,
}

/// A level's overlay: the records the loader copies (the .lit, .bss, .data, vtables and .text of levels.tsv).
pub struct Overlay {
    pub level: usize,
    pub entry: u32,
    pub records: Vec<Record>,
}

impl Overlay {
    /// The word at `address`, from the record holding it.
    pub fn word(&self, address: u32) -> Option<u32> {
        self.records.iter().find(|r| address >= r.address && (address - r.address) as usize + 4 <= r.data.len()).map(|r| {
            let o = (address - r.address) as usize;
            u32::from_le_bytes([r.data[o], r.data[o + 1], r.data[o + 2], r.data[o + 3]])
        })
    }

    /// The record holding `address` (inside the record's size), if any.
    pub fn record_of(&self, address: u32) -> Option<&Record> {
        self.records.iter().find(|r| address >= r.address && (address - r.address) as usize + 4 <= r.data.len())
    }
}

/// Reads story level `level`'s overlay (part 0 of the LEVEL file's block 0).
pub fn overlay(d: &mut Disc, tc: &toc::Toc, level: usize) -> Result<Overlay, String> {
    let f = tc.files.iter().find(|f| f.level == level && f.kind == FileKind::Level).ok_or_else(|| format!("no LEVEL file for level {level}"))?;
    let (off, n) = (u32_at(&f.header, 0x10)? as u64, u32_at(&f.header, 0x14)? as u64);
    if n == 0 || off + n > f.sectors {
        return Err(format!("L{level:02} LEVEL block 0: sectors {off}+{n} past the file's {}", f.sectors));
    }
    let head = d.sectors(f.sector + off, 1)?;
    let (o, s) = (u32_at(&head, 0)? as u64, u32_at(&head, 4)? as u64);
    let b = d.bytes_at((f.sector + off) * SECTOR + o, s)?;
    let mut records = Vec::new();
    let mut p = 0usize;
    let mut entry = None;
    while p + 16 <= b.len() {
        let (address, size, kind, e) = (u32_at(&b, p)?, u32_at(&b, p + 4)? as usize, u32_at(&b, p + 8)?, u32_at(&b, p + 12)?);
        if *entry.get_or_insert(e) != e {
            break;
        }
        if p + 16 + size > b.len() {
            return Err(format!("L{level:02} overlay record at {p:#x}: {size:#x} bytes run past the overlay's {:#x}", b.len()));
        }
        records.push(Record { address, kind, data: b[p + 16..p + 16 + size].to_vec() });
        p += 16 + size;
    }
    let entry = entry.ok_or_else(|| format!("L{level:02}: the overlay has no records"))?;
    Ok(Overlay { level, entry, records })
}

/// The shrub globals' addresses (the variables hold the count, the array pointers and the limit).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ShrubAddrs {
    pub count: u32,
    pub instances: u32,
    pub matrices: u32,
    pub colours: u32,
    pub limit: u32,
}

/// The tie globals' addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TieAddrs {
    pub count: u32,
    pub instances: u32,
    pub matrices: u32,
    pub colours: u32,
    pub limit: u32,
    /// The frame's list of ties drawn (0x240880 on Veldin): one word each, bits 0-11 the instance, bits 12-24 the quadword
    /// index of its record in the record table, bits 25-31 read by the DMA code (func_L01_004B0990 +0x44 / +0x48 form it and
    /// send it to the scratchpad; 0x4B0BBC to 0x4B0C0C read the words).
    pub list: u32,
    /// The list's size in bytes (gp+0x46BC: func_L01_004B0990 +0x40).
    pub list_bytes: u32,
    /// The variable holding the frame's record table (gp+0x46B8: func_L01_004AFA28, the cull, stores it; func_L01_004B0990
    /// +0xB0 reads it). A tie's record is 6 quadwords (sent to VU1), or 10 when its class has second-part packets: quadwords
    /// 6 to 9 are what func_L01_004B13C8 and func_L01_004B17E8 read.
    pub records: u32,
    /// The TEST_1 values the DMA chain sends before a packet's first draw (func_L01_004B0990 +0x110 to +0x11C: `lui`/`lq` of
    /// the two A+D quadwords it keeps at the scratchpad's 0x3EA0 and 0x3EB0; 0x1DA320 and 0x1DA330 on Veldin, in the overlay's
    /// record at 0x1D5680): [0] for a packet whose LOD list entry flags (+0xE) have bit 3 (0x4B0CB8 `andi $a2, $a2, 8`,
    /// 0x4B0CD8 `bnel` picks the 0x3EA0 quadword; 0x5340A: no alpha test), [1] for the others and for every second part
    /// (0x5340B: GEQUAL 0x40, a failing pixel writes its colour alone). The chain sends one only when it changes.
    pub tests: [u64; 2],
    /// The tie classes: the table of their headers' addresses (0x23C580 on Veldin: func_L01_004AFA28 +0x34 and +0x38 form it)
    /// and the variable holding how many (gp+0x46A0: +0x30). The cull reads a header's LOD distances at +0x10, +0x14 and +0x18
    /// (`lqc2 $vf16, 0x10($s0)`, `tie::TieClass::lod_distances`), its first instance at +0x20 and its instance count at +0x3E.
    pub classes: u32,
    pub class_count: u32,
}

/// Where a level's globals are in EE RAM. Every field lies in the level overlay's .bss (the boot program's own variables are
/// not among them: each address differs from level to level, and gp, the one boot constant, is only a base).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LevelGlobals {
    pub level: usize,
    /// Two words of the level's code (func_L01_004AFA28 +0x34 and +0x38, the class table's `lui` and `addiu`) and where they sit:
    /// the level's code and globals are in RAM only while those words are there (a level change loads the next overlay over the
    /// last).
    pub anchor: u32,
    pub anchor_words: [u32; 2],
    /// The gp the level's code addresses its globals from: the same on every level, set by the boot program's start code
    /// (func_0013ADA8, 0x13AF38 to 0x13AF60); recovered here from func_L01_004252F0's lui and gp forms of one variable.
    pub gp: u32,
    /// GameCamera (position +0, forward row +0x380, up row +0x3A0): func_L01_003F3660 (camera_manager_update) +0x94, the
    /// `addiu $v1, $s6, lo` after the `lui` at +0x14.
    pub camera: u32,
    /// The camera the world is drawn with (its rows; the eye is at -0x20): func_L01_003FE8F8 +0x60, the `addiu $s2, $s2, lo`
    /// after the `lui` at +0x54. That function, the only writer of the view matrix (0x1E7880 on Veldin: -right, -up, forward,
    /// eye x 1024), builds it from this copy: eye at -0x20, forward +0, right +0x10, up +0x20. In gameplay it equals GameCamera;
    /// in cutscenes it is the cutscene's camera.
    pub view_camera: u32,
    /// The view's half-angle tangents (horizontal, then vertical at +4): func_L01_003FFAC8 (view_projection_setup) +0x70, the
    /// `lwc1 $f0, lo($v1)` through `lui $v0` at +0x58 and `addiu $v1, $v0, lo` at +0x68 (+0x78 stores the vertical one at +4).
    pub tangents: u32,
    /// The terrain zone record for the camera's cell: func_L01_003FEE38 +0x98, the `addiu $a0, $v0, lo` after the `lui` at
    /// +0x8C.
    pub zones: u32,
    /// The variable holding the sky's pointer: func_L01_004A46C8 +0x30, the `lw $v1, lo($v1)` after the `lui` at +0x2C.
    pub sky: u32,
    /// Shrubs: func_L01_004252F0 stores the count at +0xC1C (`sw $v0, lo($at)`, `lui` +0xC18), the instance array pointer at
    /// +0xC0C (`lui` +0xC08), the matrix array pointer at +0xC58 (`lui` +0xC54), the colour blocks pointer at +0xC84 (`lui`
    /// +0xC80) and the distance limit at +0x3D4 (`swc1 $f1, lo($at)`, `lui` +0x3D0). func_L01_004A2200 reads the matrices
    /// (+0x34), the limit (+0x44) and the colours (+0x48) through gp.
    pub shrubs: ShrubAddrs,
    /// Ties: func_L01_004252F0 stores the count at +0x674 (`lui` +0x670; also gp form at +0x668), the instance array pointer
    /// at +0x694 (`lui` +0x690), the colour buffers' base at +0x69C (`lui` +0x698), the matrix array pointer at +0x6FC (`lui`
    /// +0x6F8) and the limit at +0x3C4 (`lui` +0x3C0). func_L01_004AFA28 reads the instances (+0x40), matrices (+0x44) and
    /// limit (+0x88), func_L01_004B0990 the colours (+0xAC), through gp.
    pub ties: TieAddrs,
    /// The variable holding the moby array's pointer: func_L01_004252F0 +0x133C, `sw $s4, lo($at)` after the `lui` at +0x1338
    /// (the gp form is at +0x262C).
    pub mobies: u32,
    /// The variable holding the end of the moby array's instances in use (0x1E00A0 on Veldin, the word after the array's
    /// pointer): func_L01_004252F0 +0x1944, `sw $v0, lo($at)` after the `lui` at +0x1940, the last instance (0x1DF9CC on Veldin)
    /// + 0x100, whose +0x20 byte the setup sets to 0xFF.
    pub moby_end: u32,
    /// The light sets (copied from the gameplay block): func_L01_004252F0 +0x378, the `addiu $s0, $s0, lo` after the `lui` at
    /// +0x368.
    pub light_sets: u32,
    /// The number of the level chunk whose terrain is loaded (0x1F3FC0 on Veldin): func_L01_004282C0 (the chunk loader) +0x1C,
    /// the `lw $v1, lo($v0)` after the `lui` at +0x4. The level's setup (func_L01_00428750 at 0x4287C4) sets it to 0 when it
    /// puts the core's terrain in the terrain buffer; the loader, given chunk N, unpacks the LEVEL file's block 4 + N (its
    /// sector and size at the LEVEL header +0x30 + 8N, the header at 0x160C40 + 0x79A0) into the same buffer and stores N here.
    pub level_chunk: u32,
    /// Entry point of the same loader in this level's overlay.
    pub load_chunk: u32,
    /// The variable holding the head of the frame's moby draw record list (0x1DFBF0 on Veldin): func_L01_00437EB0 +0x2C, the
    /// `lw $a0, lo($a0)` after the `lui` at +0x28, which it passes to func_003C5D90 (the skin job's loop).
    pub moby_jobs: u32,
    /// The variable holding the core's RAM base (0x1EA9D8 on Veldin, holding 0x91E000): func_L01_00428750 +0x60 `lw $s3, 0x18($s1)`,
    /// $s1 formed at +0x4 and +0x14; the class records' core offsets and the terrain's are added to it.
    pub core: u32,
    /// The variable holding the hero's loaded armor WAD entry (0x1D4B60 on Veldin): func_L01_00422BD0 (armor_change) +0x38
    /// `sw $s0, lo($at)` after the `lui` at +0x34, once func_L01_00422838 and func_L01_00422AB8 have loaded and installed it.
    pub armor_entry: u32,
    /// The variable holding the hero's armor buffer (0x1D4B64, holding 0x1F28000 on Veldin): func_L01_00422838 +0x40 `lw`
    /// after the `lui` at +0x3C; the model block is there (packet A of mesh m at the value + `Mesh::a_at`).
    pub armor_buffer: u32,
    /// The variable holding the wrench's model block (WAD5 entry 31; 0x1EAA34, holding 0x1F20000 on Veldin):
    /// func_L01_00422D70 +0x34 `lw` after the `lui` at +0x28.
    pub wrench_block: u32,
    /// The 16 bytes that map an armor texture setup's number to its slot (func_L01_00422AB8 +0x80 `addiu $a2, $gp, imm`, passed
    /// to func_L01_00423E10, which reads byte `local` at 0x423E8C), as the overlay holds them.
    pub armor_remap: [u8; 16],
    /// Clank's texture (func_L01_00423238): the texture a moby class of mode 2 gives its 0xFF setups (0x1D4BA8).
    pub clank_texture: SkinTexture,
    /// The ship's texture (func_L01_00423068): the texture a moby class of mode 1 gives its 0xFF setups (0x16C5B8).
    pub ship_texture: SkinTexture,
    /// The held gadget's class (`GadgetAddrs`).
    pub gadget: GadgetAddrs,
    /// The variable holding the HUD header's copy (func_L01_004233F8, the HUD loader, +0xB8 `sw $s0, imm($at)` after the `lui`
    /// at +0xB4; 0x1DFD98 on Veldin): its texture and palette tables' +4 get each entry's GS block when func_L01_0041D950
    /// uploads it (`hud::HudData`).
    pub hud_header: u32,
    /// The menu's WAD entry loader (`MenuEntry`), when the level has it.
    pub menu_entry: Option<MenuEntry>,
    /// The WAD streaming slots (`StreamSlots`).
    pub stream_slots: StreamSlots,
}

/// The slots the game streams WAD entries through (func_003AED40, the front end's function, in every level: on Veldin at
/// 0x490968, `lui $s3` at +0x10 and `addiu $v0, $s3, lo` at +0x5C form the first; +0x30 `addiu $s0, $zero, 3` counts four,
/// +0x58 `sll $v1, $s4, 5` steps 0x20). A slot: +0 its state (func_003AEFD0: 4 reading, 2 done), +0xC the buffer the entry
/// is read into, +0x10 the buffer it is unpacked into (none: 0), +0x14 the pair table (in the table of contents' copy at
/// 0x160C40), +0x18 the WAD's base sector, +0x1C the entry (0x490AF0 on Veldin reads its pair's sector and count from the table). The mission
/// menu streams the selected mission's picture through one (WAD8 from entry 996: a 2FIP image).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StreamSlots {
    pub pool: u32,
    pub count: u32,
}

/// The WAD entry a menu's loader reads into its buffer (func_003A9B10, the front end's function, in every story level: on Veldin
/// func_L01_004CE920; rac3-decomp's catalogue has level 16's copy, 0x4DFE30, as func_L16_004DFE30, its split giving the last
/// word, the `jr $ra`'s delay slot, to a 4-byte remnant, though the code is Veldin's but for its calls' targets): +0x58 `addiu $s2, $s3, 0x7090` (the entry's place in the buffer), +0xA0 `sw $s3, lo($at)`
/// after the `lui` at +0x9C (the variable holding the buffer), and the disc read of +0xCC, +0xD0 and +0xD4: the pair's sector
/// and count and the WAD's base sector, all off the table of contents' copy (0x160C40, formed at +0xB8 and +0xC0). On level
/// 18 the buffer is 0x1FB8000 and the entry WAD8's 782nd (the mission menu's pictures: the selected mission's is uploaded
/// from it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MenuEntry {
    /// The variable holding the buffer's address.
    pub buffer: u32,
    /// The entry's offset in the buffer.
    pub offset: u32,
    /// The WAD header's offset in the table of contents (its base sector at +4).
    pub wad: u32,
    /// The entry's pair (sector from the base, sector count) offset in the table of contents.
    pub pair: u32,
}

/// The bytes of the WAD entry `m` names, as the loader reads them from the disc.
pub fn menu_entry_bytes(d: &mut Disc, tc: &toc::Toc, m: &MenuEntry) -> Result<Vec<u8>, String> {
    let base = u32_at(&tc.bytes, m.wad as usize + 4)? as u64;
    let (off, n) = (u32_at(&tc.bytes, m.pair as usize)? as u64, u32_at(&tc.bytes, m.pair as usize + 4)? as u64);
    d.sectors(base + off, n)
}

/// A texture a loader reads from a WAD by a skin number and uploads (func_L01_00423238 Clank's, func_L01_00423068 the ship's): a
/// 2FIP image, its CLUT at +0x20 and texels at +0x420. The loader stores the skin number in a variable, reads the WAD's base
/// sector from the table of contents' copy in RAM (0x160C40) at the header's +4 and the image's sectors at the header's pairs +
/// 8 x the skin number, both off one base register.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SkinTexture {
    /// The variable holding the skin number.
    pub skin: u32,
    /// The WAD header's offset in the table of contents (`toc::WadHeader::at`).
    pub wad: u32,
    /// The pair of skin 0's image in that header.
    pub pair: u32,
}

/// Where the held gadget's class is (func_L01_0042A058, the gadget install, and func_L01_00424368, the class loader): the
/// install unpacks a gadget block's class into the gadget buffer and loads it at the class table index the level reserved for it
/// (it makes that index the class count for the loader's call and puts the count back after), so the class table's entry at
/// that index names the class drawn from the buffer (`moby::gadget` decodes it by that id).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GadgetAddrs {
    /// The gadget buffer (0x191ED0 on Veldin): func_L01_0042A058 +0x80 `addiu $a1, $s5, lo` after the `lui` at +0x1C, where both
    /// LZ streams are unpacked.
    pub buffer: u32,
    /// The variable holding the reserved class table index (an s16; 0x16C6A2 on Veldin): +0x1D8 `lh $v1, 0x12($s1)`, $s1 formed
    /// at +0x24 and +0x19C.
    pub index: u32,
    /// The class table's ids (an s16 per index; 0x1FDA70 on Veldin): func_L01_00424368 +0x80 `addiu $a1, $a1, lo` after the `lui`
    /// at +0x6C.
    pub class_ids: u32,
    /// The class table's class addresses (a word per index; 0x1FD200 on Veldin): func_L01_00424368 +0x90 `addiu $a0, $a0, lo`
    /// after the `lui` at +0x7C; the install reads the same at +0x210 (`lui` +0x208).
    pub classes: u32,
    /// The gadget table's first entry in the table of contents (0x4590): func_L01_0042A058 +0x68 reads the block's sector (an
    /// entry's +4) off the table of contents' copy at 0x160C40 (formed at +0x2C and +0x50).
    pub table: u32,
}

/// A game function of level 1: its address and size in bytes.
struct Func {
    name: &'static str,
    address: u32,
    size: u32,
}

const SETUP: Func = Func { name: "func_L01_004252F0", address: 0x42_52F0, size: 0x2D14 };
const CAMERA: Func = Func { name: "func_L01_003F3660", address: 0x3F_3660, size: 0x270 };
const VIEW: Func = Func { name: "func_L01_003FFAC8", address: 0x3F_FAC8, size: 0x754 };
const ZONES: Func = Func { name: "func_L01_003FEE38", address: 0x3F_EE38, size: 0xAE8 };
const SKY: Func = Func { name: "func_L01_004A46C8", address: 0x4A_46C8, size: 0x98 };
const SHRUB_CULL: Func = Func { name: "func_L01_004A2200", address: 0x4A_2200, size: 0x18D8 };
const TIE_CULL: Func = Func { name: "func_L01_004AFA28", address: 0x4A_FA28, size: 0xF3C };
const TIE_DMA: Func = Func { name: "func_L01_004B0990", address: 0x4B_0990, size: 0x8F0 };
const CHUNK: Func = Func { name: "func_L01_004282C0", address: 0x42_82C0, size: 0x254 };
const VIEW_MATRIX: Func = Func { name: "func_L01_003FE8F8", address: 0x3F_E8F8, size: 0x2D0 };
const MOBY_JOBS: Func = Func { name: "func_L01_00437EB0", address: 0x43_7EB0, size: 0x44 };
const LEVEL_LOAD: Func = Func { name: "func_L01_00428750", address: 0x42_8750, size: 0xCFC };
const ARMOR_LOAD: Func = Func { name: "func_L01_00422838", address: 0x42_2838, size: 0x27C };
const ARMOR_INSTALL: Func = Func { name: "func_L01_00422AB8", address: 0x42_2AB8, size: 0x118 };
const ARMOR_CHANGE: Func = Func { name: "func_L01_00422BD0", address: 0x42_2BD0, size: 0x90 };
const WRENCH_LOAD: Func = Func { name: "func_L01_00422D70", address: 0x42_2D70, size: 0x1B8 };
const CLANK_TEXTURE: Func = Func { name: "func_L01_00423238", address: 0x42_3238, size: 0x1C0 };
const SHIP_TEXTURE: Func = Func { name: "func_L01_00423068", address: 0x42_3068, size: 0x1D0 };
const HUD_LOAD: Func = Func { name: "func_L01_004233F8", address: 0x42_33F8, size: 0x2A0 };
const MENU_ENTRY: Func = Func { name: "func_003A9B10", address: 0x4C_E920, size: 0x1D8 };
const STREAM_SLOTS: Func = Func { name: "func_003AED40", address: 0x49_0968, size: 0x84 };
const CLASS_LOAD: Func = Func { name: "func_L01_00424368", address: 0x42_4368, size: 0x110 };
const GADGET_INSTALL: Func = Func { name: "func_L01_0042A058", address: 0x42_A058, size: 0x2A8 };

const ALL: [&Func; 22] = [&STREAM_SLOTS, &HUD_LOAD, &CLANK_TEXTURE, &SHIP_TEXTURE, &SETUP, &CAMERA, &VIEW, &ZONES, &SKY, &SHRUB_CULL, &TIE_CULL, &TIE_DMA, &CHUNK, &VIEW_MATRIX, &MOBY_JOBS, &LEVEL_LOAD, &ARMOR_LOAD, &ARMOR_INSTALL, &ARMOR_CHANGE, &WRENCH_LOAD, &CLASS_LOAD, &GADGET_INSTALL];

/// Each function's start address in `ov`, by name: how `from_overlay` finds them (the menu entry loader's when the level has
/// it).
pub fn function_starts(reference: &Overlay, ov: &Overlay) -> Result<Vec<(&'static str, u32, u32)>, String> {
    let words = text_words(ov);
    let mut v: Vec<(&'static str, u32, u32)> = ALL.iter().map(|f| Ok((f.name, f.address, locate(reference, ov, &words, f)?))).collect::<Result<_, String>>()?;
    if let Some(at) = locate_some(reference, ov, &words, &MENU_ENTRY)? {
        v.push((MENU_ENTRY.name, MENU_ENTRY.address, at));
    }
    Ok(v)
}

/// `locate`, None when the level has no copy of `f`.
fn locate_some(reference: &Overlay, ov: &Overlay, words: &[(u32, Vec<u32>)], f: &Func) -> Result<Option<u32>, String> {
    match locate(reference, ov, words, f) {
        Ok(at) => Ok(Some(at)),
        Err(e) if e.ends_with("is not in the overlay") => Ok(None),
        Err(e) => Err(e),
    }
}

/// The start in `ov` of the level 1 function at `address` of `size` bytes (`name` for the errors): found by its words, as above.
pub fn locate_function(reference: &Overlay, ov: &Overlay, name: &'static str, address: u32, size: u32) -> Result<u32, String> {
    locate(reference, ov, &text_words(ov), &Func { name, address, size })
}

/// A word with the fields the linker changes between levels' copies of a function taken out: jump targets and the immediates
/// of lui, the add/or/slt immediates, loads, stores and cache ops. Registers, opcodes and branch offsets stay.
fn norm(w: u32) -> u32 {
    match w >> 26 {
        0x02 | 0x03 => w & 0xFC00_0000,
        0x08..=0x0F | 0x18..=0x1B | 0x1E | 0x1F | 0x20..=0x2F | 0x30..=0x3F => w & 0xFFFF_0000,
        _ => w,
    }
}

/// The 4-byte aligned positions of `ov`'s records.
fn text_words(ov: &Overlay) -> Vec<(u32, Vec<u32>)> {
    ov.records
        .iter()
        .map(|r| (r.address, r.data.chunks_exact(4).map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()))
        .collect()
}

/// Locates `f` in `ov`: level 1's copy supplies the words.
fn locate(reference: &Overlay, ov: &Overlay, words: &[(u32, Vec<u32>)], f: &Func) -> Result<u32, String> {
    let n = (f.size / 4) as usize;
    let want: Vec<u32> = (0..n)
        .map(|i| reference.word(f.address + 4 * i as u32).map(norm).ok_or_else(|| format!("{}: level 1 has no word at {:#x}", f.name, f.address + 4 * i as u32)))
        .collect::<Result<_, _>>()?;
    let mut hits = Vec::new();
    for (base, ws) in words {
        if ws.len() < n {
            continue;
        }
        for i in 0..=ws.len() - n {
            if norm(ws[i]) == want[0] && ws[i..i + n].iter().zip(&want).all(|(w, x)| norm(*w) == *x) {
                hits.push(base + 4 * i as u32);
            }
        }
    }
    match hits.as_slice() {
        [one] => Ok(*one),
        [] => Err(format!("L{:02}: {} ({} words of level 1's code) is not in the overlay", ov.level, f.name, n)),
        many => Err(format!("L{:02}: {} is at {} places: {:x?}", ov.level, f.name, many.len(), many)),
    }
}

fn sext(w: u32) -> i64 {
    (w & 0xFFFF) as u16 as i16 as i64
}

/// The address a `lui` at function offset `lui`, an optional `addiu` at `add` (on the lui's register) and the memory or addiu
/// instruction at `at` form together.
fn formed(ov: &Overlay, start: u32, f: &Func, lui: u32, add: Option<u32>, at: u32) -> Result<u32, String> {
    let w = |off: u32| ov.word(start + off).ok_or_else(|| format!("L{:02} {} +{off:#x}: no word", ov.level, f.name));
    let l = w(lui)?;
    if l >> 26 != 0x0F {
        return Err(format!("L{:02} {} +{lui:#x}: {l:08x} is not a lui", ov.level, f.name));
    }
    let mut base = ((l & 0xFFFF) << 16) as i64;
    let mut reg = (l >> 16) & 31;
    if let Some(add) = add {
        let a = w(add)?;
        if a >> 26 != 0x09 || (a >> 21) & 31 != reg {
            return Err(format!("L{:02} {} +{add:#x}: {a:08x} is not an addiu on the lui's register", ov.level, f.name));
        }
        base += sext(a);
        reg = (a >> 16) & 31;
    }
    let u = w(at)?;
    let ok = matches!(u >> 26, 0x09 | 0x1E | 0x20..=0x27 | 0x28..=0x2B | 0x31 | 0x39 | 0x37 | 0x3F);
    if !ok || (u >> 21) & 31 != reg {
        return Err(format!("L{:02} {} +{at:#x}: {u:08x} does not use the base register formed at +{lui:#x}", ov.level, f.name));
    }
    Ok((base + sext(u)) as u32)
}

/// The offset of a gp-relative load or store at function offset `at`.
fn gp_offset(ov: &Overlay, start: u32, f: &Func, at: u32) -> Result<i64, String> {
    let u = ov.word(start + at).ok_or_else(|| format!("L{:02} {} +{at:#x}: no word", ov.level, f.name))?;
    if (u >> 21) & 31 != 28 || !matches!(u >> 26, 0x20..=0x27 | 0x28..=0x2B | 0x31 | 0x36 | 0x39 | 0x37 | 0x3F) {
        return Err(format!("L{:02} {} +{at:#x}: {u:08x} is not a gp-relative load or store", ov.level, f.name));
    }
    Ok(sext(u))
}

/// A skin texture loader's WAD and skin 0's pair (`SkinTexture`, its `skin` left 0): the base the `lui` and `addiu` of `base`
/// form, the `lw` of the WAD's base sector at `base.2` (on that base) and the `lw` of the image's sectors at `pair` (on that base
/// plus 8 x the skin number).
fn skin_wad(ov: &Overlay, start: u32, f: &Func, base: (u32, u32, u32), pair: u32) -> Result<SkinTexture, String> {
    let w = |off: u32| ov.word(start + off).ok_or_else(|| format!("L{:02} {} +{off:#x}: no word", ov.level, f.name));
    let toc = formed(ov, start, f, base.0, Some(base.1), base.2)? as i64 - sext(w(base.2)?);
    let p = w(pair)?;
    if p >> 26 != 0x23 {
        return Err(format!("L{:02} {} +{pair:#x}: {p:08x} is not a lw", ov.level, f.name));
    }
    // The header's +4 holds the base sector; its pairs start at +8.
    let (at, first) = (sext(w(base.2)?) - 4, sext(p));
    if toc != 0x16_0C40 || at < 0 || first < at + 8 || (first - at - 8) % 8 != 0 {
        return Err(format!("L{:02} {}: table of contents {toc:#x}, header at {at:#x}, image sectors at {first:#x}", ov.level, f.name));
    }
    Ok(SkinTexture { skin: 0, wad: at as u32, pair: ((first - at - 8) / 8) as u32 })
}

/// The globals of an already read overlay. `reference` is level 1's.
pub fn from_overlay(reference: &Overlay, ov: &Overlay) -> Result<LevelGlobals, String> {
    let found = function_starts(reference, ov)?;
    let st = |f: &Func| found.iter().find(|x| x.1 == f.address).expect("every function of ALL was located").2;
    let at = |f: &Func, lui: u32, use_: u32| formed(ov, st(f), f, lui, None, use_);

    // gp, from the two variables func_L01_004252F0 reaches both ways.
    let gp_of = |lui: u32, mem: u32, gp_at: u32| -> Result<u32, String> {
        Ok((at(&SETUP, lui, mem)? as i64 - gp_offset(ov, st(&SETUP), &SETUP, gp_at)?) as u32)
    };
    let gp = gp_of(0x670, 0x674, 0x668)?;
    let gp2 = gp_of(0x1338, 0x133C, 0x262C)?;
    if gp != gp2 {
        return Err(format!("L{:02}: func_L01_004252F0 gives gp {gp:#x} by the tie count and {gp2:#x} by the moby array", ov.level));
    }

    let anchor = st(&TIE_CULL) + 0x34;
    let word = |a: u32| ov.word(a).ok_or_else(|| format!("L{:02}: no word at {a:#x}", ov.level));
    let g = LevelGlobals {
        level: ov.level,
        anchor,
        anchor_words: [word(anchor)?, word(anchor + 4)?],
        gp,
        camera: at(&CAMERA, 0x14, 0x94)?,
        view_camera: at(&VIEW_MATRIX, 0x54, 0x60)?,
        tangents: formed(ov, st(&VIEW), &VIEW, 0x58, Some(0x68), 0x70)?,
        zones: at(&ZONES, 0x8C, 0x98)?,
        sky: at(&SKY, 0x2C, 0x30)?,
        shrubs: ShrubAddrs {
            count: at(&SETUP, 0xC18, 0xC1C)?,
            instances: at(&SETUP, 0xC08, 0xC0C)?,
            matrices: at(&SETUP, 0xC54, 0xC58)?,
            colours: at(&SETUP, 0xC80, 0xC84)?,
            limit: at(&SETUP, 0x3D0, 0x3D4)?,
        },
        ties: TieAddrs {
            count: at(&SETUP, 0x670, 0x674)?,
            instances: at(&SETUP, 0x690, 0x694)?,
            matrices: at(&SETUP, 0x6F8, 0x6FC)?,
            colours: at(&SETUP, 0x698, 0x69C)?,
            limit: at(&SETUP, 0x3C0, 0x3C4)?,
            list: formed(ov, st(&TIE_DMA), &TIE_DMA, 0x44, None, 0x48)?,
            list_bytes: (gp as i64 + gp_offset(ov, st(&TIE_DMA), &TIE_DMA, 0x40)?) as u32,
            records: (gp as i64 + gp_offset(ov, st(&TIE_DMA), &TIE_DMA, 0xB0)?) as u32,
            tests: {
                let [off, on] = [(0x110, 0x114), (0x118, 0x11C)].map(|(lui, lq)| -> Result<u64, String> {
                    let a = formed(ov, st(&TIE_DMA), &TIE_DMA, lui, None, lq)?;
                    match [0, 4, 8].map(|k| ov.word(a + k)) {
                        [Some(lo), Some(hi), Some(0x47)] => Ok(lo as u64 | (hi as u64) << 32),
                        w => Err(format!("L{:02} {} +{lq:#x}: {a:#x} holds {w:x?}, not an A+D write of TEST_1", ov.level, TIE_DMA.name)),
                    }
                });
                [off?, on?]
            },
            classes: formed(ov, st(&TIE_CULL), &TIE_CULL, 0x34, None, 0x38)?,
            class_count: (gp as i64 + gp_offset(ov, st(&TIE_CULL), &TIE_CULL, 0x30)?) as u32,
        },
        mobies: at(&SETUP, 0x1338, 0x133C)?,
        moby_end: at(&SETUP, 0x1940, 0x1944)?,
        light_sets: at(&SETUP, 0x368, 0x378)?,
        level_chunk: at(&CHUNK, 0x4, 0x1C)?,
        load_chunk: st(&CHUNK),
        moby_jobs: at(&MOBY_JOBS, 0x28, 0x2C)?,
        core: formed(ov, st(&LEVEL_LOAD), &LEVEL_LOAD, 0x4, Some(0x14), 0x60)?,
        armor_entry: at(&ARMOR_CHANGE, 0x34, 0x38)?,
        armor_buffer: at(&ARMOR_LOAD, 0x3C, 0x40)?,
        wrench_block: at(&WRENCH_LOAD, 0x28, 0x34)?,
        armor_remap: {
            let a = st(&ARMOR_INSTALL) + 0x80;
            let w = ov.word(a).ok_or_else(|| format!("L{:02} {} +0x80: no word", ov.level, ARMOR_INSTALL.name))?;
            if w >> 26 != 0x09 || (w >> 21) & 31 != 28 {
                return Err(format!("L{:02} {} +0x80: {w:08x} is not an addiu from gp", ov.level, ARMOR_INSTALL.name));
            }
            let t = (gp as i64 + sext(w)) as u32;
            let mut m = [0u8; 16];
            for (k, b) in m.iter_mut().enumerate() {
                let word = ov.word((t + k as u32) & !3).ok_or_else(|| format!("L{:02}: the armor remap table at {t:#x} is in no overlay record", ov.level))?;
                *b = (word >> (8 * ((t as usize + k) & 3))) as u8;
            }
            m
        },
        clank_texture: SkinTexture { skin: (gp as i64 + gp_offset(ov, st(&CLANK_TEXTURE), &CLANK_TEXTURE, 0x2C)?) as u32, ..skin_wad(ov, st(&CLANK_TEXTURE), &CLANK_TEXTURE, (0x58, 0x60, 0x6C), 0x74)? },
        ship_texture: SkinTexture { skin: formed(ov, st(&SHIP_TEXTURE), &SHIP_TEXTURE, 0xC, Some(0x14), 0x38)?, ..skin_wad(ov, st(&SHIP_TEXTURE), &SHIP_TEXTURE, (0x64, 0x6C, 0x78), 0x80)? },
        gadget: {
            let (s, f) = (st(&GADGET_INSTALL), &GADGET_INSTALL);
            let classes = at(&CLASS_LOAD, 0x7C, 0x90)?;
            let read = formed(ov, s, f, 0x208, None, 0x210)?;
            if read != classes {
                return Err(format!("L{:02} {} +0x210 reads the class addresses at {read:#x}, {} stores them at {classes:#x}", ov.level, f.name, CLASS_LOAD.name));
            }
            let sector = formed(ov, s, f, 0x2C, Some(0x50), 0x68)? as i64;
            let copy = sector - sext(ov.word(s + 0x68).expect("formed read it"));
            if copy != 0x16_0C40 {
                return Err(format!("L{:02} {} +0x68: off {copy:#x}, not the table of contents' copy", ov.level, f.name));
            }
            GadgetAddrs { buffer: formed(ov, s, f, 0x1C, None, 0x80)?, index: formed(ov, s, f, 0x24, Some(0x19C), 0x1D8)?, class_ids: at(&CLASS_LOAD, 0x6C, 0x80)?, classes, table: (sector - copy - 4) as u32 }
        },
        hud_header: formed(ov, st(&HUD_LOAD), &HUD_LOAD, 0xB4, None, 0xB8)?,
        stream_slots: {
            let (s, f) = (st(&STREAM_SLOTS), &STREAM_SLOTS);
            let w = |off: u32| ov.word(s + off).ok_or_else(|| format!("L{:02} {} +{off:#x}: no word", ov.level, f.name));
            let (n, step) = (w(0x30)?, w(0x58)?);
            if n & 0xFFFF_0000 != 0x2410_0000 || step != 0x0014_1940 {
                return Err(format!("L{:02} {}: +0x30 {n:08x}, +0x58 {step:08x}: not the slot count and the 0x20 step", ov.level, f.name));
            }
            StreamSlots { pool: formed(ov, s, f, 0x10, None, 0x5C)?, count: (n & 0xFFFF) + 1 }
        },
        menu_entry: match found.iter().find(|x| x.1 == MENU_ENTRY.address) {
            None => None,
            Some(&(_, _, s)) => {
                let f = &MENU_ENTRY;
                let w = ov.word(s + 0x58).ok_or_else(|| format!("L{:02} {} +0x58: no word", ov.level, f.name))?;
                if w >> 26 != 0x09 {
                    return Err(format!("L{:02} {} +0x58: {w:08x} is not an addiu", ov.level, f.name));
                }
                let toc = |at: u32| -> Result<u32, String> {
                    let a = formed(ov, s, f, 0xB8, Some(0xC0), at)?;
                    let base = a as i64 - sext(ov.word(s + at).expect("formed read it"));
                    if base != 0x16_0C40 {
                        return Err(format!("L{:02} {} +{at:#x}: off {base:#x}, not the table of contents' copy", ov.level, f.name));
                    }
                    Ok(a - 0x16_0C40)
                };
                let (pair, wad_base, count) = (toc(0xCC)?, toc(0xD0)?, toc(0xD4)?);
                if count != pair + 4 || wad_base < 4 {
                    return Err(format!("L{:02} {}: pair at {pair:#x}, count at {count:#x}, WAD base at {wad_base:#x}", ov.level, f.name));
                }
                Some(MenuEntry { buffer: formed(ov, s, f, 0x9C, None, 0xA0)?, offset: w & 0xFFFF, wad: wad_base - 4, pair })
            }
        },
    };

    // The culling functions read the same variables through gp.
    let via_gp = |f: &Func, off: u32, field: u32, what: &str| -> Result<(), String> {
        let a = (gp as i64 + gp_offset(ov, st(f), f, off)?) as u32;
        if a != field {
            return Err(format!("L{:02} {} +{off:#x} reads {a:#x} through gp {gp:#x}, the {what} is at {field:#x}", ov.level, f.name));
        }
        Ok(())
    };
    via_gp(&SHRUB_CULL, 0x34, g.shrubs.matrices, "shrub matrix array pointer")?;
    via_gp(&SHRUB_CULL, 0x44, g.shrubs.limit, "shrub limit")?;
    via_gp(&SHRUB_CULL, 0x48, g.shrubs.colours, "shrub colour blocks pointer")?;
    via_gp(&TIE_CULL, 0x40, g.ties.instances, "tie instance array pointer")?;
    via_gp(&TIE_CULL, 0x44, g.ties.matrices, "tie matrix array pointer")?;
    via_gp(&TIE_CULL, 0x88, g.ties.limit, "tie limit")?;
    via_gp(&TIE_DMA, 0xAC, g.ties.colours, "tie colour buffers' base")?;
    via_gp(&SETUP, 0x262C, g.mobies, "moby array pointer")?;
    Ok(g)
}

/// Story level `level`'s globals, read from its own code on the disc.
pub fn globals(iso_path: &str, level: usize) -> Result<LevelGlobals, String> {
    let mut d = Disc::open(iso_path)?;
    let tc = toc::read(&mut d)?;
    let reference = overlay(&mut d, &tc, 1)?;
    let ov = overlay(&mut d, &tc, level)?;
    from_overlay(&reference, &ov)
}

/// The 34 story levels, 1-14, 16-24 and 26-36.
pub const STORY_LEVELS: [usize; 34] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 16, 17, 18, 19, 20, 21, 22, 23, 24, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36];

/// Every story level's globals, in level order.
pub fn all(iso_path: &str) -> Result<Vec<LevelGlobals>, String> {
    let mut d = Disc::open(iso_path)?;
    let tc = toc::read(&mut d)?;
    let reference = overlay(&mut d, &tc, 1)?;
    STORY_LEVELS
        .iter()
        .map(|&l| {
            let ov = overlay(&mut d, &tc, l)?;
            from_overlay(&reference, &ov)
        })
        .collect()
}
