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

    std::env::set_var("PATH", merge_paths(prefix, current));
}

/// Join `prefix` and the entries of `current` into one PATH value, keeping the
/// platform separator between the two halves. (A bare `OsString::push` of the
/// old PATH fused the last prefix dir with the first existing entry — on a
/// Finder/Spotlight launch that destroyed `/usr/bin`, making `git`
/// unresolvable and aborting desktop startup.)
#[expect(
    clippy::expect_used,
    reason = "entries are fixed dirs, $HOME subdirs, or came from split_paths; only a $HOME holding the separator fails, and startup should stop then"
)]
fn merge_paths(mut prefix: Vec<PathBuf>, current: &Path) -> std::ffi::OsString {
    if !current.as_os_str().is_empty() {
        prefix.extend(std::env::split_paths(current));
    }
    std::env::join_paths(prefix).expect("valid PATH prefix")
}

fn path_is_on_path(path_env: &Path, candidate: &Path) -> bool {
    std::env::split_paths(path_env).any(|entry| entry == candidate)
}

/// Whether `command` names an executable file, using the process PATH.
pub fn command_on_path(command: &str) -> bool {
    let path = std::env::var_os("PATH").unwrap_or_default();
    command_on_path_in(command, &path)
}

/// Pure PATH lookup used by tests and callers that already captured an
/// environment snapshot.
pub fn command_on_path_in(command: &str, path_env: &std::ffi::OsStr) -> bool {
    use std::os::unix::fs::PermissionsExt;

    if command.is_empty() {
        return false;
    }
    let command_path = Path::new(command);
    let candidates = if command.contains(std::path::MAIN_SEPARATOR) {
        vec![command_path.to_path_buf()]
    } else {
        std::env::split_paths(path_env)
            .map(|dir| dir.join(command_path))
            .collect()
    };
    candidates.into_iter().any(|candidate| {
        candidate
            .metadata()
            .is_ok_and(|metadata| metadata.is_file() && metadata.permissions().mode() & 0o111 != 0)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_separator_between_prefix_and_existing_path() {
        let merged = merge_paths(
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/Users/x/.cargo/bin"),
            ],
            Path::new("/usr/bin:/bin"),
        );
        let entries: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(
            entries,
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/Users/x/.cargo/bin"),
                PathBuf::from("/usr/bin"),
                PathBuf::from("/bin"),
            ]
        );
    }

    #[test]
    fn merge_with_empty_current_is_just_the_prefix() {
        let merged = merge_paths(vec![PathBuf::from("/opt/homebrew/bin")], Path::new(""));
        let entries: Vec<PathBuf> = std::env::split_paths(&merged).collect();
        assert_eq!(entries, vec![PathBuf::from("/opt/homebrew/bin")]);
    }

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

    #[cfg(unix)]
    #[test]
    fn command_lookup_uses_injected_path_without_mutating_process_path() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::Builder::new()
            .prefix("er-path-")
            .tempdir_in(".")
            .unwrap();
        let executable = dir.path().join("runner");
        std::fs::write(&executable, "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o755)).unwrap();
        let plain = dir.path().join("plain");
        std::fs::write(&plain, "text").unwrap();
        std::fs::set_permissions(&plain, std::fs::Permissions::from_mode(0o644)).unwrap();
        let path = std::env::join_paths([dir.path()]).unwrap();
        let relative = executable
            .strip_prefix(std::env::current_dir().unwrap())
            .unwrap();

        assert!(command_on_path_in("runner", &path));
        assert!(command_on_path_in(
            executable.to_str().unwrap(),
            std::ffi::OsStr::new("")
        ));
        assert!(command_on_path_in(
            relative.to_str().unwrap(),
            std::ffi::OsStr::new("")
        ));
        assert!(!command_on_path_in("missing", &path));
        assert!(!command_on_path_in(
            plain.to_str().unwrap(),
            std::ffi::OsStr::new("")
        ));
        assert!(!command_on_path_in(
            dir.path().to_str().unwrap(),
            std::ffi::OsStr::new("")
        ));
    }
}
