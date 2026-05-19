// Tests extracted from crates/sturdy-engine/src/text_draw.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn text_draw_desc_builder_chain_works() {
    let desc = TextDrawDesc::new("Hello, world!")
        .at(10.0, 20.0)
        .font_size(24.0)
        .color([1.0, 0.5, 0.0, 1.0])
        .max_width(400.0);

    assert_eq!(desc.text, "Hello, world!");
    assert_eq!(desc.x, 10.0);
    assert_eq!(desc.y, 20.0);
    assert_eq!(desc.font_size, 24.0);
    assert_eq!(desc.typography.font_size, 24.0);
    assert_eq!(desc.color, [1.0, 0.5, 0.0, 1.0]);
    assert_eq!(desc.max_width, Some(400.0));
}

#[test]
fn text_draw_desc_default_is_opaque_white() {
    let desc = TextDrawDesc::default();
    assert_eq!(desc.color, [1.0, 1.0, 1.0, 1.0]);
    assert_eq!(desc.font_size, 16.0);
    assert!(desc.typography.standard_ligatures);
    assert!(desc.typography.kerning);
    assert_eq!(desc.max_width, None);
}
