# Sturdy Engine Roadmap

## Product Direction

Sturdy Engine is worth using in three modes:

1. **Shader playground** — open a window, write Slang, see it run, tweak parameters live.
2. **Graphical apps and custom UI** — standalone tools, dashboards, inspectors, editors.
3. **Games** — including a path toward footage that can plausibly read as real life.

The simple path must be the best path, not a toy path. Each mode should feel complete without requiring the user to build the runtime shell themselves. The architecture must scale from simple to deep without rewrites.

### What's working today

The Vulkan backend is solid: precise pipeline barriers, 2-frame-in-flight command contexts, pool-slab descriptor allocation, O(n) pass scheduling, and incremental pipeline cache saves. The deferred render graph compiles passes, infers dependencies, and submits without CPU stalls between frames. Shader reflection derives bind groups, validates resource usage, and exposes vertex inputs. The shader playground use case is functional end-to-end.

---

## API Design Contract

This rule applies to every system in this roadmap without exception.

**Every subsystem must work perfectly with zero configuration and expose every dial when the user wants one.**

Concretely: every major subsystem ships a `*Config` struct where `Default::default()` produces a production-quality result tuned for the common case. Every field in that struct is `pub`, documented with its valid range and what it trades off, and respected by the system. The user never needs to open a Config to get a good game; they open it only when the defaults are wrong for their specific case or when they want to squeeze more out.

```rust
// Zero config — just works. Defaults are tuned for a mid-range GPU at 1440p.
let scene = GpuDrivenScene::new(&engine)?;

// Full control — every dial visible, IDE-discoverable, documented.
let scene = GpuDrivenScene::with_config(&engine, GpuDrivenSceneConfig {
    restir_di: ReSTIRDiConfig {
        initial_candidates: 32,           // [8, 128] — quality vs cost
        temporal_history_frames: 30,      // [1, 60]  — stability vs responsiveness
        spatial_radius_pixels: 30.0,      // [5, 60]  — neighbourhood blending radius
        visibility_rays: true,            // false = no shadow rays, cheaper
        ..Default::default()
    },
    vrs: VrsConfig {
        min_rate: ShadingRate::Rate1x1,
        max_rate: ShadingRate::Rate2x2,   // Rate4x4 for aggressive cost reduction
        motion_threshold: 0.15,           // pixels/frame above which VRS is disabled
        luminance_gradient_threshold: 0.2,
        ..Default::default()
    },
    ..Default::default()
})?;
```

Breaking this contract is a bug, not a feature request. Adding a knob that is only accessible by editing source code is not allowed.

---

## Priority: Next Quarter

These three tracks unblock the remaining two use cases. Work on them before everything else.

### Track 1 — Game loop shell

The engine has a frame loop and a runtime settings system, but app code can't see the information a game needs.

- [x] Expose `delta_time` and `frame_index` to app code via `AppRuntimeFrame` or a new `FrameClock` helper.
- [x] Add input polling API alongside the existing callbacks: `InputHub::is_key_pressed`, `is_key_just_pressed`, `is_key_just_released`, `mouse_delta`, `mouse_position`.
- [x] Add gamepad support: wire a platform gamepad backend (gilrs or winit) into the `GamepadAxis` / `GamepadButton` polling API.
- [x] Add `ActionMap` that binds named actions to keyboard/mouse/gamepad inputs and returns digital/analog values per frame.
- [x] Add fixed-timestep and interpolation helpers (`FrameClock` with monotonic timing, delta, fixed-step accumulator, and pacing error).
- [x] Add pointer-lock and relative mouse motion for first-person cameras.
- [x] Add a default game runtime shell that wraps `AppRuntime` with the above, so a game project needs zero extra plumbing to start.
- [ ] Add a small 2D game sample and a small 3D game sample that use only the default shell.

### Track 2 — 3D lighting and materials

The scene renders instanced geometry with camera transforms but no lighting. Even basic shading changes what the engine looks like. Track 6 (below) lifts this to a full PBR deferred pipeline; the items here are the prerequisite foundations.

- [x] Add directional light: a `DirectionalLight` uniform buffer with world-space direction, colour, and ambient term bound per-frame in the scene shader.
- [x] Add Lambert diffuse + Blinn-Phong specular shading in the default 3D fragment shader, driven by the light uniform. *(Replaced by GGX PBR once Track 6 is live; kept as a fallback shading model.)*
- [x] Add a `Material` descriptor: albedo colour, roughness, metallic, emissive. One uniform buffer per draw call, reflected and bound automatically.
- [ ] Add a directional shadow map pass: depth-only render pass writing to `Depth32Float`, sampled with PCF in the lit pass. The render graph handles ordering and barriers. *(Required by Track 6 — implement here, consumed there.)*
- [ ] Add point lights and spot lights: add `PointLight` and `SpotLight` uniform entries; support clustered light assignment (tile or cluster grid) to scale to hundreds of lights.
- [ ] Add normal mapping support: read tangent-space normals from a sampled texture; transform with a TBN matrix computed from mesh tangent + bitangent attributes.
- [ ] Add image-based lighting (IBL): prefilter an environment cubemap at varying roughness levels; precompute a BRDF integration LUT (NdotV × roughness → (scale, bias)); evaluate split-sum specular and diffuse irradiance in the lit pass.
- [ ] Add a reference scene that stresses lighting, shadows, and materials with realistic content.

### Track 3 — Asset loading

Everything is created programmatically today. Real projects need to load content from disk.

- [x] Add `engine.load_texture_2d(path) -> AssetHandle<Texture>` — PNG/JPEG loading via the `image` crate, automatic mip generation, GPU upload.
- [ ] Add `engine.load_mesh(path) -> AssetHandle<Mesh>` — GLTF 2.0 loading via the `gltf` crate, producing `Vertex3d` (position/normal/tangent/UV0/UV1) arrays, index buffers, and `GltfMaterial` references.
- [ ] Add `GltfMaterial` → `UnifiedMaterial` mapping: extract baseColorTexture, metallicRoughnessTexture, normalTexture, occlusionTexture, emissiveTexture, and factor constants; produce a fully populated `UnifiedMaterial` ready for the PBR deferred pipeline (Track 6).
- [ ] Support GLTF material extensions: KHR_materials_clearcoat, KHR_materials_transmission, KHR_materials_ior, KHR_materials_sheen, KHR_materials_emissive_strength.
- [x] Add `AssetHandle<T>` with state queries: `is_ready()`, `is_loading()`, `is_degraded()`, `failed_reason()`.
- [x] Add a placeholder policy: missing/loading textures use a visible checkerboard fallback rather than panicking.
- [ ] Add shader hot reload for loose `.slang` files: detect change, recompile, keep last-known-good on failure, emit visible diagnostics.
- [ ] Add asset hot reload for textures and meshes behind the same handle/state system as the streaming path.

---

## Track 6 — Unified Material System and Deferred PBR Pipeline

A single material definition that compiles to every rendering path: deferred G-Buffer fill, forward lit, shadow depth, ray-tracing hit, and path-traced reference. No material must be re-authored when switching between raster and RT; the engine derives the correct variant automatically.

### 6a — MaterialSurface interface and UnifiedMaterial

- [ ] Define `MaterialSurface` as the canonical output of every material in Slang:
  ```
  struct MaterialSurface {
      float3 base_color;   // linear sRGB albedo
      float  metallic;     // [0, 1]
      float  roughness;    // [0, 1] perceptual; shader squares to alpha for GGX
      float3 normal_ts;    // tangent-space normal for TBN transform
      float  occlusion;    // [0, 1] AO
      float3 emissive;     // linear HDR radiance (unclamped)
      float  opacity;      // [0, 1]; 1 = fully opaque
  };
  ```
- [ ] Define `MaterialDomain` enum: `Opaque`, `Masked` (alpha-test), `Translucent`, `Decal`.
- [ ] Define `ShadingModel` enum: `Unlit`, `Lambert` (legacy fallback), `PbrMetallicRoughness`, `PbrClearcoat`, `PbrTransmission`.
- [ ] Add `UnifiedMaterial` as the engine-facing material type. It holds:
  - `MaterialDomain`, `ShadingModel`, `RenderState`
  - A Slang snippet (inline or file) that implements `MaterialSurface evaluate_material(VertexData v)` — this is the **only shader code the user writes for a material**
  - Per-input source: each of the eight `MaterialSurface` fields may come from a constant value, a sampled texture, or a procedural expression baked into the snippet
- [ ] Add `UnifiedMaterialBuilder` with fluent API: `.base_color_texture(handle)`, `.base_color_constant(color)`, `.roughness_constant(r)`, `.with_normal_map(handle)`, `.domain(Opaque)`, etc.
- [ ] Add a standard `PbrMetallicRoughness` constructor that takes the GLTF PBR parameter set and produces a complete `UnifiedMaterial` with no additional user code.
- [ ] Add a standard `ProceduralMaterial` constructor: user provides a Slang function body; engine wraps it in the `evaluate_material` interface and compiles all variants.

### 6b — Shader variant compiler

- [ ] Add `MaterialVariantCompiler` that takes a `UnifiedMaterial` and emits compiled `ShaderProgram`s for each active rendering path:
  - **`GBufferFillVariant`** — evaluates `MaterialSurface`; packs results into G-Buffer render targets
  - **`ForwardLitVariant`** — evaluates `MaterialSurface`; applies full PBR lighting in one pass (used for transparent objects and the forward fallback)
  - **`ShadowVariant`** — alpha-test only; writes `gl_FragDepth` for shadow passes (depth-only for opaque)
  - **`RtAnyHitVariant`** — evaluates opacity for alpha-masked geometry in RT traversal
  - **`RtClosestHitVariant`** — evaluates full `MaterialSurface` for RT shadow rays, reflections, and GI queries
  - **`PathTracedVariant`** — evaluates `MaterialSurface`; evaluates GGX BSDF importance sampling for offline reference renders
- [ ] Cache compiled variants by material ID and variant type; invalidate on hot reload.
- [ ] Emit readable diagnostics when a material Slang snippet fails to compile in any variant.

### 6c — PBR BRDF library (`brdf.slang`)

Implement a shared Slang module included by all lit variants. Expose `BrdfConfig` — default produces energy-conserving GGX PBR; every toggle and coefficient is overridable.

- [ ] **GGX specular**: Trowbridge-Reitz NDF, Smith height-correlated masking-shadowing G2 (Heitz 2014), Schlick Fresnel with `F0 = lerp(0.04, base_color, metallic)`.
- [ ] **Lambertian diffuse**: energy-conserving complement `(1 - metallic) * base_color / π`; expose `BrdfConfig::diffuse_model: DiffuseModel` (Lambertian / Burley / OrenNayar) for games that want softer organic surfaces.
- [ ] **Multi-scattering energy compensation**: implement Turquin 2019 fit (scale + bias from precomputed BRDF LUT); expose `BrdfConfig::multi_scatter: bool` (default true).
- [ ] **BRDF integration LUT**: precompute a 128×128 `RG16Float` texture (NdotV, roughness) → (scale, bias); ship as engine asset; expose `BrdfConfig::lut_resolution: u32` (default 128; bump to 256 for highest fidelity).
- [ ] **IBL split-sum specular**: sample prefiltered env cubemap; expose `BrdfConfig::ibl_specular_mip_count: u32` and `BrdfConfig::ibl_max_roughness: f32`.
- [ ] **IBL diffuse irradiance**: SH9 or irradiance cubemap; expose `BrdfConfig::ibl_diffuse_mode: IblDiffuseMode` (SH9 / CubemapSampled).
- [ ] **Analytic light evaluation**: directional, point (sphere), spot (cone); expose `BrdfConfig::specular_aa: bool` (default true — reduces specular aliasing on high-roughness surfaces near geometry edges) and `BrdfConfig::energy_clamp: bool`.

### 6d — Deferred G-Buffer pipeline

- [ ] Define the standard G-Buffer layout as engine constants:
  - `g0` (`RGBA8Unorm`): `base_color.rgb` + `metallic` — base albedo and conductor flag
  - `g1` (`RGBA16Float`): world-space normal (octahedral-encoded into `.xy`) + `roughness` + `occlusion`
  - `g2` (`RGBA16Float`): `emissive.rgb` + shading model ID (`.a` integer flag)
  - `depth` (`Depth32Float`): hardware depth buffer
- [ ] Add a `GBufferPass` component: allocates the four G-Buffer images from the render graph (size-matched to swapchain); records one draw per opaque mesh using that mesh's `GBufferFillVariant`; owned by the engine, zero app code required.
- [ ] Add a `DeferredLightingPass` component: reads G0/G1/G2/depth as shader inputs; one fullscreen compute or fragment dispatch; reconstructs world position from depth + inv-view-proj; evaluates all active lights and IBL; outputs linear HDR `scene_color`.
- [ ] Reconstruct world position from depth in the deferred pass (no explicit world-pos G-Buffer attachment — saves bandwidth).
- [ ] Feed `scene_color` into the existing post-processing pipeline (bloom → AA → tone mapping).
- [ ] Keep the existing forward path for `Translucent` and `Decal` domain materials: sort back-to-front, render after deferred lighting into the HDR target.
- [ ] Add a `RenderPath` enum exposed on `SceneRenderer`: `DeferredThenForward` (default), `ForwardOnly` (fallback for hardware without MRT support).

### 6e — Shadow system

Expose `ShadowConfig` — defaults give CSM with 4 cascades and PCF; every parameter is tunable.

- [ ] **Cascaded Shadow Maps (CSM)**: depth-only pass per cascade; PCF in the deferred lighting pass; blend cascades at boundaries. `CsmConfig { cascade_count: u32 [1,8], resolution: u32, pcf_radius: u32, blend_range: f32, depth_bias: f32, slope_bias: f32, stabilise_cascades: bool, lambda: f32 }`.
- [ ] **Point light shadow maps**: dual-paraboloid or 6-face cube depth map. `PointShadowConfig { resolution: u32, pcf_radius: u32, depth_bias: f32, use_paraboloid: bool }`.
- [ ] **Spot light shadow maps**: single depth map per spot. `SpotShadowConfig { resolution: u32, pcf_radius: u32, depth_bias: f32 }`.
- [ ] **Shadow map atlas**: pack all shadow maps into a single atlas; expose `ShadowAtlasConfig { atlas_resolution: u32 [1024, 16384], page_size: u32, max_cached_pages: u32 }`.
- [ ] **PCSS (Percentage-Closer Soft Shadows)**: variable-width PCF based on blocker search distance; expose `ShadowConfig::pcss: bool` (default false — higher quality, higher cost) and `PcssConfig { blocker_search_samples: u32, pcf_samples: u32, light_size_world: f32 }`.
- [ ] **Optional RT shadows**: hardware RT shadow rays replacing PCF for the primary directional light; expose `ShadowConfig::rt_shadows: RtShadowMode` (Off / DirectionalOnly / All) with graceful fallback to CSM.

### 6f — Clustered light assignment

- [ ] Build a 3D frustum cluster grid each frame on GPU (compute pass): divide view frustum into tiles × depth slices.
- [ ] Assign active point/spot lights to clusters by bounding-sphere/cone overlap test on CPU or GPU.
- [ ] Upload compact light lists per cluster; index from deferred lighting pass and forward lit pass.
- [ ] Support at least 1024 active point/spot lights in the scene at any time.

### 6g — Real-time ray tracing integration

- [ ] Build and maintain a top-level acceleration structure (TLAS) from all opaque mesh instances each frame; rebuild on mesh add/remove, incremental refit on transform change.
- [ ] Add `RtShadowPass`: trace shadow rays from G-Buffer surface points toward the primary directional light; write 1-bit visibility into a shadow mask image; composite with deferred lighting.
- [ ] Add `RtReflectionPass`: trace reflection rays from G-Buffer surface points; evaluate `RtClosestHitVariant` at hit; accumulate into a reflection radiance image; blend into deferred specular using `roughness`-based fade (smooth surfaces use RT; rough surfaces use IBL prefiltered env).
- [ ] Add `RtAmbientOcclusion`: short-range hemisphere rays from G-Buffer; replace or blend with baked AO.
- [ ] Expose `RtFeatures` flags on `SceneRenderer`: `SHADOWS`, `REFLECTIONS`, `AMBIENT_OCCLUSION`; each independently toggleable; graceful fallback when hardware RT is unavailable.
- [ ] Add a path-traced reference renderer: progressive accumulation using `PathTracedVariant` hit shaders; Russian roulette termination; NEE (next-event estimation) for direct lights; use as ground-truth comparison and screenshot output.

### 6h — Environment and IBL authoring

- [ ] Add `EnvironmentMap` asset: load HDR equirectangular (`.hdr`, `.exr`) → convert to cubemap → prefilter for specular (per-roughness mip chain using GGX importance sampling) → compute irradiance cubemap.
- [ ] Ship one default environment map as an engine asset so PBR materials look reasonable out of the box.
- [ ] Add runtime environment map switching with smooth blend transition.
- [ ] Add sky atmosphere model (Rayleigh + Mie scattering) as a procedural environment source for outdoor scenes; capture to cubemap each frame or when sun direction changes.

---

## Track 7 — Unified geometry front-end (mesh shaders + virtual mesh)

The geometry front-end is the part of the pipeline that decides **which triangles reach the rasterizer** and how they get there. This track replaces the current single-path (vertex buffer → draw call) with a pluggable abstraction so each game can choose the right trade-off between compatibility, GPU efficiency, and feature depth.

The key design rule from the research: **"mesh shader" is one backend option, not the whole system.** The asset type is `VirtualMesh`; the rendering backend is selected from `GeometryBackend` at runtime. Material/pixel shading is decoupled — all backends feed the same G-Buffer, depth, shadow, and visibility-buffer attachments.

```text
VirtualMesh (asset, authored once)
  ├── vertices + indices          → ClassicVertex, ComputeIndirect
  ├── meshlets + meshlet_vertices
  │   + meshlet_triangles + bounds → MeshShader
  ├── meshlet_groups (LOD DAG)    → VirtualizedRaster
  └── rt_proxy                   → RayTracingFallback, RayTracingSelectedClusters
```

### 7a — VirtualMesh asset type

Foundation types already added to `crates/sturdy-engine/src/geometry.rs`:
- [x] `GeometryBackend` enum: ClassicVertex, ComputeIndirect, MeshShader, VirtualizedRaster, RayTracingFallback, RayTracingSelectedClusters.
- [x] `GeometryRendererCaps` derived from `Caps`; `best_opaque_backend()` / `supports()`.
- [x] `Meshlet`: vertex/triangle offsets, counts, and `MeshletBounds` (bounding sphere, normal cone, LOD error).
- [x] `MeshletBounds` (`#[repr(C)]`, GPU-uploadable): center, radius, cone apex/axis/cutoff, LOD error.
- [x] `MeshletGroup`: compatible LOD decision group with parent error for crack-free transitions.
- [x] `SubMesh`: material range in the index buffer.
- [x] `VirtualMesh`: unified asset holding classic vertices/indices + meshlet data + LOD groups + RT proxy.
- [x] GPU indirect command structs: `DrawIndirectCommand`, `DrawIndexedIndirectCommand`, `DispatchIndirectCommand`, `DrawMeshTasksIndirectCommand`.
- [x] `HizDesc`: hierarchical-Z pyramid descriptor for GPU occlusion culling.
- [ ] Add `VirtualMesh::from_gltf_mesh()`: load a GLTF primitive and populate classic vertex/index data with correct `Vertex3d` layout (position, normal, tangent, UV0).
- [ ] Add meshlet generation via `meshopt-rs`: call `meshopt::build_meshlets()` + `meshopt::build_meshlet_bounds()` to populate `meshlets`, `meshlet_vertices`, `meshlet_triangles`, and `MeshletBounds`.
- [ ] Add LOD simplification via `meshopt-rs`: `meshopt::simplify()` at multiple error levels; pack results into `meshlet_groups` as a DAG with parent error metrics.
- [ ] Add `VirtualMeshProxy` generation: simplify the full mesh to ≤ 10% triangle count for use as the RT BLAS proxy.
- [ ] Add `VirtualMeshBuilder` with fluent API matching `UnifiedMaterialBuilder`'s style.

### 7b — Render graph indirect work variants

Foundation types added to `render_graph.rs`:
- [x] `DrawIndirectDesc`: indirect_buffer + offset + draw_count + stride + indexed flag.
- [x] `DispatchIndirectDesc`: indirect_buffer + offset.
- [x] `DrawMeshShaderDesc`: direct group_count_x/y/z mesh-shader dispatch.
- [x] `DrawMeshShaderIndirectDesc`: buffer-driven mesh-shader group counts.
- [x] `PassWork::DrawIndirect`, `PassWork::DispatchIndirect`, `PassWork::DrawMeshShader`, `PassWork::DrawMeshShaderIndirect` variants.
- [ ] Vulkan backend: emit `vkCmdDrawIndirect` / `vkCmdDrawIndexedIndirect` for `PassWork::DrawIndirect`.
- [ ] Vulkan backend: emit `vkCmdDispatchIndirect` for `PassWork::DispatchIndirect`.
- [ ] Vulkan backend: emit `vkCmdDrawMeshTasksIndirectEXT` (EXT_mesh_shader) for `PassWork::DrawMeshShaderIndirect`; check capability before use; emit error/no-op if missing.
- [ ] Add render graph validation: indirect buffer must be in `RgState::IndirectRead` before dispatch.
- [ ] Expose `DrawMeshShaderDesc` through `GraphImage` pass API analogous to `draw_mesh_instanced`.

### 7c — Classic + compute-indirect path

The lowest-risk starting point; works on all hardware.

- [ ] Add `GeometryRenderer::classic(mesh)` that produces a `DrawDesc` from a `VirtualMesh`'s vertex/index data. Replaces direct `DrawDesc` construction in `frontend_graph.rs`.
- [ ] Add `CullingComputePass`: a compute shader that reads per-object bounds and writes surviving `DrawIndexedIndirectCommand`s into a compact output buffer. Uses the previous frame's depth buffer for basic Hi-Z occlusion (even without a full pyramid).
- [ ] Add frustum-cull predicate: project object bounding sphere against the six frustum planes; write `instance_count = 0` for culled objects.
- [ ] Add `HizPass`: compute shader that builds a `log2(max(w, h))` mip pyramid from the depth buffer each frame, stored as `Format::R32Float` sampled image.
- [ ] Add Hi-Z occlusion predicate to `CullingComputePass`: project bounding sphere into screen space; sample Hi-Z at `log2(screen_radius)` mip; reject if entire sphere is behind stored depth.
- [ ] Keep `ClassicVertex` as the unconditional fallback: if the culling pass is absent or disabled, fall back to a single `DrawDesc` per sub-mesh.

### 7d — Mesh shader path

- [ ] Add `MeshShaderPipelineDesc` to `pipeline.rs`: task_shader (optional) + mesh_shader + fragment_shader + layout; no vertex input layout (mesh shader owns vertex loading).
- [ ] Add Vulkan pipeline creation for `MeshShaderPipelineDesc`: require `VK_EXT_mesh_shader`; create `VkPipeline` with mesh/task stage `VkShaderStageFlagBits`.
- [ ] Add built-in task shader template `task_cull.slang`: reads per-meshlet `MeshletBounds` from a storage buffer; evaluates frustum + backface-cone + Hi-Z tests; emits surviving meshlet workgroup indices via `EmitMeshTasksEXT`.
- [ ] Add built-in mesh shader template `mesh_emit.slang`: reads `meshlet_vertices` and `meshlet_triangles` from storage buffers; decompresses local indices; writes `gl_Position` (+ normal, UV) per vertex; emits triangle index list.
- [ ] Add `GeometryRenderer::mesh_shader(mesh, caps)` that selects `MeshShader` if available, otherwise falls back to `ClassicVertex`.
- [ ] Feed the mesh-shader path into the same G-Buffer, depth, and shadow pass attachments as the classic path. The material/pixel shader is unchanged.

### 7e — Virtual raster path (Nanite-like)

- [ ] Add cluster hierarchy traversal compute shader `cluster_lod_select.slang`: walks `MeshletGroup` DAG from a root node; for each group evaluates `lod_error / view_distance < screen_error_threshold`; writes selected meshlet indices to a compact buffer.
- [ ] Thread the selected cluster buffer into the task shader: instead of launching one workgroup per meshlet, launch one per selected cluster index.
- [ ] Add `ClusterPage` streaming abstraction: meshlet data is stored in fixed-size pages; the per-frame LOD cut drives which pages must be resident; missing pages trigger an async load request.
- [ ] Add `VirtualGeometryStats` diagnostics: drawn clusters/triangles per frame, culled clusters, LOD histogram, streaming page hits/misses.
- [ ] Implement group-level LOD selection rather than independent per-cluster to prevent cracks at LOD boundaries (requires the `meshlet_groups` DAG and parent error values).

### 7f — Ray tracing integration

- [ ] Build a `TlasBuilder` that constructs a top-level acceleration structure from all `VirtualMesh` RT proxies each frame; refit on transform changes, rebuild on mesh add/remove.
- [ ] Add `BlasBuildPass`: a compute/transfer pass that builds or refits the per-mesh BLAS from `VirtualMesh::rt_proxy` vertex/index data.
- [ ] Expose `GeometryBackend::RayTracingFallback` through the RT shadow and reflection passes in Track 6g.
- [ ] Add `GeometryBackend::RayTracingSelectedClusters`: build a BLAS from the current frame's selected cluster subset for high-quality near-camera geometry. Budget-gate this behind a distance threshold; fall back to `RayTracingFallback` beyond it (mirroring Unreal's Lumen behavior with Nanite fallback meshes).

### 7g — Mix-and-match per pass

- [ ] Expose a `RenderPassBackendOverride` per scene-renderer pass: allows a game to use `MeshShader` for the G-Buffer pass, `ClassicVertex` for shadow maps, and `RayTracingFallback` for RT reflections — all from the same `VirtualMesh` assets.
- [ ] Add `SceneRenderer::set_backend(pass, backend)` with validation against `GeometryRendererCaps`.
- [ ] Document the supported combination matrix: which `GeometryBackend` values are valid for G-Buffer / shadow / depth-prepass / RT / visibility-buffer passes.
- [ ] Add a testbed mode that cycles through available backends on keypress and shows a diagnostic overlay with triangle counts, culling stats, and timing.

---

## Track 8 — Full Bindless + GPU-Driven Architecture

True GPU-driven rendering eliminates per-draw CPU work. All scene data lives in GPU-resident buffers; shaders index into it directly. The CPU emits one or a handful of indirect dispatches per frame regardless of scene complexity. Every track above benefits from this as a foundation.

### 8a — Bindless descriptor system

- [ ] Enable `VK_EXT_descriptor_indexing` (core in Vulkan 1.2) and create one large **descriptor heap** for all textures, samplers, and storage buffers; assign stable indices at resource creation time.
- [ ] Expose `BindlessHandle<T>` as the engine-facing type: a `u32` index valid for the lifetime of the resource. Binding a texture = storing its index; sampling it = `textures[handle.index].sample(...)`.
- [ ] Store all per-material data in a single GPU-resident `StructuredBuffer<MaterialData>` indexed by `material_id`; eliminate per-draw descriptor set allocation and update.
- [ ] Implement a **mega-buffer draw path**: each draw call carries only a 4-byte push constant (an index into a `DrawData` buffer); the vertex shader reads transform, material ID, and per-object constants from `DrawData[index]` — zero CPU-side per-draw binding.
- [ ] Gate the bindless path behind `BackendFeatures::bindless`; fall back to the current grouped-descriptor path on hardware that lacks it.
- [ ] Add `BindlessTextureAtlas`: allocate texture array slices and 2D atlas regions from a single large texture; expose stable `u32` slice indices for use in shaders.
- [ ] Validate descriptor indices in debug builds; provide a readable error ("texture handle 427 out of range") instead of a GPU hang.

### 8b — Fully GPU-driven scene submission

- [ ] Build a **GPU scene buffer**: one `GpuInstanceData` per scene object (model matrix, AABB, LOD bias, material ID, visibility flags); upload once on change, never re-upload unchanged frames.
- [ ] Move frustum culling and HZB occlusion to a single GPU compute dispatch that reads `GpuInstanceData`, writes one `DrawIndexedIndirectCommand` per visible instance (plus an indirect draw count) into a persistent `VisibleDrawBuffer`.
- [ ] Use `vkCmdDrawIndexedIndirectCount` so the GPU-written draw count drives the actual number of draws with no CPU readback.
- [ ] Support two-phase occlusion culling: Phase 1 renders last frame's visible set; Phase 2 re-tests newly unoccluded objects against the freshly written depth buffer.
- [ ] Expose `GpuDrivenScene` as a drop-in replacement for `Scene`; both share `VirtualMesh` assets and `UnifiedMaterial` definitions.

### 8c — Variable Rate Shading (VRS)

- [ ] Detect `VK_KHR_fragment_shading_rate` capability and expose `BackendFeatures::variable_rate_shading` (already detected; this item wires it into the render path).
- [ ] Implement **Tier 1 VRS**: set a per-draw shading rate (1×1, 1×2, 2×1, 2×2) for screen-edge regions and low-motion areas. Target: 20–30% shading cost reduction on equivalent visual output.
- [ ] Implement **Tier 2 VRS** (per-primitive): mesh/task shader writes `SV_ShadingRate` per primitive; use lower rates for back-facing, distant, or low-variance geometry.
- [ ] Generate a **VRS image** each frame from motion vectors + luminance gradient; feed it as the fragment shading rate attachment to the G-Buffer pass.
- [ ] Disable VRS on the final tonemap pass; only apply inside the G-Buffer and deferred lighting passes where rate reduction doesn't affect post-processing quality.

### 8d — GPU Work Graphs (when available)

- [ ] Detect `VK_AMDX_shader_enqueue` (Vulkan Work Graphs experimental extension); expose `BackendFeatures::work_graphs`.
- [ ] Port the cluster LOD selection + mesh shader dispatch (Track 7e) to a Work Graph: the LOD selection node emits mesh node payloads directly, eliminating the intermediate indirect buffer round-trip.
- [ ] Prototype a **material resolve Work Graph**: visibility buffer → launch material-specific shading nodes per unique material tile — eliminates G-Buffer bandwidth for highly complex scenes.
- [ ] Keep classical indirect dispatch as the fallback on hardware without Work Graph support; expose both paths through the same `GeometryRenderer` interface.

### 8e — Shader pipeline precompilation + PSO caching

- [ ] Build a **pipeline library** at first run: compile all `UnifiedMaterial` variants to disk-cached PSOs; subsequent launches load from cache with zero compile stalls.
- [ ] Add a **PSO pre-warm pass** during loading screens: enumerate required materials and pipelines, trigger compilation on shader workers, block game start until all active-scene PSOs are ready.
- [ ] Expose `PsoWarmupReport` with compile times, cache hit rates, and total variant count so games can tune their asset load gate.
- [ ] Ship a `slangc`-ahead-of-time compiled cache in release builds so players never stall; runtime recompilation is dev-only.

---

## Track 9 — Advanced Global Illumination

Global illumination is the single biggest visual quality leap beyond deferred PBR. This track covers the full spectrum from screen-space fallbacks to full ReSTIR path tracing, giving games a slider from "fast and good" to "reference quality."

### 9a — Virtual Shadow Maps (VSM)

Replaces CSM with a page-based virtual shadow atlas — the technique powering UE5 Fortnite Chapter 4 and pairing naturally with Nanite/VirtualizedRaster geometry.

- [ ] Allocate a **virtual shadow atlas** (`R32Float`, 16384×16384 logical, backed by 128×128-pixel resident pages); maintain a page table mapping (light, mip, tile) → physical page.
- [ ] Each frame: analyse depth buffer to determine which shadow pages are visible to the camera; mark pages dirty when lights or casters move; only render dirty pages.
- [ ] Render shadow casters into dirty pages using the mesh's `ShadowVariant` + the `VirtualizedRaster` or `MeshShader` backend; casters that haven't moved do not re-render.
- [ ] Sample the VSM atlas from the deferred lighting pass using a hardware PCF or PCSS kernel; decode page table address in-shader.
- [ ] Support at least 16 directional, point, and spot light shadow sources simultaneously within the same atlas without atlas defragmentation stalls.
- [ ] Gate VSM behind a `ShadowTechnique::Virtual` capability flag; fall back to CSM atlas on hardware that can't maintain the atlas.

### 9b — ReSTIR Direct Illumination (ReSTIR DI)

Spatiotemporal importance resampling for direct lighting from many lights — the technique behind "infinite lights" in modern path-traced games.

- [ ] Implement **initial candidate sampling**: for each screen-space surface point, generate N light candidates by importance sampling the light list; store as reservoirs (`RIS_Reservoir { light_idx, weight, w_sum }`).
- [ ] Implement **temporal reuse**: reuse the previous frame's reservoir at the reprojected pixel; combine with the current frame's candidates via reservoir merge.
- [ ] Implement **spatial reuse**: share reservoirs with neighbouring pixels in a small screen-space radius; 4–8 taps; apply target function (unshadowed radiance) for bias correction.
- [ ] Validate visibility with RT shadow rays for accepted samples; pack into a `DirectLightSample` image feeding the deferred lighting pass.
- [ ] Expose `GiFeatures::RESTIR_DI`; fall back to clustered analytic lights when RT is unavailable.
- [ ] Reference the open-source RTXDI sample repository (MIT-licensed shader code) for the reservoir data structures and bias-correction math; do not take a runtime SDK dependency.

### 9c — ReSTIR Global Illumination (ReSTIR GI)

Reservoir resampling for one-bounce indirect diffuse — the technique that makes real-time GI viable for dynamic scenes.

- [ ] Trace **secondary rays** from G-Buffer surface points at 1 ray per pixel; store the hit radiance + hit position as an initial GI reservoir.
- [ ] Apply **temporal reuse** across frames with reprojection; apply **spatial reuse** across neighbouring pixels.
- [ ] Use a **BRDF-weighted target function** for reservoir acceptance so importance sampling follows the material's reflectance distribution.
- [ ] Output a denoised indirect diffuse image; composite with the deferred direct lighting output: `total = direct + indirect_diffuse + indirect_specular`.
- [ ] Expose `GiFeatures::RESTIR_GI`; degrade gracefully to SSGI (Rendering Quality section) when RT is unavailable.

### 9d — ReSTIR Path Tracing (ReSTIR PT)

Full multi-bounce path resampling — the state-of-the-art technique powering RTX Mega Geometry and NvRTX 5.6+.

- [ ] Extend the `PathTracedVariant` accumulation renderer (Track 6g) with **ReSTIR PT reservoirs**: store full path prefixes (not just final hit), resample across temporal and spatial neighbours.
- [ ] Implement **reconnection shift**: when merging two reservoirs' paths, reconnect through the merge vertex to avoid MIS weight divergence.
- [ ] Implement **hybrid shift mapping**: combine random replay and reconnection shifts for paths with visibility-sensitive bounces.
- [ ] Output a low-sample-count real-time result (1–4 paths/pixel) suitable for denoised real-time use; also expose high-SPP progressive accumulation mode.
- [ ] Gate behind `GiFeatures::RESTIR_PT`; requires hardware RT + sufficient VRAM for reservoir buffer.

### 9e — Probe-based and surfel GI (Lumen-style dynamic GI)

Screen-independent GI for dynamic scenes where ReSTIR's screen-space reservoir history breaks (teleport, scene cuts, large dynamic environments).

- [ ] Place a **world-space irradiance probe grid** (adaptive octree, ~2m probe spacing near camera); each probe stores SH9 or octahedral irradiance.
- [ ] Update probes via RT rays each frame (budget: 256–512 rays per probe, amortised over 4–8 frames); blend new samples into probe cache.
- [ ] Add **probe validity masks**: invalidate probes when geometry near them changes (dynamic objects, breakable props); mark probes for re-capture.
- [ ] Sample the probe grid from the deferred lighting pass for indirect diffuse; blend with ReSTIR GI (higher-quality, screen-space) using a screen-age weight — probes dominate for disoccluded and off-screen regions.
- [ ] Add **surfel GI** as an alternative/complement: project G-Buffer pixels into world-space surfels; accumulate incident radiance per surfel over multiple frames; read back in the lighting pass. Works without RT hardware.
- [ ] Expose `GiFeatures::PROBE_GRID`, `GiFeatures::SURFEL`; games select which techniques are active independently.

### 9f — AI denoising

- [ ] Integrate **Intel Open Image Denoise (OIDN)** for offline / high-quality reference denoising: runs on CPU or GPU (SYCL backend); produces noise-free output from 1–4 spp path-traced frames.
- [ ] Add an in-engine **temporal denoiser** for real-time ReSTIR outputs: history accumulation + variance-guided SVGF (Spatiotemporal Variance-Guided Filtering); target: stable 1-spp ReSTIR GI at 4K/60 fps.
- [ ] Expose `DenoiserMode::Temporal` (real-time, in-frame) and `DenoiserMode::OIDN` (offline, async CPU); select automatically based on whether the frame is interactive or an export.

---

## Track 10 — Temporal Upscaling and Frame Generation

Temporal upscaling is the highest-leverage performance feature available: it delivers near-native quality at 50–70% of native render cost, and frame generation doubles perceived frame rate at modest GPU cost. Every game should be able to enable at least one of these.

Only frameworks that work natively in both Vulkan and D3D12 without cross-API translation are considered. FSR 4's ML mode requires D3D12 and is excluded. Every upscaler here runs native Vulkan.

### 10a — FSR 3.1 (primary — open source, Vulkan + D3D12, all vendors)

FSR 3.1 is open-source MIT, Vulkan-native, works on every GPU vendor, and ships with frame generation. It is the primary upscaler.

- [ ] Integrate **FSR 3.1 upscaling** via the AMD FidelityFX SDK: feed motion vectors, exposure, depth, colour, and camera jitter each frame; receive upscaled output at display resolution. Runs natively on Vulkan and D3D12 with no translation layer.
- [ ] Feed FSR 3.1 with the existing TAA jitter sequence; motion vectors must cover opaque, transparent, particle, and UI-world geometry.
- [ ] Integrate **FSR 3 frame generation** (open-source, Vulkan-native): interpolates an additional frame between rendered frames using optical flow; doubles effective frame rate on all hardware with negligible VRAM overhead.
- [ ] Expose `FsrConfig` with full dials: `quality: FsrQualityMode` (NativeAA / Quality / Balanced / Performance / UltraPerformance), `sharpness: f32 [0, 1]`, `mip_lod_bias: f32`, `auto_exposure: bool`, `frame_gen: bool`, `frame_gen_latency_padding: u32`, `reactive_mask_auto: bool`, `reset_on_velocity_discontinuity: bool`, `reset_threshold: f32`.
- [ ] `FsrConfig::default()` selects Quality mode with sharpness 0.5, frame gen enabled, auto-exposure on, auto reactive mask on — a production-quality result with zero user configuration.
- [ ] Detect camera cuts and teleports automatically via velocity field discontinuity; pass the reset flag to FSR without app code.

### 10b — XeSS 2.x (fallback — open SDK, Vulkan + D3D12, DP4a runs on any GPU)

XeSS 2 has a DP4a compute path that runs on any GPU and a native XMX path for Intel Arc. It is the right fallback when FSR is disabled or when an Intel-optimised path is preferred.

- [ ] Integrate **XeSS 2.x** via the Intel open SDK: XMX hardware path for Arc GPUs, DP4a fallback for AMD, NVIDIA, and integrated graphics.
- [ ] Expose `XessConfig` with full dials: `quality: XessQualityMode`, `sharpness: f32`, `use_jitter: bool`, `motion_vector_scale: [f32; 2]`, `reset_history: bool`.
- [ ] `XessConfig::default()` selects Quality mode and works without any configuration.

### 10c — Unified upscaler interface

Only open-source, vendor-agnostic upscalers are included. FSR 3.1 covers all hardware; XeSS adds an Intel-optimised path.

- [ ] `UpscalerConfig::auto()` selects the best available option: XeSS XMX (Intel Arc) → FSR 3.1 (all others). Frame generation via FSR 3 enabled by default when available. The user calls `auto()` once and is done.
- [ ] Compute `render_resolution(display_resolution, quality)` from the active mode; all render targets allocate at render resolution; the upscaler outputs at display resolution.
- [ ] Tone mapping runs **after** the upscaler on the full-resolution output; pre-upscaler passes (bloom, AO, lens effects) run at render resolution.
- [ ] Expose `UpscalerReport` in `GraphReport`: active upscaler name and version, render resolution, display resolution, upscale ratio, frame gen active, latency estimate.
- [ ] Expose `UpscalerConfig::reactive_mask_auto: bool` (default true): auto-generate the reactive mask from transparent + particle alpha so the upscaler handles them without ghosting.

---

## Track 11 — GPU Memory and Performance Infrastructure

These items are prerequisites for hitting "record performance" at scale. They make everything else in the roadmap faster without adding visual features.

### 11a — Sub-allocation and memory budgeting

- [ ] Replace per-resource `vkAllocateMemory` with a **VMA (Vulkan Memory Allocator)**-backed sub-allocator: heap per memory type, 256 MiB blocks, linear/pool strategies per resource lifetime.
- [ ] Expose a `GpuMemoryBudget` query per frame: available VRAM, used VRAM, host-visible used; warn when budget exceeds 80%.
- [ ] Add `BufferPool` for transient per-frame scratch buffers: ring allocator in host-visible memory; zero per-frame allocation overhead.
- [ ] Implement aliased memory for G-Buffer images: all G-Buffer attachments occupy a single `vkDeviceMemory` allocation with explicit alias ranges; reclaim memory at end of G-Buffer pass.

### 11b — Async compute and multi-queue

- [ ] Detect and use a **dedicated async compute queue** (distinct from the graphics queue); expose `QueueType::AsyncCompute` in the render graph.
- [ ] Schedule the HZB build, cluster LOD selection, and ReSTIR reservoir update on the async compute queue in parallel with the previous frame's G-Buffer pass.
- [ ] Schedule texture decode and upload on the **DMA/transfer queue** in parallel with rendering; signal a semaphore when the upload is complete; consume it before the first pass that reads the texture.
- [ ] Add `PassDesc::queue: QueueType` so any render graph pass can opt into async compute or transfer queues; the compiler inserts cross-queue semaphores automatically.
- [ ] Expose `GpuTimeline` diagnostics: show per-queue utilisation and cross-queue stall gaps in the frame inspector.

### 11c — GPU crash and performance diagnostics

- [ ] Integrate **NVIDIA Aftermath** (optional, behind a feature flag): capture GPU crash dumps on device-lost; embed breadcrumbs in command buffers; emit crash dump to disk with the faulting pass name.
- [ ] Integrate **AMD Radeon GPU Profiler (RGP) markers**: insert `vkCmdBeginDebugUtilsLabelEXT` per render graph pass; visible in RGP timeline as named passes with correct durations.
- [ ] Add **GPU timestamp queries** per render graph pass: store results in a ring buffer; expose per-pass GPU time in `GraphReport` and the runtime overlay.
- [ ] Add a **frame graph inspector UI** (debug overlay mode): shows the DAG of passes, resource lifetimes, barrier counts, queue assignment, and per-pass GPU timing for the last frame.

### 11d — Texture compression pipeline

- [ ] At asset load time, transcode any uncompressed texture to the best GPU-native block-compressed format: `BC7` (colour, SDR), `BC6H` (HDR), `BC5` (normal maps), `BC4` (grayscale AO/roughness); use `intel-tex-rs` or `basis-universal` for transcoding.
- [ ] On mobile / integrated GPU paths without BC7, transcode to `ASTC 4×4` or `ETC2` instead.
- [ ] Add `TextureDesc::prefer_compressed: bool`; default true for all textures except render targets and UAVs.
- [ ] Cache the compressed result next to the source file (`.cached/texture_name.bc7.dds`) so subsequent loads skip transcode; invalidate on source file change.
- [ ] Ship a `compress_textures` CLI tool that pre-compresses all assets in a content directory; use in the release build pipeline.

---

## Track 12 — Hair, Particles, and Procedural FX

### 12a — Strand-based hair rendering

Hair is the hardest geometry type to get right. This track implements a production-quality hair pipeline.

- [ ] Define `HairStrand` asset: cubic Bézier control points per strand, root UV on scalp mesh, per-strand material ID (base color, roughness, melanin, cuticle scale, transmission).
- [ ] Implement hair rasterization using the `MeshShader` backend: task shader selects strand segments by frustum + screen-size; mesh shader tessellates each segment into an oriented quad or cylinder.
- [ ] Evaluate the **Marschner / d'Eon hair BSDF**: R (specular reflection), TT (transmission), TRT (back-scatter); expose `ShadingModel::Hair` in `brdf.slang`.
- [ ] Integrate hair with the deferred pipeline: hair geometry renders in the forward-lit pass (translucent domain) after opaque G-Buffer; reads deferred lighting output for indirect illumination.
- [ ] Add deep-opacity maps for self-shadowing in hair volumes.
- [ ] Integrate GPU-driven simulation: position-based dynamics or a Cosserat rod model; run on the async compute queue.

### 12b — GPU-driven particle system

`ParticleEmitter::new()` works out of the box; `ParticleSystemConfig` exposes every simulation and rendering parameter when defaults aren't enough.

- [ ] Define `ParticleEmitter` with `spawn_rate`, `lifetime_range`, `initial_velocity`, `velocity_spread`, `gravity_scale`, `drag`, `color_over_lifetime` (gradient), `size_over_lifetime` (curve), `rotation_over_lifetime`. `Default` gives a basic burst emitter.
- [ ] Simulate in a **GPU compute pass** each frame: update positions, integrate forces, age particles, kill expired, emit from budget. All state in GPU-resident `StructuredBuffer`s; zero CPU readback ever.
- [ ] Support **sub-step simulation**: `ParticleSystemConfig::substeps: u32` (default 1; up to 4 for high-velocity effects); runs multiple integrate-and-collide passes per rendered frame on the async compute queue.
- [ ] Render particles via the `MeshShader` backend: task shader frustum-culls particle clusters; mesh shader emits billboards (camera-facing, velocity-aligned, or fixed-axis) or arbitrary `VirtualMesh` instances per particle.
- [ ] Support `ParticleRenderMode`: Billboard (default), VelocityStretch, AxisAligned, Mesh (one `VirtualMesh` per particle), Ribbon (trail strip between consecutive particles).
- [ ] Support `ParticleForceField`: point attractor/repeller, directional wind, vortex, turbulence noise. Multiple force fields combine additively; fields are GPU-resident and updated without CPU sync.
- [ ] Support **vector field forces**: 3D `R16G16B16A16_Float` texture of velocity vectors uploaded once; particles advect through it each frame — fire, smoke, fluid FX.
- [ ] Support **collision**: particles test against scene SDF or a set of analytic shapes (sphere, box, plane); bounce or die on contact; `ParticleSystemConfig::collision_mode: CollisionMode` (Off / Sdf / Analytic).
- [ ] Expose `ParticleSystemConfig` with full dials: `max_particles: u32`, `sort_mode: ParticleSortMode` (None / BackToFront / ByDepth), `lighting_mode: ParticleLightingMode` (Unlit / DeferredLit / ForwardLit), `cast_shadows: bool`, `receive_shadows: bool`, `substeps: u32`, `collision_mode`, `emit_from: EmitShape` (Point / Sphere / Box / Mesh surface).
- [ ] `ParticleSystemConfig::default()` gives unlit billboards, BackToFront sorting, no collision — correct and fast for most particle effects with zero configuration.

### 12c — Decal system (Doom Eternal style)

Full deferred decal pipeline: OBB-projected, GPU-clustered, bindless-textured, per-channel G-Buffer writes, Reoriented Normal Mapping blending. Targets thousands of simultaneous decals with no CPU-side batching logic.

`DecalEmitter::new(obb, material)` places a decal with zero configuration. `DecalConfig` exposes every blend parameter.

- [ ] **Decal asset**: each decal carries up to five texture channels — `albedo_alpha`, `normal`, `roughness`, `metallic`, `emissive`. Every channel is optional; absent channels leave the underlying G-Buffer value untouched. Textures are sourced from the bindless texture heap (Track 8a) — no descriptor churn per decal.
- [ ] **OBB projection**: each decal is an oriented bounding box placed in world space. The deferred decal pass, running after the G-Buffer fill pass, tests each lit pixel's world position against all decals that touch its cluster tile; surviving pixels compute UV by projecting the world position into the decal's local space.
- [ ] **GPU clustering**: assign decals to the same 3D frustum cluster grid as point lights (Track 6f). Decal OBBs are assigned to overlapping cluster cells on GPU (compute pass); the deferred decal shader iterates only the decals in the pixel's cluster tile — O(visible decals per pixel), not O(total decals).
- [ ] **Reoriented Normal Mapping (RNM) blend**: combine the decal's tangent-space normal with the underlying surface's G-Buffer normal using RNM so both normals contribute — bullet holes preserve the surface's underlying brick or metal texture instead of flattening it.
- [ ] **Fade parameters**: `DecalConfig { depth_fade_start: f32, depth_fade_end: f32, angle_fade_start_cos: f32, angle_fade_end_cos: f32 }` — depth fade prevents hard edges at the OBB boundary; angle fade prevents projection onto near-perpendicular surfaces (floors receiving wall decals).
- [ ] **Per-channel blend mask**: `DecalConfig::write_mask: DecalWriteMask` — bitmask selecting which G-Buffer channels the decal writes (Albedo | Normal | Roughness | Metallic | Emissive). Defaults to all channels. Blood decals write Albedo+Normal but skip Metallic so they don't chrome-plate metal surfaces.
- [ ] **Priority and layering**: decals have an integer priority; higher-priority decals composite over lower-priority ones in the cluster ordering. `DecalConfig::priority: i32`.
- [ ] **Mesh decals** (baked, not projected): a mesh decal is authored by artists in the DCC tool and embedded in the mesh's UV space. At runtime it renders as a forward-lit sub-mesh in the same pass as the parent mesh, with its material blended on top using alpha. No OBB projection required — use for hero assets (character wounds, vehicle damage) where UV-precise placement matters.
- [ ] `DecalConfig::default()` gives full-channel write, depth fade over 5 cm, 45° angle fade, priority 0 — a correct decal with no tuning.

---

## Track 14 — GPU Physics

Cross-platform GPU physics using Vulkan compute shaders — no CUDA, no vendor lock-in, works on AMD, Intel, and NVIDIA. The simulation runs on the async compute queue in parallel with rendering; the physics world is always GPU-resident so there is no CPU↔GPU round-trip per frame.

The simulation core is **Extended Position-Based Dynamics (XPBD)** — a constraint-based method that maps naturally to GPU parallelism and handles rigid bodies, soft bodies, cloth, and fluid in one unified solver. XPBD is unconditionally stable, stiffness parameters are independent of timestep, and Lagrange multipliers persist across substeps for accurate constraint forces.

`PhysicsWorld::new()` gives a working world with gravity, 4 substeps, and collision. `PhysicsWorldConfig` exposes every solver parameter.

### 14a — Core XPBD solver

- [ ] Implement the **XPBD integration loop** in Slang compute shaders: predict positions, solve constraints (Gauss-Seidel with graph-coloured parallel islands), update velocities. Run on `QueueType::AsyncCompute`.
- [ ] Support configurable **substeps**: `PhysicsWorldConfig::substeps: u32` (default 4; up to 20 for stiff simulations). Each substep runs predict → solve → update; total cost scales linearly with substep count.
- [ ] Implement **broad-phase collision detection**: BVH rebuilt on GPU each frame using the linear BVH (LBVH) algorithm — sort primitives by Morton code, build hierarchy in O(n log n) compute passes.
- [ ] Implement **narrow-phase collision detection**: GJK/EPA for convex-convex pairs; SAT for box-box and sphere-box; sphere-sphere analytically. Generate contact manifolds on GPU.
- [ ] Implement **contact constraints**: non-penetration (distance ≥ 0) and friction (Coulomb model) as XPBD positional constraints; expose `PhysicsMaterial { friction: f32, restitution: f32, density: f32 }`.
- [ ] Expose `PhysicsWorldConfig` with full dials: `gravity: Vec3`, `substeps: u32`, `solver_iterations: u32` (constraint solve passes per substep, default 1), `contact_offset: f32`, `sleep_threshold: f32`, `sleep_frames: u32`, `max_bodies: u32`, `max_contacts: u32`.

### 14b — Rigid body dynamics

- [ ] Define `RigidBody`: mass, inertia tensor, angular/linear damping, sleeping, kinematic flag (kinematic bodies are moved by the app, not the solver — useful for animated characters driving physics).
- [ ] Support collision shapes: `CollisionShape::Sphere`, `Box`, `Capsule`, `ConvexHull(VirtualMesh)`, `TriangleMesh(VirtualMesh)` (static/kinematic only — too expensive for dynamic triangle meshes).
- [ ] Implement **compound shapes**: multiple `CollisionShape`s with local offsets per rigid body.
- [ ] Implement **joints/constraints**: `FixedJoint`, `BallJoint` (3-DOF), `HingeJoint` (1-DOF with limits), `SliderJoint` (prismatic), `SpringJoint` (distance with stiffness and damping). All expressed as XPBD constraints.
- [ ] Expose `RigidBodyConfig` with full dials: `mass`, `inertia_override: Option<Mat3>`, `linear_damping`, `angular_damping`, `kinematic`, `ccd: bool` (continuous collision detection for fast-moving bodies), `gravity_scale: f32`.

### 14c — Soft body and cloth

- [ ] Implement **XPBD soft body**: tetrahedral mesh; distance constraints between connected vertices; volume constraints for incompressibility; shape-matching constraints for stiffness. Expose `SoftBodyConfig { stiffness: f32, volume_stiffness: f32, damping: f32, collision_margin: f32 }`.
- [ ] Implement **XPBD cloth**: quad or triangle mesh; stretch constraints (warp/weft), shear constraints, bending constraints (dihedral angle). Expose `ClothConfig { stretch_stiffness: f32, shear_stiffness: f32, bend_stiffness: f32, damping: f32, thickness: f32, wind_drag: f32, wind_lift: f32 }`.
- [ ] Cloth self-collision via a GPU spatial hash: particles query neighbours in the hash; positional correction prevents interpenetration.
- [ ] Attach cloth and soft bodies to rigid bodies via `PinConstraint`: pinned vertices follow a rigid body transform — enables character-attached cloaks, flags on poles.
- [ ] Hair simulation in Track 12a re-uses the cloth strand solver rather than maintaining a separate system.

### 14d — GPU fluid simulation (SPH)

- [ ] Implement **Smoothed Particle Hydrodynamics (SPH)**: each fluid particle carries density, pressure, velocity; forces from pressure gradient, viscosity, and surface tension. Fully GPU-resident, updated on async compute.
- [ ] Spatial hashing for neighbour search: `FluidConfig::support_radius: f32` defines the kernel radius; hash grid cell size = support radius; rebuilt each substep.
- [ ] Expose `FluidConfig` with full dials: `rest_density`, `stiffness`, `viscosity`, `surface_tension`, `gravity_scale`, `max_particles: u32`, `particle_radius`, `substeps`.
- [ ] Fluid surface extraction: marching cubes or screen-space fluid rendering (depth-based normal reconstruction + SSS-like scattering for water); expose `FluidRenderMode` (Particles / MarchingCubes / ScreenSpace).
- [ ] Fluid↔rigid body two-way coupling: fluid particles exert buoyancy and drag forces on rigid bodies in their neighbourhood; rigid bodies displace fluid.

### 14e — Scene query API

- [ ] Implement GPU-accelerated **raycast**: `PhysicsWorld::raycast(origin, dir, max_dist) -> RaycastHit`; runs on the GPU BVH; result available next frame (async) or immediately (sync with GPU stall — use sparingly).
- [ ] Implement **sphere cast**, **box cast**, and **shape overlap** queries on the same BVH.
- [ ] Implement **trigger volumes**: axis-aligned or oriented boxes that report enter/stay/exit events per body; evaluated on GPU, events delivered to CPU via a compact event buffer each frame.
- [ ] Expose `PhysicsQueryConfig`: `max_results_per_query: u32`, `filter_mask: u32` (layer bitmask), `async: bool` (default true — result ready next frame, no stall).

### 14f — Physics ↔ rendering integration

- [ ] `PhysicsBody::visual_mesh() -> VirtualMesh`: the physics body drives the transform of its `VirtualMesh` in the scene each frame — zero CPU readback, updated via a GPU compute pass that writes transforms directly into the `GpuInstanceData` buffer.
- [ ] `PhysicsWorld::debug_draw(frame)`: draws collision shapes as wireframes using the existing `DebugDraw2d` line renderer extended to 3D — off by default, enabled by `PhysicsWorldConfig::debug_draw: bool`.
- [ ] Deterministic mode: `PhysicsWorldConfig::deterministic: bool` (default false); when true, uses fixed-point arithmetic and canonically ordered island processing — required for lockstep multiplayer.
- [ ] Export physics state as a compact binary snapshot; import to restore — enables save games and replay systems.

---

## Track 15 — Area Lights, Emissive Surfaces, and Physically Based Luminaires

Physically based lighting means energy is measured in real-world photometric units (lumens, lux, candela, nits) and light shapes are geometrically correct — a 2m×2m LED panel casts a soft rectangular highlight, not a point. The techniques here cover the raster approximation (LTC — two texture lookups, zero integration, runs in the clustered deferred pass) and the RT path (explicit surface sampling + ReSTIR DI), giving accurate results in both pipelines from the same light definitions.

`RectLight::new(position, orientation, size, lumens, color)` works out of the box. Every parameter has a physically meaningful default.

### 15a — LTC area lights (raster path)

Linearly Transformed Cosines (Heitz et al. SIGGRAPH 2016) evaluate an analytically exact integral of a GGX BRDF over an arbitrary convex polygon in two texture lookups. Cost is O(lights per cluster tile) — the same as a point light. Precomputed LTC matrices ship as engine assets.

- [ ] Precompute and ship two engine asset textures: `ltc_matrix.dds` (64×64 `RGBA32Float` — the 3×3 GGX LTC matrix encoded as 4 coefficients, parameterized by NdotV and roughness) and `ltc_amplitude.dds` (64×64 `RG32Float` — GGX specular and Lambertian diffuse amplitudes).
- [ ] Implement LTC evaluation in `brdf.slang`: `float3 ltc_evaluate_rect(surface, L[4], ltc_matrix, ltc_amplitude)` integrates a rectangular area light over the GGX BRDF; `ltc_evaluate_disk` and `ltc_evaluate_sphere` handle the other shapes.
- [ ] Add `LightType::Rect { half_width: f32, half_height: f32 }`, `LightType::Disk { radius: f32 }`, `LightType::Sphere { radius: f32 }`, `LightType::Tube { length: f32, radius: f32 }` to the light type enum alongside existing `Directional`, `Point`, `Spot`.
- [ ] Assign all area light types to the 3D frustum cluster grid (Track 6f) exactly as point/spot lights; the deferred lighting pass iterates cluster-assigned area lights and evaluates LTC per light.
- [ ] Support up to 1024 simultaneous area lights without performance degradation (same cluster budget as point lights — area lights consume one cluster slot each).
- [ ] Expose `AreaLightConfig` with full dials: `luminous_flux: f32` (lumens), `color: [f32; 3]` (linear), `temperature_k: Option<f32>` (blackbody color temperature override), `two_sided: bool` (light emits from both faces), `specular_only: bool` (area light contributes to specular but not diffuse — useful for subtle screen reflections without overpowering a scene), `diffuse_only: bool`.

### 15b — Emissive mesh lights (screens, monitors, signs)

An emissive mesh is any `VirtualMesh` whose `UnifiedMaterial` has a non-zero emissive channel. The user adds it to the scene once; the engine handles registering it as a light source in both the raster and RT pipelines. No additional API required.

- [ ] When a `UnifiedMaterial` is built with a non-zero `emissive` channel, automatically register the mesh as an **emissive light source** in the light pool with an approximated rectangular area light derived from the mesh's AABB.
- [ ] The emissive texture (image or video frame) drives both the G-Buffer emissive channel (which adds self-glow via the deferred lighting pass) and the area light colour: `light_color = average_emissive_texel × emission_strength` evaluated once per frame on GPU.
- [ ] Support **video-driven emissive**: `EmissiveConfig::source: EmissiveSource` — `Constant([f32; 3])`, `Texture(TextureHandle)`, or `VideoStream(VideoStreamHandle)`. The video decoder uploads frames to the emissive texture each frame on the async compute queue; the lighting system reads the frame-average color for the area light approximation. No special code path — the consumer registers a video stream, sets emission strength, and gets a physically correct glowing screen.
- [ ] For **raster**: the emissive mesh light is treated as a `LightType::Rect` area light (or a set of rect lights for large meshes) and evaluated via LTC in the deferred lighting pass. Nearby objects receive soft rectangular highlights.
- [ ] For **RT**: the emissive mesh surface is added to the **light importance list** used by ReSTIR DI (Track 9b); primary rays can directly sample the emissive triangle surface via NEE; each triangle contributes proportionally to its luminous flux.
- [ ] Expose `EmissiveConfig` with full dials: `emission_strength: f32` (nits or relative scale — `PhysicalUnit::Nits(f32)` or `PhysicalUnit::Relative(f32)`), `source: EmissiveSource`, `cast_light: bool` (default true — whether the emissive registers as a light source), `light_sample_count: u32` (how many NEE samples per pixel for this light in RT, default 1), `area_light_override: Option<LightType>` (explicit shape override for the raster LTC approximation when the AABB guess is wrong).
- [ ] `EmissiveConfig::default()` gives `cast_light: true`, `emission_strength: Relative(1.0)`, `source: Constant([1.0, 1.0, 1.0])` — any mesh with a non-zero emissive material just works.

### 15c — IES photometric profiles and flood lights

IES (Illuminating Engineering Society) files describe the real-world angular intensity distribution of a physical luminaire — a 400W stadium flood light, a theatrical fresnel, a car headlight, a neon tube. Loading a real IES file from the manufacturer's data sheet gives physically correct shadows, falloff patterns, and spill.

- [ ] Add `IesProfile` asset: load a `.ies` text file; parse the candela distribution grid (H angles × V angles); upload as a 2D `R16Float` texture indexed by `(horizontal_angle / 360°, vertical_angle / 180°)`.
- [ ] Apply the IES profile as a multiplicative attenuation on top of spot or area light evaluation: `attenuation *= ies_profile.sample(direction_to_surface)`.
- [ ] Expose `SpotLightConfig::ies_profile: Option<IesProfileHandle>` and `AreaLightConfig::ies_profile: Option<IesProfileHandle>`.
- [ ] Add **physically based flood light**: `FloodLight` — a high-power spot or rect area light with IES profile, colour temperature (2700K–6500K blackbody), `luminous_intensity: f32` (candela), and a cookie texture (a gobo — projected image mask for theatrical effects). Gobo textures sample through the light's projection frustum and multiply with the output colour.
- [ ] Expose `FloodLightConfig` with full dials: `luminous_intensity: f32` (cd), `color_temperature_k: f32`, `ies_profile: Option<IesProfileHandle>`, `cookie: Option<TextureHandle>`, `cookie_scale: [f32; 2]`, `cookie_rotation: f32`, `inner_cone_angle: f32`, `outer_cone_angle: f32`, `near_attenuation: f32`, `far_attenuation: f32`, `cast_shadows: bool`, `shadow_resolution: u32`.

### 15d — Light units and photometric pipeline

All light intensities are expressed in SI photometric units. The renderer converts to radiometric quantities internally — the user never deals with raw shader coefficients.

- [ ] Accept light intensities as `LuminousFlux(f32)` (lumens — total power emitted by the source), `Luminance(f32)` (nits — surface brightness), `LuminousIntensity(f32)` (candela — intensity in a direction), or `Illuminance(f32)` (lux — incident intensity at a surface). Convert to scene-linear radiance internally.
- [ ] Expose `PhysicsBasedLightConfig::exposure_compensation: f32` (EV offset — allows artistic brightening/dimming without changing the physical value).
- [ ] The scene's exposure setting (auto-exposure or manual EV) correctly scales all physically-specified lights so a 400-lumen bulb looks like a 400-lumen bulb relative to a 10,000-lux exterior.
- [ ] Add `LightDebugOverlay` mode: renders light ranges, cluster assignment, LTC polygon outlines, and per-light cost as a debug visualisation.

### 15e — Performance scaling to thousands of lights

- [ ] Clustered deferred handles point/spot/area lights uniformly — 1024 area lights within the cluster budget is the baseline target from Track 15a.
- [ ] For **raster with > 1024 area lights**: add a second-tier cluster using a **64×64×32 light grid** (finer cells than the primary cluster) for scenes with dense light arrays (nighttime cityscapes, stadium interiors, datacenter corridors). Secondary cluster is built on GPU async compute.
- [ ] For the **RT path**: ReSTIR DI (Track 9b) handles unlimited emissive mesh lights and area lights via importance resampling — the light pool can contain 100,000 entries and the reservoir algorithm selects the relevant ones per pixel with no per-light loop. Integrate area lights into the ReSTIR DI light pool alongside point lights.
- [ ] **Light proxy culling**: each area light (and each emissive mesh light) registers a bounding sphere for broad-phase cluster rejection. Lights whose bounding sphere doesn't intersect a cluster cell are excluded before the LTC evaluation loop.
- [ ] **Power-proportional importance sampling** for RT: lights are sampled proportional to their luminous flux so high-power lights get more RT samples; dim ambient lights rarely waste rays.

---

## Track 13 — Visibility Buffer (Hardware Rasterized Triangle ID)

A visibility buffer stores `(triangle_id, instance_id)` per pixel in a single 64-bit render target instead of a full G-Buffer. All material shading runs in screen space in a subsequent compute pass. This is better than a G-Buffer for virtual geometry because it decouples geometric complexity from shading complexity.

- [ ] Render all opaque geometry into a `visibility_buffer` image (`R64Uint` or `R32Uint×2`): encode `(instance_id << 32 | primitive_id)` per pixel using the `MeshShader` or `VirtualizedRaster` backend.
- [ ] In a **material resolve compute pass**: for each pixel, decode `(instance_id, primitive_id)` → fetch triangle vertices → interpolate barycentrics → evaluate the mesh's `UnifiedMaterial` snippet → write G0/G1/G2.
- [ ] Add `RenderPath::VisibilityBuffer` as a third option alongside `DeferredThenForward` and `ForwardOnly`; expose via `SceneRenderer::set_render_path()`.
- [ ] For hardware without mesh shaders: fall back to a compute pass that emulates visibility buffer rasterization using raster + a custom depth pass — slower but correct.
- [ ] Combine with VRS: the visibility buffer pass runs at full rate; the material resolve pass uses a VRS image to skip resolve in non-detailed regions.
- [ ] Save G-Buffer bandwidth: because material evaluation is deferred to screen space, the visibility buffer approach eliminates G-Buffer writes for occluded geometry even on non-mesh-shader hardware.

---

## Track 4 — Layout engine and widget system

The text system, input callbacks, and Clay UI bindings exist, but there is no layout engine. This is the single blocker for real GUI apps.

- [ ] Integrate `taffy` (pure-Rust flex/grid layout) as the layout engine. Map widget descriptors to taffy nodes, run layout each frame, produce screen-space rectangles.
- [ ] Build a `ScreenUiRoot` that owns a layout tree, an input dispatcher, a focus scope, and a render pass.
- [ ] Add core widgets on top of taffy: `Label`, `Button`, `TextInput`, `Checkbox`, `Toggle`, `Slider`, `ScrollRegion`, `Panel`, `Tabs`.
- [ ] Add stable widget IDs, focus scopes, modal scopes, and per-frame retained state.
- [ ] Add root-level input routing: keyboard, mouse, scroll, pointer capture, and text input ownership.
- [ ] Add theme tokens: typography scale, spacing scale, radii, semantic colors, state colors (hover/pressed/focused/disabled).
- [ ] Add a `WorldUiRoot` for UI rendered onto world-space panels, with ray-to-panel hit testing and render-to-texture support.
- [ ] Add a `TextureUiRoot` for UI rendered into named graph images for downstream composition.
- [ ] Add standalone app conveniences: menu bars, status bars, toolbars, resizable panes, tabbed documents, and inspector panels.
- [ ] Add persistent UI state helpers for window geometry, dock layout, scroll position, and selection.
- [ ] Add accessibility tree generation: roles, names, descriptions, values, bounds, focus, selection, and actions.

### Text system completeness

- [ ] Add grapheme-aware cursor movement, word movement, bidi movement, and selection across wrapped lines.
- [ ] Add single-line editable text field with cursor, selection, focus, clipboard, and keyboard navigation.
- [ ] Add multiline editable text with scrolling, grapheme-aware selection, IME composition, and platform clipboard.
- [ ] Add fallback fonts, emoji, combining marks, ligatures, and OpenType features.
- [ ] Add SDF/MSDF rendering for large scalable text and world-space text.
- [ ] Add atlas residency, eviction, and dirty-rectangle upload policies.
- [ ] Add text performance counters: shaping time, atlas uploads, cache hit/miss, and memory use per frame.

---

## Track 5 — Shader playground auto-UI

The reflection system knows uniform names and types. Auto-generating parameter controls is a force multiplier for the playground use case.

- [ ] Detect push constant `struct` fields from shader reflection after `load_slang_source`.
- [ ] For each `float` field: generate a labelled slider with configurable `[min, max]` range (default `[0, 1]`).
- [ ] For each `uint` field: generate an integer input or toggle.
- [ ] For each `float2`/`float3`: generate a vector input or, where named with colour conventions, a colour picker.
- [ ] For each `bool`: generate a checkbox.
- [ ] Bind live widget values to push constant bytes each frame — zero app code required for a basic interactive shader.
- [ ] Add named presets that save/restore the parameter state for a given shader.
- [ ] Add export to a static screenshot at current parameters.
- [ ] Add a `ShaderPlayground` type that wraps `ShaderProgram` + auto-generated UI into one drop-in component.

---

## Ongoing Architectural Constraints

These apply to all work above.

- [ ] Treat "requires restart" as a failure case unless the OS/compositor makes it impossible.
- [ ] Restrict CPU/GPU waiting to frame-boundary policy: frames-in-flight throttling, swapchain/present, readback requested by the app, or explicit shutdown/device-loss recovery.
- [ ] Add diagnostics for accidental synchronisation: blocking upload, pipeline compile stall, queue idle, fence wait outside shutdown.
- [ ] Keep the deferred frame submission contract: app calls enqueue intent, flush encodes and submits, the GPU does not wait until the next frame's fence.
- [ ] Keep all engine samples and testbed demos on the deferred path so they teach queue-and-finalize behavior.
- [ ] Standardise time as monotonic `Instant`/`Duration` at engine boundaries; expose floating seconds only as convenience views.
- [ ] Standardise colour handling: linear scene colour internally, explicit sRGB decode/encode at I/O boundaries, explicit HDR transfer policy.
- [ ] Standardise resource debug labels for surfaces, images, buffers, passes, pipelines, and generated resources.
- [ ] Standardise capability queries before feature enablement: format support, image usage, sampler limits, queue support, and present modes.

---

## Rendering Quality

Work here deepens the visual output after Track 2 is complete.

### Post-processing pipeline

- [ ] Generalise bloom, AA, and tone mapping into a proper post stack that can host exposure, bloom, temporal effects, sharpening, grading, film grain, and lens effects in any order.
- [ ] Add stronger temporal AA using real motion vectors, camera jitter, and transparency-heavy scenes.
- [ ] Add a transparency-heavy validation scene so temporal and post effects work against composited content.
- [ ] Add motion-blur validation: camera-local vectors produce stable blur; moving objects blur correctly; camera-locked overlays do not blur.

### Mip generation and sampling

- [ ] Add automatic mip generation for sampled textures where format and usage support it.
- [ ] Add explicit mip graph operations: write, read, downsample N to N+1, upsample N+1 into N, transition selected mip ranges.
- [ ] Add sampler controls for LOD bias, min/max LOD, and mip filter choices.
- [ ] Add graph validation for accidental full-resource barriers when a mip/layer range would suffice.

### Photoreal rendering path

Track 6 delivers the PBR foundation. The items below extend it toward film-quality real-time output.

- [ ] **Energy-conserving multi-scattering**: integrate Turquin 2019 multi-scattering compensation across all direct and IBL specular lobes; verify energy conservation with a white-furnace test scene.
- [ ] **Subsurface scattering (SSS)**: separable SSS using a screen-space blur of irradiance weighted by profile; expose `ShadingModel::PbrSubsurface` for skin and translucent materials.
- [ ] **Anisotropic specular**: expose anisotropy direction and magnitude in `MaterialSurface`; evaluate Ashikhmin-Shirley or GGX anisotropic VNDF in `brdf.slang`.
- [ ] **Clearcoat layer**: `ShadingModel::PbrClearcoat`; second GGX lobe at fixed 0.04 F0; additive over base.
- [ ] **Transmission and volume**: `ShadingModel::PbrTransmission`; sample behind-surface color (thin or thick volume); full glTF KHR_materials_transmission and KHR_materials_volume support.
- [ ] **Screen-space global illumination (SSGI)**: short-range indirect diffuse using screen-space ray marching; complement RT AO and RT reflections at close range.
- [ ] **Volumetric fog and atmosphere**: frustum-voxel density grid; in-scattering from directional + local lights; exponential height fog as a fast fallback; sky atmosphere model (Rayleigh + Mie) as a procedural environment source for outdoor scenes.
- [ ] **Decal system**: deferred decals write to G0/G1/G2 after the G-Buffer fill pass; `MaterialDomain::Decal` projected onto geometry; blended by alpha mask.
- [ ] **Layered surface workflows**: expose a material layer stack (base + clearcoat + fuzz) that the compiler flattens into a single `MaterialSurface` evaluation; works in deferred, forward, and RT paths.
- [ ] **High-quality translucency**: order-independent transparency (OIT) using Weighted Blended OIT (WBOIT) or Moment OIT; rendered after the deferred pass; composited into HDR target.
- [ ] **Wet and glossy surface path**: runtime wetness mask modulating roughness + darkening base color; puddle normals via procedural detail.
- [ ] **Build a reference scene aimed at realistic output** to drive future rendering priorities; evaluate against path-traced ground truth using the `PathTracedVariant` accumulation mode.

### 2D and instanced rendering

- [ ] Add a first-class 2D sprite/batch path.
- [ ] Add tilemap and simple layered-scene helpers that compose cleanly with the graph model.
- [ ] Add examples for many instanced quads, per-instance colour/material parameters, and animated GPU-updated instance data.
- [ ] Add effect-oriented instancing such as layered glow sprites or particles.

---

## Full Asset Pipeline

Work here replaces the simple synchronous load from Track 3 with a proper streaming system.

- [ ] Add a `ContentRuntime` that owns asset requests, handles, background I/O, decode/transcode workers, upload plans, residency state, and diagnostics.
- [ ] Keep asset handles stable across load, reload, failure, eviction, and revalidation.
- [ ] Add a staged asset pipeline: Requested → Reading → Decoded → Transcoded → UploadQueued → GpuResident → Ready → Degraded → Failed → Evicted.
- [ ] Add texture streaming: tiny fallback mip immediately, progressive high-mip refinement, budget eviction.
- [ ] Add per-frame upload budgeting: bytes/frame, images/frame, staging memory, transfer queue time.
- [ ] Add staging buffer/ring allocator for async uploads without per-upload allocation churn.
- [ ] Add compressed texture policy: prefer GPU-native block-compressed formats, transcode in workers when not available.
- [ ] Add content priority and cancellation: visible-now, near-future prefetch, UI-critical, editor-preview, low-priority, cancelled/stale.
- [ ] Add development loose-file mode and release package mode behind the same virtual asset paths.
- [ ] Add asset hot reload using the same handle/state system as the streaming path.
- [ ] Add Vulkan sparse/tiled residency as a later optional tier, not the first streaming implementation.

### I/O backends

- [ ] Linux: prefer `io_uring` when available, fall back to a blocking I/O thread pool.
- [ ] Windows: prefer DirectStorage where it fits the Vulkan asset pipeline, fall back to overlapped/thread-pool I/O.
- [ ] Browser/WebAssembly: use browser fetch primitives with the same asset-handle API.

---

## Multi-Window, Workspace, and Docking

- [ ] Add `WindowRegistry` / `WindowManager` owned by the runtime shell with generation-checked `WindowHandle`s.
- [ ] Add `WindowDesc` and route all window creation/destruction through the event-loop command queue.
- [ ] Add per-window surface, swapchain, present mode, frame pacing, DPI/safe-area, cursor state, IME state, and compositor effects.
- [ ] Add `FrameSet` containing zero or more `WindowFrame`s, each acquiring, rendering, submitting, and presenting independently.
- [ ] Allow mixed cadence: one window renders continuously while another redraws only when dirty.
- [ ] Add a `Workspace` model that owns dock trees, tabs, panels, floating panels, and native-window placements.
- [ ] Add split panes, tab stacks, floating panels, detach-to-window, merge-window-back, and drag-panel-between-windows.
- [ ] Preserve panel identity, focus, scroll, undo, and camera state when moving panels between windows.
- [ ] Add workspace serialization with monitor-aware restore and graceful fallback.
- [ ] Add cross-window drag/drop for panels, assets, tabs, documents, nodes, and files.
- [ ] Ensure surface-lost, minimized, or zero-size windows suspend acquire/present without blocking other windows.
- [ ] Add multi-window tests: create, resize, render, minimize, restore, close, and recreate while other windows keep rendering.

---

## Backend and Platform

### Slang compiler service

- [ ] Add `ShaderCompilerService` as an engine subsystem with worker-thread compilation, reflection, cache lookup, and diagnostics.
- [ ] Compile Slang through its in-process C API; make external `slangc` process invocation a developer-tool fallback only, not a runtime dependency.
- [ ] Ensure games ship without requiring `slangc`, the Vulkan SDK, or any external compiler on the player machine.
- [ ] Add hot reload transaction: compile on worker, reflect/validate, queue pipeline rebuild, swap at safe graph boundary, keep last-known-good on failure, emit readable diagnostics.
- [ ] Add release distribution modes: source-shipped, cache-shipped, and hybrid.
- [ ] Add packaging validation that confirms required Slang runtime libraries are present in the game bundle per target platform.
- [ ] Reflect specialization constants.
- [ ] Add per-pass GPU timestamp queries and expose pass timings in `GraphReport`.
- [ ] Keep runtime shader compilation off the render thread; compile/reflection work runs on shader worker jobs.

### Platform isolation

- [ ] Move OS-specific code into `crates/sturdy-engine-platform/src/{linux,windows,macos}/...`; keep engine code on platform-neutral query/apply APIs.
- [ ] Ensure `sturdy-engine` asks `window_appearance_caps()`, `apply_window_appearance()`, `cursor_position()`, `clipboard_caps()`, `ime_caps()`, and `gamepad_caps()` instead of matching on `target_os`.
- [ ] Add directories: `linux/wayland/`, `linux/x11/`, `linux/wayland/background_effect/`, `windows/window_effects/`, `macos/window_effects/`.
- [ ] Return platform capability structs and degraded-apply reports so higher layers choose behavior without knowing the OS implementation.

### Vulkan backend maturity

- [ ] Add Vulkan-specific tests for coordinate-space conversions, viewport/scissor behavior, texture origin handling, and readback orientation.
- [ ] Add Vulkan frame timing, timestamp query, queue wait, present wait, and frames-in-flight diagnostics.
- [ ] Add Vulkan resource lifetime validation for frame-delayed destruction, swapchain recreation, and resize churn.
- [ ] Add multi-surface Vulkan presentation: per-window surface capabilities, independent acquire/present synchronisation.
- [ ] Add Vulkan upload planning: staging rings, copy commands, layout transitions, queue ownership, semaphore sync, and frame-budgeted submission.
- [ ] Add Vulkan parallel command recording using worker-built command buffers where graph dependencies allow.
- [ ] Enable `VK_EXT_mesh_shader` device extension when detected; expose `EXT_mesh_shader` feature bits in `BackendFeatures`; wire into `MeshShaderPipelineDesc` creation (Track 7d).
- [ ] Enable `VK_KHR_fragment_shading_rate` when detected; expose `BackendFeatures::variable_rate_shading` (already detected); plumb the shading rate image attachment through the render pass API (Track 8c).
- [ ] Enable `VK_AMDX_shader_enqueue` when detected; expose `BackendFeatures::work_graphs` (Track 8d).
- [ ] Enable `vkCmdDrawIndexedIndirectCount` (core in Vulkan 1.2) for GPU-written draw counts (Track 8b).
- [ ] Add `VK_EXT_device_fault` for GPU hang diagnostics: on `VK_ERROR_DEVICE_LOST`, query fault info and emit a structured crash report with the faulting address and pass name.
- [ ] Add buffer device address (`VK_KHR_buffer_device_address`, core in Vulkan 1.2) support: expose `Buffer::device_address() -> u64` for inline pointer encoding in shaders (required for full bindless, Track 8a).

### WebGPU target

- [ ] Treat WebGPU as a browser/WebAssembly backend only, not a native desktop replacement for Vulkan.
- [ ] Add browser runtime shell handling browser constraints: event loop ownership, canvas sizing, input capture, fullscreen, pointer lock, async device acquisition.
- [ ] Add capability downgrade reporting for browser limits, missing formats, restricted threading, and presentation constraints.
- [ ] Add WebGPU conformance scenes only after Vulkan coordinate, graph, input, and runtime contracts are stable.

### Render threading

- [ ] Add `GraphicsThreadingModel`: `SingleGraphicsOwner`, `ParallelPreparationOnly`, `ParallelCommandEncoding`, `MultiQueueParallel`.
- [ ] Target `ParallelCommandEncoding` for Vulkan where safe and useful.
- [ ] Separate render preparation from backend command recording.
- [ ] Allow worker threads to build render packets, batch keys, upload plans, and graph nodes before the render owner encodes commands.
- [ ] Expose a **dedicated async compute queue** (`QueueType::AsyncCompute`) in the render graph; the compiler inserts cross-queue semaphores automatically at resource hazard boundaries (Track 11b).
- [ ] Expose a **DMA/transfer queue** (`QueueType::Transfer`) for async texture and buffer uploads; the uploader runs independently and signals a semaphore consumed by the first pass that samples the uploaded resource (Track 11b).

---

## Reference Milestones

### Milestone A — Shader playground is great
- [ ] Open a shader with `load_slang_source`, see auto-generated parameter sliders, tweak them live.
- [ ] Hot reload a loose `.slang` file and see the change in the running window.
- [ ] Toggle HDR, AA, bloom, transparency, and present policy at runtime without restart.

### Milestone B — GUI apps work
- [ ] Build a multi-panel tool with labels, buttons, text fields, sliders, and scrolling using only the engine's widget layer.
- [ ] Layout reflows correctly on window resize.
- [ ] Input routing (focus, tab order, keyboard navigation) works without app boilerplate.
- [ ] Prove hit testing, clipping, and screenshots agree on top-left/Y-down orientation.

### Milestone C — Games work
- [ ] Build one 2D game and one 3D game using only the default game shell.
- [ ] Input polling, delta time, and gamepad work out of the box.
- [ ] Scene renders with directional CSM shadows and PBR materials (deferred G-Buffer path, Track 6).
- [ ] Load a PNG texture and a GLTF mesh (with GLTF PBR materials) from disk without custom asset code.
- [ ] Switch between `DeferredThenForward` and `ForwardOnly` render paths at runtime.
- [ ] Switch other graphics settings (MSAA, bloom, tone mapping, RT features) live during gameplay.

### Milestone D — High-end rendering (raster)
- [ ] Deferred G-Buffer pipeline with full GGX PBR BRDF, energy-compensating multi-scattering, IBL split-sum, CSM shadows, clustered point/spot lights.
- [ ] All standard GLTF PBR materials render correctly out of the box; procedural materials use the same deferred path with zero extra plumbing.
- [ ] White-furnace test passes (no energy gain or loss across all roughness values).
- [ ] On mesh-shader hardware: G-Buffer and depth passes use the `MeshShader` backend; shadow passes fall back to `ClassicVertex` — same `VirtualMesh` assets, different geometry front-end per pass.
- [ ] Frustum + Hi-Z occlusion culling active via `ComputeIndirect` path even on non-mesh-shader hardware.
- [ ] Demonstrate runtime texture streaming with progressively refined mips and frame-budgeted uploads.
- [ ] Produce a reference scene evaluated against the goal of plausibly real-looking real-time footage.

### Milestone E — High-end rendering (ray tracing + virtual geometry)
- [ ] RT shadow rays replace CSM for the primary directional light with no visible seam.
- [ ] RT reflections are active on smooth surfaces; IBL takes over at high roughness with no visible discontinuity.
- [ ] Path-traced reference mode accumulates to a converged ground-truth image using the same `UnifiedMaterial` definitions as the raster path.
- [ ] `RtFeatures` flags toggle individually at runtime; each falls back cleanly to the raster equivalent.
- [ ] On supported hardware: `VirtualizedRaster` backend active for opaque G-Buffer; cluster LOD selection on GPU; measurable triangle-count reduction vs. `ClassicVertex` for high-poly test scene.
- [ ] `RenderPassBackendOverride` demonstrated: G-Buffer on `MeshShader`, shadows on `ClassicVertex`, RT on `RayTracingFallback`, all from the same `VirtualMesh` asset.

### Milestone G — Record performance

The goal: a complex 3D scene running at the frame budget of a simple one.

- [ ] Full bindless GPU-driven scene (Track 8b): CPU issues ≤ 10 draw calls regardless of scene object count; all culling and LOD on GPU.
- [ ] FSR 2 (or better) active: render at 50–60% of native resolution; upscale to display resolution; output quality within 5% PSNR of native TAA.
- [ ] Virtual Shadow Maps active: all shadow casters in the scene share a single atlas; shadow pass cost is sublinear in light count.
- [ ] Async compute active: HZB build + ReSTIR reservoir update overlap with the previous frame's G-Buffer pass; GPU utilisation >85% on the reference scene.
- [ ] ReSTIR DI active: 1024 dynamic point lights rendered at GI quality with no frame rate regression vs. the clustered-light baseline.
- [ ] VRS active in the G-Buffer pass: measured shading cost reduction ≥ 20% on a high-resolution render without perceptible quality loss.
- [ ] Texture compression active: all content assets use BC7/BC6H/BC5; VRAM footprint ≤ 25% of uncompressed equivalent.
- [ ] GPU memory sub-allocator active: zero per-frame `vkAllocateMemory` calls; G-Buffer images alias into a single heap allocation.
- [ ] GPU physics active on the async compute queue: a scene with 10,000 rigid bodies runs at 60 Hz substep budget with zero GPU stalls and zero CPU readback per frame.

### Milestone F — Browser/WebGPU
- [ ] Run a constrained browser/WebGPU sample using the same app-facing contracts as Vulkan.
- [ ] Show clear browser-specific downgrade diagnostics.
- [ ] Forward-only render path (Track 6 `ForwardOnly` mode) used as the WebGPU fallback; same `UnifiedMaterial` definitions, no re-authoring.
