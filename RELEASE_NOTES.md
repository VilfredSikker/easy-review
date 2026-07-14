# Unreleased

## AI Hub model refresh

- Refreshed the built-in catalog with Claude Fable 5, Opus 4.8, Sonnet 5, Haiku 4.5, the GPT-5.6 Sol/Terra/Luna family, GPT-5.5, GPT-5.4, GPT-5.4 Mini, and GPT-5.3 Codex Spark.
- Deprecated built-in entries are no longer advertised; existing user-defined or persisted legacy entries remain untouched.
- Added model metadata-driven effort/reasoning controls across Desktop, TUI, the action palette, and Arena. `Auto` omits provider overrides.
- Added atomic global persistence for provider, model, and effort selections, plus validation before Claude/Codex invocation.
- Added a triage-model picker in Desktop Settings, with a reset to the fastest available model.

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
