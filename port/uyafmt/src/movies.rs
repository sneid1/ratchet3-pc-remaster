//! A WAD1 movie: an MPEG-2 program stream (ISO 13818-1), demultiplexed by libmpeg's sceMpegDemuxPss in boot_elf
//! (func_00139A70: packs and system headers through func_00137620, PES packets through func_00137CC8). Its streams:
//!
//! - video, stream 0xE0: an MPEG-2 video stream (ISO 13818-2). Its headers are parsed by libmpeg (func_00134C08 and its
//!   callees); slices are decoded by the IPU, the PS2's MPEG-2 decoder, fed by func_L01_004D0350.
//! - audio, private stream 1 substream 0xFF: five streams, one per language (the payload's 4-byte header FF A1 00 xx, xx the
//!   language). func_L01_004D18E0 registers func_L01_004D0468 for the language's channel; the stream starts with SShd (0x20
//!   bytes) and SSbd (8 bytes), which func_L01_004CF578 / func_L01_004CF648 copy into the stream object (func_L01_004CF490
//!   reads the rate, func_L01_004CFCB0 the channel count), then SPU2 ADPCM frames sent to the IOP in channels * 0x100 blocks.
//! - padding, stream 0xBE: 0xFF bytes. The stream ends with the end code 0x000001B9.

use crate::track::{NOTHING, Node, Space, Tracker};
use std::collections::BTreeMap;

const G: &str = "WAD1 movie (MPEG-2 program stream)";
const GV: &str = "WAD1 movie video stream (MPEG-2, demuxed)";
const GA: &str = "WAD1 movie audio stream (demuxed)";
const DEMUX: &str = "func_00139A70 (libmpeg sceMpegDemuxPss)";

fn be16(b: &[u8], o: usize) -> Result<usize, String> {
    b.get(o..o + 2).map(|x| u16::from_be_bytes([x[0], x[1]]) as usize).ok_or_else(|| format!("+{o:#x}: past the stream's end"))
}

/// One movie of `d.len()` bytes at `at` in `parent` (the WAD1 root).
pub fn movie(t: &mut Tracker, parent: &mut Node, at: u64, d: &[u8]) {
    let mut region = t.region(parent, G, at, d.len() as u64);
    let mut packs = Vec::new();
    let mut system = Vec::new();
    let mut pes_v = Vec::new();
    let mut pes_a = Vec::new();
    let mut padding = Vec::new();
    let mut end = Vec::new();
    let mut sub_hdr = Vec::new();
    let mut video = Vec::new();
    let mut audio: BTreeMap<[u8; 4], Vec<(u64, u64)>> = BTreeMap::new();
    let mut error: Option<(&str, String)> = None;
    let mut p = 0usize;
    while p < d.len() {
        let r = (|| -> Result<(&'static str, usize), (&'static str, String)> {
            let code = d.get(p..p + 4).map(|x| u32::from_be_bytes([x[0], x[1], x[2], x[3]])).ok_or(("pack header", format!("+{p:#x}: 4 bytes past the end")))?;
            match code {
                0x1ba => {
                    let h = d.get(p..p + 14).ok_or(("pack header", format!("+{p:#x}: short pack header")))?;
                    let ok = h[4] >> 6 == 1 && h[4] & 4 != 0 && h[6] & 4 != 0 && h[8] & 4 != 0 && h[9] & 1 != 0 && h[12] & 3 == 3 && h[13] >> 3 == 0x1f;
                    let st = (h[13] & 7) as usize;
                    if !ok || d.get(p + 14..p + 14 + st).is_none_or(|s| s.iter().any(|&v| v != 0xff)) {
                        return Err(("pack header", format!("+{p:#x}: pack header {:02x?}", h)));
                    }
                    Ok(("pack", 14 + st))
                }
                0x1b9 => Ok(("end", 4)),
                _ if code >> 8 == 1 => {
                    let sid = code & 0xff;
                    let ln = be16(d, p + 4).map_err(|e| ("PES header", e))?;
                    let body = d.get(p + 6..p + 6 + ln).ok_or(("PES header", format!("+{p:#x}: packet of {ln:#x} bytes past the end")))?;
                    match sid {
                        0xbb => {
                            let ok = ln >= 6 && (ln - 6) % 3 == 0 && body[0] & 0x80 != 0 && body[2] & 1 != 0 && body[4] & 0x20 != 0 && body[5] & 0x7f == 0x7f && (6..ln).step_by(3).all(|k| body[k + 1] >> 6 == 3);
                            if !ok {
                                return Err(("system header", format!("+{p:#x}: system header {:02x?}", body)));
                            }
                            Ok(("system", 6 + ln))
                        }
                        0xbe => {
                            if body.iter().any(|&v| v != 0xff) {
                                return Err(("padding packet", format!("+{p:#x}: padding packet holds a byte other than 0xFF")));
                            }
                            Ok(("padding", 6 + ln))
                        }
                        0xe0 | 0xbd => {
                            let which = if sid == 0xe0 { "PES header, video" } else { "PES header, audio" };
                            if body.len() < 3 || body[0] >> 6 != 2 || body[0] & 0x30 != 0 {
                                return Err((which, format!("+{p:#x}: PES flags {:02x?}", &body[..3.min(body.len())])));
                            }
                            let (flags, hl) = (body[1], body[2] as usize);
                            let pts = flags >> 6;
                            let mut need = match pts {
                                0 => 0,
                                2 => 5,
                                3 => 10,
                                _ => return Err((which, format!("+{p:#x}: PTS_DTS flags {pts}"))),
                            };
                            if flags & 0x3e != 0 {
                                return Err((which, format!("+{p:#x}: PES flags {flags:#04x}")));
                            }
                            let hd = body.get(3..3 + hl).ok_or((which, format!("+{p:#x}: PES header data past the packet")))?;
                            if need >= 5 && (hd[0] >> 4 != if pts == 2 { 2 } else { 3 } || hd[0] & 1 == 0 || hd[2] & 1 == 0 || hd[4] & 1 == 0) {
                                return Err((which, format!("+{p:#x}: PTS {:02x?}", &hd[..5])));
                            }
                            if need == 10 && (hd[5] >> 4 != 1 || hd[5] & 1 == 0 || hd[7] & 1 == 0 || hd[9] & 1 == 0) {
                                return Err((which, format!("+{p:#x}: DTS {:02x?}", &hd[5..10])));
                            }
                            if flags & 1 != 0 {
                                // PES extension: only the P-STD buffer field (flags 0x1E), then '01' scale, size.
                                if hd.get(need) != Some(&0x1e) || hd.get(need + 1).is_none_or(|v| v >> 6 != 1) {
                                    return Err((which, format!("+{p:#x}: PES extension {:02x?}", hd)));
                                }
                                need += 3;
                            }
                            if hd.len() < need || hd[need..].iter().any(|&v| v != 0xff) {
                                return Err((which, format!("+{p:#x}: PES header stuffing {:02x?}", hd)));
                            }
                            Ok((if sid == 0xe0 { "video" } else { "audio" }, 9 + hl))
                        }
                        _ => Err(("PES header", format!("+{p:#x}: stream id {sid:#04x}"))),
                    }
                }
                _ => Err(("pack header", format!("+{p:#x}: no start code: {code:#010x}"))),
            }
        })();
        match r {
            Ok(("pack", n)) => {
                packs.push((p as u64, n as u64));
                p += n;
            }
            Ok(("end", n)) => {
                end.push((p as u64, n as u64));
                p += n;
                if p != d.len() {
                    error = Some(("end code", format!("+{p:#x}: the end code is not the stream's last bytes ({:#x})", d.len())));
                    break;
                }
            }
            Ok(("system", n)) => {
                system.push((p as u64, n as u64));
                p += n;
            }
            Ok(("padding", n)) => {
                padding.push((p as u64, n as u64));
                p += n;
            }
            Ok((kind, hn)) => {
                let ln = be16(d, p + 4).unwrap_or(0);
                let (start, stop) = (p + hn, p + 6 + ln);
                if kind == "video" {
                    pes_v.push((p as u64, hn as u64));
                    video.push((start as u64, (stop - start) as u64));
                } else {
                    pes_a.push((p as u64, hn as u64));
                    let sub = d.get(start..start + 4).map(|x| [x[0], x[1], x[2], x[3]]);
                    match sub {
                        Some(s) if s[0] == 0xff && s[1] == 0xa1 && s[2] == 0 => {
                            sub_hdr.push((start as u64, 4));
                            audio.entry(s).or_default().push((start as u64 + 4, (stop - start - 4) as u64));
                        }
                        _ => {
                            error = Some(("audio substream header", format!("+{start:#x}: substream header {:02x?}", sub)));
                            break;
                        }
                    }
                }
                p = stop;
            }
            Err((which, e)) => {
                error = Some((which, e));
                break;
            }
        }
    }

    let put = |t: &mut Tracker, region: &mut Node, name: &str, game: &'static str, ranges: &[(u64, u64)]| -> Node {
        let l = t.line(G, name, "movies::movie", game, Space::Disc);
        let mut n = t.pieces(region, l, ranges);
        let size = n.size;
        n.mark(0, size);
        if let Some((which, e)) = &error {
            if *which == name {
                n.fail(e.clone());
            }
        }
        n
    };
    for (name, game, ranges) in [
        ("pack header", "func_00137620 (pack parser of func_00139A70)", &packs),
        ("system header", "func_00137620", &system),
        ("PES header, video", "func_00137CC8 (PES parser of func_00139A70)", &pes_v),
        ("PES header, audio", "func_00137CC8 (PES parser of func_00139A70)", &pes_a),
        ("padding packet", "func_00139A70 (skips it by its length)", &padding),
        ("end code", DEMUX, &end),
        ("audio substream header (FF A1 00, language)", "func_00139A70 (matches stream 0xBD, substream 0xFF); func_L01_004D18E0 registers the language's channel", &sub_hdr),
    ] {
        if !ranges.is_empty() || error.as_ref().is_some_and(|e| e.0 == name) {
            let n = put(t, &mut region, name, game, ranges);
            t.close(n);
        }
    }
    if let Some((which, e)) = &error {
        if !["pack header", "system header", "PES header, video", "PES header, audio", "padding packet", "end code", "audio substream header (FF A1 00, language)"].contains(which) {
            let n = put(t, &mut region, which, DEMUX, &[]);
            let mut n = n;
            n.fail(e.clone());
            t.close(n);
        }
    }

    // The video stream.
    if !video.is_empty() {
        let mut n = put(t, &mut region, "video PES payload (the MPEG-2 video stream)", "func_L01_004D0350 (feeds it to libmpeg and the IPU)", &video);
        let es: Vec<u8> = video.iter().flat_map(|&(a, l)| d[a as usize..(a + l) as usize].iter().copied()).collect();
        let mut root = t.unpacked_root(&mut n, GV, es.len() as u64);
        video_stream(t, &mut root, &es);
        t.close(root);
        t.close(n);
    }
    // The audio streams.
    for (sub, ranges) in &audio {
        let mut n = put(t, &mut region, "audio PES payload (one language's audio stream)", "func_L01_004D0468 (the language's callback)", ranges);
        let a: Vec<u8> = ranges.iter().flat_map(|&(x, l)| d[x as usize..(x + l) as usize].iter().copied()).collect();
        let mut root = t.unpacked_root(&mut n, GA, a.len() as u64);
        audio_stream(t, &mut root, &a, sub[3]);
        t.close(root);
        t.close(n);
    }
    t.close(region);
}

/// An MPEG-2 video elementary stream, unit by start code.
fn video_stream(t: &mut Tracker, root: &mut Node, v: &[u8]) {
    let mut by: BTreeMap<&'static str, Vec<(u64, u64)>> = BTreeMap::new();
    let mut err: Option<(&'static str, String)> = None;
    let find = |from: usize| v[from..].windows(3).position(|w| w == [0, 0, 1]).map(|k| from + k);
    let mut q = match find(0) {
        Some(0) => 0,
        other => {
            err = Some(("sequence header", format!("the stream starts with {:02x?} (start code at {other:?})", &v[..8.min(v.len())])));
            v.len()
        }
    };
    while q < v.len() {
        let next = find(q + 3).unwrap_or(v.len());
        let unit = &v[q..next];
        let code = *unit.get(3).unwrap_or(&0xff);
        let body = &unit[4.min(unit.len())..];
        // (line, header bytes after the start code) or a whole-unit kind.
        let r: Result<(&'static str, usize), String> = match code {
            0xb3 => {
                if body.len() < 8 || body[6] & 0x20 == 0 {
                    Err(format!("sequence header {:02x?}", &body[..body.len().min(8)]))
                } else {
                    let intra = body[7] & 2 != 0;
                    let non_intra_at = if intra { 7 + 64 } else { 7 };
                    let non_intra = body.get(non_intra_at).is_some_and(|b| b & 1 != 0);
                    Ok(("sequence header", 8 + if intra { 64 } else { 0 } + if non_intra { 64 } else { 0 }))
                }
            }
            0xb5 => match body.first().map(|b| b >> 4) {
                Some(1) => {
                    if body.len() < 6 || body[3] & 1 == 0 {
                        Err(format!("sequence extension {:02x?}", body))
                    } else {
                        Ok(("sequence extension", 6))
                    }
                }
                Some(2) => {
                    let colour = body[0] & 1 != 0;
                    let n = if colour { 8 } else { 5 };
                    Ok(("sequence display extension", n))
                }
                Some(8) => {
                    let composite = body.get(4).is_some_and(|b| b & 0x40 != 0);
                    Ok(("picture coding extension", if composite { 7 } else { 5 }))
                }
                other => Err(format!("extension id {other:?}")),
            },
            0xb8 => {
                if body.len() < 4 || body[1] & 8 == 0 {
                    Err(format!("GOP header {:02x?}", body))
                } else {
                    Ok(("GOP header", 4))
                }
            }
            0x00 => {
                let kind = body.get(1).map(|b| (b >> 3) & 7).unwrap_or(0);
                match kind {
                    1 => Ok(("picture header", 4)),
                    2 | 3 => Ok(("picture header", 5)),
                    _ => Err(format!("picture coding type {kind}")),
                }
            }
            0x01..=0xaf => Ok(("slice", usize::MAX)),
            0xb2 => Ok(("user data", usize::MAX)),
            0xb7 => Ok(("sequence end", 0)),
            _ => Err(format!("start code {code:#04x}")),
        };
        match r {
            Ok((kind, usize::MAX)) => by.entry(kind).or_default().push((q as u64, unit.len() as u64)),
            Ok((kind, n)) => {
                if 4 + n > unit.len() {
                    err = Some((kind, format!("+{q:#x}: {kind} needs {n} bytes, the unit has {}", unit.len() - 4)));
                    break;
                }
                by.entry(kind).or_default().push((q as u64, 4 + n as u64));
                if unit[4 + n..].iter().any(|&b| b != 0) {
                    err = Some((kind, format!("+{q:#x}: {kind} followed by non-zero bytes before the next start code")));
                    break;
                }
                if unit.len() > 4 + n {
                    by.entry("zero stuffing before a start code").or_default().push(((q + 4 + n) as u64, (unit.len() - 4 - n) as u64));
                }
            }
            Err(e) => {
                err = Some(("sequence header", format!("+{q:#x}: {e}")));
                break;
            }
        }
        q = next;
    }
    let game = |k: &str| -> &'static str {
        match k {
            "slice" => "the IPU (the PS2's MPEG-2 decoder), fed by libmpeg (func_L01_004D0350's callback)",
            "user data" => "func_00134C08 (libmpeg; skips it)",
            "zero stuffing before a start code" => NOTHING,
            _ => "func_00134C08 (libmpeg's header parser) and its callees",
        }
    };
    for (kind, ranges) in &by {
        let l = t.line(GV, kind, "movies::video_stream", game(kind), Space::Unpacked);
        let mut n = t.pieces(root, l, ranges);
        let size = n.size;
        n.mark(0, size);
        if err.as_ref().is_some_and(|e| e.0 == *kind) {
            n.fail(err.as_ref().unwrap().1.clone());
        }
        t.close(n);
    }
    if let Some((kind, e)) = err {
        if !by.contains_key(kind) {
            let l = t.line(GV, kind, "movies::video_stream", game(kind), Space::Unpacked);
            let mut n = t.pieces(root, l, &[]);
            n.fail(e);
            t.close(n);
        }
    }
}

/// One language's audio stream: SShd, SSbd, SPU2 ADPCM frames.
fn audio_stream(t: &mut Tracker, root: &mut Node, a: &[u8], language: u8) {
    let u32le = |o: usize| a.get(o..o + 4).map(|x| u32::from_le_bytes([x[0], x[1], x[2], x[3]]));
    let l = t.line(
        GA,
        "SShd header (tag, size 0x18, type 0x10, rate, channels, interleave 0x20, loop -1 -1)",
        "movies::audio_stream",
        "func_L01_004CF648 copies it, func_L01_004CF490 reads the rate, func_L01_004CFCB0 the channel count; the rest is read by nothing",
        Space::Unpacked,
    );
    let mut h = t.child(root, l, 0, 0x20.min(a.len() as u64));
    let heads_ok = &a[..4.min(a.len())] == b"SShd"
        && u32le(4) == Some(0x18)
        && u32le(8) == Some(0x10)
        && u32le(12) == Some(48000)
        && u32le(16) == Some(2)
        && u32le(20) == Some(0x20)
        && u32le(24) == Some(0xffff_ffff)
        && u32le(28) == Some(0xffff_ffff)
        && a.get(0x20..0x24) == Some(b"SSbd");
    if heads_ok {
        h.mark(0, 0x20);
    } else {
        h.fail(format!("language {language}: SShd/SSbd heads {:02x?}", &a[..0x28.min(a.len())]));
    }
    t.close(h);
    if !heads_ok {
        return;
    }
    let declared = u32le(0x24).unwrap_or(0) as usize;
    let l = t.line(GA, "SSbd header (tag, data size)", "movies::audio_stream", "func_L01_004CF648 copies it; read by nothing", Space::Unpacked);
    let mut s = t.child(root, l, 0x20, 8);
    s.mark(0, 8);
    let data = &a[0x28..];
    // The data size: the frames the stream carries; one stream on the disc ends 40 bytes short of its declared size.
    if declared != data.len() && !(declared > data.len() && declared - data.len() < 0x100) {
        s.fail(format!("language {language}: SSbd size {declared:#x}, the stream carries {:#x}", data.len()));
    }
    t.close(s);
    let whole = data.len() / 16 * 16;
    let l = t.line(GA, "SPU2 ADPCM frames (16 bytes: shift and filter, flags, 28 samples)", "movies::audio_stream", "func_L01_004CFCB0 sends them to the IOP, the SPU2 plays them", Space::Unpacked);
    let mut f = t.child(root, l, 0x28, whole as u64);
    match (0..whole).step_by(16).find(|&k| data[k] >> 4 > 4 || data[k] & 15 > 12 || data[k + 1] > 7) {
        None => f.mark(0, whole as u64),
        Some(k) => f.fail(format!("language {language}: frame at {k:#x}: {:02x?}", &data[k..k + 2])),
    }
    t.close(f);
    if whole < data.len() {
        let l = t.line(GA, "partial ADPCM frame at a stream's end (the stream is cut short of its SSbd size)", "movies::audio_stream", "func_L01_004CFCB0 sends it with the rest", Space::Unpacked);
        let mut p = t.child(root, l, (0x28 + whole) as u64, (data.len() - whole) as u64);
        let n = p.size;
        if data[whole] >> 4 <= 4 && data[whole] & 15 <= 12 && data.get(whole + 1).is_none_or(|&v| v <= 7) {
            p.mark(0, n);
        }
        t.close(p);
    }
}
