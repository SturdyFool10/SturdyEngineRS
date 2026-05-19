# Sturdy Engine Roadmap

_Last updated: 2026-05-19_

## Product Direction

Sturdy Engine is a backend-neutral graphics-engine skeleton whose main goal is a fast, measurable, GPU-driven renderer capable of dense scenes that can plausibly read as real life.

The repo already contains substantial foundation work: backend-neutral core/render-graph infrastructure, a Vulkan path, a runtime shell, ECS/render-world bridging, deferred PBR, Hi-Z/OIT/post/shadow/environment-map modules, asset loading, material variants, GPU timing, memory infrastructure, and backend capability detection. This roadmap does **not** keep completed foundation work as tasks. It only lists the work still needed to turn those systems into the default, measured product path.

## Non-Negotiable Direction

1. **Measure first.** Every major rendering change must improve or intentionally trade against reference-scene metrics.
2. **The simple path is the serious path.** `AppRuntime` / `AppRenderer` should own the default frame loop and renderer stack so examples and apps do not rebuild engine plumbing.
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

---

## Priority 1 — Realistic Reference Scene And Benchmark Harness

Build the brutal test scene before adding more renderer features. It defines what “realistic enough” and “fast enough” mean.

### Scene requirements

- [ ] Create `examples/realistic_reference_scene` as the primary renderer regression example.
- [ ] Include HDR lighting and an indoor/outdoor exposure transition.
- [ ] Include glass/translucency, wet surfaces, foliage/clutter, dense static geometry, animated objects, emissives, shadows, and camera motion.
- [ ] Include debug views for G-buffer channels, depth, Hi-Z, motion vectors, material IDs, light clusters/visibility, shadow cascades, OIT, exposure, and final tonemapped output.
- [ ] Provide deterministic camera paths for benchmark and screenshot comparison runs.
- [ ] Add asset/scenario scale presets: smoke, default, stress, and pathological.

### Benchmark harness

- [ ] Add a first-party benchmark mode that records CPU/GPU frame time, P95/P99, pass timings, memory, upload bandwidth, visible/submitted triangle counts, and draw/dispatch counts.
- [ ] Export machine-readable benchmark reports.
- [ ] Add screenshot/export support for fixed camera frames.
- [ ] Add screenshot comparison hooks with tolerances suitable for temporal rendering.
- [ ] Make the reference scene the default target for renderer regression checks.

Acceptance: a renderer change can be judged against numbers and images, not subjective impressions.

---

## Priority 2 — First-Party Runtime Shell As The Default Product Path

The runtime shell should own the common frame pipeline. Testbed/example code should provide content and hooks, not rebuild HDR/MSAA/bloom/AA/tonemap/debug plumbing.

### AppRuntime / AppRenderer

- [ ] Move the common frame pipeline into `AppRuntime` / `AppRenderer`: surface acquire/present, HDR policy, scene targets, MSAA, bloom, AA, tonemapping, debug images, text overlay, and diagnostics.
- [ ] Make `AppRuntime` own the default frame loop for graphical apps.
- [ ] Route examples through the first-party runtime shell instead of custom per-example renderer assembly.
- [ ] Keep lower-level access available without making it the default path.

### Runtime settings

- [ ] Apply runtime settings through transactions.
- [ ] Every setting application must report `Applied`, `Degraded`, or `Rejected` with a reason and apply path.
- [ ] Support apply paths for immediate changes, graph rebuilds, surface recreation, window reconfiguration, and deferred/device-level changes.
- [ ] Expose settings snapshots and renderer diagnostics through the runtime controller.

### Debug shell

- [ ] Expose HDR, AA, post stack, render targets, pass timings, memory, graph inspection, backend/capability details, and runtime setting results.
- [ ] Support screenshot/export from the shell.
- [ ] Support shader and asset hot reload from the shell.
- [ ] Make debug image registration a renderer/runtime service, not testbed-local state.

Acceptance: a new graphical app gets the serious renderer, diagnostics, settings, and debug shell by default.

---

## Priority 3 — Truly GPU-Driven Render Path

The biggest performance unlock is replacing CPU object expansion with persistent GPU scene data, GPU transform generation, GPU culling, GPU compaction, and indirect-count draws.

### Object source and transform generation

- [ ] Store compact GPU object source buffers: transform source, hierarchy/parent index, mesh ID, material ID, flags, bounds source, animation/skinning handles, and visibility data.
- [ ] Generate world matrices, previous matrices, normal matrices, and render bounds on the GPU.
- [ ] Stop treating CPU-materialized model matrices and world bounds as the default render input.
- [ ] Preserve previous/current transform buffers for temporal passes.

### Visibility and draw generation

- [ ] Replace per-batch GPU culling with one cull dispatch per view/pass over the render world.
- [ ] Wire Hi-Z occlusion into the render-world culling path.
- [ ] Implement draw/dispatch compaction with GPU-written visible counts.
- [ ] Use `draw_indirect_count` / backend equivalent where supported.
- [ ] Provide an explicit fallback path where indirect-count is unavailable.
- [ ] Support two-phase occlusion where measurements justify it: previous-visible first, then newly visible after depth refresh.

### Persistent bins

- [ ] Build persistent mesh/material/pipeline bins from stable object IDs.
- [ ] Batch by shader class, render state, vertex-layout class, mesh/meshlet pass kind, and pipeline state — not by material instance.
- [ ] Keep the legacy scene/batch path only as compatibility/fallback, not the performance target.

Acceptance: CPU render submission scales with changed scene intent and pass setup, not object count.

---

## Priority 4 — Centralized Bindless Materials And Resource Tables

Photoreal scenes need many materials, textures, samplers, decals, reflection probes, lights, and per-material parameters. Per-object/per-material binding is not acceptable on the fast path.

- [ ] Define stable material IDs owned by the render/material registry.
- [ ] Store material parameters in one GPU-resident material table.
- [ ] Store texture and sampler references as bindless indices.
- [ ] Upload dirty material ranges instead of rewriting whole tables.
- [ ] Batch by material shader class and render state, not material instance.
- [ ] Add scene-wide tables for lights, decals, probes, and other frequently indexed render resources.
- [ ] Expose a bindless fast path and an explicit degraded fallback path for weaker hardware.
- [ ] Report when a material/resource feature is unavailable, degraded, or using fallback bindings.

Acceptance: adding many material instances should mostly increase data size, not pipeline count, descriptor churn, or draw-call count.

---

## Priority 5 — Production Vulkan Infrastructure

The Vulkan backend is the reference implementation. Keep it ambitious, but make capability and fallback behavior honest.

### Recording and pipelines

- [ ] Record real draw/dispatch work from pass callbacks or pass work descriptions throughout the default renderer.
- [ ] Complete Slang compiler integration and reflection-driven pipeline layout generation.
- [ ] Finish persistent pipeline cache behavior for runtime and asset-driven shader variants.
- [ ] Add pipeline cache and shader compilation diagnostics to runtime reports.
- [ ] Validate dynamic rendering, graphics pipeline library, shader objects, and fallback pipeline paths against the reference scene.

### Memory and transfers

- [ ] Use allocator-backed suballocation everywhere instead of ad hoc resource memory paths.
- [ ] Make transient render-graph resource aliasing part of the default allocation path.
- [ ] Finish upload ring/staging allocator integration for assets and dynamic data.
- [ ] Add readback paths for diagnostics, benchmark counters, screenshot/export, and GPU-generated stats.
- [ ] Track upload bandwidth and transient allocation pressure in renderer diagnostics.

### Barriers and queues

- [ ] Strengthen barrier validation for image subresources, layout transitions, and queue ownership transfers.
- [ ] Keep synchronization and queue behavior observable in graph/debug output.
- [ ] Treat backend feature use as capability-driven, not assumed.

Acceptance: higher-level renderer work should not fight memory churn, pipeline stutter, missing readbacks, or opaque synchronization failures.

---

## Priority 6 — Temporal Correctness

Temporal correctness is required before investing heavily in advanced RT, denoising, frame generation, motion blur, or sophisticated reflections.

- [ ] Use camera-local motion vectors by default.
- [ ] Maintain previous/current transform buffers for all renderable objects.
- [ ] Correctly handle skinned and animated object motion vectors.
- [ ] Separate screen-locked and camera-locked passes from world-space temporal passes.
- [ ] Support jittered projection and expose jitter state to all passes that need it.
- [ ] Track history resources explicitly in the render graph.
- [ ] Add motion-vector validation scenes and debug views.
- [ ] Validate TAA against camera cuts, disocclusion, transparency, emissives, and animated geometry.
- [ ] Add reactive mask handling for transparency/particles before temporal upscaling or frame generation depends on it.

Acceptance: TAA, exposure, bloom history, motion vectors, and future upscaling/denoising inputs are stable under camera and object motion.

---

## Priority 7 — Core Photorealism Stack

Do not add disconnected effects. Wire the existing modules into the default renderer and make them part of the measured reference scene.

### HDR, exposure, tone mapping, and bloom

- [ ] Implement real auto-exposure luminance reduction and exposure history.
- [ ] Make auto-exposure a supported runtime feature instead of a rejected config.
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
- [ ] Add parallel command recording where it reduces CPU frame time.
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

## Explicitly Deprioritized For Now

Do not spend roadmap time on these until the measured GPU-driven renderer and reference scene are stable:

- live backend/device migration
- advanced window transparency/blur polish
- full UI widget ecosystem
- neural rendering experiments
- full ray tracing visual stack
- exotic material graphs
- multi-backend parity before Vulkan is fast
- perfect editor tooling before the reference scene exists

Ray tracing should wait as a visual-feature priority until bindless resources, material tables, temporal history, denoising infrastructure, BVH/residency strategy, and render-graph scheduling are ready. Existing RT backend foundations can remain, but new RT effects should not displace the priorities above.

---

## Best Immediate Commit Sequence

1. Create `examples/realistic_reference_scene`.
2. Move the common testbed frame pipeline into `AppRuntime` / `AppRenderer`.
3. Add mandatory CPU/GPU/pass timing output to the runtime shell.
4. Add screenshot/export and graph inspection from the debug shell.
5. Implement GPU transform build from compact object buffers.
6. Replace per-batch culling with one GPU cull pass per view.
7. Wire Hi-Z into occlusion culling.
8. Add indirect-count compaction.
9. Add centralized GPU material/resource tables.
10. Finish auto exposure, mip bloom, HDR tonemap validation, and TAA validation.

This sequence keeps the engine focused on the real goal: dense, realistic scenes with measurable performance where the CPU submits compact intent and the GPU performs visibility, batching, and draw generation.
