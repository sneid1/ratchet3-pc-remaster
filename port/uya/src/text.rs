//! The game's text drawn with a vector font (`play`): the HUD's two bitmap fonts are 256 x 128 PSMT4 textures whose letters are
//! 8 to 13 texels high, which no upscale of the bitmap makes clean at a PC's resolution. So each letter the game draws is drawn
//! from Bahnschrift Bold (Windows' own squared sans, the closest system face to the game's) instead, in the game's place for it.
//!
//! # The game's font tables
//!
//! Each font set's character table is in the main program, before the set's uploads (`prompts.rs`): set A's from 0x143F44, set
//! B's from 0x14A344, one 8-byte record per character code from 0x20 (space) to 0xE0: a halfword (the button index for the
//! records before 0x20, 0 for characters), a halfword whose bits 0-8 are the letter's cell's left texel u0 (bit 11 set), and a
//! word: bits 0-8 the cell's top texel v0, 9-16 a y offset, 17-22 its height and 23-28 its width in texels. The text renderer
//! (frontbin func_38B998, as the front end's copy of it: `lhu 2(s0)`, `lw 4(s0)`, the fields `& 0x1FF`, `>> 17 & 0x3F`,
//! `>> 23 & 0x3F`) draws a letter as a quad of that cell with REGION_CLAMP on it (MINU u0, MAXU u0 + width - 1, likewise v). The
//! records before a table (codes 1 to 0xB) are the button glyphs' cells. A primitive drawing the font's texels with REGION_CLAMP
//! on a character's cell is that character.
//!
//! # Drawing a letter
//!
//! The letters are rasterised once into an atlas, cap height 48 pixels, each with its ink box. A letter's ink box is drawn on
//! the game's ink for it: its cell less the empty columns at the cell's sides (the font's texels of index 0; every letter
//! starts at its cell's left and leaves one empty column at its right, the fixed-width digits two to five). The game's text
//! draw (the front end's func_38EB10, each level's own copy: level 3's func_3FAE00, and func_3FB690 with a shadow under each
//! letter) puts the cells edge to edge (the pen moves by the cell's width and a quarter-texel field of the record's last
//! word, bits 29-31 rounded to whole texels, and back by bits 9-15 of its second halfword, the same way, before every letter
//! but the first: both 1 texel for every letter of both sets) and moves by the space's record width for a space (6 texels in
//! set A, 4 in set B), so the empty column is the gap between its letters and part of the gap between its words. So the text
//! keeps the game's spacing, baseline and line layout and only the letters' shapes are Bahnschrift's. The atlas's
//! alpha is the letter's coverage, as the game's font's, white (0x80) as its colours: the game's vertex colours tint it.

use ab_glyph::{Font, FontVec, PxScale, ScaleFont, point};
use std::collections::HashMap;

/// The two font sets' character tables in the main program (code 0x20's record).
const TABLES: [u32; 2] = [0x14_3F44, 0x14_A344];
/// The two fonts' texels in the main program, 256 x 128 PSMT4 (`prompts.rs`'s sets).
const FONTS: [u32; 2] = [0x14_5ED0, 0x14_C2D0];
const CODES: std::ops::Range<u32> = 0x20..0xE1;
/// The atlas's cap height and the padding around each letter (pixels).
const CAP: f32 = 48.0;
const PAD: u32 = 6;

/// A letter in the atlas: its ink box with the padding (x, y, w, h), the ink box itself in the same pixels, and Bahnschrift's
/// metrics at the atlas's size: the ink box's left and top from the pen on the baseline, and the advance.
#[derive(Clone, Copy)]
struct Letter {
    cell: [u32; 4],
    ink: [f32; 4],
    left: f32,
    top: f32,
    advance: f32,
}

/// A letter the game draws: its font set, its character, its colour (the shadow pass's black or the text's) and the game's box
/// for it (GS coordinates: x0, y0, x1, y1).
pub struct Drawn {
    pub set: usize,
    pub ch: char,
    pub colour: [u8; 4],
    pub game: [f64; 4],
}

pub struct Text {
    /// Per (set, u0, v0): the character and its cell's width and height in texels.
    by_cell: HashMap<(usize, u32, u32), (char, u32, u32)>,
    /// Per (set, character): its cell in the font (MINU, MINV, MAXU, MAXV, as REGION_CLAMP names it).
    cells: HashMap<(usize, char), [u32; 4]>,
    /// Per (set, character): its cell.
    metrics: HashMap<(usize, char), Cell>,
    /// Per set: the baseline below the line's top and the cap height, in texels (from the letter A's cell).
    baseline: [f32; 2],
    cap: [f32; 2],
    letters: HashMap<char, Letter>,
    pub size: (u32, u32),
    /// RGBA in the GS's units (white 0x80, alpha 0x80 opaque).
    pub texture: uyafmt::core::Texture,
}

/// A character's cell in its font: width and height (texels), y offset (quarter texels from the line's top), and the empty
/// columns at its left and right.
#[derive(Clone, Copy)]
struct Cell {
    w: u32,
    h: u32,
    yo: u32,
    empty: [u32; 2],
}

/// Where a letter is drawn: its rectangle in the cell's texel coordinates (u0, v0, u1, v1: the quad's corners map there) and its
/// atlas rectangle (pixels), with the atlas's clamp box.
pub struct Placed {
    pub quad: [f32; 4],
    pub atlas: [f32; 4],
}

impl Text {
    pub fn new(boot: &uyafmt::boot::Boot) -> Result<Text, String> {
        let (mut by_cell, mut metrics, mut cells) = (HashMap::new(), HashMap::new(), HashMap::new());
        let (mut baseline, mut cap) = ([0f32; 2], [0f32; 2]);
        for (s, &base) in TABLES.iter().enumerate() {
            let texels = boot.bytes(FONTS[s], 256 * 128 / 2)?;
            let texel = |u: u32, v: u32| texels[(v * 128 + u / 2) as usize] >> (4 * (u & 1)) & 0xf;
            for code in CODES {
                let r = boot.bytes(base + 8 * (code - 0x20), 8)?;
                let (u0, x) = (u16::from_le_bytes([r[2], r[3]]) as u32 & 0x1ff, u32::from_le_bytes([r[4], r[5], r[6], r[7]]));
                let (v0, yo, h, w) = (x & 0x1ff, x >> 9 & 0xff, x >> 17 & 0x3f, x >> 23 & 0x3f);
                // Printable ASCII only: the codes above 0x7E are the other languages' letters, which keep the game's bitmap.
                let Some(ch) = char::from_u32(code).filter(|c| c.is_ascii_graphic()) else { continue };
                if w == 0 || h == 0 {
                    return Err(format!("text: font set {s}'s character {ch:?} has a {w} x {h} cell"));
                }
                if u0 + w > 256 || v0 + h > 128 {
                    return Err(format!("text: font set {s}'s character {ch:?} has its cell ({u0}, {v0}) {w} x {h} outside the font"));
                }
                let inked: Vec<u32> = (0..w).filter(|&i| (0..h).any(|j| texel(u0 + i, v0 + j) != 0)).collect();
                let (Some(&first), Some(&last)) = (inked.first(), inked.last()) else {
                    return Err(format!("text: font set {s}'s character {ch:?} has no ink in its cell"));
                };
                by_cell.insert((s, u0, v0), (ch, w, h));
                cells.insert((s, ch), [u0, v0, u0 + w - 1, v0 + h - 1]);
                metrics.insert((s, ch), Cell { w, h, yo, empty: [first, w - 1 - last] });
                if ch == 'A' {
                    (baseline[s], cap[s]) = (yo as f32 / 4.0 + h as f32, h as f32);
                }
            }
            if cap[s] == 0.0 {
                return Err(format!("text: font set {s} has no letter A"));
            }
        }
        let path = format!("{}/Fonts/bahnschrift.ttf", std::env::var("WINDIR").map_err(|_| "text: no WINDIR")?);
        let mut font = FontVec::try_from_vec(std::fs::read(&path).map_err(|e| format!("text: {path}: {e}"))?).map_err(|e| format!("text: {path}: {e}"))?;
        // Bold, normal width.
        use ab_glyph::VariableFont;
        font.set_variation(b"wght", 700.0);
        font.set_variation(b"wdth", 100.0);
        // The pixel size whose cap height is CAP.
        let h_cap = font.outline_glyph(font.glyph_id('H').with_scale_and_position(PxScale::from(100.0), point(0.0, 0.0))).ok_or("text: no 'H'")?.px_bounds().height();
        let px = PxScale::from(100.0 * CAP / h_cap);
        let sf = font.as_scaled(px);
        // Each printable letter rasterised and packed in rows, 1024 wide.
        let (mut x, mut y, mut row) = (0u32, 0u32, 0u32);
        let mut glyphs = Vec::new();
        let mut letters = HashMap::new();
        for ch in (0x21u8..0x7f).map(char::from) {
            let id = sf.glyph_id(ch);
            let Some(o) = font.outline_glyph(id.with_scale_and_position(px, point(0.0, 0.0))) else { return Err(format!("text: Bahnschrift has no {ch:?}")) };
            let b = o.px_bounds();
            let (w, h) = (b.width().ceil() as u32 + 2 * PAD, b.height().ceil() as u32 + 2 * PAD);
            if x + w > 1024 {
                (x, y, row) = (0, y + row, 0);
            }
            letters.insert(ch, Letter { cell: [x, y, w, h], ink: [x as f32 + PAD as f32, y as f32 + PAD as f32, x as f32 + PAD as f32 + b.width(), y as f32 + PAD as f32 + b.height()], left: b.min.x, top: b.min.y, advance: sf.h_advance(id) });
            glyphs.push((x, y, o));
            x += w;
            row = row.max(h);
        }
        let height = (y + row).next_power_of_two();
        let mut px_out = vec![[0x80u8, 0x80, 0x80, 0]; 1024 * height as usize];
        for (gx, gy, o) in &glyphs {
            o.draw(|dx, dy, c| {
                let i = (gy + PAD + dy) as usize * 1024 + (gx + PAD + dx) as usize;
                px_out[i][3] = (c.clamp(0.0, 1.0) * 128.0).round() as u8;
            });
        }
        Ok(Text { by_cell, cells, metrics, baseline, cap, letters, size: (1024, height), texture: uyafmt::core::Texture { width: 1024, height: height as usize, levels: vec![px_out], gs: None } })
    }

    /// Font set `set`'s cell for `ch` (MINU, MINV, MAXU, MAXV), if the font has it.
    pub fn cell(&self, set: usize, ch: char) -> Option<[u32; 4]> {
        self.cells.get(&(set, ch)).copied()
    }

    /// The character of font set `set` whose cell is (MINU, MINV) to (MAXU, MAXV), if it is a printable one.
    pub fn char_at(&self, set: usize, region: [u32; 4]) -> Option<char> {
        let [u0, v0, u1, v1] = region;
        let &(ch, w, h) = self.by_cell.get(&(set, u0, v0))?;
        (u1 + 1 - u0 == w && v1 + 1 - v0 == h).then_some(ch)
    }

    /// Where each letter is drawn (GS coordinates, its padding included): word by word in Bahnschrift's own letters and spacing,
    /// on the game's baseline (the line's top from the letter's y offset in the table, the set's baseline below it), at the
    /// game's cap height, and stretched across to the game's ink for the word (its first letter's ink left to its last letter's
    /// ink right: the cells less their empty columns), so that every word starts and ends where the game's own letters do and
    /// the gaps between words are the game's. A word's letters are the same set and colour (the shadow pass and the text are
    /// laid out apart), on one line, less than two texels apart (the game puts its cells edge to edge and a space's width
    /// between words: 6 texels in set A, 4 in set B, its record for code 0x20; the empty column of the word's last letter makes
    /// the gap 7 and 5). The stretch is held at 1.45 of the letters' own width in the game's own proportions for the text
    /// (`across`: some text the game draws narrower than tall), the word centred on its ink when held; it is never held at a
    /// least, as a word wider than the game's ink would reach into the space after it (until 15 November 2026 it was held at 0.9
    /// of the width down, and QWARK COMICS' titles, drawn at 0.8 of that across, lost their spaces: "Issue1:PirateBooty!").
    pub fn layout(&self, letters: &[Drawn]) -> Vec<Option<[f64; 4]>> {
        let mut out = vec![None; letters.len()];
        // Each letter's texel size on screen (across, down), its baseline and its cell.
        let geo: Vec<Option<(f64, f64, f64, Cell)>> = letters
            .iter()
            .map(|d| {
                let &c = self.metrics.get(&(d.set, d.ch))?;
                let (tx, ty) = ((d.game[2] - d.game[0]) / c.w as f64, (d.game[3] - d.game[1]) / c.h as f64);
                Some((tx, ty, d.game[1] + (self.baseline[d.set] as f64 - c.yo as f64 / 4.0) * ty, c))
            })
            .collect();
        // The passes apart, in drawing order.
        let mut passes: Vec<((usize, [u8; 4]), Vec<usize>)> = Vec::new();
        for (i, d) in letters.iter().enumerate() {
            if geo[i].is_none() {
                continue;
            }
            match passes.iter_mut().find(|(k, _)| *k == (d.set, d.colour)) {
                Some((_, v)) => v.push(i),
                None => passes.push(((d.set, d.colour), vec![i])),
            }
        }
        for (_, seq) in passes {
            let mut k = 0;
            while k < seq.len() {
                let mut e = k + 1;
                while e < seq.len() {
                    let (a, b) = (&letters[seq[e - 1]], &letters[seq[e]]);
                    let (ga, gb) = (geo[seq[e - 1]].expect("kept above"), geo[seq[e]].expect("kept above"));
                    let gap = b.game[0] - a.game[2];
                    if (ga.2 - gb.2).abs() > ga.1 || gap > 2.0 * ga.0 || gap < -2.0 * ga.0 {
                        break;
                    }
                    e += 1;
                }
                let word = &seq[k..e];
                k = e;
                let set = letters[word[0]].set;
                let (tx0, ty, base, first) = geo[word[0]].expect("kept above");
                let (tx1, _, _, last) = geo[word[word.len() - 1]].expect("kept above");
                let Some(ls) = word.iter().map(|&i| self.letters.get(&letters[i].ch).copied()).collect::<Option<Vec<Letter>>>() else { continue };
                // Bahnschrift's layout of the word in atlas pixels: each letter's ink left and right from the pen.
                let mut pen = 0f32;
                let xs: Vec<(f32, f32)> = ls
                    .iter()
                    .map(|l| {
                        let x0 = pen + l.left;
                        pen += l.advance;
                        (x0, x0 + l.ink[2] - l.ink[0])
                    })
                    .collect();
                let (tl, tr) = (xs[0].0, xs.iter().map(|x| x.1).fold(f32::MIN, f32::max));
                let sy = ty * self.cap[set] as f64 / CAP as f64;
                // The game's cells for the word and its ink in them.
                let (gl, gr) = (letters[word[0]].game[0], letters[word[word.len() - 1]].game[2]);
                let (il, ir) = (gl + first.empty[0] as f64 * tx0, gr - last.empty[1] as f64 * tx1);
                // A word drawn with no height (a scale growing from nothing) is not seen: left as the game placed it.
                let Some(across) = self.across(word.iter().map(|&i| &letters[i])) else { continue };
                let natural = (tr - tl).max(1.0) as f64;
                let sx = ((ir - il) / natural).min(1.45 * across * sy);
                let left = il + ((ir - il) - natural * sx) / 2.0;
                for (j, &i) in word.iter().enumerate() {
                    let l = &ls[j];
                    let (x0, x1) = (left + (xs[j].0 - tl) as f64 * sx, left + (xs[j].1 - tl) as f64 * sx);
                    let (y0, y1) = (base + l.top as f64 * sy, base + (l.top + l.ink[3] - l.ink[1]) as f64 * sy);
                    let (px, py) = (PAD as f64 * sx, PAD as f64 * sy);
                    out[i] = Some([x0 - px, y0 - py, x1 + px, y1 + py]);
                }
            }
        }
        out
    }

    /// A drawn letter's line: its texel size on screen (down) and its baseline (GS coordinates), as `layout` finds them.
    pub fn line(&self, d: &Drawn) -> Option<(f64, f64)> {
        let c = self.metrics.get(&(d.set, d.ch))?;
        let ty = (d.game[3] - d.game[1]) / c.h as f64;
        Some((ty, d.game[1] + (self.baseline[d.set] as f64 - c.yo as f64 / 4.0) * ty))
    }

    /// How much wider than its own letters the game draws the text of `letters` (one line of one pass): the game's boxes over
    /// Bahnschrift's ink at the line's cap height, held between 0.9 and 1.45 of the text's own proportions (`across`).
    pub fn stretch(&self, letters: &[&Drawn]) -> Option<f64> {
        let (mut game, mut own) = (0f64, 0f64);
        for d in letters {
            let (ty, _) = self.line(d)?;
            let l = self.letters.get(&d.ch)?;
            game += d.game[2] - d.game[0];
            own += (l.ink[2] - l.ink[0]) as f64 * ty * self.cap[d.set] as f64 / CAP as f64;
        }
        let across = self.across(letters.iter().copied())?;
        (own > 0.0).then(|| (game / own).clamp(0.9 * across, 1.45 * across))
    }

    /// The game's scale across over its scale down for `letters` (one word or line of one pass): their boxes' widths over their
    /// cells' widths at the texel size down (None for letters drawn with no height). The game's text draw takes the two scales
    /// apart (the front end's func_38EB10 and each level's copies, level 3's func_3FAE00 and func_3FB690, the second with a
    /// shadow under each letter: a letter's box is its cell's width times the scale across and its height times the scale down,
    /// the pen moves by the scale across), and some lines it draws narrower than tall (QWARK COMICS' title "Issue 3: Shadow of
    /// the Robot": 0.67 of a pixel a texel across, 0.84 down): 1 for text drawn square.
    fn across<'a>(&self, letters: impl Iterator<Item = &'a Drawn>) -> Option<f64> {
        let (mut game, mut cells) = (0f64, 0f64);
        for d in letters {
            let c = self.metrics.get(&(d.set, d.ch))?;
            game += d.game[2] - d.game[0];
            cells += c.w as f64 * (d.game[3] - d.game[1]) / c.h as f64;
        }
        (cells > 0.0).then(|| game / cells)
    }

    /// `s` typeset in font set `set`'s line (texel size `ty`, baseline `base`, `stretch` as `stretch` gives it), starting at `x`,
    /// ending there or centred on it (`align`: GS coordinates): each printable letter's rectangle (its padding included) and
    /// its atlas rectangle (pixels). Spaces advance the pen.
    #[allow(clippy::too_many_arguments)]
    pub fn typeset(&self, set: usize, s: &str, ty: f64, base: f64, stretch: f64, x: f64, align: crate::menus::Align) -> Result<Vec<([f64; 4], [f32; 4])>, String> {
        let sy = ty * self.cap[set] as f64 / CAP as f64;
        let sx = sy * stretch;
        let space = self.letters.get(&'n').ok_or("text: no 'n'")?.advance as f64;
        let mut pen = 0f64;
        let mut out = Vec::new();
        for ch in s.chars() {
            if ch == ' ' {
                pen += space;
                continue;
            }
            let l = self.letters.get(&ch).ok_or_else(|| format!("text: Bahnschrift has no {ch:?}"))?;
            let x0 = pen + l.left as f64;
            let (w, h) = ((l.ink[2] - l.ink[0]) as f64, (l.ink[3] - l.ink[1]) as f64);
            out.push(([x0, l.top as f64, x0 + w, l.top as f64 + h], *l));
            pen += l.advance as f64;
        }
        let (ink_left, ink_right) = (out.first().map_or(0.0, |(r, _)| r[0]), out.last().map_or(0.0, |(r, _)| r[2]));
        let left = match align {
            crate::menus::Align::Left => x - ink_left * sx,
            crate::menus::Align::Right => x - ink_right * sx,
            crate::menus::Align::Centre => x - (ink_left + ink_right) / 2.0 * sx,
        };
        let (px, py) = (PAD as f64 * sx, PAD as f64 * sy);
        Ok(out
            .into_iter()
            .map(|(r, l)| {
                let c = l.cell;
                ([left + r[0] * sx - px, base + r[1] * sy - py, left + r[2] * sx + px, base + r[3] * sy + py], [c[0] as f32, c[1] as f32, (c[0] + c[2]) as f32, (c[1] + c[3]) as f32])
            })
            .collect())
    }

    /// The letter a primitive of font set `set` draws on the cell (MINU, MINV) to (MAXU, MAXV) of REGION_CLAMP, placed.
    pub fn place(&self, set: usize, region: [u32; 4]) -> Option<Placed> {
        let [u0, v0, u1, v1] = region;
        let &(ch, w, h) = self.by_cell.get(&(set, u0, v0))?;
        if u1 + 1 - u0 != w || v1 + 1 - v0 != h {
            return None;
        }
        let l = self.letters.get(&ch)?;
        let empty = self.metrics.get(&(set, ch))?.empty;
        // The letter's outline on the game's ink for it (the game's own spacing, baseline and alignment), the padding around:
        // atlas pixels per game texel across and down.
        let (ink0, ink1) = ((u0 + empty[0]) as f32, (u1 + 1 - empty[1]) as f32);
        let (sx, sy) = ((l.ink[2] - l.ink[0]) / (ink1 - ink0), (l.ink[3] - l.ink[1]) / h as f32);
        let (px, py) = (PAD as f32 / sx, PAD as f32 / sy);
        let quad = [ink0 - px, v0 as f32 - py, ink1 + px, v1 as f32 + 1.0 + py];
        let c = l.cell;
        Some(Placed { quad, atlas: [c[0] as f32, c[1] as f32, (c[0] + c[2]) as f32, (c[1] + c[3]) as f32] })
    }
}

/// The vertices (x, y: screen; u, v: the game font's texels) of a letter's quad, or a triangle of it, moved onto `placed`: each
/// vertex at a corner of the cell goes to that corner of the placed rectangle, its texture coordinates to that corner of the
/// letter in the atlas (pixels). The screen per texel is the quad's own.
pub fn corners(placed: &Placed, verts: &[(f64, f64, f32, f32)]) -> Vec<(f64, f64, f32, f32)> {
    let mm = |f: &dyn Fn(&(f64, f64, f32, f32)) -> f64| verts.iter().map(f).fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let ((x0, x1), (y0, y1)) = (mm(&|v| v.0), mm(&|v| v.1));
    let ((u0, u1), (v0, v1)) = (mm(&|v| v.2 as f64), mm(&|v| v.3 as f64));
    let (sx, sy) = ((x1 - x0) / (u1 - u0).max(1e-6), (y1 - y0) / (v1 - v0).max(1e-6));
    verts
        .iter()
        .map(|&(x, y, u, v)| {
            let (right, down) = (u as f64 - u0 > u1 - u as f64, v as f64 - v0 > v1 - v as f64);
            let (nu, nv) = (placed.quad[if right { 2 } else { 0 }] as f64, placed.quad[if down { 3 } else { 1 }] as f64);
            (x + (nu - u as f64) * sx, y + (nv - v as f64) * sy, placed.atlas[if right { 2 } else { 0 }], placed.atlas[if down { 3 } else { 1 }])
        })
        .collect()
}

/// The vertices (x, y: GS coordinates) of a letter's quad, or a triangle of it, moved onto `rect` (GS coordinates, the padding
/// included): each vertex at a corner of the quad goes to that corner of the rectangle and of the letter's atlas rectangle
/// `atlas` (pixels).
pub fn to_rect(rect: [f64; 4], atlas: [f32; 4], verts: &[(f64, f64)]) -> Vec<(f64, f64, f32, f32)> {
    let (x0, x1) = verts.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v.0), b.max(v.0)));
    let (y0, y1) = verts.iter().fold((f64::MAX, f64::MIN), |(a, b), v| (a.min(v.1), b.max(v.1)));
    verts
        .iter()
        .map(|&(x, y)| {
            let (right, down) = (x - x0 > x1 - x, y - y0 > y1 - y);
            (rect[if right { 2 } else { 0 }], rect[if down { 3 } else { 1 }], atlas[if right { 2 } else { 0 }], atlas[if down { 3 } else { 1 }])
        })
        .collect()
}
