# The engine and TUI are synchronous; tokio lives only in the front ends that need it

`er-engine` has no async items and no runtime handle, and `er-tui` drives everything from a blocking loop — crossterm `poll` every 50ms plus `std::sync::mpsc` channels for file-watch events. Only `er-desktop` (Tauri commands) and `er-mcp` (MCP stdio server) depend on tokio, and each declares it for itself.

The trade-off was deliberate: the core's work is shelling out to git, parsing, and rendering, none of which an async runtime helps with, so tokio in the engine would be weight every consumer pays for and nobody uses. The front ends that genuinely need concurrency declare it at their own layer rather than imposing it on the core.

## Consequences

- Heavy Tauri commands are `async fn`s that wrap a blocking body in `run_blocking` (`tauri::async_runtime::spawn_blocking`), so the front end's concurrency and the engine's blocking calls meet at that boundary and nowhere else.
- Neither the TUI nor a headless consumer can `await` an engine call; a change that wants `await` inside `er-engine` is a signal the work belongs in a front end instead.
