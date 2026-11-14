//! The Workspace actor: commands in, events out, one actor per repo.
//!
//! The service layer the flag path talks to. The actor loop, its protocol and
//! the use cases land in the entries that follow; this crate exists so the
//! build flag, its clippy pass and its headless build have the crate to
//! compile.
//!
//! The engine dependency is deliberately headless: the service may use the
//! always-compiled half (storage, the store registry, config, `sync.rs`) and
//! must not reach the `ui`-gated `app` or `arena` modules, which is what the
//! standalone build checks.

/// One repository's workspace: the actor's state.
pub struct Workspace;

/// The handle a front end holds for one workspace.
#[derive(Clone)]
pub struct WorkspaceHandle;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_handle_is_cloneable_and_thread_safe() {
        fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
        assert_send_sync_clone::<WorkspaceHandle>();
    }
}
