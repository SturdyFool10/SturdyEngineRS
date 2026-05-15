use super::*;
use crate::{
    Axis, Element, ElementId, LayoutCache, LayoutOutput, LayoutSizing, LayoutTree, Rect, Size,
    UiLayer, UiShape,
};
use glam::Vec2;

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
        ScrollConfig::new(Vec2::new(100.0, 80.0), Vec2::new(300.0, 260.0)).axis(ScrollAxis::Both),
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
    input.push_focus_scope(FocusScope::new(scope_id.clone(), popover_id).dismiss_on_cancel(true));

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
    // Track element is 100×40 at origin (0,0), so x=50 maps to 0.5.
    input.set_slider_config(
        id.clone(),
        SliderConfig::new(0.0, 1.0).step(0.01).track_extent(100.0),
    );
    input.set_slider_value(&id, 0.0);

    // Press at x=50: value snaps to 0.5.
    input.queue(InputEvent::Pointer(PointerState {
        position: Vec2::new(50.0, 20.0),
        phase: InteractionPhase::PressedThisFrame,
        ..PointerState::default()
    }));
    input.update(&layout);
    assert!(
        (input.slider_value(&id) - 0.5).abs() < 1e-5,
        "press snaps to 0.5"
    );

    // Drag to the rightmost thumb center: x=100-8=92 should be 1.0.
    input.queue(InputEvent::Pointer(PointerState {
        position: Vec2::new(92.0, 20.0),
        phase: InteractionPhase::Pressed,
        ..PointerState::default()
    }));
    input.update(&layout);
    assert!(
        (input.slider_value(&id) - 1.0).abs() < 1e-5,
        "drag to right edge gives 1.0"
    );
}

#[test]
fn pointer_drag_uses_slider_layout_rect_not_config_extent() {
    let id = ElementId::new("slider");
    let element = test_element(id.clone());
    let layout = layout_for(&element);
    let mut input = InputSimulator::default();
    input.set_widget_behavior(id.clone(), WidgetBehavior::slider(Axis::Horizontal));
    input.set_slider_config(
        id.clone(),
        SliderConfig::new(0.0, 1.0).step(0.01).track_extent(240.0),
    );
    input.set_slider_value(&id, 0.0);

    input.queue(InputEvent::Pointer(PointerState {
        position: Vec2::new(50.0, 20.0),
        phase: InteractionPhase::PressedThisFrame,
        ..PointerState::default()
    }));
    input.update(&layout);

    assert!(
        (input.slider_value(&id) - 0.5).abs() < 1e-5,
        "drag mapping must use the displayed 100 px layout rect, not the 240 px config extent"
    );
}

#[test]
fn pointer_drag_uses_offset_slider_final_display_rect() {
    let id = ElementId::new("slider");
    let layout = LayoutTree {
        nodes: vec![layout_node(
            id.clone(),
            0,
            Rect::new(300.0, 40.0, 240.0, 20.0),
            UiLayer::Content,
            0,
            false,
        )],
    };
    let mut input = InputSimulator::default();
    input.set_widget_behavior(id.clone(), WidgetBehavior::slider(Axis::Horizontal));
    input.set_slider_config(
        id.clone(),
        SliderConfig::new(0.0, 1.0).step(0.01).track_extent(120.0),
    );
    input.set_slider_value(&id, 0.0);

    input.queue(InputEvent::Pointer(PointerState {
        position: Vec2::new(420.0, 50.0),
        phase: InteractionPhase::PressedThisFrame,
        ..PointerState::default()
    }));
    input.update(&layout);
    assert!(
        (input.slider_value(&id) - 0.5).abs() < 1e-5,
        "center of the final displayed rect should map to 0.5"
    );

    input.queue(InputEvent::Pointer(PointerState {
        position: Vec2::new(532.0, 50.0),
        phase: InteractionPhase::Pressed,
        ..PointerState::default()
    }));
    input.update(&layout);
    assert!(
        (input.slider_value(&id) - 1.0).abs() < 1e-5,
        "right edge of the final displayed rect should map to 1.0"
    );
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
    input.push_focus_scope(FocusScope::new(scope_id.clone(), root_id).dismiss_on_cancel(true));

    input.queue(InputEvent::Key {
        name: "Escape".into(),
        pressed: true,
        repeat: false,
    });
    input.update(&LayoutTree::default());

    assert_eq!(
        input
            .dismissed_focus_scopes()
            .iter()
            .map(|id| id.hash)
            .collect::<Vec<_>>(),
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
