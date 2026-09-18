# Tauri commands that wait on the app lock run off the main thread

Synchronous Tauri commands execute on the main thread, so a command that takes the `App` mutex or shells out to git freezes the window for as long as it runs. The rule is that such a command is an `async fn` wrapping its blocking body in `run_blocking` (`tauri::async_runtime::spawn_blocking`, `crates/er-desktop/src/commands.rs`), with `AppState` all `Arc`s that derive `Clone` so the body can move onto the worker thread.

**The rule is not yet universal, and the exceptions are not all benign.** The navigation commands (`next_file`, `prev_file`, `next_hunk`, `prev_hunk`, `toggle_compacted`, `jump_to_unreviewed`) are plain `fn`s that lock the app and build a snapshot inline; `toggle_compacted` also reaches git for a compacted file. More seriously, `submit_github_review` is still a sync `fn` that locks three times and calls `refetch_and_refresh_diff` — a git shell-out under the lock, which is the exact freeze this record exists to prevent. Treat the pattern as the direction rather than as a description of the tree.

## Consequences

- The failure is near-invisible in development: a main-thread command compiles, passes its tests, and feels fine on a small repo. It only shows up as a frozen window once a real diff or a slow `gh` call makes the block long enough to notice.
- A new command is not safe because the ones beside it look the same. Check whether the body takes the lock or shells out before writing it as a plain `fn` — the neighbouring command may be one of the exceptions above.
