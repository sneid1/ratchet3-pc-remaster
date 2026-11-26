//! The native world renderer (PLAN item 1.1): this project's own renderer, on wgpu, drawing what this project's R&C3 decoders
//! (`uyafmt`) return, with the game's own camera and per-frame visibility read from EE RAM. No rac-rs renderer code and no
//! rac-formats type: the scene is uyafmt's data as it comes.
//!
//! So far it draws the terrain (`uyafmt::terrain`) as func_L01_004ADC98 and VU1 program 1 do, at full detail:
//! - which chunks: those whose +0x3A mask ANDed with the byte at their +0x3B zone of the frame's zone record (0x80 bytes the game
//!   builds each frame for the camera's cell, func_L01_003FEE38) is not zero. Veldin field 2400: the game sends exactly the 99
//!   chunks in view that pass this test (the VIF1 trace), and 669 chunks in view fail it. The view itself needs no test: the GPU
//!   clips;
//! - every chunk at full detail (the near level's strips, every vertex at its own position), in chunk order and strip order, each
//!   vertex with the colour the game's terrain light pass gives it, each strip with its GS register record's texture, clamp and
//!   mip settings;
//! - the pixel as the GS makes it from the terrain's registers: TEX0 TFX MODULATE, TCC RGBA (texel x vertex colour / 0x80, alpha
//!   likewise); TEX1 LCM 0 (the mip level from the pixel's Q: log2(1/Q) + K, at most the texture's last level; MMAG and MMIN as
//!   the record gives them); PRIM FGE, the frame's fog (the view struct's, read each frame: F per vertex from the view depth as the terrain
//!   program computes it, the colour (F x colour + (255 - F) x FOGCOL) / 256); then the GS state the game sets before the terrain
//!   (Veldin field 2400, the GS stream): TEST_1 0x5360B (alpha test GEQUAL 0x60, a failing pixel writes its colour but not its
//!   depth; Z test GEQUAL) and ALPHA_1 0x44 with PRIM ABE (colour = (Cs - Cd) x As / 0x80 + Cd).
//!
//! The picture is drawn offscreen into an RGBA8 target (the GS writes raw bytes) with a depth buffer, and read back.

use crate::pic::Image;
use std::collections::HashMap;

/// A level's globals the renderer reads each frame: the camera the world is drawn with (`view_camera`: the rows forward, right,
/// up from +0, the eye at -0x20; func_L01_003FE8F8, the only writer of the view matrix, builds it from them; in gameplay a copy
/// of GameCamera, in cutscenes the cutscene's camera); the view's half-angle tangents (horizontal, vertical) that
/// view_projection_setup writes [C: NTSC-U Veldin field 2400: position (365.3, 98.6, 80.6), orthonormal rows, tangents 0.62 and
/// 0.4805 = 0.62 x 0.775]; the terrain's zone record for the camera's cell (func_L01_003FEE38 writes it, func_L01_004ADC98
/// copies it to the scratchpad and tests each chunk against it). All live in the level overlay's .bss, so each level has its own
/// addresses.
#[derive(Clone, Copy)]
struct Globals {
    view_camera: u32,
    tangents: u32,
    zones: u32,
    /// The pointer to the level's sky (func_L01_004A46C8 reads it at 0x1E0E70 on Veldin).
    sky: u32,
    /// The shrub arrays (gp+0x4594 to gp+0x45A8 on Veldin, gp = 0x1DC8B0).
    shrubs: crate::objects::ShrubGlobals,
    /// The tie arrays (gp+0x46A4 to gp+0x46B4, the limit gp+0x46E4).
    ties: crate::objects::TieGlobals,
    /// The number of the level chunk whose terrain is in the terrain buffer (0 is the core's terrain, which the level's setup
    /// puts there; N > 0 is the LEVEL file's block 4 + N).
    level_chunk: u32,
    /// The mobies' record list head, the core's RAM base and the view camera (src/mobies.rs).
    mobies: crate::mobies::MobyGlobals,
    /// Program 6's inputs (`uyafmt::fxcode`).
    fx: uyafmt::prog6::FxAddrs,
    /// The variable holding the HUD header's copy (`uyafmt::levelcode::LevelGlobals::hud_header`).
    hud_header: u32,
    /// The menu's WAD entry loader, when the level has it (`uyafmt::levelcode::LevelGlobals::menu_entry`).
    menu_entry: Option<uyafmt::levelcode::MenuEntry>,
    /// The WAD streaming slots (`uyafmt::levelcode::LevelGlobals::stream_slots`).
    stream_slots: uyafmt::levelcode::StreamSlots,
    /// The level's light sets (`uyafmt::levelcode::LevelGlobals::light_sets`): the per-pixel lighting's (`lighting.rs`).
    light_sets: u32,
}

/// Every story level's globals, read from its own code on the disc (uyafmt::levelcode: each address from the instruction that
/// forms it in the level's copy of the function that does so on Veldin).
fn all_globals(iso: &str) -> Result<HashMap<u32, Globals>, String> {
    let all = uyafmt::levelcode::all(iso)?;
    let scenes: HashMap<u32, uyafmt::scenecode::SceneGlobals> = uyafmt::scenecode::all(iso)?.into_iter().map(|g| (g.level as u32, g)).collect();
    let fx: HashMap<u32, uyafmt::prog6::FxAddrs> = uyafmt::fxcode::all(iso)?.into_iter().map(|(l, a)| (l as u32, a)).collect();
    all.into_iter()
        .map(|g| -> Result<(u32, Globals), String> {
            let (s, t) = (g.shrubs, g.ties);
            let globals = Globals {
                view_camera: g.view_camera,
                tangents: g.tangents,
                zones: g.zones,
                sky: g.sky,
                shrubs: crate::objects::ShrubGlobals { count: s.count, instances: s.instances, matrices: s.matrices, colours: s.colours, limit: s.limit },
                ties: crate::objects::TieGlobals {
                    count: t.count,
                    instances: t.instances,
                    matrices: t.matrices,
                    colours: t.colours,
                    list: t.list,
                    list_bytes: t.list_bytes,
                    records: t.records,
                    tests: t.tests,
                    light_sets: g.light_sets,
                },
                level_chunk: g.level_chunk,
                mobies: crate::mobies::MobyGlobals {
                    core: g.core,
                    view_camera: g.view_camera,
                    armor_entry: g.armor_entry,
                    armor_buffer: g.armor_buffer,
                    wrench_block: g.wrench_block,
                    armor_remap: g.armor_remap,
                    clank: g.clank_texture,
                    ship: g.ship_texture,
                    scene: *scenes.get(&(g.level as u32)).ok_or_else(|| format!("native: no scene variables for level {}", g.level))?,
                    gadget: g.gadget,
                },
                fx: *fx.get(&(g.level as u32)).ok_or_else(|| format!("native: no program 6 addresses for level {}", g.level))?,
                hud_header: g.hud_header,
                menu_entry: g.menu_entry,
                stream_slots: g.stream_slots,
                light_sets: g.light_sets,
            };
            Ok((g.level as u32, globals))
        })
        .collect()
}

fn f32_at(ram: &[u8], a: u32) -> f32 {
    let i = a as usize;
    f32::from_le_bytes([ram[i], ram[i + 1], ram[i + 2], ram[i + 3]])
}

fn v3(ram: &[u8], a: u32) -> [f32; 3] {
    [f32_at(ram, a), f32_at(ram, a + 4), f32_at(ram, a + 8)]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

/// The game's render camera: the eye, the forward and up rows, the half-angle tangents across and up, and how much wider than the
/// game's own view it is drawn (`k`, PLAN 4.2: the screen's width over the original picture's at its height; 1.0 at the
/// original shape).
#[derive(Clone, Copy)]
pub struct Camera {
    pub eye: [f32; 3],
    pub forward: [f32; 3],
    pub up: [f32; 3],
    pub tan: [f32; 2],
    pub k: f32,
}

/// The camera at `g.view_camera` in `ram`. The tangents are the ones the game's view is made with: view_projection_setup's
/// copy of (h, v, +0xB8, +0xBC) at tangents + 0x210 (0x1E78C0 on Veldin; the same offset in every level and both front ends),
/// whose h the translated code reads times the screen's widening and the field of view's factor, and v times the factor
/// (`uycpu::Cpu::widen`, `fov`; h and v themselves at the original shape and view, so the dumps made then redraw as before).
/// `k` is the copy's shape over the game's own (h over v as the game wrote them): the screen's widening alone.
fn camera(ram: &[u8], level: u32, g: &Globals) -> Result<Camera, String> {
    let (eye, forward, up) = (v3(ram, g.view_camera - 0x20), v3(ram, g.view_camera), v3(ram, g.view_camera + 0x20));
    let tan = [f32_at(ram, g.tangents + 0x210), f32_at(ram, g.tangents + 0x214)];
    let k = tan[0] / f32_at(ram, g.tangents) / (tan[1] / f32_at(ram, g.tangents + 4));
    let len = |a: [f32; 3]| dot(a, a).sqrt();
    if !(eye.iter().chain(&forward).chain(&up).all(|x| x.is_finite()) && (len(forward) - 1.0).abs() < 0.01 && (len(up) - 1.0).abs() < 0.01 && tan[0] > 0.0 && tan[1] > 0.0) {
        return Err(format!("native: level {level} camera is not set (eye {eye:?}, forward {forward:?}, up {up:?}, tangents {tan:?})"));
    }
    if !(k.is_finite() && k > 0.0) {
        return Err(format!("native: level {level} view widened by {k} (tangent {} as made, {} as set)", tan[0], f32_at(ram, g.tangents)));
    }
    Ok(Camera { eye, forward, up, tan, k })
}

/// The view of level `level` before its code sets the camera (the fade in after its load: eye, forward and up still zero), for a
/// frame that draws no world (`Native::render`): the EE's screen primitives need only the view's shape, its tangents, which the
/// game sets with the screen; the eye at the origin looking along +x with +z up. A level whose code has set no view yet (its
/// first frame, sent as its code starts, its .bss still zero: level 27 entered from Veldin by the game's own level change, field
/// 907) is at rest as a front end's frame is (`Native::render_screen`): the PS2's frame, tangents 0.62 and 0.4805, at the
/// screen's shape (`uyart::view_k`). Any other tangents not set stop it, naming them.
fn camera_at_rest(ram: &[u8], level: u32, g: &Globals) -> Result<Camera, String> {
    let tan = [f32_at(ram, g.tangents + 0x210), f32_at(ram, g.tangents + 0x214)];
    if tan == [0.0, 0.0] && [f32_at(ram, g.tangents), f32_at(ram, g.tangents + 4)] == [0.0, 0.0] {
        let k = uyart::view_k();
        return Ok(Camera { eye: [0.0; 3], forward: [1.0, 0.0, 0.0], up: [0.0, 0.0, 1.0], tan: [0.62 * k, 0.4805], k });
    }
    let k = tan[0] / f32_at(ram, g.tangents) / (tan[1] / f32_at(ram, g.tangents + 4));
    if !(tan[0] > 0.0 && tan[1] > 0.0 && k.is_finite() && k > 0.0) {
        return Err(format!("native: level {level}: a frame with no world and no view set (tangents {tan:?}, widening {k})"));
    }
    Ok(Camera { eye: [0.0; 3], forward: [1.0, 0.0, 0.0], up: [0.0, 0.0, 1.0], tan, k })
}

/// The width of a picture `height` pixels high whose view is `k` times as wide as the PS2 view's aspect (horizontal over vertical
/// tangent, the game's 16:9 option off: 0.62 / 0.4805).
fn width(height: u32, k: f32) -> u32 {
    (height as f32 * 0.62 / 0.4805 * k).round() as u32
}

/// The depth buffer's format: depth, and the stencil the game's shadow volumes count in (`eedraw.rs`).
pub(crate) const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32FloatStencil8;


/// The near and far planes of the depth buffer, in world units. The GS has no far plane; the far one only bounds the buffer.
pub(crate) const NEAR: f32 = 0.05;
pub(crate) const FAR: f32 = 16384.0;

impl Camera {
    /// The view-projection matrix (column-major): x right (forward x up), y up, w the view depth.
    /// Reversed floating-point depth (1 at NEAR, 0 at FAR) retains distant surface detail for depth-derived lighting normals.
    pub(crate) fn matrix(&self) -> [[f32; 4]; 4] {
        self.matrix_at(self.eye)
    }

    /// The same with the eye at `eye` (the origin for the sky: the camera's rotation and projection without its translation).
    fn matrix_at(&self, eye: [f32; 3]) -> [[f32; 4]; 4] {
        let (f, u) = (self.forward, self.up);
        let r = cross(f, u);
        let (a, b) = (-NEAR / (FAR - NEAR), NEAR * FAR / (FAR - NEAR));
        let rows = [
            [r[0] / self.tan[0], r[1] / self.tan[0], r[2] / self.tan[0], -dot(r, eye) / self.tan[0]],
            [u[0] / self.tan[1], u[1] / self.tan[1], u[2] / self.tan[1], -dot(u, eye) / self.tan[1]],
            [f[0] * a, f[1] * a, f[2] * a, -dot(f, eye) * a + b],
            [f[0], f[1], f[2], -dot(f, eye)],
        ];
        let mut m = [[0f32; 4]; 4];
        for (c, col) in m.iter_mut().enumerate() {
            for (k, v) in col.iter_mut().enumerate() {
                *v = rows[k][c];
            }
        }
        m
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct Vertex {
    pub(crate) pos: [f32; 3],
    pub(crate) uv: [f32; 2],
    pub(crate) colour: [u8; 4],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct ViewU {
    m: [[f32; 4]; 4],
    /// The camera's rotation and projection without its translation (the sky).
    sky: [[f32; 4]; 4],
    eye: [f32; 4],
    forward: [f32; 4],
    /// FOGCOL (bytes) and the fog's near and far depths and F values.
    fog_colour: [f32; 4],
    fog: [f32; 4],
    /// x: how far back toward the frame before a moby vertex is drawn (`objects` `vs_moby`; 0 in the frame's own picture); y: 1
    /// with the per-pixel lighting (`lighting.rs`, `objects` `lit`); z: the scale of the texture coordinates' gradients a pack
    /// replacement and a Rift Apart material pick their level by (DLSS's render width over the picture's, so a texture is
    /// sampled as at the picture's own size; 1 without DLSS). The frame's light sets follow in the buffer (`SETS_AT`).
    lerp: [f32; 4],
}

/// Where a picture's view buffer holds the frame's light sets (`objects` `View::sets`, 0x400 bytes): after the `ViewU`, which a
/// picture between two frames writes again (`replay.rs`) with them left as they are.
pub(crate) const SETS_AT: u64 = std::mem::size_of::<ViewU>() as u64;

/// The view of camera `cam` with the frame's fog, the mobies drawn `back` of the way toward the frame before (`replay.rs`).
pub(crate) fn view_u(cam: &Camera, fog: &uyafmt::gameplay::Fog, back: f32) -> ViewU {
    view_u_at(cam, fog, back, [0.0; 2], 1.0)
}

/// The same with DLSS's jitter (`dlss::Upscaler::jitter_ndc`): clip x and y moved by `ndc` times w, so every point the view puts
/// in the picture is moved by the same share of a pixel; and its textures' gradient scale `lod` (`ViewU::lerp`).
pub(crate) fn view_u_at(cam: &Camera, fog: &uyafmt::gameplay::Fog, back: f32, ndc: [f32; 2], lod: f32) -> ViewU {
    let jitter = |mut m: [[f32; 4]; 4]| {
        for c in m.iter_mut() {
            c[0] += ndc[0] * c[3];
            c[1] += ndc[1] * c[3];
        }
        m
    };
    ViewU {
        m: jitter(cam.matrix()),
        sky: jitter(cam.matrix_at([0.0; 3])),
        eye: [cam.eye[0], cam.eye[1], cam.eye[2], 0.0],
        forward: [cam.forward[0], cam.forward[1], cam.forward[2], 0.0],
        fog_colour: [fog.colour[0] as f32, fog.colour[1] as f32, fog.colour[2] as f32, 0.0],
        fog: [fog.near, fog.far, fog.near_f, fog.far_f],
        lerp: [back, crate::enhance::lighting() as u32 as f32, lod, 0.0],
    }
}

/// The frame's fog, as a value of its own.
pub(crate) fn fog_copy(f: &uyafmt::gameplay::Fog) -> uyafmt::gameplay::Fog {
    uyafmt::gameplay::Fog { colour: f.colour, near: f.near, far: f.far, near_f: f.near_f, far_f: f.far_f }
}

/// The frame sets: a frame's buffers written each frame (the mobies', the EE steps', the effects', the ties' ranges', the sky's
/// and the shrubs' per-frame values) are a set's, and a frame builds into a set no picture still to be drawn from an earlier
/// frame holds (`replay::Replay`): `play` draws pictures between a frame and the one before while the next is built.
pub(crate) const SETS: usize = 5;

/// One frame set (`SETS`): held while `busy` has another owner (a replay of the frame built into it).
#[derive(Default)]
struct FrameSet {
    busy: std::sync::Arc<()>,
    /// The frame's picture, its depth and its view's buffer, kept for the set's next frame (a picture made
    /// each frame, 25 MB at 1920 x 1080 with its depth, waited longer for its release while the presenter drew between frames, and
    /// this PC's driver charges it to the process's commit).
    picture: Option<Picture>,
    moby: Stream,
    moby_prev: Stream,
    /// The moby vertices' per-pixel lighting (`lighting::MobyLit`, vertex buffer 2 of their runs).
    moby_lit: Stream,
    moby_lights: Stream,
    ee: Stream,
    fx: Stream,
    /// The effects' vertices' places the frame before (`effects::Effects::frame`'s `prev`) and their lighting words, none
    /// (vertex buffers 1 and 2 of `objects`' `vs_moby`, which draws them).
    fx_prev: Stream,
    fx_lit: Stream,
    tie_rooms: Vec<crate::objects::TieRoom>,
    /// The frame's water triangles the reflection is laid over (`eedraw::EeDraw::water`).
    water: Stream,
}

/// What a frame changes in the level's GPU state that every frame shares (the ties' corners laid out anew, the shadow maps'
/// matrices, static maps and uniform, the ray-traced world's new top level), and, for a frame drawn as it is prepared, the
/// commands that draw it. Applied in the frames' order by the one thread that submits: `play`'s presenter with the world
/// deferred to it (`Native::defer_world`: before the frame's first picture, and for a frame never drawn before the next one's),
/// else `Native::render` itself at the frame's end. A frame still to be drawn then never meets a later frame's changes.
#[derive(Default)]
pub struct Outbox {
    pub(crate) writes: Vec<(wgpu::Buffer, u64, Vec<u8>)>,
    pub(crate) commands: Vec<wgpu::CommandBuffer>,
}

impl Outbox {
    /// Bytes for `buffer` at `offset`.
    pub(crate) fn write(&mut self, buffer: &wgpu::Buffer, offset: u64, bytes: &[u8]) {
        self.writes.push((buffer.clone(), offset, bytes.to_vec()));
    }

    pub fn is_empty(&self) -> bool {
        self.writes.is_empty() && self.commands.is_empty()
    }

    /// The writes made on `queue` (they land before anything submitted after them) and the commands handed back, to be
    /// submitted next.
    pub fn write_out(self, queue: &wgpu::Queue) -> Vec<wgpu::CommandBuffer> {
        for (buffer, offset, bytes) in &self.writes {
            queue.write_buffer(buffer, *offset, bytes);
        }
        self.commands
    }

    /// The writes, then the commands submitted.
    pub fn apply(self, queue: &wgpu::Queue) {
        let commands = self.write_out(queue);
        if !commands.is_empty() {
            queue.submit(commands);
        }
    }
}

/// The per-draw GS settings the shader needs: TEX1 K (in mip levels) and the texture's last level.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct MatU {
    pub k: f32,
    pub last: f32,
    /// log2 of the layer's 1/Q per unit of view depth (the terrain program's 1/Q is 32 times the depth: 5).
    pub qlog: f32,
    /// TEST_1 AREF.
    pub aref: f32,
    /// CLAMP REGION_CLAMP's rectangle in texture coordinates (U min, max, V min, max); unbounded when off.
    pub region: [f32; 4],
    /// x: 1 for a pack replacement (sampled by its own texel density, `texnames`); y and z: a GS blend's selectors
    /// (`objects::Blend::Gs`) and FIX; w: the form of the blend's product the objects' shader makes (`objects` `blended`).
    pub extra: [f32; 4],
    /// A Rift Apart surface (`texnames::rift`, `lighting::MATERIAL`): x and y its first material's tiles per 1.0 of u and v, z the
    /// normal's strength, w 1 for one material, 2 for regions (0: none).
    pub rift: [f32; 4],
    /// A surface of regions' second and third materials' tiles (u, v each), and its fourth's (x, y; z and w 0): `texnames::RiftMat::uniform`.
    pub tiles: [f32; 4],
    pub tiles3: [f32; 4],
    /// Per material (the first in x): the grain's power, the specular's strength and the root of the reflectance at normal
    /// incidence (`texnames::RiftSlot`).
    pub grains: [f32; 4],
    pub specs: [f32; 4],
    pub refls: [f32; 4],
    /// Per material: its fur's length in the world (0: none; `objects` `fs_fur`) and its catchlight's strength (0: none;
    /// `lighting::MATERIAL` `material_catchlight`).
    pub furs: [f32; 4],
    pub catches: [f32; 4],
    /// Per material its metalness (`texnames::RiftSlot` metal; `lighting::MATERIAL` `material_metal`).
    pub metals: [f32; 4],
    /// A light (`texnames::light`): x its boost, y its glow; zeros for any other texture.
    pub emit: [f32; 4],
    /// The texture's relief (`texnames::RiftMat::relief_uniform`): x its strength, 0 for none.
    pub relief: [f32; 4],
    /// x: 1 when the level comes from the pixel's own Q (the EE's triangles, `objects` `fs_pass`: log2(1/Q) = log2(depth / st.z),
    /// as the GS reads it), 0 from the view depth and `qlog`. Only the objects' shader declares it.
    pub lodq: [f32; 4],
}

/// A GS register setting of the terrain: a texture, its clamp per axis, MMIN and K (bits).
type Key = (u32, bool, bool, u32, u32);

/// One run of strips of a chunk under one GS register setting: `first` and `count` index the scene's index buffer. `terrain` is
/// 0 for the core's terrain and 1 + N for level chunk N's; `chunk` indexes the scene's visibility over all of them.
struct Draw {
    terrain: u32,
    chunk: u32,
    key: u32,
    first: u32,
    count: u32,
}

impl Scene {
    /// The terrain's and the ties' textures with each one's world size per repeat (`surfaces`): the terrain's from its
    /// triangles, the ties' from every instance's LOD 0 triangles (the first part's coordinates) through its matrix.
    pub(crate) fn surface_layers(&self) -> Result<Vec<(&'static str, Vec<&uyafmt::core::Texture>, Vec<Option<(f64, f64)>>)>, String> {
        let mut terrain = crate::surfaces::Repeat::new(self.textures.len());
        for d in &self.draws {
            let texture = self.keys.get(d.key as usize).ok_or("surfaces: a terrain draw's key")?.0 as usize;
            for tri in self.indices[d.first as usize..(d.first + d.count) as usize].chunks_exact(3) {
                let v = [0, 1, 2].map(|k| self.vertices[tri[k] as usize]);
                terrain.add(texture, v.map(|v| v.pos), v.map(|v| v.uv))?;
            }
        }
        let (classes, textures, records) = &self.ties;
        let mut ties = crate::surfaces::Repeat::new(textures.len());
        if let Some((_, _, _, gameplay)) = &self.shrubs {
            for inst in &gameplay.ties {
                let ci = records.iter().position(|r| r.id == inst.class).ok_or_else(|| format!("surfaces: tie class {} has no record", inst.class))?;
                let (c, rec) = (&classes[ci], &records[ci].textures);
                let world = |p: [f32; 3]| [0, 1, 2].map(|k| (0..3).map(|j| p[j] / 1024.0 * c.scale * inst.axes[j][k]).sum::<f32>() + inst.position[k]);
                for pk in &c.lods[0] {
                    for tri in &pk.triangles {
                        let j = *pk.textures.get(tri.texture).ok_or("surfaces: tie triangle texture")?;
                        let texture = *rec.get(j).ok_or("surfaces: tie texture index byte")? as usize;
                        let v = tri.v.map(|i| &pk.vertices[i as usize]);
                        ties.add(texture, v.map(|v| world(v.pos)), v.map(|v| v.uv))?;
                    }
                }
            }
        }
        Ok(vec![("terrain", self.textures.iter().collect(), terrain.medians()), ("ties", textures.iter().collect(), ties.medians())])
    }
}

/// One level's scene from `uyafmt`: the terrain as triangles (from the strips of each chunk's near level) in chunk and strip
/// order, each chunk's visibility mask and zone, the GS register settings, the terrain textures with their mip levels, the sky
/// and what the object layers need.
pub struct Scene {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    draws: Vec<Draw>,
    keys: Vec<Key>,
    visibility: Vec<(u8, u8)>,
    textures: Vec<uyafmt::core::Texture>,
    sky: Option<uyafmt::coremisc::Sky>,
    /// What the shrubs need: the classes, their textures and records, and the gameplay block (instances).
    shrubs: Option<(Vec<uyafmt::shrub::ShrubClass>, Vec<uyafmt::core::Texture>, Vec<uyafmt::core::ClassRecord>, uyafmt::gameplay::Gameplay)>,
    /// What the ties need: the classes, their textures and records (the instances are in `shrubs`' gameplay block).
    ties: (Vec<uyafmt::tie::TieClass>, Vec<uyafmt::core::Texture>, Vec<uyafmt::core::ClassRecord>),
    /// What the mobies need: the core's decoded classes, their records and the moby textures.
    mobies: (Vec<Option<uyafmt::moby::MobyClass>>, Vec<uyafmt::core::ClassRecord>, Vec<uyafmt::core::Texture>, u32),
    /// The effect textures (program 6).
    effect_textures: Vec<uyafmt::core::Texture>,
    /// Core region 0x1A's textures (the EE's own triangles in the moby chain) and the region's core offset.
    quad_textures: (Vec<uyafmt::coremisc::QuadTexture>, u32),
    hud: uyafmt::hud::HudData,
    gs_ram: (Vec<u8>, Vec<uyafmt::hud::GsRamImage>),
    /// The light pass's inputs of each of `vertices` and the level's light sets (`lighting.rs`).
    lit: Vec<crate::lighting::LitVertex>,
    light_sets: crate::lighting::Sets,
}

/// TEST_1 AREF before the terrain (0x5360B): a pixel whose alpha is below it passes the alpha test only for its colour.
const AREF: u8 = 0x60;

impl Scene {
    /// Every texture of the scene by kind (`texnames::report`).
    pub fn texture_sets(&self) -> Vec<(&'static str, Vec<&uyafmt::core::Texture>)> {
        vec![
            ("terrain", self.textures.iter().collect()),
            ("sky", self.sky.iter().flat_map(|s| s.textures.iter()).collect()),
            ("shrubs", self.shrubs.iter().flat_map(|s| s.1.iter()).collect()),
            ("ties", self.ties.1.iter().collect()),
            ("mobies", self.mobies.2.iter().collect()),
            ("effects", self.effect_textures.iter().collect()),
            ("region 0x1A", self.quad_textures.0.iter().map(|q| &q.texture).collect()),
        ]
    }

    /// Level `level` from the disc image through `uyafmt` (the functions the disc walk runs).
    pub fn decode(iso: &str, level: u32) -> Result<Scene, String> {
        let t = std::time::Instant::now();
        let data = uyafmt::walk::level(iso, level as usize)?;
        let mut core = data.core.ok_or_else(|| format!("native: level {level}: no core"))?;
        let gameplay = data.gameplay.ok_or_else(|| format!("native: level {level}: no gameplay block"))?;
        let light_sets = crate::lighting::sets(&gameplay.light_sets)?;
        let mut lit = Vec::new();
        let ties = (std::mem::take(&mut core.ties), std::mem::take(&mut core.tie_textures), std::mem::take(&mut core.tie_classes));
        // The moby textures, then the special ones of the setup numbers -2 and -3 (`Core::special_textures`) and -1: the 8 x 8
        // PSMCT32 texture at GS block 0x3FFB, which holds zeros (black, alpha 0: the GS memory at Veldin field 2400), at the end.
        let mut moby_textures = std::mem::take(&mut core.moby_textures);
        let specials = moby_textures.len() as u32;
        let [s2, s3] = std::mem::replace(&mut core.special_textures, [uyafmt::core::Texture { width: 0, height: 0, levels: Vec::new(), gs: None }, uyafmt::core::Texture { width: 0, height: 0, levels: Vec::new(), gs: None }]);
        moby_textures.extend([s2, s3, uyafmt::core::Texture { width: 8, height: 8, levels: vec![vec![[0; 4]; 64]], gs: None }]);
        let mobies = (std::mem::take(&mut core.mobys), std::mem::take(&mut core.moby_classes), moby_textures, specials);
        let shrubs = Some((std::mem::take(&mut core.shrubs), std::mem::take(&mut core.shrub_textures), std::mem::take(&mut core.shrub_classes), gameplay));
        let (mut vertices, mut indices, mut draws) = (Vec::new(), Vec::new(), Vec::<Draw>::new());
        let mut keys: Vec<Key> = Vec::new();
        let mut key_of: HashMap<Key, u32> = HashMap::new();
        let mut visibility = Vec::new();
        let terrains = std::iter::once((0u32, &core.terrain)).chain(data.chunk_terrains.iter().enumerate().filter_map(|(n, t)| t.as_ref().map(|t| (1 + n as u32, t))));
        let mut sizes = Vec::new();
        for (terrain, tr) in terrains {
            let (first_index, first_chunk) = (indices.len(), visibility.len());
            for c in tr.chunks.iter() {
                let k = visibility.len();
                visibility.push(c.visibility);
                let base = vertices.len() as u32;
                for v in &c.vertices {
                    let p = v.position as usize;
                    vertices.push(Vertex { pos: c.positions[p], uv: v.uv, colour: c.colours[p] });
                    lit.push(crate::lighting::LitVertex { normal: c.normals[p], base: c.lights[p].base, sets: c.lights[p].sets as u32 });
                }
                // The near level: every vertex at its own position (no morph), the strips as the GS draws them.
                for s in &c.lods[2].strips {
                    let m = &c.materials[s.material];
                    let key = (m.texture, m.clamp[0], m.clamp[1], m.mmin, m.k.to_bits());
                    let id = *key_of.entry(key).or_insert_with(|| {
                        keys.push(key);
                        keys.len() as u32 - 1
                    });
                    if draws.last().is_none_or(|d| d.chunk != k as u32 || d.key != id) {
                        draws.push(Draw { terrain, chunk: k as u32, key: id, first: indices.len() as u32, count: 0 });
                    }
                    for i in 2..s.vertices.len() {
                        let (a, b, d) = (s.vertices[i - 2], s.vertices[i - 1], s.vertices[i]);
                        if a != b && b != d && a != d {
                            indices.extend([base + a, base + b, base + d]);
                        }
                    }
                    let d = draws.last_mut().expect("pushed above");
                    d.count = indices.len() as u32 - d.first;
                }
            }
            sizes.push(format!("{} {} chunks {} triangles", if terrain == 0 { "core".to_string() } else { format!("level chunk {}", terrain - 1) }, visibility.len() - first_chunk, (indices.len() - first_index) / 3));
        }
        let effect_textures = std::mem::take(&mut core.effect_textures);
        let quad_textures = (std::mem::take(&mut core.quad_textures), core.quad_region);
        let hud = data.hud.ok_or_else(|| format!("native: level {level}: no HUD"))?;
        let gs_ram = data.gs_ram.ok_or_else(|| format!("native: level {level}: no GS RAM image"))?;
        let s = Scene { vertices, indices, draws, keys, visibility, textures: core.terrain_textures, sky: core.sky, shrubs, ties, mobies, effect_textures, quad_textures, hud, gs_ram, lit, light_sets };
        println!(
            "native: level {level}: terrains {}, {} draws, {} GS settings, {} terrain textures, {:.1}s",
            sizes.join(", "),
            s.draws.len(),
            s.keys.len(),
            s.textures.len(),
            t.elapsed().as_secs_f64()
        );
        Ok(s)
    }
}

pub(crate) const SHADER: &str = r#"
enable dual_source_blending;
struct View { m: mat4x4<f32>, sky: mat4x4<f32>, eye: vec4<f32>, forward: vec4<f32>, fog_colour: vec4<f32>, fog: vec4<f32>, lerp: vec4<f32> };
struct Mat { k: f32, last: f32, qlog: f32, aref: f32, region: vec4<f32>, extra: vec4<f32>, rift: vec4<f32>, tiles: vec4<f32>, tiles3: vec4<f32>, grains: vec4<f32>, specs: vec4<f32>, refls: vec4<f32>, furs: vec4<f32>, catches: vec4<f32>, metals: vec4<f32>, emit: vec4<f32>, relief: vec4<f32> };
@group(0) @binding(0) var<uniform> view: View;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var smp: sampler;
@group(1) @binding(2) var<uniform> mat: Mat;
// A Rift Apart surface's material map and its sampler (`lighting::MATERIAL`), read when `mat.rift.w` is 1 or more; a surface of
// regions' (`mat.rift.w` 2) region map and other materials' maps.
@group(1) @binding(3) var mtl: texture_2d<f32>;
@group(1) @binding(4) var msmp: sampler;
@group(1) @binding(5) var rgn: texture_2d<f32>;
@group(1) @binding(6) var mtl1: texture_2d<f32>;
@group(1) @binding(7) var mtl2: texture_2d<f32>;
@group(1) @binding(8) var mtl3: texture_2d<f32>;
// The texture's relief map (`texnames` `relief.tsv`, `lighting::MATERIAL` `material_relief`), read when `mat.relief.x` is above 0.
@group(1) @binding(9) var rlf: texture_2d<f32>;

struct VIn { @location(0) pos: vec3<f32>, @location(1) uv: vec2<f32>, @location(2) colour: vec4<f32> };
struct VOut {
    // Keep the surface pass's depth identical to the colour pass's after shader compilation.
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // The GS interpolates Gouraud colours linearly across the screen.
    @location(1) @interpolate(linear) colour: vec4<f32>,
    @location(2) depth: f32,
    // The GS interpolates F across the primitive in screen space.
    @location(3) @interpolate(linear) f: f32,
    @location(4) wpos: vec3<f32>,
};

@vertex fn vs(v: VIn) -> VOut {
    var o: VOut;
    o.clip = view.m * vec4<f32>(v.pos, 1.0);
    o.uv = v.uv;
    o.wpos = v.pos;
    o.colour = v.colour;
    o.depth = dot(v.pos - view.eye.xyz, view.forward.xyz);
    // The terrain program's F: linear in the view depth from fog.z at fog.x to fog.w at fog.y, held beyond, truncated (FTOI4,
    // the integer part in XYZF2's F).
    let f = view.fog.z + (view.fog.w - view.fog.z) * (o.depth - view.fog.x) / (view.fog.y - view.fog.x);
    o.f = floor(clamp(f, min(view.fog.z, view.fog.w), max(view.fog.z, view.fog.w)));
    return o;
}

// The GS pixel: its colour after texture and fog (bytes), and its alpha As as a byte.
fn gs(i: VOut) -> vec4<f32> {
    // TEX1 LCM 0: LOD = log2(1/Q) + K. The terrain program (VU1 program 1) outputs Q = vf01.x / w, and 1/Q is 32 times the view
    // depth in world units (Veldin field 2400: vf01.x = -0.0239062 at VU 0x290, the matrix's w row -0.7652 per world unit).
    var t: vec4<f32>;
    if (mat.extra.x > 0.5) {
        // A pack replacement (`texnames`): the level its own texel density on screen picks.
        t = round(textureSampleGrad(tex, smp, i.uv, dpdx(i.uv) * view.lerp.z, dpdy(i.uv) * view.lerp.z) * 255.0);
    } else {
        let lod = min(log2(i.depth) + mat.qlog + mat.k, mat.last);
        // The filtered texel truncated, as the GS's filter leaves it.
        t = floor(textureSampleLevel(tex, smp, i.uv, lod) * 255.0 + 0.001);
    }
    // A Rift Apart surface: the texel times its material's grain (`lighting::MATERIAL`), its alpha the texture's.
    let g = material_grain(material_at(i.uv));
    if (mat.rift.w > 0.5) {
        t = vec4<f32>(min(round(t.rgb * g), vec3<f32>(255.0)), t.a);
    }
    let v = round(i.colour * 255.0);
    // TFX MODULATE, TCC RGBA: texel x vertex colour >> 7, clamped to 0xFF; a light (`mat.emit`) at least its own colour times
    // its boost. Its glow is applied only to the frame buffer's alpha, after alpha testing and colour blending.
    let lit_c = min(floor(t * v / 128.0), vec4<f32>(255.0));
    let lamp = mat.emit.x > 0.0;
    let c = select(lit_c, vec4<f32>(max(lit_c.rgb, min(floor(t.rgb * mat.emit.x), vec3<f32>(255.0))), lit_c.a), lamp);
    // FGE: (F x colour + (255 - F) x FOGCOL) >> 8.
    let rgb = floor((c.rgb * i.f + view.fog_colour.rgb * (255.0 - i.f)) / 256.0);
    return vec4<f32>(rgb, c.a);
}

// The pixel as the GS writes it: its colour and its alpha As exactly (a byte, as the frame buffer keeps it: the glow reads
// it back), and for the blend unit As / 0x80 as the second source's alpha (`Src1Alpha`; up to 1).
struct FOut { @location(0) @blend_src(0) c: vec4<f32>, @location(0) @blend_src(1) f: vec4<f32> };
// ABE with ALPHA_1 0x44: (Cs - Cd) x As >> 7 + Cd.
fn blended(p: vec4<f32>) -> FOut {
    var o: FOut;
    o.c = vec4<f32>(p.rgb / 255.0, material_glow_alpha(p.a) / 255.0);
    o.f = vec4<f32>(0.0, 0.0, 0.0, min(p.a / 128.0, 1.0));
    return o;
}

// TEST_1: pixels with As >= AREF write colour and depth (this pipeline), the others colour only (fs_fail's).
@fragment fn fs_pass(i: VOut) -> FOut {
    let p = gs(i);
    if (p.a < mat.aref) { discard; }
    return blended(p);
}

@fragment fn fs_fail(i: VOut) -> FOut {
    let p = gs(i);
    if (p.a >= mat.aref) { discard; }
    return blended(p);
}
"#;

pub struct Native {
    /// The frame being drawn has no view set (`camera_at_rest`: a level's first frames, the game's camera, rotation and clip
    /// matrices still zero): the world's programs run but every vertex they project collapses to a point on the PS2, so no
    /// world is drawn, only the EE's screen primitives.
    no_view: bool,
    /// GS memory across fields (`take_uploads`).
    gsm: crate::gsmem::GsMemory,
    /// The transfers taken since GS memory was last made ready for a frame, and the set written then (`settle_gsm`).
    gsm_pending: Vec<uyart::hw::GsUpload>,
    gsm_settled: Vec<uyart::hw::GsUpload>,
    /// Why the level the game is in could not be loaded (a pack file that cannot be read or decoded, `texnames`), which `play`
    /// stops the game with: else every frame of the level is drawn without its world.
    broken: Option<String>,
    /// The GPU, kept for a window that presents the pictures from it (`play.rs`).
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    /// No summary line per picture (`play`).
    pub quiet: bool,
    /// The device whose button prompts the pictures show and the icons (`prompts.rs`; None: the game's own glyphs).
    pub prompts: Option<crate::prompts::Look>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    /// The pixels that pass the alpha test (colour and depth) and those that fail it (colour only).
    pass: wgpu::RenderPipeline,
    fail: wgpu::RenderPipeline,
    /// Shared with the ties' and the shrubs' threads, which record their passes (`Native::render`).
    objects: std::sync::Arc<crate::objects::Pipelines>,
    /// The shadow steps' pipelines (`eedraw.rs`), shared with the frames' replays.
    shadow_pipes: std::sync::Arc<crate::eedraw::Pipelines>,
    view_layout: wgpu::BindGroupLayout,
    mat_layout: wgpu::BindGroupLayout,
    size: (u32, u32),
    /// The last frame's widening (`Camera::k`), the shape `resize` keeps.
    k: f32,
    iso: String,
    /// Every story level's globals.
    globals: HashMap<u32, Globals>,
    /// VU1 program 6 (the effects), from the main program (`uyafmt::prog6::microcode`).
    fx_micro: Vec<u8>,
    /// The main program from the disc (the VU1 microcode packets the moby chain uploads).
    boot: std::sync::Arc<uyafmt::boot::Boot>,
    /// The loaded level and its GPU scene.
    level: Option<(u32, Gpu)>,
    /// The level's GPU state is made again at the next level frame (`reload`), what it shows brought over.
    reload_level: bool,
    /// The frames' walks (`Walker`), made here unless `play`'s walk thread has taken it (`take_walker`).
    walker: Option<Walker>,
    /// The ambient occlusion option's passes (`ao.rs`; shared with the frame's replays).
    ao: std::sync::Arc<crate::ao::Ao>,
    /// The sky's pipelines (a level's sky makes none: `pipelines.rs`).
    sky_pipes: crate::sky::SkyPipelines,
    /// The water reflections setting's passes (`reflections.rs`), shared with the frames' replays.
    reflections: std::sync::Arc<crate::reflections::Reflections>,
    /// The light shafts option's passes (`shafts.rs`), shared with the frames' replays.
    shafts: std::sync::Arc<crate::shafts::Shafts>,
    /// The shadows option's pipelines (`shadows.rs`), shared with the frames' replays.
    light_shadows: std::sync::Arc<crate::shadows::Shadows>,
    /// The ray-traced shadows' pipelines (`raytrace.rs`), when the GPU traces rays.
    rt: Option<std::sync::Arc<crate::raytrace::RayTracing>>,
    /// The lighting option's terrain pipelines (`lighting.rs`).
    lighting: std::sync::Arc<crate::lighting::Lighting>,
    /// The last frame's EE primitives (`mobies::walk_into`) and moby vertices, emptied: the next frame's take their room (the sea's
    /// 20,000 primitives and the mobies' vertices would otherwise grow new memory every frame).
    ee_spare: Vec<(usize, crate::mobies::EePrim)>,
    moby_spare: Vec<crate::objects::ObjVertex>,
    moby_prev_spare: Vec<[f32; 3]>,
    moby_lit_spare: Vec<crate::lighting::MobyLit>,
    /// The effect layer's CPU vertices and draw list, reused across combat and pickup bursts.
    fx_spare: Vec<crate::objects::ObjVertex>,
    fx_prev_spare: Vec<[f32; 3]>,
    fx_lit_spare: Vec<crate::lighting::MobyLit>,
    fx_draw_spare: Vec<crate::objects::Draw>,
    /// The mobies' lighting rows, draws and runs of draws, and the frame's recorded commands, kept from the frame before.
    light_spare: Vec<[[f32; 4]; 4]>,
    moby_draw_spare: Vec<((u32, u16), crate::objects::Draw, u32)>,
    run_spare: Vec<crate::objects::Draw>,
    rec_spare: Vec<crate::replay::Cmd>,
    /// How many commands the last frames recorded (the room the next frame's takes).
    rec_cap: usize,
    /// The shrubs' draws and their groups, kept from the frame before (`ShrubPart::room`).
    shrub_spare: (Vec<(usize, crate::objects::Draw)>, Vec<usize>),
    /// The threads the frame's terrain, ties and shrubs passes run on beside its own (`Pool`).
    pool: std::sync::Arc<Pool>,
    /// The frame sets (`SETS`): the per-frame buffers, and the ties' ranges' rooms (their threads take them and give them back:
    /// `TiePart`).
    sets: Vec<FrameSet>,
    /// And the EE steps' vertices (`eedraw::build`).
    ee_vert_spare: Vec<crate::objects::ObjVertex>,
    /// And the water triangles' (`eedraw::EeDraw::water`: the sea's are two megabytes, page faults when made anew each frame).
    water_spare: Vec<crate::objects::ObjVertex>,
    /// Whether each frame that draws the world leaves what drawing it again needs (`replay::Replay`, taken by `take_replay`): set by
    /// `play` when it presents above the game's rate.
    pub replays: bool,
    /// Prepare world snapshots for the presenter instead of rendering an additional game-frame picture. The presenter is then
    /// the one thread that submits during play: a frame's shared changes and a frame drawn as it is prepared are left in
    /// `outbox` for it (`take_outbox`).
    pub defer_world: bool,
    outbox: Outbox,
    raster: Option<((u32, u32), std::sync::Arc<crate::surface_buffer::Buffers>)>,
    /// Whether the last world frame was drawn without DLSS though it is on (`render_frame`'s `under_model`): the next frame
    /// with it begins its history again.
    dlss_skipped: bool,
    replay: Option<crate::replay::Replay>,
    /// The last frame's set while its picture is to be shown (`take_set`).
    set_held: Option<std::sync::Arc<()>>,
    /// The depth of every frame drawn (`picture`).
    depth: Option<(wgpu::Texture, wgpu::TextureView)>,
    /// The last replay's flag that it is not to be drawn again (`replay::Replay::stale`).
    last_stale: Option<std::sync::Arc<std::sync::Mutex<bool>>>,
    /// The field, level and camera of the last frame drawn with the world on the native picture (the camera a picture between it
    /// and the next starts from).
    last_world: Option<(u64, u32, Camera)>,
    /// DLSS (`dlss.rs`): whether the driver offers it, NGX once the option is first on, the device's DLSS passes, and the upscaler
    /// at the option's mode and the picture's size (None with the option off: the world drawn at the picture's own size).
    dlss_offered: bool,
    dlss_sdk: Option<std::sync::Arc<std::sync::Mutex<dlss_wgpu::DlssSdk>>>,
    dlss_pipes: std::sync::Arc<crate::dlss::Pipes>,
    upscaler: Option<std::sync::Arc<crate::dlss::Upscaler>>,
    /// The frames with no level's world (`render_screen`): their EE textures (made at the picture's size).
    screen_tex: Option<crate::eedraw::EeTextures>,
    /// What the frames could not draw, named once (the play window's renderer is quiet).
    said: std::collections::HashSet<String>,
    /// UYA_GPU_PASSES in uya native: the passes' times of each redraw (gpu_profile), printed as the redraw ends.
    gpu_passes: Option<crate::gpu_profile::Profiler>,
}

/// A scene on the GPU: its buffers, per GS register setting its bind group and whether its texture has texels below AREF, and
/// the scene's draws and chunk visibility.
struct Gpu {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// Shared with the terrain's thread (`Native::render`), as are the buffers and the sky.
    keys: std::sync::Arc<Vec<(wgpu::BindGroup, bool)>>,
    draws: std::sync::Arc<Vec<Draw>>,
    visibility: std::sync::Arc<Vec<(u8, u8)>>,
    /// The sky the game's pointer names (`Native::render`): its address and header, from which it was made.
    sky: Option<(u32, [u8; 0x40], std::sync::Arc<crate::sky::SkyGpu>)>,
    shrubs: Option<std::sync::Arc<crate::objects::Shrubs>>,
    ties: Option<std::sync::Arc<crate::objects::Ties>>,
    mobies: Option<crate::mobies::Mobies>,
    effects: crate::effects::Effects,
    ee_textures: crate::eedraw::EeTextures,
    /// The level's sun (`shafts::Sun`: its gameplay block's first light), when it has one.
    sun: Option<crate::shafts::Sun>,
    /// The terrain's light pass inputs and light sets (`lighting.rs`).
    lit: std::sync::Arc<crate::lighting::TerrainLit>,
    /// The level's key light, its ties' casters and its shadow maps (`shadows.rs`).
    shadows: std::sync::Mutex<crate::shadows::LevelShadows>,
    /// The ray-traced shadows' structures (`raytrace.rs`), and per terrain setting (`keys`) the texture its bind group samples
    /// and its clamps in S and T, which their alpha test reads.
    rt: std::sync::Mutex<crate::raytrace::LevelRt>,
    rt_views: Vec<(wgpu::TextureView, bool, bool, Option<crate::texnames::RiftMat>)>,
}

impl Native {
    /// The renderer for pictures `height` pixels high, as wide as the PS2 view's aspect (horizontal over vertical tangent, the
    /// 16:9 option off: 0.62 / 0.4805).
    pub fn open(iso: &str, height: u32) -> Result<Native, String> {
        let width = width(height, 1.0);
        // Vulkan with DLSS's instance and device extensions (`dlss.rs`), where the driver offers it.
        let (instance, mut dlss_support) = crate::dlss::instance()?;
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
            apply_limit_buckets: false,
        }))
        .map_err(|e| format!("native: no GPU adapter: {e}"))?;
        // DLSS Frame Generation on this GPU (`streamline.rs`)?
        crate::streamline::adapter(&adapter);
        // The largest buffer the GPU makes: a level's tie corners (`objects::Ties`) pass wgpu's default 256 MB on the larger levels.
        // GPU memory in blocks near what is used (`MemoryUsage`): this PC's driver charges each block to the process's commit, and
        // the default 256 MB blocks took level 28 past 2 GB with the pictures drawn between frames held.
        // The pipeline cache (`pipelines.rs`), where `uya play` keeps one.
        let cache = if crate::pipelines::wanted() { wgpu::Features::PIPELINE_CACHE } else { wgpu::Features::empty() };
        // The ray-traced shadows (`raytrace.rs`) where the GPU traces rays: Vulkan's ray queries, which wgpu 30 offers as an
        // experimental feature.
        let rays = crate::raytrace::supported(&adapter);
        // The GPU's own buffer and texture sizes, not wgpu's defaults (8192 texels a side): a texture as large as the GPU takes is
        // made (the player, 17 November 2026, on the 2D layer's 16384-row texture that ended the game: "not cap it at 8192 make it
        // dynamic").
        let most = adapter.limits();
        let limits = wgpu::Limits { max_buffer_size: most.max_buffer_size, max_texture_dimension_1d: most.max_texture_dimension_1d, max_texture_dimension_2d: most.max_texture_dimension_2d, max_texture_dimension_3d: most.max_texture_dimension_3d, max_texture_array_layers: most.max_texture_array_layers, ..Default::default() };
        println!("native: textures up to {} texels a side", limits.max_texture_dimension_2d);
        let (rt_features, limits, experimental) = match rays {
            // SAFETY: the experimental features asked for are the ray queries and acceleration structures `raytrace.rs` uses.
            true => (crate::raytrace::features(), crate::raytrace::limits(&adapter, limits), unsafe { wgpu::ExperimentalFeatures::enabled() }),
            false => (wgpu::Features::empty(), limits, wgpu::ExperimentalFeatures::disabled()),
        };
        // UYA_RT_TIMES: the frame's own GPU times (`raytrace::GpuTimes`).
        let timing = (crate::raytrace::times_wanted() || crate::gpu_profile::wanted()) && adapter.features().contains(crate::raytrace::times_features());
        let live_timing = crate::gpu_profile::wanted();
        if live_timing && !adapter.features().contains(crate::gpu_profile::features()) {
            return Err("UYA_GPU_PASSES requires timestamp queries inside passes".into());
        }
        let rt_features = rt_features | if live_timing { crate::gpu_profile::features() } else if timing { crate::raytrace::times_features() } else { wgpu::Features::empty() };
        // Memory in blocks of 128 MB (host-visible: 64 MB): with the small blocks of `MemoryHints::MemoryUsage` (4 to 32 MB host), the
        // frame's `Queue::write_buffer` staging (2 to 4 MB each, some 10 MB a frame on level 9's ground) made and mapped a new block
        // every frame and freed it when it emptied: 13 % of the render thread there (vkAllocateMemory and vkMapMemory's kernel lock).
        let (device, queue) = crate::dlss::device(&adapter, &wgpu::DeviceDescriptor { label: Some("uya native"), required_features: wgpu::Features::DEPTH32FLOAT_STENCIL8 | wgpu::Features::DUAL_SOURCE_BLENDING | cache | rt_features, required_limits: limits, experimental_features: experimental, memory_hints: wgpu::MemoryHints::Manual { suballocated_device_memory_block_size: (128 << 20)..(128 << 20) }, ..Default::default() }, &mut dlss_support)?;
        crate::streamline::device();
        println!("native: DLSS {}", if dlss_support.super_resolution_supported { "offered by the driver" } else { "not offered on this GPU" });
        let dlss_pipes = std::sync::Arc::new(crate::dlss::Pipes::new(&device));
        // Every level's globals and the main program from the disc.
        let globals = all_globals(iso)?;
        let boot = std::sync::Arc::new(uyafmt::boot::boot(iso)?);
        let walker = Walker { boot: boot.clone(), gs_carry: None, screen_vu: crate::mobies::ScreenVu::new(), level_vu: crate::mobies::LevelVu::kept(), glyphs: crate::prompts::Glyphs::new(&boot)? };
        let fx_micro = uyafmt::prog6::microcode(&boot)?.to_vec();
        crate::pipelines::make(&device, &adapter)?;
        // The shared pipelines, made now (`pipelines.rs`). Vegetation's binding-array layouts depend on the level's materials
        // and are made with that level, never in response to a shot, pickup or a new material state during play.
        let made = std::time::Instant::now();
        // The terrain's shader with the Rift Apart materials' functions (`lighting::MATERIAL`, its `gs`).
        let terrain = format!("{SHADER}{}{}{}", crate::lighting::MATERIAL, crate::surface_buffer::COMMON, crate::surface_buffer::TERRAIN);
        let source = if crate::surfaces::on() { crate::surfaces::shader(&terrain, "t = round(textureSampleGrad(tex, smp, i.uv, dpdx(i.uv) * view.lerp.z, dpdy(i.uv) * view.lerp.z) * 255.0);", "i.uv")? } else { terrain.clone() };
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("terrain"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let uniform = |binding: u32, visibility: wgpu::ShaderStages| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let view_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view"),
            entries: &[uniform(0, wgpu::ShaderStages::VERTEX_FRAGMENT), wgpu::BindGroupLayoutEntry {
                binding: 1, visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Storage { read_only: true }, has_dynamic_offset: false, min_binding_size: None },
                count: None,
            }],
        });
        let mat_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                uniform(2, wgpu::ShaderStages::VERTEX_FRAGMENT),
                // A Rift Apart surface's material map (`texnames::flat_material` for any other) and its sampler (repeating).
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 4, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None },
                // A surface of regions' region map and its other three materials' maps (`texnames::RiftMat::entries`).
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                // The texture's relief map (`texnames::RiftMat::entries`).
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: true }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("terrain"), bind_group_layouts: &[Some(&view_layout), Some(&mat_layout)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Unorm8x4];
        // ALPHA_1 0x44: source x As + destination x (1 - As); the target's alpha is not read.
        let blend = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Src1Alpha, dst_factor: wgpu::BlendFactor::OneMinusSrc1Alpha, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent::REPLACE,
        };
        let pipeline = |entry: &str, depth_write: bool| {
            crate::surface_buffer::pipeline(&device, &wgpu::RenderPipelineDescriptor {
                label: Some(entry),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<Vertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })],
                },
                // The GS draws both faces.
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                // ZTST GEQUAL on the GS's Z (larger is nearer): nearer or equal passes.
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH,
                    depth_write_enabled: Some(depth_write),
                    depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(entry),
                    compilation_options: Default::default(),
                    // TEST_1 0x5360B's AFAIL RGB_ONLY: a failing pixel keeps the frame's alpha.
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(blend), write_mask: if depth_write { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::COLOR } })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            }, depth_write.then_some("fs_surface_terrain"))
        };
        let (pass, fail) = (pipeline("fs_pass", true), pipeline("fs_fail", false));
        let objects = std::sync::Arc::new(crate::objects::Pipelines::new(&device, &view_layout, &mat_layout));
        let shadow_pipes = std::sync::Arc::new(crate::eedraw::Pipelines::new(&device, &view_layout));
        println!("native: {} ({:?}), pictures {width}x{height}, globals of {} levels", adapter.get_info().name, adapter.get_info().backend, globals.len());
        let ao = std::sync::Arc::new(crate::ao::Ao::new(&device));
        let reflections = std::sync::Arc::new(crate::reflections::Reflections::new(&device, &queue, &view_layout));
        let sky_pipes = crate::sky::SkyPipelines::new(&device, &view_layout);
        let shafts = std::sync::Arc::new(crate::shafts::Shafts::new(&device));
        let light_shadows = std::sync::Arc::new(crate::shadows::Shadows::new(&device, &view_layout, &mat_layout));
        let rt = rays.then(|| std::sync::Arc::new(crate::raytrace::RayTracing::new(&device, (timing && crate::raytrace::times_wanted()).then(|| crate::raytrace::GpuTimes::new(&device, &queue)))));
        println!("native: ray tracing: {}", if rays { "hardware ray queries (EXPERIMENTAL_RAY_QUERY)" } else { "none on this GPU" });
        println!("native: pipelines made in {:.0} ms", made.elapsed().as_secs_f64() * 1000.0);
        let lighting = std::sync::Arc::new(crate::lighting::Lighting::new(&device, &terrain, &view_layout, &mat_layout, blend, DEPTH));
        Ok(Native { no_view: false, gsm: crate::gsmem::GsMemory::new(), gsm_pending: Vec::new(), gsm_settled: Vec::new(), broken: None, instance, adapter, quiet: false, prompts: None, device, queue, pass, fail, objects, shadow_pipes, view_layout, mat_layout, size: (width, height), k: 1.0, iso: iso.to_string(), globals, pool: std::sync::Arc::new(Pool::new(POOL_THREADS)?), fx_micro, boot, level: None, reload_level: false, walker: Some(walker), ao, lighting, sky_pipes, reflections, shafts, light_shadows, rt, ee_spare: Vec::new(), moby_spare: Vec::new(), moby_prev_spare: Vec::new(), moby_lit_spare: Vec::new(), fx_spare: Vec::new(), fx_prev_spare: Vec::new(), fx_lit_spare: Vec::new(), fx_draw_spare: Vec::new(), light_spare: Vec::new(), moby_draw_spare: Vec::new(), run_spare: Vec::new(), rec_spare: Vec::new(), rec_cap: 0, shrub_spare: (Vec::new(), Vec::new()), sets: (0..SETS).map(|_| FrameSet::default()).collect(), ee_vert_spare: Vec::new(), water_spare: Vec::new(), replays: false, defer_world: false, outbox: Outbox::default(), raster: None, dlss_skipped: false, replay: None, set_held: None, depth: None, last_stale: None, last_world: None, dlss_offered: dlss_support.super_resolution_supported, dlss_sdk: None, dlss_pipes, upscaler: None, screen_tex: None, said: Default::default(), gpu_passes: crate::gpu_profile::Profiler::new() })
    }

    /// Level `level`'s scene on the GPU: its vertices and indices, each terrain texture with its mip levels, and per GS register
    /// setting the texture, a sampler with the record's clamp and filters, and its K and last level.
    fn load(&mut self, ram: &[u8], level: u32) -> Result<(), String> {
        use wgpu::util::DeviceExt;
        let t0 = std::time::Instant::now();
        // Release the old scene before decoding and uploading the next. Assigning Some(new) only at the end kept both levels'
        // geometry, textures and acceleration structures alive throughout loading. Submitted work retains its own handles.
        self.level = None;
        if let Some(rt) = &self.rt {
            rt.passes.reset_history();
        }
        let s = Scene::decode(&self.iso, level)?;
        // The pick (UYA_PICK_PIXEL, a debugging aid) reads the camera, which a level's first frames have not set yet: only when
        // it is asked for.
        // A level's first frame before its camera is set: its terrain is not picked, said so.
        let pick_cam = match crate::surfaces::Pick::wanted().then(|| self.camera(ram, level)) {
            Some(Err(e)) => {
                println!("pick: level {level}'s terrain not picked at its load: {e}");
                None
            }
            c => c.transpose()?,
        };
        if let Some(pick) = pick_cam.and_then(|c| crate::surfaces::Pick::new(&c)) {
            for dr in &s.draws {
                for tri in s.indices[dr.first as usize..(dr.first + dr.count) as usize].chunks_exact(3) {
                    if let Some(distance) = pick.hit([s.vertices[tri[0] as usize].pos, s.vertices[tri[1] as usize].pos, s.vertices[tri[2] as usize].pos]) {
                        let key = s.keys[dr.key as usize];
                        let name = crate::texnames::name(&s.textures[key.0 as usize])?.map(|n| n.stem());
                        println!("pick: terrain {} chunk {} distance {distance:.4} texture {name:?} key {key:?}", dr.terrain, dr.chunk);
                    }
                }
            }
        }
        // The level's pack replacements decoded on every core and uploaded as each is decoded (`texnames::preload`), before the
        // layers below take them; with them Clank's and the ship's skins for the numbers the RAM holds, which the level's first
        // frame makes (`mobies::Mobies::skin`: Clank's replacement, decoded there, took that frame 44 to 52 ms longer).
        let gm = self.globals(level)?.mobies;
        let mut skins = Vec::new();
        for t in [gm.clank, gm.ship] {
            let n = ram.get(t.skin as usize..t.skin as usize + 4).map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]])).ok_or("native: a skin number past RAM")?;
            if n < 16 {
                skins.push(uyafmt::moby::wad_image(&self.iso, t.wad, (t.pair + n) as usize)?);
            }
        }
        if crate::surfaces::on() {
            crate::surfaces::assign(&s.surface_layers()?)?;
        }
        let textures: Vec<&uyafmt::core::Texture> = s.texture_sets().into_iter().flat_map(|(_, v)| v).chain(&skins).collect();
        crate::texnames::preload(&self.device, &self.queue, &textures)?;
        let d = &self.device;
        // Where the GPU traces rays, the terrain's buffers are also the ray-traced shadows' (`raytrace.rs`): a bottom level's input
        // and, for the alpha test's corners, read by a compute pass.
        let rays = if self.rt.is_some() { wgpu::BufferUsages::BLAS_INPUT | wgpu::BufferUsages::STORAGE } else { wgpu::BufferUsages::empty() };
        let vertices = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("terrain vertices"), contents: bytemuck::cast_slice(&s.vertices), usage: wgpu::BufferUsages::VERTEX | rays });
        let lit = std::sync::Arc::new(self.lighting.terrain(d, &s.lit, &s.light_sets));
        let key = crate::shadows::key_light(&s.lit, &s.light_sets);
        let shadows = std::sync::Mutex::new(crate::shadows::LevelShadows::new(key));
        let rt = std::sync::Mutex::new(crate::raytrace::LevelRt::new(key));
        let indices = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("terrain indices"), contents: bytemuck::cast_slice(&s.indices), usage: wgpu::BufferUsages::INDEX | rays });
        let mut views = Vec::with_capacity(s.textures.len());
        for t in &s.textures {
            let tex = d.create_texture(&wgpu::TextureDescriptor {
                label: Some("terrain texture"),
                size: wgpu::Extent3d { width: t.width as u32, height: t.height as u32, depth_or_array_layers: 1 },
                mip_level_count: t.levels.len() as u32,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            for (l, px) in t.levels.iter().enumerate() {
                let (w, h) = ((t.width >> l) as u32, (t.height >> l) as u32);
                self.queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: l as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    bytemuck::cast_slice(px),
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            }
            views.push(tex.create_view(&Default::default()));
        }
        // Each texture's pack replacement on the GPU (`texnames`): its view, level count and whether a texel is below AREF.
        let hd: Vec<Option<(wgpu::TextureView, usize, bool)>> = s
            .textures
            .iter()
            .map(|t| -> Result<Option<(wgpu::TextureView, usize, bool)>, String> { Ok(crate::texnames::hd(d, &self.queue, t)?.map(|h| (h.view, h.levels, h.min_alpha < AREF))) })
            .collect::<Result<_, String>>()?;
        // Each texture's Rift Apart material (`texnames::rift`), the map of those without one and the materials' sampler.
        let rift: Vec<Option<crate::texnames::RiftMat>> = s.textures.iter().map(|t| crate::texnames::rift(d, &self.queue, t)).collect::<Result<_, String>>()?;
        let lights: Vec<[f32; 2]> = s.textures.iter().map(crate::texnames::light).collect::<Result<_, String>>()?;
        // The water's ripples (`reflections`): Rift Apart's water normal while UPDATED TEXTURES is on.
        self.reflections.set_ripple_normal(crate::texnames::water_normal(d, &self.queue)?);
        let flat = crate::texnames::flat_material(d, &self.queue)?;
        let rift_sampler = crate::texnames::hd_sampler(d, false, false, false, 16);
        let mut samplers: HashMap<(bool, bool, u32), wgpu::Sampler> = HashMap::new();
        let mut keys = Vec::with_capacity(s.keys.len());
        let mut rt_views = Vec::with_capacity(s.keys.len());
        for &(texture, cs, ct, mmin, k) in &s.keys {
            let t = s.textures.get(texture as usize).ok_or_else(|| format!("native: terrain texture {texture} of {}", s.textures.len()))?;
            // TEX1 MMIN: the filter inside a level and between levels; 0 and 1 use level 0 only. MMAG is 1 (linear), LCM 0 and L 0
            // in every terrain record's TEX1 as func_L01_00424DE8 builds it (Veldin's RAM at field 2400: 0x20 | MMIN << 6 | MXL << 2,
            // K << 32).
            let (min, mip, levels) = match mmin {
                0 => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest, 1),
                1 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Nearest, 1),
                2 => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Nearest, t.levels.len()),
                3 => (wgpu::FilterMode::Nearest, wgpu::MipmapFilterMode::Linear, t.levels.len()),
                4 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Nearest, t.levels.len()),
                5 => (wgpu::FilterMode::Linear, wgpu::MipmapFilterMode::Linear, t.levels.len()),
                m => return Err(format!("native: TEX1 MMIN {m}")),
            };
            // The GS's sampler for the setting (the original's), kept by key for the draws that sample the original.
            samplers.entry((cs, ct, mmin)).or_insert_with(|| {
                let mode = |c: bool| if c { wgpu::AddressMode::ClampToEdge } else { wgpu::AddressMode::Repeat };
                d.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("terrain"),
                    address_mode_u: mode(cs),
                    address_mode_v: mode(ct),
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: min,
                    mipmap_filter: mip,
                    ..Default::default()
                })
            });
            // The vertex alpha is 0x80, so As is the texel's alpha: a texture with a texel below AREF has pixels that fail.
            let (u, view, sampler, fails) = match (&hd[texture as usize], crate::enhance::aniso()) {
                // A pack replacement: its own levels, trilinear and anisotropic (the terrain has no clamp region).
                (Some((v, n, fails)), _) => {
                    let sampler = samplers.entry((cs, ct, crate::objects::HD_MMIN)).or_insert_with(|| crate::texnames::hd_sampler(d, cs, ct, false, 16));
                    (MatU { k: 0.0, last: (n - 1) as f32, qlog: 5.0, aref: AREF as f32, region: crate::objects::NO_REGION, extra: [1.0, 0.0, 0.0, 0.0], rift: [0.0; 4], tiles: [0.0; 4], tiles3: [0.0; 4], grains: [0.0; 4], specs: [0.0; 4], refls: [0.0; 4], furs: [0.0; 4], catches: [0.0; 4], metals: [0.0; 4], emit: [0.0; 4], relief: [0.0; 4], lodq: [0.0; 4] }, v, &*sampler, *fails)
                }
                // The texture filtering option (`enhance`): the original's own levels by their density on screen (level 0 alone for
                // MMIN 0 and 1), at the option's anisotropy.
                (None, Some(aniso)) => {
                    let m = if levels == 1 { crate::objects::ANISO_MMIN0 } else { crate::objects::ANISO_MMIN };
                    let sampler = samplers.entry((cs, ct, m)).or_insert_with(|| crate::texnames::hd_sampler(d, cs, ct, levels == 1, aniso));
                    (MatU { k: 0.0, last: (levels - 1) as f32, qlog: 5.0, aref: AREF as f32, region: crate::objects::NO_REGION, extra: [1.0, 0.0, 0.0, 0.0], rift: [0.0; 4], tiles: [0.0; 4], tiles3: [0.0; 4], grains: [0.0; 4], specs: [0.0; 4], refls: [0.0; 4], furs: [0.0; 4], catches: [0.0; 4], metals: [0.0; 4], emit: [0.0; 4], relief: [0.0; 4], lodq: [0.0; 4] }, &views[texture as usize], &*sampler, t.levels[..levels].iter().flatten().any(|p| p[3] < AREF))
                }
                (None, None) => {
                    let u = MatU { k: f32::from_bits(k), last: (levels - 1) as f32, qlog: 5.0, aref: AREF as f32, region: crate::objects::NO_REGION, extra: [0.0; 4], rift: [0.0; 4], tiles: [0.0; 4], tiles3: [0.0; 4], grains: [0.0; 4], specs: [0.0; 4], refls: [0.0; 4], furs: [0.0; 4], catches: [0.0; 4], metals: [0.0; 4], emit: [0.0; 4], relief: [0.0; 4], lodq: [0.0; 4] };
                    (u, &views[texture as usize], &samplers[&(cs, ct, mmin)], t.levels[..levels].iter().flatten().any(|p| p[3] < AREF))
                }
            };
            let r = rift[texture as usize].as_ref();
            let [rift_u, tiles, tiles3, grains, specs, refls, furs, catches, metals] = crate::texnames::RiftMat::uniform(r);
            let e = lights[texture as usize];
            let u = MatU { rift: rift_u, tiles, tiles3, grains, specs, refls, furs, catches, metals, emit: [e[0], e[1], 0.0, 0.0], relief: crate::texnames::RiftMat::relief_uniform(r), ..u };
            let ub = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("material"), contents: bytemuck::bytes_of(&u), usage: wgpu::BufferUsages::UNIFORM });
            let [e3, e4, e5, e6, e7, e8, e9] = crate::texnames::RiftMat::entries(r, &flat, &rift_sampler);
            let bg = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("material"),
                layout: &self.mat_layout,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(view) },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(sampler) },
                    wgpu::BindGroupEntry { binding: 2, resource: ub.as_entire_binding() },
                    e3,
                    e4,
                    e5,
                    e6,
                    e7,
                    e8,
                    e9,
                ],
            });
            keys.push((bg, fails));
            rt_views.push((view.clone(), cs, ct, r.cloned()));
        }
        let (mut shrubs, mut ties) = (None, None);
        let mut sun = None;
        if let Some((classes, textures, records, gameplay)) = s.shrubs {
            sun = crate::shafts::Sun::of(&gameplay);
            shrubs = Some(std::sync::Arc::new(crate::objects::Shrubs::new(d, &self.queue, &self.mat_layout, &self.view_layout, &self.objects.shrub_layout, classes, &textures, &records, &gameplay)?));
            let (tc, tt, tr) = s.ties;
            ties = Some(std::sync::Arc::new(crate::objects::Ties::new(d, &self.queue, &self.mat_layout, &self.objects.tie_layout, tc, &tt, &tr, &gameplay, self.globals(level)?.ties.tests)?));
        }
        let (mc, mr, mt, specials) = s.mobies;
        let mut mobies = crate::mobies::Mobies::new(d, &self.queue, &self.mat_layout, &self.iso, level as usize, mc, &mr, &mt, specials)?;
        mobies.prepare(d, &self.queue, ram, &self.globals(level)?.mobies)?;
        let mobies = Some(mobies);
        let effects = crate::effects::Effects::new(d, &self.queue, s.effect_textures, self.fx_micro.clone())?;
        // The menu's WAD entry, read from the disc as its loader reads it; the table of contents the streaming slots read through.
        let mut disc = uyafmt::disc::Disc::open(&self.iso)?;
        let tc = uyafmt::toc::read(&mut disc)?;
        let menu = match self.globals(level)?.menu_entry {
            Some(m) => Some((m, uyafmt::levelcode::menu_entry_bytes(&mut disc, &tc, &m)?)),
            None => None,
        };
        let streams = (self.globals(level)?.stream_slots, self.iso.clone(), tc.bytes);
        let ee_textures = crate::eedraw::EeTextures::new(d, &self.queue, s.quad_textures.0, s.quad_textures.1, self.size, s.hud, s.gs_ram, menu, streams)?;
        // Vegetation adds pipelines whose layouts depend on this level's material table. The startup save predates those:
        // persist them here too, so the next launch can reuse the driver's compiled variants.
        crate::pipelines::save()?;
        println!("native: level {level}: loaded in {:.2} s", t0.elapsed().as_secs_f64());
        if let Some((named, replaced, errors)) = crate::texnames::take_counts() {
            println!("native: level {level}: {replaced} of {named} named textures drawn from the pack folder");
            for e in &errors {
                println!("native: level {level}: a texture with no pack name: {e}");
            }
        }
        self.level = Some((level, Gpu { vertices, indices, keys: std::sync::Arc::new(keys), draws: std::sync::Arc::new(s.draws), visibility: std::sync::Arc::new(s.visibility), sky: None, shrubs, ties, mobies, effects, ee_textures, sun, lit, shadows, rt, rt_views }));
        Ok(())
    }

    /// Level `level`'s globals.
    fn globals(&self, level: u32) -> Result<&Globals, String> {
        self.globals.get(&level).ok_or_else(|| format!("native: level {level} is not a story level"))
    }

    /// Whether a field's jobs read the main frame back before their first primitive (`mobies::Transfer`, TRXDIR 1, index 0): the
    /// game keeps the picture shown then, which this renderer grabs only when it has drawn the field before.
    pub fn reads_frame_back(&self, jobs: &crate::jobs::FrameJobs) -> Result<bool, String> {
        Ok(crate::mobies::walk(jobs, &self.boot, false, None)?.transfers.iter().any(|t| t.dir == 1 && t.index == 0))
    }

    /// The camera of level `level` in `ram`.
    /// Why the level could not be loaded (`render`), once it could not.
    pub fn broken(&self) -> Option<&str> {
        self.broken.as_deref()
    }

    pub fn camera(&self, ram: &[u8], level: u32) -> Result<Camera, String> {
        camera(ram, level, self.globals(level)?)
    }

    /// The GPU the pictures are drawn on, and the pictures' size.
    pub fn gpu(&self) -> (&wgpu::Instance, &wgpu::Adapter, &wgpu::Device, &wgpu::Queue) {
        (&self.instance, &self.adapter, &self.device, &self.queue)
    }

    pub fn size(&self) -> (u32, u32) {
        self.size
    }

    /// Pictures `height` pixels high from now on (the play window's resolution setting), at the shape the last frame was drawn
    /// at (`reshape`).
    pub fn resize(&mut self, height: u32) {
        self.reshape((width(height, self.k), height));
    }

    /// Pictures of `size` from now on (the game's view at another shape: the window's, PLAN 4.2; another RESOLUTION): the
    /// level's and the front end's EE textures follow (`EeTextures::resize`, in the next frame's commands: in order with the
    /// frames before when a presenter applies them), the other textures of the picture's size are made at it when next asked
    /// for, the level stays loaded, and the next world frame begins a shot (no picture between two frames of two sizes). A change
    /// of shape made the level's whole GPU state again before, and the front end's only when the front end saw it: a level
    /// frame's change left the front end's EE textures at the old width, and the next menu frame's scissors were that width in
    /// a picture of the new one (the player's v0.1.3 crash on quitting to the main menu after going fullscreen: "Scissor Rect {
    /// x: 0, y: 0, w: 2039, h: 1080 } is not contained in the render target (1920, 1080, 1)").
    fn reshape(&mut self, size: (u32, u32)) {
        if size == self.size {
            return;
        }
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native picture size") });
        if let Some((_, g)) = self.level.as_mut() {
            g.ee_textures.resize(&self.device, &mut enc, &self.shadow_pipes, size);
        }
        if let Some(t) = self.screen_tex.as_mut() {
            t.resize(&self.device, &mut enc, &self.shadow_pipes, size);
        }
        self.outbox.commands.push(enc.finish());
        self.size = size;
        self.last_world = None;
    }

    /// DLSS at the option's mode for the picture's size (`dlss.rs`): made when the option is turned on or the size changes,
    /// dropped when it is turned off. With it on and the driver not offering DLSS, the frame fails, saying so.
    fn update_upscaler(&mut self) -> Result<(), String> {
        let level = crate::enhance::dlss();
        if level == 0 {
            self.upscaler = None;
            return Ok(());
        }
        if self.upscaler.as_ref().is_some_and(|u| u.level == level && u.out == self.size) {
            return Ok(());
        }
        if !self.dlss_offered {
            return Err(format!("native: DLSS is on and {} does not offer it (an NVIDIA RTX GPU and driver are needed)", self.adapter.get_info().name));
        }
        if self.dlss_sdk.is_none() {
            self.dlss_sdk = Some(crate::dlss::sdk(&self.device)?);
        }
        // The old one's history goes with it; replays still holding it finish with it.
        self.upscaler = None;
        let sdk = self.dlss_sdk.as_ref().expect("made above");
        self.upscaler = Some(std::sync::Arc::new(crate::dlss::Upscaler::new(&self.device, &self.queue, &self.adapter, sdk, &self.dlss_pipes, level, self.size)?));
        Ok(())
    }

    /// The level's GPU state made again at the next level frame (the texture filtering option changed: its materials hold the
    /// samplers), with what its display buffer and grab hold brought over (`render_walked`).
    pub fn reload(&mut self) {
        self.last_world = None;
        self.reload_level = true;
    }

    /// Draws the world of the level being played with the game's camera and zone record in `ram` and returns the picture.
    pub fn draw(&mut self, ram: &[u8], jobs: &crate::jobs::FrameJobs, level: u32) -> Result<Image, String> {
        if let Ok(p) = std::env::var("UYA_GSM_LOAD") {
            self.settle_gsm()?;
            self.gsm.load(&p)?;
        }
        let shown = self.render(ram, jobs, level, &[])?;
        self.read_picture(&shown)
    }

    /// Draws a front end's frame (`render_screen`) and returns the picture shown, as `draw` does a level's.
    pub fn draw_screen(&mut self, ram: &[u8], jobs: &crate::jobs::FrameJobs, fill: bool) -> Result<Image, String> {
        let shown = self.render_screen(ram, jobs, fill)?;
        self.read_picture(&shown)
    }

    /// GS memory as `path` holds it (`dump_gsm`'s file: the memory a field's draw and its transfers left), in place of what the
    /// transfers taken so far leave.
    pub fn load_gsm(&mut self, path: &str) -> Result<(), String> {
        self.settle_gsm()?;
        self.gsm.load(path)
    }

    /// Exercise the presenter's actual between-frame path from saved consecutive fields (`uya native --replay-alpha`).
    /// A repeated alpha also reveals passes that accidentally accumulate when a frame is presented more than once.
    pub(crate) fn draw_replay(&self, alpha: f32) -> Result<Option<Image>, String> {
        let Some(replay) = self.replay.as_ref().filter(|r| r.between()) else { return Ok(None) };
        let (tex, depth) = picture_textures(&self.device, self.size);
        let tv = tex.create_view(&Default::default());
        let dv = depth.create_view(&Default::default());
        let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("saved frame replay") });
        let mut done = crate::replay::Chunks::default();
        replay.draw(&self.device, &self.queue, &mut enc, &crate::replay::Target { tex: &tex, depth: &depth, tv: &tv, dv: &dv, size: self.size }, alpha, None, &mut done)?;
        done.encoder(enc)?;
        self.queue.submit(done.finish()?);
        self.read_picture(&tex).map(Some)
    }

    fn read_picture(&self, shown: &wgpu::Texture) -> Result<Image, String> {
        let d = &self.device;
        let (w, h) = self.size;
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let row = (4 * w).div_ceil(256) * 256;
        let readback = d.create_buffer(&wgpu::BufferDescriptor { label: Some("readback"), size: (row * h) as u64, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false });
        let mut enc = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native readback") });
        enc.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: &shown, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &readback, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(row), rows_per_image: Some(h) } },
            extent,
        );
        self.queue.submit([enc.finish()]);
        let slice = readback.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        d.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("native: GPU wait: {e}"))?;
        rx.recv().map_err(|e| format!("native: readback: {e}"))?.map_err(|e| format!("native: readback: {e}"))?;
        let bytes = slice.get_mapped_range().map_err(|e| format!("native: readback: {e}"))?;
        let mut rgb = Vec::with_capacity((w * h * 3) as usize);
        for y in 0..h as usize {
            for px in bytes[y * row as usize..y * row as usize + 4 * w as usize].chunks_exact(4) {
                rgb.extend_from_slice(&px[..3]);
            }
        }
        drop(bytes);
        readback.unmap();
        Ok(Image { w: w as usize, h: h as usize, rgb })
    }

    /// Draws the frame as `draw` does and leaves the picture on the GPU (submitted, not waited for): the texture shown.
    /// Whether the field's jobs carry a world frame (a world program's run or a moby run): the field in which the game pauses
    /// still sends the last world frame, with its shadow volumes, and the pause's own fields send none.
    pub fn world_in(&self, jobs: &crate::jobs::FrameJobs) -> bool {
        let walk = crate::mobies::walk(jobs, &self.boot, true, None).or_else(|_| crate::mobies::walk(jobs, &self.boot, false, None));
        walk.is_ok_and(|w| w.has_runs() || w.ran.iter().any(|&r| r))
    }

    /// `read_back`: the main frame's pages read back in jobs before these that were not drawn (`rtgame::main_read_back`): the
    /// picture shown is grabbed for them, as for a read-back in this frame's own jobs.
    pub fn render(&mut self, ram: &[u8], jobs: &crate::jobs::FrameJobs, level: u32, read_back: &[u32]) -> Result<wgpu::Texture, String> {
        let walked = self.walk_here(jobs, false)?;
        self.render_walked(ram, jobs, level, read_back, walked)
    }

    /// The frame's walk on the render thread (`Walker::walk`), with the primitives' room the last frame handed back.
    fn walk_here(&mut self, jobs: &crate::jobs::FrameJobs, front_end: bool) -> Result<Walked, String> {
        let buf = if front_end { Vec::new() } else { std::mem::take(&mut self.ee_spare) };
        self.walker.as_mut().expect("the walker is the renderer's: play's walk thread walks the frames it draws").walk(jobs, front_end, buf)
    }

    /// `play`'s walk thread's `Walker`: the frames it hands over come walked (`render_walked`, `render_screen_walked`).
    pub(crate) fn take_walker(&mut self) -> Walker {
        self.walker.take().expect("the walker is taken once")
    }

    /// The room of the last frame's primitives, emptied (`Walker::walk`'s `buf`).
    pub(crate) fn take_ee_spare(&mut self) -> Vec<(usize, crate::mobies::EePrim)> {
        std::mem::take(&mut self.ee_spare)
    }

    /// `render` with the frame's walk made (`Walker::walk`).
    pub(crate) fn render_walked(&mut self, ram: &[u8], jobs: &crate::jobs::FrameJobs, level: u32, read_back: &[u32], walked: Walked) -> Result<wgpu::Texture, String> {
        let Walked::Level(walk, why) = walked else { return Err("native: a level's frame walked as the front end's".into()) };
        crate::mobies::dump_field(jobs.field);
        // A level's frames before its code sets the camera (the fade in after its load): the view at rest (`camera_at_rest`),
        // and no world drawn (`no_view`).
        self.no_view = false;
        let cam = match self.camera(ram, level) {
            Ok(c) => c,
            Err(_) => {
                self.no_view = true;
                camera_at_rest(ram, level, self.globals(level)?)?
            }
        };
        let g = *self.globals(level)?;
        // The picture at the shape the game's view was made at this frame (PLAN 4.2; `reshape`).
        self.k = cam.k;
        self.reshape((width(self.size.1, cam.k), self.size.1));
        // The terrain in the game's terrain buffer: the core's until a level chunk loads, then level chunk N's.
        let chunk = u32::from_le_bytes(ram[g.level_chunk as usize..g.level_chunk as usize + 4].try_into().expect("4 bytes"));
        if self.reload_level || self.level.as_ref().map(|l| l.0) != Some(level) {
            // The display buffer is GS memory, which a level change leaves as it was: the picture the TV shows until a frame
            // copies another into it. The level's GPU state made again holds a display buffer of its own, so it starts with the
            // one the level before left. Made empty, a trip between planets lost its last picture as the next level's code
            // came in: the game holds the planet, its stars and its name for some 50 fields while the next level's files load,
            // sending nothing, then fades that picture out from the display buffer (Veldin to Florana in the emulated game,
            // fields 2814 to 2880), and the fade and every field drawing nothing after the next level's load showed the empty
            // buffer, black (the player, 24 November 2026: "the planet disappears for a few seconds at the end of the loading
            // screen ship flight while the loading screen is still up"). The level before has this picture's size (`reshape`).
            // The same level made again (`reload`: a texture filtering or UPDATED TEXTURES change, made in a pause's options)
            // keeps its grab too, the pages it holds with it: the paused level's background, which the game puts back each pause
            // field and which the port's GS leaves nowhere else (made empty, the pause showed the game's RAM there instead).
            let same = self.level.as_ref().is_some_and(|l| l.0 == level);
            self.reload_level = false;
            let display = self.level.as_ref().map(|(_, g)| [g.ee_textures.targets[&crate::eedraw::DISPLAY].tex.clone(), g.ee_textures.display_gs.clone()]);
            let grab = self.level.as_ref().filter(|_| same).map(|(_, g)| (g.ee_textures.grab.clone(), g.ee_textures.grabbed.clone()));
            if let Err(e) = self.load(ram, level) {
                self.broken = Some(e.clone());
                return Err(e);
            }
            let ee = &mut self.level.as_mut().expect("loaded above").1.ee_textures;
            let mut enc = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("display buffer across a level change") });
            if let Some(display) = display {
                for (from, to) in display.iter().zip([&ee.targets[&crate::eedraw::DISPLAY].tex, &ee.display_gs]) {
                    enc.copy_texture_to_texture(from.as_image_copy(), to.as_image_copy(), from.size());
                }
            }
            if let Some((tex, pages)) = grab {
                enc.copy_texture_to_texture(tex.as_image_copy(), ee.grab.as_image_copy(), tex.size());
                ee.grabbed = pages;
            }
            self.queue.submit([enc.finish()]);
        }
        self.update_upscaler()?;
        // GS memory as the transfers of the fields before leave it is the level's EE textures' for the frame (`take_uploads`).
        self.settle_gsm()?;
        std::mem::swap(&mut self.gsm, &mut self.level.as_mut().expect("loaded above").1.ee_textures.gsm);
        // The frame set this frame builds into: one no frame still drawn again holds (`play` gives each replay up once the
        // pictures after its frame are drawn).
        let set_wait = std::time::Instant::now();
        let set = loop {
            if let Some(k) = self.sets.iter().position(|s| std::sync::Arc::strong_count(&s.busy) == 1) {
                break k;
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        };
        if crate::play::frame_times() { println!("frame stages: buffers {} wait {:.3}", jobs.field, set_wait.elapsed().as_secs_f64() * 1e3); }
        let fog = walk.fog(fog_of(ram, &g), near_clip_of(ram, &g));
        let zones = &ram[g.zones as usize..g.zones as usize + 0x80];
        let pic = self.picture(&cam, &fog, set);
        // The frame's light sets, as the game has them (the per-pixel lighting's, `objects` `lit`).
        let sets = ram.get(g.light_sets as usize..g.light_sets as usize + 0x400).ok_or("native: the light sets past RAM")?;
        self.queue.write_buffer(&pic.view, SETS_AT, sets);
        // The static world's layers need only RAM, the jobs, the camera and the fog: the terrain, the ties (the tie programs' runs
        // read from the frame's VIF1 jobs, `tievu::tie_draws`, then the instances) and the shrubs are built and their draws of
        // the picture recorded, each into render bundles of its own (`replay.rs`: every picture of the frame executes them), on
        // threads of their own beside the moby walk and the rest of the frame (recording ten thousand draws a frame was a third of
        // the frame on level 2). The frame executes them after the picture's clear in the game's order, each only when its
        // layer's programs ran this frame (a layer that did not run is dropped, its error with it).
        let terrain = if chunk == 0 { 0 } else { 1 + chunk };
        // The sky the game draws: the one its pointer names (func_L01_00422698 points it at a sky as it relocates it in RAM;
        // func_L01_004A46C8 draws from it each frame), made again when it names another. A level's own sky, and the next level's
        // while the level before still draws its frames under the fade (Veldin to level 27 by the game's own level change: from
        // field 535 the pointer names level 27's sky, of 5 meshes, Veldin's 8; Veldin's frames are drawn until level 27 sets its
        // camera).
        let sky_at = u32::from_le_bytes(ram[g.sky as usize..g.sky as usize + 4].try_into().expect("4 bytes"));
        let sky = {
            let gpu = &mut self.level.as_mut().expect("loaded above").1;
            match ram.get(sky_at as usize..sky_at as usize + 0x40).map(|h| <[u8; 0x40]>::try_from(h).expect("0x40 bytes")) {
                None => Err(format!("native: the sky pointer at {:#x} names {sky_at:#x}, past RAM", g.sky)),
                Some(head) if gpu.sky.as_ref().is_some_and(|s| (s.0, s.1) == (sky_at, head)) => Ok(gpu.sky.as_ref().expect("tested").2.clone()),
                Some(head) => {
                    gpu.sky = None;
                    let made = uyafmt::coremisc::sky_in_ram(ram, sky_at).map_err(|e| format!("native: {e}")).and_then(|k| crate::sky::SkyGpu::new(&self.device, &self.queue, &self.sky_pipes, &k));
                    made.map(|k| {
                        let k = std::sync::Arc::new(k);
                        gpu.sky = Some((sky_at, head, k.clone()));
                        k
                    })
                }
            }
        };
        let (scene, shrubs, ties) = {
            let gpu = &self.level.as_ref().expect("loaded above").1;
            // The lighting option (`lighting.rs`): the terrain lit per pixel, its pipelines and the light pass's inputs.
            let lit = crate::enhance::lighting().then(|| gpu.lit.clone());
            let (pass, fail) = if lit.is_some() { (self.lighting.pass.clone(), self.lighting.fail.clone()) } else { (self.pass.clone(), self.fail.clone()) };
            let scene = TerrainScene { vertices: gpu.vertices.clone(), indices: gpu.indices.clone(), draws: gpu.draws.clone(), keys: gpu.keys.clone(), visibility: gpu.visibility.clone(), sky, pass, fail, lit };
            (scene, gpu.shrubs.clone(), gpu.ties.clone())
        };
        // With the world deferred to the presenter, the ties' corners laid out anew wait for it in the frame's outbox (collected here
        // from the ties' threads, whatever becomes of the frame); else they are written as the ties' threads lay them out.
        let corner_writes = std::sync::Mutex::new(Vec::new());
        let corners_out = self.defer_world.then_some(&corner_writes);
        let objects = self.objects.clone();
        let tie_rooms = std::mem::take(&mut self.sets[set].tie_rooms);
        // The last frame's replay, which an instance's corners laid out anew (another LOD) leave not to be drawn again.
        let stale = if self.defer_world { None } else { self.last_stale.clone() };
        let stale = stale.as_deref();
        let device = self.device.clone();
        let queue = self.queue.clone();
        let queue = &queue;
        let (cam, g, fog, pic_ref, objects) = (&cam, &g, &fog, &pic, &objects);
        // DRAW DISTANCE FAR's reach, as the field's own culls took it (`range.rs`): the renderer's shrubs and effects go as far.
        let far_reach = jobs.far_reach;
        let pool = self.pool.clone();
        let pool = &pool;
        let ties_shared = ties.clone();
        let shrub_room = std::mem::take(&mut self.shrub_spare);
        let result = pool.scope(|s| {
            let device_w = device.clone();
            let terrain_part = s.spawn(move || -> Result<(Static, usize, f64), String> {
                let t = std::time::Instant::now();
                let (b, chunks) = terrain_pass(&device_w, pic_ref, &scene, terrain, zones, set)?;
                Ok((b, chunks, t.elapsed().as_secs_f64() * 1000.0))
            });
            let device_t = device.clone();
            let ties_part = s.spawn(move || -> Result<Option<TiePart>, String> {
                let Some(ti) = &ties else { return Ok(None) };
                let t = std::time::Instant::now();
                let td = crate::tievu::tie_draws(jobs)?;
                let programs = t.elapsed().as_secs_f64() * 1000.0;
                let lap = std::time::Instant::now();
                let coords = crate::objects::second_coordinates(jobs)?;
                // The instances in ranges of about as many listed tie triangles each, one a TIE_TRIANGLES, up to TIE_THREADS, each built
                // (`Ties::frame`: the frame's values of the instances kept on the GPU) and recorded into a bundle of its own on a
                // thread of the pool's (level 28's arrival: 925,000 vertices a frame took 32 to 41 ms on one thread when they were
                // made each frame); the bundles follow one another in instance order.
                let ranges = ti.split(ram, &g.ties, TIE_TRIANGLES, TIE_THREADS)?;
                // Each range's room, kept from the frame before.
                let mut spare = tie_rooms.into_iter();
                let rooms: Vec<crate::objects::TieRoom> = ranges.iter().map(|_| spare.next().unwrap_or_default()).collect();
                type Built = (Vec<Static>, crate::objects::TieRoom, (usize, usize, usize), usize, crate::objects::TieStats, [f64; 2], usize);
                let build = |range: std::ops::Range<usize>, mut room: crate::objects::TieRoom| -> Result<Built, String> {
                    let mut stats = crate::objects::TieStats::default();
                    let counts = ti.frame(queue, ram, &g.ties, cam, &td, &coords, range, set, stale, corners_out, &mut room, &mut stats)?;
                    let (n_draws, corners_drawn) = (room.draws.draws.len(), room.draws.ranges.iter().map(|r| r.1 as usize).sum());
                    let upload = stats.upload_ns as f64 / 1e6;
                    let lap = std::time::Instant::now();
                    // The draws with their TEST_1s' materials (`objects::Pipelines::draw_ties`).
                    let (corners, group) = ti.buffers(set);
                    // One bundle a range: made in pieces on the pool's threads, a one-range frame's ties recorded 0.2-0.3 ms sooner but
                    // took those threads from the mobies' crew work beside them, and the frame took 0.2-0.4 ms longer (R3-021).
                    let bundle = if n_draws > 0 {
                        vec![picture_bundle(&device_t, "native ties", pic_ref, |p| {
                            crate::replay::Enc::set_vertex_buffer(p, 0, corners);
                            crate::replay::Enc::set_bind_group(p, 2, group);
                            objects.draw_ties(p, &ti.materials, &room.draws.draws, &room.draws.ranges)
                        })?]
                    } else {
                        Vec::new()
                    };
                    let record = lap.elapsed().as_secs_f64() * 1000.0;
                    Ok((bundle, room, counts, n_draws, stats, [upload, record], corners_drawn))
                };
                let built = pool.scope(|s2| -> Result<Vec<Built>, String> {
                    let build = &build;
                    let mut work = ranges.into_iter().zip(rooms);
                    let (first, first_bufs) = work.next().expect("a range at least");
                    let others: Vec<_> = work.map(|(r, b)| s2.spawn(move || build(r, b))).collect();
                    let mut out = vec![build(first, first_bufs)?];
                    for h in others {
                        out.push(h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))?);
                    }
                    Ok(out)
                })?;
                let ranges_ms = lap.elapsed().as_secs_f64() * 1000.0;
                let (mut counts, mut bundles, mut rooms, mut draws, mut vertices) = ((0, 0, 0), Vec::new(), Vec::new(), 0, 0);
                let (mut stats, mut upload, mut record) = (crate::objects::TieStats::default(), 0.0, 0.0);
                for (b, room, n, d, s, [u, r], c) in built {
                    stats.add(&s);
                    (upload, record) = (upload + u, record + r);
                    bundles.extend(b);
                    (counts.0, counts.1, counts.2) = (counts.0 + n.0, counts.1 + n.1, counts.2 + n.2);
                    (draws, vertices) = (draws + d, vertices + c);
                    rooms.push(room);
                }
                Ok(Some(TiePart { counts, bundles, programs, stages: [ranges_ms, draws as f64, vertices as f64], stats, upload, record, all: t.elapsed().as_secs_f64() * 1000.0, rooms }))
            });
            let shrubs_part = s.spawn(move || -> Result<Option<ShrubPart>, String> {
                let Some(sh) = &shrubs else { return Ok(None) };
                let t = std::time::Instant::now();
                // The drawn instances, in the game's order, as runs of the level's built shrubs (`Shrubs::frame`).
                let visible = sh.visible(ram, &g.shrubs, cam, far_reach)?;
                let (mut draws, mut groups) = shrub_room;
                draws.clear();
                groups.clear();
                let mut stages = [0.0; 3];
                let mut lap = std::time::Instant::now();
                let mut stage = |k: usize| {
                    stages[k] = lap.elapsed().as_secs_f64() * 1000.0;
                    lap = std::time::Instant::now();
                };
                let bundles = match sh.frame(&device, queue, &objects.shrub_layout, ram, &g.shrubs, cam, fog, &visible, set, &mut draws, &mut groups)? {
                    Some((vertices, batch_vertices, fog_group)) if !draws.is_empty() => {
                        stage(0);
                        stage(1);
                        // The draws in up to SHRUB_THREADS runs of about as many each, each recorded into a bundle of its own on a
                        // thread of the pool's (level 1's 10,149 draws took 11.4 ms to record on one thread); the bundles follow one
                        // another in that order.
                        // Split where the draws the frame's indices gave were split (as many of those in each but the last), so that each
                        // shrub's failing pixels come after the same passing pixels as before.
                        let per = groups.len().div_ceil(SHRUB_THREADS);
                        let cuts: Vec<usize> = (0..SHRUB_THREADS).filter_map(|k| groups.get(k * per).copied()).chain([draws.len()]).collect();
                        let runs: Vec<&[(usize, crate::objects::Draw)]> = cuts.windows(2).map(|w| &draws[w[0]..w[1]]).collect();
                        let record = |run: &[(usize, crate::objects::Draw)]| shrub_bundle(&device, pic_ref, &fog_group, |p| match &sh.batch { Some(batch) => batch.draw(p, &vertices, &batch_vertices, run), None => objects.draw_shrubs(p, &sh.materials, &vertices, run) });
                        let bundles = pool.scope(|s2| -> Result<Vec<Static>, String> {
                            let record = &record;
                            let others: Vec<_> = runs[1..].iter().map(|&run| s2.spawn(move || record(run))).collect();
                            let mut out = vec![record(runs[0])?];
                            for h in others {
                                out.push(h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))?);
                            }
                            Ok(out)
                        })?;
                        stage(2);
                        bundles
                    }
                    _ => Vec::new(),
                };
                let (drawn, runs_made, all) = (visible.len(), groups.len(), t.elapsed().as_secs_f64() * 1000.0);
                draws.clear();
                groups.clear();
                Ok(Some(ShrubPart { drawn, draws: runs_made, stages, bundles, all, room: (draws, groups) }))
            });
            self.render_frame(ram, jobs, level, read_back, cam, g, chunk, fog, &pic, set, (terrain_part, ties_part, shrubs_part), (walk, why))
        });
        if let Some(ties) = &ties_shared {
            let corners = ties.buffers(set).0;
            let writes = corner_writes.into_inner().map_err(|_| "native: the tie corner writes' lock is poisoned")?;
            if crate::play::frame_times() && !writes.is_empty() {
                println!("frame stages: ties laid out {}: {} runs, {} bytes", jobs.field, writes.len(), writes.iter().map(|w| w.1.len()).sum::<usize>());
            }
            for (offset, bytes) in writes {
                self.outbox.writes.push((corners.clone(), offset, bytes));
            }
        }
        std::mem::swap(&mut self.gsm, &mut self.level.as_mut().expect("loaded above").1.ee_textures.gsm);
        result
    }

    /// A frame with no level's world (the front end's: its menus, the 3D its VU1 programs draw, the boot screens) drawn natively as
    /// the GS draws its jobs (`mobies::screen_walk`, `eedraw::build`), onto a cleared picture at the screen's shape (`uyart`'s
    /// view widening: the window's with WIDESCREEN on), its 2D in the middle at its own scale and what it draws over the frame's
    /// whole width across the picture (`eedraw::unstretched`), its 3D placed as the 2D about it is (`effects::Projection::at_rest`);
    /// with `fill` (a level's space scene of its arrival, which the front end draws) the frame across the picture's width instead,
    /// its top and bottom cut off (the player: "cut off the top and bottom and make it fill the screen horizontally"; at its own
    /// scale its parts past the PS2's frame showed at the sides: the backdrop's jagged outline, the frame's blue clear). Submitted,
    /// not waited for. The picture shown: the native picture, or the display buffer when the frame draws onto that (a fade).
    pub fn render_screen(&mut self, ram: &[u8], jobs: &crate::jobs::FrameJobs, fill: bool) -> Result<wgpu::Texture, String> {
        let walked = self.walk_here(jobs, true)?;
        self.render_screen_walked(ram, jobs, fill, walked)
    }

    /// `render_screen` with the frame's walk made (`Walker::walk`).
    pub(crate) fn render_screen_walked(&mut self, ram: &[u8], jobs: &crate::jobs::FrameJobs, fill: bool, walked: Walked) -> Result<wgpu::Texture, String> {
        let Walked::Screen(walk) = walked else { return Err("native: the front end's frame walked as a level's".into()) };
        crate::mobies::dump_field(jobs.field);
        self.no_view = false;
        let k = uyart::view_k();
        self.reshape((width(self.size.1, k), self.size.1));
        self.k = k;
        // The view at the picture's shape; the PS2's frame spans tangents 0.62 and 0.4805.
        let tan = if fill { [0.62, 0.4805 / k] } else { [0.62 * k, 0.4805] };
        let cam = Camera { eye: [0.0; 3], forward: [1.0, 0.0, 0.0], up: [0.0, 0.0, 1.0], tan, k: if fill { 1.0 } else { k } };
        let proj = crate::effects::Projection::at_rest(&cam, [0.62, 0.4805]);
        // No fog (F 255 at every depth).
        let fog = uyafmt::gameplay::Fog { colour: [0; 3], near: 1.0, far: 2.0, near_f: 255.0, far_f: 255.0 };
        let set = loop {
            if let Some(k) = self.sets.iter().position(|s| std::sync::Arc::strong_count(&s.busy) == 1) {
                break k;
            }
            std::thread::sleep(std::time::Duration::from_micros(200));
        };
        let pic = self.picture(&cam, &fog, set);
        if self.screen_tex.is_none() {
            let streams = (uyafmt::levelcode::StreamSlots { pool: 0, count: 0 }, self.iso.clone(), Vec::new());
            self.screen_tex = Some(crate::eedraw::EeTextures::new(&self.device, &self.queue, Vec::new(), 0, self.size, uyafmt::hud::HudData::default(), (Vec::new(), Vec::new()), None, streams)?);
        }
        // GS memory as the transfers of the fields before leave it; the frame's own packets' bytes.
        self.settle_gsm()?;
        let tex = self.screen_tex.as_mut().expect("made above");
        std::mem::swap(&mut self.gsm, &mut tex.gsm);
        tex.frame = walk.bytes;
        tex.prompts.look = self.prompts.clone();
        let mut drawn = crate::eedraw::build(&walk.ee, &walk.transfers, &proj, &cam, tex, 0, ram, &self.boot, &self.device, &self.queue, &self.mat_layout, std::mem::take(&mut self.ee_vert_spare), None, Vec::new());
        std::mem::swap(&mut self.gsm, &mut tex.gsm);
        let tex = self.screen_tex.as_ref().expect("made above");
        let d = &self.device;
        let (w, h) = self.size;
        let fs = &mut self.sets[set];
        let vb = fs.ee.put(d, &self.queue, "EE steps", bytemuck::cast_slice(&drawn.vertices));
        self.ee_vert_spare = std::mem::take(&mut drawn.vertices);
        // The steps in the jobs' order; every other frame buffer's depth starts the frame clear.
        let mut rec = crate::replay::Rec::default();
        for t in tex.targets.values() {
            rec.target(&t.view, &t.depth, true);
        }
        if let Some(vb) = &vb {
            for (_, step) in &drawn.steps {
                match step {
                    crate::eedraw::Step::Draw { .. } => ee_draw(d, &self.objects, self.size, tex, &mut rec, vb, step)?,
                    crate::eedraw::Step::Reflect(_) => return Err("native: a water reflection in a frame with no level's world".into()),
                    step => {
                        use crate::replay::Enc;
                        rec.picture();
                        rec.set_vertex_buffer(0, vb);
                        self.shadow_pipes.draw(&mut rec, step)?;
                    }
                }
            }
        }
        rec.end();
        let target = crate::replay::Target { tex: &pic.tex, depth: &pic.depth, tv: &pic.tv, dv: &pic.dv, size: self.size };
        let mut clear = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native screen clear") });
        crate::replay::statics(&mut clear, &target, &[], &[], None);
        let mut enc = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native screen frame") });
        let mut after = crate::replay::Chunks::default();
        crate::replay::play(&rec.cmds, &mut enc, &target, &crate::replay::Ctx { device: d, view_bg: &pic.view_bg, shadows: &self.shadow_pipes, ao: &self.ao, light_shadows: &self.light_shadows, reflections: &self.reflections, shafts: &self.shafts, cam: &cam, fog: &fog, fresh: true, profile: None, upscale: None }, &mut after)?;
        // The picture: the native picture, which the display buffer then takes (as the next frame's first job copies the frame into
        // it); a frame that draws onto the display buffer (a fade) shows that.
        let display = &tex.targets[&crate::eedraw::DISPLAY].tex;
        let shown = if drawn.display {
            display.clone()
        } else {
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: &pic.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: display, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            );
            self.shadow_pipes.downsample(d, &mut enc, display, &tex.display_gs, crate::eedraw::display_gs_map((w, h)));
            pic.tex.clone()
        };
        self.outbox.commands.push(clear.finish());
        self.outbox.commands.extend(after.finish()?);
        self.outbox.commands.push(enc.finish());
        if !self.defer_world {
            std::mem::take(&mut self.outbox).apply(&self.queue);
        }
        self.last_stale = None;
        self.last_world = None;
        if !self.quiet {
            println!("native: screen frame: {} EE steps ({} triangles){}", drawn.steps.len(), drawn.triangles, walk.stopped.iter().map(|why| format!(", a job not read: {why}")).chain(drawn.skipped.iter().map(|(why, n)| format!(", {n} not drawn: {why}"))).collect::<String>());
        }
        // What a screen frame cannot draw is named in the log, each reason the first time (the play window's renderer is quiet).
        for why in walk.stopped.iter().map(|w| format!("a job not read: {w}")).chain(drawn.skipped.iter().map(|(w, _)| format!("not drawn: {w}"))) {
            if self.said.len() < 400 && self.said.insert(why.clone()) {
                println!("native: field {}: screen frame: {why}", jobs.field);
            }
        }
        Ok(shown)
    }

    /// GS memory to `path` (`UYA_GSM_DUMP_AT`).
    pub fn dump_gsm(&mut self, path: &str) -> Result<(), String> {
        self.settle_gsm()?;
        self.gsm.dump(path)
    }

    /// Image transfers into GS memory (`gsmem.rs`) of fields, whichever renderer drew them, in the order the game sent them: kept
    /// across fields and levels as the GS keeps them. A frame's own are taken after it is drawn (its chain tells its draws what
    /// they sample while it runs), the fields' before it before.
    pub fn take_uploads(&mut self, uploads: Vec<uyart::hw::GsUpload>) {
        self.gsm_pending.extend(uploads);
    }

    /// GS memory made ready for a frame: the transfers taken since it was last made ready written in order, unless they are the
    /// set written then. Each transfer writes its texels whatever the memory held, so the same set written again leaves the
    /// memory as it is: on level 9's ground almost every frame's 118 transfers (785 KB, the textures the game sends each field)
    /// are the frame before's, and writing them was 0.8 ms of the render thread.
    fn settle_gsm(&mut self) -> Result<(), String> {
        let same = |a: &uyart::hw::GsUpload, b: &uyart::hw::GsUpload| (a.bitbltbuf, a.trxpos, a.trxreg) == (b.bitbltbuf, b.trxpos, b.trxreg) && a.data == b.data;
        if self.gsm_pending.len() == self.gsm_settled.len() && self.gsm_pending.iter().zip(&self.gsm_settled).all(|(a, b)| same(a, b)) {
            self.gsm_pending.clear();
            return Ok(());
        }
        for u in &self.gsm_pending {
            self.gsm.upload(u.bitbltbuf, u.trxpos, u.trxreg, &u.data)?;
        }
        std::mem::swap(&mut self.gsm_pending, &mut self.gsm_settled);
        self.gsm_pending.clear();
        Ok(())
    }

    /// The frame's replay (`replays` set: the frame drew the world on the native picture), for `play` to draw pictures between
    /// it and the frame before.
    pub fn take_replay(&mut self) -> Option<crate::replay::Replay> {
        self.replay.take()
    }

    /// With `defer_world`: what the frames rendered since the last call change in the shared GPU state, and the commands of a
    /// frame drawn as it was prepared (`Outbox`), for the presenter to apply in order. Taken after every `render`, drawn or not.
    pub fn take_outbox(&mut self) -> Outbox {
        std::mem::take(&mut self.outbox)
    }

    /// The last frame's set, held by whoever holds its picture until it is shown (`play`: the frame set's picture
    /// is drawn into again once nothing holds the set).
    /// The level's EE textures so far (`EeTextures::census`), None before a level is loaded.
    pub fn ee_census(&self) -> Option<String> {
        self.level.as_ref().map(|(_, g)| g.ee_textures.census())
    }

    pub fn take_set(&mut self) -> Option<std::sync::Arc<()>> {
        self.set_held.take()
    }

    /// The frame's native picture and depth, and its view/fog as group 0: frame set `set`'s resources, with their contents updated.
    /// The presenter holds the set until it stops displaying the picture, with or without interpolated frames.
    fn picture(&mut self, cam: &Camera, fog: &uyafmt::gameplay::Fog, set: usize) -> Picture {
        use wgpu::util::DeviceExt;
        self.set_held = Some(self.sets[set].busy.clone());
        // One depth for every frame the renderer draws (a frame's depth is read only while it is drawn: a picture between
        // frames has the presenter's own).
        if self.depth.as_ref().is_none_or(|(t, _)| (t.width(), t.height()) != self.size) {
            let t = depth_texture(&self.device, self.size);
            let v = t.create_view(&Default::default());
            self.depth = Some((t, v));
        }
        let (depth, dv) = self.depth.clone().expect("made above");
        let raster_on = crate::enhance::rt_lighting() > 0 || crate::enhance::rt_reflections() > 0;
        // The surface buffers are the world's: at DLSS's render size with it on.
        let scene = self.upscaler.as_ref().map_or(self.size, |u| u.render);
        let raster = raster_on.then(|| raster_at(&mut self.raster, &self.device, scene));
        if let Some(p) = self.sets[set].picture.as_ref().filter(|p| (p.tex.width(), p.tex.height()) == self.size && p.raster.is_some() == raster_on) {
            self.queue.write_buffer(&p.view, 0, bytemuck::bytes_of(&view_u(cam, fog, 0.0)));
            return Picture { raster, tex: p.tex.clone(), depth, tv: p.tv.clone(), dv, view: p.view.clone(), view_bg: p.view_bg.clone() };
        }
        let d = &self.device;
        let tex = colour_texture(d, self.size);
        let (tv, dv) = (tex.create_view(&Default::default()), depth.create_view(&Default::default()));
        // A picture between this frame and the one before writes its own view here (`replay::Replay::draw`).
        let mut contents = bytemuck::bytes_of(&view_u(cam, fog, 0.0)).to_vec();
        contents.resize(SETS_AT as usize + 0x400, 0);
        let view = d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("view"), contents: &contents, usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST });
        // Static geometry reads the level sets in the view uniform. Its group still needs a valid storage binding.
        let empty = d.create_buffer(&wgpu::BufferDescriptor { label: Some("empty actor lights"), size: 64, usage: wgpu::BufferUsages::STORAGE, mapped_at_creation: false });
        let view_bg = view_group(d, &self.view_layout, &view, &empty);
        let p = Picture { raster, tex, depth, tv, dv, view, view_bg };
        self.sets[set].picture = Some(Picture { raster: p.raster.clone(), tex: p.tex.clone(), depth: p.depth.clone(), tv: p.tv.clone(), dv: p.dv.clone(), view: p.view.clone(), view_bg: p.view_bg.clone() });
        p
    }

    /// `render` after the level is loaded, with the terrain's, the ties' and the shrubs' passes coming from their threads.
    #[allow(clippy::too_many_arguments)]
    /// `fset` is the frame set the frame builds into (`SETS`).
    fn render_frame(&mut self, ram: &[u8], jobs: &crate::jobs::FrameJobs, level: u32, read_back: &[u32], cam: &Camera, g: &Globals, chunk: u32, fog: &uyafmt::gameplay::Fog, pic: &Picture, fset: usize, (terrain_part, ties_part, shrubs_part): (PoolJob<Result<(Static, usize, f64), String>>, PoolJob<Result<Option<TiePart>, String>>, PoolJob<Result<Option<ShrubPart>, String>>), (mut walk, why): (crate::mobies::Walk, Option<String>)) -> Result<wgpu::Texture, String> {
        let mut tm = Times::new();
        // The mobies first: their pass makes the texture settings of the blends the chain asks for as it meets them.
        let (mut moby_objs, mut moby_draws) = (std::mem::take(&mut self.moby_spare), std::mem::take(&mut self.moby_draw_spare));
        moby_objs.clear();
        moby_draws.clear();
        let mut run: Vec<crate::objects::Draw> = std::mem::take(&mut self.run_spare);
        // Where the frame before drew each moby vertex (`mobies::Mobies::frame`), for the pictures between the two.
        let mut moby_prev = std::mem::take(&mut self.moby_prev_spare);
        moby_prev.clear();
        let mut moby_lit = std::mem::take(&mut self.moby_lit_spare);
        moby_lit.clear();
        let mut light_rows = std::mem::take(&mut self.light_spare);
        light_rows.clear();
        // The walk's EE primitives, each with the moby draw it comes before.
        let mut moby_ee;
        // The frame buffers other than the native picture the moby runs draw into (key, rows).
        let mut moby_targets = Vec::new();
        // The frame's jobs read in kick order (`mobies::walk`): the moby runs, the EE's own primitives and the passes of the
        // programs run on rac-rs's VU1. A moby chain the walk cannot read stops the mobies of this picture only (the walk is read
        // again without it), with its reason in the summary line.
        let mut mobies_skipped: Vec<(String, usize)> = why.map(|e| (format!("every run: {e}"), 1)).into_iter().collect();
        tm.mark("walk");
        // Program 6's inputs, which also give the frame's projection: it puts the shadows the EE writes into the moby chain, and the
        // water, back in the world.
        // At DRAW DISTANCE FAR each effect is drawn out to the reach too (`uyafmt::prog6::Frame::reach`, in 1/4096 view-depth units).
        let fx_reach = jobs.far_reach.map(|r| (r as f64 * 4096.0) as i64);
        let fx_frame = uyafmt::prog6::Frame::from_ram_at(ram, &g.fx).map(|f| uyafmt::prog6::Frame { reach: fx_reach, ..f }).and_then(|f| crate::effects::Projection::of(&f).map(|p| (f, p)));
        // The water reflections option (`reflections.rs`): the frame's water plane from the passes' output (a frame with no
        // projection, a level's first, reflects nothing).
        let mut passes = std::mem::take(&mut walk.passes);
        // The ray-traced reflections (`rtlight.rs`) trace the water's reflection at their level.
        let reflect = crate::enhance::reflections().max(crate::enhance::rt_reflections());
        let plane = match (reflect, &fx_frame) {
            (0, _) | (_, Err(_)) => None,
            (_, Ok((_, proj))) => crate::reflections::plane(&walk.ee, &passes, proj, cam),
        };
        tm.mark("water plane");
        for (what, ms) in crate::mobies::walk_times() {
            tm.note(what, ms);
        }
        let has_runs = walk.has_runs();
        // The transfers into and out of the main frame. A download before the frame's first primitive takes the picture shown
        // now (the frame before's, in the display buffer); the grab is made before the frame is drawn.
        let mut transfers = walk.transfers.clone();
        let mut grab_now = !read_back.is_empty();
        self.level.as_mut().expect("loaded above").1.ee_textures.grabbed.extend(read_back.iter().copied());
        for t in transfers.iter().filter(|t| t.dir == 1) {
            if t.index != 0 {
                mobies_skipped.push(("a download of the main frame after the frame's first primitive".to_string(), 1));
                continue;
            }
            let pages = t.main_pages()?;
            self.level.as_mut().expect("loaded above").1.ee_textures.grabbed.extend(pages);
            grab_now = true;
        }
        if grab_now {
            // The grab made below is the one a change of the picture's size brings over from now on (`EeTextures::resize`).
            self.level.as_mut().expect("loaded above").1.ee_textures.grab_original = None;
        }
        // The world layers whose programs ran this frame (terrain 1 and 7, ties 2 and 3, shrubs 4 and 5, effects 6).
        let ran = walk.ran;
        let (terrain_ran, ties_ran, shrubs_ran) = (!self.no_view && (ran[1] || ran[7]), !self.no_view && (ran[2] || ran[3]), !self.no_view && (ran[4] || ran[5]));
        // A frame that draws mobies and no terrain, ties or shrubs (a trip's space scene: the level's code drawing the ship on its
        // stars and the planet, the cockpit's shots; the weapon upgrade's gun) is drawn here as every frame is: no other layer
        // draws a frame (the player, 19 November 2026: "Remove all the old 2D layer/ps2 stuff"; the trip to Aquatos's Clank was
        // noise in the 2D layer, which decoded his skin from GS memory it never saw uploaded).
        // The GIF's held Q when the effect pass starts (`mobies::HeldQ`).
        let mut fx_q = None;
        let mobies_drawn = match (&mut self.level.as_mut().expect("loaded above").1.mobies, has_runs && !self.no_view) {
            (Some(mo), true) => match mo.frame(&self.pool, &self.device, &self.queue, &self.mat_layout, ram, walk, &g.mobies, &cam, jobs.field, &mut moby_objs, &mut moby_prev, &mut moby_lit, &mut light_rows, &jobs.skin, self.replays, &mut moby_draws, &mut moby_targets, &mut fx_q, &mut transfers, &mut passes) {
                Ok((n, sk, ee)) => {
                    mobies_skipped.extend(sk);
                    moby_ee = ee;
                    n
                }
                Err(e) => {
                    (moby_objs, moby_prev, moby_lit, moby_draws, moby_ee) = (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
                    mobies_skipped.push((format!("every run: {e}"), 1));
                    0
                }
            },
            _ => {
                // No moby run to draw: the EE's primitives alone, before the effects or after them.
                if let Some(crate::mobies::HeldQ::Value(q)) = walk.fx_q {
                    fx_q = Some(q);
                }
                mobies_skipped.extend(walk.stopped.into_iter().map(|e| (e, 1)));
                moby_ee = walk.ee;
                for (at, p) in &mut moby_ee {
                    *at = if p.after == crate::mobies::AFTER_EFFECTS { crate::mobies::AFTER_EFFECTS } else { 0 };
                }
                0
            }
        };
        tm.mark("mobies");
        // The effects (program 6) likewise: what the pass cannot read stops the effects of this picture only.
        let (mut fx_objs, mut fx_draws) = (std::mem::take(&mut self.fx_spare), std::mem::take(&mut self.fx_draw_spare));
        let mut fx_prev = std::mem::take(&mut self.fx_prev_spare);
        fx_objs.clear();
        fx_prev.clear();
        fx_draws.clear();
        let fx = match &fx_frame {
            Ok(_) if !ran[6] => Ok((0, Vec::new())),
            Ok((f, p)) => self.level.as_mut().expect("loaded above").1.effects.frame(&self.device, &self.mat_layout, f, p, &cam, fx_q, &mut fx_objs, &mut fx_prev, &mut fx_draws, tm.on),
            Err(e) => Err(e.clone()),
        };
        let (fx_drawn, fx_skipped) = match fx {
            Ok(r) => r,
            Err(e) => {
                fx_objs.clear();
                fx_prev.clear();
                fx_draws.clear();
                (0, vec![(format!("every effect: {e}"), 1)])
            }
        };
        tm.mark("effects");
        if ran[6] && fx_frame.is_ok() {
            let stages = self.level.as_ref().expect("loaded above").1.effects.stages;
            for (what, ms) in ["effects: prepare/decode", "effects: program", "effects: vertices", "effects: materials"].into_iter().zip(stages) {
                tm.note(what, ms);
            }
        }
        // What the EE draws itself in the chain (shadows, its own triangles), with the core's RAM address for their textures.
        let core_ram = u32::from_le_bytes(ram[g.mobies.core as usize..g.mobies.core as usize + 4].try_into().expect("4 bytes"));
        // The frame buffers the moby runs draw into.
        for &(key, rows) in &moby_targets {
            self.level.as_mut().expect("loaded above").1.ee_textures.target(&self.device, key, rows)?;
        }
        // The HUD textures and palettes resident in GS memory this frame (their tables' GS blocks in the header's copy in RAM).
        self.level.as_mut().expect("loaded above").1.ee_textures.prompts.look = self.prompts.clone();
        self.level.as_mut().expect("loaded above").1.ee_textures.hud_frame(ram, g.hud_header)?;
        // A frame before the level's code sets the camera (`no_view`) has no projection of its own: its EE primitives (the fade, the
        // HUD) are placed as a front end's frame's are (`effects::Projection::at_rest`, for the camera at rest).
        // A frame whose camera is set while program 6's inputs give no projection (its matrix has no inverse: the last frames of a
        // trip between planets and a level's first frames after its load) places them by its camera: the game puts its EE
        // primitives at the camera's own projection, the PS2's 512 x 416 frame about (2048, 2048) across the view's tangents as
        // made (`Projection::at_rest` with the camera's eye and tangents), which is where program 6's matrix puts them when it is
        // made (trip 7 to 2, field 2251: the eye's line at (2048, 2048), the tangents' corners 256 and 208 pixels off, exactly).
        // Dropped, every EE primitive of those frames was: the planet of a trip's last seconds is one, two triangles textured from
        // the frame's RAM (the player, 24 November 2026: "the planet disappears for a few seconds at the end of the loading screen
        // ship flight while the loading screen is still up").
        let own;
        let proj = match (&fx_frame, self.no_view) {
            (Ok((_, p)), false) => p,
            (Err(_), false) => {
                own = crate::effects::Projection::at_rest(cam, [cam.tan[0], cam.tan[1]]);
                &own
            }
            (_, true) => {
                own = crate::effects::Projection::at_rest(cam, [cam.tan[0] / cam.k, cam.tan[1]]);
                &own
            }
        };
        // The mobies' vertex buffers (made with room for the frame's), written on a thread of the pool while the EE's primitives
        // are made (level 9's ground: 95,000 vertices, some 7 MB a frame through `Queue::write_buffer`, 1.2 ms on this thread).
        let fs = &mut self.sets[fset];
        let d = &self.device;
        let moby_vb = fs.moby.room(d, "mobies", std::mem::size_of_val(&moby_objs[..]));
        let prev_vb = fs.moby_prev.room(d, "mobies in the frame before", std::mem::size_of_val(&moby_prev[..]));
        let lit_vb = fs.moby_lit.room(d, "mobies' lighting", std::mem::size_of_val(&moby_lit[..]));
        let lights_vb = fs.moby_lights.room(d, "actor lights", std::mem::size_of_val(&light_rows[..]));
        let moby_writes: [(&Option<wgpu::Buffer>, &[u8]); 4] = [(&moby_vb, bytemuck::cast_slice(&moby_objs)), (&prev_vb, bytemuck::cast_slice(&moby_prev)), (&lit_vb, bytemuck::cast_slice(&moby_lit)), (&lights_vb, bytemuck::cast_slice(&light_rows))];
        let pool = self.pool.clone();
        let queue = self.queue.clone();
        let mut shadows = pool.scope(|s| {
            let writes = s.spawn(|| {
                for (b, data) in moby_writes {
                    if let Some(b) = b {
                        queue.write_buffer(b, 0, data);
                    }
                }
            });
            let shadows = crate::eedraw::build(&moby_ee, &transfers, proj, &cam, &mut self.level.as_mut().expect("loaded above").1.ee_textures, core_ram, ram, &self.boot, &self.device, &self.queue, &self.mat_layout, std::mem::take(&mut self.ee_vert_spare), plane.map(|p| (&passes[..], p)), std::mem::take(&mut self.water_spare));
            if let Err(p) = writes.join() {
                std::panic::resume_unwind(p);
            }
            shadows
        });
        self.ee_spare = moby_ee;
        tm.mark("ee prims");
        let gpu = &self.level.as_ref().expect("loaded above").1;
        let terrain = if chunk == 0 { 0 } else { 1 + chunk };
        if !gpu.draws.iter().any(|d| d.terrain == terrain) {
            return Err(format!("native: level {level}: level chunk {chunk} is loaded and the scene has no terrain for it"));
        }
        let d = &self.device;
        let (w, h) = self.size;
        let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
        let joined = |what: &str, p: Box<dyn std::any::Any + Send>| format!("native: the {what} thread: {}", p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_default());
        let fs = &mut self.sets[fset];
        let view_bg = if let Some(lights) = &lights_vb {
            view_group(d, &self.view_layout, &pic.view, lights)
        } else {
            pic.view_bg.clone()
        };
        self.moby_lit_spare = moby_lit;
        self.light_spare = light_rows;
        self.moby_spare = moby_objs;
        self.moby_prev_spare = moby_prev;
        let shadow_vb = fs.ee.put(d, &self.queue, "EE steps", bytemuck::cast_slice(&shadows.vertices));
        self.ee_vert_spare = std::mem::take(&mut shadows.vertices);
        let fx_vb = fs.fx.put(d, &self.queue, "effects", bytemuck::cast_slice(&fx_objs));
        // Where each effect vertex was the frame before, and lighting words of none: the effects are drawn as the mobies are
        // (`vs_moby`), on their way between two frames in the pictures between them, and give DLSS their own motion.
        let fx_prev_vb = fs.fx_prev.put(d, &self.queue, "effects, the frame before", bytemuck::cast_slice(&fx_prev));
        self.fx_lit_spare.clear();
        self.fx_lit_spare.resize(fx_objs.len(), crate::lighting::MobyLit::default());
        let fx_lit_vb = fs.fx_lit.put(d, &self.queue, "effects' lighting, none", bytemuck::cast_slice(&self.fx_lit_spare));
        self.fx_prev_spare = fx_prev;
        // The water's triangles and the reflection they draw with; the picture is mirrored once, at the first water pass's end.
        let water_vb = fs.water.put(d, &self.queue, "water reflections", bytemuck::cast_slice(&shadows.water));
        self.water_spare = std::mem::take(&mut shadows.water);
        // With DLSS (`dlss.rs`) the world is drawn at its render size into the scene's own targets and made the picture by DLSS
        // before what the EE's packets draw after the effect pass (`replay::Cmd::Upscale`): what the world's passes draw into and
        // read is at that size, every frame with it on (a space scene's mobies, a pause's frame too: the surface buffers and the
        // scene's targets are at that size).
        // A 2D step a moby run into the main frame comes after is drawn under that run, in the world, as the game orders them: the
        // vendors' pages draw their panel and picture box, then the item's 3D model (the ship, the armor) over them. Through DLSS
        // such a page went with the world, upscaled and jittered: every letter and edge of it shook (the player, 23 November
        // 2026: "a weird light border around the screen and all the ui is vibrating while in the armour vender"). Such a frame is
        // drawn without DLSS, at the picture's size, as with DLSS off; the next frame with it starts its history again.
        let on_screen = |step: &crate::eedraw::Step| matches!(step, crate::eedraw::Step::Draw { screen: true, target, .. } if *target == crate::eedraw::MAIN);
        let last_main_moby = moby_draws.iter().rposition(|d| d.2 == crate::eedraw::MAIN);
        let under_model = last_main_moby.is_some_and(|l| shadows.steps.iter().any(|(at, step)| *at <= l && on_screen(step)));
        let upscaler = self.upscaler.clone().filter(|_| !under_model);
        let raster = if under_model && pic.raster.is_some() { Some(raster_at(&mut self.raster, &self.device, self.size)) } else { pic.raster.clone() };
        let scene_size = upscaler.as_ref().map_or((w, h), |u| u.render);
        // The mobies' opaque draws for their motion (`objects::Pipelines::draw_motion`), recorded beside the picture's.
        let mut motion = crate::replay::Rec::default();
        let water_bg = plane.map(|_| self.reflections.water_group(d, scene_size, reflect, jobs.field, gpu.sun));
        let mirrored = std::cell::Cell::new(false);
        tm.mark("buffers");
        // The rest of the picture after the static world, recorded (`replay::Rec`) and played into the frame's encoder below (and
        // into each picture drawn between this frame and the one before: `replay.rs`). Every other frame buffer's depth starts the
        // frame clear (each is drawn into within a frame: a cutscene's character into a monitor's texture).
        let mut rec = crate::replay::Rec::default();
        rec.cmds = std::mem::take(&mut self.rec_spare);
        rec.cmds.clear();
        rec.cmds.reserve(self.rec_cap.saturating_sub(rec.cmds.capacity()));
        for t in gpu.ee_textures.targets.values() {
            rec.target(&t.view, &t.depth, true);
        }
        // The ambient occlusion and shadows options (`ao.rs`, `shadows.rs`): the mobies' opaque draws into the depth and a mask,
        // the mobies' shadow map, then the occlusion and the key light's shade over the static world, before the mobies, effects and
        // 2D layer draw; over the mobies after their runs (`Cmd::AoMobies`, `Cmd::Shadows`). The static shadow maps are drawn here,
        // on a submission of their own, when the camera has left them.
        let (ao_level, shadow_level) = (crate::enhance::ao(), crate::enhance::shadows());
        let statics_ran = terrain_ran || ties_ran || shrubs_ran;
        let rays = shadow_level == crate::raytrace::RAY_TRACED;
        let shadow_frame = match statics_ran && shadow_level > 0 && !rays {
            true => {
                let terrain_draws = gpu.draws.iter().filter(|dr| dr.terrain == terrain).map(|dr| (&gpu.keys[dr.key as usize].0, dr.first..dr.first + dr.count)).collect();
                let statics = crate::shadows::Statics { terrain_id: terrain, terrain: (&gpu.vertices, &gpu.indices, terrain_draws), shrubs: gpu.shrubs.as_deref().map(|s| (s, &g.shrubs)), ties: gpu.ties.as_deref().map(|t| (t, &g.ties)) };
                gpu.shadows.lock().map_err(|_| "native: the level's shadows' lock is poisoned")?.frame(&self.light_shadows, d, &mut self.outbox, ram, shadow_level, cam.eye, statics)?
            }
            false => None,
        };
        // The ray tracing (`raytrace.rs`: the ray-traced shadows; `rtlight.rs`: the ray-traced lighting and reflections): the static
        // world's structures (built when first needed), the frame's mobies' (their opaque draws into the main frame, as the
        // occluders'), recorded below as the maps' shade is.
        let (rt_light, rt_reflect) = (crate::enhance::rt_lighting(), crate::enhance::rt_reflections());
        let rt_frame = match (statics_ran && (rays || rt_light > 0 || rt_reflect > 0), &self.rt) {
            (false, _) => None,
            (true, None) => return Err("native: SHADOWS RAY TRACED, RAY-TRACED LIGHTING and RAY-TRACED REFLECTIONS need a GPU with hardware ray queries (Vulkan's VK_KHR_ray_query); this one has none".into()),
            (true, Some(rt)) => {
                let world = crate::raytrace::WorldIn {
                    terrain,
                    vertices: &gpu.vertices,
                    indices: &gpu.indices,
                    draws: gpu.draws.iter().filter(|dr| dr.terrain == terrain && dr.count > 0).map(|dr| (dr.key as usize, dr.first, dr.count)).collect(),
                    keys: &gpu.keys,
                    views: &gpu.rt_views,
                    aref: AREF,
                    ties: gpu.ties.as_deref().map(|t| (t, &g.ties)),
                    shrubs: gpu.shrubs.as_deref().map(|s| (s, &g.shrubs)),
                    ties_ran,
                    shrubs_ran,
                };
                let mut runs = Vec::new();
                if let Some(mo) = gpu.mobies.as_ref() {
                    for (set, dr, fb) in &moby_draws {
                        if *fb == crate::eedraw::MAIN && matches!(dr.key.5, crate::objects::Blend::Alpha | crate::objects::Blend::Replace) && dr.count > 0 {
                            runs.push((&mo.materials[set], dr.key, dr.first, dr.count));
                        }
                    }
                }
                let mobies = crate::raytrace::MobiesIn { vertices: &self.moby_spare, now: moby_vb.as_ref(), before: prev_vb.as_ref(), view: &pic.view, runs };
                // The ray-traced lighting and reflections see the EE's layers too: its triangles drawn into the main frame in the
                // world (the water, the sea, the slime, its own triangles; not its 2D), then program 6's effects.
                let mut layers = crate::raytrace::EeIn { ee: shadow_vb.as_ref(), ee_vertices: self.ee_vert_spare.len() as u32, fx: fx_vb.as_ref(), fx_vertices: fx_objs.len() as u32, runs: Vec::new() };
                if rt_light > 0 || rt_reflect > 0 {
                    for (_, step) in &shadows.steps {
                        if let crate::eedraw::Step::Draw { draws, set, screen: false, flat: false, target: crate::eedraw::MAIN, .. } = step {
                            let materials = gpu.ee_textures.by_aref.get(set).ok_or_else(|| format!("native: EE material set {set} is not made"))?;
                            layers.runs.extend(draws.iter().filter(|dr| dr.count > 0).map(|dr| (materials, dr.key, false, dr.first, dr.count)));
                        }
                    }
                    layers.runs.extend(fx_draws.iter().filter(|dr| dr.count > 0).map(|dr| (&gpu.effects.materials, dr.key, true, dr.first, dr.count)));
                }
                Some(gpu.rt.lock().map_err(|_| "native: the level's ray tracing's lock is poisoned")?.frame(rt, d, &self.queue, &mut self.outbox, ram, world, mobies, layers, shadow_level > 0, raster.clone())?)
            }
        };
        tm.mark("ray tracing");
        if let Some(f) = &rt_frame {
            for (what, ms) in ["ray tracing: instances", "ray tracing: colours", "ray tracing: mobies", "ray tracing: EE layers"].into_iter().zip(f.times) {
                tm.note(what, ms);
            }
        }
        let ao_mask = (statics_ran && (ao_level > 0 || shadow_frame.is_some() || rt_frame.is_some())).then(|| crate::ao::mask(d, scene_size));
        if let Some(mask) = &ao_mask {
            use crate::replay::Enc;
            let surface_start = rec.cmds.len();
            rec.occluders(mask);
            if let (Some(vb), Some(pvb), Some(lvb), Some(mo)) = (&moby_vb, &prev_vb, &lit_vb, gpu.mobies.as_ref()) {
                rec.set_vertex_buffer(0, vb);
                rec.set_vertex_buffer(1, pvb);
                rec.set_vertex_buffer(2, lvb);
                let mut k = 0;
                while let Some(&(set, _, fb)) = moby_draws.get(k) {
                    run.clear();
                    run.extend(moby_draws[k..].iter().take_while(|(st, _, f)| *st == set && *f == fb).map(|(_, dr, _)| crate::objects::Draw { key: dr.key, first: dr.first, count: dr.count }));
                    k += run.len();
                    if fb == crate::eedraw::MAIN {
                        self.objects.draw_occluders(&mut rec, &mo.materials[&set], &run)?;
                    }
                }
            }
            rec.end();
            if let Some(raster) = &raster {
                let mut commands = crate::surface_buffer::capture(&rec.cmds[surface_start..]);
                commands.insert(0, crate::replay::Cmd::Group(0, view_bg.clone()));
                rec.outside(crate::replay::Cmd::Surfaces(raster.clone(), commands));
            }
            if let Some(f) = &shadow_frame {
                use crate::replay::Enc;
                rec.shadow_casters(f);
                if let (Some(vb), Some(pvb), Some(mo)) = (&moby_vb, &prev_vb, gpu.mobies.as_ref()) {
                    rec.set_vertex_buffer(0, vb);
                    rec.set_vertex_buffer(1, pvb);
                    let mut k = 0;
                    while let Some(&(set, _, fb)) = moby_draws.get(k) {
                        run.clear();
                    run.extend(moby_draws[k..].iter().take_while(|(st, _, f)| *st == set && *f == fb).map(|(_, dr, _)| crate::objects::Draw { key: dr.key, first: dr.first, count: dr.count }));
                        k += run.len();
                        if fb == crate::eedraw::MAIN {
                            self.light_shadows.draw_mobies(&mut rec, &mo.materials[&set], &run)?;
                        }
                    }
                }
                rec.end();
            }
            // The mobies' structures: built again in every picture when the ray-traced shadows trace them there; else the pictures
            // between frames trace (water, a pixel no raster covers) against the frame's own picture's, as they apply its traced
            // lighting (0.41 ms of a 3.9 ms picture between frames in the arena fight).
            if let Some(f) = &rt_frame {
                rec.outside(crate::replay::Cmd::RtCasters(f.clone(), rays));
            }
            // With the ray-traced lighting the occlusion is its (`rtlight.rs`), in place of the depth buffer's.
            if ao_level > 0 && rt_light == 0 {
                rec.outside(crate::replay::Cmd::Ao(ao_level, mask.clone()));
            }
            if let Some(f) = &shadow_frame {
                rec.outside(crate::replay::Cmd::Shadows(f.clone(), mask.clone(), false));
            }
            if let Some(f) = rt_frame.as_ref().filter(|_| rays) {
                rec.outside(crate::replay::Cmd::RtShadows(f.clone(), mask.clone(), false));
            }
            if let Some(f) = rt_frame.as_ref().filter(|_| rt_light > 0) {
                rec.outside(crate::replay::Cmd::RtLight(f.clone(), mask.clone(), rt_light, ao_level, false));
            }
            if let Some(f) = rt_frame.as_ref().filter(|_| rt_reflect > 0) {
                rec.outside(crate::replay::Cmd::RtGloss(f.clone(), rt_reflect));
            }
        }
        // One EE step (the frame's EE vertices `vb`): a shadow step or a draw into the native picture within its pass; a draw into
        // another frame buffer in a pass of its own, and a draw reading its own buffer after that buffer's copy is made (the
        // picture's pass ended for either, begun again by the next draw into it).
        let ee_step = |rec: &mut crate::replay::Rec, vb: &wgpu::Buffer, step: &crate::eedraw::Step| -> Result<(), String> {
            use crate::eedraw::Step;
            use crate::replay::{Cmd, Enc};
            let tx = &gpu.ee_textures;
            match step {
                Step::Draw { .. } => ee_draw(&self.device, &self.objects, self.size, tx, rec, vb, step)?,
                Step::Reflect(r) => {
                    let (Some(p), Some(wvb), Some(bg)) = (plane, &water_vb, &water_bg) else { return Err("native: water reflected with no reflection made".into()) };
                    if !mirrored.replace(true) {
                        rec.outside(Cmd::Mirror(p, reflect));
                        if let Some(f) = rt_frame.as_ref().filter(|_| rt_reflect > 0) {
                            rec.outside(Cmd::RtWater(f.clone(), p, reflect));
                        }
                    }
                    rec.picture_as("colour_water");
                    self.reflections.draw(rec, wvb, r.clone(), bg);
                }
                // With the shadows option a shadow volume's darkening keeps the colour (FIX 0, the counting and the alpha the game's):
                // the key light's shade darkens instead, never twice.
                Step::Darken(r, _) if shadow_frame.is_some() || (rays && rt_frame.is_some()) => {
                    rec.picture_as("colour_ee");
                    rec.set_vertex_buffer(0, vb);
                    self.shadow_pipes.draw(rec, &Step::Darken(r.clone(), 0))?;
                }
                step => {
                    rec.picture_as("colour_ee");
                    rec.set_vertex_buffer(0, vb);
                    self.shadow_pipes.draw(rec, step)?;
                }
            }
            Ok(())
        };
        // Then the mobies, in the chain's order, each run with its TEST_1's materials, and the EE's steps where the jobs draw them.
        // Only screen-coordinate overlays belong on the upscaled picture. A world-space GS blend can also
        // read the framebuffer. Its copy must see the colour and alpha left by the preceding world draw. (A frame with one a moby
        // run into the main frame comes after has no DLSS: `under_model`.)
        let mut on_picture: Vec<&crate::eedraw::Step> = Vec::new();
        // The EE's steps (shadows, its own triangles) consumed so far.
        let mut s = 0;
        let mut k = 0;
        loop {
            while let Some((at, step)) = shadows.steps.get(s) {
                if *at != k {
                    break;
                }
                if let Some(vb) = shadow_vb.as_ref().filter(|_| !crate::surfaces::on()) {
                    if upscaler.is_some() && on_screen(step) {
                        // With DLSS the moby chain's steps are the world's, drawn into the scene at its render size: a 2D draw
                        // there (the HUD and the boxes the game draws after the last moby run and before the effect pass) went
                        // through DLSS, upscaled and carried by the camera's motion under it, blurred and shaking while the
                        // camera moved (the player, 18 November 2026). It is drawn on DLSS's picture instead, first of what
                        // comes after DLSS.
                        on_picture.push(step);
                    } else {
                        ee_step(&mut rec, vb, step)?;
                    }
                }
                s += 1;
            }
            let Some(&(set, _, fb)) = moby_draws.get(k) else { break };
            let mo = gpu.mobies.as_ref().expect("moby draws come from the level's mobies");
            // The draws up to the next shadow step (the walk starts a new draw after each step, so `k` meets it), into one frame
            // buffer.
            let next_step = shadows.steps.get(s).map_or(usize::MAX, |(at, _)| *at);
            run.clear();
            run.extend(moby_draws[k..].iter().enumerate().take_while(|(j, (st, _, f))| *st == set && *f == fb && k + j < next_step).map(|(_, (_, dr, _))| crate::objects::Draw { key: dr.key, first: dr.first, count: dr.count }));
            k += run.len();
            if let (Some(vb), Some(pvb), Some(lvb)) = (&moby_vb, &prev_vb, &lit_vb) {
                use crate::replay::Enc;
                if fb == crate::eedraw::MAIN {
                    rec.picture_as("colour_mobies");
                    rec.set_vertex_buffer(0, vb);
                    rec.set_vertex_buffer(1, pvb);
                    rec.set_vertex_buffer(2, lvb);
                    self.objects.draw_moby(&mut rec, &mo.materials[&set], &run)?;
                    if upscaler.is_some() {
                        motion.set_vertex_buffer(0, vb);
                        motion.set_vertex_buffer(1, pvb);
                        motion.set_vertex_buffer(2, lvb);
                        self.objects.draw_motion(&mut motion, &mo.materials[&set], &run)?;
                    }
                } else {
                    // Another frame buffer: a pass of its own, the picture's begun again by the next draw into it.
                    let t = &gpu.ee_textures.targets[&fb];
                    rec.target(&t.view, &t.depth, false);
                    rec.set_vertex_buffer(0, vb);
                    self.objects.draw_clip(&mut rec, &mo.materials[&set], &run)?;
                    rec.end();
                }
            }
        }
        // The mobies' own ambient occlusion and shade.
        if let Some(mask) = &ao_mask {
            if ao_level > 0 && rt_light == 0 {
                rec.outside(crate::replay::Cmd::AoMobies);
            }
            if let Some(f) = &shadow_frame {
                rec.outside(crate::replay::Cmd::Shadows(f.clone(), mask.clone(), true));
            }
            if let Some(f) = rt_frame.as_ref().filter(|_| rays) {
                rec.outside(crate::replay::Cmd::RtShadows(f.clone(), mask.clone(), true));
            }
            if let Some(f) = rt_frame.as_ref().filter(|_| rt_light > 0) {
                rec.outside(crate::replay::Cmd::RtLight(f.clone(), mask.clone(), rt_light, ao_level, true));
            }
        }
        // Then the effects, in program 6's order (the game draws them after the mobies).
        if let (Some(vb), Some(pvb), Some(lvb)) = (fx_vb.as_ref().filter(|_| !crate::surfaces::on()), &fx_prev_vb, &fx_lit_vb) {
            use crate::replay::Enc;
            rec.picture_as("colour_effects");
            rec.set_vertex_buffer(0, vb);
            rec.set_vertex_buffer(1, pvb);
            rec.set_vertex_buffer(2, lvb);
            // As the mobies: `view.lerp.x` of the way back to the frame before (at 0, the frame's own place).
            self.objects.draw_moby(&mut rec, &gpu.effects.materials, &fx_draws)?;
            // Their motion for DLSS, over the mobies' (`objects::Pipelines::draw_fx_motion`).
            if upscaler.is_some() {
                motion.set_vertex_buffer(0, vb);
                motion.set_vertex_buffer(1, pvb);
                motion.set_vertex_buffer(2, lvb);
                self.objects.draw_fx_motion(&mut motion, &gpu.effects.materials, &fx_draws)?;
            }
        }
        // The light shafts option (`shafts.rs`): the sun's beams over the world drawn, before the glow and the 2D layer the EE's
        // packets draw after the effect pass.
        if let (l @ 1.., Some(sun)) = (crate::enhance::shafts(), gpu.sun)
            && (terrain_ran || ties_ran || shrubs_ran)
        {
            rec.outside(crate::replay::Cmd::Shafts(sun, l));
        }
        // Then what the jobs' EE packets draw after the effect pass. With DLSS: its steps in the world first (up to the first of the
        // glow's or the 2D layer's: a draw on the screen or into another frame buffer), then DLSS, then the rest onto
        // DLSS's picture at its own size.
        let late: &[(usize, crate::eedraw::Step)] = if shadow_vb.is_some() && !crate::surfaces::on() { &shadows.steps[s..] } else { &[] };
        let in_world = |step: &crate::eedraw::Step| match step {
            crate::eedraw::Step::Draw { screen, target, .. } => !*screen && *target == crate::eedraw::MAIN,
            _ => true,
        };
        let split = if upscaler.is_some() { late.iter().position(|(_, step)| !in_world(step)).unwrap_or(late.len()) } else { late.len() };
        // A water's reflection the game sends after a glow's or a 2D layer's step is drawn with the world, before DLSS: its
        // mirror and its water are made at the scene's render size (`water_bg`, `Reflections::mirror` on the scene's targets).
        // The frame stopped there before (the native picture given up for the frame's 2D layer, the GS's memory drawn as the
        // picture: the player's spot on Veldin, 18 November 2026, where the screen turned to coloured blocks every frame the
        // water and a glow were both in view). The water is a surface of the world; a glow drawn after it now lights it too.
        let order: Vec<usize> = if upscaler.is_some() {
            let late_water = |i: &usize| *i >= split && matches!(late[*i].1, crate::eedraw::Step::Reflect(_));
            (0..split).chain((split..late.len()).filter(late_water)).chain((split..late.len()).filter(|i| !late_water(i))).collect()
        } else {
            (0..late.len()).collect()
        };
        let world_steps = split + order.iter().filter(|&&i| i >= split && matches!(late[i].1, crate::eedraw::Step::Reflect(_))).count();
        for (k, &i) in order.iter().enumerate() {
            if k == world_steps && upscaler.is_some() {
                rec.outside(crate::replay::Cmd::Upscale(std::mem::take(&mut motion.cmds)));
                for step in std::mem::take(&mut on_picture) {
                    ee_step(&mut rec, shadow_vb.as_ref().expect("steps only with the EE's vertices"), step)?;
                }
            }
            ee_step(&mut rec, shadow_vb.as_ref().expect("late steps only with the EE's vertices"), &late[i].1)?;
        }
        if world_steps == late.len() && upscaler.is_some() {
            rec.outside(crate::replay::Cmd::Upscale(std::mem::take(&mut motion.cmds)));
            for step in std::mem::take(&mut on_picture) {
                ee_step(&mut rec, shadow_vb.as_ref().expect("steps only with the EE's vertices"), step)?;
            }
        }
        rec.end();
        self.rec_cap = rec.cmds.len().max(self.rec_cap - self.rec_cap / 16);
        // The picture: the native picture when the frame drew onto it (the world or the EE's packets), which the display buffer
        // then takes (as the next frame's first job copies the frame into it); else the display buffer's, as the frame left it
        // (a fade draws onto it; a frame that sends nothing leaves it as it was).
        let drew_main = terrain_ran || ties_ran || shrubs_ran || moby_draws.iter().any(|d| d.2 == crate::eedraw::MAIN) || fx_drawn > 0 || shadows.steps.iter().any(|(_, s)| matches!(s, crate::eedraw::Step::Draw { target, .. } if *target == crate::eedraw::MAIN) || !matches!(s, crate::eedraw::Step::Draw { .. }));
        let on_main = (!shadows.display || crate::surfaces::on()) && drew_main;
        let world = on_main && statics_ran;
        let deferred = self.defer_world && self.replays && world && !grab_now;
        let target = crate::replay::Target { tex: &pic.tex, depth: &pic.depth, tv: &pic.tv, dv: &pic.dv, size: self.size };
        let mut enc = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native frame") });
        // The grab: the picture shown now (the display buffer's), before this frame draws.
        if grab_now {
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: &gpu.ee_textures.targets[&crate::eedraw::DISPLAY].tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: &gpu.ee_textures.grab, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                extent,
            );
        }
        // The frame's place after the world frame before (a picture between the two, DLSS's history): a cut when it begins a shot
        // (another level or shot than the world frame before, or none before), the frame before's camera when a picture between
        // the two can be drawn (`replay::same_shot`, at most `replay::MAX_GAP` fields before: the fields between were not handed
        // over, the renderer busy).
        let before = self.last_world.filter(|(f, l, c)| *f < jobs.field && *l == level && crate::replay::same_shot(c, cam, jobs.field - *f));
        let cut = before.is_none() || (upscaler.is_some() && self.dlss_skipped);
        let (prev, from) = match before.filter(|(f, ..)| jobs.field - *f <= crate::replay::MAX_GAP) {
            Some((f, _, c)) => (Some(c), f),
            None => (None, jobs.field - 1),
        };
        // The frame's own picture drawn now: its DLSS inputs, and its view with their jitter.
        let shot = upscaler.as_ref().filter(|_| !deferred).map(|u| u.shot(d, &self.objects.motion_layout, cam, jobs.field as f64, 0.0, (jobs.field - from) as f64, cut));
        if let (Some(u), Some(sh)) = (&upscaler, &shot) {
            self.queue.write_buffer(&pic.view, 0, bytemuck::bytes_of(&view_u_at(cam, fog, 0.0, u.jitter_ndc(sh.jitter), u.lod_scale())));
        }
        let mut before_dlss = crate::replay::Chunks::default();
        let passes = self.gpu_passes.as_mut().filter(|_| !deferred && !self.defer_world).and_then(|p| p.begin(d, &self.queue, jobs.field, true, 1.0, self.size));
        if !deferred {
        crate::replay::play(&rec.cmds, &mut enc, &target, &crate::replay::Ctx { device: d, view_bg: &view_bg, shadows: &self.shadow_pipes, ao: &self.ao, light_shadows: &self.light_shadows, reflections: &self.reflections, shafts: &self.shafts, cam, fog, fresh: true, profile: passes.as_ref(), upscale: upscaler.as_deref().zip(shot.as_ref()) }, &mut before_dlss)?;
        }
        let shown = if on_main {
            if let Some(t) = gpu.ee_textures.targets.get(&crate::eedraw::DISPLAY).filter(|_| !deferred) {
                enc.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { texture: &pic.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyTextureInfo { texture: &t.tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    extent,
                );
                self.shadow_pipes.downsample(d, &mut enc, &t.tex, &gpu.ee_textures.display_gs, crate::eedraw::display_gs_map(t.size));
            }
            &pic.tex
        } else {
            &gpu.ee_textures.targets[&crate::eedraw::DISPLAY].tex
        };
        tm.mark("encode");
        let times = self.rt.as_ref().and_then(|r| r.times.as_ref());
        if let Some(t) = times {
            t.stamp(&mut enc, crate::raytrace::T_FRAME + 1);
        }
        let main_cb = enc.finish();
        tm.mark("finish");
        // The static world's bundles from their threads, recorded meanwhile.
        // The terrain's, with the sky before it (the game draws it before the world; a frame without the world, a menu screen's,
        // draws neither), from its thread.
        let (terrain_b, chunks) = match terrain_part.join().map_err(|p| joined("terrain's", p))? {
            Ok((b, n, ms)) if terrain_ran => {
                tm.note("terrain (its thread)", ms);
                // The terrain's thread drew the sky (`terrain_pass` fails without it): the one made from RAM this frame.
                let (sky_at, _, sky) = gpu.sky.as_ref().expect("the terrain's bundle draws the sky");
                sky.update(&self.queue, ram, *sky_at, fset)?;
                (Some(b), n)
            }
            Err(e) if terrain_ran => return Err(e),
            _ => (None, 0),
        };
        // The shrubs of this frame (the game's arrays in RAM), in world space, and their bundles, from their thread.
        let (shrubs_drawn, shrubs_b) = match shrubs_part.join().map_err(|p| joined("shrubs'", p))? {
            Ok(Some(sp)) if shrubs_ran => {
                self.shrub_spare = sp.room;
                tm.note("shrubs (their thread)", sp.all);
                tm.note("shrubs: indices", sp.stages[0]);
                tm.note("shrubs: upload", sp.stages[1]);
                tm.note("shrubs: pass", sp.stages[2]);
                tm.note("shrubs: material runs", sp.draws as f64);
                (sp.drawn, sp.bundles)
            }
            Err(e) if shrubs_ran => return Err(e),
            _ => (0, Vec::new()),
        };
        tm.mark("shrubs (wait)");
        // Which tie triangles the game's tie programs draw this frame (`tievu`), the ties drawn and their bundles, from their thread.
        let ((ties_drawn, ties_unsent, ties_unmatched), ties_b) = match ties_part.join().map_err(|p| joined("ties'", p))? {
            Ok(Some(tp)) => {
                self.sets[fset].tie_rooms = tp.rooms;
                if ties_ran {
                    tm.note("tie programs (their thread)", tp.programs);
                    tm.note("ties (their thread)", tp.all);
                    tm.note("ties: ranges", tp.stages[0]);
                    tm.note("ties: draws", tp.stages[1]);
                    tm.note("ties: vertices", tp.stages[2]);
                    // Summed over the ranges' threads.
                    tm.note("ties: decide (sum)", tp.stats.decide_ns as f64 / 1e6);
                    tm.note("ties: build (sum)", tp.stats.all_ns as f64 / 1e6);
                    tm.note("ties: instances built (sum)", tp.stats.build_ns as f64 / 1e6);
                    tm.note("ties: instances built", tp.stats.rebuilt as f64);
                    tm.note("ties: upload (sum)", tp.upload);
                    tm.note("ties: record (sum)", tp.record);
                    tm.note("ties: packets", tp.stats.packets as f64);
                    tm.note("ties: packets decided", tp.stats.decided as f64);
                    tm.note("ties: vertices made", tp.stats.made as f64);
                    tm.note("ties: swaying", tp.stats.sway as f64);
                    tm.note("ties: second-draw vertices", tp.stats.second as f64);
                    (tp.counts, tp.bundles)
                } else {
                    ((0, 0, 0), Vec::new())
                }
            }
            Err(e) if ties_ran => return Err(e),
            _ => ((0, 0, 0), Vec::new()),
        };
        tm.mark("ties (wait)");
        // The picture's clear, the static world's passes in the game's order (the sky and the terrain, the ties, the shrubs: each
        // its bundle executed, recorded on its thread), then the rest; the bundles kept for the frame's replay.
        let mut clear = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native clear") });
        if let Some(t) = times {
            t.stamp(&mut clear, crate::raytrace::T_FRAME);
        }
        let (mut bundles, mut surface_bundles) = (Vec::new(), Vec::new());
        let mut bundle_labels = Vec::new();
        for ((bundle, surface), label) in terrain_b.into_iter().map(|b| (b, "colour_terrain"))
            .chain(ties_b.into_iter().map(|b| (b, "colour_props")))
            .chain(shrubs_b.into_iter().map(|b| (b, "colour_vegetation"))) {
            bundles.push(bundle); surface_bundles.extend(surface); bundle_labels.push(label);
        }
        // A snapshot is drawn by the presenter alone. A frame drawn as it is prepared (one that grabs the picture shown, or draws no
        // world) leaves its commands after its shared changes; the renderer submits them itself only when no presenter draws the
        // world (with one, every submission is the presenter's, in the frames' order).
        if !deferred {
            let scene = upscaler.as_deref().map(crate::replay::scene_target);
            let world_target = scene.as_ref().unwrap_or(&target);
            let total = crate::gpu_profile::Span::begin(passes.as_ref(), &mut clear, "total");
            let colour = crate::gpu_profile::Span::begin(passes.as_ref(), &mut clear, "colour_static");
            crate::replay::statics(&mut clear, world_target, &bundles, &bundle_labels, passes.as_ref());
            colour.end(&mut clear);
            let surfaces = crate::gpu_profile::Span::begin(passes.as_ref(), &mut clear, "surface_static");
            if let Some(raster) = &raster { raster.draw(&mut clear, world_target.dv, true, &surface_bundles, &[]); }
            surfaces.end(&mut clear);
            // The last timestamp and the readback copy come after every other command of the picture.
            let mut tail = d.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("native passes times") });
            total.end(&mut tail);
            if let Some(p) = &passes { p.resolve(&mut tail); }
            self.outbox.commands.push(clear.finish());
            self.outbox.commands.extend(before_dlss.finish()?);
            self.outbox.commands.push(main_cb);
            self.outbox.commands.push(tail.finish());
        }
        if !self.defer_world {
            std::mem::take(&mut self.outbox).apply(&self.queue);
        }
        tm.mark("submit");
        if let Some(t) = times.filter(|_| !self.defer_world) {
            use crate::raytrace::*;
            let mut ran = Vec::new();
            if let Some(f) = &rt_frame {
                if f.has_mobies() {
                    ran.push((T_CASTERS, "the mobies' places and structures"));
                }
                if rays {
                    ran.extend([(T_SHADE, "the ray-traced shade over the picture"), (T_SHADE_MOBIES, "over the mobies")]);
                }
                if rt_light > 0 {
                    ran.extend([(T_LIGHT, "the ray-traced lighting over the picture"), (T_LIGHT_MOBIES, "over the mobies")]);
                }
                if rt_reflect > 0 {
                    ran.push((T_GLOSS, "the reflections on the shiny surfaces"));
                    if mirrored.get() {
                        ran.push((T_WATER, "on the water"));
                    }
                }
            }
            t.report(d, &self.queue, &ran)?;
        }
        tm.report(level);
        // The GPU passes' wait comes after the frame's own times, which it would otherwise add to their total.
        if let (Some(profiler), Some(recording)) = (self.gpu_passes.as_mut(), passes) {
            profiler.submitted(recording);
            profiler.flush(d)?;
        }
        if !self.quiet {
            println!("native: level {level}: level chunk {chunk}: {chunks} terrain chunks pass the zone test, {ties_drawn} ties drawn{}, {shrubs_drawn} shrubs drawn, {mobies_drawn} moby runs drawn{}, {fx_drawn} effects drawn{}, {} EE steps ({} shadow faces, {} triangles){}", [(ties_unsent, "not in the game's tie chain"), (ties_unmatched, "not matching the game's tie runs")].iter().filter(|(n, _)| *n > 0).map(|(n, why)| format!(", {n} {why}")).collect::<String>(), mobies_skipped.iter().map(|(why, n)| format!(", {n} not drawn: {why}")).collect::<String>(), fx_skipped.iter().map(|(why, n)| format!(", {n} not drawn: {why}")).collect::<String>(), shadows.steps.len(), shadows.faces, shadows.triangles, shadows.skipped.iter().map(|(why, n)| format!(", {n} not drawn: {why}")).collect::<String>());
        }
        // What a level's frame cannot draw is named in the log, each reason the first time (the play window's renderer is quiet).
        for why in mobies_skipped.iter().chain(&fx_skipped).chain(&shadows.skipped).map(|(w, _)| w) {
            if self.said.len() < 400 && self.said.insert(format!("{level}: {why}")) {
                println!("native: field {}: level {level}: not drawn: {why}", jobs.field);
            }
        }
        let shown = shown.clone();
        // What drawing the frame again needs, when the frame drew the world on the native picture. A picture between it and the
        // world frame before, when that one is of this level, in the same shot (`replay::same_shot`) and at most
        // `replay::MAX_GAP` fields before (the fields between were not handed over: the renderer was busy). Anything else is a
        // cut, which alone starts the ray-traced lighting's history again (`replay::Replay::cut`).
        if self.replays && world {
            let stale = std::sync::Arc::new(std::sync::Mutex::new(false));
            self.last_stale = (!deferred).then(|| stale.clone());
            self.replay = Some(crate::replay::Replay { prepared: deferred, rendered: std::sync::atomic::AtomicBool::new(!deferred), output_depth: pic.depth.clone(), output_view: pic.tv.clone(), output_depth_view: pic.dv.clone(), display: if deferred { gpu.ee_textures.targets.get(&crate::eedraw::DISPLAY).map(|t| (t.tex.clone(), gpu.ee_textures.display_gs.clone())) } else { None }, field: jobs.field, from, cut, level, size: self.size, bundles, bundle_labels, surface_bundles, raster: raster.clone(), cmds: rec.cmds, view: pic.view.clone(), view_bg, cam: *cam, prev, fog: fog_copy(fog), shadows: self.shadow_pipes.clone(), ao: self.ao.clone(), light_shadows: self.light_shadows.clone(), reflections: self.reflections.clone(), shafts: self.shafts.clone(), stale, _set: self.sets[fset].busy.clone(), upscale: upscaler.clone(), motion_layout: self.objects.motion_layout.clone() });
        } else {
            self.last_stale = None;
            rec.cmds.clear();
            self.rec_spare = rec.cmds;
        }
        self.last_world = world.then_some((jobs.field, level, *cam));
        if world {
            self.dlss_skipped = under_model;
        }
        self.fx_spare = fx_objs;
        self.fx_draw_spare = fx_draws;
        moby_draws.clear();
        self.moby_draw_spare = moby_draws;
        run.clear();
        self.run_spare = run;
        Ok(shown)
    }
}

/// An EE step's draw (`eedraw::Step::Draw`, the frame's EE vertices `vb`, with `tx`'s materials) into `rec`: into the native
/// picture within its pass; into another frame buffer in a pass of its own; a draw reading a copy of a buffer after that copy is
/// made (the picture's pass ended for either, begun again by the next draw into it). Any other step is not a draw.
fn ee_draw(device: &wgpu::Device, objects: &crate::objects::Pipelines, size: (u32, u32), tx: &crate::eedraw::EeTextures, rec: &mut crate::replay::Rec, vb: &wgpu::Buffer, step: &crate::eedraw::Step) -> Result<(), String> {
    use crate::eedraw::{Step, MAIN};
    use crate::replay::{Cmd, Enc};
    let Step::Draw { draws, set, screen, flat, target, snap, scissor } = step else { return Err("native: an EE step that is not a draw drawn as one".into()) };
    let [x0, y0, x1, y1] = *scissor;
    if snap.is_some() || *target != MAIN {
        rec.end();
    }
    if *snap == Some(crate::eedraw::MAIN_AT_GS) {
        rec.outside(Cmd::Downsample(tx.main_gs.clone(), tx.main_gs_map));
    } else if let Some(b) = snap {
        if *b == MAIN {
            rec.outside(Cmd::CopyPicture(tx.main_snap.clone(), tx.main_dst.clone()));
        } else {
            let t = &tx.targets[b];
            rec.outside(Cmd::Copy(t.tex.clone(), t.snap.clone(), t.size));
        }
    }
    // A GS blend's destination: the copy of the buffer drawn into.
    let dst = (*snap == Some(*target)).then(|| {
        let view = if *target == MAIN { tx.main_dst.create_view(&Default::default()) } else { tx.targets[target].snap.create_view(&Default::default()) };
        device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("GS blend destination"), layout: &objects.dst_layout, entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) }] })
    });
    if *target == MAIN {
        rec.picture();
        rec.set_vertex_buffer(0, vb);
        rec.set_scissor_rect([x0, y0, x1 - x0, y1 - y0])?;
        objects.draw_in(rec, &tx.by_aref[set], draws, *screen, *flat, dst.as_ref())?;
        rec.set_scissor_rect([0, 0, size.0, size.1])?;
    } else {
        let t = &tx.targets[target];
        rec.target(&t.view, &t.depth, false);
        rec.set_vertex_buffer(0, vb);
        rec.set_scissor_rect([x0, y0, x1 - x0, y1 - y0])?;
        objects.draw_in(rec, &tx.by_aref[set], draws, *screen, *flat, dst.as_ref())?;
        rec.end();
    }
    Ok(())
}

/// The frame's ties as their thread builds them (`Native::render`): the counts `Ties::frame` returns, their bundles (none: no tie
/// drawn), and the thread's times (the tie programs', all of it).
struct TiePart {
    counts: (usize, usize, usize),
    /// Its ranges' bundles, in order.
    bundles: Vec<Static>,
    programs: f64,
    /// The second parts' coordinates and the ranges built, uploaded and recorded (ms), the draws and the vertices.
    stages: [f64; 3],
    /// The ranges' builds summed (`objects::TieStats`), and their uploads' and passes' recording ms summed.
    stats: crate::objects::TieStats,
    upload: f64,
    record: f64,
    all: f64,
    /// The ranges' rooms, given back for the next frame (`FrameSet::tie_rooms`).
    rooms: Vec<crate::objects::TieRoom>,
}

/// The frame's shrubs as their thread draws them: how many, their bundles (none: no shrub drawn), the thread's time.
struct ShrubPart {
    drawn: usize,
    /// The draws, and the thread's stages' ms: the draws and the fog values (with the instances' build when the game's arrays
    /// changed), nothing (the indices' upload before), the bundles.
    draws: usize,
    stages: [f64; 3],
    /// Its bundles, in order.
    bundles: Vec<Static>,
    all: f64,
    /// The draws' and groups' room, given back for the next frame (`Native::shrub_spare`).
    room: (Vec<(usize, crate::objects::Draw)>, Vec<usize>),
}

/// A layer of the static world as its thread records it: its bundle (kept for the frame's replay, `replay.rs`) and the frame's
/// own pass of the picture executing it.
type Static = (wgpu::RenderBundle, Option<wgpu::RenderBundle>);

/// The bundles the shrubs' draws are recorded in, each on a thread (`Native::render`).
const SHRUB_THREADS: usize = 2;

/// The ranges the ties are built and recorded in, each on a thread (`Native::render`): one a `TIE_TRIANGLES` the game's listed
/// ties hold, up to `TIE_THREADS` (Veldin's and level 10's frames two, level 28's arrival all six).
const TIE_THREADS: usize = 6;
const TIE_TRIANGLES: usize = 48_000;

/// The threads `Pool` keeps: the terrain's pass, and the ties' and the shrubs' passes, each on as many threads as they split into,
/// and the mobies' buffer writes beside the EE's primitives (`render_frame`), which waited behind the ties' ranges without one.
const POOL_THREADS: usize = 1 + TIE_THREADS + SHRUB_THREADS + 1;

/// Threads kept from frame to frame for the frame's passes beside its own thread (`Native::render`), taking jobs from one queue. A
/// thread started anew each frame waited 20 to 55 ms before it first ran in some frames (level 28: the shrubs' runs started 23 to
/// 39 ms late for under a millisecond of work, the ties' thread 55.6 ms late), and the frame waited for it.
pub(crate) struct Pool {
    tx: std::sync::Mutex<std::sync::mpsc::Sender<PoolTask>>,
}

type PoolTask = Box<dyn FnOnce() + Send + 'static>;

impl Pool {
    fn new(threads: usize) -> Result<Pool, String> {
        let (tx, rx) = std::sync::mpsc::channel::<PoolTask>();
        let rx = std::sync::Arc::new(std::sync::Mutex::new(rx));
        for k in 0..threads {
            let rx = rx.clone();
            std::thread::Builder::new()
                .name(format!("uya frame {k}"))
                .spawn(move || loop {
                    // The queue's lock is held only while waiting for the next task: a thread waiting inside its task never holds
                    // back one queued after it.
                    let task = rx.lock().expect("the frame pool's queue").recv();
                    match task {
                        Ok(task) => task(),
                        Err(_) => break,
                    }
                })
                .map_err(|e| format!("native: the frame pool's thread {k}: {e}"))?;
        }
        Ok(Pool { tx: std::sync::Mutex::new(tx) })
    }

    /// Runs `f` with a scope whose jobs run on the pool's threads; every job has finished when `scope` returns, `f` panicking or not.
    pub(crate) fn scope<'env, R>(&'env self, f: impl FnOnce(&PoolScope<'env>) -> R) -> R {
        struct Wait<'a>(&'a (std::sync::Mutex<usize>, std::sync::Condvar));
        impl Drop for Wait<'_> {
            fn drop(&mut self) {
                let mut n = self.0 .0.lock().expect("the frame pool's count");
                while *n > 0 {
                    n = self.0 .1.wait(n).expect("the frame pool's count");
                }
            }
        }
        let scope = PoolScope { pool: self, pending: std::sync::Arc::new((std::sync::Mutex::new(0), std::sync::Condvar::new())), _env: std::marker::PhantomData };
        let _wait = Wait(&scope.pending);
        f(&scope)
    }
}

pub(crate) struct PoolScope<'env> {
    pool: &'env Pool,
    pending: std::sync::Arc<(std::sync::Mutex<usize>, std::sync::Condvar)>,
    _env: std::marker::PhantomData<&'env mut &'env ()>,
}

impl<'env> PoolScope<'env> {
    /// `f` on one of the pool's threads; `PoolJob::join` gives its result (or its panic).
    pub(crate) fn spawn<T: Send + 'env>(&self, f: impl FnOnce() -> T + Send + 'env) -> PoolJob<T> {
        let slot = std::sync::Arc::new((std::sync::Mutex::new(None), std::sync::Condvar::new()));
        let (done, pending) = (slot.clone(), self.pending.clone());
        *pending.0.lock().expect("the frame pool's count") += 1;
        let task: Box<dyn FnOnce() + Send + 'env> = Box::new(move || {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
            *done.0.lock().expect("a frame pool job's result") = Some(r);
            done.1.notify_all();
            *pending.0.lock().expect("the frame pool's count") -= 1;
            pending.1.notify_all();
        });
        // SAFETY: the task borrows only what lives for 'env, and `Pool::scope` returns (its `Wait` drops) only once every task
        // spawned in it has run, so no borrow outlives what it borrows.
        let task = unsafe { std::mem::transmute::<Box<dyn FnOnce() + Send + 'env>, PoolTask>(task) };
        self.pool.tx.lock().expect("the frame pool's queue").send(task).expect("the frame pool's threads stopped");
        PoolJob { slot }
    }
}

/// A job `PoolScope::spawn` started: its result once it has run.
pub(crate) struct PoolJob<T> {
    slot: std::sync::Arc<(std::sync::Mutex<Option<std::thread::Result<T>>>, std::sync::Condvar)>,
}

impl<T> PoolJob<T> {
    pub(crate) fn join(self) -> std::thread::Result<T> {
        let mut r = self.slot.0.lock().expect("a frame pool job's result");
        while r.is_none() {
            r = self.slot.1.wait(r).expect("a frame pool job's result");
        }
        r.take().expect("checked above")
    }
}

/// The frames' walks (`mobies::walk_into` for a level's frame, `mobies::screen_walk` for the front end's) and what one walk leaves
/// the next: the GS registers, and VU1 and VIF1 (the front end's, and a level's). A walk needs only the frame's jobs and the main
/// program, so `play` walks each frame on a thread of its own while the renderer draws the frame before (`Native::take_walker`):
/// the walk was a fifth of the render thread's time on level 9 (the sea's passes' 20,000 primitives).
pub(crate) struct Walker {
    boot: std::sync::Arc<uyafmt::boot::Boot>,
    /// The field last walked and the GS contexts its jobs left (`mobies::walk`'s `start` for the next field).
    gs_carry: Option<(u64, crate::mobies::GsLeft)>,
    /// VU1 and VIF1 as the front end's chains leave them, and the glyph sets' bytes (`mobies::screen_walk`).
    screen_vu: crate::mobies::ScreenVu,
    /// VU1 and VIF1 as a level's frames leave them (`mobies::walk_into`): the program resident, the runs outside the moby chain's
    /// VU1 (a space shot's stars), VIF1's registers. Kept across fields the walk skipped as across any other: every frame's jobs
    /// upload the level's programs in the same order and a shot's stars are its UNPACKs' data, so the last field walked leaves
    /// what a skipped one would have.
    level_vu: crate::mobies::LevelVu,
    glyphs: crate::prompts::Glyphs,
}

/// A frame walked (`Walker::walk`).
pub(crate) enum Walked {
    /// A level's frame, and why its moby chain could not be read when it could not (walked again without the mobies).
    Level(crate::mobies::Walk, Option<String>),
    Screen(crate::mobies::ScreenWalk),
}

impl Walker {
    /// The frame's walk, its primitives in `buf` (emptied first: the room of a frame drawn before).
    pub(crate) fn walk(&mut self, jobs: &crate::jobs::FrameJobs, front_end: bool, buf: Vec<(usize, crate::mobies::EePrim)>) -> Result<Walked, String> {
        crate::mobies::dump_field(jobs.field);
        let start = self.gs_carry.map(|(f, c)| if f + 1 == jobs.field { c } else { c.after_gap() });
        let walked = if front_end {
            let w = crate::mobies::screen_walk(jobs, start, &mut self.screen_vu, &self.glyphs);
            self.gs_carry = Some((jobs.field, w.gs_end));
            Walked::Screen(w)
        } else {
            // UYA_VU1_SCREEN_REPLAY (an instrument, for UYA_VU1_CHECK): the level's frame run again as the front end's walk runs one
            // (every VU1 program its jobs upload run on VU1 state of its own: the moby chain's runs, the shrubs' program 5, which
            // only a front end's frame runs otherwise), its output not drawn; what stops its jobs is logged once.
            if screen_replay() {
                let r = crate::mobies::screen_walk(jobs, start, &mut crate::mobies::ScreenVu::new(), &self.glyphs);
                static SEEN: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
                let mut seen = SEEN.lock().expect("the replay's log");
                for e in r.stopped {
                    if !seen.contains(&e) {
                        println!("native: UYA_VU1_SCREEN_REPLAY: field {}: {e}", jobs.field);
                        seen.push(e);
                    }
                }
            }
            // A walk that stops on its moby chain is made again without it, from the same VU1 and VIF1; one that stops then too
            // leaves them as they were.
            let kept = self.level_vu.clone();
            let (w, why) = match crate::mobies::walk_into(jobs, &self.boot, true, start, buf, &mut self.level_vu) {
                Ok(w) => (w, None),
                Err(e) => {
                    self.level_vu = kept.clone();
                    match crate::mobies::walk_into(jobs, &self.boot, false, start, Vec::new(), &mut self.level_vu) {
                        Ok(w) => (w, Some(e)),
                        Err(e) => {
                            self.level_vu = kept;
                            return Err(e);
                        }
                    }
                }
            };
            self.gs_carry = Some((jobs.field, w.gs_end));
            Walked::Level(w, why)
        };
        Ok(walked)
    }
}

/// Whether UYA_VU1_SCREEN_REPLAY is set (`Walker::walk`).
fn screen_replay() -> bool {
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("UYA_VU1_SCREEN_REPLAY").is_some())
}

/// A vertex buffer kept from frame to frame and filled with `Queue::write_buffer` (`put`), made again, with room to spare, only
/// when a frame's vertices pass its size: a vertex buffer made and filled every frame (`create_buffer_init`: wgpu clears the new
/// mapping, then the vertices are copied in) cost more than making the vertices on level 28. The write is done before the
/// frame's command buffers at the frame's submit, after every frame submitted before.
#[derive(Default)]
struct Stream(Option<wgpu::Buffer>);

impl Stream {
    /// `data` at the buffer's start, and the buffer; None for no data.
    fn put(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, label: &str, data: &[u8]) -> Option<wgpu::Buffer> {
        let b = self.room(device, label, data.len())?;
        queue.write_buffer(&b, 0, data);
        Some(b)
    }

    /// The buffer, with room for `len` bytes at its start (written by the caller); None for none.
    fn room(&mut self, device: &wgpu::Device, label: &str, len: usize) -> Option<wgpu::Buffer> {
        if len == 0 {
            return None;
        }
        let size = len as u64;
        if self.0.as_ref().is_none_or(|b| b.size() < size) {
            let cap = (size + size / 2).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT);
            // STORAGE: the ray tracing reads the mobies', the EE's and the effects' vertices (`raytrace.rs`: their places, texture
            // coordinates and colours), and builds the EE's layers from them where the GPU traces rays (BLAS_INPUT).
            let rays = if device.features().contains(wgpu::Features::EXPERIMENTAL_RAY_QUERY) { wgpu::BufferUsages::BLAS_INPUT } else { wgpu::BufferUsages::empty() };
            self.0 = Some(device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size: cap, usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::STORAGE | rays, mapped_at_creation: false }));
        }
        Some(self.0.clone().expect("made above"))
    }
}

fn view_group(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, view: &wgpu::Buffer, lights: &wgpu::Buffer) -> wgpu::BindGroup {
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("frame view and lights"), layout, entries: &[
        wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() },
        wgpu::BindGroupEntry { binding: 1, resource: lights.as_entire_binding() },
    ] })
}

/// The frame's native picture and its depth, their views, and the view's buffer and bind group (group 0 of every pass of the
/// picture).
struct Picture {
    raster: Option<std::sync::Arc<crate::surface_buffer::Buffers>>,
    tex: wgpu::Texture,
    depth: wgpu::Texture,
    tv: wgpu::TextureView,
    dv: wgpu::TextureView,
    view: wgpu::Buffer,
    view_bg: wgpu::BindGroup,
}

/// What the terrain's thread draws with (`terrain_pass`): the level's terrain buffers, draws, texture settings and chunk
/// visibility, the sky, and the terrain's pipelines (the pixels that pass the alpha test, and those that fail it).
struct TerrainScene {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    draws: std::sync::Arc<Vec<Draw>>,
    keys: std::sync::Arc<Vec<(wgpu::BindGroup, bool)>>,
    visibility: std::sync::Arc<Vec<(u8, u8)>>,
    /// The sky the game's pointer names, or why it could not be made (`Native::render`): the terrain's layer, which fails with it.
    sky: Result<std::sync::Arc<crate::sky::SkyGpu>, String>,
    pass: wgpu::RenderPipeline,
    fail: wgpu::RenderPipeline,
    /// With the lighting option: vertex buffer 1 and group 2 of `pass` and `fail` (`lighting.rs`).
    lit: Option<std::sync::Arc<crate::lighting::TerrainLit>>,
}

/// A native picture of `size` and its depth (`Native::picture`, and the pictures `play` draws between frames).
pub(crate) fn picture_textures(d: &wgpu::Device, size: (u32, u32)) -> (wgpu::Texture, wgpu::Texture) {
    (colour_texture(d, size), depth_texture(d, size))
}

/// A native picture of `size`.
fn colour_texture(d: &wgpu::Device, (w, h): (u32, u32)) -> wgpu::Texture {
    let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    d.create_texture(&wgpu::TextureDescriptor {
        label: Some("native picture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        // Read by the main frame's downsampling to the GS's size (`eedraw::Pipelines::downsample`); written by DLSS (`dlss.rs`).
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    })
}

/// A native picture's depth of `size`.
fn depth_texture(d: &wgpu::Device, (w, h): (u32, u32)) -> wgpu::Texture {
    let extent = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
    d.create_texture(&wgpu::TextureDescriptor {
        label: Some("native depth"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    })
}

/// A bundle of the static world (`replay.rs`) with the frame's view bound, recorded by `draw`, and the frame's pass of the
/// picture that executes it (recorded here, on the layer's thread).
fn picture_bundle<'a>(device: &'a wgpu::Device, _label: &'a str, pic: &'a Picture, draw: impl FnOnce(&mut crate::replay::Rec) -> Result<(), String>) -> Result<Static, String> {
    use crate::replay::Enc;
    // The commands' room is one an earlier bundle's recording left (the ties' and the terrain's are thousands).
    static ROOMS: std::sync::Mutex<Vec<Vec<crate::replay::Cmd>>> = std::sync::Mutex::new(Vec::new());
    let mut b = crate::replay::Rec::default();
    b.cmds = ROOMS.lock().expect("the bundles' rooms").pop().unwrap_or_default();
    b.set_bind_group(0, &pic.view_bg);
    draw(&mut b)?;
    let surface = pic.raster.as_ref().map(|_| crate::surface_buffer::bundle(device, &crate::surface_buffer::capture(&b.cmds), true));
    let bundle = crate::surface_buffer::bundle(device, &b.cmds, false);
    b.cmds.clear();
    let mut rooms = ROOMS.lock().expect("the bundles' rooms");
    if rooms.len() < 16 {
        rooms.push(b.cmds);
    }
    Ok((bundle, surface))
}

/// A bundle of the shrubs: the level's built shrub vertices drawn by `draw` (which binds them), with the instances' fog values
/// `fog` (group 2).
fn shrub_bundle<'a>(device: &'a wgpu::Device, pic: &'a Picture, fog: &'a wgpu::BindGroup, draw: impl FnOnce(&mut crate::replay::Rec) -> Result<(), String>) -> Result<Static, String> {
    picture_bundle(device, "native shrubs", pic, |b| {
        crate::replay::Enc::set_bind_group(b, 2, fog);
        draw(b)
    })
}

/// The terrain's bundle: the sky first (its frame set `set`'s rotations), as the game draws it before the world, then every
/// draw of terrain `terrain` whose chunk passes the zone test (its mask against the zone's byte of `zones`, the record's 0x80);
/// and the chunks drawn.
fn terrain_pass(device: &wgpu::Device, pic: &Picture, scene: &TerrainScene, terrain: u32, zones: &[u8], set: usize) -> Result<(Static, usize), String> {
    use crate::replay::Enc;
    let mut chunks = 0;
    let b = picture_bundle(device, "native terrain", pic, |p| {
        scene.sky.as_ref().map_err(|e| e.clone())?.draw(p, set);
        Enc::set_vertex_buffer(p, 0, &scene.vertices);
        if let Some(lit) = &scene.lit {
            Enc::set_vertex_buffer(p, 1, &lit.vertices);
            Enc::set_bind_group(p, 2, &lit.group);
        }
        Enc::set_index_buffer(p, &scene.indices);
        let mut last_chunk = u32::MAX;
        let mut b = crate::objects::Bound::default();
        for dr in scene.draws.iter().filter(|d| d.terrain == terrain) {
            let (mask, zone) = scene.visibility[dr.chunk as usize];
            let byte = zones.get(zone as usize).ok_or_else(|| format!("native: terrain chunk {} names zone {zone} of the record's 0x80", dr.chunk))?;
            if mask & byte == 0 {
                continue;
            }
            if dr.chunk != last_chunk {
                chunks += 1;
                last_chunk = dr.chunk;
            }
            let (bg, fails) = &scene.keys[dr.key as usize];
            b.group(p, bg);
            b.pipeline(p, &scene.pass);
            Enc::draw_indexed(p, dr.first..dr.first + dr.count);
            if *fails {
                b.pipeline(p, &scene.fail);
                Enc::draw_indexed(p, dr.first..dr.first + dr.count);
            }
        }
        Ok(())
    })?;
    Ok((b, chunks))
}

/// The frame's fog, as func_L01_003FEBC8 leaves it in the view struct (the tangents' struct, tangents - 0xB0) for the world
/// programs: FOGCOL r, g, b as words at +0x250, +0x254, +0x258; the near and far view depths times 1024 at +0x228, +0x22C; F at
/// them at +0x238, +0x23C. It copies them each frame from the level's fog (gameplay settings +0x0C on, at 0x1DFA64 on Veldin) or,
/// when the camera's +0x450 is set, from the camera's own (a level's intro). Read at the field's end, it misses a fog a level's
/// code sets for the frame's drawing alone (the jobs carry that one: `mobies::Walk::fog`).
fn fog_of(ram: &[u8], g: &Globals) -> uyafmt::gameplay::Fog {
    let vs = g.tangents as usize - 0xb0;
    let word = |o: usize| u32::from_le_bytes(ram[vs + o..vs + o + 4].try_into().expect("4 bytes"));
    uyafmt::gameplay::Fog {
        colour: [word(0x250) as u8, word(0x254) as u8, word(0x258) as u8],
        near: f32::from_bits(word(0x228)) / 1024.0,
        far: f32::from_bits(word(0x22c)) / 1024.0,
        near_f: f32::from_bits(word(0x238)),
        far_f: f32::from_bits(word(0x23c)),
    }
}

/// The surface buffers `cache` holds at `size` (the world's: DLSS's render size, or the picture's), made again when the size
/// changes.
fn raster_at(cache: &mut Option<((u32, u32), std::sync::Arc<crate::surface_buffer::Buffers>)>, device: &wgpu::Device, size: (u32, u32)) -> std::sync::Arc<crate::surface_buffer::Buffers> {
    if cache.as_ref().is_none_or(|(s, _)| *s != size) {
        *cache = Some((size, crate::surface_buffer::Buffers::new(device, size)));
    }
    cache.as_ref().expect("surface buffers allocated").1.clone()
}

/// The view's near clip as the view setup (func_L10_003E3160) scales the fog's slope by it (the view struct's +0xA0): the
/// frame's fog from the moby program's constants (`mobies::Walk::fog`).
fn near_clip_of(ram: &[u8], g: &Globals) -> f32 {
    let at = g.tangents as usize - 0xb0 + 0xa0;
    f32::from_le_bytes(ram[at..at + 4].try_into().expect("4 bytes"))
}

/// The render's time by stage, printed per frame when UYA_NATIVE_TIMES is set (an instrument; read once).
struct Times {
    on: bool,
    t: std::time::Instant,
    t0: std::time::Instant,
    stages: Vec<(&'static str, f64)>,
}

impl Times {
    fn new() -> Times {
        static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        let now = std::time::Instant::now();
        Times { on: *ON.get_or_init(|| std::env::var_os("UYA_NATIVE_TIMES").is_some()), t: now, t0: now, stages: Vec::new() }
    }
    fn mark(&mut self, stage: &'static str) {
        if self.on {
            let now = std::time::Instant::now();
            self.stages.push((stage, (now - self.t).as_secs_f64() * 1000.0));
            self.t = now;
        }
    }
    /// A time measured elsewhere (another thread's), reported with the stages.
    fn note(&mut self, what: &'static str, ms: f64) {
        if self.on {
            self.stages.push((what, ms));
        }
    }
    fn report(&self, level: u32) {
        if self.on {
            let mut s: Vec<String> = self.stages.iter().map(|(n, ms)| format!("{n} {ms:.2}")).collect();
            // The whole process's CPU time (every thread's, the game's too) and its page faults since the last report.
            static LAST: std::sync::Mutex<Option<[u64; 3]>> = std::sync::Mutex::new(None);
            let now = process_counters();
            if let Some(last) = LAST.lock().expect("process counters").replace(now) {
                s.push(format!("process: user {:.2}, process: kernel {:.2}, process: page faults {}, process: private MB {:.0}", (now[0] - last[0]) as f64 / 1e4, (now[1] - last[1]) as f64 / 1e4, (now[2] as u32).wrapping_sub(last[2] as u32), private_mb()));
            }
            println!("native times: level {level}: {:.2} ms: {}", self.t0.elapsed().as_secs_f64() * 1000.0, s.join(", "));
        }
    }
}

/// The process's private memory (its commit, MB), for the presenter's line (`play`).
pub(crate) fn private_mb() -> f64 {
    #[repr(C)]
    struct MemoryCounters {
        cb: u32,
        page_faults: u32,
        rest: [usize; 8],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn K32GetProcessMemoryInfo(p: *mut std::ffi::c_void, counters: *mut MemoryCounters, cb: u32) -> i32;
    }
    let mut m = MemoryCounters { cb: std::mem::size_of::<MemoryCounters>() as u32, page_faults: 0, rest: [0; 8] };
    // SAFETY: the struct is PROCESS_MEMORY_COUNTERS's layout; its PagefileUsage is the private commit.
    unsafe {
        assert!(K32GetProcessMemoryInfo(GetCurrentProcess(), &mut m, m.cb) != 0, "GetProcessMemoryInfo");
    }
    m.rest[6] as f64 / (1 << 20) as f64
}

/// The process's user and kernel CPU time (100 ns units) and its page faults so far (`Times::report`).
fn process_counters() -> [u64; 3] {
    #[repr(C)]
    struct MemoryCounters {
        cb: u32,
        page_faults: u32,
        rest: [usize; 8],
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn GetProcessTimes(p: *mut std::ffi::c_void, creation: *mut u64, exit: *mut u64, kernel: *mut u64, user: *mut u64) -> i32;
        fn K32GetProcessMemoryInfo(p: *mut std::ffi::c_void, counters: *mut MemoryCounters, cb: u32) -> i32;
    }
    let (mut c, mut e, mut k, mut u) = (0u64, 0u64, 0u64, 0u64);
    let mut m = MemoryCounters { cb: std::mem::size_of::<MemoryCounters>() as u32, page_faults: 0, rest: [0; 8] };
    unsafe {
        let p = GetCurrentProcess();
        assert!(GetProcessTimes(p, &mut c, &mut e, &mut k, &mut u) != 0, "GetProcessTimes");
        assert!(K32GetProcessMemoryInfo(p, &mut m, m.cb) != 0, "GetProcessMemoryInfo");
    }
    [u, k, m.page_faults as u64]
}

/// `pic` (the emulated picture) scaled to `w` x `h` (bilinear): the PS2 frame stretched to the native picture's size, as the TV
/// stretches it to the screen.
fn scaled(pic: &Image, w: usize, h: usize) -> Image {
    let mut rgb = vec![0u8; w * h * 3];
    let at = |x: usize, y: usize, c: usize| pic.rgb[(y * pic.w + x) * 3 + c] as f32;
    for y in 0..h {
        let sy = ((y as f32 + 0.5) * pic.h as f32 / h as f32 - 0.5).clamp(0.0, (pic.h - 1) as f32);
        let (y0, fy) = (sy.floor() as usize, sy.fract());
        let y1 = (y0 + 1).min(pic.h - 1);
        for x in 0..w {
            let sx = ((x as f32 + 0.5) * pic.w as f32 / w as f32 - 0.5).clamp(0.0, (pic.w - 1) as f32);
            let (x0, fx) = (sx.floor() as usize, sx.fract());
            let x1 = (x0 + 1).min(pic.w - 1);
            for c in 0..3 {
                let top = at(x0, y0, c) * (1.0 - fx) + at(x1, y0, c) * fx;
                let bot = at(x0, y1, c) * (1.0 - fx) + at(x1, y1, c) * fx;
                rgb[(y * w + x) * 3 + c] = (top * (1.0 - fy) + bot * fy).round() as u8;
            }
        }
    }
    Image { w, h, rgb }
}

/// The comparison sheet: the emulated picture stretched to the native size, the native picture, and the two blended half and half.
pub fn sheet(emulated: &Image, native: &Image) -> Image {
    let (w, h) = (native.w, native.h);
    let e = scaled(emulated, w, h);
    let mut rgb = vec![0u8; w * 3 * h * 3];
    for y in 0..h {
        for x in 0..w {
            for c in 0..3 {
                let (a, b) = (e.rgb[(y * w + x) * 3 + c], native.rgb[(y * w + x) * 3 + c]);
                let row = y * w * 3;
                rgb[(row + x) * 3 + c] = a;
                rgb[(row + w + x) * 3 + c] = b;
                rgb[(row + 2 * w + x) * 3 + c] = ((a as u16 + b as u16) / 2) as u8;
            }
        }
    }
    Image { w: w * 3, h, rgb }
}
