//! Desktop dev log filter: `ER_LOG`, `--logs`, and `log` crate target routing.

use er_engine::dev_log as engine;

pub use engine::{
    arena_line, enabled, shows_all, GROUP_APP, GROUP_ARENA, GROUP_ERP, GROUP_PROFILE,
};

/// Call once at process start (before `tauri::Builder`).
pub fn init() {
    let mut args: Vec<String> = std::env::args().collect();
    let groups = engine::parse_env_and_args(&mut args);
    engine::init_filter(groups);
    if !shows_all() {
        eprintln!(
            "er-desktop: dev log filter active (ER_LOG={})",
            std::env::var("ER_LOG").unwrap_or_default()
        );
    }
}

/// Active filter for the webview (`None` = all groups).
pub fn filter_groups() -> Option<Vec<String>> {
    if shows_all() {
        return None;
    }
    std::env::var("ER_LOG")
        .ok()
        .map(|s| {
            s.split(',')
                .map(|p| p.trim().to_ascii_lowercase())
                .filter(|p| !p.is_empty())
                .collect()
        })
        .filter(|v: &Vec<String>| !v.is_empty())
}

/// Whether a `log` crate target should print under the current filter.
pub fn enabled_for_log_target(target: &str) -> bool {
    enabled(log_target_group(target))
}

fn log_target_group(target: &str) -> &'static str {
    if target.starts_with("er.arena") {
        return GROUP_ARENA;
    }
    if target.starts_with("er.profile") || target.contains("profile_log") {
        return GROUP_PROFILE;
    }
    if target.starts_with("er.erp") || target.contains("browser_proxy") {
        return GROUP_ERP;
    }
    GROUP_APP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_target_maps_to_the_profile_group() {
        // `profile_log` emits with this target so the records reach the LogDir
        // file, and the logger's filter is what decides whether they survive.
        // If this mapping drifts, every profile line is silently dropped and the
        // log file stays empty — the failure is invisible by construction.
        assert_eq!(log_target_group("er.profile"), GROUP_PROFILE);
        // The stderr-era spelling, kept working by the `contains` arm.
        assert_eq!(log_target_group("er_desktop::profile_log"), GROUP_PROFILE);
    }

    #[test]
    fn other_targets_still_fall_through_to_their_groups() {
        assert_eq!(log_target_group("er.arena"), GROUP_ARENA);
        assert_eq!(log_target_group("er.erp"), GROUP_ERP);
        assert_eq!(log_target_group("er_desktop::commands"), GROUP_APP);
    }
}
