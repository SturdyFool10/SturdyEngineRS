// BVH-based light culling.
//
// Builds a flat AABB BVH over all point and spot lights. During the deferred
// lighting pass, each fragment traverses the BVH to find the O(log N) lights
// whose influence volumes contain that fragment, then evaluates only those.
//
// This replaces the O(N) linear light loop with O(log N) traversal, enabling
// scenes with thousands of lights at a cost proportional to lights actually
// visible at each pixel — not the total light count.
//
// Build cost: O(N log N) on the CPU when lights change. For static light sets,
// the BVH is built once; for dynamic lights it rebuilds each frame the light
// list changes (tracked via `LightBvhBuilder::dirty`).
//
// Roadmap: Track 6f — BVH-based light assignment.

use glam::Vec3;

// ── GPU node layout ───────────────────────────────────────────────────────────

/// One node of the flat BVH as uploaded to the GPU (32 bytes).
///
/// Internal node: `right_or_leaf & LEAF_FLAG == 0`.
///   `left_or_light` = left child index
///   `right_or_leaf` = right child index
///
/// Leaf node: `right_or_leaf & LEAF_FLAG != 0`.
///   `left_or_light` = index into the `lights` StructuredBuffer
///   `right_or_leaf` = LEAF_FLAG (ignored otherwise)
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuBvhNode {
    pub aabb_min: [f32; 3],
    /// Internal: left child index. Leaf: light array index.
    pub left_or_light: u32,
    pub aabb_max: [f32; 3],
    /// Internal: right child index. Leaf: `LEAF_FLAG`.
    pub right_or_leaf: u32,
}

/// Bit flag in `right_or_leaf` that identifies a leaf node.
pub const LEAF_FLAG: u32 = 0x8000_0000;

/// Sentinel value: empty BVH (no lights).
pub const BVH_EMPTY: u32 = u32::MAX;

// ── CPU build ─────────────────────────────────────────────────────────────────

/// Builds a flat AABB BVH from a list of lights.
///
/// Returns a `Vec<GpuBvhNode>` ready to upload as a `StructuredBuffer<GpuBvhNode>`.
/// The root is always at index 0. Returns an empty Vec when there are no lights.
///
/// `entries` is a list of `(aabb_min, aabb_max, light_index)` — one entry per
/// point or spot light, where the AABB encloses the light's influence volume.
pub fn build_bvh(entries: &mut Vec<(Vec3, Vec3, u32)>) -> Vec<GpuBvhNode> {
    if entries.is_empty() {
        return Vec::new();
    }
    let mut nodes: Vec<GpuBvhNode> = Vec::new();
    build_recursive(entries, 0, entries.len(), &mut nodes);
    nodes
}

/// Compute the AABB entries for a list of point lights.
pub fn point_light_entries(
    lights: &[crate::scene::PointLight],
    light_array_offset: u32,
) -> Vec<(Vec3, Vec3, u32)> {
    lights.iter().enumerate().map(|(i, l)| {
        let r = Vec3::splat(l.range);
        let c = l.position;
        (c - r, c + r, light_array_offset + i as u32)
    }).collect()
}

/// Compute the AABB entries for a list of spot lights.
pub fn spot_light_entries(
    lights: &[crate::scene::SpotLight],
    light_array_offset: u32,
) -> Vec<(Vec3, Vec3, u32)> {
    lights.iter().enumerate().map(|(i, l)| {
        // Conservative AABB: sphere enclosing the spotlight cone.
        // A tighter cone AABB exists but the sphere is correct and cheaper to compute.
        let r = Vec3::splat(l.range);
        let c = l.position;
        (c - r, c + r, light_array_offset + i as u32)
    }).collect()
}

fn build_recursive(
    entries: &mut Vec<(Vec3, Vec3, u32)>,
    start: usize,
    end: usize,
    nodes: &mut Vec<GpuBvhNode>,
) -> u32 {
    let node_idx = nodes.len() as u32;
    nodes.push(bytemuck::Zeroable::zeroed()); // placeholder, filled below

    if end - start == 1 {
        // Leaf.
        let (mn, mx, light_idx) = entries[start];
        nodes[node_idx as usize] = GpuBvhNode {
            aabb_min: mn.to_array(),
            left_or_light: light_idx,
            aabb_max: mx.to_array(),
            right_or_leaf: LEAF_FLAG,
        };
        return node_idx;
    }

    // Compute the combined AABB for this range.
    let mut combined_min = Vec3::splat(f32::INFINITY);
    let mut combined_max = Vec3::splat(f32::NEG_INFINITY);
    for (mn, mx, _) in &entries[start..end] {
        combined_min = combined_min.min(*mn);
        combined_max = combined_max.max(*mx);
    }

    // Split along the longest axis at the spatial median of centroids.
    let extent = combined_max - combined_min;
    let axis = if extent.x >= extent.y && extent.x >= extent.z { 0 }
               else if extent.y >= extent.z { 1 }
               else { 2 };

    entries[start..end].sort_unstable_by(|(a_min, a_max, _), (b_min, b_max, _)| {
        let a_c = (a_min[axis] + a_max[axis]) * 0.5;
        let b_c = (b_min[axis] + b_max[axis]) * 0.5;
        a_c.partial_cmp(&b_c).unwrap_or(std::cmp::Ordering::Equal)
    });

    let mid = start + (end - start) / 2;
    let left  = build_recursive(entries, start, mid, nodes);
    let right = build_recursive(entries, mid, end, nodes);

    nodes[node_idx as usize] = GpuBvhNode {
        aabb_min: combined_min.to_array(),
        left_or_light: left,
        aabb_max: combined_max.to_array(),
        right_or_leaf: right,
    };
    node_idx
}

// ── LightBvhBuilder ───────────────────────────────────────────────────────────

/// Tracks light changes and manages GPU BVH buffer upload.
pub struct LightBvhBuilder {
    pub dirty: bool,
    /// Cached GPU BVH nodes, ready to upload.
    pub nodes: Vec<GpuBvhNode>,
    /// GPU buffer containing the flat BVH (StructuredBuffer<GpuBvhNode>).
    pub gpu_buffer: Option<crate::Buffer>,
    /// Index of the first point light in the combined `lights` array.
    /// Directional = 0; point lights start at 1.
    pub point_light_offset: u32,
    pub spot_light_offset: u32,
}

impl LightBvhBuilder {
    pub fn new() -> Self {
        Self {
            dirty: true,
            nodes: Vec::new(),
            gpu_buffer: None,
            point_light_offset: 0,
            spot_light_offset: 0,
        }
    }

    /// Rebuild the BVH from the current point and spot light lists.
    ///
    /// `point_offset` and `spot_offset` are the indices into the `lights`
    /// StructuredBuffer where point and spot lights start (after the directional).
    pub fn rebuild(
        &mut self,
        engine: &crate::Engine,
        point_lights: &[crate::scene::PointLight],
        spot_lights:  &[crate::scene::SpotLight],
        point_offset: u32,
        spot_offset:  u32,
    ) -> crate::Result<()> {
        self.point_light_offset = point_offset;
        self.spot_light_offset  = spot_offset;

        let mut entries = point_light_entries(point_lights, point_offset);
        entries.extend(spot_light_entries(spot_lights, spot_offset));

        self.nodes = build_bvh(&mut entries);

        // Upload to GPU (create or resize buffer as needed).
        let byte_size = (self.nodes.len() * std::mem::size_of::<GpuBvhNode>()).max(32) as u64;
        let needs_new_buf = self.gpu_buffer
            .as_ref()
            .map(|b| b.desc().size < byte_size)
            .unwrap_or(true);

        if needs_new_buf {
            self.gpu_buffer = Some(engine.create_buffer(crate::BufferDesc {
                size: byte_size,
                usage: crate::BufferUsage::STORAGE | crate::BufferUsage::COPY_DST,
            })?);
        }

        if !self.nodes.is_empty() {
            let buf = self.gpu_buffer.as_ref().unwrap();
            buf.write(0, bytemuck::cast_slice(&self.nodes))?;
        }

        self.dirty = false;
        Ok(())
    }
}
