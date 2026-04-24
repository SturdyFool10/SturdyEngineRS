# Sturdy Engine Roadmap

## Product Direction

Sturdy Engine should be worth using in three modes:

1. Quick visualization and shader play
2. Graphical apps and custom UI
3. Full games, including a path toward footage that can plausibly read as real life

The simple path must be the best path, not a toy path. A small app should be
able to open a window, draw something useful, inspect it, and change major
runtime settings without rebuilding its shell or restarting.

## Priority Rules

When choosing what to do next, prefer work that improves:

1. the first 30 minutes of using the engine
2. the amount of application boilerplate removed
3. the quality of the default app/runtime shell
4. the ability to scale from simple usage into deeper control without rewrites
5. the path toward high-end game visuals and stable iteration

Deprioritize work that is technically sophisticated but does not materially move
one of the product tracks above.

## Runtime Rules

These are architectural constraints, not stretch goals:

- [ ] Treat “requires restart” as a failure case unless the operating system or driver makes it impossible
- [ ] Separate settings into:
  - [ ] immediate apply
  - [ ] graph/pipeline rebuild
  - [ ] surface/window recreation
  - [ ] live device migration
- [ ] Keep one public runtime settings model that explains which path each setting takes
- [ ] Make simple runtime settings use the same internal systems as advanced apps instead of separate code paths
- [ ] Expose capability queries and failure reasons when a requested runtime change cannot be applied exactly

## Motion And Multipass Rules

These are product rules for temporal effects and 3D composition quality:

- [ ] Treat camera-local motion vectors as the default contract for post-processing inputs
- [ ] Make it easy to render camera-locked or screen-locked elements in separate passes so they do not inherit scene motion blur or other temporal artifacts
- [ ] Treat multipass 3D composition as a first-class engine path, not an awkward escape hatch
- [ ] Require explicit motion-vector correctness validation for moving cameras, moving objects, animated materials, and camera-locked overlays
- [ ] Treat incorrect motion vectors as a high-severity rendering bug because they directly damage TAA, motion blur, and temporal stability

## Direction To Roadmap Mapping

The runtime direction document is not a separate strategy. It explains how the
current roadmap should be interpreted and prioritized.

### What the current testbed proves

Right now the testbed is still doing engine work in app code:

- [ ] manually assembling the common frame pipeline
- [ ] manually owning debug controls for AA, bloom, HDR, tone mapping, and debug images
- [ ] manually managing text atlas uploads and HUD draw plumbing
- [ ] manually handling HDR surface policy and surface recreation

Those findings map directly to `P0`. Until they are first-party engine systems,
the engine is still asking application authors to build the runtime shell
themselves.

### Product rule that drives implementation

- [ ] Keep the built-in app shell on the same renderer/runtime systems as the advanced path
- [ ] Keep the debug overlay on the same runtime settings, diagnostics, and graph resources as normal apps
- [ ] Make “more control” reveal deeper layers of the same stack instead of replacing the stack

### Public runtime model this roadmap is aiming at

The `P0` runtime settings work should converge on one first-party runtime
controller with:

- [ ] settings snapshot/query
- [ ] transaction-style setting updates
- [ ] diagnostics/query surface
- [ ] per-setting apply results that distinguish exact apply, degraded apply, and rejection

### Apply-path mapping

Every runtime-facing setting should be classified before implementation work is
considered complete:

- [ ] `Immediate`: CPU-state or binding changes that apply next frame
- [ ] `GraphRebuild`: pass, pipeline, or graph-topology rebuilds without replacing the surface
- [ ] `SurfaceRecreate`: presentation/swapchain recreation while keeping the app alive
- [ ] `WindowReconfigure`: native window/compositor/background-effect changes
- [ ] `DeviceMigration`: live adapter/backend switching without app restart

### Phase mapping

- [ ] `P0` turns the testbed shell into `AppRuntime`, `DebugShell`, `TextOverlay`, and a real runtime controller
- [ ] `P1` proves that the shell is useful immediately for debug draw, datavis, and shader playground work
- [ ] `P2` proves that the same engine path can support real tool/app UI instead of forcing bypasses
- [ ] `P3` proves that games can reuse the same runtime/debug/settings stack instead of rebuilding it
- [ ] `P4` uses that stable runtime foundation to push image quality and realistic rendering
- [ ] `P5` deepens the graph/backend architecture only where it materially supports the product tracks above

---

## P0 — App Shell And Runtime Reconfiguration

Until this exists, the engine still asks app authors to assemble too much of the
runtime by hand.

### Prompt-sized execution order

These are intended to be small enough to finish in one focused implementation
prompt. Each chunk should end with code, tests or validation, and roadmap
checkbox updates.

- [x] `P0.1` Define the first public `AppRuntime` surface and create the minimal type/module skeleton without changing behavior yet
- [x] `P0.2` Move swapchain acquire/present ownership behind `AppRuntime` while keeping the existing testbed render path working
- [x] `P0.3` Move default HDR scene target allocation/selection behind `AppRuntime`
- [x] `P0.4` Move MSAA target allocation and resolve behind `AppRuntime`
- [x] `P0.5` Move bloom, AA, and tonemap chain assembly behind `AppRuntime`
- [x] `P0.6` Add a named debug image registry owned by the runtime instead of the testbed
- [x] `P0.7` Add a runtime diagnostics data model for backend, adapter, HDR, present mode, AA, bloom state, and graph timings
- [x] `P0.8` Surface the diagnostics model through a basic first-party overlay hook, even if presentation is still minimal
- [ ] `P0.9` Internalize motion-vector generation and debug display registration as runtime features rather than testbed-only plumbing
- [x] `P0.9a` Define the runtime motion-vector contract around camera-local motion suitable for TAA and motion blur
- [x] `P0.9b` Add first-party support for camera-locked/screen-locked overlay passes that bypass scene motion blur and temporal accumulation
- [x] `P0.9c` Add a motion-vector validation/debug mode that makes incorrect object or camera motion obvious in-engine
- [x] `P0.10` Define the first `TextOverlay` API surface that lets apps request text without touching atlas/page management
- [x] `P0.11` Move the existing HUD text path behind `TextOverlay` while preserving current output
- [x] `P0.12` Add a first-pass debug action/input binding registry above raw key handling
- [x] `P0.13` Define `RuntimeSettingsSnapshot`, setting keys, and a single public settings model
- [x] `P0.14` Classify every existing runtime-facing setting into `Immediate`, `GraphRebuild`, `SurfaceRecreate`, `WindowReconfigure`, or `DeviceMigration`
- [x] `P0.15` Implement the first `Immediate` runtime settings path for low-risk settings like overlay visibility or post-process dials
- [x] `P0.16` Implement the first `GraphRebuild` runtime settings path for AA mode or post-chain topology changes
- [x] `P0.17` Implement the first `SurfaceRecreate` runtime settings path for HDR mode or present mode changes without app restart
- [x] `P0.18` Add transaction-style runtime settings updates so multiple changes apply coherently in one call
- [x] `P0.19` Add `RuntimeApplyReport` / per-setting apply results with exact, degraded, and rejected outcomes
- [x] `P0.20` Expose capability queries and failure reasons for unsupported runtime-setting requests
- [x] `P0.21` Add runtime shell support for transparent window/background configuration toggles
- [x] `P0.22` Add transparent surface clear/present handling where the backend/platform supports it
- [x] `P0.23` Define the engine-level window background effect/material abstraction with both presets and explicit descriptors
- [x] `P0.24` Implement the first Windows backdrop/material integration through that abstraction
- [x] `P0.25` Implement the first macOS material/vibrancy integration through the same abstraction
- [x] `P0.26` Implement the first Linux background-effect adapter with graceful fallback behavior
- [x] `P0.27` Make transparency/background effects runtime-toggleable without restart through the runtime settings controller
- [ ] `P0.28` Add Slang shader hot reload with clear in-app compile error reporting
- [ ] `P0.29` Add first-pass asset hot reload for common asset types used by the testbed
- [ ] `P0.30` Add stable missing/stale-asset diagnostics surfaced in the runtime overlay or logs
- [ ] `P0.31` Add screenshot/export helpers to the first-party runtime shell
- [ ] `P0.32` Add image inspection for named graph resources using the runtime-owned debug image registry
- [ ] `P0.33` Add a first-pass frame-graph inspection UI or textual inspection surface
- [ ] `P0.34` Add GPU timing and per-pass timing summaries to runtime diagnostics
- [ ] `P0.35` Convert one existing sample/testbed path to the first-party runtime shell and remove the equivalent app-side boilerplate

### Chunking rules

If a roadmap item is still too large for one prompt, split it again until the
prompt can do all of the following in one pass:

- [ ] define or narrow one public API seam
- [ ] move one existing responsibility behind that seam
- [ ] keep one existing sample or testbed path working
- [ ] verify behavior with a build, test, or sample run
- [ ] update the relevant roadmap checkboxes

### Recommended first prompt sequence

If work starts immediately, do these in order before jumping ahead:

1. `P0.1`
2. `P0.2`
3. `P0.3`
4. `P0.4`
5. `P0.6`
6. `P0.7`
7. `P0.13`
8. `P0.14`

### Internalize current testbed boilerplate

- [ ] Add a first-party `AppRuntime` / `AppRenderer` that owns the common frame loop:
  - [ ] swapchain acquire / present
  - [ ] HDR/SDR output policy
  - [ ] default HDR scene target
  - [ ] MSAA target and resolve
  - [ ] bloom / AA / tonemap chain
  - [ ] named debug image outputs
  - [ ] diagnostics overlay hook
- [ ] Internalize motion-vector generation/debug display as engine features instead of testbed-only plumbing
  - [ ] Define the engine motion-vector input contract in terms of camera-local motion
  - [ ] Distinguish world-scene motion vectors from camera-locked/screen-locked passes
  - [ ] Keep debug display and post-processing consumers on the same motion-vector interpretation
- [ ] Internalize the current HUD text path into a first-party text/debug overlay instead of making apps manage atlas pages, uploads, and quad meshes
- [ ] Add a first-party debug action registry and input binding layer above raw key events
- [ ] Internalize standard renderer diagnostics:
  - [ ] adapter/backend display
  - [ ] HDR mode
  - [ ] present mode
  - [ ] AA mode and actual sample count
  - [ ] bloom state
  - [ ] graph timings
  - [ ] debug image selection

### Runtime settings and no-restart architecture

- [ ] Add a unified runtime settings system for:
  - [ ] backend selection
  - [ ] adapter/GPU selection
  - [ ] HDR mode
  - [ ] present mode
  - [ ] surface transparency
  - [ ] window background effect/material
  - [ ] antialiasing mode and dials
  - [ ] post-processing toggles and dials
  - [ ] shader hot reload and asset hot reload policy
- [x] Let applications register their own runtime settings alongside engine-owned settings
- [x] Make setting metadata and current values queryable anywhere through a shared runtime controller
- [x] Add a polled settings change stream so systems like asset loading can react immediately to app-defined settings such as texture resolution tiers
- [ ] Apply runtime settings changes through the right internal path automatically:
  - [x] patch state in place when possible
  - [x] rebuild graph/pipelines when needed
  - [x] recreate the surface/window when needed
  - [ ] migrate live resources across devices/backends when needed
- [ ] Add a transaction-style runtime reconfiguration path so multiple setting changes can be applied coherently in one step
- [ ] Add explicit notifications for:
  - [ ] setting accepted as requested
  - [ ] setting clamped or degraded
  - [ ] setting rejected with reason
- [x] Prove one app-defined runtime setting can trigger immediate asset swaps through the shared controller path

### Window transparency and compositor effects

- [ ] Add window/background transparency support in the application shell
- [ ] Add surface alpha / transparent clear path support so rendered content can preserve transparency through presentation where the platform allows it
- [ ] Add engine-level window background effect/material support with both:
  - [x] easy presets
  - [x] explicit low-level control
- [ ] Support Windows material/effect families such as blur-behind, acrylic, mica, and tabbed/titlebar variants through one engine API
- [ ] Support macOS vibrancy/material integration through the same engine API
- [ ] Support Linux window background effects through a Linux platform adapter with:
  - [ ] Wayland `ext-background-effect-v1` as the primary background-effect protocol
  - [ ] compatibility fallbacks for older compositor-specific blur protocols only where needed
  - [ ] graceful fallback when no compositor effect protocol is available
- [ ] Let apps toggle transparency and blur/material effects at runtime without restart
- [ ] Let apps specify whether the window background effect applies to the whole window or engine-managed regions

### First-run usability

- [ ] Add shader hot reload for Slang shaders with clear in-app compile errors
- [ ] Add asset hot reload for textures, meshes, and other common inputs
- [ ] Add stable debug/error reporting for missing or stale assets
- [ ] Add screenshot/export helpers
- [ ] Add frame graph visualization / inspection UI
- [ ] Add image inspection for named graph resources
- [ ] Add GPU timing and pass timing summaries

---

## P1 — Immediate Utility: Debug Draw, Datavis, And Playground

### Debug draw and inspection

- [ ] Add immediate-style 2D drawing helpers for:
  - [ ] lines
  - [ ] polylines
  - [ ] rectangles
  - [ ] circles
  - [ ] points / markers
  - [ ] filled shapes
- [ ] Support layering those primitives with text in the same frame
- [ ] Expose sane defaults for antialiasing, thickness, color, transforms, and hit testing
- [ ] Add a built-in debug view picker for motion vectors, bloom chain, AA history, and other named graph images

### Data visualization first mile

- [ ] Add a lightweight plotting layer for:
  - [ ] axes
  - [ ] linear/log scales
  - [ ] gridlines
  - [ ] line series
  - [ ] scatter series
  - [ ] bar series
  - [ ] legends
  - [ ] value tooltips / nearest-point inspection
- [ ] Add pan/zoom helpers for 2D data views
- [ ] Add a simple palette/theme model for quick visualization work
- [ ] Add a “load data, plot it, inspect it” sample with minimal code

### Shader playground usability

- [ ] Add a shader-playground app shell with:
  - [ ] standard time/frame/resolution uniforms
  - [ ] quick texture binding helpers
  - [ ] pause / step / scrub controls
  - [ ] built-in debug overlays
  - [ ] one-click debug image switching
- [ ] Make the default playground path use the same runtime shell as normal apps

---

## P2 — App UI Must Feel Complete

The Clay/UI foundation is promising, but the app-facing shell is still missing
core behavior that makes people stay on the engine path instead of bypassing it.

### Text, input, and editing

- [ ] Replace placeholder text-size estimation in layout cache with true `textui`/font-system measurement for layout-time wrapping parity
- [ ] Add editable text fields with cursor, selection, and clipboard support
- [ ] Add IME composition support for desktop text entry
- [ ] Add focus management and keyboard navigation parity
- [ ] Add a first-party text overlay/panel stack on top of `textui` so apps do not manage atlas images directly

### Widget layer

- [ ] Add a standard widget layer for:
  - [ ] labels
  - [ ] buttons
  - [ ] checkboxes / toggles
  - [ ] sliders
  - [ ] text inputs
  - [ ] lists
  - [ ] tables
  - [ ] trees / inspectors
- [ ] Keep widgets composable with shader-driven visuals instead of forcing a theme-only path
- [ ] Add first-party inspector/panel patterns for graphics tools and game tools

### Layout and interaction behavior

- [ ] Implement arena/reuse strategy equivalent to Clay’s low-allocation hot path
- [ ] Add Clay-level scroll physics/momentum and external scroll offset query parity
- [ ] Add floating/attach-point semantics parity (`attach_to_parent`, `attach_to_id`, clip inheritance, pointer passthrough modes)
- [ ] Implement child-between-border emission parity and exact border raster semantics
- [ ] Add virtualized scrolling / large-list support
- [ ] Add robust app-facing event/state plumbing so users do not need large glue layers

### UI rendering completeness

- [ ] Add full render-graph resource binding generation (bind groups, push constants, per-pass parameter buffers) instead of pass skeletons
- [ ] Integrate real shader pipelines for all UI slots in engine runtime
- [ ] Add GPU gradient shader implementation and parameter packing contract used by engine pipelines
- [ ] Add text outline rendering path in engine shader side
- [ ] Add offscreen-to-world UI sample path in `sturdy-engine` scene layer
- [ ] Add UI search/indexing layer on top of font discovery for later UI-wide queries
- [ ] Add examples for dashboards, inspectors, and multi-panel tools

---

## P3 — Game Development Path

This track must make the engine viable for full games without forcing every
project to build a custom runtime shell first.

### Frame, time, input, and runtime

- [ ] Add fixed-timestep and interpolation helpers
- [ ] Add input action/binding helpers above raw key events
- [ ] Add simple camera controllers for common 2D and 3D cases
- [ ] Add a default game runtime shell that reuses the same renderer/debug/runtime-settings systems as apps and tools

### Content and iteration loop

- [ ] Add basic asset handles and a simple content-loading story that does not force a large asset system up front
- [ ] Add stable asset lifetime / reload semantics suitable for real game projects
- [ ] Add examples for a small 2D game and a small 3D game

### 2D and gameplay-oriented rendering

- [ ] Add a first-class 2D sprite/batch path
- [ ] Add tilemap / simple layered-scene helpers if they compose cleanly with the graph model
- [ ] Add examples for many instanced quads
- [ ] Add examples for instanced meshes with per-instance color/material parameters
- [ ] Add examples for animated GPU-updated instance data
- [ ] Add examples for effect-oriented instancing such as layered glow sprites or particles

### Scene and motion data

- [ ] Add reflected validation for instance-rate vertex inputs when available
- [ ] Add a full scene sample that writes motion vectors from material shaders
- [ ] Add explicit support for separating world passes from camera-locked/screen-locked passes in 3D scenes
- [ ] Add a sample where reticles, HUD markers, or weapon/viewmodel layers render in separate passes without inheriting world motion blur
- [ ] Add validation scenes that catch incorrect per-object motion vectors under camera pan, object motion, and mixed camera/object motion
- [ ] Add motion-vector-aware scene examples that exercise TAA in realistic content

---

## P4 — Rendering Features That Matter For Real Images

This is the path that supports high-end game visuals, including the ability to
convince people the output is real footage when the content is strong enough.

### Temporal and image-quality pipeline

- [ ] Add automatic mip generation for sampled textures where format and usage support it
- [ ] Add explicit mip graph operations:
  - [ ] write mip
  - [ ] read mip
  - [ ] downsample mip N to N+1
  - [ ] upsample/composite mip N+1 into N
  - [ ] transition selected mip ranges
- [ ] Add sampler controls for lod bias, min/max lod, and mip filter choices in the engine API
- [ ] Add graph validation for accidental full-resource barriers when a mip/layer range would be enough

### Core post-processing and realism stack

- [ ] Implement bloom as a polished reference mip-based effect:
  - [ ] bright extract
  - [ ] downsample chain
  - [ ] upsample chain
  - [ ] final composite
- [ ] Add stronger temporal AA examples using real motion vectors, camera jitter, and transparency-heavy scenes
- [ ] Add a robust post stack architecture that can host exposure, bloom, temporal effects, sharpening, grading, film grain, lens effects, and debug views cleanly
- [ ] Add transparency-heavy validation scenes so temporal and post effects are tested against composited content instead of opaque-only scenes
- [ ] Add motion-blur validation scenes that specifically verify:
  - [ ] camera-local vectors produce stable blur during camera motion
  - [ ] moving objects blur correctly relative to camera motion
  - [ ] camera-locked overlays and reticles do not blur when the camera moves
  - [ ] incorrect vectors are easy to spot with built-in debug views

### Photoreal game rendering path

- [ ] Add a production-oriented lighting and material roadmap covering:
  - [ ] physically-based materials and parameter workflows
  - [ ] image-based lighting and reflection workflows
  - [ ] shadowing suitable for dense realistic scenes
  - [ ] atmospheric / volumetric rendering path
  - [ ] decal and layered-surface workflows
  - [ ] high-quality translucency / glass / wet surfaces path
- [ ] Add a roadmap for dense-scene rendering features needed by realistic games:
  - [ ] streaming-friendly texture and geometry handling
  - [ ] foliage / clutter / debris instancing patterns
  - [ ] realistic camera and post controls
  - [ ] stable temporal accumulation under noisy/high-frequency content
- [ ] Build a reference scene specifically aimed at realistic output rather than abstract demos

---

## P5 — Render Graph Depth, Backend Maturity, And Live Migration

This work matters, but it should serve the product tracks instead of replacing them.

### Render graph scheduling depth

- [ ] Detect graph operations that can run in parallel because their image subresources, buffer ranges, queues, and pipeline resources do not conflict
- [ ] Compile parallel-ready passes into record batches grouped by queue and dependency level
- [ ] Define how graphics, compute, and transfer queues synchronize when graph passes cross queue families

### Procedural and generated resource breadth

- [ ] Make generated textures compatible with uploaded texture usage sites where format/usage allows it
- [ ] Add testbed examples for:
  - [ ] static procedural checker/noise texture
  - [ ] animated procedural texture
  - [ ] GPU-generated texture feeding a later graph pass

### Effect asset model

- [ ] Define a small effect asset format that can reference:
  - [ ] Slang shader files and entry points
  - [ ] reflected pass parameters
  - [ ] graph resource declarations
  - [ ] procedural texture recipes
  - [ ] mip policies
  - [ ] instancing inputs
- [ ] Add stable debug names for all generated resources and passes
- [ ] Keep the Rust API and asset format backed by the same engine primitives

### Backend and capability work

- [ ] Ensure image usage flags, format capabilities, and sampler capabilities are checked before enabling procedural, mip, and storage-image paths
- [ ] Add backend support for selected mip/layer image views where needed
- [ ] Add backend support for copy/blit/compute mip generation paths
- [ ] Add backend support for reflected bind group updates that can handle generated images, sampled images, samplers, storage images, uniform buffers, storage buffers, and push constants
- [ ] Keep Vulkan as the reference backend while preserving D3D12 and Metal layout constraints in the public model

### Live backend/device migration

- [ ] Add runtime GPU switching
- [ ] Add live backend switching without application restart
- [ ] Add logical resource migration/rebuild support across runtime switching
- [ ] Add a policy for preserving runtime settings, surface state, and app-facing resources across live migration
- [ ] Add validation and diagnostics for runtime feature downgrades when the new backend/device cannot match the old configuration

---

## Reference Milestones

### Milestone A — Worth Using For Quick Work

- [ ] Open a window and produce a useful plot or debug view in a short, low-boilerplate sample
- [ ] Open a shader playground with hot reload and built-in diagnostics
- [ ] Toggle HDR, AA, transparency, and debug views at runtime without app restart

### Milestone B — Worth Using For App UI

- [ ] Build a multi-panel tool app with editable text, scrolling, focus, and first-party widgets
- [ ] Enable blur/transparency/material effects for the app window at runtime
- [ ] Style parts of that UI with custom shaders and offscreen composition

### Milestone C — Worth Using For Games

- [ ] Build one small 2D game and one small 3D game without rebuilding the runtime shell
- [ ] Switch graphics settings, post, HDR, and presentation policy live during gameplay
- [ ] Build one realistic reference scene that stresses motion vectors, temporal stability, post, dense content, and translucent surfaces

### Milestone D — Worth Taking Seriously For High-End Rendering

- [ ] Build a reference scene where the engine’s output is evaluated against the goal of plausibly real-looking realtime footage
- [ ] Use that scene to drive the next realism-focused rendering priorities instead of guessing from architecture alone
