# Remove the external `/er-*` skills — review from the built-in AI Hub

## What changed

Easy Review no longer ships the external `/er-*` Claude Code skills. Both apps already run
the same review agents internally through the **AI Hub**, so the docs now send users there
instead of telling them to type slash commands in a separate Claude Code pane:

- **Open the AI Hub** with <kbd>a</kbd> in the terminal app or <kbd>Cmd</kbd>+<kbd>A</kbd> in
  the desktop app, then run a review, a specialized (expert) review, triage, or the professor.
- **Removed** all 21 `/er-*` skill directories under `skills/`, `skills/README.md`, and the
  18 `.claude/commands/*.md` symlinks that exposed them as slash commands.
- **Repurposed** `docs/guide/skills.html` ("Claude Code Skills") into **"AI Hub Actions"** — a
  both-apps reference giving each action's TUI label, desktop label, and the sidecar file it
  writes, with the TUI/desktop asymmetries called out (e.g. the desktop has no standalone
  *Generate summary* or *Answer questions* action; the TUI cannot generate a tour; publish and
  Arena are not Hub actions).
- Rewrote the rest of the user docs (README, landing page, quick start, AI Review, concepts,
  storage, comments, GitHub, reviewing, TUI, troubleshooting) to use the AI Hub. While here,
  also corrected the install command (`cargo install --git … er-tui`) and the desktop release
  status (an Apple-Silicon `.dmg` ships as of v0.4.0).

## Why it's safe

The AI Hub's agent prompts are fully self-contained embedded strings in
`crates/er-engine/src/ai/prompts.rs` — there is **no** build-time or runtime dependency on
`skills/` (no `include_dir!`/`RustEmbed`, and the desktop Tauri bundle does not ship it).
Deleting the skills does not affect the binary, the build, CI, or the desktop bundle. The
internal design docs `skills/REVIEW_PHILOSOPHY.md`, `skills/REVIEW_RULES.md`, and
`skills/PROFESSOR_PHILOSOPHY.md` are **kept** (the prompts align with them and still link to
them). This change is documentation plus file deletions only — no application code changes.

## Known gaps (to be re-implemented later)

- **Quiz** has no in-app generator — quizzes come from the external TechProfessor integration
  or imported sidecars; the TUI Quiz mode (<kbd>8</kbd>) only renders an existing `quiz.json`.
- **Guided tour** generation is desktop-only; the TUI can navigate an existing tour but not
  generate one.

These were previously covered by the now-removed `/er-quiz`, `/er-quiz-review`, and
`/er-wizard`/`/er-tour` skills and can be re-added as built-in Hub actions later.

## Implementation

- Deleted `skills/er-*/`, `skills/README.md`, and `.claude/commands/er-*.md`.
- Rewrote 13 user-facing docs + `docs/guide/assets/docs.js` (nav title) and made two surgical
  `CLAUDE.md` edits (questions answered via the Hub / Notes panel; publish via the `P` key /
  desktop Comments card instead of `/er-questions` / `/er-publish`).
- Verified: no user-facing `/er-*` slash-command instructions remain in the live docs; the
  `skills.html` rename is consistent across title, heading, nav, and inbound links; an
  adversarial audit pass confirmed action labels match the real UI.
