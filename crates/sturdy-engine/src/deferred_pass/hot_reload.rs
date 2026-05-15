use super::{DeferredPass, helpers::engine_shader};
use crate::ShaderReloadDiagnostic;

impl DeferredPass {
    /// Returns all material variant compile failures since init.
    ///
    /// Each entry is `(content_hash, error_message)`. The failed material falls
    /// back to the default G-Buffer program and is not retried until the hash changes
    /// (i.e. the `UnifiedMaterial` expression is modified).
    pub fn variant_compile_failures(&self) -> impl Iterator<Item = (u64, &str)> {
        self.failed_variants.iter().map(|(k, v)| (*k, v.as_str()))
    }

    /// Clear all recorded variant compile failures, allowing retry on next frame.
    pub fn clear_variant_failures(&mut self) {
        self.failed_variants.clear();
    }

    /// Poll for changes to the engine shader files and hot-reload as needed.
    ///
    /// Call once per frame. Returns one diagnostic per changed file.
    ///
    /// **Cache invalidation**: changes to `brdf.slang`, `material_surface.slang`, or
    /// `gbuffer_fragment.slang` automatically clear the `UnifiedMaterial` variant
    /// cache so all variants recompile against the new code on the next draw.
    ///
    /// # Example
    /// ```ignore
    /// for d in deferred.tick_hot_reload() {
    ///     if !d.success { eprintln!("{}", d.summary()); }
    /// }
    /// ```
    pub fn tick_hot_reload(&mut self) -> Vec<ShaderReloadDiagnostic> {
        let changed = self.shader_watcher.poll_changed();
        if changed.is_empty() {
            return Vec::new();
        }

        let lighting_path = engine_shader("deferred_lighting.slang");
        let gbuffer_path = engine_shader("gbuffer_fragment.slang");
        let forward_path = engine_shader("forward_opaque.slang");
        let brdf_path = engine_shader("brdf.slang");
        let mat_surface_path = engine_shader("material_surface.slang");

        let mut diags = Vec::new();
        let mut invalidate_cache = false;

        for path in changed {
            let mut success = true;
            let mut error: Option<String> = None;

            let is_shared = path == brdf_path || path == mat_surface_path;

            if is_shared {
                // Shared imports affect all generated material variants.
                invalidate_cache = true;
                for result in [
                    self.lighting_program.reload(),
                    self.default_gbuffer_program.reload_fragment(),
                    self.forward_opaque_program.reload_fragment(),
                ] {
                    if let Err(e) = result {
                        if error.is_none() {
                            error = Some(e.to_string());
                        }
                        success = false;
                    }
                }
            } else if path == lighting_path {
                match self.lighting_program.reload() {
                    Ok(_) => {}
                    Err(e) => {
                        error = Some(e.to_string());
                        success = false;
                    }
                }
            } else if path == gbuffer_path {
                invalidate_cache = true;
                if let Err(e) = self.default_gbuffer_program.reload_fragment() {
                    error = Some(e.to_string());
                    success = false;
                }
            } else if path == forward_path {
                match self.forward_opaque_program.reload_fragment() {
                    Ok(_) => {}
                    Err(e) => {
                        error = Some(e.to_string());
                        success = false;
                    }
                }
            }

            diags.push(ShaderReloadDiagnostic {
                path,
                success,
                error,
            });
        }

        if invalidate_cache {
            self.variant_cache.clear();
            self.failed_variants.clear();
        }

        diags
    }
}
