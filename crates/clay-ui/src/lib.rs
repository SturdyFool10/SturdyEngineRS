//! Backend-neutral UI layout and render queue inspired by Clay.
//!
//! This crate intentionally sits above graphics backends. It resolves UI trees
//! into draw items, batches, text scenes, and render-graph pass descriptions
//! without recording API-specific commands.

pub mod batch;
pub mod color;
pub mod context;
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
pub use element::{Element, ElementKind, ElementStyle, ImageElement, TextElement};
pub use floating::{
    FloatingAlign, FloatingCollision, FloatingLayerConfig, FloatingLayout, FloatingOptions,
    FloatingPlacement, FloatingSide, anchored_floating_layer,
};
pub use font_discovery::{FontDiscovery, FontFaceSummary, FontSearchQuery};
pub use geometry::{Axis, CornerShape, CornerSpec, Edges, Rect, Size, UiShape, radii_all};
pub use gradient::{ColorStop, Easing, EasingRegistry, Gradient, GradientKind};
pub use id::ElementId;
pub use image_tiling::{ColorSpaceTransformPlan, ImageTile, ImageTilingPlan, UiSurfacePlan};
pub use input::{
    FocusScope, Hit, InputEvent, InputSimulator, InteractionPhase, PointerButton, PointerState,
    ScrollAxis, ScrollConfig, ScrollState, WidgetConfig, WidgetState,
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
    DragBarAxis, DropdownOptionSpec, LogEntrySpec, LogLevel, ModalLayerConfig, PortalHostConfig,
    ScrollbarMetrics, SegmentSpec, WidgetPalette, button, checkbox, drag_bar, dropdown_option,
    image, image_with_options, log_entry, modal_layer, mosaic_container, portal_host, progress_bar,
    radio, scroll_container, scroll_container_with_direction, scroll_container_with_scrollbars,
    scroll_container_with_scrollbars_and_direction, scrollbar, segmented_control, slider, toggle,
    virtual_dropdown_menu, virtual_dropdown_menu_with_palette, virtual_grid, virtual_list,
    virtual_log_viewer, virtual_log_viewer_with_palette, virtual_mosaic, virtual_table,
    virtual_tree,
};
