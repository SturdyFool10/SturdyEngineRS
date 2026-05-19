use glam::{Mat4, Vec3};

use super::object::SceneObject;
use super::*;
use crate::ecs::Transform;
use crate::render_world::{RenderMesh, RenderVisibility, RenderWorld};

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
