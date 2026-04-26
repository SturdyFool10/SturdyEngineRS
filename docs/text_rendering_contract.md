# Text Rendering Contract

This contract defines what UI and engine overlay text must provide before the
text stack is considered production-ready.

## Goals

- Text must be crisp and anti-aliased at 1x, high-DPI, fractional scale, HDR,
  and SDR.
- Layout, wrapping, hit testing, clipping, and painting must use the same
  shaping and fallback decisions.
- Small body text must prioritize exact stem placement and readable hinting.
- Display text, animated text, outlines, shadows, and transformed text must have
  scalable rendering paths.
- Apps must not manage glyph atlas pages, uploads, render batches, or fallback
  font plumbing directly.

## Default Glyph Policy

- `AlphaMask` is the preferred default for small UI text. It preserves native
  rasterizer hinting and keeps dense labels, tables, and editor text readable.
- `Sdf` is the preferred scalable path for simple large text, soft shadows, and
  moderate transforms.
- `Msdf` is the preferred scalable path for sharp corners, outlines, display
  text, and larger animated UI.
- Vector/path output remains available for export, text-on-path, diagnostics,
  and future high-scale rendering.
- `Auto` must choose the path from font size, transform scale, effect
  requirements, output target, and backend capabilities. Apps can override the
  mode per run when they need exact control.

## Pixel And Color Rules

- Already-antialiased atlas glyphs must be snapped to the physical pixel grid
  before sampling, unless the text run explicitly opts into subpixel animation.
- Alpha-mask glyphs should use linear filtering only when placement or scaling
  requires it; exact 1:1 UI text should avoid extra blur.
- SDF/MSDF glyphs must sample with a field range matched to the atlas content
  and shader reconstruction path.
- Text over HDR and SDR targets must follow an explicit linear-light blending
  policy. The text stack must not silently mix gamma-space atlas content with
  scene-linear render targets.
- Tonemapped and transparent UI backgrounds must be part of validation because
  text contrast and edge quality fail differently there.

## Correctness Rules

- Font fallback must be identical for measurement and rendering.
- Line wrapping, truncation, ellipsis, and clipping must be based on shaped
  glyph runs, not byte counts or approximate character widths.
- Cursor movement, selection, and hit testing must be grapheme-aware.
- Paragraph layout must handle bidirectional text, combining marks, ligatures,
  emoji, CJK, and fallback fonts.
- Rounding must be deterministic so text does not shimmer during scrolling,
  animation, or repeated layout.
- Missing fonts, missing glyphs, unsupported features, and unsupported raster
  modes must produce explicit fallback behavior or diagnostics.

## Performance Rules

- The engine should expose per-frame text metrics for shaping/layout time, cache
  hit rates, glyph atlas page count, atlas occupancy, upload bytes, eviction
  count, draw calls, sampled pages, and prepared scene memory.
- Stable UI should have high shape/layout and prepared-scene cache hit rates.
- Scrolling large text or tables must not cause visible atlas eviction thrash.
- Atlas uploads should use dirty regions when the backend supports it.
- Atlas page size, tiling, sampler choice, and image count must respect backend
  limits and report degradation when exact settings cannot be honored.

## Validation Scenes

The text stack should include scenes for:

- small labels, dense tables, code-like text, and large display text
- high-contrast, low-contrast, transparent, HDR, and post-tonemapped backgrounds
- subpixel placement, scrolling, clipping, transforms, and animated opacity
- Latin, CJK, emoji, combining marks, bidirectional text, ligatures, and fallback
  fonts
- outlines, shadows, glows, underline, strikethrough, and background highlights

Representative scenes should have screenshot or golden-image coverage across
scale factors and output formats.
