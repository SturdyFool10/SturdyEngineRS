//! Backend-neutral UI layout and render queue inspired by Clay.
//!
//! This crate intentionally sits above graphics backends. It resolves UI trees
//! into draw items, batches, text scenes, and render-graph pass descriptions
//! without recording API-specific commands.

pub mod batch;
pub mod color;
pub mod context;
pub mod coords;
pub mod element;
pub mod floating;
pub mod font_discovery;
pub mod geometry;
pub mod gradient;
pub mod id;
pub mod image_tiling;
pub mod input;
pub mod layout;
pub mod media;
pub mod mosaic;
pub mod render_command;
pub mod render_graph;
pub mod shader;
pub mod text;
pub mod tree;
pub mod virtualization;
pub mod widgets;

pub use batch::{GpuBatch, GpuBatchKind, GpuWorkQueue, OffscreenTarget};
pub use color::{
    ColorComputationMode, ColorSpaceKind, ColorSpaceSample, ColorWorkload, CpuColorTransform,
    UiColor, color_computation_mode,
};
pub use context::{TextSceneKey, UiContext, UiFrameOutput, UiTextFrameStats, UiTreeInstance};
pub use coords::{
    ClipSpace, Ndc, RenderTargetPx, SurfacePx, TexelPx, UiPx, Uv01, WindowLogicalPx,
    WindowPhysicalPx, WorldSpace, logical_to_physical, physical_to_logical, render_target_to_uv,
    surface_to_ndc, ui_to_surface, window_logical_to_surface, window_logical_to_ui,
};
pub use element::{Element, ElementKind, ElementStyle, ImageElement, TextElement};
pub use floating::{
    FloatingAlign, FloatingAttachConfig, FloatingAttachError, FloatingCollision,
    FloatingLayerConfig, FloatingLayout, FloatingOptions, FloatingPlacement, FloatingSide,
    anchored_floating_layer, attached_floating_layer,
};
pub use font_discovery::{FontDiscovery, FontFaceSummary, FontSearchQuery};
pub use geometry::{Axis, CornerShape, CornerSpec, Edges, Rect, Size, UiShape, radii_all};
pub use gradient::{ColorStop, Easing, EasingRegistry, Gradient, GradientKind};
pub use id::ElementId;
pub use image_tiling::{ColorSpaceTransformPlan, ImageTile, ImageTilingPlan, UiSurfacePlan};
pub use input::{
    Cx, EventContext, EventPhase, FocusScope, Hit, InputEvent, InputSimulator, InteractionPhase,
    ModifierKeys, PendingRegistrations, PointerButton, PointerState, ScrollAxis, ScrollConfig,
    ScrollState, SliderConfig, UiActivationEvent, UiEventResult, UiKeyEvent, UiMode,
    UiPointerEvent, UiTextEvent, WidgetBehavior, WidgetConfig, WidgetEventCallbacks, WidgetKind,
    WidgetState,
};
pub use layout::{
    Align, LayoutCache, LayoutDirection, LayoutError, LayoutInput, LayoutOutput, LayoutPosition,
    LayoutSizing, LayoutTextCacheStats, LayoutTree, UiLayer,
};
pub use media::{
    SvgDocument, SvgRasterOptions, UiAntialiasing, UiAntialiasingMode, UiDownsampleFilter,
    UiImageError, UiImageFit, UiImageOptions, UiImageSampling, UiPixelFormat, UiRasterImage,
};
pub use mosaic::{
    MosaicBreakpoint, MosaicConfig, MosaicError, MosaicLayout, MosaicPlacement, MosaicTileLayout,
    MosaicTileMode, MosaicTileSpec,
};
pub use render_command::{
    BorderRenderData, ClipRenderData, CustomRenderData, ImageRenderData, RectangleRenderData,
    RenderCommand, RenderCommandKind, RenderCommandList, RenderData, TextPass, TextRenderData,
};
pub use render_graph::UiShaderParameterBatchPlan;
pub use render_graph::{RenderGraphTarget, UiGraphPassBuilder};
pub use shader::{
    ShaderBinding, ShaderKind, ShaderRef, ShaderSlot, UI_SHADER_PARAMETER_ALIGNMENT,
    UI_SHADER_PUSH_CONSTANT_LIMIT, UiShaderParameterBatch, UiShaderParameterRecord,
    UiShaderResource, UiShaderResourceRef, UiShaderSlotBinding, UiShaderUniform,
    UiShaderUniformEntry, UiShaderUniformPackError, UiShaderUniformPacket, UiShaderUniformValue,
};
pub use text::{FontFeatures, InvalidOpenTypeTag, TextAlign, TextOutline, TextStyle, TextWrap};
pub use tree::{ElementBuilder, UiTree};
pub use virtualization::{
    VirtualGridConfig, VirtualGridItem, VirtualGridLayout, VirtualItem, VirtualListConfig,
    VirtualListLayout, VirtualTableCell, VirtualTableConfig, VirtualTableLayout, VirtualTreeConfig,
    VirtualTreeLayout, VirtualTreeRow,
};
pub use widgets::{
    AccordionPanelConfig, BadgeVariant, BreadcrumbSpec, ChipSpec, CommandPaletteConfig,
    CommandPaletteItemSpec, ContextMenuItemSpec, DragBarAxis, DropdownOptionSpec, ListItemSpec,
    LogEntrySpec, LogLevel, ModalLayerConfig, NotificationSpec, NumberInputSpec, PortalHostConfig,
    PropertyRowSpec, ScrollbarMetrics, SegmentSpec, SortDirection, StatusBarSectionSpec, TabSpec,
    TableHeaderSpec, TextInputSpec, ToggleAnimConfig, TooltipConfig, WidgetPalette,
    WidgetRenderContext, accordion_panel, badge, breadcrumbs, button, card, card_with_palette,
    checkbox, chip, command_palette, context_menu_item, dialog_surface,
    dialog_surface_with_palette, divider, drag_bar, dropdown_option, empty_state,
    empty_state_with_palette, group_box, group_box_with_palette, icon_button, image,
    image_with_options, label, label_with_palette, list_item, log_entry, modal_layer,
    mosaic_container, notification, number_input, portal_host, progress_bar, property_row, radio,
    scroll_container, scroll_container_with_direction, scroll_container_with_scrollbars,
    scroll_container_with_scrollbars_and_direction, scrollbar, search_box, segmented_control,
    select, slider, status_bar, status_bar_with_palette, tab_bar, table_header_cell,
    table_header_row, text_input, toggle, toolbar, toolbar_with_palette, tooltip_layer,
    tooltip_layer_with_palette, tooltip_surface, virtual_context_menu, virtual_dropdown_menu,
    virtual_grid, virtual_list, virtual_log_viewer, virtual_mosaic, virtual_table, virtual_tree,
};
