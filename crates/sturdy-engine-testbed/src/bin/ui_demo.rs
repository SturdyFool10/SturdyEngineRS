use clay_ui::{
    Axis, BadgeVariant, BreadcrumbSpec, ChipSpec, DragBarAxis, Edges, Element, ElementBuilder,
    ElementId, ElementKind, ElementStyle, InputEvent, InputSimulator, LayoutCache, LayoutDirection,
    LayoutInput, LayoutSizing, LayoutTree, ListItemSpec, LogEntrySpec, LogLevel, NotificationSpec,
    NumberInputSpec, PointerButton, PointerState, PropertyRowSpec, ScrollConfig, SegmentSpec,
    Size, SliderConfig, SortDirection, StatusBarSectionSpec, TabSpec, TableHeaderSpec,
    TextInputSpec, TextStyle, TextWrap, UiColor, UiTree, VirtualListConfig, WidgetBehavior,
    WidgetPalette, WidgetState, accordion_panel, badge, breadcrumbs, button, card, checkbox, chip,
    dialog_surface, divider, empty_state, group_box, label, list_item_with_palette, notification,
    number_input, progress_bar, property_row, radio, scroll_container_with_scrollbars, search_box,
    segmented_control, select, slider, status_bar_with_palette, tab_bar, table_header_row,
    text_input, toggle, toolbar, virtual_list, virtual_log_viewer,
};
use glam::Vec2;
use sturdy_engine::{
    DebugOverlay, DebugOverlayRenderer, Engine, EngineApp, Result, ShellFrame, Surface,
    SurfaceImage, TextDrawDesc, TextPlacement, TextTypography, WindowConfig,
};

// ── Demo state ────────────────────────────────────────────────────────────────

// ── Stable element IDs ────────────────────────────────────────────────────────
// These must match the IDs used when building widget elements so that the input
// simulator can map activations / hover / focus back to demo state.

fn tab_id(i: usize) -> ElementId {
    ElementId::local("tab", i as u32, &ElementId::new("tabs-bar"))
}
fn checkbox_a_id() -> ElementId { ElementId::new("cb-enable-shadows") }
fn checkbox_b_id() -> ElementId { ElementId::new("cb-show-wireframe") }
fn toggle_id() -> ElementId { ElementId::new("toggle-autorefresh") }
fn radio_id(i: usize) -> ElementId {
    ElementId::local("radio", i as u32, &ElementId::new("radio-group"))
}
fn segment_id(i: usize) -> ElementId {
    ElementId::local("seg", i as u32, &ElementId::new("segment-group"))
}
fn accordion_a_header_id() -> ElementId { ElementId::new("acc-a-header") }
fn accordion_b_header_id() -> ElementId { ElementId::new("acc-b-header") }
fn select_id() -> ElementId { ElementId::new("backend-select") }
fn list_item_id(i: usize) -> ElementId {
    ElementId::local("li", i as u32, &list_scroll_id())
}
fn dialog_ok_id() -> ElementId { ElementId::new("dialog-ok") }
fn dialog_cancel_id() -> ElementId { ElementId::new("dialog-cancel") }
fn slider_id() -> ElementId { ElementId::new("exposure-slider") }
fn list_scroll_id() -> ElementId { ElementId::new("scene-list") }
fn inspector_scroll_id() -> ElementId { ElementId::new("inspector-props-scroll") }
fn log_scroll_id() -> ElementId { ElementId::new("log-scroll-id") }

struct UiDemo {
    overlay: DebugOverlayRenderer,
    layout_cache: LayoutCache,
    palette: WidgetPalette,
    input: InputSimulator,
    primary_held: bool,

    // Navigation
    active_tab: usize,

    // Controls tab
    checkbox_a: bool,
    checkbox_b: bool,
    toggle_on: bool,
    slider_value: f32,
    progress_value: f32,
    radio_choice: usize,
    segment_choice: usize,

    // Inputs tab
    _search_text: String,
    field_text: String,
    number_text: String,
    select_open: bool,
    select_choice: usize,

    // Layout tab
    accordion_a_open: bool,
    accordion_b_open: bool,
    list_selected: usize,
    list_scroll: f32,

    // Inspector tab
    inspector_scroll: f32,
    log_scroll: f32,
    show_dialog: bool,
}

impl EngineApp for UiDemo {
    type Error = sturdy_engine::Error;

    fn init(engine: &Engine, _surface: &Surface) -> Result<Self> {
        let mut input = InputSimulator::default();
        // Register default behaviors so keyboard and mouse work automatically.
        input.set_widget_behavior(slider_id(), WidgetBehavior::slider(Axis::Horizontal));
        input.set_slider_config(
            slider_id(),
            SliderConfig::new(0.0, 1.0).step(0.02).track_extent(200.0),
        );
        input.set_slider_value(&slider_id(), 0.62);
        input.set_widget_behavior(list_scroll_id(), WidgetBehavior::scroll_area());
        input.set_widget_behavior(inspector_scroll_id(), WidgetBehavior::scroll_area());
        input.set_widget_behavior(log_scroll_id(), WidgetBehavior::scroll_area());

        Ok(Self {
            overlay: DebugOverlayRenderer::new(engine)?,
            layout_cache: LayoutCache::default(),
            palette: WidgetPalette::default(),
            input,
            primary_held: false,
            active_tab: 0,
            checkbox_a: true,
            checkbox_b: false,
            toggle_on: true,
            slider_value: 0.62,
            progress_value: 0.45,
            radio_choice: 1,
            segment_choice: 0,
            _search_text: String::new(),
            field_text: "Edit me".into(),
            number_text: "42".into(),
            select_open: false,
            select_choice: 0,
            accordion_a_open: true,
            accordion_b_open: false,
            list_selected: 2,
            list_scroll: 0.0,
            inspector_scroll: 0.0,
            log_scroll: 0.0,
            show_dialog: false,
        })
    }

    fn render(&mut self, frame: &mut ShellFrame<'_>, surface_image: &SurfaceImage) -> Result<()> {
        let ext = surface_image.desc().extent;
        let viewport = Size::new(ext.width as f32, ext.height as f32);

        // Build the element tree and compute layout FIRST so input.update() can
        // hit-test against the frame we are actually about to render, not last
        // frame's stale geometry.
        let tree = self.build_tree(viewport);
        let layout = LayoutTree::compute(&tree.roots[0], viewport, &mut self.layout_cache)
            .map_err(|e| sturdy_engine::Error::InvalidInput(format!("layout: {e:?}")))?;

        // Register scroll configs from actual layout rects so viewport sizes are
        // accurate.  Content sizes come from known item counts × row heights.
        let list_content_h = 12.0 * 36.0;
        let insp_content_h = 10.0 * 30.0;
        let log_content_h = 20.0 * 28.0;
        for (id, content_h) in [
            (list_scroll_id(), list_content_h),
            (inspector_scroll_id(), insp_content_h),
            (log_scroll_id(), log_content_h),
        ] {
            if let Some(node) = layout.by_id(&id) {
                let vp = Vec2::new(node.rect.size.width, node.rect.size.height);
                self.input.set_scroll_config(
                    id,
                    ScrollConfig::new(vp, Vec2::new(vp.x, content_h)),
                );
            }
        }

        // Process input against the current layout.
        self.input.update(&layout);

        // ── Read back widget activations ──────────────────────────────────────
        // Tabs
        for i in 0..4 {
            if self.input.widget_state(&tab_id(i)).activated {
                self.active_tab = i;
            }
        }
        // Checkboxes / toggle
        if self.input.widget_state(&checkbox_a_id()).activated {
            self.checkbox_a = !self.checkbox_a;
        }
        if self.input.widget_state(&checkbox_b_id()).activated {
            self.checkbox_b = !self.checkbox_b;
        }
        if self.input.widget_state(&toggle_id()).activated {
            self.toggle_on = !self.toggle_on;
        }
        // Radio
        for i in 0..3 {
            if self.input.widget_state(&radio_id(i)).activated {
                self.radio_choice = i;
            }
        }
        // Segment
        for i in 0..3 {
            if self.input.widget_state(&segment_id(i)).activated {
                self.segment_choice = i;
            }
        }
        // Accordion headers
        if self.input.widget_state(&accordion_a_header_id()).activated {
            self.accordion_a_open = !self.accordion_a_open;
        }
        if self.input.widget_state(&accordion_b_header_id()).activated {
            self.accordion_b_open = !self.accordion_b_open;
        }
        // Select trigger
        if self.input.widget_state(&select_id()).activated {
            self.select_open = !self.select_open;
        }
        // List items
        for i in 0..12 {
            if self.input.widget_state(&list_item_id(i)).activated {
                self.list_selected = i;
            }
        }
        // Dialog buttons
        if self.input.widget_state(&dialog_ok_id()).activated
            || self.input.widget_state(&dialog_cancel_id()).activated
        {
            self.show_dialog = false;
        }

        // Read back slider / scroll values driven by input behaviors.
        self.slider_value = self.input.slider_value(&slider_id());
        self.list_scroll = self.input.scroll_offset(&list_scroll_id()).y;
        self.inspector_scroll = self.input.scroll_offset(&inspector_scroll_id()).y;
        self.log_scroll = self.input.scroll_offset(&log_scroll_id()).y;

        let swapchain = frame.inner().swapchain_image(surface_image)?;
        let mut overlay = DebugOverlay::new();
        // Background
        overlay.filled_rect_screen(
            ext.width,
            ext.height,
            [0.0, 0.0],
            [ext.width as f32, ext.height as f32],
            [0.008, 0.016, 0.032, 1.0],
        );
        for root in &tree.roots {
            render_element(&mut overlay, ext.width, ext.height, root, &layout, None);
        }
        self.overlay
            .draw(frame.inner(), &swapchain, ext.width, ext.height, &overlay)?;
        frame.inner().present_image(&swapchain)?;
        Ok(())
    }

    fn resize(&mut self, _w: u32, _h: u32) -> Result<()> {
        Ok(())
    }

    fn key_pressed(&mut self, key: &str, _surface: &mut Surface) -> Result<()> {
        match key {
            "1" => self.active_tab = 0,
            "2" => self.active_tab = 1,
            "3" => self.active_tab = 2,
            "4" => self.active_tab = 3,
            "C" | "c" => self.checkbox_a = !self.checkbox_a,
            "T" | "t" => self.toggle_on = !self.toggle_on,
            "R" | "r" => self.radio_choice = (self.radio_choice + 1) % 3,
            "S" | "s" => self.segment_choice = (self.segment_choice + 1) % 3,
            "D" | "d" => self.show_dialog = !self.show_dialog,
            "O" | "o" => self.accordion_a_open = !self.accordion_a_open,
            // Keep manual +/- slider as a fallback alongside mouse drag.
            "+" | "=" => {
                self.slider_value = (self.slider_value + 0.05).min(1.0);
                self.input.set_slider_value(&slider_id(), self.slider_value);
            }
            "-" => {
                self.slider_value = (self.slider_value - 0.05).max(0.0);
                self.input.set_slider_value(&slider_id(), self.slider_value);
            }
            _ => {}
        }
        Ok(())
    }

    fn key_input(
        &mut self,
        input: &sturdy_engine::KeyInput,
        _surface: &mut Surface,
    ) -> Result<()> {
        use sturdy_engine::{KeyInputState, KeyToken};
        let name = match &input.key {
            KeyToken::Key(name) => name.clone(),
            KeyToken::Modifier(_) => return Ok(()),
        };
        let pressed = input.state == KeyInputState::Pressed;
        self.input.queue(InputEvent::Key {
            name,
            pressed,
            repeat: input.repeat,
        });
        Ok(())
    }

    fn pointer_moved(&mut self, x: f32, y: f32, _surface: &mut Surface) -> Result<()> {
        use clay_ui::InteractionPhase;
        let phase = if self.primary_held {
            InteractionPhase::Pressed
        } else {
            InteractionPhase::Released
        };
        self.input.queue(InputEvent::Pointer(PointerState {
            position: glam::Vec2::new(x, y),
            button: PointerButton::Primary,
            phase,
        }));
        Ok(())
    }

    fn pointer_button(
        &mut self,
        x: f32,
        y: f32,
        button: u8,
        pressed: bool,
        _surface: &mut Surface,
    ) -> Result<()> {
        use clay_ui::InteractionPhase;
        if button == 0 {
            self.primary_held = pressed;
        }
        let btn = match button {
            0 => PointerButton::Primary,
            1 => PointerButton::Secondary,
            2 => PointerButton::Middle,
            n => PointerButton::Extra(n),
        };
        let phase = if pressed {
            InteractionPhase::PressedThisFrame
        } else {
            InteractionPhase::ReleasedThisFrame
        };
        self.input.queue(InputEvent::Pointer(PointerState {
            position: glam::Vec2::new(x, y),
            button: btn,
            phase,
        }));
        Ok(())
    }

    fn pointer_scroll(
        &mut self,
        _x: f32,
        _y: f32,
        _delta_x: f32,
        delta_y: f32,
        _surface: &mut Surface,
    ) -> Result<()> {
        self.input.queue(InputEvent::Scroll {
            target: None,
            delta: glam::Vec2::new(0.0, delta_y),
        });
        Ok(())
    }
}

impl UiDemo {
    fn build_tree(&self, viewport: Size) -> UiTree {
        let mut tree = UiTree::new();
        let root_id = ElementId::new("root");
        let toolbar_id = ElementId::local("toolbar", 0, &root_id);
        let tabs_id = ElementId::local("tabs", 0, &root_id);
        let content_id = ElementId::local("content", 0, &root_id);
        let statusbar_id = ElementId::local("statusbar", 0, &root_id);

        let toolbar_height = 40.0;
        let tabs_height = 38.0;
        let statusbar_height = 24.0;
        let content_height =
            (viewport.height - toolbar_height - tabs_height - statusbar_height).max(1.0);

        let tb = self.build_toolbar(toolbar_id, viewport.width);
        let tabs = self.build_tab_bar(tabs_id, viewport.width);
        let content = self.build_content(content_id, viewport.width, content_height);
        let sb = self.build_status_bar(statusbar_id, viewport.width);

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
            .child(tb)
            .child(tabs)
            .child(content)
            .child(sb)
            .build();

        tree.push_root(root);

        if self.show_dialog {
            let dialog_vp_id = ElementId::new("dialog-overlay");
            let dialog_content_id = ElementId::local("dialog-content", 0, &dialog_vp_id);
            let dialog_id = ElementId::local("dialog", 0, &dialog_vp_id);
            let dialog_el = dialog_surface(
                dialog_id,
                Some("Demo Dialog"),
                Size::new(420.0, 260.0),
                [build_dialog_body(dialog_content_id, &self.palette)],
            );
            let overlay_el = ElementBuilder::container(dialog_vp_id)
                .style(ElementStyle {
                    background: UiColor::from_rgba8(0, 0, 0, 160),
                    ..ElementStyle::default()
                })
                .layout(LayoutInput {
                    width: LayoutSizing::Fixed(viewport.width),
                    height: LayoutSizing::Fixed(viewport.height),
                    align_x: clay_ui::Align::Center,
                    align_y: clay_ui::Align::Center,
                    ..LayoutInput::default()
                })
                .child(dialog_el)
                .build();
            tree.push_root(overlay_el);
        }

        tree
    }

    fn build_toolbar(&self, id: ElementId, _width: f32) -> Element {
        let title_id = ElementId::local("title", 0, &id);
        let badge_id = ElementId::local("badge", 0, &id);
        let hint_id = ElementId::local("hint", 0, &id);

        let title_el = ElementBuilder::text(
            title_id,
            "SturdyEngine Widget Demo",
            TextStyle {
                font_size: 15.0,
                line_height: 20.0,
                color: UiColor::from_rgba8(230, 238, 248, 255),
                wrap: TextWrap::None,
                ..TextStyle::default()
            },
        )
        .build();

        let hint_el = ElementBuilder::text(
            hint_id,
            "1-4 tabs · C toggle · T toggle · +/- slider · R radio · S segment · D dialog · O accordion · J/K scroll",
            TextStyle {
                font_size: 11.0,
                line_height: 14.0,
                color: UiColor::from_rgba8(100, 120, 142, 255),
                wrap: TextWrap::None,
                ..TextStyle::default()
            },
        )
        .build();

        let badge_el = badge(badge_id, "alpha", BadgeVariant::Warning);

        toolbar(
            id,
            [
                title_el,
                badge_el,
                spacer_grow(ElementId::local("spacer", 0, &ElementId::new("tb"))),
                hint_el,
            ],
        )
    }

    fn build_tab_bar(&self, id: ElementId, width: f32) -> Element {
        let labels = ["Controls", "Inputs", "Layout", "Inspector"];
        let tabs: Vec<TabSpec> = (0..4)
            .zip(labels)
            .map(|(i, label)| {
                TabSpec::new(tab_id(i), label)
                    .selected(i == self.active_tab)
                    .state(self.input.widget_state(&tab_id(i)))
            })
            .collect();

        ElementBuilder::container(id.clone())
            .style(ElementStyle {
                background: UiColor::from_rgba8(12, 18, 28, 255),
                outline: UiColor::from_rgba8(255, 255, 255, 18),
                outline_width: Edges {
                    bottom: 1.0,
                    ..Edges::ZERO
                },
                ..ElementStyle::default()
            })
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::LeftToRight,
                ..LayoutInput::default()
            })
            .child(tab_bar(ElementId::local("bar", 0, &id), tabs))
            .build()
    }

    fn build_content(&self, id: ElementId, width: f32, height: f32) -> Element {
        let inner_id = ElementId::local("inner", 0, &id);
        let padding = 16.0;
        let inner_width = (width - padding * 2.0).max(1.0);

        let body = match self.active_tab {
            0 => self.build_controls_tab(inner_id, inner_width, height - padding * 2.0),
            1 => self.build_inputs_tab(inner_id, inner_width, height - padding * 2.0),
            2 => self.build_layout_tab(inner_id, inner_width, height - padding * 2.0),
            _ => self.build_inspector_tab(inner_id, inner_width, height - padding * 2.0),
        };

        ElementBuilder::container(id)
            .style(ElementStyle {
                background: UiColor::from_rgba8(8, 13, 20, 255),
                ..ElementStyle::default()
            })
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                direction: LayoutDirection::TopToBottom,
                ..LayoutInput::default()
            })
            .child(
                ElementBuilder::container(ElementId::new("content-pad"))
                    .style(ElementStyle {
                        padding: Edges::all(padding),
                        ..ElementStyle::default()
                    })
                    .layout(LayoutInput {
                        width: LayoutSizing::Fixed(width),
                        height: LayoutSizing::Fixed(height),
                        ..LayoutInput::default()
                    })
                    .child(body)
                    .build(),
            )
            .build()
    }

    // ── Controls tab ──────────────────────────────────────────────────────────

    fn build_controls_tab(&self, id: ElementId, width: f32, height: f32) -> Element {
        let col_width = ((width - 16.0) * 0.5).max(1.0);
        let left_id = ElementId::local("left", 0, &id);
        let right_id = ElementId::local("right", 0, &id);

        let left = self.build_controls_left(left_id, col_width);
        let right = self.build_controls_right(right_id, col_width);

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                direction: LayoutDirection::LeftToRight,
                gap: 16.0,
                ..LayoutInput::default()
            })
            .child(left)
            .child(right)
            .build()
    }

    fn build_controls_left(&self, id: ElementId, width: f32) -> Element {
        let btn_id = ElementId::local("btn", 0, &id);
        let prog_id = ElementId::local("prog", 0, &id);
        let badges_id = ElementId::local("badges", 0, &id);

        // Buttons section — show live hover/press from real input state.
        let buttons_card = card(
            ElementId::local("btn-card", 0, &id),
            Some("Buttons (click me)"),
            LayoutSizing::Fixed(width),
            LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
            [ElementBuilder::container(ElementId::local("btn-row", 0, &id))
                .style(ElementStyle { padding: Edges::all(12.0), ..ElementStyle::default() })
                .layout(LayoutInput {
                    width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
                    height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                    direction: LayoutDirection::LeftToRight,
                    gap: 8.0,
                    ..LayoutInput::default()
                })
                .child(button(btn_id.clone(), "Primary", &self.input.widget_state(&btn_id)))
                .child(button(
                    ElementId::local("btn-hov", 0, &id),
                    "Hover me",
                    &self.input.widget_state(&ElementId::local("btn-hov", 0, &id)),
                ))
                .child(button(
                    ElementId::local("btn-dis", 0, &id),
                    "Disabled",
                    &WidgetState { disabled: true, ..WidgetState::default() },
                ))
                .build()],
        );

        // Checkboxes & toggle — use stable IDs so clicks are detected.
        let checks_card = card(
            ElementId::local("checks-card", 0, &id),
            Some("Checkboxes & Toggle"),
            LayoutSizing::Fixed(width),
            LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
            [ElementBuilder::container(ElementId::local("checks-body", 0, &id))
                .style(ElementStyle { padding: Edges::all(12.0), ..ElementStyle::default() })
                .layout(LayoutInput {
                    width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
                    height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                    direction: LayoutDirection::TopToBottom,
                    gap: 10.0,
                    ..LayoutInput::default()
                })
                .child(checkbox(
                    checkbox_a_id(),
                    "Enable shadows",
                    self.checkbox_a,
                    &self.input.widget_state(&checkbox_a_id()),
                ))
                .child(checkbox(
                    checkbox_b_id(),
                    "Show wireframe",
                    self.checkbox_b,
                    &self.input.widget_state(&checkbox_b_id()),
                ))
                .child(toggle(
                    toggle_id(),
                    "Auto-refresh",
                    self.toggle_on,
                    &self.input.widget_state(&toggle_id()),
                ))
                .build()],
        );

        // Slider + progress — slider uses stable slider_id() for drag tracking.
        let sliders_card = card(
            ElementId::local("sliders-card", 0, &id),
            Some("Slider & Progress  (drag or +/-)"),
            LayoutSizing::Fixed(width),
            LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
            [ElementBuilder::container(ElementId::local("sliders-body", 0, &id))
                .style(ElementStyle { padding: Edges::all(12.0), ..ElementStyle::default() })
                .layout(LayoutInput {
                    width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
                    height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                    direction: LayoutDirection::TopToBottom,
                    gap: 12.0,
                    ..LayoutInput::default()
                })
                .child(label(
                    ElementId::local("slider-lbl", 0, &id),
                    format!("Exposure: {:.0}%", self.slider_value * 100.0),
                    &WidgetState::default(),
                ))
                .child(slider(
                    slider_id(),
                    DragBarAxis::Horizontal,
                    self.slider_value,
                    &self.input.widget_state(&slider_id()),
                ))
                .child(label(
                    ElementId::local("prog-lbl", 0, &id),
                    "GPU Load",
                    &WidgetState::default(),
                ))
                .child(progress_bar(prog_id, self.progress_value, &WidgetState::default()))
                .build()],
        );

        // Badges
        let badges_card = card(
            ElementId::local("badges-card", 0, &id),
            Some("Badges"),
            LayoutSizing::Fixed(width),
            LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            [ElementBuilder::container(badges_id.clone())
                .style(ElementStyle {
                    padding: Edges::all(12.0),
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
                    gap: 8.0,
                    ..LayoutInput::default()
                })
                .child(badge(ElementId::local("b0", 0, &badges_id), "Default", BadgeVariant::Default))
                .child(badge(ElementId::local("b1", 0, &badges_id), "Success", BadgeVariant::Success))
                .child(badge(ElementId::local("b2", 0, &badges_id), "Warning", BadgeVariant::Warning))
                .child(badge(ElementId::local("b3", 0, &badges_id), "Error", BadgeVariant::Error))
                .child(badge(ElementId::local("b4", 0, &badges_id), "Info", BadgeVariant::Info))
                .build()],
        );

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(buttons_card)
            .child(checks_card)
            .child(sliders_card)
            .child(badges_card)
            .build()
    }

    fn build_controls_right(&self, id: ElementId, width: f32) -> Element {
        // Radio — use stable radio_id(i) so clicks change self.radio_choice.
        let radio_card = {
            let rc_id = ElementId::local("radio-card", 0, &id);
            let body_id = ElementId::local("radio-body", 0, &rc_id);
            let options = ["Vulkan", "D3D12", "Metal"];
            let mut body = ElementBuilder::container(body_id.clone())
                .style(ElementStyle { padding: Edges::all(12.0), ..ElementStyle::default() })
                .layout(LayoutInput {
                    width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
                    height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                    direction: LayoutDirection::TopToBottom,
                    gap: 10.0,
                    ..LayoutInput::default()
                });
            for (i, opt) in options.iter().enumerate() {
                body = body.child(radio(
                    radio_id(i),
                    *opt,
                    i == self.radio_choice,
                    &self.input.widget_state(&radio_id(i)),
                ));
            }
            card(rc_id, Some("Radio — Backend"), LayoutSizing::Fixed(width),
                LayoutSizing::Fit { min: 0.0, max: f32::INFINITY }, [body.build()])
        };

        // Segmented control — use stable segment_id(i).
        let seg_card = {
            let sc_id = ElementId::local("seg-card", 0, &id);
            let labels = ["Day", "Week", "Month"];
            let segments: Vec<SegmentSpec> = labels
                .iter()
                .enumerate()
                .map(|(i, l)| {
                    SegmentSpec::new(segment_id(i), *l)
                        .selected(i == self.segment_choice)
                        .state(self.input.widget_state(&segment_id(i)))
                })
                .collect();
            card(
                sc_id.clone(),
                Some("Segmented Control"),
                LayoutSizing::Fixed(width),
                LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                [ElementBuilder::container(ElementId::local("seg-wrap", 0, &sc_id))
                    .style(ElementStyle { padding: Edges::all(12.0), ..ElementStyle::default() })
                    .layout(LayoutInput {
                        width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
                        height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                        ..LayoutInput::default()
                    })
                    .child(segmented_control(ElementId::local("seg-ctrl", 0, &sc_id), segments))
                    .build()],
            )
        };

        // Chips
        let chips_card = {
            let cc_id = ElementId::local("chips-card", 0, &id);
            let chips_body = ElementId::local("chips-body", 0, &cc_id);
            card(
                cc_id,
                Some("Chips / Tags"),
                LayoutSizing::Fixed(width),
                LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                [ElementBuilder::container(chips_body.clone())
                    .style(ElementStyle {
                        padding: Edges::all(12.0),
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
                        gap: 6.0,
                        ..LayoutInput::default()
                    })
                    .child(chip(ChipSpec::new(ElementId::local("c0", 0, &chips_body), "Rust")))
                    .child(chip(
                        ChipSpec::new(ElementId::local("c1", 0, &chips_body), "GPU")
                            .variant(BadgeVariant::Info),
                    ))
                    .child(chip(
                        ChipSpec::new(ElementId::local("c2", 0, &chips_body), "v0.1")
                            .variant(BadgeVariant::Warning)
                            .can_remove(true),
                    ))
                    .child(chip(
                        ChipSpec::new(ElementId::local("c3", 0, &chips_body), "Stable")
                            .variant(BadgeVariant::Success)
                            .can_remove(true),
                    ))
                    .build()],
            )
        };

        // Notifications
        let notif_card = {
            let nc_id = ElementId::local("notif-card", 0, &id);
            card(
                nc_id.clone(),
                Some("Notifications"),
                LayoutSizing::Fixed(width),
                LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                [ElementBuilder::container(ElementId::local("notif-body", 0, &nc_id))
                    .style(ElementStyle {
                        padding: Edges::all(12.0),
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
                        gap: 8.0,
                        ..LayoutInput::default()
                    })
                    .child(notification(NotificationSpec::new(
                        ElementId::local("n0", 0, &nc_id),
                        "Shader compiled successfully.",
                        BadgeVariant::Success,
                    )))
                    .child(notification(
                        NotificationSpec::new(
                            ElementId::local("n1", 0, &nc_id),
                            "New engine version available.",
                            BadgeVariant::Info,
                        )
                        .action("Update"),
                    ))
                    .child(notification(NotificationSpec::new(
                        ElementId::local("n2", 0, &nc_id),
                        "Pipeline stall detected on frame 312.",
                        BadgeVariant::Warning,
                    )))
                    .build()],
            )
        };

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(radio_card)
            .child(seg_card)
            .child(chips_card)
            .child(notif_card)
            .build()
    }

    // ── Inputs tab ────────────────────────────────────────────────────────────

    fn build_inputs_tab(&self, id: ElementId, width: f32, _height: f32) -> Element {
        let col_w = ((width - 16.0) * 0.5).max(1.0);
        let left_id = ElementId::local("left", 0, &id);
        let right_id = ElementId::local("right", 0, &id);

        let search_id = ElementId::local("search", 0, &left_id);
        let field_id = ElementId::local("field", 0, &left_id);
        let pw_id = ElementId::local("pw", 0, &left_id);
        let multi_id = ElementId::local("multi", 0, &left_id);

        let focused_state = {
            let mut s = WidgetState::default();
            s.focused = true;
            s
        };
        let invalid_state = {
            let mut s = WidgetState::default();
            s.invalid = true;
            s
        };

        let text_fields_card = card(
            ElementId::local("text-card", 0, &left_id),
            Some("Text Fields"),
            LayoutSizing::Fixed(col_w),
            LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            [ElementBuilder::container(ElementId::local("tf-body", 0, &left_id))
                .style(ElementStyle {
                    padding: Edges::all(12.0),
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
                    gap: 10.0,
                    ..LayoutInput::default()
                })
                .child(label(
                    ElementId::local("sl", 0, &left_id),
                    "Search",
                    &WidgetState::default(),
                ))
                .child(search_box(
                    search_id,
                    &TextInputSpec::new("").placeholder("Search widgets…"),
                    &WidgetState::default(),
                ))
                .child(label(
                    ElementId::local("fl", 0, &left_id),
                    "Text field (focused)",
                    &WidgetState::default(),
                ))
                .child(text_input(
                    field_id,
                    &TextInputSpec::new(&self.field_text).cursor_x(62.0),
                    &focused_state,
                ))
                .child(label(
                    ElementId::local("pwl", 0, &left_id),
                    "Password (invalid)",
                    &WidgetState::default(),
                ))
                .child(text_input(
                    pw_id,
                    &TextInputSpec::new("secret123").password(true),
                    &invalid_state,
                ))
                .child(label(
                    ElementId::local("mul", 0, &left_id),
                    "Multiline",
                    &WidgetState::default(),
                ))
                .child(text_input(
                    multi_id,
                    &TextInputSpec::new("Line one\nLine two\nLine three")
                        .multiline(true),
                    &WidgetState::default(),
                ))
                .build()],
        );

        // Number + select
        let num_id = ElementId::local("num", 0, &right_id);
        let _sel_id = ElementId::local("select", 0, &right_id);
        let select_options = ["Auto", "Vulkan", "DirectX 12", "Metal", "Null"];

        let numeric_select_card = card(
            ElementId::local("num-card", 0, &right_id),
            Some("Number Input & Select"),
            LayoutSizing::Fixed(col_w),
            LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            [ElementBuilder::container(ElementId::local("ns-body", 0, &right_id))
                .style(ElementStyle {
                    padding: Edges::all(12.0),
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
                    gap: 10.0,
                    ..LayoutInput::default()
                })
                .child(label(
                    ElementId::local("numl", 0, &right_id),
                    "Frame budget (ms)",
                    &WidgetState::default(),
                ))
                .child(number_input(
                    num_id,
                    &NumberInputSpec::new(&self.number_text).unit("ms"),
                    &WidgetState::default(),
                ))
                .child(label(
                    ElementId::local("sell", 0, &right_id),
                    "Backend (press D for dialog)",
                    &WidgetState::default(),
                ))
                .child(select(
                    select_id(),
                    select_options[self.select_choice],
                    self.select_open,
                    &self.input.widget_state(&select_id()),
                ))
                .build()],
        );

        // Breadcrumbs demo
        let bc_card = {
            let bc_id = ElementId::local("bc-card", 0, &right_id);
            let bc_items = [
                ("Settings", false),
                ("Rendering", false),
                ("Post Processing", true),
            ];
            let bc_specs: Vec<BreadcrumbSpec> = bc_items
                .iter()
                .enumerate()
                .map(|(i, (label, current))| {
                    BreadcrumbSpec::new(
                        ElementId::local("bc", i as u32, &bc_id),
                        *label,
                    )
                    .current(*current)
                })
                .collect();
            card(
                bc_id.clone(),
                Some("Breadcrumbs"),
                LayoutSizing::Fixed(col_w),
                LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                [ElementBuilder::container(ElementId::local("bc-body", 0, &bc_id))
                    .style(ElementStyle {
                        padding: Edges::all(12.0),
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
                    .child(breadcrumbs(
                        ElementId::local("bc-nav", 0, &bc_id),
                        bc_specs,
                    ))
                    .build()],
            )
        };

        let left = ElementBuilder::container(left_id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(text_fields_card)
            .build();

        let right = ElementBuilder::container(right_id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(numeric_select_card)
            .child(bc_card)
            .build();

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::LeftToRight,
                gap: 16.0,
                ..LayoutInput::default()
            })
            .child(left)
            .child(right)
            .build()
    }

    // ── Layout tab ────────────────────────────────────────────────────────────

    fn build_layout_tab(&self, id: ElementId, width: f32, height: f32) -> Element {
        let col_w = ((width - 16.0) * 0.5).max(1.0);
        let left_id = ElementId::local("left", 0, &id);
        let right_id = ElementId::local("right", 0, &id);

        // Accordions — use stable header IDs so clicks toggle open/closed.
        let acc_id = ElementId::local("acc", 0, &left_id);

        let acc_a_body = ElementBuilder::container(ElementId::local("acc-a-body", 0, &acc_id))
            .style(ElementStyle {
                padding: Edges::all(12.0),
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
                gap: 8.0,
                ..LayoutInput::default()
            })
            .child(label(ElementId::local("t1", 0, &acc_id), "Anti-aliasing: MSAA 4x", &WidgetState::default()))
            .child(label(ElementId::local("t2", 0, &acc_id), "Bloom threshold: 1.2", &WidgetState::default()))
            .child(label(ElementId::local("t3", 0, &acc_id), "Tone mapping: ACES", &WidgetState::default()))
            .build();

        let acc_b_body = ElementBuilder::container(ElementId::local("acc-b-body", 0, &acc_id))
            .style(ElementStyle {
                padding: Edges::all(12.0),
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
                gap: 8.0,
                ..LayoutInput::default()
            })
            .child(label(ElementId::local("t4", 0, &acc_id), "V-Sync: Mailbox", &WidgetState::default()))
            .child(label(ElementId::local("t5", 0, &acc_id), "HDR: Enabled", &WidgetState::default()))
            .build();

        let accordions_card = card(
            ElementId::local("acc-card", 0, &left_id),
            Some("Accordions (press O)"),
            LayoutSizing::Fixed(col_w),
            LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            [ElementBuilder::container(acc_id.clone())
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
                    direction: LayoutDirection::TopToBottom,
                    gap: 4.0,
                    ..LayoutInput::default()
                })
                .child(accordion_panel(
                    ElementId::local("acc-a", 0, &acc_id),
                    clay_ui::AccordionPanelConfig::new(accordion_a_header_id(), "Render Settings")
                        .open(self.accordion_a_open)
                        .state(self.input.widget_state(&accordion_a_header_id())),
                    Some(acc_a_body),
                ))
                .child(accordion_panel(
                    ElementId::local("acc-b", 0, &acc_id),
                    clay_ui::AccordionPanelConfig::new(accordion_b_header_id(), "Display Settings")
                        .open(self.accordion_b_open)
                        .state(self.input.widget_state(&accordion_b_header_id())),
                    Some(acc_b_body),
                ))
                .build()],
        );

        // Group boxes
        let gb_card = {
            let gb_id = ElementId::local("gb-card", 0, &left_id);
            group_box(
                gb_id.clone(),
                "Render Graph Stats",
                [
                    property_row(
                        PropertyRowSpec::new(ElementId::local("pr1", 0, &gb_id), "Draw calls"),
                        label(ElementId::local("pv1", 0, &gb_id), "1 024", &WidgetState::default()),
                        32.0,
                    ),
                    property_row(
                        PropertyRowSpec::new(ElementId::local("pr2", 0, &gb_id), "Triangles"),
                        label(ElementId::local("pv2", 0, &gb_id), "2.4 M", &WidgetState::default()),
                        32.0,
                    ),
                    property_row(
                        PropertyRowSpec::new(ElementId::local("pr3", 0, &gb_id), "Frame time"),
                        label(ElementId::local("pv3", 0, &gb_id), "6.4 ms", &WidgetState::default()),
                        32.0,
                    ),
                    divider(ElementId::local("div", 0, &gb_id), Axis::Horizontal),
                    property_row(
                        PropertyRowSpec::new(ElementId::local("pr4", 0, &gb_id), "VRAM used"),
                        label(ElementId::local("pv4", 0, &gb_id), "512 MB", &WidgetState::default()),
                        32.0,
                    ),
                ],
            )
        };

        let left = ElementBuilder::container(left_id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(accordions_card)
            .child(gb_card)
            .build();

        // Scrollable list
        let list_items = [
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
        // Use list_scroll_id() as the list container so the InputSimulator
        // can scroll it via wheel/keyboard.
        let list_config = VirtualListConfig::new(list_items.len(), 36.0, 300.0, self.list_scroll);
        let list_layout = list_config.layout();
        let visible_items: Vec<Element> = list_layout
            .visible_range
            .clone()
            .map(|i| {
                let (id_str, sub) = list_items[i];
                list_item_with_palette(
                    ListItemSpec::new(list_item_id(i), id_str)
                        .sublabel(sub)
                        .selected(i == self.list_selected)
                        .state(self.input.widget_state(&list_item_id(i))),
                    &self.palette,
                )
            })
            .collect();
        let list_el = virtual_list(
            list_scroll_id(),
            LayoutSizing::Fixed(col_w),
            list_layout.viewport_extent,
            &list_layout,
            visible_items,
        );

        // Table header demo
        let tbl_id = ElementId::local("tbl", 0, &right_id);
        let tbl_header = table_header_row(
            ElementId::local("tbl-hdr", 0, &tbl_id),
            28.0,
            [
                TableHeaderSpec::new(ElementId::local("th0", 0, &tbl_id), "Name", col_w * 0.45)
                    .sort(SortDirection::Ascending),
                TableHeaderSpec::new(ElementId::local("th1", 0, &tbl_id), "Type", col_w * 0.25),
                TableHeaderSpec::new(ElementId::local("th2", 0, &tbl_id), "Size", col_w * 0.30),
            ],
        );

        let list_card = card(
            ElementId::local("list-card", 0, &right_id),
            Some("Scene Hierarchy (J/K scroll)"),
            LayoutSizing::Fixed(col_w),
            LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            [ElementBuilder::container(ElementId::local("list-body", 0, &right_id))
                .style(ElementStyle {
                    padding: Edges::symmetric(0.0, 8.0),
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
                    ..LayoutInput::default()
                })
                .child(tbl_header)
                .child(list_el)
                .build()],
        );

        let right = ElementBuilder::container(right_id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(list_card)
            .build();

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                direction: LayoutDirection::LeftToRight,
                gap: 16.0,
                ..LayoutInput::default()
            })
            .child(left)
            .child(right)
            .build()
    }

    // ── Inspector tab ─────────────────────────────────────────────────────────

    fn build_inspector_tab(&self, id: ElementId, width: f32, height: f32) -> Element {
        let col_w = ((width - 16.0) * 0.5).max(1.0);
        let left_id = ElementId::local("left", 0, &id);
        let right_id = ElementId::local("right", 0, &id);

        // Property editor
        let props_id = ElementId::local("props", 0, &left_id);
        let prop_items: Vec<(&str, &str)> = vec![
            ("Background", "#0D1520"),
            ("Width", "1280 px"),
            ("Height", "800 px"),
            ("DPI Scale", "1.0×"),
            ("AA Mode", "MSAA 4×"),
            ("Bloom", "Enabled"),
            ("HDR", "Disabled"),
            ("Present", "Mailbox"),
            ("VRAM", "8 GB"),
            ("Backend", "Vulkan 1.3"),
        ];
        let props: Vec<Element> = prop_items
            .iter()
            .enumerate()
            .map(|(i, (lbl, val))| {
                property_row(
                    PropertyRowSpec::new(
                        ElementId::local("prop", i as u32, &props_id),
                        *lbl,
                    )
                    .label_width(100.0),
                    label(
                        ElementId::local("pval", i as u32, &props_id),
                        *val,
                        &WidgetState::default(),
                    ),
                    30.0,
                )
            })
            .collect();

        let scroll_content_height = props.len() as f32 * 30.0;
        let props_viewport = 260.0_f32.min(height - 60.0);
        let props_scroll_config = ScrollConfig::new(
            Vec2::new(col_w, props_viewport),
            Vec2::new(col_w, scroll_content_height),
        );
        let mut props_container = ElementBuilder::container(props_id.clone()).layout(LayoutInput {
            width: LayoutSizing::Fixed(col_w),
            height: LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            ..LayoutInput::default()
        });
        for p in props {
            props_container = props_container.child(p);
        }

        let props_card = card(
            ElementId::local("props-card", 0, &left_id),
            Some("Properties"),
            LayoutSizing::Fixed(col_w),
            LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            [scroll_container_with_scrollbars(
                inspector_scroll_id(),
                LayoutSizing::Fixed(col_w),
                LayoutSizing::Fixed(props_viewport),
                props_scroll_config,
                Vec2::new(0.0, self.inspector_scroll),
                [props_container.build()],
            )],
        );

        let left = ElementBuilder::container(left_id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(props_card)
            .build();

        // Log viewer
        let log_entries: Vec<LogEntrySpec> = (0..20)
            .map(|i| {
                let (level, msg) = match i % 5 {
                    0 => (LogLevel::Info, format!("frame {i}: scene submitted")),
                    1 => (LogLevel::Debug, format!("frame {i}: atlas dirty=false")),
                    2 => (LogLevel::Warn, format!("frame {i}: shader recompile queued")),
                    3 => (LogLevel::Error, format!("frame {i}: fence timeout after 5s")),
                    _ => (LogLevel::Trace, format!("frame {i}: present ok")),
                };
                LogEntrySpec::new(
                    ElementId::local("log", i as u32, &right_id),
                    level,
                    msg,
                )
                .timestamp(format!("{:02}:{:02}:{:02}", i / 3600, (i / 60) % 60, i % 60))
            })
            .collect();

        let log_config = VirtualListConfig::new(log_entries.len(), 28.0, 320.0, self.log_scroll);
        let log_layout = log_config.layout();
        let visible_log: Vec<LogEntrySpec> = log_layout
            .visible_range
            .clone()
            .map(|i| log_entries[i].clone())
            .collect();

        let log_el = virtual_log_viewer(
            log_scroll_id(),
            LayoutSizing::Fixed(col_w),
            &log_layout,
            visible_log,
        );

        let log_card = card(
            ElementId::local("log-card", 0, &right_id),
            Some("Event Log (J/K scroll)"),
            LayoutSizing::Fixed(col_w),
            LayoutSizing::Fit {
                min: 0.0,
                max: f32::INFINITY,
            },
            [log_el],
        );

        // Empty state example
        let empty_card = card(
            ElementId::local("empty-card", 0, &right_id),
            Some("Empty State"),
            LayoutSizing::Fixed(col_w),
            LayoutSizing::Fixed(100.0),
            [empty_state(
                ElementId::local("empty", 0, &right_id),
                "No assets loaded",
                Some("Drop files here or use File → Import"),
                col_w - 2.0,
                80.0,
            )],
        );

        let right = ElementBuilder::container(right_id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(col_w),
                height: LayoutSizing::Fit {
                    min: 0.0,
                    max: f32::INFINITY,
                },
                direction: LayoutDirection::TopToBottom,
                gap: 12.0,
                ..LayoutInput::default()
            })
            .child(log_card)
            .child(empty_card)
            .build();

        ElementBuilder::container(id)
            .layout(LayoutInput {
                width: LayoutSizing::Fixed(width),
                height: LayoutSizing::Fixed(height),
                direction: LayoutDirection::LeftToRight,
                gap: 16.0,
                ..LayoutInput::default()
            })
            .child(left)
            .child(right)
            .build()
    }

    fn build_status_bar(&self, id: ElementId, width: f32) -> Element {
        let tab_names = ["Controls", "Inputs", "Layout", "Inspector"];
        let sections = vec![
            StatusBarSectionSpec::new(ElementId::local("s0", 0, &id), "Tab")
                .value(tab_names[self.active_tab]),
            StatusBarSectionSpec::new(ElementId::local("s1", 0, &id), "Slider")
                .value(format!("{:.0}%", self.slider_value * 100.0)),
            StatusBarSectionSpec::new(ElementId::local("s2", 0, &id), "Backend").value("Vulkan"),
            StatusBarSectionSpec::new(ElementId::local("s3", 0, &id), "AA").value("MSAA 4×"),
            StatusBarSectionSpec::new(ElementId::local("s4", 0, &id), "D → dialog"),
        ];
        let mut sb = status_bar_with_palette(id, sections, &self.palette);
        sb.layout.width = LayoutSizing::Fixed(width);
        sb
    }
}

// ── Dialog body ───────────────────────────────────────────────────────────────

fn build_dialog_body(id: ElementId, palette: &WidgetPalette) -> Element {
    ElementBuilder::container(id.clone())
        .style(ElementStyle {
            padding: Edges::all(20.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            direction: LayoutDirection::TopToBottom,
            gap: 16.0,
            ..LayoutInput::default()
        })
        .child(ElementBuilder::text(
            ElementId::local("msg", 0, &id),
            "This dialog was opened by pressing D. It demonstrates the\ndialog_surface widget with a title bar and body area.",
            TextStyle {
                font_size: 14.0,
                line_height: 20.0,
                color: palette.text,
                wrap: TextWrap::Words,
                ..TextStyle::default()
            },
        ).layout(LayoutInput {
            width: LayoutSizing::Fixed(380.0),
            height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
            ..LayoutInput::default()
        }).build())
        .child(
            ElementBuilder::container(ElementId::local("btns", 0, &id))
                .layout(LayoutInput {
                    width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
                    height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                    direction: LayoutDirection::LeftToRight,
                    gap: 8.0,
                    ..LayoutInput::default()
                })
                .child(button(dialog_ok_id(), "OK", &WidgetState::default()))
                .child(button(dialog_cancel_id(), "Cancel", &WidgetState::default()))
                .build(),
        )
        .build()
}

// ── Renderer ──────────────────────────────────────────────────────────────────

fn render_element(
    overlay: &mut DebugOverlay,
    width: u32,
    height: u32,
    element: &Element,
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
            // Use the element's natural origin so text is positioned correctly
            // even when partially scrolled behind a clip boundary. The clip_rect
            // discards glyphs that fall outside the visible region.
            let desc = TextDrawDesc::new(text.text.clone())
                .placement(TextPlacement::Screen2d {
                    x: rect.origin.x,
                    y: rect.origin.y,
                })
                .typography(typo)
                .color(text.style.color.to_f32_array());
            let desc = if text.style.wrap == TextWrap::Words {
                desc.max_width(rect.size.width.max(1.0))
            } else {
                desc
            };
            // Always apply a clip rect so text cannot overflow its container.
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
            let ow = outline_max(element.style.outline_width);
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

fn outline_max(e: Edges) -> f32 {
    e.left.max(e.right).max(e.top).max(e.bottom)
}

fn spacer_grow(id: ElementId) -> Element {
    ElementBuilder::container(id)
        .layout(LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(1.0),
            ..LayoutInput::default()
        })
        .build()
}

fn main() {
    sturdy_engine::run::<UiDemo>(
        WindowConfig::new("SturdyEngine Widget Demo", 1400, 900).with_resizable(true),
    );
}
