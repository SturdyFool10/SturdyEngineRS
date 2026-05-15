use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

use glam::Vec2;

use crate::{Axis, Easing, ElementId, UiLayer, UiShape};

use super::InputSimulator;

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
        Self {
            sim,
            palette,
            pending: RefCell::new(PendingRegistrations::default()),
        }
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
        self.pending
            .borrow_mut()
            .behaviors
            .insert(hash, (id, behavior));
    }

    pub fn register_slider(&self, id: ElementId, axis: crate::Axis, config: SliderConfig) {
        let hash = id.hash;
        let mut pending = self.pending.borrow_mut();
        pending
            .behaviors
            .insert(hash, (id.clone(), WidgetBehavior::slider(axis)));
        pending.slider_configs.insert(hash, (id, config));
    }

    /// Queue scroll-area behavior + config. Called by scroll container widgets automatically.
    pub fn register_scroll(&self, id: ElementId, config: ScrollConfig) {
        let hash = id.hash;
        let mut pending = self.pending.borrow_mut();
        pending
            .behaviors
            .insert(hash, (id.clone(), WidgetBehavior::scroll_area()));
        pending.scroll_configs.insert(hash, (id, config));
    }

    /// Queue text-input behavior. Called by text input widgets automatically.
    pub fn register_text_input(&self, id: ElementId) {
        let hash = id.hash;
        self.pending
            .borrow_mut()
            .behaviors
            .insert(hash, (id, WidgetBehavior::text_input()));
    }

    /// Queue drag-bar behavior. Called by drag bar widgets automatically.
    pub fn register_drag_bar(&self, id: ElementId, axis: crate::Axis) {
        let hash = id.hash;
        self.pending
            .borrow_mut()
            .behaviors
            .insert(hash, (id, WidgetBehavior::drag_bar(axis)));
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
        self.pending
            .borrow_mut()
            .toggle_anim_updates
            .insert(id.hash, (id.clone(), new_linear));
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
        Self {
            phase,
            stopped: false,
            default_prevented: false,
        }
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
    /// Desired visual length of the draggable track in layout pixels.
    ///
    /// Pointer input is mapped through the final laid-out rect, not this
    /// configured value, so parent alignment and responsive layout stay in sync
    /// with hit testing.
    pub track_extent: f32,
    /// Radius of the draggable thumb in layout pixels. The pointer travel range
    /// is inset by this amount on both sides so the thumb stays inside the
    /// visible track.
    pub thumb_radius: f32,
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
            thumb_radius: 8.0,
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

    pub fn thumb_radius(mut self, thumb_radius: f32) -> Self {
        self.thumb_radius = thumb_radius.max(0.0);
        self
    }

    pub(super) fn clamp(&self, value: f32) -> f32 {
        value.clamp(self.min, self.max)
    }

    pub(super) fn range(&self) -> f32 {
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
