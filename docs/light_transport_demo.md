# Clean-room Light Transport Demo

This demo is an original SturdyEngine implementation inspired by the rendering ideas in `e10b/light`.

The upstream repository does not expose a visible license file or Cargo license field, so its shader and Rust source should not be copied into SturdyEngine. The implementation here uses only high-level rendering concepts:

- camera rays over a full-screen pass,
- analytic scene intersections,
- sky/sun direct lighting,
- dielectric Fresnel reflection/refraction,
- approximate per-channel dispersion,
- Beer-style absorption through glass,
- photon-map-style caustic density estimated procedurally on the receiver plane,
- simple debug views for caustic and normal inspection.

## Files

- `crates/sturdy-engine-testbed/shaders/light_transport_fragment.slang` — original Slang fullscreen shader.
- `crates/sturdy-engine-testbed/src/bin/light_transport_demo.rs` — standalone testbed binary.

## Running

```sh
cargo run -p sturdy-engine-testbed --bin light_transport_demo
```

## Controls

- `P` — pause or resume animation.
- `S` — step one frame while paused.
- `V` — cycle between beauty, caustic-only, and normal debug views.
- `[` / `]` — decrease/increase shader exposure.

## Technique summary

The shader renders an analytic glass sphere over a ground plane. It traces a primary ray against the sphere and ground. For glass hits, it evaluates dielectric Fresnel, traces reflected environment light, and traces refracted paths through the sphere separately for red, green, and blue IOR values to create a small dispersion split. Ground hits receive sky ambient, sun direct lighting, a softened analytic sphere shadow, and a deterministic caustic estimate made from an elliptical focus region plus several stable pseudo-photon lobes.

This is intentionally a compact SturdyEngine-style demo rather than a line-by-line port of the upstream WGSL renderer.
