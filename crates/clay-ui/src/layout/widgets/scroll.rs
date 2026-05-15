use crate::{
    Axis, Edges, Element, ElementBuilder, ElementId, ElementStyle, LayoutDirection, LayoutInput,
    LayoutSizing, ScrollAxis, ScrollConfig, WidgetState, radii_all,
};
use glam::Vec2;

use super::{WidgetPalette, WidgetRenderContext, control_style};

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

pub fn scrollbar<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    metrics: ScrollbarMetrics,
    cx: &C,
) -> Element {
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    scrollbar_with_palette(id, metrics, &state, &palette)
}

pub(crate) fn scrollbar_with_palette(
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
            .child(scrollbar_with_palette(
                vertical_bar_id,
                vertical_metrics,
                &WidgetState::default(),
                &WidgetPalette::default(),
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
            .child(scrollbar_with_palette(
                horizontal_bar_id,
                horizontal_metrics,
                &WidgetState::default(),
                &WidgetPalette::default(),
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
                .child(scrollbar_with_palette(
                    vertical_bar_id,
                    vertical_metrics,
                    &WidgetState::default(),
                    &WidgetPalette::default(),
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
                .child(scrollbar_with_palette(
                    horizontal_bar_id,
                    horizontal_metrics,
                    &WidgetState::default(),
                    &WidgetPalette::default(),
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
