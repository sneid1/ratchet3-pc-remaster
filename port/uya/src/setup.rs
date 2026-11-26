//! `uya.exe` started with nothing after it (a double-click): the game finds the player's disc image itself and plays it, as
//! RecompLauncher plays it (main.rs). The disc it played last (the profile's `disc.txt`), else a disc image beside the program,
//! else Windows' open dialog asks for one; the file is checked to be the USA disc (SCUS-97353, `uyafmt::boot`) and remembered. A v0.1.3 player: "where do i put the game
//! iso? And the uya exe isnt launching" (started bare, the program printed its usage to a console that closed at once). What
//! stops the game then is said in a message box, not on that console.

use std::ffi::c_void;

const OFN_HIDEREADONLY: u32 = 0x4;
const OFN_NOCHANGEDIR: u32 = 0x8;
const OFN_PATHMUSTEXIST: u32 = 0x800;
const OFN_FILEMUSTEXIST: u32 = 0x1000;
const OFN_EXPLORER: u32 = 0x8_0000;
const MB_OK: u32 = 0x0;
const MB_RETRYCANCEL: u32 = 0x5;
const MB_ICONERROR: u32 = 0x10;
const MB_ICONWARNING: u32 = 0x30;
const MB_SETFOREGROUND: u32 = 0x1_0000;
const IDRETRY: i32 = 4;
/// The longest path the dialog hands back, in UTF-16 units.
const MAX_PATH_UNITS: usize = 32768;

/// Windows' OPENFILENAMEW.
#[repr(C)]
struct OpenFileName {
    size: u32,
    owner: *mut c_void,
    instance: *mut c_void,
    filter: *const u16,
    custom_filter: *mut u16,
    max_custom_filter: u32,
    filter_index: u32,
    file: *mut u16,
    max_file: u32,
    file_title: *mut u16,
    max_file_title: u32,
    initial_dir: *const u16,
    title: *const u16,
    flags: u32,
    file_offset: u16,
    file_extension: u16,
    def_ext: *const u16,
    cust_data: isize,
    hook: *mut c_void,
    template_name: *const u16,
    reserved_ptr: *mut c_void,
    reserved: u32,
    flags_ex: u32,
}

#[link(name = "comdlg32")]
unsafe extern "system" {
    fn GetOpenFileNameW(ofn: *mut OpenFileName) -> i32;
    fn CommDlgExtendedError() -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBoxW(owner: *mut c_void, text: *const u16, caption: *const u16, kind: u32) -> i32;
}

const CAPTION: &str = "Ratchet & Clank: Up Your Arsenal";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

/// `text` in a message box of `kind` (MB_*), its answer.
fn message(text: &str, kind: u32) -> i32 {
    let (text, caption) = (wide(text), wide(CAPTION));
    // SAFETY: both strings are NUL-terminated and live through the call. In front of other windows: the console Explorer gave
    // the game is let go once the play log opens, and a box behind other windows looked like a game that never started.
    unsafe { MessageBoxW(std::ptr::null_mut(), text.as_ptr(), caption.as_ptr(), kind | MB_SETFOREGROUND) }
}

/// What stopped the game when it was started with nothing after it, in a message box (the console it would print to closes with
/// the program); `log`, the play log that says more.
pub fn stopped(e: &str, log: &str) {
    message(&format!("The game stopped:\n\n{e}\n\nIts log: {log}"), MB_OK | MB_ICONERROR);
}

/// Why `path` is not a disc the game plays (None: it is the USA disc, SCUS-97353, whose `SCUS_973.53` the game boots).
fn not_the_disc(path: &str) -> Option<String> {
    uyafmt::boot::boot(path).err()
}

/// The disc image to play for profile `profile`: the one played last, else one beside the program, else one the player picks
/// in Windows' open dialog; remembered in the profile's `disc.txt`. None: the player closed the dialog.
pub fn disc(profile: &str) -> Result<Option<String>, String> {
    let remembered = format!("{profile}/disc.txt");
    if let Ok(path) = std::fs::read_to_string(&remembered) {
        let path = path.trim().to_string();
        if std::path::Path::new(&path).is_file() && not_the_disc(&path).is_none() {
            return Ok(Some(path));
        }
    }
    let beside = beside_program()?;
    let chosen = match beside {
        Some(path) => Some(path),
        None => ask()?,
    };
    if let Some(path) = &chosen {
        std::fs::create_dir_all(profile).map_err(|e| format!("{profile}: {e}"))?;
        std::fs::write(&remembered, path).map_err(|e| format!("{remembered}: {e}"))?;
    }
    Ok(chosen)
}

/// The first disc image (.iso) in the program's folder that is the USA disc.
fn beside_program() -> Result<Option<String>, String> {
    let exe = std::env::current_exe().map_err(|e| format!("the program's folder: {e}"))?;
    let dir = exe.parent().ok_or("the program has no folder")?;
    let mut isos: Vec<std::path::PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("iso")))
        .collect();
    isos.sort();
    Ok(isos.into_iter().filter_map(|p| p.to_str().map(str::to_string)).find(|p| not_the_disc(p).is_none()))
}

/// The disc image the player picks in Windows' open dialog, checked; asked again while the file picked is not the USA disc.
fn ask() -> Result<Option<String>, String> {
    let filter: Vec<u16> = "Disc images (*.iso)\0*.iso\0All files (*.*)\0*.*\0\0".encode_utf16().collect();
    let title = wide("Choose your Ratchet & Clank: Up Your Arsenal disc image (USA, SCUS-97353)");
    loop {
        let mut file = vec![0u16; MAX_PATH_UNITS];
        let mut ofn = OpenFileName {
            size: std::mem::size_of::<OpenFileName>() as u32,
            owner: std::ptr::null_mut(),
            instance: std::ptr::null_mut(),
            filter: filter.as_ptr(),
            custom_filter: std::ptr::null_mut(),
            max_custom_filter: 0,
            filter_index: 1,
            file: file.as_mut_ptr(),
            max_file: file.len() as u32,
            file_title: std::ptr::null_mut(),
            max_file_title: 0,
            initial_dir: std::ptr::null(),
            title: title.as_ptr(),
            flags: OFN_EXPLORER | OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
            file_offset: 0,
            file_extension: 0,
            def_ext: std::ptr::null(),
            cust_data: 0,
            hook: std::ptr::null_mut(),
            template_name: std::ptr::null(),
            reserved_ptr: std::ptr::null_mut(),
            reserved: 0,
            flags_ex: 0,
        };
        // SAFETY: every pointer in `ofn` is null or points into a buffer that outlives the call, `file` with its length.
        if unsafe { GetOpenFileNameW(&mut ofn) } == 0 {
            // SAFETY: no arguments.
            return match unsafe { CommDlgExtendedError() } {
                0 => Ok(None),
                e => Err(format!("the open dialog failed (CommDlgExtendedError {e:#x})")),
            };
        }
        let end = file.iter().position(|&c| c == 0).expect("the dialog's path ends in a NUL");
        let path = String::from_utf16(&file[..end]).map_err(|e| format!("the path picked: {e}"))?;
        match not_the_disc(&path) {
            None => return Ok(Some(path)),
            Some(why) => {
                let again = message(&format!("That file is not the USA disc of Ratchet & Clank: Up Your Arsenal (SCUS-97353), which the game plays:\n\n{path}\n{why}\n\nChoose another?"), MB_RETRYCANCEL | MB_ICONWARNING);
                if again != IDRETRY {
                    return Ok(None);
                }
            }
        }
    }
}
