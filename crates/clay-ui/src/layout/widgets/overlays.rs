use crate::{
    Edges, Element, ElementBuilder, ElementId, ElementStyle, FloatingAttachConfig,
    FloatingAttachError, LayoutInput, LayoutSizing, LayoutTree, Size, TextStyle, TextWrap, UiLayer,
    attached_floating_layer, floating::place_subtree_in_layer, radii_all,
};

use super::{ModalLayerConfig, PortalHostConfig, TooltipConfig, WidgetPalette};

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

pub fn tooltip_layer(
    id: ElementId,
    layout: &LayoutTree,
    config: TooltipConfig,
    text: impl Into<String>,
) -> Result<Element, FloatingAttachError> {
    tooltip_layer_with_palette(id, layout, config, text, &WidgetPalette::default())
}

pub fn tooltip_layer_with_palette(
    id: ElementId,
    layout: &LayoutTree,
    config: TooltipConfig,
    text: impl Into<String>,
    palette: &WidgetPalette,
) -> Result<Element, FloatingAttachError> {
    let surface = tooltip_surface(
        ElementId::local("surface", 0, &id),
        text,
        config.size,
        palette,
    );
    let attach_config = FloatingAttachConfig::new(config.viewport, config.anchor, config.size)
        .options(config.options)
        .z_index(config.z_index)
        .clip(config.clip)
        .transparent_to_input(true);

    attached_floating_layer(id, layout, &attach_config, surface)
}

pub fn tooltip_surface(
    id: ElementId,
    text: impl Into<String>,
    size: Size,
    palette: &WidgetPalette,
) -> Element {
    let padding = Edges::symmetric(8.0, 5.0);
    let label_id = ElementId::local("label", 0, &id);
    let mut label = ElementBuilder::text(
        label_id,
        text,
        TextStyle {
            font_size: 12.0,
            line_height: 16.0,
            color: palette.text,
            wrap: TextWrap::Words,
            ..TextStyle::default()
        },
    )
    .layout(LayoutInput {
        width: LayoutSizing::Fixed((size.width - padding.horizontal()).max(0.0)),
        height: LayoutSizing::Fit {
            min: 0.0,
            max: (size.height - padding.vertical()).max(0.0),
        },
        ..LayoutInput::default()
    })
    .build();
    label.style.transparent_to_input = true;

    ElementBuilder::container(id)
        .style(ElementStyle {
            background: palette.surface.with_alpha(0.96),
            outline: palette.outline,
            outline_width: Edges::all(1.0),
            corner_radius: radii_all(6.0),
            padding,
            transparent_to_input: true,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(size.width.max(0.0)),
            height: LayoutSizing::Fixed(size.height.max(0.0)),
            clip_x: true,
            clip_y: true,
            ..LayoutInput::default()
        })
        .child(label)
        .build()
}
