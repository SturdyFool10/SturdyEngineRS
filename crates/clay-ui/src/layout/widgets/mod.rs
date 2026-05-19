mod overlays;
mod palette;
mod scroll;
mod selection;
mod slider;
mod types;

pub use overlays::{
    modal_layer, portal_host, tooltip_layer, tooltip_layer_with_palette, tooltip_surface,
};
pub use palette::*;
pub use scroll::{
    ScrollbarMetrics, scroll_container, scroll_container_with_direction,
    scroll_container_with_scrollbars, scroll_container_with_scrollbars_and_direction, scrollbar,
};
pub use selection::{
    button, checkbox, checkbox_styled, radio, radio_styled, segmented_control, toggle,
    toggle_styled,
};
pub use slider::{DragBarAxis, SliderStyle, drag_bar, progress_bar, slider, slider_styled};
pub use types::*;

#[cfg(test)]
pub(crate) use scroll::scrollbar_with_palette;
#[cfg(test)]
pub(crate) use selection::{
    button_with_palette, checkbox_with_palette_and_style, radio_with_palette_and_style,
    toggle_with_palette_in_space_and_style,
};
#[cfg(test)]
pub(crate) use selection::{checkbox_with_palette, radio_with_palette, toggle_with_palette};
#[cfg(test)]
pub(crate) use slider::{
    drag_bar_with_palette, progress_bar_with_palette, slider_with_palette,
    slider_with_palette_and_style,
};

use crate::{
    Axis, CornerSpec, Cx, Edges, Element, ElementBuilder, ElementId, ElementStyle, LayoutDirection,
    LayoutInput, LayoutPosition, LayoutSizing, MosaicLayout, Rect, ScrollAxis, ScrollConfig, Size,
    TextAlign, TextStyle, TextWrap, UiColor, UiImageOptions, UiShape, VirtualGridLayout,
    VirtualListLayout, VirtualTableLayout, VirtualTreeLayout, WidgetBehavior, WidgetState,
    radii_all,
};
use glam::Vec2;

// ── Widget builders ────────────────────────────────────────────────────────────

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

pub fn dropdown_option<C: WidgetRenderContext + ?Sized>(
    option: DropdownOptionSpec,
    row_height: f32,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(option.id.clone(), WidgetBehavior::interactive());
    let mut state = cx.widget_state(&option.id);
    state.disabled |= option.disabled;
    let palette = cx.widget_palette();
    let palette = &palette;
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

pub fn virtual_dropdown_menu<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    width: LayoutSizing,
    layout: &VirtualListLayout,
    visible_options: impl IntoIterator<Item = DropdownOptionSpec>,
    cx: &C,
) -> Element {
    let rows = visible_options
        .into_iter()
        .map(|option| dropdown_option(option, layout.item_extent, cx));
    let mut menu = virtual_list(id, width, layout.viewport_extent, layout, rows);
    menu.style = ElementStyle {
        background: cx.widget_palette().surface,
        outline: cx.widget_palette().outline,
        outline_width: Edges::all(1.0),
        corner_radius: radii_all(8.0),
        padding: Edges::ZERO,
        ..ElementStyle::default()
    };
    menu.layout.clip_x = true;
    menu
}

pub fn log_entry<C: WidgetRenderContext + ?Sized>(
    entry: LogEntrySpec,
    row_height: f32,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(entry.id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&entry.id);
    let palette = cx.widget_palette();
    let palette = &palette;
    let row_id = entry.id.clone();
    let focused = state.focused;
    let mut style = ElementStyle {
        background: if state.hovered || state.focused {
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

pub fn virtual_log_viewer<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    width: LayoutSizing,
    layout: &VirtualListLayout,
    visible_entries: impl IntoIterator<Item = LogEntrySpec>,
    cx: &C,
) -> Element {
    let rows = visible_entries
        .into_iter()
        .map(|entry| log_entry(entry, layout.item_extent, cx));
    let mut viewer = virtual_list(id, width, layout.viewport_extent, layout, rows);
    let palette = cx.widget_palette();
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

// ── New public widgets ─────────────────────────────────────────────────────────

pub fn label<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    text: impl Into<String>,
    cx: &C,
) -> Element {
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    compact_text(id, text, text_color(&state, &palette, false), 14.0, 18.0)
}

pub fn label_with_palette(
    id: ElementId,
    text: impl Into<String>,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    compact_text(id, text, text_color(state, palette, false), 14.0, 18.0)
}

pub fn divider(id: ElementId, axis: Axis) -> Element {
    let (width, height) = match axis {
        Axis::Horizontal => (
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            LayoutSizing::Fixed(1.0),
        ),
        Axis::Vertical => (
            LayoutSizing::Fixed(1.0),
            LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
        ),
    };
    ElementBuilder::container(id)
        .style(ElementStyle {
            background: UiColor::from_rgba8(148, 163, 184, 40),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width,
            height,
            ..LayoutInput::default()
        })
        .build()
}

pub fn badge(id: ElementId, text: impl Into<String>, variant: BadgeVariant) -> Element {
    badge_with_palette(id, text, variant, &WidgetPalette::default())
}

pub fn badge_with_palette(
    id: ElementId,
    text: impl Into<String>,
    variant: BadgeVariant,
    palette: &WidgetPalette,
) -> Element {
    let (bg, fg) = variant.colors(palette);
    let label_id = ElementId::local("label", 0, &id);
    ElementBuilder::container(id)
        .style(ElementStyle {
            background: bg,
            corner_radius: radii_all(999.0),
            padding: Edges::symmetric(6.0, 2.0),
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
            ..LayoutInput::default()
        })
        .child(compact_text(label_id, text, fg, 11.0, 14.0))
        .build()
}

pub fn empty_state(
    id: ElementId,
    title: impl Into<String>,
    description: Option<impl Into<String>>,
    width: f32,
    height: f32,
) -> Element {
    empty_state_with_palette(
        id,
        title,
        description,
        width,
        height,
        &WidgetPalette::default(),
    )
}

pub fn empty_state_with_palette(
    id: ElementId,
    title: impl Into<String>,
    description: Option<impl Into<String>>,
    width: f32,
    height: f32,
    palette: &WidgetPalette,
) -> Element {
    let title_id = ElementId::local("title", 0, &id);
    let desc_id = ElementId::local("description", 0, &id);
    let title_el = ElementBuilder::text(
        title_id,
        title,
        TextStyle {
            font_size: 16.0,
            line_height: 22.0,
            color: palette.text,
            align: TextAlign::Center,
            wrap: TextWrap::Words,
            ..TextStyle::default()
        },
    )
    .layout(LayoutInput {
        width: LayoutSizing::Fixed((width - 32.0).max(0.0)),
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        ..LayoutInput::default()
    })
    .build();

    let mut builder = ElementBuilder::container(id)
        .style(ElementStyle {
            transparent_to_input: true,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(width.max(0.0)),
            height: LayoutSizing::Fixed(height.max(0.0)),
            direction: LayoutDirection::TopToBottom,
            align_x: crate::Align::Center,
            align_y: crate::Align::Center,
            gap: 8.0,
            ..LayoutInput::default()
        })
        .child(title_el);

    if let Some(desc) = description {
        let desc_el = ElementBuilder::text(
            desc_id,
            desc,
            TextStyle {
                font_size: 13.0,
                line_height: 18.0,
                color: palette.muted_text,
                align: TextAlign::Center,
                wrap: TextWrap::Words,
                ..TextStyle::default()
            },
        )
        .layout(LayoutInput {
            width: LayoutSizing::Fixed((width - 48.0).max(0.0)),
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            ..LayoutInput::default()
        })
        .build();
        builder = builder.child(desc_el);
    }

    builder.build()
}

pub fn tab_bar<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    tabs: impl IntoIterator<Item = TabSpec>,
    cx: &C,
) -> Element {
    let mut builder = ElementBuilder::container(id).layout(LayoutInput {
        width: LayoutSizing::Grow {
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

    let palette = cx.widget_palette();
    let palette = &palette;
    for tab in tabs {
        let tab_id = tab.id.clone();
        cx.register_widget_behavior(tab.id.clone(), WidgetBehavior::interactive());
        let state = tab
            .state_override
            .clone()
            .unwrap_or_else(|| cx.widget_state(&tab.id));
        let mut tab_style = ElementStyle {
            background: if state.pressed || state.captured {
                palette.surface_pressed
            } else if state.hovered {
                palette.surface_hovered
            } else {
                UiColor::TRANSPARENT
            },
            outline: if tab.selected {
                palette.accent
            } else if state.focused {
                palette.outline_focus
            } else {
                UiColor::TRANSPARENT
            },
            outline_width: Edges {
                bottom: 2.0,
                ..Edges::ZERO
            },
            padding: Edges::symmetric(12.0, 8.0),
            ..ElementStyle::default()
        };
        if state.disabled {
            tab_style.background = UiColor::TRANSPARENT;
        }

        let label_color = if state.disabled {
            palette.muted_text
        } else if tab.selected {
            palette.accent
        } else {
            palette.text
        };

        let mut tab_builder = ElementBuilder::container(tab_id.clone())
            .style(tab_style)
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
                align_y: crate::Align::Center,
                gap: 6.0,
                ..LayoutInput::default()
            });

        if let Some(icon_key) = &tab.icon_key {
            tab_builder = tab_builder.child(icon_element(
                ElementId::local("icon", 0, &tab_id),
                icon_key,
                tab.icon_size,
            ));
        }

        tab_builder = tab_builder.child(compact_text(
            ElementId::local("label", 0, &tab_id),
            tab.label,
            label_color,
            14.0,
            18.0,
        ));

        builder = builder.child(tab_builder.build());
    }

    builder.build()
}

pub fn breadcrumbs<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    items: impl IntoIterator<Item = BreadcrumbSpec>,
    cx: &C,
) -> Element {
    let items: Vec<BreadcrumbSpec> = items.into_iter().collect();
    let count = items.len();
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
        align_y: crate::Align::Center,
        gap: 4.0,
        ..LayoutInput::default()
    });

    let palette = cx.widget_palette();
    for (index, item) in items.into_iter().enumerate() {
        let item_id = item.id.clone();
        cx.register_widget_behavior(item.id.clone(), WidgetBehavior::interactive());
        let state = cx.widget_state(&item.id);
        let color = if item.is_current {
            palette.text
        } else if state.hovered {
            palette.accent
        } else {
            palette.muted_text
        };
        builder = builder.child(compact_text(item_id.clone(), item.label, color, 13.0, 16.0));

        if index + 1 < count {
            let sep_id = ElementId::local("sep", 0, &item_id);
            builder = builder.child(compact_text(sep_id, "›", palette.muted_text, 13.0, 16.0));
        }
    }

    builder.build()
}

pub fn text_input<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    spec: &TextInputSpec,
    cx: &C,
) -> Element {
    cx.register_text_input_widget(id.clone());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    text_input_impl(id, spec, &state, &palette)
}

pub(crate) fn text_input_impl(
    id: ElementId,
    spec: &TextInputSpec,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let padding = Edges::symmetric(10.0, 6.0);
    let field_height = 32.0;
    let content_height = field_height - padding.vertical();
    let field_style = input_field_container_style(state, palette);

    let display_value: String = if spec.password && !spec.value.is_empty() {
        "•".repeat(spec.value.chars().count())
    } else {
        spec.value.clone()
    };
    let show_placeholder = display_value.is_empty();

    let text_color_val = if show_placeholder {
        palette.muted_text
    } else {
        palette.text
    };
    let display_text = if show_placeholder {
        spec.placeholder.clone()
    } else {
        display_value
    };

    let text_id = ElementId::local("text", 0, &id);
    let text_el = ElementBuilder::text(
        text_id,
        display_text,
        TextStyle {
            font_size: 14.0,
            line_height: content_height,
            color: text_color_val,
            wrap: if spec.multiline {
                TextWrap::Words
            } else {
                TextWrap::None
            },
            ..TextStyle::default()
        },
    )
    .layout(LayoutInput {
        width: LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Fixed(content_height),
        ..LayoutInput::default()
    })
    .build();

    let mut inner =
        ElementBuilder::container(ElementId::local("inner", 0, &id)).layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(content_height),
            direction: LayoutDirection::LeftToRight,
            ..LayoutInput::default()
        });

    if let Some((sel_start, sel_end)) = spec.selection {
        let sel_start = sel_start.max(0.0);
        let sel_end = sel_end.max(sel_start);
        if sel_end > sel_start {
            inner = inner.child(selection_element(
                ElementId::local("selection", 0, &id),
                sel_start,
                sel_end - sel_start,
                content_height,
                palette.accent.with_alpha(0.35),
            ));
        }
    }

    if state.focused {
        if let Some(cursor_x) = spec.cursor_x {
            inner = inner.child(cursor_element(
                ElementId::local("cursor", 0, &id),
                cursor_x,
                content_height,
                palette.accent,
            ));
        }
    }

    inner = inner.child(text_el);

    let height = if spec.multiline {
        LayoutSizing::Fit {
            min: field_height,
            max: f32::INFINITY,
        }
    } else {
        LayoutSizing::Fixed(field_height)
    };

    ElementBuilder::container(id)
        .style(field_style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 64.0,
                max: f32::INFINITY,
            },
            height,
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            clip_x: true,
            clip_y: spec.multiline,
            ..LayoutInput::default()
        })
        .child(inner.build())
        .build()
}

pub fn number_input<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    spec: &NumberInputSpec,
    cx: &C,
) -> Element {
    cx.register_text_input_widget(id.clone());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    number_input_impl(id, spec, &state, &palette)
}

pub(crate) fn number_input_impl(
    id: ElementId,
    spec: &NumberInputSpec,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let padding = Edges::symmetric(10.0, 6.0);
    let field_height = 32.0;
    let content_height = field_height - padding.vertical();
    let field_style = input_field_container_style(state, palette);

    let show_placeholder = spec.value.is_empty();
    let display_text = if show_placeholder {
        spec.placeholder.clone()
    } else {
        spec.value.clone()
    };
    let text_color_val = if show_placeholder {
        palette.muted_text
    } else {
        palette.text
    };

    let text_id = ElementId::local("text", 0, &id);
    let mut text_el = compact_text(text_id, display_text, text_color_val, 14.0, content_height);
    text_el.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };
    text_el.layout.height = LayoutSizing::Fixed(content_height);

    let mut inner =
        ElementBuilder::container(ElementId::local("inner", 0, &id)).layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(content_height),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 4.0,
            ..LayoutInput::default()
        });

    if let Some((sel_start, sel_end)) = spec.selection {
        let sel_start = sel_start.max(0.0);
        let sel_end = sel_end.max(sel_start);
        if sel_end > sel_start {
            inner = inner.child(selection_element(
                ElementId::local("selection", 0, &id),
                sel_start,
                sel_end - sel_start,
                content_height,
                palette.accent.with_alpha(0.35),
            ));
        }
    }

    if state.focused {
        if let Some(cursor_x) = spec.cursor_x {
            inner = inner.child(cursor_element(
                ElementId::local("cursor", 0, &id),
                cursor_x,
                content_height,
                palette.accent,
            ));
        }
    }

    inner = inner.child(text_el);

    if let Some(unit) = &spec.unit {
        let unit_id = ElementId::local("unit", 0, &id);
        inner = inner.child(compact_text(
            unit_id,
            unit,
            palette.muted_text,
            12.0,
            content_height,
        ));
    }

    let stepper_id = ElementId::local("stepper", 0, &id);
    let up_id = ElementId::local("up", 0, &stepper_id);
    let down_id = ElementId::local("down", 0, &stepper_id);
    let stepper = ElementBuilder::container(stepper_id)
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(20.0),
            height: LayoutSizing::Fixed(content_height),
            direction: LayoutDirection::TopToBottom,
            ..LayoutInput::default()
        })
        .child(
            ElementBuilder::container(up_id)
                .style(ElementStyle {
                    background: palette.surface_hovered,
                    corner_radius: radii_all(3.0),
                    ..ElementStyle::default()
                })
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(18.0),
                    height: LayoutSizing::Fixed(content_height * 0.5 - 1.0),
                    align_x: crate::Align::Center,
                    align_y: crate::Align::Center,
                    ..LayoutInput::default()
                })
                .child(compact_text(
                    ElementId::local("arrow", 0, &id),
                    "▲",
                    palette.muted_text,
                    8.0,
                    10.0,
                ))
                .build(),
        )
        .child(
            ElementBuilder::container(down_id)
                .style(ElementStyle {
                    background: palette.surface_hovered,
                    corner_radius: radii_all(3.0),
                    ..ElementStyle::default()
                })
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(18.0),
                    height: LayoutSizing::Fixed(content_height * 0.5 - 1.0),
                    align_x: crate::Align::Center,
                    align_y: crate::Align::Center,
                    ..LayoutInput::default()
                })
                .child(compact_text(
                    ElementId::local("arrow", 1, &id),
                    "▼",
                    palette.muted_text,
                    8.0,
                    10.0,
                ))
                .build(),
        )
        .build();

    ElementBuilder::container(id)
        .style(field_style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 64.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(field_height),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(inner.build())
        .child(stepper)
        .build()
}

pub fn search_box<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    spec: &TextInputSpec,
    cx: &C,
) -> Element {
    cx.register_text_input_widget(id.clone());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    search_box_impl(id, spec, &state, &palette)
}

pub(crate) fn search_box_impl(
    id: ElementId,
    spec: &TextInputSpec,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let padding = Edges::symmetric(8.0, 6.0);
    let field_height = 32.0;
    let content_height = field_height - padding.vertical();
    let field_style = input_field_container_style(state, palette);

    let show_placeholder = spec.value.is_empty();
    let display_text = if show_placeholder {
        spec.placeholder.clone()
    } else {
        spec.value.clone()
    };
    let text_color_val = if show_placeholder {
        palette.muted_text
    } else {
        palette.text
    };

    let search_icon_id = ElementId::local("search-icon", 0, &id);
    let search_icon = compact_text(
        search_icon_id,
        "⌕",
        palette.muted_text,
        14.0,
        content_height,
    );

    let text_id = ElementId::local("text", 0, &id);
    let mut text_el = compact_text(text_id, display_text, text_color_val, 14.0, content_height);
    text_el.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };
    text_el.layout.height = LayoutSizing::Fixed(content_height);

    let mut inner =
        ElementBuilder::container(ElementId::local("inner", 0, &id)).layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(content_height),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            ..LayoutInput::default()
        });

    if let Some((sel_start, sel_end)) = spec.selection {
        let sel_start = sel_start.max(0.0);
        let sel_end = sel_end.max(sel_start);
        if sel_end > sel_start {
            inner = inner.child(selection_element(
                ElementId::local("selection", 0, &id),
                sel_start,
                sel_end - sel_start,
                content_height,
                palette.accent.with_alpha(0.35),
            ));
        }
    }

    if state.focused {
        if let Some(cursor_x) = spec.cursor_x {
            inner = inner.child(cursor_element(
                ElementId::local("cursor", 0, &id),
                cursor_x,
                content_height,
                palette.accent,
            ));
        }
    }

    inner = inner.child(text_el);

    ElementBuilder::container(id)
        .style(field_style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 64.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(field_height),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 6.0,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(search_icon)
        .child(inner.build())
        .build()
}

pub fn select<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    selected_label: impl Into<String>,
    is_open: bool,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    select_with_palette(id, selected_label, is_open, &state, &palette)
}

pub(crate) fn select_with_palette(
    id: ElementId,
    selected_label: impl Into<String>,
    is_open: bool,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let mut style = input_field_container_style(state, palette);
    if is_open {
        style.outline = palette.outline_focus;
        style.outline_width = Edges::all(2.0);
    }

    let label_id = ElementId::local("label", 0, &id);
    let chevron_id = ElementId::local("chevron", 0, &id);
    let label_color = if state.disabled {
        palette.muted_text
    } else {
        palette.text
    };
    let chevron_char = if is_open { "▲" } else { "▼" };
    let mut label_el = compact_text(label_id, selected_label, label_color, 14.0, 18.0);
    label_el.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };

    ElementBuilder::container(id)
        .style(style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 80.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(32.0),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            ..LayoutInput::default()
        })
        .child(label_el)
        .child(compact_text(
            chevron_id,
            chevron_char,
            palette.muted_text,
            10.0,
            12.0,
        ))
        .build()
}

pub fn icon_button<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    icon_key: impl AsRef<str>,
    icon_size: f32,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    icon_button_with_palette(id, icon_key, icon_size, &state, &palette)
}

pub(crate) fn icon_button_with_palette(
    id: ElementId,
    icon_key: impl AsRef<str>,
    icon_size: f32,
    state: &WidgetState,
    palette: &WidgetPalette,
) -> Element {
    let button_size = icon_size + 12.0;
    let mut style = control_style(state, palette, false, 6.0, Edges::ZERO);
    style.outline_width = Edges::all(if state.focused { 2.0 } else { 0.0 });

    let icon_id = ElementId::local("icon", 0, &id);
    ElementBuilder::container(id)
        .style(style)
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(button_size),
            height: LayoutSizing::Fixed(button_size),
            align_x: crate::Align::Center,
            align_y: crate::Align::Center,
            ..LayoutInput::default()
        })
        .child(icon_element(icon_id, icon_key.as_ref(), icon_size))
        .build()
}

pub fn toolbar(id: ElementId, children: impl IntoIterator<Item = Element>) -> Element {
    toolbar_with_palette(id, children, &WidgetPalette::default())
}

pub fn toolbar_with_palette(
    id: ElementId,
    children: impl IntoIterator<Item = Element>,
    palette: &WidgetPalette,
) -> Element {
    let mut builder = ElementBuilder::container(id)
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges {
                bottom: 1.0,
                ..Edges::ZERO
            },
            padding: Edges::symmetric(4.0, 4.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 2.0,
            ..LayoutInput::default()
        });

    for child in children {
        builder = builder.child(child);
    }
    builder.build()
}

pub fn accordion_panel<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    config: AccordionPanelConfig,
    content: Option<Element>,
    cx: &C,
) -> Element {
    let header_id = config.id.clone();
    cx.register_widget_behavior(config.id.clone(), WidgetBehavior::interactive());
    let state = config
        .state_override
        .clone()
        .unwrap_or_else(|| cx.widget_state(&config.id));
    let palette = cx.widget_palette();
    let palette = &palette;
    let chevron_char = if config.is_open { "▼" } else { "›" };
    let mut header_style = ElementStyle {
        background: if state.pressed || state.captured {
            palette.surface_pressed
        } else if state.hovered || state.focused {
            palette.surface_hovered
        } else {
            palette.surface
        },
        outline: if state.focused {
            palette.outline_focus
        } else {
            palette.outline
        },
        outline_width: Edges {
            bottom: 1.0,
            ..Edges::ZERO
        },
        padding: Edges::symmetric(12.0, 10.0),
        ..ElementStyle::default()
    };
    if state.disabled {
        header_style.background = palette.surface_disabled;
    }

    let label_color = if state.disabled {
        palette.muted_text
    } else {
        palette.text
    };
    let mut label_el = compact_text(
        ElementId::local("label", 0, &header_id),
        config.title,
        label_color,
        14.0,
        18.0,
    );
    label_el.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };

    let header = ElementBuilder::container(header_id)
        .style(header_style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 8.0,
            ..LayoutInput::default()
        })
        .child(compact_text(
            ElementId::local("chevron", 0, &id),
            chevron_char,
            palette.muted_text,
            12.0,
            14.0,
        ))
        .child(label_el)
        .build();

    let mut panel = ElementBuilder::container(id)
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(6.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(header);

    if config.is_open {
        if let Some(body) = content {
            panel = panel.child(body);
        }
    }

    panel.build()
}

pub fn group_box(
    id: ElementId,
    label_text: impl Into<String>,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    group_box_with_palette(id, label_text, children, &WidgetPalette::default())
}

pub fn group_box_with_palette(
    id: ElementId,
    label_text: impl Into<String>,
    children: impl IntoIterator<Item = Element>,
    palette: &WidgetPalette,
) -> Element {
    let header_id = ElementId::local("header", 0, &id);
    let content_id = ElementId::local("content", 0, &id);

    let header = ElementBuilder::container(header_id.clone())
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges {
                bottom: 1.0,
                ..Edges::ZERO
            },
            padding: Edges::symmetric(12.0, 8.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            ..LayoutInput::default()
        })
        .child(compact_text(
            ElementId::local("label", 0, &header_id),
            label_text,
            palette.muted_text,
            11.0,
            14.0,
        ))
        .build();

    let mut content_builder = ElementBuilder::container(content_id).layout(LayoutInput {
        width: LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        ..LayoutInput::default()
    });

    for child in children {
        content_builder = content_builder.child(child);
    }

    ElementBuilder::container(id)
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(6.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(header)
        .child(content_builder.build())
        .build()
}

pub fn dialog_surface(
    id: ElementId,
    title: Option<impl Into<String>>,
    size: Size,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    dialog_surface_with_palette(id, title, size, children, &WidgetPalette::default())
}

pub fn dialog_surface_with_palette(
    id: ElementId,
    title: Option<impl Into<String>>,
    size: Size,
    children: impl IntoIterator<Item = Element>,
    palette: &WidgetPalette,
) -> Element {
    let body_id = ElementId::local("body", 0, &id);
    let title_bar_id = ElementId::local("title-bar", 0, &id);

    let mut builder = ElementBuilder::container(id)
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(10.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(size.width.max(0.0)),
            height: LayoutSizing::Fixed(size.height.max(0.0)),
            direction: LayoutDirection::TopToBottom,
            clip_x: true,
            clip_y: true,
            ..LayoutInput::default()
        });

    if let Some(title_text) = title {
        let title_bar_id = title_bar_id;
        let title_bar = ElementBuilder::container(title_bar_id.clone())
            .style(ElementStyle {
                background: palette.surface,
                outline: palette.outline,
                outline_width: Edges {
                    bottom: 1.0,
                    ..Edges::ZERO
                },
                padding: Edges::symmetric(16.0, 12.0),
                ..ElementStyle::default()
            })
            .layout(LayoutInput {
                width: LayoutSizing::Grow {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                ..LayoutInput::default()
            })
            .child(
                ElementBuilder::text(
                    ElementId::local("title", 0, &title_bar_id),
                    title_text,
                    TextStyle {
                        font_size: 15.0,
                        line_height: 20.0,
                        color: palette.text,
                        wrap: TextWrap::None,
                        ..TextStyle::default()
                    },
                )
                .build(),
            )
            .build();
        builder = builder.child(title_bar);
    }

    let mut body_builder = ElementBuilder::container(body_id).layout(LayoutInput {
        width: LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        ..LayoutInput::default()
    });
    for child in children {
        body_builder = body_builder.child(child);
    }

    builder.child(body_builder.build()).build()
}

pub fn context_menu_item<C: WidgetRenderContext + ?Sized>(
    item: ContextMenuItemSpec,
    row_height: f32,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(item.id.clone(), WidgetBehavior::interactive());
    let mut state = cx.widget_state(&item.id);
    state.disabled |= item.disabled;
    let palette = cx.widget_palette();
    let palette = &palette;
    let row_id = item.id.clone();

    let mut style = ElementStyle {
        background: if state.disabled {
            UiColor::TRANSPARENT
        } else if state.pressed || state.captured {
            palette.surface_pressed
        } else if state.hovered || state.focused {
            palette.surface_hovered
        } else {
            UiColor::TRANSPARENT
        },
        outline: if state.focused {
            palette.outline_focus
        } else if item.separator_before {
            palette.outline
        } else {
            UiColor::TRANSPARENT
        },
        outline_width: if state.focused {
            Edges::all(1.0)
        } else if item.separator_before {
            Edges {
                top: 1.0,
                ..Edges::ZERO
            }
        } else {
            Edges::ZERO
        },
        padding: Edges::symmetric(10.0, 0.0),
        ..ElementStyle::default()
    };
    if state.disabled {
        style.background = UiColor::TRANSPARENT;
    }

    let label_color = if state.disabled {
        palette.muted_text
    } else {
        palette.text
    };

    let mut row = ElementBuilder::container(row_id.clone())
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
        });

    if let Some(icon_key) = &item.icon_key {
        row = row.child(icon_element(
            ElementId::local("icon", 0, &row_id),
            icon_key,
            14.0,
        ));
    } else {
        row = row.child(horizontal_spacer(
            ElementId::local("icon-gap", 0, &row_id),
            14.0,
            row_height,
        ));
    }

    let mut label = compact_text(
        ElementId::local("label", 0, &row_id),
        item.label,
        label_color,
        13.0,
        16.0,
    );
    label.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };
    row = row.child(label);

    if let Some(shortcut) = item.shortcut {
        row = row.child(compact_text(
            ElementId::local("shortcut", 0, &row_id),
            shortcut,
            palette.muted_text,
            11.0,
            14.0,
        ));
    }

    row.build()
}

pub fn virtual_context_menu<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    width: f32,
    layout: &VirtualListLayout,
    visible_items: impl IntoIterator<Item = ContextMenuItemSpec>,
    cx: &C,
) -> Element {
    let rows = visible_items
        .into_iter()
        .map(|item| context_menu_item(item, layout.item_extent, cx));
    let width_sizing = LayoutSizing::Fixed(width.max(0.0));
    let mut menu = virtual_list(id, width_sizing, layout.viewport_extent, layout, rows);
    menu.style = ElementStyle {
        background: cx.widget_palette().surface,
        outline: cx.widget_palette().outline,
        outline_width: Edges::all(1.0),
        corner_radius: radii_all(8.0),
        ..ElementStyle::default()
    };
    menu.layout.clip_x = true;
    menu
}

pub fn command_palette(
    id: ElementId,
    config: CommandPaletteConfig,
    input_spec: &TextInputSpec,
    layout: &VirtualListLayout,
    visible_items: impl IntoIterator<Item = CommandPaletteItemSpec>,
    cx: &Cx,
) -> Element {
    let panel_id = ElementId::local("panel", 0, &id);
    let input_id = ElementId::local("input", 0, &panel_id);
    let list_id = ElementId::local("results", 0, &panel_id);
    let header_id = ElementId::local("header", 0, &panel_id);

    let input_el = text_input(input_id, input_spec, cx);

    let sim = cx.sim;
    let palette = cx.palette;
    let rows = visible_items
        .into_iter()
        .map(|item| command_palette_item_row(item, layout.item_extent, sim, &palette));
    let results_list = {
        let mut list = virtual_list(
            list_id,
            LayoutSizing::Fixed(config.width.max(0.0)),
            layout.viewport_extent.min(config.max_list_height),
            layout,
            rows,
        );
        list.style = ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges {
                top: 1.0,
                ..Edges::ZERO
            },
            ..ElementStyle::default()
        };
        list
    };

    let mut panel_builder = ElementBuilder::container(panel_id.clone())
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(10.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(config.width.max(0.0)),
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            clip_x: true,
            clip_y: true,
            ..LayoutInput::default()
        });

    if let Some(title_text) = &config.title {
        let header = ElementBuilder::container(header_id.clone())
            .style(ElementStyle {
                padding: Edges::symmetric(16.0, 10.0),
                outline: palette.outline,
                outline_width: Edges {
                    bottom: 1.0,
                    ..Edges::ZERO
                },
                ..ElementStyle::default()
            })
            .layout(LayoutInput {
                width: LayoutSizing::Grow {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                ..LayoutInput::default()
            })
            .child(compact_text(
                ElementId::local("title", 0, &header_id),
                title_text,
                palette.text,
                13.0,
                16.0,
            ))
            .build();
        panel_builder = panel_builder.child(header);
    }

    panel_builder = panel_builder
        .child(
            ElementBuilder::container(ElementId::local("input-wrap", 0, &panel_id))
                .style(ElementStyle {
                    padding: Edges::all(8.0),
                    ..ElementStyle::default()
                })
                .layout(LayoutInput {
                    width: LayoutSizing::Grow {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    height: LayoutSizing::Fit {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    ..LayoutInput::default()
                })
                .child(input_el)
                .build(),
        )
        .child(results_list);

    let panel = panel_builder.build();

    modal_layer(
        id,
        ModalLayerConfig::new(config.viewport)
            .backdrop(config.backdrop)
            .z_index(config.z_index),
        [
            ElementBuilder::container(ElementId::local("centering", 0, &panel_id))
                .style(ElementStyle {
                    transparent_to_input: true,
                    ..ElementStyle::default()
                })
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(config.viewport.width.max(0.0)),
                    height: LayoutSizing::Fixed(config.viewport.height.max(0.0)),
                    align_x: crate::Align::Center,
                    align_y: crate::Align::Center,
                    ..LayoutInput::default()
                })
                .child(panel)
                .build(),
        ],
    )
}

// ── Batch-2 widget builders ───────────────────────────────────────────────────

pub fn list_item<C: WidgetRenderContext + ?Sized>(item: ListItemSpec, cx: &C) -> Element {
    cx.register_widget_behavior(item.id.clone(), WidgetBehavior::interactive());
    let state = item
        .state_override
        .clone()
        .unwrap_or_else(|| cx.widget_state(&item.id));
    let palette = cx.widget_palette();
    let palette = &palette;
    let row_id = item.id.clone();
    let style = hoverable_row_style(&state, palette, item.selected);
    let label_color = if state.disabled {
        palette.muted_text
    } else if item.selected {
        palette.accent_text
    } else {
        palette.text
    };

    let mut row = ElementBuilder::container(row_id.clone())
        .style(style)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            gap: 2.0,
            clip_x: true,
            ..LayoutInput::default()
        });

    let mut label_el = compact_text(
        ElementId::local("label", 0, &row_id),
        item.label,
        label_color,
        14.0,
        18.0,
    );
    label_el.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };
    row = row.child(label_el);

    if let Some(sub) = item.sublabel {
        let sub_color = if state.disabled || item.selected {
            label_color.with_alpha(0.6)
        } else {
            palette.muted_text
        };
        let mut sub_el = compact_text(
            ElementId::local("sublabel", 0, &row_id),
            sub,
            sub_color,
            12.0,
            16.0,
        );
        sub_el.layout.width = LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        };
        row = row.child(sub_el);
    }

    row.build()
}

pub fn table_header_cell<C: WidgetRenderContext + ?Sized>(
    spec: TableHeaderSpec,
    height: f32,
    cx: &C,
) -> Element {
    cx.register_widget_behavior(spec.id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&spec.id);
    let palette = cx.widget_palette();
    let palette = &palette;
    let cell_id = spec.id.clone();
    let bg = if state.pressed || state.captured {
        palette.surface_pressed
    } else if state.hovered || state.focused {
        palette.surface_hovered
    } else {
        palette.surface
    };
    let label_color = if state.disabled {
        palette.muted_text
    } else {
        palette.text
    };

    let sort_char = match spec.sort {
        SortDirection::None => "",
        SortDirection::Ascending => " ▲",
        SortDirection::Descending => " ▼",
    };
    let label_text = if sort_char.is_empty() {
        spec.label
    } else {
        format!("{}{sort_char}", spec.label)
    };

    let mut label_el = compact_text(
        ElementId::local("label", 0, &cell_id),
        label_text,
        label_color,
        12.0,
        16.0,
    );
    label_el.layout.width = LayoutSizing::Fixed(spec.width.max(0.0));

    ElementBuilder::container(cell_id)
        .style(ElementStyle {
            background: bg,
            outline: palette.outline,
            outline_width: Edges {
                bottom: 1.0,
                right: 1.0,
                ..Edges::ZERO
            },
            padding: Edges::symmetric(8.0, 0.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(spec.width.max(0.0)),
            height: LayoutSizing::Fixed(height.max(0.0)),
            align_y: crate::Align::Center,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(label_el)
        .build()
}

pub fn table_header_row<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    height: f32,
    specs: impl IntoIterator<Item = TableHeaderSpec>,
    cx: &C,
) -> Element {
    let palette = cx.widget_palette();
    let mut builder = ElementBuilder::container(id)
        .style(ElementStyle {
            background: palette.surface,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(height.max(0.0)),
            direction: LayoutDirection::LeftToRight,
            ..LayoutInput::default()
        });

    for spec in specs {
        builder = builder.child(table_header_cell(spec, height, cx));
    }

    builder.build()
}

pub fn property_row<C: WidgetRenderContext + ?Sized>(
    spec: PropertyRowSpec,
    value: Element,
    row_height: f32,
    cx: &C,
) -> Element {
    let state = cx.widget_state(&spec.id);
    let palette = cx.widget_palette();
    let palette = &palette;
    let row_id = spec.id.clone();
    let label_id = ElementId::local("label", 0, &row_id);
    let style = hoverable_row_style(&state, palette, false);
    let label_color = palette.muted_text;

    let mut label_el = compact_text(label_id, spec.label, label_color, 13.0, 16.0);
    label_el.layout.width = LayoutSizing::Fixed(spec.label_width.max(0.0));

    ElementBuilder::container(row_id)
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
            ..LayoutInput::default()
        })
        .child(label_el)
        .child(value)
        .build()
}

pub fn chip<C: WidgetRenderContext + ?Sized>(spec: ChipSpec, cx: &C) -> Element {
    cx.register_widget_behavior(spec.id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&spec.id);
    let palette = cx.widget_palette();
    let palette = &palette;
    let chip_id = spec.id.clone();
    let remove_id = ElementId::local("remove", 0, &chip_id);
    cx.register_widget_behavior(remove_id.clone(), WidgetBehavior::interactive());
    let remove_state = cx.widget_state(&remove_id);
    let (bg, fg) = spec.variant.colors(palette);
    let border = if state.focused {
        palette.outline_focus
    } else {
        palette.outline
    };

    let mut chip_builder = ElementBuilder::container(chip_id.clone())
        .style(ElementStyle {
            background: bg,
            outline: border,
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(999.0),
            padding: Edges::symmetric(8.0, 3.0),
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
            align_y: crate::Align::Center,
            gap: 4.0,
            ..LayoutInput::default()
        })
        .child(compact_text(
            ElementId::local("label", 0, &chip_id),
            spec.label,
            fg,
            12.0,
            16.0,
        ));

    if spec.can_remove {
        let rm_bg = if remove_state.pressed || remove_state.captured {
            fg.with_alpha(0.35)
        } else if remove_state.hovered {
            fg.with_alpha(0.2)
        } else {
            UiColor::TRANSPARENT
        };
        chip_builder = chip_builder.child(
            ElementBuilder::container(remove_id)
                .style(ElementStyle {
                    background: rm_bg,
                    corner_radius: radii_all(999.0),
                    ..ElementStyle::default()
                })
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(14.0),
                    height: LayoutSizing::Fixed(14.0),
                    align_x: crate::Align::Center,
                    align_y: crate::Align::Center,
                    ..LayoutInput::default()
                })
                .child(compact_text(
                    ElementId::local("x", 0, &chip_id),
                    "×",
                    fg,
                    10.0,
                    12.0,
                ))
                .build(),
        );
    }

    chip_builder.build()
}

pub fn notification<C: WidgetRenderContext + ?Sized>(spec: NotificationSpec, cx: &C) -> Element {
    let action_id = ElementId::local("action", 0, &spec.id);
    cx.register_widget_behavior(spec.id.clone(), WidgetBehavior::interactive());
    cx.register_widget_behavior(action_id.clone(), WidgetBehavior::interactive());
    let action_state = cx.widget_state(&action_id);
    let palette = cx.widget_palette();
    let palette = &palette;
    let notif_id = spec.id.clone();
    let (bg, fg) = spec.variant.colors(palette);
    let accent_bar_id = ElementId::local("bar", 0, &notif_id);
    let msg_id = ElementId::local("message", 0, &notif_id);

    let accent_bar = ElementBuilder::container(accent_bar_id)
        .style(ElementStyle {
            background: fg,
            corner_radius: radii_all(999.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(3.0),
            height: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            ..LayoutInput::default()
        })
        .build();

    let mut msg_el = ElementBuilder::text(
        msg_id,
        spec.message,
        TextStyle {
            font_size: 13.0,
            line_height: 18.0,
            color: palette.text,
            wrap: TextWrap::Words,
            ..TextStyle::default()
        },
    )
    .layout(LayoutInput {
        width: LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        ..LayoutInput::default()
    })
    .build();
    msg_el.style.transparent_to_input = true;

    let mut row = ElementBuilder::container(notif_id.clone())
        .style(ElementStyle {
            background: bg.with_alpha(0.18),
            outline: fg.with_alpha(0.35),
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(6.0),
            padding: Edges::symmetric(10.0, 8.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 10.0,
            ..LayoutInput::default()
        })
        .child(accent_bar)
        .child(msg_el);

    if let Some(action_label) = spec.action_label {
        row = row.child(
            ElementBuilder::container(action_id)
                .style(ElementStyle {
                    background: if action_state.pressed || action_state.captured {
                        fg.with_alpha(0.3)
                    } else if action_state.hovered {
                        fg.with_alpha(0.15)
                    } else {
                        UiColor::TRANSPARENT
                    },
                    outline: fg.with_alpha(0.5),
                    outline_width: Edges::all(1.0),
                    corner_radius: radii_all(4.0),
                    padding: Edges::symmetric(8.0, 3.0),
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
                    ..LayoutInput::default()
                })
                .child(compact_text(
                    ElementId::local("action-label", 0, &notif_id),
                    action_label,
                    fg,
                    12.0,
                    16.0,
                ))
                .build(),
        );
    }

    row.build()
}

pub fn status_bar_with_palette(
    id: ElementId,
    sections: impl IntoIterator<Item = StatusBarSectionSpec>,
    palette: &WidgetPalette,
) -> Element {
    status_bar(id, sections, palette)
}

pub fn status_bar<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    sections: impl IntoIterator<Item = StatusBarSectionSpec>,
    cx: &C,
) -> Element {
    let sections: Vec<StatusBarSectionSpec> = sections.into_iter().collect();
    let count = sections.len();
    let palette = cx.widget_palette();
    let palette = &palette;
    let mut builder = ElementBuilder::container(id.clone())
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges {
                top: 1.0,
                ..Edges::ZERO
            },
            padding: Edges::symmetric(12.0, 0.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(24.0),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 0.0,
            ..LayoutInput::default()
        });

    for (i, section) in sections.into_iter().enumerate() {
        let sec_id = section.id.clone();
        let sec_state = cx.widget_state(&section.id);
        let text = if let Some(val) = section.value {
            format!("{}: {}", section.label, val)
        } else {
            section.label
        };
        let color = if sec_state.hovered {
            palette.accent
        } else {
            palette.muted_text
        };
        let mut text_el = compact_text(sec_id, text, color, 11.0, 14.0);
        text_el.layout.width = LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        };
        builder = builder.child(text_el);

        if i + 1 < count {
            builder = builder.child(
                ElementBuilder::container(ElementId::local("sep", i as u32, &id))
                    .style(ElementStyle {
                        background: palette.outline,
                        ..ElementStyle::default()
                    })
                    .layout(LayoutInput {
                        width: LayoutSizing::Fixed(1.0),
                        height: LayoutSizing::Fixed(14.0),
                        position: LayoutPosition::Flow,
                        ..LayoutInput::default()
                    })
                    .build(),
            );
            builder = builder.child(horizontal_spacer(
                ElementId::local("gap", i as u32, &id),
                12.0,
                14.0,
            ));
        }
    }

    builder.build()
}

pub fn card(
    id: ElementId,
    title: Option<impl Into<String>>,
    width: LayoutSizing,
    height: LayoutSizing,
    children: impl IntoIterator<Item = Element>,
) -> Element {
    card_with_palette(
        id,
        title,
        width,
        height,
        children,
        &WidgetPalette::default(),
    )
}

pub fn card_with_palette(
    id: ElementId,
    title: Option<impl Into<String>>,
    width: LayoutSizing,
    height: LayoutSizing,
    children: impl IntoIterator<Item = Element>,
    palette: &WidgetPalette,
) -> Element {
    let title_bar_id = ElementId::local("card-title", 0, &id);
    let body_id = ElementId::local("card-body", 0, &id);

    let mut outer = ElementBuilder::container(id)
        .style(ElementStyle {
            background: palette.surface,
            outline: palette.outline,
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(8.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width,
            height,
            direction: LayoutDirection::TopToBottom,
            clip_x: true,
            clip_y: true,
            ..LayoutInput::default()
        });

    if let Some(title_text) = title {
        outer = outer.child(
            ElementBuilder::container(title_bar_id.clone())
                .style(ElementStyle {
                    background: palette.surface_hovered,
                    outline: palette.outline,
                    outline_width: Edges {
                        bottom: 1.0,
                        ..Edges::ZERO
                    },
                    padding: Edges::symmetric(12.0, 8.0),
                    ..ElementStyle::default()
                })
                .layout(LayoutInput {
                    width: LayoutSizing::Grow {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    height: LayoutSizing::Fit {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    ..LayoutInput::default()
                })
                .child(compact_text(
                    ElementId::local("label", 0, &title_bar_id),
                    title_text,
                    palette.text,
                    13.0,
                    16.0,
                ))
                .build(),
        );
    }

    let mut body = ElementBuilder::container(body_id).layout(LayoutInput {
        width: LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        },
        height: LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        ..LayoutInput::default()
    });
    for child in children {
        body = body.child(child);
    }

    outer.child(body.build()).build()
}

// ── New private helpers ────────────────────────────────────────────────────────

fn hoverable_row_style(
    state: &WidgetState,
    palette: &WidgetPalette,
    selected: bool,
) -> ElementStyle {
    let background = if state.disabled {
        UiColor::TRANSPARENT
    } else if selected {
        palette.surface_selected
    } else if state.pressed || state.captured {
        palette.surface_pressed
    } else if state.hovered || state.focused {
        palette.surface_hovered
    } else {
        UiColor::TRANSPARENT
    };
    let outline = if state.focused {
        palette.outline_focus
    } else {
        palette.outline
    };
    ElementStyle {
        background,
        outline,
        outline_width: if state.focused {
            Edges::all(1.0)
        } else {
            Edges {
                bottom: 1.0,
                ..Edges::ZERO
            }
        },
        padding: Edges::symmetric(8.0, 6.0),
        ..ElementStyle::default()
    }
}

fn input_field_container_style(state: &WidgetState, palette: &WidgetPalette) -> ElementStyle {
    let background = if state.disabled {
        palette.surface_disabled
    } else if state.focused || state.hovered {
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
    let outline_width = if state.focused {
        Edges::all(2.0)
    } else {
        Edges::all(1.0)
    };
    ElementStyle {
        background,
        outline,
        outline_width,
        corner_radius: radii_all(6.0),
        padding: Edges::symmetric(10.0, 6.0),
        ..ElementStyle::default()
    }
}

fn icon_element(id: ElementId, image_key: &str, size: f32) -> Element {
    let size = size.max(0.0);
    let mut element = Element::image(id, image_key);
    if let crate::ElementKind::Image(image) = &mut element.kind {
        image.natural_size = Some(Size::new(size, size));
        image.options = UiImageOptions {
            fit: crate::UiImageFit::Stretch,
            ..UiImageOptions::default()
        };
    }
    element.layout.width = LayoutSizing::Fixed(size);
    element.layout.height = LayoutSizing::Fixed(size);
    element.style.transparent_to_input = true;
    element
}

fn selection_element(id: ElementId, x: f32, width: f32, height: f32, color: UiColor) -> Element {
    ElementBuilder::container(id)
        .style(ElementStyle {
            background: color,
            corner_radius: radii_all(2.0),
            transparent_to_input: true,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(width.max(0.0)),
            height: LayoutSizing::Fixed(height.max(0.0)),
            position: LayoutPosition::Absolute {
                offset: glam::Vec2::new(x, 0.0),
            },
            ..LayoutInput::default()
        })
        .build()
}

fn cursor_element(id: ElementId, x: f32, height: f32, color: UiColor) -> Element {
    ElementBuilder::container(id)
        .style(ElementStyle {
            background: color,
            transparent_to_input: true,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(2.0),
            height: LayoutSizing::Fixed(height.max(0.0)),
            position: LayoutPosition::Absolute {
                offset: glam::Vec2::new(x, 0.0),
            },
            ..LayoutInput::default()
        })
        .build()
}

fn command_palette_item_row(
    item: CommandPaletteItemSpec,
    row_height: f32,
    sim: &crate::InputSimulator,
    palette: &WidgetPalette,
) -> Element {
    let mut state = sim.widget_state(&item.id);
    state.disabled |= item.disabled;
    let row_id = item.id.clone();

    let bg = if state.disabled {
        UiColor::TRANSPARENT
    } else if item.selected {
        palette.surface_selected
    } else if state.pressed || state.captured {
        palette.surface_pressed
    } else if state.hovered || state.focused {
        palette.surface_hovered
    } else {
        UiColor::TRANSPARENT
    };

    let label_color = if state.disabled {
        palette.muted_text
    } else if item.selected {
        palette.accent_text
    } else {
        palette.text
    };
    let desc_color = if state.disabled || item.selected {
        label_color.with_alpha(180.0 / 255.0)
    } else {
        palette.muted_text
    };

    let outline = if state.focused {
        palette.outline_focus
    } else {
        UiColor::TRANSPARENT
    };

    let mut label_el = compact_text(
        ElementId::local("label", 0, &row_id),
        item.label,
        label_color,
        14.0,
        18.0,
    );
    label_el.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };

    let mut row_builder = ElementBuilder::container(row_id.clone())
        .style(ElementStyle {
            background: bg,
            outline,
            outline_width: if state.focused {
                Edges::all(1.0)
            } else {
                Edges::ZERO
            },
            padding: Edges::symmetric(16.0, 0.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(row_height.max(0.0)),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 12.0,
            clip_x: true,
            ..LayoutInput::default()
        })
        .child(label_el);

    if let Some(group) = item.group {
        row_builder = row_builder.child(compact_text(
            ElementId::local("group", 0, &row_id),
            group,
            desc_color,
            11.0,
            14.0,
        ));
    }

    if let Some(desc) = item.description {
        row_builder = row_builder.child(compact_text(
            ElementId::local("desc", 0, &row_id),
            desc,
            desc_color,
            12.0,
            16.0,
        ));
    }

    if let Some(shortcut) = item.shortcut {
        row_builder = row_builder.child(compact_text(
            ElementId::local("shortcut", 0, &row_id),
            shortcut,
            palette.muted_text,
            11.0,
            14.0,
        ));
    }

    row_builder.build()
}

// ── Existing helpers ──────────────────────────────────────────────────────────

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

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
