//! The 8 x 8 inverse DCT of ISO 13818-2 annex A: `f(x, y) = 1/4 * sum over u, v of c(u) c(v) F(u, v) cos((2x + 1) u pi / 16)
//! cos((2y + 1) v pi / 16)` with `c(0) = 1/sqrt 2` and `c(k) = 1` otherwise, done as two passes of 8 one-dimensional transforms in
//! single precision and rounded to the nearest integer; the result is clipped to -256..=255 as the standard says.

use std::sync::OnceLock;

/// `table[x][u]`: c(u) / 2 * cos((2x + 1) u pi / 16).
fn table() -> &'static [[f32; 8]; 8] {
    static T: OnceLock<[[f32; 8]; 8]> = OnceLock::new();
    T.get_or_init(|| {
        let mut t = [[0f32; 8]; 8];
        for (x, row) in t.iter_mut().enumerate() {
            for (u, v) in row.iter_mut().enumerate() {
                let c = if u == 0 { std::f64::consts::FRAC_1_SQRT_2 } else { 1.0 };
                *v = (c / 2.0 * ((2 * x + 1) as f64 * u as f64 * std::f64::consts::PI / 16.0).cos()) as f32;
            }
        }
        t
    })
}

/// Transforms the coefficients (`f[v * 8 + u]`) into the 64 samples (`f[y * 8 + x]`), in place.
pub fn idct(f: &mut [i32; 64]) {
    // Only the DC: every sample is DC / 8.
    if f[1..].iter().all(|&c| c == 0) {
        let v = ((f[0] as f32 / 8.0).round() as i32).clamp(-256, 255);
        f.fill(v);
        return;
    }
    let t = table();
    // Rows first: for each row v, the samples along x.
    let mut tmp = [0f32; 64];
    for v in 0..8 {
        let row = &f[v * 8..v * 8 + 8];
        if row.iter().all(|&c| c == 0) {
            continue;
        }
        for x in 0..8 {
            let mut s = 0f32;
            for u in 0..8 {
                s += t[x][u] * row[u] as f32;
            }
            tmp[v * 8 + x] = s;
        }
    }
    // Then the columns.
    for x in 0..8 {
        for y in 0..8 {
            let mut s = 0f32;
            for v in 0..8 {
                s += t[y][v] * tmp[v * 8 + x];
            }
            f[y * 8 + x] = (s.round() as i32).clamp(-256, 255);
        }
    }
}
