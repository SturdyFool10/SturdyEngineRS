use crate::ecs::Transform;

use super::{GpuObjectId, PreviousTransform, RenderBounds, RenderObjectState};

/// Compact per-object transform source for GPU matrix generation.
///
/// This is intentionally TRS data, not a precomputed matrix. A compute pass can
/// expand it into current world, previous world, normal matrices, and render
/// bounds. Keeping this as the uploaded source avoids CPU materializing every
/// matrix for every renderable every frame.
///
/// Layout (112 bytes, 16-byte aligned throughout):
/// ```text
///  0..12  translation      [f32; 3]
/// 12..16  parent_index     u32
/// 16..32  rotation         [f32; 4]
/// 32..44  scale            [f32; 3]
/// 44..48  flags            u32
/// 48..60  previous_translation [f32; 3]
/// 60..64  local_sphere_radius  f32       ← local bounding sphere radius
/// 64..80  previous_rotation    [f32; 4]
/// 80..92  previous_scale       [f32; 3]
/// 92..96  _pad1                u32
/// 96..108 local_sphere_center  [f32; 3]  ← local bounding sphere center
/// 108..112 _pad2               u32
/// ```
#[repr(C)]
#[derive(Copy, Clone, Debug, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GpuTransformSourceData {
    pub translation: [f32; 3],
    pub parent_index: u32,
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub flags: u32,
    pub previous_translation: [f32; 3],
    pub local_sphere_radius: f32,
    pub previous_rotation: [f32; 4],
    pub previous_scale: [f32; 3],
    pub _pad1: u32,
    pub local_sphere_center: [f32; 3],
    pub _pad2: u32,
}

impl GpuTransformSourceData {
    pub const FLAG_HAS_PREVIOUS: u32 = 1 << 0;
    pub const FLAG_HAS_PARENT: u32 = 1 << 1;
    pub const INVALID_PARENT: u32 = u32::MAX;

    pub fn from_transforms(
        transform: &Transform,
        previous: Option<&PreviousTransform>,
        parent_index: Option<u32>,
        bounds: Option<&RenderBounds>,
    ) -> Self {
        let previous_transform = previous
            .map(PreviousTransform::to_transform)
            .unwrap_or_else(|| transform.clone());
        let mut flags = 0;
        if previous.is_some() {
            flags |= Self::FLAG_HAS_PREVIOUS;
        }
        if parent_index.is_some() {
            flags |= Self::FLAG_HAS_PARENT;
        }

        let (local_sphere_center, local_sphere_radius) = bounds
            .map(|b| (b.local_sphere.center.to_array(), b.local_sphere.radius))
            .unwrap_or(([0.0, 0.0, 0.0], 0.0));

        Self {
            translation: transform.position.to_array(),
            parent_index: parent_index.unwrap_or(Self::INVALID_PARENT),
            rotation: transform.rotation.to_array(),
            scale: transform.scale.to_array(),
            flags,
            previous_translation: previous_transform.position.to_array(),
            local_sphere_radius,
            previous_rotation: previous_transform.rotation.to_array(),
            previous_scale: previous_transform.scale.to_array(),
            _pad1: 0,
            local_sphere_center,
            _pad2: 0,
        }
    }
}

/// CPU-built compact transform source buffer and stable object-slot lookup.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenderWorldGpuTransformSourceData {
    pub transforms: Vec<GpuTransformSourceData>,
    pub object_slots: Vec<(GpuObjectId, u32)>,
}

impl RenderWorldGpuTransformSourceData {
    pub fn from_states(states: &[RenderObjectState]) -> Self {
        let mut states: Vec<_> = states
            .iter()
            .filter(|state| state.mesh.is_some())
            .cloned()
            .collect();
        states.sort_by_key(|state| state.object);

        let mut transforms = Vec::with_capacity(states.len());
        let mut object_slots = Vec::with_capacity(states.len());
        for (slot, state) in states.iter().enumerate() {
            let transform = state.transform.as_ref().unwrap_or(&Transform::IDENTITY);
            transforms.push(GpuTransformSourceData::from_transforms(
                transform,
                state.previous_transform.as_ref(),
                None,
                state.bounds.as_ref(),
            ));
            object_slots.push((state.object, slot as u32));
        }

        Self {
            transforms,
            object_slots,
        }
    }

    pub fn len(&self) -> usize {
        self.transforms.len()
    }

    pub fn is_empty(&self) -> bool {
        self.transforms.is_empty()
    }

    pub fn slot_for_object(&self, object: GpuObjectId) -> Option<u32> {
        self.object_slots
            .iter()
            .find_map(|(candidate, slot)| (*candidate == object).then_some(*slot))
    }
}

/// Inclusive-start/exclusive-end dirty transform upload range.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct GpuTransformDirtyRange {
    pub start: u32,
    pub end: u32,
}

impl GpuTransformDirtyRange {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub const fn byte_offset(self) -> u64 {
        self.start as u64 * std::mem::size_of::<GpuTransformSourceData>() as u64
    }

    pub const fn byte_len(self) -> u64 {
        self.len() as u64 * std::mem::size_of::<GpuTransformSourceData>() as u64
    }
}

pub fn gpu_transform_dirty_ranges(
    dirty_slots: impl IntoIterator<Item = u32>,
    capacity: u32,
    merge_gap: u32,
) -> Vec<GpuTransformDirtyRange> {
    if capacity == 0 {
        return Vec::new();
    }

    let mut slots: Vec<u32> = dirty_slots
        .into_iter()
        .filter(|slot| *slot < capacity)
        .collect();
    slots.sort_unstable();
    slots.dedup();

    let mut ranges = Vec::new();
    let mut iter = slots.into_iter();
    let Some(first) = iter.next() else {
        return ranges;
    };

    let mut start = first;
    let mut end = first + 1;
    for slot in iter {
        if slot <= end.saturating_add(merge_gap) {
            end = slot + 1;
        } else {
            ranges.push(GpuTransformDirtyRange::new(start, end));
            start = slot;
            end = slot + 1;
        }
    }
    ranges.push(GpuTransformDirtyRange::new(start, end));
    ranges
}

#[cfg(test)]
mod tests {
    use glam::{Quat, Vec3};

    use crate::{BoundingSphere, MeshId, RenderBounds, RenderDirtyFlags, RenderMesh};

    use super::*;

    #[test]
    fn transform_source_packs_current_and_previous_trs() {
        let current = Transform::from_trs(
            Vec3::new(1.0, 2.0, 3.0),
            Quat::from_rotation_y(0.5),
            Vec3::splat(2.0),
        );
        let previous = PreviousTransform::from_transform(&Transform::from_position(Vec3::X));
        let packed =
            GpuTransformSourceData::from_transforms(&current, Some(&previous), Some(7), None);

        assert_eq!(packed.translation, [1.0, 2.0, 3.0]);
        assert_eq!(packed.scale, [2.0, 2.0, 2.0]);
        assert_eq!(packed.previous_translation, [1.0, 0.0, 0.0]);
        assert_eq!(packed.parent_index, 7);
        assert!(packed.flags & GpuTransformSourceData::FLAG_HAS_PREVIOUS != 0);
        assert!(packed.flags & GpuTransformSourceData::FLAG_HAS_PARENT != 0);
    }

    #[test]
    fn transform_source_data_filters_non_renderable_and_orders_by_object() {
        let object_a = GpuObjectId::from_raw(5);
        let object_b = GpuObjectId::from_raw(2);
        let mut a = RenderObjectState::new(object_a);
        a.mesh = Some(RenderMesh::new(MeshId::from_raw(1)));
        a.transform = Some(Transform::from_position(Vec3::new(5.0, 0.0, 0.0)));
        a.bounds = Some(RenderBounds::from_sphere(BoundingSphere::EMPTY));
        a.dirty = RenderDirtyFlags::TRANSFORM;

        let mut b = RenderObjectState::new(object_b);
        b.mesh = Some(RenderMesh::new(MeshId::from_raw(1)));
        b.transform = Some(Transform::from_position(Vec3::new(2.0, 0.0, 0.0)));

        let non_renderable = RenderObjectState::new(GpuObjectId::from_raw(9));
        let data = RenderWorldGpuTransformSourceData::from_states(&[a, non_renderable, b]);

        assert_eq!(data.len(), 2);
        assert_eq!(data.slot_for_object(object_b), Some(0));
        assert_eq!(data.slot_for_object(object_a), Some(1));
        assert_eq!(data.transforms[0].translation, [2.0, 0.0, 0.0]);
        assert_eq!(data.transforms[1].translation, [5.0, 0.0, 0.0]);
    }

    #[test]
    fn dirty_ranges_sort_dedup_filter_and_merge() {
        let ranges = gpu_transform_dirty_ranges([7, 2, 3, 7, 10, 99], 16, 1);

        assert_eq!(
            ranges,
            vec![
                GpuTransformDirtyRange::new(2, 4),
                GpuTransformDirtyRange::new(7, 8),
                GpuTransformDirtyRange::new(10, 11),
            ]
        );
        assert_eq!(
            ranges[0].byte_len(),
            2 * std::mem::size_of::<GpuTransformSourceData>() as u64
        );
    }
}
