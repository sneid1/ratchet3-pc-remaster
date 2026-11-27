//! The game's RAM and the renderer's copies of it.
//!
//! The RAM is committed with the system's write watch (`MEM_WRITE_WATCH`): the system notes each 4 KB page written, by the
//! translated code and the runtime alike, from any thread, and `GetWriteWatch` hands the list over. A copy (`RamCopy`) knows the
//! copy generation it was last brought up to date at, so bringing it up to date again (`Ram::copy_to`) copies only the pages
//! written since then. A whole copy of the 32 MB was 1.7 ms of every field on Veldin (each frame is drawn with the RAM as it stood
//! at its first kick, `hw::kick`); a field writes a few of the 32 MB.

use std::ffi::c_void;
use std::ops::{Deref, DerefMut};
use std::sync::atomic::{AtomicU64, Ordering};

const PAGE: usize = 4096;
const MEM_COMMIT: u32 = 0x1000;
const MEM_RESERVE: u32 = 0x2000;
const MEM_RELEASE: u32 = 0x8000;
const MEM_WRITE_WATCH: u32 = 0x20_0000;
const PAGE_READWRITE: u32 = 4;
const WRITE_WATCH_FLAG_RESET: u32 = 1;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn VirtualAlloc(at: *mut c_void, size: usize, kind: u32, protect: u32) -> *mut c_void;
    fn VirtualFree(at: *mut c_void, size: usize, kind: u32) -> i32;
    fn GetWriteWatch(flags: u32, base: *mut c_void, size: usize, addresses: *mut *mut c_void, count: *mut usize, granularity: *mut u32) -> u32;
    fn GetLastError() -> u32;
}

/// Which RAM a copy was made from (`RamCopy::owner`): every `Ram` has its own.
static RAMS: AtomicU64 = AtomicU64::new(1);

/// The game's RAM, zeroed at the start and watched for writes.
pub struct Ram {
    ptr: *mut u8,
    len: usize,
    /// The bytes committed (`len` up to whole pages).
    size: usize,
    id: u64,
    /// The copy generation: one more at each `copy_to`.
    generation: u64,
    /// Per page, the last generation whose `GetWriteWatch` listed it as written.
    written: Vec<u64>,
    /// `GetWriteWatch`'s list, room for every page.
    found: Vec<*mut c_void>,
    /// The bytes copied by `copy_to` since the last `take_copied`.
    copied: u64,
    /// The story level whose program's code is in RAM (`Machine::flush_cache`), None while none is; each copy takes it
    /// (`RamCopy::level`).
    pub(crate) level: Option<u32>,
}

// The RAM is owned as a Vec's bytes are: one owner, shared only by reference.
unsafe impl Send for Ram {}
unsafe impl Sync for Ram {}

impl Ram {
    /// `len` bytes of zeroed RAM, watched for writes.
    pub fn new(len: usize) -> Result<Ram, String> {
        let size = len.next_multiple_of(PAGE);
        let ptr = unsafe { VirtualAlloc(std::ptr::null_mut(), size, MEM_RESERVE | MEM_COMMIT | MEM_WRITE_WATCH, PAGE_READWRITE) };
        if ptr.is_null() {
            return Err(format!("the game's RAM: VirtualAlloc of {size} bytes with write watch failed (error {})", unsafe { GetLastError() }));
        }
        let pages = size / PAGE;
        Ok(Ram { ptr: ptr.cast(), len, size, id: RAMS.fetch_add(1, Ordering::Relaxed), generation: 0, written: vec![0; pages], found: vec![std::ptr::null_mut(); pages], copied: 0, level: None })
    }

    /// Brings `to` up to date with the RAM's first `to.len()` bytes: the pages written since it was last brought up to date (all
    /// of them the first time, or when it was made from another RAM).
    pub fn copy_to(&mut self, to: &mut RamCopy) -> Result<(), String> {
        self.generation += 1;
        let (mut count, mut granularity) = (self.found.len(), 0u32);
        let r = unsafe { GetWriteWatch(WRITE_WATCH_FLAG_RESET, self.ptr.cast(), self.size, self.found.as_mut_ptr(), &mut count, &mut granularity) };
        if r != 0 {
            return Err(format!("the game's RAM: GetWriteWatch failed (error {})", unsafe { GetLastError() }));
        }
        if granularity as usize != PAGE {
            return Err(format!("the game's RAM: GetWriteWatch's pages are {granularity} bytes, not {PAGE}"));
        }
        for &a in &self.found[..count] {
            self.written[(a as usize - self.ptr as usize) / PAGE] = self.generation;
        }
        let n = to.bytes.len();
        if to.owner != self.id {
            to.bytes.copy_from_slice(&self[..n]);
            self.copied += n as u64;
        } else {
            // The runs of pages written after the copy's generation.
            let (pages, since) = (n.div_ceil(PAGE), to.generation);
            let mut p = 0;
            while p < pages {
                if self.written[p] <= since {
                    p += 1;
                    continue;
                }
                let s = p;
                while p < pages && self.written[p] > since {
                    p += 1;
                }
                let (a, b) = (s * PAGE, (p * PAGE).min(n));
                to.bytes[a..b].copy_from_slice(&self[a..b]);
                self.copied += (b - a) as u64;
            }
        }
        (to.owner, to.generation, to.level) = (self.id, self.generation, self.level);
        Ok(())
    }

    /// The bytes `copy_to` copied since the last call.
    pub fn take_copied(&mut self) -> u64 {
        std::mem::take(&mut self.copied)
    }
}

impl Drop for Ram {
    fn drop(&mut self) {
        unsafe { VirtualFree(self.ptr.cast(), 0, MEM_RELEASE) };
    }
}

impl Deref for Ram {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        unsafe { std::slice::from_raw_parts(self.ptr, self.len) }
    }
}

impl DerefMut for Ram {
    fn deref_mut(&mut self) -> &mut [u8] {
        unsafe { std::slice::from_raw_parts_mut(self.ptr, self.len) }
    }
}

/// A copy of the RAM's first `RAM_SIZE` bytes for the renderer, made and brought up to date only by `Ram::copy_to` (it is read
/// only: a byte changed in it would stay wrong in every later copy, which takes only the pages the game wrote).
///
/// A copy made at a frame's kick (`Machine::frame_ram`) also holds what the renderer reads the frame's VIF1 chains with
/// (`hw::Chain`): their blocks, and the scratchpad as it stood with the RAM (taken when a chain first reads it; empty until
/// then). Both go round with the copy, from the game to the renderer and back, kept from frame to frame.
pub struct RamCopy {
    bytes: Vec<u8>,
    /// The RAM it was last brought up to date from (0: none yet) and the generation it was then.
    owner: u64,
    generation: u64,
    pub(crate) chains: Vec<crate::hw::Block>,
    pub(crate) spr: Vec<u8>,
    /// The story level whose code was in RAM when the copy was brought up to date (`Ram::level`).
    level: Option<u32>,
}

impl RamCopy {
    /// The bytes' address and length: valid, and the bytes unchanged, until the next `Ram::copy_to` into this copy (the vector is
    /// never resized): what the worker thread's walk of a kick's GS events reads (`hw::gsstate`), the game thread joining it
    /// before the copy.
    pub(crate) fn raw(&self) -> (*const u8, usize) {
        (self.bytes.as_ptr(), self.bytes.len())
    }

    pub fn new() -> RamCopy {
        RamCopy { bytes: vec![0; crate::RAM_SIZE as usize], owner: 0, generation: 0, chains: Vec::new(), spr: Vec::new(), level: None }
    }

    /// The RAM it was last brought up to date from and the generation it was then: which state of which RAM it holds.
    pub(crate) fn key(&self) -> (u64, u64) {
        (self.owner, self.generation)
    }

    /// The story level whose code was in RAM when the copy was brought up to date (None: no level's, the front end's or the
    /// boot's alone): the level whose memory the copy holds. During a level change the level asked for is `level_current`
    /// while the level left still runs, its memory where the new level's .bss will be; the new level's .bss is the new level's
    /// only once its own code runs (its start's first call clears it: level 6's func_003D4648, 0x1DE580 to 0x2A3730).
    pub fn level(&self) -> Option<u32> {
        self.level
    }
}

impl Default for RamCopy {
    fn default() -> RamCopy {
        RamCopy::new()
    }
}

impl Deref for RamCopy {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes
    }
}
