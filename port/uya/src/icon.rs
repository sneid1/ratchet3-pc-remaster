//! The portrait shared by the game window, taskbar and desktop shortcut.
use winit::window::Icon;

const PORTRAIT: &[u8] = include_bytes!("../assets/portrait.ico");

pub fn write(path: &str) -> Result<(), String> {
    let path = std::path::Path::new(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("icon: {}: {e}", dir.display()))?;
    }
    std::fs::write(path, PORTRAIT).map_err(|e| format!("icon: {}: {e}", path.display()))
}

/// Decode one PNG entry from the bundled multi-resolution ICO.
pub fn portrait(size: u8) -> Result<Icon, String> {
    let count = u16::from_le_bytes([PORTRAIT[4], PORTRAIT[5]]) as usize;
    let entry = PORTRAIT[6..].chunks_exact(16).take(count)
        .find(|entry| entry[0] == size && entry[1] == size)
        .ok_or_else(|| format!("icon: missing {size} px portrait"))?;
    let length = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
    let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
    let (width, height, rgba) = crate::pic::read_png(&PORTRAIT[offset..offset + length])?;
    Icon::from_rgba(rgba, width, height).map_err(|e| format!("icon: {e}"))
}
