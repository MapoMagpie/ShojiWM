---
sidebar_position: 10.62
---

# Output composition recipes

Complete effects built from [output composition](./output-composition.md). Each one
is a function that returns `null` while the effect is idle; fall back to the default
stacking then, which also keeps the fullscreen fast path:

```tsx
COMPOSITOR.rendering.composition = (output) =>
  crossfade(output) ?? <DefaultComposition />;
```

Return the default rather than `null` from the composition function itself: a
composition with no nodes draws nothing.

All names below are exported from `shoji_wm`.

## A cube between any two views

`examples/output-composition/cube-transition.tsx` turns a cube between any two pieces
of composition, e.g. two workspaces:

```tsx
import { COMPOSITOR, DefaultComposition, Layers, Windows } from "shoji_wm";
import { createCubeTransition } from "./cube-transition";

const cube = createCubeTransition({ duration: 700 });
COMPOSITOR.rendering.composition = (output) =>
  cube.compose(output) ?? <DefaultComposition />;

// When switching (switch the windows themselves without animation):
cube.start(outputName, {
  from: <><Layers layers={["background", "bottom"]} /><Windows windows={fromIds} /></>,
  to: <><Layers layers={["background", "bottom"]} /><Windows windows={toIds} /></>,
  direction: 1,
});
```

## Dimming everything but the bars

No texture needed: put a translucent fill between the windows and the bars.

```tsx
const [dimmed, setDimmed] = signal(false);

COMPOSITOR.rendering.composition = () => (
  <>
    <Layers layers={["background", "bottom"]} />
    <Windows />
    {dimmed() && <Solid color="#00000099" />}
    <Layers layers={["top", "overlay"]} />
    <LayerPopups />
  </>
);
```

`<Windows />` stays at the root, so the fullscreen fast path still works while the
fill is off.

## Crossfade

Fade from a snapshot of one view to another with two texture views:

```tsx
const [fade, setFade] = signal<number | null>(null); // 0 → 1

function crossfade(output: OutputInfo) {
  const t = fade();
  if (t === null) return null;
  const from = renderTexture({ key: "fade-from", content: <Windows windows={fromIds} /> });
  const to = renderTexture({ key: "fade-to", content: <Windows windows={toIds} /> });
  return (
    <>
      <Layers layers={["background", "bottom"]} />
      <TextureView texture={from} opacity={1 - t} />
      <TextureView texture={to} opacity={t} />
      <Layers layers={["top", "overlay"]} />
      <LayerPopups />
    </>
  );
}
```

Drive `fade` from a `createPoll` on the output and set it back to `null` at the end.

## Zoom

Zoom into a point of the screen by scaling a texture of the desktop about it. The 3D
scene clips the plane to the output, so any zoom level works:

```tsx
function zoomed(output: OutputInfo) {
  const k = zoom(); // 1 = no zoom
  if (k <= 1) return null;
  const { width, height } = outputLogicalSize(output);
  const desktop = renderTexture({ key: "zoom", content: <DefaultComposition />, scale: 2 });
  // The focus point in world coordinates (origin at the centre, +Y up).
  const cx = focusX() - width / 2;
  const cy = height / 2 - focusY();
  return (
    <Scene3D camera={screenCamera(output)}>
      <Plane texture={desktop} width={width} height={height}
        transform={transform3d().translate(cx * (1 - k), cy * (1 - k)).scale(k)} />
    </Scene3D>
  );
}
```

`scale: 2` renders the texture at twice the pixel density, so text stays sharp up to
2x. Input is not zoomed: it still goes to the windows where they really are.

## Per-window textures

A texture can frame a single window: size it to the window and shift the window to
the texture's corner with `offsetX`/`offsetY`. Leave a margin for shadows.

```tsx
const MARGIN = 48;

function windowTexture(output: OutputInfo, window: WaylandWindow) {
  const { x, y, width, height } = window.position;
  return renderTexture({
    key: `window-${window.id}`,
    width: width + MARGIN * 2,
    height: height + MARGIN * 2,
    content: (
      <Windows
        windows={[window]}
        offsetX={output.position.x - x + MARGIN}
        offsetY={output.position.y - y + MARGIN}
      />
    ),
  });
}
```

Each such texture redraws only when its window changes, so a dozen of them cost little
while they stand still. Picked by id, the window shows even when it is minimized or on
another workspace.

### A 3D window switcher

With one texture per window, a Windows Vista "Flip 3D" style switcher is a row of
planes:

```tsx
const [selected, setSelected] = signal<number | null>(null);

function flipComposition(output: OutputInfo, windows: WaylandWindow[]) {
  const index = selected();
  if (index === null || windows.length === 0) return null;
  const { height } = outputLogicalSize(output);
  // The selected window in front, the next ones further back.
  const ordered = windows.map((_, i) => windows[(index + i) % windows.length]);
  return (
    <>
      <Layers layers={["background", "bottom"]} />
      <Scene3D camera={screenCamera(output, { distance: height * 0.6 })}>
        {ordered.map((window, depth) => (
          <Plane
            texture={windowTexture(output, window)}
            width={window.position.width + MARGIN * 2}
            height={window.position.height + MARGIN * 2}
            transform={transform3d()
              .translate(depth * 90 - 150, depth * 40, -depth * 260)
              .rotateY(-30)}
          />
        ))}
      </Scene3D>
      <Layers layers={["top", "overlay"]} />
      <LayerPopups />
    </>
  );
}

COMPOSITOR.key.bind("flip-next", "Super+Tab", () =>
  setSelected(((selected.peek() ?? -1) + 1) % Math.max(1, myWindows().length)));
COMPOSITOR.key.bind("flip-pick", "Super+Return", () => {
  const index = selected.peek();
  if (index !== null) myWindows()[index]?.focus();
  setSelected(null);
});
```

Use it like the others: `flipComposition(output, myWindows()) ?? <DefaultComposition />`.
`myWindows()` is your window list, e.g. the current workspace's windows from your
window manager. Animate the row by easing the translation from a `createPoll`.

While the switcher is shown, the mouse still reaches the windows where they really
are; drive it from the keyboard.

**Blur inside per-window textures.** A window alone in its texture has nothing below
it, so a backdrop blur on it (a glass titlebar, say) blurs empty space while still
costing a blur per window. Switch those windows to an effect without a backdrop while
the switcher is open, by reading the same signal where you pick the effect:

```ts
COMPOSITOR.effect.window = (window) =>
  selected() !== null ? { behind: TINT } : { behind: GLASS };
```
