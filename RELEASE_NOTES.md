# Release Notes — v0.4.0

> Status: **in development**. This is the active release branch (`release/v0.4.0`).
> Per project policy, all feature / non-bug-fix work targets this branch (not `main`),
> and every change should add an entry below before merge.

## Highlights

- **Per-view review scoping.** A local PR tab now reviews two separate diffs — the local branch and the PR head-vs-base — with review artifacts (triage, review, questions/notes, reviewed, checklist) split per view. GitHub PR comments stay shared across both views (they belong to the PR), and the Guide tour is per-view with identical-diff reuse so a single generation serves both when the diffs match.
- **Preemptive "To Review" triage.** A background cheap-model triage scans new non-draft PRs and surfaces them in the Inbox, so review-worthy PRs appear without a manual refresh.
- **Instant PR open (stale-while-revalidate).** The top-10 "My PRs" / "To Review" PRs are cached to disk, so opening a PR renders instantly from cache and revalidates against `gh` in the background. A reliable "stale" pill lights when the PR head (or base branch) advanced on origin.
- **Readability-corrected syntax on diff backgrounds (desktop).** Token colors are now nudged — hue preserved — only as far as needed to clear WCAG AA against the actual add/del/changed-word background, fixing faint comments and saturated strings on light themes. Replaces the hand-maintained hex table that only covered one-dark-pro.
- **AI reviews in Guide mode + Guide attached to the viewed diff.** The Guide tab now supports the full AI review action set, and the tour is generated against whichever diff you're viewing (PR vs local branch) — including a "Re-run guide" affordance when it drifts.
- **Quality-of-life.** Clickable bare URLs in PR descriptions; relative "time ago" commit timestamps; settings are global-only and persist instantly (fixes the theme occasionally resetting).

## What's Changed

### Features
- PR-cache: persist the top-10 "My PRs" / "To Review" PRs for instant sidebar render and checkout (stale-while-revalidate). (#74)
- Preemptive "To Review" triage: background cheap-model triage scans for new non-draft PRs, surfaced in the Inbox. (#58)
- Clickable bare URLs in the PR description (desktop) — plain `https://…` links in the Description block now render as anchors that open in the system browser, not just markdown-style `[text](url)` links.
- Per-view review scoping (Local Branch vs PR Diff): triage / review / questions / notes / reviewed / checklist split per view; GitHub PR comments shared between the Local Branch and PR Diff views; the Guide tour is per-view with identical-diff reuse. See `docs/release-notes/per-view-review-scoping.md`.

### Fixes
- Guide freshness: recompute the branch diff hash when a tour exists so a freshly generated Guide isn't shown stale; deterministic storage fallback when the PR bucket can't be resolved.
- Settings are now global-only and save instantly. Changing the theme (or any setting) in the desktop app persists immediately to `~/.config/er/config.toml`, so opening the AI picker, the Arena launcher, or a new tab no longer reverts it. Per-repo `.er-config.toml` config is no longer read or written (fixes the theme occasionally resetting on its own).

### Docs / Internal
- Establish branching & release workflow: PRs to `main` are bug-fix-only; feature work ships on release branches with release notes.

## Contributors

- @VilfredSikker

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.3.1...v0.4.0
