/// Selects where review-artifact files live for a given tab.
///
/// `Managed` (default for TUI and Desktop) resolves to
/// `<app_data>/easy-review/repos/<repo>/branches/<branch>/`.
/// `RepoLocal` (`ER_REPO_LOCAL=1`) uses `<repo_root>/.er/` for debugging.
#[derive(Clone, Debug)]
pub enum ErRoot {
    /// Debug / escape hatch: `.er/` inside the repo working tree.
    RepoLocal(String),
    /// Shared managed storage (TUI + Desktop).
    Managed {
        /// Absolute path to the branch review directory (review.json, questions.json, …).
        agent_dir: String,
        /// Same as `agent_dir` — `session.json` and `reviewed` live here.
        session_dir: String,
    },
}

impl ErRoot {
    /// The directory where review.json, order.json, questions.json, github-comments.json, etc. live.
    pub fn er_dir(&self) -> String {
        match self {
            ErRoot::RepoLocal(repo_root) => format!("{repo_root}/.er"),
            ErRoot::Managed { agent_dir, .. } => agent_dir.clone(),
        }
    }

    /// Absolute path to session.json.
    pub fn session_path(&self) -> String {
        match self {
            ErRoot::RepoLocal(repo_root) => format!("{repo_root}/.er/session.json"),
            ErRoot::Managed { session_dir, .. } => format!("{session_dir}/session.json"),
        }
    }

    /// Absolute path to the `reviewed` marker file.
    pub fn reviewed_path(&self) -> String {
        match self {
            ErRoot::RepoLocal(repo_root) => format!("{repo_root}/.er/reviewed"),
            ErRoot::Managed { session_dir, .. } => format!("{session_dir}/reviewed"),
        }
    }

    /// Directory for file snapshots (watched-file baseline copies).
    pub fn snapshots_dir(&self) -> String {
        format!("{}/snapshots", self.er_dir())
    }

    /// Path for the agent subprocess debug log (overwritten each run).
    pub fn debug_log_path(&self) -> String {
        format!("{}/debug-agent.log", self.er_dir())
    }
}
