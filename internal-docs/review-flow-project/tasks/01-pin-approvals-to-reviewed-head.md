# 01 — Pin approvals to the reviewed head

Status: ready-for-agent
Idea: 1 in the original idea list (summarised in ../README.md)
Depends on: —
Impact: M · Complexity: L · Size: S
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — When you approve a PR from er, er tells GitHub "approve whatever the PR points at right now". If the agent pushed new commits after you last looked, your approval lands on code you never saw.
- **Why it matters** — er exists for agents that push faster than people read. An approval is the person saying "I read this". Today that claim can silently cover unread commits. er's merge button already guards against exactly this (ADR 0040); approve does not.
- **The fix** — er remembers which commit the diff on screen was built from. Approve sends that commit to GitHub, and refuses with a clear message if the PR has moved past it.
- **TL;DR** — An approval from er covers the commit the reviewer saw, never a later one.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> **An approval covers one specific commit.** If new commits land that the system can't vouch for, like an agent's push or a merge someone resolved by hand, the approval has to be given again.

And from its checklist: "Never let a bot's approval replace a required human's; dismiss approvals on new commits."

## Mission & definition of done
Every approval er sends (TUI Git hub "Approve PR", desktop GitHub card "Approve", desktop Comments card "Submit approval") is posted with `commit_id` set to the head the on-screen PR diff was built from. If GitHub's live PR head differs from that commit, er refuses before posting and says why. On a local-branch view, which has no PR head on screen, the live head is pinned and named, as ADR 0040 does for merge. COMMENT and REQUEST_CHANGES reviews keep today's behaviour.

- [ ] `er_engine::github::review_commit_id` exists, is pure, and its unit tests cover: approve with matching head, approve with moved head (error), approve with no reviewed head (live head), non-approve events (live head), empty live head (error).
- [ ] `gh_pr_submit_review` and `gh_pr_submit_review_remote` take an `expected_head: Option<&str>` and route the commit through `review_commit_id`.
- [ ] `TabState::pr_diff_head_oid()` exists in the engine; the desktop `snapshot.rs` function delegates to it; the existing test `pr_diff_head_oid_is_reported_only_while_the_pr_head_is_on_screen` passes unchanged.
- [ ] The TUI approve posts through `gh_pr_submit_review(_remote)` with event `APPROVE` and the tab's reviewed head. `gh_pr_approve` is deleted.
- [ ] Desktop `submit_github_pr_decision` and `submit_github_review` accept `expected_head: Option<String>`; the frontend passes the `pr_diff_head_oid` it rendered, captured before any refresh.
- [ ] The desktop Approve button is disabled with an explaining title when the PR head on GitHub differs from the diff on screen.
- [ ] ADR 0040 has one new Consequences bullet covering approvals.
- [ ] `just clippy`, `just lint-ui`, `just docs-check`, `just comments-check` exit 0.

**Risk:** ordinary. Touches a GitHub write path; a mistake either blocks approvals (visible, recoverable) or keeps today's unpinned behaviour.

## Before you start (orientation)
- **Branch:** this is the one task in the project that counts as a bug fix (an approval landing on commits nobody reviewed). Branch from `main`. See ../README.md "Branching".
- **Commands:**
  - Engine tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine <filter>`
  - TUI tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui <filter>`
  - Desktop backend tests: `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
  - Frontend: `~/.claude/scripts/locked.sh desktop-ui bun run test` and `~/.claude/scripts/locked.sh desktop-ui bun run check`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just lint-ui`, `~/.claude/scripts/locked.sh . just docs-check`, `~/.claude/scripts/locked.sh . just comments-check`
- **The map**
  - `crates/er-engine/src/github.rs` — `gh_pr_approve` (1301), `gh_pr_submit_review` (2280), `post_pr_review` (2375), `gh_pr_submit_review_remote` (2425), test module (3247).
  - `crates/er-engine/src/app/state/mod.rs` — `TabState.preview_head_oid` field (991), `TabState::is_remote` (2136). New method goes next to `is_remote`.
  - `crates/er-desktop/src/snapshot.rs` — `fn pr_diff_head_oid(tab: &TabState)` (905) and its test (4607).
  - `crates/er-desktop/src/commands.rs` — `submit_github_review` (3310), `submit_github_pr_decision` (3647), tests module (11113).
  - `crates/er-tui/src/input/mod.rs` — approve confirm handler (846–855), `push_comments_as_review` owner/repo block (1807–1840).
  - `crates/er-tui/src/ui/status_bar.rs:842` — approve confirm prompt text.
  - `desktop-ui/src/lib/components/BranchCard.svelte` — `submitAction` (86–101) and the Approve button (677–683).
  - `desktop-ui/src/lib/components/CommentsCard.svelte` — `submitReview` (96–107).
  - `desktop-ui/src/lib/prMergeBox.ts` — `mergeRequest` (307–320) and `mergeHeadNote` (338–365), the pattern to copy.
  - `docs/adr/0040-pr-actions-name-the-repo-and-pin-the-head.md` — one bullet added.
- **Conventions to follow** — the merge path already pins the head this way. Engine side (`crates/er-engine/src/gh_pr_actions.rs:66-70`):

  ```rust
  if head_oid.trim().is_empty() {
      anyhow::bail!(
          "Refusing to merge: the PR head commit is unknown. Refresh and try again."
      );
  }
  ```

  Frontend side (`desktop-ui/src/lib/prMergeBox.ts:318`):

  ```ts
  expected_head: prDiffHeadOid || github.head_oid || "",
  ```

  and the merge box reads the on-screen head from the snapshot (`desktop-ui/src/lib/components/PrMergeBox.svelte:30`):

  ```ts
  const prDiffHead = $derived(app.snapshot?.pr_diff_head_oid ?? null);
  ```

- **Mental model** — A PR Diff view or remote PR tab is built from a specific PR head commit; the engine stores it in `TabState.preview_head_oid` whenever it fetches the diff (`enter_pr_diff`, `apply_pr_refresh`, `new_remote`). The desktop exposes it to the frontend as `AppSnapshot.pr_diff_head_oid`, but only while a PR view is on screen (`snapshot.rs:905`). A refresh of the diff (`refetch_and_refresh_diff` → `apply_pr_refresh`, `crates/er-engine/src/app/state/mod.rs:2756-2760`) overwrites it with the newest head. Today every approval asks GitHub for the live `headRefOid` at click time and posts that (`github.rs:2289-2314`), or (TUI) runs `gh pr review --approve`, which has no commit flag at all (confirmed against `gh pr review --help`, gh 2.102.0).
- **ADRs to read first**
  - 0040 — merge is pinned to the on-screen head; this task applies the same reason to approval.
  - 0026 — all GitHub access goes through `gh`; no HTTP client.
  - 0006 — engine state is the UI contract; the reviewed-head rule belongs in the engine so the TUI gets it too.
  - 0015 — Tauri commands that shell out run off the main thread (`run_blocking`).

## Scope boundaries
- **In scope:** approval commit pinning in the engine, TUI and desktop; the disabled Approve button on a moved head; one ADR 0040 bullet.
- **OUT of scope / DO NOT TOUCH:**
  - COMMENT and REQUEST_CHANGES commit selection (they keep the live head; see Decision log).
  - Merge, update-branch and other PR actions in `gh_pr_actions.rs`.
  - The pr-head probe loop and `diff_stale` pill in `crates/er-desktop/src/main.rs` / `snapshot.rs:2701`.
  - The rate-limit work in `internal-docs/gh-rate-limit-slimming.md` (do not swap the `headRefOid` lookup for a cache read here).
  - Step 6 (stale-approval display) until a person says go.

## Steps

### Step 1 — Add the pure commit-choice function, test first
- **Goal:** one place decides which commit a review is posted against.
- **Why / invariant:** an APPROVE never posts a commit other than the one the reviewer saw; when the reviewer saw no PR head (local-branch view), the live head is used and that is a deliberate, named choice.
- **Files:** `crates/er-engine/src/github.rs` — add next to `pr_review_payload_json` (2338); tests in `mod tests` (3247).
- **Do this:**
  1. Write the tests first, at the end of `mod tests` in `github.rs`:

     ```rust
     #[test]
     fn review_commit_id_pins_approval_to_reviewed_head() {
         assert_eq!(review_commit_id("APPROVE", Some("abc"), "abc").unwrap(), "abc");
     }

     #[test]
     fn review_commit_id_refuses_approval_when_head_moved() {
         let err = review_commit_id("APPROVE", Some("aaaaaaa1"), "bbbbbbb2").unwrap_err();
         let msg = err.to_string();
         assert!(msg.contains("aaaaaaa"), "{msg}");
         assert!(msg.contains("bbbbbbb"), "{msg}");
     }

     #[test]
     fn review_commit_id_uses_live_head_without_reviewed_head() {
         assert_eq!(review_commit_id("APPROVE", None, "live").unwrap(), "live");
         assert_eq!(review_commit_id("APPROVE", Some("  "), "live").unwrap(), "live");
     }

     #[test]
     fn review_commit_id_leaves_other_events_on_live_head() {
         for event in ["COMMENT", "REQUEST_CHANGES"] {
             assert_eq!(review_commit_id(event, Some("old"), "live").unwrap(), "live");
         }
     }

     #[test]
     fn review_commit_id_refuses_empty_live_head() {
         assert!(review_commit_id("APPROVE", Some("abc"), "").is_err());
     }
     ```

  2. Run them; they fail to compile because the function does not exist. That is the red step.
  3. Add the function:

     ```rust
     /// Which commit a review is posted against. An approval covers the commit
     /// the reviewer saw (ADR 0040): if the PR moved past it, refuse instead of
     /// approving commits nobody read. Other events follow the live head so
     /// their inline anchors match what GitHub shows.
     pub fn review_commit_id(event: &str, expected_head: Option<&str>, live_head: &str) -> Result<String> {
         let live = live_head.trim();
         if live.is_empty() {
             anyhow::bail!("Failed to get HEAD SHA: empty output");
         }
         let expected = expected_head.map(str::trim).filter(|s| !s.is_empty());
         match (event, expected) {
             ("APPROVE", Some(seen)) if seen != live => anyhow::bail!(
                 "The PR moved since you reviewed it: you reviewed {}, GitHub now has {}. Sync the diff, read the new commits, then approve again.",
                 short_oid(seen),
                 short_oid(live)
             ),
             ("APPROVE", Some(seen)) => Ok(seen.to_string()),
             _ => Ok(live.to_string()),
         }
     }

     fn short_oid(oid: &str) -> &str {
         oid.get(..7).unwrap_or(oid)
     }
     ```

     At c96dc0f12 no `short_oid`-like helper exists in `crates/er-engine/src` (searched `fn short_oid`, `fn short_sha`, `get(..7)`).
- **Watch out for:** comparison is on full 40-char SHAs on both sides (`preview_head_oid` comes from `rev_parse_oid` or `headRefOid`, both full). Do not compare prefixes.
- **Verify:**
  - `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine review_commit_id` → expect `5 passed`.
  - Planted break (proves the test can fail; apply and revert in one command so nothing stays in the tree): `sed -i '' 's/if seen != live =>/if false =>/' crates/er-engine/src/github.rs && ~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine review_commit_id_refuses_approval_when_head_moved; git checkout -- crates/er-engine/src/github.rs` → expect the test to FAIL. Commit Step 1 before running this, because `git checkout --` also drops uncommitted edits; afterwards `git diff --quiet crates/er-engine/src/github.rs` exits 0.
- **If it fails:** fix the function, never the assertions.

### Step 2 — Route both submit functions through it
- **Goal:** every review post uses `review_commit_id`.
- **Why / invariant:** the live `headRefOid` lookup already happens before every post (`github.rs:2290`, `2434`), so the comparison costs no extra `gh` call.
- **Files:**
  - `crates/er-engine/src/github.rs:2280` — `pub fn gh_pr_submit_review(owner: &str, repo: &str, pr: u64, comments: &[ReviewBatchEntry], repo_root: &str, event: &str, body: &str) -> Result<()>`
  - `crates/er-engine/src/github.rs:2425` — `pub fn gh_pr_submit_review_remote(owner: &str, repo: &str, pr: u64, comments: &[ReviewBatchEntry], event: &str, body: &str) -> Result<()>`
- **Do this:** add a final parameter `expected_head: Option<&str>` to both. Replace the block that turns stdout into `commit_id` and checks it for empty:

  ```rust
  // before (2309-2314, same shape at 2454-2459)
  let commit_id = String::from_utf8_lossy(&sha_output.stdout)
      .trim()
      .to_string();
  if commit_id.is_empty() {
      anyhow::bail!("Failed to get HEAD SHA: empty output");
  }
  // after
  let commit_id = review_commit_id(
      event,
      expected_head,
      &String::from_utf8_lossy(&sha_output.stdout),
  )?;
  ```

  Then update every caller. Confirmed callers at c96dc0f12:
  - `crates/er-desktop/src/commands.rs:3576` and `:3585` (inside `submit_github_review`) — handled in Step 4; pass `None` for now so it compiles.
  - `crates/er-desktop/src/commands.rs:3693` and `:3695` (inside `submit_github_pr_decision`) — same.
  - `crates/er-tui/src/input/mod.rs:1893` and `:1895` (`push_comments_as_review`, event `COMMENT`) — pass `None`; stays that way.
  Search again before editing: `grep -rn "gh_pr_submit_review" crates`.
- **Watch out for:** at c96dc0f12 `crates/er-mcp` has no caller of `gh_pr_submit_review` or `gh_pr_approve`. If one has appeared, pass `None` and note it in Comments.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine github::` → expect all pass; `~/.claude/scripts/locked.sh . cargo check -p er-desktop` → expect no errors.
- **If it fails:** a compile error names a caller you missed; give it `None`.

### Step 3 — Move the reviewed-head rule into the engine
- **Goal:** both front ends read the reviewed head from one engine method.
- **Why / invariant:** ADR 0006. The rule "a PR head is on screen only in PR Diff or a remote tab" lives in desktop code today, so the TUI cannot use it. Behaviour must not change for the desktop.
- **Files:**
  - `crates/er-engine/src/app/state/mod.rs:2136` — `pub const fn is_remote(&self) -> bool` (add the new method right after it).
  - `crates/er-desktop/src/snapshot.rs:905-911`:

    ```rust
    fn pr_diff_head_oid(tab: &TabState) -> Option<String> {
        if tab.mode == DiffMode::PrDiff || tab.is_remote() {
            tab.preview_head_oid.clone().filter(|s| !s.is_empty())
        } else {
            None
        }
    }
    ```
- **Do this:**
  1. Add to `impl TabState` in `mod.rs`, keeping the desktop doc comment's reasoning (move the comment at `snapshot.rs:899-904` with it):

     ```rust
     /// Whether this view shows a PR head (PR Diff or a remote PR tab). A
     /// local-branch view shows the working tree, which has no PR head.
     pub fn shows_pr_head(&self) -> bool {
         self.mode == DiffMode::PrDiff || self.is_remote()
     }

     /// The PR head the on-screen diff was built from, while one is on screen.
     pub fn pr_diff_head_oid(&self) -> Option<String> {
         if self.shows_pr_head() {
             self.preview_head_oid.clone().filter(|s| !s.is_empty())
         } else {
             None
         }
     }
     ```
  2. Make `snapshot.rs`'s function a one-line delegate: `tab.pr_diff_head_oid()`. Keep the function and its name so the test at `snapshot.rs:4607` stays untouched.
  3. Add an engine test near the `is_remote_*` tests (`mod.rs:12425`), copying the three cases of the desktop test (PrDiff → Some, Branch → None, remote → Some) using `TabState::new_for_test(vec![])`.
- **Watch out for:** `DiffMode` is already in scope in `mod.rs` (45 uses of `DiffMode::PrDiff` at c96dc0f12).
- **Verify:**
  - `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_diff_head_oid` → expect the new test passes.
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop pr_diff_head_oid_is_reported_only_while_the_pr_head_is_on_screen` → expect `1 passed`.
- **If it fails:** if the desktop test fails, the delegate changed behaviour; diff it against the code quoted under this step's Files.

### Step 4 — Desktop: pass the reviewed head from the frontend
- **Goal:** the desktop approves the head the person saw, captured before any refresh.
- **Why / invariant:** both desktop approve paths refresh the diff before posting, and a refresh overwrites `preview_head_oid` with the newest head (`mod.rs:2759-2760`). Reading the head on the backend after that would pin a commit nobody reviewed. The value must come from the snapshot the person was looking at, exactly as merge does.
- **Files:**
  - `crates/er-desktop/src/commands.rs:3310` — `pub async fn submit_github_review(mode: String, summary: String, state: State<'_, AppState>) -> Result<AppSnapshot, String>`
  - `crates/er-desktop/src/commands.rs:3647` — `pub fn submit_github_pr_decision(mode: String, summary: String, state: State<AppState>) -> Result<AppSnapshot, String>`
  - `desktop-ui/src/lib/components/BranchCard.svelte:96` — `await app.cmd("submit_github_pr_decision", { mode, summary: body });`
  - `desktop-ui/src/lib/components/CommentsCard.svelte:100-102`:

    ```ts
    // Sync local diff with remote before bundling inline comments into the review.
    await app.cmd("force_refresh_diff", {});
    await app.cmd("submit_github_review", { mode: ghEvent, summary });
    ```
- **Do this:**
  0. Frontend source test first. Components here are tested by reading their source (`desktop-ui/src/lib/components/PrMergeBox.test.ts:1-8` already loads `BranchCard.svelte` as `card`). Add `desktop-ui/src/lib/components/ApprovalPin.test.ts` in the same style, asserting:
     - `BranchCard.svelte` sends `expectedHead` in its `submit_github_pr_decision` call and contains `mergeHeadNote(` (the second assertion goes green in Step 5.1);
     - in `CommentsCard.svelte`'s `submitReview`, the text `pr_diff_head_oid` appears before `force_refresh_diff` (slice the function body as `PrMergeBox.test.ts:28-31` does and compare `indexOf`), and the `submit_github_review` call sends `expectedHead`.
     Run `~/.claude/scripts/locked.sh desktop-ui bun test src/lib/components/ApprovalPin.test.ts` → expect it to fail (red). Sub-steps 4 and 5 make the CommentsCard and `expectedHead` assertions pass.
  1. Add a pure helper in `commands.rs` beside `own_pr_approval_error` (~592) and test it in `mod tests`:

     ```rust
     /// The reviewed head an approval is pinned to. A PR view with no known
     /// head cannot be approved: er would not know what the person read.
     fn approval_expected_head(
         event: &str,
         shows_pr_head: bool,
         expected_head: Option<String>,
     ) -> Result<Option<String>, String> {
         let expected = expected_head.filter(|s| !s.trim().is_empty());
         if event == "APPROVE" && shows_pr_head && expected.is_none() {
             return Err("er does not know which commit the diff on screen was built from. Sync the diff, then approve.".to_string());
         }
         Ok(expected)
     }
     ```

     Tests: APPROVE + PR view + None → Err; APPROVE + local view + None → Ok(None); APPROVE + PR view + Some("abc") → Ok(Some("abc")); COMMENT + PR view + None → Ok(None).
  2. Add `expected_head: Option<String>` as a parameter to both commands (before `state`). Tauri maps the frontend's `expectedHead` to it; the repo already relies on this (`desktop-ui/src/lib/components/ReplyActionBar.svelte:53` sends `threadId`).
  3. In each command, under the first lock that already reads the tab (`submit_github_review`: the APPROVE preflight block at 3336-3348, or a new short lock before the refresh at 3350; `submit_github_pr_decision`: the block at 3672-3690), compute `let expected = approval_expected_head(event, app.tab().shows_pr_head(), expected_head)?;`. Pass `expected.as_deref()` to every `gh_pr_submit_review(_remote)` call, including the decision-only 422 fallback in `submit_review` (3574-3595), which is a closure that captures it.
  4. Frontend, `BranchCard.svelte` `submitAction`: read `const expectedHead = app.snapshot?.pr_diff_head_oid ?? null;` at the top of the function and send `{ mode, summary: body, expectedHead }`.
  5. Frontend, `CommentsCard.svelte` `submitReview`: read `const expectedHead = app.snapshot?.pr_diff_head_oid ?? null;` **before** `force_refresh_diff`, then send `{ mode: ghEvent, summary, expectedHead }`.
- **Watch out for:** `submit_github_review`'s own `refetch_and_refresh_diff` (3351-3357) still runs; that is fine because the expected head arrived from the frontend. If the head moved, `review_commit_id` refuses and the error goes through `gh_review_submit_err` (`commands.rs:3283`), which prefixes every message with `GitHub review submission failed:`. That prefix is wrong here, since er refused and GitHub was never asked. Add an early return at the top of `gh_review_submit_err` that passes the message through unchanged when it starts with `The PR moved since you reviewed it`, and add a test beside `own_pr_approval_error_is_detected_from_github_422` (`commands.rs:11567`). The refusal is not a 422, so the decision-only fallback at 3597-3609 does not fire.
- **Verify:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop approval_expected_head` → expect `4 passed` (or however many cases you wrote, all passing).
  - `~/.claude/scripts/locked.sh desktop-ui bun run check` → expect 0 errors.
- **If it fails:** a Svelte type error on `pr_diff_head_oid` means the field name drifted; check `desktop-ui/src/lib/types.ts:587`.

### Step 5 — Desktop Approve button and TUI approve
- **Goal:** the person sees why approve is blocked before clicking; the TUI gets the same pin.
- **Why / invariant:** same reason as the merge box, which disables its confirm on a moved head (`mergeHeadNote`, `prMergeBox.ts:338`).
- **Files:**
  - `desktop-ui/src/lib/components/BranchCard.svelte:77-83` (`ownPrApprovalBlocked`, `canSubmit`) and `:677-683` (Approve button).
  - `crates/er-tui/src/input/mod.rs:846-855`:

    ```rust
    } else if action == InputMode::Confirm(ConfirmAction::ApprovePR) {
        app.input_mode = InputMode::Normal;
        let repo_root = app.tab().repo_root.clone();
        let remote = app.tab().remote_repo.clone();
        let pr = app.tab().pr_number;
        match er_engine::github::gh_pr_approve(&repo_root, remote.as_deref(), pr) {
            Ok(()) => app.notify("PR approved"),
            Err(e) => app.notify(&format!("Approve failed: {}", e)),
        }
    }
    ```
  - `crates/er-tui/src/input/mod.rs:1807-1840` — owner/repo/PR resolution inside `push_comments_as_review`.
  - `crates/er-tui/src/ui/status_bar.rs:842` — `ConfirmAction::ApprovePR => "Approve this PR on GitHub? (y/n)".to_string(),`
  - `crates/er-engine/src/github.rs:1299-1320` — `gh_pr_approve` (to delete).
- **Do this:**
  1. BranchCard: import `mergeHeadNote` from `$lib/prMergeBox`; add `const approveHeadNote = $derived(github ? mergeHeadNote(github, app.snapshot?.pr_diff_head_oid ?? null) : null);`; make `approve: !submitting && !ownPrApprovalBlocked && !approveHeadNote?.blocksMerge`; set the Approve button `title` to `approveHeadNote.text` when it blocks. Do not change `mergeHeadNote` itself.
  2. TUI: extract the owner/repo/PR block of `push_comments_as_review` (1813-1840) into `fn resolve_pr_target(app: &mut App) -> Option<(String, String, u64)>` that keeps its `notify` messages, and call it from both places.
  3. TUI approve handler: compute `let expected = app.tab().pr_diff_head_oid();`. If `app.tab().shows_pr_head()` and `expected` is `None`, notify "er does not know which commit the diff on screen was built from. Sync the diff, then approve." and return. Otherwise resolve the target and call `gh_pr_submit_review_remote(&owner, &repo, n, &[], "APPROVE", "", expected.as_deref())` when `app.tab().is_remote()`, else `gh_pr_submit_review(&owner, &repo, n, &[], &repo_root, "APPROVE", "", expected.as_deref())`. On success notify `PR approved at <short sha>` (use `expected`, or "the current GitHub head" when `None`).
  4. TUI prompt (`status_bar.rs:842`): when `app.tab().pr_diff_head_oid()` is `Some(h)`, show `Approve this PR at <first 7 of h> on GitHub? (y/n)`; else `Approve the PR's current GitHub head? (local branch view) (y/n)`. The renderer takes `&App`, and this is a cheap field read, so it stays within `crates/er-tui/src/ui/CLAUDE.md`'s rules.
  5. Delete `gh_pr_approve` (`github.rs:1299-1320`); confirm no caller remains with `grep -rn "gh_pr_approve" crates`.
  6. `ApprovalPin.test.ts` (written in Step 4.0) now passes, including its `mergeHeadNote(` assertion.
- **Watch out for:** the TUI hub item at `crates/er-engine/src/app/state/mod.rs:6566-6576` stays enabled on `pr_number.is_some()`; do not change it.
- **Verify:**
  - `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-tui` → expect all pass.
  - `~/.claude/scripts/locked.sh desktop-ui bun run test` → expect all pass.
  - Manual, desktop: open a PR you can approve in PR Diff view; push a commit to it from elsewhere; wait for the "PR head updated on origin" pill; the Approve button is disabled with the head note as its title. Click Sync; Approve is enabled again; approving succeeds and the review on GitHub shows the new head's SHA.
  - Manual, TUI: `er --pr <url>`, open the Git hub, choose "Approve PR"; the prompt names the 7-char head; `y` approves; the review on GitHub shows that SHA.
- **If it fails:** if GitHub rejects an approval whose `commit_id` equals the live head, stop and ask (see When to STOP).

### Step 6 — (separate, ask first) Show an approval as stale when the head moves
- **Goal:** the person sees that an approval they gave covers an older commit than the PR head, next to the existing "PR head updated on origin" pill.
- **Why / invariant:** the blog's rule is that an approval covers one commit. Steps 1–5 stop er from giving a wrong approval; this step shows when a right approval has gone out of date. It needs no new `gh` call: `reviews` is already in the status bundle query (`github.rs:2769`).
- **Do this only after a person approves the design in this step.** It is a separate commit, and may be a separate PR.
  - Engine: add `commit_oid: String` to `PrReview` (`github.rs:2578`), parsed in `parse_pr_reviews` (`github.rs:2650`) from `r["commit"]["oid"]`. `TODO(executor): confirm gh pr view --json reviews exposes commit.oid — run gh pr view <n> --json reviews --jq '.reviews[0]' against any PR with a review.` Extend `parse_pr_reviews_extracts_states` (`github.rs:3826`) with the field.
  - Desktop: add `commit_oid` to `GhReviewSummary` (`snapshot.rs:1365-1372`, filled at `commands.rs:743-752`). Show "Your approval covers <short>; the PR is now at <short>" when the signed-in user's latest APPROVED review (`state.gh_user`) has a `commit_oid` different from `github.head_oid`.
  - TUI: the Reviewers section in `crates/er-tui/src/ui/panel.rs` (starts at 1119) can mark the same case. At c96dc0f12 neither `crates/er-tui` nor `crates/er-engine` holds the signed-in GitHub login (no `gh_user` / `current_user`); the desktop keeps it in `AppState.gh_user`. `TODO(executor): decide with the person whether the TUI needs a login lookup (e.g. gh api user --jq .login, once per session) or shows every reviewer's stale approval instead.`
- **Verify:** parser test passes; manual check with an approval followed by a push.

## Gotchas & landmines
- **The refresh before submit.** `CommentsCard.svelte` calls `force_refresh_diff` and then `submit_github_review` calls `refetch_and_refresh_diff`; both move `preview_head_oid` to the newest head. Capture the expected head in the frontend first (Step 4.5). Reading it on the backend afterwards silently restores the bug.
- **GitHub accepts an older `commit_id`.** The REST review endpoint records a review against the commit you pass; it does not refuse the way `gh pr merge --match-head-commit` does. So er must do the refusal itself (Step 1). `TODO(executor): confirm in GitHub's REST docs for POST /repos/{owner}/{repo}/pulls/{pull_number}/reviews that an older commit_id is accepted.` If it turns out GitHub rejects a non-head `commit_id`, the refusal in Step 1 still holds; note it in Comments.
- **Race between check and post.** The live head is read, compared, then the review is posted. A push in that window yields an approval pinned to the reviewed commit, which is still correct: it covers what the person read.
- **Do not reuse `gh pr review --approve`.** It has no commit option (gh 2.102.0); that is why the TUI moves to the REST path.
- **`comments-check`** rejects comments that cite plan steps or tracker numbers. Write the reason, never "task 01".

## Decision log (forks pre-answered)
- **If you wonder whether to pin COMMENT and REQUEST_CHANGES too:** do not. Their inline comments are anchored against the refreshed diff, and pinning them to an older commit would bring back 422s on anchors. Only an approval claims "I read this".
- **If the view is a local-branch view (no PR head on screen):** pin the live head and say so in the prompt, because ADR 0040 does the same for merge and the working tree has no PR head to compare.
- **If the view is a PR view but the reviewed head is unknown** (`preview_head_oid` is `None`, which `new_remote` allows when the head moved during open, `mod.rs:1484-1488`): refuse and ask the person to Sync. Merge falls back to the live head here; approval refuses, because approval is a claim about what was read.
- **Where the frontend gets the head:** from `app.snapshot.pr_diff_head_oid` at click time, the same value the merge box uses. The backend validates it against the live head; it does not re-derive it.
- **ADR:** extend ADR 0040 with one Consequences bullet instead of a new ADR. The decision and its reason are the same as merge's. Suggested text: "Approvals are pinned the same way. An approval is posted with the on-screen head as its `commit_id`, and er refuses it when GitHub's head has moved, since GitHub records an approval on an older commit without complaint. Comments and change requests follow the live head so their inline anchors match."
- **Delete `gh_pr_approve`** once the TUI no longer calls it, since a second approve path without a pin is the bug this task removes.

## Full verification
1. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine review_commit_id` → 5 passed.
2. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine pr_diff_head_oid` → passes.
3. `~/.claude/scripts/locked.sh . cargo test -p er-desktop approval_expected_head` → passes.
4. `~/.claude/scripts/locked.sh . cargo test -p er-desktop pr_diff_head_oid_is_reported_only_while_the_pr_head_is_on_screen` → 1 passed.
5. `~/.claude/scripts/locked.sh . just test` → all pass.
6. `~/.claude/scripts/locked.sh . just test-desktop` → all pass.
7. `~/.claude/scripts/locked.sh desktop-ui bun run test` and `~/.claude/scripts/locked.sh desktop-ui bun run check` → all pass, 0 errors.
8. `~/.claude/scripts/locked.sh . just clippy` and `~/.claude/scripts/locked.sh . just lint-ui` → exit 0.
9. `~/.claude/scripts/locked.sh . just docs-check` and `~/.claude/scripts/locked.sh . just comments-check` → exit 0.
10. The two manual checks in Step 5.

In the final report, list every test added or changed: the five `review_commit_id_*` tests, the engine `pr_diff_head_oid` test, the `approval_expected_head` tests, the `gh_review_submit_err` pass-through test, `ApprovalPin.test.ts`, and (Step 6 only) the extended `parse_pr_reviews_extracts_states`.

## Rollback
Revert the commits. No sidecar or config format changes in Steps 1–5. Step 6 adds a field with a default to an in-memory and persisted status snapshot (`gh_status_cache.rs`); older caches load because the field defaults.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- GitHub rejects an approval whose `commit_id` equals the live head (would point at the REST payload shape; the pin itself is unchanged).
- Before starting Step 6.
- If you find another approve path (e.g. in er-mcp or a skill) that posts an APPROVE without going through `gh_pr_submit_review(_remote)`.

## Comments
