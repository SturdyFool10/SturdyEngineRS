# Sturdy Engine Project Layout

This workspace follows the layering proposed in `deep-research-report.md`.

## Crates

- `sturdy-engine-core`: the main engine crate. It owns backend-neutral types, device state, resources, shader descriptors, render graph compilation, capability queries, and deferred submission semantics.
- `sturdy-engine-ffi`: the C ABI boundary. It exposes opaque `uint64_t` handles, `#[repr(C)]` structs, explicit create/destroy calls, and catches panics before they can cross FFI.
- `sturdy-engine`: the fancier Rust API. It wraps core handles in RAII types and builder-friendly methods for Rust consumers.

## Current Engine Shape

- Images and buffers are first-class resources in the core engine, the C ABI, and the ergonomic Rust API.
- Render graph passes can declare image reads/writes and buffer reads/writes; compilation derives dependency edges, resource lifetimes, image barriers, buffer barriers, and record batches.
- Shader descriptors are stored in core, and the core shader module types include placeholders for Slang reflection output and backend artifacts (`SPIR-V`, `DXIL`, `MSL`).
- Canonical pipeline layout types model Slang parameter blocks as bind groups that can later translate to Vulkan descriptor sets, D3D12 descriptor tables/root signatures, and Metal argument buffers.

## Backend Plan

Backends should stay behind `sturdy_engine_core::backend::Backend`.

- Vulkan: descriptor sets or descriptor buffers, timeline semaphores, per-thread command pools, centralized queue submission.
- D3D12: descriptor heaps/tables, root signatures, fences, per-thread command allocators, PSO libraries.
- Metal: argument buffers, command buffers/encoders, shared events, binary archives.

Each backend lives in a directory module instead of a single file. Keep `mod.rs` as the backend facade and split implementation by concept, for example:

- `config.rs`: backend configuration and defaults.
- `instance.rs`: loader/instance/global API objects.
- `device.rs`: physical device selection, logical device, queues.
- `caps.rs`: capability probing and normalization.
- Future files such as `resources.rs`, `commands.rs`, `sync.rs`, `descriptors.rs`, and `pipelines.rs` should own those backend-specific concepts.

Backends are selected at runtime with `BackendKind`, not by Cargo features. Backend source files are included when the current compilation target can support that API:

- Vulkan module: included for non-`wasm32` targets.
- D3D12 module: included only on Windows.
- Metal module: included only on Apple targets.

`BackendKind::is_available_on_target` and `available_backend_kinds` expose this target availability to runtime selection code.

`BackendKind::Auto` uses this runtime preference order:

- Windows: D3D12 first, then Vulkan.
- Linux: Vulkan.
- macOS: Vulkan first, then Metal. Vulkan is attempted for portability, but Metal is expected to be the reliable native backend.
- iOS, tvOS, visionOS: Metal.

Until real backend constructors are wired, available explicit backend selections use the no-op backend shell while preserving the selected `BackendKind`. That keeps API and FFI flows testable without pretending the native renderer is implemented.

The core render graph remains backend-neutral: consumers declare pass reads/writes, then `flush` compiles dependencies, barriers, batches, and transient lifetime information before submitting.

## API Boundaries

- Core APIs may use Rust enums, `Result`, owned strings, and RAII internally.
- FFI APIs must use plain handles, integer flags, raw pointers, and return `gfx_result_t`.
- The ergonomic Rust API may own/destroy resources in `Drop`, but the FFI API must use explicit `destroy` calls.
- `include/sturdy_engine.h` is generated from `crates/sturdy-engine-ffi` with `cbindgen`; do not edit it by hand.
- Rust `bindgen` is reserved for the opposite direction: generating Rust bindings from platform/backend C headers when a backend needs it.

## Coordinate Contract

App-facing window, surface, render-target, UI, and texture pixel coordinates use a top-left origin with positive X right and positive Y down. `(0, 0)` is the top-left pixel edge. For a target of size `(width, height)`, `(width, height)` is the bottom-right pixel edge, not an addressable pixel center.

Integer pixel and texel indices run from `(0, 0)` through `(width - 1, height - 1)`. Rectangles are represented as `origin + size`; their `max_exclusive`, `right`, and `bottom` edges are exclusive so full-target rectangles may end exactly at `(width, height)`.

`WorldSpace` names game/scene coordinates without assigning a global up axis; individual scenes and cameras may be Y-up, Z-up, or otherwise defined. `ClipSpace` names backend-facing homogeneous coordinates after projection and before perspective divide, so backend adapters own clip/NDC differences instead of leaking them into app, UI, or gameplay code.

Coordinate conversions are explicit at space boundaries: logical/physical window pixels convert through the DPI scale factor, logical window and UI pixels convert to `SurfacePx` without axis flips, `SurfacePx` converts to `Ndc` in the audited backend-facing convention, and `RenderTargetPx` converts to `Uv01` by target extent.

World-space cameras own the projection from scene-defined axes into clip/NDC space. A camera may look through a Y-up world, a Z-up world, or a domain-specific coordinate system, but once positions are projected into app-facing window, surface, render-target, texture, or UI coordinates, the top-left/Y-down pixel contract above applies unchanged. UI hit testing, debug overlays, screenshots, scissor rectangles, and screen-space text must not infer their orientation from the world camera's up axis.

## Binding Generation

Rust is the source of truth for the public C ABI.

To regenerate the C header:

```sh
tools/generate-ffi-header.sh
```

The FFI crate can also generate the header as part of its build:

```sh
STURDY_GENERATE_HEADER=1 cargo check -p sturdy-engine-ffi
```

This build hook intentionally runs only when `STURDY_GENERATE_HEADER` is set, so normal builds do not require `cbindgen` to be installed.

## Testbed

The workspace includes a command-line smoke test binary:

```sh
cargo run -p sturdy-engine-testbed -- vulkan
```

Backend argument options are `auto`, `vulkan`, `d3d12`, `metal`, and `null`. The binary creates an engine, prints the selected backend and adapter name when available, creates an image and uniform buffer, imports both into a frame, declares a graphics pass, flushes, and waits for backend idle.

`null` is useful for testing the engine API and render-graph path on machines where the native graphics stack is unavailable. `vulkan` exercises the real Vulkan loader/device path.

## Vulkan Status

The Vulkan backend currently owns:

- Loader and instance creation.
- Optional validation layer enablement when `VK_LAYER_KHRONOS_validation` is available.
- macOS portability extension hooks for MoltenVK.
- Physical device selection by graphics queue support.
- Logical device and graphics queue creation.
- Capability normalization for basic limits.
- Backend-owned `VkImage` and `VkBuffer` creation/destruction.
- Dedicated `VkDeviceMemory` allocation and binding for each image/buffer.
- Default full-resource `VkImageView` creation for every Vulkan image.
- Command pool creation for the selected graphics queue family.
- Minimal one-time command buffer recording during `flush`.
- Render-graph image and buffer barrier translation to Vulkan memory barriers.
- Queue submit and fence wait for submitted command buffers.
- Descriptor set layout and Vulkan pipeline layout creation from `CanonicalPipelineLayout`.
- Descriptor pool and descriptor set allocation from `BindGroupDesc`.
- Descriptor writes for image and buffer resource bindings.
- `VkShaderModule` creation/destruction from `ShaderSource::Spirv`.
- Compute pipeline creation from SPIR-V shader modules and Vulkan pipeline layouts.
- Command recording binds pass pipelines and descriptor sets before submission.

This is intentionally simple and correct as a first resource, layout, and command path. Later work should replace per-resource dedicated allocations with allocator-backed suballocation and start recording real draw/dispatch work from pass callbacks or pipeline objects.

## Near-Term Milestones

1. Add graphics pipeline creation and dynamic rendering setup.
2. Record real dispatch/draw commands from pass work descriptions.
3. Replace dedicated Vulkan resource allocations with allocator-backed suballocation.
4. Add the Slang compiler integration that fills `ShaderReflection` and `CanonicalPipelineLayout`.
5. Add persistent pipeline cache support.
