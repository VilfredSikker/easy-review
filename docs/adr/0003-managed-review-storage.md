# Review artifacts live in managed app data, not in the repo

Review sidecars (triage, review.json, questions/notes, github-comments, reviewed, checklist, tour) are written under `<storage_root>/repos/<repo>/branches/<branch>/view-buckets/<bucket>/`, one bucket per view, with the PR bucket kept separate at `<storage_root>/repos/<owner_repo>/prs/pr-<N>/`. Repo folders are `<slugified, lowercased owner-repo, at most 80 bytes>-<8 hex of SHA-256(owner/repo)>`, and branch folders carry the same kind of hash, so two branches whose names slugify alike stay in separate folders. `ER_STORAGE_ROOT` is the root as given; otherwise `storage_root()` is `<base>/v1`, where `<base>` is the platform app-data directory plus `easy-review`. The TUI and the desktop app resolve the same path for the same branch; two front ends that resolve it differently would each review state the other never wrote, while both looked as though they were working. Keeping artifacts out of the tree also means no `.gitignore` entry and no diff noise in the repo under review.

## The `v1` segment

Data written before 1.0.0 sits directly under `<base>`. It is left where it is and never read: a pre-1.0 `config.toml` has keys this version deletes, and reading it in place would greet every upgrading user with an error. The first launch that finds entries other than `v1/` under `<base>` shows one notice naming `<base>` and its old `config.toml`, then writes `<root>/.legacy-root-noticed`. Either front end writes the same marker, so the notice appears once per machine. `er uninstall` targets `<base>`, not `v1/`, so both the old data and the current data go.

## Consequences

A new sidecar must resolve its directory through `crates/er-engine/src/storage.rs`; a hand-built path is how the two front ends drift apart.
