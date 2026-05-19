use glam::Vec3;

use crate::ecs::{Transform, World};
use crate::{BoundingSphere, MeshId};

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
