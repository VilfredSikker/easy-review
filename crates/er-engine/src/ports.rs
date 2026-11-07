//! The subprocess ports, carried as one cloneable value.
//!
//! `Ports` is passed as the last constructor parameter and cloned into
//! background work, so every caller reaches git through the same handle and a
//! test can script the answers. There is no global and no `Default`.

use std::sync::Arc;

use er_git::{CliGit, FakeGit, Git};

/// Cloneable handles to the subprocess ports.
#[derive(Clone)]
pub struct Ports {
    pub git: Arc<dyn Git>,
}

impl Ports {
    /// The real adapters: subprocesses and the network.
    pub fn cli() -> Self {
        Self {
            git: Arc::new(CliGit),
        }
    }

    /// An unscripted fake; tests script it through [`Ports::with_git`].
    pub fn for_test() -> Self {
        Self {
            git: Arc::new(FakeGit::default()),
        }
    }

    /// A handle over a specific git implementation.
    pub fn with_git(git: Arc<dyn Git>) -> Self {
        Self { git }
    }
}

impl std::fmt::Debug for Ports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ports").finish_non_exhaustive()
    }
}
