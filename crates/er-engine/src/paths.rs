/// Selects where `.er/`-equivalent files live for a given tab.
///
/// `RepoLocal` preserves existing TUI behaviour — paths resolve to `<repo_root>/.er/`.
/// `Managed` is for the desktop app — paths resolve into a versioned app-data directory.
#[derive(Clone, Debug)]
pub enum ErRoot {
    /// Standard TUI mode: `.er/` lives inside the repo working tree.
    RepoLocal(String),
    /// Desktop managed mode: agent files and session/reviewed files are in separate dirs.
    Managed {
        /// Absolute path to the active agent's directory (e.g. `.../revisions/<id>/agents/claude`).
        agent_dir: String,
        /// Absolute path to the revision root (e.g. `.../revisions/<id>/`).
        /// session.json and `reviewed` file live here.
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

    /// Path for a named debug log file.
    pub fn debug_log_path(&self, name: &str) -> String {
        format!("{}/debug-{}.log", self.er_dir(), name)
    }
}
