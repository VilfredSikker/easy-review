//! The Open hub's worktree picker and folder browser. The worktrees come
//! from the list the hub's opening asked the service for; a folder is one
//! local directory read, made on the loop at the key press as the App's.

use er_engine::dir_browse::read_directory;
use er_engine::git::Worktree;

use crate::input::OverlayAction;
use crate::screen::view::DirEntry;

/// What a picker or browser key asks of the loop.
#[derive(Debug, PartialEq, Eq)]
pub enum Pick {
    Stay,
    Close,
    /// Open this folder as a tab, or select the tab already on it.
    Open(String),
}

pub struct Worktrees {
    pub worktrees: Vec<Worktree>,
    pub selected: usize,
}

impl Worktrees {
    pub fn act(&mut self, action: OverlayAction) -> Pick {
        match action {
            OverlayAction::Next if self.selected + 1 < self.worktrees.len() => self.selected += 1,
            OverlayAction::Prev => self.selected = self.selected.saturating_sub(1),
            OverlayAction::Close => return Pick::Close,
            OverlayAction::SelectRow => {
                return match self.worktrees.get(self.selected) {
                    Some(worktree) => Pick::Open(worktree.path.clone()),
                    None => Pick::Close,
                }
            }
            _ => {}
        }
        Pick::Stay
    }
}

pub struct Directory {
    pub current_path: String,
    pub entries: Vec<DirEntry>,
    pub selected: usize,
}

impl Directory {
    /// The browser on the parent of the tab's repo root, as the App's starts.
    pub fn open(repo_root: &str) -> Self {
        let parent = std::path::Path::new(repo_root).parent();
        let start = parent.map_or_else(|| "/".to_string(), |p| p.to_string_lossy().to_string());
        Self::at(start)
    }

    fn at(current_path: String) -> Self {
        let entries = read_directory(&current_path)
            .into_iter()
            .map(|entry| DirEntry {
                name: entry.name,
                is_dir: entry.is_dir,
                is_git_repo: entry.is_git_repo,
            })
            .collect();
        Self {
            current_path,
            entries,
            selected: 0,
        }
    }

    /// Enter opens a repo or descends into a folder, and closes on a file;
    /// Backspace goes up a level.
    pub fn act(&mut self, action: OverlayAction) -> Pick {
        match action {
            OverlayAction::Next if self.selected + 1 < self.entries.len() => self.selected += 1,
            OverlayAction::Prev => self.selected = self.selected.saturating_sub(1),
            OverlayAction::Close => return Pick::Close,
            OverlayAction::GoUp => {
                let parent = std::path::Path::new(&self.current_path).parent();
                let parent = parent.map(|p| p.to_string_lossy().to_string());
                if let Some(parent) = parent.filter(|p| !p.is_empty()) {
                    *self = Self::at(parent);
                }
            }
            OverlayAction::SelectRow => {
                let Some(entry) = self.entries.get(self.selected) else {
                    return Pick::Stay;
                };
                let path = format!("{}/{}", self.current_path, entry.name);
                match (entry.is_dir, entry.is_git_repo) {
                    (true, true) => return Pick::Open(path),
                    (true, false) => *self = Self::at(path),
                    _ => return Pick::Close,
                }
            }
            _ => {}
        }
        Pick::Stay
    }
}
