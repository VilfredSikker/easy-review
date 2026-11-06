# Review artifacts live in managed app data, not in the repo

Review sidecars (triage, review.json, questions/notes, github-comments, reviewed, checklist, tour) are written under `<storage_root>/repos/<repo>/branches/<branch>/view-buckets/<bucket>/`, one bucket per view, with the PR bucket kept separate at `<storage_root>/repos/<owner_repo>/prs/pr-<N>/` — `storage_root()` being `$ER_STORAGE_ROOT` or the platform app-data directory plus `easy-review`. The TUI and the desktop app resolve the same path for the same branch; two front ends that resolve it differently would each review state the other never wrote, while both looked as though they were working. Keeping artifacts out of the tree also means no `.gitignore` entry and no diff noise in the repo under review.

## Consequences

A new sidecar must resolve its directory through `crates/er-engine/src/storage.rs`; a hand-built path is how the two front ends drift apart.
