// Bindless resource handles (Track 8a).
//
// A `BindlessHandle<T>` is a stable `u32` index into the engine's global
// bindless descriptor heap. It is valid from the time the resource is
// registered until the engine shuts down. Indices are NEVER invalidated or
// reused during a session.
//
// # Usage
//
// ```ignore
// // Register once at load time:
// let albedo_idx: BindlessHandle<Image> = engine
//     .register_bindless_image(&my_image)
//     .expect("bindless not supported");
//
// // Pass to a shader via push constants:
// frame
//     .shader_pass("my_pass")
//     .target(&output)
//     .constants(StageMask::ALL, &MyConstants { albedo_idx: albedo_idx.index() })
//     .fullscreen(&program)?;
//
// // In the Slang shader (include bindless.slang at the top):
// //   float4 color = g_bindless_textures[NonUniformResourceIndex(push.albedo_idx)]
// //       .Sample(g_bindless_samplers[push.sampler_idx], uv);
// ```
//
// # Shader integration
//
// Shaders that use bindless resources must include `bindless.slang`:
// ```slang
// #include "bindless.slang"
// ```
// and the pipeline layout must include the bindless set at **set 0**.

use std::marker::PhantomData;

/// A stable bindless index for a GPU resource of type `T`.
///
/// Wraps a `u32` index into the engine's global bindless descriptor heap.
/// Obtain via [`Engine::register_bindless_image`] or related methods.
///
/// `T` is a phantom type tag that prevents accidentally using an image index
/// where a sampler index is expected. It carries no data.
///
/// # Safety
///
/// `BindlessHandle<T>` is `Copy + Send + Sync`. The index remains valid for
/// the lifetime of the engine. Never store an index after the engine shuts down.
pub struct BindlessHandle<T> {
    index: u32,
    _marker: PhantomData<fn() -> T>,
}

impl<T> BindlessHandle<T> {
    /// Create a handle from a raw index.
    ///
    /// Prefer `Engine::register_bindless_*` over calling this directly.
    pub fn from_raw(index: u32) -> Self {
        Self {
            index,
            _marker: PhantomData,
        }
    }

    /// The underlying descriptor array index.
    ///
    /// Embed this in push constants or per-draw data buffers and pass it to
    /// the shader as `NonUniformResourceIndex(index)`.
    pub fn index(self) -> u32 {
        self.index
    }
}

// Derive manually so T doesn't need to be Clone/Copy/Debug.
impl<T> Copy for BindlessHandle<T> {}
impl<T> Clone for BindlessHandle<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> std::fmt::Debug for BindlessHandle<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "BindlessHandle({})", self.index)
    }
}

impl<T> PartialEq for BindlessHandle<T> {
    fn eq(&self, other: &Self) -> bool {
        self.index == other.index
    }
}
impl<T> Eq for BindlessHandle<T> {}

impl<T> std::hash::Hash for BindlessHandle<T> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.index.hash(state);
    }
}

// SAFETY: The index is just a u32; no non-Send/Sync data.
unsafe impl<T> Send for BindlessHandle<T> {}
unsafe impl<T> Sync for BindlessHandle<T> {}
