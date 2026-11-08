//! Review submission: the batch review body and its line comments.

use crate::cli::CliGitHub;
use crate::default_handle;
use crate::error::GhError;

/// A single comment entry for a batch PR review submission.
#[derive(Debug, Clone)]
pub struct ReviewBatchEntry {
    pub file: String,
    /// End line of the anchor (GitHub `line` field).
    pub line: usize,
    /// Start line when the comment spans multiple lines (GitHub `start_line`).
    pub start_line: Option<usize>,
    pub body: String,
    pub side: String,
}

/// The body of one `POST /pulls/{pr}/reviews` call.
struct ReviewSubmission<'a> {
    commit_id: &'a str,
    event: &'a str,
    body: &'a str,
    comments: &'a [ReviewBatchEntry],
}

fn pr_review_payload_json(
    commit_id: &str,
    event: &str,
    body: &str,
    comments: &[ReviewBatchEntry],
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "commit_id": commit_id,
        "event": event,
        "body": body,
    });
    if !comments.is_empty() {
        let comment_values: Vec<serde_json::Value> =
            comments.iter().map(review_comment_json).collect();
        payload["comments"] = serde_json::Value::Array(comment_values);
    }
    payload
}

/// One entry of a review's `comments` array, as `POST /pulls/{pr}/reviews`
/// takes it.
pub(crate) fn review_comment_json(entry: &ReviewBatchEntry) -> serde_json::Value {
    let mut obj = serde_json::json!({
        "path": entry.file,
        "line": entry.line,
        "side": entry.side,
        "body": entry.body
    });
    if let Some(start) = entry.start_line {
        if start < entry.line {
            obj["start_line"] = serde_json::json!(start);
            if entry.side == "LEFT" {
                obj["start_side"] = serde_json::json!("LEFT");
            }
        }
    }
    obj
}

/// The head oid `gh pr view` reports, or a typed error.
fn head_oid_args(pr: u64, repo: Option<&str>) -> Vec<String> {
    let mut args = vec!["pr".to_string(), "view".to_string(), pr.to_string()];
    if let Some(repo) = repo {
        args.extend(["--repo".to_string(), repo.to_string()]);
    }
    args.extend([
        "--json".to_string(),
        "headRefOid".to_string(),
        "--jq".to_string(),
        ".headRefOid".to_string(),
    ]);
    args
}

impl CliGitHub {
    fn post_pr_review(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        review: ReviewSubmission<'_>,
        dir: Option<&str>,
    ) -> Result<(), GhError> {
        let payload =
            pr_review_payload_json(review.commit_id, review.event, review.body, review.comments);
        // NamedTempFile: O_EXCL + 0600, so no symlink/race exposure in world-
        // writable /tmp and no world-readable payload. Deleted on drop.
        let mut tmp = tempfile::NamedTempFile::new().map_err(|error| GhError::Local {
            message: format!("Failed to create review payload file: {error}"),
        })?;
        serde_json::to_writer(&mut tmp, &payload).map_err(|error| GhError::Local {
            message: format!("Failed to write review payload: {error}"),
        })?;
        let tmp_path = tmp.path().to_string_lossy().into_owned();
        let args = vec![
            "api".to_string(),
            "-X".to_string(),
            "POST".to_string(),
            format!("repos/{owner}/{repo}/pulls/{pr}/reviews"),
            "--input".to_string(),
            tmp_path,
        ];
        self.run_gh(&args, dir, None).map(|_| ())
    }
}

impl CliGitHub {
    /// Head oid of a remote PR (no local clone needed) — the staleness baseline
    /// for remote tabs.
    pub(crate) fn pr_head_oid_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<String, GhError> {
        let args = head_oid_args(pr, Some(&format!("{owner}/{repo}")));
        let out = self.gh_ok(&args, None)?;
        let oid = out.trim().to_string();
        if oid.is_empty() {
            return Err(GhError::Local {
                message: "Failed to get HEAD SHA from gh pr view: empty output".to_string(),
            });
        }
        Ok(oid)
    }

    /// Submit a batch PR review with multiple comments in one API call.
    ///
    /// `comments` is a slice of `ReviewBatchEntry`. No individual comment ids are
    /// returned by the review API; the next pull rebuilds them from GitHub.
    #[expect(
        clippy::too_many_arguments,
        reason = "the review inputs travel together and match the wrapper's signature"
    )]
    pub(crate) fn submit_review(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comments: &[ReviewBatchEntry],
        repo_root: &str,
        event: &str,
        body: &str,
    ) -> Result<(), GhError> {
        let args = head_oid_args(pr, None);
        let out = self.gh_ok(&args, Some(repo_root))?;
        let commit_id = out.trim().to_string();
        if commit_id.is_empty() {
            return Err(GhError::Local {
                message: "Failed to get HEAD SHA: empty output".to_string(),
            });
        }
        self.post_pr_review(
            owner,
            repo,
            pr,
            ReviewSubmission {
                commit_id: &commit_id,
                event,
                body,
                comments,
            },
            Some(repo_root),
        )
    }

    /// Submit a batch PR review with multiple comments on a remote PR (no local
    /// repo required).
    pub(crate) fn submit_review_remote(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        comments: &[ReviewBatchEntry],
        event: &str,
        body: &str,
    ) -> Result<(), GhError> {
        let args = head_oid_args(pr, Some(&format!("{owner}/{repo}")));
        let out = self.gh_ok(&args, None)?;
        let commit_id = out.trim().to_string();
        if commit_id.is_empty() {
            return Err(GhError::Local {
                message: "Failed to get HEAD SHA: empty output".to_string(),
            });
        }
        self.post_pr_review(
            owner,
            repo,
            pr,
            ReviewSubmission {
                commit_id: &commit_id,
                event,
                body,
                comments,
            },
            None,
        )
    }
}

// ── Compatibility wrappers ──────────────────────────────────────────────────

pub fn gh_pr_head_oid_remote(owner: &str, repo: &str, pr: u64) -> anyhow::Result<String> {
    default_handle()
        .pr_head_oid_remote(owner, repo, pr)
        .map_err(|error| match error {
            GhError::Local { .. } => anyhow::Error::from(error),
            error => anyhow::anyhow!("Failed to get HEAD SHA from gh pr view: {error}"),
        })
}

pub fn gh_pr_submit_review(
    owner: &str,
    repo: &str,
    pr: u64,
    comments: &[ReviewBatchEntry],
    repo_root: &str,
    event: &str,
    body: &str,
) -> anyhow::Result<()> {
    default_handle()
        .submit_review(owner, repo, pr, comments, repo_root, event, body)
        .map_err(|error| match error {
            GhError::Local { .. } => anyhow::Error::from(error),
            error => anyhow::anyhow!("Failed to submit review: {error}"),
        })
}

pub fn gh_pr_submit_review_remote(
    owner: &str,
    repo: &str,
    pr: u64,
    comments: &[ReviewBatchEntry],
    event: &str,
    body: &str,
) -> anyhow::Result<()> {
    default_handle()
        .submit_review_remote(owner, repo, pr, comments, event, body)
        .map_err(|error| match error {
            GhError::Local { .. } => anyhow::Error::from(error),
            error => anyhow::anyhow!("Failed to submit review: {error}"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{GhCall, GhRunner};
    use crate::test_support::output;
    use std::io;
    use std::process::Output;
    use std::sync::Mutex;

    #[derive(Default)]
    struct CapturingRunner {
        payloads: Mutex<Vec<String>>,
    }

    impl GhRunner for CapturingRunner {
        fn run(&self, call: &GhCall<'_>) -> io::Result<Output> {
            if let Some(index) = call.args.iter().position(|arg| arg == "--input") {
                let path = &call.args[index + 1];
                self.payloads
                    .lock()
                    .unwrap()
                    .push(std::fs::read_to_string(path)?);
            }
            if call.args.get(1).is_some_and(|arg| arg == "api") {
                Ok(output(0, "{}", ""))
            } else {
                Ok(output(0, "abc123\n", ""))
            }
        }

        fn spawn_detached(&self, _args: &[String], _dir: Option<&str>) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn submit_review_posts_the_batch_with_the_pinned_commit() {
        let runner = std::sync::Arc::new(CapturingRunner::default());
        let github = crate::CliGitHub::new(runner.clone());
        let comments = [ReviewBatchEntry {
            file: "a.rs".into(),
            line: 3,
            start_line: None,
            body: "hey".into(),
            side: "RIGHT".into(),
        }];
        github
            .submit_review("o", "r", 7, &comments, "/root", "COMMENT", "body")
            .unwrap();
        let payloads = runner.payloads.lock().unwrap();
        assert_eq!(payloads.len(), 1);
        assert!(payloads[0].contains("abc123"), "{}", payloads[0]);
        assert!(payloads[0].contains("a.rs"), "{}", payloads[0]);
        assert!(payloads[0].contains("hey"), "{}", payloads[0]);
    }
}
