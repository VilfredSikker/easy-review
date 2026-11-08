//! The subprocess ports, carried as one cloneable value.
//!
//! `Ports` is passed as the last constructor parameter and cloned into
//! background work, so every caller reaches git and GitHub through the same
//! handles and a test can script the answers. There is no global and no
//! `Default`.

use std::sync::Arc;

use er_git::{CliGit, FakeGit, Git};
use er_github::{default_handle, FakeGitHub, GitHub};

/// Cloneable handles to the subprocess ports.
#[derive(Clone)]
pub struct Ports {
    pub git: Arc<dyn Git>,
    pub github: Arc<dyn GitHub>,
}

impl Ports {
    /// The real adapters: subprocesses and the network.
    pub fn cli() -> Self {
        Self {
            git: Arc::new(CliGit),
            github: default_handle(),
        }
    }

    /// Unscripted fakes; tests script them through [`Ports::with_git`] and
    /// [`Ports::with_github`].
    pub fn for_test() -> Self {
        Self {
            git: Arc::new(FakeGit::default()),
            github: Arc::new(FakeGitHub::default()),
        }
    }

    /// A handle over a specific git implementation, with an unscripted GitHub.
    pub fn with_git(git: Arc<dyn Git>) -> Self {
        Self {
            git,
            github: Arc::new(FakeGitHub::default()),
        }
    }

    /// A handle over a specific GitHub implementation, with an unscripted git.
    pub fn with_github(github: Arc<dyn GitHub>) -> Self {
        Self {
            git: Arc::new(FakeGit::default()),
            github,
        }
    }
}

impl std::fmt::Debug for Ports {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ports").finish_non_exhaustive()
    }
}
