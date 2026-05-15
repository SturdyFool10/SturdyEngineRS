use super::scene::Scene;
use crate::{Buffer, Mesh};

impl Scene {
    /// Iterate batches whose `UnifiedMaterial` has `MaterialDomain::Translucent`.
    ///
    /// Used by `OitPass` to draw only transparent geometry in the collect pass.
    pub fn translucent_batches(&self) -> impl Iterator<Item = (usize, Option<&Buffer>, u32)> {
        self.batches
            .values()
            .filter(|b| {
                let idx = b.mesh_idx as usize;
                self.materials
                    .get(idx)
                    .and_then(|m| m.unified.as_ref())
                    .map(|u| u.domain == super::material::MaterialDomain::Translucent)
                    .unwrap_or(false)
            })
            .map(|b| (b.mesh_idx as usize, b.gpu_buffer.as_ref(), b.total_count()))
    }

    /// Iterate over all drawable batches as `(mesh_index, instance_gpu_buffer, instance_count)`.
    ///
    /// Used by external passes (e.g. `ShadowPass`) that need to draw scene
    /// geometry with a custom shader. The GPU instance buffer is `None` when
    /// the batch has not yet been prepared for this frame.
    pub fn drawable_batches(&self) -> impl Iterator<Item = (usize, Option<&Buffer>, u32)> {
        self.batches
            .values()
            .map(|b| (b.mesh_idx as usize, b.gpu_buffer.as_ref(), b.total_count()))
    }

    /// Number of registered meshes.
    pub fn mesh_count(&self) -> usize {
        self.meshes.len()
    }

    /// Return the `Mesh` at the given index (as registered by `add_mesh`).
    pub fn mesh_at(&self, index: usize) -> &Mesh {
        &self.meshes[index].0
    }
}
