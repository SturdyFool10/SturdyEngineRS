//! Backend-neutral UI layout and render queue inspired by Clay.
//!
//! This crate intentionally sits above graphics backends. It resolves UI trees
//! into draw items, batches, text scenes, and render-graph pass descriptions
//! without recording API-specific commands.

pub mod batch;
pub mod color;
pub mod context;
pub mod element;
pub mod font_discovery;
pub mod geometry;
pub mod gradient;
pub mod id;
pub mod image_tiling;
pub mod input;
pub mod layout;
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
pub use font_discovery::{FontDiscovery, FontFaceSummary, FontSearchQuery};
pub use geometry::{Axis, CornerShape, CornerSpec, Edges, Rect, Size, UiShape, radii_all};
pub use gradient::{ColorStop, Easing, EasingRegistry, Gradient, GradientKind};
pub use id::ElementId;
pub use image_tiling::{ColorSpaceTransformPlan, ImageTile, ImageTilingPlan, UiSurfacePlan};
pub use input::{
    Hit, InputEvent, InputSimulator, InteractionPhase, PointerButton, PointerState, ScrollAxis,
    ScrollConfig, ScrollState, WidgetConfig, WidgetState,
};
pub use layout::{
    Align, LayoutCache, LayoutDirection, LayoutError, LayoutInput, LayoutOutput, LayoutSizing,
    LayoutTextCacheStats, LayoutTree, UiLayer,
};
pub use render_command::{
    BorderRenderData, ClipRenderData, CustomRenderData, ImageRenderData, RectangleRenderData,
    RenderCommand, RenderCommandKind, RenderCommandList, TextPass, TextRenderData,
};
pub use render_graph::{RenderGraphTarget, UiGraphPassBuilder};
pub use shader::{ShaderBinding, ShaderKind, ShaderRef, ShaderSlot};
pub use text::{FontFeatures, InvalidOpenTypeTag, TextAlign, TextOutline, TextStyle, TextWrap};
pub use tree::{ElementBuilder, UiTree};
pub use virtualization::{VirtualItem, VirtualListConfig, VirtualListLayout};
pub use widgets::{
    DragBarAxis, WidgetPalette, button, drag_bar, radio, scroll_container,
    scroll_container_with_direction, toggle, virtual_list,
};
