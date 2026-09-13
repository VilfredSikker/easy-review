# CLAUDE.md — easy-review (`er`)

## What this is

A terminal TUI for reviewing git diffs, built for the case where an AI writes
code faster than a person can read it. Fast, navigable, live-updating — the
point is following AI-generated changes as they land. A Tauri/Svelte desktop app
shares the same `er-engine` core and reviews the same sidecars.

Binary name is `er`. Run it from any git repo.

## Where the reasoning is written down

- **`CONTEXT.md`** — what this project's words mean. Read it before naming
  anything, and use its vocabulary rather than inventing a synonym.
- **`docs/adr/`** — why the system is shaped the way it is. Read the ADRs that
  touch the area you are about to change. Add one when a decision is hard to
  reverse, would surprise a reader, or came from a real trade-off.
- **`docs/DEVELOPMENT.md`** — build, test, and release mechanics.
- **`docs/agents/writing-docs.md`** — the rules for writing and maintaining any
  of the above. Read it before editing a doc.
- **`AGENTS.md`** — routing for tools other than Claude Code.

The code carries what and how; these files carry why. A comment that restates
what the code does is noise — the code already says it, and the restatement is
what rots. Run `just docs-check` after editing docs: it verifies every file,
link and ADR reference still resolves.

## Build & run

```bash
just install     # cargo install --path crates/er-tui → ~/.cargo/bin/er
just run         # dev build, run from the current git repo
just test        # er-engine + er-tui
```

Prefer `just` — it is a thin front-end over `scripts/`, which own the split
target dirs (`target/tui` vs `target/desktop`, so a Tauri build never bloats the
TUI's incremental cache) and the automatic cargo-gc.

`cargo install --path .` does **not** work: the root `Cargo.toml` is a virtual
manifest with no package, so cargo refuses before building anything.

The TUI needs a real terminal; running it without one is not a crash worth
debugging.

## Branching & release

- **Bug fixes** go to `main`.
- **Everything else** is developed on, and merged into, the current release
  branch. Resolve it rather than hardcoding it — a pinned version is the one
  that just shipped by the time anyone reads it:

  ```bash
  git fetch origin --quiet   # remote refs go stale, and a failed fetch is silent
  git for-each-ref --format='%(refname:short)' 'refs/remotes/origin/release/v*' \
    | sort -V | tail -1
  ```

- Release branches ship with release notes.

This is intent, not an enforced gate — CI does not check the change type, and
docs and release chores do land on `main`.

## Things that will bite

- **Config is global-only and lives in managed storage** — `<app data>/easy-review/config.toml`
  (on macOS `~/Library/Application Support/easy-review/config.toml`). There is no
  repo-local `.er-config.toml`; do not reintroduce one, and do not add a reload
  path that overwrites live config. ADR 0005 records what those two cost.
- **Agent slots do not cover every spawn path.** The cap covers the queued review
  and arena paths; the AI Hub, card AI, `spawn_command` and model discovery spawn
  without acquiring one. See ADR 0021 before describing the cap as global.
- **The desktop is push-driven.** The backend emits a revision event; the 30s
  timer is only a safety net. Fix revision invalidation before touching a timer.
- **One `er` instance per worktree.** Multi-worktree tabs work through the
  worktree picker.
- **`main` and `release/v*` can differ** — check the branch you are actually on.

The TUI binary is self-contained, but AI actions spawn the configured agent CLI,
clipboard actions shell out to the platform tool, and the desktop embeds an HTTP
client for its browser proxy. "No runtime dependencies beyond git" holds for the
diff-review loop only.

## Design principles

1. Information density over whitespace.
2. Semantic color, not decorative color.
3. Contrast creates hierarchy.
4. Speed is a feature — never trade performance for aesthetics.
5. Every pixel earns its place.

Theme tokens and the semantic color roles are in `crates/er-tui/src/ui/themes.rs`
and `desktop-ui/src/lib/themes.ts`; both front ends share one token set
(ADR 0030).

## Agent skills

The skill pack lives in `.agents/skills/` (from `mattpocock/skills`,
project-scoped for Cursor and cloud). Update with `npx skills update`.

Issues and specs live as markdown under `.scratch/<feature-slug>/`, with triage
labels recorded as a `Status:` line in each file. See `docs/agents/`.
