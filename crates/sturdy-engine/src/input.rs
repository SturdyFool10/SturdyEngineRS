use std::{
    cell::Cell,
    collections::{HashMap, HashSet},
};

mod actions;
mod capture;
mod display;
mod gamepad;
mod keybind;
mod keyboard;
mod winit_bridge;

pub use actions::{
    ActionAxisDirection, ActionBinding, ActionBindingRegistry, ActionMap, BindingChange,
};
pub use capture::KeybindCapture;
pub use gamepad::{GamepadAxis, GamepadAxisInput, GamepadButton, GamepadButtonInput, GamepadId};
pub use keybind::{KeyModifier, KeyToken, Keybind};
pub use keyboard::{KeyInput, KeyInputState, KeyModifiers};
#[cfg(feature = "app-shell")]
pub(crate) use winit_bridge::key_modifiers_from_winit;

/// A point-in-time input snapshot sampled as late as possible before GPU submission.
///
/// Obtained from [`InputHub::sample_late`]. Use the fields of this snapshot
/// for camera orientation, view matrix construction, and motion vectors so that
/// rendering reflects the most recent input rather than the game-logic snapshot.
#[derive(Clone, Debug, Default)]
pub struct LateSample {
    /// Raw device-event mouse delta accumulated since the last `update()`.
    ///
    /// Sourced from `DeviceEvent::MouseMotion` — reliable even when the cursor
    /// is grabbed or hidden. Prefer this over `mouse_delta` for first-person cameras.
    pub raw_mouse_delta: glam::Vec2,
    /// Cursor-position–derived mouse delta accumulated since the last `update()`.
    ///
    /// Zero when the cursor is locked; use `raw_mouse_delta` in that case.
    pub mouse_delta: glam::Vec2,
    /// Current cursor position in top-left/Y-down logical pixels.
    pub cursor: glam::Vec2,
    /// Latest eye-tracking gaze direction, or `None` if unavailable.
    pub gaze_direction: Option<glam::Vec2>,
}

fn clay_modifiers(modifiers: KeyModifiers) -> clay_ui::ModifierKeys {
    clay_ui::ModifierKeys {
        ctrl: modifiers.ctrl,
        alt: modifiers.alt,
        shift: modifiers.shift,
        super_: modifiers.super_,
    }
}

// ── InputHub ──────────────────────────────────────────────────────────────────

/// Drop-in input coordinator that bridges raw `EngineApp` callbacks to both
/// the UI [`clay_ui::InputSimulator`] and the game-action [`ActionMap`].
///
/// # Usage
///
/// Add `hub: InputHub` to your app struct, implement `EngineApp::input_hub`
/// to return `Some(&mut self.hub)`, and the engine shell routes all keyboard
/// and pointer events automatically. In `render`, call `hub.update(&layout)`
/// once after building the layout tree, then query widget and action states.
///
/// ```ignore
/// struct MyApp {
///     hub: InputHub,
/// }
///
/// impl EngineApp for MyApp {
///     fn input_hub(&mut self) -> Option<&mut InputHub> { Some(&mut self.hub) }
///
///     fn render(&mut self, frame: &mut ShellFrame, image: &SurfaceImage) -> Result<()> {
///         let layout = build_layout(...);
///         self.hub.update(&layout);
///         if self.hub.actions().just_pressed("Jump") { /* ... */ }
///         if self.hub.widget_state(&button_id).activated  { /* ... */ }
///         Ok(())
///     }
/// }
/// ```
///
/// For tests or replay, queue events directly:
/// ```ignore
/// hub.queue(InputEvent::Pointer(...));      // UI events
/// hub.simulate_key(&key_input);            // key → both UI and ActionMap
/// hub.update(&layout);
/// ```
pub struct InputHub {
    simulator: clay_ui::InputSimulator,
    actions: ActionMap,
    cursor: clay_ui::WindowLogicalPx,
    cursor_initialized: bool,
    /// Clay input events queued this frame, mirrored from `simulator` so they
    /// can be forwarded to clay `UiContext` trees. Drained by
    /// [`drain_clay_events`](Self::drain_clay_events).
    pending_clay_events: Vec<clay_ui::InputEvent>,
    /// Cursor-position–derived delta (UI-friendly, zero when cursor is locked).
    mouse_delta: glam::Vec2,
    pending_mouse_delta: glam::Vec2,
    /// Raw device-event delta (reliable even when cursor is grabbed/hidden).
    raw_mouse_delta: glam::Vec2,
    pending_raw_mouse_delta: glam::Vec2,
    /// Latest eye-tracking gaze direction in normalized display coordinates.
    gaze_direction: Option<glam::Vec2>,
    held_keys: HashSet<KeyToken>,
    key_just_pressed: HashSet<KeyToken>,
    key_just_released: HashSet<KeyToken>,
    pending_key_pressed: HashSet<KeyToken>,
    pending_key_released: HashSet<KeyToken>,
    held_mouse_buttons: HashSet<u8>,
    mouse_button_just_pressed: HashSet<u8>,
    mouse_button_just_released: HashSet<u8>,
    pending_mouse_button_pressed: HashSet<u8>,
    pending_mouse_button_released: HashSet<u8>,
    held_gamepad_buttons: HashSet<(GamepadId, GamepadButton)>,
    gamepad_button_just_pressed: HashSet<(GamepadId, GamepadButton)>,
    gamepad_button_just_released: HashSet<(GamepadId, GamepadButton)>,
    pending_gamepad_button_pressed: HashSet<(GamepadId, GamepadButton)>,
    pending_gamepad_button_released: HashSet<(GamepadId, GamepadButton)>,
    gamepad_axes: HashMap<(GamepadId, GamepadAxis), f32>,
    primary_held: bool,
    /// `KeyInput` events received since the last `update()`, drained into
    /// `ActionMap` after the simulator has run (so UI priority is respected).
    pending_key_inputs: Vec<KeyInput>,
    /// Desired pointer-lock state set by app code. Interior-mutable so
    /// `request_pointer_lock` / `release_pointer_lock` work on `&self`.
    pointer_lock_requested: Cell<bool>,
    /// Actual pointer-lock state, written by the shell after applying the OS grab.
    pointer_locked: bool,
}

impl Default for InputHub {
    fn default() -> Self {
        Self::new()
    }
}

impl InputHub {
    pub fn new() -> Self {
        Self {
            simulator: clay_ui::InputSimulator::default(),
            actions: ActionMap::new(),
            cursor: clay_ui::WindowLogicalPx::ZERO,
            cursor_initialized: false,
            pending_clay_events: Vec::new(),
            mouse_delta: glam::Vec2::ZERO,
            pending_mouse_delta: glam::Vec2::ZERO,
            raw_mouse_delta: glam::Vec2::ZERO,
            pending_raw_mouse_delta: glam::Vec2::ZERO,
            gaze_direction: None,
            held_keys: HashSet::new(),
            key_just_pressed: HashSet::new(),
            key_just_released: HashSet::new(),
            pending_key_pressed: HashSet::new(),
            pending_key_released: HashSet::new(),
            held_mouse_buttons: HashSet::new(),
            mouse_button_just_pressed: HashSet::new(),
            mouse_button_just_released: HashSet::new(),
            pending_mouse_button_pressed: HashSet::new(),
            pending_mouse_button_released: HashSet::new(),
            held_gamepad_buttons: HashSet::new(),
            gamepad_button_just_pressed: HashSet::new(),
            gamepad_button_just_released: HashSet::new(),
            pending_gamepad_button_pressed: HashSet::new(),
            pending_gamepad_button_released: HashSet::new(),
            gamepad_axes: HashMap::new(),
            primary_held: false,
            pending_key_inputs: Vec::new(),
            pointer_lock_requested: Cell::new(false),
            pointer_locked: false,
        }
    }

    // ── EngineApp bridge ──────────────────────────────────────────────────────

    /// Call from `EngineApp::pointer_moved`.
    pub fn on_pointer_moved(&mut self, pos: clay_ui::WindowLogicalPx) {
        use clay_ui::{InputEvent, InteractionPhase, PointerButton, PointerState};
        if self.cursor_initialized {
            self.pending_mouse_delta += pos.to_vec2() - self.cursor.to_vec2();
        } else {
            self.cursor_initialized = true;
        }
        self.cursor = pos;
        let phase = if self.primary_held {
            InteractionPhase::Pressed
        } else {
            InteractionPhase::Released
        };
        let event = InputEvent::Pointer(PointerState {
            position: pos.to_vec2(),
            button: PointerButton::Primary,
            phase,
        });
        self.pending_clay_events.push(event.clone());
        self.simulator.queue(event);
    }

    /// Call from `EngineApp::pointer_button`.
    ///
    /// `button` follows the convention 0 = primary, 1 = secondary, 2 = middle.
    pub fn on_pointer_button(&mut self, pos: clay_ui::WindowLogicalPx, button: u8, pressed: bool) {
        use clay_ui::{InputEvent, InteractionPhase, PointerButton, PointerState};
        self.cursor = pos;
        self.cursor_initialized = true;
        if button == 0 {
            self.primary_held = pressed;
        }
        if pressed {
            if self.held_mouse_buttons.insert(button) {
                self.pending_mouse_button_pressed.insert(button);
            }
            self.pending_mouse_button_released.remove(&button);
        } else {
            if self.held_mouse_buttons.remove(&button) {
                self.pending_mouse_button_released.insert(button);
            }
            self.pending_mouse_button_pressed.remove(&button);
        }
        let btn = match button {
            0 => PointerButton::Primary,
            1 => PointerButton::Secondary,
            2 => PointerButton::Middle,
            n => PointerButton::Extra(n),
        };
        let phase = if pressed {
            InteractionPhase::PressedThisFrame
        } else {
            InteractionPhase::ReleasedThisFrame
        };
        let event = InputEvent::Pointer(PointerState {
            position: pos.to_vec2(),
            button: btn,
            phase,
        });
        self.pending_clay_events.push(event.clone());
        self.simulator.queue(event);
    }

    /// Call with raw `DeviceEvent::MouseMotion` deltas for first-person camera support.
    ///
    /// This is called by the engine shell automatically. Unlike `on_pointer_moved`,
    /// raw motion fires even when the cursor is grabbed or hidden, making it the
    /// correct source for first-person look input. Read via [`raw_mouse_delta`](Self::raw_mouse_delta).
    pub fn on_raw_mouse_motion(&mut self, delta: glam::Vec2) {
        self.pending_raw_mouse_delta += delta;
    }

    /// Call from an eye-tracking backend when a gaze sample is available.
    ///
    /// The direction is expected in normalized display coordinates where
    /// `(-1, -1)` is the lower-left visible extent, `(0, 0)` is straight ahead,
    /// and `(1, 1)` is the upper-right visible extent. Passing `None` clears the
    /// current sample when hardware is absent, tracking is lost, or permission is
    /// revoked. Non-finite vectors are ignored so a bad device sample cannot
    /// poison foveated-rendering state.
    pub fn on_gaze_direction(&mut self, direction: Option<glam::Vec2>) {
        match direction {
            Some(direction) if direction.is_finite() => self.gaze_direction = Some(direction),
            Some(_) => {}
            None => self.gaze_direction = None,
        }
    }

    /// Call from `EngineApp::pointer_scroll`.
    pub fn on_pointer_scroll(&mut self, delta_x: f32, delta_y: f32) {
        use clay_ui::InputEvent;
        let event = InputEvent::Scroll {
            target: None,
            delta: glam::Vec2::new(delta_x, delta_y),
        };
        self.pending_clay_events.push(event.clone());
        self.simulator.queue(event);
    }

    /// Call from `EngineApp::key_input`.
    ///
    /// Routes the key to the UI simulator and buffers it for action dispatch.
    pub fn on_key_input(&mut self, input: &KeyInput) {
        use clay_ui::InputEvent;

        let modifiers = clay_modifiers(input.modifiers);
        if !input.repeat {
            match input.state {
                KeyInputState::Pressed => {
                    if self.held_keys.insert(input.key.clone()) {
                        self.pending_key_pressed.insert(input.key.clone());
                    }
                    self.pending_key_released.remove(&input.key);
                }
                KeyInputState::Released => {
                    if self.held_keys.remove(&input.key) {
                        self.pending_key_released.insert(input.key.clone());
                    }
                    self.pending_key_pressed.remove(&input.key);
                }
            }
        }

        // Route key name to simulator.
        if let KeyToken::Key(name) = &input.key {
            let event = InputEvent::KeyWithModifiers {
                name: name.clone(),
                pressed: input.state == KeyInputState::Pressed,
                repeat: input.repeat,
                modifiers,
            };
            self.pending_clay_events.push(event.clone());
            self.simulator.queue(event);
        }

        // Route text on every press. Text callbacks should see repeating text
        // exactly when the platform's text-input path produced it.
        if input.state == KeyInputState::Pressed {
            if let Some(text) = &input.text {
                let event = InputEvent::TextWithModifiers {
                    text: text.clone(),
                    modifiers,
                };
                self.pending_clay_events.push(event.clone());
                self.simulator.queue(event);
            }
        }

        // Buffer for deferred action dispatch after simulator.update().
        self.pending_key_inputs.push(input.clone());
    }

    /// Call from a gamepad backend when a button is pressed or released.
    pub fn on_gamepad_button(&mut self, gamepad: GamepadId, button: GamepadButton, pressed: bool) {
        let key = (gamepad, button);
        if pressed {
            if self.held_gamepad_buttons.insert(key) {
                self.pending_gamepad_button_pressed.insert(key);
            }
            self.pending_gamepad_button_released.remove(&key);
        } else {
            if self.held_gamepad_buttons.remove(&key) {
                self.pending_gamepad_button_released.insert(key);
            }
            self.pending_gamepad_button_pressed.remove(&key);
        }
    }

    /// Call from a gamepad backend when an analog axis changes.
    pub fn on_gamepad_axis(&mut self, gamepad: GamepadId, axis: GamepadAxis, value: f32) {
        self.gamepad_axes
            .insert((gamepad, axis), value.clamp(-1.0, 1.0));
    }

    /// Clear all cached state for a disconnected gamepad.
    pub fn clear_gamepad(&mut self, gamepad: GamepadId) {
        self.held_gamepad_buttons.retain(|(id, _)| *id != gamepad);
        self.gamepad_button_just_pressed
            .retain(|(id, _)| *id != gamepad);
        self.gamepad_button_just_released
            .retain(|(id, _)| *id != gamepad);
        self.pending_gamepad_button_pressed
            .retain(|(id, _)| *id != gamepad);
        self.pending_gamepad_button_released
            .retain(|(id, _)| *id != gamepad);
        self.gamepad_axes.retain(|(id, _), _| *id != gamepad);
    }

    /// Feed a gamepad button input through the same path as
    /// [`InputHub::on_gamepad_button`]. Use in tests or replay.
    pub fn simulate_gamepad_button(&mut self, input: GamepadButtonInput) {
        self.on_gamepad_button(
            input.gamepad,
            input.button,
            input.state == KeyInputState::Pressed,
        );
    }

    /// Feed a gamepad axis input through the same path as
    /// [`InputHub::on_gamepad_axis`]. Use in tests or replay.
    pub fn simulate_gamepad_axis(&mut self, input: GamepadAxisInput) {
        self.on_gamepad_axis(input.gamepad, input.axis, input.value);
    }

    // ── Simulation / testing ──────────────────────────────────────────────────

    /// Queue a low-level UI event directly into the simulator.
    ///
    /// Use this in tests or replay scenarios instead of the `on_*` methods.
    pub fn queue(&mut self, event: clay_ui::InputEvent) {
        self.pending_clay_events.push(event.clone());
        self.simulator.queue(event);
    }

    /// Take all clay input events queued since the last call to this method.
    ///
    /// Forward these to `clay_ui::UiContext::queue_input` so registered clay
    /// trees receive the events when `build_frame` runs. Typically called
    /// through [`route_to_clay`](Self::route_to_clay) or
    /// [`AppRuntimeFrame::route_input_to_ui`](crate::AppRuntimeFrame::route_input_to_ui).
    pub fn drain_clay_events(&mut self) -> Vec<clay_ui::InputEvent> {
        std::mem::take(&mut self.pending_clay_events)
    }

    /// Forward all pending clay input events to every registered tree in `ctx`.
    ///
    /// Call this once per frame before building the clay UI frame, typically
    /// right after the game shell delivers input events:
    ///
    /// ```ignore
    /// ctx.input.route_to_clay(frame.ui_context().clay());
    /// let output = frame.ui_context().build_frame();
    /// ```
    pub fn route_to_clay(&mut self, ctx: &mut clay_ui::UiContext) {
        let events = self.drain_clay_events();
        if events.is_empty() {
            return;
        }
        // Collect tree names first to avoid borrow conflict.
        let names: Vec<String> = ctx.trees().map(|(n, _)| n.to_owned()).collect();
        // Forward to all registered trees — every tree gets every event and
        // processes only the ones targeting elements within its layout.
        for name in &names {
            for event in &events {
                ctx.queue_input(name, event.clone());
            }
        }
    }

    /// Feed a `KeyInput` through the same path as `on_key_input` — routes to
    /// both the UI simulator and the action map buffer. Use in tests.
    pub fn simulate_key(&mut self, input: &KeyInput) {
        self.on_key_input(input);
    }

    // ── Frame update ──────────────────────────────────────────────────────────

    /// Process all queued events for this frame.
    ///
    /// Call once per frame after the layout tree is built. Returns the
    /// topmost interactive element under the pointer (same as
    /// [`InputSimulator::update`]).
    ///
    /// Internally:
    /// 1. Clears `ActionMap` per-frame state (`just_pressed` / `just_released`).
    /// 2. Runs `InputSimulator::update` — resolves UI events against the layout.
    /// 3. Dispatches buffered key inputs to `ActionMap`, using per-key UI
    ///    consumption data so unrelated game actions are not blocked.
    pub fn update(&mut self, tree: &clay_ui::LayoutTree) -> Option<clay_ui::Hit> {
        self.publish_polling_frame();
        self.actions.end_frame();
        let hit = self.simulator.update(tree);
        let pending = std::mem::take(&mut self.pending_key_inputs);
        for ki in &pending {
            let key_name = match &ki.key {
                KeyToken::Key(name) => name.as_str(),
                KeyToken::Modifier(_) => continue,
            };
            let this_key_consumed = self.simulator.key_input_consumed(key_name);
            self.actions.process(ki, this_key_consumed);
        }
        self.actions.process_polling(
            &self.held_mouse_buttons,
            &self.held_gamepad_buttons,
            &self.gamepad_axes,
        );
        hit
    }

    /// Take a late input snapshot immediately before GPU submission.
    ///
    /// Returns the raw mouse delta and cursor position accumulated since the
    /// last [`update`](Self::update) call. Use this snapshot for camera
    /// orientation, view matrix construction, and motion vector generation to
    /// minimise input-to-display latency — the snapshot captures motion events
    /// that arrived after the game-logic tick without consuming them.
    ///
    /// The committed per-frame state (from `update`) is unchanged; calling this
    /// method is side-effect-free.
    pub fn sample_late(&self) -> LateSample {
        LateSample {
            raw_mouse_delta: self.pending_raw_mouse_delta,
            mouse_delta: self.pending_mouse_delta,
            cursor: self.cursor.to_vec2(),
            gaze_direction: self.gaze_direction,
        }
    }

    fn publish_polling_frame(&mut self) {
        self.mouse_delta = self.pending_mouse_delta;
        self.pending_mouse_delta = glam::Vec2::ZERO;
        self.raw_mouse_delta = self.pending_raw_mouse_delta;
        self.pending_raw_mouse_delta = glam::Vec2::ZERO;
        self.key_just_pressed = std::mem::take(&mut self.pending_key_pressed);
        self.key_just_released = std::mem::take(&mut self.pending_key_released);
        self.mouse_button_just_pressed = std::mem::take(&mut self.pending_mouse_button_pressed);
        self.mouse_button_just_released = std::mem::take(&mut self.pending_mouse_button_released);
        self.gamepad_button_just_pressed = std::mem::take(&mut self.pending_gamepad_button_pressed);
        self.gamepad_button_just_released =
            std::mem::take(&mut self.pending_gamepad_button_released);
    }

    // ── Accessors ─────────────────────────────────────────────────────────────

    /// Access the underlying UI input simulator for advanced configuration
    /// (registering scroll configs, widget behaviors, focus scopes, etc.).
    pub fn simulator(&self) -> &clay_ui::InputSimulator {
        &self.simulator
    }

    pub fn simulator_mut(&mut self) -> &mut clay_ui::InputSimulator {
        &mut self.simulator
    }

    /// Access the action map to bind and query game actions.
    pub fn actions(&self) -> &ActionMap {
        &self.actions
    }

    pub fn actions_mut(&mut self) -> &mut ActionMap {
        &mut self.actions
    }

    /// Current cursor position in top-left/Y-down `WindowLogicalPx`.
    pub fn cursor_pos(&self) -> clay_ui::WindowLogicalPx {
        self.cursor
    }

    /// Current mouse position in top-left/Y-down `WindowLogicalPx`.
    pub fn mouse_position(&self) -> clay_ui::WindowLogicalPx {
        self.cursor
    }

    /// Mouse movement accumulated since the previous [`InputHub::update`].
    ///
    /// Derived from cursor-position events. Returns `Vec2::ZERO` when the cursor
    /// is locked (use [`raw_mouse_delta`](Self::raw_mouse_delta) instead for
    /// first-person cameras).
    pub fn mouse_delta(&self) -> glam::Vec2 {
        self.mouse_delta
    }

    /// Raw device-event mouse delta accumulated since the previous frame.
    ///
    /// Sourced from `DeviceEvent::MouseMotion`, which fires even when the cursor
    /// is grabbed or hidden. Use this for first-person camera look input.
    /// Returns `Vec2::ZERO` when no raw motion events were received this frame.
    pub fn raw_mouse_delta(&self) -> glam::Vec2 {
        self.raw_mouse_delta
    }

    /// Latest eye-tracking gaze direction, if hardware has provided one.
    ///
    /// Returns `None` on machines without eye tracking, before the first sample,
    /// after tracking is lost, or after [`on_gaze_direction`](Self::on_gaze_direction)
    /// is called with `None`.
    pub fn gaze_direction(&self) -> Option<glam::Vec2> {
        self.gaze_direction
    }

    /// Request that the cursor be grabbed and hidden for first-person mouse look.
    ///
    /// The shell applies the OS cursor grab on the next frame. Takes `&self` so
    /// it can be called from contexts where only a shared reference is available
    /// (e.g., [`GameContext::input`](crate::GameContext::input)).
    pub fn request_pointer_lock(&self) {
        self.pointer_lock_requested.set(true);
    }

    /// Release the cursor grab, making the cursor visible and free again.
    ///
    /// Takes `&self` for the same reason as [`request_pointer_lock`](Self::request_pointer_lock).
    pub fn release_pointer_lock(&self) {
        self.pointer_lock_requested.set(false);
    }

    /// `true` if the cursor is currently grabbed and hidden by the OS.
    ///
    /// This reflects the actual OS state, updated by the shell after each lock
    /// attempt. May differ briefly from the requested state if the grab failed.
    pub fn is_pointer_locked(&self) -> bool {
        self.pointer_locked
    }

    /// `true` if the app has requested pointer lock but the shell has not applied it yet,
    /// or if the lock state differs from the requested state.
    pub fn is_pointer_lock_pending(&self) -> bool {
        self.pointer_lock_requested.get() != self.pointer_locked
    }

    /// Called by the shell after attempting to apply or release the OS cursor grab.
    pub(crate) fn set_pointer_locked(&mut self, locked: bool) {
        self.pointer_locked = locked;
    }

    /// Called by the shell to read the desired lock state without consuming it.
    pub(crate) fn pointer_lock_desired(&self) -> bool {
        self.pointer_lock_requested.get()
    }

    /// `true` while the raw key is held down, regardless of UI consumption.
    pub fn is_key_pressed(&self, key: impl Into<KeyToken>) -> bool {
        self.held_keys.contains(&key.into())
    }

    /// `true` on the first frame the raw key was pressed, regardless of UI consumption.
    pub fn is_key_just_pressed(&self, key: impl Into<KeyToken>) -> bool {
        self.key_just_pressed.contains(&key.into())
    }

    /// `true` on the frame the raw key was released, regardless of UI consumption.
    pub fn is_key_just_released(&self, key: impl Into<KeyToken>) -> bool {
        self.key_just_released.contains(&key.into())
    }

    /// `true` while the raw mouse button is held down.
    ///
    /// Buttons follow the shell convention: 0 = primary, 1 = secondary, 2 = middle.
    pub fn is_mouse_button_pressed(&self, button: u8) -> bool {
        self.held_mouse_buttons.contains(&button)
    }

    /// `true` on the first frame the raw mouse button was pressed.
    pub fn is_mouse_button_just_pressed(&self, button: u8) -> bool {
        self.mouse_button_just_pressed.contains(&button)
    }

    /// `true` on the frame the raw mouse button was released.
    pub fn is_mouse_button_just_released(&self, button: u8) -> bool {
        self.mouse_button_just_released.contains(&button)
    }

    /// `true` while the gamepad button is held down.
    pub fn is_gamepad_button_pressed(&self, gamepad: GamepadId, button: GamepadButton) -> bool {
        self.held_gamepad_buttons.contains(&(gamepad, button))
    }

    /// `true` on the first frame the gamepad button was pressed.
    pub fn is_gamepad_button_just_pressed(
        &self,
        gamepad: GamepadId,
        button: GamepadButton,
    ) -> bool {
        self.gamepad_button_just_pressed
            .contains(&(gamepad, button))
    }

    /// `true` on the frame the gamepad button was released.
    pub fn is_gamepad_button_just_released(
        &self,
        gamepad: GamepadId,
        button: GamepadButton,
    ) -> bool {
        self.gamepad_button_just_released
            .contains(&(gamepad, button))
    }

    /// Current value for a gamepad axis in the normalized `[-1, 1]` range.
    ///
    /// Returns `0.0` if the axis has not been seen yet.
    pub fn gamepad_axis(&self, gamepad: GamepadId, axis: GamepadAxis) -> f32 {
        self.gamepad_axes
            .get(&(gamepad, axis))
            .copied()
            .unwrap_or(0.0)
    }

    // ── Simulator convenience forwards ────────────────────────────────────────

    pub fn widget_state(&self, id: &clay_ui::ElementId) -> clay_ui::WidgetState {
        self.simulator.widget_state(id)
    }

    pub fn scroll_offset(&self, id: &clay_ui::ElementId) -> glam::Vec2 {
        self.simulator.scroll_offset(id)
    }

    pub fn scroll_layout_offset(&self, id: &clay_ui::ElementId) -> glam::Vec2 {
        self.simulator.scroll_layout_offset(id)
    }

    pub fn slider_value(&self, id: &clay_ui::ElementId) -> f32 {
        self.simulator.slider_value(id)
    }

    pub fn last_event_result(&self) -> &clay_ui::UiEventResult {
        self.simulator.last_event_result()
    }

    pub fn bubble_activated(&self, id: &clay_ui::ElementId) -> bool {
        self.simulator.bubble_activated(id)
    }

    pub fn hovered(&self) -> Option<&clay_ui::ElementId> {
        self.simulator.hovered()
    }

    pub fn focused(&self) -> Option<&clay_ui::ElementId> {
        self.simulator.focused()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn press(key: &str) -> KeyInput {
        KeyInput {
            key: KeyToken::Key(key.into()),
            state: KeyInputState::Pressed,
            modifiers: KeyModifiers::default(),
            repeat: false,
            text: None,
        }
    }

    fn release(key: &str) -> KeyInput {
        KeyInput {
            key: KeyToken::Key(key.into()),
            state: KeyInputState::Released,
            modifiers: KeyModifiers::default(),
            repeat: false,
            text: None,
        }
    }

    fn press_with_ctrl(key: &str) -> KeyInput {
        KeyInput {
            key: KeyToken::Key(key.into()),
            state: KeyInputState::Pressed,
            modifiers: KeyModifiers {
                ctrl: true,
                ..KeyModifiers::default()
            },
            repeat: false,
            text: None,
        }
    }

    #[test]
    fn just_pressed_is_true_on_first_press_then_clears_after_end_frame() {
        let mut map = ActionMap::new();
        map.bind("Jump", "Space".parse().unwrap());

        map.process(&press("Space"), false);
        assert!(map.just_pressed("Jump"));
        assert!(map.is_held("Jump"));

        map.end_frame();
        assert!(!map.just_pressed("Jump"));
        assert!(map.is_held("Jump"));
    }

    #[test]
    fn just_released_is_true_on_key_up_then_clears_after_end_frame() {
        let mut map = ActionMap::new();
        map.bind("Jump", "Space".parse().unwrap());

        map.process(&press("Space"), false);
        map.end_frame();
        map.process(&release("Space"), false);

        assert!(map.just_released("Jump"));
        assert!(!map.is_held("Jump"));

        map.end_frame();
        assert!(!map.just_released("Jump"));
    }

    #[test]
    fn ui_consumed_suppresses_action_dispatch() {
        let mut map = ActionMap::new();
        map.bind("Confirm", "Enter".parse().unwrap());

        map.process(&press("Enter"), true); // ui_consumed = true
        assert!(!map.just_pressed("Confirm"));
        assert!(!map.is_held("Confirm"));
    }

    #[test]
    fn repeat_events_do_not_re_trigger_just_pressed() {
        let mut map = ActionMap::new();
        map.bind("Fire", "Space".parse().unwrap());

        map.process(&press("Space"), false);
        map.end_frame();

        let repeat_event = KeyInput {
            key: KeyToken::Key("Space".into()),
            state: KeyInputState::Pressed,
            modifiers: KeyModifiers::default(),
            repeat: true,
            text: None,
        };
        map.process(&repeat_event, false);
        assert!(!map.just_pressed("Fire"));
        assert!(map.is_held("Fire"));
    }

    #[test]
    fn multi_binding_either_key_triggers_action() {
        let mut map = ActionMap::new();
        map.bind("MoveForward", "KeyW".parse().unwrap());
        map.bind("MoveForward", "ArrowUp".parse().unwrap());

        map.process(&press("ArrowUp"), false);
        assert!(map.just_pressed("MoveForward"));
        map.end_frame();
        map.process(&release("ArrowUp"), false);
        map.end_frame();

        map.process(&press("KeyW"), false);
        assert!(map.just_pressed("MoveForward"));
    }

    #[test]
    fn modifier_binding_requires_modifier_held() {
        let mut map = ActionMap::new();
        map.bind("Undo", "Ctrl+KeyZ".parse().unwrap());

        // Press without Ctrl — should not fire.
        map.process(&press("KeyZ"), false);
        assert!(!map.just_pressed("Undo"));

        // Press with Ctrl — should fire.
        map.process(&press_with_ctrl("KeyZ"), false);
        assert!(map.just_pressed("Undo"));
    }

    #[test]
    fn lenient_matching_allows_extra_modifiers() {
        let mut map = ActionMap::new();
        // Binding has no modifiers.
        map.bind("Jump", "Space".parse().unwrap());

        // Press Space while Shift is held — should still fire.
        let event = KeyInput {
            key: KeyToken::Key("Space".into()),
            state: KeyInputState::Pressed,
            modifiers: KeyModifiers {
                shift: true,
                ..KeyModifiers::default()
            },
            repeat: false,
            text: None,
        };
        map.process(&event, false);
        assert!(map.just_pressed("Jump"));
    }

    #[test]
    fn save_and_load_config_round_trips() {
        let mut map = ActionMap::new();
        map.bind("Jump", "Space".parse().unwrap());
        map.bind("Jump", "KeyW".parse().unwrap());
        map.bind("Fire", "Ctrl+Space".parse().unwrap());

        let config = map.save_config();
        let mut map2 = ActionMap::new();
        map2.load_config(&config);

        // Both Jump bindings survive the round-trip.
        assert_eq!(map2.bindings_for("Jump").len(), 2);
        map2.process(&press("Space"), false);
        assert!(map2.just_pressed("Jump"));
        map2.end_frame();
        map2.process(&release("Space"), false); // release first before pressing second
        map2.end_frame();
        map2.process(&press("KeyW"), false);
        assert!(map2.just_pressed("Jump"));
    }

    #[test]
    fn clear_bindings_removes_action() {
        let mut map = ActionMap::new();
        map.bind("Jump", "Space".parse().unwrap());
        map.clear_bindings("Jump");

        map.process(&press("Space"), false);
        assert!(!map.just_pressed("Jump"));
    }

    #[test]
    fn load_config_replaces_existing_bindings() {
        let mut map = ActionMap::new();
        map.bind("Jump", "Space".parse().unwrap());

        let mut cfg = BTreeMap::new();
        cfg.insert("Jump".to_string(), "KeyW".to_string());
        map.load_config(&cfg);

        // Old "Space" binding is gone.
        map.process(&press("Space"), false);
        assert!(!map.just_pressed("Jump"));

        // New "KeyW" binding works.
        map.process(&press("KeyW"), false);
        assert!(map.just_pressed("Jump"));
    }

    #[test]
    fn action_map_config_saves_only_keybinds() {
        let mut map = ActionMap::new();
        map.bind("Jump", "Space".parse().unwrap());
        map.bind_input("Jump", ActionBinding::mouse_button(0));

        let config = map.save_config();

        assert_eq!(config.get("Jump").map(String::as_str), Some("Space"));
        assert_eq!(map.bindings_for("Jump").len(), 2);
        assert_eq!(map.keybinds_for("Jump").len(), 1);
    }

    #[test]
    fn input_hub_routes_mouse_button_to_action_map() {
        let mut hub = InputHub::new();
        hub.actions_mut()
            .bind_input("Fire", ActionBinding::mouse_button(0));

        hub.on_pointer_button(clay_ui::WindowLogicalPx::ZERO, 0, true);
        hub.update(&clay_ui::LayoutTree::default());
        assert!(hub.actions().is_held("Fire"));
        assert!(hub.actions().just_pressed("Fire"));
        assert_eq!(hub.actions().value("Fire"), 1.0);

        hub.update(&clay_ui::LayoutTree::default());
        assert!(hub.actions().is_held("Fire"));
        assert!(!hub.actions().just_pressed("Fire"));

        hub.on_pointer_button(clay_ui::WindowLogicalPx::ZERO, 0, false);
        hub.update(&clay_ui::LayoutTree::default());
        assert!(!hub.actions().is_held("Fire"));
        assert!(hub.actions().just_released("Fire"));
        assert_eq!(hub.actions().value("Fire"), 0.0);
    }

    #[test]
    fn input_hub_routes_gamepad_button_to_action_map() {
        let mut hub = InputHub::new();
        let gamepad = GamepadId(7);
        hub.actions_mut().bind_input(
            "Jump",
            ActionBinding::gamepad_button_for(gamepad, GamepadButton::South),
        );

        hub.on_gamepad_button(gamepad, GamepadButton::South, true);
        hub.update(&clay_ui::LayoutTree::default());
        assert!(hub.actions().is_held("Jump"));
        assert!(hub.actions().just_pressed("Jump"));

        hub.on_gamepad_button(gamepad, GamepadButton::South, false);
        hub.update(&clay_ui::LayoutTree::default());
        assert!(!hub.actions().is_held("Jump"));
        assert!(hub.actions().just_released("Jump"));
    }

    #[test]
    fn input_hub_routes_gamepad_axis_to_analog_action_value() {
        let mut hub = InputHub::new();
        let gamepad = GamepadId(4);
        hub.actions_mut().bind_input(
            "MoveRight",
            ActionBinding::gamepad_axis_for(
                gamepad,
                GamepadAxis::LeftStickX,
                ActionAxisDirection::Positive,
            )
            .with_threshold(0.25),
        );
        hub.actions_mut().bind_input(
            "MoveLeft",
            ActionBinding::gamepad_axis_for(
                gamepad,
                GamepadAxis::LeftStickX,
                ActionAxisDirection::Negative,
            )
            .with_threshold(0.25),
        );

        hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickX, 0.6);
        hub.update(&clay_ui::LayoutTree::default());
        assert_eq!(hub.actions().value("MoveRight"), 0.6);
        assert_eq!(hub.actions().value("MoveLeft"), 0.0);
        assert!(hub.actions().is_held("MoveRight"));
        assert!(!hub.actions().is_held("MoveLeft"));

        hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickX, -0.75);
        hub.update(&clay_ui::LayoutTree::default());
        assert_eq!(hub.actions().value("MoveRight"), 0.0);
        assert_eq!(hub.actions().value("MoveLeft"), 0.75);
        assert!(!hub.actions().is_held("MoveRight"));
        assert!(hub.actions().is_held("MoveLeft"));
    }

    #[test]
    fn input_hub_publishes_raw_key_polling_state_per_update() {
        let mut hub = InputHub::new();
        hub.on_key_input(&press("KeyW"));
        assert!(hub.is_key_pressed("KeyW"));
        assert!(!hub.is_key_just_pressed("KeyW"));

        hub.update(&clay_ui::LayoutTree::default());
        assert!(hub.is_key_pressed("KeyW"));
        assert!(hub.is_key_just_pressed("KeyW"));
        assert!(!hub.is_key_just_released("KeyW"));

        hub.update(&clay_ui::LayoutTree::default());
        assert!(hub.is_key_pressed("KeyW"));
        assert!(!hub.is_key_just_pressed("KeyW"));

        hub.on_key_input(&release("KeyW"));
        hub.update(&clay_ui::LayoutTree::default());
        assert!(!hub.is_key_pressed("KeyW"));
        assert!(hub.is_key_just_released("KeyW"));
    }

    #[test]
    fn input_hub_publishes_raw_mouse_polling_state_per_update() {
        let mut hub = InputHub::new();
        hub.on_pointer_moved(clay_ui::WindowLogicalPx::new(10.0, 20.0));
        hub.on_pointer_moved(clay_ui::WindowLogicalPx::new(13.0, 25.0));
        hub.on_pointer_button(clay_ui::WindowLogicalPx::new(13.0, 25.0), 0, true);

        hub.update(&clay_ui::LayoutTree::default());
        assert_eq!(
            hub.mouse_position(),
            clay_ui::WindowLogicalPx::new(13.0, 25.0)
        );
        assert_eq!(hub.mouse_delta(), glam::Vec2::new(3.0, 5.0));
        assert!(hub.is_mouse_button_pressed(0));
        assert!(hub.is_mouse_button_just_pressed(0));

        hub.update(&clay_ui::LayoutTree::default());
        assert_eq!(hub.mouse_delta(), glam::Vec2::ZERO);
        assert!(hub.is_mouse_button_pressed(0));
        assert!(!hub.is_mouse_button_just_pressed(0));

        hub.on_pointer_button(clay_ui::WindowLogicalPx::new(13.0, 25.0), 0, false);
        hub.update(&clay_ui::LayoutTree::default());
        assert!(!hub.is_mouse_button_pressed(0));
        assert!(hub.is_mouse_button_just_released(0));
    }

    #[test]
    fn input_hub_reports_optional_gaze_direction() {
        let mut hub = InputHub::new();
        assert_eq!(hub.gaze_direction(), None);

        hub.on_gaze_direction(Some(glam::Vec2::new(0.25, -0.5)));
        assert_eq!(hub.gaze_direction(), Some(glam::Vec2::new(0.25, -0.5)));

        hub.update(&clay_ui::LayoutTree::default());
        assert_eq!(hub.gaze_direction(), Some(glam::Vec2::new(0.25, -0.5)));

        hub.on_gaze_direction(None);
        assert_eq!(hub.gaze_direction(), None);
    }

    #[test]
    fn input_hub_ignores_non_finite_gaze_direction() {
        let mut hub = InputHub::new();
        hub.on_gaze_direction(Some(glam::Vec2::new(0.0, 0.0)));
        hub.on_gaze_direction(Some(glam::Vec2::new(f32::NAN, 0.25)));

        assert_eq!(hub.gaze_direction(), Some(glam::Vec2::new(0.0, 0.0)));
    }

    #[test]
    fn input_hub_publishes_gamepad_button_state_per_update() {
        let mut hub = InputHub::new();
        let gamepad = GamepadId(2);

        hub.on_gamepad_button(gamepad, GamepadButton::South, true);
        assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
        assert!(!hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::South));

        hub.update(&clay_ui::LayoutTree::default());
        assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
        assert!(hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::South));
        assert!(!hub.is_gamepad_button_just_released(gamepad, GamepadButton::South));

        hub.update(&clay_ui::LayoutTree::default());
        assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
        assert!(!hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::South));

        hub.on_gamepad_button(gamepad, GamepadButton::South, false);
        hub.update(&clay_ui::LayoutTree::default());
        assert!(!hub.is_gamepad_button_pressed(gamepad, GamepadButton::South));
        assert!(hub.is_gamepad_button_just_released(gamepad, GamepadButton::South));
    }

    #[test]
    fn input_hub_tracks_gamepad_axis_values() {
        let mut hub = InputHub::new();
        let gamepad = GamepadId(1);

        assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickX), 0.0);
        hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickX, 0.5);
        hub.on_gamepad_axis(gamepad, GamepadAxis::RightTrigger, 3.0);

        assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickX), 0.5);
        assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::RightTrigger), 1.0);
    }

    #[test]
    fn input_hub_clears_disconnected_gamepad_state() {
        let mut hub = InputHub::new();
        let gamepad = GamepadId(3);

        hub.on_gamepad_button(gamepad, GamepadButton::East, true);
        hub.on_gamepad_axis(gamepad, GamepadAxis::LeftStickY, -0.75);
        hub.update(&clay_ui::LayoutTree::default());

        assert!(hub.is_gamepad_button_pressed(gamepad, GamepadButton::East));
        assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickY), -0.75);

        hub.clear_gamepad(gamepad);
        assert!(!hub.is_gamepad_button_pressed(gamepad, GamepadButton::East));
        assert!(!hub.is_gamepad_button_just_pressed(gamepad, GamepadButton::East));
        assert_eq!(hub.gamepad_axis(gamepad, GamepadAxis::LeftStickY), 0.0);
    }
}
