use std::collections::{HashMap, HashSet};

use glam::Vec2;

use crate::{EasingRegistry, ElementId, LayoutTree, Rect};

use super::{
    EventContext, EventPhase, FocusScope, Hit, InputEvent, InteractionPhase, ModifierKeys,
    PointerState, ScrollAxis, ScrollConfig, ScrollState, SliderConfig, UiActivationEvent,
    UiEventResult, UiKeyEvent, UiMode, UiPointerEvent, UiTextEvent, WidgetBehavior, WidgetConfig,
    WidgetEventCallbacks, WidgetKind, WidgetState, focus_scope_contains,
    focus_scope_contains_with_parent_map, layout_parent_map, scroll_key_delta, slider_key_delta,
    slider_normalized_from_rect,
};

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
        let id_map: HashMap<u64, ElementId> = tree
            .nodes
            .iter()
            .map(|n| (n.id.hash, n.id.clone()))
            .collect();

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
                InputEvent::Key {
                    name,
                    pressed,
                    repeat,
                } => {
                    self.handle_key_event(tree, &name, pressed, repeat, ModifierKeys::default());
                }
                InputEvent::KeyWithModifiers {
                    name,
                    pressed,
                    repeat,
                    modifiers,
                } => {
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
            .filter(
                |(_, node)| match (active_scope, scoped_parent_map.as_ref()) {
                    (Some(scope), Some(parents)) => focus_scope_contains_with_parent_map(
                        scope,
                        &node.id,
                        parents,
                        tree.nodes.len(),
                    ),
                    _ => true,
                },
            )
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
                                if let Some(node) =
                                    tree.nodes.iter().find(|n| n.id.hash == hit.id.hash)
                                {
                                    let rect = node.rect;
                                    self.slider_track_rects.insert(hit.id.hash, rect);
                                    if let Some(config) =
                                        self.slider_configs.get(&hit.id.hash).copied()
                                    {
                                        let normalized = slider_normalized_from_rect(
                                            axis,
                                            rect,
                                            pointer.position,
                                            config.thumb_radius,
                                        );
                                        let new_value = config.clamp(
                                            config.min
                                                + normalized.clamp(0.0, 1.0) * config.range(),
                                        );
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
        let config = self
            .slider_configs
            .get(&id.hash)
            .copied()
            .unwrap_or_default();
        // Use the final displayed track rect captured on press for absolute
        // mapping. This keeps dragging in the same coordinate space as render
        // and hit testing regardless of slider width, parent alignment, or DPI.
        let Some(rect) = self.slider_track_rects.get(&id.hash).copied() else {
            return;
        };
        let normalized = slider_normalized_from_rect(axis, rect, pointer_pos, config.thumb_radius);
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
        let text_event = UiTextEvent {
            target: target.clone(),
            text,
            modifiers,
        };
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

    fn pointer_callback_target(
        &self,
        tree: &LayoutTree,
        pointer: PointerState,
    ) -> Option<ElementId> {
        match pointer.phase {
            InteractionPhase::Pressed | InteractionPhase::ReleasedThisFrame => {
                self.captured.clone().or_else(|| {
                    self.hit_test_interactive(tree, pointer.position)
                        .map(|hit| hit.id)
                })
            }
            InteractionPhase::PressedThisFrame | InteractionPhase::Released => self
                .hit_test_interactive(tree, pointer.position)
                .map(|hit| hit.id),
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
        let event = UiPointerEvent {
            target: target.clone(),
            pointer,
        };
        self.dispatch_bubbling(tree, target, |callbacks, phase, prevented| {
            let callback = match pointer.phase {
                InteractionPhase::PressedThisFrame => callbacks.on_pointer_down.as_mut(),
                InteractionPhase::ReleasedThisFrame => callbacks.on_pointer_up.as_mut(),
                InteractionPhase::Pressed | InteractionPhase::Released => {
                    callbacks.on_pointer_move.as_mut()
                }
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
            let phase = if index == 0 {
                EventPhase::Target
            } else {
                EventPhase::Bubble
            };
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
                let scroll_ok = self
                    .behaviors
                    .get(&current)
                    .map_or(true, |b| b.pointer_scroll);
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
