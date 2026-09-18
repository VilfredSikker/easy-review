# Shell out to git rather than link a git library

Every git read and write is a `git` subprocess. No gitoxide, `gix`, or `git2` dependency exists in this workspace, and none ever has. Git already handles every edge case the tool walks into, it is installed wherever `er` runs (the tool only works inside a git repo in the first place), and spawning it keeps the binary free of runtime dependencies — no C toolchain, no vendored object-database implementation to keep in step with the user's actual git. The price is a process spawn per operation plus a text parser instead of a typed object model, and the escape hatch was recorded as "optimize later if profiling shows it matters". Profiling has not shown it does.

## Considered Options

Linking a Rust git library (`git2`/libgit2 or `gix`) would drop the subprocess cost and give structured access to the object database. Rejected: it adds a build dependency and a second implementation of git's semantics, so a difference between the library and the user's real `git` becomes a bug in `er` that git cannot reproduce. The subprocess cost is not the bottleneck — the expensive parts of a refresh are parsing and rendering, and those are addressed separately.

## Consequences

`git`'s behaviour is the spec: when `er` and `git` disagree, `er` is wrong. Diff parsing runs against `git diff`'s text output, so the parser in `git/diff.rs` carries that compatibility burden, and every invocation must pass `--no-color` and `--no-ext-diff` — a user with difftastic or delta configured would otherwise get output the parser cannot read.
