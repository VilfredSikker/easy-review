# The terminal UI runs on the review service

## In plain terms

- **What changed.** `er` in the terminal now runs on the same review service
  as the desktop app. Every key reads what the service published and sends
  each change to it, so the two front ends share one set of rules for
  reviewed marks, findings, comments, runs and settings. Submitting a review
  is back in the terminal.
- **What you need to do.** Nothing. AI picks now save at once, and a few
  messages read differently; the changes are listed below.

## New: submit a review from the terminal

- **Ctrl+T in the comment composer** cycles the comment's kind, including a
  comment for your pending GitHub review.
- **"Review and submit…" in the Git hub** (`g`) lists the pending review's
  comments and submits it as Approve, Request changes or Comment.

## Changed behaviour

### Settings and the AI picker

- Every AI pick (provider, model, effort) saves to `agents.toml` at once. The
  one-action pick is gone.
- With model discovery off, a refresh says "Model discovery is turned off in
  Settings → AI". The "Unknown AI provider", "Provider has no models_command"
  and "Model refresh already in progress" notices are gone.
- Adding a watched path that is already listed is skipped.

### Working tree

- A refused commit or stage shows a message, where the old TUI quit.
- Deleting a watched file is refused outside the repo, symlinked directories
  included.

### Comments, questions and notes

- A reply to a finding writes the thread's first comment, then the reply.
- Editing a comment keeps its anchor.
- A body of only whitespace is refused.
- A refused write keeps the composer's text.

### GitHub

- Approving your own PR is refused with a message.
- Deleting a posted comment deletes it on GitHub first, and keeps the local
  rows when that fails.
- Approve while a draft review exists shows GitHub's refusal.

### Diff modes, the base and the watcher

- Expanded context lines are kept across refreshes in one mode and one base.
- `R` also clears the reviewed marks on files that changed.
- The watcher follows the active tab's checkout and refreshes that tab.

### Findings, the checklist and the clears

- `Z`, and re-running the review through the clear-then-run confirm, also
  delete the professor and expert results.
- `z` is refused while a review flow holds the bucket.
- A clear refused on a busy bucket says so, in the clear's own words. The raw
  lock error no longer replaces that message a moment later.
- The checklist notice is worked out from the item as it stood at the press.
- Finding steps (Ctrl+j/k, the File panel's j/k and the AI Summary's Enter)
  skip the findings the Gate hides.

### The PR panel, the stack and the Open hub

- Opening a stack PR or a PR URL no longer blocks the screen.
- A stack row focuses an open local tab for the same PR.
- A PR tab whose overview has not loaded shows "Not available" in the stack
  section.
- "Switch worktree" reads disabled ("No other worktrees") until the worktree
  list arrives, and the picker shows the list read when the hub opened.
- A picked worktree or folder that does not load as a repo opens as a tab that
  shows "This tab did not load: …", where the old TUI quit. `x` closes it.

### AI runs

- Review, expert, professor, triage, tour and importance runs queue under the
  agent cap and say "queued (#n, tab)". Their output is checked and committed
  through staging.
- The agent log is kept per bucket (5,000 lines) and outlives finished runs.
  It goes when the last tab that shows its bucket closes.
- A run in Staged after a commit reviews the last commit.
- Start and finish notices are the service's: "review started (tab)",
  "Validation started (review)", "<kind> completed (tab)" and
  "<kind> failed (tab): …".
- A review rerun on `y` clears before it plans. A diff with no file is refused
  before the clear and keeps its review. A refused clear starts no run.
- A diff of only review-ignored files still clears on `y`, and the planner
  then finds nothing to review, so the review is lost. The old TUI kept it.
- The running badge and the Log header count every run on the bucket, tour
  and importance included. Experts show as `expert-<id>`.
- A tour's log lines go to the tour bucket's log, which on a PR view can be
  another bucket than the tab's.
- Importance is refused for a custom CLI family, as on the desktop.
- A disabled row, such as an unset Verify command, says what it needs when
  chosen.
- The tour row is enabled when the diff has files.
- A caught planning panic shows "planning panicked: …", and the TUI keeps
  running.

### Review flows

- Interrupted flows are reconciled for each bucket the first time a tab shows
  it.
- The results overlay opens a moment after its row, once its read lands.
- A rerun the service refuses from the progress overlay says "Review: …".
- Cancel review says "No running review to cancel" for any refusal.

### Copy, export and open

- A failed copy or editor launch shows a message, where the old TUI quit.
- Copying, or `e`, before a file's parse lands says "File is still loading".
- A shift-arrow selection never carries into another file. After a jump, a
  filter change or a refresh moves to another file, a context copy reads the
  line or hunk.

### Notices

- A refusal or an error stays on the status line until it has been drawn for
  its dwell. Another task's ordinary notice in the same moment waits for it,
  so it cannot hide a refusal such as "already running" or "summary failed".
  A second refusal in the same moment still replaces the first. A key press
  ends the hold: the waiting notice shows, and a notice the key gives
  replaces both.
- "✓ AI data refreshed" stays up for 3 s, and it is said for any reload the
  service applies, the GitHub comments, the tour buckets and the arena records
  included. A write to a background tab is applied without a notice.

### Start, tabs and exit

- A tab whose first load fails shows its error, where the old TUI exited
  before it opened. Quit or switch tabs from there.
- A tab not shown yet keeps its folder name in the tab strip until it first
  loads.
- Arena state is reconciled once per bucket, when its tab is first shown.
- Closing a tab with a write in flight still lands the write, without its
  notice. A clear-then-run whose tab closes first clears and starts no run.
- On exit, the resume hint no longer prints an `er --remote <url>` line for a
  local PR tab opened from a URL. Alone, such a tab prints no hint; with
  other tabs open, it is listed by its repo root.

## Fixes that also reach the desktop

- Concurrent reviewed marks no longer drop one.
- A run in Staged after a commit reviews the last commit.

## For contributors

- The `v1-service` feature, `just test-v1` and the "Test (v1-service)" CI job
  are gone. er-tui builds with er-engine's `ui` half off.
- `cargo check -p er-tui --all-targets` joins the own-graph check
  (`just check-desktop`, the "Desktop (own graph)" CI job).
- The parity tests check the committed goldens under
  `crates/er-tui/goldens/parity/`, which were recorded from the old TUI.
  `ER_UPDATE_GOLDENS=1` records them again; a re-record is a behaviour change.
- The `ER_DEBUG` memory line on the status bar is gone.
