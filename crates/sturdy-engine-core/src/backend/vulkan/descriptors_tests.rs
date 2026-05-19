// Tests extracted from crates/sturdy-engine-core/src/backend/vulkan/descriptors.rs
// Runtime code should stay separate from test code.

use super::*;
use crate::{CanonicalGroupLayout, UpdateRate};

fn binding(kind: BindingKind, count: u32, stage_mask: StageMask) -> CanonicalBinding {
    CanonicalBinding {
        path: format!("{kind:?}"),
        kind,
        count,
        stage_mask,
        update_rate: UpdateRate::Draw,
        binding: 0,
    }
}

fn layout_with(bindings: Vec<CanonicalBinding>) -> CanonicalPipelineLayout {
    CanonicalPipelineLayout {
        groups: vec![CanonicalGroupLayout {
            name: "set0".into(),
            bindings,
        }],
        push_constants_bytes: 0,
        push_constants_stage_mask: StageMask::default(),
    }
}

#[test]
fn validates_push_constant_limit() {
    let mut layout = CanonicalPipelineLayout::default();
    layout.push_constants_bytes = 256;
    let limits = Limits {
        max_push_constants_size: 128,
        ..Limits::default()
    };

    assert!(matches!(
        validate_pipeline_layout(&layout, false, &limits),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn validates_per_stage_descriptor_limit() {
    let layout = layout_with(vec![binding(
        BindingKind::SampledImage,
        3,
        StageMask::FRAGMENT,
    )]);
    let limits = Limits {
        max_per_stage_sampled_images: 2,
        max_descriptor_set_sampled_images: 16,
        ..Limits::default()
    };

    assert!(matches!(
        validate_pipeline_layout(&layout, false, &limits),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn bindless_set_requires_bindless_support() {
    let layout = layout_with(vec![binding(
        BindingKind::SampledImage,
        BINDLESS_COUNT,
        StageMask::FRAGMENT,
    )]);

    assert!(matches!(
        validate_pipeline_layout(&layout, false, &Limits::default()),
        Err(Error::Unsupported(_))
    ));
}

#[test]
fn bindless_set_is_exempt_from_finite_descriptor_counts() {
    let mut bindless = binding(
        BindingKind::SampledImage,
        BINDLESS_COUNT,
        StageMask::FRAGMENT,
    );
    bindless.binding = BINDLESS_SAMPLED_IMAGE_BINDING;
    let layout = layout_with(vec![bindless]);
    let limits = Limits {
        max_per_stage_sampled_images: 1,
        max_descriptor_set_sampled_images: 1,
        ..Limits::default()
    };

    assert!(validate_pipeline_layout(&layout, true, &limits).is_ok());
}

#[test]
fn bindless_set_validates_heap_binding_slots() {
    let mut bad_binding = binding(
        BindingKind::SampledImage,
        BINDLESS_COUNT,
        StageMask::FRAGMENT,
    );
    bad_binding.binding = 7;
    let layout = layout_with(vec![bad_binding]);

    assert!(matches!(
        validate_pipeline_layout(&layout, true, &Limits::default()),
        Err(Error::InvalidInput(_))
    ));
}

#[test]
fn bindless_set_rejects_mixed_finite_bindings() {
    let mut finite = binding(BindingKind::Sampler, 1, StageMask::FRAGMENT);
    finite.binding = BINDLESS_SAMPLER_BINDING;
    let mut bindless = binding(
        BindingKind::SampledImage,
        BINDLESS_COUNT,
        StageMask::FRAGMENT,
    );
    bindless.binding = BINDLESS_SAMPLED_IMAGE_BINDING;
    let layout = layout_with(vec![finite, bindless]);

    assert!(matches!(
        validate_pipeline_layout(&layout, true, &Limits::default()),
        Err(Error::InvalidInput(_))
    ));
}
