//! NVIDIA Streamline for DLSS Frame Generation (DLSS-G): the FRAME GENERATION option (`settings::OPTIONS`,
//! `enhance::frame_generation`), through `native/streamline.cpp` (Streamline's headers are C++).
//!
//! - **Loaded at every start-up on a PC with an NVIDIA GPU, run only while on.** (Any other PC keeps the plain Vulkan loader and
//!   frame generation reads unavailable; UYA_NO_NVIDIA=1 stands in for such a PC.) `dlss::instance` calls `load` before the Vulkan instance exists: Streamline
//!   is initialised (DLSS-G, Reflex, PC Latency; about 1.2 s) and the Vulkan loader becomes `sl.interposer.dll`
//!   (`wgpu::hal::vulkan::LOADER`), which exports the loader's entry points and, as the instance, device and swapchain are made
//!   through them, adds DLSS-G's extensions and queues and takes over the swapchain's present. Only a loader chosen before the
//!   instance can do that, so Streamline is loaded whatever the setting (the player, 24 November 2026: "Make it so you can change
//!   frame gen setting without restarting"; loaded only with the setting on at start-up, turning it on took a restart). The
//!   setting is read at each picture (`running`): OFF, 2X, 3X and 4X apply at the next one. While it is off nothing more runs:
//!   Reflex is off, no frame is begun, slept for or marked, and the present hook only passes the present on.
//! - **When it fails** (the DLLs missing, the driver or GPU without DLSS-G, Streamline's own refusal) the reason is logged
//!   ("streamline: ...") and the run goes on with it off (`Status::Unavailable`, which the settings row shows).
//! - **Each picture** (`dlss::Upscaler::shot` begins it, `PRESENT_HOOK` ends it): a frame index; Reflex's sleep and the PC Latency
//!   markers; the camera's constants (`constants`); depth, the motion vectors and the picture before the HUD tagged (`tag`, with
//!   Streamline's own copy of each made in a command buffer of the picture's, since the next picture overdraws them before this
//!   one is presented); and at its present DLSS-G's options (on with the setting's frames to generate, or off for a picture without
//!   DLSS: the menus) between the present's own markers. The thread that records a picture hands its index on with the picture
//!   (`take_begun`, `play::Pending::fg`), and the presenter names it just before the present (`presenting`).
//! - **Counting.** The frames DLSS-G actually presents come from its state each present; they are logged, and the presenter's own
//!   counts (`play.rs`) stay the real pictures. The frames it generates per real picture are `crate::play::FG_GENERATED`, which
//!   the presenter leaves room for (one real picture every that many refreshes plus one): the setting's frames while DLSS-G shows
//!   generated frames, none once it has shown `ALONE_RUN` real pictures in a row alone (`present`). DLSS-G stops generating on
//!   its own while the game's window is not in front (Streamline's `hasFocus`: the foreground window's process; `sl.log` "DLSS-G
//!   disabled: window not focused"), and the window stays behind while the game is played on a pad. Paced for frames it was not
//!   making, the presenter drew 180 pictures a second at 2X, 120 at 3X and 90 at 4X on the player's 360 Hz display, each shown
//!   once, where it draws about 210 with FRAME GENERATION off (the player, 25 November 2026: "Frame gen appears to be dropping FPS
//!   the higher you put it").

use std::ffi::c_char;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU8, Ordering};
use std::sync::Mutex;
use wgpu::hal::api::Vulkan;

#[repr(C)]
struct Image {
    image: u64,
    view: u64,
    layout: u32,
    width: u32,
    height: u32,
    format: u32,
    usage: u32,
    flags: u32,
}

#[repr(C)]
struct Constants {
    view_to_clip: [f32; 16],
    clip_to_view: [f32; 16],
    clip_to_prev: [f32; 16],
    prev_to_clip: [f32; 16],
    jitter: [f32; 2],
    mvec_scale: [f32; 2],
    position: [f32; 3],
    up: [f32; 3],
    right: [f32; 3],
    forward: [f32; 3],
    near: f32,
    far: f32,
    fov: f32,
    aspect: f32,
    reset: u32,
}

unsafe extern "C" {
    fn uya_sl_init(dir: *const u16, logs: *const u16, log: extern "C" fn(u32, *const c_char), project: *const c_char) -> i32;
    fn uya_sl_supported(physical_device: u64, which: *mut u32) -> i32;
    fn uya_sl_ready() -> i32;
    fn uya_sl_state(max_generated: *mut u32, status: *mut u32, presented: *mut u32, min_side: *mut u32) -> i32;
    fn uya_sl_reflex(on: i32) -> i32;
    fn uya_sl_sleep(frame: u32) -> i32;
    fn uya_sl_marker(marker: u32, frame: u32) -> i32;
    fn uya_sl_options(generated: u32, back_buffers: u32) -> i32;
    fn uya_sl_constants(frame: u32, c: *const Constants) -> i32;
    fn uya_sl_tag(frame: u32, cmd: u64, depth: *const Image, motion: *const Image, hudless: *const Image) -> i32;
    fn uya_sl_shutdown() -> i32;
}

/// `sl::PCLMarker`'s values.
const SIMULATION_START: u32 = 0;
const SIMULATION_END: u32 = 1;
const RENDER_SUBMIT_START: u32 = 2;
const RENDER_SUBMIT_END: u32 = 3;
const PRESENT_START: u32 = 4;
const PRESENT_END: u32 = 5;

/// What FRAME GENERATION is in this run (`status`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Status {
    /// Off at start-up: Streamline is not loaded.
    Off,
    /// Streamline is loaded and DLSS-G runs: up to this many frames generated per real picture.
    Active(u32),
    /// It cannot run on this PC (why).
    Unavailable(String),
}

const OFF: u8 = 0;
const ACTIVE: u8 = 1;
const FAILED: u8 = 2;
static LOADED: AtomicU8 = AtomicU8::new(OFF);
static MOST: AtomicU32 = AtomicU32::new(0);
static REASON: Mutex<String> = Mutex::new(String::new());
static NEXT: AtomicU32 = AtomicU32::new(1);
thread_local! {
    /// The frame this thread began last and has not handed on (`begin`, `take_begun`): a picture's recording begins its frame
    /// on the thread that records it, which hands the index on with the picture (`play::Pending::fg`).
    static BEGUN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}
/// The frame of the picture being presented (`presenting`), 0 for a present with no tagged picture.
static PRESENTING: AtomicU32 = AtomicU32::new(0);
/// Whether frame generation ran at the last present (`present`): Reflex is switched as it turns on or off.
static ON: AtomicBool = AtomicBool::new(false);
/// The frames the window showed for the last picture presented (`shown`).
static SHOWN: AtomicU32 = AtomicU32::new(1);
/// The frames DLSS-G was asked to generate at the last present (0: none, its options off).
static ASKED: AtomicU32 = AtomicU32::new(0);
/// The pictures in a row DLSS-G was asked to generate for and showed alone (`present`).
static ALONE: AtomicU32 = AtomicU32::new(0);
/// The pictures in a row shown alone after which DLSS-G counts as making no frames: well past a frame it drops now and then
/// (a sixth of a second at 2X on a 360 Hz display).
const ALONE_RUN: u32 = 30;

/// The frames the window showed for the last picture presented: while frame generation runs, DLSS-G's count (the picture and
/// the frames it generated before it); else 1. The FPS counter adds them up (`play::Fps`).
pub(crate) fn shown() -> u32 {
    SHOWN.load(Ordering::Relaxed)
}

/// Whether frame generation runs on the pictures now: Streamline running, the option on and DLSS on (it needs DLSS's depth and
/// motion vectors).
fn running() -> bool {
    LOADED.load(Ordering::Relaxed) == ACTIVE && generated() > 0 && crate::enhance::dlss() > 0
}

/// FRAME GENERATION's state now, for the settings page.
pub(crate) fn status() -> Status {
    match LOADED.load(Ordering::Relaxed) {
        ACTIVE => Status::Active(MOST.load(Ordering::Relaxed)),
        FAILED => Status::Unavailable(REASON.lock().expect("streamline reason lock").clone()),
        _ => Status::Off,
    }
}

pub(crate) fn fail(why: String) {
    println!("streamline: frame generation unavailable, the game runs with it off: {why}");
    *REASON.lock().expect("streamline reason lock") = why;
    LOADED.store(FAILED, Ordering::Relaxed);
    crate::play::FG_GENERATED.store(0, Ordering::Relaxed);
    SHOWN.store(1, Ordering::Relaxed);
}

extern "C" fn log(kind: u32, message: *const c_char) {
    // SAFETY: Streamline passes a NUL-terminated string valid for the call.
    let m = unsafe { std::ffi::CStr::from_ptr(message) }.to_string_lossy();
    // Warnings and errors only: the information (some thousand lines) is in the file `streamline-logs\sl.log` beside the executable.
    if kind > 0 {
        println!("streamline: {}{}", ["", "warning: ", "error: "].get(kind as usize).copied().unwrap_or(""), m.trim_end());
    }
}

fn wide(s: &std::path::Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.as_os_str().encode_wide().chain(Some(0)).collect()
}

fn result(what: &str, r: i32) -> Result<(), String> {
    if r == 0 { Ok(()) } else { Err(format!("{what}: Streamline's result {r} (sl::Result; below zero, this layer's own: the DLL's Windows error negated)")) }
}

/// Before the Vulkan instance: Streamline initialised and the Vulkan loader set to its interposer. A failure is logged and
/// leaves the loader as it was.
pub(crate) fn load() {
    // Streamline's interposer would sit in every Vulkan call of the run and does nothing on a PC without an NVIDIA GPU (DLSS-G,
    // Reflex and PC Latency are NVIDIA's): there the plain Vulkan loader is used. UYA_NO_NVIDIA: this PC taken as one without
    // (for a check of what such PCs run, on an NVIDIA PC; as UYA_NO_RT and UYA_NO_ARRAYS stand in for other GPUs).
    match has_nvidia() {
        Ok(true) => {}
        Ok(false) => {
            fail("no NVIDIA GPU: the standard Vulkan loader is used, without Streamline".into());
            return;
        }
        Err(e) => panic!("streamline: the adapters could not be listed: {e}"),
    }
    let started = std::time::Instant::now();
    let loaded = (|| -> Result<(), String> {
        let exe = std::env::current_exe().map_err(|e| format!("the executable's folder: {e}"))?;
        let dir = exe.parent().ok_or("the executable has no folder")?.to_path_buf();
        for f in ["sl.interposer.dll", "sl.common.dll", "sl.dlss_g.dll", "sl.reflex.dll", "sl.pcl.dll", "nvngx_dlssg.dll"] {
            if !dir.join(f).is_file() {
                return Err(format!("{f} is not beside the executable (scripts/build.ps1 copies it from STREAMLINE_SDK)"));
            }
        }
        let logs = dir.join("streamline-logs");
        std::fs::create_dir_all(&logs).map_err(|e| format!("{}: {e}", logs.display()))?;
        let project = std::ffi::CString::new(crate::dlss::PROJECT.to_string()).expect("a uuid has no NUL");
        // SAFETY: the paths are NUL-terminated wide strings and the callback outlives the process.
        result("slInit", unsafe { uya_sl_init(wide(&dir).as_ptr(), wide(&logs).as_ptr(), log, project.as_ptr()) })?;
        wgpu::hal::vulkan::LOADER.set(dir.join("sl.interposer.dll")).map_err(|_| "the Vulkan loader was already chosen".to_string())?;
        wgpu::hal::vulkan::PRESENT_HOOK.set(present).map_err(|_| "the present hook was already set".to_string())?;
        Ok(())
    })();
    match loaded {
        Ok(()) => println!("streamline: initialised in {:.0} ms (DLSS-G, Reflex, PC Latency); the Vulkan loader is its interposer", started.elapsed().as_secs_f64() * 1e3),
        Err(e) => fail(e),
    }
}

/// Whether a DXGI adapter is an NVIDIA GPU (vendor 0x10DE), asked before any Vulkan instance exists.
fn has_nvidia() -> Result<bool, String> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, IDXGIFactory1, DXGI_ERROR_NOT_FOUND};
    if std::env::var_os("UYA_NO_NVIDIA").is_some() {
        return Ok(false);
    }
    // SAFETY: plain DXGI calls on a factory made here.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(|e| format!("CreateDXGIFactory1: {e}"))?;
        let mut i = 0;
        loop {
            match factory.EnumAdapters1(i) {
                Ok(a) => {
                    if a.GetDesc1().map_err(|e| format!("GetDesc1: {e}"))?.VendorId == 0x10DE {
                        return Ok(true);
                    }
                    i += 1;
                }
                Err(e) if e.code() == DXGI_ERROR_NOT_FOUND => return Ok(false),
                Err(e) => return Err(format!("EnumAdapters1: {e}")),
            }
        }
    }
}

/// Whether `load` made Streamline the Vulkan loader of this run (and nothing has failed since).
pub(crate) fn loaded() -> bool {
    wanted_at_start() && LOADED.load(Ordering::Relaxed) != FAILED
}

fn wanted_at_start() -> bool {
    wgpu::hal::vulkan::LOADER.get().is_some()
}

/// After the adapter is chosen: whether it runs DLSS-G.
pub(crate) fn adapter(adapter: &wgpu::Adapter) {
    if !loaded() {
        return;
    }
    // SAFETY: a Vulkan adapter (the instance is Vulkan's).
    let physical = unsafe { std::mem::transmute::<_, u64>(adapter.as_hal::<Vulkan>().expect("a Vulkan adapter").raw_physical_device()) };
    let mut which = 0u32;
    // SAFETY: `which` is a valid out pointer.
    let r = unsafe { uya_sl_supported(physical, &mut which) };
    if r != 0 {
        let feature = match which {
            1000 => "DLSS-G",
            3 => "Reflex",
            4 => "PC Latency",
            _ => "a feature",
        };
        fail(format!("{} does not run {feature} (sl::Result {r}: 2 the driver is out of date, 3 the OS, 4 hardware-accelerated GPU scheduling is off, 6 no supported adapter, 7 this adapter is not supported)", adapter.get_info().name));
    }
}

/// After the device exists: Streamline's functions and DLSS-G's limits (Reflex goes on with frame generation, `present`).
pub(crate) fn device() {
    if !loaded() {
        return;
    }
    let ready = (|| -> Result<u32, String> {
        // SAFETY: plain calls into the layer, after the device exists.
        result("the features' functions", unsafe { uya_sl_ready() })?;
        let (mut most, mut status, mut presented, mut side) = (0, 0, 0, 0);
        result("DLSS-G's state", unsafe { uya_sl_state(&mut most, &mut status, &mut presented, &mut side) })?;
        println!("streamline: DLSS-G generates up to {most} frames per real picture (status {status}, pictures from {side} pixels)");
        if most == 0 {
            return Err("DLSS-G generates no frames on this GPU".into());
        }
        Ok(most)
    })();
    match ready {
        Ok(most) => {
            MOST.store(most, Ordering::Relaxed);
            LOADED.store(ACTIVE, Ordering::Relaxed);
        }
        Err(e) => fail(e),
    }
}

/// How many frames the setting asks DLSS-G to generate per real picture, at most what it can.
fn generated() -> u32 {
    crate::enhance::frame_generation().min(MOST.load(Ordering::Relaxed))
}

/// The next picture's frame, while frame generation runs (`running`): its index, with Reflex's sleep and the markers up to the
/// start of its submission. The pictures are begun in the order they are presented.
pub(crate) fn begin() -> Option<u32> {
    if !running() {
        return None;
    }
    let index = NEXT.fetch_add(1, Ordering::Relaxed);
    // SAFETY: plain calls into the layer. Reflex's sleep and the markers are thread safe.
    let r = unsafe { [uya_sl_sleep(index), uya_sl_marker(SIMULATION_START, index), uya_sl_marker(SIMULATION_END, index), uya_sl_marker(RENDER_SUBMIT_START, index)] };
    if let Some(e) = r.iter().find(|r| **r != 0) {
        fail(format!("Reflex's sleep and markers: {e}"));
        return None;
    }
    BEGUN.with(|b| b.set(index));
    Some(index)
}

/// The frame this thread's last recording began (`begin`), taken: the picture that recording made carries it to its present
/// (`play::Pending::fg`). None when none was begun (frame generation not running, or a picture drawn without DLSS's step).
pub(crate) fn take_begun() -> Option<u32> {
    BEGUN.with(|b| Some(b.replace(0)).filter(|&i| i != 0))
}

/// The picture about to be presented: its frame (`take_begun`), or None for a picture no frame was begun for. The present hook
/// gives that frame its markers and DLSS-G's options (`present`).
pub(crate) fn presenting(frame: Option<u32>) {
    PRESENTING.store(frame.unwrap_or(0), Ordering::Relaxed);
}

/// Frame `index`'s camera: the matrices Streamline takes (row-major, row vectors, without the jitter), the camera's place and axes.
/// `cam` is the picture's, `prev` the picture before's; `jitter` DLSS's offset in render pixels, `render` its size.
pub(crate) fn constants(index: u32, cam: &crate::native::Camera, prev: &crate::native::Camera, jitter: [f32; 2], render: (u32, u32), reset: bool) {
    if let Err(e) = try_constants(index, cam, prev, jitter, render, reset) {
        fail(e);
    }
}

fn try_constants(index: u32, cam: &crate::native::Camera, prev: &crate::native::Camera, jitter: [f32; 2], render: (u32, u32), reset: bool) -> Result<(), String> {
    type M = [[f64; 4]; 4];
    let mul = |a: &M, b: &M| -> M {
        let mut r = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                r[i][j] = (0..4).map(|k| a[i][k] * b[k][j]).sum();
            }
        }
        r
    };
    let transpose = |a: &M| -> M {
        let mut r = [[0.0; 4]; 4];
        for i in 0..4 {
            for j in 0..4 {
                r[i][j] = a[j][i];
            }
        }
        r
    };
    // The inverse by Gauss-Jordan with partial pivoting.
    let inverse = |a: &M| -> Result<M, String> {
        let mut m = *a;
        let mut r: M = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        for c in 0..4 {
            let p = (c..4).max_by(|&x, &y| m[x][c].abs().total_cmp(&m[y][c].abs())).expect("rows");
            if m[p][c].abs() < 1e-30 {
                return Err("streamline: a camera matrix has no inverse".into());
            }
            m.swap(c, p);
            r.swap(c, p);
            let d = m[c][c];
            for j in 0..4 {
                m[c][j] /= d;
                r[c][j] /= d;
            }
            for i in 0..4 {
                if i != c {
                    let f = m[i][c];
                    for j in 0..4 {
                        m[i][j] -= f * m[c][j];
                        r[i][j] -= f * r[c][j];
                    }
                }
            }
        }
        Ok(r)
    };
    // The game's own matrix (`Camera::matrix`, column-major) as M[row][column].
    let world_to_clip = |c: &crate::native::Camera| -> M {
        let m = c.matrix();
        let mut r = [[0.0; 4]; 4];
        for (col, v) in m.iter().enumerate() {
            for (row, x) in v.iter().enumerate() {
                r[row][col] = *x as f64;
            }
        }
        r
    };
    let (near, far) = (crate::native::NEAR as f64, crate::native::FAR as f64);
    let (a, b) = (-near / (far - near), near * far / (far - near));
    let view_to_clip: M = [[1.0 / cam.tan[0] as f64, 0.0, 0.0, 0.0], [0.0, 1.0 / cam.tan[1] as f64, 0.0, 0.0], [0.0, 0.0, a, b], [0.0, 0.0, 1.0, 0.0]];
    let axes = |c: &crate::native::Camera| {
        let (f, u) = (c.forward, c.up);
        let r = [f[1] * u[2] - f[2] * u[1], f[2] * u[0] - f[0] * u[2], f[0] * u[1] - f[1] * u[0]];
        (r, u, f)
    };
    let (r, u, f) = axes(cam);
    // View (x right, y up, z forward) to world.
    let view_to_world: M = [
        [r[0] as f64, u[0] as f64, f[0] as f64, cam.eye[0] as f64],
        [r[1] as f64, u[1] as f64, f[1] as f64, cam.eye[1] as f64],
        [r[2] as f64, u[2] as f64, f[2] as f64, cam.eye[2] as f64],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let to_view_inverse = inverse(&view_to_clip)?;
    // Clip to the picture before's clip, and back.
    let clip_to_prev = mul(&world_to_clip(prev), &mul(&view_to_world, &to_view_inverse));
    let prev_to_clip = inverse(&clip_to_prev)?;
    let flat = |m: &M| -> [f32; 16] {
        // Row vectors: the transpose of the column-vector matrix, stored row by row.
        let t = transpose(m);
        let mut o = [0f32; 16];
        for i in 0..4 {
            for j in 0..4 {
                o[i * 4 + j] = t[i][j] as f32;
            }
        }
        o
    };
    let c = Constants {
        view_to_clip: flat(&view_to_clip),
        clip_to_view: flat(&to_view_inverse),
        clip_to_prev: flat(&clip_to_prev),
        prev_to_clip: flat(&prev_to_clip),
        jitter,
        mvec_scale: [1.0 / render.0 as f32, 1.0 / render.1 as f32],
        position: cam.eye,
        up: u,
        right: r,
        forward: f,
        near: crate::native::NEAR,
        far: crate::native::FAR,
        fov: 2.0 * cam.tan[1].atan(),
        aspect: cam.tan[0] / cam.tan[1],
        reset: reset as u32,
    };
    // SAFETY: `c` is a valid structure for the call.
    result("slSetConstants", unsafe { uya_sl_constants(index, &c) })
}

/// A texture view's Vulkan handles for Streamline. Each is taken in a statement of its own: a texture's or a view's `as_hal`
/// guard holds the device's snatch lock for reading until it drops, and a second read taken while one is held waits behind a
/// writer that is waiting (the lock lets no new reader past it) while the writer waits for the first read. Taken in one
/// expression (the texture's guard alive while the view's was taken), a present's write between the two (the presenter's
/// `Queue::present`) froze the game: the record thread, the presenter and the render thread each waiting on the lock, the game
/// thread on the renderer (the player's session, 24 November 2026, with FRAME GENERATION on).
fn image(adapter: &wgpu::Adapter, view: &wgpu::TextureView, layout: u32, usage: u32) -> Image {
    let texture = view.texture();
    // SAFETY: Vulkan textures and views (the device is Vulkan's); the handles are 64-bit.
    let image = unsafe { std::mem::transmute::<_, u64>(texture.as_hal::<Vulkan>().expect("a Vulkan texture").raw_handle()) };
    let raw_view = unsafe { std::mem::transmute::<_, u64>(view.as_hal::<Vulkan>().expect("a Vulkan view").raw_handle()) };
    let format = unsafe { adapter.as_hal::<Vulkan>().expect("a Vulkan adapter").texture_format_as_raw(texture.format()).as_raw() as u32 };
    Image { image, view: raw_view, layout, width: texture.width(), height: texture.height(), format, usage, flags: 0 }
}

/// `VkImageLayout`s of the states `wgpu::TextureUses::RESOURCE` puts a texture in (wgpu-hal `derive_image_layout`).
const SHADER_READ: u32 = 5;
const DEPTH_READ: u32 = 4;

/// Frame `index`'s depth (`depth`, a depth-only view), motion vectors (`motion`) and the picture before its HUD (`hudless`, the
/// picture's own view, when its size is the swapchain's: DLSS-G wants them equal) tagged for Streamline, in a command buffer to be
/// submitted after the picture's world and DLSS (`dlss::Upscaler::run`). Each is moved to the state its layout names first.
pub(crate) fn tag(device: &wgpu::Device, adapter: &wgpu::Adapter, index: u32, depth: &wgpu::TextureView, motion: &wgpu::TextureView, hudless: &wgpu::TextureView) -> Result<[wgpu::CommandBuffer; 2], String> {
    let (w, h) = (hudless.texture().width(), hudless.texture().height());
    let back = wgpu::hal::vulkan::BACKBUFFER.load(Ordering::Relaxed);
    let hudless_ok = back == (((w as u64) << 32) | h as u64);
    // Two encoders: wgpu's API moves the textures to their states, the raw one holds Streamline's copies (an encoder takes one API).
    let mut states = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame generation states") });
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("frame generation tags") });
    fn transition(v: &wgpu::TextureView) -> wgpu::TextureTransition<&wgpu::Texture> {
        wgpu::TextureTransition { texture: v.texture(), selector: None, state: wgpu::TextureUses::RESOURCE }
    }
    let mut all = vec![transition(depth), transition(motion)];
    if hudless_ok {
        all.push(transition(hudless));
    }
    states.transition_resources(std::iter::empty(), all.into_iter());
    // VkImageUsageFlags of the textures as they are made: transfer source 1, sampled 4, colour attachment 0x10, depth attachment 0x20.
    let depth_image = image(adapter, depth, DEPTH_READ, 0x20 | 0x4 | 0x1);
    let motion_image = image(adapter, motion, SHADER_READ, 0x10 | 0x4 | 0x1);
    let hudless_image = image(adapter, hudless, SHADER_READ, 0x10 | 0x4 | 0x1 | 0x2);
    let mut r = 0;
    // SAFETY: the Vulkan command buffer of the encoder, recording; the images are valid for the call.
    unsafe {
        enc.as_hal_mut::<Vulkan, _, _>(|e| {
            let cmd = std::mem::transmute::<_, u64>(e.expect("a Vulkan encoder").raw_handle());
            r = uya_sl_tag(index, cmd, &depth_image, &motion_image, if hudless_ok { &hudless_image } else { std::ptr::null() });
        });
    }
    result("slSetTagForFrame", r)?;
    Ok([states.finish(), enc.finish()])
}

/// What the presenter's hook counts for the log: real presents and the frames DLSS-G reported presenting, since the last line.
static COUNT: Mutex<(u64, u64, Option<std::time::Instant>)> = Mutex::new((0, 0, None));
static CURRENT: AtomicU32 = AtomicU32::new(0);

/// `wgpu::hal::vulkan::PRESENT_HOOK`: around each `vkQueuePresentKHR` (`start` before it). The picture presented is the one the
/// presenter named (`presenting`): its markers, and DLSS-G's options (the setting's frames to generate if it was tagged, else
/// off). A queue of the frames begun, the oldest taken at each present, lost its place at a picture recorded and not presented
/// and logged "Unable to find 'common' constants" once a run. With frame generation off (`running`) a present passes through
/// untouched, after the one that turns it off: DLSS-G's options off, Reflex off.
fn present(start: bool) {
    if LOADED.load(Ordering::Relaxed) != ACTIVE {
        return;
    }
    if start {
        let on = running();
        let was = ON.swap(on, Ordering::Relaxed);
        let tagged = Some(PRESENTING.swap(0, Ordering::Relaxed)).filter(|&i| i != 0);
        if !on && !was {
            CURRENT.store(0, Ordering::Relaxed);
            ASKED.store(0, Ordering::Relaxed);
            crate::play::FG_GENERATED.store(0, Ordering::Relaxed);
            SHOWN.store(1, Ordering::Relaxed);
            return;
        }
        if on != was {
            // SAFETY: a plain call into the layer, on the presenting thread.
            if let Err(e) = result(if on { "Reflex on" } else { "Reflex off" }, unsafe { uya_sl_reflex(on as i32) }) {
                fail(e);
                return;
            }
            println!("streamline: frame generation {}", if on { format!("on, {} generated per real picture", generated()) } else { "off".to_string() });
        }
        let index = tagged.unwrap_or_else(|| NEXT.fetch_add(1, Ordering::Relaxed));
        CURRENT.store(index, Ordering::Relaxed);
        let want = if tagged.is_some() && on { generated() } else { 0 };
        // Asked for frames anew (turned on, the first picture with DLSS after the menus, another setting): the presenter leaves
        // room for them until DLSS-G shows the real pictures alone (the present's end).
        if ASKED.swap(want, Ordering::Relaxed) != want {
            ALONE.store(0, Ordering::Relaxed);
            crate::play::FG_GENERATED.store(want, Ordering::Relaxed);
        }
        // SAFETY: plain calls into the layer, on the presenting thread.
        let r = unsafe {
            [if tagged.is_none() { uya_sl_sleep(index) } else { 0 }, uya_sl_marker(RENDER_SUBMIT_END, index), uya_sl_options(want, 0), uya_sl_marker(PRESENT_START, index)]
        };
        if let Some(e) = r.iter().find(|r| **r != 0) {
            fail(format!("the present's options and markers: {e}"));
        }
    } else {
        let index = CURRENT.load(Ordering::Relaxed);
        if index == 0 {
            return;
        }
        let (mut most, mut status, mut presented, mut side) = (0, 0, 0, 0);
        // SAFETY: plain calls into the layer, on the presenting thread.
        let r = unsafe {
            let m = uya_sl_marker(PRESENT_END, index);
            m | uya_sl_state(&mut most, &mut status, &mut presented, &mut side)
        };
        if r != 0 {
            fail(format!("the present's end marker and DLSS-G's state: {r}"));
            return;
        }
        SHOWN.store(presented, Ordering::Relaxed);
        // What the presenter leaves room for follows what DLSS-G shows: none once it shows the real pictures alone
        // (`ALONE_RUN` in a row: its window-focus test failed, or anything else), the setting's again at its first generated frame.
        let want = ASKED.load(Ordering::Relaxed);
        if want > 0 {
            if presented > 1 {
                ALONE.store(0, Ordering::Relaxed);
                if crate::play::FG_GENERATED.swap(want, Ordering::Relaxed) == 0 {
                    println!("streamline: DLSS-G generates frames again: the presenter leaves room for {want} between its pictures");
                }
            } else if ALONE.fetch_add(1, Ordering::Relaxed) + 1 == ALONE_RUN && crate::play::FG_GENERATED.swap(0, Ordering::Relaxed) != 0 {
                println!("streamline: DLSS-G showed {ALONE_RUN} real pictures in a row alone, asked for {want} frames between them (streamline-logs\\sl.log says why: \"window not focused\" while another program's window is in front): the presenter draws its pictures as with FRAME GENERATION off until DLSS-G generates again");
            }
        }
        let mut c = COUNT.lock().expect("streamline count lock");
        let now = std::time::Instant::now();
        let since = *c.2.get_or_insert(now);
        c.0 += 1;
        c.1 += presented as u64;
        if status != 0 {
            println!("streamline: DLSS-G status {status:#x} (1 the picture is too small, 2 Reflex not detected, 4 HDR format, 8 the constants are invalid)");
        }
        let secs = now.duration_since(since).as_secs_f64();
        if secs >= 2.0 {
            println!("streamline: {} real pictures presented and {} frames shown by DLSS-G in {secs:.1} s: {:.1} real a second, {:.1} frames a second, {} generated per real picture asked, the presenter leaving room for {}", c.0, c.1, c.0 as f64 / secs, c.1 as f64 / secs, ASKED.load(Ordering::Relaxed), crate::play::FG_GENERATED.load(Ordering::Relaxed));
            *c = (0, 0, Some(now));
        }
    }
}

/// Streamline shut down, before the device goes (`Native`'s end).
pub(crate) fn shutdown() {
    if LOADED.load(Ordering::Relaxed) != OFF || wanted_at_start() {
        // SAFETY: the last call into the layer.
        let r = unsafe { uya_sl_shutdown() };
        println!("streamline: shut down ({r})");
    }
}
