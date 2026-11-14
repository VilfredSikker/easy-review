//! The TUI consumes the shared composition; it has no kinds of its own.

use std::path::Path;

#[test]
fn the_tui_registry_has_every_shared_kind_including_session() {
    let registry = er_engine::store_registry::registry();
    for (file, kind) in er_engine::store_registry::expected_kinds() {
        assert_eq!(registry.kind_for(Path::new(file)), Some(*kind), "{file}");
    }
    for file in ["session.json", "reviewed.json"] {
        assert!(registry.kind_for(Path::new(file)).is_some(), "{file}");
    }
}

#[test]
fn the_tui_registry_does_not_claim_desktop_kinds() {
    let registry = er_engine::store_registry::registry();
    for file in [
        "gh-status-cache.json",
        "inbox.json",
        "pr-cache.json",
        "pr-open-cache.json",
        "tabs.json",
    ] {
        assert_eq!(registry.kind_for(Path::new(file)), None, "{file}");
    }
}
