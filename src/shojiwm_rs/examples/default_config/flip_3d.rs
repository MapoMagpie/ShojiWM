//! Port of `packages/config/src/flip-3d.tsx`: a Windows Vista style window
//! switcher.
//!
//! `Super+Tab` lifts every window (all workspaces, minimized ones too) off the
//! desktop into a receding diagonal stack, most recently used in front. Hold
//! `Super` and press `Tab` to flip through it, release `Super` to switch to the
//! front window. While it is open it takes all input:
//!
//! - `Tab` / `Shift+Tab`, arrow keys, the mouse wheel, touchpad scrolling and
//!   three-finger swipes flip the stack
//! - `Return` / `Space`, a click on a window, or releasing `Super` switches
//! - `Escape` returns to the desktop unchanged
//!
//! Each window is a render texture of its own, framed with a margin for its
//! shadow, laid out as a plane of a 3D scene. Opening, closing and flipping
//! ease with the window manager's easing; the stack starts and ends exactly on
//! the windows' real positions, so the desktop morphs into it and back.
//!
//! Backdrop blur cannot work inside a texture that holds one window (nothing
//! lies under the window there), so window blur is switched off while the
//! switcher is open (`blur_suspended`) and faded back in afterwards
//! (`backdrop_strength`): the hand-off back to the real desktop happens in the
//! slow tail of the closing animation, and the blur fades in over that tail.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};

use shojiwm_rs::{
    prelude::*,
    ssd::{GestureSwipeEventSnapshot, GestureSwipePhaseSnapshot, WaylandOutputSnapshot},
};

use crate::window_manager::{WINDOW_MANAGEMENT_EASING, WindowManager};

/// Opening and closing, in ms.
const TRANSITION_MS: f64 = 450.0;
/// The blur fading back in after closing, in ms.
const BLUR_FADE_MS: f64 = 350.0;
/// How quickly flipping follows the selection (time constant, ms).
const SCROLL_TIME_CONSTANT_MS: f64 = 70.0;
/// Windows shown in the stack at once.
const VISIBLE_WINDOWS: f64 = 7.0;
/// Room around each window's texture for its shadow (logical px).
const MARGIN: f64 = 48.0;
/// How far the desktop dims behind the stack.
const DIM_ALPHA: f64 = 0.45;
// The stack, after Windows Vista: the front window low on the right, the rest
// receding to the upper left, every window turned about 40° and seen slightly
// from above through a long lens (almost no perspective within a window).
/// Each window's turn about the vertical axis (degrees; the right edge recedes).
const TURN_DEGREES: f64 = 20.0;
/// Each window's tilt about the horizontal axis (degrees; far edges rise).
const PITCH_DEGREES: f64 = 6.0;
/// Where the front window's centre lands, as fractions of the output from its centre (+y up).
const FRONT_X: f64 = 0.13;
const FRONT_Y: f64 = -0.05;
/// Each window further back moves on screen by this much (fractions of the output).
const STEP_X: f64 = -0.085;
const STEP_Y: f64 = 0.048;
/// Each window further back is drawn this much smaller: scale 1 / (1 + slot × this).
const SHRINK_PER_SLOT: f64 = 0.3;
/// The front window's largest size, as fractions of the output.
const FRONT_MAX_WIDTH: f64 = 0.5;
const FRONT_MAX_HEIGHT: f64 = 0.5;
/// Touchpad scrolling / swiping distance per window (logical px).
const SCROLL_STEP_PX: f64 = 80.0;
const SWIPE_STEP_PX: f64 = 140.0;
/// Closing hands back to the real desktop once every window is this close (px).
const HANDOFF_EPSILON_PX: f64 = 0.75;
/// World units between windows of the real stack. On the desktop the windows are
/// pulled towards the camera by their stacking rank (and shrunk to match, so
/// they still cover exactly their pixels): the depth test then lays them over
/// each other in their real order as they settle, instead of z-fighting.
const STACK_DEPTH_STEP: f64 = 2.0;
/// The camera's vertical field of view: narrow, for Vista's long-lens look.
const CAMERA_FOV_DEGREES: f64 = 12.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Pose {
    x: f64,
    y: f64,
    z: f64,
    rotate_x: f64,
    rotate_y: f64,
    scale: f64,
    opacity: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Entering,
    Shown,
    Leaving,
}

struct Session {
    output: String,
    windows: Vec<Window>,
    /// Where each window was on screen when the switcher opened (`None`: not shown).
    start_poses: HashMap<Window, Option<Pose>>,
    /// Each window's rank in the real stack (0 = bottom), once the switcher closes.
    ranks_after: HashMap<Window, usize>,
    /// The pose each window had when closing started.
    leave_from: HashMap<Window, Pose>,
    /// Windows on screen once the switcher closes.
    shown_after: HashSet<Window>,
    phase: Phase,
    /// Linear progress of the current opening/closing, 0..1.
    progress: f64,
    phase_start_ms: Option<f64>,
    /// The front window's index, unbounded (taken modulo the window count).
    selected: i64,
    /// Where the stack is, easing towards `selected`.
    scroll: f64,
    last_tick_ms: Option<f64>,
    grab: Option<InputGrab>,
    poll: Option<TimerHandle>,
    scroll_remainder: f64,
    swipe_applied_steps: i64,
}

type SessionRef = Rc<RefCell<Session>>;

struct Placed {
    window: Window,
    rect: Rect,
    pose: Pose,
}

struct PlacedPlane {
    window: Window,
    rect: Rect,
    pose: Pose,
    width: f64,
    height: f64,
    transform: Transform3D,
}

struct Inner {
    wm: WindowManager,
    /// A window's place in the real stack, higher on top (its decoration's zIndex).
    stack_order: Box<dyn Fn(Window) -> i32>,
    current: RefCell<Option<SessionRef>>,
    /// Bumped when a session starts or ends, and whenever its animated state
    /// changes, so the composition re-evaluates once per frame while
    /// something moves.
    revision: Signal<u64>,
    blur_suspended: Signal<bool>,
    backdrop_strength: Signal<f64>,
    selected_window_id: Signal<Option<String>>,
    blur_fade: RefCell<Option<TimerHandle>>,
}

/// The switcher. Cheap to clone.
#[derive(Clone)]
pub struct Flip3D(Rc<Inner>);

impl Flip3D {
    pub fn new(wm: WindowManager, stack_order: impl Fn(Window) -> i32 + 'static) -> Self {
        Self(Rc::new(Inner {
            wm,
            stack_order: Box::new(stack_order),
            current: RefCell::new(None),
            revision: signal(0),
            blur_suspended: signal(false),
            backdrop_strength: signal(1.0),
            selected_window_id: signal(None),
            blur_fade: RefCell::new(None),
        }))
    }

    /// True while window backdrop effects should be off.
    pub fn blur_suspended(&self) -> ReadSignal<bool> {
        self.0.blur_suspended.read_only()
    }

    /// Scale for window backdrop effects (0..1); fades them back in after closing.
    pub fn backdrop_strength(&self) -> ReadSignal<f64> {
        self.0.backdrop_strength.read_only()
    }

    /// The window in front of the stack while the switcher is open, else `None`.
    pub fn selected_window_id(&self) -> ReadSignal<Option<String>> {
        self.0.selected_window_id.read_only()
    }

    fn touch(&self) {
        self.0.revision.update(|revision| *revision += 1);
    }

    fn current(&self) -> Option<SessionRef> {
        self.0.current.borrow().clone()
    }

    fn is_current(&self, session: &SessionRef) -> bool {
        self.current().is_some_and(|current| Rc::ptr_eq(&current, session))
    }

    fn set_current(&self, session: Option<SessionRef>) {
        *self.0.current.borrow_mut() = session;
        self.touch();
    }

    fn rank_map(&self, windows: &[Window]) -> HashMap<Window, usize> {
        let mut ordered: Vec<(usize, Window, i32)> = windows
            .iter()
            .enumerate()
            .map(|(recency, window)| (recency, *window, (self.0.stack_order)(*window)))
            .collect();
        // Ties: the more recently used window is on top.
        ordered.sort_by(|a, b| a.2.cmp(&b.2).then(b.0.cmp(&a.0)));
        ordered
            .into_iter()
            .enumerate()
            .map(|(rank, (_, window, _))| (window, rank))
            .collect()
    }

    /// Every window's pose for this frame.
    fn poses(&self, session: &Session, width: f64, height: f64, output: &WaylandOutputSnapshot) -> Vec<Placed> {
        let count = session.windows.len();
        let eased = WINDOW_MANAGEMENT_EASING.apply(session.progress);
        session
            .windows
            .iter()
            .enumerate()
            .map(|(index, window)| {
                let rect = window.rect();
                let slot = slot_pose(slot_of(index, session.scroll, count), count, rect, width, height);
                let pose = if session.phase == Phase::Leaving {
                    let from = session.leave_from.get(window).copied().unwrap_or(slot);
                    let target = if session.shown_after.contains(window) {
                        real_pose(
                            rect,
                            output,
                            width,
                            height,
                            session.ranks_after.get(window).copied().unwrap_or(0),
                        )
                    } else {
                        Pose {
                            z: from.z - height * 0.15,
                            opacity: 0.0,
                            ..from
                        }
                    };
                    lerp_pose(from, target, eased)
                } else {
                    let from = session.start_poses.get(window).copied().flatten().unwrap_or(Pose {
                        z: slot.z - height * 0.3,
                        opacity: 0.0,
                        ..slot
                    });
                    if session.phase == Phase::Entering {
                        lerp_pose(from, slot, eased)
                    } else {
                        slot
                    }
                };
                Placed {
                    window: *window,
                    rect,
                    pose,
                }
            })
            .collect()
    }

    fn ensure_ticking(&self, session: &SessionRef) {
        if session.borrow().poll.is_some() {
            return;
        }
        let this = self.clone();
        let ticking = session.clone();
        let output = session.borrow().output.clone();
        let poll = create_poll(1.0, output, move |handle| this.tick(&ticking, handle.now_ms()));
        let mut session = session.borrow_mut();
        session.last_tick_ms = None;
        session.poll = Some(poll);
    }

    fn stop_ticking(&self, session: &SessionRef) {
        if let Some(poll) = session.borrow_mut().poll.take() {
            poll.cancel();
        }
    }

    fn tick(&self, session: &SessionRef, now_ms: f64) {
        if !self.is_current(session) {
            self.stop_ticking(session);
            return;
        }
        let hand_off = {
            let mut s = session.borrow_mut();
            let dt = s.last_tick_ms.map_or(0.0, |last| now_ms - last);
            s.last_tick_ms = Some(now_ms);

            if s.phase != Phase::Shown {
                let start = *s.phase_start_ms.get_or_insert(now_ms);
                s.progress = ((now_ms - start) / TRANSITION_MS).min(1.0);
            }

            let distance = s.selected as f64 - s.scroll;
            if distance.abs() < 0.001 {
                s.scroll = s.selected as f64;
            } else {
                s.scroll += distance * (1.0 - (-dt / SCROLL_TIME_CONSTANT_MS).exp());
            }

            if s.phase == Phase::Entering && s.progress >= 1.0 {
                s.phase = Phase::Shown;
                s.progress = 1.0;
            }
            s.phase == Phase::Leaving && self.ready_to_hand_off(&s)
        };
        if hand_off {
            self.finish(session);
            return;
        }
        self.touch();
        let settled = {
            let s = session.borrow();
            s.phase == Phase::Shown && s.scroll == s.selected as f64
        };
        if settled {
            self.stop_ticking(session);
        }
    }

    /// Every window is (visually) where the real desktop will show it.
    fn ready_to_hand_off(&self, session: &Session) -> bool {
        if session.progress >= 1.0 {
            return true;
        }
        let Some((output, width, height)) = output_size(&session.output) else {
            return true;
        };
        if dim_alpha(session) > 0.01 {
            return false;
        }
        self.poses(session, width, height, &output).iter().all(|placed| {
            if !session.shown_after.contains(&placed.window) {
                return placed.pose.opacity < 0.02;
            }
            let rect = placed.rect;
            let target = real_pose(
                rect,
                &output,
                width,
                height,
                session.ranks_after.get(&placed.window).copied().unwrap_or(0),
            );
            let pose = placed.pose;
            let half_extent = rect.width.max(rect.height) / 2.0 + MARGIN;
            let deviation = (pose.x - target.x).abs()
                + (pose.y - target.y).abs()
                + (pose.z - target.z).abs()
                + (pose.rotate_x.abs() + pose.rotate_y.abs()).to_radians() * half_extent
                + (pose.scale - target.scale).abs() * half_extent;
            deviation < HANDOFF_EPSILON_PX
        })
    }

    /// Open the switcher on `output_name` (no-op while open).
    pub fn open(&self, output_name: &str) {
        if self.current().is_some() {
            return;
        }
        let windows = self.0.wm.read(|wm| wm.list_windows_by_recent_use());
        let Some((output, width, height)) = output_size(output_name) else {
            return;
        };
        if windows.is_empty() {
            return;
        }
        let ranks = self.rank_map(&windows);
        let start_poses = windows
            .iter()
            .map(|window| {
                let shown = self.0.wm.read(|wm| wm.is_window_shown_on(*window, output_name));
                let pose = shown.then(|| {
                    real_pose(window.rect(), &output, width, height, ranks.get(window).copied().unwrap_or(0))
                });
                (*window, pose)
            })
            .collect();
        let session = Rc::new(RefCell::new(Session {
            output: output_name.to_owned(),
            // Like Alt+Tab, the previous window comes to the front: the stack
            // flips once as it opens.
            selected: if windows.len() > 1 { 1 } else { 0 },
            windows,
            start_poses,
            ranks_after: ranks,
            leave_from: HashMap::new(),
            shown_after: HashSet::new(),
            phase: Phase::Entering,
            progress: 0.0,
            phase_start_ms: None,
            scroll: 0.0,
            last_tick_ms: None,
            grab: None,
            poll: None,
            scroll_remainder: 0.0,
            swipe_applied_steps: 0,
        }));
        if let Some(fade) = self.0.blur_fade.borrow_mut().take() {
            fade.cancel();
        }
        self.0.backdrop_strength.set(0.0);
        self.0.blur_suspended.set(true);
        let grab = COMPOSITOR.input.grab(self.grab_options(&session));
        session.borrow_mut().grab = Some(grab);
        self.set_current(Some(session.clone()));
        self.0.selected_window_id.set(front_window(&session.borrow()).map(|window| window.id()));
        self.ensure_ticking(&session);
    }

    fn grab_options(&self, session: &SessionRef) -> InputGrabOptions {
        let on_key = {
            let (this, session) = (self.clone(), session.clone());
            move |event: &InputGrabKeyEvent| {
                if event.state == InputGrabState::Released {
                    if event.key == "Super_L" || event.key == "Super_R" {
                        this.commit(&session);
                    }
                    return;
                }
                match event.key.as_str() {
                    "Tab" | "ISO_Left_Tab" => this.flip(&session, if event.modifiers.shift { -1 } else { 1 }),
                    "Right" | "Down" => this.flip(&session, 1),
                    "Left" | "Up" => this.flip(&session, -1),
                    "Return" | "KP_Enter" | "space" => this.commit(&session),
                    "Escape" => this.close(&session, None),
                    _ => {}
                }
            }
        };
        let on_pointer_button = {
            let (this, session) = (self.clone(), session.clone());
            move |event: &InputGrabPointerButtonEvent| {
                if event.state != InputGrabState::Pressed || event.button_name.as_deref() != Some("left") {
                    return;
                }
                if let Some(window) = this.window_at(&session, event.position.x, event.position.y) {
                    this.close(&session, Some(window));
                }
            }
        };
        let on_scroll = {
            let (this, session) = (self.clone(), session.clone());
            move |event: &InputGrabScrollEvent| {
                if event.source == "wheel" {
                    let clicks = event.discrete_y.unwrap_or(event.delta_y.signum() * 120.0) / 120.0;
                    this.flip(&session, clicks.signum() as i64 * clicks.abs().round().max(1.0) as i64);
                    return;
                }
                let steps = {
                    let mut s = session.borrow_mut();
                    s.scroll_remainder += event.delta_y;
                    let steps = (s.scroll_remainder / SCROLL_STEP_PX).trunc();
                    s.scroll_remainder -= steps * SCROLL_STEP_PX;
                    steps as i64
                };
                if steps != 0 {
                    this.flip(&session, steps);
                }
            }
        };
        let on_swipe = {
            let (this, session) = (self.clone(), session.clone());
            move |event: &GestureSwipeEventSnapshot| {
                if event.phase == GestureSwipePhaseSnapshot::Begin {
                    session.borrow_mut().swipe_applied_steps = 0;
                    return;
                }
                let travel = if event.total_x.abs() > event.total_y.abs() {
                    -event.total_x
                } else {
                    event.total_y
                };
                let steps = (travel / SWIPE_STEP_PX).trunc() as i64;
                let applied = session.borrow().swipe_applied_steps;
                if steps != applied {
                    this.flip(&session, steps - applied);
                    session.borrow_mut().swipe_applied_steps = steps;
                }
            }
        };
        let on_cancel = {
            let (this, session) = (self.clone(), session.clone());
            move |_: &InputGrabCancelReason| {
                // The screen locked or a handler failed: drop the switcher at once.
                session.borrow_mut().grab = None;
                this.finish(&session);
            }
        };
        InputGrabOptions::new()
            .on_key(on_key)
            .on_pointer_button(on_pointer_button)
            .on_scroll(on_scroll)
            .on_swipe(on_swipe)
            .on_cancel(on_cancel)
    }

    fn flip(&self, session: &SessionRef, steps: i64) {
        if !self.is_current(session) || session.borrow().phase == Phase::Leaving || steps == 0 {
            return;
        }
        session.borrow_mut().selected += steps;
        self.0.selected_window_id.set(front_window(&session.borrow()).map(|window| window.id()));
        self.ensure_ticking(session);
    }

    fn commit(&self, session: &SessionRef) {
        let front = front_window(&session.borrow());
        self.close(session, front);
    }

    /// Close the switcher, switching to `target` (`None`: back to the desktop as it was).
    fn close(&self, session: &SessionRef, target: Option<Window>) {
        if !self.is_current(session) || session.borrow().phase == Phase::Leaving {
            return;
        }
        let grab = session.borrow_mut().grab.take();
        if let Some(grab) = grab {
            grab.release();
        }
        let output_name = session.borrow().output.clone();
        let Some((output, width, height)) = output_size(&output_name) else {
            self.finish(session);
            return;
        };
        // Freeze where everything is, then switch for real.
        let poses = self.poses(&session.borrow(), width, height, &output);
        {
            let mut s = session.borrow_mut();
            for placed in poses {
                s.leave_from.insert(placed.window, placed.pose);
            }
            s.scroll = s.selected as f64;
        }
        if let Some(target) = target {
            let id = target.id();
            self.0.wm.with(|wm| wm.activate_window_by_id_instant(&id));
        }
        let windows = session.borrow().windows.clone();
        let shown: HashSet<Window> = windows
            .iter()
            .copied()
            .filter(|window| self.0.wm.read(|wm| wm.is_window_shown_on(*window, &output_name)))
            .collect();
        let mut ranks = self.rank_map(&windows);
        if let Some(target) = target {
            // Activation puts it on top; its focus may land a moment later.
            ranks.insert(target, windows.len());
        }
        {
            let mut s = session.borrow_mut();
            s.shown_after.extend(shown);
            s.ranks_after = ranks;
            s.phase = Phase::Leaving;
            s.progress = 0.0;
            s.phase_start_ms = None;
        }
        self.0.selected_window_id.set(None);
        self.ensure_ticking(session);
        self.touch();
    }

    /// Back to the real desktop, and fade window blur in.
    fn finish(&self, session: &SessionRef) {
        self.stop_ticking(session);
        let grab = session.borrow_mut().grab.take();
        if let Some(grab) = grab {
            grab.release();
        }
        if !self.is_current(session) {
            return;
        }
        self.0.backdrop_strength.set(0.0);
        self.0.blur_suspended.set(false);
        self.set_current(None);
        self.0.selected_window_id.set(None);
        let fade_start = std::cell::Cell::new(None::<f64>);
        let strength = self.0.backdrop_strength;
        let this = self.clone();
        let output = session.borrow().output.clone();
        let fade = create_poll(1.0, output, move |poll| {
            let now = poll.now_ms();
            let start = fade_start.get().unwrap_or(now);
            fade_start.set(Some(start));
            let linear = ((now - start) / BLUR_FADE_MS).min(1.0);
            strength.set(ease_out_cubic(linear));
            if linear >= 1.0 {
                poll.cancel();
                let mut slot = this.0.blur_fade.borrow_mut();
                if *slot == Some(*poll) {
                    *slot = None;
                }
            }
        });
        *self.0.blur_fade.borrow_mut() = Some(fade);
    }

    fn window_at(&self, session: &SessionRef, global_x: f64, global_y: f64) -> Option<Window> {
        let session = session.borrow();
        let (output, width, height) = output_size(&session.output)?;
        let placed = self.visible_planes(&session, width, height, &output);
        let planes: Vec<PickablePlane> = placed
            .iter()
            .map(|plane| PickablePlane {
                width: plane.width,
                height: plane.height,
                transform: plane.transform.matrix,
            })
            .collect();
        let hit = pick_plane(
            &flip_camera(width, height),
            (width, height),
            &planes,
            global_x - output.position.x as f64,
            global_y - output.position.y as f64,
        )?;
        Some(placed[hit].window)
    }

    fn visible_planes(
        &self,
        session: &Session,
        width: f64,
        height: f64,
        output: &WaylandOutputSnapshot,
    ) -> Vec<PlacedPlane> {
        let alive: HashSet<Window> = self.0.wm.read(|wm| wm.list_windows()).into_iter().collect();
        self.poses(session, width, height, output)
            .into_iter()
            .filter(|placed| alive.contains(&placed.window) && placed.pose.opacity > 0.001)
            .map(|Placed { window, rect, pose }| PlacedPlane {
                window,
                rect,
                pose,
                width: rect.width + MARGIN * 2.0,
                height: rect.height + MARGIN * 2.0,
                transform: transform3d()
                    .translate(pose.x, pose.y, pose.z)
                    .rotate_x(pose.rotate_x)
                    .rotate_y(pose.rotate_y)
                    .scale(pose.scale),
            })
            .collect()
    }

    /// The switcher's composition for `output` while open, else `None`.
    pub fn compose(&self, output: &WaylandOutputSnapshot) -> Option<OutputStack> {
        self.0.revision.get();
        let session = self.current()?;
        let session = session.borrow();
        if session.output != output.name {
            return None;
        }
        let (width, height) = output_logical_size(output);
        let planes = self.visible_planes(&session, width, height, output);
        let scene = Scene3D::new(flip_camera(width, height)).planes(planes.iter().map(|plane| {
            Plane::new(&window_texture(plane.window, plane.rect, output), plane.width, plane.height)
                .transform(plane.transform)
                .opacity(plane.pose.opacity)
        }));
        Some(
            OutputStack::new()
                .child(Layers::new([LayerName::Background, LayerName::Bottom]))
                .child(Solid::new([0.0, 0.0, 0.0, dim_alpha(&session)]))
                .child(scene)
                .child(Layers::new([LayerName::Top, LayerName::Overlay]))
                .child(LayerPopups),
        )
    }
}

fn front_window(session: &Session) -> Option<Window> {
    let count = session.windows.len() as i64;
    if count == 0 {
        return None;
    }
    session.windows.get(session.selected.rem_euclid(count) as usize).copied()
}

fn window_texture(window: Window, rect: Rect, output: &WaylandOutputSnapshot) -> RenderTexture {
    RenderTexture::new()
        .key(format!("flip-3d-{}", window.id()))
        .size(rect.width + MARGIN * 2.0, rect.height + MARGIN * 2.0)
        .child(Windows::only([window]).offset(
            output.position.x as f64 - rect.x + MARGIN,
            output.position.y as f64 - rect.y + MARGIN,
        ))
}

fn output_size(name: &str) -> Option<(WaylandOutputSnapshot, f64, f64)> {
    let output = COMPOSITOR.output.get(name)?;
    let (width, height) = output_logical_size(&output);
    Some((output, width, height))
}

/// The window exactly where it is on screen. The flip camera maps z = 0 1:1;
/// a window `rank` steps up the stack sits that much nearer the camera,
/// shrunk by the perspective it gains so it covers the same pixels.
fn real_pose(rect: Rect, output: &WaylandOutputSnapshot, width: f64, height: f64, rank: usize) -> Pose {
    let center_x = rect.x - output.position.x as f64 + rect.width / 2.0;
    let center_y = rect.y - output.position.y as f64 + rect.height / 2.0;
    let distance = camera_distance(height);
    let z = rank as f64 * STACK_DEPTH_STEP;
    let shrink = (distance - z) / distance;
    Pose {
        x: (center_x - width / 2.0) * shrink,
        y: (height / 2.0 - center_y) * shrink,
        z,
        rotate_x: 0.0,
        rotate_y: 0.0,
        scale: shrink,
        opacity: 1.0,
    }
}

/// The window's place in the stack. `slot` 0 is the front; the stack recedes
/// up and to the left. Slots below 0 have flipped past the front and fade
/// out; the last visible slot fades in.
fn slot_pose(slot: f64, count: usize, rect: Rect, width: f64, height: f64) -> Pose {
    let fit = 1.0_f64
        .min(width * FRONT_MAX_WIDTH / rect.width)
        .min(height * FRONT_MAX_HEIGHT / rect.height);
    // Depth that draws the slot 1 / (1 + slot × SHRINK_PER_SLOT) as large,
    // and the world position that puts its centre on its spot on screen.
    let depth = slot * SHRINK_PER_SLOT * camera_distance(height);
    let depth_scale = 1.0 + slot * SHRINK_PER_SLOT;
    let last = (count as f64).min(VISIBLE_WINDOWS);
    let opacity = if slot < 0.0 {
        clamp01(1.0 + slot * 2.0)
    } else if slot > last - 1.0 {
        clamp01((last - 0.5 - slot) * 2.0)
    } else {
        1.0
    };
    Pose {
        x: (FRONT_X + slot * STEP_X) * width * depth_scale,
        y: (FRONT_Y + slot * STEP_Y) * height * depth_scale,
        z: -depth,
        rotate_x: PITCH_DEGREES,
        rotate_y: TURN_DEGREES,
        scale: fit,
        opacity,
    }
}

fn slot_of(index: usize, scroll: f64, count: usize) -> f64 {
    (index as f64 - scroll + 0.5).rem_euclid(count as f64) - 0.5
}

fn dim_alpha(session: &Session) -> f64 {
    let eased = WINDOW_MANAGEMENT_EASING.apply(session.progress);
    match session.phase {
        Phase::Entering => DIM_ALPHA * eased,
        Phase::Shown => DIM_ALPHA,
        Phase::Leaving => DIM_ALPHA * (1.0 - eased),
    }
}

/// Distance at which the camera sees the plane z = 0 1:1 in logical pixels.
fn camera_distance(height: f64) -> f64 {
    height / 2.0 / (CAMERA_FOV_DEGREES.to_radians() / 2.0).tan()
}

/// Like `screen_camera` with a narrow field of view, but with room for a
/// stack that recedes several times the camera distance.
fn flip_camera(width: f64, height: f64) -> Camera {
    let distance = camera_distance(height);
    Camera {
        projection: perspective(CAMERA_FOV_DEGREES, width / height.max(1.0), distance / 100.0, distance * 6.0),
        view: look_at([0.0, 0.0, distance], [0.0, 0.0, 0.0]),
    }
}

fn lerp(from: f64, to: f64, t: f64) -> f64 {
    from + (to - from) * t
}

fn lerp_pose(from: Pose, to: Pose, t: f64) -> Pose {
    Pose {
        x: lerp(from.x, to.x, t),
        y: lerp(from.y, to.y, t),
        z: lerp(from.z, to.z, t),
        rotate_x: lerp(from.rotate_x, to.rotate_x, t),
        rotate_y: lerp(from.rotate_y, to.rotate_y, t),
        scale: lerp(from.scale, to.scale, t),
        opacity: lerp(from.opacity, to.opacity, t),
    }
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn ease_out_cubic(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(3)
}
