# Epic 1.0.0 phase 5b: handover (2026-11-25)

The goal is to finish phase 5b (the TUI on the service, ADDENDUM-07) on `release/v1.0.0` of VilfredSikker/easy-review, then close the epic's bookkeeping. Phase 5 is done; its handover is in git history and its outcome in BUILD_LOG.md. The timeline of every merge and ruling is `coordinator/LEDGER.md`; read its last entries first.

## Where it stands
Merged into `release/v1.0.0`:
- **P65 to P75:** all done, and #649 to #659 are closed.
- **P76.1:** done. #748 to #758 freeze every old-versus-new parity check into delta goldens under `crates/er-tui/goldens/parity/`, rebuild the P63 suite on the flag path, and list every App dependency. #758's body holds the twin table and the App-dependency lists.
- **P77.1:** done (#727, #728).

Open:

| Item | PR | State |
|---|---|---|
| P76.2 (#660) | #759 `epic/p76-2-tui-switch`, head `900995ce`, 1,317 counted lines | CI green; an independent review is running. Its verdict will be posted on #759 as "Review (P76.2)". |
| P77.2 (#661) | not built | Deletes `app/` and the `ui` feature. Starts after #759 merges. |

## What to do next
1. **Land #759.**
   - Read "Review (P76.2)" on #759.
   - Send must-fixes to a builder. Delta-review logic changes; a test-only delta the coordinator may check itself.
   - Merge on a merge verdict plus green CI at the head.
   - Then close #660.
2. **Build P77.2 (#661).** Read the P77 ruling, issuecomment-7127443634, and the planner on #661. Carry-overs from earlier rulings:
   - remove `assert_desktop_saves_gated` (#660 ruling item 12);
   - bound the unbounded gate loop in `er-service/src/flow_tests.rs` (`deep_config`);
   - remove the `retired_summary` gate;
   - fix any stale App wording left in the ADRs (P77 keeps the App and D20 wording until then).
3. **Close the epic.**
   - Close #661.
   - Update STATUS.md, BUILD_LOG.md and the ledger.
   - Update the Feature Board (rules below).
   - Post a phase-5b summary on the phase issue #399.
4. **File the two bugs for `main`.** They are recorded in the ledger but were never filed:
   - TUI finding navigation ignores the Gate (on `main`; fixed on release by P72.1);
   - the App's `start_comment` never clears the draft's edit id. Repro: own question, J, Down, e, Tab, Down, q, text, Enter.

## How every merge has gone
- **Merge rule:** merge only on a merge verdict at the PR's head plus green CI. Use the GitHub MCP merge tool with `expectedHeadSha`; the gh CLI does not work in the cloud container.
- **Stacked PRs:** open each part against release only after the previous part merges, so CI runs. Build the next part stacked meanwhile.
- **Merging release in:** never rebase or force-push. Take the union of conflicting sides, and regenerate TypeScript with `just protocol-gen`.
  - A clean text merge can still leave `SOURCES`/`TEST_SOURCES` lengths or `FROZEN` wrong, so recount them by hand.
  - Hand resolutions get a delta review.
- **Stop line:** 1,500 counted lines (`git diff --shortstat -M base...head` insertions), with no waivers. A file moved onto an existing path counts as rewritten (ruling issuecomment-7157796922).
- **Rulings:** post them on the item's issue as "Coordinator ruling".
- **Shared builder rules:** `/home/user/er-v1/RULES.md`, and every check runs through `/home/user/er-v1/locked.sh <worktree> <cmd>`. Both live in the cloud container, not the repo.
  - The rules: no real `gh`, agent CLIs or clipboard in tests; plants committed first and reverted; tests never weakened.

## Goldens, briefly
- `ER_UPDATE_GOLDENS=1` records from the App leg. It is refused under CI.
- After P76.2 the goldens are the only record of the expected output, so a re-record must be reviewed as a screen change.
- Each frozen test holds `parity::guard()`, and its module must be in `FROZEN`. Every golden file must name a `#[test] fn`.
- Masks cover temp dirs, arena ids, clock times, agent-log offsets and the git `<age>`. Temp roots are written as `/tmp` before drawing, so the suite passes on macOS.

## Owner's standing rules
- Never touch `~/Library/Application Support/easy-review` or `/Applications/Easy Review.app`.
- No attribution or session lines in commits, PRs or comments.
- Banned in prose: `load-bearing`, `mint(ed)`, `rung(s)`, and the `not X, but Y` sentence shape.
- **Commits are signed.** If signing fails, say so in the PR body.
- **Tests:**
  - no test is weakened or skipped to go green;
  - any test change is stated up front in the PR;
  - each must-fix closes with a planted break shown failing.
- **Where fixes go:** bug fixes go to `main`; everything else goes to `release/v1.0.0`.
- **Escalate only important or out-of-plan decisions.**
- **Feature Board** (https://claude.ai/artifact/JfHRDzatazCcfG8dVrw1jd, `tasks` collection):
  - writes need `if_version` from a fresh get;
  - use status `in_progress` or `done` only;
  - never set `prUrl`.
