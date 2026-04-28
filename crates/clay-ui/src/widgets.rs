use crate::{
    Axis, Cx, CornerSpec, Edges, Element, ElementBuilder, ElementId, ElementStyle, FloatingAlign,
    FloatingAttachConfig, FloatingAttachError, FloatingOptions, FloatingPlacement, LayoutDirection,
    LayoutInput, LayoutPosition, LayoutSizing, LayoutTree, MosaicLayout, Rect, ScrollAxis,
    ScrollConfig, Size, SliderConfig, TextAlign, TextStyle, TextWrap, UiColor, UiImageOptions,
    UiLayer, UiShape, VirtualGridLayout, VirtualListLayout, VirtualTableLayout, VirtualTreeLayout,
    WidgetBehavior, WidgetState, attached_floating_layer, floating::place_subtree_in_layer,
    radii_all,
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

pub trait WidgetRenderContext {
    fn widget_state(&self, id: &ElementId) -> WidgetState;
    fn widget_palette(&self) -> WidgetPalette;

    fn register_widget_behavior(&self, _id: ElementId, _behavior: WidgetBehavior) {}

    fn register_slider_widget(&self, id: ElementId, axis: Axis, config: SliderConfig) {
        self.register_widget_behavior(id, WidgetBehavior::slider(axis).pointer_drag(true));
        let _ = config;
    }

    fn slider_display_value(&self, _id: &ElementId, config: SliderConfig) -> f32 {
        let range = (config.max - config.min).max(f32::EPSILON);
        ((config.initial - config.min) / range).clamp(0.0, 1.0)
    }

    fn register_text_input_widget(&self, id: ElementId) {
        self.register_widget_behavior(id, WidgetBehavior::text_input());
    }

    fn register_drag_bar_widget(&self, id: ElementId, axis: Axis) {
        self.register_widget_behavior(id, WidgetBehavior::drag_bar(axis));
    }
}

impl WidgetRenderContext for Cx<'_> {
    fn widget_state(&self, id: &ElementId) -> WidgetState {
        self.state(id)
    }

    fn widget_palette(&self) -> WidgetPalette {
        self.palette
    }

    fn register_widget_behavior(&self, id: ElementId, behavior: WidgetBehavior) {
        self.register_behavior(id, behavior);
    }

    fn register_slider_widget(&self, id: ElementId, axis: Axis, config: SliderConfig) {
        self.register_slider(id, axis, config);
    }

    fn slider_display_value(&self, id: &ElementId, _config: SliderConfig) -> f32 {
        self.slider_value_normalized(id)
    }

    fn register_text_input_widget(&self, id: ElementId) {
        self.register_text_input(id);
    }

    fn register_drag_bar_widget(&self, id: ElementId, axis: Axis) {
        self.register_drag_bar(id, axis);
    }
}

impl WidgetRenderContext for WidgetState {
    fn widget_state(&self, _id: &ElementId) -> WidgetState {
        self.clone()
    }

    fn widget_palette(&self) -> WidgetPalette {
        WidgetPalette::default()
    }
}

impl WidgetRenderContext for WidgetPalette {
    fn widget_state(&self, _id: &ElementId) -> WidgetState {
        WidgetState::default()
    }

    fn widget_palette(&self) -> WidgetPalette {
        *self
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
    pub state_override: Option<WidgetState>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropdownOptionSpec {
    pub id: ElementId,
    pub label: String,
    pub selected: bool,
    pub disabled: bool,
    pub separator_before: bool,
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

#[derive(Clone, Debug, PartialEq)]
pub struct TooltipConfig {
    pub viewport: Size,
    pub anchor: ElementId,
    pub size: Size,
    pub options: FloatingOptions,
    pub z_index: i16,
    pub clip: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandPaletteItemSpec {
    pub id: ElementId,
    pub label: String,
    pub description: Option<String>,
    pub shortcut: Option<String>,
    pub group: Option<String>,
    pub selected: bool,
    pub disabled: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct CommandPaletteConfig {
    pub viewport: Size,
    pub width: f32,
    pub row_height: f32,
    pub max_list_height: f32,
    pub scroll_offset: f32,
    pub z_index: i16,
    pub backdrop: UiColor,
    pub title: Option<String>,
    pub placeholder: String,
    pub empty_text: String,
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

impl TooltipConfig {
    pub fn new(viewport: Size, anchor: ElementId, size: Size) -> Self {
        Self {
            viewport,
            anchor,
            size,
            options: FloatingOptions::default()
                .placement(FloatingPlacement::top(FloatingAlign::Center))
                .offset(8.0)
                .viewport_margin(8.0),
            z_index: 30,
            clip: true,
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
}

impl CommandPaletteItemSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            description: None,
            shortcut: None,
            group: None,
            selected: false,
            disabled: false,
        }
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn group(mut self, group: impl Into<String>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl CommandPaletteConfig {
    pub fn new(viewport: Size) -> Self {
        Self {
            viewport,
            width: 560.0,
            row_height: 52.0,
            max_list_height: 360.0,
            scroll_offset: 0.0,
            z_index: 60,
            backdrop: UiColor::from_rgba8(0, 0, 0, 140),
            title: Some("Command Palette".into()),
            placeholder: "Search commands".into(),
            empty_text: "No matching commands".into(),
        }
    }

    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(0.0);
        self
    }

    pub fn row_height(mut self, row_height: f32) -> Self {
        self.row_height = row_height.max(1.0);
        self
    }

    pub fn max_list_height(mut self, max_list_height: f32) -> Self {
        self.max_list_height = max_list_height.max(0.0);
        self
    }

    pub fn scroll_offset(mut self, scroll_offset: f32) -> Self {
        self.scroll_offset = scroll_offset.max(0.0);
        self
    }

    pub fn z_index(mut self, z_index: i16) -> Self {
        self.z_index = z_index;
        self
    }

    pub fn backdrop(mut self, backdrop: UiColor) -> Self {
        self.backdrop = backdrop;
        self
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn no_title(mut self) -> Self {
        self.title = None;
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn empty_text(mut self, empty_text: impl Into<String>) -> Self {
        self.empty_text = empty_text.into();
        self
    }
}

impl SegmentSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), selected: false, state_override: None }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn state(mut self, state: WidgetState) -> Self {
        self.state_override = Some(state);
        self
    }
}

impl DropdownOptionSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), selected: false, disabled: false, separator_before: false }
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
        Self { id, level, message: message.into(), timestamp: None, source: None }
    }

    pub fn timestamp(mut self, timestamp: impl Into<String>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }
}

// ── New spec types ────────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct TextInputSpec {
    pub value: String,
    pub placeholder: String,
    /// Pixel X offset of the text cursor within the content area (after padding).
    /// `None` suppresses cursor rendering even when focused.
    pub cursor_x: Option<f32>,
    /// Pixel (start_x, end_x) of the selection highlight within the content area.
    pub selection: Option<(f32, f32)>,
    pub password: bool,
    pub multiline: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumberInputSpec {
    /// Pre-formatted display value.
    pub value: String,
    pub placeholder: String,
    pub unit: Option<String>,
    pub cursor_x: Option<f32>,
    pub selection: Option<(f32, f32)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TabSpec {
    pub id: ElementId,
    pub label: String,
    pub selected: bool,
    pub icon_key: Option<String>,
    pub icon_size: f32,
    pub state_override: Option<WidgetState>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BreadcrumbSpec {
    pub id: ElementId,
    pub label: String,
    pub is_current: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccordionPanelConfig {
    pub id: ElementId,
    pub title: String,
    pub is_open: bool,
    pub state_override: Option<WidgetState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BadgeVariant {
    Default,
    Success,
    Warning,
    Error,
    Info,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ContextMenuItemSpec {
    pub id: ElementId,
    pub label: String,
    pub shortcut: Option<String>,
    pub icon_key: Option<String>,
    pub disabled: bool,
    pub separator_before: bool,
}

impl TextInputSpec {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            placeholder: String::new(),
            cursor_x: None,
            selection: None,
            password: false,
            multiline: false,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn cursor_x(mut self, cursor_x: f32) -> Self {
        self.cursor_x = Some(cursor_x);
        self
    }

    pub fn selection(mut self, start_x: f32, end_x: f32) -> Self {
        self.selection = Some((start_x, end_x));
        self
    }

    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }

    pub fn multiline(mut self, multiline: bool) -> Self {
        self.multiline = multiline;
        self
    }
}

impl NumberInputSpec {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            placeholder: String::new(),
            unit: None,
            cursor_x: None,
            selection: None,
        }
    }

    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn unit(mut self, unit: impl Into<String>) -> Self {
        self.unit = Some(unit.into());
        self
    }

    pub fn cursor_x(mut self, cursor_x: f32) -> Self {
        self.cursor_x = Some(cursor_x);
        self
    }

    pub fn selection(mut self, start_x: f32, end_x: f32) -> Self {
        self.selection = Some((start_x, end_x));
        self
    }
}

impl TabSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), selected: false, icon_key: None, icon_size: 16.0, state_override: None }
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn icon(mut self, icon_key: impl Into<String>) -> Self {
        self.icon_key = Some(icon_key.into());
        self
    }

    pub fn icon_size(mut self, icon_size: f32) -> Self {
        self.icon_size = icon_size;
        self
    }

    pub fn state(mut self, state: WidgetState) -> Self {
        self.state_override = Some(state);
        self
    }
}

impl BreadcrumbSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), is_current: false }
    }

    pub fn current(mut self, is_current: bool) -> Self {
        self.is_current = is_current;
        self
    }
}

impl AccordionPanelConfig {
    pub fn new(id: ElementId, title: impl Into<String>) -> Self {
        Self { id, title: title.into(), is_open: false, state_override: None }
    }

    pub fn open(mut self, is_open: bool) -> Self {
        self.is_open = is_open;
        self
    }

    pub fn state(mut self, state: WidgetState) -> Self {
        self.state_override = Some(state);
        self
    }
}

impl BadgeVariant {
    pub fn colors(self, palette: &WidgetPalette) -> (UiColor, UiColor) {
        match self {
            Self::Default => (palette.surface_hovered, palette.text),
            Self::Success => (
                UiColor::from_rgba8(20, 83, 45, 230),
                UiColor::from_rgba8(134, 239, 172, 255),
            ),
            Self::Warning => (
                UiColor::from_rgba8(92, 63, 8, 230),
                UiColor::from_rgba8(253, 224, 71, 255),
            ),
            Self::Error => (
                UiColor::from_rgba8(127, 29, 29, 230),
                UiColor::from_rgba8(252, 165, 165, 255),
            ),
            Self::Info => (
                UiColor::from_rgba8(12, 74, 110, 230),
                UiColor::from_rgba8(125, 211, 252, 255),
            ),
        }
    }
}

impl ContextMenuItemSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), shortcut: None, icon_key: None, disabled: false, separator_before: false }
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn icon(mut self, icon_key: impl Into<String>) -> Self {
        self.icon_key = Some(icon_key.into());
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
}

// ── Batch-2 spec types ────────────────────────────────────────────────────────

#[derive(Clone, Debug, PartialEq)]
pub struct ListItemSpec {
    pub id: ElementId,
    pub label: String,
    pub sublabel: Option<String>,
    pub selected: bool,
    pub state_override: Option<WidgetState>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SortDirection {
    None,
    Ascending,
    Descending,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TableHeaderSpec {
    pub id: ElementId,
    pub label: String,
    pub width: f32,
    pub sort: SortDirection,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyRowSpec {
    pub id: ElementId,
    pub label: String,
    pub label_width: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ChipSpec {
    pub id: ElementId,
    pub label: String,
    pub variant: BadgeVariant,
    pub can_remove: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NotificationSpec {
    pub id: ElementId,
    pub message: String,
    pub variant: BadgeVariant,
    pub action_label: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatusBarSectionSpec {
    pub id: ElementId,
    pub label: String,
    pub value: Option<String>,
}

impl ListItemSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), sublabel: None, selected: false, state_override: None }
    }

    pub fn sublabel(mut self, sublabel: impl Into<String>) -> Self {
        self.sublabel = Some(sublabel.into());
        self
    }

    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub fn state(mut self, state: WidgetState) -> Self {
        self.state_override = Some(state);
        self
    }
}

impl TableHeaderSpec {
    pub fn new(id: ElementId, label: impl Into<String>, width: f32) -> Self {
        Self { id, label: label.into(), width: width.max(0.0), sort: SortDirection::None }
    }

    pub fn sort(mut self, sort: SortDirection) -> Self {
        self.sort = sort;
        self
    }
}

impl PropertyRowSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), label_width: 120.0 }
    }

    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = width.max(0.0);
        self
    }
}

impl ChipSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), variant: BadgeVariant::Default, can_remove: false }
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn can_remove(mut self, can_remove: bool) -> Self {
        self.can_remove = can_remove;
        self
    }
}

impl NotificationSpec {
    pub fn new(id: ElementId, message: impl Into<String>, variant: BadgeVariant) -> Self {
        Self { id, message: message.into(), variant, action_label: None }
    }

    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action_label = Some(label.into());
        self
    }
}

impl StatusBarSectionSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self { id, label: label.into(), value: None }
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
}

// ── Widget builders ────────────────────────────────────────────────────────────

pub fn button<C: WidgetRenderContext + ?Sized>(id: ElementId, label: impl Into<String>, cx: &C) -> Element {
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

pub fn radio<C: WidgetRenderContext + ?Sized>(id: ElementId, label: impl Into<String>, checked: bool, cx: &C) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    radio_with_palette(id, label, checked, &state, &palette)
}

pub(crate) fn radio_with_palette(
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

pub fn checkbox<C: WidgetRenderContext + ?Sized>(id: ElementId, label: impl Into<String>, checked: bool, cx: &C) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    checkbox_with_palette(id, label, checked, &state, &palette)
}

pub(crate) fn checkbox_with_palette(
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

pub fn toggle<C: WidgetRenderContext + ?Sized>(id: ElementId, label: impl Into<String>, checked: bool, cx: &C) -> Element {
    cx.register_widget_behavior(id.clone(), WidgetBehavior::interactive());
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    toggle_with_palette(id, label, checked, &state, &palette)
}

pub(crate) fn toggle_with_palette(
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

pub fn segmented_control<C: WidgetRenderContext + ?Sized>(
    id: ElementId,
    segments: impl IntoIterator<Item = SegmentSpec>,
    cx: &C,
) -> Element {
    let segments = segments.into_iter().collect::<Vec<_>>();
    let count = segments.len();
    let palette = cx.widget_palette();
    let mut builder = ElementBuilder::container(id).layout(LayoutInput {
        width: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
        height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
        direction: LayoutDirection::LeftToRight,
        ..LayoutInput::default()
    });

    for (index, segment) in segments.into_iter().enumerate() {
        cx.register_widget_behavior(segment.id.clone(), WidgetBehavior::interactive());
        let state = segment.state_override.clone().unwrap_or_else(|| cx.widget_state(&segment.id));
        let mut style = control_style(&state, &palette, segment.selected, 0.0, Edges::symmetric(10.0, 6.0));
        style.shape = segment_shape(index, count, 7.0);
        style.outline_width = Edges::all(if state.focused { 2.0 } else { 1.0 });

        builder = builder.child(
            ElementBuilder::container(segment.id.clone())
                .style(style)
                .layout(LayoutInput {
                    width: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
                    height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
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

pub fn drag_bar<C: WidgetRenderContext + ?Sized>(id: ElementId, axis: impl Into<DragBarAxis>, cx: &C) -> Element {
    let axis_enum: DragBarAxis = axis.into();
    let crate_axis = match axis_enum { DragBarAxis::Horizontal => Axis::Horizontal, DragBarAxis::Vertical => Axis::Vertical };
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
    let axis_enum: DragBarAxis = axis.into();
    let crate_axis = match axis_enum { DragBarAxis::Horizontal => Axis::Horizontal, DragBarAxis::Vertical => Axis::Vertical };
    let config = config.into();
    cx.register_slider_widget(id.clone(), crate_axis, config);
    let value = cx.slider_display_value(&id, config);
    let state = cx.widget_state(&id);
    let palette = cx.widget_palette();
    slider_with_palette(id, axis_enum, value, &state, &palette)
}

pub(crate) fn slider_with_palette(
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

pub fn scrollbar<C: WidgetRenderContext + ?Sized>(id: ElementId, metrics: ScrollbarMetrics, cx: &C) -> Element {
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

pub fn log_entry<C: WidgetRenderContext + ?Sized>(entry: LogEntrySpec, row_height: f32, cx: &C) -> Element {
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

pub fn label<C: WidgetRenderContext + ?Sized>(id: ElementId, text: impl Into<String>, cx: &C) -> Element {
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

pub fn tab_bar<C: WidgetRenderContext + ?Sized>(id: ElementId, tabs: impl IntoIterator<Item = TabSpec>, cx: &C) -> Element {
    let mut builder = ElementBuilder::container(id).layout(LayoutInput {
        width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
        height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
        direction: LayoutDirection::LeftToRight,
        ..LayoutInput::default()
    });

    let palette = cx.widget_palette();
    let palette = &palette;
    for tab in tabs {
        let tab_id = tab.id.clone();
        cx.register_widget_behavior(tab.id.clone(), WidgetBehavior::interactive());
        let state = tab.state_override.clone().unwrap_or_else(|| cx.widget_state(&tab.id));
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
            outline_width: Edges { bottom: 2.0, ..Edges::ZERO },
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

pub fn breadcrumbs<C: WidgetRenderContext + ?Sized>(id: ElementId, items: impl IntoIterator<Item = BreadcrumbSpec>, cx: &C) -> Element {
    let items: Vec<BreadcrumbSpec> = items.into_iter().collect();
    let count = items.len();
    let mut builder = ElementBuilder::container(id).layout(LayoutInput {
        width: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
        height: LayoutSizing::Fit { min: 0.0, max: f32::INFINITY },
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

pub fn text_input<C: WidgetRenderContext + ?Sized>(id: ElementId, spec: &TextInputSpec, cx: &C) -> Element {
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

    let mut inner = ElementBuilder::container(ElementId::local("inner", 0, &id))
        .layout(LayoutInput {
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

pub fn number_input<C: WidgetRenderContext + ?Sized>(id: ElementId, spec: &NumberInputSpec, cx: &C) -> Element {
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

    let mut inner = ElementBuilder::container(ElementId::local("inner", 0, &id)).layout(
        LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(content_height),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            gap: 4.0,
            ..LayoutInput::default()
        },
    );

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
        inner = inner.child(compact_text(unit_id, unit, palette.muted_text, 12.0, content_height));
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

pub fn search_box<C: WidgetRenderContext + ?Sized>(id: ElementId, spec: &TextInputSpec, cx: &C) -> Element {
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
    let search_icon = compact_text(search_icon_id, "⌕", palette.muted_text, 14.0, content_height);

    let text_id = ElementId::local("text", 0, &id);
    let mut text_el = compact_text(text_id, display_text, text_color_val, 14.0, content_height);
    text_el.layout.width = LayoutSizing::Grow {
        min: 0.0,
        max: f32::INFINITY,
    };
    text_el.layout.height = LayoutSizing::Fixed(content_height);

    let mut inner = ElementBuilder::container(ElementId::local("inner", 0, &id)).layout(
        LayoutInput {
            width: LayoutSizing::Grow {
                min: 0.0,
                max: f32::INFINITY,
            },
            height: LayoutSizing::Fixed(content_height),
            direction: LayoutDirection::LeftToRight,
            align_y: crate::Align::Center,
            ..LayoutInput::default()
        },
    );

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

pub fn select<C: WidgetRenderContext + ?Sized>(id: ElementId, selected_label: impl Into<String>, is_open: bool, cx: &C) -> Element {
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
        .child(compact_text(chevron_id, chevron_char, palette.muted_text, 10.0, 12.0))
        .build()
}

pub fn icon_button<C: WidgetRenderContext + ?Sized>(id: ElementId, icon_key: impl AsRef<str>, icon_size: f32, cx: &C) -> Element {
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

pub fn toolbar(
    id: ElementId,
    children: impl IntoIterator<Item = Element>,
) -> Element {
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
    let state = config.state_override.clone().unwrap_or_else(|| cx.widget_state(&config.id));
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
        outline: if state.focused { palette.outline_focus } else { palette.outline },
        outline_width: Edges { bottom: 1.0, ..Edges::ZERO },
        padding: Edges::symmetric(12.0, 10.0),
        ..ElementStyle::default()
    };
    if state.disabled {
        header_style.background = palette.surface_disabled;
    }

    let label_color = if state.disabled { palette.muted_text } else { palette.text };
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

    let mut content_builder = ElementBuilder::container(content_id)
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
            .child(ElementBuilder::text(
                ElementId::local("title", 0, &title_bar_id),
                title_text,
                TextStyle {
                    font_size: 15.0,
                    line_height: 20.0,
                    color: palette.text,
                    wrap: TextWrap::None,
                    ..TextStyle::default()
                },
            ).build())
            .build();
        builder = builder.child(title_bar);
    }

    let mut body_builder = ElementBuilder::container(body_id)
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
    let rows = visible_items.into_iter().map(|item| {
        command_palette_item_row(item, layout.item_extent, sim, &palette)
    });
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
        ModalLayerConfig::new(config.viewport).backdrop(config.backdrop).z_index(config.z_index),
        [ElementBuilder::container(ElementId::local("centering", 0, &panel_id))
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
            .build()],
    )
}

// ── Batch-2 widget builders ───────────────────────────────────────────────────

pub fn list_item<C: WidgetRenderContext + ?Sized>(item: ListItemSpec, cx: &C) -> Element {
    cx.register_widget_behavior(item.id.clone(), WidgetBehavior::interactive());
    let state = item.state_override.clone().unwrap_or_else(|| cx.widget_state(&item.id));
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
        let mut sub_el =
            compact_text(ElementId::local("sublabel", 0, &row_id), sub, sub_color, 12.0, 16.0);
        sub_el.layout.width = LayoutSizing::Grow {
            min: 0.0,
            max: f32::INFINITY,
        };
        row = row.child(sub_el);
    }

    row.build()
}

pub fn table_header_cell<C: WidgetRenderContext + ?Sized>(spec: TableHeaderSpec, height: f32, cx: &C) -> Element {
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
    let label_color = if state.disabled { palette.muted_text } else { palette.text };

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
        .style(ElementStyle { background: palette.surface, ..ElementStyle::default() })
        .layout(LayoutInput {
            width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
            height: LayoutSizing::Fixed(height.max(0.0)),
            direction: LayoutDirection::LeftToRight,
            ..LayoutInput::default()
        });

    for spec in specs {
        builder = builder.child(table_header_cell(spec, height, cx));
    }

    builder.build()
}

pub fn property_row<C: WidgetRenderContext + ?Sized>(spec: PropertyRowSpec, value: Element, row_height: f32, cx: &C) -> Element {
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
    let border = if state.focused { palette.outline_focus } else { palette.outline };

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
            outline_width: Edges { top: 1.0, ..Edges::ZERO },
            padding: Edges::symmetric(12.0, 0.0),
            ..ElementStyle::default()
        })
        .layout(LayoutInput {
            width: LayoutSizing::Grow { min: 0.0, max: f32::INFINITY },
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
        let color = if sec_state.hovered { palette.accent } else { palette.muted_text };
        let mut text_el = compact_text(sec_id, text, color, 11.0, 14.0);
        text_el.layout.width = LayoutSizing::Fit { min: 0.0, max: f32::INFINITY };
        builder = builder.child(text_el);

        if i + 1 < count {
            builder = builder.child(
                ElementBuilder::container(ElementId::local("sep", i as u32, &id))
                    .style(ElementStyle { background: palette.outline, ..ElementStyle::default() })
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
    card_with_palette(id, title, width, height, children, &WidgetPalette::default())
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

fn hoverable_row_style(state: &WidgetState, palette: &WidgetPalette, selected: bool) -> ElementStyle {
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

fn selection_element(
    id: ElementId,
    x: f32,
    width: f32,
    height: f32,
    color: UiColor,
) -> Element {
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
mod tests {
    use super::*;
    use crate::{
        Cx, ElementKind, InputSimulator, LayoutCache, LayoutTree, MosaicConfig, MosaicTileMode,
        MosaicTileSpec, Size, UiAntialiasing, UiImageFit, UiImageSampling, VirtualGridConfig,
        VirtualListConfig, VirtualTableConfig, VirtualTreeConfig,
    };

    /// Create a no-interaction Cx for widget structure tests.
    fn test_cx_and_sim() -> (InputSimulator, WidgetPalette) {
        (InputSimulator::default(), WidgetPalette::default())
    }
    macro_rules! cx {
        ($sim:expr, $palette:expr) => {
            Cx::new(&$sim, $palette)
        };
    }

    #[test]
    fn button_builder_marks_label_as_nowrap() {
        let id = ElementId::new("button");
        let element = button_with_palette(id.clone(), "Run", &WidgetState::default(), &WidgetPalette::default());

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
        let element = toggle_with_palette(id, "Enabled", true, &WidgetState::default(), &WidgetPalette::default());
        let track = &element.children[0];

        assert_eq!(track.layout.align_x, crate::Align::End);
        assert_eq!(track.children.len(), 1);
    }

    #[test]
    fn checked_checkbox_adds_check_mark() {
        let id = ElementId::new("checkbox");
        let element = checkbox_with_palette(id, "Accept", true, &WidgetState::default(), &WidgetPalette::default());
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
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let element = segmented_control(
            id.clone(),
            [
                SegmentSpec::new(ElementId::local("one", 0, &id), "One"),
                SegmentSpec::new(ElementId::local("two", 0, &id), "Two").selected(true),
                SegmentSpec::new(ElementId::local("three", 0, &id), "Three"),
            ],
            &cx,
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
        let element = drag_bar_with_palette(id, DragBarAxis::Vertical, &WidgetState::default(), &WidgetPalette::default());

        assert_eq!(element.layout.width, LayoutSizing::Fixed(6.0));
        assert!(matches!(element.layout.height, LayoutSizing::Grow { .. }));
    }

    #[test]
    fn slider_clamps_value_into_fill_percent() {
        let id = ElementId::new("slider");
        let element = slider_with_palette(id, DragBarAxis::Horizontal, 2.0, &WidgetState::default(), &WidgetPalette::default());

        assert_eq!(element.children[0].layout.width, LayoutSizing::Percent(1.0));
        assert!(element.layout.clip_x);
        assert!(element.layout.clip_y);
    }

    #[test]
    fn progress_bar_uses_clamped_fill_percent() {
        let id = ElementId::new("progress");
        let element = progress_bar_with_palette(id, -1.0, &WidgetState::default(), &WidgetPalette::default());

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
    fn tooltip_surface_wraps_text_and_passes_through_input() {
        let id = ElementId::new("tooltip");
        let palette = WidgetPalette::default();
        let element = tooltip_surface(
            id,
            "A longer helpful hint",
            Size::new(140.0, 44.0),
            &palette,
        );

        assert_eq!(element.layout.width, LayoutSizing::Fixed(140.0));
        assert_eq!(element.layout.height, LayoutSizing::Fixed(44.0));
        assert!(element.layout.clip_x);
        assert!(element.layout.clip_y);
        assert!(element.style.transparent_to_input);
        assert_eq!(element.style.background, palette.surface.with_alpha(0.96));

        let ElementKind::Text(text) = &element.children[0].kind else {
            panic!("tooltip label should be text");
        };
        assert_eq!(text.text, "A longer helpful hint");
        assert_eq!(text.style.wrap, TextWrap::Words);
        assert!(element.children[0].style.transparent_to_input);
        assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(124.0));
    }

    #[test]
    fn tooltip_layer_attaches_to_anchor_in_top_layer() {
        let root_id = ElementId::new("root");
        let anchor_id = ElementId::new("button");
        let mut root = Element::new(root_id);
        root.style.padding = Edges::all(10.0);
        root.layout.width = LayoutSizing::Fixed(240.0);
        root.layout.height = LayoutSizing::Fixed(120.0);
        let mut anchor = Element::new(anchor_id.clone());
        anchor.layout.width = LayoutSizing::Fixed(80.0);
        anchor.layout.height = LayoutSizing::Fixed(20.0);
        root.children.push(anchor);
        let layout =
            LayoutTree::compute(&root, Size::new(240.0, 120.0), &mut LayoutCache::default())
                .unwrap();
        let tooltip_id = ElementId::new("tooltip-layer");
        let config = TooltipConfig::new(
            Size::new(240.0, 120.0),
            anchor_id.clone(),
            Size::new(120.0, 36.0),
        )
        .options(
            FloatingOptions::default()
                .placement(FloatingPlacement::bottom(FloatingAlign::Center))
                .offset(6.0),
        )
        .z_index(44);

        let element = tooltip_layer(tooltip_id, &layout, config, "Helpful").unwrap();

        assert_eq!(element.layout.layer, UiLayer::TopLayer);
        assert_eq!(element.layout.z_index, 44);
        assert!(element.style.transparent_to_input);
        assert_eq!(element.children.len(), 1);
        assert_eq!(element.children[0].layout.layer, UiLayer::TopLayer);
        assert_eq!(element.children[0].layout.z_index, 45);
        assert_eq!(
            element.children[0].layout.position,
            LayoutPosition::Absolute {
                offset: Vec2::new(8.0, 36.0)
            }
        );
    }

    #[test]
    fn tooltip_layer_reports_missing_anchor() {
        let tooltip_id = ElementId::new("tooltip-layer");
        let anchor_id = ElementId::new("missing-anchor");
        let config = TooltipConfig::new(
            Size::new(240.0, 120.0),
            anchor_id.clone(),
            Size::new(120.0, 36.0),
        );

        let error = tooltip_layer(tooltip_id, &LayoutTree::default(), config, "Helpful")
            .expect_err("missing anchor should be reported");

        assert_eq!(error, FloatingAttachError::AnchorNotFound(anchor_id));
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
        let element = scrollbar_with_palette(
            ElementId::new("scrollbar"),
            metrics,
            &WidgetState::default(),
            &WidgetPalette::default(),
        );

        assert_eq!(element.layout.width, LayoutSizing::Fixed(90.0));
        assert_eq!(element.layout.height, LayoutSizing::Fixed(8.0));
        assert_eq!(element.children[0].layout.width, LayoutSizing::Fixed(30.0));
        assert_eq!(element.children[1].layout.width, LayoutSizing::Fixed(30.0));
    }

    #[test]
    fn widget_builders_produce_layoutable_trees() {
        let root = button_with_palette(ElementId::new("button"), "Layout", &WidgetState::default(), &WidgetPalette::default());

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

        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let element = virtual_dropdown_menu(id, LayoutSizing::Fixed(180.0), &layout, options, &cx);

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

        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let element = virtual_log_viewer(id, LayoutSizing::Fixed(360.0), &layout, entries, &cx);

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

    // ── New widget tests ───────────────────────────────────────────────────────

    #[test]
    fn label_uses_text_color_from_state() {
        let id = ElementId::new("lbl");
        let mut state = WidgetState::default();
        state.disabled = true;
        let el = label_with_palette(id, "Hello", &state, &WidgetPalette::default());
        let ElementKind::Text(text) = &el.kind else {
            panic!("label should be a text element");
        };
        assert_eq!(text.text, "Hello");
        assert_eq!(text.style.color, WidgetPalette::default().muted_text);
    }

    #[test]
    fn divider_horizontal_is_1px_tall_grow_wide() {
        let el = divider(ElementId::new("div"), Axis::Horizontal);
        assert_eq!(el.layout.height, LayoutSizing::Fixed(1.0));
        assert!(matches!(el.layout.width, LayoutSizing::Grow { .. }));
    }

    #[test]
    fn divider_vertical_is_1px_wide_grow_tall() {
        let el = divider(ElementId::new("div"), Axis::Vertical);
        assert_eq!(el.layout.width, LayoutSizing::Fixed(1.0));
        assert!(matches!(el.layout.height, LayoutSizing::Grow { .. }));
    }

    #[test]
    fn badge_uses_variant_colors() {
        let palette = WidgetPalette::default();
        let (bg, fg) = BadgeVariant::Error.colors(&palette);
        let el = badge_with_palette(
            ElementId::new("badge"),
            "Error",
            BadgeVariant::Error,
            &palette,
        );
        assert_eq!(el.style.background, bg);
        let ElementKind::Text(text) = &el.children[0].kind else {
            panic!("badge child should be text");
        };
        assert_eq!(text.style.color, fg);
        assert_eq!(text.text, "Error");
    }

    #[test]
    fn empty_state_with_description_has_two_text_children() {
        let el = empty_state(
            ElementId::new("empty"),
            "No results",
            Some("Try adjusting your filters"),
            200.0,
            120.0,
        );
        assert_eq!(el.children.len(), 2);
        let ElementKind::Text(title) = &el.children[0].kind else {
            panic!("first child should be text");
        };
        assert_eq!(title.text, "No results");
        let ElementKind::Text(desc) = &el.children[1].kind else {
            panic!("second child should be text");
        };
        assert_eq!(desc.text, "Try adjusting your filters");
    }

    #[test]
    fn empty_state_without_description_has_one_text_child() {
        let el = empty_state(
            ElementId::new("empty"),
            "No results",
            None::<&str>,
            200.0,
            120.0,
        );
        assert_eq!(el.children.len(), 1);
    }

    #[test]
    fn tab_bar_selected_tab_uses_accent_outline_at_bottom() {
        let id = ElementId::new("tabs");
        let tab_a = ElementId::local("a", 0, &id);
        let tab_b = ElementId::local("b", 0, &id);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = tab_bar(
            id,
            [
                TabSpec::new(tab_a, "A").selected(true),
                TabSpec::new(tab_b, "B"),
            ],
            &cx,
        );
        let selected = &el.children[0];
        let unselected = &el.children[1];
        assert_eq!(selected.style.outline, palette.accent);
        assert_eq!(selected.style.outline_width.bottom, 2.0);
        assert_eq!(unselected.style.outline, UiColor::TRANSPARENT);
    }

    #[test]
    fn tab_with_icon_has_icon_and_label_children() {
        let id = ElementId::new("tabs");
        let tab_a = ElementId::local("a", 0, &id);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = tab_bar(
            id,
            [TabSpec::new(tab_a, "Files").icon("icon-folder").icon_size(14.0)],
            &cx,
        );
        let tab = &el.children[0];
        assert_eq!(tab.children.len(), 2);
        let ElementKind::Image(img) = &tab.children[0].kind else {
            panic!("first child should be image icon");
        };
        assert_eq!(img.image_key, "icon-folder");
    }

    #[test]
    fn breadcrumbs_inserts_separators_between_items() {
        let id = ElementId::new("bc");
        let a = ElementId::local("a", 0, &id);
        let b = ElementId::local("b", 0, &id);
        let c = ElementId::local("c", 0, &id);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = breadcrumbs(
            id,
            [
                BreadcrumbSpec::new(a, "Home"),
                BreadcrumbSpec::new(b, "Docs"),
                BreadcrumbSpec::new(c, "Page").current(true),
            ],
            &cx,
        );
        // 3 items + 2 separators = 5 children
        assert_eq!(el.children.len(), 5);
        let ElementKind::Text(sep) = &el.children[1].kind else {
            panic!("separator should be text");
        };
        assert_eq!(sep.text, "›");
    }

    #[test]
    fn text_input_shows_placeholder_when_empty() {
        let id = ElementId::new("field");
        let spec = TextInputSpec::new("").placeholder("Enter name");
        let el = text_input_impl(id, &spec, &WidgetState::default(), &WidgetPalette::default());
        let inner = &el.children[0];
        // last child of inner is the text element
        let text_child = inner.children.last().expect("should have text child");
        let ElementKind::Text(text) = &text_child.kind else {
            panic!("should be text");
        };
        assert_eq!(text.text, "Enter name");
        assert_eq!(text.style.color, WidgetPalette::default().muted_text);
    }

    #[test]
    fn text_input_masks_password_value() {
        let id = ElementId::new("field");
        let spec = TextInputSpec::new("secret").password(true);
        let el = text_input_impl(id, &spec, &WidgetState::default(), &WidgetPalette::default());
        let inner = &el.children[0];
        let text_child = inner.children.last().expect("should have text child");
        let ElementKind::Text(text) = &text_child.kind else {
            panic!("should be text");
        };
        assert_eq!(text.text, "••••••");
    }

    #[test]
    fn text_input_focused_with_cursor_adds_cursor_element() {
        let id = ElementId::new("field");
        let spec = TextInputSpec::new("hello").cursor_x(42.0);
        let mut state = WidgetState::default();
        state.focused = true;
        let el = text_input_impl(id, &spec, &state, &WidgetPalette::default());
        let inner = &el.children[0];
        // cursor + text = 2 children when focused
        assert_eq!(inner.children.len(), 2);
        let cursor = &inner.children[0];
        assert_eq!(cursor.layout.width, LayoutSizing::Fixed(2.0));
        assert_eq!(
            cursor.layout.position,
            LayoutPosition::Absolute {
                offset: glam::Vec2::new(42.0, 0.0)
            }
        );
    }

    #[test]
    fn text_input_with_selection_adds_selection_element() {
        let id = ElementId::new("field");
        let spec = TextInputSpec::new("hello world").selection(10.0, 55.0);
        let el = text_input_impl(id, &spec, &WidgetState::default(), &WidgetPalette::default());
        let inner = &el.children[0];
        // selection + text = 2 children (unfocused so no cursor)
        assert_eq!(inner.children.len(), 2);
        let sel = &inner.children[0];
        assert_eq!(sel.layout.width, LayoutSizing::Fixed(45.0));
        assert_eq!(
            sel.layout.position,
            LayoutPosition::Absolute {
                offset: glam::Vec2::new(10.0, 0.0)
            }
        );
    }

    #[test]
    fn text_input_focused_invalid_uses_invalid_outline() {
        let id = ElementId::new("field");
        let spec = TextInputSpec::new("bad value");
        let mut state = WidgetState::default();
        state.focused = true;
        state.invalid = true;
        let el = text_input_impl(id, &spec, &state, &WidgetPalette::default());
        assert_eq!(el.style.outline, WidgetPalette::default().outline_invalid);
    }

    #[test]
    fn select_trigger_shows_chevron_down_when_closed() {
        let id = ElementId::new("sel");
        let el = select_with_palette(id, "Option A", false, &WidgetState::default(), &WidgetPalette::default());
        let chevron = &el.children[1];
        let ElementKind::Text(text) = &chevron.kind else {
            panic!("chevron should be text");
        };
        assert_eq!(text.text, "▼");
    }

    #[test]
    fn select_trigger_shows_chevron_up_when_open() {
        let id = ElementId::new("sel");
        let el = select_with_palette(id, "Option A", true, &WidgetState::default(), &WidgetPalette::default());
        let chevron = &el.children[1];
        let ElementKind::Text(text) = &chevron.kind else {
            panic!("chevron should be text");
        };
        assert_eq!(text.text, "▲");
    }

    #[test]
    fn icon_button_contains_image_child() {
        let id = ElementId::new("btn");
        let el = icon_button_with_palette(id, "icon-gear", 20.0, &WidgetState::default(), &WidgetPalette::default());
        assert_eq!(el.children.len(), 1);
        let ElementKind::Image(img) = &el.children[0].kind else {
            panic!("icon_button child should be image");
        };
        assert_eq!(img.image_key, "icon-gear");
        assert_eq!(img.natural_size, Some(Size::new(20.0, 20.0)));
    }

    #[test]
    fn accordion_panel_closed_has_only_header() {
        let id = ElementId::new("accordion");
        let header_id = ElementId::local("section", 0, &id);
        let config = AccordionPanelConfig::new(header_id, "Section").open(false);
        let content = Element::new(ElementId::new("body"));
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = accordion_panel(id, config, Some(content), &cx);
        // Only the header child (content is suppressed when closed)
        assert_eq!(el.children.len(), 1);
    }

    #[test]
    fn accordion_panel_open_includes_content() {
        let id = ElementId::new("accordion");
        let header_id = ElementId::local("section", 0, &id);
        let config = AccordionPanelConfig::new(header_id, "Section").open(true);
        let content = Element::new(ElementId::new("body"));
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = accordion_panel(id, config, Some(content), &cx);
        assert_eq!(el.children.len(), 2);
    }

    #[test]
    fn dialog_surface_with_title_has_title_bar_and_body() {
        let id = ElementId::new("dialog");
        let el = dialog_surface(id, Some("My Dialog"), Size::new(400.0, 300.0), []);
        assert_eq!(el.children.len(), 2);
        // First child is title bar, second is body
        let title_bar = &el.children[0];
        let title_child = &title_bar.children[0];
        let ElementKind::Text(title) = &title_child.kind else {
            panic!("title should be text");
        };
        assert_eq!(title.text, "My Dialog");
    }

    #[test]
    fn dialog_surface_without_title_has_only_body() {
        let id = ElementId::new("dialog");
        let el = dialog_surface(id, None::<&str>, Size::new(400.0, 300.0), []);
        assert_eq!(el.children.len(), 1);
    }

    #[test]
    fn context_menu_item_separator_adds_top_outline() {
        let id = ElementId::new("item");
        let spec = ContextMenuItemSpec::new(id, "Cut").separator_before(true);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = context_menu_item(spec, 28.0, &cx);
        assert_eq!(el.style.outline_width.top, 1.0);
        assert_eq!(el.style.outline_width.bottom, 0.0);
    }

    #[test]
    fn context_menu_item_with_icon_uses_image_child() {
        let id = ElementId::new("item");
        let spec = ContextMenuItemSpec::new(id, "Open").icon("icon-open");
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = context_menu_item(spec, 28.0, &cx);
        let ElementKind::Image(img) = &el.children[0].kind else {
            panic!("first child should be icon image");
        };
        assert_eq!(img.image_key, "icon-open");
    }

    #[test]
    fn context_menu_item_without_icon_uses_spacer_placeholder() {
        let id = ElementId::new("item");
        let spec = ContextMenuItemSpec::new(id, "Delete");
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = context_menu_item(spec, 28.0, &cx);
        // Without icon, first child should be a container spacer (not image)
        assert!(
            matches!(el.children[0].kind, ElementKind::Container),
            "should be spacer container"
        );
    }

    #[test]
    fn group_box_has_header_and_content() {
        let id = ElementId::new("group");
        let child = Element::new(ElementId::new("row1"));
        let el = group_box(id, "Settings", [child]);
        assert_eq!(el.children.len(), 2);
        // Header has the label
        let header = &el.children[0];
        let ElementKind::Text(label_text) = &header.children[0].kind else {
            panic!("header child should be text label");
        };
        assert_eq!(label_text.text, "Settings");
    }

    // ── Batch-2 widget tests ───────────────────────────────────────────────────

    #[test]
    fn list_item_selected_uses_selected_surface() {
        let id = ElementId::new("item");
        let spec = ListItemSpec::new(id, "Project A").selected(true);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = list_item(spec, &cx);
        assert_eq!(el.style.background, WidgetPalette::default().surface_selected);
    }

    #[test]
    fn list_item_with_sublabel_has_two_text_children() {
        let id = ElementId::new("item");
        let spec = ListItemSpec::new(id, "Main").sublabel("subtitle text");
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = list_item(spec, &cx);
        assert_eq!(el.children.len(), 2);
        let ElementKind::Text(label) = &el.children[0].kind else {
            panic!("first child should be text");
        };
        assert_eq!(label.text, "Main");
        let ElementKind::Text(sub) = &el.children[1].kind else {
            panic!("second child should be text");
        };
        assert_eq!(sub.text, "subtitle text");
    }

    #[test]
    fn table_header_row_builds_one_cell_per_spec() {
        let id = ElementId::new("header");
        let a = ElementId::local("col-a", 0, &id);
        let b = ElementId::local("col-b", 0, &id);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = table_header_row(
            id,
            28.0,
            [
                TableHeaderSpec::new(a, "Name", 160.0).sort(SortDirection::Ascending),
                TableHeaderSpec::new(b, "Size", 80.0),
            ],
            &cx,
        );
        assert_eq!(el.children.len(), 2);
        assert_eq!(el.layout.height, LayoutSizing::Fixed(28.0));
        // Sort ascending appends ▲
        let ElementKind::Text(txt) = &el.children[0].children[0].kind else {
            panic!("cell should contain text");
        };
        assert!(txt.text.contains('▲'), "ascending sort should show ▲");
    }

    #[test]
    fn table_header_cell_descending_shows_down_arrow() {
        let id = ElementId::new("col");
        let spec = TableHeaderSpec::new(id, "Date", 100.0).sort(SortDirection::Descending);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let cell = table_header_cell(spec, 28.0, &cx);
        let ElementKind::Text(txt) = &cell.children[0].kind else {
            panic!("cell should contain text");
        };
        assert!(txt.text.contains('▼'));
    }

    #[test]
    fn property_row_has_label_and_value_children() {
        let id = ElementId::new("prop");
        let spec = PropertyRowSpec::new(id.clone(), "Opacity").label_width(100.0);
        let value = Element::new(ElementId::local("value", 0, &id));
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = property_row(spec, value, 32.0, &cx);
        assert_eq!(el.children.len(), 2);
        assert_eq!(el.layout.height, LayoutSizing::Fixed(32.0));
        let ElementKind::Text(label) = &el.children[0].kind else {
            panic!("first child should be text label");
        };
        assert_eq!(label.text, "Opacity");
    }

    #[test]
    fn chip_without_remove_has_one_text_child() {
        let id = ElementId::new("chip");
        let spec = ChipSpec::new(id, "Rust").variant(BadgeVariant::Info);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = chip(spec, &cx);
        assert_eq!(el.children.len(), 1);
        let ElementKind::Text(label) = &el.children[0].kind else {
            panic!("chip child should be text");
        };
        assert_eq!(label.text, "Rust");
    }

    #[test]
    fn chip_with_remove_has_label_and_close_button() {
        let id = ElementId::new("chip");
        let spec = ChipSpec::new(id, "v1.0").can_remove(true);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = chip(spec, &cx);
        assert_eq!(el.children.len(), 2);
    }

    #[test]
    fn notification_accent_bar_and_message_present() {
        let id = ElementId::new("notif");
        let spec = NotificationSpec::new(id, "Build failed", BadgeVariant::Error);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = notification(spec, &cx);
        // accent bar + message = 2 children
        assert_eq!(el.children.len(), 2);
        let msg_child = &el.children[1];
        let ElementKind::Text(txt) = &msg_child.kind else {
            panic!("second child should be message text");
        };
        assert_eq!(txt.text, "Build failed");
    }

    #[test]
    fn notification_with_action_has_three_children() {
        let id = ElementId::new("notif");
        let spec = NotificationSpec::new(id, "Update available", BadgeVariant::Info)
            .action("Install");
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = notification(spec, &cx);
        assert_eq!(el.children.len(), 3);
    }

    #[test]
    fn status_bar_separates_sections_with_dividers() {
        let id = ElementId::new("bar");
        let a = ElementId::local("a", 0, &id);
        let b = ElementId::local("b", 0, &id);
        let c = ElementId::local("c", 0, &id);
        let (sim, palette) = test_cx_and_sim();
        let cx = Cx::new(&sim, palette);
        let el = status_bar(
            id,
            [
                StatusBarSectionSpec::new(a, "Branch").value("main"),
                StatusBarSectionSpec::new(b, "Errors").value("0"),
                StatusBarSectionSpec::new(c, "Ready"),
            ],
            &cx,
        );
        // 3 sections + 2 × (divider + gap) = 3 + 4 = 7 children
        assert_eq!(el.children.len(), 7);
        assert_eq!(el.layout.height, LayoutSizing::Fixed(24.0));
    }

    #[test]
    fn card_without_title_has_only_body() {
        let id = ElementId::new("card");
        let el = card(
            id,
            None::<&str>,
            LayoutSizing::Fixed(200.0),
            LayoutSizing::Fixed(100.0),
            [],
        );
        assert_eq!(el.children.len(), 1);
    }

    #[test]
    fn card_with_title_has_title_bar_and_body() {
        let id = ElementId::new("card");
        let el = card(
            id,
            Some("My Card"),
            LayoutSizing::Fixed(200.0),
            LayoutSizing::Fixed(100.0),
            [],
        );
        assert_eq!(el.children.len(), 2);
        // Title bar's first child has the label
        let title_bar = &el.children[0];
        let ElementKind::Text(txt) = &title_bar.children[0].kind else {
            panic!("title bar child should be text");
        };
        assert_eq!(txt.text, "My Card");
    }
}
