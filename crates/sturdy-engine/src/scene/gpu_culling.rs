use glam::Mat4;

use super::scene::Scene;
use crate::{ComputeProgram, Engine, Frustum, GeometryBackend, RenderFrame, RenderWorld, Result};

impl Scene {
    /// Dispatch the GPU frustum culling compute shader for all batches.
    ///
    /// Call this once per frame **after** `scene.prepare_render_world()` and
    /// **before** the draw pass. Only active when `geometry_backend == ComputeIndirect`.
    ///
    /// The compute shader writes one `DrawIndexedIndirectCommand` per instance
    /// slot: visible instances get `instance_count = 1`, invisible ones get
    /// `instance_count = 0` (silently skipped by the GPU). The CPU draws all N
    /// slots via `DrawIndexedIndirect`: no readback or atomic counter needed.
    pub fn cull_gpu(
        &mut self,
        view_proj: Mat4,
        frame: &RenderFrame,
        engine: &Engine,
        render_world: &RenderWorld,
    ) -> Result<()> {
        if self.geometry_backend != GeometryBackend::ComputeIndirect {
            return Ok(());
        }

        if self.culling_program.is_none() {
            let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("shaders")
                .join("cull_compute.slang");
            self.culling_program = Some(ComputeProgram::load(engine, path)?);
        }
        let Some(program) = self.culling_program.as_ref() else {
            return Err(crate::Error::ResourceStateCorruption(
                "GPU culling program was not available after initialization".into(),
            ));
        };

        let frustum = Frustum::from_view_proj(view_proj);
        let planes: [[f32; 4]; 6] = {
            let raw = frustum.planes_raw();
            [
                raw[0].to_array(),
                raw[1].to_array(),
                raw[2].to_array(),
                raw[3].to_array(),
                raw[4].to_array(),
                raw[5].to_array(),
            ]
        };

        #[repr(C)]
        #[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
        struct CullConstants {
            frustum_planes: [[f32; 4]; 6],
            instance_count: u32,
            batch_base_idx: u32,
            index_count: u32,
            first_index: u32,
            vertex_offset: i32,
            _pad: u32,
        }

        let dispatched = render_world.with_gpu_scene_buffer(|scene_buf| -> Result<bool> {
            let Some(scene_buf) = scene_buf else {
                return Ok(false);
            };
            render_world.with_gpu_indirect_buffer(|indirect_buf| -> Result<bool> {
                let Some(indirect_buf) = indirect_buf else {
                    return Ok(false);
                };

                let mut dispatched = false;
                for batch in self.batches.values() {
                    let total = batch.scene_count;
                    if total == 0 {
                        continue;
                    }

                    let mesh_idx = batch.mesh_idx as usize;
                    let mesh = &self.meshes[mesh_idx].0;

                    let index_count = if mesh.is_indexed() {
                        mesh.index_count
                    } else {
                        mesh.vertex_count
                    };

                    let constants = CullConstants {
                        frustum_planes: planes,
                        instance_count: total,
                        batch_base_idx: batch.scene_base_idx,
                        index_count,
                        first_index: 0,
                        vertex_offset: 0,
                        _pad: 0,
                    };

                    frame.bind_buffer("scene_instances", scene_buf);
                    frame.bind_buffer("indirect_commands", indirect_buf);

                    let groups = [(total + 63) / 64, 1, 1];
                    frame.dispatch_compute_auto(
                        format!("cull-batch-{mesh_idx}"),
                        program,
                        &constants,
                        groups,
                    )?;
                    dispatched = true;
                }

                Ok(dispatched)
            })
        })?;

        self.gpu_cull_active = dispatched;
        Ok(())
    }
}
