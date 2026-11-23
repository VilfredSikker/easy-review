//! The direct commands that read a tab: each names the tab
//! by its key and reads that tab's published content.

use er_protocol::ExportOptions;

use super::annotations_resource_tests::branch_view_of_a_pr;
use super::diff_command_tests::with_tab;
use super::diff_resource_tests::with_storage;
use super::on_a_thread;
use crate::direct::os::{content_source, target, view_bucket, Editor, Target};
use crate::direct::{export, line_commits};

/// The open, reveal, line-commit and export commands each answer
/// from the content of the tab their key names.
#[test]
fn the_keyed_direct_commands_answer() {
    with_tab(|state, (key, tab)| {
        let content = state.service.content(key, &tab).expect("the tab's content");
        let root = content.tab.repo_root.clone();
        let file = std::path::Path::new(&root).join("src").join("lib.rs");
        std::fs::create_dir_all(file.parent().expect("a parent")).unwrap_or_else(|e| panic!("{e}"));
        std::fs::write(&file, "").unwrap_or_else(|e| panic!("{e}"));
        let wire = er_service::wire_tab_key(key);

        let opened = on_a_thread(state, "open_source", move |state| {
            content_source(state, wire)
                .map(|source| target(&source, Some("src/lib.rs"), Some(3), Editor::Default))
        });
        assert_eq!(
            opened,
            Ok(Target::Local {
                root,
                file,
                line: 3
            })
        );

        let bucket = on_a_thread(state, "reveal_er_folder", move |state| {
            view_bucket(state, wire)
        });
        assert_eq!(bucket, Ok(content.context.er_dir.clone()));

        let attributed = on_a_thread(state, "line_commits", move |state| {
            line_commits::keyed_line_commits(state, wire, "src/lib.rs".into(), "k".into())
        });
        let attributed = attributed.unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            (attributed.path.as_str(), attributed.cache_key.as_str()),
            ("src/lib.rs", "k")
        );

        let rendered = on_a_thread(state, "export_review", move |state| {
            export::render_tab(state, wire, &ExportOptions::default())
        });
        let branch = &content.tab.current_branch;
        let rendered = rendered.unwrap_or_else(|e| panic!("{e:?}"));
        assert!(
            rendered.starts_with(&format!("# Review export — {branch}")),
            "{rendered}"
        );
    });
}

/// On a local PR tab's branch view, whose PR bucket is not its view bucket, an
/// export to an empty path writes `export.md` in the view bucket and answers
/// that path.
#[test]
fn an_export_to_no_path_lands_in_the_view_bucket() {
    with_storage(|dir| {
        let (state, (key, tab)) = branch_view_of_a_pr(dir);
        let content = state.service.content(key, &tab).expect("the branch view");
        let (er_dir, comments_dir) = (&content.context.er_dir, &content.context.comments_dir);
        assert_ne!(er_dir, comments_dir, "the fixture's two buckets are one");
        let wire = er_service::wire_tab_key(key);

        let written = export::write_tab(&state, wire, &ExportOptions::default(), "");

        let expected = format!("{er_dir}/export.md");
        assert_eq!(written, Ok(expected.clone()));
        assert!(
            std::path::Path::new(&expected).exists(),
            "nothing at {expected}"
        );
        let in_pr_bucket = std::path::Path::new(comments_dir).join("export.md");
        assert!(!in_pr_bucket.exists(), "the export went to the PR bucket");
    });
}

/// A key the registry does not list answers `NotFound`.
#[test]
fn a_key_nobody_lists_is_not_found() {
    with_tab(|state, _| {
        let stray = er_protocol::TabKey(u64::MAX);
        let answered = view_bucket(state, stray).map_err(|e| e.kind);
        assert_eq!(answered, Err(er_protocol::ErErrorKind::NotFound));
    });
}
