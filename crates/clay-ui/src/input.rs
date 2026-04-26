use std::collections::{HashMap, HashSet};

use glam::Vec2;

use crate::{ElementId, LayoutTree, UiLayer, UiShape};

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

#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    Pointer(PointerState),
    Scroll {
        target: Option<ElementId>,
        delta: Vec2,
    },
    Text(String),
    Activate(ElementId),
    Focus(ElementId),
    Blur,
    Cancel,
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
}

impl InputSimulator {
    pub fn queue(&mut self, event: InputEvent) {
        self.events.push(event);
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
        self.active.clear();
        self.dismissed_scopes.clear();
        for scroll in self.scrolls.values_mut() {
            scroll.delta = Vec2::ZERO;
        }
        let events = std::mem::take(&mut self.events);
        for event in events {
            match event {
                InputEvent::Pointer(pointer) => {
                    self.pointer = pointer;
                    self.update_pointer_interaction(tree, pointer);
                }
                InputEvent::Scroll { target, delta } => {
                    let target = match target {
                        Some(target) if self.input_allowed_by_active_scope(tree, &target) => {
                            Some(target)
                        }
                        Some(_) => None,
                        None => self
                            .hit_test_interactive(tree, self.pointer.position)
                            .map(|hit| hit.id),
                    };
                    if let Some(target) = target {
                        self.apply_scroll(&target, delta);
                    }
                }
                InputEvent::Text(_) => {}
                InputEvent::Activate(id) => {
                    if self.activation_allowed(tree, &id) {
                        self.active.insert(id.hash);
                    }
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
        hit
    }

    pub fn hit_test(&self, tree: &LayoutTree, point: Vec2) -> Option<Hit> {
        tree.nodes
            .iter()
            .filter(|node| node.shape.contains_point(node.rect, point))
            .max_by_key(|node| (node.layer, node.z_index))
            .map(|node| Hit {
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

        tree.nodes
            .iter()
            .filter(|node| node.shape.contains_point(node.rect, point))
            .filter(|node| !node.transparent_to_input)
            .filter(|node| !self.widget_config(&node.id).disabled)
            .filter(|node| match (active_scope, scoped_parent_map.as_ref()) {
                (Some(scope), Some(parents)) => {
                    focus_scope_contains_with_parent_map(scope, &node.id, parents, tree.nodes.len())
                }
                _ => true,
            })
            .max_by_key(|node| (node.layer, node.z_index))
            .map(|node| Hit {
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
                    self.focused = Some(hit.id.clone());
                    self.pressed = Some(hit.id.clone());
                    self.captured = Some(hit.id);
                }
            }
            InteractionPhase::Pressed => {
                if self.captured.is_none() {
                    self.pressed = hit.map(|hit| hit.id);
                }
            }
            InteractionPhase::ReleasedThisFrame => {
                if let (Some(captured), Some(hit)) = (self.captured.take(), hit)
                    && captured.hash == hit.id.hash
                {
                    self.active.insert(captured.hash);
                }
                self.pressed = None;
            }
            InteractionPhase::Released => {
                self.pressed = None;
                self.captured = None;
            }
        }
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
}
