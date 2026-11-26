//! The desktop's wiring of the app owner: what `main.rs` runs at startup,
//! and the one-writer scans of the desktop's own modules.

fn production_source(path: &str) -> String {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(root.join(path)).unwrap();
    // Unit tests sit at the bottom of each file, behind `#[cfg(test)]`.
    text.split("#[cfg(test)]\nmod tests {")
        .next()
        .unwrap_or_default()
        .to_string()
}

fn sources() -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut dirs = vec![std::path::PathBuf::from("src")];
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    while let Some(dir) = dirs.pop() {
        for entry in std::fs::read_dir(root.join(&dir)).unwrap() {
            let path = dir.join(entry.unwrap().file_name());
            if root.join(&path).is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && !path.components().any(|part| part.as_os_str() == "tests")
                && !path.ends_with("tests.rs")
            {
                let name = path.to_string_lossy().into_owned();
                out.push((name.clone(), production_source(&name)));
            }
        }
    }
    out
}

/// The projects record has one writer, `projects.rs`, and the metadata
/// cache has one, the owner: no other file writes either.
#[test]
fn the_projects_record_and_the_metadata_cache_each_have_one_writer() {
    for (path, text) in sources() {
        if path != "src/projects.rs" {
            for writer in ["projects_pins::update", "ProjectsFile>()"] {
                assert!(
                    !text.contains(writer),
                    "{path} writes the projects record ({writer})"
                );
            }
        }
        if path != "src/app_owner.rs" && path != "src/page_view.rs" {
            assert!(
                !text.contains("refresh_meta_cache"),
                "{path} writes the metadata cache"
            );
        }
    }
}

/// The inbox and the projects record are the owner's to save: no desktop
/// module writes `inbox.json` or `projects.json`, or reaches the engine's
/// projects writers, itself.
#[test]
fn no_desktop_module_writes_the_inbox_or_projects_record() {
    let writers = [
        "inbox.json",
        "projects.json",
        "InboxState>()",
        "ProjectsFile>()",
        "projects_pins::update",
        "projects_pins::pin_pr",
        "projects_pins::unpin_pr",
    ];
    for (path, text) in sources() {
        let code = text
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"));
        for line in code {
            for writer in writers {
                assert!(!line.contains(writer), "{path} writes a record: {line}");
            }
        }
    }
}

/// The PR cache has one writer, the owner, over the helpers in
/// `pr_cache.rs`: no other file locks it or its fetch times to write, or
/// calls a helper that does.
#[test]
fn the_pr_cache_has_one_writer() {
    let writers = [
        "pr_cache::refresh_pr_cache",
        "pr_cache::patch_pr_head_oid",
        "pr_cache::merge_pr_results",
        "pr_cache::cache_pr",
        "pr_cache::forget_remote",
        "save_persisted_pr_cache",
    ];
    for (path, text) in sources() {
        if path == "src/app_owner.rs" || path == "src/pr_cache.rs" {
            continue;
        }
        for writer in writers {
            assert!(
                !text.contains(writer),
                "{path} writes the PR cache ({writer})"
            );
        }
        for line in text.lines() {
            let locks = line.contains("pr_cache") && line.contains(".lock()");
            assert!(
                !(locks && line.contains("mut ")),
                "{path} writes the PR cache: {line}"
            );
        }
    }
}

/// The registry's tabs register projects at startup.
#[test]
fn main_registers_projects_from_the_registry() {
    let main = production_source("src/main.rs");
    for call in ["sync_projects_from_tabs", "auto_register("] {
        assert!(!main.contains(call), "{call}");
    }
    assert!(main.contains("startup::register_open_projects(&state)"));
    assert!(main.contains("startup::meta_refresh_interval(&meta_state)"));
}

/// The PR sweep and the setup hook read the inbox prefs through the
/// library, which reads the settings actor on the flag path; `main.rs`
/// never reads them from the App.
#[test]
fn main_reads_the_inbox_prefs_through_startup() {
    let main = production_source("src/main.rs");
    assert!(!main.contains("clone_inbox_prefs"));
    assert_eq!(main.matches("startup::inbox_prefs(&bg_state)").count(), 2);
    assert!(main.contains("startup::flush_pending_notifications(&state)"));
    assert!(main.contains("startup::read_inbox_prefs_from_settings(&desktop)"));
}

/// The head probe takes its target from the library, which reads the
/// registry on the flag path, and patches through the owner.
#[test]
fn main_probes_the_pr_head_through_the_startup_target_and_the_owner() {
    let main = production_source("src/main.rs");
    assert!(main.contains("startup::pr_head_probe_target(&probe_state)"));
    assert!(!main.contains("fn pr_head_probe_target"));
    assert!(main.contains(".patch_pr_head_oid(&slug, pr_number, &oid)"));
}
