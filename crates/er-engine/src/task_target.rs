//! The identity of one background agent task: which review target it runs
//! against. The App's queue and the review-flow driver both key on it.

/// Stable identity used to dedup background tasks and match them back to
/// the tab(s) they belong to. Two tasks for the same target cannot run
/// concurrently.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BackgroundTaskTarget {
    pub repo_root: String,
    pub er_dir: String,
    pub branch_label: String,
    pub base_branch: String,
    pub scope: String,
    pub pr_number: Option<u64>,
    pub remote_repo: Option<String>,
    /// True for Desktop managed-local reviews: er_dir is outside repo_root but
    /// git commands still need cwd = repo_root (not er_dir).
    pub managed_local: bool,
}

impl BackgroundTaskTarget {
    /// Stable string key for ID generation and HashMap lookups.
    pub fn key(&self) -> String {
        format!(
            "{}|{}|{}|{}|{}|{}",
            self.repo_root,
            self.branch_label,
            self.base_branch,
            self.scope,
            self.pr_number.map(|n| n.to_string()).unwrap_or_default(),
            self.remote_repo.clone().unwrap_or_default(),
        )
    }

    /// Human-readable label for the review *target* ("branch-name",
    /// "owner/repo#123", etc.). This is the *where*, not the *what* — use
    /// `kind_label` for the intent of the agent running against it.
    pub fn display_label(&self) -> String {
        if let (Some(slug), Some(n)) = (&self.remote_repo, self.pr_number) {
            return format!("{}#{}", slug, n);
        }
        if self.branch_label.is_empty() {
            self.repo_root.clone()
        } else {
            self.branch_label.clone()
        }
    }
}

/// Human-readable name for the *kind* of agent (the intent of the run) so
/// concurrent tasks against the same target are distinguishable in the UI.
/// Without this every agent on a branch shows the branch name and they look
/// identical (e.g. a security pass and a professor pass running side by side).
///
/// Accepts the raw `BackgroundTask::kind` string: `"review"`/`"general"`,
/// `"expert:<id>"`, `"professor"`, `"triage"`, `"tour"`, or `"diagram:<kind>"`.
pub fn kind_label(kind: &str) -> String {
    match kind {
        "" | "review" | "general" => "Review".to_string(),
        "tour" => "Guide".to_string(),
        other if other.starts_with("diagram:") => {
            let sub = other.trim_start_matches("diagram:");
            format!("Diagram ({})", crate::ai::diagram_kind_label(sub))
        }
        other => {
            // expert:<id>, professor, and triage all resolve through the
            // shared finding-agent label map.
            let id = other.strip_prefix("expert:").unwrap_or(other);
            crate::ai::agent_label_for_id(id).to_string()
        }
    }
}
