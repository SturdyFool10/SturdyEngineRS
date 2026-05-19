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
    let mut total_bytes = 0u64;
    for mip in 0..desc.mip_levels {
        let width = mip_extent(desc.extent.width, mip as u32) as u64;
        let height = mip_extent(desc.extent.height, mip as u32) as u64;
        let depth = mip_extent(desc.extent.depth, mip as u32) as u64;
        let mip_bytes = if desc.format.is_block_compressed() {
            width
                .div_ceil(4)
                .saturating_mul(height.div_ceil(4))
                .saturating_mul(depth)
                .saturating_mul(desc.layers as u64)
                .saturating_mul(desc.samples as u64)
                .saturating_mul(desc.format.bc_block_bytes())
        } else {
            width
                .saturating_mul(height)
                .saturating_mul(depth)
                .saturating_mul(desc.layers as u64)
                .saturating_mul(desc.samples as u64)
                .saturating_mul(format_texel_size(desc.format))
        };
        total_bytes = total_bytes.saturating_add(mip_bytes);
    }
    total_bytes
}

fn format_texel_size(format: Format) -> u64 {
    match format {
        Format::Unknown => 1,
        Format::Rgba8Unorm | Format::Bgra8Unorm => 4,
        Format::R8Unorm => 1,
        Format::Rg8Unorm => 2,
        Format::Rgba16Float => 8,
        Format::Rgba32Float => 16,
        Format::Depth32Float | Format::Depth24Stencil8 => 4,
        // BC formats handled above.
        // YCbCr planar: 1 byte/texel for the luma plane (chroma plane is implicit).
        Format::G8_B8R8_2PLANE_420_UNORM => 1,
        Format::Bc3Unorm
        | Format::Bc3UnormSrgb
        | Format::Bc4Unorm
        | Format::Bc5Unorm
        | Format::Bc7Unorm
        | Format::Bc7UnormSrgb
        | Format::Bc6hUfloat => unreachable!("BC formats are sized by image_size"),
    }
}

fn mip_extent(base: u32, mip_level: u32) -> u32 {
    (base >> mip_level).max(1)
}

#[cfg(test)]
#[path = "alias_plan_tests.rs"]
mod tests;
