# Sturdy Engine Roadmap

## Near-Term Graphics Work

- Add indexed draw coverage in the testbed.
- Replace temporary per-draw Vulkan framebuffer creation with a framebuffer/render-pass cache.
- Add a windowing and surface system:
  - Create platform windows and backend surfaces through a runtime-selected window layer.
  - Keep swapchain/surface ownership separate from the device so surfaces can be resized or fully reconstructed during execution.
  - Model resize, format changes, color-space changes, and surface recreation as explicit events.
  - Preserve the ability to reconstruct the surface/swapchain later for HDR mode changes without tearing down the whole engine/device.
