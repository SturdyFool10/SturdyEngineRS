# Sturdy Engine Roadmap

## Product Direction

Sturdy Engine is worth using in three modes:

1. **Shader playground** — open a window, write Slang, see it run, tweak parameters live.
2. **Graphical apps and custom UI** — standalone tools, dashboards, inspectors, editors.
3. **Games** — including a path toward footage that can plausibly read as real life.

The simple path must be the best path, not a toy path. Each mode should feel complete without requiring the user to build the runtime shell themselves. The architecture must scale from simple to deep without rewrites.

### What's working today

The Vulkan backend is solid: precise pipeline barriers, 2-frame-in-flight command contexts, pool-slab descriptor allocation, O(n) pass scheduling, and incremental pipeline cache saves. The deferred render graph compiles passes, infers dependencies, and submits without CPU stalls between frames. Shader reflection derives bind groups, validates resource usage, and exposes vertex inputs. The shader playground use case is functional end-to-end.

---

## Priority: Next Quarter

These three tracks unblock the remaining two use cases. Work on them before everything else.

### Track 1 — Game loop shell

The engine has a frame loop and a runtime settings system, but app code can't see the information a game needs.

- [x] Expose `delta_time` and `frame_index` to app code via `AppRuntimeFrame` or a new `FrameClock` helper.
- [x] Add input polling API alongside the existing callbacks: `InputHub::is_key_pressed`, `is_key_just_pressed`, `is_key_just_released`, `mouse_delta`, `mouse_position`.
- [ ] Add gamepad support: wire a platform gamepad backend (gilrs or winit) into the `GamepadAxis` / `GamepadButton` polling API.
- [ ] Add `ActionMap` that binds named actions to keyboard/mouse/gamepad inputs and returns digital/analog values per frame.
- [ ] Add fixed-timestep and interpolation helpers (`FrameClock` with monotonic timing, delta, fixed-step accumulator, and pacing error).
- [ ] Add pointer-lock and relative mouse motion for first-person cameras.
- [ ] Add a default game runtime shell that wraps `AppRuntime` with the above, so a game project needs zero extra plumbing to start.
- [ ] Add a small 2D game sample and a small 3D game sample that use only the default shell.

### Track 2 — 3D lighting and materials

The scene renders instanced geometry with camera transforms but no lighting. Even basic shading changes what the engine looks like.

- [ ] Add directional light: a `DirectionalLight` uniform buffer with world-space direction, colour, and ambient term bound per-frame in the scene shader.
- [ ] Add Lambert diffuse + Blinn-Phong specular shading in the default 3D fragment shader, driven by the light uniform.
- [ ] Add a `Material` descriptor: albedo colour, roughness, metallic, emissive. One uniform buffer per draw call, reflected and bound automatically.
- [ ] Add a directional shadow map pass: depth-only render pass writing to `Depth32Float`, sampled with PCF in the lit pass. The render graph handles ordering and barriers.
- [ ] Add point lights and spot lights as follow-on.
- [ ] Add normal mapping support: read tangent-space normals from a sampled texture, transform with a TBN matrix.
- [ ] Add image-based lighting (IBL) using an environment cubemap for ambient and specular reflection.
- [ ] Add a reference scene that stresses lighting, shadows, and materials with realistic content.

### Track 3 — Asset loading

Everything is created programmatically today. Real projects need to load content from disk.

- [ ] Add `engine.load_texture_2d(path) -> AssetHandle<Texture>` — PNG/JPEG loading via the `image` crate, automatic mip generation, GPU upload.
- [ ] Add `engine.load_mesh(path) -> AssetHandle<Mesh>` — GLTF loading via the `gltf` crate, producing `Vertex3d` arrays, index buffers, and material references.
- [ ] Add `AssetHandle<T>` with state queries: `is_ready()`, `is_loading()`, `is_degraded()`, `failed_reason()`.
- [ ] Add a placeholder policy: missing/loading textures use a visible checkerboard fallback rather than panicking.
- [ ] Add shader hot reload for loose `.slang` files: detect change, recompile, keep last-known-good on failure, emit visible diagnostics.
- [ ] Add asset hot reload for textures and meshes behind the same handle/state system as the streaming path.

---

## Track 4 — Layout engine and widget system

The text system, input callbacks, and Clay UI bindings exist, but there is no layout engine. This is the single blocker for real GUI apps.

- [ ] Integrate `taffy` (pure-Rust flex/grid layout) as the layout engine. Map widget descriptors to taffy nodes, run layout each frame, produce screen-space rectangles.
- [ ] Build a `ScreenUiRoot` that owns a layout tree, an input dispatcher, a focus scope, and a render pass.
- [ ] Add core widgets on top of taffy: `Label`, `Button`, `TextInput`, `Checkbox`, `Toggle`, `Slider`, `ScrollRegion`, `Panel`, `Tabs`.
- [ ] Add stable widget IDs, focus scopes, modal scopes, and per-frame retained state.
- [ ] Add root-level input routing: keyboard, mouse, scroll, pointer capture, and text input ownership.
- [ ] Add theme tokens: typography scale, spacing scale, radii, semantic colors, state colors (hover/pressed/focused/disabled).
- [ ] Add a `WorldUiRoot` for UI rendered onto world-space panels, with ray-to-panel hit testing and render-to-texture support.
- [ ] Add a `TextureUiRoot` for UI rendered into named graph images for downstream composition.
- [ ] Add standalone app conveniences: menu bars, status bars, toolbars, resizable panes, tabbed documents, and inspector panels.
- [ ] Add persistent UI state helpers for window geometry, dock layout, scroll position, and selection.
- [ ] Add accessibility tree generation: roles, names, descriptions, values, bounds, focus, selection, and actions.

### Text system completeness

- [ ] Add grapheme-aware cursor movement, word movement, bidi movement, and selection across wrapped lines.
- [ ] Add single-line editable text field with cursor, selection, focus, clipboard, and keyboard navigation.
- [ ] Add multiline editable text with scrolling, grapheme-aware selection, IME composition, and platform clipboard.
- [ ] Add fallback fonts, emoji, combining marks, ligatures, and OpenType features.
- [ ] Add SDF/MSDF rendering for large scalable text and world-space text.
- [ ] Add atlas residency, eviction, and dirty-rectangle upload policies.
- [ ] Add text performance counters: shaping time, atlas uploads, cache hit/miss, and memory use per frame.

---

## Track 5 — Shader playground auto-UI

The reflection system knows uniform names and types. Auto-generating parameter controls is a force multiplier for the playground use case.

- [ ] Detect push constant `struct` fields from shader reflection after `load_slang_source`.
- [ ] For each `float` field: generate a labelled slider with configurable `[min, max]` range (default `[0, 1]`).
- [ ] For each `uint` field: generate an integer input or toggle.
- [ ] For each `float2`/`float3`: generate a vector input or, where named with colour conventions, a colour picker.
- [ ] For each `bool`: generate a checkbox.
- [ ] Bind live widget values to push constant bytes each frame — zero app code required for a basic interactive shader.
- [ ] Add named presets that save/restore the parameter state for a given shader.
- [ ] Add export to a static screenshot at current parameters.
- [ ] Add a `ShaderPlayground` type that wraps `ShaderProgram` + auto-generated UI into one drop-in component.

---

## Ongoing Architectural Constraints

These apply to all work above.

- [ ] Treat "requires restart" as a failure case unless the OS/compositor makes it impossible.
- [ ] Restrict CPU/GPU waiting to frame-boundary policy: frames-in-flight throttling, swapchain/present, readback requested by the app, or explicit shutdown/device-loss recovery.
- [ ] Add diagnostics for accidental synchronisation: blocking upload, pipeline compile stall, queue idle, fence wait outside shutdown.
- [ ] Keep the deferred frame submission contract: app calls enqueue intent, flush encodes and submits, the GPU does not wait until the next frame's fence.
- [ ] Keep all engine samples and testbed demos on the deferred path so they teach queue-and-finalize behavior.
- [ ] Standardise time as monotonic `Instant`/`Duration` at engine boundaries; expose floating seconds only as convenience views.
- [ ] Standardise colour handling: linear scene colour internally, explicit sRGB decode/encode at I/O boundaries, explicit HDR transfer policy.
- [ ] Standardise resource debug labels for surfaces, images, buffers, passes, pipelines, and generated resources.
- [ ] Standardise capability queries before feature enablement: format support, image usage, sampler limits, queue support, and present modes.

---

## Rendering Quality

Work here deepens the visual output after Track 2 is complete.

### Post-processing pipeline

- [ ] Generalise bloom, AA, and tone mapping into a proper post stack that can host exposure, bloom, temporal effects, sharpening, grading, film grain, and lens effects in any order.
- [ ] Add stronger temporal AA using real motion vectors, camera jitter, and transparency-heavy scenes.
- [ ] Add a transparency-heavy validation scene so temporal and post effects work against composited content.
- [ ] Add motion-blur validation: camera-local vectors produce stable blur; moving objects blur correctly; camera-locked overlays do not blur.

### Mip generation and sampling

- [ ] Add automatic mip generation for sampled textures where format and usage support it.
- [ ] Add explicit mip graph operations: write, read, downsample N to N+1, upsample N+1 into N, transition selected mip ranges.
- [ ] Add sampler controls for LOD bias, min/max LOD, and mip filter choices.
- [ ] Add graph validation for accidental full-resource barriers when a mip/layer range would suffice.

### Photoreal rendering path

- [ ] Add physically-based materials and parameter workflows.
- [ ] Add image-based lighting and reflection workflows.
- [ ] Add atmospheric/volumetric rendering.
- [ ] Add decal and layered-surface workflows.
- [ ] Add high-quality translucency/glass/wet surface path.
- [ ] Build a reference scene aimed at realistic output to drive future rendering priorities.

### 2D and instanced rendering

- [ ] Add a first-class 2D sprite/batch path.
- [ ] Add tilemap and simple layered-scene helpers that compose cleanly with the graph model.
- [ ] Add examples for many instanced quads, per-instance colour/material parameters, and animated GPU-updated instance data.
- [ ] Add effect-oriented instancing such as layered glow sprites or particles.

---

## Full Asset Pipeline

Work here replaces the simple synchronous load from Track 3 with a proper streaming system.

- [ ] Add a `ContentRuntime` that owns asset requests, handles, background I/O, decode/transcode workers, upload plans, residency state, and diagnostics.
- [ ] Keep asset handles stable across load, reload, failure, eviction, and revalidation.
- [ ] Add a staged asset pipeline: Requested → Reading → Decoded → Transcoded → UploadQueued → GpuResident → Ready → Degraded → Failed → Evicted.
- [ ] Add texture streaming: tiny fallback mip immediately, progressive high-mip refinement, budget eviction.
- [ ] Add per-frame upload budgeting: bytes/frame, images/frame, staging memory, transfer queue time.
- [ ] Add staging buffer/ring allocator for async uploads without per-upload allocation churn.
- [ ] Add compressed texture policy: prefer GPU-native block-compressed formats, transcode in workers when not available.
- [ ] Add content priority and cancellation: visible-now, near-future prefetch, UI-critical, editor-preview, low-priority, cancelled/stale.
- [ ] Add development loose-file mode and release package mode behind the same virtual asset paths.
- [ ] Add asset hot reload using the same handle/state system as the streaming path.
- [ ] Add Vulkan sparse/tiled residency as a later optional tier, not the first streaming implementation.

### I/O backends

- [ ] Linux: prefer `io_uring` when available, fall back to a blocking I/O thread pool.
- [ ] Windows: prefer DirectStorage where it fits the Vulkan asset pipeline, fall back to overlapped/thread-pool I/O.
- [ ] Browser/WebAssembly: use browser fetch primitives with the same asset-handle API.

---

## Multi-Window, Workspace, and Docking

- [ ] Add `WindowRegistry` / `WindowManager` owned by the runtime shell with generation-checked `WindowHandle`s.
- [ ] Add `WindowDesc` and route all window creation/destruction through the event-loop command queue.
- [ ] Add per-window surface, swapchain, present mode, frame pacing, DPI/safe-area, cursor state, IME state, and compositor effects.
- [ ] Add `FrameSet` containing zero or more `WindowFrame`s, each acquiring, rendering, submitting, and presenting independently.
- [ ] Allow mixed cadence: one window renders continuously while another redraws only when dirty.
- [ ] Add a `Workspace` model that owns dock trees, tabs, panels, floating panels, and native-window placements.
- [ ] Add split panes, tab stacks, floating panels, detach-to-window, merge-window-back, and drag-panel-between-windows.
- [ ] Preserve panel identity, focus, scroll, undo, and camera state when moving panels between windows.
- [ ] Add workspace serialization with monitor-aware restore and graceful fallback.
- [ ] Add cross-window drag/drop for panels, assets, tabs, documents, nodes, and files.
- [ ] Ensure surface-lost, minimized, or zero-size windows suspend acquire/present without blocking other windows.
- [ ] Add multi-window tests: create, resize, render, minimize, restore, close, and recreate while other windows keep rendering.

---

## Backend and Platform

### Slang compiler service

- [ ] Add `ShaderCompilerService` as an engine subsystem with worker-thread compilation, reflection, cache lookup, and diagnostics.
- [ ] Compile Slang through its in-process C API; make external `slangc` process invocation a developer-tool fallback only, not a runtime dependency.
- [ ] Ensure games ship without requiring `slangc`, the Vulkan SDK, or any external compiler on the player machine.
- [ ] Add hot reload transaction: compile on worker, reflect/validate, queue pipeline rebuild, swap at safe graph boundary, keep last-known-good on failure, emit readable diagnostics.
- [ ] Add release distribution modes: source-shipped, cache-shipped, and hybrid.
- [ ] Add packaging validation that confirms required Slang runtime libraries are present in the game bundle per target platform.
- [ ] Reflect specialization constants.
- [ ] Add per-pass GPU timestamp queries and expose pass timings in `GraphReport`.
- [ ] Keep runtime shader compilation off the render thread; compile/reflection work runs on shader worker jobs.

### Platform isolation

- [ ] Move OS-specific code into `crates/sturdy-engine-platform/src/{linux,windows,macos}/...`; keep engine code on platform-neutral query/apply APIs.
- [ ] Ensure `sturdy-engine` asks `window_appearance_caps()`, `apply_window_appearance()`, `cursor_position()`, `clipboard_caps()`, `ime_caps()`, and `gamepad_caps()` instead of matching on `target_os`.
- [ ] Add directories: `linux/wayland/`, `linux/x11/`, `linux/wayland/background_effect/`, `windows/window_effects/`, `macos/window_effects/`.
- [ ] Return platform capability structs and degraded-apply reports so higher layers choose behavior without knowing the OS implementation.

### Vulkan backend maturity

- [ ] Add Vulkan-specific tests for coordinate-space conversions, viewport/scissor behavior, texture origin handling, and readback orientation.
- [ ] Add Vulkan frame timing, timestamp query, queue wait, present wait, and frames-in-flight diagnostics.
- [ ] Add Vulkan resource lifetime validation for frame-delayed destruction, swapchain recreation, and resize churn.
- [ ] Add multi-surface Vulkan presentation: per-window surface capabilities, independent acquire/present synchronisation.
- [ ] Add Vulkan upload planning: staging rings, copy commands, layout transitions, queue ownership, semaphore sync, and frame-budgeted submission.
- [ ] Add Vulkan parallel command recording using worker-built command buffers where graph dependencies allow.

### WebGPU target

- [ ] Treat WebGPU as a browser/WebAssembly backend only, not a native desktop replacement for Vulkan.
- [ ] Add browser runtime shell handling browser constraints: event loop ownership, canvas sizing, input capture, fullscreen, pointer lock, async device acquisition.
- [ ] Add capability downgrade reporting for browser limits, missing formats, restricted threading, and presentation constraints.
- [ ] Add WebGPU conformance scenes only after Vulkan coordinate, graph, input, and runtime contracts are stable.

### Render threading

- [ ] Add `GraphicsThreadingModel`: `SingleGraphicsOwner`, `ParallelPreparationOnly`, `ParallelCommandEncoding`, `MultiQueueParallel`.
- [ ] Target `ParallelCommandEncoding` for Vulkan where safe and useful.
- [ ] Separate render preparation from backend command recording.
- [ ] Allow worker threads to build render packets, batch keys, upload plans, and graph nodes before the render owner encodes commands.

---

## Reference Milestones

### Milestone A — Shader playground is great
- [ ] Open a shader with `load_slang_source`, see auto-generated parameter sliders, tweak them live.
- [ ] Hot reload a loose `.slang` file and see the change in the running window.
- [ ] Toggle HDR, AA, bloom, transparency, and present policy at runtime without restart.

### Milestone B — GUI apps work
- [ ] Build a multi-panel tool with labels, buttons, text fields, sliders, and scrolling using only the engine's widget layer.
- [ ] Layout reflows correctly on window resize.
- [ ] Input routing (focus, tab order, keyboard navigation) works without app boilerplate.
- [ ] Prove hit testing, clipping, and screenshots agree on top-left/Y-down orientation.

### Milestone C — Games work
- [ ] Build one 2D game and one 3D game using only the default game shell.
- [ ] Input polling, delta time, and gamepad work out of the box.
- [ ] Scene renders with directional lighting, shadows, and basic materials.
- [ ] Load a PNG texture and a GLTF mesh from disk without custom asset code.
- [ ] Switch graphics settings live during gameplay.

### Milestone D — High-end rendering
- [ ] Produce a reference scene evaluated against the goal of plausibly real-looking realtime footage.
- [ ] Demonstrate runtime texture streaming with progressively refined mips and frame-budgeted uploads.
- [ ] Demonstrate PBR materials, IBL, and shadows in realistic content.

### Milestone E — Browser/WebGPU
- [ ] Run a constrained browser/WebGPU sample using the same app-facing contracts as Vulkan.
- [ ] Show clear browser-specific downgrade diagnostics.
