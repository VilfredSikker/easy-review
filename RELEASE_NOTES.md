# Easy Review v0.4.1

## In plain terms

- **What this is.** Easy Review (`er`) is a fast diff reviewer for people who work with AI coding tools — a terminal UI and a desktop app that share the same review engine.
- **What changed.** v0.4.1 is a focused follow-up to v0.4.0. The biggest win is invisible: reviewing a GitHub PR in the desktop app used to hammer GitHub with roughly **760–920 API calls per hour per open PR tab**; it now stays well under **~50**, so you're far less likely to trip GitHub's rate limits. On top of that, the AI **Guide** now tucks a file's tests/styles/stories directly under it, Triage's recommended files become a one-click review scope, and five nagging desktop bugs are fixed (Copy in inline threads, renamed files stuck on "Loading content…", discarded reviews on unrecognized branches, missing summary-only experts in the reviewer dropdown, and a Guide freeze after reviewing a nested file). The external `/er-*` Claude Code skills are gone — you now run every review action from the built-in **AI Hub**. The docs also got a full accuracy pass against the current keybindings and config.
- **Why it matters.** Much less GitHub rate-limit pressure, a tidier guided review, faster review scoping, fewer papercuts, and docs you can trust.
- **TL;DR.** A big cut in GitHub API traffic, a cleaner Guide, one-click triage scoping, five desktop fixes, reviews now run from the built-in AI Hub instead of external skills, and a docs accuracy pass.

## Highlights

- **Far less GitHub rate-limit pressure (desktop).** The three background loops behind an open PR tab now check whether the data they already have is still fresh before re-fetching, the 30s status refresh collapses a four-call burst into two subprocesses, a duplicated CI fetch is dropped, and a cold PR open no longer fetches the diff twice. Steady state for one idle PR tab falls from ~900 calls/hr to under ~50, with intentional (and documented) small staleness tradeoffs — manual Sync stays immediate and a real push is still detected within ≤60s. (#128)
- **Co-located tests in the Guide.** The AI Guide now nests a changed file's related files — its tests, styles, stories, and snapshots — directly beneath it in the pillar rail instead of scattering them across pillars or an "Other changes" bucket, so you read each file together with the tests that exercise it. Older tours keep loading unchanged. (#120)
- **Triage-recommended files as a quick select.** When Triage has flagged specific files to review, the file picker shows a **Triage (N)** quick-select and the AI Arena launcher shows a **Review N triage-recommended files** button — scoping a review to exactly those files in one click. The button only appears for a fresh triage against the current diff. (#116)
- **Reviews run from the built-in AI Hub.** The external `/er-*` Claude Code skills are removed; both apps already run the same review agents internally, so review, expert review, triage, and the professor now run from the AI Hub (<kbd>a</kbd> in the terminal, <kbd>Cmd</kbd>+<kbd>A</kbd> in desktop). The agent prompts are self-contained in the engine, so no binary, build, or bundle behavior changes. (#114)

## What's Changed

### Features
- Co-locate a file's tests / styles / stories / snapshots under it in the Guide (#120)
- Scope reviews to Triage-recommended files in one click (file picker + Arena) (#116)
- Remove the external `/er-*` skills; run every review action from the built-in AI Hub (#114)

### Fixes
- Copy now works on inline comments, findings, notes, and questions (desktop) (#112)
- Renamed / binary / mode-only files no longer stick on "Loading content…" in large diffs (desktop) (#125)
- Don't discard reviews whose head branch is reported as `unknown` or an unfilled placeholder (#130)
- Show experts that only contributed a summary (no findings) in the reviewer dropdown (#131)
- Stop the Guide view freezing after reviewing a nested (co-located) file (#132)

### Performance
- Slim GitHub `gh` CLI rate-limit pressure in the desktop background loops (~900 → under ~50 calls/hr per idle PR tab) (#128)

### Docs & internal
- Full accuracy pass on the guide, landing page, and README against the shipped keybindings and config (#129)
- Trim the README to a minimal quickstart, pointing the rest at the hosted guide (#133)
- Document desktop app availability and install steps (prebuilt `.dmg` + from-source) (#134)

## Contributors

- @VilfredSikker

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.4.0...v0.4.1
