//! A review flow's view and its acceptance diff, read from a tab's published
//! content by the rules the App reads its own tab by
//! (`er_engine::review_flow_driver::review_flow_inputs`). The desktop reads
//! them off the actor before it sends a flow command, so a flow never waits
//! on the App.

use std::sync::Arc;

use anyhow::{Context, Result};
use er_engine::ai::change_facts::OwnedRepoRules;
use er_engine::ai::review_md::ReviewMdSource;
use er_engine::config::ErConfig;
use er_engine::ports::Ports;
use er_engine::review_flow_driver::{self, ReviewFlowInputs, ReviewedTab};
use er_engine::tab_identity::TabIdentity;
use er_engine::task_target::BackgroundTaskTarget;

use crate::content::TabContent;
use crate::protocol::FlowContext;

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
        review_rules(self.content, self.config)
    }

    fn review_md_source(&self) -> ReviewMdSource {
        review_flow_driver::review_md_source(self.ports, &self.content.tab.identity())
    }
}

/// The repo's tables a review of `content`'s tab reads, under the key the
/// diff loaded them by, as `diff(tab)` reads them.
pub fn review_rules(content: &TabContent, config: &ErConfig) -> OwnedRepoRules {
    let meta = &content.diff.meta;
    OwnedRepoRules {
        file_kinds: config
            .file_kinds
            .repo(&meta.rules_key)
            .cloned()
            .unwrap_or_default(),
        risk: config.risk_rules_merged(&meta.rules_key, meta.risk_artifact.clone()),
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
    er_engine::diff_load::fetch_raw(ports, &id, scope, meta.committed_unpushed)
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

/// What a flow command on `content`'s tab carries: the settings view's
/// config and the saved selection's effort (A18), and the view `scope` names
/// when the command reviews one.
pub fn flow_context(
    ports: &Ports,
    content: &TabContent,
    config: Arc<ErConfig>,
    scope: Option<&str>,
) -> FlowContext {
    let effort = config
        .ai_hub
        .resolve_default_selection(&config.agent)
        .effort;
    let view = match scope {
        Some(scope) => {
            review_flow_inputs(ports, content, &config, scope).map_err(|error| error.to_string())
        }
        None => Err("this command reviews no view".into()),
    };
    FlowContext {
        config,
        effort,
        view,
    }
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
