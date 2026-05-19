// Tests extracted from crates/sturdy-engine-core/src/backend.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn auto_preference_order_matches_target_policy() {
    let order = auto_backend_preference_order();

    if cfg!(target_os = "windows") {
        assert_eq!(order, vec![BackendKind::D3d12, BackendKind::Vulkan]);
    } else if cfg!(target_os = "linux") {
        assert_eq!(order, vec![BackendKind::Vulkan]);
    } else if cfg!(target_os = "macos") {
        assert_eq!(order, vec![BackendKind::Vulkan, BackendKind::Metal]);
    } else if cfg!(any(
        target_os = "ios",
        target_os = "tvos",
        target_os = "visionos"
    )) {
        assert_eq!(order, vec![BackendKind::Metal]);
    }
}
