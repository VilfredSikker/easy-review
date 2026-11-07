/// Selects where review-artifact files live for a given tab.
///
/// Review files always live under managed storage
/// (`<storage_root>/repos/<repo>/branches/<branch>/...`); nothing writes into
/// the repository working tree.
#[derive(Clone, Debug)]
pub enum ErRoot {
    /// Shared managed storage (TUI + Desktop).
    Managed {
        /// Absolute path to the branch review directory (review.json, questions.json, …).
        agent_dir: String,
        /// Same as `agent_dir` — `session.json` and `reviewed` live here.
        session_dir: String,
    },
}

impl ErRoot {
    /// A root whose review files live in `dir`.
    pub fn managed(dir: impl Into<String>) -> Self {
        let dir = dir.into();
        Self::Managed {
            agent_dir: dir.clone(),
            session_dir: dir,
        }
    }

    /// A root for a tab that has not resolved a branch or repository yet.
    ///
    /// Each repository gets its own directory so two unresolved tabs never
    /// share a `session.json`, and none of them writes inside a repository.
    pub fn unresolved(repo_root: &str, git: &dyn er_git::Git) -> Self {
        let dir = crate::storage::storage_root().join("unresolved");
        if repo_root.is_empty() {
            return Self::managed(dir.to_string_lossy().into_owned());
        }
        Self::managed(
            dir.join(crate::storage::slug_repo(git, repo_root))
                .to_string_lossy()
                .into_owned(),
        )
    }

    /// The directory where review.json, order.json, questions.json, github-comments.json, etc. live.
    pub fn er_dir(&self) -> String {
        let Self::Managed { agent_dir, .. } = self;
        agent_dir.clone()
    }

    /// Absolute path to session.json.
    pub fn session_path(&self) -> String {
        let Self::Managed { session_dir, .. } = self;
        format!("{session_dir}/session.json")
    }

    /// Absolute path to the `reviewed` marker file.
    pub fn reviewed_path(&self) -> String {
        let Self::Managed { session_dir, .. } = self;
        format!("{session_dir}/reviewed")
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
