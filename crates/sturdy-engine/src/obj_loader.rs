// Wavefront OBJ + MTL mesh loading via the `tobj` crate.
// Called by mesh_loader::load_mesh_from_path for .obj files.
//
// Each OBJ group/object becomes one MeshPrimitive. Material parameters are
// approximated from MTL Kd (diffuse) → base_color, d (dissolve) → opacity,
// and the PBR extensions Pr/Pm (roughness/metallic) when present.

use std::path::Path;

use crate::{Engine, Mesh, Result, Vertex3d};
use crate::mesh_loader::{MeshAlphaMode, MeshMaterialParams, MeshPrimitive};

pub(crate) fn load(engine: &Engine, path: &Path) -> Result<Vec<MeshPrimitive>> {
    let load_opts = tobj::LoadOptions {
        triangulate: true,
        single_index: true,
        ..Default::default()
    };

    let (models, materials_result) = tobj::load_obj(path, &load_opts).map_err(|e| {
        crate::Error::Unknown(format!("obj load '{}': {e}", path.display()))
    })?;

    let materials = materials_result.unwrap_or_default();

    let mut out = Vec::new();
    for model in &models {
        let mesh_data = &model.mesh;
        let name = model.name.clone();

        // Build vertex list. With single_index, all attribute arrays are indexed
        // by mesh.indices. Normals and texcoords are optional in OBJ.
        let has_normals  = !mesh_data.normals.is_empty();
        let has_uvs      = !mesh_data.texcoords.is_empty();

        // Vertex count from indices (triangulate ensures multiples of 3).
        let index_count = mesh_data.indices.len();

        let mut vertices: Vec<Vertex3d> = Vec::with_capacity(index_count);
        for &idx in &mesh_data.indices {
            let i = idx as usize;
            let position = [
                mesh_data.positions[i * 3],
                mesh_data.positions[i * 3 + 1],
                mesh_data.positions[i * 3 + 2],
            ];
            let normal = if has_normals && i * 3 + 2 < mesh_data.normals.len() {
                [
                    mesh_data.normals[i * 3],
                    mesh_data.normals[i * 3 + 1],
                    mesh_data.normals[i * 3 + 2],
                ]
            } else {
                [0.0, 1.0, 0.0]
            };
            let uv = if has_uvs && i * 2 + 1 < mesh_data.texcoords.len() {
                [
                    mesh_data.texcoords[i * 2],
                    // OBJ has V=0 at bottom; flip to match GPU convention (V=0 at top).
                    1.0 - mesh_data.texcoords[i * 2 + 1],
                ]
            } else {
                [0.0, 0.0]
            };
            vertices.push(Vertex3d { position, normal, uv });
        }

        // With single_index the mesh is already de-indexed into our vertex list;
        // generate sequential indices.
        let indices: Vec<u32> = (0u32..vertices.len() as u32).collect();

        let mesh = match Mesh::indexed_3d(engine, &vertices, &indices) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[obj] skipping model '{name}': {e}");
                continue;
            }
        };

        // Material: look up by index if available.
        let material_params = mesh_data
            .material_id
            .and_then(|mid| materials.get(mid))
            .map(extract_material)
            .unwrap_or_default();

        out.push(MeshPrimitive { mesh, name, material_params });
    }

    Ok(out)
}

fn extract_material(mat: &tobj::Material) -> MeshMaterialParams {
    // Base color from Kd (diffuse). Fall back to white.
    let kd = mat.diffuse.unwrap_or([1.0, 1.0, 1.0]);

    // Opacity from `d` (dissolve). 1.0 = fully opaque.
    let opacity = mat.dissolve.unwrap_or(1.0);
    let alpha_mode = if opacity < 1.0 { MeshAlphaMode::Blend } else { MeshAlphaMode::Opaque };

    // Roughness approximated from Ns (shininess): higher shininess → lower roughness.
    // Ns range in OBJ is typically [0, 1000]; map to perceptual roughness [0, 1].
    let roughness = mat.shininess
        .map(|ns| 1.0 - (ns.min(1000.0) / 1000.0).sqrt())
        .unwrap_or(0.5);

    // No metallic field in standard OBJ/MTL; default to dielectric.
    let metallic = 0.0_f32;

    // Emissive: tobj 4.x doesn't expose Ke directly. Check unknown params map.
    // Provide zero as the safe default; artists can override with scene.set_material().
    let ke = [0.0_f32; 3];

    MeshMaterialParams {
        base_color_factor: [kd[0], kd[1], kd[2], opacity],
        metallic_factor:   metallic,
        roughness_factor:  roughness,
        emissive_factor:   ke,
        emissive_strength: 1.0,
        double_sided:      false,
        alpha_mode,
        alpha_cutoff:      0.5,
        unlit:             false,
    }
}
