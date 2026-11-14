//! What a reader can see: the workspace targets and the active one.
//!
//! The view is immutable once published. The actor builds the next one, bumps
//! the revision, and swaps the `Arc`; a reader that grabbed the previous view
//! keeps rendering it until it asks again.

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

/// The whole read side of one workspace, published per revision.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WorkspaceView {
    pub revision: u64,
    pub targets: Vec<WorkspaceTarget>,
    /// Index into `targets` of the target the user last opened.
    pub active: Option<usize>,
    /// The last effect failure, cleared by the next successful open.
    pub last_error: Option<String>,
}

/// The actor-thread state. Private to the crate: readers only ever see a
/// published [`WorkspaceView`].
pub(crate) struct WorkspaceState {
    view: WorkspaceView,
    revision: u64,
}

impl WorkspaceState {
    pub(crate) fn new() -> Self {
        Self {
            view: WorkspaceView::default(),
            revision: 0,
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
