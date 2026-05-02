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

Implement a shared Slang module included by all lit variants.

- [ ] **GGX specular**: Trowbridge-Reitz NDF, Smith height-correlated masking-shadowing G2 (Heitz 2014), Schlick Fresnel with `F0 = lerp(0.04, base_color, metallic)`.
- [ ] **Lambertian diffuse**: energy-conserving complement `(1 - metallic) * base_color / π`.
- [ ] **Multi-scattering energy compensation**: implement Turquin 2019 fit (scale + bias from precomputed BRDF LUT) to recover energy lost to multiple surface reflections at high roughness.
- [ ] **BRDF integration LUT**: precompute a 128×128 `RG16Float` texture (NdotV, roughness) → (scale, bias) offline; ship as an engine asset; evaluate in all lit passes for energy-conserving IBL and point-light specular.
- [ ] **IBL split-sum specular**: sample prefiltered env cubemap at mip = `roughness * MAX_MIP`; multiply by BRDF LUT lookup.
- [ ] **IBL diffuse irradiance**: sample irradiance cubemap (or SH9 coefficients); weight by `(1 - metallic) * base_color`.
- [ ] **Analytic light evaluation**: directional, point (sphere), spot (cone); attenuate by inverse-square law for point/spot; angular attenuation for spot; compute one `brdf_eval(surface, L, V)` call per light.

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

- [ ] **Cascaded Shadow Maps (CSM)**: 4 cascades; depth-only pass per cascade using each mesh's `ShadowVariant`; store as `Depth32Float` array; PCF 3×3 in the deferred lighting pass; blend cascades at boundaries.
- [ ] **Point light shadow maps**: dual-paraboloid or 6-face cube depth map; PCF lookup in deferred pass.
- [ ] **Spot light shadow maps**: single depth map per spot; PCF.
- [ ] **Shadow map atlas**: pack all shadow maps into a single atlas texture to avoid descriptor pressure.
- [ ] **Optional RT shadows**: when `VK_KHR_ray_tracing_pipeline` is available, replace raster PCF with hardware RT shadow rays for the primary directional light; fall back to CSM transparently.

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

### Milestone F — Browser/WebGPU
- [ ] Run a constrained browser/WebGPU sample using the same app-facing contracts as Vulkan.
- [ ] Show clear browser-specific downgrade diagnostics.
- [ ] Forward-only render path (Track 6 `ForwardOnly` mode) used as the WebGPU fallback; same `UnifiedMaterial` definitions, no re-authoring.
