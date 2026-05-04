// GLTF 2.0 mesh and material loading.
// Called by mesh_loader::load_mesh_from_path for .gltf / .glb files.

use std::path::Path;

use crate::{Engine, Mesh, Result, Vertex3d};
use crate::mesh_loader::{MeshAlphaMode, MeshMaterialParams, MeshPrimitive};

pub(crate) fn load(engine: &Engine, path: &Path) -> Result<Vec<MeshPrimitive>> {
    let (doc, buffers, _images) = gltf::import(path).map_err(|e| {
        crate::Error::Unknown(format!("gltf import '{}': {e}", path.display()))
    })?;

    let mut out = Vec::new();

    for mesh in doc.meshes() {
        let mesh_name = mesh.name().unwrap_or("mesh").to_owned();
        let multi = mesh.primitives().len() > 1;
        for (slot, primitive) in mesh.primitives().enumerate() {
            let name = if multi { format!("{mesh_name}.{slot}") } else { mesh_name.clone() };
            match extract_primitive(engine, &primitive, &buffers, &name) {
                Ok(p) => out.push(p),
                Err(e) => eprintln!("[gltf] skipping '{name}' in '{}': {e}", path.display()),
            }
        }
    }
    Ok(out)
}

fn extract_primitive(
    engine: &Engine,
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    name: &str,
) -> Result<MeshPrimitive> {
    let reader = primitive.reader(|buf| buffers.get(buf.index()).map(|d| d.0.as_slice()));

    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or_else(|| crate::Error::Unknown(format!("'{name}' has no POSITION attribute")))?
        .collect();

    let normals: Vec<[f32; 3]> = reader
        .read_normals()
        .map(|n| n.collect())
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; positions.len()]);

    let uvs: Vec<[f32; 2]> = reader
        .read_tex_coords(0)
        .map(|u| u.into_f32().collect())
        .unwrap_or_else(|| vec![[0.0, 0.0]; positions.len()]);

    let n = positions.len();
    let vertices: Vec<Vertex3d> = (0..n)
        .map(|i| Vertex3d {
            position: positions[i],
            normal:   normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
            uv:       uvs.get(i).copied().unwrap_or([0.0, 0.0]),
        })
        .collect();

    let indices: Vec<u32> = reader
        .read_indices()
        .map(|idx| idx.into_u32().collect())
        .unwrap_or_else(|| (0u32..n as u32).collect());

    let mesh = Mesh::indexed_3d(engine, &vertices, &indices)?;
    let material_params = extract_material(&primitive.material());

    Ok(MeshPrimitive { mesh, name: name.to_owned(), material_params })
}

fn extract_material(material: &gltf::Material<'_>) -> MeshMaterialParams {
    let pbr = material.pbr_metallic_roughness();
    let emissive = material.emissive_factor();
    let alpha_mode = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => MeshAlphaMode::Opaque,
        gltf::material::AlphaMode::Mask   => MeshAlphaMode::Mask,
        gltf::material::AlphaMode::Blend  => MeshAlphaMode::Blend,
    };
    MeshMaterialParams {
        base_color_factor: pbr.base_color_factor(),
        metallic_factor:   pbr.metallic_factor(),
        roughness_factor:  pbr.roughness_factor(),
        emissive_factor:   [emissive[0], emissive[1], emissive[2]],
        emissive_strength: material.emissive_strength().unwrap_or(1.0),
        double_sided:      material.double_sided(),
        alpha_mode,
        alpha_cutoff:      material.alpha_cutoff().unwrap_or(0.5),
        unlit:             material.unlit(),
    }
}
