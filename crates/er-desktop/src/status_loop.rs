//! The 30 s GitHub status loop's pick of the PR it refreshes.

/// The active tab's PR, as its card shows it. Read from the registry, the
/// tab's content and the PR cache, so a held App never skips a tick.
pub fn pick(state: &crate::state::DesktopState) -> Option<(String, String, u64)> {
    crate::resources::pr::active_status_key(state)
}
