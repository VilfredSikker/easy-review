# app/ — application state

All review state, and the operations on it. No rendering, no event loop, no
async: the front ends read this state and draw it.

## Ownership boundary

`TabState` is one review target — working tree, branch view, local PR, or remote
PR. `App` owns what has to survive a tab switch: the open tabs and active index,
the AI provider/model selection, and app-level background review tasks that keep
running while the user navigates elsewhere. Caches that exist only for the
desktop (GitHub list/status caches, loading flags, terminal sessions) belong to
`crates/er-desktop`'s `AppState`. Which side of the line a new field goes on is
`crates/er-engine/src/app/state/agent.md`.

Never widen an existing field so a second front end can read something else out
of it. The engine compiles for both, so nothing flags it, and it breaks at
runtime in whichever one the change was not written for.
`docs/adr/0006-engine-state-is-the-ui-contract.md`.

## Persistence

Sidecars resolve through `crates/er-engine/src/storage.rs`: one bucket per view
under `<storage_root>/repos/<repo>/branches/<branch>/`, with PR artifacts
separately at `<storage_root>/repos/<repo>/prs/pr-<N>/`. `storage_root` is
`$ER_STORAGE_ROOT` as given, else the platform app-data dir plus
`easy-review/v1`; an older install's data stays beside it and is not read
(ADR 0003). Repo and branch folders carry a hash suffix, so build them with
`repo_storage_slug_from_root`/`slug_branch`, never by concatenation.

| Sidecar | Resolves to |
|---|---|
| `reviewed.json`, `questions.json`, `notes.json`, `checklist.json`, `snapshots/` | the active view bucket |
| `github-comments.json` | the PR bucket, whatever view is active |

Never build one of these paths by hand. `reviewed.json` is deleted rather than emptied
when nothing is reviewed. Scoping exceptions and why they exist:
`docs/adr/0003-managed-review-storage.md`,
`docs/adr/0004-per-view-artifact-scoping.md`,
`docs/adr/0007-three-comment-stores.md`.

Config is global-only at `<storage_root>/config.toml` (under `easy-review/v1/`).
`~/.config/er/config.toml` and the pre-1.0 `<base>/config.toml` are not read.
There is no per-repo config file; do not add one back.
`docs/adr/0005-global-only-config.md`.

Nothing imports a repo `.er/`; review files always live under managed storage.

## Traps

- **Switching mode can change the bucket.** `set_mode` reloads the managed root,
  `reviewed` and the AI state when it does, and that reload must run before the
  diff refresh and the selection restore — the restore is what clamps the cursor
  into the new file list.
- **`gh stack` lookups never run inline.** They shell out to GitHub, so opening
  the Open hub only requests the lookup; a worker thread runs it and the result
  lands on a later tick, applied only while the tab still carries the
  `request_seq` it was requested under. A lookup that finished after the tab
  moved, closed or was re-requested is dropped.
- **`gh stack view` only describes the checked-out branch.** Run it only when
  the tab views that checkout; a PR tab without one rebuilds its stack from the
  repo's open PRs instead. Running it anywhere else shows some other branch's
  stack.
