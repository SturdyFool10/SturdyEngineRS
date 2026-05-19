// Tests extracted from crates/sturdy-engine-core/src/render_graph/alias_plan.rs
// See scripts/extract_tests.py for the extraction logic.

use super::super::{VirtualBuffer, VirtualImage};
use super::*;
use crate::{BufferDesc, Extent3d, ImageUsage};

fn desc_defaults() -> ImageDesc {
    ImageDesc {
        dimension: crate::ImageDimension::D2,
        extent: Extent3d::default(),
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: ImageUsage::SAMPLED,
        transient: false,
        clear_value: None,
        debug_name: None,
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
        drm_format_modifier: None,
    }
}

fn color_image(handle: ImageHandle, w: u32, h: u32) -> VirtualImage {
    VirtualImage {
        handle,
        desc: ImageDesc {
            extent: Extent3d {
                width: w,
                height: h,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::RENDER_TARGET | ImageUsage::SAMPLED,
            ..desc_defaults()
        },
        imported: false,
        first_use: 0,
        last_use: 0,
    }
}

fn depth_image(handle: ImageHandle) -> VirtualImage {
    VirtualImage {
        handle,
        desc: ImageDesc {
            extent: Extent3d {
                width: 1920,
                height: 1080,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Depth32Float,
            usage: ImageUsage::DEPTH_STENCIL,
            ..desc_defaults()
        },
        imported: false,
        first_use: 0,
        last_use: 0,
    }
}

fn hdr_image(handle: ImageHandle) -> VirtualImage {
    VirtualImage {
        handle,
        desc: ImageDesc {
            extent: Extent3d {
                width: 1920,
                height: 1080,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba16Float,
            usage: ImageUsage::RENDER_TARGET | ImageUsage::SAMPLED,
            ..desc_defaults()
        },
        imported: false,
        first_use: 0,
        last_use: 0,
    }
}

#[test]
fn pack_lifetimes_assigns_non_overlapping_to_same_slot() {
    let resources = vec![
        (ImageHandle(0), 0u32, 1u32),
        (ImageHandle(1), 2u32, 3u32),
        (ImageHandle(2), 0u32, 3u32),
    ];
    let (lifetimes, slot_count) = pack_lifetimes(resources.into_iter(), 0);
    assert_eq!(slot_count, 2);
    let slot_for = |h: ImageHandle| {
        lifetimes
            .iter()
            .find(|(hh, _)| *hh == h)
            .unwrap()
            .1
            .alias_slot
    };
    assert_eq!(slot_for(ImageHandle(0)), slot_for(ImageHandle(1)));
    assert_ne!(slot_for(ImageHandle(0)), slot_for(ImageHandle(2)));
}

#[test]
fn pack_lifetimes_all_overlapping_gets_unique_slots() {
    let resources = vec![
        (ImageHandle(0), 0u32, 3u32),
        (ImageHandle(1), 1u32, 4u32),
        (ImageHandle(2), 2u32, 5u32),
    ];
    let (_, slot_count) = pack_lifetimes(resources.into_iter(), 0);
    assert_eq!(slot_count, 3);
}

#[test]
fn pack_lifetimes_empty_produces_zero_slots() {
    let (lifetimes, slot_count) =
        pack_lifetimes(std::iter::empty::<(ImageHandle, u32, u32)>(), 0);
    assert_eq!(slot_count, 0);
    assert!(lifetimes.is_empty());
}

#[test]
fn pack_lifetimes_slot_offset_applied() {
    let resources = vec![(ImageHandle(0), 0u32, 1u32)];
    let (lifetimes, slot_count) = pack_lifetimes(resources.into_iter(), 10);
    assert_eq!(slot_count, 1);
    assert_eq!(lifetimes[0].1.alias_slot, 10);
}

#[test]
fn different_compatibility_classes_get_independent_slots() {
    // Color and depth images must not alias each other (different memory types).
    let mut depth = depth_image(ImageHandle(0));
    let mut color = color_image(ImageHandle(1), 1920, 1080);
    depth.first_use = 0;
    depth.last_use = 5;
    color.first_use = 0;
    color.last_use = 5;

    let plan = build_alias_plan(&[depth, color], &[]);
    // Both have overlapping lifetimes but different compat classes → 2 slots.
    assert_eq!(plan.image_slot_count, 2);
}

#[test]
fn alias_plan_contains_concrete_placements() {
    let images = vec![VirtualImage {
        handle: ImageHandle(7),
        desc: ImageDesc {
            extent: Extent3d {
                width: 4,
                height: 4,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::SAMPLED,
            ..desc_defaults()
        },
        imported: false,
        first_use: 0,
        last_use: 2,
    }];
    let buffers = vec![VirtualBuffer {
        handle: BufferHandle(9),
        desc: BufferDesc {
            size: 128,
            usage: BufferUsage::COPY_DST,
        },
        imported: false,
        first_use: 1,
        last_use: 3,
    }];

    let plan = build_alias_plan(&images, &buffers);
    assert_eq!(plan.image_placements.len(), 1);
    assert_eq!(plan.buffer_placements.len(), 1);

    let ip = &plan.image_placements[0].1;
    assert_eq!(ip.heap, 0);
    assert_eq!(ip.offset, 0);
    assert_eq!(ip.size, 64);
    assert_eq!(ip.alignment, 256);
    assert_eq!(ip.lifetime.first_pass, 0);
    assert_eq!(
        ip.compatibility,
        AliasCompatibilityClass {
            kind: AliasResourceKind::Image,
            format: Format::Rgba8Unorm,
            usage_bits: ImageUsage::SAMPLED.0,
            samples: 1,
        }
    );

    let bp = &plan.buffer_placements[0].1;
    assert_eq!(bp.heap, 1);
    assert_eq!(bp.size, 128);
    assert_eq!(
        bp.compatibility,
        AliasCompatibilityClass {
            kind: AliasResourceKind::Buffer,
            format: Format::Unknown,
            usage_bits: BufferUsage::COPY_DST.0,
            samples: 1,
        }
    );
}

/// Deferred-style GBuffer stress test.
///
/// Simulated frame: depth prepass → GBuffer → lighting → postprocess → present.
///
///   depth       (passes 0..3)
///   albedo      (passes 1..2)
///   normal      (passes 1..2)
///   hdr_accum   (passes 2..4)
///   postprocess (passes 3..4)
///
/// Expected: depth and hdr_accum are different formats and cannot alias.
/// albedo + postprocess (same format, non-overlapping: 1..2 vs 3..4) → same slot.
/// normal can alias with postprocess if its lifetime ends before postprocess begins.
#[test]
fn gbuffer_stress_test_achieves_aliasing_savings() {
    let usage = ImageUsage::RENDER_TARGET | ImageUsage::SAMPLED;

    let mut depth = depth_image(ImageHandle(0));
    depth.first_use = 0;
    depth.last_use = 3;

    let albedo = VirtualImage {
        handle: ImageHandle(1),
        desc: ImageDesc {
            extent: Extent3d {
                width: 1920,
                height: 1080,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage,
            ..desc_defaults()
        },
        imported: false,
        first_use: 1,
        last_use: 2,
    };

    let normal = VirtualImage {
        handle: ImageHandle(2),
        desc: ImageDesc {
            extent: Extent3d {
                width: 1920,
                height: 1080,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage,
            ..desc_defaults()
        },
        imported: false,
        first_use: 1,
        last_use: 2,
    };

    let mut hdr_accum = hdr_image(ImageHandle(3));
    hdr_accum.first_use = 2;
    hdr_accum.last_use = 4;

    let postprocess = VirtualImage {
        handle: ImageHandle(4),
        desc: ImageDesc {
            extent: Extent3d {
                width: 1920,
                height: 1080,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage,
            ..desc_defaults()
        },
        imported: false,
        first_use: 3,
        last_use: 4,
    };

    let images = vec![depth, albedo, normal, hdr_accum, postprocess];
    let plan = build_alias_plan(&images, &[]);

    // Total image count = 5; slot count must be less (aliasing occurred).
    assert!(
        plan.image_slot_count < 5,
        "expected aliasing to reduce slot count below 5, got {}",
        plan.image_slot_count
    );

    // Savings must be positive.
    assert!(
        plan.image_savings_bytes > 0,
        "expected positive aliasing savings, got 0"
    );

    // The total aliased size must be <= the sum of individual resource sizes.
    let individual_total: u64 = images.iter().map(|i| image_size(i.desc)).sum();
    let aliased_total: u64 = plan.image_slot_sizes.iter().sum();
    assert!(aliased_total <= individual_total);
    assert_eq!(individual_total - aliased_total, plan.image_savings_bytes);
}

#[test]
fn slot_sizes_reflect_largest_resource_in_slot() {
    // Two images in the same compatibility class with non-overlapping lifetimes.
    let make = |handle: u64, w: u32, first: u32, last: u32| VirtualImage {
        handle: ImageHandle(handle),
        desc: ImageDesc {
            extent: Extent3d {
                width: w,
                height: w,
                depth: 1,
            },
            mip_levels: 1,
            layers: 1,
            samples: 1,
            format: Format::Rgba8Unorm,
            usage: ImageUsage::RENDER_TARGET,
            ..desc_defaults()
        },
        imported: false,
        first_use: first,
        last_use: last,
    };

    let small = make(0, 64, 0, 1); // 64*64*4 = 16 384 bytes
    let large = make(1, 128, 2, 3); // 128*128*4 = 65 536 bytes

    let plan = build_alias_plan(&[small, large], &[]);
    assert_eq!(
        plan.image_slot_count, 1,
        "non-overlapping images should share a slot"
    );
    assert_eq!(
        plan.image_slot_sizes[0], 65_536,
        "slot size = max of residents"
    );
    assert_eq!(
        plan.image_savings_bytes, 16_384,
        "savings = small image size"
    );
}

#[test]
fn image_size_uses_bc_block_bytes() {
    let mut desc = desc_defaults();
    desc.extent = Extent3d {
        width: 4,
        height: 4,
        depth: 1,
    };
    desc.format = Format::Bc4Unorm;
    assert_eq!(image_size(desc), 8);

    desc.extent.width = 5;
    assert_eq!(image_size(desc), 16);

    desc.extent.height = 9;
    desc.layers = 2;
    assert_eq!(image_size(desc), 96);
}
