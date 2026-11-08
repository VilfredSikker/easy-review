//! The reviewer's GitHub pending review: line comments collected before one
//! Comment / Approve / Request changes decision posts them together.
//!
//! GitHub holds the draft (a review in state PENDING, visible only to its
//! author), not a sidecar. So it survives a restart, matches what github.com
//! shows as "Pending", and a second device sees the same list. GitHub allows
//! one pending review per reviewer per PR, which is why the first comment
//! starts it and every later one is appended to it.

use crate::cli::CliGitHub;
use crate::default_handle;
use crate::error::GhError;
use crate::review::{review_comment_json, ReviewBatchEntry};
use serde::Serialize;
use std::collections::HashMap;

/// The viewer's pending review on one PR, plus the request that asked for it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct PendingReview {
    /// REST id of the pending review; `None` until a first comment starts one.
    pub review_id: Option<u64>,
    /// GraphQL id of the pending review, which the append mutation takes.
    #[serde(skip)]
    pub review_node_id: Option<String>,
    pub comments: Vec<PendingReviewComment>,
    /// The `gh` login the review belongs to.
    pub viewer: Option<String>,
    /// The latest request for the viewer's review, when one was made.
    pub review_request: Option<ReviewRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PendingReviewComment {
    /// GraphQL id; removal goes through the GraphQL mutation, which accepts
    /// a comment that is still pending.
    pub id: String,
    pub database_id: Option<u64>,
    pub path: String,
    pub line: Option<u64>,
    pub start_line: Option<u64>,
    /// "LEFT" | "RIGHT"
    pub side: String,
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ReviewRequest {
    pub requested_by: String,
    /// ISO-8601.
    pub requested_at: String,
}

const PENDING_REVIEW_QUERY: &str = r#"query($owner:String!,$repo:String!,$pr:Int!){
  viewer{login}
  repository(owner:$owner,name:$repo){
    pullRequest(number:$pr){
      reviews(states:PENDING,first:10){nodes{id databaseId viewerDidAuthor
        comments(first:100){nodes{id databaseId path line startLine originalLine originalStartLine body}}}}
      reviewThreads(last:100){nodes{diffSide comments(last:50){nodes{id}}}}
      timelineItems(itemTypes:[REVIEW_REQUESTED_EVENT],last:50){nodes{
        ... on ReviewRequestedEvent{createdAt actor{login} requestedReviewer{... on User{login}}}}}
    }
  }
}"#;

const ADD_THREAD_MUTATION: &str = r#"mutation($review:ID!,$path:String!,$body:String!,$line:Int!,$side:DiffSide!,$startLine:Int,$startSide:DiffSide){
  addPullRequestReviewThread(input:{pullRequestReviewId:$review,path:$path,body:$body,line:$line,side:$side,startLine:$startLine,startSide:$startSide}){thread{id}}
}"#;

const DELETE_COMMENT_MUTATION: &str = r#"mutation($id:ID!){
  deletePullRequestReviewComment(input:{id:$id}){clientMutationId}
}"#;

/// GitHub rejects a Comment or Request changes review that says nothing: no
/// summary and no line comments. Approve may be empty.
pub fn review_body_required(event: &str, comment_count: usize) -> bool {
    event != "APPROVE" && comment_count == 0
}

/// A review payload without `event`, which GitHub stores as PENDING.
fn start_review_payload(commit_id: &str, entry: &ReviewBatchEntry) -> serde_json::Value {
    serde_json::json!({
        "commit_id": commit_id,
        "comments": [review_comment_json(entry)],
    })
}

/// `-f` (string) and `-F` (typed) fields for the append mutation. A range
/// sends `startLine`; a single line leaves it unset, which GraphQL reads as
/// null.
fn add_thread_fields(review_node_id: &str, entry: &ReviewBatchEntry) -> (Vec<String>, Vec<String>) {
    let mut strings = vec![
        format!("query={ADD_THREAD_MUTATION}"),
        format!("review={review_node_id}"),
        format!("path={}", entry.file),
        format!("body={}", entry.body),
        format!("side={}", entry.side),
    ];
    let mut ints = vec![format!("line={}", entry.line)];
    if let Some(start) = entry.start_line.filter(|s| *s < entry.line) {
        ints.push(format!("startLine={start}"));
        strings.push(format!("startSide={}", entry.side));
    }
    (strings, ints)
}

fn parse_pending_review(json: &str) -> Result<PendingReview, GhError> {
    let v: serde_json::Value = serde_json::from_str(json).map_err(|_| GhError::Local {
        message: "Failed to parse the pending review response".to_string(),
    })?;
    let data = &v["data"];
    let viewer = data["viewer"]["login"].as_str().map(str::to_string);
    let pr = &data["repository"]["pullRequest"];
    if pr.is_null() {
        return Err(GhError::Local {
            message: "Pull request not found".to_string(),
        });
    }

    let mut out = PendingReview {
        viewer: viewer.clone(),
        ..PendingReview::default()
    };
    // Pending reviews are only ever the viewer's, but the flag keeps a
    // future API change from handing someone else's draft to this one.
    let review = pr["reviews"]["nodes"].as_array().and_then(|nodes| {
        nodes
            .iter()
            .find(|r| r["viewerDidAuthor"].as_bool() != Some(false))
    });
    if let Some(review) = review {
        // GitHub keeps the side on the thread, not on the comment: asking a
        // comment for `diffSide` fails the whole query.
        let sides = thread_sides(&pr["reviewThreads"]["nodes"]);
        out.review_id = review["databaseId"].as_u64();
        out.review_node_id = review["id"].as_str().map(str::to_string);
        out.comments = review["comments"]["nodes"]
            .as_array()
            .map(|nodes| {
                nodes
                    .iter()
                    .filter_map(|n| parse_pending_comment(n, &sides))
                    .collect()
            })
            .unwrap_or_default();
    }

    if let Some(me) = viewer.as_deref() {
        out.review_request = pr["timelineItems"]["nodes"]
            .as_array()
            .and_then(|nodes| {
                nodes.iter().rev().find(|n| {
                    n["requestedReviewer"]["login"]
                        .as_str()
                        .is_some_and(|l| l.eq_ignore_ascii_case(me))
                })
            })
            .and_then(|n| {
                Some(ReviewRequest {
                    requested_by: n["actor"]["login"].as_str()?.to_string(),
                    requested_at: n["createdAt"].as_str()?.to_string(),
                })
            });
    }
    Ok(out)
}

/// Comment node id -> the `diffSide` of the thread it belongs to.
fn thread_sides(threads: &serde_json::Value) -> HashMap<String, String> {
    let mut sides = HashMap::new();
    for thread in threads.as_array().into_iter().flatten() {
        let Some(side) = thread["diffSide"].as_str() else {
            continue;
        };
        for c in thread["comments"]["nodes"].as_array().into_iter().flatten() {
            if let Some(id) = c["id"].as_str() {
                sides.insert(id.to_string(), side.to_string());
            }
        }
    }
    sides
}

fn parse_pending_comment(
    n: &serde_json::Value,
    sides: &HashMap<String, String>,
) -> Option<PendingReviewComment> {
    let id = n["id"].as_str()?.to_string();
    // A pending comment's `line` is null once the head moves past it; the
    // original anchor is still the one GitHub will post it on.
    let line = n["line"].as_u64().or_else(|| n["originalLine"].as_u64());
    let start_line = n["startLine"]
        .as_u64()
        .or_else(|| n["originalStartLine"].as_u64());
    Some(PendingReviewComment {
        side: sides
            .get(&id)
            .cloned()
            .unwrap_or_else(|| "RIGHT".to_string()),
        id,
        database_id: n["databaseId"].as_u64(),
        path: n["path"].as_str()?.to_string(),
        line,
        start_line,
        body: n["body"].as_str().unwrap_or_default().to_string(),
    })
}

impl CliGitHub {
    fn post_json(&self, path: &str, payload: &serde_json::Value) -> Result<(), GhError> {
        // A temp file rather than an argument: the body is user text, and a
        // NamedTempFile is 0600 and O_EXCL (see `post_pr_review`).
        let mut tmp = tempfile::NamedTempFile::new().map_err(|error| GhError::Local {
            message: format!("Failed to create review payload file: {error}"),
        })?;
        serde_json::to_writer(&mut tmp, payload).map_err(|error| GhError::Local {
            message: format!("Failed to write review payload: {error}"),
        })?;
        let tmp_path = tmp.path().to_string_lossy().into_owned();
        let args = vec![
            "api".to_string(),
            "-X".to_string(),
            "POST".to_string(),
            path.to_string(),
            "--input".to_string(),
            tmp_path,
        ];
        self.run_gh(&args, None, None).map(|_| ())
    }
}

impl CliGitHub {
    /// The viewer's pending review on `owner/repo#pr`. An empty review (no
    /// `review_id`) when none has been started.
    pub(crate) fn pending_review(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
    ) -> Result<PendingReview, GhError> {
        let stdout = self.graphql(
            PENDING_REVIEW_QUERY,
            &[("owner", owner), ("repo", repo)],
            &[("pr", &pr.to_string())],
        )?;
        parse_pending_review(&stdout)
    }

    /// Add one line comment to the viewer's pending review, starting the review
    /// when there is none yet.
    pub(crate) fn pending_review_add_comment(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        entry: &ReviewBatchEntry,
    ) -> Result<(), GhError> {
        let current = self.pending_review(owner, repo, pr)?;
        match current.review_node_id {
            Some(review) => {
                let (strings, ints) = add_thread_fields(&review, entry);
                let mut args = vec!["api".to_string(), "graphql".to_string()];
                for value in strings {
                    args.extend(["-f".to_string(), value]);
                }
                for value in ints {
                    args.extend(["-F".to_string(), value]);
                }
                self.run_gh(&args, None, None).map(|_| ())
            }
            None => {
                let commit_id = self.pr_head_oid_remote(owner, repo, pr)?;
                let payload = start_review_payload(&commit_id, entry);
                self.post_json(
                    &format!("repos/{owner}/{repo}/pulls/{pr}/reviews"),
                    &payload,
                )
            }
        }
    }

    /// Drop one comment from the pending review. The review itself stays, empty,
    /// until it is submitted.
    pub(crate) fn pending_review_remove_comment(
        &self,
        comment_node_id: &str,
    ) -> Result<(), GhError> {
        self.graphql(DELETE_COMMENT_MUTATION, &[("id", comment_node_id)], &[])
            .map(|_| ())
    }

    /// Submit the viewer's review with `event`, posting its pending comments with
    /// it. Without a pending review this is a plain review carrying only the
    /// summary. Returns how many line comments went out with it.
    pub(crate) fn pending_review_submit(
        &self,
        owner: &str,
        repo: &str,
        pr: u64,
        event: &str,
        body: &str,
    ) -> Result<usize, GhError> {
        let current = self.pending_review(owner, repo, pr)?;
        let count = current.comments.len();
        if review_body_required(event, count) && body.trim().is_empty() {
            return Err(GhError::Local {
                message:
                    "GitHub requires a summary for this review when it carries no line comments"
                        .to_string(),
            });
        }
        match current.review_id {
            Some(id) => {
                let args = vec![
                    "api".to_string(),
                    "-X".to_string(),
                    "POST".to_string(),
                    format!("repos/{owner}/{repo}/pulls/{pr}/reviews/{id}/events"),
                    "-f".to_string(),
                    format!("event={event}"),
                    "-f".to_string(),
                    format!("body={body}"),
                ];
                self.run_gh(&args, None, None)?;
            }
            None => self.submit_review_remote(owner, repo, pr, &[], event, body)?,
        }
        Ok(count)
    }
}

// Compatibility wrappers, kept for the front ends and `app/state`.

pub fn gh_pending_review(owner: &str, repo: &str, pr: u64) -> anyhow::Result<PendingReview> {
    default_handle()
        .pending_review(owner, repo, pr)
        .map_err(|error| anyhow::anyhow!("Failed to read the pending review: {error}"))
}

pub fn gh_pending_review_add_comment(
    owner: &str,
    repo: &str,
    pr: u64,
    entry: &ReviewBatchEntry,
) -> anyhow::Result<()> {
    default_handle()
        .pending_review_add_comment(owner, repo, pr, entry)
        .map_err(|error| {
            anyhow::anyhow!("Failed to add the comment to your pending review: {error}")
        })
}

pub fn gh_pending_review_remove_comment(comment_node_id: &str) -> anyhow::Result<()> {
    default_handle()
        .pending_review_remove_comment(comment_node_id)
        .map_err(|error| {
            anyhow::anyhow!("Failed to remove the comment from your pending review: {error}")
        })
}

pub fn gh_pending_review_submit(
    owner: &str,
    repo: &str,
    pr: u64,
    event: &str,
    body: &str,
) -> anyhow::Result<usize> {
    default_handle()
        .pending_review_submit(owner, repo, pr, event, body)
        .map_err(|error| anyhow::anyhow!("Failed to submit the pending review: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_reads_side_from_threads_not_comments() {
        let comments = PENDING_REVIEW_QUERY
            .split("comments(first:100)")
            .nth(1)
            .and_then(|rest| rest.split('}').next())
            .unwrap();
        assert!(!comments.contains("diffSide"));
        assert!(PENDING_REVIEW_QUERY.contains("reviewThreads"));
    }

    #[test]
    fn comment_without_a_thread_defaults_to_right() {
        let json = r#"{"data":{"viewer":{"login":"vsd"},"repository":{"pullRequest":{
          "reviews":{"nodes":[{"id":"PRR_1","databaseId":1,"viewerDidAuthor":true,"comments":{"nodes":[
            {"id":"PRRC_z","databaseId":9,"path":"a.rs","line":3,"body":"x"}]}}]},
          "reviewThreads":{"nodes":[]},"timelineItems":{"nodes":[]}}}}}"#;
        assert_eq!(
            parse_pending_review(json).unwrap().comments[0].side,
            "RIGHT"
        );
    }

    fn entry(line: usize, start: Option<usize>, side: &str) -> ReviewBatchEntry {
        ReviewBatchEntry {
            file: "src/lib.rs".into(),
            line,
            start_line: start,
            body: "why not a match?".into(),
            side: side.into(),
        }
    }

    const RESPONSE: &str = r#"{"data":{"viewer":{"login":"vsd"},"repository":{"pullRequest":{
      "reviews":{"nodes":[{"id":"PRR_1","databaseId":77,"viewerDidAuthor":true,"comments":{"nodes":[
        {"id":"PRRC_a","databaseId":5,"path":"src/a.rs","line":12,"startLine":10,"originalLine":12,"originalStartLine":10,"body":"range"},
        {"id":"PRRC_b","databaseId":6,"path":"src/b.rs","line":null,"startLine":null,"originalLine":4,"originalStartLine":null,"body":"moved"}
      ]}}]},
      "reviewThreads":{"nodes":[
        {"diffSide":"RIGHT","comments":{"nodes":[{"id":"PRRC_a"}]}},
        {"diffSide":"LEFT","comments":{"nodes":[{"id":"PRRC_b"}]}}
      ]},
      "timelineItems":{"nodes":[
        {"createdAt":"2026-10-01T10:00:00Z","actor":{"login":"sune"},"requestedReviewer":{"login":"VSD"}},
        {"createdAt":"2026-10-02T10:00:00Z","actor":{"login":"martin"},"requestedReviewer":{"login":"someone"}},
        {"createdAt":"2026-10-03T10:00:00Z","actor":{"login":"sune"},"requestedReviewer":{}}
      ]}
    }}}}"#;

    #[test]
    fn parses_the_viewers_pending_review_and_request() {
        let r = parse_pending_review(RESPONSE).unwrap();
        assert_eq!(r.review_id, Some(77));
        assert_eq!(r.review_node_id.as_deref(), Some("PRR_1"));
        assert_eq!(r.viewer.as_deref(), Some("vsd"));
        assert_eq!(r.comments.len(), 2);
        assert_eq!(r.comments[0].line, Some(12));
        assert_eq!(r.comments[0].start_line, Some(10));
        // An outdated pending comment keeps its original anchor.
        assert_eq!(r.comments[1].line, Some(4));
        assert_eq!(r.comments[1].side, "LEFT");
        let req = r.review_request.unwrap();
        assert_eq!(req.requested_by, "sune");
        assert_eq!(req.requested_at, "2026-10-01T10:00:00Z");
    }

    #[test]
    fn no_pending_review_is_an_empty_one() {
        let json = r#"{"data":{"viewer":{"login":"vsd"},"repository":{"pullRequest":{
          "reviews":{"nodes":[]},"timelineItems":{"nodes":[]}}}}}"#;
        let r = parse_pending_review(json).unwrap();
        assert_eq!(r.review_id, None);
        assert!(r.comments.is_empty());
        assert_eq!(r.review_request, None);
    }

    #[test]
    fn ignores_a_review_the_viewer_did_not_write() {
        let json = r#"{"data":{"viewer":{"login":"vsd"},"repository":{"pullRequest":{
          "reviews":{"nodes":[{"id":"PRR_x","databaseId":1,"viewerDidAuthor":false,"comments":{"nodes":[]}}]},
          "timelineItems":{"nodes":[]}}}}}"#;
        assert_eq!(parse_pending_review(json).unwrap().review_id, None);
    }

    #[test]
    fn a_missing_pr_is_an_error() {
        let json = r#"{"data":{"viewer":{"login":"vsd"},"repository":{"pullRequest":null}}}"#;
        assert!(parse_pending_review(json).is_err());
    }

    #[test]
    fn starting_a_review_omits_the_event_so_it_stays_pending() {
        let p = start_review_payload("abc", &entry(9, Some(7), "LEFT"));
        assert!(p.get("event").is_none());
        assert_eq!(p["commit_id"], "abc");
        assert_eq!(p["comments"][0]["start_line"], 7);
        assert_eq!(p["comments"][0]["start_side"], "LEFT");
    }

    #[test]
    fn append_fields_send_a_start_line_only_for_ranges() {
        let (s, i) = add_thread_fields("PRR_1", &entry(4, None, "RIGHT"));
        assert!(s.contains(&"review=PRR_1".to_string()));
        assert!(s.contains(&"side=RIGHT".to_string()));
        assert_eq!(i, vec!["line=4".to_string()]);

        let (s, i) = add_thread_fields("PRR_1", &entry(9, Some(7), "RIGHT"));
        assert!(i.contains(&"startLine=7".to_string()));
        assert!(s.contains(&"startSide=RIGHT".to_string()));
    }

    #[test]
    fn only_an_empty_comment_or_changes_review_needs_a_summary() {
        assert!(!review_body_required("APPROVE", 0));
        assert!(review_body_required("COMMENT", 0));
        assert!(review_body_required("REQUEST_CHANGES", 0));
        assert!(!review_body_required("REQUEST_CHANGES", 2));
    }
}
