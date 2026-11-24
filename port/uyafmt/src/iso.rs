//! The disc's ISO 9660 volume: the primary volume descriptor, the path tables and the directories, which give every file the
//! PS2 can open by name (SYSTEM.CNF, the boot program, the IOP images, the network GUI and the Sly 2 demo). R&C3's own data is
//! not in the directory: the game reads it by sector through its table of contents (toc.rs).

use crate::disc::{Disc, SECTOR, u32_at};

pub struct Volume {
    pub space: u64,
    pub path_table_size: u64,
    /// L, optional L, M, optional M path table sectors.
    pub path_tables: [u64; 4],
    pub dirs: Vec<Extent>,
    pub files: Vec<Extent>,
}

pub struct Extent {
    pub path: String,
    pub sector: u64,
    pub size: u64,
}

impl Extent {
    pub fn sectors(&self) -> u64 {
        self.size.div_ceil(SECTOR).max(1)
    }
}

pub fn volume(d: &mut Disc) -> Result<Volume, String> {
    let pvd = d.sectors(16, 1)?;
    if pvd[0] != 1 || &pvd[1..6] != b"CD001" {
        return Err("sector 16 is not a primary volume descriptor".into());
    }
    let term = d.sectors(17, 1)?;
    if term[0] != 255 || &term[1..6] != b"CD001" {
        return Err("sector 17 is not the volume descriptor set terminator".into());
    }
    let space = u32_at(&pvd, 80)? as u64;
    let path_table_size = u32_at(&pvd, 132)? as u64;
    let path_tables = [u32_at(&pvd, 140)? as u64, u32_at(&pvd, 144)? as u64, crate::disc::u32_be(&pvd, 148)? as u64, crate::disc::u32_be(&pvd, 152)? as u64];
    let root = (u32_at(&pvd, 156 + 2)? as u64, u32_at(&pvd, 156 + 10)? as u64);
    let mut v = Volume { space, path_table_size, path_tables, dirs: Vec::new(), files: Vec::new() };
    walk(d, root.0, root.1, "", &mut v)?;
    v.files.sort_by_key(|f| f.sector);
    v.dirs.sort_by_key(|f| f.sector);
    Ok(v)
}

fn walk(d: &mut Disc, sector: u64, size: u64, path: &str, v: &mut Volume) -> Result<(), String> {
    v.dirs.push(Extent { path: format!("{path}/"), sector, size });
    let data = d.bytes_at(sector * SECTOR, size)?;
    let mut o = 0usize;
    while o < data.len() {
        let len = data[o] as usize;
        if len == 0 {
            o = (o / SECTOR as usize + 1) * SECTOR as usize;
            continue;
        }
        let rec = &data[o..o + len];
        let (at, n, flags, nl) = (u32_at(rec, 2)? as u64, u32_at(rec, 10)? as u64, rec[25], rec[32] as usize);
        let name = &rec[33..33 + nl];
        if name != [0] && name != [1] {
            let name = String::from_utf8_lossy(name).to_string();
            let name = name.split(';').next().unwrap_or("").to_string();
            let full = format!("{path}/{name}");
            if flags & 2 != 0 {
                walk(d, at, n, &full, v)?;
            } else {
                v.files.push(Extent { path: full, sector: at, size: n });
            }
        }
        o += len;
    }
    Ok(())
}
