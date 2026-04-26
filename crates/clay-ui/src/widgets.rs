use crate::{
    Axis, CornerSpec, Edges, Element, ElementBuilder, ElementId, ElementStyle, LayoutDirection,
    LayoutInput, LayoutPosition, LayoutSizing, MosaicLayout, Rect, ScrollAxis, ScrollConfig, Size,
    TextStyle, TextWrap, UiColor, UiImageOptions, UiLayer, UiShape, VirtualGridLayout,
    VirtualListLayout, VirtualTableLayout, VirtualTreeLayout, WidgetState, radii_all,
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

#[derive(Clone, Debug, PartialEq)]
pub struct DropdownOptionSpec {
    pub id: ElementId,
    pub label: String,
    pub selected: bool,
    pub disabled: bool,
    pub separator_before: bool,
    pub state: WidgetState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntrySpec {
    pub id: ElementId,
    pub level: LogLevel,
    pub message: String,
    pub timestamp: Option<String>,
    pub source: Option<String>,
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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PortalHostConfig {
    pub size: Size,
    pub z_index: i16,
    pub clip: bool,
    pub transparent_to_input: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModalLayerConfig {
    pub size: Size,
    pub z_index: i16,
    pub backdrop: UiColor,
    pub clip: bool,
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

impl PortalHostConfig {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            z_index: 0,
            clip: true,
            transparent_to_input: true,
        }
    }

    pub fn z_index(mut self, z_index: i16) -> Self {
        self.z_index = z_index;
        self
    }

    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
    }

    pub fn transparent_to_input(mut self, transparent_to_input: bool) -> Self {
        self.transparent_to_input = transparent_to_input;
        self
    }
}

impl ModalLayerConfig {
    pub fn new(size: Size) -> Self {
        Self {
            size,
            z_index: 0,
            backdrop: UiColor::from_rgba8(0, 0, 0, 160),
            clip: true,
        }
    }

    pub fn z_index(mut self, z_index: i16) -> Self {
        self.z_index = z_index;
        self
    }

    pub fn backdrop(mut self, backdrop: UiColor) -> Self {
        self.backdrop = backdrop;
        self
    }

    pub fn clip(mut self, clip: bool) -> Self {
        self.clip = clip;
        self
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

impl DropdownOptionSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            selected: false,
            disabled: false,
            separator_before: false,
            state: WidgetState::default(),
        }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn separator_before(mut self, separator_before: bool) -> Self {
        self.separator_before = separator_before;
        self
    }

    pub fn state(mut self, state: WidgetState) -> Self {
        self.state = state;
        self
    }
}

impl LogLevel {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Trace => "TRACE",
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub fn color(self, palette: &WidgetPalette) -> UiColor {
        match self {
            Self::Trace => palette.muted_text,
            Self::Debug => UiColor::from_rgba8(125, 211, 252, 255),
            Self::Info => palette.text,
            Self::Warn => UiColor::from_rgba8(251, 191, 36, 255),
            Self::Error => UiColor::from_rgba8(248, 113, 113, 255),
        }
    }
}

impl LogEntrySpec {
    pub fn new(id: ElementId, level: LogLevel, message: impl Into<String>) -> Self {
        Self {
            id,
            level,
            message: message.into(),
            timestamp: None,
            source: None,
            state: WidgetState::default(),
        }
    }

    pub fn timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
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

pub fn image(id: ElementId, image_key: impl Into<String>, natural_size: Size) -> Element {
    image_with_options(id, image_key, natural_size, UiImageOptions::default())
}

pub fn image_with_options(
    id: ElementId,
    image_key: impl Into<String>,
    natural_size: Size,
    options: UiImageOptions,
) -> Element {
    let mut element = Element::image(id, image_key);
    if let crate::ElementKind::Image(image) = &mut element.kind {
        image.natural_size = Some(natural_size);
        image.options = options;
    }
    element.layout.width = LayoutSizing::Fixed(natural_size.width.max(0.0));
    element.layout.height = LayoutSizing::Fixed(natural_size.height.max(0.0));
    element
}

pub fn portal_host(
    id: ElementId,
    config: PortalHostConfig,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    let mut builder = ElementBuilder::container(id)
        .style(ElementStyle {
            transparent_to_input: config.transparent_to_input,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(config.size.width.max(0.0)),
            height: LayoutSizing::Fixed(config.size.height.max(0.0)),
            clip_x: config.clip,
            clip_y: config.clip,
            layer: UiLayer::TopLayer,
            z_index: config.z_index,
            ..LayoutInput::default()
        });
    for mut child in children {
        place_subtree_in_layer(
            &mut child,
            UiLayer::TopLayer,
            config.z_index.saturating_add(1),
        );
        builder = builder.child(child);
    }
    builder.build()
}

pub fn modal_layer(
    id: ElementId,
    config: ModalLayerConfig,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    let mut builder = ElementBuilder::container(id)
        .style(ElementStyle {
            background: config.backdrop,
            transparent_to_input: false,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(config.size.width.max(0.0)),
            height: LayoutSizing::Fixed(config.size.height.max(0.0)),
            clip_x: config.clip,
            clip_y: config.clip,
            layer: UiLayer::TopLayer,
            z_index: config.z_index,
            ..LayoutInput::default()
        });
    for mut child in children {
        place_subtree_in_layer(
            &mut child,
            UiLayer::TopLayer,
            config.z_index.saturating_add(1),
        );
        builder = builder.child(child);
    }
    builder.build()
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
    let content_id = ElementId::local("virtual-content", 0, &id);
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

pub fn dropdown_option(
    option: DropdownOptionSpec,
    row_height: f32,
    palette: &WidgetPalette,
) -> Element {
    let mut state = option.state.clone();
    state.disabled |= option.disabled;
    let row_id = option.id.clone();
    let mut style = control_style(
        &state,
        palette,
        option.selected,
        0.0,
        Edges::symmetric(10.0, 0.0),
    );
    style.outline_width = if state.focused {
        Edges::all(1.0)
    } else if option.separator_before {
        Edges {
            top: 1.0,
            ..Edges::ZERO
        }
    } else {
        Edges::ZERO
    };
    style.shape = UiShape::Rect;

    let indicator_style = ElementStyle {
        background: if option.selected && !state.disabled {
            palette.accent
        } else {
            UiColor::TRANSPARENT
        },
        shape: UiShape::Capsule,
        ..ElementStyle::default()
    };
    let mut label = compact_text(
        ElementId::local("label", 0, &row_id),
        option.label,
        text_color(&state, palette, option.selected),
        14.0,
        18.0,
    );
    label.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };

    ElementBuilder::container(option.id)
        .style(style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(row_height.max(0.0)),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 8.0,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(ElementId::local("selected-indicator", 0, &row_id))
                .style(indicator_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(4.0),
                    height: LayoutSizing::Fixed(16.0),
                    ..LayoutInput::default()
                })
                .build(),
        )
        .child(label)
        .build()
}

pub fn virtual_dropdown_menu(
    id: ElementId,
    width: LayoutSizing,
    layout: &VirtualListLayout,
    visible_options: impl IntoIterator<Item = DropdownOptionSpec>,
) -> Element {
    virtual_dropdown_menu_with_palette(
        id,
        width,
        layout,
        visible_options,
        &WidgetPalette::default(),
    )
}

pub fn virtual_dropdown_menu_with_palette(
    id: ElementId,
    width: LayoutSizing,
    layout: &VirtualListLayout,
    visible_options: impl IntoIterator<Item = DropdownOptionSpec>,
    palette: &WidgetPalette,
) -> Element {
    let rows = visible_options
        .into_iter()
        .map(|option| dropdown_option(option, layout.item_extent, palette));
    let mut menu = virtual_list(id, width, layout.viewport_extent, layout, rows);
    menu.style = ElementStyle {
        background: palette.surface,
        outline: palette.outline,
        outline_width: Edges::all(1.0),
        corner_radius: radii_all(8.0),
        padding: Edges::ZERO,
        ..ElementStyle::default()
    };
    menu.layout.clip_x = true;
    menu
}

pub fn log_entry(entry: LogEntrySpec, row_height: f32, palette: &WidgetPalette) -> Element {
    let row_id = entry.id.clone();
    let focused = entry.state.focused;
    let mut style = ElementStyle {
        background: if entry.state.hovered || entry.state.focused {
            palette.surface_hovered
        } else {
            UiColor::TRANSPARENT
        },
        outline: palette.outline.with_alpha(0.45),
        outline_width: Edges {
            bottom: 1.0,
            ..Edges::ZERO
        },
        padding: Edges::symmetric(8.0, 0.0),
        ..ElementStyle::default()
    };
    if focused {
        style.outline = palette.outline_focus;
        style.outline_width = Edges::all(1.0);
    }

    let mut level = monospace_text(
        ElementId::local("level", 0, &row_id),
        entry.level.label(),
        entry.level.color(palette),
    );
    level.layout.width = LayoutSizing::Fixed(52.0);

    let mut timestamp = monospace_text(
        ElementId::local("timestamp", 0, &row_id),
        entry.timestamp.unwrap_or_default(),
        palette.muted_text,
    );
    timestamp.layout.width = LayoutSizing::Fixed(82.0);

    let mut source = monospace_text(
        ElementId::local("source", 0, &row_id),
        entry.source.unwrap_or_default(),
        palette.muted_text,
    );
    source.layout.width = LayoutSizing::Fixed(96.0);

    let mut message = monospace_text(
        ElementId::local("message", 0, &row_id),
        entry.message,
        palette.text,
    );
    message.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };

    ElementBuilder::container(entry.id)
        .style(style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(row_height.max(0.0)),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 8.0,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(level)
        .child(timestamp)
        .child(source)
        .child(message)
        .build()
}

pub fn virtual_log_viewer(
    id: ElementId,
    width: LayoutSizing,
    layout: &VirtualListLayout,
    visible_entries: impl IntoIterator<Item = LogEntrySpec>,
) -> Element {
    virtual_log_viewer_with_palette(
        id,
        width,
        layout,
        visible_entries,
        &WidgetPalette::default(),
    )
}

pub fn virtual_log_viewer_with_palette(
    id: ElementId,
    width: LayoutSizing,
    layout: &VirtualListLayout,
    visible_entries: impl IntoIterator<Item = LogEntrySpec>,
    palette: &WidgetPalette,
) -> Element {
    let rows = visible_entries
        .into_iter()
        .map(|entry| log_entry(entry, layout.item_extent, palette));
    let mut viewer = virtual_list(id, width, layout.viewport_extent, layout, rows);
    viewer.style = ElementStyle {
        background: palette.surface.with_alpha(0.82),
        outline: palette.outline,
        outline_width: Edges::all(1.0),
        corner_radius: radii_all(6.0),
        ..ElementStyle::default()
    };
    viewer.layout.clip_x = true;
    viewer
}

pub fn virtual_grid(
    id: ElementId,
    viewport_size: Vec2,
    layout: &VirtualGridLayout,
    visible_items: impl IntoIterator<Item = Element>,
) -> Element {
    let content_id = ElementId::local("grid-content", 0, &id);
    let mut content = ElementBuilder::container(content_id.clone()).layout(LayoutInput {
        width: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        scroll_offset: -layout.scroll_offset,
        ..LayoutInput::default()
    });

    if layout.before_rows_extent > 0.0 {
        content = content.child(spacer(
            ElementId::local("before-rows-spacer", 0, &content_id),
            LayoutSizing::Fixed(layout.content_size.x),
            layout.before_rows_extent,
        ));
    }

    let mut item_iter = visible_items.into_iter();
    for row in layout.render_rows.clone() {
        let row_id = ElementId::local("row", row as u32, &content_id);
        let mut row_builder = ElementBuilder::container(row_id.clone()).layout(LayoutInput {
            width: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(layout.item_size.y),
            direction: LayoutDirection::LeftToRight,
            ..LayoutInput::default()
        });

        if layout.before_columns_extent > 0.0 {
            row_builder = row_builder.child(horizontal_spacer(
                ElementId::local("before-columns-spacer", 0, &row_id),
                layout.before_columns_extent,
                layout.item_size.y,
            ));
        }

        for column in layout.render_columns.clone() {
            let index = row * layout.column_count + column;
            if index >= layout.item_count {
                continue;
            }
            if let Some(item) = item_iter.next() {
                row_builder = row_builder.child(item);
            }
        }

        if layout.after_columns_extent > 0.0 {
            row_builder = row_builder.child(horizontal_spacer(
                ElementId::local("after-columns-spacer", 0, &row_id),
                layout.after_columns_extent,
                layout.item_size.y,
            ));
        }

        content = content.child(row_builder.build());
    }

    if layout.after_rows_extent > 0.0 {
        content = content.child(spacer(
            ElementId::local("after-rows-spacer", 0, &content_id),
            LayoutSizing::Fixed(layout.content_size.x),
            layout.after_rows_extent,
        ));
    }

    scroll_container(
        id,
        LayoutSizing::Fixed(viewport_size.x.max(0.0)),
        LayoutSizing::Fixed(viewport_size.y.max(0.0)),
        ScrollAxis::Both,
        layout.scroll_offset,
        [content.build()],
    )
}

pub fn virtual_table(
    id: ElementId,
    viewport_size: Vec2,
    layout: &VirtualTableLayout,
    visible_cells: impl IntoIterator<Item = Element>,
) -> Element {
    let content_id = ElementId::local("table-content", 0, &id);
    let mut content = ElementBuilder::container(content_id.clone()).layout(LayoutInput {
        width: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        scroll_offset: -layout.scroll_offset,
        ..LayoutInput::default()
    });

    if layout.before_rows_extent > 0.0 {
        content = content.child(spacer(
            ElementId::local("before-rows-spacer", 0, &content_id),
            LayoutSizing::Fixed(layout.content_size.x),
            layout.before_rows_extent,
        ));
    }

    let mut cell_iter = visible_cells.into_iter();
    for row in layout.render_rows.clone() {
        let row_id = ElementId::local("row", row as u32, &content_id);
        let mut row_builder = ElementBuilder::container(row_id.clone()).layout(LayoutInput {
            width: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(layout.cell_size.y),
            direction: LayoutDirection::LeftToRight,
            ..LayoutInput::default()
        });

        if layout.before_columns_extent > 0.0 {
            row_builder = row_builder.child(horizontal_spacer(
                ElementId::local("before-columns-spacer", 0, &row_id),
                layout.before_columns_extent,
                layout.cell_size.y,
            ));
        }

        for column in layout.render_columns.clone() {
            if row >= layout.row_count || column >= layout.column_count {
                continue;
            }
            if let Some(cell) = cell_iter.next() {
                row_builder = row_builder.child(cell);
            }
        }

        if layout.after_columns_extent > 0.0 {
            row_builder = row_builder.child(horizontal_spacer(
                ElementId::local("after-columns-spacer", 0, &row_id),
                layout.after_columns_extent,
                layout.cell_size.y,
            ));
        }

        content = content.child(row_builder.build());
    }

    if layout.after_rows_extent > 0.0 {
        content = content.child(spacer(
            ElementId::local("after-rows-spacer", 0, &content_id),
            LayoutSizing::Fixed(layout.content_size.x),
            layout.after_rows_extent,
        ));
    }

    scroll_container(
        id,
        LayoutSizing::Fixed(viewport_size.x.max(0.0)),
        LayoutSizing::Fixed(viewport_size.y.max(0.0)),
        ScrollAxis::Both,
        layout.scroll_offset,
        [content.build()],
    )
}

pub fn virtual_tree(
    id: ElementId,
    width: LayoutSizing,
    viewport_height: f32,
    layout: &VirtualTreeLayout,
    visible_rows: impl IntoIterator<Item = Element>,
) -> Element {
    let content_id = ElementId::local("tree-content", 0, &id);
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
            ElementId::local("before-rows-spacer", 0, &content_id),
            width,
            layout.before_extent,
        ));
    }

    for row in visible_rows {
        content = content.child(row);
    }

    if layout.after_extent > 0.0 {
        content = content.child(spacer(
            ElementId::local("after-rows-spacer", 0, &content_id),
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

pub fn mosaic_container(
    id: ElementId,
    layout: &MosaicLayout,
    tiles: impl IntoIterator<Item = Element>,
) -> Element {
    mosaic_content_from_tile_rects(
        id,
        layout.content_size,
        layout
            .tiles
            .iter()
            .map(|tile_layout| tile_layout.rect)
            .zip(tiles),
    )
}

pub fn virtual_mosaic(
    id: ElementId,
    layout: &MosaicLayout,
    viewport_size: Vec2,
    scroll_offset: Vec2,
    overscan: f32,
    visible_tiles: impl IntoIterator<Item = Element>,
) -> Element {
    let viewport_size = viewport_size.max(Vec2::ZERO);
    let scroll_config =
        ScrollConfig::new(viewport_size, layout.content_size.to_vec2()).axis(ScrollAxis::Both);
    let scroll_offset = scroll_config.clamp_offset(scroll_offset);
    let viewport = Rect::new(
        scroll_offset.x,
        scroll_offset.y,
        viewport_size.x,
        viewport_size.y,
    );
    let content_id = ElementId::local("mosaic-content", 0, &id);
    let content = mosaic_content_from_tile_rects(
        content_id,
        layout.content_size,
        layout
            .visible_tiles(viewport, overscan.max(0.0))
            .map(|tile_layout| tile_layout.rect)
            .zip(visible_tiles),
    );

    scroll_container(
        id,
        LayoutSizing::Fixed(viewport_size.x),
        LayoutSizing::Fixed(viewport_size.y),
        ScrollAxis::Both,
        scroll_offset,
        [content],
    )
}

fn mosaic_content_from_tile_rects(
    id: ElementId,
    content_size: Size,
    tiles: impl IntoIterator<Item = (Rect, Element)>,
) -> Element {
    let mut builder = ElementBuilder::container(id).layout(LayoutInput {
        width: LayoutSizing::Fixed(content_size.width),
        height: LayoutSizing::Fixed(content_size.height),
        ..LayoutInput::default()
    });

    for (tile_rect, mut tile) in tiles {
        tile.layout.width = LayoutSizing::Fixed(tile_rect.size.width);
        tile.layout.height = LayoutSizing::Fixed(tile_rect.size.height);
        tile.layout.position = LayoutPosition::Absolute {
            offset: tile_rect.origin,
        };
        builder = builder.child(tile);
    }

    builder.build()
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

fn horizontal_spacer(id: ElementId, width: f32, height: f32) -> Element {
    ElementBuilder::container(id)
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(width.max(0.0)),
            height: LayoutSizing::Fixed(height.max(0.0)),
            ..LayoutInput::default()
        })
        .build()
}

fn compact_text(
    id: ElementId,
    text: impl Into<String>,
    color: UiColor,
    font_size: f32,
    line_height: f32,
) -> Element {
    let style = TextStyle {
        font_size,
        line_height,
        color,
        wrap: TextWrap::None,
        ..TextStyle::default()
    };
    ElementBuilder::text(id, text, style).build()
}

fn monospace_text(id: ElementId, text: impl Into<String>, color: UiColor) -> Element {
    let style = TextStyle {
        font_size: 12.0,
        line_height: 16.0,
        color,
        wrap: TextWrap::None,
        family_candidates: vec!["monospace".into()],
        ..TextStyle::default()
    };
    ElementBuilder::text(id, text, style).build()
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

fn place_subtree_in_layer(element: &mut Element, layer: UiLayer, base_z_index: i16) {
    let z_index = base_z_index.saturating_add(element.layout.z_index);
    element.layout.layer = layer;
    element.layout.z_index = z_index;
    for child in &mut element.children {
        place_subtree_in_layer(child, layer, z_index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ElementKind, LayoutCache, LayoutTree, MosaicConfig, MosaicTileMode, MosaicTileSpec, Size,
        UiAntialiasing, UiImageFit, UiImageSampling, VirtualGridConfig, VirtualListConfig,
        VirtualTableConfig, VirtualTreeConfig,
    };

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

        let element = virtual_dropdown_menu(id, LayoutSizing::Fixed(180.0), &layout, options);

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

        let element = virtual_log_viewer(id, LayoutSizing::Fixed(360.0), &layout, entries);

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
