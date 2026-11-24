//! The table of contents at sector 1001, as the game reads it.
//!
//! func_0013D2E0 reads 16 sectors from sector 0x3E9 to 0x160C40. The game's structure there: the eight global WAD headers, each
//! (byte size, sector, then (u32, u32) pairs), one after another from +0; the level table at +0x7400, 60 slots of three
//! (header sector, file sectors) pairs (AUDIO, LEVEL, SCENE); then, from +0x79A0, the current level's LEVEL header (0x60 bytes),
//! AUDIO header (+0x7A00, 0x1818) and SCENE header (+0x9218, 0x26F0), which func_0013D310 copies in from the header sectors the
//! slot names (1, 4 and 5 sectors). So the disc's bytes +0x79A0..+0x8000 are read and overwritten.

use crate::disc::{Disc, u32_at};

pub const TOC_SECTOR: u64 = 1001;
pub const TOC_SECTORS: u64 = 16;
pub const LEVEL_TABLE: usize = 0x7400;
pub const LEVEL_SLOTS: usize = 60;
pub const LEVEL_HEADERS: usize = 0x79A0;

pub struct WadHeader {
    /// Offset of the header in the table of contents.
    pub at: usize,
    pub size: usize,
    pub sector: u64,
    pub pairs: Vec<(u32, u32)>,
}

/// The three files of a level-table slot.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FileKind {
    Audio,
    Level,
    Scene,
}

impl FileKind {
    pub const ALL: [FileKind; 3] = [FileKind::Audio, FileKind::Level, FileKind::Scene];
    pub fn name(self) -> &'static str {
        match self {
            FileKind::Audio => "AUDIO",
            FileKind::Level => "LEVEL",
            FileKind::Scene => "SCENE",
        }
    }
    /// The header's byte size and the sectors func_0013D310 reads for it.
    pub fn header(self) -> (usize, u64) {
        match self {
            FileKind::Audio => (0x1818, 4),
            FileKind::Level => (0x60, 1),
            FileKind::Scene => (0x26f0, 5),
        }
    }
}

pub struct LevelFile {
    pub level: usize,
    pub kind: FileKind,
    pub header_sector: u64,
    pub sectors: u64,
    pub header: Vec<u8>,
    /// The file's first sector, from the header's +4.
    pub sector: u64,
}

pub struct Toc {
    pub bytes: Vec<u8>,
    pub wads: Vec<WadHeader>,
    /// Where the WAD headers end (the level table must start there).
    pub wads_end: usize,
    pub files: Vec<LevelFile>,
}

pub fn read(d: &mut Disc) -> Result<Toc, String> {
    let bytes = d.sectors(TOC_SECTOR, TOC_SECTORS)?;
    let mut wads = Vec::new();
    let mut at = 0;
    while wads.len() < 8 {
        let size = u32_at(&bytes, at)? as usize;
        let sector = u32_at(&bytes, at + 4)? as u64;
        if size < 8 || (size - 8) % 8 != 0 || at + size > LEVEL_TABLE {
            return Err(format!("table of contents +{at:#x}: WAD header {} size {size:#x}", wads.len() + 1));
        }
        let pairs = (0..(size - 8) / 8).map(|k| Ok((u32_at(&bytes, at + 8 + 8 * k)?, u32_at(&bytes, at + 12 + 8 * k)?))).collect::<Result<Vec<_>, String>>()?;
        wads.push(WadHeader { at, size, sector, pairs });
        at += size;
    }
    let wads_end = at;
    let mut files = Vec::new();
    for level in 0..LEVEL_SLOTS {
        for (k, kind) in FileKind::ALL.into_iter().enumerate() {
            let e = LEVEL_TABLE + level * 24 + k * 8;
            let (hs, n) = (u32_at(&bytes, e)? as u64, u32_at(&bytes, e + 4)? as u64);
            if hs == 0 && n == 0 {
                continue;
            }
            let (hsize, hsec) = kind.header();
            let h = d.sectors(hs, hsec)?;
            let size = u32_at(&h, 0)? as usize;
            if size != hsize {
                return Err(format!("level {level} {}: header at sector {hs} has size {size:#x}, not {hsize:#x}", kind.name()));
            }
            let sector = u32_at(&h, 4)? as u64;
            files.push(LevelFile { level, kind, header_sector: hs, sectors: n, header: h, sector });
        }
    }
    Ok(Toc { bytes, wads, wads_end, files })
}
