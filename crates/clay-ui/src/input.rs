use std::collections::{HashMap, HashSet};

use glam::Vec2;

use crate::{ElementId, LayoutTree};

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
    pub delta: Vec2,
    pub momentum: Vec2,
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
}

#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    pub id: ElementId,
    pub z_index: i16,
}

#[derive(Default)]
pub struct InputSimulator {
    pointer: PointerState,
    focused: Option<ElementId>,
    active: HashSet<u64>,
    scrolls: HashMap<u64, ScrollState>,
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

    pub fn scroll_state(&self, id: &ElementId) -> ScrollState {
        self.scrolls.get(&id.hash).copied().unwrap_or_default()
    }

    pub fn update(&mut self, tree: &LayoutTree) -> Option<Hit> {
        let events = std::mem::take(&mut self.events);
        for event in events {
            match event {
                InputEvent::Pointer(pointer) => self.pointer = pointer,
                InputEvent::Scroll { target, delta } => {
                    if let Some(target) = target {
                        self.scrolls.entry(target.hash).or_default().delta += delta;
                    }
                }
                InputEvent::Text(_) => {}
                InputEvent::Activate(id) => {
                    self.active.insert(id.hash);
                }
                InputEvent::Focus(id) => self.focused = Some(id),
            }
        }
        self.hit_test(tree, self.pointer.position)
    }

    pub fn hit_test(&self, tree: &LayoutTree, point: Vec2) -> Option<Hit> {
        tree.nodes
            .iter()
            .filter(|node| node.rect.contains(point))
            .max_by_key(|node| node.z_index)
            .map(|node| Hit {
                id: node.id.clone(),
                z_index: node.z_index,
            })
    }
}
