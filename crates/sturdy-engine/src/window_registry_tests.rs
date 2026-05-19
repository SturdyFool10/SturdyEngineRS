// Tests extracted from crates/sturdy-engine/src/window_registry.rs
// Runtime code should stay separate from test code.

use super::*;

#[test]
fn registry_returns_inserted_window_by_handle() {
    let mut registry = WindowRegistry::new();
    let handle = registry.insert("primary");

    assert_eq!(registry.get(handle), Some(&"primary"));
    assert!(registry.contains(handle));
    assert_eq!(registry.live_count(), 1);
}

#[test]
fn removed_handle_does_not_resolve_after_slot_reuse() {
    let mut registry = WindowRegistry::new();
    let old = registry.insert("old");
    assert_eq!(registry.remove(old), Some("old"));

    let new = registry.insert("new");

    assert_eq!(old.id(), new.id());
    assert_ne!(old.generation(), new.generation());
    assert_eq!(registry.get(old), None);
    assert_eq!(registry.get(new), Some(&"new"));
}

#[test]
fn stale_remove_does_not_remove_new_window() {
    let mut registry = WindowRegistry::new();
    let old = registry.insert(1);
    assert_eq!(registry.remove(old), Some(1));
    let new = registry.insert(2);

    assert_eq!(registry.remove(old), None);
    assert_eq!(registry.get(new), Some(&2));
}

#[test]
fn iter_returns_live_handles() {
    let mut registry = WindowRegistry::new();
    let removed = registry.insert("removed");
    let live = registry.insert("live");
    assert_eq!(registry.remove(removed), Some("removed"));

    let entries = registry.iter().collect::<Vec<_>>();

    assert_eq!(entries, vec![(live, &"live")]);
}
