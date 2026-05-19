// Tests extracted from crates/clay-ui/src/layout/widgets/mod.rs
// Runtime code should stay separate from test code.

use super::*;
use crate::{
    ColorSpaceKind, Cx, ElementKind, FloatingAlign, FloatingAttachError, FloatingOptions,
    FloatingPlacement, InputSimulator, LayoutCache, LayoutTree, MosaicConfig, MosaicTileMode,
    MosaicTileSpec, Size, UiAntialiasing, UiImageFit, UiImageSampling, UiLayer, VirtualGridConfig,
    VirtualListConfig, VirtualTableConfig, VirtualTreeConfig,
};

/// Create a no-interaction Cx for widget structure tests.
fn test_cx_and_sim() -> (InputSimulator, WidgetPalette) {
    (InputSimulator::default(), WidgetPalette::default())
}
#[test]
fn button_builder_marks_label_as_nowrap() {
    let id = ElementId::new("button");
    let element = button_with_palette(
        id.clone(),
        "Run",
        &WidgetState::default(),
        &WidgetPalette::default(),
    );

    assert_eq!(element.children.len(), 1);
    let ElementKind::Text(text) = &element.children[0].kind else {
        panic!("button child should be text");
    };
    assert_eq!(text.text, "Run");
    assert_eq!(text.style.wrap, TextWrap::None);
    assert_eq!(element.style.padding, Edges::symmetric(12.0, 7.0));
}

#[test]
fn selected_radio_uses_selected_surface() {
    let id = ElementId::new("radio");
    let palette = WidgetPalette::default();
    let element = radio_with_palette(id, "Choice", true, &WidgetState::default(), &palette);

    assert_eq!(
        element.children[0].style.background,
        palette.surface_selected
    );
}

#[test]
fn checked_toggle_places_knob_at_end() {
    let id = ElementId::new("toggle");
    // progress=1.0 -> padding + full travel.
    let element = toggle_with_palette(
        id,
        "Enabled",
        1.0,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let track = &element.children[0];

    assert_eq!(track.children.len(), 1);
    let knob = &track.children[0];
    assert_eq!(
        knob.layout.position,
        crate::LayoutPosition::Absolute {
            // offset is relative to the content rect (already inset by
            // track_padding=2); knob_travel = 36-4-16 = 16, progress=1.
            offset: Vec2::new(16.0, 0.0)
        },
    );
}

#[test]
fn unchecked_toggle_places_knob_at_start() {
    let id = ElementId::new("toggle");
    // progress=0.0 -> track padding.
    let element = toggle_with_palette(
        id,
        "Enabled",
        0.0,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let track = &element.children[0];

    let knob = &track.children[0];
    assert_eq!(
        knob.layout.position,
        crate::LayoutPosition::Absolute {
            // offset is relative to content rect; progress=0 so knob_offset_x=0.
            offset: Vec2::new(0.0, 0.0)
        },
    );
}

#[test]
fn custom_toggle_style_controls_track_and_knob_metrics() {
    let id = ElementId::new("toggle");
    let style = ToggleStyle {
        track_width: 48.0,
        track_height: 24.0,
        knob_size: 20.0,
        track_padding: 2.0,
        label_gap: 12.0,
    };
    let element = toggle_with_palette_in_space_and_style(
        id,
        "Enabled",
        1.0,
        ColorSpaceKind::Oklab,
        &style,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let track = &element.children[0];
    let knob = &track.children[0];

    assert_eq!(element.layout.gap, 12.0);
    assert_eq!(track.layout.width, LayoutSizing::Fixed(48.0));
    assert_eq!(track.layout.height, LayoutSizing::Fixed(24.0));
    assert_eq!(knob.layout.width, LayoutSizing::Fixed(20.0));
    assert_eq!(
        knob.layout.position,
        LayoutPosition::Absolute {
            // offset relative to content rect; knob_travel = 48-4-20 = 24, progress=1.
            offset: Vec2::new(24.0, 0.0)
        }
    );
}

#[test]
fn checked_checkbox_adds_check_mark() {
    let id = ElementId::new("checkbox");
    let element = checkbox_with_palette(
        id,
        "Accept",
        true,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let box_element = &element.children[0];

    assert_eq!(box_element.children.len(), 1);
    assert_eq!(box_element.layout.width, LayoutSizing::Fixed(16.0));
    assert!(box_element.children[0].style.transparent_to_input);
    assert_eq!(
        box_element.children[0].layout.width,
        LayoutSizing::Fixed(10.0)
    );
}

#[test]
fn custom_checkbox_and_radio_styles_control_indicator_metrics() {
    let checkbox_id = ElementId::new("checkbox");
    let checkbox = checkbox_with_palette_and_style(
        checkbox_id,
        "Accept",
        true,
        &CheckboxStyle {
            indicator_size: 22.0,
            mark_size: 12.0,
            indicator_radius: 6.0,
            mark_radius: 3.0,
            indicator_padding: Edges::all(5.0),
            label_gap: 10.0,
        },
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    assert_eq!(checkbox.layout.gap, 10.0);
    assert_eq!(checkbox.children[0].layout.width, LayoutSizing::Fixed(22.0));
    assert_eq!(
        checkbox.children[0].children[0].layout.width,
        LayoutSizing::Fixed(12.0)
    );

    let radio_id = ElementId::new("radio");
    let radio = radio_with_palette_and_style(
        radio_id,
        "Choice",
        true,
        &RadioStyle {
            indicator_size: 18.0,
            label_gap: 11.0,
        },
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    assert_eq!(radio.layout.gap, 11.0);
    assert_eq!(radio.children[0].layout.width, LayoutSizing::Fixed(18.0));
    assert_eq!(radio.children[0].layout.height, LayoutSizing::Fixed(18.0));
}

#[test]
fn segmented_control_shapes_outer_segments() {
    let id = ElementId::new("segments");
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let element = segmented_control(
        id.clone(),
        [
            SegmentSpec::new(ElementId::local("one", 0, &id), "One"),
            SegmentSpec::new(ElementId::local("two", 0, &id), "Two").selected(true),
            SegmentSpec::new(ElementId::local("three", 0, &id), "Three"),
        ],
        &cx,
    );

    assert!(matches!(
        element.children[0].style.shape,
        UiShape::IndependentCorners { .. }
    ));
    assert_eq!(element.children[1].style.shape, UiShape::Rect);
    assert!(matches!(
        element.children[2].style.shape,
        UiShape::IndependentCorners { .. }
    ));
}

#[test]
fn drag_bar_has_stable_axis_dimensions() {
    let id = ElementId::new("drag");
    let element = drag_bar_with_palette(
        id,
        DragBarAxis::Vertical,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );

    assert_eq!(element.layout.width, LayoutSizing::Fixed(6.0));
    assert!(matches!(element.layout.height, LayoutSizing::Grow { .. }));
}

#[test]
fn slider_clamps_value_into_fill_and_thumb_travel() {
    let id = ElementId::new("slider");
    let element = slider_with_palette(
        id,
        DragBarAxis::Horizontal,
        2.0,
        240.0,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );

    assert_eq!(element.layout.width, LayoutSizing::Fixed(240.0));
    assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(230.0));
    assert_eq!(
        element.children[0].layout.position,
        LayoutPosition::Absolute {
            offset: Vec2::new(2.0, 2.0)
        }
    );
    assert_eq!(
        element.children[1].layout.position,
        LayoutPosition::Absolute {
            offset: Vec2::new(224.0, 2.0)
        }
    );
    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
}

#[test]
fn custom_slider_style_controls_visual_metrics() {
    let id = ElementId::new("slider");
    let element = slider_with_palette_and_style(
        id,
        DragBarAxis::Horizontal,
        1.0,
        300.0,
        &SliderStyle {
            track_extent: Some(300.0),
            track_cross_extent: 28.0,
            thumb_size: 24.0,
            fill_inset: 3.0,
        },
        &WidgetState::default(),
        &WidgetPalette::default(),
    );

    assert_eq!(element.layout.width, LayoutSizing::Fixed(300.0));
    assert_eq!(element.layout.height, LayoutSizing::Fixed(28.0));
    assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(285.0));
    assert_eq!(element.children[0].layout.height, LayoutSizing::Fixed(22.0));
    assert_eq!(
        element.children[1].layout.position,
        LayoutPosition::Absolute {
            offset: Vec2::new(276.0, 2.0)
        }
    );
}

#[test]
fn progress_bar_uses_clamped_fill_percent() {
    let id = ElementId::new("progress");
    let element =
        progress_bar_with_palette(id, -1.0, &WidgetState::default(), &WidgetPalette::default());

    assert_eq!(element.children[0].layout.width, LayoutSizing::Percent(0.0));
    assert_eq!(element.layout.height, LayoutSizing::Fixed(8.0));
}

#[test]
fn image_builder_exposes_fit_sampling_and_edge_aa() {
    let id = ElementId::new("image");
    let options = UiImageOptions::default()
        .fit(UiImageFit::Cover)
        .sampling(UiImageSampling::MipmapLinear)
        .edge_antialiasing(UiAntialiasing::supersampled(2));
    let element = image_with_options(id, "avatar", Size::new(64.0, 32.0), options);

    assert_eq!(element.layout.width, LayoutSizing::Fixed(64.0));
    assert_eq!(element.layout.height, LayoutSizing::Fixed(32.0));
    let ElementKind::Image(image) = &element.kind else {
        panic!("image builder should create an image element");
    };
    assert_eq!(image.image_key, "avatar");
    assert_eq!(image.natural_size, Some(Size::new(64.0, 32.0)));
    assert_eq!(image.tint, UiColor::WHITE);
    assert_eq!(image.options.fit, UiImageFit::Cover);
    assert_eq!(image.options.sampling, UiImageSampling::MipmapLinear);
    assert_eq!(
        image.options.edge_antialiasing,
        UiAntialiasing::supersampled(2)
    );
}

#[test]
fn portal_host_uses_top_layer_and_can_pass_through_input() {
    let id = ElementId::new("portal");
    let child_id = ElementId::local("popover", 0, &id);
    let mut child = Element::new(child_id);
    child.layout.z_index = 3;
    child
        .children
        .push(Element::new(ElementId::local("label", 0, &child.id)));
    let element = portal_host(
        id,
        PortalHostConfig::new(Size::new(320.0, 180.0)).z_index(8),
        [child],
    );

    assert_eq!(element.layout.layer, UiLayer::TopLayer);
    assert_eq!(element.layout.z_index, 8);
    assert_eq!(element.layout.width, LayoutSizing::Fixed(320.0));
    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
    assert!(element.style.transparent_to_input);
    assert_eq!(element.children.len(), 1);
    assert_eq!(element.children[0].layout.layer, UiLayer::TopLayer);
    assert_eq!(element.children[0].layout.z_index, 12);
    assert_eq!(
        element.children[0].children[0].layout.layer,
        UiLayer::TopLayer
    );
    assert_eq!(element.children[0].children[0].layout.z_index, 12);
}

#[test]
fn modal_layer_uses_backdrop_and_captures_input() {
    let id = ElementId::new("modal");
    let dialog = Element::new(ElementId::local("dialog", 0, &id));
    let backdrop = UiColor::from_rgba8(5, 10, 15, 192);
    let element = modal_layer(
        id,
        ModalLayerConfig::new(Size::new(640.0, 360.0))
            .z_index(20)
            .backdrop(backdrop),
        [dialog],
    );

    assert_eq!(element.layout.layer, UiLayer::TopLayer);
    assert_eq!(element.layout.z_index, 20);
    assert_eq!(element.style.background, backdrop);
    assert!(!element.style.transparent_to_input);
    assert_eq!(element.children.len(), 1);
    assert_eq!(element.children[0].layout.layer, UiLayer::TopLayer);
    assert_eq!(element.children[0].layout.z_index, 21);
}

#[test]
fn tooltip_surface_wraps_text_and_passes_through_input() {
    let id = ElementId::new("tooltip");
    let palette = WidgetPalette::default();
    let element = tooltip_surface(
        id,
        "A longer helpful hint",
        Size::new(140.0, 44.0),
        &palette,
    );

    assert_eq!(element.layout.width, LayoutSizing::Fixed(140.0));
    assert_eq!(element.layout.height, LayoutSizing::Fixed(44.0));
    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
    assert!(element.style.transparent_to_input);
    assert_eq!(element.style.background, palette.surface.with_alpha(0.96));

    let ElementKind::Text(text) = &element.children[0].kind else {
        panic!("tooltip label should be text");
    };
    assert_eq!(text.text, "A longer helpful hint");
    assert_eq!(text.style.wrap, TextWrap::Words);
    assert!(element.children[0].style.transparent_to_input);
    assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(124.0));
}

#[test]
fn tooltip_layer_attaches_to_anchor_in_top_layer() {
    let root_id = ElementId::new("root");
    let anchor_id = ElementId::new("button");
    let mut root = Element::new(root_id);
    root.style.padding = Edges::all(10.0);
    root.layout.width = LayoutSizing::Fixed(240.0);
    root.layout.height = LayoutSizing::Fixed(120.0);
    let mut anchor = Element::new(anchor_id.clone());
    anchor.layout.width = LayoutSizing::Fixed(80.0);
    anchor.layout.height = LayoutSizing::Fixed(20.0);
    root.children.push(anchor);
    let layout =
        LayoutTree::compute(&root, Size::new(240.0, 120.0), &mut LayoutCache::default()).unwrap();
    let tooltip_id = ElementId::new("tooltip-layer");
    let config = TooltipConfig::new(
        Size::new(240.0, 120.0),
        anchor_id.clone(),
        Size::new(120.0, 36.0),
    )
    .options(
        FloatingOptions::default()
            .placement(FloatingPlacement::bottom(FloatingAlign::Center))
            .offset(6.0),
    )
    .z_index(44);

    let element = tooltip_layer(tooltip_id, &layout, config, "Helpful").unwrap();

    assert_eq!(element.layout.layer, UiLayer::TopLayer);
    assert_eq!(element.layout.z_index, 44);
    assert!(element.style.transparent_to_input);
    assert_eq!(element.children.len(), 1);
    assert_eq!(element.children[0].layout.layer, UiLayer::TopLayer);
    assert_eq!(element.children[0].layout.z_index, 45);
    assert_eq!(
        element.children[0].layout.position,
        LayoutPosition::Absolute {
            offset: Vec2::new(8.0, 36.0)
        }
    );
}

#[test]
fn tooltip_layer_reports_missing_anchor() {
    let tooltip_id = ElementId::new("tooltip-layer");
    let anchor_id = ElementId::new("missing-anchor");
    let config = TooltipConfig::new(
        Size::new(240.0, 120.0),
        anchor_id.clone(),
        Size::new(120.0, 36.0),
    );

    let error = tooltip_layer(tooltip_id, &LayoutTree::default(), config, "Helpful")
        .expect_err("missing anchor should be reported");

    assert_eq!(error, FloatingAttachError::AnchorNotFound(anchor_id));
}

#[test]
fn scrollbar_metrics_map_scroll_offset_to_thumb_position() {
    let metrics = ScrollbarMetrics::new(
        Axis::Vertical,
        Vec2::new(100.0, 100.0),
        Vec2::new(100.0, 300.0),
        Vec2::new(0.0, 100.0),
        90.0,
        18.0,
    );

    assert!(metrics.visible());
    assert_eq!(metrics.max_offset, 200.0);
    assert_eq!(metrics.thumb_extent, 30.0);
    assert_eq!(metrics.thumb_offset, 30.0);
}

#[test]
fn scrollbar_builder_uses_axis_dimensions_and_thumb_offset() {
    let metrics = ScrollbarMetrics::new(
        Axis::Horizontal,
        Vec2::new(100.0, 40.0),
        Vec2::new(300.0, 40.0),
        Vec2::new(100.0, 0.0),
        90.0,
        18.0,
    );
    let element = scrollbar_with_palette(
        ElementId::new("scrollbar"),
        metrics,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );

    assert_eq!(element.layout.width, LayoutSizing::Fixed(90.0));
    assert_eq!(element.layout.height, LayoutSizing::Fixed(8.0));
    assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(30.0));
    assert_eq!(element.children[1].layout.width, LayoutSizing::Fixed(30.0));
}

#[test]
fn widget_builders_produce_layoutable_trees() {
    let root = button_with_palette(
        ElementId::new("button"),
        "Layout",
        &WidgetState::default(),
        &WidgetPalette::default(),
    );

    let layout =
        LayoutTree::compute(&root, Size::new(200.0, 40.0), &mut LayoutCache::default()).unwrap();

    assert!(layout.by_id(&root.id).is_some());
}

#[test]
fn virtual_list_builder_adds_scroll_spacers_and_clip_viewport() {
    let id = ElementId::new("virtual-list");
    let layout = VirtualListConfig::new(100, 20.0, 80.0, 60.0)
        .overscan_items(1)
        .layout();
    let items = layout
        .render_items()
        .map(|item| {
            let mut element = Element::new(ElementId::local("row", item.index as u32, &id));
            element.layout.width = LayoutSizing::Fixed(200.0);
            element.layout.height = LayoutSizing::Fixed(item.extent);
            element
        })
        .collect::<Vec<_>>();

    let element = virtual_list(id, LayoutSizing::Fixed(200.0), 80.0, &layout, items);

    assert!(element.layout.clip_y);
    assert_eq!(element.layout.height, LayoutSizing::Fixed(80.0));
    let content = &element.children[0];
    assert_eq!(content.layout.scroll_offset, Vec2::new(0.0, -60.0));
    let virtual_content = &content.children[0];
    assert_eq!(
        virtual_content.children[0].layout.height,
        LayoutSizing::Fixed(layout.before_extent)
    );
    assert_eq!(
        virtual_content.children.last().unwrap().layout.height,
        LayoutSizing::Fixed(layout.after_extent)
    );
}

#[test]
fn virtual_dropdown_menu_builds_visible_option_rows() {
    let id = ElementId::new("virtual-dropdown");
    let layout = VirtualListConfig::new(8, 24.0, 72.0, 48.0)
        .overscan_items(0)
        .layout();
    let options = layout
        .render_items()
        .map(|item| {
            DropdownOptionSpec::new(
                ElementId::local("option", item.index as u32, &id),
                format!("Option {}", item.index),
            )
            .selected(item.index == 3)
            .disabled(item.index == 4)
        })
        .collect::<Vec<_>>();

    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let element = virtual_dropdown_menu(id, LayoutSizing::Fixed(180.0), &layout, options, &cx);

    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
    assert_eq!(element.layout.height, LayoutSizing::Fixed(72.0));
    assert_eq!(
        element.children[0].layout.scroll_offset,
        Vec2::new(0.0, -48.0)
    );

    let virtual_content = &element.children[0].children[0];
    assert_eq!(virtual_content.children.len(), 5);
    assert_eq!(
        virtual_content.children[0].layout.height,
        LayoutSizing::Fixed(layout.before_extent)
    );

    let first_option = &virtual_content.children[1];
    assert_eq!(first_option.layout.height, LayoutSizing::Fixed(24.0));
    let ElementKind::Text(text) = &first_option.children[1].kind else {
        panic!("dropdown option label should be text");
    };
    assert_eq!(text.text, "Option 2");
    assert_eq!(text.style.wrap, TextWrap::None);

    let selected_option = &virtual_content.children[2];
    assert_eq!(
        selected_option.children[0].style.background,
        WidgetPalette::default().accent
    );
    let disabled_option = &virtual_content.children[3];
    assert_eq!(
        disabled_option.style.background,
        WidgetPalette::default().surface_disabled
    );
}

#[test]
fn virtual_log_viewer_builds_visible_log_rows() {
    let id = ElementId::new("virtual-log");
    let layout = VirtualListConfig::new(6, 20.0, 40.0, 20.0)
        .overscan_items(0)
        .layout();
    let entries = layout
        .render_items()
        .map(|item| {
            LogEntrySpec::new(
                ElementId::local("entry", item.index as u32, &id),
                LogLevel::Warn,
                format!("line {}", item.index),
            )
            .timestamp(format!("00:0{}", item.index))
            .source("ui")
        })
        .collect::<Vec<_>>();

    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let element = virtual_log_viewer(id, LayoutSizing::Fixed(360.0), &layout, entries, &cx);

    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
    assert_eq!(element.layout.height, LayoutSizing::Fixed(40.0));
    assert_eq!(
        element.children[0].layout.scroll_offset,
        Vec2::new(0.0, -20.0)
    );

    let virtual_content = &element.children[0].children[0];
    assert_eq!(virtual_content.children.len(), 4);
    let first_entry = &virtual_content.children[1];
    assert_eq!(first_entry.layout.height, LayoutSizing::Fixed(20.0));
    assert_eq!(first_entry.children.len(), 4);

    let ElementKind::Text(level) = &first_entry.children[0].kind else {
        panic!("log level should be text");
    };
    assert_eq!(level.text, "WARN");
    assert_eq!(level.style.wrap, TextWrap::None);
    assert_eq!(level.style.family_candidates, vec!["monospace"]);

    let ElementKind::Text(message) = &first_entry.children[3].kind else {
        panic!("log message should be text");
    };
    assert_eq!(message.text, "line 1");
    assert!(matches!(
        first_entry.children[3].layout.width,
        LayoutSizing::Grow { .. }
    ));
}

#[test]
fn virtual_grid_builder_groups_rendered_items_into_rows() {
    let id = ElementId::new("virtual-grid");
    let layout = VirtualGridConfig::new(
        20,
        Vec2::new(40.0, 24.0),
        Vec2::new(80.0, 48.0),
        Vec2::new(40.0, 24.0),
        4,
    )
    .overscan_rows(0)
    .overscan_columns(0)
    .layout();
    let items = layout
        .render_items()
        .map(|item| {
            let mut element = Element::new(ElementId::local("cell", item.index as u32, &id));
            element.layout.width = LayoutSizing::Fixed(item.size.x);
            element.layout.height = LayoutSizing::Fixed(item.size.y);
            element
        })
        .collect::<Vec<_>>();

    let element = virtual_grid(id, Vec2::new(80.0, 48.0), &layout, items);

    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
    assert_eq!(
        element.children[0].layout.scroll_offset,
        Vec2::new(-40.0, -24.0)
    );
    let virtual_content = &element.children[0].children[0];
    assert_eq!(virtual_content.children.len(), 4);
    assert_eq!(
        virtual_content.children[0].layout.height,
        LayoutSizing::Fixed(layout.before_rows_extent)
    );
    let first_row = &virtual_content.children[1];
    assert_eq!(first_row.layout.height, LayoutSizing::Fixed(24.0));
    assert_eq!(
        first_row.children[0].layout.width,
        LayoutSizing::Fixed(40.0)
    );
}

#[test]
fn virtual_table_builder_groups_rendered_cells_into_rows() {
    let id = ElementId::new("virtual-table");
    let layout = VirtualTableConfig::new(
        10,
        4,
        Vec2::new(48.0, 22.0),
        Vec2::new(96.0, 44.0),
        Vec2::new(48.0, 22.0),
    )
    .overscan_rows(0)
    .overscan_columns(0)
    .layout();
    let cells = layout
        .render_cells()
        .map(|cell| {
            let mut element = Element::new(ElementId::local("cell", cell.index as u32, &id));
            element.layout.width = LayoutSizing::Fixed(cell.size.x);
            element.layout.height = LayoutSizing::Fixed(cell.size.y);
            element
        })
        .collect::<Vec<_>>();

    let element = virtual_table(id, Vec2::new(96.0, 44.0), &layout, cells);

    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
    assert_eq!(
        element.children[0].layout.scroll_offset,
        Vec2::new(-48.0, -22.0)
    );
    let virtual_content = &element.children[0].children[0];
    assert_eq!(virtual_content.children.len(), 4);
    let first_row = &virtual_content.children[1];
    assert_eq!(first_row.layout.height, LayoutSizing::Fixed(22.0));
    assert_eq!(
        first_row.children[0].layout.width,
        LayoutSizing::Fixed(48.0)
    );
}

#[test]
fn virtual_tree_builder_adds_scroll_spacers_and_clip_viewport() {
    let id = ElementId::new("virtual-tree");
    let layout = VirtualTreeConfig::new(40, 18.0, 72.0, 54.0)
        .overscan_rows(1)
        .layout();
    let rows = layout
        .render_rows()
        .map(|row| {
            let mut element = Element::new(ElementId::local("row", row.row_index as u32, &id));
            element.layout.width = LayoutSizing::Fixed(220.0);
            element.layout.height = LayoutSizing::Fixed(row.extent);
            element
        })
        .collect::<Vec<_>>();

    let element = virtual_tree(id, LayoutSizing::Fixed(220.0), 72.0, &layout, rows);

    assert!(element.layout.clip_y);
    assert_eq!(
        element.children[0].layout.scroll_offset,
        Vec2::new(0.0, -54.0)
    );
    let virtual_content = &element.children[0].children[0];
    assert_eq!(
        virtual_content.children[0].layout.height,
        LayoutSizing::Fixed(layout.before_extent)
    );
    assert_eq!(
        virtual_content.children.last().unwrap().layout.height,
        LayoutSizing::Fixed(layout.after_extent)
    );
}

#[test]
fn mosaic_container_positions_tiles_from_mosaic_layout() {
    let id = ElementId::new("mosaic");
    let layout = MosaicConfig::new(200.0, 2, 100.0)
        .tile(
            MosaicTileSpec::new("image")
                .mode(MosaicTileMode::Fit)
                .aspect_ratio(2.0),
        )
        .tile(MosaicTileSpec::new("side"))
        .layout()
        .unwrap();
    let tile_a = Element::new(ElementId::local("image", 0, &id));
    let tile_b = Element::new(ElementId::local("side", 0, &id));

    let element = mosaic_container(id, &layout, [tile_a, tile_b]);

    assert_eq!(element.layout.width, LayoutSizing::Fixed(200.0));
    assert_eq!(element.layout.height, LayoutSizing::Fixed(100.0));
    assert_eq!(
        element.children[0].layout.position,
        LayoutPosition::Absolute {
            offset: layout.tiles[0].rect.origin
        }
    );
    assert_eq!(element.children[0].layout.height, LayoutSizing::Fixed(50.0));
    assert_eq!(
        element.children[1].layout.position,
        LayoutPosition::Absolute {
            offset: layout.tiles[1].rect.origin
        }
    );
}

#[test]
fn virtual_mosaic_emits_visible_tiles_inside_full_sized_content() {
    let id = ElementId::new("virtual-mosaic");
    let layout = MosaicConfig::new(200.0, 2, 50.0)
        .tile(MosaicTileSpec::new("a"))
        .tile(MosaicTileSpec::new("b"))
        .tile(MosaicTileSpec::new("c"))
        .tile(MosaicTileSpec::new("d"))
        .tile(MosaicTileSpec::new("e"))
        .tile(MosaicTileSpec::new("f"))
        .layout()
        .unwrap();
    let viewport_size = Vec2::new(200.0, 49.0);
    let scroll_offset = Vec2::new(20.0, 50.5);
    let visible_tiles = layout
        .visible_tiles(Rect::new(0.0, 50.5, viewport_size.x, viewport_size.y), 0.0)
        .map(|tile| Element::new(ElementId::local(&tile.name, tile.source_index as u32, &id)))
        .collect::<Vec<_>>();

    let element = virtual_mosaic(
        id.clone(),
        &layout,
        viewport_size,
        scroll_offset,
        0.0,
        visible_tiles,
    );

    assert!(element.layout.clip_x);
    assert!(element.layout.clip_y);
    assert_eq!(element.layout.width, LayoutSizing::Fixed(200.0));
    assert_eq!(element.layout.height, LayoutSizing::Fixed(49.0));

    let scroll_content = &element.children[0];
    assert_eq!(scroll_content.layout.scroll_offset, Vec2::new(0.0, -50.5));

    let mosaic_content = &scroll_content.children[0];
    assert_eq!(mosaic_content.layout.width, LayoutSizing::Fixed(200.0));
    assert_eq!(mosaic_content.layout.height, LayoutSizing::Fixed(150.0));
    assert_eq!(
        mosaic_content
            .children
            .iter()
            .map(|child| child.id.label.as_str())
            .collect::<Vec<_>>(),
        vec!["c", "d"]
    );
    assert_eq!(
        mosaic_content.children[0].layout.position,
        LayoutPosition::Absolute {
            offset: layout.tile("c").unwrap().rect.origin
        }
    );
    assert_eq!(
        mosaic_content.children[1].layout.position,
        LayoutPosition::Absolute {
            offset: layout.tile("d").unwrap().rect.origin
        }
    );
}

#[test]
fn scroll_container_filters_offset_by_axis_and_clips_viewport() {
    let id = ElementId::new("scroll");
    let child = Element::new(ElementId::local("child", 0, &id));

    let vertical = scroll_container(
        id.clone(),
        LayoutSizing::Fixed(100.0),
        LayoutSizing::Fixed(80.0),
        ScrollAxis::Vertical,
        Vec2::new(40.0, 24.0),
        [child.clone()],
    );
    assert!(!vertical.layout.clip_x);
    assert!(vertical.layout.clip_y);
    assert_eq!(
        vertical.children[0].layout.scroll_offset,
        Vec2::new(0.0, -24.0)
    );

    let both = scroll_container(
        id,
        LayoutSizing::Fixed(100.0),
        LayoutSizing::Fixed(80.0),
        ScrollAxis::Both,
        Vec2::new(40.0, 24.0),
        [child],
    );
    assert!(both.layout.clip_x);
    assert!(both.layout.clip_y);
    assert_eq!(
        both.children[0].layout.scroll_offset,
        Vec2::new(-40.0, -24.0)
    );
}

#[test]
fn scroll_container_with_scrollbars_adds_viewport_and_bars() {
    let id = ElementId::new("scrollbars");
    let child = Element::new(ElementId::local("child", 0, &id));
    let element = scroll_container_with_scrollbars(
        id,
        LayoutSizing::Fixed(100.0),
        LayoutSizing::Fixed(80.0),
        ScrollConfig::new(Vec2::new(100.0, 80.0), Vec2::new(220.0, 240.0)).axis(ScrollAxis::Both),
        Vec2::new(30.0, 40.0),
        [child],
    );

    assert_eq!(element.children.len(), 2);
    let row = &element.children[0];
    assert_eq!(row.children.len(), 2);
    assert_eq!(
        row.children[0].id,
        ElementId::local("viewport", 0, &element.id)
    );
    assert_eq!(
        row.children[0].children[0].layout.scroll_offset,
        Vec2::new(-30.0, -40.0)
    );
    assert_eq!(
        row.children[1].id,
        ElementId::local("vertical-scrollbar", 0, &element.id)
    );
    assert_eq!(
        element.children[1].id,
        ElementId::local("horizontal-scrollbar", 0, &element.id)
    );
}

// ── New widget tests ───────────────────────────────────────────────────────

#[test]
fn label_uses_text_color_from_state() {
    let id = ElementId::new("lbl");
    let mut state = WidgetState::default();
    state.disabled = true;
    let el = label_with_palette(id, "Hello", &state, &WidgetPalette::default());
    let ElementKind::Text(text) = &el.kind else {
        panic!("label should be a text element");
    };
    assert_eq!(text.text, "Hello");
    assert_eq!(text.style.color, WidgetPalette::default().muted_text);
}

#[test]
fn divider_horizontal_is_1px_tall_grow_wide() {
    let el = divider(ElementId::new("div"), Axis::Horizontal);
    assert_eq!(el.layout.height, LayoutSizing::Fixed(1.0));
    assert!(matches!(el.layout.width, LayoutSizing::Grow { .. }));
}

#[test]
fn divider_vertical_is_1px_wide_grow_tall() {
    let el = divider(ElementId::new("div"), Axis::Vertical);
    assert_eq!(el.layout.width, LayoutSizing::Fixed(1.0));
    assert!(matches!(el.layout.height, LayoutSizing::Grow { .. }));
}

#[test]
fn badge_uses_variant_colors() {
    let palette = WidgetPalette::default();
    let (bg, fg) = BadgeVariant::Error.colors(&palette);
    let el = badge_with_palette(
        ElementId::new("badge"),
        "Error",
        BadgeVariant::Error,
        &palette,
    );
    assert_eq!(el.style.background, bg);
    let ElementKind::Text(text) = &el.children[0].kind else {
        panic!("badge child should be text");
    };
    assert_eq!(text.style.color, fg);
    assert_eq!(text.text, "Error");
}

#[test]
fn empty_state_with_description_has_two_text_children() {
    let el = empty_state(
        ElementId::new("empty"),
        "No results",
        Some("Try adjusting your filters"),
        200.0,
        120.0,
    );
    assert_eq!(el.children.len(), 2);
    let ElementKind::Text(title) = &el.children[0].kind else {
        panic!("first child should be text");
    };
    assert_eq!(title.text, "No results");
    let ElementKind::Text(desc) = &el.children[1].kind else {
        panic!("second child should be text");
    };
    assert_eq!(desc.text, "Try adjusting your filters");
}

#[test]
fn empty_state_without_description_has_one_text_child() {
    let el = empty_state(
        ElementId::new("empty"),
        "No results",
        None::<&str>,
        200.0,
        120.0,
    );
    assert_eq!(el.children.len(), 1);
}

#[test]
fn tab_bar_selected_tab_uses_accent_outline_at_bottom() {
    let id = ElementId::new("tabs");
    let tab_a = ElementId::local("a", 0, &id);
    let tab_b = ElementId::local("b", 0, &id);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = tab_bar(
        id,
        [
            TabSpec::new(tab_a, "A").selected(true),
            TabSpec::new(tab_b, "B"),
        ],
        &cx,
    );
    let selected = &el.children[0];
    let unselected = &el.children[1];
    assert_eq!(selected.style.outline, palette.accent);
    assert_eq!(selected.style.outline_width.bottom, 2.0);
    assert_eq!(unselected.style.outline, UiColor::TRANSPARENT);
}

#[test]
fn tab_with_icon_has_icon_and_label_children() {
    let id = ElementId::new("tabs");
    let tab_a = ElementId::local("a", 0, &id);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = tab_bar(
        id,
        [TabSpec::new(tab_a, "Files")
            .icon("icon-folder")
            .icon_size(14.0)],
        &cx,
    );
    let tab = &el.children[0];
    assert_eq!(tab.children.len(), 2);
    let ElementKind::Image(img) = &tab.children[0].kind else {
        panic!("first child should be image icon");
    };
    assert_eq!(img.image_key, "icon-folder");
}

#[test]
fn breadcrumbs_inserts_separators_between_items() {
    let id = ElementId::new("bc");
    let a = ElementId::local("a", 0, &id);
    let b = ElementId::local("b", 0, &id);
    let c = ElementId::local("c", 0, &id);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = breadcrumbs(
        id,
        [
            BreadcrumbSpec::new(a, "Home"),
            BreadcrumbSpec::new(b, "Docs"),
            BreadcrumbSpec::new(c, "Page").current(true),
        ],
        &cx,
    );
    // 3 items + 2 separators = 5 children
    assert_eq!(el.children.len(), 5);
    let ElementKind::Text(sep) = &el.children[1].kind else {
        panic!("separator should be text");
    };
    assert_eq!(sep.text, "›");
}

#[test]
fn text_input_shows_placeholder_when_empty() {
    let id = ElementId::new("field");
    let spec = TextInputSpec::new("").placeholder("Enter name");
    let el = text_input_impl(
        id,
        &spec,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let inner = &el.children[0];
    // last child of inner is the text element
    let text_child = inner.children.last().expect("should have text child");
    let ElementKind::Text(text) = &text_child.kind else {
        panic!("should be text");
    };
    assert_eq!(text.text, "Enter name");
    assert_eq!(text.style.color, WidgetPalette::default().muted_text);
}

#[test]
fn text_input_masks_password_value() {
    let id = ElementId::new("field");
    let spec = TextInputSpec::new("secret").password(true);
    let el = text_input_impl(
        id,
        &spec,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let inner = &el.children[0];
    let text_child = inner.children.last().expect("should have text child");
    let ElementKind::Text(text) = &text_child.kind else {
        panic!("should be text");
    };
    assert_eq!(text.text, "••••••");
}

#[test]
fn text_input_focused_with_cursor_adds_cursor_element() {
    let id = ElementId::new("field");
    let spec = TextInputSpec::new("hello").cursor_x(42.0);
    let mut state = WidgetState::default();
    state.focused = true;
    let el = text_input_impl(id, &spec, &state, &WidgetPalette::default());
    let inner = &el.children[0];
    // cursor + text = 2 children when focused
    assert_eq!(inner.children.len(), 2);
    let cursor = &inner.children[0];
    assert_eq!(cursor.layout.width, LayoutSizing::Fixed(2.0));
    assert_eq!(
        cursor.layout.position,
        LayoutPosition::Absolute {
            offset: glam::Vec2::new(42.0, 0.0)
        }
    );
}

#[test]
fn text_input_with_selection_adds_selection_element() {
    let id = ElementId::new("field");
    let spec = TextInputSpec::new("hello world").selection(10.0, 55.0);
    let el = text_input_impl(
        id,
        &spec,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let inner = &el.children[0];
    // selection + text = 2 children (unfocused so no cursor)
    assert_eq!(inner.children.len(), 2);
    let sel = &inner.children[0];
    assert_eq!(sel.layout.width, LayoutSizing::Fixed(45.0));
    assert_eq!(
        sel.layout.position,
        LayoutPosition::Absolute {
            offset: glam::Vec2::new(10.0, 0.0)
        }
    );
}

#[test]
fn text_input_focused_invalid_uses_invalid_outline() {
    let id = ElementId::new("field");
    let spec = TextInputSpec::new("bad value");
    let mut state = WidgetState::default();
    state.focused = true;
    state.invalid = true;
    let el = text_input_impl(id, &spec, &state, &WidgetPalette::default());
    assert_eq!(el.style.outline, WidgetPalette::default().outline_invalid);
}

#[test]
fn select_trigger_shows_chevron_down_when_closed() {
    let id = ElementId::new("sel");
    let el = select_with_palette(
        id,
        "Option A",
        false,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let chevron = &el.children[1];
    let ElementKind::Text(text) = &chevron.kind else {
        panic!("chevron should be text");
    };
    assert_eq!(text.text, "▼");
}

#[test]
fn select_trigger_shows_chevron_up_when_open() {
    let id = ElementId::new("sel");
    let el = select_with_palette(
        id,
        "Option A",
        true,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    let chevron = &el.children[1];
    let ElementKind::Text(text) = &chevron.kind else {
        panic!("chevron should be text");
    };
    assert_eq!(text.text, "▲");
}

#[test]
fn icon_button_contains_image_child() {
    let id = ElementId::new("btn");
    let el = icon_button_with_palette(
        id,
        "icon-gear",
        20.0,
        &WidgetState::default(),
        &WidgetPalette::default(),
    );
    assert_eq!(el.children.len(), 1);
    let ElementKind::Image(img) = &el.children[0].kind else {
        panic!("icon_button child should be image");
    };
    assert_eq!(img.image_key, "icon-gear");
    assert_eq!(img.natural_size, Some(Size::new(20.0, 20.0)));
}

#[test]
fn accordion_panel_closed_has_only_header() {
    let id = ElementId::new("accordion");
    let header_id = ElementId::local("section", 0, &id);
    let config = AccordionPanelConfig::new(header_id, "Section").open(false);
    let content = Element::new(ElementId::new("body"));
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = accordion_panel(id, config, Some(content), &cx);
    // Only the header child (content is suppressed when closed)
    assert_eq!(el.children.len(), 1);
}

#[test]
fn accordion_panel_open_includes_content() {
    let id = ElementId::new("accordion");
    let header_id = ElementId::local("section", 0, &id);
    let config = AccordionPanelConfig::new(header_id, "Section").open(true);
    let content = Element::new(ElementId::new("body"));
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = accordion_panel(id, config, Some(content), &cx);
    assert_eq!(el.children.len(), 2);
}

#[test]
fn dialog_surface_with_title_has_title_bar_and_body() {
    let id = ElementId::new("dialog");
    let el = dialog_surface(id, Some("My Dialog"), Size::new(400.0, 300.0), []);
    assert_eq!(el.children.len(), 2);
    // First child is title bar, second is body
    let title_bar = &el.children[0];
    let title_child = &title_bar.children[0];
    let ElementKind::Text(title) = &title_child.kind else {
        panic!("title should be text");
    };
    assert_eq!(title.text, "My Dialog");
}

#[test]
fn dialog_surface_without_title_has_only_body() {
    let id = ElementId::new("dialog");
    let el = dialog_surface(id, None::<&str>, Size::new(400.0, 300.0), []);
    assert_eq!(el.children.len(), 1);
}

#[test]
fn context_menu_item_separator_adds_top_outline() {
    let id = ElementId::new("item");
    let spec = ContextMenuItemSpec::new(id, "Cut").separator_before(true);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = context_menu_item(spec, 28.0, &cx);
    assert_eq!(el.style.outline_width.top, 1.0);
    assert_eq!(el.style.outline_width.bottom, 0.0);
}

#[test]
fn context_menu_item_with_icon_uses_image_child() {
    let id = ElementId::new("item");
    let spec = ContextMenuItemSpec::new(id, "Open").icon("icon-open");
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = context_menu_item(spec, 28.0, &cx);
    let ElementKind::Image(img) = &el.children[0].kind else {
        panic!("first child should be icon image");
    };
    assert_eq!(img.image_key, "icon-open");
}

#[test]
fn context_menu_item_without_icon_uses_spacer_placeholder() {
    let id = ElementId::new("item");
    let spec = ContextMenuItemSpec::new(id, "Delete");
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = context_menu_item(spec, 28.0, &cx);
    // Without icon, first child should be a container spacer (not image)
    assert!(
        matches!(el.children[0].kind, ElementKind::Container),
        "should be spacer container"
    );
}

#[test]
fn group_box_has_header_and_content() {
    let id = ElementId::new("group");
    let child = Element::new(ElementId::new("row1"));
    let el = group_box(id, "Settings", [child]);
    assert_eq!(el.children.len(), 2);
    // Header has the label
    let header = &el.children[0];
    let ElementKind::Text(label_text) = &header.children[0].kind else {
        panic!("header child should be text label");
    };
    assert_eq!(label_text.text, "Settings");
}

// ── Batch-2 widget tests ───────────────────────────────────────────────────

#[test]
fn list_item_selected_uses_selected_surface() {
    let id = ElementId::new("item");
    let spec = ListItemSpec::new(id, "Project A").selected(true);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = list_item(spec, &cx);
    assert_eq!(
        el.style.background,
        WidgetPalette::default().surface_selected
    );
}

#[test]
fn list_item_with_sublabel_has_two_text_children() {
    let id = ElementId::new("item");
    let spec = ListItemSpec::new(id, "Main").sublabel("subtitle text");
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = list_item(spec, &cx);
    assert_eq!(el.children.len(), 2);
    let ElementKind::Text(label) = &el.children[0].kind else {
        panic!("first child should be text");
    };
    assert_eq!(label.text, "Main");
    let ElementKind::Text(sub) = &el.children[1].kind else {
        panic!("second child should be text");
    };
    assert_eq!(sub.text, "subtitle text");
}

#[test]
fn table_header_row_builds_one_cell_per_spec() {
    let id = ElementId::new("header");
    let a = ElementId::local("col-a", 0, &id);
    let b = ElementId::local("col-b", 0, &id);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = table_header_row(
        id,
        28.0,
        [
            TableHeaderSpec::new(a, "Name", 160.0).sort(SortDirection::Ascending),
            TableHeaderSpec::new(b, "Size", 80.0),
        ],
        &cx,
    );
    assert_eq!(el.children.len(), 2);
    assert_eq!(el.layout.height, LayoutSizing::Fixed(28.0));
    // Sort ascending appends ▲
    let ElementKind::Text(txt) = &el.children[0].children[0].kind else {
        panic!("cell should contain text");
    };
    assert!(txt.text.contains('▲'), "ascending sort should show ▲");
}

#[test]
fn table_header_cell_descending_shows_down_arrow() {
    let id = ElementId::new("col");
    let spec = TableHeaderSpec::new(id, "Date", 100.0).sort(SortDirection::Descending);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let cell = table_header_cell(spec, 28.0, &cx);
    let ElementKind::Text(txt) = &cell.children[0].kind else {
        panic!("cell should contain text");
    };
    assert!(txt.text.contains('▼'));
}

#[test]
fn property_row_has_label_and_value_children() {
    let id = ElementId::new("prop");
    let spec = PropertyRowSpec::new(id.clone(), "Opacity").label_width(100.0);
    let value = Element::new(ElementId::local("value", 0, &id));
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = property_row(spec, value, 32.0, &cx);
    assert_eq!(el.children.len(), 2);
    assert_eq!(el.layout.height, LayoutSizing::Fixed(32.0));
    let ElementKind::Text(label) = &el.children[0].kind else {
        panic!("first child should be text label");
    };
    assert_eq!(label.text, "Opacity");
}

#[test]
fn chip_without_remove_has_one_text_child() {
    let id = ElementId::new("chip");
    let spec = ChipSpec::new(id, "Rust").variant(BadgeVariant::Info);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = chip(spec, &cx);
    assert_eq!(el.children.len(), 1);
    let ElementKind::Text(label) = &el.children[0].kind else {
        panic!("chip child should be text");
    };
    assert_eq!(label.text, "Rust");
}

#[test]
fn chip_with_remove_has_label_and_close_button() {
    let id = ElementId::new("chip");
    let spec = ChipSpec::new(id, "v1.0").can_remove(true);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = chip(spec, &cx);
    assert_eq!(el.children.len(), 2);
}

#[test]
fn notification_accent_bar_and_message_present() {
    let id = ElementId::new("notif");
    let spec = NotificationSpec::new(id, "Build failed", BadgeVariant::Error);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = notification(spec, &cx);
    // accent bar + message = 2 children
    assert_eq!(el.children.len(), 2);
    let msg_child = &el.children[1];
    let ElementKind::Text(txt) = &msg_child.kind else {
        panic!("second child should be message text");
    };
    assert_eq!(txt.text, "Build failed");
}

#[test]
fn notification_with_action_has_three_children() {
    let id = ElementId::new("notif");
    let spec = NotificationSpec::new(id, "Update available", BadgeVariant::Info).action("Install");
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = notification(spec, &cx);
    assert_eq!(el.children.len(), 3);
}

#[test]
fn status_bar_separates_sections_with_dividers() {
    let id = ElementId::new("bar");
    let a = ElementId::local("a", 0, &id);
    let b = ElementId::local("b", 0, &id);
    let c = ElementId::local("c", 0, &id);
    let (sim, palette) = test_cx_and_sim();
    let cx = Cx::new(&sim, palette);
    let el = status_bar(
        id,
        [
            StatusBarSectionSpec::new(a, "Branch").value("main"),
            StatusBarSectionSpec::new(b, "Errors").value("0"),
            StatusBarSectionSpec::new(c, "Ready"),
        ],
        &cx,
    );
    // 3 sections + 2 × (divider + gap) = 3 + 4 = 7 children
    assert_eq!(el.children.len(), 7);
    assert_eq!(el.layout.height, LayoutSizing::Fixed(24.0));
}

#[test]
fn card_without_title_has_only_body() {
    let id = ElementId::new("card");
    let el = card(
        id,
        None::<&str>,
        LayoutSizing::Fixed(200.0),
        LayoutSizing::Fixed(100.0),
        [],
    );
    assert_eq!(el.children.len(), 1);
}

#[test]
fn card_with_title_has_title_bar_and_body() {
    let id = ElementId::new("card");
    let el = card(
        id,
        Some("My Card"),
        LayoutSizing::Fixed(200.0),
        LayoutSizing::Fixed(100.0),
        [],
    );
    assert_eq!(el.children.len(), 2);
    // Title bar's first child has the label
    let title_bar = &el.children[0];
    let ElementKind::Text(txt) = &title_bar.children[0].kind else {
        panic!("title bar child should be text");
    };
    assert_eq!(txt.text, "My Card");
}
