# er — AI Integration Spec

## Overview

A bidirectional feedback loop between `er` (TUI reviewer) and Claude Code (AI assistant). Claude writes `.er-*` sidecar files, the user reviews and adds comments inside `er`, Claude reads the feedback and iterates.

```
┌─────────────┐     .er-* files      ┌──────────────┐
│             │ ──────────────────▶   │              │
│ Claude Code │                       │   er  (TUI)  │
│  (skills)   │   ◀──────────────── │              │
│             │    .er-feedback.json  │              │
└─────────────┘                       └──────────────┘
        │                                    │
        │         user adds comments         │
        │         on hunks via `c` key       │
        └────────────────────────────────────┘
                    loop continues
```

---

## Workflow

### First pass

1. User runs `er-review` in Claude Code → generates all `.er-*` files
2. User opens `er`, presses `v` to enter AI view mode
3. AI annotations appear: risk dots, inline findings, side panel, guided review

### Feedback loop

4. User navigates to a hunk, presses `c` to add a comment
5. Comment is appended to `.er-feedback.json` with full context (file, hunk, line range)
6. User runs `er-questions` in Claude Code → reads feedback, updates `.er-*` files
7. `er` detects file changes (via mtime check or file watcher), refreshes AI data
8. New/updated annotations appear — loop continues

### Staleness detection

- Every `.er-*` file stores a `diff_hash` — a SHA-256 of the raw diff output
- On startup and on each `refresh_diff()`, `er` computes the current diff hash
- If any `.er-*` file's `diff_hash` doesn't match → show warning banner:
  `⚠ AI review outdated — diff has changed. Run er-review to refresh.`
- Stale files still display (data is useful for context) but with dimmed styling
- Specific hunks that no longer exist are flagged as `[stale]`

---

## .er-* File Specifications

All files live in the repo root (next to `.git/`). All JSON files share a common header:

```json
{
  "version": 1,
  "diff_hash": "sha256-of-raw-diff-output",
  "created_at": "2026-02-24T10:30:00Z",
  "base_branch": "main",
  "head_branch": "feature/auth-refactor",
  ...
}
```

### .er-review.json

The core review file. Contains per-file risk assessments and per-hunk findings.

```json
{
  "version": 1,
  "diff_hash": "abc123...",
  "created_at": "2026-02-24T10:30:00Z",
  "base_branch": "main",
  "head_branch": "feature/auth-refactor",
  "files": {
    "src/auth/session.rs": {
      "risk": "high",
      "risk_reason": "Session token handling with security implications",
      "summary": "Refactors session creation to use JWT instead of opaque tokens",
      "findings": [
        {
          "id": "f1",
          "severity": "high",
          "category": "security",
          "title": "Token expiry not enforced",
          "description": "The new JWT is created without an `exp` claim. Tokens will never expire, creating a session fixation risk.",
          "hunk_index": 2,
          "line_start": 45,
          "line_end": 52,
          "suggestion": "Add `exp` claim with a reasonable TTL:\n```rust\n.set_expiration(now + Duration::hours(24))\n```",
          "related_files": ["src/auth/middleware.rs:validate_token()"]
        }
      ]
    },
    "src/api/routes.rs": {
      "risk": "low",
      "risk_reason": "Route registration only, no logic changes",
      "summary": "Adds /auth/refresh endpoint",
      "findings": []
    }
  }
}
```

**Risk levels:** `"high"`, `"medium"`, `"low"`, `"info"`

**Finding categories:** `"security"`, `"bug"`, `"performance"`, `"logic"`, `"style"`, `"test"`, `"docs"`

**Hunk targeting:** `hunk_index` is 0-based into the file's hunks array. `line_start`/`line_end` are new-file line numbers within that hunk (for precise annotation positioning). If both are null, the finding applies to the whole hunk.

### .er-order.json

Suggested review order. Files sorted by review priority (risk, dependency graph, logical grouping).

```json
{
  "version": 1,
  "diff_hash": "abc123...",
  "created_at": "2026-02-24T10:30:00Z",
  "base_branch": "main",
  "head_branch": "feature/auth-refactor",
  "order": [
    {
      "path": "src/auth/session.rs",
      "reason": "Core security change — review first",
      "group": "auth-core"
    },
    {
      "path": "src/auth/middleware.rs",
      "reason": "Validates tokens from session.rs",
      "group": "auth-core"
    },
    {
      "path": "src/api/routes.rs",
      "reason": "Wires up the new endpoint",
      "group": "api"
    },
    {
      "path": "tests/auth_test.rs",
      "reason": "Test coverage for the above",
      "group": "tests"
    }
  ],
  "groups": {
    "auth-core": { "label": "Authentication Core", "color": "red" },
    "api": { "label": "API Layer", "color": "blue" },
    "tests": { "label": "Test Suite", "color": "green" }
  }
}
```

**In `er`:** When `.er-order.json` is present, the file tree sorts by this order instead of alphabetical. Group headers appear as separators. User can press `O` to toggle between AI order and alphabetical.

### .er-summary.md

Plain markdown branch summary, shown in the `?` overlay popup and in the Side Panel header.

```markdown
## Branch Summary

**auth-refactor** → main | 8 files | +342 -89

### What changed
Migrates session management from opaque server-side tokens to stateless JWTs.
The refresh token flow is new — previously sessions were permanent.

### Key decisions
- JWT chosen over Paseto for ecosystem compatibility
- 24h access token TTL, 30d refresh token
- Refresh tokens stored in HttpOnly cookies

### Risk areas
- `session.rs`: Token expiry not yet enforced (see finding f1)
- No rate limiting on the refresh endpoint
- Missing test coverage for token rotation edge cases

### Review focus
Start with `session.rs` (the JWT construction) → `middleware.rs` (validation) →
`routes.rs` (endpoint wiring) → tests.
```

### .er-checklist.json

Review checklist that tracks what's been verified.

```json
{
  "version": 1,
  "diff_hash": "abc123...",
  "created_at": "2026-02-24T10:30:00Z",
  "base_branch": "main",
  "head_branch": "feature/auth-refactor",
  "items": [
    {
      "id": "c1",
      "text": "JWT expiry claim is set and enforced",
      "category": "security",
      "checked": false,
      "related_findings": ["f1"],
      "related_files": ["src/auth/session.rs"]
    },
    {
      "id": "c2",
      "text": "Refresh token rotation prevents replay",
      "category": "security",
      "checked": false,
      "related_findings": [],
      "related_files": ["src/auth/session.rs", "src/api/routes.rs"]
    },
    {
      "id": "c3",
      "text": "Error responses don't leak internal details",
      "category": "security",
      "checked": false,
      "related_findings": [],
      "related_files": ["src/api/routes.rs"]
    },
    {
      "id": "c4",
      "text": "Tests cover happy path and edge cases",
      "category": "test",
      "checked": false,
      "related_findings": [],
      "related_files": ["tests/auth_test.rs"]
    }
  ]
}
```

**In `er`:** Checklist appears in the Side Panel. User presses `x` to toggle items. Checked state is written back to the file so it persists across sessions.

### .er-feedback.json

User comments written by `er` when the user presses `c` on a hunk. Claude reads this file to understand the reviewer's questions and concerns.

```json
{
  "version": 1,
  "diff_hash": "abc123...",
  "comments": [
    {
      "id": "u1",
      "timestamp": "2026-02-24T10:45:00Z",
      "file": "src/auth/session.rs",
      "hunk_index": 2,
      "line_start": 45,
      "line_end": 52,
      "line_content": "    let token = jwt::encode(&header, &claims, &key)?;",
      "comment": "What happens if the key is rotated while active tokens exist? Do we need a key ID (kid) in the header?",
      "in_reply_to": null,
      "resolved": false
    },
    {
      "id": "u2",
      "timestamp": "2026-02-24T10:47:00Z",
      "file": "src/auth/middleware.rs",
      "hunk_index": 0,
      "line_start": 12,
      "line_end": 12,
      "line_content": "    if token.is_expired() { return Err(AuthError::Expired); }",
      "comment": "Should this return a 401 with a specific error code so the client knows to refresh?",
      "in_reply_to": null,
      "resolved": false
    },
    {
      "id": "u3",
      "timestamp": "2026-02-24T11:02:00Z",
      "file": "src/auth/session.rs",
      "hunk_index": 2,
      "line_start": 45,
      "line_end": 52,
      "line_content": "    let token = jwt::encode(&header, &claims, &key)?;",
      "comment": "Thanks, the kid approach makes sense. Can you also check if we need to handle the JWKS rotation endpoint?",
      "in_reply_to": "a1",
      "resolved": false
    }
  ]
}
```

**`in_reply_to`:** If this comment is a follow-up to an AI response, this references the AI answer's ID (from the updated `.er-review.json`). This creates a threaded conversation.

**`line_content`:** The actual diff line the cursor was on when the comment was created. Serves as a human-readable anchor even if line numbers shift.

**`resolved`:** User can press `R` on a comment to mark it resolved. Claude skips resolved comments in the next iteration.

### AI response threading

When Claude processes feedback via `er-questions`, it adds `responses` entries to findings in `.er-review.json`:

```json
{
  "id": "f1",
  "severity": "high",
  "title": "Token expiry not enforced",
  "description": "...",
  "responses": [
    {
      "id": "a1",
      "in_reply_to": "u1",
      "timestamp": "2026-02-24T10:50:00Z",
      "text": "Good catch. Adding a `kid` (Key ID) header is the standard approach for key rotation. The JWT header should include `kid` pointing to the current key's ID, and the validation middleware should look up the key by `kid` from a JWKS endpoint or local keystore.\n\nI've updated finding f1 to include this as an additional recommendation.",
      "new_findings": []
    }
  ]
}
```

This way the conversation is visible in `er`: finding → user comment → AI response → user follow-up.

---

## Claude Code Skills

### er-review (primary)

**Trigger:** User runs `er-review` in Claude Code while in a git repo.

**What it does:**
1. Runs `git diff <base>..HEAD` to get the raw diff
2. Computes `diff_hash = sha256(raw_diff)`
3. Analyzes each file and hunk for risk, bugs, security issues, style
4. Generates all four `.er-*` files:
   - `.er-review.json` — findings and risk levels
   - `.er-order.json` — suggested review order
   - `.er-summary.md` — branch summary
   - `.er-checklist.json` — review checklist
5. If `.er-feedback.json` already exists with unresolved comments, incorporates them into the analysis

**Base branch detection:** Uses the same logic as `er` — checks `git config branch.<current>.merge`, falls back to `main`/`master`.

### er-risk-sort

**Trigger:** `er-risk-sort` — regenerates `.er-order.json` only.

Useful when you want to re-sort the review order without re-running the full review (e.g., after resolving some findings and wanting to reprioritize).

### er-summary

**Trigger:** `er-summary` — regenerates `.er-summary.md` only.

Quick branch summary without the full review. Useful for understanding a branch before deciding whether to do a full AI review.

### er-checklist

**Trigger:** `er-checklist` — regenerates `.er-checklist.json` only.

Creates a targeted checklist. Can accept a focus area (e.g., "security", "performance") to generate domain-specific checklists.

### er-questions (the feedback loop)

**Trigger:** `er-questions` — reads `.er-feedback.json` and updates the review.

**What it does:**
1. Reads `.er-feedback.json` for unresolved comments
2. For each comment:
   - Reads the referenced file and hunk context
   - Generates a response
   - Adds the response to the relevant finding in `.er-review.json`
   - May create new findings if the comment reveals something
   - May update `.er-checklist.json` with new items
3. Preserves all existing data — only appends/updates, never deletes

---

## er TUI Changes

### New keybindings

| Key | Context | Action |
|-----|---------|--------|
| `v` | Normal | Cycle view mode: Default → Overlay → Side Panel → AI Review |
| `V` | Normal | Quick switch: jump directly to a mode (popup selector) |
| `c` | On a hunk | Open comment input for the current hunk |
| `C` | On a file | Open comment input for the entire file |
| `?` | Normal | Toggle summary overlay (if `.er-summary.md` exists) |
| `x` | Checklist item | Toggle checklist item checked/unchecked |
| `R` | On a comment | Toggle comment resolved/unresolved |
| `O` | File tree | Toggle between AI-suggested order and alphabetical |
| `F` | Normal | Show findings list (jump to any finding) |

### Comment input flow

When user presses `c`:

1. `er` opens a text input overlay at the bottom (similar to search input)
2. User types their comment (multi-line support with Shift+Enter or configurable)
3. Enter submits → comment is appended to `.er-feedback.json`
4. Esc cancels
5. The comment appears inline in the diff view immediately (local render, no AI response yet)
6. Bottom bar shows: `Comment saved → run er-questions for AI response`

### File watcher integration

`er` already has a file watcher for diff refresh. Extend it to also watch `.er-*` files:

- On `.er-*.json` mtime change → reload AI data, refresh UI
- On `.er-summary.md` mtime change → reload summary
- Visual flash notification: `✓ AI data refreshed`

### Staleness display

When `diff_hash` doesn't match:
- Top bar shows `⚠ AI review stale` in yellow
- Risk dots in file tree get a `?` suffix (e.g., `●?` instead of `●`)
- Findings that reference hunks that no longer exist show `[stale]` tag
- All AI data still renders (it's useful context) but with reduced opacity/dimmed borders

### View mode behaviors with feedback

**Overlay mode:**
- Inline comment markers appear between hunks: `💬 "What about key rotation?" (you, 10:45)`
- AI responses appear below: `🤖 "Good catch. Adding kid header..." (Claude, 10:50)`
- Unresolved comments have bright left border, resolved are dimmed

**Side Panel mode:**
- Comments section in the AI panel, grouped by hunk
- Threaded view: user comment → AI response → user follow-up
- Resolved comments collapsed by default

**AI Review mode:**
- Finding walkthrough includes the conversation thread
- Finding card shows: description → suggestion → user comments → AI responses
- Next/prev navigation skips resolved conversations

---

## .gitignore

Add to the project's `.gitignore`:

```
.er-review.json
.er-order.json
.er-summary.md
.er-checklist.json
.er-feedback.json
```

These are ephemeral review artifacts, not source code.

---

## Implementation Order

### Phase 1: File reading + Overlay mode
1. Add `.er-review.json` parser to `er` (serde deserialization)
2. Add `diff_hash` computation (SHA-256 of raw diff)
3. Add staleness detection + warning banner
4. Render risk dots in file tree (colored circles based on risk level)
5. Render inline finding banners between hunks in diff view
6. Add `v` keybind to toggle overlay on/off
7. Create the `er-review` Claude Code skill

### Phase 2: Comment system + Feedback loop
8. Add comment input mode (text input overlay)
9. Write comments to `.er-feedback.json`
10. Render user comments inline in diff view
11. Create the `er-questions` Claude Code skill
12. Add AI response rendering (threaded under findings)
13. Add `R` to resolve/unresolve comments

### Phase 3: Side Panel + AI Review mode
14. Implement 3-column layout for Side Panel mode
15. Add `.er-summary.md` rendering in panel header
16. Add `.er-checklist.json` rendering with toggle
17. Implement AI Review guided walkthrough mode
18. Add `.er-order.json` support for file tree sorting

### Phase 4: Remaining skills + polish
19. Create `er-risk-sort`, `er-summary`, `er-checklist` skills
20. Add file watcher for `.er-*` files
21. Add `V` quick mode selector popup
22. Add `F` findings list popup
23. Polish: animations, smooth transitions between modes
