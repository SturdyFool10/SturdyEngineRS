// Graph diagnostics and frame-report types.
//
// These are the read-only introspection types returned by RenderFrame::describe()
// and RenderFrame::validate(). They have no dependencies on mutable frame state
// and are extracted here so that callers can reference them without pulling in
// the full frontend_graph module.

use crate::{Format, QueueType};
use sturdy_engine_core::Extent3d;

// ── PassKind ──────────────────────────────────────────────────────────────────

/// The kind of render pass recorded in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassKind {
    /// Fullscreen triangle pass driven by a fragment shader.
    Fullscreen,
    /// Hardware image resolve pass.
    Resolve,
    /// Compute dispatch pass.
    Compute,
    /// Mesh draw pass.
    Mesh,
}

// ── Per-pass / per-image info ─────────────────────────────────────────────────

/// Per-pass information returned by [`RenderFrame::describe`](crate::RenderFrame::describe).
pub struct GraphPassInfo {
    pub name: String,
    pub kind: PassKind,
    /// GPU queue this pass executes on.
    pub queue: QueueType,
    /// Names of frame images read by this pass.
    pub reads: Vec<String>,
    /// Names of frame images written by this pass.
    pub writes: Vec<String>,
    /// Names of buffers read by this pass.
    pub buffer_reads: Vec<String>,
    /// Names of buffers written by this pass.
    pub buffer_writes: Vec<String>,
    /// GPU execution time for this pass in the **previous** frame (milliseconds).
    ///
    /// `None` until the second frame, or on backends without timestamp support.
    /// Timestamp queries are one frame in arrears — the value reflects last frame's
    /// GPU cost, not the frame currently being recorded.
    pub gpu_time_ms: Option<f32>,
}

/// Per-image information returned by [`RenderFrame::describe`](crate::RenderFrame::describe).
pub struct GraphImageInfo {
    pub name: String,
    pub format: Format,
    pub extent: Extent3d,
    pub write_count: usize,
    pub read_count: usize,
}

// ── GraphReport ───────────────────────────────────────────────────────────────

/// A snapshot of the render graph recorded so far in a [`RenderFrame`](crate::RenderFrame).
pub struct GraphReport {
    pub passes: Vec<GraphPassInfo>,
    pub images: Vec<GraphImageInfo>,
}

// ── Diagnostics ───────────────────────────────────────────────────────────────

/// Severity level of a graph diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticLevel {
    Warning,
    Error,
}

/// A diagnostic produced by [`RenderFrame::validate`](crate::RenderFrame::validate).
pub struct GraphDiagnostic {
    pub level: DiagnosticLevel,
    pub message: String,
}
