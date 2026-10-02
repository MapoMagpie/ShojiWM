//! Time: easing curves, animated values and timers.
//!
//! The compositor drives time. Every request carries its timestamp, and while
//! something animates the config answers with `next_poll_in_ms = 0`, so the
//! compositor ticks it once per frame and [`Animation`] values advance at
//! the frame's presentation time.
//!
//! ```
//! use shojiwm_rs::prelude::*;
//!
//! let scope = Scope::root();
//! let open = scope.run(|| Animation::new(0.0));
//! open.start(AnimationOptions::to(1.0, 200.0).easing(Easing::EASE_OUT_CUBIC));
//! let scale = scope.memo(move || 0.8 + open.get() * 0.2);
//! # let _ = scale;
//! ```

use std::{cell::RefCell, rc::Rc};

use shojiwm_lib::ssd::ManagedWindowAnimationEasingSnapshot;

use crate::reactive::{Memo, Prop, ReadSignal, Signal, on_cleanup, signal};

/// An easing curve mapping progress `0..=1` to eased progress.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Easing {
    Linear,
    /// CSS `cubic-bezier(x1, y1, x2, y2)`.
    CubicBezier(f64, f64, f64, f64),
}

impl Easing {
    pub const EASE: Self = Self::CubicBezier(0.25, 0.1, 0.25, 1.0);
    pub const EASE_IN: Self = Self::CubicBezier(0.42, 0.0, 1.0, 1.0);
    pub const EASE_OUT: Self = Self::CubicBezier(0.0, 0.0, 0.58, 1.0);
    pub const EASE_IN_OUT: Self = Self::CubicBezier(0.42, 0.0, 0.58, 1.0);
    pub const EASE_OUT_CUBIC: Self = Self::CubicBezier(0.22, 1.0, 0.36, 1.0);
    pub const EASE_IN_OUT_CUBIC: Self = Self::CubicBezier(0.65, 0.0, 0.35, 1.0);

    pub fn apply(self, progress: f64) -> f64 {
        let progress = clamp_unit(progress);
        match self {
            Self::Linear => progress,
            Self::CubicBezier(x1, y1, x2, y2) => {
                if progress == 0.0 || progress == 1.0 {
                    return progress;
                }
                cubic_bezier_y(x1, y1, x2, y2, progress)
            }
        }
    }
}

impl From<Easing> for ManagedWindowAnimationEasingSnapshot {
    fn from(easing: Easing) -> Self {
        match easing {
            Easing::Linear => Self::Linear,
            Easing::CubicBezier(x1, y1, x2, y2) => Self::CubicBezier { x1, y1, x2, y2 },
        }
    }
}

/// `cubicBezier(x1, y1, x2, y2)`.
pub const fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64) -> Easing {
    Easing::CubicBezier(x1, y1, x2, y2)
}

fn clamp_unit(value: f64) -> f64 {
    if !value.is_finite() {
        0.0
    } else {
        value.clamp(0.0, 1.0)
    }
}

fn cubic_bezier_y(x1: f64, y1: f64, x2: f64, y2: f64, x: f64) -> f64 {
    let cx = 3.0 * x1;
    let bx = 3.0 * (x2 - x1) - cx;
    let ax = 1.0 - cx - bx;
    let cy = 3.0 * y1;
    let by = 3.0 * (y2 - y1) - cy;
    let ay = 1.0 - cy - by;
    let sample_x = |t: f64| ((ax * t + bx) * t + cx) * t;
    let sample_y = |t: f64| ((ay * t + by) * t + cy) * t;
    let derivative_x = |t: f64| (3.0 * ax * t + 2.0 * bx) * t + cx;

    let solve_x = || {
        let mut t = x;
        for _ in 0..8 {
            let estimate = sample_x(t) - x;
            if estimate.abs() < 1e-6 {
                return t;
            }
            let derivative = derivative_x(t);
            if derivative.abs() < 1e-6 {
                break;
            }
            t -= estimate / derivative;
        }
        let (mut lower, mut upper) = (0.0, 1.0);
        t = x;
        for _ in 0..12 {
            let estimate = sample_x(t);
            if (estimate - x).abs() < 1e-7 {
                return t;
            }
            if x > estimate {
                lower = t;
            } else {
                upper = t;
            }
            t = (upper - lower) * 0.5 + lower;
        }
        t
    };
    sample_y(solve_x())
}

/// How a running animation repeats.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Repeat {
    Loop,
    PingPong,
}

/// Parameters of [`Animation::start`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AnimationOptions {
    pub duration_ms: f64,
    /// Start value; the current value when `None`.
    pub from: Option<f64>,
    pub to: f64,
    pub easing: Easing,
    pub repeat: Option<Repeat>,
}

impl AnimationOptions {
    /// Animate from the current value to `to` over `duration_ms`.
    pub fn to(to: f64, duration_ms: f64) -> Self {
        Self {
            duration_ms,
            from: None,
            to,
            easing: Easing::Linear,
            repeat: None,
        }
    }

    pub fn from(mut self, from: f64) -> Self {
        self.from = Some(from);
        self
    }

    pub fn easing(mut self, easing: Easing) -> Self {
        self.easing = easing;
        self
    }

    pub fn repeat(mut self, repeat: Repeat) -> Self {
        self.repeat = Some(repeat);
        self
    }
}

struct Timeline {
    started_at_ms: f64,
    duration_ms: f64,
    from: f64,
    to: f64,
    easing: Easing,
    repeat: Option<Repeat>,
}

struct AnimationEntry {
    value: Signal<f64>,
    timeline: Option<Timeline>,
}

#[derive(Default)]
struct Clock {
    now_ms: f64,
    animations: Vec<Option<Rc<RefCell<AnimationEntry>>>>,
    timers: Vec<Timer>,
    next_timer_id: u64,
}

struct Timer {
    id: u64,
    due_ms: f64,
    interval_ms: Option<f64>,
    callback: Rc<dyn Fn()>,
}

thread_local! {
    static CLOCK: RefCell<Clock> = RefCell::new(Clock::default());
}

pub(crate) fn reset_clock() {
    CLOCK.with(|clock| *clock.borrow_mut() = Clock::default());
}

/// The compositor time of the request being handled, in milliseconds.
pub fn now_ms() -> f64 {
    CLOCK.with(|clock| clock.borrow().now_ms)
}

pub(crate) fn set_now(now_ms: f64) {
    CLOCK.with(|clock| {
        let mut clock = clock.borrow_mut();
        if now_ms > clock.now_ms {
            clock.now_ms = now_ms;
        }
    });
}

/// A number animated over time; reading it is tracked like a signal.
#[derive(Clone, Copy)]
pub struct Animation {
    value: Signal<f64>,
    slot: usize,
}

impl Animation {
    /// Created in (and disposed with) the current scope.
    pub fn new(initial: f64) -> Self {
        let value = signal(initial);
        let entry = Rc::new(RefCell::new(AnimationEntry {
            value,
            timeline: None,
        }));
        let slot = CLOCK.with(|clock| {
            let mut clock = clock.borrow_mut();
            match clock.animations.iter().position(Option::is_none) {
                Some(slot) => {
                    clock.animations[slot] = Some(entry);
                    slot
                }
                None => {
                    clock.animations.push(Some(entry));
                    clock.animations.len() - 1
                }
            }
        });
        on_cleanup(move || {
            CLOCK.with(|clock| {
                if let Some(entry) = clock.borrow_mut().animations.get_mut(slot) {
                    *entry = None;
                }
            });
        });
        Self { value, slot }
    }

    fn entry(&self) -> Option<Rc<RefCell<AnimationEntry>>> {
        CLOCK.with(|clock| {
            clock
                .borrow()
                .animations
                .get(self.slot)
                .cloned()
                .flatten()
                .filter(|entry| entry.borrow().value == self.value)
        })
    }

    /// Tracked read of the current value.
    pub fn get(&self) -> f64 {
        self.value.try_with(|value| *value).unwrap_or_default()
    }

    pub fn get_untracked(&self) -> f64 {
        self.value
            .try_with(|value| *value)
            .map(|_| self.value.get_untracked())
            .unwrap_or_default()
    }

    pub fn signal(&self) -> ReadSignal<f64> {
        self.value.read_only()
    }

    pub fn map<U: PartialEq + 'static>(self, f: impl Fn(f64) -> U + 'static) -> Memo<U> {
        self.value.map(move |value| f(*value))
    }

    pub fn start(&self, options: AnimationOptions) {
        let Some(entry) = self.entry() else {
            return;
        };
        let from = options.from.unwrap_or_else(|| self.get_untracked());
        entry.borrow_mut().timeline = Some(Timeline {
            started_at_ms: now_ms(),
            duration_ms: options.duration_ms.max(1.0).floor(),
            from,
            to: options.to,
            easing: options.easing,
            repeat: options.repeat,
        });
        self.value.set(from);
    }

    pub fn stop(&self) {
        if let Some(entry) = self.entry() {
            entry.borrow_mut().timeline = None;
        }
    }

    /// Jump to `value`, stopping any running animation.
    pub fn set(&self, value: f64) {
        self.stop();
        self.value.set(value);
    }

    pub fn running(&self) -> bool {
        self.entry()
            .is_some_and(|entry| entry.borrow().timeline.is_some())
    }
}

impl From<Animation> for Prop<f64> {
    fn from(animation: Animation) -> Self {
        Prop::Signal(animation.value)
    }
}

/// Advance every running animation to `now_ms`. Returns whether any is
/// still running.
pub(crate) fn advance_animations(now_ms: f64) -> bool {
    let entries: Vec<_> = CLOCK.with(|clock| clock.borrow().animations.iter().flatten().cloned().collect());
    let mut running = false;
    for entry in entries {
        let update = {
            let mut entry = entry.borrow_mut();
            let Some(timeline) = &entry.timeline else {
                continue;
            };
            let elapsed = (now_ms - timeline.started_at_ms).max(0.0);
            let raw = elapsed / timeline.duration_ms;
            let progress = match timeline.repeat {
                None => clamp_unit(raw),
                Some(Repeat::Loop) => raw - raw.floor(),
                Some(Repeat::PingPong) => {
                    let cycle = raw % 2.0;
                    if cycle <= 1.0 { cycle } else { 2.0 - cycle }
                }
            };
            let eased = timeline.easing.apply(progress);
            let next = timeline.from + (timeline.to - timeline.from) * eased;
            if timeline.repeat.is_none() && (raw >= 1.0 || (next - timeline.to).abs() <= 1e-4) {
                let to = timeline.to;
                entry.timeline = None;
                (entry.value, to)
            } else {
                running = true;
                (entry.value, next)
            }
        };
        update.0.set(update.1);
    }
    running
}

pub(crate) fn has_running_animations() -> bool {
    CLOCK.with(|clock| {
        clock
            .borrow()
            .animations
            .iter()
            .flatten()
            .any(|entry| entry.borrow().timeline.is_some())
    })
}

/// A scheduled callback; cancel it with [`TimerHandle::cancel`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimerHandle(u64);

impl TimerHandle {
    pub fn cancel(self) {
        CLOCK.with(|clock| clock.borrow_mut().timers.retain(|timer| timer.id != self.0));
    }

    pub fn is_pending(self) -> bool {
        CLOCK.with(|clock| clock.borrow().timers.iter().any(|timer| timer.id == self.0))
    }
}

/// Whole milliseconds, at least 1, like the TypeScript scheduler's polls.
///
/// Rounding *down* matters for per-frame intervals: a 60 Hz interval of
/// 16.67 ms would sometimes come due a fraction of a millisecond after the
/// next frame's timestamp (frame times jitter), skipping that frame and
/// catching up with a double step on the next one. At 16 ms it fires on
/// every frame.
fn whole_ms(delay_ms: f64) -> f64 {
    if delay_ms.is_finite() { delay_ms.floor().max(1.0) } else { 1.0 }
}

fn add_timer(delay_ms: f64, interval_ms: Option<f64>, callback: Rc<dyn Fn()>) -> TimerHandle {
    CLOCK.with(|clock| {
        let mut clock = clock.borrow_mut();
        clock.next_timer_id += 1;
        let id = clock.next_timer_id;
        let due_ms = clock.now_ms + whole_ms(delay_ms);
        clock.timers.push(Timer {
            id,
            due_ms,
            interval_ms,
            callback,
        });
        TimerHandle(id)
    })
}

/// Run `f` once, `delay_ms` from now (`setTimeout`).
pub fn set_timeout(delay_ms: f64, f: impl Fn() + 'static) -> TimerHandle {
    add_timer(delay_ms, None, Rc::new(f))
}

/// Run `f` every `interval_ms` until cancelled (`createPoll`).
pub fn set_interval(interval_ms: f64, f: impl Fn() + 'static) -> TimerHandle {
    let interval_ms = whole_ms(interval_ms);
    add_timer(interval_ms, Some(interval_ms), Rc::new(f))
}

/// Run the timers due at `now_ms`; returns whether any ran.
pub(crate) fn run_due_timers(now_ms: f64) -> bool {
    let mut ran = false;
    loop {
        let due = CLOCK.with(|clock| {
            let mut clock = clock.borrow_mut();
            let index = clock.timers.iter().position(|timer| timer.due_ms <= now_ms)?;
            let timer = &mut clock.timers[index];
            let callback = timer.callback.clone();
            match timer.interval_ms {
                Some(interval) => {
                    timer.due_ms = now_ms + interval;
                }
                None => {
                    clock.timers.remove(index);
                }
            }
            Some(callback)
        });
        let Some(callback) = due else {
            break;
        };
        ran = true;
        callback();
    }
    ran
}

/// Milliseconds until the next timer, if any.
pub(crate) fn next_timer_delay() -> Option<u64> {
    CLOCK.with(|clock| {
        let clock = clock.borrow();
        clock
            .timers
            .iter()
            .map(|timer| (timer.due_ms - clock.now_ms).ceil().max(1.0) as u64)
            .min()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reactive::Scope;

    #[test]
    fn cubic_bezier_matches_endpoints_and_is_monotonic() {
        let ease = Easing::EASE_OUT_CUBIC;
        assert_eq!(ease.apply(0.0), 0.0);
        assert_eq!(ease.apply(1.0), 1.0);
        let mut previous = 0.0;
        for step in 1..10 {
            let value = ease.apply(step as f64 / 10.0);
            assert!(value >= previous);
            previous = value;
        }
        assert!((Easing::Linear.apply(0.25) - 0.25).abs() < 1e-9);
    }

    #[test]
    fn animation_advances_with_the_clock() {
        reset_clock();
        let scope = Scope::root();
        let value = scope.run(|| Animation::new(0.0));
        set_now(100.0);
        value.start(AnimationOptions::to(1.0, 100.0));
        assert!(value.running());
        set_now(150.0);
        assert!(advance_animations(150.0));
        assert!((value.get_untracked() - 0.5).abs() < 1e-9);
        set_now(200.0);
        assert!(!advance_animations(200.0));
        assert_eq!(value.get_untracked(), 1.0);
        scope.dispose();
        assert!(!has_running_animations());
    }

    #[test]
    fn a_per_frame_interval_fires_on_every_jittery_frame() {
        reset_clock();
        let fired = Rc::new(std::cell::Cell::new(0));
        let frame = 1000.0 / 60.0;
        {
            let fired = fired.clone();
            set_interval(frame, move || fired.set(fired.get() + 1));
        }
        // Predicted presentation times wobble by a fraction of a millisecond.
        for (index, jitter) in [0.0, -0.3, 0.2, -0.4, 0.1, -0.2, 0.3, -0.1].into_iter().enumerate() {
            let now = (index + 1) as f64 * frame + jitter;
            set_now(now);
            run_due_timers(now);
            assert_eq!(fired.get(), index + 1, "frame {index} was skipped");
        }
    }

    #[test]
    fn timers_fire_once_or_repeatedly() {
        reset_clock();
        let fired = Rc::new(RefCell::new(Vec::new()));
        {
            let fired = fired.clone();
            set_timeout(10.0, move || fired.borrow_mut().push("once"));
        }
        let interval = {
            let fired = fired.clone();
            set_interval(5.0, move || fired.borrow_mut().push("tick"))
        };
        assert_eq!(next_timer_delay(), Some(5));
        set_now(5.0);
        run_due_timers(5.0);
        set_now(10.0);
        run_due_timers(10.0);
        interval.cancel();
        set_now(20.0);
        run_due_timers(20.0);
        assert_eq!(*fired.borrow(), ["tick", "once", "tick"]);
        assert_eq!(next_timer_delay(), None);
    }
}
