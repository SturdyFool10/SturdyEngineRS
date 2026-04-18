use std::collections::HashMap;

use crate::{BufferHandle, BufferUsage, Format, ImageDesc, ImageHandle};

use super::{VirtualBuffer, VirtualImage};

/// Lifetime and aliasing slot assignment for one transient resource.
///
/// Two resources with non-overlapping `[first_pass, last_pass]` ranges share
/// the same `alias_slot`, meaning they can occupy the same physical memory.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceLifetime {
    pub first_pass: u32,
    pub last_pass: u32,
    /// Index into the pool of aliasable memory slots. Resources in the same
    /// slot have non-overlapping lifetimes and can share physical memory.
    pub alias_slot: u32,
}

#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub enum AliasResourceKind {
    Image,
    Buffer,
}

/// Resources in the same compatibility class can share alias slots.
///
/// Different classes require different memory types or tiling layouts and
/// therefore cannot be aliased even if their lifetimes do not overlap.
#[derive(Copy, Clone, Debug, Eq, Hash, PartialEq)]
pub struct AliasCompatibilityClass {
    pub kind: AliasResourceKind,
    pub format: Format,
    pub usage_bits: u32,
    pub samples: u8,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AliasPlacement {
    pub heap: u32,
    pub block: u32,
    pub offset: u64,
    pub size: u64,
    pub alignment: u64,
    pub lifetime: ResourceLifetime,
    pub compatibility: AliasCompatibilityClass,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct AliasPlan {
    pub transient_image_count: usize,
    pub transient_buffer_count: usize,
    /// Per-transient-image lifetime and alias-slot assignment.
    pub image_lifetimes: Vec<(ImageHandle, ResourceLifetime)>,
    /// Per-transient-buffer lifetime and alias-slot assignment.
    pub buffer_lifetimes: Vec<(BufferHandle, ResourceLifetime)>,
    /// Concrete image alias placements keyed by image handle.
    pub image_placements: Vec<(ImageHandle, AliasPlacement)>,
    /// Concrete buffer alias placements keyed by buffer handle.
    pub buffer_placements: Vec<(BufferHandle, AliasPlacement)>,
    /// How many distinct memory slots images need.
    pub image_slot_count: usize,
    /// How many distinct memory slots buffers need.
    pub buffer_slot_count: usize,
    /// Maximum resource size per image alias slot (indexed by alias_slot).
    pub image_slot_sizes: Vec<u64>,
    /// Maximum resource size per buffer alias slot (indexed by alias_slot).
    pub buffer_slot_sizes: Vec<u64>,
    /// Bytes saved vs. allocating each transient image independently.
    pub image_savings_bytes: u64,
    /// Bytes saved vs. allocating each transient buffer independently.
    pub buffer_savings_bytes: u64,
}

impl AliasPlan {
    /// Total bytes saved by aliasing (images + buffers).
    pub fn total_savings_bytes(&self) -> u64 {
        self.image_savings_bytes + self.buffer_savings_bytes
    }
}

/// Greedy interval-graph-coloring alias plan.
///
/// Resources are grouped by `AliasCompatibilityClass` first.  Within each
/// group, resources are sorted by first_pass and assigned to the first alias
/// slot whose last occupant ended before this resource starts.  This minimises
/// the number of distinct memory slots needed inside each coarse resource class.
pub(super) fn build_alias_plan(images: &[VirtualImage], buffers: &[VirtualBuffer]) -> AliasPlan {
    let transient_images: Vec<&VirtualImage> = images.iter().filter(|i| !i.imported).collect();
    let transient_buffers: Vec<&VirtualBuffer> = buffers.iter().filter(|b| !b.imported).collect();

    let (image_lifetimes, image_slot_count) = pack_by_class(transient_images.iter().map(|img| {
        (
            img.handle,
            img.first_use,
            img.last_use,
            image_compatibility(img.desc),
        )
    }));
    let (buffer_lifetimes, buffer_slot_count) =
        pack_by_class(transient_buffers.iter().map(|buf| {
            (
                buf.handle,
                buf.first_use,
                buf.last_use,
                buffer_compatibility(buf.desc.usage),
            )
        }));

    // Compute per-slot sizes (max resource size in each slot).
    let mut image_slot_sizes = vec![0u64; image_slot_count];
    for (handle, lifetime) in &image_lifetimes {
        if let Some(img) = transient_images.iter().find(|i| i.handle == *handle) {
            let slot = lifetime.alias_slot as usize;
            image_slot_sizes[slot] = image_slot_sizes[slot].max(image_size(img.desc));
        }
    }
    let mut buffer_slot_sizes = vec![0u64; buffer_slot_count];
    for (handle, lifetime) in &buffer_lifetimes {
        if let Some(buf) = transient_buffers.iter().find(|b| b.handle == *handle) {
            let slot = lifetime.alias_slot as usize;
            buffer_slot_sizes[slot] = buffer_slot_sizes[slot].max(buf.desc.size);
        }
    }

    // Savings = (sum of individual sizes) − (sum of slot sizes).
    let image_individual_total: u64 = transient_images.iter().map(|i| image_size(i.desc)).sum();
    let image_aliased_total: u64 = image_slot_sizes.iter().sum();
    let image_savings_bytes = image_individual_total.saturating_sub(image_aliased_total);

    let buffer_individual_total: u64 = transient_buffers.iter().map(|b| b.desc.size).sum();
    let buffer_aliased_total: u64 = buffer_slot_sizes.iter().sum();
    let buffer_savings_bytes = buffer_individual_total.saturating_sub(buffer_aliased_total);

    let image_placements = image_lifetimes
        .iter()
        .filter_map(|(handle, lifetime)| {
            let image = transient_images.iter().find(|i| i.handle == *handle)?;
            Some((
                *handle,
                image_placement((**image).clone(), lifetime.clone()),
            ))
        })
        .collect();
    let buffer_placements = buffer_lifetimes
        .iter()
        .filter_map(|(handle, lifetime)| {
            let buffer = transient_buffers.iter().find(|b| b.handle == *handle)?;
            Some((
                *handle,
                buffer_placement((**buffer).clone(), lifetime.clone()),
            ))
        })
        .collect();

    AliasPlan {
        transient_image_count: transient_images.len(),
        transient_buffer_count: transient_buffers.len(),
        image_lifetimes,
        buffer_lifetimes,
        image_placements,
        buffer_placements,
        image_slot_count,
        buffer_slot_count,
        image_slot_sizes,
        buffer_slot_sizes,
        image_savings_bytes,
        buffer_savings_bytes,
    }
}

/// Group resources by compatibility class, then pack lifetimes within each
/// group independently.  Slot IDs are globally unique across all groups.
fn pack_by_class<H: Copy>(
    resources: impl Iterator<Item = (H, u32, u32, AliasCompatibilityClass)>,
) -> (Vec<(H, ResourceLifetime)>, usize) {
    let mut groups: HashMap<AliasCompatibilityClass, Vec<(H, u32, u32)>> = HashMap::new();
    for (handle, first, last, class) in resources {
        groups.entry(class).or_default().push((handle, first, last));
    }

    let mut lifetimes = Vec::new();
    let mut slot_offset = 0usize;

    // Sort groups for deterministic slot assignment.
    let mut group_keys: Vec<AliasCompatibilityClass> = groups.keys().copied().collect();
    group_keys.sort_by_key(|c| (c.kind as u8, c.usage_bits, c.samples, c.format as u8));

    for key in group_keys {
        let items = &groups[&key];
        let (group_lifetimes, group_slots) =
            pack_lifetimes(items.iter().copied(), slot_offset as u32);
        lifetimes.extend(group_lifetimes);
        slot_offset += group_slots;
    }

    (lifetimes, slot_offset)
}

/// Assign alias slots to resources using greedy interval coloring.
///
/// `slot_offset` is added to every assigned slot index so that slot IDs are
/// globally unique when multiple compatibility-class groups are combined.
///
/// Returns `(lifetimes_with_slots, number_of_new_slots)`.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) fn pack_lifetimes<H: Copy>(
    resources: impl Iterator<Item = (H, u32, u32)>,
    slot_offset: u32,
) -> (Vec<(H, ResourceLifetime)>, usize) {
    let mut items: Vec<(H, u32, u32)> = resources.collect();
    items.sort_unstable_by_key(|(_, first, _)| *first);

    let mut slot_last_use: Vec<u32> = Vec::new();
    let mut result = Vec::with_capacity(items.len());

    for (handle, first, last) in items {
        let slot = slot_last_use
            .iter()
            .position(|&end| end < first)
            .unwrap_or_else(|| {
                slot_last_use.push(0);
                slot_last_use.len() - 1
            });
        slot_last_use[slot] = last;
        result.push((
            handle,
            ResourceLifetime {
                first_pass: first,
                last_pass: last,
                alias_slot: slot_offset + slot as u32,
            },
        ));
    }

    let slot_count = slot_last_use.len();
    (result, slot_count)
}

fn image_placement(image: VirtualImage, lifetime: ResourceLifetime) -> AliasPlacement {
    AliasPlacement {
        heap: 0,
        block: lifetime.alias_slot,
        offset: 0,
        size: image_size(image.desc),
        alignment: 256,
        lifetime,
        compatibility: image_compatibility(image.desc),
    }
}

fn buffer_placement(buffer: VirtualBuffer, lifetime: ResourceLifetime) -> AliasPlacement {
    AliasPlacement {
        heap: 1,
        block: lifetime.alias_slot,
        offset: 0,
        size: buffer.desc.size,
        alignment: 256,
        lifetime,
        compatibility: buffer_compatibility(buffer.desc.usage),
    }
}

fn image_compatibility(desc: ImageDesc) -> AliasCompatibilityClass {
    AliasCompatibilityClass {
        kind: AliasResourceKind::Image,
        format: desc.format,
        usage_bits: desc.usage.0,
        samples: desc.samples,
    }
}

fn buffer_compatibility(usage: BufferUsage) -> AliasCompatibilityClass {
    AliasCompatibilityClass {
        kind: AliasResourceKind::Buffer,
        format: Format::Unknown,
        usage_bits: usage.0,
        samples: 1,
    }
}

fn image_size(desc: ImageDesc) -> u64 {
    let texel_size = format_texel_size(desc.format);
    let mut total_texels = 0u64;
    for mip in 0..desc.mip_levels {
        total_texels += mip_extent(desc.extent.width, mip as u32) as u64
            * mip_extent(desc.extent.height, mip as u32) as u64
            * mip_extent(desc.extent.depth, mip as u32) as u64
            * desc.layers as u64
            * desc.samples as u64;
    }
    total_texels.saturating_mul(texel_size)
}

fn format_texel_size(format: Format) -> u64 {
    match format {
        Format::Unknown => 1,
        Format::Rgba8Unorm | Format::Bgra8Unorm => 4,
        Format::Rgba16Float => 8,
        Format::Rgba32Float => 16,
        Format::Depth32Float | Format::Depth24Stencil8 => 4,
    }
}

fn mip_extent(base: u32, mip_level: u32) -> u32 {
    (base >> mip_level).max(1)
}

#[cfg(test)]
mod tests {
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
        let depth_usage = ImageUsage::DEPTH_STENCIL;

        let mut depth = depth_image(ImageHandle(0));
        depth.first_use = 0;
        depth.last_use = 3;

        let mut albedo = VirtualImage {
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
        let _ = albedo; // bind

        let mut normal = VirtualImage {
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

        let mut postprocess = VirtualImage {
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
}
