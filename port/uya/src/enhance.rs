//! The remaster's graphics options (PLAN item 8) on the renderer's side, process-wide like a game's options: the values of the
//! GRAPHICS tab's options (`settings::OPTIONS`), set by `play` when the player changes them and by `uya native --set`. Each is
//! off (0) by default, and with every one off the picture is the game's.
//!
//! - `texture_filtering`: off, the GS's own filtering of the game's textures (their levels picked from Q and TEX1 K for the
//!   PS2's 448 lines); 2x to 16x, every one of the game's textures sampled at the level its own texel density on screen picks,
//!   trilinear, anisotropic up to that many samples. A pack replacement (`texnames`) is always drawn trilinear at 16x.
//! - `ambient_occlusion` (`ao.rs`): low and high, on the level's static world and its mobies.
//! - `lighting` (`lighting.rs`): per pixel, the game's own light passes evaluated at every pixel.
//! - `rift_apart_surfaces` (UPDATED TEXTURES; `texnames`, `lighting::MATERIAL`): which replacement textures are drawn, by level
//!   (`texture_level`): OFF none (the packs too), HUD ONLY the 2D layer's, LOW and MEDIUM every replacement with its top mip
//!   levels dropped to a cap (`texture_caps`), MAX every replacement at its own size, and AUTO the highest of these the GPU's
//!   memory takes (`auto_textures`, decided as the renderer opens its GPU). From LOW up the surfaces the pack folder's
//!   `0-riftapart` pairs (PLAN 9.2) are drawn with their Rift Apart material on the game's own texture (the pack's): its grain,
//!   its relief and its gloss, lit per pixel.

use std::sync::atomic::{AtomicU8, Ordering};

const NONE: AtomicU8 = AtomicU8::new(0);
static LEVELS: [AtomicU8; crate::settings::OPTIONS.len()] = [NONE; crate::settings::OPTIONS.len()];

/// The renderer's options from now on: those of `s`.
pub(crate) fn set(s: &crate::settings::Settings) {
    for (l, v) in LEVELS.iter().zip(s.values) {
        l.store(v, Ordering::Relaxed);
    }
    // The per-pixel lighting of the mobies takes the skin job's joint matrices (`mobies::Mobies::skin_pass`).
    uyart::SKIN_JOINTS.store(lighting(), Ordering::Relaxed);
}

/// Option `key`'s value now (its place in its `values`).
pub(crate) fn level(key: &str) -> u8 {
    LEVELS[crate::settings::index(key)].load(Ordering::Relaxed)
}

/// FRAME GENERATION: frames DLSS-G is to generate per picture the port draws (0 off, 1 for 2X, 2 for 3X, 3 for 4X). What it
/// is generating now is `play::FG_GENERATED`.
pub(crate) fn frame_generation() -> u32 {
    u32::from(level("frame_generation"))
}

/// FRAME PACING UNCAPPED (`play.rs` `Timeline`): the pictures drawn back to back, each shown once done.
pub(crate) fn uncapped() -> bool {
    level("frame_pacing") == 0
}

/// Whether the updated textures' surfaces are there to draw (`texnames::open`: the updated-textures folder found and read).
static RIFT_SURFACES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The updated textures' surfaces read, or not there (`texnames::open`); the skin job's joint matrices follow (`set`).
pub(crate) fn set_rift_surfaces(there: bool) {
    RIFT_SURFACES.store(there, Ordering::Relaxed);
    uyart::SKIN_JOINTS.store(lighting(), Ordering::Relaxed);
}

/// UPDATED TEXTURES' levels (`texture_level`): the first five values of `rift_apart_surfaces`, in order.
pub(crate) const TEXTURES_OFF: u8 = 0;
pub(crate) const TEXTURES_HUD: u8 = 1;
pub(crate) const TEXTURES_LOW: u8 = 2;
pub(crate) const TEXTURES_MEDIUM: u8 = 3;
pub(crate) const TEXTURES_MAX: u8 = 4;
/// The value of `rift_apart_surfaces` that is AUTO (after the five levels).
pub(crate) const TEXTURES_AUTO: u8 = 5;

/// The level AUTO chose for this GPU (`auto_textures`); OFF until the renderer has opened its GPU, before which no texture is read.
static AUTO: AtomicU8 = AtomicU8::new(TEXTURES_OFF);

/// The UPDATED TEXTURES level drawn now, `TEXTURES_OFF` to `TEXTURES_MAX`: the option's own, or for AUTO the level chosen for this GPU.
pub(crate) fn texture_level() -> u8 {
    match level("rift_apart_surfaces") {
        l if l < TEXTURES_AUTO => l,
        _ => AUTO.load(Ordering::Relaxed),
    }
}

/// The largest side, in texels, a replacement texture and a Rift Apart material map are kept at (`texnames`: their top mip levels
/// are dropped until they are no bigger): at LOW 256 and 128, at MEDIUM 512 and 256; None at the other levels (their own size).
/// Measured with `UYA_MEM_LOG` (`auto_textures`): LOW's textures fit a 2 GB card, MEDIUM's a 4 GB card. The first caps tried,
/// 512 and 256 for LOW and 1024 and 512 for MEDIUM, took 1.6 and 3.3 GB: with the GPU held down to 3.4 GB free (a 4 GB card)
/// MEDIUM's 2.9 GB ran at 21 ms a picture, LOW's 1.6 GB at 1.8 ms.
pub(crate) fn texture_caps() -> Option<(u32, u32)> {
    match texture_level() {
        TEXTURES_LOW => Some((256, 128)),
        TEXTURES_MEDIUM => Some((512, 256)),
        _ => None,
    }
}

/// Whether the pack folder's `0-riftapart` surfaces are drawn (`texnames`, `rift_apart_surfaces`): the option at LOW or above and the
/// updated textures there. Without them (a release installed without the updated-textures volumes, the usual way) the option,
/// on by default, drew nothing new but still lit every pixel (`lighting`): the world's per-pixel lighting and the mobies' skin
/// job capture, three times the frame's jobs a field, at a cost a weak PC could not pay, with LIGHTING at VERTEX (two v0.1.3
/// players without the volumes: a Ryzen 5 4600G's integrated graphics at 20 to 22 pictures a second, a GTX 1650 Ti).
pub(crate) fn rift_apart() -> bool {
    texture_level() >= TEXTURES_LOW && RIFT_SURFACES.load(Ordering::Relaxed)
}

/// The anisotropy the game's own textures are sampled with (`texture_filtering`): None, the GS's own filtering.
pub(crate) fn aniso() -> Option<u16> {
    match level("texture_filtering") {
        0 => None,
        l => Some(1 << l),
    }
}

/// The water reflections' level (`reflections.rs`): 0 off, 1 low (the reflection at half the picture's size), 2 high.
pub(crate) fn reflections() -> u8 {
    level("water_reflections")
}

/// The light shafts' level (`shafts.rs`): 0 off, 1 low, 2 high.
pub(crate) fn shafts() -> u8 {
    level("light_shafts")
}

/// The ambient occlusion's level: 0 off, 1 low, 2 high.
pub(crate) fn ao() -> u8 {
    level("ambient_occlusion")
}

/// The real-time shadows' level: 0 off, 1 low, 2 medium, 3 high (`shadows.rs`: the maps' size), 4 ray traced
/// (`raytrace::RAY_TRACED`).
pub(crate) fn shadows() -> u8 {
    level("shadows")
}

/// The ray-traced lighting's level (`rtlight.rs`): 0 off, 1 low, 2 high.
pub(crate) fn rt_lighting() -> u8 {
    level("ray_traced_lighting")
}

/// The ray-traced reflections' level (`rtlight.rs`): 0 off, 1 low, 2 high.
pub(crate) fn rt_reflections() -> u8 {
    level("ray_traced_reflections")
}

/// Whether the FPS counter is shown (`play.rs` `Fps`, the DISPLAY tab's FPS COUNTER).
pub(crate) fn fps_counter() -> bool {
    level("fps_counter") == 1
}

/// DLSS's level (`dlss.rs`): 0 off, 1 DLAA, 2 quality, 3 balanced, 4 performance.
pub(crate) fn dlss() -> u8 {
    level("dlss")
}

/// Whether the world is lit per pixel (`lighting.rs`): the LIGHTING option at PER-PIXEL, or the Rift Apart surfaces on (their
/// materials' relief and gloss are lit at every pixel, `lighting::MATERIAL`).
pub(crate) fn lighting() -> bool {
    level("lighting") > 0 || rift_apart()
}

/// The GPU memory (MB) the game reserves at 1080 lines in the heaviest level, level 23 (`UYA_MEM_LOG` at field 900 with the
/// community packs and the updated textures), at each level: HUD ONLY, LOW, MEDIUM, MAX. Every story level was measured at MAX
/// (27 November 2026): 2,496 MB (level 34) to 4,672 (level 23), Veldin 4,288, the figure AUTO first took, which a GPU sized for
/// Veldin would have overrun on levels 4, 16, 18 and 23; at MEDIUM level 18, with the most Rift Apart materials, took 1,600.
/// OFF is 448 on Veldin: the game's own 0.45 GB. The reserved figure counts the driver's blocks, so it is more than the
/// allocations (MAX 4,543). HUD ONLY's uncapped HUD takes more than LOW, so AUTO, which picks the highest level that fits, never
/// picks it: a GPU that fits it fits LOW.
const LEVEL_MB: [f64; 4] = [1088.0, 832.0, 1728.0, 4672.0];

/// Of those, the picture's render targets and copies at 1080 lines (the native picture, the EE's targets, the copies and the
/// grabbed frame: 175 MB), which grow with the square of the height.
const TARGETS_MB: f64 = 175.0;

/// The share of the budget the game plans to use: the budget moves with the other programs' use of the GPU.
const BUDGET_USE: f64 = 0.95;

/// AUTO's level for a GPU with `budget` MB of video memory (an integrated GPU's with `ram` MB of system memory beside it) at
/// `height` picture lines: the highest level whose textures and the game's other memory fit (HUD ONLY's uncapped pictures take
/// more than LOW's); OFF when none does.
fn auto_level(budget: f64, integrated: bool, ram: f64, height: u32) -> u8 {
    // An integrated GPU's memory is the PC's: no more than a quarter of it (the game's own 1.4 GB and Windows keep the rest).
    let budget = if integrated { budget.min(ram / 4.0) } else { budget } * BUDGET_USE;
    let targets = TARGETS_MB * ((height as f64 / 1080.0).powi(2) - 1.0);
    [TEXTURES_MAX, TEXTURES_MEDIUM, TEXTURES_LOW, TEXTURES_HUD]
        .into_iter()
        .find(|&l| LEVEL_MB[l as usize - 1] + targets <= budget)
        .unwrap_or(TEXTURES_OFF)
}

/// The video memory the OS lets this process use on the adapter with PCI ids `vendor` and `device` (DXGI's
/// `IDXGIAdapter3::QueryVideoMemoryInfo`, the local segment), and the PC's physical memory, in MB.
#[cfg(windows)]
fn video_memory(vendor: u32, device: u32) -> Result<(f64, f64), String> {
    use windows::Win32::Graphics::Dxgi::{CreateDXGIFactory1, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, DXGI_QUERY_VIDEO_MEMORY_INFO, IDXGIAdapter3, IDXGIFactory1};
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    use windows::core::Interface;
    let e = |what: &str| { let what = what.to_string(); move |e: windows::core::Error| format!("textures: AUTO: {what}: {e}") };
    // SAFETY: plain DXGI and Win32 calls on interfaces and a struct made here.
    unsafe {
        let factory: IDXGIFactory1 = CreateDXGIFactory1().map_err(e("CreateDXGIFactory1"))?;
        let mut i = 0;
        let adapter = loop {
            let a = factory.EnumAdapters1(i).map_err(|_| format!("textures: AUTO: DXGI has no adapter with vendor {vendor:#x} device {device:#x}"))?;
            let d = a.GetDesc1().map_err(e("GetDesc1"))?;
            if d.VendorId == vendor && d.DeviceId == device {
                break a;
            }
            i += 1;
        };
        let adapter: IDXGIAdapter3 = adapter.cast().map_err(e("IDXGIAdapter3"))?;
        let mut info = DXGI_QUERY_VIDEO_MEMORY_INFO::default();
        adapter.QueryVideoMemoryInfo(0, DXGI_MEMORY_SEGMENT_GROUP_LOCAL, &mut info).map_err(e("QueryVideoMemoryInfo"))?;
        let mut ram = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
        GlobalMemoryStatusEx(&mut ram).map_err(e("GlobalMemoryStatusEx"))?;
        Ok((info.Budget as f64 / 1048576.0, ram.ullTotalPhys as f64 / 1048576.0))
    }
}

/// UPDATED TEXTURES' AUTO (`texture_level`): the highest level whose textures fit the GPU the renderer draws on, decided from the
/// OS's budget for it as the renderer opens it (`native::Native::open`); one log line says what was found and chosen.
#[cfg(windows)]
pub(crate) fn auto_textures(info: &wgpu::AdapterInfo, height: u32) -> Result<(), String> {
    let (budget, ram) = video_memory(info.vendor, info.device)?;
    let integrated = info.device_type == wgpu::DeviceType::IntegratedGpu;
    let level = auto_level(budget, integrated, ram, height);
    let name = ["OFF", "HUD ONLY", "LOW", "MEDIUM", "MAX"][level as usize];
    println!("textures: AUTO: {} budget {budget:.0} MB{}: {name}", info.name, if integrated { format!(" (integrated, {ram:.0} MB of system memory)") } else { String::new() });
    AUTO.store(level, Ordering::Relaxed);
    uyart::SKIN_JOINTS.store(lighting(), Ordering::Relaxed);
    Ok(())
}
