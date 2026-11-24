//! The LEVEL core's collision region (core index word 5; the region runs to the next core offset), as the game's collision code
//! reads it.
//!
//! The region starts with a 0x40-byte header. collision_load (func_L01_004250B0, the same function in all 34 story levels) reads
//! its words 0 to 2: byte offsets, from the header, of the terrain collision (stored at 0x1EAA80), the hero's groups and the
//! camera's groups (0 for none; stored at gp+0x33D0 and gp+0x33D4). Words 3 to 15 are zero and read by nothing.
//!
//! Terrain collision: a three-level grid and the cells it names. Every offset in it is from the terrain collision's start.
//! func_L01_003F83A8 (the grid lookup that func_L01_003F8448, func_L01_003F9498 and func_L01_003FA2D8 call, the same in all 34
//! story levels) reads it with a cell's three coordinates (x, y, z):
//! - root table: u16 first x, u16 count, then count u16 values; the table for x is at (value * 4), 0 for none;
//! - second-level table: u16 first y, u16 count, then count u32 byte offsets of third-level tables, 0 for none;
//! - third-level table: u16 first z, u16 count, then count u32 cell values, 0 for none. A cell value is (byte offset << 8) | the
//!   cell's size in quadwords: the query DMAs that many quadwords from that offset into the scratchpad.
//!
//! A cell (func_L01_003F8448, func_L01_003F9498 and func_L01_003FA2D8 read it from the scratchpad): u8 triangle count T, a byte
//! nothing reads (zero), u8 vertex count V, u8 count E of triangles that are quads; then V u32 vertices (x in bits 0 to 9, y in
//! bits 10 to 19, z in bits 20 to 31, scaled and added to the cell's origin by the code), T four-byte triangles (three u8 vertex
//! indices and a u8 of surface flags, whose low five bits and bits 5 and 7 the queries test), then E u8 indices: the first E
//! triangles are quads (i0, i1, i2) plus that fourth index, which the second pass tests as the triangle (i0, i2, fourth); then
//! zero bytes up to a multiple of 16 (the quadword count).
//!
//! Hero and camera collision: a table of u32 count N, 12 zero bytes and N 16-byte groups. collision_load adds the table's
//! address to each group's data offset. func_L01_003FBAF8 (hero; func_L01_003FC030 jumps into it with the camera's table, both
//! the same in all 34 story levels) reads a group as u16 x, y, z and radius of a bounding sphere in 1/64 units, a u8 triangle
//! count at +8 (+9 is not read, zero), a u16 vertex count at +10 and the u32 data offset at +12. The group's data is V vertices of
//! four u16 (x, y, z in 1/64 units; the fourth is zero and the code overwrites its slot with an outcode) and T four-byte
//! triangles (three u8 vertex indices and a u8 flags byte), then zero bytes up to a multiple of 16.
//!
//! Everything between these structures and after the last is zero (alignment of tables and cells to 4 or 16 bytes, and the
//! region's end).

use crate::disc::{u16_at, u32_at};
use crate::track::{Node, Space, Tracker};

const G: &str = "LEVEL core collision (unpacked)";
const DECODER: &str = "collision::collision";
const LOADER: &str = "func_L01_004250B0 (same in all 34 story levels)";

/// The line kinds, in the order the lines are made.
const N_KINDS: usize = 12;
const ROOT: usize = 0;
const L2: usize = 1;
const L3: usize = 2;
const CELL_HEAD: usize = 3;
const CELL_VTX: usize = 4;
const CELL_TRI: usize = 5;
const CELL_QUAD: usize = 6;
const GROUP_TABLE: usize = 7;
const GROUP_REC: usize = 8;
const GROUP_VTX: usize = 9;
const GROUP_TRI: usize = 10;
const PAD: usize = 11;

/// (name, game function) per kind.
const LINES: [(&str, &str); N_KINDS] = [
    (
        "terrain grid root table (u16 first x, u16 count, count u16 values: the x table's offset / 4)",
        "func_L01_003F83A8 (same in all 34 story levels; called by func_L01_003F8448, func_L01_003F9498 and func_L01_003FA2D8)",
    ),
    (
        "terrain grid second-level tables (u16 first y, u16 count, count u32 third-level table offsets)",
        "func_L01_003F83A8 (same in all 34 story levels)",
    ),
    (
        "terrain grid third-level tables (u16 first z, u16 count, count u32 cell values: offset << 8 | size in quadwords)",
        "func_L01_003F83A8 (same in all 34 story levels) returns the value; func_L01_003F8448 splits it into the DMA's source offset and quadword count",
    ),
    (
        "terrain cell headers (u8 triangle count, a zero byte, u8 vertex count, u8 quad count)",
        "func_L01_003F8448, func_L01_003F9498 and func_L01_003FA2D8 (same in all 34 story levels) read bytes 0, 2 and 3; byte 1 is read by nothing and zero",
    ),
    (
        "terrain cell vertices (u32: x bits 0-9, y bits 10-19, z bits 20-31)",
        "func_L01_003F8448, func_L01_003F9498 and func_L01_003FA2D8 (same in all 34 story levels)",
    ),
    (
        "terrain cell triangles (u8 i0, i1, i2 and a u8 of surface flags)",
        "func_L01_003F8448, func_L01_003F9498 and func_L01_003FA2D8 (same in all 34 story levels)",
    ),
    (
        "terrain cell quad indices (u8 fourth vertex index of each of the first quad-count triangles)",
        "func_L01_003F8448, func_L01_003F9498 and func_L01_003FA2D8 (same in all 34 story levels): the second pass rewrites those triangles to (i0, i2, this index)",
    ),
    (
        "hero and camera collision group table headers (u32 count, 12 zero bytes)",
        "func_L01_004250B0 (same in all 34 story levels) reads the count; the 12 bytes are read by nothing and zero",
    ),
    (
        "hero and camera collision groups (u16 sphere x, y, z, radius; u8 triangles, zero, u16 vertices; u32 data offset)",
        "func_L01_003FBAF8 (hero) and func_L01_003FC030 (camera) (same in all 34 story levels); func_L01_004250B0 relocates the data offset; +9 is read by nothing and zero",
    ),
    (
        "hero and camera collision group vertices (4 u16: x, y, z, zero)",
        "func_L01_003FBAF8 (hero) and func_L01_003FC030 (camera) (same in all 34 story levels); the fourth u16 is overwritten by the code and zero",
    ),
    (
        "hero and camera collision group triangles (u8 i0, i1, i2 and a u8 flags byte)",
        "func_L01_003FBAF8 (hero) and func_L01_003FC030 (camera) (same in all 34 story levels)",
    ),
    (
        "zero padding (cells and groups to 16 bytes, tables to 4, structures between, the region's end)",
        crate::track::NOTHING,
    ),
];

struct Plan {
    /// Byte ranges (offset, length) of each line kind.
    r: [Vec<(u64, u64)>; N_KINDS],
}

fn bad(what: &str, at: u64, v: u64, want: &str) -> String {
    format!("collision +{at:#x} {what}: {v:#x}, expected {want}")
}

impl Plan {
    /// A structure of kind `k` at `at`, `len` bytes: inside the region.
    fn add(&mut self, k: usize, at: u64, len: u64, n: u64, what: &str) -> Result<(), String> {
        if at + len > n {
            return Err(format!("collision +{at:#x}+{len:#x} {what}: past the region's {n:#x} bytes"));
        }
        if len > 0 {
            self.r[k].push((at, len));
        }
        Ok(())
    }
}

/// Every structure of the region, checked: the offsets are in range, the structures do not overlap, every byte between them is zero.
fn plan(b: &[u8]) -> Result<Plan, String> {
    let n = b.len() as u64;
    let mut p = Plan { r: Default::default() };
    if n < 0x40 {
        return Err(format!("collision region of {n:#x} bytes is shorter than its 0x40-byte header"));
    }
    let h = [u32_at(b, 0)? as u64, u32_at(b, 4)? as u64, u32_at(b, 8)? as u64];
    if let Some(k) = b[12..0x40].iter().position(|&v| v != 0) {
        return Err(bad("header byte", 12 + k as u64, b[12 + k] as u64, "zero"));
    }
    if h[0] != 0x40 {
        return Err(bad("terrain offset (header word 0)", 0, h[0], "0x40"));
    }
    let base = h[0];
    // The grid.
    let mn_cnt = |o: u64, what: &str| -> Result<u64, String> {
        if o % 4 != 0 || o + 4 > n {
            return Err(bad(what, o, o, "a 4-aligned table inside the region"));
        }
        Ok(u16_at(b, o as usize + 2)? as u64)
    };
    let cnt = mn_cnt(base, "root table")?;
    p.add(ROOT, base, 4 + 2 * cnt, n, "root table")?;
    let mut l2 = Vec::new();
    for i in 0..cnt {
        let e = u16_at(b, (base + 4 + 2 * i) as usize)? as u64;
        if e != 0 {
            l2.push(base + 4 * e);
        }
    }
    l2.sort_unstable();
    l2.dedup();
    let mut l3 = Vec::new();
    for &o in &l2 {
        let c = mn_cnt(o, "second-level table")?;
        p.add(L2, o, 4 + 4 * c, n, "second-level table")?;
        for j in 0..c {
            let e = u32_at(b, (o + 4 + 4 * j) as usize)? as u64;
            if e != 0 {
                l3.push(base + e);
            }
        }
    }
    l3.sort_unstable();
    l3.dedup();
    let mut cells = Vec::new();
    for &o in &l3 {
        let c = mn_cnt(o, "third-level table")?;
        p.add(L3, o, 4 + 4 * c, n, "third-level table")?;
        for j in 0..c {
            let e = u32_at(b, (o + 4 + 4 * j) as usize)?;
            if e != 0 {
                cells.push(e);
            }
        }
    }
    cells.sort_unstable();
    cells.dedup();
    for &e in &cells {
        let (a, q) = (base + (e >> 8) as u64, (e & 0xff) as u64);
        if q == 0 || a % 16 != 0 {
            return Err(bad("cell value", a, e as u64, "a 16-aligned offset and a non-zero quadword count"));
        }
        if a + 4 > n {
            return Err(format!("collision cell at {a:#x}: past the region's {n:#x} bytes"));
        }
        let (t, zero, v, x) = (b[a as usize] as u64, b[a as usize + 1], b[a as usize + 2] as u64, b[a as usize + 3] as u64);
        if zero != 0 {
            return Err(bad("cell header byte 1", a + 1, zero as u64, "zero"));
        }
        let need = 4 + 4 * v + 4 * t + x;
        if (need + 15) / 16 != q {
            return Err(format!("collision cell at {a:#x}: {t} triangles, {v} vertices and {x} quads take {need:#x} bytes, not {q} quadwords"));
        }
        if x > t {
            return Err(bad("cell quad count", a + 3, x, &format!("at most the triangle count {t}")));
        }
        p.add(CELL_HEAD, a, 4, n, "cell header")?;
        p.add(CELL_VTX, a + 4, 4 * v, n, "cell vertices")?;
        p.add(CELL_TRI, a + 4 + 4 * v, 4 * t, n, "cell triangles")?;
        p.add(CELL_QUAD, a + 4 + 4 * v + 4 * t, x, n, "cell quad indices")?;
        p.add(PAD, a + need, 16 * q - need, n, "cell padding")?;
        let tri = (a + 4 + 4 * v) as usize;
        for k in 0..t as usize {
            for c in 0..3 {
                let i = b[tri + 4 * k + c] as u64;
                if i >= v {
                    return Err(bad("cell triangle index", (tri + 4 * k + c) as u64, i, &format!("below the vertex count {v}")));
                }
            }
        }
        let ex = tri + 4 * t as usize;
        for k in 0..x as usize {
            let i = b[ex + k] as u64;
            if i >= v {
                return Err(bad("cell quad index", (ex + k) as u64, i, &format!("below the vertex count {v}")));
            }
        }
    }
    // Hero and camera groups.
    for off in [h[1], h[2]] {
        if off == 0 {
            continue;
        }
        if off % 16 != 0 || off + 16 > n {
            return Err(bad("group table offset", off, off, "a 16-aligned table inside the region"));
        }
        let cnt = u32_at(b, off as usize)? as u64;
        p.add(GROUP_TABLE, off, 4, n, "group table count")?;
        p.add(PAD, off + 4, 12, n, "group table padding")?;
        p.add(GROUP_REC, off + 16, 16 * cnt, n, "groups")?;
        for k in 0..cnt {
            let g = (off + 16 + 16 * k) as usize;
            let (t, nine, v, d) = (b[g + 8] as u64, b[g + 9], u16_at(b, g + 10)? as u64, u32_at(b, g + 12)? as u64);
            if nine != 0 {
                return Err(bad("group byte 9", g as u64 + 9, nine as u64, "zero"));
            }
            let a = off + d;
            if a % 16 != 0 {
                return Err(bad("group data offset", g as u64 + 12, d, "a multiple of 16"));
            }
            p.add(GROUP_VTX, a, 8 * v, n, "group vertices")?;
            p.add(GROUP_TRI, a + 8 * v, 4 * t, n, "group triangles")?;
            p.add(PAD, a + 8 * v + 4 * t, (16 - (8 * v + 4 * t) % 16) % 16, n, "group padding")?;
            for i in 0..v as usize {
                let w = u16_at(b, a as usize + 8 * i + 6)?;
                if w != 0 {
                    return Err(bad("group vertex fourth u16", a + 8 * i as u64 + 6, w as u64, "zero"));
                }
            }
            let tri = (a + 8 * v) as usize;
            for i in 0..t as usize {
                for c in 0..3 {
                    let x = b[tri + 4 * i + c] as u64;
                    if x >= v {
                        return Err(bad("group triangle index", (tri + 4 * i + c) as u64, x, &format!("below the vertex count {v}")));
                    }
                }
            }
        }
    }
    // No two structures overlap; every byte between them is zero padding.
    let mut all: Vec<(u64, u64)> = p.r.iter().flatten().copied().collect();
    all.push((0, 0x40));
    all.sort_unstable();
    let mut at = 0u64;
    let mut gaps = Vec::new();
    for &(a, l) in &all {
        if a < at {
            return Err(format!("collision structures overlap at {a:#x} (the previous one ends at {at:#x})"));
        }
        if a > at {
            gaps.push((at, a - at));
        }
        at = a + l;
    }
    if at < n {
        gaps.push((at, n - at));
    }
    for &(a, l) in &gaps {
        if let Some(k) = b[a as usize..(a + l) as usize].iter().position(|&v| v != 0) {
            return Err(bad("byte between structures", a + k as u64, b[a as usize + k] as u64, "zero"));
        }
    }
    p.r[PAD].extend(gaps);
    for r in p.r.iter_mut() {
        r.sort_unstable();
    }
    Ok(p)
}

/// The collision region of `b` at `at` in `parent` (the unpacked core's root): the header, the terrain collision's grid and cells,
/// the hero's and the camera's groups and their data. On a failure the whole extent is one failed line and the error is returned.
pub fn collision(t: &mut Tracker, parent: &mut Node, at: u64, b: &[u8]) -> Result<(), String> {
    let mut region = t.region(parent, G, at, b.len() as u64);
    let p = match plan(b) {
        Ok(p) => p,
        Err(e) => {
            let l = t.line(G, "collision region (not decoded: a value is not what the decoder requires)", DECODER, LOADER, Space::Unpacked);
            let mut n = t.child(&mut region, l, 0, b.len() as u64);
            n.fail(e.clone());
            t.close(n);
            t.close(region);
            return Err(e);
        }
    };
    let l = t.line(
        G,
        "collision header (16 words; word 0 terrain offset, word 1 hero groups, word 2 camera groups, words 3-15 zero)",
        DECODER,
        "func_L01_004250B0 (same in all 34 story levels) reads words 0 to 2; words 3 to 15 are read by nothing and zero",
        Space::Unpacked,
    );
    let mut n = t.child(&mut region, l, 0, 0x40);
    n.mark(0, 0x40);
    t.close(n);
    for k in 0..N_KINDS {
        let ranges = &p.r[k];
        if ranges.is_empty() {
            continue;
        }
        let l = t.line(G, LINES[k].0, DECODER, LINES[k].1, Space::Unpacked);
        let mut n = t.pieces(&mut region, l, ranges);
        let total = n.size;
        n.mark(0, total);
        t.close(n);
    }
    t.close(region);
    Ok(())
}
