# A tab carries the config it filters by

## Context

Two features resolve a file's verdict from something the reviewer declares rather
than something a model produced: importance tiers (`[importance.<repo>]`) and
watched files (`[watched]`). Both are read while filtering or rendering a diff,
and both live in `ErConfig`.

`ErConfig` is a field on `App`. `TabState` has never held one. That was fine
while every config-driven decision happened on `App`, but the file filter does
not: `TabState::visible_files` and `TabState::filtered_reviewed_count` are the
two call sites, and both are `&self` on the tab.

So the rules had nowhere to be read from. The alternatives were considered in
`crates/er-engine/src/CLAUDE.md`'s terms — the engine holds state and the
operations on it, and a front end that needs something the shared state model
does not express gets an explicit field.

## Decision

A tab carries the config it resolves against, copied on the way in.

- `TabState.importance` holds this repo's `ImportanceRepoConfig`.
- `App::push_tab` fills it when a tab opens, keyed by
  `storage::slug_repo(repo_root)` — the same key managed storage uses, so the
  rule table and the bucket agree on what a repo is called.
- `App::sync_importance_to_tabs` refreshes every open tab, and is called where
  the config changes: the desktop's `apply_config_side_effects`, the TUI's
  `config_hub_persist_live`.
- `watched_config` is copied onto tabs the same way, so this follows a shape the
  engine already uses. It differs in reach: watched config is pushed to the
  active tab when that tab's settings change, while importance goes to every tab,
  because a filter in a background tab still has to agree with the reviewer's
  rules.

## Consequences

**Good.** The filter stays a pure function of tab state, which is what makes it
testable without an `App`. `visible_files` keeps its signature, so neither front
end changed. And a tab now filters the way its own repo's config says, which is
the only behaviour that makes sense in a multi-repo window.

**Costs.** A config change is only visible to tabs after something calls the
sync — a stale copy is possible in principle, so every new config-writing path
has to remember it. The alternative, passing `&ErConfig` down into the filter,
would put a config parameter on every front-end call site and make the engine's
filter depend on the app's whole configuration.

**Rejected: resolving in the front ends.** Both front ends would have to resolve
a path to a tier with the same precedence rules, and the TUI's filter expression
would stop agreeing with the desktop's file list.
