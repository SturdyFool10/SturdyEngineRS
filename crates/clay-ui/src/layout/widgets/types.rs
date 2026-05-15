use crate::{
    Edges, ElementId, FloatingAlign, FloatingOptions, FloatingPlacement, Size, UiColor, WidgetState,
};

use super::WidgetPalette;

#[derive(Clone, Debug, PartialEq)]
pub struct CheckboxStyle {
    pub indicator_size: f32,
    pub mark_size: f32,
    pub indicator_radius: f32,
    pub mark_radius: f32,
    pub indicator_padding: Edges,
    pub label_gap: f32,
}

impl Default for CheckboxStyle {
    fn default() -> Self {
        Self {
            indicator_size: 16.0,
            mark_size: 10.0,
            indicator_radius: 4.0,
            mark_radius: 2.0,
            indicator_padding: Edges::all(3.0),
            label_gap: 8.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadioStyle {
    pub indicator_size: f32,
    pub label_gap: f32,
}

impl Default for RadioStyle {
    fn default() -> Self {
        Self {
            indicator_size: 16.0,
            label_gap: 8.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToggleStyle {
    pub track_width: f32,
    pub track_height: f32,
    pub knob_size: f32,
    pub track_padding: f32,
    pub label_gap: f32,
}

impl Default for ToggleStyle {
    fn default() -> Self {
        Self {
            track_width: 36.0,
            track_height: 20.0,
            knob_size: 16.0,
            track_padding: 2.0,
            label_gap: 8.0,
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
        Self {
            id,
            label: label.into(),
            selected: false,
            state_override: None,
        }
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
        Self {
            id,
            label: label.into(),
            selected: false,
            disabled: false,
            separator_before: false,
        }
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
        Self {
            id,
            level,
            message: message.into(),
            timestamp: None,
            source: None,
        }
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
        Self {
            id,
            label: label.into(),
            selected: false,
            icon_key: None,
            icon_size: 16.0,
            state_override: None,
        }
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
        Self {
            id,
            label: label.into(),
            is_current: false,
        }
    }

    pub fn current(mut self, is_current: bool) -> Self {
        self.is_current = is_current;
        self
    }
}

impl AccordionPanelConfig {
    pub fn new(id: ElementId, title: impl Into<String>) -> Self {
        Self {
            id,
            title: title.into(),
            is_open: false,
            state_override: None,
        }
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
        Self {
            id,
            label: label.into(),
            shortcut: None,
            icon_key: None,
            disabled: false,
            separator_before: false,
        }
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
        Self {
            id,
            label: label.into(),
            sublabel: None,
            selected: false,
            state_override: None,
        }
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
        Self {
            id,
            label: label.into(),
            width: width.max(0.0),
            sort: SortDirection::None,
        }
    }

    pub fn sort(mut self, sort: SortDirection) -> Self {
        self.sort = sort;
        self
    }
}

impl PropertyRowSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            label_width: 120.0,
        }
    }

    pub fn label_width(mut self, width: f32) -> Self {
        self.label_width = width.max(0.0);
        self
    }
}

impl ChipSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            variant: BadgeVariant::Default,
            can_remove: false,
        }
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
        Self {
            id,
            message: message.into(),
            variant,
            action_label: None,
        }
    }

    pub fn action(mut self, label: impl Into<String>) -> Self {
        self.action_label = Some(label.into());
        self
    }
}

impl StatusBarSectionSpec {
    pub fn new(id: ElementId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
            value: None,
        }
    }

    pub fn value(mut self, value: impl Into<String>) -> Self {
        self.value = Some(value.into());
        self
    }
}
