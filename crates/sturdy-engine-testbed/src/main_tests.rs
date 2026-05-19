// Tests extracted from crates/sturdy-engine-testbed/src/main.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn debug_image_export_names_are_filesystem_friendly() {
    assert_eq!(sanitize_debug_image_name("hdr_composite"), "hdr_composite");
    assert_eq!(
        sanitize_debug_image_name("motion/debug view"),
        "motion_debug_view"
    );
    assert_eq!(sanitize_debug_image_name(""), "debug-image");
}
