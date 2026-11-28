//! UYA_PROFILE=START,SECONDS: where the game's threads spend their time (an instrument). START seconds after `uya play` begins,
//! a thread of its own pauses every other thread of the process about once a millisecond for SECONDS seconds, reads where it is
//! and the return addresses above it (the x64 unwinder, `RtlVirtualUnwind`), and lets it go on; then it names the addresses from
//! the executable's PDB (dbghelp) and prints, for each thread with samples, the functions it was in (self) and the functions on
//! its stack (inclusive), as shares of its samples. No allocation happens while a thread is paused.
//!
//! The stall watch (`watch`, UYA_STALL_WATCH=1 in `uya play`; off by default since the player's word, 25 November 2026: "turn
//! the census thing and other debug stuff off"): a field that has not ended for `STALL` gets every thread's stack written
//! to the log, three times half a second apart, named from the PDBs, once per stall. A freeze then says where each thread
//! waits or spins. The player's game froze on 24 November 2026 after 15 minutes on level 14 with nothing in play.log: the game
//! thread had ended field 52966's sound calls and never began field 52967's.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::Diagnostics::Debug::{
    CONTEXT, CONTEXT_FULL_AMD64, GetThreadContext, IMAGEHLP_LINE64, RtlLookupFunctionEntry, RtlVirtualUnwind, SYMBOL_INFO, SYMOPT_DEFERRED_LOADS,
    SYMOPT_LOAD_LINES, SYMOPT_UNDNAME, SymFromAddr, SymGetLineFromAddr64, SymInitialize, SymRefreshModuleList, SymSetOptions, UNW_FLAG_NHANDLER,
};
use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next};
use windows::Win32::System::Threading::{
    GetCurrentProcess, GetCurrentThreadId, GetThreadDescription, OpenThread, ResumeThread, SuspendThread, THREAD_GET_CONTEXT, THREAD_QUERY_LIMITED_INFORMATION,
    THREAD_SUSPEND_RESUME,
};

const DEPTH: usize = 40;

/// Starts the sampler when UYA_PROFILE is set.
pub fn start() {
    let Ok(v) = std::env::var("UYA_PROFILE") else { return };
    let mut it = v.split(',').map(|x| x.trim().parse::<f64>().expect("UYA_PROFILE=START,SECONDS (numbers)"));
    let (from, secs) = (it.next().expect("UYA_PROFILE=START,SECONDS"), it.next().expect("UYA_PROFILE=START,SECONDS"));
    std::thread::Builder::new().name("profiler".into()).spawn(move || run(from, secs)).expect("the profiler's thread");
}

struct Thread {
    id: u32,
    handle: HANDLE,
    name: String,
}

fn threads(known: &mut Vec<Thread>) {
    let (pid, me) = (std::process::id(), unsafe { GetCurrentThreadId() });
    // SAFETY: a snapshot of the system's threads, walked with an entry of its own size, closed after.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).expect("a thread snapshot");
        let mut te = THREADENTRY32 { dwSize: std::mem::size_of::<THREADENTRY32>() as u32, ..Default::default() };
        let mut ok = Thread32First(snap, &mut te).is_ok();
        while ok {
            if te.th32OwnerProcessID == pid && te.th32ThreadID != me && !known.iter().any(|t| t.id == te.th32ThreadID) {
                if let Ok(h) = OpenThread(THREAD_SUSPEND_RESUME | THREAD_GET_CONTEXT | THREAD_QUERY_LIMITED_INFORMATION, false, te.th32ThreadID) {
                    let name = GetThreadDescription(h).ok().and_then(|p| p.to_string().ok()).filter(|s| !s.is_empty()).unwrap_or_else(|| format!("thread {}", te.th32ThreadID));
                    known.push(Thread { id: te.th32ThreadID, handle: h, name });
                }
            }
            ok = Thread32Next(snap, &mut te).is_ok();
        }
        let _ = CloseHandle(snap);
    }
}

/// The thread's return addresses, innermost first, while it is paused (no allocation: `out` is the caller's).
fn sample(h: HANDLE, out: &mut [u64; DEPTH]) -> usize {
    // SAFETY: the thread is paused between SuspendThread and ResumeThread; its context is read into an aligned CONTEXT and
    // unwound with the system's unwinder through its own (unmoving) stack.
    unsafe {
        if SuspendThread(h) == u32::MAX {
            return 0;
        }
        let mut ctx = CONTEXT { ContextFlags: CONTEXT_FULL_AMD64, ..Default::default() };
        let mut n = 0;
        if GetThreadContext(h, &mut ctx).is_ok() {
            out[0] = ctx.Rip;
            n = 1;
            while n < DEPTH {
                let mut base = 0u64;
                let f = RtlLookupFunctionEntry(ctx.Rip, &mut base, None);
                if f.is_null() {
                    // A leaf with no unwind data: only where the sample landed, its return address on top of the stack.
                    if n > 1 || ctx.Rsp == 0 {
                        break;
                    }
                    ctx.Rip = *(ctx.Rsp as *const u64);
                    ctx.Rsp += 8;
                } else {
                    let (mut data, mut frame) = (std::ptr::null_mut(), 0u64);
                    let _ = RtlVirtualUnwind(UNW_FLAG_NHANDLER, base, ctx.Rip, f, &mut ctx, &mut data, &mut frame, None);
                }
                if ctx.Rip == 0 {
                    break;
                }
                out[n] = ctx.Rip;
                n += 1;
            }
        }
        ResumeThread(h);
        n
    }
}

fn run(from: f64, secs: f64) {
    std::thread::sleep(Duration::from_secs_f64(from));
    let mut known = Vec::new();
    let mut samples: Vec<(u32, Vec<u64>)> = Vec::with_capacity((secs * 1000.0 * 24.0) as usize);
    let t0 = Instant::now();
    let mut next_list = Instant::now();
    let mut rounds = 0u64;
    while t0.elapsed().as_secs_f64() < secs {
        if Instant::now() >= next_list {
            threads(&mut known);
            next_list = Instant::now() + Duration::from_millis(500);
        }
        for t in &known {
            let mut frames = [0u64; DEPTH];
            let n = sample(t.handle, &mut frames);
            if n > 0 {
                samples.push((t.id, frames[..n].to_vec()));
            }
        }
        rounds += 1;
        std::thread::sleep(Duration::from_micros(900));
    }
    report(&known, &samples, rounds, t0.elapsed().as_secs_f64());
    for t in known {
        // SAFETY: the handles opened above, closed once.
        unsafe {
            let _ = CloseHandle(t.handle);
        }
    }
}

/// dbghelp for this process, started once (the profiler and the stall watch share it), with the modules loaded since its start
/// (a level's library) made known to it.
fn dbghelp() -> Result<HANDLE, String> {
    static STARTED: OnceLock<Result<(), String>> = OnceLock::new();
    // SAFETY: dbghelp for this process, the PDBs beside the modules.
    let process = unsafe { GetCurrentProcess() };
    STARTED
        .get_or_init(|| {
            // The executable names its PDB without a folder ("uya.pdb", lld's /pdbaltpath): it is looked for beside the executable.
            let exe = std::env::current_exe().map_err(|e| format!("dbghelp: the executable: {e}"))?;
            let dir = std::ffi::CString::new(exe.parent().ok_or("dbghelp: the executable's folder")?.to_string_lossy().as_bytes()).map_err(|e| format!("dbghelp: {e}"))?;
            unsafe {
                SymSetOptions(SYMOPT_UNDNAME | SYMOPT_DEFERRED_LOADS | SYMOPT_LOAD_LINES);
                SymInitialize(process, windows::core::PCSTR(dir.as_ptr() as *const u8), true).map_err(|e| format!("dbghelp: {e}"))
            }
        })
        .clone()?;
    unsafe { SymRefreshModuleList(process) }.map_err(|e| format!("dbghelp: the modules: {e}"))?;
    Ok(process)
}

/// The function at an address (`a` itself when no PDB names it).
fn symbol(process: HANDLE, a: u64) -> String {
    let mut buf = vec![0u64; (std::mem::size_of::<SYMBOL_INFO>() + 1024) / 8 + 1];
    let info = buf.as_mut_ptr() as *mut SYMBOL_INFO;
    // SAFETY: `buf` is large enough for SYMBOL_INFO and 1024 name bytes, and aligned for it.
    unsafe {
        (*info).SizeOfStruct = std::mem::size_of::<SYMBOL_INFO>() as u32;
        (*info).MaxNameLen = 1024;
        let mut disp = 0u64;
        if SymFromAddr(process, a, Some(&mut disp), info).is_ok() {
            let bytes = std::slice::from_raw_parts((*info).Name.as_ptr() as *const u8, (*info).NameLen as usize);
            let s = String::from_utf8_lossy(bytes).to_string();
            s.chars().take(150).collect()
        } else {
            format!("{a:#x}")
        }
    }
}

/// The source file and line of an address, where the PDB has its line table.
fn source_line(process: HANDLE, a: u64) -> Option<String> {
    let mut line = IMAGEHLP_LINE64 { SizeOfStruct: std::mem::size_of::<IMAGEHLP_LINE64>() as u32, ..Default::default() };
    let mut disp = 0u32;
    // SAFETY: dbghelp fills `line`, whose file name it owns.
    unsafe {
        SymGetLineFromAddr64(process, a, &mut disp, &mut line).ok()?;
        let file = line.FileName.to_string().unwrap_or_default();
        Some(format!("{}:{}", file.rsplit(['\\', '/']).next().unwrap_or(""), line.LineNumber))
    }
}

/// How long the busy watch keeps its readings: longer than a level's load (4 to 7 s), whose field is the longest the loop waits.
const BUSY_KEPT: Duration = Duration::from_secs(12);

/// The busy watch's requests (`busy`): a slow field's number, start and end.
static BUSY: OnceLock<std::sync::Mutex<std::sync::mpsc::Sender<(u64, Instant, Instant)>>> = OnceLock::new();

#[link(name = "kernel32")]
unsafe extern "system" {
    fn QueryThreadCycleTime(thread: HANDLE, cycles: *mut u64) -> i32;
}

/// The busy watch: every 50 ms the cycles each of the process's threads has run (QueryThreadCycleTime, which counts at the
/// time stamp counter's rate), the last `BUSY_KEPT` of readings kept. For a field the game's loop found slow (`busy`), once
/// a reading after it is in: the threads that ran between the readings around it, the busiest first, with their names. A
/// stall then says whether the game's own threads ran through it (their code ran long) or did not (they waited), and which
/// of the other threads ran meanwhile.
pub fn busy_watch() {
    let (tx, rx) = std::sync::mpsc::channel::<(u64, Instant, Instant)>();
    if BUSY.set(std::sync::Mutex::new(tx)).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("uya busy watch".into())
        .spawn(move || {
            let mut known = Vec::new();
            // Each reading: when, the time stamp counter then, and each known thread's cycles.
            let mut readings: std::collections::VecDeque<(Instant, u64, Vec<(u32, u64)>)> = std::collections::VecDeque::new();
            let mut asked: Vec<(u64, Instant, Instant)> = Vec::new();
            let mut listed: Option<Instant> = None;
            loop {
                if listed.is_none_or(|l| l.elapsed() >= Duration::from_secs(1)) {
                    threads(&mut known);
                    listed = Some(Instant::now());
                }
                let (now, tsc) = (Instant::now(), unsafe { core::arch::x86_64::_rdtsc() });
                let cycles = known
                    .iter()
                    .map(|t| {
                        let mut c = 0u64;
                        // SAFETY: a handle opened with THREAD_QUERY_LIMITED_INFORMATION (`threads`), kept open.
                        unsafe { QueryThreadCycleTime(t.handle, &mut c) };
                        (t.id, c)
                    })
                    .collect();
                readings.push_back((now, tsc, cycles));
                while readings.front().is_some_and(|r| now.duration_since(r.0) > BUSY_KEPT) {
                    readings.pop_front();
                }
                while let Ok(a) = rx.try_recv() {
                    asked.push(a);
                }
                asked.retain(|&(f, from, to)| {
                    if now <= to {
                        return true;
                    }
                    match (readings.iter().rev().find(|r| r.0 <= from), readings.iter().find(|r| r.0 >= to)) {
                        (Some(a), Some(b)) => busy_line(f, a, b, &known),
                        _ => println!("play: field {f}: the busy watch has no readings around it"),
                    }
                    false
                });
                std::thread::sleep(Duration::from_millis(50));
            }
        })
        .expect("the busy watch's thread");
}

/// Field `f`, slow (`rtgame`), from `from` to `to`: its threads' time is said once the busy watch has read them after it.
pub fn busy(f: u64, from: Instant, to: Instant) {
    if let Some(tx) = BUSY.get() {
        let _ = tx.lock().expect("busy watch lock").send((f, from, to));
    }
}

/// The threads' time between readings `a` and `b` (ms at the time stamp counter's rate between them), every thread that ran
/// 1 ms or more, the busiest first.
fn busy_line(f: u64, a: &(Instant, u64, Vec<(u32, u64)>), b: &(Instant, u64, Vec<(u32, u64)>), known: &[Thread]) {
    let span = b.0.duration_since(a.0).as_secs_f64() * 1e3;
    let per_ms = (b.1 - a.1) as f64 / span;
    let mut ran: Vec<(f64, &str)> = b
        .2
        .iter()
        .map(|&(id, c)| {
            // A thread not yet listed at reading `a` (the list is made again once a second) is counted from its start, at most
            // a second before `a`.
            let c0 = a.2.iter().find(|x| x.0 == id).map_or(0, |x| x.1);
            let name = known.iter().find(|t| t.id == id).map_or("?", |t| t.name.as_str());
            ((c - c0) as f64 / per_ms, name)
        })
        .filter(|r| r.0 >= 1.0)
        .collect();
    ran.sort_by(|x, y| y.0.total_cmp(&x.0));
    let list: Vec<String> = ran.iter().map(|(ms, name)| format!("{name} {ms:.1}")).collect();
    println!("play: field {f}: the threads' time in the {span:.0} ms around it: {}", if list.is_empty() { "none ran 1 ms".to_string() } else { list.join(", ") });
}

/// How long a field may go unended before the stall watch writes the threads' stacks: past a level's load (6 s on level 19 at
/// 1440 lines) and its textures' preload.
const STALL: Duration = Duration::from_secs(15);

/// The stall watch, when UYA_STALL_WATCH is set: `field` is the game thread's field counter (`rtgame::game` stores each field's
/// number as it begins).
pub fn watch(field: Arc<AtomicU64>) {
    if std::env::var_os("UYA_STALL_WATCH").is_none() {
        return;
    }
    std::thread::Builder::new()
        .name("uya stall watch".into())
        .spawn(move || {
            let (mut last, mut since, mut told) = (field.load(Ordering::SeqCst), Instant::now(), false);
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let f = field.load(Ordering::SeqCst);
                if f != last {
                    (last, since, told) = (f, Instant::now(), false);
                } else if f != 0 && !told && since.elapsed() >= STALL {
                    told = true;
                    stalled(f, since.elapsed());
                }
            }
        })
        .expect("the stall watch's thread");
}

/// Every thread's stack, three times half a second apart: in full the first time, and after it the threads whose stack moved
/// (a thread that spins, not one that waits).
fn stalled(f: u64, held: Duration) {
    let mut known = Vec::new();
    threads(&mut known);
    let mut rounds: Vec<Vec<Vec<u64>>> = Vec::new();
    for r in 0..3 {
        if r > 0 {
            std::thread::sleep(Duration::from_millis(500));
        }
        let mut got = Vec::with_capacity(known.len());
        for t in &known {
            let mut frames = [0u64; DEPTH];
            let n = sample(t.handle, &mut frames);
            got.push(frames[..n].to_vec());
        }
        rounds.push(got);
    }
    println!("stall: field {f} has not ended for {:.0} s: the stacks of the game's {} threads", held.as_secs_f64(), known.len());
    match dbghelp() {
        Err(e) => println!("stall: {e}"),
        Ok(process) => {
            let frame = |a: u64| match source_line(process, a) {
                Some(l) => format!("{} ({l})", symbol(process, a)),
                None => symbol(process, a),
            };
            for (k, t) in known.iter().enumerate() {
                println!("stall: thread {} ({})", t.name, t.id);
                for (d, &a) in rounds[0][k].iter().enumerate() {
                    println!("stall:   {d:2} {}", frame(a));
                }
                for (r, round) in rounds.iter().enumerate().skip(1) {
                    if round[k] != rounds[0][k] {
                        println!("stall:   {} s later:", r as f64 * 0.5);
                        for (d, &a) in round[k].iter().enumerate().take(12) {
                            println!("stall:     {d:2} {}", frame(a));
                        }
                    }
                }
            }
        }
    }
    for t in known {
        // SAFETY: the handles opened above, closed once.
        unsafe {
            let _ = CloseHandle(t.handle);
        }
    }
}

fn report(known: &[Thread], samples: &[(u32, Vec<u64>)], rounds: u64, secs: f64) {
    let process = match dbghelp() {
        Ok(p) => p,
        Err(e) => {
            println!("profile: {e}");
            return;
        }
    };
    let mut names: HashMap<u64, String> = HashMap::new();
    let mut name = |a: u64| -> String { names.entry(a).or_insert_with(|| symbol(process, a)).clone() };
    println!("profile: {rounds} rounds in {secs:.1} s, {} samples, {} threads", samples.len(), known.len());
    // A sample in a wait (the kernel's waits and sleeps) is a thread with nothing to do: only the others are counted.
    let idle = |n: &str| (n.starts_with("Nt") || n.starts_with("Zw")) && (n.contains("Wait") || n.contains("Delay") || n.contains("RemoveIoCompletion"));
    let mut busy: Vec<(usize, &Thread, Vec<&Vec<u64>>)> = Vec::new();
    for t in known {
        let mine: Vec<&Vec<u64>> = samples.iter().filter(|s| s.0 == t.id && !idle(&name(s.1[0]))).map(|s| &s.1).collect();
        let all = samples.iter().filter(|s| s.0 == t.id).count();
        if !mine.is_empty() && all > 0 {
            busy.push((mine.len(), t, mine));
        }
    }
    busy.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, t, _) in &busy {
        let all = samples.iter().filter(|s| s.0 == t.id).count();
        let mine = busy.iter().find(|b| b.1.id == t.id).map_or(0, |b| b.0);
        println!("profile: busy {:5.1}%  {} ({})", 100.0 * mine as f64 / all as f64, t.name, t.id);
    }
    for (_, t, mine) in busy.into_iter().take(12) {
        let (mut own, mut all): (HashMap<String, u64>, HashMap<String, u64>) = (HashMap::new(), HashMap::new());
        // Each sample charged to the innermost of the port's own functions on its stack (the allocator, memcpy, wgpu and the
        // driver below it counted with the function that called them).
        let mut ours: HashMap<String, u64> = HashMap::new();
        for frames in &mine {
            if let Some(f) = frames.iter().map(|&a| name(a)).find(|n| n.starts_with("uya::") || n.starts_with("uyart::") || n.starts_with("uyafmt::") || n.starts_with("tr_")) {
                *ours.entry(f).or_default() += 1;
            }
        }
        let mut ours: Vec<(u64, String)> = ours.into_iter().map(|(k, v)| (v, k)).collect();
        ours.sort_by(|a, b| b.cmp(a));
        // The same by source line (the port's crates keep line tables).
        let mut lines: HashMap<String, u64> = HashMap::new();
        for frames in &mine {
            if let Some(&a) = frames.iter().find(|&&a| { let n = name(a); n.starts_with("uya::") || n.starts_with("uyart::") || n.starts_with("uyafmt::") }) {
                let at = source_line(process, a).unwrap_or_else(|| format!("{a:#x}"));
                *lines.entry(at).or_default() += 1;
            }
        }
        let mut lines: Vec<(u64, String)> = lines.into_iter().map(|(k, v)| (v, k)).collect();
        lines.sort_by(|a, b| b.cmp(a));
        for frames in &mine {
            *own.entry(name(frames[0])).or_default() += 1;
            let mut seen: Vec<String> = Vec::new();
            for &a in frames.iter() {
                let n = name(a);
                if !seen.contains(&n) {
                    *all.entry(n.clone()).or_default() += 1;
                    seen.push(n);
                }
            }
        }
        // Idle threads (waiting in the kernel most of the time) are listed with their top only.
        let n = mine.len() as f64;
        let mut own: Vec<(u64, String)> = own.into_iter().map(|(k, v)| (v, k)).collect();
        own.sort_by(|a, b| b.cmp(a));
        let mut all: Vec<(u64, String)> = all.into_iter().map(|(k, v)| (v, k)).collect();
        all.sort_by(|a, b| b.cmp(a));
        println!("profile: thread {} ({}): {} samples", t.name, t.id, mine.len());
        for (c, f) in ours.iter().take(30).filter(|(c, _)| *c as f64 / n >= 0.005) {
            println!("profile:   ours {:5.1}%  {f}", 100.0 * *c as f64 / n);
        }
        for (c, f) in lines.iter().take(40).filter(|(c, _)| *c as f64 / n >= 0.004) {
            println!("profile:   line {:5.1}%  {f}", 100.0 * *c as f64 / n);
        }
        for (c, f) in own.iter().take(30).filter(|(c, _)| *c as f64 / n >= 0.005) {
            println!("profile:   self {:5.1}%  {f}", 100.0 * *c as f64 / n);
        }
        for (c, f) in all.iter().take(60).filter(|(c, _)| *c as f64 / n >= 0.01) {
            println!("profile:   incl {:5.1}%  {f}", 100.0 * *c as f64 / n);
        }
    }
}
