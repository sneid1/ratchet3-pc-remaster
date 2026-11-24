//! The disc's volume structures: ISO 9660 (ECMA-119) and the UDF 1.02 bridge (ECMA-167 / OSTA UDF) laid over it, and SYSTEM.CNF.
//! The PS2's boot ROM and CDVD file system read them, not R&C3's code: the ROM reads the logo and SYSTEM.CNF to boot, the CDVD
//! file system reads the volume descriptors, path tables and directories (or the UDF file set) to open files by name.
//!
//! Sectors 0-999 and the disc's last 13 sectors are claimed here. The disc's layout (from its bytes): sectors 0-11 the PS2 logo,
//! 16-17 the ISO 9660 primary volume descriptor and terminator, 18-20 the UDF volume recognition sequence, 32-37 the main and
//! 48-53 the reserve volume descriptor sequences, 64-65 the integrity sequence, 256 and the last sector the anchor pointers,
//! 257-260 the path tables, 261-265 the directories, 266-267 the file set, 268-272 the directories' file identifiers, 273-321
//! the 49 file entries (the UDF partition starts at 266), SYSTEM.CNF at 1000 (claimed by the walk with the ISO files). Every
//! other sector below 1000 and the 12 before the last sector are zero.
//!
//! Every descriptor is read field by field from its standard. A tag's checksum and CRC are verified; every zero or reserved
//! byte is required to be zero; a value the decoder does not expect fails the instance with its offset and value.

use crate::disc::{Disc, SECTOR};
use crate::iso::{Extent, Volume};
use crate::rd::Fields;
use crate::track::{LineId, Space, Tracker};

const ISO: &str = "ISO 9660 volume";
const UDF: &str = "UDF volume";
const ISO_GAME: &str = "the PS2's boot ROM / CDVD file system (ECMA-119), not game code";
const UDF_GAME: &str = "the PS2's boot ROM / CDVD file system (ECMA-167 / OSTA UDF 1.02), not game code";
const CNF_GAME: &str = "the PS2's boot ROM (reads BOOT2, VER and VMODE to start the boot program), not game code";

/// The UDF partition's first sector (the file set descriptor, partition block 0).
const PART: u64 = 266;
/// The partition's blocks that hold file entries: 7..56 (sectors 273..322).
const FE_BLOCKS: std::ops::Range<u32> = 7..56;

type Fl<'a, 'b> = Fields<'a, 'b>;

// ---------------------------------------------------------------------------------------------------------------------
// Reading helpers: every field read through them is marked read.

fn u8_(f: &mut Fl, at: usize) -> Result<u8, String> {
    let v = *f.b.get(at).ok_or_else(|| format!("+{at:#x}: past the end {:#x}", f.b.len()))?;
    f.mark(at, 1);
    Ok(v)
}

fn u16_(f: &mut Fl, at: usize) -> Result<u16, String> {
    let v = crate::disc::u16_at(f.b, at)?;
    f.mark(at, 2);
    Ok(v)
}

fn be16(f: &mut Fl, at: usize) -> Result<u16, String> {
    let v = f.b.get(at..at + 2).map(|s| u16::from_be_bytes([s[0], s[1]])).ok_or_else(|| format!("+{at:#x}: past the end {:#x}", f.b.len()))?;
    f.mark(at, 2);
    Ok(v)
}

fn be32(f: &mut Fl, at: usize) -> Result<u32, String> {
    let v = crate::disc::u32_be(f.b, at)?;
    f.mark(at, 4);
    Ok(v)
}

fn u64_(f: &mut Fl, at: usize) -> Result<u64, String> {
    let s = f.b.get(at..at + 8).ok_or_else(|| format!("+{at:#x}: past the end {:#x}", f.b.len()))?;
    let v = u64::from_le_bytes([s[0], s[1], s[2], s[3], s[4], s[5], s[6], s[7]]);
    f.mark(at, 8);
    Ok(v)
}

fn eq(v: u64, w: u64, what: &str, at: usize) -> Result<(), String> {
    if v == w { Ok(()) } else { Err(format!("+{at:#x} {what}: {v:#x}, expected {w:#x}")) }
}

fn is8(f: &mut Fl, at: usize, w: u8, what: &str) -> Result<(), String> {
    let v = u8_(f, at)?;
    eq(v as u64, w as u64, what, at)
}

fn is16(f: &mut Fl, at: usize, w: u16, what: &str) -> Result<(), String> {
    let v = u16_(f, at)?;
    eq(v as u64, w as u64, what, at)
}

fn is32(f: &mut Fl, at: usize, w: u32, what: &str) -> Result<(), String> {
    let v = f.u32(at)?;
    eq(v as u64, w as u64, what, at)
}

/// A little-endian u16 followed by its big-endian copy (ECMA-119 7.2.3).
fn both16(f: &mut Fl, at: usize, what: &str) -> Result<u16, String> {
    let a = u16_(f, at)?;
    let b = be16(f, at + 2)?;
    if a != b {
        return Err(format!("+{at:#x} {what}: little-endian {a:#x}, big-endian {b:#x}"));
    }
    Ok(a)
}

/// A little-endian u32 followed by its big-endian copy (ECMA-119 7.3.3).
fn both32(f: &mut Fl, at: usize, what: &str) -> Result<u32, String> {
    let a = f.u32(at)?;
    let b = be32(f, at + 4)?;
    if a != b {
        return Err(format!("+{at:#x} {what}: little-endian {a:#x}, big-endian {b:#x}"));
    }
    Ok(a)
}

/// The bytes from `from` to the end of the structure, required zero.
fn rest(f: &mut Fl, from: usize, what: &str) -> Result<(), String> {
    let len = f.b.len();
    if from > len {
        return Err(format!("+{from:#x} {what}: past the end {len:#x}"));
    }
    f.fill(from, len - from, 0, what)
}

fn zeros(f: &mut Fl, at: usize, len: usize, what: &str) -> Result<(), String> {
    f.fill(at, len, 0, what)
}

fn padded(ident: &[u8], pad: u8, n: usize) -> Vec<u8> {
    let mut v = vec![pad; n];
    v[..ident.len()].copy_from_slice(ident);
    v
}

// ---------------------------------------------------------------------------------------------------------------------
// ECMA-167 basic structures.

fn crc16(b: &[u8]) -> u16 {
    let mut crc: u16 = 0;
    for &x in b {
        crc ^= (x as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

/// A descriptor tag (ECMA-167 3/7.2) at `at`: identifier, version 2, checksum, reserved, serial 0, CRC and its length, location.
/// Returns the CRC length.
fn tag(f: &mut Fl, at: usize, id: u16, loc: u32) -> Result<usize, String> {
    let b = f.b;
    let t = b.get(at..at + 16).ok_or_else(|| format!("+{at:#x} tag: past the end {:#x}", b.len()))?;
    let got = u16_(f, at)?;
    eq(got as u64, id as u64, "tag identifier", at)?;
    is16(f, at + 2, 2, "tag descriptor version")?;
    let sum = t.iter().enumerate().filter(|&(i, _)| i != 4).fold(0u8, |s, (_, &x)| s.wrapping_add(x));
    let ck = u8_(f, at + 4)?;
    eq(ck as u64, sum as u64, "tag checksum", at + 4)?;
    is8(f, at + 5, 0, "tag reserved")?;
    is16(f, at + 6, 0, "tag serial number")?;
    let crc = u16_(f, at + 8)?;
    let len = u16_(f, at + 10)? as usize;
    let l = f.u32(at + 12)?;
    eq(l as u64, loc as u64, "tag location", at + 12)?;
    let body = b.get(at + 16..at + 16 + len).ok_or_else(|| format!("+{:#x} tag CRC length {len:#x} runs past the sector", at + 10))?;
    eq(crc as u64, crc16(body) as u64, "tag CRC", at + 8)?;
    Ok(len)
}

/// A timestamp (ECMA-167 1/7.3): type and time zone, year, month, day, hour, minute, second, centiseconds, hundreds of
/// microseconds, microseconds.
fn timestamp(f: &mut Fl, at: usize) -> Result<(), String> {
    let b = f.b;
    let tt = u16_(f, at)?;
    let typ = tt >> 12;
    let mut tz = (tt & 0xfff) as i32;
    if tz & 0x800 != 0 {
        tz -= 0x1000;
    }
    let y = u16_(f, at + 2)?;
    let r = b.get(at + 4..at + 12).ok_or_else(|| format!("+{at:#x} timestamp: past the end {:#x}", b.len()))?;
    f.mark(at + 4, 8);
    let ok = typ <= 2
        && (typ != 1 || (-1440..=1440).contains(&tz) || tz == -2047)
        && (1..=9999).contains(&y)
        && (1..=12).contains(&r[0])
        && (1..=31).contains(&r[1])
        && r[2] < 24
        && r[3] < 60
        && r[4] < 60
        && r[5] < 100
        && r[6] < 100
        && r[7] < 100;
    if ok { Ok(()) } else { Err(format!("+{at:#x} timestamp {:02x?}", &b[at..at + 12])) }
}

/// A dstring of `n` bytes (ECMA-167 1/7.2.12): empty (all zero) or a compression id (8 or 16), the characters, zero padding and
/// the used length (id and characters) in the last byte. Returns the text.
fn dstring(f: &mut Fl, at: usize, n: usize) -> Result<String, String> {
    let b = f.b;
    let x = b.get(at..at + n).ok_or_else(|| format!("+{at:#x} dstring: past the end {:#x}", b.len()))?;
    if x.iter().all(|&c| c == 0) {
        f.mark(at, n);
        return Ok(String::new());
    }
    let (comp, used) = (x[0], x[n - 1] as usize);
    if comp != 8 && comp != 16 {
        return Err(format!("+{at:#x} dstring compression id {comp}"));
    }
    if used < 1 || used > n - 1 {
        return Err(format!("+{at:#x} dstring used length {used} in {n} bytes"));
    }
    if x[used..n - 1].iter().any(|&c| c != 0) {
        return Err(format!("+{at:#x} dstring padding is not zero"));
    }
    f.mark(at, n);
    chars(&x[..used], at)
}

/// The characters of an OSTA compressed-unicode string (compression id, then 8-bit or big-endian 16-bit characters).
fn chars(x: &[u8], at: usize) -> Result<String, String> {
    let body = &x[1..];
    match x[0] {
        8 => Ok(body.iter().map(|&c| c as char).collect()),
        16 if body.len() % 2 == 0 => {
            let u: Vec<u16> = body.chunks(2).map(|p| u16::from_be_bytes([p[0], p[1]])).collect();
            String::from_utf16(&u).map_err(|_| format!("+{at:#x} file identifier is not UTF-16"))
        }
        c => Err(format!("+{at:#x} file identifier compression id {c}, {} bytes", body.len())),
    }
}

/// A charspec (ECMA-167 1/7.2.1): type 0 and "OSTA Compressed Unicode".
fn charspec(f: &mut Fl, at: usize) -> Result<(), String> {
    is8(f, at, 0, "character set type")?;
    f.same(at + 1, &padded(b"OSTA Compressed Unicode", 0, 63), "character set information")
}

/// An entity identifier (ECMA-167 1/7.4): flags, 23 identifier bytes (padded with `pad`), 8 suffix bytes.
fn entity(f: &mut Fl, at: usize, flags: u8, ident: &[u8], pad: u8, suffix: &[u8; 8]) -> Result<(), String> {
    is8(f, at, flags, "entity identifier flags")?;
    f.same(at + 1, &padded(ident, pad, 23), "entity identifier")?;
    f.same(at + 24, suffix, "entity identifier suffix")
}

const SUFFIX_NONE: [u8; 8] = [0; 8];
/// UDF 1.02 (revision 0x0102), OS class and id 0.
const SUFFIX_UDF: [u8; 8] = [2, 1, 0, 0, 0, 0, 0, 0];
/// UDF 1.02, domain flags 3 (hard and soft write protect).
const SUFFIX_DOMAIN: [u8; 8] = [2, 1, 3, 0, 0, 0, 0, 0];

fn generator(f: &mut Fl, at: usize) -> Result<(), String> {
    entity(f, at, 0, b"DVD-ROM GENERATOR", 0, &SUFFIX_NONE)
}

/// An extent_ad (ECMA-167 3/7.1): length, location.
fn extent_ad(f: &mut Fl, at: usize, len: u32, loc: u32) -> Result<(), String> {
    is32(f, at, len, "extent length")?;
    is32(f, at + 4, loc, "extent location")
}

/// A lb_addr (ECMA-167 4/7.1): logical block number, partition reference number.
fn lb_addr(f: &mut Fl, at: usize, block: u32) -> Result<(), String> {
    is32(f, at, block, "logical block number")?;
    is16(f, at + 4, 0, "partition reference number")
}

// ---------------------------------------------------------------------------------------------------------------------
// UDF descriptors.

/// Volume structure descriptors 18-20 (ECMA-167 2/9): type 0, "BEA01" / "NSR02" / "TEA01", version 1, zero.
fn recognition(f: &mut Fl, ident: &[u8; 5]) -> Result<(), String> {
    is8(f, 0, 0, "structure type")?;
    f.same(1, ident, "standard identifier")?;
    is8(f, 6, 1, "structure version")?;
    rest(f, 7, "reserved")
}

/// Anchor volume descriptor pointer (ECMA-167 3/10.2): the main and reserve volume descriptor sequence extents.
fn anchor(f: &mut Fl, s: u64) -> Result<(), String> {
    tag(f, 0, 2, s as u32)?;
    extent_ad(f, 16, 0x8000, 32)?;
    extent_ad(f, 24, 0x8000, 48)?;
    rest(f, 32, "reserved")
}

/// Primary volume descriptor (ECMA-167 3/10.1).
fn vds_primary(f: &mut Fl, s: u64) -> Result<(), String> {
    tag(f, 0, 1, s as u32)?;
    is32(f, 16, 0, "volume descriptor sequence number")?;
    is32(f, 20, 0, "primary volume descriptor number")?;
    dstring(f, 24, 32)?;
    is16(f, 56, 1, "volume sequence number")?;
    is16(f, 58, 1, "maximum volume sequence number")?;
    is16(f, 60, 2, "interchange level")?;
    is16(f, 62, 2, "maximum interchange level")?;
    is32(f, 64, 1, "character set list")?;
    is32(f, 68, 1, "maximum character set list")?;
    dstring(f, 72, 128)?;
    charspec(f, 200)?;
    charspec(f, 264)?;
    extent_ad(f, 328, 0, 0)?;
    extent_ad(f, 336, 0, 0)?;
    entity(f, 344, 0, b"PLAYSTATION", b' ', &SUFFIX_NONE)?;
    timestamp(f, 376)?;
    generator(f, 388)?;
    zeros(f, 420, 64, "implementation use")?;
    is32(f, 484, 0, "predecessor volume descriptor sequence location")?;
    is16(f, 488, 0, "flags")?;
    rest(f, 490, "reserved")
}

/// Implementation use volume descriptor (ECMA-167 3/10.4, UDF "*UDF LV Info").
fn vds_impl_use(f: &mut Fl, s: u64) -> Result<(), String> {
    tag(f, 0, 4, s as u32)?;
    is32(f, 16, 1, "volume descriptor sequence number")?;
    entity(f, 20, 0, b"*UDF LV Info", 0, &SUFFIX_UDF)?;
    charspec(f, 52)?;
    dstring(f, 116, 128)?;
    for k in 0..3 {
        dstring(f, 244 + 36 * k, 36)?;
    }
    generator(f, 352)?;
    zeros(f, 384, 128, "implementation use")?;
    rest(f, 512, "reserved")
}

/// Partition descriptor (ECMA-167 3/10.5): the partition starts at sector 266 and runs to the sector before the last.
fn vds_partition(f: &mut Fl, s: u64, last: u64) -> Result<(), String> {
    tag(f, 0, 5, s as u32)?;
    is32(f, 16, 2, "volume descriptor sequence number")?;
    is16(f, 20, 1, "partition flags (allocated)")?;
    is16(f, 22, 0, "partition number")?;
    entity(f, 24, 2, b"+NSR02", 0, &SUFFIX_NONE)?;
    zeros(f, 56, 128, "partition contents use")?;
    is32(f, 184, 1, "access type (read only)")?;
    is32(f, 188, PART as u32, "partition starting location")?;
    is32(f, 192, (last - PART) as u32, "partition length")?;
    generator(f, 196)?;
    zeros(f, 228, 128, "implementation use")?;
    rest(f, 356, "reserved")
}

/// Logical volume descriptor (ECMA-167 3/10.6): 2048-byte blocks, the file set at partition block 0 (2 blocks), the integrity
/// sequence at sector 64 (2 sectors), one type 1 partition map.
fn vds_logical_volume(f: &mut Fl, s: u64) -> Result<(), String> {
    tag(f, 0, 6, s as u32)?;
    is32(f, 16, 3, "volume descriptor sequence number")?;
    charspec(f, 20)?;
    dstring(f, 84, 128)?;
    is32(f, 212, SECTOR as u32, "logical block size")?;
    entity(f, 216, 0, b"*OSTA UDF Compliant", 0, &SUFFIX_DOMAIN)?;
    // Logical volume contents use: a long_ad of the file set descriptor.
    is32(f, 248, 0x1000, "file set descriptor extent length")?;
    lb_addr(f, 252, 0)?;
    zeros(f, 258, 6, "file set descriptor implementation use")?;
    is32(f, 264, 6, "map table length")?;
    is32(f, 268, 1, "number of partition maps")?;
    generator(f, 272)?;
    zeros(f, 304, 128, "implementation use")?;
    extent_ad(f, 432, 0x1000, 64)?;
    // Type 1 partition map (ECMA-167 3/10.7.2): type, length, volume sequence number, partition number.
    is8(f, 440, 1, "partition map type")?;
    is8(f, 441, 6, "partition map length")?;
    is16(f, 442, 1, "volume sequence number")?;
    is16(f, 444, 0, "partition number")?;
    rest(f, 446, "reserved")
}

/// Unallocated space descriptor (ECMA-167 3/10.8): no extents.
fn vds_unallocated(f: &mut Fl, s: u64) -> Result<(), String> {
    tag(f, 0, 7, s as u32)?;
    is32(f, 16, 4, "volume descriptor sequence number")?;
    is32(f, 20, 0, "number of allocation descriptors")?;
    rest(f, 24, "reserved")
}

/// Terminating descriptor (ECMA-167 3/10.9): the tag and 496 reserved bytes. The file set's one (sector 267) carries the
/// absolute sector in its tag location.
fn terminating(f: &mut Fl, s: u64) -> Result<(), String> {
    tag(f, 0, 8, s as u32)?;
    rest(f, 16, "reserved")
}

/// Logical volume integrity descriptor (ECMA-167 3/10.10, UDF 2.2.6): closed, no free space, the file and directory counts the
/// file entries must match.
fn integrity(f: &mut Fl, s: u64, last: u64, files: u32, dirs: u32) -> Result<(), String> {
    tag(f, 0, 9, s as u32)?;
    timestamp(f, 16)?;
    is32(f, 28, 1, "integrity type (close)")?;
    extent_ad(f, 32, 0, 0)?;
    // Logical volume header descriptor: the next unique id, then 24 reserved bytes.
    u64_(f, 40)?;
    zeros(f, 48, 24, "logical volume header reserved")?;
    is32(f, 72, 1, "number of partitions")?;
    is32(f, 76, 48, "length of implementation use")?;
    is32(f, 80, 0, "free space table")?;
    is32(f, 84, (last - PART) as u32, "size table")?;
    generator(f, 88)?;
    is32(f, 120, files, "number of files (the file entries decoded)")?;
    is32(f, 124, dirs, "number of directories (the file entries decoded)")?;
    is16(f, 128, 0x102, "minimum UDF read revision")?;
    is16(f, 130, 0x102, "minimum UDF write revision")?;
    is16(f, 132, 0x102, "maximum UDF write revision")?;
    zeros(f, 134, 2, "implementation use")?;
    rest(f, 136, "reserved")
}

/// File set descriptor (ECMA-167 4/14.1): its root directory's ICB is partition block 7 (a 316-byte file entry).
fn file_set(f: &mut Fl, s: u64) -> Result<(), String> {
    tag(f, 0, 256, (s - PART) as u32)?;
    timestamp(f, 16)?;
    is16(f, 28, 3, "interchange level")?;
    is16(f, 30, 3, "maximum interchange level")?;
    is32(f, 32, 1, "character set list")?;
    is32(f, 36, 1, "maximum character set list")?;
    is32(f, 40, 0, "file set number")?;
    is32(f, 44, 0, "file set descriptor number")?;
    charspec(f, 48)?;
    dstring(f, 112, 128)?;
    charspec(f, 240)?;
    dstring(f, 304, 32)?;
    dstring(f, 336, 32)?;
    dstring(f, 368, 32)?;
    is32(f, 400, 316, "root directory ICB extent length")?;
    lb_addr(f, 404, FE_BLOCKS.start)?;
    zeros(f, 410, 6, "root directory ICB implementation use")?;
    entity(f, 416, 0, b"*OSTA UDF Compliant", 0, &SUFFIX_DOMAIN)?;
    zeros(f, 448, 16, "next extent")?;
    zeros(f, 464, 16, "system stream directory ICB")?;
    zeros(f, 480, 32, "reserved")?;
    rest(f, 512, "reserved")
}

/// What a file entry gives the walk: its type (4 directory, 5 file), information length and allocation extents (length,
/// partition block).
struct FileEntry {
    file_type: u8,
    extents: Vec<(u32, u32)>,
}

/// File entry (ECMA-167 4/14.9) with its extended attribute header and attributes (UDF 3.3.4.5: free EA space, DVD CGMS info)
/// and short allocation descriptors.
fn file_entry(f: &mut Fl, s: u64) -> Result<FileEntry, String> {
    let b = f.b;
    let block = (s - PART) as u32;
    let crc_len = tag(f, 0, 261, block)?;
    // ICB tag (ECMA-167 4/14.6).
    is32(f, 16, 0, "prior recorded number of direct entries")?;
    is16(f, 20, 4, "strategy type")?;
    is16(f, 22, 0, "strategy parameter")?;
    is16(f, 24, 1, "maximum number of entries")?;
    is8(f, 26, 0, "ICB reserved")?;
    let file_type = u8_(f, 27)?;
    if file_type != 4 && file_type != 5 {
        return Err(format!("+0x1b file type {file_type}, expected 4 (directory) or 5 (file)"));
    }
    lb_addr(f, 28, 0)?;
    is16(f, 34, 0x0630, "ICB flags (short allocation descriptors; contiguous, archive, non-relocatable, system)")?;
    is32(f, 36, 0xffff_ffff, "uid (not set)")?;
    is32(f, 40, 0xffff_ffff, "gid (not set)")?;
    is32(f, 44, 0x14a5, "permissions")?;
    u16_(f, 48)?;
    is8(f, 50, 0, "record format")?;
    is8(f, 51, 0, "record display attributes")?;
    is32(f, 52, 0, "record length")?;
    let info_len = u64_(f, 56)?;
    let blocks = u64_(f, 64)?;
    for at in [72, 84, 96] {
        timestamp(f, at)?;
    }
    is32(f, 108, 1, "checkpoint")?;
    zeros(f, 112, 16, "extended attribute ICB")?;
    generator(f, 128)?;
    u64_(f, 160)?;
    let l_ea = f.u32(168)? as usize;
    let l_ad = f.u32(172)? as usize;
    let end = 176 + l_ea + l_ad;
    if end > b.len() {
        return Err(format!("+0xa8 extended attributes {l_ea:#x} and allocation descriptors {l_ad:#x} run past the sector"));
    }
    eq(crc_len as u64, (end - 16) as u64, "tag CRC length", 10)?;
    // Extended attribute header descriptor (ECMA-167 4/14.10.1) and its attributes.
    let e = 176;
    let hl = tag(f, e, 262, block)?;
    eq(hl as u64, 8, "extended attribute header CRC length", e + 10)?;
    let ia = f.u32(e + 16)? as usize;
    let aa = f.u32(e + 20)? as usize;
    eq(ia as u64, 24, "implementation attributes location", e + 16)?;
    eq(aa as u64, l_ea as u64, "application attributes location", e + 20)?;
    let mut at = ia;
    while at < l_ea {
        let o = e + at;
        is32(f, o, 2048, "extended attribute type (implementation use)")?;
        is8(f, o + 4, 1, "extended attribute subtype")?;
        zeros(f, o + 5, 3, "extended attribute reserved")?;
        let al = f.u32(o + 8)? as usize;
        let iul = f.u32(o + 12)? as usize;
        let id = b.get(o + 17..o + 40).ok_or_else(|| format!("+{o:#x} extended attribute identifier past the end"))?;
        let ident: Vec<u8> = id.iter().copied().take_while(|&c| c != 0).collect();
        let (want_al, want_iul): (usize, usize) = match ident.as_slice() {
            b"*UDF FreeEASpace" => (52, 4),
            b"*UDF DVD CGMS Info" => (56, 8),
            _ => return Err(format!("+{:#x} extended attribute identifier {:?}", o + 16, String::from_utf8_lossy(&ident))),
        };
        eq(al as u64, want_al as u64, "extended attribute length", o + 8)?;
        if o + want_al > e + l_ea {
            return Err(format!("+{o:#x} extended attribute of {want_al} bytes runs past the extended attributes' {l_ea:#x} bytes"));
        }
        eq(iul as u64, want_iul as u64, "extended attribute implementation use length", o + 12)?;
        entity(f, o + 16, 0, &ident, 0, &SUFFIX_UDF)?;
        // The header checksum: the 16-bit sum of the attribute's first 48 bytes (UDF 3.3.4.5.1.2).
        let sum = b[o..o + 48].iter().fold(0u16, |s, &x| s.wrapping_add(x as u16));
        let ck = u16_(f, o + 48)?;
        eq(ck as u64, sum as u64, "extended attribute header checksum", o + 48)?;
        zeros(f, o + 50, iul - 2, "extended attribute data (free space, CGMS information, protection system information)")?;
        at += al;
        if at > l_ea {
            return Err(format!("+{o:#x} extended attribute runs past the extended attributes' {l_ea:#x} bytes"));
        }
    }
    // Short allocation descriptors (ECMA-167 4/14.14.1): extent length (type in the top two bits) and partition block.
    eq((l_ad % 8) as u64, 0, "allocation descriptors length", 172)?;
    let mut extents = Vec::new();
    let mut total = 0u64;
    for k in 0..l_ad / 8 {
        let at = 176 + l_ea + 8 * k;
        let len = f.u32(at)?;
        let pos = f.u32(at + 4)?;
        if len >> 30 != 0 {
            return Err(format!("+{at:#x} extent type {}, expected 0 (recorded and allocated)", len >> 30));
        }
        total += len as u64;
        extents.push((len, pos));
    }
    eq(total, info_len, "allocation extents' total against the information length", 56)?;
    eq(blocks, info_len.div_ceil(SECTOR), "logical blocks recorded", 64)?;
    rest(f, end, "reserved")?;
    Ok(FileEntry { file_type, extents })
}

/// A directory's file identifier descriptors (ECMA-167 4/14.4) in its sector: `length` bytes, then zero.
fn identifiers(f: &mut Fl, s: u64, length: usize) -> Result<(), String> {
    let block = (s - PART) as u32;
    let mut at = 0;
    while at < length {
        let crc_len = tag(f, at, 257, block)?;
        is16(f, at + 16, 1, "file version number")?;
        let ch = u8_(f, at + 18)?;
        if !matches!(ch, 0 | 2 | 0x0a) {
            return Err(format!("+{:#x} file characteristics {ch:#x}, expected 0 (file), 2 (directory) or 0xa (parent)", at + 18));
        }
        let l_fi = u8_(f, at + 19)? as usize;
        is32(f, at + 20, 316, "ICB extent length")?;
        let blk = f.u32(at + 24)?;
        is16(f, at + 28, 0, "ICB partition reference number")?;
        zeros(f, at + 30, 6, "ICB implementation use")?;
        let l_iu = u16_(f, at + 36)? as usize;
        eq(l_iu as u64, 0, "length of implementation use", at + 36)?;
        let total = (38 + l_iu + l_fi).div_ceil(4) * 4;
        eq(crc_len as u64, (total - 16) as u64, "tag CRC length", at + 10)?;
        if !FE_BLOCKS.contains(&blk) {
            return Err(format!("+{:#x} ICB block {blk}, the file entries are blocks {}..{}", at + 24, FE_BLOCKS.start, FE_BLOCKS.end));
        }
        if l_fi == 0 {
            eq(ch as u64, 0x0a, "file characteristics of a nameless entry", at + 18)?;
        } else {
            let x = f.b.get(at + 38..at + 38 + l_fi).ok_or_else(|| format!("+{:#x} file identifier past the end", at + 38))?;
            chars(x, at + 38)?;
            f.mark(at + 38, l_fi);
        }
        zeros(f, at + 38 + l_fi, total - 38 - l_fi, "file identifier descriptor padding")?;
        at += total;
    }
    eq(at as u64, length as u64, "file identifier descriptors' end", at)?;
    rest(f, length, "directory sector padding")
}

// ---------------------------------------------------------------------------------------------------------------------
// ISO 9660 structures.

const A_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 _!\"%&'()*+,-./:;<=>?";
const NAME_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_.;";

fn a_chars(f: &mut Fl, at: usize, n: usize, what: &str) -> Result<(), String> {
    let x = f.b.get(at..at + n).ok_or_else(|| format!("+{at:#x} {what}: past the end"))?;
    if let Some(k) = x.iter().position(|c| !A_CHARS.contains(c)) {
        return Err(format!("+{:#x} {what}: byte {:#04x} is not an a-character", at + k, x[k]));
    }
    f.mark(at, n);
    Ok(())
}

/// A volume date and time (ECMA-119 8.4.26.1): 16 digits and a time zone byte, or "unspecified" (16 '0' characters and 0).
fn iso_date(f: &mut Fl, at: usize) -> Result<(), String> {
    let b = f.b;
    let x = b.get(at..at + 17).ok_or_else(|| format!("+{at:#x} date: past the end"))?;
    if x[..16].iter().all(|&c| c == b'0') && x[16] == 0 {
        f.mark(at, 17);
        return Ok(());
    }
    if !x[..16].iter().all(|c| c.is_ascii_digit()) {
        return Err(format!("+{at:#x} date {:?}", String::from_utf8_lossy(x)));
    }
    let n = |a: usize, len: usize| x[a..a + len].iter().fold(0u32, |s, &c| s * 10 + (c - b'0') as u32);
    let tz = x[16] as i8;
    let ok = (1..=9999).contains(&n(0, 4)) && (1..=12).contains(&n(4, 2)) && (1..=31).contains(&n(6, 2)) && n(8, 2) < 24 && n(10, 2) < 60 && n(12, 2) < 60 && (-48..=52).contains(&tz);
    if !ok {
        return Err(format!("+{at:#x} date {:?}", String::from_utf8_lossy(x)));
    }
    f.mark(at, 17);
    Ok(())
}

struct Record {
    len: usize,
    extent: u32,
    size: u32,
    flags: u8,
    name: Vec<u8>,
}

/// A directory record (ECMA-119 9.1) at `at`: length, extended attribute length, extent and size (both-endian), the 7-byte
/// recording date, flags, unit size, interleave gap, volume sequence number, the identifier, its padding and the zero system
/// use bytes.
fn dir_record(f: &mut Fl, at: usize) -> Result<Record, String> {
    let b = f.b;
    let len = u8_(f, at)? as usize;
    is8(f, at + 1, 0, "extended attribute record length")?;
    let extent = both32(f, at + 2, "extent location")?;
    let size = both32(f, at + 10, "data length")?;
    let y = u8_(f, at + 18)?;
    let mo = u8_(f, at + 19)?;
    let d = u8_(f, at + 20)?;
    let h = u8_(f, at + 21)?;
    let mi = u8_(f, at + 22)?;
    let sc = u8_(f, at + 23)?;
    let tz = u8_(f, at + 24)? as i8;
    if !(y > 0 && (1..=12).contains(&mo) && (1..=31).contains(&d) && h < 24 && mi < 60 && sc < 60 && (-48..=52).contains(&tz)) {
        return Err(format!("+{:#x} recording date {:02x?}", at + 18, &b[at + 18..at + 25]));
    }
    let flags = u8_(f, at + 25)?;
    if flags != 0 && flags != 2 {
        return Err(format!("+{:#x} file flags {flags:#x}, expected 0 (file) or 2 (directory)", at + 25));
    }
    is8(f, at + 26, 0, "file unit size")?;
    is8(f, at + 27, 0, "interleave gap size")?;
    let seq = both16(f, at + 28, "volume sequence number")?;
    eq(seq as u64, 1, "volume sequence number", at + 28)?;
    let l_fi = u8_(f, at + 32)? as usize;
    let name = b.get(at + 33..at + 33 + l_fi).ok_or_else(|| format!("+{:#x} file identifier past the end", at + 33))?.to_vec();
    if l_fi == 1 && (name[0] == 0 || name[0] == 1) {
        f.mark(at + 33, 1);
    } else {
        if l_fi == 0 || name.iter().any(|c| !NAME_CHARS.contains(c)) {
            return Err(format!("+{:#x} file identifier {:?}", at + 33, String::from_utf8_lossy(&name)));
        }
        f.mark(at + 33, l_fi);
    }
    let pad = if l_fi % 2 == 0 { 1 } else { 0 };
    let used = 33 + l_fi + pad;
    if len < used {
        return Err(format!("+{at:#x} record length {len} for an identifier of {l_fi} bytes"));
    }
    zeros(f, at + 33 + l_fi, pad, "identifier padding")?;
    zeros(f, at + used, len - used, "system use")?;
    Ok(Record { len, extent, size, flags, name })
}

/// Sector 16: the primary volume descriptor (ECMA-119 8.4), cross-checked against the directory walk.
fn iso_primary(f: &mut Fl, total: u64, v: &Volume) -> Result<(), String> {
    is8(f, 0, 1, "volume descriptor type (primary)")?;
    f.same(1, b"CD001", "standard identifier")?;
    is8(f, 6, 1, "volume descriptor version")?;
    is8(f, 7, 0, "unused")?;
    a_chars(f, 8, 32, "system identifier")?;
    a_chars(f, 40, 32, "volume identifier")?;
    zeros(f, 72, 8, "unused")?;
    let space = both32(f, 80, "volume space size")?;
    eq(space as u64, total, "volume space size against the image", 80)?;
    zeros(f, 88, 32, "unused")?;
    let n = both16(f, 120, "volume set size")?;
    eq(n as u64, 1, "volume set size", 120)?;
    let n = both16(f, 124, "volume sequence number")?;
    eq(n as u64, 1, "volume sequence number", 124)?;
    let n = both16(f, 128, "logical block size")?;
    eq(n as u64, SECTOR, "logical block size", 128)?;
    let pts = both32(f, 132, "path table size")?;
    eq(pts as u64, v.path_table_size, "path table size against the directory walk", 132)?;
    let l1 = f.u32(140)?;
    let l2 = f.u32(144)?;
    let m1 = be32(f, 148)?;
    let m2 = be32(f, 152)?;
    for (k, got) in [l1, l2, m1, m2].into_iter().enumerate() {
        eq(got as u64, v.path_tables[k], "path table location against the directory walk", 140 + 4 * k)?;
    }
    let root = dir_record(f, 156)?;
    eq(root.len as u64, 34, "root directory record length", 156)?;
    eq(root.flags as u64, 2, "root directory record flags", 181)?;
    eq(root.name[0] as u64, 0, "root directory record identifier", 189)?;
    if !v.dirs.iter().any(|e| e.path == "/" && e.sector == root.extent as u64 && e.size == root.size as u64) {
        return Err(format!("+0x9c root directory extent {} size {} is not the directory walk's root", root.extent, root.size));
    }
    a_chars(f, 190, 128, "volume set identifier")?;
    a_chars(f, 318, 128, "publisher identifier")?;
    a_chars(f, 446, 128, "data preparer identifier")?;
    a_chars(f, 574, 128, "application identifier")?;
    a_chars(f, 702, 37, "copyright file identifier")?;
    a_chars(f, 739, 37, "abstract file identifier")?;
    a_chars(f, 776, 37, "bibliographic file identifier")?;
    for at in [813, 830, 847, 864] {
        iso_date(f, at)?;
    }
    is8(f, 881, 1, "file structure version")?;
    is8(f, 882, 0, "reserved")?;
    zeros(f, 883, 512, "application use")?;
    rest(f, 1395, "reserved")
}

/// Sector 17: the volume descriptor set terminator (ECMA-119 8.3).
fn iso_terminator(f: &mut Fl) -> Result<(), String> {
    is8(f, 0, 255, "volume descriptor type (terminator)")?;
    f.same(1, b"CD001", "standard identifier")?;
    is8(f, 6, 1, "volume descriptor version")?;
    rest(f, 7, "reserved")
}

/// A path table (ECMA-119 9.4), little-endian (L) or big-endian (M): each record is the identifier length, extended attribute
/// length, extent, parent directory number, the identifier and a pad byte when the length is odd. Cross-checked against the
/// directories the walk found.
fn path_table(f: &mut Fl, size: usize, big: bool, dirs: &[Extent]) -> Result<(), String> {
    let b = f.b;
    let mut at = 0;
    let mut paths: Vec<String> = Vec::new();
    let mut found: Vec<(String, u64)> = Vec::new();
    while at < size {
        let l_di = u8_(f, at)? as usize;
        is8(f, at + 1, 0, "extended attribute record length")?;
        let (extent, parent) = if big { (be32(f, at + 2)?, be16(f, at + 6)?) } else { (f.u32(at + 2)?, u16_(f, at + 6)?) };
        let name = b.get(at + 8..at + 8 + l_di).ok_or_else(|| format!("+{:#x} directory identifier past the end", at + 8))?;
        let path = if paths.is_empty() {
            if l_di != 1 || name[0] != 0 || parent != 1 {
                return Err(format!("+{at:#x} first record is not the root: identifier {name:02x?}, parent {parent}"));
            }
            f.mark(at + 8, 1);
            "/".to_string()
        } else {
            if l_di == 0 || name.iter().any(|c| !NAME_CHARS.contains(c)) {
                return Err(format!("+{:#x} directory identifier {:?}", at + 8, String::from_utf8_lossy(name)));
            }
            f.mark(at + 8, l_di);
            let up = (parent as usize).checked_sub(1).and_then(|i| paths.get(i)).ok_or_else(|| format!("+{:#x} parent directory number {parent} is not an earlier record", at + 6))?;
            format!("{up}{}/", String::from_utf8_lossy(name))
        };
        let pad = l_di % 2;
        zeros(f, at + 8 + l_di, pad, "padding")?;
        paths.push(path.clone());
        found.push((path, extent as u64));
        at += 8 + l_di + pad;
    }
    eq(at as u64, size as u64, "path table's end", at)?;
    let mut want: Vec<(String, u64)> = dirs.iter().map(|e| (e.path.clone(), e.sector)).collect();
    found.sort();
    want.sort();
    if found != want {
        return Err(format!("the directories {found:?} are not the directory walk's {want:?}"));
    }
    rest(f, size, "padding")
}

/// A directory (ECMA-119 6.8): `size` bytes of records, then zero to the end of the sector. Its "." record is itself; every
/// sub-directory record is a directory the walk found.
fn directory(f: &mut Fl, s: u64, size: usize, dirs: &[Extent]) -> Result<(), String> {
    let mut at = 0;
    let mut n = 0;
    while at < size {
        let r = dir_record(f, at)?;
        match n {
            0 | 1 => {
                if r.name != [n as u8] {
                    return Err(format!("+{at:#x} record {n} is {:?}, expected {:?}", r.name, [n as u8]));
                }
            }
            _ => {}
        }
        if n == 0 && (r.extent as u64 != s || r.size as usize != size) {
            return Err(format!("+{at:#x} '.' record extent {} size {}, expected {s} and {size}", r.extent, r.size));
        }
        if n >= 2 && r.flags == 2 && !dirs.iter().any(|e| e.sector == r.extent as u64 && e.size == r.size as u64) {
            return Err(format!("+{at:#x} directory {:?} at {} size {} is not one the walk found", String::from_utf8_lossy(&r.name), r.extent, r.size));
        }
        at += r.len;
        n += 1;
    }
    eq(at as u64, size as u64, "records' end", at)?;
    rest(f, size, "padding")
}

// ---------------------------------------------------------------------------------------------------------------------
// The walk's entry point.

/// Claims `count` sectors from `sector` as one instance of `line` held by `holder`, and decodes it with `read`.
fn inst<R>(t: &mut Tracker, d: &mut Disc, group: &str, holder: &str, line: LineId, sector: u64, count: u64, read: R) -> Result<(), String>
where
    R: FnOnce(&mut Fl) -> Result<(), String>,
{
    t.claim(sector, count, &format!("{group} {holder}"));
    let b = d.sectors(sector, count)?;
    let mut root = t.root(group, holder, count * SECTOR, Space::Disc);
    let mut n = t.child(&mut root, line, 0, count * SECTOR);
    if let Err(e) = read(&mut Fields::new(&b, &mut n)) {
        n.fail(e);
    }
    t.close(n);
    t.close(root);
    Ok(())
}

fn sector_name(s: u64) -> String {
    format!("sector {s}")
}

/// Every sector in [0, 1000) and the disc's last 13: the PS2 logo (left unread), the ISO 9660 and UDF structures and the zero
/// sectors between them.
pub fn structures(t: &mut Tracker, d: &mut Disc, v: &Volume) -> Result<(), String> {
    let total = d.sectors;
    let last = total - 1;

    // ISO 9660 volume.
    let l_logo = t.ignored_line(ISO, "PS2 logo (system area sectors 0-11)", "irrelevant to the game: only the PS2's boot ROM reads it (it draws it at boot); no code on the disc reads it", Space::Disc);
    t.note(l_logo, "24,576 bytes the boot ROM reads and draws; 18,177 are 0x4d and 2,946 are 0xb2, the rest short runs of other bytes that recur at a 0x180 stride (0x42f, 0x5af, 0x72f), so it is a scrambled bitmap; its scrambling and layout cannot be established from the bytes alone");
    let l_zero_iso = t.line(ISO, "system area after the logo (sectors 12-15, zero)", "volume::zero", ISO_GAME, Space::Disc);
    let l_pvd = t.line(ISO, "primary volume descriptor", "volume::iso_primary", ISO_GAME, Space::Disc);
    let l_term = t.line(ISO, "volume descriptor set terminator", "volume::iso_terminator", ISO_GAME, Space::Disc);
    let l_pt = t.line(ISO, "path table (L, optional L, M, optional M)", "volume::path_table", ISO_GAME, Space::Disc);
    let l_dir = t.line(ISO, "directory records", "volume::directory", ISO_GAME, Space::Disc);

    // UDF volume.
    let l_vrs = t.line(UDF, "volume recognition sequence (BEA01, NSR02, TEA01)", "volume::recognition", UDF_GAME, Space::Disc);
    let l_avdp = t.line(UDF, "anchor volume descriptor pointer", "volume::anchor", UDF_GAME, Space::Disc);
    let l_prim = t.line(UDF, "primary volume descriptor", "volume::vds_primary", UDF_GAME, Space::Disc);
    let l_iu = t.line(UDF, "implementation use volume descriptor", "volume::vds_impl_use", UDF_GAME, Space::Disc);
    let l_part = t.line(UDF, "partition descriptor", "volume::vds_partition", UDF_GAME, Space::Disc);
    let l_lv = t.line(UDF, "logical volume descriptor", "volume::vds_logical_volume", UDF_GAME, Space::Disc);
    let l_usd = t.line(UDF, "unallocated space descriptor", "volume::vds_unallocated", UDF_GAME, Space::Disc);
    let l_td = t.line(UDF, "terminating descriptor", "volume::terminating", UDF_GAME, Space::Disc);
    let l_lvid = t.line(UDF, "logical volume integrity descriptor", "volume::integrity", UDF_GAME, Space::Disc);
    let l_fsd = t.line(UDF, "file set descriptor", "volume::file_set", UDF_GAME, Space::Disc);
    let l_fid = t.line(UDF, "file identifier descriptors of a directory", "volume::identifiers", UDF_GAME, Space::Disc);
    let l_fe = t.line(UDF, "file entry (with its extended attributes and allocation descriptors)", "volume::file_entry", UDF_GAME, Space::Disc);
    let l_zero = t.line(UDF, "zero-filled sectors between the volume structures", "volume::zero", UDF_GAME, Space::Disc);

    inst(t, d, ISO, "sectors 0-11", l_logo, 0, 12, |_| Ok(()))?;
    inst(t, d, ISO, "sectors 12-15", l_zero_iso, 12, 4, |f| rest(f, 0, "system area"))?;
    inst(t, d, ISO, "sector 16", l_pvd, 16, 1, |f| iso_primary(f, total, v))?;
    inst(t, d, ISO, "sector 17", l_term, 17, 1, iso_terminator)?;
    for (s, ident) in [(18, b"BEA01"), (19, b"NSR02"), (20, b"TEA01")] {
        inst(t, d, UDF, &sector_name(s), l_vrs, s, 1, |f| recognition(f, ident))?;
    }
    for (a, b) in [(21, 32), (38, 48), (54, 64), (66, 256), (322, 1000), (last - 12, last)] {
        inst(t, d, UDF, &format!("sectors {a}-{}", b - 1), l_zero, a, b - a, |f| rest(f, 0, "unused sectors"))?;
    }

    // The main (32) and reserve (48) volume descriptor sequences, the integrity sequence and the anchors.
    for base in [32u64, 48] {
        inst(t, d, UDF, &sector_name(base), l_prim, base, 1, |f| vds_primary(f, base))?;
        inst(t, d, UDF, &sector_name(base + 1), l_iu, base + 1, 1, |f| vds_impl_use(f, base + 1))?;
        inst(t, d, UDF, &sector_name(base + 2), l_part, base + 2, 1, |f| vds_partition(f, base + 2, last))?;
        inst(t, d, UDF, &sector_name(base + 3), l_lv, base + 3, 1, |f| vds_logical_volume(f, base + 3))?;
        inst(t, d, UDF, &sector_name(base + 4), l_usd, base + 4, 1, |f| vds_unallocated(f, base + 4))?;
        inst(t, d, UDF, &sector_name(base + 5), l_td, base + 5, 1, |f| terminating(f, base + 5))?;
    }
    inst(t, d, UDF, "sector 65", l_td, 65, 1, |f| terminating(f, 65))?;
    inst(t, d, UDF, "sector 256", l_avdp, 256, 1, |f| anchor(f, 256))?;
    inst(t, d, UDF, &sector_name(last), l_avdp, last, 1, |f| anchor(f, last))?;

    // The path tables and directories: sectors the directory walk gave.
    let pt_sectors = v.path_table_size.div_ceil(SECTOR).max(1);
    for (k, &s) in v.path_tables.iter().enumerate() {
        let big = k >= 2;
        inst(t, d, ISO, &sector_name(s), l_pt, s, pt_sectors, |f| path_table(f, v.path_table_size as usize, big, &v.dirs))?;
    }
    for e in &v.dirs {
        inst(t, d, ISO, &e.path, l_dir, e.sector, e.sectors(), |f| directory(f, e.sector, e.size as usize, &v.dirs))?;
    }

    // The file set, its terminator, the file entries and the directories' file identifiers.
    inst(t, d, UDF, "sector 266", l_fsd, PART, 1, |f| file_set(f, PART))?;
    inst(t, d, UDF, "sector 267", l_td, PART + 1, 1, |f| terminating(f, PART + 1))?;
    let (mut files, mut dirs) = (0u32, 0u32);
    let mut dir_extents: Vec<(u32, u32)> = Vec::new();
    for s in PART + FE_BLOCKS.start as u64..PART + FE_BLOCKS.end as u64 {
        let mut got: Option<FileEntry> = None;
        inst(t, d, UDF, &sector_name(s), l_fe, s, 1, |f| {
            got = Some(file_entry(f, s)?);
            Ok(())
        })?;
        if let Some(e) = got {
            if e.file_type == 4 {
                dirs += 1;
                if let [one] = e.extents.as_slice() {
                    dir_extents.push(*one);
                }
            } else {
                files += 1;
            }
        }
    }
    // A directory's identifiers lie in one sector among 268..273 (partition blocks 2..7).
    for (len, pos) in dir_extents {
        let s = PART + pos as u64;
        if !(PART + 2..PART + FE_BLOCKS.start as u64).contains(&s) || len as u64 > SECTOR {
            return Err(format!("a directory's identifiers at partition block {pos}, {len} bytes, are not in sectors 268..273"));
        }
        inst(t, d, UDF, &sector_name(s), l_fid, s, 1, |f| identifiers(f, s, len as usize))?;
    }
    inst(t, d, UDF, "sector 64", l_lvid, 64, 1, |f| integrity(f, 64, last, files, dirs))?;
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------------
// SYSTEM.CNF.

/// SYSTEM.CNF (sector 1000): the text file the boot ROM reads. Each line is `KEY = value ` and CR LF; the keys are BOOT2 (the
/// boot program, `cdrom0:\NAME;1`), VER and VMODE (NTSC or PAL), in that order.
pub fn system_cnf(t: &mut Tracker, root: &mut crate::track::Node, b: &[u8]) -> Result<(), String> {
    let l = t.line("ISO file /SYSTEM.CNF", "SYSTEM.CNF text (BOOT2, VER and VMODE lines)", "volume::system_cnf", CNF_GAME, Space::Disc);
    let mut n = t.child(root, l, 0, b.len() as u64);
    if let Err(e) = cnf(&mut Fields::new(b, &mut n)) {
        n.fail(e);
    }
    t.close(n);
    Ok(())
}

fn cnf(f: &mut Fl) -> Result<(), String> {
    let b = f.b;
    let mut at = 0;
    let mut keys: Vec<String> = Vec::new();
    while at < b.len() {
        let end = b[at..].windows(2).position(|w| w == b"\r\n").ok_or_else(|| format!("+{at:#x} a line without CR LF"))? + at;
        let line = std::str::from_utf8(&b[at..end]).ok().filter(|s| s.is_ascii()).ok_or_else(|| format!("+{at:#x} a line that is not ASCII"))?;
        let (key, val) = line.split_once(" = ").ok_or_else(|| format!("+{at:#x} line {line:?} is not 'KEY = value '"))?;
        let val = val.strip_suffix(' ').ok_or_else(|| format!("+{at:#x} line {line:?} has no space before CR LF"))?;
        let ok = match key {
            "BOOT2" => val.strip_prefix("cdrom0:\\").is_some_and(|n| n.ends_with(";1") && n.len() > 2 && n.bytes().all(|c| NAME_CHARS.contains(&c))),
            "VER" => val.split_once('.').is_some_and(|(a, c)| !a.is_empty() && !c.is_empty() && a.bytes().chain(c.bytes()).all(|x| x.is_ascii_digit())),
            "VMODE" => val == "NTSC" || val == "PAL",
            _ => return Err(format!("+{at:#x} unknown key {key:?}")),
        };
        if !ok {
            return Err(format!("+{at:#x} {key} value {val:?}"));
        }
        keys.push(key.to_string());
        f.mark(at, end + 2 - at);
        at = end + 2;
    }
    if keys != ["BOOT2", "VER", "VMODE"] {
        return Err(format!("lines {keys:?}, expected BOOT2, VER, VMODE"));
    }
    Ok(())
}
