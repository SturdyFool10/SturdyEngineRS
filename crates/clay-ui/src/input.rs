use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

use glam::Vec2;

use crate::{Axis, Easing, EasingRegistry, ElementId, LayoutTree, Rect, UiLayer, UiShape};

// ── Build-time widget context ─────────────────────────────────────────────────

/// Deferred registrations collected while building a widget tree.
///
/// Apply with [`PendingRegistrations::apply`] before calling
/// [`InputSimulator::update`].
#[derive(Default)]
pub struct PendingRegistrations {
    behaviors: HashMap<u64, (ElementId, WidgetBehavior)>,
    slider_configs: HashMap<u64, (ElementId, SliderConfig)>,
    scroll_configs: HashMap<u64, (ElementId, ScrollConfig)>,
    toggle_anim_updates: HashMap<u64, (ElementId, f32)>,
}

impl PendingRegistrations {
    /// Push all collected registrations into the simulator.
    pub fn apply(self, sim: &mut InputSimulator) {
        for (_, (id, behavior)) in self.behaviors {
            sim.set_widget_behavior(id, behavior);
        }
        for (_, (id, config)) in self.slider_configs {
            sim.set_slider_config(id, config);
        }
        for (_, (id, config)) in self.scroll_configs {
            sim.set_scroll_config(id, config);
        }
        for (_, (id, progress)) in self.toggle_anim_updates {
            sim.set_toggle_animation_progress(id, progress);
        }
    }
}

/// Build-time context passed to every widget builder.
///
/// Holds a shared reference to the [`InputSimulator`] (for reading per-frame
/// widget states) plus the active [`crate::WidgetPalette`].  Non-default input
/// behaviors — sliders, scroll containers, text inputs — queue their
/// registrations into the owned [`PendingRegistrations`] so they can be
/// applied to the simulator (which needs `&mut`) after tree building.
///
/// # Typical frame loop
///
/// ```ignore
/// let sim = hub.simulator();
/// let mut cx = Cx::new(sim, palette);
///
/// let tree = build_my_ui(&mut cx, viewport);
///
/// let pending = cx.finish();
/// pending.apply(hub.simulator_mut());   // apply before update!
///
/// hub.update(&layout);
/// ```
pub struct Cx<'a> {
    pub(crate) sim: &'a InputSimulator,
    /// The active widget palette. Read by widget builders for all colors.
    pub palette: crate::WidgetPalette,
    pending: RefCell<PendingRegistrations>,
}

impl<'a> Cx<'a> {
    pub fn new(sim: &'a InputSimulator, palette: crate::WidgetPalette) -> Self {
        Self { sim, palette, pending: RefCell::new(PendingRegistrations::default()) }
    }

    /// Returns the current interaction state for `id` (hover, press, focus, …).
    pub fn state(&self, id: &ElementId) -> WidgetState {
        self.sim.widget_state(id)
    }

    /// Returns the normalized (0 – 1) display value of a slider.
    ///
    /// Returns 0.0 if the slider has not been registered yet.
    pub fn slider_value_normalized(&self, id: &ElementId) -> f32 {
        let raw = self.sim.slider_value(id);
        if let Some(config) = self.sim.slider_config(id) {
            let range = (config.max - config.min).max(f32::EPSILON);
            ((raw - config.min) / range).clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    /// Returns the raw slider value.
    pub fn slider_value(&self, id: &ElementId) -> f32 {
        self.sim.slider_value(id)
    }

    /// Returns the layout-space scroll offset (negated) for child positioning.
    pub fn scroll_layout_offset(&self, id: &ElementId) -> glam::Vec2 {
        self.sim.scroll_layout_offset(id)
    }

    // ── Deferred registration ─────────────────────────────────────────────────

    /// Queue slider behavior + config. Called by [`crate::slider`] automatically.
    pub fn register_behavior(&self, id: ElementId, behavior: WidgetBehavior) {
        let hash = id.hash;
        self.pending.borrow_mut().behaviors.insert(hash, (id, behavior));
    }

    pub fn register_slider(&self, id: ElementId, axis: crate::Axis, config: SliderConfig) {
        let hash = id.hash;
        let mut pending = self.pending.borrow_mut();
        pending.behaviors.insert(hash, (id.clone(), WidgetBehavior::slider(axis)));
        pending.slider_configs.insert(hash, (id, config));
    }

    /// Queue scroll-area behavior + config. Called by scroll container widgets automatically.
    pub fn register_scroll(&self, id: ElementId, config: ScrollConfig) {
        let hash = id.hash;
        let mut pending = self.pending.borrow_mut();
        pending.behaviors.insert(hash, (id.clone(), WidgetBehavior::scroll_area()));
        pending.scroll_configs.insert(hash, (id, config));
    }

    /// Queue text-input behavior. Called by text input widgets automatically.
    pub fn register_text_input(&self, id: ElementId) {
        let hash = id.hash;
        self.pending.borrow_mut().behaviors.insert(hash, (id, WidgetBehavior::text_input()));
    }

    /// Queue drag-bar behavior. Called by drag bar widgets automatically.
    pub fn register_drag_bar(&self, id: ElementId, axis: crate::Axis) {
        let hash = id.hash;
        self.pending.borrow_mut().behaviors.insert(hash, (id, WidgetBehavior::drag_bar(axis)));
    }

    /// Advances the toggle animation for `id` toward `target` (0.0=off, 1.0=on)
    /// and returns the eased progress value for rendering this frame.
    ///
    /// On the first call for a given id the progress snaps to `target` so there
    /// is no jarring animation from zero.  Pass `dt=0.0` to always snap.
    pub fn advance_toggle_animation(
        &self,
        id: &ElementId,
        target: f32,
        duration: f32,
        dt: f32,
        easing: Easing,
    ) -> f32 {
        let target = target.clamp(0.0, 1.0);
        let current = self.sim.toggle_animation_progress(id, target);
        let new_linear = if dt > 0.0 && duration > f32::EPSILON {
            let step = dt / duration;
            if target >= 0.5 {
                (current + step).min(1.0)
            } else {
                (current - step).max(0.0)
            }
        } else {
            target
        };
        self.pending.borrow_mut().toggle_anim_updates.insert(id.hash, (id.clone(), new_linear));
        self.sim.easing_registry().evaluate(easing, new_linear)
    }

    /// Consume the context and return the deferred registrations.
    pub fn finish(self) -> PendingRegistrations {
        self.pending.into_inner()
    }
}

// ── Event propagation model ───────────────────────────────────────────────────

/// The phase of an event as it travels through the UI tree.
///
/// Events flow in three phases:
/// 1. `Capture` — root to target, ancestors get first look
/// 2. `Target` — the element directly under the pointer or with focus
/// 3. `Bubble` — target back up to root, ancestors get a second look
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EventPhase {
    Capture,
    Target,
    #[default]
    Bubble,
}

/// Tracks propagation state for one event dispatch.
///
/// Pass to event handlers so they can call `stop_propagation()` to prevent
/// the event from reaching further elements in the current phase order.
#[derive(Clone, Debug)]
pub struct EventContext {
    phase: EventPhase,
    stopped: bool,
    default_prevented: bool,
}

impl EventContext {
    pub fn new(phase: EventPhase) -> Self {
        Self { phase, stopped: false, default_prevented: false }
    }

    pub fn phase(&self) -> EventPhase {
        self.phase
    }

    /// Returns `true` while the event is still allowed to propagate.
    pub fn is_propagating(&self) -> bool {
        !self.stopped
    }

    /// Prevent this event from reaching any more handlers.
    pub fn stop_propagation(&mut self) {
        self.stopped = true;
    }

    /// Prevent the widget's built-in behavior for this event.
    pub fn prevent_default(&mut self) {
        self.default_prevented = true;
    }

    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }
}

/// Records which input categories were consumed by the UI during the most
/// recent [`InputSimulator::update`] call.
///
/// Game and app layers should check this before processing the same raw
/// events themselves — if the UI consumed a key or pointer press, the
/// underlying game action should usually be suppressed.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct UiEventResult {
    /// A pointer press or release was handled by an interactive UI element.
    pub pointer_consumed: bool,
    /// `true` if any keyboard event was consumed by the UI this frame.
    pub key_consumed: bool,
    /// Names of individual keys consumed by the UI this frame.
    ///
    /// Use this for per-key dispatch priority instead of the coarser
    /// `key_consumed` flag — e.g. to allow a game action on `KeyA` even
    /// if the UI consumed `Enter` in the same frame.
    pub keys_consumed: HashSet<String>,
    /// One or more scroll events were absorbed by a UI scroll container.
    pub scroll_consumed: bool,
    /// Text input was routed to a focused text-input widget.
    pub text_consumed: bool,
}

/// Controls how [`InputSimulator`] processes input each frame.
///
/// Set via [`InputSimulator::set_mode`]. The default is [`UiMode::Active`].
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum UiMode {
    /// Normal operation: UI elements receive, process, and consume events.
    #[default]
    Active,
    /// UI receives events and updates hover/focus visual state, but reports
    /// nothing as consumed. Callers checking [`InputSimulator::last_event_result`]
    /// will see all flags `false`, so game/app layers process every event as
    /// if the UI were absent.
    ///
    /// Use this for menus that want hover highlights but must not block game input,
    /// or for screenshot/spectator modes where UI should be decorative only.
    Passthrough,
    /// All event processing is skipped. [`InputSimulator::update`] discards
    /// queued events and returns `None` immediately — zero cost above the
    /// function call itself.
    ///
    /// Use this when UI is hidden and should have no effect on input routing.
    Disabled,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum PointerButton {
    Primary,
    Secondary,
    Middle,
    Extra(u8),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InteractionPhase {
    PressedThisFrame,
    Pressed,
    ReleasedThisFrame,
    Released,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointerState {
    pub position: Vec2,
    pub button: PointerButton,
    pub phase: InteractionPhase,
}

impl Default for PointerState {
    fn default() -> Self {
        Self {
            position: Vec2::ZERO,
            button: PointerButton::Primary,
            phase: InteractionPhase::Released,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScrollState {
    pub offset: Vec2,
    pub delta: Vec2,
    pub momentum: Vec2,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ScrollAxis {
    #[default]
    Vertical,
    Horizontal,
    Both,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollConfig {
    pub viewport: Vec2,
    pub content: Vec2,
    pub axis: ScrollAxis,
    pub disabled: bool,
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self {
            viewport: Vec2::ZERO,
            content: Vec2::ZERO,
            axis: ScrollAxis::Vertical,
            disabled: false,
        }
    }
}

impl ScrollConfig {
    pub fn new(viewport: Vec2, content: Vec2) -> Self {
        Self {
            viewport,
            content,
            ..Self::default()
        }
    }

    pub fn axis(mut self, axis: ScrollAxis) -> Self {
        self.axis = axis;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn max_offset(self) -> Vec2 {
        Vec2::new(
            (self.content.x - self.viewport.x).max(0.0),
            (self.content.y - self.viewport.y).max(0.0),
        )
    }

    pub fn filter_delta(self, delta: Vec2) -> Vec2 {
        match self.axis {
            ScrollAxis::Vertical => Vec2::new(0.0, delta.y),
            ScrollAxis::Horizontal => Vec2::new(delta.x, 0.0),
            ScrollAxis::Both => delta,
        }
    }

    pub fn clamp_offset(self, offset: Vec2) -> Vec2 {
        let max = self.max_offset();
        Vec2::new(offset.x.clamp(0.0, max.x), offset.y.clamp(0.0, max.y))
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ModifierKeys {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiKeyEvent {
    pub target: ElementId,
    pub name: String,
    pub pressed: bool,
    pub repeat: bool,
    pub modifiers: ModifierKeys,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiTextEvent {
    pub target: ElementId,
    pub text: String,
    pub modifiers: ModifierKeys,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiPointerEvent {
    pub target: ElementId,
    pub pointer: PointerState,
}

#[derive(Clone, Debug, PartialEq)]
pub struct UiActivationEvent {
    pub target: ElementId,
}

type KeyCallback = Box<dyn FnMut(&UiKeyEvent, &mut EventContext)>;
type TextCallback = Box<dyn FnMut(&UiTextEvent, &mut EventContext)>;
type PointerCallback = Box<dyn FnMut(&UiPointerEvent, &mut EventContext)>;
type ActivationCallback = Box<dyn FnMut(&UiActivationEvent, &mut EventContext)>;

#[derive(Default)]
pub struct WidgetEventCallbacks {
    pub on_key_down: Option<KeyCallback>,
    pub on_key_up: Option<KeyCallback>,
    pub on_text: Option<TextCallback>,
    pub on_pointer_down: Option<PointerCallback>,
    pub on_pointer_up: Option<PointerCallback>,
    pub on_pointer_move: Option<PointerCallback>,
    pub on_activate: Option<ActivationCallback>,
}

impl WidgetEventCallbacks {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn on_key_down<F>(mut self, f: F) -> Self
    where
        F: FnMut(&UiKeyEvent, &mut EventContext) + 'static,
    {
        self.on_key_down = Some(Box::new(f));
        self
    }

    pub fn on_key_up<F>(mut self, f: F) -> Self
    where
        F: FnMut(&UiKeyEvent, &mut EventContext) + 'static,
    {
        self.on_key_up = Some(Box::new(f));
        self
    }

    pub fn on_text<F>(mut self, f: F) -> Self
    where
        F: FnMut(&UiTextEvent, &mut EventContext) + 'static,
    {
        self.on_text = Some(Box::new(f));
        self
    }

    pub fn on_pointer_down<F>(mut self, f: F) -> Self
    where
        F: FnMut(&UiPointerEvent, &mut EventContext) + 'static,
    {
        self.on_pointer_down = Some(Box::new(f));
        self
    }

    pub fn on_pointer_up<F>(mut self, f: F) -> Self
    where
        F: FnMut(&UiPointerEvent, &mut EventContext) + 'static,
    {
        self.on_pointer_up = Some(Box::new(f));
        self
    }

    pub fn on_pointer_move<F>(mut self, f: F) -> Self
    where
        F: FnMut(&UiPointerEvent, &mut EventContext) + 'static,
    {
        self.on_pointer_move = Some(Box::new(f));
        self
    }

    pub fn on_activate<F>(mut self, f: F) -> Self
    where
        F: FnMut(&UiActivationEvent, &mut EventContext) + 'static,
    {
        self.on_activate = Some(Box::new(f));
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    Pointer(PointerState),
    Scroll {
        target: Option<ElementId>,
        delta: Vec2,
    },
    /// A physical or logical key event.
    /// `name` uses web-standard key names: `"Enter"`, `"Space"`, `"ArrowUp"`,
    /// `"ArrowDown"`, `"ArrowLeft"`, `"ArrowRight"`, `"PageUp"`, `"PageDown"`,
    /// `"Home"`, `"End"`, `"Escape"`, `"Tab"`, `"Backspace"`, `"Delete"`, etc.
    Key {
        name: String,
        pressed: bool,
        repeat: bool,
    },
    KeyWithModifiers {
        name: String,
        pressed: bool,
        repeat: bool,
        modifiers: ModifierKeys,
    },
    Text(String),
    TextWithModifiers {
        text: String,
        modifiers: ModifierKeys,
    },
    Activate(ElementId),
    Focus(ElementId),
    Blur,
    Cancel,
}

// ── Widget behavior types ─────────────────────────────────────────────────────

/// The semantic kind of a widget, used to drive default input behaviors.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WidgetKind {
    /// Standard interactive element: button, checkbox, tab, list item.
    /// Default: Enter/Space activates when focused, click activates.
    Interactive,
    /// Element that owns a scroll offset.
    /// Default: wheel scrolls, arrow/page/home/end keys scroll when focused.
    ScrollArea,
    /// Draggable value control.
    /// Default: horizontal/vertical drag changes value, arrow keys step.
    Slider { axis: Axis },
    /// Resizer / splitter between panels.
    /// Default: drag produces a delta; no activation on release.
    DragBar { axis: Axis },
    /// Text editing field.
    /// Default: `InputEvent::Text` is routed to the focused text input.
    TextInput,
}

impl Default for WidgetKind {
    fn default() -> Self {
        Self::Interactive
    }
}

/// Per-widget opt-in/opt-out flags for default input behaviors.
/// All flags default to `true`. Register a behavior with specific flags set
/// to `false` to suppress that behavior for a particular widget.
#[derive(Clone, Debug, PartialEq)]
pub struct WidgetBehavior {
    pub kind: WidgetKind,
    /// Enter/Space activates the focused `Interactive` widget.
    pub keyboard_activate: bool,
    /// Arrow/Page/Home/End keys scroll a focused `ScrollArea`.
    pub keyboard_scroll: bool,
    /// Arrow keys step a focused `Slider` value.
    pub keyboard_slider: bool,
    /// Escape dispatches a `Cancel` event.
    pub keyboard_escape: bool,
    /// Pointer click/tap activates an `Interactive` widget.
    pub pointer_activate: bool,
    /// Scroll wheel is routed to a `ScrollArea`.
    pub pointer_scroll: bool,
    /// Pointer drag updates a `Slider` value or produces a `DragBar` delta.
    pub pointer_drag: bool,
}

impl Default for WidgetBehavior {
    fn default() -> Self {
        Self {
            kind: WidgetKind::Interactive,
            keyboard_activate: true,
            keyboard_scroll: true,
            keyboard_slider: true,
            keyboard_escape: true,
            pointer_activate: true,
            pointer_scroll: true,
            pointer_drag: true,
        }
    }
}

impl WidgetBehavior {
    pub fn interactive() -> Self {
        Self::default()
    }

    pub fn scroll_area() -> Self {
        Self {
            kind: WidgetKind::ScrollArea,
            pointer_activate: false,
            ..Self::default()
        }
    }

    pub fn slider(axis: Axis) -> Self {
        Self {
            kind: WidgetKind::Slider { axis },
            pointer_activate: false,
            ..Self::default()
        }
    }

    pub fn drag_bar(axis: Axis) -> Self {
        Self {
            kind: WidgetKind::DragBar { axis },
            keyboard_activate: false,
            pointer_activate: false,
            ..Self::default()
        }
    }

    pub fn text_input() -> Self {
        Self {
            kind: WidgetKind::TextInput,
            keyboard_activate: false,
            ..Self::default()
        }
    }
}

impl WidgetBehavior {
    pub fn keyboard_activate(mut self, enabled: bool) -> Self {
        self.keyboard_activate = enabled;
        self
    }

    pub fn keyboard_scroll(mut self, enabled: bool) -> Self {
        self.keyboard_scroll = enabled;
        self
    }

    pub fn keyboard_slider(mut self, enabled: bool) -> Self {
        self.keyboard_slider = enabled;
        self
    }

    pub fn keyboard_escape(mut self, enabled: bool) -> Self {
        self.keyboard_escape = enabled;
        self
    }

    pub fn pointer_activate(mut self, enabled: bool) -> Self {
        self.pointer_activate = enabled;
        self
    }

    pub fn pointer_scroll(mut self, enabled: bool) -> Self {
        self.pointer_scroll = enabled;
        self
    }

    pub fn pointer_drag(mut self, enabled: bool) -> Self {
        self.pointer_drag = enabled;
        self
    }
}

/// Configuration for a `Slider`-kind widget. Register with
/// `InputSimulator::set_slider_config` alongside the widget behavior.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SliderConfig {
    pub min: f32,
    pub max: f32,
    /// Starting value on the first frame. Defaults to `min`.
    pub initial: f32,
    /// Value change per arrow-key press.
    pub step: f32,
    /// Value change for Page Up / Page Down.
    pub large_step: f32,
    /// Pixel length of the draggable track (used to map drag delta → value).
    pub track_extent: f32,
}

impl Default for SliderConfig {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 1.0,
            initial: 0.0,
            step: 0.01,
            large_step: 0.1,
            track_extent: 100.0,
        }
    }
}

impl SliderConfig {
    pub fn new(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            initial: min,
            ..Self::default()
        }
    }

    /// Set the starting value (clamped to [min, max]).
    pub fn initial(mut self, value: f32) -> Self {
        self.initial = value.clamp(self.min, self.max);
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.step = step.abs();
        self
    }

    pub fn large_step(mut self, large_step: f32) -> Self {
        self.large_step = large_step.abs();
        self
    }

    pub fn track_extent(mut self, track_extent: f32) -> Self {
        self.track_extent = track_extent.max(1.0);
        self
    }

    fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }

    fn range(&self) -> f32 {
        (self.max - self.min).max(0.0)
    }
}

impl From<f32> for SliderConfig {
    fn from(value: f32) -> Self {
        SliderConfig::new(0.0, 1.0).initial(value)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: ElementId,
    pub shape: UiShape,
    pub layer: UiLayer,
    pub z_index: i16,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WidgetConfig {
    pub disabled: bool,
    pub read_only: bool,
    pub invalid: bool,
    pub accessibility_label: Option<String>,
}

impl WidgetConfig {
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    pub fn accessibility_label(mut self, label: impl Into<String>) -> Self {
        self.accessibility_label = Some(label.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct FocusScope {
    pub id: ElementId,
    pub root: ElementId,
    pub trap_focus: bool,
    pub block_background_input: bool,
    pub dismiss_on_outside_pointer: bool,
    pub dismiss_on_cancel: bool,
    pub restore_focus: Option<ElementId>,
}

impl FocusScope {
    pub fn new(id: ElementId, root: ElementId) -> Self {
        Self {
            id,
            root,
            trap_focus: false,
            block_background_input: false,
            dismiss_on_outside_pointer: false,
            dismiss_on_cancel: false,
            restore_focus: None,
        }
    }

    pub fn modal(id: ElementId, root: ElementId) -> Self {
        Self::new(id, root)
            .trap_focus(true)
            .block_background_input(true)
    }

    pub fn trap_focus(mut self, trap_focus: bool) -> Self {
        self.trap_focus = trap_focus;
        self
    }

    pub fn block_background_input(mut self, block_background_input: bool) -> Self {
        self.block_background_input = block_background_input;
        self
    }

    pub fn dismiss_on_outside_pointer(mut self, dismiss_on_outside_pointer: bool) -> Self {
        self.dismiss_on_outside_pointer = dismiss_on_outside_pointer;
        self
    }

    pub fn dismiss_on_cancel(mut self, dismiss_on_cancel: bool) -> Self {
        self.dismiss_on_cancel = dismiss_on_cancel;
        self
    }

    pub fn restore_focus(mut self, restore_focus: ElementId) -> Self {
        self.restore_focus = Some(restore_focus);
        self
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WidgetState {
    pub hovered: bool,
    pub focused: bool,
    pub pressed: bool,
    pub captured: bool,
    pub activated: bool,
    pub disabled: bool,
    pub read_only: bool,
    pub invalid: bool,
    pub accessibility_label: Option<String>,
}

#[derive(Default)]
pub struct InputSimulator {
    pointer: PointerState,
    focused: Option<ElementId>,
    hovered: Option<ElementId>,
    pressed: Option<ElementId>,
    captured: Option<ElementId>,
    active: HashSet<u64>,
    scrolls: HashMap<u64, ScrollState>,
    scroll_configs: HashMap<u64, ScrollConfig>,
    widgets: HashMap<u64, WidgetConfig>,
    focus_scopes: Vec<FocusScope>,
    dismissed_scopes: Vec<ElementId>,
    events: Vec<InputEvent>,
    // Behavior / advanced input
    behaviors: HashMap<u64, WidgetBehavior>,
    slider_configs: HashMap<u64, SliderConfig>,
    slider_values: HashMap<u64, f32>,
    /// (drag-start position, value at drag start) keyed by element hash.
    drag_origins: HashMap<u64, (Vec2, f32)>,
    /// Track rect captured on press; used for absolute-position slider mapping.
    slider_track_rects: HashMap<u64, Rect>,
    /// Linear animation progress (0=off, 1=on) for toggle widgets.
    toggle_anim: HashMap<u64, f32>,
    easing_registry: EasingRegistry,
    text_buffer: String,
    event_result: UiEventResult,
    mode: UiMode,
    /// Elements that want to be notified when a descendant activates (bubble).
    bubble_listeners: HashSet<u64>,
    /// Elements notified via bubble propagation during this update call.
    bubbled_activations: HashSet<u64>,
    callbacks: HashMap<u64, WidgetEventCallbacks>,
}

impl InputSimulator {
    pub fn queue(&mut self, event: InputEvent) {
        self.events.push(event);
    }

    /// Set the UI processing mode. Takes effect on the next call to [`update`](Self::update).
    pub fn set_mode(&mut self, mode: UiMode) {
        self.mode = mode;
    }

    /// Returns the current UI processing mode.
    pub fn mode(&self) -> UiMode {
        self.mode
    }

    pub fn pointer(&self) -> PointerState {
        self.pointer
    }

    pub fn focused(&self) -> Option<&ElementId> {
        self.focused.as_ref()
    }

    pub fn hovered(&self) -> Option<&ElementId> {
        self.hovered.as_ref()
    }

    pub fn captured(&self) -> Option<&ElementId> {
        self.captured.as_ref()
    }

    pub fn pressed(&self) -> Option<&ElementId> {
        self.pressed.as_ref()
    }

    pub fn set_widget_config(&mut self, id: ElementId, config: WidgetConfig) {
        self.widgets.insert(id.hash, config);
    }

    pub fn widget_config(&self, id: &ElementId) -> WidgetConfig {
        self.widgets.get(&id.hash).cloned().unwrap_or_default()
    }

    // ── Behavior API ─────────────────────────────────────────────────────────

    /// Register the input behavior for a widget. This controls which default
    /// input-to-action mappings apply and which can be opted out of.
    pub fn set_widget_behavior(&mut self, id: ElementId, behavior: WidgetBehavior) {
        self.behaviors.insert(id.hash, behavior);
    }

    /// Returns the registered behavior for `id`, or the default behavior if
    /// none has been registered (all flags on, `Interactive` kind).
    pub fn widget_behavior(&self, id: &ElementId) -> WidgetBehavior {
        self.behaviors.get(&id.hash).cloned().unwrap_or_default()
    }

    // ── Slider API ────────────────────────────────────────────────────────────

    /// Register a slider config. The simulator will update the slider value
    /// automatically on drag and on keyboard arrow keys when the element is
    /// focused.  Call this each frame alongside `set_widget_behavior`.
    pub fn set_slider_config(&mut self, id: ElementId, config: SliderConfig) {
        let clamped = config.clamp(*self.slider_values.entry(id.hash).or_insert(config.initial));
        self.slider_values.insert(id.hash, clamped);
        self.slider_configs.insert(id.hash, config);
    }

    /// Returns the current value for a registered slider (0.0 if not registered).
    pub fn slider_value(&self, id: &ElementId) -> f32 {
        self.slider_values.get(&id.hash).copied().unwrap_or(0.0)
    }

    /// Returns the registered [`SliderConfig`] for `id`, if any.
    pub fn slider_config(&self, id: &ElementId) -> Option<&SliderConfig> {
        self.slider_configs.get(&id.hash)
    }

    /// Programmatically set a slider value; clamped to [min, max] if a config
    /// is registered.
    pub fn set_slider_value(&mut self, id: &ElementId, value: f32) {
        let clamped = self
            .slider_configs
            .get(&id.hash)
            .map_or(value, |c| c.clamp(value));
        self.slider_values.insert(id.hash, clamped);
    }

    // ── Toggle animation API ──────────────────────────────────────────────────

    /// Returns the stored linear animation progress (0=off, 1=on) for a toggle,
    /// falling back to `default` when the toggle has not been registered yet.
    pub fn toggle_animation_progress(&self, id: &ElementId, default: f32) -> f32 {
        self.toggle_anim.get(&id.hash).copied().unwrap_or(default)
    }

    /// Store the linear animation progress for a toggle (called via
    /// [`PendingRegistrations::apply`]).
    pub fn set_toggle_animation_progress(&mut self, id: ElementId, progress: f32) {
        self.toggle_anim.insert(id.hash, progress.clamp(0.0, 1.0));
    }

    /// Returns a reference to the easing registry so widgets can evaluate
    /// custom easing curves registered by the application.
    pub fn easing_registry(&self) -> &EasingRegistry {
        &self.easing_registry
    }

    /// Returns a mutable reference to the easing registry for registering
    /// custom easing curves.
    pub fn easing_registry_mut(&mut self) -> &mut EasingRegistry {
        &mut self.easing_registry
    }

    // ── Drag bar API ──────────────────────────────────────────────────────────

    /// Returns the total drag displacement from the start of the current drag
    /// for a `DragBar`-kind element, or `None` if it is not currently captured.
    pub fn drag_total(&self, id: &ElementId) -> Option<Vec2> {
        if self.captured.as_ref().is_some_and(|c| c.hash == id.hash) {
            self.drag_origins
                .get(&id.hash)
                .map(|(start, _)| self.pointer.position - *start)
        } else {
            None
        }
    }

    /// Returns `true` while the element is being actively dragged (captured +
    /// pointer held).
    pub fn dragging(&self, id: &ElementId) -> bool {
        self.captured.as_ref().is_some_and(|c| c.hash == id.hash)
            && self.drag_origins.contains_key(&id.hash)
    }

    // ── Text input API ────────────────────────────────────────────────────────

    /// Returns text typed this frame that should be delivered to the currently
    /// focused `TextInput`-kind element, or an empty string if nothing was typed
    /// or the focused element is not a text input.
    pub fn text_this_frame(&self) -> &str {
        &self.text_buffer
    }

    /// Returns the element that should receive text input this frame, i.e. the
    /// focused element if it has `WidgetKind::TextInput` behavior.
    pub fn text_target(&self) -> Option<&ElementId> {
        let focused = self.focused.as_ref()?;
        let behavior = self.behaviors.get(&focused.hash)?;
        matches!(behavior.kind, WidgetKind::TextInput).then_some(focused)
    }

    /// Returns which input categories the UI consumed during the last
    /// [`update`](Self::update) call.
    ///
    /// Use this in game or app layers to skip handling events that were
    /// already handled by the UI — e.g. skip a "jump" action if the UI
    /// consumed the Space key for activating a focused button.
    pub fn last_event_result(&self) -> &UiEventResult {
        &self.event_result
    }

    /// Returns `true` if the UI consumed the named key during the last
    /// [`update`](Self::update) call.
    ///
    /// More precise than `last_event_result().key_consumed` when multiple
    /// different keys are pressed in one frame.
    pub fn key_input_consumed(&self, key_name: &str) -> bool {
        self.event_result.keys_consumed.contains(key_name)
    }

    // ── Propagation path queries ──────────────────────────────────────────────

    /// Returns the capture-phase path from the tree root down to `target`.
    ///
    /// The first element in the returned vec is the outermost ancestor; the
    /// last is `target` itself.  Walk this slice in order to implement
    /// ancestor-first (capture-phase) event handling: stop when an element
    /// is considered to have "handled" the event.
    ///
    /// Returns an empty vec if `target` is not in the current layout tree.
    pub fn propagation_path(&self, target: &ElementId, tree: &LayoutTree) -> Vec<ElementId> {
        if tree.by_id(target).is_none() {
            return Vec::new();
        }
        let parent_map: HashMap<u64, u64> =
            tree.nodes.iter().map(|n| (n.id.hash, n.parent)).collect();
        let id_map: HashMap<u64, ElementId> =
            tree.nodes.iter().map(|n| (n.id.hash, n.id.clone())).collect();

        let mut path = vec![target.clone()];
        let mut current = parent_map.get(&target.hash).copied().unwrap_or(0);
        for _ in 0..tree.nodes.len() {
            if current == 0 {
                break;
            }
            if let Some(id) = id_map.get(&current) {
                path.push(id.clone());
            }
            current = parent_map.get(&current).copied().unwrap_or(0);
        }
        path.reverse(); // root-first (capture order)
        path
    }

    /// Returns the bubble-phase path from `target` up to the tree root.
    ///
    /// This is the reverse of [`propagation_path`](Self::propagation_path) —
    /// walk it to implement target-first (bubble-phase) event handling.
    pub fn bubble_path(&self, target: &ElementId, tree: &LayoutTree) -> Vec<ElementId> {
        let mut path = self.propagation_path(target, tree);
        path.reverse();
        path
    }

    // ── Bubble listener API ───────────────────────────────────────────────────

    /// Register `id` as a bubble listener.
    ///
    /// Whenever any descendant of this element activates during [`update`](Self::update),
    /// the element is added to the bubbled-activations set for that frame so
    /// [`bubble_activated`](Self::bubble_activated) returns `true`.
    ///
    /// Typical use: a list container registers itself so it can detect which
    /// item was selected without knowing every item's ID in advance.
    pub fn set_bubble_listener(&mut self, id: ElementId) {
        self.bubble_listeners.insert(id.hash);
    }

    /// Remove a previously registered bubble listener.
    pub fn clear_bubble_listener(&mut self, id: &ElementId) {
        self.bubble_listeners.remove(&id.hash);
    }

    /// Returns `true` if a descendant of `id` activated during the last
    /// [`update`](Self::update) call and `id` was registered as a bubble listener.
    pub fn bubble_activated(&self, id: &ElementId) -> bool {
        self.bubbled_activations.contains(&id.hash)
    }

    pub fn set_event_callbacks(&mut self, id: ElementId, callbacks: WidgetEventCallbacks) {
        self.callbacks.insert(id.hash, callbacks);
    }

    pub fn clear_event_callbacks(&mut self, id: &ElementId) {
        self.callbacks.remove(&id.hash);
    }

    pub fn push_focus_scope(&mut self, scope: FocusScope) {
        self.focus_scopes.push(scope);
    }

    pub fn pop_focus_scope(&mut self) -> Option<FocusScope> {
        let scope = self.focus_scopes.pop()?;
        if let Some(restore_focus) = &scope.restore_focus {
            if !self.widget_config(restore_focus).disabled {
                self.focused = Some(restore_focus.clone());
            }
        } else if self
            .focused
            .as_ref()
            .is_some_and(|focused| focused.hash == scope.root.hash)
        {
            self.focused = None;
        }
        Some(scope)
    }

    pub fn clear_focus_scopes(&mut self) {
        self.focus_scopes.clear();
    }

    pub fn focus_scopes(&self) -> &[FocusScope] {
        &self.focus_scopes
    }

    pub fn active_focus_scope(&self) -> Option<&FocusScope> {
        self.focus_scopes.last()
    }

    pub fn dismissed_focus_scopes(&self) -> &[ElementId] {
        &self.dismissed_scopes
    }

    pub fn take_dismissed_focus_scopes(&mut self) -> Vec<ElementId> {
        std::mem::take(&mut self.dismissed_scopes)
    }

    pub fn widget_state(&self, id: &ElementId) -> WidgetState {
        let config = self.widget_config(id);
        WidgetState {
            hovered: self
                .hovered
                .as_ref()
                .is_some_and(|hovered| hovered.hash == id.hash),
            focused: self
                .focused
                .as_ref()
                .is_some_and(|focused| focused.hash == id.hash),
            pressed: self
                .pressed
                .as_ref()
                .is_some_and(|pressed| pressed.hash == id.hash),
            captured: self
                .captured
                .as_ref()
                .is_some_and(|captured| captured.hash == id.hash),
            activated: self.active.contains(&id.hash),
            disabled: config.disabled,
            read_only: config.read_only,
            invalid: config.invalid,
            accessibility_label: config.accessibility_label,
        }
    }

    pub fn scroll_state(&self, id: &ElementId) -> ScrollState {
        self.scrolls.get(&id.hash).copied().unwrap_or_default()
    }

    pub fn scroll_offset(&self, id: &ElementId) -> Vec2 {
        self.scroll_state(id).offset
    }

    pub fn scroll_layout_offset(&self, id: &ElementId) -> Vec2 {
        -self.scroll_offset(id)
    }

    pub fn set_scroll_config(&mut self, id: ElementId, config: ScrollConfig) {
        let state = self.scrolls.entry(id.hash).or_default();
        state.offset = config.clamp_offset(state.offset);
        self.scroll_configs.insert(id.hash, config);
    }

    pub fn set_scroll_offset(&mut self, id: &ElementId, offset: Vec2) {
        let offset = self
            .scroll_configs
            .get(&id.hash)
            .copied()
            .map_or(offset.max(Vec2::ZERO), |config| config.clamp_offset(offset));
        self.scrolls.entry(id.hash).or_default().offset = offset;
    }

    pub fn scroll_by(&mut self, id: &ElementId, delta: Vec2) {
        self.apply_scroll(id, delta);
    }

    pub fn scroll_to(&mut self, id: &ElementId, offset: Vec2) {
        let config = self
            .scroll_configs
            .get(&id.hash)
            .copied()
            .unwrap_or_default();
        if config.disabled {
            return;
        }

        let current = self.scroll_offset(id);
        let filtered_offset = match config.axis {
            ScrollAxis::Vertical => Vec2::new(current.x, offset.y),
            ScrollAxis::Horizontal => Vec2::new(offset.x, current.y),
            ScrollAxis::Both => offset,
        };
        let offset = config.clamp_offset(filtered_offset);
        let state = self.scrolls.entry(id.hash).or_default();
        state.delta += offset - state.offset;
        state.offset = offset;
    }

    pub fn scroll_page_by(&mut self, id: &ElementId, pages: Vec2) {
        let config = self
            .scroll_configs
            .get(&id.hash)
            .copied()
            .unwrap_or_default();
        self.apply_scroll(id, config.viewport * pages);
    }

    pub fn scroll_to_start(&mut self, id: &ElementId) {
        self.scroll_to(id, Vec2::ZERO);
    }

    pub fn scroll_to_end(&mut self, id: &ElementId) {
        let end = self
            .scroll_configs
            .get(&id.hash)
            .copied()
            .map_or(Vec2::ZERO, ScrollConfig::max_offset);
        self.scroll_to(id, end);
    }

    pub fn update(&mut self, tree: &LayoutTree) -> Option<Hit> {
        self.event_result = UiEventResult::default();

        // Disabled: discard queued events and do no work.
        if self.mode == UiMode::Disabled {
            self.events.clear();
            return None;
        }

        self.active.clear();
        self.bubbled_activations.clear();
        self.dismissed_scopes.clear();
        self.text_buffer.clear();
        for scroll in self.scrolls.values_mut() {
            scroll.delta = Vec2::ZERO;
        }
        let events = std::mem::take(&mut self.events);
        for event in events {
            match event {
                InputEvent::Pointer(pointer) => {
                    self.pointer = pointer;
                    let target = self.pointer_callback_target(tree, pointer);
                    let default_prevented = target
                        .as_ref()
                        .is_some_and(|id| self.dispatch_pointer_callbacks(tree, id, pointer));
                    if !default_prevented {
                        self.update_pointer_interaction(tree, pointer);
                    } else {
                        self.event_result.pointer_consumed = true;
                    }
                }
                InputEvent::Scroll { target, delta } => {
                    // Resolve the starting element: explicit target (if scope
                    // allows), or the interactive element under the cursor.
                    let start = match target {
                        Some(t) if self.input_allowed_by_active_scope(tree, &t) => Some(t),
                        Some(_) => None,
                        None => self
                            .hit_test_interactive(tree, self.pointer.position)
                            .map(|hit| hit.id),
                    };
                    if let Some(start) = start {
                        self.apply_scroll_propagating(tree, start.hash, delta);
                    }
                }
                InputEvent::Key { name, pressed, repeat } => {
                    self.handle_key_event(tree, &name, pressed, repeat, ModifierKeys::default());
                }
                InputEvent::KeyWithModifiers { name, pressed, repeat, modifiers } => {
                    self.handle_key_event(tree, &name, pressed, repeat, modifiers);
                }
                InputEvent::Text(text) => {
                    self.handle_text_event(tree, text, ModifierKeys::default());
                }
                InputEvent::TextWithModifiers { text, modifiers } => {
                    self.handle_text_event(tree, text, modifiers);
                }
                InputEvent::Activate(id) => {
                    self.activate_widget(tree, &id);
                }
                InputEvent::Focus(id) => {
                    if self.focus_allowed(tree, &id) {
                        self.focused = Some(id);
                    }
                }
                InputEvent::Blur => self.focused = None,
                InputEvent::Cancel => {
                    if let Some(scope) = self.active_focus_scope().cloned()
                        && scope.dismiss_on_cancel
                    {
                        self.dismissed_scopes.push(scope.id);
                    }
                }
            }
        }
        self.reconcile_active_focus_scope(tree);
        let hit = self.hit_test_interactive(tree, self.pointer.position);
        self.hovered = hit.as_ref().map(|hit| hit.id.clone());

        // Passthrough: visual state (hover, focus) updated above, but clear all
        // consumption flags so game/app layers see unfiltered events.
        if self.mode == UiMode::Passthrough {
            self.event_result = UiEventResult::default();
        }

        hit
    }

    pub fn hit_test(&self, tree: &LayoutTree, point: Vec2) -> Option<Hit> {
        tree.nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.shape.contains_point(node.rect, point))
            .max_by(|(ai, a), (bi, b)| {
                (a.layer, a.z_index)
                    .cmp(&(b.layer, b.z_index))
                    .then_with(|| bi.cmp(ai))
            })
            .map(|(_, node)| Hit {
                id: node.id.clone(),
                shape: node.shape,
                layer: node.layer,
                z_index: node.z_index,
            })
    }

    pub fn hit_test_interactive(&self, tree: &LayoutTree, point: Vec2) -> Option<Hit> {
        let active_scope = self.active_focus_scope();
        let scoped_parent_map = active_scope
            .filter(|scope| scope.block_background_input)
            .map(|_| layout_parent_map(tree));

        // Layout nodes are in post-order (children before parents), so among
        // nodes with equal (layer, z_index) we prefer the one with the smaller
        // index — that is the deepest/most-specific descendant under the cursor.
        tree.nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.shape.contains_point(node.rect, point))
            .filter(|(_, node)| !node.transparent_to_input)
            .filter(|(_, node)| !self.widget_config(&node.id).disabled)
            .filter(|(_, node)| match (active_scope, scoped_parent_map.as_ref()) {
                (Some(scope), Some(parents)) => {
                    focus_scope_contains_with_parent_map(scope, &node.id, parents, tree.nodes.len())
                }
                _ => true,
            })
            .max_by(|(ai, a), (bi, b)| {
                (a.layer, a.z_index)
                    .cmp(&(b.layer, b.z_index))
                    .then_with(|| bi.cmp(ai)) // lower index = deeper child wins
            })
            .map(|(_, node)| Hit {
                id: node.id.clone(),
                shape: node.shape,
                layer: node.layer,
                z_index: node.z_index,
            })
    }

    fn update_pointer_interaction(&mut self, tree: &LayoutTree, pointer: PointerState) {
        if pointer.phase == InteractionPhase::PressedThisFrame
            && let Some(scope) = self.active_focus_scope().cloned()
            && scope.dismiss_on_outside_pointer
            && !self.point_inside_focus_scope(tree, &scope, pointer.position)
        {
            self.dismissed_scopes.push(scope.id);
        }

        let hit = self.hit_test_interactive(tree, pointer.position);
        match pointer.phase {
            InteractionPhase::PressedThisFrame => {
                if let Some(hit) = hit {
                    self.event_result.pointer_consumed = true;
                    self.focused = Some(hit.id.clone());
                    self.pressed = Some(hit.id.clone());
                    // Record drag origin for drag bars (sliders use track-rect instead).
                    let start_value = self.slider_values.get(&hit.id.hash).copied().unwrap_or(0.0);
                    if let Some(behavior) = self.behaviors.get(&hit.id.hash) {
                        if behavior.pointer_drag
                            && matches!(
                                behavior.kind,
                                WidgetKind::Slider { .. } | WidgetKind::DragBar { .. }
                            )
                        {
                            self.drag_origins
                                .insert(hit.id.hash, (pointer.position, start_value));
                        }
                    }
                    // For sliders: capture the track rect and snap value to click position.
                    if let Some(behavior) = self.behaviors.get(&hit.id.hash).cloned() {
                        if behavior.pointer_drag {
                            if let WidgetKind::Slider { axis } = behavior.kind {
                                if let Some(node) = tree.nodes.iter().find(|n| n.id.hash == hit.id.hash) {
                                    let rect = node.rect;
                                    self.slider_track_rects.insert(hit.id.hash, rect);
                                    if let Some(config) = self.slider_configs.get(&hit.id.hash).copied() {
                                        let normalized = match axis {
                                            Axis::Horizontal => {
                                                let inner_left = rect.origin.x + 10.0;
                                                let inner_width = (rect.size.width - 20.0).max(f32::EPSILON);
                                                (pointer.position.x - inner_left) / inner_width
                                            }
                                            Axis::Vertical => {
                                                let inner_top = rect.origin.y + 10.0;
                                                let inner_height = (rect.size.height - 20.0).max(f32::EPSILON);
                                                (pointer.position.y - inner_top) / inner_height
                                            }
                                        };
                                        let new_value = config.clamp(config.min + normalized.clamp(0.0, 1.0) * config.range());
                                        self.slider_values.insert(hit.id.hash, new_value);
                                    }
                                }
                            }
                        }
                    }
                    self.captured = Some(hit.id);
                }
            }
            InteractionPhase::Pressed => {
                if let Some(captured) = &self.captured.clone() {
                    // Apply drag → slider value while pointer is held.
                    self.apply_drag_to_slider(captured, pointer.position);
                } else {
                    self.pressed = hit.map(|hit| hit.id);
                }
            }
            InteractionPhase::ReleasedThisFrame => {
                // Apply one last drag update before clearing captured state.
                if let Some(captured) = &self.captured.clone() {
                    self.apply_drag_to_slider(captured, pointer.position);
                }
                if let (Some(captured), Some(hit)) = (self.captured.take(), hit)
                    && captured.hash == hit.id.hash
                {
                    self.event_result.pointer_consumed = true;
                    let behavior = self.behaviors.get(&captured.hash);
                    let activate_ok = behavior.map_or(true, |b| b.pointer_activate);
                    if activate_ok {
                        self.activate_widget(tree, &captured);
                    }
                } else {
                    self.captured = None;
                }
                self.pressed = None;
                // Clear drag origin only on full release so drag_total() is
                // still readable on ReleasedThisFrame.
            }
            InteractionPhase::Released => {
                self.pressed = None;
                if let Some(old) = self.captured.take() {
                    self.drag_origins.remove(&old.hash);
                    self.slider_track_rects.remove(&old.hash);
                }
            }
        }
    }

    fn apply_drag_to_slider(&mut self, id: &ElementId, pointer_pos: Vec2) {
        let behavior = match self.behaviors.get(&id.hash) {
            Some(b) if b.pointer_drag => b.clone(),
            _ => return,
        };
        let WidgetKind::Slider { axis } = behavior.kind else {
            return;
        };
        let config = self.slider_configs.get(&id.hash).copied().unwrap_or_default();
        // Use the track rect captured on press for absolute-position mapping.
        // The slider layout has 2 px padding (from control_style) plus an 8 px
        // thumb radius, so the thumb CENTRE travels from track+10 to track+W−10,
        // giving inner_left = 10 and inner_width = W − 20.
        let Some(rect) = self.slider_track_rects.get(&id.hash).copied() else {
            return;
        };
        let normalized = match axis {
            Axis::Horizontal => {
                let inner_left = rect.origin.x + 10.0;
                let inner_width = (rect.size.width - 20.0).max(f32::EPSILON);
                (pointer_pos.x - inner_left) / inner_width
            }
            Axis::Vertical => {
                let inner_top = rect.origin.y + 10.0;
                let inner_height = (rect.size.height - 20.0).max(f32::EPSILON);
                (pointer_pos.y - inner_top) / inner_height
            }
        };
        let new_value = config.clamp(config.min + normalized.clamp(0.0, 1.0) * config.range());
        self.slider_values.insert(id.hash, new_value);
    }

    fn handle_key_event(
        &mut self,
        tree: &LayoutTree,
        name: &str,
        pressed: bool,
        repeat: bool,
        modifiers: ModifierKeys,
    ) {
        let focused = self
            .focused
            .clone()
            .filter(|id| !self.widget_config(id).disabled);

        if let Some(focused_id) = &focused {
            let key_event = UiKeyEvent {
                target: focused_id.clone(),
                name: name.to_string(),
                pressed,
                repeat,
                modifiers,
            };
            if self.dispatch_key_callbacks(tree, focused_id, &key_event) {
                self.event_result.key_consumed = true;
                self.event_result.keys_consumed.insert(name.to_string());
                return;
            }
        }

        // Escape is a default behavior too, so callbacks can prevent it and
        // focused widgets can opt out with WidgetBehavior::keyboard_escape(false).
        if name == "Escape" && pressed {
            let escape_ok = focused
                .as_ref()
                .and_then(|id| self.behaviors.get(&id.hash))
                .map_or(true, |behavior| behavior.keyboard_escape);
            if escape_ok {
                if let Some(scope) = self.active_focus_scope().cloned() {
                    if scope.dismiss_on_cancel {
                        self.dismissed_scopes.push(scope.id);
                        self.event_result.key_consumed = true;
                        self.event_result.keys_consumed.insert(name.to_string());
                        return;
                    }
                }
            }
        }

        let Some(focused) = focused else {
            return;
        };

        let behavior = self
            .behaviors
            .get(&focused.hash)
            .cloned()
            .unwrap_or_default();

        match behavior.kind {
            WidgetKind::Interactive | WidgetKind::TextInput => {
                if behavior.keyboard_activate
                    && pressed
                    && !matches!(behavior.kind, WidgetKind::TextInput)
                    && (name == "Enter" || name == "Space")
                {
                    if self.activate_widget(tree, &focused) {
                        self.event_result.key_consumed = true;
                        self.event_result.keys_consumed.insert(name.to_string());
                    }
                }
            }

            WidgetKind::ScrollArea => {
                if behavior.keyboard_scroll && (pressed || repeat) {
                    let config = self
                        .scroll_configs
                        .get(&focused.hash)
                        .copied()
                        .unwrap_or_default();
                    let delta = scroll_key_delta(name, config);
                    if delta != Vec2::ZERO {
                        // Use propagating scroll so keyboard scroll at the limit
                        // also flows to parent containers.
                        self.apply_scroll_propagating(tree, focused.hash, delta);
                        self.event_result.key_consumed = true;
                        self.event_result.keys_consumed.insert(name.to_string());
                    }
                }
            }

            WidgetKind::Slider { axis } => {
                if behavior.keyboard_slider && (pressed || repeat) {
                    let config = self
                        .slider_configs
                        .get(&focused.hash)
                        .copied()
                        .unwrap_or_default();
                    let delta = slider_key_delta(name, axis, &config);
                    if delta != 0.0 {
                        let current = self
                            .slider_values
                            .get(&focused.hash)
                            .copied()
                            .unwrap_or(config.min);
                        let new_value = config.clamp(current + delta);
                        self.slider_values.insert(focused.hash, new_value);
                        self.event_result.key_consumed = true;
                        self.event_result.keys_consumed.insert(name.to_string());
                    }
                }
            }

            WidgetKind::DragBar { .. } => {
                // Drag bars don't have a keyboard default behavior.
            }
        }
    }

    fn handle_text_event(&mut self, tree: &LayoutTree, text: String, modifiers: ModifierKeys) {
        let Some(target) = self.text_target().cloned() else {
            return;
        };
        let text_event = UiTextEvent { target: target.clone(), text, modifiers };
        if self.dispatch_text_callbacks(tree, &target, &text_event) {
            self.event_result.text_consumed = true;
            return;
        }
        if !text_event.text.is_empty() {
            self.text_buffer.push_str(&text_event.text);
            self.event_result.text_consumed = true;
        }
    }

    fn activate_widget(&mut self, tree: &LayoutTree, id: &ElementId) -> bool {
        if !self.activation_allowed(tree, id) {
            return false;
        }
        let event = UiActivationEvent { target: id.clone() };
        if self.dispatch_activation_callbacks(tree, id, &event) {
            self.event_result.pointer_consumed = true;
            return false;
        }
        self.active.insert(id.hash);
        self.propagate_bubble_activation(id.hash, tree);
        true
    }

    fn pointer_callback_target(&self, tree: &LayoutTree, pointer: PointerState) -> Option<ElementId> {
        match pointer.phase {
            InteractionPhase::Pressed | InteractionPhase::ReleasedThisFrame => {
                self.captured.clone().or_else(|| {
                    self.hit_test_interactive(tree, pointer.position).map(|hit| hit.id)
                })
            }
            InteractionPhase::PressedThisFrame | InteractionPhase::Released => {
                self.hit_test_interactive(tree, pointer.position).map(|hit| hit.id)
            }
        }
    }

    fn dispatch_key_callbacks(
        &mut self,
        tree: &LayoutTree,
        target: &ElementId,
        event: &UiKeyEvent,
    ) -> bool {
        self.dispatch_bubbling(tree, target, |callbacks, phase, prevented| {
            let callback = if event.pressed {
                callbacks.on_key_down.as_mut()
            } else {
                callbacks.on_key_up.as_mut()
            }?;
            let mut ctx = EventContext::new(phase);
            if prevented {
                ctx.prevent_default();
            }
            callback(event, &mut ctx);
            Some(ctx)
        })
    }

    fn dispatch_text_callbacks(
        &mut self,
        tree: &LayoutTree,
        target: &ElementId,
        event: &UiTextEvent,
    ) -> bool {
        self.dispatch_bubbling(tree, target, |callbacks, phase, prevented| {
            let callback = callbacks.on_text.as_mut()?;
            let mut ctx = EventContext::new(phase);
            if prevented {
                ctx.prevent_default();
            }
            callback(event, &mut ctx);
            Some(ctx)
        })
    }

    fn dispatch_pointer_callbacks(
        &mut self,
        tree: &LayoutTree,
        target: &ElementId,
        pointer: PointerState,
    ) -> bool {
        let event = UiPointerEvent { target: target.clone(), pointer };
        self.dispatch_bubbling(tree, target, |callbacks, phase, prevented| {
            let callback = match pointer.phase {
                InteractionPhase::PressedThisFrame => callbacks.on_pointer_down.as_mut(),
                InteractionPhase::ReleasedThisFrame => callbacks.on_pointer_up.as_mut(),
                InteractionPhase::Pressed | InteractionPhase::Released => callbacks.on_pointer_move.as_mut(),
            }?;
            let mut ctx = EventContext::new(phase);
            if prevented {
                ctx.prevent_default();
            }
            callback(&event, &mut ctx);
            Some(ctx)
        })
    }

    fn dispatch_activation_callbacks(
        &mut self,
        tree: &LayoutTree,
        target: &ElementId,
        event: &UiActivationEvent,
    ) -> bool {
        self.dispatch_bubbling(tree, target, |callbacks, phase, prevented| {
            let callback = callbacks.on_activate.as_mut()?;
            let mut ctx = EventContext::new(phase);
            if prevented {
                ctx.prevent_default();
            }
            callback(event, &mut ctx);
            Some(ctx)
        })
    }

    fn dispatch_bubbling(
        &mut self,
        tree: &LayoutTree,
        target: &ElementId,
        mut call: impl FnMut(&mut WidgetEventCallbacks, EventPhase, bool) -> Option<EventContext>,
    ) -> bool {
        let mut default_prevented = false;
        let path = self.bubble_path(target, tree);
        for (index, id) in path.iter().enumerate() {
            let phase = if index == 0 { EventPhase::Target } else { EventPhase::Bubble };
            let Some(callbacks) = self.callbacks.get_mut(&id.hash) else {
                continue;
            };
            let Some(ctx) = call(callbacks, phase, default_prevented) else {
                continue;
            };
            default_prevented |= ctx.default_prevented();
            if !ctx.is_propagating() {
                break;
            }
        }
        default_prevented
    }

    /// Walk from `source_hash` up the layout tree, marking every registered
    /// bubble listener as having received a bubbled activation.
    fn propagate_bubble_activation(&mut self, source_hash: u64, tree: &LayoutTree) {
        if self.bubble_listeners.is_empty() {
            return;
        }
        let parent_map: HashMap<u64, u64> =
            tree.nodes.iter().map(|n| (n.id.hash, n.parent)).collect();
        let mut current = parent_map.get(&source_hash).copied().unwrap_or(0);
        for _ in 0..tree.nodes.len() {
            if current == 0 {
                break;
            }
            if self.bubble_listeners.contains(&current) {
                self.bubbled_activations.insert(current);
            }
            current = parent_map.get(&current).copied().unwrap_or(0);
        }
    }

    /// Walk from `start_hash` up the layout tree applying scroll delta to every
    /// registered scroll container along the way.  Each container consumes only
    /// what it can actually move (clamped to its own max offset), and the
    /// per-axis remainder propagates to the next ancestor.  The ancestor does
    /// not need to be the direct parent — any registered scroll container that
    /// is an ancestor in the layout tree will receive leftover delta.
    fn apply_scroll_propagating(&mut self, tree: &LayoutTree, start_hash: u64, delta: Vec2) {
        if delta.x.abs() < 0.001 && delta.y.abs() < 0.001 {
            return;
        }
        // Build a child→parent hash map for this frame's tree once.
        let parent_map: HashMap<u64, u64> =
            tree.nodes.iter().map(|n| (n.id.hash, n.parent)).collect();

        let mut remaining = delta;
        let mut current = start_hash;

        loop {
            if remaining.x.abs() < 0.5 && remaining.y.abs() < 0.5 {
                break;
            }

            // Try to consume scroll at this node.
            if let Some(config) = self.scroll_configs.get(&current).copied() {
                let scroll_ok =
                    self.behaviors.get(&current).map_or(true, |b| b.pointer_scroll);
                if scroll_ok && !config.disabled {
                    let consumed = self.consume_scroll(current, remaining, config);
                    remaining -= consumed;
                }
            }

            // Walk to parent; stop at root (parent == 0) or unknown node.
            let parent = parent_map.get(&current).copied().unwrap_or(0);
            if parent == 0 {
                break;
            }
            current = parent;
        }
    }

    /// Apply as much of `delta` as the scroll container at `hash` can absorb
    /// and return the amount actually consumed (per axis).
    fn consume_scroll(&mut self, hash: u64, delta: Vec2, config: ScrollConfig) -> Vec2 {
        let filtered = config.filter_delta(delta);
        let state = self.scrolls.entry(hash).or_default();
        let before = state.offset;
        state.offset = config.clamp_offset(state.offset + filtered);
        let consumed = state.offset - before;
        state.delta += consumed;
        if consumed != Vec2::ZERO {
            self.event_result.scroll_consumed = true;
        }
        consumed
    }

    fn apply_scroll(&mut self, id: &ElementId, delta: Vec2) {
        let config = self
            .scroll_configs
            .get(&id.hash)
            .copied()
            .unwrap_or_default();
        if config.disabled {
            return;
        }

        let delta = config.filter_delta(delta);
        let state = self.scrolls.entry(id.hash).or_default();
        state.delta += delta;
        state.offset = config.clamp_offset(state.offset + delta);
    }

    fn activation_allowed(&self, tree: &LayoutTree, id: &ElementId) -> bool {
        !self.widget_config(id).disabled && self.scope_allows_widget_input(tree, id)
    }

    fn focus_allowed(&self, tree: &LayoutTree, id: &ElementId) -> bool {
        !self.widget_config(id).disabled && self.scope_allows_widget_input(tree, id)
    }

    fn input_allowed_by_active_scope(&self, tree: &LayoutTree, id: &ElementId) -> bool {
        match self.active_focus_scope() {
            Some(scope) if scope.block_background_input => focus_scope_contains(tree, scope, id),
            _ => true,
        }
    }

    fn scope_allows_widget_input(&self, tree: &LayoutTree, id: &ElementId) -> bool {
        match self.active_focus_scope() {
            Some(scope) if scope.block_background_input || scope.trap_focus => {
                focus_scope_contains(tree, scope, id)
            }
            _ => true,
        }
    }

    fn reconcile_active_focus_scope(&mut self, tree: &LayoutTree) {
        let Some(scope) = self.active_focus_scope().cloned() else {
            return;
        };
        if !scope.trap_focus {
            return;
        }

        let focus_allowed = self
            .focused
            .as_ref()
            .is_some_and(|focused| focus_scope_contains(tree, &scope, focused));
        if focus_allowed {
            return;
        }

        self.focused = self.focus_scope_fallback(tree, &scope);
    }

    fn focus_scope_fallback(&self, tree: &LayoutTree, scope: &FocusScope) -> Option<ElementId> {
        if tree.by_id(&scope.root).is_some() && !self.widget_config(&scope.root).disabled {
            return Some(scope.root.clone());
        }

        let parents = layout_parent_map(tree);
        tree.nodes
            .iter()
            .filter(|node| {
                focus_scope_contains_with_parent_map(scope, &node.id, &parents, tree.nodes.len())
            })
            .find(|node| !node.transparent_to_input && !self.widget_config(&node.id).disabled)
            .map(|node| node.id.clone())
    }

    fn point_inside_focus_scope(&self, tree: &LayoutTree, scope: &FocusScope, point: Vec2) -> bool {
        let parents = layout_parent_map(tree);
        tree.nodes.iter().any(|node| {
            focus_scope_contains_with_parent_map(scope, &node.id, &parents, tree.nodes.len())
                && node.shape.contains_point(node.rect, point)
        })
    }
}

fn scroll_key_delta(name: &str, config: ScrollConfig) -> Vec2 {
    let line = match config.axis {
        ScrollAxis::Vertical => Vec2::new(0.0, 24.0),
        ScrollAxis::Horizontal => Vec2::new(24.0, 0.0),
        ScrollAxis::Both => Vec2::splat(24.0),
    };
    let page = match config.axis {
        ScrollAxis::Vertical => Vec2::new(0.0, config.viewport.y * 0.9),
        ScrollAxis::Horizontal => Vec2::new(config.viewport.x * 0.9, 0.0),
        ScrollAxis::Both => config.viewport * 0.9,
    };
    match name {
        "ArrowDown" => line,
        "ArrowUp" => -line,
        "ArrowRight" if matches!(config.axis, ScrollAxis::Horizontal | ScrollAxis::Both) => {
            Vec2::new(line.x, 0.0)
        }
        "ArrowLeft" if matches!(config.axis, ScrollAxis::Horizontal | ScrollAxis::Both) => {
            Vec2::new(-line.x, 0.0)
        }
        "PageDown" => page,
        "PageUp" => -page,
        // End/Home: return a large delta; apply_scroll clamps to max_offset.
        "End" => config.max_offset() * 2.0,
        "Home" => -(config.max_offset() * 2.0),
        _ => Vec2::ZERO,
    }
}

fn slider_key_delta(name: &str, axis: Axis, config: &SliderConfig) -> f32 {
    let (pos, neg) = match axis {
        Axis::Horizontal => ("ArrowRight", "ArrowLeft"),
        Axis::Vertical => ("ArrowDown", "ArrowUp"),
    };
    if name == pos {
        config.step
    } else if name == neg {
        -config.step
    } else if name == "PageDown" {
        config.large_step
    } else if name == "PageUp" {
        -config.large_step
    } else if name == "End" {
        config.range() // clamp will pin to max
    } else if name == "Home" {
        -config.range() // clamp will pin to min
    } else {
        0.0
    }
}

fn focus_scope_contains(tree: &LayoutTree, scope: &FocusScope, id: &ElementId) -> bool {
    let parents = layout_parent_map(tree);
    focus_scope_contains_with_parent_map(scope, id, &parents, tree.nodes.len())
}

fn layout_parent_map(tree: &LayoutTree) -> HashMap<u64, u64> {
    let mut parents = HashMap::with_capacity(tree.nodes.len());
    for node in &tree.nodes {
        parents.insert(node.id.hash, node.parent);
    }
    parents
}

fn focus_scope_contains_with_parent_map(
    scope: &FocusScope,
    id: &ElementId,
    parents: &HashMap<u64, u64>,
    limit: usize,
) -> bool {
    if id.hash == scope.root.hash {
        return true;
    }

    let mut parent = parents.get(&id.hash).copied().unwrap_or(id.parent);
    for _ in 0..=limit {
        if parent == 0 {
            return false;
        }
        if parent == scope.root.hash {
            return true;
        }
        let Some(next_parent) = parents.get(&parent).copied() else {
            return false;
        };
        parent = next_parent;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Element, ElementId, LayoutCache, LayoutOutput, LayoutSizing, LayoutTree, Rect, Size,
    };

    fn test_element(id: ElementId) -> Element {
        let mut element = Element::new(id);
        element.layout.width = LayoutSizing::Fixed(100.0);
        element.layout.height = LayoutSizing::Fixed(40.0);
        element
    }

    fn layout_for(element: &Element) -> LayoutTree {
        LayoutTree::compute(element, Size::new(100.0, 40.0), &mut LayoutCache::default()).unwrap()
    }

    fn layout_node(
        id: ElementId,
        parent: u64,
        rect: Rect,
        layer: UiLayer,
        z_index: i16,
        transparent_to_input: bool,
    ) -> LayoutOutput {
        LayoutOutput {
            id,
            parent,
            rect,
            content_size: rect.size,
            shape: UiShape::Rect,
            layer,
            z_index,
            clip: false,
            transparent_to_input,
        }
    }

    #[test]
    fn widget_state_tracks_hover_focus_press_capture_and_activation() {
        let id = ElementId::new("button");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        let state = input.widget_state(&id);
        assert!(state.hovered);
        assert!(state.focused);
        assert!(state.pressed);
        assert!(state.captured);
        assert!(!state.activated);

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::ReleasedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        let state = input.widget_state(&id);
        assert!(state.hovered);
        assert!(state.focused);
        assert!(!state.pressed);
        assert!(!state.captured);
        assert!(state.activated);
    }

    #[test]
    fn disabled_widgets_do_not_focus_capture_or_activate() {
        let id = ElementId::new("disabled-button");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_widget_config(id.clone(), WidgetConfig::default().disabled(true));

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Activate(id.clone()));
        let hit = input.update(&layout);

        let state = input.widget_state(&id);
        assert!(hit.is_none());
        assert!(state.disabled);
        assert!(!state.hovered);
        assert!(!state.focused);
        assert!(!state.pressed);
        assert!(!state.captured);
        assert!(!state.activated);
    }

    #[test]
    fn widget_state_exposes_readonly_invalid_and_accessibility_metadata() {
        let id = ElementId::new("field");
        let mut input = InputSimulator::default();
        input.set_widget_config(
            id.clone(),
            WidgetConfig::default()
                .read_only(true)
                .invalid(true)
                .accessibility_label("Username"),
        );

        let state = input.widget_state(&id);

        assert!(state.read_only);
        assert!(state.invalid);
        assert_eq!(state.accessibility_label.as_deref(), Some("Username"));
    }

    #[test]
    fn targeted_scroll_updates_and_clamps_offset() {
        let id = ElementId::new("scroll");
        let mut input = InputSimulator::default();
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(100.0, 80.0), Vec2::new(100.0, 240.0)),
        );

        input.queue(InputEvent::Scroll {
            target: Some(id.clone()),
            delta: Vec2::new(10.0, 96.0),
        });
        input.update(&LayoutTree::default());

        let state = input.scroll_state(&id);
        assert_eq!(state.delta, Vec2::new(0.0, 96.0));
        assert_eq!(state.offset, Vec2::new(0.0, 96.0));
        assert_eq!(input.scroll_layout_offset(&id), Vec2::new(0.0, -96.0));

        input.queue(InputEvent::Scroll {
            target: Some(id.clone()),
            delta: Vec2::new(0.0, 500.0),
        });
        input.update(&LayoutTree::default());

        assert_eq!(input.scroll_offset(&id), Vec2::new(0.0, 160.0));
    }

    #[test]
    fn untargeted_scroll_uses_interactive_hit() {
        let id = ElementId::new("scroll-hit");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(100.0, 40.0), Vec2::new(100.0, 120.0)),
        );
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::Released,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Scroll {
            target: None,
            delta: Vec2::new(0.0, 24.0),
        });

        input.update(&layout);

        assert_eq!(input.scroll_offset(&id), Vec2::new(0.0, 24.0));
    }

    #[test]
    fn horizontal_scroll_ignores_vertical_delta() {
        let id = ElementId::new("horizontal-scroll");
        let mut input = InputSimulator::default();
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(80.0, 40.0), Vec2::new(200.0, 400.0))
                .axis(ScrollAxis::Horizontal),
        );
        input.queue(InputEvent::Scroll {
            target: Some(id.clone()),
            delta: Vec2::new(44.0, 96.0),
        });

        input.update(&LayoutTree::default());

        assert_eq!(input.scroll_state(&id).delta, Vec2::new(44.0, 0.0));
        assert_eq!(input.scroll_offset(&id), Vec2::new(44.0, 0.0));
    }

    #[test]
    fn programmatic_scroll_helpers_clamp_and_report_delta() {
        let id = ElementId::new("programmatic-scroll");
        let mut input = InputSimulator::default();
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(100.0, 80.0), Vec2::new(300.0, 260.0))
                .axis(ScrollAxis::Both),
        );

        input.scroll_by(&id, Vec2::new(40.0, 96.0));
        assert_eq!(input.scroll_state(&id).delta, Vec2::new(40.0, 96.0));
        assert_eq!(input.scroll_offset(&id), Vec2::new(40.0, 96.0));

        input.update(&LayoutTree::default());
        assert_eq!(input.scroll_state(&id).delta, Vec2::ZERO);

        input.scroll_page_by(&id, Vec2::new(1.0, 1.0));
        assert_eq!(input.scroll_state(&id).delta, Vec2::new(100.0, 80.0));
        assert_eq!(input.scroll_offset(&id), Vec2::new(140.0, 176.0));

        input.update(&LayoutTree::default());
        input.scroll_to_end(&id);
        assert_eq!(input.scroll_offset(&id), Vec2::new(200.0, 180.0));
        assert_eq!(input.scroll_state(&id).delta, Vec2::new(60.0, 4.0));

        input.update(&LayoutTree::default());
        input.scroll_to_start(&id);
        assert_eq!(input.scroll_offset(&id), Vec2::ZERO);
        assert_eq!(input.scroll_state(&id).delta, Vec2::new(-200.0, -180.0));
    }

    #[test]
    fn programmatic_scroll_respects_disabled_and_axis() {
        let id = ElementId::new("disabled-scroll");
        let mut input = InputSimulator::default();
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(80.0, 40.0), Vec2::new(200.0, 160.0))
                .axis(ScrollAxis::Vertical)
                .disabled(true),
        );

        input.scroll_by(&id, Vec2::new(50.0, 50.0));
        input.scroll_to_end(&id);
        assert_eq!(input.scroll_state(&id), ScrollState::default());

        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(80.0, 40.0), Vec2::new(200.0, 160.0))
                .axis(ScrollAxis::Vertical),
        );
        input.scroll_by(&id, Vec2::new(50.0, 50.0));
        input.scroll_to(&id, Vec2::new(120.0, 80.0));

        assert_eq!(input.scroll_offset(&id), Vec2::new(0.0, 80.0));
        assert_eq!(input.scroll_state(&id).delta, Vec2::new(0.0, 80.0));
    }

    #[test]
    fn hit_testing_prefers_higher_layers_over_z_index() {
        let base_id = ElementId::new("base");
        let overlay_id = ElementId::new("overlay");
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        let layout = LayoutTree {
            nodes: vec![
                LayoutOutput {
                    id: base_id,
                    parent: 0,
                    rect,
                    content_size: rect.size,
                    shape: UiShape::Rect,
                    layer: UiLayer::Content,
                    z_index: 100,
                    clip: false,
                    transparent_to_input: false,
                },
                LayoutOutput {
                    id: overlay_id.clone(),
                    parent: 0,
                    rect,
                    content_size: rect.size,
                    shape: UiShape::Rect,
                    layer: UiLayer::Overlay,
                    z_index: 0,
                    clip: false,
                    transparent_to_input: false,
                },
            ],
        };
        let input = InputSimulator::default();

        let hit = input.hit_test_interactive(&layout, Vec2::new(10.0, 10.0));

        assert_eq!(hit.map(|hit| hit.id.hash), Some(overlay_id.hash));
    }

    #[test]
    fn hit_testing_uses_resolved_shape_coverage() {
        let shaped_id = ElementId::new("rounded-button");
        let mut element = test_element(shaped_id.clone());
        element.style.corner_radius = crate::radii_all(20.0);
        let layout = layout_for(&element);
        let input = InputSimulator::default();

        let corner_hit = input.hit_test_interactive(&layout, Vec2::new(1.0, 1.0));
        let body_hit = input.hit_test_interactive(&layout, Vec2::new(50.0, 20.0));

        assert!(corner_hit.is_none());
        assert_eq!(body_hit.map(|hit| hit.id.hash), Some(shaped_id.hash));
    }

    #[test]
    fn transparent_elements_do_not_steal_interactive_hits() {
        let base_id = ElementId::new("base");
        let overlay_id = ElementId::new("overlay");
        let rect = Rect::new(0.0, 0.0, 100.0, 40.0);
        let layout = LayoutTree {
            nodes: vec![
                layout_node(base_id.clone(), 0, rect, UiLayer::Content, 0, false),
                layout_node(overlay_id, 0, rect, UiLayer::TopLayer, 10, true),
            ],
        };
        let input = InputSimulator::default();

        let hit = input.hit_test_interactive(&layout, Vec2::new(10.0, 10.0));

        assert_eq!(hit.map(|hit| hit.id.hash), Some(base_id.hash));
    }

    #[test]
    fn focus_scope_blocks_background_hit_testing() {
        let background_id = ElementId::new("background");
        let modal_id = ElementId::new("modal");
        let modal_child_id = ElementId::local("button", 0, &modal_id);
        let layout = LayoutTree {
            nodes: vec![
                layout_node(
                    background_id.clone(),
                    0,
                    Rect::new(0.0, 0.0, 100.0, 100.0),
                    UiLayer::Content,
                    0,
                    false,
                ),
                layout_node(
                    modal_id.clone(),
                    0,
                    Rect::new(30.0, 30.0, 40.0, 40.0),
                    UiLayer::TopLayer,
                    0,
                    true,
                ),
                layout_node(
                    modal_child_id.clone(),
                    modal_id.hash,
                    Rect::new(35.0, 35.0, 20.0, 20.0),
                    UiLayer::TopLayer,
                    1,
                    false,
                ),
            ],
        };
        let mut input = InputSimulator::default();
        input.push_focus_scope(FocusScope::modal(ElementId::new("scope"), modal_id));

        let background_hit = input.hit_test_interactive(&layout, Vec2::new(10.0, 10.0));
        let modal_hit = input.hit_test_interactive(&layout, Vec2::new(36.0, 36.0));

        assert!(background_hit.is_none());
        assert_eq!(modal_hit.map(|hit| hit.id.hash), Some(modal_child_id.hash));

        input.queue(InputEvent::Activate(background_id.clone()));
        input.update(&layout);
        assert!(!input.widget_state(&background_id).activated);

        input.queue(InputEvent::Activate(modal_child_id.clone()));
        input.update(&layout);
        assert!(input.widget_state(&modal_child_id).activated);
    }

    #[test]
    fn focus_scope_traps_focus_and_restores_previous_focus() {
        let background_id = ElementId::new("background");
        let modal_id = ElementId::new("modal");
        let modal_child_id = ElementId::local("button", 0, &modal_id);
        let layout = LayoutTree {
            nodes: vec![
                layout_node(
                    background_id.clone(),
                    0,
                    Rect::new(0.0, 0.0, 100.0, 100.0),
                    UiLayer::Content,
                    0,
                    false,
                ),
                layout_node(
                    modal_id.clone(),
                    0,
                    Rect::new(20.0, 20.0, 60.0, 60.0),
                    UiLayer::TopLayer,
                    0,
                    false,
                ),
                layout_node(
                    modal_child_id.clone(),
                    modal_id.hash,
                    Rect::new(30.0, 30.0, 20.0, 20.0),
                    UiLayer::TopLayer,
                    1,
                    false,
                ),
            ],
        };
        let mut input = InputSimulator::default();
        input.queue(InputEvent::Focus(background_id.clone()));
        input.update(&layout);
        assert_eq!(input.focused().map(|id| id.hash), Some(background_id.hash));

        input.push_focus_scope(
            FocusScope::modal(ElementId::new("scope"), modal_id.clone())
                .restore_focus(background_id.clone()),
        );
        input.queue(InputEvent::Focus(background_id.clone()));
        input.update(&layout);
        assert_eq!(input.focused().map(|id| id.hash), Some(modal_id.hash));

        input.queue(InputEvent::Focus(modal_child_id.clone()));
        input.update(&layout);
        assert_eq!(input.focused().map(|id| id.hash), Some(modal_child_id.hash));

        input.pop_focus_scope();

        assert_eq!(input.focused().map(|id| id.hash), Some(background_id.hash));
    }

    #[test]
    fn focus_scope_reports_outside_pointer_dismissal() {
        let background_id = ElementId::new("background");
        let popover_id = ElementId::new("popover");
        let layout = LayoutTree {
            nodes: vec![
                layout_node(
                    background_id,
                    0,
                    Rect::new(0.0, 0.0, 200.0, 120.0),
                    UiLayer::Content,
                    0,
                    false,
                ),
                layout_node(
                    popover_id.clone(),
                    0,
                    Rect::new(40.0, 30.0, 80.0, 40.0),
                    UiLayer::TopLayer,
                    0,
                    false,
                ),
            ],
        };
        let scope_id = ElementId::new("popover-scope");
        let mut input = InputSimulator::default();
        input.push_focus_scope(
            FocusScope::new(scope_id.clone(), popover_id.clone())
                .block_background_input(true)
                .dismiss_on_outside_pointer(true),
        );

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        assert_eq!(
            input
                .dismissed_focus_scopes()
                .iter()
                .map(|id| id.hash)
                .collect::<Vec<_>>(),
            vec![scope_id.hash]
        );
        assert_eq!(
            input
                .take_dismissed_focus_scopes()
                .iter()
                .map(|id| id.hash)
                .collect::<Vec<_>>(),
            vec![scope_id.hash]
        );
        assert!(input.dismissed_focus_scopes().is_empty());

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(50.0, 40.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);
        assert!(input.dismissed_focus_scopes().is_empty());
    }

    #[test]
    fn focus_scope_reports_cancel_dismissal() {
        let popover_id = ElementId::new("popover");
        let layout = LayoutTree {
            nodes: vec![layout_node(
                popover_id.clone(),
                0,
                Rect::new(40.0, 30.0, 80.0, 40.0),
                UiLayer::TopLayer,
                0,
                false,
            )],
        };
        let scope_id = ElementId::new("popover-scope");
        let mut input = InputSimulator::default();
        input.push_focus_scope(
            FocusScope::new(scope_id.clone(), popover_id).dismiss_on_cancel(true),
        );

        input.queue(InputEvent::Cancel);
        input.update(&layout);

        assert_eq!(
            input
                .dismissed_focus_scopes()
                .iter()
                .map(|id| id.hash)
                .collect::<Vec<_>>(),
            vec![scope_id.hash]
        );
    }

    // ── Behavior / key / slider tests ─────────────────────────────────────────

    #[test]
    fn enter_activates_focused_interactive_widget() {
        let id = ElementId::new("btn");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();

        // Focus the element via pointer press then release on it.
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::ReleasedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        // Now send Enter key.
        input.queue(InputEvent::Key {
            name: "Enter".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&layout);

        assert!(input.widget_state(&id).activated);
    }

    #[test]
    fn space_activates_focused_interactive_widget() {
        let id = ElementId::new("btn");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&layout);
        input.queue(InputEvent::Key {
            name: "Space".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&layout);

        assert!(input.widget_state(&id).activated);
    }

    #[test]
    fn keyboard_activate_false_suppresses_enter() {
        let id = ElementId::new("btn");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_widget_behavior(
            id.clone(),
            WidgetBehavior {
                keyboard_activate: false,
                ..WidgetBehavior::interactive()
            },
        );
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&layout);
        input.queue(InputEvent::Key {
            name: "Enter".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&layout);

        assert!(!input.widget_state(&id).activated);
    }

    #[test]
    fn pointer_activate_false_suppresses_click_activation() {
        let id = ElementId::new("btn");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_widget_behavior(
            id.clone(),
            WidgetBehavior {
                pointer_activate: false,
                ..WidgetBehavior::interactive()
            },
        );
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::ReleasedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        assert!(!input.widget_state(&id).activated);
    }

    #[test]
    fn arrow_keys_scroll_focused_scroll_area() {
        let id = ElementId::new("scroll");
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::scroll_area());
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(200.0, 100.0), Vec2::new(200.0, 400.0)),
        );
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&LayoutTree::default());

        input.queue(InputEvent::Key {
            name: "ArrowDown".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&LayoutTree::default());

        assert!(input.scroll_offset(&id).y > 0.0);
    }

    #[test]
    fn page_down_scrolls_by_viewport_fraction() {
        let id = ElementId::new("scroll");
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::scroll_area());
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(200.0, 100.0), Vec2::new(200.0, 500.0)),
        );
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&LayoutTree::default());

        input.queue(InputEvent::Key {
            name: "PageDown".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&LayoutTree::default());

        // Page delta is viewport.y * 0.9 = 90.0; clamped to max_offset = 400.
        assert_eq!(input.scroll_offset(&id).y, 90.0);
    }

    #[test]
    fn pointer_scroll_false_ignores_wheel_events() {
        let id = ElementId::new("scroll");
        let mut input = InputSimulator::default();
        input.set_widget_behavior(
            id.clone(),
            WidgetBehavior {
                pointer_scroll: false,
                ..WidgetBehavior::scroll_area()
            },
        );
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(200.0, 100.0), Vec2::new(200.0, 400.0)),
        );
        input.queue(InputEvent::Scroll {
            target: Some(id.clone()),
            delta: Vec2::new(0.0, 50.0),
        });
        input.update(&LayoutTree::default());

        assert_eq!(input.scroll_offset(&id), Vec2::ZERO);
    }

    #[test]
    fn arrow_keys_step_slider_value() {
        let id = ElementId::new("slider");
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::slider(Axis::Horizontal));
        input.set_slider_config(
            id.clone(),
            SliderConfig::new(0.0, 1.0).step(0.1).track_extent(200.0),
        );
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&LayoutTree::default());

        input.queue(InputEvent::Key {
            name: "ArrowRight".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&LayoutTree::default());

        assert!((input.slider_value(&id) - 0.1).abs() < 1e-5);
    }

    #[test]
    fn slider_value_clamped_at_max() {
        let id = ElementId::new("slider");
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::slider(Axis::Horizontal));
        input.set_slider_config(
            id.clone(),
            SliderConfig::new(0.0, 1.0).step(0.3).track_extent(100.0),
        );
        input.set_slider_value(&id, 0.9);
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&LayoutTree::default());
        for _ in 0..5 {
            input.queue(InputEvent::Key {
                name: "ArrowRight".into(),
                pressed: true,
                repeat: false,
            });
        }
        input.update(&LayoutTree::default());

        assert_eq!(input.slider_value(&id), 1.0);
    }

    #[test]
    fn pointer_drag_updates_slider_value() {
        let id = ElementId::new("slider");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::slider(Axis::Horizontal));
        // Track element is 100×40 at origin (0,0).
        // inner_left = 10 (padding 2 + thumb_radius 8), inner_width = 100 - 20 = 80.
        // Pressing at x=50 maps to (50-10)/80 = 0.5 exactly.
        input.set_slider_config(
            id.clone(),
            SliderConfig::new(0.0, 1.0).step(0.01).track_extent(100.0),
        );
        input.set_slider_value(&id, 0.0);

        // Press at x=50: value snaps to (50-10)/80 = 0.5.
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(50.0, 20.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);
        assert!((input.slider_value(&id) - 0.5).abs() < 1e-5, "press snaps to 0.5");

        // Drag to x=90 (inner right edge, 10+80=90): value should be (90-10)/80 = 1.0.
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(90.0, 20.0),
            phase: InteractionPhase::Pressed,
            ..PointerState::default()
        }));
        input.update(&layout);
        assert!((input.slider_value(&id) - 1.0).abs() < 1e-5, "drag to right edge gives 1.0");
    }

    #[test]
    fn drag_total_reports_displacement_while_captured() {
        let id = ElementId::new("drag-bar");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::drag_bar(Axis::Horizontal));

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 20.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(35.0, 20.0),
            phase: InteractionPhase::Pressed,
            ..PointerState::default()
        }));
        input.update(&layout);

        let total = input.drag_total(&id);
        assert_eq!(total, Some(Vec2::new(25.0, 0.0)));
    }

    #[test]
    fn text_events_routed_to_focused_text_input() {
        let id = ElementId::new("field");
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::text_input());
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&LayoutTree::default());

        input.queue(InputEvent::Text("Hello".into()));
        input.update(&LayoutTree::default());

        assert_eq!(input.text_target().map(|id| id.hash), Some(id.hash));
        assert_eq!(input.text_this_frame(), "Hello");
    }

    #[test]
    fn text_events_ignored_when_focused_widget_is_not_text_input() {
        let id = ElementId::new("btn");
        let mut input = InputSimulator::default();
        input.set_widget_behavior(id.clone(), WidgetBehavior::interactive());
        input.queue(InputEvent::Focus(id.clone()));
        input.update(&LayoutTree::default());

        input.queue(InputEvent::Text("x".into()));
        input.update(&LayoutTree::default());

        assert!(input.text_target().is_none());
        assert_eq!(input.text_this_frame(), "");
    }

    #[test]
    fn escape_key_dismisses_scope_with_dismiss_on_cancel() {
        let scope_id = ElementId::new("scope");
        let root_id = ElementId::new("root");
        let mut input = InputSimulator::default();
        input.push_focus_scope(
            FocusScope::new(scope_id.clone(), root_id).dismiss_on_cancel(true),
        );

        input.queue(InputEvent::Key {
            name: "Escape".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&LayoutTree::default());

        assert_eq!(
            input.dismissed_focus_scopes().iter().map(|id| id.hash).collect::<Vec<_>>(),
            vec![scope_id.hash]
        );
    }

    // ── UiMode tests ──────────────────────────────────────────────────────────

    #[test]
    fn disabled_mode_discards_events_and_returns_no_hit() {
        let id = ElementId::new("button");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_mode(UiMode::Disabled);

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Key {
            name: "Enter".into(),
            pressed: true,
            repeat: false,
        });
        let hit = input.update(&layout);

        assert!(hit.is_none());
        // No state should have changed.
        assert!(!input.widget_state(&id).focused);
        assert!(!input.widget_state(&id).hovered);
        assert_eq!(input.last_event_result(), &UiEventResult::default());
        // Events must have been discarded (no pending events after update).
        assert_eq!(input.mode(), UiMode::Disabled);
    }

    #[test]
    fn disabled_mode_is_zero_cost_even_with_many_queued_events() {
        let mut input = InputSimulator::default();
        input.set_mode(UiMode::Disabled);

        for _ in 0..1000 {
            input.queue(InputEvent::Pointer(PointerState {
                position: Vec2::new(10.0, 10.0),
                phase: InteractionPhase::PressedThisFrame,
                ..PointerState::default()
            }));
        }
        // Should return immediately and discard all events.
        let hit = input.update(&LayoutTree::default());
        assert!(hit.is_none());
    }

    #[test]
    fn passthrough_mode_tracks_hover_but_reports_no_consumption() {
        let id = ElementId::new("button");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_mode(UiMode::Passthrough);

        // Press on the element.
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        let hit = input.update(&layout);

        // Hit test still runs — visual hover is tracked.
        assert!(hit.is_some());
        assert_eq!(hit.map(|h| h.id.hash), Some(id.hash));
        // But consumption flags must all be false so game layer acts on the event.
        assert_eq!(input.last_event_result(), &UiEventResult::default());
    }

    #[test]
    fn passthrough_mode_scroll_updates_offset_but_does_not_report_consumed() {
        let id = ElementId::new("scroll");
        let mut input = InputSimulator::default();
        input.set_mode(UiMode::Passthrough);
        input.set_scroll_config(
            id.clone(),
            ScrollConfig::new(Vec2::new(100.0, 80.0), Vec2::new(100.0, 240.0)),
        );

        input.queue(InputEvent::Scroll {
            target: Some(id.clone()),
            delta: Vec2::new(0.0, 40.0),
        });
        input.update(&LayoutTree::default());

        // Scroll state updates (UI visual position is maintained)…
        assert_eq!(input.scroll_offset(&id).y, 40.0);
        // …but no consumption is reported to the game layer.
        assert!(!input.last_event_result().scroll_consumed);
    }

    #[test]
    fn switching_from_disabled_to_active_resumes_normal_processing() {
        let id = ElementId::new("button");
        let element = test_element(id.clone());
        let layout = layout_for(&element);
        let mut input = InputSimulator::default();
        input.set_mode(UiMode::Disabled);

        // Events queued while disabled are discarded.
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);
        assert!(!input.widget_state(&id).focused);

        // Re-enable; new events are processed normally.
        input.set_mode(UiMode::Active);
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(10.0, 10.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        assert!(input.widget_state(&id).focused);
        assert!(input.last_event_result().pointer_consumed);
    }

    // ── Propagation path tests ────────────────────────────────────────────────

    fn three_node_layout() -> (ElementId, ElementId, ElementId, LayoutTree) {
        let root_id = ElementId::new("root");
        let child_id = ElementId::local("child", 0, &root_id);
        let grandchild_id = ElementId::local("gc", 0, &child_id);

        let root_rect = Rect::new(0.0, 0.0, 200.0, 200.0);
        let child_rect = Rect::new(10.0, 10.0, 80.0, 80.0);
        let gc_rect = Rect::new(20.0, 20.0, 30.0, 30.0);

        let layout = LayoutTree {
            nodes: vec![
                LayoutOutput {
                    id: root_id.clone(),
                    parent: 0,
                    rect: root_rect,
                    content_size: root_rect.size,
                    shape: UiShape::Rect,
                    layer: UiLayer::Content,
                    z_index: 0,
                    clip: false,
                    transparent_to_input: true,
                },
                LayoutOutput {
                    id: child_id.clone(),
                    parent: root_id.hash,
                    rect: child_rect,
                    content_size: child_rect.size,
                    shape: UiShape::Rect,
                    layer: UiLayer::Content,
                    z_index: 0,
                    clip: false,
                    transparent_to_input: true,
                },
                LayoutOutput {
                    id: grandchild_id.clone(),
                    parent: child_id.hash,
                    rect: gc_rect,
                    content_size: gc_rect.size,
                    shape: UiShape::Rect,
                    layer: UiLayer::Content,
                    z_index: 0,
                    clip: false,
                    transparent_to_input: false,
                },
            ],
        };
        (root_id, child_id, grandchild_id, layout)
    }

    #[test]
    fn propagation_path_is_root_to_target() {
        let (root_id, child_id, gc_id, layout) = three_node_layout();
        let input = InputSimulator::default();

        let path = input.propagation_path(&gc_id, &layout);

        assert_eq!(path.len(), 3);
        assert_eq!(path[0].hash, root_id.hash);
        assert_eq!(path[1].hash, child_id.hash);
        assert_eq!(path[2].hash, gc_id.hash);
    }

    #[test]
    fn bubble_path_is_target_to_root() {
        let (root_id, child_id, gc_id, layout) = three_node_layout();
        let input = InputSimulator::default();

        let path = input.bubble_path(&gc_id, &layout);

        assert_eq!(path.len(), 3);
        assert_eq!(path[0].hash, gc_id.hash);
        assert_eq!(path[1].hash, child_id.hash);
        assert_eq!(path[2].hash, root_id.hash);
    }

    #[test]
    fn propagation_path_for_unknown_element_is_empty() {
        let (_root_id, _child_id, _gc_id, layout) = three_node_layout();
        let input = InputSimulator::default();

        let path = input.propagation_path(&ElementId::new("not-in-tree"), &layout);

        assert!(path.is_empty());
    }

    #[test]
    fn bubble_listener_fires_when_descendant_activates_via_pointer() {
        let (_root_id, child_id, gc_id, layout) = three_node_layout();
        let mut input = InputSimulator::default();

        // Register the child container as a bubble listener.
        input.set_bubble_listener(child_id.clone());

        // Click the grandchild (the only non-transparent node).
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::ReleasedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        assert!(input.widget_state(&gc_id).activated);
        assert!(input.bubble_activated(&child_id));
    }

    #[test]
    fn bubble_listener_fires_for_keyboard_activation() {
        let (_root_id, child_id, gc_id, layout) = three_node_layout();
        let mut input = InputSimulator::default();

        input.set_bubble_listener(child_id.clone());
        // Focus the grandchild then activate via Enter.
        input.queue(InputEvent::Focus(gc_id.clone()));
        input.update(&layout);
        input.queue(InputEvent::Key {
            name: "Enter".into(),
            pressed: true,
            repeat: false,
        });
        input.update(&layout);

        assert!(input.widget_state(&gc_id).activated);
        assert!(input.bubble_activated(&child_id));
    }

    #[test]
    fn bubble_listener_clears_each_frame() {
        let (_root_id, child_id, gc_id, layout) = three_node_layout();
        let mut input = InputSimulator::default();
        input.set_bubble_listener(child_id.clone());

        // Activate via pointer press+release.
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::ReleasedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);
        assert!(input.bubble_activated(&child_id));

        // Next frame with no events: bubble_activated must be false.
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::Released,
            ..PointerState::default()
        }));
        input.update(&layout);
        assert!(!input.bubble_activated(&child_id));
        // Grandchild is not activated this frame either.
        assert!(!input.widget_state(&gc_id).activated);
    }

    #[test]
    fn non_listener_ancestor_does_not_appear_in_bubble_activations() {
        let (root_id, _child_id, gc_id, layout) = three_node_layout();
        let mut input = InputSimulator::default();
        // Only root is registered — child is not.
        input.set_bubble_listener(root_id.clone());

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::ReleasedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        assert!(input.widget_state(&gc_id).activated);
        assert!(input.bubble_activated(&root_id));
    }

    #[test]
    fn clear_bubble_listener_stops_future_notifications() {
        let (_root_id, child_id, gc_id, layout) = three_node_layout();
        let mut input = InputSimulator::default();
        input.set_bubble_listener(child_id.clone());
        input.clear_bubble_listener(&child_id);

        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::PressedThisFrame,
            ..PointerState::default()
        }));
        input.queue(InputEvent::Pointer(PointerState {
            position: Vec2::new(25.0, 25.0),
            phase: InteractionPhase::ReleasedThisFrame,
            ..PointerState::default()
        }));
        input.update(&layout);

        assert!(input.widget_state(&gc_id).activated);
        assert!(!input.bubble_activated(&child_id));
    }
}
