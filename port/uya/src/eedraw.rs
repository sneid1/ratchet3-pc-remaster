//! What the EE draws itself into the frame's jobs (`mobies::EePrim`), at the point of the jobs where it draws it: the shadows,
//! and its own triangles and sprites (weapon fire, the lights on Ratchet's pack, the cockpit glass, the glow's passes, the
//! cloud haze, the HUD, the menus, the letterbox), as a native GS would draw them:
//! - into the native picture (FRAME FBP 0xF4) or into another frame buffer (a `Target`: a texture of the GS's own size, by FBP
//!   and width, kept from frame to frame as GS memory is; its draws filter what they sample over the GS's texels, so the glow's
//!   blur passes spread as far as on the GS); a texture that is a frame buffer drawn this frame samples its target (through a
//!   copy made just before when it is the buffer drawn into, or the native picture; a draw into another frame buffer samples the
//!   native picture at the GS's size, `EeTextures::main_gs`);
//! - with textures uploaded in the jobs (core region 0x1A, the main program, a loaded HUD bank) or resident in GS memory (a HUD
//!   texture whose GS block the HUD header's copy holds, an image of the level's GS RAM image);
//! - with the GS's alpha test and blend: a blend no blend state can do (`objects::Blend::Gs`) is done by the shader on a copy
//!   of the destination.
//!
//! # The shadows
//!
//! The game's shadows are shadow volumes counted in the frame buffer's alpha (Veldin field 2400, after moby run 183):
//! 1. sprites over the whole view with ALPHA_1 (Cs - Cd) x FIX + Cd, FIX 0 (the colour stays) write alpha 0x80 where the
//!    alpha is below 0x80 (TEST_1 0x35001: the destination alpha test passes pixels whose alpha's top bit is clear; no alpha or
//!    Z test, a failing pixel writes the frame), so every pixel the scene drew with alpha up to 0x80 holds 0x80;
//! 2. with FRAME_1's mask 0xFFFFFF (only alpha is written) the volumes' faces, textured with the frame buffer itself at their own
//!    pixel (TEX0 TBP0 = FRAME FBP x 32, ST / Q the pixel), MODULATE, Z test GEQUAL: each face that is nearer than the scene sets
//!    the pixel's alpha to its alpha x the face's A / 0x80, A 0x7F or 0x82, so a face of A 0x7F takes the alpha one step down
//!    (0x80 to 0x7F, 0x7F to 0x7E ...) and one of A 0x82 one step up (0x7F to 0x80, 0x80 to 0x82 ...): the alpha is a counter
//!    whose zero is 0x80, below zero where more 0x7F faces than 0x82 faces are in front of the scene;
//! 3. sprites over the view with the destination alpha test (TEST_1 DATE, DATM 0: only pixels whose alpha is below 0x80) and
//!    ALPHA_1 (Cs - Cd) x FIX + Cd with Cs black and FIX 0x20: those pixels keep three quarters of their colour.
//!
//! Natively the counter is the stencil buffer: step 1 sets it to 0 (and raises the alpha to 0x80 where it is below, which later
//! blends with the destination alpha read; a pixel above 0x80 keeps its alpha, as the GS's destination alpha test leaves it,
//! for the glow), step 2 increments or decrements it for each face that passes the depth test, step 3 draws where its
//! top bit is set (below zero). The faces' screen positions are put back in the world with the frame's projection
//! (`effects::Projection`), the sprites' rectangles become the native picture's.
//!
//! # The EE's own triangles
//!
//! After the last moby run (Veldin field 2400: the green beam, the bolt, the light on Ratchet's pack) the chain uploads textures
//! of core region 0x1A (IMAGE transfers of a record's CLUT and texels from the region's RAM address, the core's plus the region's
//! core offset: func_L01_00428750 +0x8A4 stores it at 0x1DFA04 on Veldin) and draws triangle strips with them: TEX0 names the
//! uploaded block, so the upload's source minus the region's address is the record's texel offset (`Core::quad_textures`).
//! Each vertex is put back in the world as the shadows' are; S, T and Q (REGLIST RGBAQ's Q) as program 6's are carried (S d,
//! T d, Q d); the pixel is the objects' (MODULATE, the GS fog when PRIM FGE is set, the packet's ALPHA_1, TEST_1's alpha test as
//! a material set, Z test GEQUAL, CLAMP_1 and TEX1_1). A primitive with a state these do not cover is counted by its state.

use crate::effects::Projection;
use crate::mobies::EePrim;
use crate::native::Camera;
use crate::objects::{Blend, Draw, Key, Materials, ObjVertex, tex1_k};
use std::collections::HashMap;

/// A frame buffer's key: its FBP and its width in 64-pixel units (FBW). The glow passes draw images of several widths at one
/// FBP, one after another: each width is an image of its own.
fn buffer(fbp: u64, fbw: u64) -> u32 {
    (fbp & 0x1ff | (fbw & 0x3f) << 16) as u32
}

/// The frame buffer the world programs draw into (FRAME_1 0x800F4: FBP 0xF4, 512 pixels wide, PSMCT32, in every traced frame):
/// the native picture. Its window is 512 x 416 GS pixels (SCISSOR_1 0x19F000001FF0000).
pub(crate) const MAIN: u32 = 0xF4 | 8 << 16;
const MAIN_GS: (u32, u32) = (512, 416);

/// The display buffer (DISPFB: FBP 0, 512 pixels wide, PSMCT24): each frame's first job copies the frame drawn before into it
/// (sprites sampling the main frame), and a fade draws onto it while the frames draw no world. Natively the native picture is
/// copied into it after each frame that draws onto the main frame; a frame that draws onto it shows it (`EeDraw::display`).
/// Its window is the main frame's.
pub(crate) const DISPLAY: u32 = 8 << 16;

/// The display buffer's rows as the game samples it: the first job's copy stretches the main frame's 416 rows over 448
/// (sprites from v 0..416 to y 0..448). The native display buffer holds the picture unstretched, so a texel row v of it is the
/// picture's v / 448. The Flux Rifle's scope (flux_scope_draw) draws the display buffer from v 8 to 440 over the screen through
/// the frame's alpha; read as 416 rows its last 24 rows were past the picture (streaks) and the lens's zoom drifted off the
/// centre frame after frame (the player, 22 November 2026: "The screen is buggy when you zoom in with the flux rifle").
const DISPLAY_ROWS: f64 = 448.0;

/// The display buffer at the GS's size (`EeTextures::display_gs`): 512 x 448, the picture stretched over the rows as the game's
/// copy stretches it.
pub(crate) const DISPLAY_GS: (u32, u32) = (512, DISPLAY_ROWS as u32);

/// `Step::Draw::snap` for the main frame at the GS's size (`EeTextures::main_gs`), made just before the draw.
pub(crate) const MAIN_AT_GS: u32 = u32::MAX;

/// Another frame buffer the frame's EE packets draw into (the glow's smaller buffers, a picture rendered into a texture): a
/// texture of the GS's own size (the display buffer: the native picture's), kept from frame to frame as the GS keeps its memory,
/// with a copy that a draw sampling the buffer it draws into reads. Both are textures of the EE materials. At the GS's size its
/// draws filter what they sample over the GS's texels: the glow's passes average whole texels, so its halo spreads as on the GS
/// (level 7 field 41400: at the native picture's size the halos came out a third as wide).
pub(crate) struct Target {
    /// Its size in GS pixels (FRAME FBW x 64 by the scissor's height) and in texels.
    pub gs: (u32, u32),
    pub size: (u32, u32),
    pub tex: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub depth: wgpu::TextureView,
    pub snap: wgpu::Texture,
    index: u32,
    snap_index: u32,
}

/// The EE triangles' textures: region 0x1A's textures and a white one, with a material set per TEST_1 AREF (256: the alpha
/// test never passes).
/// The quick-select ring's R2 tab (the rotated letters on the tab at the ring's lower left, a 32 x 32 HUD texture; the ring shows
/// its second ring while R2 is held), drawn as the active device's input for R2 (`EeTextures::ring_tab`; the player, 19 November
/// 2026: "it still shows to R2 prompt").
const RING_TAB: &str = "fb793dd301dea0f9-69a9372fa06e85e0-0000155b";

pub(crate) struct EeTextures {
    base: Materials,
    pub by_aref: HashMap<u16, Materials>,
    /// Each region 0x1A texture's index by its texels' offset in the region, its CLUT's offset and its size.
    offsets: HashMap<u32, u32>,
    cluts: Vec<u32>,
    sizes: Vec<(usize, usize)>,
    white: u32,
    /// Region 0x1A's offset in the core.
    region: u32,
    /// Textures decoded from disc data the game loaded (`loaded`), by their TEX0 and uploads.
    loaded: HashMap<(u64, u32, u32), u32>,
    /// Textures decoded from the frame's RAM (`loaded`), by their TEX0 and their pixels' hash.
    ram_made: HashMap<(u64, u64), u32>,
    /// The menu's WAD entry (`uyafmt::levelcode::MenuEntry`) and its bytes from the disc, when the level has its loader; this
    /// frame's RAM address of the entry (the buffer the variable holds plus the entry's offset; None while the variable is 0).
    menu: Option<(uyafmt::levelcode::MenuEntry, Vec<u8>)>,
    menu_at: Option<u32>,
    /// The WAD streaming slots, the disc and its table of contents they read through; this frame's slots that hold an entry
    /// (its RAM address and its key: pair table, base sector, entry), and each entry's bytes as the slot holds them (unpacked
    /// when the slot unpacks), by key.
    streams: (uyafmt::levelcode::StreamSlots, String, Vec<u8>),
    streamed: Vec<(u32, (u32, u32, u32, bool))>,
    stream_bytes: HashMap<(u32, u32, u32, bool), Vec<u8>>,
    /// The other frame buffers drawn into, by `buffer`.
    pub targets: HashMap<u32, Target>,
    /// The copy of the native picture a draw sampling the main frame reads, and its texture in the materials.
    pub main_snap: wgpu::Texture,
    main_snap_index: u32,
    /// The copy a GS blend into the native picture reads at the pixel's own place (`objects` `gs_blend`): the picture drawn into,
    /// texel for texel (with DLSS, before its step, the scene at its render size in its top left), where `main_snap` is the
    /// whole picture over the texture, as a draw sampling the frame by its shares reads it.
    pub main_dst: wgpu::Texture,
    /// The main frame at the GS's size (512 x 416) a draw into another frame buffer samples, each GS pixel the average of the
    /// native picture's pixels it covers (`Pipelines::downsample`), its texture in the materials, and where GS pixel (u, v)
    /// lies in the native picture: x from map[0] x u + map[1], y from map[2] x v + map[3] (`Projection::ndc`).
    pub main_gs: wgpu::Texture,
    main_gs_index: u32,
    pub main_gs_map: [f32; 4],
    /// The display buffer at the GS's size (`DISPLAY_GS`, box-filtered from the native display buffer each time that takes a
    /// frame's picture: `Pipelines::downsample`), and its texture in the materials: what a draw sampling the display buffer
    /// reads. The Flux Rifle's scope draws the display buffer (the frame before, the scope included) a little zoomed into the
    /// frame through its alpha each frame; the GS resamples it at 512 x 448 every time, which blurs the frames fed back into a
    /// smooth radial smear. At the picture's own size every frame fed back stayed sharp, stacked copies stepping out from the
    /// middle (the player, 22 November 2026: "The screen is buggy when you zoom in with the flux rifle").
    pub display_gs: wgpu::Texture,
    display_gs_index: u32,
    /// The main frame as the game last downloaded it (GS to EE: the mission menu keeps the frame it opened over and puts it
    /// back as its background each frame), its texture in the materials, and the main frame's pages it holds (0 to 103).
    pub grab: wgpu::Texture,
    grab_index: u32,
    pub grabbed: std::collections::HashSet<u32>,
    /// The native picture's size.
    size: (u32, u32),
    /// The HUD's tables and banks; this frame's GS blocks of its textures and palettes (from the header's copy in RAM) and the
    /// banks' RAM bases; its textures made so far, by (texture, palette).
    hud: uyafmt::hud::HudData,
    hud_tex: HashMap<u32, usize>,
    hud_pal: HashMap<u32, usize>,
    hud_bases: [u32; 5],
    hud_made: HashMap<(usize, usize), u32>,
    /// The HUD textures (as `hud_made` keys them) that are the quick-select ring's R2 tab (`RING_TAB`), and the tab drawn as the
    /// active device's input for R2 (`ring_tab`): the atlas's version and the icon's cell it was made of, and its index.
    ring_tabs: std::collections::HashSet<(usize, usize)>,
    ring_tab_icon: Option<((u32, [u32; 4]), u32)>,
    /// The GS RAM image and its images; this frame's GS block of its base (the word at 0x1A1EF0 / 256); its textures made so
    /// far, by TBP and CBP.
    gs_ram: (Vec<u8>, Vec<uyafmt::hud::GsRamImage>),
    gs_base: u32,
    gs_made: HashMap<(u32, u32), u32>,
    /// GS memory as the transfers of the fields before leave it (`gsmem.rs`), cleared with the level; the textures made from it,
    /// by TEX0 and the transfers so far (`resident_texture`), and by TEX0 and their pixels' hash.
    pub gsm: crate::gsmem::GsMemory,
    gsm_cache: HashMap<(u64, u64, Option<u64>), u32>,
    gsm_made: HashMap<(u64, u64), u32>,
    /// The button prompts' icons (`prompts.rs`).
    pub prompts: crate::prompts::Prompts,
    /// The frame's `Pre`s (`prepare`), kept for their room.
    pre: Vec<Option<Pre>>,
    /// A screen frame's packets that have no EE address (`mobies::ScreenWalk::bytes`, from `mobies::FRAME_BYTES` on): a texture
    /// uploaded from them is read from them (`loaded`).
    pub frame: Vec<u8>,
}

impl EeTextures {
    /// What the level's EE draws have made so far, for `play`'s line every 300 fields: the textures and their texels, and the
    /// ones made for each content of the frame's RAM, of GS memory, of disc data, and the HUD's.
    pub fn census(&self) -> String {
        let (n, bytes) = self.base.census();
        format!(
            "{n} textures ({:.0} MB of texels): {} made from the frame's RAM, {} from GS memory ({} uploads cached), {} from disc data, {} of the HUD",
            bytes as f64 / 1048576.0,
            self.ram_made.len(),
            self.gsm_made.len(),
            self.gsm_cache.len(),
            self.loaded.len(),
            self.hud_made.len()
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue, quads: Vec<uyafmt::coremisc::QuadTexture>, region: u32, size: (u32, u32), hud: uyafmt::hud::HudData, gs_ram: (Vec<u8>, Vec<uyafmt::hud::GsRamImage>), menu: Option<(uyafmt::levelcode::MenuEntry, Vec<u8>)>, streams: (uyafmt::levelcode::StreamSlots, String, Vec<u8>)) -> Result<EeTextures, String> {
        // Palette selection is dynamic, but the bank images and palettes are already
        // present during level load. Decode only installed names matching both here.
        let mut names = crate::texnames::PalettedNames::default();
        let mut palette = |bytes: &[u8]| {
            let colours: Vec<[u8; 4]> = (0..256usize).map(|i| {
                let j = (i & !0x18) | ((i & 8) << 1) | ((i & 0x10) >> 1);
                bytes[4 * j..4 * j + 4].try_into().expect("palette colour")
            }).collect();
            names.palette(&colours);
        };
        for &(bank, offset) in &hud.palettes {
            if let Some(bytes) = hud.banks[bank].as_ref() {
                palette(bytes.get(offset..offset + 0x400).ok_or("HUD preload: palette past bank")?);
            }
        }
        for img in gs_ram.1.iter().filter(|i| i.clut) {
            palette(gs_ram.0.get(img.offset..img.offset + 0x400).ok_or("GS preload: palette past image")?);
        }
        for t in &hud.textures {
            if let Some(bytes) = hud.banks[t.bank].as_ref() {
                let (w, h) = (1 << t.lw, 1 << t.lh);
                let indices = bytes.get(t.offset..t.offset + w * h).ok_or("HUD preload: texture past bank")?;
                names.image(0x13, w, h, indices)?;
                names.image(0x1b, w, h, indices)?;
            }
        }
        for img in gs_ram.1.iter().filter(|i| !i.clut) {
            names.image(0x13, img.width, img.height, gs_ram.0.get(img.offset..img.offset + img.width * img.height).ok_or("GS preload: texture past image")?)?;
        }
        names.preload(device, queue, "HUD and resident images")?;
        let offsets = quads.iter().enumerate().map(|(i, q)| (q.texels, i as u32)).collect();
        let cluts = quads.iter().map(|q| q.clut).collect();
        // The sizes by texture index, the white texture's (after the region's) included.
        let sizes = quads.iter().map(|q| (q.texture.width, q.texture.height)).chain([(1, 1)]).collect();
        let mut textures: Vec<uyafmt::core::Texture> = quads.into_iter().map(|q| q.texture).collect();
        let white = textures.len() as u32;
        textures.push(uyafmt::core::Texture { width: 1, height: 1, levels: vec![vec![[0x80; 4]]], gs: None });
        let mut base = Materials::new(device, queue, &textures, 0.0, 0)?.pixel_q();
        // Drawn into by DLSS's scaled copy (`dlss::Upscaler::scaled_copy`).
        let main_snap = target_texture(device, "native picture copy", size, true);
        let main_dst = target_texture(device, "native picture copy for a GS blend", size, false);
        let main_snap_index = base.push_view(main_snap.create_view(&Default::default()), size, &mut []);
        let grab = target_texture(device, "grabbed frame", size, false);
        let grab_index = base.push_view(grab.create_view(&Default::default()), size, &mut []);
        let main_gs = target_texture(device, "main frame at the GS's size", MAIN_GS, true);
        let main_gs_index = base.push_view(main_gs.create_view(&Default::default()), MAIN_GS, &mut []);
        let display_gs = target_texture(device, "display buffer at the GS's size", DISPLAY_GS, true);
        let display_gs_index = base.push_view(display_gs.create_view(&Default::default()), DISPLAY_GS, &mut []);
        let mut sizes: Vec<(usize, usize)> = sizes;
        sizes.push((size.0 as usize, size.1 as usize));
        sizes.push((size.0 as usize, size.1 as usize));
        sizes.push((MAIN_GS.0 as usize, MAIN_GS.1 as usize));
        sizes.push((DISPLAY_GS.0 as usize, DISPLAY_GS.1 as usize));
        let mut t = EeTextures { base, by_aref: HashMap::new(), offsets, cluts, sizes, white, region, loaded: HashMap::new(), ram_made: HashMap::new(), menu, menu_at: None, streams, streamed: Vec::new(), stream_bytes: HashMap::new(), targets: HashMap::new(), main_snap, main_snap_index, main_dst, main_gs, main_gs_index, main_gs_map: [0.0; 4], display_gs, display_gs_index, grab, grab_index, grabbed: std::collections::HashSet::new(), size, hud, hud_tex: HashMap::new(), hud_pal: HashMap::new(), hud_bases: [0; 5], hud_made: HashMap::new(), ring_tabs: std::collections::HashSet::new(), ring_tab_icon: None, gs_ram, gs_base: 0, gs_made: HashMap::new(), gsm: crate::gsmem::GsMemory::new(), gsm_cache: HashMap::new(), gsm_made: HashMap::new(), prompts: Default::default(), frame: Vec::new(), pre: Vec::new() };
        // The display buffer, which each frame's picture is copied into.
        t.target(device, DISPLAY, MAIN_GS.1)?;
        Ok(t)
    }

    /// This frame's HUD textures and palettes in GS memory: each table entry's +4 in the header's copy (at the address the
    /// variable `header` holds), the GS block func_L01_0041D950 wrote when it uploaded the entry (0 before), and the banks' RAM
    /// bases (the header's +0x74 to +0x84, written when a bank loads).
    pub fn hud_frame(&mut self, ram: &[u8], header: u32) -> Result<(), String> {
        let rd = |a: usize| ram.get(a..a + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes"))).ok_or_else(|| format!("native: the HUD header's copy at {a:#x} past RAM"));
        let h = rd(header as usize)? as usize;
        let gs = |table: usize, i: usize| -> Result<u32, String> { Ok(rd(h + table + 8 * i + 4)? & 0xffff) };
        self.hud_tex.clear();
        self.hud_pal.clear();
        self.hud_bases = [0; 5];
        // The variable is 0 until the level's HUD loader makes the copy: the level's start clears its .bss, the variable with it,
        // as its first call (level 6's func_003D4648) and loads the HUD later (func_L01_00428750's call of the loader), and the
        // frames it sends between have no HUD texture, palette or bank in GS memory.
        if h != 0 {
            for i in 0..self.hud.textures.len() {
                match gs(self.hud.texture_table, i)? {
                    0 => {}
                    b => _ = self.hud_tex.insert(b, i),
                }
            }
            for i in 0..self.hud.palettes.len() {
                match gs(self.hud.palette_table, i)? {
                    0 => {}
                    b => _ = self.hud_pal.insert(b, i),
                }
            }
            for (k, base) in self.hud_bases.iter_mut().enumerate() {
                *base = rd(h + 0x74 + 4 * k)?;
            }
        }
        // The GS layout's CLUT base (word 0x20 of the layout at 0x1A1ED0, a main program variable func_L01_00423B48 sends the GS
        // RAM image's first loop to).
        self.gs_base = rd(0x1A_1EF0)? >> 8;
        // The menu's WAD entry in RAM.
        self.menu_at = match &self.menu {
            Some((m, _)) => match rd(m.buffer as usize)? {
                0 => None,
                b => Some(b + m.offset),
            },
            None => None,
        };
        // The streaming slots done (state 2): the entry each holds, read from the disc as the slot read it.
        self.streamed.clear();
        let (slots, iso, toc) = &self.streams;
        for k in 0..slots.count {
            let s = (slots.pool + 0x20 * k) as usize;
            let (state, read_to, unpack_to, table, base, entry) = (rd(s)?, rd(s + 0xC)?, rd(s + 0x10)?, rd(s + 0x14)?, rd(s + 0x18)?, rd(s + 0x1C)?);
            if state != 2 || table == 0 {
                continue;
            }
            let key = (table, base, entry, unpack_to != 0);
            if !self.stream_bytes.contains_key(&key) {
                let pair = table.checked_sub(0x16_0C40).ok_or_else(|| format!("native: streaming slot {k}'s pair table {table:#x} is not in the table of contents' copy"))? as usize + 8 * entry as usize;
                let u = |o: usize| toc.get(o..o + 4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes"))).ok_or_else(|| format!("native: streaming slot {k}: pair {pair:#x} past the table of contents"));
                let mut d = uyafmt::disc::Disc::open(iso)?;
                let bytes = d.sectors(base as u64 + u(pair)? as u64, u(pair + 4)? as u64)?;
                let bytes = if unpack_to != 0 { uyafmt::lz::unpack(&bytes).map_err(|e| format!("native: streaming slot {k}'s entry {entry}: {e}"))?.data } else { bytes };
                self.stream_bytes.insert(key, bytes);
            }
            self.streamed.push((if unpack_to != 0 { unpack_to } else { read_to }, key));
        }
        Ok(())
    }

    /// `n` bytes at RAM address `src` of disc data the game loaded there: the main program (the HUD's font and pictures), a
    /// HUD bank, the menu's WAD entry (the mission menu's frame), a streaming slot's entry (the selected mission's picture).
    /// The `n` bytes an upload from RAM address `src` sends: from disc data the game loaded (the main program, a HUD bank, the
    /// menu's WAD entry: the same bytes every frame, so a texture of them is made once; a streaming slot's entry), else from the
    /// frame's RAM `ram`, the memory its jobs were built from (a cutscene's stream buffer, the stars of a trip: what the DMA sent
    /// the GS; their bytes can change from frame to frame). The second value says the bytes at `src` can change from frame to
    /// frame: the frame's RAM's, and a streaming slot's entry, which the slot replaces with the next entry it reads at the same
    /// address (the ITEMS screen streams each item's picture to one buffer: level 28's 0x55B600, WAD8 entry 978 + the item's
    /// picture number).
    fn loaded<'a>(&'a self, boot: &'a uyafmt::boot::Boot, ram: &'a [u8], src: u32, n: usize) -> Result<(&'a [u8], bool), String> {
        // A screen frame's own packets (a PATH3 transfer's, a VU1 program's): their bytes can change from frame to frame.
        if let Some(at) = src.checked_sub(crate::mobies::FRAME_BYTES) {
            let at = at as usize;
            return self.frame.get(at..at + n).map(|b| (b, true)).ok_or_else(|| format!("EE triangle: {src:#x}+{n:#x} is past the frame's own packets"));
        }
        if let Ok(b) = boot.bytes(src, n) {
            return Ok((b, false));
        }
        if let Some((k, o)) = self.hud_bank_at(src, n) {
            return Ok((&self.hud.banks[k].as_deref().expect("hud_bank_at found it loaded")[o..o + n], false));
        }
        if let (Some((_, bytes)), Some(at)) = (&self.menu, self.menu_at) {
            if let Some(b) = src.checked_sub(at).and_then(|o| bytes.get(o as usize..o as usize + n)) {
                return Ok((b, false));
            }
        }
        for (at, key) in &self.streamed {
            if let Some(b) = src.checked_sub(*at).and_then(|o| self.stream_bytes[key].get(o as usize..o as usize + n)) {
                return Ok((b, true));
            }
        }
        let at = (src & 0x1ff_ffff) as usize;
        ram.get(at..at + n).map(|b| (b, true)).ok_or_else(|| format!("EE triangle: {src:#x}+{n:#x} is past the frame's RAM"))
    }

    /// The texture GS memory holds at `tex0`'s TBP (`gsm`: the transfers of this field's frames before and of the fields before):
    /// PSMCT32, or PSMT8 and PSMT8H through the CLUT (PSMCT32, CSM1, 256 colours) at CBP. Made once for each content it has.
    fn resident_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tex0: u64, tex1: u64, mip: [u64; 2], texa: Option<u64>) -> Result<u32, String> {
        if let Some(&i) = self.gsm_cache.get(&(tex0, self.gsm.sent, texa)).filter(|_| mip_levels(tex1) == 0) {
            return Ok(i);
        }
        let (levels, gs) = gs_levels(&self.gsm, tex0, tex1, mip, texa)?;
        let (w, h) = (1u32 << (tex0 >> 26 & 15), 1u32 << (tex0 >> 30 & 15));
        let hash = levels.iter().flatten().flatten().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        let i = match self.gsm_made.get(&(tex0, hash)) {
            Some(&i) => i,
            None => {
                let t = uyafmt::core::Texture { width: w as usize, height: h as usize, levels, gs };
                crate::texnames::dump_ui(&t)?;
                let EeTextures { base, by_aref, sizes, .. } = &mut *self;
                let i = base.push(device, queue, &t, &mut by_aref.values_mut().collect::<Vec<_>>())?;
                sizes.push((t.width, t.height));
                self.gsm_made.insert((tex0, hash), i);
                i
            }
        };
        if mip_levels(tex1) == 0 {
            self.gsm_cache.insert((tex0, self.gsm.sent, texa), i);
        }
        Ok(i)
    }

    /// A texture the GS samples beyond the upload the chain made at its TBP (another size or format: the fall down level 27's
    /// pipe uploads 32 x 32 PSMT8 texels and samples 64 x 64 of them with TBW 1): what GS memory holds there, as the fields
    /// before left it with this upload (and the chain's CLUT upload, if it made one) written where their BITBLTBUF and TRXPOS put
    /// them. Made once for each content it has (`gsm_made`).
    #[allow(clippy::too_many_arguments)]
    fn beyond_upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, boot: &uyafmt::boot::Boot, ram: &[u8], tex0: u64, tex1: u64, mip: [u64; 2], up: crate::mobies::Upload, clut: Option<crate::mobies::Upload>, texa: Option<u64>) -> Result<u32, String> {
        let mut g = self.gsm.clone();
        for u in std::iter::once(up).chain(clut) {
            let (bitblt, trxpos) = u.regs.ok_or_else(|| format!("EE texture TEX0 {tex0:#x}: the upload {u:x?} has no transfer registers"))?;
            let n = (u.w * u.h * crate::gsmem::bits(u.psm as u64)?).div_ceil(8) as usize;
            let (bytes, _) = self.loaded(boot, ram, u.src, n)?;
            let qws: Vec<u128> = bytes.chunks(16).map(|c| {
                let mut q = [0u8; 16];
                q[..c.len()].copy_from_slice(c);
                u128::from_le_bytes(q)
            }).collect();
            g.upload(bitblt, trxpos, u.w as u64 | (u.h as u64) << 32, &qws)?;
        }
        let (levels, gs) = gs_levels(&g, tex0, tex1, mip, texa)?;
        let hash = levels.iter().flatten().flatten().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3));
        if let Some(&i) = self.gsm_made.get(&(tex0, hash)) {
            return Ok(i);
        }
        let t = uyafmt::core::Texture { width: 1 << (tex0 >> 26 & 15), height: 1 << (tex0 >> 30 & 15), levels, gs };
        crate::texnames::dump_ui(&t)?;
        let EeTextures { base, by_aref, sizes, .. } = &mut *self;
        let i = base.push(device, queue, &t, &mut by_aref.values_mut().collect::<Vec<_>>())?;
        sizes.push((t.width, t.height));
        self.gsm_made.insert((tex0, hash), i);
        Ok(i)
    }

    /// The texture of the GS RAM image's first loop at `tex0`'s TBP (PSMT8, its CLUT there too: `core::gs_ram_texture`), None
    /// when the TBP is below the image's base or the image holds no 8-bit image of that size there with a palette at the CBP:
    /// what GS memory holds there then is what a later transfer put there (`resident_texture`: a level's map in the pause menu's
    /// MAP, sent once when a map is picked and sampled by every frame after, at offset 0x44900 of level 28's).
    fn gs_ram_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tex0: u64) -> Result<Option<u32>, String> {
        // A screen frame's textures (no level's GS RAM image).
        if self.gs_ram.1.is_empty() {
            return Ok(None);
        }
        let (tbp, psm, cbp) = ((tex0 & 0x3fff) as u32, tex0 >> 20 & 0x3f, (tex0 >> 37 & 0x3fff) as u32);
        let (Some(o), Some(c)) = (tbp.checked_sub(self.gs_base), cbp.checked_sub(self.gs_base)) else { return Ok(None) };
        if let Some(&i) = self.gs_made.get(&(tbp, cbp)) {
            return Ok(Some(i));
        }
        let (w, h) = (1usize << (tex0 >> 26 & 15), 1usize << (tex0 >> 30 & 15));
        let images = &self.gs_ram.1;
        if !images.iter().any(|g| !g.clut && g.offset == 256 * o as usize && (g.width, g.height) == (w, h)) || !images.iter().any(|g| g.clut && g.offset == 256 * c as usize) {
            return Ok(None);
        }
        if psm != 0x13 || tex0 >> 51 & 0x1f != 0 || tex0 >> 56 & 0x1f != 0 {
            return Err(format!("EE triangle: the GS RAM image's texture at block {tbp:#x} sampled with TEX0 {tex0:#x}"));
        }
        let t = uyafmt::core::gs_ram_texture(&self.gs_ram.0, &self.gs_ram.1, 256 * o as usize, 1 << (tex0 >> 26 & 15), 1 << (tex0 >> 30 & 15), 256 * c as usize).map_err(|e| format!("EE triangle: TEX0 {tex0:#x}: {e}"))?;
        let EeTextures { base, by_aref, sizes, .. } = &mut *self;
        let i = base.push(device, queue, &t, &mut by_aref.values_mut().collect::<Vec<_>>())?;
        sizes.push((t.width, t.height));
        self.gs_made.insert((tbp, cbp), i);
        Ok(Some(i))
    }

    /// The HUD texture resident at `tex0`'s TBP (PSMT8H or PSMT8: 8-bit, 2^TW x 2^TH, the texture table entry's size) through
    /// the palette at its CBP (PSMCT32, CSM1, 256 colours): the palette resident there, or the one the frame's jobs upload there
    /// from a loaded bank (`clut`'s source less the bank's RAM base). None when no HUD texture is at the TBP.
    /// The loaded HUD bank holding `n` bytes at RAM address `src`, and the offset there.
    fn hud_bank_at(&self, src: u32, n: usize) -> Option<(usize, usize)> {
        (0..5).find(|&k| self.hud_bases[k] != 0 && self.hud.banks[k].as_ref().is_some_and(|b| src >= self.hud_bases[k] && ((src - self.hud_bases[k]) as usize) + n <= b.len())).map(|k| (k, (src - self.hud_bases[k]) as usize))
    }

    /// A HUD texture the frame's jobs upload from a loaded HUD bank (`up`, PSMT8 or PSMT8H as TEX0 samples it) through the
    /// palette at CBP; None when `up` is from no bank.
    fn hud_upload(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tex0: u64, up: crate::mobies::Upload, clut: Option<crate::mobies::Upload>) -> Result<Option<u32>, String> {
        let (lw, lh) = ((tex0 >> 26 & 15) as u32, (tex0 >> 30 & 15) as u32);
        let Some((bank, offset)) = self.hud_bank_at(up.src, 1 << (lw + lh)) else { return Ok(None) };
        let ti = self.hud.textures.iter().position(|t| (t.bank, t.offset, t.lw, t.lh) == (bank, offset, lw, lh)).ok_or_else(|| format!("EE triangle: texels uploaded from HUD bank {bank} +{offset:#x}, no HUD texture of 2^{lw} x 2^{lh} there"))?;
        self.hud_decode(device, queue, ti, tex0, clut).map(Some)
    }

    fn hud_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, tex0: u64, clut: Option<crate::mobies::Upload>) -> Result<Option<u32>, String> {
        let Some(&ti) = self.hud_tex.get(&((tex0 & 0x3fff) as u32)) else { return Ok(None) };
        self.hud_decode(device, queue, ti, tex0, clut).map(Some)
    }

    /// HUD texture `ti` as `tex0` samples it, through the palette resident at its CBP or uploaded there from a loaded bank.
    fn hud_decode(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, ti: usize, tex0: u64, clut: Option<crate::mobies::Upload>) -> Result<u32, String> {
        let (psm, cbp) = (tex0 >> 20 & 0x3f, (tex0 >> 37 & 0x3fff) as u32);
        let t = self.hud.textures[ti].clone();
        if !matches!(psm, 0x13 | 0x1b) || 1 << (tex0 >> 26 & 15) != 1 << t.lw || 1 << (tex0 >> 30 & 15) != 1 << t.lh || tex0 >> 51 & 0x1f != 0 || tex0 >> 56 & 0x1f != 0 {
            return Err(format!("EE triangle: HUD texture {ti} (2^{} x 2^{}) sampled with TEX0 {tex0:#x}", t.lw, t.lh));
        }
        // The palette: (bank, offset).
        // An upload in the jobs is the palette there now; else the one resident.
        let pal = match (clut, self.hud_pal.get(&cbp)) {
            (Some(c), _) => self.hud_bank_at(c.src, 0x400).ok_or_else(|| format!("EE triangle: HUD texture {ti}: its palette is uploaded from {:#x}, in no loaded HUD bank", c.src))?,
            (None, Some(&pi)) => self.hud.palettes[pi],
            (None, None) => return Err(format!("EE triangle: HUD texture {ti}: no palette at GS block {cbp:#x}")),
        };
        let key = (ti, pal.0 << 28 | pal.1);
        if self.ring_tabs.contains(&key) {
            if let Some(i) = self.ring_tab(device, queue) {
                return Ok(i);
            }
        }
        if let Some(&i) = self.hud_made.get(&key) {
            return Ok(i);
        }
        let bank = |k: usize| self.hud.banks[k].as_deref().ok_or_else(|| format!("EE triangle: HUD bank {k} is empty"));
        let n = 1usize << (t.lw + t.lh);
        let texels = bank(t.bank)?.get(t.offset..t.offset + n).ok_or("EE triangle: HUD texels past their bank")?;
        let p = bank(pal.0)?.get(pal.1..pal.1 + 0x400).ok_or("EE triangle: HUD palette past its bank")?;
        // CSM1: the palette's index bits 3 and 4 swapped. The palette in index order is the texture's GS form with its indices
        // (`texnames::name`: the PCSX2 name a pack replacement or a remaster's UI art is found by).
        let clut: Vec<[u8; 4]> = (0..256)
            .map(|x: usize| {
                let j = (x & !0x18) | ((x & 8) << 1) | ((x & 0x10) >> 1);
                [p[4 * j], p[4 * j + 1], p[4 * j + 2], p[4 * j + 3]]
            })
            .collect();
        let px: Vec<[u8; 4]> = texels.iter().map(|&x| clut[x as usize]).collect();
        let gs = uyafmt::core::GsLevel { psm: psm as u32, texels: texels.to_vec(), clut };
        let tx = uyafmt::core::Texture { width: 1 << t.lw, height: 1 << t.lh, levels: vec![px], gs: Some(gs) };
        crate::texnames::dump_ui(&tx)?;
        let EeTextures { base, by_aref, sizes, .. } = &mut *self;
        let i = base.push(device, queue, &tx, &mut by_aref.values_mut().collect::<Vec<_>>())?;
        sizes.push((tx.width, tx.height));
        self.hud_made.insert(key, i);
        if crate::texnames::name(&tx)?.is_some_and(|n| n.stem() == RING_TAB) {
            self.ring_tabs.insert(key);
            if let Some(t) = self.ring_tab(device, queue) {
                return Ok(t);
            }
        }
        Ok(i)
    }

    /// The quick-select ring's R2 tab as the active device shows R2 (`prompts::Atlas::icon`: with the keyboard and mouse the key
    /// bound to the action on R2, CROUCH; on an Xbox controller its button), or None where the game's own "R2" stays (a
    /// PlayStation controller, no device look). The icon, its shape kept, fills the middle of a texture four times the tab's 32
    /// texels a side, counted at 32 (the sprite's coordinates are the tab's texels: `push_sharp`'s way).
    fn ring_tab(&mut self, device: &wgpu::Device, queue: &wgpu::Queue) -> Option<u32> {
        let look = self.prompts.look.clone()?;
        let cell = look.atlas.icon(look.device, &look.bindings, crate::prompts::Button::R2)?;
        let made = (look.atlas.version, cell);
        if let Some((m, i)) = self.ring_tab_icon {
            if m == made {
                return Some(i);
            }
        }
        const SIDE: usize = 128;
        const FILL: f32 = 0.8;
        let [cx, cy, cw, ch] = cell;
        let scale = (SIDE as f32 * FILL) / cw.max(ch) as f32;
        let (w, h) = ((cw as f32 * scale).round() as usize, (ch as f32 * scale).round() as usize);
        let (x0, y0) = ((SIDE - w) / 2, (SIDE - h) / 2);
        let atlas = &look.atlas.texture;
        let mut px = vec![[0u8; 4]; SIDE * SIDE];
        for y in 0..h {
            for x in 0..w {
                // The icon's texel under the pixel's middle.
                let (u, v) = (cx as usize + ((x as f32 + 0.5) / scale) as usize, cy as usize + ((y as f32 + 0.5) / scale) as usize);
                px[(y0 + y) * SIDE + x0 + x] = atlas.levels[0][v.min(atlas.height - 1) * atlas.width + u.min(atlas.width - 1)];
            }
        }
        let big = uyafmt::core::Texture { width: SIDE, height: SIDE, levels: vec![px], gs: None };
        let view = view(device, queue, &big);
        let EeTextures { base, by_aref, sizes, .. } = &mut *self;
        sizes.push((32, 32));
        let i = base.push_view(view, (32, 32), &mut by_aref.values_mut().collect::<Vec<_>>());
        self.ring_tab_icon = Some((made, i));
        Some(i)
    }

    /// A texture made from `t` among the EE textures (`prompts.rs`'s icons).
    pub(crate) fn push_texture(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> u32 {
        let EeTextures { base, by_aref, sizes, .. } = &mut *self;
        sizes.push((t.width, t.height));
        base.push_made(device, queue, t, &mut by_aref.values_mut().collect::<Vec<_>>())
    }

    /// `t` (a HUD font or glyph sheet) made at four times its resolution (`sharp`), counted at its own size (the GS's
    /// coordinates and REGION_CLAMP are in its texels).
    fn push_sharp(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> u32 {
        let big = sharp(t, Some((0.2, 0.8)));
        let view = view(device, queue, &big);
        let EeTextures { base, by_aref, sizes, .. } = &mut *self;
        sizes.push((t.width, t.height));
        base.push_view(view, (t.width as u32, t.height as u32), &mut by_aref.values_mut().collect::<Vec<_>>())
    }

    /// The target `key` (`buffer`: made when first drawn into, FBW x 64 GS pixels wide and `rows` high), and its textures.
    pub fn target(&mut self, device: &wgpu::Device, key: u32, rows: u32) -> Result<&Target, String> {
        // The display buffer's window is the main frame's (its last 32 rows the TV shows below the picture).
        let rows = if key == DISPLAY { MAIN_GS.1 } else { rows };
        let (fbp, gs) = (key & 0x1ff, ((key >> 16) * 64, rows));
        if let Some(t) = self.targets.get(&key) {
            if t.gs != gs {
                return Err(format!("EE target FBP {fbp:#x}: {} x {} GS pixels, drawn before as {} x {}", gs.0, gs.1, t.gs.0, t.gs.1));
            }
            return Ok(&self.targets[&key]);
        }
        // The GS's own size; the display buffer, which shows the native picture, the native picture's.
        let size = if key == DISPLAY { ((gs.0 * self.size.0).div_ceil(MAIN_GS.0), (gs.1 * self.size.1).div_ceil(MAIN_GS.1)) } else { gs };
        if crate::mobies::eedump() {
            eprintln!("target {key:#x}: GS {gs:?}, native {size:?}");
        }
        let tex = target_texture(device, "EE target", size, true);
        let snap = target_texture(device, "EE target copy", size, false);
        let depth = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("EE target depth"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::native::DEPTH,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let EeTextures { base, by_aref, sizes, .. } = &mut *self;
        let mut others: Vec<&mut Materials> = by_aref.values_mut().collect();
        let index = base.push_view(tex.create_view(&Default::default()), size, &mut others);
        let snap_index = base.push_view(snap.create_view(&Default::default()), size, &mut others);
        sizes.push((size.0 as usize, size.1 as usize));
        sizes.push((size.0 as usize, size.1 as usize));
        let view = tex.create_view(&Default::default());
        self.targets.insert(key, Target { gs, size, tex, view, depth, snap, index, snap_index });
        Ok(&self.targets[&key])
    }
}

/// A texture a frame buffer is drawn into (`render`) or copied into, of the native picture's format.
fn target_texture(device: &wgpu::Device, label: &str, size: (u32, u32), render: bool) -> wgpu::Texture {
    let usage = if render { wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST } else { wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST };
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage,
        view_formats: &[],
    })
}

/// One EE triangle (or sprite, two triangles) as the objects draw it: its texture setting, its material set (TEST_1 AREF, 256
/// never), whether it is drawn in the picture's own coordinates (2D: the Z test off or ALWAYS, into the main frame) and its
/// vertices.
/// A texture the chain uploaded from the main program, as the GS samples it with `tex0`: 2^TW x 2^TH texels of PSMT4 (a CLUT of
/// 16 PSMCT32 entries, 8 x 2) or PSMT8 (256 entries, 16 x 16, in a CSM1 read's order), uploaded with the same format and size,
/// CLUT offset 0; each texel its palette colour, alpha 0x80 = 1.0.
/// The mip levels beyond the first that TEX1 `tex1` samples: MXL, 0 when its MMIN reads level 0 alone (NEAREST or LINEAR).
fn mip_levels(tex1: u64) -> u64 {
    if tex1 >> 6 & 7 < 2 { 0 } else { tex1 >> 2 & 7 }
}

/// The texture TEX0 `tex0` samples in GS memory `g` as the GS reads it, level 0 and, when TEX1 `tex1`'s MMIN takes mip levels,
/// levels 1 to MXL from MIPTBP1 and MIPTBP2 (`mip`: each level's base and width, its size half the level's before, the CLUT
/// TEX0's): the lights of the fall down level 27's pipe sample their level 2 or 3, white, where level 0's texels about the
/// light are another texture's. MTBA (the bases worked out by the GS) is named, not read.
fn gs_levels(g: &crate::gsmem::GsMemory, tex0: u64, tex1: u64, mip: [u64; 2], texa: Option<u64>) -> Result<(Vec<Vec<[u8; 4]>>, Option<uyafmt::core::GsLevel>), String> {
    let (px, gs) = g.texture(tex0, texa)?;
    let n = mip_levels(tex1);
    if n > 0 && tex1 >> 9 & 1 == 1 {
        return Err(format!("EE texture TEX0 {tex0:#x}: TEX1 {tex1:#x} has its mip bases worked out by the GS (MTBA)"));
    }
    let mut levels = vec![px];
    let (tw, th) = (tex0 >> 26 & 15, tex0 >> 30 & 15);
    for k in 1..=n.min(6) {
        if k > tw || k > th {
            break;
        }
        // Level k's base and width: MIPTBP1's (levels 1 to 3) or MIPTBP2's (4 to 6), 20 bits each.
        let word = mip[((k - 1) / 3) as usize] >> (20 * ((k - 1) % 3));
        let (tbp, tbw) = (word & 0x3fff, word >> 14 & 0x3f);
        let tex0_k = tex0 & !(0x3fff | 0x3f << 14 | 0xff << 26) | tbp | tbw << 14 | (tw - k) << 26 | (th - k) << 30;
        levels.push(g.texture(tex0_k, texa)?.0);
    }
    Ok((levels, gs))
}

/// A PSMT4 or PSMT8 texture uploaded (`up`) with its PSMCT32 CLUT (CSM1, CSA 0) from disc data the game loaded (`bytes`).
fn loaded_texture<'a>(bytes: impl Fn(u32, usize) -> Result<&'a [u8], String>, tex0: u64, up: crate::mobies::Upload, clut: Option<crate::mobies::Upload>, resident: Option<&[[u8; 4]]>, texa: Option<u64>) -> Result<uyafmt::core::Texture, String> {
    let (psm, tw, th, cpsm, csm) = (tex0 >> 20 & 0x3f, 1u32 << (tex0 >> 26 & 15), 1u32 << (tex0 >> 30 & 15), tex0 >> 51 & 15, tex0 >> 55 & 1);
    let unfit = || format!("EE texture TEX0 {tex0:#x} from uploads {up:x?}, CLUT {clut:x?}");
    if up.psm as u64 != psm || up.w != tw || up.h != th {
        return Err(unfit());
    }
    let n = (tw * th) as usize;
    let b = crate::gsmem::bits(psm)? as usize;
    // An upload's data, texel after texel, each its format's bits (PSMCT24: three bytes, the 4-bit formats the low nibble first).
    let values = |data: &[u8], b: usize, n: usize| -> Vec<u32> {
        (0..n)
            .map(|i| match b {
                32 => u32::from_le_bytes([data[4 * i], data[4 * i + 1], data[4 * i + 2], data[4 * i + 3]]),
                24 => u32::from_le_bytes([data[3 * i], data[3 * i + 1], data[3 * i + 2], 0]),
                16 => u16::from_le_bytes([data[2 * i], data[2 * i + 1]]) as u32,
                8 => data[i] as u32,
                _ => (data[i / 2] >> (4 * (i % 2)) & 15) as u32,
            })
            .collect()
    };
    let texels = values(bytes(up.src, (n * b).div_ceil(8))?, b, n);
    let (px, gs): (Vec<[u8; 4]>, _) = match b {
        8 | 4 => {
            let entries = if b == 8 { 256 } else { 16 };
            // The CLUT uploaded in the jobs (CPSM's colours, CSM1: a 256-colour one's entry i at the upload's texel i with bits 3
            // and 4 swapped), else the one GS memory holds.
            let colours: Vec<[u8; 4]> = match (clut, resident) {
                (Some(c), _) => {
                    if c.psm as u64 != cpsm || (c.w * c.h) as usize != entries || csm != 0 {
                        return Err(unfit());
                    }
                    let cb = crate::gsmem::bits(cpsm)? as usize;
                    let pal = values(bytes(c.src, entries * cb / 8)?, cb, entries);
                    (0..entries).map(|i| crate::gsmem::colour(cpsm, pal[if entries == 256 { (i & !0x18) | (i & 8) << 1 | (i & 0x10) >> 1 } else { i }], texa)).collect::<Result<_, _>>()?
                }
                (None, Some(r)) => r.to_vec(),
                (None, None) => return Err(unfit()),
            };
            let px = texels.iter().map(|&v| colours[v as usize]).collect();
            // An 8-bit texture with a PSMCT32 CLUT: with the indices, its GS form, which names it (`texnames::name`) for a pack
            // replacement or a remaster's UI art (a 4-bit one has no PCSX2 name the port makes).
            (px, (b == 8 && cpsm == 0).then(|| uyafmt::core::GsLevel { psm: psm as u32, texels: texels.iter().map(|&v| v as u8).collect(), clut: colours }))
        }
        _ => (texels.iter().map(|&v| crate::gsmem::colour(psm, v, texa)).collect::<Result<_, _>>()?, None),
    };
    Ok(uyafmt::core::Texture { width: tw as usize, height: th as usize, levels: vec![px], gs })
}

/// What a primitive's GS state gives its triangles (`tri_state`): the same for every primitive of that state and of the same
/// `screen`, so `build` keeps the last one (the sea's 20,000 triangles a frame come in a few runs of one state).
#[derive(Clone, Copy)]
struct TriState {
    key: Key,
    set: u16,
    screen: bool,
    /// A triangle in the world with a vertex the world cannot hold (`triangle`): drawn at its GS positions with the GS's depth.
    flat: bool,
    fbp: u32,
    snap: Option<u32>,
    scissor: [u32; 4],
    size: (f32, f32),
    map: Option<([f64; 2], [f64; 2])>,
    decal: bool,
    window: Option<((u32, u32), f64, f64)>,
    /// A 2D primitive of the main frame drawn across the whole wide picture (PLAN 4.2: one over the frame's whole width, or
    /// sampling a frame buffer); every other one is drawn at the original picture's scale about the centre (`unstretched`).
    wide: bool,
}

/// The GS state words `TriState` comes from, and the primitive's `screen` and `flat`.
type StateKey = ((u64, u64, u64, u64, u64, u64), (u64, u64, u64, u64, Option<crate::mobies::Upload>, Option<crate::mobies::Upload>, (bool, bool)));

fn state_key(p: &EePrim, screen: (bool, bool)) -> StateKey {
    ((p.prim, p.frame, p.test, p.alpha, p.tex0, p.clamp), (p.tex1, p.xyoffset, p.scissor, p.xyoffset1, p.texels, p.clut, screen))
}

/// Whether `p` with `screen` and `full` has the state `memo` was made for (`state_key`'s comparison, field by field where the
/// primitive is, so that the sea's 20,000 primitives a frame build no key to compare). The words that tell most often come first.
fn same_state(memo: &((StateKey, bool), TriState), p: &EePrim, screen: (bool, bool), full: bool) -> bool {
    let (((prim, frame, test, alpha, tex0, clamp), (tex1, xyoffset, scissor, xyoffset1, texels, clut, s)), f) = &memo.0;
    *prim == p.prim && *tex0 == p.tex0 && *alpha == p.alpha && *test == p.test && *frame == p.frame && *clamp == p.clamp && *tex1 == p.tex1 && *xyoffset == p.xyoffset
        && *scissor == p.scissor && *xyoffset1 == p.xyoffset1 && *texels == p.texels && *clut == p.clut && *s == screen && *f == full
}

/// A primitive `build` reads: one of the frame's, where it is, or one made for it (a prompt's plate, a line's sprite), boxed so
/// that the primitives pass through `build`'s iterators as a pointer (an `EePrim` is some 330 bytes, and a frame has 20,000).
enum Held<'a> {
    Frame(&'a EePrim),
    Made(Box<EePrim>),
}

impl std::ops::Deref for Held<'_> {
    type Target = EePrim;
    fn deref(&self) -> &EePrim {
        match self {
            Held::Frame(p) => p,
            Held::Made(p) => p,
        }
    }
}

/// A primitive's triangles (`build`): its GS state's `TriState` (from `memo` when the last primitive's state was the same) and
/// its vertices. The state is left in `memo`, or in `scratch` for a primitive drawn as another (`prompts::swap`: true).
#[allow(clippy::too_many_arguments)]
fn triangle(p: &EePrim, pre: Option<&Pre>, wide_row: bool, memo: &mut Option<((StateKey, bool), TriState)>, scratch: &mut Option<TriState>, worlds: &mut Worlds, proj: &Projection, cam: &Camera, tex: &mut EeTextures, core_ram: u32, ram: &[u8], boot: &uyafmt::boot::Boot, device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, verts: &mut TriVerts) -> Result<bool, String> {
    // A PS2 button glyph or a HUD font's letter drawn as the button of the device being played with or in the vector font
    // (`prompts::swap`, which gives nothing for any other primitive).
    let swapped = crate::prompts::swap(p, tex, device, queue);
    // What `prepare` made of the frame's primitive, unless it is drawn as another.
    let pre = pre.filter(|_| swapped.is_none());
    let p = swapped.as_ref().map_or(p, |(q, _)| q);
    let ztst = p.test >> 17 & 3;
    // 2D: no Z test (or ALWAYS), or Z test GEQUAL with every vertex nearer than the native picture's near plane (the GS Z of
    // the cutscene's letterbox bars, 0xFFFFF3, is nearer than anything the frame holds: it passes everywhere). The main frame
    // takes 3D primitives too (Z test GEQUAL), another frame buffer 2D ones: a PSMCT32 buffer, every bit written.
    // Each vertex's world position and depth, made once for the tests below and the vertices (`tri_verts`).
    let mut known: [Option<([f32; 3], f32)>; 3] = [None; 3];
    // In the world, a vertex nearer than the native picture's near plane is one behind the eye: program 0's variant B (no clipping)
    // gives one to a moby the camera is inside, its GS Z wrapped to the Z24 buffer's largest (the fall down level 27's pipe, the
    // tube's nearest rings: Q below 0, Z 0xFFFFE2). The GS draws the triangle at its screen positions with that Z (the emulated
    // PS2's picture: the tube whole); put back in the world it would be cut at the near plane and its texture stretched across
    // the picture. Drawn as the GS draws it (`vs_flat`).
    let (screen, flat) = match pre {
        Some(pr) => {
            if !pr.screen {
                known = pr.known.map(Some);
            }
            (pr.screen, pr.flat)
        }
        None => {
            let mut at = |i: usize, v: &crate::effects::GsVertex| *known[i].get_or_insert_with(|| worlds.get(proj, cam, v.x, v.y, v.z));
            let nearest = ztst == 2 && p.verts.iter().enumerate().all(|(i, v)| at(i, v).1 < crate::native::NEAR);
            let screen = p.test >> 16 & 1 == 0 || ztst == 1 || nearest;
            (screen, !screen && p.verts.iter().enumerate().any(|(i, v)| at(i, v).1 < crate::native::NEAR))
        }
    };
    let full = screen && wide_row;
    let kept = matches!((&swapped, &*memo), (None, Some(m)) if same_state(m, p, (screen, flat), full));
    if !kept {
        let st = tri_state(p, screen, flat, full, swapped.as_ref().map(|s| s.1), proj, cam, tex, core_ram, ram, boot, device, layout, queue)?;
        *memo = swapped.is_none().then(|| ((state_key(p, (screen, flat)), full), st));
        *scratch = Some(st);
    }
    let st = match (&*memo, &*scratch) {
        (Some(m), _) if kept || swapped.is_none() => &m.1,
        (_, Some(s)) => s,
        _ => unreachable!("the state was made above"),
    };
    // The vertices `prepare` made, for a triangle in the world whose texture is not a frame buffer and is the size it took.
    match pre.and_then(|pr| pr.world).filter(|(_, size)| st.map.is_none() && st.size == *size) {
        Some((w, _)) => {
            verts.v[..3].copy_from_slice(&w);
            verts.n = 3;
        }
        None => tri_verts(p, st.look(), worlds, &known, proj, cam, verts),
    }
    Ok(swapped.is_some())
}

/// For each primitive, whether it is over the main frame's whole width (PLAN 4.2: drawn across the whole wide picture when 2D,
/// `TriState::wide`): its window x (less XYOFFSET) from pixel 0 to 511 (a fade, the letterbox, a full-view pass), alone or as
/// one of a row of primitives of the same GS state and height, each starting where the last ends (the pause's dimming: sixteen
/// sprites 32 pixels wide).
fn full_width(prims: &[(usize, EePrim)]) -> Vec<bool> {
    let span = |p: &EePrim| {
        let (xo, yo) = ((p.xyoffset & 0xffff) as f64 / 16.0, (p.xyoffset >> 32 & 0xffff) as f64 / 16.0);
        p.verts.iter().fold([f64::MAX, f64::MIN, f64::MAX, f64::MIN], |s, v| [s[0].min(v.x - xo), s[1].max(v.x - xo), s[2].min(v.y - yo), s[3].max(v.y - yo)])
    };
    let state = |p: &EePrim| (p.prim, p.frame, p.test, p.alpha, p.tex0, p.xyoffset, p.scissor);
    let mut full = vec![false; prims.len()];
    let mut i = 0;
    // The span of primitive `i` when the last row's end has made it already.
    let mut carry = None;
    while i < prims.len() {
        let p = &prims[i].1;
        let first = carry.take().unwrap_or_else(|| span(p));
        let mut hi = first[1];
        let mut j = i + 1;
        while let Some((_, q)) = prims.get(j) {
            if state(q) != state(p) {
                break;
            }
            let s = span(q);
            if s[0] != hi || (s[2], s[3]) != (first[2], first[3]) {
                carry = Some(s);
                break;
            }
            hi = s[1];
            j += 1;
        }
        let f = first[0] <= 1.0 && hi >= MAIN_GS.0 as f64 - 1.0;
        full[i..j].fill(f);
        i = j;
    }
    full
}

/// The world positions of the frame's EE vertices and their view depths (`Projection::world`, then the distance along the view),
/// kept for the last few asked: the same point gives the same, and a strip's vertex is asked by each of its three triangles (and
/// twice by each: the 2D test, then the vertex).
#[derive(Default)]
struct Worlds {
    last: [Option<((u64, u64, u64), [f32; 3], f32)>; 4],
    next: usize,
}

impl Worlds {
    fn get(&mut self, proj: &Projection, cam: &Camera, x: f64, y: f64, z: f64) -> ([f32; 3], f32) {
        let k = (x.to_bits(), y.to_bits(), z.to_bits());
        if let Some((_, w, d)) = self.last.iter().flatten().find(|e| e.0 == k) {
            return (*w, *d);
        }
        let (w, d) = world_depth(proj, cam, x, y, z);
        self.last[self.next] = Some((k, w, d));
        self.next = (self.next + 1) % self.last.len();
        (w, d)
    }
}

/// A GS vertex's world position (`Projection::world`) and its depth along the view.
fn world_depth(proj: &Projection, cam: &Camera, x: f64, y: f64, z: f64) -> ([f32; 3], f32) {
    let w = proj.world(x, y, z);
    let d = (0..3).map(|a| (w[a] - cam.eye[a]) * cam.forward[a]).sum::<f32>();
    (w, d)
}

/// A frame primitive's triangle as `prepare` makes it on the crew's threads (`crew.rs`) before `build` reads the primitives in
/// order: its vertices' world positions and depths, whether it is drawn on the screen or flat (`triangle`), and, for one drawn in
/// the world, its vertices as `tri_verts` gives them when its texture is not a frame buffer (no `map`) and is `size` texels (a
/// texture's 2^TW x 2^TH, which `tri_state` holds every texture not a frame buffer to; 1 x 1 untextured).
#[derive(Clone, Copy)]
struct Pre {
    known: [([f32; 3], f32); 3],
    screen: bool,
    flat: bool,
    world: Option<([ObjVertex; 3], (f32, f32))>,
}

/// The frame primitives a chunk of `prepare` holds.
const PRE_CHUNK: usize = 1024;

/// `Pre` of each of the frame's triangles into `out` (None for any other primitive; the frame before's, for its room), made on the crew's threads in chunks of `PRE_CHUNK` primitives: what `triangle` and `tri_verts` make of a primitive from
/// it alone, the projection and the camera.
///
/// The crew's job also holds three chunks of the serial work `build` needs before it reads the primitives, none of which touches
/// what `prepare` writes: the prompts' text laid out (`prompts::prepare`), each primitive's row (`full_width`) and each one's
/// water pass; they run beside the chunks of `prepare` instead of before them. Their results are the function's.
fn prepare(prims: &[(usize, EePrim)], proj: &Projection, cam: &Camera, out: &mut Vec<Option<Pre>>, tex: &mut EeTextures, passes: Option<&[std::ops::Range<usize>]>) -> (Vec<String>, Vec<bool>, Vec<Option<usize>>) {
    // Every slot is written below: the frame before's are kept as they are (made anew, the 3 MB of 20,000 took 0.5 ms).
    out.resize(prims.len(), None);
    let parts: Vec<std::sync::Mutex<&mut [Option<Pre>]>> = out.chunks_mut(PRE_CHUNK).map(std::sync::Mutex::new).collect();
    let tex = std::sync::Mutex::new(tex);
    let problems = std::sync::Mutex::new(None);
    let rows = std::sync::Mutex::new(None);
    let pass_of = std::sync::Mutex::new(None);
    crate::crew::run(2 + parts.len(), &|k| {
        match k {
            0 => *problems.lock().expect("the prompts' chunk") = Some(crate::prompts::prepare(prims, &mut tex.lock().expect("the textures' chunk"))),
            1 => {
                *rows.lock().expect("the rows' chunk") = Some(full_width(prims));
                *pass_of.lock().expect("the passes' chunk") = Some(match passes {
                    Some(ranges) => {
                        let mut of = vec![None; prims.len()];
                        for (k, r) in ranges.iter().enumerate().rev() {
                            if r.start < r.end {
                                of[r.start.min(prims.len())..r.end.min(prims.len())].fill(Some(k));
                            }
                        }
                        of
                    }
                    None => Vec::new(),
                });
            }
            _ => {
                let mut part = parts[k - 2].lock().expect("a chunk of the crew's fill");
                for (j, slot) in part.iter_mut().enumerate() {
                    *slot = pre_of(prims, proj, cam, (k - 2) * PRE_CHUNK + j);
                }
            }
        }
    });
    let take = |what: &str| format!("the {what} chunk ran");
    (problems.into_inner().expect("lock").expect(&take("prompts")), rows.into_inner().expect("lock").expect(&take("rows")), pass_of.into_inner().expect("lock").expect(&take("passes")))
}

/// `Pre` of primitive `i` (`prepare`).
fn pre_of(prims: &[(usize, EePrim)], proj: &Projection, cam: &Camera, i: usize) -> Option<Pre> {
    {
        let p = &prims[i].1;
        if !(3..=5).contains(&(p.prim & 7)) || p.verts.len() != 3 {
            return None;
        }
        let ztst = p.test >> 17 & 3;
        // On the screen whatever its vertices' depths (`triangle`).
        if p.test >> 16 & 1 == 0 || ztst == 1 {
            return Some(Pre { known: [([0.0; 3], 0.0); 3], screen: true, flat: false, world: None });
        }
        let known: [([f32; 3], f32); 3] = std::array::from_fn(|i| world_depth(proj, cam, p.verts[i].x, p.verts[i].y, p.verts[i].z));
        let screen = ztst == 2 && known.iter().all(|k| k.1 < crate::native::NEAR);
        let flat = !screen && known.iter().any(|k| k.1 < crate::native::NEAR);
        let world = (!screen && !flat).then(|| {
            let size = if p.prim & 0x10 == 0 { (1.0, 1.0) } else { ((1u64 << (p.tex0 >> 26 & 15)) as f32, (1u64 << (p.tex0 >> 30 & 15)) as f32) };
            let look = Look { screen: false, flat: false, size, map: None, decal: decal(p), window: None, wide: false };
            let mut out = TriVerts { n: 0, v: bytemuck::Zeroable::zeroed() };
            tri_verts(p, look, &mut Worlds::default(), &known.map(Some), proj, cam, &mut out);
            ([out.v[0], out.v[1], out.v[2]], size)
        });
        Some(Pre { known, screen, flat, world })
    }
}

/// TFX 1 (DECAL) of a textured primitive: the texel as it is (`tri_verts`).
fn decal(p: &EePrim) -> bool {
    p.prim & 0x10 != 0 && p.tex0 >> 35 & 3 == 1
}

/// What of a primitive's `TriState` its vertices take (`tri_verts`).
#[derive(Clone, Copy)]
struct Look {
    screen: bool,
    flat: bool,
    size: (f32, f32),
    map: Option<([f64; 2], [f64; 2])>,
    decal: bool,
    window: Option<((u32, u32), f64, f64)>,
    wide: bool,
}

impl TriState {
    fn look(&self) -> Look {
        let TriState { screen, flat, size, map, decal, window, wide, .. } = *self;
        Look { screen, flat, size, map, decal, window, wide }
    }
}

/// A primitive's `TriState`: its targets, material set, blend, texture, sampler key and scissor (`swapped`: the atlas texture
/// `prompts::swap` gave it).
#[allow(clippy::too_many_arguments)]
fn tri_state(p: &EePrim, screen: bool, flat: bool, full: bool, swapped: Option<u32>, proj: &Projection, cam: &Camera, tex: &mut EeTextures, core_ram: u32, ram: &[u8], boot: &uyafmt::boot::Boot, device: &wgpu::Device, layout: &wgpu::BindGroupLayout, queue: &wgpu::Queue) -> Result<TriState, String> {
    let t = p.test;
    let (ate, atst, aref, afail, date, ztst) = (t & 1, t >> 1 & 7, (t >> 4 & 0xff) as u16, t >> 12 & 3, t >> 14 & 1, t >> 17 & 3);
    // PSMCT32, or PSMCT24 or PSMCT16 (another buffer: the display buffer, the glow's; its colour is kept at 8 bits a channel).
    let (fbp, fpsm) = (buffer(p.frame, p.frame >> 16), p.frame >> 24 & 0x3f);
    // ZTST GREATER (a monitor's screen in front of its frame) is GEQUAL natively: the two differ only at an equal depth.
    if !screen && (!matches!(ztst, 2 | 3) || fbp != MAIN) || date != 0 || p.frame >> 32 != 0 || !(fpsm == 0 || matches!(fpsm, 1 | 2) && fbp != MAIN) {
        return Err(format!("EE triangle: TEST {t:#x} FRAME {:#x}", p.frame));
    }
    // Another frame buffer: its window (XYOFFSET to the scissor's height) in GS pixels.
    if fbp != MAIN {
        tex.target(device, fbp, (p.scissor >> 48 & 0x7ff) as u32 + 1)?;
    }
    // The alpha test (the material set): none (or ALWAYS) 0, GEQUAL AREF, GREATER AREF (GEQUAL AREF + 1) or NEVER (256); a
    // failing pixel writing its colour (RGB_ONLY), its colour and alpha (FB_ONLY: the set + 1024), but not its depth, or nothing
    // (KEEP: the set + 512).
    let failing = || match afail {
        0 => Ok(512),
        1 => Ok(1024),
        3 => Ok(0),
        _ => Err(format!("EE triangle: TEST {t:#x}")),
    };
    let set = match (ate, atst) {
        (0, _) | (1, 1) => 0,
        _ => match (atst, failing()?) {
            (5, f) => aref + f,
            (6, f) => aref + 1 + f,
            (0, f) if f != 512 => 256 + f,
            _ => return Err(format!("EE triangle: TEST {t:#x}")),
        },
    };
    // A texture's function (TEX0 TCC and TFX, `Materials::texture_function`): the set + 2048 for TCC 0, + 4096 x TFX for
    // HIGHLIGHT and HIGHLIGHT2 (DECAL is MODULATE with the vertex colour 0x80: `tri_verts`).
    let set = if p.prim & 0x10 == 0 { set } else { set | ((p.tex0 >> 34 & 1 == 0) as u16) << 11 | match p.tex0 >> 35 & 3 { t @ 2..=3 => (t as u16 - 1) << 12, _ => 0 } };
    let blend = if p.prim & 0x40 == 0 {
        Blend::Replace
    } else {
        let a = |sh: u32| (p.alpha >> sh & 3) as u8;
        Blend::of_any(a(0), a(2), a(4), a(6), (p.alpha >> 32) as u8)?
    };
    // The texture's size in texels (2^TW x 2^TH), for UV coordinates; for a frame buffer the frame drew into, the affine map from
    // its GS pixels to the texture's coordinates instead, and whether it is the buffer drawn into (read from its copy).
    let mut size = (1.0f32, 1.0f32);
    let mut map: Option<([f64; 2], [f64; 2])> = None;
    let mut snap = None;
    // TFX 1 (DECAL): the texel as it is (the vertex colour 0x80 under MODULATE; with TCC 0 the vertex's alpha).
    let decal = decal(p);
    let tbp = (p.tex0 & 0x3fff) as u32;
    let sampled = (p.prim & 0x10 != 0 && tbp % 32 == 0 && matches!(p.tex0 >> 20 & 0x3f, 0 | 2)).then_some(buffer(p.tex0 >> 5, p.tex0 >> 14)).filter(|b| *b == MAIN || tex.targets.contains_key(b));
    let texture = if p.prim & 0x10 == 0 {
        tex.white
    } else if let Some(b) = sampled {
        // A frame buffer drawn this frame (any texture function: `set`'s below).
        // The buffer it draws into, and the main frame (drawn in the native picture's pass), are read from a copy made before;
        // into another buffer, the main frame at the GS's size.
        let at_gs = b == MAIN && fbp != MAIN;
        let copy = b == fbp || b == MAIN;
        snap = copy.then_some(if at_gs { MAIN_AT_GS } else { b });
        // A GS blend reads a copy of the buffer drawn into: a second copy (of the main frame, for another buffer) it cannot.
        if matches!(blend, Blend::Gs(..)) && b == MAIN && fbp != MAIN {
            return Err(format!("EE triangle: a GS blend into {fbp:#x} sampling the main frame"));
        }
        let (gs, index, snap_index) = if at_gs { (MAIN_GS, tex.main_gs_index, tex.main_gs_index) } else if b == MAIN { (MAIN_GS, tex.main_snap_index, tex.main_snap_index) } else if b == DISPLAY && fbp != DISPLAY { (DISPLAY_GS, tex.display_gs_index, tex.display_gs_index) } else { let t = &tex.targets[&b]; (t.gs, t.index, t.snap_index) };
        // GS pixel (u, v) of the buffer is window coordinate (u, v): for the main frame the native picture's point of the
        // primitive coordinate (u, v) + context 1's XYOFFSET (`Projection::ndc`), for another its (u / width, v / height).
        map = Some(if b == MAIN {
            let (xo, yo) = ((p.xyoffset1 & 0xffff) as f64 / 16.0, (p.xyoffset1 >> 32 & 0xffff) as f64 / 16.0);
            let (n0, n1) = (proj.ndc(cam, xo, yo), proj.ndc(cam, xo + 1.0, yo + 1.0));
            let m = ([((n1[0] - n0[0]) / 2.0) as f64, ((n0[0] + 1.0) / 2.0) as f64], [(-(n1[1] - n0[1]) / 2.0) as f64, ((1.0 - n0[1]) / 2.0) as f64]);
            if at_gs {
                let (w, h) = (tex.size.0 as f64, tex.size.1 as f64);
                tex.main_gs_map = [(m.0[0] * w) as f32, (m.0[1] * w) as f32, (m.1[0] * h) as f32, (m.1[1] * h) as f32];
                ([1.0 / gs.0 as f64, 0.0], [1.0 / gs.1 as f64, 0.0])
            } else {
                m
            }
        } else if b == DISPLAY {
            ([1.0 / gs.0 as f64, 0.0], [1.0 / DISPLAY_ROWS, 0.0])
        } else {
            ([1.0 / gs.0 as f64, 0.0], [1.0 / gs.1 as f64, 0.0])
        });
        if copy { snap_index } else { index }
    } else {
        let tex0 = p.tex0;
        let region = core_ram.wrapping_add(tex.region);
        // Not uploaded in the jobs: a HUD texture, or one of the GS RAM image, resident in GS memory.
        let hud = match p.texels {
            _ if swapped.is_some() => None,
            Some(_) => None,
            None => match tex.hud_texture(device, queue, tex0, p.clut)? {
                Some(i) => Some(i),
                None => match tex.gs_ram_texture(device, queue, tex0)? {
                    Some(i) => Some(i),
                    None => Some(tex.resident_texture(device, queue, tex0, p.tex1, p.mip, p.texa)?),
                },
            },
        };
        let up = p.texels;
        let i = if let Some(i) = swapped {
            i
        } else if let Some(i) = hud {
            i
        } else if let Some(&i) = up.and_then(|up| tex.offsets.get(&up.src.wrapping_sub(region))) {
            // A region 0x1A texture has its record's own CLUT: the one uploaded to CBP must be it.
            if p.clut.map(|c| c.src) != Some(region.wrapping_add(tex.cluts[i as usize])) {
                return Err(format!("EE triangle: texels of region 0x1A texture {i} with the CLUT uploaded from {:x?}", p.clut));
            }
            i
        } else if let Some(i) = match up {
            Some(up) => tex.hud_upload(device, queue, tex0, up, p.clut)?,
            None => None,
        } {
            i
        } else if let Some(up) = up.filter(|u| u.psm as u64 != tex0 >> 20 & 0x3f || (u.w, u.h) != (1 << (tex0 >> 26 & 15), 1 << (tex0 >> 30 & 15))) {
            tex.beyond_upload(device, queue, boot, ram, tex0, p.tex1, p.mip, up, p.clut, p.texa)?
        } else if let Some(up) = up {
            // Uploaded from disc data the game loaded, decoded from the disc's bytes and made once; or from the frame's RAM
            // (`EeTextures::loaded`), made once for each content it has (`ram_made`).
            // A CLUT the chain does not upload is the one GS memory holds from the fields before (an indexed format's).
            let resident = match p.clut {
                Some(_) => None,
                None if !matches!(crate::gsmem::bits(tex0 >> 20 & 0x3f)?, 8 | 4) => None,
                None => Some(tex.gsm.clut(tex0, p.texa)?),
            };
            let k = (tex0, up.src, p.clut.map_or(u32::MAX, |c| c.src));
            let from_ram = std::cell::Cell::new(resident.is_some());
            match tex.loaded.get(&k) {
                Some(&i) => i,
                None => {
                    let t = loaded_texture(|a, n| tex.loaded(boot, ram, a, n).map(|(b, r)| { from_ram.set(from_ram.get() | r); b }), tex0, up, p.clut, resident.as_deref(), p.texa)?;
                    let content = from_ram.get().then(|| {
                        let px: Vec<u8> = t.levels[0].iter().flatten().copied().collect();
                        (tex0, px.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &b| (h ^ b as u64).wrapping_mul(0x100_0000_01b3)))
                    });
                    if let Some(&i) = content.and_then(|c| tex.ram_made.get(&c)) {
                        i
                    } else {
                    crate::texnames::dump_ui(&t)?;
                    // The HUD's text and button glyphs at four times their resolution (`sharp`).
                    let i = if crate::prompts::font_or_sheet(up.src) {
                        tex.push_sharp(device, queue, &t)
                    } else {
                        let EeTextures { base, by_aref, sizes, .. } = &mut *tex;
                        let i = base.push(device, queue, &t, &mut by_aref.values_mut().collect::<Vec<_>>())?;
                        sizes.push((t.width, t.height));
                        i
                    };
                    match content {
                        Some(c) => {
                            tex.ram_made.insert(c, i);
                        }
                        None => {
                            tex.loaded.insert(k, i);
                        }
                    }
                    i
                    }
                }
            }
        } else {
            return Err(format!("EE triangle: TEX0 {tex0:#x}: no texture uploaded in the jobs or resident in GS memory there"));
        };
        // The GS samples at S / Q x 2^TW; the native sampler at S / Q x the texture's width: they agree when 2^TW is the width.
        let (w, h) = tex.sizes[i as usize];
        if 1usize << (tex0 >> 26 & 15) != w || 1usize << (tex0 >> 30 & 15) != h {
            return Err(format!("EE triangle: TEX0_1 {tex0:#x} for a {w} x {h} texture"));
        }
        size = (w as f32, h as f32);
        i
    };
    // A GS blend the shader does reads a copy of the buffer drawn into.
    if matches!(blend, Blend::Gs(..)) {
        snap = Some(fbp);
    }
    // CLAMP_1 WMS and WMT: 0 repeat, 1 clamp, 2 REGION_CLAMP (the HUD's font: each glyph's cell).
    let (wms, wmt) = (p.clamp & 3, p.clamp >> 2 & 3);
    if wms == 3 || wmt == 3 {
        return Err(format!("EE triangle: CLAMP_1 {:#x}", p.clamp));
    }
    let region = if wms == 2 || wmt == 2 { p.clamp & 0xfff_ffff_ffff } else { 0 };
    let key: Key = (texture, wms != 0, wmt != 0, (p.tex1 >> 6 & 7) as u32, tex1_k((p.tex1 >> 32) as u32).to_bits(), blend, region);
    let base = &tex.base;
    let m = tex.by_aref.entry(set).or_insert_with(|| {
        let m = match set & 0x1ff {
            256 => base.with_aref(0).never_passing(),
            a => base.with_aref(a),
        };
        let m = match set & 0x600 {
            512 => m.keeping(),
            1024 => m.fb_only(),
            _ => m,
        };
        match set >> 11 {
            0 => m,
            f => m.texture_function(f & 1 == 0, match f >> 1 { 0 => 0, h => h as u8 + 1 }),
        }
    });
    m.get(device, layout, key)?;
    // Another frame buffer's window: the primitive's coordinate less XYOFFSET over the buffer's size.
    let window = (fbp != MAIN).then(|| (tex.targets[&fbp].gs, (p.xyoffset & 0xffff) as f64 / 16.0, (p.xyoffset >> 32 & 0xffff) as f64 / 16.0));
    let wide = full || sampled.is_some();
    // SCISSOR_1 (window pixels, both ends drawn) in the target's own pixels, x0, y0, x1, y1: GS pixel x covers the native
    // picture from the point of GS coordinate x to that of x + 1 (the half pixel in). The main frame's whole window is the whole
    // wide picture; another scissor of a 2D primitive is placed as the primitive is (`unstretched`).
    let scissor = {
        let sc = p.scissor;
        let (x0, x1, y0, y1) = ((sc & 0x7ff) as f64, (sc >> 16 & 0x7ff) as f64 + 1.0, (sc >> 32 & 0x7ff) as f64, (sc >> 48 & 0x7ff) as f64 + 1.0);
        let (size, a, b) = match window {
            Some((gs, _, _)) => (tex.targets[&fbp].size, [x0 / gs.0 as f64, y0 / gs.1 as f64], [x1 / gs.0 as f64, y1 / gs.1 as f64]),
            None => {
                let (xo, yo) = ((p.xyoffset & 0xffff) as f64 / 16.0, (p.xyoffset >> 32 & 0xffff) as f64 / 16.0);
                let (mut n0, mut n1) = (proj.ndc(cam, x0 + xo, y0 + yo), proj.ndc(cam, x1 + xo, y1 + yo));
                if screen && !wide && !(x0 <= 0.0 && x1 >= MAIN_GS.0 as f64) {
                    (n0, n1) = (unstretched(n0, cam), unstretched(n1, cam));
                }
                (tex.size, [(n0[0] as f64 + 1.0) / 2.0, (1.0 - n0[1] as f64) / 2.0], [(n1[0] as f64 + 1.0) / 2.0, (1.0 - n1[1] as f64) / 2.0])
            }
        };
        // The rectangle the scissor's corners map to, whichever way the map turns them: on level 18's arrival the view camera
        // and program 6's projection disagree for some fields, and the main frame's full scissor comes out upside down. A
        // scissor whose ends are the wrong way round passes nothing, as on the GS.
        let px = |f: f64, n: u32| (f * n as f64).round().clamp(0.0, n as f64) as u32;
        if x1 <= x0 || y1 <= y0 {
            [0; 4]
        } else {
            [px(a[0].min(b[0]), size.0), px(a[1].min(b[1]), size.1), px(a[0].max(b[0]), size.0), px(a[1].max(b[1]), size.1)]
        }
    };
    Ok(TriState { key, set, screen, flat, fbp, snap, scissor, size, map, decal, window, wide })
}

/// A point of the main frame's 2D layer (normalised device coordinates as `Projection::ndc` maps the frame across the picture)
/// at the original picture's scale about the picture's centre (PLAN 4.2): the game draws its 2D layer for the 512-pixel frame,
/// which is the middle of the wide picture (Hor+), so menus, boxes and text keep their shape (exact at `k` 1).
fn unstretched(n: [f32; 2], cam: &Camera) -> [f32; 2] {
    [n[0] / cam.k, n[1]]
}

/// A primitive's vertices as `tri_verts` gives them: a triangle's 3 or a sprite's rectangle's 6 (the slots past `n` are not
/// read), held in place (the sea's 20,000 triangles a frame would otherwise each allocate theirs).
struct TriVerts {
    n: usize,
    v: [ObjVertex; 6],
}

impl TriVerts {
    fn as_slice(&self) -> &[ObjVertex] {
        &self.v[..self.n]
    }
}

/// A primitive's vertices in the native picture (`screen`: in its coordinates) or the world, with the coordinates its state's
/// texture and map give them.
fn tri_verts(p: &EePrim, look: Look, worlds: &mut Worlds, known: &[Option<([f32; 3], f32)>; 3], proj: &Projection, cam: &Camera, out: &mut TriVerts) {
    let Look { screen, flat, size, map, decal, window, wide } = look;
    let kick = p.verts[p.verts.len() - 1].rgba;
    let (tw, th) = ((1u64 << (p.tex0 >> 26 & 15)) as f32, (1u64 << (p.tex0 >> 30 & 15)) as f32);
    // S, T, Q: FST's UV in texels over the texture's size (Q 1), else ST and Q; through a frame buffer's map (ST in texels is S x
    // 2^TW).
    let stq = |v: &crate::effects::GsVertex| match (map, p.prim & 0x100 != 0) {
        (None, true) => [v.u / size.0, v.v / size.1, 1.0],
        (None, false) => [v.s, v.t, v.q],
        (Some((mu, mv)), true) => [(mu[0] * v.u as f64 + mu[1]) as f32, (mv[0] * v.v as f64 + mv[1]) as f32, 1.0],
        (Some((mu, mv)), false) => [(mu[0] * (v.s * tw) as f64 + mu[1] * v.q as f64) as f32, (mv[0] * (v.t * th) as f64 + mv[1] * v.q as f64) as f32, v.q],
    };
    // A fade (an untextured black primitive over the picture blended by its alpha, ALPHA_1 0x44) at 7/8 opaque or more is drawn
    // black: the 7 % or less of the picture it leaves was black on a PS2's television, and a frame the game holds under one while it
    // loads showed it (the player, 17 November 2026: loading the Veldin save, the hero standing in the level for 12 fields under a
    // fade at 119 of 128 before the ship's landing, "it briefly shows you loaded in before the out of ship animation plays").
    let fade = screen && p.prim & 0x10 == 0 && p.alpha & 0xff == 0x44;
    // `at`: the vertex's world position and depth, where `triangle` has made them.
    let mut vertex = |v: &crate::effects::GsVertex, x: f64, y: f64, st: [f32; 3], at: Option<([f32; 3], f32)>| {
        // IIP 0: the colour of the vertex that draws the primitive; FGE 0: no fog.
        let colour = if p.prim & 8 == 0 { kick } else { v.rgba };
        let colour = if decal { [0x80, 0x80, 0x80, if p.tex0 >> 34 & 1 == 1 { 0x80 } else { colour[3] }] } else { colour };
        let colour = if fade && colour[..3] == [0, 0, 0] && colour[3] >= 0x70 { [0, 0, 0, 0x80] } else { colour };
        let f = if p.prim & 0x20 == 0 { 256.0 } else { v.f as f32 };
        if screen {
            // The GS's pixel centre is its integer coordinate, the native picture's half a pixel in: a GS coordinate is half a GS
            // pixel further on in the native picture (the glow's passes, which shift by half texels, rely on it).
            let n = match window {
                Some((gs, xo, yo)) => [((x - xo + 0.5) / gs.0 as f64 * 2.0 - 1.0) as f32, (1.0 - (y - yo + 0.5) / gs.1 as f64 * 2.0) as f32],
                // A world-less frame's back (`EePrim::fill`: a trip's starfield): the PS2's frame across the picture's width, scaled
                // whole (its top and bottom cut), as a front end's space scene is drawn.
                None if p.fill => {
                    let n = proj.ndc(cam, x + 0.5, y + 0.5);
                    [n[0], n[1] * cam.k]
                }
                // Full-width solid overlays use window boundaries (e.g. a letterbox from 0 to 512).
                // A half-texel sampling offset here exposes the scene along the top and left at HD sizes.
                None if wide && p.prim & 0x10 == 0 => {
                    let xo = (p.xyoffset & 0xffff) as f64 / 16.0;
                    let yo = (p.xyoffset >> 32 & 0xffff) as f64 / 16.0;
                    // A sprite or triangle covers the GS pixels whose centres are in it: its edges at the next pixel's boundary
                    // (one from -0.5 to 415.5 covers rows 0 to 415, the frame's whole height; mapped as it stood, the menus' black
                    // clear and dimming left the last half pixel under them, a light line along the picture's bottom: the player,
                    // 20 November 2026, "a light coloured border around the top and bottom of screen"; the stores dim the screen
                    // with two triangles from -0.5 to 511.5 and 415.5, which left the right edge and the bottom light: 23
                    // November 2026, "the same border is now appearing in normal stores").
                    let (x, y) = ((x - xo).ceil(), (y - yo).ceil());
                    [(x * 2.0 / MAIN_GS.0 as f64 - 1.0) as f32, ((1.0 - y * 2.0 / MAIN_GS.1 as f64) * proj.fill) as f32]
                }
                None if wide => proj.ndc(cam, x + 0.5, y + 0.5),
                None => unstretched(proj.ndc(cam, x + 0.5, y + 0.5), cam),
            };
            ObjVertex { pos: [n[0], n[1], 0.0], st, colour, f }
        } else if flat {
            // At the GS position, z the depth of the GS Z's view depth (`vs_flat`, `native::Camera::matrix`'s third row), the
            // largest (1, at NEAR) for one nearer: the GS's Z24 buffer keeps a wrapped Z at its largest. S, T and Q as the GS takes
            // them, interpolated across the picture.
            let (_, d) = at.unwrap_or_else(|| worlds.get(proj, cam, x, y, v.z));
            let (near, far) = (crate::native::NEAR, crate::native::FAR);
            let z = if d > near { (-near / (far - near) + near * far / (far - near) / d).max(0.0) } else { 1.0 };
            let n = proj.ndc(cam, x, y);
            ObjVertex { pos: [n[0], n[1], z], st, colour, f }
        } else {
            let (pos, d) = at.unwrap_or_else(|| worlds.get(proj, cam, x, y, v.z));
            ObjVertex { pos, st: st.map(|c| c * d), colour, f }
        }
    };
    if p.prim & 7 == 6 {
        // A sprite: its two corners' rectangle, the coordinates of each corner taken from the vertex that gives its side.
        let (a, b) = (&p.verts[0], &p.verts[1]);
        let (sa, sb) = (stq(a), stq(b));
        let mut c = |x: &crate::effects::GsVertex, y: &crate::effects::GsVertex| vertex(b, x.x, y.y, [if std::ptr::eq(x, a) { sa[0] } else { sb[0] }, if std::ptr::eq(y, a) { sa[1] } else { sb[1] }, 1.0], None);
        *out = TriVerts { n: 6, v: [c(a, a), c(b, a), c(a, b), c(b, a), c(b, b), c(a, b)] };
    } else {
        out.n = 0;
        for (i, v) in p.verts.iter().enumerate() {
            out.v[out.n] = vertex(v, v.x, v.y, stq(v), known[i]);
            out.n += 1;
        }
    }
}

/// A host to local transfer into the main frame of data the jobs hold (`mobies::Transfer::image`, PSMCT32 or PSMCT24) as the
/// GS writes it: a rectangle of its pixels where it lands in the frame (TRXPOS in the frame's pages from DBP), each pixel the
/// upload's (PSMCT24 leaves the frame's alpha: TEST NEVER, the failing pixels writing the colour alone); its texture made once
/// for each content it has. Its material key, vertices (two triangles in the native picture across the frame's whole width, as
/// the frame is) and material set.
fn picture(t: &crate::mobies::Transfer, tex: &mut EeTextures, boot: &uyafmt::boot::Boot, ram: &[u8], device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout) -> Result<(Key, Vec<ObjVertex>, u16), String> {
    let up = t.image.ok_or("an IMAGE into the main frame with no data in the jobs")?;
    let (dbp, psm) = ((t.bitbltbuf >> 32 & 0x3fff) as u32, t.bitbltbuf >> 56 & 0x3f);
    let (w, h) = (up.w as usize, up.h as usize);
    let px: Vec<[u8; 4]> = match psm {
        0 => tex.loaded(boot, ram, up.src, 4 * w * h)?.0.chunks_exact(4).map(|c| [c[0], c[1], c[2], c[3]]).collect(),
        1 => tex.loaded(boot, ram, up.src, 3 * w * h)?.0.chunks_exact(3).map(|c| [c[0], c[1], c[2], 0x80]).collect(),
        _ => return Err(format!("an IMAGE of PSM {psm:#x} into the main frame")),
    };
    let hash = px.iter().flatten().fold(0xcbf2_9ce4_8422_2325u64 ^ (w as u64) << 32 ^ h as u64, |a, &b| (a ^ b as u64).wrapping_mul(0x100_0000_01b3));
    let i = match tex.ram_made.get(&(u64::MAX, hash)) {
        Some(&i) => i,
        None => {
            let t = uyafmt::core::Texture { width: w, height: h, levels: vec![px], gs: None };
            let EeTextures { base, by_aref, sizes, .. } = &mut *tex;
            let i = base.push(device, queue, &t, &mut by_aref.values_mut().collect::<Vec<_>>())?;
            sizes.push((w, h));
            tex.ram_made.insert((u64::MAX, hash), i);
            i
        }
    };
    let set = if psm == 1 { 256 } else { 0 };
    let key: Key = (i, true, true, 1, tex1_k(0).to_bits(), Blend::Replace, 0);
    let base = &tex.base;
    let m = tex.by_aref.entry(set).or_insert_with(|| if set == 256 { base.with_aref(0).never_passing() } else { base.with_aref(0) });
    m.get(device, layout, key)?;
    // Where it lands: the frame's page of DBP (the main frame, FBP 0xF4, 8 pages of 64 x 32 a row), then TRXPOS.
    let page = dbp / 32 - 0xF4;
    let (x0, y0) = (((page % 8) * 64) as f32 + (t.trxpos >> 32 & 0x7ff) as f32, ((page / 8) * 32) as f32 + (t.trxpos >> 48 & 0x7ff) as f32);
    let c = |x: f32, y: f32, s: f32, u: f32| {
        let (a, b) = (x / MAIN_GS.0 as f32, y / MAIN_GS.1 as f32);
        ObjVertex { pos: [a * 2.0 - 1.0, 1.0 - b * 2.0, 0.0], st: [s, u, 1.0], colour: [0x80; 4], f: 256.0 }
    };
    let (x1, y1) = (x0 + w as f32, y0 + h as f32);
    Ok((key, vec![c(x0, y0, 0.0, 0.0), c(x1, y0, 1.0, 0.0), c(x0, y1, 0.0, 1.0), c(x1, y0, 1.0, 0.0), c(x1, y1, 1.0, 1.0), c(x0, y1, 0.0, 1.0)], set))
}

/// One step, with its range of the frame's shadow vertices.
pub(crate) enum Step {
    /// Alpha 0x80 and the counter 0.
    Zero(std::ops::Range<u32>),
    /// The faces that count up and those that count down.
    Count(std::ops::Range<u32>, std::ops::Range<u32>),
    /// Where the counter is below zero: colour := Cd + (Cs - Cd) x FIX / 0x80, alpha := As.
    Darken(std::ops::Range<u32>, u8),
    /// The EE's own triangles, drawn as the objects are with the material set of TEST_1 AREF (256: never passes; + 512: a
    /// failing pixel writes nothing); in the picture's own coordinates when `screen` is set (2D); into frame buffer `target`
    /// (`MAIN` or one of `EeTextures::targets`), reading a copy of buffer `snap` made just before, inside `scissor` (SCISSOR_1
    /// in the target's pixels: x0, y0, x1, y1); at the GS's positions with the GS's depth when `flat` is set (`TriState::flat`).
    Draw { draws: Vec<Draw>, set: u16, screen: bool, flat: bool, target: u32, snap: Option<u32>, scissor: [u32; 4] },
    /// The water reflections (`reflections.rs`) over a pass's level water triangles drawn so far: a range of `EeDraw::water`.
    Reflect(std::ops::Range<u32>),
}

/// The frame's steps (each with the moby draw it comes before), their vertices, the shadow faces and EE triangles drawn, and the
/// primitives not drawn by reason.
pub(crate) struct EeDraw {
    pub steps: Vec<(usize, Step)>,
    pub vertices: Vec<ObjVertex>,
    pub faces: usize,
    pub triangles: usize,
    pub skipped: Vec<(String, usize)>,
    /// Whether the frame draws onto the display buffer (a fade): the picture is then the display buffer's.
    pub display: bool,
    /// The water's triangles of the reflections' plane (world positions), which `Step::Reflect` draws.
    pub water: Vec<ObjVertex>,
}

/// A step being built: its vertices' runs in the frame's vertices (`Runs`), or the shadow faces' two counting lists (put after the
/// rest at the end).
enum Raw {
    Zero(std::ops::Range<u32>),
    Count(Vec<ObjVertex>, Vec<ObjVertex>),
    Darken(std::ops::Range<u32>, u8),
    Draw(Vec<(Key, std::ops::Range<u32>)>, u16, bool, bool, u32, Option<u32>, [u32; 4]),
    Reflect(std::ops::Range<u32>),
}

/// The frame's EE vertices as the steps are built, each run of a step at the end in turn: only the last run grows (the next
/// primitive joins the last step or begins another), so the steps' vertices are written once, where they are drawn from.
struct Runs(Vec<ObjVertex>);

impl Runs {
    fn run(&mut self, vs: &[ObjVertex]) -> std::ops::Range<u32> {
        let first = self.0.len() as u32;
        self.0.extend_from_slice(vs);
        first..self.0.len() as u32
    }
    fn grow(&mut self, r: &mut std::ops::Range<u32>, vs: &[ObjVertex]) {
        assert_eq!(r.end as usize, self.0.len(), "an EE step's run grows only at the frame's vertices' end");
        self.0.extend_from_slice(vs);
        r.end = self.0.len() as u32;
    }
}

/// The steps of the frame's EE primitives; `core_ram` is the core's RAM address.
#[allow(clippy::too_many_arguments)]
///
/// `transfers` (`mobies::Walk::transfers`): an IMAGE into the main frame puts back the pages the game downloaded (the native
/// picture `EeTextures::grab` holds), before the primitive at its index. The vertices go into `vertices` (emptied first: the
/// frame before's, handed back for its room). `water`: the passes' output (`mobies::Walk::passes`) and the reflections' plane,
/// when the frame's water reflects (`reflections.rs`): each pass's triangles of the plane are drawn again with the reflection
/// after the pass (`Step::Reflect`), their vertices into `water_verts` (emptied first: the frame before's, for its room).
pub(crate) fn build(prims: &[(usize, EePrim)], transfers: &[crate::mobies::Transfer], proj: &Projection, cam: &Camera, tex: &mut EeTextures, core_ram: u32, ram: &[u8], boot: &uyafmt::boot::Boot, device: &wgpu::Device, queue: &wgpu::Queue, layout: &wgpu::BindGroupLayout, mut vertices: Vec<ObjVertex>, water: Option<(&[std::ops::Range<usize>], crate::reflections::Plane)>, mut water_verts: Vec<ObjVertex>) -> EeDraw {
    // Each triangle's world positions and vertices, made on the crew's threads (`prepare`).
    let mut pre = std::mem::take(&mut tex.pre);
    // The frame's text laid out in the vector font before its letters are drawn (`prompts::prepare`), each primitive's row and
    // pass, beside it.
    let (problems, rows, pass_of) = prepare(prims, proj, cam, &mut pre, tex, water.map(|(ranges, _)| ranges));
    let mut triangles = 0;
    let mut skipped: Vec<(String, usize)> = Vec::new();
    let mut skip = |why: String| match skipped.iter_mut().find(|(w, _)| *w == why) {
        Some((_, n)) => *n += 1,
        None => skipped.push((why, 1)),
    };
    for why in problems {
        skip(why);
    }
    let mut raw: Vec<(usize, Raw)> = Vec::new();
    let pick = crate::surfaces::Pick::new(cam);
    vertices.clear();
    let mut runs = Runs(vertices);
    // The last triangle's state (`triangle`).
    let mut memo: Option<((StateKey, bool), TriState)> = None;
    let mut worlds = Worlds::default();
    // The state of a primitive drawn as another, which `memo` does not keep.
    let mut scratch: Option<TriState> = None;
    // The vertices of the primitive being drawn (`triangle`).
    let mut drawn = TriVerts { n: 0, v: bytemuck::Zeroable::zeroed() };
    let mut faces = 0;
    let mut display = false;
    // The view's origin on the screen (the corner of step 1's sprites): the faces sample the frame at their own pixel from it.
    let mut origin: Option<[f64; 2]> = None;
    // A sprite's rectangle as two triangles in the native picture.
    let rect = |p: &EePrim| -> [ObjVertex; 6] {
        let (a, b) = (&p.verts[0], &p.verts[1]);
        let c = |x: f64, y: f64| ObjVertex { pos: { let n = proj.ndc(cam, x + 0.5, y + 0.5); [n[0], n[1], 0.0] }, st: [0.0, 0.0, 1.0], colour: b.rgba, f: 256.0 };
        [c(a.x, a.y), c(b.x, a.y), c(a.x, b.y), c(b.x, a.y), c(b.x, b.y), c(a.x, b.y)]
    };
    // An untextured line as the GS steps it: n steps (the longer side, rounded up), step i's pixel at the point i / n of the
    // way rounded (half away from zero), inside SCISSOR and the buffer, the second vertex's colour (or the colours between
    // with IIP); each pixel a one-pixel sprite. (Every frame's line at x -0.5 along the display buffer's left edge rounds to
    // column -1: it writes nothing.)
    // The restores go in before the primitive at their index (at its place; after the last, at the frame's end). The primitives
    // are read where they are (the sea's passes give 20,000 a frame), the line's sprites below made here.
    let mut restores = transfers.iter().filter(|t| t.dir == 0).peekable();
    // The prompts' plates, each before the primitive it goes behind, and the primitives the prompts take out (`prompts::prepare`:
    // with the keyboard and mouse in a menu, the labels' glyphs).
    let (mut plates, hidden) = (std::mem::take(&mut tex.prompts.plates).into_iter().peekable(), std::mem::take(&mut tex.prompts.hidden));
    // `rows`, and `pass_of`: the pass each primitive is read in (the first of `water`'s ranges holding its index), looked up as it
    // goes (both made in `prepare`).
    let mut own = prims.iter().enumerate().peekable();
    // The place of the last primitive drawn: the prompts' primitives after the frame's last go there.
    let mut last_at = 0;
    let items = std::iter::from_fn(|| loop {
        if let Some(&(i, &(at, _))) = own.peek() {
            if let Some(t) = restores.next_if(|t| t.index <= i) {
                return Some((at, Err(t)));
            }
            if let Some((_, plate)) = plates.next_if(|(k, _)| *k <= i) {
                return Some((at, Ok((Held::Made(Box::new(plate)), false, None))));
            }
        }
        match own.next() {
            Some((i, _)) if hidden.contains(&i) => continue,
            Some((i, (at, p))) => {
                last_at = *at;
                return Some((*at, Ok((Held::Frame(p), rows[i], Some(i)))));
            }
            None => {
                if let Some(t) = restores.next() {
                    return Some((crate::mobies::AFTER_EFFECTS, Err(t)));
                }
                return plates.next().map(|(_, plate)| (last_at, Ok((Held::Made(Box::new(plate)), false, None))));
            }
        }
    });
    // The line's sprites wait here for their turn (`items` is read again once they are given).
    let mut items = items;
    let mut waiting = std::collections::VecDeque::new();
    let prims = std::iter::from_fn(|| loop {
        if let Some(item) = waiting.pop_front() {
            return Some(item);
        }
        let (at, p) = items.next()?;
        let (p, row, index) = match p {
            Ok(p) => p,
            Err(t) => return Some((at, Err(t))),
        };
        if p.prim & 7 != 1 || p.prim & 0x10 != 0 {
            return Some((at, Ok((p, row, index))));
        }
        let (a, b) = (p.verts[0], p.verts[1]);
        let (xo, yo) = ((p.xyoffset & 0xffff) as f64 / 16.0, (p.xyoffset >> 32 & 0xffff) as f64 / 16.0);
        let (x0, y0, x1, y1) = ((a.x - xo) as f32, (a.y - yo) as f32, (b.x - xo) as f32, (b.y - yo) as f32);
        let n = (x1 - x0).abs().max((y1 - y0).abs()).ceil().max(1.0) as i32;
        let sc = p.scissor;
        let (sx0, sx1, sy0, sy1) = ((sc & 0x7ff) as i32, (sc >> 16 & 0x7ff) as i32, (sc >> 32 & 0x7ff) as i32, (sc >> 48 & 0x7ff) as i32);
        waiting.extend((0..n).filter_map(|i| {
            let t = i as f32 / n as f32;
            let (x, y) = ((x0 + (x1 - x0) * t).round() as i32, (y0 + (y1 - y0) * t).round() as i32);
            if x < sx0.max(0) || x > sx1 || y < sy0.max(0) || y > sy1 {
                return None;
            }
            let rgba = if p.prim & 8 != 0 { [0, 1, 2, 3].map(|k| (a.rgba[k] as f32 + (b.rgba[k] as f32 - a.rgba[k] as f32) * t) as u8) } else { b.rgba };
            let corner = |d: f64| crate::effects::GsVertex { x: x as f64 + d + xo, y: y as f64 + d + yo, rgba, ..b };
            Some((at, Ok((Held::Made(Box::new(EePrim { prim: p.prim & !7 | 6, verts: crate::mobies::Verts::new(&[corner(-0.5), corner(0.5)]), ..(*p).clone() })), false, None))))
        }));
    });
    // The water reflections: the triangles of the plane, and the pass being read (its index among `water`'s ranges), where it
    // came and where its triangles start.
    water_verts.clear();
    let mut in_pass: Option<(usize, usize, u32)> = None;
    // Set before the frame is built (`mobies::dump_field`).
    let dump = crate::mobies::eedump();
    for (at, p) in prims {
        // A pass's last primitive read: its triangles of the plane drawn again with the reflection, after it.
        let pass = match (&p, water) {
            (Ok((_, _, Some(i))), Some(_)) => pass_of[*i],
            _ => None,
        };
        if in_pass.map(|(k, _, _)| k) != pass {
            if let Some((_, pat, start)) = in_pass.take()
                && water_verts.len() as u32 > start
            {
                raw.push((pat, Raw::Reflect(start..water_verts.len() as u32)));
            }
            in_pass = pass.map(|k| (k, at, water_verts.len() as u32));
        }
        let (p, wide_row, index) = match p {
            Ok((p, row, index)) => (p, row, index),
            Err(t) => {
                // The pages put back: each a sprite of the grabbed picture over the same place, its colour alone (PSMCT24 leaves the
                // frame's alpha), every pixel copied (TEST NEVER: the failing pixels write the colour only).
                match t.main_pages() {
                    Ok(pages) if pages.iter().all(|p| tex.grabbed.contains(p)) => {
                        let key: Key = (tex.grab_index, true, true, 0, tex1_k(0).to_bits(), Blend::Replace, 0);
                        let base = &tex.base;
                        let m = tex.by_aref.entry(256).or_insert_with(|| base.with_aref(0).never_passing());
                        if let Err(e) = m.get(device, layout, key) {
                            skip(e);
                            continue;
                        }
                        let mut v = Vec::new();
                        // A page's pixels in the main frame's window: the native picture is that window, MAIN_GS pixels.
                        for p in pages {
                            let (x, y) = (((p % 8) * 64) as f32, ((p / 8) * 32) as f32);
                            let c = |x: f32, y: f32| {
                                let (u, v) = (x / MAIN_GS.0 as f32, y / MAIN_GS.1 as f32);
                                ObjVertex { pos: [u * 2.0 - 1.0, 1.0 - v * 2.0, 0.0], st: [u, v, 1.0], colour: [0x80; 4], f: 256.0 }
                            };
                            v.extend([c(x, y), c(x + 64.0, y), c(x, y + 32.0), c(x + 64.0, y), c(x + 64.0, y + 32.0), c(x, y + 32.0)]);
                        }
                        triangles += v.len() / 3;
                        raw.push((at, Raw::Draw(vec![(key, runs.run(&v))], 256, true, false, MAIN, None, [0, 0, tex.size.0, tex.size.1])));
                    }
                    // Any other picture into the main frame (the front end's art behind its menus): its pixels over the frame's
                    // pixels it covers, as the GS writes them.
                    Ok(_) => match picture(t, tex, boot, ram, device, queue, layout) {
                        Ok(v) => {
                            triangles += v.1.len() / 3;
                            raw.push((at, Raw::Draw(vec![(v.0, runs.run(&v.1))], v.2, true, false, MAIN, None, [0, 0, tex.size.0, tex.size.1])));
                        }
                        Err(e) => skip(e),
                    },
                    Err(e) => skip(e),
                }
                continue;
            }
        };
        let p: &EePrim = &p;
        if dump {
            eprintln!("EE at {at}: PRIM {:#x} FRAME {:#x} TEST {:#x} ALPHA {:#x} TEX0 {:#x} TEX1 {:#x} XYO {:#x} SCISSOR {:#x} verts {:?}", p.prim, p.frame, p.test, p.alpha, p.tex0, p.tex1, p.xyoffset, p.scissor, p.verts.iter().map(|v| (v.x, v.y, v.z, v.rgba, v.u, v.v, v.s, v.t, v.q)).collect::<Vec<_>>());
        }
        let t = p.test;
        let (ate, atst, afail, date, datm, zte, ztst) = (t & 1, t >> 1 & 7, t >> 12 & 3, t >> 14 & 1, t >> 15 & 1, t >> 16 & 1, t >> 17 & 3);
        let (fbp, fbmsk) = (p.frame & 0x1ff, (p.frame >> 32) as u32);
        let main = buffer(p.frame, p.frame >> 16) == MAIN;
        let (ty, tme, abe, fst) = (p.prim & 7, p.prim >> 4 & 1, p.prim >> 6 & 1, p.prim >> 8 & 1);
        let al = |s: u32| p.alpha >> s & 3;
        // ALPHA_1 (Cs - Cd) x FIX + Cd.
        let fix_blend = (al(0), al(2), al(4), al(6)) == (0, 1, 2, 1);
        let fix = (p.alpha >> 32) as u8;
        // No alpha test passes and a failing pixel writes the frame (FB_ONLY).
        let fb_only = ate == 1 && atst == 0 && afail == 1;
        let last = raw.last_mut().filter(|(a, _)| *a == at);
        if main && ty == 6 && tme == 0 && abe == 1 && fix_blend && fix == 0 && fb_only && date == 1 && datm == 0 && zte == 1 && ztst == 1 {
            if p.verts[1].rgba[3] != 0x80 {
                skip(format!("shadow: alpha set to {:#x}", p.verts[1].rgba[3]));
                continue;
            }
            let corner = [p.verts[0].x.min(p.verts[1].x), p.verts[0].y.min(p.verts[1].y)];
            match last {
                Some((_, Raw::Zero(r))) => {
                    runs.grow(r, &rect(p));
                    let o = origin.expect("set with the step's first sprite");
                    origin = Some([o[0].min(corner[0]), o[1].min(corner[1])]);
                }
                _ => {
                    raw.push((at, Raw::Zero(runs.run(&rect(p)))));
                    origin = Some(corner);
                }
            }
        } else if main && ty == 6 && tme == 0 && abe == 1 && fix_blend && fb_only && date == 1 && datm == 0 && zte == 1 && ztst == 1 && fbmsk == 0 {
            match last {
                Some((_, Raw::Darken(r, f))) if *f == fix => runs.grow(r, &rect(p)),
                _ => raw.push((at, Raw::Darken(runs.run(&rect(p)), fix))),
            }
        } else if main && (3..=5).contains(&ty) && tme == 1 && fst == 0 && fbmsk == 0x00ff_ffff && fb_only && date == 0 && zte == 1 && ztst == 2 {
            // TEX0: the frame itself (TBP0 = FBP x 32), RGBA (TCC 1), MODULATE (TFX 0).
            let tex0 = p.tex0;
            if tex0 & 0x3fff != fbp * 32 || tex0 >> 34 & 7 != 1 {
                skip(format!("shadow: face textured with TEX0 {tex0:#x} (FRAME {:#x})", p.frame));
                continue;
            }
            let Some(o) = origin else {
                skip("shadow: face before the alpha is set".into());
                continue;
            };
            let (tw, th) = ((1u64 << (tex0 >> 26 & 15)) as f64, (1u64 << (tex0 >> 30 & 15)) as f64);
            if p.verts.iter().any(|v| ((v.s / v.q) as f64 * tw - (v.x - o[0])).abs() > 1.0 || ((v.t / v.q) as f64 * th - (v.y - o[1])).abs() > 1.0) {
                skip("shadow: face sampling the frame away from its own pixel".into());
                continue;
            }
            // IIP 0: the alpha of the vertex that draws it.
            let a = p.verts[2].rgba[3];
            if p.prim >> 3 & 1 == 1 && p.verts.iter().any(|v| v.rgba[3] != a) {
                skip("shadow: face with Gouraud alpha".into());
                continue;
            }
            let tri = p.verts.iter().map(|v| ObjVertex { pos: proj.world(v.x, v.y, v.z), st: [0.0, 0.0, 1.0], colour: v.rgba, f: 256.0 });
            faces += 1;
            let (up, down) = match last {
                Some((_, Raw::Count(u, d))) => (u, d),
                _ => {
                    raw.push((at, Raw::Count(Vec::new(), Vec::new())));
                    let Some((_, Raw::Count(u, d))) = raw.last_mut() else { unreachable!("pushed above") };
                    (u, d)
                }
            };
            match a {
                0x82 => up.extend(tri),
                0x7f => down.extend(tri),
                0x80 => {}
                a => skip(format!("shadow: face alpha {a:#x}")),
            }
        } else if buffer(p.frame, p.frame >> 16) == DISPLAY && tme == 1 && buffer(p.tex0 >> 5, p.tex0 >> 14) == MAIN && p.tex0 & 0x1f == 0 {
            // The copy of the frame before into the display buffer: the native picture is copied there after each frame.
            continue;
        } else if zte == 1 && ztst == 0 {
            // Z test NEVER: no pixel passes.
            continue;
        } else if (3..=6).contains(&ty) {
            display |= buffer(p.frame, p.frame >> 16) == DISPLAY;
            let pre = index.and_then(|i| pre[i].as_ref());
            match triangle(p, pre, wide_row, &mut memo, &mut scratch, &mut worlds, proj, cam, tex, core_ram, ram, boot, device, queue, layout, &mut drawn) {
                Ok(swapped) => {
                    let st = match (swapped, &memo, &scratch) {
                        (false, Some(m), _) => &m.1,
                        (true, _, Some(s)) => s,
                        _ => unreachable!("`triangle` leaves the state where it says"),
                    };
                    let (key, set, screen, flat, target, snap, scissor) = (st.key, st.set, st.screen, st.flat, st.fbp, st.snap, st.scissor);
                    triangles += 1;
                    let tri = drawn.as_slice();
                    if let Some(pick) = pick.as_ref().filter(|_| target == MAIN) {
                        for v in tri.chunks_exact(3) {
                            let positions = [v[0].pos, v[1].pos, v[2].pos];
                            let hit = if screen || flat { pick.hit_screen(positions).then_some(0.0) } else { pick.hit(positions) };
                            if let Some(distance) = hit {
                                let name = tex.by_aref[&set].names[key.0 as usize].map(|n| n.stem());
                                println!("pick: EE after {at} distance {distance:.4} screen {screen} texture {name:?} key {key:?} aref {set} alpha {:#x} test {:#x} zbuf {:#x} colours {:?} stq {:?}", p.alpha, p.test, p.zbuf, v.iter().map(|v| v.colour).collect::<Vec<_>>(), v.iter().map(|v| v.st).collect::<Vec<_>>());
                            }
                        }
                    }
                    // A translucent layer's triangle of the plane, each vertex with its opacity (`reflections::opacity`).
                    if let (Some(_), Some((_, plane))) = (in_pass, water)
                        && !screen
                        && !flat
                        && target == MAIN
                        && tri.len() == 3
                        && let Some(op) = crate::reflections::opacity(p)
                        && plane.holds([tri[0].pos, tri[1].pos, tri[2].pos])
                    {
                        water_verts.extend(tri.iter().zip(p.verts.iter()).map(|(v, g)| ObjVertex { f: op.of(g), ..*v }));
                    }
                    // The game copies the screen in adjacent sprites. Take one snapshot for a
                    // continuous blit: filtering across a join must read the original left strip.
                    // Overlapping blits and destination-dependent blends still take a fresh copy.
                    let continues_blit = |draws: &[(Key, std::ops::Range<u32>)]| {
                        if !wide_row || !main || (ty, tme, abe, fst) != (6, 1, 0, 1) || key.0 != tex.main_snap_index {
                            return false;
                        }
                        let Some((prev_key, range)) = draws.last() else { return false };
                        if *prev_key != key || range.len() < 6 { return false; }
                        let prev = &runs.0[range.end as usize - 6..range.end as usize];
                        tri[0].pos[0] < tri[1].pos[0] && prev[0].pos[0] < prev[1].pos[0]
                            && prev[1].pos == tri[0].pos && prev[4].pos == tri[2].pos
                            && prev[1].st == tri[0].st && prev[4].st == tri[2].st
                    };
                    match last {
                        Some((_, Raw::Draw(d, s, sc, fl, tg, sn, sr))) if *s == set && *sc == screen && *fl == flat && *tg == target && *sn == snap && *sr == scissor && (snap != Some(target) || continues_blit(d)) => match d.last_mut() {
                            Some((k, r)) if *k == key => runs.grow(r, tri),
                            _ => d.push((key, runs.run(tri))),
                        },
                        _ => raw.push((at, Raw::Draw(vec![(key, runs.run(tri))], set, screen, flat, target, snap, scissor))),
                    }
                }
                Err(why) => skip(why),
            }
        } else {
            skip(format!("EE primitive PRIM {:#x} FRAME {:#x} TEST {t:#x} ALPHA {:#x} TEX0 {:#x}", p.prim, p.frame, p.alpha, p.tex0));
        }
    }
    if let Some((_, pat, start)) = in_pass
        && water_verts.len() as u32 > start
    {
        raw.push((pat, Raw::Reflect(start..water_verts.len() as u32)));
    }
    // The shadow faces' counting lists after the rest.
    let mut vertices = runs.0;
    let mut put = |v: Vec<ObjVertex>| {
        let first = vertices.len() as u32;
        vertices.extend(v);
        first..vertices.len() as u32
    };
    let steps = raw
        .into_iter()
        .map(|(at, r)| {
            (
                at,
                match r {
                    Raw::Zero(r) => Step::Zero(r),
                    Raw::Count(u, d) => {
                        let u = put(u);
                        Step::Count(u, put(d))
                    }
                    Raw::Darken(r, f) => Step::Darken(r, f),
                    Raw::Draw(d, set, screen, flat, target, snap, scissor) => Step::Draw { draws: d.into_iter().map(|(key, r)| Draw { key, first: r.start, count: r.end - r.start }).collect(), set, screen, flat, target, snap, scissor },
                    Raw::Reflect(r) => Step::Reflect(r),
                },
            )
        })
        .collect();
    tex.pre = pre;
    EeDraw { steps, vertices, faces, triangles, skipped, display, water: water_verts }
}

const SHADER: &str = r#"
struct View { m: mat4x4<f32>, sky: mat4x4<f32>, eye: vec4<f32>, forward: vec4<f32>, fog_colour: vec4<f32>, fog: vec4<f32> };
@group(0) @binding(0) var<uniform> view: View;

struct VIn { @location(0) pos: vec3<f32>, @location(1) st: vec3<f32>, @location(2) colour: vec4<f32>, @location(3) f: f32 };
struct VOut { @builtin(position) clip: vec4<f32>, @location(0) @interpolate(flat) colour: vec4<f32> };

@vertex fn vs_world(v: VIn) -> VOut {
    var o: VOut;
    o.clip = view.m * vec4<f32>(v.pos, 1.0);
    o.colour = v.colour;
    return o;
}

// A position already in the picture's normalised device coordinates.
@vertex fn vs_screen(v: VIn) -> VOut {
    var o: VOut;
    o.clip = vec4<f32>(v.pos.xy, 0.0, 1.0);
    o.colour = v.colour;
    return o;
}

// The colour and As as the target holds them (alpha over 0x80).
@fragment fn fs(i: VOut) -> @location(0) vec4<f32> {
    let c = round(i.colour * 255.0);
    return vec4<f32>(c.rgb / 255.0, c.a / 255.0);
}
"#;

/// The main frame at the GS's size: GS pixel (u, v) is the average of the native picture's pixels it covers (`map`: where its
/// edges lie in the native picture), each by the area it covers.
const DOWNSAMPLE: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> map: vec4<f32>;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

@fragment fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let g = floor(at.xy);
    let lo = map.xz * g + map.yw;
    let hi = map.xz * (g + 1.0) + map.yw;
    let last = vec2<i32>(textureDimensions(src)) - 1;
    var sum = vec4<f32>(0.0);
    var area = 0.0;
    for (var y = i32(floor(lo.y)); f32(y) < hi.y; y++) {
        let wy = min(f32(y + 1), hi.y) - max(f32(y), lo.y);
        for (var x = i32(floor(lo.x)); f32(x) < hi.x; x++) {
            let wx = min(f32(x + 1), hi.x) - max(f32(x), lo.x);
            sum += textureLoad(src, clamp(vec2<i32>(x, y), vec2<i32>(0), last), 0) * wx * wy;
            area += wx * wy;
        }
    }
    return sum / area;
}
"#;

/// The shadow steps' pipelines, and the main frame's downsampling to the GS's size.
pub(crate) struct Pipelines {
    gs_size: wgpu::RenderPipeline,
    gs_size_layout: wgpu::BindGroupLayout,
    zero: wgpu::RenderPipeline,
    up: wgpu::RenderPipeline,
    down: wgpu::RenderPipeline,
    darken: wgpu::RenderPipeline,
}

impl Pipelines {
    pub fn new(device: &wgpu::Device, view_layout: &wgpu::BindGroupLayout) -> Pipelines {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("shadows"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("shadows"), bind_group_layouts: &[Some(view_layout)], immediate_size: 0 });
        let attrs = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Unorm8x4, 3 => Float32];
        let pipeline = |label: &str, vs: &str, depth: wgpu::CompareFunction, stencil: wgpu::StencilFaceState, masks: (u32, u32), writes: wgpu::ColorWrites, blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(vs),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout { array_stride: std::mem::size_of::<ObjVertex>() as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &attrs })],
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: None, ..Default::default() },
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: crate::native::DEPTH,
                    depth_write_enabled: Some(false),
                    depth_compare: Some(depth),
                    stencil: wgpu::StencilState { front: stencil, back: stencil, read_mask: masks.0, write_mask: masks.1 },
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend, write_mask: writes })],
                }),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        let down_module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("downsample"), source: wgpu::ShaderSource::Wgsl(DOWNSAMPLE.into()) });
        let gs_size_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("downsample"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry { binding: 1, visibility: wgpu::ShaderStages::FRAGMENT, ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None }, count: None },
            ],
        });
        let gs_size = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("downsample"),
            layout: Some(&device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("downsample"), bind_group_layouts: &[Some(&gs_size_layout)], immediate_size: 0 })),
            vertex: wgpu::VertexState { module: &down_module, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState { module: &down_module, entry_point: Some("fs"), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: None, write_mask: wgpu::ColorWrites::ALL })] }),
            multiview_mask: None,
            cache: crate::pipelines::cache(),
        });
        let op = |pass_op: wgpu::StencilOperation| wgpu::StencilFaceState { compare: wgpu::CompareFunction::Always, fail_op: wgpu::StencilOperation::Keep, depth_fail_op: wgpu::StencilOperation::Keep, pass_op };
        let always = wgpu::CompareFunction::Always;
        // Z test GEQUAL on the GS: nearer or equal passes.
        let nearer = wgpu::CompareFunction::GreaterEqual;
        Pipelines {
            gs_size,
            gs_size_layout,
            // Step 1's sprites pass only where the frame's alpha is below 0x80 (DATE, DATM 0) and write 0x80 there: the alpha
            // becomes the greater of its own and 0x80, so a pixel above 0x80 keeps it for the glow's extraction (level 6 field
            // 9000: the hangar's red tubes).
            zero: pipeline(
                "shadow zero",
                "vs_screen",
                always,
                op(wgpu::StencilOperation::Replace),
                (0xff, 0xff),
                wgpu::ColorWrites::ALPHA,
                Some(wgpu::BlendState { color: wgpu::BlendComponent::REPLACE, alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Max } }),
            ),
            up: pipeline("shadow up", "vs_world", nearer, op(wgpu::StencilOperation::IncrementWrap), (0xff, 0xff), wgpu::ColorWrites::empty(), None),
            down: pipeline("shadow down", "vs_world", nearer, op(wgpu::StencilOperation::DecrementWrap), (0xff, 0xff), wgpu::ColorWrites::empty(), None),
            darken: pipeline(
                "shadow darken",
                "vs_screen",
                always,
                wgpu::StencilFaceState { compare: wgpu::CompareFunction::Equal, fail_op: wgpu::StencilOperation::Keep, depth_fail_op: wgpu::StencilOperation::Keep, pass_op: wgpu::StencilOperation::Keep },
                (0x80, 0),
                wgpu::ColorWrites::ALL,
                Some(wgpu::BlendState {
                    color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Constant, dst_factor: wgpu::BlendFactor::OneMinusConstant, operation: wgpu::BlendOperation::Add },
                    alpha: wgpu::BlendComponent::REPLACE,
                }),
            ),
        }
    }

    /// The main frame at the GS's size: `to` (512 x 416) from the native picture `from` (`DOWNSAMPLE`).
    pub fn downsample(&self, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, from: &wgpu::Texture, to: &wgpu::Texture, map: [f32; 4]) {
        use wgpu::util::DeviceExt;
        let ub = device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some("downsample"), contents: bytemuck::cast_slice(&map), usage: wgpu::BufferUsages::UNIFORM });
        let src = from.create_view(&Default::default());
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("downsample"),
            layout: &self.gs_size_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&src) }, wgpu::BindGroupEntry { binding: 1, resource: ub.as_entire_binding() }],
        });
        let view = to.create_view(&Default::default());
        let mut p = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("main frame at the GS's size"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: &view, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store } })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        p.set_pipeline(&self.gs_size);
        p.set_bind_group(0, &bg, &[]);
        p.draw(0..3, 0..1);
    }

    /// Draws shadow step `step` (the frame's EE vertex buffer bound); the EE triangles' steps are the objects pipelines'.
    pub fn draw<'a, E: crate::replay::Enc<'a>>(&'a self, pass: &mut E, step: &Step) -> Result<(), String> {
        match step {
            Step::Zero(r) => {
                pass.set_pipeline(&self.zero);
                pass.set_stencil_reference(0)?;
                pass.draw(r.clone());
            }
            Step::Count(u, d) => {
                pass.set_pipeline(&self.up);
                pass.draw(u.clone());
                pass.set_pipeline(&self.down);
                pass.draw(d.clone());
            }
            Step::Darken(r, fix) => {
                let f = (*fix as f64 / 128.0).min(1.0);
                pass.set_pipeline(&self.darken);
                pass.set_stencil_reference(0x80)?;
                pass.set_blend_constant(wgpu::Color { r: f, g: f, b: f, a: f })?;
                pass.draw(r.clone());
            }
            Step::Draw { .. } => unreachable!("the caller draws the EE triangles with the objects pipelines"),
            Step::Reflect(_) => unreachable!("the caller draws the water's reflection (`reflections.rs`)"),
        }
        Ok(())
    }
}

/// The text's upscale: a texture of the HUD's fonts or glyph sheets (an alpha mask with 16 or 256 levels at PS2 resolution) at
/// four times its size, as the same letters drawn finer: each channel interpolated (Catmull-Rom, on colour times alpha). With
/// `edge` (from, to) the alpha's edge is narrowed to that band (a texture drawn by a pipeline that cannot anti-alias it itself);
/// without, the alpha stays a smooth field whose 0.5 line is the letters' outline, for a shader that draws the edge at the
/// screen's pixels. The texture's units are kept (alpha 0x80 opaque).
pub fn sharp(t: &uyafmt::core::Texture, edge: Option<(f32, f32)>) -> uyafmt::core::Texture {
    const K: usize = 4;
    let (w, h) = (t.width, t.height);
    let src = &t.levels[0];
    let px = |x: i64, y: i64| -> [f32; 4] {
        let p = src[(y.clamp(0, h as i64 - 1) as usize) * w + x.clamp(0, w as i64 - 1) as usize];
        let a = (p[3] as f32 / 128.0).min(1.0);
        [p[0] as f32 * a, p[1] as f32 * a, p[2] as f32 * a, a]
    };
    let cr = |t: f32| -> [f32; 4] {
        let (t2, t3) = (t * t, t * t * t);
        [(-t3 + 2.0 * t2 - t) / 2.0, (3.0 * t3 - 5.0 * t2 + 2.0) / 2.0, (-3.0 * t3 + 4.0 * t2 + t) / 2.0, (t3 - t2) / 2.0]
    };
    let (bw, bh) = (w * K, h * K);
    let mut acc: Vec<[f32; 4]> = Vec::with_capacity(bw * bh);
    for oy in 0..bh {
        let sy = (oy as f32 + 0.5) / K as f32 - 0.5;
        let (y0, fy) = (sy.floor() as i64, sy - sy.floor());
        let wy = cr(fy);
        for ox in 0..bw {
            let sx = (ox as f32 + 0.5) / K as f32 - 0.5;
            let (x0, fx) = (sx.floor() as i64, sx - sx.floor());
            let wx = cr(fx);
            let mut a = [0f32; 4];
            for (j, wyj) in wy.iter().enumerate() {
                for (i, wxi) in wx.iter().enumerate() {
                    let p = px(x0 - 1 + i as i64, y0 - 1 + j as i64);
                    for c in 0..4 {
                        a[c] += p[c] * wxi * wyj;
                    }
                }
            }
            acc.push(a);
        }
    }
    let out = acc
        .iter()
        .map(|p| {
            let a = p[3].clamp(0.0, 1.0);
            let sa = match edge {
                Some((lo, hi)) => {
                    let e = ((a - lo) / (hi - lo)).clamp(0.0, 1.0);
                    e * e * (3.0 - 2.0 * e)
                }
                None => a,
            };
            let col = |c: usize| if a > 1e-4 { (p[c] / a).clamp(0.0, 255.0) as u8 } else { 0 };
            [col(0), col(1), col(2), (sa * 128.0).round() as u8]
        })
        .collect();
    uyafmt::core::Texture { width: w * K, height: h * K, levels: vec![out], gs: None }
}

/// A texture view of `t` (its first level, RGBA8).
pub fn view(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> wgpu::TextureView {
    let (w, h) = (t.width as u32, t.height as u32);
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sharp text"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        bytemuck::cast_slice(&t.levels[0]),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
        wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
    );
    tex.create_view(&Default::default())
}

/// `Pipelines::downsample`'s map from `DISPLAY_GS`'s pixels into the native display buffer of `size` pixels.
pub(crate) fn display_gs_map(size: (u32, u32)) -> [f32; 4] {
    [size.0 as f32 / DISPLAY_GS.0 as f32, 0.0, size.1 as f32 / DISPLAY_GS.1 as f32, 0.0]
}
