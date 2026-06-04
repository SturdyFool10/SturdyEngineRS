// Central GPU-resident material registry (Priority 4 — Centralized Bindless Materials).
//
// `MaterialRegistry` owns a single persistent GPU buffer that acts as a
// `StructuredBuffer<GpuMaterialEntry>` for all materials.  Each entry stores
// PBR scalar parameters alongside bindless texture indices so shaders can
// access every material texture through the global bindless heap without any
// per-draw descriptor updates.
//
// ## Usage
//
// At init:
// ```ignore
// let mut mat_registry = MaterialRegistry::new(&engine)?;
// let rock_id = mat_registry.register(MaterialEntry {
//     albedo_idx: rock_albedo_image.bindless_handle(),
//     normal_idx: rock_normal_image.bindless_handle(),
//     roughness: 0.8,
//     ..Default::default()
// });
// ```
//
// Each frame (before any pass that uses the material table):
// ```ignore
// mat_registry.flush(&engine)?;
// frame.bind_material_table(&mat_registry);
// ```
//
// In the Slang shader (include bindless.slang at the top):
// ```slang
// #include "bindless.slang"
//
// StructuredBuffer<GpuMaterialEntry> material_table;
//
// GBufferOut main(VSOut v, uniform DrawConstants push) {
//     GpuMaterialEntry mat = material_table[push.material_id];
//     float4 albedo = bindless_sample(mat.albedo_idx, mat.sampler_idx, v.uv);
//     // ...
// }
// ```
//
// ## Thread safety
//
// `MaterialRegistry` is `Send` and `Sync`-safe as long as `register`,
// `update`, and `flush` are called from a single thread (the main render
// thread).  The GPU buffer is read-only from the GPU side.

use crate::render_world::MaterialId;
use crate::{Buffer, BufferDesc, BufferUsage, Engine, Image, Result};

/// Sentinel index: no texture bound.  Shaders should substitute a 1×1 white
/// texture at this index via the engine's built-in fallback slot, or branch on
/// a flag bit in `flags`.
pub const MATERIAL_NO_TEXTURE: u32 = u32::MAX;

/// Maximum number of material entries in the registry.
///
/// Each `GpuMaterialEntry` is 64 bytes, so the default table occupies 256 KB.
pub const MATERIAL_REGISTRY_CAPACITY: u32 = 4096;

/// CPU-side material descriptor used when registering or updating an entry.
///
/// Texture slots take an optional `u32` bindless index obtained from
/// [`Image::bindless_handle`].  Pass `None` (or omit) for slots that have no
/// texture; the registry substitutes [`MATERIAL_NO_TEXTURE`].
#[derive(Clone, Debug, Default)]
pub struct MaterialEntry {
    /// Bindless index of the albedo/base-colour texture, or `None`.
    pub albedo_idx: Option<u32>,
    /// Bindless index of the tangent-space normal map, or `None`.
    pub normal_idx: Option<u32>,
    /// Bindless index of a combined roughness+metallic texture (R=roughness,
    /// G=metallic), or `None`.  Scalar `roughness`/`metallic` below apply as
    /// a multiplier on top of the texture sample, or as the sole value when no
    /// texture is bound.
    pub roughness_metallic_idx: Option<u32>,
    /// Bindless index of the emissive texture, or `None`.
    pub emissive_idx: Option<u32>,
    /// Bindless index of a sampler that applies to all texture slots in this
    /// material, or `None` to use the engine default (linear, clamp).
    pub sampler_idx: Option<u32>,

    /// Base-colour tint (linear RGB).  Multiplied with the albedo texture; use
    /// `[1, 1, 1]` for no tint.
    pub albedo_tint: [f32; 3],
    /// Opacity [0, 1].  Values below 1.0 require an alpha-blend draw state.
    pub opacity: f32,

    /// Self-emission (linear RGB, HDR values allowed).
    pub emissive_color: [f32; 3],
    /// Metallic factor [0, 1].  Multiplied with `roughness_metallic_idx.g`
    /// when a texture is bound.
    pub metallic: f32,

    /// Roughness [0, 1].  Multiplied with `roughness_metallic_idx.r`
    /// when a texture is bound.
    pub roughness: f32,
    /// Padding / future flags.
    pub flags: u32,
}

impl MaterialEntry {
    /// Convenience: set texture slots from `Image::bindless_handle()` directly.
    pub fn albedo(mut self, image: &Image) -> Self {
        self.albedo_idx = image.bindless_handle();
        self
    }

    pub fn normal(mut self, image: &Image) -> Self {
        self.normal_idx = image.bindless_handle();
        self
    }

    pub fn roughness_metallic(mut self, image: &Image) -> Self {
        self.roughness_metallic_idx = image.bindless_handle();
        self
    }

    pub fn emissive_texture(mut self, image: &Image) -> Self {
        self.emissive_idx = image.bindless_handle();
        self
    }
}

/// GPU layout of a single material entry.
///
/// **Exactly 64 bytes** — 16 × `u32`/`f32`.  Must match the Slang declaration
/// in `material_table.slang` (see engine shaders).
///
/// Texture slots set to [`MATERIAL_NO_TEXTURE`] indicate "no texture".
/// Shaders should test `albedo_idx != MATERIAL_NO_TEXTURE` before sampling.
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuMaterialEntry {
    /// Bindless sampled-image index for albedo. `MATERIAL_NO_TEXTURE` = none.
    pub albedo_idx: u32,
    /// Bindless sampled-image index for the tangent-space normal map.
    pub normal_idx: u32,
    /// Bindless sampled-image index for roughness+metallic (R=rough, G=metal).
    pub roughness_metallic_idx: u32,
    /// Bindless sampled-image index for emissive.
    pub emissive_idx: u32,
    /// Bindless sampler index for all texture slots in this material.
    pub sampler_idx: u32,

    /// `[r, g, b]` albedo tint in linear space + opacity.
    pub albedo_tint: [f32; 3],
    pub opacity: f32,

    /// `[r, g, b]` emissive radiance (HDR) + metallic factor.
    pub emissive_color: [f32; 3],
    pub metallic: f32,

    /// Roughness + flags (upper 16 bits for future use) + padding.
    pub roughness: f32,
    pub flags: u32,
    pub _pad: [f32; 2],
}

impl GpuMaterialEntry {
    const fn from_cpu(e: &MaterialEntry) -> Self {
        Self {
            albedo_idx: match e.albedo_idx {
                Some(i) => i,
                None => MATERIAL_NO_TEXTURE,
            },
            normal_idx: match e.normal_idx {
                Some(i) => i,
                None => MATERIAL_NO_TEXTURE,
            },
            roughness_metallic_idx: match e.roughness_metallic_idx {
                Some(i) => i,
                None => MATERIAL_NO_TEXTURE,
            },
            emissive_idx: match e.emissive_idx {
                Some(i) => i,
                None => MATERIAL_NO_TEXTURE,
            },
            sampler_idx: match e.sampler_idx {
                Some(i) => i,
                None => 0,
            },
            albedo_tint: e.albedo_tint,
            opacity: e.opacity,
            emissive_color: e.emissive_color,
            metallic: e.metallic,
            roughness: e.roughness,
            flags: e.flags,
            _pad: [0.0, 0.0],
        }
    }
}

const GPU_ENTRY_SIZE: u64 = std::mem::size_of::<GpuMaterialEntry>() as u64;

/// Central GPU-resident material table.
///
/// Create one registry per engine session and share it across the frame loop.
/// Register materials with [`register`](Self::register), call
/// [`flush`](Self::flush) each frame before any pass that reads `material_table`,
/// and bind the buffer with [`RenderFrame::bind_material_table`].
pub struct MaterialRegistry {
    gpu_buffer: Buffer,
    /// Shadow copy on the CPU for dirty tracking.
    cpu_entries: Vec<GpuMaterialEntry>,
    /// How many entries are currently allocated (monotonically increasing).
    count: u32,
    /// Index of the first dirty entry.  `u32::MAX` = clean.
    dirty_start: u32,
    /// Index one past the last dirty entry.  `0` when clean.
    dirty_end: u32,
    /// Whether this registry has been registered with the engine's global
    /// resource table.  Set to `true` on the first successful `flush`.
    registered_globally: bool,
}

const CLEAN_START: u32 = u32::MAX;

impl MaterialRegistry {
    /// Create a registry with capacity for [`MATERIAL_REGISTRY_CAPACITY`]
    /// entries.  The GPU buffer is allocated up-front so the address stays
    /// stable.
    pub fn new(engine: &Engine) -> Result<Self> {
        let gpu_buffer = engine.create_buffer(BufferDesc {
            size: MATERIAL_REGISTRY_CAPACITY as u64 * GPU_ENTRY_SIZE,
            usage: BufferUsage::STORAGE | BufferUsage::COPY_DST,
        })?;
        Ok(Self {
            gpu_buffer,
            cpu_entries: Vec::with_capacity(MATERIAL_REGISTRY_CAPACITY as usize),
            count: 0,
            dirty_start: CLEAN_START,
            dirty_end: 0,
            registered_globally: false,
        })
    }

    /// Register a new material and return its stable [`MaterialId`].
    ///
    /// The returned ID is stable for the lifetime of the registry and can be
    /// passed to [`RenderMaterial`](crate::render_world::RenderMaterial) so
    /// ECS objects can reference their GPU material entry.
    ///
    /// Panics when the registry is full (> [`MATERIAL_REGISTRY_CAPACITY`]).
    pub fn register(&mut self, entry: MaterialEntry) -> MaterialId {
        assert!(
            self.count < MATERIAL_REGISTRY_CAPACITY,
            "MaterialRegistry is full ({MATERIAL_REGISTRY_CAPACITY} materials)"
        );
        let id = self.count;
        self.count += 1;
        let gpu = GpuMaterialEntry::from_cpu(&entry);
        self.cpu_entries.push(gpu);
        self.mark_dirty(id);
        MaterialId::from_raw(id)
    }

    /// Update the material at `id` with new parameters.  The change will be
    /// uploaded to the GPU on the next [`flush`](Self::flush) call.
    pub fn update(&mut self, id: MaterialId, entry: MaterialEntry) {
        let idx = id.as_u32() as usize;
        assert!(idx < self.cpu_entries.len(), "MaterialId out of range");
        self.cpu_entries[idx] = GpuMaterialEntry::from_cpu(&entry);
        self.mark_dirty(id.as_u32());
    }

    /// Update only the texture indices for `id` (e.g. after a texture has finished
    /// async loading and obtained its bindless handle).
    pub fn update_textures(
        &mut self,
        id: MaterialId,
        albedo: Option<u32>,
        normal: Option<u32>,
        roughness_metallic: Option<u32>,
        emissive: Option<u32>,
    ) {
        let idx = id.as_u32() as usize;
        assert!(idx < self.cpu_entries.len(), "MaterialId out of range");
        let e = &mut self.cpu_entries[idx];
        if let Some(v) = albedo {
            e.albedo_idx = v;
        }
        if let Some(v) = normal {
            e.normal_idx = v;
        }
        if let Some(v) = roughness_metallic {
            e.roughness_metallic_idx = v;
        }
        if let Some(v) = emissive {
            e.emissive_idx = v;
        }
        self.mark_dirty(id.as_u32());
    }

    /// Upload all dirty entries to the GPU.
    ///
    /// Call once per frame before any pass that samples `material_table`.
    /// No-op when no entries have changed since the last flush.
    pub fn flush(&mut self, engine: &Engine) -> Result<()> {
        // Auto-register in the engine global registry on first flush so the
        // reflection-driven bind-group builder can find "material_table" without
        // an explicit per-frame frame.bind_material_table() call.
        if !self.registered_globally {
            engine.register_global_buffer(
                "material_table",
                self.gpu_buffer.handle(),
                self.gpu_buffer.desc(),
            );
            self.registered_globally = true;
        }

        if self.dirty_start == CLEAN_START || self.dirty_end == 0 {
            return Ok(());
        }
        let start = self.dirty_start as usize;
        let end = self.dirty_end as usize;
        let byte_offset = start as u64 * GPU_ENTRY_SIZE;
        let dirty_slice = &self.cpu_entries[start..end];
        self.gpu_buffer
            .write(byte_offset, bytemuck::cast_slice(dirty_slice))?;
        // Mark clean.
        self.dirty_start = CLEAN_START;
        self.dirty_end = 0;
        Ok(())
    }

    /// The underlying GPU buffer — bind this as `"material_table"` in the frame
    /// or use [`RenderFrame::bind_material_table`].
    pub fn gpu_buffer(&self) -> &Buffer {
        &self.gpu_buffer
    }

    /// Number of registered materials.
    pub fn len(&self) -> u32 {
        self.count
    }

    /// `true` when no materials have been registered.
    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub(crate) fn mark_dirty(&mut self, idx: u32) {
        if self.dirty_start == CLEAN_START {
            self.dirty_start = idx;
            self.dirty_end = idx + 1;
        } else {
            self.dirty_start = self.dirty_start.min(idx);
            self.dirty_end = self.dirty_end.max(idx + 1);
        }
    }
}

// ── Slang shader declaration reference ───────────────────────────────────────
//
// In any shader that reads the material table, declare:
//
//   #include "bindless.slang"
//
//   struct GpuMaterialEntry {
//       uint albedo_idx;
//       uint normal_idx;
//       uint roughness_metallic_idx;
//       uint emissive_idx;
//       uint sampler_idx;
//       float3 albedo_tint; float opacity;
//       float3 emissive_color; float metallic;
//       float roughness; uint flags; float2 _pad;
//   };
//
//   StructuredBuffer<GpuMaterialEntry> material_table;
//
// Then in the frame Rust code call `frame.bind_material_table(&registry)` before
// the draw or dispatch that uses this shader.
