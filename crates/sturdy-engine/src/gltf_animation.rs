// GLTF skin and animation loading.
//
// Loads `GltfSkin` and `AnimationClip` from a GLTF document. These are
// standalone asset types — they don't require an Engine or GPU resources.
// Upload happens lazily: `AnimationPlayer::upload()` writes bone matrices
// to a GPU buffer each frame.
//
// Usage pattern:
//   let (skins, clips) = load_skins_and_animations(path)?;
//   let player = AnimationPlayer::new(&engine, Arc::new(skins[0]))?;
//   player.play(Arc::new(clips[0])); // "Walk", "Idle", etc.

use std::path::Path;

/// Read a float4x4 accessor's data as `Vec<Mat4>` by walking raw bytes.
fn read_mat4_accessor(acc: &gltf::Accessor<'_>, buffers: &[gltf::buffer::Data]) -> Vec<Mat4> {
    const MAT4_BYTES: usize = 64; // 16 × f32
    let view = match acc.view() {
        Some(v) => v,
        None    => return vec![Mat4::IDENTITY; acc.count()],
    };
    let buf = &buffers[view.buffer().index()].0;
    let stride = view.stride().unwrap_or(MAT4_BYTES);
    let base   = view.offset() + acc.offset();
    (0..acc.count()).map(|i| {
        let s = base + i * stride;
        let bytes = &buf[s..s + MAT4_BYTES];
        let arr: [f32; 16] = bytemuck::pod_read_unaligned(bytes);
        Mat4::from_cols_array(&arr)
    }).collect()
}

use glam::{Mat4, Quat, Vec3};

use crate::animation::{
    AnimationChannel, AnimationClip, AnimationProperty, GltfSkin, Interpolation,
};
use crate::Result;

/// Load all skins and animation clips from a GLTF/GLB file.
///
/// Returns `(skins, clips)`. Most files have one skin and N clips.
/// Mesh data is NOT loaded here — use `engine.load_mesh(path)` for that.
pub fn load_skins_and_animations(
    path: impl AsRef<Path>,
) -> Result<(Vec<GltfSkin>, Vec<AnimationClip>)> {
    let path = path.as_ref();
    let (doc, buffers, _) = gltf::import(path).map_err(|e| {
        crate::Error::Unknown(format!("gltf import '{}': {e}", path.display()))
    })?;

    let skins = load_skins(&doc, &buffers);
    let clips = load_animations(&doc, &buffers);
    Ok((skins, clips))
}

/// Load all skins from an already-imported GLTF document.
pub fn load_skins(doc: &gltf::Document, buffers: &[gltf::buffer::Data]) -> Vec<GltfSkin> {
    let nodes: Vec<gltf::Node<'_>> = doc.nodes().collect();

    doc.skins().map(|skin| {
        let joints: Vec<usize> = skin.joints().map(|j| j.index()).collect();
        let n = joints.len();

        // Inverse bind matrices — read raw bytes from the buffer view.
        let ibm: Vec<Mat4> = if let Some(acc) = skin.inverse_bind_matrices() {
            read_mat4_accessor(&acc, buffers)
        } else {
            vec![Mat4::IDENTITY; n]
        };

        // Build parent map over joint indices (within the skin's joint list).
        let joint_to_skin_idx: std::collections::HashMap<usize, usize> =
            joints.iter().enumerate().map(|(i, &node_idx)| (node_idx, i)).collect();

        let mut parent = vec![None; n];
        let mut bind_t = vec![Vec3::ZERO; n];
        let mut bind_r = vec![Quat::IDENTITY; n];
        let mut bind_s = vec![Vec3::ONE; n];

        for (si, &node_idx) in joints.iter().enumerate() {
            let node = &nodes[node_idx];
            // Decompose bind-pose local transform.
            let (t, r, s) = node.transform().decomposed();
            bind_t[si] = Vec3::from_array(t);
            bind_r[si] = Quat::from_array(r);
            bind_s[si] = Vec3::from_array(s);

            // Find parent among the skin's joints.
            for ch in node.children() {
                if let Some(&child_si) = joint_to_skin_idx.get(&ch.index()) {
                    parent[child_si] = Some(si);
                }
            }
        }

        GltfSkin {
            name: skin.name().unwrap_or("skin").to_owned(),
            joint_count: n,
            inverse_bind_matrices: ibm,
            parent,
            bind_translation: bind_t,
            bind_rotation: bind_r,
            bind_scale: bind_s,
        }
    }).collect()
}

/// Load all animation clips from an already-imported GLTF document.
pub fn load_animations(
    doc: &gltf::Document,
    buffers: &[gltf::buffer::Data],
) -> Vec<AnimationClip> {
    doc.animations().map(|anim| {
        let mut duration = 0.0f32;
        let mut channels = Vec::new();

        for ch in anim.channels() {
            let sampler = ch.sampler();
            let target  = ch.target();

            let interp = match sampler.interpolation() {
                gltf::animation::Interpolation::Linear      => Interpolation::Linear,
                gltf::animation::Interpolation::Step        => Interpolation::Step,
                gltf::animation::Interpolation::CubicSpline => Interpolation::CubicSpline,
            };

            let property = match target.property() {
                gltf::animation::Property::Translation => AnimationProperty::Translation,
                gltf::animation::Property::Rotation    => AnimationProperty::Rotation,
                gltf::animation::Property::Scale       => AnimationProperty::Scale,
                _ => continue, // MorphTargetWeights not yet supported
            };

            let reader = ch.reader(|buf| buffers.get(buf.index()).map(|d| d.0.as_slice()));

            let times: Vec<f32> = match reader.read_inputs() {
                Some(t) => { let v: Vec<f32> = t.collect(); duration = duration.max(v.last().copied().unwrap_or(0.0)); v }
                None => continue,
            };

            let values: Vec<f32> = match reader.read_outputs() {
                Some(gltf::animation::util::ReadOutputs::Translations(it)) => {
                    it.flat_map(|[x,y,z]| [x,y,z]).collect()
                }
                Some(gltf::animation::util::ReadOutputs::Rotations(it)) => {
                    it.into_f32().flat_map(|[x,y,z,w]| [x,y,z,w]).collect()
                }
                Some(gltf::animation::util::ReadOutputs::Scales(it)) => {
                    it.flat_map(|[x,y,z]| [x,y,z]).collect()
                }
                _ => continue,
            };

            channels.push(AnimationChannel {
                joint_index:   target.node().index(),
                property,
                interpolation: interp,
                times,
                values,
            });
        }

        AnimationClip {
            name:     anim.name().unwrap_or("animation").to_owned(),
            duration,
            channels,
        }
    }).collect()
}

/// Load the skinned vertex data (position, normal, uv, tangent, joint indices,
/// weights) for a specific GLTF primitive.
///
/// Returns `None` if the primitive has no JOINTS_0 attribute.
pub fn load_skinned_vertices(
    primitive: &gltf::Primitive<'_>,
    buffers:   &[gltf::buffer::Data],
) -> Option<(Vec<crate::mesh::SkinnedVertex3d>, Vec<u32>)> {
    let reader = primitive.reader(|buf| buffers.get(buf.index()).map(|d| d.0.as_slice()));

    let positions: Vec<[f32; 3]> = reader.read_positions()?.collect();
    let n = positions.len();

    let normals: Vec<[f32; 3]> = reader.read_normals()
        .map(|it| it.collect())
        .unwrap_or_else(|| vec![[0.0, 1.0, 0.0]; n]);

    let uvs: Vec<[f32; 2]> = reader.read_tex_coords(0)
        .map(|it| it.into_f32().collect())
        .unwrap_or_else(|| vec![[0.0, 0.0]; n]);

    // JOINTS_0: 4 joint indices per vertex (u8 or u16), stored as f32 in SkinnedVertex3d.
    let joints: Vec<[f32; 4]> = match reader.read_joints(0)? {
        gltf::mesh::util::ReadJoints::U8(it) =>
            it.map(|[a,b,c,d]| [a as f32, b as f32, c as f32, d as f32]).collect(),
        gltf::mesh::util::ReadJoints::U16(it) =>
            it.map(|[a,b,c,d]| [a as f32, b as f32, c as f32, d as f32]).collect(),
    };

    // WEIGHTS_0: 4 blend weights per vertex.
    let weights: Vec<[f32; 4]> = reader.read_weights(0)?
        .into_f32()
        .collect();

    let tangents: Vec<[f32; 4]> = reader.read_tangents()
        .map(|it| it.collect())
        .unwrap_or_else(|| vec![[1.0, 0.0, 0.0, 1.0]; n]);

    let mut vertices: Vec<crate::mesh::SkinnedVertex3d> = (0..n).map(|i| {
        crate::mesh::SkinnedVertex3d {
            position:  positions[i],
            normal:    normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]),
            uv:        uvs.get(i).copied().unwrap_or([0.0, 0.0]),
            tangent:   tangents.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 1.0]),
            joint_idx: joints.get(i).copied().unwrap_or([0.0; 4]),
            joint_wt:  weights.get(i).copied().unwrap_or([1.0, 0.0, 0.0, 0.0]),
        }
    }).collect();

    // Generate tangents if GLTF didn't provide them.
    if reader.read_tangents().is_none() {
        let indices: Vec<u32> = reader.read_indices()
            .map(|it| it.into_u32().collect())
            .unwrap_or_else(|| (0..n as u32).collect());

        // Convert to Vertex3d, compute tangents, copy back.
        let mut v3d: Vec<crate::Vertex3d> = vertices.iter().map(|v| crate::Vertex3d {
            position: v.position, normal: v.normal, uv: v.uv,
            tangent: [1.0, 0.0, 0.0, 1.0],
        }).collect();
        crate::mesh::compute_tangents(&mut v3d, &indices);
        for (sv, v) in vertices.iter_mut().zip(v3d.iter()) {
            sv.tangent = v.tangent;
        }
    }

    let indices: Vec<u32> = reader.read_indices()
        .map(|it| it.into_u32().collect())
        .unwrap_or_else(|| (0..n as u32).collect());

    Some((vertices, indices))
}
