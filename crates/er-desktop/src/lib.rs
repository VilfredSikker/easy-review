pub mod ai_ask;
/// The desktop's side of the app owner, which lives in er-service: the
/// startup steps that read `DesktopState`, and the tests of that wiring.
pub mod app_owner {
    pub mod startup;
    #[cfg(test)]
    mod tests {
        mod wiring;
    }
}
pub mod arena_service;
pub mod ask_context;
pub mod auto_triage;
pub mod browser_proxy;
pub mod browser_webview;
pub mod comment_tick;
pub mod dev_log;
pub mod direct;
pub mod export;
pub mod flow_service;
pub mod frame_script;
pub mod gh_pr_actions;
pub mod github_image;
pub mod line_commits_cache;
pub mod main_webview_policy;
pub mod native_notify;
pub mod os_launch;
pub mod page_view;
pub mod profile_log;
pub mod resources;
pub mod revision_watcher;
pub mod route_helpers;
pub mod runs;
pub mod service;
pub mod state;
pub mod status_loop;
pub mod store_registry;
pub mod tabs;
pub mod task_hooks;
pub mod terminal;
pub mod view_state;
pub mod watcher;
pub mod window_placement;

#[cfg(test)]
mod tests {
    /// er-desktop builds one path. A `v1-service` feature in its manifest
    /// would bring back a build without the service, which the page cannot
    /// read; the flag stays on er-tui alone.
    #[test]
    fn the_manifest_declares_no_v1_service_feature() {
        let manifest = include_str!("../Cargo.toml");
        assert!(
            !manifest.contains("v1-service"),
            "er-desktop's Cargo.toml names v1-service"
        );
    }
}
