# git/ — diff parsing and the er-git shims

`diff.rs`, `diff_stats.rs` and `file_kind.rs` parse `git diff` text and spawn
nothing. The git subprocess code lives in `crates/er-git` behind the `Git` trait
(`crates/er-git/CLAUDE.md`, `docs/adr/0054-git-is-a-port.md`); the re-exports
here keep `er_engine::git::…` resolving for callers the later migration PRs have
not moved yet.

## Parser contract

Input is raw unified diff text from `git diff`; output is the structured tree
(`DiffFile` → `DiffHunk` → `DiffLine`). A line-by-line state machine over
`diff --git`, `new file`/`deleted file`/`rename from`, `@@` headers and
`+`/`-`/space content; it reads the `similarity index` percentage onto the
file and skips `index`, `---`, `+++`, mode lines, and `\ No newline at end of file`.

A hunk's `lines` describe the diff, not the file: `Fold(n)` is synthetic,
inserted by the in-process context fold, carrying no line numbers and standing
for `n` context lines the fold removed.

Parser tests live in `diff.rs`.
