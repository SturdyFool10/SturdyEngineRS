# Sturdy Engine Roadmap

## Near-Term Graphics Work

- [x] Add indexed draw coverage in the testbed.
- [x] Replace temporary per-draw Vulkan framebuffer creation with a framebuffer/render-pass cache.
- [x] Add a windowing and surface system.
  - [x] Create platform windows and backend surfaces through a runtime-selected window layer.
  - [x] Keep swapchain/surface ownership separate from the device so surfaces can be resized or fully reconstructed during execution.
  - [ ] Model resize, format changes, color-space changes, and surface recreation as explicit events.
  - [ ] Preserve the ability to reconstruct the surface/swapchain later for HDR mode changes without tearing down the whole engine/device.

## Reflection-Driven Renderer Todo

Goal: build a Slang reflection-driven renderer that talks directly to Vulkan, D3D12, and Metal, exposes the full useful capability surface of the selected GPU/backend, and lets users construct graph work that is submitted without implicit CPU/GPU flushing unless they explicitly request synchronization.

API requirement: consuming the engine must stay simple and graphics API agnostic for common work. Users should be able to create resources, build render/compute passes, bind reflected shader parameters, upload textures, and submit frames without writing Vulkan/D3D12/Metal-specific code. Backend-specific escape hatches are allowed for advanced capability access, but they must not leak into the default path.

### Resource And Binding Foundation

- [x] Represent Slang-reflected descriptor binding kinds, including images, buffers, samplers, and acceleration structures.
- [x] Create Vulkan descriptor set layouts and pipeline layouts from `CanonicalPipelineLayout`.
- [x] Create Vulkan descriptor pools/sets from `BindGroupDesc`.
- [x] Write Vulkan image and buffer descriptors.
- [x] Add `SamplerHandle`.
- [x] Add `SamplerDesc` with filter, mip, address, anisotropy, comparison, LOD, and border-color controls.
- [x] Add `Device::create_sampler` / `destroy_sampler` and ergonomic Rust RAII wrappers.
- [x] Store and destroy Vulkan `VkSampler` objects.
- [x] Add `ResourceBinding::Sampler`.
- [x] Write Vulkan sampler descriptors from bind group entries.
- [ ] Decide whether combined image sampler should be represented as a distinct `BindingKind` or composed from separate image/sampler bindings.
- [x] Add descriptor validation that verifies resource binding kind matches reflected binding kind before backend descriptor writes.
- [x] Add tests for sampled image + sampler binding paths.
- [x] Add an API-agnostic bind group builder for image, buffer, and sampler entries.
- [x] Add an API-agnostic pipeline layout builder for common reflected binding declarations.

### Texture Upload And Copy Work

- [x] Support `CopyImageToBuffer` pass work for GPU-to-CPU/readback paths.
- [x] Add `CopyBufferToImageDesc`.
- [x] Add `PassWork::CopyBufferToImage`.
- [x] Validate copy image extents, mip level, array layer, aspect, and buffer ranges during graph pass insertion.
- [x] Record Vulkan `cmd_copy_buffer_to_image`.
- [x] Add render graph image/buffer state examples for upload staging buffers.
- [x] Add ergonomic texture creation/upload helper that creates a staging buffer, writes CPU data, schedules copy, and transitions to shader-read.
- [x] Add testbed textured quad or textured triangle shader that exercises CPU-to-GPU upload and sampler binding.
- [ ] Add readback verification for a small uploaded texture in headless mode.

### Push Constants

- [x] Represent `push_constants_bytes` in `CanonicalPipelineLayout`.
- [x] Create Vulkan push constant ranges when `push_constants_bytes` is non-zero.
- [ ] Reflect push constant size and stage visibility from Slang instead of always setting `push_constants_bytes` to `0`.
- [x] Preserve push constant byte size when merging graphics shader reflections.
- [x] Add pass-level push constant data, offset, and stage mask.
- [x] Record Vulkan `cmd_push_constants` after pipeline bind and before draw/dispatch.
- [x] Validate push constant byte count against pipeline layout limits and reflected layout.
- [x] Add a testbed path that animates per-draw data through push constants instead of uniform buffer updates.

### Deferred Submission And Synchronization

- [x] Compile render graph passes into barriers and record batches.
- [x] Record Vulkan command buffers from compiled graph work.
- [x] Submit Vulkan graph work through a backend `flush` path.
- [x] Persist whole-resource final graph states across frames for imported images and buffers.
- [x] Replace whole-resource persistent states with subresource/range-aware state tracking.
- [x] Stop treating Vulkan flush as submit-and-wait.
- [x] Add `SubmissionHandle` or equivalent frame/timeline token.
- [x] Add explicit wait APIs for submission wait, frame wait, readback wait, and device idle.
- [x] Make presentation wait only on the synchronization needed for the acquired swapchain image.
- [x] Add deferred destruction tied to submission completion.
- [x] Add tests that prove graph submission does not CPU-wait unless requested.
- [ ] Keep convenience APIs such as `render_image` and `render_surface` allowed to wait where their contract requires it.

### Multithreaded Command Recording

- [x] Replace the single Vulkan `CommandContext` command pool model with per-thread/per-frame command pools.
- [x] Make `RecordBatch` drive actual independent command buffer recording.
- [x] Keep queue submission centralized and externally synchronized per Vulkan queue.
- [x] Decide primary-command-buffer-per-batch vs secondary-command-buffer-per-pass strategy.
- [x] Make descriptor, resource, and pipeline registries safely readable during parallel recording.
- [ ] Add frame-local upload arenas and command allocator recycling.
- [ ] Add multi-queue ownership and synchronization support for graphics, compute, and transfer queues.
- [ ] Add tests with independent graph batches that can record in parallel.

### Transient Resource Memory Aliasing

- [x] Track virtual image and buffer lifetimes in the compiled render graph.
- [x] Count transient image and buffer resources in `AliasPlan`.
- [ ] Expand `AliasPlan` to contain concrete placements: heap/block, offset, size, alignment, lifetime, and compatibility class.
- [ ] Group transient resources by memory type, usage, format/aspect, tiling, sample count, and aliasing compatibility.
- [ ] Interval-pack non-overlapping lifetimes into shared Vulkan memory allocations.
- [ ] Bind transient Vulkan images/buffers to alias-plan offsets instead of independent allocations.
- [ ] Emit required aliasing/discard/layout barriers around reused memory.
- [ ] Add graph compiler diagnostics showing aliasing savings in bytes.
- [ ] Add stress tests for deferred-style resources: GBuffer, depth, HDR, postprocess, and shadow-map lifetimes.

### Capability, Adapter, And Extension Exposure

- [x] Expose backend kind, adapter name, and basic normalized caps.
- [ ] Add `AdapterInfo` with vendor ID, device ID, device type, backend, driver version, and queue families.
- [ ] Add adapter enumeration before device creation.
- [ ] Add `AdapterSelection` to `DeviceDesc` so users can pick a graphics card.
- [ ] Respect `DeviceDesc.validation` when constructing backend configs.
- [ ] Expand `Caps` into a richer `BackendFeatures`/`Limits` model.
- [ ] Expose raw backend extension names and feature names.
- [ ] Let users request required, optional, and disabled features/extensions at device creation.
- [ ] Surface unsupported required features as clear creation errors.
- [ ] Add Vulkan feature chain assembly for requested features.
- [ ] Add normalized flags for ray tracing, mesh/task shaders, descriptor indexing, descriptor buffer, timeline semaphores, dynamic rendering, synchronization2, VRS, HDR presentation, and bindless resources.
- [ ] Keep backend-specific escape hatches so users can opt into new graphics API features before the engine has a normalized abstraction.

### HDR, Surface, And Presentation Control

- [x] Create Vulkan surfaces and swapchains.
- [x] Acquire and present surface images.
- [x] Resize Vulkan surfaces.
- [ ] Enumerate supported surface formats, present modes, and color spaces.
- [ ] Add HDR preference/configuration to device or surface creation.
- [ ] Let users choose SDR/HDR format and color space when supported.
- [ ] Recreate swapchains for HDR mode changes without recreating the whole device.
- [ ] Expose present mode selection: FIFO, mailbox, immediate, relaxed FIFO where available.
- [ ] Add testbed UI/CLI flags for backend, adapter, validation, present mode, and HDR preference.

### Slang Reflection Completeness

- [x] Compile Slang to backend-preferred shader IR for source shaders.
- [x] Extract descriptor binding ranges into canonical group layouts.
- [ ] Extract push constant ranges.
- [ ] Preserve Vulkan descriptor set/binding indices rather than assuming vector order always matches final binding numbers.
- [ ] Track update rate from Slang attributes or explicit engine metadata.
- [ ] Reflect arrays, bindless descriptor arrays, and unsized arrays correctly.
- [ ] Reflect acceleration structure bindings only when the backend feature is enabled.
- [ ] Add reflection tests for separate textures/samplers, combined-style material blocks, push constants, and bindless arrays.

### Low-Level Escape Hatches

- [ ] Define what native handles can be exported/imported per backend.
- [ ] Allow advanced users to inspect raw Vulkan/D3D12/Metal capability data behind backend-specific APIs.
- [ ] Add explicit resource import paths for externally created images/buffers where safe.
- [ ] Add debug object naming and marker APIs.
- [ ] Add capture/debug integration points for RenderDoc, PIX, and Xcode GPU capture where applicable.
