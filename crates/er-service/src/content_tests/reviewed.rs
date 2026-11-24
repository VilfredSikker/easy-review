//! `reviewed(tab)` counts the marks on the diff's files on the server, moves
//! with every section it reads, and carries the view a write compares.

use super::*;
use crate::{content_view, painted_view_matches, reviewed_resource, reviewed_version};

fn section<T>(arc: &mut Arc<T>) -> &mut T {
    Arc::get_mut(arc).unwrap_or_else(|| panic!("a fresh section is shared"))
}

/// A Branch tab whose diff holds `src/a.rs` and `src/b.rs`, with a mark on
/// `src/b.rs` and one on a file the diff no longer holds.
fn marked() -> crate::content::TabContent {
    let mut loaded = loaded_tab("/repo");
    loaded.diff.files = er_engine::git::parse_diff(RAW);
    loaded.marks = HashMap::from([
        ("src/b.rs".to_string(), "h".to_string()),
        ("gone.rs".to_string(), "h".to_string()),
    ]);
    crate::content::TabContent::from_loaded(TabKey::next(), *loaded, 1)
}

#[test]
fn the_counts_take_only_the_marks_on_the_diffs_files() {
    let reviewed = reviewed_resource(&marked());
    assert_eq!(reviewed.paths, ["src/b.rs"]);
    assert_eq!((reviewed.reviewed_count, reviewed.total_count), (1, 2));
}

/// A reload that changes the file list moves the count without a reviewed
/// write, so the diff's revision is part of the version, as are the marks'
/// and the recorded mode's.
#[test]
fn each_section_the_marks_read_raises_their_version() {
    let base = reviewed_version(&marked());
    for name in ["diff", "reviewed", "mode"] {
        let mut content = marked();
        match name {
            "diff" => section(&mut content.diff).revision += 5,
            "reviewed" => section(&mut content.reviewed).revision += 5,
            _ => content.mode_revision += 5,
        }
        assert!(reviewed_version(&content) > base, "{name} left {base}");
    }
}

#[test]
fn a_painted_view_matches_only_the_view_the_content_shows() {
    let content = marked();
    let painted = content_view(&content);
    assert_eq!(
        (
            painted.mode.as_str(),
            painted.branch.as_str(),
            painted.pr_number
        ),
        ("branch", "", None)
    );
    assert!(painted_view_matches(&content, &painted));
    for (name, view) in [
        (
            "mode",
            er_protocol::PaintedView {
                mode: "staged".into(),
                ..painted.clone()
            },
        ),
        (
            "repo",
            er_protocol::PaintedView {
                repo_root: "/other".into(),
                ..painted.clone()
            },
        ),
        (
            "pr",
            er_protocol::PaintedView {
                pr_number: Some(7),
                ..painted.clone()
            },
        ),
        (
            "branch",
            er_protocol::PaintedView {
                branch: "main".into(),
                ..painted.clone()
            },
        ),
    ] {
        assert!(
            !painted_view_matches(&content, &view),
            "another {name} matched"
        );
    }
}

/// Two reviewed writes that publish in the other order than they saved in:
/// the view ends on the newer marks.
#[test]
fn a_reviewed_write_finishing_late_drops_behind_the_newer_one() -> anyhow::Result<()> {
    use crate::annotations::{marks_patch, update_marks};
    let tmp = tempfile::tempdir()?;
    let store = er_store::ArtifactStore::new(tmp.path());
    let scope = er_store::Scope::dir(tmp.path())?;
    let mark = |path: &'static str| {
        update_marks(&store, &scope, |map| {
            map.insert(path.to_string(), "h".to_string());
            1
        })
    };
    let (older, newer) = (mark("src/a.rs")?, mark("src/b.rs")?);
    let saved = newer.map.clone();

    let mut state = WorkspaceState::new();
    let key = TabKey::next();
    let loaded = loaded_tab("/repo");
    let ctx = loaded.context.clone();
    let first = ContentPatch::Loaded {
        key,
        since: None,
        loaded,
    };
    for patch in [
        first,
        marks_patch(&ctx, newer)?.1,
        marks_patch(&ctx, older)?.1,
    ] {
        state
            .apply_patch(patch)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
    }
    assert_eq!(content_of(state.view(), key)?.reviewed.marks, saved);
    Ok(())
}
