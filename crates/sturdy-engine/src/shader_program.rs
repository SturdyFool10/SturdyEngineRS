// ShaderProgram — fullscreen and compute shader program type.
//
// A `ShaderProgram` wraps a compiled Slang vertex+fragment or compute shader
// pair together with its reflected pipeline layout and a per-target-format
// pipeline cache.  It is the primary input to fullscreen and compute passes
// recorded through `RenderFrame` / `GraphImage`.
//
// ## Fullscreen shaders
//
// Fragment-stage programs run over a single fullscreen triangle. The built-in
// vertex shader (`fullscreen_vertex.slang`) provides `SV_POSITION` and UV
// coordinates to the fragment stage. Fragment shaders declare named texture
// and buffer bindings; the render graph resolves them by name at flush time.
//
// ## Compute shaders
//
// Compute-stage programs are dispatched via `frame.dispatch_compute_auto(…)`
// or `frame.dispatch_compute(…)`. They do not use the fullscreen triangle.
//
// ## Hot reload
//
// File-backed programs (loaded via `load_fragment` or `load_compute`) can be
// hot-reloaded by calling `program.reload()`. The vertex shader, reflection,
// and pipeline cache are rebuilt; all pipeline objects are re-created lazily
// on the next draw that uses the updated program.

mod builtin;
mod desc;
mod fullscreen_vertex;
mod program;
mod shader_name;
mod slang_entry_points;

pub(crate) use builtin::builtin_shader_path;
pub use desc::ShaderProgramDesc;
pub(crate) use fullscreen_vertex::{FullscreenVertex, create_fullscreen_triangle};
pub use program::ShaderProgram;
pub use shader_name::ShaderName;
pub use slang_entry_points::SlangEntryPoints;
