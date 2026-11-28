//! Live pass timings (`UYA_GPU_PASSES`), independent of the blocking offline RT timer.
//! Eight reusable query/readback slots; results are collected at least three submissions later.
//! If all slots are busy, skip a sample. Profiling never waits for the GPU or changes its pace.
//!
//! With `UYA_GPU_COUNTS` as well, each timed render pass also counts what it ran (a pipeline statistics query: vertex shader
//! invocations, primitives out of the clipper, fragment shader invocations) and its draw calls, printed per picture as
//! "GPU counts": numbers that do not depend on the GPU that ran them, where milliseconds on one GPU say little about another.

use std::{cell::{Cell, RefCell}, collections::{BTreeMap, VecDeque}, sync::mpsc};

const QUERIES: u32 = wgpu::QUERY_SET_MAX_QUERIES;
const SLOTS: usize = 8;
/// The pipeline statistics queries a picture can hold, and what each counts (three u64s, in the flags' bit order).
const COUNTS: u32 = 1024;
const STATS: wgpu::PipelineStatisticsTypes = wgpu::PipelineStatisticsTypes::VERTEX_SHADER_INVOCATIONS
    .union(wgpu::PipelineStatisticsTypes::CLIPPER_PRIMITIVES_OUT)
    .union(wgpu::PipelineStatisticsTypes::FRAGMENT_SHADER_INVOCATIONS);
const STAT_BYTES: u64 = 3 * 8;

pub(crate) fn features() -> wgpu::Features {
    crate::raytrace::times_features() | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES | if counts_wanted() { wgpu::Features::PIPELINE_STATISTICS_QUERY } else { wgpu::Features::empty() }
}

pub(crate) fn wanted() -> bool { std::env::var_os("UYA_GPU_PASSES").is_some() }

/// UYA_GPU_COUNTS (with UYA_GPU_PASSES): the passes' counts as well as their times.
pub(crate) fn counts_wanted() -> bool { wanted() && std::env::var_os("UYA_GPU_COUNTS").is_some() }

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
    /// The pipeline statistics queries and their resolve buffer (copied into `read` after the timestamps), with UYA_GPU_COUNTS.
    stats: Option<(wgpu::QuerySet, wgpu::Buffer)>,
}

pub(crate) struct Recording {
    slot: Slot,
    spans: RefCell<Vec<(&'static str, u32)>>,
    /// The counted passes in order (a pass split in parts counts each part under its name), and the draw calls by name.
    counted: RefCell<Vec<&'static str>>,
    draws: RefCell<BTreeMap<&'static str, u64>>,
    counting: Cell<bool>,
    field: u64,
    first: bool,
    alpha: f32,
    size: (u32, u32),
}

pub(crate) struct Span<'a>(Option<(&'a Recording, u32)>);
impl<'a> Span<'a> {
    pub fn begin(recording: Option<&'a Recording>, enc: &mut wgpu::CommandEncoder, name: &'static str) -> Self {
        let Some(r) = recording else { return Self(None) };
        let mut spans = r.spans.borrow_mut();
        let index = spans.len() as u32 * 2;
        assert!(index + 1 < QUERIES, "live GPU profiling query capacity exceeded");
        spans.push((name, index));
        enc.write_timestamp(&r.slot.queries, index);
        Self(Some((r, index)))
    }
    pub fn begin_render(recording: Option<&'a Recording>, pass: &mut wgpu::RenderPass<'_>, name: &'static str) -> Self {
        let Some(r) = recording else { return Self(None) };
        let mut spans = r.spans.borrow_mut();
        let index = spans.len() as u32 * 2;
        assert!(index + 1 < QUERIES, "live GPU profiling query capacity exceeded");
        spans.push((name, index));
        pass.write_timestamp(&r.slot.queries, index);
        Self(Some((r, index)))
    }
    pub fn end_render(self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some((r, i)) = self.0 { pass.write_timestamp(&r.slot.queries, i + 1); }
    }
    pub fn begin_compute(recording: Option<&'a Recording>, pass: &mut wgpu::ComputePass<'_>, name: &'static str) -> Self {
        let Some(r) = recording else { return Self(None) };
        let mut spans = r.spans.borrow_mut();
        let index = spans.len() as u32 * 2;
        assert!(index + 1 < QUERIES, "live GPU profiling query capacity exceeded");
        spans.push((name, index));
        pass.write_timestamp(&r.slot.queries, index);
        Self(Some((r, index)))
    }
    pub fn end_compute(self, pass: &mut wgpu::ComputePass<'_>) {
        if let Some((r, i)) = self.0 { pass.write_timestamp(&r.slot.queries, i + 1); }
    }
    pub fn none() -> Self { Self(None) }
    pub fn end(self, enc: &mut wgpu::CommandEncoder) {
        if let Some((r, i)) = self.0 { enc.write_timestamp(&r.slot.queries, i + 1); }
    }
}

impl Recording {
    pub fn resolve(&self, enc: &mut wgpu::CommandEncoder) {
        let count = self.spans.borrow().len() as u32 * 2;
        enc.resolve_query_set(&self.slot.queries, 0..count, &self.slot.resolve, 0);
        enc.copy_buffer_to_buffer(&self.slot.resolve, 0, &self.slot.read, 0, count as u64 * 8);
        let counted = self.counted.borrow().len() as u32;
        if let (Some((set, resolve)), true) = (&self.slot.stats, counted > 0) {
            enc.resolve_query_set(set, 0..counted, resolve, 0);
            enc.copy_buffer_to_buffer(resolve, 0, &self.slot.read, QUERIES as u64 * 8, counted as u64 * STAT_BYTES);
        }
    }
    /// With UYA_GPU_COUNTS, the counts of `pass` from here to `count_end` go to `name` (none is open: the passes do not nest).
    pub fn count_begin(&self, pass: &mut wgpu::RenderPass<'_>, name: &'static str) {
        let Some((set, _)) = &self.slot.stats else { return };
        let mut counted = self.counted.borrow_mut();
        assert!((counted.len() as u32) < COUNTS, "live GPU counts: query capacity exceeded");
        assert!(!self.counting.get(), "live GPU counts: a count begun inside another");
        pass.begin_pipeline_statistics_query(set, counted.len() as u32);
        counted.push(name);
        self.counting.set(true);
    }
    pub fn count_end(&self, pass: &mut wgpu::RenderPass<'_>) {
        if self.counting.replace(false) {
            pass.end_pipeline_statistics_query();
        }
    }
    /// `n` draw calls under `name`.
    pub fn count_draws(&self, name: &'static str, n: u64) {
        if self.slot.stats.is_some() {
            *self.draws.borrow_mut().entry(name).or_default() += n;
        }
    }
}

struct Pending {
    recording: Recording,
    ready: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
    sequence: u64,
}

pub(crate) struct Profiler {
    free: Vec<Slot>,
    pending: VecDeque<Pending>,
    made: usize,
    sequence: u64,
    period: f64,
}

impl Profiler {
    pub fn new() -> Option<Self> {
        wanted().then(|| Self { free: Vec::new(), pending: VecDeque::new(), made: 0, sequence: 0, period: 0.0 })
    }
    pub fn begin(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, field: u64, first: bool, alpha: f32, size: (u32, u32)) -> Option<Recording> {
        if !device.features().contains(features()) { return None; }
        self.collect();
        self.period = queue.get_timestamp_period() as f64;
        let slot = if let Some(slot) = self.free.pop() { slot } else if self.made < SLOTS {
            self.made += 1;
            let stats = counts_wanted().then(|| (
                device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("live GPU counts"), ty: wgpu::QueryType::PipelineStatistics(STATS), count: COUNTS }),
                device.create_buffer(&wgpu::BufferDescriptor { label: Some("live GPU counts resolve"), size: COUNTS as u64 * STAT_BYTES, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }),
            ));
            Slot {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("live GPU passes"), ty: wgpu::QueryType::Timestamp, count: QUERIES }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor { label: Some("live GPU resolve"), size: QUERIES as u64 * 8, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }),
                read: device.create_buffer(&wgpu::BufferDescriptor { label: Some("live GPU readback"), size: QUERIES as u64 * 8 + if stats.is_some() { COUNTS as u64 * STAT_BYTES } else { 0 }, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
                stats,
            }
        } else { return None; };
        Some(Recording { slot, spans: RefCell::new(Vec::new()), counted: RefCell::new(Vec::new()), draws: RefCell::new(BTreeMap::new()), counting: Cell::new(false), field, first, alpha, size })
    }
    /// Called only after the picture's existing submission, with no additional submission or wait.
    pub fn submitted(&mut self, recording: Recording) {
        let (tx, ready) = mpsc::channel();
        recording.slot.read.slice(..).map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
        self.sequence += 1;
        self.pending.push_back(Pending { recording, ready, sequence: self.sequence });
    }
    pub fn collect(&mut self) { self.collect_after(3); }
    /// `uya native`: waits for the picture just submitted and prints its passes, so every redraw of a saved frame has its row.
    pub fn flush(&mut self, device: &wgpu::Device) -> Result<(), String> {
        device.poll(wgpu::PollType::wait_indefinitely()).map_err(|e| format!("GPU passes: wait: {e}"))?;
        self.collect_after(0);
        Ok(())
    }
    fn collect_after(&mut self, lag: u64) {
        while let Some(p) = self.pending.front() {
            if self.sequence < p.sequence + lag { break; }
            let result = match p.ready.try_recv() {
                Ok(r) => r,
                Err(mpsc::TryRecvError::Empty) => break,
                Err(e) => { eprintln!("GPU passes: readback callback: {e}"); break; }
            };
            let p = self.pending.pop_front().expect("front checked");
            let r = p.recording;
            if let Err(e) = result {
                eprintln!("GPU passes: field {} readback: {e}", r.field);
            } else {
                match r.slot.read.slice(..).get_mapped_range() {
                    Ok(bytes) => {
                        let stamps: &[u64] = bytemuck::cast_slice(&bytes);
                        let mut totals = BTreeMap::<&str, f64>::new();
                        for &(name, i) in r.spans.borrow().iter() {
                            *totals.entry(name).or_default() += stamps[i as usize + 1].wrapping_sub(stamps[i as usize]) as f64 * self.period / 1e6;
                        }
                        let times = totals.iter().map(|(n, t)| format!(" {n}={t:.4}")).collect::<String>();
                        println!("GPU passes: field {} {} alpha {:.6} size {}x{} ms{}", r.field, if r.first { "first" } else { "repeat" }, r.alpha, r.size.0, r.size.1, times);
                        if r.slot.stats.is_some() {
                            // Per name: draw calls, vertex shader invocations, primitives out of the clipper, fragment shader
                            // invocations, and those over the picture's pixels.
                            let values: &[u64] = bytemuck::cast_slice(&bytes[QUERIES as usize * 8..]);
                            let mut counts = BTreeMap::<&str, [u64; 4]>::new();
                            for (k, &name) in r.counted.borrow().iter().enumerate() {
                                let c = counts.entry(name).or_default();
                                for j in 0..3 {
                                    c[j + 1] += values[3 * k + j];
                                }
                            }
                            for (&name, &n) in r.draws.borrow().iter() {
                                counts.entry(name).or_default()[0] += n;
                            }
                            let pixels = r.size.0 as f64 * r.size.1 as f64;
                            let all = counts.values().fold([0u64; 4], |a, c| [a[0] + c[0], a[1] + c[1], a[2] + c[2], a[3] + c[3]]);
                            let line = counts.iter().chain(std::iter::once((&"all", &all))).map(|(n, c)| format!(" {n}={}/{}/{}/{} ({:.2}/px)", c[0], c[1], c[2], c[3], c[3] as f64 / pixels)).collect::<String>();
                            println!("GPU counts: field {} {} size {}x{} draws/vertices/primitives/fragments{line}", r.field, if r.first { "first" } else { "repeat" }, r.size.0, r.size.1);
                        }
                    }
                    Err(e) => eprintln!("GPU passes: field {} mapped range: {e}", r.field),
                }
            }
            r.slot.read.unmap();
            self.free.push(r.slot);
        }
    }
}
