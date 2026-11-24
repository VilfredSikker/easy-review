//! The start's PR probe (P68.1): one read of whether the first tab's branch
//! has a pull request, on its own thread as the App's start makes it, so the
//! loop never waits on GitHub. It only reads, and only through the port's
//! probe; what it finds lands in `TuiView` on a later tick.

use std::sync::mpsc::{channel, Receiver};
use std::sync::Arc;

use er_engine::github::{BranchPrProbe, GitHub};
use er_engine::TabKey;

/// A probe in flight for one tab.
pub struct Probe {
    key: TabKey,
    answer: Receiver<BranchPrProbe>,
}

impl Probe {
    /// Starts the probe of `repo_root`'s branch. A failed probe answers
    /// `Unknown`, which marks nothing.
    pub fn spawn(github: Arc<dyn GitHub>, key: TabKey, repo_root: String) -> Self {
        let (tx, answer) = channel();
        std::thread::spawn(move || {
            let probe = github
                .probe_pr_for_current_branch(&repo_root)
                .unwrap_or(BranchPrProbe::Unknown);
            let _ = tx.send(probe);
        });
        Self { key, answer }
    }

    /// The probe's answer, once it has one.
    pub fn try_landed(&self) -> Option<(TabKey, BranchPrProbe)> {
        let probe = self.answer.try_recv().ok()?;
        Some((self.key, probe))
    }
}
