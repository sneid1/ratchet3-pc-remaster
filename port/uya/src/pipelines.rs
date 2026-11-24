//! The pipeline cache (PLAN 8.9), always on: every render pipeline the game makes goes through one `wgpu::PipelineCache` (the
//! Vulkan driver's compiled pipelines), read from the profile's folder when the renderer opens and written back after startup
//! and level loading, so later starts can reuse the compiled variants.
//!
//! The renderer makes its shared pipelines when it opens (`native::Native::open`: the terrain,
//! the objects, the shadows and the EE's steps, the sky, ambient occlusion, the water reflections, the light shafts) and the play
//! window with its surface (`play.rs`: the blit, the 2D layer of the front end). GPU vegetation adds two pipelines during level
//! loading because its texture-array layout depends on that level's materials (`shrub_batch`). They use this cache too;
//! `Native::load` saves it after making them. Meeting a new GS state during play makes bind groups and samplers only.
//!
//! `uya play` keeps the cache (`open` with the profile's folder before the renderer opens); `uya native` and the reference keep
//! none (`cache` is None: their pipelines are made as before).

use std::path::PathBuf;
use std::sync::OnceLock;

/// The folder the cache is kept in, set by `open`.
static DIR: OnceLock<PathBuf> = OnceLock::new();
/// The cache, made with the renderer's device (`make`), and the file it is kept in with the bytes read from it.
static CACHE: OnceLock<(wgpu::PipelineCache, PathBuf, Vec<u8>)> = OnceLock::new();

/// Keep the pipeline cache in `dir` (the profile's folder): the renderer opened next makes its pipelines through it.
pub fn open(dir: &std::path::Path) {
    if DIR.set(dir.to_path_buf()).is_err() {
        panic!("pipelines::open: the cache's folder is set already");
    }
}

/// Whether a cache is to be kept: the device needs `wgpu::Features::PIPELINE_CACHE` (Vulkan's).
pub(crate) fn wanted() -> bool {
    DIR.get().is_some()
}

/// Makes the cache for `device` (on `adapter`) from its file, when one is to be kept: the file is named by wgpu's key for the
/// adapter (its vendor and device), and data from another driver version is set aside by wgpu, which starts the cache empty.
pub(crate) fn make(device: &wgpu::Device, adapter: &wgpu::Adapter) -> Result<(), String> {
    let Some(dir) = DIR.get() else { return Ok(()) };
    let info = adapter.get_info();
    let key = wgpu::util::pipeline_cache_key(&info).ok_or_else(|| format!("pipelines: {} ({:?}) keeps no pipeline cache", info.name, info.backend))?;
    let path = dir.join(format!("{key}.bin"));
    let data = match std::fs::read(&path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(format!("pipelines: {}: {e}", path.display())),
    };
    // SAFETY: the data is what `PipelineCache::get_data` wrote for an adapter of the same key (or nothing); wgpu checks its
    // header against the device and driver and starts the cache empty when it does not match (`fallback`).
    let cache = unsafe { device.create_pipeline_cache(&wgpu::PipelineCacheDescriptor { label: Some("uya pipelines"), data: (!data.is_empty()).then_some(&data[..]), fallback: true }) };
    println!("pipelines: {} ({} bytes)", path.display(), data.len());
    CACHE.set((cache, path, data)).map_err(|_| "pipelines: the cache is made already".to_string())
}

/// The cache every pipeline is made through (`RenderPipelineDescriptor::cache`); None where none is kept.
pub(crate) fn cache() -> Option<&'static wgpu::PipelineCache> {
    CACHE.get().map(|c| &c.0)
}

/// Writes the cache to its file when its bytes differ from the loaded file: called after startup and level loading.
pub fn save() -> Result<(), String> {
    let Some((cache, path, read)) = CACHE.get() else { return Ok(()) };
    let data = cache.get_data().ok_or_else(|| format!("pipelines: {}: the cache gives no data", path.display()))?;
    if data == *read {
        return Ok(());
    }
    // Written beside the file and moved over it: a cut-short write never leaves half a cache.
    let part = path.with_extension("part");
    std::fs::write(&part, &data).map_err(|e| format!("pipelines: {}: {e}", part.display()))?;
    std::fs::rename(&part, path).map_err(|e| format!("pipelines: {}: {e}", path.display()))?;
    println!("pipelines: {} written ({} bytes)", path.display(), data.len());
    Ok(())
}
