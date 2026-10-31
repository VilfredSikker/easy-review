# git/ — git subprocess and diff parsing

Everything that spawns `git` or reads `git diff` output. No application state,
no UI: this module returns data and the caller decides what it means.

## Boundaries

- Git is a subprocess, never a linked library. No `git2`/`gix` dependency, and
  don't add one. `git`'s behaviour is the spec, so a disagreement between `er`
  and `git` is a bug in `er`. `docs/adr/0001-shell-out-to-git.md`.
- Within this module, git is spawned from `status.rs`, except `blame.rs`, which
  keeps `git blame` and the range lookups it needs beside its porcelain parser.
  A new git command goes into one of these, not into the caller.
- `diff.rs` parses text and spawns nothing.

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

## Every invocation passes `--no-color` and `--no-ext-diff`

Without them a user with difftastic or delta configured gets output the parser
cannot read. Any new call site passes both.
