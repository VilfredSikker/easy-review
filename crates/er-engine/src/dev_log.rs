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

}
