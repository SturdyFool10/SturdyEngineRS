use glam::Vec3;

use crate::ecs::{Transform, World};
use crate::{BackendKind, BoundingSphere, Engine, GpuInstanceData, MeshId};

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
