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
- [x] Remove all direct Vulkan imports from:
  - [x] `device.rs`
  - [x] any non-`backend/vulkan/*` modules
- [x] Ensure only `backend/vulkan/*` references:
  - Vulkan types
  - Vulkan extensions
  - Vulkan-specific logic

## 2. Enforce layering rules

- [x] Core layer (`sturdy-engine-core`) contains:
  - [x] traits
  - [x] handles
  - [x] abstract resources
  - [x] graph system
- [x] Backend layer contains:
  - [x] actual API implementations
- [x] Engine layer (`sturdy-engine`) contains:
  - [x] ergonomic API
  - [x] chaining
  - [x] runtime management

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

- [x] fluent API

## 9. Add semantic roles

- [x] Texture
- [x] ColorAttachment
- [x] DepthAttachment
- [x] Storage
- [x] GBuffer
- [x] Presentable
- [x] Intermediate

---

# 🔗 Phase 4 — Image-Centric API

## 10. GraphFrame

- [x] image()
- [x] swapchain_image()
- [x] present()

## 11. ImageNode

- [x] deferred graph node

## 12. Operations

- [x] clear()
- [x] compute()
- [x] fullscreen()
- [x] copy_to()
- [x] blend_over()
- [x] draw()

## 13. Deferred execution

- [x] build graph, no immediate execution

## 14. Hook into RenderGraph

- [x] convert chains to passes

---

# ✍️ Phase 6 — Text System Integration

- [x] layout/shaping split
- [x] atlas system
- [x] engine adapter
- [x] draw_text API
- [x] support any writable image

---

# 🌈 Phase 7 — HDR Pipeline

- [x] HDR formats
- [x] tonemap pipeline
- [x] fallback handling

---

# 🎮 Phase 8 — GPU Enumeration & Switching

- [x] AdapterInfo expansion
- [x] DeviceManager
- [ ] runtime switching
- [ ] logical resources

---

# 📂 Phase 9 — File Structure Cleanup

- [x] modular files
- [x] strict concept separation

---

# 🚀 Phase 10 — Milestone

- [x] render HDR image
- [x] fullscreen shader
- [x] present
- [x] compute pass
- [x] text rendering

---

# 🧠 Principles

- Image-centric
- Graph-driven
- Deferred execution
- Backend isolation
- Rebuild on GPU switch
