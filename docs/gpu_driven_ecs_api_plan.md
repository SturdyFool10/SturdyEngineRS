# GPU-Driven ECS + Mass Batching API Plan

This plan defines the direction for making SturdyEngine handle hundreds of thousands of renderable entities on high-end machines while keeping every engine system callable from any thread.

The core rule: **the CPU owns gameplay intent; the GPU owns render work expansion.** CPU systems should not submit one draw or upload one matrix per object. CPU systems should update compact, dirty, stable object state; GPU compute expands that into matrices, visibility, LOD, material selection, and indirect draw commands.

## References and takeaways

- **Bevy ECS**: systems are ordinary functions over typed component/resource access. Non-conflicting systems can run in parallel by default. The important API lesson is that the scheduler should infer or declare access cleanly, then exploit parallelism without user ceremony.
- **Unity DOTS / Entities + Hybrid Renderer**: ECS entities are collected into render data and sent through the rendering layer. The lesson is to keep rendering as a system over ECS data rather than an object-oriented scene graph that every game system mutates directly.
- **VkGuide GPU-driven rendering**: GPU-driven engines store object data in large GPU buffers, avoid per-object push constants/descriptors, cull on the GPU, and issue indirect draws. VkGuide explicitly describes object buffers, bindless resources, big vertex/index buffers, and compute writing indirect commands/instance remap buffers. It cites tutorial-scale numbers such as 125k objects and 40M triangles over multiple passes on an RTX 2080, and describes much higher object throughput when CPU submission is removed.
- **VkGuide compute culling**: compute shaders read object IDs + batch IDs, test object bounds against frustum/occlusion, atomically increment indirect draw instance counts, and write an instance-remap buffer from `SV_InstanceID` to object ID.

These references line up with SturdyEngine's roadmap Track 8: bindless descriptor system, per-material GPU buffer, mega-buffer draw path, GPU scene buffer, frustum/HZB culling, and `vkCmdDrawIndexedIndirectCount`.

## Current SturdyEngine state

Already present:

- `World`, `WorldView`, `ParallelSystem`, and `CompiledSchedule` provide a wave-based multithreaded ECS scheduler.
- `Scene` already has thread-safe command scaffolding (`SceneCommands`) and stable IDs.
- `Scene::gpu_scene_buffer` and `GpuInstanceData` already exist as a flat per-object GPU buffer concept.
- `Scene::cull_gpu` exists but currently dispatches per batch and writes one indirect command per instance slot.
- Bindless and descriptor-indexing groundwork exists in core.
- Render graph has indirect draw and multi-queue concepts.

Main gaps:

1. ECS render components are not yet the primary scene-authoring API.
2. CPU still materializes too much per-object render data.
3. GPU culling is per batch, not one dispatch over the render world.
4. Object matrices are still CPU-produced; GPU matrix expansion from compact transform state is not the default.
5. Material data is not fully centralized in one GPU-resident material table.
6. Draw batching is not yet organized around a persistent `GpuRenderWorld` with mesh/material/pipeline bins.

## Design principles

### 1. Stable IDs, compact deltas

Every renderable entity gets a stable `GpuObjectId`. CPU systems update compact component data and mark dirty ranges. The renderer uploads only changed slots/pages.

Avoid uploading per-frame per-object derived data:

- Do not upload model, model-view, normal matrix, previous model, bounds, and material constants separately.
- Upload compact source state: local transform, parent index, mesh ID, material ID, flags, bounds source, animation/skinning handles.
- Derive world matrices, previous matrices, normal matrices, and render bounds on GPU.

### 2. One global GPU scene, many views

The GPU scene is global and persistent. Per camera/light/shadow pass data is view-specific:

- Object data buffer: persistent.
- Transform source buffer: persistent, dirty-updated.
- World matrix buffer: persistent, GPU-written.
- Previous world matrix buffer: persistent, GPU-copied/rotated.
- Bounds buffer: persistent, GPU-written or CPU-authored for static meshes.
- Material table: persistent.
- Mesh table: persistent.
- Per-view cull output: transient per frame/view.

### 3. Batch by what the GPU cannot vary cheaply

Batch keys should be only the state that truly requires a different GPU draw/dispatch:

```text
PipelineId + Meshlet/MeshPassKind + MaterialShaderClass + VertexLayoutClass + RenderStateClass
```

Not per object. Not per material instance if material parameters are data in a buffer.

### 4. Bindless first, fallback explicit

Fast path:

- One global sampled image heap.
- One global sampler heap.
- One material table `StructuredBuffer<MaterialData>`.
- One object table `StructuredBuffer<ObjectData>`.
- Mesh/meshlet tables in storage buffers.
- Push constants only for view/pass global IDs, never per-object IDs.

Fallback path:

- Grouped descriptor sets per material class.
- CPU-side batches remain functional but are not the performance target.

### 5. ECS APIs are thread-safe by construction

Engine API law says every API is available on every thread. For ECS/render integration:

- Systems can enqueue render mutations from worker threads.
- `WorldCommands` and `SceneCommands` are per-thread/per-system buffers, applied at phase barriers.
- Resource registries use fine-grained locks or sharded queues, not one giant render-thread lock.
- Rendering reads a snapshot/extracted render world, never the live gameplay world directly during GPU submission.

## Proposed public API

### ECS components

```rust
#[derive(Component)]
pub struct Transform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

#[derive(Component)]
pub struct Parent(pub Entity);

#[derive(Component)]
pub struct LocalToWorld {
    pub object: GpuObjectId,
}

#[derive(Component)]
pub struct RenderMesh {
    pub mesh: MeshHandle,
    pub lod_group: Option<LodGroupHandle>,
}

#[derive(Component)]
pub struct RenderMaterial {
    pub material: MaterialHandle,
}

#[derive(Component)]
pub struct RenderBounds {
    pub local_sphere: Sphere,
    pub local_aabb: Aabb,
}

#[derive(Component)]
pub struct RenderVisibility {
    pub flags: VisibilityFlags,
    pub layer_mask: LayerMask,
}

#[derive(Component)]
pub struct PreviousTransform {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}
```

`LocalToWorld` is intentionally a handle, not a CPU matrix. The CPU may cache a matrix for gameplay queries, but rendering should not depend on CPU matrices.

### Render world resource

```rust
pub struct RenderWorld {
    pub objects: GpuObjectAllocator,
    pub meshes: MeshRegistry,
    pub materials: MaterialRegistry,
    pub dirty: RenderDirtyQueues,
}

impl RenderWorld {
    pub fn reserve_object(&self) -> GpuObjectId;
    pub fn commands(&self) -> RenderWorldCommands;
    pub fn set_mesh(&self, object: GpuObjectId, mesh: MeshHandle);
    pub fn set_material(&self, object: GpuObjectId, material: MaterialHandle);
    pub fn set_visibility(&self, object: GpuObjectId, visibility: VisibilityFlags);
}
```

All methods are callable from any thread. Internally they append to lock-free or sharded command queues. The render extraction phase applies them in deterministic order.

### App setup API

```rust
app.add_systems(Update, (
    player_input,
    physics_step,
    animation_graph,
    gameplay_ai,
));

app.add_systems(RenderExtract, (
    extract_transforms,
    extract_renderables,
    extract_lights,
));

app.add_render_pipeline(GpuDrivenPbrPipeline::default());
```

### System authoring

Short term: keep explicit access declarations.

```rust
struct MovementSystem;

impl ParallelSystem for MovementSystem {
    fn access() -> SystemAccess {
        SystemAccess::new()
            .read_component::<Velocity>()
            .write_component::<Transform>()
    }

    fn run(&mut self, world: &WorldView<'_>, _commands: &mut WorldCommands) {
        let velocities = world.read::<Velocity>();
        let mut transforms = world.write::<Transform>();
        // parallel-friendly iteration helpers already exist on WorldView
    }
}
```

Medium term: add Bevy-like system params over the same scheduler:

```rust
fn movement(mut q: Query<(&mut Transform, &Velocity)>, time: Res<DeltaTime>) {
    q.par_for_each(|(transform, velocity)| {
        transform.translation += velocity.linear * time.dt;
    });
}
```

The derive/macro layer should only generate `SystemAccess` and typed query adapters. The storage model remains explicit and inspectable.

### High-level spawn API

```rust
let soldier = world.spawn((
    Transform::from_translation(pos),
    RenderMesh::new(meshes.soldier_lod_group),
    RenderMaterial::new(materials.soldier_pbr),
    RenderBounds::from_mesh(meshes.soldier_lod0),
    RenderVisibility::default().shadow_caster(true),
    GameplayTag::Enemy,
));
```

This should allocate a `GpuObjectId` lazily during `RenderExtract`, not synchronously inside gameplay spawn. That keeps `spawn` cheap and thread-safe.

## GPU data layout

### CPU-authored compact object source

Prefer compact SoA/AoSoA over one large AoS if profiling shows better upload/cache behavior. Initial layout can be AoS for simplicity, but API should not expose it.

```rust
#[repr(C)]
pub struct GpuObjectSource {
    pub translation: [f32; 3],
    pub scale_x: f32,
    pub rotation: [f32; 4],
    pub scale_yz_parent_hi: [u32; 4],
    pub mesh_id: u32,
    pub material_id: u32,
    pub flags: u32,
    pub parent_index: u32,
    pub local_bounds_center_radius: [f32; 4],
}
```

Possible simplification:

```rust
pub struct GpuObjectSource {
    pub trs0: [f32; 4], // translation.xyz + scale.x
    pub trs1: [f32; 4], // rotation quat
    pub trs2: [f32; 4], // scale.yz + packed parent/flags via bitcasts or separate u32 buffer
    pub mesh_material_flags: [u32; 4],
    pub local_bounds: [f32; 4],
}
```

### GPU-derived object data

```rust
#[repr(C)]
pub struct GpuObjectDerived {
    pub world: [[f32; 4]; 4],
    pub prev_world: [[f32; 4]; 4],
    pub normal: [[f32; 4]; 3],
    pub world_bounds: [f32; 4],
}
```

Important: the vertex shader can compute `view * world * position` from global view constants and `world`. We do not need to store model-view per object. If a normal matrix is expensive or non-uniform scale is uncommon, normal matrix can be generated only for objects/materials that need it.

### Material table

```rust
#[repr(C)]
pub struct MaterialData {
    pub base_color: [f32; 4],
    pub emissive_roughness_metallic: [f32; 4],
    pub texture_indices: [u32; 8],
    pub sampler_indices: [u32; 4],
    pub flags: u32,
    pub shader_class: u32,
    pub _pad: [u32; 2],
}
```

Materials are data. Shader class selects a small family of shaders, not a unique pipeline per material instance.

### Mesh table

```rust
#[repr(C)]
pub struct MeshDrawData {
    pub index_offset: u32,
    pub vertex_offset: i32,
    pub index_count: u32,
    pub meshlet_offset: u32,
    pub meshlet_count: u32,
    pub lod_offset: u32,
    pub lod_count: u32,
    pub flags: u32,
}
```

Long term, mesh shaders / software raster / virtual geometry consume meshlet tables rather than classic vertices for high-poly content.

## GPU frame pipeline

```text
Gameplay ECS systems (parallel)
  ↓
RenderExtract systems (parallel, command-buffered)
  ↓
Apply RenderWorldCommands
  ↓
Upload dirty object/material/mesh pages
  ↓
GPU transform build
  - local TRS -> world matrix
  - parent hierarchy levels or dirty-root traversal
  - prev_world rotation/copy for motion vectors
  - world bounds
  ↓
Build/update HZB depth pyramid from previous frame
  ↓
GPU cull per view/pass
  - frustum
  - distance/LOD
  - HZB occlusion
  - render layer mask
  - shadow caster flags
  ↓
GPU compact/bin visible work
  - per batch/material/pipeline indirect commands
  - instance remap buffer
  - optional meshlet work queues
  ↓
Draw
  - vkCmdDrawIndexedIndirectCount for classic path
  - mesh/task shaders or compute-raster path for virtual geometry
  ↓
Material resolve / deferred lighting / post
```

## GPU matrix generation

The video you mention likely refers to one of these patterns:

1. **Vertex shader computes final matrix use directly**
   - CPU uploads local/world compact state.
   - Vertex shader reads `ObjectData[object_id].world` and view constants.
   - Cheapest to implement; still needs `world` generated somewhere.

2. **Compute shader expands compact transform to matrices**
   - CPU uploads TRS only for dirty objects.
   - Compute shader writes `world`, `prev_world`, `normal`, `world_bounds`.
   - Vertex shader reads precomputed matrices.
   - Good first target.

3. **GPU hierarchy solve**
   - CPU uploads local TRS + parent index + hierarchy level.
   - Compute dispatch per hierarchy depth, or one kernel over topologically sorted levels.
   - Static hierarchy subtrees can be skipped.
   - This is the correct long-term scene graph path.

4. **Fully GPU-driven animation/skinning**
   - CPU uploads animation parameters and skeleton state.
   - GPU computes bone matrices/skinned bounds into buffers.
   - Required for large crowds.

Recommendation: implement option 2 first, then option 3.

## Batching model

### Persistent batch table

```rust
pub struct RenderBatchKey {
    pub pass: MeshPassKind,
    pub pipeline_class: PipelineClassId,
    pub shader_class: ShaderClassId,
    pub mesh_id: MeshId,
    pub render_state: RenderStateId,
}

pub struct RenderBatch {
    pub key: RenderBatchKey,
    pub indirect_command_offset: u32,
    pub instance_capacity: u32,
}
```

Each object stores its batch ID or computes it from mesh/material pass metadata. Culling writes visible object IDs into the batch's instance-remap span.

### Indirect command strategy

Phase 1:

- One indirect command per batch.
- Compute culling atomically increments `instanceCount`.
- `final_instance_ids[firstInstance + atomic_index] = object_id`.
- CPU loops batches and issues `draw_indexed_indirect` or one multi-draw per pass if backend supports it.

Phase 2:

- `vkCmdDrawIndexedIndirectCount` per pass.
- GPU compacts visible batch commands into a pass command buffer.
- CPU emits one draw-indirect-count per pass.

Phase 3:

- Meshlet/mesh shader path.
- GPU emits meshlet task lists rather than classic indexed draws.

## ECS + renderer thread model

### Thread availability contract

Every public API remains thread-safe:

```rust
Engine::global().materials().create_pbr(desc);
Engine::global().meshes().load("soldier.glb");
world.commands().spawn(bundle);
render_world.commands().set_material(object, material);
scene.commands().set_transform(object, transform);
```

Implementation rule:

- Public APIs enqueue commands or mutate sharded registries.
- Frame boundaries apply commands deterministically.
- GPU resources are created through thread-safe registries with independent locks.
- Queue submission remains serialized internally per Vulkan queue, invisible to callers.

### ECS schedule phases

```text
Startup
FixedUpdate
PreUpdate
Update
PostUpdate
RenderExtract
RenderPrepare
RenderSubmit
```

Only `RenderSubmit` talks to backend command buffers. All earlier render phases are parallel systems over ECS/render-world data.

### Extraction pattern

```rust
fn extract_renderables(
    q: Query<(Entity, &Transform, &RenderMesh, &RenderMaterial, Option<&Parent>)>,
    mut render: ResMut<RenderWorld>,
) {
    q.par_for_each(|(entity, transform, mesh, material, parent)| {
        render.commands().upsert_object(RenderObjectUpdate {
            entity,
            local_transform: transform.compact(),
            parent: parent.map(|p| p.0),
            mesh: mesh.mesh,
            material: material.material,
        });
    });
}
```

The query reads gameplay ECS; commands update render-world staging. It does not block on GPU or submit draw calls.

## API roadmap

### Slice 1 — API/data groundwork

- Add render ECS components:
  - `RenderMesh`
  - `RenderMaterial`
  - `RenderBounds`
  - `RenderVisibility`
  - `PreviousTransform`
- Add `RenderWorld` resource and command queue.
- Add `GpuObjectId` allocator.
- Add tests for thread-safe command enqueue and deterministic apply.

### Slice 2 — GPU object source buffer

- Replace/augment `GpuInstanceData` with:
  - compact source buffer
  - derived matrix/bounds buffer
- Upload only dirty object slots.
- Keep current `Scene` API as compatibility facade over `RenderWorld`.

### Slice 3 — GPU transform build pass

- Add `transform_build.slang` compute shader.
- Dispatch over dirty objects.
- Generate:
  - world matrix
  - previous world matrix
  - normal matrix or normal transform data
  - world-space bounds
- Keep hierarchy to CPU/topological initially; add GPU hierarchy levels later.

### Slice 4 — Single-dispatch culling

- Replace per-batch `Scene::cull_gpu` dispatch with one dispatch per view/pass.
- Input: all renderable object IDs + batch IDs.
- Output:
  - indirect command buffer
  - visible instance remap buffer
  - draw count buffer
- Use HZB when available; frustum-only fallback.

### Slice 5 — Material table + bindless fast path

- Create `MaterialData` GPU table.
- Assign stable material IDs.
- Convert PBR shaders to read material IDs from object/instance data.
- Stop allocating descriptor sets per material on bindless-capable devices.

### Slice 6 — Mega-buffer mesh path

- Pack static meshes into large vertex/index arenas.
- Store mesh offsets in `MeshDrawData`.
- Draw path binds one vertex/index arena per pass.
- Longer term: vertex pulling from storage buffers to support mixed compressed vertex formats.

### Slice 7 — Meshlet/virtual geometry path

- Meshlet generation already exists; integrate with render-world object IDs.
- GPU LOD and cluster selection.
- Optional software raster / visibility buffer / material resolve path for Doom-scale geometric density.

## Success targets

Initial high-end desktop targets:

- 100k renderable entities: CPU render submission below 1 ms.
- 500k renderable entities: CPU submission still mostly flat; GPU cull cost scales linearly and predictably.
- 10M-50M submitted triangles before culling: real frame cost primarily visible triangles + material complexity, not object count.
- Per-object CPU upload: only dirty compact transform/material/visibility state.
- Per-object draw calls: zero.
- Per-object descriptor sets: zero on bindless path.

## Important constraints

- Do not make `Scene` the only authoring path. ECS must become the primary authoring model.
- Do not require users to call render APIs from a render thread.
- Do not expose queue submission or Vulkan synchronization details.
- Do not require users to manually batch entities.
- Do not upload model-view matrices per object. View data is per camera/pass.
- Do not make each material a unique pipeline unless its shader class actually differs.

## Near-term implementation recommendation

The next concrete code slice should be:

1. Add `RenderMesh`, `RenderMaterial`, `RenderBounds`, `RenderVisibility`, and `GpuObjectId` in separate files under `src/scene/` or a new `src/render_world/` module.
2. Add `RenderWorld` and `RenderWorldCommands` as thread-safe resources.
3. Add an extraction system from ECS `Transform` + render components into `RenderWorldCommands`.
4. Keep existing `Scene` rendering working by adapting `Scene` to consume the same `GpuObjectId`/buffer structures.

That slice improves API direction without destabilizing the renderer. The GPU transform/culling pass should follow immediately after.
