// STereoLithography (STL) mesh loading via the `stl_io` crate.
// Supports both binary and ASCII STL.
// Called by mesh_loader::load_mesh_from_path for .stl files.
//
// STL has no materials, UVs, or named groups. Each file produces exactly one
// MeshPrimitive with default PBR parameters (white, roughness 0.5, metallic 0).
// Face normals from the STL header are used as vertex normals.

use std::{fs::File, io::BufReader, path::Path};

use crate::{Engine, Mesh, Result, Vertex3d};
use crate::mesh_loader::{MeshMaterialParams, MeshPrimitive};

pub(crate) fn load(engine: &Engine, path: &Path) -> Result<Vec<MeshPrimitive>> {
    let file = File::open(path).map_err(|e| {
        crate::Error::Unknown(format!("stl open '{}': {e}", path.display()))
    })?;
    let mut reader = BufReader::new(file);

    let stl = stl_io::read_stl(&mut reader).map_err(|e| {
        crate::Error::Unknown(format!("stl parse '{}': {e}", path.display()))
    })?;

    // Each STL triangle has 3 vertices and one face normal shared by all 3.
    // We de-duplicate by building unique vertices (pos + normal) and an index buffer.
    // For simplicity in this first implementation, each triangle becomes 3 unique
    // vertices (no vertex sharing). Proper de-duplication is a follow-up.
    let triangle_count = stl.faces.len();
    let mut vertices: Vec<Vertex3d> = Vec::with_capacity(triangle_count * 3);
    let mut indices: Vec<u32> = Vec::with_capacity(triangle_count * 3);

    for (i, face) in stl.faces.iter().enumerate() {
        let n = face.normal;
        let normal: [f32; 3] = [n[0], n[1], n[2]];

        for vi in 0..3 {
            let v = stl.vertices[face.vertices[vi]];
            vertices.push(Vertex3d {
                position: [v[0], v[1], v[2]],
                normal,
                uv: [0.0, 0.0],
            });
            indices.push((i * 3 + vi) as u32);
        }
    }

    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("stl_mesh")
        .to_owned();

    let mesh = Mesh::indexed_3d(engine, &vertices, &indices)?;
    Ok(vec![MeshPrimitive {
        mesh,
        name,
        material_params: MeshMaterialParams::default(),
    }])
}
