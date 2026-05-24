/// Runtime selection policy for Lumen-like realtime global illumination.
///
/// This module does not render GI by itself. It is the backend-neutral contract
/// that higher-level renderer code can use to pick an honest path:
/// hardware ray tracing when available and requested, or a compute-friendly
/// screen-probe/surface-cache path when RT is unavailable or disabled.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RealtimeGiRequest {
    /// Disable realtime GI entirely.
    Disabled,
    /// Pick the best available path for the current device.
    Auto,
    /// Force the non-RT path. Useful for validation and lower-end hardware.
    Software,
    /// Force hardware ray-traced GI; planning rejects this when RT is unavailable.
    HardwareRayTracing,
    /// Prefer RT for high-frequency/direct visibility and the software cache for
    /// stable diffuse history. Falls back to software when RT is unavailable.
    Hybrid,
}

impl Default for RealtimeGiRequest {
    fn default() -> Self {
        Self::Auto
    }
}

/// Concrete GI path chosen for a frame/renderer configuration.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum RealtimeGiPath {
    Disabled,
    /// Non-RT GI path: screen probes, signed-distance/surface-cache traces,
    /// temporal accumulation, and radiance cache reuse.
    ScreenProbeGather,
    /// Hardware ray-traced GI path.
    HardwareRayTracing,
    /// Hardware RT assisted by the same probe/cache infrastructure used by the
    /// software path, so the renderer can keep stable history and fallbacks.
    Hybrid,
}

impl RealtimeGiPath {
    pub const fn uses_ray_tracing(self) -> bool {
        matches!(self, Self::HardwareRayTracing | Self::Hybrid)
    }

    pub const fn uses_software_cache(self) -> bool {
        matches!(self, Self::ScreenProbeGather | Self::Hybrid)
    }
}

/// Device/runtime capabilities relevant to realtime GI.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct RealtimeGiCaps {
    pub compute: bool,
    pub ray_tracing: bool,
    pub bindless: bool,
    /// Whether temporal history resources can be kept across frames. The
    /// software path can still run without history, but it is lower quality.
    pub temporal_history: bool,
}

impl Default for RealtimeGiCaps {
    fn default() -> Self {
        Self {
            compute: true,
            ray_tracing: false,
            bindless: false,
            temporal_history: true,
        }
    }
}

impl RealtimeGiCaps {
    pub fn from_caps(caps: &sturdy_engine_core::Caps) -> Self {
        Self {
            compute: true,
            ray_tracing: caps.supports_raytracing,
            bindless: caps.supports_bindless,
            temporal_history: caps.max_frames_in_flight > 1,
        }
    }
}

/// Quality/performance knobs for realtime GI planning.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RealtimeGiSettings {
    pub request: RealtimeGiRequest,
    /// Diffuse probe rays budgeted per frame for the non-RT path.
    pub software_probe_rays_per_frame: u32,
    /// Hardware RT rays budgeted per frame for the RT path.
    pub hardware_rays_per_frame: u32,
    /// Maximum frames over which diffuse radiance may converge.
    pub temporal_accumulation_frames: u32,
}

impl Default for RealtimeGiSettings {
    fn default() -> Self {
        Self {
            request: RealtimeGiRequest::Auto,
            software_probe_rays_per_frame: 1_000_000,
            hardware_rays_per_frame: 2_000_000,
            temporal_accumulation_frames: 8,
        }
    }
}

/// Result of GI path selection. `degraded_reason` is populated whenever the
/// selected path is lower quality than requested or loses an important support
/// feature such as bindless indexing or temporal history.
#[derive(Clone, Debug, PartialEq)]
pub struct RealtimeGiPlan {
    pub requested: RealtimeGiRequest,
    pub path: RealtimeGiPath,
    pub probe_rays_per_frame: u32,
    pub rt_rays_per_frame: u32,
    pub temporal_accumulation_frames: u32,
    pub degraded_reason: Option<String>,
}

impl RealtimeGiPlan {
    pub fn plan(caps: RealtimeGiCaps, settings: RealtimeGiSettings) -> Self {
        let mut degraded = Vec::new();

        let path = match settings.request {
            RealtimeGiRequest::Disabled => RealtimeGiPath::Disabled,
            RealtimeGiRequest::Auto => {
                if caps.ray_tracing && caps.compute {
                    RealtimeGiPath::Hybrid
                } else if caps.compute {
                    RealtimeGiPath::ScreenProbeGather
                } else {
                    degraded.push("compute is unavailable, realtime GI disabled".to_string());
                    RealtimeGiPath::Disabled
                }
            }
            RealtimeGiRequest::Software => {
                if caps.compute {
                    RealtimeGiPath::ScreenProbeGather
                } else {
                    degraded.push("software GI requires compute support".to_string());
                    RealtimeGiPath::Disabled
                }
            }
            RealtimeGiRequest::HardwareRayTracing => {
                if caps.ray_tracing {
                    RealtimeGiPath::HardwareRayTracing
                } else if caps.compute {
                    degraded.push(
                        "hardware ray tracing unavailable, using screen-probe GI fallback"
                            .to_string(),
                    );
                    RealtimeGiPath::ScreenProbeGather
                } else {
                    degraded
                        .push("hardware ray tracing and compute fallback unavailable".to_string());
                    RealtimeGiPath::Disabled
                }
            }
            RealtimeGiRequest::Hybrid => {
                if caps.ray_tracing && caps.compute {
                    RealtimeGiPath::Hybrid
                } else if caps.compute {
                    degraded.push(
                        "hardware ray tracing unavailable, using software GI only".to_string(),
                    );
                    RealtimeGiPath::ScreenProbeGather
                } else {
                    degraded.push("hybrid GI requires compute support".to_string());
                    RealtimeGiPath::Disabled
                }
            }
        };

        if path != RealtimeGiPath::Disabled && !caps.bindless {
            degraded
                .push("bindless resources unavailable, GI will need fallback bindings".to_string());
        }
        if path != RealtimeGiPath::Disabled && !caps.temporal_history {
            degraded.push("temporal history unavailable, GI will converge noisier".to_string());
        }

        let uses_software = path.uses_software_cache();
        let uses_rt = path.uses_ray_tracing();
        let temporal_accumulation_frames = if caps.temporal_history {
            settings.temporal_accumulation_frames.max(1)
        } else {
            1
        };

        Self {
            requested: settings.request,
            path,
            probe_rays_per_frame: if uses_software {
                settings.software_probe_rays_per_frame
            } else {
                0
            },
            rt_rays_per_frame: if uses_rt {
                settings.hardware_rays_per_frame
            } else {
                0
            },
            temporal_accumulation_frames,
            degraded_reason: if degraded.is_empty() {
                None
            } else {
                Some(degraded.join("; "))
            },
        }
    }

    pub const fn is_enabled(&self) -> bool {
        !matches!(self.path, RealtimeGiPath::Disabled)
    }
}

/// Settings for the software/hybrid GI surface cache used by the non-RT path.
///
/// The cache is a renderer-facing contract for Lumen-like behavior: nearby
/// surfaces update at higher density, distant surfaces live in coarser clipmaps,
/// and only a bounded number of probes/pages are refreshed each frame.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RealtimeGiSurfaceCacheSettings {
    /// World-space radius around the camera covered by the innermost clipmap.
    pub inner_radius_m: f32,
    /// Number of nested clipmaps. Each clipmap doubles coverage radius.
    pub clipmap_count: u32,
    /// Probe spacing in the innermost clipmap.
    pub probe_spacing_m: f32,
    /// Square atlas resolution in pages. `atlas_pages_per_side^2` is the total
    /// resident page capacity for surface-card/probe radiance data.
    pub atlas_pages_per_side: u32,
    /// Maximum cache pages/probes allowed to refresh per frame.
    pub max_page_updates_per_frame: u32,
    /// Keep this fraction of the atlas free for newly visible geometry before
    /// evicting stable pages.
    pub emergency_free_fraction: f32,
}

impl Default for RealtimeGiSurfaceCacheSettings {
    fn default() -> Self {
        Self {
            inner_radius_m: 24.0,
            clipmap_count: 4,
            probe_spacing_m: 1.5,
            atlas_pages_per_side: 128,
            max_page_updates_per_frame: 512,
            emergency_free_fraction: 0.10,
        }
    }
}

/// Derived surface-cache allocation for a chosen GI path.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RealtimeGiSurfaceCachePlan {
    pub enabled: bool,
    pub clipmap_count: u32,
    pub inner_radius_m: f32,
    pub outer_radius_m: f32,
    pub probe_spacing_m: f32,
    pub atlas_pages_per_side: u32,
    pub atlas_page_capacity: u32,
    pub reserved_free_pages: u32,
    pub max_page_updates_per_frame: u32,
}

impl RealtimeGiSurfaceCachePlan {
    pub fn plan(
        gi: &RealtimeGiPlan,
        settings: RealtimeGiSurfaceCacheSettings,
    ) -> RealtimeGiSurfaceCachePlan {
        if !gi.path.uses_software_cache() {
            return Self::disabled();
        }

        let clipmap_count = settings.clipmap_count.clamp(1, 8);
        let inner_radius_m = settings.inner_radius_m.max(1.0);
        let probe_spacing_m = settings.probe_spacing_m.max(0.25);
        let atlas_pages_per_side = settings.atlas_pages_per_side.clamp(16, 16_384);
        let atlas_page_capacity = atlas_pages_per_side.saturating_mul(atlas_pages_per_side);
        let reserved_free_pages = ((atlas_page_capacity as f32
            * settings.emergency_free_fraction.clamp(0.0, 0.5))
        .round() as u32)
            .min(atlas_page_capacity / 2);
        let usable_pages = atlas_page_capacity.saturating_sub(reserved_free_pages);
        let max_page_updates_per_frame = settings
            .max_page_updates_per_frame
            .max(1)
            .min(usable_pages.max(1));
        let outer_radius_m = inner_radius_m * 2.0_f32.powi(clipmap_count.saturating_sub(1) as i32);

        Self {
            enabled: true,
            clipmap_count,
            inner_radius_m,
            outer_radius_m,
            probe_spacing_m,
            atlas_pages_per_side,
            atlas_page_capacity,
            reserved_free_pages,
            max_page_updates_per_frame,
        }
    }

    pub const fn disabled() -> Self {
        Self {
            enabled: false,
            clipmap_count: 0,
            inner_radius_m: 0.0,
            outer_radius_m: 0.0,
            probe_spacing_m: 0.0,
            atlas_pages_per_side: 0,
            atlas_page_capacity: 0,
            reserved_free_pages: 0,
            max_page_updates_per_frame: 0,
        }
    }

    /// Conservative probe count for one clipmap level. This is CPU-side planning
    /// only; the GPU path can page only visible/needed probes into the atlas.
    pub fn probes_per_clipmap(&self) -> u32 {
        if !self.enabled || self.probe_spacing_m <= 0.0 {
            return 0;
        }

        let diameter = self.inner_radius_m * 2.0;
        let probes_per_axis = (diameter / self.probe_spacing_m).ceil().max(1.0) as u32;
        probes_per_axis.saturating_mul(probes_per_axis)
    }

    /// Total logical probes across all clipmaps before visibility/residency culling.
    pub fn logical_probe_count(&self) -> u32 {
        self.probes_per_clipmap().saturating_mul(self.clipmap_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_uses_hybrid_when_rt_and_compute_are_available() {
        let plan = RealtimeGiPlan::plan(
            RealtimeGiCaps {
                compute: true,
                ray_tracing: true,
                bindless: true,
                temporal_history: true,
            },
            RealtimeGiSettings::default(),
        );

        assert_eq!(plan.path, RealtimeGiPath::Hybrid);
        assert!(plan.rt_rays_per_frame > 0);
        assert!(plan.probe_rays_per_frame > 0);
        assert_eq!(plan.degraded_reason, None);
    }

    #[test]
    fn forced_rt_falls_back_to_software_without_rt() {
        let plan = RealtimeGiPlan::plan(
            RealtimeGiCaps {
                compute: true,
                ray_tracing: false,
                bindless: true,
                temporal_history: true,
            },
            RealtimeGiSettings {
                request: RealtimeGiRequest::HardwareRayTracing,
                ..RealtimeGiSettings::default()
            },
        );

        assert_eq!(plan.path, RealtimeGiPath::ScreenProbeGather);
        assert_eq!(plan.rt_rays_per_frame, 0);
        assert!(plan.probe_rays_per_frame > 0);
        assert!(
            plan.degraded_reason
                .as_deref()
                .unwrap()
                .contains("fallback")
        );
    }

    #[test]
    fn no_temporal_history_clamps_accumulation_to_one_frame() {
        let plan = RealtimeGiPlan::plan(
            RealtimeGiCaps {
                compute: true,
                ray_tracing: false,
                bindless: false,
                temporal_history: false,
            },
            RealtimeGiSettings {
                request: RealtimeGiRequest::Software,
                temporal_accumulation_frames: 16,
                ..RealtimeGiSettings::default()
            },
        );

        assert_eq!(plan.path, RealtimeGiPath::ScreenProbeGather);
        assert_eq!(plan.temporal_accumulation_frames, 1);
        let reason = plan.degraded_reason.as_deref().unwrap();
        assert!(reason.contains("bindless"));
        assert!(reason.contains("temporal history"));
    }

    #[test]
    fn surface_cache_is_enabled_for_software_and_hybrid_paths_only() {
        let software = RealtimeGiPlan::plan(
            RealtimeGiCaps {
                compute: true,
                ray_tracing: false,
                bindless: true,
                temporal_history: true,
            },
            RealtimeGiSettings {
                request: RealtimeGiRequest::Software,
                ..RealtimeGiSettings::default()
            },
        );
        let software_cache =
            RealtimeGiSurfaceCachePlan::plan(&software, RealtimeGiSurfaceCacheSettings::default());
        assert!(software_cache.enabled);
        assert!(software_cache.logical_probe_count() > 0);

        let hardware = RealtimeGiPlan::plan(
            RealtimeGiCaps {
                compute: true,
                ray_tracing: true,
                bindless: true,
                temporal_history: true,
            },
            RealtimeGiSettings {
                request: RealtimeGiRequest::HardwareRayTracing,
                ..RealtimeGiSettings::default()
            },
        );
        let hardware_cache =
            RealtimeGiSurfaceCachePlan::plan(&hardware, RealtimeGiSurfaceCacheSettings::default());
        assert!(!hardware_cache.enabled);
        assert_eq!(hardware_cache.logical_probe_count(), 0);
    }

    #[test]
    fn surface_cache_clamps_invalid_settings_to_safe_budget() {
        let gi = RealtimeGiPlan::plan(
            RealtimeGiCaps {
                compute: true,
                ray_tracing: true,
                bindless: true,
                temporal_history: true,
            },
            RealtimeGiSettings::default(),
        );
        let cache = RealtimeGiSurfaceCachePlan::plan(
            &gi,
            RealtimeGiSurfaceCacheSettings {
                inner_radius_m: 0.0,
                clipmap_count: 99,
                probe_spacing_m: 0.0,
                atlas_pages_per_side: 1,
                max_page_updates_per_frame: u32::MAX,
                emergency_free_fraction: 1.0,
            },
        );

        assert!(cache.enabled);
        assert_eq!(cache.clipmap_count, 8);
        assert_eq!(cache.inner_radius_m, 1.0);
        assert_eq!(cache.probe_spacing_m, 0.25);
        assert_eq!(cache.atlas_pages_per_side, 16);
        assert_eq!(cache.atlas_page_capacity, 256);
        assert_eq!(cache.reserved_free_pages, 128);
        assert_eq!(cache.max_page_updates_per_frame, 128);
        assert_eq!(cache.outer_radius_m, 128.0);
    }
}
