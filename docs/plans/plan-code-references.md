# Code References

## Overview

When reviewing a diff, you often see a function, type, or variable being changed and want to know: where else is this used? A code references feature lets the user place their cursor on a symbol and instantly see all references across the codebase — without leaving `er`.

This is the TUI equivalent of "Find All References" in an IDE, but designed for code review context: you're not editing, you're understanding impact.

---

## 1. Trigger & Flow

### Activation

**Key:** `g` (go to references) when the cursor is on a diff line (line-level navigation active via ↑/↓).

### Flow

1. User navigates to a line with ↑/↓ (enters line mode)
2. Presses `g`
3. `er` extracts the most likely symbol from the current line (see §3)
4. Runs `git grep` across the repo for that symbol
5. Opens a **references overlay** showing all matches grouped by file
6. User can navigate matches with j/k, press Enter to jump to that file in a pager, or Esc to dismiss

### Quick symbol pick

If the line contains multiple plausible symbols, `er` picks the most "interesting" one using heuristics (see §3). If the user wants a different symbol, they can press `g` again to cycle, or `G` to type a custom search term.

---

## 2. References Overlay

A modal popup (similar to the existing worktree picker overlay) that fills most of the screen:

```
┌─ References: validate_token_expiry ─────────────────────────┐
│                                                              │
│  src/auth.rs                                                 │
│  ● 45: + validate_token_expiry(&token)?;           ← (diff) │
│    12:   pub fn validate_token_expiry(token: &str)           │
│    89:   validate_token_expiry(&refresh_token)?;             │
│                                                              │
│  src/middleware.rs                                            │
│    34:   if validate_token_expiry(&t).is_err() {             │
│    67:   // TODO: validate_token_expiry for refresh           │
│                                                              │
│  src/tests/auth_test.rs                                      │
│    15:   validate_token_expiry("valid-token")?;              │
│    28:   validate_token_expiry("expired-token")              │
│    45:   validate_token_expiry("")                            │
│                                                              │
│  7 references in 3 files                                     │
│                                                              │
│  j/k: navigate  Enter: open in $EDITOR  Esc: close          │
└──────────────────────────────────────────────────────────────┘
```

### Layout

- **Title bar:** Shows the searched symbol
- **Results:** Grouped by file, each match shows line number + line content
- **Origin marker:** The diff line that triggered the search is marked with `●` and `(diff)`
- **Footer:** Total count + keybind hints

### Styling

- File headers: bold, slightly brighter
- Line numbers: dimmed, right-aligned
- Match highlight: the symbol within each line is highlighted (bold or colored)
- Selected result: background highlight (same accent as file selection)
- Lines from the current diff: subtly marked to distinguish "in this PR" from "existing code"

---

## 3. Symbol Extraction

### Strategy: line-aware heuristic

Given a diff line like `+ validate_token_expiry(&token)?;`, extract the most "interesting" symbol.

```rust
pub fn extract_symbol(line: &str, language_hint: Option<&str>) -> Option<String> {
    let clean = line.trim_start_matches(|c: char| c == '+' || c == '-' || c == ' ');

    // Strategy 1: If the line is a function/method definition, use the function name
    // Patterns: fn foo, def foo, function foo, pub fn foo, async fn foo, etc.
    if let Some(name) = extract_function_def(clean, language_hint) {
        return Some(name);
    }

    // Strategy 2: If the line is a function call, use the called function
    // Pattern: identifier followed by (
    if let Some(name) = extract_function_call(clean) {
        return Some(name);
    }

    // Strategy 3: If the line is a type/struct/class definition
    // Patterns: struct Foo, class Foo, type Foo, interface Foo
    if let Some(name) = extract_type_def(clean, language_hint) {
        return Some(name);
    }

    // Strategy 4: If the line is an import/use statement
    // Patterns: use foo::bar, import { Foo }, from 'foo' import bar
    if let Some(name) = extract_import(clean, language_hint) {
        return Some(name);
    }

    // Strategy 5: Longest identifier-like token that's not a keyword
    extract_longest_identifier(clean, language_hint)
}
```

### Language-specific patterns

Use the file extension to determine language. Common patterns:

| Language | Function def | Type def | Import |
|----------|-------------|----------|--------|
| Rust | `fn name`, `pub fn name` | `struct Name`, `enum Name`, `trait Name` | `use path::Name` |
| Python | `def name` | `class Name` | `from x import name`, `import name` |
| JS/TS | `function name`, `const name =` | `class Name`, `interface Name`, `type Name` | `import { Name }` |
| Go | `func name`, `func (r) name` | `type Name struct` | `import "path"` |
| Java | `void name(`, `public Type name(` | `class Name`, `interface Name` | `import path.Name` |

### Keyword exclusion

Don't offer common keywords as symbols:

```rust
const COMMON_KEYWORDS: &[&str] = &[
    "if", "else", "for", "while", "return", "let", "const", "var",
    "fn", "pub", "struct", "enum", "impl", "use", "mod", "trait",
    "def", "class", "import", "from", "function", "async", "await",
    "true", "false", "null", "None", "self", "this", "super",
];
```

### Fallback

If no symbol can be extracted, show a text input for manual entry (same as pressing `G`).

---

## 4. Search Backend

### Primary: `git grep`

Fast, respects `.gitignore`, available everywhere git is:

```rust
pub fn git_grep_symbol(symbol: &str, repo_root: &str) -> Result<Vec<GrepMatch>> {
    let output = Command::new("git")
        .args([
            "grep",
            "-n",              // Line numbers
            "--no-color",
            "-w",              // Word boundary matching
            "-I",              // Skip binary files
            symbol,
        ])
        .current_dir(repo_root)
        .output()?;

    parse_grep_output(&String::from_utf8_lossy(&output.stdout))
}

#[derive(Debug, Clone)]
pub struct GrepMatch {
    pub file: String,
    pub line_num: usize,
    pub content: String,
    pub is_in_diff: bool,  // Whether this file is part of the current diff
}
```

### Word boundaries

`-w` flag ensures `token` doesn't match `token_expiry`. This is important — without it, short identifiers would produce too many false matches.

### Performance

`git grep` is very fast (uses git's internal index). On a 100K-line repo:
- Simple symbol: ~50ms
- Common symbol (100+ matches): ~200ms
- Maximum: cap results at 200 matches to prevent UI overload

```rust
// In git_grep_symbol, add --max-count for safety
.args(["grep", "-n", "--no-color", "-w", "-I", "--max-count=200", symbol])
```

### Fallback: ripgrep

If available, prefer `rg` for non-git files or faster results:

```rust
pub fn rg_symbol(symbol: &str, repo_root: &str) -> Result<Vec<GrepMatch>> {
    let output = Command::new("rg")
        .args([
            "-n",          // Line numbers
            "--no-heading",
            "-w",          // Word boundary
            "-I",          // No binary
            "--max-count", "200",
            symbol,
        ])
        .current_dir(repo_root)
        .output();

    match output {
        Ok(out) => parse_rg_output(&String::from_utf8_lossy(&out.stdout)),
        Err(_) => git_grep_symbol(symbol, repo_root),  // Fallback to git grep
    }
}
```

---

## 5. Result Grouping & Sorting

### Group by file

Results are grouped by file path. Within each file, matches are sorted by line number.

### File ordering priority

1. **Files in the current diff** — shown first (most relevant for review context)
2. **Test files** — shown last (pattern: `*test*`, `*spec*`, `__tests__/`)
3. **Everything else** — sorted alphabetically

### Diff awareness

Mark results that fall within the current diff:

```rust
fn annotate_diff_matches(matches: &mut Vec<GrepMatch>, diff_files: &[DiffFile]) {
    let diff_paths: HashSet<&str> = diff_files.iter().map(|f| f.path.as_str()).collect();
    for m in matches.iter_mut() {
        m.is_in_diff = diff_paths.contains(m.file.as_str());
    }
}
```

In the overlay, diff files get a subtle marker: `(in diff)` next to the file header.

---

## 6. Navigation Within the Overlay

| Key | Action |
|-----|--------|
| `j` / `↓` | Next match |
| `k` / `↑` | Previous match |
| `n` | Next file group |
| `N` | Previous file group |
| `Enter` | Open file at line in `$EDITOR` (same as existing `e` key behavior) |
| `Esc` | Close overlay |
| `g` | Cycle to next symbol candidate on the original line |
| `G` | Manual symbol input (text prompt) |
| `/` | Filter results by filename |

### Enter behavior

Opens the file at the matched line in the user's `$EDITOR` (or `$VISUAL`), same as the existing `e` key in diff view:

```rust
fn open_in_editor(file: &str, line: usize, repo_root: &str) -> Result<()> {
    let editor = std::env::var("VISUAL")
        .or_else(|_| std::env::var("EDITOR"))
        .unwrap_or_else(|_| "vim".to_string());

    Command::new(&editor)
        .arg(format!("+{}", line))
        .arg(file)
        .current_dir(repo_root)
        .status()?;
    Ok(())
}
```

---

## 7. Data Model

### Overlay state

```rust
#[derive(Debug, Clone)]
pub struct ReferencesState {
    pub symbol: String,
    pub matches: Vec<GrepMatch>,
    pub grouped: Vec<FileGroup>,
    pub selected: usize,          // Flat index across all matches
    pub scroll: u16,
    pub filter: String,           // Optional filename filter
}

#[derive(Debug, Clone)]
pub struct FileGroup {
    pub file: String,
    pub is_in_diff: bool,
    pub matches: Vec<GrepMatch>,
}
```

### Integration with OverlayData

The existing `OverlayData` enum handles modal popups:

```rust
pub enum OverlayData {
    // ...existing variants...
    References(ReferencesState),
}
```

When `OverlayData::References` is active, the overlay renders on top of the diff view (same pattern as worktree picker).

---

## 8. Keybind Summary

| Key | Context | Action |
|-----|---------|--------|
| `g` | Line mode (cursor on a diff line) | Find references for extracted symbol |
| `g` | Inside references overlay | Cycle to next symbol candidate |
| `G` | Line mode or overlay | Manual symbol search (text input) |
| `Esc` | References overlay | Close |

---

## 9. Edge Cases

### No matches

```
  References: unknown_symbol
  No references found in the repository.
  Press G to search for a different term, or Esc to close.
```

### Too many matches

If > 200 matches, show a truncation notice:

```
  200+ references found. Showing first 200.
  Press G to refine your search term.
```

### Binary/generated files

`git grep -I` skips binary files. For generated files (like `.min.js`), they'll appear in results but can be filtered with `/` if noisy.

### Symbol not on current line

If the cursor line is a blank context line or a comment with no identifiers, show:

```
  No symbol found on current line. Press G to search manually.
```

---

## Implementation Steps

1. **Symbol extraction** — `extract_symbol()` with language-aware heuristics, keyword exclusion
2. **Git grep integration** — `git_grep_symbol()` in `src/git/status.rs`, `GrepMatch` struct, result parser
3. **References overlay** — New `OverlayData::References` variant, rendering with file grouping + match highlighting
4. **Overlay navigation** — j/k for matches, n/N for file groups, Enter to open in editor
5. **Diff awareness** — Mark matches in diff files, sort diff files first
6. **`g` keybind** — Extract symbol from current line, trigger grep, open overlay
7. **`G` manual search** — Text input mode for custom symbol
8. **Symbol cycling** — Multiple candidates per line, `g` cycles through them

## Files Changed

| File | Change |
|------|--------|
| `src/git/status.rs` | `git_grep_symbol()`, `GrepMatch`, grep output parser |
| `src/app/state.rs` | `ReferencesState`, `FileGroup`, `OverlayData::References`, symbol extraction logic |
| `src/ui/overlay.rs` | References overlay rendering (file groups, match highlighting, scroll) |
| `src/main.rs` | `g`/`G` keybinds, overlay input handling (j/k/n/N/Enter/Esc within references) |
| `src/ui/status_bar.rs` | Hint line when overlay is open |
