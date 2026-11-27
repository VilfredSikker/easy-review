use std::path::{Path, PathBuf};

/// Only the desktop's composition registers the app records' kinds; the TUI
/// and MCP build their registries from the shared table alone, which their
/// "does not claim" tests read.
#[test]
fn only_the_desktop_registers_the_app_record_kinds() {
    let call = concat!("register_desktop", "_kinds(");
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut callers = Vec::new();
    for krate in std::fs::read_dir(&crates).unwrap() {
        let krate = krate.unwrap().path();
        let mut dirs: Vec<PathBuf> = ["src", "tests"].map(|dir| krate.join(dir)).into();
        while let Some(dir) = dirs.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    dirs.push(path);
                } else if path.extension().is_some_and(|ext| ext == "rs")
                    && std::fs::read_to_string(&path).unwrap().contains(call)
                {
                    let name = path.strip_prefix(&crates).unwrap();
                    callers.push(name.to_string_lossy().into_owned());
                }
            }
        }
    }
    callers.sort();
    assert_eq!(
        callers,
        [
            "er-desktop/src/store_registry.rs",
            "er-service/src/app_records.rs"
        ]
    );
}

/// Every er-service source under `src`, outside the tests, with its path
/// from `src`.
fn service_sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    let mut dirs = vec![src.clone()];
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path
                .strip_prefix(&src)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            if path.is_dir() {
                if !name.ends_with("tests") {
                    dirs.push(path);
                }
            } else if path.extension().is_some_and(|ext| ext == "rs") && !name.ends_with("tests.rs")
            {
                let text = std::fs::read_to_string(&path).unwrap();
                let code = text
                    .split("#[cfg(test)]\nmod tests {")
                    .next()
                    .unwrap_or_default();
                out.push((name, code.to_string()));
            }
        }
    }
    out
}

/// Inside er-service the owner is the one writer of the projects record and
/// the one saver of the inbox: no other module calls the record's writers,
/// the engine's, or the inbox's save, by path or by a name it imports, so
/// none can write and skip the publish. `projects.rs` holds the record's
/// writers and calls the engine's.
#[test]
fn only_the_owner_writes_the_projects_record_and_saves_the_inbox() {
    let owned = [
        ("projects", "ensure_remote_project"),
        ("projects", "delete_project"),
        ("projects", "reorder_projects"),
        ("projects", "save_pr"),
        ("projects", "unsave_pr"),
        ("projects", "sync_projects_from_refs"),
        ("projects", "auto_register"),
        ("projects", "dismiss_pr"),
        ("projects", "undismiss_pr"),
        ("projects", "track_pr"),
        ("projects", "untrack_pr"),
        ("projects", "add_tracked_branch"),
        ("projects", "remove_tracked_branch"),
        ("projects", "set_active"),
        ("projects", "patch_project_review_settings"),
        ("inbox", "save_inbox_state"),
    ];
    let engine = [
        ("projects_pins", "update"),
        ("projects_pins", "pin_pr"),
        ("projects_pins", "unpin_pr"),
        ("projects_pins", "ensure_project_for_remote"),
        ("projects_pins", "resolve_project_for_pin"),
    ];
    for (path, text) in service_sources() {
        if path == "app_records/app_owner.rs" {
            continue;
        }
        let writers = owned.iter();
        let writers = writers.chain(engine.iter().filter(|_| path != "app_records/projects.rs"));
        for (module, name) in writers {
            let call = format!("{module}::{name}");
            assert!(!text.contains(&call), "{path} writes a record ({call})");
            let imports = imported(&text, module);
            assert!(
                !imports.iter().any(|imported| imported == name),
                "{path} imports a record's writer ({call})"
            );
        }
    }
}

/// The names the `use` items of `text` that name `module::` import,
/// braced lists included.
fn imported(text: &str, module: &str) -> Vec<String> {
    let path = format!("{module}::");
    text.split("use ")
        .skip(1)
        .filter_map(|rest| rest.split_once(';').map(|(item, _)| item))
        .filter(|item| item.contains(&path))
        .flat_map(|item| {
            item.split(|c: char| !(c.is_alphanumeric() || c == '_'))
                .filter(|word| !word.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>()
        })
        .collect()
}
