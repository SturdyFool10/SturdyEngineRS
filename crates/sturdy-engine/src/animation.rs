// Skeletal animation — Linear Blend Skinning (LBS).
//
// Core types:
//   AnimationClip  — time-sampled keyframe data for a single named animation
//   GltfSkin       — joint hierarchy + inverse bind matrices from a GLTF skin
//   AnimationPlayer — advances time, samples keyframes, writes bone matrices to GPU
//
// The GPU bone buffer (float4x4 × joint_count) is bound as "bone_matrices"
// before each skinned draw. The skinned vertex shader blends up to 4 joints
// per vertex using standard LBS.
//
// Roadmap: Track 1 — game loop shell (skinned character support).

use crate::{Buffer, BufferDesc, BufferUsage, Engine, Result};
use glam::{Mat4, Quat, Vec3};
use std::sync::Arc;

// ── Keyframe interpolation ────────────────────────────────────────────────────

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Interpolation {
    Linear,
    Step,
    /// CubicSpline approximated as linear (correct tangent storage is a follow-up).
    CubicSpline,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum AnimationProperty {
    Translation,
    Rotation,
    Scale,
}

// ── AnimationChannel ──────────────────────────────────────────────────────────

/// One animation channel: time samples for a single property of a single joint.
pub struct AnimationChannel {
    pub joint_index: usize,
    pub property: AnimationProperty,
    pub interpolation: Interpolation,
    /// Keyframe timestamps in seconds.
    pub times: Vec<f32>,
    /// Flat packed values: 3 floats per sample for Translation/Scale, 4 for Rotation (xyzw).
    pub values: Vec<f32>,
}

impl AnimationChannel {
    fn vps(&self) -> usize {
        match self.property {
            AnimationProperty::Rotation => 4,
            AnimationProperty::Translation | AnimationProperty::Scale => 3,
        }
    }

    /// Sample the channel at time `t` (clamped to the clip range).
    /// Returns 3 or 4 floats depending on `property`.
    fn sample(&self, t: f32) -> Vec<f32> {
        let n = self.times.len();
        if n == 0 {
            return vec![0.0; self.vps()];
        }
        let vps = self.vps();

        if t <= self.times[0] {
            return self.values[..vps].to_vec();
        }
        if t >= self.times[n - 1] {
            let s = (n - 1) * vps;
            return self.values[s..s + vps].to_vec();
        }

        let hi = self.times.partition_point(|&s| s <= t);
        let lo = hi - 1;
        let alpha = (t - self.times[lo]) / (self.times[hi] - self.times[lo]).max(1e-7);
        let v0 = &self.values[lo * vps..lo * vps + vps];
        let v1 = &self.values[hi * vps..hi * vps + vps];

        match (self.interpolation, self.property) {
            (Interpolation::Step, _) => v0.to_vec(),
            (_, AnimationProperty::Rotation) => {
                let q0 = Quat::from_xyzw(v0[0], v0[1], v0[2], v0[3]);
                let q1 = Quat::from_xyzw(v1[0], v1[1], v1[2], v1[3]);
                let q = q0.slerp(q1, alpha);
                vec![q.x, q.y, q.z, q.w]
            }
            _ => v0
                .iter()
                .zip(v1.iter())
                .map(|(a, b)| a + (b - a) * alpha)
                .collect(),
        }
    }
}

// ── AnimationClip ─────────────────────────────────────────────────────────────

/// One named animation (e.g. "Idle", "Walk", "Jump"). Loaded from GLTF.
///
/// Share via `Arc<AnimationClip>` across multiple players.
pub struct AnimationClip {
    pub name: String,
    /// Total duration in seconds.
    pub duration: f32,
    pub channels: Vec<AnimationChannel>,
}

// ── GltfSkin ──────────────────────────────────────────────────────────────────

/// Joint hierarchy from a GLTF skin.
///
/// Loaded alongside a skinned mesh. Pass an `Arc<GltfSkin>` to
/// [`AnimationPlayer::new`].
pub struct GltfSkin {
    pub name: String,
    pub joint_count: usize,
    /// Per-joint: mesh-bind-pose → joint-local-space transform (inverse of the
    /// bind-pose global transform). One matrix per joint.
    pub inverse_bind_matrices: Vec<Mat4>,
    /// Parent joint index. `None` = root joint.
    pub parent: Vec<Option<usize>>,
    /// Bind-pose local translation per joint.
    pub bind_translation: Vec<Vec3>,
    /// Bind-pose local rotation per joint.
    pub bind_rotation: Vec<Quat>,
    /// Bind-pose local scale per joint.
    pub bind_scale: Vec<Vec3>,
}

impl GltfSkin {
    /// Compute the per-joint bone matrices given current local transforms.
    ///
    /// `bone_matrix[i] = global_joint_transform[i] × inverse_bind_matrix[i]`
    pub fn compute_bone_matrices(&self, t: &[Vec3], r: &[Quat], s: &[Vec3]) -> Vec<Mat4> {
        let n = self.joint_count;
        let mut global = vec![Mat4::IDENTITY; n];
        for i in 0..n {
            let lt = if i < t.len() {
                t[i]
            } else {
                self.bind_translation[i]
            };
            let lr = if i < r.len() {
                r[i]
            } else {
                self.bind_rotation[i]
            };
            let ls = if i < s.len() {
                s[i]
            } else {
                self.bind_scale[i]
            };
            let local = Mat4::from_scale_rotation_translation(ls, lr, lt);
            global[i] = match self.parent[i] {
                Some(p) => global[p] * local,
                None => local,
            };
        }
        global
            .iter()
            .zip(&self.inverse_bind_matrices)
            .map(|(g, ibm)| *g * *ibm)
            .collect()
    }
}

// ── AnimationPlayer ───────────────────────────────────────────────────────────

/// Drives one [`AnimationClip`] on one [`GltfSkin`] and uploads bone matrices
/// to the GPU each frame.
///
/// # Usage
/// ```ignore
/// let mut player = AnimationPlayer::new(&engine, Arc::clone(&skin))?;
/// player.play(Arc::clone(&walk_clip));
///
/// // Game loop:
/// player.advance(ctx.dt());
/// player.upload(&engine)?;
/// frame.bind_buffer("bone_matrices", player.bone_buffer().unwrap());
/// // then draw the skinned mesh normally.
/// ```
pub struct AnimationPlayer {
    skin: Arc<GltfSkin>,
    clip: Option<Arc<AnimationClip>>,
    /// Current playback position in seconds.
    pub time: f32,
    /// Playback rate multiplier. Default 1.0.
    pub speed: f32,
    /// Loop the clip when it reaches the end. Default true.
    pub looping: bool,
    /// True when a non-looping clip has reached its end.
    pub finished: bool,

    joint_t: Vec<Vec3>,
    joint_r: Vec<Quat>,
    joint_s: Vec<Vec3>,
    /// Current bone matrices in mesh space. Updated by `advance()`.
    pub bone_matrices: Vec<Mat4>,
    gpu_buffer: Option<Buffer>,
}

impl AnimationPlayer {
    pub fn new(engine: &Engine, skin: Arc<GltfSkin>) -> Result<Self> {
        let n = skin.joint_count;
        let joint_t = skin.bind_translation.clone();
        let joint_r = skin.bind_rotation.clone();
        let joint_s = skin.bind_scale.clone();
        let bone_matrices = skin.compute_bone_matrices(&joint_t, &joint_r, &joint_s);
        let gpu_buffer = engine.create_buffer(BufferDesc {
            size: (n * std::mem::size_of::<Mat4>()) as u64,
            usage: BufferUsage::STORAGE,
        })?;
        let p = Self {
            skin,
            clip: None,
            time: 0.0,
            speed: 1.0,
            looping: true,
            finished: false,
            joint_t,
            joint_r,
            joint_s,
            bone_matrices,
            gpu_buffer: Some(gpu_buffer),
        };
        p.flush_to_gpu();
        Ok(p)
    }

    /// Start playing `clip` from time 0.
    pub fn play(&mut self, clip: Arc<AnimationClip>) {
        self.clip = Some(clip);
        self.time = 0.0;
        self.finished = false;
    }

    /// Stop playback and return joints to bind pose.
    pub fn stop(&mut self) {
        self.clip = None;
        self.time = 0.0;
        self.finished = false;
        self.joint_t = self.skin.bind_translation.clone();
        self.joint_r = self.skin.bind_rotation.clone();
        self.joint_s = self.skin.bind_scale.clone();
    }

    /// Advance time by `dt` seconds and resample all joint transforms.
    ///
    /// Call once per frame before `upload()`.
    pub fn advance(&mut self, dt: f32) {
        let clip = match self.clip.clone() {
            Some(c) => c,
            None => return,
        };

        self.time += dt * self.speed;
        if self.looping && clip.duration > 0.0 {
            self.time = self.time.rem_euclid(clip.duration);
        } else if self.time >= clip.duration {
            self.time = clip.duration;
            self.finished = true;
        }

        // Reset to bind pose then apply channels.
        self.joint_t.copy_from_slice(&self.skin.bind_translation);
        self.joint_r.copy_from_slice(&self.skin.bind_rotation);
        self.joint_s.copy_from_slice(&self.skin.bind_scale);

        for ch in &clip.channels {
            let ji = ch.joint_index;
            if ji >= self.skin.joint_count {
                continue;
            }
            let v = ch.sample(self.time);
            match ch.property {
                AnimationProperty::Translation => {
                    self.joint_t[ji] = Vec3::new(v[0], v[1], v[2]);
                }
                AnimationProperty::Rotation => {
                    self.joint_r[ji] = Quat::from_xyzw(v[0], v[1], v[2], v[3]).normalize();
                }
                AnimationProperty::Scale => {
                    self.joint_s[ji] = Vec3::new(v[0], v[1], v[2]);
                }
            }
        }

        self.bone_matrices =
            self.skin
                .compute_bone_matrices(&self.joint_t, &self.joint_r, &self.joint_s);
    }

    /// Upload the current bone matrices to the GPU buffer.
    ///
    /// Call once per frame after `advance()`. The buffer is sized on first use;
    /// subsequent frames reuse it.
    pub fn upload(&mut self, engine: &Engine) -> Result<()> {
        let needed = (self.skin.joint_count * std::mem::size_of::<Mat4>()) as u64;
        let too_small = self
            .gpu_buffer
            .as_ref()
            .map(|b| b.desc().size < needed)
            .unwrap_or(true);
        if too_small {
            self.gpu_buffer = Some(engine.create_buffer(BufferDesc {
                size: needed,
                usage: BufferUsage::STORAGE,
            })?);
        }
        self.flush_to_gpu();
        Ok(())
    }

    /// GPU bone matrix buffer. Bind as `"bone_matrices"` before each skinned draw.
    pub fn bone_buffer(&self) -> Option<&Buffer> {
        self.gpu_buffer.as_ref()
    }

    fn flush_to_gpu(&self) {
        if let Some(buf) = &self.gpu_buffer {
            // Mat4 is #[repr(C)], so we can cast directly.
            let bytes: &[u8] = bytemuck::cast_slice(&self.bone_matrices);
            let _ = buf.write(0, bytes);
        }
    }
}
