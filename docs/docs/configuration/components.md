---
sidebar_position: 8
---

# SSD Components

These are the building blocks you assemble inside
[`COMPOSITOR.window.composition`](./window-composition.md) to draw window
decorations. They are imported from `shoji_wm`:

```tsx
import {Box, Label, Button, AppIcon, Image, ShaderEffect, WindowBorder} from 'shoji_wm';
```

`<ManagedWindow/>` and `<ClientWindow/>` are documented on the
[Window composition](./window-composition.md) page.

## Common props

Every component accepts these (from `ComponentProps`):

| Prop | Type | Meaning |
| --- | --- | --- |
| `children` | nodes | Child components |
| `style` | `SSDStyle` | Visual styling — see [Style reference](#style-reference) |
| `id` | `string` | Stable node id for targeted invalidation |
| `onHoverChange` | `(hovered: boolean) => void` | Pointer enter/leave |
| `onActiveChange` | `(active: boolean) => void` | Press/release |
| `paint` | `PaintShaderHandle` | Replaces the built-in background/border — see [Paint shaders](./paint.md) |
| `overlay` | `PaintShaderHandle` | Painted above the children — see [Paint shaders](./paint.md) |

All `style` values (and most props) accept either a plain value or a signal, so
they update reactively.

---

## `<Box/>`

A flexbox-like container that arranges children horizontally or vertically.

| Prop | Type | Meaning |
| --- | --- | --- |
| `direction` | `"row" \| "column" \| "horizontal" \| "vertical"` | Layout axis (default `"row"`) |
| `split` | `Direction` | Split direction for two-panel layouts |
| `style` | `SSDStyle` | Styling |

```tsx
<Box direction="row" style={{gap: 8, padding: 4, alignItems: 'center'}}>
  <AppIcon icon={window.icon} style={{width: 16, height: 16}} />
  <Label text={window.title} style={{flexGrow: 1}} />
</Box>
```

## `<Label/>`

Renders a text string.

| Prop | Type | Meaning |
| --- | --- | --- |
| `text` | `string` (or signal) | The text to display |
| `style` | `SSDStyle` | Font and color via `fontSize`, `fontWeight`, `fontFamily`, `color`, `textAlign`, `lineHeight` |

```tsx
<Label
  text={window.title}
  style={{color: '#f5f7fa', fontSize: 13, fontWeight: 600, fontFamily: ['Noto Sans CJK JP', 'Noto Color Emoji']}}
/>
```

## `<Button/>`

A pressable region that triggers an action on click.

| Prop | Type | Meaning |
| --- | --- | --- |
| `onClick` | `() => void` or `WindowActionDescriptor` | Action on click |
| `onHoverChange` | `(hovered: boolean) => void` | Track hover for visual feedback |
| `style` | `SSDStyle` | Styling |

`onClick` accepts either a callback, or a descriptor from `windowAction(...)`
for the built-in window operations: `"close"`, `"maximize"`, `"unmaximize"`,
`"minimize"`, `"fullscreen"`, `"unfullscreen"`.

```tsx
import {Button, windowAction} from 'shoji_wm';

// Built-in action
<Button onClick={windowAction('close')} style={{width: 12, height: 12}} />

// Custom handler + hover feedback
const [hover, setHover] = useState(false);
<Button
  onHoverChange={setHover}
  onClick={() => window.minimize()}
  style={{width: 16, height: 16, borderRadius: 8, background: hover((h) => h ? '#FFFFFF40' : '#FFFFFF20')}}
/>
```

## `<AppIcon/>`

Renders a window's application icon.

| Prop | Type | Meaning |
| --- | --- | --- |
| `icon` | `WindowIcon \| undefined` (or signal) | Pass `window.icon` for reactive updates |
| `style` | `SSDStyle` | Sizing |

```tsx
<AppIcon icon={window.icon} style={{width: 16, height: 16}} />
```

## `<Image/>`

Displays an image from a file path (resolved relative to the config package
root) or a reactive source.

| Prop | Type | Meaning |
| --- | --- | --- |
| `src` | `string` (or signal) | Image path |
| `fit` | `"contain" \| "cover" \| "fill"` | How the image fills its box |
| `style` | `SSDStyle` | Sizing/positioning |

```tsx
<Image src="./assets/x.svg" style={{width: 16, height: 16, pointerEvents: 'none'}} />
```

## ShaderEffect

`<ShaderEffect/>` is a container that applies a compiled GPU effect to the region
its children occupy. See [Effects](./effects.md) for building the `shader`.

| Prop | Type | Meaning |
| --- | --- | --- |
| `shader` | `CompiledEffectHandle` | The compiled effect to render |
| `direction` | `Direction` | Layout axis for children (like `<Box/>`) |
| `style` | `SSDStyle` | Styling |

```tsx
<ShaderEffect shader={frostedGlass} direction="row" style={{height: 28, paddingX: 8, alignItems: 'center'}}>
  <Label text={window.title} />
</ShaderEffect>
```

## WindowBorder

`<WindowBorder/>` is a chrome container placed around `<ClientWindow/>` that draws
the border and provides interactive resize hit areas.

| Prop | Type | Meaning |
| --- | --- | --- |
| `style` | `SSDStyle` | Border via `border`, `borderRadius`, `background`, etc. |
| `interaction` | `WindowBorderInteraction` | Resize hit areas |

`interaction.resizeHitArea` is either a single number, or
`{edgePx?, cornerPx?}` — the grab thickness along edges and in corners.

```tsx
<WindowBorder
  style={{border: {px: 2, color: borderColor}, borderRadius: 10}}
  interaction={{resizeHitArea: {edgePx: 8, cornerPx: 14}}}
>
  <ClientWindow />
</WindowBorder>
```

---

## Style reference

The `style` prop is an `SSDStyle`. Every value may be a signal. Lengths are in
logical pixels unless noted.

### Sizing

`width`, `height` (number or string like `"100%"`), `minWidth`, `minHeight`,
`maxWidth`, `maxHeight`, `flexGrow`, `flexShrink`.

### Spacing

`gap`, `padding`, `paddingX`, `paddingY`, `paddingTop/Right/Bottom/Left`,
`margin`, `marginX`, `marginY`, `marginTop/Right/Bottom/Left`.

### Layout & position

`alignItems` (`"start" | "center" | "end" | "stretch"`), `justifyContent`
(`"start" | "center" | "end" | "space-between"`), `position`
(`"relative" | "absolute"`), `inset`, `top`, `right`, `bottom`, `left`,
`zIndex`, `overflow` (`"visible" | "hidden"`), `pointerEvents`
(`"auto" | "none"`), `transform` (`{translateX, translateY, scale, scaleX, scaleY}`).

### Appearance

`background`, `color`, `opacity`, `visible`, `cursor`, `borderRadius`.

Borders: `border`, `borderTop`, `borderRight`, `borderBottom`, `borderLeft` —
each a `{px, color}` value — plus `borderFit` (`"normal" | "fit-children"`).
A side value overrides `border` for that side, both in width (the content box
moves accordingly) and color. Any non-zero width covers at least one physical
pixel.

Shadows: `boxShadow` — one `{x, y, blur, spread, color, inset}` value or an
array of them, CSS semantics. See [Paint shaders](./paint.md#shadows-boxshadow).

Colors are `#RGB`, `#RGBA`, `#RRGGBB` or `#RRGGBBAA`.

### Text (for `<Label/>`)

`fontSize`, `fontWeight` (`"normal" | "medium" | "semibold" | "bold"` or a
number), `fontFamily` (string or string array of fallbacks), `textAlign`
(`"start" | "center" | "end"`), `lineHeight`.

```tsx
const style: SSDStyle = {
  height: 28,
  paddingX: 8,
  gap: 8,
  alignItems: 'center',
  background: window.isFocused((f) => (f ? '#1f2430cc' : '#2a2f3acc')),
  borderRadius: 8,
};
```
