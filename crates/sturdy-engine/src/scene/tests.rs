use glam::{Mat4, Vec3};

use super::object::SceneObject;
use super::*;
use crate::ecs::Transform;
use crate::render_world::{
    MaterialId, RenderBounds, RenderMesh, RenderVisibility, RenderWorld, VisibilityFlags,
};
use crate::{BoundingSphere, GpuInstanceData};

#[test]
fn sync_render_world_ignores_invalid_mesh_without_creating_scene_object() {
    let mut scene = Scene::new();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_transform(object, Transform::from_position(Vec3::new(1.0, 2.0, 3.0)));

    assert_eq!(scene.sync_render_world(&render_world), 0);
    assert!(scene.objects.is_empty());
    assert!(scene.render_world_objects.is_empty());
}

#[test]
fn sync_render_world_removes_hidden_mapped_scene_object() {
    let mut scene = Scene::new();
    let render_world = RenderWorld::new();
    let object = render_world.reserve_object();
    let scene_object = ObjectId::from_raw(0);

    scene.objects.push(SceneObject::new(
        MeshId::from_raw(0),
        Mat4::IDENTITY,
        ObjectKind::Dynamic,
    ));
    scene.render_world_objects.insert(object, scene_object);

    render_world.set_mesh(object, RenderMesh::new(MeshId::from_raw(0)));
    render_world.set_visibility(object, RenderVisibility::hidden());

    assert_eq!(scene.sync_render_world(&render_world), 0);
    assert!(scene.render_world_objects.is_empty());
    assert_eq!(scene.objects[0].mesh_id, MeshId::from_raw(u32::MAX));
}

#[test]
fn scene_object_metadata_preserves_render_world_material_bounds_and_flags() {
    let mut object = SceneObject::new(MeshId::from_raw(2), Mat4::IDENTITY, ObjectKind::Dynamic);
    let bounds = RenderBounds::from_sphere(BoundingSphere {
        center: Vec3::new(1.0, 2.0, 3.0),
        radius: 4.0,
    });
    let flags = VisibilityFlags::VISIBLE | VisibilityFlags::RECEIVE_SHADOW;

    object.set_render_metadata(
        Some(bounds),
        Some(MaterialId::from_raw(9).as_u32()),
        flags,
        0.5,
    );

    let metadata = object.instance_metadata(BoundingSphere::EMPTY);
    assert_eq!(metadata.local_sphere.center, Vec3::new(1.0, 2.0, 3.0));
    assert_eq!(metadata.local_sphere.radius, 4.0);
    assert_eq!(metadata.material_id, 9);
    assert_eq!(metadata.lod_bias, 0.5);
    assert!(metadata.flags & GpuInstanceData::FLAG_DYNAMIC != 0);
    assert!(metadata.flags & GpuInstanceData::FLAG_RECEIVE_SHADOW != 0);
    assert_eq!(metadata.flags & GpuInstanceData::FLAG_CAST_SHADOW, 0);
}
