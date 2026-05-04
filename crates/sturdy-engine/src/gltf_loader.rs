// GLTF 2.0 mesh and material loading.
// Called by mesh_loader::load_mesh_from_path for .gltf / .glb files.

use std::sync::Arc;
use std::path::Path;

use crate::{Engine, FrameSyncReason, Image, Mesh, Result, TextureUploadDesc, Vertex3d};
use crate::mesh::compute_tangents;
use crate::mesh_loader::{MeshAlphaMode, MeshMaterialParams, MeshPrimitive, MeshTextures};

pub(crate) fn load(engine: &Engine, path: &Path) -> Result<Vec<MeshPrimitive>> {
    let (doc, buffers, images) = gltf::import(path).map_err(|e| {
        crate::Error::Unknown(format!("gltf import '{}': {e}", path.display()))
    })?;

    // Upload all images to GPU once; indexed by gltf source image index.
    let gpu_images = upload_images(engine, &images)?;

    let mut out = Vec::new();

    for mesh in doc.meshes() {
        let mesh_name = mesh.name().unwrap_or("mesh").to_owned();
        let multi = mesh.primitives().len() > 1;
        for (slot, primitive) in mesh.primitives().enumerate() {
            let name = if multi { format!("{mesh_name}.{slot}") } else { mesh_name.clone() };
            match extract_primitive(engine, &primitive, &buffers, &gpu_images, &name) {
                Ok(p) => out.push(p),
                Err(e) => eprintln!("[gltf] skipping '{name}' in '{}': {e}", path.display()),
            }
        }
    }
    Ok(out)
}

fn upload_images(engine: &Engine, images: &[gltf::image::Data]) -> Result<Vec<Option<Arc<Image>>>> {
    if images.is_empty() {
        return Ok(Vec::new());
    }

    let mut frame = engine.begin_frame()?;
    let mut gpu_images: Vec<Option<Arc<Image>>> = Vec::with_capacity(images.len());

    for (i, img) in images.iter().enumerate() {
        match upload_one_image(&mut frame, img, i) {
            Ok(image) => gpu_images.push(Some(Arc::new(image))),
            Err(e) => {
                eprintln!("[gltf] failed to upload image {i}: {e}");
                gpu_images.push(None);
            }
        }
    }

    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    Ok(gpu_images)
}

fn upload_one_image(frame: &mut crate::Frame, img: &gltf::image::Data, idx: usize) -> Result<Image> {
    use gltf::image::Format as GltfFmt;

    let w = img.width;
    let h = img.height;

    // Convert to RGBA8 — the only 8-bit format the engine uploads directly.
    let rgba: Vec<u8> = match img.format {
        GltfFmt::R8 => img.pixels.iter().flat_map(|&r| [r, r, r, 255]).collect(),
        GltfFmt::R8G8 => img.pixels.chunks(2).flat_map(|c| [c[0], c[1], 0, 255]).collect(),
        GltfFmt::R8G8B8 => img.pixels.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        GltfFmt::R8G8B8A8 => img.pixels.clone(),
        GltfFmt::R16 => {
            img.pixels.chunks(2)
                .flat_map(|c| { let v = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8; [v, v, v, 255] })
                .collect()
        }
        GltfFmt::R16G16 => {
            img.pixels.chunks(4)
                .flat_map(|c| {
                    let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8;
                    let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8;
                    [r, g, 0, 255]
                })
                .collect()
        }
        GltfFmt::R16G16B16 => {
            img.pixels.chunks(6)
                .flat_map(|c| {
                    let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8;
                    let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8;
                    let b = (u16::from_le_bytes([c[4], c[5]]) >> 8) as u8;
                    [r, g, b, 255]
                })
                .collect()
        }
        GltfFmt::R16G16B16A16 => {
            img.pixels.chunks(8)
                .flat_map(|c| {
                    let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8;
                    let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8;
                    let b = (u16::from_le_bytes([c[4], c[5]]) >> 8) as u8;
                    let a = (u16::from_le_bytes([c[6], c[7]]) >> 8) as u8;
                    [r, g, b, a]
                })
                .collect()
        }
        GltfFmt::R32G32B32FLOAT => {
            img.pixels.chunks(12)
                .flat_map(|c| {
                    let r = (f32::from_le_bytes([c[0],c[1],c[2],c[3]]).clamp(0.0,1.0) * 255.0) as u8;
                    let g = (f32::from_le_bytes([c[4],c[5],c[6],c[7]]).clamp(0.0,1.0) * 255.0) as u8;
                    let b = (f32::from_le_bytes([c[8],c[9],c[10],c[11]]).clamp(0.0,1.0) * 255.0) as u8;
                    [r, g, b, 255]
                })
                .collect()
        }
        GltfFmt::R32G32B32A32FLOAT => {
            img.pixels.chunks(16)
                .flat_map(|c| {
                    let r = (f32::from_le_bytes([c[0],c[1],c[2],c[3]]).clamp(0.0,1.0) * 255.0) as u8;
                    let g = (f32::from_le_bytes([c[4],c[5],c[6],c[7]]).clamp(0.0,1.0) * 255.0) as u8;
                    let b = (f32::from_le_bytes([c[8],c[9],c[10],c[11]]).clamp(0.0,1.0) * 255.0) as u8;
                    let a = (f32::from_le_bytes([c[12],c[13],c[14],c[15]]).clamp(0.0,1.0) * 255.0) as u8;
                    [r, g, b, a]
                })
                .collect()
        }
    };

    frame.upload_texture_2d(
        format!("gltf-image-{idx}"),
        TextureUploadDesc::sampled_rgba8(w, h),
        &rgba,
    )
}

fn extract_primitive(
    engine: &Engine,
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    gpu_images: &[Option<Arc<Image>>],
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

    let gltf_tangents: Option<Vec<[f32; 4]>> = reader
        .read_tangents()
        .map(|t| t.collect());

    let n = positions.len();
    let mut vertices: Vec<Vertex3d> = (0..n)
        .map(|i| Vertex3d {
            position: positions[i],
            normal:   normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
            uv:       uvs.get(i).copied().unwrap_or([0.0, 0.0]),
            tangent:  gltf_tangents.as_ref()
                        .and_then(|t| t.get(i))
                        .copied()
                        .unwrap_or([1.0, 0.0, 0.0, 1.0]),
        })
        .collect();

    let indices: Vec<u32> = reader
        .read_indices()
        .map(|idx| idx.into_u32().collect())
        .unwrap_or_else(|| (0u32..n as u32).collect());

    // Generate tangents when the GLTF file doesn't supply them.
    if gltf_tangents.is_none() {
        compute_tangents(&mut vertices, &indices);
    }

    let mesh = Mesh::indexed_3d(engine, &vertices, &indices)?;
    let material = &primitive.material();
    let material_params = extract_material(material);
    let textures = extract_textures(material, gpu_images);

    Ok(MeshPrimitive { mesh, name: name.to_owned(), material_params, textures })
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

fn extract_textures(material: &gltf::Material<'_>, gpu_images: &[Option<Arc<Image>>]) -> MeshTextures {
    let pbr = material.pbr_metallic_roughness();

    let lookup = |tex: gltf::texture::Texture<'_>| -> Option<Arc<Image>> {
        gpu_images.get(tex.source().index()).and_then(|v| v.clone())
    };

    MeshTextures {
        base_color:        pbr.base_color_texture().and_then(|t| lookup(t.texture())),
        metallic_roughness: pbr.metallic_roughness_texture().and_then(|t| lookup(t.texture())),
        normal:            material.normal_texture().and_then(|t| lookup(t.texture())),
        occlusion:         material.occlusion_texture().and_then(|t| lookup(t.texture())),
        emissive:          material.emissive_texture().and_then(|t| lookup(t.texture())),
    }
}
