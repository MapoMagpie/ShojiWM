//! Pointer and keyboard input of SSD `<Popup>`s (see `ssd::popup`).
//!
//! The compositor never opens or closes a popup itself; it tells the config
//! what happened and the config updates its `open` signal:
//!
//! - interest (`interestfor`): the pointer entered / left a popup's anchor
//!   or, for an interactive popup, the popup itself;
//! - anchor press: a press on a popup's anchor;
//! - dismiss (light dismiss of `Auto` popups): a press outside, Escape, the
//!   window going away, or another `Auto` popup opening.

use std::collections::HashSet;

use smithay::{
    desktop::Window,
    utils::{Logical, Point},
};

use super::{
    DecorationStateChangeHandler, LogicalPoint, PopupDismissReason, PopupInfo, PopupLayer,
    PopupMode, WindowDecorationState, WindowTransform,
};
use crate::state::ShojiWM;

/// Per-compositor popup input bookkeeping.
#[derive(Default)]
pub struct PopupInputState {
    /// Open `Auto` popups as (window id, node id), oldest first.
    open_auto: Vec<(String, String)>,
    /// Close requests already sent for a reason the user did not cause
    /// (`AnchorGone`, `OtherPopup`), so a config that keeps the popup open is
    /// not asked again on every frame.
    requested: HashSet<(String, String, PopupDismissReason)>,
    /// Popups the pointer currently shows interest in.
    interest: Vec<PopupInterest>,
}

struct PopupInterest {
    window: Window,
    window_id: String,
    node_id: String,
    handler: DecorationStateChangeHandler,
}

/// A runtime handler to invoke on behalf of a popup.
struct PopupRequest {
    window: Window,
    window_id: String,
    handler: String,
}

/// Popups are drawn only while their window sits still.
fn is_still(transform: WindowTransform) -> bool {
    transform.translate_x.abs() < 1e-3
        && transform.translate_y.abs() < 1e-3
        && (transform.scale_x - 1.0).abs() < 1e-3
        && (transform.scale_y - 1.0).abs() < 1e-3
}

fn pointer_on(popup: &PopupInfo, layer: PopupLayer, pos: Point<f64, Logical>) -> bool {
    popup.popup.layer == layer && popup.is_open_and_interactive() && popup.contains(pos.x, pos.y)
}

impl ShojiWM {
    fn decoration_takes_popup_input(
        &self,
        decoration: &WindowDecorationState,
        output_name: Option<&str>,
    ) -> bool {
        is_still(decoration.visual_transform)
            && output_name.map_or_else(
                || decoration.managed_window_allows_input(),
                |output| decoration.managed_window_allows_input_on_output(output),
            )
    }

    /// The window whose interactive popup is under `pos`: `Top` popups of any
    /// window first, then `Window` popups down to the first window whose own
    /// decoration covers `pos`. Such a popup hides the client surfaces under
    /// it from the pointer.
    pub(crate) fn ssd_popup_window_under(&self, pos: Point<f64, Logical>) -> Option<Window> {
        if !self
            .window_decorations
            .values()
            .any(|decoration| decoration.has_open_interactive_popup())
        {
            return None;
        }
        let output_name = self.output_name_at_point(pos);
        let windows = self.windows_top_to_bottom();
        let eligible = |window: &Window| {
            self.window_decorations.get(window).filter(|decoration| {
                self.decoration_takes_popup_input(decoration, output_name.as_deref())
            })
        };
        for window in &windows {
            if let Some(decoration) = eligible(window)
                && decoration
                    .layout
                    .popups()
                    .iter()
                    .any(|popup| pointer_on(popup, PopupLayer::Top, pos))
            {
                return Some((*window).clone());
            }
        }
        let logical_pos = LogicalPoint::new(pos.x.floor() as i32, pos.y.floor() as i32);
        for window in &windows {
            let Some(decoration) = self.window_decorations.get(*window) else {
                continue;
            };
            if eligible(window).is_some()
                && decoration
                    .layout
                    .popups()
                    .iter()
                    .any(|popup| pointer_on(popup, PopupLayer::Window, pos))
            {
                return Some((*window).clone());
            }
            if crate::backend::visual::transformed_root_rect(
                decoration.layout.root.rect,
                decoration.visual_transform,
            )
            .contains(logical_pos)
            {
                break;
            }
        }
        None
    }

    /// The window whose decoration (or popup) the pointer is on, if nothing
    /// covers it there: a layer-shell surface or a higher window's client.
    fn decoration_window_under_pointer(&self, pos: Point<f64, Logical>) -> Option<Window> {
        let contents = self.pointer_contents_at(pos);
        if contents.layer.is_some() {
            return None;
        }
        let (window, _) = self.decoration_under(pos)?;
        self.pointer_allows_window_interaction(
            contents.surface.as_ref().map(|(surface, _)| surface),
            &window,
        )
        .then_some(window)
    }

    /// Tell popups whose interest changed: the pointer entered or left their
    /// anchor, or the interactive popup itself.
    pub(crate) fn update_popup_interest(&mut self, pos: Point<f64, Logical>) {
        let mut next = Vec::new();
        if let Some(window) = self.decoration_window_under_pointer(pos)
            && let Some(decoration) = self.window_decorations.get(&window)
        {
            let window_id = decoration.snapshot.id.clone();
            for popup in decoration.layout.popups() {
                let (Some(node_id), Some(handler)) = (
                    popup.node_id.clone(),
                    popup
                        .handlers
                        .as_ref()
                        .and_then(|handlers| handlers.interest_change.clone()),
                ) else {
                    continue;
                };
                if popup.has_interest(pos.x, pos.y) {
                    next.push(PopupInterest {
                        window: window.clone(),
                        window_id: window_id.clone(),
                        node_id,
                        handler,
                    });
                }
            }
        }
        let previous = std::mem::take(&mut self.popup_input.interest);
        let same = |left: &PopupInterest, right: &PopupInterest| {
            left.window_id == right.window_id && left.node_id == right.node_id
        };
        let mut requests = Vec::new();
        for lost in previous
            .iter()
            .filter(|old| !next.iter().any(|new| same(old, new)))
        {
            requests.push(PopupRequest {
                window: lost.window.clone(),
                window_id: lost.window_id.clone(),
                handler: lost.handler.handler_for(false).to_owned(),
            });
        }
        for gained in next
            .iter()
            .filter(|new| !previous.iter().any(|old| same(old, new)))
        {
            requests.push(PopupRequest {
                window: gained.window.clone(),
                window_id: gained.window_id.clone(),
                handler: gained.handler.handler_for(true).to_owned(),
            });
        }
        self.popup_input.interest = next;
        self.invoke_popup_requests(requests);
    }

    /// A pointer press at `pos`: press the anchors under it (`trigger="click"`)
    /// and light-dismiss the `Auto` popups it is outside of.
    pub(crate) fn handle_popup_press(&mut self, pos: Point<f64, Logical>) {
        if !self
            .window_decorations
            .values()
            .any(|decoration| decoration.has_popup())
        {
            return;
        }
        let mut requests = Vec::new();
        if let Some(window) = self.decoration_window_under_pointer(pos)
            && let Some(decoration) = self.window_decorations.get(&window)
        {
            for popup in decoration.layout.popups() {
                let Some(handler) = popup
                    .handlers
                    .as_ref()
                    .and_then(|handlers| handlers.anchor_press.clone())
                else {
                    continue;
                };
                if popup.anchor_contains(pos.x, pos.y)
                    && !(popup.is_open_and_interactive() && popup.contains(pos.x, pos.y))
                {
                    requests.push(PopupRequest {
                        window: window.clone(),
                        window_id: decoration.snapshot.id.clone(),
                        handler,
                    });
                }
            }
        }
        for (window, decoration) in &self.window_decorations {
            for popup in decoration.layout.popups() {
                if !popup.popup.close_on_outside_press
                    || popup.contains(pos.x, pos.y)
                    || popup.anchor_contains(pos.x, pos.y)
                {
                    continue;
                }
                if let Some(handler) = dismiss_handler(&popup, PopupDismissReason::OutsidePress) {
                    requests.push(PopupRequest {
                        window: window.clone(),
                        window_id: decoration.snapshot.id.clone(),
                        handler,
                    });
                }
            }
        }
        self.invoke_popup_requests(requests);
    }

    /// The most recently opened `Auto` popup that closes on Escape.
    fn escape_popup(&self) -> Option<PopupRequest> {
        self.popup_input
            .open_auto
            .iter()
            .rev()
            .find_map(|(window_id, node_id)| {
                let (window, decoration) = self
                    .window_decorations
                    .iter()
                    .find(|(_, decoration)| &decoration.snapshot.id == window_id)?;
                let popup = decoration
                    .layout
                    .popups()
                    .into_iter()
                    .find(|popup| popup.node_id.as_ref() == Some(node_id))?;
                if !popup.popup.close_on_escape {
                    return None;
                }
                Some(PopupRequest {
                    window: window.clone(),
                    window_id: window_id.clone(),
                    handler: dismiss_handler(&popup, PopupDismissReason::Escape)?,
                })
            })
    }

    /// Whether Escape belongs to a popup now, instead of the focused client.
    pub(crate) fn popup_takes_escape(&self) -> bool {
        self.escape_popup().is_some()
    }

    pub(crate) fn dismiss_popup_on_escape(&mut self) {
        let requests = self.escape_popup().into_iter().collect();
        self.invoke_popup_requests(requests);
    }

    /// After a refresh: track which `Auto` popups are open, and ask the ones
    /// whose window went away, or that another `Auto` popup replaced, to close.
    pub(crate) fn sync_popup_dismissals(&mut self) {
        if self.popup_input.open_auto.is_empty()
            && !self
                .window_decorations
                .values()
                .any(|decoration| decoration.has_open_interactive_popup())
        {
            return;
        }

        struct Open {
            window: Window,
            window_id: String,
            node_id: String,
            popup: PopupInfo,
            is_new: bool,
            hidden: bool,
        }
        let mut open = Vec::new();
        for (window, decoration) in &self.window_decorations {
            for popup in decoration.layout.popups() {
                if !(popup.popup.open && popup.popup.mode == PopupMode::Auto) {
                    continue;
                }
                let Some(node_id) = popup.node_id.clone() else {
                    continue;
                };
                let window_id = decoration.snapshot.id.clone();
                let is_new = !self
                    .popup_input
                    .open_auto
                    .iter()
                    .any(|(open_window, open_node)| {
                        *open_window == window_id && *open_node == node_id
                    });
                open.push(Open {
                    window: window.clone(),
                    window_id,
                    node_id,
                    popup,
                    is_new,
                    hidden: !decoration.managed_window_allows_render(),
                });
            }
        }

        let wanted = dismissals(
            &open
                .iter()
                .map(|entry| OpenPopup {
                    window_id: &entry.window_id,
                    node_id: &entry.node_id,
                    enclosing: &entry.popup.enclosing,
                    is_new: entry.is_new,
                    hidden: entry.hidden,
                })
                .collect::<Vec<_>>(),
        )
        .into_iter()
        .map(|(index, reason)| (&open[index], reason));

        let state = &mut self.popup_input;
        let mut requests = Vec::new();
        for (entry, reason) in wanted {
            let key = (entry.window_id.clone(), entry.node_id.clone(), reason);
            if let Some(handler) = dismiss_handler(&entry.popup, reason)
                && state.requested.insert(key)
            {
                requests.push(PopupRequest {
                    window: entry.window.clone(),
                    window_id: entry.window_id.clone(),
                    handler,
                });
            }
        }

        // Oldest first: the ones still open keep their place, new ones follow.
        let still_open = |window_id: &str, node_id: &str| {
            open.iter()
                .any(|entry| entry.window_id == window_id && entry.node_id == node_id)
        };
        state
            .open_auto
            .retain(|(window_id, node_id)| still_open(window_id, node_id));
        state.open_auto.extend(
            open.iter()
                .filter(|entry| entry.is_new)
                .map(|entry| (entry.window_id.clone(), entry.node_id.clone())),
        );
        state
            .requested
            .retain(|(window_id, node_id, _)| still_open(window_id, node_id));
        self.invoke_popup_requests(requests);
    }

    fn invoke_popup_requests(&mut self, requests: Vec<PopupRequest>) {
        for request in requests {
            self.invoke_decoration_runtime_handler(
                &request.window,
                &request.window_id,
                &request.handler,
            );
        }
    }
}

/// An open `Auto` popup, as `dismissals` sees it.
struct OpenPopup<'a> {
    window_id: &'a str,
    node_id: &'a str,
    /// Ids of the popups it is nested in.
    enclosing: &'a [String],
    /// It was not open at the previous refresh.
    is_new: bool,
    /// Its window is not drawn.
    hidden: bool,
}

/// Which open `Auto` popups to ask to close, by index into `open`: those
/// whose window is hidden, and older ones that a newly opened popup not
/// nested in them replaces (one `Auto` popup chain at a time).
fn dismissals(open: &[OpenPopup<'_>]) -> Vec<(usize, PopupDismissReason)> {
    let mut wanted = Vec::new();
    for (index, entry) in open.iter().enumerate() {
        if entry.hidden {
            wanted.push((index, PopupDismissReason::AnchorGone));
        }
        let replaced = !entry.is_new
            && open.iter().any(|new| {
                new.is_new
                    && !(new.window_id == entry.window_id
                        && new.enclosing.iter().any(|id| id == entry.node_id))
            });
        if replaced {
            wanted.push((index, PopupDismissReason::OtherPopup));
        }
    }
    wanted
}

fn dismiss_handler(popup: &PopupInfo, reason: PopupDismissReason) -> Option<String> {
    (popup.popup.open && popup.popup.mode == PopupMode::Auto)
        .then(|| popup.handlers.as_ref()?.dismiss.as_ref())
        .flatten()
        .map(|handlers| handlers.handler_for(reason).to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn popup<'a>(
        window_id: &'a str,
        node_id: &'a str,
        enclosing: &'a [String],
        is_new: bool,
    ) -> OpenPopup<'a> {
        OpenPopup {
            window_id,
            node_id,
            enclosing,
            is_new,
            hidden: false,
        }
    }

    #[test]
    fn a_new_auto_popup_replaces_the_others_but_not_its_ancestors() {
        let none: Vec<String> = Vec::new();
        let in_menu = vec!["menu".to_owned()];
        // A submenu opens inside an open menu: the menu stays.
        let open = [
            popup("w1", "menu", &none, false),
            popup("w1", "submenu", &in_menu, true),
        ];
        assert!(dismissals(&open).is_empty());
        // Another window's menu opens: both of the first window go.
        let open = [
            popup("w1", "menu", &none, false),
            popup("w1", "submenu", &in_menu, false),
            popup("w2", "menu", &none, true),
        ];
        assert_eq!(
            dismissals(&open),
            [
                (0, PopupDismissReason::OtherPopup),
                (1, PopupDismissReason::OtherPopup)
            ]
        );
        // The same node id in another window is not an ancestor.
        let open = [
            popup("w1", "menu", &none, false),
            popup("w2", "submenu", &in_menu, true),
        ];
        assert_eq!(dismissals(&open), [(0, PopupDismissReason::OtherPopup)]);
    }

    #[test]
    fn a_hidden_window_takes_its_popups_with_it() {
        let none: Vec<String> = Vec::new();
        let open = [OpenPopup {
            hidden: true,
            ..popup("w1", "menu", &none, false)
        }];
        assert_eq!(dismissals(&open), [(0, PopupDismissReason::AnchorGone)]);
    }
}
