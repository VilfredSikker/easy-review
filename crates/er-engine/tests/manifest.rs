//! The engine has one half: no `ui` feature and none of the App's
//! dependencies. A re-added feature would bring back a build only some crates
//! see, which is what the App's removal took away (ADR 0068).

use toml::Value;

fn manifest() -> anyhow::Result<Value> {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    Ok(toml::from_str(&std::fs::read_to_string(path)?)?)
}

/// Every dependency table, the `[target.*]` ones included.
fn dependency_tables(manifest: &Value) -> Vec<&toml::Table> {
    let kinds = ["dependencies", "dev-dependencies", "build-dependencies"];
    let mut roots = vec![manifest];
    if let Some(targets) = manifest.get("target").and_then(Value::as_table) {
        roots.extend(targets.values());
    }
    roots
        .into_iter()
        .flat_map(|root| kinds.iter().filter_map(|kind| root.get(*kind)?.as_table()))
        .collect()
}

#[test]
fn the_engine_has_no_ui_feature() -> anyhow::Result<()> {
    let manifest = manifest()?;
    let features = manifest
        .get("features")
        .and_then(Value::as_table)
        .expect("er-engine declares features");
    assert!(
        !features.contains_key("ui"),
        "er-engine declares a `ui` feature"
    );
    for (name, enables) in features {
        let enables = enables.as_array().expect("a feature lists what it enables");
        assert!(
            !enables.iter().any(|item| item.as_str() == Some("ui")),
            "feature `{name}` turns on `ui`"
        );
    }
    Ok(())
}

#[test]
fn no_dependency_carries_the_apps_text_area() -> anyhow::Result<()> {
    let manifest = manifest()?;
    for table in dependency_tables(&manifest) {
        assert!(
            !table.contains_key("tui-textarea-2"),
            "er-engine depends on tui-textarea-2 again"
        );
        let own = table.get("er-engine").and_then(|dep| dep.get("features"));
        let own = own
            .and_then(Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        assert!(
            !own.iter().any(|item| item.as_str() == Some("ui")),
            "er-engine's dependency on itself turns on `ui`"
        );
    }
    Ok(())
}
