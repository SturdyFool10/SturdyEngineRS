use std::collections::HashMap;

use glam::Vec2;

use crate::{Axis, ElementId, LayoutTree, Rect};

use super::{FocusScope, ScrollAxis, ScrollConfig, SliderConfig};

pub(super) fn scroll_key_delta(name: &str, config: ScrollConfig) -> Vec2 {
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

pub(super) fn slider_key_delta(name: &str, axis: Axis, config: &SliderConfig) -> f32 {
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

pub(super) fn slider_normalized_from_rect(
    axis: Axis,
    rect: Rect,
    pointer_pos: Vec2,
    thumb_radius: f32,
) -> f32 {
    match axis {
        Axis::Horizontal => {
            let radius = thumb_radius.max(0.0).min(rect.size.width * 0.5);
            let start = rect.origin.x + radius;
            let travel = (rect.size.width - radius * 2.0).max(f32::EPSILON);
            (pointer_pos.x - start) / travel
        }
        Axis::Vertical => {
            let radius = thumb_radius.max(0.0).min(rect.size.height * 0.5);
            let start = rect.origin.y + radius;
            let travel = (rect.size.height - radius * 2.0).max(f32::EPSILON);
            (pointer_pos.y - start) / travel
        }
    }
}

pub(super) fn focus_scope_contains(tree: &LayoutTree, scope: &FocusScope, id: &ElementId) -> bool {
    let parents = layout_parent_map(tree);
    focus_scope_contains_with_parent_map(scope, id, &parents, tree.nodes.len())
}

pub(super) fn layout_parent_map(tree: &LayoutTree) -> HashMap<u64, u64> {
    let mut parents = HashMap::with_capacity(tree.nodes.len());
    for node in &tree.nodes {
        parents.insert(node.id.hash, node.parent);
    }
    parents
}

pub(super) fn focus_scope_contains_with_parent_map(
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
