# Runtime Product Direction

This document turns the current testbed findings into product-facing engine
direction. The goal is to remove obvious boilerplate from applications while
keeping a clean path into deeper control.

## What The Testbed Shows

The current testbed is doing engine work in application code:

- It manually assembles the common frame pipeline: scene target, optional MSAA,
  motion vectors, bloom, AA, tonemap, HUD, present.
- It manually owns debug controls for AA, bloom, HDR, tone mapping, and debug
  image switching.
- It manually manages text atlas images, tiling, uploads, and quad generation.
- It manually handles HDR surface policy and surface recreation.

That is useful for validating primitives, but it is the wrong default product
surface. App authors should not need to rebuild these systems.

## Product Rule

The simple path must use the same core systems as the advanced path.

That means:

- the built-in app shell cannot be a toy wrapper
- the debug overlay cannot be a separate one-off
- runtime settings must flow through the same renderer/surface/device systems
- opting into more control must reveal deeper layers, not replace the stack

## Runtime Configuration Model

The engine should model runtime changes explicitly instead of pretending every
setting is the same kind of toggle.

### Change classes

Every runtime setting should resolve to one of these internal apply paths:

1. `Immediate`
   - just patch CPU state or bindings and use it next frame
   - examples: bloom threshold, tonemap exposure, debug overlay visibility

2. `GraphRebuild`
   - rebuild passes, pipelines, or graph layout without changing the surface
   - examples: AA mode, enabling motion-vector debug view, changing post chain topology

3. `SurfaceRecreate`
   - keep the app and device alive, recreate presentation/surface resources
   - examples: HDR mode, present mode, surface alpha mode, swapchain format/color space

4. `WindowReconfigure`
   - update native window attributes or compositor integration
   - examples: window transparency, blur/material effect, chrome changes

5. `DeviceMigration`
   - move the live runtime to another adapter or backend without app restart
   - examples: backend switch, GPU switch, major feature availability change

The public API should surface the result clearly:

- applied exactly
- applied with degradation/clamping
- rejected with reason

## Proposed Public Runtime API

The application shell should expose a first-party runtime controller:

```rust
pub struct RuntimeController {
    // engine-owned runtime state
}

impl RuntimeController {
    pub fn settings(&self) -> RuntimeSettingsSnapshot;
    pub fn transact(&mut self) -> RuntimeSettingsTransaction<'_>;
    pub fn diagnostics(&self) -> RuntimeDiagnostics;
}
```

The transaction groups multiple changes so the runtime can apply them in a
coherent order:

```rust
runtime
    .transact()
    .set_aa(AntiAliasingConfig::taa())
    .set_hdr_mode(HdrPreference::PreferHdr)
    .set_window_background(WindowBackground::windows_acrylic())
    .set_surface_transparency(SurfaceTransparency::Enabled)
    .apply()?;
```

The result should describe what happened:

```rust
pub struct RuntimeApplyReport {
    pub changes: Vec<RuntimeChangeResult>,
}

pub enum RuntimeChangeResult {
    Applied { setting: RuntimeSettingKey, path: RuntimeApplyPath },
    Degraded {
        setting: RuntimeSettingKey,
        path: RuntimeApplyPath,
        reason: String,
    },
    Rejected {
        setting: RuntimeSettingKey,
        reason: String,
    },
}
```

## Internalize The Testbed

The first internalization target is a default runtime shell.

### `AppRuntime`

`AppRuntime` should own:

- surface creation/acquire/present
- HDR output policy
- default scene target selection
- MSAA allocation and resolve
- bloom / AA / tonemap chain
- default debug image registry
- renderer diagnostics collection
- default overlay text/panel layer
- runtime settings controller

Applications should provide content and hooks, not shell plumbing:

```rust
pub trait AppLayer {
    fn render_scene(&mut self, cx: &mut SceneRenderContext<'_>) -> Result<()>;
    fn build_ui(&mut self, ui: &mut UiContext<'_>) -> Result<()>;
}
```

### `DebugShell`

This should be a first-party engine system, not testbed code.

It should provide:

- typed bool/enum/float/int controls
- input action bindings
- overlay text and inspector panels
- debug image picker
- timings, backend, adapter, HDR, AA, present mode display

### `TextOverlay`

Applications should not touch tiled atlas uploads directly just to draw a HUD.

`TextOverlay` should own:

- `textui` shaping/raster requests
- engine-limit-aware tiling
- atlas image lifetime
- page uploads
- batching and draw emission

The app should say “draw text/panel here,” not “rebuild my atlas pages.”

## Window Transparency And Background Effects

Window background handling should be a first-class engine concept.

### Goals

- easy presets for common use
- explicit low-level control when needed
- runtime toggle without app restart
- transparent surface content when the platform allows it

### Proposed public model

```rust
pub enum SurfaceTransparency {
    Disabled,
    Enabled,
}

pub enum WindowBackdrop {
    None,
    Transparent(WindowTransparencyDesc),
    Blurred(WindowBlurDesc),
    Material(WindowMaterialDesc),
    Custom(WindowBackdropHandle),
}

pub enum WindowBackgroundEffect {
    None,
    Blur(WindowBlurDesc),
    Material(WindowMaterialDesc),
    Custom(WindowEffectHandle),
}
```

Preset builders should exist for convenience:

```rust
WindowBackgroundEffect::windows_blur();
WindowBackgroundEffect::windows_acrylic();
WindowBackgroundEffect::windows_mica();
WindowBackgroundEffect::windows_tabbed();
WindowBackgroundEffect::macos_vibrancy(MacosMaterial::HudWindow);
WindowBackgroundEffect::wayland_background_blur();
```

That convenience surface is fine for quick usage, but the engine should expose a
single platform-agnostic abstraction above all OS-specific effect systems.

### Platform-agnostic abstraction rule

Do not expose raw Windows/macOS/Wayland effect APIs as the main engine surface.

The public API should describe:

- what visual result the app wants
- what control level it wants
- what the compositor/platform can actually do

The engine should map that request to the right platform-specific implementation.

### Proposed abstraction

```rust
pub struct WindowAppearance {
    pub transparency: SurfaceTransparency,
    pub backdrop: WindowBackdrop,
    pub corner_style: Option<WindowCornerStyle>,
    pub shadow: WindowShadowMode,
}

pub enum WindowBackdrop {
    None,
    Transparent(WindowTransparencyDesc),
    Blurred(WindowBlurDesc),
    Material(WindowMaterialDesc),
    Custom(WindowBackdropHandle),
}

pub struct WindowBlurDesc {
    pub radius: Option<f32>,
    pub tint: Option<LinearColor>,
    pub opacity: f32,
    pub region: WindowEffectRegion,
    pub quality: WindowEffectQuality,
}

pub struct WindowMaterialDesc {
    pub kind: WindowMaterialKind,
    pub tint: Option<LinearColor>,
    pub fallback_blur: Option<WindowBlurDesc>,
    pub region: WindowEffectRegion,
}

pub enum WindowMaterialKind {
    Auto,
    ThinTranslucent,
    ThickTranslucent,
    NoiseTranslucent,
    TitlebarTranslucent,
    Hud,
}

pub enum WindowEffectRegion {
    FullWindow,
    ClientArea,
    Titlebar,
    Custom(WindowRegionHandle),
}
```

This keeps the semantic contract platform-neutral:

- `ThinTranslucent` can map to one Windows material, one macOS material, and a
  compositor blur/tint combination on Wayland
- `TitlebarTranslucent` can map to tabbed/titlebar-specific materials where they
  exist and degrade cleanly where they do not
- `Hud` gives the engine a semantic target for tool-style windows without
  forcing consumers to know platform names

### Capability model

The engine should report capabilities in the same abstract language:

```rust
pub struct WindowAppearanceCaps {
    pub transparency: RuntimeCapabilityState,
    pub blur: RuntimeCapabilityState,
    pub materials: Vec<WindowMaterialSupport>,
    pub custom_regions: RuntimeCapabilityState,
    pub live_reconfiguration: RuntimeCapabilityState,
}

pub struct WindowMaterialSupport {
    pub kind: WindowMaterialKind,
    pub quality: WindowEffectQuality,
}
```

That lets the app ask “can I do a translucent HUD window?” instead of “do you
support Windows acrylic or NSVisualEffectMaterialHudWindow?”

### Control layers

The API should have two layers:

1. Semantic presets for most users
   - `WindowBackdrop::Blurred(...)`
   - `WindowMaterialKind::Hud`
   - `WindowEffectRegion::FullWindow`

2. Escape hatches for advanced users
   - `Custom(WindowBackdropHandle)`
   - platform-specific extension objects behind capability checks
   - explicit region handles for engine-managed or user-managed regions

The escape hatch should exist, but it should not be the default path.

### Platform adapters underneath

Internally the engine should implement platform adapters such as:

- `windows_window_appearance.rs`
- `macos_window_appearance.rs`
- `wayland_window_appearance.rs`

Those adapters should translate from the semantic model into native calls.

The higher layers should not care whether the actual implementation uses:

- Windows DWM blur/acrylic/mica/tabbed variants
- macOS vibrancy/material APIs
- Wayland `ext-background-effect-v1`

The app asks for intent. The adapter picks the best native implementation.

### Platform policy

- Windows:
  - map to DWM/system material families such as blur-behind, acrylic, mica,
    and tabbed/titlebar variants
- macOS:
  - map to NSVisualEffectView/vibrancy/material behaviors
- Linux:
  - on Wayland, use `ext-background-effect-v1` as the primary background-effect protocol
  - keep older compositor-specific blur protocols as explicit compatibility fallbacks only
  - degrade cleanly to transparency/no backdrop effect when the compositor exposes no supported protocol

The public API should stay the same regardless of which platform adapter wins.

### Surface alpha

The engine should separately model presentation alpha support. A blurred or
transparent window is not enough if the presented surface cannot preserve alpha.

The runtime should expose:

- whether alpha presentation is supported
- whether alpha is currently enabled
- what fallback was used if alpha presentation is unavailable

## No-Restart Backend And Device Switching

Changing backend or GPU is harder than changing HDR or blur, but the product
goal should still be “no restart.”

That requires a live migration path:

1. quiesce the current frame loop
2. snapshot runtime settings
3. create the new device/backend
4. recreate or migrate logical resources
5. recreate the surface/window attachments as needed
6. restore runtime settings with degradation reports where needed
7. resume rendering

The important point is that the application should not rebuild itself around
this. The runtime should own the migration.

## First Implementation Slice

The first slice should remove the biggest amount of testbed boilerplate with the
lowest architectural risk.

### Slice 1

1. Add `DebugShell`
   - overlay text
   - action binding registry
   - built-in renderer/runtime diagnostics

2. Add `TextOverlay`
   - internalize tiled text atlas management now living in the testbed

3. Add `AppRuntime`
   - own the common post stack and debug-image registry
   - expose hooks for scene render and UI build

4. Extend window/surface config types
   - surface transparency
   - window background effect/material
   - runtime reconfigure transaction surface

This sequence removes a large amount of testbed-specific scaffolding while
laying the right foundation for runtime transparency, blur/material effects, and
later backend/device migration.
