# PR Overview View

## Overview

When `er` opens a GitHub PR (via URL argument or `--pr` flag), add a dedicated PR overview that shows the PR description, CI check status, review state, and labels. This gives the reviewer context before diving into code — the "what and why" before the "how".

Accessible as a toggle overlay or as the initial landing screen when opening a PR.

---

## 1. PR Metadata Storage

Currently `TabState` only stores `base_branch` from the PR. Expand to persist full PR context.

### New struct

```rust
#[derive(Debug, Clone)]
pub struct PrMeta {
    pub number: u64,
    pub owner: String,
    pub repo: String,
    pub title: String,
    pub body: String,               // Markdown description
    pub author: String,
    pub author_avatar: Option<String>,
    pub state: PrState,
    pub draft: bool,
    pub labels: Vec<String>,
    pub base_branch: String,
    pub head_branch: String,
    pub url: String,
    pub created_at: String,
    pub updated_at: String,
    pub additions: usize,
    pub deletions: usize,
    pub changed_files: usize,
    pub mergeable: Option<bool>,
    pub review_decision: Option<String>,  // APPROVED, CHANGES_REQUESTED, REVIEW_REQUIRED
    pub reviewers: Vec<Reviewer>,
    pub checks: Vec<CiCheck>,
}

#[derive(Debug, Clone)]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

#[derive(Debug, Clone)]
pub struct Reviewer {
    pub login: String,
    pub state: String,   // APPROVED, CHANGES_REQUESTED, COMMENTED, PENDING
}

#[derive(Debug, Clone)]
pub struct CiCheck {
    pub name: String,
    pub status: CheckStatus,
    pub conclusion: Option<String>,  // success, failure, neutral, skipped, etc.
    pub url: Option<String>,
    pub started_at: Option<String>,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone)]
pub enum CheckStatus {
    Queued,
    InProgress,
    Completed,
}
```

### Add to TabState

```rust
pub struct TabState {
    // ...existing fields...

    /// GitHub PR metadata (None if not opened from a PR)
    pub pr: Option<PrMeta>,
}
```

---

## 2. Fetching PR Data

### Single `gh` call for metadata

```rust
pub fn gh_pr_metadata(pr_number: u64, repo_root: &str) -> Result<PrMeta> {
    let output = Command::new("gh")
        .args([
            "pr", "view",
            &pr_number.to_string(),
            "--json",
            "number,title,body,author,state,isDraft,labels,baseRefName,headRefName,\
             url,createdAt,updatedAt,additions,deletions,changedFiles,mergeable,\
             reviewDecision,reviews,statusCheckRollup",
        ])
        .current_dir(repo_root)
        .output()?;

    let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
    parse_pr_metadata(&json)
}
```

### Parsing

```rust
fn parse_pr_metadata(json: &serde_json::Value) -> Result<PrMeta> {
    let author = json["author"]["login"].as_str().unwrap_or("unknown");
    let state = match json["state"].as_str().unwrap_or("OPEN") {
        "MERGED" => PrState::Merged,
        "CLOSED" => PrState::Closed,
        _ => PrState::Open,
    };

    // Parse reviews into deduplicated reviewers (latest state per user)
    let reviewers = parse_reviewers(&json["reviews"]);

    // Parse CI checks
    let checks = parse_checks(&json["statusCheckRollup"]);

    // Parse labels
    let labels: Vec<String> = json["labels"]
        .as_array()
        .map(|arr| arr.iter().filter_map(|l| l["name"].as_str().map(String::from)).collect())
        .unwrap_or_default();

    Ok(PrMeta {
        number: json["number"].as_u64().unwrap_or(0),
        title: json["title"].as_str().unwrap_or("").to_string(),
        body: json["body"].as_str().unwrap_or("").to_string(),
        author: author.to_string(),
        state,
        draft: json["isDraft"].as_bool().unwrap_or(false),
        labels,
        base_branch: json["baseRefName"].as_str().unwrap_or("").to_string(),
        head_branch: json["headRefName"].as_str().unwrap_or("").to_string(),
        url: json["url"].as_str().unwrap_or("").to_string(),
        created_at: json["createdAt"].as_str().unwrap_or("").to_string(),
        updated_at: json["updatedAt"].as_str().unwrap_or("").to_string(),
        additions: json["additions"].as_u64().unwrap_or(0) as usize,
        deletions: json["deletions"].as_u64().unwrap_or(0) as usize,
        changed_files: json["changedFiles"].as_u64().unwrap_or(0) as usize,
        mergeable: json["mergeable"].as_str().map(|s| s == "MERGEABLE"),
        review_decision: json["reviewDecision"].as_str().map(String::from),
        reviewers,
        checks,
        author_avatar: None,
    })
}
```

### Reviewer dedup

GitHub returns all review events. Deduplicate to latest state per reviewer:

```rust
fn parse_reviewers(reviews: &serde_json::Value) -> Vec<Reviewer> {
    let mut latest: std::collections::HashMap<String, String> = HashMap::new();

    if let Some(arr) = reviews.as_array() {
        for review in arr {
            let login = review["author"]["login"].as_str().unwrap_or("").to_string();
            let state = review["state"].as_str().unwrap_or("PENDING").to_string();
            if !login.is_empty() {
                latest.insert(login, state);
            }
        }
    }

    latest.into_iter()
        .map(|(login, state)| Reviewer { login, state })
        .collect()
}
```

### CI checks parsing

```rust
fn parse_checks(rollup: &serde_json::Value) -> Vec<CiCheck> {
    let mut checks = Vec::new();
    if let Some(arr) = rollup.as_array() {
        for item in arr {
            // statusCheckRollup items can be CheckRun or StatusContext
            let name = item["name"].as_str()
                .or_else(|| item["context"].as_str())
                .unwrap_or("unknown")
                .to_string();

            let status = match item["status"].as_str().unwrap_or("") {
                "IN_PROGRESS" => CheckStatus::InProgress,
                "COMPLETED" => CheckStatus::Completed,
                _ => CheckStatus::Queued,
            };

            let conclusion = item["conclusion"].as_str().map(|s| s.to_lowercase());
            let url = item["detailsUrl"].as_str()
                .or_else(|| item["targetUrl"].as_str())
                .map(String::from);

            checks.push(CiCheck {
                name,
                status,
                conclusion,
                url,
                started_at: item["startedAt"].as_str().map(String::from),
                completed_at: item["completedAt"].as_str().map(String::from),
            });
        }
    }
    checks
}
```

---

## 3. When to Fetch

### On PR open

During `TabState::from_github_pr()` and the `--pr` flow, fetch full metadata after checkout:

```rust
// After existing checkout + base branch logic:
let pr_meta = gh_pr_metadata(pr_number, &repo_root)?;
tab.pr = Some(pr_meta);
```

### Manual refresh

`R` key (Shift-R) re-fetches PR metadata (checks may have completed, reviews may have arrived):

```rust
pub fn refresh_pr_metadata(&mut self) -> Result<()> {
    if let Some(ref mut pr) = self.pr {
        let updated = gh_pr_metadata(pr.number, &self.repo_root)?;
        *pr = updated;
    }
    Ok(())
}
```

### Periodic check polling (optional)

CI checks change over time. Optionally poll every 30 seconds while PR overview is visible:

```rust
// In event loop, alongside AI file polling
if app.tab().pr.is_some() && poll_counter % 300 == 0 {
    let _ = app.tab_mut().refresh_pr_metadata();
}
```

---

## 4. PR Overview Rendering

### Access

**Key:** `p` toggles the PR overview overlay when a PR is loaded. If no PR context exists, show: `"No PR context — open with er <pr-url> or er --pr <number>"`

The overview is a full-screen overlay (like the AI Review dashboard), not a new DiffMode. It's context you glance at, not a mode you stay in.

### Layout

```
┌─ PR #42: Fix token expiry in JWT validation ────────────────────────────┐
│                                                                          │
│  octocat → main ← fix/jwt-expiry          OPEN · 2 hours ago            │
│                                                                          │
│  Labels: bug, security, priority:high                                    │
│  +45 −12 across 3 files                                                  │
│                                                                          │
│ ─── Description ─────────────────────────────────────────────────────── │
│                                                                          │
│  ## Problem                                                              │
│  JWT tokens issued by the auth service have no expiry enforcement.       │
│  Tokens remain valid indefinitely once issued, creating a security       │
│  risk if a token is leaked.                                              │
│                                                                          │
│  ## Solution                                                             │
│  - Add `exp` claim validation in `validate_token()`                      │
│  - Set default token lifetime to 1 hour                                  │
│  - Add refresh token rotation                                            │
│                                                                          │
│  ## Testing                                                              │
│  - Added unit tests for expiry validation                                │
│  - Verified backward compatibility with existing tokens                  │
│                                                                          │
│ ─── CI Checks ───────────────────────────────────────────────────────── │
│                                                                          │
│  ✓ build (ubuntu-latest)         passed    2m 14s                        │
│  ✓ build (macos-latest)          passed    3m 01s                        │
│  ✓ clippy                        passed    1m 22s                        │
│  ✗ test-integration              failed    4m 33s                        │
│  ◑ deploy-preview                running   0m 45s                        │
│  · security-scan                 queued                                  │
│                                                                          │
│ ─── Reviews ─────────────────────────────────────────────────────────── │
│                                                                          │
│  ✓ alice         approved                                                │
│  ✗ bob           changes requested                                       │
│  · charlie       pending                                                 │
│                                                                          │
│  Review decision: CHANGES_REQUESTED                                      │
│                                                                          │
│ ─── Mergeable ───────────────────────────────────────────────────────── │
│                                                                          │
│  ⚠ Not mergeable — 1 failing check, changes requested                   │
│                                                                          │
│  p: close  R: refresh  o: open in browser  ↑↓: scroll                   │
└──────────────────────────────────────────────────────────────────────────┘
```

### Sections

The overlay is a scrollable view with these sections:

**1. Header (always visible, 3–4 lines)**
- PR title (large, bold)
- `author → base ← head` with branch names
- State badge: `OPEN` / `MERGED` / `CLOSED` / `DRAFT`
- Relative timestamp, labels
- Stats: `+adds −dels across N files`

**2. Description (scrollable)**
- Render the PR body as best-effort markdown:
  - `##` headings → bold lines
  - `- item` → bullet points
  - `` `code` `` → styled spans
  - Code blocks → indented with code background
  - Links → show text (URL in dimmed parentheses)
- If no description: `(No description provided)`

**3. CI Checks**
- One line per check, sorted: failed first, then running, then queued, then passed
- Status icons:
  - `✓` green — passed/success
  - `✗` red — failed/failure
  - `◑` yellow — in progress
  - `·` dim — queued/pending
  - `⊘` dim — skipped/neutral
- Name, conclusion, and duration (if completed)

**4. Reviews**
- One line per reviewer, sorted: changes_requested first, then approved, then pending
- Status icons:
  - `✓` green — approved
  - `✗` red — changes requested
  - `💬` blue — commented (no verdict)
  - `·` dim — pending
- Overall review decision shown below

**5. Merge status**
- Single line summary:
  - `✓ Ready to merge` (all checks pass, approved, no conflicts)
  - `⚠ Not mergeable` with reasons (failing checks, changes requested, conflicts)
  - `⊘ Merged` or `⊘ Closed`

---

## 5. Markdown Rendering

PR descriptions are markdown. Full markdown rendering in a TUI is complex — implement a pragmatic subset:

```rust
pub fn render_markdown(text: &str, width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for raw_line in text.lines() {
        let trimmed = raw_line.trim();

        if trimmed.starts_with("## ") || trimmed.starts_with("### ") {
            // Section heading — bold
            let heading = trimmed.trim_start_matches('#').trim();
            lines.push(Line::from(Span::styled(
                heading.to_string(),
                Style::default().fg(TEXT_BRIGHT).add_modifier(Modifier::BOLD),
            )));
            lines.push(Line::from(""));
        } else if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            // Bullet point
            let content = trimmed[2..].trim();
            lines.push(Line::from(vec![
                Span::styled("  • ", Style::default().fg(MUTED)),
                render_inline_markdown(content),
            ]));
        } else if trimmed.starts_with("```") {
            // Code block toggle (track in/out state)
            // Inside code blocks: render with code background
        } else if trimmed.is_empty() {
            lines.push(Line::from(""));
        } else {
            // Regular paragraph — word wrap
            let wrapped = word_wrap(trimmed, width - 4);
            for w in wrapped {
                lines.push(Line::from(render_inline_markdown(&w)));
            }
        }
    }

    lines
}

/// Handle inline `code`, **bold**, and [links](url) within a line
fn render_inline_markdown(text: &str) -> Span<'static> {
    // Simple: just return as-is for v1
    // Later: parse backticks, bold, links into styled spans
    Span::styled(text.to_string(), Style::default().fg(TEXT))
}
```

Start simple — headings, bullets, paragraphs, and word wrapping. Add inline code/bold/link styling later.

---

## 6. Overlay State

```rust
pub struct PrOverviewState {
    pub scroll: u16,
    pub total_lines: usize,
}
```

Stored in `TabState`:

```rust
pub struct TabState {
    // ...existing...
    pub pr: Option<PrMeta>,
    pub pr_overview: Option<PrOverviewState>,
}
```

The overlay is opened/closed by toggling `pr_overview`:

```rust
pub fn toggle_pr_overview(&mut self) {
    if self.pr.is_none() { return; }
    if self.pr_overview.is_some() {
        self.pr_overview = None;
    } else {
        self.pr_overview = Some(PrOverviewState { scroll: 0, total_lines: 0 });
    }
}
```

---

## 7. Keybindings

| Key | Context | Action |
|-----|---------|--------|
| `p` | Normal (PR loaded) | Toggle PR overview overlay |
| `p` | Normal (no PR) | Show "no PR context" message |
| `R` | PR overview | Refresh PR metadata (re-fetch checks, reviews) |
| `o` | PR overview | Open PR in browser (`gh pr view --web`) |
| `↑` / `↓` | PR overview | Scroll |
| `j` / `k` | PR overview | Scroll (alternative) |
| `Esc` | PR overview | Close overlay |
| `Enter` | PR overview, cursor on a failed check | Open check URL in browser |

### Open in browser

```rust
pub fn open_pr_in_browser(pr_number: u64, repo_root: &str) -> Result<()> {
    Command::new("gh")
        .args(["pr", "view", &pr_number.to_string(), "--web"])
        .current_dir(repo_root)
        .status()?;
    Ok(())
}
```

---

## 8. Status Bar Integration

When a PR is loaded, the top status bar always shows a compact PR summary — even outside the overlay:

```
  PR #42 · Fix token expiry  ✓4 ✗1 ◑1  alice:✓ bob:✗     src/auth.rs  hunk 2/4
```

Breakdown:
- `PR #42` — PR number (clickable hint: press `p` for details)
- Truncated title
- Check summary: `✓4 ✗1 ◑1` (4 passed, 1 failed, 1 running)
- Reviewer summary: compact `login:status` pairs
- Then the normal file/hunk info

This gives at-a-glance CI and review status without opening the overlay.

---

## 9. Auto-Show on PR Open

When `er` opens a PR for the first time (via URL or `--pr`), briefly show the PR overview as the landing screen. The user presses any navigation key (j/k/n/1/2/3) to dismiss and jump into the diff.

```rust
// In App::new_with_args, after PR setup:
if tab.pr.is_some() {
    tab.pr_overview = Some(PrOverviewState { scroll: 0, total_lines: 0 });
}
```

This means the first thing you see when opening a PR is the description and check status — then you start reviewing code.

---

## 10. Edge Cases

### gh not installed / not authenticated

If `gh` is unavailable, PR metadata can't be fetched. The PR overview shows:

```
  GitHub CLI (gh) required for PR overview.
  Install: https://cli.github.com
  Then: gh auth login
```

The rest of `er` works normally (diff is from git, not gh).

### No CI checks

If the PR has no status checks:

```
  CI Checks
  No checks configured for this repository.
```

### Very long description

Word-wrap at terminal width minus padding. The description section is scrollable — it can be arbitrarily long without breaking layout.

### PR from fork

Fork PRs work with `gh pr checkout` (creates local branch from fork). The `head` branch shows as `fork-owner:branch-name`. No special handling needed.

### Stale metadata

CI checks and reviews change after fetch. The status bar check counts may be stale. Show relative time since last fetch:

```
  PR #42 · ✓4 ✗1  (fetched 5m ago — R to refresh)
```

---

## Implementation Steps

1. **PrMeta struct + storage** — Add to `TabState`, populate during PR checkout flow
2. **gh_pr_metadata()** — Fetch full PR JSON via `gh pr view --json ...`, parse into PrMeta
3. **PR overview overlay** — New rendering function with header, description, checks, reviews, merge status sections
4. **Markdown renderer** — Pragmatic subset: headings, bullets, paragraphs, word wrap
5. **CI check rendering** — Status icons, sorting (failed first), duration display
6. **Review rendering** — Per-reviewer status, overall decision
7. **Status bar integration** — Compact PR summary (number, check counts, reviewer states)
8. **`p` keybind** — Toggle overlay, auto-show on PR open
9. **`R` refresh** — Re-fetch metadata (new check results, review updates)
10. **`o` open in browser** — `gh pr view --web`

## Files Changed

| File | Change |
|------|--------|
| `src/github.rs` | `PrMeta`, `CiCheck`, `Reviewer`, `PrState`, `CheckStatus` structs; `gh_pr_metadata()`, parsers |
| `src/app/state.rs` | `pr: Option<PrMeta>`, `pr_overview: Option<PrOverviewState>`, `toggle_pr_overview()`, `refresh_pr_metadata()` |
| `src/ui/pr_overview.rs` | **New file** — PR overview overlay rendering (header, description, checks, reviews, merge status) |
| `src/ui/mod.rs` | Import and dispatch to `pr_overview` when overlay is active |
| `src/ui/status_bar.rs` | Compact PR summary in top bar (PR#, check counts, reviewer states) |
| `src/ui/utils.rs` | `render_markdown()` for PR description |
| `src/main.rs` | `p` keybind, `R` refresh, `o` open in browser, PR overview input handling |
