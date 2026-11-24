//! Threads kept for the render thread's loops over many items (`run`): the loop's chunks are shared out between the calling thread
//! and `HELPERS` threads, each taking the next chunk not yet taken. The caller waits only for the chunks the helpers have taken: a
//! helper that wakes after every chunk is taken takes none, so a busy machine never holds the frame back for a thread that has not
//! run yet (the frame pool's queue, `native::Pool`, would: its threads may be in the frame's ties). Level 9's sea gives the EE's
//! primitives pass 20,000 triangles a frame (`eedraw::build`): their world positions and vertices are made here.

use std::sync::{Condvar, Mutex, OnceLock};

/// The threads besides the caller's: the machine runs the game's EE thread, the render, record and present threads beside them.
const HELPERS: usize = 4;

/// A loop `run` shares out: the work for chunk k, how many chunks, the next not taken, and how many the helpers are running.
struct Job {
    work: *const (dyn Fn(usize) + Sync),
    chunks: usize,
    next: usize,
    running: usize,
    panic: Option<Box<dyn std::any::Any + Send>>,
}

// SAFETY: `work` points at a `Sync` closure that `run` keeps alive until no helper holds or can take a chunk of its job.
unsafe impl Send for Job {}

struct Crew {
    job: Mutex<Option<Job>>,
    /// A job given (helpers wait on it), and a helper's chunk finished (`run` waits on it).
    given: Condvar,
    finished: Condvar,
}

fn crew() -> &'static Crew {
    static CREW: OnceLock<&'static Crew> = OnceLock::new();
    CREW.get_or_init(|| {
        let c: &'static Crew = Box::leak(Box::new(Crew { job: Mutex::new(None), given: Condvar::new(), finished: Condvar::new() }));
        for k in 0..HELPERS {
            std::thread::Builder::new().name(format!("uya crew {k}")).spawn(move || helper(c)).expect("a crew thread");
        }
        c
    })
}

fn helper(c: &'static Crew) {
    let mut job = c.job.lock().expect("the crew's job");
    loop {
        match job.as_mut() {
            Some(j) if j.next < j.chunks => {
                let (k, work) = (j.next, j.work);
                j.next += 1;
                j.running += 1;
                drop(job);
                // SAFETY: the job is in the slot and this chunk taken, so `run` has not returned (it waits for `running` 0).
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| unsafe { (*work)(k) }));
                job = c.job.lock().expect("the crew's job");
                let j = job.as_mut().expect("a job stays until its chunks have run");
                j.running -= 1;
                if let Err(p) = r {
                    j.panic.get_or_insert(p);
                }
                if j.running == 0 && j.next >= j.chunks {
                    c.finished.notify_all();
                }
            }
            _ => job = c.given.wait(job).expect("the crew's job"),
        }
    }
}

/// `work(k)` for each chunk k below `chunks`, on the calling thread and the crew's, in any order; returns once every one has run
/// (a panic in one is resumed here).
pub(crate) fn run(chunks: usize, work: &(dyn Fn(usize) + Sync)) {
    if chunks <= 1 {
        (0..chunks).for_each(work);
        return;
    }
    let c = crew();
    // SAFETY: the job leaves the slot below, before `run` returns, once no helper is running a chunk of it; a helper takes a
    // chunk only from the slot.
    let work_static: *const (dyn Fn(usize) + Sync + 'static) = unsafe { std::mem::transmute::<*const (dyn Fn(usize) + Sync + '_), *const (dyn Fn(usize) + Sync + 'static)>(work) };
    {
        let mut job = c.job.lock().expect("the crew's job");
        assert!(job.is_none(), "crew::run from two threads at once");
        *job = Some(Job { work: work_static, chunks, next: 0, running: 0, panic: None });
    }
    c.given.notify_all();
    let mut job = c.job.lock().expect("the crew's job");
    loop {
        let j = job.as_mut().expect("the caller's job");
        if j.next < j.chunks {
            let k = j.next;
            j.next += 1;
            drop(job);
            work(k);
            job = c.job.lock().expect("the crew's job");
        } else if j.running > 0 {
            job = c.finished.wait(job).expect("the crew's job");
        } else {
            let panic = job.take().expect("the caller's job").panic;
            drop(job);
            if let Some(p) = panic {
                std::panic::resume_unwind(p);
            }
            return;
        }
    }
}
