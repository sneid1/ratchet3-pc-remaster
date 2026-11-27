//! UYA_HEAP_LOG=SECONDS: where the game's Rust heap is, by the code that allocated it (an instrument, `uya` only: the binary's
//! global allocator, `main.rs`).
//!
//! `Tracker` wraps std's `System` allocator. While the variable is set (read once by `start`, before the game starts) it keeps the
//! live bytes and live count of the allocations by owner:
//!   - every allocation of 4 KB or more is recorded exactly, with the stack that made it;
//!   - a smaller one is recorded when its address's hash falls in 1 of 64 (the address, not a counter, so that the free finds out
//!     from the pointer alone whether it was recorded, without a lookup for the other 63 in 64); its bytes and its count are scaled by 64.
//!     The sample is of live blocks, picked by where the allocator put them, which does not depend on what they are for.
//!   - the owner is the first frame of the stack in the port's own crates (uya, uyart, uyafmt, uycpu), past the allocator, alloc,
//!     core, std and hashbrown frames (a Vec, String or HashMap that grows is its owner's: a grown block keeps the owner of the
//!     block it came from); an allocation with none of the port's own frames in its first 32 is the first frame that is not std's
//!     (wgpu's, a driver's), shown as such. The stack is captured with `RtlCaptureStackBackTrace`; the names are looked up when a
//!     report is written, not when the block is made.
//! The tables are made from `System` directly (never from this allocator), a thread-local flag keeps a nested allocation of the
//! tracker's own or the reporter's from being recorded, and a spin lock covers the tables (nothing under it allocates). Off, an
//! allocation costs one relaxed atomic load and nothing else.
//!
//! Every SECONDS seconds, and at the end of `main` (`finish`), the totals and the 40 largest owners by live bytes are printed on
//! lines that start "heap:", the owners named from the executable's PDB (dbghelp: function, file:line, and the next function of
//! the port's own up the stack as "<-"). The PDB's symbols are loaded by the first report (the process's private memory grows by
//! what dbghelp holds from then on).

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::{Cell, UnsafeCell};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering::*};
use std::sync::{Mutex, OnceLock};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::Diagnostics::Debug::{
    IMAGEHLP_LINE64, RtlCaptureStackBackTrace, SYMBOL_INFO, SYMOPT_DEFERRED_LOADS, SYMOPT_LOAD_LINES, SYMOPT_UNDNAME, SymFromAddr, SymGetLineFromAddr64, SymInitialize, SymSetOptions,
};
use windows::Win32::System::Memory::{GetProcessHeaps, HEAP_SUMMARY, HeapSummary};
use windows::Win32::System::Threading::GetCurrentProcess;

/// The binary's global allocator: std's, with the bookkeeping above while UYA_HEAP_LOG is set.
pub struct Tracker;

/// From this size an allocation is recorded exactly; below it, one in `1 << SAMPLE_BITS` by address.
const EXACT: usize = 4096;
const SAMPLE_BITS: u32 = 6;
const WEIGHT: u64 = 1 << SAMPLE_BITS;
/// Frames kept of a stack.
const DEPTH: usize = 32;
/// Distinct stacks kept (the last bucket, 0, takes what comes after: it is printed when it holds anything).
const OWNERS: usize = 1 << 15;

static ON: AtomicBool = AtomicBool::new(false);
static LOCK: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Set while this thread is inside the tracker's or the reporter's own code: its allocations are not recorded.
    static IN: Cell<bool> = const { Cell::new(false) };
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Entry {
    ptr: usize,
    size: usize,
    owner: u32,
    weight: u32,
}

#[derive(Clone, Copy)]
#[repr(C)]
struct Owner {
    hash: u64,
    n: u32,
    bytes: i64,
    count: i64,
    frames: [u64; DEPTH],
}

struct State {
    ptrs: *mut Entry,
    cap: usize,
    len: usize,
    owners: *mut Owner,
    owners_len: usize,
    slots: *mut u32,
    exact_bytes: i64,
    exact_count: i64,
    sampled_bytes: i64,
    sampled_count: i64,
}

struct Shared(UnsafeCell<State>);
// SAFETY: every access is under `LOCK`.
unsafe impl Sync for Shared {}

static STATE: Shared = Shared(UnsafeCell::new(State {
    ptrs: std::ptr::null_mut(),
    cap: 0,
    len: 0,
    owners: std::ptr::null_mut(),
    owners_len: 0,
    slots: std::ptr::null_mut(),
    exact_bytes: 0,
    exact_count: 0,
    sampled_bytes: 0,
    sampled_count: 0,
}));

fn lock() {
    let mut spins = 0u32;
    while LOCK.compare_exchange_weak(false, true, Acquire, Relaxed).is_err() {
        spins += 1;
        if spins < 200 {
            std::hint::spin_loop();
        } else {
            std::thread::yield_now();
        }
    }
}

fn unlock() {
    LOCK.store(false, Release);
}

fn mix(p: usize) -> u64 {
    ((p >> 4) as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// Whether the block at `p` of `size` bytes is one the tracker records (the same answer when it is made, grown and freed).
fn recorded(p: *mut u8, size: usize) -> bool {
    size >= EXACT || mix(p as usize) >> (64 - SAMPLE_BITS) == 0
}

/// System memory for a table, zeroed.
fn table<T>(n: usize) -> *mut T {
    let layout = Layout::array::<T>(n).expect("heaplog: a table's size");
    // SAFETY: a non-empty layout; the pointer is checked.
    let p = unsafe { System.alloc_zeroed(layout) } as *mut T;
    assert!(!p.is_null(), "heaplog: no memory for its tables");
    p
}

impl State {
    fn home(&self, p: usize) -> usize {
        (mix(p) >> 20) as usize & (self.cap - 1)
    }

    /// Records a block; one already recorded at the address (its free went unseen) is replaced.
    unsafe fn insert(&mut self, e: Entry) {
        unsafe {
            if (self.len + 1) * 2 > self.cap {
                self.grow();
            }
            let mut i = self.home(e.ptr);
            loop {
                let s = &mut *self.ptrs.add(i);
                if s.ptr == e.ptr {
                    self.account(s.owner, s.size as i64 * s.weight as i64, -(s.weight as i64), s.weight == 1);
                    *s = e;
                    break;
                }
                if s.ptr == 0 {
                    *s = e;
                    self.len += 1;
                    break;
                }
                i = (i + 1) & (self.cap - 1);
            }
            self.account(e.owner, e.size as i64 * e.weight as i64, e.weight as i64, e.weight == 1);
        }
    }

    unsafe fn grow(&mut self) {
        unsafe {
            let (old, old_cap) = (self.ptrs, self.cap);
            self.cap = if old_cap == 0 { 1 << 16 } else { old_cap * 2 };
            self.ptrs = table::<Entry>(self.cap);
            for i in 0..old_cap {
                let e = *old.add(i);
                if e.ptr != 0 {
                    let mut j = self.home(e.ptr);
                    while (*self.ptrs.add(j)).ptr != 0 {
                        j = (j + 1) & (self.cap - 1);
                    }
                    *self.ptrs.add(j) = e;
                }
            }
            if !old.is_null() {
                System.dealloc(old as *mut u8, Layout::array::<Entry>(old_cap).expect("heaplog: a table's size"));
            }
        }
    }

    fn account(&mut self, owner: u32, bytes: i64, count: i64, exact: bool) {
        // SAFETY: `owner` came from `find_owner`, below `owners_len`.
        let o = unsafe { &mut *self.owners.add(owner as usize) };
        o.bytes += bytes;
        o.count += count;
        if exact {
            self.exact_bytes += bytes;
            self.exact_count += count;
        } else {
            self.sampled_bytes += bytes;
            self.sampled_count += count;
        }
    }

    /// Takes the record of the block at `p` out (backward-shift deletion), its owner's and the totals' bytes with it.
    unsafe fn remove(&mut self, p: usize) -> Option<Entry> {
        unsafe {
            if self.cap == 0 {
                return None;
            }
            let mask = self.cap - 1;
            let mut i = self.home(p);
            loop {
                let s = *self.ptrs.add(i);
                if s.ptr == 0 {
                    return None;
                }
                if s.ptr == p {
                    break;
                }
                i = (i + 1) & mask;
            }
            let found = *self.ptrs.add(i);
            let mut j = i;
            loop {
                j = (j + 1) & mask;
                let s = *self.ptrs.add(j);
                if s.ptr == 0 {
                    break;
                }
                let k = self.home(s.ptr);
                let between = if i <= j { i < k && k <= j } else { i < k || k <= j };
                if !between {
                    *self.ptrs.add(i) = s;
                    i = j;
                }
            }
            (*self.ptrs.add(i)).ptr = 0;
            self.len -= 1;
            self.account(found.owner, -(found.size as i64 * found.weight as i64), -(found.weight as i64), found.weight == 1);
            Some(found)
        }
    }

    /// The owner (distinct stack) with these frames; 0, the overflow bucket, when the table of stacks is full.
    unsafe fn find_owner(&mut self, hash: u64, frames: &[u64]) -> u32 {
        unsafe {
            let mask = OWNERS * 2 - 1;
            let mut i = hash as usize & mask;
            loop {
                let v = *self.slots.add(i);
                if v == 0 {
                    if self.owners_len >= OWNERS {
                        return 0;
                    }
                    let id = self.owners_len;
                    self.owners_len += 1;
                    let o = &mut *self.owners.add(id);
                    o.hash = hash;
                    o.n = frames.len() as u32;
                    o.frames[..frames.len()].copy_from_slice(frames);
                    *self.slots.add(i) = id as u32;
                    return id as u32;
                }
                let o = &*self.owners.add(v as usize);
                if o.hash == hash && o.frames[..o.n as usize] == *frames {
                    return v;
                }
                i = (i + 1) & mask;
            }
        }
    }
}

/// The stack here, innermost first (its first frames are the tracker's own), and its hash.
fn capture(out: &mut [u64; DEPTH]) -> (usize, u64) {
    let mut raw = [std::ptr::null_mut::<core::ffi::c_void>(); DEPTH];
    // SAFETY: fills `raw`; takes no locks of this program's.
    let n = unsafe { RtlCaptureStackBackTrace(2, &mut raw, None) } as usize;
    let mut h = 0xcbf2_9ce4_8422_2325u64;
    for i in 0..n {
        out[i] = raw[i] as u64;
        h = (h ^ out[i]).wrapping_mul(0x0000_0100_0000_01b3);
    }
    (n, h | 1)
}

/// Records a new block of `size` bytes at `p`, with `owner` when it is a grown block (its old owner's), else the owner of the stack here.
fn record(p: *mut u8, size: usize, owner: Option<u32>) {
    let weight = if size >= EXACT { 1 } else { WEIGHT as u32 };
    let owner = owner.unwrap_or_else(|| {
        let mut frames = [0u64; DEPTH];
        let (n, h) = capture(&mut frames);
        lock();
        // SAFETY: under the lock.
        let id = unsafe { (*STATE.0.get()).find_owner(h, &frames[..n]) };
        unlock();
        id
    });
    lock();
    // SAFETY: under the lock.
    unsafe { (*STATE.0.get()).insert(Entry { ptr: p as usize, size, owner, weight }) };
    unlock();
}

fn forget(p: *mut u8) -> Option<Entry> {
    lock();
    // SAFETY: under the lock.
    let e = unsafe { (*STATE.0.get()).remove(p as usize) };
    unlock();
    e
}

/// Runs `f` with this thread's allocations unrecorded; false (and `f` not run) when it already is.
fn guarded(f: impl FnOnce()) -> bool {
    if IN.get() {
        return false;
    }
    IN.set(true);
    f();
    IN.set(false);
    true
}

// SAFETY: every method does std's `System` allocator's work and, beside it, the bookkeeping, which touches no block of the caller.
unsafe impl GlobalAlloc for Tracker {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if ON.load(Relaxed) && !p.is_null() && recorded(p, l.size()) {
            guarded(|| record(p, l.size(), None));
        }
        p
    }

    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if ON.load(Relaxed) && !p.is_null() && recorded(p, l.size()) {
            guarded(|| record(p, l.size(), None));
        }
        p
    }

    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        if ON.load(Relaxed) && recorded(p, l.size()) && !IN.get() {
            forget(p);
        }
        unsafe { System.dealloc(p, l) };
    }

    unsafe fn realloc(&self, p: *mut u8, l: Layout, new_size: usize) -> *mut u8 {
        if !ON.load(Relaxed) {
            return unsafe { System.realloc(p, l, new_size) };
        }
        // The old block's record goes before the block can be freed and its address reused by another thread.
        let old = if recorded(p, l.size()) && !IN.get() { forget(p) } else { None };
        let q = unsafe { System.realloc(p, l, new_size) };
        if q.is_null() {
            if let Some(e) = old {
                guarded(|| record(p, e.size, Some(e.owner)));
            }
        } else if let Some(e) = old {
            if recorded(q, new_size) {
                guarded(|| record(q, new_size, Some(e.owner)));
            }
        } else if recorded(q, new_size) {
            guarded(|| record(q, new_size, None));
        }
        q
    }
}

/// Reads UYA_HEAP_LOG once; set, turns the tracker on and starts the reporter. Call it first in `main`.
pub fn start() {
    let Ok(v) = std::env::var("UYA_HEAP_LOG") else { return };
    let secs: f64 = v.trim().parse().expect("UYA_HEAP_LOG=SECONDS (a number)");
    assert!(secs > 0.0, "UYA_HEAP_LOG=SECONDS (more than 0)");
    // SAFETY: before any other thread of the program is started; the tables are System memory.
    unsafe {
        let s = &mut *STATE.0.get();
        s.owners = table::<Owner>(OWNERS);
        s.owners_len = 1;
        s.slots = table::<u32>(OWNERS * 2);
        s.grow();
    }
    START.set(std::time::Instant::now()).expect("heaplog: started once");
    ON.store(true, SeqCst);
    std::thread::Builder::new()
        .name("heaplog".into())
        .spawn(move || {
            IN.set(true);
            loop {
                std::thread::sleep(std::time::Duration::from_secs_f64(secs));
                report("");
            }
        })
        .expect("the heap log's thread");
}

static START: OnceLock<std::time::Instant> = OnceLock::new();

/// The last report, when the program ends (`main`); nothing when UYA_HEAP_LOG is not set.
pub fn finish() {
    if ON.load(Relaxed) {
        let was = IN.replace(true);
        report(" (end)");
        IN.set(was);
    }
}

/// dbghelp for this process, started once, looking for the PDB beside the executable and in its `deps` folder.
fn dbghelp() -> HANDLE {
    static STARTED: OnceLock<()> = OnceLock::new();
    // SAFETY: dbghelp for this process, the PDBs beside the modules.
    let process = unsafe { GetCurrentProcess() };
    STARTED.get_or_init(|| {
        let exe = std::env::current_exe().expect("heaplog: the executable");
        let dir = exe.parent().expect("heaplog: the executable's folder").to_string_lossy().into_owned();
        let path = std::ffi::CString::new(format!("{dir};{dir}\\deps")).expect("heaplog: the symbol path");
        unsafe {
            SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES);
            SymInitialize(process, windows::core::PCSTR(path.as_ptr() as *const u8), true).expect("heaplog: dbghelp");
        }
    });
    process
}

/// The function name at a return address (one byte before it, inside the call), without the legacy mangling's `::h<hash>`.
fn function(process: HANDLE, a: u64) -> Option<String> {
    let mut buf = vec![0u64; (std::mem::size_of::<SYMBOL_INFO>() + 1024) / 8 + 1];
    let info = buf.as_mut_ptr() as *mut SYMBOL_INFO;
    // SAFETY: `buf` is large enough for SYMBOL_INFO and 1024 name bytes, and aligned for it.
    unsafe {
        (*info).SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
        (*info).MaxNameLen = 1024;
        let mut disp = 0u64;
        SymFromAddr(process, a - 1, Some(&mut disp), info).ok()?;
        let bytes = std::slice::from_raw_parts((*info).Name.as_ptr() as *const u8, (*info).NameLen as usize);
        let s: String = String::from_utf8_lossy(bytes).chars().take(150).collect();
        let b = s.as_bytes();
        let n = b.len();
        Some(if n > 19 && &b[n - 19..n - 16] == b"::h" && b[n - 16..].iter().all(u8::is_ascii_hexdigit) { s[..n - 19].to_string() } else { s })
    }
}

fn source_line(process: HANDLE, a: u64) -> Option<String> {
    let mut line = IMAGEHLP_LINE64 { SizeOfStruct: std::mem::size_of::<IMAGEHLP_LINE64>() as u32, ..Default::default() };
    let mut disp = 0u32;
    // SAFETY: dbghelp fills `line`, whose file name it owns.
    unsafe {
        SymGetLineFromAddr64(process, a - 1, &mut disp, &mut line).ok()?;
        let file = line.FileName.to_string().unwrap_or_default();
        Some(format!("{}:{}", file.rsplit(['\\', '/']).next().unwrap_or(""), line.LineNumber))
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Port,
    Std,
    Other,
}

struct Named {
    name: String,
    line: Option<String>,
    kind: Kind,
}

fn name_frame(process: HANDLE, a: u64) -> Named {
    let Some(name) = function(process, a) else {
        return Named { name: format!("{a:#x}"), line: None, kind: Kind::Other };
    };
    let bare = name.trim_start_matches('<');
    let port = !bare.contains("::__rust_") && ["uya::", "uyart::", "uyafmt::", "uycpu::"].iter().any(|c| bare.starts_with(c)) && !bare.starts_with("uya::heaplog::");
    // `uya::_::__rust_realloc` and the like are the global allocator's shims (`main.rs`), not an owner.
    let std = bare.starts_with("uya::heaplog::")
        || bare.contains("::__rust_")
        || bare.contains("::__rdl_")
        || ["core::", "alloc::", "std::", "hashbrown::", "__rust", "__rdl", "_rust", "rust_", "RtlCapture", "main", "BaseThreadInitThunk", "RtlUserThreadStart"].iter().any(|c| bare.starts_with(c));
    let line = if port { source_line(process, a) } else { None };
    Named { name, line, kind: if port { Kind::Port } else if std { Kind::Std } else { Kind::Other } }
}

static START_DBGHELP: OnceLock<()> = OnceLock::new();
static REPORT: Mutex<()> = Mutex::new(());

/// The process's heaps, allocated and committed MB each (the Rust heap's blocks are in the first, with every C library's).
fn heaps() -> Vec<(f64, f64)> {
    let mut heaps = [HANDLE::default(); 64];
    let mut v = Vec::new();
    // SAFETY: fills `heaps`; HeapSummary walks each heap under its own lock (this thread holds none of the program's).
    unsafe {
        let n = (GetProcessHeaps(&mut heaps) as usize).min(heaps.len());
        for h in &heaps[..n] {
            let mut hs = HEAP_SUMMARY { cb: std::mem::size_of::<HEAP_SUMMARY>() as u32, ..Default::default() };
            if HeapSummary(*h, 0, &mut hs).as_bool() && hs.cbCommitted > 0 {
                v.push((hs.cbAllocated as f64 / (1 << 20) as f64, hs.cbCommitted as f64 / (1 << 20) as f64));
            }
        }
    }
    v
}

/// What dbghelp's symbols and lines hold in the first heap once the first report has loaded them (MB, f64 bits), taken out of later reports' heap figures.
static DBGHELP_MB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Prints the report: the totals and the 40 largest owners by live bytes.
fn report(label: &str) {
    let _one = REPORT.lock().expect("heaplog: the report lock");
    // The owners are copied out under the lock, into System memory (nothing under the lock may allocate from this allocator).
    static SNAP: OnceLock<usize> = OnceLock::new();
    let snap = *SNAP.get_or_init(|| table::<Owner>(OWNERS) as usize) as *mut Owner;
    let (n, totals) = {
        lock();
        // SAFETY: under the lock; `snap` holds OWNERS owners.
        let r = unsafe {
            let s = &*STATE.0.get();
            std::ptr::copy_nonoverlapping(s.owners, snap, s.owners_len);
            (s.owners_len, (s.exact_bytes, s.exact_count, s.sampled_bytes, s.sampled_count))
        };
        unlock();
        r
    };
    // SAFETY: `n` owners were copied to `snap`.
    let owners = unsafe { std::slice::from_raw_parts(snap, n) };
    let (private_before, heaps_before) = (crate::native::private_mb(), heaps());
    let first = START_DBGHELP.set(()).is_ok();
    let process = dbghelp();
    static CACHE: OnceLock<Mutex<HashMap<u64, std::sync::Arc<Named>>>> = OnceLock::new();
    let mut cache = CACHE.get_or_init(Default::default).lock().expect("heaplog: the name cache");
    let mut by_owner: HashMap<String, (i64, i64)> = HashMap::new();
    let mb = |b: i64| b as f64 / (1 << 20) as f64;
    for (id, o) in owners.iter().enumerate() {
        if o.bytes == 0 && o.count == 0 {
            continue;
        }
        let key = if id == 0 {
            "(more stacks than the table keeps)".to_string()
        } else {
            let frames: Vec<std::sync::Arc<Named>> = o.frames[..o.n as usize].iter().map(|&a| cache.entry(a).or_insert_with(|| std::sync::Arc::new(name_frame(process, a))).clone()).collect();
            let first = frames.iter().find(|f| f.kind == Kind::Port).or_else(|| frames.iter().find(|f| f.kind == Kind::Other));
            match first {
                Some(f) => {
                    let caller = frames.iter().skip_while(|g| !std::ptr::eq(&***g, &**f)).skip(1).find(|g| g.kind == Kind::Port && g.name != f.name);
                    let tag = if f.kind == Kind::Port { "" } else { "[not the port's] " };
                    format!("{tag}{} {}{}", f.name, f.line.as_deref().unwrap_or(""), caller.map(|c| format!(" <- {}", c.name)).unwrap_or_default())
                }
                None => "(no frame past the allocator)".to_string(),
            }
        };
        let e = by_owner.entry(key).or_default();
        e.0 += o.bytes;
        e.1 += o.count;
    }
    let (eb, ec, sb, sc) = totals;
    let total = eb + sb;
    let mut v: Vec<(String, (i64, i64))> = by_owner.into_iter().collect();
    v.sort_by(|a, b| b.1.0.cmp(&a.1.0).then_with(|| a.0.cmp(&b.0)));
    let mut out = format!(
        "heap: t={:.0} s{label}: {:.1} MB tracked live in {} allocations ({:.1} MB in {} of 4 KB or more, exact; {:.1} MB in {} smaller, sampled 1 in {} by address and scaled, {:.1}% of the bytes); {} distinct stacks",
        START.get().expect("heaplog: started").elapsed().as_secs_f64(),
        mb(total),
        ec + sc,
        mb(eb),
        ec,
        mb(sb),
        sc,
        WEIGHT,
        if total > 0 { sb as f64 * 100.0 / total as f64 } else { 0.0 },
        n - 1
    );
    let mut shown = 0i64;
    for (i, (k, (b, c))) in v.iter().take(40).enumerate() {
        shown += b;
        out.push_str(&format!("\nheap: {:2}. {:8.1} MB {:5.1}% {:9} allocations  {k}", i + 1, mb(*b), if total > 0 { *b as f64 * 100.0 / total as f64 } else { 0.0 }, c));
    }
    out.push_str(&format!("\nheap: the {} largest owners hold {:.1} MB, {:.1}% of the tracked bytes", v.len().min(40), mb(shown), if total > 0 { shown as f64 * 100.0 / total as f64 } else { 0.0 }));
    if first {
        // The first report's own symbols: the first heap grew by what dbghelp holds.
        let after = heaps();
        DBGHELP_MB.store((after[0].0 - heaps_before[0].0).max(0.0).to_bits(), Relaxed);
    }
    let dbg = f64::from_bits(DBGHELP_MB.load(Relaxed));
    let first_heap = heaps_before[0];
    out.push_str(&format!(
        "\nheap: the process's private memory {private_before:.0} MB before this report's symbols; its first heap (the Rust heap's blocks and every C library's) {:.0} MB allocated, {:.0} MB committed{}; {} other heaps {:.0} MB allocated",
        first_heap.0,
        first_heap.1,
        if first { String::new() } else { format!(", of the allocated {dbg:.0} MB dbghelp's (its symbols, loaded by the first report: {:.0} MB allocated without them)", first_heap.0 - dbg) },
        heaps_before.len() - 1,
        heaps_before[1..].iter().map(|h| h.0).sum::<f64>()
    ));
    println!("{out}");
}
