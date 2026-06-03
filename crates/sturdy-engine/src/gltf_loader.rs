// GLTF 2.0 mesh and material loading.
// Called by mesh_loader::load_mesh_from_path for .gltf / .glb files.

use std::path::Path;
use std::sync::Arc;
use rayon::prelude::*;

use crate::mesh::compute_tangents;
use crate::mesh_loader::{MeshAlphaMode, MeshMaterialParams, MeshPrimitive, MeshTextures};
use crate::{Engine, FrameSyncReason, Image, Mesh, Result, TextureUploadDesc, Vertex3d};

pub(crate) fn load(engine: &Engine, path: &Path) -> Result<Vec<MeshPrimitive>> {
    tracing::info!(path = %path.display(), "loading GLTF");
    let (doc, buffers, images) = gltf::import(path).map_err(|e| {
        tracing::error!(path = %path.display(), "GLTF import failed — file may be missing, corrupt, or use unsupported extensions: {e}");
        crate::Error::Unknown(format!("gltf import '{}': {e}", path.display()))
    })?;
    tracing::debug!(
        meshes = doc.meshes().count(),
        images = images.len(),
        path = %path.display(),
        "GLTF parsed"
    );

    // Upload all images to GPU once; indexed by gltf source image index.
    let gpu_images = upload_images(engine, &images)?;

    let mut out = Vec::new();

    for mesh in doc.meshes() {
        let mesh_name = mesh.name().unwrap_or("mesh").to_owned();
        let multi = mesh.primitives().len() > 1;
        for (slot, primitive) in mesh.primitives().enumerate() {
            let name = if multi {
                format!("{mesh_name}.{slot}")
            } else {
                mesh_name.clone()
            };
            match extract_primitive(engine, &primitive, &buffers, &gpu_images, &name) {
                Ok(p) => out.push(p),
                Err(e) => tracing::warn!("gltf: skipping '{name}' in '{}': {e}", path.display()),
            }
        }
    }
    tracing::info!(path = %path.display(), primitives = out.len(), "GLTF loaded");
    Ok(out)
}

fn upload_images(engine: &Engine, images: &[gltf::image::Data]) -> Result<Vec<Option<Arc<Image>>>> {
    if images.is_empty() {
        return Ok(Vec::new());
    }

    // Phase 1 (parallel): convert all GLTF image formats to RGBA8 on rayon threads.
    // This is pure CPU work (no GPU API calls) and scales with core count.
    let decoded: Vec<Option<(u32, u32, Vec<u8>)>> = images
        .par_iter()
        .enumerate()
        .map(|(i, img)| {
            match decode_to_rgba8(img) {
                Ok(rgba) => Some((img.width, img.height, rgba)),
                Err(e) => {
                    tracing::error!("GLTF: failed to decode image {i}: {e}");
                    None
                }
            }
        })
        .collect();

    // Phase 2 (sequential): upload decoded RGBA8 data to GPU.
    let mut frame = engine.begin_frame()?;
    let mut gpu_images = Vec::with_capacity(images.len());
    for (i, maybe) in decoded.into_iter().enumerate() {
        match maybe {
            Some((w, h, rgba)) => match frame.upload_texture_2d(
                format!("gltf-image-{i}"),
                TextureUploadDesc::sampled_rgba8(w, h),
                &rgba,
            ) {
                Ok(img) => gpu_images.push(Some(Arc::new(img))),
                Err(e) => {
                    tracing::error!("GLTF: failed to upload image {i}: {e}");
                    gpu_images.push(None);
                }
            },
            None => gpu_images.push(None),
        }
    }

    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.recycle_staging_arena();
    Ok(gpu_images)
}

fn decode_to_rgba8(img: &gltf::image::Data) -> Result<Vec<u8>> {
    use gltf::image::Format as GltfFmt;
    Ok(match img.format {
        GltfFmt::R8 => img.pixels.iter().flat_map(|&r| [r, r, r, 255]).collect(),
        GltfFmt::R8G8 => img.pixels.chunks(2).flat_map(|c| [c[0], c[1], 0, 255]).collect(),
        GltfFmt::R8G8B8 => img.pixels.chunks(3).flat_map(|c| [c[0], c[1], c[2], 255]).collect(),
        GltfFmt::R8G8B8A8 => img.pixels.clone(),
        GltfFmt::R16 => img.pixels.chunks(2).flat_map(|c| { let v = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8; [v, v, v, 255] }).collect(),
        GltfFmt::R16G16 => img.pixels.chunks(4).flat_map(|c| { let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8; let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8; [r, g, 0, 255] }).collect(),
        GltfFmt::R16G16B16 => img.pixels.chunks(6).flat_map(|c| { let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8; let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8; let b = (u16::from_le_bytes([c[4], c[5]]) >> 8) as u8; [r, g, b, 255] }).collect(),
        GltfFmt::R16G16B16A16 => img.pixels.chunks(8).flat_map(|c| { let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8; let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8; let b = (u16::from_le_bytes([c[4], c[5]]) >> 8) as u8; let a = (u16::from_le_bytes([c[6], c[7]]) >> 8) as u8; [r, g, b, a] }).collect(),
        GltfFmt::R32G32B32FLOAT => img.pixels.chunks(12).flat_map(|c| { let r = (f32::from_le_bytes([c[0], c[1], c[2], c[3]]).clamp(0.0, 1.0) * 255.0) as u8; let g = (f32::from_le_bytes([c[4], c[5], c[6], c[7]]).clamp(0.0, 1.0) * 255.0) as u8; let b = (f32::from_le_bytes([c[8], c[9], c[10], c[11]]).clamp(0.0, 1.0) * 255.0) as u8; [r, g, b, 255] }).collect(),
        _ => return Err(crate::Error::Unknown(format!("unsupported GLTF image format: {:?}", img.format))),
    })
}

fn _upload_one_image_legacy(
    frame: &mut crate::Frame,
    img: &gltf::image::Data,
    idx: usize,
) -> Result<Image> {
    use gltf::image::Format as GltfFmt;

    let w = img.width;
    let h = img.height;
    tracing::debug!(idx, width = w, height = h, source_format = ?img.format, "uploading GLTF image");

    // Convert to RGBA8 — the only 8-bit format the engine uploads directly.
    let rgba: Vec<u8> = match img.format {
        GltfFmt::R8 => img.pixels.iter().flat_map(|&r| [r, r, r, 255]).collect(),
        GltfFmt::R8G8 => img
            .pixels
            .chunks(2)
            .flat_map(|c| [c[0], c[1], 0, 255])
            .collect(),
        GltfFmt::R8G8B8 => img
            .pixels
            .chunks(3)
            .flat_map(|c| [c[0], c[1], c[2], 255])
            .collect(),
        GltfFmt::R8G8B8A8 => img.pixels.clone(),
        GltfFmt::R16 => img
            .pixels
            .chunks(2)
            .flat_map(|c| {
                let v = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8;
                [v, v, v, 255]
            })
            .collect(),
        GltfFmt::R16G16 => img
            .pixels
            .chunks(4)
            .flat_map(|c| {
                let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8;
                let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8;
                [r, g, 0, 255]
            })
            .collect(),
        GltfFmt::R16G16B16 => img
            .pixels
            .chunks(6)
            .flat_map(|c| {
                let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8;
                let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8;
                let b = (u16::from_le_bytes([c[4], c[5]]) >> 8) as u8;
                [r, g, b, 255]
            })
            .collect(),
        GltfFmt::R16G16B16A16 => img
            .pixels
            .chunks(8)
            .flat_map(|c| {
                let r = (u16::from_le_bytes([c[0], c[1]]) >> 8) as u8;
                let g = (u16::from_le_bytes([c[2], c[3]]) >> 8) as u8;
                let b = (u16::from_le_bytes([c[4], c[5]]) >> 8) as u8;
                let a = (u16::from_le_bytes([c[6], c[7]]) >> 8) as u8;
                [r, g, b, a]
            })
            .collect(),
        GltfFmt::R32G32B32FLOAT => img
            .pixels
            .chunks(12)
            .flat_map(|c| {
                let r =
                    (f32::from_le_bytes([c[0], c[1], c[2], c[3]]).clamp(0.0, 1.0) * 255.0) as u8;
                let g =
                    (f32::from_le_bytes([c[4], c[5], c[6], c[7]]).clamp(0.0, 1.0) * 255.0) as u8;
                let b =
                    (f32::from_le_bytes([c[8], c[9], c[10], c[11]]).clamp(0.0, 1.0) * 255.0) as u8;
                [r, g, b, 255]
            })
            .collect(),
        GltfFmt::R32G32B32A32FLOAT => img
            .pixels
            .chunks(16)
            .flat_map(|c| {
                let r =
                    (f32::from_le_bytes([c[0], c[1], c[2], c[3]]).clamp(0.0, 1.0) * 255.0) as u8;
                let g =
                    (f32::from_le_bytes([c[4], c[5], c[6], c[7]]).clamp(0.0, 1.0) * 255.0) as u8;
                let b =
                    (f32::from_le_bytes([c[8], c[9], c[10], c[11]]).clamp(0.0, 1.0) * 255.0) as u8;
                let a = (f32::from_le_bytes([c[12], c[13], c[14], c[15]]).clamp(0.0, 1.0) * 255.0)
                    as u8;
                [r, g, b, a]
            })
            .collect(),
    };

    frame.upload_texture_2d(
        format!("gltf-image-{idx}"),
        TextureUploadDesc::sampled_rgba8(w, h),
        &rgba,
    )
}

/// Verify that every accessor in `primitive` references a buffer index that
/// exists in `buffers`. Returns an error describing the first out-of-range
/// reference found, naming the offending attribute.
fn validate_buffer_refs(
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    name: &str,
) -> Result<()> {
    let n = buffers.len();
    for (semantic, accessor) in primitive.attributes() {
        if let Some(view) = accessor.view() {
            let idx = view.buffer().index();
            if idx >= n {
                return Err(crate::Error::Unknown(format!(
                    "'{name}': attribute {semantic:?} references buffer {idx} but only {n} \
                     buffer(s) were loaded — file may be corrupt or reference an external \
                     buffer that failed to load"
                )));
            }
        }
    }
    if let Some(accessor) = primitive.indices() {
        if let Some(view) = accessor.view() {
            let idx = view.buffer().index();
            if idx >= n {
                return Err(crate::Error::Unknown(format!(
                    "'{name}': index accessor references buffer {idx} but only {n} buffer(s) \
                     were loaded"
                )));
            }
        }
    }
    Ok(())
}

fn extract_primitive(
    engine: &Engine,
    primitive: &gltf::Primitive<'_>,
    buffers: &[gltf::buffer::Data],
    gpu_images: &[Option<Arc<Image>>],
    name: &str,
) -> Result<MeshPrimitive> {
    // Check upfront that every accessor's buffer view references a buffer that
    // was actually loaded. gltf::import validates document structure but does
    // not guarantee every accessor's buffer view index is within the loaded
    // buffers slice (e.g. sparse accessors without a base buffer view, or
    // externally referenced buffers that failed to load).
    validate_buffer_refs(primitive, buffers, name)?;

    let reader = primitive.reader(|buf| buffers.get(buf.index()).map(|d| d.0.as_slice()));

    let positions: Vec<[f32; 3]> = reader
        .read_positions()
        .ok_or_else(|| crate::Error::Unknown(format!("'{name}' has no POSITION attribute")))?
        .collect();

    // For optional attributes, distinguish "not present in file" (valid — use
    // defaults) from "present but unreadable" (corrupt — warn loudly).
    let claims_normals = primitive
        .attributes()
        .any(|(s, _)| s == gltf::Semantic::Normals);
    let normals: Vec<[f32; 3]> = match reader.read_normals() {
        Some(n) => n.collect(),
        None => {
            if claims_normals {
                tracing::warn!(
                    "'{name}': NORMAL attribute present but unreadable — using up-vector fallback"
                );
            }
            vec![[0.0, 1.0, 0.0]; positions.len()]
        }
    };

    let claims_uvs = primitive
        .attributes()
        .any(|(s, _)| s == gltf::Semantic::TexCoords(0));
    let uvs: Vec<[f32; 2]> = match reader.read_tex_coords(0) {
        Some(u) => u.into_f32().collect(),
        None => {
            if claims_uvs {
                tracing::warn!(
                    "'{name}': TEXCOORD_0 attribute present but unreadable — using zero fallback"
                );
            }
            vec![[0.0, 0.0]; positions.len()]
        }
    };

    let claims_tangents = primitive
        .attributes()
        .any(|(s, _)| s == gltf::Semantic::Tangents);
    let gltf_tangents: Option<Vec<[f32; 4]>> = match reader.read_tangents() {
        Some(t) => Some(t.collect()),
        None => {
            if claims_tangents {
                tracing::warn!(
                    "'{name}': TANGENT attribute present but unreadable — will recompute"
                );
            }
            None
        }
    };

    let n = positions.len();
    let mut vertices: Vec<Vertex3d> = (0..n)
        .map(|i| Vertex3d {
            position: positions[i],
            normal: normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
            uv: uvs.get(i).copied().unwrap_or([0.0, 0.0]),
            tangent: gltf_tangents
                .as_ref()
                .and_then(|t| t.get(i))
                .copied()
                .unwrap_or([1.0, 0.0, 0.0, 1.0]),
        })
        .collect();

    let claims_indices = primitive.indices().is_some();
    let indices: Vec<u32> = match reader.read_indices() {
        Some(idx) => idx.into_u32().collect(),
        None => {
            if claims_indices {
                tracing::warn!(
                    "'{name}': index buffer present but unreadable — falling back to sequential indices"
                );
            }
            (0u32..n as u32).collect()
        }
    };

    // Generate tangents when the GLTF file doesn't supply them.
    if gltf_tangents.is_none() {
        compute_tangents(&mut vertices, &indices);
    }

    let mesh = Mesh::indexed_3d(engine, &vertices, &indices)?;
    let material = &primitive.material();
    let material_params = extract_material(material);
    let textures = extract_textures(material, gpu_images);

    Ok(MeshPrimitive {
        mesh,
        name: name.to_owned(),
        material_params,
        textures,
    })
}

fn extract_material(material: &gltf::Material<'_>) -> MeshMaterialParams {
    let pbr = material.pbr_metallic_roughness();
    let emissive = material.emissive_factor();
    let alpha_mode = match material.alpha_mode() {
        gltf::material::AlphaMode::Opaque => MeshAlphaMode::Opaque,
        gltf::material::AlphaMode::Mask => MeshAlphaMode::Mask,
        gltf::material::AlphaMode::Blend => MeshAlphaMode::Blend,
    };

    // KHR_materials_transmission
    let transmission_factor = material
        .transmission()
        .map(|t| t.transmission_factor())
        .unwrap_or(0.0);

    // KHR_materials_ior
    let ior = material.ior().unwrap_or(1.5);

    // KHR_materials_specular: treat specular_factor as an F0 scale when present
    let _specular_factor = material
        .specular()
        .map(|s| s.specular_factor())
        .unwrap_or(1.0);

    // KHR_materials_volume: attenuation / thick dielectric — captured in transmission
    let _attenuation_color = material
        .volume()
        .map(|v| v.attenuation_color())
        .unwrap_or([1.0, 1.0, 1.0]);

    MeshMaterialParams {
        base_color_factor: pbr.base_color_factor(),
        metallic_factor: pbr.metallic_factor(),
        roughness_factor: pbr.roughness_factor(),
        emissive_factor: [emissive[0], emissive[1], emissive[2]],
        emissive_strength: material.emissive_strength().unwrap_or(1.0),
        double_sided: material.double_sided(),
        alpha_mode,
        alpha_cutoff: material.alpha_cutoff().unwrap_or(0.5),
        unlit: material.unlit(),
        clearcoat_factor: 0.0, // KHR_materials_clearcoat not in gltf 1.x
        clearcoat_roughness_factor: 0.0,
        transmission_factor,
        ior,
        sheen_color_factor: [0.0, 0.0, 0.0], // KHR_materials_sheen not in gltf 1.x
        sheen_roughness_factor: 0.0,
    }
}

fn extract_textures(
    material: &gltf::Material<'_>,
    gpu_images: &[Option<Arc<Image>>],
) -> MeshTextures {
    let pbr = material.pbr_metallic_roughness();

    let lookup = |tex: gltf::texture::Texture<'_>| -> Option<Arc<Image>> {
        gpu_images.get(tex.source().index()).and_then(|v| v.clone())
    };

    // KHR_materials_transmission texture
    let transmission = material
        .transmission()
        .and_then(|t| t.transmission_texture())
        .and_then(|t| lookup(t.texture()));

    MeshTextures {
        base_color: pbr.base_color_texture().and_then(|t| lookup(t.texture())),
        metallic_roughness: pbr
            .metallic_roughness_texture()
            .and_then(|t| lookup(t.texture())),
        normal: material.normal_texture().and_then(|t| lookup(t.texture())),
        occlusion: material
            .occlusion_texture()
            .and_then(|t| lookup(t.texture())),
        emissive: material
            .emissive_texture()
            .and_then(|t| lookup(t.texture())),
        // KHR_materials_clearcoat / sheen not available in gltf 1.x — None for now
        clearcoat: None,
        clearcoat_roughness: None,
        clearcoat_normal: None,
        transmission,
        sheen_color: None,
        sheen_roughness: None,
    }
}
