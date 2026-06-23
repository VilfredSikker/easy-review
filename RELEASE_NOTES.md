# Release Notes — v0.4.0

> Status: **in development**. This is the active release branch (`release/v0.4.0`).
> Per project policy, all feature / non-bug-fix work targets this branch (not `main`),
> and every change should add an entry below before merge.

## Highlights

_TBD — summarize the headline features once the release is cut._

## What's Changed

### Features
- PR-cache: persist the top-10 "My PRs" / "To Review" PRs for instant sidebar render and checkout (stale-while-revalidate). (#74)
- Preemptive "To Review" triage: background cheap-model triage scans for new non-draft PRs, surfaced in the Inbox. (#58)
- Clickable bare URLs in the PR description (desktop) — plain `https://…` links in the Description block now render as anchors that open in the system browser, not just markdown-style `[text](url)` links.

### Fixes
- _none yet_

### Docs / Internal
- Establish branching & release workflow: PRs to `main` are bug-fix-only; feature work ships on release branches with release notes.

## Contributors

- @VilfredSikker

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.3.1...v0.4.0
