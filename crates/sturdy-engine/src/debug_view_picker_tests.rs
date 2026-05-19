// Tests extracted from crates/sturdy-engine/src/debug_view_picker.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn picker_cycles_through_debug_image_names() {
    let engine = Engine::with_backend(crate::BackendKind::Null).unwrap();
    let picker = DebugViewPicker::new(&engine).unwrap();
    let mut controller = RuntimeController::default();
    picker.register(&controller).unwrap();

    let names = vec!["motion_debug_view".to_string(), "hdr_composite".to_string()];
    assert_eq!(
        picker.cycle_next(&mut controller, &names).unwrap(),
        Some("motion_debug_view".to_string())
    );
    assert_eq!(
        picker.cycle_next(&mut controller, &names).unwrap(),
        Some("hdr_composite".to_string())
    );
    assert_eq!(picker.cycle_next(&mut controller, &names).unwrap(), None);
}
