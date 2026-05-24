// Shadow pipeline — orchestrates all per-frame shadow passes as a single unit.
//
// Extracted from DeferredPass so shadow rendering has one clear responsibility.
// CsmPass (directional), SpotShadowPass, and PointShadowPass are composed here.
// The draw() call renders every enabled shadow type and binds all frame resources
// so the deferred lighting shader finds them by name.
//
// Reusable: ShadowPipeline can be created and driven independently of DeferredPass.

use glam::Mat4;

use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, Format, ImageDesc, ImageDimension, ImageUsage,
    PointShadowConfig, PointShadowPass, RenderFrame, Result, SpotShadowConfig, SpotShadowPass,
    scene::Scene,
    shadow_pass::{CsmConfig, CsmPass},
};
use sturdy_engine_core::Extent3d;

/// Renders all shadow passes for a frame and binds their outputs.
///
/// After `draw()` the following frame resources are registered:
/// - `"shadow_atlas"`, `"shadow_map_0..3"` (CSM directional)
/// - `"spot_shadow_map_0..3"` (spot shadows or 1×1 black fallback)
/// - `"point_shadow_front/back_0..3"` (point shadows or 1×1 black fallback)
/// - `"csm_data"`, `"spot_shadow_data"`, `"point_shadow_data"` (GPU buffers)
pub struct ShadowPipeline {
    pub csm: CsmPass,
    spot: Option<SpotShadowPass>,
    point: Option<PointShadowPass>,
    /// Zero-filled spot shadow data — bound when no spot pass is active.
    empty_spot_shadow_buf: Buffer,
    /// Zero-filled point shadow data — bound when no point pass is active.
    empty_point_shadow_buf: Buffer,
    /// 1×1 Depth32Float black image — fallback for unbound shadow map slots.
    black_depth: crate::Image,
}

impl ShadowPipeline {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_csm_config(engine, CsmConfig::default())
    }

    pub fn with_csm_config(engine: &Engine, csm_config: CsmConfig) -> Result<Self> {
        let csm = CsmPass::with_config(engine, csm_config)?;

        let empty_spot_shadow_buf = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<crate::GpuSpotShadowData>() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        let empty_spot = crate::GpuSpotShadowData {
            light_indices: [0xFFFFFFFF; crate::spot_shadow_pass::MAX_SPOT_SHADOWS],
            ..bytemuck::Zeroable::zeroed()
        };
        empty_spot_shadow_buf.write(0, bytemuck::bytes_of(&empty_spot))?;

        let empty_point_shadow_buf = engine.create_buffer(BufferDesc {
            size: std::mem::size_of::<crate::GpuPointShadowData>() as u64,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        let empty_point = crate::GpuPointShadowData {
            light_indices: [0xFFFFFFFF; crate::point_shadow_pass::MAX_POINT_SHADOWS],
            ..bytemuck::Zeroable::zeroed()
        };
        empty_point_shadow_buf.write(0, bytemuck::bytes_of(&empty_point))?;

        let black_depth = engine.create_image(ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: 1,
                height: 1,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL | ImageUsage::SAMPLED,
            transient: false,
            clear_value: None,
            debug_name: Some("shadow_pipeline_black_depth"),
            compression: Default::default(),
            min_lod_bits: None,
            msaa_resolve_to_single_sampled: false,
            drm_format_modifier: None,
        })?;

        Ok(Self {
            csm,
            spot: None,
            point: None,
            empty_spot_shadow_buf,
            empty_point_shadow_buf,
            black_depth,
        })
    }

    /// Render all enabled shadow passes and bind their outputs to `frame`.
    ///
    /// `view`/`proj` are the camera matrices; `cam_near`/`cam_far` drive CSM partitioning.
    pub fn draw(
        &mut self,
        scene: &Scene,
        view: Mat4,
        proj: Mat4,
        cam_near: f32,
        cam_far: f32,
        frame: &RenderFrame,
        engine: &Engine,
    ) -> Result<()> {
        // ── CSM — directional light, N cascades ──────────────────────────────
        self.csm
            .draw(scene, view, proj, cam_near, cam_far, frame)
            .map_err(|e| {
                tracing::error!("CsmPass::draw: {e:?}");
                e
            })?;
        frame.bind_buffer("csm_data", &self.csm.csm_buffer);

        // ── Spot light shadows ───────────────────────────────────────────────
        // Spot lights begin after [directional, point_lights...] in the GPU lights buffer.
        let spot_buf_offset = 1u32 + scene.point_lights.len() as u32;
        if let Some(spot) = &mut self.spot {
            if !scene.spot_lights.is_empty() {
                spot.draw(scene, spot_buf_offset, frame, engine)?;
                frame.bind_buffer("spot_shadow_data", &spot.shadow_buf);
            } else {
                // Pass attached but no lights — bind black fallbacks and the
                // zeroed empty buffer so the shader sees count=0 even if a
                // previous frame left stale data in spot.shadow_buf.
                frame.bind_buffer("spot_shadow_data", &self.empty_spot_shadow_buf);
                for name in [
                    "spot_shadow_map_0",
                    "spot_shadow_map_1",
                    "spot_shadow_map_2",
                    "spot_shadow_map_3",
                ] {
                    frame.bind_image(name, &self.black_depth);
                }
            }
        } else {
            frame.bind_buffer("spot_shadow_data", &self.empty_spot_shadow_buf);
            for name in [
                "spot_shadow_map_0",
                "spot_shadow_map_1",
                "spot_shadow_map_2",
                "spot_shadow_map_3",
            ] {
                frame.bind_image(name, &self.black_depth);
            }
        }

        // ── Point light shadows (dual-paraboloid) ────────────────────────────
        // Point lights begin at index 1 (after the directional light at 0).
        let point_buf_offset = 1u32;
        if let Some(pt) = &mut self.point {
            if !scene.point_lights.is_empty() {
                pt.draw(scene, point_buf_offset, frame, engine)?;
                frame.bind_buffer("point_shadow_data", &pt.shadow_buf);
            } else {
                // Pass attached but no lights — use the zeroed empty buffer so
                // the shader sees count=0 even if a previous frame left stale
                // data in pt.shadow_buf.
                frame.bind_buffer("point_shadow_data", &self.empty_point_shadow_buf);
                for name in [
                    "point_shadow_front_0",
                    "point_shadow_front_1",
                    "point_shadow_front_2",
                    "point_shadow_front_3",
                    "point_shadow_back_0",
                    "point_shadow_back_1",
                    "point_shadow_back_2",
                    "point_shadow_back_3",
                ] {
                    frame.bind_image(name, &self.black_depth);
                }
            }
        } else {
            frame.bind_buffer("point_shadow_data", &self.empty_point_shadow_buf);
            for name in [
                "point_shadow_front_0",
                "point_shadow_front_1",
                "point_shadow_front_2",
                "point_shadow_front_3",
                "point_shadow_back_0",
                "point_shadow_back_1",
                "point_shadow_back_2",
                "point_shadow_back_3",
            ] {
                frame.bind_image(name, &self.black_depth);
            }
        }

        Ok(())
    }

    // ── Configuration accessors ───────────────────────────────────────────────

    pub fn csm_config_mut(&mut self) -> &mut CsmConfig {
        &mut self.csm.config
    }

    pub fn set_spot_shadows(&mut self, pass: SpotShadowPass) {
        self.spot = Some(pass);
    }

    pub fn clear_spot_shadows(&mut self) {
        self.spot = None;
    }

    pub fn spot_shadow_config_mut(&mut self) -> Option<&mut SpotShadowConfig> {
        self.spot.as_mut().map(|s| &mut s.config)
    }

    pub fn set_point_shadows(&mut self, pass: PointShadowPass) {
        self.point = Some(pass);
    }

    pub fn clear_point_shadows(&mut self) {
        self.point = None;
    }

    pub fn point_shadow_config_mut(&mut self) -> Option<&mut PointShadowConfig> {
        self.point.as_mut().map(|p| &mut p.config)
    }
}
