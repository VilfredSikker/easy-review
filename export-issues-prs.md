# Issues & Pull Requests — vilfredsikker/easy-review

---

## ISSUES

### Issue #12 [OPEN]
**Title:** Automatically release new versions to `brew`
**URL:** https://github.com/VilfredSikker/easy-review/issues/12
**Description:**
It would be great to be able to get automatic releases of this tool when running `brew` 😄 

https://docs.brew.sh/Adding-Software-to-Homebrew

---

### Issue #18 [OPEN]
**Title:** LSP integration for symbol info, references, and diagnostics
**URL:** https://github.com/VilfredSikker/easy-review/issues/18
**Description:**
## Context

When reviewing diffs in `er`, you see changed lines but have no IDE intelligence — you can't see a type signature, jump to a definition, or see compile errors inline. You have to context-switch to an editor. Since `er` is built for fast AI-assisted code review, this friction matters.

**TL;DR:** Give the diff viewer IDE smarts by talking to language servers in a background thread, keeping the sync event loop untouched.

## Features

- **Auto symbol info** — when the user settles on a diff line (300ms debounce), send an LSP hover request. Type signature and docs appear in the **side panel** automatically.
- **Go-to-definition** (`gd`) — jump to the symbol's definition. If in the diff, navigate there. If external, open `$EDITOR`.
- **Find references** (`gr`) — show all references in the side panel, grouped by file, navigable.
- **Diagnostics** — `E`/`W` gutter markers on diff lines with errors/warnings. `D` to expand detail in the side panel.

## Architecture

Follows the existing `watch/` module pattern — background threads + `mpsc` channels, no async runtime.

```
Main thread (sync, 100ms poll)              Background threads (per LSP server)
  |                                           |
  |-- lsp_cmd_tx.send(LspCommand) ----------> |  Write thread: recv -> serialize -> write stdin
  |                                           |  Read thread: read stdout -> parse -> send event
  |<-- lsp_rx.try_recv() ------------------- |  (mpsc::Sender<LspEvent>)
```

- Two threads per LSP server (read + write)
- Request/response matching via monotonic u64 IDs
- 5-second timeout for stale requests
- Lazy server start on first request per language
- `textDocument/didOpen` on demand (only files the user navigates to)

## Module Structure

```
src/lsp/
  mod.rs          -- Public API: LspManager, LspEvent, re-exports
  transport.rs    -- JSON-RPC framing (Content-Length read/write)
  protocol.rs     -- Minimal LSP types (hand-rolled, no lsp-types crate)
  server.rs       -- Single LSP server lifecycle: spawn, initialize, shutdown
  manager.rs      -- Multi-server coordinator: one per language, routing
  positions.rs    -- Diff line -> LSP Position mapping
```

## Key Design Decisions

| Decision | Choice | Why |
|----------|--------|-----|
| Async runtime | No — `std::thread` + `mpsc` | Matches existing architecture |
| LSP types | Hand-rolled in `protocol.rs` | `lsp-types` pulls ~5 transitive deps; small subset needed |
| Server lifecycle | Lazy start on first request | rust-analyzer takes 10-30s to index; don't block startup |
| Document sync | `didOpen` on demand | Only files user navigates to |
| Position mapping | `DiffLine.new_num` | Exact for Add/Context lines. Delete -> unavailable |
| UI surface | Side panel | Consistent with existing AI/comment panels |
| Feature flag | `[lsp] enabled = false` | Opt-in until stable |

## Interaction Model

**Automatic, not manual.** LSP info appears as the user navigates — no explicit hover key.

- 300ms debounce on line navigation before sending hover request
- Side panel shows type signature + docs when available
- `gd` / `gr` use vim-style `g` prefix (500ms chord window)

## Configuration

```toml
# .er-config.toml
[lsp]
enabled = true
auto_detect = true

[lsp.servers]
rust = { command = "rust-analyzer", args = [] }
typescript = { command = "typescript-language-server", args = ["--stdio"] }
python = { command = "pyright-langserver", args = ["--stdio"] }
go = { command = "gopls", args = ["serve"] }
```

## Phased Implementation

### Phase 1: Foundation — Transport + Single Server
- JSON-RPC Content-Length framing
- Spawn LSP server, initialize/shutdown handshake
- `textDocument/didOpen` + basic `hover` request/response
- Background threads with channels
- Unit tests for framing and protocol

### Phase 2: Manager + Auto Symbol Info
- `LspManager` with multi-language support
- Language detection from file extensions
- Debounced auto-hover (300ms)
- Side panel `PanelContent::LspInfo`
- Loading indicator in status bar

### Phase 3: Go-to-Definition + Find References
- `g` prefix mode (`gd`, `gr`)
- Go-to-def navigation
- Find-refs in side panel (`PanelContent::LspReferences`)

### Phase 4: Diagnostics
- `publishDiagnostics` notification handling
- Gutter markers (`E`/`W`)
- `D` key for detail expansion

### Phase 5: Polish
- Auto-detect servers on PATH
- `didChange` on watch events
- Crash recovery (max 3 retries/session)
- Column cursor for precise positioning
- Debug logging

## Error Handling

| Scenario | Behavior |
|----------|----------|
| Server not on PATH | One-time notification |
| Server crashes | Retry on next request (max 3/session) |
| Request times out (5s) | Clear loading, notification |
| Deleted lines | "LSP unavailable" (no `new_num`) |
| Unknown language | Silent no-op |
| `er` crashes | Panic hook kills children |

## Risks

| Risk | Mitigation |
|------|------------|
| Slow server startup (10-30s) | Lazy start + loading indicator |
| Position mapping for modified lines | `new_num` exact for Add/Context; Delete unmappable |
| Child process leaks | Panic hook + Drop impl |
| Hand-rolled protocol bugs | Unit tests; migrate to `lsp-types` if needed |

---

### Issue #21 [OPEN] | Labels: enhancement
**Title:** Agent Review Trigger — trigger AI review from inside ER
**URL:** https://github.com/VilfredSikker/easy-review/issues/21
**Description:**
**Priority:** P0 | **Effort:** Small

## Problem

ER already consumes `.er-*.json` sidecar files written by external tools, but triggering the agent review requires leaving ER, switching to a terminal, and running a separate command. This breaks flow and adds friction to the review loop.

## Proposal

Add a keybind (`A` by default, configurable) that triggers an agent review from inside ER. ER shells out to the configured agent command in the background, shows a status indicator, and auto-loads results when the `.er-*.json` files appear.

## How it works

1. Developer presses `A` (or configured key) in ER
2. ER reads the agent command from config (`agent.command` + `agent.args`)
3. ER shells out to the agent process in the background, expanding template variables: `{repo_root}`, `{base_branch}`, `{head_branch}`, `{diff_path}`, `{pr_number}`
4. A status indicator appears in the header bar ("Agent reviewing..." with spinner)
5. When the agent writes `.er-*.json` files, ER's existing file watcher detects them and loads results. Status updates to "Review ready"

## Config

Extends the existing `AgentConfig` struct:

```toml
[agent]
command = "claude"
args = ["--print", "-p", "/review-pr"]
trigger_key = "A"              # customizable keybind
auto_review = false            # trigger automatically on diff load?
timeout_secs = 300             # kill after 5 min
```

Template variables are expanded before execution. This means any agent tool works — Claude Code with a custom skill, a Python script, aider, or the multi-agent arena (#2).

## Implementation notes

- The `AgentConfig` struct already exists with `command` and `args` fields — extend it with `trigger_key`, `auto_review`, `timeout_secs`
- Background process spawning via `std::process::Command` with non-blocking wait
- Status state lives on `TabState` (per-repo): `agent_status: Option<AgentStatus>` with variants `Running { pid, started_at }` / `Completed` / `Failed { stderr }`
- Kill on timeout using the stored PID
- The existing file watcher + AI loader poll (every ~10 ticks) handles result loading — no new watching infrastructure needed
- Add key routing in the normal-mode handler in `main.rs`

## Acceptance criteria

- [ ] Pressing the trigger key spawns the configured agent command
- [ ] Template variables are correctly expanded
- [ ] Header bar shows spinner while agent is running
- [ ] Results auto-load when `.er-*.json` files are written
- [ ] Agent process is killed on timeout
- [ ] Status shows error if agent exits non-zero
- [ ] `auto_review = true` triggers on diff load
- [ ] Works with any command (Claude Code, custom script, etc.)

---

### Issue #22 [OPEN] | Labels: enhancement
**Title:** Multi-Agent Arena (er-arena) — run N agents in parallel
**URL:** https://github.com/VilfredSikker/easy-review/issues/22
**Description:**
**Priority:** P1 | **Effort:** Medium

## Problem

A single AI reviewer has blind spots. Different models, prompts, and review focuses (security vs architecture vs correctness) catch different issues. Running multiple agents manually and reconciling their output is tedious.

Inspired by the [Latent Space "competition among agents"](https://www.latent.space/p/reviews-dead) concept: the cost of optionality is the lowest in the history of software engineering.

## Proposal

A standalone CLI tool (`er-arena`) that ER invokes via the agent trigger system (#21). It runs N agents in parallel with different configurations and synthesises their findings into standard `.er-*.json` output.

## Architecture

`er-arena` is a separate binary, decoupled from ER — usable standalone or from CI. ER calls it as its `agent.command`.

### Arena config (`.er-arena.toml`)

```toml
[[agents]]
name = "claude-security"
command = "claude"
args = ["--print", "-p", "/review-security"]

[[agents]]
name = "claude-architecture"
command = "claude"
args = ["--print", "-p", "/review-arch"]

[[agents]]
name = "gemini-general"
command = "gemini-review"
args = ["{diff_path}"]

[synthesis]
strategy = "merge"   # merge | vote | highest-severity
```

### Synthesis strategies

- **Merge** (default): Combine all findings, deduplicate by location + category, keep the highest severity when duplicates conflict
- **Vote**: Only surface findings that N/M agents agree on. Reduces noise at the cost of recall
- **Highest-severity**: For each file, use the agent that flagged the most severe issues. Good for security-focused reviews

### Output

Standard `.er-*.json` files so ER doesn't need to know agents competed. Findings include an `agent_name` field for attribution in the UI.

## ER integration

```toml
# .er-config.toml — use arena as the agent command
[agent]
command = "er-arena"
args = ["{repo_root}", "--base", "{base_branch}"]
```

## Open questions

- [ ] Should er-arena produce one merged `.er-review.json`, or one per agent (e.g. `.er-review-claude-security.json`)? Could support both via a `--output` flag
- [ ] Should synthesis be done by a final LLM call, or purely algorithmic (dedup + severity merge)?
- [ ] Cost/token visibility: should the arena report total tokens used across agents?

## Acceptance criteria

- [ ] Arena config defines N agents with independent commands
- [ ] All agents run in parallel
- [ ] Findings are synthesised according to the configured strategy
- [ ] Output is valid `.er-review.json` consumable by ER
- [ ] Each finding has `agent_name` attribution
- [ ] Arena exits with non-zero if all agents fail
- [ ] Usable standalone (not just via ER)

---

### Issue #23 [OPEN] | Labels: enhancement
**Title:** Archwatch Integration — launch dependency graph from ER
**URL:** https://github.com/VilfredSikker/easy-review/issues/23
**Description:**
**Priority:** P0 | **Effort:** Small

## Problem

Understanding the architectural impact of a change requires switching from ER to a separate archwatch browser window. The dependency information isn't available in the review flow where decisions are made.

## Proposal

Two integration modes, shipped incrementally.

### Mode 1: Launch with context (P0, this issue)

Press `g` (graph) in ER to launch archwatch in the browser, pre-focused on modules affected by the current diff. Changed nodes are visually highlighted (pulsing, colour-coded by risk level from the AI review if available).

#### Config

```toml
[archwatch]
binary = "aw"                  # path to archwatch binary
port = 3210                    # default port
auto_launch = false            # launch on diff load?
```

#### How it works

1. ER computes the list of changed file paths from the current diff
2. ER spawns `aw {repo_root} --highlight {file1},{file2},... --port {port}` (or connects to an already-running instance)
3. Archwatch opens in the browser with changed nodes marked
4. If AI review data is loaded, ER passes risk levels as query params so archwatch can colour-code nodes

#### Archwatch changes needed

- Accept `--highlight` flag with comma-separated file paths
- Accept `--risk` flag or query param with JSON mapping files to risk levels
- If already running, accept highlight updates via the existing WebSocket channel

### Mode 2: Inline impact summary (P2, future)

Extract archwatch's analyzer into a shared Rust crate (`aw-core`) that ER can depend on. Display a dependency summary in ER's side panel: for each changed file, show direct dependents, transitive dependents, and a coupling score. No D3 visualization needed — text-based impact analysis.

This mode is tracked separately and depends on the shared crate extraction.

## Acceptance criteria (Mode 1)

- [ ] Pressing `g` launches archwatch focused on changed modules
- [ ] If archwatch is already running, it receives highlight updates
- [ ] Changed nodes are visually distinct in the archwatch UI
- [ ] Config allows customising binary path and port
- [ ] `auto_launch = true` starts archwatch on diff load
- [ ] Works without archwatch installed (shows helpful error message)

---

### Issue #24 [OPEN] | Labels: enhancement
**Title:** Verification Pipeline — run lint/test/security checks from ER
**URL:** https://github.com/VilfredSikker/easy-review/issues/24
**Description:**
**Priority:** P1 | **Effort:** Medium

## Problem

AI review findings are opinions. Tests, lints, and contract checks are facts. Currently there's no way to run deterministic verification steps from within ER and see their results alongside AI findings. Developers have to context-switch to a terminal, run checks manually, and mentally correlate the output.

Inspired by the [Latent Space article](https://www.latent.space/p/reviews-dead) on deterministic verification and [Airlock's](https://github.com/airlock-hq/airlock) pipeline concept — but implemented as a review feature, not a git proxy.

## Proposal

A configurable pipeline of verification steps defined in `.er-config.toml`. Each step runs a command, captures pass/fail, and surfaces results in ER's header bar and a dedicated pipeline panel.

### Pipeline config

```toml
[[pipeline.steps]]
name = "lint"
command = "cargo clippy -- -D warnings"
blocking = true                # must pass before approval

[[pipeline.steps]]
name = "test"
command = "cargo test"
blocking = true

[[pipeline.steps]]
name = "secrets"
command = "trufflehog git file://. --since-commit {base_sha}"
blocking = true

[[pipeline.steps]]
name = "contracts"
command = "check-api-contracts.sh"
blocking = false               # advisory only
```

### ER display

- **Header bar**: A row of step names with pass/fail/running icons (checkmark, X, spinner)
- **Pipeline panel** (new `PanelContent::Pipeline` variant): Full output for each step, expandable/collapsible
- **Blocking failures**: Highlighted in red. The approval workflow (#25) prevents push until these pass

### Execution

- Triggered manually with `P` (pipeline) or automatically on diff load (`auto_run = true`)
- Steps run in parallel by default (sequential if a `depends_on` field is set)
- Results cached per diff hash, invalidated on change
- Template variables: `{repo_root}`, `{base_branch}`, `{base_sha}`, `{head_sha}`

## Implementation notes

- New `PipelineConfig` struct with `Vec<PipelineStep>`
- `PipelineStep`: `name`, `command`, `blocking`, `depends_on: Option<String>`, `timeout_secs`
- Pipeline state on `TabState`: `pipeline: Option<PipelineState>` with per-step status
- Background execution via `std::process::Command`, stdout/stderr captured
- New `PanelContent::Pipeline` variant for the side panel
- Header bar rendering updated to show step status icons

## Open questions

- [ ] Should pipeline results be persisted to a `.er-pipeline.json` file? Useful for the approval workflow to know last-run status across restarts
- [ ] Should there be a "fix" mode where failing lint steps auto-apply fixes (like Airlock's pre-freeze steps)?

## Acceptance criteria

- [ ] Pipeline steps defined in config run on keypress
- [ ] Header bar shows per-step pass/fail/running status
- [ ] Pipeline panel shows full output per step
- [ ] Blocking failures are visually distinct
- [ ] Steps support template variable expansion
- [ ] Results are cached per diff hash
- [ ] `auto_run = true` triggers pipeline on diff load
- [ ] Individual steps can be re-run

---

### Issue #25 [CLOSED] | Labels: enhancement
**Title:** Review Session Persistence — survive ER restarts
**URL:** https://github.com/VilfredSikker/easy-review/issues/25
**Description:**
**Priority:** P0 | **Effort:** Small

## Problem

When ER restarts, all review progress is lost — which files were marked as reviewed, scroll position, comment drafts. For large PRs reviewed across multiple sittings, this means re-doing triage work.

## Proposal

Persist the review session to a `.er-session.json` file keyed by diff hash. When ER opens the same branch, restore the session state.

### Persisted state

- Reviewed file set (files marked with `Space`)
- Scroll position (selected file index + line offset)
- Comment drafts (in-progress text that hasn't been submitted)
- Compaction overrides (files manually expanded/collapsed)
- Filter state (active filter expression)
- Active diff mode

### File format

```json
{
  "version": 1,
  "diff_hash": "abc123...",
  "branch": "feature/my-branch",
  "base_branch": "main",
  "created_at": "2026-03-04T12:00:00Z",
  "updated_at": "2026-03-04T14:30:00Z",
  "reviewed_files": ["src/main.rs", "src/config.rs"],
  "selected_file_index": 3,
  "scroll_offset": 42,
  "filter_expression": "+*.rs,-*.lock",
  "diff_mode": "branch",
  "expanded_files": ["Cargo.lock"],
  "comment_drafts": []
}
```

### Behaviour

- Auto-saved on every state change (debounced, ~2s)
- Loaded on startup if diff hash matches (same branch state)
- Invalidated when diff hash changes (new commits pushed)
- Location: repo root `.er-session.json` (gitignored by convention)
- Multiple sessions per repo not needed — one branch at a time

## Implementation notes

- Serialization via `serde_json` (already a dependency)
- Save triggered from the main event loop after state changes, debounced
- Load in `TabState::new()` after diff parsing, before first render
- Diff hash comparison uses existing `compute_diff_hash()` from `ai/loader.rs`

## Acceptance criteria

- [ ] Session state is persisted to `.er-session.json`
- [ ] Reviewed files are restored on reopen
- [ ] Scroll position is restored
- [ ] Session is invalidated when diff changes
- [ ] Save is debounced (not on every keypress)
- [ ] Filter and diff mode are restored
- [ ] Compaction overrides survive restart

---

### Issue #26 [OPEN] | Labels: enhancement
**Title:** Diff Summary / Changelog Generation — generate PR descriptions from ER
**URL:** https://github.com/VilfredSikker/easy-review/issues/26
**Description:**
**Priority:** P1 | **Effort:** Small

## Problem

After reviewing a PR, writing the PR description or changelog entry is a separate manual step. The agent that reviewed the code already understands the changes — it should be able to generate a summary.

## Proposal

A keybind (`C`) that triggers an agent to generate a human-readable changelog or PR description from the current diff. This is a natural extension of the agent trigger system (#21) — same config mechanism, different prompt.

### Output targets

1. **`.er-summary.md`** — already supported by ER's AI loader. The summary appears in the AI Summary panel
2. **GitHub PR body** — optionally push the summary to the PR description via `gh pr edit --body`

### Config

```toml
[agent.changelog]
command = "claude"
args = ["--print", "-p", "Generate a concise PR description for this diff: {diff_path}"]
push_to_pr = false             # auto-update GitHub PR body?
```

Or reuse the main agent command with a different prompt template, selectable via the keybind.

### UX flow

1. Developer finishes reviewing, presses `C`
2. Agent generates summary → written to `.er-summary.md`
3. Summary appears in ER's existing AI Summary panel
4. If `push_to_pr = true` and a PR is detected, update the PR body via `gh pr edit`
5. Developer can edit the summary via the comment system before pushing

## Implementation notes

- Reuses the agent spawning infrastructure from #21
- `.er-summary.md` loading already exists in `ai/loader.rs`
- GitHub PR body update uses existing `gh` CLI shell-out pattern from `github.rs`
- Could be implemented as a second agent "slot" alongside the review agent, or as a mode flag on the same trigger

## Acceptance criteria

- [ ] Pressing `C` triggers changelog generation
- [ ] Output written to `.er-summary.md` and displayed in panel
- [ ] Optionally pushes to GitHub PR description
- [ ] Works with any configured agent command
- [ ] Developer can preview before pushing to PR

---

### Issue #27 [OPEN] | Labels: enhancement
**Title:** Inline Spec Coverage Indicators — show spec requirement coverage per file
**URL:** https://github.com/VilfredSikker/easy-review/issues/27
**Description:**
**Priority:** P2 | **Effort:** Medium

## Problem

The [Latent Space article](https://www.latent.space/p/reviews-dead) argues that specs should be the source of truth, and code should be verified against specs rather than reviewed directly. But adopting a full BDD system is a large commitment. There's a lightweight middle ground.

## Proposal

If `.spec.md`, `.feature`, or similar spec files exist adjacent to changed code, ER shows a coverage indicator per file: which spec requirements are addressed by the change.

### How it works

1. ER scans for spec files adjacent to or associated with changed files (configurable patterns)
2. Spec files are parsed for individual requirements (numbered items, Given/When/Then blocks, checkbox lists)
3. The AI review agent can reference specs and annotate findings with spec coverage (new field in `.er-review.json`)
4. ER displays per-file spec coverage in the file list: e.g. `[3/5 specs]` next to the filename
5. Expanding shows which specs are covered and which are missing

### Config

```toml
[specs]
patterns = ["*.spec.md", "*.feature", "SPEC.md"]
search_dirs = [".", "specs/", "../specs/"]
```

### Extended `.er-review.json` format

```json
{
  "files": {
    "src/auth.rs": {
      "spec_coverage": {
        "spec_file": "src/auth.spec.md",
        "total_requirements": 5,
        "covered": ["req-1", "req-3", "req-4"],
        "uncovered": ["req-2", "req-5"],
        "notes": "Password validation logic not updated for new spec requirement"
      }
    }
  }
}
```

## Implementation notes

- Spec file discovery is a scan at diff-load time — no new watching needed
- Spec parsing is best done by the AI agent (complex formats), not ER itself
- ER just consumes the `spec_coverage` field from the review JSON
- Minimal ER changes: render the coverage indicator in file list + panel detail

## Acceptance criteria

- [ ] ER discovers spec files based on configured patterns
- [ ] Coverage indicators shown in file list when data available
- [ ] Panel shows detailed coverage breakdown
- [ ] Agent review format supports `spec_coverage` field
- [ ] Works without spec files (graceful degradation)

---

### Issue #28 [OPEN] | Labels: enhancement
**Title:** Review Approval Workflow — one-key ship-it action
**URL:** https://github.com/VilfredSikker/easy-review/issues/28
**Description:**
**Priority:** P1 | **Effort:** Small

## Problem

After completing a review in ER, the developer still has to context-switch to push, approve on GitHub, or confirm the branch is ready. There's no single "I'm done, ship it" action that validates everything is clear.

## Proposal

A readiness state machine that tracks review completion and enables a one-key approval action.

### Readiness conditions

All must be true for "Ready to approve" state:

1. All files marked as reviewed (or filtered to reviewed-only with none remaining)
2. All blocking pipeline steps pass (#24, if configured)
3. No unresolved high-severity AI findings
4. No unresolved personal questions (unless explicitly dismissed)

### UX

- Header bar shows a readiness indicator: red (not ready) → yellow (partial) → green (ready)
- Hovering/expanding shows which conditions are not met
- When ready, pressing `G` (go) triggers the approval action:
  - Push to remote (`git push`)
  - Approve GitHub PR (`gh pr review --approve`)
  - Or just mark locally as approved (if no PR)
- Configurable which actions `G` performs

### Config

```toml
[approval]
push_on_approve = true
gh_approve = true
require_all_reviewed = true
require_pipeline_pass = true
require_no_high_severity = true
```

### Safety

- Approval action always asks for confirmation (ER's existing `ConfirmAction` system)
- Force-approve available with `Shift+G` (bypasses readiness check, still confirms)

## Implementation notes

- Readiness computed from existing state: `reviewed_files`, `pipeline_status`, `ai.findings`
- New `ApprovalState` enum on `TabState`: `NotReady { reasons }` / `Ready` / `Approved`
- `G` key routes through `ConfirmAction::Approve` before executing
- Shell-outs to `git push` and `gh pr review` follow existing patterns in `github.rs`

## Acceptance criteria

- [ ] Readiness indicator in header bar
- [ ] All configurable conditions checked
- [ ] `G` triggers push + approval when ready
- [ ] Confirmation required before execution
- [ ] Force-approve available with `Shift+G`
- [ ] Each approval action independently configurable

---

### Issue #29 [OPEN] | Labels: enhancement
**Title:** Custom Annotation Layers — pluggable inline overlays for any tool
**URL:** https://github.com/VilfredSikker/easy-review/issues/29
**Description:**
**Priority:** P2 | **Effort:** Small

## Problem

ER currently supports three inline annotation types: AI findings, GitHub comments, and personal questions. But other tools produce useful per-line annotations too — test coverage, performance profiling, type coverage, security scan results. There's no way to surface these in the review flow.

## Proposal

Allow any tool to write `.er-annotations.json` with typed annotation layers. Each layer appears as a togglable inline overlay with a distinct colour, independent of the built-in layers.

### File format

```json
{
  "version": 1,
  "diff_hash": "abc123...",
  "layers": [
    {
      "id": "coverage",
      "name": "Test Coverage",
      "color": "#22c55e",
      "icon": "C",
      "annotations": [
        {
          "file": "src/main.rs",
          "line_start": 42,
          "line_end": 50,
          "severity": "info",
          "message": "No test coverage for this block",
          "detail": "Branch coverage: 0% (0/3 branches)"
        }
      ]
    },
    {
      "id": "perf",
      "name": "Performance",
      "color": "#f59e0b",
      "icon": "P",
      "annotations": [
        {
          "file": "src/git/diff.rs",
          "line_start": 100,
          "line_end": 100,
          "severity": "medium",
          "message": "Hot path: 45% of CPU time in this function",
          "detail": "Consider caching the parse result"
        }
      ]
    }
  ]
}
```

### ER display

- Each layer gets a toggle key (auto-assigned or configured)
- Annotations render inline like AI findings, but with the layer's colour and icon
- Layer visibility managed via `InlineLayers` extension
- Layers listed in the settings overlay for toggle management

### Producing annotations

Any tool can write `.er-annotations.json`:
- Coverage: `cargo llvm-cov` output → converter script → `.er-annotations.json`
- Performance: `cargo flamegraph` or profiler output → converter
- Security: `semgrep` or `snyk` output → converter
- The arena (#22) could also write custom layers per agent

## Implementation notes

- Follows the `.er-*.json` convention — loaded by `ai/loader.rs` alongside other sidecar files
- Annotation struct is a superset of `Finding` with an added `layer_id` field
- `InlineLayers` extended with a `HashMap<String, bool>` for custom layer toggles
- Rendering in `diff_view.rs` extended to handle arbitrary layers

## Acceptance criteria

- [ ] `.er-annotations.json` loaded and parsed
- [ ] Custom layers rendered inline with distinct colours
- [ ] Each layer independently togglable
- [ ] Layers appear in settings overlay
- [ ] Navigation (`J`/`K`) includes custom annotations
- [ ] Staleness detection via diff hash (same as AI findings)
- [ ] Graceful handling of malformed layer files

---

### Issue #30 [CLOSED] | Labels: enhancement
**Title:** Modal Hub Architecture — consolidate keybinds into contextual popups
**URL:** https://github.com/VilfredSikker/easy-review/issues/30
**Description:**
## Problem

The current keyspace is crowded with single-purpose bindings (`G`, `P`, `Ctrl+P`, `z/Z`, `f/F`, `R`, `w/W`, `m`, `o`, `x`). As features grow, this doesn't scale — users can't memorize 25+ bindings, and there's no room for new ones.

## Proposal

Replace rare/grouped bindings with **modal hubs** — press one key to open a contextual popup, navigate with `j/k`, select with `Enter`. Same pattern as the existing Settings overlay (`S`).

### Modals

| Key | Modal | Contains |
|-----|-------|---------|
| `g` | **Git modal** | git push, pull GitHub comments (`G`), push GitHub comments (`P`), stash, cleanup artifacts (`z/Z`), history ops |
| `A` | **AI modal** | trigger review, multi-agent arena, ask about code, generate changelog/PR description, explain change |
| `v` | **Verify modal** | run tests, lint, type check, security scan (covers #24) |
| `?` | **Help modal** | full keybind reference, always discoverable |

`A` is a natural pair with `a` (AI panel toggle), following the existing `c/C` and `q/Q` convention.

### Keybinds to remove (moved into modals or dropped)

| Binding | Reason |
|---------|--------|
| `G` | → Git modal (pull GitHub comments) |
| `P` | → Git modal (push GitHub comments) |
| `Ctrl+P` | → Git modal (git push) |
| `z/Z` | → Git modal (cleanup artifacts) |
| `R` / `Shift+R` | → Settings or drop (mtime sort is rare) |
| `f/F` | Evaluate: keep direct if filter is frequent, else → nav modal |
| `w/W`, `m`, `o`, `x` | Audit and move or drop |

### Direct bindings to keep (core review flow only)

- **Navigation:** `j/k` `J/K` `[/]` `Tab` `1-5` `Enter` `Esc`
- **Review:** `c` `q` `r` `d` `U`
- **Tools:** `/` `e` `a` `S` `t`
- **Quit:** `Ctrl+q`

## Related issues absorbed by this

- #21 — Agent Review Trigger → AI modal
- #22 — Multi-Agent Arena → AI modal
- #23 — Archwatch Integration → Git modal
- #24 — Verification Pipeline → Verify modal
- #26 — Diff Summary / Changelog → AI modal
- #28 — Review Approval Workflow → could live in Review modal

## Implementation order

1. Build modal framework (reuse overlay infrastructure from Settings)
2. Git modal — migrate existing bindings (`G`, `P`, `Ctrl+P`, `z/Z`)
3. AI modal — migrate existing + add new AI commands
4. Verify modal — new functionality
5. Help modal — enumerate all bindings
6. Audit and remove freed direct bindings

---

### Issue #37 [OPEN]
**Title:** explore: implement Verify hub — run tests, linter, and type checker from within EasyReview
**URL:** https://github.com/VilfredSikker/easy-review/issues/37
**Description:**
## Context

The Verify hub (opened via `v`) is currently stubbed out with three placeholder actions:
- **Run tests** — run project test suite
- **Run linter** — run configured linter
- **Type check** — run type checker

Security scan is explicitly out of scope for now.

This issue is **exploratory only** — the goal is to map the problem space before any implementation.

---

## The Core Problem

EasyReview is a TUI tool that runs inside an arbitrary project directory. To execute project-specific checks, it needs to know *how* to run them. The challenge: every project is different.

A Rust project uses `cargo test` / `cargo clippy` / `cargo check`.  
A Node project might use `bun test` / `eslint` / `tsc`.  
A Python project uses `pytest` / `ruff` / `mypy`.

EasyReview has no knowledge of these by default.

---

## Design Questions to Explore

### 1. How does EasyReview discover the project setup?

**Option A — Config file detection**  
Scan the working directory for known config files (`Cargo.toml`, `package.json`, `pyproject.toml`, etc.) and infer the toolchain. Map toolchain → default commands.

- ✅ Zero config for common stacks
- ❌ Fragile — projects override defaults constantly (`bun test` vs `vitest`, etc.)
- ❌ Doesn't handle monorepos well

**Option B — EasyReview config file (`.easyreview.toml`)**  
User defines commands explicitly in a project-level config:
```toml
[verify]
test = "bun test"
lint = "bun lint"
typecheck = "bun check"
```

- ✅ Explicit, reliable, flexible
- ✅ Easy to extend (just add a key)
- ❌ Requires setup per project
- ❌ Config file proliferation

**Option C — Read scripts from existing project manifests**  
Parse `package.json` scripts, `Makefile` targets, or `Cargo.toml` workspace commands directly.

- ✅ No extra config if conventions are followed
- ❌ Naming conventions vary wildly (`test` vs `test:unit` vs `test:coverage`)
- ❌ Ambiguous which script to use

**Option D — User-configurable command palette (runtime)**  
Let users define/override commands from within EasyReview itself (e.g. via a settings prompt), stored in `~/.config/easyreview/` or `.easyreview.toml`.

- ✅ Flexible, no assumptions
- ❌ More UX complexity

### 2. How does EasyReview *run* the commands?

**Option A — Spawn subprocess, capture stdout/stderr**  
Run the command as a child process, stream output into a scrollable TUI panel.

- ✅ Clean, no side effects
- ✅ Keeps EasyReview in control of the UI

**Option B — Suspend TUI, hand off to terminal**  
Temporarily suspend the TUI (like `vim :!`), run the command in the raw terminal, then resume.

- ✅ Output renders exactly as the tool intends (colors, progress bars)
- ✅ Interactive tools (e.g. test watchers) work naturally
- ❌ Loses TUI context during run

**Option C — Open a split pane / overlay with output**  
Keep the diff visible, show output in a bottom panel or overlay while the command runs.

- ✅ Best UX — context stays visible
- ❌ Most complex to implement (async output streaming into ratatui)

### 3. Performance

Runs need to be **fast** and non-blocking. Concerns:
- Long-running test suites should not freeze the TUI
- Output should stream as it arrives, not batch at end
- Should be cancellable (Esc or `q` while running)

### 4. Error surfacing

When a check fails, what does EasyReview show?
- Raw output only?
- Parsed errors highlighted in the diff view?
- Badge/indicator on the file that failed?

Deeper integration (e.g. linking lint errors back to diff lines) is powerful but complex.

---

## Constraints

- Must work without internet — commands run locally
- Must respect the project's existing toolchain — EasyReview should not install or manage dependencies
- Runs should feel snappy — probably means running only what's needed (affected files?) rather than full suite by default
- Security scan deferred — not in scope for this iteration

---

## Suggested Next Step

Pick an approach for discovery (likely B or a hybrid of B+A) and for execution (likely A or C), prototype the subprocess runner with streaming output into a TUI overlay, then wire up the Verify hub actions.

---

### Issue #45 [CLOSED]
**Title:** [Explore] In-tool Agent Prompting Integration
**URL:** https://github.com/VilfredSikker/easy-review/issues/45
**Description:**
## Objective
Investigate and design a way to integrate AI agent prompting directly into the easy-review TUI workflow.

## The Problem
Currently, users often perform a '2-step approach':
1. View diff/code in easy-review.
2. Switch context to an agent to ask for a review/fix.

## Vision
Building AI prompting as a first-class citizen in the tool.
- **Lightweight/Agnostic:** Ideally, a way to pipe selection or file context to a configurable command.
- **Integrated:** Deep integration where the TUI itself handles the interaction.

---

### Issue #46 [OPEN]
**Title:** [Feature] Theme Customization System
**URL:** https://github.com/VilfredSikker/easy-review/issues/46
**Description:**
## Objective
Allow users to override the default colors used in the TUI through a theme-config.

## Requirements
1. **Config File:** Support a theme section in the main config for color overrides.
2. **Dynamic Application:** The UI should read these values and apply them to standard components.
3. **Defaults:** Fall back to existing brand colors if no config exists.

---


## PULL REQUESTS

### PR #1 [CLOSED]
**Title:** add landingpage
**Branch:** `landing-page` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/1
**Description:**
(no description)

---

### PR #2 [CLOSED]
**Title:** Improve diff view contrast with full-width backgrounds
**Branch:** `ui-contrasts` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/2
**Description:**
## In plain terms

**The problem:** Diff line backgrounds (green for added, red for removed) only colored the text area, leaving the gutter and trailing space as dark background. The colors themselves were also too subtle — hard to distinguish at a glance.

**Why it matters:** The whole point of `er` is fast code review. If add/delete lines don&#39;t visually pop, scanning diffs is slow.

**The fix:** Three small changes — bolder background colors, backgrounds that stretch the full line width (gutter to right edge), and gutter line numbers that inherit the diff background color.

**TL;DR:** Green and red backgrounds are now wider and more vivid so diffs are easier to scan.

## Summary

- Bump `ADD_BG`, `DEL_BG`, `HUNK_BG` saturation in `styles.rs`
- Apply line-level `.style()` so diff backgrounds fill full width
- Gutter (line numbers) inherits add/delete/hunk background per line type
- Hunk header marker column gets `HUNK_BG` background

## Test plan

- [x] `cargo build` compiles cleanly
- [x] `cargo test` — all 3 tests pass
- [x] Visual check: run `er` on a branch with changes and confirm full-width colored bands for add/delete/hunk lines

---

### PR #3 [CLOSED]
**Title:** Add PR base hint, filter presets, and filtered reviewed count
**Branch:** `pr-hint-and-filter-reviewed` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/3
**Description:**
## In plain terms

**The problem:** Running `er` on a manually checked-out PR branch picks `main` as the base via the fallback chain, even if the PR targets `develop`. This shows a different (larger) diff than `er --pr N`. Also, the reviewed counter ignores active filters, so you can&#39;t track progress within a filtered subset of files.

**Why it matters:** Users expect the same diff whether they run `er` or `er --pr N` on the same branch. And when filtering to focus on e.g. `*.rs` files, the reviewed counter should reflect that scope.

**The fix:** Three improvements:
1. When `gh` is available and the current branch has an open PR targeting a different base, show a notification hint (no auto-switch)
2. Filter history overlay (`F` key) now includes built-in presets (frontend, backend, config, docs)
3. Status bar shows both filtered reviewed count (yellow) and total reviewed count (blue) when a filter is active

**TL;DR:** `er` now tells you when you&#39;re looking at the wrong diff base, offers quick filter presets, and tracks review progress per-filter.

## Changes

| File | Description |
|------|-------------|
| `src/github.rs` | Add `gh_pr_for_current_branch()` — silently checks if current branch has an open PR |
| `src/main.rs` | Call the new function after init, show hint if PR base differs from detected base |
| `src/app/filter.rs` | Add `FilterPreset` struct and `FILTER_PRESETS` constant |
| `src/app/state.rs` | Add `filtered_reviewed_count()`, wire presets into filter history overlay |
| `src/ui/overlay.rs` | Render presets section in filter history popup with separator |
| `src/ui/status_bar.rs` | Show `2/5 · 4/12 reviewed` (yellow=filtered, blue=total) when filter active |

## Test plan

- [x] `cargo build` — compiles clean
- [x] `cargo test` — all 204 tests pass
- [ ] Manual: check out a PR branch targeting non-main, run `er`, confirm hint in status bar
- [ ] Manual: run `er` on a branch with no PR — no hint, no error, no delay
- [ ] Manual: run `er --pr N` — no hint shown (already PR-aware)
- [ ] Manual: press `F` to see filter presets + history in overlay
- [ ] Manual: apply a filter, review some files, confirm yellow/blue counts update correctly

---

### PR #4 [CLOSED]
**Title:** Add Recent mode (key 4) — sort branch diff files by mtime
**Branch:** `claude/add-recent-diff-mode-xR529` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/4
**Description:**
New DiffMode::Recent shows the same branch diff but sorts files by
filesystem modification time (newest first), surfacing actively worked-on
files at the top. Shares diff data, AI state, and reviewed status with
Branch mode.

- Add DiffMode::Recent variant with git_mode() returning &#34;branch&#34;
- Sort files by mtime in refresh_diff_impl() for Recent mode
- Preserve file selection by path when switching between modes
- Show relative time column (e.g. &#34;2m ago&#34;) in file tree for Recent mode
- Add key 4 binding and RECENT tab in status bar
- Update all DiffMode::Branch checks to include Recent where appropriate

https://claude.ai/code/session_01UiN2bvcqY5j1CrVAMZZN48

---

### PR #5 [CLOSED]
**Title:** Add settings overlay UI with persistent configuration
**Branch:** `claude/add-settings-system-lkscI` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/5
**Description:**
## Summary
This PR adds a settings overlay UI that allows users to view and modify application configuration at runtime, with changes persisted to disk. Configuration is loaded from per-repo or global config files on startup.

## Key Changes

- **New configuration system** (`src/config.rs`):
  - `ErConfig` struct with feature flags, agent settings, and display options
  - Support for loading config from `.er-config.toml` (repo-local) or `~/.config/er/config.toml` (global)
  - `SettingsItem` enum to define different setting types (boolean toggles, number edits, string displays, section headers)
  - `settings_items()` function that builds the complete settings menu structure

- **Settings overlay UI** (`src/ui/settings.rs`):
  - Renders a centered popup with all configurable settings
  - Visual indicators for selection state and setting values
  - Help text showing keyboard shortcuts (j/k for navigation, Space/Enter to toggle, s to save, Esc to cancel)
  - Styled section headers and different rendering for each setting type

- **App state integration** (`src/app/state.rs`):
  - Added `config: ErConfig` field to `App` struct
  - New `OverlayData::Settings` variant to track settings overlay state with a saved config snapshot for cancel/revert
  - Methods: `open_settings()`, `settings_toggle()`, `settings_save()`, `settings_cancel()`
  - Navigation logic that skips section headers when moving up/down in settings list
  - Enter key handling that toggles boolean settings or saves on non-toggleable items

- **Input handling** (`src/main.rs`):
  - Dedicated keybinding handler for settings overlay with j/k/Space/Enter/s/Esc support
  - Remapped `Shift+S` from &#34;stage current hunk&#34; to &#34;open settings&#34; (stage hunk moved to `Ctrl+S`)

- **UI integration** (`src/ui/mod.rs`):
  - Settings overlay rendered separately from other overlays since it needs App access
  - Conditional rendering based on overlay type

- **Dependencies** (`Cargo.toml`):
  - Added `toml` for config file parsing
  - Added `dirs` for cross-platform config directory resolution

## Implementation Details

- Settings changes are held in memory and only persisted when explicitly saved (s key)
- Pressing Escape reverts to the snapshot taken when the overlay was opened
- Config is loaded once at app startup from the first tab&#39;s repo root
- Section headers are non-selectable and automatically skipped during navigation
- Boolean toggles show visual feedback with [x] or [ ] checkboxes

https://claude.ai/code/session_01RQvRJbaeRZhGDpcy9SVwbg

---

### PR #6 [CLOSED]
**Title:** Add watched files feature for git-ignored paths
**Branch:** `claude/watched-ignored-files-Qhm27` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/6
**Description:**
## Summary
This PR introduces a &#34;watched files&#34; feature that allows users to opt-in to viewing and tracking git-ignored files through a `.er-config.toml` configuration file. Users can monitor changes to ignored files using either content mode (full file display) or snapshot mode (diff against a saved snapshot).

## Key Changes

- **Configuration System**: Added `ErConfig` and `WatchedConfig` structs to parse `.er-config.toml` with glob patterns for watched paths and diff mode selection
- **Watched Files Discovery**: Implemented `discover_watched_files()` to find files matching configured glob patterns, with gitignore status verification
- **Snapshot Diffing**: Added snapshot-based diffing via `save_snapshot()` and `diff_watched_file_snapshot()` using `git diff --no-index`
- **File Content Reading**: Implemented `read_watched_file_content()` with binary detection and UTF-8 validation
- **UI Integration**:
  - Added watched files section to file tree with visual separator and relative timestamps
  - Implemented dedicated diff view renderer for watched files with content/snapshot modes
  - Added watched file styling with distinct colors (cool blue) to differentiate from tracked files
  - Integrated watched file navigation into existing file selection logic (next/prev file)
- **State Management**: Extended `TabState` with watched file tracking, selection, and visibility toggles
- **User Controls**:
  - `W` key to toggle watched files section visibility
  - `s` key to update snapshots when viewing watched files in snapshot mode
  - Automatic watched file rescanning every ~5 seconds
- **Status Bar**: Added hints for watched files functionality when configured

## Implementation Details

- Watched files are sorted by modification time (most recent first)
- Files not in `.gitignore` are flagged with a warning indicator (⚠)
- Large files (&gt;10MB) and files with &gt;10,000 lines are truncated in content mode
- Snapshot diffs use unified format with 3-line context
- Navigation seamlessly transitions between diff files and watched files sections
- Configuration reloading and watched file refresh are available as public methods for future extensibility

https://claude.ai/code/session_01SxuZSfVJkJmhW7PmKBYNfT

---

### PR #7 [CLOSED]
**Title:** Mtime sort toggle, watch-by-default, and position persistence
**Branch:** `claude/add-recent-diff-mode-xR529` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/7
**Description:**
## In plain terms

**The problem** — Recent mode was a separate diff mode that duplicated Branch logic, watch mode had to be manually enabled, toggling sort or receiving watch events jumped your file selection, committing required leaving the TUI, and untracked files were invisible in the unstaged view.

**Why it matters** — When reviewing AI-generated code, the tool needs to stay out of your way. Losing your place, missing new files, or context-switching to commit breaks flow.

**The fix** — Shift+R toggles mtime sorting in any diff mode. Watch mode starts automatically and detects edits, staging, and commits. File/hunk/line position is preserved across all refreshes. Press c in Staged mode to commit inline. Untracked files now appear in Unstaged mode.

**TL;DR** — Sort by recency anywhere, watch just works, commit without leaving er, and new files actually show up.

## Changes

- **Mtime sort toggle** — `DiffMode::Recent` replaced with `sort_by_mtime` flag on `TabState`. `Shift+R` toggles it in any mode. File tree shows relative timestamps when active. Status bar shows `R RECENT` indicator.
- **Watch on by default** — File watcher starts automatically in `run_app()`. `w` still toggles.
- **Watch detects staging and commits** — Watcher allows `.git/index` and `.git/refs/` events through instead of filtering all `.git/` changes.
- **Position persistence** — `refresh_diff_impl` saves current file path, hunk, and line before re-parsing, then restores by path lookup. Covers watch events, manual reload, sort toggle, and mode switches.
- **Quick commit** — `c` in Staged mode opens an inline commit message input (green badge, Enter commits, Esc cancels). In other modes `c` still opens comment input. Added `git_commit()` to git/status.rs.
- **Untracked files in Unstaged mode** — Runs `git ls-files --others --exclude-standard` and appends synthetic unified diffs for each untracked file, so new files appear as Added with full content.
- **`R` hint in toolbar** — Shows `c commit` in Staged mode, `c comment` otherwise.
- **Docs updated** — CLAUDE.md and README.md reflect all changes.

## Test plan

- [ ] Run `er` — watch mode should be active immediately (no need to press `w`)
- [ ] Edit a file externally — diff should refresh, keeping current file selected
- [ ] `git add` a file — er detects the staging change
- [ ] `git commit` — er detects the commit and updates
- [ ] Press `Shift+R` — files sort by mtime with timestamps; press again to restore default
- [ ] Navigate to a specific hunk/line, press `r` — position preserved
- [ ] Switch to Unstaged mode (key 2) with untracked files present — they appear as Added
- [ ] In Staged mode, press `c` — commit input bar appears; type message, Enter commits
- [ ] In Branch mode, press `c` — comment input appears (unchanged behavior)

---

### PR #8 [CLOSED]
**Title:** enhance comment system: GitHub sync, replies, deletion, inline rendering
**Branch:** `claude/enhance-comment-system-F6ioh` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/8
**Description:**
Implement four major comment system improvements:

1. GitHub PR comment sync (pull/push):
   - Pull PR review comments via `gh api` on `G` key
   - Push local comments to GitHub with `P` key
   - Dedup via github_id, track sync state in .er-feedback.json
   - New fields: source, github_id, author, synced on FeedbackComment
   - GitHubSyncState struct for PR metadata

2. Single-level replies:
   - `r` key on focused comment starts reply (1 level only)
   - Block replies to replies (flat threading)
   - Indented rendering with ↳ prefix
   - replies_to() query method on AiState

3. Comment deletion with confirmation:
   - `d` key on focused comment enters confirm mode (y/n)
   - Cascade deletes replies when parent is deleted
   - GitHub API deletion for synced comments
   - Deletion rules: local always, GitHub only own comments

4. Context-aware comment rendering:
   - Line comments (line_start set) render inline after target line
   - Hunk comments (no line_start) render after hunk
   - `c` creates line comment, `C` creates hunk comment
   - Split comments_for_hunk() into comments_for_line() +
     comments_for_hunk_only()
   - Visual distinction: different bg colors for inline vs hunk

Also adds:
- Tab key for comment focus/navigation within hunks
- Arrow keys navigate between comments when focused
- R key toggles resolved status on focused comment
- Focus indicator (◆) and synced indicator (↑ synced)
- Author display for GitHub comments
- 10 new unit tests for query methods

https://claude.ai/code/session_01RgoqJ1GWaDrrpzSoqTQS3K

---

### PR #9 [CLOSED]
**Title:** Add commit history view mode (key 4)
**Branch:** `claude/add-commit-history-view-8Lj2Y` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/9
**Description:**
New DiffMode::History alongside Branch, Unstaged, and Staged that shows
the commit history of the current branch. Left panel displays a commit
list, right panel shows the full multi-file diff of the selected commit.

Features:
- Git log loading with commit metadata (hash, subject, author, stats)
- Commit diff rendering with file-sectioned view and syntax highlighting
- Navigation: j/k for commits, n/N for files within commit, arrows for lines
- LRU cache (5 entries) for recently viewed commit diffs
- Lazy loading: fetches 50 more commits when scrolling past end
- Search: filter commits by subject, hash, or author with /
- Merge commit detection with visual indicator
- Status bar shows &#34;4 HISTORY&#34; mode with commit info
- Edge cases: empty history, root commits, detached HEAD fallback

Files changed:
- src/git/status.rs: CommitInfo, git_log_branch(), git_diff_commit(), parsers
- src/app/state.rs: DiffMode::History, HistoryState, DiffCache, navigation
- src/ui/file_tree.rs: conditional commit list rendering
- src/ui/diff_view.rs: multi-file commit diff renderer
- src/ui/status_bar.rs: History mode label and adapted hints
- src/main.rs: key 4 binding, history-aware input routing

https://claude.ai/code/session_01V8QxYehENo1xkM31L26Frz

---

### PR #10 [CLOSED]
**Title:** feat: add large diff performance optimizations
**Branch:** `claude/large-diff-performance-7HSUQ` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/10
**Description:**
Implement comprehensive performance hardening for diffs with 500+ files
and 10,000+ lines:

**Auto-compaction (§1):**
- Pattern-based compaction for lock files, generated code, minified assets
- Size threshold (500 lines) auto-compacts large file diffs
- Compacted files show summary with add/del counts, expandable via Enter
- Memory freed by clearing hunks on compacted files

**Deduplicated git calls &amp; debounced refresh (§5):**
- Reduce git diff calls from 3 to max 2 per refresh by reusing raw output
- Debounce file watcher refreshes (200ms) to batch rapid changes
- Fast hash (DefaultHasher) for internal change detection on watch events
- SHA-256 preserved for .er-review.json compatibility

**Virtualized diff rendering (§2):**
- Only build Line objects for visible viewport + 20-line buffer
- Kicks in above 200 diff lines, early-exits past viewport
- Eliminates main rendering bottleneck for large files

**File tree virtualization (§4):**
- Only render ListItems visible in the terminal viewport
- Auto-scroll to keep selection centered
- Reduces 500 ListItem allocations to ~40 per frame

**Precomputed hunk offsets (§8):**
- O(1) scroll position lookup via cumulative line offset array
- Rebuilt on file selection and refresh
- Replaces O(n) hunk iteration in scroll_to_current_hunk

**Two-phase lazy parsing (§3):**
- Header-only scan for diffs above 5000 lines
- On-demand file parsing when user navigates to a file
- Stores raw diff + byte offsets for instant re-parse
- Pattern-based compaction applied to headers in lazy mode

**Syntax highlighting cache (§2):**
- Hash-keyed cache (content + filename) avoids re-highlighting
- 10K entry cache with full eviction on overflow
- High hit rate since most lines don&#39;t change between frames

**Large file warnings (§6):**
- Files above 2000 lines show warning indicator in title bar

**Memory budget tracking (§7):**
- Tracks parsed files, total lines, compacted file counts
- Displayed in status bar when ER_DEBUG=1 is set

All 181 tests pass (8 new tests added). Zero compiler warnings.

https://claude.ai/code/session_01HNWxbPE14Nmr4pDTcox7U4

---

### PR #11 [CLOSED]
**Title:** UI refactor: replace ViewMode with InlineLayers + PanelContent system
**Branch:** `agent-terminal-panel` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/11
**Description:**
## In plain terms

Right now the UI has 4 discrete view modes that you cycle through with `v`/`V`. Comments are only visible when AI data is loaded. There&#39;s no way to jump between comments across files.

After this PR: comments are always visible inline, mode switching is gone, and `J`/`K` let you jump between all comments across every file.

## What changed

### Core refactor: ViewMode → InlineLayers + PanelContent
- Replaced 4 discrete `ViewMode` variants (`Default`, `Overlay`, `SidePanel`, `AiReview`) with a composable system
- `InlineLayers` controls what appears inline in the diff (AI findings, comments, or both)
- `PanelContent` controls the right-side context panel (nothing, AI summary, PR overview)
- `a` toggles AI findings inline; `p` cycles the context panel
- `v`/`V` keys removed; `ai_panel.rs` and `ai_review_view.rs` deleted

### History diff mode merged
- Commit history view (`4` key) was a separate branch; merged in and fixed
- Sticky filename headers in history diff view
- Panel keys work in history mode

### Cross-file comment navigation
- `J`/`K` jump between all comments and questions globally, wrapping around
- `all_comments_ordered()` and `all_questions_ordered()` on `AiState` return sorted navigation tuples
- Focused comment gets brighter background, `▸` prefix, bold `◆ focused` badge

### Bug fixes
- Panel scrolling fixed
- Focus indicator rendering corrected
- Jump feedback improved
- Lazy-mode file parsing on navigation
- `Highlighter` mutability after history view merge

## Files changed

| File | Change |
|------|--------|
| `src/ai/review.rs` | Added `all_comments_ordered()`, `all_questions_ordered()`, updated `InlineLayers`/`PanelContent` types |
| `src/app/state.rs` | New navigation state, J/K handlers, composable layer tracking |
| `src/main.rs` | Input routing for new keybindings |
| `src/ui/diff_view.rs` | Inline rendering for all comment types, focus visuals, history diff |
| `src/ui/file_tree.rs` | Commit list rendering in history mode |
| `src/ui/status_bar.rs` | Updated hints for new keybindings |
| `src/ui/styles.rs` | Focus highlight styles, removed unused constants |
| `src/ui/mod.rs` | Layout dispatch for new panel system |
| `src/ui/ai_panel.rs` | Deleted (replaced by PanelContent) |
| `src/ui/ai_review_view.rs` | Deleted (replaced by InlineLayers) |

## Known follow-ups (not in this PR)
- `PrOverview` panel stub needs `github.rs` integration
- File tree comment indicators: counts instead of just diamonds
- AI badge cosmetics
- Bottom bar hint curation
- `comment_focus` legacy pattern cleanup

## Test plan
- [ ] `cargo build --release` passes
- [ ] 242 tests passing (`cargo test`)
- [ ] `a` key toggles AI findings inline
- [ ] `p` key cycles context panel
- [ ] `4` key shows commit history, diff renders with sticky filename header
- [ ] `J`/`K` navigate between comments across files, wrapping at boundaries
- [ ] Focused comment shows brighter background + `▸` prefix + bold badge
- [ ] Comments visible without AI data loaded

---

### PR #13 [CLOSED]
**Title:** Add CI workflow for build, test, lint, and format checks
**Branch:** `claude/setup-ci-checks-NE60R` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/13
**Description:**
Runs on push/PR to main/master with four parallel jobs:
- cargo check (compilation)
- cargo test (unit tests)
- cargo clippy (lint, warnings as errors)
- cargo fmt (format check)

https://claude.ai/code/session_012JerVcASPnNwPefNzhC4ZC

---

### PR #14 [CLOSED]
**Title:** fix all high-risk TODOs: panics, path traversal, and data loss
**Branch:** `claude/search-todo-fix-edLdG` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/14
**Description:**
- overlay.rs: use char-aware truncation to prevent mid-codepoint slice panic on non-ASCII paths
- state.rs: use saturating_sub and .get() to prevent underflow panic on empty hunks
- state.rs: clamp active_tab index in tab()/tab_mut() to prevent OOB panic
- diff_view.rs: bounds-check history.selected_commit and sticky header indices with .get()
- diff.rs: snap byte offsets to char boundaries in parse_file_at_offset to prevent UTF-8 panic
- config.rs: atomic config write via tmp file + rename to prevent data loss on crash
- status.rs: validate watched file paths stay within repo root to prevent path traversal
- main.rs: install panic hook to restore terminal (disable raw mode) on panic

https://claude.ai/code/session_011R45ZB9PLujYG4kxNdkcbs

---

### PR #15 [CLOSED]
**Title:** Fix CI formatting and clippy checks
**Branch:** `claude/fix-ci-formatting-checks-RnxSG` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/15
**Description:**
Apply rustfmt formatting across all source files and resolve all clippy
warnings: replace match-with-single-arm with if-let, use is_some_and
instead of map_or(false), collapse nested if statements, use
strip_prefix instead of manual slicing, replace match with matches!
macro, fix unused imports/assignments, and add targeted allow attributes
for type_complexity and field_reassign_with_default in tests.

https://claude.ai/code/session_01RjKPZAbxdqx4bC2QJTawqh

---

### PR #16 [CLOSED]
**Title:** Add test coverage analysis with prioritized improvement areas
**Branch:** `claude/analyze-test-coverage-P8sLN` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/16
**Description:**
Analyzes all 324 existing tests across the codebase and identifies
critical gaps: config.rs (0 tests), comment lifecycle in app/state.rs,
base branch detection, and GitHub comment sync. Proposes ~82 new tests
across 4 priority tiers.

https://claude.ai/code/session_011eLe5b48oSNmwPjvQ8igMz

---

### PR #17 [CLOSED]
**Title:** Hidden view mode, dynamic tabs, and history split diff
**Branch:** `hidden-view` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/17
**Description:**
## Summary

Right now `er` shows all mode tabs (Branch, Unstaged, Staged, History, Conflicts) regardless of whether they have content — you get dead tabs when the working tree is clean, when there&#39;s no merge, etc. Watched files are toggled via `W` as an embedded section, mixing them into the regular file list without a dedicated navigation context. Split diff doesn&#39;t work in History mode. All of this adds friction to the review flow.

This PR fixes that by making tabs content-aware, giving watched files their own mode, and extending split diff to History.

### What changed

- **Content-gated dynamic tabs** — tabs only appear when they have data. Unstaged/Staged disappear when the tree is clean, Conflicts when not merging, Hidden when no watched files are configured. Number keys (`1`-`9`) dynamically map to visible tabs instead of fixed positions.
- **Hidden mode** (`DiffMode::Hidden`) — a dedicated full-screen view for watched files. No git diff runs; watched files are the entire content. Has its own input handling (j/k navigation, `/` search), its own file tree renderer, and its own status bar hints.
- **History split diff** — side-by-side view for commit diffs with synchronized scrolling, sticky file headers, and file N/M indicators. Falls back to unified view on narrow terminals (&lt;60 cols).
- **Config path fix** — `load_config`/`save_config` now use `~/.config/er/` consistently (was hitting `~/Library/Application Support/` on macOS via `dirs::config_dir()`).
- **New feature flags** — `view_hidden` and `watched_in_all_tabs` in settings overlay.
- **Syntax highlight improvements** and AI loader enhancements.
- **Cleanup scripts** — `scripts/er-cleanup` and `scripts/er-cleanup-all`.

## Test plan

- [ ] `cargo check` passes (verified)
- [ ] Launch `er` in a repo with unstaged changes — verify Unstaged tab appears with dynamic numbering
- [ ] Stage all changes — verify Unstaged tab disappears, numbers re-adjust
- [ ] Configure `.er-config.toml` with watched paths — verify Hidden tab appears
- [ ] Press the Hidden tab number — verify full-screen watched files view with j/k navigation
- [ ] Enable split diff in settings, switch to History — verify side-by-side commit diff
- [ ] Narrow terminal below 60 cols in History split — verify fallback to unified
- [ ] Toggle `watched_in_all_tabs` in settings — verify watched files section shows/hides in non-Hidden modes

🤖 Generated with [Claude Code](https://claude.com/claude-code)

---

### PR #19 [CLOSED]
**Title:** Feature/v1.4 review quality
**Branch:** `feature/v1.4-review-quality` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/19
**Description:**
Description

---

### PR #20 [CLOSED]
**Title:** fixes across the app
**Branch:** `fixes` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/20
**Description:**
(no description)

---

### PR #31 [CLOSED]
**Title:** Add Modal Hub Architecture — consolidate keybinds into contextual popups
**Branch:** `claude/add-dark-mode-Xdiqy` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/31
**Description:**
Implement four modal hubs (g=Git, A=AI, v=Verify, ?=Help) that group
related actions into navigable popup menus, reducing keyspace pressure
and improving discoverability. Closes #30.

- Add HubKind, HubItem, HubAction types and OverlayData::ModalHub variant
- Git hub: push, stage, refresh, GitHub comment sync
- AI hub: copy context, toggle findings/comments/questions, cleanup
- Verify hub: placeholder items for tests/lint/typecheck/security
- Help hub: keybind reference organized by category
- Hub action dispatch routes selections to existing app methods
- Navigation: j/k to move, Enter to select, Esc to close

https://claude.ai/code/session_01LiZo5aJQD3pAPsJnXXozHJ

---

### PR #32 [CLOSED]
**Title:** Add review approval workflow with readiness checks
**Branch:** `claude/add-dark-mode-8kOR2` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/32
**Description:**
## Summary
Implements a complete review approval workflow that allows users to approve and ship code changes with configurable readiness checks. The feature includes approval state tracking, configuration options, and integration with git push and GitHub PR approval.

## Key Changes

- **New `ApprovalState` enum** (`state.rs`): Tracks three readiness states (NotReady, Partial, Ready) for the approval workflow
- **New `ApprovalConfig` struct** (`config.rs`): Configurable approval settings including:
  - `push_on_approve`: Push branch to remote on approval (default: true)
  - `gh_approve`: Approve PR on GitHub via `gh pr review --approve` (default: false)
  - `require_all_reviewed`: Require all files marked as reviewed (default: true)
  - `require_no_questions`: Require no unresolved personal questions (default: true)
  - `require_no_high_findings`: Require no high-severity AI findings (default: false)
  - `require_no_unresolved_comments`: Require no unresolved local GitHub comments (default: false)
- **New `approval_readiness()` method** (`state.rs`): Computes approval readiness by checking four conditions and returns state plus met/total check counts
- **New `ConfirmAction::Approve` variant** (`state.rs`): Represents the approval action in the confirmation workflow
- **New `execute_approval()` function** (`main.rs`): Executes the approval workflow (push and/or GitHub PR approval) with error handling
- **New `gh_pr_approve()` function** (`github.rs`): Wraps `gh pr review --approve` CLI command
- **UI enhancements** (`status_bar.rs`):
  - Approval readiness indicator in top bar showing NOT READY/PARTIAL/READY status with color coding
  - New &#34;g&#34; keybinding hint for approval action
  - Confirmation prompt for approval action in bottom bar
- **Settings integration** (`config.rs`): Added approval section to settings UI with toggles for all approval configuration options

## Implementation Details

- The approval readiness check is non-blocking: if not all checks pass, users receive a notification showing progress (e.g., &#34;2/4 checks passed&#34;) but can still force approval via settings
- The workflow integrates with existing git and GitHub operations, reusing established patterns for error handling and user feedback
- Configuration defaults are conservative: only &#34;all reviewed&#34; and &#34;no questions&#34; checks are enabled by default
- The approval action is added to the normal input handler (KeyCode::Char(&#39;g&#39;)) and follows the existing confirm/cancel pattern

https://claude.ai/code/session_01S148KJbJ9qFQ7DjgeQayPb

---

### PR #33 [CLOSED]
**Title:** Add session persistence to restore review progress across restarts
**Branch:** `claude/move-er-files-folder-5pZ5O` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/33
**Description:**
## Summary
This PR adds session persistence functionality that allows users to resume their code review progress exactly where they left off, even after closing and reopening the application. The session state is automatically saved and restored based on a diff hash to ensure consistency.

## Key Changes
- **New `SessionState` struct** in `src/app/state.rs`: A serializable data structure that captures all relevant review state including:
  - Diff hash (SHA-256) for validation on restore
  - Navigation state (selected file, hunk, line, scroll positions)
  - Diff viewing mode and expanded files
  - Filter expression and history
  - View preferences (unreviewed-only, sort-by-mtime)
  - In-progress comment drafts with metadata

- **Session I/O methods** on `SessionState`:
  - `save()`: Atomically writes session to `.er/session.json` using tmp+rename pattern
  - `load()`: Reads and deserializes session from disk, returning `None` if missing or invalid

- **Session capture/restore on `TabState`**:
  - `capture_session()`: Converts current `TabState` into a `SessionState` for persistence
  - `restore_session()`: Restores state from disk if diff hash matches, with safety clamping for indices
  - `save_session()`: Convenience method to capture and persist current state

- **Auto-save integration in `src/main.rs`**:
  - Session is restored during app initialization (after filter application)
  - Debounced auto-save (~2 seconds) triggered after any key input
  - Explicit save on application quit

- **Export `SessionState`** in `src/app/mod.rs` for public API access

## Implementation Details
- Diff hash validation ensures sessions are only restored when reviewing the same diff, preventing stale state from being applied to different changes
- All numeric indices are clamped to current bounds during restore to handle cases where file/hunk counts may have changed
- Comment drafts are only restored if non-empty, preventing spurious empty state
- Atomic file writes (tmp+rename) prevent corruption if the app crashes during save
- Debounced saves reduce I/O overhead while keeping state reasonably fresh

https://claude.ai/code/session_015r26c3Gpu3yojamJbMxcNS

---

### PR #34 [CLOSED]
**Title:** Add comprehensive test coverage for config, UI, and state modules
**Branch:** `claude/fix-analysis-findings-Q8vGG` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/34
**Description:**
## Summary
This PR significantly expands test coverage across the codebase, adding 61 new tests across 6 modules. The focus is on critical gaps in configuration handling, UI utilities, and state management, bringing the total test count from 324 to 385 tests.

## Key Changes

### Configuration Module (`src/config.rs`) — 18 tests added
- **`deep_merge()` function**: 6 tests covering empty base/overlay, scalar replacement, recursive nested table merging (3+ levels deep), array replacement, and type mismatches
- **`load_config()` function**: 3 tests for missing files, partial TOML merging with defaults, and malformed TOML fallback behavior
- **Default values**: 4 tests verifying `FeatureFlags`, `DisplayConfig`, and `AgentConfig` defaults
- **`settings_items()` function**: 3 tests for item count, BoolToggle get/set closures, and section header ordering

### State Module (`src/app/state.rs`) — 11 tests added
- **Filter pipeline**: 5 tests covering search + filter interaction, unreviewed-only toggling, and snap-to-visible behavior with empty results
- **Comment lifecycle**: 5 tests for starting comments, submitting empty comments, editing comments, and handling empty comment lists
- **Filter history**: 2 tests for deduplication and 20-item cap enforcement

### Git Status Module (`src/git/status.rs`) — 7 tests added
- **Base branch detection**: 4 integration tests using temporary git repos to verify detection of `main`, `master`, and `develop` branches
- **Upstream branch parsing**: 3 unit tests for the `strip_upstream()` helper logic

### UI Panel Module (`src/ui/panel.rs`) — 13 tests added
- **`check_icon()` function**: 7 tests covering success, failure, cancelled, timed_out, skipped, unknown, and None status states
- **`review_state_style()` function**: 6 tests for APPROVED, CHANGES_REQUESTED, COMMENTED, DISMISSED, PENDING, and unknown review states

### UI Status Bar Module (`src/ui/status_bar.rs`) — 4 tests added
- **`spans_width()` function**: 2 tests for correct width calculation and empty spans
- **`pack_hint_lines()` function**: 2 tests for single-line and multi-line wrapping behavior
- **`Hint::width()` method**: 1 test verifying key + label width calculation

### UI Utils Module (`src/ui/utils.rs`) — 8 tests added
- **`centered_rect()` function**: 3 tests for centering in larger areas, clamping when popup exceeds bounds, and zero-size popups
- **`word_wrap()` function**: 5 existing tests (already present, now documented)
- **Deduplication**: Moved `centered_rect()` from `overlay.rs` and `settings.rs` to shared `utils.rs` to eliminate code duplication

### UI Diff View Module (`src/ui/diff_view.rs`) — 3 tests added
- **`format_size()` function**: 3 tests covering byte, kilobyte, and megabyte ranges with proper formatting

## Implementation Details

- **Temporary git repos**: Added `tempfile` dev-dependency for integration tests in `git/status.rs` that create real git repositories to test base branch detection
- **Test helpers**: Reused existing `make_file()`, `make_test_tab()`, and `make_test_app()` helpers for state tests
- **Code deduplication**: Extracted `centered_rect()` to `ui/utils.rs` and removed duplicate implementations from `overlay.rs` and `settings.rs`
- **Coverage tracking**: Updated `TEST_COVERAGE_ANALYSIS.md` to reflect new test counts and coverage improvements

## Test Coverage Improvements
- `config.rs`: 0 → 18 tests (now covered)
- `app/state.rs`: 101 → 112 tests (filter + comment lifecycle)
- `git/status.rs`: 18 → 25 tests (base branch detection)
-

https://claude.ai/code/session_01LrdMyWvrnBrfNmZewhzxNF

---

### PR #35 [OPEN]
**Title:** Add Archwatch integration for dependency graph visualization
**Branch:** `claude/add-dark-mode-slMRH` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/35
**Description:**
## Summary
This PR adds integration with Archwatch, a dependency graph visualization tool, allowing users to launch and interact with dependency graphs directly from the application with highlighted changed modules.

## Key Changes
- **New `archwatch` module** (`src/archwatch.rs`): Implements core Archwatch integration logic
  - `launch_archwatch()`: Main entry point that either updates a running Archwatch instance via WebSocket or spawns a new process
  - `try_websocket_update()`: Attempts to send highlight updates to an already-running Archwatch instance
  - Helper functions to build CLI arguments and risk-level query parameters from changed files
  - Comprehensive unit tests for argument and parameter building

- **Configuration support** (`src/config.rs`):
  - New `ArchwatchConfig` struct with configurable binary path, port, and auto-launch option
  - Sensible defaults: binary=&#34;archwatch&#34;, port=3210, auto_launch=false
  - Settings UI integration for Archwatch configuration

- **User interaction** (`src/main.rs`):
  - New keybinding `g` to manually launch Archwatch with highlighted changed files
  - Auto-launch support when loading a diff (if enabled in config)
  - Proper error handling and user notifications

- **Dependencies** (`Cargo.toml`):
  - Added `tungstenite` (v0.24) for WebSocket communication with running Archwatch instances

## Implementation Details
- The integration is non-blocking: if no Archwatch instance is running, a new process is spawned in the background
- WebSocket connectivity is checked with a 500ms timeout before attempting to send updates
- Risk levels from AI analysis are preserved and passed to Archwatch for visual differentiation
- Empty file lists are handled gracefully with informative messages
- All helper functions are thoroughly tested with unit tests

https://claude.ai/code/session_01YPiRmiF5iK649ff66yFGgj

---

### PR #36 [CLOSED]
**Title:** changes to settings, keymap hints
**Branch:** `changes-to-settings` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/36
**Description:**
(no description)

---

### PR #38 [CLOSED]
**Title:** fix scroll
**Branch:** `split-view-old-diff-on-new-files` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/38
**Description:**
(no description)

---

### PR #39 [CLOSED]
**Title:** Add background summary agent for generating diff summaries
**Branch:** `claude/update-er-file-paths-75GHc` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/39
**Description:**
## Summary
Adds a background agent system to generate `.er/summary.md` files containing concise markdown summaries of code changes. The agent runs asynchronously in a spawned thread and can optionally push the summary to the GitHub PR body.

## Key Changes

- **New `SummaryAgentStatus` enum** in `state.rs` to track agent lifecycle (Running, Done, Failed)
- **Summary agent fields** added to `App` struct:
  - `summary_rx`: Channel receiver for agent completion
  - `summary_status`: Current status of the running agent
- **`spawn_summary_agent()` method** that:
  - Validates no agent is already running
  - Builds a templated prompt for the agent
  - Spawns a background thread to execute the configured command
  - Supports `summary.command` and `summary.args` config overrides
  - Optionally pushes generated summary to PR body via `gh pr edit`
- **`check_summary_agent()` method** called from event loop to poll for completion and handle success/failure states
- **New `SummaryConfig` struct** in `config.rs` with:
  - `command`: Optional override for agent command (defaults to `agent.command`)
  - `args`: Optional override for agent args (defaults to `agent.args`)
  - `push_to_pr`: Boolean flag to auto-push summary to PR body
- **`gh_pr_edit_body()` function** in `github.rs` to update PR body via `gh pr edit --body`
- **Keybinding `D`** to trigger summary generation from normal mode
- **Settings UI** to display and toggle summary configuration
- **Directory structure migration**: Updated documentation and code to use `.er/` directory structure (`.er/summary.md`, `.er/snapshots/`, etc.) instead of flat `.er-*` files
- **Event loop integration**: Added `check_summary_agent()` call in main event loop to poll agent status

## Implementation Details

- Agent runs in a separate thread with `std::sync::mpsc::channel()` for non-blocking communication
- Prompt includes base branch and output path information
- `.er/` directory is created automatically if it doesn&#39;t exist
- Agent stderr is captured and displayed in notifications on failure
- Successful completion triggers AI state reload to pick up the new summary
- Thread crash is detected via `TryRecvError::Disconnected`

https://claude.ai/code/session_01ViV3DvrQrykLDP9DWmgDYs

---

### PR #40 [CLOSED]
**Title:** Add per-file context line expansion/collapse with auto-expand
**Branch:** `claude/expand-lines-around-changes-7c99s` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/40
**Description:**
## Summary
This PR adds the ability to dynamically adjust the number of context lines shown in diffs on a per-file basis, with keyboard shortcuts (+/- keys) to expand and collapse context through predefined levels (3 → 10 → 25 → 50 → full). It also includes automatic context expansion for small files when selected.

## Key Changes

- **Per-file context overrides**: Added `context_overrides` HashMap to `TabState` to track custom context line counts per file path. Overrides are cleared when diffs are refreshed.

- **Context expansion/collapse methods**: 
  - `expand_context()`: Steps through increasing context levels (3 → 10 → 25 → 50 → 99999)
  - `collapse_context()`: Steps back through decreasing levels
  - `maybe_auto_expand_context()`: Automatically expands to full context for files with ≤ threshold diff lines

- **Git diff refactoring**: Modified `git_diff_raw_file()` to accept an optional `context_lines` parameter, allowing custom `--unified=N` values. Added `refetch_file_with_context()` to re-fetch a file&#39;s diff with a specific context level.

- **UI enhancements**:
  - Display context level in diff view title (e.g., &#34;[context: 10]&#34; or &#34;[full context]&#34;)
  - Show gap indicators between hunks with line counts (e.g., &#34;··· 42 lines hidden (+/- to expand) ···&#34;)
  - Added &#34;+/-&#34; hint to status bar

- **Configuration**: Added `auto_context_threshold` config option (default: 50 lines) to control automatic context expansion behavior.

- **Keyboard bindings**: 
  - `+` or `=`: Expand context for current file
  - `-`: Collapse context for current file
  - Auto-expand triggers on file navigation (j/k keys)

## Implementation Details

- Context steps are predefined as `[3, 10, 25, 50, 99999]` to provide reasonable progression
- History mode is excluded from context expansion (would require per-commit diff handling)
- Compacted files are automatically expanded before context adjustment
- Untracked files (Added status with synthetic diffs) skip context expansion
- Gap calculations between hunks account for line number differences to show accurate hidden line counts

https://claude.ai/code/session_01NW8Ubb1JNstkmhaNc47RJT

---

### PR #41 [CLOSED]
**Title:** feat: one-command install via curl + CI release workflow
**Branch:** `easy-install` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/41
**Description:**
## Summary

- **Release workflow** (`.github/workflows/release.yml`): Builds pre-built binaries for macOS x86, macOS ARM, and Linux x86_64 on `v*` tags. Creates a GitHub Release with tarballs attached.
- **Install script** (`install.sh`): Detects OS/arch, downloads the matching binary from GitHub Releases, installs to `~/.local/bin/`. Supports `--version` and `--dir` flags.
- **README**: Updated install section with curl (quick) and from-source methods.
- **Cargo.toml**: Added `license`, `repository`, `keywords`, `categories` for future crates.io publishing.
- **LICENSE**: MIT license file.

## Test plan

- [ ] Push a `v0.1.0` tag after merge → verify all 3 binaries appear on the GitHub Release page
- [ ] Run `bash install.sh` on macOS → verify `er --version` works
- [ ] Run `bash install.sh --help` locally (verified locally)

---

### PR #42 [CLOSED]
**Title:** Update marketing page and remove flaky tests
**Branch:** `update-marketing-page` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/42
**Description:**
## Summary
- Redesigned marketing page (`docs/index.html`) with showcase sections and animations
- Removed 4 flaky `detect_base_branch_*` tests from `src/git/status.rs` that were environment-dependent and blocking the pre-push hook
- Cleaned up unused test helpers (`init_temp_repo`, `init_temp_repo_with_branch`, `git_in`)
- Removed unused release workflow, install script, and LICENSE file

## Test plan
- [x] `cargo test` passes (383 tests, 0 failures)
- [x] Pre-push hook succeeds
- [ ] Verify marketing page renders correctly at the hosted URL

---

### PR #43 [CLOSED]
**Title:** Fix review navigation, panel improvements, delete watched files
**Branch:** `fix-review-navigation` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/43
**Description:**
## Summary
- Fix Ctrl+J/K finding navigation to scroll to the actual diff line instead of just the hunk header
- Add file-level findings support in the AI review data model
- Add PR overview panel with CI checks status and reviewer state
- Add delete watched files in Hidden mode (`d` key with y/n confirmation)
- Watch `.git/refs/` for commit detection, add `.work/` to gitignore
- Add `view_hidden` feature flag for the Hidden mode (key `6`)

## Test plan
- [x] `cargo test` — 390 tests pass
- [x] `cargo build` — clean compile
- [ ] Manual: `er` → navigate findings with Ctrl+J/K → verify scroll lands on the finding&#39;s line
- [ ] Manual: `er` → `6` → select watched file → `d` → confirm with `y` → file deleted
- [ ] Manual: `er` → open panel (`p`) → cycle to PR overview → verify CI checks and reviewers render

---

### PR #44 [CLOSED]
**Title:** Update README for first release
**Branch:** `first-release` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/44
**Description:**
## Summary
- Rewrites the README to be concise and product-focused for the first public release
- Replaces verbose problem/feature descriptions with a clean structure: install, usage, keybindings, and feature overview
- Adds badge shields (Rust, MIT license)

## Test plan
- [x] All 387 tests pass
- [ ] Verify README renders correctly on GitHub

---

### PR #47 [CLOSED]
**Title:** feat: no-checkout PR review
**Branch:** `no-checkout-pr-review` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/47
**Description:**
## In plain terms

**Problem:** Running `er --pr N` or `er ` calls `gh pr checkout`, which switches your local branch. If you have uncommitted work or just want to glance at a PR, this is disruptive.

**Why it matters:** Developers using AI coding tools often have work-in-progress on their current branch. Forcing a checkout to review a PR creates friction — you have to stash, switch, review, switch back, pop.

**The fix:** Fetch the PR&#39;s head commit to a local ref (`refs/er/pr/N/head`) and diff against it, leaving the working tree completely untouched. No branch switch, no stash needed.

**TL;DR:** Review any PR without leaving your current branch.

## Changes

- **`src/github.rs`** — Added `fetch_pr_head()` (fetches PR head to local ref) and `gh_pr_head_branch_name()`. Modified `gh_pr_overview()` to accept explicit PR number instead of auto-detecting from branch.
- **`src/git/status.rs`** — Added `head_ref: Option&lt;&amp;str&gt;` param to `git_diff_raw()` and `git_diff_raw_file()` so diffs can target the fetched ref instead of HEAD.
- **`src/git/diff.rs`** — Forwarded `head_ref` through `expand_compacted_file()` and `refetch_file_with_context()`.
- **`src/app/state.rs`** — Added `pr_head_ref` and `pr_number` fields to `TabState`. Replaced `gh_pr_checkout` with `fetch_pr_head` in PR URL handler. Threaded `pr_head_ref` through all diff call sites.
- **`src/main.rs`** — Updated `--pr` handler to fetch instead of checkout. Locked Unstaged/Staged mode switches (keys `2`/`3`) during PR review. Passed explicit `pr_number` to overview and comment sync functions.
- **`src/ui/status_bar.rs`** — Shows `[PR #N]` indicator in cyan when reviewing a PR.
- **`src/ui/panel.rs`** — Fixed UTF-8 string offset for `&#34; → &#34;` arrow in path display.

## Test plan

- [ ] `er --pr ` from a different branch — diff shows correctly, `git branch` confirms no checkout happened
- [ ] `er ` — same verification
- [ ] Keys `2`/`3` (Unstaged/Staged) are no-ops during PR review
- [ ] Comment pull/push (`G`/`P`) works against the correct PR
- [ ] Compacted file expand (`Enter`) fetches correctly using PR ref
- [ ] Plain `er` (no PR) works identically to before
- [ ] `cargo test` — 394 tests pass

---

### PR #48 [CLOSED]
**Title:** feat: theme system + no-checkout PR review
**Branch:** `new-design-update-and-themes` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/48
**Description:**
## Summary

- **Theme system**: Dynamic color theming with 4 built-in presets (Ocean Depth, Moonlight, Daybreak, High Contrast). All 31 color constants refactored to read from a global `Theme` struct. Live switching via settings overlay (`S` key), persists in `.er-config.toml`.
- **No-checkout PR review**: Merged from `no-checkout-pr-review` — review PRs via `--pr N` without switching branches (fetches PR head ref), plus `--remote` flag for reviewing PRs without a local clone.
- **Design context**: Added `.impeccable.md` with semantic color token structure and design principles for future UI work.

## Test plan

- [x] 415 tests pass (`cargo test`)
- [x] Clippy clean (`cargo clippy`)
- [ ] Manual: launch `er`, press `S`, cycle Theme setting, verify colors update live
- [ ] Manual: restart `er`, verify theme persists from config
- [ ] Manual: test `er --pr ` reviews without branch checkout
- [ ] Manual: test `er --remote ` for remote review

---

### PR #49 [OPEN]
**Title:** Improve file watcher debouncing and error handling for state persistence
**Branch:** `claude/fix-dashboard-reactivity-Qs6YC` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/49
**Description:**
## Summary
This PR improves the robustness of the application by enhancing file watcher debouncing logic, adding proper error reporting for disk write failures, and fixing state restoration to preserve hunk offset calculations.

## Key Changes

- **File watcher debouncing**: Implemented a dual-deadline system with both a soft deadline (200ms) and hard deadline (2000ms) to prevent excessive refresh delays during rapid file changes while ensuring timely updates
- **Error handling for disk writes**: Replaced silent error discarding with explicit error logging for relocated questions and comments, improving visibility into persistence failures
- **Finding focus preservation**: Modified `reload_ai_state()` to only clear focused finding IDs if they no longer exist in reloaded data, preserving user focus when possible
- **State restoration**: Added `rebuild_hunk_offsets()` call during session restoration to ensure hunk offset calculations are properly recomputed for restored file selections
- **Cache invalidation**: Added `filter_expr` field to `FileTreeCache` to properly track filter expression changes and invalidate cache when needed

## Implementation Details

- The file watcher now tracks both `refresh_deadline` (soft, 200ms) and `refresh_max_deadline` (hard, 2000ms) to balance responsiveness with batching efficiency
- Disk write errors are now reported to stderr with context about what operation failed
- Finding focus is preserved across AI state reloads by checking if the focused finding still exists in the reloaded data before clearing it
- Cache validation now includes filter expression comparison to ensure filtered file lists are properly regenerated when filters change

https://claude.ai/code/session_01P3o3RhdDmHuhET5cPQY3iU

---

### PR #50 [CLOSED]
**Title:** feat: guided AI agent prompting, Config Hub, and hardening
**Branch:** `claude/plan-issue-45-52KoH` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/50
**Description:**
## In plain terms

**The problem:** Running AI reviews from `er` required external Claude Code skill files installed separately. Settings were a simple toggle overlay with no way to edit strings or manage lists.

**Why it matters:** The external skill dependency made setup harder and the config UI couldn&#39;t handle the new agent settings (command path, args, prompt templates).

**The fix:** Embed prompt templates directly in the binary, replace the Settings overlay with a full Config Hub (inline editing, list management, local/global save), add a unified Open hub, and harden the codebase against shell injection, UTF-8 panics, and crash-inducing unwraps.

**TL;DR:** AI agent prompting works out of the box now, settings are fully editable, and 66 new tests cover the security-critical paths.

## Summary

- **Guided AI agent prompting** — Embedded prompt templates let the TUI invoke AI review/questions directly via the configured agent command, no external skill files needed
- **Config Hub** (`S` key) — Replaces Settings overlay with inline string editing, number cycling, watched path list management, descriptions, and local vs global save
- **Unified Open hub** (`o` key) — Browse folders, switch worktrees, open remote PRs by URL, open current PR in browser
- **Agent status badges** — Persistent top-bar indicator while agent commands run
- **Agent log streaming** — Real-time stdout/stderr display with stream-json parsing for Claude Code output
- **Security hardening** — Shell injection fix in `spawn_command`, narrowed `Bash(cp *)` to `Bash(cp .er/*)`, safe match guards replacing `unwrap()` panics
- **UTF-8 safety** — Fixed panics in `truncate_str`, cursor movement, path truncation, notification width
- **Robustness** — Atomic file writes, layout clamping for small terminals, zombie process fix, panel scroll caps
- **66 new tests** (412 → 478) covering `sanitize_for_shell`, `parse_stream_json_line`, `truncate_str` multi-byte, `config_hub_items`, `split_shell_args`

## Test plan

- [x] All 478 tests pass
- [x] `cargo clippy` clean
- [x] Command injection: branch names with shell metacharacters are safely quoted
- [x] History mode: no panics when `history` is None
- [x] Config Hub: string editing, number cycling, list add/delete, save local/global
- [x] UTF-8: emoji and CJK characters in truncation, cursor movement, path display
- [x] Agent args: quoted arguments preserved through config round-trip

Closes #45

---

### PR #51 [OPEN]
**Title:** fix dedup on PR status comments/approved
**Branch:** `claude/review-release-0.3-Plk1L` → `main`
**URL:** https://github.com/VilfredSikker/easy-review/pull/51
**Description:**
(no description)

---
