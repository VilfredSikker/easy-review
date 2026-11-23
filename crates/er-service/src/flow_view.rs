//! A review flow's view and its acceptance diff, read from a tab's published
//! content by the rules the App reads its own tab by
//! (`er_engine::review_flow_driver::review_flow_inputs`). The desktop reads
//! them off the actor before it sends a flow command, so a flow never waits
//! on the App.

use anyhow::{Context, Result};
use er_engine::ai::change_facts::OwnedRepoRules;
use er_engine::ai::review_md::ReviewMdSource;
use er_engine::config::ErConfig;
use er_engine::ports::Ports;
use er_engine::review_flow_driver::{self, ReviewFlowInputs, ReviewedTab};
use er_engine::tab_identity::TabIdentity;
use er_engine::task_target::BackgroundTaskTarget;

use crate::content::TabContent;

/// A tab's content as the flow rules read it.
struct ContentTab<'a> {
    ports: &'a Ports,
    content: &'a TabContent,
    config: &'a ErConfig,
}

impl ReviewedTab for ContentTab<'_> {
    fn identity(&self) -> TabIdentity<'_> {
        self.content.tab.identity()
    }

    fn er_dir(&self) -> String {
        self.content.context.er_dir.clone()
    }

    fn raw_diff(&self, scope: &str) -> Result<String> {
        review_raw_diff(self.ports, self.content, scope)
    }

    /// The repo's tables under the key the diff loaded them by, as
    /// `diff(tab)` reads them.
    fn owned_rules(&self) -> OwnedRepoRules {
        let meta = &self.content.diff.meta;
        OwnedRepoRules {
            file_kinds: self
                .config
                .file_kinds
                .repo(&meta.rules_key)
                .cloned()
                .unwrap_or_default(),
            risk: self
                .config
                .risk_rules_merged(&meta.rules_key, meta.risk_artifact.clone()),
        }
    }

    fn review_md_source(&self) -> ReviewMdSource {
        review_flow_driver::review_md_source(self.ports, &self.content.tab.identity())
    }
}

/// The unfiltered diff a review of `scope` reads on `content`'s tab: the
/// diff the content holds when it is the one the scope reviews, else a fresh
/// fetch of that scope. History, Conflicts and Hidden are refused by the
/// flow before it reads a diff.
pub fn review_raw_diff(ports: &Ports, content: &TabContent, scope: &str) -> Result<String> {
    let meta = &content.diff.meta;
    let id = content.tab.identity();
    if !meta.branch_diff_hash.is_empty()
        && review_flow_driver::cached_diff_matches_scope(&id, scope)
    {
        return Ok(meta.raw.clone());
    }
    er_engine::diff_load::fetch_raw(ports, &id, scope, false)
}

/// The view a flow on `content`'s tab reviews for `scope`, or why it cannot
/// be reviewed.
pub fn review_flow_inputs(
    ports: &Ports,
    content: &TabContent,
    config: &ErConfig,
    scope: &str,
) -> Result<ReviewFlowInputs> {
    let tab = ContentTab {
        ports,
        content,
        config,
    };
    review_flow_driver::review_flow_inputs(&tab, scope)
}

/// The filtered diff the first of `contents` that shows `target` has now,
/// fetched fresh, which acceptance checks a run against.
pub fn current_review_diff<'a>(
    ports: &Ports,
    contents: impl IntoIterator<Item = &'a TabContent>,
    target: &BackgroundTaskTarget,
) -> Result<String> {
    let content = contents
        .into_iter()
        .find(|content| {
            review_flow_driver::shows_target(
                &content.tab.identity(),
                &content.context.er_dir,
                target,
            )
        })
        .context("The captured review target is no longer open; start a new Review")?;
    let raw = er_engine::diff_load::fetch_raw(ports, &content.tab.identity(), &target.scope, false)
        .context("fetch the current diff for review acceptance")?;
    Ok(review_flow_driver::filtered_review_diff(
        &raw,
        &target.repo_root,
        target.remote_repo.as_deref(),
    ))
}
