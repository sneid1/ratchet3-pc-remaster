//! The pan slide: command 0x21 with bit 2 moves a sound's or a stream's pan angle to a target by itself over some ticks (989snd
//! FUN_00002BE4 sets it up, FUN_0000ED4C runs it each tick from FUN_0000EC6C, FUN_00002F18 is its step). The game uses it for what
//! moves round the listener: it sends the angle each field and the slide carries the pan between.
//!
//! The setup takes the target, the way round (0: the shorter) and a time, which for this command is 30 ticks for half a circle and
//! less for less (`30 * distance / 180`); no time, or a distance too short for one, sets the pan at once. A slide longer in time
//! than in degrees moves a degree every `time / distance` ticks; else it moves by `distance * 32 / time` of 1/32 degree a tick. The
//! pan is set through the sound's volume-and-pan call with the volume left alone, so everything a pan change does follows.

use super::{stream, Engine};

/// A slide running on a sound or stream.
#[derive(Clone, Copy)]
pub struct Slide {
    target: i32,
    /// A degree a step (-1 or 1) every `reload` ticks (`once`), else this many 1/32 degrees every tick.
    step: i32,
    once: bool,
    counter: i32,
    reload: i32,
}

impl Engine {
    /// The pan angle of a sound or stream (the record's +0x12): the sound's angle and offset, or the stream's.
    fn pan_of(&self, handle: u32) -> Option<i32> {
        if stream::is_stream(handle) {
            return self.stream_pan(handle);
        }
        let k = self.index_of(handle)?;
        Some((self.insts[k].pan_base + self.insts[k].pan_off).rem_euclid(360))
    }

    fn slide_of(&mut self, handle: u32) -> Option<&mut Option<Slide>> {
        if stream::is_stream(handle) {
            return self.stream_slide(handle);
        }
        let k = self.index_of(handle)?;
        Some(&mut self.insts[k].slide)
    }

    /// 989snd FUN_00002BE4 with mode 4: slides `handle`'s pan to `target`, the long way round by `dir` (0: the shorter) taking
    /// `time` ticks for half a circle.
    pub(super) fn pan_slide(&mut self, handle: u32, target: i32, dir: i32, time: i32) {
        let Some(cur) = self.pan_of(handle) else { return };
        // The distance the slide goes: forward (dir > 0, or 0), back (dir < 0), and for 0 the shorter way.
        let mut d = target - cur;
        if dir >= 0 && d < 0 {
            d += 360;
        }
        if dir < 0 && d > 0 {
            d -= 360;
        }
        if dir == 0 && d.abs() > 180 {
            d -= 360;
        }
        let time = time * d.abs() / 180;
        if time == 0 {
            if let Some(s) = self.slide_of(handle) {
                *s = None;
            }
            self.set_volume_pan(handle, 0x7fff_ffff, target);
            return;
        }
        let slide = if d == 0 {
            None
        } else if d.abs() < time {
            let period = (time / d).abs();
            Some(Slide { target, step: d.signum(), once: true, counter: period, reload: period })
        } else {
            Some(Slide { target, step: (d << 5) / time, once: false, counter: 0, reload: 0 })
        };
        if let Some(s) = self.slide_of(handle) {
            *s = slide;
        }
    }

    /// One tick of the slide of `handle` (989snd FUN_0000ED4C, FUN_00002F18).
    pub(super) fn slide_run(&mut self, handle: u32) {
        let Some(mut s) = self.slide_of(handle).and_then(|s| *s) else { return };
        if s.once {
            s.counter -= 1;
        }
        if s.counter != 0 {
            if let Some(slot) = self.slide_of(handle) {
                *slot = Some(s);
            }
            return;
        }
        let Some(cur) = self.pan_of(handle) else { return };
        let t = s.target;
        // The step: a degree, or 1/32 degrees (the module takes the sum as a 16-bit value).
        let x = if s.step.abs() < 2 { (cur + s.step) as i16 as i32 } else { ((cur * 0x20 + s.step) * 0x800) >> 16 };
        // The target as the step meets it, a circle either side when the step has gone round.
        let end = if x < 360 || cur <= t {
            if x < 0 && cur < t {
                t - 360
            } else {
                t
            }
        } else {
            t + 360
        };
        let passed = (s.step >= 1 && x > end && end > cur) || (s.step < 0 && x < end && end < cur);
        let to = if passed {
            t
        } else if x < 0 {
            x + 360
        } else if x >= 360 {
            x - 360
        } else {
            x
        };
        self.set_volume_pan(handle, 0x7fff_ffff, to);
        let slot = self.slide_of(handle).expect("the sound is still there");
        if to == t {
            *slot = None;
        } else {
            if s.once {
                s.counter = s.reload;
            }
            *slot = Some(s);
        }
    }
}
