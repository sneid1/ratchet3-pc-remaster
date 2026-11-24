//! The memory card server (mcserv, server 0x80000400), native: one PS2 card in port 0, kept as plain files in the player's profile
//! (`Iop::memcard`, the profile's `memcard` folder): each directory of the card a folder, each of its files a file, so the game's
//! save folder is `memcard/BASCUS-97353RATCHET` and its four slots `save0.bin` to `save3.bin` there (PLAN 7.1). The folder is read
//! once, when the game first calls the server; a file is written when the game closes it after writing it (to a temporary name
//! first, then renamed over the old one, so a save cut short leaves the last one whole), a directory when the game makes it, and
//! an entry removed when the game deletes it. Port 1 has no card.
//!
//! Requests as libmc (boot_elf 0x12d000..0x12ea00) sends them: a 0x30-byte block (0x15A480) or, for the calls that name a file,
//! a 0x414-byte block (0x15A4B0: +0 port, +4 slot, then the call's words, the name from +0x14); the reply is one word, the
//! result sceMcSync reads (0x15B9C0). What the boot calls:
//! - 0xFE, init (0x12dbd4): the reply is 12 bytes, the result, mcserv's version (at least 0x20A, 0x12dc0c) and mcman's (at least
//!   0x20E);
//! - 1, sceMcGetInfo (0x12e400): +4 port, +8 slot, +0xC / +0x10 / +0x14 whether the format, the free space and the type are
//!   wanted, +0x1C the EE block they go in (type at +0, free clusters at +4, format at +0x90; the end callback 0x12e3a8 copies
//!   them out);
//! - 0xD, sceMcGetDir (0x12e580): +8 the mode (0 first, 1 next), +0xC the most entries, +0x10 the EE table, +0x14 the name
//!   pattern; the result is the count of 0x40-byte entries written (dates, size +0x10, attributes +0x14, name +0x20).
//! What a save calls:
//! - 2, sceMcOpen (0x12dca0): +0 port, +4 slot, +8 the mode (1 read, 2 write, 3 both, 0x200 create, 0x400 truncate), +0x14 the
//!   name; the result a descriptor (the lowest free of `FDS`), -4 no such file (or no parent to create it in), -7 none free.
//!   Mode 0x40 is sceMcMkdir (0x12ddc8): 0, or -4 when it exists or has no parent.
//! - 3, sceMcClose (0x12de00): +0 the descriptor; 0. A file written through the descriptor goes to its plain file then.
//! - 4, sceMcSeek (0x12dec0): +0 the descriptor, +0x10 the offset, +0x14 whence (0 from the start, 1 from here, 2 from the end);
//!   the new position.
//! - 5, sceMcRead (0x12e050): +0 the descriptor, +0xC the size, +0x18 the EE buffer, +0x1C an EE block whose head and tail
//!   bytes the end callback (0x12dfa0) copies into the buffer when their lengths (+0, +4) are not 0; the bytes read. The port
//!   writes them all straight into the buffer and both lengths 0.
//! - 6, sceMcWrite (0x12e168): +0 the descriptor, +0x14 the head's length (the bytes up to the source's next 16-byte boundary,
//!   or all of a write of 16 or fewer) and +0x20 its bytes, +0x18 the rest's EE address and +0xC its length; the bytes written.
//! - 0xF, sceMcDelete (0x12e7a8): +0 port, +4 slot, +0x14 the name; 0, -4 no such entry, -6 a directory not empty.
//! New entries are dated with the console's clock (sceMcStDateTime: second, minute, hour, day, month at +1..+5, the year at +6,
//! Japan time) and carry mcman's modes (0x8497 a file, 0x8427 a directory); entries read from the folder carry the same modes (the
//! game makes no other) and the dates of their plain files. Any other function stops the game with its number.

use crate::{Cpu, Ext};
use std::collections::BTreeMap;

/// The card's size in 1 KB clusters as GetInfo counts the free space of an 8 MB card.
const CLUSTERS: u32 = 8000;
/// Files open at once.
const FDS: usize = 3;
/// The modes mcman gives a new file and a new directory.
const MODE_FILE: u16 = 0x8497;
const MODE_DIR: u16 = 0x8427;
/// A directory's ".." entry.
const MODE_DOTDOT: u16 = 0xa426;

/// An open file: its path and position, and whether it was written (or made or emptied) since it was opened.
struct Fd {
    path: String,
    pos: usize,
    dirty: bool,
}

#[derive(Clone)]
struct Entry {
    dir: bool,
    attr: u16,
    data: Vec<u8>,
    created: [u8; 8],
    modified: [u8; 8],
}

pub struct Mc {
    /// The card's folder (None: a card that lives only as long as the run).
    path: Option<String>,
    /// Absolute paths ("/BASCUS-97353...", "/BASCUS-97353.../file") to entries.
    files: BTreeMap<String, Entry>,
    /// GetInfo's first answer after the card is "inserted" says it is new (-1); after that 0.
    seen: bool,
    /// GetDir's listing in progress: the matching entries' records left.
    listing: Vec<[u8; 0x40]>,
    fds: [Option<Fd>; FDS],
}

impl Mc {
    /// The card held in the folder `path` (none there yet: a new, empty card).
    fn load(path: Option<String>) -> Result<Mc, String> {
        let mut files = BTreeMap::new();
        if let Some(root) = &path {
            let root = std::path::Path::new(root);
            if root.exists() {
                read_dir(root, "", &mut files)?;
            }
        }
        Ok(Mc { path, files, seen: false, listing: Vec::new(), fds: [None, None, None] })
    }

    /// Where the card's entry `path` ("/BASCUS-97353RATCHET/save0.bin") is kept.
    fn host(&self, path: &str) -> Option<std::path::PathBuf> {
        self.path.as_ref().map(|root| std::path::Path::new(root).join(path.trim_start_matches('/')))
    }

    /// The file `path` written to its plain file: a temporary one first, renamed over the old.
    fn write_file(&self, path: &str) -> Result<(), String> {
        let Some(h) = self.host(path) else { return Ok(()) };
        let tmp = h.with_extension("part");
        std::fs::write(&tmp, &self.files[path].data).map_err(|e| format!("{}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, &h).map_err(|e| format!("{}: {e}", h.display()))
    }

    /// The directory `path` made in the card's folder.
    fn make_dir(&self, path: &str) -> Result<(), String> {
        let Some(h) = self.host(path) else { return Ok(()) };
        std::fs::create_dir_all(&h).map_err(|e| format!("{}: {e}", h.display()))
    }

    /// The entry `path` (a file, or an empty directory) taken out of the card's folder.
    fn remove(&self, path: &str, dir: bool) -> Result<(), String> {
        let Some(h) = self.host(path) else { return Ok(()) };
        if dir { std::fs::remove_dir(&h) } else { std::fs::remove_file(&h) }.map_err(|e| format!("{}: {e}", h.display()))
    }

    fn used(&self) -> u32 {
        self.files.values().map(|e| if e.dir { 2 } else { (e.data.len() as u32).div_ceil(1024) + 1 }).sum()
    }
}

/// The folder `dir` read into `files` under the card path `at` ("" for the card's root): folders as directories, files as files.
fn read_dir(dir: &std::path::Path, at: &str, files: &mut BTreeMap<String, Entry>) -> Result<(), String> {
    let list = std::fs::read_dir(dir).map_err(|e| format!("the memory card's folder {}: {e}", dir.display()))?;
    for d in list {
        let d = d.map_err(|e| format!("{}: {e}", dir.display()))?;
        let name = d.file_name().into_string().map_err(|n| format!("{}: {n:?} is not a name the card can hold", dir.display()))?;
        let meta = d.metadata().map_err(|e| format!("{}: {e}", d.path().display()))?;
        let stamp = |t: std::io::Result<std::time::SystemTime>| t.map(date_at).map_err(|e| format!("{}: {e}", d.path().display()));
        let (created, modified) = (stamp(meta.created())?, stamp(meta.modified())?);
        let path = format!("{at}/{name}");
        if meta.is_dir() {
            files.insert(path.clone(), Entry { dir: true, attr: MODE_DIR, data: Vec::new(), created, modified });
            read_dir(&d.path(), &path, files)?;
        } else {
            let data = std::fs::read(d.path()).map_err(|e| format!("{}: {e}", d.path().display()))?;
            files.insert(path, Entry { dir: false, attr: MODE_FILE, data, created, modified });
        }
    }
    Ok(())
}

fn w(b: &[u8], o: usize) -> u32 {
    b.get(o..o + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap())).unwrap_or(0)
}

fn name_at(b: &[u8], o: usize) -> String {
    let s = &b[o.min(b.len())..];
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..n]).into_owned()
}

/// `pattern` (a path whose last part may hold * and ?) against `path`.
fn matches(pattern: &str, path: &str) -> bool {
    let (pd, pn) = pattern.rsplit_once('/').unwrap_or(("", pattern));
    let (fd, fname) = path.rsplit_once('/').unwrap_or(("", path));
    if pd.trim_end_matches('/') != fd.trim_end_matches('/') {
        return false;
    }
    fn glob(p: &[u8], s: &[u8]) -> bool {
        match (p.first(), s.first()) {
            (None, None) => true,
            (Some(b'*'), _) => glob(&p[1..], s) || (!s.is_empty() && glob(p, &s[1..])),
            (Some(b'?'), Some(_)) => glob(&p[1..], &s[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => glob(&p[1..], &s[1..]),
            _ => false,
        }
    }
    glob(pn.as_bytes(), fname.as_bytes())
}

/// A GetDir record (sceMcTblGetDir): created, modified, +0x10 the size, +0x14 the mode, +0x20 the name.
fn record(name: &str, e: &Entry, size: u32) -> [u8; 0x40] {
    let mut ent = [0u8; 0x40];
    ent[0..8].copy_from_slice(&e.created);
    ent[8..16].copy_from_slice(&e.modified);
    ent[0x10..0x14].copy_from_slice(&size.to_le_bytes());
    ent[0x14..0x16].copy_from_slice(&e.attr.to_le_bytes());
    let name = name.as_bytes();
    ent[0x20..0x20 + name.len().min(31)].copy_from_slice(&name[..name.len().min(31)]);
    ent
}

/// A name as an absolute path on the card (the current directory is the root: the game calls no ChDir).
fn abs(name: &str) -> String {
    let p = name.trim_end_matches('/');
    if p.starts_with('/') { p.to_string() } else { format!("/{p}") }
}

/// The directory holding `path`, None for the root's entries.
fn parent(path: &str) -> Option<&str> {
    path.rsplit_once('/').map(|(d, _)| d).filter(|d| !d.is_empty())
}

/// The console's clock as mcman dates an entry (sceMcStDateTime): +1 second, +2 minute, +3 hour, +4 day, +5 month, +6 the year.
fn date() -> [u8; 8] {
    date_at(std::time::SystemTime::now())
}

/// A moment as mcman dates an entry, in the console's Japan time (as `iop::clock` gives the console's clock): a plain file's
/// times read back.
fn date_at(t: std::time::SystemTime) -> [u8; 8] {
    let t = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).expect("a file time after 1970") + 9 * 3600;
    let (days, s) = (t / 86400, t % 86400);
    // Days to a civil date (proleptic Gregorian).
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = (yoe + era * 400 + if m <= 2 { 1 } else { 0 }) as u16;
    [0, (s % 60) as u8, (s / 60 % 60) as u8, (s / 3600) as u8, d as u8, m as u8, y as u8, (y >> 8) as u8]
}

fn reply(rsize: usize, words: &[u32]) -> Vec<u8> {
    let mut r = vec![0u8; rsize];
    for (k, v) in words.iter().enumerate() {
        r[4 * k..4 * k + 4].copy_from_slice(&v.to_le_bytes());
    }
    r
}

pub fn serve(c: &mut Cpu, fno: u32, send: &[u8], rsize: usize) -> Vec<u8> {
    if c.mach().iop.mc.is_none() {
        let path = c.mach().iop.memcard.clone();
        let mc = Mc::load(path).unwrap_or_else(|e| panic!("the memory card: {e}"));
        c.mach().iop.mc = Some(mc);
    }
    match fno {
        0xfe => reply(rsize, &[0, 0x20a, 0x20e]),
        1 => {
            let (port, slot, want_format, want_free, want_type, out) = (w(send, 4), w(send, 8), w(send, 0xc), w(send, 0x10), w(send, 0x14), w(send, 0x1c));
            if port != 0 || slot != 0 {
                // No card: libmc's result for an empty port, type, free and format 0.
                let mut b = crate::iop::ee_read(c, out, 0x94);
                for (want, at) in [(want_type, 0usize), (want_free, 4), (want_format, 0x90)] {
                    if want != 0 {
                        b[at..at + 4].fill(0);
                    }
                }
                crate::iop::ee_write(c, out, &b);
                return reply(rsize, &[(-10i32) as u32]);
            }
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            let r = if mc.seen { 0 } else { (-1i32) as u32 };
            mc.seen = true;
            let free = CLUSTERS - mc.used();
            let mut b = crate::iop::ee_read(c, out, 0x94);
            if want_type != 0 {
                b[0..4].copy_from_slice(&2u32.to_le_bytes());
            }
            if want_free != 0 {
                b[4..8].copy_from_slice(&free.to_le_bytes());
            }
            if want_format != 0 {
                b[0x90..0x94].copy_from_slice(&1u32.to_le_bytes());
            }
            crate::iop::ee_write(c, out, &b);
            reply(rsize, &[r])
        }
        0xd => {
            let (port, mode, max, table, pattern) = (w(send, 0), w(send, 8), w(send, 0xc), w(send, 0x10), name_at(send, 0x14));
            if port != 0 {
                return reply(rsize, &[(-10i32) as u32]);
            }
            let pattern = if pattern.starts_with('/') { pattern } else { format!("/{pattern}") };
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            if mode == 0 {
                // A directory's listing starts with its "." and ".." (size 0), as mcman's does: the game's save check
                // (0x39cbf8) wants its directory's 8 files and these 2.
                let dir = pattern.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
                let mut list = Vec::new();
                if let Some(d) = mc.files.get(dir).filter(|d| d.dir) {
                    for (name, mode) in [(".", MODE_DIR), ("..", MODE_DOTDOT)] {
                        if matches(&pattern, &format!("{dir}/{name}")) {
                            list.push(record(name, &Entry { attr: mode, data: Vec::new(), ..d.clone() }, 0));
                        }
                    }
                }
                for (p, e) in mc.files.iter().filter(|(p, _)| matches(&pattern, p)) {
                    // A directory's size is its entry count, "." and ".." with it.
                    let size = if e.dir { 2 + mc.files.keys().filter(|k| parent(k) == Some(p.as_str())).count() } else { e.data.len() };
                    list.push(record(p.rsplit('/').next().unwrap_or(p), e, size as u32));
                }
                mc.listing = list;
            }
            let n = (max as usize).min(mc.listing.len());
            let take: Vec<[u8; 0x40]> = mc.listing.drain(..n).collect();
            for (k, ent) in take.iter().enumerate() {
                crate::iop::ee_write(c, table + 0x40 * k as u32, ent);
            }
            reply(rsize, &[n as u32])
        }
        // Open, and Mkdir (mode 0x40).
        2 => {
            let (port, mode, name) = (w(send, 0), w(send, 8), name_at(send, 0x14));
            if port != 0 {
                return reply(rsize, &[(-10i32) as u32]);
            }
            let path = abs(&name);
            let now = date();
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            let parent_ok = parent(&path).is_none_or(|p| mc.files.get(p).is_some_and(|e| e.dir));
            if mode == 0x40 {
                if mc.files.contains_key(&path) || !parent_ok {
                    return reply(rsize, &[(-4i32) as u32]);
                }
                mc.make_dir(&path).unwrap_or_else(|e| panic!("the memory card: {e}"));
                mc.files.insert(path, Entry { dir: true, attr: MODE_DIR, data: Vec::new(), created: now, modified: now });
                return reply(rsize, &[0]);
            }
            if mode & !0x603 != 0 {
                panic!("memory card Open of {path:?} with mode {mode:#x}: not one libmc's callers use");
            }
            let dirty = match mc.files.get_mut(&path) {
                Some(e) if e.dir => panic!("memory card Open of {path:?}: a directory"),
                Some(e) => {
                    if mode & 0x400 != 0 {
                        e.data.clear();
                        e.modified = now;
                    }
                    mode & 0x400 != 0
                }
                None if mode & 0x200 != 0 && parent_ok => {
                    mc.files.insert(path.clone(), Entry { dir: false, attr: MODE_FILE, data: Vec::new(), created: now, modified: now });
                    true
                }
                None => return reply(rsize, &[(-4i32) as u32]),
            };
            let Some(fd) = mc.fds.iter().position(|f| f.is_none()) else { return reply(rsize, &[(-7i32) as u32]) };
            mc.fds[fd] = Some(Fd { path, pos: 0, dirty });
            reply(rsize, &[fd as u32])
        }
        // Close.
        3 => {
            let fd = w(send, 0) as usize;
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            let Some(f) = mc.fds.get_mut(fd).and_then(|f| f.take()) else { panic!("memory card Close of descriptor {fd}, which is not open") };
            if f.dirty {
                mc.write_file(&f.path).unwrap_or_else(|e| panic!("the memory card: {e}"));
            }
            reply(rsize, &[0])
        }
        // Seek.
        4 => {
            let (fd, off, whence) = (w(send, 0) as usize, w(send, 0x10) as i32 as i64, w(send, 0x14));
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            let f = mc.fds.get(fd).and_then(|f| f.as_ref()).unwrap_or_else(|| panic!("memory card Seek on descriptor {fd}, which is not open"));
            let len = mc.files[&f.path].data.len() as i64;
            let base = match whence {
                0 => 0,
                1 => f.pos as i64,
                2 => len,
                _ => panic!("memory card Seek with whence {whence}"),
            };
            let pos = (base + off).clamp(0, len) as usize;
            mc.fds[fd].as_mut().expect("checked").pos = pos;
            reply(rsize, &[pos as u32])
        }
        // Read: the bytes straight into the buffer, the callback's head and tail lengths 0.
        5 => {
            let (fd, size, buf, rdata) = (w(send, 0) as usize, w(send, 0xc) as usize, w(send, 0x18), w(send, 0x1c));
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            let f = mc.fds.get_mut(fd).and_then(|f| f.as_mut()).unwrap_or_else(|| panic!("memory card Read on descriptor {fd}, which is not open"));
            let data = &mc.files[&f.path].data;
            let n = size.min(data.len().saturating_sub(f.pos));
            let bytes = data[f.pos..f.pos + n].to_vec();
            f.pos += n;
            crate::iop::ee_write(c, buf, &bytes);
            crate::iop::ee_write(c, rdata, &[0u8; 8]);
            reply(rsize, &[n as u32])
        }
        // Write: the head from the request, the rest from EE memory.
        6 => {
            let (fd, head, rest_at, rest) = (w(send, 0) as usize, w(send, 0x14) as usize, w(send, 0x18), w(send, 0xc) as usize);
            if head > 16 {
                panic!("memory card Write with a head of {head} bytes");
            }
            let mut bytes = send[0x20..0x20 + head].to_vec();
            bytes.extend(crate::iop::ee_read(c, rest_at, rest));
            let now = date();
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            let f = mc.fds.get_mut(fd).and_then(|f| f.as_mut()).unwrap_or_else(|| panic!("memory card Write on descriptor {fd}, which is not open"));
            let e = mc.files.get_mut(&f.path).expect("an open file's entry");
            if e.data.len() < f.pos + bytes.len() {
                e.data.resize(f.pos + bytes.len(), 0);
            }
            e.data[f.pos..f.pos + bytes.len()].copy_from_slice(&bytes);
            e.modified = now;
            f.pos += bytes.len();
            f.dirty = true;
            reply(rsize, &[bytes.len() as u32])
        }
        // Delete.
        0xf => {
            let (port, name) = (w(send, 0), name_at(send, 0x14));
            if port != 0 {
                return reply(rsize, &[(-10i32) as u32]);
            }
            let path = abs(&name);
            let mc = c.mach().iop.mc.as_mut().expect("loaded above");
            let r = match mc.files.get(&path) {
                None => -4,
                Some(e) if e.dir && mc.files.keys().any(|k| parent(k) == Some(path.as_str())) => -6,
                Some(e) => {
                    let dir = e.dir;
                    mc.remove(&path, dir).unwrap_or_else(|e| panic!("the memory card: {e}"));
                    mc.files.remove(&path);
                    0
                }
            };
            reply(rsize, &[r as u32])
        }
        _ => panic!("memory card function {fno:#x} ({} bytes sent): its request is not read from libmc yet", send.len()),
    }
}
