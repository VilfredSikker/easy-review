# Review artifacts live in managed app data, not in the repo

Review sidecars (triage, review.json, questions/notes, github-comments, reviewed, checklist, tour) are written under `<storage_root>/repos/<repo>/branches/<branch>/view-buckets/<bucket>/`, one bucket per view, with the PR bucket kept separate at `<storage_root>/repos/<owner_repo>/prs/pr-<N>/` — `storage_root()` being `$ER_STORAGE_ROOT` or the platform app-data directory plus `easy-review`. The TUI and the desktop app resolve the same path for the same branch; two front ends that resolve it differently would each review state the other never wrote, while both looked as though they were working. Keeping artifacts out of the tree also means no `.gitignore` entry and no diff noise in the repo under review. `ER_REPO_LOCAL=1` opts back into repo `.er/` for debugging.

The one-time migration from repo `.er/` that earlier docs describe never ran: `migrate_into_managed` and its helpers have no production callers, so nothing imports a repo `.er/` automatically. Record the migration as unreachable rather than as a feature — if you find the functions still in `storage.rs`, they are awaiting removal, and a new caller would resurrect behaviour no one has exercised.

## Consequences

Repo-local `.er/` is still a live code path behind `ER_REPO_LOCAL`, so a new sidecar must resolve its directory through `crates/er-engine/src/storage.rs` and must not build a path by hand — a hand-built path is how the two front ends drift apart again.
