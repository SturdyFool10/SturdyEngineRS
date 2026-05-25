use glam::Vec3;

use crate::ecs::{Transform, World};
use crate::{BackendKind, BoundingSphere, Engine, GeometryBackend, GpuInstanceData, MeshId};

use super::*;

#[test]
fn gpu_object_allocator_reuses_released_ids() {
    let allocator = GpuObjectAllocator::new();
    let a = allocator.reserve();
    let b = allocator.reserve();

    assert_ne!(a, b);
    assert_eq!(allocator.allocated_count(), 2);

    allocator.release(a);
    assert_eq!(allocator.allocated_count(), 1);
    assert_eq!(allocator.reserve(), a);
}

#[test]
fn render_world_applies_commands_and_tracks_dirty_slots() {
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();
    render_world.set_transform(object, Transform::from_position(Vec3::new(1.0, 2.0, 3.0)));
    render_world.set_visibility(object, RenderVisibility::hidden());

    assert_eq!(render_world.pending_command_count(), 3);
    assert_eq!(render_world.apply_pending(), 3);
    assert_eq!(render_world.object_count(), 1);

    let state = render_world.object(object).expect("object should exist");
    assert_eq!(state.transform.unwrap().position, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(state.visibility, RenderVisibility::hidden());
    assert!(state.dirty.contains(RenderDirtyFlags::STRUCTURAL));
    assert!(state.dirty.contains(RenderDirtyFlags::TRANSFORM));
    assert!(state.dirty.contains(RenderDirtyFlags::VISIBILITY));

    let dirty = render_world.take_dirty();
    assert_eq!(dirty.len(), 1);
    assert!(render_world.take_dirty().is_empty());
}

#[test]
fn extract_from_world_allocates_links_and_mirrors_components() {
    let mut world = World::new();
    let mesh = MeshId::from_raw(7);
    let entity = world
        .spawn()
        .with(Transform::from_position(Vec3::new(4.0, 5.0, 6.0)))
        .with(RenderMesh::new(mesh))
        .with(RenderMaterial::new(MaterialId::from_raw(3)))
        .with(RenderBounds::from_sphere(BoundingSphere {
            center: Vec3::ZERO,
            radius: 2.0,
        }))
        .with(RenderVisibility::default().shadow_caster(false))
        .id();

    let render_world = RenderWorld::new();
    let stats = render_world.extract_from_world(&mut world);

    assert_eq!(stats.allocated_objects, 1);
    assert_eq!(stats.extracted_entities, 1);
    assert!(stats.applied_commands >= 6);

    let link = *world
        .get::<LocalToWorld>(entity)
        .expect("extraction should insert LocalToWorld");
    let state = render_world
        .object(link.object)
        .expect("extraction should create render object state");

    assert_eq!(state.mesh.unwrap().mesh, mesh);
    assert_eq!(state.material.unwrap().material, MaterialId::from_raw(3));
    assert_eq!(state.transform.unwrap().position, Vec3::new(4.0, 5.0, 6.0));
    assert_eq!(state.bounds.unwrap().local_sphere.radius, 2.0);
    assert!(
        !state
            .visibility
            .flags
            .contains(VisibilityFlags::CAST_SHADOW)
    );
}

#[test]
fn releasing_object_returns_slot_after_apply() {
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();
    render_world.apply_pending();

    render_world.release_object(object);
    render_world.apply_pending();

    let reused = render_world.reserve_object();
    assert_eq!(reused, object);
}

#[test]
fn gpu_scene_data_groups_by_mesh_and_preserves_material_bounds_and_flags() {
    let render_world = RenderWorld::new();
    let object_a = render_world.reserve_object();
    let object_b = render_world.reserve_object();

    render_world.set_mesh(object_a, RenderMesh::new(MeshId::from_raw(1)));
    render_world.set_material(object_a, RenderMaterial::new(MaterialId::from_raw(7)));
    render_world.set_transform(
        object_a,
        Transform::from_position(Vec3::new(10.0, 0.0, 0.0)),
    );
    render_world.set_bounds(
        object_a,
        RenderBounds::from_sphere(BoundingSphere {
            center: Vec3::ZERO,
            radius: 3.0,
        }),
    );
    render_world.set_visibility(object_a, RenderVisibility::default().shadow_caster(false));

    render_world.set_mesh(object_b, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_visibility(object_b, RenderVisibility::hidden());
    render_world.apply_pending();

    let data = render_world.build_gpu_scene_data(2);
    assert_eq!(data.len(), 1);
    assert_eq!(
        data.range_for_mesh(1),
        Some(RenderWorldBatchRange::new(0, 1))
    );
    assert_eq!(data.range_for_mesh(0), None);

    let instance = data.instances[0];
    assert_eq!(instance.mesh_id, 1);
    assert_eq!(instance.material_id, 7);
    assert_eq!(instance.bounds, [10.0, 0.0, 0.0, 3.0]);
    assert!(instance.flags & GpuInstanceData::FLAG_DYNAMIC != 0);
    assert_eq!(instance.flags & GpuInstanceData::FLAG_CAST_SHADOW, 0);
    assert!(instance.flags & GpuInstanceData::FLAG_RECEIVE_SHADOW != 0);
}

#[test]
fn gpu_scene_upload_reuses_slots_for_non_structural_changes() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world.apply_pending();

    let first = render_world.prepare_gpu_scene(&engine, 1).unwrap();
    assert!(first.full_rebuild);
    assert!(first.indirect_reallocated);
    assert_eq!(first.uploaded_instances, 1);

    render_world.set_transform(object, Transform::from_position(Vec3::X));
    let second = render_world.prepare_gpu_scene(&engine, 1).unwrap();
    assert!(!second.full_rebuild);
    assert!(!second.indirect_reallocated);
    assert_eq!(second.uploaded_instances, 1);

    let third = render_world.prepare_gpu_scene(&engine, 1).unwrap();
    assert!(!third.full_rebuild);
    assert_eq!(third.uploaded_instances, 0);
}

#[test]
fn gpu_transform_source_prepare_allocates_sources_and_derived_buffers() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::new(1.0, 2.0, 3.0)));
    render_world.apply_pending();

    let stats = render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();

    assert_eq!(stats.object_count, 1);
    assert!(stats.source_reallocated);
    assert!(stats.derived_reallocated);
    assert!(stats.full_source_upload);
    assert_eq!(stats.uploaded_source_ranges, 1);
    assert_eq!(stats.uploaded_source_objects, 1);
    assert_eq!(
        stats.uploaded_source_bytes,
        std::mem::size_of::<GpuTransformSourceData>() as u64
    );
    assert!(stats.uses_gpu_generation);
    assert!(stats.total_derived_bytes > 0);
    assert_eq!(render_world.gpu_transform_source_slot(object), Some(0));
    render_world.with_gpu_transform_source_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_current_matrix_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_previous_matrix_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_normal_matrix_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_world_bounds_buffer(|buffer| assert!(buffer.is_some()));
}

#[test]
fn gpu_transform_source_prepare_reuses_capacity_and_uploads_dirty_transform_ranges() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object_a = render_world.reserve_object();
    let object_b = render_world.reserve_object();
    let object_c = render_world.reserve_object();

    for object in [object_a, object_b, object_c] {
        render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
        render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    }

    let first = render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    assert!(first.full_source_upload);
    assert_eq!(first.uploaded_source_objects, 3);

    let slot_b = render_world
        .gpu_transform_source_slot(object_b)
        .expect("object_b should have a source slot");
    render_world.set_transform(object_b, Transform::from_position(Vec3::X));

    let second = render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    assert!(!second.source_reallocated);
    assert!(!second.derived_reallocated);
    assert!(!second.full_source_upload);
    assert_eq!(second.uploaded_source_ranges, 1);
    assert_eq!(second.uploaded_source_objects, 1);
    assert_eq!(
        render_world.gpu_transform_source_slot(object_b),
        Some(slot_b)
    );

    let third = render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    assert!(!third.full_source_upload);
    assert_eq!(third.uploaded_source_ranges, 0);
    assert_eq!(third.uploaded_source_objects, 0);
}

#[test]
fn gpu_transform_source_prepare_reports_cpu_matrix_fallback_degradation() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));

    let stats = render_world
        .prepare_gpu_transform_sources(
            &engine,
            1,
            RenderWorldGpuMatrixSettings {
                prefer_gpu_generation: false,
                ..RenderWorldGpuMatrixSettings::default()
            },
        )
        .unwrap();

    assert!(!stats.uses_gpu_generation);
    assert_eq!(stats.total_derived_bytes, 0);
    assert!(
        stats
            .degraded_reason
            .as_deref()
            .unwrap_or_default()
            .contains("disabled by settings")
    );
    render_world.with_gpu_current_matrix_buffer(|buffer| assert!(buffer.is_none()));
}

#[test]
fn gpu_transform_source_prepare_excludes_hidden_and_invalid_mesh_objects() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let visible = render_world.reserve_object();
    let hidden = render_world.reserve_object();
    let invalid_mesh = render_world.reserve_object();

    render_world.set_mesh(visible, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(visible, Transform::from_position(Vec3::ZERO));

    render_world.set_mesh(hidden, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_visibility(hidden, RenderVisibility::hidden());

    render_world.set_mesh(invalid_mesh, RenderMesh::new(MeshId::from_raw(99)));

    let stats = render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();

    assert_eq!(stats.object_count, 1);
    assert_eq!(render_world.gpu_transform_source_slot(visible), Some(0));
    assert_eq!(render_world.gpu_transform_source_slot(hidden), None);
    assert_eq!(render_world.gpu_transform_source_slot(invalid_mesh), None);
}

#[test]
fn gpu_transform_build_pass_dispatches_after_sources_are_prepared() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();

    let pass = RenderWorldGpuTransformBuildPass::new(&engine).unwrap();
    let frame = engine.begin_render_frame().unwrap();
    let stats = pass.execute(&frame, &render_world).unwrap();

    assert!(stats.dispatched);
    assert_eq!(stats.object_count, 1);
    assert_eq!(stats.workgroup_count, 1);
    assert_eq!(stats.objects_per_workgroup, 64);
    assert_eq!(stats.skipped_reason, None);
}

#[test]
fn gpu_transform_build_pass_skips_when_gpu_generation_is_disabled() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world
        .prepare_gpu_transform_sources(
            &engine,
            1,
            RenderWorldGpuMatrixSettings {
                prefer_gpu_generation: false,
                ..RenderWorldGpuMatrixSettings::default()
            },
        )
        .unwrap();

    let pass = RenderWorldGpuTransformBuildPass::new(&engine).unwrap();
    let frame = engine.begin_render_frame().unwrap();
    let stats = pass.execute(&frame, &render_world).unwrap();

    assert!(!stats.dispatched);
    assert_eq!(stats.object_count, 1);
    assert!(stats.skipped_reason.unwrap().contains("disabled"));
}

#[test]
fn gpu_cull_outputs_allocate_visibility_flags_and_plan_single_dispatch() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();

    let stats = render_world
        .prepare_gpu_cull_outputs(&engine, RenderWorldGpuCullSettings::default(), false)
        .unwrap();

    assert_eq!(stats.object_count, 1);
    assert!(stats.visibility_reallocated);
    assert_eq!(stats.output_bytes, std::mem::size_of::<u32>() as u64);
    render_world.with_gpu_visibility_flags_buffer(|buffer| assert!(buffer.is_some()));

    let plan = render_world
        .gpu_cull_plan()
        .expect("cull plan should be stored");
    assert!(plan.uses_gpu_culling);
    assert!(plan.single_dispatch);
    assert_eq!(plan.dispatch_count, 1);
    assert_eq!(plan.workgroup_count, 1);
}

#[test]
fn gpu_cull_pass_dispatches_scene_wide_after_outputs_are_prepared() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    render_world
        .prepare_gpu_cull_outputs(&engine, RenderWorldGpuCullSettings::default(), false)
        .unwrap();

    let pass = RenderWorldGpuCullPass::new(&engine).unwrap();
    let frame = engine.begin_render_frame().unwrap();
    let stats = pass
        .execute(&frame, &render_world, glam::Mat4::IDENTITY, None)
        .unwrap();

    assert!(stats.dispatched);
    assert_eq!(stats.object_count, 1);
    assert_eq!(stats.workgroup_count, 1);
    assert_eq!(stats.objects_per_workgroup, 64);
    assert_eq!(stats.skipped_reason, None);
}

#[test]
fn gpu_cull_pass_skips_when_single_dispatch_is_disabled() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    render_world
        .prepare_gpu_cull_outputs(
            &engine,
            RenderWorldGpuCullSettings {
                prefer_single_dispatch: false,
                ..RenderWorldGpuCullSettings::default()
            },
            false,
        )
        .unwrap();

    let pass = RenderWorldGpuCullPass::new(&engine).unwrap();
    let frame = engine.begin_render_frame().unwrap();
    let stats = pass
        .execute(&frame, &render_world, glam::Mat4::IDENTITY, None)
        .unwrap();

    assert!(!stats.dispatched);
    assert_eq!(stats.object_count, 1);
    assert!(stats.skipped_reason.unwrap().contains("single-dispatch"));
}

fn gbuffer_bins(render_world: &RenderWorld) -> RenderWorldPersistentBins {
    RenderWorldPersistentBins::from_states(
        &render_world.snapshot(),
        GeometryBackend::ComputeIndirect,
        PipelineClass::GBuffer,
        MaterialShaderClass::PbrOpaque,
        VertexLayoutClass::StaticMesh,
        RenderStateClass::OpaqueDepthWrite,
    )
}

#[test]
fn gpu_draw_generation_allocates_bins_indirect_commands_and_count_buffer() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object_a = render_world.reserve_object();
    let object_b = render_world.reserve_object();

    render_world.set_mesh(object_a, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object_a, Transform::from_position(Vec3::ZERO));
    render_world.set_mesh(object_b, RenderMesh::new(MeshId::from_raw(1)));
    render_world.set_transform(object_b, Transform::from_position(Vec3::X));
    render_world
        .prepare_gpu_transform_sources(&engine, 2, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    render_world
        .prepare_gpu_cull_outputs(&engine, RenderWorldGpuCullSettings::default(), false)
        .unwrap();

    let bins = gbuffer_bins(&render_world);
    let stats = render_world
        .prepare_gpu_draw_generation(
            &engine,
            &bins,
            [
                RenderWorldGpuMeshDrawInfo::indexed(MeshId::from_raw(0), 36),
                RenderWorldGpuMeshDrawInfo::indexed(MeshId::from_raw(1), 72),
            ],
            false,
        )
        .unwrap();

    assert_eq!(stats.bin_count, 2);
    assert_eq!(stats.object_count, 2);
    assert!(stats.bin_buffer_reallocated);
    assert!(stats.indirect_buffer_reallocated);
    assert!(stats.count_buffer_reallocated);
    assert!(stats.visible_instance_buffer_reallocated);
    assert_eq!(
        stats.uploaded_bin_bytes,
        2 * std::mem::size_of::<RenderWorldGpuBinData>() as u64
    );
    assert_eq!(
        stats.visible_instance_bytes,
        2 * std::mem::size_of::<u32>() as u64
    );
    assert!(!stats.uses_indirect_count);
    assert!(
        stats
            .degraded_reason
            .unwrap()
            .contains("indirect-count unavailable")
    );
    render_world.with_gpu_draw_bin_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_draw_indirect_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_draw_count_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_visible_instance_buffer(|buffer| assert!(buffer.is_some()));
    render_world.with_gpu_draw_output(|output| {
        let output = output.expect("draw output should include visible-instance remap");
        assert_eq!(output.max_draw_count, 2);
        assert!(!output.use_indirect_count);
        assert_eq!(
            output.visible_instances.desc().size,
            4 * std::mem::size_of::<u32>() as u64
        );
    });
}

#[test]
fn gpu_draw_generation_pass_dispatches_after_prepare() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world
        .prepare_gpu_transform_sources(&engine, 1, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    render_world
        .prepare_gpu_cull_outputs(&engine, RenderWorldGpuCullSettings::default(), false)
        .unwrap();
    let bins = gbuffer_bins(&render_world);
    render_world
        .prepare_gpu_draw_generation(
            &engine,
            &bins,
            [RenderWorldGpuMeshDrawInfo::indexed(MeshId::from_raw(0), 36)],
            true,
        )
        .unwrap();

    let pass = RenderWorldGpuDrawGenerationPass::new(&engine).unwrap();
    let frame = engine.begin_render_frame().unwrap();
    let stats = pass.execute(&frame, &render_world).unwrap();

    assert!(stats.dispatched);
    assert_eq!(stats.bin_count, 1);
    assert_eq!(stats.object_count, 1);
    assert_eq!(stats.workgroup_count, 1);
    assert_eq!(stats.bins_per_workgroup, 64);
    assert_eq!(stats.skipped_reason, None);

    let report = frame.describe();
    assert!(report.passes.iter().any(|pass| {
        pass.buffer_writes
            .iter()
            .any(|name| name == "render_world_visible_instances")
    }));
}

#[test]
fn gpu_draw_generation_reports_missing_mesh_draw_info() {
    let engine = Engine::with_backend(BackendKind::Null).unwrap();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(7)));
    render_world.set_transform(object, Transform::from_position(Vec3::ZERO));
    render_world
        .prepare_gpu_transform_sources(&engine, 8, RenderWorldGpuMatrixSettings::default())
        .unwrap();
    render_world
        .prepare_gpu_cull_outputs(&engine, RenderWorldGpuCullSettings::default(), false)
        .unwrap();
    let bins = gbuffer_bins(&render_world);

    let err = render_world
        .prepare_gpu_draw_generation(&engine, &bins, [], true)
        .unwrap_err();
    assert!(err.to_string().contains("missing GPU draw info"));
}
