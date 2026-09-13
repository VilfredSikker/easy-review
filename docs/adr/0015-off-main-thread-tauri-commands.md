# Tauri commands that wait on the app lock run off the main thread

Synchronous Tauri commands execute on the main thread, so any command that takes the `App` mutex or shells out to git froze the window for as long as it ran. Every such command is therefore an `async fn` that wraps its blocking body in `run_blocking` (`tauri::async_runtime::spawn_blocking`, `crates/er-desktop/src/commands.rs`), and `AppState` is all `Arc`s that derive `Clone` so the body can be moved onto the worker thread. New commands that touch the lock or git follow the same pattern; a plain `fn` is the mistake this record exists to prevent.

## Consequences

- The failure is near-invisible in development: a main-thread command compiles, passes its tests, and feels fine on a small repo. It only shows up as a frozen window once a real diff or a slow `gh` call makes the block long enough to notice.
