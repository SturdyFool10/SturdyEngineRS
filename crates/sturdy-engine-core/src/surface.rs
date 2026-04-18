mod surface_capabilities;
mod surface_color_space;
mod surface_event;
mod surface_hdr_preference;
mod surface_info;
mod surface_present_mode;
mod surface_recreate_desc;
mod surface_size;

#[cfg(not(target_arch = "wasm32"))]
mod native_surface_desc;

#[cfg(not(target_arch = "wasm32"))]
pub use native_surface_desc::NativeSurfaceDesc;
pub use surface_capabilities::{SurfaceCapabilities, SurfaceFormatInfo};
pub use surface_color_space::SurfaceColorSpace;
pub use surface_event::SurfaceEvent;
pub use surface_hdr_preference::SurfaceHdrPreference;
pub use surface_info::SurfaceInfo;
pub use surface_present_mode::SurfacePresentMode;
pub use surface_recreate_desc::SurfaceRecreateDesc;
pub use surface_size::SurfaceSize;
