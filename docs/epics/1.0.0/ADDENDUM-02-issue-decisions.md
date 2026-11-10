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
- **D15: 1.0.0 imports the project list once** (owner, 2026-11-09). This is the only exception to "1.0.0 carries no old data". When the projects record is absent, the app reads the old `<config_dir>/er/projects.json` once, writes it into the store under the lock, and never touches the old file. Reviews, caches, tabs and inbox state stay behind as planned. Reason: the owner has a real project list, and losing projects, pins and tracked branches on upgrade is the one left-behind item a user would notice. Tracked as #452, after P22 PR 1 (#449).
- **D16: agent settings move to `agents.toml`** (owner, 2026-11-10). Agent and provider settings (CLIs, models, providers) leave `config.toml` for their own `agents.toml` next to it, in the managed config location (ADR 0005, D6), with the provider catalog compiled into the app. `config.toml` keeps everything else. Phase 3 creates the file.
- **D17: panel round two is measured by a free proxy** (owner, 2026-11-10). Each `PanelRun` records whether round 2 changed the `quorum()` outcome against the round-1 votes. No extra arbiter call is made. The default round count stays as it is until the owner decides from the data.
- **D18: `[commands].summary` becomes a built-in job** (owner, 2026-11-10). The summary runs as an ordinary agent job through `er-agent`, like card AI, and commits its output through the store. The `[commands].summary` key is retired, and the first-launch notice tells the user.
