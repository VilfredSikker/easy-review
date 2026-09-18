# Configuration Reference

`ErConfig` is global-only: one `config.toml`, shared by every repo. Settings apply live and auto-persist, so there is no unsaved-settings state to lose. A repo-local `.er-config.toml` was removed outright and must not come back — it permanently shadowed global theme saves. See `docs/adr/0005-global-only-config.md`.

## Where the file lives

| Location | Path |
|---|---|
| Live config | `<storage_root>/config.toml` |
| macOS | `~/Library/Application Support/easy-review/config.toml` |
| Legacy, migration source only | `~/.config/er/config.toml` (`$XDG_CONFIG_HOME/er/config.toml`) |

`storage_root` is `$ER_STORAGE_ROOT` or `dirs::data_dir()/easy-review` — the same root that holds review sidecars (`crates/er-engine/src/storage.rs`).

| Variable | Effect |
|---|---|
| `ER_STORAGE_ROOT` | Replaces the storage root. Config and review sidecars both move with it; tests use it to write under a temp dir. |
| `ER_CONFIG_PATH` | Replaces the config path outright — a full file path, not a directory. |
| `ER_REPO_LOCAL=1` | Review sidecars go to `<repo>/.er/` instead of managed storage. Debug only; the config path is unaffected. |

**Legacy migration.** On load, when the managed file is missing, the legacy file is copied into it once. Copy-only — the legacy file is never moved or deleted, so leave it on disk: it is read directly for that session if the copy fails.

**A config that will not parse** is copied to `config.toml.invalid` beside it, and built-in defaults are used.

## Live editing

- **TUI:** `,` opens the config hub (General and Terminal tabs). An edit applies immediately and persists on the spot; a failed save reverts the config to the last good snapshot and notifies, so `Esc` is always a clean close.
- **Desktop:** the Settings panel edits the same struct and persists through the same `save_config`. The desktop diff view keeps its own split and wrap state in `localStorage` (`er.diffViewMode`, `er.wrapLines`), and no frontend code reads the snapshot's `display` block — so `display.split_diff` and `display.wrap_lines` move the TUI only.

## All options

### `[features]`

Mode toggles. All default to `true`.

| Key | Effect |
|---|---|
| `view_branch` | Branch diff mode |
| `view_unstaged` | Unstaged diff mode |
| `view_staged` | Staged diff mode |
| `view_history` | Commit history mode |
| `view_conflicts` | Merge-conflict mode (tab appears only while a merge is active) |
| `view_hidden` | Hidden/watched files mode (tab appears when `[watched]` paths exist) |
| `view_tour` | Guided tour mode (tab appears when a tour exists for the branch — it may be the other view bucket's, reused on a matching diff hash) |
| `arena` | Multi-round review arena (desktop) |
| `model_discovery` | Query provider `models_command` CLIs and merge the discovered models into the pickers |

History, Conflicts and Hidden are also dropped in a read-only PR view, whatever these flags say.

### `[display]`

TUI rendering, plus `theme`, which both front ends share (`docs/adr/0030-shared-theme-tokens.md`).

| Key | Default | Meaning |
|---|---|---|
| `theme` | `"graphite"` | `graphite`, `slate`, `midnight`, `ember`, `paper`, `daylight`, `contrast-dark`, `contrast-light`. Retired names still resolve through aliases. |
| `tab_width` | `4` | Spaces per tab (1–16) |
| `line_numbers` | `true` | Line numbers in the diff view |
| `wrap_lines` | `false` | Wrap long lines instead of scrolling horizontally |
| `split_diff` | `false` | Side-by-side diff |
| `auto_context_threshold` | `100` | Auto-pick unified context per file by size tier; `0` disables |

### `[hints]`

Bottom-bar key hint groups: `navigation`, `comments`, `staging` (default `true`); `verbose` (default `false`).

### `[watched]`

Git-ignored paths to show alongside tracked changes.

| Key | Default | Meaning |
|---|---|---|
| `paths` | `[]` | Glob patterns, e.g. `[".work/**/*", ".er/**/*"]` |
| `diff_mode` | `"content"` | `content` shows the file; `snapshot` diffs it against a saved baseline |

Watched files should be in `.gitignore`; `er` warns when they are not.

### `[agent]`

The fallback used whenever `[ai_hub]` has no providers.

| Key | Default |
|---|---|
| `command` | `"claude"` |
| `args` | `["--print", "--output-format", "stream-json", "-p", "{prompt}"]` |
| `model` | empty |
| `effort` | unset — Claude `--effort`, legacy path only |

`{prompt}` is how the prompt reaches the agent. An `args` override that omits it drops the prompt silently; nothing validates that.

### `[summary]`

Diff summary / changelog generation: `command` (defaults to `agent.command`), `args` (`{diff}` is replaced with the raw diff), `push_to_pr` (default `false` — push the summary to the GitHub PR body).

### `[commands]`

Shell strings run via `sh -c` for hub actions: `summary`, `test`, `lint`, `typecheck`, `security` — each optional. Placeholders: `{base}`, `{branch}`, `{repo}`, `{output}` (default output path, e.g. the managed `{er_dir}/summary.md`).

### `[packages]`

Per-package command overrides for mono-repos. Each key is a package id carrying `label`, `test`, `lint`, `typecheck`, `security`. Packages get their own section in the Verify hub, each disabled until it has at least one command.

### `[inbox]`

Desktop sidebar inbox and OS notifications: `show` and `notify`, each a table of kind → bool, all defaulting to `true`. Kinds: `review_requested` (also accepts the stored alias `review_rerequested`), `pr_review_approved`, `pr_review_changes_requested`, `pr_review_received`, `pr_comment`, `pr_comment_reply`, `mention`, `ci_failed`, `pr_merged`, `pr_closed`, `github_refresh_failed`, `ai_review_done`, `ai_review_failed`, `ai_triage_done`, `ai_triage_failed`. An unrecognized kind stays visible and never notifies.

### `[ai_hub]`

Provider and model presets for every AI Hub action — review, triage, tours, experts, Professor, validation, questions, summary, card AI.

Top-level keys: `default_provider`, `default_model`, `default_effort` (optional), `max_concurrent_reviews`, `removed_catalog_providers`.

```toml
[ai_hub]
default_provider = "claude"
default_model = "sonnet-5"
default_effort = "high"        # omit, or pick Auto in the UI, for the provider default
max_concurrent_reviews = 3     # background reviews + arena reviewers

[ai_hub.providers.claude]
label = "Claude"
command = "claude"
args = ["--print", "-p", "{prompt}"]
family = "claude"              # optional: adopt another family's arg conventions
models_command = []            # optional: CLI that lists model ids, e.g. ["agent", "--list-models"]

[[ai_hub.providers.claude.models]]
id = "sonnet-5"                # required
label = "Sonnet 5"
args = ["--model", "claude-sonnet-5"]
effort_levels = ["low", "medium", "high", "xhigh", "max"]
```

Provider keys: `label`, `command`, `args`, `family`, `models_command`, `models`, `removed_catalog_models`.
Model keys: `id` (required), `label`, `description`, `args`, `effort_levels`, and the optional `cost_per_1k_in`, `cost_per_1k_out`, `avg_latency_ms` used for arena cost estimates.

Rules:

- A built-in catalog — claude, codex, cursor, opencode — is merged into the in-memory config on load, adding models it has and you do not. Deprecated Claude model ids are dropped, and a `default_model` naming one is replaced with the catalog default. The file itself is untouched until something saves, and models found by `models_command` discovery are never persisted.
- Deleting a catalog provider or model in the UI records its id in `removed_catalog_providers` / `removed_catalog_models`, which is what stops the catalog re-adding it.
- With no providers configured, `[ai_hub]` is inert and every action falls back to `[agent]`.
- Provider `args` are the shared base for that CLI; model `args` are appended after them. OpenCode is the exception: its model and effort flags are inserted **before** `{prompt}`, which is a trailing positional argument there.
- `effort_levels` is authoritative per model — support is never inferred from a model id. `Auto` omits the override; otherwise Claude receives `--effort <level>`, Codex `-c model_reasoning_effort=<level>`, OpenCode `--variant <level>`.
- Triage forces low effort regardless of `default_effort`. A model chosen for a single run overrides the default for that run only.
- Claude, Codex and Cursor Agent spawns that write sidecars receive the active review bucket as `--add-dir`, never the storage root, and Codex treats it as writable under `workspace-write`. Custom provider commands are never given unknown CLI flags.
- OpenCode runs as `opencode run --auto` with an `OPENCODE_PERMISSION` env object (bare permission JSON; no `--add-dir`) denying every `external_directory` path but the active bucket. Card AI keeps `--auto` with a read-only permission object instead.
- `max_concurrent_reviews` bounds the background review queue and arena reviewer rounds (default 3; the pickers offer 1–6 in the TUI and 1–16 in the desktop). Every path that spawns a review agent acquires a slot — the background review dispatch, arena reviewer rounds, the arena arbiter, the AI Hub, card AI and `spawn_command`; `model_discovery::run_models_command` is a listing probe and takes none. See `docs/adr/0021-agent-concurrency.md`.

## Review sidecars

Sidecars live in managed storage, never in the repo under review: `<storage_root>/repos/<repo_slug>/branches/<branch_slug>/view-buckets/<bucket>/`, with the PR bucket kept separate at `<storage_root>/repos/<owner_repo_slug>/prs/pr-<N>/`. The TUI and the desktop resolve the same path for the same branch. See `docs/adr/0003-managed-review-storage.md`; the split per view bucket is `docs/adr/0004-per-view-artifact-scoping.md`.

| File | Written by |
|---|---|
| `review.json` | General review — per-file risk, summaries, findings |
| `order.json` | General review — suggested review order |
| `checklist.json` | General review — manual verification items |
| `summary.md` | General review — overall summary |
| `triage.json` | Triage — routing verdict, not findings |
| `experts/<id>.json` | One specialized expert review each |
| `professor.json` | Professor — teaching insights |
| `tour.json` | Guided tour, one per view bucket |
| `questions.json`, `notes.json`, `github-comments.json`, `reviewed`, `session.json` | the app itself (`docs/adr/0007-three-comment-stores.md`) |

A specialized expert review writes only its own `experts/<id>.json` and never overwrites the general artifacts. At load, a fresh expert sidecar (matching diff hash) merges into the in-memory review as an extra inline banner labeled per expert; the order, checklist and summary panels still require a general review run. Expert ids live in `EXPERTS` in `crates/er-engine/src/ai/experts.rs`.

Prompts are compiled into the binary — nothing reads a prompt or skill file at runtime (`docs/adr/0023-self-contained-agent-prompts.md`). `skills/REVIEW_RULES.md` and `skills/PROFESSOR_PHILOSOPHY.md` survive only as provenance for that wording.
