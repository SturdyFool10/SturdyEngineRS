use clay_ui::{
    Edges, Element, ElementBuilder, ElementId, ElementKind, ElementStyle, LayoutCache,
    LayoutDirection, LayoutInput, LayoutSizing, LayoutTextCacheStats, LayoutTree, Size, TextStyle,
    TextWrap, UiColor, UiTree, radii_all,
};
use glam::Vec2;
use sturdy_engine::{
    DebugOverlay, DebugOverlayRenderer, Engine, EngineApp, Result, ShellFrame, Surface,
    SurfaceImage, TextDrawDesc, TextPlacement, TextTypography, WindowConfig,
};
const NAV_ITEMS: [&str; 4] = ["Overview", "Scenes", "Profiler", "Assets"];

struct UiDemo {
    overlay: DebugOverlayRenderer,
    layout_cache: LayoutCache,
    text_stats: LayoutTextCacheStats,
    selected_nav: usize,
    log_scroll: f32,
}

impl EngineApp for UiDemo {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, _surface: &Surface) -> Result<Self> {
        Ok(Self {
            overlay: DebugOverlayRenderer::new(engine)?,
            layout_cache: LayoutCache::default(),
            text_stats: LayoutTextCacheStats::default(),
            selected_nav: 0,
            log_scroll: 0.0,
        })
    }

    fn render(&mut self, frame: &mut ShellFrame<'_>, surface_image: &SurfaceImage) -> Result<()> {
        let ext = surface_image.desc().extent;
        let viewport = Size::new(ext.width as f32, ext.height as f32);
        let tree = build_ui_tree(
            viewport,
            self.selected_nav,
            self.log_scroll,
            self.text_stats,
        );
        self.layout_cache.reset_text_stats();
        let layout = LayoutTree::compute(&tree.roots[0], viewport, &mut self.layout_cache)
            .map_err(|error| {
                sturdy_engine::Error::InvalidInput(format!("ui layout failed: {error:?}"))
            })?;
        self.text_stats = self.layout_cache.text_stats();
        let swapchain = frame.inner().swapchain_image(surface_image)?;
        let mut overlay = DebugOverlay::new();
        overlay.filled_rect_screen(
            ext.width,
            ext.height,
            [0.0, 0.0],
            [ext.width as f32, ext.height as f32],
            [0.012, 0.024, 0.047, 1.0],
        );
        for root in &tree.roots {
            append_element(
                &mut overlay,
                ext.width,
                ext.height,
                root,
                &layout,
                Vec2::ZERO,
                None,
            );
        }
        self.overlay
            .draw(frame.inner(), &swapchain, ext.width, ext.height, &overlay)?;
        frame.inner().present_image(&swapchain)?;
        Ok(())
    }

    fn resize(&mut self, _width: u32, _height: u32) -> Result<()> {
        Ok(())
    }

    fn key_pressed(&mut self, key: &str, _surface: &mut Surface) -> Result<()> {
        match key {
            "1" => self.selected_nav = 0,
            "2" => self.selected_nav = 1,
            "3" => self.selected_nav = 2,
            "4" => self.selected_nav = 3,
            "J" | "j" => self.log_scroll = (self.log_scroll + 24.0).min(360.0),
            "K" | "k" => self.log_scroll = (self.log_scroll - 24.0).max(0.0),
            _ => {}
        }
        Ok(())
    }
}

fn build_ui_tree(
    viewport: Size,
    selected_nav: usize,
    log_scroll: f32,
    text_stats: LayoutTextCacheStats,
) -> UiTree {
    let mut tree = UiTree::new();
    let header_height = 84.0;
    let section_gap = 14.0;
    let body_height = (viewport.height - header_height - section_gap).max(1.0);
    let sidebar_width = 240.0;
    let content_width = (viewport.width - sidebar_width - section_gap).max(1.0);
    let sidebar_inner_width = (sidebar_width - 32.0).max(1.0);
    let content_inner_width = (content_width - 36.0).max(1.0);
    let metrics_height = 114.0;
    let inspector_height = 120.0;
    let text_quality_height = 172.0;
    let content_gap = 16.0;
    let log_height = (body_height
        - 36.0
        - metrics_height
        - inspector_height
        - text_quality_height
        - content_gap * 3.0)
        .max(180.0);
    let log_viewport_height = (log_height - 70.0).max(120.0);
    let root_id = ElementId::new("root");
    let header_id = ElementId::local("header", 0, &root_id);
    let body_id = ElementId::local("body", 0, &root_id);
    let sidebar_id = ElementId::local("sidebar", 0, &body_id);
    let content_id = ElementId::local("content", 0, &body_id);

    let header = ElementBuilder::container(header_id.clone())
        .style(panel_style(
            [12, 18, 28, 255],
            [255, 255, 255, 24],
            24.0,
            Edges::symmetric(20.0, 18.0),
        ))
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(viewport.width),
            height: LayoutSizing::Fixed(header_height),
            direction: LayoutDirection::TopToBottom,
            gap: 4.0,
            ..LayoutInput::default()
        })
        .child(text_element(
            &header_id,
            "title",
            "SturdyEngine UI Demo",
            28.0,
            [242, 247, 252, 255],
        ))
        .child(text_element(
            &header_id,
            "subtitle",
            "Real clay-ui layout -> render commands -> overlay renderer",
            15.0,
            [137, 161, 184, 255],
        ))
        .build();

    let sidebar = ElementBuilder::container(sidebar_id.clone())
        .style(panel_style(
            [11, 16, 25, 255],
            [255, 255, 255, 18],
            18.0,
            Edges::all(16.0),
        ))
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(240.0),
            height: LayoutSizing::Fixed(body_height),
            direction: LayoutDirection::TopToBottom,
            gap: 10.0,
            ..LayoutInput::default()
        })
        .child(section_label(&sidebar_id, "help-label", "Input"))
        .child(help_panel(&sidebar_id))
        .child(section_label(&sidebar_id, "nav-label", "Navigation"))
        .child(nav_column(&sidebar_id, selected_nav, sidebar_inner_width))
        .build();

    let content = ElementBuilder::container(content_id.clone())
        .style(panel_style(
            [7, 12, 20, 255],
            [255, 255, 255, 14],
            18.0,
            Edges::all(18.0),
        ))
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(content_width),
            height: LayoutSizing::Fixed(body_height),
            direction: LayoutDirection::TopToBottom,
            gap: 16.0,
            ..LayoutInput::default()
        })
        .child(metrics_row(&content_id, content_inner_width, text_stats))
        .child(inspector_panel(
            &content_id,
            selected_nav,
            content_inner_width,
            inspector_height,
        ))
        .child(text_quality_panel(
            &content_id,
            content_inner_width,
            text_quality_height,
        ))
        .child(log_panel(
            &content_id,
            log_scroll,
            content_inner_width,
            log_height,
            log_viewport_height,
        ))
        .build();

    let body = ElementBuilder::container(body_id.clone())
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(viewport.width),
            height: LayoutSizing::Fixed(body_height),
            direction: LayoutDirection::LeftToRight,
            gap: 14.0,
            ..LayoutInput::default()
        })
        .child(sidebar)
        .child(content)
        .build();

    let root = ElementBuilder::container(root_id.clone())
        .style(ElementStyle {
            background: rgba([3, 6, 12, 255]),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(viewport.width),
            height: LayoutSizing::Fixed(viewport.height),
            direction: LayoutDirection::TopToBottom,
            gap: section_gap,
            ..LayoutInput::default()
        })
        .child(header)
        .child(body)
        .build();

    tree.push_root(root);
    tree
}

fn nav_column(parent: &ElementId, selected_nav: usize, width: f32) -> clay_ui::Element {
    let nav_id = ElementId::local("nav-column", 0, parent);
    let mut builder = ElementBuilder::container(nav_id.clone()).layout(LayoutInput {
        width: LayoutSizing::Fixed(width),
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        gap: 8.0,
        ..LayoutInput::default()
    });

    for (index, label) in NAV_ITEMS.iter().enumerate() {
        let item_id = ElementId::local("nav-item", index as u32, &nav_id);
        let selected = index == selected_nav;
        let mut style = panel_style(
            if selected {
                [31, 81, 150, 255]
            } else {
                [16, 22, 34, 255]
            },
            if selected {
                [126, 185, 255, 255]
            } else {
                [255, 255, 255, 22]
            },
            12.0,
            Edges::symmetric(14.0, 12.0),
        );
        style.outline_width = Edges::all(if selected { 2.0 } else { 1.0 });
        builder = builder.child(
            ElementBuilder::container(item_id.clone())
                .style(style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(width),
                    height: LayoutSizing::Fixed(52.0),
                    ..LayoutInput::default()
                })
                .child(text_element(
                    &item_id,
                    "label",
                    *label,
                    18.0,
                    if selected {
                        [244, 249, 255, 255]
                    } else {
                        [191, 205, 219, 255]
                    },
                ))
                .build(),
        );
    }

    builder.build()
}

fn help_panel(parent: &ElementId) -> clay_ui::Element {
    let panel_id = ElementId::local("help-panel", 0, parent);
    ElementBuilder::container(panel_id.clone())
        .style(panel_style(
            [10, 14, 20, 255],
            [255, 255, 255, 18],
            14.0,
            Edges::all(14.0),
        ))
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(208.0),
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            gap: 8.0,
            ..LayoutInput::default()
        })
        .child(text_element(
            &panel_id,
            "line-1",
            "1-4 switch sections",
            15.0,
            [225, 232, 239, 255],
        ))
        .child(text_element(
            &panel_id,
            "line-2",
            "J / K scroll the event log",
            15.0,
            [225, 232, 239, 255],
        ))
        .child(text_element(
            &panel_id,
            "line-3",
            "Rounded borders and clipping are active here",
            14.0,
            [132, 149, 167, 255],
        ))
        .build()
}

fn metrics_row(
    parent: &ElementId,
    width: f32,
    text_stats: LayoutTextCacheStats,
) -> clay_ui::Element {
    let row_id = ElementId::local("metrics-row", 0, parent);
    let cards = [
        (
            "Text Cache",
            format!("{} / {}", text_stats.hits, text_stats.misses),
            [78, 201, 176, 255],
        ),
        ("Visible Passes", "27".to_string(), [110, 168, 254, 255]),
        ("Transient MB", "192".to_string(), [255, 183, 77, 255]),
    ];

    let mut builder = ElementBuilder::container(row_id.clone()).layout(LayoutInput {
        width: LayoutSizing::Fixed(width),
        height: LayoutSizing::Fixed(114.0),
        direction: LayoutDirection::LeftToRight,
        gap: 14.0,
        ..LayoutInput::default()
    });

    for (index, (title, value, accent)) in cards.iter().enumerate() {
        let card_id = ElementId::local("metric-card", index as u32, &row_id);
        let mut style = panel_style(
            [12, 18, 28, 255],
            [255, 255, 255, 20],
            16.0,
            Edges::all(14.0),
        );
        style.outline_width = Edges::all(1.0);
        builder = builder.child(
            ElementBuilder::container(card_id.clone())
                .style(style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed((width - 28.0) / 3.0),
                    height: LayoutSizing::Fixed(114.0),
                    direction: LayoutDirection::TopToBottom,
                    gap: 6.0,
                    ..LayoutInput::default()
                })
                .child(text_element(
                    &card_id,
                    "title",
                    *title,
                    14.0,
                    [135, 153, 171, 255],
                ))
                .child(text_element(&card_id, "value", value, 28.0, *accent))
                .child(text_element(
                    &card_id,
                    "hint",
                    if index == 0 {
                        "hits / misses"
                    } else {
                        "live UI card"
                    },
                    13.0,
                    [105, 120, 136, 255],
                ))
                .build(),
        );
    }

    builder.build()
}

fn inspector_panel(
    parent: &ElementId,
    selected_nav: usize,
    width: f32,
    height: f32,
) -> clay_ui::Element {
    let panel_id = ElementId::local("inspector-panel", 0, parent);
    let selected = NAV_ITEMS[selected_nav];
    ElementBuilder::container(panel_id.clone())
        .style(panel_style(
            [10, 15, 23, 255],
            [255, 255, 255, 18],
            18.0,
            Edges::all(16.0),
        ))
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(width),
            height: LayoutSizing::Fixed(height),
            direction: LayoutDirection::TopToBottom,
            gap: 10.0,
            ..LayoutInput::default()
        })
        .child(section_label(&panel_id, "section", selected))
        .child(text_element(
            &panel_id,
            "description",
            match selected {
                "Overview" => "Dashboard composition with nested cards, labels, and badges.",
                "Scenes" => "Hierarchy-style layout with panels and rounded outlines.",
                "Profiler" => "Dense metric rows and a clipped scrolling event feed.",
                _ => "Asset-browser style blocks with persistent status metadata.",
            },
            16.0,
            [208, 217, 226, 255],
        ))
        .child(status_badges(&panel_id))
        .build()
}

fn status_badges(parent: &ElementId) -> clay_ui::Element {
    let row_id = ElementId::local("badges", 0, parent);
    let badges = [
        ("Stable", [37, 99, 53, 255], [134, 239, 172, 255]),
        ("Input Ready", [29, 78, 216, 255], [147, 197, 253, 255]),
        ("AA On", [120, 53, 15, 255], [253, 186, 116, 255]),
    ];

    let mut builder = ElementBuilder::container(row_id.clone()).layout(LayoutInput {
        width: LayoutSizing::Grow {
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
    });

    for (index, (label, bg, fg)) in badges.iter().enumerate() {
        let badge_id = ElementId::local("badge", index as u32, &row_id);
        let mut style = panel_style(*bg, *fg, 999.0, Edges::symmetric(12.0, 8.0));
        style.outline_width = Edges::all(1.0);
        builder = builder.child(
            ElementBuilder::container(badge_id.clone())
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
                .child(text_element_nowrap(&badge_id, "text", *label, 14.0, *fg))
                .build(),
        );
    }

    builder.build()
}

fn text_quality_panel(parent: &ElementId, width: f32, height: f32) -> clay_ui::Element {
    let panel_id = ElementId::local("text-quality-panel", 0, parent);
    let left_id = ElementId::local("text-quality-left", 0, &panel_id);
    let right_id = ElementId::local("text-quality-right", 0, &panel_id);
    let column_width = ((width - 44.0) * 0.5).max(1.0);

    let left = ElementBuilder::container(left_id.clone())
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(column_width),
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            gap: 8.0,
            ..LayoutInput::default()
        })
        .child(text_element(
            &left_id,
            "small-label",
            "Small text: 12px/16px should stay crisp over dark UI chrome.",
            12.0,
            [231, 238, 246, 255],
        ))
        .child(text_element(
            &left_id,
            "dense-row",
            "Dense row  A1: glyphs=128  atlas=2  upload=dirty",
            14.0,
            [174, 190, 206, 255],
        ))
        .child(text_element(
            &left_id,
            "large-display",
            "Display Text 48",
            32.0,
            [110, 231, 183, 255],
        ))
        .build();

    let right = ElementBuilder::container(right_id.clone())
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(column_width),
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            gap: 8.0,
            ..LayoutInput::default()
        })
        .child(text_element(
            &right_id,
            "fallback-script",
            "Fallback: Latin + CJK 漢字かな + accents cafe\u{0301}",
            15.0,
            [219, 226, 235, 255],
        ))
        .child(text_element(
            &right_id,
            "bidi",
            "Bidi sample: UI 123 שלום مرحبا",
            15.0,
            [186, 201, 219, 255],
        ))
        .child(text_element(
            &right_id,
            "emoji-symbols",
            "Symbols: ✓ ★ ◆ → ←  fraction 1/2",
            15.0,
            [253, 202, 112, 255],
        ))
        .build();

    ElementBuilder::container(panel_id.clone())
        .style(panel_style(
            [8, 13, 20, 255],
            [126, 185, 255, 42],
            18.0,
            Edges::all(16.0),
        ))
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(width),
            height: LayoutSizing::Fixed(height),
            direction: LayoutDirection::TopToBottom,
            gap: 12.0,
            ..LayoutInput::default()
        })
        .child(section_label(
            &panel_id,
            "text-quality-title",
            "Text Quality Validation",
        ))
        .child(
            ElementBuilder::container(ElementId::local("text-quality-row", 0, &panel_id))
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(width - 32.0),
                    height: LayoutSizing::Fit {
                        min: 0.0,
                        max: f32::INFINITY,
                    },
                    direction: LayoutDirection::LeftToRight,
                    gap: 12.0,
                    ..LayoutInput::default()
                })
                .child(left)
                .child(right)
                .build(),
        )
        .build()
}

fn log_panel(
    parent: &ElementId,
    log_scroll: f32,
    width: f32,
    height: f32,
    viewport_height: f32,
) -> clay_ui::Element {
    let panel_id = ElementId::local("log-panel", 0, parent);
    let list_id = ElementId::local("log-list", 0, &panel_id);
    let mut list = ElementBuilder::container(list_id.clone()).layout(LayoutInput {
        width: LayoutSizing::Fixed((width - 16.0).max(1.0)),
        height: LayoutSizing::Fit {
            min: 0.0,
            max: f32::INFINITY,
        },
        direction: LayoutDirection::TopToBottom,
        gap: 8.0,
        scroll_offset: Vec2::new(0.0, -log_scroll),
        ..LayoutInput::default()
    });

    for index in 0..18 {
        let row_id = ElementId::local("log-row", index as u32, &list_id);
        let mut style = panel_style(
            if index % 2 == 0 {
                [12, 18, 27, 255]
            } else {
                [9, 14, 21, 255]
            },
            [255, 255, 255, 10],
            10.0,
            Edges::symmetric(12.0, 10.0),
        );
        style.outline_width = Edges::all(1.0);
        list = list.child(
            ElementBuilder::container(row_id.clone())
                .style(style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed((width - 16.0).max(1.0)),
                    height: LayoutSizing::Fixed(48.0),
                    ..LayoutInput::default()
                })
                .child(text_element(
                    &row_id,
                    "line",
                    format!(
                        "#{index:02}  ui.batch={}  text.scene={}  clip={}",
                        3 + index,
                        1 + (index % 4),
                        if index % 3 == 0 { "on" } else { "off" }
                    ),
                    15.0,
                    [214, 223, 232, 255],
                ))
                .build(),
        );
    }

    ElementBuilder::container(panel_id.clone())
        .style(panel_style(
            [8, 12, 18, 255],
            [255, 255, 255, 18],
            18.0,
            Edges::all(16.0),
        ))
        .layout(LayoutInput {
            width: LayoutSizing::Fixed(width),
            height: LayoutSizing::Fixed(height),
            direction: LayoutDirection::TopToBottom,
            gap: 12.0,
            ..LayoutInput::default()
        })
        .child(section_label(&panel_id, "log-title", "Clipped Event Log"))
        .child(
            ElementBuilder::container(ElementId::local("log-viewport", 0, &panel_id))
                .style(panel_style(
                    [5, 8, 13, 255],
                    [255, 255, 255, 12],
                    14.0,
                    Edges::all(8.0),
                ))
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(width - 32.0),
                    height: LayoutSizing::Fixed(viewport_height),
                    clip_y: true,
                    ..LayoutInput::default()
                })
                .child(list.build())
                .build(),
        )
        .build()
}

fn section_label(parent: &ElementId, label: &str, text: &str) -> clay_ui::Element {
    text_element(parent, label, text, 13.0, [122, 143, 164, 255])
}

fn text_element(
    parent: &ElementId,
    label: &str,
    text: impl Into<String>,
    size: f32,
    color: [u8; 4],
) -> clay_ui::Element {
    let style = TextStyle {
        font_size: size,
        line_height: size * 1.35,
        color: rgba(color),
        ..TextStyle::default()
    };
    ElementBuilder::text(ElementId::local(label, 0, parent), text, style).build()
}

fn text_element_nowrap(
    parent: &ElementId,
    label: &str,
    text: impl Into<String>,
    size: f32,
    color: [u8; 4],
) -> clay_ui::Element {
    let mut element = text_element(parent, label, text, size, color);
    if let ElementKind::Text(text) = &mut element.kind {
        text.style.wrap = TextWrap::None;
    }
    element
}

fn panel_style(background: [u8; 4], outline: [u8; 4], radius: f32, padding: Edges) -> ElementStyle {
    ElementStyle {
        background: rgba(background),
        outline: rgba(outline),
        outline_width: Edges::all(1.0),
        corner_radius: radii_all(radius),
        padding,
        ..ElementStyle::default()
    }
}

fn rgba(color: [u8; 4]) -> UiColor {
    UiColor::from_rgba8(color[0], color[1], color[2], color[3])
}

fn append_element(
    overlay: &mut DebugOverlay,
    width: u32,
    height: u32,
    element: &Element,
    layout: &LayoutTree,
    parent_origin: Vec2,
    clip: Option<clay_ui::Rect>,
) {
    let Some(node) = layout.by_id(&element.id) else {
        return;
    };
    let mut rect = node.rect;
    rect.origin += parent_origin;
    let clip = if element.layout.clip_x || element.layout.clip_y {
        Some(if let Some(parent_clip) = clip {
            intersect_rect(parent_clip, rect)
        } else {
            rect
        })
    } else {
        clip
    };

    if let Some(rect) = clipped_rect(rect, clip) {
        if element.style.background.is_visible() {
            let radius = element.style.corner_radius.x.max(0.0);
            if radius > 0.0 {
                overlay.filled_rounded_rect_screen(
                    width,
                    height,
                    [rect.origin.x, rect.origin.y],
                    [rect.size.width, rect.size.height],
                    radius,
                    element.style.background.to_f32_array(),
                );
            } else {
                overlay.filled_rect_screen(
                    width,
                    height,
                    [rect.origin.x, rect.origin.y],
                    [rect.size.width, rect.size.height],
                    element.style.background.to_f32_array(),
                );
            }
        }

        if let ElementKind::Text(text) = &element.kind {
            let typography = TextTypography::default()
                .font_size(text.style.font_size)
                .line_height(text.style.line_height);
            let desc = TextDrawDesc::new(text.text.clone())
                .placement(TextPlacement::Screen2d {
                    x: rect.origin.x,
                    y: rect.origin.y,
                })
                .typography(typography)
                .color(text.style.color.to_f32_array());
            let desc = if text.style.wrap == TextWrap::Words {
                desc.max_width(rect.size.width.max(1.0))
            } else {
                desc
            };
            overlay.add_text(desc);
        }

        for child in &element.children {
            append_element(overlay, width, height, child, layout, rect.origin, clip);
        }

        if element.style.outline.is_visible() && edge_max(element.style.outline_width) > 0.0 {
            overlay.rounded_rectangle_outline_screen(
                width,
                height,
                [rect.origin.x, rect.origin.y],
                [rect.size.width, rect.size.height],
                element.style.corner_radius.x.max(0.0),
                edge_max(element.style.outline_width).max(1.0),
                element.style.outline.to_f32_array(),
            );
        }
    }
}

fn clipped_rect(rect: clay_ui::Rect, clip: Option<clay_ui::Rect>) -> Option<clay_ui::Rect> {
    let clipped = if let Some(clip) = clip {
        intersect_rect(rect, clip)
    } else {
        rect
    };
    (clipped.size.width > 0.0 && clipped.size.height > 0.0).then_some(clipped)
}

fn intersect_rect(a: clay_ui::Rect, b: clay_ui::Rect) -> clay_ui::Rect {
    let left = a.origin.x.max(b.origin.x);
    let top = a.origin.y.max(b.origin.y);
    let right = a.right().min(b.right());
    let bottom = a.bottom().min(b.bottom());
    let width = (right - left).max(0.0);
    let height = (bottom - top).max(0.0);
    clay_ui::Rect::new(left, top, width, height)
}

fn edge_max(edges: Edges) -> f32 {
    edges.left.max(edges.right).max(edges.top).max(edges.bottom)
}

fn main() {
    sturdy_engine::run::<UiDemo>(
        WindowConfig::new("SturdyEngine UI Demo", 1280, 800).with_resizable(true),
    );
}
