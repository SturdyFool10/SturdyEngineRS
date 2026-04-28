# Sturdy Engine Roadmap

> TODO-only cleaned version. Completed checkbox items and the duplicate roadmap copy were removed.

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

### Chunking rules

If a roadmap item is still too large for one prompt, split it again until the
prompt can do all of the following in one pass:

- [ ] define or narrow one public API seam
- [ ] move one existing responsibility behind that seam
- [ ] keep one existing sample or testbed path working
- [ ] verify behavior with a build, test, or sample run
- [ ] update the relevant roadmap checkboxes

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
- [ ] Apply runtime settings changes through the right internal path automatically:
  - [ ] migrate live resources across devices/backends when needed
- [ ] Add a transaction-style runtime reconfiguration path so multiple setting changes can be applied coherently in one step
- [ ] Add explicit notifications for:
  - [ ] setting clamped or degraded

### Window transparency and compositor effects

- [ ] Add window/background transparency support in the application shell
- [ ] Add surface alpha / transparent clear path support so rendered content can preserve transparency through presentation where the platform allows it
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

## P2 — App UI Must Feel Complete

The Clay/UI foundation is promising, but the app-facing shell is still missing
core behavior that makes people stay on the engine path instead of bypassing it.

### Text, input, and editing

Prompt-sized text execution order:

- [ ] `P2.T3` Add text quality validation scenes and screenshot/golden coverage for scale factors, HDR/SDR, fallback fonts, and animated UI
- [ ] `P2.T4` Add text performance instrumentation and budgets for shaping, caching, atlas uploads, draw calls, and memory use
- [ ] `P2.T5` Upgrade glyph atlas lifetime, dirty uploads, eviction, tiling, and backend-limit handling for real UI workloads
- [ ] `P2.T6` Integrate engine-side text shaders for alpha mask, SDF/MSDF, outlines, shadows, and linear-light blending
- [ ] `P2.T7` Expose app-facing rich text controls after measurement, rendering, and atlas behavior are stable
- [ ] `P2.T8` Build editable text, IME, selection, clipboard, focus, and keyboard navigation on top of the shared text model

Prompt-sized text follow-up chunks:

- [ ] `P2.T3a` Add a deterministic text validation harness that can render `ui_demo` text scenes at fixed sizes and scale factors without manual window resizing
- [ ] `P2.T3b` Add screenshot/golden coverage for small alpha-mask UI labels, dense tables, code-like text, and large SDF display text over dark and light UI backgrounds
- [ ] `P2.T3c` Add resize, scrolling, clipping, and fractional-position scenes that specifically catch stale clip extents, shimmer, and thickness changes
- [ ] `P2.T3d` Add fallback-script scenes for Latin, CJK, emoji, combining marks, Arabic/Hebrew bidi text, ligatures, and missing-glyph diagnostics
- [ ] `P2.T4a` Add `textui` prepared-scene cache hit/miss counters keyed separately from layout measurement caches
- [ ] `P2.T4b` Add per-frame text timings for shaping, glyph rasterization, atlas snapshotting, tiling, upload recording, and mesh construction
- [ ] `P2.T4c` Add text memory counters for font data, shaped layouts, prepared scenes, atlas pages, cached snapshots, and GPU atlas images
- [ ] `P2.T4d` Add resize-specific text telemetry so cache churn is visible as stable-label hits, wrapped-label remeasurements, atlas uploads, and evictions
- [ ] `P2.T5a` Add stable atlas page handles with frame-delayed destruction so UI text texture identity survives normal window resize and layout churn
- [ ] `P2.T5b` Add dirty-rectangle atlas upload plumbing and fall back to whole-page upload only when the backend or tiling path requires it
- [ ] `P2.T5c` Add atlas occupancy and eviction policy tests for scrolling lists, dense tables, and mixed fallback fonts
- [ ] `P2.T5d` Add backend-limit tests for page sizing, tiling, texture count, sampler selection, and degradation diagnostics
- [ ] `P2.T6a` Split alpha-mask text sampling from SDF/MSDF sampling so exact 1:1 UI text can use the sharpest sampler path
- [ ] `P2.T6b` Validate SDF/MSDF field range, outline, shadow, glow, and opacity behavior against screenshot cases before using MSDF for screen-space UI text by default
- [ ] `P2.T6c` Implement explicit linear-light text blending policy for SDR, HDR, tonemapped, and transparent UI targets
- [ ] `P2.T7a` Define a shared rich-text run model used by measurement, painting, hit testing, accessibility labels, and future editing
- [ ] `P2.T7b` Expose per-span typography, color, OpenType features, underline, strikethrough, background highlight, outline, shadow, glow, alignment, truncation, and wrapping modes
- [ ] `P2.T8a` Build the first single-line editable field using the shared shaped-run model, with cursor movement, selection, focus, clipboard, and keyboard navigation
- [ ] `P2.T8b` Add multiline editing, scrolling, grapheme-aware selection, bidi cursor movement, IME composition, and platform clipboard integration

- [ ] Build text quality validation scenes:
  - [ ] small UI labels, dense tables, code-like text, and large display text
  - [ ] high-contrast, low-contrast, transparent, HDR, and post-tonemapped backgrounds
  - [ ] subpixel positions, scrolling, transforms, clipping, and animated opacity
  - [ ] resize growth/shrink cases that prove text clip extents update with the application viewport
  - [ ] pixel-alignment cases that prove stems do not look thicker, thinner, or smaller as labels move
  - [ ] Latin, CJK, emoji, combining marks, bidirectional text, ligatures, and fallback fonts
- [ ] Add golden-image or screenshot-diff tests for representative text cases across scale factors and output formats
- [ ] Add text performance budgets and instrumentation:
  - [ ] shape/layout cache hit rate for the full `textui` prepared layout and GPU scene caches
  - [ ] glyph atlas page count, occupancy, uploads, and evictions
  - [ ] CPU shaping/layout time
  - [ ] GPU draw calls, sampled pages, and overdraw
  - [ ] memory use for font data, layouts, atlases, and prepared scenes
- [ ] Implement robust glyph atlas management for UI workloads:
  - [ ] persistent pages with stable lifetime and frame-delayed destruction
  - [ ] dirty-region uploads instead of whole-page uploads where supported
  - [ ] eviction policy that avoids visible thrash during scrolling and language fallback
  - [ ] backend-limit-aware page sizing, tiling, and sampler selection
- [ ] Make text layout correct before rendering:
  - [ ] shared measurement path for layout, wrapping, hit testing, clipping, and painting
  - [ ] consistent font fallback between measurement and rendering
  - [ ] Unicode line breaking, grapheme-aware cursor movement, and bidirectional paragraph handling
  - [ ] make application/UI clipping explicit in text draw commands instead of relying only on CPU-side clipped layout bounds
  - [ ] deterministic rounding so layout does not shimmer while scrolling or animating
- [ ] Expose rich text styling needed for real UI:
  - [ ] per-span font family, weight, style, stretch, size, color, features, and variation axes
  - [ ] underline, strikethrough, background highlight, outline, shadow, and glow effects
  - [ ] truncation, ellipsis, wrapping modes, alignment, line height, and letter/word spacing
  - [ ] inline icons/images and baseline-aligned widgets
- [ ] Add editable text fields with cursor, selection, and clipboard support
- [ ] Add IME composition support for desktop text entry
- [ ] Add focus management and keyboard navigation parity
- [ ] Add a first-party text overlay/panel stack on top of `textui` so apps do not manage atlas images directly

### Widget layer

Prompt-sized UI/control execution order:

- [x] `P2.U2` Basic scroll containers: scroll_container_with_scrollbars, parent clipping (clip_x/clip_y), wheel/touchpad input, keyboard scroll, and external scroll offset control are implemented. Inertial/momentum hooks remain.
  - [ ] Remaining: inertial/momentum scroll physics, scroll snapping, sticky children
- [ ] `P2.U6` Add multiline text inputs with wrapping, scrolling, selection, clipboard, IME, undo/redo, soft tabs, line navigation, and shape-aware clipping (text_input widget exists with multiline flag; actual cursor/selection/clipboard logic not yet wired)
- [ ] `P2.U7` Add stylable multiline text editing rich enough for code editors: per-range styling, syntax/highlight spans, gutters, line numbers, diagnostics, inline widgets, minimap hooks, code folding hooks, and performant viewport virtualization
- [ ] `P2.U9` Add date, time, and date-time selector widgets with typed entry, picker popovers, min/max, locale/time-zone formatting hooks, and keyboard-only operation
- [ ] `P2.U11` Add first-class UI layering and stacking contexts so apps can declare what renders behind/in front of what without relying on tree order hacks (UiLayer enum and portal_host/modal_layer exist; full z-ordered stacking context API incomplete)
- [ ] `P2.U12` Add shader/effect style slots for backgrounds, borders, masks, text fills, outlines, shadows, glows, backdrop filters, and per-state transitions, with Slang parameter binding and render-graph pass integration
- [ ] `P2.U13` Add shape-aware rendering, clipping, hit testing, focus rings, shadows, and effect regions so rounded rects, independent corner shapes, squircles, paths, and masks behave consistently across input and paint (CornerSpec and UiShape for hit testing exist; GPU-side shape-aware rendering not yet integrated)
- [ ] `P2.U14` Add fancy default border options: per-side/per-corner styling, inner/outer/center strokes, dashed/dotted/double strokes, gradient borders, image/shader borders, glow/bloom borders, and polished focus/error/selection presets
- [x] `P2.U15` Virtualized variants: virtual_list, virtual_grid, virtual_table, virtual_tree, virtual_log_viewer, virtual_mosaic, virtual_context_menu, virtual_dropdown_menu all implemented. Code editor virtualization remains (P2.U7).
- [ ] `P2.U16` Add render-graph-aware backdrop/effect shaders so UI surfaces can sample named scene images, blur/grade/dim the game behind a pause menu tile, and route effects through explicit graph resources
- [ ] `P2.U17` Add validation scenes for all standard widgets, nested scrolling, modals, shape clipping, shader styles, virtualized controls, render-graph backdrops, and resize/scale-factor behavior

**Foundation widgets (implemented):**
- Interactive controls: button, checkbox, radio, toggle, slider, progress_bar, drag_bar, segmented_control
- Input fields: text_input (visual), number_input, search_box, select
- Navigation: tab_bar, breadcrumbs, accordion_panel
- Containers: card, group_box, dialog_surface, toolbar, status_bar, scroll_container variants, portal_host, modal_layer, tooltip_layer
- Data display: label, badge, chip, notification, empty_state, divider, list_item, property_row, table_header_cell, table_header_row, log_entry, image, icon_button, scrollbar
- Layout helpers: mosaic_container, context_menu_item, command_palette
- `WidgetRenderContext` trait with `Cx<'_>` (deferred registration path), `WidgetPalette`, and `WidgetState` implementations

- [ ] Add remaining standard widgets:
  - [ ] color pickers
  - [ ] date / time / date-time selectors
  - [ ] code/rich-text editor
- [ ] Keep widgets composable with shader-driven visuals instead of forcing a theme-only path
- [ ] Treat every widget as a shape-aware object for focus rings, scroll regions, shadows, and accessibility bounds
- [ ] Add shape primitives and corner controls suitable for high-end UI work:
  - [ ] per-corner smoothing and antialiasing controls
  - [ ] shape composition for cutouts, holes, masks, and decorative corners
- [ ] Add polished built-in border presets:
  - [ ] subtle hairline, focus ring, inset, raised, etched, glass, neon, warning/error, and selected states
  - [ ] per-side and per-corner color/width/style
  - [ ] inside, centered, and outside stroke placement
  - [ ] dashed, dotted, double, gradient, textured, image, and shader-driven borders
  - [ ] optional shadow, glow, bloom extraction, and animated border parameters
- [ ] Add widget APIs for shader/effect customization that feel familiar to web developers:
  - [ ] per-state style rules for hover, active, focus, disabled, invalid, selected, checked, and open
  - [ ] offscreen effect routes for bloom, blur, drop shadow, glow, and custom passes
- [ ] Add full text styling controls at the widget layer
- [ ] Add first-party inspector/panel patterns for graphics tools and game tools

### Layout and interaction behavior

- [ ] Implement arena/reuse strategy equivalent to Clay’s low-allocation hot path
- [x] Add scroll containers with wheel, touchpad, keyboard, drag-scroll, and programmatic scroll input; overlay scrollbars; external scroll offset query
  - [ ] Remaining: scroll snapping, momentum/inertia hooks, sticky children, anchor preservation
- [ ] Add Clay-level scroll physics/momentum
- [x] Add floating/attach-point semantics: `anchored_floating_layer`, `attached_floating_layer`, FloatingOptions, FloatingPlacement, FloatingCollision, clip inheritance
- [x] Add a UI top-layer and portal system: portal_host, modal_layer, tooltip_layer, UiTree multi-root for overlays
  - [ ] Remaining: full dismiss policies (escape mapping, focus loss, nested scope, delayed pointer capture)
- [x] Add parent clipping by rect and scroll viewport (clip_x/clip_y in LayoutInput)
  - [ ] Remaining: rounded-rect clip, arbitrary path/mask, shader-generated alpha mask
- [x] Add virtualized scrolling / large-list support (virtual_list, virtual_grid, virtual_table, virtual_tree, virtual_log_viewer, virtual_mosaic)
  - [ ] Remaining: virtual rich/code editor; scroll anchoring, selection retention across scroll
- [ ] Add explicit stacking contexts and layer slots with full event capture and keyboard navigation loops
- [ ] Implement child-between-border emission parity and exact border raster semantics
- [x] Add robust app-facing event/state plumbing (InputHub, WidgetRenderContext, Cx deferred-registration, UiEventResult consumption flags; no large glue layer needed)

### UI rendering completeness

- [ ] Add full render-graph resource binding generation (bind groups, push constants, per-pass parameter buffers) instead of pass skeletons
  - [ ] bind group creation and reflected binding layout merge for UI shader slots
  - [ ] GPU buffer allocation/upload and binding for per-pass parameter buffers
  - [ ] built-in UI uniform parameter packing for rect, UV, shape, clip, state, time, and DPI scale
- [ ] Integrate real shader pipelines for all UI slots in engine runtime
- [ ] Define the UI shape contract shared by rendering, input, clipping, and effects:
  - [ ] rectangle
  - [ ] rounded rectangle with independent radii
  - [ ] arbitrary path / mask
  - [ ] shader-produced coverage mask
- [ ] Make UI antialiasing analytic and shape-aware for fills, borders, outlines, masks, and clips
- [ ] Add GPU gradient shader implementation and parameter packing contract used by engine pipelines
- [ ] Add text outline rendering path in engine shader side
- [ ] Add a UI material/effect model with slots for:
  - [ ] text fill/outline/shadow/glow
  - [ ] inner shadow
  - [ ] drop shadow
  - [ ] bloom/glow extraction
  - [ ] custom Slang pass hooks
- [ ] Add element-scoped shader overrides:
  - [ ] run a custom fragment shader over that slot's exact shape coverage and clip region
  - [ ] validate merged built-in/user uniform layouts with stable reflection and clear diagnostics
- [ ] Allow per-slot effect routing so a widget can, for example, send only its border to bloom while keeping the fill in the normal UI pass
- [ ] Allow UI shaders to sample render-graph resources through explicit declarations:
  - [ ] previous scene color
  - [ ] HDR scene color
  - [ ] depth
  - [ ] normals / material IDs where available
- [ ] Add backdrop-filter style UI effects that operate on graph images behind the widget:
  - [ ] blur
  - [ ] dim / tint
  - [ ] saturation / contrast / brightness
  - [ ] pixelation
  - [ ] refraction / glass distortion
  - [ ] depth-aware blur where depth is available
- [ ] Add a pause-menu/backdrop sample where a UI tile blurs and tints the live game image beneath it without manual app-side graph wiring
- [ ] Add offscreen UI compositing for effects that require extra passes without forcing every widget into a texture
- [ ] Add render-graph diagnostics for UI pass count, offscreen allocations, clip mask cost, shader slot usage, overdraw, and batched/unbatched draws
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

## UI / Event System Additions

### Event Loop
- [x] Define unified EngineEvent model (InputEvent: Pointer, Scroll, Key, Text, Activate, Focus, Blur, Cancel)
- [x] Implement capture/target/bubble propagation (EventPhase, propagation_path, bubble_path)
- [x] Add EventContext with propagation control (stop_propagation, prevent_default)

### Input System
- [x] Mouse input fully wired: CursorMoved/MouseInput/MouseWheel → InputHub → InputSimulator hit-test → widget states
- [x] Pointer events: hover, press, drag, scroll; captured/released-this-frame phases
- [x] Slider drag tracking with SliderConfig (min/max/step/track_extent)
- [x] Scroll containers: wheel, keyboard (arrow/page/home/end), programmatic set/scroll_by/scroll_to
- [x] FocusScope: modal, trap_focus, block_background_input, dismiss_on_outside_pointer
- [x] Bubble listeners for container-level activation detection
- [x] WidgetEventCallbacks for per-element custom handlers
- [x] Expand action mapping system (ActionMap, ActionBindingRegistry, KeybindCapture, rebinding + config)
- [ ] Add gamepad abstraction

### UI Runtime Modes
- [x] Add passthrough mode (UiMode::Passthrough: visual state only, no event consumption)
- [x] Ensure zero-cost disable path (UiMode::Disabled: discard queue, no work)

### UI/Event Integration
- [x] Route events through UI → App/Game priority (UiEventResult: pointer_consumed, key_consumed, keys_consumed, scroll_consumed, text_consumed)
- [x] Add event consumption handling (per-key consumption check via key_input_consumed)
- [x] Cx deferred-registration context: PendingRegistrations apply behaviors/configs after tree build, before update

### UI Rendering Integration
- [ ] Backdrop effects (blur, tint, distortion)
- [ ] Depth-aware UI effects
- [ ] Offscreen UI compositing

### Advanced UI
- [ ] Code editor widget
- [ ] Virtualized code editor
- [ ] Scroll inertia/momentum physics
- [ ] Full accessibility support (accessibility_label in WidgetConfig is a stub)
