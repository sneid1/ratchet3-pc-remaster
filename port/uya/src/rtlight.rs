//! Ray-traced lighting and reflections (PLAN 8.10): the `ray_traced_lighting` and `ray_traced_reflections` options
//! (`settings::OPTIONS`, `enhance::rt_lighting`, `enhance::rt_reflections`), searching the level's acceleration structures and
//! shading what the rays meet as the game draws it (`raytrace.rs`, its scene library `raytrace::scene`).
//!
//! **Lighting: the light one bounce brings, and the occlusion, traced** (`light`). The game's own lighting stays the base: its
//! vertex colours and light sets light every surface as before (per vertex or per pixel), and the bounce adds to it.
//! - From each pixel (half the picture's size; the 2 x 2 pixels' nearest), put back in the world from the depth buffer with its
//!   normal, `RAYS` rays (low 1, high 2) over the hemisphere, cosine-weighted, each pixel of a 4 x 4 tile taking its own
//!   sixteenth of the heights above the surface (Hammersley's) at an angle around it of its own, up to `REACH` units. What a ray meets
//!   gives the light the game drew it with: its texel times its vertex colours (the game's light sets and its baked light: the
//!   level's sun among them), shaded by the key light's shadow there when the shadows option is on (as the picture's surfaces
//!   are); the EE's layers it passes (`raytrace::scene`'s `scene_hit`: the water, the sea and the slime over what is behind
//!   them, the effects' light added) light it as the game draws them, so the sewers' glowing slime lights its walls and the
//!   walkway over it; a ray meeting nothing brings nothing (the sky's light is in the game's own). The mean is the bounce's
//!   light at the pixel (Lambert: the cosine weighting takes the cosine and the pi). Only a surface occludes, and only one the
//!   picture draws: a surface the structures hold in front of what the picture shows where the camera sees it (a tie triangle
//!   the game's own program leaves undrawn this frame as facing away; a tie drawn morphed off its LOD's mesh) is passed
//!   (`drawn_hit`). Met, those made a sharp patch of full shade in the bark's colour on Florana's trunks in another place every
//!   frame (the player, 16 November 2026: the flicker, "they move rapidly around based on ratchets position").
//! - The same rays give the ambient occlusion, in place of the screen-space one (`ao.rs`) when AMBIENT OCCLUSION is on: each
//!   meeting a surface within `OCCLUSION` units occludes by (1 - t / OCCLUSION)^2, darkening by `ao::STRENGTH` of it, as the
//!   screen-space one darkens (the contact shade), and within `WIDE` units by `WIDE_STRENGTH` of (1 - t / WIDE)^2 (the shade
//!   under counters and canopies and inside rooms), the two together darkening by at most `MOST_SHADE`; off screen and behind
//!   what the picture shows count too. (The player, 15 November 2026, on the first version, the bounce at 0.6 and the contact
//!   shade alone: "Dont really see a difference"; a Fable agent chose the full bounce and the wide shade with its cap.)
//! - Smoothed over 7 x 7 of those pixels and then over every other pixel of 9 x 9 (`denoise`: weighed by distance, by depth
//!   within 6% and by normal, with a small share of every weight by distance and rough depth alone so a curved close surface is
//!   smoothed too, and no sample above twice its neighbourhood's mean: no fireflies), the 4 x 4 tiles' directions together; one
//!   ray's light is capped (`FIREFLY`); the result is kept over frames (`TEMPORAL`: the last frame's reprojected by the cameras,
//!   rejected on depth or normal change, limited to the spread of the frame's own rays about the pixel, 0.9 kept), each frame's
//!   rays turned from the last's (`TRACE`'s frame), so the frames kept together average many directions; then each picture
//!   pixel takes the four nearest, weighed by place, depth and normal.
//! - Applied over the picture after the static world (every pixel) and after the moby runs (the mobies' pixels, the occluders'
//!   mask), as the occlusion and the shade are: the pixel + the bounce x the pixel's own surface (its texel, the vertex colours'
//!   hue: the eye's ray through the pixel meets it in the structures; a pixel whose surface they lack, an effect or a glow,
//!   takes no bounce; a metal, where the game draws its shine, scatters none: times 1 - the shine; a translucent pixel, glass, takes its share: times
//!   the surface's alpha, so a glass's sparkle texels are no white dots) x `BOUNCE`, faded by the
//!   frame's fog, then times 1 - the occlusion. The picture's alpha (the glow's) kept.
//!
//! **Reflections** (`gloss`, `water`).
//! - Shiny surfaces: where the game draws a shine (a tie packet's second part, its coordinates from the eye's reflection or
//!   the direction to a light: chrome, polished metal), the picture's pixel (half the picture's size on low, its size on high;
//!   the eye's ray meets its surface in the structures) reflects the ray about the surface's normal and traces it; what it meets,
//!   drawn as the game draws it with the EE's layers before it (water, slime, the effects' light) and fogged by the game's fog
//!   over the whole path, or where it meets nothing the sky the picture
//!   shows that way (nothing where the picture shows none), in the metal's own hue (its texel's), is mixed into the pixel by S
//!   = the shine there (the material's times the pixel's alpha, which the game's own shine adds by) x the metal's Fresnel term
//!   for the eye's angle (Schlick, `METAL_F0`) x `GLOSS`: the true reflection takes the place of the game's own shine (a fixed
//!   picture mapped by the eye's reflection, which it covers), keeping S's share of what was drawn. (The first version screened
//!   it over the game's shine at F0 0.6 and 0.35 or 0.6 of it, and the player saw no difference.)
//!   A Rift Apart surface (PLAN 9.2, `texnames::RiftMat`) reflects the same way by its own material: its shine here is its
//!   gloss (its map's blue; a surface of regions' materials by its region map, `raytrace` `NAME_surface`), its F0 its
//!   reflectance with its metalness (`texnames::RiftSlot::f0`), and the reflection takes its hue by its metalness: polished
//!   steel mirrors the room in its own colour, a worn paint shows a faint sheen toward grazing angles, a rough stone none.
//! - The water: the screen-space mirror (`reflections.rs`) makes the water's reflection as before; then each place of it whose
//!   water point (the eye's ray to the water's plane) lies where the picture can show water traces the mirrored ray, and where it
//!   meets a surface or one of the EE's layers (the effects, other water: from where the ray has left the water's own waves on)
//!   puts it there, drawn and fogged as above: what the screen-space mirror cannot show (above the
//!   picture's top, behind the camera, hidden behind what stands nearer) is there; where the ray meets nothing the mirror's sky
//!   stays. The water's own sheen draws it as before (`reflections::Reflections::draw`), so the water stays whole underneath.
//!
//! Every step is a command of the frame's recorded list (`replay::Cmd::RtLight`, `RtGloss`, `RtWater`). **The pictures between
//! two frames** (PLAN 8.8; 100 a second on this PC's display against the game's 60 frames) trace no lighting and no shiny
//! reflections of their own: the frame's own picture keeps what it traced (`Traced`, its textures taken from and given back to
//! `Passes::spare`), and each picture between frames applies it, every pixel taking what the frame traced where the frame's
//! camera saw the pixel's point (`in_frame`); a point the frame had hidden behind something nearer takes what it traced for the
//! same surface within 4 half-size pixels (a pan past a trunk showed the door behind it as a lighter patch until then). The
//! water's traced reflection is made in every picture, as the screen-space mirror under it is. **What the hardware is asked:**
//! a pixel's own surface is looked for only within three pixels' width of its depth (`seen`: the ray's interval, so the RT
//! cores search only the boxes there, not every leaf between the eye and it); a shadow ray stops at the first caster the
//! hardware confirms in the reach's first half (`scene_any`, `RAY_FLAG_TERMINATE_ON_FIRST_HIT`), and only past it is the
//! nearest one looked for, for the fade. Florana's frame, measured (`UYA_RT_TIMES`): the lighting 4.2 ms to 2.5, the shiny
//! surfaces 1.5 to 0.5, the shade 0.55 to 0.35, and nothing of the first two in a picture between frames.

use crate::native::Camera;
use crate::raytrace::{uniform, RtFrame, ENABLES};
use wgpu::util::DeviceExt;

/// The bounce's rays a pixel at the option's levels (low, high), how far they reach (world units), and how much of their light the
/// pixel's surface takes.
const RAYS: [u32; 2] = [1, 2];
const REACH: f32 = 24.0;
const BOUNCE: f32 = 1.0;
/// How near a surface occludes (world units: the contact shade, at `ao::STRENGTH`), how far the wide shade reaches and its
/// strength, and the most the two darken together.
const OCCLUSION: f32 = 2.0;
const WIDE: f32 = 6.0;
const WIDE_STRENGTH: f32 = 0.45;
const MOST_SHADE: f32 = 0.75;
/// How far a reflected ray reaches (world units).
const FAR: f32 = 800.0;
/// The reflections' strength on the game's shiny surfaces, and the metal's reflectance looking straight at it.
const GLOSS: f32 = 1.0;
const METAL_F0: f32 = 0.8;

const COMMON: &str = r#"
struct U {
    r: vec4<f32>, u: vec4<f32>, f: vec4<f32>, eye: vec4<f32>,
    // The camera's tangents and the picture's size.
    tan: vec4<f32>,
    // The depth buffer's near and far planes, the key light's darkness and its shadows' reach.
    nf: vec4<f32>,
    // The fog's near and far view depths and F at them.
    fog: vec4<f32>,
    // Toward the key light; w 1 when what the rays meet is shaded by it.
    to_light: vec4<f32>,
    // The fog's colour (bytes).
    fog_colour: vec4<f32>,
    // The pass's own: the lighting's bounce, occlusion strength, rays a pixel, reach; the reflections' strength, F0, reach.
    opt: vec4<f32>,
    // The pass's target size; the lighting's occlusion reach and wide shade's reach; the water's height and reach.
    size: vec4<f32>,
    // The lighting's wide shade's strength (0 with AMBIENT OCCLUSION off) and the most the shades darken together.
    opt2: vec4<f32>,
    // The camera the frame's own picture was traced from (`Traced`): right, up, forward, eye, tangents (the picture's own in a
    // frame's own picture).
    fr: vec4<f32>, fu: vec4<f32>, ff: vec4<f32>, feye: vec4<f32>, ftan: vec4<f32>,
};
@group(0) @binding(0) var<uniform> u: U;
// What a pass stores for the frames after it: a NaN or an infinity taken as nothing, the rest within the half float's range.
// One such texel (a ray met a light or a surface at no distance, a normal of no length, a sum past 65504 in the half floats)
// is kept by the history and spread by the smoothing and the reprojection, a few pixels a frame, until the whole picture is
// wrong and stays wrong while the camera keeps the history (the player, 18 November 2026: a spot on Veldin where "the screen
// gets completely" broken). The bits are tested, not `v != v`: a shader compiler may take every float as finite.
fn finite(v: vec4<f32>) -> vec4<f32> {
    let bad = (bitcast<vec4<u32>>(v) & vec4<u32>(0x7f800000u)) == vec4<u32>(0x7f800000u);
    return clamp(select(v, vec4<f32>(0.0), bad), vec4<f32>(-60000.0), vec4<f32>(60000.0));
}
@group(0) @binding(8) var raster_normal: texture_2d<f32>;
@group(0) @binding(9) var raster_bounce: texture_2d<f32>;
@group(0) @binding(10) var raster_hue: texture_2d<f32>;

fn raster_valid(at: vec2<i32>, p: vec3<f32>) -> bool {
    let z = dot(p - u.eye.xyz, u.f.xyz);
    let saved = textureLoad(raster_hue, at, 0).w;
    return saved > 0.0 && abs(saved - z) <= 0.01 + 0.0006 * z;
}

// Where the frame's camera saw world point `p`: its place in the frame's picture (pixels) and its view depth there.
fn in_frame(p: vec3<f32>) -> vec3<f32> {
    let rel = p - u.feye.xyz;
    let z = dot(rel, u.ff.xyz);
    let s = vec2<f32>(dot(rel, u.fr.xyz) / (z * u.ftan.x), dot(rel, u.fu.xyz) / (z * u.ftan.y));
    return vec3<f32>((s.x + 1.0) * 0.5 * u.tan.z, (1.0 - s.y) * 0.5 * u.tan.w, z);
}

fn view_z(d: f32) -> f32 {
    return u.nf.x * u.nf.y / (u.nf.x + d * (u.nf.y - u.nf.x));
}

// The world position of the picture's pixel at `px` with depth `d` (`native::Camera::matrix` inverted).
fn world(px: vec2<f32>, d: f32) -> vec3<f32> {
    let z = view_z(d);
    let ndc = vec2<f32>(px.x / u.tan.z * 2.0 - 1.0, 1.0 - px.y / u.tan.w * 2.0);
    return u.eye.xyz + u.r.xyz * (ndc.x * u.tan.x * z) + u.u.xyz * (ndc.y * u.tan.y * z) + u.f.xyz * z;
}

// The width of a picture pixel one unit from the eye.
fn pixel_angle() -> f32 {
    return 2.0 * u.tan.y / u.tan.w;
}

// The share of a pixel at view depth `vz` that is not fog (F / 255), as the occlusion's and the shade's.
fn clear_at(vz: f32) -> f32 {
    let f = u.fog.z + (u.fog.w - u.fog.z) * (vz - u.fog.x) / (u.fog.y - u.fog.x);
    return clamp(f, min(u.fog.z, u.fog.w), max(u.fog.z, u.fog.w)) / 255.0;
}

// Colour `c` seen `dist` away along a ray, fogged as the GS fogs a pixel at that view depth: (F x c + (255 - F) x FOGCOL) / 256.
fn fogged(c: vec3<f32>, dist: f32) -> vec3<f32> {
    let f = clear_at(dist) * 255.0;
    return (c * 255.0 * f + u.fog_colour.rgb * (255.0 - f)) / (256.0 * 255.0);
}

"#;

/// What the steps that search the structures share besides `COMMON`: the key light's shade where a ray met a surface, and the
/// pixel's own surface.
const SCENE_HELP: &str = r#"
// How much of the key light's light a surface the rays met keeps: its shadow's darkness where something casts on it within the
// shadows' reach, as the ray-traced shade (`raytrace.rs`) darkens the picture's own; all of it when the shadows option is off.
fn sun(s: Surface) -> f32 {
    if (u.to_light.w < 0.5) {
        return 1.0;
    }
    let facing = dot(s.n, u.to_light.xyz);
    if (facing <= 0.0) {
        return 1.0;
    }
    // As the shade: anything in the reach's first half shades fully (the first hit ends the search), else the nearest in its
    // second half fades it.
    let reach = u.nf.w;
    let o = s.p + s.n * (0.02 + 0.002 * s.t);
    var lit = 0.0;
    if (!scene_any(o, u.to_light.xyz, 0.5 * reach)) {
        lit = smoothstep(0.5 * reach, reach, scene_t_from(o, u.to_light.xyz, 0.5 * reach, reach));
    }
    return 1.0 - u.nf.z * (1.0 - lit) * smoothstep(0.0, 0.35, facing);
}

// The surface the eye's ray through the picture's pixel at world position `p` meets in the structures within three pixels'
// width of its depth (the hardware searches only the boxes there), else none.
fn seen(p: vec3<f32>) -> Surface {
    let dist = length(p - u.eye.xyz);
    let dir = (p - u.eye.xyz) / dist;
    let foot = 3.0 * dist * pixel_angle() + 0.02;
    return scene_surface(u.eye.xyz, dir, max(dist - foot, 0.0), dist + foot, 0.0, pixel_angle());
}
"#;

/// The bounce's and the occlusion's rays (`light`): into `gi` (the bounce's light and the occlusion) and `geo` (the normal and
/// the view depth; w 0 where the picture shows the sky), half the picture's size.
const TRACE: &str = r#"
// The most light one ray brings (per colour channel; the bounce is the mean of the pixel's rays).
const FIREFLY: f32 = 0.9;
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var gi: texture_storage_2d<rgba16float, write>;
@group(0) @binding(3) var geo: texture_storage_2d<rgba32float, write>;

fn load_d(p: vec2<i32>) -> f32 {
    return textureLoad(depth, clamp(p, vec2<i32>(0), vec2<i32>(u.tan.zw) - vec2<i32>(1)), 0);
}

fn world_at(q: vec2<i32>) -> vec3<f32> {
    return world(vec2<f32>(q) + vec2<f32>(0.5), load_d(q));
}

fn hash(p: vec2<u32>) -> f32 {
    var h = p.x * 0x8da6b343u ^ p.y * 0xd8163841u;
    h = (h ^ (h >> 15u)) * 0x2c1b3c6du;
    h = h ^ (h >> 12u);
    return f32(h & 0xffffffu) / 16777216.0;
}

// How far off its surface a pixel's rays start (along its normal, at view depth `z`): a pixel and a half and a little more.
fn lift(z: f32) -> f32 {
    return 0.01 + 1.5 * z * pixel_angle();
}

// Whether a surface the rays met at world point `h` is one the picture draws: not in front of what the picture shows where the
// camera sees `h` (the nearest of the 3 x 3 pixels there, less half a ray's lift). A surface the structures hold in front of it is
// one the picture leaves out this frame (a tie triangle the game's program left undrawn as facing away, which the structures
// keep: rays from the surface the picture shows behind it met it at once, a sharp patch of full shade in the bark's colour, in
// another place every frame), and the rays pass it. Behind the eye, off the picture or behind what it shows, a surface stands.
fn drawn_at(h: vec3<f32>) -> bool {
    let rel = h - u.eye.xyz;
    let z = dot(rel, u.f.xyz);
    if (z <= u.nf.x) {
        return true;
    }
    let s = vec2<f32>(dot(rel, u.r.xyz) / (z * u.tan.x), dot(rel, u.u.xyz) / (z * u.tan.y));
    if (abs(s.x) >= 1.0 || abs(s.y) >= 1.0) {
        return true;
    }
    let c = vec2<i32>(vec2<f32>((s.x + 1.0) * 0.5 * u.tan.z, (1.0 - s.y) * 0.5 * u.tan.w));
    var near = 0.0;
    for (var dy = -1; dy <= 1; dy = dy + 1) {
        for (var dx = -1; dx <= 1; dx = dx + 1) {
            near = max(near, load_d(c + vec2<i32>(dx, dy)));
        }
    }
    // The picture shows only the sky there: nothing it draws stands at `h`.
    if (near <= 0.0) {
        return false;
    }
    let zb = view_z(near);
    return z >= zb - 0.5 * lift(zb);
}

// What a ray from `o` along `d` meets that the picture draws (`drawn_at`; past up to two surfaces it does not), as `scene_hit`.
fn drawn_hit(o: vec3<f32>, d: vec3<f32>, tmax: f32, w0: f32, spread: f32) -> Surface {
    var s = scene_hit(o, d, tmax, w0, spread, 0.0);
    var past = 0.0;
    for (var k = 0; k < 2; k = k + 1) {
        if (s.hit == 0u || s.layer != 0u || drawn_at(s.p)) {
            return s;
        }
        // s.t is already measured from the original origin below. Adding `past` again skips real geometry after the second
        // rejected surface, and can even put the next origin beyond tmax.
        past = s.t + 0.01;
        if (past >= tmax) {
            var none: Surface;
            return none;
        }
        s = scene_hit(o + d * past, d, tmax - past, w0 + spread * past, spread, 0.0);
        s.t = s.t + past;
    }
    // Exhausting the skip budget does not make the last invisible triangle a valid occluder.
    if (s.hit != 0u && s.layer == 0u && !drawn_at(s.p)) {
        var none: Surface;
        return none;
    }
    return s;
}

@compute @workgroup_size(8, 8) fn trace(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= u.size.x || f32(id.y) >= u.size.y) {
        return;
    }
    // Of the picture's 2 x 2 pixels, the nearest.
    let q0 = vec2<i32>(id.xy) * 2;
    var q = q0;
    var d = load_d(q0);
    for (var k = 1; k < 4; k = k + 1) {
        let c = q0 + vec2<i32>(k & 1, k >> 1);
        let dc = load_d(c);
        if (dc > d) {
            d = dc;
            q = c;
        }
    }
    if (d <= 0.0) {
        textureStore(gi, id.xy, vec4<f32>(0.0));
        textureStore(geo, id.xy, vec4<f32>(0.0));
        return;
    }
    let p = world(vec2<f32>(q) + vec2<f32>(0.5), d);
    // The surface's normal from the nearer neighbour on each axis, turned to the eye (as the shade's).
    let pl = world_at(q - vec2<i32>(1, 0));
    let pr = world_at(q + vec2<i32>(1, 0));
    let pu = world_at(q - vec2<i32>(0, 1));
    let pd = world_at(q + vec2<i32>(0, 1));
    let dx = select(pr - p, p - pl, length(pl - p) < length(pr - p));
    let dy = select(pd - p, p - pu, length(pu - p) < length(pd - p));
    var n = normalize(cross(dx, dy));
    if (dot(n, u.eye.xyz - p) < 0.0) {
        n = -n;
    }
    if (raster_valid(q, p)) { n = normalize(textureLoad(raster_normal, q, 0).xyz); }
    let vz = dot(p - u.eye.xyz, u.f.xyz);
    let o = p + n * lift(vz);
    let a = select(vec3<f32>(1.0, 0.0, 0.0), vec3<f32>(0.0, 1.0, 0.0), abs(n.x) > 0.9);
    let tx = normalize(cross(n, a));
    let ty = cross(n, tx);
    let rays = u32(u.opt.z);
    let all = 16u * rays;
    // The frame's own directions (`opt2.z`, the frames traced): each pixel's sixteenth of the heights moves on by 5 of the 4 x 4
    // tile's sixteen a frame (every tile still takes all sixteen), and its angle around the normal by the golden ratio's share of
    // a turn, so the frames kept together (`TEMPORAL`) are many rays a pixel, not the same two again (a pattern fixed to the
    // screen, which slid over the walls and rocks as the camera turned: the haze).
    let frame = u32(u.opt2.z);
    let j = ((id.x & 3u) + 4u * (id.y & 3u) + 5u * frame) & 15u;
    let turn = 6.2831853 * fract(hash(id.xy) + 0.618034 * f32(frame & 1023u));
    var e = vec3<f32>(0.0);
    var occ = 0.0;
    var wide = 0.0;
    for (var k = 0u; k < rays; k = k + 1u) {
        let i = j + 16u * k;
        let u1 = (f32(i) + 0.5) / f32(all);
        let phi = 6.2831853 * f32(reverseBits(i)) * 2.3283064e-10 + turn;
        let r = sqrt(u1);
        let dir = normalize(tx * (r * cos(phi)) + ty * (r * sin(phi)) + n * sqrt(max(1.0 - u1, 0.0)));
        // What the ray sees, its texture blurred to a quarter of the distance (a bounce's light is soft): a surface with the EE's
        // layers before it (water, slime, the effects' light), or a layer alone; only a surface occludes, and only one the picture
        // draws (`drawn_hit`).
        let s = drawn_hit(o, dir, u.opt.w, 0.05, 0.25);
        var er = s.add;
        if (s.hit != 0u) {
            er = er + s.colour * sun(s);
        }
        // One ray's light is limited (a lit white surface or a glow met by one sample of a pixel is a firefly: the white dots).
        e = e + min(er, vec3<f32>(FIREFLY));
        if (s.hit != 0u && s.layer == 0u) {
            if (s.t < u.size.z) {
                let near = 1.0 - s.t / u.size.z;
                occ = occ + near * near;
            }
            if (s.t < u.size.w) {
                let far = 1.0 - s.t / u.size.w;
                wide = wide + far * far;
            }
        }
    }
    // The shade: the contact shade at the occlusion's strength (`opt.y`, 0 with AMBIENT OCCLUSION off) and the wide one, together
    // at most `opt2.y`.
    let shade = min((u.opt.y * occ + u.opt2.x * wide) / f32(rays), u.opt2.y);
    textureStore(gi, id.xy, finite(vec4<f32>(e / f32(rays), shade)));
    // The ray starts at the nearest full-size pixel, but history reconstructs the half-size texel's centre. Store the depth
    // of that centre on the same surface plane; copying the corner's depth shifts sloping surfaces when reprojected.
    let centre = world(vec2<f32>(q0) + vec2<f32>(1.0), d) - u.eye.xyz;
    let denom = dot(n, centre);
    let centre_z = select(vz, vz * dot(n, p - u.eye.xyz) / denom, abs(denom) > 1e-5);
    textureStore(geo, id.xy, finite(vec4<f32>(n, centre_z)));
}
"#;

/// The bounce and occlusion smoothed (`light`): each half-size pixel the mean of its 7 x 7 neighbours `size.z` apart (1, then 2
/// over 5 x 5) of about its depth and normal, weighed by distance.
const DENOISE: &str = r#"
struct U { size: vec4<f32> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var gi: texture_2d<f32>;
@group(0) @binding(2) var geo: texture_2d<f32>;
@group(0) @binding(3) var out: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8) fn denoise(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(u.size.xy);
    let at = vec2<i32>(id.xy);
    if (at.x >= size.x || at.y >= size.y) {
        return;
    }
    let c = textureLoad(geo, at, 0);
    if (c.w <= 0.0) {
        textureStore(out, at, vec4<f32>(0.0));
        return;
    }
    let step = i32(u.size.z);
    let r = i32(u.size.w);
    // The neighbourhood's mean by distance and rough depth alone (a curved, close surface's normals and depths give a pixel no
    // neighbour of its own facing: its sample would stand alone): what a single sample is limited to, and a small share of every
    // weight, so no pixel is left unsmoothed.
    var around = vec4<f32>(0.0);
    var around_w = 0.0;
    for (var dy = -r; dy <= r; dy = dy + 1) {
        for (var dx = -r; dx <= r; dx = dx + 1) {
            let q = at + vec2<i32>(dx, dy) * step;
            if (q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y) {
                continue;
            }
            let s = textureLoad(geo, q, 0);
            if (s.w <= 0.0) {
                continue;
            }
            let w = exp(-f32(dx * dx + dy * dy) / 8.0) * exp(-abs(s.w - c.w) / (0.3 * c.w + 0.05));
            around = around + textureLoad(gi, q, 0) * w;
            around_w = around_w + w;
        }
    }
    around = around / around_w;
    let most = around.rgb * 2.0 + vec3<f32>(0.03);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var dy = -r; dy <= r; dy = dy + 1) {
        for (var dx = -r; dx <= r; dx = dx + 1) {
            let q = at + vec2<i32>(dx, dy) * step;
            if (q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y) {
                continue;
            }
            let s = textureLoad(geo, q, 0);
            if (s.w <= 0.0) {
                continue;
            }
            let near = exp(-f32(dx * dx + dy * dy) / 8.0);
            let w = near * (exp(-abs(s.w - c.w) / (0.06 * c.w + 0.05)) * pow(max(dot(s.xyz, c.xyz), 0.0), 4.0) + 0.1 * exp(-abs(s.w - c.w) / (0.3 * c.w + 0.05)));
            var v = textureLoad(gi, q, 0);
            v = vec4<f32>(min(v.rgb, most), v.a);
            sum = sum + v * w;
            weight = weight + w;
        }
    }
    textureStore(out, at, select(vec4<f32>(0.0), sum / weight, weight > 0.0));
}
"#;

/// The smoothed light kept over frames (`light`): each half-size pixel's point put back in the last frame's picture
/// (`in_frame`, the last frame's camera in the uniform's frame fields), the light and occlusion kept there taken in by the shader's
/// `KEEP` where the last frame saw the point at its depth and facing (bilinear); a point it did not see takes the new light alone.
/// What is kept is limited to the spread of this frame's own rays about the pixel (their mean over 5 x 5 pixels of about its
/// depth, `SPREAD` of their standard deviation either side), so a change in the light leaves no trail while a frame's own noise
/// (its rays are its own: `TRACE`'s frame) is averaged away. The first version limited it to the 3 x 3 neighbours' smoothed
/// light: with every frame's rays the same, nothing was averaged, and the smoothing's blotches stayed on the screen, sliding over
/// walls and rocks as the camera turned (the player, 16 November 2026: "a weird texture on this rock and this blue wall that
/// moves around as you look around on it").
const TEMPORAL: &str = r#"
const KEEP: f32 = 0.9;
const SPREAD: f32 = 1.0;
@group(0) @binding(1) var cur: texture_2d<f32>;
@group(0) @binding(2) var cur_geo: texture_2d<f32>;
@group(0) @binding(3) var old: texture_2d<f32>;
@group(0) @binding(4) var old_geo: texture_2d<f32>;
@group(0) @binding(5) var out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(6) var raw: texture_2d<f32>;

@compute @workgroup_size(8, 8) fn temporal(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = vec2<i32>(u.size.xy);
    let at = vec2<i32>(id.xy);
    if (at.x >= size.x || at.y >= size.y) {
        return;
    }
    let c = textureLoad(cur, at, 0);
    let g = textureLoad(cur_geo, at, 0);
    if (g.w <= 0.0) {
        textureStore(out, at, finite(c));
        return;
    }
    // This frame's rays about the pixel, of about its depth: their mean and standard deviation.
    var m1 = vec4<f32>(0.0);
    var m2 = vec4<f32>(0.0);
    var n = 0.0;
    for (var dy = -2; dy <= 2; dy = dy + 1) {
        for (var dx = -2; dx <= 2; dx = dx + 1) {
            let q = clamp(at + vec2<i32>(dx, dy), vec2<i32>(0), size - vec2<i32>(1));
            let s = textureLoad(cur_geo, q, 0);
            if (s.w <= 0.0 || abs(s.w - g.w) > 0.06 * g.w + 0.05) {
                continue;
            }
            let v = textureLoad(raw, q, 0);
            m1 = m1 + v;
            m2 = m2 + v * v;
            n = n + 1.0;
        }
    }
    let mean = m1 / n;
    let sd = sqrt(max(m2 / n - mean * mean, vec4<f32>(0.0)));
    let lo = min(mean - SPREAD * sd, c);
    let hi = max(mean + SPREAD * sd, c);
    let scale = u.tan.zw / u.size.xy;
    let px = (vec2<f32>(at) + vec2<f32>(0.5)) * scale;
    let ndc = vec2<f32>(px.x / u.tan.z * 2.0 - 1.0, 1.0 - px.y / u.tan.w * 2.0);
    let p = u.eye.xyz + u.r.xyz * (ndc.x * u.tan.x * g.w) + u.u.xyz * (ndc.y * u.tan.y * g.w) + u.f.xyz * g.w;
    let fp = in_frame(p);
    var acc = vec4<f32>(0.0);
    var seen = 0.0;
    if (fp.z > 0.0) {
        let h = fp.xy / scale - vec2<f32>(0.5);
        let h0 = vec2<i32>(floor(h));
        let fr = h - floor(h);
        for (var k = 0; k < 4; k = k + 1) {
            let off = vec2<i32>(k & 1, k >> 1);
            let q = h0 + off;
            if (q.x < 0 || q.y < 0 || q.x >= size.x || q.y >= size.y) {
                continue;
            }
            let s = textureLoad(old_geo, q, 0);
            if (s.w <= 0.0 || abs(s.w - fp.z) > 0.02 * fp.z + 0.05 || dot(s.xyz, g.xyz) < 0.8) {
                continue;
            }
            let wb = select(1.0 - fr.x, fr.x, off.x == 1) * select(1.0 - fr.y, fr.y, off.y == 1);
            let w = wb;
            acc = acc + textureLoad(old, q, 0) * w;
            seen = seen + w;
        }
    }
    if (seen < 0.05) {
        textureStore(out, at, finite(c));
        return;
    }
    let kept = clamp(acc / seen, lo, hi);
    // Reject other surfaces above, then normalize the surviving bilinear taps. Multiplying KEEP by their similarity again
    // discarded most history on curved surfaces whenever the camera moved, even when the taps were on the same surface.
    textureStore(out, at, finite(mix(c, kept, KEEP)));
}
"#;

/// The bounce and occlusion over the picture (`light`): the pixel + the bounce x its surface's own colour, times 1 - the
/// occlusion (dual-source: the colour added, the factor the picture is multiplied by).
const APPLY: &str = r#"
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var gi: texture_2d<f32>;
@group(0) @binding(3) var geo: texture_2d<f32>;
@group(0) @binding(4) var mask: texture_2d<f32>;

struct FOut { @location(0) @blend_src(0) add: vec4<f32>, @location(0) @blend_src(1) times: vec4<f32> };

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

fn load_d(p: vec2<i32>) -> f32 {
    return textureLoad(depth, clamp(p, vec2<i32>(0), vec2<i32>(u.tan.zw) - vec2<i32>(1)), 0);
}

fn world_at(q: vec2<i32>) -> vec3<f32> {
    return world(vec2<f32>(q) + vec2<f32>(0.5), load_d(q));
}

fn light(at: vec2<f32>) -> FOut {
    var o: FOut;
    o.add = vec4<f32>(0.0);
    o.times = vec4<f32>(1.0);
    let q = vec2<i32>(at);
    let d = load_d(q);
    if (d <= 0.0) {
        return o;
    }
    let p = world(at, d);
    let vz = dot(p - u.eye.xyz, u.f.xyz);
    let pl = world_at(q - vec2<i32>(1, 0));
    let pr = world_at(q + vec2<i32>(1, 0));
    let pu = world_at(q - vec2<i32>(0, 1));
    let pd = world_at(q + vec2<i32>(0, 1));
    let dx = select(pr - p, p - pl, length(pl - p) < length(pr - p));
    let dy = select(pd - p, p - pu, length(pu - p) < length(pd - p));
    var n = normalize(cross(dx, dy));
    if (dot(n, u.eye.xyz - p) < 0.0) {
        n = -n;
    }
    // Use the same normal as TRACE so smooth character/prop normals retain their own lighting history.
    if (raster_valid(q, p)) { n = normalize(textureLoad(raster_normal, q, 0).xyz); }
    // The four nearest half-size pixels about where the frame's camera saw the point (the pixel itself in the frame's own
    // picture), by place, view depth and normal.
    let size = vec2<i32>(u.size.xy);
    let fp = in_frame(p);
    let h = fp.xy * 0.5 - vec2<f32>(0.5);
    let h0 = vec2<i32>(floor(h));
    let fr = h - floor(h);
    var g = vec4<f32>(0.0);
    var weight = 0.0;
    // How much of the four the frame saw at the point's own depth and facing.
    var matched = 0.0;
    for (var k = 0; k < 4; k = k + 1) {
        let off = vec2<i32>(k & 1, k >> 1);
        let c = clamp(h0 + off, vec2<i32>(0), size - vec2<i32>(1));
        let s = textureLoad(geo, c, 0);
        if (s.w <= 0.0) {
            continue;
        }
        let wb = select(1.0 - fr.x, fr.x, off.x == 1) * select(1.0 - fr.y, fr.y, off.y == 1) + 0.001;
        let m = exp(-abs(s.w - fp.z) / (0.03 * fp.z + 0.05)) * pow(max(dot(s.xyz, n), 0.0), 8.0);
        let w = wb * m + 0.0001 * wb;
        matched = matched + wb * m;
        g = g + textureLoad(gi, c, 0) * w;
        weight = weight + w;
    }
    // A point the frame's camera did not see there (in a picture between frames, a place the frame had hidden behind something
    // nearer, which the camera has since moved past: the door behind a trunk in a pan): the half-size pixels within 4 of it that
    // the frame saw at its depth and facing, the nearer the more (the bounce and the shade change little over a surface).
    if (matched < 0.02) {
        let c0 = vec2<i32>(round(h));
        var near = vec4<f32>(0.0);
        var near_w = 0.0;
        for (var dy = -4; dy <= 4; dy = dy + 1) {
            for (var dx = -4; dx <= 4; dx = dx + 1) {
                let c = clamp(c0 + vec2<i32>(dx, dy), vec2<i32>(0), size - vec2<i32>(1));
                let s = textureLoad(geo, c, 0);
                if (s.w <= 0.0) {
                    continue;
                }
                let m = exp(-abs(s.w - fp.z) / (0.03 * fp.z + 0.05)) * pow(max(dot(s.xyz, n), 0.0), 8.0);
                if (m < 0.5) {
                    continue;
                }
                let w = m / f32(1 + dx * dx + dy * dy);
                near = near + textureLoad(gi, c, 0) * w;
                near_w = near_w + w;
            }
        }
        if (near_w > 0.0) {
            g = near;
            weight = near_w;
        }
    }
    if (weight <= 0.0) {
        return o;
    }
    g = g / weight;
    let clear = clear_at(vz);
    let pixel = vec2<i32>(at);
    if (raster_valid(pixel, p)) {
        o.add = vec4<f32>(g.rgb * textureLoad(raster_bounce, pixel, 0).rgb * u.opt.x * clear, 0.0);
    } else {
        let s = seen(p);
        if (s.hit != 0u) {
            let tint = s.vertex / max(max(s.vertex.r, max(s.vertex.g, s.vertex.b)), 1e-3);
            o.add = vec4<f32>(g.rgb * s.texel * tint * (1.0 - s.shine * s.metal) * s.alpha * u.opt.x * clear, 0.0);
        }
    }
    let m = 1.0 - g.a * clear;
    o.times = vec4<f32>(m, m, m, 1.0);
    return o;
}

@fragment fn fs_static(@builtin(position) at: vec4<f32>) -> FOut {
    return light(at.xy);
}

@fragment fn fs_mobies(@builtin(position) at: vec4<f32>) -> FOut {
    if (textureLoad(mask, vec2<i32>(at.xy), 0).r > 0.5) {
        var o: FOut;
        o.add = vec4<f32>(0.0);
        o.times = vec4<f32>(1.0);
        return o;
    }
    return light(at.xy);
}
"#;

/// The reflections on the game's shiny surfaces (`gloss`): into `out` (the reflection, and in alpha its weight), at the target's
/// size.
const GLOSS_TRACE: &str = r#"
// Later EE layers can replace depth without being an opaque world surface. Only
// those pixels still need a camera ray; ordinary rasterized world pixels do not.
fn primary_surface(p: vec3<f32>, at: vec2<i32>) -> Surface {
    if (!raster_valid(at, p)) { return seen(p); }
    let n = textureLoad(raster_normal, at, 0);
    let b = textureLoad(raster_bounce, at, 0);
    let h = textureLoad(raster_hue, at, 0);
    var s: Surface;
    s.hit = 1u; s.p = p; s.t = length(p - u.eye.xyz);
    s.n = normalize(n.xyz); s.shine = n.w; s.f0 = b.w;
    s.texel = h.rgb; s.metal = 1.0; s.alpha = 1.0;
    return s;
}



@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var picture: texture_2d<f32>;
@group(0) @binding(3) var out: texture_storage_2d<rgba16float, write>;
@group(0) @binding(4) var geo: texture_storage_2d<rgba32float, write>;

fn load_d(p: vec2<i32>) -> f32 {
    return textureLoad(depth, clamp(p, vec2<i32>(0), vec2<i32>(u.tan.zw) - vec2<i32>(1)), 0);
}

// What the picture shows along direction `r` from the eye where it shows the sky; alpha 0 where it shows none.
fn sky(r: vec3<f32>) -> vec4<f32> {
    let ahead = dot(r, u.f.xyz);
    if (ahead <= 0.0) {
        return vec4<f32>(0.0);
    }
    let s = vec2<f32>(dot(r, u.r.xyz) / (ahead * u.tan.x), dot(r, u.u.xyz) / (ahead * u.tan.y));
    if (abs(s.x) >= 1.0 || abs(s.y) >= 1.0) {
        return vec4<f32>(0.0);
    }
    let t = vec2<i32>(vec2<f32>((s.x + 1.0) * 0.5 * u.tan.z, (1.0 - s.y) * 0.5 * u.tan.w));
    if (load_d(t) > 0.0) {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(textureLoad(picture, t, 0).rgb, 1.0);
}

// The share of light a GGX surface of reflectance `f0` and perceptual roughness `rough` reflects toward the eye at `nv` (the
// split-sum's directional albedo, Karis's analytic fit). The trace's one mirror ray stood for the whole lobe at gloss x
// Schlick: a rough stone or dirt (gloss 0.1 to 0.5) took 20 to 80 % of a sharp image of the scene at grazing angles, where the
// ground is seen. A rough dielectric reflects some 4 % at every angle; a smooth one keeps Schlick's rise.
fn env_brdf(f0: f32, rough: f32, nv: f32) -> f32 {
    let r = rough * vec4<f32>(-1.0, -0.0275, -0.572, 0.022) + vec4<f32>(1.0, 0.0425, 1.04, -0.04);
    let a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
    let ab = vec2<f32>(-1.04, 1.04) * a004 + r.zw;
    return clamp(f0 * ab.x + ab.y, 0.0, 1.0);
}

@compute @workgroup_size(8, 8) fn gloss(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= u.size.x || f32(id.y) >= u.size.y) {
        return;
    }
    let at = (vec2<f32>(id.xy) + vec2<f32>(0.5)) * u.tan.zw / u.size.xy;
    textureStore(geo, id.xy, vec4<f32>(0.0));
    let d = load_d(vec2<i32>(at));
    if (d <= 0.0) {
        textureStore(out, id.xy, vec4<f32>(0.0));
        return;
    }
    let p = world(at, d);
    let s = primary_surface(p, vec2<i32>(at));
    if (s.hit == 0u || s.shine <= 0.0) {
        textureStore(out, id.xy, vec4<f32>(0.0));
        return;
    }
    let v = normalize(p - u.eye.xyz);
    let wide = s.t * pixel_angle();
    // Sample four parts of the reflected pixel over successive frames on HIGH, integrated by the reflection history.
    // Texture mipmaps cannot filter thin geometry such as aiming rings. Keep one ray per frame, including on LOW.
    let sample_id = u32(u.opt2.z) & 3u;
    let offset = select(vec2<f32>(0.0), (vec2<f32>(f32(sample_id & 1u), f32(sample_id >> 1u)) - 0.5) * 0.5, u.opt.z > 1.0);
    let sample_ray = world(at + offset * u.tan.zw / u.size.xy, d) - u.eye.xyz;
    let dir = normalize(sample_ray);
    let r = reflect(dir, s.n);
    // Intersect the offset camera ray with the surface plane. Orthogonally projecting a constant-depth offset makes the
    // footprint collapse on surfaces seen at a grazing angle.
    let denominator = dot(s.n, sample_ray);
    var sample_p = s.p;
    if (abs(denominator) > 1e-5) {
        sample_p = u.eye.xyz + sample_ray * (dot(s.n, s.p - u.eye.xyz) / denominator);
    }
    let origin = sample_p + s.n * (0.01 + 1.5 * wide);
    let h = scene_hit(origin, r, u.opt.w, wide, pixel_angle(), 0.0);
    var c: vec4<f32>;
    if (h.hit != 0u) {
        c = vec4<f32>(fogged(h.colour * sun(h), s.t + h.t), 1.0);
    } else {
        c = sky(r);
    }
    // An additive ring or flash exists even if the ray finds no opaque background and no on-screen sky. Keep its light
    // independently of the background's coverage; otherwise turning the camera switches the whole reflection on and off.
    let colour = min(c.rgb + h.add * clear_at(s.t + select(u.opt.w, h.t, h.hit != 0u)), vec3<f32>(1.0));
    let cosv = clamp(dot(-v, s.n), 0.0, 1.0);
    // The surface's own reflectance (a Rift Apart material's), else the game's chrome's: the pass's metal F0.
    let f0 = select(u.opt.y, s.f0, s.f0 >= 0.0);
    let f = f0 + (1.0 - f0) * pow(1.0 - cosv, 5.0);
    // A metal's reflection takes its hue, a paint's or a stone's does not.
    let hue = mix(vec3<f32>(1.0), s.texel / max(max(s.texel.r, max(s.texel.g, s.texel.b)), 1e-3), s.metal);
    // A Rift Apart surface reflects what a GGX surface of its roughness (1 - its gloss) does (`env_brdf`); the game's chrome, a
    // mirror, by its shine and Schlick's Fresnel.
    let weight = select(s.shine * f, env_brdf(s.f0, 1.0 - s.shine, cosv), s.f0 >= 0.0) * u.opt.x;
    textureStore(out, id.xy, finite(vec4<f32>(colour * hue * weight, c.a * weight)));
    textureStore(geo, id.xy, finite(vec4<f32>(s.n, dot(s.p - u.eye.xyz, u.f.xyz))));
}
"#;

/// The shiny surfaces' reflection mixed into the picture by its weight (`gloss`).
const GLOSS_APPLY: &str = r#"
@group(0) @binding(1) var refl: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
@group(0) @binding(3) var depth: texture_depth_2d;
@group(0) @binding(4) var geo: texture_2d<f32>;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
}

// The reflection the frame's own picture traced where the frame's camera saw the pixel's point.
@fragment fn fs(@builtin(position) at: vec4<f32>) -> @location(0) vec4<f32> {
    let d = textureLoad(depth, vec2<i32>(at.xy), 0);
    if (d <= 0.0) {
        return vec4<f32>(0.0);
    }
    let fp = in_frame(world(at.xy, d));
    if (fp.z <= 0.0 || any(fp.xy < vec2<f32>(0.0)) || any(fp.xy >= u.tan.zw)) {
        return vec4<f32>(0.0);
    }
    let q = vec2<i32>(fp.xy * vec2<f32>(textureDimensions(geo)) / u.tan.zw);
    let g = textureLoad(geo, q, 0);
    if (g.w <= 0.0 || abs(g.w - fp.z) > 0.02 * fp.z + 0.05) {
        return vec4<f32>(0.0);
    }
    return textureSampleLevel(refl, smp, fp.xy / u.tan.zw, 0.0);
}
"#;

/// The water's reflection traced (`water`): each place of the reflections' picture (`reflections.rs`) whose water point the
/// picture can show takes the surface the mirrored ray meets.
const WATER: &str = r#"
@group(0) @binding(1) var depth: texture_depth_2d;
@group(0) @binding(2) var refl: texture_storage_2d<rgba8unorm, write>;

fn load_d(p: vec2<i32>) -> f32 {
    return textureLoad(depth, clamp(p, vec2<i32>(0), vec2<i32>(u.tan.zw) - vec2<i32>(1)), 0);
}

@compute @workgroup_size(8, 8) fn water(@builtin(global_invocation_id) id: vec3<u32>) {
    if (f32(id.x) >= u.size.x || f32(id.y) >= u.size.y) {
        return;
    }
    let at = (vec2<f32>(id.xy) + vec2<f32>(0.5)) * u.tan.zw / u.size.xy;
    let ndc = vec2<f32>(at.x / u.tan.z * 2.0 - 1.0, 1.0 - at.y / u.tan.w * 2.0);
    let ray = u.f.xyz + u.r.xyz * ndc.x * u.tan.x + u.u.xyz * ndc.y * u.tan.y;
    if (ray.z >= -1e-4) {
        return;
    }
    let k = (u.size.z - u.eye.z) / ray.z;
    if (k <= 0.0) {
        return;
    }
    // Water can be drawn here only over what lies below its top (or over nothing).
    let d = load_d(vec2<i32>(at));
    if (d > 0.0 && world(at, d).z > u.size.z + u.size.w) {
        return;
    }
    let w = u.eye.xyz + ray * k;
    let dist = k * length(ray);
    let dir = normalize(vec3<f32>(ray.x, ray.y, -ray.z));
    let wide = dist * pixel_angle() * u.tan.z / u.size.x;
    // The EE's layers from where the mirrored ray has left the water's own waves (its reach over the ray's climb).
    let near = (2.0 * u.size.w + 0.05) / max(dir.z, 0.02);
    let h = scene_hit(w + vec3<f32>(0.0, 0.0, 0.01), dir, u.opt.w, wide, pixel_angle() * u.tan.z / u.size.x, near);
    if (h.hit == 0u) {
        return;
    }
    let c = fogged(h.colour * sun(h), dist + h.t) + h.add * clear_at(dist + h.t);
    textureStore(refl, id.xy, vec4<f32>(min(c, vec3<f32>(1.0)), 1.0));
}
"#;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PassU {
    r: [f32; 4],
    u: [f32; 4],
    f: [f32; 4],
    eye: [f32; 4],
    tan: [f32; 4],
    nf: [f32; 4],
    fog: [f32; 4],
    to_light: [f32; 4],
    fog_colour: [f32; 4],
    opt: [f32; 4],
    size: [f32; 4],
    opt2: [f32; 4],
    fr: [f32; 4],
    fu: [f32; 4],
    ff: [f32; 4],
    feye: [f32; 4],
    ftan: [f32; 4],
}

/// The textures a step keeps from picture to picture at one size, used within one submission (the lighting's rays' light and
/// its first smoothing).
struct Kept {
    size: (u32, u32),
    textures: Vec<wgpu::Texture>,
}

/// A texture the ray-traced steps write and read (storage and sampled).
fn step_texture(device: &wgpu::Device, size: (u32, u32), format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ray-traced step"),
        size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

impl Kept {
    /// `n` textures of `format` at `size`, made again when the size changes.
    fn get(slot: &std::sync::Mutex<Option<Kept>>, device: &wgpu::Device, size: (u32, u32), n: usize, format: wgpu::TextureFormat) -> Vec<wgpu::Texture> {
        let mut k = slot.lock().expect("a ray-traced step's textures");
        if k.as_ref().is_none_or(|k| k.size != size) {
            *k = Some(Kept { size, textures: (0..n).map(|_| step_texture(device, size, format)).collect() });
        }
        k.as_ref().expect("made above").textures.clone()
    }
}

/// What a frame's own picture traced (`raytrace::RtFrame::traced`), kept for the pictures drawn between it and the frame before
/// (PLAN 8.8): those trace no lighting and no reflections of the shiny surfaces again, each pixel taking what the frame's own
/// picture traced where the frame's camera saw its point (`in_frame`: the camera between two frames is at most a field's
/// motion from it, and the bounce, the occlusion and a reflection change little over that), so a picture between frames costs
/// the GPU what the frame's own steps' application costs. The camera the frame's picture was traced from, the lighting's
/// smoothed light and occlusion and its normals and view depths (half the picture's size), the reflections' picture and its
/// size. Its textures go back to `Passes::spare` when the frame goes.
#[derive(Default)]
pub(crate) struct Traced {
    cam: Option<Camera>,
    light: Option<[wgpu::Texture; 2]>,
    gloss: Option<([wgpu::Texture; 2], (u32, u32))>,
}

/// The lighting kept from the last frame (`TEMPORAL`).
struct History {
    size: (u32, u32),
    light: wgpu::Texture,
    geo: wgpu::Texture,
    cam: Camera,
}

/// A pass's pipelines for a frame with no mobies to trace (0) and with them (1).
type Both<T> = [T; 2];
/// A pass's pipelines by `variant`: with mobies to trace (+1), with EE layers (+2).
type Four<T> = [T; 4];

/// The ray-traced lighting's and reflections' pipelines (`raytrace::RayTracing::passes`) and the textures they keep.
pub(crate) struct Passes {
    trace: Four<wgpu::ComputePipeline>,
    trace_layout: wgpu::BindGroupLayout,
    denoise: wgpu::ComputePipeline,
    denoise_layout: wgpu::BindGroupLayout,
    temporal: wgpu::ComputePipeline,
    temporal_layout: wgpu::BindGroupLayout,
    /// The last frame's smoothed light and its normals and depths, and the camera it was traced from (kept over frames).
    history: std::sync::Mutex<Option<History>>,
    gloss_history: std::sync::Mutex<Option<History>>,
    /// At [mobies traced]: over every pixel, over the mobies'.
    apply: Both<[wgpu::RenderPipeline; 2]>,
    apply_layout: wgpu::BindGroupLayout,
    gloss: Four<wgpu::ComputePipeline>,
    gloss_layout: wgpu::BindGroupLayout,
    gloss_apply: wgpu::RenderPipeline,
    gloss_apply_layout: wgpu::BindGroupLayout,
    water: Four<wgpu::ComputePipeline>,
    water_layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// The lighting's rays' light and its first smoothing (half the picture's size), and the textures the frames gone gave back
    /// (`Traced`), for the next frames'.
    light_kept: std::sync::Mutex<Option<Kept>>,
    spare: std::sync::Mutex<Vec<wgpu::Texture>>,
    /// The frames the lighting's rays were traced for (each frame's rays turn by it: `TRACE`).
    frames: std::sync::atomic::AtomicU32,
    gloss_frames: std::sync::atomic::AtomicU32,
}

fn texture_entry(binding: u32, visibility: wgpu::ShaderStages, sample: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility, ty: wgpu::BindingType::Texture { sample_type: sample, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false }, count: None }
}

fn storage_texture(binding: u32, format: wgpu::TextureFormat) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry { binding, visibility: wgpu::ShaderStages::COMPUTE, ty: wgpu::BindingType::StorageTexture { access: wgpu::StorageTextureAccess::WriteOnly, format, view_dimension: wgpu::TextureViewDimension::D2 }, count: None }
}

/// A fullscreen pass over the picture.
fn picture_pass<'e>(enc: &'e mut wgpu::CommandEncoder, target: &wgpu::TextureView, label: &str, times: Option<wgpu::RenderPassTimestampWrites<'e>>) -> wgpu::RenderPass<'e> {
    enc.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment { view: target, depth_slice: None, resolve_target: None, ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store } })],
        depth_stencil_attachment: None,
        timestamp_writes: times,
        occlusion_query_set: None,
        multiview_mask: None,
    })
}

impl Passes {
    pub fn new(device: &wgpu::Device, world: &wgpu::BindGroupLayout, mobies: &wgpu::BindGroupLayout, layers: &wgpu::BindGroupLayout) -> Passes {
        let (c, f) = (wgpu::ShaderStages::COMPUTE, wgpu::ShaderStages::FRAGMENT);
        let depth = wgpu::TextureSampleType::Depth;
        let plain = wgpu::TextureSampleType::Float { filterable: false };
        let layout = |label: &str, entries: &[wgpu::BindGroupLayoutEntry]| device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries });
        let half = wgpu::TextureFormat::Rgba16Float;
        let geometry = wgpu::TextureFormat::Rgba32Float;
        let trace_layout = layout("ray-traced lighting", &[uniform(0, c), texture_entry(1, c, depth), storage_texture(2, half), storage_texture(3, geometry), texture_entry(8, c, plain), texture_entry(9, c, plain), texture_entry(10, c, plain)]);
        let denoise_layout = layout("ray-traced lighting, smoothed", &[uniform(0, c), texture_entry(1, c, plain), texture_entry(2, c, plain), storage_texture(3, half)]);
        let temporal_layout = layout("ray-traced lighting, kept", &[uniform(0, c), texture_entry(1, c, plain), texture_entry(2, c, plain), texture_entry(3, c, plain), texture_entry(4, c, plain), storage_texture(5, half), texture_entry(6, c, plain)]);
        let apply_layout = layout("ray-traced lighting, applied", &[uniform(0, f), texture_entry(1, f, depth), texture_entry(2, f, plain), texture_entry(3, f, plain), texture_entry(4, f, plain), texture_entry(8, f, plain), texture_entry(9, f, plain), texture_entry(10, f, plain)]);
        let gloss_layout = layout("ray-traced reflections", &[uniform(0, c), texture_entry(1, c, depth), texture_entry(2, c, plain), storage_texture(3, half), storage_texture(4, geometry), texture_entry(8, c, plain), texture_entry(9, c, plain), texture_entry(10, c, plain)]);
        let gloss_apply_layout = layout("ray-traced reflections, applied", &[uniform(0, f), texture_entry(1, f, wgpu::TextureSampleType::Float { filterable: true }), wgpu::BindGroupLayoutEntry { binding: 2, visibility: f, ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering), count: None }, texture_entry(3, f, depth), texture_entry(4, f, plain)]);
        let water_layout = layout("ray-traced water", &[uniform(0, c), texture_entry(1, c, depth), storage_texture(2, wgpu::TextureFormat::Rgba8Unorm)]);
        let module = |label: &str, source: String| device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
        // A pipeline layout of `own` and the scene's groups (the mobies' when `m`, the EE layers' when `e`).
        let scene_layout = |own: &wgpu::BindGroupLayout, m: bool, e: bool| {
            let mut groups = vec![Some(own), Some(world)];
            if m {
                groups.push(Some(mobies));
            }
            if e {
                groups.push(Some(layers));
            }
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ray-traced step"), bind_group_layouts: &groups, immediate_size: 0 })
        };
        let compute = |label: &str, body: &str, entry: &str, own: &wgpu::BindGroupLayout, v: usize| {
            let (m, e) = (v & 1 != 0, v & 2 != 0);
            let md = module(label, format!("{ENABLES}{}{COMMON}{SCENE_HELP}{body}", crate::raytrace::scene(m, e)));
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some(label), layout: Some(&scene_layout(own, m, e)), module: &md, entry_point: Some(entry), compilation_options: Default::default(), cache: crate::pipelines::cache() })
        };
        let trace = [0, 1, 2, 3].map(|v| compute("ray-traced lighting", TRACE, "trace", &trace_layout, v));
        let gloss = [0, 1, 2, 3].map(|v| compute("ray-traced reflections", GLOSS_TRACE, "gloss", &gloss_layout, v));
        let water = [0, 1, 2, 3].map(|v| compute("ray-traced water", WATER, "water", &water_layout, v));
        let denoise = {
            let md = module("ray-traced lighting, smoothed", DENOISE.into());
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ray-traced lighting, smoothed"), bind_group_layouts: &[Some(&denoise_layout)], immediate_size: 0 });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("ray-traced lighting, smoothed"), layout: Some(&pl), module: &md, entry_point: Some("denoise"), compilation_options: Default::default(), cache: crate::pipelines::cache() })
        };
        let temporal = {
            let md = module("ray-traced lighting, kept", format!("{COMMON}{TEMPORAL}"));
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ray-traced lighting, kept"), bind_group_layouts: &[Some(&temporal_layout)], immediate_size: 0 });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor { label: Some("ray-traced lighting, kept"), layout: Some(&pl), module: &md, entry_point: Some("temporal"), compilation_options: Default::default(), cache: crate::pipelines::cache() })
        };
        // The bounce added and the picture times 1 - the occlusion (dual source); the picture's alpha kept.
        let add_times = wgpu::BlendState {
            color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::Src1, operation: wgpu::BlendOperation::Add },
            alpha: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::Zero, dst_factor: wgpu::BlendFactor::One, operation: wgpu::BlendOperation::Add },
        };
        let render = |label: &str, md: &wgpu::ShaderModule, pl: &wgpu::PipelineLayout, entry: &str, blend: wgpu::BlendState, mask: wgpu::ColorWrites| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(pl),
                vertex: wgpu::VertexState { module: md, entry_point: Some("vs"), compilation_options: Default::default(), buffers: &[] },
                fragment: Some(wgpu::FragmentState { module: md, entry_point: Some(entry), compilation_options: Default::default(), targets: &[Some(wgpu::ColorTargetState { format: wgpu::TextureFormat::Rgba8Unorm, blend: Some(blend), write_mask: mask })] }),
                primitive: Default::default(),
                depth_stencil: None,
                multisample: Default::default(),
                multiview_mask: None,
                cache: crate::pipelines::cache(),
            })
        };
        let apply = [false, true].map(|m| {
            let md = module("ray-traced lighting, applied", format!("{ENABLES}enable dual_source_blending;\n{}{COMMON}{SCENE_HELP}{APPLY}", crate::raytrace::scene(m, false)));
            let pl = scene_layout(&apply_layout, m, false);
            [render("ray-traced lighting", &md, &pl, "fs_static", add_times, wgpu::ColorWrites::ALL), render("ray-traced lighting, mobies", &md, &pl, "fs_mobies", add_times, wgpu::ColorWrites::ALL)]
        });
        // Premultiplied reflected light plus the picture x (1 - covered share). Additive effects still contribute when the
        // reflected background is unknown (zero coverage); filtering also no longer multiplies edge coverage twice.
        let mix = wgpu::BlendState { color: wgpu::BlendComponent { src_factor: wgpu::BlendFactor::One, dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha, operation: wgpu::BlendOperation::Add }, alpha: wgpu::BlendComponent::REPLACE };
        let gloss_apply = {
            let md = module("ray-traced reflections, applied", format!("{COMMON}{GLOSS_APPLY}"));
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("ray-traced reflections, applied"), bind_group_layouts: &[Some(&gloss_apply_layout)], immediate_size: 0 });
            render("ray-traced reflections", &md, &pl, "fs", mix, wgpu::ColorWrites::COLOR)
        };
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor { label: Some("ray-traced reflections"), mag_filter: wgpu::FilterMode::Linear, min_filter: wgpu::FilterMode::Linear, ..Default::default() });
        Passes { trace, trace_layout, denoise, denoise_layout, temporal, temporal_layout, history: std::sync::Mutex::new(None), gloss_history: std::sync::Mutex::new(None), apply, apply_layout, gloss, gloss_layout, gloss_apply, gloss_apply_layout, water, water_layout, sampler, light_kept: std::sync::Mutex::new(None), spare: std::sync::Mutex::new(Vec::new()), frames: std::sync::atomic::AtomicU32::new(0), gloss_frames: std::sync::atomic::AtomicU32::new(0) }
    }

    /// A texture for a frame's `Traced` of `size` and `format`: one a frame gone gave back, or a new one.
    fn texture(&self, device: &wgpu::Device, size: (u32, u32), format: wgpu::TextureFormat) -> wgpu::Texture {
        let mut spare = self.spare.lock().expect("the ray-traced steps' spare textures");
        match spare.iter().position(|t| (t.width(), t.height()) == size && t.format() == format) {
            Some(k) => spare.swap_remove(k),
            None => step_texture(device, size, format),
        }
    }

    /// A frame's `Traced` given back when the frame goes (`raytrace::RtFrame`'s drop).
    pub(crate) fn give_back(&self, t: Traced) {
        let mut spare = self.spare.lock().expect("the ray-traced steps' spare textures");
        spare.extend(t.light.into_iter().flatten());
        spare.extend(t.gloss.into_iter().flat_map(|g| g.0));
    }

    /// A new level or material configuration must not inherit another scene's illumination or reflection history.
    pub(crate) fn reset_history(&self) {
        *self.history.lock().expect("lighting history") = None;
        *self.gloss_history.lock().expect("reflection history") = None;
        self.spare.lock().expect("ray-traced spare textures").clear();
    }

    /// Both traced signals use the same geometry-validated reprojection, at their own resolution.
    #[allow(clippy::too_many_arguments)]
    fn accumulate(&self, history: &std::sync::Mutex<Option<History>>, f: &RtFrame, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, size: (u32, u32), cam: &Camera, fog: &uyafmt::gameplay::Fog, current: &wgpu::Texture, geo: &wgpu::Texture, raw: &wgpu::TextureView) -> Result<wgpu::Texture, String> {
        let hs = (current.width(), current.height());
        let acc = self.texture(device, hs, wgpu::TextureFormat::Rgba16Float);
        let mut hist = history.lock().map_err(|_| "native: ray-traced history lock is poisoned")?;
        if hist.as_ref().is_none_or(|h| h.size != hs) {
            *hist = Some(History { size: hs, light: step_texture(device, hs, wgpu::TextureFormat::Rgba16Float), geo: step_texture(device, hs, wgpu::TextureFormat::Rgba32Float), cam: *cam });
        }
        let h = hist.as_mut().expect("made above");
        let u = pass_u(f, cam, &h.cam, fog, size, [0.0; 4], [hs.0 as f32, hs.1 as f32, 0.0, 0.0], [0.0; 4]);
        let ub = buffer(device, "ray-traced history", bytemuck::bytes_of(&u));
        let views = [current, geo, &h.light, &h.geo, &acc].map(|t| t.create_view(&Default::default()));
        let g = group(device, "ray-traced history", &self.temporal_layout, &[ub.as_entire_binding(), wgpu::BindingResource::TextureView(&views[0]), wgpu::BindingResource::TextureView(&views[1]), wgpu::BindingResource::TextureView(&views[2]), wgpu::BindingResource::TextureView(&views[3]), wgpu::BindingResource::TextureView(&views[4]), wgpu::BindingResource::TextureView(raw)]);
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ray-traced history"), timestamp_writes: None });
        p.set_pipeline(&self.temporal);
        p.set_bind_group(0, &g, &[]);
        p.dispatch_workgroups(hs.0.div_ceil(8), hs.1.div_ceil(8), 1);
        drop(p);
        let ext = wgpu::Extent3d { width: hs.0, height: hs.1, depth_or_array_layers: 1 };
        enc.copy_texture_to_texture(acc.as_image_copy(), h.light.as_image_copy(), ext);
        enc.copy_texture_to_texture(geo.as_image_copy(), h.geo.as_image_copy(), ext);
        h.cam = *cam;
        Ok(acc)
    }
}

/// The steps' uniform for picture `size` seen from `cam` (the frame's own picture traced from `frame`) with the frame's fog, the
/// pass's own values `opt`, `own` and `opt2`.
#[allow(clippy::too_many_arguments)]
fn pass_u(f: &RtFrame, cam: &Camera, frame: &Camera, fog: &uyafmt::gameplay::Fog, size: (u32, u32), opt: [f32; 4], own: [f32; 4], opt2: [f32; 4]) -> PassU {
    let w4 = |a: [f32; 3]| [a[0], a[1], a[2], 0.0];
    let right_of = |c: &Camera| {
        let (fw, up) = (c.forward, c.up);
        [fw[1] * up[2] - fw[2] * up[1], fw[2] * up[0] - fw[0] * up[2], fw[0] * up[1] - fw[1] * up[0]]
    };
    let (fw, up) = (cam.forward, cam.up);
    let right = right_of(cam);
    let (to, sun) = match f.to_light {
        Some(d) => (d, f.sun),
        None => ([0.0, 0.0, 1.0], false),
    };
    PassU {
        r: w4(right),
        u: w4(up),
        f: w4(fw),
        eye: w4(cam.eye),
        tan: [cam.tan[0], cam.tan[1], size.0 as f32, size.1 as f32],
        nf: [crate::native::NEAR, crate::native::FAR, crate::shadows::DARKNESS, crate::shadows::REACH],
        fog: [fog.near, fog.far, fog.near_f, fog.far_f],
        to_light: [to[0], to[1], to[2], sun as u32 as f32],
        fog_colour: [fog.colour[0] as f32, fog.colour[1] as f32, fog.colour[2] as f32, 0.0],
        opt,
        size: own,
        opt2,
        fr: w4(right_of(frame)),
        fu: w4(frame.up),
        ff: w4(frame.forward),
        feye: w4(frame.eye),
        ftan: [frame.tan[0], frame.tan[1], 0.0, 0.0],
    }
}

fn buffer(device: &wgpu::Device, label: &str, contents: &[u8]) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage: wgpu::BufferUsages::UNIFORM })
}

fn group(device: &wgpu::Device, label: &str, layout: &wgpu::BindGroupLayout, resources: &[wgpu::BindingResource]) -> wgpu::BindGroup {
    let entries: Vec<wgpu::BindGroupEntry> = resources.iter().enumerate().map(|(k, r)| wgpu::BindGroupEntry { binding: k as u32, resource: r.clone() }).collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries })
}

fn depth_view(depth: &wgpu::Texture) -> wgpu::TextureView {
    depth.create_view(&wgpu::TextureViewDescriptor { aspect: wgpu::TextureAspect::DepthOnly, ..Default::default() })
}

/// The scene groups bound from 1 of a compute pass (`raytrace::scene`): the static world's, the mobies' and the EE layers'.
fn bind_scene_compute(p: &mut wgpu::ComputePass, f: &RtFrame) {
    p.set_bind_group(1, &f.world, &[]);
    let mut next = 2;
    if let Some(g) = f.mobies_group() {
        p.set_bind_group(next, g, &[]);
        next += 1;
    }
    if let Some(g) = f.layers_group() {
        p.set_bind_group(next, g, &[]);
    }
}

/// A compute pass's pipeline of `four` for the frame's groups (`Four`).
fn variant(f: &RtFrame) -> usize {
    f.has_mobies() as usize + 2 * f.layers_group().is_some() as usize
}

fn bind_scene_render(p: &mut wgpu::RenderPass, f: &RtFrame) {
    p.set_bind_group(1, &f.world, &[]);
    if let Some(g) = f.mobies_group() {
        p.set_bind_group(2, g, &[]);
    }
}

/// The half-size step textures: the rays' light and occlusion, the normals and depths, the smoothed light and occlusion.
fn half(size: (u32, u32)) -> (u32, u32) {
    (size.0.div_ceil(2), size.1.div_ceil(2))
}

/// The ray-traced lighting at `level` (1 low, 2 high) with the ambient occlusion's level `ao` (0: none) over the picture
/// `target` (`size` pixels, its depth `depth`) seen from `cam` (`replay::Cmd::RtLight`): in the frame's own picture (`fresh`)
/// with `mobies` false the rays traced and smoothed into the frame's `Traced`; then applied over every pixel, or with `mobies`
/// over the mobies' pixels (`mask`), from the frame's `Traced` (a picture between frames traces nothing).
#[allow(clippy::too_many_arguments)]
pub(crate) fn light(f: &RtFrame, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, depth: &wgpu::Texture, mask: &wgpu::TextureView, target: &wgpu::TextureView, size: (u32, u32), cam: &Camera, fog: &uyafmt::gameplay::Fog, level: u8, ao: u8, mobies: bool, fresh: bool, profile: Option<&crate::gpu_profile::Recording>) -> Result<(), String> {
    let ps = &f.rt.passes;
    let hs = half(size);
    let rays = RAYS[(level.clamp(1, 2) - 1) as usize];
    let occlusion = if ao > 0 { crate::ao::STRENGTH } else { 0.0 };
    let wide = if ao > 0 { WIDE_STRENGTH } else { 0.0 };
    let dv = depth_view(depth);
    let times = f.rt.times.as_ref();
    let stamp = if mobies { crate::raytrace::T_LIGHT_MOBIES } else { crate::raytrace::T_LIGHT };
    if let Some(t) = times {
        t.stamp(enc, stamp);
    }
    let mut traced = f.traced.lock().map_err(|_| "native: a frame's ray-traced steps' lock is poisoned")?;
    if fresh && !mobies {
        let timing = crate::gpu_profile::Span::begin(profile, enc, "lighting_trace_filter");
        let kept = Kept::get(&ps.light_kept, device, hs, 2, wgpu::TextureFormat::Rgba16Float);
        let (light, geo) = (ps.texture(device, hs, wgpu::TextureFormat::Rgba16Float), ps.texture(device, hs, wgpu::TextureFormat::Rgba32Float));
        let (raw, tmp, lv, gv) = (kept[0].create_view(&Default::default()), kept[1].create_view(&Default::default()), light.create_view(&Default::default()), geo.create_view(&Default::default()));
        // The frames traced so far (`TRACE`'s frame: its rays' own directions).
        let frame = ps.frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 1024;
        let u = pass_u(f, cam, cam, fog, size, [BOUNCE, occlusion, rays as f32, REACH], [hs.0 as f32, hs.1 as f32, OCCLUSION, WIDE], [wide, MOST_SHADE, frame as f32, 0.0]);
        let ub = buffer(device, "ray-traced lighting", bytemuck::bytes_of(&u));
        let g = raster_group(f, device, "ray-traced lighting", &ps.trace_layout, &[ub.as_entire_binding(), wgpu::BindingResource::TextureView(&dv), wgpu::BindingResource::TextureView(&raw), wgpu::BindingResource::TextureView(&gv)])?;
        // Smoothed twice: the rays' light into the second kept texture (7 x 7), then that into the frame's (every other of 9 x 9).
        let du = buffer(device, "ray-traced lighting, smoothed", bytemuck::cast_slice(&[hs.0 as f32, hs.1 as f32, 1.0, 3.0]));
        let gd = group(device, "ray-traced lighting, smoothed", &ps.denoise_layout, &[du.as_entire_binding(), wgpu::BindingResource::TextureView(&raw), wgpu::BindingResource::TextureView(&gv), wgpu::BindingResource::TextureView(&tmp)]);
        let du2 = buffer(device, "ray-traced lighting, smoothed again", bytemuck::cast_slice(&[hs.0 as f32, hs.1 as f32, 2.0, 2.0]));
        let gd2 = group(device, "ray-traced lighting, smoothed again", &ps.denoise_layout, &[du2.as_entire_binding(), wgpu::BindingResource::TextureView(&tmp), wgpu::BindingResource::TextureView(&gv), wgpu::BindingResource::TextureView(&lv)]);
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ray-traced lighting"), timestamp_writes: None });
        let trace = crate::gpu_profile::Span::begin_compute(profile, &mut p, "light_rays");
        p.set_pipeline(&ps.trace[variant(f)]);
        p.set_bind_group(0, &g, &[]);
        bind_scene_compute(&mut p, f);
        p.dispatch_workgroups(hs.0.div_ceil(8), hs.1.div_ceil(8), 1);
        trace.end_compute(&mut p);
        let filter = crate::gpu_profile::Span::begin_compute(profile, &mut p, "light_denoise");
        p.set_pipeline(&ps.denoise);
        p.set_bind_group(0, &gd, &[]);
        p.dispatch_workgroups(hs.0.div_ceil(8), hs.1.div_ceil(8), 1);
        p.set_bind_group(0, &gd2, &[]);
        p.dispatch_workgroups(hs.0.div_ceil(8), hs.1.div_ceil(8), 1);
        filter.end_compute(&mut p);
        drop(p);
        // Kept over frames: the smoothed light with the last frame's into `acc` (the frame's), and `acc` and the normals kept for
        // the next frame.
        let acc = ps.accumulate(&ps.history, f, device, enc, size, cam, fog, &light, &geo, &raw)?;
        ps.spare.lock().map_err(|_| "native: the ray-traced steps' spare textures' lock is poisoned")?.push(light);
        let light = acc;
        if let Some(old) = traced.light.replace([light, geo]) {
            ps.spare.lock().map_err(|_| "native: the ray-traced steps' spare textures' lock is poisoned")?.extend(old);
        }
        traced.cam = Some(*cam);
        timing.end(enc);
    }
    let ([light, geo], frame) = match (&traced.light, &traced.cam) {
        (Some(l), Some(c)) => (l, c),
        _ => return Err("native: the ray-traced lighting applied before the frame's own picture traced it".into()),
    };
    let u = pass_u(f, cam, frame, fog, size, [BOUNCE, occlusion, rays as f32, REACH], [hs.0 as f32, hs.1 as f32, OCCLUSION, WIDE], [wide, MOST_SHADE, 0.0, 0.0]);
    let ub = buffer(device, "ray-traced lighting, applied", bytemuck::bytes_of(&u));
    let (lv, gv) = (light.create_view(&Default::default()), geo.create_view(&Default::default()));
    let ga = raster_group(f, device, "ray-traced lighting, applied", &ps.apply_layout, &[ub.as_entire_binding(), wgpu::BindingResource::TextureView(&dv), wgpu::BindingResource::TextureView(&lv), wgpu::BindingResource::TextureView(&gv), wgpu::BindingResource::TextureView(mask)])?;
    {
        let timing = crate::gpu_profile::Span::begin(profile, enc, if mobies { "lighting_mobies" } else { "lighting_composite" });
        let mut p = picture_pass(enc, target, "ray-traced lighting", None);
        p.set_pipeline(&ps.apply[f.has_mobies() as usize][mobies as usize]);
        p.set_bind_group(0, &ga, &[]);
        bind_scene_render(&mut p, f);
        p.draw(0..3, 0..1);
        drop(p);
        timing.end(enc);
    }
    if let Some(t) = times {
        t.stamp(enc, stamp + 1);
    }
    Ok(())
}

/// The reflections on the game's shiny surfaces at `level` (1 low: traced at half the picture's size; 2 high: at its size) over
/// the picture `target` (its texture `picture`, `size` pixels, its depth `depth`) seen from `cam` (`replay::Cmd::RtGloss`):
/// traced into the frame's `Traced` in the frame's own picture (`fresh`), and mixed in from there.
#[allow(clippy::too_many_arguments)]
pub(crate) fn gloss(f: &RtFrame, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, picture: &wgpu::Texture, depth: &wgpu::Texture, target: &wgpu::TextureView, size: (u32, u32), cam: &Camera, fog: &uyafmt::gameplay::Fog, level: u8, fresh: bool, profile: Option<&crate::gpu_profile::Recording>) -> Result<(), String> {
    let ps = &f.rt.passes;
    let dv = depth_view(depth);
    let times = f.rt.times.as_ref();
    if let Some(t) = times {
        t.stamp(enc, crate::raytrace::T_GLOSS);
    }
    let mut traced = f.traced.lock().map_err(|_| "native: a frame's ray-traced steps' lock is poisoned")?;
    if fresh {
        let timing = crate::gpu_profile::Span::begin(profile, enc, "reflection_trace_filter");
        let rs = if level >= 2 { size } else { half(size) };
        let out = ps.texture(device, rs, wgpu::TextureFormat::Rgba16Float);
        let geo = ps.texture(device, rs, wgpu::TextureFormat::Rgba32Float);
        let rv = out.create_view(&Default::default());
        let gv = geo.create_view(&Default::default());
        let frame = ps.gloss_frames.fetch_add(1, std::sync::atomic::Ordering::Relaxed) & 3;
        let u = pass_u(f, cam, cam, fog, size, [GLOSS, METAL_F0, if level >= 2 { 4.0 } else { 1.0 }, FAR], [rs.0 as f32, rs.1 as f32, 0.0, 0.0], [0.0, 0.0, frame as f32, 0.0]);
        let ub = buffer(device, "ray-traced reflections", bytemuck::bytes_of(&u));
        let pv = picture.create_view(&Default::default());
        let g = raster_group(f, device, "ray-traced reflections", &ps.gloss_layout, &[ub.as_entire_binding(), wgpu::BindingResource::TextureView(&dv), wgpu::BindingResource::TextureView(&pv), wgpu::BindingResource::TextureView(&rv), wgpu::BindingResource::TextureView(&gv)])?;
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ray-traced reflections"), timestamp_writes: None });
        p.set_pipeline(&ps.gloss[variant(f)]);
        p.set_bind_group(0, &g, &[]);
        bind_scene_compute(&mut p, f);
        p.dispatch_workgroups(rs.0.div_ceil(8), rs.1.div_ceil(8), 1);
        drop(p);
        let acc = ps.accumulate(&ps.gloss_history, f, device, enc, size, cam, fog, &out, &geo, &rv)?;
        ps.spare.lock().map_err(|_| "native: the ray-traced steps' spare textures' lock is poisoned")?.push(out);
        if let Some((old, _)) = traced.gloss.replace(([acc, geo], rs)) {
            ps.spare.lock().map_err(|_| "native: the ray-traced steps' spare textures' lock is poisoned")?.extend(old);
        }
        traced.cam = Some(*cam);
        timing.end(enc);
    }
    let (([out, geo], _), frame) = match (&traced.gloss, &traced.cam) {
        (Some(g), Some(c)) => (g, c),
        _ => return Err("native: the ray-traced reflections applied before the frame's own picture traced them".into()),
    };
    let u = pass_u(f, cam, frame, fog, size, [GLOSS, METAL_F0, 0.0, FAR], [size.0 as f32, size.1 as f32, 0.0, 0.0], [0.0; 4]);
    let ub = buffer(device, "ray-traced reflections, applied", bytemuck::bytes_of(&u));
    let rv = out.create_view(&Default::default());
    let gv = geo.create_view(&Default::default());
    let ga = group(device, "ray-traced reflections, applied", &ps.gloss_apply_layout, &[ub.as_entire_binding(), wgpu::BindingResource::TextureView(&rv), wgpu::BindingResource::Sampler(&ps.sampler), wgpu::BindingResource::TextureView(&dv), wgpu::BindingResource::TextureView(&gv)]);
    {
        let timing = crate::gpu_profile::Span::begin(profile, enc, "reflection_composite");
        let mut p = picture_pass(enc, target, "ray-traced reflections", None);
        p.set_pipeline(&ps.gloss_apply);
        p.set_bind_group(0, &ga, &[]);
        p.draw(0..3, 0..1);
        drop(p);
        timing.end(enc);
    }
    if let Some(t) = times {
        t.stamp(enc, crate::raytrace::T_GLOSS + 1);
    }
    Ok(())
}

/// The water's reflection traced into the reflections' picture at `level` (`reflections::Reflections::traced`), after the
/// screen-space mirror made it (`replay::Cmd::RtWater`), for the picture of `size` pixels (its depth `depth`) seen from `cam`.
/// Every picture traces it (the mirror is made again in every picture, and a picture between frames that took the frame's
/// traced water over its own mirror would show what the mirror and the rays each put there in turn).
#[allow(clippy::too_many_arguments)]
pub(crate) fn water(f: &RtFrame, device: &wgpu::Device, enc: &mut wgpu::CommandEncoder, reflections: &crate::reflections::Reflections, depth: &wgpu::Texture, size: (u32, u32), cam: &Camera, fog: &uyafmt::gameplay::Fog, plane: crate::reflections::Plane, level: u8) {
    let ps = &f.rt.passes;
    let (tex, rs) = reflections.traced(device, size, level);
    let rv = tex.create_view(&Default::default());
    let u = pass_u(f, cam, cam, fog, size, [0.0, 0.0, 0.0, FAR], [rs.0 as f32, rs.1 as f32, plane.h, plane.tol], [0.0; 4]);
    let ub = buffer(device, "ray-traced water", bytemuck::bytes_of(&u));
    let dv = depth_view(depth);
    let g = group(device, "ray-traced water", &ps.water_layout, &[ub.as_entire_binding(), wgpu::BindingResource::TextureView(&dv), wgpu::BindingResource::TextureView(&rv)]);
    let times = f.rt.times.as_ref();
    if let Some(t) = times {
        t.stamp(enc, crate::raytrace::T_WATER);
    }
    {
        let mut p = enc.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("ray-traced water"), timestamp_writes: None });
        p.set_pipeline(&ps.water[variant(f)]);
        p.set_bind_group(0, &g, &[]);
        bind_scene_compute(&mut p, f);
        p.dispatch_workgroups(rs.0.div_ceil(8), rs.1.div_ceil(8), 1);
    }
    if let Some(t) = times {
        t.stamp(enc, crate::raytrace::T_WATER + 1);
    }
}

fn raster_group(f: &RtFrame, device: &wgpu::Device, label: &str, layout: &wgpu::BindGroupLayout, resources: &[wgpu::BindingResource<'_>]) -> Result<wgpu::BindGroup, String> {
    let raster = f.raster.as_ref().ok_or("native: RT lighting/reflections without raster surface buffers")?;
    let mut entries: Vec<_> = resources.iter().enumerate().map(|(i, r)| wgpu::BindGroupEntry { binding: i as u32, resource: r.clone() }).collect();
    entries.extend(raster.views.iter().enumerate().map(|(i, v)| wgpu::BindGroupEntry { binding: 8 + i as u32, resource: wgpu::BindingResource::TextureView(v) }));
    Ok(device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries }))
}
