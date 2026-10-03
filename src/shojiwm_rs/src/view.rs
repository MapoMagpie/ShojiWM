//! Decoration views: builders for the nodes the TSX config writes as JSX.
//!
//! | TSX                 | Rust                                  |
//! |---------------------|---------------------------------------|
//! | `<Box direction="row">` | [`Flex::row()`] / [`Flex::column()`] |
//! | `<Label text=…>`    | [`Label::new`]                        |
//! | `<Button onClick=…>`| [`Button::new`]`.on_click(…)`         |
//! | `<Image src=…>`     | [`Image::new`]                        |
//! | `<AppIcon>`         | [`AppIcon::new`]                      |
//! | `<ShaderEffect shader=…>` | [`ShaderEffect::new`]           |
//! | `<WindowBorder>`    | [`WindowBorder::new`]                 |
//! | `<ClientWindow />`  | [`ClientWindow::new`]                 |
//! | `<ManagedWindow …>` | [`ManagedWindow::new`]                |
//! | `{cond && <X/>}`    | [`Element::child_dyn`]                |
//!
//! A composition function runs **once** per window (SolidJS style). Props
//! that are signals, memos or [`derive`](crate::reactive::derive)d values are
//! tracked per node: when one changes only that node is re-sent to the
//! compositor (or, for a shader uniform, only that uniform). Structure that
//! depends on state goes through [`Element::child_dyn`].

use std::{
    cell::{Cell, RefCell},
    collections::{BTreeSet, HashMap},
    rc::Rc,
};

use shojiwm_lib::{
    runtime_api::CompositionPatch,
    ssd::{
        BoxNode, ButtonNode, DecorationInteractionHandlers, DecorationNode, DecorationNodeKind,
        DecorationStateChangeHandler, DecorationStyle, ImageFit, ImageNode, LabelNode,
        LayoutDirection, ManagedWindowRectSnapshot, ManagedWindowState, ShaderEffectNode,
        ShaderUniformValue, TransformOrigin, WindowAction, WindowBorderInteraction,
        WindowResizeHitArea, WindowTransform,
    },
};

use crate::{
    assets,
    effect::{Effect, PaintShader, Uniform},
    reactive::{Observer, Prop, Scope, untrack},
    style::Style,
};

/// A logical-space rectangle.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    pub fn contains(&self, x: f64, y: f64) -> bool {
        x >= self.x && x < self.right() && y >= self.y && y < self.bottom()
    }
}

impl From<Rect> for ManagedWindowRectSnapshot {
    fn from(rect: Rect) -> Self {
        Self {
            x: rect.x,
            y: rect.y,
            width: rect.width,
            height: rect.height,
        }
    }
}

impl From<ManagedWindowRectSnapshot> for Rect {
    fn from(rect: ManagedWindowRectSnapshot) -> Self {
        Self::new(rect.x, rect.y, rect.width, rect.height)
    }
}

impl From<shojiwm_lib::ssd::WindowPositionSnapshot> for Rect {
    fn from(rect: shojiwm_lib::ssd::WindowPositionSnapshot) -> Self {
        Self::new(rect.x, rect.y, rect.width, rect.height)
    }
}

impl From<shojiwm_lib::ssd::LayerPositionSnapshot> for Rect {
    fn from(rect: shojiwm_lib::ssd::LayerPositionSnapshot) -> Self {
        Self::new(
            rect.x as f64,
            rect.y as f64,
            rect.width as f64,
            rect.height as f64,
        )
    }
}

pub use shojiwm_lib::ssd::LayoutDirection as Direction;

/// What a button does when clicked.
#[derive(Clone)]
enum ClickAction {
    /// Handled by the compositor without asking the config.
    Window(WindowAction),
    Handler(Rc<dyn Fn()>),
}

#[derive(Clone)]
enum ElementKind {
    Box { direction: Prop<LayoutDirection> },
    Label { text: Prop<String> },
    Button,
    AppIcon,
    Image { src: Prop<String>, fit: ImageFit },
    ShaderEffect {
        direction: Prop<LayoutDirection>,
        effect: Box<Effect>,
    },
    WindowBorder { interaction: WindowBorderInteraction },
    WindowSlot,
}

/// A child of an element: fixed, or recomputed when what it reads changes.
#[derive(Clone)]
pub enum Child {
    Element(Element),
    Dynamic(Rc<dyn Fn() -> Vec<Element>>),
}

impl From<Element> for Child {
    fn from(element: Element) -> Self {
        Self::Element(element)
    }
}

/// A decoration node under construction.
#[derive(Clone)]
pub struct Element {
    kind: ElementKind,
    style: Box<Style>,
    key: Option<String>,
    on_click: Option<ClickAction>,
    on_hover_change: Option<Rc<dyn Fn(bool)>>,
    on_active_change: Option<Rc<dyn Fn(bool)>>,
    paint: Option<Box<PaintShader>>,
    overlay: Option<Box<PaintShader>>,
    children: Vec<Child>,
}

impl Element {
    fn new(kind: ElementKind) -> Self {
        Self {
            kind,
            style: Box::default(),
            key: None,
            on_click: None,
            on_hover_change: None,
            on_active_change: None,
            paint: None,
            overlay: None,
            children: Vec::new(),
        }
    }

    /// Replace the built-in background/border painting with a paint shader
    /// (the `paint` prop). On a [`ShaderEffect`] it is drawn over the effect.
    pub fn paint(mut self, shader: PaintShader) -> Self {
        self.paint = Some(Box::new(shader));
        self
    }

    /// A paint shader drawn above the children (the `overlay` prop).
    pub fn overlay(mut self, shader: PaintShader) -> Self {
        self.overlay = Some(Box::new(shader));
        self
    }

    /// The paint shaders of this element with their patch stage indices.
    fn paint_shaders(&self) -> impl Iterator<Item = (usize, &PaintShader)> {
        [
            (shojiwm_lib::runtime_api::PAINT_STAGE_INDEX, self.paint.as_deref()),
            (shojiwm_lib::runtime_api::OVERLAY_STAGE_INDEX, self.overlay.as_deref()),
        ]
        .into_iter()
        .filter_map(|(index, shader)| shader.map(|shader| (index, shader)))
    }

    pub fn style(mut self, style: Style) -> Self {
        self.style = Box::new(style);
        self
    }

    /// Stable identity among siblings (the JSX `key`).
    pub fn key(mut self, key: impl Into<String>) -> Self {
        self.key = Some(key.into());
        self
    }

    /// Layout direction of a [`Flex`] or [`ShaderEffect`].
    pub fn direction(mut self, value: impl Into<Prop<LayoutDirection>>) -> Self {
        match &mut self.kind {
            ElementKind::Box { direction } | ElementKind::ShaderEffect { direction, .. } => {
                *direction = value.into();
            }
            _ => tracing::warn!("direction() only applies to Flex and ShaderEffect"),
        }
        self
    }

    /// Image scaling ([`Image`] only).
    pub fn fit(mut self, value: ImageFit) -> Self {
        if let ElementKind::Image { fit, .. } = &mut self.kind {
            *fit = value;
        }
        self
    }

    /// Resize handles of a [`WindowBorder`]: width of the edges and corners.
    pub fn resize_hit_area(mut self, edge_px: i32, corner_px: i32) -> Self {
        if let ElementKind::WindowBorder { interaction } = &mut self.kind {
            interaction.resize_hit_area = Some(WindowResizeHitArea {
                edge_width: Some(edge_px),
                corner_width: Some(corner_px),
            });
        }
        self
    }

    pub fn child(mut self, child: impl Into<Child>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn children<C: Into<Child>>(mut self, children: impl IntoIterator<Item = C>) -> Self {
        self.children.extend(children.into_iter().map(Into::into));
        self
    }

    /// Children recomputed whenever what `f` reads changes, e.g. an icon
    /// shown only while hovered. Signals created inside `f` live until the
    /// next recomputation.
    pub fn child_dyn<I>(mut self, f: impl Fn() -> I + 'static) -> Self
    where
        I: IntoIterator<Item = Element>,
    {
        self.children
            .push(Child::Dynamic(Rc::new(move || f().into_iter().collect())));
        self
    }

    /// Run `f` when the node is clicked.
    pub fn on_click(mut self, f: impl Fn() + 'static) -> Self {
        self.on_click = Some(ClickAction::Handler(Rc::new(f)));
        self
    }

    /// Let the compositor perform `action` directly on click.
    pub fn on_click_action(mut self, action: WindowAction) -> Self {
        self.on_click = Some(ClickAction::Window(action));
        self
    }

    pub fn on_hover_change(mut self, f: impl Fn(bool) + 'static) -> Self {
        self.on_hover_change = Some(Rc::new(f));
        self
    }

    pub fn on_active_change(mut self, f: impl Fn(bool) + 'static) -> Self {
        self.on_active_change = Some(Rc::new(f));
        self
    }
}

/// `<Box>`: a flex container.
pub struct Flex;

impl Flex {
    pub fn row() -> Element {
        Element::new(ElementKind::Box {
            direction: LayoutDirection::Row.into(),
        })
    }

    pub fn column() -> Element {
        Element::new(ElementKind::Box {
            direction: LayoutDirection::Column.into(),
        })
    }
}

/// `<Label>`: a line of text.
pub struct Label;

impl Label {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(text: impl Into<Prop<String>>) -> Element {
        Element::new(ElementKind::Label { text: text.into() })
    }
}

/// `<Button>`: a clickable node; set the action with [`Element::on_click`].
pub struct Button;

impl Button {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Element {
        Element::new(ElementKind::Button)
    }
}

/// `<Image>`: an image file, resolved against the asset root.
pub struct Image;

impl Image {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(src: impl Into<Prop<String>>) -> Element {
        Element::new(ElementKind::Image {
            src: src.into().map(|src| assets::resolve(&src)),
            fit: ImageFit::Contain,
        })
    }
}

/// `<AppIcon>`: the window's application icon.
pub struct AppIcon;

impl AppIcon {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Element {
        Element::new(ElementKind::AppIcon)
    }
}

/// `<ShaderEffect>`: a container drawn through an effect pipeline.
pub struct ShaderEffect;

impl ShaderEffect {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(effect: Effect) -> Element {
        Element::new(ElementKind::ShaderEffect {
            direction: LayoutDirection::Column.into(),
            effect: Box::new(effect),
        })
    }
}

/// `<WindowBorder>`: the window frame; resizes from its edges.
pub struct WindowBorder;

impl WindowBorder {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Element {
        Element::new(ElementKind::WindowBorder {
            interaction: WindowBorderInteraction::default(),
        })
    }
}

/// `<ClientWindow />`: where the client surface goes.
pub struct ClientWindow;

impl ClientWindow {
    #[allow(clippy::new_ret_no_self)]
    pub fn new() -> Element {
        Element::new(ElementKind::WindowSlot)
    }
}

/// `ManagedWindow transform`: scale and translation around an origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ManagedTransform {
    pub origin_x: f64,
    pub origin_y: f64,
    pub translate_x: f64,
    pub translate_y: f64,
    pub scale_x: f64,
    pub scale_y: f64,
}

impl Default for ManagedTransform {
    fn default() -> Self {
        Self {
            origin_x: 0.5,
            origin_y: 0.5,
            translate_x: 0.0,
            translate_y: 0.0,
            scale_x: 1.0,
            scale_y: 1.0,
        }
    }
}

/// `<ManagedWindow>`: the root of a composition, carrying window placement.
#[derive(Clone, Default)]
pub struct ManagedWindow {
    rect: Option<Prop<Rect>>,
    workspace: Option<Prop<serde_json::Value>>,
    visible_outputs: Option<Prop<Option<Vec<String>>>>,
    visible: Option<Prop<bool>>,
    idle: Option<Prop<bool>>,
    interactive: Option<Prop<bool>>,
    force_rect_size: Option<Prop<bool>>,
    tiled: Option<Prop<bool>>,
    allow_tearing: Option<Prop<bool>>,
    z_index: Option<Prop<i32>>,
    opacity: Option<Prop<f64>>,
    transform: Option<Prop<ManagedTransform>>,
    children: Vec<Child>,
}

macro_rules! managed_props {
    ($($(#[$doc:meta])* $field:ident: $ty:ty),* $(,)?) => {
        impl ManagedWindow {
            $(
                $(#[$doc])*
                pub fn $field(mut self, value: impl Into<Prop<$ty>>) -> Self {
                    self.$field = Some(value.into());
                    self
                }
            )*
        }
    };
}

managed_props! {
    /// Where the window (decorations included) goes, in logical pixels.
    rect: Rect,
    workspace: serde_json::Value,
    /// Restrict the window to these outputs; `None` means everywhere.
    visible_outputs: Option<Vec<String>>,
    visible: bool,
    /// An idle window is not drawn and receives no input.
    idle: bool,
    interactive: bool,
    /// Configure the client to exactly the rect size.
    force_rect_size: bool,
    tiled: bool,
    /// Allow tearing page flips while the window is scanned out directly.
    allow_tearing: bool,
    z_index: i32,
    opacity: f64,
    transform: ManagedTransform,
}

impl ManagedWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn child(mut self, child: impl Into<Child>) -> Self {
        self.children.push(child.into());
        self
    }

    pub fn child_dyn<I>(mut self, f: impl Fn() -> I + 'static) -> Self
    where
        I: IntoIterator<Item = Element>,
    {
        self.children
            .push(Child::Dynamic(Rc::new(move || f().into_iter().collect())));
        self
    }

    /// Split off the children: they become the decoration tree.
    pub(crate) fn take_children(&mut self) -> Vec<Child> {
        std::mem::take(&mut self.children)
    }

    /// Read every prop (tracked).
    pub(crate) fn resolve(&self) -> ManagedWindowState {
        fn read<T: Clone>(prop: &Option<Prop<T>>) -> Option<T> {
            prop.as_ref().map(Prop::get)
        }
        let transform = read(&self.transform).unwrap_or_default();
        let opacity = read(&self.opacity).unwrap_or(1.0);
        let visible = read(&self.visible).unwrap_or(true);
        let idle = read(&self.idle).unwrap_or(false);
        ManagedWindowState {
            managed: true,
            rect: read(&self.rect).map(Into::into),
            workspace: read(&self.workspace),
            visible_outputs: read(&self.visible_outputs).flatten().map(|outputs| {
                let mut seen = BTreeSet::new();
                outputs
                    .into_iter()
                    .filter(|output| seen.insert(output.clone()))
                    .collect()
            }),
            visible,
            idle,
            interactive: read(&self.interactive).unwrap_or(true),
            force_rect_size: read(&self.force_rect_size).unwrap_or(false),
            tiled: read(&self.tiled).unwrap_or(false),
            allow_tearing: read(&self.allow_tearing),
            z_index: read(&self.z_index),
            transform: WindowTransform {
                origin: TransformOrigin {
                    x: transform.origin_x,
                    y: transform.origin_y,
                },
                translate_x: transform.translate_x,
                translate_y: transform.translate_y,
                scale_x: transform.scale_x,
                scale_y: transform.scale_y,
                opacity: if visible && !idle { opacity as f32 } else { 0.0 },
            },
            surface_policy: None,
        }
    }
}

/// What a composition function returns.
pub enum Composition {
    Managed(Box<ManagedWindow>),
    /// A bare tree; the compositor places the window itself.
    Unmanaged(Element),
}

impl From<ManagedWindow> for Composition {
    fn from(window: ManagedWindow) -> Self {
        Self::Managed(Box::new(window))
    }
}

impl From<Element> for Composition {
    fn from(element: Element) -> Self {
        Self::Unmanaged(element)
    }
}

/// Per-window bookkeeping shared by the mounted nodes.
#[derive(Default)]
pub(crate) struct DirtySet {
    pub nodes: BTreeSet<String>,
    pub uniforms: BTreeSet<(String, usize, String)>,
    pub managed: bool,
    /// Rebuild the whole tree (no new composition run).
    pub full: bool,
    /// Run the composition function again.
    pub recompose: bool,
    /// Re-evaluate the window effects.
    pub effects: bool,
}

impl DirtySet {
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
            && self.uniforms.is_empty()
            && !self.managed
            && !self.full
            && !self.recompose
            && !self.effects
    }
}

/// Runtime handlers of one window, by handler id.
pub(crate) type HandlerMap = HashMap<String, Rc<dyn Fn()>>;

#[derive(Clone)]
pub(crate) struct ViewContext {
    pub dirty: Rc<RefCell<DirtySet>>,
    pub handlers: Rc<RefCell<HandlerMap>>,
    /// Called on the first mark after the set was drained.
    pub on_dirty: Rc<dyn Fn()>,
}

impl ViewContext {
    pub fn mark(&self, f: impl FnOnce(&mut DirtySet)) {
        let was_empty = {
            let mut dirty = self.dirty.borrow_mut();
            let was_empty = dirty.is_empty();
            f(&mut dirty);
            was_empty
        };
        if was_empty {
            (self.on_dirty)();
        }
    }

    pub fn mark_node(&self, id: &str) {
        self.mark(|dirty| {
            dirty.nodes.insert(id.to_owned());
        });
    }

    pub fn mark_managed(&self) {
        self.mark(|dirty| dirty.managed = true);
    }

    pub fn mark_full(&self) {
        self.mark(|dirty| dirty.full = true);
    }

    pub fn mark_recompose(&self) {
        self.mark(|dirty| dirty.recompose = true);
    }

    fn register_handler(&self, id: String, handler: Rc<dyn Fn()>) {
        self.handlers.borrow_mut().insert(id.clone(), handler);
        let handlers = self.handlers.clone();
        crate::reactive::on_cleanup(move || {
            handlers.borrow_mut().remove(&id);
        });
    }
}

struct UniformBinding {
    stage_index: usize,
    name: String,
    observer: Observer,
    last: RefCell<Option<ShaderUniformValue>>,
}

struct Region {
    index: usize,
    f: Rc<dyn Fn() -> Vec<Element>>,
    scope: Scope,
    observer: Observer,
    stale: Rc<Cell<bool>>,
    nodes: Vec<MountedNode>,
}

enum MountedChild {
    Node(Box<MountedNode>),
    Region(Region),
}

pub(crate) struct MountedNode {
    id: String,
    element: Element,
    observer: Observer,
    uniforms: Vec<UniformBinding>,
    children: Vec<MountedChild>,
}

fn child_id(parent: &str, index: usize, element: &Element) -> String {
    match &element.key {
        Some(key) => format!("{parent}/#{key}"),
        None => format!("{parent}/{index}"),
    }
}

impl MountedNode {
    /// Attach observers and handlers; must run inside the owning scope.
    pub(crate) fn mount(element: Element, id: String, context: &ViewContext) -> Self {
        let observer = {
            let context = context.clone();
            let id = id.clone();
            Observer::new(move || context.mark_node(&id))
        };
        crate::reactive::on_cleanup(move || observer.dispose());

        if let Some(ClickAction::Handler(handler)) = &element.on_click {
            context.register_handler(format!("{id}#click"), handler.clone());
        }
        for (name, handler) in [
            ("hover", &element.on_hover_change),
            ("active", &element.on_active_change),
        ] {
            if let Some(handler) = handler {
                let on = handler.clone();
                let off = handler.clone();
                context.register_handler(format!("{id}#{name}.true"), Rc::new(move || on(true)));
                context.register_handler(format!("{id}#{name}.false"), Rc::new(move || off(false)));
            }
        }

        let mut uniforms = Vec::new();
        let mut bind = |stage_index: usize, name: &str, value: &Prop<Uniform>| {
            if value.is_static() {
                return;
            }
            let binding_observer = {
                let context = context.clone();
                let key = (id.clone(), stage_index, name.to_owned());
                Observer::new(move || {
                    let key = key.clone();
                    context.mark(|dirty| {
                        dirty.uniforms.insert(key);
                    });
                })
            };
            crate::reactive::on_cleanup(move || binding_observer.dispose());
            uniforms.push(UniformBinding {
                stage_index,
                name: name.to_owned(),
                observer: binding_observer,
                last: RefCell::new(None),
            });
        };
        if let ElementKind::ShaderEffect { effect, .. } = &element.kind {
            for (stage_index, stage) in effect.shader_stages() {
                for (name, value) in stage.uniform_props() {
                    bind(stage_index, name, value);
                }
            }
        }
        for (stage_index, shader) in element.paint_shaders() {
            for (name, value) in shader.uniform_props() {
                bind(stage_index, name, value);
            }
        }

        let children = element
            .children
            .iter()
            .enumerate()
            .map(|(index, child)| match child {
                Child::Element(child) => MountedChild::Node(Box::new(Self::mount(
                    child.clone(),
                    child_id(&id, index, child),
                    context,
                ))),
                Child::Dynamic(f) => {
                    let stale = Rc::new(Cell::new(true));
                    let region_observer = {
                        let context = context.clone();
                        let parent = id.clone();
                        let stale = stale.clone();
                        Observer::new(move || {
                            stale.set(true);
                            context.mark_node(&parent);
                        })
                    };
                    crate::reactive::on_cleanup(move || region_observer.dispose());
                    let scope = crate::reactive::current_scope()
                        .map(Scope::child)
                        .unwrap_or_else(Scope::root);
                    MountedChild::Region(Region {
                        index,
                        f: f.clone(),
                        scope,
                        observer: region_observer,
                        stale,
                        nodes: Vec::new(),
                    })
                }
            })
            .collect();

        Self {
            id,
            element,
            observer,
            uniforms,
            children,
        }
    }

    /// Build the compositor node, re-reading props and re-running stale
    /// dynamic children.
    pub(crate) fn build(&mut self, context: &ViewContext) -> DecorationNode {
        let id = self.id.clone();
        let element = &self.element;
        let uniforms = &self.uniforms;
        let (kind, style) = self.observer.track(|| {
            // Reactive uniforms are tracked by their own binding (patched
            // alone); everything else belongs to the node.
            let mut read = |stage_index: usize, name: &str, value: &Prop<Uniform>| {
                match uniforms
                    .iter()
                    .find(|binding| binding.stage_index == stage_index && binding.name == name)
                {
                    Some(binding) => {
                        let value: ShaderUniformValue = binding.observer.track(|| value.get()).into();
                        *binding.last.borrow_mut() = Some(value.clone());
                        value
                    }
                    None => untrack(|| value.get()).into(),
                }
            };
            let mut style = element.style.resolve();
            style.paint = element
                .paint
                .as_ref()
                .map(|shader| shader.compile(&mut read, shojiwm_lib::runtime_api::PAINT_STAGE_INDEX));
            style.overlay = element
                .overlay
                .as_ref()
                .map(|shader| shader.compile(&mut read, shojiwm_lib::runtime_api::OVERLAY_STAGE_INDEX));
            let kind = match &element.kind {
                ElementKind::Box { direction } => DecorationNodeKind::Box(BoxNode {
                    direction: direction.get(),
                }),
                ElementKind::Label { text } => DecorationNodeKind::Label(LabelNode { text: text.get() }),
                ElementKind::Button => DecorationNodeKind::Button(ButtonNode {
                    action: match &element.on_click {
                        Some(ClickAction::Window(action)) => action.clone(),
                        Some(ClickAction::Handler(_)) => WindowAction::RuntimeHandler(format!("{id}#click")),
                        None => WindowAction::RuntimeHandler(format!("{id}#none")),
                    },
                }),
                ElementKind::AppIcon => DecorationNodeKind::AppIcon,
                ElementKind::Image { src, fit } => DecorationNodeKind::Image(ImageNode {
                    src: src.get(),
                    fit: *fit,
                }),
                ElementKind::ShaderEffect { direction, effect } => {
                    let shader = effect.compile_with(&mut read);
                    DecorationNodeKind::ShaderEffect(ShaderEffectNode {
                        direction: direction.get(),
                        shader,
                    })
                }
                ElementKind::WindowBorder { .. } => DecorationNodeKind::WindowBorder,
                ElementKind::WindowSlot => DecorationNodeKind::WindowSlot,
            };
            (kind, style)
        });

        let interaction = DecorationInteractionHandlers {
            hover_change: element.on_hover_change.as_ref().map(|_| DecorationStateChangeHandler {
                true_handler: format!("{id}#hover.true"),
                false_handler: format!("{id}#hover.false"),
            }),
            active_change: element.on_active_change.as_ref().map(|_| DecorationStateChangeHandler {
                true_handler: format!("{id}#active.true"),
                false_handler: format!("{id}#active.false"),
            }),
        };
        let window_border_interaction = match &element.kind {
            ElementKind::WindowBorder { interaction } => *interaction,
            _ => WindowBorderInteraction::default(),
        };

        let mut children = Vec::new();
        for child in &mut self.children {
            match child {
                MountedChild::Node(node) => children.push(node.build(context)),
                MountedChild::Region(region) => {
                    if region.stale.replace(false) {
                        region.scope.clear();
                        let f = region.f.clone();
                        let observer = region.observer;
                        let elements = region.scope.run(|| observer.track(|| f()));
                        let parent = id.clone();
                        let index = region.index;
                        region.nodes = region.scope.run(|| {
                            elements
                                .into_iter()
                                .enumerate()
                                .map(|(position, element)| {
                                    let child_id = match &element.key {
                                        Some(key) => format!("{parent}/{index}#{key}"),
                                        None => format!("{parent}/{index}.{position}"),
                                    };
                                    MountedNode::mount(element, child_id, context)
                                })
                                .collect()
                        });
                    }
                    for node in &mut region.nodes {
                        children.push(node.build(context));
                    }
                }
            }
        }

        DecorationNode {
            stable_id: Some(id),
            interaction,
            window_border_interaction,
            kind,
            style,
            children,
        }
    }

    fn find_mut(&mut self, id: &str) -> Option<&mut MountedNode> {
        if self.id == id {
            return Some(self);
        }
        if !id.starts_with(&self.id) {
            return None;
        }
        for child in &mut self.children {
            match child {
                MountedChild::Node(node) => {
                    if let Some(found) = node.find_mut(id) {
                        return Some(found);
                    }
                }
                MountedChild::Region(region) => {
                    for node in &mut region.nodes {
                        if let Some(found) = node.find_mut(id) {
                            return Some(found);
                        }
                    }
                }
            }
        }
        None
    }

    /// Current value of a reactive uniform, if its shape is unchanged.
    fn read_uniform(&self, stage_index: usize, name: &str) -> Option<ShaderUniformValue> {
        let binding = self
            .uniforms
            .iter()
            .find(|binding| binding.stage_index == stage_index && binding.name == name)?;
        let prop = match self
            .element
            .paint_shaders()
            .find(|(index, _)| *index == stage_index)
        {
            Some((_, shader)) => shader.uniform_props().find(|(candidate, _)| *candidate == name)?.1,
            None => {
                let ElementKind::ShaderEffect { effect, .. } = &self.element.kind else {
                    return None;
                };
                let (_, stage) = effect
                    .shader_stages()
                    .find(|(index, _)| *index == stage_index)?;
                stage.uniform_props().find(|(candidate, _)| *candidate == name)?.1
            }
        };
        let value: ShaderUniformValue = binding.observer.track(|| prop.get()).into();
        let mut last = binding.last.borrow_mut();
        if !last.as_ref().is_some_and(|last| last.shape_matches(&value)) {
            return None;
        }
        *last = Some(value.clone());
        Some(value)
    }
}

fn is_descendant(id: &str, ancestor: &str) -> bool {
    id.len() > ancestor.len()
        && id.starts_with(ancestor)
        && matches!(id.as_bytes()[ancestor.len()], b'/')
}

/// The mounted view of one window and the tree the compositor holds.
pub(crate) struct ViewTree {
    pub root: MountedNode,
    pub tree: DecorationNode,
}

pub(crate) struct ViewUpdate {
    pub patches: Vec<CompositionPatch>,
    pub dirty_node_ids: Vec<String>,
}

impl ViewTree {
    pub fn new(mut root: MountedNode, context: &ViewContext) -> Self {
        let tree = root.build(context);
        Self { root, tree }
    }

    pub fn rebuild(&mut self, context: &ViewContext) {
        self.tree = self.root.build(context);
    }

    /// Turn dirty node and uniform marks into patches and apply them to the
    /// cached tree. Unknown ids (nodes replaced meanwhile) are skipped.
    pub fn update(
        &mut self,
        nodes: BTreeSet<String>,
        uniforms: BTreeSet<(String, usize, String)>,
        context: &ViewContext,
    ) -> ViewUpdate {
        let mut top_level: Vec<String> = Vec::new();
        for id in &nodes {
            if !nodes.iter().any(|other| is_descendant(id, other)) {
                top_level.push(id.clone());
            }
        }

        let mut patches = Vec::new();
        let mut dirty_node_ids = Vec::new();
        let mut uniform_fallback = BTreeSet::new();
        for (node_id, stage_index, name) in &uniforms {
            if top_level
                .iter()
                .any(|id| id == node_id || is_descendant(node_id, id))
            {
                continue;
            }
            let Some(node) = self.root.find_mut(node_id) else {
                continue;
            };
            match node.read_uniform(*stage_index, name) {
                Some(value) => {
                    replace_uniform(&mut self.tree, node_id, *stage_index, name, &value);
                    if !dirty_node_ids.contains(node_id) {
                        dirty_node_ids.push(node_id.clone());
                    }
                    patches.push(CompositionPatch::ShaderUniform {
                        node_id: node_id.clone(),
                        stage_index: *stage_index,
                        name: name.clone(),
                        value,
                    });
                }
                None => {
                    uniform_fallback.insert(node_id.clone());
                }
            }
        }

        for id in top_level.into_iter().chain(uniform_fallback) {
            let Some(node) = self.root.find_mut(&id) else {
                continue;
            };
            let built = node.build(context);
            replace_node(&mut self.tree, &id, built.clone());
            patches.retain(|patch| !(patch.node_id() == id || is_descendant(patch.node_id(), &id)));
            dirty_node_ids.retain(|existing| existing != &id && !is_descendant(existing, &id));
            dirty_node_ids.push(id.clone());
            patches.push(CompositionPatch::ReplaceNode {
                node_id: id,
                node: built,
            });
        }

        ViewUpdate {
            patches,
            dirty_node_ids,
        }
    }
}

fn replace_node(tree: &mut DecorationNode, id: &str, node: DecorationNode) -> bool {
    if tree.stable_id.as_deref() == Some(id) {
        *tree = node;
        return true;
    }
    let Some(child) = tree.children.iter_mut().find(|child| {
        child
            .stable_id
            .as_deref()
            .is_some_and(|child_id| id == child_id || is_descendant(id, child_id))
    }) else {
        return false;
    };
    replace_node(child, id, node)
}

fn replace_uniform(
    tree: &mut DecorationNode,
    id: &str,
    stage_index: usize,
    name: &str,
    value: &ShaderUniformValue,
) {
    if tree.stable_id.as_deref() == Some(id) {
        let paint = match stage_index {
            shojiwm_lib::runtime_api::PAINT_STAGE_INDEX => Some(&mut tree.style.paint),
            shojiwm_lib::runtime_api::OVERLAY_STAGE_INDEX => Some(&mut tree.style.overlay),
            _ => None,
        };
        if let Some(paint) = paint {
            if let Some(paint) = paint {
                paint.uniforms.insert(name.to_owned(), value.clone());
            }
            return;
        }
        if let DecorationNodeKind::ShaderEffect(node) = &mut tree.kind {
            let stage = if stage_index == shojiwm_lib::runtime_api::SHADER_INPUT_STAGE_INDEX {
                match &mut node.shader.input {
                    shojiwm_lib::ssd::EffectInput::Shader(stage) => Some(stage),
                    _ => None,
                }
            } else {
                match node.shader.pipeline.get_mut(stage_index) {
                    Some(shojiwm_lib::ssd::EffectStage::Shader(stage)) => Some(stage),
                    _ => None,
                }
            };
            if let Some(stage) = stage {
                stage.uniforms.insert(name.to_owned(), value.clone());
            }
        }
        return;
    }
    for child in &mut tree.children {
        replace_uniform(child, id, stage_index, name, value);
    }
}

/// The style a node would get, for tests and debugging.
pub fn resolve_style(style: &Style) -> DecorationStyle {
    style.resolve()
}
