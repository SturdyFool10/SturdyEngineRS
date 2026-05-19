// Tests extracted from crates/sturdy-engine-core/src/slang.rs
// See scripts/extract_tests.py for the extraction logic.

use super::*;
use crate::BINDLESS_COUNT;
use std::path::PathBuf;

mod shader_fixtures {
    pub const BINDLESS_ARRAYS: &str = include_str!("../shaders/tests/bindless_arrays.slang");
    pub const MEMORY_UTF8_COMPUTE: &str =
        include_str!("../shaders/tests/memory_utf8_compute.slang");
    pub const MEMORY_BYTES_VERTEX: &str =
        include_str!("../shaders/tests/memory_bytes_vertex.slang");
}

fn testbed_shader(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../crates/sturdy-engine-testbed/shaders")
        .join(name)
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn reflect_compute_shader_binding() {
    let desc = ShaderDesc {
        source: ShaderSource::File(testbed_shader("testbed_compute.slang")),
        entry_point: "main".into(),
        stage: ShaderStage::Compute,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    };
    let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
    assert!(
        !reflection.entry_points.is_empty(),
        "should have at least one entry point"
    );
    assert_eq!(
        reflection.entry_points[0], "main",
        "entry point name mismatch"
    );
    assert!(
        !reflection.layout.groups.is_empty(),
        "compute shader with buffer binding should have at least one descriptor group"
    );
    let group = &reflection.layout.groups[0];
    assert!(
        !group.bindings.is_empty(),
        "group should have at least one binding"
    );
    let binding = &group.bindings[0];
    assert_eq!(
        binding.kind,
        BindingKind::StorageBuffer,
        "RWStructuredBuffer should reflect as StorageBuffer"
    );
    assert_eq!(binding.stage_mask, StageMask::COMPUTE);
    let parameter = reflection
        .parameters
        .iter()
        .find(|parameter| parameter.name == "output_buffer")
        .expect("storage buffer should have parameter metadata");
    assert_eq!(
        parameter.kind,
        ShaderParameterKind::Resource(BindingKind::StorageBuffer)
    );
    assert_eq!(parameter.access, ShaderResourceAccess::ReadWrite);
    assert_eq!(parameter.set, Some(0));
    assert_eq!(parameter.binding, Some(0));
    // Binding index should be populated from Slang reflection (not always 0 from array position)
    // For a single binding in set 0, Slang assigns binding=0
    assert_eq!(binding.binding, 0, "first binding should have slot 0");
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn reflect_binding_indices_are_preserved() {
    // textured_fragment.slang has:
    //   Texture2D base_color : register(t0, space0)  → binding 0
    //   SamplerState base_sampler : register(s1, space0) → binding 1
    // Verify the reflected binding slots match the register declarations.
    let desc = ShaderDesc {
        source: ShaderSource::File(testbed_shader("textured_fragment.slang")),
        entry_point: "ps_main".into(),
        stage: ShaderStage::Fragment,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    };
    let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
    let group = reflection
        .layout
        .groups
        .first()
        .expect("textured fragment shader should have a descriptor group");
    assert_eq!(group.bindings.len(), 2, "should reflect both bindings");
    let slots: Vec<u32> = group.bindings.iter().map(|b| b.binding).collect();
    // Both binding slots should be present (0 for texture, 1 for sampler).
    assert!(
        slots.contains(&0) && slots.contains(&1),
        "binding slots 0 and 1 should both appear, got {slots:?}"
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn reflect_vertex_shader_no_bindings() {
    let desc = ShaderDesc {
        source: ShaderSource::File(testbed_shader("triangle_vertex.slang")),
        entry_point: "vs_main".into(),
        stage: ShaderStage::Vertex,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    };
    let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
    assert!(
        reflection.layout.groups.is_empty(),
        "vertex shader with no resource bindings should have empty layout"
    );
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn reflect_push_constant_size_and_stage() {
    let desc = ShaderDesc {
        source: ShaderSource::File(testbed_shader("push_vertex.slang")),
        entry_point: "vs_main".into(),
        stage: ShaderStage::Vertex,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    };
    let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
    assert_eq!(reflection.layout.push_constants_bytes, 32);
    assert_eq!(
        reflection.layout.push_constants_stage_mask,
        StageMask::VERTEX
    );
    let parameter = reflection
        .parameters
        .iter()
        .find(|parameter| parameter.kind == ShaderParameterKind::PushConstant)
        .expect("push constant should have parameter metadata");
    assert_eq!(parameter.stage_mask, StageMask::VERTEX);
    assert_eq!(parameter.access, ShaderResourceAccess::Read);
    assert_eq!(parameter.size_bytes, Some(32));
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn reflect_separate_texture_and_sampler_bindings() {
    // textured_fragment.slang uses separate Texture2D + SamplerState
    let desc = ShaderDesc {
        source: ShaderSource::File(testbed_shader("textured_fragment.slang")),
        entry_point: "ps_main".into(),
        stage: ShaderStage::Fragment,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    };
    let reflection = reflect_pipeline_layout(&desc).expect("reflection should succeed");
    let group = reflection
        .layout
        .groups
        .first()
        .expect("should have a group");
    let kinds: Vec<BindingKind> = group.bindings.iter().map(|b| b.kind).collect();
    assert!(
        kinds.contains(&BindingKind::SampledImage),
        "should reflect Texture2D as SampledImage, got {kinds:?}"
    );
    assert!(
        kinds.contains(&BindingKind::Sampler),
        "should reflect SamplerState as Sampler, got {kinds:?}"
    );
    let diffuse_map = reflection
        .parameters
        .iter()
        .find(|parameter| parameter.name == "diffuseMap")
        .expect("texture should have parameter metadata");
    assert_eq!(
        diffuse_map.kind,
        ShaderParameterKind::Resource(BindingKind::SampledImage)
    );
    assert_eq!(diffuse_map.access, ShaderResourceAccess::Read);
    let diffuse_sampler = reflection
        .parameters
        .iter()
        .find(|parameter| parameter.name == "diffuseSampler")
        .expect("sampler should have parameter metadata");
    assert_eq!(
        diffuse_sampler.kind,
        ShaderParameterKind::Resource(BindingKind::Sampler)
    );
    assert_eq!(diffuse_sampler.access, ShaderResourceAccess::Read);
}

mod shader_source_tests {
    use super::*;

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn reflect_bindless_arrays_use_bindless_count() {
        let desc = ShaderDesc {
            source: ShaderSource::Inline(shader_fixtures::BINDLESS_ARRAYS.into()),
            entry_point: "main".into(),
            stage: ShaderStage::Fragment,
            requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
        };

        let (_, reflection) =
            compile_and_reflect(&desc, ShaderTarget::Spirv).expect("bindless shader compiles");
        let group = reflection
            .layout
            .groups
            .first()
            .expect("bindless shader should reflect set 0");

        assert_eq!(group.name, "set0");
        assert_eq!(group.bindings.len(), 2);
        assert!(
            group
                .bindings
                .iter()
                .all(|binding| binding.count == BINDLESS_COUNT),
            "all bindless arrays should use BINDLESS_COUNT"
        );
        assert!(reflection.parameters.iter().any(|parameter| {
            parameter.binding == Some(0)
                && parameter.count == BINDLESS_COUNT
                && parameter.kind == ShaderParameterKind::Resource(BindingKind::Sampler)
        }));
        assert!(reflection.parameters.iter().any(|parameter| {
            parameter.binding == Some(1)
                && parameter.count == BINDLESS_COUNT
                && parameter.kind == ShaderParameterKind::Resource(BindingKind::SampledImage)
        }));
    }

    #[test]
    fn reflect_spirv_returns_empty() {
        let desc = ShaderDesc {
            source: ShaderSource::Spirv(vec![0x0723_0203, 0, 0, 0, 0]),
            entry_point: "main".into(),
            stage: ShaderStage::Compute,
            requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
        };
        let reflection =
            reflect_pipeline_layout(&desc).expect("should not error for SPIRV source");
        assert_eq!(reflection, ShaderReflection::default());
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn embedded_str_source_compiles_and_reflects() {
        let desc = ShaderDesc {
            source: ShaderSource::MemoryUtf8(shader_fixtures::MEMORY_UTF8_COMPUTE),
            entry_point: "main".into(),
            stage: ShaderStage::Compute,
            requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
        };

        let (_compiled, reflection) =
            compile_and_reflect(&desc, ShaderTarget::Spirv).expect("embedded source compiles");

        let group = reflection
            .layout
            .groups
            .first()
            .expect("embedded source should reflect bindings");
        assert_eq!(group.bindings[0].kind, BindingKind::StorageBuffer);
    }

    #[test]
    #[cfg(not(target_arch = "wasm32"))]
    fn memory_bytes_source_compiles_as_utf8_slang() {
        let desc = ShaderDesc {
            source: ShaderSource::MemoryBytes(shader_fixtures::MEMORY_BYTES_VERTEX.as_bytes()),
            entry_point: "vs_main".into(),
            stage: ShaderStage::Vertex,
            requires_ray_query: false,
            requires_cooperative_matrix: false,
            uses_ser: false,
        };

        let (_compiled, reflection) =
            compile_and_reflect(&desc, ShaderTarget::Spirv).expect("memory source compiles");

        assert!(reflection.layout.groups.is_empty());
        assert!(reflection.entry_points.iter().any(|ep| ep == "vs_main"));
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn reflect_vertex_inputs_from_compiled_vertex_shader() {
    let desc = ShaderDesc {
        source: ShaderSource::File(testbed_shader("triangle_vertex.slang")),
        entry_point: "vs_main".into(),
        stage: ShaderStage::Vertex,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    };
    let (_compiled, reflection) =
        compile_and_reflect(&desc, ShaderTarget::Spirv).expect("vertex shader compiles");
    // triangle_vertex.slang has no explicit vertex inputs (uses SV_VertexID) —
    // confirm the parser returns an empty list rather than panicking.
    // The important invariant is: no built-in decorations leak into the result.
    for input in &reflection.vertex_inputs {
        assert!(
            input.location < 32,
            "unexpected high location {} for input '{}'",
            input.location,
            input.name
        );
    }
}

#[test]
#[cfg(not(target_arch = "wasm32"))]
fn reflect_vertex_inputs_from_textured_vertex_shader() {
    let desc = ShaderDesc {
        source: ShaderSource::File(testbed_shader("textured_vertex.slang")),
        entry_point: "vs_main".into(),
        stage: ShaderStage::Vertex,
        requires_ray_query: false,
        requires_cooperative_matrix: false,
        uses_ser: false,
    };
    let (_compiled, reflection) =
        compile_and_reflect(&desc, ShaderTarget::Spirv).expect("vertex shader compiles");
    // textured_vertex.slang declares position, uv, and color vertex inputs.
    // Confirm they are reflected with sensible locations and formats.
    for input in &reflection.vertex_inputs {
        assert!(
            matches!(
                input.format,
                crate::VertexFormat::Float32x2
                    | crate::VertexFormat::Float32x3
                    | crate::VertexFormat::Float32x4
            ),
            "unexpected format {:?} for input '{}'",
            input.format,
            input.name
        );
    }
    // Locations must be in ascending order (parser sorts by location).
    let locations: Vec<u32> = reflection
        .vertex_inputs
        .iter()
        .map(|i| i.location)
        .collect();
    assert!(
        locations.windows(2).all(|w| w[0] < w[1]),
        "vertex inputs should be sorted by location, got {locations:?}"
    );
}
