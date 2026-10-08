/**
 * Flip 3D: a Windows Vista style window switcher.
 *
 * `Super+Tab` lifts every window (all workspaces, minimized ones too) off the
 * desktop into a receding diagonal stack, most recently used in front. Hold
 * `Super` and press `Tab` to flip through it, release `Super` to switch to the
 * front window. While it is open it takes all input:
 *
 * - `Tab` / `Shift+Tab`, arrow keys, the mouse wheel, touchpad scrolling and
 *   three-finger swipes flip the stack
 * - `Return` / `Space`, a click on a window, or releasing `Super` switches
 * - `Escape` returns to the desktop unchanged
 *
 * Each window is a render texture of its own, framed with a margin for its
 * shadow, laid out as a plane of a 3D scene. Opening, closing and flipping
 * ease with the window manager's easing; the stack starts and ends exactly on
 * the windows' real positions, so the desktop morphs into it and back.
 *
 * Backdrop blur cannot work inside a texture that holds one window (nothing
 * lies under the window there), so window blur is switched off while the
 * switcher is open (`blurSuspended`) and faded back in afterwards
 * (`backdropStrength`): the hand-off back to the real desktop happens in the
 * slow tail of the closing animation, and the blur fades in over that tail.
 */
import {
  COMPOSITOR,
  createPoll,
  easeOutCubic,
  LayerPopups,
  Layers,
  outputLogicalSize,
  pickPlane,
  Plane,
  renderTexture,
  Scene3D,
  lookAt,
  perspective,
  signal,
  Solid,
  transform3d,
  Windows,
  type Camera,
  type CompositionRenderable,
  type GestureSwipeEvent,
  type InputGrab,
  type OutputInfo,
  type PollHandle,
  type ReadonlySignal,
  type WaylandWindow,
} from "shoji_wm";
import {
  WINDOW_MANAGEMENT_EASING,
  type HybridWindowManager,
} from "./window-manager";

/** Opening and closing, in ms. */
const TRANSITION_MS = 450;
/** The blur fading back in after closing, in ms. */
const BLUR_FADE_MS = 350;
/** How quickly flipping follows the selection (time constant, ms). */
const SCROLL_TIME_CONSTANT_MS = 70;
/** Windows shown in the stack at once. */
const VISIBLE_WINDOWS = 7;
/** Room around each window's texture for its shadow (logical px). */
const MARGIN = 48;
/** How far the desktop dims behind the stack. */
const DIM_ALPHA = 0.45;
// The stack, after Windows Vista: the front window low on the right, the rest
// receding to the upper left, every window turned about 40° and seen slightly
// from above through a long lens (almost no perspective within a window).
/** Each window's turn about the vertical axis (degrees; the right edge recedes). */
const TURN_DEGREES = 20;
/** Each window's tilt about the horizontal axis (degrees; far edges rise). */
const PITCH_DEGREES = 6;
/** Where the front window's centre lands, as fractions of the output from its centre (+y up). */
const FRONT_X = 0.13;
const FRONT_Y = -0.05;
/** Each window further back moves on screen by this much (fractions of the output). */
const STEP_X = -0.085;
const STEP_Y = 0.048;
/** Each window further back is drawn this much smaller: scale 1 / (1 + slot × this). */
const SHRINK_PER_SLOT = 0.3;
/** The front window's largest size, as fractions of the output. */
const FRONT_MAX_WIDTH = 0.5;
const FRONT_MAX_HEIGHT = 0.5;
/** Touchpad scrolling / swiping distance per window (logical px). */
const SCROLL_STEP_PX = 80;
const SWIPE_STEP_PX = 140;
/** Closing hands back to the real desktop once every window is this close (px). */
const HANDOFF_EPSILON_PX = 0.75;
/**
 * World units between windows of the real stack. On the desktop the windows are
 * pulled towards the camera by their stacking rank (and shrunk to match, so
 * they still cover exactly their pixels): the depth test then lays them over
 * each other in their real order as they settle, instead of z-fighting.
 */
const STACK_DEPTH_STEP = 2;
/** The camera's vertical field of view: narrow, for Vista's long-lens look. */
const CAMERA_FOV_DEGREES = 12;

interface Pose {
  x: number;
  y: number;
  z: number;
  rotateX: number;
  rotateY: number;
  scale: number;
  opacity: number;
}

interface Rect {
  x: number;
  y: number;
  width: number;
  height: number;
}

type Phase = "entering" | "shown" | "leaving";

interface Session {
  output: string;
  windows: WaylandWindow[];
  /** Where each window was on screen when the switcher opened (`null`: not shown). */
  startPoses: Map<string, Pose | null>;
  /** Each window's rank in the real stack (0 = bottom), once the switcher closes. */
  ranksAfter: Map<string, number>;
  /** The pose each window had when closing started. */
  leaveFrom: Map<string, Pose>;
  /** Windows on screen once the switcher closes. */
  shownAfter: Set<string>;
  phase: Phase;
  /** Linear progress of the current opening/closing, 0..1. */
  progress: number;
  phaseStartMs: number | null;
  /** The front window's index, unbounded (taken modulo the window count). */
  selected: number;
  /** Where the stack is, easing towards `selected`. */
  scroll: number;
  lastTickMs: number | null;
  grab: InputGrab | null;
  poll: PollHandle | null;
  scrollRemainder: number;
  swipeAppliedSteps: number;
}

export interface Flip3D {
  /** Open the switcher on `output` (no-op while open). */
  open(output: string): void;
  /** The switcher's composition for `output` while open, else `null`. */
  compose(output: OutputInfo): CompositionRenderable | null;
  /** True while window backdrop effects should be off. */
  readonly blurSuspended: ReadonlySignal<boolean>;
  /** Scale for window backdrop effects (0..1); fades them back in after closing. */
  readonly backdropStrength: ReadonlySignal<number>;
  /** The window in front of the stack while the switcher is open, else `null`. */
  readonly selectedWindowId: ReadonlySignal<string | null>;
}

export function createFlip3D(options: {
  windowManager: HybridWindowManager;
  /** A window's place in the real stack, higher on top (its decoration's zIndex). */
  stackOrder: (window: WaylandWindow) => number;
}): Flip3D {
  const wm = options.windowManager;
  const [current, setCurrent] = signal<Session | null>(null);
  // Bumped whenever the session's animated state changes, so the composition
  // re-evaluates once per frame while something moves.
  const [revision, setRevision] = signal(0);
  const [blurSuspended, setBlurSuspended] = signal(false);
  const [backdropStrength, setBackdropStrength] = signal(1);
  const [selectedWindowId, setSelectedWindowId] = signal<string | null>(null);
  let blurFade: PollHandle | null = null;

  const touch = () => setRevision(revision.peek() + 1);

  function outputSize(name: string) {
    const output = COMPOSITOR.output.get(name);
    return output ? { output, ...outputLogicalSize(output) } : null;
  }

  function rectOf(window: WaylandWindow): Rect {
    const rect = window.rect;
    return { x: rect.x, y: rect.y, width: rect.width, height: rect.height };
  }

  /**
   * The window exactly where it is on screen. `screenCamera` maps z = 0 1:1; a
   * window `rank` steps up the stack sits that much nearer the camera, shrunk
   * by the perspective it gains so it covers the same pixels.
   */
  function realPose(
    rect: Rect,
    output: OutputInfo,
    width: number,
    height: number,
    rank: number,
  ): Pose {
    const centerX = rect.x - output.position.x + rect.width / 2;
    const centerY = rect.y - output.position.y + rect.height / 2;
    const distance = cameraDistance(height);
    const z = rank * STACK_DEPTH_STEP;
    const shrink = (distance - z) / distance;
    return {
      x: (centerX - width / 2) * shrink,
      y: (height / 2 - centerY) * shrink,
      z,
      rotateX: 0,
      rotateY: 0,
      scale: shrink,
      opacity: 1,
    };
  }

  /** Rank of each window in the real stack, bottom first. */
  function stackRanks(windows: WaylandWindow[]): Map<string, number> {
    const ordered = windows
      .map((window, recency) => ({ window, recency, order: options.stackOrder(window) }))
      // Ties: the more recently used window is on top.
      .sort((a, b) => a.order - b.order || b.recency - a.recency);
    return new Map(ordered.map(({ window }, rank) => [window.id, rank]));
  }

  /**
   * The window's place in the stack. `slot` 0 is the front; the stack recedes
   * up and to the right. Slots below 0 have flipped past the front and fade
   * out; the last visible slot fades in.
   */
  function slotPose(slot: number, count: number, rect: Rect, width: number, height: number): Pose {
    const fit = Math.min(
      1,
      (width * FRONT_MAX_WIDTH) / rect.width,
      (height * FRONT_MAX_HEIGHT) / rect.height,
    );
    // Depth that draws the slot 1 / (1 + slot × SHRINK_PER_SLOT) as large,
    // and the world position that puts its centre on its spot on screen.
    const depth = slot * SHRINK_PER_SLOT * cameraDistance(height);
    const depthScale = 1 + slot * SHRINK_PER_SLOT;
    const last = Math.min(count, VISIBLE_WINDOWS);
    let opacity = 1;
    if (slot < 0) {
      opacity = clamp01(1 + slot * 2);
    } else if (slot > last - 1) {
      opacity = clamp01((last - 0.5 - slot) * 2);
    }
    return {
      x: (FRONT_X + slot * STEP_X) * width * depthScale,
      y: (FRONT_Y + slot * STEP_Y) * height * depthScale,
      z: -depth,
      rotateX: PITCH_DEGREES,
      rotateY: TURN_DEGREES,
      scale: fit,
      opacity,
    };
  }

  function slotOf(index: number, scroll: number, count: number): number {
    return mod(index - scroll + 0.5, count) - 0.5;
  }

  /** Every window's pose for this frame. */
  function poses(session: Session, width: number, height: number, output: OutputInfo) {
    const count = session.windows.length;
    const eased = WINDOW_MANAGEMENT_EASING(session.progress);
    return session.windows.map((window, index) => {
      const rect = rectOf(window);
      const slot = slotPose(slotOf(index, session.scroll, count), count, rect, width, height);
      let pose: Pose;
      if (session.phase === "leaving") {
        const from = session.leaveFrom.get(window.id) ?? slot;
        const target = session.shownAfter.has(window.id)
          ? realPose(rect, output, width, height, session.ranksAfter.get(window.id) ?? 0)
          : { ...from, z: from.z - height * 0.15, opacity: 0 };
        pose = lerpPose(from, target, eased);
      } else {
        const start = session.startPoses.get(window.id);
        const from = start ?? { ...slot, z: slot.z - height * 0.3, opacity: 0 };
        pose = session.phase === "entering" ? lerpPose(from, slot, eased) : slot;
      }
      return { window, rect, pose };
    });
  }

  function dimAlpha(session: Session): number {
    const eased = WINDOW_MANAGEMENT_EASING(session.progress);
    switch (session.phase) {
      case "entering":
        return DIM_ALPHA * eased;
      case "shown":
        return DIM_ALPHA;
      case "leaving":
        return DIM_ALPHA * (1 - eased);
    }
  }

  function ensureTicking(session: Session) {
    if (session.poll) {
      return;
    }
    session.lastTickMs = null;
    session.poll = createPoll(1, (handle) => tick(session, handle.nowMs), {
      output: session.output,
      dirty: "none",
    });
  }

  function stopTicking(session: Session) {
    session.poll?.cancel();
    session.poll = null;
  }

  function tick(session: Session, nowMs: number) {
    if (current.peek() !== session) {
      stopTicking(session);
      return;
    }
    const dt = session.lastTickMs === null ? 0 : nowMs - session.lastTickMs;
    session.lastTickMs = nowMs;

    if (session.phase !== "shown") {
      session.phaseStartMs ??= nowMs;
      session.progress = Math.min(1, (nowMs - session.phaseStartMs) / TRANSITION_MS);
    }

    const distance = session.selected - session.scroll;
    if (Math.abs(distance) < 0.001) {
      session.scroll = session.selected;
    } else {
      session.scroll += distance * (1 - Math.exp(-dt / SCROLL_TIME_CONSTANT_MS));
    }

    if (session.phase === "entering" && session.progress >= 1) {
      session.phase = "shown";
      session.progress = 1;
    }
    if (session.phase === "leaving" && readyToHandOff(session)) {
      finish(session);
      return;
    }
    touch();
    if (session.phase === "shown" && session.scroll === session.selected) {
      stopTicking(session);
    }
  }

  /** Every window is (visually) where the real desktop will show it. */
  function readyToHandOff(session: Session): boolean {
    if (session.progress >= 1) {
      return true;
    }
    const size = outputSize(session.output);
    if (!size) {
      return true;
    }
    const { output, width, height } = size;
    if (dimAlpha(session) > 0.01) {
      return false;
    }
    return poses(session, width, height, output).every(({ window, rect, pose }) => {
      if (!session.shownAfter.has(window.id)) {
        return pose.opacity < 0.02;
      }
      const target = realPose(rect, output, width, height, session.ranksAfter.get(window.id) ?? 0);
      const halfExtent = Math.max(rect.width, rect.height) / 2 + MARGIN;
      const deviation =
        Math.abs(pose.x - target.x) +
        Math.abs(pose.y - target.y) +
        Math.abs(pose.z - target.z) +
        (Math.abs(pose.rotateX) + Math.abs(pose.rotateY)) * (Math.PI / 180) * halfExtent +
        Math.abs(pose.scale - target.scale) * halfExtent;
      return deviation < HANDOFF_EPSILON_PX;
    });
  }

  function open(outputName: string) {
    if (current.peek()) {
      return;
    }
    const size = outputSize(outputName);
    const windows = wm.listWindowsByRecentUse();
    if (!size || windows.length === 0) {
      return;
    }
    const { output, width, height } = size;
    const ranks = stackRanks(windows);
    const startPoses = new Map<string, Pose | null>();
    for (const window of windows) {
      startPoses.set(
        window.id,
        wm.isWindowShownOn(window, outputName)
          ? realPose(rectOf(window), output, width, height, ranks.get(window.id) ?? 0)
          : null,
      );
    }
    const session: Session = {
      output: outputName,
      windows,
      startPoses,
      ranksAfter: ranks,
      leaveFrom: new Map(),
      shownAfter: new Set(),
      phase: "entering",
      progress: 0,
      phaseStartMs: null,
      // Like Alt+Tab, the previous window comes to the front: the stack flips
      // once as it opens.
      selected: windows.length > 1 ? 1 : 0,
      scroll: 0,
      lastTickMs: null,
      grab: null,
      poll: null,
      scrollRemainder: 0,
      swipeAppliedSteps: 0,
    };
    blurFade?.cancel();
    blurFade = null;
    setBackdropStrength(0);
    setBlurSuspended(true);
    session.grab = COMPOSITOR.input.grab({
      onKey: (event) => {
        if (event.state === "released") {
          if (event.key === "Super_L" || event.key === "Super_R") {
            commit(session);
          }
          return;
        }
        switch (event.key) {
          case "Tab":
          case "ISO_Left_Tab":
            flip(session, event.modifiers.shift ? -1 : 1);
            break;
          case "Right":
          case "Down":
            flip(session, 1);
            break;
          case "Left":
          case "Up":
            flip(session, -1);
            break;
          case "Return":
          case "KP_Enter":
          case "space":
            commit(session);
            break;
          case "Escape":
            close(session, null);
            break;
        }
      },
      onPointerButton: (event) => {
        if (event.state !== "pressed" || event.buttonName !== "left") {
          return;
        }
        const window = windowAt(session, event.position.x, event.position.y);
        if (window) {
          close(session, window);
        }
      },
      onScroll: (event) => {
        if (event.source === "wheel") {
          const clicks = (event.discreteY ?? Math.sign(event.deltaY) * 120) / 120;
          flip(session, Math.sign(clicks) * Math.max(1, Math.round(Math.abs(clicks))));
          return;
        }
        session.scrollRemainder += event.deltaY;
        const steps = Math.trunc(session.scrollRemainder / SCROLL_STEP_PX);
        if (steps !== 0) {
          session.scrollRemainder -= steps * SCROLL_STEP_PX;
          flip(session, steps);
        }
      },
      onSwipe: (event: GestureSwipeEvent) => {
        if (event.phase === "begin") {
          session.swipeAppliedSteps = 0;
          return;
        }
        const travel = Math.abs(event.totalX) > Math.abs(event.totalY) ? -event.totalX : event.totalY;
        const steps = Math.trunc(travel / SWIPE_STEP_PX);
        if (steps !== session.swipeAppliedSteps) {
          flip(session, steps - session.swipeAppliedSteps);
          session.swipeAppliedSteps = steps;
        }
      },
      onCancel: () => {
        // The screen locked or a handler failed: drop the switcher at once.
        session.grab = null;
        finish(session);
      },
    });
    setCurrent(session);
    setSelectedWindowId(frontWindow(session)?.id ?? null);
    ensureTicking(session);
  }

  function flip(session: Session, steps: number) {
    if (current.peek() !== session || session.phase === "leaving" || steps === 0) {
      return;
    }
    session.selected += steps;
    setSelectedWindowId(frontWindow(session)?.id ?? null);
    ensureTicking(session);
  }

  function frontWindow(session: Session): WaylandWindow | undefined {
    const count = session.windows.length;
    return session.windows[mod(session.selected, count)];
  }

  function commit(session: Session) {
    close(session, frontWindow(session) ?? null);
  }

  /** Close the switcher, switching to `target` (`null`: back to the desktop as it was). */
  function close(session: Session, target: WaylandWindow | null) {
    if (current.peek() !== session || session.phase === "leaving") {
      return;
    }
    const size = outputSize(session.output);
    session.grab?.release();
    session.grab = null;
    if (!size) {
      finish(session);
      return;
    }
    const { output, width, height } = size;
    // Freeze where everything is, then switch for real.
    for (const { window, pose } of poses(session, width, height, output)) {
      session.leaveFrom.set(window.id, pose);
    }
    session.scroll = session.selected;
    if (target) {
      wm.activateWindowById(target.id, { instant: true });
    }
    for (const window of session.windows) {
      if (wm.isWindowShownOn(window, session.output)) {
        session.shownAfter.add(window.id);
      }
    }
    session.ranksAfter = stackRanks(session.windows);
    if (target) {
      // Activation puts it on top; its focus may land a moment later.
      session.ranksAfter.set(target.id, session.windows.length);
    }
    session.phase = "leaving";
    setSelectedWindowId(null);
    session.progress = 0;
    session.phaseStartMs = null;
    ensureTicking(session);
    touch();
  }

  /** Back to the real desktop, and fade window blur in. */
  function finish(session: Session) {
    stopTicking(session);
    session.grab?.release();
    session.grab = null;
    if (current.peek() !== session) {
      return;
    }
    setBackdropStrength(0);
    setBlurSuspended(false);
    setCurrent(null);
    setSelectedWindowId(null);
    let fadeStartMs: number | null = null;
    blurFade = createPoll(
      1,
      (handle) => {
        fadeStartMs ??= handle.nowMs;
        const linear = Math.min(1, (handle.nowMs - fadeStartMs) / BLUR_FADE_MS);
        setBackdropStrength(easeOutCubic(linear));
        if (linear >= 1) {
          handle.cancel();
          if (blurFade === handle) {
            blurFade = null;
          }
        }
      },
      { output: session.output, dirty: "none" },
    );
  }

  function windowAt(session: Session, globalX: number, globalY: number) {
    const size = outputSize(session.output);
    if (!size) {
      return undefined;
    }
    const { output, width, height } = size;
    const placed = visiblePlanes(session, width, height, output);
    const hit = pickPlane(
      flipCamera(width, height),
      { width, height },
      placed.map(({ planeWidth, planeHeight, transform }) => ({
        width: planeWidth,
        height: planeHeight,
        transform,
      })),
      globalX - output.position.x,
      globalY - output.position.y,
    );
    return hit === null ? undefined : placed[hit].window;
  }

  function visiblePlanes(session: Session, width: number, height: number, output: OutputInfo) {
    const alive = new Set(wm.listWindows().map((window) => window.id));
    return poses(session, width, height, output)
      .filter(({ window, pose }) => alive.has(window.id) && pose.opacity > 0.001)
      .map(({ window, rect, pose }) => ({
        window,
        rect,
        pose,
        planeWidth: rect.width + MARGIN * 2,
        planeHeight: rect.height + MARGIN * 2,
        transform: transform3d()
          .translate(pose.x, pose.y, pose.z)
          .rotateX(pose.rotateX)
          .rotateY(pose.rotateY)
          .scale(pose.scale),
      }));
  }

  function windowTexture(window: WaylandWindow, rect: Rect, output: OutputInfo) {
    return renderTexture({
      key: `flip-3d-${window.id}`,
      width: rect.width + MARGIN * 2,
      height: rect.height + MARGIN * 2,
      content: (
        <Windows
          windows={[window]}
          offsetX={output.position.x - rect.x + MARGIN}
          offsetY={output.position.y - rect.y + MARGIN}
        />
      ),
    });
  }

  function compose(output: OutputInfo): CompositionRenderable | null {
    const session = current();
    if (!session || session.output !== output.name) {
      return null;
    }
    revision();
    const { width, height } = outputLogicalSize(output);
    const planes = visiblePlanes(session, width, height, output);
    return (
      <>
        <Layers layers={["background", "bottom"]} />
        <Solid color={[0, 0, 0, dimAlpha(session)]} />
        <Scene3D camera={flipCamera(width, height)}>
          {planes.map(({ window, rect, pose, planeWidth, planeHeight, transform }) => (
            <Plane
              texture={windowTexture(window, rect, output)}
              width={planeWidth}
              height={planeHeight}
              transform={transform}
              opacity={pose.opacity}
            />
          ))}
        </Scene3D>
        <Layers layers={["top", "overlay"]} />
        <LayerPopups />
      </>
    );
  }

  return {
    open,
    compose,
    blurSuspended,
    backdropStrength,
    selectedWindowId,
  };
}

/** Distance at which the camera sees the plane z = 0 1:1 in logical pixels. */
function cameraDistance(height: number): number {
  return height / 2 / Math.tan((CAMERA_FOV_DEGREES * Math.PI) / 360);
}

/**
 * Like `screenCamera` with a narrow field of view, but with room for a stack
 * that recedes several times the camera distance.
 */
function flipCamera(width: number, height: number): Camera {
  const distance = cameraDistance(height);
  return {
    projection: perspective(CAMERA_FOV_DEGREES, width / Math.max(height, 1), distance / 100, distance * 6),
    view: lookAt([0, 0, distance], [0, 0, 0]),
  };
}

function lerp(from: number, to: number, t: number): number {
  return from + (to - from) * t;
}

function lerpPose(from: Pose, to: Pose, t: number): Pose {
  return {
    x: lerp(from.x, to.x, t),
    y: lerp(from.y, to.y, t),
    z: lerp(from.z, to.z, t),
    rotateX: lerp(from.rotateX, to.rotateX, t),
    rotateY: lerp(from.rotateY, to.rotateY, t),
    scale: lerp(from.scale, to.scale, t),
    opacity: lerp(from.opacity, to.opacity, t),
  };
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}

function mod(value: number, divisor: number): number {
  return ((value % divisor) + divisor) % divisor;
}
