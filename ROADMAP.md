# Sturdy Engine Roadmap

_Last updated: 2026-06-02_

## Product Direction

Sturdy Engine is a backend-neutral graphics-engine skeleton whose main goal is a fast, measurable, GPU-driven renderer capable of dense scenes that can plausibly read as real life.

The repo already contains substantial foundation work: backend-neutral core/render-graph infrastructure, a Vulkan path, a runtime shell, ECS/render-world bridging, deferred PBR, Hi-Z/OIT/post/shadow/environment-map modules, asset loading, material variants, GPU timing, memory infrastructure, and backend capability detection. This roadmap does **not** keep completed foundation work as tasks. It only lists the work still needed to turn those systems into the default, measured product path.

## Non-Negotiable Direction

1. **Measure first.** Every major rendering change must improve or intentionally trade against reference-scene metrics.
2. **The simple path is the serious path.** `AppRuntime` / `RuntimeApp` should own the default frame loop and renderer stack so examples and apps do not rebuild engine plumbing.
3. **CPU submits intent, GPU expands work.** The CPU should not touch every renderable object every frame.
4. **Bindless and GPU-resident tables are the fast path.** Fallbacks are explicit, degraded, and reported.
5. **Temporal correctness comes before advanced effects.** Bad motion/history data breaks TAA, motion blur, denoising, upscaling, reflections, and shadows.
6. **Vulkan is the reference implementation.** D3D12 and Metal follow through capability abstraction; high-end features require honest capability checks.

## Success Metrics

The main renderer regression target is the realistic reference scene. Track at least:

- CPU frame time
- GPU frame time
- P95/P99 frame time
- per-pass GPU timings
- memory usage and memory budget pressure
- upload bandwidth
- visible triangles
- submitted triangles
- draw and dispatch counts
- shader/pipeline cache hit rates
- screenshot comparisons
- runtime setting application results: exact, degraded, or rejected

Initial performance targets:

- 100k renderable entities with CPU render submission below 1 ms on the bindless/GPU-driven path.
- 500k mostly-static or mostly-flat entities without per-object CPU submission becoming the bottleneck.
- Zero per-object draw calls or per-object descriptor sets on the bindless path.

AAA comparison targets (once the full stack through Priority 17 is running):

- GPU frame time equal to or lower than equivalent Unreal Engine 5 Nanite+Lumen scenes at the same visual fidelity, measured on the same hardware by rendering the same scene geometry.
- Meshlet cull efficiency: ≥ 40% triangle reduction vs. a naive unculled draw at 30% occlusion.
- VRS shading efficiency: ≥ 15% deferred shading time reduction on scenes with ≥ 30% flat/sky area.
- Async compute utilization: ≥ 70% simultaneous graphics + async queue activity on a frame timeline.
- Zero CPU submission stalls: CPU frame time for render submission ≤ 0.3 ms regardless of scene object count, on the GPU-driven path.

---

## Priority 0 — ~~SRD Engine-Standard Denoiser~~ (removed)

SRD was an attempt at a first-party real-time denoiser. The full framework was removed as a failed approach. The Cornell path-tracer testbed retains simple reference temporal accumulation (running average via `temporal_accumulate.slang` + `GraphImageHistory` ping-pong) with no SRD dependency. When denoising is needed again (Priority 17 RT reflections), it will be designed from the actual use case rather than speculatively.

---

## ~~Priority 0 — Full SRD denoiser (removed)~~

_All SRD source files, shaders, and docs deleted 2026-06-02. Simple temporal accumulation in the path-tracing testbed replaced with a direct `GraphImageHistory` ping-pong + `temporal_accumulate.slang` shader. Denoising for RT reflections will be designed from actual use-case requirements in Priority 17._

### ~~Naming, ownership, and legal-distinction baseline~~

- [x] Introduce the public `SrdDenoiser` API as the preferred engine denoiser name.
- [x] Move the reference temporal accumulation denoiser out of `realtime_raytracing.rs` into `srd_denoiser.rs`.
- [x] Rename shader bindings from generic/vendor-like accumulation names to SRD names: `srd_current_signal`, `srd_history_signal`, `srd_current_sampler`, and `srd_history_sampler`.
- [x] Rename the Cornell accumulation shader to `srd_temporal_accumulate.slang`.
- [x] Keep `RealtimeRayTracingDenoiser` only as a deprecated compatibility alias.
- [x] Audit public APIs, shader files, docs, examples, and debug labels for accidental vendor-denoiser names or family names.
- [x] Replace paper-derived placeholder family names with SRD-owned names before exposing them publicly:
  - `RadianceStabilizer` for radiance reconstruction.
  - `ShadowStabilizer` for shadow reconstruction.
  - `OcclusionStabilizer` for AO/directional occlusion reconstruction.
  - `ReferenceTemporal` for simple accumulation/testing.
- [x] Add a short `docs/srd.md` identity document that states SRD is SturdyEngine's first-party denoiser, defines accepted terminology, and records what names must not be copied from vendor SDKs.

### SRD crate/module architecture

- [x] Add `crates/sturdy-engine/src/srd_denoiser.rs` as the initial SRD module.
- [x] Split SRD into feature files once implementation grows:
  - `srd_denoiser.rs` (thin aggregator)
  - `srd_denoiser/api.rs`
  - `srd_denoiser/settings.rs`
  - `srd_denoiser/resources.rs`
  - `srd_denoiser/dispatch.rs`
  - `srd_denoiser/pipeline.rs`
  - `srd_denoiser/reference_temporal_executor.rs`
  - `srd_denoiser/radiance_stabilizer.rs`
  - (shadow_stabilizer, occlusion_stabilizer pending algorithm implementation)
- [x] Define stable public descriptors for SRD instance creation, denoiser IDs, resource slots, texture pools, pass descriptions, pipelines, and dispatches.
- [x] Keep the SRD algorithm layer backend-neutral: it may describe GPU work, but renderer/RHI code performs allocation, barriers, binding, and dispatch.
- [x] Add capability reporting for SRD features: temporal history, compute support, storage textures, half-float support, subgroup/wave support, ray-tracing guide support, and backend shader-model constraints.
- [x] Add unit tests for descriptor construction, denoiser ID uniqueness, unsupported mode rejection, settings validation, and dispatch generation.

### Public SRD API and settings

- [x] Add `SrdDenoiserSettings` with capped temporal accumulation frame count.
- [x] Add `SrdDenoiserMode` with SRD-owned mode names.
- [x] Add `SrdTemporalConstants` with explicit `#[repr(C)]` layout.
- [x] Add `SrdInstance` / `SrdInstanceDesc` for multi-denoiser instances.
- [x] Add `SrdDenoiserId` for stable per-instance denoiser routing.
- [x] Add `SrdCommonSettings` with current/previous matrices, jitter, frame index, dynamic-resolution rectangles, motion-vector scale, depth scale, history mode, split-screen, and validation toggles.
- [x] Add per-family settings:
  - `SrdRadianceSettings`
  - `SrdShadowSettings`
  - `SrdOcclusionSettings`
  - `SrdReferenceSettings`
- [x] Add `SrdHistoryMode::{KeepAccumulating, InvalidateHistory, ZeroHistory}`.
- [x] Add explicit settings validation with actionable `Error::InvalidInput` messages.
- [x] Expose runtime settings for SRD quality level / max accumulation frames in the path-tracing testbed.
- [ ] Expose runtime settings for SRD debug mode, split-screen, and validation output once those paths execute.

### Resource and binding model

- [x] Add `SrdTemporalBindings` for SRD-owned shader binding names.
- [x] Define SRD resource slots for guide inputs:
  - motion vectors,
  - normal/roughness,
  - view depth,
  - material ID / material class,
  - confidence / variance where available.
- [x] Define SRD resource slots for noisy inputs:
  - diffuse radiance,
  - specular radiance,
  - combined radiance,
  - AO / directional occlusion,
  - penumbra,
  - translucency,
  - spectral or frequency-binned radiance for the path tracer.
- [x] Define SRD resource slots for outputs:
  - denoised diffuse,
  - denoised specular,
  - denoised combined radiance,
  - denoised AO,
  - denoised directional occlusion,
  - denoised shadow/translucency,
  - validation/debug output.
- [x] Implement permanent history texture pools with persistent previous/current ping-pong resources.
- [x] Implement transient scratch texture pools with aliasing where lifetimes and formats are compatible.
- [x] Add resource format validation for guide inputs and outputs.
- [x] Add renderer debug labels for every SRD texture and pass.

### Pass graph, dispatch, and renderer integration

- [x] Implement the initial SRD reference temporal accumulation pass for progressive samples.
- [x] Replace the single fullscreen-fragment accumulation path with a compute-capable SRD pass path where backend support allows it.
- [x] Add an SRD pass builder for read/write resources, constants, shader program, workgroup size, and debug name.
- [x] Add per-frame dispatch generation that returns ordered dispatch descriptions instead of directly executing all work inside `SrdDenoiser::accumulate`.
- [x] Add clear passes for `ClearAndRestart` history resets.
- [x] Add ping-pong resource metadata so previous/current history swaps are SRD-owned.
- [x] Add constant-buffer arena/ring allocation for SRD dispatch constants.
- [x] Add adjacent-constant reuse detection to avoid redundant uploads.
- [x] Add a graph-backed reference-temporal executor that consumes SRD dispatch/resource/pipeline descriptions and maps them to the current fullscreen render-graph path.
- [x] Add renderer integration that consumes SRD dispatch descriptions and performs barriers, binding, constant upload, and compute submission for compute-capable SRD passes.
- [x] Add GPU timing markers for SRD passes.
- [x] Add graph-inspector output for SRD resources, pass order, and history state.

### Algorithm completion from the SRD design/paper

- [x] Reference temporal accumulation for path-traced samples.
- [x] Add SRD-owned variance/moment constants and settings for temporal history beyond the current luminance moment stored in alpha.
- [x] Add SRD-owned history rejection settings using motion vectors, depth, normals, material IDs, and dynamic-resolution rectangles.
- [x] History clamping against current-neighborhood statistics.
- [x] Add SRD-owned anti-firefly / bright outlier suppression settings.
- [x] Spatial edge-aware filter pass for radiance.
- [x] A-trous wavelet filter path for high-variance radiance reconstruction.
- [x] Recurrent blur/post-blur path for stable radiance reconstruction.
- [x] Hit-distance or ray-length guide support for ray-traced signals.
- [x] Separate diffuse/specular reconstruction paths.
- [x] Combined diffuse+specular fast path for simpler integrations.
- [x] AO and directional-occlusion denoising path using depth/normal edge stopping.
- [x] Shadow penumbra denoising path.
- [x] Translucent shadow denoising path.
- [x] Spectral path-tracing support using fixed bins or compact spectral coefficients instead of raw stochastic wavelengths.
- [x] Optional confidence/variance inputs for adaptive accumulation are represented in SRD slots/settings.
- [x] Split-screen and validation output modes are represented in SRD common settings and slots.
- [x] Deterministic test scenes and screenshot comparisons for each SRD mode.

### Shader library and packing contract

- [x] Add SRD shader helper library for packing/unpacking guide and noisy signal data.
- [x] Define engine-standard normal/roughness/material packing expected by SRD.
- [x] Define engine-standard motion-vector convention for SRD and verify it matches TAA/motion-vector debug tools.
- [x] Define SRD depth convention: linear view depth, scale, and invalid-depth behavior.
- [x] Add spectral radiance layout metadata for the Cornell/path-tracing path.
- [x] Generate or validate Rust constant layouts against shader constant layouts.
- [ ] Add shader tests for SRD helper functions where the shader test harness can cover them.

### Runtime/product integration

- [x] Make SRD the default denoiser option for hardware path-tracing accumulation.
- [x] Add runtime UI controls for SRD quality preset and explicit accumulation reset in the hardware path-tracing testbed.
- [ ] Add runtime UI controls for advanced SRD mode selection, split-screen, and validation output once those modes are implemented.
- [x] Add debug images for SRD current signal, reference-temporal output, and Cornell SRD display output.
- [ ] Add debug images for SRD history signal, variance/moments, guide rejection, split-screen, and validation output as those passes land.
- [x] Reset SRD history on resize, camera cuts, material/lighting mode changes, shader reloads, guide-format changes, and dynamic-resolution incompatibility.
- [ ] Add benchmark counters for SRD GPU time, dispatch count, history memory, transient memory, and quality preset.
- [ ] Include SRD in the realistic reference scene once that scene exists.

### Acceptance

SRD is considered complete when SturdyEngine has a legally distinct, Rust-first, backend-neutral denoiser system that covers the design/paper goals: descriptor-driven integration, persistent/transient resource pools, ordered compute dispatches, robust temporal history, radiance/shadow/occlusion modes, shader packing contracts, debug/validation views, runtime controls, and benchmark coverage.

---

## Priority 1 — First-Party Runtime Shell As The Default Product Path

The runtime shell should own the common frame pipeline. Testbed/example code should provide content and hooks, not rebuild HDR/MSAA/bloom/AA/tonemap/debug plumbing.

### RuntimeApp / AppRuntime frame loop

- [x] Define `RuntimeApp` trait with `init(runtime: &mut AppRuntime)`, `update(&mut AppRuntimeFrame)`, `resize`, `input_hub`, `key_pressed`, `pointer_moved`, `pointer_button`, and `runtime_settings_changed`.
- [x] Add `run_with_runtime<App: RuntimeApp>(config)` and `try_run_with_runtime` as the primary entry points.
- [x] Create `AppRuntime` before calling app init so the app sees surface/engine state from the start.
- [x] `AppRuntimeFrame::finish_and_present` records CPU time, P95/P99, GPU time, GPU P95/P99, and GPU pass timings with per-queue breakdown into `RuntimeDiagnostics`.
- [x] `AppRuntimeFrame` exposes `window_scale_factor`, `window_logical_size`, `runtime_controller`, `runtime_diagnostics`, `register_debug_image`, `save_named_graph_image_png`, `default_runtime_overlay_lines`, `runtime_graph_inspection_lines`, and `shell_frame()` bridge.
- [x] Migrate testbed main, shader_playground, plot_demo, coordinate_validation, and ui_demo from `EngineApp`/`GameApp` to `RuntimeApp`.
- [x] Remove obsolete `game_2d` and `game_3d` testbed binaries; fixed-step samples should return later through `RuntimeApp` when needed.
- [x] Expose `AppRuntimeFrame::run_default_post_process` that handles HDR, bloom, AA, tonemapping, and debug images without per-example wiring.

### Runtime settings

- [x] Settings changes are applied via `RuntimeSettingsTransaction`.
- [x] Surface settings (HDR, present mode, transparency) report `Applied`, `Degraded`, or `Failed` with reason and apply path.
- [x] Window settings (title, size, decorations, resizable, always-on-top, corner style) apply immediately.
- [x] Apply settings changes through the `RuntimeApp::runtime_settings_changed` callback automatically.
- [ ] Every setting application must report `Applied`, `Degraded`, or `Rejected` with a reason and apply path — extend to all remaining settings (AA, bloom, AO, shadow quality).
- [ ] Support apply paths for graph rebuilds and deferred/device-level changes.

### Debug shell

- [x] Expose HDR, AA, post stack, render targets, pass timings, memory, graph inspection, backend/capability details, and runtime setting results via a consistent debug overlay.
- [x] Support screenshot/export from the shell.
- [ ] Support shader and asset hot reload from the shell.
- [x] Make debug image registration a renderer/runtime service, not testbed-local state.

Acceptance: a new graphical app gets the serious renderer, diagnostics, settings, and debug shell by default.

---

## Priority 2 — Benchmark Harness And Reference Scene

Build a benchmark harness before adding more renderer features. It defines what "realistic enough" and "fast enough" mean.

### Benchmark harness

- [x] Add a first-party benchmark mode that records CPU/GPU frame time, P95/P99, pass timings, memory, upload bandwidth, visible/submitted triangle counts, and draw/dispatch counts. (`AppRuntime::start_benchmark` / `stop_benchmark`, `BenchmarkReport`, `BenchmarkFrameSample`, `FrameStats`)
- [x] Export machine-readable benchmark reports. (`BenchmarkReport::to_json` via `serde_json`)
- [ ] Add screenshot/export support for fixed camera frames.
- [ ] Add screenshot comparison hooks with tolerances suitable for temporal rendering.

### Reference scene

- [ ] Create `examples/realistic_reference_scene` as the primary renderer regression example — do this after the engine is ready to support it.
- [ ] Include HDR lighting and an indoor/outdoor exposure transition.
- [ ] Include glass/translucency, wet surfaces, foliage/clutter, dense static geometry, animated objects, emissives, shadows, and camera motion.
- [ ] Include debug views for G-buffer channels, depth, Hi-Z, motion vectors, material IDs, light clusters/visibility, shadow cascades, OIT, exposure, and final tonemapped output.
- [ ] Provide deterministic camera paths for benchmark and screenshot comparison runs.
- [ ] Add asset/scenario scale presets: smoke, default, stress, and pathological.
- [ ] Make the reference scene the default target for renderer regression checks.

Acceptance: a renderer change can be judged against numbers and images, not subjective impressions.

---

## Priority 3 — Truly GPU-Driven Render Path

The biggest performance unlock is replacing CPU object expansion with persistent GPU scene data, GPU transform generation, GPU culling, GPU compaction, and indirect-count draws.

### Object source and transform generation

- [x] Store compact GPU object source buffers: `GpuTransformSourceData` (112B TRS + prev-TRS + flags + bounds), `transform_source_buffer` in `RenderWorldGpuSceneState`, upload via `prepare_gpu_transform_sources`.
- [x] Generate world matrices, previous matrices, normal matrices, and render bounds on the GPU. (`render_world_transform_build.slang`, `RenderWorldGpuTransformBuildPass::execute`)
- [x] Stop treating CPU-materialized model matrices and world bounds as the default render input. (`DeferredPass::draw_gpu_driven` uses MRT indirect G-buffer fill; `Scene::draw_gbuffer_render_world_bins` dispatches compute + GPU-indirect draws.)
- [x] Preserve previous/current transform buffers for temporal passes. (`current_matrix_buffer` / `previous_matrix_buffer` in `RenderWorldGpuSceneState`)

### Visibility and draw generation

- [x] Replace per-batch GPU culling with one cull dispatch per view/pass over the render world. (`render_world_cull.slang`, `RenderWorldGpuCullPass::execute`, `Scene::dispatch_render_world_gpu_passes`)
- [x] Wire Hi-Z occlusion into the render-world culling path for the deferred G-buffer. `dispatch_render_world_gpu_passes` accepts `depth: Option<&GraphImage>` + `proj`; calls `hiz_pass.execute_history` to build the Hi-Z pyramid and returns previous-frame pyramid to cull pass. `DeferredPass::draw_gpu_driven` pre-allocates `gbuffer_depth` to share it with Hi-Z + G-buffer fill.
- [x] Implement draw/dispatch compaction with GPU-written visible counts. (`render_world_draw_generate.slang` atomically writes `render_world_visible_draw_count`; `render_world_visible_instances` remapping table; `RenderWorldGpuDrawGenerationPass::execute`)
- [x] Use `draw_indirect_count` / backend equivalent where supported. (`prepare_gpu_draw_generation` uses `engine.caps().features.draw_indirect_count` to select the path; `draw_mesh_indirect_count_mrt_with_push_constants_and_depth` added.)
- [x] Provide an explicit fallback path where indirect-count is unavailable. (Both `DrawIndirect` and `DrawIndirectCount` paths exist; scene selects based on caps.)
- [ ] Support two-phase occlusion where measurements justify it: previous-visible first, then newly visible after depth refresh.

### Persistent bins

- [x] Build persistent mesh/material/pipeline bins from stable object IDs. (`RenderWorldPersistentBins::from_states`, keyed by `RenderWorldBinKey` — pipeline × geometry × material-shader × vertex-layout × render-state × mesh.)
- [x] Batch by shader class, render state, vertex-layout class, mesh/meshlet pass kind, and pipeline state — not by material instance. (`RenderWorldBinKey` explicitly excludes material instance ID.)
- [x] Keep the legacy scene/batch path only as compatibility/fallback, not the performance target. (Legacy `Scene::drawable_batches()` loop preserved in `draw_impl` when `render_world` is `None`; GPU-driven path activates when `scene.gpu_cull_active()`.)

Acceptance: CPU render submission scales with changed scene intent and pass setup, not object count.

---

## Priority 4 — Centralized Bindless Materials And Resource Tables

Photoreal scenes need many materials, textures, samplers, decals, reflection probes, lights, and per-material parameters. Per-object/per-material binding is not acceptable on the fast path.

- [x] Define stable material IDs owned by the render/material registry. (`render_world::MaterialId` — already existed as the stable slot handle; `MaterialRegistry` now manages the GPU table at those slots.)
- [x] Store material parameters in one GPU-resident material table. (`MaterialRegistry` — 4096-entry `StructuredBuffer<GpuMaterialEntry>`, 64 bytes/entry, persistent GPU buffer.)
- [x] Store texture and sampler references as bindless indices. (`GpuMaterialEntry` stores `albedo_idx`, `normal_idx`, `roughness_metallic_idx`, `emissive_idx`, `sampler_idx` as `u32` bindless heap indices. `Image::bindless_handle()` is now auto-populated at creation time for all sampled, non-depth, non-transient engine images.)
- [x] Upload dirty material ranges instead of rewriting whole tables. (`MaterialRegistry::flush` tracks `dirty_start`/`dirty_end` and uploads only the changed sub-range.)
- [ ] Batch by material shader class and render state, not material instance.
- [ ] Add scene-wide tables for lights, decals, probes, and other frequently indexed render resources.
- [ ] Expose a bindless fast path and an explicit degraded fallback path for weaker hardware.
- [ ] Report when a material/resource feature is unavailable, degraded, or using fallback bindings.

### Bindless foundation (in-progress as part of Priority 4)

These items establish the layers needed before the full bindless-primary path is complete:

- [x] **Image auto-registration**: Every sampled, non-depth, non-transient engine image is auto-registered in the bindless heap at `Engine::create_image` time. `Image::bindless_handle()` returns the stable `u32` index. No manual `register_bindless_image` calls needed for asset textures.
- [x] **`frame.bind_material_table`**: One-liner to wire `MaterialRegistry` into the reflection-driven bind group builder for any shader that declares `StructuredBuffer<GpuMaterialEntry> material_table`.
- [x] **`material_table.slang`**: Engine-standard shader header declaring `GpuMaterialEntry`, `material_table`, and helpers (`material_sample_albedo`, `material_sample_normal_ts`, etc.).
- [ ] **Bindless set 0 auto-injection**: Auto-prepend the bindless set to all reflected pipeline layouts so any shader can access `g_bindless_textures[]` without explicitly declaring unbounded arrays. Requires set-numbering convention (`set 0 = bindless`, `set 1+ = per-pass`) to be enforced across all engine shaders.
- [x] **Engine-global name registry**: `Engine::register_global_buffer` / `register_global_image` — when `build_reflected_bind_group` cannot resolve a resource by name from per-frame registered bindings, falls back to the engine-wide registry. `MaterialRegistry::flush` auto-registers `"material_table"` on first flush, eliminating per-frame `frame.bind_material_table()` calls for shaders that declare the binding.
- [ ] **`frame.bind_image` transparent bindless routing**: When an image has a `bindless_handle` and the reflected binding is a `SampledImage`, route through the bindless heap (push constant index) instead of allocating a descriptor set slot. Requires shaders to use `g_bindless_textures[NonUniformResourceIndex(idx)]` — coordinate with the set-convention migration.
- [x] **Buffer auto-registration**: Non-transient `STORAGE` buffers are auto-registered in the bindless heap at `Device::create_buffer` time. `Buffer::bindless_handle()` returns the stable `u32` index (previously tracked only internally; now exposed at the engine-level `Buffer` API).

Acceptance: adding many material instances should mostly increase data size, not pipeline count, descriptor churn, or draw-call count.

---

## Priority 5 — Production Vulkan Infrastructure

The Vulkan backend is the reference implementation. Keep it ambitious, but make capability and fallback behavior honest.

### Recording and pipelines

- [x] Track draw/dispatch call counts per frame in `CommandContext`; cache in `FramedCommands`; expose via `Backend::frame_draw_dispatch_counts` → `Device::frame_draw_dispatch_counts`; wire to `RuntimeWorkloadDiagnostics::draw_count` / `dispatch_count` in `finish_and_present`.
- [ ] Record real draw/dispatch work from pass callbacks or pass work descriptions throughout the default renderer.
- [x] Engine requests all modern Vulkan features automatically at startup: `dynamic_rendering`, `synchronization2`, `timeline_semaphores`, `push_descriptor`, `graphics_pipeline_library`, `extended_dynamic_state3`, `vertex_input_dynamic_state`, `shader_object`, `memory_priority`, `custom_border_color`, `conditional_rendering`, `maintenance5`, `maintenance6`, `dynamic_rendering_local_read`, `null_descriptor`, `depth_clip_enable`, `calibrated_timestamps`, `shader_module_identifier`. All as optional — falls back gracefully when unavailable.
- [x] `BackendFeature` enum and `BackendFeatures` struct extended with 7 new variants: `Maintenance5`, `Maintenance6`, `DynamicRenderingLocalRead`, `NullDescriptor`, `DepthClipEnable`, `CalibratedTimestamps`, `ShaderModuleIdentifier`. All detected via `AvailableFeatureChain` and enabled in `FeatureRequest`.
- [x] **Slang reflection read/write correctness** — `slang_binding_type_to_kind` now extracts the `MUTABLE_FLAG` for TYPED_BUFFER/RAW_BUFFER types; non-mutable storage buffers (`StructuredBuffer<T>`) get `ShaderResourceAccess::Read` instead of `ReadWrite`. `reflected_buffer_read_names` now includes read-only storage buffers alongside uniform buffers. `reflected_buffer_write_names` now excludes read-only storage buffers. Cross-queue barriers for readonly storage inputs (e.g., `render_world_visible_instances` → G-buffer draw) are now correctly `ShaderWrite → ShaderRead` instead of `ShaderWrite → ShaderWrite`.
- [x] **Descriptor buffer error logging** — `write_descriptor_to_buffer` (GFX-7a path) now logs `tracing::error!` with the handle and failure reason when a resource lookup fails instead of silently leaving the descriptor zeroed (GPU null pointer path).
- [ ] Complete Slang compiler integration and reflection-driven pipeline layout generation.
- [x] Finish persistent pipeline cache behavior for runtime and asset-driven shader variants. (`pipeline_cache_path()` / `load_pipeline_cache_file()` / `save_pipeline_cache_file()` fully implemented; loaded at startup from `~/.cache/sturdy-engine/pipeline_cache_v2.bin`, checkpointed after every 8 new pipelines, saved on shutdown.)
- [ ] Add pipeline cache and shader compilation diagnostics to runtime reports.
- [ ] Validate dynamic rendering, graphics pipeline library, shader objects, and fallback pipeline paths against the reference scene.
- [x] Wire push descriptor layout flag — `DescriptorRegistry` detects groups where all bindings have `UpdateRate::Draw`, creates those layouts with `PUSH_DESCRIPTOR_BIT_KHR`, skips pool allocation, and the reflection builder (`build_reflected_bind_group`) automatically routes draw-rate bindings into `PushDescriptorSetDesc` which is wired to `PassDesc::push_descriptor_set`. Activated when a shader declares bindings at set=3 (`[[vk::binding(x, 3)]]`).
- [x] Shader objects end-to-end — `ShaderProgram` compiles `VkShaderEXT` vertex+fragment objects alongside the pipeline when `shader_object` feature is enabled. `record_fullscreen_shader_pass` uses `ShaderBinding::ShaderObjects` when both objects are available; the compiled pipeline serves as the render-state anchor. `Engine::create_shader_object` + `ShaderObject` RAII wrapper added.

### Memory and transfers

- [x] **`BufferUsage::GPU_ONLY`** — Hint that a buffer is exclusively GPU-resident (never CPU-written). Vulkan backend prefers `DEVICE_LOCAL` memory; falls back to `HOST_VISIBLE` on integrated/budget GPU. Wired to all GPU-driven compute output buffers: `current_matrix_buffer`, `previous_matrix_buffer`, `normal_matrix_buffer`, `world_bounds_buffer`, `visibility_flags_buffer`, `draw_indirect_buffer`, `draw_count_buffer`, `visible_instance_buffer`. These buffers now live in GDDR on discrete GPU — major bandwidth improvement for large GPU-driven scenes.
- [x] **`PassWork::FillBuffer`** — GPU-side buffer zeroing via `vkCmdFillBuffer`. Allows `GPU_ONLY`/`DEVICE_LOCAL` buffers to be reset without a CPU write. `RenderFrame::fill_buffer(&buf, value)` records a transfer pass on the async compute queue. Used to zero `draw_count_buffer` before each draw generation dispatch.
- [x] **`PassWork::CopyBuffer` + `Frame::copy_buffer` + `Frame::upload_buffer_data`** — Buffer-to-buffer copy pass (staging → DEVICE_LOCAL). Enables the GPU_ONLY path for any buffer type. `Frame::upload_buffer_data(dst, data)` allocates from UploadArena, writes CPU data, and records the copy. `Engine::create_gpu_buffer(data, usage)` wraps this in a synchronous one-shot frame.
- [x] **DEVICE_LOCAL vertex/index buffers** — Mesh `upload_slice` for VERTEX/INDEX buffers now calls `Engine::create_gpu_buffer` on Vulkan (detected via `parallel_secondary_recording_supported()`), allocating in DEVICE_LOCAL memory and staging the upload. GPU reads vertex/index data from VRAM instead of BAR/system RAM.
- [x] **Cross-frame bind group cache** — `Engine` holds `BindGroupFrameCache` (hash → `Arc<BindGroup>`). `build_reflected_bind_group` checks the cache before calling `vkAllocateDescriptorSets`. Hit rate is high for stable scenes (same G-buffer images, shadow atlas, material table bound every frame). Cache entries are evicted after `BIND_GROUP_CACHE_TTL = 4` frames. `held_bind_groups` is now `Vec<Arc<BindGroup>>`. Frame advance (`advance_bind_group_frame`) is called in `flush()`.
- [ ] Use allocator-backed suballocation everywhere instead of ad hoc resource memory paths.
- [ ] Make transient render-graph resource aliasing part of the default allocation path.
- [x] Expose `RenderFrame::upload_uniform<T>` with `UniformBinding` for push-descriptor UBO binding via transient pool. (`PushDescriptorBinding::UniformBuffer`, `register_raw_buffer`, `register_transient_buffer_handles`)
- [ ] Finish upload ring/staging allocator integration for assets and dynamic data.
- [ ] Add readback paths for diagnostics, benchmark counters, screenshot/export, and GPU-generated stats.
- [x] Wire `Device::memory_budget()` to `RuntimeWorkloadDiagnostics::memory_used_bytes` / `memory_budget_bytes` each frame in `finish_and_present`.
- [x] Track upload bandwidth in renderer diagnostics: `UploadArena::bytes_uploaded()` accumulates per-frame staging bytes; `RenderFrame::frame_upload_bytes()` exposes it; wired to `RuntimeWorkloadDiagnostics::upload_bytes` in `finish_and_present`.
- [x] Track transient allocation pressure in renderer diagnostics. (`AliasHeapRegistry::total_bytes()` → `Device::transient_aliased_bytes()` → `RuntimeWorkloadDiagnostics::transient_aliased_bytes` wired in `finish_and_present`.)
- [x] `Scene::submitted_triangle_count()` from GPU-driven bins; `AppRuntimeFrame::report_scene_workload(scene)` feeds `submitted_triangles` into diagnostics.
- [x] Transient buffer pool exhaustion diagnostic — `BufferPool::alloc` now logs a `tracing::warn!` with requested size, cursor, and capacity when the pool runs out. `usage_fraction() -> f32` accessor added for telemetry.

### Barriers and queues

- [x] **Stage mask completeness** — `stage_mask()` (legacy) and `stage_mask2()` (sync2) updated: `ShaderRead`/`UniformRead` now include `MESH_SHADER_EXT | TASK_SHADER_EXT | RAY_TRACING_SHADER_KHR`; `ShaderWrite` now includes `FRAGMENT_SHADER` (for OIT/storage writes in fragment) plus mesh/task/RT stages. `UniformRead` matches `ShaderRead` for stage coverage.
- [x] **Access mask correctness** — `DepthWrite` now includes both `DEPTH_STENCIL_ATTACHMENT_READ | DEPTH_STENCIL_ATTACHMENT_WRITE` (depth test reads happen alongside writes). `RenderTarget` now includes `COLOR_ATTACHMENT_READ | COLOR_ATTACHMENT_WRITE` (blending reads the attachment). Both legacy and sync2 paths updated.
- [x] **Depth image layout fix** — `image_layout_for_format(state, format)` returns `DEPTH_STENCIL_READ_ONLY_OPTIMAL` for depth images in `ShaderRead` state instead of the incorrect `SHADER_READ_ONLY_OPTIMAL`. Used in barrier recording (sync2 and legacy paths) and push descriptor binding. `image_descriptor_layout` also format-aware in `descriptors.rs`.
- [x] **Cross-queue release barriers** — `CompiledGraph` now has `release_buffer_barriers_per_batch` and `release_image_barriers_per_batch`. The compiler generates release barriers for every acquire barrier that crosses queue families. `submit_graph` records release barriers at the END of each batch's command buffer before `vkEndCommandBuffer`, satisfying the Vulkan spec requirement for EXCLUSIVE queue ownership transfers.
- [x] **Uniform read state precision** — `dispatch_on_queue` now assigns `RgState::UniformRead` (→ `UNIFORM_READ` access) for uniform buffer reads and `RgState::ShaderRead` (→ `SHADER_READ` access) for read-only storage buffers, via `reflected_read_state_for`. Previously both used `ShaderRead`.
- [x] Name all VkQueue handles at device creation time using `VK_EXT_debug_utils` when available (graphics, compute, transfer, async_compute, dma).
- [x] **`ShaderPassIntent::async_compute()`** — submits to `AsyncCompute` queue (mirrors `compute()` but routes to dedicated compute queue where available). HiZ pyramid build (`hiz_pass.rs`) migrated to async_compute; now groups in the same batch as transform-build/cull/draw-gen, eliminating 2 unnecessary batch boundaries and semaphore synchronizations per frame.
- [x] **Pipeline cache time-based flush** — `PipelineRegistry::maybe_checkpoint` now also triggers after 5 minutes regardless of pipeline count. Prevents cache loss in short sessions that compile fewer than 8 new pipelines.
- [ ] Keep synchronization and queue behavior observable in graph/debug output.
- [ ] Treat backend feature use as capability-driven, not assumed.
- [x] NVIDIA Reflex latency sleep (`Surface::latency_sleep()`) and AMD Anti-Lag frame-start (`Device::anti_lag_frame_start()`) called in `AppRuntime::acquire_frame` before input sampling; `LatencyMode` setting changes wire to `Device::set_reflex_mode` / `set_anti_lag_mode` in `apply_pending_runtime_settings`.

- [x] **Automatic mipmap generation** — `TextureUploadDesc::generate_mips: bool` (default `true` for sampled textures). `upload_texture_2d` computes `floor(log2(max(w,h)))+1` mip levels, creates images with `COPY_SRC|COPY_DST|SAMPLED`, and appends a `GenerateMipmaps` pass after the staging copy. Block-compressed textures skip mip gen (driver handles them). Significant quality and bandwidth improvement.
- [x] **Parallel GLTF image decoding** — `upload_images` now phase-splits: (1) parallel `rayon::par_iter` converts all images to RGBA8 simultaneously; (2) sequential frame upload with one `flush/wait/recycle`. Multi-image GLTF files decode N images in parallel.
- [x] **Parallel shader compilation** — `ShaderProgram::new` compiles vertex+fragment/compute shaders in parallel via `rayon::join`. The Slang compilation phase already releases the Device lock, so both stages can run concurrently.
- [x] **Parallel ECS world extraction** — `RenderWorld::extract_from_world` now: (1) allocates missing objects sequentially, (2) generates commands in parallel via `flat_map_iter`, (3) pushes all commands in a single lock acquisition via `push_batch`. Eliminates N×6 per-entity lock acquisitions.
- [x] **Parallel `transform_source_states`** — collect+filter over state HashMap uses `par_iter` for visible object filtering.
- [x] **Parallel `gpu_scene_entries_from_states`** — phase-split: parallel `filter_map` per object, sequential BTreeMap aggregation for stable ordering.
- [x] **Parallel `extract_from_world`** — parallel per-entity command generation, single batch push.
- [x] **`parking_lot::Mutex/RwLock`** throughout Vulkan backend and engine — replaces `std::sync::Mutex/RwLock` across `VulkanBackend` (8 Mutex + 2 RwLock), `Device`, `Engine`, `RenderWorld`, `MeshProgram`, `ShaderProgram`, `AssetLoader`, `Runtime`, and all internal modules. `parking_lot` is 2-5× faster on uncontended paths (the common case) and eliminates poisoning overhead.
- [x] **Rayon-parallel object iteration** — `scene::prepare_internal` phase-splits into parallel data computation (rayon `par_iter` over objects) then sequential batch aggregation; `gpu_scene_entries_from_states` and `transform_source_states` also parallelized.
- [x] **Persistent staging arena pool** — `Engine::staging_pool` holds reset `UploadArena` instances across frames. `begin_frame` reuses a pooled arena (zero HOST_VISIBLE alloc on steady state); `Frame::recycle_staging_arena` returns it after GPU wait.
- [x] **Reflection-driven async compute auto-routing** — `dispatch_compute_auto` now analyzes shader parameter bindings: if no registered frame images are sampled (only storage buffers/images), the pass automatically routes to `QueueType::AsyncCompute` for GPU overlap; otherwise stays on Compute.
- [x] Signal semaphore stage `BOTTOM_OF_PIPE` (was `ALL_COMMANDS`), timeline wait stage `NONE` (was deprecated `TOP_OF_PIPE`).
- [x] `#[inline]` on all barrier helper functions (`stage_mask`, `access_mask`, `stage_mask2`, `access_mask2`, `image_layout`, `image_layout_for_format`).
- [x] `Vec::with_capacity` for per-pass barrier vectors in compile path.

Acceptance: higher-level renderer work should not fight memory churn, pipeline stutter, missing readbacks, or opaque synchronization failures.

---

## Priority 6 — Temporal Correctness

Temporal correctness is required before investing heavily in advanced RT, denoising, frame generation, motion blur, or sophisticated reflections.

- [x] Use camera-local motion vectors by default. (`CameraMotionVectorPass` reconstructs world pos from G-Buffer depth + camera matrices; attached to `DeferredPass` via `set_camera_motion_vectors`; output exposed through `DeferredOutput::motion_vectors`.)
- [x] Maintain previous/current transform buffers for all renderable objects. (GPU-driven path: `previous_matrix_buffer` in `RenderWorldGpuSceneState`; bound as `"previous_matrices"` in `draw_gbuffer_render_world_bins` and `draw_inner`. G-buffer vertex shaders now output `curr_clip`/`prev_clip` for per-object motion vectors. Legacy path uses camera-only reprojection as fallback.)
- [ ] Correctly handle skinned and animated object motion vectors.
- [ ] Separate screen-locked and camera-locked passes from world-space temporal passes.
- [x] Support jittered projection and expose jitter state to all passes that need it. (`DeferredPass::draw_with_camera` takes `&SceneCamera` and automatically uses `camera.jittered_projection()` + `camera.previous_view_proj`; jitter UV returned to TAA via `camera.jitter_uv`; `draw_with_camera_gpu_driven` does the same for the GPU-driven path.)
- [ ] Track history resources explicitly in the render graph.
- [ ] Add motion-vector validation scenes and debug views.
- [ ] Validate TAA against camera cuts, disocclusion, transparency, emissives, and animated geometry.
- [ ] Add reactive mask handling for transparency/particles before temporal upscaling or frame generation depends on it.

Acceptance: TAA, exposure, bloom history, motion vectors, and future upscaling/denoising inputs are stable under camera and object motion.

---

## Priority 7 — Core Photorealism Stack

Do not add disconnected effects. Wire the existing modules into the default renderer and make them part of the measured reference scene.

### HDR, exposure, tone mapping, and bloom

- [x] Implement real auto-exposure luminance reduction and exposure history. `AutoExposurePass` runs GPU histogram + adapt compute each frame; `adapted_ev` returned via `RuntimePostProcessOutput::adapted_ev`; testbed converts to linear exposure scale with `exp2(REF_EV - adapted_ev)` and feeds into `TonemapParams::exposure` on the next frame (1-frame lag, imperceptible at 60+ fps).
- [ ] Make auto-exposure a supported runtime feature with runtime settings toggle (currently always-on in testbed; `AutoExposureConfig::enabled` controls it).
- [ ] Use mip-based bloom in the default post stack.
- [ ] Stabilize HDR tonemapping and color-management behavior across SDR/HDR surfaces.
- [ ] Add debug views for luminance, exposure, bloom mips, and tonemap output.

### Lighting, shadows, and environment

- [ ] Wire image-based lighting and reflection probes into the default renderer path.
- [ ] Stabilize cascaded directional shadows under camera motion.
- [ ] Add point/spot shadow atlas or clustered shadow management suitable for many lights.
- [ ] Integrate ambient occlusion with depth/normal data and runtime controls.
- [ ] Keep shadow, AO, and reflection cost visible in pass timings.

### Materials and scene phenomena

- [ ] Define the default glass/translucency policy and use OIT where needed.
- [ ] Add wet-surface, decal, layered-material, terrain, and foliage material coverage to the reference scene.
- [ ] Ensure sampler controls and generated mips are used consistently by material shaders.

Acceptance: the default renderer can present a coherent realistic scene without per-example custom effect wiring.

---

## Priority 8 — Dense-Scene Systems

After GPU culling and bindless materials are stable, focus on scene density and asset scale.

- [ ] Move mesh storage toward mega-buffer layouts.
- [ ] Add GPU LOD selection.
- [ ] Integrate meshlets and mesh-shader paths where available.
- [ ] Provide a fallback indexed-indirect path for hardware without mesh shaders.
- [ ] Add texture streaming and residency tracking.
- [ ] Add runtime mip/asset streaming.
- [ ] Add foliage/clutter instancing built for GPU culling and bindless materials.
- [ ] Defer virtualized/sparse geometry beyond the first measured GPU-driven path unless the reference scene proves it is required.

Acceptance: dense geometry and foliage scale through GPU visibility, LOD, streaming, and indirect work rather than CPU draw expansion.

---

## Priority 9 — Render Graph Scheduling Across Queues

Only deepen graph scheduling after the renderer has real passes, real resources, and useful timing feedback.

- [ ] Model graphics, compute, and transfer queue work in renderer-facing graph diagnostics.
- [ ] Schedule async compute only where measurements show overlap benefits.
- [ ] Validate cross-queue synchronization and ownership transfers.
- [x] Add parallel command recording infrastructure: `SecondaryPool` (one pool+buffer per slot), `CommandContext::record_parallel_compute` (records items into secondaries on `std::thread::scope` threads, executes via `vkCmdExecuteCommands`), `FramedCommands::prepare_parallel_secondary_capacity`, exposed through `Backend` trait, `Device`, and `Engine`. Call `engine.prepare_parallel_secondary_capacity(N)` before first use.
- [x] `PassWork::MultiMeshIndirectDraw` — collapses N bin draw passes into ONE render pass (one `vkCmdBeginRenderingKHR` / N mesh-switch draws / `vkCmdEndRenderingKHR`). N−1 render-pass boundaries eliminated; especially impactful on tile GPUs. Wired into `draw_gbuffer_render_world_bins` via `draw_multi_mesh_indirect_mrt`.
- [x] `RenderFrame::dispatch_async_compute_auto` — targets `QueueType::AsyncCompute` instead of `Compute`. GPU-driven compute passes (transform build, cull, draw gen) now route to async compute; on hardware with a dedicated compute queue they overlap GPU execution with shadow-map rendering. Falls back to graphics queue on hardware without a dedicated compute family.
- [ ] Wire parallel secondary recording into the shadow cascade pass (4 cascades in parallel).
- [ ] Feed pass timing data back into scheduling decisions.
- [ ] Optimize transient resource lifetimes and aliasing.
- [ ] Minimize barriers after correctness is validated.
- [ ] Support generated resources and effect assets through the graph.

Acceptance: queue scheduling improves measured frame time or resource pressure and never hides synchronization hazards.

---

## Priority 10 — Portability Without Lowest-Common-Denominator Design

- [ ] Keep Vulkan as the reference implementation on Linux and a primary path elsewhere.
- [ ] Route D3D12 and Metal through the same capability model as features are implemented.
- [ ] Add capability checks for high-end features instead of weakening the API globally.
- [ ] Allow degraded fallback behavior, but report it explicitly.
- [ ] Ensure runtime settings explain unavailable/degraded features clearly.
- [ ] Avoid claiming multi-backend parity until the Vulkan path is fast and measured.

Acceptance: the engine remains portable without making the high-end path dishonest or slow.

---

## Priority 11 — CPU + GPU Maximum Parallelism

Goal: saturate all GPU queues every frame and eliminate CPU submission as a bottleneck. The engine submits graphics, async compute, and DMA work simultaneously and records command buffers in parallel on the CPU. At the end of this priority, a GPU profiler should show all three queues active for the majority of a frame with no idle gaps.

### Parallel CPU command recording

- [ ] **Parallel batch recording**: `submit_graph` currently records batches sequentially. Record independent batches on separate threads using the existing `SecondaryPool` infrastructure. Batch independence is determined by the compiled graph's dependency edges — batches with no shared resource writes can be recorded concurrently. Use `rayon::scope` to launch one thread per independent batch; join before submission.
- [ ] **Shadow cascade parallel secondaries**: Wire `record_parallel_compute` into the shadow cascade pass. 4 cascades = 4 secondaries recorded on rayon threads simultaneously, executed via `vkCmdExecuteCommands` in one primary. This alone eliminates ~3× redundant per-cascade state setup overhead.
- [ ] **Parallel G-buffer bin recording**: When `MultiMeshIndirectDraw` bin count exceeds a configurable threshold, split into N secondaries recorded in parallel. Each secondary owns its vertex/index buffer range and its indirect draw slice.
- [ ] **Deferred skinning on worker threads**: Skinning compute dispatches for animated meshes are recorded on worker threads concurrently with shadow recording.

### True simultaneous multi-queue submission

- [ ] **Simultaneous queue submit**: After all batches are recorded, submit graphics + async compute + DMA to their respective queues in a single round of `vkQueueSubmit2` calls. Express cross-queue dependencies with timeline semaphores, not pipeline barriers that stall a single queue.
- [ ] **Async compute saturation**: The async compute queue must never idle during a frame. While graphics renders shadow maps, async compute runs: current-frame Hi-Z rebuild, next-frame cull pass (ping-pong buffers), or post-processing compute chains.
- [ ] **DMA queue for continuous streaming**: Texture mip decoding, BC compression uploads, and mesh uploads route to the DMA queue. The graphics queue never stalls for upload fences.
- [ ] **Frame pipeline overlap**: Model frame N's GPU execution overlapping with frame N+1's CPU preparation. Timeline semaphores control resource access: acquire next-frame resources only after the GPU signals completion of frame N−`frames_in_flight`. CPU records frame N+1's command buffers while GPU executes frame N.
- [ ] **Per-queue submit diagnostics**: Track submitted command buffer count, semaphore wait count, and submit latency per queue per frame. Report in `RuntimeWorkloadDiagnostics`.

### Async compute pass chaining

- [ ] **Post-processing chain on async compute**: HDR histogram, auto-exposure adaptation, SRD denoise dispatches, and bloom downsample/upsample all route to async compute once the current-frame G-buffer is resolved. These have no dependency on the shadow or lighting passes running simultaneously on graphics.
- [ ] **BLAS/TLAS update on async compute**: Dynamic-object BVH refits run on async compute, overlapping shadow-map rendering on graphics.
- [ ] **Occlusion readback on async compute**: Copy visible-count buffer to readback memory asynchronously without stalling the graphics queue.
- [ ] **Next-frame Hi-Z pre-build**: After depth resolve, trigger Hi-Z rebuild on async compute. The following frame's cull pass can begin as soon as Hi-Z is ready, before graphics has finished presenting the current frame.

Acceptance: GPU profiler shows graphics + async compute queues both active for ≥ 70% of frame time; DMA queue feeds streaming without stalling graphics; CPU submission stays below 0.3 ms on the GPU-driven path.

---

## Priority 12 — Shader Intelligence & Adaptive Render Strategy

The engine reflects every shader at compile time and uses that information to automatically decide execution strategy, pipeline layout, queue routing, variant selection, and runtime quality scaling. The CPU submits intent; the engine decides how to fulfill it most efficiently given the scene, the hardware, and the frame budget.

### Shader Capability Profile

- [x] **`ShaderCapabilityProfile`**: Computed once at `ShaderProgram` / `ComputeProgram` creation from Slang reflection. Stored on the program and queried with zero runtime overhead. Fields:
  - `async_compute_eligible: bool` — no sampled images at sets > 0 (non-bindless, per-frame render targets); pass can run on async compute. Bindless heap images (set 0) excluded from check.
  - `bindless_only: bool` — all non-push-constant resources are at set 0 (bindless heap); no per-frame/per-pass descriptor set allocation needed.
  - `requires_mesh_shading: bool` — entry point uses task or mesh stages.
  - `requires_ray_tracing: bool` — entry point uses raygen, any-hit, or closest-hit stages.
  - `wave_ops_used: bool` — shader declares subgroup/wave capability in SPIR-V; subgroup size must be respected at pipeline creation.
  - `workgroup_size: [u32; 3]` — from SPIR-V `OpExecutionMode LocalSize`.
  - `storage_write_image_names: Box<[Box<str>]>` — writable storage image names.
  - `sampled_image_names: Box<[Box<str>]>` — sampled image names at sets > 0 (cached for async compute routing).
  - `push_constant_bytes: u32`.
  - `estimated_wave_occupancy: u8` — heuristic (0–100) from workgroup size; 64-invocation workgroups score 100.
- [x] **Profile cached on program**: No reflection queries at frame time. The scheduler reads plain struct fields.

### Automatic queue routing (extended)

- [x] **`dispatch_compute_auto` uses profile**: Fast path (`async_compute_eligible = true`) takes zero locks; slow path uses cached `sampled_image_names` instead of scanning `reflection.parameters` — no per-call reflection scan.
- [ ] **Async compute chain detection**: When two consecutive compute passes both have `async_compute_eligible = true` and the second reads only what the first writes, they are fused into a single async compute batch. No queue family ownership transfer between them.
- [ ] **Graphics-forced passes**: Passes with non-empty `storage_write_image_names` that were produced as render targets force graphics queue and receive a semaphore wait relative to the render target producer.

### Automatic pipeline variant selection

- [ ] **Feature-gated shader variants**: Shaders declare capability macros (`BINDLESS_ENABLED`, `MESH_SHADER_ENABLED`, `RAY_QUERY_ENABLED`, `WAVE_OPS_ENABLED`). At compile time the engine compiles all valid combinations given the current `BackendFeatures`. At bind time the engine selects the best-matching variant without user code.
- [ ] **Zero-fallback policy**: When a shader requires a feature absent from `BackendFeatures`, the engine selects the nearest degraded variant and logs a warning. The shader author does not choose the fallback path.
- [ ] **Active variant in debug overlay**: `ShaderCapabilityProfile` records which variant is live. The debug overlay shows it per pass.

### Bindless set 0 auto-injection

- [ ] **Set 0 auto-prepend**: When `ShaderCapabilityProfile::bindless_only` is true, the pipeline layout builder auto-prepends set 0 = bindless heap (`g_bindless_textures[]`, `g_bindless_samplers[]`, `g_bindless_buffers[]`) without any explicit shader declaration. Shaders that index bindless resources get it for free.
- [ ] **Set numbering convention**: Set 0 = bindless (auto-injected when eligible), set 1 = per-frame (reflection-driven), set 2 = per-pass (push descriptors). All engine shaders follow this. Old shaders with manual set 0 declarations are migrated.
- [ ] **Transparent bindless routing for `frame.bind_image`**: When an image has a `bindless_handle` and the reflected binding is a sampled image, push the bindless index as a push constant instead of allocating a descriptor set slot.

### Automatic workgroup size tuning

- [ ] **Device-query-driven workgroup selection**: For compute shaders that declare a `WORKGROUP_SIZE` specialization constant, query `VkPhysicalDeviceSubgroupProperties::subgroupSize` and `maxComputeWorkGroupInvocations`. Select the largest power-of-2 size that aligns with the subgroup and fits in device limits. Write the specialization constant before pipeline creation.
- [ ] **Image-space tuning default**: Fullscreen or image-space compute passes default to square workgroups (8×8 or 16×16 depending on device) for better L1 cache locality.

### Render Strategy Selector

- [x] **`RenderStrategySelector`**: Frame-level system in `render_strategy.rs` owned by `AppRuntime`. Reads last-frame GPU time + draw count; produces `FrameRenderStrategy` each frame. Accessible via `runtime.current_render_strategy()`.
  - `dynamic_resolution_scale: f32` — 0.5–1.0, reduced 5%/frame when over-budget.
  - `lod_bias: f32` — 0.0–2.0, increased 0.2/frame after resolution floor.
  - `shadow_cascade_count: u32` — 1–4, reduced to 1 when heavily over-budget.
  - `shadow_cascade_max_distance: f32`.
  - `vrs_quality: VrsQuality` — `Full`, `ContentAdaptive`, `Pipeline2x2`, `Off`.
  - `occlusion_mode: OcclusionMode` — `SinglePass` or `TwoPass`.
  - `meshlet_path: bool`, `async_compute_overlap: bool`, `rt_reflections: bool`.
- [x] **Timing-driven quality adaptation**: 5% over-budget → reduce quality (resolution → LOD → shadow cascades). Under-budget 3 consecutive frames → restore one step. Imperceptible step sizes.
- [x] **Scene-state-driven choices**: `occlusion_mode` auto-selects `TwoPass` when draw_count ≥ 5000, `SinglePass` below that threshold.
- [x] **Target frame time**: Set via `runtime.strategy_selector_mut().set_target_frame_ms(Some(16.6))`. `None` = strategy fixed at maximum quality.
- [x] **Target frame time via `RuntimeSettings`**: Expose `target_frame_ms: Option<f32>` in `RuntimeSettingsSnapshot` and `RuntimeSettingKey::TargetFrameMs`; standard settings transactions now update `RenderStrategySelector` immediately.
- [ ] **Strategy observable in debug overlay**: All `FrameRenderStrategy` fields shown in debug overlay with per-field change reason.

Acceptance: on hardware with all features, the engine selects the optimal shader variant, routes all eligible compute to async compute, injects bindless set 0 without manual descriptor code, and adapts quality automatically to meet a configured frame-time target — with no per-frame user code.

---

## Priority 13 — Two-Phase Occlusion & Persistent Visibility

Single-pass Hi-Z culling against the previous frame's depth misses newly-visible objects after large camera moves or when occluders disappear. Two-phase occlusion closes this gap for the general case without sacrificing the fast path when the camera is stable.

- [ ] **Phase 1 — Previous-frame Hi-Z cull**: Existing path. Dispatch cull + draw-gen against the previous frame's Hi-Z pyramid. Record indirect draws for Phase-1-visible objects.
- [ ] **Phase 2 — Depth-only fill**: Render Phase-1-visible objects to a depth-only render target (no MRT G-buffer output, no fragment shader). This is cheap: hardware early-Z eliminates most pixel work.
- [ ] **Hi-Z rebuild from Phase-2 depth**: Build a new Hi-Z pyramid on async compute from the Phase-2 depth buffer. This is the current-frame occlusion reference.
- [ ] **Phase 3 — Current-frame Hi-Z cull**: Dispatch a second cull + draw-gen pass against the new Hi-Z. Finds objects visible now that were occluded in Phase 1.
- [ ] **Phase 3 G-buffer fill**: Render Phase-3 newly-visible objects to the full MRT G-buffer. These objects use the same per-bin bins as Phase 1, just a different visibility mask.
- [ ] **GPU-driven conditional Phase 3**: If Phase 3's visible count (GPU-written atomic) is zero, skip the Phase-3 G-buffer fill using `ConditionalRenderingDesc`. Zero CPU involvement in the skip decision.
- [ ] **Persistent visibility cache**: Store Phase-1 visible object IDs across frames. Objects in the cache that fail Phase-1 cull (became occluded) are removed. Objects that pass Phase-3 (newly visible) are added. On a stable camera, Phase-3 dispatches but finds nothing — the conditional skip fires immediately.
- [ ] **`OcclusionMode` switch in `RenderStrategySelector`**: Two-phase is enabled only when scene depth complexity justifies it. Below a triangle/occluder threshold, `OcclusionMode::SinglePass` avoids the extra cull + depth-only pass overhead entirely.

Acceptance: no missing objects after large camera cuts; static-camera overhead is one extra cull dispatch + a conditional skip that fires immediately; GPU profiler shows Phase-3 G-buffer time as zero on a static camera.

---

## Priority 14 — Meshlet Pipeline & Cluster Geometry

Meshlets enable per-cluster visibility on the GPU — eliminating overdraw before the vertex shader runs, not after. They also unlock per-cluster LOD selection, backface cone culling, and sub-mesh occlusion.

### Meshlet build pipeline

- [ ] **Offline meshlet generation**: At asset load time (or as a pre-build step using `meshopt` or equivalent), split each mesh into clusters of up to 64 vertices / 64 triangles. Store per meshlet: vertex range, index range, vertex count, triangle count, bounding sphere (center + radius in object space), backface cone (apex + axis + cutoff angle).
- [ ] **Mega-buffer layout**: All meshlet vertex data resides in one engine-global `VertexMegaBuffer` (DEVICE_LOCAL, bindless). All index data in one `IndexMegaBuffer`. Per-mesh `MeshletDescriptor` stores the byte range into each. Eliminates per-draw vertex/index buffer binds entirely on the meshlet path.
- [ ] **`GpuMeshletEntry`**: Stored in a persistent bindless buffer alongside `GpuInstanceData`. Fields: base vertex, base index, vertex count, triangle count, bounding sphere, cone. 48 bytes per meshlet.
- [ ] **LOD cluster ranges**: Coarser LOD levels are a contiguous range of coarser-resolution meshlets in the same mega-buffer. GPU LOD selection is a single range offset — no separate LOD mesh allocation.

### Task + Mesh shader pipeline

- [ ] **Task shader culling**: Each task shader thread culls one meshlet. Tests: frustum cull (sphere vs. 6 planes), Hi-Z occlusion (project sphere to screen, sample Hi-Z mip), backface cone cull (dot product of view and cone axis vs. cutoff). Surviving meshlets call `EmitMeshTasksEXT(1, 1, 1)`. Culled meshlets emit nothing.
- [ ] **Mesh shader output**: Mesh shader reads vertex positions and attributes from the mega-buffers via bindless indices. Applies the instance transform (from `current_matrix_buffer` via bindless). Fills all MRT attribute outputs. No vertex buffer binding in the render pass.
- [ ] **Task-shader LOD selection**: Task shader computes projected bounding sphere diameter, looks up the LOD table, and emits tasks for the appropriate meshlet range. LOD transitions are per-cluster, not per-object.
- [ ] **Indexed-indirect fallback path**: When `BackendFeatures::mesh_shading` is false, the engine renders the same mega-buffer data via standard indexed indirect draws with object-level GPU LOD selection. Quality is lower (no per-cluster cull, coarser LOD) but correctness is maintained.
- [ ] **`PassWork::DrawMeshShaderIndirect` wiring**: Route the existing `DrawMeshShaderIndirect` pass work variant into the G-buffer fill path when the meshlet path is active via `FrameRenderStrategy::meshlet_path`.
- [ ] **Meshlet diagnostics**: Track meshlet cull rate (culled / total per frame) via GPU atomic + readback. Report as `meshlet_cull_efficiency` in `RuntimeWorkloadDiagnostics`.

Acceptance: triangle throughput for occluded scenes ≥ 40% better than indexed-indirect baseline on the same hardware; meshlet build step adds no visible hitch to asset loading; fallback path produces visually correct output.

---

## Priority 15 — Clustered Lighting & Shadow Atlas

Directional shadows alone cannot represent a photoreal scene. A clustered light system and shadow atlas unlock dozens to hundreds of point, spot, and area lights at AAA scale without per-light CPU submission or descriptor-set churn.

### Clustered light assignment

- [ ] **Screen-space 3D cluster grid**: Divide the view frustum into a configurable cluster grid (default 16×9×24, depth sliced logarithmically). Each cluster is a frustum sub-region represented as a min/max AABB in view space.
- [ ] **Light assignment compute pass on async compute**: For each light, test its bounding volume (sphere for point, cone for spot) against each cluster. Write the light index into a per-cluster light list (bindless storage buffer). Runs on async compute, overlapping shadow-map rendering on graphics.
- [ ] **Per-cluster light index list**: Flat buffer of packed light indices with a per-cluster `(offset, count)` header. Shader reads `cluster_lights[cluster_offsets[id].start + i]` for each contributing light. Entire structure is bindless.
- [ ] **`GpuLightEntry`**: Stored in a persistent bindless light table. Fields: world position, direction, color×intensity, inner/outer cone angles, range, shadow atlas tile (or `~0` for unshadowed). 64 bytes per light.
- [ ] **Deferred shading integration**: The deferred shading pass computes the cluster index from NDC position + linear depth, reads the light count, loops over contributing lights, evaluates each light's PBR BRDF contribution using `GpuLightEntry`, and accumulates into the HDR lighting buffer.

### Shadow atlas

- [ ] **Shadow atlas texture**: One `Depth32Float` texture (configurable size, default 4096×4096). Sub-tiles are allocated per shadow-casting light. Tile size is proportional to light importance (screen-space coverage heuristic).
- [ ] **Atlas tile allocator**: Frame-level allocator assigns tiles to shadow-casting lights. Static lights cache their tile assignment across frames. Dynamic lights reallocate each frame.
- [ ] **Point light shadows**: 6 face renders per point light into 6 atlas tiles arranged as a horizontal strip. Optional dual-paraboloid projection to reduce to 2 tiles on budget-constrained frames.
- [ ] **Spot light shadows**: One tile per spot light.
- [ ] **Shadow atlas parallel recording** (requires Priority 11): Each shadow light tile is a separate render pass. Record in parallel using the `SecondaryPool` infrastructure from Priority 11. N lights = N secondaries recorded simultaneously.
- [ ] **Receiver plane depth bias**: Per-tile slope-scale bias computed from the light direction relative to the surface normal. No manual bias constant required.
- [ ] **Atlas visibility pre-cull**: Before recording shadow tiles, run a GPU cull pass per light frustum to determine which objects are shadow receivers/casters. Only visible objects appear in shadow draws.

Acceptance: scenes with 64 dynamic shadow-casting lights render with no per-light descriptor churn; shadow quality is stable under camera motion; atlas memory usage is tracked and reported.

---

## Priority 16 — Content-Adaptive Variable Rate Shading

VRS reduces pixel shader invocations in visually flat or fast-moving regions without perceptible quality loss. Content-adaptive VRS computes a per-tile shading rate from scene data each frame rather than applying a fixed global rate.

- [ ] **VRS rate image compute pass on async compute**: Generates a `FRAGMENT_SHADING_RATE_ATTACHMENT_OPTIMAL` image each frame. Runs on async compute, overlapping shadow rendering. Per 8×8 tile:
  - Sample luma gradient from the previous frame's tonemapped output. High gradient (edges, detail) → 1×1. Low gradient (sky, large flat surfaces) → 2×2 or 4×4.
  - Sample motion vector magnitude. Fast motion → reduce rate (temporal artifacts are more visible under motion).
  - Sample G-buffer roughness. Low roughness (sharp speculars) → 1×1. High roughness (diffuse) → 2×2.
  - Clamp to device `fragmentShadingRateAttachmentTexelSize` limits.
- [ ] **Deferred shading pass VRS attachment**: Bind the generated rate image as `PassDesc::shading_rate_image`. Requires `BackendFeatures::vrs_attachment` and `BackendFeatures::dynamic_rendering`. Both are already auto-requested at startup.
- [ ] **Tier fallback**: When `vrs_attachment` is unavailable but `vrs_pipeline` is present, fall back to pipeline shading rate (uniform 2×2 for background/sky passes, 1×1 for foreground). Reported as `VrsQuality::Pipeline` in `FrameRenderStrategy`.
- [ ] **VRS debug view**: Color-coded overlay in the debug shell showing per-tile effective shading rate (1×1 green, 2×2 yellow, 4×4 red).
- [ ] **VRS efficiency metric**: Track shaded sample count via performance query vs. full-rate equivalent. Report as `vrs_shading_efficiency: f32` in `RuntimeWorkloadDiagnostics`.

Acceptance: VRS reduces deferred shading GPU time by ≥ 15% on scenes with large flat or background areas; no perceptible quality degradation in static screenshots vs. full-rate baseline; efficiency metric reported correctly.

---

## Priority 17 — Hybrid RT Reflections

Ray-traced reflections close the gap between raster quality and full path-tracing for specular surfaces. The hybrid approach traces one reflection ray per pixel, denoises with SRD, and composites over the raster G-buffer.

**Prerequisites**: Priority 4 (bindless), Priority 6 (temporal correctness), production BLAS/TLAS infrastructure from Priority 5. A lightweight denoiser (temporal accumulation + spatial filter) will be designed for RT reflections specifically when this priority begins — not a framework built speculatively.

- [ ] **BLAS build pipeline**: Build a compacted BLAS per static mesh cluster at load time. Compact after initial build (readback + re-build into smaller allocation). Store BLAS handle alongside `GpuMeshletEntry`.
- [ ] **Dynamic TLAS update on async compute**: Each frame, build the TLAS from all visible instance transforms read from `current_matrix_buffer`. TLAS build runs on async compute, overlapping shadow rendering. Only rebuild full TLAS when instances appear/disappear; otherwise use incremental update.
- [ ] **Reflection raygen pass**: One raygen dispatch per screen pixel. Sample G-buffer normal + roughness + position. For roughness below `reflection_roughness_threshold`, trace a mirror-jittered reflection ray. Store hit distance, hit surface albedo, hit normal in a half-res `Rgba16Float` buffer.
- [ ] **Reflection denoising**: Design a minimal temporal + spatial denoiser specifically for the RT reflection signal. Inputs: hit distance guide, G-buffer motion vectors, surface normal + roughness. Output: stable specular buffer. Design from this concrete use case — not a general-purpose framework.
- [ ] **Composite pass**: Blend the denoised reflection over the raster specular contribution based on material roughness and Fresnel factor. Below `reflection_roughness_threshold`, RT dominates; above it, IBL/SSR handles specular.
- [ ] **`RenderStrategySelector` RT gate**: Enable RT reflections only when `BackendFeatures::ray_tracing` is true AND the frame budget has ≥ 2 ms of headroom after raster passes complete. Disable gracefully and fall back to IBL when over-budget.
- [ ] **RT diagnostics**: Track raygen invocation count, acceleration structure memory, TLAS build time, and SRD denoise time per frame. Report in `RuntimeWorkloadDiagnostics`.

Acceptance: mirror and near-mirror surfaces show geometrically correct reflections not possible with SSR; SRD keeps the signal stable under camera motion; RT cost is tracked and budget-gated automatically by `RenderStrategySelector`.

---

## Explicitly Deprioritized For Now

Do not spend roadmap time on these until the measured GPU-driven renderer and reference scene are stable:

- live backend/device migration
- advanced window transparency/blur polish
- full UI widget ecosystem
- neural rendering experiments
- exotic material graphs
- multi-backend parity before Vulkan is fast and measured
- perfect editor tooling before the reference scene exists
- full path tracing (RT reflections in Priority 17 are the stepping stone; full path tracing follows once that is stable and measured)

---

## Best Immediate Commit Sequence

1. ~~Route existing testbed examples through RuntimeApp instead of GameApp/EngineApp.~~ ✅ Done (main, shader_playground, plot_demo, coordinate_validation, ui_demo migrated).
2. ~~Migrate `game_2d` and `game_3d` to `RuntimeApp` (needs fixed-step support or InputHub pattern).~~ Removed obsolete binaries; future fixed-step samples should use `RuntimeApp`.
3. ~~Expose `AppRuntimeFrame::run_default_post_process` so new apps don't need per-example bloom/AA/tonemap wiring.~~ ✅ Done.
4. ~~Add screenshot/export and graph inspection from the debug shell.~~ ✅ Done.
5. ~~Implement GPU transform build from compact object buffers.~~ ✅ Done (`render_world_transform_build.slang`, `RenderWorldGpuTransformBuildPass`, `dispatch_render_world_gpu_passes`).
6. ~~Replace per-batch culling with one GPU cull pass per view.~~ ✅ Done (`render_world_cull.slang`, `RenderWorldGpuCullPass`; deferred G-buffer uses `draw_gpu_driven`).
7. Wire Hi-Z into occlusion culling for the deferred G-buffer pass (currently only wired through `Scene::draw_inner` forward path).
8. ~~Add indirect-count compaction.~~ ✅ Done (`draw_mesh_indirect_count_mrt_with_push_constants_and_depth`, `DrawIndirectCount` path in draw gen).
9. Add centralized GPU material/resource tables.
10. Finish auto exposure, mip bloom, HDR tonemap validation, and TAA validation.
11. Create `examples/realistic_reference_scene` once the engine can support it end-to-end.

This sequence keeps the engine focused on the real goal: dense, realistic scenes with measurable performance where the CPU submits compact intent and the GPU performs visibility, batching, and draw generation.
