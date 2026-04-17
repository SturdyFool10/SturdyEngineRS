mod surface_color_space;
mod surface_event;
mod surface_info;
mod surface_size;

#[cfg(not(target_arch = "wasm32"))]
mod native_surface_desc;

#[cfg(not(target_arch = "wasm32"))]
pub use native_surface_desc::NativeSurfaceDesc;
pub use surface_color_space::SurfaceColorSpace;
pub use surface_event::SurfaceEvent;
pub use surface_info::SurfaceInfo;
pub use surface_size::SurfaceSize;
