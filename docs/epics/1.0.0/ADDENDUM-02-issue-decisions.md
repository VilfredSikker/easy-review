# Addendum 02: decisions from the issue enrichment pass

Recorded 2026-11-06, by the owner. Where this file disagrees with [PLAN.md](./PLAN.md), [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md) or an issue body, this file wins.

- **D11: `github.rs`'s git calls move in P08.** `crates/er-engine/src/github.rs` has 19 production `git` spawns that no plan entry owned. P08 (#379) adds the `Git` methods they need and migrates them. The `er::git_spawn` allow-list is empty after P09 (#380), as planned, and P11 (#382) moves code that already calls the port. P08 may grow to L. If it passes about 1,500 lines, split the `github.rs` part into a PR stacked on P08.
- **D12: P22 takes the full fix.** #393's plan correction is accepted as written:
  - One typed `ProjectsFile` in the engine is the single writer of projects. The desktop's view logic calls it, and the MCP copy of the type is deleted.
  - All six callers of `er-desktop/src/persist.rs` move to the store, including tabs and the two caches the plan did not name (`pr_open_cache.rs`, `gh_status_cache.rs`). After that, `persist.rs` and every write to `<config_dir>/er/` are gone.
  - Expect a two-PR split.
- **D13: P13's three behaviour changes are accepted.** `valid_remote_slug` accepts PR and tree URLs. `remote_matches_repo` becomes exact and case-insensitive, so `owner/repo-fork` no longer matches `owner/repo`. `parse_remote_url("owner/repo")` returns `Some`. The rule table in #384 stands.
- **D14: the store locks with `std::fs::File::lock`.**
  - The workspace `rust-version` rises from 1.88 to the pinned toolchain (`rust-toolchain.toml`, 1.98.0). The owner prefers current dependencies for stability, so the epic also gets one dependency-upgrade PR, P25, in phase 0.
  - P25 does the upgrade before the new crates exist. Everything after it builds on current versions, and it is the only PR whose diff is mostly a lockfile.
  - P25 blocks P07 (#378) and P23 (#394).
