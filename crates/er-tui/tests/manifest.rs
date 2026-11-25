//! er-tui builds on the service alone: no feature brings the App path back,
//! and no er-engine line turns on the `ui` half, which holds the App.

/// Each line that names er-engine, dev-dependency included.
fn engine_lines(manifest: &str) -> Vec<&str> {
    manifest
        .lines()
        .filter(|line| line.trim_start().starts_with("er-engine"))
        .collect()
}

#[test]
fn the_manifest_names_no_v1_service_feature() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("v1-service"),
        "er-tui's Cargo.toml names v1-service"
    );
}

#[test]
fn every_engine_line_leaves_the_ui_half_off() {
    let manifest = include_str!("../Cargo.toml");
    let lines = engine_lines(manifest);
    assert_eq!(lines.len(), 2, "{lines:?}");
    for line in lines {
        assert!(line.contains("default-features = false"), "{line}");
        assert!(!line.contains("\"ui\""), "{line}");
    }
}
