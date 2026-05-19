// Tests extracted from crates/textui/src/lib.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn atlas_page_side_is_clamped_to_hardware_limit() {
    let mut text = TextUi::new();
    text.begin_frame_info(TextFrameInfo::new(1, 128));
    assert_eq!(text.resolved_page_side(), 128);
}

#[test]
fn atlas_page_data_reuses_cached_snapshot_until_pixels_change() {
    let mut page = AtlasPage::new(4, TextAtlasContentMode::AlphaMask);
    page.blit(&[255, 255, 255, 255], [1, 1], [0, 0]);

    let first = page.data(0);
    let second = page.data(0);
    assert!(Arc::ptr_eq(&first.rgba8, &second.rgba8));

    page.blit(&[127, 127, 127, 127], [1, 1], [1, 0]);
    let changed = page.data(0);
    assert!(!Arc::ptr_eq(&first.rgba8, &changed.rgba8));
}
