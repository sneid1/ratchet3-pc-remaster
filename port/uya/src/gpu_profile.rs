//! Live pass timings (`UYA_GPU_PASSES`), independent of the blocking offline RT timer.
//! Eight reusable query/readback slots; results are collected at least three submissions later.
//! If all slots are busy, skip a sample. Profiling never waits for the GPU or changes its pace.

use std::{cell::RefCell, collections::{BTreeMap, VecDeque}, sync::mpsc};

const QUERIES: u32 = wgpu::QUERY_SET_MAX_QUERIES;
const SLOTS: usize = 8;

pub(crate) fn features() -> wgpu::Features { crate::raytrace::times_features() | wgpu::Features::TIMESTAMP_QUERY_INSIDE_PASSES }

pub(crate) fn wanted() -> bool { std::env::var_os("UYA_GPU_PASSES").is_some() }

struct Slot {
    queries: wgpu::QuerySet,
    resolve: wgpu::Buffer,
    read: wgpu::Buffer,
}

pub(crate) struct Recording {
    slot: Slot,
    spans: RefCell<Vec<(&'static str, u32)>>,
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
            Slot {
                queries: device.create_query_set(&wgpu::QuerySetDescriptor { label: Some("live GPU passes"), ty: wgpu::QueryType::Timestamp, count: QUERIES }),
                resolve: device.create_buffer(&wgpu::BufferDescriptor { label: Some("live GPU resolve"), size: QUERIES as u64 * 8, usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC, mapped_at_creation: false }),
                read: device.create_buffer(&wgpu::BufferDescriptor { label: Some("live GPU readback"), size: QUERIES as u64 * 8, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }),
            }
        } else { return None; };
        Some(Recording { slot, spans: RefCell::new(Vec::new()), field, first, alpha, size })
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
                    }
                    Err(e) => eprintln!("GPU passes: field {} mapped range: {e}", r.field),
                }
            }
            r.slot.read.unmap();
            self.free.push(r.slot);
        }
    }
}
