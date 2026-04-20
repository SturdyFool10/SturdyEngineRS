# 🧠 Sturdy Engine — Architecture & API Refactor TODO

## 🎯 Goals

- Full backend-agnostic core (no Vulkan leakage outside backend/)
- Image-centric engine API
- Render graph driven execution
- Text system integrated as image operations
- HDR + FP16/FP32 support
- Strong capability/limit querying
- GPU enumeration + switching
- Clean, small, modular file structure

---

# 🔥 Phase 1 — Backend Isolation (CRITICAL FIRST STEP)

## 1. Remove backend leakage from core

- [x] Move backend creation out of `device.rs`
- [x] Create `backend/factory.rs`
  - [x] `create_backend(desc: &DeviceDesc)`
  - [x] `enumerate_adapters(kind: BackendKind)`
- [x] Remove all direct Vulkan imports from:
  - [x] `device.rs`
  - [x] any non-`backend/vulkan/*` modules
- [x] Ensure only `backend/vulkan/*` references:
  - Vulkan types
  - Vulkan extensions
  - Vulkan-specific logic

## 2. Enforce layering rules

- [x] Core layer (`sturdy-engine-core`) contains:
  - [x] traits
  - [x] handles
  - [x] abstract resources
  - [x] graph system
- [x] Backend layer contains:
  - [x] actual API implementations
- [x] Engine layer (`sturdy-engine`) contains:
  - [x] Rust wrapper API
  - [x] chaining
  - [x] runtime management

---

# 🧩 Phase 2 — Capability System Expansion

## 3. Expand `Caps`

- [x] mesh_shading
- [x] ray_tracing
- [x] bindless
- [x] hdr_output
- [x] shader_fp16
- [x] shader_fp64
- [x] image_fp16_render
- [x] image_fp32_render
- [x] dynamic_rendering
- [x] timeline_semaphores

## 4. Expand `Limits`

- [x] max_texture_2d_size
- [x] max_texture_array_layers
- [x] max_color_attachments
- [x] max_compute_workgroup_size
- [x] max_compute_invocations
- [x] max_push_constants_size

## 5. Add format capabilities

- [x] FormatCapabilities struct
- [x] device.format_capabilities(format)

## 6. Add surface/HDR queries

- [x] SurfaceHdrCaps
- [x] HDR10 support
- [x] scRGB support

---

# 🖼️ Phase 3 — Image System Overhaul

## 7. Expand ImageDesc

- [x] dimension
- [x] mip_levels
- [x] layers
- [x] samples
- [x] transient
- [x] clear_value
- [x] debug_name

## 8. Introduce ImageBuilder

- [x] fluent API

## 9. Add semantic roles

- [x] Texture
- [x] ColorAttachment
- [x] DepthAttachment
- [x] Storage
- [x] GBuffer
- [x] Presentable
- [x] Intermediate

---

# 🔗 Phase 4 — Image-Centric API

## 10. GraphFrame

- [x] image()
- [x] swapchain_image()
- [x] present()

## 11. ImageNode

- [x] deferred graph node

## 12. Operations

- [x] clear()
- [x] compute()
- [x] fullscreen()
- [x] copy_to()
- [x] blend_over()
- [x] draw()

## 13. Deferred execution

- [x] build graph, no immediate execution

## 14. Hook into RenderGraph

- [x] convert chains to passes

---

# ✍️ Phase 6 — Text System Integration

- [x] layout/shaping split
- [x] atlas system
- [x] engine adapter
- [x] draw_text API
- [x] support any writable image

---

# 🌈 Phase 7 — HDR Pipeline

- [x] HDR formats
- [x] tonemap pipeline
- [x] fallback handling

---

# 🎮 Phase 8 — GPU Enumeration & Switching

- [x] AdapterInfo expansion
- [x] DeviceManager
- [ ] runtime switching
- [ ] logical resources

---

# 📂 Phase 9 — File Structure Cleanup

- [x] modular files
- [x] strict concept separation

---

# 🚀 Phase 10 — Milestone

- [x] render HDR image
- [x] fullscreen shader
- [x] present
- [x] compute pass
- [x] text rendering

---

# 🧠 Principles

- Image-centric
- Graph-driven
- Deferred execution
- Backend isolation
- Rebuild on GPU switch

---

# Phase 11+ — Reflection-First Graph Systems

The current testbed points at the next technical direction: reflected Slang
shaders should drive pass inputs, pipeline layouts, graph resource declarations,
and validation data. The engine should infer layout, binding, barrier, mip
transition, and draw metadata when that information is available from shader
reflection and declared resource intent.

Chained frontend calls should record graph operations, not force immediate
execution order. Graph compilation determines the actual ordering, barriers,
resource states, and parallel batches from reflection data and declared resource
uses.

The previous phases remain active where unchecked. These objectives build on
top of that foundation instead of replacing it.

## Goals

- Make reflected Slang shaders the primary way to describe pass inputs,
  constants, resource bindings, and pipeline layouts.
- Support multi-pass render graphs composed from reflected shader passes.
- Keep the frontend render API destination-oriented: create or fetch graph
  images, then execute reflected shaders onto those images.
- Treat chained image/shader operations as graph declarations that can be
  reordered when dependencies allow it.
- Add a proceduralism layer so textures can come from shader-authored or
  CPU-authored generators, not only image files or image sequences.
- Expose mips as a graph resource concept for graphical effects such as
  bloom, hierarchical blur, downsample chains, luminance pyramids, and stylized
  texture lookups.
- Expose GPU instancing as a normal draw concept for repeated geometry,
  particle-like effects, sprite fields, impostors, and per-instance material
  variation.
- Keep the engine backend-agnostic while allowing Vulkan, D3D12, and Metal
  backends to map these concepts to their native resource and command models.

## Current Testbed Baseline

- The testbed already demonstrates the right shape: Slang source files,
  reflection-derived pipeline layout, push constants, a fullscreen pass, and a
  frame graph submission path.
- The current implementation still requires manual shader grouping, graphics
  pipeline creation, vertex buffer binding, push-constant upload, and draw
  command recording.
- The next layer should define reusable engine primitives such as reflected
  fullscreen pass, procedural image, mip chain, and instanced draw.

## Target Frontend Shape

The frontend API should look roughly like:

```rust
let scene = engine.load_shader("scene.slang")?;
let bloom_downsample = engine.load_shader("bloom_downsample.slang")?;
let bloom_upsample = engine.load_shader("bloom_upsample.slang")?;
let composite = engine.load_shader("composite.slang")?;

fn render(frame: &mut RenderFrame) -> Result<()> {
    let hdr = frame.image("hdr_scene", ImageDesc::hdr_color());
    let bloom = frame.image("bloom_chain", ImageDesc::hdr_mips());
    let output = frame.swapchain_image();

    hdr.execute_shader(&scene)?;
    bloom.execute_shader(&bloom_downsample)?;
    bloom.execute_shader(&bloom_upsample)?;
    output.execute_shader(&composite)?;

    Ok(())
}
```

- `frame.image(name, desc)` returns a logical graph image. The API presents it
  as image creation, but the engine caches the real backing images per
  swapchain image and only creates new GPU resources when the requested
  descriptor/name/cache key is not already resident.
- `image.execute_shader(shader)` records a graph pass where `image` is the
  destination resource.
- Reflected shader metadata declares the source images, samplers, buffers,
  constants, and storage resources required by the pass.
- Reflected image names are resolved against the current frame's named graph
  images and persistent cached images.
- Chained calls record logical operations. The call order is a declaration
  order, not necessarily the final execution order.
- The graph compiler uses reflected resource dependencies, destination images,
  explicit reads/writes, subresource ranges, and queue capabilities to decide
  what must happen before what.
- Independent operations may execute in parallel or in separate queue batches
  when their resource usage does not conflict.
- `render()` records graph intent. At the end of the render function, the engine
  executes the accumulated graph by default.
- Calling `flush()` executes the graph immediately, clears the pending graph
  command list, and allows later calls in the same frame to record a new graph
  segment.

## Phase 11 — Reflected Pass API

- [x] Add an engine-level `ShaderProgram` or `EffectPass` wrapper that groups
  shader stages, entry points, reflected layout, and pipeline cache keys.
- [x] Add graph pass builders that map reflected shader parameters to graph
  resources.
- [x] Add `GraphImage::execute_shader(shader)` as the primary
  destination-oriented frontend pass API.
- [x] Add shader-side naming conventions for reflected image dependencies.
- [x] Resolve reflected image dependency names against frame image names before
  graph compilation.
- [ ] Validate pass resources against shader reflection before graph submission.
- [x] Report missing resources with source-level names from Slang reflection.
- [x] Support reflected push-constant structs with typed Rust upload helpers.
- [x] Cache graphics and compute pipelines from reflected pass descriptors.
- [x] Add testbed examples for:
  - [x] fullscreen reflected fragment pass
  - [x] reflected compute pass (ComputeProgram + GraphImage::execute_compute)
  - [x] reflected image input/output pass (two-pass scene → composite)
  - [x] reflected uniform/push-constant animation pass

## Phase 12 — Render Graph Composition

- [x] Add named graph resources so passes can connect by semantic names such as
  `scene_color`, `bright_extract`, `bloom_mips`, and `final_color`.
- [x] Add per-swapchain-image caches for named graph images and intermediate
  resources.
- [x] Key cached graph images by logical name, image descriptor, swapchain image
  index, and relevant usage flags.
- [x] Reuse cached graph images across frames unless the swapchain, descriptor,
  usage, or logical name changes.
- [x] Retire or recreate cached graph images on swapchain resize and incompatible
  descriptor changes.
- [x] Add graph templates for common pass shapes:
  - [x] fullscreen color pass (`execute_shader`, `execute_shader_auto`)
  - [x] compute image pass (`execute_compute`)
  - [x] downsample pass (`image_at_fraction` + `execute_shader_auto`)
  - [x] upsample/composite pass (`image_sized_to` + `execute_shader_auto`)
  - [x] copy/resolve/pass-through pass (`ShaderProgram::passthrough` + `GraphImage::blit_from`)
- [x] Add graph introspection data for passes and images (`RenderFrame::describe` → `GraphReport`)
- [x] Add graph validation for unused outputs and write-after-write hazards (`RenderFrame::validate` → `Vec<GraphDiagnostic>`)
- [x] Add graph validation for read-before-write hazards (warn when a pass reads an image that no pass in the current frame writes to).
- [ ] Add graph validation for accidental full-resource barriers when a mip/layer range would be enough (requires Phase 14 subresource model).
- [x] Add examples showing a multi-pass graph assembled from reflected shaders (testbed: scene → bloom → tonemap).

## Phase 12.5 — Graph Execution Semantics

- [x] Treat frame render callbacks as graph recording scopes.
- [x] Treat chained image/shader calls as unordered graph operation declarations
  unless explicit dependencies or resource hazards require ordering.
- [x] Implement deferred bind group construction so passes can be declared in any
  order regardless of when their input images are registered: read-name resolution
  and bind group creation are deferred to `submit_pending_passes`, which runs
  after all declarations in the frame are complete.
  **Note**: `describe()` / `validate()` called before `flush()` will show empty
  read-edges for deferred passes; the scheduler itself sees fully-resolved edges.
- [x] Add a graph scheduler (`schedule_pass_order`) that derives execution order
  from declared image/buffer reads and writes.
- [x] Preserve source declaration order as a deterministic tie-breaker for
  otherwise independent operations (wave-sorted Kahn's).
- [x] Derive dependency edges for:
  - [x] read-after-write
  - [x] write-after-read
  - [x] write-after-write
  - [ ] selected mip/layer hazards (requires Phase 14 subresource model)
  - [x] buffer range hazards
  - [ ] explicit user ordering constraints
- [ ] Detect graph operations that can run in parallel because their image
  subresources, buffer ranges, queues, and pipeline resources do not conflict.
- [ ] Compile parallel-ready passes into record batches grouped by queue and
  dependency level.
- [ ] Define how graphics, compute, and transfer queues synchronize when graph
  passes cross queue families.
- [x] Execute the pending render graph automatically when the render callback
  returns successfully (via `EngineApp` shell + `Drop` auto-flush fallback).
- [x] Add `flush()` as an explicit graph execution boundary inside a frame.
- [x] After `flush()`, clear pending passes; persistent images remain cached.
- [x] Errors from auto-flush (Drop) are silently discarded; explicit `flush()`
  propagates errors to the caller.
- [x] Ensure presentation is appended after the final graph segment (`present_image`
  is now a deferred pending pass, scheduled after its writer).

## Phase 13 — Procedural Texture Layer

- [ ] Introduce a `ProceduralImage` / `GeneratedTexture` concept that owns a
  generation recipe, target image description, update policy, and cache state.
- [ ] Support CPU-authored procedural textures for small/generated assets:
  noise, gradients, ramps, masks, lookup tables, and debug patterns.
- [ ] Support GPU-authored procedural textures through reflected compute or
  fullscreen shader passes.
- [ ] Support animated procedural textures driven by frame time, frame index,
  user parameters, or external data.
- [ ] Allow procedural textures to regenerate:
  - [ ] once at creation
  - [ ] when parameters change
  - [ ] every frame
  - [ ] on explicit request
- [ ] Make generated textures compatible with uploaded texture usage sites:
  sampled images, storage images, render targets, and graph intermediates where
  the format/usage allows it.
- [ ] Add serialization-friendly recipe descriptors so procedural assets can be
  authored without embedding runtime-only closures in asset data.
- [ ] Add testbed examples for:
  - [ ] static procedural checker/noise texture
  - [ ] animated procedural texture
  - [ ] procedural mask feeding a reflected shader pass
  - [ ] GPU-generated texture feeding a later graph pass

## Phase 14 — Mip Resources and Mip-Based Effects

- [ ] Add image builders for full mip chains and selected mip counts.
- [ ] Add subresource-aware graph helpers for addressing individual mips and
  mip ranges.
- [ ] Add automatic mip generation for sampled textures where format and usage
  support it.
- [ ] Add explicit mip graph operations:
  - [ ] write mip
  - [ ] read mip
  - [ ] downsample mip N to N+1
  - [ ] upsample/composite mip N+1 into N
  - [ ] transition selected mip ranges
- [ ] Add sampler controls for lod bias, min/max lod, and mip filter choices in
  the engine API.
- [ ] Implement bloom as the reference mip-based effect:
  - [ ] bright extract
  - [ ] downsample chain
  - [ ] upsample chain
  - [ ] final composite
- [ ] Add testbed examples that render or sample individual mip levels.

## Phase 15 — GPU Instancing as a First-Class Concept

- [ ] Add instance buffer builders with clear per-instance layout
  declarations.
- [ ] Add reflected validation for instance-rate vertex inputs when available.
- [ ] Add graph draw helpers for instanced meshes, fullscreen instance fields,
  sprite batches, and indirect-ready instance buffers.
- [ ] Add per-instance data upload paths that work with the existing upload
  arena and deferred frame model.
- [ ] Add optional storage-buffer-driven instancing for large or variable-size
  instance data.
- [ ] Add examples for:
  - [ ] many instanced quads
  - [ ] instanced meshes with per-instance color/material parameters
  - [ ] animated GPU-updated instance data
  - [ ] effect-oriented instancing, such as layered glow sprites or particles

## Phase 16 — Effect Asset Model

- [ ] Define a small effect asset format that can reference:
  - [ ] Slang shader files and entry points
  - [ ] reflected pass parameters
  - [ ] graph resource declarations
  - [ ] procedural texture recipes
  - [ ] mip policies
  - [ ] instancing inputs
- [ ] Add stable debug names for all generated resources and passes.
- [ ] Keep the Rust API and asset format backed by the same engine primitives.

## Phase 17 — Backend and Capability Work

- [ ] Ensure image usage flags, format capabilities, and sampler capabilities
  are checked before enabling procedural, mip, and storage-image paths.
- [ ] Add backend support for selected mip/layer image views where needed.
- [ ] Add backend support for copy/blit/compute mip generation paths.
- [ ] Add backend support for reflected bind group updates that can handle
  generated images, sampled images, samplers, storage images, uniform buffers,
  storage buffers, and push constants.
- [ ] Keep Vulkan as the reference backend while preserving D3D12 and Metal
  layout constraints in the public model.

## Reference Milestone

- [ ] Build a new testbed scene that demonstrates the technical target:
  a reflected shader graph renders a scene into HDR color, generates a
  procedural animated texture, uses mips for bloom, draws instanced elements,
  composites the result, and presents it.

---

## Phase 18 — Application Shell & Frame Ergonomics

The testbed exposes several categories of boilerplate that a user application
should not have to write. Every application today requires ~80 lines of winit
scaffolding, a `NativeSurfaceDesc` extraction helper, a multi-call frame
submission sequence, per-struct unsafe `bytemuck` impls, and a repeated
`ImageBuilder` pattern for swapchain-sized FP16 images. This phase eliminates
each of those in turn.

### 18.1 — Application Shell (winit integration)

The `App` struct and its `ApplicationHandler` impl are identical in every winit
application using this engine. The engine should own that loop.

- [x] Add an `EngineApp` trait with the minimum surface an application must
  implement:
  - `fn init(engine: &Engine, surface: &Surface) -> Result<Self>`
  - `fn render(&mut self, frame: RenderFrame, surface_image: &SurfaceImage) -> Result<()>`
  - `fn resize(&mut self, width: u32, height: u32) -> Result<()>`
- [x] Add `sturdy_engine::run(title, width, height, impl EngineApp)` that
  creates the event loop, creates the window, drives the winit `ApplicationHandler`
  lifecycle, and calls the trait methods at the right moments.
- [x] Handle close, resize, and redraw internally so user code never imports
  winit directly for standard use cases.
- [x] Surface creation from a winit `Window` should live inside the shell, not
  require a `native_surface_desc` helper in user code.
- [x] Provide a `WindowConfig` builder for title, size, resizability, and HDR
  preference so callers can configure the shell without touching winit types.

Target: the entire `App` + `ApplicationHandler` impl (currently ~80 lines) is
replaced by implementing `EngineApp` and calling `sturdy_engine::run(...)`.

### 18.2 — Engine Surface Convenience

Even outside the full shell, extracting a `NativeSurfaceDesc` from a winit
`Window` currently takes 19 lines of handle extraction and error mapping.

- [x] Add `Engine::create_surface_for_window(window: &impl HasWindowHandle + HasDisplayHandle)`
  that handles handle extraction, `.as_raw()`, error mapping, and size clamping
  internally, returning a `Surface`.
- [x] Guard the new method behind the same `#[cfg(not(target_arch = "wasm32"))]`
  gate already used for `NativeSurfaceDesc`.

Target: `native_surface_desc` helper disappears; `Renderer::new` becomes one line.

### 18.3 — Single-Call Frame Submission

Every render function ends with the same three-call sequence:
`frame.flush()`, `frame.wait()`, `self.surface.present()`. These always appear
together and failing to call any of them is a bug.

- [x] Add `RenderFrame::finish_and_present(surface: &Surface)` that calls
  `flush()`, `wait()`, and `surface.present()` in sequence, returning the first
  error if any step fails.
- [x] Deprecate the separate `flush` + `wait` + `surface.present` pattern for
  the standard swapchain submission path; keep it available for advanced
  split-frame use cases.

Target: the last three lines of every `render()` collapse to one.

### 18.4 — Swapchain-Sized Image Helpers

Creating an FP16 intermediate buffer sized to match the swapchain currently
requires four lines of `ImageBuilder` ceremony. This pattern repeats for every
HDR intermediate in a frame.

- [x] Add `RenderFrame::hdr_color_image(name)` that creates a `Rgba16Float`
  color attachment sized to the current swapchain image, equivalent to the
  `ImageBuilder::new_2d(Format::Rgba16Float, w, h).role(ColorAttachment).build()`
  + `frame.image(name, desc)` pair.
- [x] Add `RenderFrame::hdr_image_sized_to(name, format, surface_image)` for
  callers that need a different format or explicit sizing source.
- [x] Add `ImageDesc::hdr_color(width, height)` as the low-level analogue
  for users who build their own descriptors.

Target: the `ImageBuilder` block in `render()` collapses to one line.

### 18.5 — Push Constant Derive Macro

Every push constant struct requires five lines of ceremony: `#[repr(C)]`,
`#[derive(Copy, Clone)]`, and two `unsafe impl bytemuck::*` blocks. A proc macro
eliminates this.

- [x] Add a `push_constants` attribute macro (in `sturdy-engine-macros`) that
  emits `#[repr(C)]`, `Copy`, `Clone`, `bytemuck::Pod`, and `bytemuck::Zeroable`
  from a single `#[push_constants]` attribute.
- [x] Re-export the macro from `sturdy_engine` so users import it from one place.
- [x] Apply `#[push_constants]` to the engine's own push constant structs
  (`BrightPassConstants`, `DownsampleConstants`, `UpsampleConstants`,
  `BloomCompositeConstants`) and remove the hand-written impls.

Target: 5 lines of per-struct ceremony become 1 derive attribute.

### 18.6 — Stage Mask Inference

`execute_shader_with_constants` and `execute_shader_with_push_constants` require
the caller to pass `StageMask::FRAGMENT` even though the `ShaderProgram` already
carries stage information from Slang reflection.

- [x] Add `GraphImage::execute_shader_with_constants_auto<T: bytemuck::Pod>` (or
  rename the existing method) that reads the stage mask from the shader's
  reflected stage instead of requiring the caller to supply it.
- [x] Fall back to `FRAGMENT` for programs whose reflection does not expose a
  stage, matching current behaviour.
- [x] Keep the explicit-stage variants for callers that need to override the
  inferred stage (e.g. a compute-capable pass driven through a fragment entry
  point for compatibility reasons).

Target: `StageMask::FRAGMENT` disappears from standard fullscreen pass call sites.
