/// Runtime render strategy — adaptive quality decisions driven by GPU timing and scene state.
///
/// The `RenderStrategySelector` operates like a runtime optimizer: it observes what the engine
/// was asked to render (scene object count, pass structure) and how long it actually took
/// (per-frame GPU time), then adjusts the `FrameRenderStrategy` to stay within a user-set
/// frame-time budget. No per-frame user code is needed.
///
/// When `target_frame_ms` is `None`, the selector is a no-op — the strategy stays at its
/// initial (maximum-quality) settings. This is the correct default for offline rendering,
/// benchmarks, and cases where the user manages quality manually.

/// Shading rate quality for VRS-capable hardware.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum VrsQuality {
    /// Full resolution shading everywhere (VRS disabled).
    #[default]
    Full,
    /// Content-adaptive: low-frequency regions get 2×2 shading; edges stay 1×1.
    ContentAdaptive,
    /// Pipeline-level 2×2 for the whole frame.
    Pipeline2x2,
    /// VRS completely off (same as Full but signals intent to skip VRS setup).
    Off,
}

/// Hi-Z occlusion culling mode.
#[derive(Copy, Clone, Debug, Default, Eq, PartialEq)]
pub enum OcclusionMode {
    /// Single-pass: cull against the previous frame's Hi-Z pyramid. Fast, misses newly-visible
    /// objects after large camera moves (acceptable in most cases).
    #[default]
    SinglePass,
    /// Two-phase: Phase 1 vs previous Hi-Z, then current-frame Hi-Z rebuild, then Phase 3 vs
    /// current Hi-Z. Catches all newly-visible objects; carries extra cull + depth-only pass cost.
    TwoPass,
}

/// Per-frame quality and routing decisions produced by the `RenderStrategySelector`.
///
/// Passed to renderer systems each frame. Systems read what they care about and ignore the rest.
/// All fields have sensible defaults that produce maximum quality output.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRenderStrategy {
    /// Render resolution relative to native (0.5–1.0). 1.0 = full resolution.
    ///
    /// Automatically reduced when the GPU is over-budget; restored when under-budget
    /// for 3 consecutive frames. Minimum is 0.5 (50% native resolution per axis).
    pub dynamic_resolution_scale: f32,
    /// Global LOD distance bias. 0.0 = default geometry detail; positive = coarser LOD.
    ///
    /// Increased by 0.2 per over-budget frame after `dynamic_resolution_scale` hits its floor.
    pub lod_bias: f32,
    /// Active shadow cascade count (1–4). Reduced to 1 when heavily over-budget.
    pub shadow_cascade_count: u32,
    /// Maximum distance for shadow cascade coverage in world units.
    pub shadow_cascade_max_distance: f32,
    /// Variable-rate shading quality. `Full` when hardware doesn't support VRS.
    pub vrs_quality: VrsQuality,
    /// Hi-Z occlusion culling strategy.
    pub occlusion_mode: OcclusionMode,
    /// Use the mesh shader pipeline when hardware supports it and scene complexity justifies it.
    pub meshlet_path: bool,
    /// Allow GPU-driven compute passes to run on the async compute queue.
    pub async_compute_overlap: bool,
    /// Use ray-traced reflections when RT hardware is available.
    pub rt_reflections: bool,
}

impl Default for FrameRenderStrategy {
    fn default() -> Self {
        Self {
            dynamic_resolution_scale: 1.0,
            lod_bias: 0.0,
            shadow_cascade_count: 4,
            shadow_cascade_max_distance: 200.0,
            vrs_quality: VrsQuality::Full,
            occlusion_mode: OcclusionMode::SinglePass,
            meshlet_path: false,
            async_compute_overlap: true,
            rt_reflections: false,
        }
    }
}

/// Adaptive render quality selector.
///
/// Reads GPU frame time history and scene state each frame, then adjusts the
/// `FrameRenderStrategy` to stay within a configured frame-time budget. When no
/// budget is configured (`target_frame_ms = None`), the strategy is fixed at its
/// initial (maximum-quality) values.
///
/// # Quality ladder
///
/// When over-budget, quality is reduced in this priority order (each step is tried
/// before moving to the next):
/// 1. Reduce `dynamic_resolution_scale` by 5% (floor: 50% native).
/// 2. Increase `lod_bias` by 0.2 (cap: 2.0).
/// 3. Reduce `shadow_cascade_count` by 1 (floor: 1).
///
/// Quality is restored in reverse order after 3 consecutive under-budget frames.
/// Step sizes are chosen to be imperceptible frame-to-frame.
pub struct RenderStrategySelector {
    target_frame_ms: Option<f32>,
    strategy: FrameRenderStrategy,
    /// Frames consecutively under budget. Reset on any over-budget frame.
    under_budget_frames: u32,
}

impl Default for RenderStrategySelector {
    fn default() -> Self {
        Self::new()
    }
}

impl RenderStrategySelector {
    /// Create a selector with no frame-time target (strategy stays at maximum quality).
    pub fn new() -> Self {
        Self {
            target_frame_ms: None,
            strategy: FrameRenderStrategy::default(),
            under_budget_frames: 0,
        }
    }

    /// Create a selector that adapts quality to hit `target_ms` milliseconds per frame.
    pub fn with_target_frame_ms(target_ms: f32) -> Self {
        Self {
            target_frame_ms: Some(target_ms.max(1.0)),
            strategy: FrameRenderStrategy::default(),
            under_budget_frames: 0,
        }
    }

    /// Set or clear the frame-time target.
    ///
    /// Pass `None` to disable adaptive quality (strategy stays fixed). Pass `Some(ms)` to
    /// enable. The current strategy is preserved when changing targets.
    pub fn set_target_frame_ms(&mut self, target_ms: Option<f32>) {
        self.target_frame_ms = target_ms.map(|ms| ms.max(1.0));
        self.under_budget_frames = 0;
    }

    /// Update the strategy given the last frame's GPU time and scene object count.
    ///
    /// Call once per frame after `pass_timings` are available (typically in the frame-end path).
    /// Returns a reference to the (possibly updated) strategy for the next frame.
    pub fn update(
        &mut self,
        gpu_frame_ms: Option<f32>,
        scene_object_count: u64,
    ) -> &FrameRenderStrategy {
        let Some(target_ms) = self.target_frame_ms else {
            return &self.strategy;
        };
        let Some(gpu_ms) = gpu_frame_ms else {
            return &self.strategy;
        };

        // Enable two-pass occlusion only for dense scenes where the overhead pays off.
        // Below ~5000 objects, single-pass is faster.
        self.strategy.occlusion_mode = if scene_object_count >= 5000 {
            OcclusionMode::TwoPass
        } else {
            OcclusionMode::SinglePass
        };

        // 5% tolerance above target before triggering a quality reduction step.
        if gpu_ms > target_ms * 1.05 {
            self.under_budget_frames = 0;
            self.reduce_quality();
        } else if gpu_ms < target_ms * 0.90 {
            self.under_budget_frames += 1;
            if self.under_budget_frames >= 3 {
                self.increase_quality();
            }
        } else {
            // Within tolerance — don't change anything.
            self.under_budget_frames = 0;
        }

        &self.strategy
    }

    /// Access the current strategy without triggering an update.
    pub fn strategy(&self) -> &FrameRenderStrategy {
        &self.strategy
    }

    /// Replace the current strategy (e.g. for preset-based quality settings).
    pub fn set_strategy(&mut self, strategy: FrameRenderStrategy) {
        self.strategy = strategy;
        self.under_budget_frames = 0;
    }

    fn reduce_quality(&mut self) {
        let s = &mut self.strategy;
        if s.dynamic_resolution_scale > 0.5 {
            s.dynamic_resolution_scale = (s.dynamic_resolution_scale - 0.05).max(0.5);
        } else if s.lod_bias < 2.0 {
            s.lod_bias = (s.lod_bias + 0.2).min(2.0);
        } else if s.shadow_cascade_count > 1 {
            s.shadow_cascade_count -= 1;
        }
        // At the floor of all quality knobs: nothing more can be done.
    }

    fn increase_quality(&mut self) {
        self.under_budget_frames = 0;
        let s = &mut self.strategy;
        // Restore in reverse order (shadows first, then LOD, then resolution).
        if s.shadow_cascade_count < 4 {
            s.shadow_cascade_count += 1;
        } else if s.lod_bias > 0.0 {
            s.lod_bias = (s.lod_bias - 0.2).max(0.0);
        } else if s.dynamic_resolution_scale < 1.0 {
            s.dynamic_resolution_scale = (s.dynamic_resolution_scale + 0.05).min(1.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_target_strategy_unchanged() {
        let mut sel = RenderStrategySelector::new();
        let before = sel.strategy().clone();
        sel.update(Some(50.0), 1000);
        assert_eq!(*sel.strategy(), before);
    }

    #[test]
    fn over_budget_reduces_resolution() {
        let mut sel = RenderStrategySelector::with_target_frame_ms(16.0);
        sel.update(Some(20.0), 100); // over budget: 20ms > 16*1.05
        assert!(sel.strategy().dynamic_resolution_scale < 1.0);
    }

    #[test]
    fn under_budget_3_frames_increases_quality() {
        let mut sel = RenderStrategySelector::with_target_frame_ms(16.0);
        // First push quality down to get something to restore.
        for _ in 0..10 {
            sel.update(Some(20.0), 100);
        }
        let degraded = sel.strategy().clone();
        // Now hold under budget for 3 frames.
        sel.update(Some(10.0), 100);
        sel.update(Some(10.0), 100);
        sel.update(Some(10.0), 100);
        assert_ne!(*sel.strategy(), degraded);
    }

    #[test]
    fn scene_density_gates_two_pass() {
        let mut sel = RenderStrategySelector::with_target_frame_ms(16.0);
        sel.update(Some(14.0), 100);
        assert_eq!(sel.strategy().occlusion_mode, OcclusionMode::SinglePass);
        sel.update(Some(14.0), 6000);
        assert_eq!(sel.strategy().occlusion_mode, OcclusionMode::TwoPass);
    }

    #[test]
    fn resolution_floor_at_50_percent() {
        let mut sel = RenderStrategySelector::with_target_frame_ms(16.0);
        for _ in 0..100 {
            sel.update(Some(100.0), 100);
        }
        assert!(sel.strategy().dynamic_resolution_scale >= 0.5);
    }
}
