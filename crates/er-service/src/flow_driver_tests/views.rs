//! What a flow reviews and where its providers run: the view rules
//! (`review_flow_inputs`) keep the reviewed target apart from the checkout
//! that holds it, and every stage the service host launches runs in that
//! checkout. Moved from the driver's `mod.rs`, where they ran on the App's
//! tab.

use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use er_engine::ai::change_facts::OwnedRepoRules;
use er_engine::ai::review_flow::{FlowState, FlowTaskStatus};
use er_engine::ai::review_md::ReviewMdSource;
use er_engine::config::{ErConfig, ReviewModelGroups, ReviewModelRef};
use er_engine::ports::Ports;
use er_engine::review_flow_driver::{review_flow_inputs, ReviewFlowInputs, ReviewedTab};
use er_engine::tab_identity::{DiffMode, TabIdentity};
use std::sync::Arc;

use super::driver::{dirs, fake_config, with_script, with_second_provider, RAW};
use super::Harness;
use crate::protocol::FlowContext;

/// A tab as the view rules read it.
struct Tab {
    repo_root: String,
    current_branch: String,
    mode: DiffMode,
    remote_repo: Option<String>,
    pr_number: Option<u64>,
    local_branch_view: Option<String>,
    pr_head_ref: Option<String>,
    checkout_root: Option<String>,
    er_dir: String,
}

impl ReviewedTab for Tab {
    fn identity(&self) -> TabIdentity<'_> {
        TabIdentity {
            repo_root: &self.repo_root,
            current_branch: &self.current_branch,
            base_branch: "main",
            mode: self.mode,
            tour_is_pr: false,
            remote_repo: self.remote_repo.as_deref(),
            pr_number: self.pr_number,
            local_branch_view: self.local_branch_view.as_deref(),
            pr_head_ref: self.pr_head_ref.as_deref(),
            checkout_root: self.checkout_root.as_deref(),
        }
    }

    fn er_dir(&self) -> String {
        self.er_dir.clone()
    }

    fn raw_diff(&self, _scope: &str) -> Result<String> {
        Ok(RAW.into())
    }

    fn owned_rules(&self) -> OwnedRepoRules {
        OwnedRepoRules::default()
    }

    fn review_md_source(&self) -> ReviewMdSource {
        ReviewMdSource::None
    }
}

fn tab(repo_root: &str, er_dir: &str) -> Tab {
    Tab {
        repo_root: repo_root.into(),
        current_branch: "feature".into(),
        mode: DiffMode::Branch,
        remote_repo: None,
        pr_number: None,
        local_branch_view: None,
        pr_head_ref: None,
        checkout_root: None,
        er_dir: er_dir.into(),
    }
}

// The lint gate reads this file alone, without the module's `cfg(test)`.
#[cfg(test)]
fn git(root: &Path, args: &[&str]) {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Starts the flow on `view`, confirms a Deep plan, and runs it to its end.
fn run_flow(harness: &Harness, config: &ErConfig, view: ReviewFlowInputs, deep: bool) {
    let bucket = view.target.er_dir.clone();
    harness.set_context(config, view);
    harness.start("branch").unwrap();
    if deep {
        harness.advance_until(
            &bucket,
            Duration::from_secs(15),
            "Deep did not reach confirmation",
            |flow| flow.state == FlowState::AwaitingConfirmation,
        );
        harness.confirm(&bucket).unwrap();
    }
    harness.run_until_terminal(&bucket);
    let state = harness.flow(&bucket).state;
    assert!(matches!(state, FlowState::Done { .. }), "{state:?}");
}

#[test]
fn linked_checkout_review_flow_keeps_execution_separate_from_target_identity() {
    let _serial = er_agent::agent_slots::test_serial();
    for remote in [None, Some("example/linked-checkout")] {
        for deep in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let paths = dirs(dir.path());
            let repo = dir.path().join("repo");
            let checkout = dir.path().join("feature");
            git(&repo, &["init", "-b", "main"]);
            git(&repo, &["config", "user.name", "Test"]);
            git(&repo, &["config", "user.email", "test@example.invalid"]);
            std::fs::create_dir_all(repo.join("src")).unwrap();
            std::fs::write(repo.join("src/lib.rs"), "main checkout\n").unwrap();
            git(&repo, &["add", "."]);
            git(
                &repo,
                &["-c", "commit.gpgsign=false", "commit", "-qm", "seed"],
            );
            git(
                &repo,
                &[
                    "worktree",
                    "add",
                    "-b",
                    "feature",
                    checkout.to_str().unwrap(),
                ],
            );
            std::fs::write(checkout.join("src/lib.rs"), "feature checkout\n").unwrap();
            let log = dir.path().join("execution.log");
            let primary = if deep { "arena" } else { "expert" };
            // Every actual subprocess records the source it can read. The
            // arena response satisfies all three round schemas.
            let script = format!(
                r#"
record() {{ printf '%s|%s\n%s\n' "$1" "$(pwd -P)" "$(cat src/lib.rs)" >> '{log}'; }}
case "$1" in
  *triage.json*) record triage; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","verdict":{{"primary":"{primary}","experts":["security"]}}}}' "$hash" > '__BUCKET__/triage.json' ;;
  *experts/security.json*) record expert; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); mkdir -p '__BUCKET__/experts'; printf '{{"version":1,"expert_id":"security","diff_hash":"%s","files":{{}}}}' "$hash" > '__BUCKET__/experts/security.json' ;;
  *review.json*) record general; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","files":{{"src/lib.rs":{{"risk":"high","findings":[{{"id":"general:1","severity":"high","title":"Check the feature source"}}]}}}}}}' "$hash" > '__BUCKET__/review.json'; printf '{{"version":1,"diff_hash":"%s","order":[],"groups":{{}}}}' "$hash" > '__BUCKET__/order.json'; printf '{{"version":1,"diff_hash":"%s","items":[]}}' "$hash" > '__BUCKET__/checklist.json'; printf 'summary\n' > '__BUCKET__/summary.md' ;;
  *) record arena; printf '{{"findings":[],"ballots":[],"verdicts":[]}}' ;;
esac
"#,
                log = log.display(),
            );
            let mut config = with_script(er_engine::test_support::staged_script(
                &script,
                &paths.bucket,
            ));
            if deep {
                with_second_provider(&mut config);
            }
            er_engine::storage::with_process_test_storage_root(dir.path(), || {
                let mut tab = tab(&paths.repo, &paths.bucket);
                tab.local_branch_view = Some("feature".into());
                tab.checkout_root = Some(checkout.to_string_lossy().into_owned());
                tab.remote_repo = remote.map(str::to_owned);
                let view = review_flow_inputs(&tab, "branch").unwrap();
                assert_eq!(view.target.repo_root, paths.repo);
                assert_eq!(view.target.er_dir, paths.bucket);
                assert_eq!(view.repo_root, checkout.to_string_lossy());

                let harness = Harness::new(Ports::cli());
                run_flow(&harness, &config, view, deep);
            });
            let observed = std::fs::read_to_string(&log).unwrap();
            let execution = std::fs::canonicalize(&checkout).unwrap();
            let stages = if deep {
                vec!["triage", "arena"]
            } else {
                vec!["triage", "general", "expert", "arena"]
            };
            for stage in stages {
                assert!(
                    observed.contains(&format!(
                        "{stage}|{}\nfeature checkout\n",
                        execution.display()
                    )),
                    "missing {stage} at feature checkout\n{observed}"
                );
            }
            assert!(!observed.contains("main checkout"), "{observed}");
        }
    }
}

#[test]
fn pr_view_without_checkout_reviews_from_the_head_ref() {
    let _serial = er_agent::agent_slots::test_serial();
    for deep in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let paths = dirs(dir.path());
        let log = dir.path().join("execution.log");
        let primary = if deep { "arena" } else { "expert" };
        // Each subprocess records where it ran and whether its prompt names
        // the PR head ref and how to fetch it, since the working tree holds
        // another branch. The general finding gives verification work.
        let script = format!(
            r#"
record() {{ case "$2" in *refs/er/pr/7/head*pull/7/head*|*pull/7/head*refs/er/pr/7/head*) r=ref ;; *) r=noref ;; esac; printf '%s|%s|%s\n' "$1" "$(pwd -P)" "$r" >> '{log}'; }}
case "$1" in
  *triage.json*) record triage "$1"; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","verdict":{{"primary":"{primary}","experts":["security"]}}}}' "$hash" > '__BUCKET__/triage.json' ;;
  *experts/security.json*) record expert "$1"; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); mkdir -p '__BUCKET__/experts'; printf '{{"version":1,"expert_id":"security","diff_hash":"%s","files":{{}}}}' "$hash" > '__BUCKET__/experts/security.json' ;;
  *review.json*) record general "$1"; hash=$(cat '__BUCKET__/.diff-tmp.sha256'); printf '{{"version":1,"diff_hash":"%s","files":{{"src/lib.rs":{{"risk":"high","findings":[{{"id":"general:1","severity":"high","title":"Check the PR source"}}]}}}}}}' "$hash" > '__BUCKET__/review.json'; printf '{{"version":1,"diff_hash":"%s","order":[],"groups":{{}}}}' "$hash" > '__BUCKET__/order.json'; printf '{{"version":1,"diff_hash":"%s","items":[]}}' "$hash" > '__BUCKET__/checklist.json'; printf 'summary\n' > '__BUCKET__/summary.md' ;;
  *) record arena "$1"; printf '{{"findings":[],"ballots":[],"verdicts":[]}}' ;;
esac
"#,
            log = log.display(),
        );
        let mut config = with_script(er_engine::test_support::staged_script(
            &script,
            &paths.bucket,
        ));
        if deep {
            with_second_provider(&mut config);
        }
        er_engine::storage::with_process_test_storage_root(dir.path(), || {
            let mut tab = tab(&paths.repo, &paths.bucket);
            tab.remote_repo = Some("example/pr-no-checkout".into());
            tab.pr_number = Some(7);
            tab.local_branch_view = Some("feature/pr-head".into());
            tab.current_branch = "feature/pr-head".into();
            tab.pr_head_ref = Some("refs/er/pr/7/head".into());
            let view = review_flow_inputs(&tab, "branch").unwrap();
            assert_eq!(view.repo_root, paths.repo);

            let harness = Harness::new(Ports::cli());
            run_flow(&harness, &config, view, deep);
        });
        let observed = std::fs::read_to_string(&log).unwrap();
        let execution = std::fs::canonicalize(&paths.repo).unwrap();
        let stages = if deep {
            vec!["triage", "arena"]
        } else {
            vec!["triage", "general", "expert", "arena"]
        };
        for stage in stages {
            assert!(
                observed.contains(&format!("{stage}|{}|ref\n", execution.display())),
                "{stage} did not run in the repo with the head ref (deep: {deep})\n{observed}"
            );
        }
        assert!(!observed.contains("|noref"), "deep: {deep}\n{observed}");
    }
}

/// Staged and unstaged diffs exist only in a checkout of the viewed branch,
/// so a view without one is refused, and a start on that refusal fails.
#[test]
fn view_without_checkout_still_rejects_working_tree_scopes() {
    let dir = tempfile::tempdir().unwrap();
    let paths = dirs(dir.path());
    er_engine::storage::with_test_storage_root(dir.path(), || {
        let mut tab = tab(&paths.repo, &paths.bucket);
        tab.mode = DiffMode::Unstaged;
        tab.local_branch_view = Some("feature/pr-head".into());
        let error = review_flow_inputs(&tab, "unstaged")
            .unwrap_err()
            .to_string();
        assert!(error.contains("checkout"), "{error}");

        let harness = Harness::new(Ports::cli());
        harness.flows().host.take_context(FlowContext {
            config: Arc::new(fake_config(&paths.bucket, "general", &[])),
            effort: None,
            view: Err(error.clone()),
        });
        let refused = harness.start("unstaged").unwrap_err().to_string();
        assert_eq!(refused, error);
        assert!(er_engine::arena::check_bucket_admission(&paths.bucket).is_ok());
    });
}

/// A started flow's rows keep the model their stage captured; a later
/// settings change relabels nothing.
#[test]
fn a_started_flow_keeps_its_captured_triage_model() {
    let _serial = er_agent::agent_slots::test_serial();
    let dir = tempfile::tempdir().unwrap();
    let paths = dirs(dir.path());
    let mut config = fake_config(&paths.bucket, "general", &[]);
    let provider = config.ai_hub.providers["fake"].clone();
    config.ai_hub.providers = [("fast", "fast-model"), ("standard", "standard-model")]
        .iter()
        .map(|(provider_id, model_id)| {
            let mut provider = provider.clone();
            provider.models[0].id = (*model_id).into();
            ((*provider_id).into(), provider)
        })
        .collect();
    let selection = |provider_id: &str, model_id: &str| ReviewModelRef {
        provider_id: provider_id.into(),
        model_id: model_id.into(),
    };
    config.ai_hub.default_provider = Some("fast".into());
    config.ai_hub.review_models = ReviewModelGroups {
        fast: vec![selection("fast", "fast-model")],
        standard: vec![selection("standard", "standard-model")],
        advanced: vec![selection("standard", "standard-model")],
    };
    er_engine::storage::with_process_test_storage_root(dir.path(), || {
        let harness = Harness::new(Ports::cli());
        let view = super::driver::view(&paths);
        harness.set_context(&config, view.clone());
        harness.start("branch").unwrap();
        let captured = harness.flow(&paths.bucket).tasks[0].clone();
        assert!(matches!(
            captured.status,
            FlowTaskStatus::Queued | FlowTaskStatus::Running
        ));
        assert_eq!(captured.provider_id, "fast");
        assert_eq!(captured.model_id, "fast-model");

        let mut edited = config.clone();
        edited.ai_hub.providers.get_mut("fast").unwrap().models[0].label =
            Some("Edited Fast Label".into());
        edited.ai_hub.review_models.fast = vec![selection("standard", "standard-model")];
        harness.set_context(&edited, view);
        harness.advance();
        assert_eq!(
            harness.flow(&paths.bucket).tasks[0].model_label,
            captured.model_label,
            "task metadata remains tied to the captured model after settings change"
        );
        assert!(harness.cancel(&paths.bucket));
        harness.drain(&paths.bucket);
    });
}
