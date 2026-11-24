//! Moby classes (the LEVEL core's moby class data and the SCENE blocks' classes), split into structures as R&C3's own code reads
//! them. The class loader is func_L01_00424368, which calls func_L01_00424048 to relocate the header's offsets into pointers;
//! the instance constructor is func_L01_00435F98; func_L01_0043EA78 draws the meshes. All of these are the same in all 34 story
//! levels.
//!
//! A class is a header and structures that the header's offsets, the structures' own offsets and two 16-byte-unit indices
//! reach; each structure runs to the next one's start and may be followed by zero bytes up to it:
//! - the header: 0x48 bytes and one offset per animation sequence (the byte at +0xC counts them);
//! - the LOD table (header +0x2C = k: the table is at 16 * k, 0x40 bytes: a 4-byte header, then 4-byte entries
//!   [start, count, start, count] of two runs of mesh records that func_L01_0043EA78 picks by level of detail), then
//!   attachment point pairs (16 bytes: two points of x, y, z, joint as s16, read by func_L01_004367E8) up to the next structure;
//! - the hull blocks (header +0x2E = e, s16: 16 * e): an index row of 16 bytes (a block number in 16-byte units from the row's
//!   start, 0xFF for none; func_L01_00436988 and func_L01_004369B8 read byte +1 + n for point list n) and the blocks: a 16-byte
//!   sphere (func_L01_0040BEC0 loads it with lqc2) and 8-byte points (x, y, z, w as s16, func_L01_0040C368), the count being
//!   the s16 at +0x16 of the block; an empty block is a zero sphere;
//! - the animation sequences (a 0x1C-byte header and one frame offset per frame; the byte at +0x10 counts the frames) with their
//!   frames, and a 0x20-byte record when +0x14 is not zero (func_L01_00450D80); a frame offset may carry 0xF0 in its top byte
//!   (the class 7994's only frame; the frame is at the low 24 bits);
//! - the mesh records: 16 bytes each, counted by +4 + +5 + +6 and the LOD table's +1 (func_L01_00424048 relocates +0 and +8):
//!   +0 offset of packet A, +4 the quadword count of A (low byte; byte +5 is zero) and the quadword count of the texture setup
//!   at its end (the high half, which func_L01_00423E10 clears after the load), +8 offset of packet B, +0xC four bytes: the
//!   quadword count of B, the quadword counts of the two buffers func_L01_0043EA78 reserves for B's output (positions as three
//!   s16, normals as four bytes) and the vertex count;
//! - packet A: a VIF1 stream (func_L01_0043EA78 gives the DMA tag): UNPACK codes (V2-16 0x75, V4-8 0x6E and, at the end, V4-32
//!   0x6C) each followed by its data, then NOP codes to a quadword. The unpacks fill VU1 program 0's buffers at 0xC2, 0x12D and
//!   0x14E (plus the TOP offset); the 0x6C block is the texture setup, 64 bytes per texture, which func_L01_00423E10 and
//!   func_L01_00423700 rewrite (word 0, word 1 and word 8, the texture number, are read; the rest is overwritten);
//! - packet B: the vertex rows that func_L01_0043EA78 queues in a job list and func_003D00F8 (level 1: 0x3F6FF0) skins into the
//!   VU1 buffers (V3-16 positions and V4-8 normals): a header, joint matrix pairs, vertex copy slots and 16-byte rows (see
//!   `K::PB`; two layouts, skinned through VU0 and with the joint numbers and weights in the row);
//! - the header +0x10 collision data (16-byte header of three sizes, 0x20-byte sphere and capsule entries, 8-byte points and 4-byte
//!   faces; the sphere sweeps func_003CE740, func_L01_003F9498, func_L01_003FA2D8 and func_003CF790 read it through the moby
//!   instance's +0x98 pointer, which func_L01_00442E08 sets to the class's +0x10), the joint matrices (header +0x14, 64 bytes per joint, the joint count being the
//!   byte at +8), the joint table (header +0x18, 16 bytes per joint: x, y, z and the scratchpad address of the parent joint's
//!   matrix, 0x70000000 + 0x40 * parent, or 0 for the root; func_L01_0043AAF8 DMAs it to the scratchpad), the rows before the
//!   joint matrices that func_L01_00422AB8 overwrites (header +0xF rows of 16 bytes), the list table (header +0x1C: a count and
//!   offsets of lists: a u16 n, a byte m, a zero byte, n joint numbers, m more bytes and 0xFF), the effect records (header
//!   +0x28, 0x20 bytes each, +0xD of them; func_L01_004A6F58 reads +0x10, +0x14, +0x18, +0x1A);
//! - the texture-id records (header +0x20; 16 bytes each, the last has a negative +0xC) and, when the byte at +0xB is not zero,
//!   in front of them 16 bytes that func_L01_00424048 fills with palette addresses and (high nibble * low nibble) palettes of
//!   0x400 bytes (func_L01_00437C88).
//!
//! Frames and base blocks are decoded by anim.rs in the `Space` of the class (the front end's model blocks are on the disc). The
//! WAD5 armor model blocks (`armor`) are the same structures in two forms: records 0 to 34 are a 16-byte header, mesh records and
//! packets that func_L01_00423F28 relocates and func_L01_00424008 patches into moby class 0's header; records 35 to 55 are whole
//! moby classes.
//!
//!
//! # VU1 program 0: how a mesh becomes GS vertices
//!
//! Program 0 (VU1 microcode, RAM 0x110798, 10680 bytes, `scripts/uya_vugen.py` PROGS[0]) draws one mesh record per run. The EE's
//! func_L01_0043EA78 (draw queue loop) builds the DMA chain: per mesh record the 6 constant quadwords at TOPS + 0x123 and the 4
//! matrix rows at TOPS + 0x129, the skinned positions (V3-16, TOPS + 0) and the second V4-8 buffer (TOPS + 0x61, packet B's
//! output) from func_003D00F8's job, then packet A's three UNPACKs, then MSCAL. TOPS alternates between 0 and 0x164 from run to run
//! (the double buffer; the program reads XTOP and works on addresses relative to it). Every address below is TOPS + the offset.
//!
//! Entries (MSCAL operand times 8): 0x00 (once per frame: sets the output buffer pointers vf27 = 0x2C8, 0x348 and ends), 0x40 and
//! 0x60 (as 0x50 and 0x70 with TOPS taken as 0x164), 0x50 (variant A, below), 0x70 (variant B), 0xA0 (a pre-pass that turns
//! `count = |header w|` quadwords of each of the position, colour and UV buffers from float to int with FTOI12 and then runs A;
//! its count is byte +3 of the strip list header, zero in every packet on the disc). func_L01_0043EA78 loads the MSCAL operand
//! 0xA at 0x43EF34 and replaces it with 0xE at 0x43EF88 (so B), which the instance reaches only when the tests at 0x43EF40 to
//! 0x43EF80 fall through (the branches at 0x43EF78 and 0x43EF80 skip 0x43EF88 and keep A; the others leave this path); the tests
//! compare the instance's bounding sphere (the queue entry's +0, radius in w) with the instance's distance limit and the view
//! limits in vf19 to vf23. In the traced frame 364 runs are B and 11 are A.
//!
//! ## Inputs of a run (VU1 quadwords, TOPS-relative)
//!
//! | offset | what | written by |
//! |---|---|---|
//! | 0x000 to 0x060 | positions, one quadword per vertex: x, y, z as 32-bit words holding the s16 value (V3-16 UNPACK 0x69, w 0), up to 97 vertices | func_003D00F8 (packet B's skin) |
//! | 0x061 to 0x0C1 | colours, one quadword per vertex: r, g, b, a as words holding bytes (V4-8 UNPACK 0x6E): the lit vertex colour, which program 0 copies to the GS unchanged | func_003D00F8 (packet B's second output; the lighting is the skin job's, not VU1's) |
//! | 0x0C2 to 0x122 | texture coordinates: s16 u, s16 v as words (V2-16 UNPACK 0x75 of packet A), z = 0x1000 (the VIF mask 0x20202020 selects COL for lane z and STCOL sets 0x1000 in every lane), w 0 | packet A |
//! | 0x123 (vf24) | A+D GIF tag template (NLOOP, then word 1 0x103E4000: PRE, NREG 1, register 0xE); its word x is also read as a float: the lowest fog value (140.25 in the trace) | the 6 global quadwords at 0x1E7600 to 0x1E7650 (gp - 0x27C0 to gp - 0x2770), copied to scratchpad 0x70003E80 to 0x70003ED0 at 0x43ECD8 to 0x43ED04 and DMA'd by the UNPACK 0x6C to TOPS + 0x123 (0x43ED70 to 0x43ED94) |
//! | 0x124 (vf23) | strip GIF tag template: x is the float Q numerator (-0.02381), word y 0x303E4000 (PRE, PRIM 0x7C = triangle strip, Gouraud, textured, fog, alpha blend, STQ, context 1; NREG 3), z 0x412 (ST, RGBAQ, XYZF2), w; the program stores it at each strip start and overwrites x with NLOOP and the EOP bit | same |
//! | 0x125 (vf28) | 4.0, 4.0, 1.0, 1.0: clip scale used only by the clipping code | same |
//! | 0x126 (vf21) | the clip box scale: 1/1024, 0.001197, -1.2e-7, -41.83 (x, y, z, w) | same |
//! | 0x127 | not read by program 0 | same |
//! | 0x128 (vf22) | 2048.0, 2048.0 (GS window centre x and y), 8388112.0 (z offset), 293.25 (fog offset) | same |
//! | 0x129 to 0x12B (vf17 to vf19) | rows 0 to 2 of the instance's model-to-clip matrix (the instance's scale, `lw` of +0x2C times 1/1024, is multiplied into them at 0x43F0BC to 0x43F0C4; the view and projection come from the 4 x 4 at the camera globals) | func_L01_0043EA78 at 0x43F0C8 to 0x43F104 and 0x440200 to 0x44022C |
//! | 0x12C (vf20) | row 3 of that matrix (the translation; its w lane is the fog base) | same |
//! | 0x12D | strip list (V4-8 UNPACK 0x6E of packet A): the header quadword, then the entries | packet A |
//! | 0x12D + the list's quadword count | texture setups (V4-32 UNPACK 0x6C): four quadwords per texture | packet A, words rewritten by func_L01_00423E10 |
//!
//! Output: the program writes GIF packets to 0x2C8 or 0x348 (0x80 quadwords each, alternating) and XGKICKs them. The GS state it
//! relies on (ALPHA, TEST, the texture's TEX0 palette and mips) is the EE's.
//!
//! ## Strip list
//!
//! Quadword 0 of the list (V4-8, bytes as signed values; the program reads them with ILW of the word): x = the cull mode (see
//! "Culling"), y = the quadword count of the list when texture setups follow it and 0 when none do, z = the first vertex of
//! the first zero-started group (1-based list value), w = the pre-pass count. Quadwords 1 on hold the entries, four bytes each, in
//! order. An entry is a 1-based vertex number v: positive v means vertex v - 1 without ADC, negative v means vertex v + 127 (the
//! byte is 0x80 + number) with ADC; 0 starts a group.
//!
//! The program reads the entries four ahead and handles a 0 as a group boundary: it closes the strip, writes the A+D packet of the
//! next texture setup (TEX1_1, CLAMP_1, TEX0_1, MIPTBP1_1; the four quadwords are copied, word z of the first is converted from the
//! float 20.0 to the register number 0x14 by FTOI0), opens a new strip, and replaces the 0 by the group's first vertex with ADC set:
//! group k (k = 0, 1, ...) starts with the header's byte +2 (k = 0) or the w word of quadword k - 1 of the texture-setup area
//! (k >= 1; `ILW.w vi12, 0(vi11)` at micro 0x1360 for the first boundary and `ILW.w vi12, 0(vi01)` in each boundary handler, for
//! example at 0x1B50, with vi01 recovered from vf10w and advanced by one quadword at each boundary). A group boundary whose
//! first vertex is 0 ends the run (the last group's end marker). The zeros are, in order, the starts of groups 0 to n - 1 for the n
//! texture setups and then the end marker; entries before the first zero (when entry 0 is not zero) form a first strip drawn with the
//! texture bound before it; the last strip's final three entries (always 1) are the pipeline's look-ahead and are never drawn; all
//! bytes after the end marker are 0. `PacketA::strips` is this decomposition: one `Strip` per group, its first vertex with `skip`
//! set, texture `Some(k)` for the group of setup k and `None` for the first strip. The vertices the GS receives are the strip's, in
//! order. The program cuts a long strip into GIF packets of up to 42 vertices (0x80 quadwords, 5 taken by an A+D packet) and
//! repeats the last two vertices, with their ADC bits, at the start of the next packet; the GS draws the same triangles as the
//! whole strip: for each vertex k >= 2 without ADC the triangle (k-2, k-1, k).
//!
//! ## Per-vertex formula (variants A and B)
//!
//! Inputs: the vertex's position p = (x, y, z) as f32 (ITOF0 of the stored word), its u and v as f32 (ITOF12: s16 / 4096), the colour
//! words, and the quadword values vf17 to vf20 (matrix rows), vf22, vf23.x and vf24.x of the table. All f32 operations of the
//! VU1 round toward zero (the products and the sums separately, as the GS words of the trace show; the interpreter's IEEE
//! rounding to nearest differs in the last bit of some words). Let `pos` be the vertex's position in the mesh's stream: the vertices
//! of all strips in order, 0 for the first.
//!
//! 1. clip = (vf20 + x * vf17 + y * vf18 + z * vf19) in all four lanes, accumulated in this order: for pos 0 and 1 (both variants)
//!    vf20, then x, y, z; for pos >= 2 in variant B vf20, then z, y, x; in variant A x * vf17, then y, then z, then vf20 last.
//! 2. Q = vf23.x / clip.w (DIV).
//! 3. XYZ: X = clip.x * Q + vf22.x, Y = clip.y * Q + vf22.y, Z = clip.z * Q + vf22.z; the GS word is FTOI4 (times 16, truncated) of
//!    each, so X and Y are 12.4 fixed point screen coordinates and Z is the depth.
//! 4. F: variant B: fog = vf20.w + vf22.w for every vertex of the instance; variant A: fog = clip.w + vf22.w. Then fog = max(fog, vf24.x),
//!    fog = min(fog, 255.0); if the vertex has ADC, fog = fog + 3072.0 (so the GS word has bit 15 set: the ADC bit and bit 14, which
//!    the GS ignores); the GS word is FTOI4(fog), whose bits 4 to 11 are F.
//! 5. ST/Q: S = u * Q, T = v * Q, Q' = Q (the stored third lane is 1.0 * Q). The fourth word is not written by the program (stale; the
//!    GS ignores it). Exception (variant B, 17 of the 364 runs): when the stream's fourth vertex has ADC (entry 3 of the list negative)
//!    the program multiplies vertex pos 1's ST by Q a second time (`MULq.xyz vf10` at micro 0x1490 and again at 0x1C70 on the
//!    path IBLTZ vi05 takes), so S, T and Q' of that vertex are u * Q * Q, v * Q * Q and Q * Q.
//! 6. RGBA: the colour buffer's words, unchanged (the GS reads the low 8 bits of each word; an alpha of 0x80 is 1.0).
//! 7. ADC (bit 15 of the XYZF2 word's fourth word): set for vertices with `skip`.
//!
//! Per moby (not per vertex): the matrix rows, vf22, vf23.x, vf24.x, and in variant B the fog value. Per vertex: everything else.
//!
//! The vertices go to the GS as ST, RGBAQ, XYZF2 in PACKED mode with PRIM 0x7C from the tag template: Gouraud triangle strips,
//! textured by the texture bound by the preceding A+D packet (TME on, STQ coordinates), fog on, alpha blend on.
//!
//! ## Texture binding
//!
//! A strip with `texture` Some(k) is drawn with the A+D registers built from `PacketA::textures[k]`: TEX1_1 = ((levels - 1) << 2) | (mmin
//! << 6) | 0x20 | (k << 32), CLAMP_1 = wms | (wmt << 2) | (r << 24), with the record r (the texture's table-1 record number) given
//! by the class record's texture index bytes: r = bytes[local] (func_L01_00423E10 at 0x423E80 to 0x423EA0: the block's word 8 is
//! the class-local number t; `lbu` of byte t of the 16 bytes at the class record's +0x10 (the index's moby class records are 32
//! bytes, words 6 and 7); the record at the table's base + 16 * r, where the base is index + word 0xF's value, 0x428C30 and 0x424340);
//! func_L01_00423700 (0x423700 to 0x4239F4) writes the registers from the block's words 0, 1, 4, 5 and that record. A strip with
//! `texture` None keeps the texture of the previous strip drawn. A byte 0xFF in the index bytes means no record. The trace shows
//! CLAMP_1 bits 24 and up equal to r for every textured group of the core's classes and TEX1_1's MMIN and K equal to the words.
//!
//! ## Culling (variant A only)
//!
//! Variant B ignores the strip list's byte +0. In variant A (MSCAL 0x50) byte +0 = -2 draws every triangle; 0 or 2 sets ADC on the
//! third vertex of every triangle k (k >= 2, without ADC) whose flag S equals 0 (byte 0) or 1 (byte 2), where S = 1 when the
//! screen-space orientation is negative: with s the vertices' X, Y after step 3 (float), e1 = s[k-1] - s[k-2], e2 = s[k] - s[k-1], (a, b)
//! = (e1, e2) for even k and (e2, e1) for odd k (k = the stream position `pos`), the VU1 computes a.x * b.y - b.x * a.y (the first
//! product rounded toward zero, then the difference, `MULAy.x` and `MSUBy.x` at micro 0x428 and 0x430 and the same pairs) and S is
//! its sign flag (FSAND 0x002: the result is negative). The trace holds no mesh with byte +0 other than -2; the rule is checked on all 11
//! variant A runs by running the game's program on them with the byte set to 0 and 2 (1864 triangles, every ADC as the rule
//! says). On the disc 2 mesh records of 50367 have byte +0 = 0 (see `PacketA::cull`).
//!
//! ## Clipping
//!
//! After the third vertex of a triangle the program tests vf13 to vf16 = clip * vf21 (CLIP against |w * vf21.w|) for each vertex.
//! A triangle with all three vertices outside the same plane is sent as it is (the GS discards it); one with a vertex outside the
//! box but not all three outside the same plane is replaced by a clipped polygon computed by the subroutine at micro 0x1CD0 to 0x29B0
//! (B) or the code at 0xE60 to 0x1230 (A). That code is not decoded here and the trace holds no run that executes it; a renderer
//! that draws the triangles the formula gives and lets the GPU clip gets the visible result.
//!
//! What stays undecoded: the joint matrices' w lanes are read by nothing (marked read: loaded by lqc2).
//!
//!
//! # The draw job and the skin job (func_L01_0043EA78, func_003C5D90, func_L01_00439E00, func_003D00F8)
//!
//! Program 0 draws what the EE hands it: per mesh the skinned positions (TOPS + 0), the lit colours (TOPS + 0x61) and, for rigid
//! meshes, the texture coordinates (TOPS + 0xC2). Level 1's addresses (gp = 0x1DC8B0; the globals are the level overlay's .bss and
//! are another level's own; the code named func_003xxxxx is boot_elf's, at another address in each level's image):
//!
//! 1. func_L01_0043EA78 (0x43EA78; called at 0x438088 for the first list and 0x438160 for the moby array `lw 0x1E009C`, 0x100 bytes
//!    per moby; an instance whose +0x20 byte is negative is skipped, 0x43EE90) loops over the instances. For each visible one it appends a
//!    **draw record** to a job list (built in the scratchpad at 0x70002C00 / 0x70003000, flushed to RAM; the lists of the last frames
//!    stay in RAM: the traced Veldin frame's is at 0x800000, the next frame's at 0x680000) and the VU1 DMA chain.
//! 2. func_003C5D90 (level 1 0x43E998; called at 0x437EE0 after func_L01_00437EB0 has copied the (cos, sin) table to 0x70003800 and
//!    with the list head `lw 0x1DFBF0` and a1 = `lw 0x1DFBF8` = the 0x2000-byte RAM buffer 0x67E000) runs every record in list order:
//!    func_L01_00439E00 (0x439E00: builds the instance's joint matrices in the scratchpad 0x70000000 + 0x40 j) and func_003D00F8
//!    (level 1 0x3F6FF0: skins the record's meshes; returns the next record).
//!
//! ## The draw record (`SkinInputs`, `JobLights`, `RigidInputs`)
//!
//! A record is a 0x40-byte header, the light section (+0x40 to +0xBF, `JobLights`), then n 8-byte mesh entries (packet B's RAM address
//! << 4 | its quadword count (low byte), the destination RAM address of the mesh's output arrays) and a zero entry; a second group
//! (`RigidInputs`) follows when +0x14 is not zero. Header (func_L01_0043EA78 0x43F158 to 0x43F25C): +0 u32 mesh entry count n (the
//! class's +4 or +5 mesh count of the run drawn, or the LOD run's), +4 the instance's +0x44 (a float: the blend of the two pose
//! buffers), +8 the instance's +0x50, +0xC its +0x54, +0x10 the joint count (class +8 near, +9 far; bit 26 set, `sh 0x400, 0x12(t8)`, when byte 2 of the
//! instance's +0x40 word equals its byte 3 and byte 0 + 1 equals byte 1), +0x14 the second group's quadword count << 16 | alpha, +0x18 class +0x18 (joint table),
//! +0x1C class +0x14 (bind matrices), +0x20 / +0x24 / +0x28 / +0x2C the data, quadword count and two list counts of the pose buffer at
//! instance +0x58 (the buffer's +0x10, +6, +8, +0xC), +0x30 / +0x34 / +0x38 / +0x3C the same for +0x5C.
//!
//! **Which mesh records.** The class's mesh records start at class +0 (relocated pointer); the record array is the class's
//! `meshes`. The instance's +0x7C halfword s4: when it is not negative the run is `meshes_near` records from 0 (class +4; `lbu 4(s1)`
//! at 0x43F17C) or, when the depth test at 0x43F134 to 0x43F168 says far (`t0` = the integer left in vf1 by the culling code
//! at 0x43EF6C, clamped to the limit in the high half of s7 and to 0, minus the byte at instance +0x62 << 10, greater than 0), `meshes_far` records from
//! record `meshes_near` (class +5 from 0x43F190; joint count class +9); when s4 is negative the LOD table is used (0x43FBA0 to
//! 0x43FC58): bits 0 to 14 of s4 are a mask, entries from the highest set bit down: entry b is at class + 0x10 * (header +0x2C) + 4
//! + 4 b (`LodTable::entries[b]`), its run (start, count) is drawn, or its alternate run when the draw is the far one and that run's
//! start is not zero; the records count from record 0.
//!
//! **The per-instance constants of program 0** (TOPS + 0x129 to + 0x12C, 0x43F078 to 0x43F104): the four rows are the instance's
//! 4 x 4 transformed by the camera 4 x 4. Let R0, R1, R2 be the instance's rotation rows (x, y, z, w) at +0xC0, +0xD0, +0xE0, scaled
//! in x, y, z by the float at instance +0x2C (`vmulx.xyz` by vf10x at 0x43F0BC to 0x43F0C4), and T the instance's position (+0x10)
//! times 1024 (`vmulw.xyz` by vf24w) minus vf24 (the camera position at 0x1E2A40 times 1024, `lqc2 0x140(0x1E2900)`, scaled by vf1x =
//! 1024.0 at 0x43EC70). With the camera matrix rows C0 to C3 (0x1E2940, 0x1E2950, 0x1E2960, 0x1E2970; vf28 to vf31), row i of the
//! upload is C0 * Ri.x + C1 * Ri.y + C2 * Ri.z + C3 * Ri.w for i = 0, 1, 2 and C0 * T.x + C1 * T.y + C2 * T.z + C3 for the fourth
//! (0x43F0C8 to 0x43F104, the chains of VMULAx, VMADDAy, VMADDAz, VMADDw; the same code path at 0x440200 to 0x44022C for the second
//! draw). The constants at TOPS + 0x123 to + 0x128 are the module doc's table.
//!
//! ## The skin job (`skin`)
//!
//! func_003D00F8 DMAs the light section to 0x70001C00, loads vf25 to vf31 from it, then for each mesh entry DMAs packet B to the
//! scratchpad (0x2000 or 0x2800, double buffered), runs its rows through VU0 (program: RAM 0x10008C, MPG of 256 instructions to
//! micro address 0, and 0x100898, 72 instructions to micro address 0x800; boot_elf's data) and DMAs the three output arrays to the
//! entry's destination. VU0's data memory (256 quadwords) and the staging array at 0x67E000 are never cleared (`SkinState`). The
//! skinned layout (func_003D00F8 0x3F6FF0 to 0x3F8044, micro program 0 to 0x7FF):
//!
//! - at the start of every mesh each (joint number, address) pair of packet B copies the joint matrix to VU0 address 16 * address;
//! - a row (16 bytes: u16 bits 0 to 8 slot, bits 9 to 15 joint j; bytes 2, 3 = b2, b3; weights b4 to b7; angle indices lookup[0],
//!   lookup[1]; position x, y, z s16) belongs to group 1, 2 or 3 by its index against the header's three counts. Group 1 (two
//!   joints): matrix M = blend of the matrices at VU0 b2 and b3 with weights b4 / 256 and b5 / 256 (VMULAx, VMADDy per row and
//!   lane, every product and sum rounded toward zero); the joint matrix j is stored at b6 and M at b7. Group 2 (three joints):
//!   the matrices at b2, b3 and 2 * j, weights b4, b5, b6 over 256, M stored at b7. Group 3: M is the matrix at b2 and the joint
//!   matrix j is stored at b3. (Rows later in the packet or in later meshes read what these stores left.)
//! - position: M's rows 0 to 2 times (x, y, z) plus row 3 (VMULAy, VMADDAz, VMADDAw, VMADDw in that order), truncated to integers
//!   (FTOI0) and stored as s16;
//! - normal: (cos A * cos B, sin A * cos B, sin B) from the (cos, sin) table, rotated by M's rows 0 to 2 (the same chain without the
//!   fourth row); its length (the sum of the squares, VRSQRT) gives Q = 1 / sqrt; the two light values d_k = row0_k * nx + row1_k * ny
//!   + row2_k * nz over the three light direction rows, s_k = max(d_k, d_k * min(wrap_k, 0));
//! - colour: base + Q * (col0 * s0 + col1 * s1 + col2 * s2) per r, g, b, a lane, as floats 65536 + k / 128; the byte is the s16 of the
//!   low 16 bits of the float, times 128 saturated to 16 bits, shifted right by 7 (`ppach`, `pmulth`, `pmfhl.sh`, `psrah`, `ppacb`),
//!   which is the low 16 bits clamped to 0 to 255;
//! - the staging records: row r's position and colour are stored in staging record `slot` of the row 7 after it (the encoder
//!   compensates the pipeline's delay); the rows past the last use the u16s of the look-ahead rows (`SkinnedB::tail`): row r with
//!   r + 7 = rows + k uses u16 0 of look-ahead row k while k is below the look-ahead count, the later ones lane 2, 3, ... of the last
//!   look-ahead row; the slot 255 is a trash record;
//! - the output vertices are the rows in order, then one per vertex copy: the staging record (entry >> 7); header +0xE (the count of
//!   normals filled with a constant) makes the last that many vertices' colours the record's +0xAC word.
//!
//! The rigid layout (func_003D00F8's second entry 0x3F8048, micro program 0x888 to 0xA3F): per row the matrix is joint 0's matrix
//! (count 0 or 1), or the blend of joints 0, 1 (and 2) with weights b12.. / 256 (count 2, 3); position and normal as above; the
//! light values are max(d_k, 0), the colour the constant 65536 + 0x70 / 128 (alpha 0x80) plus col_k * 0.25 * s_k, the byte the low 8
//! bits of the float (no clamp) with alpha the record's +0x14 low byte; the texture coordinates are
//! ((rows 0 to 2 of the second group's matrix) . normal + 1.0) * 4096 as s16, shifted right by one. The outputs are the texture
//! coordinates (4 n bytes), positions (6 n), colours (4 n), each padded to 16 bytes, in that order from the destination.
//!
//! The arithmetic is the VU's (`vmul`, `vadd`): products and sums rounded toward zero (a sum is the exact value rounded toward
//! zero, including when one operand is more than 2^53 times smaller), denormals read as and become zero.
//!
//! Level overlay .bss (level 1 addresses; every other level has its own): the moby array pointer 0x1E009C, the light sets 0x1F17C0
//! (20 sets of 0x40 bytes), the camera 0x1E2900 to 0x1E2A5F, the job list head 0x1DFBF0, the job lists, the pose buffers (the
//! instance's +0x58 and +0x5C), the instances. boot_elf data (the same bytes in every level; the address is the level's): the VU0 program,
//! the (cos, sin) table (level 1 0x2BF5A0, `SkinInputs::trig`).

use crate::disc::{u16_at, u32_at};
use crate::track::{NOTHING, Node, Space, Tracker};
use std::collections::HashSet;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum K {
    Header,
    Lod,
    Attach,
    Hull,
    Rec,
    T10,
    T14,
    T18,
    T28,
    Rt,
    LTab,
    List,
    TexId,
    PalShorts,
    Pal,
    Seq,
    Sx,
    Frame,
    Base,
    PA,
    PB,
}

struct Spec {
    name: &'static str,
    decoder: &'static str,
    game: &'static str,
    note: &'static str,
}

const DEC: &str = "moby::class";
const NO_POINTER: &str = "searched: no header word, sequence header, list, mesh record or frame offset of the class points into it, and the class's pointer is only dereferenced through those; it may be animation data";

impl K {
    fn spec(self) -> Spec {
        let s = |name, decoder, game, note| Spec { name, decoder, game, note };
        match self {
            K::Header => s(
                "moby class header (0x48 bytes, then one sequence offset each)",
                DEC,
                "func_L01_00424368 and func_L01_00424048 (+0, +4 to +6, +0xB, +0xC, +0x10 to +0x20, +0x28, +0x2D, the sequence offsets), func_L01_00442E08 (+0xE, +0xF, +0x10, +0x24, +0x40), func_L01_00435F98 (+0x44, +0x47), func_L01_0043EA78 (+7, +9), func_L01_004A72A8 (+0xD), func_L01_004367A0 (+0x2C), func_L01_00436988 (+0x2E), func_L01_0032BDB8 (+0x30 to +0x3F), func_L01_00304A18 (+0x46) (all the same in all 34 story levels); +0xA is zero",
                "",
            ),
            K::Lod => s(
                "moby class LOD table (header +0x2C, 16-byte units: a 4-byte header, then 4-byte entries)",
                DEC,
                "func_L01_00424048 (+1 is added to the mesh record count), func_L01_004367A0 (+2), func_L01_0043EA78 (the entries: start and count of two runs of mesh records)",
                "byte +0 (the first extra mesh record number in the data): no reader in func_L01_00424048, func_L01_004367A0, func_L01_004367E8 or func_L01_0043EA78 (xrefs of the LOD table's byte +0 over the 34 overlays)",
            ),
            K::Attach => s(
                "moby class attachment point pairs (16 bytes: two points of x, y, z, joint as s16)",
                DEC,
                "func_L01_004367E8 (same in all 34 story levels)",
                "",
            ),
            K::Hull => s(
                "moby class hull blocks (header +0x2E, 16-byte units: an index row, then blocks of a 16-byte sphere and 8-byte points)",
                DEC,
                "func_L01_00436988 and func_L01_004369B8 (the index bytes +1 to +0xF, the sphere, the points), func_L01_0040BEC0, func_L01_0040C368",
                "index byte +0: no reader in func_L01_00436988 or func_L01_004369B8 (both start at byte +1)",
            ),
            K::Rec => s(
                "moby class mesh records (16 bytes: +0 packet A, +4 its quadword count and the texture setup's, +8 packet B, +0xC four counts)",
                DEC,
                "func_L01_00424048 (+0, +8), func_L01_00423E10 (+6), func_L01_0043EA78 (+4, +0xC) (same in all 34 story levels)",
                "",
            ),
            K::T10 => s(
                "moby class collision data (header +0x10: a 16-byte header, 0x20-byte entries, 8-byte points, 4-byte faces)",
                DEC,
                "the sphere sweeps func_003CE740 (level 1: 0x3F8448; frontbin has a copy), func_L01_003F9498, func_L01_003FA2D8 and func_003CF790 (level 1: 0x3FB1D0), which read the class through each moby instance's +0x98 (func_L01_00442E08 sets it to the class's +0x10): header +4 (lw 0x3F8E24, 0x3F8F60: the entries' bytes), +8 (lw 0x3F8F30: the faces' bytes) and +0xC (lw 0x3F8D40, 0x3F8DE8, 0x3F8F28: the points' bytes); entries from +0x10, 0x20 bytes each, +0 u8 type (lb 0x3F8FA0), +2 s16 flags (lh 0x3F8F9C: bits 0 and 1 select the entry for the two sweep modes, bit 15 ends the list); type 1 sphere: f32 x, y, z at +0x10 and radius at +0x1C (lqc2 0x3F9230); type 3 capsule: the same and the segment's length along the instance's z axis as f32 at +4 (lqc2 0x3F91C8, 0x3F91CC); type 2 sphere on a joint: u32 joint number at +4 (lw 0x3F91F8), f32 radius at +0xC, f32 offset x, y, z at +0x10 (lw 0x3F91F8, lqc2 0x3F91FC and 0x3F9200); type 4 capsule between two joints: u16 joint numbers at +4 and +6 (lh 0x3F8FD0, 0x3F8FD4; each row is loaded from scratchpad 0x70000000 + 16 * the number, lqc2 0x3F8FE8, 0x3F8FEC), f32 radius at +0xC (lqc2 0x3F8FF4); points from header +4 after the entries: x, y, z as s16 and a w the transform overwrites (ld 0x3F8E50..0x3F8E7C); faces after the points: three u8 point numbers and a flag byte (lw 0x3F8F40, 0x3F89A0; the loop at 0x3F8978 tests bit 7 of the flag byte at 0x3F89C4 and bits 0 to 4 at 0x3F89EC..0x3F89F4; bits 5 and 6 are read by nothing); func_L01_004366B0 (+4: the entry count 32 * n; it writes +4, +0x10 and +0x1C of entry k), func_L01_004435E8 (+8 not zero: the hull has faces); the fields' zero bytes (entry +4..+0xF of a sphere, +8..+0xF of the other types, +0x1C of a type 2, +0x10..+0x1F of a type 4, a point's w) are lanes that a quadword load brings in and no instruction uses (the decoder requires them zero) (same in all 34 story levels)",
                "header +0 (a u32: zero, or two u16 numbers) and entry +1 (1 or 4): no reader in the four sweeps, func_L01_004366B0 or func_L01_004435E8 (xrefs: every load through a moby's +0x98 in the 34 overlays is a null test, a save and clear of the pointer (func_00380AB0, func_L01_003C4428), an assignment from the class's +0x10 (func_L01_003C43E0, func_L01_004B95D8, func_L01_00442E08, func_L01_00442F20) or one of these)",
            ),
            K::T14 => s(
                "moby class joint matrices (4 x 4 f32 per joint, header +0x14)",
                DEC,
                "func_L01_00439E00 (DMAs nj * 4 quadwords to scratchpad 0x1C00 at 0x43A7FC, then lqc2 of the four rows of each joint at 0x43AA38 to 0x43AA44 and multiplies them into the joint's animated matrix)",
                "the row w lanes are loaded by lqc2 but no vector instruction uses them",
            ),
            K::T18 => s(
                "moby class joint table (16 bytes per joint: x, y, z as f32 and the parent matrix's scratchpad address, header +0x18)",
                DEC,
                "func_L01_0043AAF8 (DMAs it to the scratchpad; func_L01_0043C028, func_L01_0043CF90 and func_L01_0043D9D8 read it the same way)",
                "",
            ),
            K::T28 => s(
                "moby class effect records (0x20 bytes: header +0x28, count +0xD)",
                DEC,
                "func_L01_004A72A8 and func_L01_004A6F58 (+0x10, +0x14, +0x18, +0x1A) (same in all 34 story levels)",
                "+0 to +0xF, +0x19 and +0x1C to +0x1F: no reader (the effect record readers named read only +0x10, +0x14, +0x18 and +0x1A; xrefs of the header's +0x28 in the 34 overlays)",
            ),
            K::Rt => s(
                "moby class rows overwritten at run time (header +0xF rows of 16 bytes before the joint matrices)",
                DEC,
                "func_L01_00422AB8 copies data over them; header +0xF is read by func_L01_00422AB8, func_L01_00435F98, func_L01_00442E08, func_L01_00442F20 and func_L01_004A0B88, none of which reads the rows' file bytes",
                "the bytes in the file are read by nothing",
            ),
            K::LTab => s(
                "moby class list table (header +0x1C: a count, then list offsets)",
                DEC,
                "func_L01_00424048 (relocates it), func_L01_00436BD8, func_L01_00437350 and func_L01_0043CCD0 (same in all 34 story levels)",
                "",
            ),
            K::List => s(
                "moby class mesh joint list (u16 n, a byte m, a zero byte, n joint numbers, m more bytes, 0xFF)",
                DEC,
                "func_L01_0043CCD0 (n as lhu, the joint numbers as lbu), func_L01_00436BD8 (n as lbu, m as lbu +2, and it stores the pointer to the m bytes at its record's +0xC), func_L01_00437350 (n as lbu, the first of the m bytes as lbu) (same in all 34 story levels)",
                "the m bytes after the first and the 0xFF: no reader (the record func_L01_00436BD8 builds at 0x1F47C0 + 0x40 k, referenced from the moby's +0x50, is read by func_L01_0043CF90 and func_L01_0043D9D8 only at +2, +8, +0x10 and +0x1C, by func_L01_00436C90 at +0x1C; nothing reads its +0xC pointer; xrefs of the table 0x1F47C0..0x1F4BC0 and of every load through a +0x50 field in the 34 overlays)",
            ),
            K::TexId => s(
                "moby class texture-id records (16 bytes: texture numbers ended by 0xFF, 0xFF fill, an offset; header +0x20)",
                DEC,
                "func_L01_00424048 (maps the numbers, relocates +0xC), func_L01_00437C88 (same in all 34 story levels)",
                "",
            ),
            K::PalShorts => s(
                "moby class palette address slots (16 bytes, zero in the file; header +0xB)",
                DEC,
                "func_L01_00424048 writes them (same in all 34 story levels)",
                "",
            ),
            K::Pal => s(
                "moby class texture palettes (256 RGBA colours of 4 bytes each; header +0xB: high nibble textures times low nibble variants)",
                DEC,
                "func_L01_00437C88 (hands each palette's address to the GS upload)",
                "",
            ),
            K::Seq => s(
                "moby class animation sequence header (0x1C bytes, then one frame offset per frame)",
                DEC,
                "func_L01_00424048 (+0x10, +0x14, the frame offsets), func_L01_00436D30, func_L01_004389D8 and func_L01_00435F98 (+0x10), func_L01_00436350, func_L01_00438180 (+0x11, +0x12), func_L01_00436D30 and func_L01_00437500 (+0x13), func_L01_00423DB8, func_L01_00450D80, func_L01_00450FE0 (+0x14), func_L01_004389D8 (+0x18) (same in all 34 story levels); +0 to +0xF skipped: every load through a sequence pointer in these and the other readers is at +0x10 on",
                "",
            ),
            K::Sx => s(
                "moby class sequence record (0x20 bytes: an id, a byte, two zero bytes, seven f32; sequence +0x14)",
                DEC,
                "func_L01_00450D80 (same in all 34 story levels)",
                "",
            ),
            K::Frame => s("moby class animation frame", "", "", "anim.rs's lines (anim::frame, anim::packed_frame)"),
            K::PA => s(
                "moby class mesh packet A (VIF1 stream: UNPACK codes and data, then NOP codes)",
                DEC,
                "func_L01_0043EA78 (builds the DMA tag), VIF1 and VU1 program 0 (the UNPACK data: the module doc's \"VU1 program 0\"), func_L01_00423E10 and func_L01_00423700 (the texture setup: words 0, 1, 4, 5 and 8 of each 64-byte block)",
                "words 9, 12 and 13 of each texture setup block are overwritten by func_L01_00423700 before anything reads them (words 0, 1, 4, 5 and 8 are read by func_L01_00423E10 first; words 2, 3, 6, 7, 10, 11, 14 and 15 are read by VU1 program 0 as parts of the quadwords it copies and of the w lanes it walks)",
            ),
            K::Base => s("packed animation sequence base block", "", "", "decoded by anim::packed_base"),
            K::PB => s(
                "moby class mesh packet B (skinned vertex data: a header, joint matrix pairs, vertex copy slots and 16-byte vertex rows)",
                DEC,
                "func_L01_0043EA78 (reads the record's +8 and +0xC at 0x43F6A8 to 0x43F6D4 and queues (address << 4 | quadword count, destination) in the job list), func_003C5D90 (level 1: 0x43E998, runs each job list), func_003D00F8 (level 1: 0x3F6FF0: DMAs each packet to scratchpad 0x2000 or 0x2800 and skins it; its entry at +0x1058, level 1 0x3F8048, runs the second job group) and the VU0 microprogram its vcallms 0 at 0x3F7328 starts (VU0 program in the main program at 0x10008C and 0x100894) (same in all 34 story levels)",
                "two layouts told apart by their size words. Skinned rows (VU0 microprogram): u16 header +0 pair count, +2, +4, +6 row counts of the three groups (VU0 vi03, vi04, vi05 at 0x3F71D8 to 0x3F71E8), +8 vertex copy count, +0xA vertex count, +0xC rows offset (0x3F71C8), +0xE count of normals filled with a constant (0x3F71D0, 0x3F8018); pairs of bytes (joint matrix number, VU0 data address) from +0x10 (0x3F7240, 0x3F7244); zero padding to 8; u16 copy slots (0x3F7A34 to 0x3F7C80); zero padding to the rows offset; rows of 16 bytes: u16 +0 (bits 0 to 8 slot, 0x3F773C; bits 9 to 15 joint matrix number, 0x3F72B4 and 0x3F72D0), bytes +2 and +3 VU0 data addresses (MTIR vi06 and vi08, VU0 0x48 and 0x40), +4 to +7 weights (ITOF12 vf24, vf04, MTIR vi08 and vi09, VU0 0x200), +8 and +9 lookup indices into the 8-byte table at scratchpad 0x3800 that func_003BE0D0 (level 1: 0x437EB0) copies from 0x2BF5A0 (0x3F7294 to 0x3F72C8), +10 to +15 the vertex x, y, z as s16 (with +8 and +9 as the lane before: qmtc2 vf1 at 0x3F72E0); then the look-ahead rows the four-row loop loads past the last row ((3 - rows) mod 4, at least 1, quadwords: lq at 0x3F7400 to 0x3F741C and 0x3F7698 to 0x3F76B4). Rigid-weight layout (rows of func_003D00F8 at 0x3F8048): u32 +0 row count (0x3F8190), +4, +8, +0xC end offsets of the three output arrays in the scratchpad output buffer (0x3F8194 to 0x3F81B0; V4-8 normals, V3-16 positions, V4-8), rows of 16 bytes: x, y, z as s16 and the two lookup indices (+6, +7) (lhu at 0x3F81D0, ld at 0x3F81D8), +8 to +10 joint matrix numbers (lw at 0x3F81EC), +11 their count (lbu at 0x3F81E4), +12 to +15 weights (lw at 0x3F81F4)",
            ),
        }
    }

    fn short(self) -> &'static str {
        match self {
            K::Header => "header",
            K::Lod => "LOD table",
            K::Attach => "attachment points",
            K::Hull => "hull blocks",
            K::Rec => "mesh records",
            K::T10 => "collision data",
            K::T14 => "joint matrices",
            K::T18 => "joint table",
            K::T28 => "effect records",
            K::Rt => "overwritten rows",
            K::LTab => "list table",
            K::List => "joint list",
            K::TexId => "texture-id records",
            K::PalShorts => "palette address slots",
            K::Pal => "palettes",
            K::Seq => "sequence header",
            K::Sx => "sequence record",
            K::Frame => "frame",
            K::Base => "packed sequence base block",
            K::PA => "packet A",
            K::PB => "packet B",
        }
    }
}

/// A decoded moby class (`class` and `class_in` return it for every class of the core and of the SCENE blocks).
#[derive(Clone, Debug)]
pub struct MobyClass {
    pub header: ClassHeader,
    /// The LOD table (header +0x2C), None when the class has none.
    pub lod: Option<LodTable>,
    /// Every mesh record in file order (header +4 + +5 + +6 + the LOD table's byte +1 of them). A record whose packet A has no
    /// quadwords (class 0's unused record slots, which func_L01_00424008 fills for armor) has `packet_a` None.
    pub meshes: Vec<Mesh>,
    /// The joint table (header +0x18): one entry per joint (header byte +8).
    pub joints: Vec<Joint>,
    /// The joint matrices of the bind pose (header +0x14): one 4 x 4 f32 matrix per joint, as four rows. func_L01_00439E00 DMAs
    /// them to scratchpad 0x1C00 and multiplies each into the joint's animated matrix.
    pub bind: Vec<[[f32; 4]; 4]>,
    /// The texture-id records (header +0x20).
    pub texture_ids: Vec<TextureIdRecord>,
}

/// The fields of the 0x48-byte class header that the draw and the instance code read (see `K::Header` for the readers).
#[derive(Clone, Debug)]
pub struct ClassHeader {
    /// +4: mesh records of the first run (func_L01_0043EA78 at 0x43F17C `lbu v0, 4(s1)`: the meshes drawn near; the run starts at
    /// record 0).
    pub meshes_near: u8,
    /// +5: mesh records of the second run (0x43F190 `lbu v0, 5(s1)`: the meshes drawn far; the run starts at record `meshes_near`,
    /// 0x43F1A0 `lbu at, 4(s1)`).
    pub meshes_far: u8,
    /// +6: mesh records of the third run (func_L01_00424048 adds it to the record count; func_L01_0043EA78 reads it at 0x44003C
    /// for its second draw).
    pub meshes_third: u8,
    /// +7 (read by func_L01_0043EA78 as the header says; its use in the draw loop is not decoded here).
    pub byte_7: u8,
    /// +8: joint count of the near run (0x43F184 `lbu a2, 8(s1)`): the number of joints, which sizes the joint table and matrices.
    pub joints: u8,
    /// +9: joint count used with the far run (0x43F1A8 `lbu a2, 9(s1)`).
    pub joints_far: u8,
    /// +0xB: palettes (high nibble textures, low nibble variants; func_L01_00424048 0x4241AC), zero for none.
    pub palettes: u8,
    /// +0xD: effect record count (func_L01_004A72A8).
    pub effects: u8,
    /// +0xE (func_L01_00442E08).
    pub byte_e: u8,
    /// +0xF: rows overwritten before the joint matrices (func_L01_00422AB8 and others).
    pub rows: u8,
    /// +0x2C: the LOD table's position in 16-byte units (0 for none).
    pub lod_table: u8,
    /// +0x2D (func_L01_00424368 at 0x424418: shifted left by 10 into a table at 0x2006F0).
    pub byte_2d: u8,
    /// +0x2E: hull block index in 16-byte units (s16).
    pub hull_blocks: i16,
    /// +0x30: the bounding sphere (x, y, z, radius) in model units, loaded whole by `lq` (func_L01_0032BDB8 `lq v0, 0x30(v1)`;
    /// the instance's scale (instance +0x2C times 1/1024, 0x3A80 = 0.0009765625) multiplies the model vectors).
    pub sphere: [f32; 4],
    /// +0x40 (u32, func_L01_00442E08).
    pub word_40: u32,
    /// +0x44 (u16, func_L01_00435F98).
    pub half_44: u16,
    /// +0x46 (func_L01_00304A18).
    pub byte_46: u8,
    /// +0x47 (func_L01_00435F98, func_L01_00424048 at 0x424338).
    pub byte_47: u8,
}

/// The LOD table (header +0x2C, 16-byte units; 0x40 bytes): func_L01_0043EA78 at 0x43FBDC to 0x43FC58 picks from it. The draw
/// queue entry names the entries to draw as a 15-bit mask (`s4`, highest set bit first, `plzcw` at 0x43FBC8); for an entry the
/// run at bytes +4, +5 (start record, record count) is drawn, or, when the entry's alternate flag is set (the upper doubleword
/// of `s4` not zero, 0x43FC10), the run at +6, +7 when its start is not zero (0x43FC18 to 0x43FC20). Records are counted from
/// the first mesh record (`lw s1, 0(a0)` at 0x43FBF8).
#[derive(Clone, Debug)]
pub struct LodTable {
    /// Bytes +0 to +3: +1 is the number of extra mesh records (func_L01_00424048 0x424088), +2 is read by func_L01_004367A0,
    /// +0 and +3 are read by nothing.
    pub header: [u8; 4],
    /// The 15 entries at +4 + 4 * i.
    pub entries: Vec<LodEntry>,
}

/// One LOD table entry: two runs of mesh records, each `(first record, record count)`.
#[derive(Clone, Copy, Debug)]
pub struct LodEntry {
    pub run: (u8, u8),
    pub alternate: (u8, u8),
}

/// One joint of the joint table (header +0x18, 16 bytes; func_L01_0043AAF8 DMAs it to the scratchpad).
#[derive(Clone, Copy, Debug)]
pub struct Joint {
    /// The joint's offset from its parent (x, y, z as f32).
    pub offset: [f32; 3],
    /// The parent joint's number, None for the root. The file holds the parent's scratchpad matrix address, 0x70000000 + 0x40 *
    /// parent, or 0.
    pub parent: Option<usize>,
}

/// A texture-id record (header +0x20, 16 bytes): class-local texture numbers ended by 0xFF (the other bytes up to +0xC are
/// 0xFF), and the offset at +0xC that func_L01_00424048 relocates (the last record's is negative). func_L01_00424048 maps the
/// numbers through the class record's 16 texture index bytes (0x424168 to 0x424198).
#[derive(Clone, Debug)]
pub struct TextureIdRecord {
    pub numbers: Vec<u8>,
    pub offset: i32,
}

/// One mesh record (16 bytes, func_L01_00424048 relocates +0 and +8; header `K::Rec`).
#[derive(Clone, Debug)]
pub struct Mesh {
    /// The record's number in the array (what the LOD runs count).
    pub record: usize,
    /// Record +0xC byte 0: quadword count of packet B.
    pub b_qwords: u8,
    /// Record +0xC byte 1: quadword count of the buffer func_L01_0043EA78 reserves for the skinned positions (0x43F6C8).
    pub position_qwords: u8,
    /// Record +0xC byte 2: quadword count of the buffer for the second V4-8 output (the colours/normals).
    pub normal_qwords: u8,
    /// Record +0xC byte 3: the vertex count, the number of entries of the skinned vertex output (VU1 program 0's position
    /// buffer at TOPS + 0, 3 x s16 each, the colour buffer at TOPS + 0x61 and the UV buffer at TOPS + 0xC2; at most 97).
    pub vertices: u8,
    /// The offset of packet A in the class's bytes (record +0 after func_L01_00424048 relocates it); the packet is at the class's
    /// address plus this in RAM.
    pub a_at: usize,
    /// The offset of packet B in the class's bytes (record +8); func_L01_0043EA78 queues the packet's RAM address (the class's
    /// address plus this) in the draw job (see `skin`).
    pub b_at: usize,
    pub packet_a: Option<PacketA>,
    pub packet_b: Option<PacketB>,
}

/// Packet A decoded as VU1 program 0 reads it (see the module doc: "VU1 program 0").
#[derive(Clone, Debug)]
pub struct PacketA {
    /// The V2-16 UNPACK (code 0x75, VU1 address TOPS + 0xC2): per vertex index the texture coordinates u, v as f32, the file's
    /// s16 divided by 4096 (ITOF12 at micro 0x13D8 and others): 1.0 is the texture size. None when the packet has no such UNPACK
    /// (the 2561 mesh records of the disc whose packet B has the rigid layout): the UV buffer is then written by the skin job, not
    /// stored in the class (the rigid layout's third output array has 4 bytes per vertex, the size of a V2-16 vertex, and the trace
    /// holds the UVs of such a mesh in an EE-built buffer that no class data contains).
    pub uvs: Option<Vec<[f32; 2]>>,
    /// The strip list's header quadword byte +0 (V4-8 UNPACK at TOPS + 0x12D, `ILW.x vi10, -2(vi08)` at micro 0x1440): -2 draws
    /// every triangle; 0 or 2 culls the triangles whose screen-space area sign flag equals it (see the module doc).
    pub cull: i8,
    /// Header byte +3 (`ILW.w vi02, 10(vi01)` at micro 0xB8, the pre-pass entry 0xA0 only): zero in every packet of the disc.
    pub prepass_count: i8,
    /// The strips in drawing order.
    pub strips: Vec<Strip>,
    /// The texture setups (UNPACK 0x6C), one per 64 bytes, in the order the strips with a texture consume them.
    pub textures: Vec<TextureSetup>,
}

/// A run of vertices as the GS draws it: a triangle strip with restarts. For vertex k >= 2 the GS draws the triangle (k-2, k-1, k)
/// unless `skip` is set on k (ADC); the first vertex of a strip always has `skip` set. The VU1 program splits long strips into
/// GIF packets of at most 42 vertices and repeats the last two vertices (with their `skip` bits) at the start of the next one,
/// which draws the same triangles.
#[derive(Clone, Debug)]
pub struct Strip {
    /// The texture setup (index into `PacketA::textures`) the game binds before this strip (its A+D registers precede it in the
    /// GIF stream); None: the strip is drawn with the texture bound before it (the previous strip's, or the previous mesh's).
    pub texture: Option<usize>,
    pub vertices: Vec<StripVertex>,
}

/// A vertex of a strip.
#[derive(Clone, Copy, Debug)]
pub struct StripVertex {
    /// The vertex's index into the mesh's skinned vertex output, from 0 (the program's 1-based list value minus 1: the position at
    /// VU1 TOPS + index, the colour at TOPS + 0x61 + index, the UV at TOPS + 0xC2 + index).
    pub index: u8,
    /// The ADC bit of the vertex's XYZF2 write: no triangle is drawn on it.
    pub skip: bool,
}

/// One 64-byte texture setup of a packet A (UNPACK 0x6C at TOPS + 0x12D + the strip list's quadword count; four quadwords).
/// func_L01_00423E10 reads words 0, 1, 4, 5 and 8 of each (0x423E9C, 0x423EA4, 0x423EAC, 0x423EB4 and 0x423E80) and func_L01_00423700
/// rewrites words 0, 1, 4, 5, 8, 9, 12 and 13 with the GS registers TEX1_1, CLAMP_1, TEX0_1 and MIPTBP1_1 of the texture record.
#[derive(Clone, Copy, Debug)]
pub struct TextureSetup {
    /// Word 8 (s32): the class-local texture number t. func_L01_00423E10 (0x423E80 `lw t1, 0x20(s0)`, 0x423E8C `lbu t1, 0(s7 + t1)`)
    /// maps it through the class record's 16 texture index bytes to the record number r of the core index's moby texture table
    /// (table 1) and reads the record at table + 16 * r (0x423E94 `sll a1, t1, 4`, 0x423EA0 `addu a1, s3, a1`; func_L01_00424368's
    /// callers pass the table as the index base plus its word 15, 0x428C30 to 0x428C3C, and the 16 bytes at the class record +0x10).
    /// A negative number is not mapped (0x423E84 `bltz`): -1 to -3 select the register sets of func_L01_00423700's special cases.
    pub local: i32,
    /// Word 0: TEX1_1 K (shifted to bit 32 by func_L01_00423700 at 0x42376C and later).
    pub k: u32,
    /// Word 1: TEX1_1 MMIN (shifted left by 6).
    pub mmin: u32,
    /// Word 4: CLAMP_1 WMS.
    pub wms: u32,
    /// Word 5: CLAMP_1 WMT (shifted left by 2).
    pub wmt: u32,
    /// The low 16 bits of word 2 of the first quadword as an s16: variant A of program 0 reads it (`ILW.z vi15, 0(vi11)` at micro
    /// 0x7A0) as the number of quadwords of the A+D packet to copy (4), and a negative value would toggle its cull mode (micro 0x7E0
    /// to 0x7F0); variant B always copies four quadwords. The word's float (20.0, the register number 0x14 after FTOI0) is
    /// not data.
    pub a_count: i16,
}

/// Packet B as data: the skinning input of func_003D00F8 (level 1: 0x3F6FF0), in either of its two layouts (see `K::PB`).
#[derive(Clone, Debug, PartialEq)]
pub enum PacketB {
    /// Rows skinned through VU0 (the layout with the header of u16s).
    Skinned(SkinnedB),
    /// Rows with up to three joint numbers and weights in the row (the layout whose header is four u32s).
    Rigid(RigidB),
}

/// The VU0-skinned layout. Header u16s: +0 pair count n0, +2, +4, +6 the three row groups' counts, +8 vertex copy count, +0xA
/// vertex count, +0xC the offset of the rows, +0xE the count of normals filled with a constant.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinnedB {
    /// Header +2, +4, +6: the row counts of the three groups (VU0 vi03, vi04 and vi05 at 0x3F71D8 to 0x3F71E8).
    pub groups: [u16; 3],
    /// Header +0xE: the count of normals filled with a constant (0x3F71D0, 0x3F8018).
    pub constant_normals: u16,
    /// The (joint matrix number, VU0 data address) bytes from +0x10 (0x3F7240, 0x3F7244).
    pub pairs: Vec<(u8, u8)>,
    /// The vertex copy slots (u16 each, 0x3F7A34 to 0x3F7C80): after the pairs, padded to 8 bytes.
    pub copies: Vec<u16>,
    /// The rows (16 bytes each, group order).
    pub rows: Vec<SkinRow>,
    /// The look-ahead rows after the last row (`(3 - rows) mod 4` of them, 4 when that is 0), each as its eight u16s. func_003D00F8's
    /// four-row loop reads them as rows; the encoder puts the output slots of the last rows in them (see `skin`).
    pub tail: Vec<[u16; 8]>,
}

/// One row of the VU0-skinned layout (16 bytes; readers in `K::PB`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkinRow {
    /// +0 bits 0 to 8: the output slot (0x3F773C).
    pub slot: u16,
    /// +0 bits 9 to 15: the joint matrix number (0x3F72B4, 0x3F72D0).
    pub joint: u8,
    /// +2, +3: VU0 data addresses (MTIR vi06 and vi08, VU0 0x48 and 0x40).
    pub vu0: [u8; 2],
    /// +4 to +7: the weights (ITOF12 vf24 and vf04, MTIR vi08 and vi09, VU0 0x200).
    pub weights: [u8; 4],
    /// +8, +9: lookup indices into the 8-byte table at scratchpad 0x3800 that func_003BE0D0 (level 1: 0x437EB0) copies from
    /// 0x2BF5A0 (0x3F7294 to 0x3F72C8): 256 entries of (cos, sin) as f32 of 2 pi k / 256, so the vertex's normal is two angles.
    pub lookup: [u8; 2],
    /// +10 to +15: the vertex x, y, z as s16 in the bind pose (qmtc2 vf1 at 0x3F72E0).
    pub pos: [i16; 3],
}

/// The rigid-weight layout: u32 header (+0 row count, +4, +8, +0xC the end offsets of the three output arrays in the scratchpad
/// output buffer, 0x3F8190 to 0x3F81B0) and 16-byte rows.
#[derive(Clone, Debug, PartialEq)]
pub struct RigidB {
    pub ends: [u32; 3],
    pub rows: Vec<RigidRow>,
}

/// One row of the rigid-weight layout (func_003D00F8 at 0x3F8048).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigidRow {
    /// +0 to +5: x, y, z as s16 in the bind pose (lhu at 0x3F81D0).
    pub pos: [i16; 3],
    /// +6, +7: angle indices into the (cos, sin) table of `SkinRow::lookup` (ld at 0x3F81D8): the vertex's normal.
    pub lookup: [u8; 2],
    /// +8 to +10: joint matrix numbers (lw at 0x3F81EC); `joint_count` of them are used.
    pub joints: [u8; 3],
    /// +11: the number of joint numbers used (lbu at 0x3F81E4).
    pub joint_count: u8,
    /// +12 to +15: the weights (lw at 0x3F81F4).
    pub weights: [u8; 4],
}

enum Part {
    None,
    Header(ClassHeader),
    Lod(LodTable),
    Rec(Vec<[u32; 4]>),
    PA(PacketA),
    PB(PacketB),
    Joints(Vec<Joint>),
    Bind(Vec<[[f32; 4]; 4]>),
    TexIds(Vec<TextureIdRecord>),
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
struct Item {
    a: usize,
    size: Option<usize>,
    kind: K,
    x: usize,
    y: usize,
}

struct Piece {
    a: usize,
    size: usize,
    nx: usize,
    kind: K,
    x: usize,
    y: usize,
}

fn rb(b: &[u8], o: usize) -> Result<usize, String> {
    b.get(o).map(|&v| v as usize).ok_or_else(|| format!("byte +{o:#x} past the class's {:#x} bytes", b.len()))
}

fn rw(b: &[u8], o: usize) -> Result<usize, String> {
    u32_at(b, o).map(|v| v as usize)
}

fn rh(b: &[u8], o: usize) -> Result<usize, String> {
    u16_at(b, o).map(|v| v as usize)
}

fn rs(b: &[u8], o: usize) -> Result<i32, String> {
    u16_at(b, o).map(|v| v as i16 as i32)
}

fn m(n: &mut Node, at: usize, len: usize) {
    n.mark(at as u64, len as u64);
}

fn item(a: usize, size: Option<usize>, kind: K, x: usize, y: usize) -> Item {
    Item { a, size, kind, x, y }
}

/// Every structure of the class, with its exact extent, found from the header and the structures' own offsets.
fn plan(b: &[u8]) -> Result<Vec<Piece>, String> {
    let len = b.len();
    if len < 0x48 {
        return Err(format!("class of {len:#x} bytes is shorter than its header"));
    }
    let nseq = rb(b, 0xc)?;
    let k = rb(b, 0x2c)?;
    let e = rs(b, 0x2e)?;
    let hdr_end = 0x48 + 4 * nseq;
    if hdr_end > len {
        return Err(format!("header of {hdr_end:#x} bytes in {len:#x}"));
    }
    if e < 0 {
        return Err(format!("hull block index {e}"));
    }
    let ptr = |o: usize, what: &str| -> Result<usize, String> {
        if o >= len { Err(format!("{what} at {o:#x} past the class's {len:#x} bytes")) } else { Ok(o) }
    };
    let mut it: Vec<Item> = vec![item(0, Some(hdr_end), K::Header, nseq, 0)];
    let ct = rb(b, 4)? + rb(b, 5)? + rb(b, 6)? + if k != 0 { rb(b, k * 0x10 + 1)? } else { 0 };
    let nj = rb(b, 8)?;
    let w0 = rw(b, 0)?;
    if w0 != 0 {
        it.push(item(ptr(w0, "mesh records")?, Some(16 * ct), K::Rec, ct, 0));
    }
    let w10 = rw(b, 0x10)?;
    if w10 != 0 {
        it.push(item(ptr(w10, "collision data")?, None, K::T10, nj, 0));
    }
    let w14 = rw(b, 0x14)?;
    if w14 != 0 {
        it.push(item(ptr(w14, "joint matrices")?, Some(64 * nj), K::T14, nj, 0));
    }
    let rows = rb(b, 0xf)?;
    if rows != 0 {
        if w14 < 16 * rows {
            return Err(format!("{rows} rows to overwrite before joint matrices at {w14:#x}"));
        }
        it.push(item(w14 - 16 * rows, Some(16 * rows), K::Rt, rows, 0));
    }
    let w18 = rw(b, 0x18)?;
    if w18 != 0 {
        it.push(item(ptr(w18, "joint table")?, Some(16 * nj), K::T18, nj, 0));
    }
    let w28 = rw(b, 0x28)?;
    if w28 != 0 {
        let c = rb(b, 0xd)?;
        it.push(item(ptr(w28, "effect records")?, Some(0x20 * c), K::T28, c, 0));
    }
    let w1c = rw(b, 0x1c)?;
    if w1c != 0 {
        ptr(w1c, "list table")?;
        let nn = rw(b, w1c)?;
        if w1c + 4 + 4 * nn > len {
            return Err(format!("list table of {nn} offsets at {w1c:#x} past the class's {len:#x} bytes"));
        }
        it.push(item(w1c, Some(4 + 4 * nn), K::LTab, nn, 0));
        for i in 0..nn {
            let o = ptr(rw(b, w1c + 4 + 4 * i)?, "list")?;
            let n = rh(b, o)?;
            let mm = rb(b, o + 2)?;
            if o + 4 + n + mm + 1 > len {
                return Err(format!("list at {o:#x} of {n} + {mm} bytes past the class's {len:#x} bytes"));
            }
            it.push(item(o, Some(4 + n + mm + 1), K::List, n, mm));
        }
    }
    let tex = rw(b, 0x20)?;
    if tex != 0 {
        ptr(tex, "texture ids")?;
        let mut q = tex;
        let mut n = 1;
        loop {
            if q + 16 > len {
                return Err(format!("texture ids at {tex:#x} not ended before the class's {len:#x} bytes"));
            }
            if (rw(b, q + 12)? as u32 as i32) < 0 {
                break;
            }
            q += 16;
            n += 1;
        }
        it.push(item(tex, Some(16 * n), K::TexId, n, 0));
        let pb = rb(b, 0xb)?;
        if pb != 0 {
            let (hi, lo) = (pb >> 4, pb & 15);
            let pal = hi * lo * 0x400;
            if tex < pal + 0x10 {
                return Err(format!("{hi} x {lo} palettes before the class start (texture ids at {tex:#x})"));
            }
            it.push(item(tex - pal, Some(pal), K::Pal, hi, lo));
            it.push(item(tex - pal - 0x10, Some(0x10), K::PalShorts, hi, 0));
        }
    }
    for i in 0..nseq {
        let s = rw(b, 0x48 + 4 * i)?;
        if s == 0 {
            continue;
        }
        ptr(s, "sequence")?;
        if s + 0x1c > len {
            return Err(format!("sequence header at {s:#x} past the class's {len:#x} bytes"));
        }
        let nf = rb(b, s + 0x10)?;
        if s + 0x1c + 4 * nf > len {
            return Err(format!("sequence at {s:#x}: {nf} frame offsets past the class's {len:#x} bytes"));
        }
        it.push(item(s, Some(0x1c + 4 * nf), K::Seq, nf, 0));
        let x = rw(b, s + 0x14)?;
        if x != 0 {
            it.push(item(ptr(s + x, "sequence record")?, Some(0x20), K::Sx, 0, 0));
        }
        let base = packed_base(b, &mut it, s, nf, rb(b, 8)?)?;
        for kk in 0..nf {
            let mut fp = rw(b, s + 0x1c + 4 * kk)?;
            if fp >= len {
                if fp >> 24 != 0xf0 || (fp & 0xff_ffff) >= len {
                    return Err(format!("sequence at {s:#x}: frame offset {fp:#x}"));
                }
                fp &= 0xff_ffff;
            }
            it.push(frame_item(b, fp, base)?);
        }
    }
    if w0 != 0 {
        if w0 + 16 * ct > len {
            return Err(format!("{ct} mesh records at {w0:#x} past the class's {len:#x} bytes"));
        }
        for r in 0..ct {
            let (a0, w1, a2, w3) = (rw(b, w0 + 16 * r)?, rw(b, w0 + 16 * r + 4)?, rw(b, w0 + 16 * r + 8)?, rw(b, w0 + 16 * r + 12)?);
            it.push(item(ptr(a0, "packet A")?, Some(16 * (w1 & 0xff)), K::PA, w1, w3 >> 24));
            it.push(item(ptr(a2, "packet B")?, Some(16 * (w3 & 0xff)), K::PB, w3, 0));
        }
    }
    if k != 0 {
        let kb = k * 16;
        if kb + 0x40 > len {
            return Err(format!("LOD table at {kb:#x} past the class's {len:#x} bytes"));
        }
        it.push(item(kb, Some(0x40), K::Lod, 0, 0));
        it.push(item(kb + 0x40, None, K::Attach, 0, 0));
    }
    if e != 0 {
        let eb = e as usize * 16;
        if eb + 16 > len {
            return Err(format!("hull blocks at {eb:#x} past the class's {len:#x} bytes"));
        }
        it.push(item(eb, None, K::Hull, 0, 0));
    }

    tile(it, len)
}

/// The animation sequence of one slot of the core index's word 0x1E table, of `b.len()` bytes: a sequence header at 0, its
/// sequence record at +0x14's offset (when not zero) and its frames, every offset relative to the sequence (the frame offsets
/// carry 0xF0 in their top byte).
fn plan_slot(b: &[u8], joints: usize) -> Result<Vec<Piece>, String> {
    let len = b.len();
    if len < 0x1c {
        return Err(format!("slot sequence of {len:#x} bytes is shorter than its header"));
    }
    let nf = rb(b, 0x10)?;
    if 0x1c + 4 * nf > len {
        return Err(format!("{nf} frame offsets past the slot's {len:#x} bytes"));
    }
    let mut it = vec![item(0, Some(0x1c + 4 * nf), K::Seq, nf, 0)];
    let x = rw(b, 0x14)?;
    if x != 0 {
        if x + 0x20 > len {
            return Err(format!("sequence record at {x:#x} past the slot's {len:#x} bytes"));
        }
        it.push(item(x, Some(0x20), K::Sx, 0, 0));
    }
    let base = packed_base(b, &mut it, 0, nf, joints)?;
    for kk in 0..nf {
        let fp = rw(b, 0x1c + 4 * kk)?;
        let f = if fp >> 24 == 0xf0 { fp & 0xff_ffff } else { fp };
        if f < 0x1c + 4 * nf || f >= len {
            return Err(format!("frame offset {fp:#x}"));
        }
        it.push(frame_item(b, f, base)?);
    }
    tile(it, len)
}

/// A packed sequence (its +0x13 is not 0xFF) at `s` with `nf` frames: its +0x13 records and base block follow the frame offset
/// table (anim.rs); returns the base block's offset, None for a sequence in the unpacked form. `joints` is the class's byte +8.
fn packed_base(b: &[u8], it: &mut Vec<Item>, s: usize, nf: usize, joints: usize) -> Result<Option<usize>, String> {
    let recs = rb(b, s + 0x13)?;
    if recs == 0xff {
        return Ok(None);
    }
    let at = s + 0x1c + 4 * nf;
    let base = at + 4 * recs;
    if base > b.len() {
        return Err(format!("sequence at {s:#x}: base block at {base:#x} past the {:#x} bytes", b.len()));
    }
    let size = 4 * recs + crate::anim::base_len(&b[base..], joints).map_err(|e| format!("sequence at {s:#x}: {e}"))?;
    it.push(item(at, Some(size), K::Base, recs, joints));
    Ok(Some(base))
}

/// A frame at `f`: in the unpacked form (its sequence's +0x13 is 0xFF; anim.rs) it is 0x10 + 16 * its +6 quadword count bytes;
/// a packed frame (`base` its sequence's base block) runs to the next structure, its zero padding included (anim::packed_frame).
fn frame_item(b: &[u8], f: usize, base: Option<usize>) -> Result<Item, String> {
    if let Some(base) = base {
        return Ok(item(f, None, K::Frame, 0, base));
    }
    let size = 0x10 + 16 * rh(b, f + 6)?;
    if f + size > b.len() {
        return Err(format!("frame at {f:#x} of {size:#x} bytes past the {:#x} bytes", b.len()));
    }
    Ok(item(f, Some(size), K::Frame, 1, 0))
}

/// Tiles the structures: each runs to the next one's start when its size is not settled; a settled size may not overrun it.
fn tile(it: Vec<Item>, len: usize) -> Result<Vec<Piece>, String> {
    let mut seen: HashSet<Item> = HashSet::new();
    let mut items: Vec<Item> = Vec::new();
    for i in it {
        if seen.insert(i) {
            items.push(i);
        }
    }
    items.sort_by_key(|i| (i.a, if matches!(i.size, Some(s) if s != 0) { 0 } else { 1 }));
    let mut starts: Vec<usize> = items.iter().filter(|i| i.size != Some(0)).map(|i| i.a).collect();
    starts.sort_unstable();
    starts.dedup();
    let mut out: Vec<Piece> = Vec::new();
    for i in &items {
        let nx = starts.iter().copied().find(|&s| s > i.a).unwrap_or(len);
        let size = match i.size {
            Some(s) => s,
            None => nx - i.a,
        };
        if size == 0 {
            continue;
        }
        if i.a + size > nx {
            return Err(format!("{} at {:#x} of {size:#x} bytes runs over the next structure at {nx:#x}", i.kind.short(), i.a));
        }
        out.push(Piece { a: i.a, size, nx, kind: i.kind, x: i.x, y: i.y });
    }
    for w in out.windows(2) {
        if w[0].a == w[1].a {
            return Err(format!("{} and {} both start at {:#x}", w[0].kind.short(), w[1].kind.short(), w[0].a));
        }
    }
    Ok(out)
}

/// Decodes one class of `b.len()` bytes at `at` in `parent` (in the unpacked space) into lines of `group`. Every byte of the class
/// is on a line. A class the plan cannot settle gets one failed line over all its bytes and the error is returned too.
pub fn class(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8]) -> Result<MobyClass, String> {
    class_in(t, parent, group, at, b, Space::Unpacked)
}

/// The same for a class that sits in `parent`'s own space `space`: the front end's model blocks (WAD4 pairs 347 to 349, on the
/// disc, loaded by frontbin func_00393DC8 and relocated by frontbin func_00394C58, the front end's copy of func_L01_00424048)
/// are `Space::Disc` classes; every line of such a class, the animation frames and base blocks included, is in that space.
pub fn class_in(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], space: Space) -> Result<MobyClass, String> {
    class_noted(t, parent, group, at, b, space, "")
}

/// `class_in`, with `note` added to every line the class creates (when not empty).
fn class_noted(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], space: Space, note: &str) -> Result<MobyClass, String> {
    let len = b.len();
    if parent.overlaps(at, len as u64) {
        return Err(format!("{}: moby class at {at:#x}+{len:#x} overlaps another structure", parent.place));
    }
    let pieces = match plan(b) {
        Ok(p) => p,
        Err(e) => {
            let l = t.line(group, "moby class (not split)", "", "", space);
            let mut n = t.child(parent, l, at, len as u64);
            n.fail(e.clone());
            t.close(n);
            return Err(format!("{} moby class at {at:#x}: {e}", parent.place));
        }
    };
    let joints = rb(b, 8)?;
    let parts = place(t, parent, group, at, b, &pieces, joints, space, note).map_err(|e| format!("{}: moby class at {at:#x}: {e}", parent.place))?;
    assemble(parts, b).map_err(|e| format!("{}: moby class at {at:#x}: {e}", parent.place))
}

/// The mesh records joined to the packets they name by offset (shared by `assemble` and `armor`).
fn build_meshes(recs: &[[u32; 4]], pa: &std::collections::HashMap<usize, PacketA>, pb: &std::collections::HashMap<usize, PacketB>) -> Result<Vec<Mesh>, String> {
    let mut meshes = Vec::with_capacity(recs.len());
    for (r, rec) in recs.iter().enumerate() {
        let (a, w1, bo, w3) = (rec[0] as usize, rec[1], rec[2] as usize, rec[3]);
        let packet_a = if w1 & 0xff == 0 { None } else { Some(pa.get(&a).ok_or_else(|| format!("mesh record {r}: no packet A at {a:#x}"))?.clone()) };
        let packet_b = if w3 & 0xff == 0 { None } else { Some(pb.get(&bo).ok_or_else(|| format!("mesh record {r}: no packet B at {bo:#x}"))?.clone()) };
        meshes.push(Mesh { record: r, b_qwords: w3 as u8, position_qwords: (w3 >> 8) as u8, normal_qwords: (w3 >> 16) as u8, vertices: (w3 >> 24) as u8, a_at: a, b_at: bo, packet_a, packet_b });
    }
    Ok(meshes)
}

/// Puts the decoded structures together: the mesh records name their packets by offset.
fn assemble(parts: Vec<(usize, Part)>, b: &[u8]) -> Result<MobyClass, String> {
    let mut header = None;
    let mut lod = None;
    let mut recs: Vec<[u32; 4]> = Vec::new();
    let mut joints = Vec::new();
    let mut bind = Vec::new();
    let mut texture_ids = Vec::new();
    let mut pa: std::collections::HashMap<usize, PacketA> = std::collections::HashMap::new();
    let mut pb: std::collections::HashMap<usize, PacketB> = std::collections::HashMap::new();
    for (at, part) in parts {
        match part {
            Part::None => {}
            Part::Header(h) => header = Some(h),
            Part::Lod(l) => lod = Some(l),
            Part::Rec(r) => recs = r,
            Part::PA(p) => {
                pa.insert(at, p);
            }
            Part::PB(p) => {
                pb.insert(at, p);
            }
            Part::Joints(j) => joints = j,
            Part::Bind(m) => bind = m,
            Part::TexIds(t) => texture_ids = t,
        }
    }
    let header = header.ok_or("no header part")?;
    let meshes = build_meshes(&recs, &pa, &pb)?;
    let _ = b;
    Ok(MobyClass { header, lod, meshes, joints, bind, texture_ids })
}

/// Decodes the animation sequence of one slot of the core index's word 0x1E table (moby class 0's sequences, which func_L01_00428750
/// and func_L01_00423DB8 reach through the table instead of the class header), `b.len()` bytes at `at` in `parent`, into lines of
/// `group`: the same sequence header, sequence record and frame lines as a class's own sequences.
/// `joints` is moby class 0's byte +8 (the class whose sequences the slots hold).
pub fn slot_sequence(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], joints: usize) -> Result<(), String> {
    let len = b.len();
    if parent.overlaps(at, len as u64) {
        return Err(format!("{}: slot sequence at {at:#x}+{len:#x} overlaps another structure", parent.place));
    }
    let pieces = plan_slot(b, joints).map_err(|e| format!("{} slot sequence at {at:#x}: {e}", parent.place))?;
    place(t, parent, group, at, b, &pieces, joints, Space::Unpacked, "").map(|_| ()).map_err(|e| format!("{}: slot sequence at {at:#x}: {e}", parent.place))
}

/// Puts the planned structures on their lines and decodes them.
fn place(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8], pieces: &[Piece], joints: usize, space: Space, note: &str) -> Result<Vec<(usize, Part)>, String> {
    let len = b.len();
    let mut covered = 0usize;
    let mut parts: Vec<(usize, Part)> = Vec::new();
    let mut failed: Option<String> = None;
    for p in pieces {
        // A structure's zero padding up to the next one belongs to it; other bytes there are a trailer line of their own.
        let pad = p.nx - (p.a + p.size);
        let (end, trailer) = if pad == 0 {
            (p.a + p.size, None)
        } else if b[p.a + p.size..p.nx].iter().all(|&v| v == 0) {
            (p.nx, None)
        } else {
            (p.a + p.size, Some((p.a + p.size, p.nx)))
        };
        if p.kind == K::Base || (p.kind == K::Frame && p.x == 0) {
            // A packed sequence's base block, or a packed frame with its zero padding: anim.rs's lines (a bad value fails its
            // own line).
            let _ = if p.kind == K::Base {
                crate::anim::packed_base(t, parent, group, at + p.a as u64, &b[p.a..p.a + p.size], p.x, p.y, space)
            } else {
                crate::anim::packed_frame(t, parent, group, at + p.a as u64, &b[p.a..end], &b[p.y..], joints, space)
            };
            covered += end - p.a;
            if let Some((x0, x1)) = trailer {
                let name = format!("moby class data after {} (no pointer reaches it)", p.kind.short());
                let l = t.ignored_line(group, &name, NO_POINTER, space);
                let n = t.child(parent, l, at + x0 as u64, (x1 - x0) as u64);
                t.close(n);
                covered += x1 - x0;
            }
            continue;
        }
        if p.kind == K::Frame && p.x == 1 {
            // An unpacked frame: anim.rs's lines (a bad value fails its own line); its zero padding is a line of its own.
            let _ = crate::anim::frame(t, parent, group, at + p.a as u64, &b[p.a..p.a + p.size], space);
            if end > p.a + p.size {
                let l = t.line(group, "moby class zero padding after an animation frame", "moby::class", NOTHING, space);
                let mut n = t.child(parent, l, at + (p.a + p.size) as u64, (end - p.a - p.size) as u64);
                n.mark(0, (end - p.a - p.size) as u64);
                t.close(n);
            }
            covered += end - p.a;
            if let Some((x0, x1)) = trailer {
                let name = format!("moby class data after {} (no pointer reaches it)", p.kind.short());
                let l = t.ignored_line(group, &name, NO_POINTER, space);
                let n = t.child(parent, l, at + x0 as u64, (x1 - x0) as u64);
                t.close(n);
                covered += x1 - x0;
            }
            continue;
        }
        let sp = p.kind.spec();
        let l = t.line(group, sp.name, sp.decoder, sp.game, space);
        if !sp.note.is_empty() {
            t.note(l, sp.note);
        }
        if !note.is_empty() {
            t.note(l, note);
        }
        let mut n = t.child(parent, l, at + p.a as u64, (end - p.a) as u64);
        if !sp.decoder.is_empty() {
            let d = &b[p.a..end];
            match decode(p.kind, d, p.size, p.x, p.y, &mut n) {
                Ok(part) => {
                    if end > p.a + p.size {
                        m(&mut n, p.size, end - p.a - p.size);
                    }
                    parts.push((p.a, part));
                }
                Err(e) => {
                    failed.get_or_insert_with(|| format!("{} at {:#x}: {e}", p.kind.short(), p.a));
                    n.fail(e)
                }
            }
        }
        t.close(n);
        covered += end - p.a;
        if let Some((x0, x1)) = trailer {
            let name = format!("moby class data after {} (no pointer reaches it)", p.kind.short());
            let l = t.ignored_line(group, &name, NO_POINTER, space);
            let n = t.child(parent, l, at + x0 as u64, (x1 - x0) as u64);
            t.close(n);
            covered += x1 - x0;
        }
    }
    if covered != len {
        // The plan always tiles the bytes; this would be a bug in it.
        return Err(format!("{covered:#x} of {len:#x} bytes on a line"));
    }
    match failed {
        Some(e) => Err(e),
        None => Ok(parts),
    }
}

fn rf(b: &[u8], o: usize) -> Result<f32, String> {
    u32_at(b, o).map(f32::from_bits)
}

fn decode(kind: K, d: &[u8], size: usize, x: usize, y: usize, n: &mut Node) -> Result<Part, String> {
    match kind {
        K::Header => {
            for o in [0usize, 0x10, 0x14, 0x18, 0x1c, 0x20, 0x28, 0x40, 0x24] {
                m(n, o, 4);
            }
            m(n, 0x30, 16);
            m(n, 0x2e, 2);
            m(n, 0x44, 2);
            for o in [4usize, 5, 6, 7, 8, 9, 0xb, 0xc, 0xd, 0xe, 0xf, 0x2c, 0x2d, 0x46, 0x47] {
                m(n, o, 1);
            }
            if d[0xa] != 0 {
                return Err(format!("+0xA byte {:#04x}, expected zero", d[0xa]));
            }
            m(n, 0xa, 1);
            m(n, 0x48, 4 * x);
            Ok(Part::Header(ClassHeader {
                meshes_near: d[4],
                meshes_far: d[5],
                meshes_third: d[6],
                byte_7: d[7],
                joints: d[8],
                joints_far: d[9],
                palettes: d[0xb],
                effects: d[0xd],
                byte_e: d[0xe],
                rows: d[0xf],
                lod_table: d[0x2c],
                byte_2d: d[0x2d],
                hull_blocks: rs(d, 0x2e)? as i16,
                sphere: [rf(d, 0x30)?, rf(d, 0x34)?, rf(d, 0x38)?, rf(d, 0x3c)?],
                word_40: u32_at(d, 0x40)?,
                half_44: u16_at(d, 0x44)?,
                byte_46: d[0x46],
                byte_47: d[0x47],
            }))
        }
        K::Lod => {
            n.skip(0, 1);
            m(n, 1, 3);
            m(n, 4, 60);
            let entries = (0..15).map(|i| LodEntry { run: (d[4 + 4 * i], d[5 + 4 * i]), alternate: (d[6 + 4 * i], d[7 + 4 * i]) }).collect();
            Ok(Part::Lod(LodTable { header: [d[0], d[1], d[2], d[3]], entries }))
        }
        K::Attach => {
            if size % 16 != 0 {
                return Err(format!("{size:#x} bytes are not whole point pairs"));
            }
            m(n, 0, size);
            Ok(Part::None)
        }
        K::Hull => hull(d, n).map(|_| Part::None),
        K::Rec => {
            let mut recs = Vec::with_capacity(x);
            for r in 0..x {
                let o = 16 * r;
                if d[o + 5] != 0 {
                    return Err(format!("mesh record {r}: +5 byte {:#04x}, expected zero", d[o + 5]));
                }
                m(n, o, 4);
                m(n, o + 4, 4);
                m(n, o + 8, 8);
                recs.push([rw(d, o)? as u32, rw(d, o + 4)? as u32, rw(d, o + 8)? as u32, rw(d, o + 12)? as u32]);
            }
            Ok(Part::Rec(recs))
        }
        K::T18 => {
            let mut joints = Vec::with_capacity(x);
            for j in 0..x {
                let w = rw(d, 16 * j + 12)?;
                if w != 0 && !(w >= 0x7000_0000 && (w - 0x7000_0000) % 0x40 == 0 && (w - 0x7000_0000) / 0x40 < j) {
                    return Err(format!("joint {j}: parent matrix address {w:#x}"));
                }
                let parent = if w == 0 { None } else { Some((w - 0x7000_0000) / 0x40) };
                joints.push(Joint { offset: [rf(d, 16 * j)?, rf(d, 16 * j + 4)?, rf(d, 16 * j + 8)?], parent });
            }
            m(n, 0, size);
            Ok(Part::Joints(joints))
        }
        K::T28 => {
            for i in 0..x {
                n.skip(0x20 * i as u64, 0x10);
                n.skip(0x20 * i as u64 + 0x19, 1);
                n.skip(0x20 * i as u64 + 0x1c, 4);
                m(n, 0x20 * i + 0x10, 8);
                m(n, 0x20 * i + 0x18, 1);
                m(n, 0x20 * i + 0x1a, 2);
            }
            Ok(Part::None)
        }
        K::LTab => {
            m(n, 0, 4 + 4 * x);
            Ok(Part::None)
        }
        K::List => {
            m(n, 0, 3);
            if d[3] != 0 {
                return Err(format!("list +3 byte {:#04x}, expected zero", d[3]));
            }
            m(n, 3, 1);
            m(n, 4, x);
            if d[4 + x + y] != 0xff {
                return Err(format!("list not ended by 0xFF: {:#04x}", d[4 + x + y]));
            }
            if y != 0 {
                m(n, 4 + x, 1);
                n.skip(5 + x as u64, y as u64 - 1);
            }
            n.skip(4 + x as u64 + y as u64, 1);
            Ok(Part::None)
        }
        K::TexId => {
            let mut recs = Vec::with_capacity(x);
            for i in 0..x {
                let e = &d[16 * i..16 * i + 12];
                let stop = e.iter().position(|&v| v == 0xff).ok_or_else(|| format!("texture-id record {i}: no 0xFF ends the texture numbers"))?;
                if e[stop..].iter().any(|&v| v != 0xff) {
                    return Err(format!("texture-id record {i}: fill after the texture numbers is not 0xFF"));
                }
                m(n, 16 * i, 16);
                recs.push(TextureIdRecord { numbers: e[..stop].to_vec(), offset: rw(d, 16 * i + 12)? as u32 as i32 });
            }
            Ok(Part::TexIds(recs))
        }
        K::PalShorts => {
            if d[..16].iter().any(|&v| v != 0) {
                return Err("palette address slots are not zero".to_string());
            }
            m(n, 0, 16);
            Ok(Part::None)
        }
        K::Pal => {
            m(n, 0, size);
            Ok(Part::None)
        }
        K::Seq => {
            n.skip(0, 0x10);
            m(n, 0x10, 12);
            m(n, 0x1c, 4 * x);
            Ok(Part::None)
        }
        K::Sx => {
            if d[2] != 0 || d[3] != 0 {
                return Err(format!("sequence record +2 and +3: {:#04x} {:#04x}, expected zero", d[2], d[3]));
            }
            m(n, 0, 0x20);
            Ok(Part::None)
        }
        K::PA => packet_a(&d[..size], x >> 16, Some(y), n).map(Part::PA),
        K::T14 => {
            m(n, 0, size);
            let mut bind = Vec::with_capacity(x);
            for j in 0..x {
                let mut mat = [[0f32; 4]; 4];
                for (r, row) in mat.iter_mut().enumerate() {
                    for (c, v) in row.iter_mut().enumerate() {
                        *v = rf(d, 64 * j + 16 * r + 4 * c)?;
                    }
                }
                bind.push(mat);
            }
            Ok(Part::Bind(bind))
        }
        K::PB => packet_b(d, size, n).map(Part::PB),
        K::Rt => {
            n.skip(0, size as u64);
            Ok(Part::None)
        }
        K::T10 => t10(d, x, n).map(|_| Part::None),
        K::Frame | K::Base => Ok(Part::None),
    }
}

/// The collision data (see `K::T10`): a 16-byte header (+0 read by nothing, then the sizes in bytes of the entries, the faces and the
/// points), 0x20-byte entries (the last one has flag bit 15), 8-byte points, 4-byte faces, and zero bytes up to the next structure.
fn t10(d: &[u8], nj: usize, n: &mut Node) -> Result<(), String> {
    if d.len() < 0x10 {
        return Err(format!("collision data of {:#x} bytes has no header", d.len()));
    }
    let (h4, h8, hc) = (rw(d, 4)?, rw(d, 8)?, rw(d, 0xc)?);
    if h4 % 0x20 != 0 || h8 % 4 != 0 || hc % 8 != 0 {
        return Err(format!("collision data sizes {h4:#x}, {h8:#x}, {hc:#x}: not whole entries, faces and points"));
    }
    let body = 0x10 + h4 + hc + h8;
    if body > d.len() {
        return Err(format!("collision data of {body:#x} bytes in {:#x}", d.len()));
    }
    if let Some(k) = d[body..].iter().position(|&v| v != 0) {
        return Err(format!("collision data: byte {:#04x} at +{:#x} after the faces", d[body + k], body + k));
    }
    n.skip(0, 4);
    m(n, 4, 12);
    let ne = h4 / 0x20;
    for i in 0..ne {
        let o = 0x10 + 0x20 * i;
        let (ty, fl) = (d[o], rh(d, o + 2)?);
        if fl & !0x8003 != 0 || (fl & 0x8000 != 0) != (i + 1 == ne) {
            return Err(format!("entry {i} at +{o:#x}: flags {fl:#06x} (only bits 0, 1 and, on the last entry only, 15)"));
        }
        m(n, o, 1);
        n.skip(o as u64 + 1, 1);
        m(n, o + 2, 2);
        let zero = |n: &mut Node, a: usize, b: usize| -> Result<(), String> {
            if let Some(k) = d[o + a..o + b].iter().position(|&v| v != 0) {
                return Err(format!("entry {i} (type {ty}) at +{o:#x}: byte {:#04x} at +{:#x}, expected zero", d[o + a + k], a + k));
            }
            m(n, o + a, b - a);
            Ok(())
        };
        match ty {
            1 => {
                zero(n, 4, 0x10)?;
                m(n, o + 0x10, 16);
            }
            3 => {
                m(n, o + 4, 4);
                zero(n, 8, 0x10)?;
                m(n, o + 0x10, 16);
            }
            2 => {
                let j = rw(d, o + 4)?;
                if j >= nj {
                    return Err(format!("entry {i}: joint {j} of {nj}"));
                }
                m(n, o + 4, 4);
                zero(n, 8, 0xc)?;
                m(n, o + 0xc, 4);
                m(n, o + 0x10, 12);
                zero(n, 0x1c, 0x20)?;
            }
            4 => {
                let (ja, jb) = (rh(d, o + 4)?, rh(d, o + 6)?);
                if ja >= nj || jb >= nj {
                    return Err(format!("entry {i}: joints {ja} and {jb} of {nj}"));
                }
                m(n, o + 4, 4);
                zero(n, 8, 0xc)?;
                m(n, o + 0xc, 4);
                zero(n, 0x10, 0x20)?;
            }
            _ => return Err(format!("entry {i} at +{o:#x}: type {ty}")),
        }
    }
    let p0 = 0x10 + h4;
    for k in 0..hc / 8 {
        let o = p0 + 8 * k;
        if rh(d, o + 6)? != 0 {
            return Err(format!("point {k}: w {:#06x}, expected zero", rh(d, o + 6)?));
        }
        m(n, o, 8);
    }
    let f0 = p0 + hc;
    for k in 0..h8 / 4 {
        let o = f0 + 4 * k;
        if let Some(&v) = d[o..o + 3].iter().find(|&&v| v as usize >= hc / 8) {
            return Err(format!("face {k}: point number {v} of {}", hc / 8));
        }
        m(n, o, 4);
    }
    m(n, body, d.len() - body);
    Ok(())
}

/// The hull blocks: an index row, then the blocks it names (each runs to the next block's start; the last to the end).
fn hull(d: &[u8], n: &mut Node) -> Result<(), String> {
    let size = d.len();
    if size < 16 {
        return Err(format!("hull blocks of {size:#x} bytes have no index row"));
    }
    let mut idx: Vec<usize> = d[..16].iter().filter(|&&v| v != 0xff).map(|&v| v as usize).collect();
    idx.sort_unstable();
    idx.dedup();
    n.skip(0, 1);
    m(n, 1, 15);
    if idx.first().is_none_or(|&i| i < 1) {
        return Err(format!("hull index row {:02x?}", &d[..16]));
    }
    for j in 0..idx.len() {
        let p = 16 * idx[j];
        let last = j + 1 == idx.len();
        let end = if last { size } else { 16 * idx[j + 1] };
        if end > size || p + 16 > end {
            return Err(format!("hull block {} at {p:#x} runs to {end:#x} in {size:#x}", idx[j]));
        }
        if d[p..p + 16].iter().all(|&v| v == 0) {
            if !last && end - p != 16 {
                return Err(format!("empty hull block at {p:#x} of {:#x} bytes", end - p));
            }
            if last && d[p + 16..end].iter().any(|&v| v != 0) {
                return Err(format!("data after the last hull block at {p:#x}"));
            }
            m(n, p, end - p);
            continue;
        }
        if p + 0x18 > end {
            return Err(format!("hull block at {p:#x} has no point count"));
        }
        let cnt = rs(d, p + 0x16)?;
        if cnt <= 0 {
            return Err(format!("hull block at {p:#x}: {cnt} points"));
        }
        let cnt = cnt as usize;
        let sz = 16 + (8 * cnt).div_ceil(16) * 16;
        if p + sz > end || (!last && p + sz != end) {
            return Err(format!("hull block at {p:#x}: {cnt} points need {sz:#x} bytes, the block has {:#x}", end - p));
        }
        if d[p + 16 + 8 * cnt..end].iter().any(|&v| v != 0) {
            return Err(format!("hull block at {p:#x}: padding after the points is not zero"));
        }
        m(n, p, end - p);
    }
    Ok(())
}

/// Packet B: the skinned-rows layout or the rigid-weight layout (told apart by the size words they must satisfy), see `K::PB`'s
/// game string for the readers.
fn packet_b(d: &[u8], size: usize, n: &mut Node) -> Result<PacketB, String> {
    if size < 16 || size % 16 != 0 {
        return Err(format!("packet B of {size:#x} bytes"));
    }
    let q16 = |v: usize| v.div_ceil(16) * 16;
    let cnt = rw(d, 0)?;
    // The rigid-weight layout: the u32 header names the row count and the ends of the three output arrays.
    if size == 16 + 16 * cnt && rw(d, 4)? == q16(4 * cnt) && rw(d, 8)? == q16(4 * cnt) + q16(6 * cnt) && rw(d, 12)? == 2 * q16(4 * cnt) + q16(6 * cnt) {
        m(n, 0, 16);
        let mut rows = Vec::with_capacity(cnt);
        for r in 0..cnt {
            let o = 16 + 16 * r;
            m(n, o, 6);
            m(n, o + 6, 2);
            m(n, o + 8, 3);
            m(n, o + 11, 1);
            m(n, o + 12, 4);
            rows.push(RigidRow {
                pos: [rs(d, o)? as i16, rs(d, o + 2)? as i16, rs(d, o + 4)? as i16],
                lookup: [d[o + 6], d[o + 7]],
                joints: [d[o + 8], d[o + 9], d[o + 10]],
                joint_count: d[o + 11],
                weights: [d[o + 12], d[o + 13], d[o + 14], d[o + 15]],
            });
        }
        return Ok(PacketB::Rigid(RigidB { ends: [rw(d, 4)? as u32, rw(d, 8)? as u32, rw(d, 12)? as u32], rows }));
    }
    let h: Vec<usize> = (0..8).map(|i| rh(d, 2 * i)).collect::<Result<_, _>>()?;
    let (n0, rows, copies, verts, roff) = (h[0], h[1] + h[2] + h[3], h[4], h[5], h[6]);
    let pad = (2 * n0).next_multiple_of(8);
    let ext = 0x10 + pad + 2 * copies;
    if verts != rows + copies || roff != ext.next_multiple_of(16) {
        return Err(format!("packet B header {h:?}: neither layout"));
    }
    let look = (3usize.wrapping_sub(rows)) % 4;
    let look = if look == 0 { 4 } else { look };
    if size != roff + 16 * (rows + look) {
        return Err(format!("packet B header {h:?}: {size:#x} bytes, {} expected", roff + 16 * (rows + look)));
    }
    if d[0x10 + 2 * n0..0x10 + pad].iter().any(|&v| v != 0) || d[ext..roff].iter().any(|&v| v != 0) {
        return Err("packet B padding is not zero".to_string());
    }
    m(n, 0, 16);
    m(n, 0x10, 2 * n0);
    m(n, 0x10 + 2 * n0, pad - 2 * n0);
    m(n, 0x10 + pad, 2 * copies);
    m(n, ext, roff - ext);
    for r in 0..rows + look {
        let o = roff + 16 * r;
        m(n, o, 16);
    }
    let pairs = (0..n0).map(|i| (d[0x10 + 2 * i], d[0x11 + 2 * i])).collect();
    let copy_slots = (0..copies).map(|i| rh(d, 0x10 + pad + 2 * i).map(|v| v as u16)).collect::<Result<Vec<_>, _>>()?;
    let mut skin_rows = Vec::with_capacity(rows);
    for r in 0..rows {
        let o = roff + 16 * r;
        let w0 = rh(d, o)?;
        skin_rows.push(SkinRow {
            slot: (w0 & 0x1ff) as u16,
            joint: (w0 >> 9) as u8,
            vu0: [d[o + 2], d[o + 3]],
            weights: [d[o + 4], d[o + 5], d[o + 6], d[o + 7]],
            lookup: [d[o + 8], d[o + 9]],
            pos: [rs(d, o + 10)? as i16, rs(d, o + 12)? as i16, rs(d, o + 14)? as i16],
        });
    }
    let tail = (0..look)
        .map(|k| {
            let o = roff + 16 * (rows + k);
            let mut l = [0u16; 8];
            for (i, v) in l.iter_mut().enumerate() {
                *v = u16::from_le_bytes([d[o + 2 * i], d[o + 2 * i + 1]]);
            }
            l
        })
        .collect();
    Ok(PacketB::Skinned(SkinnedB { groups: [h[1] as u16, h[2] as u16, h[3] as u16], constant_normals: h[7] as u16, pairs, copies: copy_slots, rows: skin_rows, tail }))
}

/// Packet A: UNPACK codes (VIFcode: immediate u16, count u8, command u8) with their data, and zero NOP codes. The texture setup at
/// the end (`tail` quadwords, the mesh record's high half) is 64-byte blocks. `vcount` is the mesh record's vertex count. The
/// three UNPACKs VU1 program 0 reads are (VU1 address as an offset from TOPS, which the immediate adds: bit 15 set): V2-16
/// (0x75) texture coordinates at 0xC2, V4-8 (0x6E) the strip list at 0x12D and V4-32 (0x6C) the texture setups right after the
/// strip list; any other layout is an error.
fn packet_a(p: &[u8], tail: usize, vcount: Option<usize>, n: &mut Node) -> Result<PacketA, String> {
    let bound = vcount.unwrap_or(0x61);
    struct Up {
        cmd: usize,
        num: usize,
        imm: usize,
        at: usize,
        sz: usize,
    }
    let mut ups: Vec<Up> = Vec::new();
    let mut i = 0usize;
    while i < p.len() {
        let w = u32_at(p, i)?;
        let (cmd, num) = ((w >> 24) as usize, ((w >> 16) & 0xff) as usize);
        if w == 0 {
            m(n, i, 4);
            i += 4;
            continue;
        }
        if !(0x60..=0x7f).contains(&cmd) {
            return Err(format!("+{i:#x}: VIFcode {w:#010x}"));
        }
        let (vn, vl) = ((cmd & 0xf) >> 2, cmd & 3);
        let bits = match vl {
            0 => 32,
            1 => 16,
            2 => 8,
            _ => return Err(format!("+{i:#x}: UNPACK {cmd:#04x}")),
        };
        let sz = ((vn + 1) * bits * num).div_ceil(8).next_multiple_of(4);
        if i + 4 + sz > p.len() {
            return Err(format!("+{i:#x}: UNPACK {cmd:#04x} of {num} elements past the packet's {:#x} bytes", p.len()));
        }
        m(n, i, 4);
        ups.push(Up { cmd, num, imm: (w & 0xffff) as usize, at: i + 4, sz });
        if cmd == 0x6c && tail != 0 {
            if num != tail || i + 4 + sz != p.len() {
                return Err(format!("+{i:#x}: texture setup of {num} quadwords, the mesh record says {tail}, ends at {:#x} of {:#x}", i + 4 + sz, p.len()));
            }
            // Words 0, 1, 4, 5 and 8 are read by func_L01_00423E10 (and then rewritten, with 9, 12 and 13, by func_L01_00423700);
            // words 2, 3, 6, 7, 10, 11, 14 and 15 are copied whole quadwords by VU1 program 0 (LQI at micro 0x1788 to 0x17A0).
            for blk in 0..num / 4 {
                let o = i + 4 + 64 * blk;
                m(n, o, 0x24);
                n.skip(o as u64 + 0x24, 4);
                m(n, o + 0x28, 8);
                n.skip(o as u64 + 0x30, 8);
                m(n, o + 0x38, 8);
            }
            i += 4 + sz;
            continue;
        }
        m(n, i + 4, sz);
        i += 4 + sz;
    }
    if tail != 0 && p.len() < 4 + 16 * tail {
        return Err(format!("texture setup of {tail} quadwords in a packet of {:#x} bytes", p.len()));
    }
    let mut uv: Option<&Up> = None;
    let mut list: Option<&Up> = None;
    let mut tex: Option<&Up> = None;
    for u in &ups {
        if u.imm & 0xc000 != 0x8000 {
            return Err(format!("UNPACK {:#04x} immediate {:#06x}: expected the TOPS flag and signed data", u.cmd, u.imm));
        }
        let slot = match (u.cmd, u.imm & 0x3ff) {
            (0x75, 0xc2) => &mut uv,
            (0x6e, 0x12d) => &mut list,
            (0x6c, _) => &mut tex,
            (c, a) => return Err(format!("UNPACK {c:#04x} to VU1 address TOPS + {a:#x}: not one VU1 program 0 reads")),
        };
        if slot.replace(u).is_some() {
            return Err(format!("two UNPACK {:#04x} in one packet", u.cmd));
        }
    }
    let list = list.ok_or("packet A has no strip list")?;
    if let Some(t) = tex {
        if t.imm & 0x3ff != 0x12d + list.num || t.num != tail || tail % 4 != 0 {
            return Err(format!("texture setup at TOPS + {:#x} of {} quadwords after a strip list of {}", t.imm & 0x3ff, t.num, list.num));
        }
    } else if tail != 0 {
        return Err("the mesh record names a texture setup the packet does not have".to_string());
    }
    let uvs = match uv {
        Some(u) => {
            if vcount.is_some_and(|c| u.num != c) || u.num > bound || u.sz != 4 * u.num {
                return Err(format!("{} texture coordinates for {bound} vertices", u.num));
            }
            let mut v = Vec::with_capacity(u.num);
            for k in 0..u.num {
                let o = u.at + 4 * k;
                v.push([rs(p, o)? as f32 / 4096.0, rs(p, o + 2)? as f32 / 4096.0]);
            }
            Some(v)
        }
        None => None,
    };
    let texbytes: &[u8] = match tex {
        Some(t) => &p[t.at..t.at + t.sz],
        None => &[],
    };
    let textures: Vec<TextureSetup> = (0..texbytes.len() / 64)
        .map(|k| {
            let o = 64 * k;
            Ok(TextureSetup { local: rw(texbytes, o + 0x20)? as u32 as i32, k: rw(texbytes, o)? as u32, mmin: rw(texbytes, o + 4)? as u32, wms: rw(texbytes, o + 0x10)? as u32, wmt: rw(texbytes, o + 0x14)? as u32, a_count: rh(texbytes, o + 8)? as u16 as i16 })
        })
        .collect::<Result<_, String>>()?;
    let (cull, prepass_count, strips) = strip_list(&p[list.at..list.at + list.sz], texbytes, bound)?;
    Ok(PacketA { uvs, cull, prepass_count, strips, textures })
}

/// Decodes a packet A given as bytes (a mesh record's `size` = 16 * low byte of its +4 word, `tail` its high half; or the VIF1 stream
/// of a packet A found in RAM), without a vertex count: the vertex indices must be at most 97. For the check against a RAM dump.
pub fn decode_packet_a(p: &[u8], tail: usize) -> Result<PacketA, String> {
    let mut t = Tracker::new();
    let mut root = t.root("check", "packet A", p.len() as u64, Space::Unpacked);
    let l = t.line("check", "packet A", "", "", Space::Unpacked);
    let mut n = t.child(&mut root, l, 0, p.len() as u64);
    packet_a(p, tail, None, &mut n)
}

/// The strip list (V4-8, the quadwords as signed bytes) and the texture setups, decoded as VU1 program 0 walks them (the loop
/// of the module doc's "strip list").
fn strip_list(list: &[u8], tex: &[u8], vcount: usize) -> Result<(i8, i8, Vec<Strip>), String> {
    let num = list.len() / 4;
    let ntex = tex.len() / 64;
    if num < 1 {
        return Err("empty strip list".to_string());
    }
    let (cull, y, z, w) = (list[0] as i8, list[1], list[2] as i8, list[3] as i8);
    if (ntex == 0 && y != 0) || (ntex != 0 && y as usize != num) {
        return Err(format!("strip list header: texture offset {y} for {num} quadwords and {ntex} texture setups"));
    }
    if z < 0 {
        return Err(format!("strip list header: first vertex {z}"));
    }
    let e: Vec<i8> = list[4..].iter().map(|&v| v as i8).collect();
    let zeros: Vec<usize> = e.iter().enumerate().filter(|(_, v)| **v == 0).map(|(i, _)| i).collect();
    if zeros.len() < ntex + 1 {
        return Err(format!("strip list: {} zero entries for {ntex} texture setups", zeros.len()));
    }
    // The vertex that starts the group after zero k: the header's byte +2 for k = 0, else the w word of texture setup quadword k - 1
    // (func_L01_0043EA78's data; micro 0x1360, 0xCA0 and the same pairs of each slot: `ILW.w vi12, 0(vi11)`, then 0(vi01) with
    // vi01 recovered from vf10w, one quadword further at each zero).
    let sub = |k: usize| -> Result<usize, String> {
        let v = if k == 0 { z as usize } else { rw(tex, 16 * (k - 1) + 12)? };
        if v >= 128 {
            return Err(format!("group {k} starts with vertex {v}"));
        }
        Ok(v)
    };
    for k in 0..ntex {
        if sub(k)? == 0 {
            return Err(format!("group {k} has no first vertex"));
        }
    }
    if sub(ntex)? != 0 {
        return Err(format!("the last group's end marker is {}", sub(ntex)?));
    }
    if let Some(&bad) = e[zeros[ntex]..].iter().find(|&&v| v != 0) {
        return Err(format!("strip list: entry {bad} after the end marker"));
    }
    let vertex = |v: i8, first: bool| -> Result<StripVertex, String> {
        let (idx, neg) = if v > 0 { (v as usize, false) } else { ((v as i32 + 128) as usize, true) };
        if idx < 1 || idx > vcount {
            return Err(format!("strip vertex {v}: index {idx} of {vcount} vertices"));
        }
        Ok(StripVertex { index: (idx - 1) as u8, skip: neg || first })
    };
    let mut strips = Vec::new();
    // The entries before the first zero start a strip that uses the texture bound before (their first entry carries ADC).
    if zeros[0] > 0 {
        let mut ents = &e[..zeros[0]];
        if ntex == 0 {
            // The three entries before the end marker are the pipeline's look-ahead, never drawn; the data holds 1 there.
            if ents.len() < 3 || ents[ents.len() - 3..].iter().any(|&v| v != 1) {
                return Err("strip list: the last three entries before the end marker are not 1".to_string());
            }
            ents = &ents[..ents.len() - 3];
        }
        if ents[0] >= 0 {
            return Err(format!("strip list: first entry {} without ADC", ents[0]));
        }
        strips.push(Strip { texture: None, vertices: ents.iter().map(|&v| vertex(v, false)).collect::<Result<_, _>>()? });
    }
    for k in 0..ntex {
        let mut ents = &e[zeros[k] + 1..zeros[k + 1]];
        if k + 1 == ntex {
            if ents.len() < 3 || ents[ents.len() - 3..].iter().any(|&v| v != 1) {
                return Err("strip list: the last three entries before the end marker are not 1".to_string());
            }
            ents = &ents[..ents.len() - 3];
        }
        let mut vs = vec![vertex(sub(k)? as i8, true)?];
        for &v in ents {
            vs.push(vertex(v, false)?);
        }
        strips.push(Strip { texture: Some(k), vertices: vs });
    }
    Ok((cull, w, strips))
}

const ARMOR: &str = "moby::armor";
const ARMOR_LOAD: &str = "the block is loaded by func_L01_00422838 (frontbin func_00393A18, front end func_00398AD0) into the buffer at 0x1D4B64 and relocated by func_L01_00422AB8 (frontbin func_00393C98), which calls func_L01_00423F28 and then func_L01_00424008 (called from func_L01_00422BD0, func_L01_004D2370 and func_L01_00428750)";

/// Decodes one WAD5 armor model block (`b` is the whole block, a whole number of sectors, `at` its place in `parent`, Disc space) into
/// lines of `group`. Records 0 to 34 have a 16-byte header (the counts n0, n1 and n2 of mesh records in three groups, a count byte
/// that is n0 + n1, the u32 0x10, the offset of the texture-id records, a zero word), n0 + n1 + n2 mesh records in the class form
/// (their offsets are from the block start), the packets A and B they name, which tile the block up to the texture-id records, those
/// records and zero bytes to the end of the sector: func_L01_00423F28 relocates exactly these and func_L01_00424008 patches the
/// counts and the two offsets into moby class 0's header (+4 to +7, +0 and +0x20), so that func_L01_0043EA78 draws class 0 with the
/// armor's meshes. Records 35 to 55 (first bytes 50 04 00 00 or A0 04 00 00) are whole moby classes no story code loads: an ignored line.
///
/// Returns the block's meshes (`Some`), or `None` for a class-form record. The block's mesh records, packets and texture-id records are
/// decoded exactly as a class's are (`packet_a`, `Mesh::a_at`): `a_at` is the packet A's offset from the block's start, and the
/// game's RAM holds the block at its load address (see `Armor`), so packet A is at that address plus `a_at`. Any failing structure
/// fails its node and the function returns its error.
pub fn armor(t: &mut Tracker, parent: &mut Node, group: &str, at: u64, b: &[u8]) -> Result<Option<Armor>, String> {
    let len = b.len();
    let place = format!("{} armor block at {at:#x}", parent.place);
    if len < 16 {
        return Err(format!("{place}: {len:#x} bytes"));
    }
    let (n0, n1, n2, tot) = (rb(b, 0)?, rb(b, 1)?, rb(b, 2)?, rb(b, 3)?);
    if tot != n0 + n1 || rw(b, 4)? != 0x10 || rw(b, 12)? != 0 {
        // Records 35 to 55 hold whole moby classes in the class form, but func_L01_00423F28 (which relocates a record's
        // block in the story levels) reads the other layout's header, and no loader of these was found in the story
        // overlays or the front ends: read by nothing the decode covers (the multiplayer overlays are left out).
        if parent.overlaps(at, len as u64) {
            return Err(format!("{place}: overlaps another structure"));
        }
        let l = t.ignored_line(
            group,
            "armor model block in the moby class form (records 35 to 55)",
            "read by nothing: func_L01_00423F28, which relocates an armor record's model block in the story levels, reads the other layout's header, and no loader of these blocks was found in the 34 story overlays or the front ends (the multiplayer overlays are left out by the player)",
            Space::Disc,
        );
        let n = t.child(parent, l, at, len as u64);
        t.close(n);
        return Ok(None);
    }
    let nrec = n0 + n1 + n2;
    let o8 = rw(b, 8)?;
    let first = 16 + 16 * nrec;
    let planned = (|| -> Result<Vec<(usize, usize, K, usize, usize)>, String> {
        if first > o8 || o8 > len {
            return Err(format!("{place}: {nrec} records, texture-id records at {o8:#x} in {len:#x} bytes"));
        }
        let mut pieces: Vec<(usize, usize, K, usize, usize)> = Vec::new();
        for r in 0..nrec {
            let o = 16 + 16 * r;
            let (a0, w1, a2, w3) = (rw(b, o)?, rw(b, o + 4)?, rw(b, o + 8)?, rw(b, o + 12)?);
            pieces.push((a0, 16 * (w1 & 0xff), K::PA, w1, w3 >> 24));
            pieces.push((a2, 16 * (w3 & 0xff), K::PB, w3, 0));
        }
        pieces.sort_by_key(|p| p.0);
        let mut pos = first;
        for p in &pieces {
            if p.0 != pos || p.0 + p.1 > o8 {
                return Err(format!("{place}: packet at {:#x} of {:#x} bytes where {pos:#x} was expected", p.0, p.1));
            }
            pos = p.0 + p.1;
        }
        if pos != o8 {
            return Err(format!("{place}: packets end at {pos:#x}, the texture-id records start at {o8:#x}"));
        }
        Ok(pieces)
    })();
    let (pieces, ntex) = match planned.and_then(|pc| {
        let mut q = o8;
        let mut n = 1;
        loop {
            if q + 16 > len {
                return Err(format!("{place}: texture-id records at {o8:#x} not ended before {len:#x}"));
            }
            if (rw(b, q + 12)? as u32 as i32) < 0 {
                return Ok((pc, n));
            }
            q += 16;
            n += 1;
        }
    }) {
        Ok(v) => v,
        Err(e) => {
            let l = t.line(group, "armor model block (not split)", "", "", Space::Disc);
            let mut n = t.child(parent, l, at, len as u64);
            n.fail(e.clone());
            t.close(n);
            return Err(e);
        }
    };
    let mut first_err: Option<String> = None;
    let l = t.line(
        group,
        "armor model block header (16 bytes: +0, +1, +2 mesh record counts, +3 a count byte, +4 the mesh records' offset, +8 the texture-id records' offset, +0xC zero)",
        ARMOR,
        "func_L01_00423F28 (+0, +1, +2, +4, +8) and func_L01_00424008 (+0 to +3 into moby class 0's +4 to +7, +4 and +8 into its +0 and +0x20; func_L01_0043EA78 reads the class's +7) (same in all 34 story levels)",
        Space::Disc,
    );
    t.note(l, ARMOR_LOAD);
    t.note(l, "+0xC: no reader (func_L01_00423F28 and func_L01_00424008 read +0 to +8 only)");
    let mut n = t.child(parent, l, at, 16);
    n.mark(0, 12);
    n.skip(12, 4);
    t.close(n);
    let l = t.line(
        group,
        "armor model block mesh records (16 bytes: +0 packet A, +4 its quadword count and the texture setup's, +8 packet B, +0xC four counts)",
        DEC,
        "func_L01_00423F28 (relocates +0 and +8), func_L01_00423E10 (+6), func_L01_0043EA78 (+4, +0xC) (same in all 34 story levels)",
        Space::Disc,
    );
    let mut n = t.child(parent, l, at + 16, 16 * nrec as u64);
    let recs = match decode(K::Rec, &b[16..first], 16 * nrec, nrec, 0, &mut n) {
        Ok(Part::Rec(r)) => Some(r),
        Ok(_) => unreachable!("K::Rec decodes to Part::Rec"),
        Err(e) => {
            n.fail(e.clone());
            first_err.get_or_insert(e);
            None
        }
    };
    t.close(n);
    let mut pa: std::collections::HashMap<usize, PacketA> = std::collections::HashMap::new();
    let mut pb: std::collections::HashMap<usize, PacketB> = std::collections::HashMap::new();
    for p in &pieces {
        let sp = p.2.spec();
        let l = t.line(group, sp.name, sp.decoder, sp.game, Space::Disc);
        if !sp.note.is_empty() {
            t.note(l, sp.note);
        }
        let mut n = t.child(parent, l, at + p.0 as u64, p.1 as u64);
        if p.1 != 0 {
            match decode(p.2, &b[p.0..p.0 + p.1], p.1, p.3, p.4, &mut n) {
                Ok(Part::PA(a)) => {
                    pa.insert(p.0, a);
                }
                Ok(Part::PB(v)) => {
                    pb.insert(p.0, v);
                }
                Ok(_) => unreachable!("K::PA and K::PB decode to their own parts"),
                Err(e) => {
                    n.fail(e.clone());
                    first_err.get_or_insert(e);
                }
            }
        }
        t.close(n);
    }
    let l = t.line(
        group,
        "armor model block texture-id records (16 bytes: texture numbers ended by 0xFF, 0xFF fill, an offset; the last has a negative +0xC)",
        DEC,
        "func_L01_00423F28 (maps the numbers through the table at gp - 0x5B90, relocates +0xC, until a record's +0xC is negative), func_L01_00423E10 (same in all 34 story levels)",
        Space::Disc,
    );
    let mut n = t.child(parent, l, at + o8 as u64, 16 * ntex as u64);
    let texture_ids = match decode(K::TexId, &b[o8..o8 + 16 * ntex], 16 * ntex, ntex, 0, &mut n) {
        Ok(Part::TexIds(v)) => Some(v),
        Ok(_) => unreachable!("K::TexId decodes to Part::TexIds"),
        Err(e) => {
            n.fail(e.clone());
            first_err.get_or_insert(e);
            None
        }
    };
    t.close(n);
    let end = o8 + 16 * ntex;
    if end < len {
        let l = t.line(group, "armor model block zero padding (to a whole sector)", ARMOR, NOTHING, Space::Disc);
        let mut n = t.child(parent, l, at + end as u64, (len - end) as u64);
        match b[end..].iter().position(|&v| v != 0) {
            Some(k) => {
                let e = format!("byte {:#04x} at +{:#x} after the texture-id records, expected zero", b[end + k], end + k);
                n.fail(format!("{place}: {e}"));
                first_err.get_or_insert(e);
            }
            None => n.mark(0, (len - end) as u64),
        }
        t.close(n);
    }
    if let Some(e) = first_err {
        return Err(format!("{place}: {e}"));
    }
    let meshes = build_meshes(&recs.expect("no error"), &pa, &pb).map_err(|e| format!("{place}: {e}"))?;
    Ok(Some(Armor { counts: [n0 as u8, n1 as u8, n2 as u8], meshes, texture_ids: texture_ids.expect("no error") }))
}

/// One decoded WAD5 armor model block (a mesh block of the armor WAD, the first member of pair 2 * entry of `tc.wads[4]`).
///
/// Where it is in RAM: func_L01_00422838 reads the entry's texture block and then its mesh block over it into the buffer whose
/// address variable is `armor_buffer` (0x1D4B64 on Veldin, holding 0x1F28000: the hero's armor, entry 0 to 4, or the skin 5 to 13),
/// and func_L01_00422D70 reads entry 31 (the wrench) into the buffer the variable at 0x1EAA34 holds (0x1F20000 on Veldin). Both
/// are then relocated in place by func_L01_00423F28: the mesh records' offsets +0 and +8 become the block's RAM address plus the
/// offset. Packet A of mesh `m` is therefore at RAM `base + m.a_at`.
///
/// Texture numbers: a texture setup's word 8 (`TextureSetup::local`) is mapped by func_L01_00423E10 (0x423E8C `lbu t1, 0(s7 + t1)`)
/// through the 16 bytes at gp - 0x5B90 (the remap table func_L01_00422AB8 and func_L01_00422F28 pass as `a2`), which gives the armor
/// texture slot; the slot is the index of the image in the entry's texture block (the pair's second member: a count and three
/// offsets of 2FIP images, see `wads.rs`). func_L01_00423E10 passes the slot to func_L01_004239F8 (its `a1` is 0, so the core
/// index's moby texture table and func_L01_00423700 are not used), which rewrites words 0, 1, 4, 5 of the setup with the registers,
/// words 8 and 9 with the slot's TEX0 (`0x1F3E40 + 24 * slot`, filled from the TEX0 array at 0x1D4B70 + 8 * slot by
/// func_L01_00422AB8, and for the wrench slot 0 from 0x1D4B98 by func_L01_00422F28) and words 12 and 13 with its MIPTBP1.
#[derive(Clone, Debug)]
pub struct Armor {
    /// Header +0, +1, +2: the mesh record counts of the three groups (func_L01_00424008 copies them to class +4 to +6).
    pub counts: [u8; 3],
    /// The mesh records in file order, each with its packets decoded; `a_at` and `b_at` are offsets from the block's start.
    pub meshes: Vec<Mesh>,
    /// The texture-id records (the numbers are the file's, before the remap table).
    pub texture_ids: Vec<TextureIdRecord>,
}

// ---------------------------------------------------------------------------------------------------------------------------------
// The skin job

/// VU arithmetic (the VU0 and VU1 FMAC): operands with a denormal value read as zero, a product or a sum is the exact result rounded
/// toward zero, a result below the smallest normal float is zero, and an overflow is the largest float.
fn vflush(x: f32) -> f32 {
    if x.abs() < f32::MIN_POSITIVE { 0.0 } else { x }
}

/// The exact value `x` rounded toward zero to f32.
fn vrz(x: f64) -> f32 {
    let mut y = x as f32;
    if (y as f64).abs() > x.abs() {
        y = f32::from_bits(y.to_bits() - 1);
    }
    vflush(y)
}

fn vmul(a: f32, b: f32) -> f32 {
    vrz(vflush(a) as f64 * vflush(b) as f64)
}

/// The sum rounded toward zero. The float64 sum alone loses an operand that is more than 2^53 times smaller than the other, which
/// decides the last bit when the signs differ (a matrix row of 1.0 and 1e-16 residues does this): the rounding error of the float64
/// sum (Knuth's TwoSum) says on which side of the float64 result the exact sum lies.
fn vadd(a: f32, b: f32) -> f32 {
    let (a, b) = (vflush(a) as f64, vflush(b) as f64);
    let s = a + b;
    let bb = s - a;
    let err = (a - (s - bb)) + (b - bb);
    let mut y = vrz(s);
    if err != 0.0 && y as f64 == s && (err < 0.0) != (s < 0.0) && s != 0.0 {
        y = vflush(f32::from_bits(y.to_bits() - 1));
    }
    y
}

/// VMAX of two floats.
fn vmax(a: f32, b: f32) -> f32 {
    if vflush(a) > vflush(b) { a } else { b }
}

/// VRSQRT with fs = 1.0: 1 / sqrt(x) rounded toward zero; the largest float for 0.
fn vrsqrt1(x: f32) -> f32 {
    if x == 0.0 { f32::MAX } else { vrz(1.0 / (x.abs() as f64).sqrt()) }
}

type Row = [f32; 4];
type Mat = [Row; 4];

/// The light section of a skin job's draw record: the 8 quadwords at the record's +0x40 to +0xBF that func_003D00F8 DMAs to the
/// scratchpad 0x70001C00.
///
/// func_L01_0043EA78 writes them for every moby it queues (the record is built in the scratchpad at 0x70002C00 or 0x70003000 and
/// DMA'd to the job list in RAM; at the end of a frame the lists of the last frames are still in RAM: level 1, the list of the
/// traced frame at 0x800000, the next frame's at 0x680000, an older one at 0x620000). They come from the moby instance and the light
/// sets:
/// - the moby's light choice, the u64 at instance +0x38: bits 0 to 3 the light set A (func_L01_0043EA78 copies the 20 sets of 0x40
///   bytes at 0x1F17C0, `lui 0x1F; addiu 0x17C0` at 0x43EBDC, to the scratchpad 0x70002000; set k is at 0x70002000 + 0x40 k, and a
///   set is two lights: colour 0 and colour 1 at +0x00 and +0x20 (x, y, z, w), direction 0 and 1 at +0x10 and +0x30), bits 8 to 11
///   the set B and bits 16 to 23 the weight w of B in 1/256 (0: set A alone, 0x43F288 to 0x43F300; else 0x43F308 to 0x43F450: the
///   colours and directions of A * (1 - w) + B * w, the directions renormalised by VRSQRT);
/// - the instance's rotation, the three rows at +0xC0, +0xD0 and +0xE0 (x, y, z, w; the instance's position is +0x10): the directions
///   are carried into the model's frame with the transpose of its 3 x 3 (vf12 to vf14 at 0x43F098 to 0x43F0B8, used at 0x43F290 to
///   0x43F2B8 and 0x43F3B8 to 0x43F44C);
/// - the instance's colour +0x3C (four bytes), +0x78 (the flat colour) and +0x23 (alpha scale).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JobLights {
    /// Record +0x40, +0x50, +0x60 (`sw` at 0x43F2D8 to 0x43F2F8 and 0x43F404 to 0x43F448): the three rows of the matrix of light
    /// directions: row i holds, in lane 0, light 0's direction along axis i (x, y, z) in the model's frame and, in lane 1, light 1's;
    /// lanes 2 and 3 are zero. The skin job loads them as vf25 to vf27. A vertex's light value k is the sum of row i lane k times the
    /// normal's component i.
    pub dirs: [Row; 3],
    /// Record +0x70, +0x80, +0x90: the light colours (light 0, light 1, and a zero row): +0x70 and +0x80 are the set's colours (the
    /// words are clamped below at zero as integers, `pmaxw` at 0x43F2A0 and 0x43F2A4), +0x90 is zero. They are vf28 to vf30 of the
    /// skin job.
    pub colours: [Row; 3],
    /// Record +0xA0, +0xA4, +0xA8 (the sets' colour 0 and colour 1 w words, `sw` at 0x43F2C8 and 0x43F2CC; 0 for the third): the
    /// wrap factors. The job loads the quadword as vf1 and takes the minimum with 0.0: a vertex's light value is max(d, d * min(w, 0)).
    pub wrap: [f32; 3],
    /// Record +0xAC: the instance's flat colour, the word at instance +0x78 with its top byte replaced by the byte at record +0xBC
    /// (`lw`, `sw` and `sb` at 0x440018 to 0x440034, taken when the instance's +0x7E bit 0 is set). The skin job writes it as the
    /// colour of the last `constant_normals` vertices of a mesh whose packet B header +0xE is not zero.
    pub flat: [u8; 4],
    /// Record +0xB0: the instance's base colour as four floats: the bytes of the instance's +0x3C (with the alpha byte computed at
    /// 0x43F000 to 0x43F068 ORed into the top) as 65536 + byte / 128 (`pextlb`, `pextlh`, `por` with 0x47800000 at 0x43F050 to
    /// 0x43F068). It is the start of the colour sum (vf31); the low byte of the final float is the colour byte.
    pub base: Row,
}

fn f32_at(b: &[u8], o: usize) -> Result<f32, String> {
    u32_at(b, o).map(f32::from_bits)
}

fn row_at(b: &[u8], o: usize) -> Result<Row, String> {
    Ok([f32_at(b, o)?, f32_at(b, o + 4)?, f32_at(b, o + 8)?, f32_at(b, o + 12)?])
}

impl JobLights {
    /// Reads the light section from a draw record `job` (the 0xC0 header bytes of the record in RAM).
    pub fn read(job: &[u8]) -> Result<JobLights, String> {
        if job.len() < 0xC0 {
            return Err(format!("a draw record's header is 0xC0 bytes, got {}", job.len()));
        }
        Ok(JobLights {
            dirs: [row_at(job, 0x40)?, row_at(job, 0x50)?, row_at(job, 0x60)?],
            colours: [row_at(job, 0x70)?, row_at(job, 0x80)?, row_at(job, 0x90)?],
            wrap: [f32_at(job, 0xA0)?, f32_at(job, 0xA4)?, f32_at(job, 0xA8)?],
            flat: [job[0xAC], job[0xAD], job[0xAE], job[0xAF]],
            base: row_at(job, 0xB0)?,
        })
    }
}

/// The second job group of a draw record (func_003D00F8's second entry, level 1 0x3F8048): the meshes of the rigid layout. The
/// group follows the record's mesh entries: at record + 0x40 + 0x10 * (n / 2 + 9) for the first group's n mesh entries (what
/// func_003D00F8 returns in `s1`), 0x40 bytes of matrix and then its own 8-byte mesh entries (packet B address << 4 | quadword count,
/// destination) ended by a zero word; the record's +0x14 is its quadword count in the high half and the alpha byte in the low.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RigidInputs {
    /// The first three rows of the group's 4 x 4 matrix (func_003D00F8 DMAs the group to 0x70001C00 and loads them as vf22 to vf24 at
    /// 0x3F8118 to 0x3F8120): the texture coordinate of a vertex is the s and t of its transformed normal's three dots with them.
    pub view: [Row; 3],
    /// The alpha byte of every output colour: the low byte of the record's +0x14 (`andi sp, a0, 0xFFFF` at 0x3F8084, `sb sp, 3(a1)`
    /// at 0x3F8314).
    pub alpha: u8,
}

impl RigidInputs {
    /// `group` are the bytes of the second group (at least 0x30 for the three rows), `field_14` the record's +0x14 word.
    pub fn read(group: &[u8], field_14: u32) -> Result<RigidInputs, String> {
        Ok(RigidInputs { view: [row_at(group, 0)?, row_at(group, 0x10)?, row_at(group, 0x20)?], alpha: field_14 as u8 })
    }
}

/// Everything per frame that func_003D00F8 reads for one moby instance's draw record (the meshes of one job).
#[derive(Clone, Debug)]
pub struct SkinInputs {
    /// The animated joint matrices: matrix j is the 64 bytes at the scratchpad 0x70000000 + 0x40 * j, row-major 4 x 4 f32 (the
    /// fourth row is the translation), when func_003D00F8 starts. func_L01_00439E00 (called by func_003C5D90, level 1 0x43E998, for
    /// each job just before func_003D00F8 with the same record) computes them from the record's pose buffers (record +0x20 to +0x2C
    /// and +0x30 to +0x3C: the data, quadword count and the two list counts of the instance's +0x58 and +0x5C pose buffers, which hold
    /// a frame as `anim.rs` describes it), the class's bind matrices (record +0x1C, class +0x14) and joint table (record +0x18, class
    /// +0x18), the blend (record +4, instance +0x44) and the instance's +0x50 and +0x54 pointers. They live only in the scratchpad
    /// (0x70000000 to 0x70001BFF); they are not in RAM. A record overwrites the first `joint count` matrices (record +0x10); the others, and
    /// all of them for a class without joints, are what earlier records left (a mesh of a jointless class reads matrix 0 of the
    /// record before it).
    pub joints: Vec<Mat>,
    /// The light section of the draw record.
    pub lights: JobLights,
    /// The second job group's inputs, for a record that has one (record +0x14 not zero).
    pub rigid: Option<RigidInputs>,
    /// The table of (cos, sin) of 2 pi i / 256 that normals' angle indices select: boot_elf's data at 0x2BF5A0 (0x800 bytes, the same
    /// in every level; func_003BE0D0, level 1 0x437EB0, copies it to the scratchpad 0x70003800). The values are not the correctly
    /// rounded f32 of the cosines (199 of the 256 pairs differ in the last bits), so they are read from the game's data.
    pub trig: Vec<[f32; 2]>,
}

impl SkinInputs {
    /// Reads the (cos, sin) table from the 0x800 bytes at RAM 0x2BF5A0.
    pub fn trig_table(b: &[u8]) -> Result<Vec<[f32; 2]>, String> {
        if b.len() < 0x800 {
            return Err(format!("the (cos, sin) table is 0x800 bytes, got {}", b.len()));
        }
        (0..256).map(|i| Ok([f32_at(b, 8 * i)?, f32_at(b, 8 * i + 4)?])).collect()
    }
}

/// The state that func_003D00F8 keeps between the meshes of a job and between jobs (nothing clears it): VU0's data memory (256
/// quadwords, in which packet B's pairs place joint matrices and in which rows place and read blended matrices by address) and the
/// staging array at 0x67E000 (512 records of 16 bytes, in which rows place a vertex that copies read later). A packet's rows read
/// what the packets before them left; start a frame's jobs from `SkinState::new()` in the order the game runs them (list order).
#[derive(Clone, Debug)]
pub struct SkinState {
    /// VU0 data memory in quadwords (VU0 address = 16 * index).
    pub vu0: Vec<Row>,
    /// The staging records: the vertex's position (x, y, z as s16) and colour bytes.
    pub staging: Vec<([i16; 3], [u8; 4])>,
    /// The normal of each staging record's vertex (`Skinned::normals`), which a copy takes with it.
    pub staging_normals: Vec<[f32; 3]>,
}

impl SkinState {
    pub fn new() -> SkinState {
        SkinState { vu0: vec![[0.0; 4]; 256], staging: vec![([0; 3], [0; 4]); 512], staging_normals: vec![[0.0; 3]; 512] }
    }
}

impl Default for SkinState {
    fn default() -> SkinState {
        SkinState::new()
    }
}

/// What the skin job writes for one mesh: the three arrays that VU1 program 0 reads at TOPS + 0 (positions), + 0x61 (colours) and,
/// for the rigid layout, + 0xC2 (texture coordinates). func_003D00F8 DMAs them to the destination of the mesh's job entry as
/// [texture coordinates when rigid][colours when skinned][positions][colours when rigid], each padded to 16 bytes.
#[derive(Clone, Debug, PartialEq)]
pub struct Skinned {
    /// x, y, z as the s16 of the VU1 V3-16 UNPACK.
    pub positions: Vec<[i16; 3]>,
    /// r, g, b, a bytes (the V4-8 UNPACK).
    pub colours: Vec<[u8; 4]>,
    /// s, t as the s16 of the V2-16 UNPACK (1.0 is 4096), for the rigid layout; the skinned layout takes them from packet A.
    pub uvs: Option<Vec<[i16; 2]>>,
    /// Each vertex's normal as the job lights it, in the model's frame (the light directions' frame, `JobLights::dirs`): the
    /// (cos, sin) table's normal through the vertex's matrix rows 0 to 2, not of unit length (the skinned layout's light values
    /// are its dots with the light rows times 1 / its length, the rigid layout's the dots alone). Not uploaded: the per-pixel
    /// lighting's (port/uya `lighting.rs`).
    pub normals: Vec<[f32; 3]>,
}

/// Computes what func_003D00F8 writes for `mesh`'s packet B from the instance's per-frame inputs and the state the packets before it
/// left (see the module doc, "The skin job"). Meshes of the same job go through the same `state` in job-entry order.
pub fn skin(mesh: &Mesh, inputs: &SkinInputs, state: &mut SkinState) -> Result<Skinned, String> {
    skin_as::<true>(mesh, inputs, state)
}

/// `skin` in plain floats: the same matrices, VU0 stores, staging records and normals, without the VU's rounding toward zero and
/// without the colours (all zero, `Skinned::colours`): the positions to within a unit (a value the VU truncates on the other side
/// of a whole number), the normals as the job makes them to a float's last bits. The per-pixel lighting's (port/uya
/// `mobies.rs`), where the exact sums cost too much a frame.
pub fn skin_fast(mesh: &Mesh, inputs: &SkinInputs, state: &mut SkinState) -> Result<Skinned, String> {
    skin_as::<false>(mesh, inputs, state)
}

/// `skin` with the VU's arithmetic (`EXACT`) or plain floats (`skin_fast`).
fn skin_as<const EXACT: bool>(mesh: &Mesh, inputs: &SkinInputs, state: &mut SkinState) -> Result<Skinned, String> {
    if inputs.trig.len() != 256 {
        return Err(format!("the trig table has {} entries", inputs.trig.len()));
    }
    match mesh.packet_b.as_ref().ok_or("mesh record has no packet B")? {
        PacketB::Skinned(sb) => skin_rows::<EXACT>(mesh, sb, inputs, state),
        PacketB::Rigid(rb) => skin_rigid::<EXACT>(rb, inputs),
    }
}

/// The VU's product (`vmul`) or a plain one.
fn mul_as<const EXACT: bool>(a: f32, b: f32) -> f32 {
    if EXACT { vmul(a, b) } else { a * b }
}

/// The VU's sum (`vadd`) or a plain one.
fn add_as<const EXACT: bool>(a: f32, b: f32) -> f32 {
    if EXACT { vadd(a, b) } else { a + b }
}

/// A blend of matrices row by row: m[0] * w[0] + m[1] * w[1] (+ m[2] * w[2]), the chain of VMULA and VMADDA (the VU's
/// arithmetic with `EXACT`).
fn blend_as<const EXACT: bool>(m: &[&Mat], w: &[f32]) -> Mat {
    let mut out = [[0f32; 4]; 4];
    for r in 0..4 {
        for l in 0..4 {
            let mut acc = mul_as::<EXACT>(m[0][r][l], w[0]);
            for k in 1..m.len() {
                acc = add_as::<EXACT>(acc, mul_as::<EXACT>(m[k][r][l], w[k]));
            }
            out[r][l] = acc;
        }
    }
    out
}

/// The vector `v` (x, y, z) times the matrix `m` plus `t` times its fourth row (when `t` is given): the chain of VMULAx, VMADDAy,
/// VMADDAz and VMADDw (the VU's arithmetic with `EXACT`).
fn transform_as<const EXACT: bool>(m: &Mat, v: [f32; 3], t: Option<f32>) -> Row {
    let mut out = [0f32; 4];
    for l in 0..4 {
        let mut acc = mul_as::<EXACT>(m[0][l], v[0]);
        acc = add_as::<EXACT>(acc, mul_as::<EXACT>(m[1][l], v[1]));
        acc = add_as::<EXACT>(acc, mul_as::<EXACT>(m[2][l], v[2]));
        if let Some(t) = t {
            acc = add_as::<EXACT>(acc, mul_as::<EXACT>(m[3][l], t));
        }
        out[l] = acc;
    }
    out
}

fn joint(inputs: &SkinInputs, j: usize) -> Result<&Mat, String> {
    inputs.joints.get(j).ok_or_else(|| format!("joint matrix {j} of {}", inputs.joints.len()))
}

/// The colour byte the skin job makes of a skinned vertex's lit colour word: the low 16 bits of the float's bits as an s16, times
/// 0x80 saturated to 16 bits, shifted right by 7, low byte (`ppach`, `pmulth`, `pmfhl.sh`, `psrah`, `ppacb`).
fn colour_byte(bits: u32) -> u8 {
    let v = (bits & 0xFFFF) as u16 as i16 as i32;
    ((v * 128).clamp(-32768, 32767) >> 7) as u8
}

fn skin_rows<const EXACT: bool>(mesh: &Mesh, sb: &SkinnedB, inputs: &SkinInputs, st: &mut SkinState) -> Result<Skinned, String> {
    let nn = sb.rows.len();
    let vc = nn + sb.copies.len();
    if vc != mesh.vertices as usize {
        return Err(format!("packet B has {vc} vertices, the mesh record {}", mesh.vertices));
    }
    let l = &inputs.lights;
    // VU0 data memory: the joint matrices of packet B's pairs.
    for &(j, a) in &sb.pairs {
        let m = *joint(inputs, j as usize)?;
        for k in 0..4 {
            st.vu0[(a as usize + k) & 0xFF] = m[k];
        }
    }
    let mat_at = |st: &SkinState, a: usize| -> Mat { [st.vu0[a & 0xFF], st.vu0[(a + 1) & 0xFF], st.vu0[(a + 2) & 0xFF], st.vu0[(a + 3) & 0xFF]] };
    let mut positions = Vec::with_capacity(vc);
    let mut colours = Vec::with_capacity(vc);
    let mut normals = Vec::with_capacity(vc);
    let (g1, g2) = (sb.groups[0] as usize, sb.groups[1] as usize);
    let wrap = [l.wrap[0].min(0.0), l.wrap[1].min(0.0), l.wrap[2].min(0.0)];
    for (r, row) in sb.rows.iter().enumerate() {
        let w = |b: u8| b as f32 / 256.0;
        let (b2, b3) = (row.vu0[0] as usize, row.vu0[1] as usize);
        // The matrix the row's vertex uses.
        let m: Mat = if r < g1 {
            // Two joints: blend the matrices at b2 and b3 by weights 0 and 1 into the matrix at b7; the row's own joint matrix goes to b6.
            let (m0, m1) = (mat_at(st, b2), mat_at(st, b3));
            let m = blend_as::<EXACT>(&[&m0, &m1], &[w(row.weights[0]), w(row.weights[1])]);
            let own = *joint(inputs, row.joint as usize)?;
            for k in 0..4 {
                st.vu0[(row.weights[2] as usize + k) & 0xFF] = own[k];
            }
            for k in 0..4 {
                st.vu0[(row.weights[3] as usize + k) & 0xFF] = m[k];
            }
            m
        } else if r < g1 + g2 {
            // Three joints: the matrices at b2, b3 and 2 * joint (the row's joint field is the third address / 2), weights 0 to 2, into b7.
            let (m0, m1, m2) = (mat_at(st, b2), mat_at(st, b3), mat_at(st, 2 * row.joint as usize));
            let m = blend_as::<EXACT>(&[&m0, &m1, &m2], &[w(row.weights[0]), w(row.weights[1]), w(row.weights[2])]);
            for k in 0..4 {
                st.vu0[(row.weights[3] as usize + k) & 0xFF] = m[k];
            }
            m
        } else {
            // One joint: the matrix at b2; the row's own joint matrix goes to b3.
            let m = mat_at(st, b2);
            let own = *joint(inputs, row.joint as usize)?;
            for k in 0..4 {
                st.vu0[(b3 + k) & 0xFF] = own[k];
            }
            m
        };
        let p = [row.pos[0] as f32, row.pos[1] as f32, row.pos[2] as f32];
        let pos = transform_as::<EXACT>(&m, p, Some(1.0));
        let [ca, sa] = inputs.trig[row.lookup[0] as usize];
        let [cb, sb_] = inputs.trig[row.lookup[1] as usize];
        let n = transform_as::<EXACT>(&m, [mul_as::<EXACT>(ca, cb), mul_as::<EXACT>(sa, cb), sb_], None);
        // The lit colour (with the VU's arithmetic only: `skin_fast` makes none).
        let mut c = [0u8; 4];
        if EXACT {
            let sq = [mul_as::<EXACT>(n[0], n[0]), mul_as::<EXACT>(n[1], n[1]), mul_as::<EXACT>(n[2], n[2])];
            let q = vrsqrt1(add_as::<EXACT>(add_as::<EXACT>(sq[0], sq[1]), sq[2]));
            let mut s = [0f32; 3];
            for k in 0..3 {
                // The dot of the k-th light with the normal: vf25 * x + vf26 * y + vf27 * z, then max(d, d * wrap).
                let d = add_as::<EXACT>(add_as::<EXACT>(mul_as::<EXACT>(l.dirs[0][k], n[0]), mul_as::<EXACT>(l.dirs[1][k], n[1])), mul_as::<EXACT>(l.dirs[2][k], n[2]));
                s[k] = vmax(d, mul_as::<EXACT>(d, wrap[k]));
            }
            for k in 0..4 {
                let acc = mul_as::<EXACT>(l.colours[0][k], s[0]);
                let acc = add_as::<EXACT>(acc, mul_as::<EXACT>(l.colours[1][k], s[1]));
                let acc = add_as::<EXACT>(acc, mul_as::<EXACT>(l.colours[2][k], s[2]));
                let lit = add_as::<EXACT>(mul_as::<EXACT>(acc, q), mul_as::<EXACT>(l.base[k], 1.0));
                c[k] = colour_byte(lit.to_bits());
            }
        }
        positions.push([(pos[0] as i32) as i16, (pos[1] as i32) as i16, (pos[2] as i32) as i16]);
        colours.push(c);
        normals.push([n[0], n[1], n[2]]);
    }
    // The staging records: row r's vertex goes to the staging record named by the output slot of the row 7 after it (the encoder
    // compensates the pipeline's delay this way); for the last rows the slots are in the look-ahead rows: the first u16s of the rows
    // up to the last look-ahead row R, then u16 2, 3, ... of R.
    if sb.tail.is_empty() {
        return Err("packet B has no look-ahead row".to_string());
    }
    let look = sb.tail.len();
    for r in 0..nn {
        let j = r + 7;
        let slot = if j < nn {
            sb.rows[j].slot as usize
        } else if j - nn < look {
            (sb.tail[j - nn][0] & 0x1FF) as usize
        } else {
            let lane = j - nn - look + 2;
            if lane > 7 {
                return Err(format!("row {r}: output slot beyond the look-ahead row"));
            }
            (sb.tail[look - 1][lane] & 0x1FF) as usize
        };
        st.staging[slot] = (positions[r], colours[r]);
        st.staging_normals[slot] = normals[r];
    }
    // The vertex copies: copy i is the staging record (entry >> 7).
    for &c in &sb.copies {
        let (p, col) = st.staging[(c >> 7) as usize & 0x1FF];
        positions.push(p);
        colours.push(col);
        normals.push(st.staging_normals[(c >> 7) as usize & 0x1FF]);
    }
    // The last `constant_normals` vertices get the flat colour.
    let cn = sb.constant_normals as usize;
    if cn > vc {
        return Err(format!("{cn} constant colours for {vc} vertices"));
    }
    for c in colours[vc - cn..].iter_mut() {
        *c = l.flat;
    }
    Ok(Skinned { positions, colours, uvs: None, normals })
}

fn skin_rigid<const EXACT: bool>(rb: &RigidB, inputs: &SkinInputs) -> Result<Skinned, String> {
    let g = inputs.rigid.as_ref().ok_or("a rigid packet B needs the second job group's inputs")?;
    let l = &inputs.lights;
    let mut quarter = [l.colours[0], l.colours[1], l.colours[2]];
    for c in quarter.iter_mut() {
        for k in 0..3 {
            c[k] = mul_as::<EXACT>(c[k], 0.25);
        }
    }
    // The rigid colour sum starts at the constant 65536 + 0x70 / 128 for r, g, b and 65536 + 0x80 / 128 for a.
    let base = [f32::from_bits(0x4780_0070), f32::from_bits(0x4780_0070), f32::from_bits(0x4780_0070), f32::from_bits(0x4780_0080)];
    let mut out = Skinned { positions: Vec::new(), colours: Vec::new(), uvs: Some(Vec::new()), normals: Vec::new() };
    let mut uvs = Vec::new();
    for row in &rb.rows {
        let w = |b: u8| b as f32 / 256.0;
        let wt = [w(row.weights[0]), w(row.weights[1]), w(row.weights[2])];
        let m: Mat = match row.joint_count {
            0 | 1 => *joint(inputs, row.joints[0] as usize)?,
            2 => blend_as::<EXACT>(&[joint(inputs, row.joints[0] as usize)?, joint(inputs, row.joints[1] as usize)?], &wt[..2]),
            _ => blend_as::<EXACT>(&[joint(inputs, row.joints[0] as usize)?, joint(inputs, row.joints[1] as usize)?, joint(inputs, row.joints[2] as usize)?], &wt),
        };
        let p = [row.pos[0] as f32, row.pos[1] as f32, row.pos[2] as f32];
        let pos = transform_as::<EXACT>(&m, p, Some(1.0));
        let [ca, sa] = inputs.trig[row.lookup[0] as usize];
        let [cb, sb] = inputs.trig[row.lookup[1] as usize];
        let n = transform_as::<EXACT>(&m, [mul_as::<EXACT>(ca, cb), mul_as::<EXACT>(sa, cb), sb], None);
        // The lit colour (with the VU's arithmetic only: `skin_fast` makes none).
        let mut c = [0u8; 4];
        if EXACT {
            // The light values: max(vf25 * x + vf26 * y + vf27 * z, 0) per lane x, y, z (no wrap factor, no normalisation).
            let mut s = [0f32; 3];
            for k in 0..3 {
                let d = add_as::<EXACT>(add_as::<EXACT>(mul_as::<EXACT>(l.dirs[0][k], n[0]), mul_as::<EXACT>(l.dirs[1][k], n[1])), mul_as::<EXACT>(l.dirs[2][k], n[2]));
                s[k] = vmax(d, 0.0);
            }
            for k in 0..4 {
                let acc = mul_as::<EXACT>(base[k], 1.0);
                let acc = add_as::<EXACT>(acc, mul_as::<EXACT>(quarter[0][k], s[0]));
                let acc = add_as::<EXACT>(acc, mul_as::<EXACT>(quarter[1][k], s[1]));
                let lit = add_as::<EXACT>(acc, mul_as::<EXACT>(quarter[2][k], s[2]));
                c[k] = lit.to_bits() as u8;
            }
        }
        c[3] = g.alpha;
        // The texture coordinates: (dot of the normal with view rows 0 and 1 + 1.0) * 4096 as an s16, shifted right by one.
        let mut uv = [0i16; 2];
        for k in 0..2 {
            let d = add_as::<EXACT>(add_as::<EXACT>(mul_as::<EXACT>(g.view[0][k], n[0]), mul_as::<EXACT>(g.view[1][k], n[1])), mul_as::<EXACT>(g.view[2][k], n[2]));
            let t = (add_as::<EXACT>(d, 1.0) as f64 * 4096.0).trunc() as i32;
            uv[k] = (t as i16) >> 1;
        }
        out.positions.push([(pos[0] as i32) as i16, (pos[1] as i32) as i16, (pos[2] as i32) as i16]);
        out.colours.push(c);
        out.normals.push([n[0], n[1], n[2]]);
        uvs.push(uv);
    }
    out.uvs = Some(uvs);
    Ok(out)
}

/// Decodes a packet B given as bytes (the packet at its RAM address, as many bytes as the draw job's entry's quadword count says).
/// For checks against a RAM dump: `class` decodes the same packets from the disc.
pub fn decode_packet_b(p: &[u8]) -> Result<PacketB, String> {
    let mut t = Tracker::new();
    let mut root = t.root("check", "packet B", p.len() as u64, Space::Unpacked);
    let l = t.line("check", "packet B", "", "", Space::Unpacked);
    let mut n = t.child(&mut root, l, 0, p.len() as u64);
    packet_b(p, p.len(), &mut n)
}

/// A WAD5 armor record as the game loads it: the model block's meshes (`armor`) and the texture block's images in slot order
/// (the block is a u32 count of 1 to 3, three u32 image offsets, then the 2FIP images; func_L01_00422838 reads it into the
/// buffer before the model block, func_L01_00422AB8 builds the slots' TEX0 from it). Entry `entry` of the armor WAD (the
/// global WAD header 4: model block at pair 2 x entry, texture block at pair 2 x entry + 1).
/// The 2FIP image of pair `pair` of the WAD whose header is at `wad` in the table of contents (a skin texture: Clank's, the
/// ship's; `levelcode::SkinTexture`), as its loader reads it and uploads its CLUT (+0x20) and texels (+0x420).
pub fn wad_image(iso_path: &str, wad: u32, pair: usize) -> Result<crate::core::Texture, String> {
    let mut d = crate::disc::Disc::open(iso_path)?;
    let tc = crate::toc::read(&mut d)?;
    let w = tc.wads.iter().find(|w| w.at == wad as usize).ok_or_else(|| format!("the table of contents has no WAD header at {wad:#x}"))?;
    let (s, c) = w.pairs.get(pair).copied().ok_or_else(|| format!("WAD at {wad:#x}: no pair {pair}"))?;
    let b = d.sectors(w.sector + s as u64, c as u64)?;
    crate::pif::texture(&b)
}

pub fn armor_entry(iso_path: &str, entry: usize) -> Result<(Armor, Vec<crate::core::Texture>), String> {
    let mut d = crate::disc::Disc::open(iso_path)?;
    let tc = crate::toc::read(&mut d)?;
    let w = tc.wads.get(4).ok_or("the table of contents has no armor WAD (header 4)")?;
    let pair = |k: usize| w.pairs.get(k).copied().ok_or_else(|| format!("armor WAD entry {entry}: no pair {k}"));
    let ((ms, mc), (ts, tcount)) = (pair(2 * entry)?, pair(2 * entry + 1)?);
    let b = d.sectors(w.sector + ms as u64, mc as u64)?;
    let mut t = Tracker::new();
    let mut root = t.root("armor", &format!("entry {entry}"), b.len() as u64, Space::Disc);
    let armor = armor(&mut t, &mut root, "armor", 0, &b)?.ok_or_else(|| format!("armor WAD entry {entry}: a block in the moby class form"))?;
    let tb = d.sectors(w.sector + ts as u64, tcount as u64)?;
    let count = u32_at(&tb, 0)? as usize;
    if !(1..=3).contains(&count) {
        return Err(format!("armor WAD entry {entry}: texture list count {count}"));
    }
    let offs: Vec<usize> = (0..count).map(|i| u32_at(&tb, 4 + 4 * i).map(|v| v as usize)).collect::<Result<_, _>>()?;
    let textures = offs.iter().map(|&o| crate::pif::texture(tb.get(o..).ok_or("armor texture offset past its block")?)).collect::<Result<Vec<_>, _>>()?;
    Ok((armor, textures))
}

/// The gadget of moby class `id` as a level installs it in its gadget buffer: the class and its one texture. The gadget table
/// (`levelcode::GadgetAddrs::table`, in the table of contents after its WAD header's size and base sector: five-word entries of
/// the class id, the block's sector from the WAD's base and its sector count, the sound bank's sector and count) names the block
/// by the class's id. func_L01_00429C30 reads the block into a slot; func_L01_0042A058 (the same in all 34 story levels) skips a
/// block whose texture stream is empty (+0xC), unpacks the second LZ stream (at the block's +8 less the entry's sector x 0x800: a
/// texture list and 2FIP images, the armor lists' form) into the gadget buffer and uploads its texture's palette and level 0
/// when the list holds one (+0x94; a list of another count uploads nothing), then unpacks the first stream (+0, likewise) over it,
/// a moby class, and loads it (func_L01_00424368) with setup number 0 mapped to that texture.
pub fn gadget(iso_path: &str, table: u32, id: u32) -> Result<(MobyClass, crate::core::Texture), String> {
    let mut d = crate::disc::Disc::open(iso_path)?;
    let tc = crate::toc::read(&mut d)?;
    let w = tc.wads.iter().find(|w| w.at + 8 == table as usize).ok_or_else(|| format!("the table of contents has no WAD header before the gadget table at {table:#x}"))?;
    let mut entry = None;
    for k in 0..(w.size - 8) / 20 {
        if u32_at(&tc.bytes, table as usize + 20 * k)? == id {
            entry = Some(table as usize + 20 * k);
            break;
        }
    }
    let e = entry.ok_or_else(|| format!("no gadget table entry has the moby class id {id:#x}"))?;
    let (sector, sectors) = (u32_at(&tc.bytes, e + 4)? as u64, u32_at(&tc.bytes, e + 8)? as u64);
    let b = d.sectors(w.sector + sector, sectors)?;
    if u32_at(&b, 0xC)? == 0 {
        return Err(format!("gadget {id:#x}: its block has no texture stream, which func_L01_0042A058 does not install"));
    }
    let stream = |k: usize| -> Result<Vec<u8>, String> {
        let o = (u32_at(&b, k)? as u64).checked_sub(sector * crate::disc::SECTOR).ok_or_else(|| format!("gadget {id:#x}: stream offset +{k:#x} before its block"))? as usize;
        Ok(crate::lz::unpack(b.get(o..).ok_or_else(|| format!("gadget {id:#x}: stream at {o:#x} past its block"))?).map_err(|e| format!("gadget {id:#x}: {e}"))?.data)
    };
    let tex = stream(8)?;
    let count = u32_at(&tex, 0)?;
    if count != 1 {
        return Err(format!("gadget {id:#x}: {count} textures, and func_L01_0042A058 uploads a list of one only"));
    }
    let texture = crate::pif::texture(tex.get(u32_at(&tex, 4)? as usize..).ok_or_else(|| format!("gadget {id:#x}: texture offset past its list"))?)?;
    let data = stream(0)?;
    let mut t = Tracker::new();
    let mut root = t.root("gadget", &format!("class {id:#x}"), data.len() as u64, Space::Unpacked);
    Ok((class(&mut t, &mut root, "gadget", 0, &data)?, texture))
}
