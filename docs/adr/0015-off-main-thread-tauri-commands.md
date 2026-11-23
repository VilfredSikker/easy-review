# Tauri commands that block run off the main thread

Synchronous Tauri commands execute on the main thread, so a command that waits on a lock, an actor's reply or a git subprocess freezes the window for as long as it runs. The rule is that such a command is an `async fn` that moves its blocking body onto `tauri::async_runtime::spawn_blocking`, with `DesktopState` all `Arc`s that derive `Clone` so the body can move onto the worker thread. The three resource commands (ADR 0072) all do: a send can wait on an actor and its save, and the first `settings` fetch starts the settings actor, which loads the config.

A body that blocks on a `WorkspaceHandle` reply must bound the wait with `send_and_wait` (`REPLY_TIMEOUT`) or its own `recv_timeout` — see ADR 0067.

## Consequences

- The failure is near-invisible in development: a main-thread command compiles, passes its tests, and feels fine on a small repo. It only shows up as a frozen window once a real diff or a slow `gh` call makes the block long enough to notice.
- A new command is not safe because the ones beside it look the same. Check whether the body takes the lock or shells out before writing it as a plain `fn` — a quick plain `fn` beside it is no proof.
