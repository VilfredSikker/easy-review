# git/ — Git Operations

Pure diff parsing + shelling out to git. No application state, no UI.

## Files

| File | Lines | Purpose |
|------|-------|---------|
| `mod.rs` | ~14 | Re-exports public types and functions |
| `diff.rs` | ~717 | `parse_diff()` — unified diff text to structured data |
| `status.rs` | ~527 | All git commands (diff, staging, branches, worktrees) |

## diff.rs — Parser

**Input:** Raw unified diff text from `git diff`.
**Output:** `Vec<DiffFile>`, each containing `Vec<DiffHunk>`, each containing `Vec<DiffLine>`.

Key types:
- `DiffFile` — `{ path, status: FileStatus, hunks, adds, dels }`
- `DiffHunk` — `{ header, old_start, old_count, new_start, new_count, lines }`
- `DiffLine` — `{ line_type: LineType, content, old_num, new_num }`
- `FileStatus` — `Added | Modified | Deleted | Renamed(String) | Copied(String)`

The parser is a line-by-line state machine. It handles: `diff --git` headers, `new file`/`deleted file`/`rename from`, `@@` hunk headers, and content lines (`+`/`-`/space). Skips `index`, `---`, `+++`, `similarity index`, mode lines, and `\ No newline at end of file`.

Has extensive unit tests covering edge cases (renames, mode-only, no-newline markers, multi-hunk files).

## status.rs — Git Commands

All git commands pass `--no-color` and `--no-ext-diff` (prevents difftastic/delta from intercepting).

Key functions:
- `get_repo_root() / get_repo_root_in(dir)` — `git rev-parse --show-toplevel`
- `get_current_branch_in(repo_root)` — `git rev-parse --abbrev-ref HEAD`
- `detect_base_branch_in(repo_root)` — fallback chain: upstream tracking → main → master → develop → dev → origin/*
- `git_diff_raw(mode, base, repo_root)` — runs `git diff` with mode-specific args
- `git_stage_file / git_unstage_file` — `git add` / `git reset HEAD`
- `git_stage_hunk(path, hunk, repo_root)` — builds a minimal patch via `reconstruct_hunk_patch()` and pipes to `git apply --cached --unidiff-zero`
- `list_worktrees(repo_root)` — parses `git worktree list --porcelain`

Debug logging: when `$ER_DEBUG` is set, `git_diff_raw` writes the raw command and output to `/tmp/er_debug.log`.

## Important Patterns

- `detect_base_branch_impl` uses a closure for running git commands — avoids code duplication between `_in` and non-`_in` variants.
- Hunk staging constructs a full patch (with `diff --git` header, `---/+++`, and hunk) then pipes via stdin — this is how `git add -p` works internally.
