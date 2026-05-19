# Sturdy Engine Roadmap

## Product Direction

Sturdy Engine is worth using in three modes:

1. **Shader playground** — open a window, write Slang, see it run, tweak parameters live.
2. **Graphical apps and custom UI** — standalone tools, dashboards, inspectors, editors.
3. **Games** — including a path toward footage that can plausibly read as real life.

The simple path must be the best path, not a toy path. Each mode should feel complete without requiring the user to build the runtime shell themselves. The architecture must scale from simple to deep without rewrites.

### What's working today

**Core infrastructure**: Vulkan backend with block-based sub-allocation (256 MiB device-local, 64 MiB host-visible blocks), precise pipeline barriers, 2-frame-in-flight command contexts, pool-slab descriptor allocation, O(n) pass scheduling, incremental pipeline cache saves. `Engine::memory_budget()` exposes VRAM usage stats per frame. Render graph compiles passes, infers dependencies, submits without CPU stalls. GPU timestamp queries per pass.

**ECS**: Generational entity/component system with sparse-set storage, multi-component queries, transform hierarchy, built-in components (Transform, Velocity, Health, SceneLink, Name), Schedule runner. Fully tested. Currently single-threaded (`Schedule::run(&mut World)` — sequential, exclusive world borrow). Parallel scheduling is Track ECS-MT.

**Game loop shell**: `FrameClock`, `InputHub` (keyboard/mouse/gamepad), `ActionMap`, fixed-timestep accumulator, pointer-lock. `GameApp` + `run_game` zero-config shell. `HeadlessApp` + `run_headless` for windowless compute. 2D and 3D game samples.

**Deferred PBR pipeline**: G-Buffer fill (albedo/normal/roughness/emissive), GGX specular (Trowbridge-Reitz NDF, Smith G2, Schlick Fresnel), Lambertian diffuse, split-sum IBL with SH9 diffuse, BRDF LUT, multi-scattering (Kulla-Conty). Cascaded shadow maps (4 cascades, PCF or PCSS, texel-snap). PCSS: 16-tap blocker search + 32-tap variable-kernel PCF. Spot light shadow maps (up to 4, PCF). Point light shadow maps (up to 4, dual-paraboloid). BVH-culled point/spot/rect/sphere/disk lights. OIT (Per-Pixel Linked List). Forward-only path. Procedural sky (Rayleigh + Mie). Environment map blending. Normal mapping.

**Unified material system**: `MaterialSurface`, `UnifiedMaterial` expression trees, `GBufferFillVariant`/`ForwardLitVariant`/`ShadowVariant` codegen. `DeferredPass::tick_hot_reload()` clears the variant cache when shared modules change. `MeshProgram` implements `Reloadable`.

**Asset pipeline**: PNG/JPEG/WebP/BMP/TGA/TIFF/HDR/EXR, GLTF 2.0 (full PBR + extensions), OBJ, STL. Automatic mip generation. Tangent generation. `AssetHandle<T>`. Checkerboard fallback. Hot reload.

**Geometry backends**: `ClassicVertex`, `ComputeIndirect` (CPU frustum culling + indirect draw), `VirtualMesh` (meshlet generation via meshopt, RT proxy). Indirect draw/dispatch variants in render graph.

**Shader playground**: Full auto-UI from reflection, slider/toggle/text fields, presets, screenshot export.

**Sprites, 2D, text**: `SpriteBatch`, `DebugDraw2d`, text rendering (shaping, atlas, tiling). Clay UI bindings.

**Post-processing**: Bloom, TAA, FXAA, MSAA, tone mapping. Mip pyramid ops with graph validation.

**Platform**: Surface-lost and device-lost recovery, zero-size window suspension.

---

## API Design Contract

Two laws. Both are absolute. Breaking either is a bug, not a feature request.

---

### Law 1 — Zero config, full control

**Every subsystem must work perfectly with zero configuration and expose every dial when the user wants one.**

Every major subsystem ships a `*Config` struct where `Default::default()` produces a production-quality result. Every field is `pub`, documented with its valid range and trade-off. The user never opens a Config to get a good game — they open it when defaults are wrong for their case.

Adding a knob only accessible by editing source code is not allowed.

---

### Law 2 — Every API is available on every thread, always

**No engine API is restricted to the render thread, the main thread, or any other specific thread.**

If you can call it at all, you can call it from any thread. There are no "main-thread-only" methods, no thread-affinity checks, no panics because you called something from a worker. The engine handles synchronisation internally and transparently.

The only constraint that exists is a hardware one: GPU queue submission is serialized per Vulkan queue. This is invisible to the caller — no API exposes queue submission directly. From the user's perspective, every engine call is simply safe on every thread.

**What this means in practice:**

```rust
// All of these are valid from any thread, at any time:
let buf  = Engine::global().create_buffer(desc)?;
let tex  = Engine::global().load_texture_2d("rock.png")?;
scene.set_transform(obj, mat);         // atomic — no lock needed
scene.commands().add_mesh(mesh, prog); // queued — applied at frame start
world.resource::<Engine>().caps();     // from an ECS system on a worker thread
```

**Why this matters:** Unity restricts most of its API to the main thread. Godot's scene tree is single-threaded. The result is that any serious multithreading requires contorted workarounds — job systems that can't touch the scene, callbacks bounced to the main thread, thread-safe alternatives that only cover a fraction of the API. We refuse to build that. If a feature exists, it is thread-safe. If we cannot make it thread-safe, we do not ship it until we can.

Adding a thread-affinity restriction to any public API requires explicit sign-off and must be documented as a temporary limitation with a concrete plan to remove it.

---

## Priority Order

The items below are ordered so that each layer multiplies the value of what comes after it. Ground work first.

```
Foundation (performance ceiling)
  └─ Vulkan extension coverage (Track GFX-1 + GFX-2)  → prerequisite for all foundation work
  └─ GPU-driven scene + bindless          → unlocks 100k+ draw counts
  └─ Temporal upscaling (FSR 3.1)         → makes expensive GI/RT viable at 60fps
  └─ Texture compression                  → VRAM headroom for real content
  └─ Async compute                        → free perf from queue overlap

Visual quality (Track GFX-3 required first, then in dependency order)
  └─ RT pipeline foundation (Track GFX-3) → prerequisite for all RT visual features
  └─ Ambient occlusion (GTAO + RTAO)      → biggest cheap visual leap
  └─ Reflections (SSR → RT)              → correct specular from scene geometry
  └─ Global illumination (SSGI → RTGI → PTGI)
  └─ Shadows (VSM → RT shadows)
  └─ Volumetric fog + light shafts
  └─ Motion blur (scatter-as-gather)
  └─ Depth of field (physically based)
  └─ Post-processing stack (tone mapping, grain, lens effects)

Geometry
  └─ Virtual geometry system (full Nanite equivalent)
       DAG → cluster select → software raster → vis buffer → material resolve → streaming
  └─ Ray tracing geometry integration (TLAS/BLAS for RT AO, shadows, reflections, GI)

Application capabilities (parallel with visual quality, after GFX-1 + GFX-2)
  └─ Video encode/decode (Track GFX-4)    → cutscenes, replay, streaming, video textures
  └─ External resource interop (GFX-5)    → camera capture, compositor, cross-process GPU memory
  └─ GPU enhancements (GFX-6)             → DGC, Reflex/Anti-Lag, cooperative matrix, optical flow
  └─ Next-gen descriptors (GFX-7)         → descriptor buffer, descriptor heap

Engine + ECS thread safety (foundational — do alongside GPU-driven work)
  └─ Engine::global() OnceLock accessor ✓ — any thread, zero cost
  └─ World resource system ✓ — insert_resource / resource / resource_mut / remove_resource
  └─ Fine-grained resource locks — image/buffer/pipeline/shader registries independent
  └─ Parallel command recording — ThreadRenderContext + secondary CBs per worker
  └─ Thread-safe Scene — atomic transforms, SceneCommands queue, SceneView
  └─ Parallel asset loading — load/decode/upload on rayon workers from any thread
  └─ Parallel PSO compilation — no more render-thread stutter on first material use
  └─ Parallel ECS schedule — wave-based, rayon, WorldView + WorldCommands

Code organisation (continuous guardrail)
  └─ Keep large systems split by ownership, not by arbitrary line count
  └─ Keep public facades thin and compatibility re-exports stable while moving code
  └─ Separate runtime code, test fixtures, shader fixtures, generated code, and demos

Physics, UI, Platform (parallel, after foundation)
```

---

## Immediate Stub And Correctness Queue

These items were found by scanning for stubs, incomplete implementations, and placeholder behavior in the current tree. Handle these before taking more roadmap feature work unless a user explicitly asks for a different item.

- [x] `crates/sturdy-engine-core/src/backend/vulkan/mod.rs`: Vulkan `VkShaderEXT` creation/destruction is implemented behind `create_shader_object`, including aligned SPIR-V storage, descriptor/push-constant layout propagation, explicit destruction, and drop-time cleanup.
- [x] `crates/sturdy-engine-core/src/backend/vulkan/commands.rs`: `PassDesc::shader_binding` now records either `vkCmdBindPipeline` or `vkCmdBindShadersEXT`, shares descriptor/push-constant binding, records required shader-object dynamic state, and uses `pass.pipeline` as the graphics render-state/fallback anchor.
- [x] `crates/sturdy-engine-core/src/backend/vulkan/commands.rs`: `PassWork::DecodeVideoFrame` / `EncodeVideoFrame` return `Unsupported`. Either implement Vulkan video command recording and queue routing, or mark the render-graph video pass scaffolding as explicitly non-executable until sessions exist.
- [x] `crates/sturdy-engine-core/src/backend/vulkan/mod.rs`: `create_video_session` no longer returns a fake success or immediate destroy. Vulkan video sessions are retained in a backend registry with bound memory and destroyed through `destroy_video_session`; frame encode/decode passes remain non-executable until parameters, DPB management, and command recording are wired.
- [x] `crates/sturdy-engine-core/src/backend/vulkan/commands.rs`: `PassWork::ExecuteGeneratedCommands` / `PreprocessGeneratedCommands` return `Unsupported`. The pass variants are explicitly non-executable until DGC layouts/resources/recording exist, and runtime DGC `BackendFeatures` are disabled.
- [x] `crates/sturdy-engine-core/src/backend/vulkan/commands.rs`: `PassWork::EstimateOpticalFlow` records `vkBindOpticalFlowSessionImageNV` and `vkCmdOpticalFlowExecuteNV` through backend-owned `VkOpticalFlowSessionNV`; optical-flow image usage flags chain the required Vulkan pNext metadata at image creation.
- [x] `crates/sturdy-engine-core/src/backend/vulkan/mod.rs`: BLAS/TLAS build-size queries reject `AccelerationStructureBuildMode::Compact` even though AS compaction command recording exists. Compaction size queries are now source-driven and return a conservative source-size upper bound with zero scratch so callers can create a valid destination before exact compacted-size readback exists.
- [x] `crates/sturdy-engine-core/src/backend/vulkan/mod.rs`: `latency_mode()` reports Reflex/Anti-Lag availability, but `set_reflex_mode` / `set_anti_lag_mode` still use backend default `Unsupported`. `latency_mode()` now reports stored controllable modes, and Anti-Lag is wired through `vkAntiLagUpdateAMD` when `VK_AMD_anti_lag` is enabled.
- [x] `crates/sturdy-engine/src/ui_renderer.rs`: text atlas quads are rendered as solid white rectangles because atlas texture binding is TODO, and scissor/image commands are skipped. Text atlas pages are now bound and sampled, Clay image commands draw through an image registry, and rectangle/border/image quads are CPU-clipped by active scissor commands.
- [x] `crates/sturdy-engine/src/ui_renderer.rs`: glyph quad coordinates are assumed to already be normalized. `draw_ui_text` now uses the provided viewport dimensions to convert pixel-space glyph positions to NDC before drawing.
- [x] `crates/sturdy-engine/src/post_process.rs`: auto-exposure and lens dirt/flare configs are public but shader implementations are missing. Lens dirt/flare now has a real post-process shader pass; auto-exposure is documented and rejected at runtime until luminance reduction/history support exists.
- [x] `crates/sturdy-engine/src/ecs/components.rs`: `Transform::look_at(target, up)` ignores the `up` vector. It now builds a stable look-at basis from forward and caller-supplied up, with fallback handling for degenerate or parallel inputs.

---

## Foundation — Performance Ceiling

These items multiply the value of everything above them. Do them before adding more visual features.

### GPU-Driven Scene + Bindless (Track 8)

Without this, the CPU submits one draw call per mesh. At 10,000 objects the CPU becomes the bottleneck. With it, a single indirect dispatch handles 1,000,000 objects.

**8a — Bindless descriptor system**
- [x] Enable `VK_EXT_descriptor_indexing`; create one large descriptor heap for textures, samplers, storage images, and storage buffers; assign stable `u32` indices at resource creation when bindless is available.
- [x] `BindlessHandle<T>`: a `u32` index valid for the resource lifetime. Binding = storing index; sampling = `textures[handle.index].sample(...)`.
- [ ] Per-material data in a single GPU-resident `StructuredBuffer<MaterialData>` indexed by `material_id`; eliminate per-draw descriptor set allocation.
- [ ] Mega-buffer draw path: each draw carries only a 4-byte push constant (index into `DrawData`); vertex shader reads transform, material ID, per-object constants from `DrawData[index]`.
- [x] Gate bindless behind `BackendFeatures::bindless`; fall back to grouped-descriptor path.
- [x] Validate descriptor indices in debug builds; readable error instead of GPU hang.

**8b — Fully GPU-driven scene submission**
- [ ] GPU scene buffer: one `GpuInstanceData` per scene object (model matrix, AABB, LOD bias, material ID, visibility flags); upload once on change.
- [ ] Single GPU compute dispatch for frustum culling + HZB occlusion; writes `DrawIndexedIndirectCommand` per visible instance.
- [ ] `vkCmdDrawIndexedIndirectCount` (Vulkan 1.2): GPU-written draw count drives actual draw count, no CPU readback.
- [ ] Two-phase occlusion culling: Phase 1 renders last frame's visible set; Phase 2 re-tests newly unoccluded objects against fresh depth buffer.
- [ ] `GpuDrivenScene` as drop-in replacement for `Scene`; same `VirtualMesh` assets and `UnifiedMaterial` definitions.

**8c — Variable Rate Shading (VRS)**
- [ ] Wire `VK_KHR_fragment_shading_rate` (already detected) into the render path.
- [ ] Tier 1 VRS: per-draw shading rate (1×1, 1×2, 2×1, 2×2) for screen-edge and low-motion regions. Target: 20–30% shading cost reduction.
- [ ] VRS image generated from motion vectors + luminance gradient each frame.
- [ ] Disable VRS on the tonemap pass; only inside G-Buffer and deferred lighting.

**8e — PSO caching**
- [x] Pipeline library at first run: compile all `UnifiedMaterial` variants to disk-cached PSOs.
- [x] PSO pre-warm pass during loading screens; block game start until all active-scene PSOs are ready.
- [x] `PsoWarmupReport`: compile times, cache hit rates, total variant count.

### Temporal Upscaling + Frame Generation (Track 10)

FSR 3.1 is open-source MIT, Vulkan-native, all GPU vendors. Renders at 50–70% of native resolution, outputs native quality. Frame generation doubles effective frame rate. Without upscaling, expensive GI and RT features are unusable on mid-range hardware.

**10a — FSR 3.1 (primary)**
- [ ] Integrate FSR 3.1 upscaling via AMD FidelityFX SDK: feed motion vectors, depth, colour, camera jitter; receive upscaled native-resolution output.
- [ ] Integrate FSR 3 frame generation: optical-flow frame interpolation, doubles effective frame rate on all hardware.
- [ ] Expose `FsrConfig { quality: FsrQualityMode, sharpness: f32, mip_lod_bias: f32, auto_exposure: bool, frame_gen: bool, reactive_mask_auto: bool }`. `Default` = Quality mode, frame gen on, auto-exposure on.
- [ ] Auto-detect camera cuts and teleports via velocity discontinuity; pass FSR reset flag without app code.
- [ ] Reactive mask auto-generated from transparent + particle alpha to prevent ghosting.

**10b — XeSS 2.x (fallback for Intel Arc)**
- [ ] Integrate XeSS 2.x via Intel open SDK: XMX hardware path (Arc GPUs), DP4a fallback (all others).
- [ ] `XessConfig { quality: XessQualityMode, sharpness: f32, use_jitter: bool }`.

**10c — Unified upscaler interface**
- [ ] `UpscalerConfig::auto()`: XeSS XMX (Intel Arc) → FSR 3.1 (all others). Frame gen on by default.
- [ ] `render_resolution(display_resolution, quality)` from active mode; render targets allocate at render resolution.
- [ ] Tone mapping runs **after** the upscaler at display resolution. All pre-upscale passes (bloom, AO, GI) run at render resolution.
- [ ] `UpscalerReport` in `GraphReport`: active upscaler, render/display resolution, upscale ratio, frame gen active, latency estimate.

### Texture Compression (Track 11d)

Uncompressed RGBA8 costs 4 bytes/texel. BC7 costs 1 byte/texel with near-lossless quality. A scene with 50 textures at 2048×2048 = 800 MB uncompressed → 200 MB compressed. Without compression real content blows VRAM budgets.

**Latest research**: BC7 Mode 6 for opaque colour (Leadwerks approach — full 8-bit precision per channel), BC5 for normal maps (two-channel, reconstruct Z), BC6H for HDR/emissive, BC4 for single-channel (roughness/AO). On mobile: ASTC 4×4 (better than ETC2, roughly BC7 quality).

- [x] At asset load time, transcode uncompressed textures to BC3sRGB (albedo), BC5 (normal maps), BC4 (roughness/AO/single-channel) via `texpresso`. BC7 pending a pure-Rust encoder; HDR stays Rgba16Float (no BC6H encoder available). Auto-selects format from filename heuristics.
- [ ] Mobile/integrated fallback: ASTC 4×4 when BC7 is unavailable.
- [ ] `TextureDesc::prefer_compressed: bool` (default true; false for render targets and UAVs).
- [x] Cache compressed result as `.sce-cache/<stem>.<format_tag>.sceb` next to source; invalidated on source mtime + dimensions change.
- [x] `compress_textures` CLI tool for pre-compressing asset directories in the release pipeline.

### Async Compute (Track 11b)

Overlap shadow rendering, culling, and GI updates with the previous frame's G-Buffer pass. Free performance on any GPU with a dedicated async compute queue (most discrete GPUs since 2015).

- [x] Detect and use dedicated async compute queue; expose `QueueType::AsyncCompute` in the render graph.
- [x] `PassDesc::queue: QueueType`; render graph compiler groups passes by queue (`build_batches`), routes each batch to the correct `VkQueue`, and chain semaphores / timeline semaphores provide automatic inter-batch cross-queue synchronization.
- [ ] Schedule HZB build, cluster LOD selection, and GI probe updates on the async compute queue.
- [x] DMA/transfer queue (QueueType::Dma + QueueFamilyMap::dma + select_dma(); falls back to graphics when no TRANSFER-only family is available) for texture decode+upload; signal semaphore on completion; consume before first shader read.
- [x] `GpuTimeline` diagnostics: per-queue utilisation and cross-queue stall gaps.

### GPU Memory Infrastructure (Track 11a)

The block sub-allocator (256 MiB blocks) already exists. `Engine::memory_budget()` exposes VRAM stats. Remaining items:

- [x] `BufferPool` for transient per-frame scratch (uniform uploads, staging): ring allocator in host-visible memory; sub-allocates from a single persistent block; resets at frame start. Zero allocation overhead for constant buffer updates.
- [x] Aliased memory for G-Buffer images: the render graph already tracks lifetimes — commit the alias plan to the allocator so non-overlapping transient images share VkDeviceMemory. Saves ~50–100 MB/frame on a full G-Buffer + shadow atlas.
- [x] Warn in console when `memory_budget().over_budget()` is true (device_local > 80%).
- [x] Dedicated allocations for resources > 64 MiB: skip the pool and use a direct `vkAllocateMemory`; prevents a single large texture from fragmenting the whole pool.

---

## Graphics API Extension Coverage (Track GFX)

All visual-quality tracks, GPU-driven rendering, ray tracing, video, and async compute depend on specific Vulkan extensions being wired and active in the backend. This track makes every detected extension actually usable, adds detection for extensions currently absent, and sequences new infrastructure work in dependency order.

**Rule**: no visual-quality, RT, video, or high-level GPU-driven feature may be marked complete while a GFX item it depends on remains open.

The phases are ordered by dependency. GFX-1 must finish before Foundation tracks move beyond their current state. GFX-2 must finish before visual quality tracks begin. GFX-3 must finish before any RT feature. GFX-4, GFX-5, and GFX-6 are independent and can proceed in parallel with visual quality work after GFX-1 and GFX-2.

---

### GFX-1 — Core API modernization

The highest-leverage changes: extensions already detected but unused, plus infrastructure that everything else depends on. No feature track starts until these are done.

**GFX-1a — `VK_KHR_synchronization2` (detected, not used)**

Every `cmd_pipeline_barrier` in `commands.rs` uses the legacy two-stage API. Sync2 provides `StageMask2` + `AccessMask2` — fine-grained pairs that eliminate false dependencies. Also simplifies queue submission.

- [x] Replace all `cmd_pipeline_barrier` calls with `cmd_pipeline_barrier2` + `DependencyInfo`. Gate on `features.synchronization2`; fall back to legacy on pre-1.3 devices.
- [x] Replace both `cmd_write_timestamp` calls (lines 236, 265 in `commands.rs`) with `cmd_write_timestamp2` using `PipelineStageFlags2::TOP_OF_PIPE` / `BOTTOM_OF_PIPE`.
- [x] Replace per-batch wait-stage arrays with `SemaphoreSubmitInfo2` in the queue submission path (`commands.rs` line 320).
- [x] Add `synchronization2_khr: Option<ash::khr::synchronization2::Device>` to `VulkanBackend`; initialize when enabled on sub-1.3 devices; dispatch all sync2 calls through it.

**GFX-1b — `VK_KHR_dynamic_rendering` (detected, not used)**

Currently using legacy render passes and framebuffers. Dynamic rendering eliminates both, reduces pipeline coupling, removes the render-pass–to–pipeline compatibility constraint, and is required before VRS attachment mode (GFX-2a) works cleanly.

- [x] Implement `cmd_begin_rendering` / `cmd_end_rendering` path in `commands.rs`. Replace `cmd_begin_render_pass` / `cmd_end_render_pass` for all graphics passes.
- [x] Construct `VkRenderingAttachmentInfo` from `ColorTarget` / `DepthTarget` descriptors; pass via `VkRenderingInfo` to `cmd_begin_rendering`.
- [x] Add `VkPipelineRenderingCreateInfo` to graphics pipeline creation in `pipelines.rs`. Thread attachment formats from `GraphicsPipelineDesc` into the create-info chain instead of a render pass handle.
- [x] Delete `FramebufferCache` and all render pass object management from `PipelineRegistry`. Framebuffers are now created transiently per-pass in `CommandContext::record_draw_pass` and destroyed after the frame fence fires at the start of the next frame (`transient_framebuffers: Vec<vk::Framebuffer>` on `CommandContext`). The legacy render-pass path is retained for non-dynamic-rendering devices.
- [x] Gate on `features.dynamic_rendering`; keep the legacy path for portability subset devices.
- [x] **Enables**: GFX-2a (VRS attachment image now implemented). GFX-2c pipeline library and `VK_KHR_dynamic_rendering_local_read` pending.

**GFX-1c — `VK_KHR_timeline_semaphore` (detected, not used)**

`surfaces.rs` has `render_finished: Vec<vk::Semaphore>` — one binary semaphore per swapchain image. Inter-batch chain semaphores are also binary. Timeline semaphores collapse this to two monotonic counters and are a prerequisite for async compute cross-queue synchronization (Track 11b).

- [x] Replace per-surface `render_finished: Vec<vk::Semaphore>` with a single `render_finished_timeline: vk::Semaphore` + `render_finished_value: u64`. Each frame increments and signals the timeline value.
- [x] Replace inter-batch chain semaphores in `commands.rs` with timeline signal/wait pairs (`chain_timeline: vk::Semaphore`, `chain_value: u64`). Binary fallback retained when `timeline_semaphores` is false.
- [x] Expose `Device::wait_for_timeline(semaphore, value, timeout_ns)` and `Device::signal_timeline(semaphore, value)` for engine consumers. Add `Device::create_timeline_semaphore(initial_value)` and `Device::destroy_timeline_semaphore`.
- [x] Gate on `features.timeline_semaphores`; fall back to binary semaphores on Vulkan 1.0 devices.
- [ ] **Enables**: Track 11b (async compute, cross-queue semaphore chains).

**GFX-1d — `VK_KHR_buffer_device_address` (Vulkan 1.2 core) — complete**

Raw GPU virtual addresses for buffers. Required by ray tracing acceleration structures, descriptor buffer, device-generated commands, and any GPU-side linked structure. Nearly universal — this is Vulkan 1.2 core.

- [x] Detect `VK_KHR_buffer_device_address` (or `api_version >= 1.2`); add `BackendFeatures::buffer_device_address`.
- [x] Enable `bufferDeviceAddress = VK_TRUE` in `VkPhysicalDeviceBufferDeviceAddressFeatures` during device creation.
- [x] Add `Device::buffer_device_address(handle) -> Option<u64>` — returns `None` when unavailable or the buffer was not created with `BufferUsage::SHADER_DEVICE_ADDRESS`.
- [x] Gate `BufferUsage::SHADER_DEVICE_ADDRESS` on buffer creation behind `features.buffer_device_address`.
- [ ] **Required by**: GFX-3a (RT acceleration structures), GFX-6a (device-generated commands), GFX-7a (descriptor buffer).

**GFX-1e — Memory management extensions**

The current `GpuAllocator` has no budget awareness. Exceeding the budget causes silent performance cliffs on integrated and mobile GPUs.

- [x] Detect `VK_EXT_memory_budget`; add `BackendFeatures::memory_budget`.
- [x] When available, call `vkGetPhysicalDeviceMemoryProperties2` with `VkPhysicalDeviceMemoryBudgetPropertiesEXT`. Expose `Engine::memory_budget_ext() -> Option<MemoryBudgetReport>` with per-heap `budget` and `usage` in bytes.
- [x] Integrate budget into the allocator: when `device_local_usage > 80% of device_local_budget`, scale down new block sizes (256 MiB → 32 MiB) and emit a structured warning via debug utils.
- [x] Detect `VK_EXT_memory_priority`; add `BackendFeatures::memory_priority`.
- [x] When available, assign priority during `vkAllocateMemory` via `VkMemoryPriorityAllocateInfoEXT`: device-local 0.7, host-visible (staging) 0.1.
- [x] Detect `VK_EXT_pageable_device_local_memory`; add `BackendFeatures::pageable_device_local_memory`. Extension device loaded as `pageable_memory_ext` in VulkanBackend.
- [x] When available alongside `memory_priority`, call `vkSetDeviceMemoryPriorityEXT` at allocation time for render targets (0.9 priority via pageable_memory_ext).
- [x] Detect `VK_KHR_dedicated_allocation` (Vulkan 1.1 core); query `VkMemoryDedicatedRequirementsKHR` for every image; allocate a dedicated `VkDeviceMemory` when `prefersDedicatedAllocation` is true or size > 64 MiB. `Allocation::dedicated()` sentinel tracks dedicated allocations through the allocator's `dealloc` path.
- [ ] **Improves**: Track 11a (GPU memory infrastructure), streaming texture residency (Track 7f, Full Asset Pipeline).

**GFX-1f — Push descriptors (`VK_KHR_push_descriptor`, Vulkan 1.4 core)**

Eliminates descriptor pool allocation for per-draw data. Complements bindless: bindless for the resource heap, push descriptors for per-pass data too large for push constants.

- [x] Detect `VK_KHR_push_descriptor`; add `BackendFeatures::push_descriptors`.
- [x] Add `push_descriptor_khr: Option<ash::khr::push_descriptor::Device>` to `VulkanBackend`.
- [x] Add `PassDesc::push_descriptor_set: Option<PushDescriptorSetDesc>` to the render graph.
- [x] Record `cmd_push_descriptor_set_khr` before pass work when present.
- [x] `PushDescriptorSetDesc` mirrors `BindGroupDesc` but without a persistent `VkDescriptorSet` allocation.

**GFX-1g — Device fault and diagnostic breadcrumbs**

`VK_ERROR_DEVICE_LOST` currently produces no actionable information. These extensions provide pass-level crash attribution.

- [x] Detect `VK_EXT_device_fault`; add `BackendFeatures::device_fault`.
- [x] On `VK_ERROR_DEVICE_LOST` from graph submission, call `vkGetDeviceFaultInfoEXT` and return a structured `Error::DeviceLost` report with device-fault description, address records, vendor info strings, and vendor binary size.
- [x] Detect `VK_NV_device_diagnostic_checkpoints` (NVIDIA); add `BackendFeatures::device_diagnostic_checkpoints_nv`.
- [x] Insert checkpoint markers at the start and end of each render graph pass in debug builds. On device loss, surviving checkpoints identify the faulting pass. Marker value encodes `pass_idx * 2` (start) and `pass_idx * 2 + 1` (end) as a `usize` pointer.
- [x] Detect `VK_AMD_buffer_marker` (AMD); add `BackendFeatures::buffer_marker_amd`.
- [x] Write a 32-bit pass index into a host-visible breadcrumb buffer at the start and end of each pass in debug builds via `vkCmdWriteBufferMarkerAMD`. `BreadcrumbBuffer` allocated in `CommandContext`; marker = `pass_idx * 2` (start) / `+1` (end).
- [x] Detect `VK_EXT_device_address_binding_report`; add `BackendFeatures::device_address_binding_report`.
- [x] Register the device-address-binding-report callback when available to log GPU VA binding events for postmortem address resolution.
- [x] Detect `VK_EXT_device_memory_report`; add `BackendFeatures::device_memory_report`.
- [x] Register the device-memory-report callback in debug builds via  (static extern fn) chained into  via .

**GFX-1h — Host image copy (`VK_EXT_host_image_copy`, Vulkan 1.4 core)**

Copies images from the CPU without staging buffers. Eliminates the staging round-trip on unified-memory hardware (Apple Silicon, integrated GPUs).

- [x] Detect `VK_EXT_host_image_copy` (or `api_version >= 1.4`); add `BackendFeatures::host_image_copy`.
- [x] Expose `Device::copy_memory_to_image(handle, mip, layer, data)` — calls `vkCopyMemoryToImageEXT` when `host_image_copy` is available; integration into the asset pipeline upload path is pending.
- [x] Expose `Device::transition_image_layout_cpu(handle, new_layout)` — calls `vkTransitionImageLayoutEXT` when `host_image_copy` is available; CPU-side initial layout transitions without command buffer submission.
- [ ] **Improves**: Full Asset Pipeline (staging ring allocator savings on integrated/mobile).

---

### GFX-2 — Feature enablement

Complete these before beginning Tracks AO, Reflections, GI, Shadows, VRS, Fog, and GPU-driven culling.

**GFX-2a — VRS command recording (`VK_KHR_fragment_shading_rate`, partially wired)**

Three sub-modes (pipeline, primitive, attachment) are fully detected in `caps.rs` and `device.rs`. Pipeline-tier VRS is command-recording ready; attachment VRS still needs dynamic rendering attachment plumbing.

- [x] Add `PassDesc::pipeline_shading_rate: Option<ShadingRate>` with graphics-work validation.
- [x] Record `cmd_set_fragment_shading_rate_khr` before draws when `PassDesc::pipeline_shading_rate` is set.
- [x] Add `PassDesc::shading_rate_image: Option<ImageHandle>` (enabled when attachment-tier VRS is available). Bind `VkRenderingFragmentShadingRateAttachmentInfoKHR` inside `cmd_begin_rendering` with 16x16 texel size default.
- [x] Add `RgState::ShadingRateAttachment` to the render graph state enum; access/stage/image-layout mappings wired for all barrier functions.
- [x] Split `BackendFeatures::variable_rate_shading` into `vrs_pipeline: bool`, `vrs_primitive: bool`, `vrs_attachment: bool` matching the three sub-modes detected in `caps.rs`.
- [ ] **Depends on**: GFX-1b (dynamic rendering required for attachment VRS). **Enables**: Track 8c.

**GFX-2b — Conditional rendering (`VK_EXT_conditional_rendering`)**

Required for GPU-driven occlusion culling to skip draw calls without a CPU readback.

- [x] Detect `VK_EXT_conditional_rendering`; add `BackendFeatures::conditional_rendering`.
- [x] Add `conditional_rendering_ext: Option<ash::ext::conditional_rendering::Device>` to `VulkanBackend`.
- [x] Add `PassDesc::predicate: Option<ConditionalRenderingDesc>` to the render graph where `ConditionalRenderingDesc` holds a buffer handle, byte offset, and invert flag.
- [x] Wrap passes with a predicate in `cmd_begin_conditional_rendering_ext` / `cmd_end_conditional_rendering_ext`.
- [x] **Enables**: Track 8b (two-phase occlusion culling — Phase 2 predicate skips draws for fully occluded instances without CPU readback).

**GFX-2c — Graphics pipeline library (`VK_EXT_graphics_pipeline_library`)**

Pre-compiles disjoint pipeline state chunks that are linked at draw time. Eliminates PSO stutter for the pre-rasterization and fragment-shader stages.

- [x] Detect `VK_EXT_graphics_pipeline_library`; add `BackendFeatures::graphics_pipeline_library`.
- [x] Split `create_graphics_pipeline` into four library linkage stages: `VertexInput`, `PreRasterization`, `FragmentShader`, `FragmentOutput` via `VK_EXT_graphics_pipeline_library`. VertexInput and FragmentOutput libraries are cached by descriptor hash and reused across materials; PreRasterization (VS) and FragmentShader (FS) libraries are per-material and linked immediately. Active when `graphics_pipeline_library_enabled && dynamic_rendering`.
- [x] Material variant compilation only produces `PreRasterization` + `FragmentShader` libraries; links against cached VertexInput and FragmentOutput libraries into a final pipeline with `VK_PIPELINE_CREATE_LINK_TIME_OPTIMIZATION_EXT`.
- [x] Detect `VK_EXT_pipeline_creation_cache_control` (Vulkan 1.3 core); add `BackendFeatures::pipeline_creation_cache_control`. `VK_PIPELINE_CREATE_FAIL_ON_PIPELINE_COMPILE_REQUIRED_BIT` integration pending.
- [ ] **Enables**: Track 8e (PSO pre-warm pass, `PsoWarmupReport`). Reduces first-draw stutter from Track ECS-MT-d-6.

**GFX-2d — Extended dynamic state 3 (`VK_EXT_extended_dynamic_state3`)**

Makes almost all remaining pipeline state dynamic. Eliminates PSO variants for state that changes per-pass but is currently baked at creation time.

- [x] Detect `VK_EXT_extended_dynamic_state3`; add `BackendFeatures::extended_dynamic_state3`.
- [x] Add `RasterState::polygon_mode: PolygonMode` (Fill / Line / Point) and `depth_clamp: bool`; baked into pipeline statically or recorded dynamically when EDS3 is available.
- [x] Record `cmd_set_polygon_mode_ext` and `cmd_set_depth_clamp_enable_ext` in the render graph pass recording path when `extended_dynamic_state3` is active; add `VK_DYNAMIC_STATE_POLYGON_MODE_EXT` / `DEPTH_CLAMP_ENABLE_EXT` to pipeline dynamic state lists.
- [x] `RasterState::rasterizer_discard` wired: pipeline always declares `RASTERIZER_DISCARD_ENABLE` as dynamic, `cmd_set_rasterizer_discard_enable` recorded before every graphics draw (pipeline and shader-object paths). Color blend equation remains static.

**GFX-2e — Vertex input dynamic state (`VK_EXT_vertex_input_dynamic_state`)**

Fully dynamic vertex binding and attribute descriptions. Required for virtual geometry (Track 7) where vertex formats vary per cluster.

- [x] Detect `VK_EXT_vertex_input_dynamic_state`; add `BackendFeatures::vertex_input_dynamic_state`.
- [x] When available, create graphics pipelines with `VK_DYNAMIC_STATE_VERTEX_INPUT_EXT`; record `cmd_set_vertex_input_ext` per draw from the pipeline's vertex binding/attribute arrays.
- [ ] **Enables**: Track 7d (hardware rasterization path where mesh shader is unavailable), dynamic mesh format switching in the virtual geometry pipeline.

**GFX-2f — Conservative rasterization (`VK_EXT_conservative_rasterization`)**

Required for voxelization, SDF generation, and shadow map rendering without sub-texel gaps.

- [x] Detect `VK_EXT_conservative_rasterization`; add `BackendFeatures` overestimate/underestimate capability flags. Underestimate is exposed only when `primitiveUnderestimation` is reported by the device properties.
- [x] Add `GraphicsPipelineDesc::conservative_raster: ConservativeRasterMode` (Off / Overestimate / Underestimate) and chain `VkPipelineRasterizationConservativeStateCreateInfoEXT` for requested modes.
- [x] **Enables**: Track 7c (software rasterizer coverage validation), voxelization passes for GI probes.

**GFX-2g — Global queue priority (`VK_KHR_global_priority`, Vulkan 1.4 core)**

Async compute needs its queue at `MEDIUM` priority. Without this, OS scheduling may starve the compute queue under GPU load.

- [x] Detect `VK_KHR_global_priority`; add `BackendFeatures::global_queue_priority`.
- [x] When requested and enabled, assign `VK_QUEUE_GLOBAL_PRIORITY_HIGH_KHR` to the graphics queue and `VK_QUEUE_GLOBAL_PRIORITY_MEDIUM_KHR` to the async compute queue via `VkDeviceQueueGlobalPriorityCreateInfoKHR` (unified queue families keep graphics `HIGH`; transfer-only families use `LOW`).
- [x] **Enables**: Track 11b (async compute — correct scheduling under GPU contention).

**GFX-2h — Presentation extensions**

Track LL requires several of these. Wire them here so Track LL items do not carry extension detection work.

- [x] Detect `VK_EXT_swapchain_colorspace`; add `BackendFeatures::swapchain_colorspace`. Surface color-space API wiring remains.
- [x] Detect `VK_KHR_present_id`; add `BackendFeatures::present_id`. `VkPresentIdKHR` present chaining remains.
- [x] Detect `VK_KHR_present_wait`; add `BackendFeatures::present_wait`. `Surface::wait_for_present(id, timeout)` remains.
- [x] Detect `VK_KHR_swapchain_maintenance1`; add `BackendFeatures::swapchain_maintenance1`. Runtime present-mode mutation and release fences remain.
- [x] Detect `VK_EXT_full_screen_exclusive`; add `BackendFeatures::full_screen_exclusive`. `SurfaceConfig::exclusive_fullscreen` remains.
- [x] Detect `VK_GOOGLE_display_timing`; add `BackendFeatures::display_timing`. Timing query integration remains.
- [x] Detect `VK_EXT_present_mode_fifo_latest_ready`; add `BackendFeatures::present_mode_fifo_latest_ready`. `PresentMode::FifoLatestReady` API remains.
- [ ] **Enables**: Track LL (low-latency presentation), `BackendFeatures::hdr_output` (pair swapchain colorspace with HDR metadata).

**GFX-2i — HDR metadata (`VK_EXT_hdr_metadata`, detected, not wired)**

`BackendFeatures::hdr_output` is set on supported hardware but `vkSetHdrMetadataEXT` is never called. Engine consumers cannot specify color primaries or luminance.

- [x] Add `Surface::set_hdr_metadata(meta: HdrMetadata)` where `HdrMetadata` carries SMPTE 2086 display primaries, white point xy, mastering max/min luminance, MaxCLL, and MaxFALL.
- [x] Auto-call `vkSetHdrMetadataEXT` when the surface color space is HDR10 (`VK_COLOR_SPACE_HDR10_ST2084_EXT`) and metadata is provided.
- [x] Expose `SurfaceCapabilities::hdr_metadata_supported: bool`. Gate on `features.hdr_output`.
- [ ] **Depends on**: GFX-2h (`VK_EXT_swapchain_colorspace` for HDR10 color space selection). **Enables**: Track Post (HDR display output, `ToneMapConfig::hdr_output`).

**GFX-2j — Performance query and pipeline statistics**

Without hardware counters, profiling data is timestamps only. These expose ROP throughput, cache hit rates, and compiled shader stats.

- [x] Detect `VK_KHR_performance_query`; add `BackendFeatures::performance_query`.
- [x] Expose `Engine::enumerate_performance_counters() -> Vec<PerfCounter>` via `vkEnumeratePhysicalDeviceQueueFamilyPerformanceQueryCountersKHR`; returns index, name, description, category.
- [x] Add `PassDesc::perf_counters: Option<Vec<PerfCounterHandle>>` to record selected counters for specific passes.
- [x] Detect `VK_KHR_pipeline_executable_properties`; add `BackendFeatures::pipeline_executable_properties`.
- [x] Expose `Engine::pipeline_executable_stats(pipeline) -> Vec<ExecutableStat>` for per-stage register usage, code size, and IR via `vkGetPipelineExecutableStatisticsKHR`.
- [x] Detect `VK_AMD_shader_info`; expose AMD-specific compiled shader statistics (`VkShaderStatisticsInfoAMD`) via `Engine::pipeline_shader_stats_amd(pipeline) -> Vec<AmdShaderStageStats>` alongside the KHR path.
- [x] Detect `VK_AMD_shader_core_properties` and `VK_NV_shader_sm_builtins`; expose shader core / SM count via `Engine::shader_core_count() -> Option<u32>` for workgroup size tuning.
- [x] Expose GPU profiling readback through the existing per-pass timing API — `Device::pass_timings()` now returns `Vec<PassTimingReport>` with `name`, `gpu_ms`, and `perf_counters: HashMap<PerfCounterHandle, u64>` fields. Counter recording is wired in the API; backend query-pool recording is a follow-on.

**GFX-2k — Sampler and image quality extensions**

- [x] Detect `VK_EXT_sampler_filter_minmax` (Vulkan 1.2 core); add `BackendFeatures::sampler_filter_minmax`.
- [x] Expose `SamplerDesc::reduction_mode: SamplerReductionMode` (WeightedAverage / Min / Max) and chain `VkSamplerReductionModeCreateInfo` for min/max reduction when `VK_EXT_sampler_filter_minmax` is available. The Max reduction mode is required for Hi-Z pyramid construction (Track 7b, 8b).
- [x] Detect `VK_EXT_custom_border_color`; add `BackendFeatures::custom_border_color` and logical-device feature enablement.
- [x] Expose `SamplerDesc::border_color: BorderColor` with a `Custom([f32; 4])` variant and chain `VkSamplerCustomBorderColorCreateInfoEXT` for custom colors when the feature is enabled.
- [x] Detect `VK_EXT_filter_cubic`; add `BackendFeatures::filter_cubic`.
- [x] Expose `FilterMode::Cubic` through `SamplerDesc::{mag_filter, min_filter}` for Catmull-Rom upscaling and map it to `VK_FILTER_CUBIC_EXT` when `VK_EXT_filter_cubic` is available.
- [x] Detect `VK_EXT_image_view_min_lod`; add `BackendFeatures::image_view_min_lod`.
- [x] Expose `ImageDesc::with_min_lod(f32)` / `min_lod()` for mipmap streaming (clamp visible mips to what is resident); chains `VkImageViewMinLodCreateInfoEXT` when the extension is available.
- [x] Detect `VK_EXT_image_compression_control`; add `BackendFeatures::image_compression_control`.
- [x] Add `ImageDesc::compression: ImageCompression` (Default / Fixed { bits_per_component: u32 } / Disabled). Vulkan image creation now chains `VkImageCompressionControlEXT` for explicit fixed-rate or disabled compression when supported; actual compression property readback remains pending.
- [x] Detect `VK_EXT_image_compression_control_swapchain`; `BackendFeatures::image_compression_control_swapchain` set; `create_swapchain` accepts optional `VkImageCompressionControlEXT` chain for callers that want to set a preference.
- [x] Detect `VK_EXT_multisampled_render_to_single_sampled`; add `BackendFeatures::msaa_render_to_single_sampled`.
- [x] Expose as `ImageDesc::msaa_resolve_to_single_sampled: bool`. On tile-based hardware this eliminates the MSAA allocation entirely — the GPU resolves on-chip.

---

### GFX-3 — Ray tracing pipeline completion

Complete before Track AO (RTAO), Track Refl (RT Reflections), Track GI (RTGI, PTGI), Track Shadow (RT shadows), and Track 7g (RT geometry integration). GFX-1d (buffer device address) is a hard prerequisite.

Current status: the backend foundation is implemented. Vulkan can create acceleration structures, query BLAS/TLAS build sizes, record BLAS/TLAS builds with automatic transient scratch or caller-provided scratch buffers, create ray-tracing pipelines, and record `vkCmdTraceRaysKHR` with caller-provided SBT regions. `BackendFeatures::ray_tracing` is overridden to true only when the logical device enabled the RT pipeline feature. Remaining work in this track is advanced AS/RT features, not the base command-recording substrate.

**GFX-3a — RT command recording foundation — complete**

- [x] Add `AccelerationStructure` resource type to `ResourceRegistry`: backed by `vk::AccelerationStructureKHR` + its own `vk::Buffer` allocation.
- [x] Add `Device::{create_acceleration_structure, destroy_acceleration_structure, acceleration_structure_desc}` and export `AccelerationStructureDesc` / `AccelerationStructureKind`.
- [x] Add BLAS/TLAS build-size queries (`Device::{blas_build_sizes, tlas_build_sizes}`) so callers can allocate AS and scratch storage from driver-reported sizes.
- [x] Add `PassWork::BuildBlas(BlasBuildDesc)` and `PassWork::BuildTlas(TlasBuildDesc)`. The render graph tracks AS build/read states and records Vulkan AS build commands.
- [x] `BlasBuildDesc`: geometry inputs (vertex buffer handle, optional index buffer handle, vertex format, stride, transform buffer), build/update mode, optional caller-provided scratch buffer.
- [x] `TlasBuildDesc`: instance buffer handle (array of `VkAccelerationStructureInstanceKHR`), build/update mode, optional caller-provided scratch buffer.
- [x] Add `PassWork::TraceRays(TraceRaysDesc)` for dispatching ray tracing pipelines.
- [x] `TraceRaysDesc`: ray tracing pipeline handle, `ShaderBindingTable` regions for raygen/miss/hit/callable, dispatch dimensions (width, height, depth).
- [x] Add `RayTracingPipelineDesc`; add `Device::create_ray_tracing_pipeline(RayTracingPipelineDesc) -> PipelineHandle`. `RayTracingPipelineDesc` lists shader stages and shader groups.
- [x] Mark `BackendFeatures::ray_tracing` as true only when the logical device actually enabled `VK_KHR_ray_tracing_pipeline`.
- [x] `ShaderBindingTable` helper: allocate a strided buffer and fill it with the pipeline's shader group handles via `vkGetRayTracingShaderGroupHandlesKHR`. Auto-align to `VkPhysicalDeviceRayTracingPipelinePropertiesKHR` handle/base alignment. `TraceRaysDesc` still accepts caller-provided SBT regions for advanced/manual layouts.
- [x] Per-frame AS scratch allocator: auto-allocate transient scratch buffers from build-size queries when BLAS/TLAS descriptors omit `scratch_buffer`.
- [x] AS compaction command path: `AccelerationStructureBuildMode::Compact` records `vkCmdCopyAccelerationStructureKHR(... COMPACT)` and requires a source AS.
- [x] **Depends on**: GFX-1d (buffer device address). **Enables**: every RT visual feature.

**GFX-3b — Inline ray queries (`VK_KHR_ray_query`) — complete**

Traces rays from any shader stage without a separate RT pipeline. Simpler than full RT pipelines for AO and shadow rays — many RT AO and hard shadow implementations only need this.

- [x] Detect `VK_KHR_ray_query`; add `BackendFeatures::ray_query`.
- [x] Enable `VkPhysicalDeviceRayQueryFeaturesKHR::rayQuery = VK_TRUE` in device creation when requested.
- [x] Add portable `DeviceFeature::RayQuery`.
- [x] No new command recording required — ray queries work via SPIR-V instructions in any existing shader stage. Expose `ShaderDesc::requires_ray_query: bool`; device-side validation rejects shaders that require ray query when the backend did not enable it.
- [x] **Simpler path than GFX-3a** for AO and hard shadows. Implement this first.

**GFX-3c — RT acceleration structure enhancements**

- [x] Detect `VK_KHR_ray_tracing_position_fetch`; add `BackendFeatures::ray_tracing_position_fetch`.
- [x] Detect `VK_KHR_ray_tracing_maintenance1`; add `BackendFeatures::ray_tracing_maintenance1`.
- [x] When `VK_KHR_ray_tracing_position_fetch` is enabled, add `VK_BUILD_ACCELERATION_STRUCTURE_ALLOW_DATA_ACCESS_KHR` to BLAS build flags. Exposes `gl_HitTriangleVertexPositionsEXT` in hit shaders — eliminates the vertex buffer fetch in closest-hit shaders.
- [x] `vkCmdTraceRaysIndirectKHR2` support via `TraceRaysDesc::indirect: Option<(BufferHandle, u64)>`; when set, `VK_KHR_ray_tracing_maintenance1` extension dispatches all SBT regions and dimensions from the GPU buffer.
- [x] Detect `VK_KHR_pipeline_library` (already requested alongside RT in `device.rs`); expose `RayTracingPipelineDesc::use_pipeline_libraries: bool`. Pre-compile hit group shader libraries once per material; link them into per-scene RT pipelines at TLAS build time instead of recompiling on every material combination change. (Flag added; pipeline library linking not yet wired in pipeline creation.)
- [x] Detect `VK_EXT_opacity_micromap`; add `BackendFeatures::opacity_micromap`.
- [x] Expose `MicromapBuildDesc`, `MicromapFormat`, `MicromapAttachDesc` types; `BlasBuildDesc::opacity_micromap: Option<MicromapAttachDesc>` for per-triangle opacity micromap attachment. Command recording pending (requires VkMicromapEXT creation).
- [x] Detect `VK_EXT_ray_tracing_invocation_reorder` (Shader Execution Reordering, SER); add `BackendFeatures::shader_execution_reordering`.
- [x] Expose `ShaderDesc::uses_ser: bool` to enable the `ShaderInvocationReorderNV` SPIR-V capability. SER reorders ray invocations across a warp to reduce divergence — 30–60% throughput uplift on complex scenes with many materials.
- [x] Detect `VK_NV_cluster_acceleration_structure` (NVIDIA Mega Geometry); add `BackendFeatures::cluster_acceleration_structure`.
- [x] Expose cluster-based AS build and traversal: `ClusterAccelerationStructureBuildDesc` public type; `PassWork::BuildClusterAccelerationStructure` render-graph variant; `ash::nv::cluster_acceleration_structure::Device` loader stored in `VulkanBackend`; command recording via `cmd_build_cluster_acceleration_structure_indirect_nv`.

---

### GFX-4 — Video encode/decode

Hardware video is present on most modern GPUs but entirely absent from the engine. Required for in-engine cutscenes, replay recording, streaming, and video texture use cases. This track is independent of GFX-3 and can proceed in parallel with visual quality work.

**GFX-4a — Video decode (H.264, H.265, AV1, VP9)**

- [x] Detect `VK_KHR_video_queue`; add `BackendFeatures::video_queue`.
- [x] Create a `VK_QUEUE_VIDEO_DECODE_BIT_KHR` queue family when available (may overlap with compute or transfer).
- [x] Detect `VK_KHR_video_decode_queue`, `VK_KHR_video_decode_h264`, `VK_KHR_video_decode_h265`, `VK_KHR_video_decode_av1`, `VK_KHR_video_decode_vp9`; add per-codec booleans to `BackendFeatures` (`video_decode_h264`, `video_decode_h265`, `video_decode_av1`, `video_decode_vp9`).
- [x] `Device::create_video_session` creates `VkVideoSessionKHR` via `ash::khr::video_queue::Device`, queries memory requirements, binds memory, supports H.264/H.265 decode and encode. `VkVideoSessionParametersKHR` and DPB image management are pending.
- [x] Add `PassWork::DecodeVideoFrame(DecodeFrameDesc)` to the render graph; Vulkan command recording currently returns `Unsupported`.
- [x] `DecodeFrameDesc`: session handle, compressed bitstream buffer handle, output image handle, output layer.
- [x] Expose `Engine::create_video_decode_session(codec, resolution, profile, max_dpb_slots) -> VideoDecodeSession` with full DPB management.
- [x] `DecodedFrame` images are importable into the render graph as `RgState::ShaderRead` (sampled texture, after YCbCr conversion if needed).
- [x] Detect `VK_KHR_video_maintenance1` and `VK_KHR_video_maintenance2`; enable when present (they simplify session parameter management).

**GFX-4b — Video encode (H.264, H.265, AV1)**

- [x] Detect `VK_KHR_video_encode_queue`, `VK_KHR_video_encode_h264`, `VK_KHR_video_encode_h265`, `VK_KHR_video_encode_av1`; add `BackendFeatures::video_encode_h264`, `video_encode_h265`, `video_encode_av1`.
- [x] Detect `VK_KHR_video_encode_quantization_map`; expose `EncodeFrameDesc::quantization_map: Option<ImageHandle>` for per-coding-block quality control. Vulkan command recording currently returns `Unsupported`.
- [x] Add `VideoEncodeSession` resource: manages `VkVideoSessionKHR` for encode, reference picture management, and rate control state.
- [x] Add `PassWork::EncodeVideoFrame(EncodeFrameDesc)` to the render graph. Input = HDR/SDR image handle; output = compressed bitstream in a CPU-readable buffer.
- [x] Expose `Engine::create_video_encode_session(codec, resolution, config) -> VideoEncodeSession`.
- [x] `VideoEncodeConfig { codec: VideoCodec, width, height, bitrate: BitRateControl, quality: QualityPreset }`. `Default` = H.265, CBR 10 Mbps, medium quality.
- [x] `VideoEncodeSession::read_bitstream() -> Vec<u8>` — CPU readback of the encoded output after the encode frame command completes.

---

### GFX-5 — External resource interop

Required before multi-process GPU memory sharing, camera capture pipelines, CUDA/OpenCL interop, and zero-copy Wayland compositing. Independent of GFX-3 and GFX-4; can proceed in parallel.

**GFX-5a — External memory**

- [x] Detect `VK_KHR_external_memory_fd`, `VK_KHR_external_memory_win32`, `VK_EXT_external_memory_dma_buf`, `VK_EXT_external_memory_host`, `VK_EXT_image_drm_format_modifier`; add `BackendFeatures` fields.
- [x] Expose `Engine::export_buffer_fd` / `export_image_fd` / `create_exportable_buffer` / `create_exportable_image` — `VkExportMemoryAllocateInfo`-backed resources, `vkGetMemoryFdKHR` export.
- [x] Expose `Engine::import_host_memory(ptr, size) -> Result<BufferHandle>` — full `VK_EXT_external_memory_host` implementation with alignment validation via `VkPhysicalDeviceExternalMemoryHostPropertiesEXT`.
- [x] `ImageDesc::drm_format_modifier: Option<u64>` — chains `VkImageDrmFormatModifierExplicitCreateInfoEXT` into image creation when set.

**GFX-5b — External semaphores and fences**

- [x] Detect `VK_KHR_external_semaphore_fd`, `VK_KHR_external_fence_fd`, `VK_KHR_external_semaphore_win32`, `VK_KHR_external_fence_win32`; add `BackendFeatures` fields.
- [x] Expose `Engine::create_exportable_semaphore`, `export_semaphore_fd`, `import_semaphore_fd` — full `VK_KHR_external_semaphore_fd` implementation.
- [x] External fence API: `FenceHandle` type; `Device::create_exportable_fence`, `export_fence_fd`, `import_fence_fd` via `VK_KHR_external_fence_fd`.
- [ ] **Enables**: CUDA interop, OpenGL interop, Android `ANativeWindow` pipeline, V4L2 camera capture with GPU-side YCbCr decode.

---

### GFX-6 — Application-layer GPU capabilities

These do not block rendering quality work. They can proceed in parallel with visual quality tracks after GFX-1 and GFX-2 are complete.

**GFX-6a — Device-generated commands (`VK_EXT_device_generated_commands`)**

GPU-generated command streams with state switches and pipeline binds. Required for fully GPU-driven rendering where the CPU does zero per-draw work.

- [x] Detect `VK_EXT_device_generated_commands` (cross-vendor); add `BackendFeatures::device_generated_commands`. Detect `VK_NV_device_generated_commands` as a fallback for NVIDIA hardware that predates the EXT; add `BackendFeatures::device_generated_commands_nv`.
- [x] Add `IndirectCommandLayout` resource: `Device::create_indirect_command_layout(desc)` creates `VkIndirectCommandsLayoutNV` via the NV extension when available. Token types: Draw, DrawIndexed, Dispatch, IndexBuffer, Pipeline, PushConstant, VertexBuffer.
- [x] Add `PassWork::ExecuteGeneratedCommands(DgcExecuteDesc)` to the render graph. Vulkan command recording uses `vkCmdExecuteGeneratedCommandsNV` with the stored layout handle.
- [x] Add `PassWork::PreprocessGeneratedCommands(DgcPreprocessDesc)` for the mandatory preprocessing pass. Vulkan command recording uses `vkCmdPreprocessGeneratedCommandsNV`.
- [ ] **Depends on**: GFX-1d (buffer device address). **Enables**: fully GPU-driven scene with GPU-side pipeline switching for material batching.

**GFX-6b — Latency reduction (NVIDIA Reflex 2 and AMD Anti-Lag)**

Reduce input-to-display latency by letting the driver control when the engine submits GPU work relative to the display vblank.

- [x] Detect `VK_NV_low_latency2`; add `BackendFeatures::reflex`. Expose `Engine::set_reflex_mode(mode: ReflexMode)` (Off / On / OnPlusBoost).
- [x] Call `vkSetLatencySleepModeNV` on all active swapchains when `set_reflex_mode` is called. Call `vkLatencySleepNV` + `vkWaitSemaphores` via `Surface::latency_sleep()` using a per-backend timeline semaphore.
- [x] Detect `VK_AMD_anti_lag`; add `BackendFeatures::anti_lag`. Expose `Engine::set_anti_lag_mode(mode: AntiLagMode)`.
- [x] Call `vkAntiLagUpdateAMD` each frame via `Engine::anti_lag_frame_start()` using a raw Vulkan entry-point loader when `VK_AMD_anti_lag` is enabled.
- [x] Expose `Engine::latency_mode() -> Option<LatencyMode>` reporting which driver-level latency feature is available. Reflex and Anti-Lag mode switching are wired through the Vulkan backend.
- [ ] **Depends on**: GFX-2h (`VK_KHR_present_id` for frame correlation in Reflex). **Enables**: Track LL `Surface::present_latency_hint()` to give accurate driver-derived latency estimates.

**GFX-6c — Cooperative matrix (`VK_KHR_cooperative_matrix`)**

GEMM-style matrix multiply-accumulate across a subgroup. Required for neural denoising and ML inference directly in shaders.

- [x] Detect `VK_KHR_cooperative_matrix`; add `BackendFeatures::cooperative_matrix`. Detect `VK_NV_cooperative_matrix` as a fallback for older NVIDIA hardware. Detect `VK_NV_cooperative_matrix2` for the extended NV version (type conversions, reductions, tensor addressing).
- [x] Expose `Engine::enumerate_cooperative_matrix_properties() -> Vec<CoopMatrixProperty>` via `vkGetPhysicalDeviceCooperativeMatrixPropertiesKHR`; returns scope, element types (A/B/C/result), M×N×K size, saturation flag.
- [x] No new command recording required — cooperative matrix instructions are SPIR-V extensions. Expose `ShaderDesc::requires_cooperative_matrix: bool` so the pipeline builder enables the required SPIR-V capability.
- [ ] **Enables**: Research Horizons — neural denoising (replaceable kernel slot in the SVGF denoiser pass), ML inference passes for upscaling fallbacks.

**GFX-6d — Advanced shader features**

A collection of shader-stage capabilities that unlock specific rendering algorithms. None require new command recording — they are SPIR-V capabilities gated behind feature detection.

- [x] Detect `VK_KHR_fragment_shader_barycentric`; add `BackendFeatures::fragment_shader_barycentric`. Enables `BaryCoordKHR` and `BaryCoordNoPerspKHR` built-ins for custom interpolation. **Needed by**: Track 7e (visibility buffer material resolve — compute analytic barycentrics from hit triangle data).
- [x] Detect `VK_EXT_fragment_shader_interlock`; add `BackendFeatures::fragment_shader_interlock`. Enables per-pixel, per-sample, or per-shading-rate-tile critical sections between fragment invocations. Required for correct linked-list OIT without depth-sorted draw order.
- [x] Detect `VK_EXT_shader_atomic_float` and `VK_EXT_shader_atomic_float2`; add `BackendFeatures::shader_atomic_float` and `shader_atomic_float16`. Enable for HDR luminance histogram accumulation (`autoexposure` compute pass) and RTXGI reservoir weight writes.
- [x] Detect `VK_KHR_compute_shader_derivatives`; add `BackendFeatures::compute_shader_derivatives`. Enables `dFdx`/`dFdy` in compute shaders for screen-space filtering without a separate full-quad pass.
- [x] Detect `VK_KHR_shader_clock`; add `BackendFeatures::shader_clock`. Exposes `clock2x32ARB()` and `clockRealtimeEXT()` for micro-timing of shader sub-expressions during profiling.
- [x] Detect `VK_EXT_post_depth_coverage`; add `BackendFeatures::post_depth_coverage`. Enables accessing the post-depth-test coverage mask in fragment shaders (required for some MSAA resolve techniques and coverage-based transparency).

**GFX-6e — Optical flow (`VK_NV_optical_flow`)**

Hardware optical flow estimation. Supplement or replace compute-shader optical flow in FSR 3.1 frame generation (Track 10) on NVIDIA hardware.

- [x] Detect `VK_NV_optical_flow`; add `BackendFeatures::optical_flow_nv`.
- [x] Add public `OpticalFlowSessionDesc`, `OpticalFlowEstimateDesc`, and `PassWork::EstimateOpticalFlow`; Vulkan session creation, image binding, and command recording are wired through `VK_NV_optical_flow`.
- [x] Add `OpticalFlowSession` resource: `Device::create_optical_flow_session(desc)` creates `VkOpticalFlowSessionNV` with configurable width, height, and 1×1/2×2/4×4/8×8 output grid via raw fp call.
- [x] `PassWork::EstimateOpticalFlow` is executable in the Vulkan render graph when `VK_NV_optical_flow` is enabled and images were created with optical-flow usage flags.
- [ ] **Enables**: Track 10 (FSR 3 frame generation — provides better motion vectors than the compute optical flow path on NVIDIA).

---

### GFX-7 — Next-gen descriptor model

These require a larger architectural change to the descriptor management layer. Do after GFX-1 through GFX-2 are stable and the bindless heap (Track 8a) is fully exercised.

**GFX-7a — Descriptor buffer (`VK_EXT_descriptor_buffer`)**

Moves descriptors into application-managed buffers, composable with the existing bindless heap. Eliminates descriptor pool management entirely.

- [x] Detect `VK_EXT_descriptor_buffer`; add `BackendFeatures::descriptor_buffer`. Query `VkPhysicalDeviceDescriptorBufferPropertiesEXT` for descriptor offset alignment.
- [x] Implement `DescriptorBufferHeap`: a `Buffer` (created with `DESCRIPTOR_BUFFER_BIT_EXT`) holding all descriptor data addressed by byte offset. When `descriptor_buffer` is available, offer it as an alternative backing for the `DescriptorRegistry`.
- [x] Expose `Device::descriptor_buffer_offset_alignment() -> Option<u64>` from the physical device properties.
- [x] Map `BindGroupDesc` to a `DescriptorBufferHeap` sub-range. Bind groups become CPU-written buffer regions rather than `VkDescriptorSet`s; bind via `cmd_bind_descriptor_buffer_embedded_samplers_ext` + `cmd_set_descriptor_buffer_offsets_ext`.
- [ ] **Depends on**: GFX-1d (buffer device address required for descriptor buffer binding). **Improves**: bindless heap management and per-draw descriptor binding overhead at scale.

**GFX-7b — Descriptor heap (`VK_EXT_descriptor_heap`, Roadmap 2026)**

D3D12-style two-heap model (resource heap + sampler heap). Planned KHR extension; design the bindless heap layout now to avoid future incompatibility.

- [x] Track `VK_EXT_descriptor_heap` availability; add `BackendFeatures::descriptor_heap`.
- [x] Design the existing `BindlessHeap` in `bindless.rs` as a logical two-heap model: sampler descriptors (binding 0) are the "sampler heap"; resource descriptors (bindings 1-3: sampled images, storage images, storage buffers) are the "resource heap". Documented in bindless.rs header.
- [x] `DescriptorHeapBindlessHeap` fully implemented: HOST_VISIBLE buffer with `DESCRIPTOR_HEAP_BIT_EXT | SHADER_DEVICE_ADDRESS`, descriptor sizes queried from driver, resource-heap layout (samplers → sampled images → storage images → storage buffers), `write_sampler_descriptors` / `write_resource_descriptors` called on registration. Heap populated in parallel with the pool heap on every `register_bindless_*` call. Pipelines carry `VK_PIPELINE_CREATE_2_DESCRIPTOR_HEAP_BIT_EXT` when feature is active. `SamplerDesc` stored alongside `VkSampler` in resource registry for create-info reconstruction. **Remaining for full switch**: bindless.slang must adopt `[[vk::heap]]` / SPIR-V `HeapEXT` access decorations, and `cmd_bind_resource_heap` / `cmd_bind_sampler_heap` must replace `vkCmdBindDescriptorSets` in the command recording path once shaders are updated.

**GFX-7c — Work graphs (`VK_AMDX_shader_enqueue`) — already in roadmap**

Already referenced in the "Backend and Platform Maturity" section. Cross-reference: complete GFX-1 through GFX-2 before beginning this.

- [x] `VK_AMDX_shader_enqueue`: add `BackendFeatures::work_graphs`.
- [ ] Port cluster LOD selection (Track 7b) to a Work Graph once GFX-1 and GFX-2 are complete. Work graphs allow a cluster traversal shader to directly enqueue mesh shader workgroups without a CPU-side indirect buffer round-trip.

---

### GFX-8 — Shader object (optional alternative rendering path)

`VK_EXT_shader_object` compiles shaders into standalone `VkShaderEXT` objects that bind individually per draw, bypassing the monolithic `VkPipeline`. Not a prerequisite for anything but valuable for the shader playground product direction.

- [x] Detect `VK_EXT_shader_object`; add `BackendFeatures::shader_object`.
- [x] Expose `Device::create_shader_object(ShaderObjectDesc) -> ShaderObjectHandle` / `destroy_shader_object` API scaffolding.
- [x] Implement Vulkan `VkShaderEXT` creation/destruction behind `create_shader_object`.
- [x] Add `PassDesc::shader_binding: ShaderBinding` enum: `Pipeline(PipelineHandle)` or `ShaderObjects(...)`.
- [x] Wire the command recording path in `commands.rs` so render/dispatch passes select `cmd_bind_shaders_ext` vs `cmd_bind_pipeline` based on the active binding variant. Graphics shader-object passes use `pass.pipeline` as the render-state/fallback anchor while binding shader stages independently.
- [x] The pipeline path remains primary. Shader objects are an opt-in alternative; useful for real-time shader permutation switching and the shader playground (Track 1 product direction).

---

## Visual Quality — Ambient Occlusion (Track AO)

The single cheapest high-impact visual improvement. Without AO, objects look pasted onto surfaces — crevices don't darken, edges don't settle, contact shadows don't exist at sub-shadow-map scale. Every modern game has this.

### Ground Truth Ambient Occlusion (GTAO)

GTAO (Jimenez et al. 2016, extended by Intel 2021 and UE5 Lumen) is the modern standard. It computes multi-bounce bent normals and accurate horizon-based integration — strictly more correct than SSAO at similar cost.

`DeferredPass::set_ao(AoPass::new(&engine)?)`  
`AoConfig::default()` gives production-quality results at < 0.5 ms on a 1080p mid-range GPU.

- [ ] Implement screen-space horizon search: for each pixel, march rays in the view-space hemisphere and find the maximum elevation angle (horizon) in each direction. Integrate visibility using the cosine-weighted hemisphere formula. Produces a `R8Unorm` AO image.
- [ ] Use 8 slices × 8 samples/slice (64 total) as the default. `AoConfig::slice_count` and `AoConfig::steps_per_slice` are tunable.
- [ ] Temporal accumulation: reproject previous frame's AO using the motion vector buffer; blend with current frame (blend factor `AoConfig::temporal_blend`, default 0.1). Reduces noise without increasing per-frame cost.
- [ ] Apply spatial 3×3 bilateral filter weighted by depth and normal similarity to remove remaining temporal noise.
- [ ] Bent normal output: `AoConfig::bent_normals: bool` (default false). When enabled, exports a world-space bent normal used to improve IBL diffuse sampling direction.
- [ ] Bind AO texture as `"gtao_result"` in the lighting pass; deferred lighting multiplies ambient by `ao_value` per pixel.
- [ ] `AoConfig { slice_count: u32 [4,16], steps_per_slice: u32 [4,16], radius_world: f32, falloff_start: f32, falloff_end: f32, intensity: f32, temporal_blend: f32, bent_normals: bool }`. All `pub`, `Default::default()` production-ready.

### RTAO — Ray-Traced Ambient Occlusion (hardware optional)

When hardware RT is available, replace the screen-space horizon search with short-range hemisphere rays from the G-Buffer. Ground truth, no screen-space artifacts.

- [ ] Detect `VK_KHR_ray_tracing_pipeline`; fall back silently to GTAO when unavailable.
- [ ] Trace N short rays per pixel from the G-Buffer surface point (default 1 ray/pixel — temporal accumulation covers the rest). Max distance = `AoConfig::radius_world`.
- [ ] Temporal accumulation and spatial denoise reuse the same passes as GTAO. Users switch between raster and RT AO by changing `AoConfig::mode: AoMode` (GTAO / RTAO / Auto). `Auto` selects RTAO when hardware is available.
- [ ] `AoMode::RTAO` exposes `rays_per_pixel: u32 [1, 4]` as an additional dial.

---

## Visual Quality — Reflections (Track Refl)

Specular IBL gives environment reflections. Scene reflections — a wet floor reflecting the wall, a car window showing the road — require tracing into actual geometry. SSR covers the common case cheaply; RT covers the rest correctly.

### Screen-Space Reflections (SSR)

Uses the depth buffer to ray-march in screen space. Works for surfaces visible to the camera. Fails at edges and silhouettes (falls back to IBL). Cost: ~0.5–1 ms at 1080p.

- [ ] Implement hierarchical depth buffer ray march: trace the reflection ray using the Hi-Z pyramid (4–8 levels) to skip large empty sections. Avoids per-texel marching for most rays.
- [ ] Per-pixel reflection ray: origin = G-Buffer world position, direction = `reflect(-V, N)`. March in view space; terminate on depth hit.
- [ ] Resolve pass: for each hit, sample the previous frame's colour buffer at the hit UV (to avoid feedback loops). Apply Beckmann-roughness-based cone sampling: rougher surfaces sample a wider cone of jittered rays and blur the result.
- [ ] Blend SSR with IBL specular: `ssrBlend = ssrContribution * (1 - roughness²)`. Smooth surfaces get SSR, rough surfaces fall back to IBL.
- [ ] Temporal accumulation of the SSR result with reprojection via the motion vector buffer. Reduces noise from stochastic cone sampling.
- [ ] Expose `SsrConfig { max_steps: u32, thickness: f32, fade_start: f32, fade_end: f32, roughness_cutoff: f32, temporal_blend: f32 }`. `Default` production-quality.

### RT Reflections (hardware optional)

When hardware RT is available, trace reflection rays into the full TLAS rather than the depth buffer. No screen-space limitations. Blends with SSR based on roughness and screen-space availability.

- [ ] Trace one reflection ray per pixel from G-Buffer surface into the TLAS.
- [ ] Evaluate `RtClosestHitVariant` at the hit — same `UnifiedMaterial` shading without re-rasterising geometry.
- [ ] Accumulate with temporal reprojection; apply a joint bilateral denoiser (2-pass spatial, or SVGF when available).
- [ ] Expose `RtReflectionConfig { rays_per_pixel: u32 [1,4], max_roughness: f32, denoise: bool }`.
- [ ] `ReflectionConfig::mode: ReflectionMode` (SSR / RT / Auto). `Auto` selects RT when hardware is available and `roughness < max_roughness`.

---

## Visual Quality — Global Illumination (Track GI)

GI is the largest remaining visual gap. IBL provides sky illumination; GI adds inter-object light bouncing — a red wall bleeding onto the floor, light wrapping around a door frame, shadowed corners filling with diffuse light. Order: SSGI is cheapest, Surfel GI works without RT, RTGI is ground-truth indirect, PTGI is full path tracing.

### SSGI — Screen-Space Global Illumination

Short-range indirect diffuse via screen-space ray marching. Works without RT hardware. Complements GTAO (AO kills the zero-bounce term; SSGI adds the one-bounce term).

- [ ] Ray march the previous-frame colour buffer in screen space from each G-Buffer pixel. Accumulate incident radiance weighted by the cosine of the angle to the hit normal.
- [ ] 8 rays per pixel at 1/4 resolution; temporal accumulation at full resolution.
- [ ] Composite into deferred lighting: `total_diffuse = direct + ssgi_indirect * (1 - ao)`.
- [ ] Expose `SsgiConfig { ray_count: u32, max_distance: f32, thickness: f32, temporal_blend: f32, intensity: f32 }`.

### Surfel GI — Hardware-Agnostic Dynamic GI (Lumen-style)

World-space surfels accumulate incident radiance from RT rays or screen-space probes and inject it as diffuse irradiance into the lighting pass. Works on any GPU; quality scales with RT availability. This is the Lumen-style technique — adaptive surfel placement, amortised over multiple frames.

- [ ] Spawn surfels by projecting each G-Buffer pixel to world space at a configurable spacing. Store per-surfel position, normal, radius, irradiance (RGB SH1 = 4 floats).
- [ ] Each frame, trace `N` hemisphere rays per surfel (budget: 64–256 total across all surfels, amortised over 4–8 frames). Accumulate hit radiance via exponential moving average.
- [ ] Invalidate surfels when geometry near them changes (check velocity buffer for dynamic objects). Re-initialise stale surfels.
- [ ] Evaluate surfel irradiance in the lighting pass: for each pixel, gather nearby surfels (spatial hash query); blend by distance and normal alignment.
- [ ] Expose `SurfelGiConfig { surfel_spacing_cm: f32, rays_per_surfel: u32, history_frames: u32, max_surfels: u32, intensity: f32 }`.

### RTGI — Ray-Traced Global Illumination (one bounce)

Trace one secondary ray per pixel from the G-Buffer; evaluate radiance at the hit point using the deferred lighting result from the previous frame (no recursive tracing needed). Temporal reuse via ReSTIR GI reservoirs for stable 1-spp output.

- [ ] Trace 1 secondary ray per pixel from G-Buffer surface in BRDF-sampled direction.
- [ ] At hit: sample previous frame's HDR colour buffer at the reprojected hit UV (deferred radiance cache). Multiply by hit surface albedo. Add to indirect diffuse.
- [ ] Apply temporal reservoir reuse (ReSTIR GI): merge current-frame reservoir with reprojected previous-frame reservoir; 4–8 spatial taps at neighbours.
- [ ] Apply joint bilateral denoiser (SVGF or equivalent): variance-guided spatial filter to stabilise the 1-spp output.
- [ ] Composite: `total = direct + indirect_diffuse * ao + indirect_specular`.
- [ ] Expose `RtGiConfig { rays_per_pixel: u32 [1,4], reservoir_temporal_frames: u32, spatial_taps: u32, denoise: bool, intensity: f32 }`.

### PTGI — Path-Traced Global Illumination (multi-bounce reference)

Full stochastic path tracing using ReSTIR PT reservoirs. Russian roulette termination. NEE (next-event estimation) for direct lights. Suitable for: offline renders, high-quality screenshots, progressive accumulation in paused frames, and as a reference renderer to validate the raster pipeline.

- [ ] Extend `PathTracedVariant` shader with ReSTIR PT: store full path prefixes (not just final hit) as reservoirs; resample across temporal and spatial neighbours.
- [ ] Reconnection shift: when merging two reservoirs' paths, reconnect through the merge vertex to avoid MIS weight divergence.
- [ ] Hybrid shift mapping: combine random replay and reconnection for visibility-sensitive bounces.
- [ ] Russian roulette termination after bounce 2 (configurable minimum).
- [ ] NEE: at each bounce, directly sample the light list proportional to luminous flux.
- [ ] Two modes: `PtMode::RealTime` (1–4 paths/pixel, denoised, 60 fps target) and `PtMode::Progressive` (accumulates indefinitely, used for screenshots and reference).
- [ ] Expose `PtGiConfig { mode: PtMode, max_bounces: u32, min_rr_depth: u32, spp_realtime: u32, denoise: bool }`.
- [ ] Gate behind `GiFeatures::PTGI`; requires hardware RT.

### GI Feature Flags

```rust
deferred.set_gi(GiConfig {
    mode: GiMode::Auto, // SSGI → SurfelGI → RTGI → PTGI based on hardware
    ssgi:   SsgiConfig   { .. Default::default() },
    surfel: SurfelGiConfig { .. Default::default() },
    rtgi:   RtGiConfig   { .. Default::default() },
    ptgi:   PtGiConfig   { .. Default::default() },
});
```

`GiMode::Auto` selects the highest-quality mode available. `GiMode::SSGI` / `GiMode::Surfel` / `GiMode::RTGI` / `GiMode::PTGI` force a specific mode.

---

## Visual Quality — Shadows (Track Shadow)

Current: CSM (4 cascades, PCF or PCSS) ✓, spot PCF ✓, point dual-paraboloid ✓. Remaining:

- [ ] **Virtual Shadow Maps (VSM)**: page-based virtual atlas (`R32Float`, 16K×16K logical, 128×128 resident pages). Only render pages visible to the camera and dirty pages. Supports 16+ simultaneous shadow sources without atlas defragmentation stalls. Gate behind `ShadowTechnique::Virtual`; fall back to CSM.
- [ ] **RT shadows**: hardware RT shadow rays replacing PCF for the primary directional light. `ShadowConfig::rt_shadows: RtShadowMode` (Off / DirectionalOnly / All); graceful fallback to CSM.
- [ ] **Denoising slot**: RT shadow output routes through a replaceable denoiser pass (`ShadowDenoiser::Svgf` initially; `ShadowDenoiser::Neural` when Track LL — Research Horizons matures).
- [ ] **Point light shadow atlas**: consolidate the 8 dual-paraboloid maps (4 lights × front/back) into a single atlas to reduce texture binding count.

---

## Visual Quality — Volumetric Fog and Light Shafts (Track Fog)

Volumetric effects add atmospheric depth — the sense that there is air between objects. God rays through windows, haze in a forest, underwater scatter. Essential for outdoor and dramatic scenes.

### Froxel Volume (main path)

The standard modern approach: a 3D view-frustum-aligned voxel grid ("froxels") sampled for density and in-scattering. Used in Frostbite, Unreal, and Unity HDRP.

- [ ] Allocate a `RGBA16Float` froxel volume texture (160×90×64 voxels by default; configurable). UV = screen XY, W = linearised depth. Each froxel stores scattered radiance + transmittance.
- [ ] Froxel density injection compute pass: write density values per froxel from:
  - **Homogeneous fog**: constant `base_density` + exponential height falloff `exp(-height * falloff)`.
  - **Heterogeneous turbulence**: sample a tileable 3D Worley/Perlin noise texture; modulate density by `noise(pos * frequency + time * scroll_speed)`. Exposes `VolumetricConfig::turbulence_strength`, `turbulence_frequency`, and `turbulence_scroll: Vec3` for wind direction.
  - **Local fog volumes**: axis-aligned or spherical volumes with local density override. `FogVolume { bounds, density, color, absorption }` added to the scene list. Injected as a sphere/box SDF contribution per froxel.
- [ ] In-scattering evaluation compute pass: for each froxel, sample lighting contributions:
  - Directional light: sample CSM shadow map at froxel world position; modulate by `phase(cos_theta, g)` (Henyey-Greenstein phase function with anisotropy `g`).
  - Point/spot/area lights: BVH traversal (same as deferred lighting) restricted to froxels within each light's range.
- [ ] Temporal accumulation: reproject previous frame's froxel volume using camera motion; blend with current frame (suppresses temporal shimmer in turbulence).
- [ ] Ray-march integration pass (full-screen): front-to-back accumulate transmittance and in-scattering along the view ray; terminate when transmittance < 0.001. Output: `RGBA16Float` fog colour + transmittance.
- [ ] Composite into deferred lighting after the main pass: `final_colour = scene_colour * fog.a + fog.rgb`.
- [ ] Expose `VolumetricConfig { enabled: bool, froxel_resolution: [u32;3], base_density: f32, height_falloff: f32, base_height: f32, scattering_color: [f32;3], absorption_color: [f32;3], anisotropy_g: f32, turbulence_strength: f32, turbulence_frequency: f32, turbulence_scroll: [f32;3], max_distance: f32 }`. `Default` gives a light outdoor haze.

### Participating Media Volumes

- [ ] `FogVolume` scene primitive: axis-aligned box or sphere with local density override, scattering colour, and absorption coefficient. Injected into the froxel grid per-froxel.
- [ ] Support up to 16 simultaneous `FogVolume` primitives; evaluated in the injection pass alongside global density.

---

## Visual Quality — Motion Blur (Track MB)

Camera and per-object motion blur from motion vectors already computed by TAA. The goal is cinematic accuracy — matching how a physical camera with a given shutter angle captures motion.

### Physically Accurate Motion Blur

Based on McGuire et al. 2012 "A Reconstruction Filter for Plausible Motion Blur" (scatter-as-gather), improved by Guertin et al. 2014. This approach is correct for both camera motion and independently moving objects, avoids the "bright edge" artefact of naive gather, and is used in shipping AAA titles.

- [ ] **Velocity tile pass**: downsample the motion vector buffer to tiles (40×40 pixels). Per tile, write the maximum velocity magnitude (max across all pixels in the tile). Dilate the tile max by a 3×3 neighbourhood to bleed fast-moving edges into adjacent tiles.
- [ ] **Reconstruction filter**: for each pixel, use the tile maximum velocity as the sample scatter radius. Gather N samples along the blur direction (default N=15) using a jittered Poisson pattern along the velocity vector. Each sample contributes based on: (a) whether its velocity magnitude is compatible with the centre pixel's (to prevent background leaking into foreground), and (b) a soft depth test (allow blurry foreground to bleed over sharp background — the correct physical behaviour). Accumulate weighted samples.
- [ ] **Per-object blur**: separate pass renders moving objects' velocity vectors to the motion vector buffer with object-space velocity. This produces correct per-object blur independent of camera motion.
- [ ] **Shutter angle control**: `MotionBlurConfig::shutter_angle_degrees: f32` (default 180° — standard film). 360° = maximum blur (open shutter for full frame period). The blur length = `pixel_velocity * (shutter_angle / 360)`.
- [ ] **Temporal integration mode** (highest quality, optional): accumulate jittered sub-frame samples across 3–5 frames using reprojection. Each frame uses a different jitter pattern. Produces near-ground-truth blur with 0 additional samples per frame. Gate behind `MotionBlurConfig::temporal: bool`.
- [ ] Expose `MotionBlurConfig { enabled: bool, shutter_angle_degrees: f32 [0,360], sample_count: u32 [8,32], max_blur_radius_pixels: f32, temporal: bool }`. `Default` = enabled, 180° shutter, 15 samples.

---

## Visual Quality — Depth of Field (Track DoF)

The user's priority: **physically correct lens simulation with proper foreground bleeding into background**. In real optics, a blurry foreground object's bokeh extends over the sharp background — naive gather-based DoF gets this wrong. The correct approach combines Pixar's physically based lens model with a scatter-as-gather composite.

### Physical Lens Model

Based on: Kolb, Mitchell, Hanrahan (1995) "A Realistic Camera Model for Computer Graphics"; Cook et al. (1984) "Distributed Ray Tracing"; Pixar's RenderMan camera documentation; and more recent work from Jimenez et al. on real-time bokeh.

- [ ] **Circle of Confusion (CoC)** computed per pixel from:
  ```
  sensor_width = 36mm (full-frame default)  
  coc_mm = abs(focal_length² / (f_stop * (focus_distance - focal_length))
               * (depth - focus_distance) / depth)
  coc_pixels = coc_mm / sensor_width * render_width
  ```
  All physical — f-stop, focal length (mm), focus distance (m), and sensor size (mm) are the user-facing dials. No abstract "blur strength" sliders.
- [ ] **Aperture shape (bokeh)**: sample the aperture as a polygon with `blade_count` blades (default 6). Blade rotation: `blade_rotation_degrees`. Blade curvature: `blade_curvature ∈ [-1, 1]` where -1 = concave inward (cat-eye style), 0 = flat (regular hexagon), +1 = convex outward (rounded, lens-quality). Generate the aperture mask once and store as a prefiltered texture.
- [ ] **Cat-eye vignetting**: at the sensor periphery, the aperture appears as an ellipse rotated toward screen centre (cats-eye effect). Scale and rotate the bokeh kernel per pixel based on screen-space distance from centre.

### Scatter-as-Gather Composite (correct foreground bleed)

- [ ] Classify pixels by depth relative to focus: **near field** (in front of focal plane) → foreground, **far field** (behind focal plane) → background. CoC is positive for far, negative for near.
- [ ] **Separate near and far bokeh passes**: run the blur kernel twice — once gathering near-field pixels (using their CoC as the radius), once for far-field. Using separate passes prevents the two from mixing at the focal plane.
- [ ] **Foreground bleed**: the near-field pass uses a **max-CoC gather** — each pixel samples within the maximum CoC of any pixel in the tile. This is the scatter-as-gather trick: blurry foreground pixels "scatter" their bokeh into the sharp region behind them by contributing to the gather of nearby background pixels. The contribution weight is `smoothstep(0, 1, near_coc / sample_distance)`.
- [ ] **Composite order**: (1) render sharp layer; (2) composite far bokeh behind sharp; (3) composite near bokeh over everything (near-field always occludes). Alpha channel encodes coverage to handle partially blurred edges correctly.
- [ ] **Chromatic aberration**: the CoC varies slightly by wavelength (real lenses). Apply: split the gather into R/G/B channels with per-channel CoC scaled by `aberration_strength` (red slightly wider, blue slightly narrower). Default: off. `DoFConfig::chromatic_aberration: f32 [0, 1]`.
- [ ] Expose `DoFConfig { enabled: bool, focal_length_mm: f32, f_stop: f32, focus_distance_m: f32, sensor_width_mm: f32, blade_count: u32 [3,12], blade_rotation_deg: f32, blade_curvature: f32 [-1,1], chromatic_aberration: f32 [0,1], max_coc_pixels: f32, sample_count: u32 [16,64] }`.
  - `DoFConfig::from_focal_distance(focal_length_mm, f_stop, focus_distance_m)` — physical constructor.
  - `DoFConfig::portrait()` — f/1.8, 85mm, shallow DoF.
  - `DoFConfig::cinematic()` — f/2.8, 32mm, wide cinematic look.
  - `DoFConfig::default()` = disabled.

---

## Visual Quality — Post-Processing Stack (Track Post)

The current post stack (bloom → TAA → tonemap) is a fixed pipeline. It needs to become an ordered, composable stack with each effect tunable independently.

### Tone Mapping and Exposure

- [ ] **Auto-exposure (eye adaptation)**: compute a luminance histogram over the HDR frame using a compute shader (256 bins, log2 scale). Derive EV100 from the histogram's 50th percentile (configurable). Smoothly interpolate toward target exposure with configurable adaptation speed. Expose `AutoExposureConfig { enabled: bool, min_ev: f32, max_ev: f32, target_percentile: f32, adaptation_speed: f32 }`.
- [ ] **Tone mapping operator selection**: expose `ToneMapConfig::operator: ToneMapOp` with options:
  - `AgX` (default) — filmic, wide gamut, no colour channel clipping, designed by Troy Sobotka; the current standard in Blender and modern pipelines.
  - `ACES` (Academy Colour Encoding System) — industry standard for film-to-display; strong shoulder roll-off.
  - `Filmic` (Uncharted 2 / Hejl) — popular game approximation.
  - `Reinhard` — classic, simple, physically motivated.
  - `None` — linear passthrough for HDR display output.
- [ ] **HDR display output**: when a HDR display is available (`BackendFeatures::hdr_output`), bypass tonemapping and output ST.2084 (PQ) encoded HDR. Expose `ToneMapConfig::hdr_output: bool` (auto when display supports it).

### Film Grain, Lens Effects

- [ ] **Film grain**: additive noise in the temporal domain. Grain pattern changes every frame (animated noise); apply at display resolution after upscaling. `GrainConfig { intensity: f32, size: f32, colored: bool }`.
- [ ] **Chromatic aberration**: radial RGB channel fringing toward screen edges. Separate pass or integrated into tonemap. `CaConfig { strength: f32, radial_falloff: f32 }`. Off by default.
- [ ] **Vignette**: soft darkening toward screen corners. `VignetteConfig { intensity: f32, radius: f32, feather: f32 }`.
- [ ] **Lens dirt/flare** (optional): a texture mask applied to the bloom result to simulate lens imperfections. `LensConfig { dirt_texture: Option<TextureHandle>, dirt_strength: f32, flare_enabled: bool }`.
- [ ] Generalise the post stack as an ordered `Vec<PostEffect>` where effects can be reordered, enabled/disabled, or parameterised at runtime. Execution order: GTAO → GI → Fog → Motion Blur → Bloom → DoF → Upscale → Tone Map → Grain → Vignette → CA → Output.

---

## Virtual Geometry System (Track 7)

A complete Nanite-equivalent: triangle count becomes irrelevant. The GPU selects the finest cluster LOD that keeps screen error below a threshold, software-rasterizes micro-triangles, and defers shading entirely to screen space. Geometric complexity scales to billions of triangles with constant shading cost.

### Architecture overview

```
VirtualMesh (authored once, streamed in pages)
  ├── Meshlets (64–128 verts, 64–128 tris each)
  ├── MeshletGroup DAG  ← continuous LOD, crack-free at group boundaries
  ├── ClusterPage[]     ← streaming unit; each page = N meshlets
  └── RT proxy          ← coarser mesh for ray tracing fallback

Per-frame GPU pipeline:
  Instance cull    → reject instances outside frustum + Hi-Z
  Cluster select   → walk DAG per cluster, find the active LOD cut
       ↓
  Hardware raster  → mesh shader path for clusters ≥ ~32 on-screen tris
  Software raster  → compute shader for micro-clusters (< ~32 on-screen tris)
       ↓
  Visibility buffer → R64Uint: (instance_id_32 | cluster_id_22 | tri_id_10)
       ↓
  Material resolve → compute: decode → interpolate barycentrics → eval UnifiedMaterial
       ↓
  Deferred lighting (unchanged)
```

### 7a — Cluster hierarchy DAG

The DAG is the core data structure. Without it you get per-mesh LOD (like HLOD) instead of per-region continuous LOD (like Nanite).

- [ ] **Group partitioning**: after meshlet generation, partition meshlets into groups of 4–8 using a graph-cut algorithm that respects shared boundary edges. Groups that share edges must not simplify independently — this prevents cracks at LOD transitions.
- [ ] **Parent group construction**: simplify each group's boundary edges by 50%, merge simplified groups into parent groups. Repeat up the hierarchy until the whole mesh is one group. Record the simplification error at each level as a `screen_error: f32` (projected size in pixels at which this level becomes indistinguishable from the next).
- [ ] **Error propagation**: a cluster's `cluster_error` is `max(own_simplification_error, max(children_cluster_errors))`. This guarantees that if a cluster's error is below the screen threshold, all its children would also have been below threshold — enabling the "active cut" invariant.
- [ ] **`MeshletGroup`** (already defined): extend to carry `parent_group_id: Option<u32>`, `parent_error: f32`, `cluster_error: f32`, `lod_bounds: BoundingSphere`. The active-cut invariant: render cluster C iff `C.cluster_error < threshold` AND (`C.parent_error >= threshold` OR C has no parent).
- [ ] **GPU layout**: pack `MeshletGroup` data into `ClusterPage` with a fixed page size (e.g., 128 KiB). The DAG links use page-relative indices to enable streaming. Export a `DagIndex` buffer (per-cluster: `[page_id, slot_in_page, parent_group_offset]`) that the selection shader can walk.

### 7b — GPU cluster selection

Runs on compute before any rasterization. The output is two compact lists: one for hardware rasterization, one for software rasterization.

- [ ] **Instance cull pass**: for each scene instance, project the root bounding sphere. Reject instances outside the view frustum or fully occluded by the previous frame's Hi-Z pyramid. Surviving instances emit a work item per cluster page that needs evaluation.
- [ ] **Cluster selection compute** (`cluster_select.slang`): for each cluster, load its `cluster_error` and `parent_error`. Project the cluster's `lod_bounds` sphere to screen pixels: `screen_error = cluster_error * K / max(view_dist, 1)` where `K` is a pixel-threshold constant. Apply the active-cut test: emit the cluster iff `screen_error < pixel_threshold` AND (`parent_screen_error >= pixel_threshold` OR is root). Write surviving cluster indices to a `VisibleClusters` buffer.
- [ ] **Bin into hardware vs software lists**: each surviving cluster checks its projected triangle count. Clusters where `max_tri_screen_area < 1px²` (micro-triangles) go to the software list; others go to the hardware list. This split is the core performance insight: hardware rasterization is wasteful for sub-pixel triangles.
- [ ] **`HizPass`**: compute shader building a mip pyramid from the previous frame's depth buffer (log2 resolution chain). The cluster selection reads this for occlusion rejection: if a cluster's bounding sphere is fully behind the Hi-Z pyramid, discard it without any triangle work.
- [ ] Expose `VirtualGeometryConfig { pixel_error_threshold: f32 [0.5, 4.0], software_raster_threshold_px2: f32, hiz_enabled: bool }`. `Default` = 1.0px error, 1 px² software threshold, Hi-Z on.

### 7c — Software rasterization

Hardware rasterization has per-triangle setup overhead (~50 clocks). For a 4×4 pixel cluster, the rasterizer spends more time on setup than on shading. A compute-shader rasterizer amortises this over a whole workgroup of triangles.

- [ ] **Visibility buffer format**: `R64Uint` per pixel encoding `(instance_id[31:10] | cluster_id[9:0])` in the upper 32 bits, `triangle_id[31:0]` in the lower. If `R64Uint` is unavailable, fall back to two `R32Uint` images.
- [ ] **Software rasterizer compute** (`software_raster.slang`): one workgroup per micro-cluster. Each thread handles one triangle. Compute clip-space positions for all 3 vertices; clip to the tile; iterate pixels in the bounding box; compute barycentric coordinates; depth-test via `InterlockedMax` on a `R32Uint` depth-as-uint buffer (float-sortable when MSB is 0). On depth pass, write the encoded cluster+triangle ID to the visibility buffer using `InterlockedExchange` (64-bit on supporting hardware, or two 32-bit atomics).
- [ ] **Edge cases**: sub-pixel triangles (area < 0.5 px²) are discarded — the parent cluster (at lower LOD) already covers the area. Back-face culling in the vertex transform step before rasterization begins.
- [ ] **Two-pass architecture**: dispatch the hardware list through the mesh shader (Task → Mesh → Fragment writing to visibility buffer). Dispatch the software list through the software rasterizer compute. Both write to the same visibility buffer. Depth-testing ensures correctness when both see the same pixel.

### 7d — Hardware rasterization via mesh shaders

- [ ] `MeshShaderPipelineDesc`: task_shader + mesh_shader + fragment_shader; no vertex input layout.
- [ ] Vulkan pipeline creation: require `VK_EXT_mesh_shader`.
- [ ] **Task shader** (`task_cull.slang`): one workgroup per cluster in the hardware list. Reads cluster bounds; applies frustum + backface-cone + Hi-Z tests; emits one mesh workgroup per surviving cluster via `EmitMeshTasksEXT`.
- [ ] **Mesh shader** (`mesh_emit.slang`): reads `meshlet_vertices` and `meshlet_triangles` from the cluster page; decompresses local indices; writes `gl_Position`, normal, UV, and the encoded `cluster_id`/`instance_id` as flat interpolated outputs; emits the triangle list.
- [ ] **Fragment shader**: writes `(instance_id | cluster_id)` and `triangle_id` to the visibility buffer — no material evaluation yet.
- [ ] Fallback: when `VK_EXT_mesh_shader` is unavailable, fall back to `ClassicVertex` + `ComputeIndirect` path (pre-transformed index buffer per surviving cluster).

### 7e — Visibility buffer and material resolve

Decouples geometry cost from shading cost. Every pixel evaluates exactly one `UnifiedMaterial` regardless of overdraw.

- [ ] Render all opaque geometry (hardware + software paths) into the visibility buffer.
- [ ] **Material resolve compute** (`material_resolve.slang`): for each screen pixel, decode `(instance_id, cluster_id, triangle_id)`. Fetch the three vertex indices for that triangle from the cluster page. Fetch world positions and compute barycentrics analytically (use the derivative method: `ddx`/`ddy` on the encoded IDs to find adjacent triangles). Reconstruct UVs, normals, and tangents. Evaluate the mesh's `UnifiedMaterial` snippet. Write G0/G1/G2 (same layout as the raster G-Buffer fill). This produces an identical G-Buffer regardless of whether the geometry came from hardware or software rasterization.
- [ ] **Gradient computation without texture derivatives**: since barycentrics are computed analytically rather than interpolated by the rasterizer, texture derivatives (`ddx`/`ddy`) are not available. Compute them from adjacent pixel visibility — sample the vis buffer at `(x±1, y±1)`, decode barycentrics for those pixels, compute UV differences. This is correct for all but the sharpest silhouette pixels (which use a fallback).
- [ ] `RenderPath::VirtualGeometry` alongside `DeferredThenForward` and `ForwardOnly`. Automatically selects hardware or software rasterization per cluster.

### 7f — Streaming

- [ ] **`ClusterPage`**: fixed 128 KiB pages. Each page holds a self-contained set of meshlets (vertices, triangles, bounds). Pages are the streaming unit — resident or not. A page index table maps `(mesh_id, page_id)` → GPU address (null if not resident).
- [ ] **Page request generation**: after cluster selection, walk the surviving cluster list and check each cluster's page index. Clusters in non-resident pages cannot be rendered. Emit page load requests to the streaming system (one atomic-append per missing page).
- [ ] **Fallback rendering**: when a page is not resident, use the parent cluster (which is at lower LOD and may be in a resident page). This means a missing high-detail page gracefully falls back to the next LOD level — no missing geometry, just lower detail.
- [ ] **Page eviction**: LRU eviction when VRAM budget (`Engine::memory_budget().over_budget()`) is exceeded. Pages not seen in the last N frames are evicted first.
- [ ] **`VirtualGeometryStats`**: drawn clusters/triangles, software-rasterized clusters/triangles, culled clusters (frustum / Hi-Z / LOD), page requests/frame, resident pages, evicted pages, streaming latency.

### 7g — Ray tracing integration

- [ ] `TlasBuilder`: TLAS from all `VirtualMesh` RT proxies; refit on transform change, rebuild on add/remove.
- [ ] `BlasBuildPass`: build or refit BLAS from `VirtualMesh::rt_proxy` (coarser simplified mesh — correct for RT AO, shadows, reflections; not suitable for primary visibility).
- [ ] `GeometryBackend::RayTracingSelectedClusters`: build a per-frame BLAS from the current frame's software-rasterized cluster subset for high-quality near-camera RT.

---

## ECS Multithreading and Thread-Safe Engine (Track ECS-MT)

This track is the implementation of Law 2. Every item here is required, not optional.

### Design invariants

1. **Safety without annotation**: if two systems provably have disjoint write sets (and neither reads what the other writes), the scheduler runs them in parallel. Checked at schedule-build time.
2. **Opt-in access declaration**: existing `FnMut(&mut World)` systems continue to work unchanged — conservatively treated as "writes everything," run serially. New `ParallelSystem` systems declare their access and gain automatic parallelism.
3. **Engine is a free-threaded shared reference**: `Engine` is `Arc<EngineInner>`, `Send + Sync + 'static`. `Engine::global()` is always available. No method on `Engine` requires being on a specific thread.
4. **No thread affinity anywhere**: the render thread owns queue submission (hardware constraint) but nothing else. Scene updates, asset loads, ECS queries, resource creation, PSO compilation — all are available on all threads.
5. **The engine handles synchronisation, not the caller**: internal locks, atomics, and queues are the engine's problem. The user calls the API; the engine serialises where required.

### ECS-MT-a — Access declaration and dependency graph

- [x] **`SystemAccess` struct**: `reads: TypeIdSet`, `writes: TypeIdSet`, `resources_read: TypeIdSet`, `resources_written: TypeIdSet`. A system that declares no access is treated as "all" (safe but serial).
- [x] **`ParallelSystem` trait**:
  ```rust
  pub trait ParallelSystem: Send + Sync + 'static {
      fn access() -> SystemAccess where Self: Sized;
      fn run(&mut self, world: &WorldView<'_>);
  }
  ```
  `WorldView` provides read access to declared-read components and exclusive access to declared-write components, enforced by borrow-check at runtime (debug) and TypeId-aliasing safety (release).
- [x] **Dependency graph construction** at `Schedule::build()`: for each pair of systems A, B: add a directed edge A → B if A writes what B reads, or A reads what B writes, or both write the same component. Systems with no path between them form independent nodes that can run in parallel.
- [x] **`Schedule::build() -> CompiledSchedule`**: materialises the dependency graph into execution waves. Each wave is a set of systems with no dependencies on each other — all systems in a wave run concurrently. Waves execute in sequence.
- [x] `Schedule::add_parallel_system(name, impl ParallelSystem)` alongside the existing `add_system`. Adding a parallel system after a wave boundary causes a new wave to start. Explicit ordering: `add_system_after("name_a", "name_b")`.

### ECS-MT-b — Parallel world access

The `WorldView` type provides safe concurrent access to component storages without requiring a `&mut World`.

- [x] **Component storage locking**: `components: HashMap<TypeId, UnsafeCell<Box<dyn ComponentVec>>>`. `UnsafeCell` is used instead of `Arc<RwLock>` — the scheduler statically proves disjoint write sets before each wave, so locking overhead is eliminated while preserving all safety invariants.
- [x] **`WorldView<'_>`**: a non-exclusive borrow of `World` that grants:
  - `view.read::<C>() -> ComponentReadGuard<C>` — shared access to `ComponentStorage<C>`.
  - `view.write::<C>() -> ComponentWriteGuard<C>` — exclusive access to `ComponentStorage<C>`.
  - `view.read_par::<C>(f)` / `view.write_par::<C>(f)` — parallel iteration via `rayon::par_iter`.
  - `view.resource::<R>()` — immutable resource access (see ECS-MT-c).
- [x] **Parallel query API**: `view.read_par::<Transform>(|entity, tf| { ... })` / `view.write_par::<Transform>(...)` use `rayon::par_iter` internally, splitting the dense component array across threads.
- [x] **`world.spawn` / `despawn`**: structural mutations deferred to `WorldCommands`; the queue is flushed between schedule waves on the main thread (see ECS-MT-f).

### ECS-MT-c — Resource system ✓ (serial path complete)

"Resources" are singleton values in the World (not component data) — the engine reference, the time step, the event queues.

- [x] **`World::insert_resource<R: Send + Sync + 'static>(value: R)`**: stores under `TypeId`; replaces any existing value of the same type.
- [x] **`World::resource<R>() -> Option<&R>`** / **`resource_mut<R>() -> Option<&mut R>`** / **`remove_resource<R>() -> Option<R>`** / **`has_resource<R>() -> bool`**: full serial access API. 7 tests covering all paths.
- [x] **`World::resource_unwrap<R>()`** / **`resource_unwrap_mut<R>()`**: panicking variants with type name in the message.
- [x] **`WorldView::resource::<R>()`**: thread-safe immutable resource access from parallel systems. `resource_mut` requires interior mutability in the resource type and is left to the caller (parallel mutable access to a single resource is a design question, not an engine gap).
- [ ] The engine auto-inserted as a resource at startup: `world.insert_resource(Engine::global().clone())`. Systems access via `view.resource_unwrap::<Engine>()` — no argument threading.

### ECS-MT-d — Full engine thread safety

**Requirement**: every public API in the engine — resource creation, scene mutation, asset loading, shader compilation, draw call recording — must be callable from any thread at any time without external synchronisation. "When we can" acknowledges one hard constraint: GPU queue submission must be serialized per Vulkan queue. Everything else is parallelizable.

The architecture has three layers:

```
Worker threads (any number, any time)
  ├── Engine::global()           → create buffers, images, shaders; load assets
  ├── Scene::update_transform()  → lock-free per-object atomic write
  ├── ThreadRenderContext        → record secondary command buffers in parallel
  └── WorldView                  → read/write disjoint ECS components

Render thread (one, owns the frame)
  ├── collect secondary CBs from workers
  ├── execute vkCmdExecuteCommands
  └── vkQueueSubmit (serialized per queue — Vulkan spec requirement)
```

**ECS-MT-d-1: Engine global accessor and Arc architecture** ✓

- [x] `Engine: Clone + Send + Sync + 'static`. Compile-time assertion verifies this at build time.
- [x] Global accessor via `static GLOBAL_ENGINE: OnceLock<Engine>`. Set automatically in all shell entry points (`run_game`, `run_headless`, `try_run`, `render_to_rgba8`) before any application code runs. Never set manually.
  ```rust
  let engine = Engine::global();       // &'static Engine — zero cost, panics if unset
  let engine = Engine::try_global();   // Option<&'static Engine>
  ```
- [ ] All `Engine` methods take `&self` only. Any that currently take `&mut self` are refactored to use interior mutability.

**ECS-MT-d-2: Fine-grained resource registry locking**

The current `Mutex<DeviceInner>` serialises all resource creation through a single lock. Splitting by resource type allows, e.g., image creation and buffer writes to proceed simultaneously on different threads.

- [ ] Replace `Mutex<DeviceInner>` with independent per-subsystem locks:
  - `resources: RwLock<ResourceRegistry>` — already in place; image/buffer create/destroy take a write lock; GPU buffer writes take a read lock (they only mutate mapped memory, not the registry).
  - `shaders: Mutex<ShaderRegistry>` — already in place.
  - `pipelines: Mutex<PipelineRegistry>` — already in place.
  - `descriptors: RwLock<DescriptorRegistry>` — already in place.
  - `handle_allocators: Mutex<HandleAllocators>` — split handle allocation from resource construction so the handle lock is held for nanoseconds, not for the full GPU object creation time.
- [ ] `Engine::create_buffer()` / `create_image()`: acquire the write lock only long enough to call `vkCreateBuffer` / `vkCreateImage` and insert into the registry. GPU memory binding happens while holding the write lock (Vulkan `vkBindBufferMemory` is internally synchronized per spec when using distinct `VkDeviceMemory` objects). The allocator uses its own `Mutex` and is not held during registry operations.
- [ ] `Engine::write_buffer()`: acquires only a **read** lock on the registry (looks up the mapped pointer, does a `memcpy`). Multiple threads can write to different buffers simultaneously.

**ECS-MT-d-3: Parallel command buffer recording**

Vulkan's threading model: `vkCmd*` calls on a `VkCommandBuffer` require external synchronisation, but different command buffers from different pools are completely independent. This is the foundation for parallel recording.

- [ ] **Thread-local command pools**: each worker thread owns a `vk::CommandPool` created with `VK_COMMAND_POOL_CREATE_RESET_COMMAND_BUFFER_BIT`. Thread-local storage: `thread_local! { static CMD_POOL: RefCell<ThreadCommandPool> }`. Pools are created lazily on first use and destroyed when the thread exits.
- [ ] **`ThreadRenderContext`**: a per-thread handle handed out by the render thread at frame start. Contains a secondary command buffer from the thread's local pool. Exposes the same API as `RenderFrame` (bind images/buffers, draw meshes, dispatch compute) but records into a secondary CB.
  ```rust
  // Render thread: distribute contexts to workers.
  let contexts = frame.parallel_contexts(num_workers);
  rayon::scope(|s| {
      for (ctx, work) in contexts.iter_mut().zip(work_items) {
          s.spawn(move |_| work.record(ctx)); // runs on worker threads
      }
  });
  // Render thread: collect and execute all secondary CBs.
  frame.execute_parallel_contexts(contexts);
  ```
- [ ] **Secondary command buffer inheritance**: secondary CBs inherit the current render pass and framebuffer from the primary CB via `VkCommandBufferInheritanceInfo`. This means the render pass must be started on the primary CB before distributing `ThreadRenderContext`s, and the secondary CBs record inside it.
- [ ] **Thread-safe binding table**: `ThreadRenderContext::bind_image()` / `bind_buffer()` writes to a per-context local binding table (not shared). At `execute_parallel_contexts()` time, the render thread merges binding tables before submitting. No locking needed during recording.
- [ ] **`RenderFrame` remains the render-thread owner** of the primary CB and queue submission. `ThreadRenderContext` cannot call `vkQueueSubmit`. This enforces the Vulkan serialisation requirement without preventing any parallel recording.

**ECS-MT-d-4: Thread-safe Scene mutations**

The `Scene` struct currently uses `&mut self` for everything. The goal: transform updates (the hot path) are lock-free; structural changes (add/remove mesh, add/remove object) are queued and applied at frame-start.

- [ ] **Lock-free transform updates**: replace the transform `Vec<Mat4>` in `Scene` with `Vec<AtomicCell<Mat4>>` (using `atomic` crate or a `[AtomicU64; 8]` encoding for the 8 f32 fields). `Scene::set_transform(id, mat)` becomes a non-blocking atomic store. Multiple threads can call `set_transform` for different objects simultaneously with no locking. Reads at `prepare()` time drain the atomic cells.
- [ ] **`SceneCommands` queue** for structural mutations: a lock-free multi-producer single-consumer queue (`crossbeam::SegQueue`). Any thread pushes commands; the render thread drains them at the start of `scene.prepare()`.
  ```rust
  scene.commands().add_mesh(mesh, program);     // any thread, any time
  scene.commands().remove_object(id);           // any thread, any time
  scene.commands().set_material(id, mat);       // any thread, any time
  // Applied at frame start:
  scene.prepare(&engine)?;  // drains commands, uploads GPU data
  ```
- [ ] `Scene::prepare()` is still called on the render thread. It drains `SceneCommands`, applies structural changes, then uploads any dirty transforms to GPU. This is the only moment the scene is exclusively mutated.
- [ ] **Read-only scene access from worker threads**: expose `Scene::mesh_at()`, `Scene::object_transform()`, and other read methods through a `SceneView` — a shared reference that can be sent across threads and provides immutable access to scene data between `prepare()` calls.

**ECS-MT-d-5: Thread-safe asset loading**

- [ ] `engine.load_texture_2d(path)` — already returns an `AssetHandle<Image>`. The load, decode, and GPU upload all run on worker threads from a shared `rayon` pool. The `Engine::global()` accessor means this can be called from any system without passing the engine around.
- [ ] `engine.load_mesh(path)` — same: decode on worker thread, GPU buffer upload via `Engine::write_buffer()` (which is already thread-safe per ECS-MT-d-2).
- [ ] `engine.load_hdr_texture(path)` — same pattern. The IBL precomputation (environment map convolution) runs on a worker thread; results uploaded when complete.
- [ ] All asset handles are `Arc<AssetState<T>>` — cloneable, `Send + Sync`. Multiple threads can query `handle.is_ready()` concurrently without locking.

**ECS-MT-d-6: Parallel shader and pipeline compilation**

PSO compilation is one of the most expensive operations in the engine (10–500 ms per variant). Running it on the render thread causes frame stutters.

- [ ] `UnifiedMaterial` variant compilation (`GBufferFillVariant`, `ForwardLitVariant`) runs on worker threads via `rayon::spawn`. The compiled `MeshProgram` is sent back to the render thread via a `crossbeam::channel` and inserted into `DeferredPass::variant_cache` at the start of the next frame.
- [ ] `DeferredPass::tick_hot_reload()` dispatches recompilation to worker threads rather than blocking the render thread. The last-known-good program remains active until the worker delivers the new one.
- [ ] `Engine::create_graphics_pipeline()` and `create_compute_pipeline()` are safe to call from any thread — Vulkan `vkCreateGraphicsPipelines` is thread-safe per spec when called with different `VkPipeline` output handles. The `Mutex<PipelineRegistry>` is held only for the registry insertion after creation, not during the compilation itself.
- [ ] `PipelineCompileTask`: a future / rayon task that compiles a PSO and returns `Result<Pipeline>`. The `PipelineRegistry` records in-flight tasks; queries for the same PSO key return a "compiling" sentinel instead of blocking.

### ECS-MT-e — rayon integration and schedule executor

- [x] Add `rayon` as a dependency. Use `rayon::ThreadPoolBuilder::new().num_threads(num_cpus::get().saturating_sub(1))` — reserve one core for the render thread and OS.
- [x] **`CompiledSchedule::run(&self, world: &World)`**: iterate waves sequentially. Within each wave, use `rayon::scope` to launch one task per system. Each task receives a `WorldView` (shared borrow with per-component `RwLock`s) and a `&mut WorldCommands`. The scope returns when all tasks in the wave complete.
- [x] **Timing diagnostics**: `CompiledSchedule::debug_timing: bool` records wall time per system and reports wave-level parallelism efficiency (actual elapsed / sum of system times).
- [x] **Backward compatibility**: `Schedule::run(&mut world)` continues to work. `CompiledSchedule` is opt-in via `Schedule::build()`.

### ECS-MT-f — WorldCommands (deferred structural mutations)

- [x] `WorldCommands` is a lock-free append-only buffer per system (one per wave slot). Commands: `Spawn { components }`, `Despawn { entity }`, `Insert { entity, component }`, `Remove { entity, type_id }`.
- [x] Parallel systems receive `&mut WorldCommands` alongside `WorldView`. After all systems in a wave finish, the main thread applies commands to `World` before starting the next wave.
- [x] `WorldCommands::spawn() -> EntityBuilder<'_>` — entity ID allocated immediately from an atomic counter on `EntityAllocator`; components recorded into the buffer. The entity exists in the world only after the wave's command flush.

---

## GPU Physics (Track 14)

Cross-platform GPU physics (Vulkan compute, no CUDA). Async compute queue. XPBD solver.

### 14a — Core XPBD solver
- [ ] XPBD integration loop in Slang: predict → solve (Gauss-Seidel with graph-coloured islands) → update velocities. Async compute queue.
- [ ] Configurable substeps: `PhysicsWorldConfig::substeps` (default 4; up to 20).
- [ ] Broad-phase: GPU LBVH rebuilt each frame (Morton code sort, O(n log n)).
- [ ] Narrow-phase: GJK/EPA convex-convex; SAT box-box and sphere-box; sphere-sphere analytic. Contact manifolds on GPU.
- [ ] `PhysicsWorldConfig` with full dials: gravity, substeps, solver_iterations, contact_offset, sleep_threshold, max_bodies, max_contacts.

### 14b — Rigid body dynamics
- [ ] `RigidBody`: mass, inertia tensor, angular/linear damping, sleeping, kinematic.
- [ ] Shapes: Sphere, Box, Capsule, ConvexHull, TriangleMesh (static/kinematic only).
- [ ] Joints: FixedJoint, BallJoint, HingeJoint, SliderJoint, SpringJoint. All XPBD constraints.

### 14c — Soft body and cloth
- [ ] XPBD soft body: tetrahedral mesh; distance + volume + shape-matching constraints.
- [ ] XPBD cloth: stretch + shear + bending constraints; `ClothConfig`.
- [ ] GPU spatial hash for cloth self-collision.

### 14d — Fluid (SPH)
- [ ] SPH on async compute: density, pressure, viscosity, surface tension. Spatial hash neighbour search.
- [ ] Surface extraction: marching cubes or screen-space fluid rendering.

### 14e — Scene queries
- [ ] GPU raycast, sphere cast, box cast, shape overlap via GPU BVH (async or sync).
- [ ] Trigger volumes with enter/stay/exit events delivered via compact CPU event buffer.

### 14f — Physics ↔ rendering
- [ ] Physics transforms → `GpuInstanceData` via GPU compute (zero CPU readback).
- [ ] `PhysicsWorld::debug_draw(frame)`: wireframe collision shapes.

---

## UI Layout Engine (Track 4)

The text system, input callbacks, and Clay UI bindings exist. There is no layout engine — the single blocker for the graphical-apps use case.

- [ ] Integrate `taffy` (pure-Rust flex/grid layout). Map widget descriptors to taffy nodes, run layout each frame, produce screen-space rectangles.
- [ ] `ScreenUiRoot`: layout tree, input dispatcher, focus scope, render pass.
- [ ] Core widgets: `Label`, `Button`, `TextInput`, `Checkbox`, `Toggle`, `Slider`, `ScrollRegion`, `Panel`, `Tabs`.
- [ ] Stable widget IDs, focus scopes, modal scopes, per-frame retained state.
- [ ] Root-level input routing: keyboard, mouse, scroll, pointer capture, text input ownership.
- [ ] Theme tokens: typography scale, spacing scale, radii, semantic colours, state colours.
- [ ] `WorldUiRoot` for UI on world-space panels with ray-to-panel hit testing and render-to-texture.
- [ ] Standalone app conveniences: menu bars, toolbars, resizable panes, tabbed documents, inspector panels.
- [ ] Accessibility tree: roles, names, descriptions, values, bounds, focus, selection, actions.

### Text system completeness
- [ ] Grapheme-aware cursor, word, and bidi movement; selection across wrapped lines.
- [ ] Single-line editable text field: cursor, selection, focus, clipboard, keyboard nav.
- [ ] Multiline editable text: scrolling, grapheme selection, IME composition, platform clipboard.
- [ ] Fallback fonts, emoji, combining marks, ligatures, OpenType features.
- [ ] SDF/MSDF rendering for large scalable text and world-space text.
- [ ] Atlas residency, eviction, dirty-rectangle upload.

---

## Area Lights, Photometric Luminaires (Track 15)

- [ ] **LTC area lights (raster)**: precompute `ltc_matrix.dds` + `ltc_amplitude.dds` (64×64 RGBA32Float). Implement `ltc_evaluate_rect`, `ltc_evaluate_disk`, `ltc_evaluate_sphere` in `brdf.slang`. Assign area lights to the cluster grid as point/spot lights.
- [ ] **Emissive mesh lights**: auto-register `UnifiedMaterial` with non-zero emissive as an area light source (AABB-derived rect). Video-driven emissive: `EmissiveConfig::source: EmissiveSource` (Constant / Texture / VideoStream).
- [ ] **IES photometric profiles**: load `.ies` candela distribution; upload as `R16Float` texture; apply as multiplicative attenuation on spot or area lights.
- [ ] **Flood lights**: `FloodLight` — high-power spot with IES profile, colour temperature (2700K–6500K), luminous intensity (cd), cookie texture (gobo).
- [ ] **Light units**: accept LuminousFlux (lm), Luminance (nits), LuminousIntensity (cd), Illuminance (lux). Convert to scene-linear radiance internally.
- [ ] **Auto-exposure integration**: physically specified lights scale correctly with the auto-exposure EV.

---

## Backend and Platform Maturity

### Vulkan backend
- [x] `VK_EXT_mesh_shader`: device extension when detected; `BackendFeatures::mesh_shading` set; commands routed through `mesh_shader_ext`.
- [x] `VK_AMDX_shader_enqueue`: `BackendFeatures::work_graphs` detected and set.
- [x] `vkCmdDrawIndexedIndirectCount` (Vulkan 1.2) for GPU-written draw counts; wired in commands.rs `PassWork::DrawIndirect`.
- [x] `VK_EXT_device_fault`: structured crash report on device-lost via `gather_device_fault_info`.
- [x] Buffer device address (`VK_KHR_buffer_device_address`): `Device::buffer_device_address()` returns `Option<u64>`.
- [ ] Vulkan parallel command recording via secondary command buffers.
- [ ] Multi-surface presentation: per-window surface capabilities, independent acquire/present sync.

### Slang compiler service
- [ ] `ShaderCompilerService`: worker-thread compilation, reflection, cache lookup; off the render thread.
- [ ] Compile Slang through its in-process C API; external `slangc` is a developer fallback only.
- [ ] Hot reload transaction: compile on worker, reflect/validate, swap at safe graph boundary, keep last-known-good.
- [ ] Games ship without `slangc`, Vulkan SDK, or any external compiler on the player machine.
- [ ] Reflect specialisation constants.

### Platform isolation
- [ ] Move OS-specific code into `crates/sturdy-engine-platform/src/{linux,windows,macos}/...`; engine code on platform-neutral APIs.
- [ ] Directories: `linux/wayland/`, `linux/x11/`, `windows/window_effects/`, `macos/window_effects/`.

---

## Multi-Window, Workspace, and Docking

- [ ] `WindowRegistry` / `WindowManager` with generation-checked `WindowHandle`s.
- [ ] Per-window surface, swapchain, present mode, frame pacing, DPI/safe-area, cursor state, IME state.
- [ ] `FrameSet`: zero or more `WindowFrame`s, each acquiring, rendering, submitting, presenting independently.
- [ ] Mixed cadence: one window continuous, another redraws only when dirty.
- [ ] `Workspace` model: dock trees, tabs, panels, floating panels, native-window placements.
- [ ] Split panes, tab stacks, floating panels, detach-to-window, merge-window-back, drag-panel-between-windows.
- [ ] Workspace serialisation with monitor-aware restore and graceful fallback.
- [ ] Cross-window drag/drop for panels, assets, tabs, documents, files.
- [ ] Multi-window tests: create, resize, render, minimize, restore, close, recreate while other windows keep rendering.

---

## Full Asset Pipeline

- [ ] `ContentRuntime`: asset requests, handles, background I/O, decode/transcode workers, upload plans, residency state.
- [ ] Staged pipeline: Requested → Reading → Decoded → Transcoded → UploadQueued → GpuResident → Ready → Degraded → Failed → Evicted.
- [ ] Texture streaming: tiny fallback mip immediately, progressive high-mip refinement, budget eviction.
- [ ] Per-frame upload budgeting: bytes/frame, images/frame, staging memory, transfer queue time.
- [ ] Staging ring allocator for async uploads without per-upload allocation churn.
- [ ] Content priority and cancellation: visible-now, near-future prefetch, UI-critical, low-priority, cancelled.
- [ ] Asset hot reload using the same handle/state system as the streaming path.

### I/O backends
- [ ] Linux: prefer `io_uring`, fall back to blocking thread pool.
- [ ] Windows: prefer DirectStorage where it fits the Vulkan pipeline, fall back to overlapped I/O.

---

## Low-Latency Presentation (Track LL)

*Source: deep-research-report.md, §Low-latency input, rendering, and presentation.*

Visual latency is a whole-stack problem. Operating systems and compositors decide present cadence; graphics APIs expose present modes and queue controls; the engine decides when to sample input, how many frames to pipeline, and when to commit the final camera state. A high-quality engine must control all three layers.

### Present mode selection

- [ ] Query available present modes at surface creation; expose `SurfacePresentStrategy { Fifo, Mailbox, Immediate }` alongside `SurfacePresentMode`. Default to `Fifo` (universally available, stable cadence); offer `Mailbox` for low-latency windowed mode; document `Immediate` as tearing-risk.
- [ ] On Windows: request DXGI flip-model swap chain with a frame-latency waitable object (`DXGI_SWAP_CHAIN_FLAG_FRAME_LATENCY_WAITABLE_OBJECT`). Block on the waitable at the top of the frame loop to pace at exactly one frame of buffering when desired.
- [ ] Expose `SurfaceConfig::max_frames_in_flight: u32` (default 2). Clamped to swapchain image count. Lower values reduce latency; higher values reduce GPU stalls on slow frames.
- [ ] `Surface::present_latency_hint() -> Duration` — best-effort estimate of display latency from queue submission to photons, derived from `VkPastPresentationTimingGOOGLE` or `DXGI_FRAME_STATISTICS`.

### Late input sampling

- [x] **Sample input as late as possible** before GPU submission, not at the top of the frame. A late sample can save 4–8 ms of input latency versus sampling at game-logic time.
- [x] Add `InputHub::sample_late()` — a second, render-thread snapshot (`LateSample`) taken immediately before `frame.flush()`. Camera jitter, view matrix, and motion vectors should use this snapshot, not the physics snapshot.
- [ ] `FixedUpdateContext` keeps the physics snapshot; `GameContext::render_input` is the late-sampled snapshot.

### Frame-time diagnostics

Research (PresentMon, PIX, Vulkan timestamp queries) consistently shows that mean FPS is a poor metric. What matters is per-frame time stability and latency breakdown.

- [x] `FrameTimingReport` per frame: `cpu_ms`, `gpu_ms`, `present_to_display_ms`, `total_latency_ms`. Surfaced via `RuntimeGraphDiagnostics`; present/display latency remains `None` until a backend supplies display timing, so total latency is only populated when all components are measured.
- [x] Track P95 and P99 frame times over a rolling 128-frame window alongside the mean. Expose via `RuntimeTimingSummary`.
- [x] Log a warning when P99 exceeds 2× P50 (jitter spike) or when GPU occupancy drops below 70% (CPU-bound frame).
- [x] `Engine::frame_timing() -> Option<FrameTimingReport>` — callable from any thread, updated each frame.

---

## Research Horizons (2026 and beyond)

*These items derive from peer-reviewed and preprint research as of May 2026. They are directional signals, not committed roadmap items. Designs should not hard-depend on them; they become concrete when the technique is production-proven and implementation cost is understood.*

### Neural denoising (SIGGRAPH Asia 2024)

**Online Neural Denoising with Cross-Regression for Interactive Rendering** (ACM TOG, SIGGRAPH Asia 2024) shows that real-time denoising is moving from hand-built bilateral/SVGF filters toward online, temporally aware neural reconstruction that accumulates information across frames with learned kernels.

- [ ] **Short-term** (no change): ship SVGF for RT shadow and GI denoising as planned (Track Shadow, Track GI).
- [ ] **Medium-term**: design the SVGF denoiser pass with a replaceable kernel slot. When a neural denoiser is available (compiled ONNX or custom Slang kernel), it slots in without changing the pass interface.
- [ ] Neural denoiser output should feed the same temporal accumulation path as SVGF so motion-vector reprojection still works.

### GPU procedural geometry (Eurographics 2025)

**Real-time Procedural Resurfacing Using GPU Mesh Shader** (CGF 2025) and **Real-Time GPU Tree Generation** (HPG 2025) demonstrate that mesh shaders are most valuable not as a vertex-stage replacement, but as a **GPU-side detail-generation mechanism** that can produce geometry on the fly from compact procedural descriptions.

- [ ] Add `ProceduralMeshProvider` trait: given a cluster or seed, emit mesh-shader-compatible meshlet data. Connects to Track 7 (VirtualMesh) hardware rasterization path.
- [ ] Evaluate tree/foliage generation as a candidate for GPU procedural instancing: store a procedural descriptor per instance rather than full geometry.

### Wavelet-space super-resolution (arXiv 2025)

**Wavelet-Space Super-Resolution for Real-Time Rendering** proposes a non-proprietary upscaler that better preserves high-frequency structure than image-space methods.

- [ ] When FSR 3.1 (Track 10) is implemented, expose `UpscalerConfig::algorithm: UpscalerAlgorithm` with `Fsr31`, `Xess2`, and `Wavelet` variants. Wavelet becomes available when the technique matures.
- [ ] The unified upscaler interface (Track 10c) already accounts for this via `UpscalerConfig::auto()`.

### 3D Gaussian Splatting and streaming representations

**Streaming Real-Time Rendered Scenes as 3D Gaussians** (arXiv 2026) replaces viewpoint-locked 2D video with a streamable 3D representation that supports view correction — directly relevant to cloud rendering and XR.

- [ ] Track as a **Scene representation** option alongside raster and RT: `SceneRepresentation::GaussianSplat`.
- [ ] Gaussian splat rendering requires sorted alpha compositing (OIT is an approximation); a dedicated splat rasterizer (compute-based) would be correct.
- [ ] Not a near-term deliverable; keep the architecture open by not hard-wiring assumptions that only raster geometry will be used.

### Hybrid foveated path tracing (arXiv 2026)

**Hybrid Foveated Path Tracing with Peripheral Gaussians** combines high-fidelity foveal rendering with approximate peripheral Gaussian representation and depth-guided reprojection — a systems idea for XR.

- [x] Foveated rendering requires eye-tracking input (`InputHub::gaze_direction() -> Option<Vec2>`). `InputHub` now exposes optional normalized gaze direction, returns `None` without hardware, accepts backend samples, and ignores invalid non-finite samples.
- [ ] `RenderConfig::foveal_region: Option<FovealDesc>` — describes the high-detail region. Passes that support VRS (Track 8c) use this to reduce peripheral shading rate.

---

## Code Organisation

This is a continuous engineering track, not a polish pass. Capability work should leave
the engine easier to navigate than it found it. Files must stay digestible: target ≤ 800
lines for any `.rs` source file, split files over ~1 200 lines unless there is a written
reason they are intentionally cohesive, and keep module boundaries aligned with ownership
and invariants rather than arbitrary line count.

Public API compatibility matters during this work. Move implementation into focused
modules, then keep existing public paths alive with `pub use` re-exports until an explicit
breaking-change window.

### Project-wide organisation rules

- [ ] Add a lightweight file-size report script or CI check that lists `.rs` files over 800 lines and fails only when a file crosses 1 200 lines without an allow-list note.
- [ ] Keep facade files (`lib.rs`, `mod.rs`, backend entry points) mostly declarations and re-exports. Implementation-heavy impl blocks move into owned modules.
- [ ] Split types together with their inherent impls, trait impls, `Default` impls, and small type-local helper functions. Do not leave orphan impl blocks behind in facade files.
- [ ] Split pure functional code by job: parsing, validation, scheduling, rasterization, upload, diagnostics, and platform translation each get their own module when they grow independently.
- [ ] Keep test-only code in `#[cfg(test)] mod tests` blocks or `tests/` modules beside the subsystem they validate. Shared shader fixtures live under `shaders/tests/`.
- [ ] Keep generated shader templates, runtime shaders, and test shaders in separate folders so language accounting and shader-error searches stay accurate.
- [ ] When a split touches public types, add compatibility re-exports in the old module and update `docs/architecture.md` with the new ownership boundary.
- [ ] Prefer one module per lifecycle owner: creation/bootstrap, runtime mutation, cache/registry, diagnostics/reporting, and tests should not share one large file.

### Current files over 1 000 lines

Snapshot from `find crates -name '*.rs' -print0 | xargs -0 wc -l | awk '$2 != "total" && $1 > 1000' | sort -nr` on 2026-05-13:

| File | Lines | Organisation need |
|---|---:|---|
| `crates/clay-ui/src/layout/widgets/mod.rs` | 4 556 | Continue splitting widget builders by family and shared behavior. |
| `crates/sturdy-engine/src/frontend_graph.rs` | 3 425 | Finish graph image and render-frame extraction. |
| `crates/sturdy-engine/src/runtime.rs` | 2 817 | Split config, diagnostics, controller, and runtime shell. |
| `crates/sturdy-engine/src/application.rs` | 2 588 | Split window setup, shell frame, event loop runner, and app traits. |
| `crates/sturdy-engine/src/lib.rs` | 2 111 | Move `Engine` impls into `engine/*` modules; keep facade exports. |
| `crates/sturdy-engine-core/src/render_graph.rs` | 2 111 | Split graph model, compiler, barriers, scheduler, aliasing, and validation. |
| `crates/textui/src/atlas.rs` | 1 957 | Split atlas allocation, glyph upload, cache policy, and diagnostics. |
| `crates/sturdy-engine/src/tests.rs` | 1 664 | Group integration tests into domain modules. |
| `crates/sturdy-engine-core/src/backend/vulkan/commands.rs` | 1 568 | Split command contexts, frame acquisition, submission, barriers, and debug labels. |
| `crates/sturdy-engine-testbed/src/main.rs` | 1 564 | Split app state, tonemapping controls, runtime settings, resources, and entry point. |
| `crates/sturdy-engine-core/src/slang.rs` | 1 523 | Split sessions, modules, diagnostics, reflection, and panic-policy tests. |
| `crates/sturdy-engine-core/src/device.rs` | 1 494 | Split backend-agnostic device traits, handles, features, and errors. |
| `crates/textui/src/text_ui_input_widget.rs` | 1 450 | Split text editing model, layout, event handling, and rendering bridge. |
| `crates/textui/src/lib.rs` | 1 384 | Move implementation into modules; keep crate facade lean. |
| `crates/clay-ui/src/layout/input/tests.rs` | 1 261 | Split behavior tests by focus, scroll, slider, callbacks, and overlay scopes. |
| `crates/clay-ui/src/layout/input/simulator.rs` | 1 158 | Continue splitting pointer routing, keyboard routing, focus, scroll, and slider handling. |
| `crates/textui/src/editor.rs` | 1 058 | Split editor state, cursor/selection movement, edits, undo/redo, and scroll metrics. |

### Per-file split plans

| File | Target organisation |
|---|---|
| `crates/clay-ui/src/layout/widgets/mod.rs` | Initial split done: `layout/widgets/mod.rs` is now the facade; `widgets/palette.rs` owns `ToggleAnimConfig`, `WidgetPalette`, and `WidgetRenderContext`; `widgets/types.rs` owns widget style/config/spec types; `widgets/selection.rs` owns button/radio/checkbox/toggle/segmented controls; `widgets/slider.rs` owns `DragBarAxis`, `SliderStyle`, drag bars, sliders, and progress bars; `widgets/scroll.rs` owns `ScrollbarMetrics`, scrollbars, and scroll containers; `widgets/overlays.rs` owns portal hosts, modal layers, and tooltip layers/surfaces. Next splits: context-menu overlays, `widgets/inputs.rs` for text, number, search, select; `widgets/virtualized.rs` for virtual list/dropdown/log/grid/table/tree/mosaic; `widgets/navigation.rs` for tabs, breadcrumbs, accordion; `widgets/data.rs` for list item, table header, property row, chip; `widgets/feedback.rs` for badges, notifications, status bar; `widgets/primitives.rs` for local layout/style helpers. |
| `crates/clay-ui/src/layout/input.rs` | Initial split done: `layout/input.rs` is now a facade with `input/types.rs`, `input/simulator.rs`, `input/helpers.rs`, and `input/tests.rs`. Remaining split: `input/context.rs` for `PendingRegistrations`, `Cx`, `EventContext`, `UiEventResult`; `input/events.rs` for event enums and payload structs; `input/behavior.rs` for `WidgetKind`, `WidgetBehavior`, `WidgetConfig`, `WidgetState`; `input/scroll.rs`; `input/focus.rs`; `input/dispatch.rs`; `input/slider.rs`; split tests by behavior family. |
| `crates/sturdy-engine/src/frontend_graph.rs` | `frontend_graph/mod.rs` facade; finish `graph_image.rs` with `GraphImage*` types and impls; `render_frame.rs` for `RenderFrame` and `RenderFrameInner`; `pass_intent.rs` for `ShaderPassIntent`; `recording.rs` for fullscreen/compute recording helpers; `resources.rs` for explicit resource import and subresource validation; `submit.rs` for pending-pass submission; keep `scheduler.rs` and `reflection.rs`; move tests into `frontend_graph/tests.rs`. |
| `crates/sturdy-engine/src/runtime.rs` | `runtime/mod.rs` facade; `runtime/app.rs` for `AppRuntime`; `runtime/frame.rs` for `AppRuntimeFrame`; `runtime/controller.rs` for `RuntimeController` and change reports; `runtime/settings.rs` or `runtime/settings/*` for setting ids, values, descriptors, entries, support, sources, transactions, and defaults; `runtime/diagnostics.rs` for diagnostics structs; `runtime/timing.rs` for `FrameTimingReport`, `RuntimeTimingSummary`, history; `runtime/debug_images.rs`; `runtime/context.rs` for scene/UI context structs. |
| `crates/sturdy-engine/src/application.rs` | `application/mod.rs` facade; `application/config.rs` for `WindowConfig`, `WindowDesc`, shell commands; `application/app.rs` for `EngineApp`; `application/shell_frame.rs` for `ShellFrame`, motion vectors, post-process output; `application/runner.rs` for `run`/`try_run`; `application/shell_app.rs` for `ShellApp` lifecycle; `application/gamepad.rs`; `application/window.rs` for window state/create/apply helpers; `application/window_settings.rs` for runtime setting translation; platform-specific helpers under `application/platform/`. |
| `crates/sturdy-engine/src/lib.rs` | Keep as crate facade; `engine/mod.rs` owns `Engine`; `engine/core.rs` for construction/global/wait/surface basics; `engine/resources.rs` for images, buffers, samplers, shaders; `engine/pipelines.rs`; `engine/assets.rs`; `engine/bindless.rs`; `engine/frame.rs`; `engine/sync.rs` for `FrameSync*`; `handles.rs` for `Image`, `Buffer`, `Sampler`, `Shader`, `BindGroup`, `PipelineLayout`, `Pipeline`, `Surface`, `SurfaceImage`; `passes.rs` for `DrawPassBuilder` and `ComputePassBuilder`; `frame.rs` for `Frame`. |
| `crates/sturdy-engine-core/src/render_graph.rs` | `render_graph/mod.rs` facade; `render_graph/model.rs` for queue/access/state/use/key structs; `render_graph/work.rs` for draw/dispatch/copy/pass work descriptions; `render_graph/resources.rs` for virtual image/buffer/resource state; `render_graph/graph.rs` for `RenderGraph` type and type-local impls; `render_graph/compiler.rs` for compile logic; `render_graph/scheduler.rs` for record batches; `render_graph/barriers.rs`; `render_graph/copy_validation.rs`; keep `alias_plan.rs`; move tests into `render_graph/tests.rs`. |
| `crates/textui/src/atlas.rs` | Keep `atlas.rs` as facade; `atlas/key.rs` for raster keys; `atlas/glyph_atlas.rs` for `GlyphAtlas` and its impl; `atlas/worker.rs` for worker loop/messages; `atlas/cursor.rs` for cursor-stop and glyph-cluster math; `atlas/raster_alpha.rs`; `atlas/raster_field.rs`; `atlas/outline.rs` for flattening and winding; `atlas/field_encoding.rs`; `atlas/image_ops.rs` for color-image sampling/subimages; `atlas/upload.rs` for texture upload/write paths. |
| `crates/sturdy-engine/src/input.rs` | Initial split done: parent is now ~977 lines with `input/keybind.rs`, `keyboard.rs`, `gamepad.rs`, `capture.rs`, `actions.rs`, `display.rs`, and `winit_bridge.rs`. Remaining optional splits: `input/hub.rs`, `input/clay_bridge.rs`, and tests beside owning modules. |
| `crates/sturdy-engine/src/tests.rs` | Replace with `tests/mod.rs`; split into `tests/bind_groups.rs`, `tests/shader_reflection.rs`, `tests/render_frame.rs`, `tests/runtime.rs`, `tests/graph_report.rs`, `tests/input.rs`, `tests/upload.rs`, `tests/sync.rs`, `tests/deferred_destroy.rs`, and `tests/backend_null.rs`. Keep shader fixtures in `shaders/tests/`. |
| `crates/sturdy-engine/src/scene/scene.rs` | Initial split done: parent is now ~995 lines with `scene/lights.rs`, `gpu_constants.rs`, `gpu_culling.rs`, `material_state.rs`, `queries.rs`, existing `gpu_instance.rs`, and `batch.rs`. Remaining optional splits: `scene/prepare.rs`, `scene/draw.rs`, and camera/output orchestration if the parent grows again. |
| `crates/sturdy-engine-core/src/backend/vulkan/commands.rs` | `commands/mod.rs` facade; `commands/context.rs` for `CommandContext`; `commands/render.rs` for render pass/draw recording; `commands/compute.rs`; `commands/copy.rs`; `commands/barriers.rs` for access/stage/layout/aspect helpers; `commands/push_constants.rs`; `commands/subresources.rs`; `commands/framed.rs` for `FramedCommands`; keep `batch_pool.rs`. |
| `crates/sturdy-engine-testbed/src/main.rs` | `main.rs` only starts the app; `testbed/app.rs` for `Testbed` and `EngineApp` impl; `testbed/tonemap.rs` for settings/dials/operator parsing; `testbed/aa.rs`; `testbed/resources.rs` for shader/texture setup; `testbed/ui.rs` for debug/runtime UI; `testbed/runtime_settings.rs`; `testbed/helpers.rs` for labels/sanitization/path helpers. |
| `crates/sturdy-engine-core/src/slang.rs` | `slang/mod.rs` facade; `slang/ffi.rs` for raw `sys`; `slang/session.rs` for session/request guards; `slang/source.rs` for source-input handling and SPIR-V byte parsing; `slang/reflection.rs`; `slang/layout_merge.rs`; `slang/compile.rs` for CLI/compiler artifact paths; `slang/targets.rs`; `slang/diagnostics.rs`; keep `spirv_push_constants.rs` and `spirv_vertex_inputs.rs`; move tests into `slang/tests.rs`. |
| `crates/sturdy-engine-core/src/device.rs` | `device/mod.rs` facade; `device/desc.rs` for `DeviceDesc`; `device/adapter.rs`; `device/inner.rs` for `DeviceInner`; `device/resources.rs`; `device/pipelines.rs`; `device/bind_groups.rs`; `device/surfaces.rs`; `device/frame.rs` for `Frame`; `device/deferred_destroy.rs`; `device/validation.rs`; `device/reflection.rs` for shader layout merge/cache helpers. |
| `crates/textui/src/text_ui_input_widget.rs` | `input_widget/mod.rs` facade; `input_widget/singleline.rs`; `input_widget/multiline.rs`; `input_widget/rich_viewer.rs`; `input_widget/events.rs` for keyboard/pointer/gamepad handling; `input_widget/paint.rs`; `input_widget/scrollbars.rs`; `input_widget/state.rs` for state sync and cleanup; pure cursor/edit helpers stay in `editor/*`. |
| `crates/textui/src/lib.rs` | Keep as crate facade and public types; `config.rs` for text fundamentals/options/color/raster config; `gpu_scene.rs` for `TextGpuQuad`, `TextGpuScene`, page data; `text_ui.rs` for `TextUi` lifecycle; `layout.rs` for buffer/layout measurement; `raster/mod.rs`, `raster/alpha.rs`, `raster/field.rs`, `raster/outline.rs`, `raster/distance.rs`; `cache_key.rs`; tests by module. |
| `crates/textui/src/editor.rs` | `editor/mod.rs` facade; `editor/state.rs` for `InputState`, undo entries, scroll metrics; `editor/cursor.rs`; `editor/selection.rs`; `editor/navigation.rs`; `editor/edits.rs` for insert/delete/paste/cut; `editor/undo.rs`; `editor/scroll.rs`; `editor/layout.rs` for visual cursor and preferred-x helpers. |
| `crates/sturdy-engine/src/deferred_pass.rs` | Initial split done: parent is now ~758 lines with `deferred_pass/config.rs`, `constants.rs`, `helpers.rs`, `hot_reload.rs`, `oit.rs`, and `shadows.rs`. Remaining optional splits: `deferred_pass/pass.rs` for constructors, `gbuffer.rs`, `lighting.rs`, and `environment.rs` if the parent grows again. |

### Cross-crate follow-up tracks

- [ ] **Clay UI split**: Clay source now lives under `crates/clay-ui/src/layout/` with public module paths preserved from `lib.rs`. `layout/widgets/` exists with palette/context in `widgets/palette.rs`, widget specs in `widgets/types.rs`, selection controls in `widgets/selection.rs`, slider/progress controls in `widgets/slider.rs`, scroll primitives in `widgets/scroll.rs`, and portal/modal/tooltip overlays in `widgets/overlays.rs`; next, split context menus, text/input, virtualized, navigation, data, feedback, and shared primitive helpers.
- [ ] **Clay UI input split**: first pass done with `layout/input.rs` as a facade over `input/types.rs`, `input/simulator.rs`, `input/helpers.rs`, and `input/tests.rs`. Continue separating raw events, pointer capture, keyboard focus, navigation, gesture recognition, and test helpers.
- [ ] **Text UI split**: move atlas allocation, glyph cache policy, upload staging, editor state, input widget behavior, and scene building into separate modules. Keep `textui::lib` as a small public facade.
- [ ] **Engine-core render graph split**: extract `render_graph/model.rs`, `compiler.rs`, `barriers.rs`, `scheduler.rs`, `aliasing.rs`, `validation.rs`, and `diagnostics.rs`. This pairs with async compute, transient memory, and future backend support.
- [ ] **Vulkan backend split**: keep `backend/vulkan/mod.rs` as the backend facade. Move command recording/submission, descriptor allocation, resource creation, synchronization, feature queries, and pipeline cache handling into independent ownership modules.
- [ ] **Slang service split**: isolate compiler session setup, module loading, reflection, diagnostic formatting, shader cache keys, and test fixtures. This keeps shader-input expansion work from growing a single compiler file.
- [ ] **Test suite split**: replace large crate-level `tests.rs` files with domain modules (`tests/render_graph.rs`, `tests/materials.rs`, `tests/runtime.rs`, `tests/scene.rs`, `tests/shaders.rs`) or integration tests where they exercise public APIs.

### `crates/sturdy-engine/src/frontend_graph.rs` (≈ 3 425 lines)

The first two extractions are done. The remaining file still holds graph-image ownership
and frame-building logic that should become separate modules:

| Target file | Contents | Est. lines |
|---|---|---|
| `graph_report.rs` | `PassKind`, `GraphPassInfo`, `GraphImageInfo`, `GraphReport`, `DiagnosticLevel`, `GraphDiagnostic` | ~120 |
| `shader_program.rs` | `ShaderProgramDesc`, `ShaderName`, `SlangEntryPoints`, `ShaderProgram` | ~360 |
| `graph_image.rs` | `GraphImage`, `GraphImageView`, `GraphImageCacheKey`, `GraphImageDescKey`, `ImageRef for GraphImage` | ~1 600 |
| `render_frame.rs` | `RenderFrame`, `ShaderPassIntent`, frame builders | ~1 700 |

- [x] Extract `graph_report.rs`
- [x] Extract `shader_program.rs`
- [ ] Extract `graph_image.rs`
- [ ] Extract `render_frame.rs`

### `crates/sturdy-engine/src/runtime.rs` (≈ 2 817 lines)

| Target file | Contents | Est. lines |
|---|---|---|
| `runtime/config.rs` | `RuntimeSettingDescriptor`, `RuntimeSettingEntry`, `RuntimeSettingId`, `RuntimeSettingKey`, `RuntimeSettingOption`, `RuntimeSettingValue`, `RuntimeSettingSource`, `RuntimeSettingSupport`, `RuntimeSettingChange`, `WindowMode`, `RuntimeSettingsSnapshot`, `RuntimeSettingsTransaction` | ~500 |
| `runtime/diagnostics.rs` | `AssetDiagnostic`, `AssetState`, `RuntimeDiagnostics`, `RuntimeGraphDiagnostics`, `RuntimePassTiming`, `RuntimeTimingSummary`, `RuntimeUserDiagnostic`, `RuntimeWindowDiagnostics`, `DebugImageRegistry` | ~400 |
| `runtime/controller.rs` | `RuntimeController`, `RuntimeApplyNotification`, `RuntimeApplyPath`, `RuntimeApplyReport`, `RuntimeChangeResult` | ~400 |
| `runtime/mod.rs` | `AppRuntime`, `AppRuntimeFrame`, `SceneRenderContext`, `UiContext`, re-exports | ~1 379 |

- [ ] Extract `runtime/config.rs`
- [ ] Extract `runtime/diagnostics.rs`
- [ ] Extract `runtime/controller.rs`

### `crates/sturdy-engine/src/application.rs` (≈ 2 588 lines)

| Target file | Contents | Est. lines |
|---|---|---|
| `application/window.rs` | `WindowConfig`, `WindowDesc`, `WindowMode`, per-window state | ~400 |
| `application/shell_frame.rs` | `ShellFrame`, post-process helpers, `RuntimePostProcessDesc`, `RuntimePostProcessOutput` | ~600 |
| `application/app_runner.rs` | `run`, `try_run`, event loop wiring | ~800 |
| `application/mod.rs` | `EngineApp` trait, `MotionVector*`, re-exports | ~788 |

- [ ] Extract `application/window.rs`
- [ ] Extract `application/shell_frame.rs`
- [ ] Extract `application/app_runner.rs`

### `crates/sturdy-engine/src/lib.rs` (≈ 2 071 lines)

`lib.rs` is mostly the `Engine` struct and its impls. The clean split is:

| Target file | Contents | Est. lines |
|---|---|---|
| `engine/core.rs` | `Engine` struct, `new`, `with_backend`, global accessor, `wait_idle`, surface methods | ~400 |
| `engine/resources.rs` | `create_image`, `create_buffer`, `create_sampler`, `create_shader`, `write_buffer`, etc. | ~400 |
| `engine/assets.rs` | `load_texture_2d`, `load_hdr_texture*`, `load_mesh`, `drain_pending_uploads`, `checkerboard_texture`, `generate_texture_2d` | ~350 |
| `engine/bindless.rs` (separate from `bindless.rs`) | `register_bindless_*`, `bindless_supported` | ~80 |
| `engine/frame.rs` | `begin_frame`, `begin_render_frame`, `begin_frame_for_surface`, `render_image` | ~80 |
| `lib.rs` | RAII wrappers (`Image`, `Buffer`, `Sampler`, `Shader`, `Pipeline`, `Surface`, `Frame`, `SurfaceImage`), all `pub use` re-exports | ~760 |

- [ ] Extract `engine/core.rs`
- [ ] Extract `engine/assets.rs`

---

## Ongoing Architectural Constraints

### Threading (Law 2 compliance)

- [ ] **Every public API is callable from any thread.** No method may panic, deadlock, or return an error solely because it was called from a non-render thread. If a method cannot currently be made thread-safe, it is not shipped until it can be.
- [ ] **No thread-affinity documentation is acceptable as a permanent state.** If a doc comment says "must be called from the render thread" or "main thread only," that is a bug tracker entry, not a design decision.
- [ ] **Test thread safety explicitly.** Any new public API ships with a test that calls it from a `std::thread::spawn`ed thread. Integration tests cover concurrent calls to `Engine::create_buffer`, `scene.set_transform`, `engine.load_texture_2d` from multiple threads simultaneously.
- [ ] **Audit existing APIs.** Document the current threading status of every `Engine`, `Scene`, `RenderFrame`, `DeferredPass`, and `World` method. For each one that is not thread-safe: file a task, link to the ECS-MT sub-item that fixes it, and add a `#[doc = "⚠ Not yet thread-safe — see Track ECS-MT-d-N"]` attribute.
- [ ] **The render thread owns submission, not APIs.** The render thread is distinguished only in that it calls `vkQueueSubmit`. Nothing else is render-thread exclusive. Any API that is currently render-thread-only due to implementation convenience (not hardware constraint) must be moved to the shared path.

### General

- [ ] Treat "requires restart" as a failure unless the OS/compositor makes it impossible.
- [ ] Restrict CPU/GPU waiting to frame-boundary policy: frames-in-flight throttling, swapchain/present, explicit shutdown/device-loss recovery.
- [ ] Add diagnostics for accidental synchronisation: blocking upload, pipeline compile stall, fence wait outside shutdown.
- [ ] Keep the deferred frame submission contract: app enqueues intent, flush encodes and submits, GPU does not wait until next frame's fence.
- [ ] Standardise time as monotonic `Instant`/`Duration` at engine boundaries; floating seconds only as convenience views.
- [ ] Standardise colour handling: linear scene colour internally, explicit sRGB decode/encode at I/O boundaries.
- [ ] Standardise resource debug labels for all surfaces, images, buffers, passes, pipelines, and generated resources.
- [ ] Standardise capability queries before feature enablement.
- [ ] **Track time-domain frame metrics, not just FPS.** Mean FPS masks jitter and tail latency. `FrameTimingReport` must include CPU duration, GPU duration, present-to-display duration, P95, and P99 over a 128-frame rolling window. See Track LL.
- [ ] **Late input sampling.** Camera matrix and motion vectors used in the render path must be derived from the latest possible input snapshot, not the game-logic snapshot. See Track LL.
- [ ] **Minimize frame queuing.** Default to 2 frames in flight. Never allow unbounded command buffer queuing; always pace against the waitable object or fence. See Track LL.
