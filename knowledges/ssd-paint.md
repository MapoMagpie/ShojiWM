# SSD paint pipeline

How decoration nodes draw themselves (background, border, `boxShadow`,
`paint` / `overlay` shaders). User docs: `docs/docs/configuration/paint.md`.

## Data flow

```
DecorationStyle.{background, border*, box_shadow, paint, overlay}
  │  layout (ssd/mod.rs): whole physical px, root-local (LayoutFrame)
  ▼
ssd/paint.rs   back_slots()/has_overlay() → PaintSlot keys
               node_geometry() → PaintGeometry (PxRect, per-side border,
                                  radii, padding, content, inner edge, clips)
               paint_item()   → PaintItem { program, geometry, colors, uniforms }
  │  ssd/integration.rs collect_cached_buffers(): one CachedDecorationBuffer
  │  per slot, ordered by collect_render_orders() (same slot keys)
  ▼
backend/paint.rs  paint_element(): LayoutToOutput maps root-local px to the
                  output (identity at the layout scale), StablePaintElement
                  = one render_pixel_shader_to quad
```

The old `rounded.rs` / `rounded_rect.frag` path and the logical→physical
re-fitting in `decoration.rs` (border_fit anchors, paired border geometry)
are gone: the renderer never re-snaps, it draws `PaintGeometry` as is.

## One contract for built-ins and users

`paint_prelude.glsl` defines the uniforms, `PaintContext`, and helpers.
Every program is `prelude + source + main()`; `main()` applies opacity,
the element alpha and the two clips (inherited clip + nearest rounded
ancestor clip). Built-ins (`paint_builtin_box.frag`,
`paint_builtin_shadow.frag`) are written against the same `paint_main`
contract as user shaders. All programs declare the same `shoji_*` uniform
set (smithay rejects unknown uniforms but tolerates optimised-out ones).
A user shader that fails to compile falls back to `shoji_default_paint`
through `compile_shader_or_fallback` (same error overlay as effects).

Output is premultiplied. Coverage is `clamp(0.5 - sdf, 0, 1)`, so edges on
the integer grid are exact (pixel centres at .5).

## Order per node (front → back keys)

`overlay`, label/icon, children, then for an ordinary node
`inset-shadow-*`, `box` | `paint`, `shadow-*`; for a `<ShaderEffect>`
`paint` | `border`, `inset-shadow-*`, `shader`, `fill`, `shadow-*`.

## Uniform fast path

`paint` / `overlay` uniforms use `CompositionPatch::ShaderUniform` with
`PAINT_STAGE_INDEX` (u32::MAX-1) / `OVERLAY_STAGE_INDEX` (u32::MAX-2)
(`runtime_api/message.rs`). `apply_shader_uniform_fast_update` updates
the tree, the computed tree and the matching `:paint` / `:overlay`
buffers without relayout. TS: `serialize.ts` registers the bindings;
Rust SDK: `view.rs` `paint_shaders()`.

## Tests

- `backend/paint.rs` tests: GPU readback probes (borders on whole pixels
  at 1.5x, per-side borders, rounded cut + spread shadow, blur/inset
  shadow, custom shader sees layout pixels at 1.25x).
- `shoji_wm` evaluator test `embedded_runtime_sends_paint_shaders_and_patches_their_uniforms`.
- `shojiwm_rs/tests/reactive_runtime.rs` (overlay uniform patch, box_shadow).

## Not done yet

- Per-component post effects on the final image (render a subtree
  offscreen, then run an effect pipeline over it).
- Paint for layer / popup surfaces (fancy popups).
- Unverified: whether a `boxShadow` reaching outside the window root
  survives the snapshot-based close / minimize animations.

## EffectContext frame shape

Effects get the same kind of shape data: `effect.frame_radius_phy_px`,
`frame_border_phy_px`, `clip_rect_phy_px` / `clip_radius_phy_px` /
`has_clip` and `scale`. The pipeline carries an `EffectFrame`
(`backend/shader_effect.rs`, resolved per run into `ResolvedEffectFrame`,
rescaled for `renderTo` state sizes). Sources: SSD `<ShaderEffect>` nodes
store a `NodeEffectShape` (layout px) on `CachedShaderEffect`; window slots
use `WindowDecorationState::effect_frame_shape` (the `<WindowBorder>`, or
its inner edge for root-surface sources); layers / popups / background get
`EffectFrame::plain` (radius 0, output scale). Built-in display programs
leave the uniforms at 0 (`scale` falls back to 1 in GLSL).

