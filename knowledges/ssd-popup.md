# SSD popups (`<Popup>`)

Decoration subtrees anchored to their parent node but drawn outside the
window (tooltips, menus with buttons). User docs:
`docs/docs/configuration/components.md#popup`.

## Data flow

```
TS <Popup open placement align offset collision layer>   (index.ts / types.ts)
Rust SDK Popup::new().open(..).placement(..)            (shojiwm_rs view.rs)
  │  bridge.rs parse_popup()
  ▼
DecorationNodeKind::Popup(PopupNode)                     (ssd/popup.rs)
  │  layout (ssd/mod.rs): out of flow, sized by content (column), placed by
  │  layout_popup() → popup::popup_rect() next to the parent's border box,
  │  no inherited clip; computed_style(): pointer_events none for `hint`,
  │  visible=false while closed
  ▼
paint / text / icon buffers as usual (integration.rs); collect_cached_buffers
resets the clip state at the popup and emits no shader buffers inside it
  │
  ▼
backends: PopupScopes (buffer key prefix → layer) splits every window's
buffers into DecorationPart::Window (the existing paths, which now filter
popups out) and DecorationPart::Popups(layer) (decoration.rs
popup_elements_for_decoration)
```

## Where the popup pass is drawn

`ssd_popup_scene_elements` in `backend/tty.rs` and `backend/winit.rs`:

- `layer: "top"`: right after the upper layer-shell elements (so above every
  window and closing snapshot, below layer-shell top/overlay), skipped while
  the fullscreen fast path is active.
- `layer: "window"`: in the window loop, just before the window's own
  elements (in-front effects); skipped in full-window-snapshot mode.

Elements are relocated by the window's root physical origin. A window whose
visual transform is not identity (open/close/minimize/scale animations) draws
no popups.

## Flip / collision

`popup::set_popup_viewports` holds the output rects (thread-local, set at
the start of `refresh_window_decorations_for_output`). The layout picks the
output under the anchor's center and flips / slides the popup in root-local
pixels. A window moved without relayout (`translated()`) keeps the old
placement until its next relayout.

## Things to know

- `open` is excluded from `kind_layout_equivalent`: closed popups are laid out,
  so toggling `open` takes the `reapply_tree_preserving_layout` path.
- Popups are excluded from `bounds_rect` / `resolved_layout_bounds_rect`, so
  they never change the window's size, input region or snapshot bounds.
- Opening a popup inserts its nodes into the paint order and renumbers the
  nodes after it. `merge_*_buffers` therefore re-reads the order of every
  buffer they keep from the new order map; before, kept labels could end up
  behind a rebuilt translucent background (text looked grey).
- Dirty-scope matching (`is_descendant_node_id`) accepts both `.` (TS ids)
  and `/` (Rust SDK ids); before popups, Rust SDK node patches without a
  relayout did not rebuild the children's buffers.

## Input (modes, triggers, light dismiss)

Model: HTML popover. `PopupMode::{Hint, Auto, Manual}`; the open state stays
in the config, the compositor only reports events through `PopupHandlers`
(on `DecorationInteractionHandlers::popup`, wire props `onInterestChange`,
`onAnchorPress`, `onOpenChange` = one handler id per `PopupDismissReason`).

- Hit testing (`ssd/mod.rs`): `hit_test_at` / `interaction_targets_at_precise`
  check `popup::popup_hit` first (innermost interactive popup); the regular
  traversals skip popup children. `DecorationHitTestResult::Popup` = on a
  popup, no button (swallowed). Inside a popup the hover targets are the DOM
  chain (`popup::hover_chain`): nodes under the pointer + all ancestors with
  handlers; `input.rs` keeps `decoration_hover_targets` as a set and diffs it.
- Which window (`ssd/popup_input.rs` `ssd_popup_window_under`): `Top`
  popups of any window, then `Window` popups down to the first window whose
  root covers the point; only untransformed windows. `state.rs`
  `surface_under` returns `None` there (after the upper layer-shell layers),
  so clients under a popup lose the pointer; `decoration_under` /
  `decoration_interaction_targets_under` route to the popup's window.
- `PopupInfo` (`popup::popups`): anchor rect, areas (own bounds + open nested
  interactive popups), enclosing popup ids; nested popups inherit the
  outermost layer.
- Interest (`update_popup_interest`, from `update_decoration_hover_target`):
  pointer on the anchor, or on an open interactive popup, of the decoration
  window under the pointer. Anchor press + outside press:
  `handle_popup_press` at every pointer press. Escape: keyboard filter →
  `KeyboardAction::DismissPopup` when `popup_takes_escape()` (most recently
  opened `Auto` popup with `close_on_escape` and dismiss handlers).
- `sync_popup_dismissals` (end of `refresh_window_decorations_for_output`):
  tracks open `Auto` popups in opening order; `dismissals()` (pure, tested)
  asks hidden windows' popups (`anchor-gone`) and older popups replaced by a
  new non-nesting one (`other-popup`) to close; these automatic requests are
  sent once per open popup.
- `trigger` lives in the SDKs: TS `Popup` function component (`index.ts`,
  `createPoll` delays), Rust `Element::trigger` (`TriggerState`, `set_timeout`).

## Tests

- `ssd::popup::tests` (placement math, flip, scope matching).
- `ssd::tests::popup_*`, `closed_popup_is_neither_drawn_nor_in_a_scope`.
- `ssd::integration::tests::dirty_scope_covers_descendants_in_both_id_styles`.
- `shoji_wm` `embedded_runtime_sends_popups_with_their_placement`.
- `shojiwm_rs/tests/reactive_runtime.rs` `hovering_a_button_opens_its_popup`,
  `popup_triggers_follow_compositor_events`.
- `ssd::tests::interactive_popup_takes_input_and_keeps_its_anchor_hovered`,
  `popup_info_reports_areas_anchor_interest_and_nesting`,
  `ssd::popup_input::tests` (dismissal decisions),
  `shoji_wm` `embedded_runtime_popup_triggers_follow_compositor_events`.
- Not covered by tests: the `ShojiWM` glue in `popup_input.rs` (needs a
  running compositor).

## Not done yet

- Keyboard navigation inside popups (focus, arrow keys).
- A slide-only `collision: "shift"` mode, and keeping a closing popup drawn
  until its fade-out ends (today `open={false}` hides it at once).
- Drawing popups during window transform animations.
- `<ShaderEffect>` effects inside popups (backdrop sampling).
- Output-level SSD layers (OSDs) built on the same pass.
