//! A crash's place in the log: an access violation or another fatal exception on any thread (the game's, the renderer's, a
//! driver's, a library's as the process ends) is written to stderr, which play.log holds, with the code, the faulting address as
//! a module and an offset in it, the address read or written, and that thread's stack. Without it a crash left only an exit
//! code (0xC0000005, RecompLauncher's "it read or wrote memory it shouldn't").

use std::sync::atomic::{AtomicU32, Ordering};

/// Exceptions written so far: the first few only (a fault inside a library's own handling can repeat).
static SEEN: AtomicU32 = AtomicU32::new(0);

/// The module holding `addr` and the offset in it.
fn place(addr: usize) -> String {
    use windows::Win32::Foundation::HMODULE;
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    let mut m = HMODULE::default();
    // SAFETY: the address is only looked up among the process's modules.
    if unsafe { GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, windows::core::PCWSTR(addr as *const u16), &mut m) }.is_err() {
        return format!("{addr:#x} (in no module)");
    }
    let mut name = [0u16; 260];
    let n = unsafe { GetModuleFileNameW(Some(m), &mut name) } as usize;
    let path = String::from_utf16_lossy(&name[..n]);
    let file = path.rsplit('\\').next().unwrap_or(&path).to_string();
    format!("{file}+{:#x} ({addr:#x})", addr - m.0 as usize)
}

unsafe extern "system" fn handler(e: *mut windows::Win32::System::Diagnostics::Debug::EXCEPTION_POINTERS) -> i32 {
    // SAFETY: the system passes a valid record for the exception being raised.
    let r = unsafe { &*(*e).ExceptionRecord };
    let code = r.ExceptionCode.0 as u32;
    // Access violation, illegal instruction, stack buffer overrun, heap corruption: never handled by the game.
    if matches!(code, 0xC000_0005 | 0xC000_001D | 0xC000_0409 | 0xC000_0374) && SEEN.fetch_add(1, Ordering::Relaxed) < 3 {
        let what = match (code, r.NumberParameters) {
            (0xC000_0005, n) if n >= 2 => format!(", {} {:#x}", ["reading", "writing", "?", "?", "?", "?", "?", "?", "executing"][r.ExceptionInformation[0].min(8)], r.ExceptionInformation[1]),
            _ => String::new(),
        };
        let thread = std::thread::current().name().map(str::to_string).unwrap_or_else(|| "a thread not the game's".into());
        eprintln!("crash: exception {code:#010x} at {}{what}, on {thread}", place(r.ExceptionAddress as usize));
        eprintln!("crash: stack:\n{}", std::backtrace::Backtrace::force_capture());
    }
    0 // EXCEPTION_CONTINUE_SEARCH: the exception goes on as before
}

/// Writes every fatal exception's place from now on (`play`'s start).
pub fn report() {
    use windows::Win32::System::Diagnostics::Debug::AddVectoredExceptionHandler;
    // SAFETY: the handler only reads the record and writes to stderr.
    unsafe { AddVectoredExceptionHandler(1, Some(handler)) };
}
