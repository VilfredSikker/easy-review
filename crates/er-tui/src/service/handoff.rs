//! The clipboard and editor handoffs. They run on the loop at the key press,
//! as the App's do, behind a trait so a test records them and no test
//! reaches a real clipboard or editor.

use std::path::Path;

pub trait Launch {
    fn clipboard(&self, text: &str) -> anyhow::Result<()>;
    fn editor(&self, repo_root: &str, path: &Path, line: usize) -> anyhow::Result<()>;
}

/// The operating system's clipboard and the user's editor.
#[cfg_attr(test, expect(dead_code, reason = "tests launch through the recorder"))]
struct Os;

impl Launch for Os {
    fn clipboard(&self, text: &str) -> anyhow::Result<()> {
        er_engine::os_launch::copy_to_clipboard(text)
    }

    fn editor(&self, repo_root: &str, path: &Path, line: usize) -> anyhow::Result<()> {
        er_engine::os_launch::open_editor(repo_root, path, line)
    }
}

#[cfg(not(test))]
pub fn launcher() -> Box<dyn Launch> {
    Box::new(Os)
}

/// Each launch a test asked for.
#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Launched {
    Clipboard(String),
    Editor {
        repo_root: String,
        path: std::path::PathBuf,
        line: usize,
    },
}

/// Records the launches, and answers each with `fail` when it is set.
#[cfg(test)]
#[derive(Clone, Default)]
pub struct Recorder {
    pub launched: std::sync::Arc<std::sync::Mutex<Vec<Launched>>>,
    pub fail: std::sync::Arc<std::sync::Mutex<Option<String>>>,
}

#[cfg(test)]
impl Recorder {
    fn record(&self, launched: Launched) -> anyhow::Result<()> {
        if let Some(error) = self.fail.lock().map_or(None, |fail| fail.clone()) {
            anyhow::bail!(error);
        }
        if let Ok(mut all) = self.launched.lock() {
            all.push(launched);
        }
        Ok(())
    }

    pub fn taken(&self) -> Vec<Launched> {
        self.launched
            .lock()
            .map(|mut all| std::mem::take(&mut *all))
            .unwrap_or_default()
    }
}

#[cfg(test)]
impl Launch for Recorder {
    fn clipboard(&self, text: &str) -> anyhow::Result<()> {
        self.record(Launched::Clipboard(text.to_string()))
    }

    fn editor(&self, repo_root: &str, path: &Path, line: usize) -> anyhow::Result<()> {
        self.record(Launched::Editor {
            repo_root: repo_root.to_string(),
            path: path.to_path_buf(),
            line,
        })
    }
}
