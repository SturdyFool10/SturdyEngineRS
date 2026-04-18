# 🧠 Sturdy Engine — Architecture & API Refactor TODO

## 🎯 Goals

- Full backend-agnostic core (no Vulkan leakage outside backend/)
- Image-centric high-level API
- Render graph driven execution
- Text system integrated as image operations
- HDR + FP16/FP32 support
- Strong capability/limit querying
- GPU enumeration + switching
- Clean, small, modular file structure

---

# 🔥 Phase 1 — Backend Isolation (CRITICAL FIRST STEP)

## 1. Remove backend leakage from core

- [x] Move backend creation out of `device.rs`
- [x] Create `backend/factory.rs`
  - [x] `create_backend(desc: &DeviceDesc)`
  - [x] `enumerate_adapters(kind: BackendKind)`
- [ ] Remove all direct Vulkan imports from:
  - [x] `device.rs`
  - [ ] any non-`backend/vulkan/*` modules
- [ ] Ensure only `backend/vulkan/*` references:
  - Vulkan types
  - Vulkan extensions
  - Vulkan-specific logic

## 2. Enforce layering rules

- [ ] Core layer (`sturdy-engine-core`) contains:
  - [ ] traits
  - [ ] handles
  - [ ] abstract resources
  - [ ] graph system
- [ ] Backend layer contains:
  - [ ] actual API implementations
- [ ] Engine layer (`sturdy-engine`) contains:
  - [ ] ergonomic API
  - [ ] chaining
  - [ ] runtime management

---

# 🧩 Phase 2 — Capability System Expansion

## 3. Expand `Caps`

- [x] mesh_shading
- [x] ray_tracing
- [x] bindless
- [x] hdr_output
- [x] shader_fp16
- [x] shader_fp64
- [x] image_fp16_render
- [x] image_fp32_render
- [x] dynamic_rendering
- [x] timeline_semaphores

## 4. Expand `Limits`

- [x] max_texture_2d_size
- [x] max_texture_array_layers
- [x] max_color_attachments
- [x] max_compute_workgroup_size
- [x] max_compute_invocations
- [x] max_push_constants_size

## 5. Add format capabilities

- [x] FormatCapabilities struct
- [x] device.format_capabilities(format)

## 6. Add surface/HDR queries

- [x] SurfaceHdrCaps
- [x] HDR10 support
- [x] scRGB support

---

# 🖼️ Phase 3 — Image System Overhaul

## 7. Expand ImageDesc

- [x] dimension
- [x] mip_levels
- [x] layers
- [x] samples
- [x] transient
- [x] clear_value
- [x] debug_name

## 8. Introduce ImageBuilder

- [ ] fluent API

## 9. Add semantic roles

- [ ] Texture
- [ ] ColorAttachment
- [ ] DepthAttachment
- [ ] Storage
- [ ] GBuffer
- [ ] Presentable
- [ ] Intermediate

---

# 🔗 Phase 4 — Image-Centric API

## 10. GraphFrame

- [ ] image()
- [ ] swapchain_image()
- [ ] present()

## 11. ImageNode

- [ ] deferred graph node

## 12. Operations

- [ ] clear()
- [ ] compute()
- [ ] fullscreen()
- [ ] copy_to()
- [ ] blend_over()
- [ ] draw()

## 13. Deferred execution

- [ ] build graph, no immediate execution

## 14. Hook into RenderGraph

- [ ] convert chains to passes

---

# ✍️ Phase 6 — Text System Integration

- [ ] layout/shaping split
- [ ] atlas system
- [ ] engine adapter
- [ ] draw_text API
- [ ] support any writable image

---

# 🌈 Phase 7 — HDR Pipeline

- [ ] HDR formats
- [ ] tonemap pipeline
- [ ] fallback handling

---

# 🎮 Phase 8 — GPU Enumeration & Switching

- [ ] AdapterInfo expansion
- [ ] DeviceManager
- [ ] runtime switching
- [ ] logical resources

---

# 📂 Phase 9 — File Structure Cleanup

- [ ] modular files
- [ ] strict concept separation

---

# 🚀 Phase 10 — Milestone

- [ ] render HDR image
- [ ] fullscreen shader
- [ ] present
- [ ] compute pass
- [ ] text rendering

---

# 🧠 Principles

- Image-centric
- Graph-driven
- Deferred execution
- Backend isolation
- Rebuild on GPU switch
