//! er-tui builds on the service alone: no feature brings the App path back,
//! and every er-engine dependency turns default features off and names no
//! `ui` feature, in the inline form or the `[dependencies.er-engine]` table.

use toml::Value;

#[test]
fn the_manifest_names_no_v1_service_feature() {
    let manifest = include_str!("../Cargo.toml");
    assert!(
        !manifest.contains("v1-service"),
        "er-tui's Cargo.toml names v1-service"
    );
}

#[test]
fn every_engine_dependency_leaves_default_features_off() -> anyhow::Result<()> {
    let manifest: Value = toml::from_str(include_str!("../Cargo.toml"))?;
    let kinds = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut roots = vec![&manifest];
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        roots.extend(targets.values());
    }
    let engines: Vec<&Value> = roots
        .into_iter()
        .flat_map(|root| {
            kinds
                .iter()
                .filter_map(|kind| root.get(*kind)?.get("er-engine"))
        })
        .collect();
    assert_eq!(engines.len(), 2, "{engines:?}");
    for engine in engines {
        let off = engine.get("default-features").and_then(Value::as_bool);
        assert_eq!(off, Some(false), "{engine}");
        let features = engine.get("features").and_then(Value::as_array);
        let features = features.map(Vec::as_slice).unwrap_or_default();
        assert!(
            !features
                .iter()
                .any(|feature| feature.as_str() == Some("ui")),
            "{engine}"
        );
    }
    Ok(())
}
