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
    command_on_path_for_platform(command, path_env, cfg!(windows))
}

fn command_on_path_for_platform(command: &str, path_env: &std::ffi::OsStr, windows: bool) -> bool {
    command_candidates(command, path_env, windows)
        .into_iter()
        .any(|candidate| {
            candidate.metadata().is_ok_and(|metadata| {
                metadata.is_file() && (windows || has_execute_permission(&metadata))
            })
        })
}

fn has_execute_permission(metadata: &std::fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        let _ = metadata;
        true
    }
}

fn command_candidates(command: &str, path_env: &std::ffi::OsStr, windows: bool) -> Vec<PathBuf> {
    if command.is_empty() || command.ends_with('/') || (windows && command.ends_with('\\')) {
        return Vec::new();
    }
    let explicit_path = command.contains('/')
        || (windows && (command.contains('\\') || command.as_bytes().get(1) == Some(&b':')));
    let command_path = PathBuf::from(command);
    if explicit_path {
        // Rust tries the appended .exe first for explicit Windows paths,
        // then falls back to the exact path, including explicit .cmd/.bat.
        if windows && !command.to_ascii_lowercase().ends_with(".exe") {
            let mut exe = command_path.as_os_str().to_os_string();
            exe.push(".exe");
            return vec![PathBuf::from(exe), command_path];
        }
        return vec![command_path];
    }

    // Command adds only .exe during Windows PATH lookup. It does not expand
    // PATHEXT to discover .cmd/.bat shims for extensionless commands.
    let name = if windows && !command.contains('.') {
        PathBuf::from(format!("{command}.exe"))
    } else {
        command_path
    };
    std::env::split_paths(path_env)
        .filter(|dir| !windows || !dir.as_os_str().is_empty())
        .map(|dir| dir.join(&name))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_keeps_separator_between_prefix_and_existing_path() {
        let current = std::env::join_paths(["/usr/bin", "/bin"]).unwrap();
        let merged = merge_paths(
            vec![
                PathBuf::from("/opt/homebrew/bin"),
                PathBuf::from("/Users/x/.cargo/bin"),
            ],
            Path::new(&current),
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
        assert!(!command_on_path_in("", &path));
        assert!(!command_on_path_in("plain", &path));
        assert!(!command_on_path_in(
            plain.to_str().unwrap(),
            std::ffi::OsStr::new("")
        ));
        assert!(!command_on_path_in(
            dir.path().to_str().unwrap(),
            std::ffi::OsStr::new("")
        ));
    }

    #[test]
    fn windows_lookup_finds_exe_without_unix_execute_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let executable = dir.path().join("runner.exe");
        std::fs::write(&executable, "test executable").unwrap();
        std::fs::create_dir(dir.path().join("directory.exe")).unwrap();
        for name in ["shim.cmd", "batch.bat", "extensionless"] {
            std::fs::write(dir.path().join(name), "test command").unwrap();
        }
        let path = std::env::join_paths([dir.path()]).unwrap();
        let lookup = |command: &str| command_on_path_for_platform(command, &path, true);

        assert!(lookup("runner"));
        assert!(lookup("runner.exe"));
        assert!(lookup(executable.to_str().unwrap()));
        assert!(lookup(dir.path().join("runner").to_str().unwrap()));
        assert!(!lookup("missing"));
        assert!(!lookup("missing.exe"));
        assert!(!lookup("directory"));
        assert!(!lookup("directory.exe"));
        assert!(!lookup(dir.path().to_str().unwrap()));
        assert!(!lookup(""));
        assert!(!lookup("shim"));
        assert!(!lookup("batch"));
        assert!(!lookup("extensionless"));
        assert!(lookup(dir.path().join("extensionless").to_str().unwrap()));
        // Explicit batch extensions are supported by Rust's Windows launcher.
        assert!(lookup("shim.cmd"));
        assert!(lookup("batch.bat"));

        #[cfg(windows)]
        {
            assert!(command_on_path_in("runner", &path));
            assert!(command_on_path_in("runner.exe", &path));
            assert!(command_on_path_in(
                &executable.to_str().unwrap().replace('\\', "/"),
                &path
            ));
            assert!(!command_on_path_in("missing", &path));
            assert!(!command_on_path_in("directory", &path));
        }
    }

    #[test]
    fn windows_candidates_keep_explicit_paths_out_of_path_search() {
        let path = std::env::join_paths(["unrelated"]).unwrap();
        for command in [
            r"tools\runner",
            "tools/runner",
            r"C:\tools\runner",
            "C:runner",
        ] {
            assert_eq!(
                command_candidates(command, &path, true),
                vec![
                    PathBuf::from(format!("{command}.exe")),
                    PathBuf::from(command)
                ]
            );
        }
        for command in [r"tools\runner.exe", "tools/runner.EXE"] {
            assert_eq!(
                command_candidates(command, &path, true),
                vec![PathBuf::from(command)]
            );
        }
        assert!(command_candidates(r"tools\", &path, true).is_empty());
        assert!(command_candidates("tools/", &path, true).is_empty());
        assert_eq!(
            command_candidates("runner", &path, true),
            vec![PathBuf::from("unrelated").join("runner.exe")]
        );
        assert_eq!(
            command_candidates("runner.cmd", &path, true),
            vec![PathBuf::from("unrelated").join("runner.cmd")]
        );
    }
}
