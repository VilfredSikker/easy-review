# Comment System Extensions

## Overview

Four improvements to the existing comment system: GitHub PR comment sync, single-level replies, comment deletion, and smarter inline vs after-hunk rendering based on comment target.

### What exists today

- `FeedbackComment` struct with `hunk_index`, `line_start`, `in_reply_to`, `resolved` fields (schema ready, UI not wired)
- `comments_for_hunk()` query on `AiState` (returns all comments for a file+hunk, no line vs hunk distinction)
- `start_comment()` / `submit_comment()` / `cancel_comment()` lifecycle in `TabState`
- `InputMode::Comment` with basic text input (Enter/Esc/Backspace)
- Comments render after each hunk in `diff_view.rs` with 💬 icon and timestamp
- `comment_line_num` captured during `start_comment()` (stored as `line_start`) but not used in rendering

### What's NOT implemented

- No GitHub PR comment sync (pull or push)
- No reply UI (`in_reply_to` field exists but unused)
- No comment focus / navigation (can't select individual comments)
- No comment deletion
- No distinction between line comments (inline) and hunk comments (after hunk) in rendering

---

## 1. GitHub PR Comment Sync

Pull existing GitHub PR review comments into `er` and push `er` comments back to the PR.

### Pull: GitHub → er

**When:** On startup if the repo has a PR checked out (detected via `gh pr view --json number`), and on manual refresh (`G` key).

**API:** `gh api repos/{owner}/{repo}/pulls/{number}/comments` returns review comments with:
- `id` (GitHub comment ID)
- `body` (markdown text)
- `path` (file path)
- `line` / `original_line` (line number in diff)
- `side` ("RIGHT" for new side, "LEFT" for old side)
- `in_reply_to_id` (threading)
- `user.login` (author)
- `created_at` / `updated_at`
- `diff_hunk` (the hunk context)

Also pull PR-level (non-inline) comments via `gh api repos/{owner}/{repo}/issues/{number}/comments`.

**Mapping to FeedbackComment:**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackComment {
    pub id: String,
    pub timestamp: String,
    pub file: String,
    pub hunk_index: Option<usize>,
    pub line_start: Option<usize>,
    pub line_end: Option<usize>,
    pub line_content: String,
    pub comment: String,
    pub in_reply_to: Option<String>,
    pub resolved: bool,

    // ── New fields ──
    /// "local" | "github"
    #[serde(default = "default_source")]
    pub source: String,

    /// GitHub comment ID (for sync/dedup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub github_id: Option<u64>,

    /// Author display name ("You" for local, GitHub login for remote)
    #[serde(default)]
    pub author: String,

    /// Whether this comment was pushed to GitHub
    #[serde(default)]
    pub synced: bool,
}

fn default_source() -> String { "local".to_string() }
```

**Hunk matching:** GitHub gives `line` (new-side line number) and `diff_hunk` (the surrounding context). To find the `hunk_index`:
1. Iterate the file's hunks
2. Check if the comment's `line` falls within the hunk's new-side line range
3. If no match (stale comment after rebase), attach to the file level (`hunk_index: None`)

**Dedup:** Use `github_id` as the unique key. On each sync:
- New GitHub comments (no matching `github_id` in feedback) → insert
- Existing GitHub comments (matching `github_id`) → update body/resolved if changed
- Deleted GitHub comments (in feedback but not in API response) → remove
- Local comments with `synced: true` → already pushed, skip

### Push: er → GitHub

**When:** User presses `P` on a local comment (or `Shift-P` to push all unpushed).

**API:** `gh api repos/{owner}/{repo}/pulls/{number}/comments -f body=... -f path=... -f line=... -f side=RIGHT`

For replies: `gh api repos/{owner}/{repo}/pulls/{number}/comments -f body=... -F in_reply_to={github_id}`

**After push:** Set `synced: true` and store the returned `github_id` on the local comment.

**Conflict handling:** If the diff has changed since the PR was fetched (force push), line numbers may be stale. Show a warning: `⚠ Comment may be misplaced — diff has changed since last sync`. Don't block the push — GitHub handles stale comments gracefully.

### GitHub sync state

Store sync metadata in `.er-feedback.json` header:

```json
{
  "version": 1,
  "diff_hash": "...",
  "github": {
    "pr_number": 42,
    "owner": "user",
    "repo": "easy-review",
    "last_synced": "2026-02-25T10:00:00Z"
  },
  "comments": [...]
}
```

### New struct

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitHubSyncState {
    pub pr_number: Option<u64>,
    pub owner: String,
    pub repo: String,
    pub last_synced: String,
}
```

Add to `ErFeedback`:
```rust
pub struct ErFeedback {
    pub version: u32,
    pub diff_hash: String,
    #[serde(default)]
    pub github: Option<GitHubSyncState>,
    #[serde(default)]
    pub comments: Vec<FeedbackComment>,
}
```

### GitHub module additions

**File:** `src/github.rs`

```rust
/// Fetch all review comments for a PR
pub fn gh_pr_comments(owner: &str, repo: &str, pr: u64) -> Result<Vec<GitHubComment>> {
    // gh api repos/{owner}/{repo}/pulls/{pr}/comments --paginate
    // Parse JSON array into Vec<GitHubComment>
}

/// Push a comment to a PR
pub fn gh_pr_push_comment(
    owner: &str, repo: &str, pr: u64,
    path: &str, line: usize, body: &str,
) -> Result<u64> {
    // gh api -X POST repos/{owner}/{repo}/pulls/{pr}/comments ...
    // Returns the new comment's GitHub ID
}

/// Push a reply to an existing PR comment
pub fn gh_pr_reply_comment(
    owner: &str, repo: &str, pr: u64,
    in_reply_to: u64, body: &str,
) -> Result<u64> { ... }

/// Delete a PR comment
pub fn gh_pr_delete_comment(
    owner: &str, repo: &str, comment_id: u64,
) -> Result<()> {
    // gh api -X DELETE repos/{owner}/{repo}/pulls/{pr}/comments/{comment_id}
}
```

```rust
#[derive(Debug, Clone, Deserialize)]
pub struct GitHubComment {
    pub id: u64,
    pub body: String,
    pub path: Option<String>,       // None for PR-level comments
    pub line: Option<usize>,
    pub original_line: Option<usize>,
    pub side: Option<String>,
    pub in_reply_to_id: Option<u64>,
    pub user: GitHubUser,
    pub created_at: String,
    pub updated_at: String,
    pub diff_hunk: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct GitHubUser {
    pub login: String,
}
```

### Keybindings

| Key | Context | Action |
|-----|---------|--------|
| `G` | Normal | Sync GitHub comments (pull) |
| `P` | On a comment | Push this comment to GitHub |
| `Shift-P` | Normal | Push all unpushed local comments |

### Rendering

GitHub comments render like local comments but with the author's GitHub login instead of "You":

```
  💬 octocat  2h ago
    What about key rotation?

  💬 You  10m ago  ↑ synced
    Good point, adding kid header.
```

The `↑ synced` indicator shows the comment was pushed to GitHub.

---

## 2. Comment on Comments (Single-Level Replies)

Allow replying to an existing comment. Only one level deep — no recursive threading.

### Data model

Already supported: `FeedbackComment.in_reply_to` exists. Replies set `in_reply_to` to the parent comment's `id`.

**Constraint:** If a comment already has `in_reply_to` set (it's a reply), it cannot be replied to. The `r` key is disabled on replies. This keeps things flat — one parent, N replies.

### Starting a reply

**Key:** `r` when cursor is on a comment in the diff view.

**Requires:** Comments need to be navigable. Add a concept of "focused comment" within a hunk.

```rust
// In TabState
pub comment_focus: Option<CommentFocus>,

#[derive(Debug, Clone)]
pub struct CommentFocus {
    pub file: String,
    pub hunk_index: Option<usize>,
    pub comment_id: String,
}
```

**Navigation:** When viewing a hunk that has comments, pressing `Tab` moves focus into the comment list. Arrow keys navigate between comments. `Esc` or `Tab` again returns to hunk navigation. The focused comment gets a highlighted border.

### Reply flow

1. User focuses a comment, presses `r`
2. If the focused comment is itself a reply (`in_reply_to.is_some()`), show notification: "Cannot reply to a reply" and abort
3. Otherwise, enter `InputMode::Comment` with `comment_reply_to = Some(parent_id)`
4. `submit_comment()` already handles `in_reply_to` — no change needed there

### Reply rendering

Replies render indented under their parent:

```
  💬 octocat  2h ago
    What about key rotation?
      ↳ 💬 You  10m ago
        Adding kid header to the JWT.
      ↳ 💬 coworker  5m ago
        LGTM, don't forget the JWKS endpoint.
```

Implementation: After rendering a comment, check for replies (`comments.iter().filter(|c| c.in_reply_to == Some(parent.id))`). Render each reply with 4 extra spaces of indent and a `↳` prefix.

### Reply to GitHub comments

When replying to a GitHub comment (`source == "github"`), use `gh_pr_reply_comment()` on push. GitHub's API natively supports `in_reply_to` for review comment replies.

---

## 3. Delete Own Comments

Allow deleting local comments. GitHub comments require API deletion.

### Deletion rules

- **Local comments** (`source == "local"`): Always deletable
- **GitHub comments you authored** (`source == "github"`, `author == your_login`): Deletable (also deletes from GitHub via API)
- **GitHub comments by others**: Not deletable — show "Cannot delete others' comments"

### Keybinding

**Key:** `d` when a comment is focused (via `Tab` navigation from §2).

### Flow

1. User focuses a comment, presses `d`
2. Check deletion rules above
3. Show confirmation: `Delete comment? (y/n)`
4. On `y`:
   - Remove from `comments` array in `.er-feedback.json`
   - If `github_id.is_some()` → call `gh_pr_delete_comment()`
   - Also remove any replies to this comment (cascade)
   - Reload AI state
5. On `n` or `Esc`: Cancel

### Confirmation mode

Add a new input mode:

```rust
pub enum InputMode {
    Normal,
    Search,
    Comment,
    Confirm(ConfirmAction),
}

#[derive(Debug, Clone)]
pub enum ConfirmAction {
    DeleteComment { comment_id: String },
}
```

Handle in event loop: only `y` and `n`/`Esc` are accepted. Any other key is ignored.

### Cascade deletion

When deleting a parent comment, also delete all replies. This prevents orphan replies:

```rust
fn delete_comment(&mut self, comment_id: &str) -> Result<()> {
    if let Some(ref mut feedback) = self.ai.feedback {
        // Remove the comment and all its replies
        feedback.comments.retain(|c| {
            c.id != comment_id && c.in_reply_to.as_deref() != Some(comment_id)
        });
        self.save_feedback()?;
    }
    Ok(())
}
```

---

## 4. Line Comments Inline, Hunk Comments After Hunks

Currently all comments render after the hunk. This change makes comment placement context-aware.

### Two comment types

| Type | Condition | Rendering |
|------|-----------|-----------|
| **Line comment** | `line_start.is_some()` | Inline, directly after the target line |
| **Hunk comment** | `line_start.is_none()` | After all lines in the hunk (current behavior) |

### Line comment rendering

**File:** `src/ui/diff_view.rs`

In the line-by-line rendering loop for a hunk, after rendering each diff line, check if any comments target that line:

```rust
// Inside the hunk line rendering loop
for (line_idx, line) in hunk.lines.iter().enumerate() {
    let new_line_num = /* compute new-side line number */;

    // Render the diff line itself
    lines.push(render_diff_line(line, ...));

    // Render any line-targeted comments
    if matches!(tab.mode, DiffMode::Branch | DiffMode::Recent | DiffMode::Unstaged | DiffMode::Staged) {
        let line_comments = tab.ai.comments_for_line(&file.path, hunk_idx, new_line_num);
        for comment in &line_comments {
            lines.push(render_inline_comment(comment, indent + 4));
            // Also render replies
            let replies = tab.ai.replies_to(&comment.id);
            for reply in &replies {
                lines.push(render_inline_reply(reply, indent + 8));
            }
        }
    }
}

// After all hunk lines: render hunk-level comments (no line_start)
let hunk_comments = tab.ai.comments_for_hunk_only(&file.path, hunk_idx);
for comment in &hunk_comments {
    lines.push(render_hunk_comment(comment));
    let replies = tab.ai.replies_to(&comment.id);
    for reply in &replies {
        lines.push(render_hunk_reply(reply));
    }
}
```

### New query methods on AiState

```rust
impl AiState {
    /// Comments targeting a specific line within a hunk
    pub fn comments_for_line(&self, path: &str, hunk_idx: usize, line_num: usize) -> Vec<&FeedbackComment> {
        match &self.feedback {
            Some(fb) => fb.comments.iter().filter(|c| {
                c.file == path
                    && c.hunk_index == Some(hunk_idx)
                    && c.line_start == Some(line_num)
                    && c.in_reply_to.is_none()  // Only top-level
            }).collect(),
            None => Vec::new(),
        }
    }

    /// Comments targeting the hunk as a whole (no specific line)
    pub fn comments_for_hunk_only(&self, path: &str, hunk_idx: usize) -> Vec<&FeedbackComment> {
        match &self.feedback {
            Some(fb) => fb.comments.iter().filter(|c| {
                c.file == path
                    && c.hunk_index == Some(hunk_idx)
                    && c.line_start.is_none()
                    && c.in_reply_to.is_none()  // Only top-level
            }).collect(),
            None => Vec::new(),
        }
    }

    /// Replies to a specific comment
    pub fn replies_to(&self, comment_id: &str) -> Vec<&FeedbackComment> {
        match &self.feedback {
            Some(fb) => fb.comments.iter().filter(|c| {
                c.in_reply_to.as_deref() == Some(comment_id)
            }).collect(),
            None => Vec::new(),
        }
    }
}
```

### Visual distinction

Line comments are tighter (less padding, no separator line):

```
  + let token = jwt::encode(&header, &claims, &key)?;
     💬 octocat: What about key rotation?          ← line comment, inline
       ↳ 💬 You: Adding kid header.
  + let refresh = generate_refresh_token();
  + Ok((token, refresh))
  ─── end of hunk ───
  💬 You: This whole hunk needs error handling.    ← hunk comment, after
```

Line comments use a slightly different background (a shade lighter than `COMMENT_BG`) to visually distinguish them from hunk comments.

### Comment creation context

When pressing `c`:
- If cursor is on a specific diff line → create a **line comment** (captures `line_start`)
- `start_comment()` already captures `comment_line_num` from `current_line` — this becomes `line_start`

When pressing `C` (Shift-C):
- Always creates a **hunk comment** (no `line_start`), regardless of cursor position
- Useful for general observations about the hunk

Current `start_comment()` already distinguishes these — `comment_line_num` is `Some(n)` for `c` and `None` for `C`. The only change is in rendering.

### Rename existing query

Rename `comments_for_hunk()` → split into `comments_for_line()` + `comments_for_hunk_only()`. Update all call sites (diff_view.rs overlay rendering, AI review panel, side panel).

---

## Implementation Order

### Step 1: Line vs hunk comment rendering (§4)
- Split `comments_for_hunk()` into `comments_for_line()` + `comments_for_hunk_only()`
- Add `replies_to()` query
- Update diff_view.rs rendering: line comments inline, hunk comments after
- Visual styling distinction

### Step 2: Comment focus & navigation (§2 prereq)
- Add `CommentFocus` struct and `comment_focus` field to TabState
- `Tab` key enters/exits comment focus mode within a hunk
- Arrow keys to navigate focused comments
- Highlight focused comment

### Step 3: Replies (§2)
- `r` on focused comment → enter reply mode
- Block replies to replies
- Indented reply rendering with `↳` prefix
- Reply submission with `in_reply_to` set

### Step 4: Comment deletion (§3)
- Add `ConfirmAction` and confirm mode to InputMode
- `d` on focused comment → confirm → delete
- Cascade delete replies
- Deletion rules (local always, GitHub only own)

### Step 5: GitHub comment sync — pull (§1)
- Add `source`, `github_id`, `author`, `synced` fields to FeedbackComment
- Add `GitHubSyncState` to ErFeedback
- Implement `gh_pr_comments()` in github.rs
- Hunk matching logic (line → hunk_index)
- `G` keybind to trigger sync
- Dedup on `github_id`

### Step 6: GitHub comment sync — push (§1)
- Implement `gh_pr_push_comment()` and `gh_pr_reply_comment()` in github.rs
- `P` on comment → push to GitHub
- `Shift-P` → push all unpushed
- Set `synced: true` + store `github_id` after push
- GitHub reply threading via `in_reply_to`

### Step 7: GitHub comment deletion (§3 + §1)
- Implement `gh_pr_delete_comment()` in github.rs
- On delete of synced comment → also delete from GitHub
- Handle API errors gracefully (comment already deleted, permissions)

## Files Changed

| File | Change |
|------|--------|
| `src/ai/review.rs` | New fields on FeedbackComment, GitHubSyncState, split query methods, replies_to() |
| `src/app/state.rs` | CommentFocus, comment navigation, delete_comment(), ConfirmAction, reply flow |
| `src/ui/diff_view.rs` | Inline line comments, after-hunk comments, reply rendering, focus highlighting |
| `src/main.rs` | `r` reply, `d` delete, `Tab` comment focus, `G` sync, `P`/`Shift-P` push, confirm mode handler |
| `src/github.rs` | gh_pr_comments(), gh_pr_push_comment(), gh_pr_reply_comment(), gh_pr_delete_comment(), GitHubComment struct |
| `src/ui/status_bar.rs` | Hints for new keybinds (r/d/G/P) |

## Keybinding Summary

| Key | Context | Action |
|-----|---------|--------|
| `c` | On a diff line | Line comment (inline) |
| `C` | On a hunk | Hunk comment (after hunk) |
| `Tab` | In hunk with comments | Enter/exit comment focus |
| `r` | Focused comment | Reply (1 level only) |
| `d` | Focused comment | Delete (with confirmation) |
| `R` | Focused comment | Toggle resolved |
| `G` | Normal | Pull GitHub PR comments |
| `P` | Focused comment | Push to GitHub |
| `Shift-P` | Normal | Push all unpushed comments |
