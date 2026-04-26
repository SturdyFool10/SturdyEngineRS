use crate::{
    Axis, CornerSpec, Edges, Element, ElementBuilder, ElementId, ElementStyle, LayoutDirection,
    LayoutInput, LayoutSizing, ScrollAxis, ScrollConfig, TextStyle, TextWrap, UiColor, UiShape,
    VirtualListLayout, WidgetState, radii_all,
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

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentSpec {
    pub id: ElementId,
    pub label: String,
    pub selected: bool,
    pub state: WidgetState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollbarMetrics {
    pub axis: Axis,
    pub viewport_extent: f32,
    pub content_extent: f32,
    pub offset: f32,
    pub max_offset: f32,
    pub track_extent: f32,
    pub thumb_extent: f32,
    pub thumb_offset: f32,
}

impl ScrollbarMetrics {
    pub fn new(
        axis: Axis,
        viewport: Vec2,
        content: Vec2,
        offset: Vec2,
        track_extent: f32,
        min_thumb_extent: f32,
    ) -> Self {
        let (viewport_extent, content_extent, offset) = match axis {
            Axis::Horizontal => (viewport.x, content.x, offset.x),
            Axis::Vertical => (viewport.y, content.y, offset.y),
        };
        let viewport_extent = viewport_extent.max(0.0);
        let content_extent = content_extent.max(0.0);
        let track_extent = track_extent.max(0.0);
        let max_offset = (content_extent - viewport_extent).max(0.0);
        let offset = offset.clamp(0.0, max_offset);
        let visible = max_offset > 0.0 && track_extent > 0.0 && content_extent > 0.0;
        let thumb_extent = if visible {
            let ratio = (viewport_extent / content_extent).clamp(0.0, 1.0);
            (track_extent * ratio).clamp(min_thumb_extent.max(0.0).min(track_extent), track_extent)
        } else {
            track_extent
        };
        let thumb_travel = (track_extent - thumb_extent).max(0.0);
        let thumb_offset = if max_offset > 0.0 {
            thumb_travel * (offset / max_offset)
        } else {
            0.0
        };

        Self {
            axis,
            viewport_extent,
            content_extent,
            offset,
            max_offset,
            track_extent,
            thumb_extent,
            thumb_offset,
        }
    }

    pub fn from_config(axis: Axis, config: ScrollConfig, offset: Vec2, track_extent: f32) -> Self {
        Self::new(
            axis,
            config.viewport,
            config.content,
            offset,
            track_extent,
            18.0,
        )
    }

    pub fn visible(self) -> bool {
        self.max_offset > 0.0 && self.track_extent > 0.0 && self.content_extent > 0.0
    }
}

impl SegmentSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            selected: false,
            state: WidgetState::default(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn state(mut self, state: WidgetState) -> Self {
        self.state = state;
        self
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

pub fn checkbox(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
) -> Element {
    checkbox_with_palette(id, label, checked, state, &WidgetPalette::default())
}

pub fn checkbox_with_palette(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let box_id = ElementId::local("box", 0, &id);
    let mark_id = ElementId::local("mark", 0, &box_id);
    let mut box_style = control_style(state, palette, checked, 4.0, Edges::all(3.0));
    box_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });
    let mark_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent_text
        },
        corner_radius: radii_all(2.0),
        ..ElementStyle::default()
    };

    let mut box_builder = ElementBuilder::container(box_id)
        .style(box_style)
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(16.0),
            height: LayoutSizing::Fixed(16.0),
            ..LayoutInput::default()
        });
    if checked {
        box_builder = box_builder.child(
            ElementBuilder::container(mark_id)
                .style(mark_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(10.0),
                    height: LayoutSizing::Fixed(10.0),
                    ..LayoutInput::default()
                })
                .build(),
        );
    }

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
        .child(box_builder.build())
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

pub fn segmented_control(
    id: ElementId,
    segments: impl IntoIterator<Item = SegmentSpec>,
) -> Element {
    segmented_control_with_palette(id, segments, &WidgetPalette::default())
}

pub fn segmented_control_with_palette(
    id: ElementId,
    segments: impl IntoIterator<Item = SegmentSpec>,
    palette: &WidgetPalette,
) -> Element {
    let segments = segments.into_iter().collect::<Vec<_>>();
    let count = segments.len();
    let mut builder = ElementBuilder::container(id).layout(LayoutInput {
        width: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::LeftToRight,
        ..LayoutInput::default()
    });

    for (index, segment) in segments.into_iter().enumerate() {
        let mut style = control_style(
            &segment.state,
            palette,
            segment.selected,
            0.0,
            Edges::symmetric(10.0, 6.0),
        );
        style.shape = segment_shape(index, count, 7.0);
        style.outline_width = Edges::all(if segment.state.focused { 2.0 } else { 1.0 });

        builder = builder.child(
            ElementBuilder::container(segment.id.clone())
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
                    ElementId::local("label", 0, &segment.id),
                    segment.label,
                    text_color(&segment.state, palette, segment.selected),
                ))
                .build(),
        );
    }

    builder.build()
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

pub fn slider(
    id: ElementId,
    axis: impl Into<DragBarAxis>,
    value: f32,
    state: &WidgetState,
) -> Element {
    slider_with_palette(id, axis, value, state, &WidgetPalette::default())
}

pub fn slider_with_palette(
    id: ElementId,
    axis: impl Into<DragBarAxis>,
    value: f32,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let axis = axis.into();
    let value = value.clamp(0.0, 1.0);
    let fill_id = ElementId::local("fill", 0, &id);
    let thumb_id = ElementId::local("thumb", 0, &id);
    let mut track_style = control_style(state, palette, false, 999.0, Edges::all(2.0));
    track_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });
    let fill_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent
        },
        corner_radius: radii_all(999.0),
        ..ElementStyle::default()
    };
    let thumb_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent_text
        },
        outline: palette.outline,
        outline_width: Edges::all(1.0),
        corner_radius: radii_all(999.0),
        ..ElementStyle::default()
    };
    let (width, height, direction, fill_width, fill_height, thumb_width, thumb_height) = match axis
    {
        DragBarAxis::Horizontal => (
            LayoutSizing::Grow {
                min: 64.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Fixed(20.0),
            LayoutDirection::LeftToRight,
            LayoutSizing::Percent(value),
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Fixed(16.0),
            LayoutSizing::Fixed(16.0),
        ),
        DragBarAxis::Vertical => (
            LayoutSizing::Fixed(20.0),
            LayoutSizing::Grow {
                min: 64.0,
                max: f32::INFINITY,
            },
            LayoutDirection::TopToBottom,
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Percent(value),
            LayoutSizing::Fixed(16.0),
            LayoutSizing::Fixed(16.0),
        ),
    };

    ElementBuilder::container(id)
        .style(track_style)
        .layout(LayoutInput {
            width,
            height,
            direction,
            clip_x: true,
            clip_y: true,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(fill_id)
                .style(fill_style)
                .layout(LayoutInput {
                    width: fill_width,
                    height: fill_height,
                    ..LayoutInput::default()
                })
                .build(),
        )
        .child(
            ElementBuilder::container(thumb_id)
                .style(thumb_style)
                .layout(LayoutInput {
                    width: thumb_width,
                    height: thumb_height,
                    ..LayoutInput::default()
                })
                .build(),
        )
        .build()
}

pub fn progress_bar(id: ElementId, value: f32, state: &WidgetState) -> Element {
    progress_bar_with_palette(id, value, state, &WidgetPalette::default())
}

pub fn progress_bar_with_palette(
    id: ElementId,
    value: f32,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let value = value.clamp(0.0, 1.0);
    let fill_id = ElementId::local("fill", 0, &id);
    let mut track_style = control_style(state, palette, false, 999.0, Edges::all(2.0));
    track_style.outline_width = Edges::ZERO;
    let fill_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent
        },
        corner_radius: radii_all(999.0),
        ..ElementStyle::default()
    };

    ElementBuilder::container(id)
        .style(track_style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 64.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(8.0),
            direction: LayoutDirection::LeftToRight,
            clip_x: true,
            clip_y: true,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(fill_id)
                .style(fill_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Percent(value),
                    height: LayoutSizing::Grow {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    ..LayoutInput::default()
                })
                .build(),
        )
        .build()
}

pub fn scrollbar(id: ElementId, metrics: ScrollbarMetrics, state: &WidgetState) -> Element {
    scrollbar_with_palette(id, metrics, state, &WidgetPalette::default())
}

pub fn scrollbar_with_palette(
    id: ElementId,
    metrics: ScrollbarMetrics,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let spacer_id = ElementId::local("before-thumb", 0, &id);
    let thumb_id = ElementId::local("thumb", 0, &id);
    let mut track_style = control_style(state, palette, false, 999.0, Edges::ZERO);
    track_style.outline_width = Edges::ZERO;
    track_style.background = if metrics.visible() {
        palette.surface.with_alpha(0.55)
    } else {
        palette.surface.with_alpha(0.22)
    };
    let thumb_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else if state.pressed || state.captured {
            palette.surface_pressed
        } else if state.hovered || state.focused {
            palette.surface_hovered
        } else {
            palette.outline
        },
        corner_radius: radii_all(999.0),
        ..ElementStyle::default()
    };
    let thickness = 8.0;
    let (
        track_width,
        track_height,
        direction,
        spacer_width,
        spacer_height,
        thumb_width,
        thumb_height,
    ) = match metrics.axis {
        Axis::Horizontal => (
            LayoutSizing::Fixed(metrics.track_extent),
            LayoutSizing::Fixed(thickness),
            LayoutDirection::LeftToRight,
            LayoutSizing::Fixed(metrics.thumb_offset),
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Fixed(metrics.thumb_extent),
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
        ),
        Axis::Vertical => (
            LayoutSizing::Fixed(thickness),
            LayoutSizing::Fixed(metrics.track_extent),
            LayoutDirection::TopToBottom,
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Fixed(metrics.thumb_offset),
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Fixed(metrics.thumb_extent),
        ),
    };

    ElementBuilder::container(id)
        .style(track_style)
        .layout(LayoutInput {
            width: track_width,
            height: track_height,
            direction,
            clip_x: true,
            clip_y: true,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(spacer_id)
                .layout(LayoutInput {
                    width: spacer_width,
                    height: spacer_height,
                    ..LayoutInput::default()
                })
                .build(),
        )
        .child(
            ElementBuilder::container(thumb_id)
                .style(thumb_style)
                .layout(LayoutInput {
                    width: thumb_width,
                    height: thumb_height,
                    ..LayoutInput::default()
                })
                .build(),
        )
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

pub fn scroll_container_with_scrollbars(
    id: ElementId,
    width: LayoutSizing,
    height: LayoutSizing,
    config: ScrollConfig,
    scroll_offset: Vec2,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    scroll_container_with_scrollbars_and_direction(
        id,
        width,
        height,
        config,
        scroll_offset,
        LayoutDirection::TopToBottom,
        children,
    )
}

pub fn scroll_container_with_scrollbars_and_direction(
    id: ElementId,
    width: LayoutSizing,
    height: LayoutSizing,
    config: ScrollConfig,
    scroll_offset: Vec2,
    direction: LayoutDirection,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    let viewport_id = ElementId::local("viewport", 0, &id);
    let vertical_bar_id = ElementId::local("vertical-scrollbar", 0, &id);
    let horizontal_bar_id = ElementId::local("horizontal-scrollbar", 0, &id);
    let row_id = ElementId::local("viewport-row", 0, &id);
    let scroll_offset = config.clamp_offset(scroll_offset);
    let viewport = scroll_container_with_direction(
        viewport_id,
        width,
        height,
        config.axis,
        scroll_offset,
        direction,
        children,
    );
    let vertical_metrics =
        ScrollbarMetrics::from_config(Axis::Vertical, config, scroll_offset, config.viewport.y);
    let horizontal_metrics =
        ScrollbarMetrics::from_config(Axis::Horizontal, config, scroll_offset, config.viewport.x);

    match config.axis {
        ScrollAxis::Vertical => ElementBuilder::container(id)
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
                gap: 4.0,
                ..LayoutInput::default()
            })
            .child(viewport)
            .child(scrollbar(
                vertical_bar_id,
                vertical_metrics,
                &WidgetState::default(),
            ))
            .build(),
        ScrollAxis::Horizontal => ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 4.0,
                ..LayoutInput::default()
            })
            .child(viewport)
            .child(scrollbar(
                horizontal_bar_id,
                horizontal_metrics,
                &WidgetState::default(),
            ))
            .build(),
        ScrollAxis::Both => {
            let row = ElementBuilder::container(row_id)
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
                    gap: 4.0,
                    ..LayoutInput::default()
                })
                .child(viewport)
                .child(scrollbar(
                    vertical_bar_id,
                    vertical_metrics,
                    &WidgetState::default(),
                ))
                .build();

            ElementBuilder::container(id)
                .layout(LayoutInput {
                    width: LayoutSizing::Fit {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    height: LayoutSizing::Fit {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    direction: LayoutDirection::TopToBottom,
                    gap: 4.0,
                    ..LayoutInput::default()
                })
                .child(row)
                .child(scrollbar(
                    horizontal_bar_id,
                    horizontal_metrics,
                    &WidgetState::default(),
                ))
                .build()
        }
    }
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

fn segment_shape(index: usize, count: usize, radius: f32) -> UiShape {
    if count <= 1 {
        return UiShape::rounded_rect(radii_all(radius));
    }

    let rounded = CornerSpec::round(radius);
    let square = CornerSpec::round(0.0);
    if index == 0 {
        UiShape::independent_corners(rounded, square, square, rounded)
    } else if index + 1 == count {
        UiShape::independent_corners(square, rounded, rounded, square)
    } else {
        UiShape::Rect
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
    fn checked_checkbox_adds_check_mark() {
        let id = ElementId::new("checkbox");
        let element = checkbox(id, "Accept", true, &WidgetState::default());
        let box_element = &element.children[0];

        assert_eq!(box_element.children.len(), 1);
        assert_eq!(box_element.layout.width, LayoutSizing::Fixed(16.0));
        assert_eq!(
            box_element.children[0].layout.width,
            LayoutSizing::Fixed(10.0)
        );
    }

    #[test]
    fn segmented_control_shapes_outer_segments() {
        let id = ElementId::new("segments");
        let element = segmented_control(
            id.clone(),
            [
                SegmentSpec::new(ElementId::local("one", 0, &id), "One"),
                SegmentSpec::new(ElementId::local("two", 0, &id), "Two").selected(true),
                SegmentSpec::new(ElementId::local("three", 0, &id), "Three"),
            ],
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
        let element = drag_bar(id, DragBarAxis::Vertical, &WidgetState::default());

        assert_eq!(element.layout.width, LayoutSizing::Fixed(6.0));
        assert!(matches!(element.layout.height, LayoutSizing::Grow { .. }));
    }

    #[test]
    fn slider_clamps_value_into_fill_percent() {
        let id = ElementId::new("slider");
        let element = slider(id, DragBarAxis::Horizontal, 2.0, &WidgetState::default());

        assert_eq!(element.children[0].layout.width, LayoutSizing::Percent(1.0));
        assert!(element.layout.clip_x);
        assert!(element.layout.clip_y);
    }

    #[test]
    fn progress_bar_uses_clamped_fill_percent() {
        let id = ElementId::new("progress");
        let element = progress_bar(id, -1.0, &WidgetState::default());

        assert_eq!(element.children[0].layout.width, LayoutSizing::Percent(0.0));
        assert_eq!(element.layout.height, LayoutSizing::Fixed(8.0));
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
        let element = scrollbar(
            ElementId::new("scrollbar"),
            metrics,
            &WidgetState::default(),
        );

        assert_eq!(element.layout.width, LayoutSizing::Fixed(90.0));
        assert_eq!(element.layout.height, LayoutSizing::Fixed(8.0));
        assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(30.0));
        assert_eq!(element.children[1].layout.width, LayoutSizing::Fixed(30.0));
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

    #[test]
    fn scroll_container_with_scrollbars_adds_viewport_and_bars() {
        let id = ElementId::new("scrollbars");
        let child = Element::new(ElementId::local("child", 0, &id));
        let element = scroll_container_with_scrollbars(
            id,
            LayoutSizing::Fixed(100.0),
            LayoutSizing::Fixed(80.0),
            ScrollConfig::new(Vec2::new(100.0, 80.0), Vec2::new(220.0, 240.0))
                .axis(ScrollAxis::Both),
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
}
