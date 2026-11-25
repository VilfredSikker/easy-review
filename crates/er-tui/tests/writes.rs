//! No TUI module writes a store artifact, the working tree or GitHub itself:
//! each write is a command the service runs (ADR 0073). The one session save
//! is `service/session.rs`'s, and the one file write is `update.rs`'s
//! downloaded archive. Test modules are skipped.

use std::path::{Path, PathBuf};

const STORE: [&str; 11] = [
    "save_reviewed",
    "update_reviewed",
    "save_config",
    "annotation_store::update_",
    "annotation_store::delete",
    "update_questions(",
    "update_notes(",
    "update_github_comments(",
    ".write(&",
    "save_session(",
    "save_snapshot",
];

const GIT: [&str; 5] = [
    "stage_file",
    "unstage_file",
    "stage_all",
    "git.commit(",
    "git.push(",
];

const GITHUB: [&str; 9] = [
    "post_line_comment",
    "post_reply",
    "post_general",
    "submit_review",
    "pending_review_add_comment",
    "pending_review_remove_comment",
    "pending_review_submit",
    "delete_comment",
    "gh_pr_approve",
];

const FILES: [&str; 8] = [
    "fs::write",
    "remove_file",
    "remove_dir",
    "create_dir",
    "rename(",
    "File::create",
    "OpenOptions",
    "fs::copy",
];

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{e}")) {
        let path = entry.unwrap_or_else(|e| panic!("{e}")).path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Each source file's code outside its test module and comments, joined
/// into one line, named by its path from `src/`.
fn sources() -> Vec<(String, String)> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut paths = Vec::new();
    walk(&src, &mut paths);
    let mut out = Vec::new();
    for path in paths {
        let name = path.strip_prefix(&src).unwrap_or(&path);
        let name = name.to_string_lossy().replace('\\', "/");
        if name.ends_with("tests.rs") || name == "golden.rs" || name == "service/parity.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{e}"));
        let mut code = String::new();
        let mut lines = text.lines().peekable();
        while let Some(line) = lines.next() {
            let next = lines.peek().map_or("", |next| next.trim());
            if line.trim() == "#[cfg(test)]" && next.starts_with("mod ") && next.ends_with('{') {
                break;
            }
            code.push_str(line.split("//").next().unwrap_or(""));
        }
        out.push((name, code.split_whitespace().collect()));
    }
    out
}

#[test]
fn no_tui_module_writes_for_itself() {
    let sources = sources();
    assert!(sources.iter().any(|(name, _)| name == "service/shell.rs"));
    for (name, flat) in &sources {
        let needles = STORE.iter().chain(&GIT).chain(&GITHUB).chain(&FILES);
        for needle in needles {
            let allowed = match *needle {
                "save_session(" => name == "service/session.rs",
                _ if FILES.contains(needle) => name == "update.rs",
                _ => false,
            };
            assert!(allowed || !flat.contains(needle), "{name} names {needle}");
        }
    }
}

/// No path the TUI runs says an area is not on this build: every key, hub
/// row and panel goes to the service.
#[test]
fn no_unported_path_is_left() {
    for (name, flat) in &sources() {
        for needle in ["UNPORTED", "notonthisbuild", "fnunported"] {
            assert!(!flat.contains(needle), "{name} names {needle}");
        }
    }
}
