// Tests extracted from crates/sturdy-engine-ffi/src/lib.rs
// Runtime code should stay separate from test code.

use std::ffi::CString;
use std::ptr;

use super::*;

#[test]
fn ffi_declares_and_flushes_a_frame() {
    let mut device = gfx_device_t::default();
    assert_eq!(gfx_create_device(&mut device), gfx_result_t::GFX_OK);

    let image_desc = gfx_image_desc_t {
        width: 64,
        height: 64,
        depth: 1,
        mip_levels: 1,
        layers: 1,
        samples: 1,
        format: gfx_format_t::GFX_FORMAT_RGBA8_UNORM as u32,
        usage_flags: gfx_image_usage_t::GFX_IMAGE_USAGE_RENDER_TARGET as u32
            | gfx_image_usage_t::GFX_IMAGE_USAGE_SAMPLED as u32,
    };
    let mut image = gfx_image_t::default();
    assert_eq!(
        gfx_create_image(device, &image_desc, &mut image),
        gfx_result_t::GFX_OK
    );
    let buffer_desc = gfx_buffer_desc_t {
        size: 256,
        usage_flags: gfx_buffer_usage_t::GFX_BUFFER_USAGE_UNIFORM as u32,
    };
    let mut buffer = gfx_buffer_t::default();
    assert_eq!(
        gfx_create_buffer(device, &buffer_desc, &mut buffer),
        gfx_result_t::GFX_OK
    );

    let mut frame = gfx_frame_t::default();
    assert_eq!(gfx_begin_frame(device, &mut frame), gfx_result_t::GFX_OK);
    assert_eq!(gfx_frame_import_image(frame, image), gfx_result_t::GFX_OK);
    assert_eq!(gfx_frame_import_buffer(frame, buffer), gfx_result_t::GFX_OK);

    let name = CString::new("clear").unwrap();
    let write = gfx_resource_use_t {
        image,
        access: gfx_access_t::GFX_ACCESS_WRITE as u32,
        state: gfx_rg_state_t::GFX_RG_STATE_RENDER_TARGET as u32,
        subresource: gfx_subresource_range_t {
            base_mip: 0,
            mip_count: 1,
            base_layer: 0,
            layer_count: 1,
        },
    };
    let pass = gfx_pass_desc_t {
        name_utf8: name.as_ptr(),
        queue: gfx_queue_type_t::GFX_QUEUE_GRAPHICS as u32,
        shader: gfx_shader_t::default(),
        reads: ptr::null(),
        read_count: 0,
        writes: &write,
        write_count: 1,
        buffer_reads: ptr::null(),
        buffer_read_count: 0,
        buffer_writes: ptr::null(),
        buffer_write_count: 0,
    };
    assert_eq!(gfx_frame_add_pass(frame, &pass), gfx_result_t::GFX_OK);
    assert_eq!(gfx_frame_flush(frame), gfx_result_t::GFX_OK);
    assert_eq!(gfx_frame_wait(frame), gfx_result_t::GFX_OK);

    assert_eq!(gfx_destroy_frame(frame), gfx_result_t::GFX_OK);
    assert_eq!(gfx_destroy_buffer(device, buffer), gfx_result_t::GFX_OK);
    assert_eq!(gfx_destroy_image(device, image), gfx_result_t::GFX_OK);
    assert_eq!(gfx_destroy_device(device), gfx_result_t::GFX_OK);
}
