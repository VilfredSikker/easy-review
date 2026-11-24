//! The first-launch notices: 1.0 starts with empty storage, and two settings
//! are no longer read. Each shows once per machine, whichever front end runs
//! first, because its marker lives in the new storage root.
//!
//! The markers are files the artifact store does not own. They carry no
//! envelope and no version, so they are not kinds; the test below fails if a
//! name here resolves to a registered one.

use std::path::Path;

/// The marker a first-launch notice leaves behind, so it shows once.
const LEGACY_ROOT_MARKER: &str = ".legacy-root-noticed";
/// The retired `[commands].summary` key was announced. Separate from the
/// legacy-root marker, so a machine that already saw that notice still hears
/// about the key.
const RETIRED_SUMMARY_MARKER: &str = ".retired-summary-noticed";
/// The retired per-project `auto_triage` flag was announced. Separate from the
/// other two markers for the same reason.
const RETIRED_AUTO_TRIAGE_MARKER: &str = ".retired-auto-triage-noticed";

/// Write a launch marker. A failed write means the notice shows again next
/// launch, so the error is dropped on purpose.
fn mark_notice_once(dir: &Path, marker: &str) {
    let _ = std::fs::create_dir_all(dir);
    let _ = std::fs::write(dir.join(marker), b"");
}

/// Imports the pre-1.0 project list once, then answers this launch's notice.
///
/// The import is a no-op once the projects record exists; running it here
/// means the notice can name the import on the launch that made it.
pub fn notice() -> Option<String> {
    crate::projects_pins::import_legacy_projects_once();
    let base = crate::storage::storage_base();
    let root = crate::storage::storage_root();
    notice_in(&base, &root)
}

/// [`notice`] without the import, for a given base and root. Writes the
/// marker of each part it answers.
pub fn notice_in(base: &Path, root: &Path) -> Option<String> {
    // With the override set, base and root are the same directory and
    // there is no previous version's data.
    if base == root {
        return None;
    }
    let mut messages: Vec<String> = Vec::new();

    if !root.join(LEGACY_ROOT_MARKER).exists() {
        let entries = crate::storage::legacy_root_entries_in(base, false);
        let projects_imported = root.join(crate::legacy_projects::IMPORTED_MARKER).exists();
        if let Some(message) = legacy_root_notice(base, &entries, projects_imported) {
            // A failed write means the notice shows again next launch.
            mark_notice_once(root, LEGACY_ROOT_MARKER);
            messages.push(message);
        }
    }

    if !root.join(RETIRED_SUMMARY_MARKER).exists() {
        if let Some(message) = retired_summary_notice(&crate::config::managed_config_path()) {
            mark_notice_once(root, RETIRED_SUMMARY_MARKER);
            messages.push(message);
        }
    }

    if !root.join(RETIRED_AUTO_TRIAGE_MARKER).exists() {
        if let Some(path) = crate::projects_pins::projects_artifact_path() {
            if let Some(message) = retired_auto_triage_notice(&path) {
                mark_notice_once(root, RETIRED_AUTO_TRIAGE_MARKER);
                messages.push(message);
            }
        }
    }

    (!messages.is_empty()).then(|| messages.join(" "))
}

/// The one-time notice that the previous version's data was left behind.
///
/// `None` when there is nothing from a previous version. The old settings path
/// is named only when a `config.toml` is among the entries; the project list is
/// named when the one-time import ran.
fn legacy_root_notice(
    base: &Path,
    entries: &[std::path::PathBuf],
    projects_imported: bool,
) -> Option<String> {
    if entries.is_empty() {
        return None;
    }
    let base_text = base.display();
    let settings = if entries
        .iter()
        .any(|entry| entry.file_name().and_then(|name| name.to_str()) == Some("config.toml"))
    {
        format!(" (old settings: {base_text}/config.toml)")
    } else {
        String::new()
    };
    let projects = if projects_imported {
        " Your project list was imported into this version."
    } else {
        ""
    };
    Some(format!(
        "Easy Review 1.0 starts with empty storage. Data from the previous version is untouched in {base_text}{settings}. It is not read any more; delete it when you no longer need it.{projects}"
    ))
}

/// The notice that `[commands].summary` is no longer read.
fn retired_summary_notice(config_path: &Path) -> Option<String> {
    if !crate::config::retired_summary_command_set(config_path) {
        return None;
    }
    Some(format!(
        "The [commands].summary setting is no longer read: Generate summary now runs a built-in job. You can remove it from {}.",
        config_path.display()
    ))
}

/// The notice that the per-project `auto_triage` flag is no longer read.
fn retired_auto_triage_notice(projects_path: &Path) -> Option<String> {
    if !crate::projects_pins::retired_auto_triage_set() {
        return None;
    }
    Some(format!(
        "The project auto-triage setting is no longer read: triage starts from the sidebar. You can remove it from {}.",
        projects_path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_root_notice_names_the_base_and_the_old_config() {
        let base = std::path::PathBuf::from("/data/easy-review");
        let with_config = [base.join("repos"), base.join("config.toml")];
        let text = legacy_root_notice(&base, &with_config, false).expect("a notice");
        assert!(text.contains("/data/easy-review"), "{text}");
        assert!(text.contains("/data/easy-review/config.toml"), "{text}");
        assert!(!text.contains("project list"), "{text}");

        let without_config = [base.join("repos")];
        let text = legacy_root_notice(&base, &without_config, false).expect("a notice");
        assert!(text.contains("/data/easy-review"), "{text}");
        assert!(!text.contains("config.toml"), "{text}");

        let text = legacy_root_notice(&base, &without_config, true).expect("a notice");
        assert!(text.contains("project list was imported"), "{text}");
    }

    /// The front ends call `notice` at start, so the import runs there even
    /// when the launch has nothing to say.
    #[test]
    fn the_notice_imports_the_old_project_list() {
        let store = tempfile::tempdir().unwrap();
        let config = tempfile::tempdir().unwrap();
        let legacy_dir = config.path().join("er");
        std::fs::create_dir_all(&legacy_dir).unwrap();
        std::fs::write(
            legacy_dir.join("projects.json"),
            r#"{"active_id":null,"projects":[{"id":"p1","name":"One","root_path":"/repos/one"}]}"#,
        )
        .unwrap();
        let config_dir = config.path().to_string_lossy().into_owned();
        crate::storage::with_test_storage_root(store.path(), || {
            crate::test_support::with_var("ER_LEGACY_CONFIG_DIR", config_dir, || {
                let record = crate::projects_pins::projects_artifact_path().expect("a path");
                assert!(!record.exists());
                assert_eq!(notice(), None, "the override has no previous version");
                assert!(record.exists(), "the old project list was not imported");
            });
        });
    }

    /// No quoted name in the production part may resolve to a registered kind.
    /// The registry is the source of truth, so a new kind needs no edit here.
    #[test]
    fn the_helper_never_names_a_registered_kind() {
        let production = include_str!("first_launch.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap_or_default();
        let registry = crate::store_registry::registry();
        for literal in production.split('"').skip(1).step_by(2) {
            if literal.is_empty() {
                continue;
            }
            assert!(
                registry.kind_for(std::path::Path::new(literal)).is_none(),
                "first_launch.rs names the registered kind for {literal}"
            );
        }
    }
}
