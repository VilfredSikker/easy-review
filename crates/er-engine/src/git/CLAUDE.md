# git/ — git subprocess and diff parsing

Everything that spawns `git` or reads `git diff` output. No application state,
no UI: this module returns data and the caller decides what it means.

## Boundaries

- Git is a subprocess, never a linked library. No `git2`/`gix` dependency, and
  don't add one. `git`'s behaviour is the spec, so a disagreement between `er`
  and `git` is a bug in `er`. `docs/adr/0001-shell-out-to-git.md`.
- Every git subprocess in the workspace is spawned from `status.rs`. A new git
  command goes there, not in the caller.
- `diff.rs` parses text and spawns nothing.

## Parser contract

Input is raw unified diff text from `git diff`; output is the structured tree
(`DiffFile` → `DiffHunk` → `DiffLine`). A line-by-line state machine over
`diff --git`, `new file`/`deleted file`/`rename from`, `@@` headers and
`+`/`-`/space content; it skips `index`, `---`, `+++`, mode and similarity
lines, and `\ No newline at end of file`.

A hunk's `lines` describe the diff, not the file: `Fold(n)` is synthetic,
inserted by the in-process context fold, carrying no line numbers and standing
for `n` context lines the fold removed.

Parser tests live in `diff.rs`.

## Every invocation passes `--no-color` and `--no-ext-diff`

Without them a user with difftastic or delta configured gets output the parser
cannot read. Any new call site passes both.
