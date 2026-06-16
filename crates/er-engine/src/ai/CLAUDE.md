# ai/ — AI Review Integration

Data model and loader for AI-generated review sidecars. Sidecar files are
produced by external tools (Claude Code skills) or by agent subprocesses
spawned from `app/state/comments.rs`; this module reads and models that
output. Sidecars live in managed storage by default (`TabState::er_dir()`);
paths below use the repo-local `.er/` names, which are identical.

## Files

| File | Purpose |
|------|---------|
| `mod.rs` | Re-exports |
| `review.rs` | Data model: `AiState`, `ErReview`, `Finding`, `InlineLayers`, `PanelContent`, `CommentRef`, comment index |
| `loader.rs` | File I/O: loads sidecar JSON, computes diff hashes, mtime polling |
| `comments.rs` | Question/GitHub-comment storage types and persistence (atomic writes) |
| `prompts.rs` | Prompt templates for built-in agent spawns |
| `experts.rs` | Expert review files (`experts/*.json`) |
| `triage.rs` | Fast branch triage (`triage.json`) |
| `professor.rs` | Learning/teaching insights (`professor.json`) |
| `finding_cleanup.rs` / `finding_responses.rs` | Finding lifecycle: cleanup and AI responses |
| `relocate.rs` | Re-anchor findings/comments when the diff shifts |

## Sidecar Files

| File | Struct | Purpose |
|------|--------|---------|
| `review.json` | `ErReview` | Per-file risk levels, findings, suggestions |
| `order.json` | `ErOrder` | Suggested file review order with groupings |
| `summary.md` | (raw text) | Markdown summary of overall changes |
| `checklist.json` | `ErChecklist` | Review checklist items |
| `triage.json` | `TriageReview` | Fast branch scan / routing verdict |
| `professor.json` | — | Teaching insights |
| `experts/*.json` | — | Domain-specific expert findings |
| `questions.json` | `ErQuestions` | Personal review questions (written by `er`) |
| `notes.json` | `ErNotes` | Local actionable notes — private, agent hand-off oriented (written by `er`) |
| `github-comments.json` | `ErGitHubComments` | GitHub PR comments, two-way sync (written by `er`) |

## Key Types (review.rs)

**`AiState`** — Aggregate state for one tab: optional `review`, `order`,
`summary`, `agent_summaries`, `checklist`, `questions`, `github_comments`,
`triage`, legacy `feedback`, plus:
- `is_stale` — true if any sidecar's `diff_hash` differs from the current diff
- `stale_files` — per-file staleness set
- `comment_index` — lazily-built `CommentIndexData` for O(1) per-file comment lookup

**`InlineLayers`** — visibility toggles for inline annotation layers
(findings, questions, GitHub comments, hide-resolved). Replaced the old
`ViewMode` enum together with **`PanelContent`** (what the side panel shows:
`FileDetail | AiSummary | PrOverview | SymbolRefs | AgentLog`).

**`ErReview`** → `ErFileReview` → `Finding` — review contains per-file
reviews, each containing findings with severity, category, description,
suggestion, hunk references.

**`RiskLevel`** — `High | Medium | Low | Info` with display helpers.

**`CommentRef`** — unified query enum wrapping `ReviewQuestion` (as either a
`Question` or `Note` — notes reuse the `ReviewQuestion` shape and live in
`notes.json`), `GitHubReviewComment`, or legacy `FeedbackComment`.

## Loader (loader.rs)

- `compute_diff_hash(raw_diff)` — SHA-256 hex string via `sha2` (persisted in sidecars)
- Fast non-cryptographic hash for internal watch-mode change detection
- `load_ai_state(...)` — reads all sidecars, sets `is_stale` / `stale_files`
- mtime polling with a cache to limit `stat` calls during the event loop

## Important Patterns

- Global staleness (`is_stale`) dims the AI overlay; per-file staleness dims individual files/comments
- `AiState` preserves panel/review focus and cursor across reloads (handled by `TabState::reload_ai_state()`)
- `er` writes only `questions.json` and `github-comments.json`; all other sidecars are read-only from `er`'s perspective
- Findings link to hunks via `hunk_index: Option<usize>`, enabling inline display in the diff view
