//! `uya play <disc.iso>`: the game in a window, played live (PLAN items 3.1 to 3.3, 4.3 and 4.1's display mode and resolution).
//!
//! Three threads, so that no stage waits on another more than it must:
//! - the window's (the process's main thread): the window's events, the keyboard and mouse (`input::Kbm`), the cursor (shown and
//!   free in the game's menus, hidden and held in the window in play), the display mode (windowed or fullscreen borderless);
//! - the game's (`rtgame.rs`): each field reads every input device (`input::Input`), hands the pad report to the game (port 0
//!   slot 0, as padman's frames carry it), runs the game's code for the field on the port's own runtime (the code as port/uyatr
//!   translated it, `uyart::Machine::field`; there is no other machine since PLAN 0.6), and sends the field's GS jobs as the DMA
//!   kicks handed them over (`jobs.rs`) and the EE RAM they were built from (the field before's) to the renderer; it runs at the
//!   console's 59.94 fields a second at most, and the next field's code runs while the renderer draws the last one; the menus
//!   are made a PC game's around each field (`menus.rs`: the pointer, the Display settings page); the game's movies play into the
//!   window from within its field (`cinema.rs`), and its sound calls go to the port's sound engine (`snd`, `audio.rs`);
//! - the renderer's: draws each field's frame natively (`native::Native::render`, the picture left on the GPU) and presents it
//!   in the window (a copy into the window's texture and one triangle scaled to the window, letterboxed), with no read back to
//!   the CPU. A front end's field (its menus, the 3D its VU1 programs draw, a trip's space scene) is drawn natively from its jobs
//!   as the GS draws them (`native::Native::render_screen`), on a cleared picture at the window's shape. Nothing is emulated:
//!   what the native frame cannot draw is missing from the picture and named in the log, and a frame it cannot draw at all stops
//!   the game, naming why. A movie's picture is upscaled to the window (edge-directed, then sharpened).
//!
//! The memory card is the profile's `memcard` folder, the card's files as plain files (`uyart::mc`: the game's saves are
//! `memcard/BASCUS-97353RATCHET/save0.bin` to `save3.bin`). Its boxes are answered for the player and not shown (a PC game makes
//! its save file without asking and saves without a box): see MC_MESSAGE and LEVEL_MESSAGE.
//!
//! Options: --warp LEVEL@FIELD,... (after FIELD, at the first field in which the game itself asks for levels: in play, after a
//! cutscene, a menu or the ship; the game is asked for LEVEL through its own level change),
//! --control (the game driven by commands on stdin, answered on stdout: `control.rs`; fields run only when told, as with --fast),
//! --start LEVEL (straight into LEVEL: the front end's own request_level_change as soon as the front end runs, no menus),
//! --fast (no field pacing: fields run as fast as the game's code goes, a field the renderer is not ready for is not drawn, and
//! movies are skipped as they start unless --movies), --hover FIELD:X:Y,... (the cursor moved there, no click), --mouse FIELD:DX:DY:FIELDS,... (the mouse moved DX, DY counts in each
//! of FIELDS fields from FIELD, as the window's raw motion: x right, y down), --write-icon (copy the portrait to the profile and end),
//! --height H (the native picture's height; default the profile's resolution setting), --monitor N (default the first
//! monitor that is not the primary one), --aspect W:H (the window and picture at that shape, not the monitor's), --seconds S (quit after S seconds), --press FIELD:BUTTONS:FIELDS,... (scripted presses
//! on top of the devices, as `frames` takes them; a movie starting while one is held is skipped), --click FIELD:X:Y,... (a left
//! click at X, Y, shares of the picture, the cursor left there), --prompts keyboard|xbox|dualsense|dualshock4 (the button prompts of
//! that device whatever is used; default the device used last), --profile DIR (the player's profile folder,
//! default %USERPROFILE%\Games\Ratchet and Clank 3, which holds the prompt atlas `prompts.bin` of `scripts/prompt-icons.py`, the
//! memory card and `settings.txt`), --rift DIR (UPDATED TEXTURES draws the surfaces of DIR, a test's folder, in place of the
//! profile's `texture_replacements\0-riftapart`), --mute (the sound device gets silence; everything is still counted), --wav FILE (the mix, as it is
//! handed to the sound device, before the mute). UYA_RAM_AT=F,F,... writes the EE RAM at those fields to ramFFFFF.bin in the working
//! folder.

use crate::input::{Device, Input};

/// The game thread, on the port's own runtime.
#[path = "rtgame.rs"]
mod rtgame;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{DeviceEvent, DeviceId, ElementState, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::PhysicalKey;
use winit::window::{CursorGrabMode, Window, WindowId};

pub struct Options {
    /// The native picture's height; None: the profile's setting (`settings.rs`).
    pub height: Option<u32>,
    pub seconds: Option<f64>,
    pub monitor: Option<usize>,
    /// --aspect W:H: the window, and so the picture, at that shape instead of the monitor's (a 16:9 picture on a portrait
    /// monitor: the runs that dump the game's RAM for `uya native` keep the shape the player plays at).
    pub aspect: Option<(f32, f32)>,
    pub presses: Vec<(u64, u16, u64)>,
    /// --write-icon: copy the bundled portrait to the profile and end, with no window.
    pub write_icon: bool,
    /// --warp LEVEL@FIELD,...: after FIELD, once the game is in play (`Menus::asks_levels`), it is asked for LEVEL through its own
    /// level change.
    pub warps: Vec<(u32, u64)>,
    /// --click FIELD:X:Y,...: the left button clicked at (X, Y), shares of the picture, at FIELD (the cursor stays there).
    pub clicks: Vec<(u64, f32, f32)>,
    /// --hover FIELD:X:Y,...: the cursor moved to (X, Y) at FIELD and left there, no button pressed.
    pub hovers: Vec<(u64, f32, f32)>,
    /// --mouse FIELD:DX:DY:FIELDS,...: the mouse moved (DX, DY) counts in each of FIELDS fields from FIELD (raw motion, y down).
    pub mice: Vec<(u64, f64, f64, u64)>,
    /// --fast: no field pacing, fields the renderer is not ready for not drawn, movies skipped (unless `movies`).
    pub fast: bool,
    /// --movies: the movies played even with --fast.
    pub movies: bool,
    /// --start LEVEL: the level asked for from the front end as soon as it runs (`rtgame::request_level`).
    pub start: Option<u32>,
    /// --control: the control channel on stdin (`control.rs`).
    pub control: bool,
    pub prompts: Option<Device>,
    /// The player's profile folder (the prompt atlas `prompts.bin`).
    pub profile: String,
    /// --rift DIR: the Rift Apart surfaces of DIR (a test's folder) in place of the profile's `texture_replacements\0-riftapart`
    /// (`texnames::open`).
    pub rift: Option<String>,
    /// --mute and --wav FILE: the live audio (`audio.rs`).
    pub audio: crate::audio::Options,
}

/// The front end's memory card message: frontbin draws it by the word at 0x1D4CE8 (switch 0x3B82F0, jump table 0x31C890) and
/// reads its answer by the same word (0x3B8710, table 0x31C910). 15 is "no save data on the memory card: would you like to create
/// a save file?" (text 0x19F) with "â—‹ Yes" (0x8E3) and "â–³ No"; 27 is "if you do not create a save file you will not be able to
/// save: continue anyway?" (0x1A1, 0x19A) with "âœ• Yes" (0x8E2) and "â–³ No". A PC game makes its save file without asking: 15 is
/// answered Yes (Circle), and 27, should it come, Yes (Cross). Neither is shown, nor are the card's working messages around them
/// ("Checking Memory Card (PS2)... please do not remove", "Creating Save Game File": the values whose answer routine is the
/// empty 0x3B8D08, and 14), so making the save file is a short pause on the menu. The rest (the card's errors, 11 with no box,
/// 18 the save slots) show as the game draws them.
const MC_MESSAGE: u32 = 0x1D_4CE8;
const MC_HIDDEN: [u32; 15] = [9, 10, 12, 13, 14, 15, 16, 17, 23, 25, 27, 29, 30, 31, 32];
/// A pause's SAVE screen checks the card through the same word: its state (`menucode::MenuGlobals::save`[0], level 5's at
/// 0x1D8300; func_L05_00477858) is 0 while a slot is chosen, 1 as the screen opens, and 2 or 3 while the card is checked, when the
/// screen draws the front end's messages by MC_MESSAGE (func_L05_00477BA8: 15 is text 0x19F with 0x8E3 and 0x8DF, as in the front
/// end) and reads their answers (func_L05_00478000: Circle at 15 creates the save file). There are its boxes when the card has no
/// save file (a --start run, which skips the front end's card check): answered and hidden as the front end's are.
const SAVE_CARD: u32 = 2;
/// A level's save messages: the struct func_003DBEC8 draws (`menucode::MenuGlobals::saves`, in each level's own place: Veldin's is
/// 0x1EA740, with the level's switch at 0x413938 and jump table 0x2BBA10): +0x40 picks the box, +0x5C is its frame count. 2 is the
/// autosave notice ("Progress is saving. While this icon is on screen, do not remove the Memory Card (PS2)...", text 0xB75), a
/// modal box that offers "âœ• Continue" once its frame count reaches 121. The count is put there at once and Continue pressed for
/// the player, and the box is not shown: the save is a pause of a few fields. As `message`, it is the value LEVEL_SAVING.
const LEVEL_AUTOSAVE: u32 = 2;
const LEVEL_SAVING: u32 = 0x100;
const MC_CREATE: u32 = 15;
const MC_CREATE_YES: u16 = crate::input::CIRCLE;
const MC_CONTINUE: u32 = 27;
const MC_CONTINUE_YES: u16 = crate::input::CROSS;

/// WIDESCREEN (`settings.rs`): the picture at the window's shape, or (off) at the original picture's, with bars at the sides. Set
/// by the renderer from the settings; read by the presenter, which sets the game's view to the shape (`Screen::acquire`).
static WIDE: AtomicBool = AtomicBool::new(true);

/// The NTSC field: 1001 / 60000 s.
const FIELD: Duration = Duration::from_nanos(16_683_333);

/// EE RAM, as the native frame reads it.
const RAM: usize = 32 << 20;

/// What the game thread sends the renderer per field.
pub(crate) struct Frame {
    field: u64,
    /// When the game thread began the field (the presenter's timeline: `Timeline`).
    at: Instant,
    /// The EE RAM the field's jobs were built from: as it stood at its last frame's first kick (or at the field's end, when it
    /// kicked none).
    ram: uyart::RamCopy,
    /// Its draw lists still in `ram`, built from it on the walk thread (`walk_ahead`).
    jobs: crate::jobs::Kicked,
    /// The main frame's pages read back in jobs before these the renderer did not get (`rtgame::main_read_back`).
    pub(crate) read_back: Vec<u32>,
    /// The image transfers into GS memory (`uyart::hw::GsUpload`) of the fields and frames since the last frame the renderer got
    /// that it did not (fields it could not take while the game ran unpaced, frames a field ran before its last, fields of a
    /// hidden memory card message), oldest first; and the frame's own. The native renderer keeps GS memory from them
    /// (`native::Native::take_uploads`).
    uploads_before: Vec<uyart::hw::GsUpload>,
    uploads: Vec<uyart::hw::GsUpload>,
    /// A front end's program is the code in RAM (`uyart::Machine::front_end`): the frame is its own (the menus, a trip's or an
    /// arrival's space scene), never a level's world, whatever camera the level left set.
    front_end: bool,
    /// The device whose button prompts the frame shows, and whether the game shows a menu (with the keyboard and mouse, the
    /// prompts of what the mouse clicks are not drawn).
    prompts: Device,
    menu: bool,
    /// The port's text in place of the game's (`menus.rs`).
    lines: Vec<crate::menus::Line>,
    /// Where the mouse points on the picture (shares), for the answer buttons of a box.
    pointer: Option<(f32, f32)>,
    /// The player's bindings: what the prompts show (`prompts::Look::bindings`).
    bindings: crate::bindings::Bindings,
}

/// What the renderer is sent: a field's frame, or a movie's picture (`cinema.rs`).
pub(crate) enum Msg {
    Field(Frame),
    Movie(crate::cinema::Picture),
    /// The control channel's `shot`: the picture shown, written to this PNG file; the result goes back.
    Shot(String, std::sync::mpsc::Sender<Result<(), String>>),
    /// Answered once every frame sent before it is drawn (a control command's last field: its picture, and the answer
    /// buttons and controls it offers, are the window's before the command is answered).
    Drawn(std::sync::mpsc::Sender<()>),
    /// The player changed the settings: the resolution is applied here.
    Settings(crate::settings::Settings),
}

/// What the window's thread is told: a thread's error or the game's end (the window closes), whether the game shows a menu (the
/// cursor), the settings the player changed (the display mode).
enum Event {
    Ready,
    Ended(Result<(), String>),
    /// The renderer's end, after it took the GPU, DLSS and Streamline down (`render`'s end): the process ends after it (`end`).
    Rendered(Result<(), String>),
    Menu(bool),
    Settings(crate::settings::Settings),
}

pub fn play(iso: &str, o: Options) -> Result<(), String> {
    println!("play: Ratchet & Clank: Up Your Arsenal PC Remaster v{}", crate::update::VERSION);
    // A crash's place and stack into the log (`crash.rs`).
    #[cfg(windows)]
    crate::crash::report();
    // UYA_PROFILE=START,SECONDS: the threads' time by function (`sampler.rs`, an instrument).
    #[cfg(windows)]
    crate::sampler::start();
    // The game runs at normal priority (main() lowers it for the batch tools).
    #[cfg(windows)]
    unsafe {
        use windows::Win32::System::Threading::{GetCurrentProcess, NORMAL_PRIORITY_CLASS, SetPriorityClass};
        SetPriorityClass(GetCurrentProcess(), NORMAL_PRIORITY_CLASS).map_err(|e| format!("play: SetPriorityClass: {e}"))?;
    }
    // The game's threads have a core of their own; every other thread of the process the rest (`keep_game_core`).
    #[cfg(windows)]
    keep_game_core()?;
    // A copy of the bundled portrait for external launchers (`icon.rs`).
    let icon = format!("{}/icon.ico", o.profile);
    if o.write_icon || !std::path::Path::new(&icon).exists() {
        crate::icon::write(&icon)?;
        println!("play: icon written to {icon}");
        if o.write_icon {
            return Ok(());
        }
    }
    let el = EventLoop::<Event>::with_user_event().build().map_err(|e| format!("play: event loop: {e}"))?;
    let mut app = App {
        iso: iso.to_string(),
        o,
        proxy: el.create_proxy(),
        window: None,
        kbm: None,
        quit: Arc::new(AtomicBool::new(false)),
        result: None,
        t0: Instant::now(),
        focused: false,
        menu: true,
        fullscreen: false,
        windowed: (PhysicalSize::new(1, 1), PhysicalPosition::new(0, 0)),
        rendering: false,
        ending: None,
    };
    el.run_app(&mut app).map_err(|e| format!("play: event loop: {e}"))?;
    app.quit.store(true, Ordering::SeqCst);
    app.result.unwrap_or(Ok(()))
}

/// Where the window was when the game last ended (the profile's `window.txt`): its monitor (by name) and its place and size
/// while windowed. The game opens there again, as a PC game does; with no file (or its monitor gone) it opens on the main
/// monitor.
struct Placement {
    monitor: String,
    at: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
}

impl Placement {
    const FILE: &'static str = "window.txt";

    fn load(profile: &str) -> Result<Option<Placement>, String> {
        let path = format!("{profile}/{}", Placement::FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(format!("{path}: {e}")),
        };
        let mut v = std::collections::HashMap::new();
        for line in text.lines().map(str::trim).filter(|l| !l.is_empty()) {
            let (k, x) = line.split_once('=').ok_or_else(|| format!("{path}: \"{line}\" is not key = value"))?;
            v.insert(k.trim().to_string(), x.trim().to_string());
        }
        let get = |k: &str| v.get(k).ok_or_else(|| format!("{path}: no \"{k}\""));
        let num = |k: &str| -> Result<i32, String> { get(k)?.parse().map_err(|_| format!("{path}: {k} is not a number")) };
        let size = |k: &str| -> Result<u32, String> { get(k)?.parse().map_err(|_| format!("{path}: {k} is not a size")) };
        Ok(Some(Placement { monitor: get("monitor")?.clone(), at: PhysicalPosition::new(num("x")?, num("y")?), size: PhysicalSize::new(size("width")?, size("height")?) }))
    }

    fn save(&self, profile: &str) -> Result<(), String> {
        let path = format!("{profile}/{}", Placement::FILE);
        let text = format!("monitor = {}\nx = {}\ny = {}\nwidth = {}\nheight = {}\n", self.monitor, self.at.x, self.at.y, self.size.width, self.size.height);
        std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))
    }
}

/// Whether the window rectangle (place, size) has its middle on the monitor.
fn on_monitor(at: PhysicalPosition<i32>, size: PhysicalSize<u32>, m: &winit::monitor::MonitorHandle) -> bool {
    let (mp, ms) = (m.position(), m.size());
    let (cx, cy) = (at.x + size.width as i32 / 2, at.y + size.height as i32 / 2);
    cx >= mp.x && cx < mp.x + ms.width as i32 && cy >= mp.y && cy < mp.y + ms.height as i32
}

/// A window of the picture's size (`h` lines, at the monitor's shape) centred on the monitor where the monitor holds it, else as
/// large as it allows.
fn centred(h: u32, m: &winit::monitor::MonitorHandle) -> (PhysicalSize<u32>, PhysicalPosition<i32>) {
    let (mp, ms) = (m.position(), m.size());
    let w = (h as f32 * ms.width as f32 / ms.height as f32).round() as u32;
    let fit = (ms.width as f32 * 0.95 / w as f32).min(ms.height as f32 * 0.9 / h as f32).min(1.0);
    let (ww, wh) = ((w as f32 * fit) as u32, (h as f32 * fit) as u32);
    (PhysicalSize::new(ww, wh), PhysicalPosition::new(mp.x + (ms.width as i32 - ww as i32) / 2, mp.y + (ms.height as i32 - wh as i32) / 2))
}

struct App {
    iso: String,
    o: Options,
    proxy: EventLoopProxy<Event>,
    window: Option<Arc<Window>>,
    kbm: Option<Arc<Mutex<crate::input::Kbm>>>,
    quit: Arc<AtomicBool>,
    result: Option<Result<(), String>>,
    t0: Instant,
    focused: bool,
    /// The game shows a menu: the cursor is shown and free.
    menu: bool,
    /// Whether the window is fullscreen now, and its size and place while windowed (kept as the player moves and sizes it, and
    /// given back when fullscreen ends).
    fullscreen: bool,
    windowed: (PhysicalSize<u32>, PhysicalPosition<i32>),
    /// The render thread runs (`Event::Rendered` when it ends), and when the game's end began (`end`).
    rendering: bool,
    ending: Option<Instant>,
}

/// The longest the process waits for the renderer's own end after the game's (`App::end`).
const ENDING: Duration = Duration::from_secs(10);

/// The CPU sets of the core the game's threads have to themselves (`keep_game_core`).
#[cfg(windows)]
static GAME_CORE: std::sync::OnceLock<Vec<u32>> = std::sync::OnceLock::new();

/// How many cores of the fastest class besides the game's a PC with slower cores needs for the other threads to keep to them
/// (`keep_game_core`): the frame builder, its walk thread, the presenter and the crew's four.
#[cfg(windows)]
const FAST_CORES: usize = 4;

/// How many cores besides the game's a PC needs for one to be kept for the game's threads (`keep_game_core`). With fewer, every
/// other thread would share what is left: the issue #2 player's two-core laptop (an Intel HD 520's, four logical processors)
/// kept one core for the game, whose code took 1 to 13 ms a field, and left the other to the frame builder, its crew, the
/// presenter and the movies, which built a frame in 24 to 35 ms, drew 144 to 241 of every 300 frames and played the first
/// movie's 13.3 s in 14.3.
#[cfg(windows)]
const LEAST_OTHER_CORES: usize = 3;

/// Keeps a core for the game's threads (Windows' CPU sets): the PC's last core of its fastest class, both its logical
/// processors, for the host's field loop and the game's own threads (`uyart::kernel::THREAD_START`), which take turns, one
/// running at a time. Every other thread of the process (the frame builder, the command encoders, the crew, the presenter, the
/// drivers' own) defaults to the other cores, so none of them shares the game's core and slows it. A field the game's code does
/// not finish in 16.7 ms is a frame the game drops, and the code slows when other threads share its core: in the Annihilation
/// Nation arena's fight (the player's save, Counting Ducks) Windows had packed the process's threads onto five of the eight
/// cores, two to a core, and the game's code took 10.2 ms a field, 11.9 ms with twelve busy threads beside it. With the core
/// kept, two runs each way (23 November 2026): fields over 16.7 ms 24 and 26 before, 12 and 11 after; the game's frames dropped
/// 23 and 24, then 14 and 13; the presenter's pictures as they were. Raising the game's priority instead made it worse (452
/// such fields): it waits on the others.
///
/// On a PC with cores of two classes (Intel's performance and efficiency cores), the other threads take the fastest class's
/// other cores alone when it has `FAST_CORES` of them, leaving the slower cores to the rest of the PC: the frame builder and its
/// helpers each wait on the slowest of them, and the v0.1.4 player's i7-13700K (8 performance cores, 8 efficiency cores) built a
/// Florana frame (DRAW DISTANCE FAR, an option since removed) in 15 to 26 ms where this PC's one class of cores takes 8 to 10, every picture after
/// the GPU late with it. A PC with fewer such cores keeps them all, as one with one class does.
///
/// A PC with fewer than `LEAST_OTHER_CORES` cores besides the game's keeps none: every thread may run on every core.
#[cfg(windows)]
fn keep_game_core() -> Result<(), String> {
    use windows::Win32::System::SystemInformation::{CpuSetInformation, GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION};
    use windows::Win32::System::Threading::{GetCurrentProcess, SetProcessDefaultCpuSets};
    let mut len = 0u32;
    // SAFETY: the size query, with no buffer; it fails with the size needed.
    let _ = unsafe { GetSystemCpuSetInformation(None, 0, &mut len, None, None) };
    let mut buf = vec![0u64; (len as usize).div_ceil(8)];
    // SAFETY: the buffer holds `len` bytes at the entries' alignment.
    unsafe { GetSystemCpuSetInformation(Some(buf.as_mut_ptr().cast()), len, &mut len, None, None) }.ok().map_err(|e| format!("play: the PC's CPU sets: {e}"))?;
    // (efficiency class, group, core, CPU set id) of each logical processor.
    let mut sets = Vec::new();
    let (bytes, mut at) = (buf.as_ptr().cast::<u8>(), 0usize);
    while at < len as usize {
        // SAFETY: each entry lies in the `len` bytes written; its Size is the step to the next.
        let e = unsafe { &*bytes.add(at).cast::<SYSTEM_CPU_SET_INFORMATION>() };
        if e.Type == CpuSetInformation {
            // SAFETY: the CpuSet arm is the one an entry of this type holds.
            let c = unsafe { e.Anonymous.CpuSet };
            sets.push((c.EfficiencyClass, c.Group, c.CoreIndex, c.Id));
        }
        at += e.Size as usize;
    }
    let &(class, group, core, _) = sets.iter().max().ok_or("play: the PC lists no CPU sets")?;
    let (own, rest): (Vec<&(u8, u16, u8, u32)>, Vec<_>) = sets.iter().partition(|s| (s.0, s.1, s.2) == (class, group, core));
    let mut rest_cores: Vec<(u16, u8)> = rest.iter().map(|s| (s.1, s.2)).collect();
    rest_cores.sort_unstable();
    rest_cores.dedup();
    if rest_cores.len() < LEAST_OTHER_CORES {
        println!("play: the PC's {} cores ({} logical processors) are every thread's: a core is kept for the game's threads only with {LEAST_OTHER_CORES} more besides it", rest_cores.len() + 1, sets.len());
        return Ok(());
    }
    // The fastest class's other cores, when the PC has a slower class and enough of them (`FAST_CORES`).
    let fast: Vec<&(u8, u16, u8, u32)> = rest.iter().copied().filter(|s| s.0 == class).collect();
    let mut fast_cores: Vec<(u16, u8)> = fast.iter().map(|s| (s.1, s.2)).collect();
    fast_cores.sort_unstable();
    fast_cores.dedup();
    let slower = rest.len() - fast.len();
    let others = if slower > 0 && fast_cores.len() >= FAST_CORES { &fast } else { &rest };
    let own: Vec<u32> = own.iter().map(|s| s.3).collect();
    let others: Vec<u32> = others.iter().map(|s| s.3).collect();
    // SAFETY: the process's own handle and CPU set ids the system listed.
    unsafe { SetProcessDefaultCpuSets(GetCurrentProcess(), Some(&others)) }.ok().map_err(|e| format!("play: the process's CPU sets: {e}"))?;
    let of = match others.len() < rest.len() {
        true => format!(" of its fastest cores ({slower} of slower cores left to the rest of the PC)"),
        false if slower > 0 => format!(" ({slower} of them of slower cores: {} faster cores besides the game's, fewer than {FAST_CORES})", fast_cores.len()),
        false => String::new(),
    };
    println!("play: the game's threads have core {core} (CPU sets {own:?}) to themselves; the other threads the other {} logical processors{of}", others.len());
    GAME_CORE.set(own).map_err(|_| "play: the game's core kept twice")?;
    uyart::kernel::THREAD_START.set(enter_game_core).map_err(|_| "play: the game threads' start set twice".into())
}

/// The calling thread (the host's field loop, or a game thread's host thread) runs on the core kept for the game's threads; on
/// a PC that keeps none (`LEAST_OTHER_CORES`), on any core.
#[cfg(windows)]
fn enter_game_core() {
    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadSelectedCpuSets};
    let Some(ids) = GAME_CORE.get() else { return };
    // SAFETY: the calling thread's own handle and CPU set ids the system listed.
    unsafe { SetThreadSelectedCpuSets(GetCurrentThread(), ids) }.ok().expect("a thread of the game onto the game's core");
}

impl App {
    /// Opens the window on its monitor and starts the game and the renderer.
    fn start(&mut self, el: &ActiveEventLoop) -> Result<(), String> {
        let monitors: Vec<_> = el.available_monitors().collect();
        // The window's monitor: --monitor's, else the one the window was on when the game last ended (the profile's
        // `window.txt`), else the main monitor.
        let placed = Placement::load(&self.o.profile)?;
        let last = placed.as_ref().and_then(|p| monitors.iter().find(|m| m.name().as_deref() == Some(p.monitor.as_str())));
        let monitor = match self.o.monitor {
            Some(i) => monitors.get(i).cloned().ok_or_else(|| format!("play: --monitor {i}: {} monitors", monitors.len()))?,
            None => match last {
                Some(m) => m.clone(),
                None => el.primary_monitor().or_else(|| monitors.first().cloned()).ok_or("play: no monitor")?,
            },
        };
        // The player's settings (`settings.rs`): the display mode, and the resolution unless --height names one.
        let file = format!("{}/settings.txt", self.o.profile);
        NEW_PROFILE.store(!std::path::Path::new(&file).exists() && self.o.height.is_none(), Ordering::Relaxed);
        let settings = crate::settings::Settings::load(&file)?;
        // The update check (`update.rs`), in a thread of its own while the game starts.
        crate::update::start(&self.o.profile, settings.get("update_check") == 1);
        // The native picture's height; its width is the shape of what it is shown on (PLAN 4.2): the monitor's in fullscreen, and
        // the window, which opens at the monitor's shape, otherwise (`uyart::set_view_k`, every frame).
        let h = self.o.height.unwrap_or(settings.height());
        // Windowed: where the window was when the game last ended, if that is on this monitor; else the picture's size centred
        // on it. Fullscreen: borderless over the whole monitor.
        let (size, at) = match &placed {
            Some(p) if on_monitor(p.at, p.size, &monitor) => (p.size, p.at),
            _ => centred(h, &monitor),
        };
        let (mp, ms) = (monitor.position(), monitor.size());
        // --aspect W:H: a window of that shape instead, the picture's size where the monitor holds it (else as large as it allows),
        // centred on it.
        let (size, at) = match self.o.aspect {
            Some((aw, ah)) => {
                let w = (h as f32 * aw / ah).round() as u32;
                let fit = (ms.width as f32 * 0.95 / w as f32).min(ms.height as f32 * 0.9 / h as f32).min(1.0);
                let (ww, wh) = ((w as f32 * fit) as u32, (h as f32 * fit) as u32);
                (PhysicalSize::new(ww, wh), PhysicalPosition::new(mp.x + (ms.width as i32 - ww as i32) / 2, mp.y + (ms.height as i32 - wh as i32) / 2))
            }
            None => (size, at),
        };
        println!("play: pictures {h} lines, window {}x{} at {},{} {:?} on {} ({}x{} at {},{})", size.width, size.height, at.x, at.y, settings.display(), monitor.name().unwrap_or_default(), ms.width, ms.height, mp.x, mp.y);
        // API-driven checks can render without occupying the player's display.
        let hidden = self.o.control && std::env::var_os("UYA_CONTROL_HIDDEN").is_some();
        self.fullscreen = !hidden && settings.display() == crate::settings::Display::Fullscreen;
        let full = self.fullscreen.then(|| winit::window::Fullscreen::Borderless(Some(monitor.clone())));
        let attrs = Window::default_attributes().with_title("Ratchet & Clank: Up Your Arsenal").with_window_icon(Some(crate::icon::portrait(64)?)).with_inner_size(size).with_position(at).with_fullscreen(full).with_visible(false).with_active(!hidden);
        #[cfg(windows)]
        let attrs = {
            use winit::platform::windows::WindowAttributesExtWindows;
            attrs.with_taskbar_icon(Some(crate::icon::portrait(128)?))
        };
        let window = Arc::new(el.create_window(attrs).map_err(|e| format!("play: window: {e}"))?);
        self.window = Some(window.clone());
        // The game's view at the window's shape from the first frame on (`Screen::acquire` sets it again at each picture shown):
        // the frames drawn before the first picture was shown took the original shape (a boot's first picture at 16:9 was
        // 697x540, the next 960x540).
        let s = window.inner_size();
        if s.width > 0 && s.height > 0 {
            uyart::set_view_k(if settings.wide() { s.width as f32 / (s.height as f32 * 0.62 / 0.4805) } else { 1.0 });
        }
        self.windowed = (size, at);
        // winit gives the window's handles only on this thread: the renderer makes its surface from them (it holds the window,
        // so they stay valid as long as the surface).
        let handles = {
            use winit::raw_window_handle::{HasDisplayHandle, HasWindowHandle};
            Handles(window.display_handle().map_err(|e| format!("play: window: {e}"))?.as_raw(), window.window_handle().map_err(|e| format!("play: window: {e}"))?.as_raw())
        };
        let input = Input::new(self.o.prompts);
        self.kbm = Some(input.kbm.clone());
        let input = Arc::new(Mutex::new(input));
        // Frames go one at a time to the renderer; the RAM buffers come back to the game thread to be filled again. A frame is taken
        // only when the renderer's walk thread waits for one (`walk_ahead`): it holds the next frame, walked, while the renderer draws
        // the one before, as this channel held it before the walk had a thread of its own.
        let (tx, rx) = std::sync::mpsc::sync_channel::<Msg>(0);
        let (back_tx, back_rx) = std::sync::mpsc::channel::<uyart::RamCopy>();
        // Four: the renderer's, its walk thread's, the game thread's spare, and one with the game's read-back thread (`rtgame`).
        for _ in 0..4 {
            back_tx.send(uyart::RamCopy::new()).expect("the channel is open");
        }
        let (iso, height, proxy, kbm) = (self.iso.clone(), h, self.proxy.clone(), input.lock().expect("input lock").kbm.clone());
        let atlas = format!("{}/prompts.bin", self.o.profile);
        // The fields paced at the console's rate (not --fast or --control): the display's refreshes between them can be drawn.
        let paced_control = self.o.control && std::env::var_os("UYA_CONTROL_PACED").is_some();
        let paced = !self.o.fast && (!self.o.control || paced_control);
        // The HD texture packs in the profile's `texture_replacements`, always (the player: "The HD texture packs should be the
        // only thing used and not a menu option"): each level's replacements are decoded when the level loads (`texnames`).
        crate::texnames::open_in_play(std::path::Path::new(&format!("{}/texture_replacements", self.o.profile)), self.o.rift.as_deref().map(std::path::Path::new))?;
        // The pipeline cache in the profile's folder (`pipelines.rs`), always.
        crate::pipelines::open(std::path::Path::new(&self.o.profile));
        std::thread::Builder::new()
            .name("uya render".into())
            .spawn(move || {
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    render(&iso, height, settings, &atlas, window, handles, rx, back_tx, kbm, paced, (!hidden).then_some(&proxy))
                })).unwrap_or_else(|_| Err("play: the render thread panicked".into()));
                let _ = proxy.send_event(Event::Rendered(r));
            })
            .map_err(|e| format!("play: render thread: {e}"))?;
        self.rendering = true;
        let (iso, presses, proxy, quit) = (self.iso.clone(), self.o.presses.clone(), self.proxy.clone(), self.quit.clone());
        let (mice, fast, start, control) = (self.o.mice.clone(), self.o.fast || (self.o.control && !paced_control), self.o.start, self.o.control);
        // AUDIO > MIXING at the start: the mix's rate is made then (`audio.rs`).
        let audio = crate::audio::Options { device_rate: settings.get("audio_mixing") == 1, ..self.o.audio.clone() };
        let (profile, events, warps) = (self.o.profile.clone(), self.proxy.clone(), self.o.warps.clone());
        // The pointer script: the clicks and the hovers in field order (a click is a hover with the left button).
        let mut pointer: Vec<(u64, f32, f32, bool)> = self.o.clicks.iter().map(|&(f, u, v)| (f, u, v, true)).chain(self.o.hovers.iter().map(|&(f, u, v)| (f, u, v, false))).collect();
        pointer.sort_by_key(|p| p.0);
        // The game's movies in the window (`cinema.rs`); with --fast and no --movies none is attached and the game's player skips
        // each movie as it starts.
        let field = Arc::new(AtomicU64::new(0));
        // A field that does not end: every thread's stack into the log (`sampler::watch`); a slow field: the threads' time in it
        // (`sampler::busy_watch`).
        #[cfg(windows)]
        crate::sampler::watch(field.clone());
        #[cfg(windows)]
        crate::sampler::busy_watch();
        if !(self.o.fast || self.o.control) || self.o.movies {
            crate::cinema::attach(&iso, tx.clone(), input.clone(), quit.clone(), &presses, field.clone());
        }
        std::thread::Builder::new()
            .name("uya game".into())
            .spawn(move || {
                #[cfg(windows)]
                enter_game_core();
                // Publish the game's result before its channels close. Otherwise the walk and render threads
                // can finish normally on disconnect and close the window before the game reports a failure.
                let r = rtgame::game(&iso, (&presses, &pointer, &warps, &mice), (fast, start, control), input, &tx, &back_rx, &quit, &field, settings, &profile, &events, &audio);
                let result = match r {
                    Ok(rtgame::GameEnd::Quit) => Ok(()),
                    Ok(rtgame::GameEnd::RendererStopped) => {
                        crate::cinema::detach();
                        return;
                    }
                    Err(e) => Err(e),
                };
                let _ = proxy.send_event(Event::Ended(result));
                // The movies' hold on the renderer's channel let go: with the game's own (this thread's, at its end) it closes, and
                // the renderer ends (`Event::Rendered`).
                crate::cinema::detach();
            })
            .map_err(|e| format!("play: game thread: {e}"))?;
        Ok(())
    }

    fn end(&mut self, el: &ActiveEventLoop, r: Result<(), String>) {
        self.quit.store(true, Ordering::SeqCst);
        if self.ending.is_none() {
            self.ending = Some(Instant::now());
            // Where the window is, for the next start (`Placement`).
            let saved = self.place().and_then(|p| p.map_or(Ok(()), |p| p.save(&self.o.profile)));
            if self.result.is_none() {
                self.result = Some(r.and(saved));
            }
        } else if self.result.is_none() {
            self.result = Some(r);
        }
        // The renderer takes the GPU, DLSS and Streamline down itself once the game's frames end (`render`'s end), and the process
        // ends after it (`Event::Rendered`), at most ENDING later (`about_to_wait`). Ended while the renderer was at that (or still
        // drawing, with the movies holding its channel open), the drivers' libraries were unloaded under it: the process closed
        // with an access violation after the game had quit, now and then (exit 0xC0000005, DLSS frame generation on).
        if !self.rendering {
            el.exit();
        }
    }

    /// The window's monitor, and its place and size while windowed (None before the window is open, or while Windows reports it
    /// on no monitor: minimised).
    fn place(&self) -> Result<Option<Placement>, String> {
        let Some(w) = &self.window else { return Ok(None) };
        let Some(m) = w.current_monitor() else { return Ok(None) };
        let monitor = m.name().ok_or("play: the window's monitor has no name")?;
        let (size, at) = match self.fullscreen {
            true => self.windowed,
            false => (w.inner_size(), w.outer_position().map_err(|e| format!("play: the window's place: {e}"))?),
        };
        Ok(Some(Placement { monitor, at, size }))
    }

    /// The display mode the player set (`settings.rs`), when it differs from the window's: fullscreen on the monitor the window is
    /// on now, or back to a window there (where it was before it went fullscreen, when that is on this monitor; else centred on
    /// it at its size). Nothing else moves the window: a change of resolution or of any other setting leaves it where it is.
    fn display(&mut self, full: bool) {
        let Some(w) = self.window.clone() else { return };
        if full == self.fullscreen {
            return;
        }
        self.fullscreen = full;
        if full {
            w.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
            return;
        }
        w.set_fullscreen(None);
        if let Some(m) = w.current_monitor().filter(|m| !on_monitor(self.windowed.1, self.windowed.0, m)) {
            let (mp, ms) = (m.position(), m.size());
            let size = PhysicalSize::new(self.windowed.0.width.min(ms.width), self.windowed.0.height.min(ms.height));
            self.windowed = (size, PhysicalPosition::new(mp.x + (ms.width as i32 - size.width as i32) / 2, mp.y + (ms.height as i32 - size.height as i32) / 2));
        }
        let _ = w.request_inner_size(self.windowed.0);
        w.set_outer_position(self.windowed.1);
    }

    /// The cursor is held in the window and hidden while the window has the focus and the game is played (the mouse turns the
    /// camera); shown, still held in the window, while the quick-select ring is open; in the menus it is shown and free.
    fn grab(&self) {
        let on = self.focused && !self.menu;
        if let Some(w) = &self.window {
            let _ = w.set_cursor_grab(if on { CursorGrabMode::Confined } else { CursorGrabMode::None });
            w.set_cursor_visible(!on);
        }
    }
}

impl ApplicationHandler<Event> for App {
    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.window.is_none() {
            if let Err(e) = self.start(el) {
                self.end(el, Err(e));
            }
        }
    }

    fn user_event(&mut self, el: &ActiveEventLoop, e: Event) {
        match e {
            Event::Ready => {
                if let Some(window) = &self.window {
                    window.set_visible(true);
                }
            }
            Event::Ended(r) => self.end(el, r),
            Event::Rendered(r) => {
                self.rendering = false;
                self.end(el, r);
            }
            Event::Menu(m) => {
                self.menu = m;
                self.grab();
            }
            Event::Settings(s) => self.display(s.display() == crate::settings::Display::Fullscreen),
        }
    }

    fn window_event(&mut self, el: &ActiveEventLoop, _: WindowId, e: WindowEvent) {
        let Some(kbm) = &self.kbm else { return };
        match e {
            WindowEvent::CloseRequested => self.end(el, Ok(())),
            WindowEvent::Focused(f) => {
                self.focused = f;
                self.grab();
                if !f {
                    // Nothing stays held while the window is away.
                    let mut k = kbm.lock().expect("input lock");
                    k.keys.clear();
                    k.pressed.clear();
                    k.mouse = [false; 5];
                    k.mouse_pressed = [false; 5];
                }
            }
            // The window's place and size as the player moves and sizes it (not while fullscreen): what fullscreen gives back and
            // the next start opens at.
            WindowEvent::Moved(at) if !self.fullscreen => self.windowed.1 = at,
            WindowEvent::Resized(size) if !self.fullscreen && size.width > 0 && size.height > 0 => self.windowed.0 = size,
            WindowEvent::KeyboardInput { event, .. } => {
                if let PhysicalKey::Code(code) = event.physical_key {
                    let mut k = kbm.lock().expect("input lock");
                    if event.state == ElementState::Pressed {
                        if !event.repeat {
                            k.touched = true;
                            // A tap shorter than a field still reaches the game (`Kbm::pressed`).
                            k.pressed.insert(code);
                        }
                        k.keys.insert(code);
                    } else {
                        k.keys.remove(&code);
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                // Notches up are positive; a touchpad's pixels count a notch for every 40.
                let lines = match delta {
                    winit::event::MouseScrollDelta::LineDelta(_, y) => y as f64,
                    winit::event::MouseScrollDelta::PixelDelta(p) => p.y / 40.0,
                };
                let mut k = kbm.lock().expect("input lock");
                k.wheel += lines;
                k.touched = true;
            }
            WindowEvent::CursorMoved { position, .. } => {
                let mut k = kbm.lock().expect("input lock");
                k.cursor = Some((position.x, position.y));
                k.moved = true;
            }
            WindowEvent::CursorLeft { .. } => kbm.lock().expect("input lock").cursor = None,
            WindowEvent::MouseInput { state, button, .. } => {
                let i = match button {
                    MouseButton::Left => 0,
                    MouseButton::Right => 1,
                    MouseButton::Middle => 2,
                    MouseButton::Back => 3,
                    MouseButton::Forward => 4,
                    MouseButton::Other(_) => return,
                };
                let mut k = kbm.lock().expect("input lock");
                k.mouse[i] = state == ElementState::Pressed;
                if state == ElementState::Pressed {
                    k.touched = true;
                    k.click |= i == 0;
                    // A click shorter than a field still reaches the game (`Kbm::pressed`).
                    k.mouse_pressed[i] = true;
                }
            }
            _ => {}
        }
    }

    fn device_event(&mut self, _: &ActiveEventLoop, _: DeviceId, e: DeviceEvent) {
        if let (DeviceEvent::MouseMotion { delta }, Some(kbm), true) = (e, &self.kbm, self.focused) {
            let mut k = kbm.lock().expect("input lock");
            k.motion.0 += delta.0;
            k.motion.1 += delta.1;
        }
    }

    fn about_to_wait(&mut self, el: &ActiveEventLoop) {
        if let Some(t) = self.ending {
            if t.elapsed() >= ENDING {
                println!("play: the renderer did not stop within {} s of the game's end: the process ends without it", ENDING.as_secs());
                el.exit();
            } else {
                el.set_control_flow(ControlFlow::WaitUntil(t + ENDING));
            }
            return;
        }
        match self.o.seconds {
            Some(s) if self.t0.elapsed().as_secs_f64() >= s => {
                println!("play: {s} s: quit");
                self.end(el, Ok(()));
            }
            Some(s) => el.set_control_flow(ControlFlow::WaitUntil(self.t0 + Duration::from_secs_f64(s))),
            None => el.set_control_flow(ControlFlow::Wait),
        }
    }
}

/// A new profile (no settings.txt when the game started, no --height): once the renderer knows its GPU, an integrated one
/// starts at 720P (`INTEGRATED_720`, which the game thread applies as the player's own change of the setting would be applied,
/// saved and shown on the settings page: `rtgame.rs`). 1080 was more than such a GPU draws at 60 (a v0.1.3 player's Ryzen 5
/// 4600G drew Veldin at 20 to 22 pictures a second at 1080), and the picture is brought up to the window edge-directed
/// (`Screen::window_copy`). The renderer's own adapter is asked, not one found before it: Streamline must choose the Vulkan
/// loader before any instance is made (`streamline::load`).
pub(crate) static NEW_PROFILE: AtomicBool = AtomicBool::new(false);
pub(crate) static INTEGRATED_720: AtomicBool = AtomicBool::new(false);

/// The renderer thread: each frame drawn and handed to the presenter (`present`), which shows it in the window.
#[allow(clippy::too_many_arguments)]
/// The renderer's walk thread: each frame the renderer draws (a field that sends the GS something) walked (`native::Walker`) before
/// it is handed over, so that its walk is done while the renderer draws the frame before; every other message handed over in its
/// place among them. Its jobs are built here first, its draw lists read from its RAM copy (`jobs::Kicked`), and handed over
/// with the walk. `spare`: the primitives' room of the frames drawn, for the next walks.
fn walk_ahead(rx: Receiver<Msg>, out: std::sync::mpsc::SyncSender<(Msg, Option<Result<(crate::jobs::FrameJobs, crate::native::Walked), String>>)>, mut walker: crate::native::Walker, spare: Receiver<Vec<(usize, crate::mobies::EePrim)>>) {
    let mut buf = Vec::new();
    while let Ok(mut msg) = rx.recv() {
        let walked = match &mut msg {
            Msg::Field(fr) if !(fr.jobs.is_empty() && fr.uploads.is_empty()) => {
                while let Ok(b) = spare.try_recv() {
                    buf = b;
                }
                let front = fr.front_end;
                Some(match std::mem::take(&mut fr.jobs).into_frame(&fr.ram) {
                    Ok(fj) => walker.walk(&fj, front, if front { Vec::new() } else { std::mem::take(&mut buf) }).map(|w| (fj, w)),
                    Err(e) => Err(e),
                })
            }
            _ => None,
        };
        if out.send((msg, walked)).is_err() {
            return;
        }
    }
}

fn render(iso: &str, height: u32, settings: crate::settings::Settings, atlas: &str, window: Arc<Window>, handles: Handles, rx: Receiver<Msg>, back: std::sync::mpsc::Sender<uyart::RamCopy>, kbm: Arc<Mutex<crate::input::Kbm>>, paced: bool, reveal: Option<&EventLoopProxy<Event>>) -> Result<(), String> {
    // The graphics options as the profile has them (`enhance`), and the settings last applied.
    crate::enhance::set(&settings);
    WIDE.store(settings.wide(), Ordering::Relaxed);
    let mut applied = settings;
    let mut native = crate::native::Native::open(iso, height)?;
    let gpu = native.gpu().1.get_info();
    if NEW_PROFILE.load(Ordering::Relaxed) && gpu.device_type == wgpu::DeviceType::IntegratedGpu {
        println!("play: {} is an integrated GPU: a new profile's RESOLUTION is 720P", gpu.name);
        INTEGRATED_720.store(true, Ordering::Relaxed);
    }
    // The options again where this GPU has no DLSS (`dlss::unavailable`, known now the device is made): DLSS off, and with it the
    // ray-traced options (`Settings::constrain`), whatever the profile's file says (a profile brought from an RTX PC).
    let mut kept = settings;
    kept.constrain();
    crate::enhance::set(&kept);
    println!("play: settings {kept}");
    native.quiet = true;
    let mut atlas = std::sync::Arc::new(crate::prompts::Atlas::read(atlas)?);
    // The save slots' pictures, from the disc (`uyafmt::wad38::slot_pictures`).
    let pictures = Arc::new(uyafmt::wad38::slot_pictures(iso)?);
    // The game's text in the vector font (`text.rs`), from the main program's font tables.
    let boot = crate::boot_image(iso)?;
    let text = std::sync::Arc::new(crate::text::Text::new(&boot)?);
    // The display's refresh rate (the window's monitor's): above the game's 59.94 fields a second, with the fields paced, every
    // refresh shows a picture between two of the game's frames (PLAN 8.8, `replay.rs`, `Timeline`); else each of the game's frames
    // is shown as it is drawn.
    // The refreshes are the window's display's own, waited for (`VBlank`): a FIFO surface follows the main display's (on this PC
    // 180 Hz beside the game's 100 Hz one) when they differ.
    let monitor = window.current_monitor();
    let hz = monitor.as_ref().and_then(|m| m.refresh_rate_millihertz()).map(|m| m as f64 / 1000.0);
    let name = monitor.as_ref().and_then(|m| m.name()).unwrap_or_default();
    let vblank = match (paced, hz) {
        (true, Some(h)) if h > 61.0 => match VBlank::open(&name) {
            Ok(v) => Some(v),
            Err(e) => {
                println!("play: {e}: each of the game's frames shown as it is drawn");
                None
            }
        },
        _ => None,
    };
    let between = vblank.is_some();
    native.replays = between;
    // With the fields paced the presenter submits every frame's world itself, in the frames' order (`Plain`): on the plain path a
    // picture's window copy then goes on the GPU before the next picture's world, never behind it.
    native.defer_world = paced;
    let mut screen = Screen::new(&native, window, handles, kbm.clone(), false)?;
    if let Some(events) = reveal {
        // Cover the entire client area before Windows exposes the window at its final size.
        // Some compositors defer acquisition for hidden surfaces. Show the configured window there as well.
        if let Some(out) = screen.acquire()? {
            let view = out.texture.create_view(&Default::default());
            let mut enc = screen.device.create_command_encoder(&Default::default());
            enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("startup black"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                ..Default::default()
            });
            screen.queue.submit([enc.finish()]);
            screen.window.pre_present_notify();
            screen.queue.present(out);
        }
        let _ = events.send_event(Event::Ready);
    }
    // Every pipeline is made (the renderer's, the window's): the cache written for the next start.
    crate::pipelines::save()?;
    println!("play: {name} runs at {}: {}", hz.map_or("an unknown rate".to_string(), |h| format!("{h:.2} Hz")), if between { "a picture at every refresh, between two of the game's frames" } else { "each of the game's frames as it is drawn" });
    // One frame waits for the presenter at most: each holds a frame set, and the next frame is built while it waits.
    let (show, shows) = std::sync::mpsc::sync_channel::<Show>(1);
    let presenter = std::thread::Builder::new().name("uya present".into()).spawn(move || present(screen, shows, hz, vblank, paced)).map_err(|e| format!("play: present thread: {e}"))?;
    // A message the presenter cannot take: it stopped, with its reason.
    let stopped = |presenter: std::thread::JoinHandle<Result<(), String>>| -> String {
        match presenter.join() {
            Ok(Err(e)) => e,
            Ok(Ok(())) => "play: the presenter stopped".into(),
            Err(_) => "play: the presenter panicked".into(),
        }
    };
    // Each frame drawn is walked on a thread of its own, a frame ahead (`walk_ahead`), and handed over walked; the primitives' room
    // goes back to it once the frame is drawn.
    let (ahead_tx, ahead) = std::sync::mpsc::sync_channel::<(Msg, Option<Result<(crate::jobs::FrameJobs, crate::native::Walked), String>>)>(0);
    let (spare_tx, spare_rx) = std::sync::mpsc::channel::<Vec<(usize, crate::mobies::EePrim)>>();
    let walker = native.take_walker();
    let walk_thread = std::thread::Builder::new().name("uya walk".into()).spawn(move || walk_ahead(rx, ahead_tx, walker, spare_rx)).map_err(|e| format!("play: walk thread: {e}"))?;
    let (t0, mut ns, mut n, mut handed_ns) = (Instant::now(), 0u64, 0u64, 0u64);
    // What the last frame was drawn as (logged when it changes).
    let mut path = "";
    while let Ok((msg, walked)) = ahead.recv() {
        let mut fr = match msg {
            Msg::Field(fr) => fr,
            Msg::Drawn(reply) => {
                if show.send(Show::Drawn(reply)).is_err() {
                    return Err(stopped(presenter));
                }
                continue;
            }
            Msg::Shot(file, reply) => {
                if show.send(Show::Shot(file, reply)).is_err() {
                    return Err(stopped(presenter));
                }
                continue;
            }
            Msg::Movie(pic) => {
                if show.send(Show::Frame { field: None, build_ms: 0.0, shown: Shown::Picture(pic), shape: native.size(), replay: None, set: None, outbox: native.take_outbox() }).is_err() {
                    return Err(stopped(presenter));
                }
                continue;
            }
            // The settings changed: the graphics options from the next frame; a new resolution, another texture filtering (the
            // level's materials hold their samplers) or the Rift Apart surfaces turned on or off (the level's replacements are
            // uploaded with it) makes the level's GPU state again.
            Msg::Settings(mut s) => {
                s.constrain();
                crate::enhance::set(&s);
                WIDE.store(s.wide(), Ordering::Relaxed);
                if s.height() != applied.height() {
                    native.resize(s.height());
                    println!("play: pictures {}x{} now", native.size().0, native.size().1);
                } else if s.get("texture_filtering") != applied.get("texture_filtering") || s.get("rift_apart_surfaces") != applied.get("rift_apart_surfaces") {
                    native.reload();
                }
                applied = s;
                continue;
            }
        };
        let t = Instant::now();
        // Busy with the frame until it is handed to the presenter (`renderer_held`).
        RENDER_BUSY.store(clock(), Ordering::Relaxed);
        native.take_uploads(std::mem::take(&mut fr.uploads_before));
        // A field that sends the GS nothing (no jobs, no uploads) leaves the PS2's frame buffer as it was: the picture shown stays.
        // The quit question draws itself on one field in five and sends nothing on the others; drawn as empty frames, those showed
        // the bare world four fields in five, the question flashing over it. The walk thread walks every other (`walk_ahead`).
        let Some(walked) = walked else {
            RENDER_BUSY.store(0, Ordering::Relaxed);
            back.send(fr.ram).map_err(|_| "play: the game stopped")?;
            continue;
        };
        // The level whose world the frame's RAM holds: the level whose code was in RAM when the frame's RAM was copied
        // (`uyart::RamCopy::level`). While a level change fades the old level out, `level_current` already names the new one
        // and its .bss still holds the old level's memory, until its start clears it: leaving Qwark's hideout for the Phoenix
        // (19 to 6), the old level's word cdcdcdcd stood where the Phoenix keeps its HUD header's copy, read as an address. A front
        // end's frame names the level being entered, if any (`level_current`).
        let level = if fr.front_end {
            crate::level_current(&fr.ram)
        } else {
            fr.ram.level().ok_or_else(|| format!("play: field {}: a level's frame, and no story level's code in RAM", fr.field))?
        };
        // The prompt icons (`prompts.rs`), from the atlas the profile holds: the keyboard's left stick shows the move keys bound
        // (the atlas's cell for it drawn again when they change).
        if atlas.moves() != fr.bindings.moves() {
            atlas = Arc::new(atlas.with_moves(fr.bindings.moves()));
        }
        // The answers a native frame's labels offer (`prompts::prepare`), for the game thread.
        let native_answers = Arc::new(std::sync::Mutex::new(Vec::new()));
        let slots = crate::menus::slot_pictures(&fr.ram);
        native.prompts = Some(crate::prompts::Look { device: fr.prompts, atlas: atlas.clone(), text: text.clone(), clicks: fr.menu && fr.prompts == Device::Keyboard, lines: Arc::new(fr.lines), pointer: fr.pointer, answers: native_answers.clone(), bindings: fr.bindings, pictures: pictures.clone(), slots });
        // A paused level is drawn natively as any frame of it is: the field in which the game pauses carries the world and reads the
        // picture back (a download of the main frame: the native picture is grabbed, `Native::render`), and each pause field after
        // it puts that picture back into the main frame (an IMAGE of the grabbed pages) and draws its menus over it (the player,
        // 19 November 2026: "Remove all the old 2D layer/ps2 stuff").
        // A front end's frame (its menus, the 3D its VU1 programs draw, a trip's or an arrival's space scene) is drawn natively as
        // a level's is, from its jobs as the GS draws them (`Native::render_screen`; the player, 19 November 2026: "Remove all the
        // old 2D layer/ps2 stuff").
        // A front end's frame while a level is entered, once the front end shows that level's arrival in space (the load screen,
        // from the level loader's step 2: `LOAD_STEP`), is drawn across the picture's width. Before it, from the save slot
        // NEW GAME writes or LOAD GAME reads to the screen's fade to black, the frames are the slot screen's, the level already
        // named: drawn in the load screen's place and shape they zoomed in (the player: "The load game zoom in issue also shows
        // up on new game", "the load screen zoom happens too early").
        let arrival = !matches!(level, 0 | u32::MAX) && crate::load_step(&fr.ram) >= 2;
        // A front end's frame decodes what it draws first where it is asked for (`texnames::FRONT`).
        crate::texnames::front_end(fr.front_end);
        let picture = match walked {
            Ok((fj, w)) if fr.front_end => native.render_screen_walked(&fr.ram, &fj, arrival, w),
            Ok((fj, w)) => native.render_walked(&fr.ram, &fj, level, &fr.read_back, w),
            Err(e) => Err(e),
        };
        // A level's first frame since its load that drew the world is drawn, every replacement it and the fade in's frames before
        // it draw decoded for them (`texnames::FIRST`): from the next frame on, textures first drawn go to the background worker.
        if !fr.front_end && picture.is_ok() && native.drew_world() {
            crate::texnames::first_drawn(level);
        }
        spare_tx.send(native.take_ee_spare()).map_err(|_| "play: the walk thread stopped")?;
        let shown = match picture {
            Ok(t) => {
                kbm.lock().expect("input lock").answers = (fr.field, std::mem::take(&mut *native_answers.lock().expect("answers lock")));
                Shown::Gpu(t)
            }
            Err(e) => {
                // A level that cannot be loaded (a pack file that cannot be read or decoded among others) stops the game, naming why.
                if let Some(b) = native.broken() {
                    return Err(format!("play: field {}: level {level}: {b}", fr.field));
                }
                // A frame the native renderer cannot draw stops the game, naming why: no other layer draws it (the player, 19
                // November 2026: "Every bug is always because of a fallback. Why do we have any?").
                return Err(format!("play: field {}: level {level}: {e}", fr.field));
            }
        };
        native.take_uploads(std::mem::take(&mut fr.uploads));
        if std::env::var("UYA_GSM_DUMP_AT").is_ok_and(|v| v.split(',').any(|f| f.parse() == Ok(fr.field))) {
            native.dump_gsm(&format!("gsm{:05}.bin", fr.field))?;
        }
        let kind = match &shown {
            Shown::Gpu(_) if fr.front_end => "the front end's native picture",
            Shown::Gpu(_) => "the native picture",
            Shown::Picture(_) => "a movie",
        };
        if kind != path {
            println!("play: field {}: level {level}: {kind}", fr.field);
            path = kind;
        }
        back.send(fr.ram).map_err(|_| "play: the game stopped")?;
        // A frame that drew the world leaves what drawing it again needs (`Native::take_replay`): the presenter draws the pictures
        // between it and the frame before from it.
        let replay = match &shown {
            Shown::Gpu(_) => native.take_replay(),
            _ => None,
        };
        // The frame set the picture is in, held until the picture is shown (`Native::take_set`).
        let set = native.take_set();
        let prepared_ms = t.elapsed().as_secs_f64() * 1e3;
        // What the frame changes in the shared GPU state, and a frame drawn as it was prepared: for the presenter, in order.
        let outbox = native.take_outbox();
        let handing = Instant::now();
        if show.send(Show::Frame { field: Some((fr.field, fr.at, handing)), build_ms: prepared_ms, shown, shape: native.size(), replay, set, outbox }).is_err() {
            return Err(stopped(presenter));
        }
        let handed = handing.elapsed();
        handed_ns += handed.as_nanos() as u64;
        RENDER_BUSY.store(0, Ordering::Relaxed);
        frame_drawn(t.elapsed().as_nanos() as u64);
        if frame_times() { println!("frame stages: render {} prepare {:.3} send {:.3}", fr.field, prepared_ms, t.elapsed().as_secs_f64() * 1e3 - prepared_ms); }
        ns += t.elapsed().as_nanos() as u64;
        diag_fold(&DIAG_FRAME, t.elapsed().saturating_sub(handed).as_secs_f64() * 1e3);
        n += 1;
        if fr.field % 300 == 0 {
            let ee = native.ee_census().map_or(String::new(), |c| format!("; the level's EE draws {c}"));
            // Of the time a frame takes: the builder's own work, and the wait to hand the frame to the presenter (which takes
            // the next only when it has shown the one before, and that waits for the GPU); and, where the presenter shows each
            // frame as it comes, the GPU's time a picture and the presenter's (the builder is held by the GPU when the wait
            // approaches the GPU's time, and by its own work when the wait is near 0).
            let per = |ns: u64| ns as f64 / 1e6 / n as f64;
            let plain = match (PLAIN_GPU.lock().expect("GPU time lock").take(), PLAIN_SHOWN.lock().expect("presenter time lock").take()) {
                (Some(g), Some(p)) => format!("; the GPU {:.1} ms a picture (at most {:.1}), the presenter {:.1} ms (at most {:.1})", g.0, g.1, p.0, p.1),
                _ => String::new(),
            };
            println!("play: field {}, {:.1} s: drawn in {:.1} ms a frame (the builder's own work {:.1} ms, the wait to hand the frame over {:.1} ms){plain}{ee}", fr.field, t0.elapsed().as_secs_f64(), per(ns), per(ns - handed_ns), per(handed_ns));
            // The figures FPS COUNTER's DETAILED shows, whatever the option, so a report's log has them.
            let (vram, budget) = crate::enhance::vram()?;
            println!(
                "play: field {}: diagnostics: {} pictures a second; the game's own thread {:.1} ms a field, the frame builder {:.1} ms a frame, the GPU {:.1} ms a picture; video memory {vram:.0} MB used of a {budget:.0} MB budget; {:.0} MB of memory in use, {:.0} MB private",
                fr.field,
                DIAG_FPS.load(Ordering::Relaxed),
                diag(&DIAG_GAME),
                diag(&DIAG_FRAME),
                diag(&DIAG_GPU),
                crate::native::working_set_mb(),
                crate::native::private_mb()
            );
            if let Some(m) = crate::native::mem_log(native.gpu().2) {
                println!("play: field {}: {m}", fr.field);
            }
            (ns, n, handed_ns) = (0, 0, 0);
        }
    }
    walk_thread.join().map_err(|_| "play: the walk thread panicked")?;
    drop(show);
    let r = match presenter.join() {
        Ok(r) => r,
        Err(_) => Err("play: the presenter panicked".into()),
    };
    // Streamline shut down with the window gone (the presenter's) and the device still there (`native`'s), its plugins freeing
    // what they made on it. The shutdown unloads them, and the device's release goes through their Vulkan hooks: released after
    // it, the render thread called into code no longer there ("crash: exception 0xc0000005 at ... (in no module), executing ...,
    // on uya render", after "streamline: shut down", on every quit with DLSS frame generation once the process waited for this
    // end). The process is ending: the device and what it made are left to it.
    crate::streamline::shutdown();
    std::mem::forget(native);
    r
}

/// What the renderer hands the presenter: a frame's picture (a field's, with the field and when the game thread began it, and
/// with its replay when it drew the world: `replay.rs`; or a movie's picture), at the native pictures' size `shape`; the control
/// channel's `shot` and `Drawn`, after every frame sent before them.
enum Show {
    /// `field`: the field, when the game thread began it and when the renderer handed it over; `build_ms`, the renderer's time on
    /// it until then.
    /// `set`: the frame set the picture belongs to (`Native::take_set`), held while it may be shown.
    /// `outbox`: the frame's changes to the shared GPU state, and its commands when it was drawn as it was prepared
    /// (`native::Outbox`), applied by the presenter in the frames' order.
    Frame { field: Option<(u64, Instant, Instant)>, build_ms: f64, shown: Shown, shape: (u32, u32), replay: Option<crate::replay::Replay>, set: Option<Arc<()>>, outbox: crate::native::Outbox },
    Shot(String, std::sync::mpsc::Sender<Result<(), String>>),
    Drawn(std::sync::mpsc::Sender<()>),
}

/// The presenter thread: shows each frame in the window as it comes (with the fields paced, at the plain path's steady pace:
/// `Plain`), or, with the display's vertical blank (`render`), puts the frames that drew the world on the display's timeline
/// (`Timeline`), which keeps one picture's world on the GPU while it records the next.
fn present(mut screen: Screen, shows: Receiver<Show>, hz: Option<f64>, vblank: Option<VBlank>, paced: bool) -> Result<(), String> {
    // The picture shown last, for `shot` while no timeline runs.
    let mut last: Option<(Shown, (u32, u32), Option<Arc<()>>)> = None;
    let between = vblank.is_some();
    // The presenter is the one thread whose lateness shows: it runs above the others (a refresh missed shows the picture before
    // twice).
    #[cfg(windows)]
    if between {
        use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
        // SAFETY: the calling thread's own handle.
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) }.map_err(|e| format!("play: the presenter's priority: {e}"))?;
    }
    let mut tl = Timeline::new(hz.unwrap_or(60.0), vblank, &screen.device, &screen.queue)?;
    // Each frame as it comes, the fields paced: at a steady pace (`Plain`), its window copy made as soon as its world is done and
    // presented at its time on a thread of its own (`Presents`), the window holding a picture for each one waiting.
    let mut plain = (!between && paced).then(Plain::new);
    let mut presents = match plain {
        Some(_) => Some(Presents::start(screen.window.clone(), screen.queue.clone())?),
        None => None,
    };
    // The GPU's time a picture, where the presenter shows each frame as it comes (`GpuClock`): from when the frame's world is
    // submitted (by the presenter with the fields paced, by the renderer before it hands the frame over without).
    let gpu_clock = if between { None } else { Some(GpuClock::start(screen.device.clone(), screen.queue.clone())?) };
    // UYA_FG_PACE=N: the presenter paced as if DLSS-G generated N frames per picture (`FG_GENERATED`), with none generated (an
    // instrument: the pacing checked apart from Streamline).
    if let Ok(v) = std::env::var("UYA_FG_PACE") {
        FG_GENERATED.store(v.parse().map_err(|_| format!("UYA_FG_PACE={v}: a number of frames"))?, Ordering::Relaxed);
    }
    loop {
        // While the timeline runs: the picture on the GPU finished (waited for a millisecond at most) or, with none to wait for,
        // the display's next refresh; then every message waiting; then the timeline's next steps (`Timeline::pump`). Else the
        // next message, waited for.
        if tl.running() {
            tl.wait(&screen)?;
        }
        loop {
            let msg = if tl.running() {
                match shows.try_recv() {
                    Ok(m) => m,
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => return Ok(()),
                }
            } else {
                match shows.recv() {
                    Ok(m) => m,
                    Err(_) => return Ok(()),
                }
            };
            match msg {
                Show::Frame { field, build_ms, shown, shape, replay, set, outbox } => {
                    match (between, field, replay, &shown) {
                        // A scene snapshot: drawn on the timeline, its changes applied before its first picture.
                        (true, Some((f, at, ready)), Some(r), Shown::Gpu(t)) if r.prepared => tl.push(f, at, ready, t.clone(), shape, r, outbox),
                        // A world frame the renderer recorded whole (it grabs the picture shown): after the frames before it, the
                        // newest of them drawn at its own time into the display buffer it grabs; then its commands, and the frame
                        // begins the timeline again as its own picture.
                        (true, Some((f, at, ready)), Some(r), Shown::Gpu(t)) => {
                            tl.stop(&mut screen)?;
                            outbox.apply(&screen.queue);
                            tl.push(f, at, ready, t.clone(), shape, r, crate::native::Outbox::default());
                        }
                        _ => {
                            // The timeline ends: the window's picture is the newest of the game's frames on it at its own time (a
                            // pause's menus are drawn over the level's last picture, the game's own, not one between two). Then
                            // the frame's own commands, which may draw on that picture in the display buffer.
                            if let Some((pic, shape)) = tl.stop(&mut screen)? {
                                screen.present_to(None, &Shown::Gpu(pic), shape)?;
                            }
                            outbox.apply(&screen.queue);
                            if let (Some(c), Some((f, _, _)), Shown::Gpu(_)) = (&gpu_clock, field, &shown) {
                                c.after_submit(f, Instant::now());
                            }
                            match (&mut plain, &mut presents) {
                                // At the plain path's steady pace: the picture's time (`Plain::hold`, once its world is done), its
                                // window copy made now, and the copy presented at its time (`Presents`); a frame not held (a
                                // movie's) presented after those before it.
                                (Some(p), Some(presents)) => {
                                    let target = match (field, &shown) {
                                        (Some((f, at, _)), Shown::Gpu(_)) => Some((f, at, p.hold(&screen, f, at, build_ms)?)),
                                        _ => None,
                                    };
                                    // The window holds one image at a time: the picture before presented first.
                                    presents.wait()?;
                                    let copying = Instant::now();
                                    let out = screen.acquire()?;
                                    let out = screen.window_copy(out, &shown, shape)?;
                                    let ms = copying.elapsed().as_secs_f64() * 1e3;
                                    if let Some((f, _, _)) = target {
                                        PLAIN_SHOWN.lock().expect("presenter time lock").add(ms);
                                        p.shown(f, ms);
                                    }
                                    if let Some(out) = out {
                                        presents.send(out, target.map_or_else(Instant::now, |t| t.2), target.map(|t| (t.0, t.1)))?;
                                    }
                                }
                                _ => {
                                    let showing = Instant::now();
                                    screen.present(&shown, shape, None)?;
                                    if !between && matches!(shown, Shown::Gpu(_)) {
                                        PLAIN_SHOWN.lock().expect("presenter time lock").add(showing.elapsed().as_secs_f64() * 1e3);
                                    }
                                }
                            }
                        }
                    }
                    last = Some((shown, shape, set));
                }
                Show::Shot(file, reply) => {
                    // The game's own frame: the newest frame on the timeline at its own time, else the last picture shown.
                    let r = match tl.own_picture(&mut screen) {
                        Ok(Some((pic, shape))) => screen.present_to(None, &Shown::Gpu(pic.clone()), shape).and_then(|()| screen.shot(&Shown::Gpu(pic), shape, &file)),
                        Ok(None) => match &last {
                            Some((shown, shape, _)) => screen.shot(shown, *shape, &file),
                            None => Err("play: no picture shown yet".into()),
                        },
                        Err(e) => Err(e),
                    };
                    let _ = reply.send(r);
                }
                Show::Drawn(reply) => {
                    let _ = reply.send(());
                }
            }
        }
        if tl.running() {
            tl.pump(&mut screen)?;
        }
    }
}

/// A time (ms) kept as its sum, count and the worst, for the play line every 300 fields.
struct Stat {
    sum: f64,
    n: u64,
    max: f64,
}

impl Stat {
    fn add(&mut self, ms: f64) {
        self.sum += ms;
        self.n += 1;
        self.max = self.max.max(ms);
    }

    /// The mean and the worst since the last, None when there is none; the count starts again.
    fn take(&mut self) -> Option<(f64, f64)> {
        let r = (self.n > 0).then(|| (self.sum / self.n as f64, self.max));
        (self.sum, self.n, self.max) = (0.0, 0, 0.0);
        r
    }
}

/// Where the presenter shows each frame as it comes (60 Hz and below, no timeline): the GPU's time a picture (`GpuClock`) and
/// the presenter's own (its copy, the acquire of the window's image and the present, which may wait for the display), since the
/// last play line.
static PLAIN_GPU: Mutex<Stat> = Mutex::new(Stat { sum: 0.0, n: 0, max: 0.0 });
static PLAIN_SHOWN: Mutex<Stat> = Mutex::new(Stat { sum: 0.0, n: 0, max: 0.0 });

/// The plain path's recent pictures as the GPU clock saw them (`GpuClock`): each one's field, when the GPU finished its world
/// and the GPU's time on it (ms), the last 16.
static PLAIN_DONE: Mutex<std::collections::VecDeque<(u64, Instant, f64)>> = Mutex::new(std::collections::VecDeque::new());

/// The plain path's presents at their times (`Plain`): each window copy, made as soon as its picture's world is done, presented by
/// a thread of its own at the picture's time, so the presenter goes on to the next picture's world meanwhile (holding the picture
/// on the presenter put the next picture's world on the GPU only after that time, and a picture slower than the pace's interval
/// was late however little the GPU took on the others). The window holds one acquired image at a time (wgpu: "Surface image is
/// already acquired"), so one picture waits at most: the next copy waits for its present (`wait`), and the GPU runs an interval
/// ahead at most.
struct Presents {
    tx: Option<std::sync::mpsc::Sender<(wgpu::SurfaceTexture, Instant, Option<(u64, Instant)>)>>,
    presented: std::sync::mpsc::Receiver<()>,
    waiting: bool,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Presents {
    fn start(window: Arc<Window>, queue: wgpu::Queue) -> Result<Presents, String> {
        let (tx, rx) = std::sync::mpsc::channel::<(wgpu::SurfaceTexture, Instant, Option<(u64, Instant)>)>();
        let (told, presented) = std::sync::mpsc::channel::<()>();
        let thread = std::thread::Builder::new()
            .name("uya presents".into())
            .spawn(move || {
                #[cfg(windows)]
                {
                    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
                    // SAFETY: the calling thread's own handle.
                    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
                }
                for (out, at, picture) in rx {
                    let now = Instant::now();
                    if at > now {
                        std::thread::sleep(at - now);
                    }
                    let presenting = Instant::now();
                    window.pre_present_notify();
                    queue.present(out);
                    if told.send(()).is_err() {
                        return;
                    }
                    if frame_times()
                        && let Some((f, field_at)) = picture
                    {
                        println!("frame stages: plain shown {f} field at {:.3} presented {:.3} took {:.3} target {:.3}", stamp(field_at), stamp(presenting), presenting.elapsed().as_secs_f64() * 1e3, stamp(at));
                    }
                }
            })
            .map_err(|e| format!("play: the presents' thread: {e}"))?;
        Ok(Presents { tx: Some(tx), presented, waiting: false, thread: Some(thread) })
    }

    /// Window image `out` presented at `at` (picture `picture`: its field and when it began).
    fn send(&mut self, out: wgpu::SurfaceTexture, at: Instant, picture: Option<(u64, Instant)>) -> Result<(), String> {
        self.waiting = true;
        self.tx.as_ref().expect("open until dropped").send((out, at, picture)).map_err(|_| "play: the presents' thread stopped".to_string())
    }

    /// Until the picture waiting, if any, is presented.
    fn wait(&mut self) -> Result<(), String> {
        if std::mem::take(&mut self.waiting) {
            self.presented.recv().map_err(|_| "play: the presents' thread stopped".to_string())?;
        }
        Ok(())
    }
}

impl Drop for Presents {
    /// Every picture waiting presented before the window's surface goes.
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The plain path's pace (`Plain`): the game thread offers the renderer one field in this many (`rtgame.rs`), 1 to
/// `PLAIN_MOST`. The timeline leaves it at 1: it draws its own pictures between the frames, at its own pace.
pub(crate) static PACE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// The slowest plain pace: one field in four, 15 pictures a second.
const PLAIN_MOST: u32 = 4;

/// The plain path's steady pictures (60 Hz and below, the fields paced). Public issue #1's player (a Ryzen 5 4600G's integrated
/// graphics: the GPU 20 to 27 ms a picture on Veldin, 33 to 50 in its heavier stretches) got whichever fields found the renderer
/// free, 76 to 120 of 300 at uneven intervals, which looks worse than a steady lower rate (the user, 28 November 2026: "Steady
/// pacing full stop across all FPS ranges, as well as below 60").
/// - The pace (`PACE`): one field in `pace`, the fewest whose time the frame builder and the GPU (with the presenter's own time)
///   take on average within 95 % of it: 60, 30, 20 or 15 pictures a second. The GPU works an interval ahead at most (each
///   picture's world goes on it as soon as its frame comes) and the latency below takes up the slower pictures: held to 9
///   pictures in 10 (the user, of 20 a second where 40 had been shown unevenly: "30 would be fine, but dropping to 20 is not
///   fine"), a GPU of 20 to 34 ms a picture, 26 on average, fell to 20 though it keeps 30; held to 3 in 4, one of 14.5 ms on
///   average, that had drawn every field, fell to 30. The means are the last second's pictures' (`Recent`). A slower pace is
///   taken once the pace has failed that for three seconds (`SLOWER_AFTER`: a stall at a level's start does not lower the
///   rate), a faster one once it has passed within 90 % for three or within half for half a second (`Holds`); the first is
///   chosen at once from the first 30 pictures.
///   Halving the pictures halves the frame builder's and the GPU's work as well.
/// - Each picture is presented `latency` after its field began (the game's fields begin at its even rate): the presenter submits
///   each frame's world (`Native::defer_world`), and once it is done makes its window copy, which is presented at its time by a
///   thread of its own (`Presents`); the pictures leave at the fields' intervals, not at the GPU's uneven finishes. The latency is what 49 pictures in
///   50 need (with 9 in 10, a picture in ten was late, each a few milliseconds off its interval): their field's start to their
///   world done as the GPU clock saw it (`PLAIN_DONE`; measured when this thread came to the picture, after holding the one
///   before, a picture's time counted that hold, and the latency and the GPU's time fed on themselves: 100 ms after the field
///   for a world done at 32), with the presenter's time and 1 ms, at most two of the pace's intervals (one picture waits for its
///   time at most: `Presents`). It goes up when the need
///   passes it by 2 ms (by 2 % of the time between the pictures' fields at most: a picture shows that much later than its
///   field's interval) and down when it passes the need by 8 ms (1 %), and is set at once after a gap.
struct Plain {
    pace: u32,
    latency: f64,
    /// The last 120 pictures' times: from their field's start to their world done on the GPU (s), the renderer's to hand each over
    /// and the GPU's on each (ms).
    arrivals: std::collections::VecDeque<f64>,
    build: std::collections::VecDeque<f64>,
    gpu: std::collections::VecDeque<f64>,
    /// The last second's pictures' times (`Recent`, what the pace is chosen from): the renderer's to hand each over and the
    /// GPU's on each (ms).
    recent: [Recent; 2],
    /// The presenter's recent time a picture (ms: the window's copy and the present).
    present_ms: f64,
    /// When the last picture was presented, and when its field began.
    last: Option<Instant>,
    last_at: Option<Instant>,
    /// When the pace's criteria began to hold (`Holds`); whether the first pace was chosen.
    holds: Holds,
    settled: bool,
    /// Since the last line: the pictures and of them those late (their world done after their time); when it began.
    counts: (u64, u64),
    since: Instant,
}

/// `v` onto the end of a ring of the last 120.
fn ring_push(ring: &mut std::collections::VecDeque<f64>, v: f64) {
    ring.push_back(v);
    if ring.len() > 120 {
        ring.pop_front();
    }
}

/// A ring's 90th percentile (0 when empty).
fn ring_p90(ring: &std::collections::VecDeque<f64>) -> f64 {
    ring_share(ring, 0.9)
}

/// The value a share `q` of a ring's values are at most (0 when empty).
fn ring_share(ring: &std::collections::VecDeque<f64>, q: f64) -> f64 {
    let mut v: Vec<f64> = ring.iter().copied().collect();
    v.sort_by(f64::total_cmp);
    v.get(((v.len() as f64 * q) as usize).min(v.len().saturating_sub(1))).copied().unwrap_or(0.0)
}

/// The span over which the paces judge the recent pictures (`Recent`): the last second's pictures, by time. When they were kept
/// by count (the last 120), a stall's slow pictures stayed in for as long as 120 more pictures took. At one refresh in 8 that
/// was 2.7 s, and with the three seconds a faster pace had to pass, the player's PC kept 45 a second for 4.3 s after a stall at
/// a mission's start, although it keeps up (360 Hz, 180 to 320 pictures a second otherwise; level 26, the first battle
/// mission, 28 November 2026).
const RECENT: Duration = Duration::from_secs(1);

/// How long a pace must fail before a slower one is taken: three times `RECENT`. Slow pictures over less than two seconds
/// (a stall at a level's or a mission's start: the player's above were slow for some 1.7 s) then never lower the rate,
/// while a GPU that cannot keep the pace fails all along. With one second, that stall took the player to one refresh in 8.
const SLOWER_AFTER: Duration = Duration::from_secs(3);

/// The times (ms) of the pictures of the last `RECENT`, each with when it was taken.
#[derive(Default)]
struct Recent(std::collections::VecDeque<(Instant, f64)>);

impl Recent {
    fn push(&mut self, at: Instant, ms: f64) {
        self.0.push_back((at, ms));
        while self.0.front().is_some_and(|&(t, _)| at.saturating_duration_since(t) > RECENT) {
            self.0.pop_front();
        }
    }

    fn len(&self) -> usize {
        self.0.len()
    }

    /// The mean (the window not empty: `len`).
    fn mean(&self) -> f64 {
        self.0.iter().map(|p| p.1).sum::<f64>() / self.0.len() as f64
    }

    /// The value a share `q` of the times are at most (the window not empty: `len`).
    fn share(&self, q: f64) -> f64 {
        let mut v: Vec<f64> = self.0.iter().map(|p| p.1).collect();
        v.sort_by(f64::total_cmp);
        v[((v.len() as f64 * q) as usize).min(v.len() - 1)]
    }
}

/// When the pace began to fail, when a faster one began to pass with the usual margin and when one began to pass with half to
/// spare (`Holds::next`): a pace changes only once its criterion has held.
#[derive(Default)]
struct Holds {
    failing: Option<Instant>,
    passing: Option<Instant>,
    clear: Option<Instant>,
}

impl Holds {
    /// The pace after this picture: `slow` is what the recent times admit with the usual margin, `fast` with the margin a faster
    /// pace needs (no back and forth at the edge), `clear` with half the interval to spare. A slower pace is taken once `slow`
    /// has been slower for `SLOWER_AFTER`. A faster one is taken once `fast` has been faster for three seconds, or once `clear`
    /// has been faster for half a second: a PC far from the edge (one a stall slowed) comes back at once.
    fn next(&mut self, now: Instant, pace: u32, slow: u32, fast: u32, clear: u32) -> u32 {
        let held = |t: &mut Option<Instant>, on: bool, d: Duration| match on {
            true => now.duration_since(*t.get_or_insert(now)) >= d,
            false => {
                *t = None;
                false
            }
        };
        let slower = held(&mut self.failing, slow > pace, SLOWER_AFTER);
        let cleared = held(&mut self.clear, clear < pace, Duration::from_millis(500));
        let faster = held(&mut self.passing, fast < pace, Duration::from_secs(3));
        if slower {
            slow
        } else if cleared {
            clear
        } else if faster {
            fast
        } else {
            pace
        }
    }
}

impl Plain {
    fn new() -> Plain {
        Plain { pace: 1, latency: 0.0, arrivals: Default::default(), build: Default::default(), gpu: Default::default(), recent: Default::default(), present_ms: 1.0, last: None, last_at: None, holds: Holds::default(), settled: false, counts: (0, 0), since: Instant::now() }
    }

    /// Picture `field`'s time (its field began at `at`, the renderer's time on it `build_ms`), once its world is done on the GPU:
    /// `latency` after its field began. A picture whose world is done later than that is late. The world's finish and GPU time are the GPU clock's (`GpuClock`, `PLAIN_DONE`: seen as the GPU finished it,
    /// not when this thread came to wait for it, after the picture before was shown).
    fn hold(&mut self, screen: &Screen, field: u64, at: Instant, build_ms: f64) -> Result<Instant, String> {
        // Everything submitted so far, the picture's world among it, waited for a few milliseconds at a time.
        let index = screen.queue.submit([]);
        let waiting = Instant::now();
        while !finished(&screen.device, &index, Duration::from_millis(2))? {
            if waiting.elapsed() > Duration::from_secs(10) {
                return Err("play: the GPU did not finish a picture in 10 s".into());
            }
        }
        // The GPU clock's callback for it has run, on this thread's wait or on the clock's own poll, which may still be writing it.
        let seen = Instant::now();
        let (done, gpu_ms) = loop {
            if let Some(d) = PLAIN_DONE.lock().expect("GPU finishes lock").iter().find(|d| d.0 == field) {
                break (d.1, d.2);
            }
            if seen.elapsed() > Duration::from_millis(100) {
                return Err(format!("play: field {field}: the GPU clock did not see its picture finish"));
            }
            std::thread::sleep(Duration::from_micros(200));
        };
        // After a gap (a level's load, the renderer held): the times begin again.
        let gap = self.last.is_none_or(|l| done.saturating_duration_since(l) > Duration::from_millis(500));
        if gap {
            self.arrivals.clear();
        }
        ring_push(&mut self.arrivals, done.saturating_duration_since(at).as_secs_f64());
        ring_push(&mut self.build, build_ms);
        ring_push(&mut self.gpu, gpu_ms);
        self.recent[0].push(done, build_ms);
        self.recent[1].push(done, gpu_ms);
        let interval = self.pace as f64 * FIELD_S;
        let need = (ring_share(&self.arrivals, 0.98) + (self.present_ms + 1.0) / 1e3).min(2.0 * interval);
        match self.last_at {
            Some(l) if !gap => {
                let dt = at.saturating_duration_since(l).as_secs_f64();
                if need > self.latency + 0.002 {
                    self.latency += (need - self.latency).min(0.02 * dt);
                } else if self.latency > need + 0.008 {
                    self.latency -= (self.latency - need).min(0.01 * dt);
                }
            }
            _ => self.latency = need,
        }
        self.last_at = Some(at);
        self.cadence(field);
        let target = at + Duration::from_secs_f64(self.latency);
        self.counts.1 += (done > target) as u64;
        Ok(target)
    }

    /// The picture of field `field` presented, the presenter's part of it `ms`.
    fn shown(&mut self, field: u64, ms: f64) {
        fold(&mut self.present_ms, ms);
        let now = Instant::now();
        self.last = Some(now);
        self.counts.0 += 1;
        let secs = now.duration_since(self.since).as_secs_f64();
        if secs >= 10.0 {
            let (n, late) = self.counts;
            println!("play: field {field}: {:.1} pictures a second, of one field in {}: each {:.1} ms after its field began, {late} of {n} late; the frame builder {:.1} ms and the GPU {:.1} ms a picture (9 in 10)", n as f64 / secs, self.pace, self.latency * 1e3, ring_p90(&self.build), ring_p90(&self.gpu));
            (self.counts, self.since) = ((0, 0), now);
        }
    }

    /// The pace for the last second's times (`PACE`, `Recent`, `Holds`).
    fn cadence(&mut self, field: u64) {
        if self.gpu.len() < 30 || self.recent[1].len() < 8 {
            return;
        }
        let (build, gpu) = (self.recent[0].mean(), self.recent[1].mean());
        let most = build.max(gpu + self.present_ms) / 1e3;
        let pick = |share: f64| (1..=PLAIN_MOST).find(|&k| most <= share * k as f64 * FIELD_S).unwrap_or(PLAIN_MOST);
        let was = self.pace;
        if !self.settled {
            self.settled = true;
            self.pace = pick(0.95);
        } else {
            self.pace = self.holds.next(Instant::now(), self.pace, pick(0.95), pick(0.90), pick(0.5));
        }
        if self.pace != was {
            self.holds = Holds::default();
            PACE.store(self.pace, Ordering::Relaxed);
            println!("play: field {field}: pictures of one field in {} ({:.0} a second): the frame builder {build:.1} ms and the GPU {gpu:.1} ms a picture (the last second's means)", self.pace, 1.0 / (self.pace as f64 * FIELD_S));
        }
    }
}

/// The diagnostics (FPS COUNTER's DETAILED, `Fps`, and the play log's diagnostics line, every 300 fields): the recent time in ms
/// of a field on the game's own thread (its EE work, `rtgame`), of a frame on the frame builder (its own work, without the wait
/// to hand the frame to the presenter) and of a picture on the GPU, each an f32's bits with each new time weighed in a tenth
/// (`diag_fold`); and the pictures the window showed in the last second (`Fps::count`).
pub(crate) static DIAG_GAME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static DIAG_FRAME: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static DIAG_GPU: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static DIAG_FPS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A new time `ms` weighed into the recent time `a` holds (a tenth of it).
pub(crate) fn diag_fold(a: &std::sync::atomic::AtomicU32, ms: f64) {
    let recent = f32::from_bits(a.load(Ordering::Relaxed)) as f64;
    a.store(((recent * 0.9 + ms * 0.1) as f32).to_bits(), Ordering::Relaxed);
}

/// The recent time `a` holds (ms).
fn diag(a: &std::sync::atomic::AtomicU32) -> f64 {
    f32::from_bits(a.load(Ordering::Relaxed)) as f64
}

struct ClockState {
    /// When the GPU finished the last picture.
    last: Mutex<Option<Instant>>,
    /// The pictures whose finish is not seen yet.
    pending: std::sync::atomic::AtomicU64,
}

/// The GPU's time a picture where the presenter shows each frame as it comes: from when the builder has handed the picture's
/// world to the GPU (or when the picture before finished, if the GPU is still at that one) to when the GPU finishes it. A GPU
/// that keeps up shows the tail of a picture it began as it was built; a GPU that does not shows its whole time a picture. The
/// finish is seen by a thread that polls the device every half millisecond while a picture is pending and sleeps otherwise (the
/// PCs this line is for have few cores to spare), so no thread waits for the GPU.
struct GpuClock {
    queue: wgpu::Queue,
    state: Arc<ClockState>,
    /// The polling thread, woken when a picture is handed over.
    poller: std::thread::Thread,
}

impl GpuClock {
    fn start(device: wgpu::Device, queue: wgpu::Queue) -> Result<GpuClock, String> {
        let state = Arc::new(ClockState { last: Mutex::new(None), pending: std::sync::atomic::AtomicU64::new(0) });
        let polled = state.clone();
        let poller = std::thread::Builder::new()
            .name("uya gpu clock".into())
            .spawn(move || {
                // Until the renderer lets go of its clock (and every picture's callback has run): asleep while no picture is
                // pending (woken by `after_submit`, and every 100 ms to see whether the renderer has let go).
                while Arc::strong_count(&polled) > 1 {
                    if polled.pending.load(Ordering::Relaxed) == 0 {
                        std::thread::park_timeout(Duration::from_millis(100));
                        continue;
                    }
                    if let Err(e) = device.poll(wgpu::PollType::Poll) {
                        println!("play: the GPU clock stopped: {e}");
                        return;
                    }
                    std::thread::sleep(Duration::from_micros(500));
                }
            })
            .map_err(|e| format!("play: GPU clock thread: {e}"))?
            .thread()
            .clone();
        Ok(GpuClock { queue, state, poller })
    }

    /// The world of field `field`'s picture, just built, was handed to the GPU at `submitted`.
    fn after_submit(&self, field: u64, submitted: Instant) {
        self.state.pending.fetch_add(1, Ordering::Relaxed);
        self.poller.unpark();
        let state = self.state.clone();
        self.queue.on_submitted_work_done(move || {
            let done = Instant::now();
            let mut last = state.last.lock().expect("GPU clock lock");
            let began = last.map_or(submitted, |l| l.max(submitted));
            let gpu_ms = done.saturating_duration_since(began).as_secs_f64() * 1e3;
            PLAIN_GPU.lock().expect("GPU time lock").add(gpu_ms);
            diag_fold(&DIAG_GPU, gpu_ms);
            *last = Some(last.map_or(done, |l| l.max(done)));
            state.pending.fetch_sub(1, Ordering::Relaxed);
            if frame_times() {
                println!("frame stages: plain done {field} at {:.3} GPU {gpu_ms:.3}", stamp(done));
            }
            let mut finished = PLAIN_DONE.lock().expect("GPU finishes lock");
            finished.push_back((field, done, gpu_ms));
            if finished.len() > 16 {
                finished.pop_front();
            }
        });
    }
}

/// The NTSC field in seconds.
const FIELD_S: f64 = 1001.0 / 60000.0;

/// The frames DLSS Frame Generation is generating now per picture the presenter draws: 0 while it is not (Streamline not loaded,
/// DLSS-G unsupported, FRAME GENERATION off, or DLSS-G showing the real pictures alone, as it does while the game's window is not
/// in front). Set by frame generation's code from what DLSS-G shows (`streamline::present`); the presenter draws its own pictures
/// at least this many refreshes plus one apart, so the generated ones fit within the display's rate, and counts only its own.
pub(crate) static FG_GENERATED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// The refreshes between two pictures the presenter draws at the least: one, or one more per generated frame.
fn least_step() -> u32 {
    FG_GENERATED.load(Ordering::Relaxed) + 1
}

/// A picture's time (ms) into a recent time (`Timeline::encode_ms`, `gpu_ms`, `display_ms`): a tenth of the way, the time held
/// to twice the recent one and half a ms. One picture held up (the player's session on Crash Site, 23 November 2026: a picture
/// every few seconds took 10 to 130 ms) moved the recent time by up to 13 ms and the pace fell to 120 or 60 pictures a second
/// for the two seconds until `cadence` settled again; times that stay high still move it as before.
fn fold(recent: &mut f64, ms: f64) {
    *recent = *recent * 0.9 + ms.min(*recent * 2.0 + 0.5) * 0.1;
}

/// One display's vertical blank (the window's monitor, `\\.\DISPLAYn`): the kernel's display wait (gdi32's D3DKMT calls, which
/// DXGI's WaitForVBlank makes too), each wait returning as that display begins a refresh.
struct VBlank {
    adapter: u32,
    source: u32,
}

#[repr(C)]
struct OpenAdapterFromName {
    name: [u16; 32],
    adapter: u32,
    luid: [u32; 2],
    source: u32,
}

#[repr(C)]
struct WaitForVBlank {
    adapter: u32,
    device: u32,
    source: u32,
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn D3DKMTOpenAdapterFromGdiDisplayName(p: *mut OpenAdapterFromName) -> i32;
    fn D3DKMTWaitForVerticalBlankEvent(p: *const WaitForVBlank) -> i32;
}

impl VBlank {
    fn open(display: &str) -> Result<VBlank, String> {
        let mut name = [0u16; 32];
        let wide: Vec<u16> = display.encode_utf16().collect();
        if wide.is_empty() || wide.len() >= 32 {
            return Err(format!("play: the display's name \"{display}\" for its vertical blank"));
        }
        name[..wide.len()].copy_from_slice(&wide);
        let mut p = OpenAdapterFromName { name, adapter: 0, luid: [0; 2], source: 0 };
        // SAFETY: the struct is D3DKMT_OPENADAPTERFROMGDIDISPLAYNAME's layout, its name terminated.
        let status = unsafe { D3DKMTOpenAdapterFromGdiDisplayName(&mut p) };
        if status != 0 {
            return Err(format!("play: {display}: its adapter for the vertical blank: status {status:#x}"));
        }
        Ok(VBlank { adapter: p.adapter, source: p.source })
    }

    /// Returns at the display's next vertical blank.
    fn wait(&self) -> Result<(), String> {
        let w = WaitForVBlank { adapter: self.adapter, device: 0, source: self.source };
        // SAFETY: the adapter and source come from `open`.
        let status = unsafe { D3DKMTWaitForVerticalBlankEvent(&w) };
        if status != 0 {
            return Err(format!("play: the display's vertical blank: status {status:#x}"));
        }
        Ok(())
    }
}

// SAFETY: the handles are plain values the kernel calls take from any thread.
unsafe impl Send for VBlank {}

/// The display's refreshes as they come (`VBlank`), noted by a thread of their own: the presenter reads the last one, and so
/// where every refresh falls, without waiting for the next.
struct Refreshes {
    base: Instant,
    last: Arc<std::sync::atomic::AtomicU64>,
    stop: Arc<AtomicBool>,
}

impl Refreshes {
    fn start(vblank: VBlank) -> Result<Refreshes, String> {
        let base = Instant::now();
        let (last, stop) = (Arc::new(std::sync::atomic::AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        let (l, s) = (last.clone(), stop.clone());
        std::thread::Builder::new()
            .name("uya refreshes".into())
            .spawn(move || {
                #[cfg(windows)]
                {
                    use windows::Win32::System::Threading::{GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_HIGHEST};
                    // SAFETY: the calling thread's own handle.
                    let _ = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_HIGHEST) };
                }
                // A display that is off or occluded has no vertical blank to wait for: tried again every 50 ms until it has one,
                // the pictures timed meanwhile by the clock at the display's rate (`Timeline::grid`).
                let mut failing = false;
                while !s.load(Ordering::Relaxed) {
                    match vblank.wait() {
                        Ok(()) => {
                            if failing {
                                println!("play: the display's vertical blank is back: the pictures follow its refreshes again");
                                failing = false;
                            }
                            l.store(base.elapsed().as_nanos() as u64 + 1, Ordering::Release);
                        }
                        Err(e) => {
                            if !failing {
                                println!("{e} (the display off or occluded): the pictures are timed by the clock at the display's rate until it has one");
                                failing = true;
                            }
                            std::thread::sleep(Duration::from_millis(50));
                        }
                    }
                }
            })
            .map_err(|e| format!("play: the refreshes' thread: {e}"))?;
        Ok(Refreshes { base, last, stop })
    }

    /// The last refresh noted (None before the first).
    fn last(&self) -> Option<Instant> {
        let n = self.last.load(Ordering::Acquire);
        (n != 0).then(|| self.base + Duration::from_nanos(n - 1))
    }
}

impl Drop for Refreshes {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The display's timeline (PLAN 8.8): the game's frames that drew the world, each placed at the time its field began plus a
/// latency, and pictures of the game's time at the display's refreshes: between frame N - 1 and frame N, `alpha` of the way,
/// frame N's replay drawn at it (`replay::Replay::draw`), or frame N's own picture at its own time (and when no picture between
/// the two can be drawn: a cut, the first frame after a menu).
///
/// - One picture's world is on the GPU while the next is recorded on the recorder's thread (`Recorder`: recording a scene
///   snapshot replays its static world's render bundles, which wgpu does on the CPU, as long as the GPU takes a picture on
///   Florana with ray tracing; recorded on the presenter's thread, the two followed each other); the next is submitted once the
///   first is shown, so the window's copy of a picture never queues behind the next picture's world, and the presenter presents
///   only a picture whose GPU work is done (the driver's present waits for it, and blocked every other submission meanwhile).
/// - The pace (`cadence`): a picture every `step` refreshes, the fastest whose pictures' 90th percentile GPU time (each game
///   frame's first picture traces the frame's lighting; the pictures after it reuse that; the GPU waits through each window's
///   copy) stays under 93 % of the interval and whose recording fits, down to 15 a second: a GPU that cannot keep the game's
///   60 shows a steady lower rate between its frames (the user, 28 November 2026: "Steady pacing full stop across all FPS
///   ranges, as well as below 60"), not each frame once as the GPU finishes it, at its uneven intervals. The times are the last
///   second's pictures' (`Recent`). A slower pace is taken once it has failed for three seconds (`SLOWER_AFTER`), a faster one
///   once it has passed with a quarter to spare for three or with half to spare for half a second (`Holds`): a few slow
///   pictures at a level's or a mission's start do not lower the rate of a PC that keeps up. UNCAPPED has no pace but the
///   least step while a frame's first picture fits the game's own rate, and the steps below it beyond that.
/// - A picture is drawn for the refresh it can reach (its GPU's 90th percentile time and a margin from when the GPU can take it,
///   after the picture on it is shown, on the display's grid of refreshes: `Refreshes`) and no sooner than the pace's next
///   (`plan`). It is recorded in time for its world to go on the GPU the moment the picture before is shown, and no sooner than
///   the latency allows; presented the refresh before its own, so it shows at that refresh and the game's time it was drawn for
///   is the game's time on the display, in every mode (UNCAPPED too: the pictures are as many as the GPU keeps up with, each
///   at the refresh its time was drawn for), and never in the refresh the last picture takes (the window shows one picture a
///   refresh).
/// - The game's fields are placed on the wall clock by a line through the last 120 frames' field starts (`fit`): the game's time
///   advances by the refreshes a picture spans over the line's seconds a field, with a correction of 2 % at most toward the
///   target the latency places at the picture's refresh (`time_at`), never past the newest frame and never back.
/// - The latency (`need`, `slew_latency`) is what the frames need: a field (none for each frame shown once), the time 9 in 10
///   frames took from their field's start on the line to the presenter, the longest a picture may be recorded before its
///   refresh (`lead`) and 1 ms. It moves only when the need passes it by 3 ms (up: the game's time slows by 2 % at most) or
///   it passes the need by 8 ms (down, 1 %).
/// How far off the fitted line a frame's field may begin before the line begins again from it (`Timeline::push`): three fields.
/// A field the game thread began a little late slips by a few milliseconds and the line follows; only a step goes past this.
const OFF_LINE: f64 = 3.0 * FIELD_S;

struct Timeline {
    profile: Option<crate::gpu_profile::Profiler>,
    period: f64,
    /// The display's refreshes: they are at the last one noted and whole periods from it.
    refreshes: Option<Refreshes>,
    /// The pace (`cadence`): a picture every `step` refreshes.
    step: u32,
    /// When the pace's criteria began to hold (`cadence`, `Holds`): a pace changes only after its criterion has held for a while;
    /// and whether the first pace was chosen yet.
    holds: Holds,
    settled: bool,
    /// The last second's GPU times a picture (ms), a frame's first picture's and all (`Recent`): what the pace is chosen from.
    recent: [Recent; 2],
    /// The game's fields on the wall clock: the recent frames' (time since `t0`, field), their least-squares line (its seconds a
    /// field, `rate`, and the mean point it passes through), from which the game's time is placed at a refresh (`time_at`).
    fit: std::collections::VecDeque<(f64, f64)>,
    rate: f64,
    fit_mean: (f64, f64),
    /// Recent GPU times a picture (ms), a frame's first picture's, the pictures after it, and all: the last 120 of each and the
    /// 90th percentile of each (what a pace and a picture's refresh are chosen from: a picture in ten may take more).
    gpu_ring: [std::collections::VecDeque<f64>; 3],
    gpu_p90: [f64; 3],
    /// When the timeline was made (the presented pictures' times in the log are from it).
    t0: Instant,
    /// The presenter's time a picture (ms: recording it and handing it to the window): the sum and the worst since the last line.
    work: (f64, f64),
    /// The pictures shown at a later refresh than the one they were drawn for, since the last line.
    late: u64,
    /// How long after its field began the game's time shown was on the display (ms: from the start of the field it shows to the
    /// refresh the picture shows at): the sum and the worst since the last line.
    behind: (f64, f64),
    frames: std::collections::VecDeque<TimelineFrame>,
    /// The game's time (in fields) of the last picture recorded, and the refresh it was drawn for.
    shown: Option<(f64, Instant)>,
    latency: f64,
    /// The recent frames' time from their field's start to the presenter (s).
    arrivals: std::collections::VecDeque<f64>,
    /// The picture a replay is drawn into (its size).
    extra: Option<(wgpu::Texture, wgpu::Texture, wgpu::TextureView, wgpu::TextureView, (u32, u32))>,
    /// Since the last line: pictures shown, of them between two frames, of them the game's own, shown again; and when it began.
    counts: [u64; 4],
    since: Instant,
    /// UYA_PRESENTED=DIR,FROM,COUNT: the COUNT pictures shown from the first at the game's time FROM written to DIR.
    capture: Option<Capture>,
    /// The picture whose world is on the GPU, and the next one, recorded and not submitted yet (`pump`).
    gpu: Option<Pending>,
    staged: Option<Pending>,
    /// The next picture's recording time and refresh (`plan`), kept until it is recorded, and planned again only while the
    /// picture on the GPU runs past its expected time: planned again as time passes, a picture woken a little late for its
    /// recording would lose its refresh, and the next, without end.
    next: Option<(Instant, Instant)>,
    /// When the last picture was presented.
    presented: Option<Instant>,
    /// Recent times (ms): recording a picture, and a picture's GPU time from its submission to its completion, for a frame's
    /// first picture (which traces the frame's lighting) and for the pictures after it, and handing a picture to the window.
    /// Since the last line: the GPU times' sums and counts.
    encode_ms: f64,
    gpu_ms: [f64; 2],
    display_ms: f64,
    gpu_sum: [f64; 2],
    gpu_n: [u64; 2],
    /// The changes of frames passed before any picture of theirs was recorded, in the frames' order: applied before the next
    /// picture (`record`), as a frame's own are before its first.
    unapplied: std::collections::VecDeque<crate::native::Outbox>,
    /// The recorder's thread, and the picture it records now (its frame's changes' commands in; its world's come back: `collect`).
    recorder: Recorder,
    recording: Option<Pending>,
}

/// The recorder's thread (`Timeline`): a scene snapshot's picture drawn into a command buffer there (`replay::Replay::draw`)
/// while the presenter's thread shows the picture before; one at a time, in the order asked.
struct Recorder {
    jobs: std::sync::mpsc::Sender<RecordJob>,
    done: std::sync::mpsc::Receiver<Result<Recorded, String>>,
}

/// A picture for the recorder: the frame's snapshot, the picture it is drawn into, where between its frames, its pass timer.
struct RecordJob {
    replay: Arc<crate::replay::Replay>,
    pic: wgpu::Texture,
    alpha: f32,
    profile: Option<crate::gpu_profile::Recording>,
}

/// A picture the recorder drew: its world's commands (DLSS's a command buffer of their own), its pass timer, and the time it
/// took (ms).
struct Recorded {
    cmd: Vec<wgpu::CommandBuffer>,
    profile: Option<crate::gpu_profile::Recording>,
    ms: f64,
    /// The frame its recording began for DLSS Frame Generation (`streamline::take_begun`).
    fg: Option<u32>,
}

impl Recorder {
    fn start(device: wgpu::Device, queue: wgpu::Queue) -> Result<Recorder, String> {
        let (jobs, asked) = std::sync::mpsc::channel::<RecordJob>();
        let (tell, done) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("uya record".into())
            .spawn(move || {
                while let Ok(j) = asked.recv() {
                    let started = Instant::now();
                    let r = (|| -> Result<Recorded, String> {
                        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("present scene snapshot") });
                        // Under the replay's flag, as the presenter's own recording was (a later frame laying ties out anew sets it).
                        let stale = j.replay.stale().lock().map_err(|_| "play: a replay's flag is poisoned")?;
                        let r = &j.replay;
                        let mut cmd = crate::replay::Chunks::default();
                        r.draw(&device, &queue, &mut enc, &crate::replay::Target { tex: &j.pic, depth: &r.output_depth, tv: &r.output_view, dv: &r.output_depth_view, size: r.size }, j.alpha, j.profile.as_ref(), &mut cmd)?;
                        drop(stale);
                        cmd.encoder(enc)?;
                        Ok(Recorded { cmd: cmd.finish()?, profile: j.profile, ms: started.elapsed().as_secs_f64() * 1e3, fg: crate::streamline::take_begun() })
                    })();
                    if tell.send(r).is_err() {
                        return;
                    }
                }
            })
            .map_err(|e| format!("play: the recorder's thread: {e}"))?;
        Ok(Recorder { jobs, done })
    }
}

/// A picture on its way to the window (`Timeline::pump`): its commands (its frame's shared changes' commands, then its world;
/// empty once submitted), its submission and when it was made and finished, and what it shows.
struct Pending {
    profile: Option<crate::gpu_profile::Recording>,
    cmd: Vec<wgpu::CommandBuffer>,
    index: Option<wgpu::SubmissionIndex>,
    submitted: Option<Instant>,
    done: Option<Instant>,
    /// Whether it draws a world on the GPU (its GPU time is then a picture's).
    drawn: bool,
    /// Its frame for DLSS Frame Generation (`streamline::take_begun`), named to Streamline as it is presented.
    fg: Option<u32>,
    pic: wgpu::Texture,
    shape: (u32, u32),
    field: u64,
    between: bool,
    again: bool,
    /// The game's time it shows, `alpha` of the way from the frame before (field `from`), and the refresh it is drawn for.
    g: f64,
    alpha: f32,
    from: u64,
    at: Instant,
    /// Whether it traces its frame's lighting (the frame's first picture).
    first: bool,
    encode_ms: f64,
    /// When its recording began.
    recorded: Instant,
    /// When the field of the game's time it shows (`g`) began, on its frame's field times.
    begun: Instant,
}

/// Whether the GPU has done submission `index`, waiting for it `timeout` at most.
fn finished(device: &wgpu::Device, index: &wgpu::SubmissionIndex, timeout: Duration) -> Result<bool, String> {
    match device.poll(wgpu::PollType::Wait { submission_index: Some(index.clone()), timeout: Some(timeout) }) {
        Ok(_) => Ok(true),
        Err(wgpu::PollError::Timeout) => Ok(false),
        Err(e) => Err(format!("play: the presenter's GPU wait: {e}")),
    }
}

struct TimelineFrame {
    field: u64,
    at: Instant,
    pic: wgpu::Texture,
    shape: (u32, u32),
    replay: Arc<crate::replay::Replay>,
    /// The frame's changes to the shared GPU state (`native::Outbox`), until they are applied: before the frame's first picture,
    /// or, the frame passed, before the next picture.
    outbox: Option<crate::native::Outbox>,
}

/// The pictures shown, copied as they are shown and written once all are (`Timeline::capture`).
struct Capture {
    dir: String,
    from: f64,
    count: usize,
    taken: Vec<(wgpu::Texture, String)>,
    /// The refresh of the first picture taken.
    first: Option<Instant>,
}

impl Timeline {
    fn new(hz: f64, vblank: Option<VBlank>, device: &wgpu::Device, queue: &wgpu::Queue) -> Result<Timeline, String> {
        let capture = std::env::var("UYA_PRESENTED").ok().map(|v| {
            let p: Vec<&str> = v.split(',').collect();
            assert!(p.len() == 3, "UYA_PRESENTED=DIR,FROM,COUNT, not {v}");
            Capture { dir: p[0].to_string(), from: p[1].parse().expect("UYA_PRESENTED's FROM, a field"), count: p[2].parse().expect("UYA_PRESENTED's COUNT"), taken: Vec::new(), first: None }
        });
        // Until pictures are timed: about what a combat picture takes on this PC.
        let refreshes = vblank.map(Refreshes::start).transpose()?;
        Ok(Timeline { profile: crate::gpu_profile::Profiler::new(), period: 1.0 / hz, refreshes, step: 1, holds: Holds::default(), settled: false, recent: Default::default(), fit: Default::default(), rate: FIELD_S, fit_mean: (0.0, 0.0), gpu_ring: Default::default(), gpu_p90: [12.0, 10.0, 12.0], t0: Instant::now(), work: (0.0, 0.0), late: 0, behind: (0.0, 0.0), frames: Default::default(), shown: None, latency: FIELD_S, arrivals: Default::default(), extra: None, counts: [0; 4], since: Instant::now(), capture, gpu: None, staged: None, next: None, presented: None, encode_ms: 5.0, gpu_ms: [12.0, 10.0], display_ms: 1.0, gpu_sum: [0.0; 2], gpu_n: [0; 2], unapplied: Default::default(), recorder: Recorder::start(device.clone(), queue.clone())?, recording: None })
    }

    /// Whether frames wait to be shown on the timeline.
    fn running(&self) -> bool {
        !self.frames.is_empty()
    }

    /// The last refresh noted (`Refreshes`), from which the refreshes are whole periods; with none noted yet (a display off or
    /// occluded from the start: no vertical blank to wait for), the timeline's start, so that pictures go on at the display's
    /// rate and the game with them. None without a display's vertical blank at all (no timeline).
    fn grid(&self) -> Option<Instant> {
        self.refreshes.as_ref().map(|r| r.last().unwrap_or(self.t0))
    }

    /// The display's first refresh after `t` (the one a picture presented at `t` shows at).
    fn refresh_after(&self, t: Instant) -> Instant {
        let g = self.grid().expect("a refresh noted");
        let n = if t < g { 0.0 } else { ((t - g).as_secs_f64() / self.period).floor() + 1.0 };
        g + Duration::from_secs_f64(n * self.period)
    }

    /// The display's first refresh at `t` or after it (within a fifth of a millisecond: a time a hair past a refresh, from
    /// times rounded on their way, keeps it).
    fn refresh_from(&self, t: Instant) -> Instant {
        let g = self.grid().expect("a refresh noted");
        let n = if t < g { 0.0 } else { ((t - g).as_secs_f64() - 0.0002).max(0.0) / self.period }.ceil();
        g + Duration::from_secs_f64(n * self.period)
    }

    /// Until the picture on the GPU is done (a millisecond at most); with none on it, until the next step is due (a picture held
    /// for its refresh presented, the next picture recorded) or the next refresh, whichever is first.
    fn wait(&mut self, screen: &Screen) -> Result<(), String> {
        if let Some(p) = self.gpu.as_mut().filter(|p| p.done.is_none()) {
            if finished(&screen.device, p.index.as_ref().expect("submitted"), Duration::from_millis(1))? {
                p.done = Some(Instant::now());
            }
            return Ok(());
        }
        let now = Instant::now();
        if self.grid().is_none() {
            std::thread::sleep(Duration::from_millis(1));
            return Ok(());
        }
        let mut until = self.refresh_after(now);
        if let Some(p) = &self.gpu {
            until = until.min(self.present_from(p.at));
        }
        if self.staged.is_none()
            && self.recording.is_none()
            && let Some((due, _)) = self.next.or_else(|| self.plan(now))
        {
            until = until.min(due);
        }
        if until > now {
            if self.recording.is_some() {
                self.collect(Some(until - now))?;
            } else {
                std::thread::sleep(until - now);
            }
        }
        Ok(())
    }

    /// The recorder's picture, once drawn (waited for `wait` at most; None: not waited for), into `staged`.
    fn collect(&mut self, wait: Option<Duration>) -> Result<(), String> {
        if self.recording.is_none() {
            return Ok(());
        }
        let r = match wait {
            Some(w) => match self.recorder.done.recv_timeout(w) {
                Ok(r) => r,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(()),
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return Err("play: the recorder stopped".into()),
            },
            None => match self.recorder.done.try_recv() {
                Ok(r) => r,
                Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => return Err("play: the recorder stopped".into()),
            },
        }?;
        let mut p = self.recording.take().expect("checked above");
        p.cmd.extend(r.cmd);
        p.profile = r.profile;
        p.encode_ms = r.ms;
        p.fg = r.fg;
        fold(&mut self.encode_ms, r.ms);
        if frame_times() {
            println!("frame stages: recorded {} {:.3}", p.field, r.ms);
        }
        self.staged = Some(p);
        Ok(())
    }

    /// The next picture's recording time and the refresh it is drawn for (None: no frame to draw it from). The refresh: the first its GPU time (the 90th percentile of the recent times; a frame's first picture's
    /// when its time falls in a frame with no picture yet: `first_at`) and a margin (a fifth and a millisecond) reach from when
    /// the GPU can take it, and no sooner than the pace's next (`step` refreshes after the last picture's: UNCAPPED has no pace
    /// but the least step, a picture at every refresh the GPU keeps up with, and one a refresh further out when it does not). The
    /// GPU takes it once the picture on it is shown (done, from the refresh before its own) and the window has its copy, and once
    /// it is recorded (on the recorder's thread, beside the picture on the GPU). It is recorded no sooner than the latency allows
    /// (`lead`: the frame it shows is there): the slack before its refresh is on the GPU's side, so a picture slow on the GPU
    /// still makes its refresh, and one quick waits for it. The picture is then presented the refresh before its own
    /// (`present_from`), so it shows at the refresh its game time was drawn for, in every mode.
    fn plan(&self, now: Instant) -> Option<(Instant, Instant)> {
        self.frames.back()?;
        let ms = |v: f64| Duration::from_secs_f64(v.max(0.0) / 1e3);
        let (encode, display, lead) = (ms(self.encode_ms), ms(self.display_ms), Duration::from_secs_f64(self.lead()));
        let step = self.step.max(least_step());
        // The pace's next: the display's refresh nearest `step` periods after the last picture's, on the refreshes as they are now.
        // Counted from the last picture's planned refresh alone it never met the display's refreshes again: a display whose real
        // rate is a little off its nominal one (the the second PC's 144 Hz one, on) drifted it off them by a fraction a picture, and at one
        // refresh in 6 half the pictures were presented before the refresh they were drawn for and shown one early.
        let pace = self.shown.map(|(_, s)| self.refresh_from(s + Duration::from_secs_f64(self.period * (step as f64 - 0.5))));
        // The refresh after the one the picture before takes (presented at `t`): a picture drawn for an earlier refresh would be
        // held past it (`present_from`) and shown a refresh late, and the one after it planned from that refresh again, so one
        // late picture left every picture after it late (the report 157844 player's RTX 4080 SUPER at 165 Hz, every picture
        // of 330 a refresh late for seconds at a time, at 2 to 3 ms a picture on the GPU). The plan goes on from where the
        // pictures are shown.
        let next = |t: Instant| self.refresh_after(t) + Duration::from_secs_f64(self.period);
        let reach = |start: Instant, floor: Option<Instant>| {
            let at = |gpu_ms: f64| {
                let at = self.refresh_from(start + ms(gpu_ms) + ms(gpu_ms / 5.0 + 1.0));
                let at = pace.map_or(at, |p| at.max(p));
                floor.map_or(at, |f| at.max(f))
            };
            let after = at(self.gpu_p90[1]);
            if self.first_at(after) { at(self.gpu_p90[0]) } else { after }
        };
        let Some(p) = &self.gpu else {
            let at = reach(now + encode, self.presented.map(next));
            return Some((at.checked_sub(lead).unwrap_or(now).max(now), at));
        };
        let shown = p.done.unwrap_or_else(|| p.submitted.expect("submitted") + ms(self.gpu_ms[!p.first as usize])).max(now).max(self.present_from(p.at));
        // The picture on the GPU is presented after the window's copy of it.
        let at = reach((shown + display).max(now + encode), Some(next(shown + display)));
        Some((at.checked_sub(lead).unwrap_or(now).max(now), at))
    }

    /// How long before its refresh a picture may be recorded (s): its recording's and GPU time as a frame's first picture, with
    /// `plan`'s margin, and a refresh (the refresh a picture reaches is up to one after that).
    fn lead(&self) -> f64 {
        (self.encode_ms + 1.2 * self.gpu_p90[0] + 1.0) / 1e3 + self.period
    }

    /// When field `field` began on the fitted clock (`fit`).
    fn field_time(&self, field: u64) -> Instant {
        self.t0 + Duration::from_secs_f64((self.fit_mean.0 + (field as f64 - self.fit_mean.1) * self.rate).max(0.0))
    }

    /// The latency the frames need (s): the time from a field's start on the fitted clock to the refresh its picture shows at. A
    /// picture between two frames shows a field behind the newest it needs, and with it the time most frames (9 in 10: a rare late one holds the picture a refresh) took from their
    /// field's start to the presenter, and the longest a picture may be recorded before its refresh (`lead`), and 1 ms.
    fn need(&self) -> f64 {
        let mut a: Vec<f64> = self.arrivals.iter().copied().collect();
        a.sort_by(f64::total_cmp);
        // At most two fields: the timeline keeps three frames (the renderer's frame sets are five), so a frame later than that
        // is passed by whatever the latency is (a level's first frames, held up for seconds while its pipelines are made).
        let late = a.get(((a.len() as f64 * 0.9) as usize).min(a.len().saturating_sub(1))).copied().unwrap_or(0.0).clamp(0.0, 2.0 * self.rate);
        self.rate + self.lead() + late + 0.001
    }

    /// The latency moved toward what the frames need, for a picture `at` the refresh: it holds inside a dead band (up when the
    /// need passes it by 3 ms, at most 2 % of the time since the last picture, so the game's time slows by no more; down when it
    /// passes the need by 8 ms, at most 1 %): the game's time does not follow the arrivals' noise. At once with no picture before
    /// it (the first, or after a cut).
    fn slew_latency(&mut self, at: Instant) {
        let need = self.need();
        match self.shown {
            None => self.latency = need,
            Some((_, s)) => {
                let dt = at.saturating_duration_since(s).as_secs_f64();
                // Never more than two fields above the need: the game's time follows the latency by 2 % at most, so this moves
                // the target and not the picture.
                self.latency = self.latency.min(need + 2.0 * self.rate);
                if need > self.latency + 0.003 {
                    self.latency += (need - self.latency).min(0.02 * dt);
                } else if self.latency > need + 0.008 {
                    self.latency -= (self.latency - need).min(0.01 * dt);
                }
            }
        }
    }

    /// The game's time (in fields) a picture at refresh `at` shows: the time the latency places there on the fitted clock (the
    /// target), reached by the clock's own rate from the last picture's, with a correction toward the target of 2 % at most (a
    /// picture's game time advances by the refreshes it spans, over the game's seconds a field, whatever the frames' arrivals
    /// do); never past the newest frame and never back: no picture is shown out of order.
    fn time_at(&self, at: Instant) -> f64 {
        let newest = self.frames.back().expect("running");
        let top = newest.field as f64;
        let target = top + (at.saturating_duration_since(self.field_time(newest.field)).as_secs_f64() - self.latency) / self.rate;
        match self.shown {
            None => target.min(top),
            Some((g0, s)) => {
                let step = at.saturating_duration_since(s).as_secs_f64() / self.rate;
                let g = g0 + step;
                (g + (target - g).clamp(-0.02 * step, 0.02 * step)).min(top).max(g0)
            }
        }
    }

    /// Whether a picture at refresh `at` is its frame's first (it traces the frame's lighting): the frame whose interval holds
    /// its time (`record`) has no picture yet.
    fn first_at(&self, at: Instant) -> bool {
        let j = self.time_at(at).ceil() as u64;
        let f = self.frames.iter().find(|f| f.field >= j).or(self.frames.back()).expect("running");
        f.replay.prepared && !f.replay.rendered.load(Ordering::Acquire)
    }

    /// The pace for the recent GPU times (`step`): the fastest step whose pictures' 90th percentile GPU time and the window's copy
    /// take under 93 % of its interval (and whose recording and copy fit it), down to 15 pictures a second, whether or not its
    /// pictures are as many as the game's frames: a PC that cannot keep up shows a steady lower rate between the game's frames
    /// (the user, 28 November 2026: "Steady pacing full stop across all FPS ranges, as well as below 60"), where each game frame
    /// shown once as it came had the GPU's uneven intervals. A step whose interval holds a field or more draws each picture in a
    /// frame of its own (each traces its frame's lighting: a frame's first picture's time). Held to 49 pictures in 50, a GPU that
    /// had shown 29 a second unevenly fell to 18 (the user: "evenness shouldnt come at the cost of that many frames"); at 9 in 10
    /// a picture in some thirty is a refresh late. UNCAPPED has no step but the least
    /// while a frame's first picture fits the game's own rate (a picture at every refresh the GPU keeps up with), and the steps
    /// below it beyond that. The times are the last second's pictures' (`Recent`): none is judged while the last second had no
    /// frame's first picture (the game sent no frame). A slower pace is taken once the criterion has failed for three seconds
    /// (`SLOWER_AFTER`), a faster one once it has passed with a quarter to spare for three or with half to spare for half a
    /// second (`Holds`); the first is chosen at once from the first 30 pictures.
    fn cadence(&mut self, field: u64) {
        let (hz, fps) = (1.0 / self.period, 1.0 / FIELD_S);
        if self.gpu_ring[2].len() < 30 || self.recent[0].len() == 0 || self.recent[1].len() < 8 {
            return;
        }
        let (first, all) = (self.recent[0].share(0.9), self.recent[1].share(0.9));
        let least = least_step();
        let slowest = ((hz / 15.0).floor() as u32).max(least);
        let copy = |s: f64| (self.encode_ms + self.display_ms + 0.5) / 1e3 <= s;
        // The step a share `most` of its interval admits (the slowest when none does).
        let pick = |most: f64| -> u32 {
            if crate::enhance::uncapped() && (first + self.display_ms) / 1e3 * fps <= most && copy(1.0 / fps) {
                return least;
            }
            (least..=slowest)
                .find(|&s| {
                    let interval = s as f64 * self.period;
                    let gpu = if interval >= FIELD_S { first } else { all };
                    (gpu + self.display_ms) / 1e3 <= most * interval && copy(interval)
                })
                .unwrap_or(slowest)
        };
        let was = self.step;
        if !self.settled {
            self.settled = true;
            self.step = pick(0.93);
        } else {
            self.step = self.holds.next(Instant::now(), self.step, pick(0.93), pick(0.75), pick(0.5));
        }
        self.step = self.step.max(least);
        if self.step != was {
            (self.holds, self.next) = (Holds::default(), None);
            let uncapped = crate::enhance::uncapped() && self.step == least;
            println!("play: field {field}: pictures {}", if uncapped && self.step > 1 { format!("uncapped at one refresh in {} at most, DLSS frame generation making {} between", self.step, self.step - 1) } else if uncapped { "uncapped: a picture at every refresh the GPU keeps up with".to_string() } else if self.step == 1 { format!("at every refresh ({:.0} a second)", 1.0 / self.period) } else { format!("at one refresh in {} ({:.0} a second)", self.step, 1.0 / (self.period * self.step as f64)) });
        }
    }

    /// The timeline's next steps, as far as they go now: the picture on the GPU, once done, presented (from the refresh before
    /// the one it is drawn for, and never two in one refresh); then, with the GPU free, the recorded picture submitted; then, with
    /// none recorded, the next picture recorded once it is due (`plan`).
    fn pump(&mut self, screen: &mut Screen) -> Result<(), String> {
        if self.grid().is_none() {
            return Ok(());
        }
        loop {
            let mut moved = false;
            if let Some(p) = self.gpu.as_mut()
                && p.done.is_none()
                && finished(&screen.device, p.index.as_ref().expect("submitted"), Duration::ZERO)?
            {
                p.done = Some(Instant::now());
            }
            if let Some(at) = self.gpu.as_ref().filter(|p| p.done.is_some()).map(|p| p.at)
                && Instant::now() >= self.present_from(at)
            {
                let p = self.gpu.take().expect("checked above");
                self.show(screen, p)?;
                moved = true;
            }
            let recording = self.recording.is_some();
            self.collect(None)?;
            moved |= recording && self.recording.is_none();
            if self.gpu.is_none()
                && let Some(p) = self.staged.take()
            {
                self.submit(screen, p);
                moved = true;
            }
            // The picture on the GPU past the time it was expected to take: the next one, not recorded yet, planned again.
            let overrun = self.gpu.as_ref().is_some_and(|p| p.done.is_none() && Instant::now() > p.submitted.expect("submitted") + Duration::from_secs_f64(self.gpu_ms[!p.first as usize] / 1e3));
            if self.staged.is_none() && self.recording.is_none() && (self.next.is_none() || overrun) {
                self.next = self.plan(Instant::now());
            }
            if self.staged.is_none()
                && self.recording.is_none()
                && let Some((due, at)) = self.next
                && Instant::now() + Duration::from_micros(300) >= due
            {
                self.next = None;
                self.record(screen, at)?;
                moved = true;
            }
            if !moved {
                return Ok(());
            }
        }
    }

    /// Picture `p`'s commands submitted: its world on the GPU.
    fn submit(&mut self, screen: &Screen, mut p: Pending) {
        p.index = Some(screen.queue.submit(std::mem::take(&mut p.cmd)));
        p.submitted = Some(Instant::now());
        if let (Some(profiler), Some(recording)) = (&mut self.profile, p.profile.take()) { profiler.submitted(recording); }
        self.gpu = Some(p);
    }

    /// The earliest moment a picture drawn for refresh `at` is presented: after the vertical blank before `at` (it then shows at
    /// `at`), and not before the refresh the last picture presented takes (the window would replace that one unseen).
    fn present_from(&self, at: Instant) -> Instant {
        let before = at.checked_sub(Duration::from_secs_f64(self.period)).unwrap_or(at);
        match self.presented {
            Some(t) => before.max(self.refresh_after(t)),
            None => before,
        }
    }

    /// The next picture recorded (`staged`) for refresh `at` (`plan`): the game's time there, the frame whose interval holds that
    /// time, and its replay drawn there (or the frame's own picture as the renderer drew it).
    fn record(&mut self, screen: &Screen, at: Instant) -> Result<(), String> {
        self.slew_latency(at);
        let g = self.time_at(at);
        let again = self.shown.is_some_and(|(g0, _)| g <= g0);
        // The frame whose interval holds the time: frame j's from j - 1 to j. The changes of the frames passed and then of that
        // frame land now, in the frames' order, before the picture: the picture before is submitted, and the next picture is
        // recorded only after this one is, so none drawn before meets them. The passed frames' commands go at once, the frame's
        // own with the picture.
        let j = g.ceil() as u64;
        let (device, queue) = (screen.device.clone(), screen.queue.clone());
        let changes = Instant::now();
        let mut passed: Vec<crate::native::Outbox> = self.unapplied.drain(..).collect();
        while self.frames.len() > 1 && self.frames[0].field < j {
            passed.extend(self.frames.pop_front().and_then(|f| f.outbox));
        }
        for o in passed {
            o.apply(&queue);
        }
        let mut cmd = self.frames[0].outbox.take().map_or_else(Vec::new, |o| o.write_out(&queue));
        let changes_ms = changes.elapsed().as_secs_f64() * 1e3;
        let f = &self.frames[0];
        // The frame's interval: from the world frame before it (fields passed over between them, the renderer busy, are spanned).
        let from = f.replay.from.min(f.field - 1) as f64;
        let g = g.max(from);
        let alpha = ((g - from) / (f.field as f64 - from)) as f32;
        // The game's time shown is `f.field - g` fields before the frame's field began.
        let begun = f.at.checked_sub(Duration::from_secs_f64((f.field as f64 - g).max(0.0) * FIELD_S)).unwrap_or(f.at);
        // A picture between the two, under the replay's flag (a later frame laying ties out anew sets it before it writes them).
        let stale = f.replay.stale().lock().map_err(|_| "play: a replay's flag is poisoned")?;
        let between = alpha < 0.9995 && f.replay.between() && !*stale;
        let first = f.replay.prepared && !f.replay.rendered.load(Ordering::Acquire);
        let started = Instant::now();
        let profile = self.profile.as_mut().filter(|_| f.replay.prepared || between).and_then(|p| p.begin(&device, &queue, f.field, first, if between { alpha } else { 1.0 }, f.replay.size));
        if f.replay.prepared {
            // The snapshot drawn at the output's camera only (its first picture traces its lighting, the later ones reuse it), on
            // the recorder's thread; the picture is staged once its commands come back (`collect`).
            drop(stale);
            let job = RecordJob { replay: f.replay.clone(), pic: f.pic.clone(), alpha: if between { alpha } else { 1.0 }, profile };
            let (field, shape, pic) = (f.field, f.shape, f.pic.clone());
            self.recorder.jobs.send(job).map_err(|_| "play: the recorder stopped")?;
            if frame_times() {
                println!("frame stages: record {field} changes {changes_ms:.3}");
            }
            self.shown = Some((g, at));
            self.recording = Some(Pending { profile: None, cmd, index: None, submitted: None, done: None, drawn: true, fg: None, pic, shape, field, between, again, g, alpha, from: from as u64, at, first, encode_ms: 0.0, recorded: started, begun });
            return Ok(());
        }
        let (world, pic) = if between {
            if self.extra.as_ref().is_none_or(|e| e.4 != f.replay.size) {
                let (tex, depth) = crate::native::picture_textures(&device, f.replay.size);
                let (tv, dv) = (tex.create_view(&Default::default()), depth.create_view(&Default::default()));
                self.extra = Some((tex, depth, tv, dv, f.replay.size));
            }
            let (tex, depth, tv, dv, size) = self.extra.as_ref().expect("made above");
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("a picture between frames") });
            let mut done = crate::replay::Chunks::default();
            f.replay.draw(&device, &queue, &mut enc, &crate::replay::Target { tex, depth, tv, dv, size: *size }, alpha, profile.as_ref(), &mut done)?;
            done.encoder(enc)?;
            (Some(done.finish()?), tex.clone())
        } else {
            // The frame's own picture, as the renderer drew it.
            (None, f.pic.clone())
        };
        drop(stale);
        let encode_ms = started.elapsed().as_secs_f64() * 1e3;
        if frame_times() {
            println!("frame stages: record {} changes {changes_ms:.3} draw {encode_ms:.3}", f.field);
        }
        let (field, shape) = (f.field, f.shape);
        fold(&mut self.encode_ms, encode_ms);
        self.shown = Some((g, at));
        let drawn = world.is_some();
        cmd.extend(world.into_iter().flatten());
        self.staged = Some(Pending { profile: if drawn { profile } else { None }, cmd, index: None, submitted: None, done: None, drawn, fg: crate::streamline::take_begun(), pic, shape, field, between, again, g, alpha, from: from as u64, at, first, encode_ms, recorded: started, begun });
        Ok(())
    }

    /// Picture `p`, done on the GPU, into the window; its times kept (`Timeline::gpu_ms`) and logged.
    fn show(&mut self, screen: &mut Screen, p: Pending) -> Result<(), String> {
        let started = Instant::now();
        // No image to present into (the window minimised): the picture passes.
        // The window's copy submitted, then the next picture's world (recorded already) behind it, and only then the present,
        // which may wait for the display: the GPU draws the next picture meanwhile instead of standing idle (the copy is ahead
        // of it on the queue, so the window still gets the picture first).
        let mut copied = Instant::now();
        if let Some(out) = screen.acquire()? {
            let out = screen.window_copy(Some(out), &Shown::Gpu(p.pic.clone()), p.shape)?.expect("an image to present into");
            copied = Instant::now();
            if let Some(next) = self.staged.take() {
                self.submit(screen, next);
            }
            crate::streamline::presenting(p.fg);
            screen.present_now(out);
        }
        let presented = Instant::now();
        self.presented = Some(presented);
        // The refresh it shows at: the first after it is in the window.
        let shows = self.refresh_after(presented);
        let late = shows > p.at + Duration::from_secs_f64(self.period / 2.0);
        let behind = shows.saturating_duration_since(p.begun).as_secs_f64() * 1e3;
        self.behind = (self.behind.0 + behind, self.behind.1.max(behind));
        // The window's copy: what holds the next picture off the GPU (the present's wait for the display does not).
        let display_ms = (copied - started).as_secs_f64() * 1e3;
        fold(&mut self.display_ms, display_ms);
        let gpu_ms = p.done.zip(p.submitted).map_or(0.0, |(d, s)| d.saturating_duration_since(s).as_secs_f64() * 1e3);
        let k = !p.first as usize;
        if p.drawn {
            diag_fold(&DIAG_GPU, gpu_ms);
            for i in [k, 2] {
                let ring = &mut self.gpu_ring[i];
                ring.push_back(gpu_ms);
                if ring.len() > 120 {
                    ring.pop_front();
                }
                if ring.len() >= 8 {
                    let mut v: Vec<f64> = ring.iter().copied().collect();
                    v.sort_by(f64::total_cmp);
                    self.gpu_p90[i] = v[((v.len() as f64 * 0.9) as usize).min(v.len() - 1)];
                }
            }
            if p.first {
                self.recent[0].push(presented, gpu_ms);
            }
            self.recent[1].push(presented, gpu_ms);
            fold(&mut self.gpu_ms[k], gpu_ms);
            self.gpu_sum[k] += gpu_ms;
            self.gpu_n[k] += 1;
        }
        let kind = if p.between { "between" } else { "own" };
        if frame_times() {
            let t = |i: Option<Instant>| i.map_or(-1.0, |i| i.saturating_duration_since(self.t0).as_secs_f64() * 1e3);
            println!("frame stages: present {} {kind} encode {:.3} GPU {gpu_ms:.3}{} display {display_ms:.3}{} at {:.3} shows {:.3} for {:.3} recorded {:.3} submitted {:.3} done {:.3} g {:.5} lat {:.3}", p.field, p.encode_ms, if p.first { " first" } else { "" }, if late { " late" } else { "" }, t(Some(started)), t(Some(shows)), t(Some(p.at)), t(Some(p.recorded)), t(p.submitted), t(p.done), p.g, self.latency * 1e3);
        }
        let work = p.encode_ms + display_ms;
        self.work = (self.work.0 + work, self.work.1.max(work));
        self.late += late as u64;
        self.counts[0] += 1;
        self.counts[if p.between { 1 } else { 2 }] += 1;
        self.counts[3] += p.again as u64;
        let (device, queue) = (screen.device.clone(), screen.queue.clone());
        if let Some(c) = self.capture.as_mut().filter(|c| p.g >= c.from && c.taken.len() < c.count) {
            let pic = &p.pic;
            let copy = device.create_texture(&wgpu::TextureDescriptor { label: Some("presented picture copy"), size: pic.size(), mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: pic.format(), usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC, view_formats: &[] });
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("presented picture copy") });
            enc.copy_texture_to_texture(pic.as_image_copy(), copy.as_image_copy(), pic.size());
            queue.submit([enc.finish()]);
            let first = *c.first.get_or_insert(p.at);
            c.taken.push((copy, format!("{} game time {:.3}: field {}, {:.3} of the way from field {} ({kind}), at {:.2} ms", c.taken.len(), p.g, p.field, p.alpha, p.from, p.at.saturating_duration_since(first).as_secs_f64() * 1e3)));
            if c.taken.len() == c.count {
                write_capture(&device, &queue, c)?;
            }
        }
        let secs = started.duration_since(self.since).as_secs_f64();
        if secs >= 2.0 {
            let [n, b, own, twice] = self.counts;
            let avg = |k: usize| self.gpu_sum[k] / self.gpu_n[k].max(1) as f64;
            println!("play: field {}: {:.1} pictures a second at the display's {:.2} Hz: {b} between two of the game's frames, {own} the game's own, {twice} shown twice, {} shown a refresh late; the GPU {:.2} ms a frame's first picture ({}), {:.2} ms a picture after ({}); the game's time shown {:.1} ms after its field began (at most {:.1}; the timeline's latency {:.1}); the presenter's time a picture {:.2} ms, at most {:.2}; {:.0} MB private", p.field, n as f64 / secs, 1.0 / self.period, self.late, avg(0), self.gpu_n[0], avg(1), self.gpu_n[1], self.behind.0 / n.max(1) as f64, self.behind.1, self.latency * 1e3, self.work.0 / n.max(1) as f64, self.work.1, crate::native::private_mb());
            self.counts = [0; 4];
            self.work = (0.0, 0.0);
            self.behind = (0.0, 0.0);
            self.late = 0;
            self.gpu_sum = [0.0; 2];
            self.gpu_n = [0; 2];
            self.since = started;
        }
        self.cadence(p.field);
        Ok(())
    }

    /// Every picture on its way waited for and shown, in order: before a picture drawn outside the timeline's steps, whose
    /// commands then follow theirs (they share the snapshots' view buffers and traced lighting).
    fn drain(&mut self, screen: &mut Screen) -> Result<(), String> {
        loop {
            if let Some(mut p) = self.gpu.take() {
                if p.done.is_none() {
                    if !finished(&screen.device, p.index.as_ref().expect("submitted"), Duration::from_secs(10))? {
                        return Err("play: the GPU did not finish a picture in 10 s".into());
                    }
                    p.done = Some(Instant::now());
                }
                self.show(screen, p)?;
            }
            while self.recording.is_some() {
                self.collect(Some(Duration::from_secs(10)))?;
            }
            match self.staged.take() {
                Some(p) => self.submit(screen, p),
                None => return Ok(()),
            }
        }
    }

    /// The newest frame's own picture (at its own time, not between two frames), drawn now when the frame is a scene snapshot,
    /// after every picture on its way: what a pause's menus are drawn over and what `shot` takes. None with no frame on the
    /// timeline.
    fn own_picture(&mut self, screen: &mut Screen) -> Result<Option<(wgpu::Texture, (u32, u32))>, String> {
        if !self.running() {
            return Ok(None);
        }
        self.drain(screen)?;
        // Every frame's changes, in order, up to the newest's; the frames before it are passed (their changes would not be
        // undone for a picture of theirs).
        let queue = screen.queue.clone();
        for o in self.unapplied.drain(..) {
            o.apply(&queue);
        }
        while self.frames.len() > 1 {
            if let Some(o) = self.frames.pop_front().and_then(|f| f.outbox) {
                o.apply(&queue);
            }
        }
        if let Some(o) = self.frames[0].outbox.take() {
            o.apply(&queue);
        }
        let f = self.frames.back().expect("running");
        let alpha = 1.0;
        if f.replay.prepared {
            let (device, queue) = (screen.device.clone(), screen.queue.clone());
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("the game's own picture") });
            let mut done = crate::replay::Chunks::default();
            f.replay.draw(&device, &queue, &mut enc, &crate::replay::Target { tex: &f.pic, depth: &f.replay.output_depth, tv: &f.replay.output_view, dv: &f.replay.output_depth_view, size: f.replay.size }, alpha, None, &mut done)?;
            done.encoder(enc)?;
            queue.submit(done.finish()?);
        }
        if frame_times() {
            println!("frame stages: own picture {} at alpha {alpha:.3} ({})", f.field, if f.replay.prepared { "its snapshot drawn now" } else { "as the renderer drew it" });
        }
        Ok(Some((f.pic.clone(), f.shape)))
    }

    /// The timeline ends (a frame shown as it is drawn: a menu, a movie, the 2D layer); the next world frame begins it again. The
    /// newest frame's own picture, when it ran.
    fn stop(&mut self, screen: &mut Screen) -> Result<Option<(wgpu::Texture, (u32, u32))>, String> {
        let newest = self.own_picture(screen)?;
        for o in self.unapplied.drain(..) {
            o.apply(&screen.queue);
        }
        self.frames.clear();
        self.shown = None;
        self.next = None;
        self.fit.clear();
        Ok(newest)
    }

    /// Frame `field` (its field began at `at`, the renderer handed it over at `ready`) with its changes to the shared GPU state
    /// (`outbox`): onto the timeline.
    #[allow(clippy::too_many_arguments)]
    fn push(&mut self, field: u64, at: Instant, ready: Instant, pic: wgpu::Texture, shape: (u32, u32), replay: crate::replay::Replay, outbox: crate::native::Outbox) {
        // A cut (the field goes back, or the frames stopped for half a second: a level change, a menu) begins the clock and the
        // game's time again. A frame that does not follow the last one within a few fields (`replay::MAX_GAP`: fields passed over
        // while the renderer was busy are spanned) only begins the timeline's frames again (the frames passed keep their changes'
        // order): the game's time shown goes on across it, as the clock says.
        let t = at.saturating_duration_since(self.t0).as_secs_f64();
        let cut = self.fit.back().is_some_and(|&(lt, lf)| (field as f64) <= lf || t - lt > 0.5);
        if cut {
            self.fit.clear();
            self.shown = None;
        }
        // A frame whose field began well off the line (`OFF_LINE`: the PC held the game for a moment, a stall, or the game thread
        // began fields late for a while) begins the line again from it, and the game's time shown goes on from the last picture's
        // from now. Kept, the line through the step was the step's, not the game's, for its 120 frames (two seconds): the game's
        // time shown ran slow, caught the newest frame, stopped and jumped (the game frozen 400 ms: speeds of 0.13, then 0.85 to
        // 1.13 in turn for two seconds, single pictures 2.24 times the game's; a v0.1.3 player: "random slowdowns followed by fast
        // catch-up surges with character, camera, and scenery shaking"). The last picture's game time is not advanced across the
        // step: the step was the display holding still, already.
        else if self.fit.len() >= 16 && (t - (self.fit_mean.0 + (field as f64 - self.fit_mean.1) * self.rate)).abs() > OFF_LINE {
            self.fit.clear();
            self.shown = self.shown.map(|(g, _)| (g, ready));
        }
        if self.frames.back().is_some_and(|b| cut || field - b.field > crate::replay::MAX_GAP) {
            for f in self.frames.drain(..) {
                self.unapplied.extend(f.outbox);
            }
            self.next = None;
        }
        // The game's fields on the wall clock: the line through the last 120 frames' field starts (a field the game thread began
        // late is not made up, so its time slips, and the line's slope is the game's real seconds a field).
        self.fit.push_back((t, field as f64));
        while self.fit.len() > 120 {
            self.fit.pop_front();
        }
        let n = self.fit.len() as f64;
        let (tm, fm) = (self.fit.iter().map(|p| p.0).sum::<f64>() / n, self.fit.iter().map(|p| p.1).sum::<f64>() / n);
        let (cov, var) = self.fit.iter().fold((0.0, 0.0), |(c, v), &(t, f)| (c + (f - fm) * (t - tm), v + (f - fm) * (f - fm)));
        self.fit_mean = (tm, fm);
        self.rate = if self.fit.len() >= 16 && var > 0.0 { (cov / var).clamp(FIELD_S, 1.4 * FIELD_S) } else { FIELD_S };
        // How late the frame is on that clock: the time from its field's start on the line to the presenter.
        self.arrivals.push_back(ready.saturating_duration_since(self.field_time(field)).as_secs_f64());
        while self.arrivals.len() > 120 {
            self.arrivals.pop_front();
        }
        self.frames.push_back(TimelineFrame { field, at, pic, shape, replay: Arc::new(replay), outbox: (!outbox.is_empty()).then_some(outbox) });
        // The presenter behind by more than two frames: the oldest are passed.
        while self.frames.len() > 3 {
            let passed = self.frames.pop_front().and_then(|f| f.outbox);
            self.unapplied.extend(passed);
        }
    }
}

/// The captured pictures (`Capture`) as PNG files `pNNN.png` and their times in `presented.txt` in the capture's folder.
fn write_capture(device: &wgpu::Device, queue: &wgpu::Queue, c: &Capture) -> Result<(), String> {
    std::fs::create_dir_all(&c.dir).map_err(|e| format!("{}: {e}", c.dir))?;
    let mut lines = Vec::new();
    for (k, (tex, line)) in c.taken.iter().enumerate() {
        let (w, h) = (tex.width(), tex.height());
        let row = (4 * w).div_ceil(256) * 256;
        let buf = device.create_buffer(&wgpu::BufferDescriptor { label: Some("presented picture"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("presented picture") });
        enc.copy_texture_to_buffer(tex.as_image_copy(), wgpu::TexelCopyBufferInfo { buffer: &buf, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } }, tex.size());
        queue.submit([enc.finish()]);
        let slice = buf.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("play: capture: GPU wait: {e}"))?;
        rx.recv().map_err(|e| format!("play: capture: {e}"))?.map_err(|e| format!("play: capture: {e}"))?;
        let bytes = slice.get_mapped_range().map_err(|e| format!("play: capture: {e}"))?;
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h as usize {
            for px in bytes[y * row as usize..y * row as usize + 4 * w as usize].chunks_exact(4) {
                rgb.extend_from_slice(&px[..3]);
            }
        }
        drop(bytes);
        buf.unmap();
        let file = format!("{}/p{k:03}.png", c.dir);
        std::fs::write(&file, crate::pic::png(&crate::pic::Image { w: w as usize, h: h as usize, rgb })).map_err(|e| format!("{file}: {e}"))?;
        lines.push(line.clone());
    }
    std::fs::write(format!("{}/presented.txt", c.dir), lines.join("\n") + "\n").map_err(|e| format!("{}: {e}", c.dir))?;
    println!("play: {} pictures shown written to {}", c.taken.len(), c.dir);
    Ok(())
}

/// The window's display and window handles, taken on the window's thread.
#[derive(Clone, Copy)]
struct Handles(winit::raw_window_handle::RawDisplayHandle, winit::raw_window_handle::RawWindowHandle);

// SAFETY: a Win32 window handle is a plain value that any thread may present to (the surface is made and used on the renderer's).
unsafe impl Send for Handles {}

/// A frame's picture: the native one on the GPU, or a movie's.
enum Shown {
    Gpu(wgpu::Texture),
    Picture(crate::cinema::Picture),
}

/// The window's surface, on the native renderer's GPU, and what draws a picture into it.
struct Screen {
    device: wgpu::Device,
    queue: wgpu::Queue,
    window: Arc<Window>,
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The presenter's own copy of a picture (a movie's, or a native picture the window could not show when it came), its bind
    /// group and size.
    frame: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
    /// The native pictures sampled where they are (`window_copy`): a bind group by texture, the newest last (the frame sets'
    /// pictures, the timeline's, the display buffer; at most `PICTURE_GROUPS`).
    pictures: Vec<(wgpu::Texture, wgpu::BindGroup)>,
    /// What the window showed last, its bind group and size (`shot`).
    last: Option<(wgpu::BindGroup, (u32, u32))>,
    /// Where the picture is in the window, for the mouse (`Kbm::viewport`).
    kbm: Arc<Mutex<crate::input::Kbm>>,
    /// A movie's upscale: the edge-directed pass (the movie to the picture's size in the window, into `up`) and the sharpening
    /// pass (`up` into the window, pixel for pixel).
    easu: wgpu::RenderPipeline,
    rcas: wgpu::RenderPipeline,
    up: Option<(wgpu::Texture, wgpu::BindGroup, (u32, u32))>,
    /// The quick-select ring's pointer (`pointer`).
    pointer: wgpu::RenderPipeline,
    /// The FPS counter (`Fps`).
    fps: Fps,
    /// The settings page's tab bar (`TabBar`).
    tab_bar: TabBar,
    /// The update notice's plate on the main menu (`NoticePlate`).
    notice: NoticePlate,
    /// The window's size, the surface's and the game's view widening last said in the log, and whether the surface was made
    /// again for a suboptimal image at this size (`acquire`).
    shape_said: (u32, u32, u32, u32, f32),
    suboptimal_redone: bool,
}

/// The most native pictures the window keeps a bind group for (`Screen::picture_group`): the frame sets', the timeline's and the
/// display buffer's.
const PICTURE_GROUPS: usize = 16;

/// The FPS counter's figures as drawn in its atlas: their cap height in pixels.
const FPS_CAP: f32 = 48.0;

/// Letters rasterised once into an atlas row, as `text.rs` rasterises the game's letters, in Bahnschrift Bold as the port's own
/// text, with a cell of full coverage for boxes; and the pipeline that draws them, premultiplied, over the window's picture (the
/// FPS counter, `Fps`; the settings page's tab bar, `TabBar`).
struct Glyphs {
    pipeline: wgpu::RenderPipeline,
    group: wgpu::BindGroup,
    /// Each character's cell in the atlas (pixels: x0, y0, x1, y1), its place from the pen on the baseline, and its advance.
    glyphs: std::collections::HashMap<char, FpsGlyph>,
    atlas: (u32, u32),
    /// The cell of full coverage (pixels), a box's.
    solid: [f32; 4],
}

impl Glyphs {
    /// The characters `chars` (and the space) at a cap height of `FPS_CAP` pixels.
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat, chars: &str, label: &str) -> Result<Glyphs, String> {
        use ab_glyph::{Font, FontVec, PxScale, ScaleFont, VariableFont, point};
        let path = format!("{}/Fonts/bahnschrift.ttf", std::env::var("WINDIR").map_err(|_| format!("{label}: no WINDIR"))?);
        let mut font = FontVec::try_from_vec(std::fs::read(&path).map_err(|e| format!("{label}: {path}: {e}"))?).map_err(|e| format!("{label}: {path}: {e}"))?;
        font.set_variation(b"wght", 700.0);
        font.set_variation(b"wdth", 100.0);
        let h_cap = font.outline_glyph(font.glyph_id('H').with_scale_and_position(PxScale::from(100.0), point(0.0, 0.0))).ok_or_else(|| format!("{label}: no 'H'"))?.px_bounds().height();
        let px = PxScale::from(100.0 * FPS_CAP / h_cap);
        let sf = font.as_scaled(px);
        const PAD: u32 = 4;
        // The solid cell first, 8 pixels a side.
        const SOLID: u32 = 8;
        let mut glyphs = std::collections::HashMap::new();
        let mut outlines = Vec::new();
        let (mut x, mut rows) = (PAD + SOLID + PAD, SOLID);
        for ch in chars.chars().filter(|c| *c != ' ') {
            if glyphs.contains_key(&ch) {
                continue;
            }
            let id = sf.glyph_id(ch);
            let o = font.outline_glyph(id.with_scale_and_position(px, point(0.0, 0.0))).ok_or_else(|| format!("{label}: no {ch:?} in {path}"))?;
            let b = o.px_bounds();
            let (w, h) = (b.width().ceil() as u32, b.height().ceil() as u32);
            glyphs.insert(ch, FpsGlyph { cell: [x as f32, PAD as f32, (x + w) as f32, (PAD + h) as f32], offset: [b.min.x, b.min.y], advance: sf.h_advance(id) });
            outlines.push((o, x));
            x += w + PAD;
            rows = rows.max(h);
        }
        glyphs.insert(' ', FpsGlyph { cell: [0.0; 4], offset: [0.0; 2], advance: sf.h_advance(sf.glyph_id(' ')) });
        let (aw, ah) = (x.div_ceil(256) * 256, rows + 2 * PAD);
        let mut texels = vec![0u8; (aw * ah) as usize];
        for y in PAD..PAD + SOLID {
            texels[(y * aw + PAD) as usize..(y * aw + PAD + SOLID) as usize].fill(255);
        }
        for (o, x0) in &outlines {
            o.draw(|gx, gy, c| texels[((PAD + gy) * aw + x0 + gx) as usize] = (c.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
        let tex = device.create_texture(&wgpu::TextureDescriptor { label: Some(label), size: wgpu::Extent3d { width: aw, height: ah, depth_or_array_layers: 1 }, mip_level_count: 1, sample_count: 1, dimension: wgpu::TextureDimension::D2, format: wgpu::TextureFormat::R8Unorm, usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST, view_formats: &[] });
        queue.write_texture(wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All }, &texels, wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(aw), rows_per_image: Some(ah) }, wgpu::Extent3d { width: aw, height: ah, depth_or_array_layers: 1 });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(FPS_SHADER.into()) });
        let attrs = wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(label),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<FpsVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: crate::pipelines::cache(),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some(label), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let view = tex.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout: &pipeline.get_bind_group_layout(0), entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&sampler) }] });
        let solid = [(PAD + 2) as f32, (PAD + 2) as f32, (PAD + SOLID - 2) as f32, (PAD + SOLID - 2) as f32];
        Ok(Glyphs { pipeline, group, glyphs, atlas: (aw, ah), solid })
    }

    /// The rectangle (x0, y0, x1, y1, pixels of a `size` picture) of atlas cell `cell`, in `colour`, as two triangles.
    fn quad(&self, verts: &mut Vec<FpsVertex>, size: (f32, f32), [x0, y0, x1, y1]: [f32; 4], cell: [f32; 4], colour: [f32; 4]) {
        let ndc = |x: f32, y: f32| [x / size.0 * 2.0 - 1.0, 1.0 - y / size.1 * 2.0];
        let uv = |u: f32, v: f32| [u / self.atlas.0 as f32, v / self.atlas.1 as f32];
        let corner = [(x0, y0, cell[0], cell[1]), (x1, y0, cell[2], cell[1]), (x0, y1, cell[0], cell[3]), (x1, y1, cell[2], cell[3])];
        for k in [0, 1, 2, 1, 3, 2] {
            let (x, y, u, v) = corner[k];
            verts.push(FpsVertex { pos: ndc(x, y), uv: uv(u, v), colour });
        }
    }

    /// The width of `text` at a cap height `cap` pixels high.
    fn width(&self, text: &str, cap: f32) -> f32 {
        text.chars().map(|ch| self.glyphs[&ch].advance * cap / FPS_CAP).sum()
    }

    /// `text` with its pen starting at `x` on baseline `base`, its cap height `cap` pixels, in `colour`.
    fn text(&self, verts: &mut Vec<FpsVertex>, size: (f32, f32), text: &str, (x, base): (f32, f32), cap: f32, colour: [f32; 4]) {
        let scale = cap / FPS_CAP;
        let mut pen = x;
        for ch in text.chars() {
            let g = self.glyphs[&ch];
            if g.cell[2] > g.cell[0] {
                let (x0, y0) = (pen + g.offset[0] * scale, base + g.offset[1] * scale);
                self.quad(verts, size, [x0, y0, x0 + (g.cell[2] - g.cell[0]) * scale, y0 + (g.cell[3] - g.cell[1]) * scale], g.cell, colour);
            }
            pen += g.advance * scale;
        }
    }

    /// `verts` drawn over `view`.
    fn draw(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, verts: &[FpsVertex], label: &str) {
        use wgpu::util::DeviceExt;
        if verts.is_empty() {
            return;
        }
        let vb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents: bytemuck::cast_slice(verts), usage: wgpu::BufferUsages::VERTEX });
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some(label),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &self.group, &[]);
        pass.set_vertex_buffer(0, vb.slice(..));
        pass.draw(0..verts.len() as u32, 0..1);
    }
}

/// The FPS counter (the DISPLAY tab's FPS COUNTER, `enhance::fps_counter`; the player, 17 November 2026: "Add an FPS counter and
/// FPS counter option in settings"): how many frames the window showed in the last second, taken twice a second (with FRAME
/// GENERATION, DLSS-G's, the frames it generated included: `streamline::shown`; the player, 24 November 2026, at 2X: "The in game
/// FPS counter didnt show 347", the frames DLSS-G showed, as it counted only the port's own pictures), in its top
/// right corner over everything (the top left's is the HUD's weapon panel), white on a dark shadow, in Bahnschrift Bold as the port's own text (`text.rs`).
///
/// FPS COUNTER's DETAILED (`enhance::diagnostics`) adds the diagnostics under it, smaller, taken with the figure (the report
/// 155269 player: "an optional diagnostics overlay (shader compile status, frame time, VRAM) so players and bug reports can see
/// what's happening"): the game's own thread's ms a field, the frame builder's ms a frame and the GPU's ms a picture (`DIAG_*`),
/// the video memory in use of the OS's budget (`enhance::vram`) and the memory of the PC the game holds now (its working set: the private commit also counts the GPU memory this PC's driver charges to it, `native::working_set_mb`). Every shader is made before
/// play, so there is no compile status to show.
struct Fps {
    g: Glyphs,
    /// When the window showed each picture of the last second and how many frames it showed for it; the figure shown and when
    /// it was taken.
    shown: std::collections::VecDeque<(Instant, u32)>,
    value: usize,
    taken: Instant,
    /// The diagnostics' lines as taken with the figure (DETAILED only).
    detail: Vec<String>,
}

#[derive(Clone, Copy)]
struct FpsGlyph {
    cell: [f32; 4],
    offset: [f32; 2],
    advance: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FpsVertex {
    pos: [f32; 2],
    uv: [f32; 2],
    colour: [f32; 4],
}

const FPS_SHADER: &str = r#"
@group(0) @binding(0) var glyphs: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct VIn { @location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) colour: vec4<f32> };
struct VOut { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) colour: vec4<f32> };
@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.p = vec4<f32>(v.pos, 0.0, 1.0);
    o.uv = v.uv;
    o.colour = v.colour;
    return o;
}
// The letter's coverage times the colour, premultiplied.
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let a = textureSample(glyphs, smp, i.uv).r * i.colour.a;
    return vec4<f32>(i.colour.rgb * a, a);
}
"#;

impl Fps {
    /// The figures, the space, the point and the slash, and the letters of FPS and the diagnostics' names and units.
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Result<Fps, String> {
        Ok(Fps { g: Glyphs::new(device, queue, format, "0123456789./FPSGAMERUVB", "fps counter")?, shown: std::collections::VecDeque::new(), value: 0, taken: Instant::now(), detail: Vec::new() })
    }

    /// The window showed a picture at `now`, and as many frames for it as the last picture presented had (`streamline::shown`).
    fn count(&mut self, now: Instant) -> Result<(), String> {
        self.shown.push_back((now, crate::streamline::shown()));
        while self.shown.front().is_some_and(|&(t, _)| now - t > Duration::from_secs(1)) {
            self.shown.pop_front();
        }
        if now - self.taken >= Duration::from_millis(500) {
            (self.value, self.taken) = (self.shown.iter().map(|&(_, n)| n as usize).sum(), now);
            DIAG_FPS.store(self.value as u32, Ordering::Relaxed);
            self.detail.clear();
            if crate::enhance::diagnostics() {
                let (vram, budget) = crate::enhance::vram()?;
                let gb = |mb: f64| mb / 1024.0;
                self.detail = vec![
                    format!("GAME {:.1} MS", diag(&DIAG_GAME)),
                    format!("FRAME {:.1} MS", diag(&DIAG_FRAME)),
                    format!("GPU {:.1} MS", diag(&DIAG_GPU)),
                    format!("VRAM {:.1}/{:.1} GB", gb(vram), gb(budget)),
                    format!("RAM {:.1} GB", gb(crate::native::working_set_mb())),
                ];
            }
        }
        Ok(())
    }

    /// The counter into `view` (`size` pixels), when the option is on: its cap height 2.2 % of the height, as far from the top
    /// and the right as 1.2 % of it; the diagnostics' lines under it at three quarters of its height.
    fn draw(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, size: (u32, u32)) {
        if !crate::enhance::fps_counter() || size.0 == 0 || size.1 == 0 {
            return;
        }
        let (w, h) = (size.0 as f32, size.1 as f32);
        let cap = (0.022 * h).max(10.0);
        let margin = (0.012 * h).round();
        let shadow = (0.0016 * h).round().max(1.0);
        let figure = format!("{} FPS", self.value);
        let detail = if crate::enhance::diagnostics() { &self.detail[..] } else { &[] };
        let small = (cap * 0.75).max(8.0);
        let mut verts = Vec::new();
        let mut base = margin;
        for (text, cap) in std::iter::once((&figure, cap)).chain(detail.iter().map(|t| (t, small))) {
            base += cap;
            let width = self.g.width(text, cap);
            for (d, colour) in [(shadow, [0.0, 0.0, 0.0, 0.8]), (0.0, [1.0, 1.0, 1.0, 1.0])] {
                self.g.text(&mut verts, (w, h), text, (w - margin - width + d, base + d), cap, colour);
            }
            base += cap * 0.5;
        }
        self.g.draw(device, enc, view, &verts, "fps counter");
    }
}

/// The settings page's tab bar (`menus::Menus::tab_bar`), drawn by the window over the finished picture as the FPS counter is,
/// so that nothing the game draws, and no step of the picture's way to the window, hides it: a plate a tab where the page's mouse
/// takes its clicks (the 2D layer's place, the original picture's width at the picture's height in its middle: `Input::pointer`),
/// dark with a gold edge, lit amber under the pointer, the tab showing amber (the port's plates' colours, `prompts::plate_prims`),
/// its name centred on it in the game's gold letters on a dark shadow, at most nine tenths of the plate's width.
struct TabBar {
    g: Glyphs,
}

impl TabBar {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Result<TabBar, String> {
        let chars: String = crate::settings::TABS.iter().flat_map(|(_, name)| name.chars()).collect();
        Ok(TabBar { g: Glyphs::new(device, queue, format, &chars, "tab bar")? })
    }

    /// `tabs` into `view` (`size` pixels), over the picture at `viewport`.
    fn draw(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, size: (u32, u32), viewport: [f32; 4], tabs: &[crate::menus::TabShown]) {
        let [vx, vy, vw, vh] = viewport;
        if tabs.is_empty() || vw <= 0.0 || vh <= 0.0 {
            return;
        }
        let size = (size.0 as f32, size.1 as f32);
        let w2 = vh * 0.62 / 0.4805;
        let left = vx + (vw - w2) / 2.0;
        let at = |u: f32, v: f32| (left + u * w2, vy + v * vh);
        let rgb = |c: [u8; 3]| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, 1.0];
        let (bx, by) = ((0.002 * w2).max(1.0), (0.003 * vh).max(1.0));
        let shadow = (0.0016 * vh).round().max(1.0);
        let mut verts = Vec::new();
        for t in tabs {
            let (fill, edge) = match t.look {
                crate::menus::Plate::Off => ([0x14, 0x0c, 0x04], [0xe8, 0xa8, 0x30]),
                crate::menus::Plate::Lit => ([0x48, 0x2e, 0x08], [0xff, 0xe0, 0x90]),
                crate::menus::Plate::On => ([0x8a, 0x58, 0x10], [0xff, 0xf0, 0xc0]),
            };
            let ((x0, y0), (x1, y1)) = (at(t.rect[0], t.rect[1]), at(t.rect[2], t.rect[3]));
            let solid = self.g.solid;
            self.g.quad(&mut verts, size, [x0, y0, x1, y1], solid, rgb(fill));
            for e in [[x0, y0, x1, y0 + by], [x0, y1 - by, x1, y1], [x0, y0, x0 + bx, y1], [x1 - bx, y0, x1, y1]] {
                self.g.quad(&mut verts, size, e, solid, rgb(edge));
            }
            let cap = ((y1 - y0) * 0.38).min(0.9 * (x1 - x0) / self.g.width(t.name, 1.0));
            let (tx, base) = ((x0 + x1 - self.g.width(t.name, cap)) / 2.0, (y0 + y1 + cap) / 2.0);
            self.g.text(&mut verts, size, t.name, (tx + shadow, base + shadow), cap, [0.0, 0.0, 0.0, 0.7]);
            self.g.text(&mut verts, size, t.name, (tx, base), cap, rgb([0xff, 0xcc, 0x66]));
        }
        self.g.draw(device, enc, view, &verts, "tab bar");
    }
}

/// The update notice's plate on the main menu (`update::plate`; the player's word and the design in `update.rs`), drawn by the window
/// over the finished picture as the tab bar is: a plate in the settings tabs' colours at `update::PLATE` (shares of the original
/// picture), dark with a gold edge, lit amber under the pointer, its three lines centred on it in the game's gold letters on a dark
/// shadow (the first at a tab's letters' size, the second at 0.7 of it, the third dimmer at 0.6), each within nine tenths of its
/// width; it fades in over 0.3 s the first time it shows.
struct NoticePlate {
    g: Glyphs,
    first: Option<Instant>,
}

impl NoticePlate {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Result<NoticePlate, String> {
        // Every printable character: the lines hold capitals, figures, a release's "v" and stops, and a key's name (`Key::label`).
        let chars: String = ('!'..='~').collect();
        Ok(NoticePlate { g: Glyphs::new(device, queue, format, &chars, "update notice")?, first: None })
    }

    /// `plate` into `view` (`size` pixels), over the picture at `viewport`.
    fn draw(&mut self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, size: (u32, u32), viewport: [f32; 4], plate: Option<&crate::update::Shown>) {
        let [vx, vy, vw, vh] = viewport;
        let Some(p) = plate.filter(|_| vw > 0.0 && vh > 0.0) else { return };
        let alpha = (self.first.get_or_insert_with(Instant::now).elapsed().as_secs_f32() / 0.3).min(1.0);
        let size = (size.0 as f32, size.1 as f32);
        let w2 = vh * 0.62 / 0.4805;
        let left = vx + (vw - w2) / 2.0;
        let at = |u: f32, v: f32| (left + u * w2, vy + v * vh);
        let rgba = |c: [u8; 3], a: f32| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, a * alpha];
        let [u0, v0, u1, v1] = crate::update::PLATE;
        let ((x0, y0), (x1, y1)) = (at(u0, v0), at(u1, v1));
        let (bx, by) = ((0.002 * w2).max(1.0), (0.003 * vh).max(1.0));
        let shadow = (0.0016 * vh).round().max(1.0);
        // The main menu panel's own colours (its body 82, 54, 22 over the background, its highlight band 119, 68, 16, its
        // letters 255, 204, 102 and the highlighted item's near white), the edge brighter so the plate stands off the dark room:
        // in the settings tabs' near black it was a faint outline (the player: "The box looks faint").
        let (fill, edge) = if p.lit { ([0x80, 0x50, 0x16], [0xff, 0xe0, 0x90]) } else { ([0x52, 0x36, 0x16], [0xd6, 0x96, 0x34]) };
        let mut verts = Vec::new();
        let solid = self.g.solid;
        self.g.quad(&mut verts, size, [x0, y0, x1, y1], solid, rgba(fill, 0.94));
        for e in [[x0, y0, x1, y0 + by], [x0, y1 - by, x1, y1], [x0, y0, x0 + bx, y1], [x1 - bx, y0, x1, y1]] {
            self.g.quad(&mut verts, size, e, solid, rgba(edge, 1.0));
        }
        // A tab's letters are 0.38 of its 0.046 of the picture's height (`TabBar::draw`, `menus::TAB_BAR`).
        let tab = 0.046 * 0.38 * vh;
        let (gold, cream, soft) = ([0xff, 0xcc, 0x66], [0xf0, 0xe4, 0xc8], [0xe0, 0xb0, 0x5c]);
        let mut base = y0 + 0.011 * vh;
        for (k, (line, (cap, colour))) in p.lines.iter().zip([(tab, gold), (0.7 * tab, cream), (0.6 * tab, soft)]).enumerate() {
            let cap = cap.min(0.9 * (x1 - x0) / self.g.width(line, 1.0));
            base += if k == 0 { cap } else { cap + 0.0095 * vh };
            let tx = (x0 + x1 - self.g.width(line, cap)) / 2.0;
            self.g.text(&mut verts, size, line, (tx + shadow, base + shadow), cap, [0.0, 0.0, 0.0, 0.75 * alpha]);
            self.g.text(&mut verts, size, line, (tx, base), cap, rgba(colour, 1.0));
        }
        self.g.draw(device, enc, view, &verts, "update notice");
    }
}

/// The quick-select ring as the game draws it in play, in shares of the original picture (its 0.62 / 0.4805 shape at the native
/// picture's height, as every 2D element is placed): its middle, and its slots' distance from it across and up (Veldin, the ring
/// held open at 2562 x 1440: its middle at 806, 712, the slots 290 across and 280 up and down from it); and the pointer's radius
/// as a share of the picture's height.
const RING_AT: (f32, f32) = (0.2444, 0.4944);
const RING_SLOTS: (f32, f32) = (0.1561, 0.1944);
const POINTER: f32 = 0.024;

/// The shape of the game's movies: the original picture's (0.62 / 0.4805, the console's 4:3).
const MOVIE_SHAPE: f32 = 0.62 / 0.4805;

/// How much a movie is zoomed, and as much again widened, in a window wider than the movie (a v0.1.3 player: 'Intro "Press
/// Start" screen stays locked to 4:3'). The movie fills the window itself: a blurred copy of it beside the 4:3 picture duplicated
/// things at the wrong size and place, and a glow carrying its edges out still read as bars. The window's extra width is shared
/// evenly: the square root of how much wider the window is than the movie, both ways, so a 16:9 window takes the movie zoomed
/// 1.174 (7.4 % of its height cut at the top and at the bottom, inside the tenth these pictures keep clear for a television's edge)
/// and widened 1.174. At most 1.25 each: a window wider than 2:1 keeps black sides rather than cut or widen more. 1 with
/// WIDESCREEN off (the console's shape with bars, as the world is shown then), in a window as wide as the movie or narrower (the
/// whole movie across its width) and in one with no size (minimised: nothing is shown).
fn movie_zoom(win: winit::dpi::PhysicalSize<u32>) -> f32 {
    if !WIDE.load(Ordering::Relaxed) || win.width == 0 || win.height == 0 {
        return 1.0;
    }
    let wider = win.width as f32 / win.height as f32 / MOVIE_SHAPE;
    if wider <= 1.0 { 1.0 } else { wider.sqrt().min(1.25) }
}

const BLIT: &str = r#"
@group(0) @binding(0) var pic: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct VOut { @builtin(position) p: vec4<f32>, @location(0) uv: vec2<f32> };
// One triangle over the viewport.
@vertex fn vs(@builtin(vertex_index) i: u32) -> VOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VOut;
    o.p = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    return vec4<f32>(textureSample(pic, smp, i.uv).rgb, 1.0);
}
// The quick-select ring's pointer (`Screen::pointer`), drawn in a small viewport about where the mouse points (uv 0 to 1 across
// it): a white ring and dot edged in black, premultiplied, over the picture.
fn band(d: f32, lo: f32, hi: f32, aa: f32) -> f32 {
    return smoothstep(lo - aa, lo + aa, d) * (1.0 - smoothstep(hi - aa, hi + aa, d));
}
@fragment fn fs_pointer(i: VOut) -> @location(0) vec4<f32> {
    let d = length(i.uv * 2.0 - 1.0);
    let aa = max(fwidth(d), 0.02);
    let white = max(band(d, 0.62, 0.80, aa), 1.0 - smoothstep(0.16 - aa, 0.16 + aa, d));
    let edge = max(band(d, 0.50, 0.92, aa), 1.0 - smoothstep(0.28 - aa, 0.28 + aa, d));
    return vec4<f32>(vec3<f32>(white), max(white, 0.7 * edge));
}
// A movie's picture to the window: edge-directed upscaling, then sharpening at the window's size (the method of AMD FidelityFX
// Super Resolution 1, EASU and RCAS: MIT licensed, written here from its description).
fn luma(c: vec3<f32>) -> f32 {
    return c.b * 0.5 + (c.r * 0.5 + c.g);
}
fn texel(p: vec2<i32>) -> vec3<f32> {
    return textureLoad(pic, clamp(p, vec2<i32>(0), vec2<i32>(textureDimensions(pic)) - vec2<i32>(1)), 0).rgb;
}
struct Edge { dir: vec2<f32>, len: f32 };
// One of the four texels around the pixel: the direction of its brightness gradient (the '+' around it, a above, b left, c the
// texel, d right, e below) and how much of an edge it is, weighted by the pixel's bilinear weight for it.
fn edge(acc: ptr<function, Edge>, w: f32, a: f32, b: f32, c: f32, d: f32, e: f32) {
    let dx = d - b;
    let lx = clamp(abs(dx) / max(max(abs(d - c), abs(c - b)), 1e-5), 0.0, 1.0);
    let dy = e - a;
    let ly = clamp(abs(dy) / max(max(abs(e - c), abs(c - a)), 1e-5), 0.0, 1.0);
    (*acc).dir += vec2<f32>(dx, dy) * w;
    (*acc).len += (lx * lx + ly * ly) * w;
}
// One texel of the 12 into the pixel: a Lanczos-2-shaped weight over its offset rotated into the edge's frame and stretched along it.
fn lobe(ac: ptr<function, vec3<f32>>, aw: ptr<function, f32>, off: vec2<f32>, dir: vec2<f32>, len: vec2<f32>, lob: f32, clp: f32, c: vec3<f32>) {
    let v = vec2<f32>(off.x * dir.x + off.y * dir.y, off.x * -dir.y + off.y * dir.x) * len;
    let d2 = min(dot(v, v), clp);
    var wb = 0.4 * d2 - 1.0;
    var wa = lob * d2 - 1.0;
    wb *= wb;
    wa *= wa;
    wb = 25.0 / 16.0 * wb - (25.0 / 16.0 - 1.0);
    let w = wb * wa;
    *ac += c * w;
    *aw += w;
}
@fragment fn fs_easu(i: VOut) -> @location(0) vec4<f32> {
    var pp = i.uv * vec2<f32>(textureDimensions(pic)) - vec2<f32>(0.5);
    let fp = floor(pp);
    pp -= fp;
    let o = vec2<i32>(fp);
    //    b c
    //  e f g h
    //  i j k l
    //    n m
    let b = texel(o + vec2<i32>(0, -1));
    let c = texel(o + vec2<i32>(1, -1));
    let e = texel(o + vec2<i32>(-1, 0));
    let f = texel(o);
    let g = texel(o + vec2<i32>(1, 0));
    let h = texel(o + vec2<i32>(2, 0));
    let ti = texel(o + vec2<i32>(-1, 1));
    let j = texel(o + vec2<i32>(0, 1));
    let k = texel(o + vec2<i32>(1, 1));
    let l = texel(o + vec2<i32>(2, 1));
    let n = texel(o + vec2<i32>(0, 2));
    let m = texel(o + vec2<i32>(1, 2));
    let bl = luma(b);
    let cl = luma(c);
    let el = luma(e);
    let fl = luma(f);
    let gl = luma(g);
    let hl = luma(h);
    let il = luma(ti);
    let jl = luma(j);
    let kl = luma(k);
    let ll = luma(l);
    let nl = luma(n);
    let ml = luma(m);
    var acc = Edge(vec2<f32>(0.0), 0.0);
    edge(&acc, (1.0 - pp.x) * (1.0 - pp.y), bl, el, fl, gl, jl);
    edge(&acc, pp.x * (1.0 - pp.y), cl, fl, gl, hl, kl);
    edge(&acc, (1.0 - pp.x) * pp.y, fl, il, jl, kl, nl);
    edge(&acc, pp.x * pp.y, gl, jl, kl, ll, ml);
    // The edge's direction (none: along x) and its strength, 0 (flat) to 1 (a clean edge).
    var dir = acc.dir;
    let dd = dot(dir, dir);
    if dd < 1.0 / 32768.0 {
        dir = vec2<f32>(1.0, 0.0);
    } else {
        dir *= inverseSqrt(dd);
    }
    var len = acc.len * 0.5;
    len *= len;
    // The kernel is stretched along the edge (up to the diagonal's sqrt 2) and narrowed across it, and its window widens from
    // sqrt 2 to a little over 2 texels with the edge's strength.
    let stretch = dot(dir, dir) / max(abs(dir.x), abs(dir.y));
    let len2 = vec2<f32>(1.0 + (stretch - 1.0) * len, 1.0 - 0.5 * len);
    let lob = 0.5 + ((1.0 / 4.0 - 0.04) - 0.5) * len;
    let clp = 1.0 / lob;
    var ac = vec3<f32>(0.0);
    var aw = 0.0;
    lobe(&ac, &aw, vec2<f32>(0.0, -1.0) - pp, dir, len2, lob, clp, b);
    lobe(&ac, &aw, vec2<f32>(1.0, -1.0) - pp, dir, len2, lob, clp, c);
    lobe(&ac, &aw, vec2<f32>(-1.0, 1.0) - pp, dir, len2, lob, clp, ti);
    lobe(&ac, &aw, vec2<f32>(0.0, 1.0) - pp, dir, len2, lob, clp, j);
    lobe(&ac, &aw, vec2<f32>(0.0, 0.0) - pp, dir, len2, lob, clp, f);
    lobe(&ac, &aw, vec2<f32>(-1.0, 0.0) - pp, dir, len2, lob, clp, e);
    lobe(&ac, &aw, vec2<f32>(1.0, 1.0) - pp, dir, len2, lob, clp, k);
    lobe(&ac, &aw, vec2<f32>(2.0, 1.0) - pp, dir, len2, lob, clp, l);
    lobe(&ac, &aw, vec2<f32>(2.0, 0.0) - pp, dir, len2, lob, clp, h);
    lobe(&ac, &aw, vec2<f32>(1.0, 0.0) - pp, dir, len2, lob, clp, g);
    lobe(&ac, &aw, vec2<f32>(1.0, 2.0) - pp, dir, len2, lob, clp, m);
    lobe(&ac, &aw, vec2<f32>(0.0, 2.0) - pp, dir, len2, lob, clp, n);
    // No ringing: the result stays within the four nearest texels.
    let lo = min(min(f, g), min(j, k));
    let hi = max(max(f, g), max(j, k));
    return vec4<f32>(clamp(ac / aw, lo, hi), 1.0);
}
// The upscaled picture sharpened pixel for pixel: each pixel pushed from its four neighbours as far as keeps every channel within
// 0..1 (the cross's lowest and highest values set the limit), less where the cross looks like noise rather than an edge (the
// movies' compression).
const SHARPEN: f32 = 0.87;
@fragment fn fs_rcas(i: VOut) -> @location(0) vec4<f32> {
    let p = vec2<i32>(i.uv * vec2<f32>(textureDimensions(pic)));
    //    b
    //  d e f
    //    h
    let b = texel(p + vec2<i32>(0, -1));
    let d = texel(p + vec2<i32>(-1, 0));
    let e = texel(p);
    let f = texel(p + vec2<i32>(1, 0));
    let h = texel(p + vec2<i32>(0, 1));
    let mn = min(min(b, d), min(f, h));
    let mx = max(max(b, d), max(f, h));
    let hit_min = min(mn, e) / max(4.0 * mx, vec3<f32>(1e-5));
    let hit_max = (vec3<f32>(1.0) - max(mx, e)) / min(4.0 * mn - vec3<f32>(4.0), vec3<f32>(-1e-5));
    let lobes = max(-hit_min, hit_max);
    var w = max(-(0.25 - 1.0 / 16.0), min(max(lobes.r, max(lobes.g, lobes.b)), 0.0)) * SHARPEN;
    let bl = luma(b);
    let dl = luma(d);
    let el = luma(e);
    let fl = luma(f);
    let hl = luma(h);
    let range = max(max(max(bl, dl), max(el, fl)), hl) - min(min(min(bl, dl), min(el, fl)), hl);
    let noise = clamp(abs(0.25 * (bl + dl + fl + hl) - el) / max(range, 1e-5), 0.0, 1.0);
    w *= 1.0 - 0.5 * noise;
    return vec4<f32>(clamp((w * (b + d + f + h) + e) / (4.0 * w + 1.0), vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
"#;

impl Screen {
    /// With `fifo` the surface shows one picture a refresh, in order (the timeline's: `Timeline`).
    fn new(native: &crate::native::Native, window: Arc<Window>, handles: Handles, kbm: Arc<Mutex<crate::input::Kbm>>, fifo: bool) -> Result<Screen, String> {
        let (instance, adapter, device, _) = native.gpu();
        // SAFETY: the handles are the window's, which `Screen` holds for as long as the surface.
        let surface = unsafe { instance.create_surface_unsafe(wgpu::SurfaceTargetUnsafe::RawHandle { raw_display_handle: Some(handles.0), raw_window_handle: handles.1 }) }.map_err(|e| format!("play: window surface: {e}"))?;
        if !adapter.is_surface_supported(&surface) {
            return Err(format!("play: {} cannot present to the window", adapter.get_info().name));
        }
        let caps = surface.get_capabilities(adapter);
        // The picture's bytes are the colours shown: a surface format that stores them as they are (not sRGB-encoded).
        let format = caps.formats.iter().copied().find(|f| !f.is_srgb()).ok_or_else(|| format!("play: the window offers only sRGB formats: {:?}", caps.formats))?;
        // Mailbox shows each frame as soon as it is drawn without tearing and without waiting for the display; else Immediate,
        // else FIFO.
        let modes: &[wgpu::PresentMode] = if fifo { &[wgpu::PresentMode::Fifo] } else { &[wgpu::PresentMode::Mailbox, wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo] };
        let present_mode = modes.iter().copied().find(|m| caps.present_modes.contains(m)).ok_or("play: no present mode")?;
        let size = window.inner_size();
        let mut config = surface.get_default_config(adapter, size.width.max(1), size.height.max(1)).ok_or("play: the window's surface has no configuration for the GPU")?;
        config.format = format;
        config.present_mode = present_mode;
        // Mailbox with a third image: FRAME PACING UNCAPPED presents each picture as soon as it is done, often while one is shown
        // and one waits, and the newest replaces the waiting one, so the lag does not grow. Immediate and FIFO keep one frame in
        // flight.
        config.desired_maximum_frame_latency = if present_mode == wgpu::PresentMode::Mailbox { 2 } else { 1 };
        surface.configure(device, &config);
        println!("play: presenting {format:?}, {present_mode:?}, on {} ({:?})", adapter.get_info().name, adapter.get_info().backend);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("blit"), source: wgpu::ShaderSource::Wgsl(BLIT.into()) });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blit"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("blit"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blit"),
            layout: Some(&pl),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: crate::pipelines::cache(),
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("blit"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        let pass = |entry: &str, target: wgpu::TextureFormat| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&pl),
                vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: target, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        let (easu, rcas) = (pass("fs_easu", wgpu::TextureFormat::Rgba8Unorm), pass("fs_rcas", format));
        // The pointer binds nothing: its layout made from its shaders.
        let pointer = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("quick-select pointer"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs_pointer"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format, blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING), write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: crate::pipelines::cache(),
        });
        let fps = Fps::new(&device, native.gpu().3, format)?;
        let tab_bar = TabBar::new(&device, native.gpu().3, format)?;
        let notice = NoticePlate::new(&device, native.gpu().3, format)?;
        Ok(Screen { device: device.clone(), queue: native.gpu().3.clone(), window, surface, config, pipeline, layout, sampler, frame: None, pictures: Vec::new(), last: None, kbm, easu, rcas, up: None, pointer, fps, tab_bar, notice, shape_said: (0, 0, 0, 0, 0.0), suboptimal_redone: false })
    }

    /// The shape `shown` is shown at, the native picture's height high (`size`: the native picture's size when the frame was
    /// drawn): the native picture at its own (the shape the game's view was made at, the window's: PLAN 4.2), a movie at its own
    /// (`MOVIE_SHAPE`) widened by `zoom` (`movie_zoom`: its rows were cut by as much where it was uploaded, `window_copy`).
    fn shape(&self, size: (u32, u32), shown: &Shown, zoom: f32) -> (u32, u32) {
        let h = size.1;
        match shown {
            Shown::Gpu(_) => size,
            Shown::Picture(_) => ((h as f32 * MOVIE_SHAPE * zoom * zoom).round() as u32, h),
        }
    }

    /// `shown` as `present` puts it in the window, drawn at its shape (`shape`) the native picture's height high into a texture
    /// of its own and written to `path` as a PNG (the control channel's `shot`: the renderer's picture, not a grab of the window).
    fn shot(&mut self, shown: &Shown, size: (u32, u32), path: &str) -> Result<(), String> {
        let (device, queue) = (&self.device.clone(), &self.queue.clone());
        let (w, h) = self.shape(size, shown, movie_zoom(self.window.inner_size()));
        let format = self.config.format;
        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("shot"),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = tex.create_view(&Default::default());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("shot") });
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shot"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some((bg, _)) = &self.last {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.pointer(&mut enc, &view, [0.0, 0.0, w as f32, h as f32], (w, h));
        let tabs = self.kbm.lock().expect("input lock").tab_bar.clone();
        self.tab_bar.draw(device, &mut enc, &view, (w, h), [0.0, 0.0, w as f32, h as f32], &tabs);
        let notice = self.kbm.lock().expect("input lock").notice.clone();
        self.notice.draw(device, &mut enc, &view, (w, h), [0.0, 0.0, w as f32, h as f32], notice.as_ref());
        self.fps.draw(device, &mut enc, &view, (w, h));
        let row = (4 * w).div_ceil(256) * 256;
        let readback = device.create_buffer(&wgpu::BufferDescriptor { label: Some("shot readback"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        );
        queue.submit([enc.finish()]);
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("play: shot: GPU wait: {e}"))?;
        rx.recv().map_err(|e| format!("play: shot: {e}"))?.map_err(|e| format!("play: shot: {e}"))?;
        let bytes = slice.get_mapped_range().map_err(|e| format!("play: shot: {e}"))?;
        let bgra = matches!(format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h as usize {
            for px in bytes[y * row as usize..y * row as usize + 4 * w as usize].chunks_exact(4) {
                rgb.extend_from_slice(&if bgra { [px[2], px[1], px[0]] } else { [px[0], px[1], px[2]] });
            }
        }
        drop(bytes);
        readback.unmap();
        std::fs::write(path, crate::pic::png(&crate::pic::Image { w: w as usize, h: h as usize, rgb })).map_err(|e| format!("{path}: {e}"))
    }

    /// While the quick-select ring is held open with the keyboard and mouse (`input::Kbm::ring_pointer`), a pointer where the mouse
    /// points on it (the player, 16 November 2026: "you cant see what your selecting"), into `view` (`size` pixels) over the picture
    /// at `viewport`: from the ring's middle toward its slots, on a slot when the mouse points far enough to take it.
    fn pointer(&self, enc: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, viewport: [f32; 4], size: (u32, u32)) {
        let Some((x, y)) = self.kbm.lock().expect("input lock").ring_pointer else { return };
        let [vx, vy, vw, vh] = viewport;
        // The original picture's width at the picture's height, in its middle: where the game's 2D layer is placed.
        let w = vw.min(vh * 0.62 / 0.4805);
        let left = vx + (vw - w) / 2.0;
        let (px, py) = (left + (RING_AT.0 + x as f32 * RING_SLOTS.0) * w, vy + (RING_AT.1 + y as f32 * RING_SLOTS.1) * vh);
        let r = (POINTER * vh).max(6.0);
        if px - r < 0.0 || py - r < 0.0 || px + r > size.0 as f32 || py + r > size.1 as f32 {
            return;
        }
        let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("quick-select pointer"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        pass.set_viewport(px - r, py - r, 2.0 * r, 2.0 * r, 0.0, 1.0);
        pass.set_pipeline(&self.pointer);
        pass.draw(0..3, 0..1);
    }

    /// The window's next image to present into (None: the window has no picture now, minimised or occluded): with a FIFO surface,
    /// handed over as the display takes the one before.
    fn acquire(&mut self) -> Result<Option<wgpu::SurfaceTexture>, String> {
        let win = self.window.inner_size();
        if win.width == 0 || win.height == 0 {
            return Ok(None);
        }
        if (win.width, win.height) != (self.config.width, self.config.height) {
            self.config.width = win.width;
            self.config.height = win.height;
            self.surface.configure(&self.device, &self.config);
            self.suboptimal_redone = false;
        }
        // The game's view at the window's shape (`uyart::set_view_k`): its width over the original picture's at its height
        // (`Native::open`'s 0.62 / 0.4805), 1.0 for a window of the original shape, and with WIDESCREEN off.
        let k = if WIDE.load(Ordering::Relaxed) { win.width as f32 / (win.height as f32 * 0.62 / 0.4805) } else { 1.0 };
        uyart::set_view_k(k);
        // Each change of the window's size, the surface's or the view's widening is said: a player's log then shows the shape
        // history around a picture shown at the wrong scale (a v0.1.3 player: "Scaled to 4K until pausing with KB").
        let shape = (win.width, win.height, self.config.width, self.config.height, k);
        if shape != self.shape_said {
            println!("play: window {}x{}, surface {}x{}, view widening {k:.4}", win.width, win.height, self.config.width, self.config.height);
            self.shape_said = shape;
        }
        match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => Ok(Some(t)),
            // A suboptimal image (the swapchain's extent or form no longer the window's, as after a switch to fullscreen, which
            // Windows makes after the window's size is read): the surface made again once at this size and a new image taken, so
            // a stale swapchain is not stretched over the window until the size next changes. An image still suboptimal after it
            // is shown as it is.
            wgpu::CurrentSurfaceTexture::Suboptimal(t) if !self.suboptimal_redone => {
                drop(t);
                println!("play: the window's surface was suboptimal at {}x{}: made again", self.config.width, self.config.height);
                self.surface.configure(&self.device, &self.config);
                self.suboptimal_redone = true;
                match self.surface.get_current_texture() {
                    wgpu::CurrentSurfaceTexture::Success(t) | wgpu::CurrentSurfaceTexture::Suboptimal(t) => Ok(Some(t)),
                    wgpu::CurrentSurfaceTexture::Validation => Err("play: the window's surface texture failed validation".into()),
                    _ => Ok(None),
                }
            }
            wgpu::CurrentSurfaceTexture::Suboptimal(t) => Ok(Some(t)),
            wgpu::CurrentSurfaceTexture::Timeout | wgpu::CurrentSurfaceTexture::Occluded => Ok(None),
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                Ok(None)
            }
            wgpu::CurrentSurfaceTexture::Validation => Err("play: the window's surface texture failed validation".into()),
        }
    }

    /// Draws `shown` (the native pictures of size `size`) into the window, scaled to fit at its shape and centred, and presents it.
    fn present(&mut self, shown: &Shown, size: (u32, u32), _: Option<()>) -> Result<(), String> {
        match self.acquire()? {
            Some(out) => self.present_into(out, shown, size),
            // The picture is still taken in (a movie's, a frame's: the window shows it once it can).
            None => self.present_to(None, shown, size),
        }
    }

    /// `present` into the window's image `out`.
    fn present_into(&mut self, out: wgpu::SurfaceTexture, shown: &Shown, size: (u32, u32)) -> Result<(), String> {
        self.present_to(Some(out), shown, size)
    }

    fn present_to(&mut self, out: Option<wgpu::SurfaceTexture>, shown: &Shown, native_size: (u32, u32)) -> Result<(), String> {
        if let Some(out) = self.window_copy(out, shown, native_size)? {
            self.present_now(out);
        }
        Ok(())
    }

    /// The window's image presented (its copy submitted by `window_copy`).
    fn present_now(&mut self, out: wgpu::SurfaceTexture) {
        let presenting = Instant::now();
        self.window.pre_present_notify();
        self.queue.present(out);
        if frame_times() { println!("frame stages: screen present {:.3}", presenting.elapsed().as_secs_f64() * 1e3); }
    }

    /// The bind group that samples native picture `t` where it is, made once per texture and kept while pictures are of its size
    /// (the newest last, at most `PICTURE_GROUPS`).
    fn picture_group(&mut self, t: &wgpu::Texture) -> wgpu::BindGroup {
        if let Some(k) = self.pictures.iter().position(|(p, _)| p == t) {
            let kept = self.pictures.remove(k);
            let bg = kept.1.clone();
            self.pictures.push(kept);
            return bg;
        }
        self.pictures.retain(|(p, _)| (p.width(), p.height()) == (t.width(), t.height()));
        if self.pictures.len() == PICTURE_GROUPS {
            self.pictures.remove(0);
        }
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("native picture"),
            layout: &self.layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.create_view(&Default::default())) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
        });
        self.pictures.push((t.clone(), bg.clone()));
        bg
    }

    /// `present_to`'s copy of the picture into the window's image `out`, submitted; the image, not yet presented, returned (None
    /// without one: the picture is taken in for the next present).
    fn window_copy(&mut self, out: Option<wgpu::SurfaceTexture>, shown: &Shown, native_size: (u32, u32)) -> Result<Option<wgpu::SurfaceTexture>, String> {
        let (device, queue) = (&self.device.clone(), &self.queue.clone());
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("present") });
        let win = self.window.inner_size();
        let zoom = movie_zoom(win);
        let size = match shown {
            Shown::Gpu(t) => (t.width(), t.height()),
            // A movie's rows the window shows: its top and bottom cut alike by the zoom (`movie_zoom`).
            Shown::Picture(p) => (p.w, ((p.h as f32 / zoom).round() as u32).clamp(1, p.h)),
        };
        // A native picture the window shows now is sampled where it is (`picture_group`); a movie's picture, and a native one the
        // window cannot show now, are copied into the presenter's own `frame` first (a native picture goes back to the renderer once
        // shown, and would be drawn over before the window could show it). The copy was a whole picture's read and write at every
        // present: on an integrated GPU's shared memory some 16 MB a picture at 1080 lines.
        let direct = match (shown, out.is_some()) {
            (Shown::Gpu(t), true) => Some(self.picture_group(t)),
            _ => None,
        };
        if direct.is_none() {
            if self.frame.as_ref().is_none_or(|f| f.2 != size) {
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("window picture"),
                    size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("blit"),
                    layout: &self.layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&tex.create_view(&Default::default())) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
                });
                self.frame = Some((tex, bg, size));
            }
            let (tex, _, _) = self.frame.as_ref().expect("made above");
            let extent = wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 };
            match shown {
                Shown::Gpu(t) => enc.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    extent,
                ),
                Shown::Picture(p) => queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    &p.rgba[(4 * p.w * ((p.h - size.1) / 2)) as usize..],
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * p.w), rows_per_image: Some(size.1) },
                    extent,
                ),
            }
        }
        let source = match direct {
            Some(bg) => bg,
            None => self.frame.as_ref().expect("made for the picture above").1.clone(),
        };
        self.last = Some((source.clone(), size));
        // The window's image (none now: the picture is taken in, and shown by the next present).
        let Some(out) = out else {
            queue.submit([enc.finish()]);
            return Ok(None);
        };
        // The picture at its own shape, as large as the window allows, centred, on whole pixels: the world at the native
        // picture's (the shape the game's view was made at, the window's: PLAN 4.2), a movie at its own widened by the zoom
        // (`movie_zoom`: across the whole of a 16:9 or 16:10 window).
        let (ww, wh) = (win.width as f32, win.height as f32);
        let shape = self.shape(native_size, shown, zoom);
        let s = (ww / shape.0 as f32).min(wh / shape.1 as f32);
        let (vw, vh) = ((shape.0 as f32 * s).round().max(1.0), (shape.1 as f32 * s).round().max(1.0));
        let (vx, vy) = (((ww - vw) / 2.0).floor(), ((wh - vh) / 2.0).floor());
        self.kbm.lock().expect("input lock").viewport = [vx, vy, vw, vh];
        // A movie's picture, or a native picture smaller than its place in the window (a RESOLUTION below the window's height:
        // 540 to 900 for an integrated GPU), brought to that size, edge-directed, into `up`, and sharpened into the window.
        let upscaled = matches!(shown, Shown::Picture(_)) || vh > size.1 as f32;
        if upscaled {
            let size = (vw as u32, vh as u32);
            if self.up.as_ref().is_none_or(|u| u.2 != size) {
                let t = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("movie, upscaled"),
                    size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("movie, upscaled"),
                    layout: &self.layout,
                    entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&t.create_view(&Default::default())) }, wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) }],
                });
                self.up = Some((t, bg, size));
            }
            let (t, _, _) = self.up.as_ref().expect("made above");
            let view = t.create_view(&Default::default());
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("movie upscale"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.easu);
            pass.set_bind_group(0, &source, &[]);
            pass.draw(0..3, 0..1);
        }
        let view = out.texture.create_view(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
            if upscaled {
                let (_, bg, _) = self.up.as_ref().expect("upscaled above");
                pass.set_pipeline(&self.rcas);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            } else {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &source, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.pointer(&mut enc, &view, [vx, vy, vw, vh], (win.width, win.height));
        let tabs = self.kbm.lock().expect("input lock").tab_bar.clone();
        self.tab_bar.draw(device, &mut enc, &view, (win.width, win.height), [vx, vy, vw, vh], &tabs);
        let notice = self.kbm.lock().expect("input lock").notice.clone();
        self.notice.draw(device, &mut enc, &view, (win.width, win.height), [vx, vy, vw, vh], notice.as_ref());
        self.fps.count(Instant::now())?;
        self.fps.draw(device, &mut enc, &view, (win.width, win.height));
        let done_encoding = Instant::now();
        let commands = enc.finish();
        let finished = Instant::now();
        queue.submit([commands]);
        if frame_times() { println!("frame stages: screen finish {:.3} submit {:.3}", (finished - done_encoding).as_secs_f64() * 1e3, finished.elapsed().as_secs_f64() * 1e3); }
        Ok(Some(out))
    }
}

/// When the renderer began the frame it is busy with (nanoseconds from `clock`), 0 while it waits for one.
static RENDER_BUSY: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The process's clock for the renderer's busy mark and the presenter's log lines: its start, the first time it is read.
fn clock_start() -> Instant {
    static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    *START.get_or_init(Instant::now)
}

fn clock() -> u64 {
    clock_start().elapsed().as_nanos() as u64 + 1
}

/// `t` on the process's clock (ms), for the log's per-picture lines (UYA_FRAME_TIMES).
fn stamp(t: Instant) -> f64 {
    t.saturating_duration_since(clock_start()).as_secs_f64() * 1e3
}

/// The renderer's usual time for a frame (nanoseconds): an average of its frames, each counted at most `USUAL_CAP` so that a
/// level's load does not raise it.
static RENDER_USUAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const USUAL_CAP: u64 = 100_000_000;

/// A frame drawn in `ns` nanoseconds, into the renderer's usual time (`renderer_held`).
fn frame_drawn(ns: u64) {
    let usual = RENDER_USUAL.load(Ordering::Relaxed);
    RENDER_USUAL.store(if usual == 0 { ns.min(USUAL_CAP) } else { usual - usual / 8 + ns.min(USUAL_CAP) / 8 }, Ordering::Relaxed);
}

/// Whether the renderer has been on one frame for more than two fields and more than three of its usual frames: a level's load
/// or another long build, not a frame a little late. The game thread waits for it then (as on a loading screen) rather than
/// running on undrawn (`rtgame.rs`). Two fields alone held a GPU that takes longer than that for every frame: the game ran in
/// slow motion as well as at its few pictures a second (a v0.1.3 player on a Ryzen 5 4600G's Radeon graphics: 42 ms a frame
/// in Veldin, the game at 44.5 fields a second, 74 % of its speed); it now drops pictures there, not game time.
pub(crate) fn renderer_held() -> bool {
    let since = RENDER_BUSY.load(Ordering::Relaxed);
    since != 0 && clock().saturating_sub(since) > ((2.0 * FIELD_S * 1e9) as u64).max(3 * RENDER_USUAL.load(Ordering::Relaxed))
}

/// Whole-frame scheduling diagnostic, separate from GPU timestamp readback (which would serialize the workload).
pub(crate) fn frame_times() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UYA_FRAME_TIMES").is_some())
}
