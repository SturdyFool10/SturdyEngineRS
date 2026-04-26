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
- [x] `P0.9` Internalize motion-vector generation and debug display registration as runtime features rather than testbed-only plumbing
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
- [x] `P0.28` Add Slang shader hot reload with clear in-app compile error reporting
- [x] `P0.29` Add first-pass asset hot reload for common asset types used by the testbed
- [x] `P0.30` Add stable missing/stale-asset diagnostics surfaced in the runtime overlay or logs
- [x] `P0.31` Add screenshot/export helpers to the first-party runtime shell
- [x] `P0.32` Add image inspection for named graph resources using the runtime-owned debug image registry
- [x] `P0.33` Add a first-pass frame-graph inspection UI or textual inspection surface
- [x] `P0.34` Add GPU timing and per-pass timing summaries to runtime diagnostics
- [x] `P0.35` Convert one existing sample/testbed path to the first-party runtime shell and remove the equivalent app-side boilerplate

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
  - [x] setting accepted as requested
  - [ ] setting clamped or degraded
  - [x] setting rejected with reason
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

- [x] Add immediate-style 2D drawing helpers for:
  - [x] lines
  - [x] polylines
  - [x] rectangles
  - [x] circles
  - [x] points / markers
  - [x] filled shapes
- [x] Support layering those primitives with text in the same frame
- [x] Expose sane defaults for antialiasing, thickness, color, transforms, and hit testing
- [x] Add a built-in debug view picker for motion vectors, bloom chain, AA history, and other named graph images

### Data visualization first mile

- [x] Add a lightweight plotting layer for:
  - [x] axes
  - [x] linear/log scales
  - [x] gridlines
  - [x] line series
  - [x] scatter series
  - [x] bar series
  - [x] legends
  - [x] value tooltips / nearest-point inspection
- [x] Add pan/zoom helpers for 2D data views
- [x] Add a simple palette/theme model for quick visualization work
- [x] Add a “load data, plot it, inspect it” sample with minimal code

### Shader playground usability

- [x] Add a shader-playground app shell with:
  - [x] standard time/frame/resolution uniforms
  - [x] quick texture binding helpers
  - [x] pause / step / scrub controls
  - [x] built-in debug overlays
  - [x] one-click debug image switching
- [x] Make the default playground path use the same runtime shell as normal apps

---

## P2 — App UI Must Feel Complete

The Clay/UI foundation is promising, but the app-facing shell is still missing
core behavior that makes people stay on the engine path instead of bypassing it.

### Text, input, and editing

Prompt-sized text execution order:

1. [x] `P2.T1` Write the text rendering contract and default policy for alpha mask, SDF/MSDF, vector fallback, snapping, blending, and failure behavior
2. [x] `P2.T2` Replace first-party `UiContext` layout-time text estimation with the same shaping/measurement path used for rendering
3. [ ] `P2.T3` Add text quality validation scenes and screenshot/golden coverage for scale factors, HDR/SDR, fallback fonts, and animated UI
4. [ ] `P2.T4` Add text performance instrumentation and budgets for shaping, caching, atlas uploads, draw calls, and memory use
5. [ ] `P2.T5` Upgrade glyph atlas lifetime, dirty uploads, eviction, tiling, and backend-limit handling for real UI workloads
6. [ ] `P2.T6` Integrate engine-side text shaders for alpha mask, SDF/MSDF, outlines, shadows, and linear-light blending
7. [ ] `P2.T7` Expose app-facing rich text controls after measurement, rendering, and atlas behavior are stable
8. [ ] `P2.T8` Build editable text, IME, selection, clipboard, focus, and keyboard navigation on top of the shared text model

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

- [x] Replace placeholder text-size estimation in first-party `UiContext` layout with true `textui` measurement for layout-time wrapping parity
  - [x] add `clay-ui` unit coverage for measured text layout and layout text cache hit/miss behavior
  - [x] bin wrapped text layout and scene cache widths so small window resizes do not invalidate unchanged text
  - [x] ignore available width in text cache keys when a label is estimated to fit naturally, so normal UI labels survive resize churn
- [x] Define the production text rendering contract for UI and engine overlays in [docs/text_rendering_contract.md](docs/text_rendering_contract.md):
  - [x] crisp anti-aliased output at 1x, high-DPI, fractional scale, HDR, and SDR
  - [x] predictable pixel snapping that avoids double-blurring already-antialiased glyphs
  - [x] linear-light blending policy for text over HDR/SDR render targets
  - [x] explicit fallback behavior when a font, glyph, feature, or raster mode is unavailable
- [x] Choose and document the default glyph rendering policy:
  - [x] alpha masks for small body text where hinting and exact stem placement matter most
  - [x] SDF/MSDF for scalable display text, transforms, outlines, shadows, and animated UI
  - [x] vector/path fallback for export, path text, diagnostics, and future high-scale use
  - [x] per-run override hooks for apps that need exact control
- [ ] Build text quality validation scenes:
  - [x] add first-pass `ui_demo` text quality panel covering small labels, dense rows, display text, fallback scripts, and bidi samples
  - [x] surface previous-frame text layout cache hit/miss counters in `ui_demo`
  - [ ] small UI labels, dense tables, code-like text, and large display text
  - [ ] high-contrast, low-contrast, transparent, HDR, and post-tonemapped backgrounds
  - [ ] subpixel positions, scrolling, transforms, clipping, and animated opacity
  - [ ] resize growth/shrink cases that prove text clip extents update with the application viewport
  - [ ] pixel-alignment cases that prove stems do not look thicker, thinner, or smaller as labels move
  - [ ] Latin, CJK, emoji, combining marks, bidirectional text, ligatures, and fallback fonts
- [ ] Add golden-image or screenshot-diff tests for representative text cases across scale factors and output formats
- [ ] Add text performance budgets and instrumentation:
  - [x] expose first-pass per-frame/per-tree text scene, glyph quad, atlas page, and atlas byte counters
  - [x] expose first-pass layout text cache hit/miss counters
  - [x] cache immutable `textui` atlas page snapshots until glyph pixels change
  - [x] pass engine atlas pages and untiled atlas frames as shared immutable byte slices instead of cloning full page pixels
  - [ ] shape/layout cache hit rate for the full `textui` prepared layout and GPU scene caches
  - [ ] glyph atlas page count, occupancy, uploads, and evictions
  - [ ] CPU shaping/layout time
  - [x] expose first-pass UI text command and batch counters
  - [ ] GPU draw calls, sampled pages, and overdraw
  - [ ] memory use for font data, layouts, atlases, and prepared scenes
- [ ] Implement robust glyph atlas management for UI workloads:
  - [x] avoid per-frame full-page copies in the untiled engine atlas path
  - [ ] persistent pages with stable lifetime and frame-delayed destruction
  - [ ] dirty-region uploads instead of whole-page uploads where supported
  - [ ] eviction policy that avoids visible thrash during scrolling and language fallback
  - [ ] backend-limit-aware page sizing, tiling, and sampler selection
- [ ] Make text layout correct before rendering:
  - [ ] shared measurement path for layout, wrapping, hit testing, clipping, and painting
  - [ ] consistent font fallback between measurement and rendering
  - [ ] Unicode line breaking, grapheme-aware cursor movement, and bidirectional paragraph handling
  - [x] snap 2D engine text placement to whole screen pixels to avoid fractional-origin thickness changes
  - [x] clip tiled text atlas quads in atlas UV/page space instead of screen space so resized windows do not hide text beyond the atlas tile extent
  - [x] keep large screen-space UI text on the SDF path until MSDF screen rendering is validated
  - [x] remove frame-varying text layout cache IDs and per-frame pixel hashing from the text atlas tiling hot path
  - [x] upload text atlas RGBA8 bytes directly instead of rebuilding a temporary texture buffer with a per-pixel closure
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

1. [x] `P2.U1` Define the shared widget state/event model for focus, hover, press, capture, disabled/read-only states, validation, and accessibility labels
2. [ ] `P2.U2` Add shape-aware scroll containers with parent clipping, scrollbars, wheel/touchpad input, keyboard scroll, inertial/momentum hooks, and external scroll offset control
3. [ ] `P2.U3` Add first-party buttons, icon buttons, segmented buttons, radio groups, checkboxes, toggles, sliders, drag bars, splitters, resizers, and stateful style variants on top of the shared widget model
4. [ ] `P2.U4` Add dropdown/select/combobox widgets with popup anchoring, keyboard navigation, typeahead, disabled items, separators, and virtualized long-option lists
5. [ ] `P2.U5` Add single-line text inputs with caret, selection, clipboard, validation state, placeholder, password/secret display mode, and IME composition hooks
6. [ ] `P2.U6` Add multiline text inputs with wrapping, scrolling, selection, clipboard, IME, undo/redo, soft tabs, line navigation, and shape-aware clipping
7. [ ] `P2.U7` Add stylable multiline text editing rich enough for code editors: per-range styling, syntax/highlight spans, gutters, line numbers, diagnostics, inline widgets, minimap hooks, code folding hooks, and performant viewport virtualization
8. [ ] `P2.U8` Add modal, dialog, popover, tooltip, context-menu, and command-palette primitives using a top-layer/portal model with focus trapping and backdrop/input blocking rules
   - [x] First-pass `TopLayer` portal host and modal backdrop builders
   - [x] First-pass focus-scope stack with modal background blocking, focus trapping, and restore-focus handoff
9. [ ] `P2.U9` Add date, time, and date-time selector widgets with typed entry, picker popovers, min/max, locale/time-zone formatting hooks, and keyboard-only operation
10. [x] `P2.U10` Add a CSS-style `mosaic` layout primitive for dense visual tiling: named tiles, span/fit/fill modes, intrinsic aspect ratios, responsive breakpoints, stable placement, and predictable hit/clip regions
11. [ ] `P2.U11` Add first-class UI layering and stacking contexts so apps can declare what renders behind/in front of what without relying on tree order hacks
12. [ ] `P2.U12` Add shader/effect style slots for backgrounds, borders, masks, text fills, outlines, shadows, glows, backdrop filters, and per-state transitions, with Slang parameter binding and render-graph pass integration
13. [ ] `P2.U13` Add shape-aware rendering, clipping, hit testing, focus rings, shadows, and effect regions so rounded rects, independent corner shapes, squircles, paths, and masks behave consistently across input and paint
14. [ ] `P2.U14` Add fancy default border options: per-side/per-corner styling, inner/outer/center strokes, dashed/dotted/double strokes, gradient borders, image/shader borders, glow/bloom borders, and polished focus/error/selection presets
15. [ ] `P2.U15` Add virtualized variants for large widgets so lists, grids, tables, trees, logs, inspectors, dropdowns, mosaics, and code editors avoid drawing offscreen elements while preserving measurement, keyboard navigation, selection, and scroll responsiveness
16. [ ] `P2.U16` Add render-graph-aware backdrop/effect shaders so UI surfaces can sample named scene images, blur/grade/dim the game behind a pause menu tile, and route effects through explicit graph resources
17. [ ] `P2.U17` Add validation scenes for all standard widgets, nested scrolling, modals, shape clipping, shader styles, virtualized controls, render-graph backdrops, and resize/scale-factor behavior

- [ ] Add a standard widget layer for:
  - [x] first-pass button, radio, toggle, checkbox, segmented-control, slider, progress-bar, and drag-bar builders on the shared `WidgetState` contract
  - [ ] labels
  - [ ] buttons and icon buttons
  - [x] segmented buttons
  - [ ] radio buttons / radio groups
  - [ ] toggle groups
  - [x] checkboxes / toggles
  - [x] sliders
  - [ ] drag bars / splitters / resizers
  - [x] progress bars / meters
  - [ ] dropdowns / selects / comboboxes
  - [ ] single-line text inputs
  - [ ] multiline text inputs
  - [ ] stylable multiline/code-editor text inputs
  - [ ] time selectors
  - [ ] date selectors
  - [ ] date-time selectors
  - [ ] modals / dialogs
  - [x] first-pass modal layer and transparent portal-host primitives
  - [ ] popovers / tooltips / context menus
  - [ ] menu bars / command bars / toolbars
  - [ ] tabs
  - [ ] accordions / disclosure panels
  - [ ] breadcrumbs
  - [ ] search boxes
  - [ ] lists
  - [ ] tables
  - [ ] grids
  - [ ] trees / inspectors
  - [ ] property editors
  - [ ] color pickers
- [ ] Keep widgets composable with shader-driven visuals instead of forcing a theme-only path
- [ ] Treat every widget as a shape-aware object for:
  - [x] rendering
  - [x] first-pass clipping metadata for children
  - [x] hit testing
  - [ ] focus rings
  - [ ] scroll regions
  - [ ] shadows / outlines / glow
  - [ ] accessibility bounds
- [ ] Add shape primitives and corner controls suitable for high-end UI work:
  - [x] independent corner radii
  - [x] independent corner shape families such as round, bevel, notch, scoop, and chamfer
  - [x] squircle / superellipse shapes with tunable exponent
  - [x] capsule and pill helpers
  - [x] circle and ellipse helpers
  - [ ] per-corner smoothing and antialiasing controls
  - [ ] shape composition for cutouts, holes, masks, and decorative corners
- [ ] Add polished built-in border presets:
  - [ ] subtle hairline, focus ring, inset, raised, etched, glass, neon, warning/error, and selected states
  - [ ] per-side and per-corner color/width/style
  - [ ] inside, centered, and outside stroke placement
  - [ ] dashed, dotted, double, gradient, textured, image, and shader-driven borders
  - [ ] optional shadow, glow, bloom extraction, and animated border parameters
- [ ] Add widget APIs for shader/effect customization that feel familiar to web developers:
  - [x] first-pass slot enum for background, border/outline, text fill/outline, image, mask, shadow, backdrop, overlay, and custom slots
  - [ ] per-state style rules for hover, active, focus, disabled, invalid, selected, checked, and open
  - [x] first-pass bindable shader parameters, textures, graph image names, buffers, gradients, and color values
  - [x] first-pass element-slot shader targeting for background, border/outline, image, and custom render commands
  - [x] strongly typed custom uniforms and additional app-provided uniforms/resources carried alongside built-in UI command data
  - [ ] offscreen effect routes for bloom, blur, drop shadow, glow, and custom passes
- [ ] Add full text styling controls at the widget layer:
  - [ ] font family fallback lists, exact font handles, synthetic fallback policy, and missing-glyph diagnostics
  - [ ] size, line height, weight, stretch, slant/italic, variation axes, and optical sizing
  - [ ] OpenType features, kerning, ligatures, numeric styles, stylistic sets, and language/script tags
  - [ ] color, gradients, opacity, outline, shadow, glow, background spans, and selection styling
  - [ ] wrapping, truncation, ellipsis, alignment, baseline, letter spacing, word spacing, and tab stops
- [ ] Add first-party inspector/panel patterns for graphics tools and game tools

### Layout and interaction behavior

- [ ] Implement arena/reuse strategy equivalent to Clay’s low-allocation hot path
- [ ] Add shape-aware scrollable containers:
  - [x] persistent clamped scroll state with targeted and hit-tested scroll events
  - [x] first-pass clipped scroll container builder with vertical, horizontal, and both-axis offsets
  - [x] vertical, horizontal, and both-axis scrolling
  - [ ] wheel, touchpad, keyboard, drag-scroll, and programmatic scroll input
  - [x] first-pass programmatic scroll commands for scroll-by, scroll-to, page, start, and end movement
  - [x] first-pass scrollbar metrics and composable visual scrollbar builders
  - [ ] overlay scrollbars and custom scrollbar styling
  - [ ] scroll snapping, momentum hooks, sticky children, and anchor preservation
  - [ ] virtualized child measurement for large lists and editor buffers
- [ ] Add Clay-level scroll physics/momentum and external scroll offset query parity
- [ ] Add floating/attach-point semantics parity (`attach_to_parent`, `attach_to_id`, clip inheritance, pointer passthrough modes)
  - [x] first-pass absolute child positioning in `LayoutInput`
  - [x] descendant subtree translation when a positioned parent moves
  - [x] first-pass anchored floating rect placement from an anchor rect, side, alignment, offset, and viewport margin
  - [x] first-pass viewport collision handling with flip, clamp, and flip-then-clamp policies
  - [ ] direct `attach_to_id` resolution from a computed `LayoutTree`
- [ ] Add a UI top-layer and portal system for modals, dropdowns, popovers, tooltips, context menus, and drag previews
  - [x] first-pass transparent portal host builder for top-layer content
  - [x] first-pass modal backdrop layer builder that clips to the viewport and captures input
  - [x] first-pass focus scope stack for modal blocking, focus trapping, and restore-focus behavior
  - [x] first-pass anchored popover/menu placement and collision avoidance helper
  - [x] first-pass dismiss signaling for outside pointer presses and cancel events
  - [ ] full dismiss policies for escape key mapping, focus loss, parent close, nested scopes, and delayed pointer capture
- [ ] Add explicit stacking contexts and layer slots:
  - [x] first-pass `UiLayer` model carried through layout, hit testing, and render command ordering
  - [x] background/content/foreground/overlay/top-layer slots
  - [ ] app-declared z ordering independent of tree insertion order
  - [x] hit-test ordering that matches visual stacking across first-pass layer slots
  - [x] first-pass pointer passthrough, modal blocking, and focus trapping
  - [ ] full event capture, nested modal policies, and keyboard navigation loops
- [ ] Add parent clipping that can clip children by:
  - [ ] rect
  - [ ] rounded rect
  - [x] first-pass resolved shape metadata on clip commands
  - [ ] arbitrary path / mask where backend support exists
  - [ ] scroll viewport
  - [ ] shader-generated alpha mask where explicitly requested
- [ ] Add a CSS-style `mosaic` layout function:
  - [x] first-pass stable dense packing of heterogenous tiles
  - [x] first-pass named tile areas, declarative spans, and explicit tile placement with collision diagnostics
  - [x] first-pass fixed cells, aspect-ratio fit, and span/fill/fit modes
  - [x] first-pass responsive breakpoint rules for column counts
  - [x] first-pass deterministic hit-test traversal and viewport filtering
  - [x] first-pass element builder that emits mosaic tiles as absolute-positioned children
  - [x] first-pass virtualized viewport builder that preserves full content size
- [ ] Add virtualized versions of every potentially large primitive:
  - [x] first-pass fixed-size virtual list range/spacer helper
  - [x] first-pass virtual list builder
  - [x] first-pass fixed-size virtual grid range/spacer helper and builder
  - [x] first-pass fixed-size virtual table range/spacer helper and builder
  - [x] first-pass fixed-height virtual tree / inspector range/spacer helper and builder
  - [x] virtual dropdown menu
  - [x] virtual log viewer
  - [x] virtual mosaic
  - [ ] virtual rich/code editor
  - [ ] shared item measurement, cache invalidation, scroll anchoring, selection retention, and focus retention
- [ ] Implement child-between-border emission parity and exact border raster semantics
- [ ] Add virtualized scrolling / large-list support
- [ ] Add robust app-facing event/state plumbing so users do not need large glue layers

### UI rendering completeness

- [ ] Add full render-graph resource binding generation (bind groups, push constants, per-pass parameter buffers) instead of pass skeletons
  - [x] first-pass graph read planning for UI shader slot image, named-image, and buffer resources
  - [ ] bind group creation and reflected binding layout merge for UI shader slots
  - [x] first-pass app uniform push-constant byte packing with stable named offsets
  - [x] first-pass graph pass push-constant attachment for single-command UI shader-slot batches
  - [x] first-pass multi-command app-uniform parameter batch byte packing with per-command offsets
  - [ ] GPU buffer allocation/upload and binding for per-pass parameter buffers
  - [ ] built-in UI uniform parameter packing for rect, UV, shape, clip, state, time, and DPI scale
- [ ] Integrate real shader pipelines for all UI slots in engine runtime
- [ ] Define the UI shape contract shared by rendering, input, clipping, and effects:
  - [ ] rectangle
  - [ ] rounded rectangle with independent radii
  - [x] first-pass `UiShape` model carried through element style and render commands
  - [x] rectangle
  - [x] rounded rectangle with independent radii
  - [x] per-corner independent shape families and properties
  - [x] squircle / superellipse
  - [x] capsule / pill
  - [x] circle / ellipse
  - [x] resolved shape coverage helpers used by input hit testing
  - [x] clip commands carry resolved shape for future mask/scissor backends
  - [ ] arbitrary path / mask
  - [ ] shader-produced coverage mask
- [ ] Make UI antialiasing analytic and shape-aware for fills, borders, outlines, masks, and clips
- [ ] Add first-party SVG and raster image primitives:
  - [x] first-pass `resvg` backed SVG parse/raster asset primitive
  - [x] exposed SVG/vector AA dials for native AA, supersampling, downsample filter, target scale, pixel snapping, and max render size
  - [x] first-pass encoded raster image decode into RGBA8 UI assets
  - [x] image element/widget metadata for fit mode, sampler policy, tint, and edge AA
- [ ] Add GPU gradient shader implementation and parameter packing contract used by engine pipelines
- [ ] Add text outline rendering path in engine shader side
- [ ] Add a UI material/effect model with slots for:
  - [x] first-pass fill/background shader slot metadata
  - [x] first-pass border/outline shader slot metadata
  - [ ] text fill/outline/shadow/glow
  - [ ] inner shadow
  - [ ] drop shadow
  - [x] first-pass mask/clip and backdrop slot declarations
  - [ ] bloom/glow extraction
  - [ ] custom Slang pass hooks
- [ ] Add element-scoped shader overrides:
  - [x] select a concrete element by ID and render slot through `ElementStyle` slot bindings
  - [ ] run a custom fragment shader over that slot's exact shape coverage and clip region
  - [x] carry additional user uniforms alongside built-in UI command data such as element rect, shape, clip, layer, and z order
  - [ ] validate merged built-in/user uniform layouts with stable reflection and clear diagnostics
  - [x] declare app textures, graph image names, samplers, and small parameter buffers on the element slot descriptor
  - [x] preserve normal UI hit testing, layout, batching diagnostics, and fallback shader selection metadata
- [ ] Allow per-slot effect routing so a widget can, for example, send only its border to bloom while keeping the fill in the normal UI pass
- [ ] Allow UI shaders to sample render-graph resources through explicit declarations:
  - [ ] previous scene color
  - [ ] HDR scene color
  - [ ] depth
  - [ ] normals / material IDs where available
  - [x] first-pass named graph/debug image declarations resolved into graph reads
  - [x] first-pass app-provided textures and buffers declared as graph reads
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
