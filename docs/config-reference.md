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

Optional runtime provider/model presets for the AI Hub. When present, AI Hub actions can switch between providers such as Claude, Codex, and Cursor without editing config mid-session. Selection is session-local; presets still come from TOML.

```toml
[ai_hub]
default_provider = "claude"
default_model = "sonnet-4.6"

[ai_hub.reviewer_models]
triage = "haiku-4.5"

[ai_hub.providers.claude]
label = "Claude"
command = "claude"
args = ["--print", "-p", "{prompt}"]

[[ai_hub.providers.claude.models]]
id = "sonnet-4.6"
label = "Sonnet 4.6"
args = ["--model", "claude-sonnet-4-6"]

[[ai_hub.providers.claude.models]]
id = "opus-4.6"
label = "Opus 4.6"
args = ["--model", "claude-opus-4-6"]

[[ai_hub.providers.claude.models]]
id = "opus-4.7"
label = "Opus 4.7"
args = ["--model", "claude-opus-4-7"]

[[ai_hub.providers.claude.models]]
id = "opus-4.8"
label = "Opus 4.8"
args = ["--model", "claude-opus-4-8"]

[[ai_hub.providers.claude.models]]
id = "haiku-4.5"
label = "Haiku 4.5"
args = ["--model", "claude-haiku-4-5-20251001"]

[ai_hub.providers.codex]
label = "Codex"
command = "codex"
args = ["exec", "--ignore-user-config", "--skip-git-repo-check", "--sandbox", "workspace-write", "{prompt}"]

[[ai_hub.providers.codex.models]]
id = "gpt-5.4"
label = "GPT-5.4"
args = ["--model", "gpt-5.4"]

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
- On load, `er` merges any missing built-in catalog models (e.g. new Opus releases) into your config in memory without rewriting your TOML file.
- The selected provider/model applies to AI Hub actions such as review, triage, experts, professor, questions, and summary.
- `[ai_hub.reviewer_models]` overrides the hub model for specific reviewer kinds. Triage uses `triage = "haiku-4.5"` by default in the example config; when unset, triage falls back to the fastest model in the active provider list.

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

## `.er/` AI artifacts

General review (AI Hub **Run review**) writes:

| File | Purpose |
|------|---------|
| `.er/review.json` | Per-file risk, summaries, findings |
| `.er/order.json` | Suggested review order |
| `.er/checklist.json` | Manual verification items |
| `.er/summary.md` | Overall summary |

**Specialized expert reviews** (AI Hub **Specialized review**) write findings only to `.er/experts/<id>.json` (e.g. `security`, `patterns`). They do not overwrite general artifacts.

At load time, `er` merges fresh expert sidecars (matching `diff_hash`) into the in-memory review so expert findings appear as **additional inline banners** labeled by expert (e.g. "Security finding"). Order/checklist/summary panels still require a general review run.

Expert ids (v1): `security`, `performance`, `reliability`, `testing`, `api`, `patterns`, `simplifying`, `mentorship`.

**Professor** (AI Hub **Professor**) writes teaching insights to `.er/professor.json` (merged inline at load; labeled with agent pill **Professor**). Not a code review — see `skills/PROFESSOR_PHILOSOPHY.md`.

**Multi-reviewer runs** (AI Hub **Run reviewers…** or **Review select files** → choose reviewers): spawn General + any experts + Professor concurrently. Each finding shows an agent pill (`General`, `Security`, …).

Shared prompt rules: `skills/REVIEW_RULES.md` (referenced by skills and engine spawn prompts).
