//! Session-scoped, app-level background review tasks.
//!
//! Unlike per-tab `command_rx`/`command_status` state, these tasks live on
//! the `App` so they survive tab switches. Used currently for AI *review*
//! commands started from the desktop UI; summary/questions/validate stay
//! tab-local.
//!
//! All state is session-only — no persistence across app restarts.

use std::collections::{HashMap, VecDeque};
use std::time::{SystemTime, UNIX_EPOCH};

use super::{AgentLogEntry, AgentLogSource, CommandStatus, TabState};

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
    /// [`kind_label`] for the intent of the agent running against it.
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
/// `"expert:<id>"`, `"professor"`, `"triage"`, or `"tour"`.
pub fn kind_label(kind: &str) -> String {
    match kind {
        "" | "review" | "general" => "Review".to_string(),
        "tour" => "Guide".to_string(),
        other => {
            // expert:<id>, professor, and triage all resolve through the
            // shared finding-agent label map.
            let id = other.strip_prefix("expert:").unwrap_or(other);
            crate::ai::agent_label_for_category(id).to_string()
        }
    }
}

#[derive(Debug, Clone)]
pub struct BackgroundTask {
    pub id: String,
    pub kind: String,
    pub target: BackgroundTaskTarget,
    pub status: CommandStatus,
    pub started_at_ms: u128,
    pub finished_at_ms: Option<u128>,
    pub error: Option<String>,
}

impl BackgroundTask {
    pub fn new(kind: String, target: BackgroundTaskTarget) -> Self {
        let now = unix_now_ms();
        let id = format!("{}:{}:{}", kind, target.key(), now);
        Self {
            id,
            kind,
            target,
            status: CommandStatus::Running,
            started_at_ms: now,
            finished_at_ms: None,
            error: None,
        }
    }
}

/// Whether to emit `[bg]` debug logs about background task lifecycle.
/// Opt-in only — set `ER_DESKTOP_DEBUG_BG=1` (poll/snapshot logging is very noisy).
pub fn debug_bg_enabled() -> bool {
    std::env::var("ER_DESKTOP_DEBUG_BG").as_deref() == Ok("1")
}

pub fn unix_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// A review/agent task accepted but not yet started because the
/// concurrent-review cap was reached. Holds everything needed to launch
/// later from the dispatch loop. `task.started_at_ms` is the enqueue time
/// until launch, when it's refreshed.
#[derive(Debug, Clone)]
pub(crate) struct PendingBackgroundTask {
    pub task: BackgroundTask,
    pub command_name: String,
    pub prompt: String,
    pub prepared_diff: bool,
    /// Optional action-bound provider/model/effort selection. This is captured
    /// when a TUI action is queued so it cannot leak into later actions or be
    /// replaced by a changed global default before launch.
    pub ai_selection: Option<crate::config::AiSelection>,
}

/// In-flight + recently finished background task channels. The `App` owns
/// this; one entry per task id.
pub(crate) struct BackgroundTaskHandle {
    pub task: BackgroundTask,
    /// One-shot result channel; produces `Ok(())` on success or an `Err`
    /// describing the failure when the subprocess finishes.
    pub result_rx: std::sync::mpsc::Receiver<anyhow::Result<()>>,
    /// Per-task log entries draining into `App` log buffer.
    pub log_rx: std::sync::mpsc::Receiver<AgentLogEntry>,
    /// Ring buffer of recent log entries for app-wide log access (capped at 500).
    pub recent_log: VecDeque<AgentLogEntry>,
}

/// Drained view used by snapshot building. Includes Running plus
/// Done/Failed within the recent window so the UI can render transient
/// "task done" toasts.
#[derive(Debug, Clone)]
pub struct BackgroundTaskSnapshot {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub target_label: String,
    pub scope: String,
    /// Stable target identity for UI matching (not just display labels).
    pub repo_root: String,
    pub branch_label: String,
    pub pr_number: Option<u64>,
    pub remote_repo: Option<String>,
    pub status: String,
    pub error: Option<String>,
    pub started_at_ms: u128,
    pub finished_at_ms: Option<u128>,
    /// Last 40 log entries from the handle's ring buffer. Empty for retired tasks.
    pub recent_log: Vec<AgentLogEntry>,
}

impl BackgroundTaskSnapshot {
    pub fn from_task(task: &BackgroundTask) -> Self {
        let status = match &task.status {
            CommandStatus::Running => "running",
            CommandStatus::Done => "done",
            CommandStatus::Failed(_) => "failed",
        };
        Self {
            id: task.id.clone(),
            kind: task.kind.clone(),
            // `label` is the agent's intent (what it's doing); `target_label`
            // stays the branch/PR (where it's doing it) for secondary context.
            label: kind_label(&task.kind),
            target_label: task.target.display_label(),
            scope: task.target.scope.clone(),
            repo_root: task.target.repo_root.clone(),
            branch_label: task.target.branch_label.clone(),
            pr_number: task.target.pr_number,
            remote_repo: task.target.remote_repo.clone(),
            status: status.to_string(),
            error: task.error.clone(),
            started_at_ms: task.started_at_ms,
            finished_at_ms: task.finished_at_ms,
            recent_log: Vec::new(),
        }
    }
}

impl TabState {
    /// Does this tab represent the same review target? Used to merge
    /// app-level task state into the per-tab snapshot view.
    pub fn matches_target(&self, target: &BackgroundTaskTarget) -> bool {
        if self.repo_root != target.repo_root {
            return false;
        }
        if self.pr_number != target.pr_number {
            return false;
        }
        if self.remote_repo != target.remote_repo {
            return false;
        }
        // Branch label: prefer local_branch_view, fall back to current_branch.
        let label = self
            .local_branch_view
            .clone()
            .unwrap_or_else(|| self.current_branch.clone());
        label == target.branch_label
    }

    /// Convert a target's completion into a synthetic agent log entry so
    /// the active tab's agent log panel surfaces status even when the
    /// task started on a different tab.
    pub(crate) fn push_synthetic_log(&mut self, name: &str, text: String, source: AgentLogSource) {
        self.agent_log.push_back(AgentLogEntry {
            timestamp: std::time::Instant::now(),
            command_name: name.to_string(),
            source,
            text,
        });
        if self.agent_log.len() > 5000 {
            self.agent_log.pop_front();
        }
    }
}

pub(crate) type BackgroundTaskMap = HashMap<String, BackgroundTaskHandle>;

#[cfg(test)]
mod tests {
    use super::*;

    fn target(branch: &str, scope: &str) -> BackgroundTaskTarget {
        BackgroundTaskTarget {
            repo_root: "/repo".to_string(),
            er_dir: "/repo/.er".to_string(),
            branch_label: branch.to_string(),
            base_branch: "main".to_string(),
            scope: scope.to_string(),
            pr_number: None,
            remote_repo: None,
            managed_local: false,
        }
    }

    #[test]
    fn target_equality_uses_all_fields() {
        let a = target("feat-a", "branch");
        let b = target("feat-a", "branch");
        assert_eq!(a, b);
        assert_eq!(a.key(), b.key());

        let c = target("feat-b", "branch");
        assert_ne!(a, c);

        let d = target("feat-a", "unstaged");
        assert_ne!(a, d);
    }

    #[test]
    fn kind_label_shows_agent_intent() {
        assert_eq!(kind_label("review"), "Review");
        assert_eq!(kind_label("general"), "Review");
        assert_eq!(kind_label(""), "Review");
        assert_eq!(kind_label("tour"), "Guide");
        assert_eq!(kind_label("professor"), "Professor");
        assert_eq!(kind_label("triage"), "Triage");
        assert_eq!(kind_label("expert:security"), "Security");
        assert_eq!(kind_label("expert:performance"), "Performance");
    }

    #[test]
    fn snapshot_label_is_intent_not_target() {
        // Two agents on the same branch must produce distinct labels so the
        // UI doesn't show the branch name twice.
        let t = target("feat-a", "branch");
        let sec = BackgroundTask::new("expert:security".to_string(), t.clone());
        let prof = BackgroundTask::new("professor".to_string(), t.clone());

        let sec_snap = BackgroundTaskSnapshot::from_task(&sec);
        let prof_snap = BackgroundTaskSnapshot::from_task(&prof);

        assert_eq!(sec_snap.label, "Security");
        assert_eq!(prof_snap.label, "Professor");
        assert_ne!(sec_snap.label, prof_snap.label);
        // Target context is preserved separately.
        assert_eq!(sec_snap.target_label, "feat-a");
        assert_eq!(prof_snap.target_label, "feat-a");
    }

    #[test]
    fn task_id_includes_target_and_timestamp() {
        let t = target("feat-a", "branch");
        let task = BackgroundTask::new("review".to_string(), t.clone());
        assert!(task.id.starts_with("review:"));
        assert!(task.id.contains(&t.key()));
        assert!(matches!(task.status, crate::app::CommandStatus::Running));
    }

    #[test]
    fn tab_matches_target_on_branch_label() {
        use crate::app::TabState;
        // Build a minimal TabState by reusing the engine's constructor path
        // is heavy — instead, just construct the small bit we need with the
        // public default-like API. Since TabState doesn't expose a stub
        // builder, use new_with_base if the repo exists; otherwise skip.
        let tmp = std::env::temp_dir().join("er-bg-tab-match-test");
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        // Make it a git repo so TabState::new works.
        let ok = std::process::Command::new("git")
            .arg("init")
            .current_dir(&tmp)
            .output()
            .is_ok();
        if !ok {
            return;
        }
        let mut tab = match TabState::new(tmp.to_string_lossy().to_string()) {
            Ok(t) => t,
            Err(_) => return,
        };
        tab.current_branch = "feat-a".to_string();
        let t_match = BackgroundTaskTarget {
            repo_root: tab.repo_root.clone(),
            er_dir: tab.er_dir(),
            branch_label: "feat-a".to_string(),
            base_branch: "main".to_string(),
            scope: "branch".to_string(),
            pr_number: None,
            remote_repo: None,
            managed_local: false,
        };
        let t_nomatch = BackgroundTaskTarget {
            branch_label: "feat-b".to_string(),
            ..t_match.clone()
        };
        assert!(tab.matches_target(&t_match));
        assert!(!tab.matches_target(&t_nomatch));
    }
}
