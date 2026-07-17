# Easy Review (Unreleased)

## In plain terms

- **What changed.** After an agent uploads triage/review/tour sidecars via MCP, there was no way to bookmark that PR or list what was already reviewed. New tools pin into Desktop Saved PRs and scan managed storage for uploaded artifacts. Separately, building the desktop app from a fresh clone used to fail with a cryptic `error: no such command: tauri` — the scripts now preflight the toolchain and the README lists prerequisites.
- **`easy-review-mcp` connects reliably.** `npx -y easy-review-mcp` often failed with "Failed to connect" — the launcher fetched the `er-mcp` binary from GitHub Releases with Node's `fetch()`, which stalled on the release redirect (curl pulled the same file in ~3s while `fetch()` hung 60–90s), blew Claude Code's 30s connection timeout, and left a broken half-cache (the tarball, but no extracted binary) that made every retry fail too. The binary now ships as a platform-specific npm package installed alongside the launcher, so it comes down through npm's robust, cached client instead of the flaky redirect fetch.
- **Why it matters.** You can find agent-reviewed PRs again from MCP (`list_pinned_prs` / `list_artifacts`) and see the same pins in the Desktop sidebar; new contributors get an actionable desktop setup path instead of a dead end; and the MCP connects reliably — instantly on repeat runs — instead of hanging.
- **TL;DR.** MCP pin/list reviewed artifacts, desktop build-from-source preflight, and a reliable `easy-review-mcp` install.

## What's Changed

### Features
- MCP `pin_pr` / `unpin_pr` / `list_pinned_prs` write Desktop Saved PRs (`projects.json` `saved_prs`) with Value-preserving updates (`er-engine::projects_pins`).
- MCP `list_artifacts` scans managed `prs/pr-*` buckets for uploaded triage/review/tour and marks whether each is pinned.
- `er-review` skill documents pin + find-reviewed-work flow.

### Fixes
- **`easy-review-mcp` connects reliably.** The npx launcher now resolves the prebuilt `er-mcp` binary from a platform-specific optional dependency (`easy-review-mcp-<os>-<arch>`) that npm installs from the registry, replacing the on-serve Node `fetch()` from GitHub Releases that stalled on the redirect and left a broken half-cache. The binary download moves onto npm's robust, cached client, and once installed the serve path does no network I/O at all. A hardened GitHub-download fallback (curl-first, with retries + a hard timeout) remains for hosts with no matching optional dependency. Bumps workspace + npm packages to 0.4.5.

---

# Easy Review v0.4.4

## In plain terms

- **What this is.** Easy Review (`er`) is a fast diff reviewer for people who work with AI coding tools — a terminal UI and a desktop app that share the same review engine.
- **What changed.** v0.4.4 ships Easy Review MCP (`er-mcp`) with an `npx` launcher and an `er-review` agent skill so external agents can triage/review PRs and upload sidecars into the same storage Desktop/TUI already read. It also fixes AI agent access to managed review storage, Codex card AI prompts, and the Cursor Grok model slug.
- **Why it matters.** Agents can run “ER review” end-to-end without a custom integration, and AI Hub actions against managed storage and Cursor Grok behave reliably.
- **TL;DR.** MCP server + npm launcher + review skill, plus agent storage and Grok model fixes.

## Highlights

- **Easy Review MCP (`er-mcp`).** Stdio MCP server for PR queues and client-owned sidecar upload (`prepare_review` → author → `upload_artifacts`). Release CI ships `er-mcp-<triple>.tar.gz` assets alongside the TUI binary. (#143)
- **`easy-review-mcp` npm package.** `npx -y easy-review-mcp` downloads the platform binary from GitHub Releases on first run (`npm/er-mcp`). (#144)
- **`er-review` agent skill.** `npx skills add VilfredSikker/easy-review -s er-review` so agents can run “ER review” end-to-end. Source: `skills/er-review/SKILL.md`.
- **Safer AI agent managed storage.** Agents get `--add-dir` scoped to the active review/arena bucket instead of the global managed root; Codex Elaborate/Validate no longer receive Claude-only flags; session AI picks stay session-only. (#141)
- **Cursor Grok model slug.** Catalog and persisted configs that still used `grok-4.5` are repaired to `cursor-grok-4.5-high`. (#142)

## What's Changed

### Features
- Easy Review MCP stdio server for PR prepare/upload workflows (#143)
- `easy-review-mcp` npm launcher (`npx`) and release assets for `er-mcp` (#144)
- `er-review` agent skill for end-to-end MCP reviews
- Add Cursor Grok 4.5 to the AI Hub catalog (#142)

### Fixes
- Allow AI agents to use managed storage with scoped `--add-dir`; fix Codex card AI prompts and session AI selection (#141)
- Repair stale Cursor Grok `--model` slug to `cursor-grok-4.5-high`

## Contributors

- @VilfredSikker

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.4.3...v0.4.4

# Easy Review v0.4.3

## In plain terms

- **What this is.** Easy Review (`er`) is a fast diff reviewer for people who work with AI coding tools — a terminal UI and a desktop app that share the same review engine.
- **What changed.** v0.4.3 refreshes the AI Hub model catalog and makes model effort settings consistent across Desktop, TUI, action palettes, and Arena. It also fixes GPT reviews and makes the desktop worktree-copy action point at the actual linked checkout.
- **Why it matters.** Newer model choices are easier to configure, AI reviews behave more reliably, and copying a linked worktree now gives you the path you actually need.
- **TL;DR.** Refreshed AI models and effort controls, more reliable GPT reviews, and a linked-worktree path fix.

## AI Hub model refresh

- Refreshed the built-in catalog with Claude Fable 5, Opus 4.8, Sonnet 5, Haiku 4.5, the GPT-5.6 Sol/Terra/Luna family, GPT-5.5, GPT-5.4, GPT-5.4 Mini, GPT-5.3 Codex Spark, and Cursor Grok 4.5.
- Deprecated built-in entries are no longer advertised; deprecated Claude IDs are also removed from persisted model selections and reviewer assignments, while other user-defined models remain untouched.
- Added model metadata-driven effort/reasoning controls across Desktop, TUI, the action palette, and Arena. `Auto` omits provider overrides.
- Added atomic global persistence for provider, model, and effort selections, plus validation before Claude/Codex invocation.
- Added a triage-model picker in Desktop Settings, with a reset to the fastest available model.

## Highlights

- **Refreshed AI Hub models and effort controls.** The built-in catalog now includes the latest supported Claude and GPT families plus Cursor Grok 4.5, while deprecated built-in entries are no longer advertised. Effort and reasoning controls are metadata-driven and consistent across Desktop, TUI, the action palette, and Arena; `Auto` leaves provider-specific overrides unset.
- **Deprecated Claude selections are cleaned up.** Persisted selections for deprecated Claude models are removed, and a deprecated default automatically falls back to the current catalog default without affecting other custom models.
- **More reliable GPT reviews.** GPT review configuration and invocation paths now handle the refreshed model metadata and provider settings correctly.
- **Correct linked-worktree path copying (desktop).** The branch context bar now copies the filesystem path of the selected linked worktree rather than the project root.
- **Safer macOS DMG packaging.** The staged app is ad-hoc signed and stripped of quarantine metadata before the DMG is created, avoiding the harsher Gatekeeper “damaged” failure for unsigned bundles.

## What's Changed

### Features
- Refresh AI Hub models and effort controls across Desktop, TUI, and Arena (#137, #140)

### Fixes
- Fix GPT reviews
- Remove deprecated Claude models from persisted AI Hub selections
- Copy the selected linked worktree path from the desktop branch context bar (#138)
- Ad-hoc sign the app before bundling it into the release DMG

### Documentation
- Clarify that a notarized direct-download desktop DMG is not yet available.

## Contributors

- @VilfredSikker

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.4.2...v0.4.3

# Easy Review v0.4.2

## In plain terms

- **What this is.** Easy Review (`er`) is a fast diff reviewer for people who work with AI coding tools — a terminal UI and a desktop app that share the same review engine.
- **What changed.** v0.4.2 is a small follow-up to v0.4.1 with two fixes. In the desktop app, split-diff view now holds an exact 50/50 split with word wrap for long lines, instead of the widest line stretching the panel and forcing the whole diff to scroll sideways as one surface. Separately, a completed AI review no longer vanishes — showing "No findings written" with a stale "fresh" badge — when a model anchors a finding to a deleted line with a negative or invalid line number.
- **Why it matters.** A predictable split view for long lines, and AI reviews that reliably show their findings instead of silently disappearing.
- **TL;DR.** Exact 50/50 split view with word wrap (desktop), and a fix so completed AI reviews stop dropping their findings on a bad line anchor.

## Highlights

- **Fixed 50/50 split view + word wrap (desktop).** The diff row band is now always viewport-width, so split view holds an exact 50/50 no matter how long any line is, and unified view never exceeds the viewport. Long lines word-wrap by default (toggle in the diff view-settings dropdown) or pan horizontally inside their own panel — gutters stay pinned either way. (#136)
- **AI reviews no longer disappear on a negative line anchor.** The diff annotator tags deleted lines in a way that sometimes led a model to write a negative `line_start`, which failed the sidecar's parser and silently discarded the entire review. Invalid or negative anchors now degrade to a hunk-level anchor instead of failing the whole file, and a single malformed finding no longer takes the rest of the review down with it. (#135)

## What's Changed

### Features
- Exact 50/50 split view with word wrap and in-panel horizontal scroll for long lines (desktop) (#136)

### Fixes
- Don't drop an entire completed review when a finding has a negative or invalid line anchor (#135)

## Contributors

- @VilfredSikker

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.4.1...v0.4.2
