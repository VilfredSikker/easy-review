# Configuration is global-only, and lives in managed storage

There is no per-repo config file. `ErConfig` resolves to `$ER_CONFIG_PATH`, else `<storage_root>/config.toml` — `storage_root()` being `$ER_STORAGE_ROOT` as given, or `dirs::data_dir()/easy-review/v1`, which puts it at `~/Library/Application Support/easy-review/v1/config.toml` on macOS. Agent and provider settings (`[agent]`, `[ai_hub]`) live in `agents.toml` beside it; everything else stays in `config.toml`. Settings apply live to the running app and auto-persist to that file, so there is no "unsaved settings" state to lose. An older `~/Library/Application Support/easy-review/config.toml` is not read (ADR 0003). A repo-local `.er-config.toml` was removed outright because it permanently shadowed global theme saves — changing the theme appeared to do nothing — and a stale-reload path separately clobbered unsaved desktop settings. Neither failure is fixable while a per-repo layer sits underneath the global one.

## Consequences

- `~/.config/er/config.toml` (`$XDG_CONFIG_HOME/er/config.toml`) is not read or copied (1.0.0); managed storage is the only source.
- `load_global_config()` is the only entry point, and `save_config()` the only writer. Reading it for a fresh value is fine; assigning the result back over `app.config` mid-session is what clobbered unsaved settings.
- Anything that varies per repo or branch belongs in managed storage keyed by repo and branch (see `storage.rs`), not in a file next to the code.
