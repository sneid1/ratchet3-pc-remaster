wgpu-hal 30.0.1 as shipped on crates.io, with THREE changes for the port (MIT OR Apache-2.0, see LICENSE.MIT / LICENSE.APACHE).
The port's native renderer (port/uya) and its play window draw through wgpu 30; this copy is the one the port's workspace builds
(port/Cargo.toml, [patch.crates-io]). It was first made for rac-rs (extern/rac-rs/rac-rs/vendor/wgpu-hal, the same files) and
moved into the port with PLAN item 0.6, so the game's build reads nothing from rac-rs's tree.

What changed
  src/dx12/instance.rs, in `Instance::init`: `let factory_media = lib_dxgi.create_factory_media().ok();` became
  `let factory_media = None;` (plus comment lines), and `#[allow(dead_code)]` on `create_factory_media` in src/dx12/mod.rs
  (the function is no longer called). Apart from the second and third changes below, nothing else differs from the published crate (compare with
  `diff -r` against the crate unpacked from crates.io; only the .cargo-ok marker file is missing here).

Why
  wgpu-hal's DirectX 12 start-up always calls CreateDXGIFactory1(IID_IDXGIFactoryMedia). That factory is used only for
  SurfaceTarget::SurfaceHandle (composition surface handles), never for window (HWND) surfaces, which is all the port uses.
  ReShade 6.8.0 hooks CreateDXGIFactory1 in its dxgi.dll proxy and its hook crashes (access violation) on that interface id,
  so a wgpu program could not start on DirectX 12 with ReShade installed. A SurfaceHandle surface now fails with wgpu's own
  error "IDXGIFactoryMedia not found"; the port does not use one. wgpu has no option that avoids the call (checked in 30.0.1).

How it is used
  port/Cargo.toml: [patch.crates-io] wgpu-hal = { path = "vendor/wgpu-hal" } (cargo reads [patch] only at the workspace root).

How to re-apply on a wgpu upgrade
  1. Copy the new wgpu-hal-X.Y.Z from the cargo registry over this folder (leave out .cargo-ok).
  2. In src/dx12/instance.rs find `create_factory_media()` in `Instance::init` and replace the call with `None`.
  3. If a wgpu release stops creating the factory unconditionally (or ReShade's hook survives it), delete this folder and the
     [patch] lines instead.

Streamline (second change, for DLSS Frame Generation; port/uya/src/streamline.rs)
  src/vulkan/mod.rs: `LOADER` (a path), `PRESENT_HOOK` (called with true before and false after each vkQueuePresentKHR) and
  `BACKBUFFER` (the last swapchain's size). src/vulkan/instance.rs: `Instance::init_with_callback` loads the Vulkan loader from
  `LOADER` when it is set, else `ash::Entry::load()` as before. src/vulkan/swapchain/native.rs: the hook around the present, and
  `BACKBUFFER` set where the swapchain is made. With none of them set the backend is the published crate's.

Older Intel drivers (third change, marked "PORT CHANGE" in src/vulkan/descriptor.rs)
  `create_descriptor_pool`: a pool whose layout has no descriptors gets one sampler entry instead of an empty `pPoolSizes`
  list. This is wgpu trunk's own fix, gfx-rs/wgpu#10124 (merged 3 September 2026, in no 30.x release), copied as it is.
  Why: wgpu 30's descriptor allocator (gfx-rs/wgpu#9161) makes a pool as soon as a layout is registered, and wgpu-core makes
  an empty layout while it makes every device, so `vkCreateDescriptorPool` got `poolSizeCount` 0. That is valid only from
  Vulkan 1.3.215; Intel's Windows drivers for 6th to 10th generation graphics read the first entry anyway and crash in
  igvk64.dll inside `request_device`. A player's report (GitHub issue #2, Intel HD Graphics 520, v0.1.0 and v0.1.5): play.log
  ended with an access violation in igvk64.dll on the render thread, after Streamline's device set-up and before the line the
  game prints once its device exists. On this PC (an instrument layer logging the calls) that pool was the first one v0.1.5
  made, with no sizes, and the only such pool in a run.
  On a wgpu upgrade: if the new release has #10124 (CHANGELOG: "Fix crash on older Vulkan drivers when `poolSizeCount == 0`"),
  drop this change; otherwise apply it again.
