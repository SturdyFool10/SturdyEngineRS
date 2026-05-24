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
fn material_expr_swizzles_and_constant_alias_generate_expected_slang() {
    let scalar = MaterialExpr::<[f32; 4]>::texture("packed").g();
    assert_eq!(
        scalar.to_slang_expr(),
        "(((packed.Sample(material_sampler, v.uv))).g)"
    );

    let rgb = MaterialExpr::image_sequence("fire_frames", 8, 24.0).rgb3();
    let rgb_slang = rgb.to_slang_expr();
    assert!(rgb_slang.contains("fire_frames.Sample"));
    assert!(rgb_slang.ends_with(".rgb)"));

    let constant = MaterialExpr::constant([0.25f32, 0.5, 0.75, 1.0]);
    assert_eq!(
        constant.to_slang_expr(),
        "float4(0.250000, 0.500000, 0.750000, 1.000000)"
    );
}

#[test]
fn metallic_roughness_texture_uses_single_packed_gltf_texture_channels() {
    let material = UnifiedMaterial::pbr_metallic_roughness("packed_mr")
        .metallic_roughness_texture("orm")
        .build();
    let source = material.generate_gbuffer_source();

    assert_eq!(source.matches("Texture2D<float4> orm;").count(), 1);
    assert!(source.contains("float metallic = (((orm.Sample(material_sampler, v.uv))).b);"));
    assert!(source.contains("float roughness = (((orm.Sample(material_sampler, v.uv))).g);"));
    assert!(!source.contains("orm__met"));
    assert!(!source.contains("orm__rou"));
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
