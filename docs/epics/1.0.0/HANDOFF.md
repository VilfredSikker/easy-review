# Epic 1.0.0: handover (2026-11-25, phase 5b done)

Phase 5b (the TUI on the service, ADDENDUM-07) is done on `release/v1.0.0`: P63 to P77 are merged and #647 to #661 are closed. The TUI runs on er-service alone, and er-engine has one half (no `app/`, no `ui` feature). The timeline is `coordinator/LEDGER.md`; the outcome and the phase-5b exit list are in BUILD_LOG.md.

## Open

- **Phase 6 (consolidate, #400)** is not planned. It starts with a gate and an addendum, as phases 3 to 5b did.
- **Bugs for `main`:** #760 (finding navigation steps onto findings the Gate hides) and #761 (a new question after a paused edit overwrites the edited row).
- **Flakes on release:** #762 (er-tui watched tests and `hub_keys_match_the_apps` under parallel load).
- **Feature Board:** the phase-5b tasks still need their final status (the owner applies these; see the session summary).

## How the last merges went

The coordinator ran locally (macOS) from 2026-11-25. The builder rules and `locked.sh` live in `~/.claude-worktrees/er-v1/`; item clones sit beside them; checks run through `~/.claude/scripts/epic-run.sh` and the machine-wide lock; merges use `gh pr merge --merge --match-head-commit <reviewed sha>`. The rest of the merge rules (stop line, no rebase, plants, rulings on the item issue) are unchanged; see the cloud handover in git history (`369df1f8f`).
