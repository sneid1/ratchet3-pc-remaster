//! The main program (rac3-decomp's boot_elf) as the boot program leaves it in RAM.
//!
//! The disc boots `SCUS_973.53`, a loader linked at 0x800000 whose data holds the main program as an LZ stream ("WAD" header,
//! `lz::unpack`) at file offset 0x7080. The loader (0x800488) unpacks it to 0x8BC700 and copies each record ({address, size,
//! kind, entry}, then `size` bytes) to its address until the entry word changes (rac3-decomp tools/levels.py `parse_records`
//! reads the records the same way; scripts/uya_disc.py is the same unpacking in Python). The records at or above 0x100000 make
//! the RAM image from 0x100000. Kind 8 records only reserve zeroed memory (0x152400 and 0x1D9900: the game's own state), and
//! between the records lie the overlays' and the buffers' places: only the other records' bytes are the program's.

use crate::disc::{Disc, SECTOR, u32_at};
use crate::lz;

/// Where the loader's LZ stream starts in `SCUS_973.53`.
const PAYLOAD_OFFSET: usize = 0x7080;
/// The RAM image's first address.
pub const BASE: u32 = 0x10_0000;

/// The main program in RAM from `BASE`.
pub struct Boot {
    pub ram: Vec<u8>,
    /// The spans of the records that carry bytes: (address, size).
    records: Vec<(u32, usize)>,
}

impl Boot {
    /// `len` bytes at RAM address `address`.
    pub fn bytes(&self, address: u32, len: usize) -> Result<&[u8], String> {
        if !self.records.iter().any(|&(a, n)| address >= a && (address - a) as usize + len <= n) {
            return Err(format!("boot: {address:#x}+{len:#x} is in none of the main program's records that carry bytes"));
        }
        let o = (address - BASE) as usize;
        Ok(&self.ram[o..o + len])
    }
}

/// Reads `SCUS_973.53` from the disc image and lays out the main program as the loader does.
pub fn boot(iso_path: &str) -> Result<Boot, String> {
    let mut d = Disc::open(iso_path)?;
    let v = crate::iso::volume(&mut d)?;
    let f = v.files.iter().find(|f| f.path == "/SCUS_973.53").ok_or("boot: no /SCUS_973.53 on the disc")?;
    let loader = d.bytes_at(f.sector * SECTOR, f.size)?;
    let payload = lz::unpack(loader.get(PAYLOAD_OFFSET..).ok_or("boot: SCUS_973.53 is shorter than its payload's offset")?)?.data;
    let mut records = Vec::new();
    let (mut p, mut entry) = (0usize, None);
    while p + 16 <= payload.len() {
        let (address, size, e) = (u32_at(&payload, p)?, u32_at(&payload, p + 4)? as usize, u32_at(&payload, p + 12)?);
        if *entry.get_or_insert(e) != e {
            break;
        }
        let bytes = payload.get(p + 16..p + 16 + size).ok_or_else(|| format!("boot: record at {p:#x} ({size:#x} bytes) past the payload's end"))?;
        records.push((address, e, u32_at(&payload, p + 8)?, bytes));
        p += 16 + size;
    }
    let loaded: Vec<_> = records.into_iter().filter(|(a, ..)| *a >= BASE).collect();
    let end = loaded.iter().map(|(a, .., b)| *a as usize + b.len()).max().ok_or("boot: the payload has no record at or above 0x100000")?;
    let mut ram = vec![0u8; end - BASE as usize];
    for (a, _, _, b) in &loaded {
        let o = (a - BASE) as usize;
        ram[o..o + b.len()].copy_from_slice(b);
    }
    // The spans of byte-carrying records, a record that starts within 0x80 of the one before (its alignment: the font's last
    // rows at 0x14C2D0 + 0x4000 run on into the next record) joining it.
    let mut sorted: Vec<_> = loaded.iter().map(|(a, _, kind, b)| (*a, *kind, b.len())).collect();
    sorted.sort();
    let mut records: Vec<(u32, usize)> = Vec::new();
    let mut joinable = false;
    for (a, kind, n) in sorted {
        if kind == 8 {
            joinable = false;
            continue;
        }
        match records.last_mut() {
            Some((s, len)) if joinable && a >= *s + *len as u32 && a - (*s + *len as u32) < 0x80 => *len = (a - *s) as usize + n,
            _ => records.push((a, n)),
        }
        joinable = true;
    }
    Ok(Boot { ram, records })
}
