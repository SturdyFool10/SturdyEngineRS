// Image-based lighting (IBL) environment map.
//
// Loads an equirectangular HDR image and precomputes three GPU resources:
//
//   1. `specular` — Texture2DArray (SPECULAR_LAYER_COUNT layers, RGBA16Float)
//      Each layer holds the environment prefiltered at one roughness level.
//      Layer 0 = roughness 0 (mirror), last layer = roughness 1 (fully diffuse).
//
//   2. `sh9_buffer` — StructuredBuffer<float4> (9 elements)
//      SH2 (L0+L1+L2) irradiance coefficients for diffuse IBL.
//      Cheap to evaluate at any surface normal; replaces the old constant ambient.
//
//   3. `equirectangular` — Texture2D<float4> (RGBA16Float)
//      The raw HDR source kept for reference or re-processing.
//
// All precomputation runs on the CPU at load time. At 128×64 per layer and
// 256 GGX samples, the full precompute takes < 200 ms on a modern CPU.
//
// Roadmap: Track 6c (BRDF library) + Track 6h (IBL authoring).

use std::path::Path;

use crate::{Buffer, BufferDesc, BufferUsage, Engine, FrameSyncReason, Image, ImageDesc,
            ImageDimension, ImageUsage, Result, TextureUploadDesc};
use sturdy_engine_core::Extent3d;

// ── Constants ─────────────────────────────────────────────────────────────────

/// Number of roughness layers in the prefiltered specular array.
pub const SPECULAR_LAYER_COUNT: u32 = 8;

/// Width of each prefiltered specular layer (equirectangular 2:1 ratio).
pub const SPECULAR_WIDTH: u32 = 128;

/// Height of each prefiltered specular layer.
pub const SPECULAR_HEIGHT: u32 = 64;

/// Number of GGX importance samples per texel during specular prefilter.
const SPECULAR_SAMPLES: u32 = 256;

/// Number of SH2 basis functions (L0 + L1 + L2 = 1 + 3 + 5).
pub const SH9_COUNT: usize = 9;

// ── EnvironmentMap ────────────────────────────────────────────────────────────

/// A precomputed image-based lighting environment.
///
/// Attach to a [`DeferredPass`] via [`DeferredPass::set_environment_map`] to
/// enable split-sum specular reflections and SH9 diffuse irradiance.
///
/// # Example
/// ```ignore
/// let env = EnvironmentMap::from_hdr(&engine, "assets/hdri/sunset.hdr")?;
/// deferred.set_environment_map(env);
/// ```
pub struct EnvironmentMap {
    /// Raw equirectangular HDR source (RGBA16Float, 1 mip).
    pub equirectangular: Image,
    /// Prefiltered specular: Texture2DArray, `SPECULAR_LAYER_COUNT` layers, same resolution.
    /// Bind as `"env_specular"` in the frame for the deferred lighting shader.
    pub specular: Image,
    /// SH9 irradiance coefficients (9 × float4, only xyz used per element).
    /// Bind as `"sh9_irradiance"` in the frame.
    pub sh9_buffer: Buffer,
    /// Raw SH9 coefficients for serialisation / debug.
    pub sh9_coefficients: [[f32; 3]; SH9_COUNT],
}

impl EnvironmentMap {
    /// Load an HDR equirectangular image and precompute all IBL resources.
    ///
    /// Supports `.hdr` (RGBE) and `.exr` (OpenEXR). Blocks until GPU upload
    /// completes — call once at init, not per frame.
    pub fn from_hdr(engine: &Engine, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let dyn_image = image::open(path).map_err(|e| {
            crate::Error::Unknown(format!("EnvironmentMap: open '{}': {e}", path.display()))
        })?;
        let rgba32 = dyn_image.into_rgba32f();
        let (w, h) = (rgba32.width() as usize, rgba32.height() as usize);
        let pixels: Vec<[f32; 4]> = rgba32.as_raw()
            .chunks(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect();

        Self::from_cpu_pixels(engine, &pixels, w, h)
    }

    /// Precompute IBL from already-decoded f32 RGBA pixels (row-major, equirectangular).
    pub fn from_cpu_pixels(engine: &Engine, pixels: &[[f32; 4]], width: usize, height: usize) -> Result<Self> {
        let sh9_coefficients = compute_sh9(pixels, width, height);
        let sh9_buffer       = upload_sh9_buffer(engine, &sh9_coefficients)?;
        let specular         = prefilter_specular_cpu(engine, pixels, width, height)?;
        let equirectangular  = upload_equirectangular(engine, pixels, width, height)?;

        Ok(Self { equirectangular, specular, sh9_buffer, sh9_coefficients })
    }
}

// ── Equirectangular upload ────────────────────────────────────────────────────

fn upload_equirectangular(engine: &Engine, pixels: &[[f32; 4]], w: usize, h: usize) -> Result<Image> {
    let f16: Vec<u8> = pixels.iter()
        .flat_map(|p| {
            [half::f16::from_f32(p[0]), half::f16::from_f32(p[1]),
             half::f16::from_f32(p[2]), half::f16::from_f32(p[3])]
                .map(|v| v.to_le_bytes())
                .concat()
        })
        .collect();

    let mut frame = engine.begin_frame()?;
    let image = frame.upload_texture_2d(
        "env_equirectangular",
        TextureUploadDesc::sampled_rgba16f(w as u32, h as u32),
        &f16,
    )?;
    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    Ok(image)
}

// ── Specular prefilter ────────────────────────────────────────────────────────

/// Compute the GGX-prefiltered specular env map on the CPU and upload as a
/// Texture2DArray with `SPECULAR_LAYER_COUNT` layers at `SPECULAR_WIDTH×SPECULAR_HEIGHT`.
fn prefilter_specular_cpu(
    engine: &Engine,
    src_pixels: &[[f32; 4]],
    src_w: usize,
    src_h: usize,
) -> Result<Image> {
    let layers = SPECULAR_LAYER_COUNT as usize;
    let out_w  = SPECULAR_WIDTH  as usize;
    let out_h  = SPECULAR_HEIGHT as usize;
    let bytes_per_layer = out_w * out_h * 8; // RGBA16Float = 8 bytes/texel

    // Compute all layers on CPU.
    let mut all_bytes: Vec<u8> = Vec::with_capacity(bytes_per_layer * layers);
    for layer in 0..layers {
        let roughness = if layers <= 1 {
            0.0f32
        } else {
            layer as f32 / (layers - 1) as f32
        };
        let layer_pixels = prefilter_layer(src_pixels, src_w, src_h, out_w, out_h, roughness, SPECULAR_SAMPLES);
        for p in &layer_pixels {
            for &ch in p {
                let f16 = half::f16::from_f32(ch);
                all_bytes.extend_from_slice(&f16.to_le_bytes());
            }
        }
    }

    // Create the array image.
    let image = engine.create_image(ImageDesc {
        dimension: ImageDimension::D2,
        extent: Extent3d { width: out_w as u32, height: out_h as u32, depth: 1 },
        mip_levels: 1,
        layers: SPECULAR_LAYER_COUNT as u16,
        samples: 1,
        format: crate::Format::Rgba16Float,
        usage: ImageUsage::SAMPLED | ImageUsage::COPY_DST,
        transient: false,
        clear_value: None,
        debug_name: Some("env_specular"),
    })?;

    // Upload: one copy command per layer using a shared staging buffer.
    let staging = engine.create_buffer(BufferDesc {
        size: all_bytes.len() as u64,
        usage: BufferUsage::COPY_SRC,
    })?;
    staging.write(0, &all_bytes)?;

    let mut frame = engine.begin_frame()?;
    frame.import_image(&image)?;
    frame.import_buffer(&staging)?;

    for layer in 0..layers {
        frame.copy_buffer_to_image(
            format!("env-specular-layer-{layer}"),
            &staging,
            &image,
            crate::ImageCopyRegion {
                buffer_offset: (layer * bytes_per_layer) as u64,
                mip_level: 0,
                base_layer: layer as u32,
                layer_count: 1,
                width: out_w as u32,
                height: out_h as u32,
                depth: 1,
            },
        )?;
    }

    // Transition to ShaderRead so the image is sampeable in the render frame.
    let subresource = crate::SubresourceRange { base_mip: 0, mip_count: 1, base_layer: 0, layer_count: SPECULAR_LAYER_COUNT as u16 };
    frame.add_pass(crate::PassDesc {
        name: "env-specular-shader-read".into(),
        queue: crate::QueueType::Graphics,
        shader: None,
        pipeline: None,
        bind_groups: Vec::new(),
        push_constants: None,
        work: crate::PassWork::None,
        reads: vec![crate::ImageUse {
            image: image.handle(),
            access: crate::Access::Read,
            state: crate::RgState::ShaderRead,
            subresource,
        }],
        writes: Vec::new(),
        buffer_reads: Vec::new(),
        buffer_writes: Vec::new(),
        clear_colors: Vec::new(),
        clear_depth: None,
    })?;

    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    Ok(image)
}

/// GGX-prefilter one roughness layer at the given output resolution.
fn prefilter_layer(
    src: &[[f32; 4]],
    src_w: usize,
    src_h: usize,
    out_w: usize,
    out_h: usize,
    roughness: f32,
    samples: u32,
) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0f32; 4]; out_w * out_h];
    for y in 0..out_h {
        for x in 0..out_w {
            let u = (x as f32 + 0.5) / out_w as f32;
            let v = (y as f32 + 0.5) / out_h as f32;
            let n = latlong_to_dir(u, v);
            let rgb = prefilter_texel(src, src_w, src_h, n, roughness, samples);
            out[y * out_w + x] = [rgb[0], rgb[1], rgb[2], 1.0];
        }
    }
    out
}

/// GGX importance-sampled prefilter for a single output texel.
fn prefilter_texel(
    src: &[[f32; 4]],
    src_w: usize,
    src_h: usize,
    n: [f32; 3],
    roughness: f32,
    samples: u32,
) -> [f32; 3] {
    if roughness < 1e-4 {
        // Mirror: just sample the env at N directly.
        let uv = dir_to_latlong(n);
        return sample_latlong(src, src_w, src_h, uv);
    }

    let v = n; // isotropic assumption: V = R = N
    let mut acc = [0.0f32; 3];
    let mut total_weight = 0.0f32;

    for i in 0..samples {
        let xi = hammersley(i, samples);
        let h  = importance_sample_ggx(xi, n, roughness);
        let l  = reflect_dir(v, h);
        let n_dot_l = dot3(n, l).max(0.0);
        if n_dot_l > 0.0 {
            let uv = dir_to_latlong(l);
            let c  = sample_latlong(src, src_w, src_h, uv);
            acc[0] += c[0] * n_dot_l;
            acc[1] += c[1] * n_dot_l;
            acc[2] += c[2] * n_dot_l;
            total_weight += n_dot_l;
        }
    }
    if total_weight > 1e-6 {
        [acc[0] / total_weight, acc[1] / total_weight, acc[2] / total_weight]
    } else {
        [0.0; 3]
    }
}

// ── SH9 computation ───────────────────────────────────────────────────────────

/// Project the equirectangular env onto 9 SH2 basis functions.
fn compute_sh9(pixels: &[[f32; 4]], w: usize, h: usize) -> [[f32; 3]; SH9_COUNT] {
    let mut sh = [[0.0f64; 3]; SH9_COUNT];
    let mut total = 0.0f64;

    for y in 0..h {
        let v = (y as f64 + 0.5) / h as f64;
        let theta = v * std::f64::consts::PI;
        let sin_t = theta.sin();
        let cos_t = theta.cos();

        for x in 0..w {
            let u = (x as f64 + 0.5) / w as f64;
            let phi = (u - 0.5) * 2.0 * std::f64::consts::PI;
            let dir = [sin_t * phi.cos(), cos_t, sin_t * phi.sin()];
            let basis = sh2_basis(dir);
            let px = &pixels[y * w + x];
            let weight = sin_t; // solid-angle correction

            for (i, b) in basis.iter().enumerate() {
                sh[i][0] += px[0] as f64 * b * weight;
                sh[i][1] += px[1] as f64 * b * weight;
                sh[i][2] += px[2] as f64 * b * weight;
            }
            total += weight;
        }
    }

    let scale = 4.0 * std::f64::consts::PI / total;
    let mut out = [[0.0f32; 3]; SH9_COUNT];
    for i in 0..SH9_COUNT {
        out[i] = [
            (sh[i][0] * scale) as f32,
            (sh[i][1] * scale) as f32,
            (sh[i][2] * scale) as f32,
        ];
    }
    out
}

/// Upload SH9 as a `StructuredBuffer<float4>` (9 elements, xyz=irradiance, w=0).
fn upload_sh9_buffer(engine: &Engine, coeffs: &[[f32; 3]; SH9_COUNT]) -> Result<Buffer> {
    let mut data = vec![0u8; SH9_COUNT * 16]; // 9 × float4 = 144 bytes
    for (i, c) in coeffs.iter().enumerate() {
        let base = i * 16;
        data[base..base + 4].copy_from_slice(&c[0].to_le_bytes());
        data[base + 4..base + 8].copy_from_slice(&c[1].to_le_bytes());
        data[base + 8..base + 12].copy_from_slice(&c[2].to_le_bytes());
        // w = 0.0, already zeroed
    }
    let buf = engine.create_buffer(BufferDesc {
        size: data.len() as u64,
        usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
    })?;
    buf.write(0, &data)?;
    Ok(buf)
}

// ── BRDF LUT (public, used by DeferredPass) ───────────────────────────────────

/// Compute and upload the 128×128 GGX split-sum BRDF integration LUT.
///
/// Returns an `Rgba16Float` image where:
///   R = BRDF scale  (multiply by F0)
///   G = BRDF bias   (add to the specular result independent of F0)
///
/// Called once in `DeferredPass::new()`. The result is bound as `"brdf_lut"`.
pub fn compute_brdf_lut(engine: &Engine) -> Result<Image> {
    const SIZE: usize = 128;
    const SAMPLES: u32 = 512;

    let mut pixels = vec![0u8; SIZE * SIZE * 8]; // RGBA16Float = 8 bytes/texel
    for y in 0..SIZE {
        let roughness = (y as f32 + 0.5) / SIZE as f32;
        for x in 0..SIZE {
            let n_dot_v = (x as f32 + 0.5) / SIZE as f32;
            let (scale, bias) = integrate_brdf(n_dot_v, roughness, SAMPLES);
            let offset = (y * SIZE + x) * 8;
            let s16 = half::f16::from_f32(scale.clamp(0.0, 1.0));
            let b16 = half::f16::from_f32(bias.clamp(0.0, 1.0));
            pixels[offset..offset + 2].copy_from_slice(&s16.to_le_bytes());
            pixels[offset + 2..offset + 4].copy_from_slice(&b16.to_le_bytes());
            // B and A channels left as 0
        }
    }

    let mut frame = engine.begin_frame()?;
    let image = frame.upload_texture_2d(
        "brdf_lut",
        TextureUploadDesc::sampled_rgba16f(SIZE as u32, SIZE as u32),
        &pixels,
    )?;
    let _ = image.set_debug_name("brdf_lut");
    frame.flush_with_reason(FrameSyncReason::CompatibilityShim)?;
    frame.wait_with_reason(FrameSyncReason::CompatibilityShim)?;
    Ok(image)
}

/// GGX split-sum integral for a single (NdotV, roughness) pair.
fn integrate_brdf(n_dot_v: f32, roughness: f32, samples: u32) -> (f32, f32) {
    // V in tangent space, N = (0,0,1).
    let v = [f32::sqrt(1.0 - n_dot_v * n_dot_v), 0.0, n_dot_v];
    let mut scale = 0.0f32;
    let mut bias  = 0.0f32;

    for i in 0..samples {
        let xi = hammersley(i, samples);
        let h  = ggx_sample_tangent(xi, roughness);
        let l  = reflect_dir(v, h);
        let n_dot_l = l[2].max(0.0);
        let n_dot_h = h[2].max(0.0);
        let v_dot_h = dot3(v, h).max(0.0);
        if n_dot_l > 0.0 {
            let g   = g_smith(n_dot_v, n_dot_l, roughness);
            let g_vis = g * v_dot_h / (n_dot_h * n_dot_v + 1e-7);
            let fc  = (1.0 - v_dot_h).powi(5);
            scale += (1.0 - fc) * g_vis;
            bias  += fc * g_vis;
        }
    }
    (scale / samples as f32, bias / samples as f32)
}

// ── Math helpers ──────────────────────────────────────────────────────────────

fn dot3(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize3(v: [f32; 3]) -> [f32; 3] {
    let len = f32::sqrt(dot3(v, v)).max(1e-7);
    [v[0] / len, v[1] / len, v[2] / len]
}

fn reflect_dir(i: [f32; 3], n: [f32; 3]) -> [f32; 3] {
    let d = 2.0 * dot3(i, n);
    normalize3([d * n[0] - i[0], d * n[1] - i[1], d * n[2] - i[2]])
}

fn radical_inverse(mut bits: u32) -> f32 {
    bits = bits.reverse_bits();
    bits as f32 * 2.3283064365386963e-10
}

fn hammersley(i: u32, n: u32) -> [f32; 2] {
    [i as f32 / n as f32, radical_inverse(i)]
}

/// GGX importance sample in tangent space (N=(0,0,1)).
fn ggx_sample_tangent(xi: [f32; 2], roughness: f32) -> [f32; 3] {
    let a = roughness * roughness;
    let phi = 2.0 * std::f32::consts::PI * xi[0];
    let cos_t = f32::sqrt((1.0 - xi[1]) / (1.0 + (a * a - 1.0) * xi[1]));
    let sin_t = f32::sqrt(1.0 - cos_t * cos_t);
    [sin_t * phi.cos(), sin_t * phi.sin(), cos_t]
}

/// GGX importance sample around arbitrary normal N.
fn importance_sample_ggx(xi: [f32; 2], n: [f32; 3], roughness: f32) -> [f32; 3] {
    let h_tan = ggx_sample_tangent(xi, roughness);
    // Build a TBN frame around N.
    let up = if n[2].abs() < 0.999 { [0.0f32, 0.0, 1.0] } else { [1.0, 0.0, 0.0] };
    let right = normalize3(cross3(up, n));
    let fwd   = cross3(n, right);
    normalize3([
        right[0] * h_tan[0] + fwd[0] * h_tan[1] + n[0] * h_tan[2],
        right[1] * h_tan[0] + fwd[1] * h_tan[1] + n[1] * h_tan[2],
        right[2] * h_tan[0] + fwd[2] * h_tan[1] + n[2] * h_tan[2],
    ])
}

fn cross3(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1]*b[2] - a[2]*b[1], a[2]*b[0] - a[0]*b[2], a[0]*b[1] - a[1]*b[0]]
}

/// Smith G term (Schlick-GGX for IBL — k = roughness²/2).
fn g_schlick(n_dot_v: f32, roughness: f32) -> f32 {
    let k = roughness * roughness / 2.0;
    n_dot_v / (n_dot_v * (1.0 - k) + k + 1e-7)
}

fn g_smith(n_dot_v: f32, n_dot_l: f32, roughness: f32) -> f32 {
    g_schlick(n_dot_v, roughness) * g_schlick(n_dot_l, roughness)
}

// ── Equirectangular sampling ──────────────────────────────────────────────────

fn latlong_to_dir(u: f32, v: f32) -> [f32; 3] {
    let phi   = (u - 0.5) * 2.0 * std::f32::consts::PI;
    let theta = v * std::f32::consts::PI;
    [theta.sin() * phi.cos(), theta.cos(), theta.sin() * phi.sin()]
}

fn dir_to_latlong(d: [f32; 3]) -> [f32; 2] {
    let d = normalize3(d);
    let u = f32::atan2(d[2], d[0]) / (2.0 * std::f32::consts::PI) + 0.5;
    let v = f32::acos(d[1].clamp(-1.0, 1.0)) / std::f32::consts::PI;
    [u, v]
}

/// Bilinear sample of the source equirectangular image at UV.
fn sample_latlong(src: &[[f32; 4]], w: usize, h: usize, uv: [f32; 2]) -> [f32; 3] {
    let u = uv[0].rem_euclid(1.0) * w as f32;
    let v = uv[1].clamp(0.0, 1.0 - 1e-6) * h as f32;
    let x0 = (u as usize).min(w - 1);
    let y0 = (v as usize).min(h - 1);
    let x1 = (x0 + 1).min(w - 1);
    let y1 = (y0 + 1).min(h - 1);
    let fx = u.fract();
    let fy = v.fract();

    let s = |x: usize, y: usize| -> [f32; 3] {
        let p = &src[y * w + x];
        [p[0], p[1], p[2]]
    };

    let lerp1 = |a: [f32; 3], b: [f32; 3], t: f32| -> [f32; 3] {
        [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
    };

    let top    = lerp1(s(x0, y0), s(x1, y0), fx);
    let bottom = lerp1(s(x0, y1), s(x1, y1), fx);
    lerp1(top, bottom, fy)
}

/// SH2 (L0+L1+L2) basis functions evaluated at unit direction `d`.
fn sh2_basis(d: [f64; 3]) -> [f64; 9] {
    let [x, y, z] = d;
    [
        0.282095,                       // l=0
        0.488603 * y,                   // l=1 m=-1
        0.488603 * z,                   // l=1 m=0
        0.488603 * x,                   // l=1 m=1
        1.092548 * x * y,               // l=2 m=-2
        1.092548 * y * z,               // l=2 m=-1
        0.315392 * (3.0*z*z - 1.0),    // l=2 m=0
        1.092548 * x * z,               // l=2 m=1
        0.546274 * (x*x - y*y),         // l=2 m=2
    ]
}
