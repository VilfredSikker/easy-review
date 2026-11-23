# Epic 1.0.0 phase 5: handover (2026-11-23)

The goal is **"implement phase 5 fully"**: finish every item of ADDENDUM-06 (desktop protocol) on `release/v1.0.0` of VilfredSikker/easy-review. Phase 5b (TUI, P65c onward) is out of scope.

## Where it stands
Merged into `release/v1.0.0` (head `7f7c08da7`):
- P50 to P59 and P61, all done;
- P60.1, done (#694–#696, #699, #700);
- P60.2a (#701, #702).

Open, in order:

| Item | PRs (stacked) | State |
|---|---|---|
| P60.2b (#619) | #703 `epic/p60c-jobs-triage-tasks` (b0ff2b625, base release) → #706 `…-runs` (80d2da1cd) → #707 `…-agents` (533817d07) | Built and pushed, **not reviewed**. The local finish run (`wf_0651ed27-f75`) was stopped by the owner on 2026-11-23 while it was still building #707. Its last six commits (eba165070…533817d07: validate and send-note-to-agent on the task host, the tick's agent duties dropped, the saved pick no longer copied into the App) are pushed. Gates were not confirmed on #707's head, so run them first. Read #619's split comment to check whether #707 is the last part. The last part must also remove the tick's `check_ai_files_changed` and the `tick.rs` scan exception (ruling on #620); add them to #707 or a fourth stacked PR if they are missing. Then review all three. |
| P62.1 (#621) | #704 `epic/p62a-types-barrel` (35bc1d4c2, base release) → #705 `epic/p62a-delete-old-store-layer` (e4b8a9dd7) | **Merge verdicts on both.** Held on purpose until P60.2b lands (plan order), then merge release in and land. |
| P62.1c | not built | Decided on #621: `TabRow` gains the four tab-row values (Working tab's live branch label, a local PR's head-branch label, `change_token`, a local PR's owner/repo). Then P60.2b's flag-off catch-up (adapters, `legacySnapshot` stops serving `agent_commands`, `agent_log`, `background_tasks`, `bg_loading`; run and triage commands routed), and `legacySnapshot.svelte.ts` and `poll` are deleted. Starts after P60.2b and P62.1 merge. Close #621 after it. |
| P62.2 (#622) | not built | Deletes the old desktop path and the flag. Last item, after P62.1c. |

## What to do next
1. Review #703, #706 and #707 (P60.2b) against the P60.2 entry in ADDENDUM-06 and the rulings on #619 and #620. Fix the must-fixes, then land bottom-up.
2. Merge release into #704 and #705, gate, read `git show --remerge-diff`, and land.
3. Build P62.1c, review, land.
4. Build P62.2, review, land. Close #398's phase-5 items, then update STATUS.md and BUILD_LOG.md.

## How every merge has gone
- **Verdict and CI:** merge only on a merge verdict at the PR's head plus green CI, with `gh pr merge N --merge --match-head-commit <full sha>`.
- **Stacked PRs:** after a base merges, retarget the next PR to release (`gh api -X PATCH repos/VilfredSikker/easy-review/pulls/N -f base=release/v1.0.0`), then close and reopen it so CI runs.
- **Head moved after the verdict:** if the new commits are only merges or the reviewed fix, read `git log`/`--remerge-diff` and land. Anything else gets a delta review.
- **Conflicts with release:** merge release in; never rebase, never force-push. Take the union of enums, match arms and test lists. Regenerate the generated TypeScript (`just protocol-gen`) instead of hand-merging it. Hand resolutions get a delta review.
- **Stop line:** 1,500 counted lines per PR (`git diff --shortstat -M base...head` insertions), with no waivers. To get under it, split into a stacked PR, or trim comments only.
- **Rulings:** post decisions on the item's issue as **Coordinator ruling**, then close each issue when its item is fully merged.

## Rulings so far (all on GitHub)
- #612: A9 tab switch accepted; a `diff(tab)` row cache is a follow-up.
- #614: A9 mark-reviewed accepted.
- #615: parity leaves out `relocated_at_hash` on unanchored notes in Conflicts and Hidden.
- #616: click-to-paint numbers are deferred to the release preview; add-question overrun accepted.
- #620: P60.2 removes `check_ai_files_changed`; the export rename goes through a `lib.rs` alias.
- #621: `legacySnapshot` stays in #705; P62.1c removes it.

The phase-5 exit list (P62.1/P62.2) carries:
- the `diff(tab)` row cache;
- the click-to-paint numbers;
- `agent.md` for stores, rewritten in P62.2;
- trimming `test-v1` (A12).

## Owner's standing rules
- Never touch `~/Library/Application Support/easy-review` or `/Applications/Easy Review.app`.
- No attribution or session lines in commits, PRs or comments.
- Banned in prose: `load-bearing`, `mint(ed)`, `rung(s)`, and the `not X, but Y` sentence shape.
- Commits are signed through 1Password. When signing fails, agents may commit unsigned and say so in the PR body; the owner accepted that.
- Tests: no test is weakened or skipped to go green; any test change is stated up front in the PR; each must-fix closes with a planted break shown failing.
- Bug fixes go to `main`; everything else goes to `release/v1.0.0`.
- Feature Board (https://claude.ai/artifact/JfHRDzatazCcfG8dVrw1jd, `tasks` collection): `er-80`…`er-100` = P50…P62.2.
  - Writes need `if_version` from a fresh get.
  - Use status `in_progress` or `done` only, never backlog/todo/planned, and never set `prUrl`: those fire board automations.
  - er-97 (P60.2) is in progress; er-99 (P62.1) is in progress; er-100 (P62.2) is not started.

## Coordinator files in the repo
`docs/epics/1.0.0/coordinator/` holds the copies the local session used:
- `LEDGER.md`: the full timeline;
- `epic-item-v2.js`: the workflow script, with build, finish and review modes; launch it with the Workflow tool, `scriptPath` and the args;
- `args-*.json`: the run briefs. Reuse their `extra` and `review_focus` text, with the local paths changed to your checkout.

## Local-only state (not reachable from a cloud session)
The local coordinator kept these on the owner's Mac under `~/.claude-worktrees/er-v1/`:
- per-item clones (`p60d` holds P60.2b, `p62a` holds P62.1);
- `coordinator-state/LEDGER.md` (full timeline), `epic-item-v2.js` (the workflow script) and the `args-*.json` run briefs;
- the `epic-run.sh`/`locked.sh` wrappers for heavy checks under a temp HOME.

A cloud session works from the GitHub branches above and runs the repo's own gates (`just fmt-check clippy protocol-check comments-check test test-desktop test-v1 test-all lint-ui test-ui check-ui`; `check-ui` has one known error in `src/main.ts`).

Also pending locally:
- a status-docs commit (STATUS.md and BUILD_LOG.md for P58.2 to P61) is staged in `epic-docs`, unsigned-blocked by 1Password;
- the bookkeeping for P60.2b and P62.1 still needs adding to it.
