# er-git — the git port

Everything that spawns `git` lives here. Callers hold a `Git` handle instead of
a free function, so a test can answer with `FakeGit` and needs no repository.
`docs/adr/0054-git-is-a-port.md`.

## Boundaries

- Git is a subprocess, never a linked library. No `git2`/`gix` dependency, and
  don't add one. `git`'s behaviour is the spec, so a disagreement between `er`
  and `git` is a bug in `er`. `docs/adr/0001-shell-out-to-git.md`.
- A new git command is a new `Git` trait method in `cli.rs`, its `CliGit`
  forward, its `FakeGit` slot and a `CliGit` test, all in the PR that needs it.
  Never spawn `git` from a caller: the `er::git_spawn` gate fails the build,
  and there is no allow-list, so every spawn outside this crate is an error.
- Every invocation passes `--no-color` and `--no-ext-diff`. Without them a user
  with difftastic or delta configured gets output the parser cannot read.

## The handle

The git handle travels as `er_engine::ports::Ports { pub git: Arc<dyn Git> }`,
passed as the last constructor parameter, cloned into background inputs, and
replaced with `Ports::for_test()` only by test constructors. There is no global,
and no `Default` on that path.
