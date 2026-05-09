# Sturdy Engine Roadmap

## Product Direction

Sturdy Engine is worth using in three modes:

1. **Shader playground** — open a window, write Slang, see it run, tweak parameters live.
2. **Graphical apps and custom UI** — standalone tools, dashboards, inspectors, editors.
3. **Games** — including a path toward footage that can plausibly read as real life.

The simple path must be the best path, not a toy path. Each mode should feel complete without requiring the user to build the runtime shell themselves. The architecture must scale from simple to deep without rewrites.

### What's working today

**Core infrastructure**: Vulkan backend with precise pipeline barriers, 2-frame-in-flight command contexts, pool-slab descriptor allocation, O(n) pass scheduling, and incremental pipeline cache saves. The render graph compiles passes, infers dependencies, and submits without CPU stalls. Shader reflection derives bind groups, validates resource usage, and exposes vertex inputs. GPU timestamp queries per pass.

**ECS**: Generational entity/component system with sparse-set storage, multi-component queries, transform hierarchy, built-in components (Transform, Velocity, Health, SceneLink, Name), and a Schedule runner. Fully tested.

**Game loop shell**: `FrameClock`, `InputHub` (keyboard/mouse/gamepad), `ActionMap`, fixed-timestep accumulator, pointer-lock. `GameApp` + `run_game` zero-config shell. `HeadlessApp` + `run_headless` for windowless compute. 2D and 3D game samples.

**Deferred PBR pipeline**: G-Buffer fill (albedo/normal/roughness/emissive), GGX specular (Trowbridge-Reitz NDF, Smith G2, Schlick Fresnel), Lambertian diffuse, split-sum IBL with SH9 diffuse, BRDF LUT, multi-scattering (Kulla-Conty). Cascaded shadow maps (4 cascades, PCF, texel-snap). Spot light shadow maps (up to 4, PCF). Point light shadow maps (up to 4, dual-paraboloid). BVH-culled point/spot/rect/sphere/disk lights. OIT (Per-Pixel Linked List). Forward-only path. Procedural sky (Rayleigh + Mie). Environment map blending. Normal mapping.

**Unified material system**: `MaterialSurface` shared module, `UnifiedMaterial` with expression trees, `GBufferFillVariant` shader codegen, `ForwardLitVariant`, `ShadowVariant` (with alpha test). Hot reload for shaders and assets with last-known-good semantics.

**Asset pipeline**: PNG/JPEG/WebP/BMP/TGA/TIFF/HDR/EXR textures, GLTF 2.0 (full PBR + extensions), OBJ, STL. Automatic mip generation. Tangent generation. `AssetHandle<T>` with state queries. Checkerboard fallback for missing textures. `AssetWatcher` for hot reload.

**Geometry backends**: `ClassicVertex`, `ComputeIndirect` (CPU frustum culling + indirect draw), `VirtualMesh` asset type with meshlet generation (via meshopt), RT proxy simplification, `VirtualMeshBuilder`. Indirect draw/dispatch variants in render graph.

**Shader playground**: Full auto-UI from reflection, slider/toggle/text fields, presets, screenshot export, `ShaderPlayground` wrapper.

**Sprites, 2D, text**: `SpriteBatch`, `DebugDraw2d`, text rendering (shaping, atlas, tiling). Clay UI bindings.

**Post-processing**: Bloom, TAA, FXAA, MSAA, tone mapping. Mip pyramid ops with graph validation.

**Platform**: Surface-lost and device-lost recovery, zero-size window suspension.

---

## API Design Contract

**Every subsystem must work perfectly with zero configuration and expose every dial when the user wants one.**

Every major subsystem ships a `*Config` struct where `Default::default()` produces a production-quality result. Every field is `pub`, documented with its valid range and trade-off. The user never opens a Config to get a good game; they open it only when defaults are wrong for their specific case.

Breaking this contract is a bug. Adding a knob only accessible by editing source code is not allowed.

---

---

## Track 2 — Reference Scene

- [ ] Add a reference scene that stresses lighting, shadows, and materials with realistic content.

---

## Track 6 — Remaining Rendering

### 6b — Shader variant compiler

- [ ] **RT variants**: `RtAnyHitVariant`, `RtClosestHitVariant`, `PathTracedVariant`.
- [ ] Cache invalidation on shader hot reload.

### 6e — Shadow system

- [ ] **Shadow map atlas**: pack all shadow maps into a single atlas. `ShadowAtlasConfig { atlas_resolution: u32 [1024, 16384], page_size: u32, max_cached_pages: u32 }`.
- [ ] **PCSS** (Percentage-Closer Soft Shadows): variable-width PCF based on blocker search distance. `ShadowConfig::pcss: bool` (default false) + `PcssConfig { blocker_search_samples: u32, pcf_samples: u32, light_size_world: f32 }`.
- [ ] **Optional RT shadows**: hardware RT shadow rays replacing PCF. `ShadowConfig::rt_shadows: RtShadowMode` (Off / DirectionalOnly / All) with graceful fallback to CSM.

### 6g — Real-time ray tracing

- [ ] Build and maintain a TLAS from all opaque mesh instances each frame; refit on transform change, rebuild on add/remove.
- [ ] `RtShadowPass`: trace shadow rays from G-Buffer surface points toward the primary directional light; write shadow mask; composite with deferred lighting.
- [ ] `RtReflectionPass`: trace reflection rays from G-Buffer; evaluate `RtClosestHitVariant`; blend into deferred specular using roughness-based fade.
- [ ] `RtAmbientOcclusion`: short-range hemisphere rays from G-Buffer.
- [ ] `RtFeatures` flags on `SceneRenderer`: `SHADOWS`, `REFLECTIONS`, `AMBIENT_OCCLUSION`; graceful fallback when RT is unavailable.
- [ ] Path-traced reference renderer: progressive accumulation via `PathTracedVariant`, Russian roulette, NEE.

---

## Track 4 — Layout engine and widget system

The text system, input callbacks, and Clay UI bindings exist, but there is no layout engine. This blocks the graphical-apps use case.

- [ ] Integrate `taffy` (pure-Rust flex/grid layout). Map widget descriptors to taffy nodes, run layout each frame, produce screen-space rectangles.
- [ ] Build a `ScreenUiRoot`: layout tree, input dispatcher, focus scope, render pass.
- [ ] Core widgets: `Label`, `Button`, `TextInput`, `Checkbox`, `Toggle`, `Slider`, `ScrollRegion`, `Panel`, `Tabs`.
- [ ] Stable widget IDs, focus scopes, modal scopes, per-frame retained state.
- [ ] Root-level input routing: keyboard, mouse, scroll, pointer capture, text input ownership.
- [ ] Theme tokens: typography scale, spacing scale, radii, semantic colors, state colors.
- [ ] `WorldUiRoot` for UI on world-space panels with ray-to-panel hit testing and render-to-texture.
- [ ] `TextureUiRoot` for UI rendered into named graph images.
- [ ] Standalone app conveniences: menu bars, status bars, toolbars, resizable panes, tabbed documents, inspector panels.
- [ ] Persistent UI state helpers: window geometry, dock layout, scroll position, selection.
- [ ] Accessibility tree: roles, names, descriptions, values, bounds, focus, selection, actions.

### Text system completeness

- [ ] Grapheme-aware cursor movement, word movement, bidi movement, selection across wrapped lines.
- [ ] Single-line editable text field with cursor, selection, focus, clipboard, keyboard navigation.
- [ ] Multiline editable text with scrolling, grapheme-aware selection, IME composition, platform clipboard.
- [ ] Fallback fonts, emoji, combining marks, ligatures, OpenType features.
- [ ] SDF/MSDF rendering for large scalable text and world-space text.
- [ ] Atlas residency, eviction, dirty-rectangle upload policies.
- [ ] Text performance counters: shaping time, atlas uploads, cache hit/miss, memory use per frame.

---

## Track 7 — Unified geometry front-end (mesh shaders + virtual mesh)

The geometry front-end decides which triangles reach the rasterizer. This track replaces the single draw-call path with a pluggable abstraction. Material/pixel shading is decoupled — all backends feed the same G-Buffer, depth, shadow, and visibility-buffer attachments.

### 7a — VirtualMesh asset type

- [ ] Add LOD group DAG: partition meshlets into groups with shared boundaries; record parent error metrics; enable `VirtualizedRaster` backend (Nanite-style LOD selection).

### 7b — Render graph indirect work variants

- [ ] Vulkan backend: emit `vkCmdDrawMeshTasksIndirectEXT` (EXT_mesh_shader) for `PassWork::DrawMeshShaderIndirect`; check capability; emit error/no-op if missing.
- [ ] Add render graph validation: indirect buffer must be in `RgState::IndirectRead` before dispatch.
- [ ] Expose `DrawMeshShaderDesc` through `GraphImage` pass API analogous to `draw_mesh_instanced`.

### 7c — Classic + compute-indirect path

- [ ] Add `HizPass`: compute shader building a mip pyramid from the previous frame's depth buffer; feed into the culling shader for occlusion rejection.

### 7d — Mesh shader path

- [ ] Add `MeshShaderPipelineDesc`: task_shader (optional) + mesh_shader + fragment_shader + layout; no vertex input layout.
- [ ] Vulkan pipeline creation for `MeshShaderPipelineDesc`: require `VK_EXT_mesh_shader`.
- [ ] Built-in task shader `task_cull.slang`: frustum + backface-cone + Hi-Z tests per meshlet; emit surviving workgroup indices.
- [ ] Built-in mesh shader `mesh_emit.slang`: decompress local indices; write position/normal/UV; emit triangle list.
- [ ] `GeometryRenderer::mesh_shader(mesh, caps)` selects `MeshShader` if available, falls back to `ClassicVertex`.
- [ ] Feed mesh-shader path into same G-Buffer, depth, and shadow attachments as the classic path.

### 7e — Virtual raster path (Nanite-like)

- [ ] Cluster hierarchy traversal compute shader `cluster_lod_select.slang`: walk `MeshletGroup` DAG; write selected meshlet indices.
- [ ] Thread selected cluster buffer into task shader.
- [ ] `ClusterPage` streaming: meshlet data in fixed-size pages; per-frame LOD cut drives residency; missing pages trigger async load.
- [ ] `VirtualGeometryStats` diagnostics: drawn clusters/triangles, culled clusters, LOD histogram, streaming page hits/misses.

### 7f — Ray tracing integration

- [ ] `TlasBuilder`: TLAS from all `VirtualMesh` RT proxies; refit on transform changes, rebuild on add/remove.
- [ ] `BlasBuildPass`: build or refit BLAS from `VirtualMesh::rt_proxy` vertex/index data.
- [ ] `GeometryBackend::RayTracingFallback` through RT shadow and reflection passes (Track 6g).
- [ ] `GeometryBackend::RayTracingSelectedClusters`: BLAS from current frame's selected cluster subset for high-quality near-camera geometry.

### 7g — Mix-and-match per pass

- [ ] `RenderPassBackendOverride` per scene-renderer pass: `MeshShader` for G-Buffer, `ClassicVertex` for shadow maps, `RayTracingFallback` for RT reflections — all from the same `VirtualMesh` assets.
- [ ] `SceneRenderer::set_backend(pass, backend)` with validation against `GeometryRendererCaps`.
- [ ] Document supported combination matrix.
- [ ] Testbed mode: cycle through available backends on keypress with diagnostic overlay (triangle counts, culling stats, timing).

---

## Track 8 — Full Bindless + GPU-Driven Architecture

### 8a — Bindless descriptor system

- [ ] Enable `VK_EXT_descriptor_indexing`; create one large descriptor heap for all textures, samplers, and storage buffers; stable `u32` indices at resource creation.
- [ ] `BindlessHandle<T>`: a `u32` index valid for the resource lifetime. Binding = storing index; sampling = `textures[handle.index].sample(...)`.
- [ ] Per-material data in a single GPU-resident `StructuredBuffer<MaterialData>` indexed by `material_id`; eliminate per-draw descriptor set allocation.
- [ ] Mega-buffer draw path: each draw carries only a 4-byte push constant (index into `DrawData`); vertex shader reads transform, material ID, per-object constants from `DrawData[index]`.
- [ ] Gate bindless path behind `BackendFeatures::bindless`; fall back to grouped-descriptor path.
- [ ] `BindlessTextureAtlas`: allocate texture array slices and 2D atlas regions; stable `u32` slice indices.
- [ ] Validate descriptor indices in debug builds; readable error instead of GPU hang.

### 8b — Fully GPU-driven scene submission

- [ ] GPU scene buffer: one `GpuInstanceData` per scene object (model matrix, AABB, LOD bias, material ID, visibility flags); upload once on change.
- [ ] Single GPU compute dispatch for frustum culling + HZB occlusion; writes `DrawIndexedIndirectCommand` per visible instance.
- [ ] `vkCmdDrawIndexedIndirectCount`: GPU-written draw count drives actual draw count, no CPU readback.
- [ ] Two-phase occlusion culling: Phase 1 renders last frame's visible set; Phase 2 re-tests newly unoccluded objects against fresh depth buffer.
- [ ] `GpuDrivenScene` as drop-in replacement for `Scene`; same `VirtualMesh` assets and `UnifiedMaterial` definitions.

### 8c — Variable Rate Shading (VRS)

- [ ] Wire `VK_KHR_fragment_shading_rate` (already detected) into the render path.
- [ ] Tier 1 VRS: per-draw shading rate (1×1, 1×2, 2×1, 2×2) for screen-edge and low-motion regions. Target: 20–30% shading cost reduction.
- [ ] Tier 2 VRS: mesh/task shader writes `SV_ShadingRate` per primitive.
- [ ] VRS image generated from motion vectors + luminance gradient each frame.
- [ ] Disable VRS on the tonemap pass; only inside G-Buffer and deferred lighting.

### 8d — GPU Work Graphs (when available)

- [ ] Detect `VK_AMDX_shader_enqueue`; expose `BackendFeatures::work_graphs`.
- [ ] Port cluster LOD selection + mesh shader dispatch (Track 7e) to a Work Graph.
- [ ] Prototype material resolve Work Graph: visibility buffer → material-specific shading nodes per tile.
- [ ] Keep classical indirect dispatch as fallback.

### 8e — Shader pipeline precompilation + PSO caching

- [ ] Pipeline library at first run: compile all `UnifiedMaterial` variants to disk-cached PSOs; subsequent launches load from cache.
- [ ] PSO pre-warm pass during loading screens: enumerate required pipelines, compile on shader workers, block game start until ready.
- [ ] `PsoWarmupReport`: compile times, cache hit rates, total variant count.
- [ ] `slangc` ahead-of-time compiled cache in release builds; runtime recompilation dev-only.

---

## Track 9 — Advanced Global Illumination

### 9a — Virtual Shadow Maps (VSM)

- [ ] Virtual shadow atlas (`R32Float`, 16384×16384 logical, 128×128 resident pages); page table mapping (light, mip, tile) → physical page.
- [ ] Per-frame: analyse depth buffer for visible pages; mark dirty when lights/casters move; only render dirty pages.
- [ ] Sample VSM atlas from deferred lighting with hardware PCF or PCSS kernel.
- [ ] Support 16+ simultaneous shadow sources in the same atlas.
- [ ] Gate behind `ShadowTechnique::Virtual`; fall back to CSM.

### 9b — ReSTIR Direct Illumination (ReSTIR DI)

- [ ] Initial candidate sampling: per pixel, generate N light candidates by importance sampling; store as `RIS_Reservoir`.
- [ ] Temporal reuse: reuse previous frame's reservoir at reprojected pixel; merge with current candidates.
- [ ] Spatial reuse: share reservoirs with neighbouring pixels (4–8 taps); target function for bias correction.
- [ ] Visibility via RT shadow rays for accepted samples; output `DirectLightSample` image feeding deferred lighting.
- [ ] Expose `GiFeatures::RESTIR_DI`; fall back to clustered analytic lights when RT unavailable.

### 9c — ReSTIR Global Illumination (ReSTIR GI)

- [ ] Trace secondary rays from G-Buffer at 1 ray/pixel; store hit radiance + position as initial GI reservoir.
- [ ] Temporal and spatial reuse.
- [ ] BRDF-weighted target function.
- [ ] Output denoised indirect diffuse; composite: `total = direct + indirect_diffuse + indirect_specular`.
- [ ] Expose `GiFeatures::RESTIR_GI`; degrade to SSGI when RT unavailable.

### 9d — ReSTIR Path Tracing (ReSTIR PT)

- [ ] Extend `PathTracedVariant` with ReSTIR PT reservoirs storing full path prefixes.
- [ ] Reconnection shift for merging reservoir paths.
- [ ] Hybrid shift mapping for visibility-sensitive bounces.
- [ ] Real-time output (1–4 paths/pixel) + high-SPP progressive accumulation mode.
- [ ] Gate behind `GiFeatures::RESTIR_PT`; requires RT hardware + sufficient VRAM.

### 9e — Probe-based and surfel GI (Lumen-style)

- [ ] World-space irradiance probe grid (adaptive octree, ~2m probe spacing); each probe stores SH9 or octahedral irradiance.
- [ ] Update probes via RT rays per frame (256–512 rays/probe, amortised over 4–8 frames).
- [ ] Probe validity masks: invalidate when nearby geometry changes.
- [ ] Sample probe grid for indirect diffuse; blend with ReSTIR GI using screen-age weight.
- [ ] Surfel GI: project G-Buffer pixels to world-space surfels; accumulate incident radiance; read back in lighting pass. Works without RT hardware.
- [ ] Expose `GiFeatures::PROBE_GRID`, `GiFeatures::SURFEL`.

### 9f — AI denoising

- [ ] Integrate **Intel Open Image Denoise (OIDN)**: CPU or GPU (SYCL); noise-free from 1–4 spp path-traced frames.
- [ ] In-engine temporal denoiser for ReSTIR: SVGF; target stable 1-spp ReSTIR GI at 4K/60 fps.
- [ ] `DenoiserMode::Temporal` (real-time) and `DenoiserMode::OIDN` (offline); select automatically.

---

## Track 10 — Temporal Upscaling and Frame Generation

Only open-source, Vulkan-native frameworks are considered.

### 10a — FSR 3.1 (primary)

- [ ] Integrate FSR 3.1 upscaling via AMD FidelityFX SDK: motion vectors, exposure, depth, colour, camera jitter → upscaled output.
- [ ] Integrate FSR 3 frame generation: interpolate an additional frame using optical flow.
- [ ] Expose `FsrConfig { quality: FsrQualityMode, sharpness: f32, mip_lod_bias: f32, auto_exposure: bool, frame_gen: bool, ... }`. `Default` selects Quality mode, frame gen on, auto-exposure on.
- [ ] Detect camera cuts/teleports via velocity discontinuity; pass reset flag automatically.

### 10b — XeSS 2.x (fallback)

- [ ] Integrate XeSS 2.x via Intel open SDK: XMX path for Arc, DP4a for all other GPUs.
- [ ] Expose `XessConfig { quality: XessQualityMode, sharpness: f32, use_jitter: bool, ... }`.

### 10c — Unified upscaler interface

- [ ] `UpscalerConfig::auto()`: XeSS XMX (Intel Arc) → FSR 3.1 (all others). Frame gen enabled by default.
- [ ] `render_resolution(display_resolution, quality)` from active mode; render targets allocate at render resolution.
- [ ] Tone mapping runs after the upscaler on full-resolution output.
- [ ] `UpscalerReport` in `GraphReport`: active upscaler, render/display resolution, upscale ratio, frame gen active, latency estimate.
- [ ] Auto reactive mask from transparent + particle alpha.

---

## Track 11 — GPU Memory and Performance Infrastructure

### 11a — Sub-allocation and memory budgeting

- [ ] Replace per-resource `vkAllocateMemory` with VMA-backed sub-allocator: heap per memory type, 256 MiB blocks.
- [ ] `GpuMemoryBudget` query per frame: available/used VRAM, host-visible used; warn at 80%.
- [ ] `BufferPool` for transient per-frame scratch: ring allocator in host-visible memory.
- [ ] Aliased memory for G-Buffer images: single `vkDeviceMemory` with explicit alias ranges; reclaim at end of G-Buffer pass.

### 11b — Async compute and multi-queue

- [ ] Dedicated async compute queue; expose `QueueType::AsyncCompute` in the render graph.
- [ ] Schedule HZB build, cluster LOD selection, ReSTIR update on async compute queue.
- [ ] DMA/transfer queue for texture decode+upload; signal semaphore when complete; consume before first pass that reads.
- [ ] `PassDesc::queue: QueueType`; compiler inserts cross-queue semaphores automatically.
- [ ] `GpuTimeline` diagnostics: per-queue utilisation and cross-queue stall gaps.

### 11c — GPU crash and performance diagnostics

- [ ] Integrate NVIDIA Aftermath (optional, feature-flagged): GPU crash dumps on device-lost, breadcrumbs in command buffers.
- [ ] AMD RGP markers: `vkCmdBeginDebugUtilsLabelEXT` per render graph pass.
- [ ] Frame graph inspector UI (debug overlay): pass DAG, resource lifetimes, barrier counts, queue assignment, per-pass GPU timing.

### 11d — Texture compression pipeline

- [ ] At asset load time, transcode uncompressed textures to BC7 (colour), BC6H (HDR), BC5 (normals), BC4 (grayscale) using `intel-tex-rs` or `basis-universal`.
- [ ] Mobile fallback: ASTC 4×4 or ETC2.
- [ ] `TextureDesc::prefer_compressed: bool` (default true except render targets and UAVs).
- [ ] Cache compressed result next to source (`.cached/texture_name.bc7.dds`); invalidate on source change.
- [ ] `compress_textures` CLI tool for pre-compressing asset directories.

---

## Track 12 — Hair, Particles, and Procedural FX

### 12a — Strand-based hair rendering

- [ ] `HairStrand` asset: cubic Bézier control points, root UV on scalp mesh, per-strand material (base color, roughness, melanin, cuticle scale, transmission).
- [ ] Hair rasterization via `MeshShader` backend: task shader selects by frustum + screen-size; mesh shader tessellates segments into oriented quads or cylinders.
- [ ] Marschner / d'Eon hair BSDF (R, TT, TRT); expose `ShadingModel::Hair` in `brdf.slang`.
- [ ] Integrate hair with deferred pipeline: forward-lit pass after opaque G-Buffer; reads deferred lighting output.
- [ ] Deep-opacity maps for self-shadowing in hair volumes.
- [ ] GPU-driven simulation: position-based dynamics or Cosserat rod model on async compute queue.

### 12b — GPU-driven particle system

- [ ] `ParticleEmitter` with `spawn_rate`, `lifetime_range`, velocity, drag, `color_over_lifetime`, `size_over_lifetime`, `rotation_over_lifetime`. `Default` gives a basic burst emitter.
- [ ] GPU compute simulation per frame: update/integrate/kill/emit. All state GPU-resident; zero CPU readback.
- [ ] Sub-step simulation: `ParticleSystemConfig::substeps: u32` (default 1; up to 4).
- [ ] Render via `MeshShader`: task frustum-culls clusters; mesh shader emits billboards or `VirtualMesh` instances per particle.
- [ ] `ParticleRenderMode`: Billboard, VelocityStretch, AxisAligned, Mesh, Ribbon.
- [ ] `ParticleForceField`: point attractor/repeller, wind, vortex, turbulence noise. GPU-resident.
- [ ] Vector field forces: 3D `R16G16B16A16_Float` velocity texture; particles advect each frame.
- [ ] Collision: scene SDF or analytic shapes. `ParticleSystemConfig::collision_mode: CollisionMode`.
- [ ] `ParticleSystemConfig` with full dials. `Default` gives unlit billboards, BackToFront sorting, no collision.

### 12c — Decal system

- [ ] Decal asset: up to five texture channels (albedo_alpha, normal, roughness, metallic, emissive); absent channels leave G-Buffer untouched.
- [ ] OBB projection: test lit pixel world position against decal OBBs; compute UV in decal local space.
- [ ] GPU clustering: assign decals to the same 3D frustum cluster grid as point lights.
- [ ] Reoriented Normal Mapping (RNM) blend for normals.
- [ ] Fade parameters: `DecalConfig { depth_fade_start, depth_fade_end, angle_fade_start_cos, angle_fade_end_cos }`.
- [ ] Per-channel write mask: `DecalConfig::write_mask: DecalWriteMask`.
- [ ] Priority and layering: integer priority for cluster ordering.
- [ ] Mesh decals (baked, UV-space): forward-lit sub-mesh blended on top using alpha.
- [ ] `DecalConfig::default()`: full-channel write, 5 cm depth fade, 45° angle fade, priority 0.

---

## Track 13 — Visibility Buffer

- [ ] Render all opaque geometry into `visibility_buffer` (`R64Uint` or `R32Uint×2`): encode `(instance_id << 32 | primitive_id)` per pixel.
- [ ] Material resolve compute pass: decode `(instance_id, primitive_id)` → interpolate barycentrics → evaluate `UnifiedMaterial` → write G0/G1/G2.
- [ ] `RenderPath::VisibilityBuffer` alongside `DeferredThenForward` and `ForwardOnly`.
- [ ] Fallback for hardware without mesh shaders: compute pass emulating visibility buffer rasterization.
- [ ] Combine with VRS: visibility pass at full rate; material resolve pass uses VRS image.

---

## Track 14 — GPU Physics

Cross-platform GPU physics using Vulkan compute — no CUDA, no vendor lock-in. Simulation runs on the async compute queue; physics world is GPU-resident. Based on Extended Position-Based Dynamics (XPBD).

### 14a — Core XPBD solver

- [ ] XPBD integration loop in Slang compute: predict → solve constraints (Gauss-Seidel with graph-coloured islands) → update velocities.
- [ ] Configurable substeps: `PhysicsWorldConfig::substeps: u32` (default 4; up to 20).
- [ ] Broad-phase collision: GPU LBVH rebuilt each frame (Morton code sort, O(n log n)).
- [ ] Narrow-phase: GJK/EPA for convex-convex; SAT for box-box and sphere-box; sphere-sphere analytic. Contact manifolds on GPU.
- [ ] Contact constraints: non-penetration + Coulomb friction. `PhysicsMaterial { friction, restitution, density }`.
- [ ] `PhysicsWorldConfig` with full dials: gravity, substeps, solver_iterations, contact_offset, sleep_threshold, max_bodies, max_contacts.

### 14b — Rigid body dynamics

- [ ] `RigidBody`: mass, inertia tensor, angular/linear damping, sleeping, kinematic flag.
- [ ] Collision shapes: Sphere, Box, Capsule, ConvexHull, TriangleMesh (static/kinematic only).
- [ ] Compound shapes: multiple `CollisionShape`s with local offsets.
- [ ] Joints: FixedJoint, BallJoint, HingeJoint, SliderJoint, SpringJoint. All as XPBD constraints.
- [ ] `RigidBodyConfig` with full dials: mass, inertia override, damping, kinematic, CCD, gravity_scale.

### 14c — Soft body and cloth

- [ ] XPBD soft body: tetrahedral mesh; distance, volume, shape-matching constraints. `SoftBodyConfig`.
- [ ] XPBD cloth: stretch, shear, bending constraints. `ClothConfig { stretch_stiffness, shear_stiffness, bend_stiffness, damping, thickness, wind_drag, wind_lift }`.
- [ ] Cloth self-collision via GPU spatial hash.
- [ ] `PinConstraint`: attach cloth/soft bodies to rigid bodies.
- [ ] Hair simulation in Track 12a reuses the cloth strand solver.

### 14d — GPU fluid simulation (SPH)

- [ ] SPH: density, pressure, viscosity, surface tension. GPU-resident, async compute.
- [ ] Spatial hashing for neighbour search; rebuilt each substep.
- [ ] `FluidConfig` with full dials: rest_density, stiffness, viscosity, surface_tension, max_particles, particle_radius, substeps.
- [ ] Surface extraction: marching cubes or screen-space fluid rendering. `FluidRenderMode`.
- [ ] Fluid↔rigid body two-way coupling.

### 14e — Scene query API

- [ ] GPU-accelerated raycast via GPU BVH; async (result next frame) or sync. `RaycastHit`.
- [ ] Sphere cast, box cast, shape overlap on same BVH.
- [ ] Trigger volumes: per-body enter/stay/exit events from GPU, delivered via compact event buffer each frame.
- [ ] `PhysicsQueryConfig`: max_results_per_query, filter_mask, async flag.

### 14f — Physics ↔ rendering integration

- [ ] `PhysicsBody::visual_mesh()`: physics body drives `VirtualMesh` transform via GPU compute writing directly into `GpuInstanceData` buffer.
- [ ] `PhysicsWorld::debug_draw(frame)`: wireframe collision shapes using `DebugDraw2d` extended to 3D. Off by default.
- [ ] Deterministic mode: `PhysicsWorldConfig::deterministic: bool`; fixed-point arithmetic, canonical island ordering.
- [ ] Export physics state as binary snapshot; import to restore.

---

## Track 15 — Area Lights, Emissive Surfaces, and Physically Based Luminaires

### 15a — LTC area lights (raster path)

- [ ] Precompute and ship `ltc_matrix.dds` (64×64 RGBA32Float) and `ltc_amplitude.dds` (64×64 RG32Float).
- [ ] `ltc_evaluate_rect`, `ltc_evaluate_disk`, `ltc_evaluate_sphere` in `brdf.slang`.
- [ ] `LightType::Rect`, `LightType::Disk`, `LightType::Sphere`, `LightType::Tube` added to light type enum.
- [ ] Assign area lights to cluster grid exactly as point/spot lights.
- [ ] `AreaLightConfig` with full dials: luminous_flux, color, temperature_k, two_sided, specular_only, diffuse_only.

### 15b — Emissive mesh lights

- [ ] Auto-register `UnifiedMaterial` with non-zero emissive channel as an emissive light source (approximated rect area light from AABB).
- [ ] Emissive texture drives both G-Buffer emissive and area light colour (`average_emissive_texel × emission_strength`).
- [ ] `EmissiveConfig::source: EmissiveSource` — Constant, Texture, VideoStream.
- [ ] Raster: treat as `LightType::Rect` evaluated via LTC.
- [ ] RT: add to ReSTIR DI light pool; NEE surface sampling proportional to luminous flux.
- [ ] `EmissiveConfig` with full dials: emission_strength, source, cast_light, light_sample_count, area_light_override.

### 15c — IES photometric profiles and flood lights

- [ ] `IesProfile` asset: load `.ies` file; parse candela distribution; upload as `R16Float` texture.
- [ ] Apply IES profile as multiplicative attenuation on spot or area light.
- [ ] `SpotLightConfig::ies_profile` and `AreaLightConfig::ies_profile`.
- [ ] `FloodLight`: high-power spot or rect area with IES profile, colour temperature (2700K–6500K), luminous_intensity (cd), cookie texture.
- [ ] `FloodLightConfig` with full dials.

### 15d — Light units and photometric pipeline

- [ ] Accept LuminousFlux (lm), Luminance (nits), LuminousIntensity (cd), Illuminance (lux). Convert to scene-linear radiance internally.
- [ ] `PhysicsBasedLightConfig::exposure_compensation: f32` (EV offset).
- [ ] Scene exposure correctly scales physically-specified lights.
- [ ] `LightDebugOverlay`: light ranges, cluster assignment, LTC polygon outlines, per-light cost.

### 15e — Performance scaling to thousands of lights

- [ ] For raster with > 1024 area lights: second-tier cluster using a 64×64×32 light grid, built on async compute.
- [ ] For RT: integrate area lights into ReSTIR DI light pool alongside point lights.
- [ ] Light proxy culling: each area/emissive light registers a bounding sphere for broad-phase cluster rejection.
- [ ] Power-proportional importance sampling for RT.

---

## Rendering Quality

### Post-processing pipeline

- [ ] Generalise bloom, AA, and tone mapping into a proper post stack that can host exposure, bloom, temporal effects, sharpening, grading, film grain, lens effects in any order.
- [ ] Stronger temporal AA using real motion vectors, camera jitter, transparency-heavy scenes.
- [ ] Transparency-heavy validation scene for temporal and post effects.
- [ ] Motion-blur validation: camera-local vectors produce stable blur; moving objects blur correctly; camera-locked overlays do not blur.

### Photoreal rendering path

- [ ] **Subsurface scattering (SSS)**: separable SSS via screen-space irradiance blur weighted by profile. `ShadingModel::PbrSubsurface`.
- [ ] **Anisotropic specular**: anisotropy direction + magnitude in `MaterialSurface`; Ashikhmin-Shirley or GGX anisotropic VNDF.
- [ ] **Clearcoat layer**: `ShadingModel::PbrClearcoat`; second GGX lobe at fixed 0.04 F0.
- [ ] **Transmission and volume**: `ShadingModel::PbrTransmission`; full glTF KHR_materials_transmission and KHR_materials_volume.
- [ ] **Screen-space global illumination (SSGI)**: short-range indirect diffuse via screen-space ray marching.
- [ ] **Volumetric fog and atmosphere**: frustum-voxel density grid; in-scattering from directional + local lights; exponential height fog fallback.
- [ ] **Layered surface workflows**: material layer stack (base + clearcoat + fuzz) flattened by the compiler.
- [ ] **Wet and glossy surface path**: runtime wetness mask modulating roughness + darkening base color.
- [ ] Build a reference scene for realistic output; evaluate against path-traced ground truth.

### 2D and instanced rendering

- [ ] Tilemap and simple layered-scene helpers.
- [ ] Examples for many instanced quads, per-instance colour/material parameters, animated GPU-updated instance data.
- [ ] Effect-oriented instancing: layered glow sprites, particles.

---

## Full Asset Pipeline

- [ ] `ContentRuntime`: asset requests, handles, background I/O, decode/transcode workers, upload plans, residency state, diagnostics.
- [ ] Stable asset handles across load, reload, failure, eviction, revalidation.
- [ ] Staged pipeline: Requested → Reading → Decoded → Transcoded → UploadQueued → GpuResident → Ready → Degraded → Failed → Evicted.
- [ ] Texture streaming: tiny fallback mip immediately, progressive high-mip refinement, budget eviction.
- [ ] Per-frame upload budgeting: bytes/frame, images/frame, staging memory, transfer queue time.
- [ ] Staging buffer/ring allocator for async uploads.
- [ ] Compressed texture policy: prefer GPU-native formats, transcode in workers.
- [ ] Content priority and cancellation: visible-now, near-future prefetch, UI-critical, low-priority, cancelled/stale.
- [ ] Development loose-file mode and release package mode behind the same virtual asset paths.
- [ ] Asset hot reload using the same handle/state system as the streaming path.

### I/O backends

- [ ] Linux: prefer `io_uring` when available, fall back to blocking I/O thread pool.
- [ ] Windows: prefer DirectStorage where it fits the Vulkan asset pipeline, fall back to overlapped/thread-pool I/O.
- [ ] Browser/WebAssembly: use browser fetch primitives with same asset-handle API.

---

## Multi-Window, Workspace, and Docking

- [ ] `WindowRegistry` / `WindowManager` with generation-checked `WindowHandle`s.
- [ ] `WindowDesc` and route all window creation/destruction through event-loop command queue.
- [ ] Per-window surface, swapchain, present mode, frame pacing, DPI/safe-area, cursor state, IME state, compositor effects.
- [ ] `FrameSet` containing zero or more `WindowFrame`s, each acquiring, rendering, submitting, presenting independently.
- [ ] Mixed cadence: one window renders continuously while another redraws only when dirty.
- [ ] `Workspace` model: dock trees, tabs, panels, floating panels, native-window placements.
- [ ] Split panes, tab stacks, floating panels, detach-to-window, merge-window-back, drag-panel-between-windows.
- [ ] Preserve panel identity, focus, scroll, undo, camera state when moving panels between windows.
- [ ] Workspace serialization with monitor-aware restore and graceful fallback.
- [ ] Cross-window drag/drop for panels, assets, tabs, documents, nodes, files.
- [ ] Multi-window tests: create, resize, render, minimize, restore, close, recreate while other windows keep rendering.

---

## Backend and Platform

### Slang compiler service

- [ ] `ShaderCompilerService`: worker-thread compilation, reflection, cache lookup, diagnostics.
- [ ] Compile Slang through its in-process C API; external `slangc` is a developer fallback only.
- [ ] Games ship without requiring `slangc`, Vulkan SDK, or any external compiler on player machine.
- [ ] Hot reload transaction: compile on worker, reflect/validate, queue pipeline rebuild, swap at safe graph boundary, keep last-known-good, emit diagnostics.
- [ ] Release distribution modes: source-shipped, cache-shipped, hybrid.
- [ ] Packaging validation: required Slang runtime libraries present in the game bundle per platform.
- [ ] Reflect specialization constants.
- [ ] Keep runtime shader compilation off the render thread.

### Platform isolation

- [ ] Move OS-specific code into `crates/sturdy-engine-platform/src/{linux,windows,macos}/...`; engine code on platform-neutral APIs.
- [ ] Platform capability structs and degraded-apply reports so higher layers choose behavior without knowing the OS.
- [ ] Directories: `linux/wayland/`, `linux/x11/`, `windows/window_effects/`, `macos/window_effects/`.

### Vulkan backend maturity

- [ ] Vulkan-specific tests: coordinate-space conversions, viewport/scissor, texture origin, readback orientation.
- [ ] Vulkan frame timing, timestamp query, queue wait, present wait, frames-in-flight diagnostics.
- [ ] Vulkan resource lifetime validation: frame-delayed destruction, swapchain recreation, resize churn.
- [ ] Multi-surface Vulkan presentation: per-window surface capabilities, independent acquire/present sync.
- [ ] Vulkan upload planning: staging rings, copy commands, layout transitions, queue ownership, semaphore sync, frame-budgeted submission.
- [ ] Vulkan parallel command recording via worker-built command buffers.
- [ ] `VK_EXT_mesh_shader`: device extension when detected; `EXT_mesh_shader` bits in `BackendFeatures`.
- [ ] `VK_KHR_fragment_shading_rate`: plumb shading rate image attachment through render pass API.
- [ ] `VK_AMDX_shader_enqueue`: `BackendFeatures::work_graphs`.
- [ ] `vkCmdDrawIndexedIndirectCount` (Vulkan 1.2) for GPU-written draw counts.
- [ ] `VK_EXT_device_fault`: enhanced GPU hang diagnostics; structured crash report on `VK_ERROR_DEVICE_LOST`.
- [ ] Buffer device address (`VK_KHR_buffer_device_address`, Vulkan 1.2): `Buffer::device_address() -> u64` for inline pointer encoding.

---

## Ongoing Architectural Constraints

- [ ] Treat "requires restart" as a failure case unless the OS/compositor makes it impossible.
- [ ] Restrict CPU/GPU waiting to frame-boundary policy: frames-in-flight throttling, swapchain/present, readback requested by the app, shutdown/device-loss recovery.
- [ ] Add diagnostics for accidental synchronisation: blocking upload, pipeline compile stall, queue idle, fence wait outside shutdown.
- [ ] Keep the deferred frame submission contract: app enqueues intent, flush encodes and submits, GPU does not wait until next frame's fence.
- [ ] Keep all engine samples and testbed demos on the deferred path.
- [ ] Standardise time as monotonic `Instant`/`Duration` at engine boundaries; floating seconds only as convenience views.
- [ ] Standardise colour handling: linear scene colour internally, explicit sRGB decode/encode at I/O boundaries.
- [ ] Standardise resource debug labels for surfaces, images, buffers, passes, pipelines, and generated resources.
- [ ] Standardise capability queries before feature enablement.
