//! Mouse-responsive widget demo.
//!
//! Three tabs show live hover/press/drag/scroll/click behaviour.
//! A status bar at the bottom shows cursor position and the hovered element.

use clay_ui::{
    Axis, Edges, Element, ElementBuilder, ElementId, ElementKind, ElementStyle, LayoutCache,
    LayoutDirection, LayoutInput, LayoutSizing, LayoutTree, ListItemSpec, ScrollConfig, Size,
    SliderConfig, StatusBarSectionSpec, TabSpec, TextWrap, UiColor, UiTree, VirtualListConfig,
    WidgetBehavior, WidgetPalette, WidgetState, WindowLogicalPx, button, checkbox, label,
    list_item, radio, slider, status_bar_with_palette, tab_bar, toggle, virtual_list,
};
use glam::Vec2;
use sturdy_engine::{
    DebugOverlay, DebugOverlayRenderer, Engine, EngineApp, InputHub, KeyInput, Result, ShellFrame,
    Surface, SurfaceImage, TextDrawDesc, TextPlacement, TextTypography, WindowConfig,
};

// ── Stable element IDs ────────────────────────────────────────────────────────

fn tab_id(i: usize) -> ElementId {
    ElementId::local("tab", i as u32, &ElementId::new("tabbar"))
}
fn btn_id(i: usize) -> ElementId {
    ElementId::local("btn", i as u32, &ElementId::new("btns"))
}
fn checkbox_id(i: usize) -> ElementId {
    ElementId::local("cb", i as u32, &ElementId::new("ctrls"))
}
fn toggle_id() -> ElementId {
    ElementId::new("toggle")
}
fn radio_id(i: usize) -> ElementId {
    ElementId::local("radio", i as u32, &ElementId::new("ctrls"))
}
fn slider_id() -> ElementId {
    ElementId::new("slider")
}
fn list_scroll_id() -> ElementId {
    ElementId::new("list-scroll")
}
fn list_item_id(i: usize) -> ElementId {
    ElementId::local("item", i as u32, &list_scroll_id())
}

// ── App state ─────────────────────────────────────────────────────────────────

struct UiDemo {
    overlay: DebugOverlayRenderer,
    layout_cache: LayoutCache,
    palette: WidgetPalette,
    hub: InputHub,

    active_tab: usize,

    // Tab 0 — Buttons
    click_counts: [u32; 3],

    // Tab 1 — Controls
    checkboxes: [bool; 3],
    toggle_on: bool,
    radio_choice: usize,
    slider_value: f32,

    // Tab 2 — List
    list_selected: usize,
    list_scroll: f32,
}

const LIST_ITEMS: &[(&str, &str)] = &[
    ("scene_root", "Root node"),
    ("camera_main", "Main camera"),
    ("dir_light", "Directional light"),
    ("mesh_terrain", "Terrain mesh"),
    ("mesh_rocks", "Rock cluster"),
    ("particle_smoke", "Smoke FX"),
    ("ui_canvas", "HUD canvas"),
    ("audio_ambient", "Ambient audio"),
    ("post_bloom", "Bloom pass"),
    ("post_aa", "AA pass"),
    ("sky_dome", "Sky dome"),
    ("water_surface", "Water surface"),
];

impl EngineApp for UiDemo {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, _surface: &Surface) -> Result<Self> {
        let mut hub = InputHub::new();
        {
            let sim = hub.simulator_mut();
            sim.set_widget_behavior(slider_id(), WidgetBehavior::slider(Axis::Horizontal));
            sim.set_slider_config(
                slider_id(),
                SliderConfig::new(0.0, 1.0).step(0.01).track_extent(240.0),
            );
            sim.set_slider_value(&slider_id(), 0.5);
            sim.set_widget_behavior(list_scroll_id(), WidgetBehavior::scroll_area());
        }

        Ok(Self {
            overlay: DebugOverlayRenderer::new(engine)?,
            layout_cache: LayoutCache::default(),
            palette: WidgetPalette::default(),
            hub,
            active_tab: 0,
            click_counts: [0; 3],
            checkboxes: [true, false, true],
            toggle_on: true,
            radio_choice: 1,
            slider_value: 0.5,
            list_selected: 0,
            list_scroll: 0.0,
        })
    }

    fn pointer_moved(&mut self, pos: WindowLogicalPx, _surface: &mut Surface) -> Result<()> {
        self.hub.on_pointer_moved(pos);
        Ok(())
    }

    fn pointer_button(
        &mut self, pos: WindowLogicalPx, button: u8, pressed: bool, _surface: &mut Surface,
    ) -> Result<()> {
        self.hub.on_pointer_button(pos, button, pressed);
        Ok(())
    }

    fn pointer_scroll(
        &mut self, _pos: WindowLogicalPx, delta_x: f32, delta_y: f32, _surface: &mut Surface,
    ) -> Result<()> {
        self.hub.on_pointer_scroll(delta_x, delta_y);
        Ok(())
    }

    fn key_input(&mut self, input: &KeyInput, _surface: &mut Surface) -> Result<()> {
        self.hub.on_key_input(input);
        Ok(())
    }

    fn render(&mut self, frame: &mut ShellFrame<'_>, surface_image: &SurfaceImage) -> Result<()> {
        let ext = surface_image.desc().extent;
        let viewport = Size::new(ext.width as f32, ext.height as f32);

        // Build tree + layout so the hub can hit-test against real geometry.
        let tree = self.build_tree(viewport);
        let layout = LayoutTree::compute(&tree.roots[0], viewport, &mut self.layout_cache)
            .map_err(|e| sturdy_engine::Error::InvalidInput(format!("layout: {e:?}")))?;

        // Update scroll config from the actual laid-out rect each frame.
        if let Some(node) = layout.by_id(&list_scroll_id()) {
            let vp = Vec2::new(node.rect.size.width, node.rect.size.height);
            let content_h = LIST_ITEMS.len() as f32 * 36.0;
            self.hub.simulator_mut().set_scroll_config(
                list_scroll_id(),
                ScrollConfig::new(vp, Vec2::new(vp.x, content_h)),
            );
        }

        self.hub.update(&layout);

        // ── React to activations ──────────────────────────────────────────────
        for i in 0..3 {
            if self.hub.widget_state(&tab_id(i)).activated {
                self.active_tab = i;
            }
        }
        for i in 0..3 {
            if self.hub.widget_state(&btn_id(i)).activated {
                self.click_counts[i] += 1;
            }
        }
        for i in 0..3 {
            if self.hub.widget_state(&checkbox_id(i)).activated {
                self.checkboxes[i] = !self.checkboxes[i];
            }
        }
        if self.hub.widget_state(&toggle_id()).activated {
            self.toggle_on = !self.toggle_on;
        }
        for i in 0..3 {
            if self.hub.widget_state(&radio_id(i)).activated {
                self.radio_choice = i;
            }
        }
        for i in 0..LIST_ITEMS.len() {
            if self.hub.widget_state(&list_item_id(i)).activated {
                self.list_selected = i;
            }
        }
        self.slider_value = self.hub.slider_value(&slider_id());
        self.list_scroll = self.hub.scroll_offset(&list_scroll_id()).y;

        // ── Draw ──────────────────────────────────────────────────────────────
        let swapchain = frame.inner().swapchain_image(surface_image)?;
        let mut overlay = DebugOverlay::new();

        // Background.
        overlay.filled_rect_screen(
            ext.width, ext.height,
            [0.0, 0.0],
            [ext.width as f32, ext.height as f32],
            [0.007, 0.013, 0.022, 1.0],
        );

        // Widget tree.
        for root in &tree.roots {
            render_element(&mut overlay, ext.width, ext.height, root, &layout, None);
        }

        // Cursor crosshair at the current pointer position (top-left/Y-down).
        let cx = self.hub.cursor_pos().to_vec2();
        let col = [0.95, 0.65, 0.15, 0.85_f32];
        overlay.filled_rect_screen(ext.width, ext.height, [(cx.x - 8.0).max(0.0), cx.y - 0.5], [16.0, 1.0], col);
        overlay.filled_rect_screen(ext.width, ext.height, [cx.x - 0.5, (cx.y - 8.0).max(0.0)], [1.0, 16.0], col);

        self.overlay.draw(frame.inner(), &swapchain, ext.width, ext.height, &overlay)?;
        frame.inner().present_image(&swapchain)?;
        Ok(())
    }

    fn resize(&mut self, _w: u32, _h: u32) -> Result<()> {
        Ok(())
    }
}

// ── UI tree ───────────────────────────────────────────────────────────────────

impl UiDemo {
    fn build_tree(&self, viewport: Size) -> UiTree {
        let mut tree = UiTree::new();

        let root_id = ElementId::new("root");
        let tab_h = 38.0;
        let sb_h = 24.0;
        let content_h = (viewport.height - tab_h - sb_h).max(1.0);

        let tabs = self.build_tabs(ElementId::local("tabs", 0, &root_id), viewport.width);
        let content = self.build_content(
            ElementId::local("content", 0, &root_id),
            viewport.width,
            content_h,
        );
        let sb = self.build_status_bar(ElementId::local("sb", 0, &root_id), viewport.width);

        let root = ElementBuilder::container(root_id)
            .style(ElementStyle {
                background: UiColor::from_rgba8(10, 15, 24, 255),
                ..ElementStyle::default()
            })
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(viewport.width),
                height: LayoutSizing::Fixed(viewport.height),
                direction: LayoutDirection::TopToBottom,
                ..LayoutInput::default()
            })
            .child(tabs)
            .child(content)
            .child(sb)
            .build();

        tree.push_root(root);
        tree
    }

    fn build_tabs(&self, id: ElementId, width: f32) -> Element {
        let labels = ["Buttons", "Controls", "List"];
        let tabs: Vec<TabSpec> = (0..3)
            .zip(labels)
            .map(|(i, lbl)| {
                TabSpec::new(tab_id(i), lbl)
                    .selected(i == self.active_tab)
                    .state(self.hub.widget_state(&tab_id(i)))
            })
            .collect();

        ElementBuilder::container(id.clone())
            .style(ElementStyle {
                background: UiColor::from_rgba8(12, 18, 28, 255),
                outline: UiColor::from_rgba8(255, 255, 255, 18),
                outline_width: Edges { bottom: 1.0, ..Edges::ZERO },
                ..ElementStyle::default()
            })
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                direction: LayoutDirection::LeftToRight,
                ..LayoutInput::default()
            })
            .child(tab_bar(ElementId::local("bar", 0, &id), tabs, &self.palette))
            .build()
    }

    fn build_content(&self, id: ElementId, width: f32, height: f32) -> Element {
        let pad = 20.0;
        let inner_w = (width - pad * 2.0).max(1.0);
        let inner_h = (height - pad * 2.0).max(1.0);

        let body = match self.active_tab {
            0 => self.build_buttons_tab(ElementId::local("body", 0, &id), inner_w, inner_h),
            1 => self.build_controls_tab(ElementId::local("body", 0, &id), inner_w, inner_h),
            _ => self.build_list_tab(ElementId::local("body", 0, &id), inner_w, inner_h),
        };

        ElementBuilder::container(id)
            .style(ElementStyle {
                background: UiColor::from_rgba8(8, 13, 20, 255),
                padding: Edges::all(pad),
                ..ElementStyle::default()
            })
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                ..LayoutInput::default()
            })
            .child(body)
            .build()
    }

    // ── Tab 0: Buttons ────────────────────────────────────────────────────────

    fn build_buttons_tab(&self, id: ElementId, width: f32, height: f32) -> Element {
        let col_w = ((width - 16.0) * 0.5).max(1.0);

        // Left: three big buttons with live click counts.
        let btn_labels = ["Primary Action", "Secondary", "Danger"];
        let mut btn_col = ElementBuilder::container(ElementId::local("left", 0, &id))
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                direction: LayoutDirection::TopToBottom,
                gap: 10.0,
                ..LayoutInput::default()
            });

        btn_col = btn_col.child(
            label(ElementId::local("lbl", 0, &id), "Click any button:", &WidgetState::default()),
        );

        for (i, lbl) in btn_labels.iter().enumerate() {
            let bid = btn_id(i);
            let state = self.hub.widget_state(&bid);
            btn_col = btn_col.child(button(bid, *lbl, &state));
        }

        // Show click counts.
        btn_col = btn_col.child(spacer(ElementId::local("sp", 0, &id), 12.0));
        btn_col = btn_col.child(label(
            ElementId::local("counts-lbl", 0, &id),
            "Clicks recorded:",
            &WidgetState::default(),
        ));
        for (i, lbl) in btn_labels.iter().enumerate() {
            btn_col = btn_col.child(label(
                ElementId::local("cnt", i as u32, &id),
                format!("  {} → {}", lbl, self.click_counts[i]),
                &WidgetState::default(),
            ));
        }

        // Right: instructions / live state.
        let hovered_label = self
            .hub
            .hovered()
            .map(|id| id.label.clone())
            .unwrap_or_else(|| "—".into());
        let cursor = self.hub.cursor_pos();
        let any_pressed = (0..3).any(|i| self.hub.widget_state(&btn_id(i)).pressed);

        let mut right_col = ElementBuilder::container(ElementId::local("right", 0, &id))
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                direction: LayoutDirection::TopToBottom,
                gap: 8.0,
                ..LayoutInput::default()
            });

        right_col = right_col.child(label(
            ElementId::local("r0", 0, &id),
            "Live mouse state:",
            &WidgetState::default(),
        ));
        right_col = right_col.child(label(
            ElementId::local("r1", 0, &id),
            format!("  cursor  ({:.0}, {:.0})", cursor.x, cursor.y),
            &WidgetState::default(),
        ));
        right_col = right_col.child(label(
            ElementId::local("r2", 0, &id),
            format!("  hovered  {}", hovered_label),
            &WidgetState::default(),
        ));
        right_col = right_col.child(label(
            ElementId::local("r3", 0, &id),
            format!("  any btn pressed  {}", any_pressed),
            &WidgetState::default(),
        ));

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                direction: LayoutDirection::LeftToRight,
                gap: 16.0,
                ..LayoutInput::default()
            })
            .child(btn_col.build())
            .child(right_col.build())
            .build()
    }

    // ── Tab 1: Controls ───────────────────────────────────────────────────────

    fn build_controls_tab(&self, id: ElementId, width: f32, height: f32) -> Element {
        let col_w = ((width - 16.0) * 0.5).max(1.0);
        let left_id = ElementId::local("left", 0, &id);
        let right_id = ElementId::local("right", 0, &id);

        // Left: checkboxes + toggle.
        let cb_labels = ["Enable shadows", "Show wireframe", "V-Sync"];
        let mut left = ElementBuilder::container(left_id.clone())
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                direction: LayoutDirection::TopToBottom,
                gap: 10.0,
                ..LayoutInput::default()
            });

        left = left.child(label(
            ElementId::local("clbl", 0, &left_id),
            "Checkboxes (click to toggle):",
            &WidgetState::default(),
        ));
        for (i, lbl) in cb_labels.iter().enumerate() {
            let cid = checkbox_id(i);
            left = left.child(checkbox(
                cid.clone(),
                *lbl,
                self.checkboxes[i],
                &self.hub.widget_state(&cid),
            ));
        }

        left = left.child(spacer(ElementId::local("sp", 0, &left_id), 8.0));
        left = left.child(label(
            ElementId::local("tlbl", 0, &left_id),
            "Toggle:",
            &WidgetState::default(),
        ));
        left = left.child(toggle(
            toggle_id(),
            "Auto-refresh",
            self.toggle_on,
            clay_ui::ToggleAnimConfig::default(),
            &self.hub.widget_state(&toggle_id()),
        ));

        // Right: radio + slider.
        let radio_labels = ["Vulkan", "D3D12", "Metal"];
        let mut right = ElementBuilder::container(right_id.clone())
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                direction: LayoutDirection::TopToBottom,
                gap: 10.0,
                ..LayoutInput::default()
            });

        right = right.child(label(
            ElementId::local("rlbl", 0, &right_id),
            "Radio (click to select):",
            &WidgetState::default(),
        ));
        for (i, lbl) in radio_labels.iter().enumerate() {
            let rid = radio_id(i);
            right = right.child(radio(
                rid.clone(),
                *lbl,
                i == self.radio_choice,
                &self.hub.widget_state(&rid),
            ));
        }

        right = right.child(spacer(ElementId::local("sp", 0, &right_id), 8.0));
        right = right.child(label(
            ElementId::local("slbl", 0, &right_id),
            format!("Slider — drag to change ({:.0}%)", self.slider_value * 100.0),
            &WidgetState::default(),
        ));
        right = right.child(slider(
            slider_id(),
            clay_ui::DragBarAxis::Horizontal,
            self.slider_value,
            &self.hub.widget_state(&slider_id()),
        ));

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                direction: LayoutDirection::LeftToRight,
                gap: 16.0,
                ..LayoutInput::default()
            })
            .child(left.build())
            .child(right.build())
            .build()
    }

    // ── Tab 2: List ───────────────────────────────────────────────────────────

    fn build_list_tab(&self, id: ElementId, width: f32, height: f32) -> Element {
        let (selected_id, selected_sub) = LIST_ITEMS[self.list_selected];
        let mut col = ElementBuilder::container(id.clone())
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            });

        col = col.child(label(
            ElementId::local("hint", 0, &id),
            "Click to select · scroll wheel or drag to scroll",
            &WidgetState::default(),
        ));

        // Virtualised scrollable list.
        let list_viewport_h = (height - 60.0).max(40.0);
        let config = VirtualListConfig::new(LIST_ITEMS.len(), 36.0, list_viewport_h, self.list_scroll);
        let layout = config.layout();
        let items: Vec<Element> = layout
            .visible_range
            .clone()
            .map(|i| {
                let (id_str, sub) = LIST_ITEMS[i];
                list_item(
                    ListItemSpec::new(list_item_id(i), id_str)
                        .sublabel(sub)
                        .selected(i == self.list_selected)
                        .state(self.hub.widget_state(&list_item_id(i))),
                    &self.palette,
                )
            })
            .collect();

        col = col.child(virtual_list(
            list_scroll_id(),
            LayoutSizing::Fixed(width),
            layout.viewport_extent,
            &layout,
            items,
        ));

        col = col.child(label(
            ElementId::local("sel", 0, &id),
            format!("Selected: {} — {}", selected_id, selected_sub),
            &WidgetState::default(),
        ));

        col.build()
    }

    fn build_status_bar(&self, id: ElementId, width: f32) -> Element {
        let hovered = self
            .hub
            .hovered()
            .map(|e| e.label.clone())
            .unwrap_or_else(|| "—".into());
        let cursor = self.hub.cursor_pos();
        let tab_names = ["Buttons", "Controls", "List"];

        let sections = vec![
            StatusBarSectionSpec::new(ElementId::local("s0", 0, &id), "tab")
                .value(tab_names[self.active_tab]),
            StatusBarSectionSpec::new(ElementId::local("s1", 0, &id), "cursor")
                .value(format!("({:.0}, {:.0})", cursor.x, cursor.y)),
            StatusBarSectionSpec::new(ElementId::local("s2", 0, &id), "hovered")
                .value(hovered),
        ];

        let mut sb = status_bar_with_palette(id, sections, &self.palette);
        sb.layout.width = LayoutSizing::Fixed(width);
        sb
    }
}

// ── Renderer ──────────────────────────────────────────────────────────────────

fn render_element(
    overlay: &mut DebugOverlay,
    width: u32,
    height: u32,
    element: &clay_ui::Element,
    layout: &LayoutTree,
    clip: Option<clay_ui::Rect>,
) {
    let Some(node) = layout.by_id(&element.id) else {
        return;
    };
    let rect = node.rect;
    let clip = if element.layout.clip_x || element.layout.clip_y {
        Some(clip.map_or(rect, |c| intersect(c, rect)))
    } else {
        clip
    };

    if let Some(visible) = clipped(rect, clip) {
        if element.style.background.is_visible() {
            let r = element.style.corner_radius.x.max(0.0);
            if r > 0.0 {
                overlay.filled_rounded_rect_screen(
                    width, height,
                    [visible.origin.x, visible.origin.y],
                    [visible.size.width, visible.size.height],
                    r,
                    element.style.background.to_f32_array(),
                );
            } else {
                overlay.filled_rect_screen(
                    width, height,
                    [visible.origin.x, visible.origin.y],
                    [visible.size.width, visible.size.height],
                    element.style.background.to_f32_array(),
                );
            }
        }

        if let ElementKind::Text(text) = &element.kind {
            let typo = TextTypography::default()
                .font_size(text.style.font_size)
                .line_height(text.style.line_height);
            let desc = TextDrawDesc::new(text.text.clone())
                .placement(TextPlacement::Screen2d { x: rect.origin.x, y: rect.origin.y })
                .typography(typo)
                .color(text.style.color.to_f32_array());
            let desc = if text.style.wrap == TextWrap::Words {
                desc.max_width(rect.size.width.max(1.0))
            } else {
                desc
            };
            let desc = if let Some(c) = clip {
                desc.clip_rect(c.origin.x, c.origin.y, c.size.width, c.size.height)
            } else {
                desc
            };
            overlay.add_text(desc);
        }

        for child in &element.children {
            render_element(overlay, width, height, child, layout, clip);
        }

        if element.style.outline.is_visible() {
            let ow = {
                let e = element.style.outline_width;
                e.left.max(e.right).max(e.top).max(e.bottom)
            };
            if ow > 0.0 {
                overlay.rounded_rectangle_outline_screen(
                    width, height,
                    [visible.origin.x, visible.origin.y],
                    [visible.size.width, visible.size.height],
                    element.style.corner_radius.x.max(0.0),
                    ow,
                    element.style.outline.to_f32_array(),
                );
            }
        }
    }
}

fn clipped(rect: clay_ui::Rect, clip: Option<clay_ui::Rect>) -> Option<clay_ui::Rect> {
    let r = clip.map_or(rect, |c| intersect(rect, c));
    (r.size.width > 0.5 && r.size.height > 0.5).then_some(r)
}

fn intersect(a: clay_ui::Rect, b: clay_ui::Rect) -> clay_ui::Rect {
    let l = a.origin.x.max(b.origin.x);
    let t = a.origin.y.max(b.origin.y);
    let r = a.right().min(b.right());
    let bot = a.bottom().min(b.bottom());
    clay_ui::Rect::new(l, t, (r - l).max(0.0), (bot - t).max(0.0))
}

fn spacer(id: ElementId, h: f32) -> Element {
    ElementBuilder::container(id)
        .layout(LayoutInput {
            width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
            height: LayoutSizing::Fixed(h),
            ..LayoutInput::default()
        })
        .build()
}

fn main() {
    sturdy_engine::run::<UiDemo>(
        WindowConfig::new("SturdyEngine — Mouse Input Demo", 900, 700).with_resizable(true),
    );
}
