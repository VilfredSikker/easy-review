# 04 — Resolve GitHub threads from er

Status: ready-for-agent
Idea: 10 in the original idea list (summarised in ../README.md)
Depends on: —
Impact: M · Complexity: L · Size: S
Anchors verified: 2026-10-17 at commit c96dc0f12. Re-check every `path:line` before editing; lines drift.

> **For the executor:** Build exactly what this document says, in order. It is self-contained; read `../README.md` once for project-wide rules. Where a step says **Why / invariant**, that is what must stay true: if reality doesn't match the script, preserve the invariant and **stop and ask rather than guess**. Do not refactor, rename, or "improve" anything not listed in a step.

## In plain terms
- **The problem** — In the desktop app, clicking Resolve on a GitHub comment thread only flips a flag in er's local file. GitHub never hears about it. Worse, the next background comment sync (every 45 seconds) copies GitHub's state back, so the thread quietly reopens in er too.
- **Why it matters** — "Every finding has to be answered" only works if answering in er shows up where the PR lives. Later tasks (a review that closes its own fixed findings, an "all answered" state) need er to be able to resolve a thread on GitHub.
- **The fix** — When the thread is a real GitHub review thread, er resolves it on GitHub first, then marks it resolved locally. Private questions and notes keep resolving locally only, as today.
- **TL;DR** — Resolve on a GitHub thread in er resolves it on GitHub.

## Source
From *How the Claude Code team does code review* (https://claude.dev/blog/solving-code-review-at-anthropic/):

> **Every finding has to be answered.** You either fix it or resolve the thread with a reason.

and from its checklist: "Add automated security review, and require every review thread resolved before merge."

## Mission & definition of done
The desktop `resolve_thread` command resolves the GitHub review thread (GraphQL `resolveReviewThread`, through `gh api graphql`) when the id names a root GitHub comment that has a `github_id`, and only then writes the local `resolved` flag. A failure leaves both GitHub and the local file unchanged and returns the error, so the optimistic paint rolls back. Questions, notes and unpushed local drafts behave exactly as today.

- [ ] `er_engine::github::gh_resolve_review_thread(owner, repo, pr, root_comment_id)` exists; its pure parsers have unit tests (thread found / not found / already resolved / next page; mutation response).
- [ ] `resolve_thread` in `crates/er-desktop/src/commands.rs` calls it for GitHub roots, outside the app lock, before the local write.
- [ ] A pushed-in-a-review comment with no `github_id` yet is refused with a message telling the person to pull GitHub comments first.
- [ ] After a successful resolve, the PR comment bundle cache is invalidated, so the next pull does not reopen the thread.
- [ ] No path can send a question or note id to GitHub (ADR 0007).
- [ ] `just clippy`, `just comments-check` exit 0; `just test` and `just test-desktop` pass.

**Risk:** ordinary. A new GitHub write; mistakes are visible (thread resolved or not) and reversible on GitHub.

## Before you start (orientation)
- **Branch:** feature. Use the current release branch; see ../README.md "Branching".
- **Commands:**
  - Engine tests: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine <filter>`
  - Desktop backend tests: `~/.claude/scripts/locked.sh . cargo test -p er-desktop <filter>`
  - Gates: `~/.claude/scripts/locked.sh . just clippy`, `~/.claude/scripts/locked.sh . just comments-check`, `~/.claude/scripts/locked.sh . just test`, `~/.claude/scripts/locked.sh . just test-desktop`
- **The map**
  - `crates/er-engine/src/github.rs` — "GraphQL review thread resolution" section (1863–2188): `ReviewThread` (1907), `gh_pr_review_threads` (1933), `invalidate_pr_comments_cache` (2104), `gh_pr_review_threads_remote` (2142), `parse_review_threads_response` (2166). Paginated query pattern in `gh_pr_thread_addressing_remote` (3176). Tests in `mod tests` (3247), fixture style in `parse_review_threads_response_extracts_resolved_and_outdated_state` (3434).
  - `crates/er-engine/src/gh_pr_actions.rs` — private `fn graphql(query, strings, typed, what)` (479) and `fn graphql_args` (488); test `graphql_keeps_names_as_strings_and_numbers_typed` (682).
  - `crates/er-desktop/src/commands.rs` — `mark_thread_resolved_in_files` (2805), `mark_thread_resolved_in_memory` (2852), `resolve_thread` (2872), `active_github_key` (549), `abort_wrong_view` (288); tests `resolve_thread_updates_question_file` (11577) and `resolve_thread_updates_github_comment_file` (11625).
  - `crates/er-engine/src/sync.rs:273-285` — where a pull copies GitHub's `isResolved` into each comment (the reason the local-only flag gets undone).
  - `crates/er-engine/src/ai/comments.rs:335` — `GitHubReviewComment` (`github_id: Option<u64>`, `synced`, `source`, `in_reply_to`, `resolved`).
  - `desktop-ui/src/lib/optimisticLocal.ts:498-503` (`buildResolve`) and `:1186-1189` (resolve rollback) — already present; no frontend change needed.
- **Conventions to follow** — GraphQL with variables, never string interpolation (`crates/er-engine/src/gh_pr_actions.rs:192-205`):

  ```rust
  const MERGE_QUEUE_QUERY: &str = "query($owner: String!, $name: String!, $branch: String!) { \
      repository(owner: $owner, name: $name) { mergeQueue(branch: $branch) { id } } }";
  ...
  let out = graphql(
      MERGE_QUEUE_QUERY,
      &[("owner", owner), ("name", repo), ("branch", base)],
      &[],
      "merge queue query failed",
  )?;
  ```

  Pagination loop shape to copy: `gh_pr_thread_addressing_remote` (`github.rs:3176-3239`, `PAGE_SIZE = 100`, `MAX_PAGES = 10`).
- **Mental model** — A GitHub comment in er is a `GitHubReviewComment` in the PR bucket's `github-comments.json`. A comment pulled from GitHub has `id = "gh-<databaseId>"`, `github_id = Some(databaseId)`, `source = "github"`. A local comment pushed one at a time gets its `github_id` back from the push; a local comment pushed inside a batch review has `synced = true` but no `github_id` until the next pull replaces it with the GitHub copy. Every pull rewrites GitHub entries from GitHub, taking `resolved` from the GraphQL `reviewThreads.isResolved` map (`sync.rs:273`). GitHub resolves threads (by thread node id, `PRRT_…`), never single comments. er does not fetch or store thread node ids today: the queries at `github.rs:1940` and `:2148` select `isResolved isOutdated comments { databaseId }` only.
- **ADRs to read first**
  - 0026 — all GitHub access is `gh`; GraphQL goes through `gh api graphql`.
  - 0007 — questions and notes are private by file; no new path may reach GitHub with them.
  - 0008 — threads are one level deep; only a root is a thread.
  - 0015 — Tauri commands that shell out run off the main thread; `resolve_thread` already uses `run_blocking`.

## Scope boundaries
- **In scope:** engine lookup + mutation; desktop `resolve_thread` calling it; cache invalidation; tests.
- **OUT of scope / DO NOT TOUCH:**
  - The TUI. It has no resolve action at c96dc0f12 (only `toggle_hide_resolved`, `crates/er-tui/src/input/normal.rs:440-443`). Do not add one here.
  - er-mcp `pr_feedback_resolve` (`crates/er-engine/src/pr_review_feedback.rs:497-520`), which resolves questions and notes only.
  - Unresolving a thread (no unresolve action exists anywhere).
  - Resolving "with a reason" or recording an outcome; that is task 03 / 09 territory.
  - The sync queries at `github.rs:1940` and `:2148`, `ReviewThreadState`, and `sync.rs`. Do not add the thread id to them.
  - The 45s comment-sync loop and the rate-limit plan in `internal-docs/gh-rate-limit-slimming.md`.

## Steps

### Step 1 — Expose the GraphQL helper inside the crate
- **Goal:** `github.rs` can run GraphQL with variables.
- **Why / invariant:** variables keep a repo name like `123` a string and keep thread ids and cursors out of the query text.
- **Files:** `crates/er-engine/src/gh_pr_actions.rs:479` — `fn graphql(query: &str, strings: &[(&str, &str)], typed: &[(&str, &str)], what: &str) -> Result<String>`
- **Do this:** change `fn graphql` to `pub(crate) fn graphql`. Nothing else in that file.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine graphql_keeps_names_as_strings_and_numbers_typed` → `1 passed`.
- **If it fails:** stop; this step should be a visibility change only.

### Step 2 — Pure parsers, test first
- **Goal:** find a thread's node id from its root comment, and read the mutation result.
- **Why / invariant:** the root comment of a GitHub thread is its first comment, and er's thread id is the root's id, so `comments(first: 1)` is enough to match `github_id`. Fetching one comment per thread also keeps each page small.
- **Files:** `crates/er-engine/src/github.rs`, after `parse_review_threads_response` (ends 2188); tests at the end of `mod tests`.
- **Do this:**
  1. Tests first (fixtures follow the JSON shape at `github.rs:3436-3465`):

     ```rust
     #[test]
     fn thread_id_page_finds_thread_by_root_comment() {
         let json = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{
             "pageInfo":{"hasNextPage":false,"endCursor":null},
             "nodes":[
               {"id":"PRRT_a","isResolved":false,"comments":{"nodes":[{"databaseId":11}]}},
               {"id":"PRRT_b","isResolved":true,"comments":{"nodes":[{"databaseId":22}]}}
             ]}}}}}"#;
         let page = parse_thread_id_page(json, 11).unwrap();
         assert_eq!(page.found, Some(("PRRT_a".to_string(), false)));
         assert_eq!(page.next_cursor, None);
         let page = parse_thread_id_page(json, 22).unwrap();
         assert_eq!(page.found, Some(("PRRT_b".to_string(), true)));
     }

     #[test]
     fn thread_id_page_reports_next_cursor_when_not_found() {
         let json = r#"{"data":{"repository":{"pullRequest":{"reviewThreads":{
             "pageInfo":{"hasNextPage":true,"endCursor":"CUR"},
             "nodes":[{"id":"PRRT_a","isResolved":false,"comments":{"nodes":[{"databaseId":11}]}}]
             }}}}}"#;
         let page = parse_thread_id_page(json, 99).unwrap();
         assert_eq!(page.found, None);
         assert_eq!(page.next_cursor.as_deref(), Some("CUR"));
     }

     #[test]
     fn thread_id_page_rejects_malformed_json() {
         assert!(parse_thread_id_page("{}", 1).is_err());
     }

     #[test]
     fn resolve_thread_response_reads_is_resolved() {
         let ok = r#"{"data":{"resolveReviewThread":{"thread":{"id":"PRRT_a","isResolved":true}}}}"#;
         assert!(parse_resolve_thread_response(ok).unwrap());
         let err = r#"{"data":{"resolveReviewThread":null},"errors":[{"message":"no"}]}"#;
         assert!(parse_resolve_thread_response(err).is_err());
     }
     ```

  2. Run: `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine thread_id_page` → fails to compile (red).
  3. Implement with `serde_json::Value` (no new serde structs, so the sync structs stay untouched):

     ```rust
     #[derive(Debug, PartialEq, Eq)]
     struct ThreadIdPage {
         /// (thread node id, isResolved) of the thread whose root is the comment.
         found: Option<(String, bool)>,
         /// Cursor for the next page, when there is one.
         next_cursor: Option<String>,
     }

     fn parse_thread_id_page(json: &str, root_comment_id: u64) -> Result<ThreadIdPage> { … }
     fn parse_resolve_thread_response(json: &str) -> Result<bool> { … }
     ```

     `parse_thread_id_page` errors when `data.repository.pullRequest.reviewThreads` is missing. `next_cursor` is `Some` only when `hasNextPage` is true and `endCursor` is a non-empty string. `parse_resolve_thread_response` returns `data.resolveReviewThread.thread.isResolved`, and errors (including the first `errors[].message` in the text) when it is missing.
- **Watch out for:** `databaseId` can be `null` (see the existing fixture at 3459); skip such nodes.
- **Verify:** `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine thread_id_page` → `3 passed`; `... resolve_thread_response` → `1 passed`.
- **If it fails:** fix the parser and leave the fixtures alone.

### Step 3 — The `gh` call: look up, then resolve
- **Goal:** `pub fn gh_resolve_review_thread(owner: &str, repo: &str, pr: u64, root_comment_id: u64) -> Result<()>`.
- **Why / invariant:** er stores comment ids, GitHub resolves thread ids. Looking the thread up at click time keeps the sync path and `github-comments.json` format unchanged. A resolve is a manual, rare action, so one to eleven `gh` calls per click is acceptable under the rate-limit plan (it adds nothing to any timer).
- **Files:** `crates/er-engine/src/github.rs`, beside the parsers from Step 2.
- **Do this:**

  ```rust
  const THREAD_IDS_QUERY: &str = "query($owner: String!, $name: String!, $number: Int!, $after: String) { \
      repository(owner: $owner, name: $name) { pullRequest(number: $number) { \
      reviewThreads(first: 100, after: $after) { pageInfo { hasNextPage endCursor } \
      nodes { id isResolved comments(first: 1) { nodes { databaseId } } } } } } }";

  const RESOLVE_THREAD_MUTATION: &str = "mutation($threadId: ID!) { \
      resolveReviewThread(input: {threadId: $threadId}) { thread { id isResolved } } }";
  ```

  Loop up to 10 pages (same limits as `gh_pr_thread_addressing_remote`). Call `crate::gh_pr_actions::graphql(THREAD_IDS_QUERY, strings, &[("number", &pr.to_string())], "review thread lookup failed")`, where `strings` is `[("owner", owner), ("name", repo)]` plus `("after", cursor)` only when there is a cursor (omit it on the first page; an empty string is not a valid cursor). Then:
  - found and already resolved → `Ok(())` with no mutation;
  - found and open → run the mutation with `&[("threadId", id)]`, `&[]`, `"resolving the review thread failed"`; `parse_resolve_thread_response` must return `true`, else bail;
  - not found after the last page → bail `"GitHub has no review thread starting at comment {root_comment_id}. Pull GitHub comments and try again."`.
- **Watch out for:** `graphql` already maps a non-zero `gh` exit to an error with stderr. Permission errors (the viewer cannot resolve threads on this repo) arrive that way; let them through unchanged.
- **Verify:** `~/.claude/scripts/locked.sh . cargo check -p er-engine` → no errors (the network path is covered by the manual check in Step 5).
- **If it fails:** a type error on `graphql` means Step 1 did not land.

### Step 4 — Decide which ids go to GitHub (desktop), test first
- **Goal:** a pure helper that says whether a resolve must reach GitHub.
- **Why / invariant:** ADR 0007. Only an entry in `github-comments.json` may reach GitHub; `questions.json` and `notes.json` are never read for this decision.
- **Files:** `crates/er-desktop/src/commands.rs`, beside `mark_thread_resolved_in_files` (2805); tests beside `resolve_thread_updates_github_comment_file` (11625).
- **Do this:**
  1. Tests first, reusing the `GitHubReviewComment` literal from `resolve_thread_updates_github_comment_file` (11632-11660) with fields changed per case:
     - `source: "github"`, `github_id: Some(7)`, root, unresolved → `Ok(Some(7))`
     - same but `resolved: true` → `Ok(None)`
     - `in_reply_to: Some("gh-6")` → `Ok(None)`
     - `source: "local"`, `synced: false`, `github_id: None` → `Ok(None)` (local draft, local resolve as today)
     - `source: "local"`, `synced: true`, `github_id: None` → `Err(..)` containing `Pull GitHub comments`
     - id not in the file, or file missing → `Ok(None)`
  2. Implement:

     ```rust
     /// The GitHub comment id whose thread must be resolved on GitHub before
     /// the local flag is set, or `None` when the resolve stays local. Reads
     /// only the GitHub comment store, so a question or note never reaches
     /// GitHub (ADR 0007).
     fn github_thread_root_id(gc_path: &str, id: &str) -> Result<Option<u64>, String> { … }
     ```

     The pushed-without-id case must refuse: a local resolve there would be thrown away by the next pull, which replaces the local copy with GitHub's (`sync.rs:233-237` keeps only unsynced local comments). Write that refusal as exactly `return Err(PULL_FIRST.to_string());`, with `const PULL_FIRST: &str = "This comment was pushed in a review and has no GitHub id yet. Pull GitHub comments first, then resolve.";`, so the planted break in this step's Verify can target it.
- **Verify:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop github_thread_root_id` → all cases pass.
  - Planted break, applied and reverted in one command (commit this step first, since `git checkout --` drops uncommitted edits): `sed -i '' 's/return Err(PULL_FIRST.to_string());/return Ok(None);/' crates/er-desktop/src/commands.rs && ~/.claude/scripts/locked.sh . cargo test -p er-desktop github_thread_root_id; git checkout -- crates/er-desktop/src/commands.rs` → expect the synced-without-id case to FAIL; afterwards `git diff --quiet crates/er-desktop/src/commands.rs` exits 0.
- **If it fails:** fix the helper.

### Step 5 — Wire it into `resolve_thread`
- **Goal:** GitHub first, then the local write; no network call under the app lock.
- **Why / invariant:** if GitHub refuses, nothing local changes and the command returns `Err`, so the frontend's optimistic resolve rolls back (`optimisticLocal.ts:1186-1189`). Holding the app lock across a `gh` round trip would freeze every other command for its length.
- **Files:** `crates/er-desktop/src/commands.rs:2871-2900`:

  ```rust
  #[tauri::command]
  pub async fn resolve_thread(
      id: String,
      view: Option<crate::snapshot::OptimisticView>,
      state: State<'_, AppState>,
  ) -> Result<AppSnapshot, String> {
      let state = state.inner().clone();
      run_blocking(move || {
          let mut app = state.app.lock().map_err(|e| e.to_string())?;
          if let Some(early) = abort_wrong_view(&app, &state, view.as_ref()) {
              return early;
          }
          let tab = app.tab();
          ...
  ```
- **Do this:**
  1. Phase 1, under the lock: run `abort_wrong_view`; read `gc_path = app.tab().github_comments_path()`; call `github_thread_root_id(&gc_path, &id)?`; when it is `Some(n)`, also read `active_github_key(&app, &state)` and return `Err("No GitHub PR detected for the active tab")` if it is `None`. Drop the lock.
  2. Phase 2, no lock: when there is a GitHub root, call `er_engine::github::gh_resolve_review_thread(&owner, &repo, number, n).map_err(|e| e.to_string())?;` then `er_engine::github::invalidate_pr_comments_cache();` (same call as `commands.rs:3193`).
  3. Phase 3, lock again: run `abort_wrong_view` again (the person may have switched tabs during the call; GitHub is already resolved, and the next pull will show it), then run the existing local write unchanged (the `q_path` / `notes_path` / `mark_thread_resolved_in_files` / `mark_thread_resolved_in_memory` / `mark_sidecar_written` block, 2884-2897).
- **Watch out for:** `mark_thread_resolved_in_files` returns `None` ("Thread not found or already resolved") for an id it cannot find. Keep that message.
- **Verify:**
  - `~/.claude/scripts/locked.sh . cargo test -p er-desktop resolve_thread` → the two existing tests and the new ones pass.
  - Manual: open a PR with an open review thread in the desktop app. Click Resolve on it. On github.com the thread shows "resolved". Wait more than 45 seconds (one comment-sync tick); in er the thread is still resolved.
  - Manual: on a repo where you lack permission to resolve, click Resolve; an error appears and the thread is shown unresolved again.
- **If it fails:** if the thread reopens after the sync tick, check the cache invalidation and that the mutation actually ran (GitHub web).

## Gotchas & landmines
- **Today's local-only resolve is silently undone.** The pull at `sync.rs:273-285` sets `resolved: state.resolved` from GitHub on every GitHub comment. That is why GitHub has to be resolved first, and why the 60-second comment bundle cache (`github.rs:2082-2099`) must be invalidated afterwards, or a warm cached pull reopens the thread.
- **`ReviewThreadState` derives `Copy`** (`github.rs:1925`) and `sync.rs:273` uses `.copied()`; `merged_outdated_state` is a `const fn` taking it by value (`sync.rs:137`). Adding a `String` thread id there breaks all three. This task does not touch it.
- **Batch-pushed comments have no `github_id`** until the next pull (`crates/er-tui/src/input/mod.rs:1902-1906` and `commands.rs:3618-3626` mark them `synced` without an id). Step 4 refuses those with a pull-first message.
- **GraphQL cursors:** omit `after` on the first page; `-f after=` sends an empty string, which is not a valid cursor.
- **`comments-check`** rejects comments citing plan steps or tracker numbers.

## Decision log (forks pre-answered)
- **Fetch the thread id at click time, or store it during sync?** At click time. Storing it means changing the sync queries, `ReviewThreadState` (which is `Copy`) or the `github-comments.json` format, and adding work to the 45s loop for a rare manual action.
- **Where the new `gh` code lives:** `github.rs`, in its "GraphQL review thread resolution" section, with `gh_pr_actions::graphql` made `pub(crate)`. Writing a second GraphQL runner would duplicate `graphql_args`.
- **Resolve a reply id?** Leave replies local, as today (ADR 0008: only a root is a thread; the desktop offers Resolve on roots).
- **Thread already resolved on GitHub:** skip the mutation and mark it locally.
- **No frontend change.** The optimistic resolve and its rollback already exist. A label change ("Resolve on GitHub") is not part of this task.
- **TUI:** no resolve action exists there, so none is added here.

## Full verification
1. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine thread_id_page` → 3 passed.
2. `~/.claude/scripts/locked.sh . ./scripts/er-tui.sh test -p er-engine resolve_thread_response` → 1 passed.
3. `~/.claude/scripts/locked.sh . cargo test -p er-desktop github_thread_root_id` → all pass.
4. `~/.claude/scripts/locked.sh . cargo test -p er-desktop resolve_thread` → all pass.
5. `~/.claude/scripts/locked.sh . just test` and `~/.claude/scripts/locked.sh . just test-desktop` → all pass.
6. `~/.claude/scripts/locked.sh . just clippy` → exit 0.
7. `~/.claude/scripts/locked.sh . just comments-check` → exit 0.
8. The manual checks in Step 5.

In the final report, list every test added: the four engine parser tests and the `github_thread_root_id` cases.

## Rollback
Revert the commits. No sidecar or config format changes. Threads resolved on GitHub while the change was live stay resolved there; that is the intended effect.

## When to STOP and ask
- An anchor doesn't exist or doesn't match.
- A verify step fails in a way not covered.
- A step needs something OUT of scope.
- A `TODO(executor)` can't be resolved with its search hint.
- `gh api graphql` rejects `resolveReviewThread` for a user who can resolve the same thread on github.com (would mean a token scope problem worth a person's look).
- You find a GitHub comment shape where the thread's first comment is not the comment er stores as the root.
- Anyone asks for unresolve or resolve-with-reason as part of this task.

## Comments
