# Agent Collaboration Manifest

## Active Agents

- **Agent: Text Rendering System Architect**

## Agent Plans

### Diagnostics Fix

**Goal:** Fix all 157+ diagnostics across the codebase, including:
1. Fix `for as in` syntax errors — `as` is a reserved keyword in Rust, rename to `accel_as` or `as_ref`
2. Fix truncated `text_engine.rs` — file ends mid-function at line ~6940
3. Fix incomplete `object.rs` — unclosed doc comment
4. Fix `scene/mod.rs` — references many non-existent modules, remove stale module declarations

**Key Fixes:**
- `crates/sturdy-engine/src/scene/material.rs` — rename `for as in` to `for accel_as in`
- `crates/sturdy-engine/src/scene/material_asset.rs` — rename `for as in` to `for accel_as in`
- `crates/sturdy-engine/src/scene/material_graph.rs` — rename `for as in` to `for accel_as in`
- `crates/sturdy-engine/src/text_engine.rs` — complete the truncated `offline_pipeline` function and close the struct
- `crates/sturdy-engine/src/scene/object.rs` — complete the doc comment
- `crates/sturdy-engine/src/scene/mod.rs` — remove all stale module declarations that reference non-existent files

**Implementation Files:**
- `crates/sturdy-engine/src/scene/material.rs`
- `crates/sturdy-engine/src/scene/material_asset.rs`
- `crates/sturdy-engine/src/scene/material_graph.rs`
- `crates/sturdy-engine/src/text_engine.rs`
- `crates/sturdy-engine/src/scene/object.rs`
- `crates/sturdy-engine/src/scene/mod.rs`

### Text Rendering System Integration

**Goal:** Create a powerful text rendering system that integrates textui's glyphon, cosmic-text, and shared_lru capabilities with sturdy-engine's abstraction layer.

**Scope:**
- SDF/MSDF glyph rendering for 2D and 3D text
- LRU-based glyph caching for fast text operations
- Text rendering to off-screen textures for HDR/shading effects
- Backend-neutral text rendering (works on Vulkan, D3D12, Metal)
- Fast instanced glyph rendering with GPU batching
- allows for ligatures and OpenType features
- allows drawing to an off-screen render target so shaders can use text as an input for effects(Mask / other)
- use the pre-existing textui and just adapt it for our current engine

**Key Features:**
- MSDF rendering for 3D text (faster than dynamic shaping, looks great even at full screen)
- SDF rendering for 2D text (crisp, efficient)
- LRU glyph cache for repeated text operations
- Text rendering to HDR buffers (FP16) for post-processing effects
- Text rendering to SDR buffers (Rgba8Unorm) for display output
- Material-driven text effects (bloom, chromatic aberration, ACES tone mapping)
- GPU-driven text atlas generation (no CPU upload)
- CPU-driven text atlas generation (cosmic-text shaping, swash rendering)
- Text rendering with push constants (position, scale, color, material parameters)
- Text rendering with bind groups (texture bindings, material bindings)
- Text rendering with acceleration structures (for raytraced/hybrid modes)
- Text rendering with raytraced stages (for raytraced modes)
- Text rendering with path traced bounces (for path traced modes)

**Implementation Files:**
- `crates/sturdy-engine/src/text_render.rs` — Core text rendering system
- `crates/sturdy-engine/src/text_render_sdf.rs` — SDF glyph rendering
- `crates/sturdy-engine/src/text_render_msdf.rs` — MSDF glyph rendering
- `crates/sturdy-engine/src/text_render_atlas.rs` — Text atlas management
- `crates/sturdy-engine/src/text_render_lru.rs` — LRU glyph cache
- `crates/sturdy-engine/src/text_render_material.rs` — Material-driven text effects
- `crates/sturdy-engine/src/text_render_offscreen.rs` — Off-screen text rendering
- `crates/sturdy-engine/src/text_render_hdr.rs` — HDR text rendering
- `crates/sturdy-engine/src/text_render_sdr.rs` — SDR text rendering
- `crates/sturdy-engine/src/text_render_raytraced.rs` — Raytraced text rendering
- `crates/sturdy-engine/src/text_render_path_traced.rs` — Path traced text rendering
- `crates/sturdy-engine/src/text_render_3d.rs` — 3D text rendering
- `crates/sturdy-engine/src/text_render_2d.rs` — 2D text rendering
- `crates/sturdy-engine/src/text_render_gpu.rs` — GPU-driven text atlas
- `crates/sturdy-engine/src/text_render_cpu.rs` — CPU-driven text atlas
- `crates/sturdy-engine/src/text_render_push_constants.rs` — Text push constants
- `crates/sturdy-engine/src/text_render_bind_groups.rs` — Text bind groups
- `crates/sturdy-engine/src/text_render_acceleration.rs` — Text acceleration structures
- `crates/sturdy-engine/src/text_render_raytraced_stages.rs` — Text raytraced stages
- `crates/sturdy-engine/src/text_render_path_traced_bounces.rs` — Text path traced bounces
- `crates/sturdy-engine/src/text_render_shader_programs.rs` — Text shader programs
- `crates/sturdy-engine/src/text_render_material_preset.rs` — Text material presets
- `crates/sturdy-engine/src/text_render_asset.rs` — Text asset pipeline
- `crates/sturdy-engine/src/text_render_preview.rs` — Text preview system
- `crates/sturdy-engine/src/text_render_debug.rs` — Text debug tools
- `crates/sturdy-engine/src/text_render_offline.rs` — Offline text rendering
- `crates/sturdy-engine/src/text_render_game.rs` — Game text rendering

### Rendering Mode Support

**Goal:** Ensure text rendering works across all rendering modes without breaking down.

**Rendering Modes:**
- **Rasterized** — Traditional raster pipeline (Vulkan/D3D12/Metal graphics pipelines)
- **Hybrid** — Raster + raytraced elements (acceleration structures, ray queries)
- **Raytraced** — Primary raytraced pipeline (closest hit, miss, ray generation shaders)
- **Path Traced** — Offline rendering, full path tracing (multiple bounces, importance sampling)

**Key Requirements:**
- Text rendering definitions are rendering-mode-agnostic
- Text rendering parameters translate across all modes
- Text rendering shaders compile across all IR targets (SPIR-V, DXIL, MSL, raytraced extensions)
- Text rendering caching works across modes
- Text rendering graph composition supports mode-specific nodes

**Implementation Files:**
- `crates/sturdy-engine/src/text_render.rs` — Mode-agnostic text rendering
- `crates/sturdy-engine/src/text_render_material.rs` — Mode-specific text effects
- `crates/sturdy-engine/src/text_render_raytraced.rs` — Mode-specific raytraced text
- `crates/sturdy-engine/src/text_render_path_traced.rs` — Mode-specific path traced text

### Game and Offline Rendering Support

**Goal:** Enable both games and offline rendering systems for text.

**Game Features:**
- Real-time text updates (time-varying, user-driven)
- Text parameter streaming (bindless descriptor support)
- Text caching for repeated usage
- Text shader compilation caching (persistent pipeline cache)
- GPU capture integration (RenderDoc, Pix, Xcode)
- Fast text rendering for game performance

**Offline Rendering Features:**
- Path traced text rendering
- Text shader optimization for offline use
- Text parameter batch processing
- Text result caching across frames
- Offline text render graph construction (no swapchain, no surface)

**Implementation Files:**
- `crates/sturdy-engine/src/text_render_offline.rs` — Offline text rendering
- `crates/sturdy-engine/src/text_render_game.rs` — Game text rendering
- `crates/sturdy-engine/src/text_render_asset.rs` — Text asset pipeline
- `crates/sturdy-engine/src/text_render_preview.rs` — Text preview system

### Extra Engine Capabilities

**Goal:** Find extra things we can do to make the engine more powerful while keeping boilerplate small.

**Capabilities:**
- Text shader language support (Slang, HLSL, GLSL, MSL cross-compile)
- Text parameter serialization (save/load text states)
- Text shader optimization (pre-compiled shader artifacts)
- Text cache management (LRU, size-based eviction)
- Text graph DSL (declarative text composition)
- Text asset pipeline (asset loading, validation, caching)
- Text preview system (CPU-side text preview)
- Text debugging tools (shader inspection, parameter visualization)
- Text performance profiling (shader compile time, GPU execution time)

**Implementation Files:**
- `crates/sturdy-engine/src/text_render_asset.rs` — Text asset pipeline
- `crates/sturdy-engine/src/text_render_preview.rs` — Text preview system
- `crates/sturdy-engine/src/text_render_debug.rs` — Text debug tools
- `crates/sturdy-engine/src/text_render_shader_programs.rs` — Text shader programs

## Agent Completion

When this agent is done with all plans, their name will be removed from the Active Agents list.
