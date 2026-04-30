# Sturdy Engine Roadmap

> TODO-only merged roadmap. This version folds in the latency, threaded input, render-threading, Vulkan-first backend, browser-only WebGPU, coordinate-system standardization, robustness, platform isolation, flush-free submission, runtime asset streaming, runtime Slang shader compilation, full UI/text-system requirements, and multi-window productivity-workspace requirements.

## Product Direction

Sturdy Engine should be worth using in three modes:

1. Quick visualization and shader play.
2. Graphical apps and custom UI.
3. Full games, including a path toward footage that can plausibly read as real life.

The simple path must be the best path, not a toy path. A small app should be able to open a window, draw something useful, inspect it, and change major runtime settings without rebuilding its shell or restarting.

### Graphics API direction

For now, Vulkan is the singular native graphics API and the reference implementation. The engine should still be written so that additional backends can be added without rewriting the app/runtime/render-graph API, but no abstraction work should slow down making the Vulkan path excellent.

- [ ] Treat Vulkan as the native reference backend until it is strong enough to carry the engine by itself.
- [ ] Keep the public rendering model backend-ready, but do not design around hypothetical backend quirks before Vulkan is correct.
- [ ] Plan WebGPU as a browser/WebAssembly backend only, not as a native desktop replacement for Vulkan.
- [ ] Keep backend-facing types small, explicit, and capability-driven so future native backends can be added behind the same public contracts.
- [ ] Ensure backend differences are handled in backend adapters, not leaked into app/UI/game code.

### UI product direction

The UI system should be one engine feature that scales across three modes instead of three unrelated stacks. A pause menu, an in-world terminal, and a standalone app should share layout, widgets, text shaping, style/effect definitions, input routing, accessibility metadata, and render-graph integration.

- [ ] Treat UI as a first-class renderer client, not as a debug overlay bolted onto the engine.
- [ ] Support three app-facing UI roots:
  - [ ] `ScreenUiRoot`: screen-space UI for HUDs, pause menus, overlays, tools, and standalone apps.
  - [ ] `WorldUiRoot`: UI rendered into world-space panels, billboards, terminals, labels, and diegetic controls.
  - [ ] `TextureUiRoot`: UI rendered into named images for later composition, post-processing, streaming, or use as material inputs.
- [ ] Make the same widget/layout/text model work for game overlays, world UI, and standalone apps, with different input adapters and render targets.
- [ ] Keep the default app-facing coordinate contract top-left/Y-down in pixel units for all screen-space and texture-space UI.
- [ ] Require all 3D/world UI adapters to explicitly convert between ray/world/surface-local coordinates and top-left/Y-down `UiPx`.
- [ ] Make UI rendering capable of using the same shader/material/render-graph infrastructure as the rest of the engine.
- [ ] Keep simple UI code small, while allowing advanced users to opt into custom widgets, custom layout, custom Slang effects, custom render targets, and custom event policies.



### Multi-window product direction

The engine should support productivity-app style workspaces where a single process and binary can own many independent native windows. Windows should not be special one-off shells; each window should be a normal engine render target with access to the same asset system, runtime settings model, renderer, UI system, input/action system, and platform capability queries.

- [ ] Treat multi-window support as a core runtime capability, not as an example-only trick.
- [ ] Allow applications to spawn, close, hide, show, resize, rename, move, and reconfigure windows through engine APIs.
- [ ] Avoid global “the window”, “the surface”, “the swapchain”, “the UI root”, and “the frame” assumptions in public and internal APIs.
- [ ] Give every window a stable engine `WindowId` / `WindowHandle` with generation checks so stale window handles fail safely.
- [ ] Support practical “as many windows as the OS, compositor, GPU, and memory allow” behavior instead of an artificial engine-side fixed limit.
- [ ] Let each window host any combination of screen UI, world rendering, texture UI, debug views, editors, inspectors, node graphs, consoles, shader playgrounds, preview panes, and game views.
- [ ] Make docking/splitting/merging work both within one native window and across multiple native windows.
- [ ] Allow panels, tabs, documents, and viewports to detach into new native windows and merge back without losing state.
- [ ] Keep all windows in the same process and runtime instance by default so resources, caches, settings, jobs, and assets are shared.
- [ ] Keep browser/WebGPU behavior explicit: browser targets may support multiple logical UI surfaces or canvases, but arbitrary native top-level window spawning is a native-platform feature, not a browser guarantee.

## Priority Rules

When choosing what to do next, prefer work that improves:

1. The first 30 minutes of using the engine.
2. The amount of application boilerplate removed.
3. The quality of the default app/runtime shell.
4. The ability to scale from simple usage into deeper control without rewrites.
5. The path toward high-end game visuals and stable iteration.

Deprioritize work that is technically sophisticated but does not materially move one of the product tracks above.

## Runtime Rules

These are architectural constraints, not stretch goals:

- [ ] Treat “requires restart” as a failure case unless the operating system, browser, compositor, or driver makes it impossible.
- [ ] Separate runtime setting apply paths into:
  - [ ] `Immediate`: CPU-state or binding changes that apply next frame.
  - [ ] `GraphRebuild`: pass, pipeline, or graph-topology rebuilds without replacing the surface.
  - [ ] `SurfaceRecreate`: presentation/swapchain recreation while keeping the app alive.
  - [ ] `WindowCreate`: native window creation requested through the runtime command queue.
  - [ ] `WindowDestroy`: native window teardown, surface retirement, and per-window resource cleanup.
  - [ ] `WindowReconfigure`: native window/compositor/background-effect changes.
  - [ ] `DeviceMigration`: live adapter/device migration without app restart.
  - [ ] `BackendUnavailable`: requested backend is not valid for the current target, such as WebGPU outside the browser target.
- [ ] Keep one public runtime settings model that explains which path each setting takes.
- [ ] Make simple runtime settings use the same internal systems as advanced apps instead of separate code paths.
- [ ] Expose capability queries and failure reasons when a requested runtime change cannot be applied exactly.
- [ ] Surface degraded-apply results instead of silently falling back.

## Robustness, Error Handling, And Platform Isolation Rules

The engine should not crash for recoverable runtime, compositor, asset, input, allocation, or configuration failures. A panic is only acceptable for a hard incompatibility that makes the engine impossible to run, or inside test-only fixture code where the panic is the assertion mechanism.

### Panic and unwrap policy

- [ ] Audit production crates for `.unwrap()`, `.expect()`, `panic!`, `todo!`, `unimplemented!`, unchecked indexing, and unchecked arithmetic in runtime paths.
- [ ] Treat production `.unwrap()` / `.expect()` as bugs unless the surrounding code proves the only failure mode is a hard engine incompatibility.
- [ ] Convert recoverable failures into typed errors, degraded runtime setting results, capability rejections, or visible diagnostics.
- [ ] Add a small number of explicit panic-allowed escape hatches with justification comments for impossible internal invariants.
- [ ] Keep test-only `.unwrap()` / `.expect()` allowed when they express fixture setup or expected test invariants, but avoid copying that style into samples, examples, app shell code, platform code, or backend code.
- [ ] Add a CI or local-tooling panic audit that reports new runtime `.unwrap()`, `.expect()`, and `panic!` call sites by crate and module.
- [ ] Consider enabling Clippy `unwrap_used` / `expect_used` as warn-by-default for engine crates after the first cleanup pass, with targeted `allow` annotations only where justified.

### Findings from the current scan

- [x] Clean up Vulkan allocator panic paths in the backend allocator:
  - [x] Replace `expect("fresh block must fit")` with an allocator error that includes requested size, alignment, block capacity, and memory type.
  - [x] Replace `expect("allocation block_id not found in pool")` and `expect("no pool for allocation memory type")` with a structured allocator-corruption or invalid-allocation-handle error.
  - [x] Replace `position(...).unwrap()` and `last_mut().unwrap()` with `Entry`-style logic or explicit error returns.
  - [x] Make deallocation return `Result<()>` so bad allocator state can be reported instead of panicking inside cleanup.
  - [x] Keep debug assertions for impossible allocator invariants, but make release builds return errors with diagnostics.
- [x] Clean up Linux Wayland background-effect panic paths:
  - [x] Replace `expect("inserted state disappeared")` with `HashMap::entry` or a helper that returns `NativeWindowAppearanceError::ApplyFailed` if state creation fails.
  - [x] Treat missing Wayland globals, missing protocol capabilities, invalid foreign handles, and compositor refusal as degraded `WindowReconfigure` results, not crashes.
  - [x] Report the selected background-effect protocol and fallback reason in runtime diagnostics.
- [x] Clean up build-script panics in `sturdy-engine-ffi`:
  - [x] Keep header generation optional unless `STURDY_GENERATE_HEADER` explicitly requests it.
  - [x] When explicitly requested, fail with actionable diagnostics that mention `cbindgen`, output path, and the command that failed.
  - [x] Avoid panicking for workspace path assumptions; return a clear build error instead.
- [ ] Separate policy for tests and examples:
  - [ ] Tests may unwrap fixture setup when the panic message documents the invariant being tested.
  - [x] Examples and testbed apps should show the engine's error-reporting path instead of crashing on common setup failures.
  - [ ] Render-graph tests may keep assertion-oriented `expect` calls under `#[cfg(test)]`, but shared test helpers should carry good failure messages.
- [x] Audit raw-handle, foreign-object, and OS integration code for unchecked assumptions, because these are user-environment failures rather than engine invariants.

### Robust error model

- [x] Add or standardize error categories for:
  - [x] `HardIncompatible`: cannot run the engine or backend at all.
  - [x] `Unsupported`: feature unavailable on the current platform/backend/compositor.
  - [x] `Degraded`: requested feature partially applied with a documented fallback.
  - [x] `InvalidInput`: app/user requested an impossible or malformed operation.
  - [x] `BackendFailure`: driver/API operation failed.
  - [x] `PlatformFailure`: OS/window/compositor operation failed.
  - [x] `ResourceStateCorruption`: engine internal state is inconsistent but can be reported before shutdown.
- [x] Attach enough context to errors to debug them without reproducing immediately: setting key, apply path, backend, platform, adapter, surface size, and relevant handles or resource names.
- [x] Add user-facing diagnostics for failures that app authors or users can fix, and keep internal cause chains for developers.
- [x] Make runtime setting application return exact, degraded, rejected, unavailable, or failed results instead of only `Ok` / `Err`.

### Default submission and synchronization rule

- [ ] Make the default graphics path deferred and frame-boundary synchronized: app/API calls queue work, but do not flush, submit, or wait immediately.
- [ ] Treat implicit per-call flushes as bugs unless they are inside explicitly documented compatibility shims or hard-incompatibility shutdown paths.
- [ ] Ensure normal draw, image, shader, mesh, object, UI, and render-target APIs only enqueue intent/resource mutations for the current frame or a named future frame.
- [ ] Reflect queued shader work, resolve pipeline/binding metadata, batch draw calls/objects/meshes, compile graph dependencies, order passes, encode backend commands, and submit only during the frame finalization path.
- [ ] Restrict CPU/GPU waiting to frame-boundary policy: frames-in-flight throttling, swapchain/present policy, readback completion requested by the app, or explicit shutdown/device-loss recovery.
- [x] Add an expert-only explicit flush API that requires a reason enum and returns a diagnostic report explaining what was submitted, waited on, and why.
- [ ] Add diagnostics for accidental synchronization: blocking upload, immediate readback, pipeline compile stall, queue idle, fence wait, swapchain acquire/present wait, and graph-finalization cost.
  - [x] Report explicit frame flush/wait/present synchronization reason, submission token, and whether the frame submitted, waited, or presented.
- [ ] Keep engine samples and testbeds on the deferred path so examples teach queue-and-finalize behavior instead of immediate-mode flushing.

### Runtime asset, texture, and shader compilation rules

Runtime content should behave like the rest of the engine: app code queues intent, worker systems prepare it, frame finalization makes it visible to the GPU, and the CPU does not stall unless the app explicitly asks for a blocking operation.

- [ ] Add a first-class runtime asset system that supports asynchronous loading, decode/transcode, upload planning, GPU upload, residency tracking, and hot reload without blocking the main/render thread by default.
- [ ] Treat asset requests as queued work: app code asks for a texture/mesh/shader/effect by handle or virtual path, receives a stable handle immediately, and observes readiness/degraded/failure state through polling, callbacks, or diagnostics.
- [ ] Support asset sources through one virtual asset interface:
  - [ ] Loose files on disk.
  - [ ] Packed game archives.
  - [ ] Memory-backed blobs.
  - [ ] Rust `include_bytes!` assets.
  - [ ] Rust `include_str!` Slang shader source.
  - [ ] Browser/WebAssembly fetch/package sources for the future WebGPU target.
- [x] Extract embedded Slang shader source strings from `text_overlay` and other Rust modules into `.slang` files, then load them with `include_str!` so shader code lives in shader assets instead of inline Rust literals.
- [ ] Add OS-specific high-performance I/O adapters behind the platform layer instead of baking them into engine/render code:
  - [ ] Linux: prefer `io_uring` when available and safe, fall back to a blocking I/O thread pool using `pread`/normal file APIs.
  - [ ] Windows: prefer DirectStorage where it fits the Vulkan asset pipeline, fall back to overlapped/thread-pool file I/O.
  - [ ] Browser: use browser streaming/fetch primitives and keep the same asset-handle API even though the backend is WebGPU-only there.
- [ ] Keep all platform I/O choices capability-driven with visible diagnostics for which path was selected and why a faster path was unavailable.
- [ ] Add a staged asset pipeline:
  - [ ] `Requested`.
  - [ ] `Reading`.
  - [ ] `Decoded`.
  - [ ] `Transcoded`.
  - [ ] `UploadQueued`.
  - [ ] `GpuResident`.
  - [ ] `Ready`.
  - [ ] `Degraded`.
  - [ ] `Failed`.
  - [ ] `Evicted`.
- [ ] Add runtime texture streaming with mip-level residency, priority, budget, and fallback texture behavior.
- [ ] Support streaming the lowest usable mip first, then refining higher mips as I/O, decode, transcode, and upload work completes.
- [ ] Add per-asset priorities for visible objects, upcoming scene cells, UI assets, materials, and prefetch hints.
- [ ] Add upload budgets per frame so runtime streaming cannot accidentally destroy latency or frame pacing.
- [ ] Route uploads through the deferred frame work queue and Vulkan transfer/graphics synchronization plan; no texture upload should force queue idle or frame-internal CPU waits during normal rendering.
- [ ] Add texture format capability checks before accepting compressed, storage, sampled, mip-generated, render-target, or sparse-residency paths.
- [ ] Add optional sparse texture / tiled residency support for Vulkan after the normal streaming path is stable, and keep it behind capability checks because not every GPU/driver path will make sparse residency a practical win.
- [ ] Add placeholder asset policy so missing or still-streaming textures, meshes, and shaders produce visible diagnostics and safe fallback visuals rather than crashes.
- [ ] Add asset lifetime and eviction rules that cooperate with frames-in-flight, deferred destruction, hot reload, and render graph resource aliases.
- [ ] Add asset streaming diagnostics: selected I/O backend, queue depth, bytes/sec, decode time, transcode time, upload bytes/frame, upload stalls avoided, residency budget, evictions, failures, and shader compile latency.
- [ ] Add explicit blocking APIs only for app-requested cases like loading screens, tests, screenshots/readbacks, editor import steps, or shutdown; they must report exactly what they waited on.
  - [x] Route screenshot/readback completion through an explicit `ReadbackCompletion` sync helper that reports the submit and wait.

### Runtime Slang shader compilation rules

Slang should be a library dependency of the engine/game build, not an end-user machine dependency. Game users should not need `slangc`, the Vulkan SDK, Visual Studio, or any separate shader compiler installed for normal runtime compilation.

- [ ] Add an engine-owned Slang compiler service that uses the Slang compilation API directly instead of shelling out to `slangc`.
- [ ] Support compiling Slang modules from:
  - [ ] Loose `.slang` files for development and hot reload.
  - [ ] Packed shader assets distributed with the game.
  - [ ] Rust `include_str!` source embedded in app/game code.
  - [ ] Precompiled/intermediate cached shader blobs generated by the engine cache.
- [ ] Add a Rust-facing shader API where the app can write something equivalent to:

  ```rust
  let shader = engine.shaders().load_slang_source(
      ShaderName::new("debug/fullscreen_triangle"),
      include_str!("shaders/fullscreen_triangle.slang"),
      SlangEntryPoints::graphics("vs_main", "fs_main"),
  );
  ```

- [ ] Ensure the above path works on a clean player PC with no external `slangc` executable installed.
- [ ] Bundle or statically/dynamically ship the required Slang compiler libraries according to target packaging rules, and validate startup diagnostics clearly report missing bundled compiler components as packaging bugs.
- [ ] For the Vulkan backend, compile Slang to SPIR-V with Vulkan-appropriate profile, target, matrix layout, binding, and capability settings owned by the engine.
- [ ] Keep future browser/WebGPU shader support behind the same source/entry-point/effect API, but allow the backend adapter to choose WGSL/SPIR-V-cross/Slang target strategy later.
- [ ] Cache shader compilation results by source hash, include graph hash, compiler version, target backend, target profile, feature/capability set, preprocessor defines, specialization constants, and debug/optimization mode.
- [ ] Compile and reflect shaders asynchronously where possible, then make the resulting pipeline/binding changes visible during graph finalization or a graph rebuild transaction.
- [ ] Use reflection to derive or validate descriptor/bind group layouts, push constants, specialization constants, vertex inputs, render-target formats, storage image usage, and resource access declarations.
- [ ] Make shader diagnostics first-class engine errors: source file/module name, include path, entry point, stage, line/column where available, compiler output, target backend, and suggested capability/settings mismatch.
- [ ] Support hot reload for development without changing the runtime distribution model: loose-file reload in dev, embedded/packed stable assets in release.
- [ ] Add policy for failed shader hot reload: keep using the last known-good pipeline, show diagnostics, and retry on the next source change.
- [ ] Allow apps to opt into shipping shader source, shipping compiled shader cache blobs, or shipping both; the engine should be able to compile from source when source is present and validate/load cache blobs when they are present.
- [ ] Add a deterministic shader cache directory and invalidation policy per app/game so players do not pay unnecessary compile cost on every launch.
- [ ] Keep runtime shader compilation off the render thread by default; pipeline creation may become visible at frame finalization, but compile/reflection work should happen on shader worker jobs.
- [ ] Add tests for `include_str!` shader compilation, loose-file shader compilation, missing compiler-library packaging diagnostics, shader cache hit/miss behavior, hot reload failure fallback, and reflected binding validation.

### Platform isolation rule

- [ ] Move OS-specific code into `crates/sturdy-engine-platform/src/{linux,windows,macos}/...` and keep app/runtime/engine code on platform-neutral query/apply APIs.
- [ ] Ensure `sturdy-engine` asks questions like `window_appearance_caps()`, `apply_window_appearance()`, `cursor_position()`, `clipboard_caps()`, `ime_caps()`, and `gamepad_caps()` instead of matching on `target_os` itself.
- [ ] Keep raw-window-handle conversion, Wayland/X11/Win32/AppKit object wrapping, compositor protocols, and native material APIs in the platform crate.
- [ ] Return platform capability structs and degraded-apply reports so higher engine layers can choose behavior without knowing the OS-specific implementation.
- [ ] Add directories for platform internals:
  - [ ] `crates/sturdy-engine-platform/src/linux/wayland/`
  - [ ] `crates/sturdy-engine-platform/src/linux/x11/`
  - [ ] `crates/sturdy-engine-platform/src/linux/wayland/background_effect/`
  - [ ] `crates/sturdy-engine-platform/src/windows/window_effects/`
  - [ ] `crates/sturdy-engine-platform/src/macos/window_effects/`
- [ ] Keep the top-level platform modules as thin adapters that expose shared traits/types and dispatch to OS-specific implementations.

## Coordinate, Unit, And Space Standardization Rules

The engine needs one app-facing coordinate convention before more UI, input, render graph, and backend work piles up.

### Canonical app-facing coordinate contract

- [x] Standardize app-facing screen/window/UI/render-target pixel coordinates as top-left origin, positive X right, positive Y down.
- [x] Define `(0, 0)` as the top-left pixel edge of the target.
- [x] Define `(width, height)` as the bottom-right pixel edge of the target.
- [x] Document that integer pixel indices run from `(0, 0)` through `(width - 1, height - 1)`, while rectangle max edges are exclusive and may equal `(width, height)`.
- [x] Treat cursor coordinates, widget bounds, scissor/clipping rects, screen-space draw commands, and debug overlays as top-left origin by default.
- [x] Convert backend-specific clip/NDC conventions inside backend or render-pass adapter code, not in app/UI/game code.

### Explicit coordinate-space types

Add small strongly-typed wrappers or clearly named structs for common coordinate spaces:

- [x] `WindowLogicalPx`: DPI-scaled window coordinates from the platform layer.
- [x] `WindowPhysicalPx`: physical window coordinates after applying scale factor.
- [x] `SurfacePx`: swapchain/surface pixel coordinates.
- [x] `RenderTargetPx`: offscreen render target pixel coordinates.
- [x] `UiPx`: UI layout and hit-test coordinates.
- [x] `TexelPx`: integer texture texel coordinates.
- [x] `Uv01`: normalized texture coordinate space.
- [x] `Ndc`: normalized device coordinate space.
- [x] `ClipSpace`: backend-facing clip space after projection.
- [x] `WorldSpace`: game/world coordinates, which may be Y-up, Z-up, or otherwise scene-defined.

### Conversion and validation rules

- [x] Add explicit conversion functions for logical-to-physical pixels, window-to-surface pixels, UI-to-surface pixels, surface-to-NDC, and render-target-to-UV.
- [x] Keep Y-flips in one audited location per backend/pass type.
- [x] Add debug assertions for accidentally mixing logical pixels, physical pixels, UVs, and NDC values.
- [x] Add golden validation scenes that draw crosshairs, rects, cursor markers, clipped UI, and texture samples at each corner and edge.
- [x] Add a test scene that proves `(0,0)` is top-left and `(width,height)` is the bottom-right edge for the window, UI, debug overlay, and render target paths.
- [ ] Add a test scene that proves scissor rectangles, texture readbacks, pointer hit tests, and screenshot/export use the same orientation.
- [x] Add documentation for when world-space cameras may use Y-up or Z-up while screen-space remains top-left/Y-down.

### Other standardization wins

- [x] Standardize rectangle representation as `origin + size` plus helper accessors for `min`, `max_exclusive`, `center`, and `contains`.
- [ ] Standardize time as monotonic `Instant`/`Duration` at engine boundaries; do not expose floating seconds except as convenience views.
- [ ] Standardize color handling: linear scene color internally, explicit sRGB decode/encode, explicit HDR transfer/output policy, and named units for nits where applicable.
- [ ] Standardize resource naming and debug labels for surfaces, images, buffers, passes, pipelines, and generated resources.
- [ ] Standardize error reporting as structured diagnostics with user-facing reason, internal cause, severity, and apply-path context.
- [ ] Standardize capability queries before feature enablement: format support, image usage, sampler limits, queue support, present modes, timestamp support, and browser limitations.

## UI, Layout, Text, And Interaction Rules

The UI stack should be designed like an engine subsystem, not like a temporary immediate-mode tool panel. It must remain easy for the first window and first pause menu, but the architecture should not dead-end when used for a full application, a game HUD, or interactive world-space surfaces.

### UI roots and surfaces

- [ ] Add explicit UI root types for screen, world, and texture/offscreen UI instead of making all UI pretend to be a window overlay.
- [ ] Give every UI root a declared coordinate space, target image/surface, scale factor, color space, transparency mode, and input adapter.
- [ ] Allow multiple UI roots per frame: HUD, pause menu, debug overlay, in-world terminal panels, and app/tool panels should not fight over one global UI context.
- [ ] Add root-level policies for focus scope, modal scope, top-layer stacking, pointer capture, keyboard capture, gamepad focus, and text-input ownership.
- [ ] Support rendering UI directly to the swapchain, to intermediate graph images, and to reusable textures/materials.
- [ ] Support dynamic-resolution UI targets for world panels so expensive panels can update less often without changing widget code.
- [ ] Add clear lifetime rules: persistent UI state is owned by stable widget IDs or app state, while frame-local paint/layout commands are rebuilt every frame.

### Layout system requirements

- [ ] Standardize layout units: physical pixels, logical pixels, percentages, em/rem-like font-relative units, viewport units, and explicit world-panel units where needed.
- [ ] Add layout containers for rows, columns, stacks, overlays, grids, absolute positioning, anchors, docking/split panes, scroll regions, and virtualized lists/tables.
- [ ] Add constraint-driven sizing primitives: min/max, fit-content, fill, aspect-ratio lock, intrinsic text/image size, baseline alignment, and content-driven measurement.
- [ ] Add z/layer semantics that are independent of tree order: normal flow, top layer, popover/menu layer, tooltip layer, drag layer, debug layer, and compositor/backdrop layer.
- [ ] Add safe-area and overscan handling for games and TV-like display modes.
- [ ] Add DPI and scale-factor tests that prove layout, hit-testing, text, clip bounds, and effects agree.
- [ ] Add layout invalidation diagnostics so expensive relayouts, text remeasurements, and scroll anchor changes are visible.
- [ ] Add viewport virtualization for large lists, file browsers, inspectors, logs, code editors, inventories, chat windows, and timeline-style widgets.

### Widget, interaction, and state model

- [ ] Define a standard widget event contract with capture, target, bubble, cancellation, default actions, and per-widget override points.
- [ ] Keep default widget behavior complete for keyboard, mouse, scroll, text input, touch-like pointer events where supported, and gamepad navigation.
- [ ] Make every default widget behavior individually disableable or replaceable: focus, hover, press, drag, scroll, keyboard activation, text acceptance, selection, navigation, and gamepad actions.
- [ ] Add a stable action/callback model: `on_pointer_down`, `on_pointer_up`, `on_click`, `on_double_click`, `on_drag_start`, `on_drag`, `on_drag_end`, `on_key_down`, `on_key_up`, `on_text_input`, `on_submit`, `on_cancel`, `on_focus`, `on_blur`, `on_scroll`, and `on_value_changed`.
- [ ] Add state primitives for hovered, pressed, focused, focus-visible, active, checked, selected, disabled, read-only, invalid, loading, dirty, captured, modal, and consumed.
- [ ] Add high-level widgets for labels, buttons, icon buttons, toggles, sliders, ranges, progress bars, text fields, combo boxes, menus, context menus, tabs, accordions, split panes, dock panels, trees, tables, property grids, color pickers, file/path pickers, code editors, consoles, graphs, timeline controls, and asset browsers.
- [ ] Add per-widget style/config structs for standard controls: `CheckboxStyle`, `RadioStyle`, `ToggleStyle`, `SliderStyle`, and related metric structs.
- [ ] Move hard-coded control metrics into style/config: checkbox/radio indicator size, check mark size, toggle track/knob size, slider track height, thumb radius, fill inset, label gap, padding, and corner radii.
- [ ] Add widget click-area policy: indicator-only, label-and-indicator, full-row, and custom hit-shape override.
- [ ] Add per-part widget style hooks for track, fill, thumb, mark, knob, indicator, label, focus ring, disabled overlay, and invalid/error affordance.
- [ ] Make slider input derive its travel metrics from `SliderStyle`/final geometry instead of hard-coded thumb radius constants, including vertical slider coverage.
- [ ] Add tests for tiny controls, oversized thumbs/knobs, transparent tracks, alpha fills, shader borders, full-row click policies, label-only pass-through policies, and mismatched config/display sizes.
- [ ] Add explicit widget visual-overflow policy so shadows, glows, focus rings, and shader effects can be clipped, expanded, or routed through an offscreen pass intentionally.
- [ ] Add undo/redo and command-history hooks for text editing, inspector/property editing, graph editors, and standalone apps.
- [ ] Add drag-and-drop with typed payloads, visual previews, accepted/rejected drop feedback, and cross-root support where possible.
- [ ] Add declarative shortcut/chord registration that resolves conflicts across app, UI root, modal, text input, and game contexts.

### Text system requirements

- [ ] Make one shaped text model serve measurement, painting, hit testing, selection, editing, accessibility labels, clipboard, and serialization.
- [ ] Support grapheme-aware cursor movement, word movement, line movement, bidi movement, rectangular selection where appropriate, and selection across wrapped lines.
- [ ] Support fallback fonts, emoji, combining marks, CJK, Arabic/Hebrew bidi text, ligatures, OpenType features, variable font axes, and missing-glyph diagnostics.
- [ ] Support text runs with font family, size, weight, style, width/stretch, feature tags, language/script, color, opacity, underline, strikethrough, background, outline, shadow, glow, and custom shader parameters.
- [ ] Separate screen-space small text, large display text, and world-space text quality modes instead of forcing one rendering strategy everywhere.
- [ ] Prefer crisp alpha-mask or high-quality grayscale paths for exact screen-space UI text where they produce the best result.
- [ ] Use SDF/MSDF or distance-field variants for large scalable text, world-space text, outlines, glows, and transformed text when they are the better quality/performance tradeoff.
- [ ] Add mip-aware and anisotropic world-text sampling so in-world terminals and labels do not shimmer or crawl at oblique angles.
- [ ] Add text shaping/layout cache keys that include font data, feature tags, script/language, DPI/scale, wrapping width, and relevant rendering mode.
- [ ] Add atlas residency, eviction, dirty upload, page tiling, and frame-delayed destruction policies that do not stall normal frames.
- [ ] Add IME composition, candidate window positioning, clipboard integration, platform text input hints, password/secure text field behavior, and text-field accessibility metadata.

### UI graphics and effects

- [ ] Make fills, borders, clips, masks, shadows, outlines, glows, gradients, image fills, nine-slice images, vector paths, and custom Slang effects use one shape/material contract.
- [ ] Add analytic antialiasing for rects, rounded rects, independent corners, squircles, circles, paths, borders, clips, and focus rings.
- [ ] Add gradient support with color stops, per-gap easing, linear/sRGB/Oklab or engine color-space policy, repeat/mirror modes, and shader-backed evaluation.
- [ ] Add backdrop-filter effects for screen UI: blur, tint, saturation, contrast, brightness, distortion, and masked/local application.
- [ ] Add depth-aware UI effects for game overlays and world UI: depth fade, occlusion policy, soft intersection, outline-through-walls policy, and optional depth-tested labels.
- [ ] Add animation primitives for transitions, transforms, opacity, color, layout, scroll, shader parameters, and visibility with deterministic frame-time input.
- [ ] Add timeline/spring/easing utilities that can run on the CPU by default and feed shader parameters when appropriate.
- [ ] Expand easing into a trait-based animation contract so apps can define reusable custom easing functions, register them by stable ID, and share the same easing between CPU layout/animation and shader parameters.
- [ ] Add per-widget transition configuration for colors, opacity, transforms, layout metrics, and shader uniforms, including color-space selection and reduced-motion policy.
- [ ] Add overdraw, pass-count, offscreen allocation, clip-mask cost, text-atlas cost, and widget batching diagnostics.

### In-game overlay UI

- [ ] Add a first-class `ScreenUiRoot` sample for HUD plus pause menu, with gameplay input suspended or scoped while the pause menu is active.
- [ ] Add UI-to-game input arbitration: gameplay, debug console, modal UI, text field, pointer capture, and gamepad navigation should have explicit priority rules.
- [ ] Add safe-area, aspect-ratio, ultrawide, controller navigation, pause/resume, and focus restoration behavior for game overlays.
- [ ] Add post-process/backdrop integration so pause menus can blur/tint/desaturate the live game image beneath them through the render graph.
- [ ] Add accessibility hooks useful for games: scalable UI, high-contrast themes, reduced motion, remappable navigation actions, subtitles/captions surfaces, and narration metadata where practical.

### In-world and diegetic UI

- [ ] Add `WorldUiRoot` for UI panels attached to world transforms, cameras, entities, bones/sockets, billboards, curved panels, and screen-facing labels.
- [ ] Add ray-to-UI hit testing that maps mouse/controller/VR-like rays onto panel-local top-left/Y-down coordinates.
- [ ] Add focus and capture rules for interacting with in-world panels without stealing unrelated gameplay input forever.
- [ ] Add render-to-texture UI with configurable update rate, resolution scale, mip generation, color space, alpha mode, and material binding.
- [ ] Add world-panel text quality tests for distance, angle, mip transitions, anisotropy, HDR/SDR, bloom, and post-process interaction.
- [ ] Add terminal-style widgets for monospace text, scrollback, cursor blink, selection, command input, and shader/material styling.
- [ ] Add examples for an in-world computer terminal, an inventory panel on a 3D object, floating nameplates, and interactable control panels.

### Standalone app UI

- [ ] Make the UI stack strong enough that a standalone app can be “just UI” without bypassing the engine shell.
- [ ] Add app-window conveniences: menu bars, command palettes, status bars, toolbars, sidebars, resizable panes, tabbed documents, inspector panels, preferences dialogs, file dialogs through platform adapters, notifications, drag/drop files, clipboard, and multi-window planning.
- [ ] Add accessibility tree generation from the UI tree, including roles, names, descriptions, values, bounds, focus, selection, and actions.
- [ ] Add theme tokens and design-system primitives: typography scale, spacing scale, radii, stroke widths, elevations, semantic colors, state colors, animation durations, and density modes.
- [ ] Add persistent UI state helpers for window geometry, panels, dock layout, scroll position, selection, recent files, and command history.
- [ ] Add data-binding and observable-state helpers that remain optional and do not replace direct imperative UI construction for simple apps.

### UI API ergonomics

- [ ] Provide a small simple path: `ui.label`, `ui.button`, `ui.text_field`, `ui.image`, `ui.panel`, `ui.window`, and `ui.menu` should be enough for first apps and tools.
- [ ] Provide a powerful path: custom widgets can participate in layout, paint, hit testing, focus, accessibility, animation, and render-graph effects without private engine hooks.
- [ ] Make every UI command carry enough identity and source-location/debug metadata to diagnose layout churn, duplicate IDs, bad focus scopes, and expensive widget trees.
- [ ] Add typed handles for fonts, images, icons, materials, shaders, sounds-for-UI, and async assets so widgets can refer to resources without blocking.
- [ ] Add snapshot/golden tests for representative UI modes: pause menu, settings screen, inventory, terminal, dashboard app, code editor, world panel, and high-DPI small text.


## Multi-Window, Workspace, Docking, And Surface Rules

The app shell must scale from one game window to a productivity workspace with many windows. A window is a runtime object with platform state, input focus, a surface, optional swapchain, one or more UI roots, and one or more render targets. The engine should own the complexity while app code sees a straightforward `WindowHandle` and `WindowContext`.

### Window registry and ownership

- [ ] Add a central `WindowRegistry` / `WindowManager` owned by the runtime shell.
- [ ] Store every native window in a `WindowRecord` keyed by engine `WindowId`, with a generation counter to reject stale handles.
- [ ] Track the platform `WindowId`, engine `WindowId`, title, role, parent/logical owner, current monitor, DPI scale, safe area, surface size, window size, focus state, minimized/occluded state, and close policy per window.
- [ ] Split window resources into explicit layers:
  - [ ] `WindowRecord`: native window handle, title, size, scale, focus, cursor, IME, compositor capabilities.
  - [ ] `SurfaceRecord`: Vulkan surface, surface capabilities, present support, format selection, present policy.
  - [ ] `SwapchainRecord`: swapchain images, image views, acquire/present synchronization, frame index, surface-lost state.
  - [ ] `WindowUiRecord`: UI roots, docking tree, viewport state, per-window style overrides, accessibility tree root.
  - [ ] `WindowRenderRecord`: render graph outputs, per-window frame pacing, queued present work, diagnostics.
- [ ] Make “primary window” optional. The first window may be special for app convenience, but internal systems must handle zero, one, or many live windows.
- [ ] Keep shared resources global where correct: Vulkan instance/device, allocator, shader compiler, pipeline cache, asset manager, texture streamer, render graph compiler cache, job system, input thread, and runtime settings database.
- [ ] Keep per-window resources per-window: native window, surface, swapchain, present mode, frame pacing state, DPI/safe-area conversion, cursor state, IME state, compositor effects, transparency/blur state, and close behavior.

### Window creation and event-loop command model

- [ ] Route all native window creation/destruction through an event-loop command queue rather than constructing windows from arbitrary worker/render threads.
- [ ] Add `RuntimeCommand::CreateWindow(WindowDesc) -> PendingWindowHandle` and `RuntimeCommand::CloseWindow(WindowHandle)`.
- [ ] Add `WindowDesc` fields for title, initial size, minimum size, resizable, decorations, transparency, blur request, present policy, UI root kind, initial dock layout, icon, cursor policy, and render mode.
- [ ] Use the platform event loop as the authority for native window creation, then publish the resulting engine `WindowHandle` back to the app/runtime.
- [ ] Support creating windows during startup, from UI callbacks, from worker threads via a proxy/channel, and from app/game logic, but perform the native operation only on the valid platform/event-loop path.
- [ ] Convert native OS/windowing errors into typed `WindowCreateError` / `WindowReconfigureError` / `WindowDestroyError` values with degraded-result diagnostics.
- [ ] Never panic because a requested secondary window could not be created; reject the request or degrade the feature unless the first required window cannot exist at all.

### Multi-window event routing and input

- [ ] Route every window-scoped event by engine `WindowId` before it reaches UI/app logic.
- [ ] Keep global/device events separate from window-scoped events so raw mouse/gamepad input and focused-window keyboard input do not get mixed.
- [ ] Give the threaded input system per-window input snapshots plus global device snapshots.
- [ ] Track per-window focus, hover, capture, pointer lock, cursor visibility, cursor icon, IME enablement, text input target, and drag/drop state.
- [ ] Support keyboard shortcuts at multiple scopes: focused widget, focused panel, focused window, workspace, and global app.
- [ ] Allow gamepad navigation to target a focused window, focused UI root, focused panel, or explicitly selected viewport.
- [ ] Support cross-window drag/drop of engine panels, documents, assets, tabs, nodes, and files.
- [ ] Preserve coordinate correctness by converting all pointer positions into the target window's top-left/Y-down `SurfacePx` and `UiPx` spaces before dispatch.

### Rendering model for multiple windows

- [ ] Replace single-surface frame assumptions with a `FrameSet` containing zero or more `WindowFrame`s.
- [ ] Allow each `WindowFrame` to acquire, render, submit, and present independently while sharing the global device, command pools, allocators, shaders, assets, and pipeline cache.
- [ ] Allow one app update/simulation step to feed many window renders, so multiple editor/game views can inspect the same world without duplicating simulation.
- [ ] Support rendering a window from:
  - [ ] the main world camera,
  - [ ] an editor viewport camera,
  - [ ] a UI-only root,
  - [ ] a render-to-texture output,
  - [ ] a diagnostic/debug view,
  - [ ] a custom app callback.
- [ ] Treat every native swapchain output as a render graph leaf target, not as the render graph itself.
- [ ] Let render graph passes share intermediate images across windows when useful, but keep final swapchain acquire/present ownership per window.
- [ ] Ensure surface-lost, minimized, occluded, resized, or hidden windows pause/recreate only their own swapchain without destabilizing other windows.
- [ ] Add per-window present policy, present mode, frame pacing, and latency diagnostics.
- [ ] Support mixed cadence: one window can render continuously while another redraws only when dirty.
- [ ] Add multi-window tests that create, resize, render, minimize, restore, close, and recreate windows while other windows keep rendering.

### Docking, splitting, merging, and workspace model

- [ ] Add a `Workspace` model above UI roots that owns panels, documents, dock nodes, floating panels, and native-window placements.
- [ ] Represent docking layout as data:
  - [ ] `DockNode::Split { axis, ratio, first, second }`
  - [ ] `DockNode::Tabs { active, tabs }`
  - [ ] `DockNode::Panel(PanelId)`
  - [ ] `DockNode::Empty`
- [ ] Allow any dock node/tab/panel to detach into a new native window.
- [ ] Allow a native window's workspace tree to merge into another native window's tree.
- [ ] Preserve panel identity, undo history, selection state, scroll state, text-edit state, viewport camera state, and render settings when moving panels between windows.
- [ ] Add workspace serialization so productivity apps can restore multi-window layouts across runs.
- [ ] Support monitor-aware restore with graceful fallback when monitors are missing, moved, scaled differently, or unavailable.
- [ ] Add APIs for app authors to declare panel factories, allowed docking targets, close rules, persistence keys, and default placements.

### Public API shape

- [ ] Add app-facing APIs similar to:

```rust
let secondary = engine.windows().spawn(WindowDesc::new("Material Preview"));
engine.workspace().move_panel_to_window(material_panel, secondary);
```

- [ ] Add per-window callbacks without forcing apps to manually match raw platform IDs:

```rust
fn window_created(&mut self, ctx: &mut WindowContext<'_>) -> EngineResult<()>;
fn window_event(&mut self, ctx: &mut WindowContext<'_>, event: WindowEvent) -> EngineResult<()>;
fn render_window(&mut self, frame: &mut WindowFrame<'_>) -> EngineResult<()>;
fn window_closed(&mut self, id: WindowHandle) -> EngineResult<()>;
```

- [ ] Allow simple apps to ignore multi-window APIs and still get one default window.
- [ ] Allow advanced apps to create editor-style workspace shells without bypassing the engine runtime.
- [ ] Make every `WindowContext` expose the same core engine APIs: assets, renderer, UI, runtime settings, diagnostics, task spawning, and platform capabilities.
- [ ] Keep platform handles behind explicit escape hatches for integrations, not as the normal multi-window API.

### Browser/WebGPU mapping

- [ ] Treat browser/WebGPU as a backend/target with different window semantics rather than weakening native multi-window design.
- [ ] Map native windows to logical canvases or DOM-hosted surfaces only where the browser integration supports it.
- [ ] Make arbitrary OS-level spawning unavailable on browser targets and report `WindowCreateError::UnsupportedOnTarget`.
- [ ] Keep docking/splitting/merging fully available inside a single browser canvas or DOM container.
- [ ] Keep the same `Workspace` and `DockNode` model across native and browser targets even when the native top-level window backend differs.

## Motion And Multipass Rules

These are product rules for temporal effects and 3D composition quality:

- [ ] Treat camera-local motion vectors as the default contract for post-processing inputs.
- [ ] Make it easy to render camera-locked or screen-locked elements in separate passes so they do not inherit scene motion blur or other temporal artifacts.
- [ ] Treat multipass 3D composition as a first-class engine path, not an awkward escape hatch.
- [ ] Require explicit motion-vector correctness validation for moving cameras, moving objects, animated materials, and camera-locked overlays.
- [ ] Treat incorrect motion vectors as a high-severity rendering bug because they directly damage TAA, motion blur, and temporal stability.

## Direction To Roadmap Mapping

The runtime direction document is not a separate strategy. It explains how this roadmap should be interpreted and prioritized.

### What the current testbed proves

Right now the testbed is still doing engine work in app code:

- [ ] Manually assembling the common frame pipeline.
- [ ] Manually owning debug controls for AA, bloom, HDR, tone mapping, and debug images.
- [ ] Manually managing text atlas uploads and HUD draw plumbing.
- [ ] Manually handling HDR surface policy and surface recreation.
- [ ] Manually handling too much input, latency, frame pacing, and present policy behavior outside a first-party runtime.

Those findings map directly to `P0`. Until they are first-party engine systems, the engine is still asking application authors to build the runtime shell themselves.

### Product rule that drives implementation

- [ ] Keep the built-in app shell on the same renderer/runtime systems as the advanced path.
- [ ] Keep the debug overlay on the same runtime settings, diagnostics, and graph resources as normal apps.
- [ ] Make “more control” reveal deeper layers of the same stack instead of replacing the stack.
- [ ] Keep Vulkan-first implementation details behind backend seams that future backends can implement later.

### Public runtime model this roadmap is aiming at

The `P0` runtime settings work should converge on one first-party runtime controller with:

- [ ] Settings snapshot/query.
- [ ] Transaction-style setting updates.
- [ ] Diagnostics/query surface.
- [ ] Per-setting apply results that distinguish exact apply, degraded apply, rejection, and target/backend unavailability.
- [ ] Runtime-visible latency, input, frame pacing, present, and threading policy.

### Phase mapping

- [ ] `P0` turns the testbed shell into `AppRuntime`, `AppRenderer`, `DebugShell`, `TextOverlay`, standardized coordinate spaces, and a real runtime controller.
- [ ] `P1` proves that the shell is useful immediately for debug draw, datavis, and shader playground work.
- [ ] `P2` proves that the same engine path can support real tool/app UI instead of forcing bypasses.
- [ ] `P3` proves that games can reuse the same runtime/debug/settings stack, threaded input, frame pacing, and latency model instead of rebuilding it.
- [ ] `P4` uses that stable runtime foundation to push image quality and realistic rendering.
- [ ] `P5` deepens the graph/backend architecture only where it materially supports the product tracks above.

---

## P0 — App Shell, Runtime Reconfiguration, And Coordinate Foundation

Until this exists, the engine still asks app authors to assemble too much of the runtime by hand.

### Chunking rules

If a roadmap item is still too large for one prompt, split it again until the prompt can do all of the following in one pass:

- [ ] Define or narrow one public API seam.
- [ ] Move one existing responsibility behind that seam.
- [ ] Keep one existing sample or testbed path working.
- [ ] Verify behavior with a build, test, or sample run.
- [ ] Update the relevant roadmap checkboxes.

### Internalize current testbed boilerplate

- [ ] Add a first-party `AppRuntime` / `AppRenderer` that owns the common frame loop:
  - [ ] Swapchain acquire / present.
  - [ ] HDR/SDR output policy.
  - [ ] Default HDR scene target.
  - [ ] MSAA target and resolve.
  - [ ] Bloom / AA / tonemap chain.
  - [ ] Named debug image outputs.
  - [ ] Diagnostics overlay hook.
  - [ ] Input snapshot selection.
  - [ ] Frame pacing and present policy.
- [ ] Internalize motion-vector generation/debug display as engine features instead of testbed-only plumbing.
- [ ] Internalize the current HUD text path into a first-party text/debug overlay instead of making apps manage atlas pages, uploads, and quad meshes.
- [ ] Add a first-party debug action registry and input binding layer above raw key events.
- [ ] Internalize standard renderer diagnostics:
  - [ ] Adapter/backend display.
  - [ ] Vulkan physical device and queue-family display.
  - [ ] HDR mode.
  - [ ] Present mode.
  - [ ] AA mode and actual sample count.
  - [ ] Bloom state.
  - [ ] Graph timings.
  - [ ] Debug image selection.
  - [ ] Coordinate-space sanity overlay.
  - [ ] Latency, input, queue, and frame-pacing data.

### Coordinate and unit foundation

- [x] Add canonical coordinate-space types and conversion helpers listed in the coordinate-standardization section.
- [x] Convert winit cursor coordinates into engine top-left/Y-down `WindowLogicalPx` and `WindowPhysicalPx` immediately at the platform boundary.
- [x] Convert Vulkan framebuffer, viewport, scissor, and clip-space details behind renderer helper functions so app/UI code never has to remember Vulkan orientation rules.
- [x] Audit all UI layout, hit testing, debug drawing, texture blitting, screenshot/export, and readback paths for bottom-left assumptions.
- [x] Add tests for edge-inclusive/exclusive rectangle behavior.
- [ ] Add debug views for coordinate spaces and DPI scale factor.

### Robustness and platform-isolation execution chunks

- [x] `P0.R1` Add a panic audit script that scans production crates for `.unwrap()`, `.expect()`, `panic!`, `todo!`, and `unimplemented!`, excluding `#[cfg(test)]` blocks and generated files where possible.
- [x] `P0.R2` Define the production panic policy in docs and CI: hard incompatibility may abort; recoverable runtime/platform/backend failures must return structured diagnostics.
- [x] `P0.R3` Convert the Vulkan allocator's deallocation and block-selection panic paths into `Result`-returning code with allocator diagnostics.
- [x] `P0.R4` Convert Linux Wayland background-effect state insertion and protocol fallback paths to non-panicking apply/degrade results.
- [x] `P0.R5` Move Linux Wayland background-effect internals under `platform/linux/wayland/background_effect` and keep `linux/mod.rs` as a thin adapter.
- [x] `P0.R6` Add KDE/KWin blur fallback support behind the Linux platform adapter, with ext-background-effect-v1 preferred when available.
- [x] `P0.R7` Add platform capability queries and degraded apply results so engine/runtime code never matches on Linux/Windows/macOS compositor details directly.
- [x] `P0.R8` Add tests or validation harnesses for no-protocol, ext-only, KDE-only, ext-to-KDE fallback, and no-blur fallback paths.



### Multi-window shell foundation

- [x] Replace single-window app shell assumptions with a `WindowRegistry` keyed by engine `WindowId`.
- [x] Add `WindowHandle` with generation checks so stale handles cannot silently target a newly-created window.
- [x] Add `WindowDesc` and route window creation through the runtime/event-loop command queue.
- [x] Create a default primary window through the same code path used for every later spawned window.
- [x] Route native `WindowEvent`s into per-window engine events before input/UI/app dispatch.
- [x] Store per-window DPI scale, surface size, safe area, cursor state, focus state, and compositor-effect state.
- [x] Update the app shell so closing one non-primary window tears down only that window's UI/surface/swapchain state.
- [x] Add diagnostics for live window count, focused window, hovered window, dirty windows, and windows waiting for surface recreation.

### Runtime settings and no-restart architecture

- [x] Add a unified runtime settings system for:
  - [x] Native backend selection, initially only Vulkan.
  - [x] Browser backend selection, WebGPU only when targeting browser/WebAssembly.
  - [x] Adapter/GPU selection.
  - [x] HDR mode.
  - [x] Present mode.
  - [x] Present policy.
  - [x] Latency mode.
  - [x] Frame-rate cap/pacing mode.
  - [x] Max frames in flight.
  - [x] Threaded input mode.
  - [x] Render threading mode.
  - [x] Surface transparency.
  - [x] Window background effect/material.
  - [x] Antialiasing mode and dials.
  - [x] Post-processing toggles and dials.
  - [x] Shader hot reload and asset hot reload policy.
- [ ] Apply runtime settings changes through the right internal path automatically.
  - [x] Route `SurfaceRecreate` settings through `AppRuntime` with structured `Applied` / `Failed` reports instead of process exit.
  - [x] Report `WindowReconfigure` setting outcomes, including native appearance degradation/failure, through runtime apply notifications.
- [ ] Add a transaction-style runtime reconfiguration path so multiple setting changes can be applied coherently in one step.
- [x] Add explicit notifications for:
  - [x] Setting clamped.
  - [x] Setting degraded.
  - [x] Setting rejected.
  - [x] Setting unavailable on the current platform/backend.

### Window transparency and compositor effects

- [ ] Add window/background transparency support in the application shell.
- [ ] Add surface alpha / transparent clear path support so rendered content can preserve transparency through presentation where the platform allows it.
- [ ] Support Windows material/effect families such as blur-behind, acrylic, mica, and tabbed/titlebar variants through one engine API.
- [ ] Support macOS vibrancy/material integration through the same engine API.
- [ ] Support Linux window background effects through a Linux platform adapter that hides Wayland/X11/compositor details from the engine runtime.
- [ ] Let apps toggle transparency and blur/material effects at runtime without restart.
- [ ] Let apps specify whether the window background effect applies to the whole window or engine-managed regions.

#### Linux background-effect fallback plan

- [ ] Move the current Wayland `ext-background-effect-v1` implementation out of `linux/mod.rs` into focused modules:
  - [ ] `linux/wayland/background_effect/mod.rs`
  - [ ] `linux/wayland/background_effect/ext_background_effect_v1.rs`
  - [ ] `linux/wayland/background_effect/kde_blur.rs`
  - [ ] `linux/wayland/foreign_surface.rs`
  - [ ] `linux/wayland/globals.rs`
- [ ] Add a Linux-facing abstraction:
  - [ ] `LinuxBackgroundEffectBackend::ExtBackgroundEffectV1`
  - [ ] `LinuxBackgroundEffectBackend::KdeBlur`
  - [ ] `LinuxBackgroundEffectBackend::TransparentNoBlur`
  - [ ] `LinuxBackgroundEffectBackend::UnsupportedDisplayServer`
- [ ] Prefer `ext-background-effect-v1` when the Wayland compositor exposes it and reports blur capability.
- [ ] Fall back to KDE/KWin `org_kde_kwin_blur_manager` / `org_kde_kwin_blur` when `ext-background-effect-v1` is unavailable, lacks blur capability, fails to bind, or fails during initial apply.
- [ ] Fall back to transparent/no-blur presentation when neither protocol is available or when the display handle is not a supported Wayland/X11 target.
- [ ] Return degraded `WindowReconfigure` results instead of treating missing blur protocols as fatal.
- [ ] Track one background-effect state per surface key with protocol kind, region, size, compositor capability version, and last apply result.
- [ ] On enable/update:
  - [ ] Convert engine window/effect regions into surface-local top-left/Y-down rectangles.
  - [ ] Clamp empty or zero-sized regions to a safe no-op rather than panicking.
  - [ ] For `ext-background-effect-v1`, create or reuse the surface effect object, call `set_blur_region(Some(region))`, and apply on the next `wl_surface.commit`.
  - [ ] For KDE blur, create or reuse the blur object, call `set_region(region)`, `commit()`, and commit the surface.
- [ ] On disable:
  - [ ] For `ext-background-effect-v1`, call `set_blur_region(None)`, destroy the effect object if no longer needed, and commit the surface.
  - [ ] For KDE blur, call the manager `unset(surface)` path or release the blur object according to the protocol state, then commit.
- [ ] Handle compositor capability changes:
  - [ ] If `ext-background-effect-v1` blur capability disappears, downgrade to KDE blur if available.
  - [ ] If KDE blur disappears or fails, downgrade to transparent/no-blur with a visible diagnostic.
  - [ ] If a better protocol appears later, allow a runtime re-apply without restarting.
- [ ] Add diagnostics fields for `window_effect_protocol`, `window_effect_fallback_reason`, `blur_region`, `compositor_supports_blur`, and `last_window_effect_error`.
- [ ] Add fake-global/fake-protocol tests for:
  - [ ] ext-background-effect success.
  - [ ] ext-background-effect missing blur capability.
  - [ ] ext-background-effect bind failure falling back to KDE blur.
  - [ ] KDE blur success.
  - [ ] both protocols missing.
  - [ ] disable/reenable paths.
  - [ ] resize and region update paths.

### First-run usability

- [ ] Add shader hot reload for Slang shaders with clear in-app compile errors.
- [ ] Add asset hot reload for textures, meshes, and other common inputs.
- [ ] Add stable debug/error reporting for missing or stale assets.
- [ ] Add screenshot/export helpers.
- [x] Add frame graph visualization / inspection UI.
- [ ] Add image inspection for named graph resources.
- [ ] Add GPU timing and pass timing summaries.
- [x] Add a one-file sample that opens a window, uses top-left pixel coordinates, draws a rect at each corner, and displays cursor position.

---

## P1 — Quick Visualization, Debug Draw, And Shader Playground

This phase proves the runtime shell is immediately useful before the engine tries to be a full app or game framework.

- [ ] Add immediate debug drawing for points, lines, rects, circles, text labels, and image quads in top-left/Y-down screen coordinates.
- [ ] Add optional world-space debug drawing that explicitly declares its camera and projection space.
- [ ] Add shader playground support with hot reload, reflected parameter UI, named render targets, and screenshot/export.
- [ ] Add plot/datavis helpers that use the same runtime renderer and debug overlay as normal apps.
- [ ] Add coordinate-space validation widgets and examples to catch orientation regressions early.
- [ ] Add a runtime diagnostics panel that works before the full UI stack is complete.

---

## P2 — App UI Must Feel Complete

The Clay/UI foundation is promising, but the app-facing shell is still missing core behavior that makes people stay on the engine path instead of bypassing it.
### UI mode coverage

- [ ] Build one UI architecture that supports screen overlays, in-world panels, offscreen texture UI, and standalone app windows.
- [ ] Add `ScreenUiRoot`, `WorldUiRoot`, and `TextureUiRoot` examples before adding too many specialized widgets.
- [ ] Add samples for a pause menu over a live scene, an in-world terminal rendered onto a mesh, and a standalone dashboard/tool app.
- [ ] Ensure all three samples share the same layout, widget, text, style, animation, input, and accessibility paths.
- [ ] Add root-specific adapters only where necessary: input mapping, render target, scale factor, color space, depth/occlusion, and update frequency.

### Layout and state foundation

- [ ] Add layout primitives for flex-like rows/columns, grids, overlays, anchors, docking, split panes, absolute layers, scroll regions, and virtualized lists/tables.
- [ ] Add intrinsic measurement for text, images, icons, custom widgets, and aspect-ratio constrained elements.
- [ ] Add stable widget IDs, focus scopes, modal scopes, top-layer scopes, drag scopes, and persistent state scopes.
- [ ] Add root-level state persistence for standalone apps: window layout, dock layout, split positions, scroll offsets, selection, and recent command history.
- [ ] Add duplicate-ID, layout-cycle, invalidation, and expensive-remeasure diagnostics.


### Text, input, and editing

Prompt-sized text execution order:

- [ ] `P2.T3` Add text quality validation scenes and screenshot/golden coverage for scale factors, HDR/SDR, fallback fonts, and animated UI.
- [ ] `P2.T4` Add text performance instrumentation and budgets for shaping, caching, atlas uploads, draw calls, and memory use.
- [ ] `P2.T5` Upgrade glyph atlas lifetime, dirty uploads, eviction, tiling, and backend-limit handling for real UI workloads.
- [ ] `P2.T6` Integrate engine-side text shaders for alpha mask, SDF/MSDF, outlines, shadows, and linear-light blending.
- [ ] `P2.T7` Expose app-facing rich text controls after measurement, rendering, and atlas behavior are stable.
- [ ] `P2.T8` Build editable text, IME, selection, clipboard, focus, and keyboard navigation on top of the shared text model.

Prompt-sized text follow-up chunks:

- [ ] `P2.T3a` Add a deterministic text validation harness that can render `ui_demo` text scenes at fixed sizes and scale factors without manual window resizing.
- [ ] `P2.T3b` Add screenshot/golden coverage for small alpha-mask UI labels, dense tables, code-like text, and large SDF display text over dark and light UI backgrounds.
- [ ] `P2.T3c` Add resize, scrolling, clipping, and fractional-position scenes that catch stale clip extents, shimmer, and thickness changes.
- [ ] `P2.T3d` Add fallback-script scenes for Latin, CJK, emoji, combining marks, Arabic/Hebrew bidi text, ligatures, and missing-glyph diagnostics.
- [ ] `P2.T4a` Add `textui` prepared-scene cache hit/miss counters keyed separately from layout measurement caches.
- [ ] `P2.T4b` Add per-frame text timings for shaping, glyph rasterization, atlas snapshotting, tiling, upload recording, and mesh construction.
- [ ] `P2.T4c` Add text memory counters for font data, shaped layouts, prepared scenes, atlas pages, cached snapshots, and GPU atlas images.
- [ ] `P2.T4d` Add resize-specific text telemetry so cache churn is visible as stable-label hits, wrapped-label remeasurements, atlas uploads, and evictions.
- [ ] `P2.T5a` Add stable atlas page handles with frame-delayed destruction so UI text texture identity survives normal window resize and layout churn.
- [ ] `P2.T5b` Add dirty-rectangle atlas upload plumbing and fall back to whole-page upload only when the backend or tiling path requires it.
- [ ] `P2.T5c` Add atlas occupancy and eviction policy tests for scrolling lists, dense tables, and mixed fallback fonts.
- [ ] `P2.T5d` Add backend-limit tests for page sizing, tiling, texture count, sampler selection, and degradation diagnostics.
- [ ] `P2.T6a` Split alpha-mask text sampling from SDF/MSDF sampling so exact 1:1 UI text can use the sharpest sampler path.
- [ ] `P2.T6b` Validate SDF/MSDF field range, outline, shadow, glow, and opacity behavior against screenshot cases before using MSDF for screen-space UI text by default.
- [ ] `P2.T6c` Implement explicit linear-light text blending policy for SDR, HDR, tonemapped, and transparent UI targets.
- [ ] `P2.T7a` Define a shared rich-text run model used by measurement, painting, hit testing, accessibility labels, and future editing.
- [ ] `P2.T7b` Expose per-span typography, color, OpenType features, underline, strikethrough, background highlight, outline, shadow, glow, alignment, truncation, and wrapping modes.
- [ ] `P2.T8a` Build the first single-line editable field using the shared shaped-run model, with cursor movement, selection, focus, clipboard, and keyboard navigation.
- [ ] `P2.T8b` Add multiline editing, scrolling, grapheme-aware selection, bidi cursor movement, IME composition, and platform clipboard integration.

### Widget layer

- [ ] Add remaining standard widgets:
  - [ ] Color pickers.
  - [ ] Date / time / date-time selectors.
  - [ ] Code/rich-text editor.
- [ ] Add inertial/momentum scroll physics, scroll snapping, sticky children, and anchor preservation.
- [ ] Add multiline text inputs with wrapping, scrolling, selection, clipboard, IME, undo/redo, soft tabs, line navigation, and shape-aware clipping.
- [ ] Add stylable multiline text editing rich enough for code editors: per-range styling, syntax/highlight spans, gutters, line numbers, diagnostics, inline widgets, minimap hooks, code folding hooks, and performant viewport virtualization.
- [ ] Add first-class UI layering and stacking contexts so apps can declare what renders behind/in front of what without relying on tree order hacks.
- [ ] Add full dismiss policies for top layers: escape mapping, focus loss, nested scope, delayed pointer capture, and outside-click behavior.
- [ ] Add shader/effect style slots for backgrounds, borders, masks, text fills, outlines, shadows, glows, backdrop filters, and per-state transitions with Slang parameter binding and render-graph pass integration.
- [ ] Add shape-aware rendering, clipping, hit testing, focus rings, shadows, and effect regions so rounded rects, independent corner shapes, squircles, paths, and masks behave consistently across input and paint.
- [ ] Add fancy default border options: per-side/per-corner styling, inner/outer/center strokes, dashed/dotted/double strokes, gradient borders, image/shader borders, glow/bloom borders, and polished focus/error/selection presets.
- [ ] Add virtual rich/code editor support, scroll anchoring, and selection retention across scroll.
- [ ] Keep widgets composable with shader-driven visuals instead of forcing a theme-only path.
- [ ] Treat every widget as a shape-aware object for focus rings, scroll regions, shadows, and accessibility bounds.

### UI coordinate and input correctness

- [x] Ensure all UI layout, hit testing, paint, clipping, scroll offset, and focus bounds use top-left/Y-down `UiPx`.
- [ ] Add UI validation scenes for each corner, edge, nested scroll region, modal/top layer, scale factor, and framebuffer size.
- [x] Ensure cursor coordinates map to widget hit regions without any hidden Y-flip.
- [ ] Ensure texture-backed UI widgets document whether their source rects are texel-space top-left rectangles or normalized UVs.
- [ ] Add screenshot/golden tests that catch half-pixel, inclusive/exclusive edge, and DPI conversion regressions.



### Docking workspace and multi-window UI

- [ ] Add a `Workspace` model that owns dock trees, tabs, panels, documents, floating panels, and native-window placements.
- [ ] Implement split panes, tab stacks, floating panels, detach-to-window, merge-window-back, and drag panel between windows.
- [ ] Make panel movement preserve state: focus, text selection, scroll offset, undo stack, viewport camera, and runtime settings.
- [ ] Add cross-window drag/drop routing for panels, assets, tabs, documents, nodes, and files.
- [ ] Add per-window menu bars, command palettes, status bars, overlays, and modal layers.
- [ ] Add workspace persistence with monitor-aware restore and fallback placement.
- [ ] Add a productivity-app sample that opens multiple windows from the same process and moves docked panels between them.

### UI rendering completeness

- [ ] Add full render-graph resource binding generation instead of pass skeletons:
  - [ ] Bind group creation and reflected binding layout merge for UI shader slots.
  - [ ] GPU buffer allocation/upload and binding for per-pass parameter buffers.
  - [ ] Built-in UI uniform parameter packing for rect, UV, shape, clip, state, time, and DPI scale.
- [ ] Integrate real shader pipelines for all UI slots in engine runtime.
- [ ] Define the UI shape contract shared by rendering, input, clipping, and effects.
- [ ] Make UI antialiasing analytic and shape-aware for fills, borders, outlines, masks, and clips.
- [ ] Add GPU gradient shader implementation and parameter packing contract used by engine pipelines.
- [ ] Add text outline rendering path in engine shader side.
- [ ] Add a UI material/effect model with slots for text fill/outline/shadow/glow, inner shadow, drop shadow, bloom/glow extraction, and custom Slang pass hooks.
- [ ] Add element-scoped shader overrides with stable reflection and clear diagnostics.
- [ ] Allow UI shaders to sample render-graph resources through explicit declarations.
- [ ] Add backdrop-filter style UI effects that operate on graph images behind the widget.
- [ ] Add a pause-menu/backdrop sample where a UI tile blurs and tints the live game image beneath it without manual app-side graph wiring.
- [ ] Add offscreen UI compositing for effects that require extra passes without forcing every widget into a texture.
- [ ] Add render-graph diagnostics for UI pass count, offscreen allocations, clip mask cost, shader slot usage, overdraw, and batched/unbatched draws.
- [ ] Add offscreen-to-world UI sample path in `sturdy-engine` scene layer.
- [ ] Add examples for dashboards, inspectors, and multi-panel tools.

---

## P3 — Game Development Path

This track must make the engine viable for full games without forcing every project to build a custom runtime shell first.

### Frame, time, input, and runtime

- [ ] Add fixed-timestep and interpolation helpers.
- [ ] Add input action/binding helpers above raw key events.
- [ ] Add simple camera controllers for common 2D and 3D cases.
- [ ] Add a default game runtime shell that reuses the same renderer/debug/runtime-settings systems as apps and tools.
- [ ] Split the app shell into explicit input drain, update/simulation, render preparation, command recording, submit, and present phases.
- [ ] Add `FrameClock` and `FramePacer` helpers with monotonic timing, frame index, delta, fixed-step state, and pacing error.
- [ ] Add frame targets for uncapped, VSync-driven, fixed Hz, and display-refresh-minus-margin modes.

### Latency and frame pacing

- [ ] Add a first-party `LatencyController` with presets for `Throughput`, `Balanced`, `LowLatency`, and `UltraLowLatency`.
- [ ] Add timestamped input events and per-frame input-age diagnostics.
- [ ] Add configurable input sampling timing: OS-event receipt, start of frame, before simulation, and before render.
- [ ] Add optional render-only late latching for camera, pointer, and controller state.
- [ ] Replace hard-coded `flush -> wait -> present` style convenience paths with a configurable CPU/GPU queue policy.
- [ ] Remove flushes from default engine behavior: API calls enqueue work, while graph compilation, batching, command encoding, submission, and optional frame-boundary waits happen after `render`.
- [ ] Ensure the default cadence is `collect intent -> finalize frame -> submit -> present`, not `call API -> flush -> wait`.
- [ ] Keep explicit flushing available only for named needs such as readbacks, screenshots, blocking resource upload, external interop, debug capture, tests, shutdown, or API-consumer-requested synchronization.
- [ ] Add `max_frames_in_flight` runtime setting with diagnostics for actual queue depth.
- [ ] Add present policy presets above raw present modes: `Auto`, `NoTear`, `LowLatencyNoTear`, `LowLatencyAllowTear`, and `Explicit`.
- [ ] Add latency overlay lines for input age, CPU update time, CPU render preparation time, command recording time, GPU time, present wait, queue depth, and pacing error.
- [ ] Add a `latency_lab` testbed scene for comparing latency modes under CPU and GPU load.

### Threaded input

- [ ] Add a dedicated input-processing thread.
- [ ] Keep OS window/event-loop ownership on the winit thread, but forward normalized keyboard, mouse, text, focus, and pointer events to the input thread immediately.
- [ ] Poll gamepads from the input thread through a pluggable gamepad backend.
- [ ] Timestamp all input events at source, receipt, processing, snapshot publish, simulation sample, and render sample time where available.
- [ ] Publish low-lock or lock-free `InputSnapshot`s for frame code.
- [ ] Add action maps that resolve keyboard, mouse, text, and gamepad input away from the main thread.
- [ ] Add configurable key repeat, modifier, deadzone, chord, double-click/tap, and text-input handling.
- [ ] Add UI input feedback so UI consumption can affect action dispatch without making the input thread depend on UI internals.
- [ ] Add input diagnostics: queue depth, snapshot age, newest event age, oldest pending event age, dropped/coalesced event count, and gamepad poll timing.
- [ ] Support inline input mode as a fallback for debugging and constrained platforms.
- [ ] Add tests proving cursor coordinates remain top-left/Y-down after threaded input processing.

### Render threading abstraction

- [ ] Add `GraphicsThreadingModel` with `SingleGraphicsOwner`, `ParallelPreparationOnly`, `ParallelCommandEncoding`, and `MultiQueueParallel`.
- [ ] For Vulkan, target `ParallelCommandEncoding` where safe and useful.
- [ ] For future browser WebGPU, target `SingleGraphicsOwner + ParallelPreparationOnly`.
- [ ] Add backend threading capability queries.
- [ ] Separate render preparation from backend command recording.
- [ ] Make render preparation produce deferred frame work instead of forcing backend submission from app-facing calls.
- [ ] Allow worker threads to build render packets, batch keys, upload plans, shader reflection jobs, and graph nodes before the render owner/finalizer encodes Vulkan commands.
- [ ] Add backend-neutral `RenderPacket`s that can be built on worker threads.
- [ ] Add a render-owner thread path where all actual graphics API calls happen on one thread.
- [ ] Add Vulkan parallel command recording path using worker-built command buffers or pass batches where graph dependencies allow it.
- [ ] Keep future backend support compatible by ensuring app/game code emits render intent rather than raw backend commands.
- [ ] Add render-threading runtime settings: `Auto`, `SingleRenderThread`, `ParallelPreparationOnly`, `ParallelCommandRecording`, and `MultiQueueExperimental`.
- [ ] Add render diagnostics: worker count, packet build time, command recording time, queue submit time, frames in flight, and backend threading model.



### Multi-window game/editor view support

- [ ] Allow one simulation update to render multiple views into multiple windows.
- [ ] Support independent editor cameras, game cameras, preview cameras, and UI-only windows.
- [ ] Support mixed redraw policy where the game window renders continuously while inspectors/previews redraw only when dirty.
- [ ] Add per-window input focus and capture rules for game view, editor viewport, terminal, and UI-only windows.
- [ ] Add a sample with a running game view, detached asset inspector, detached debug console, and detached render preview.

### Content and iteration loop
### In-world UI, diegetic surfaces, and game overlay UI

- [ ] Add game-overlay UI sample with HUD, pause menu, modal settings, controller navigation, and gameplay-input suspension.
- [ ] Add in-world terminal sample using `WorldUiRoot`, render-to-texture, mip generation, ray-to-panel hit testing, and terminal text editing.
- [ ] Add world-panel material integration so UI textures can be used by normal scene materials with alpha, emissive, HDR, bloom, and depth behavior.
- [ ] Add input arbitration between gameplay, overlay UI, debug UI, text input, and in-world panels.
- [ ] Add late-latched pointer/ray input for world UI so interaction feels responsive without corrupting simulation input.
- [ ] Add world-UI diagnostics: target resolution, update rate, snapshot age, mip generation cost, hit-test ray, focused panel, captured pointer, and text atlas use.


- [ ] Add basic asset handles and a simple content-loading story that does not force a large asset system up front.
- [ ] Add stable asset lifetime / reload semantics suitable for real game projects.
- [ ] Add runtime asset streaming that starts simple but already has the right stages: request, read, decode/transcode, upload plan, GPU upload, residency, ready/degraded/failed.
- [ ] Add a texture streaming path that can show a low mip or placeholder immediately, then refine as higher mips arrive.
- [ ] Add asset prefetch hints for scene transitions, upcoming level cells, UI screens, and known gameplay events.
- [ ] Add asset hot reload that uses the same runtime asset handle/state system as release asset streaming.
- [ ] Add runtime Slang shader compilation from loose files, packed assets, memory blobs, and `include_str!` embedded source.
- [ ] Add shader worker jobs so Slang compilation/reflection does not block input, simulation, or normal draw-call collection.
- [ ] Add a last-known-good shader/pipeline fallback for hot reload and runtime shader compile failures.
- [ ] Add examples for a small 2D game, a small 3D game, a streamed-texture scene, and an embedded-`include_str!` shader sample.

### 2D and gameplay-oriented rendering

- [ ] Add a first-class 2D sprite/batch path.
- [ ] Add tilemap / simple layered-scene helpers if they compose cleanly with the graph model.
- [ ] Add examples for many instanced quads.
- [ ] Add examples for instanced meshes with per-instance color/material parameters.
- [ ] Add examples for animated GPU-updated instance data.
- [ ] Add examples for effect-oriented instancing such as layered glow sprites or particles.
- [ ] Ensure all 2D helpers default to top-left/Y-down screen coordinates unless explicitly placed in world space.

### Scene and motion data

- [ ] Add reflected validation for instance-rate vertex inputs when available.
- [ ] Add a full scene sample that writes motion vectors from material shaders.
- [ ] Add explicit support for separating world passes from camera-locked/screen-locked passes in 3D scenes.
- [ ] Add a sample where reticles, HUD markers, or weapon/viewmodel layers render in separate passes without inheriting world motion blur.
- [ ] Add validation scenes that catch incorrect per-object motion vectors under camera pan, object motion, and mixed camera/object motion.
- [ ] Add motion-vector-aware scene examples that exercise TAA in realistic content.

---

## P4 — Rendering Features That Matter For Real Images

This is the path that supports high-end game visuals, including the ability to convince people the output is real footage when the content is strong enough.

### Temporal and image-quality pipeline

- [ ] Add automatic mip generation for sampled textures where format and usage support it.
- [ ] Add explicit mip graph operations:
  - [ ] Write mip.
  - [ ] Read mip.
  - [ ] Downsample mip N to N+1.
  - [ ] Upsample/composite mip N+1 into N.
  - [ ] Transition selected mip ranges.
- [ ] Add sampler controls for LOD bias, min/max LOD, and mip filter choices in the engine API.
- [ ] Add graph validation for accidental full-resource barriers when a mip/layer range would be enough.
- [ ] Ensure texture-space conventions are explicit: texel rectangles are top-left/Y-down where app-facing, UV orientation is declared per sampling path, and backend flips stay internal.

### Core post-processing and realism stack

- [ ] Implement bloom as a polished reference mip-based effect:
  - [ ] Bright extract.
  - [ ] Downsample chain.
  - [ ] Upsample chain.
  - [ ] Final composite.
- [ ] Add stronger temporal AA examples using real motion vectors, camera jitter, and transparency-heavy scenes.
- [ ] Add a robust post stack architecture that can host exposure, bloom, temporal effects, sharpening, grading, film grain, lens effects, and debug views cleanly.
- [ ] Add transparency-heavy validation scenes so temporal and post effects are tested against composited content instead of opaque-only scenes.
- [ ] Add motion-blur validation scenes that specifically verify:
  - [ ] Camera-local vectors produce stable blur during camera motion.
  - [ ] Moving objects blur correctly relative to camera motion.
  - [ ] Camera-locked overlays and reticles do not blur when the camera moves.
  - [ ] Incorrect vectors are easy to spot with built-in debug views.

### Photoreal game rendering path

- [ ] Add a production-oriented lighting and material roadmap covering:
  - [ ] Physically-based materials and parameter workflows.
  - [ ] Image-based lighting and reflection workflows.
  - [ ] Shadowing suitable for dense realistic scenes.
  - [ ] Atmospheric / volumetric rendering path.
  - [ ] Decal and layered-surface workflows.
  - [ ] High-quality translucency / glass / wet surfaces path.
- [ ] Add a roadmap for dense-scene rendering features needed by realistic games:
  - [ ] Streaming-friendly texture and geometry handling.
  - [ ] Foliage / clutter / debris instancing patterns.
  - [ ] Realistic camera and post controls.
  - [ ] Stable temporal accumulation under noisy/high-frequency content.
- [ ] Build a reference scene specifically aimed at realistic output rather than abstract demos.

---

## P5 — Render Graph Depth, Backend Maturity, And Future Backend Additions

This work matters, but it should serve the product tracks instead of replacing them. Vulkan is the native reference backend. Future backends should plug into standardized contracts after those contracts have been proven by Vulkan.

### Render graph scheduling depth

### Deferred frame finalization and flush-free default

- [ ] Define the normal frame cadence as:
  - [ ] App/game/UI code issues high-level graphics calls that enqueue render intent, resource mutations, shader work, upload requests, and draw/object/mesh submissions.
  - [ ] The engine does not immediately flush command buffers, idle queues, wait on fences, or force GPU visibility from those calls.
  - [ ] After app `render`, the runtime finalizes the frame by draining queued work into a frame build context.
  - [ ] Shader modules and entry points are reflected, validated, and associated with pipeline/bind-layout metadata during frame finalization or an asynchronous preparation stage.
  - [ ] Draw calls, UI geometry, objects, meshes, materials, instances, and compatible state are batched and sorted according to renderer policy.
  - [ ] Uploads, generated resources, render-target writes, mip work, read dependencies, write dependencies, and presentation dependencies are compiled into the render graph.
  - [ ] The graph scheduler orders passes, inserts barriers, chooses queues, groups parallel-ready work, and emits a Vulkan submission plan.
  - [ ] The backend encodes Vulkan command buffers from that plan, submits them, and presents without unnecessary CPU waits.
- [ ] Ensure app-facing graphics APIs never use `flush` as the mechanism for making state visible to later work in the same frame; use graph dependencies, resource states, barriers, and finalization ordering instead.
- [ ] Add `FrameWorkQueue` / `RenderIntentQueue` style internals that collect work from the main thread, input/UI systems, asset uploads, shader compilation/reflection, debug draw, and render-prep workers.
- [ ] Add a `FrameFinalizer` that is the only default place allowed to transform queued app work into graph nodes, batches, backend command buffers, submissions, and present operations.
- [ ] Add a `SubmissionPlan` diagnostic object that records pass order, batch groups, queue submissions, semaphores/fences/timeline values, and any waits the CPU actually performed.
- [ ] Make synchronization explicit and coarse-grained:
  - [ ] Frame-boundary fence wait when the latency/frames-in-flight policy requires it.
  - [ ] Swapchain acquire/present waits required by the platform/backend.
  - [ ] Explicit app-requested flush/readback/screenshot/interoperability waits.
  - [ ] Shutdown, resize, device-loss, and hard recovery drains.
- [ ] Add `ExplicitFlushReason` with variants such as `Readback`, `Screenshot`, `BlockingUpload`, `ExternalInterop`, `DebugCapture`, `TestHarness`, `Shutdown`, and `UserRequested`.
- [ ] Make `flush(reason)` return a `FlushReport` containing queued-work counts, submitted command buffers, waited fences/timeline values, elapsed CPU wait time, and warnings when the flush defeats the selected latency policy.
- [ ] Add debug assertions or diagnostics for accidental flushes inside common draw/image/shader APIs.
- [ ] Keep immediate-mode convenience APIs as enqueue-only facades over the same frame work queue so beginner-friendly drawing still uses the production submission path.
- [ ] Add tests that issue multiple graphics calls and prove no backend submit/wait happens before frame finalization unless an explicit flush reason is supplied.
- [ ] Add tests that prove shader reflection, batching, graph ordering, Vulkan command encoding, submit, and present happen in the expected finalization order.


- [ ] Detect graph operations that can run in parallel because their image subresources, buffer ranges, queues, and pipeline resources do not conflict.
- [ ] Compile parallel-ready passes into record batches grouped by queue and dependency level.
- [ ] Define how graphics, compute, and transfer queues synchronize when graph passes cross queue families.
- [ ] Add queue-family and timeline/semaphore diagnostics specific to Vulkan first.
- [ ] Keep the public graph model independent from Vulkan handles, layouts, and barriers.

### Procedural and generated resource breadth

- [ ] Make generated textures compatible with uploaded texture usage sites where format/usage allows it.
- [ ] Add testbed examples for:
  - [ ] Static procedural checker/noise texture.
  - [ ] Animated procedural texture.
  - [ ] GPU-generated texture feeding a later graph pass.

### Runtime texture and asset streaming pipeline

- [ ] Build a `ContentRuntime` that owns asset requests, asset handles, background I/O, decode/transcode workers, upload plans, residency state, and diagnostics.
- [ ] Keep asset handles stable across load, reload, failure, eviction, and revalidation.
- [ ] Define `AssetHandle<T>` state queries:
  - [ ] `is_requested()`.
  - [ ] `is_loading()`.
  - [ ] `is_ready()`.
  - [ ] `is_degraded()`.
  - [ ] `failed_reason()`.
  - [ ] `resident_mips()`.
  - [ ] `debug_name()`.
- [ ] Add a texture streaming state machine:
  - [ ] Missing texture placeholder.
  - [ ] Tiny fallback mip.
  - [ ] Streamed base/lowest practical mip.
  - [ ] Progressive high-mip refinement.
  - [ ] Fully resident.
  - [ ] Budget eviction back down to lower mips.
- [ ] Add per-frame upload budgeting with knobs for bytes/frame, images/frame, staging memory budget, transfer queue time budget, and emergency load-screen mode.
- [ ] Add staging buffer/ring allocator support for async uploads without per-upload allocation churn.
- [ ] Add transfer command planning that feeds the same deferred graph finalization path as draw calls and generated resources.
- [ ] Add CPU decode and GPU upload queues that can operate ahead of the visible frame while respecting frames-in-flight resource lifetime.
- [ ] Add content priority and cancellation:
  - [ ] Visible-now assets.
  - [ ] Near-future prefetch.
  - [ ] UI-critical assets.
  - [ ] Editor-preview assets.
  - [ ] Low-priority background assets.
  - [ ] Cancelled/stale requests.
- [ ] Add asset package/index format planning with stable asset IDs, content hashes, dependency lists, compression metadata, mip ranges, and shader include graphs.
- [ ] Add development loose-file mode and release package mode behind the same virtual asset paths.
- [ ] Add compressed texture policy: prefer GPU-native block-compressed runtime formats when supported, transcode/decode in workers when not, and provide visible fallback diagnostics.
- [ ] Add image orientation/coordinate tests so streamed textures, uploaded textures, generated textures, screenshots, and UI images agree with the top-left/Y-down app-facing coordinate contract.
- [ ] Add Vulkan sparse/tiled residency as a later optional tier for very large textures or virtual-texturing experiments, not as the required first streaming implementation.
- [ ] Add browser/WebGPU constraints to the content API now: no native file assumptions, async fetch-friendly asset sources, and no Vulkan-specific handles in public asset APIs.

### Runtime shader compiler and pipeline cache

- [ ] Add `ShaderCompilerService` as an engine subsystem with worker-thread compilation, reflection, cache lookup, and diagnostics.
- [ ] Add `ShaderSource` variants:
  - [ ] `FilePath`.
  - [ ] `VirtualAssetPath`.
  - [ ] `MemoryBytes`.
  - [ ] `MemoryUtf8`.
  - [ ] `EmbeddedStr` for `include_str!`.
  - [ ] `EmbeddedBytes` for precompiled/cache blobs.
- [ ] Add `ShaderCompileRequest` with module name, virtual path, source, entry points, stage/pipeline kind, target backend, profile, defines, specialization constants, debug level, optimization level, and capability requirements.
- [ ] Compile Slang through its in-process API and make external process invocation a developer-tool fallback only, not a runtime dependency.
- [ ] For Vulkan, emit SPIR-V and preserve enough reflection metadata to build descriptor layouts, push constants, vertex input declarations, resource states, and graph pass resource declarations.
- [ ] Add `CompiledShaderArtifact` containing SPIR-V bytes, reflection info, diagnostics, compiler version, source hash, dependency/include hash, target key, and cache key.
- [ ] Add pipeline cache integration so shader compile output, reflected layouts, pipeline layouts, and Vulkan pipeline objects can be reused safely when the source/capability key matches.
- [ ] Add hot reload transaction behavior:
  - [ ] Compile new shader on worker thread.
  - [ ] Reflect and validate bindings/resources.
  - [ ] Create or queue pipeline rebuild.
  - [ ] Swap to the new pipeline only at a safe graph boundary.
  - [ ] Keep last-known-good on failure.
  - [ ] Emit readable diagnostics without crashing the app.
- [ ] Add release distribution modes:
  - [ ] Source-shipped: game ships Slang source and the bundled compiler compiles/caches on target machines.
  - [ ] Cache-shipped: game ships engine-produced compiled artifacts and validates them before use.
  - [ ] Hybrid: game ships both source and cache, using cache first and source as fallback/rebuild path.
- [ ] Add packaging validation that confirms required Slang runtime/compiler libraries are present in the game bundle for each target platform.
- [ ] Add security/sandbox rules for shader includes: virtual paths only by default, no arbitrary filesystem escape from packaged shader imports unless the app enables dev mode.
- [ ] Add tests for embedded source compilation, package source compilation, source include resolution, cache invalidation, last-known-good fallback, and diagnostics formatting.

### Effect asset model

- [ ] Define a small effect asset format that can reference:
  - [ ] Slang shader files, embedded Slang source, packed shader assets, cached shader artifacts, and entry points.
  - [ ] Reflected pass parameters.
  - [ ] Graph resource declarations.
  - [ ] Procedural texture recipes.
  - [ ] Mip policies.
  - [ ] Instancing inputs.
- [ ] Add stable debug names for all generated resources and passes.
- [ ] Keep the Rust API and asset format backed by the same engine primitives.
- [ ] Allow effect assets to be created from Rust code using embedded `include_str!` Slang source without changing the effect/pipeline/runtime API.



### Multi-surface Vulkan presentation

- [ ] Refactor Vulkan surface/swapchain ownership so each native window has its own surface and swapchain state.
- [ ] Keep the Vulkan instance, device, allocator, descriptor infrastructure, pipeline cache, shader compiler, asset streamer, and global render resources shared across windows.
- [ ] Query and store surface capabilities per window/surface instead of assuming one global presentation configuration.
- [ ] Do not assume every surface can use the exact same present mode, format, extent, transform, alpha mode, or present queue without validation.
- [ ] Make every swapchain image acquire/present path independently synchronized while still participating in the global frame scheduler.
- [ ] Allow command recording for different windows to run in parallel where the Vulkan backend supports it.
- [ ] Make minimized/occluded/zero-size windows suspend acquire/present without blocking other windows.
- [ ] Add per-window swapchain recreation and surface-lost recovery.
- [ ] Present all ready windows at frame finalization without introducing CPU waits for unrelated windows.
- [ ] Add debug names and diagnostics for every window surface, swapchain, swapchain image, acquire semaphore, render-finished semaphore, and present operation.

### Vulkan backend maturity

- [ ] Make Vulkan submission default to deferred frame finalization instead of per-call flush/submit behavior.
- [ ] Track Vulkan command buffers, fences, semaphores, timeline values, and present operations through the `SubmissionPlan` / `FlushReport` diagnostics.
- [ ] Validate that allocator uploads, image transitions, descriptor updates, and pipeline creation do not force queue idle or fence waits during normal draw/image/shader calls.
- [ ] Add Vulkan upload planning for streamed assets: staging rings, copy commands, layout transitions, queue ownership, timeline/semaphore sync, and frame-budgeted submission.
- [ ] Add Vulkan SPIR-V module creation from runtime Slang compiler artifacts without requiring `slangc` on the user machine.
- [ ] Add Vulkan pipeline cache and descriptor-layout cache integration keyed by Slang reflection and engine capability settings.

- [ ] Ensure image usage flags, format capabilities, and sampler capabilities are checked before enabling procedural, mip, and storage-image paths.
- [ ] Add Vulkan support for selected mip/layer image views where needed.
- [ ] Add Vulkan support for copy/blit/compute mip generation paths.
- [ ] Add Vulkan support for reflected bind group updates that can handle generated images, sampled images, samplers, storage images, uniform buffers, storage buffers, and push constants.
- [ ] Add Vulkan frame timing, timestamp query, queue wait, present wait, and frames-in-flight diagnostics.
- [ ] Add Vulkan resource lifetime validation for frame-delayed destruction, swapchain recreation, and resize churn.
- [ ] Add Vulkan-specific backend tests for coordinate-space conversions, viewport/scissor behavior, texture origin handling, and screenshot/readback orientation.

### Backend abstraction seams

- [ ] Define backend capability traits only after the Vulkan implementation proves the required operations.
- [ ] Keep backend abstraction data-oriented: capabilities, formats, queues, resource usage, presentation, synchronization, shader reflection, and command submission.
- [ ] Avoid exposing backend-specific handles through public app/UI/game APIs except behind explicit expert escape hatches.
- [ ] Add backend conformance tests that any future backend must pass:
  - [ ] Coordinate orientation.
  - [ ] Render target read/write.
  - [ ] Texture sampling and readback.
  - [ ] Present/surface behavior.
  - [ ] Shader parameter binding.
  - [ ] Debug labels and diagnostics.
  - [ ] Runtime setting apply paths.
- [ ] Keep native backend expansion separate from browser WebGPU support.

### Browser WebGPU target

- [ ] Treat WebGPU as a browser/WebAssembly backend.
- [ ] Add a browser runtime shell plan that accepts browser constraints around event loop ownership, canvas sizing, input capture, fullscreen, pointer lock, and async device acquisition.
- [ ] Map engine top-left/Y-down app coordinates to canvas coordinates without changing app/UI code.
- [ ] Use `SingleGraphicsOwner + ParallelPreparationOnly` as the conservative browser WebGPU threading model.
- [ ] Add capability downgrade reporting for browser limits, missing formats, missing timestamp queries, restricted threading, storage texture differences, and presentation constraints.
- [ ] Add WebGPU conformance scenes only after the Vulkan coordinate, graph, input, and runtime contracts are stable.

### Live device and backend migration

- [ ] Add runtime GPU switching within the Vulkan backend.
- [ ] Add logical resource migration/rebuild support across device switching.
- [ ] Add a policy for preserving runtime settings, surface state, and app-facing resources across device migration.
- [ ] Add validation and diagnostics for runtime feature downgrades when the new device cannot match the old configuration.
- [ ] Defer live native backend switching until at least two native backends exist and the Vulkan path is mature.
- [ ] Reject WebGPU selection on non-browser targets with a clear diagnostic instead of pretending it is a native backend.

---

## Reference Milestones

### Milestone A — Worth Using For Quick Work

- [ ] Open a window and produce a useful plot or debug view in a short, low-boilerplate sample.
- [ ] Open a shader playground with hot reload, runtime Slang compilation, embedded `include_str!` shader support, and built-in diagnostics.
- [ ] Toggle HDR, AA, transparency, present policy, and debug views at runtime without app restart.
- [ ] Prove top-left/Y-down coordinates through a visible corner/cursor validation sample.

### Milestone B — Worth Using For App UI

- [ ] Build a multi-panel tool app with editable text, scrolling, focus, and first-party widgets.
- [ ] Enable blur/transparency/material effects for the app window at runtime.
- [ ] Style parts of that UI with custom shaders and offscreen composition.
- [ ] Prove UI hit testing, clipping, render target sampling, and screenshots agree on coordinate orientation.



### Milestone B2 — Worth Using For Productivity Workspaces

- [ ] One process can create and own multiple native windows.
- [ ] Each window can render UI, world content, texture previews, or diagnostics through normal engine APIs.
- [ ] Panels can split, tab, detach into another native window, and merge back without losing state.
- [ ] Window creation, destruction, resize, minimize, restore, DPI changes, and surface recreation are robust and non-panicking.
- [ ] A sample productivity app restores a multi-window dock layout across runs.

### Milestone C — Worth Using For Games

- [ ] Build one small 2D game and one small 3D game without rebuilding the runtime shell.
- [ ] Switch graphics settings, post, HDR, latency mode, frame pacing, and presentation policy live during gameplay.
- [ ] Use threaded input with keyboard, mouse, and gamepad in the default game shell.
- [ ] Build one realistic reference scene that stresses motion vectors, temporal stability, post, dense content, and translucent surfaces.

### Milestone D — Worth Taking Seriously For High-End Rendering

- [ ] Build a reference scene where the engine’s output is evaluated against the goal of plausibly real-looking realtime footage.
- [ ] Use that scene to drive the next realism-focused rendering priorities instead of guessing from architecture alone.
- [ ] Demonstrate runtime asset streaming with progressively refined textures and frame-budgeted Vulkan uploads.
- [ ] Demonstrate Vulkan parallel render preparation/recording where it improves real frame time without damaging latency.

### Milestone E — Worth Porting To Browser

- [ ] Run a constrained browser/WebGPU sample that uses the same app-facing coordinate, runtime, input snapshot, and render graph contracts as Vulkan.
- [ ] Show clear browser-specific downgrade diagnostics rather than hiding missing capabilities.

---

## UI / Event System Remaining Work

### Event loop

- [ ] Move event-loop policy under the runtime shell so app/game code does not own redraw scheduling directly.
- [ ] Route winit keyboard, mouse, text, focus, window, scale-factor, and resize events into normalized timestamped engine events.
- [ ] Preserve capture/target/bubble UI semantics while allowing raw input events to be forwarded to the threaded input worker immediately.
- [ ] Add event-loop diagnostics for event age, redraw scheduling, frame pacing, and coalesced pointer movement.

### Input system

- [ ] Add gamepad abstraction.
- [ ] Add threaded input worker and snapshot publishing.
- [ ] Add UI-consumption feedback from frame/UI processing back into action dispatch.
- [ ] Add pointer-lock and relative mouse-motion plan for games.
- [ ] Add input capture policies for modal UI, text input, gameplay, debug console, and browser canvas focus.
- [ ] Add accessibility-aware input metadata where practical without blocking core keyboard/mouse/gamepad behavior.

### UI rendering integration

- [ ] Add backdrop effects: blur, tint, distortion.
- [ ] Add depth-aware UI effects.
- [ ] Add offscreen UI compositing.
- [ ] Ensure all UI rendering integration paths use the standardized coordinate and rectangle contract.

### Advanced UI

- [ ] Add code editor widget.
- [ ] Add virtualized code editor.
- [ ] Add scroll inertia/momentum physics.
- [ ] Add full accessibility support.
- [ ] Add `ScreenUiRoot`, `WorldUiRoot`, and `TextureUiRoot` as explicit UI entry points.
- [ ] Add in-world UI ray hit testing, focus/capture, render-to-texture, mip generation, and material binding.
- [ ] Add pause-menu/HUD, in-world terminal, and standalone app reference examples.
- [ ] Add UI root diagnostics for coordinate space, scale factor, render target, color space, input capture, focus scope, and pass count.
- [ ] Add accessibility tree generation for standalone app UI and reusable metadata for game/UI narration.



### Multi-window and workspace

- [ ] Replace single-window assumptions in app shell, input routing, render frame creation, surface ownership, and debug overlays.
- [ ] Add `WindowRegistry`, `WindowHandle`, `WindowDesc`, `WindowContext`, `WindowFrame`, and `FrameSet`.
- [ ] Add event-loop command routing for create/destroy/reconfigure window operations.
- [ ] Add per-window surface/swapchain state and per-window present diagnostics.
- [ ] Add docking workspace model with split, tab, detach, merge, and persistence.
- [ ] Add cross-window drag/drop and command routing.
- [ ] Add native multi-window sample and stress tests.
