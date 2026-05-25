# SRD: Sturdy Real-Time Denoiser

SRD is SturdyEngine's first-party real-time denoising system. It is a Rust-first, engine-native denoiser for sparse rendering signals such as hardware path-traced samples, ray-traced lighting, ambient occlusion, soft shadows, and future GI/reflection outputs.

SRD is not a wrapper around a vendor denoising SDK. The implementation should be legally distinct in public names, shader binding names, file layout, integration model, settings names, and debug labels.

## Accepted public terminology

Use these names in Rust APIs, shader bindings, documentation, runtime UI, and debug overlays:

| Concept | SRD term |
|---|---|
| Engine denoiser system | SRD / Sturdy Real-Time Denoiser |
| Main simple temporal path | `ReferenceTemporal` |
| Radiance reconstruction family | `RadianceStabilizer` |
| Shadow reconstruction family | `ShadowStabilizer` |
| AO/directional occlusion family | `OcclusionStabilizer` |
| Current noisy input binding | `srd_current_signal` |
| Previous history input binding | `srd_history_signal` |
| Current noisy input sampler | `srd_current_sampler` |
| Previous history sampler | `srd_history_sampler` |
| Temporal constants block | `SrdTemporalConstants` |
| Public Rust entry point | `SrdDenoiser` |

## Names to avoid

Do not expose vendor SDK identifiers, family names, namespaces, shader binding names, or file naming conventions as SRD public API. In particular, avoid adopting names from third-party denoiser packages for:

- denoiser family names,
- settings structs,
- resource slot names,
- shader pass names,
- shader helper functions,
- namespace/module names,
- debug labels.

Research notes may cite papers or prior art, but implementation-facing names should remain SRD-owned.

## Design rules

1. **Rust owns the host API.** SRD public types should be strongly typed Rust structs/enums with validation helpers.
2. **The algorithm layer is backend-neutral.** SRD may describe GPU work, resources, and dispatches, but renderer/RHI code owns allocation, barriers, binding, and submission.
3. **Settings must be explicit.** Invalid frame sizes, depth scales, jitter, accumulation settings, and resource descriptions should fail early with actionable errors.
4. **History is SRD-owned.** Persistent denoising history should be represented as SRD-managed history/pool state, not ad-hoc per-demo graph names.
5. **Shader contracts must be documented.** Motion vectors, linear depth, normal/roughness/material packing, hit distance, and spectral bins must have engine-standard definitions before advanced reconstruction relies on them.
6. **Debuggability is required.** Every SRD pass should have stable debug labels and expose enough intermediate output to diagnose history rejection, variance, and guide mismatch.

## Current implementation status

The current implementation provides the SRD reference temporal accumulation path used by the hardware path-tracing testbed. It establishes SRD-owned Rust names, shader binding names, constants, descriptor planning, history-pool metadata, a graph-backed reference executor, and runtime controls for quality/reset in the testbed.

The graph-backed executor is an interim bridge: it consumes SRD dispatch/resource/pipeline descriptions and maps them onto the existing fullscreen render-graph path. Future compute implementations should preserve the same descriptor contract while replacing the fullscreen bridge with storage-texture compute submission.

The remaining roadmap work is tracked in `ROADMAP.md` under `Priority 0 — SRD Engine-Standard Denoiser`.
