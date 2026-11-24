//! One directory's listing for a folder browser: hidden entries skipped,
//! directories first, then by name, each directory marked when it holds a
//! git repo.

/// A directory entry for the filesystem browser
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    pub name: String,
    pub is_dir: bool,
    pub is_git_repo: bool,
}

/// Read directory entries, sorted: directories first (with git repo marker), then files
pub fn read_directory(path: &str) -> Vec<DirEntry> {
    let mut entries = Vec::new();
    if let Ok(read_dir) = std::fs::read_dir(path) {
        for entry in read_dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Skip hidden files/dirs (except .git check is internal)
            if name.starts_with('.') {
                continue;
            }
            if let Ok(metadata) = entry.metadata() {
                let is_dir = metadata.is_dir();
                let is_git_repo = is_dir && entry.path().join(".git").exists();
                entries.push(DirEntry {
                    name,
                    is_dir,
                    is_git_repo,
                });
            }
        }
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    entries
}
