//! What a reader can see: the workspace targets, the active one, and each
//! loaded tab's content (ADR 0071).
//!
//! The view is immutable once published. The actor builds the next one, bumps
//! the revision, and swaps the `Arc`; a reader that grabbed the previous view
//! keeps rendering it until it asks again.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fmt;
use std::sync::Arc;

use er_engine::ai::review_flow::ReviewFlow;
use er_engine::arena::{ArenaProgressState, RunStatus};
use er_engine::task_target::BackgroundTaskTarget;
use er_engine::TabKey;

use crate::content::{self, ContentPatch, PatchRefused, TabContent};

/// One PR the workspace has open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorkspaceTarget {
    /// The local project root, empty when the caller had no local project.
    pub repo_root: String,
    /// `owner/repo`, empty when the caller could not resolve a remote.
    pub remote: String,
    pub pr_number: Option<u64>,
    /// The head branch, when the caller knew it. The open commands do not.
    pub branch: Option<String>,
    /// Short label for the front end: `owner/repo#12`, or `pr-12`.
    pub label: String,
    /// The PR state fetched off-actor (`OPEN`, `MERGED`, …), when the remote
    /// was known and the fetch succeeded.
    pub pr_state: Option<String>,
}

impl WorkspaceTarget {
    /// The actor this target was opened on.
    pub fn workspace_key(&self) -> String {
        er_engine::tab_identity::workspace_key(Some(&self.repo_root), &self.remote)
    }
}

/// The whole read side of one workspace, published per revision.
#[derive(Clone, Default)]
pub struct WorkspaceView {
    pub revision: u64,
    pub targets: Vec<WorkspaceTarget>,
    /// Index into `targets` of the target the user last opened.
    pub active: Option<usize>,
    /// The last effect failure, cleared by the next successful open.
    pub last_error: Option<String>,
    /// The arena runs this actor started, by run id order. Run records stay
    /// on disk; this section carries what only the live run knows.
    pub arena_runs: Vec<LiveArenaRun>,
    /// The review flows this actor runs or recovered, by bucket.
    pub review_flows: Vec<LiveReviewFlow>,
    /// Each loaded tab's content, by the key its caller gave. A publish
    /// copies these pointers; the sections behind them are shared.
    pub tabs: BTreeMap<TabKey, Arc<TabContent>>,
    /// Why a tab's first load failed, by its key, until a load succeeds or
    /// the tab closes. A tab that loaded keeps its content instead.
    pub load_errors: BTreeMap<TabKey, LoadError>,
}

/// A failed first load, at the revision that recorded it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadError {
    pub revision: u64,
    pub message: String,
}

/// Content compares by its section revisions: a section is replaced, never
/// changed in place, so equal revisions are equal content.
impl PartialEq for WorkspaceView {
    fn eq(&self, other: &Self) -> bool {
        let revisions = |view: &Self| {
            view.tabs
                .iter()
                .map(|(key, content)| {
                    (
                        *key,
                        content.diff.revision,
                        content.reviewed.revision,
                        content.ai.revision,
                        content.pr.revision,
                        content.mode_revision,
                    )
                })
                .collect::<Vec<_>>()
        };
        self.revision == other.revision
            && self.targets == other.targets
            && self.active == other.active
            && self.last_error == other.last_error
            && self.arena_runs == other.arena_runs
            && self.review_flows == other.review_flows
            && self.load_errors == other.load_errors
            && revisions(self) == revisions(other)
    }
}

impl fmt::Debug for WorkspaceView {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WorkspaceView")
            .field("revision", &self.revision)
            .field("targets", &self.targets)
            .field("active", &self.active)
            .field("last_error", &self.last_error)
            .field("arena_runs", &self.arena_runs)
            .field("review_flows", &self.review_flows)
            .field("tabs", &self.tabs.values().collect::<Vec<_>>())
            .field("load_errors", &self.load_errors)
            .finish()
    }
}

impl WorkspaceView {
    /// The content loaded under `key`.
    pub fn tab(&self, key: TabKey) -> Option<&Arc<TabContent>> {
        self.tabs.get(&key)
    }

    pub fn arena_run(&self, run_id: &str) -> Option<&LiveArenaRun> {
        self.arena_runs.iter().find(|run| run.run_id == run_id)
    }

    pub fn review_flow(&self, er_dir: &str) -> Option<&LiveReviewFlow> {
        self.review_flows.iter().find(|flow| flow.er_dir == er_dir)
    }
}

/// One review flow as the view shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct LiveReviewFlow {
    pub er_dir: String,
    pub flow: ReviewFlow,
    /// The flow is over but a stage or an arena run still writes the bucket.
    pub draining: bool,
    /// The target each flow-owned arena run captured, by run id. A run is read
    /// or accepted only against the target it started for.
    pub run_targets: Vec<(String, BackgroundTaskTarget)>,
    /// For an interrupted flow, the stages a re-run would run again; empty when
    /// it would start over.
    pub rerun_stages: Vec<String>,
}

/// One arena run's live state, copied from its control when the actor
/// publishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LiveArenaRun {
    pub run_id: String,
    /// The bucket the run writes to.
    pub er_dir: String,
    pub status: RunStatus,
    pub progress: ArenaProgressState,
}

/// The actor-thread state. Private to the crate: readers only ever see a
/// published [`WorkspaceView`].
pub(crate) struct WorkspaceState {
    view: WorkspaceView,
    revision: u64,
    /// Keys whose tab closed, the newest `CLOSED_KEPT`. A key is never issued
    /// twice, so a load that lands for one after its drop belongs to a tab
    /// that is gone. A load sent after the close is refused at dispatch, by
    /// the app registry's list, so only the loads in flight need the set.
    closed: HashSet<TabKey>,
    closed_order: VecDeque<TabKey>,
    /// The sequence of the last reviewed patch applied, per bucket.
    marks_seq: HashMap<String, u64>,
}

/// How many closed keys the workspace remembers for the loads still in flight.
pub(crate) const CLOSED_KEPT: usize = 256;

impl WorkspaceState {
    pub(crate) fn new() -> Self {
        Self {
            view: WorkspaceView::default(),
            revision: 0,
            closed: HashSet::new(),
            closed_order: VecDeque::new(),
            marks_seq: HashMap::new(),
        }
    }

    pub(crate) fn view(&self) -> &WorkspaceView {
        &self.view
    }

    /// Records an opened target, replacing the entry with the same identity so
    /// reopening a PR refreshes its state instead of growing the list.
    /// Returns the new revision.
    pub(crate) fn apply_opened(&mut self, target: WorkspaceTarget) -> u64 {
        if let Some(index) = self
            .view
            .targets
            .iter()
            .position(|existing| same_target(existing, &target))
        {
            self.view.targets[index] = target;
            self.view.active = Some(index);
        } else {
            self.view.targets.push(target);
            self.view.active = Some(self.view.targets.len() - 1);
        }
        self.view.last_error = None;
        self.bump()
    }

    /// Records an effect failure. The previous active target stays active; the
    /// error is additive so a failed reopen does not blank the workspace.
    pub(crate) fn apply_failure(&mut self, message: String) -> u64 {
        self.view.last_error = Some(message);
        self.bump()
    }

    /// Raises the revision after a write the view does not carry (an
    /// annotation sidecar change), so readers refresh.
    pub(crate) fn bump_revision(&mut self) -> u64 {
        self.bump()
    }

    /// Replaces the live arena section. The caller publishes it with the
    /// next revision.
    pub(crate) fn set_arena_runs(&mut self, runs: Vec<LiveArenaRun>) {
        self.view.arena_runs = runs;
    }

    /// Replaces the flow section and answers the buckets whose flow changed,
    /// including one that left. The caller publishes the next revision.
    pub(crate) fn set_review_flows(&mut self, flows: Vec<LiveReviewFlow>) -> Vec<String> {
        let mut changed: Vec<String> = flows
            .iter()
            .filter(|flow| self.view.review_flow(&flow.er_dir) != Some(*flow))
            .map(|flow| flow.er_dir.clone())
            .collect();
        changed.extend(
            self.view
                .review_flows
                .iter()
                .filter(|old| !flows.iter().any(|flow| flow.er_dir == old.er_dir))
                .map(|old| old.er_dir.clone()),
        );
        self.view.review_flows = flows;
        changed
    }

    /// Applies an effect's content patch with a new revision, or refuses it
    /// when the tab changed since the effect started. A refused patch moves
    /// nothing but the reviewed marks it carries: they were already saved,
    /// so they land, and their sequence orders the writes still in flight.
    /// Answers the revision and how many reviewed marks a load cleared.
    pub(crate) fn apply_patch(
        &mut self,
        mut patch: ContentPatch,
    ) -> Result<(u64, usize), PatchRefused> {
        let (mut loads, mut drops) = (Vec::new(), Vec::new());
        content::lifecycle_keys(&patch, &mut loads, &mut drops);
        let refused = if loads.iter().any(|key| self.closed.contains(key)) {
            Err(PatchRefused::Moved)
        } else {
            content::check(&self.view.tabs, &patch)
        };
        if let Err(refused) = refused {
            let mut saved = ContentPatch::Several(content::saved_marks(patch));
            if !matches!(&saved, ContentPatch::Several(parts) if parts.is_empty()) {
                content::drop_stale_marks(&mut saved, &mut self.marks_seq);
                let revision = self.bump();
                content::apply(&mut self.view.tabs, saved, revision);
            }
            return Err(refused);
        }
        content::drop_stale_marks(&mut patch, &mut self.marks_seq);
        for key in loads.iter().chain(&drops) {
            self.view.load_errors.remove(key);
        }
        for key in drops {
            self.remember_closed(key);
        }
        let revision = self.bump();
        let cleared = content::apply(&mut self.view.tabs, patch, revision);
        Ok((revision, cleared))
    }

    /// Records why `key`'s first load failed, at a new revision the caller
    /// publishes. A key that closed meanwhile records nothing.
    pub(crate) fn record_load_error(&mut self, key: TabKey, message: String) {
        if self.closed.contains(&key) || self.view.tabs.contains_key(&key) {
            return;
        }
        let revision = self.bump();
        self.view
            .load_errors
            .insert(key, LoadError { revision, message });
    }

    /// Records the close of a key that holds no content, so a first load in
    /// flight for it is refused when it lands, and moves no revision: the view
    /// does not change. False when the key holds content, which a drop patch
    /// removes with a new revision.
    pub(crate) fn drop_unloaded(&mut self, key: TabKey) -> bool {
        if self.view.tabs.contains_key(&key) {
            return false;
        }
        self.view.load_errors.remove(&key);
        self.remember_closed(key);
        true
    }

    fn remember_closed(&mut self, key: TabKey) {
        if !self.closed.insert(key) {
            return;
        }
        self.closed_order.push_back(key);
        while self.closed_order.len() > CLOSED_KEPT {
            if let Some(oldest) = self.closed_order.pop_front() {
                self.closed.remove(&oldest);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn closed_len(&self) -> usize {
        self.closed.len()
    }

    fn bump(&mut self) -> u64 {
        self.revision += 1;
        self.view.revision = self.revision;
        self.revision
    }
}

/// Two targets are the same workspace entry when they name the same PR in the
/// same repo. A missing PR number (a remote-only tab) falls back to the label.
fn same_target(left: &WorkspaceTarget, right: &WorkspaceTarget) -> bool {
    left.repo_root == right.repo_root
        && left.remote == right.remote
        && left.pr_number == right.pr_number
        && (left.pr_number.is_some() || left.label == right.label)
}
