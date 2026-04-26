use crate::{
    Element, ElementBuilder, ElementId, ElementStyle, LayoutInput, LayoutPosition, LayoutSizing,
    Rect, Size, UiLayer,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatingSide {
    Top,
    Right,
    Bottom,
    Left,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatingAlign {
    Start,
    Center,
    End,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FloatingCollision {
    None,
    Clamp,
    Flip,
    FlipAndClamp,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FloatingPlacement {
    pub side: FloatingSide,
    pub align: FloatingAlign,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatingOptions {
    pub placement: FloatingPlacement,
    pub collision: FloatingCollision,
    pub offset: f32,
    pub viewport_margin: f32,
    pub match_anchor_width: bool,
    pub constrain_to_viewport: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatingLayout {
    pub requested_placement: FloatingPlacement,
    pub placement: FloatingPlacement,
    pub rect: Rect,
    pub flipped: bool,
    pub clamped: bool,
    pub constrained: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FloatingLayerConfig {
    pub viewport: Size,
    pub anchor: Rect,
    pub content_size: Size,
    pub options: FloatingOptions,
    pub z_index: i16,
    pub clip: bool,
    pub transparent_to_input: bool,
}

impl FloatingSide {
    pub const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Right => Self::Left,
            Self::Bottom => Self::Top,
            Self::Left => Self::Right,
        }
    }
}

impl FloatingPlacement {
    pub const fn new(side: FloatingSide, align: FloatingAlign) -> Self {
        Self { side, align }
    }

    pub const fn top(align: FloatingAlign) -> Self {
        Self::new(FloatingSide::Top, align)
    }

    pub const fn right(align: FloatingAlign) -> Self {
        Self::new(FloatingSide::Right, align)
    }

    pub const fn bottom(align: FloatingAlign) -> Self {
        Self::new(FloatingSide::Bottom, align)
    }

    pub const fn left(align: FloatingAlign) -> Self {
        Self::new(FloatingSide::Left, align)
    }

    pub const fn opposite(self) -> Self {
        Self {
            side: self.side.opposite(),
            align: self.align,
        }
    }
}

impl Default for FloatingPlacement {
    fn default() -> Self {
        Self::bottom(FloatingAlign::Start)
    }
}

impl Default for FloatingCollision {
    fn default() -> Self {
        Self::FlipAndClamp
    }
}

impl Default for FloatingOptions {
    fn default() -> Self {
        Self {
            placement: FloatingPlacement::default(),
            collision: FloatingCollision::default(),
            offset: 4.0,
            viewport_margin: 8.0,
            match_anchor_width: false,
            constrain_to_viewport: true,
        }
    }
}

impl FloatingOptions {
    pub fn placement(mut self, placement: FloatingPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn collision(mut self, collision: FloatingCollision) -> Self {
        self.collision = collision;
        self
    }

    pub fn offset(mut self, offset: f32) -> Self {
        self.offset = offset.max(0.0);
        self
    }

    pub fn viewport_margin(mut self, viewport_margin: f32) -> Self {
        self.viewport_margin = viewport_margin.max(0.0);
        self
    }

    pub fn match_anchor_width(mut self, match_anchor_width: bool) -> Self {
        self.match_anchor_width = match_anchor_width;
        self
    }

    pub fn constrain_to_viewport(mut self, constrain_to_viewport: bool) -> Self {
        self.constrain_to_viewport = constrain_to_viewport;
        self
    }
}

impl FloatingLayout {
    pub fn compute(
        anchor: Rect,
        content_size: Size,
        viewport: Size,
        options: FloatingOptions,
    ) -> Self {
        let requested_placement = options.placement;
        let (content_size, constrained) =
            constrained_content_size(content_size, anchor, viewport, options);
        let mut placement = requested_placement;
        let mut rect = place_rect(anchor, content_size, placement, options.offset);
        let mut flipped = false;

        if matches!(
            options.collision,
            FloatingCollision::Flip | FloatingCollision::FlipAndClamp
        ) && primary_overflow(rect, placement.side, viewport, options.viewport_margin) > 0.0
        {
            let current_space = primary_space(anchor, placement.side, viewport, options);
            let opposite = placement.opposite();
            let opposite_space = primary_space(anchor, opposite.side, viewport, options);
            if opposite_space > current_space {
                placement = opposite;
                rect = place_rect(anchor, content_size, placement, options.offset);
                flipped = true;
            }
        }

        let (rect, clamped) = if matches!(
            options.collision,
            FloatingCollision::Clamp | FloatingCollision::FlipAndClamp
        ) {
            clamp_rect(rect, viewport, options.viewport_margin)
        } else {
            (rect, false)
        };

        Self {
            requested_placement,
            placement,
            rect,
            flipped,
            clamped,
            constrained,
        }
    }
}

impl FloatingLayerConfig {
    pub fn new(viewport: Size, anchor: Rect, content_size: Size) -> Self {
        Self {
            viewport,
            anchor,
            content_size,
            options: FloatingOptions::default(),
            z_index: 0,
            clip: true,
            transparent_to_input: true,
        }
    }

    pub fn options(mut self, options: FloatingOptions) -> Self {
        self.options = options;
        self
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

    pub fn layout(self) -> FloatingLayout {
        FloatingLayout::compute(self.anchor, self.content_size, self.viewport, self.options)
    }
}

pub fn anchored_floating_layer(
    id: ElementId,
    config: FloatingLayerConfig,
    mut content: Element,
) -> Element {
    let layout = config.layout();
    content.layout.position = LayoutPosition::Absolute {
        offset: layout.rect.origin,
    };
    content.layout.width = LayoutSizing::Fixed(layout.rect.size.width);
    content.layout.height = LayoutSizing::Fixed(layout.rect.size.height);
    place_subtree_in_layer(
        &mut content,
        UiLayer::TopLayer,
        config.z_index.saturating_add(1),
    );

    ElementBuilder::container(id)
        .style(ElementStyle {
            transparent_to_input: config.transparent_to_input,
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(config.viewport.width.max(0.0)),
            height: LayoutSizing::Fixed(config.viewport.height.max(0.0)),
            clip_x: config.clip,
            clip_y: config.clip,
            layer: UiLayer::TopLayer,
            z_index: config.z_index,
            ..LayoutInput::default()
        })
        .child(content)
        .build()
}

fn constrained_content_size(
    content_size: Size,
    anchor: Rect,
    viewport: Size,
    options: FloatingOptions,
) -> (Size, bool) {
    let mut size = Size::new(content_size.width.max(0.0), content_size.height.max(0.0));
    if options.match_anchor_width {
        size.width = anchor.size.width.max(0.0);
    }

    if !options.constrain_to_viewport {
        return (size, false);
    }

    let max_width = (viewport.width - options.viewport_margin * 2.0).max(0.0);
    let max_height = (viewport.height - options.viewport_margin * 2.0).max(0.0);
    let constrained = size.width > max_width || size.height > max_height;
    size.width = size.width.min(max_width);
    size.height = size.height.min(max_height);
    (size, constrained)
}

fn place_rect(anchor: Rect, content_size: Size, placement: FloatingPlacement, offset: f32) -> Rect {
    let offset = offset.max(0.0);
    let origin = match placement.side {
        FloatingSide::Top => glam::Vec2::new(
            aligned_cross_axis(anchor, content_size, placement),
            anchor.origin.y - offset - content_size.height,
        ),
        FloatingSide::Right => glam::Vec2::new(
            anchor.right() + offset,
            aligned_cross_axis(anchor, content_size, placement),
        ),
        FloatingSide::Bottom => glam::Vec2::new(
            aligned_cross_axis(anchor, content_size, placement),
            anchor.bottom() + offset,
        ),
        FloatingSide::Left => glam::Vec2::new(
            anchor.origin.x - offset - content_size.width,
            aligned_cross_axis(anchor, content_size, placement),
        ),
    };
    Rect {
        origin,
        size: content_size,
    }
}

fn aligned_cross_axis(anchor: Rect, content_size: Size, placement: FloatingPlacement) -> f32 {
    let (start, anchor_extent, content_extent) = match placement.side {
        FloatingSide::Top | FloatingSide::Bottom => {
            (anchor.origin.x, anchor.size.width, content_size.width)
        }
        FloatingSide::Right | FloatingSide::Left => {
            (anchor.origin.y, anchor.size.height, content_size.height)
        }
    };

    match placement.align {
        FloatingAlign::Start => start,
        FloatingAlign::Center => start + (anchor_extent - content_extent) * 0.5,
        FloatingAlign::End => start + anchor_extent - content_extent,
    }
}

fn primary_space(
    anchor: Rect,
    side: FloatingSide,
    viewport: Size,
    options: FloatingOptions,
) -> f32 {
    let margin = options.viewport_margin;
    let offset = options.offset;
    match side {
        FloatingSide::Top => anchor.origin.y - margin - offset,
        FloatingSide::Right => viewport.width - margin - anchor.right() - offset,
        FloatingSide::Bottom => viewport.height - margin - anchor.bottom() - offset,
        FloatingSide::Left => anchor.origin.x - margin - offset,
    }
    .max(0.0)
}

fn primary_overflow(rect: Rect, side: FloatingSide, viewport: Size, margin: f32) -> f32 {
    match side {
        FloatingSide::Top => (margin - rect.origin.y).max(0.0),
        FloatingSide::Right => (rect.right() - (viewport.width - margin)).max(0.0),
        FloatingSide::Bottom => (rect.bottom() - (viewport.height - margin)).max(0.0),
        FloatingSide::Left => (margin - rect.origin.x).max(0.0),
    }
}

fn clamp_rect(rect: Rect, viewport: Size, margin: f32) -> (Rect, bool) {
    let min_x = margin;
    let min_y = margin;
    let max_x = (viewport.width - margin - rect.size.width).max(min_x);
    let max_y = (viewport.height - margin - rect.size.height).max(min_y);
    let x = rect.origin.x.clamp(min_x, max_x);
    let y = rect.origin.y.clamp(min_y, max_y);
    let clamped = x != rect.origin.x || y != rect.origin.y;
    (Rect::new(x, y, rect.size.width, rect.size.height), clamped)
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
    use crate::Element;

    #[test]
    fn floating_layout_places_bottom_start_from_anchor() {
        let anchor = Rect::new(50.0, 20.0, 80.0, 24.0);
        let layout = FloatingLayout::compute(
            anchor,
            Size::new(100.0, 60.0),
            Size::new(400.0, 300.0),
            FloatingOptions::default().offset(4.0),
        );

        assert_eq!(
            layout.placement,
            FloatingPlacement::bottom(FloatingAlign::Start)
        );
        assert_eq!(layout.rect, Rect::new(50.0, 48.0, 100.0, 60.0));
        assert!(!layout.flipped);
        assert!(!layout.clamped);
    }

    #[test]
    fn floating_layout_flips_when_primary_side_overflows() {
        let anchor = Rect::new(50.0, 260.0, 80.0, 24.0);
        let layout = FloatingLayout::compute(
            anchor,
            Size::new(120.0, 80.0),
            Size::new(300.0, 300.0),
            FloatingOptions::default().offset(4.0).viewport_margin(8.0),
        );

        assert_eq!(
            layout.requested_placement,
            FloatingPlacement::bottom(FloatingAlign::Start)
        );
        assert_eq!(
            layout.placement,
            FloatingPlacement::top(FloatingAlign::Start)
        );
        assert_eq!(layout.rect, Rect::new(50.0, 176.0, 120.0, 80.0));
        assert!(layout.flipped);
        assert!(!layout.clamped);
    }

    #[test]
    fn floating_layout_clamps_secondary_axis_inside_viewport() {
        let anchor = Rect::new(250.0, 40.0, 40.0, 20.0);
        let layout = FloatingLayout::compute(
            anchor,
            Size::new(100.0, 60.0),
            Size::new(300.0, 240.0),
            FloatingOptions::default()
                .offset(4.0)
                .viewport_margin(8.0)
                .collision(FloatingCollision::Clamp),
        );

        assert_eq!(layout.rect, Rect::new(192.0, 64.0, 100.0, 60.0));
        assert!(layout.clamped);
        assert!(!layout.flipped);
    }

    #[test]
    fn floating_layout_can_match_anchor_width_and_constrain_size() {
        let anchor = Rect::new(16.0, 20.0, 96.0, 20.0);
        let layout = FloatingLayout::compute(
            anchor,
            Size::new(260.0, 400.0),
            Size::new(160.0, 180.0),
            FloatingOptions::default()
                .match_anchor_width(true)
                .viewport_margin(10.0),
        );

        assert_eq!(layout.rect.size, Size::new(96.0, 160.0));
        assert!(layout.constrained);
    }

    #[test]
    fn anchored_floating_layer_builds_absolute_top_layer_content() {
        let id = ElementId::new("floating-host");
        let mut content = Element::new(ElementId::new("menu"));
        content.layout.z_index = 2;
        let element = anchored_floating_layer(
            id,
            FloatingLayerConfig::new(
                Size::new(300.0, 200.0),
                Rect::new(20.0, 30.0, 80.0, 20.0),
                Size::new(120.0, 90.0),
            )
            .z_index(40),
            content,
        );

        assert_eq!(element.layout.layer, UiLayer::TopLayer);
        assert_eq!(element.layout.z_index, 40);
        assert!(element.style.transparent_to_input);
        assert_eq!(element.children.len(), 1);
        assert_eq!(element.children[0].layout.layer, UiLayer::TopLayer);
        assert_eq!(element.children[0].layout.z_index, 43);
        assert_eq!(
            element.children[0].layout.position,
            LayoutPosition::Absolute {
                offset: glam::Vec2::new(20.0, 54.0)
            }
        );
        assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(120.0));
        assert_eq!(element.children[0].layout.height, LayoutSizing::Fixed(90.0));
    }
}
