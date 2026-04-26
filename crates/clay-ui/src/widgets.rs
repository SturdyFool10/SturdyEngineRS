use crate::{
    Axis, Edges, Element, ElementBuilder, ElementId, ElementStyle, LayoutDirection, LayoutInput,
    LayoutSizing, ScrollAxis, TextStyle, TextWrap, UiColor, VirtualListLayout, WidgetState,
    radii_all,
};
use glam::Vec2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WidgetPalette {
    pub text: UiColor,
    pub muted_text: UiColor,
    pub surface: UiColor,
    pub surface_hovered: UiColor,
    pub surface_pressed: UiColor,
    pub surface_selected: UiColor,
    pub surface_disabled: UiColor,
    pub outline: UiColor,
    pub outline_focus: UiColor,
    pub outline_invalid: UiColor,
    pub accent: UiColor,
    pub accent_text: UiColor,
}

impl Default for WidgetPalette {
    fn default() -> Self {
        Self {
            text: UiColor::from_rgba8(226, 232, 240, 255),
            muted_text: UiColor::from_rgba8(148, 163, 184, 255),
            surface: UiColor::from_rgba8(15, 23, 42, 255),
            surface_hovered: UiColor::from_rgba8(30, 41, 59, 255),
            surface_pressed: UiColor::from_rgba8(51, 65, 85, 255),
            surface_selected: UiColor::from_rgba8(37, 99, 235, 255),
            surface_disabled: UiColor::from_rgba8(24, 31, 43, 180),
            outline: UiColor::from_rgba8(148, 163, 184, 80),
            outline_focus: UiColor::from_rgba8(96, 165, 250, 255),
            outline_invalid: UiColor::from_rgba8(248, 113, 113, 255),
            accent: UiColor::from_rgba8(59, 130, 246, 255),
            accent_text: UiColor::WHITE,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DragBarAxis {
    Horizontal,
    Vertical,
}

impl From<Axis> for DragBarAxis {
    fn from(value: Axis) -> Self {
        match value {
            Axis::Horizontal => Self::Horizontal,
            Axis::Vertical => Self::Vertical,
        }
    }
}

pub fn button(id: ElementId, label: impl Into<String>, state: &WidgetState) -> Element {
    button_with_palette(id, label, state, &WidgetPalette::default())
}

pub fn button_with_palette(
    id: ElementId,
    label: impl Into<String>,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let mut style = control_style(state, palette, false, 8.0, Edges::symmetric(12.0, 7.0));
    style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });

    ElementBuilder::container(id.clone())
        .style(style)
        .layout(LayoutInput {
            width: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            ..LayoutInput::default()
        })
        .child(label_element(
            ElementId::local("label", 0, &id),
            label,
            text_color(state, palette, false),
        ))
        .build()
}

pub fn radio(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
) -> Element {
    radio_with_palette(id, label, checked, state, &WidgetPalette::default())
}

pub fn radio_with_palette(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let mut indicator_style = control_style(state, palette, checked, 999.0, Edges::ZERO);
    indicator_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });
    let dot_id = ElementId::local("dot", 0, &id);

    ElementBuilder::container(id.clone())
        .layout(LayoutInput {
            width: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::LeftToRight,
            gap: 8.0,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(dot_id)
                .style(indicator_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(16.0),
                    height: LayoutSizing::Fixed(16.0),
                    ..LayoutInput::default()
                })
                .build(),
        )
        .child(label_element(
            ElementId::local("label", 0, &id),
            label,
            text_color(state, palette, checked),
        ))
        .build()
}

pub fn toggle(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
) -> Element {
    toggle_with_palette(id, label, checked, state, &WidgetPalette::default())
}

pub fn toggle_with_palette(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let track_id = ElementId::local("track", 0, &id);
    let knob_id = ElementId::local("knob", 0, &track_id);
    let mut track_style = control_style(state, palette, checked, 999.0, Edges::all(2.0));
    track_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });
    let knob_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent_text
        },
        corner_radius: radii_all(999.0),
        ..ElementStyle::default()
    };

    ElementBuilder::container(id.clone())
        .layout(LayoutInput {
            width: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::LeftToRight,
            gap: 8.0,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(track_id)
                .style(track_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(36.0),
                    height: LayoutSizing::Fixed(20.0),
                    direction: LayoutDirection::LeftToRight,
                    align_x: if checked {
                        crate::Align::End
                    } else {
                        crate::Align::Start
                    },
                    ..LayoutInput::default()
                })
                .child(
                    ElementBuilder::container(knob_id)
                        .style(knob_style)
                        .layout(LayoutInput {
                            width: LayoutSizing::Fixed(16.0),
                            height: LayoutSizing::Fixed(16.0),
                            ..LayoutInput::default()
                        })
                        .build(),
                )
                .build(),
        )
        .child(label_element(
            ElementId::local("label", 0, &id),
            label,
            text_color(state, palette, checked),
        ))
        .build()
}

pub fn drag_bar(id: ElementId, axis: impl Into<DragBarAxis>, state: &WidgetState) -> Element {
    drag_bar_with_palette(id, axis, state, &WidgetPalette::default())
}

pub fn drag_bar_with_palette(
    id: ElementId,
    axis: impl Into<DragBarAxis>,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let axis = axis.into();
    let style = control_style(state, palette, false, 999.0, Edges::ZERO);
    let (width, height) = match axis {
        DragBarAxis::Horizontal => (
            LayoutSizing::Grow {
                min: 16.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Fixed(6.0),
        ),
        DragBarAxis::Vertical => (
            LayoutSizing::Fixed(6.0),
            LayoutSizing::Grow {
                min: 16.0,
                max: f32::INFINITY,
            },
        ),
    };

    ElementBuilder::container(id)
        .style(style)
        .layout(LayoutInput {
            width,
            height,
            ..LayoutInput::default()
        })
        .build()
}

pub fn scroll_container(
    id: ElementId,
    width: LayoutSizing,
    height: LayoutSizing,
    axis: ScrollAxis,
    scroll_offset: Vec2,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    scroll_container_with_direction(
        id,
        width,
        height,
        axis,
        scroll_offset,
        LayoutDirection::TopToBottom,
        children,
    )
}

pub fn scroll_container_with_direction(
    id: ElementId,
    width: LayoutSizing,
    height: LayoutSizing,
    axis: ScrollAxis,
    scroll_offset: Vec2,
    direction: LayoutDirection,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    let content_id = ElementId::local("content", 0, &id);
    let content_offset = match axis {
        ScrollAxis::Vertical => Vec2::new(0.0, -scroll_offset.y),
        ScrollAxis::Horizontal => Vec2::new(-scroll_offset.x, 0.0),
        ScrollAxis::Both => -scroll_offset,
    };
    let mut content = ElementBuilder::container(content_id).layout(LayoutInput {
        width,
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction,
        scroll_offset: content_offset,
        ..LayoutInput::default()
    });
    for child in children {
        content = content.child(child);
    }

    ElementBuilder::container(id)
        .layout(LayoutInput {
            width,
            height,
            clip_x: matches!(axis, ScrollAxis::Horizontal | ScrollAxis::Both),
            clip_y: matches!(axis, ScrollAxis::Vertical | ScrollAxis::Both),
            ..LayoutInput::default()
        })
        .child(content.build())
        .build()
}

pub fn virtual_list(
    id: ElementId,
    width: LayoutSizing,
    viewport_height: f32,
    layout: &VirtualListLayout,
    visible_items: impl IntoIterator<Item = Element>,
) -> Element {
    let content_id = ElementId::local("content", 0, &id);
    let mut content = ElementBuilder::container(content_id.clone()).layout(LayoutInput {
        width,
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        scroll_offset: Vec2::new(0.0, -layout.scroll_offset),
        ..LayoutInput::default()
    });

    if layout.before_extent > 0.0 {
        content = content.child(spacer(
            ElementId::local("before-spacer", 0, &content_id),
            width,
            layout.before_extent,
        ));
    }

    for item in visible_items {
        content = content.child(item);
    }

    if layout.after_extent > 0.0 {
        content = content.child(spacer(
            ElementId::local("after-spacer", 0, &content_id),
            width,
            layout.after_extent,
        ));
    }

    scroll_container(
        id,
        width,
        LayoutSizing::Fixed(viewport_height.max(0.0)),
        ScrollAxis::Vertical,
        Vec2::new(0.0, layout.scroll_offset),
        [content.build()],
    )
}

fn label_element(id: ElementId, label: impl Into<String>, color: UiColor) -> Element {
    let style = TextStyle {
        font_size: 14.0,
        line_height: 18.0,
        color,
        wrap: TextWrap::None,
        ..TextStyle::default()
    };
    ElementBuilder::text(id, label, style).build()
}

fn spacer(id: ElementId, width: LayoutSizing, height: f32) -> Element {
    ElementBuilder::container(id)
        .layout(LayoutInput {
            width,
            height: LayoutSizing::Fixed(height.max(0.0)),
            ..LayoutInput::default()
        })
        .build()
}

fn control_style(
    state: &WidgetState,
    palette: &WidgetPalette,
    selected: bool,
    radius: f32,
    padding: Edges,
) -> ElementStyle {
    let background = if state.disabled {
        palette.surface_disabled
    } else if selected {
        palette.surface_selected
    } else if state.pressed || state.captured {
        palette.surface_pressed
    } else if state.hovered || state.focused {
        palette.surface_hovered
    } else {
        palette.surface
    };
    let outline = if state.invalid {
        palette.outline_invalid
    } else if state.focused {
        palette.outline_focus
    } else {
        palette.outline
    };

    ElementStyle {
        background,
        outline,
        outline_width: Edges::all(1.0),
        corner_radius: radii_all(radius),
        padding,
        ..ElementStyle::default()
    }
}

fn text_color(state: &WidgetState, palette: &WidgetPalette, selected: bool) -> UiColor {
    if state.disabled {
        palette.muted_text
    } else if selected {
        palette.accent_text
    } else {
        palette.text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ElementKind, LayoutCache, LayoutTree, Size, VirtualListConfig};

    #[test]
    fn button_builder_marks_label_as_nowrap() {
        let id = ElementId::new("button");
        let element = button(id.clone(), "Run", &WidgetState::default());

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
        let element = toggle(id, "Enabled", true, &WidgetState::default());
        let track = &element.children[0];

        assert_eq!(track.layout.align_x, crate::Align::End);
        assert_eq!(track.children.len(), 1);
    }

    #[test]
    fn drag_bar_has_stable_axis_dimensions() {
        let id = ElementId::new("drag");
        let element = drag_bar(id, DragBarAxis::Vertical, &WidgetState::default());

        assert_eq!(element.layout.width, LayoutSizing::Fixed(6.0));
        assert!(matches!(element.layout.height, LayoutSizing::Grow { .. }));
    }

    #[test]
    fn widget_builders_produce_layoutable_trees() {
        let root = button(ElementId::new("button"), "Layout", &WidgetState::default());

        let layout =
            LayoutTree::compute(&root, Size::new(200.0, 40.0), &mut LayoutCache::default())
                .unwrap();

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
}
