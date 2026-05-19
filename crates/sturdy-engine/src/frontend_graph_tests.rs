// Tests extracted from crates/sturdy-engine/src/frontend_graph.rs
// Runtime code should stay separate from test code.

use super::scheduler::{
    has_declaration_order_hazard, has_read_after_write_dependency, schedule_pass_order,
};
use super::*;

fn image_use(image: u64, access: Access, state: RgState) -> crate::ImageUse {
    crate::ImageUse {
        image: core::ImageHandle(image),
        access,
        state,
        subresource: single_subresource(),
    }
}

fn image_use_mip(image: u64, mip: u16, access: Access, state: RgState) -> crate::ImageUse {
    crate::ImageUse {
        image: core::ImageHandle(image),
        access,
        state,
        subresource: SubresourceRange::new(mip, 1, 0, 1),
    }
}

fn pass(name: &str, reads: &[u64], writes: &[u64]) -> PassDesc {
    PassDesc {
        name: name.to_owned(),
        queue: QueueType::Graphics,
        shader: None,
        pipeline: None,
        bind_groups: Vec::new(),
        push_constants: None,
        pipeline_shading_rate: None,
        work: PassWork::None,
        reads: reads
            .iter()
            .copied()
            .map(|image| image_use(image, Access::Read, RgState::ShaderRead))
            .collect(),
        writes: writes
            .iter()
            .copied()
            .map(|image| image_use(image, Access::Write, RgState::RenderTarget))
            .collect(),
        buffer_reads: Vec::new(),
        buffer_writes: Vec::new(),
        clear_colors: Vec::new(),
        clear_depth: None,
        push_descriptor_set: None,
        predicate: None,
        shader_binding: None,
        shading_rate_image: None,
        perf_counters: None,
    }
}

fn pass_with_uses(
    name: &str,
    reads: Vec<crate::ImageUse>,
    writes: Vec<crate::ImageUse>,
) -> PassDesc {
    PassDesc {
        name: name.to_owned(),
        queue: QueueType::Graphics,
        shader: None,
        pipeline: None,
        bind_groups: Vec::new(),
        push_constants: None,
        pipeline_shading_rate: None,
        work: PassWork::None,
        reads,
        writes,
        buffer_reads: Vec::new(),
        buffer_writes: Vec::new(),
        clear_colors: Vec::new(),
        clear_depth: None,
        push_descriptor_set: None,
        predicate: None,
        shader_binding: None,
        shading_rate_image: None,
        perf_counters: None,
    }
}

#[test]
fn scheduler_keeps_raw_edges_through_declaration_order_waw() {
    let passes = vec![
        pass("tonemap", &[2], &[1]),
        pass("composite", &[], &[2]),
        pass("hud", &[], &[1]),
    ];

    assert!(has_read_after_write_dependency(&passes[1], &passes[0]));
    let order = schedule_pass_order(&passes, &[]);
    assert_eq!(order, vec![1, 0, 2]);
}

#[test]
fn declaration_order_hazards_do_not_create_reverse_waw_edges() {
    let first = pass("first", &[], &[1]);
    let second = pass("second", &[], &[1]);

    assert!(has_declaration_order_hazard(&first, &second));
    assert!(!has_read_after_write_dependency(&first, &second));
    assert!(!has_read_after_write_dependency(&second, &first));
}

#[test]
fn alpha_overlay_read_write_creates_dependency_on_previous_target_write() {
    let tonemap = pass("tonemap", &[2], &[1]);
    let overlay = pass("hud", &[1], &[1]);

    assert!(has_read_after_write_dependency(&tonemap, &overlay));
}

#[test]
fn non_overlapping_mip_writes_do_not_create_declaration_hazard() {
    let mip0 = pass_with_uses(
        "mip0",
        Vec::new(),
        vec![image_use_mip(1, 0, Access::Write, RgState::RenderTarget)],
    );
    let mip1 = pass_with_uses(
        "mip1",
        Vec::new(),
        vec![image_use_mip(1, 1, Access::Write, RgState::RenderTarget)],
    );

    assert!(!has_declaration_order_hazard(&mip0, &mip1));
}

#[test]
fn overlapping_mip_write_and_read_create_raw_dependency() {
    let writer = pass_with_uses(
        "writer",
        Vec::new(),
        vec![image_use_mip(1, 2, Access::Write, RgState::RenderTarget)],
    );
    let reader = pass_with_uses(
        "reader",
        vec![image_use_mip(1, 2, Access::Read, RgState::ShaderRead)],
        Vec::new(),
    );

    assert!(has_read_after_write_dependency(&writer, &reader));
}

#[test]
fn full_resource_access_overlaps_selected_mip() {
    let full = crate::ImageUse {
        image: core::ImageHandle(1),
        access: Access::Write,
        state: RgState::RenderTarget,
        subresource: SubresourceRange::WHOLE,
    };
    let mip = image_use_mip(1, 3, Access::Read, RgState::ShaderRead);

    assert!(image_uses_overlap(&full, &mip));
}

#[test]
fn scheduler_allows_independent_mip_writes_before_dependent_read() {
    let passes = vec![
        pass_with_uses(
            "read-mip1",
            vec![image_use_mip(1, 1, Access::Read, RgState::ShaderRead)],
            Vec::new(),
        ),
        pass_with_uses(
            "write-mip0",
            Vec::new(),
            vec![image_use_mip(1, 0, Access::Write, RgState::RenderTarget)],
        ),
        pass_with_uses(
            "write-mip1",
            Vec::new(),
            vec![image_use_mip(1, 1, Access::Write, RgState::RenderTarget)],
        ),
    ];

    let order = schedule_pass_order(&passes, &[]);
    assert_eq!(order, vec![1, 2, 0]);
}

#[test]
fn subresource_validation_rejects_out_of_bounds_mips_and_layers() {
    let desc = ImageDesc {
        dimension: crate::ImageDimension::D2,
        extent: core::Extent3d {
            width: 64,
            height: 64,
            depth: 1,
        },
        mip_levels: 4,
        layers: 2,
        samples: 1,
        format: Format::Rgba8Unorm,
        usage: crate::ImageUsage::SAMPLED,
        transient: false,
        clear_value: None,
        debug_name: None,
        compression: Default::default(),
        min_lod_bits: None,
        msaa_resolve_to_single_sampled: false,
        drm_format_modifier: None,
    };

    assert!(validate_subresource(desc, SubresourceRange::new(3, 1, 1, 1)).is_ok());
    assert!(validate_subresource(desc, SubresourceRange::new(4, 1, 0, 1)).is_err());
    assert!(validate_subresource(desc, SubresourceRange::new(2, 3, 0, 1)).is_err());
    assert!(validate_subresource(desc, SubresourceRange::new(0, 1, 2, 1)).is_err());
    assert!(validate_subresource(desc, SubresourceRange::new(0, 1, 1, 2)).is_err());
}
