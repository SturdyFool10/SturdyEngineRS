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
  └─ GPU-driven scene + bindless          → unlocks 100k+ draw counts
  └─ Temporal upscaling (FSR 3.1)         → makes expensive GI/RT viable at 60fps
  └─ Texture compression                  → VRAM headroom for real content
  └─ Async compute                        → free perf from queue overlap

Visual quality (dependency order)
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

Engine + ECS thread safety (foundational — do alongside GPU-driven work)
  └─ Engine::global() OnceLock accessor ✓ — any thread, zero cost
  └─ World resource system ✓ — insert_resource / resource / resource_mut / remove_resource
  └─ Fine-grained resource locks — image/buffer/pipeline/shader registries independent
  └─ Parallel command recording — ThreadRenderContext + secondary CBs per worker
  └─ Thread-safe Scene — atomic transforms, SceneCommands queue, SceneView
  └─ Parallel asset loading — load/decode/upload on rayon workers from any thread
  └─ Parallel PSO compilation — no more render-thread stutter on first material use
  └─ Parallel ECS schedule — wave-based, rayon, WorldView + WorldCommands

Physics, UI, Platform (parallel, after foundation)
```

---

## Foundation — Performance Ceiling

These items multiply the value of everything above them. Do them before adding more visual features.

### GPU-Driven Scene + Bindless (Track 8)

Without this, the CPU submits one draw call per mesh. At 10,000 objects the CPU becomes the bottleneck. With it, a single indirect dispatch handles 1,000,000 objects.

**8a — Bindless descriptor system**
- [ ] Enable `VK_EXT_descriptor_indexing`; create one large descriptor heap for all textures, samplers, and storage buffers; assign stable `u32` indices at resource creation.
- [ ] `BindlessHandle<T>`: a `u32` index valid for the resource lifetime. Binding = storing index; sampling = `textures[handle.index].sample(...)`.
- [ ] Per-material data in a single GPU-resident `StructuredBuffer<MaterialData>` indexed by `material_id`; eliminate per-draw descriptor set allocation.
- [ ] Mega-buffer draw path: each draw carries only a 4-byte push constant (index into `DrawData`); vertex shader reads transform, material ID, per-object constants from `DrawData[index]`.
- [ ] Gate bindless behind `BackendFeatures::bindless`; fall back to grouped-descriptor path.
- [ ] Validate descriptor indices in debug builds; readable error instead of GPU hang.

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
- [ ] Pipeline library at first run: compile all `UnifiedMaterial` variants to disk-cached PSOs.
- [ ] PSO pre-warm pass during loading screens; block game start until all active-scene PSOs are ready.
- [ ] `PsoWarmupReport`: compile times, cache hit rates, total variant count.

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

- [ ] At asset load time, transcode uncompressed textures to best GPU-native format: BC7 (colour SDR), BC6H (HDR/emissive), BC5 (normal maps, reconstruct Z in shader), BC4 (roughness/AO/single-channel). Use `intel-tex-rs` for CPU transcoding.
- [ ] Mobile/integrated fallback: ASTC 4×4 when BC7 is unavailable.
- [ ] `TextureDesc::prefer_compressed: bool` (default true; false for render targets and UAVs).
- [ ] Cache compressed result as `.cached/<name>.<format>.dds` next to source; invalidate on source mtime change.
- [ ] `compress_textures` CLI tool for pre-compressing asset directories in the release pipeline.

### Async Compute (Track 11b)

Overlap shadow rendering, culling, and GI updates with the previous frame's G-Buffer pass. Free performance on any GPU with a dedicated async compute queue (most discrete GPUs since 2015).

- [ ] Detect and use dedicated async compute queue; expose `QueueType::AsyncCompute` in the render graph.
- [ ] `PassDesc::queue: QueueType`; render graph compiler inserts cross-queue semaphores automatically.
- [ ] Schedule HZB build, cluster LOD selection, and GI probe updates on the async compute queue.
- [ ] DMA/transfer queue for texture decode+upload; signal semaphore on completion; consume before first shader read.
- [ ] `GpuTimeline` diagnostics: per-queue utilisation and cross-queue stall gaps.

### GPU Memory Infrastructure (Track 11a)

The block sub-allocator (256 MiB blocks) already exists. `Engine::memory_budget()` exposes VRAM stats. Remaining items:

- [ ] `BufferPool` for transient per-frame scratch (uniform uploads, staging): ring allocator in host-visible memory; sub-allocates from a single persistent block; resets at frame start. Zero allocation overhead for constant buffer updates.
- [ ] Aliased memory for G-Buffer images: the render graph already tracks lifetimes — commit the alias plan to the allocator so non-overlapping transient images share VkDeviceMemory. Saves ~50–100 MB/frame on a full G-Buffer + shadow atlas.
- [ ] Warn in console when `memory_budget().over_budget()` is true (device_local > 80%).
- [ ] Dedicated allocations for resources > 64 MiB: skip the pool and use a direct `vkAllocateMemory`; prevents a single large texture from fragmenting the whole pool.

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

- [ ] **`SystemAccess` struct**: `reads: TypeIdSet`, `writes: TypeIdSet`, `resources_read: TypeIdSet`, `resources_written: TypeIdSet`. A system that declares no access is treated as "all" (safe but serial).
- [ ] **`ParallelSystem` trait**:
  ```rust
  pub trait ParallelSystem: Send + Sync + 'static {
      fn access() -> SystemAccess where Self: Sized;
      fn run(&mut self, world: &WorldView<'_>);
  }
  ```
  `WorldView` provides read access to declared-read components and exclusive access to declared-write components, enforced by borrow-check at runtime (debug) and TypeId-aliasing safety (release).
- [ ] **Dependency graph construction** at `Schedule::build()`: for each pair of systems A, B: add a directed edge A → B if A writes what B reads, or A reads what B writes, or both write the same component. Systems with no path between them form independent nodes that can run in parallel.
- [ ] **`Schedule::build() -> CompiledSchedule`**: materialises the dependency graph into execution waves. Each wave is a set of systems with no dependencies on each other — all systems in a wave run concurrently. Waves execute in sequence.
- [ ] `Schedule::add_parallel_system(name, impl ParallelSystem)` alongside the existing `add_system`. Adding a parallel system after a wave boundary causes a new wave to start. Explicit ordering: `add_system_after("name_a", "name_b")`.

### ECS-MT-b — Parallel world access

The `WorldView` type provides safe concurrent access to component storages without requiring a `&mut World`.

- [ ] **Component storage locking**: change `components: HashMap<TypeId, Box<dyn ComponentVec>>` to `components: HashMap<TypeId, Arc<RwLock<dyn ComponentVec>>>`. The `Arc` enables sharing across threads within a frame; the `RwLock` enforces exclusive write access per component type.
- [ ] **`WorldView<'_>`**: a non-exclusive borrow of `World` that grants:
  - `view.read::<C>() -> ComponentReadGuard<C>` — shared lock on `ComponentStorage<C>`.
  - `view.write::<C>() -> ComponentWriteGuard<C>` — exclusive lock on `ComponentStorage<C>`.
  - `view.query_par::<(A, B)>(...)` — parallel iterator using `rayon::par_iter` over the intersection of A and B.
  - `view.resource::<R>()` / `view.resource_mut::<R>()` — access to type-keyed resources (see ECS-MT-c).
- [ ] **Parallel query API**: `world.query_par::<Transform>(|entity, transform| { ... })` uses `rayon::par_iter` internally, splitting the dense component array across threads. No system-level scheduling required — useful for data-parallel operations within a single system (e.g., integrating 100k transform+velocity pairs).
- [ ] **`world.spawn` / `despawn`**: these still require exclusive `&mut World`. Provide a `WorldCommands` queue — systems post deferred spawn/despawn commands; the queue is flushed between schedule waves on the main thread. This is the standard ECS command pattern.

### ECS-MT-c — Resource system ✓ (serial path complete)

"Resources" are singleton values in the World (not component data) — the engine reference, the time step, the event queues.

- [x] **`World::insert_resource<R: Send + Sync + 'static>(value: R)`**: stores under `TypeId`; replaces any existing value of the same type.
- [x] **`World::resource<R>() -> Option<&R>`** / **`resource_mut<R>() -> Option<&mut R>`** / **`remove_resource<R>() -> Option<R>`** / **`has_resource<R>() -> bool`**: full serial access API. 7 tests covering all paths.
- [x] **`World::resource_unwrap<R>()`** / **`resource_unwrap_mut<R>()`**: panicking variants with type name in the message.
- [ ] **`WorldView::resource::<R>()` / `resource_mut::<R>()`**: thread-safe access from parallel systems, with access declared in `SystemAccess`. Requires ECS-MT-a (WorldView).
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

- [ ] Add `rayon` as a dependency. Use `rayon::ThreadPoolBuilder::new().num_threads(num_cpus::get().saturating_sub(1))` — reserve one core for the render thread and OS.
- [ ] **`CompiledSchedule::run(&self, world: &World)`**: iterate waves sequentially. Within each wave, use `rayon::scope` to launch one task per system. Each task receives a `WorldView` (shared borrow with per-component `RwLock`s) and a `&mut WorldCommands`. The scope returns when all tasks in the wave complete.
- [ ] **Timing diagnostics**: `CompiledSchedule::debug_timing: bool` records wall time per system and reports wave-level parallelism efficiency (actual elapsed / sum of system times).
- [ ] **Backward compatibility**: `Schedule::run(&mut world)` continues to work. `CompiledSchedule` is opt-in via `Schedule::build()`.

### ECS-MT-f — WorldCommands (deferred structural mutations)

- [ ] `WorldCommands` is a lock-free append-only buffer per system (one per wave slot). Commands: `Spawn { components }`, `Despawn { entity }`, `Insert { entity, component }`, `Remove { entity, type_id }`.
- [ ] Parallel systems receive `&mut WorldCommands` alongside `WorldView`. After all systems in a wave finish, the main thread applies commands to `World` before starting the next wave.
- [ ] `WorldCommands::spawn() -> EntityBuilder<'_>` — entity ID allocated immediately from an atomic counter on `EntityAllocator`; components recorded into the buffer. The entity exists in the world only after the wave's command flush.

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
- [ ] `VK_EXT_mesh_shader`: device extension when detected; `EXT_mesh_shader` bits in `BackendFeatures`.
- [ ] `VK_AMDX_shader_enqueue`: `BackendFeatures::work_graphs`. Port cluster LOD to Work Graph.
- [ ] `vkCmdDrawIndexedIndirectCount` (Vulkan 1.2) for GPU-written draw counts.
- [ ] `VK_EXT_device_fault`: structured crash report on device-lost.
- [ ] Buffer device address (`VK_KHR_buffer_device_address`): `Buffer::device_address() -> u64` for bindless.
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
