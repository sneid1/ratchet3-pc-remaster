//! `uyaref mpegcmp DISC SECTOR BYTES [--out DIR] [--every N] [--lang L]`: an instrument. One movie of the disc (its first sector and
//! length, as the game's movie player is given them) is demultiplexed by `pss` and its video decoded twice, by the port's decoder
//! (`video`) and by rac-engine's (fmv, the decoder the port used before), picture by picture in display order: their sizes, time
//! stamps and types, the difference of every plane (largest, and the PSNR), and every N-th picture of both side by side, the port's
//! on the left (`DIR/mpeg_FFFFF.png`). The language's sound track is decoded by `audio` and by rac-engine's ADPCM decoder and the
//! two set against each other sample by sample.

use uya::mpeg::{audio::Track, pss::{Demuxer, Packet}, video::Decoder};
use rac_engine::fmv::{adpcm_stream, mpeg2};
use std::io::{Read, Seek, SeekFrom};

/// BT.601 limited-range YCbCr 4:2:0 to RGB (each colour sample to its 2 x 2 pixels).
fn rgb(w: usize, h: usize, y: &[u8], cb: &[u8], cr: &[u8]) -> Vec<u8> {
    let cw = w.div_ceil(2);
    let mut out = Vec::with_capacity(w * h * 3);
    for j in 0..h {
        for i in 0..w {
            let l = (y[j * w + i] as f32 - 16.0) * 1.164;
            let (u, v) = (cb[(j / 2) * cw + i / 2] as f32 - 128.0, cr[(j / 2) * cw + i / 2] as f32 - 128.0);
            out.extend([l + 1.596 * v, l - 0.392 * u - 0.813 * v, l + 2.017 * u].map(|c| c.round().clamp(0.0, 255.0) as u8));
        }
    }
    out
}

/// The largest difference and the PSNR (dB) of two planes.
fn diff(a: &[u8], b: &[u8]) -> (u8, f64) {
    let (mut max, mut se) = (0u8, 0f64);
    for (x, y) in a.iter().zip(b) {
        let d = x.abs_diff(*y);
        max = max.max(d);
        se += (d as f64) * (d as f64);
    }
    let mse = se / a.len().max(1) as f64;
    (max, if mse == 0.0 { f64::INFINITY } else { 10.0 * (255.0 * 255.0 / mse).log10() })
}

/// `uyaref mpegcmp DISC SECTOR BYTES [...]`, or `uyaref mpegcmp DISC all [--lang L]` for every movie of WAD1 (its 100 records: the
/// subtitle table and the program stream, each a sector relative to the WAD and a length).
pub fn compare(a: &[String]) -> Result<(), String> {
    let usage = "usage: uyaref mpegcmp DISC SECTOR BYTES [--out DIR] [--every N] [--lang L] | uyaref mpegcmp DISC all [--lang L]";
    let iso = a.get(1).ok_or(usage)?;
    let opt = |name: &str| a.iter().position(|x| x == name).map(|i| a[i + 1].clone());
    let out = opt("--out");
    let every: u64 = opt("--every").map(|v| v.parse().expect("--every N")).unwrap_or(100);
    let lang: u8 = opt("--lang").map(|v| v.parse().expect("--lang L")).unwrap_or(0);
    if a.get(2).map(String::as_str) == Some("all") {
        let mut d = uyafmt::disc::Disc::open(iso)?;
        let toc = uyafmt::toc::read(&mut d)?;
        let w = &toc.wads[0];
        let mut lines = Vec::new();
        for r in 0..w.pairs.len() / 2 {
            let (movie, bytes) = w.pairs[2 * r + 1];
            if movie == 0 || bytes == 0 {
                continue;
            }
            let sector = w.sector + movie as u64;
            println!("==== movie record {r}: sector {sector:#x}, {bytes} bytes");
            let s = one(iso, sector, bytes as u64, None, u64::MAX, lang).map_err(|e| format!("movie record {r} (sector {sector:#x}): {e}"))?;
            lines.push(format!("record {r:2} sector {sector:#x}: {s}"));
        }
        for l in &lines {
            println!("{l}");
        }
        return Ok(());
    }
    let num = |s: &String| -> Result<u64, String> { if let Some(h) = s.strip_prefix("0x") { u64::from_str_radix(h, 16) } else { s.parse() }.map_err(|e| format!("{s}: {e}")) };
    let sector = num(a.get(2).ok_or(usage)?)?;
    let bytes = num(a.get(3).ok_or(usage)?)?;
    let s = one(iso, sector, bytes, out, every, lang)?;
    println!("{s}");
    Ok(())
}

/// One movie compared; the summary line.
fn one(iso: &str, sector: u64, bytes: u64, out: Option<String>, every: u64, lang: u8) -> Result<String, String> {
    if let Some(d) = &out {
        std::fs::create_dir_all(d).map_err(|e| format!("{d}: {e}"))?;
    }
    let mut f = std::fs::File::open(iso).map_err(|e| format!("{iso}: {e}"))?;
    f.seek(SeekFrom::Start(sector * 2048)).map_err(|e| e.to_string())?;
    let mut dm = Demuxer::new(f.take(bytes));
    let (mut mine, mut theirs) = (Decoder::new(), mpeg2::Decoder::new());
    let (mut a_mine, mut a_theirs) = (Vec::<[i16; 2]>::new(), Vec::<i16>::new());
    let mut track = Track::new(lang);
    let mut their_adpcm: Option<adpcm_stream::AdpcmDecoder> = None;
    let (mut qa, mut qb) = (std::collections::VecDeque::new(), std::collections::VecDeque::new());
    let (mut n, mut worst, mut sum_psnr, mut exact) = (0u64, f64::INFINITY, 0f64, 0u64);
    let mut t_mine = std::time::Duration::ZERO;
    let mut compare_ready = |qa: &mut std::collections::VecDeque<uya::mpeg::video::Frame>, qb: &mut std::collections::VecDeque<mpeg2::Frame>| -> Result<(), String> {
        while let (Some(x), Some(y)) = (qa.front(), qb.front()) {
            if (x.width, x.height) != (y.width, y.height) {
                return Err(format!("picture {n}: {}x{} against {}x{}", x.width, x.height, y.width, y.height));
            }
            let (dy, dcb, dcr) = (diff(&x.y, &y.y), diff(&x.cb, &y.cb), diff(&x.cr, &y.cr));
            let p = dy.1.min(dcb.1).min(dcr.1);
            worst = worst.min(p);
            sum_psnr += p.min(99.0);
            exact += (p.is_infinite()) as u64;
            if (every != u64::MAX && (n < 12 || n % every == 0)) || p < 40.0 {
                println!(
                    "picture {n}: type {} pts {:?}/{:?}: Y max {} PSNR {:.1}, Cb {} {:.1}, Cr {} {:.1}",
                    x.coding_type, x.pts, y.pts, dy.0, dy.1, dcb.0, dcb.1, dcr.0, dcr.1
                );
            }
            if x.pts != y.pts {
                return Err(format!("picture {n}: time stamp {:?} against {:?}", x.pts, y.pts));
            }
            if let Some(d) = &out {
                if n % every == 0 {
                    let (w, h) = (x.width as usize, x.height as usize);
                    let (l, r) = (rgb(w, h, &x.y, &x.cb, &x.cr), rgb(w, h, &y.y, &y.cb, &y.cr));
                    let mut both = Vec::with_capacity(w * h * 6);
                    for j in 0..h {
                        both.extend_from_slice(&l[j * w * 3..(j + 1) * w * 3]);
                        both.extend_from_slice(&r[j * w * 3..(j + 1) * w * 3]);
                    }
                    let path = format!("{d}/mpeg_{n:05}.png");
                    std::fs::write(&path, uya::pic::png(&uya::pic::Image { w: 2 * w, h, rgb: both })).map_err(|e| format!("{path}: {e}"))?;
                }
            }
            qa.pop_front();
            qb.pop_front();
            n += 1;
        }
        Ok(())
    };
    while let Some(pk) = dm.next_packet()? {
        match pk {
            Packet::Video { pts, data } => {
                let t = std::time::Instant::now();
                mine.push(&data, pts).map_err(|e| format!("the port's decoder, {} pictures past the last compared: {e}", qa.len()))?;
                while let Some(fr) = mine.next_frame() {
                    qa.push_back(fr);
                }
                t_mine += t.elapsed();
                theirs.push(&data, pts);
                while let Some(fr) = theirs.next_frame() {
                    qb.push_back(fr);
                }
                compare_ready(&mut qa, &mut qb)?;
            }
            Packet::Audio { track: t, pts, data } => {
                track.push(t, pts, &data, &mut a_mine)?;
                if t == track.id() {
                    let mut body = &data[..];
                    if their_adpcm.is_none() {
                        let (h, k) = adpcm_stream::parse_header(body).ok_or("rac-engine: no SShd header")?;
                        body = &body[k..];
                        their_adpcm = Some(adpcm_stream::AdpcmDecoder::new(h.channels as usize, h.interleave));
                    }
                    their_adpcm.as_mut().expect("made above").push(body, &mut a_theirs);
                }
            }
        }
    }
    let t = std::time::Instant::now();
    mine.flush()?;
    while let Some(fr) = mine.next_frame() {
        qa.push_back(fr);
    }
    t_mine += t.elapsed();
    theirs.flush();
    while let Some(fr) = theirs.next_frame() {
        qb.push_back(fr);
    }
    compare_ready(&mut qa, &mut qb)?;
    if !qa.is_empty() || !qb.is_empty() {
        return Err(format!("the decoders gave different numbers of pictures: {} left over by the port's, {} by rac-engine's", qa.len(), qb.len()));
    }
    let theirs_pairs: Vec<[i16; 2]> = a_theirs.chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    let same = a_mine.iter().zip(&theirs_pairs).filter(|(x, y)| x == y).count();
    Ok(format!(
        "{n} pictures, {exact} identical, worst PSNR {worst:.1} dB, mean {:.1} dB; {:.1} ms a picture; sound track {}: {} frames, rac-engine's {}, {same} the same; {:?}",
        sum_psnr / n.max(1) as f64,
        t_mine.as_secs_f64() * 1000.0 / n.max(1) as f64,
        track.id(),
        a_mine.len(),
        theirs_pairs.len(),
        mine.stats
    ))
}
