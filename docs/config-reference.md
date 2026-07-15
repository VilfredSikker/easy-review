# Configuration Reference

`er` loads configuration from TOML files. Settings are deep-merged: global provides shared defaults, per-repo overrides individual fields.

## Config Files

| Location | Purpose |
|----------|---------|
| `~/.config/er/config.toml` | Global defaults (shared across all repos) |
| `.er-config.toml` (repo root) | Per-repo overrides |

**Priority:** per-repo > global > built-in defaults.

Per-repo only needs to specify fields that differ from your global config. Unspecified fields inherit from global, then from built-in defaults.

## Live Editing

Press `,` inside `er` to open the settings hub. Changes apply immediately and can be persisted to `~/.config/er/config.toml` from inside the hub.

## All Options

### `[features]`

Feature toggles. All default to `true`.

```toml
[features]
view_branch = true         # Enable branch diff mode
view_unstaged = true       # Enable unstaged diff mode
view_staged = true         # Enable staged diff mode
view_history = true        # Enable commit history mode
view_conflicts = true      # Enable merge conflicts mode (tab appears during a merge)
view_hidden = true         # Enable hidden/watched files mode (tab appears when [watched] paths exist)
view_tour = true           # Enable AI guided tour mode (tab appears when a tour.json exists)
arena = true               # Enable the multi-reviewer arena (desktop)
```

### `[display]`

Rendering options.

```toml
[display]
theme = "graphite"   # graphite | slate | midnight | ember | paper | daylight | contrast-dark | contrast-light
tab_width = 4        # Spaces per tab character (1-16)
line_numbers = true  # Show line numbers in diff view
wrap_lines = false   # Wrap long lines instead of horizontal scroll
split_diff = false   # Side-by-side diff view
```

### `[agent]`

AI agent command configuration. Used when triggering AI review from within `er`.

```toml
[agent]
command = "claude"                      # Binary to invoke
args = ["--print", "-p", "{prompt}"]    # Arguments ({prompt} is replaced)
```

### `[ai_hub]`

Optional runtime provider/model presets for the AI Hub. When present, AI Hub actions can switch between providers such as Claude, Codex, and Cursor. Desktop Settings persists the selected default immediately; the AI action palette keeps mid-session picks session-only. The TUI persists defaults when you save General settings.

```toml
[ai_hub]
default_provider = "claude"
default_model = "sonnet-5"
# Optional; omit or use Auto in the UI for the provider default.
# default_effort = "high"

[ai_hub.providers.claude]
label = "Claude"
command = "claude"
args = ["--print", "-p", "{prompt}"]

[[ai_hub.providers.claude.models]]
id = "fable-5"
label = "Fable 5"
args = ["--model", "claude-fable-5"]
effort_levels = ["low", "medium", "high", "xhigh", "max"]

[[ai_hub.providers.claude.models]]
id = "sonnet-5"
label = "Sonnet 5"
args = ["--model", "claude-sonnet-5"]
effort_levels = ["low", "medium", "high", "xhigh", "max"]

[[ai_hub.providers.claude.models]]
id = "opus-4.8"
label = "Opus 4.8"
args = ["--model", "claude-opus-4-8"]
effort_levels = ["low", "medium", "high", "xhigh", "max"]

[[ai_hub.providers.claude.models]]
id = "haiku-4.5"
label = "Haiku 4.5"
args = ["--model", "claude-haiku-4-5-20251001"]
effort_levels = []

[ai_hub.providers.codex]
label = "Codex"
command = "codex"
args = ["exec", "--ignore-user-config", "--skip-git-repo-check", "--sandbox", "workspace-write", "{prompt}"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.4"
label = "GPT-5.4"
args = ["--model", "gpt-5.4"]
effort_levels = ["low", "medium", "high", "xhigh"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.5"
label = "GPT-5.5"
args = ["--model", "gpt-5.5"]
effort_levels = ["low", "medium", "high", "xhigh"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.6-sol"
label = "GPT-5.6 Sol"
args = ["--model", "gpt-5.6-sol"]
effort_levels = ["low", "medium", "high", "xhigh", "max"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.6-terra"
label = "GPT-5.6 Terra"
args = ["--model", "gpt-5.6-terra"]
effort_levels = ["low", "medium", "high", "xhigh", "max"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.6-luna"
label = "GPT-5.6 Luna"
args = ["--model", "gpt-5.6-luna"]
effort_levels = ["low", "medium", "high", "xhigh", "max"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.4-mini"
label = "GPT-5.4 Mini"
args = ["--model", "gpt-5.4-mini"]
effort_levels = ["low", "medium", "high", "xhigh"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.3-codex-spark"
label = "GPT-5.3 Codex Spark"
args = ["--model", "gpt-5.3-codex-spark"]
effort_levels = ["low", "medium", "high", "xhigh"]

[ai_hub.providers.cursor]
label = "Cursor"
command = "agent"
args = ["--print", "--trust", "--force", "--output-format", "stream-json", "-p", "{prompt}"]

[[ai_hub.providers.cursor.models]]
id = "composer-2.5"
label = "Composer 2.5"
args = ["--model", "composer-2.5"]
```

Rules:
- Provider `args` are the shared base arguments for that CLI.
- Model `args` are appended after provider args.
- If `[ai_hub]` is absent, `er` falls back to the single `[agent]` configuration.
- On load, `er` merges missing current built-in catalog models into your config in memory without rewriting your TOML file; unknown legacy reviewer-model entries are ignored and disappear the next time the config is saved.
- The selected default provider/model is used by every ordinary AI Hub action, including review, triage, tours, experts, Professor, validation, questions, summary, and card AI. Triage still forces low effort. An explicit model selected for a single review run overrides it only for that run.
- Each model's `effort_levels` metadata is authoritative. `Auto` (the default) omits the override; Claude receives `--effort <level>` and Codex receives `-c model_reasoning_effort=<level>` only for supported levels.
- Built-in Claude, Codex, and Cursor Agent launches that write review sidecars receive the active review bucket (`er_dir`) as an additional directory via `--add-dir` — not the global storage root. Codex treats that path as writable under `workspace-write`. Custom provider commands are not given unknown CLI flags.

### `[watched]`

Monitor git-ignored files (e.g., agent work directories, sidecar files). Watched files appear in the file tree alongside tracked changes.

```toml
[watched]
paths = [".work/**/*", ".er/**/*"]  # Glob patterns for files to watch
diff_mode = "content"               # "content" (show file) or "snapshot" (diff against saved baseline)
```

**Note:** Watched files should be in `.gitignore`. `er` warns if they aren't.

## Example Configs

### Global (`~/.config/er/config.toml`)

Shared preferences across all repos:

```toml
[features]
view_conflicts = false

[display]
tab_width = 2
wrap_lines = true

[watched]
paths = [".work/**/*", ".er/**/*"]
```

### Per-repo (`.er-config.toml`)

Only override what's different for this repo:

```toml
[display]
tab_width = 4

[watched]
paths = [".work/**/*", ".er/**/*", "logs/**/*.log"]
diff_mode = "snapshot"
```

Result: `view_conflicts = false` and `wrap_lines = true` from global, `tab_width = 4` and extra watched paths from local.

## Review sidecar files (managed storage)

TUI and Desktop share the same sidecar directory per repo/branch/view bucket under managed app data (default: `~/.local/share/easy-review/repos/<repo>/branches/<branch>/view-buckets/<bucket>/`). Set `ER_REPO_LOCAL=1` to use repo-local `.er/` instead (debug only).

General review (AI Hub **Run review**) writes:

| File | Purpose |
|------|---------|
| `review.json` | Per-file risk, summaries, findings |
| `order.json` | Suggested review order |
| `checklist.json` | Manual verification items |
| `summary.md` | Overall summary |

**Specialized expert reviews** (AI Hub **Specialized review**) write findings only to `experts/<id>.json` (e.g. `security`, `patterns`). They do not overwrite general artifacts.

At load time, `er` merges fresh expert sidecars (matching `diff_hash`) into the in-memory review so expert findings appear as **additional inline banners** labeled by expert (e.g. "Security finding"). Order/checklist/summary panels still require a general review run.

Expert ids (v1): `security`, `performance`, `reliability`, `testing`, `api`, `patterns`, `simplifying`, `mentorship`.

**Professor** (AI Hub **Professor**) writes teaching insights to `professor.json` (merged inline at load; labeled with agent pill **Professor**). Not a code review — see `skills/PROFESSOR_PHILOSOPHY.md`.

**Multi-reviewer runs** (AI Hub **Run reviewers…** or **Review select files** → choose reviewers): spawn General + any experts + Professor concurrently. Each finding shows an agent pill (`General`, `Security`, …).

Shared prompt rules: `skills/REVIEW_RULES.md` (referenced by skills and engine spawn prompts).
