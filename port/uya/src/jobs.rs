//! The frame's GS jobs as the DMA sent them: every VIF1 chain (its words in DMA order with the TTE words, the block marks
//! rac-rs's VIF1 takes, and each DMA block's word index, EE source address, quadwords and tag id) and every GIF path 3
//! transfer, in kick order. The DMA has followed the tags and read every block from RAM or scratchpad, so the native frame
//! reads what the GS was sent this frame, including the blocks a RAM dump cannot hold (scratchpad, data the EE rewrote after
//! the kick).
//!
//! `uya play` with UYA_RAM_AT (and the reference's `uyaref frames --ram-dir DIR`) saves them beside each RAM dump as
//! `jobsNNNNN.bin` (`write`); `uya native` reads them (`read`). In play the runtime hands a frame's draw lists over as blocks of
//! the frame's RAM copy (`Kicked`), and their words are read from it on the renderer's side.

/// One job of the frame.
pub enum Job {
    /// A VIF1 DMA chain: words, marks (word index, phase: 0 a block's data, 2 its tag's two VIF words), blocks (word index of
    /// the block's first data word, EE source address with bit 31 for scratchpad, quadwords, tag id; 99 a normal transfer).
    Vif1 { words: Vec<u32>, marks: Vec<(usize, u32)>, blocks: Vec<(usize, u32, u32, u32)>, dispfb: u64 },
    /// A GIF path 3 transfer's quadwords.
    Gif { qs: Vec<u128>, dispfb: u64 },
}

impl Job {
    /// A job as the port's runtime handed it over (`uyart::hw::Job`), a chain's words read from `ram`, the RAM copy it was kicked
    /// with (`uyart::hw::Chain::build`).
    fn of(j: uyart::hw::Job, ram: &uyart::RamCopy) -> Result<Job, String> {
        Ok(match j {
            uyart::hw::Job::Vif1 { words, marks, blocks, dispfb } => Job::Vif1 { words, marks, blocks, dispfb },
            uyart::hw::Job::Gif { qs, dispfb } => Job::Gif { qs, dispfb },
            uyart::hw::Job::Chain(c) => {
                let (words, marks, blocks, dispfb) = c.build(ram)?;
                Job::Vif1 { words, marks, blocks, dispfb }
            }
        })
    }

    /// The same, the runtime's job left as it is.
    fn of_ref(j: &uyart::hw::Job, ram: &uyart::RamCopy) -> Result<Job, String> {
        Ok(match j {
            uyart::hw::Job::Vif1 { words, marks, blocks, dispfb } => Job::Vif1 { words: words.clone(), marks: marks.clone(), blocks: blocks.clone(), dispfb: *dispfb },
            uyart::hw::Job::Gif { qs, dispfb } => Job::Gif { qs: qs.clone(), dispfb: *dispfb },
            uyart::hw::Job::Chain(c) => {
                let (words, marks, blocks, dispfb) = c.build(ram)?;
                Job::Vif1 { words, marks, blocks, dispfb }
            }
        })
    }
}

/// A field's jobs as the port's runtime handed them over (`uyart::hw::Job`, in kick order), its frame's draw lists still in the
/// RAM copy they were kicked with (`uyart::hw::Chain`: `uyart::Machine::frame_ram`, the frame's `ram`). The frame's jobs are
/// built from that copy (`into_frame`): on the renderer's walk thread for a frame it draws (`play::walk_ahead`), off the game
/// thread.
#[derive(Default)]
pub struct Kicked {
    field: u64,
    jobs: Vec<uyart::hw::Job>,
    skin: Vec<(u64, std::sync::Arc<uyart::hw::SkinJoints>)>,
}

impl Kicked {
    pub fn new(field: u64, jobs: Vec<uyart::hw::Job>, skin: Vec<(u64, std::sync::Arc<uyart::hw::SkinJoints>)>) -> Kicked {
        Kicked { field, jobs, skin }
    }

    pub fn is_empty(&self) -> bool {
        self.jobs.is_empty()
    }

    /// The frame's jobs, its draw lists read from `ram`, the RAM copy they were kicked with.
    pub fn into_frame(self, ram: &uyart::RamCopy) -> Result<FrameJobs, String> {
        let jobs = self.jobs.into_iter().map(|j| Job::of(j, ram)).collect::<Result<Vec<Job>, String>>()?;
        Ok(FrameJobs { field: self.field, jobs, skin: self.skin })
    }

    /// The same, these left as they are (for the game thread's own looks at a frame it also hands over: `rtgame`).
    pub fn frame(&self, ram: &uyart::RamCopy) -> Result<FrameJobs, String> {
        let jobs = self.jobs.iter().map(|j| Job::of_ref(j, ram)).collect::<Result<Vec<Job>, String>>()?;
        Ok(FrameJobs { field: self.field, jobs, skin: self.skin.clone() })
    }
}

pub struct FrameJobs {
    pub field: u64,
    pub jobs: Vec<Job>,
    /// The skin job's joint matrices of the last fields, by field, oldest first: the frame's mobies were skinned with one of them
    /// (`uyart::hw::SkinJoints`, `rtgame`, `mobies::SKIN_FIELDS`; none without the per-pixel lighting).
    pub skin: Vec<(u64, std::sync::Arc<uyart::hw::SkinJoints>)>,
}

impl FrameJobs {
    /// The file (little-endian u32 words): "UYAJ", the version, the field, the job count; per job its kind (1 VIF1, 2 GIF) and
    /// DISPFB (2 words); a VIF1 job's word count and words, mark count and marks (2 words), block count and blocks (4 words); a
    /// GIF job's quadword count and quadwords (4 words). Version 3 then holds the skin job's joint matrices: the field count, and
    /// per field its number (2 words), its transfer count and each one's source, quadword count and byte offset (3 words), the byte
    /// count and bytes, the (cos, sin) table's byte count and bytes. Version 4 appends the captured-output flag and output count;
    /// per output: record index, RAM destination, byte count, normal count, transfer bytes and normals (three f32s each).
    /// Captured fields omit the obsolete joint-matrix prefix and trig table; uncaptured fields retain the v3 input layout.
    pub fn write(&self, path: &str) -> Result<(), String> {
        let mut o: Vec<u32> = vec![u32::from_le_bytes(*b"UYAJ"), 4, self.field as u32, self.jobs.len() as u32];
        for j in &self.jobs {
            match j {
                Job::Vif1 { words, marks, blocks, dispfb } => {
                    o.extend([1, *dispfb as u32, (*dispfb >> 32) as u32, words.len() as u32]);
                    o.extend_from_slice(words);
                    o.push(marks.len() as u32);
                    o.extend(marks.iter().flat_map(|&(i, p)| [i as u32, p]));
                    o.push(blocks.len() as u32);
                    o.extend(blocks.iter().flat_map(|&(i, s, q, id)| [i as u32, s, q, id]));
                }
                Job::Gif { qs, dispfb } => {
                    o.extend([2, *dispfb as u32, (*dispfb >> 32) as u32, qs.len() as u32]);
                    o.extend(qs.iter().flat_map(|&q| [q as u32, (q >> 32) as u32, (q >> 64) as u32, (q >> 96) as u32]));
                }
            }
        }
        o.push(self.skin.len() as u32);
        for (f, s) in &self.skin {
            o.extend([*f as u32, (*f >> 32) as u32, s.records.len() as u32]);
            o.extend(s.records.iter().flat_map(|&(a, q, at)| [a, q, at as u32]));
            for b in [&s.bytes, &s.trig] {
                o.push(b.len() as u32);
                o.extend(b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes"))));
            }
            o.extend([s.captured as u32, s.outputs.len() as u32]);
            for output in &s.outputs {
                o.extend([output.record as u32, output.dest, output.bytes.len() as u32, output.normals.len() as u32]);
                o.extend(output.bytes.chunks_exact(4).map(|b| u32::from_le_bytes(b.try_into().expect("4 bytes"))));
                o.extend(output.normals.iter().flatten().map(|v| v.to_bits()));
            }
        }
        std::fs::write(path, o.iter().flat_map(|w| w.to_le_bytes()).collect::<Vec<u8>>()).map_err(|e| format!("{path}: {e}"))
    }

    pub fn read(path: &str) -> Result<FrameJobs, String> {
        let b = std::fs::read(path).map_err(|e| format!("{path}: {e}"))?;
        let w: Vec<u32> = b.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().expect("4 bytes"))).collect();
        let mut i = 0;
        let mut next = |n: usize| -> Result<&[u32], String> {
            let s = w.get(i..i + n).ok_or_else(|| format!("{path}: ends inside a job"))?;
            i += n;
            Ok(s)
        };
        let head = next(4)?;
        let version = head[1];
        if head[0] != u32::from_le_bytes(*b"UYAJ") || !matches!(version, 1 | 3 | 4) {
            return Err(format!("{path}: not a version 1, 3 or 4 jobs file"));
        }
        let (field, count) = (head[2] as u64, head[3] as usize);
        let mut jobs = Vec::with_capacity(count);
        for _ in 0..count {
            let h = next(4)?.to_vec();
            let dispfb = h[1] as u64 | (h[2] as u64) << 32;
            let n = h[3] as usize;
            jobs.push(match h[0] {
                1 => {
                    let words = next(n)?.to_vec();
                    let m = next(1)?[0] as usize;
                    let marks = next(2 * m)?.chunks_exact(2).map(|c| (c[0] as usize, c[1])).collect();
                    let k = next(1)?[0] as usize;
                    let blocks = next(4 * k)?.chunks_exact(4).map(|c| (c[0] as usize, c[1], c[2], c[3])).collect();
                    Job::Vif1 { words, marks, blocks, dispfb }
                }
                2 => Job::Gif { qs: next(4 * n)?.chunks_exact(4).map(|c| c[0] as u128 | (c[1] as u128) << 32 | (c[2] as u128) << 64 | (c[3] as u128) << 96).collect(), dispfb },
                k => return Err(format!("{path}: job kind {k}")),
            });
        }
        // Version 1 files were written before the skin job's joint matrices were kept: none.
        let mut skin = Vec::new();
        if version >= 3 {
            for _ in 0..next(1)?[0] {
                let h = next(3)?.to_vec();
                let mut s = uyart::hw::SkinJoints { records: next(3 * h[2] as usize)?.chunks_exact(3).map(|c| (c[0], c[1], c[2] as usize)).collect(), ..Default::default() };
                for b in [&mut s.bytes, &mut s.trig] {
                    let len = next(1)?[0] as usize;
                    *b = next(len / 4)?.iter().flat_map(|w| w.to_le_bytes()).collect();
                }
                if version >= 4 {
                    let h = next(2)?.to_vec();
                    s.captured = h[0] != 0;
                    for _ in 0..h[1] {
                        let h = next(4)?.to_vec();
                        let bytes = next(h[2] as usize / 4)?.iter().flat_map(|w| w.to_le_bytes()).collect();
                        let normals = next(h[3] as usize * 3)?.chunks_exact(3).map(|v| [f32::from_bits(v[0]), f32::from_bits(v[1]), f32::from_bits(v[2])]).collect();
                        s.outputs.push(uyart::hw::SkinOutput { record: h[0] as usize, dest: h[1], bytes, normals });
                    }
                }
                skin.push((h[0] as u64 | (h[1] as u64) << 32, std::sync::Arc::new(s)));
            }
        }
        Ok(FrameJobs { field, jobs, skin })
    }
}
