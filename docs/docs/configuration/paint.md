---
sidebar_position: 8.5
---

# Paint shaders

Every server-side decoration is drawn by a shader. Paint shaders let you
replace that drawing for one component while ShojiWM keeps doing the hard
part: the layout is resolved to **whole physical pixels** first (fractional
scaling, per-side rounding, the window's sub-pixel position, clipping), and
your shader only decides the color of each pixel.

```tsx
import {Box, paintShader} from 'shoji_wm';

const gradientBorder = paintShader('./shaders/gradient-border.frag');

<WindowBorder
  paint={gradientBorder}
  style={{border: {px: 1.5, color: '#7aa2f7'}, borderRadius: 12, background: '#1e1e2e'}}
>
  ...
</WindowBorder>
```

Every component accepts two paint slots:

| Prop | Drawn | Use it for |
| --- | --- | --- |
| `paint` | Instead of the built-in background and border | Gradients, patterns, custom borders |
| `overlay` | Above the component's children | Focus rings, sheens, glows |

The built-in background and border are themselves a paint shader written
against the same contract, so a custom `paint` starts from exactly the
geometry the built-in look uses.

## Pixel units: `phy_px`

Everything a shader receives is in **physical pixels**, and the names say so
with a `_phy_px` suffix (`size_phy_px`, `border_phy_px`, ...). Styles in your
config stay in logical pixels as usual: `borderRadius: 12` at 1.5x arrives in
the shader as `radius_phy_px = 18`. Values are whole numbers; the centre of a
pixel is at `.5`, so an edge on the pixel grid comes out perfectly sharp.

## The `paint_main` contract

A paint shader is a GLSL file with one function:

```glsl
vec4 paint_main(PaintContext ctx) {
    // Return this pixel's color, premultiplied by its alpha.
    return ctx.background * shoji_outer_coverage(ctx);
}
```

The returned color is **premultiplied** (`rgb * a, a`). The context colors
already are, so multiplying a context color by a coverage value is correct;
for your own colors use `shoji_premultiply(vec4(r, g, b, a))`.

```glsl
struct PaintContext {
    vec2 frag_phy_px;          // this pixel, from the component's top-left
    vec2 size_phy_px;          // the component's border box
    vec4 border_phy_px;        // top, right, bottom, left
    vec4 radius_phy_px;        // top-left, top-right, bottom-right, bottom-left
    vec4 padding_phy_px;       // top, right, bottom, left
    vec4 content_rect_phy_px;  // inside border and padding: x, y, width, height
    vec4 inner_rect_phy_px;    // the inner edge of the border
    vec4 inner_radius_phy_px;
    bool has_hole;             // <WindowBorder>: the inner area shows the client
    vec4 background;           // style.background, premultiplied
    vec4 border_color;         // style.border color, premultiplied
    float scale;               // physical pixels per logical pixel
};
```

`opacity`, the clip of rounded ancestors and the window's own fade are applied
by ShojiWM after `paint_main` returns.

### Helpers

| Helper | Result |
| --- | --- |
| `shoji_rrect_sdf(p, rect, radius)` | Signed distance to a rounded rect (negative inside) |
| `shoji_coverage(sdf)` | Pixel coverage of a distance, antialiased over one physical pixel |
| `shoji_rrect_coverage(p, rect, radius)` | Coverage of a rounded rect |
| `shoji_blurred_rrect_coverage(p, rect, radius, sigma)` | Coverage of a Gaussian-blurred rounded rect |
| `shoji_outer_coverage(ctx)` | Coverage of the border box |
| `shoji_inner_coverage(ctx)` | Coverage of the area inside the border |
| `shoji_border_coverage(ctx)` | Coverage of the border ring |
| `shoji_fill_coverage(ctx)` | Where the background goes (minus the client area of a `<WindowBorder>`) |
| `shoji_border_color_at(ctx)` | The border color of the side this pixel belongs to |
| `shoji_default_paint(ctx)` | The built-in background + border |
| `shoji_over(src, dst)` | `src` over `dst`, premultiplied |
| `shoji_premultiply(color)` | Premultiply a straight-alpha color |

Rects are `(x, y, width, height)`; radii are top-left, top-right,
bottom-right, bottom-left.

## Examples

A gradient border over the regular background:

```glsl
// shaders/gradient-border.frag
vec4 paint_main(PaintContext ctx) {
    vec2 uv = ctx.frag_phy_px / ctx.size_phy_px;
    vec4 from = vec4(0.48, 0.64, 0.97, 1.0);
    vec4 to = vec4(0.95, 0.55, 0.66, 1.0);
    vec4 border = mix(from, to, uv.x * 0.7 + uv.y * 0.3) * shoji_border_coverage(ctx);
    vec4 fill = ctx.background * shoji_fill_coverage(ctx);
    return shoji_over(border, fill);
}
```

A glow drawn around a component, in the overlay layer. `outsets` gives the
shader room outside the component (logical pixels):

```glsl
// shaders/glow.frag
uniform float strength;

vec4 paint_main(PaintContext ctx) {
    float d = shoji_rrect_sdf(ctx.frag_phy_px, vec4(vec2(0.0), ctx.size_phy_px), ctx.radius_phy_px);
    float glow = d > 0.0 ? strength * exp(-d / (3.0 * ctx.scale)) : 0.0;
    return vec4(0.48, 0.64, 0.97, 1.0) * glow;
}
```

```tsx
// inside a composition function
const [hover, setHover] = useState(false);
const glow = paintShader('./shaders/glow.frag', {
  uniforms: {strength: hover((on) => (on ? 0.7 : 0))},
  outsets: 16,
});

<Box overlay={glow} onHoverChange={setHover} style={{borderRadius: 8}} />
```

## Uniforms and animation

`uniforms` takes the same values as effect stages: numbers, 2–4 component
arrays and `uniformArray(...)`, each of which may be a signal. When only a
uniform changes, ShojiWM repaints the component without laying the window out
again.

There is no automatic `time` uniform: a paint shader is repainted only when
its geometry or a uniform changes. To animate, drive a uniform from a signal
(an [animation](./animations.md) value, for example).

Uniform names starting with `shoji_`, and `alpha`, `size` and `tex`, are
reserved.

## Shadows: `boxShadow`

The CSS-style `boxShadow` is built in, so most shadows need no shader:

```tsx
<WindowBorder
  style={{
    borderRadius: 12,
    boxShadow: [
      {y: 10, blur: 28, color: '#000000b0'},
      {blur: 8, spread: -2, color: '#ffffff30', inset: true},
    ],
  }}
/>
```

| Field | Meaning |
| --- | --- |
| `x`, `y` | Offset (logical pixels) |
| `blur` | Blur radius, CSS semantics |
| `spread` | Grows (positive) or shrinks (negative) the shadow |
| `color` | Shadow color |
| `inset` | Draw inside the padding box instead of around the component |

The first entry is drawn on top. An outer shadow is never drawn below the
component itself, and an inset shadow never covers the border.

## Paint order

Inside one component, back to front:

1. outer `boxShadow`s
2. `paint` (or the built-in background and border)
3. inset `boxShadow`s
4. label / icon content, then the children
5. `overlay`

On a [`<ShaderEffect/>`](./components.md#shadereffect) the effect output sits
between the built-in background and the border; a custom `paint` is drawn over
the effect output (and the inset shadows go between the effect and `paint`).

## Errors

A paint shader that fails to compile is reported like an effect shader error,
and the component falls back to the built-in look until the file is fixed.

## Rust

The Rust config SDK has the same API:

```rust
use shojiwm_rs::prelude::*;

let hover = signal(0.0);
let glow = paint_shader("shaders/glow.frag").uniform("strength", hover).outsets(16.0);
Flex::row()
    .overlay(glow)
    .style(Style::new().border_radius(8.0).box_shadow(shadow(0.0, 10.0, 28.0, hex("#000000b0"))));
```
