//! The disc image, read by sector ranges (never whole: the image is 4.4 GB).

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};

pub const SECTOR: u64 = 0x800;

pub struct Disc {
    file: File,
    pub sectors: u64,
    pub bytes: u64,
}

impl Disc {
    pub fn open(path: &str) -> Result<Disc, String> {
        let file = File::open(path).map_err(|e| format!("{path}: {e}"))?;
        let bytes = file.metadata().map_err(|e| format!("{path}: {e}"))?.len();
        if bytes % SECTOR != 0 {
            return Err(format!("{path}: {bytes} bytes is not a whole number of {SECTOR}-byte sectors"));
        }
        Ok(Disc { file, sectors: bytes / SECTOR, bytes })
    }

    /// `count` sectors from `sector`.
    pub fn sectors(&mut self, sector: u64, count: u64) -> Result<Vec<u8>, String> {
        self.bytes_at(sector * SECTOR, count * SECTOR)
    }

    /// `len` bytes from byte offset `at`.
    pub fn bytes_at(&mut self, at: u64, len: u64) -> Result<Vec<u8>, String> {
        if at + len > self.bytes {
            return Err(format!("disc read {at:#x}+{len:#x} runs past the image's end {:#x}", self.bytes));
        }
        let mut out = vec![0u8; len as usize];
        self.file.seek(SeekFrom::Start(at)).map_err(|e| e.to_string())?;
        self.file.read_exact(&mut out).map_err(|e| e.to_string())?;
        Ok(out)
    }
}

pub fn u16_at(b: &[u8], at: usize) -> Result<u16, String> {
    b.get(at..at + 2).map(|s| u16::from_le_bytes([s[0], s[1]])).ok_or_else(|| format!("u16 at {at:#x} past the end {:#x}", b.len()))
}

pub fn u32_at(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("u32 at {at:#x} past the end {:#x}", b.len()))
}

pub fn u32_be(b: &[u8], at: usize) -> Result<u32, String> {
    b.get(at..at + 4).map(|s| u32::from_be_bytes([s[0], s[1], s[2], s[3]])).ok_or_else(|| format!("u32 at {at:#x} past the end {:#x}", b.len()))
}
