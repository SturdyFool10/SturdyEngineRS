/// Scene-wide GPU resource table categories.
///
/// These tables complement the material table: materials reference textures and
/// samplers, while the renderer indexes lights, decals, probes, and GI cache
/// entries through stable scene-wide table slots.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub enum SceneResourceTableKind {
    Lights,
    Decals,
    ReflectionProbes,
    IrradianceProbes,
    SurfaceCacheCards,
}

impl SceneResourceTableKind {
    pub const fn default_stride_bytes(self) -> u32 {
        match self {
            Self::Lights => 128,
            Self::Decals => 128,
            Self::ReflectionProbes => 96,
            Self::IrradianceProbes => 64,
            Self::SurfaceCacheCards => 64,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Lights => "lights",
            Self::Decals => "decals",
            Self::ReflectionProbes => "reflection_probes",
            Self::IrradianceProbes => "irradiance_probes",
            Self::SurfaceCacheCards => "surface_cache_cards",
        }
    }
}

/// Stable slot in a scene-wide GPU resource table.
#[repr(C)]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd)]
pub struct SceneResourceId {
    pub table: SceneResourceTableKind,
    pub index: u32,
}

impl SceneResourceId {
    pub const INVALID_INDEX: u32 = u32::MAX;

    pub const fn new(table: SceneResourceTableKind, index: u32) -> Self {
        Self { table, index }
    }

    pub const fn invalid(table: SceneResourceTableKind) -> Self {
        Self {
            table,
            index: Self::INVALID_INDEX,
        }
    }

    pub const fn is_valid(self) -> bool {
        self.index != Self::INVALID_INDEX
    }
}

/// Capabilities relevant to scene-wide resource tables.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SceneResourceTableCaps {
    pub storage_buffers: bool,
    pub bindless_resources: bool,
    pub partial_buffer_updates: bool,
}

impl Default for SceneResourceTableCaps {
    fn default() -> Self {
        Self {
            storage_buffers: true,
            bindless_resources: false,
            partial_buffer_updates: true,
        }
    }
}

impl SceneResourceTableCaps {
    pub fn from_caps(caps: &sturdy_engine_core::Caps) -> Self {
        Self {
            storage_buffers: true,
            bindless_resources: caps.supports_bindless,
            partial_buffer_updates: true,
        }
    }
}

/// Settings for one scene-wide resource table.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SceneResourceTableSettings {
    pub kind: SceneResourceTableKind,
    pub initial_capacity: u32,
    pub max_resources: u32,
    pub stride_bytes: u32,
    pub dirty_range_merge_gap: u32,
}

impl SceneResourceTableSettings {
    pub const fn new(kind: SceneResourceTableKind) -> Self {
        Self {
            kind,
            initial_capacity: 256,
            max_resources: 1_000_000,
            stride_bytes: kind.default_stride_bytes(),
            dirty_range_merge_gap: 4,
        }
    }

    pub const fn lights() -> Self {
        Self::new(SceneResourceTableKind::Lights)
    }

    pub const fn decals() -> Self {
        Self::new(SceneResourceTableKind::Decals)
    }

    pub const fn reflection_probes() -> Self {
        Self::new(SceneResourceTableKind::ReflectionProbes)
    }

    pub const fn irradiance_probes() -> Self {
        Self::new(SceneResourceTableKind::IrradianceProbes)
    }

    pub const fn surface_cache_cards() -> Self {
        Self::new(SceneResourceTableKind::SurfaceCacheCards)
    }
}

/// Allocation/update plan for one scene-wide GPU resource table.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SceneResourceTablePlan {
    pub kind: SceneResourceTableKind,
    pub fast_path: bool,
    pub capacity: u32,
    pub max_resources: u32,
    pub stride_bytes: u32,
    pub total_table_bytes: u64,
    pub supports_dirty_range_uploads: bool,
    pub degraded_reason: Option<String>,
}

impl SceneResourceTablePlan {
    pub fn plan(
        requested_resources: u32,
        caps: SceneResourceTableCaps,
        settings: SceneResourceTableSettings,
    ) -> Self {
        let mut degraded = Vec::new();
        let stride_bytes = settings.stride_bytes.max(16).next_power_of_two();
        let max_resources = settings.max_resources.max(1);
        let requested_resources = requested_resources.min(max_resources);
        let capacity = settings
            .initial_capacity
            .max(requested_resources)
            .max(1)
            .next_power_of_two()
            .min(max_resources.next_power_of_two());

        if !caps.storage_buffers {
            degraded.push(format!(
                "{} table requires fallback bindings because storage buffers are unavailable",
                settings.kind.label()
            ));
        }
        if !caps.bindless_resources {
            degraded.push(format!(
                "{} table cannot use bindless resource references",
                settings.kind.label()
            ));
        }
        if !caps.partial_buffer_updates {
            degraded.push(format!(
                "{} table dirty updates require full-table uploads",
                settings.kind.label()
            ));
        }

        let fast_path = caps.storage_buffers && caps.bindless_resources;
        let supports_dirty_range_uploads = caps.storage_buffers && caps.partial_buffer_updates;
        let total_table_bytes = capacity as u64 * stride_bytes as u64;

        Self {
            kind: settings.kind,
            fast_path,
            capacity,
            max_resources,
            stride_bytes,
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

/// Inclusive-start/exclusive-end range of resource table slots to upload.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct SceneResourceDirtyRange {
    pub table: SceneResourceTableKind,
    pub start: u32,
    pub end: u32,
}

impl SceneResourceDirtyRange {
    pub const fn new(table: SceneResourceTableKind, start: u32, end: u32) -> Self {
        Self { table, start, end }
    }

    pub const fn len(self) -> u32 {
        self.end.saturating_sub(self.start)
    }

    pub const fn byte_offset(self, stride_bytes: u32) -> u64 {
        self.start as u64 * stride_bytes as u64
    }

    pub const fn byte_len(self, stride_bytes: u32) -> u64 {
        self.len() as u64 * stride_bytes as u64
    }
}

/// Build upload ranges for dirty resources in one table.
pub fn scene_resource_dirty_ranges(
    table: SceneResourceTableKind,
    dirty_resources: impl IntoIterator<Item = SceneResourceId>,
    capacity: u32,
    merge_gap: u32,
) -> Vec<SceneResourceDirtyRange> {
    if capacity == 0 {
        return Vec::new();
    }

    let mut ids: Vec<u32> = dirty_resources
        .into_iter()
        .filter(|id| id.table == table && id.is_valid() && id.index < capacity)
        .map(|id| id.index)
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
            ranges.push(SceneResourceDirtyRange::new(table, start, end));
            start = id;
            end = id + 1;
        }
    }
    ranges.push(SceneResourceDirtyRange::new(table, start, end));
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_plan_uses_fast_path_with_bindless_storage() {
        let plan = SceneResourceTablePlan::plan(
            700,
            SceneResourceTableCaps {
                storage_buffers: true,
                bindless_resources: true,
                partial_buffer_updates: true,
            },
            SceneResourceTableSettings::lights(),
        );

        assert_eq!(plan.kind, SceneResourceTableKind::Lights);
        assert!(plan.fast_path);
        assert_eq!(plan.capacity, 1024);
        assert_eq!(plan.stride_bytes, 128);
        assert_eq!(plan.total_table_bytes, 1024 * 128);
        assert_eq!(plan.degraded_reason, None);
    }

    #[test]
    fn table_plan_reports_degraded_fallbacks() {
        let plan = SceneResourceTablePlan::plan(
            12,
            SceneResourceTableCaps {
                storage_buffers: true,
                bindless_resources: false,
                partial_buffer_updates: false,
            },
            SceneResourceTableSettings {
                kind: SceneResourceTableKind::ReflectionProbes,
                initial_capacity: 4,
                max_resources: 64,
                stride_bytes: 80,
                dirty_range_merge_gap: 0,
            },
        );

        assert!(!plan.fast_path);
        assert_eq!(plan.capacity, 16);
        assert_eq!(plan.stride_bytes, 128);
        assert!(!plan.supports_dirty_range_uploads);
        let reason = plan.degraded_reason.as_deref().unwrap();
        assert!(reason.contains("reflection_probes"));
        assert!(reason.contains("bindless"));
        assert!(reason.contains("full-table"));
    }

    #[test]
    fn dirty_ranges_filter_table_invalid_and_out_of_capacity_ids() {
        let ranges = scene_resource_dirty_ranges(
            SceneResourceTableKind::Decals,
            [
                SceneResourceId::new(SceneResourceTableKind::Lights, 1),
                SceneResourceId::new(SceneResourceTableKind::Decals, 4),
                SceneResourceId::new(SceneResourceTableKind::Decals, 2),
                SceneResourceId::new(SceneResourceTableKind::Decals, 3),
                SceneResourceId::new(SceneResourceTableKind::Decals, 8),
                SceneResourceId::invalid(SceneResourceTableKind::Decals),
                SceneResourceId::new(SceneResourceTableKind::Decals, 99),
            ],
            16,
            1,
        );

        assert_eq!(
            ranges,
            vec![
                SceneResourceDirtyRange::new(SceneResourceTableKind::Decals, 2, 5),
                SceneResourceDirtyRange::new(SceneResourceTableKind::Decals, 8, 9),
            ]
        );
        assert_eq!(ranges[0].byte_offset(128), 2 * 128);
        assert_eq!(ranges[0].byte_len(128), 3 * 128);
    }
}
