use crate::MaterialId;

/// Backend/runtime capabilities for the centralized GPU material table.
///
/// The fast path stores all material parameters in a GPU-resident table and
/// references textures/samplers through bindless indices. Fallbacks are explicit
/// so runtime diagnostics can report when the renderer is not on the intended path.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MaterialTableCaps {
    pub storage_buffers: bool,
    pub bindless_resources: bool,
    pub partial_buffer_updates: bool,
}

impl Default for MaterialTableCaps {
    fn default() -> Self {
        Self {
            storage_buffers: true,
            bindless_resources: false,
            partial_buffer_updates: true,
        }
    }
}

impl MaterialTableCaps {
    pub fn from_caps(caps: &sturdy_engine_core::Caps) -> Self {
        Self {
            storage_buffers: true,
            bindless_resources: caps.supports_bindless,
            partial_buffer_updates: true,
        }
    }
}

/// Runtime policy for material table allocation and upload.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MaterialTableSettings {
    /// Number of material slots to reserve up-front.
    pub initial_capacity: u32,
    /// Maximum material slots before creation/update should be rejected by users
    /// of this planning layer.
    pub max_materials: u32,
    /// Size of one packed GPU material record in bytes.
    pub material_stride_bytes: u32,
    /// Merge dirty material updates separated by at most this many clean slots.
    pub dirty_range_merge_gap: u32,
}

impl Default for MaterialTableSettings {
    fn default() -> Self {
        Self {
            initial_capacity: 1024,
            max_materials: 1_000_000,
            material_stride_bytes: 256,
            dirty_range_merge_gap: 4,
        }
    }
}

/// Selected material table path and allocation parameters.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MaterialTablePlan {
    pub fast_path: bool,
    pub capacity: u32,
    pub max_materials: u32,
    pub material_stride_bytes: u32,
    pub total_table_bytes: u64,
    pub supports_dirty_range_uploads: bool,
    pub degraded_reason: Option<String>,
}

impl MaterialTablePlan {
    pub fn plan(
        requested_materials: u32,
        caps: MaterialTableCaps,
        settings: MaterialTableSettings,
    ) -> Self {
        let mut degraded = Vec::new();
        let material_stride_bytes = settings.material_stride_bytes.max(16).next_power_of_two();
        let max_materials = settings.max_materials.max(1);
        let requested_materials = requested_materials.min(max_materials);
        let capacity = settings
            .initial_capacity
            .max(requested_materials)
            .max(1)
            .next_power_of_two()
            .min(max_materials.next_power_of_two());

        if !caps.storage_buffers {
            degraded.push(
                "storage buffers unavailable, material table must use legacy bindings".to_string(),
            );
        }
        if !caps.bindless_resources {
            degraded.push(
                "bindless resources unavailable, texture/sampler indices need fallback bindings"
                    .to_string(),
            );
        }
        if !caps.partial_buffer_updates {
            degraded.push(
                "partial buffer updates unavailable, dirty ranges require full-table uploads"
                    .to_string(),
            );
        }

        let fast_path = caps.storage_buffers && caps.bindless_resources;
        let supports_dirty_range_uploads = caps.partial_buffer_updates && caps.storage_buffers;
        let total_table_bytes = capacity as u64 * material_stride_bytes as u64;

        Self {
            fast_path,
            capacity,
            max_materials,
            material_stride_bytes,
            total_table_bytes,
            supports_dirty_range_uploads,
            degraded_reason: if degraded.is_empty() {
                None
            } else {
                Some(degraded.join("; "))
            },
        }
    }
}

/// Inclusive-start/exclusive-end range of material table slots to upload.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct MaterialTableDirtyRange {
    pub start: u32,
    pub end: u32,
}

impl MaterialTableDirtyRange {
    pub const fn new(start: u32, end: u32) -> Self {
        Self { start, end }
    }

    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub const fn is_empty(self) -> bool {
        self.start >= self.end
    }

    pub const fn byte_offset(self, material_stride_bytes: u32) -> u64 {
        self.start as u64 * material_stride_bytes as u64
    }

    pub const fn byte_len(self, material_stride_bytes: u32) -> u64 {
        self.len() as u64 * material_stride_bytes as u64
    }
}

/// Build compact upload ranges from dirty material IDs.
///
/// Invalid IDs are ignored. Duplicate IDs are collapsed. Adjacent dirty IDs are
/// merged, and small gaps can be deliberately over-uploaded to reduce update calls.
pub fn material_table_dirty_ranges(
    dirty_materials: impl IntoIterator<Item = MaterialId>,
    capacity: u32,
    merge_gap: u32,
) -> Vec<MaterialTableDirtyRange> {
    if capacity == 0 {
        return Vec::new();
    }

    let mut ids: Vec<u32> = dirty_materials
        .into_iter()
        .filter(|id| id.is_valid() && id.as_u32() < capacity)
        .map(MaterialId::as_u32)
        .collect();
    ids.sort_unstable();
    ids.dedup();

    let mut ranges = Vec::new();
    let mut iter = ids.into_iter();
    let Some(first) = iter.next() else {
        return ranges;
    };

    let mut start = first;
    let mut end = first + 1;
    for id in iter {
        if id <= end.saturating_add(merge_gap) {
            end = id + 1;
        } else {
            ranges.push(MaterialTableDirtyRange::new(start, end));
            start = id;
            end = id + 1;
        }
    }
    ranges.push(MaterialTableDirtyRange::new(start, end));
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_reports_fast_path_when_bindless_storage_table_is_available() {
        let plan = MaterialTablePlan::plan(
            1500,
            MaterialTableCaps {
                storage_buffers: true,
                bindless_resources: true,
                partial_buffer_updates: true,
            },
            MaterialTableSettings::default(),
        );

        assert!(plan.fast_path);
        assert_eq!(plan.capacity, 2048);
        assert_eq!(plan.material_stride_bytes, 256);
        assert_eq!(plan.total_table_bytes, 2048 * 256);
        assert!(plan.supports_dirty_range_uploads);
        assert_eq!(plan.degraded_reason, None);
    }

    #[test]
    fn plan_reports_bindless_and_update_fallbacks() {
        let plan = MaterialTablePlan::plan(
            16,
            MaterialTableCaps {
                storage_buffers: true,
                bindless_resources: false,
                partial_buffer_updates: false,
            },
            MaterialTableSettings {
                initial_capacity: 8,
                max_materials: 64,
                material_stride_bytes: 96,
                dirty_range_merge_gap: 0,
            },
        );

        assert!(!plan.fast_path);
        assert_eq!(plan.capacity, 16);
        assert_eq!(plan.material_stride_bytes, 128);
        assert!(!plan.supports_dirty_range_uploads);
        let reason = plan.degraded_reason.as_deref().unwrap();
        assert!(reason.contains("bindless"));
        assert!(reason.contains("partial buffer"));
    }

    #[test]
    fn dirty_ranges_are_sorted_deduped_and_merged_by_gap() {
        let ranges = material_table_dirty_ranges(
            [
                MaterialId::from_raw(9),
                MaterialId::from_raw(2),
                MaterialId::from_raw(3),
                MaterialId::from_raw(7),
                MaterialId::from_raw(7),
                MaterialId::INVALID,
                MaterialId::from_raw(99),
            ],
            16,
            1,
        );

        assert_eq!(
            ranges,
            vec![
                MaterialTableDirtyRange::new(2, 4),
                MaterialTableDirtyRange::new(7, 10),
            ]
        );
        assert_eq!(ranges[1].byte_offset(256), 7 * 256);
        assert_eq!(ranges[1].byte_len(256), 3 * 256);
    }
}
