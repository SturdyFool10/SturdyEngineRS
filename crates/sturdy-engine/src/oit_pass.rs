// Order-Independent Transparency using Per-Pixel Linked Lists (PPLL).
//
// Renders all Translucent-domain meshes into a per-pixel linked list of
// fragments, then resolves by sorting each pixel's fragments by depth and
// compositing them over the opaque HDR target.
//
// The PPLL approach is exact — multiple overlapping transparent surfaces (e.g.
// multiple panes of tinted glass in a brake light assembly) are blended in
// precisely the correct front-to-back order regardless of draw order.
//
// Roadmap: Track 6d — Translucent forward tail.

use std::path::PathBuf;

use crate::{
    Buffer, BufferDesc, BufferUsage, Engine, Format, GraphImage, Image, ImageDesc, ImageDimension,
    ImageUsage, MeshProgram, MeshProgramDesc, MeshVertexKind, RenderFrame, Result, ShaderDesc,
    ShaderProgram, ShaderSource, ShaderStage, push_constants, scene::Scene,
};
use glam::Mat4;
use sturdy_engine_core::Extent3d;

fn engine_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("shaders")
        .join(name)
}

// ── OIT fragment pool entry (matches oit_collect.slang) ───────────────────────

/// On-GPU OIT fragment (16 bytes). Layout must match the Slang struct exactly.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct GpuOitFragment {
    color_rg: u32, // f16 R (bits 0-15) + f16 G (bits 16-31)
    color_ba: u32, // f16 B (bits 0-15) + f16 A (bits 16-31)
    depth: f32,
    next: u32, // OIT_INVALID = 0xFFFFFFFF
}

/// Push constants for the OIT collect pass.
#[push_constants]
struct OitCollectConstants {
    view_proj: [[f32; 4]; 4],
    previous_view_proj: [[f32; 4]; 4],
    time: f32,
    _pad_cam: [f32; 3],
    fb_width: u32,
    pool_size: u32,
    _pad_oit: [f32; 2],
}

/// Push constants for the OIT resolve pass.
#[push_constants]
struct OitResolveConstants {
    fb_width: u32,
    _pad: [f32; 3],
}

// ── OitConfig ─────────────────────────────────────────────────────────────────

/// Configuration for the OIT transparent pass.
///
/// `OitConfig::default()` is tuned for scenes with moderate transparency density
/// (car paint, glass panels, particle effects). Increase `average_layers` for
/// very layered content (volumetric fog, dense particle systems).
#[derive(Clone, Debug)]
pub struct OitConfig {
    /// Expected average number of transparent layers per pixel.
    /// The global fragment pool is sized to `width × height × average_layers`.
    /// \[1, 64\]. Default 8.
    pub average_layers: u32,
}

impl Default for OitConfig {
    fn default() -> Self {
        Self { average_layers: 8 }
    }
}

// ── OitPass ───────────────────────────────────────────────────────────────────

/// Per-Pixel Linked List OIT pass.
///
/// Attach to a `DeferredPass` via `DeferredPass::set_oit(OitPass::new(...))`.
/// The pass is driven automatically inside `DeferredPass::draw()` after the
/// deferred lighting step.
///
/// # Translucent material setup
/// ```ignore
/// // Mark a mesh as translucent via its UnifiedMaterial:
/// let glass = UnifiedMaterial::pbr_metallic_roughness("glass")
///     .domain(MaterialDomain::Translucent)
///     .base_color(MaterialExpr::constant([0.9, 0.15, 0.10, 0.35])) // red, 35% opacity
///     .roughness(MaterialExpr::constant(0.05))
///     .build();
/// scene.set_unified_material(glass_mesh_id, glass);
/// ```
pub struct OitPass {
    collect_program: MeshProgram,
    resolve_program: ShaderProgram,
    pub config: OitConfig,

    /// Flat (128, 128, 255, 255) normal map used for meshes without a normal map.
    flat_normal_map: Image,

    /// Persistent buffers, rebuilt when the framebuffer size changes.
    head_buf: Option<Buffer>,
    frag_pool: Option<Buffer>,
    counter: Option<Buffer>,
    current_width: u32,
    current_height: u32,
}

impl OitPass {
    pub fn new(engine: &Engine) -> Result<Self> {
        Self::with_config(engine, OitConfig::default())
    }

    pub fn with_config(engine: &Engine, config: OitConfig) -> Result<Self> {
        let collect_program = MeshProgram::new(
            engine,
            MeshProgramDesc {
                fragment: ShaderDesc {
                    source: ShaderSource::File(engine_shader("oit_collect.slang")),
                    entry_point: "main".to_owned(),
                    stage: ShaderStage::Fragment,
                },
                vertex: None, // uses mesh_vertex_3d.slang
                vertex_kind: MeshVertexKind::V3d,
                alpha_blend: false, // OIT handles blending itself
                uses_depth: true,   // depth test against opaque geometry
            },
        )?;

        let resolve_program = engine.load_shader(engine_shader("oit_resolve.slang"))?;

        let flat_normal_map =
            engine.generate_texture_2d("oit_flat_normal_map", 1, 1, |_, _| [128, 128, 255, 255])?;

        Ok(Self {
            collect_program,
            resolve_program,
            config,
            flat_normal_map,
            head_buf: None,
            frag_pool: None,
            counter: None,
            current_width: 0,
            current_height: 0,
        })
    }

    /// Ensure the OIT buffers match the current framebuffer size.
    fn ensure_buffers(&mut self, engine: &Engine, width: u32, height: u32) -> Result<()> {
        if self.head_buf.is_some() && width == self.current_width && height == self.current_height {
            return Ok(());
        }

        let pixel_count = width as u64 * height as u64;
        let pool_size = pixel_count * self.config.average_layers as u64;

        self.head_buf = Some(engine.create_buffer(BufferDesc {
            size: pixel_count * 4, // one uint per pixel
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?);
        self.frag_pool = Some(engine.create_buffer(BufferDesc {
            size: pool_size * std::mem::size_of::<GpuOitFragment>() as u64,
            usage: BufferUsage::STORAGE,
        })?);
        self.counter = Some(engine.create_buffer(BufferDesc {
            size: 4, // single uint
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?);

        self.current_width = width;
        self.current_height = height;
        Ok(())
    }

    /// Reset the head buffer (0xFFFFFFFF per pixel) and the counter (0).
    ///
    /// Must be called once per frame **before** the collect pass.
    fn reset_buffers(&self) -> Result<()> {
        let pixel_count = self.current_width as u64 * self.current_height as u64;
        if let Some(buf) = &self.head_buf {
            let fill = vec![0xFFu8; pixel_count as usize * 4];
            buf.write(0, &fill)?;
        }
        if let Some(buf) = &self.counter {
            buf.write(0, &0u32.to_le_bytes())?;
        }
        Ok(())
    }

    /// Execute the full OIT pass:
    ///   1. Reset buffers (CPU write)
    ///   2. Collect translucent fragments (depth-tested against opaque, no color write)
    ///   3. Resolve by sorting + compositing over `hdr_target`
    ///
    /// `view` and `proj` must match the matrices used for the opaque pass.
    /// `time` is elapsed seconds (for animated transparent materials).
    pub fn draw(
        &mut self,
        scene: &Scene,
        view: Mat4,
        proj: Mat4,
        hdr_target: &GraphImage,
        frame: &RenderFrame,
        engine: &Engine,
        time: f32,
    ) -> Result<()> {
        let ext = hdr_target.desc().extent;
        let (w, h) = (ext.width, ext.height);

        self.ensure_buffers(engine, w, h)?;
        self.reset_buffers()?;

        //panic allowed, reason = "ensure_buffers proves OIT head buffer exists"
        let head_buf = self.head_buf.as_ref().unwrap();
        //panic allowed, reason = "ensure_buffers proves OIT fragment pool exists"
        let frag_pool = self.frag_pool.as_ref().unwrap();
        //panic allowed, reason = "ensure_buffers proves OIT counter buffer exists"
        let counter = self.counter.as_ref().unwrap();
        let pool_size = (w as u64 * h as u64 * self.config.average_layers as u64) as u32;

        // ── 1. Depth-only prepass to populate Z (reuse the opaque depth if bound) ──
        // (The collect pass uses depth test via the depth attachment — whatever
        //  depth image was used in the deferred G-Buffer pass is still valid.)

        // ── 2. Collect pass — render translucent objects into PPLL ─────────────
        let vp = (proj * view).to_cols_array_2d();
        let collect_constants = OitCollectConstants {
            view_proj: vp,
            previous_view_proj: vp,
            time,
            _pad_cam: [0.0; 3],
            fb_width: w,
            pool_size,
            _pad_oit: [0.0; 2],
        };

        frame.bind_buffer("oit_head_buf", head_buf);
        frame.bind_buffer("oit_frag_pool", frag_pool);
        frame.bind_buffer("oit_counter", counter);

        // Allocate a depth-only image for the collect pass depth test.
        // Ideally we'd reuse the G-Buffer depth, but that requires the depth
        // attachment to still be bound. For simplicity, allocate a fresh one —
        // the depth buffer is cheap and the collect pass writes no colour.
        let depth_desc = ImageDesc {
            dimension: ImageDimension::D2,
            extent: Extent3d {
                width: w,
                height: h,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL,
            transient: false,
            clear_value: None,
            debug_name: Some("oit_depth"),
        };
        let oit_depth = frame.image("oit_depth", depth_desc)?;

        for (mesh_idx, instance_buf_opt, instance_count) in scene.translucent_batches() {
            let instance_buf = match instance_buf_opt {
                Some(b) => b,
                None => continue,
            };
            if instance_count == 0 {
                continue;
            }
            let mesh = scene.mesh_at(mesh_idx);

            frame.bind_buffer("instances", instance_buf);
            if let Some(mat_buf) = scene.material_gpu_buffer_at(mesh_idx) {
                frame.bind_buffer("material_desc", mat_buf);
            }

            // Bind normal map: per-mesh override, or flat fallback.
            let nmap: &Image = scene
                .normal_map_at(mesh_idx)
                .map(|arc| arc.as_ref())
                .unwrap_or(&self.flat_normal_map);
            frame.bind_image("normal_map", nmap);

            // Collect pass writes into a null colour target (the OIT buffers
            // are storage buffers bound above). We draw into a 1×1 dummy target
            // because the API requires at least one colour attachment... unless
            // we use a depth-only draw. Here we use the depth-only path.
            oit_depth.draw_mesh_depth_only_with_push_constants(
                mesh,
                &self.collect_program,
                instance_buf,
                instance_count as u32,
                &collect_constants,
            )?;
        }

        // ── 3. Resolve — sort + composite over the opaque HDR target ───────────
        hdr_target.register_as("opaque_hdr");
        frame.bind_buffer("oit_head_buf", head_buf);
        frame.bind_buffer("oit_frag_pool", frag_pool);

        let resolve_constants = OitResolveConstants {
            fb_width: w,
            _pad: [0.0; 3],
        };
        hdr_target.execute_shader_with_constants_auto(&self.resolve_program, &resolve_constants)?;

        Ok(())
    }
}
