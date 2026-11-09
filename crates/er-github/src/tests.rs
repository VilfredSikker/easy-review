//! Tests moved from `legacy.rs` when it was split into concern modules.
//!
//! They live in one module because they span every concern and share a few
//! fixtures. Private helpers they exercise are `pub(crate)` in their module.

use crate::checkout::{fetch_pr_base_ref_with, pin_pr_base_commit};
use crate::comments::{parse_paginated, parse_review_threads_response, run_parallel};
use crate::diff::{gh_diff_too_large, git_merge_base_diff};
use crate::repo::{owner_repo_matches_slug, parse_owner_repo_from_remote, remote_matches_repo};
use crate::*;
use std::collections::HashMap;
use std::sync::Arc;

#[test]
fn run_parallel_overlaps_independent_work() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{Duration, Instant};

    let concurrent = AtomicUsize::new(0);
    let max_concurrent = AtomicUsize::new(0);
    let start = Instant::now();
    let (left, right) = run_parallel(
        || {
            let n = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            max_concurrent.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            concurrent.fetch_sub(1, Ordering::SeqCst);
            1u8
        },
        || {
            let n = concurrent.fetch_add(1, Ordering::SeqCst) + 1;
            max_concurrent.fetch_max(n, Ordering::SeqCst);
            std::thread::sleep(Duration::from_millis(40));
            concurrent.fetch_sub(1, Ordering::SeqCst);
            2u8
        },
    );
    assert_eq!((left, right), (1, 2));
    assert_eq!(
        max_concurrent.load(Ordering::SeqCst),
        2,
        "both closures should run at the same time"
    );
    assert!(
        start.elapsed() < Duration::from_millis(70),
        "sequential 40ms+40ms should overlap; took {:?}",
        start.elapsed()
    );
}

// ── local checkout matching (remote PR reviews) ──

#[test]
fn ssh_and_https_origins_resolve_to_the_same_slug() {
    for remote in [
        "git@github.com:Acme/discovery.git",
        "https://github.com/Acme/discovery.git",
        "https://github.com/Acme/discovery",
    ] {
        let (owner, repo) = parse_owner_repo_from_remote(remote).unwrap();
        assert_eq!((owner.as_str(), repo.as_str()), ("Acme", "discovery"));
    }
}

#[test]
fn slug_match_is_case_insensitive() {
    assert!(owner_repo_matches_slug(
        "Acme",
        "Discovery",
        "acme/discovery"
    ));
    assert!(owner_repo_matches_slug(
        "acme",
        "discovery",
        "Acme/Discovery"
    ));
    assert!(owner_repo_matches_slug(
        "acme",
        "discovery",
        "acme/discovery.git"
    ));
}

#[test]
fn slug_match_rejects_other_repos_and_malformed_slugs() {
    assert!(!owner_repo_matches_slug(
        "acme",
        "discovery",
        "acme/discovery-api"
    ));
    assert!(!owner_repo_matches_slug(
        "acme",
        "discovery",
        "other/discovery"
    ));
    assert!(!owner_repo_matches_slug("acme", "discovery", "discovery"));
    assert!(!owner_repo_matches_slug("acme", "discovery", ""));
}

#[test]
fn local_checkout_for_repo_needs_both_inputs() {
    let github =
        crate::FakeGitHub::default().with_get_repo_info(Ok(("acme".into(), "discovery".into())));
    assert_eq!(
        crate::GitHub::local_checkout_for_repo(&github, &er_git::CliGit, "", "acme/discovery"),
        None
    );
    assert_eq!(
        crate::GitHub::local_checkout_for_repo(&github, &er_git::CliGit, "/tmp/some-repo", ""),
        None
    );
    assert_eq!(
        crate::GitHub::local_checkout_for_repo(
            &github,
            &er_git::CliGit,
            "/tmp/some-repo",
            "acme/discovery"
        ),
        Some("/tmp/some-repo".to_string())
    );
}

#[test]
fn parse_standard_url() {
    let pr = parse_github_pr_url("https://github.com/owner/repo/pull/42").unwrap();
    assert_eq!(pr.owner, "owner");
    assert_eq!(pr.repo, "repo");
    assert_eq!(pr.number, 42);
}

#[test]
fn parse_url_with_trailing_path() {
    let pr = parse_github_pr_url("https://github.com/owner/repo/pull/42/files").unwrap();
    assert_eq!(pr.number, 42);
}

#[test]
fn parse_url_with_commits() {
    let pr = parse_github_pr_url("https://github.com/owner/repo/pull/42/commits").unwrap();
    assert_eq!(pr.number, 42);
}

#[test]
fn parse_url_no_scheme() {
    let pr = parse_github_pr_url("github.com/owner/repo/pull/42").unwrap();
    assert_eq!(pr.owner, "owner");
    assert_eq!(pr.repo, "repo");
    assert_eq!(pr.number, 42);
}

#[test]
fn parse_http_url() {
    let pr = parse_github_pr_url("http://github.com/owner/repo/pull/99").unwrap();
    assert_eq!(pr.number, 99);
}

#[test]
fn parse_invalid_not_github() {
    assert!(parse_github_pr_url("https://gitlab.com/owner/repo/pull/42").is_none());
}

#[test]
fn parse_invalid_not_pull() {
    assert!(parse_github_pr_url("https://github.com/owner/repo/issues/42").is_none());
}

#[test]
fn parse_invalid_no_number() {
    assert!(parse_github_pr_url("https://github.com/owner/repo/pull/").is_none());
}

#[test]
fn parse_invalid_non_numeric() {
    assert!(parse_github_pr_url("https://github.com/owner/repo/pull/abc").is_none());
}

#[test]
fn parse_invalid_too_short() {
    assert!(parse_github_pr_url("https://github.com/owner").is_none());
}

#[test]
fn parse_invalid_empty() {
    assert!(parse_github_pr_url("").is_none());
}

#[test]
fn parse_invalid_zero_pr_number() {
    assert!(parse_github_pr_url("https://github.com/owner/repo/pull/0").is_none());
}

#[test]
fn is_github_pr_url_true() {
    assert!(is_github_pr_url("https://github.com/owner/repo/pull/1"));
}

#[test]
fn is_github_pr_url_false() {
    assert!(!is_github_pr_url("/some/local/path"));
    assert!(!is_github_pr_url("not-a-url"));
}

#[test]
fn is_no_upstream_to_refresh_only_matches_no_upstream_errors() {
    let no_upstream = anyhow::anyhow!("Branch 'feature/a' has no upstream to refresh");
    let fetch_failed =
        anyhow::anyhow!("Failed to fetch upstream for 'feature/a' from 'origin': failed");

    assert!(is_no_upstream_to_refresh(&no_upstream));
    assert!(!is_no_upstream_to_refresh(&fetch_failed));
}

#[test]
fn parse_review_threads_response_extracts_resolved_and_outdated_state() {
    let json = r#"{
        "data": {
            "repository": {
                "pullRequest": {
                    "reviewThreads": {
                        "nodes": [
                            {
                                "isResolved": false,
                                "isOutdated": true,
                                "comments": {
                                    "nodes": [
                                        { "databaseId": 3188301632 }
                                    ]
                                }
                            },
                            {
                                "isResolved": true,
                                "isOutdated": false,
                                "comments": {
                                    "nodes": [
                                        { "databaseId": 3188301633 },
                                        { "databaseId": null }
                                    ]
                                }
                            }
                        ]
                    }
                }
            }
        }
    }"#;

    let state = parse_review_threads_response(json).unwrap();

    assert_eq!(
        state.get(&3188301632),
        Some(&ReviewThreadState {
            resolved: false,
            outdated: true,
        })
    );
    assert_eq!(
        state.get(&3188301633),
        Some(&ReviewThreadState {
            resolved: true,
            outdated: false,
        })
    );
    assert!(!state.contains_key(&0));
}

// ── remote_matches_repo ──

#[test]
fn remote_matches_https_url() {
    assert!(remote_matches_repo(
        "https://github.com/owner/repo.git",
        "owner",
        "repo"
    ));
}

#[test]
fn remote_matches_ssh_url() {
    assert!(remote_matches_repo(
        "git@github.com:owner/repo.git",
        "owner",
        "repo"
    ));
}

#[test]
fn remote_no_false_positive_substring_owner() {
    // "other-owner/repo" must not match "owner/repo"
    assert!(!remote_matches_repo(
        "https://github.com/other-owner/repo.git",
        "owner",
        "repo"
    ));
}

#[test]
fn remote_no_false_positive_substring_ssh() {
    assert!(!remote_matches_repo(
        "git@github.com:other-owner/repo.git",
        "owner",
        "repo"
    ));
}

#[test]
fn remote_matches_https_without_git_suffix() {
    assert!(remote_matches_repo(
        "https://github.com/owner/repo",
        "owner",
        "repo"
    ));
}

// ── PrOverviewData struct ──

#[test]
fn pr_overview_data_fields_accessible() {
    let data = PrOverviewData {
        number: 42,
        title: "Fix the bug".to_string(),
        body: "Detailed description".to_string(),
        state: "OPEN".to_string(),
        author: "contributor".to_string(),
        url: "https://github.com/owner/repo/pull/42".to_string(),
        base_branch: "main".to_string(),
        head_branch: "fix/the-bug".to_string(),
        checks: vec![],
        reviewers: vec![],
    };
    assert_eq!(data.number, 42);
    assert_eq!(data.title, "Fix the bug");
    assert_eq!(data.state, "OPEN");
    assert_eq!(data.author, "contributor");
    assert_eq!(data.base_branch, "main");
    assert_eq!(data.head_branch, "fix/the-bug");
    assert!(data.checks.is_empty());
    assert!(data.reviewers.is_empty());
}

#[test]
fn parse_pr_commits_reverses_chronological_github_order() {
    let raw = br#"{
      "commits": [
        {
          "oid": "1111111111111111111111111111111111111111",
          "messageHeadline": "old commit",
          "authors": [{"login": "ada"}],
          "committedDate": "2026-06-01T10:00:00Z"
        },
        {
          "oid": "2222222222222222222222222222222222222222",
          "messageHeadline": "new commit",
          "authors": [{"name": "Grace Hopper"}],
          "committedDate": "2026-06-02T10:00:00Z"
        }
      ]
    }"#;

    let commits = parse_pr_commits_view_json(raw, 10).expect("parse commits");

    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].hash, "2222222222222222222222222222222222222222");
    assert_eq!(commits[0].short_hash, "2222222");
    assert_eq!(commits[0].subject, "new commit");
    assert_eq!(commits[0].author, "Grace Hopper");
    assert_eq!(commits[1].hash, "1111111111111111111111111111111111111111");
    assert_eq!(commits[1].author, "ada");
}

#[test]
fn parse_pr_commits_view_marks_diffstat_unknown() {
    let raw = br#"{"commits": [{"oid": "1111111111111111111111111111111111111111"}]}"#;
    let commits = parse_pr_commits_view_json(raw, 10).expect("parse commits");
    assert!(!commits[0].diffstat_known);
}

#[test]
fn parse_pr_commits_graphql_reads_every_page_with_diffstat() {
    let page = |oid: &str, subject: &str, adds: u64, more: bool| {
        format!(
            r#"{{"data":{{"repository":{{"pullRequest":{{"commits":{{
              "pageInfo":{{"hasNextPage":{more},"endCursor":"c"}},
              "nodes":[{{"commit":{{"oid":"{oid}","messageHeadline":"{subject}",
                "committedDate":"2026-06-01T10:00:00Z","additions":{adds},"deletions":2,
                "changedFilesIfAvailable":3,
                "authors":{{"nodes":[{{"name":"Ada","user":{{"login":"ada"}}}},
                                     {{"name":"Ghost","user":null}}]}}}}}}]}}}}}}}}}}"#
        )
    };
    // `gh api --paginate` concatenates one document per page.
    let raw = format!(
        "{}\n{}",
        page("1111111111111111111111111111111111111111", "old", 5, true),
        page("2222222222222222222222222222222222222222", "new", 0, false),
    );

    let commits = parse_pr_commits_graphql(raw.as_bytes(), 10).expect("parse commits");

    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0].subject, "new");
    assert_eq!((commits[0].adds, commits[0].dels), (0, 2));
    assert!(commits[0].diffstat_known);
    assert_eq!(commits[1].subject, "old");
    assert_eq!(
        (commits[1].file_count, commits[1].adds, commits[1].dels),
        (3, 5, 2)
    );
    assert_eq!(commits[1].author, "ada, Ghost");
}

#[test]
fn combined_status_json_parses_overview_comments_reviews_independently() {
    let json = r#"{
        "number": 42,
        "title": "Fix the bug",
        "body": "Detailed description",
        "state": "OPEN",
        "isDraft": false,
        "author": { "login": "contributor" },
        "reviewDecision": "APPROVED",
        "mergeable": "MERGEABLE",
        "headRefName": "fix/the-bug",
        "baseRefName": "main",
        "labels": [{ "name": "bug" }, { "name": "p1" }],
        "url": "https://github.com/owner/repo/pull/42",
        "comments": [
            {
                "author": { "login": "alice" },
                "body": "looks good",
                "createdAt": "2026-06-01T10:00:00Z",
                "url": "https://github.com/owner/repo/pull/42#issuecomment-1"
            },
            {
                "author": { "login": "bob" },
                "body": "one nit",
                "createdAt": "2026-06-02T11:00:00Z",
                "url": "https://github.com/owner/repo/pull/42#issuecomment-2"
            }
        ],
        "reviews": [
            {
                "author": { "login": "carol" },
                "state": "CHANGES_REQUESTED",
                "body": "needs work",
                "submittedAt": "2026-06-01T12:00:00Z"
            },
            {
                "author": { "login": "carol" },
                "state": "APPROVED",
                "body": "lgtm now",
                "submittedAt": "2026-06-03T09:00:00Z"
            }
        ]
    }"#;

    let overview = parse_pr_overview(json).unwrap();
    assert_eq!(overview.number, 42);
    assert_eq!(overview.author, "contributor");
    assert_eq!(overview.review_decision.as_deref(), Some("APPROVED"));
    assert_eq!(overview.labels, vec!["bug".to_string(), "p1".to_string()]);

    let comments = parse_pr_comments(json).unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].author, "alice");
    assert_eq!(comments[1].author, "bob");
    assert_eq!(comments[1].body, "one nit");

    let reviews = parse_pr_reviews(json).unwrap();
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[0].state, "CHANGES_REQUESTED");
    assert_eq!(reviews[1].state, "APPROVED");
    assert_eq!(reviews[1].submitted_at, "2026-06-03T09:00:00Z");
}

#[test]
fn ci_check_fields_accessible() {
    let check = CiCheck {
        name: "CI / test".to_string(),
        status: "completed".to_string(),
        conclusion: Some("success".to_string()),
    };
    assert_eq!(check.name, "CI / test");
    assert_eq!(check.status, "completed");
    assert_eq!(check.conclusion, Some("success".to_string()));
}

#[test]
fn ci_check_conclusion_can_be_none() {
    let check = CiCheck {
        name: "CI / test".to_string(),
        status: "in_progress".to_string(),
        conclusion: None,
    };
    assert!(check.conclusion.is_none());
}

#[test]
fn reviewer_status_fields_accessible() {
    let reviewer = ReviewerStatus {
        login: "octocat".to_string(),
        state: "APPROVED".to_string(),
    };
    assert_eq!(reviewer.login, "octocat");
    assert_eq!(reviewer.state, "APPROVED");
}

// ── gh pr view JSON parsing (unit tests using serde_json directly) ──

#[test]
fn pr_overview_parsed_from_gh_json() {
    // Simulate the JSON output of `gh pr view --json number,title,body,state,author,...`
    let json = r#"{
        "number": 123,
        "title": "Add feature X",
        "body": "This PR adds feature X",
        "state": "OPEN",
        "author": {"login": "developer"},
        "baseRefName": "main",
        "headRefName": "feature/x",
        "reviews": [
            {"author": {"login": "reviewer1"}, "state": "APPROVED"},
            {"author": {"login": "reviewer2"}, "state": "CHANGES_REQUESTED"}
        ]
    }"#;

    // Exercise the REAL parser — a parser regression must fail this test.
    let pr = parse_pr_overview(json).unwrap();
    assert_eq!(pr.number, 123);
    assert_eq!(pr.title, "Add feature X");
    assert_eq!(pr.state, "OPEN");
    assert_eq!(pr.author, "developer");
    assert_eq!(pr.base_ref_name, "main");
    assert_eq!(pr.head_ref_name, "feature/x");
}

#[test]
fn pr_overview_handles_missing_optional_fields() {
    // Minimal JSON with only required fields — defaults must kick in.
    let json = r#"{"number": 1, "title": "T", "state": "OPEN"}"#;
    let pr = parse_pr_overview(json).unwrap();
    assert_eq!(pr.number, 1);
    assert_eq!(pr.author, ""); // missing → empty string default
    assert!(pr.labels.is_empty()); // missing → empty vec
    assert!(!pr.is_draft); // missing → false default
}

// ── parse_pr_overview ──

#[test]
fn parse_pr_overview_full_json() {
    let json = r#"{
        "number": 7,
        "title": "Add feature",
        "state": "OPEN",
        "isDraft": false,
        "author": {"login": "alice"},
        "url": "https://github.com/o/r/pull/7",
        "reviewDecision": "APPROVED",
        "mergeable": "MERGEABLE",
        "headRefName": "feat/x",
        "baseRefName": "main",
        "labels": [{"name": "bug"}, {"name": "ui"}],
        "createdAt": "2026-10-30T09:12:00Z",
        "reviewRequests": [
            {"__typename": "User", "login": "bob"},
            {"__typename": "Team", "name": "Core", "slug": "o/core"}
        ],
        "commits": [
            {"oid": "aaa", "messageHeadline": "first"},
            {"oid": "bbb", "messageHeadline": "second"}
        ]
    }"#;
    let pr = parse_pr_overview(json).unwrap();
    assert_eq!(pr.created_at.as_deref(), Some("2026-10-30T09:12:00Z"));
    assert_eq!(
        pr.review_requests,
        vec![
            PrReviewRequest {
                reviewer: "bob".into(),
                is_team: false
            },
            PrReviewRequest {
                reviewer: "o/core".into(),
                is_team: true
            },
        ]
    );
    assert_eq!(pr.commit_count, 2);
    assert_eq!(pr.number, 7);
    assert_eq!(pr.title, "Add feature");
    assert_eq!(pr.state, "OPEN");
    assert!(!pr.is_draft);
    assert_eq!(pr.author, "alice");
    assert_eq!(pr.review_decision.as_deref(), Some("APPROVED"));
    assert_eq!(pr.mergeable.as_deref(), Some("MERGEABLE"));
    assert_eq!(pr.head_ref_name, "feat/x");
    assert_eq!(pr.base_ref_name, "main");
    assert_eq!(pr.labels, vec!["bug".to_string(), "ui".to_string()]);
}

#[test]
fn parse_pr_overview_missing_optional_fields() {
    let json = r#"{"number": 1, "title": "T", "state": "OPEN", "isDraft": true, "author": {}, "labels": []}"#;
    let pr = parse_pr_overview(json).unwrap();
    assert_eq!(pr.number, 1);
    assert!(pr.is_draft);
    assert_eq!(pr.author, "");
    assert!(pr.labels.is_empty());
    assert!(pr.review_decision.is_none());
    assert!(pr.mergeable.is_none());
    assert!(pr.merge_state_status.is_none());
    assert!(pr.auto_merge_method.is_none());
    assert_eq!(pr.head_ref_oid, "");
    assert!(!pr.is_cross_repository);
    assert!(pr.created_at.is_none());
    assert!(pr.review_requests.is_empty());
    assert_eq!(pr.commit_count, 0);
}

#[test]
fn parse_pr_overview_skips_review_requests_without_a_name() {
    let json = r#"{"number": 3, "reviewRequests": [
        {"__typename": "Bot", "login": "copilot-pull-request-reviewer"},
        {"__typename": "User"},
        {"__typename": "Team", "name": "Docs"}
    ]}"#;
    let pr = parse_pr_overview(json).unwrap();
    let names: Vec<_> = pr
        .review_requests
        .iter()
        .map(|r| r.reviewer.as_str())
        .collect();
    assert_eq!(names, vec!["copilot-pull-request-reviewer", "Docs"]);
    assert!(pr.review_requests[1].is_team);
}

// ── pr_reviewers ──

fn review(author: &str, state: &str, at: &str) -> PrReview {
    PrReview {
        author: author.into(),
        state: state.into(),
        body: String::new(),
        submitted_at: at.into(),
    }
}

#[test]
fn pr_reviewers_lists_requested_first_then_reviewers_by_first_review() {
    let requests = vec![PrReviewRequest {
        reviewer: "dan".into(),
        is_team: false,
    }];
    let reviews = vec![
        review("carol", "COMMENTED", "2026-10-30T10:00:00Z"),
        review("bob", "APPROVED", "2026-10-30T10:05:00Z"),
    ];
    let got = pr_reviewers("alice", &requests, &reviews);
    let order: Vec<_> = got.iter().map(|r| r.reviewer.as_str()).collect();
    assert_eq!(order, vec!["dan", "carol", "bob"]);
    assert!(got[0].requested && got[0].state.is_none());
    assert_eq!(got[1].state.as_deref(), Some("COMMENTED"));
    assert_eq!(got[2].state.as_deref(), Some("APPROVED"));
    assert!(!got[2].requested);
}

#[test]
fn pr_reviewers_keeps_an_approval_over_a_later_comment() {
    let reviews = vec![
        review("bob", "COMMENTED", "2026-10-30T11:00:00Z"),
        review("bob", "APPROVED", "2026-10-30T10:00:00Z"),
    ];
    let got = pr_reviewers("alice", &[], &reviews);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].state.as_deref(), Some("APPROVED"));
}

#[test]
fn pr_reviewers_latest_decisive_review_wins() {
    let reviews = vec![
        review("bob", "CHANGES_REQUESTED", "2026-10-30T10:00:00Z"),
        review("bob", "APPROVED", "2026-10-30T11:00:00Z"),
    ];
    let got = pr_reviewers("alice", &[], &reviews);
    assert_eq!(got[0].state.as_deref(), Some("APPROVED"));
}

#[test]
fn pr_reviewers_re_request_keeps_prior_state() {
    let requests = vec![PrReviewRequest {
        reviewer: "Bob".into(),
        is_team: false,
    }];
    let reviews = vec![review("bob", "CHANGES_REQUESTED", "2026-10-30T10:00:00Z")];
    let got = pr_reviewers("alice", &requests, &reviews);
    assert_eq!(got.len(), 1);
    assert!(got[0].requested);
    assert_eq!(got[0].state.as_deref(), Some("CHANGES_REQUESTED"));
}

#[test]
fn pr_reviewers_leaves_out_the_author_and_pending_drafts() {
    let reviews = vec![
        review("alice", "COMMENTED", "2026-10-30T10:00:00Z"),
        review("bob", "PENDING", ""),
    ];
    assert!(pr_reviewers("Alice", &[], &reviews).is_empty());
}

#[test]
fn pr_reviewers_does_not_merge_a_user_into_a_team_of_the_same_name() {
    let requests = vec![PrReviewRequest {
        reviewer: "core".into(),
        is_team: true,
    }];
    let reviews = vec![review("core", "APPROVED", "2026-10-30T10:00:00Z")];
    let got = pr_reviewers("alice", &requests, &reviews);
    assert_eq!(got.len(), 2);
    assert!(got[0].state.is_none());
    assert_eq!(got[1].state.as_deref(), Some("APPROVED"));
}

#[test]
fn parse_pr_overview_reads_merge_box_fields() {
    let json = r#"{
        "number": 9,
        "state": "OPEN",
        "mergeStateStatus": "BEHIND",
        "headRefOid": "abc123",
        "isCrossRepository": true,
        "autoMergeRequest": {"mergeMethod": "SQUASH"}
    }"#;
    let pr = parse_pr_overview(json).unwrap();
    assert_eq!(pr.merge_state_status.as_deref(), Some("BEHIND"));
    assert_eq!(pr.head_ref_oid, "abc123");
    assert!(pr.is_cross_repository);
    assert_eq!(pr.auto_merge_method.as_deref(), Some("SQUASH"));
}

#[test]
fn parse_pr_overview_null_auto_merge_is_none() {
    let json = r#"{"number": 9, "autoMergeRequest": null}"#;
    let pr = parse_pr_overview(json).unwrap();
    assert!(pr.auto_merge_method.is_none());
}

#[test]
fn parse_pr_overview_invalid_json_errors() {
    assert!(parse_pr_overview("not json").is_err());
}

// ── parse_pr_comments ──

#[test]
fn parse_pr_comments_extracts_list() {
    let json = r#"{"comments": [
        {"author": {"login": "bob"}, "body": "Nice!", "createdAt": "2025-01-01T00:00:00Z", "url": "https://x/1"},
        {"author": {"login": "carol"}, "body": "+1", "createdAt": "2025-01-02T00:00:00Z", "url": "https://x/2"}
    ]}"#;
    let comments = parse_pr_comments(json).unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!(comments[0].author, "bob");
    assert_eq!(comments[0].body, "Nice!");
    assert_eq!(comments[1].author, "carol");
}

#[test]
fn parse_pr_comments_empty_array() {
    let json = r#"{"comments": []}"#;
    let comments = parse_pr_comments(json).unwrap();
    assert!(comments.is_empty());
}

// ── parse_pr_reviews ──

#[test]
fn parse_pr_reviews_extracts_states() {
    let json = r#"{"reviews": [
        {"author": {"login": "r1"}, "state": "APPROVED", "body": "", "submittedAt": "2025-01-01T00:00:00Z"},
        {"author": {"login": "r2"}, "state": "CHANGES_REQUESTED", "body": "fix", "submittedAt": "2025-01-02T00:00:00Z"}
    ]}"#;
    let reviews = parse_pr_reviews(json).unwrap();
    assert_eq!(reviews.len(), 2);
    assert_eq!(reviews[0].state, "APPROVED");
    assert_eq!(reviews[1].state, "CHANGES_REQUESTED");
    assert_eq!(reviews[1].body, "fix");
}

// ── parse_pr_checks ──

#[test]
fn parse_pr_checks_maps_state_and_bucket() {
    let json = r#"[
        {"name": "test", "state": "SUCCESS", "bucket": "pass", "link": "https://ci/1"},
        {"name": "lint", "state": "FAILURE", "bucket": "fail", "link": "https://ci/2"},
        {"name": "build", "state": "PENDING", "bucket": "pending"}
    ]"#;
    let checks = parse_pr_checks(json).unwrap();
    assert_eq!(checks.len(), 3);
    assert_eq!(checks[0].status, "COMPLETED");
    assert_eq!(checks[0].conclusion, "SUCCESS");
    assert_eq!(checks[0].url.as_deref(), Some("https://ci/1"));
    assert_eq!(checks[1].conclusion, "FAILURE");
    assert_eq!(checks[2].status, "PENDING");
    assert!(checks[2].url.is_none());
}

#[test]
fn parse_pr_checks_reads_timings_and_description() {
    let json = r#"[
        {"name": "test / python", "state": "SUCCESS", "bucket": "pass",
         "startedAt": "2026-10-30T09:00:00Z", "completedAt": "2026-10-30T09:01:52Z",
         "description": ""},
        {"name": "Vercel", "state": "SUCCESS", "bucket": "pass",
         "startedAt": "0001-01-01T00:00:00Z", "completedAt": "0001-01-01T00:00:00Z",
         "description": "Ready"},
        {"name": "cleanup", "state": "IN_PROGRESS", "bucket": "pending",
         "startedAt": "2026-10-30T09:00:00Z", "completedAt": "0001-01-01T00:00:00Z"}
    ]"#;
    let checks = parse_pr_checks(json).unwrap();
    assert_eq!(
        checks[0].started_at.as_deref(),
        Some("2026-10-30T09:00:00Z")
    );
    assert_eq!(checks[0].duration_secs(), Some(112));
    assert!(checks[0].description.is_none());

    assert!(checks[1].started_at.is_none());
    assert!(checks[1].completed_at.is_none());
    assert_eq!(checks[1].duration_secs(), None);
    assert_eq!(checks[1].description.as_deref(), Some("Ready"));

    assert!(checks[2].started_at.is_some());
    assert!(checks[2].completed_at.is_none());
    assert_eq!(checks[2].duration_secs(), None);
}

#[test]
fn check_duration_is_none_when_end_precedes_start() {
    let check = CheckRun {
        started_at: Some("2026-10-30T09:01:00Z".into()),
        completed_at: Some("2026-10-30T09:00:00Z".into()),
        ..Default::default()
    };
    assert_eq!(check.duration_secs(), None);
}

#[test]
fn parse_pr_checks_empty_array() {
    assert!(parse_pr_checks("[]").unwrap().is_empty());
}

// ── parse_remote_url ──

#[test]
fn parse_remote_url_ssh_scp_style() {
    let (owner, repo) = parse_remote_url("git@github.com:Acme/My-Repo.git").unwrap();
    assert_eq!(owner, "Acme");
    assert_eq!(repo, "My-Repo");
}

#[test]
fn parse_remote_url_https() {
    let (owner, repo) = parse_remote_url("https://github.com/Acme/My-Repo.git").unwrap();
    assert_eq!(owner, "Acme");
    assert_eq!(repo, "My-Repo");
}

#[test]
fn parse_remote_url_https_no_git_suffix() {
    let (owner, repo) = parse_remote_url("https://github.com/Acme/My-Repo").unwrap();
    assert_eq!(owner, "Acme");
    assert_eq!(repo, "My-Repo");
}

#[test]
fn parse_remote_url_ssh_scheme() {
    let (owner, repo) = parse_remote_url("ssh://git@github.com/Acme/My-Repo.git").unwrap();
    assert_eq!(owner, "Acme");
    assert_eq!(repo, "My-Repo");
}

#[test]
fn parse_remote_url_same_owner_repo_for_ssh_and_https() {
    // Both forms of the same origin must resolve identically.
    let ssh = parse_remote_url("git@github.com:Acme/My-Repo.git").unwrap();
    let https = parse_remote_url("https://github.com/Acme/My-Repo.git").unwrap();
    assert_eq!(ssh, https);
    assert_eq!(ssh, ("Acme".to_string(), "My-Repo".to_string()));
}

#[test]
fn parse_remote_url_preserves_case() {
    let (owner, repo) = parse_remote_url("https://github.com/UPPER/lower.git").unwrap();
    assert_eq!(owner, "UPPER");
    assert_eq!(repo, "lower");
}

#[test]
fn parse_remote_url_invalid_no_slash() {
    assert!(parse_remote_url("https://github.com/only-one-segment").is_none());
}

#[test]
fn parse_remote_url_empty() {
    assert!(parse_remote_url("").is_none());
}

#[cfg(test)]
fn git(root: &std::path::Path, args: &[&str]) -> String {
    let out = std::process::Command::new("git")
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A clone whose origin had a `stack-base` branch that was then deleted —
/// a stacked PR's base after it merged. Returns (tempdir, clone, base sha).
fn clone_with_deleted_base() -> (tempfile::TempDir, std::path::PathBuf, String) {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin.git");
    let clone = dir.path().join("clone");
    git(
        dir.path(),
        &["init", "--bare", "-b", "main", origin.to_str().unwrap()],
    );
    git(
        dir.path(),
        &["clone", origin.to_str().unwrap(), clone.to_str().unwrap()],
    );
    git(&clone, &["commit", "--allow-empty", "-m", "root"]);
    git(&clone, &["push", "origin", "HEAD:main"]);
    git(&clone, &["checkout", "-b", "stack-base"]);
    git(&clone, &["commit", "--allow-empty", "-m", "base"]);
    let sha = git(&clone, &["rev-parse", "HEAD"]);
    git(&clone, &["push", "origin", "stack-base"]);
    git(&clone, &["checkout", "main"]);
    git(&clone, &["push", "origin", "--delete", "stack-base"]);
    (dir, clone, sha)
}

#[test]
fn deleted_base_branch_is_a_missing_remote_ref() {
    let (_dir, clone, _) = clone_with_deleted_base();
    let err =
        fetch_base_branch_ref(&er_git::CliGit, clone.to_str().unwrap(), "stack-base").unwrap_err();
    assert!(is_missing_remote_ref(&err), "{err:#}");
    assert!(!is_missing_remote_ref(&anyhow::anyhow!(
        "Could not resolve host: github.com"
    )));
}

#[test]
fn pr_base_falls_back_to_the_pr_base_commit_when_the_branch_is_deleted() {
    let (_dir, clone, sha) = clone_with_deleted_base();
    let root = clone.to_str().unwrap();
    // Before: the plain base fetch is the error the user saw in set_mode.
    assert!(fetch_base_branch_ref(&er_git::CliGit, root, "stack-base").is_err());

    let base =
        fetch_pr_base_ref_with(&er_git::CliGit, root, 7, "stack-base", || Ok(sha.clone())).unwrap();
    assert!(is_pinned_pr_base(&base));
    assert_eq!(
        rev_parse_oid(&er_git::CliGit, root, &base).as_deref(),
        Some(sha.as_str())
    );
}

#[test]
fn pr_base_fetch_of_a_live_branch_never_looks_up_the_commit() {
    let (_dir, clone, _) = clone_with_deleted_base();
    let base = fetch_pr_base_ref_with(&er_git::CliGit, clone.to_str().unwrap(), 7, "main", || {
        panic!("the fallback must not run for a branch origin still has")
    })
    .unwrap();
    assert_eq!(base, "origin/main");
}

#[test]
fn fetch_failure_text_survives_the_git_error() {
    let missing = er_git::FakeGit::default().with_fetch(Err(er_git::GitError::Failed {
        context: "git fetch failed".into(),
        stderr: "fatal: couldn't find remote ref refs/heads/x".into(),
    }));
    let err = fetch_base_branch_ref(&missing, "/repo", "x").unwrap_err();
    assert!(is_missing_remote_ref(&err), "{err:#}");

    let no_upstream = er_git::FakeGit::default().with_fetch(Err(er_git::GitError::Failed {
        context: "git fetch failed".into(),
        stderr: "fatal: has no upstream to refresh".into(),
    }));
    let err = fetch_base_branch_ref(&no_upstream, "/repo", "x").unwrap_err();
    assert!(is_no_upstream_to_refresh(&err), "{err:#}");
}

#[test]
fn fetch_pr_head_calls_fetch_with_the_pull_refspec() {
    let fake = er_git::FakeGit::default().with_fetch(Ok(()));

    let ref_name = fetch_pr_head(&fake, 7, "/repo").unwrap();

    assert_eq!(ref_name, "refs/er/pr/7/head");
    assert!(
        fake.calls()
            .iter()
            .any(|c| c
                == "fetch(root=\"/repo\", args=[\"origin\", \"+pull/7/head:refs/er/pr/7/head\"])"),
        "{:?}",
        fake.calls()
    );
}

#[test]
fn deleted_base_pins_to_its_local_commit() {
    let (_dir, clone, sha) = clone_with_deleted_base();
    let root = clone.to_str().unwrap();
    let pinned = pin_pr_base_commit(&er_git::CliGit, root, 7, &sha).unwrap();
    assert_eq!(pinned, "refs/er/pr/7/base");
    assert_eq!(
        rev_parse_oid(&er_git::CliGit, root, &pinned).as_deref(),
        Some(sha.as_str())
    );
}

#[test]
fn deleted_base_commit_missing_locally_is_fetched_by_sha() {
    let (dir, _clone, sha) = clone_with_deleted_base();
    let origin = dir.path().join("origin.git");
    // GitHub serves any commit by SHA; a bare repo only does with this set.
    git(
        &origin,
        &["config", "uploadpack.allowAnySHA1InWant", "true"],
    );
    let fresh = dir.path().join("fresh");
    git(
        dir.path(),
        &[
            "clone",
            "--no-local",
            origin.to_str().unwrap(),
            fresh.to_str().unwrap(),
        ],
    );
    let root = fresh.to_str().unwrap();
    assert!(!ref_exists_locally(
        &er_git::CliGit,
        root,
        &format!("{sha}^{{commit}}")
    ));

    let pinned = pin_pr_base_commit(&er_git::CliGit, root, 7, &sha).unwrap();
    assert_eq!(
        rev_parse_oid(&er_git::CliGit, root, &pinned).as_deref(),
        Some(sha.as_str())
    );
}

#[test]
fn ci_checks_parsed_from_gh_json() {
    // Simulate `gh pr checks --json name,state,bucket` output
    let json = r#"[
        {"name": "test", "state": "SUCCESS", "bucket": "pass"},
        {"name": "lint", "state": "FAILURE", "bucket": "fail"},
        {"name": "build", "state": "PENDING"}
    ]"#;
    // Exercise the REAL parser — a regression in state/bucket mapping must fail this.
    let checks = parse_pr_checks(json).unwrap();
    assert_eq!(checks.len(), 3);
    assert_eq!(checks[0].name, "test");
    assert_eq!(checks[0].status, "COMPLETED");
    assert_eq!(checks[0].conclusion, "SUCCESS");
    assert_eq!(checks[1].name, "lint");
    assert_eq!(checks[1].status, "COMPLETED");
    assert_eq!(checks[1].conclusion, "FAILURE");
    assert_eq!(checks[2].name, "build");
    assert_eq!(checks[2].status, "PENDING");
    assert_eq!(checks[2].conclusion, "");
}

#[test]
fn parse_pr_list_queue_marks_review_requested_of_me() {
    let json = r#"[
      {
        "number": 7,
        "title": "Tiny fix",
        "headRefName": "fix",
        "baseRefName": "main",
        "state": "OPEN",
        "isDraft": false,
        "author": {"login": "alice"},
        "reviewRequests": [{"login": "bob"}],
        "reviewDecision": "REVIEW_REQUIRED",
        "mergeable": "MERGEABLE",
        "mergeStateStatus": "CLEAN",
        "additions": 4,
        "deletions": 1,
        "changedFiles": 1,
        "updatedAt": "2026-07-01T00:00:00Z",
        "labels": [{"name": "bug"}],
        "url": "https://github.com/o/r/pull/7",
        "latestReviews": [{"author": {"login": "carol"}, "state": "COMMENTED"}]
      }
    ]"#;
    let prs = parse_pr_list_queue(json, Some("bob")).unwrap();
    assert_eq!(prs.len(), 1);
    assert!(prs[0].review_requested_of_me);
    assert_eq!(prs[0].additions, 4);
    assert_eq!(prs[0].labels, vec!["bug".to_string()]);
    assert!(prs[0].my_latest_review_state.is_none());

    let prs2 = parse_pr_list_queue(json, Some("carol")).unwrap();
    assert!(!prs2[0].review_requested_of_me);
    assert_eq!(prs2[0].my_latest_review_state.as_deref(), Some("COMMENTED"));
}

#[test]
fn summarize_checks_state_detects_failures() {
    let checks = vec![
        CheckRun {
            name: "lint".into(),
            status: "COMPLETED".into(),
            conclusion: "SUCCESS".into(),
            ..Default::default()
        },
        CheckRun {
            name: "test".into(),
            status: "COMPLETED".into(),
            conclusion: "FAILURE".into(),
            ..Default::default()
        },
    ];
    assert_eq!(summarize_checks_state(&checks), "failing");
}

#[test]
fn summarize_checks_state_treats_in_progress_as_pending() {
    let checks = vec![CheckRun {
        name: "build".into(),
        status: "IN_PROGRESS".into(),
        conclusion: "".into(),
        ..Default::default()
    }];
    assert_eq!(summarize_checks_state(&checks), "pending");
    let queued = vec![CheckRun {
        name: "build".into(),
        status: "QUEUED".into(),
        conclusion: "".into(),
        ..Default::default()
    }];
    assert_eq!(summarize_checks_state(&queued), "pending");
}

#[test]
fn thread_addressing_summary_from_graphql() {
    let json = r#"{
      "data": {
        "repository": {
          "pullRequest": {
            "reviewThreads": {
              "nodes": [
                {"isResolved": true, "isOutdated": false, "comments": {"nodes": [{"databaseId": 1}]}},
                {"isResolved": false, "isOutdated": true, "comments": {"nodes": [{"databaseId": 2}]}}
              ]
            }
          }
        }
      }
    }"#;
    let s = summarize_thread_addressing_from_graphql(json);
    assert!(s.all_addressed);
    assert_eq!(s.thread_count, 2);
}

// ── caches ──

fn cli_github() -> CliGitHub {
    CliGitHub::new(Arc::new(crate::testing::ScriptedRunner::new()))
}

#[cfg(test)]
#[test]
fn repo_info_is_served_from_cache_within_60_seconds() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_str().unwrap();
    git_init_with_remote(root, "https://github.com/Acme/discovery.git");
    let github = cli_github();
    let (owner, repo) = github
        .get_repo_info(&er_git::CliGit, root)
        .expect("parses origin remote");
    assert_eq!((owner.as_str(), repo.as_str()), ("Acme", "discovery"));
    // A fresh lookup would now fail; a hit proves the cache served it.
    std::process::Command::new("git")
        .args(["remote", "remove", "origin"])
        .current_dir(root)
        .output()
        .unwrap();
    let (owner2, repo2) = github
        .get_repo_info(&er_git::CliGit, root)
        .expect("cache serves");
    assert_eq!((owner2.as_str(), repo2.as_str()), ("Acme", "discovery"));
}

#[test]
fn repo_info_cache_is_keyed_by_the_root_as_given() {
    let caches = crate::Caches::new();
    caches
        .repo_info
        .put("/Repo/Root".to_string(), Arc::new(("o".into(), "r".into())));
    assert!(caches.repo_info.get(&"/Repo/Root".to_string()).is_some());
    assert!(
        caches.repo_info.get(&"/repo/root".to_string()).is_none(),
        "the repo root key keeps case"
    );
}

#[test]
fn comment_bundle_cache_serves_within_ttl_and_expires() {
    let cache: crate::cache::TtlCache<(String, String, u64), Arc<PrCommentBundle>> =
        crate::cache::TtlCache::new(std::time::Duration::from_millis(5), Some(4));
    let bundle = Arc::new(PrCommentBundle {
        comments: Vec::new(),
        threads: HashMap::new(),
    });
    cache.put(("o".into(), "r".into(), 1u64), Arc::clone(&bundle));
    assert!(
        cache.get(&("o".into(), "r".into(), 1u64)).is_some(),
        "fresh entry serves"
    );
    std::thread::sleep(std::time::Duration::from_millis(20));
    assert!(
        cache.get(&("o".into(), "r".into(), 1u64)).is_none(),
        "expired entry must miss"
    );
}

#[test]
fn comment_bundle_cache_is_keyed_by_lowercased_owner_repo_pr() {
    let runner = Arc::new(crate::testing::ScriptedRunner::new());
    runner.script(
        &[
            "gh",
            "api",
            "--include",
            "repos/Acme/Repo/pulls/1/comments",
            "--paginate",
        ],
        Ok(crate::testing::ScriptedRunner::output(0, "[]", "")),
    );
    runner.script(
        &["gh", "api", "--include", "graphql"],
        Ok(crate::testing::ScriptedRunner::output(0, "{}", "")),
    );
    let github = CliGitHub::new(runner);
    github
        .pr_comment_bundle_cached("Acme", "Repo", 1, None)
        .expect("bundle");
    assert!(
        github
            .cache()
            .pr_comments
            .get(&("acme".into(), "repo".into(), 1))
            .is_some(),
        "the cache key is lowercased"
    );
}

#[test]
fn push_comment_sends_path_line_side_and_body() {
    let runner = Arc::new(crate::testing::ScriptedRunner::new());
    runner.script(
        &["gh", "pr", "view"],
        Ok(crate::test_support::output(0, "abc\n", "")),
    );
    runner.script(
        &["gh", "api", "--include", "-X", "POST"],
        Ok(crate::test_support::output(0, r#"{"id":9}"#, "")),
    );
    let github = CliGitHub::new(runner.clone());
    let id = github
        .push_comment(
            "o",
            "r",
            7,
            NewLineComment {
                path: "a.rs",
                line_start: 3,
                line_end: Some(5),
                body: "hey",
                side: "RIGHT",
            },
            "/root",
        )
        .unwrap();
    assert_eq!(id, 9);
    let calls = runner.calls();
    let post = &calls[1];
    for expected in [
        "path=a.rs",
        "line=5",
        "start_line=3",
        "side=RIGHT",
        "body=hey",
        "commit_id=abc",
    ] {
        assert!(
            post.iter().any(|arg| arg == expected),
            "{expected} missing from {post:?}"
        );
    }
}

#[test]
fn reply_comment_targets_the_parent_comment_id() {
    let runner = Arc::new(crate::testing::ScriptedRunner::new());
    runner.script(
        &["gh", "api", "--include", "-X", "POST"],
        Ok(crate::test_support::output(0, r#"{"id":10}"#, "")),
    );
    let github = CliGitHub::new(runner.clone());
    let id = github
        .reply_comment("o", "r", 7, 42, "hi", "/root")
        .unwrap();
    assert_eq!(id, 10);
    let calls = runner.calls();
    assert!(
        calls[0].iter().any(|arg| arg == "in_reply_to=42"),
        "{:?}",
        calls[0]
    );
}

#[test]
fn delete_comment_treats_404_as_already_gone() {
    let runner = Arc::new(crate::testing::ScriptedRunner::new());
    runner.script(
        &["gh", "api", "--include", "-X", "DELETE"],
        Ok(crate::test_support::output(
            1,
            "HTTP/2.0 404 Not Found\r\n\r\n{}",
            "gh: Not Found (HTTP 404)",
        )),
    );
    let github = CliGitHub::new(runner);
    github.delete_comment("o", "r", 5, "/root").unwrap();
}

#[test]
fn delete_comment_fails_on_403() {
    let runner = Arc::new(crate::testing::ScriptedRunner::new());
    runner.script(
        &["gh", "api", "--include", "-X", "DELETE"],
        Ok(crate::test_support::output(
            1,
            "HTTP/2.0 403 Forbidden\r\n\r\n\
             {\"message\":\"Resource not accessible by integration\"}",
            "gh: Resource not accessible by integration (HTTP 403)",
        )),
    );
    let github = CliGitHub::new(runner);
    let error = github.delete_comment("o", "r", 5, "/root").unwrap_err();
    assert!(matches!(error, GhError::Forbidden { .. }), "{error:?}");
}

#[test]
fn invalidate_comments_leaves_merge_settings_cached() {
    let github = cli_github();
    let bundle = Arc::new(PrCommentBundle {
        comments: Vec::new(),
        threads: HashMap::new(),
    });
    github
        .cache()
        .pr_comments
        .put(("o".into(), "r".into(), 1), bundle);
    github.cache().merge_queue.put("o/r:main".to_string(), true);
    github.invalidate_comments();
    assert!(github
        .cache()
        .pr_comments
        .get(&("o".into(), "r".into(), 1))
        .is_none());
    assert_eq!(
        github.cache().merge_queue.get(&"o/r:main".to_string()),
        Some(true)
    );
}

#[cfg(test)]
fn git_init_with_remote(root: &str, remote: &str) {
    let out = std::process::Command::new("git")
        .args(["init", "-q", root])
        .output()
        .expect("git init");
    assert!(out.status.success());
    let out = std::process::Command::new("git")
        .args(["remote", "add", "origin", remote])
        .current_dir(root)
        .output()
        .expect("git remote add");
    assert!(out.status.success());
}

#[test]
fn pr_number_from_notification_url_parses_pulls_and_issues() {
    assert_eq!(
        pr_number_from_notification_url("https://api.github.com/repos/org/repo/pulls/42"),
        Some(42)
    );
    assert_eq!(
        pr_number_from_notification_url("https://api.github.com/repos/org/repo/issues/99"),
        Some(99)
    );
    assert_eq!(
        pr_number_from_notification_url("https://github.com/org/repo/pull/7"),
        Some(7)
    );
    assert_eq!(
        pr_number_from_notification_url("https://api.github.com/repos/org/repo/commits/abc"),
        None
    );
}

#[test]
fn diff_too_large_matches_githubs_refusals_and_nothing_else() {
    assert!(gh_diff_too_large(
        "could not find pull request diff: HTTP 406: Sorry, the diff exceeded the maximum \
         number of files (300). Consider using 'List pull requests files' API"
    ));
    assert!(gh_diff_too_large("PullRequest.diff too_large"));
    assert!(!gh_diff_too_large("HTTP 404: Not Found"));
    assert!(!gh_diff_too_large(
        "no pull requests found for branch 14060"
    ));
}

#[test]
fn merge_base_diff_excludes_commits_the_base_gained_since_the_branch_point() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    git(root, &["init", "-q", "-b", "main"]);
    std::fs::write(root.join("shared.txt"), "one\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base"]);

    git(root, &["checkout", "-q", "-b", "feature"]);
    std::fs::write(root.join("feature.txt"), "from the PR\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "pr"]);
    let head = git(root, &["rev-parse", "HEAD"]);

    git(root, &["checkout", "-q", "main"]);
    std::fs::write(root.join("landed-later.txt"), "not part of the PR\n").unwrap();
    git(root, &["add", "."]);
    git(root, &["commit", "-q", "-m", "base moved on"]);
    let base_tip = git(root, &["rev-parse", "HEAD"]);

    let diff =
        git_merge_base_diff(&er_git::CliGit, root.to_str().unwrap(), &base_tip, &head).unwrap();
    assert!(diff.contains("feature.txt"), "{diff}");
    assert!(!diff.contains("landed-later.txt"), "{diff}");
}

#[test]
fn parse_paginated_handles_brackets_inside_strings() {
    let one = r#"[{"id":1,"body":"a ] bracket and ][ in text"}]"#;
    let parsed: Vec<serde_json::Value> = parse_paginated(one).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0]["body"], "a ] bracket and ][ in text");

    let two = r#"[{"id":1}][{"id":2}]"#;
    let parsed: Vec<serde_json::Value> = parse_paginated(two).unwrap();
    assert_eq!(parsed.len(), 2);
}

#[test]
fn parse_paginated_rejects_a_bad_chunk() {
    let bad = r#"[{"id":1}][not json]"#;
    assert!(parse_paginated::<serde_json::Value>(bad).is_err());
}

#[test]
fn pr_lookup_absent_only_when_gh_says_no_pull_request() {
    assert_eq!(
        classify_pr_lookup_failure("no pull requests found for branch \"feat\""),
        PrLookup::Absent
    );
    assert!(matches!(
        classify_pr_lookup_failure("gh: authentication required"),
        PrLookup::Unknown(_)
    ));
    assert!(matches!(
        classify_pr_lookup_failure("request timed out"),
        PrLookup::Unknown(_)
    ));
}
