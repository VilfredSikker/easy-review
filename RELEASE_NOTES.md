# Easy Review v0.4.0

## In plain terms

- **What this is.** Easy Review (`er`) is a fast diff reviewer for people who work with AI coding tools — a terminal UI and a desktop app that share the same review engine.
- **What changed.** v0.4.0 makes reviewing a PR feel instant and guided. Re-opening a PR you've already seen now paints from a local cache instead of phoning GitHub twice. A local PR tab cleanly separates "my branch" from "the PR diff" so review notes no longer bleed between the two. A new AI-guided **Guide** walks you through a diff one focus area at a time, and AI **Arena** reviews keep running in the background while you work elsewhere. Eight themes now drive both the terminal and desktop apps, and syntax colors stay readable on top of the green/red diff backgrounds.
- **Why it matters.** Less waiting, fewer mixed-up review artifacts, and a review surface you can actually read on any theme.
- **TL;DR.** Faster PR opens, cleaner per-view review state, an AI guide + background AI reviews, and a proper theme system.

## Highlights

- **Per-view review scoping (Local Branch vs PR Diff).** A local PR tab reviews two separate diffs — your local branch and the PR head-vs-base — and now keeps their review artifacts (triage, review, questions/notes, reviewed, checklist) split per view. GitHub PR comments stay shared across both views since they belong to the PR, and the Guide is per-view with identical-diff reuse so one generation serves both when the diffs match. (#102)
- **New theme system + app icon.** A token-driven theme system with eight themes — Graphite (default), Slate, Midnight, Ember, Paper, Daylight, Contrast Dark, Contrast Light — shared across the terminal and desktop apps, plus a new Review List app icon. (#89)
- **AI-guided Guide (tour) walkthrough.** A guided pillar-by-pillar review in both the TUI and desktop: sticky in-diff headers, mark-reviewed straight from the group rail, the full AI review action set inside Guide mode, and the tour attached to whichever diff you're viewing (PR vs local branch). (#93, #103, #105, #108)
- **Background AI Arena runs.** Arena reviews are now tab-independent — start a run and switch tabs; it keeps going in the background and lands when it's done. (#95)
- **Instant PR re-open (stale-while-revalidate).** The open-diff cache is persisted and trusted, and worktree PR metadata is cached in the snapshot build, so re-opening an unchanged PR renders from disk with no `gh` round-trips. A reliable "stale" pill lights when the PR head (or base branch) advanced on origin, and Sync re-saves so the next open is instant too.
- **Readability-corrected syntax on diff backgrounds (desktop).** Token colors are nudged — hue preserved — only as far as needed to clear contrast against the actual add/del/changed-word background, fixing faint comments and washed-out strings on light themes. (#107)
- **Quality-of-life.** Notes split from questions across desktop and TUI, with a Notes sidepanel (bulk Ask AI / Promote) and TUI Notes support; per-tab export buttons; GFM tables in desktop markdown; clickable bare URLs in PR descriptions; and relative "time ago" commit timestamps. (#86, #83, #92, #85, #84, #111, #110, #82)

## What's Changed

### Features
- Split review artifacts between PR Diff and local Branch views (#102)
- Adopt new theme system and Review List app icon (#89)
- Tour walkthrough: AI-guided pillar review (TUI + desktop) (#93)
- Guide view: sticky file headers + mark-reviewed from the group rail (#103)
- Attach the Guide to the viewed diff (PR vs local branch) (#105)
- Enable AI reviews in Guide (tour) mode (#108)
- Make AI arena runs tab-independent (background runs) (#95)
- Instant PR re-open: persist open-diff cache, cache worktree PR metadata, stale-while-revalidate with a reliable stale pill
- Split local notes from questions (#86)
- Notes sidepanel: bulk Ask AI / Promote, scrollable findings, parent-first preview (#83)
- Add Notes support to the TUI (q → Ctrl+t) (#92)
- Add comments/findings/questions visibility toggles to diff settings (#82)
- Add per-tab export buttons to the right panel (#85)
- Render GFM tables in desktop markdown (#84)
- Linkify bare URLs in PR description (desktop) (#111)
- Render commit timestamps as relative "time ago" in desktop (#110)

### Fixes
- Keep syntax tokens legible on diff highlight backgrounds (desktop) (#107)
- Make AI Review Arena follow the active theme (desktop) (#104)
- Force-fetch PR head so rebased PRs swap cleanly (#106)
- Fix finding validation failing for expert/professor findings (#99)
- Fix batch review submit on local branch with explicit PR number (#88)
- Show diff search bar below the sticky file header (desktop) (#87)
- Address v0.4.0 review findings

### Docs & internal
- Static documentation site for the terminal UI and desktop app (#81)
- Product visuals + landing-page Guide & AI Arena (#98)
- Add justfile task runner for build/run/test/lint commands (#91)
- Remove bottom panel (#90)
- Remove orphan code (#97)
- Cover anchor resolution and ISO timestamp formatting in tests (#96)

## Contributors

- @VilfredSikker

**Full Changelog**: https://github.com/VilfredSikker/easy-review/compare/v0.3.1...v0.4.0
