//! Reproduction: local GitHub comments vanish when `pr_number` is assigned to a
//! local branch tab AFTER comments were already added.
//!
//! A local-branch tab starts with `pr_number = None`, so its comments land in
//! the branch view bucket. Once the tab learns its PR number the PR bucket is
//! authoritative, and the AI reload both front ends run
//! (`ai::tab_ai::assemble_tab_ai`) must carry the early comments into it
//! rather than orphan them.

use std::path::Path;

use er_engine::ai::annotation_store::load_github_comments;
use er_engine::ai::tab_ai::adopt_pr_bucket_comments;
use er_engine::ai::ErGithubComments;
use serde_json::json;

fn local_comment(id: &str, body: &str) -> serde_json::Value {
    json!({
        "id": id, "file": "f.txt", "hunk_index": 0, "line_start": 2, "line_end": null,
        "comment": body, "source": "local"
    })
}

fn write_comments(bucket: &Path, comments: &[serde_json::Value]) {
    er_engine::test_support::write_sidecar_json::<ErGithubComments>(
        bucket,
        json!({"version": 1, "diff_hash": "hash", "comments": comments}),
    );
}

fn stored(bucket: &Path) -> Option<ErGithubComments> {
    let scope = er_store::Scope::dir(bucket).ok()?;
    match load_github_comments(&er_engine::store_registry::store(), &scope).ok()? {
        er_store::Loaded::Found(stored) => Some(stored.value),
        _ => None,
    }
}

fn bodies(comments: Option<&ErGithubComments>) -> Vec<String> {
    comments
        .map(|gc| gc.comments.iter().map(|c| c.comment.clone()).collect())
        .unwrap_or_default()
}

#[test]
fn local_comments_survive_late_pr_number_assignment() {
    let base = tempfile::tempdir().unwrap();
    let branch = base.path().join("branch");
    let pr = base.path().join("pr");
    std::fs::create_dir_all(&branch).unwrap();
    std::fs::create_dir_all(&pr).unwrap();
    let (branch_dir, pr_dir) = (
        branch.to_string_lossy().into_owned(),
        pr.to_string_lossy().into_owned(),
    );

    er_engine::storage::with_test_storage_root(base.path(), || {
        // Two comments written BEFORE the PR number is known.
        write_comments(
            &branch,
            &[
                local_comment("c1", "comment one"),
                local_comment("c2", "comment two"),
            ],
        );

        // The PR number becomes known and the AI state reloads.
        let adopted = adopt_pr_bucket_comments(stored(&branch), &pr_dir, &branch_dir);
        let after = bodies(adopted.as_ref());
        assert_eq!(
            after.len(),
            2,
            "local comments must survive late pr_number assignment (found {after:?})"
        );
        assert!(after.iter().any(|c| c == "comment one"));
        assert!(after.iter().any(|c| c == "comment two"));
        assert_eq!(
            bodies(stored(&pr).as_ref()),
            after,
            "the PR bucket holds them"
        );
        assert!(stored(&branch).is_none(), "the orphan file is removed");

        // A comment the branch bucket gains later accumulates with the
        // migrated ones.
        write_comments(&branch, &[local_comment("c3", "comment three")]);
        let adopted = adopt_pr_bucket_comments(stored(&branch), &pr_dir, &branch_dir);
        assert_eq!(
            bodies(adopted.as_ref()).len(),
            3,
            "all three local comments should be visible"
        );
    });
}
