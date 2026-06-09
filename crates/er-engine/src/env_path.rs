//! Augment `PATH` so CLI tools installed outside `/usr/bin` are discoverable.
//!
//! macOS GUI apps launched from Finder get a minimal PATH that usually omits
//! Homebrew (`/opt/homebrew/bin`, `/usr/local/bin`) and user-local bins.

use std::path::{Path, PathBuf};

/// Prepend common CLI install locations to `PATH` when they are missing.
///
/// Call once near process startup (before any `git` / `gh` subprocess).
pub fn init_cli_path() {
    let current = std::env::var_os("PATH").unwrap_or_default();
    let current = Path::new(&current);
    let mut prefix: Vec<PathBuf> = Vec::new();

    for dir in ["/opt/homebrew/bin", "/usr/local/bin"] {
        let path = PathBuf::from(dir);
        if path.is_dir() && !path_is_on_path(current, &path) {
            prefix.push(path);
        }
    }

    if let Some(home) = dirs::home_dir() {
        for sub in [".local/bin", ".cargo/bin"] {
            let path = home.join(sub);
            if path.is_dir() && !path_is_on_path(current, &path) {
                prefix.push(path);
            }
        }
    }

    if prefix.is_empty() {
        return;
    }

    let mut merged = std::env::join_paths(prefix).expect("valid PATH prefix");
    if !current.as_os_str().is_empty() {
        merged.push(current);
    }
    std::env::set_var("PATH", merged);
}

fn path_is_on_path(path_env: &Path, candidate: &Path) -> bool {
    std::env::split_paths(path_env).any(|entry| entry == candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prepends_missing_homebrew_paths() {
        let original = std::env::var_os("PATH");
        std::env::set_var("PATH", "/usr/bin:/bin");
        init_cli_path();
        let updated = std::env::var("PATH").unwrap();
        if Path::new("/opt/homebrew/bin").is_dir() {
            assert!(updated.contains("/opt/homebrew/bin"));
        }
        if Path::new("/usr/local/bin").is_dir() {
            assert!(updated.contains("/usr/local/bin"));
        }
        match original {
            Some(value) => std::env::set_var("PATH", value),
            None => std::env::remove_var("PATH"),
        }
    }
}
