use crate::{
    ColorSpaceKind, Edges, Element, ElementBuilder, ElementId, ElementStyle, LayoutDirection,
    LayoutInput, LayoutPosition, LayoutSizing, WidgetBehavior, WidgetState, radii_all,
};
use glam::Vec2;

use super::{
    CheckboxStyle, RadioStyle, SegmentSpec, ToggleAnimConfig, ToggleStyle, WidgetPalette,
    WidgetRenderContext, control_style, label_element, segment_shape, text_color,
};

pub fn button<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    label: impl Into<String>,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    button_with_palette(id, label, &state, &palette)
}

pub(crate) fn button_with_palette(
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

pub fn radio<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    cx: &C,
) -> Element {
    radio_styled(id, label, checked, RadioStyle::default(), cx)
}

pub fn radio_styled<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    style: RadioStyle,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    radio_with_palette_and_style(id, label, checked, &style, &state, &palette)
}

#[cfg(test)]
pub(crate) fn radio_with_palette(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    radio_with_palette_and_style(id, label, checked, &RadioStyle::default(), state, palette)
}

pub(crate) fn radio_with_palette_and_style(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    style: &RadioStyle,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    // The indicator uses the user's `id` so that activation events are
    // associated with the id the caller registered.  The outer row container
    // is transparent so clicking the label text does not activate the widget.
    let outer_id = ElementId::local("outer", 0, &id);
    let mut indicator_style = control_style(state, palette, checked, 999.0, Edges::ZERO);
    indicator_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });

    ElementBuilder::container(outer_id)
        .style(ElementStyle {
            transparent_to_input: true,
            ..ElementStyle::default()
        })
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
            gap: style.label_gap,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(id.clone())
                .style(indicator_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(style.indicator_size.max(1.0)),
                    height: LayoutSizing::Fixed(style.indicator_size.max(1.0)),
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

pub fn checkbox<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    cx: &C,
) -> Element {
    checkbox_styled(id, label, checked, CheckboxStyle::default(), cx)
}

pub fn checkbox_styled<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    style: CheckboxStyle,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    checkbox_with_palette_and_style(id, label, checked, &style, &state, &palette)
}

#[cfg(test)]
pub(crate) fn checkbox_with_palette(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    checkbox_with_palette_and_style(
        id,
        label,
        checked,
        &CheckboxStyle::default(),
        state,
        palette,
    )
}

pub(crate) fn checkbox_with_palette_and_style(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    style: &CheckboxStyle,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    // The box indicator uses the user's `id` so activation events match the
    // registered id.  The outer row is transparent so the label does not
    // receive click events.
    let outer_id = ElementId::local("outer", 0, &id);
    let mark_id = ElementId::local("mark", 0, &id);
    let mut box_style = control_style(
        state,
        palette,
        checked,
        style.indicator_radius,
        style.indicator_padding,
    );
    box_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });
    let mark_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent_text
        },
        corner_radius: radii_all(style.mark_radius),
        transparent_to_input: true,
        ..ElementStyle::default()
    };

    let mut box_builder = ElementBuilder::container(id.clone())
        .style(box_style)
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(style.indicator_size.max(1.0)),
            height: LayoutSizing::Fixed(style.indicator_size.max(1.0)),
            ..LayoutInput::default()
        });
    if checked {
        box_builder = box_builder.child(
            ElementBuilder::container(mark_id)
                .style(mark_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(style.mark_size.max(0.0)),
                    height: LayoutSizing::Fixed(style.mark_size.max(0.0)),
                    ..LayoutInput::default()
                })
                .build(),
        );
    }

    ElementBuilder::container(outer_id)
        .style(ElementStyle {
            transparent_to_input: true,
            ..ElementStyle::default()
        })
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
            gap: style.label_gap,
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

pub fn toggle<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    anim: ToggleAnimConfig,
    cx: &C,
) -> Element {
    toggle_styled(id, label, checked, anim, ToggleStyle::default(), cx)
}

pub fn toggle_styled<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    label: impl Into<String>,
    checked: bool,
    anim: ToggleAnimConfig,
    style: ToggleStyle,
    cx: &C,
) -> Element {
    // The pill indicator uses the user's `id` so that activation events are
    // associated with the registered id.
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    let target = if checked { 1.0 } else { 0.0 };
    let progress = cx.advance_toggle_animation(&id, target, anim);
    toggle_with_palette_in_space_and_style(
        id,
        label,
        progress,
        anim.color_space,
        &style,
        &state,
        &palette,
    )
}

#[cfg(test)]
pub(crate) fn toggle_with_palette(
    id: ElementId,
    label: impl Into<String>,
    progress: f32,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    toggle_with_palette_in_space_and_style(
        id,
        label,
        progress,
        ColorSpaceKind::LinearSrgb,
        &ToggleStyle::default(),
        state,
        palette,
    )
}

pub(crate) fn toggle_with_palette_in_space_and_style(
    id: ElementId,
    label: impl Into<String>,
    progress: f32,
    color_space: ColorSpaceKind,
    style: &ToggleStyle,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    // `id` IS the pill/track indicator.  The outer row container is transparent
    // so clicking the label does not activate the widget.
    let outer_id = ElementId::local("outer", 0, &id);
    let knob_id = ElementId::local("knob", 0, &id);
    let progress = progress.clamp(0.0, 1.0);

    let track_width = style.track_width.max(1.0);
    let track_height = style.track_height.max(1.0);
    let knob_size = style.knob_size.max(1.0);
    let track_padding = style
        .track_padding
        .max(0.0)
        .min((track_width.min(track_height) - knob_size).max(0.0) * 0.5);
    let knob_travel = (track_width - track_padding * 2.0 - knob_size).max(0.0);
    // The knob is positioned with `LayoutPosition::Absolute`, whose offset is
    // relative to the *content rect* of the track (i.e. already inset by the
    // track's padding).  Adding `track_padding` here would double-count it,
    // leaving a visible gap at the top and clipping the knob on the far side.
    let knob_offset_x = progress * knob_travel;

    let off_style = control_style(state, palette, false, 999.0, Edges::all(track_padding));
    let on_style = control_style(state, palette, true, 999.0, Edges::all(track_padding));
    let mut track_style = off_style.clone();
    track_style.background =
        off_style
            .background
            .mix_in_space(on_style.background, progress as f64, color_space);
    track_style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });
    let knob_style = ElementStyle {
        background: if state.disabled {
            palette.muted_text
        } else {
            palette.accent_text
        },
        corner_radius: radii_all(999.0),
        transparent_to_input: true,
        ..ElementStyle::default()
    };

    ElementBuilder::container(outer_id)
        .style(ElementStyle {
            transparent_to_input: true,
            ..ElementStyle::default()
        })
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
            gap: style.label_gap,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(id.clone())
                .style(track_style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(track_width),
                    height: LayoutSizing::Fixed(track_height),
                    clip_x: true,
                    clip_y: true,
                    ..LayoutInput::default()
                })
                .child(
                    ElementBuilder::container(knob_id)
                        .style(knob_style)
                        .layout(LayoutInput {
                            width: LayoutSizing::Fixed(knob_size),
                            height: LayoutSizing::Fixed(knob_size),
                            position: LayoutPosition::Absolute {
                                offset: Vec2::new(knob_offset_x, 0.0),
                            },
                            ..LayoutInput::default()
                        })
                        .build(),
                )
                .build(),
        )
        .child(label_element(
            ElementId::local("label", 0, &id),
            label,
            text_color(state, palette, progress >= 0.5),
        ))
        .build()
}

pub fn segmented_control<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    segments: impl IntoIterator<Item = SegmentSpec>,
    cx: &C,
) -> Element {
    let segments = segments.into_iter().collect::<Vec<_>>();
    let count = segments.len();
    let palette = cx.widget_palette();
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
        cx.register_widget_behavior(segment.id.clone(), WidgetBehavior::interactive());
        let state = segment
            .state_override
            .clone()
            .unwrap_or_else(|| cx.widget_state(&segment.id));
        let mut style = control_style(
            &state,
            &palette,
            segment.selected,
            0.0,
            Edges::symmetric(10.0, 6.0),
        );
        style.shape = segment_shape(index, count, 7.0);
        style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });

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
                    text_color(&state, &palette, segment.selected),
                ))
                .build(),
        );
    }

    builder.build()
}
