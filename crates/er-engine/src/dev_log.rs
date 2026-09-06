//! Dev-only log groups for desktop / arena diagnostics.
//!
//! Filter via `ER_LOG` (comma-separated) or `--logs arena` on the binary.
//! Empty / `all` / `*` → show every group.

use std::sync::OnceLock;

static FILTER: OnceLock<Option<Vec<String>>> = OnceLock::new();

/// Known groups (document in `crates/er-desktop/agent.md`).
pub const GROUP_ARENA: &str = "arena";
pub const GROUP_PROFILE: &str = "profile";
pub const GROUP_ERP: &str = "erp";
pub const GROUP_APP: &str = "app";

/// Install the active filter (`None` = show all groups).
pub fn init_filter(groups: Option<Vec<String>>) {
    let _ = FILTER.set(normalize_groups(groups));
}

/// `true` when no filter is active (default dev: everything).
pub fn shows_all() -> bool {
    matches!(FILTER.get(), None | Some(None))
}

/// Whether a log group should be emitted.
pub fn enabled(group: &str) -> bool {
    match FILTER.get() {
        None | Some(None) => true,
        Some(Some(groups)) => groups.iter().any(|g| g == group),
    }
}

/// Arena diagnostics (`[er-arena]` on stderr).
pub fn arena_line(message: impl AsRef<str>) {
    if enabled(GROUP_ARENA) {
        eprintln!("[er-arena] {}", message.as_ref());
    }
}

fn normalize_groups(groups: Option<Vec<String>>) -> Option<Vec<String>> {
    let list = groups?;
    if list.is_empty() {
        return None;
    }
    let expanded: Vec<String> = list
        .into_iter()
        .flat_map(|g| {
            g.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .collect();
    if expanded.is_empty() || expanded.iter().any(|g| g == "all" || g == "*") {
        return None;
    }
    Some(expanded)
}

/// Parse `ER_LOG` and strip `--logs` / `--logs=…` from `args` (mutated).
pub fn parse_env_and_args(args: &mut Vec<String>) -> Option<Vec<String>> {
    let mut from_args: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].clone();
        if arg == "--logs" {
            from_args = args.get(i + 1).cloned();
            args.remove(i);
            if i < args.len() {
                args.remove(i);
            }
            continue;
        }
        if let Some(rest) = arg.strip_prefix("--logs=") {
            from_args = Some(rest.to_string());
            args.remove(i);
            continue;
        }
        i += 1;
    }

    let from_env = std::env::var("ER_LOG").ok();
    let raw = from_args.or(from_env);
    let groups = raw.map(|s| {
        s.split(',')
            .map(|p| p.trim().to_string())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
    });
    if let Some(ref g) = groups {
        if !g.is_empty() {
            let joined = g.join(",");
            std::env::set_var("ER_LOG", &joined);
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_all_wildcards() {
        assert!(normalize_groups(Some(vec!["*".into()])).is_none());
        assert!(normalize_groups(Some(vec!["all".into()])).is_none());
    }

    #[test]
    fn normalize_keeps_groups() {
        assert_eq!(
            normalize_groups(Some(vec!["arena".into()])),
            Some(vec!["arena".into()])
        );
    }

    /// `parse_env_and_args` both reads and writes the process-global `ER_LOG`,
    /// so its tests must not run beside other env-mutating tests. Clears any
    /// leaked value up front; every test clears again before dropping the guard.
    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        let guard = crate::storage::STORAGE_TEST_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("ER_LOG");
        guard
    }

    #[test]
    fn logs_flag_pair_is_stripped_and_overrides_the_env() {
        let _guard = env_guard();
        std::env::set_var("ER_LOG", "app");
        let mut args = vec![
            "er-desktop".to_string(),
            "--logs".to_string(),
            "arena,profile".to_string(),
            "--pr".to_string(),
            "7".to_string(),
        ];
        let groups = parse_env_and_args(&mut args);
        assert_eq!(
            groups,
            Some(vec!["arena".to_string(), "profile".to_string()])
        );
        assert_eq!(
            args,
            vec!["er-desktop", "--pr", "7"],
            "both flag tokens are consumed, the rest of the argv is preserved"
        );
        // The chosen groups are published back so spawned children inherit them.
        assert_eq!(std::env::var("ER_LOG").unwrap(), "arena,profile");
        std::env::remove_var("ER_LOG");
    }

    #[test]
    fn inline_logs_flag_trims_whitespace_and_blank_groups() {
        let _guard = env_guard();
        let mut args = vec!["--logs=  arena , , erp ".to_string(), "--pr".to_string()];
        let groups = parse_env_and_args(&mut args);
        assert_eq!(groups, Some(vec!["arena".to_string(), "erp".to_string()]));
        assert_eq!(args, vec!["--pr"], "only the inline flag itself is removed");
        std::env::remove_var("ER_LOG");
    }

    #[test]
    fn dangling_logs_flag_is_dropped_and_the_env_is_used_instead() {
        let _guard = env_guard();
        std::env::set_var("ER_LOG", " erp , app ");
        let mut args = vec!["--logs".to_string()];
        let groups = parse_env_and_args(&mut args);
        assert_eq!(groups, Some(vec!["erp".to_string(), "app".to_string()]));
        assert!(
            args.is_empty(),
            "a value-less --logs is still stripped from argv"
        );
        std::env::remove_var("ER_LOG");
    }

    #[test]
    fn without_flag_or_env_argv_is_untouched_and_no_filter_is_published() {
        let _guard = env_guard();
        let mut args = vec!["--pr".to_string(), "7".to_string()];
        assert_eq!(parse_env_and_args(&mut args), None);
        assert_eq!(args, vec!["--pr", "7"]);
        assert!(
            std::env::var("ER_LOG").is_err(),
            "no groups means nothing is written to the env"
        );
    }
}
