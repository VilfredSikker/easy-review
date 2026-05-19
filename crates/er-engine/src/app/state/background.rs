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

    /// Human-readable label for UI ("branch-name", "owner/repo#123", etc.).
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
/// Auto-on in debug builds (e.g. `cargo tauri dev`); release builds require
/// `ER_DESKTOP_DEBUG_BG=1`.
pub fn debug_bg_enabled() -> bool {
    cfg!(debug_assertions) || std::env::var("ER_DESKTOP_DEBUG_BG").as_deref() == Ok("1")
}

pub fn unix_now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
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
            label: task.target.display_label(),
            target_label: task.target.display_label(),
            scope: task.target.scope.clone(),
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
