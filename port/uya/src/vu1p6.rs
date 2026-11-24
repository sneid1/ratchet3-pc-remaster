//! VU1 program 6, the effects' (`uyafmt::prog6::MICROCODE`, 222 instruction pairs), done by the port's own code from its
//! microcode on `vu1::Vu1` (the module doc there: the arithmetic, the pipeline and the check against the translation).
//!
//! Entry 0x00 loads the 17 constant quadwords at 0 (vf28 to vf31 the camera matrix's rows, vf25 its Q scale, vf26 the screen
//! offset, vf27 a sprite's size scale, vf21 to vf24 the ground quad's corner texture coordinates, vf15 to vf20 GIF tags and
//! colours) and sets the buffer registers (vi14 0x3FF, vi15 0x200, vi08 -2). Entry 0xA8 draws a buffer of items: vi15 turns to
//! the other of the two buffers (0 and 0x200), whose quadword 2 w is the item count in quadwords, and each item is made into its
//! GIF packet from the buffer + 0x80 on, by its first quadword's w (vi09): 0 a sprite (four quadwords in: the centre, its colour,
//! its size and the texture's ST box; a strip of four corners about the centre at the size times vf27 in screen space), a
//! positive one a ground quad (five: four corners about the centre in the world's x and y), -1 a line (four: its two points'
//! strip of two vertices), -2 a ribbon (six: two points and their half widths, four vertices). Every vertex goes through the
//! matrix (ACC = vf31 + vf28 x + vf29 y + vf30 z), its screen position is vf26 + clip.xyz vf25.x / clip.w (FTOI4), w 1.0
//! before the conversion. The packets are kicked at once after the last item, and its GIF tag's first word is then set to
//! 0x8001 (NLOOP 1 with EOP: the GIF reads the packet while the program runs on, `vu1::Vu1::defer_packets`).

use crate::vu1::{bc, s16, sx, Vu1, ACC, W, X, XY, XYZ, XYZW, Y, ZW};

/// A buffer holds 0x200 quadwords; an item is four quadwords or more.
const MOST_ITEMS: u32 = 0x400;

pub(crate) fn run(v: &mut Vu1, entry: usize) -> Result<usize, String> {
    match entry {
        0x00 => Ok(constants(v)),
        0xa8 => items(v),
        e => Err(format!("VU1 program 6 started at {e:#x} (its runs start at 0x0 and 0xA8)")),
    }
}

/// Entry 0x00 (to the E bit at 0x98).
fn constants(v: &mut Vu1) -> usize {
    // 0x000, 0x008: vf11.zw and vf12.zw times vf0.x (0 of their own signs).
    v.mul::<ZW>(11, v.vf[11], bc(v.vf[0][0]));
    v.mul::<ZW>(12, v.vf[12], bc(v.vf[0][0]));
    for (q, r) in [28, 29, 30, 31, 25, 26, 27, 21, 22, 23, 24, 15, 16, 17, 18, 19, 20].into_iter().enumerate() {
        v.lq::<XYZW>(r, q as u32);
    }
    v.set_vi(14, 1023);
    v.set_vi(15, 512);
    v.set_vi(8, 0u32.wrapping_sub(2));
    0xa0
}

/// ACC = vf31 + vf28 p.x + vf29 p.y; vf`d` = ACC + vf30 p.z (MULAw, MADDAx, MADDAy, MADDz).
fn transform(v: &mut Vu1, d: usize, p: [u32; 4], z: u32) {
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.madd::<XYZW>(ACC, v.vf[28], bc(p[0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(p[1]));
    v.madd::<XYZW>(d, v.vf[30], bc(z));
}

/// Entry 0xA8: the buffer's items, kicked; the micro address of the E bit's delay slot.
fn items(v: &mut Vu1) -> Result<usize, String> {
    // 0x0A8..0x0E8: the other buffer; vi12 its end (its count from quadword 2 w), vi10 the item read, vi11 the packet written,
    // vi13 the packets' start, vi09 the first item's kind.
    v.set_vi(15, v.vi[15].wrapping_add(512));
    v.set_vi(15, v.vi[15] & v.vi[14]);
    v.ilw(12, v.vi[15].wrapping_add(2), 3);
    v.ilw(9, v.vi[15], 3);
    v.lq::<XYZW>(1, v.vi[15]);
    v.set_vi(10, v.vi[15]);
    v.set_vi(11, v.vi[15].wrapping_add(128));
    v.set_vi(13, v.vi[15].wrapping_add(128));
    v.set_vi(12, v.vi[12].wrapping_add(v.vi[10]));
    for _ in 0..MOST_ITEMS {
        // 0x0F0: IBNE vi9, vi0 to 0x200; its delay slot (0x0F8) MULAw ACC = vf31 vf0.w and LQ vf14 = 2(vi10).
        let kind = v.vi[9];
        v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
        v.lq::<XYZW>(14, v.vi[10].wrapping_add(2));
        let end = if s16(kind) == 0 {
            sprite(v)
        } else {
            // 0x200: IBLTZ vi9 to 0x408; its delay slot LQ vf13 = 1(vi10).
            v.lq::<XYZW>(13, v.vi[10].wrapping_add(1));
            if s16(kind) >= 0 {
                ground(v)
            } else {
                // 0x408: MULAw ACC = vf31 vf0.w, IBEQ vi9, vi8 (-2) to 0x530; its delay slot MADDAx ACC += vf28 vf1.x, vi1 = vi11.
                let ribbon = s16(v.vi[8]) == s16(v.vi[9]);
                v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
                v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[1][0]));
                v.set_vi(1, v.vi[11]);
                if ribbon { ribbon_item(v) } else { line(v) }
            }
        };
        // Each item ends with IBNE vi12, vi10 back to 0x0F0 (its delay slot run either way); after the last, XGKICK vi13 with the
        // E bit, and in its delay slot ISW.x vi2 (0x8001) to the last packet's tag.
        if s16(v.vi[12]) != s16(v.vi[10]) {
            continue;
        }
        v.kick(v.vi[13]);
        v.isw::<X>(2, v.vi[1]);
        return Ok(end);
    }
    Err(format!("VU1 program 6: a buffer of more than {MOST_ITEMS} items (its end at {:#x} never reached)", v.vi[12]))
}

/// A sprite (0x100..0x1E8, after the dispatch's delay slot): its run's E bit slot is 0x1F8.
fn sprite(v: &mut Vu1) -> usize {
    // 0x100..0x110: the centre to clip space (vf2); its colour (vf13) and the texture's box (vf20.xy).
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[1][0]));
    v.lq::<XYZW>(13, v.vi[10].wrapping_add(1));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[1][1]));
    v.lq::<XY>(20, v.vi[10].wrapping_add(3));
    v.madd::<XYZW>(2, v.vf[30], bc(v.vf[1][2]));
    v.set_vi(10, v.vi[10].wrapping_add(4));
    v.set_vi(1, v.vi[11]);
    // 0x120..0x130: the corners' offsets (vf11 = size vf27; vf12 = (-size.y, size.x vf27.y)); Q = vf25.x / clip.w.
    v.mul::<XY>(11, v.vf[27], v.vf[14]);
    v.set_vi(11, v.vi[11].wrapping_add(12));
    v.mul::<Y>(12, v.vf[27], bc(v.vf[14][0]));
    v.set_vi(2, v.vf[14][2] & 0xffff);
    v.sub::<X>(12, v.vf[0], bc(v.vf[14][1]));
    v.div(v.vf[25][0], v.vf[2][3]);
    // 0x138..0x150: the corners' w 1.0; the next item's first quadword; the packet's tag (vf15) and the texture word.
    v.add::<W>(5, v.vf[0], bc(v.vf[0][0]));
    v.lq::<XYZW>(1, v.vi[10]);
    v.add::<W>(8, v.vf[0], bc(v.vf[0][0]));
    v.add::<W>(6, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1], v.vf[15]);
    v.add::<W>(7, v.vf[0], bc(v.vf[0][0]));
    v.vf[19][0] = sx(v.vi[2]);
    // 0x158..0x178: the corners about the screen offset; the centre times Q.
    v.add::<XYZW>(5, v.vf[26], v.vf[11]);
    v.sub::<XYZW>(8, v.vf[26], v.vf[11]);
    v.st::<XYZW>(v.vi[1].wrapping_add(3), v.vf[13]);
    v.mul::<XYZ>(2, v.vf[2], v.q());
    v.st::<XYZW>(v.vi[1].wrapping_add(2), v.vf[20]);
    v.add::<XYZW>(6, v.vf[26], v.vf[12]);
    v.sub::<XYZW>(7, v.vf[26], v.vf[12]);
    v.st::<XYZW>(v.vi[1].wrapping_add(1), v.vf[19]);
    // 0x188..0x1C0: the corners at the centre's screen position, FTOI4, between the ST quadwords vf21..vf24.
    v.add::<XYZ>(5, v.vf[5], v.vf[2]);
    v.st::<XYZW>(v.vi[1].wrapping_add(4), v.vf[21]);
    v.add::<XYZ>(6, v.vf[6], v.vf[2]);
    v.add::<XYZ>(7, v.vf[7], v.vf[2]);
    v.st::<XYZW>(v.vi[1].wrapping_add(6), v.vf[22]);
    v.add::<XYZ>(8, v.vf[8], v.vf[2]);
    v.ftoi::<1, XYZW>(5, v.vf[5]);
    v.st::<XYZW>(v.vi[1].wrapping_add(8), v.vf[23]);
    v.ftoi::<1, XYZW>(6, v.vf[6]);
    v.set_vi(2, 0u32.wrapping_sub(32767));
    v.ftoi::<1, XYZW>(7, v.vf[7]);
    v.st::<XYZW>(v.vi[1].wrapping_add(10), v.vf[24]);
    v.ftoi::<1, XYZW>(8, v.vf[8]);
    v.set_vi(9, v.vf[1][3] & 0xffff);
    v.st::<XYZW>(v.vi[1].wrapping_add(5), v.vf[5]);
    v.st::<XYZW>(v.vi[1].wrapping_add(7), v.vf[6]);
    v.st::<XYZW>(v.vi[1].wrapping_add(9), v.vf[7]);
    v.st::<XYZW>(v.vi[1].wrapping_add(11), v.vf[8]);
    0x1f8
}

/// A ground quad (0x210..0x3F0): its run's E bit slot is 0x400.
fn ground(v: &mut Vu1) -> usize {
    // 0x210..0x248: the four corners about the centre (vf2, vf3, vf4, vf5; x and y), the texture word to vf19.x.
    v.lq::<XY>(20, v.vi[10].wrapping_add(3));
    v.set_vi(10, v.vi[10].wrapping_add(5));
    v.add::<XY>(2, v.vf[1], v.vf[14]);
    v.set_vi(2, v.vf[14][2] & 0xffff);
    v.sub::<XY>(5, v.vf[1], v.vf[14]);
    v.set_vi(1, v.vi[11]);
    v.sub::<X>(3, v.vf[1], bc(v.vf[14][1]));
    v.set_vi(11, v.vi[11].wrapping_add(15));
    v.add::<X>(4, v.vf[1], bc(v.vf[14][1]));
    v.add::<Y>(3, v.vf[1], bc(v.vf[14][0]));
    v.sub::<Y>(4, v.vf[1], bc(v.vf[14][0]));
    v.vf[19][0] = sx(v.vi[2]);
    // 0x250..0x2A8: the corners to clip space (z the centre's), the tag (vf16), Q for the first.
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.st::<XYZW>(v.vi[1], v.vf[16]);
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[2][0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[2][1]));
    v.madd::<XYZW>(2, v.vf[30], bc(v.vf[1][2]));
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.st::<XYZW>(v.vi[1].wrapping_add(2), v.vf[20]);
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[3][0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[3][1]));
    v.st::<XYZW>(v.vi[1].wrapping_add(1), v.vf[19]);
    v.madd::<XYZW>(3, v.vf[30], bc(v.vf[1][2]));
    v.div(v.vf[25][0], v.vf[2][3]);
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.st::<XYZW>(v.vi[1].wrapping_add(4), v.vf[13]);
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[4][0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[4][1]));
    v.st::<XYZW>(v.vi[1].wrapping_add(7), v.vf[13]);
    v.madd::<XYZW>(4, v.vf[30], bc(v.vf[1][2]));
    v.st::<XYZW>(v.vi[1].wrapping_add(10), v.vf[13]);
    // 0x2C0..0x2E8: each corner's ST (vf21..vf24 times its Q) and position times Q, Q for the next.
    v.mul::<XYZ>(6, v.vf[21], v.q());
    v.st::<XYZW>(v.vi[1].wrapping_add(13), v.vf[13]);
    v.mul::<XYZ>(2, v.vf[2], v.q());
    v.div(v.vf[25][0], v.vf[3][3]);
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[5][0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[5][1]));
    v.madd::<XYZW>(5, v.vf[30], bc(v.vf[1][2]));
    v.mul::<XYZ>(7, v.vf[22], v.q());
    v.mul::<XYZ>(3, v.vf[3], v.q());
    v.div(v.vf[25][0], v.vf[4][3]);
    v.mul::<XYZ>(8, v.vf[23], v.q());
    v.mul::<XYZ>(4, v.vf[4], v.q());
    v.div(v.vf[25][0], v.vf[5][3]);
    // 0x350..0x388: the positions' w 1.0, the STs stored, the last corner.
    v.add::<W>(2, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1].wrapping_add(3), v.vf[6]);
    v.add::<W>(3, v.vf[0], bc(v.vf[0][0]));
    v.add::<W>(5, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1].wrapping_add(6), v.vf[7]);
    v.add::<W>(4, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1].wrapping_add(9), v.vf[8]);
    v.mul::<XYZ>(9, v.vf[24], v.q());
    v.mul::<XYZ>(5, v.vf[5], v.q());
    // 0x390..0x3F0: the screen positions (FTOI4) stored; the next item's first quadword.
    v.add::<XYZ>(2, v.vf[2], v.vf[26]);
    v.lq::<XYZW>(1, v.vi[10]);
    v.add::<XYZ>(3, v.vf[3], v.vf[26]);
    v.add::<XYZ>(4, v.vf[4], v.vf[26]);
    v.add::<XYZ>(5, v.vf[5], v.vf[26]);
    v.st::<XYZW>(v.vi[1].wrapping_add(12), v.vf[9]);
    v.ftoi::<1, XYZW>(2, v.vf[2]);
    v.set_vi(2, 0u32.wrapping_sub(32767));
    v.ftoi::<1, XYZW>(3, v.vf[3]);
    v.ftoi::<1, XYZW>(4, v.vf[4]);
    v.ftoi::<1, XYZW>(5, v.vf[5]);
    v.set_vi(9, v.vf[1][3] & 0xffff);
    v.st::<XYZW>(v.vi[1].wrapping_add(5), v.vf[2]);
    v.st::<XYZW>(v.vi[1].wrapping_add(8), v.vf[3]);
    v.st::<XYZW>(v.vi[1].wrapping_add(11), v.vf[4]);
    v.st::<XYZW>(v.vi[1].wrapping_add(14), v.vf[5]);
    0x400
}

/// A line (0x418..0x518, after 0x408's pair and its delay slot): its run's E bit slot is 0x528.
fn line(v: &mut Vu1) -> usize {
    // 0x418..0x440: its two points to clip space (vf3, vf4); Q for the first.
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[1][1]));
    v.set_vi(11, v.vi[11].wrapping_add(6));
    v.madd::<XYZW>(3, v.vf[30], bc(v.vf[1][2]));
    v.set_vi(2, 68);
    let p = v.vf[14];
    transform(v, 4, p, p[2]);
    v.div(v.vf[25][0], v.vf[3][3]);
    // 0x448..0x4B0: w 1.0, its colours (vf13 and the item's fourth quadword, vf14), the tag (vf17) and vf19.x = 68.
    v.add::<W>(3, v.vf[0], bc(v.vf[0][0]));
    v.lq::<XYZW>(14, v.vi[10].wrapping_add(3));
    v.vf[19][0] = sx(v.vi[2]);
    v.set_vi(10, v.vi[10].wrapping_add(4));
    v.mul::<XYZ>(3, v.vf[3], v.q());
    v.div(v.vf[25][0], v.vf[4][3]);
    v.add::<W>(4, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1], v.vf[17]);
    v.st::<XYZW>(v.vi[1].wrapping_add(1), v.vf[19]);
    v.st::<XYZW>(v.vi[1].wrapping_add(2), v.vf[13]);
    v.mul::<XYZ>(4, v.vf[4], v.q());
    v.st::<XYZW>(v.vi[1].wrapping_add(4), v.vf[14]);
    // 0x4B8..0x518: the next item's first quadword; the screen positions.
    v.lq::<XYZW>(1, v.vi[10]);
    v.add::<XYZ>(3, v.vf[3], v.vf[26]);
    v.add::<XYZ>(4, v.vf[4], v.vf[26]);
    v.ftoi::<1, XYZW>(3, v.vf[3]);
    v.ftoi::<1, XYZW>(4, v.vf[4]);
    v.set_vi(9, v.vf[1][3] & 0xffff);
    v.set_vi(2, 0u32.wrapping_sub(32767));
    v.st::<XYZW>(v.vi[1].wrapping_add(3), v.vf[3]);
    v.st::<XYZW>(v.vi[1].wrapping_add(5), v.vf[4]);
    0x528
}

/// A ribbon (0x530..0x6D8): its run's E bit slot is 0x6E8.
fn ribbon_item(v: &mut Vu1) -> usize {
    // 0x530..0x570: the first point's two vertices (vf1 +- vf14), the second's (vf13 +- vf14 vf13.w); the texture word.
    v.add::<XYZ>(2, v.vf[1], v.vf[14]);
    v.lq::<XY>(20, v.vi[10].wrapping_add(5));
    v.sub::<XYZ>(3, v.vf[1], v.vf[14]);
    v.mul::<XYZ>(14, v.vf[14], bc(v.vf[13][3]));
    v.ilw(2, v.vi[10].wrapping_add(5), 2);
    let p = v.vf[2];
    transform(v, 2, p, p[2]);
    v.add::<XYZ>(4, v.vf[13], v.vf[14]);
    v.set_vi(11, v.vi[11].wrapping_add(13));
    v.sub::<XYZ>(5, v.vf[13], v.vf[14]);
    // 0x578..0x5B8: vertices 2 and 3 to clip space; Q for the first; the colours (vf13, vf14).
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.vf[19][0] = sx(v.vi[2]);
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[3][0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[3][1]));
    v.madd::<XYZW>(3, v.vf[30], bc(v.vf[3][2]));
    v.div(v.vf[25][0], v.vf[2][3]);
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.lq::<XYZW>(13, v.vi[10].wrapping_add(3));
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[4][0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[4][1]));
    v.lq::<XYZW>(14, v.vi[10].wrapping_add(4));
    v.madd::<XYZW>(4, v.vf[30], bc(v.vf[4][2]));
    v.set_vi(10, v.vi[10].wrapping_add(6));
    // 0x5C8..0x670: each vertex times its Q, the next's Q; the tag (vf18), colours, texture word and ST box stored.
    v.mul::<XYZ>(2, v.vf[2], v.q());
    v.div(v.vf[25][0], v.vf[3][3]);
    v.mul::<XYZW>(ACC, v.vf[31], bc(v.vf[0][3]));
    v.st::<XYZW>(v.vi[1], v.vf[18]);
    v.madd::<XYZW>(ACC, v.vf[28], bc(v.vf[5][0]));
    v.madd::<XYZW>(ACC, v.vf[29], bc(v.vf[5][1]));
    v.st::<XYZW>(v.vi[1].wrapping_add(3), v.vf[13]);
    v.madd::<XYZW>(5, v.vf[30], bc(v.vf[5][2]));
    v.st::<XYZW>(v.vi[1].wrapping_add(8), v.vf[14]);
    v.mul::<XYZ>(3, v.vf[3], v.q());
    v.div(v.vf[25][0], v.vf[4][3]);
    v.st::<XYZW>(v.vi[1].wrapping_add(1), v.vf[19]);
    v.st::<XYZW>(v.vi[1].wrapping_add(2), v.vf[20]);
    v.mul::<XYZ>(4, v.vf[4], v.q());
    v.div(v.vf[25][0], v.vf[5][3]);
    v.add::<W>(2, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1].wrapping_add(4), v.vf[21]);
    v.add::<W>(3, v.vf[0], bc(v.vf[0][0]));
    v.add::<W>(5, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1].wrapping_add(6), v.vf[22]);
    v.add::<W>(4, v.vf[0], bc(v.vf[0][0]));
    v.st::<XYZW>(v.vi[1].wrapping_add(9), v.vf[23]);
    v.mul::<XYZ>(5, v.vf[5], v.q());
    v.st::<XYZW>(v.vi[1].wrapping_add(11), v.vf[24]);
    // 0x678..0x6D8: the next item's first quadword; the screen positions.
    v.add::<XYZ>(2, v.vf[2], v.vf[26]);
    v.lq::<XYZW>(1, v.vi[10]);
    v.add::<XYZ>(3, v.vf[3], v.vf[26]);
    v.add::<XYZ>(4, v.vf[4], v.vf[26]);
    v.add::<XYZ>(5, v.vf[5], v.vf[26]);
    v.ftoi::<1, XYZW>(2, v.vf[2]);
    v.set_vi(2, 0u32.wrapping_sub(32767));
    v.ftoi::<1, XYZW>(3, v.vf[3]);
    v.ftoi::<1, XYZW>(4, v.vf[4]);
    v.ftoi::<1, XYZW>(5, v.vf[5]);
    v.set_vi(9, v.vf[1][3] & 0xffff);
    v.st::<XYZW>(v.vi[1].wrapping_add(5), v.vf[2]);
    v.st::<XYZW>(v.vi[1].wrapping_add(7), v.vf[3]);
    v.st::<XYZW>(v.vi[1].wrapping_add(10), v.vf[4]);
    v.st::<XYZW>(v.vi[1].wrapping_add(12), v.vf[5]);
    0x6e8
}
