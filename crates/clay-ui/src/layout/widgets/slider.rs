use crate::{
    Axis, Edges, Element, ElementBuilder, ElementId, ElementStyle, LayoutDirection, LayoutInput,
    LayoutPosition, LayoutSizing, SliderConfig, WidgetState, radii_all,
};
use glam::Vec2;

use super::{WidgetPalette, WidgetRenderContext, control_style};

#[derive(Clone, Debug, PartialEq)]
pub struct SliderStyle {
    pub track_extent: Option<f32>,
    pub track_cross_extent: f32,
    pub thumb_size: f32,
    pub fill_inset: f32,
}

impl Default for SliderStyle {
    fn default() -> Self {
        Self {
            track_extent: None,
            track_cross_extent: 20.0,
            thumb_size: 16.0,
            fill_inset: 2.0,
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

pub fn drag_bar<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    axis: impl Into<DragBarAxis>,
    cx: &C,
) -> Element {
    let axis_enum: DragBarAxis = axis.into();
    let crate_axis = match axis_enum {
        DragBarAxis::Horizontal => Axis::Horizontal,
        DragBarAxis::Vertical => Axis::Vertical,
    };
    cx.register_drag_bar_widget(id.clone(), crate_axis);
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    drag_bar_with_palette(id, axis_enum, &state, &palette)
}

pub(crate) fn drag_bar_with_palette(
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

pub fn slider<C, S>(id: ElementId, axis: impl Into<DragBarAxis>, config: S, cx: &C) -> Element
where
    C: WidgetRenderContext + ?Sized,
    S: Into<SliderConfig>,
{
    slider_styled(id, axis, config, SliderStyle::default(), cx)
}

pub fn slider_styled<C, S>(
    id: ElementId,
    axis: impl Into<DragBarAxis>,
    config: S,
    style: SliderStyle,
    cx: &C,
) -> Element
where
    C: WidgetRenderContext + ?Sized,
    S: Into<SliderConfig>,
{
    let axis_enum: DragBarAxis = axis.into();
    let crate_axis = match axis_enum {
        DragBarAxis::Horizontal => Axis::Horizontal,
        DragBarAxis::Vertical => Axis::Vertical,
    };
    let mut config = config.into();
    let track_extent = style.track_extent.unwrap_or(config.track_extent).max(20.0);
    let thumb_radius = style.thumb_size.max(1.0) * 0.5;
    config = config.track_extent(track_extent).thumb_radius(thumb_radius);
    cx.register_slider_widget(id.clone(), crate_axis, config);
    let value = cx.slider_display_value(&id, config);
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    slider_with_palette_and_style(id, axis_enum, value, track_extent, &style, &state, &palette)
}

#[cfg(test)]
pub(crate) fn slider_with_palette(
    id: ElementId,
    axis: impl Into<DragBarAxis>,
    value: f32,
    track_extent: f32,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    slider_with_palette_and_style(
        id,
        axis,
        value,
        track_extent,
        &SliderStyle::default(),
        state,
        palette,
    )
}

pub(crate) fn slider_with_palette_and_style(
    id: ElementId,
    axis: impl Into<DragBarAxis>,
    value: f32,
    track_extent: f32,
    style: &SliderStyle,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let axis = axis.into();
    let value = value.clamp(0.0, 1.0);
    let track_extent = track_extent.max(20.0);
    let thumb_size = style.thumb_size.max(1.0);
    let thumb_radius = thumb_size * 0.5;
    let track_cross_extent = style.track_cross_extent.max(thumb_size);
    let fill_inset = style.fill_inset.max(0.0).min(track_cross_extent * 0.5);
    let fill_id = ElementId::local("fill", 0, &id);
    let thumb_id = ElementId::local("thumb", 0, &id);

    // Fill and thumb are absolute so their displayed position is tied directly
    // to the same final track rect used by input.rs.
    let mut track_style = control_style(state, palette, false, 999.0, Edges::ZERO);
    track_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });

    let fill_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent
        },
        corner_radius: radii_all(999.0),
        transparent_to_input: true,
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
        transparent_to_input: true,
        ..ElementStyle::default()
    };

    let travel = (track_extent - thumb_radius * 2.0).max(0.0);
    let thumb_center = thumb_radius + value * travel;
    let (width, height, direction, fill_width, fill_height, fill_pos, thumb_pos) = match axis {
        DragBarAxis::Horizontal => (
            LayoutSizing::Fixed(track_extent),
            LayoutSizing::Fixed(track_cross_extent),
            LayoutDirection::LeftToRight,
            LayoutSizing::Fixed((thumb_center - fill_inset).max(0.0)),
            LayoutSizing::Fixed((track_cross_extent - fill_inset * 2.0).max(0.0)),
            LayoutPosition::Absolute {
                offset: Vec2::new(fill_inset, fill_inset),
            },
            LayoutPosition::Absolute {
                offset: Vec2::new(
                    thumb_center - thumb_radius,
                    (track_cross_extent - thumb_size) * 0.5,
                ),
            },
        ),
        DragBarAxis::Vertical => (
            LayoutSizing::Fixed(track_cross_extent),
            LayoutSizing::Fixed(track_extent),
            LayoutDirection::TopToBottom,
            LayoutSizing::Fixed((track_cross_extent - fill_inset * 2.0).max(0.0)),
            LayoutSizing::Fixed((thumb_center - fill_inset).max(0.0)),
            LayoutPosition::Absolute {
                offset: Vec2::new(fill_inset, fill_inset),
            },
            LayoutPosition::Absolute {
                offset: Vec2::new(
                    (track_cross_extent - thumb_size) * 0.5,
                    thumb_center - thumb_radius,
                ),
            },
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
                    position: fill_pos,
                    ..LayoutInput::default()
                })
                .build(),
        )
        .child(
            ElementBuilder::container(thumb_id)
                .style(thumb_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(thumb_size),
                    height: LayoutSizing::Fixed(thumb_size),
                    position: thumb_pos,
                    ..LayoutInput::default()
                })
                .build(),
        )
        .build()
}

pub fn progress_bar<C: WidgetRenderContext + ?Sized>(id: ElementId, value: f32, cx: &C) -> Element {
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    progress_bar_with_palette(id, value, &state, &palette)
}

pub(crate) fn progress_bar_with_palette(
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
