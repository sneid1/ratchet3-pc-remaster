//! The surfaces worth new textures (PLAN 9.2 step 1): `uya native ... --surfaces OUT.tsv` draws a RAM dump's frame with every
//! terrain and tie texture replaced by a texture of its own id colour, then counts each id's pixels in the picture.
//!
//! The id texture keeps the original's size and its alpha (GS scale), so the alpha test cuts what it cuts in the game's picture.
//! The terrain's and the objects' shaders (`native::SHADER`, `objects::SHADER`) take the id texture's texel as the pixel, with no
//! vertex colour, no fog and full opacity where it passes the test (`shader`). The frame's overlays are left out: the EE's own
//! primitives and passes (shadows, the glow, the HUD, full-frame copies) and the effects, which would change the id colours
//! (`native::Native::render_frame`). The mobies, shrubs, sky and water still cover what they cover. A pixel is an id's when its
//! green's high nibble is 0xA and its blue 0x5A (`code`); among the picture's other colours that happens about once in
//! 4096 x 256 / ids, a few pixels a frame.
//!
//! With the count, each texture's world size per repeat: the length in world units of 1.0 of u and of v (the texture's width
//! and height), from the level's geometry, as the terrain and the ties are drawn: per triangle the world vectors dP/du and
//! dP/dv solved from its three positions and texture coordinates, the median over the texture's triangles weighted by their
//! world area (`Repeat`). A tie's vertices are its class-local positions / 1024 x the class's scale through the instance's
//! axes plus its position (`objects`, the tie programs' transform); the second part's coordinates (made from normals) are not
//! a repeat and are left out.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

static ON: AtomicBool = AtomicBool::new(false);

/// Turns the id drawing on for the process (before `native::Native::open`, which builds the shaders).
pub fn turn_on() {
    ON.store(true, Ordering::Relaxed);
}

pub(crate) fn on() -> bool {
    ON.load(Ordering::Relaxed)
}

/// A code-level material probe for a saved camera: `UYA_PICK_PIXEL=0.75,0.87` selects a fraction of its picture. It reports
/// the actual triangles under that point, including overlapping draw layers, without changing any rendering state.
pub(crate) struct Pick {
    eye: [f32; 3],
    dir: [f32; 3],
    point: [f32; 2],
}

impl Pick {
    /// The picture's point UYA_PICK_PIXEL asks for (x, y fractions), None when it is not set.
    fn point() -> Option<[f32; 2]> {
        static POINT: std::sync::OnceLock<Option<[f32; 2]>> = std::sync::OnceLock::new();
        *POINT.get_or_init(|| std::env::var("UYA_PICK_PIXEL").ok().map(|s| {
            let (x, y) = s.split_once(',').expect("UYA_PICK_PIXEL needs x,y fractions");
            let p = [x.parse::<f32>().expect("pick x"), y.parse::<f32>().expect("pick y")];
            assert!(p.iter().all(|x| (0.0..=1.0).contains(x)), "pick fractions must be in 0..1");
            p
        }))
    }

    /// Whether a pick is asked for (UYA_PICK_PIXEL set): only then does a level's load need the camera.
    pub fn wanted() -> bool {
        Self::point().is_some()
    }

    pub fn new(cam: &crate::native::Camera) -> Option<Self> {
        let point = Self::point()?;
        let right = cross(cam.forward, cam.up);
        let dir = std::array::from_fn(|k| cam.forward[k] + right[k] * (2.0 * point[0] - 1.0) * cam.tan[0] + cam.up[k] * (1.0 - 2.0 * point[1]) * cam.tan[1]);
        let length = dot(dir, dir).sqrt();
        Some(Self { eye: cam.eye, dir: dir.map(|v| v / length), point })
    }

    pub fn hit_screen(&self, p: [[f32; 3]; 3]) -> bool {
        let ray = Self { eye: [2.0 * self.point[0] - 1.0, 1.0 - 2.0 * self.point[1], 1.0], dir: [0.0, 0.0, -1.0], point: self.point };
        ray.hit(p).is_some()
    }

    pub fn hit(&self, p: [[f32; 3]; 3]) -> Option<f32> {
        let sub = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|k| a[k] - b[k]);
        let (e1, e2) = (sub(p[1], p[0]), sub(p[2], p[0]));
        let h = cross(self.dir, e2);
        let det = dot(e1, h);
        if det.abs() < 1e-8 { return None; }
        let s = sub(self.eye, p[0]);
        let u = dot(s, h) / det;
        let q = cross(s, e1);
        let v = dot(self.dir, q) / det;
        let t = dot(e2, q) / det;
        (u >= 0.0 && v >= 0.0 && u + v <= 1.0 && t > 0.0).then_some(t)
    }
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] { [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]] }

/// One counted texture: its PCSX2 name's stem, its layer, its size and its world size per repeat (u, v).
pub(crate) struct Entry {
    pub stem: String,
    pub layer: &'static str,
    pub width: usize,
    pub height: usize,
    pub repeat: Option<(f64, f64)>,
}

struct Ids {
    by_name: HashMap<crate::texnames::TexName, u32>,
    entries: Vec<Entry>,
}

static IDS: Mutex<Option<Ids>> = Mutex::new(None);

/// The bounds (min, max) of the hero armor's vertices drawn in world space this run (`mobies`), for the world's units per
/// metre: Ratchet's height in units.
static HERO: Mutex<Option<([f32; 3], [f32; 3])>> = Mutex::new(None);

pub(crate) fn hero_point(p: [f32; 3]) {
    let mut g = HERO.lock().expect("surfaces: hero lock");
    let b = g.get_or_insert((p, p));
    for k in 0..3 {
        b.0[k] = b.0[k].min(p[k]);
        b.1[k] = b.1[k].max(p[k]);
    }
}

const G_MARK: u8 = 0xA0;
const B_MARK: u8 = 0x5A;
const MAX_ID: u32 = 0xFFF;

fn code(id: u32) -> [u8; 3] {
    [(id & 0xFF) as u8, G_MARK | (id >> 8) as u8, B_MARK]
}

fn decode(p: &[u8]) -> Option<u32> {
    (p[2] == B_MARK && p[1] & 0xF0 == G_MARK).then(|| p[0] as u32 | ((p[1] & 0xF) as u32) << 8).filter(|&id| id > 0)
}

/// Gives each of the level's terrain and tie textures an id (1 on; a texture both layers use keeps the first), with its world
/// size per repeat: `repeats` per layer, indexed as the layer's textures.
pub(crate) fn assign(layers: &[(&'static str, Vec<&uyafmt::core::Texture>, Vec<Option<(f64, f64)>>)]) -> Result<(), String> {
    let mut ids = Ids { by_name: HashMap::new(), entries: Vec::new() };
    for (layer, textures, repeats) in layers {
        for (i, t) in textures.iter().enumerate() {
            let name = crate::texnames::name(t)?.ok_or_else(|| format!("surfaces: {layer} texture {i} has no GS form, so no PCSX2 name"))?;
            if ids.by_name.contains_key(&name) {
                continue;
            }
            let id = ids.entries.len() as u32 + 1;
            if id > MAX_ID {
                return Err(format!("surfaces: more than {MAX_ID} textures"));
            }
            ids.by_name.insert(name, id);
            ids.entries.push(Entry { stem: name.stem(), layer, width: t.width, height: t.height, repeat: repeats[i] });
        }
    }
    *IDS.lock().map_err(|_| "surfaces: lock poisoned")? = Some(ids);
    Ok(())
}

/// The id texture of `t` on the GPU (`texnames::hd` in id mode): a terrain or tie texture's id, or for any other named texture
/// drawn through the objects' materials (the mobies', the shrubs') an id given now, its layer `objects` and its world size per
/// repeat from the triangles drawn with it (`object_triangle`); None for a texture with no name.
pub(crate) fn id_view(device: &wgpu::Device, queue: &wgpu::Queue, t: &uyafmt::core::Texture) -> Result<Option<crate::texnames::HdView>, String> {
    let Some(name) = crate::texnames::name(t)? else { return Ok(None) };
    let id = {
        let mut g = IDS.lock().map_err(|_| "surfaces: lock poisoned")?;
        let ids = g.as_mut().ok_or("surfaces: a texture drawn before the level's ids were given")?;
        match ids.by_name.get(&name) {
            Some(&id) => id,
            None => {
                let id = ids.entries.len() as u32 + 1;
                if id > MAX_ID {
                    return Err(format!("surfaces: more than {MAX_ID} textures"));
                }
                ids.by_name.insert(name, id);
                ids.entries.push(Entry { stem: name.stem(), layer: "objects", width: t.width, height: t.height, repeat: None });
                id
            }
        }
    };
    let c = code(id);
    let level0 = t.levels.first().ok_or("surfaces: a texture with no level")?;
    let rgba: Vec<u8> = level0.iter().flat_map(|p| [c[0], c[1], c[2], p[3]]).collect();
    crate::texnames::upload_level0(device, queue, "surface id", t.width as u32, t.height as u32, rgba).map(Some)
}

/// The shader with its pack-replacement branch drawing the id texel as the pixel: `sample` is the shader's line that samples a
/// replacement (`t = round(textureSampleGrad(...)...)`), `uv` its texture coordinates.
pub(crate) fn shader(src: &str, sample: &str, uv: &str) -> Result<String, String> {
    if src.matches(sample).count() != 1 {
        return Err(format!("surfaces: the shader's replacement line `{sample}` is not there once"));
    }
    let id = format!("{{ let s = round(textureSampleLevel(tex, smp, {uv}, 0.0) * 255.0); return vec4<f32>(s.rgb, select(s.a, 128.0, s.a >= mat.aref)); }}");
    Ok(src.replace(sample, &id))
}

/// Each id's pixels in the picture.
fn count(img: &crate::pic::Image) -> HashMap<u32, u64> {
    let mut n = HashMap::new();
    for p in img.rgb.chunks_exact(3) {
        if let Some(id) = decode(p) {
            *n.entry(id).or_insert(0) += 1;
        }
    }
    n
}

/// Writes `out` (tab-separated): per terrain and tie texture of the level its stem, layer, size, pixels, share of the picture
/// and world size per repeat, most pixels first (those not in the picture too, at 0: the level's whole world, PLAN 9.2); prints
/// the first ten.
pub(crate) fn write(img: &crate::pic::Image, level: u32, out: &str) -> Result<(), String> {
    let g = IDS.lock().map_err(|_| "surfaces: lock poisoned")?;
    let ids = g.as_ref().ok_or("surfaces: no level was loaded")?;
    let n = count(img);
    let total = (img.w * img.h) as f64;
    let mut rows: Vec<(u32, u64)> = (1..=ids.entries.len() as u32).map(|id| (id, n.get(&id).copied().unwrap_or(0))).collect();
    rows.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    let hero = match *HERO.lock().map_err(|_| "surfaces: lock poisoned")? {
        Some((lo, hi)) => format!("hero armor extent x {:.3} y {:.3} z {:.3} units", hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]),
        None => "hero armor not drawn".to_string(),
    };
    println!("surfaces: {hero}");
    let mut s = format!("# level {level}, {} x {} pixels, {hero}\nstem\tlayer\twidth\theight\tpixels\tshare\trepeat_u\trepeat_v\n", img.w, img.h);
    let objects = OBJECTS.lock().map_err(|_| "surfaces: lock poisoned")?.take().unwrap_or_default();
    for &(id, px) in &rows {
        let e = &ids.entries[id as usize - 1];
        // An object's texture: its repeat from the triangles drawn with it this run (none when nothing was drawn with it).
        let repeat = e.repeat.or_else(|| objects.get(&e.stem).cloned().and_then(medians));
        let (ru, rv) = repeat.map_or((String::new(), String::new()), |(u, v)| (format!("{u:.4}"), format!("{v:.4}")));
        s += &format!("{}\t{}\t{}\t{}\t{px}\t{:.6}\t{ru}\t{rv}\n", e.stem, e.layer, e.width, e.height, px as f64 / total);
    }
    std::fs::write(out, &s).map_err(|e| format!("{out}: {e}"))?;
    let covered: u64 = rows.iter().map(|r| r.1).sum();
    println!("surfaces: level {level}: {} textures, {} drawn, {:.1}% of the picture; {out}", rows.len(), rows.iter().filter(|r| r.1 > 0).count(), covered as f64 / total * 100.0);
    for &(id, px) in rows.iter().take(10) {
        let e = &ids.entries[id as usize - 1];
        println!("  {:5.2}%  {} ({}, {} x {})", px as f64 / total * 100.0, e.stem, e.layer, e.width, e.height);
    }
    Ok(())
}

/// World size per repeat per texture, from triangles (`add`): the area-weighted medians of |dP/du| and |dP/dv|.
pub(crate) struct Repeat {
    samples: Vec<Vec<(f64, f64, f64)>>,
}

impl Repeat {
    pub fn new(textures: usize) -> Repeat {
        Repeat { samples: vec![Vec::new(); textures] }
    }

    pub fn add(&mut self, texture: usize, p: [[f32; 3]; 3], uv: [[f32; 2]; 3]) -> Result<(), String> {
        let s = self.samples.get_mut(texture).ok_or_else(|| format!("surfaces: texture {texture} past the layer's"))?;
        s.extend(sample(p, uv));
        Ok(())
    }

    pub fn medians(self) -> Vec<Option<(f64, f64)>> {
        self.samples.into_iter().map(medians).collect()
    }
}

/// One triangle's area and the world lengths of 1.0 of u and of v across it; None for a triangle whose coordinates do not span
/// the texture's plane (a line or a point of it), which gives no repeat.
fn sample(p: [[f32; 3]; 3], uv: [[f32; 2]; 3]) -> Option<(f64, f64, f64)> {
    let e1 = [0, 1, 2].map(|k| (p[1][k] - p[0][k]) as f64);
    let e2 = [0, 1, 2].map(|k| (p[2][k] - p[0][k]) as f64);
    let (d1u, d1v) = ((uv[1][0] - uv[0][0]) as f64, (uv[1][1] - uv[0][1]) as f64);
    let (d2u, d2v) = ((uv[2][0] - uv[0][0]) as f64, (uv[2][1] - uv[0][1]) as f64);
    let det = d1u * d2v - d2u * d1v;
    let cross = [e1[1] * e2[2] - e1[2] * e2[1], e1[2] * e2[0] - e1[0] * e2[2], e1[0] * e2[1] - e1[1] * e2[0]];
    let area = 0.5 * (cross[0] * cross[0] + cross[1] * cross[1] + cross[2] * cross[2]).sqrt();
    if det.abs() < 1e-9 || area <= 0.0 {
        return None;
    }
    let du = [0, 1, 2].map(|k| (e1[k] * d2v - e2[k] * d1v) / det);
    let dv = [0, 1, 2].map(|k| (e2[k] * d1u - e1[k] * d2u) / det);
    let len = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    Some((area, len(du), len(dv)))
}

/// The area-weighted medians of the samples' u and v lengths; None for no samples.
fn medians(mut s: Vec<(f64, f64, f64)>) -> Option<(f64, f64)> {
    if s.is_empty() {
        return None;
    }
    let half = s.iter().map(|x| x.0).sum::<f64>() / 2.0;
    let mut median = |k: usize| {
        s.sort_by(|a, b| (if k == 1 { a.1 } else { a.2 }).total_cmp(&(if k == 1 { b.1 } else { b.2 })));
        let mut acc = 0.0;
        for x in &s {
            acc += x.0;
            if acc >= half {
                return if k == 1 { x.1 } else { x.2 };
            }
        }
        unreachable!("the weights sum to twice half")
    };
    let u = median(1);
    let v = median(2);
    Some((u, v))
}

/// The samples of the objects' textures (`object_triangle`), by stem.
static OBJECTS: Mutex<Option<HashMap<String, Vec<(f64, f64, f64)>>>> = Mutex::new(None);

/// A triangle an object (a moby) drew in the world with texture `name`: its world positions and texture coordinates, for the
/// texture's world size per repeat (`write`).
pub(crate) fn object_triangle(name: crate::texnames::TexName, p: [[f32; 3]; 3], uv: [[f32; 2]; 3]) {
    if let Some(s) = sample(p, uv) {
        OBJECTS.lock().expect("surfaces: objects lock").get_or_insert_with(HashMap::new).entry(name.stem()).or_default().push(s);
    }
}
