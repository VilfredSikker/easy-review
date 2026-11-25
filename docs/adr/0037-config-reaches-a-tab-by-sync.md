# A tab carries the config it filters by

## Context

Two features resolve a file's verdict from something the reviewer declares rather
than something a model produced: importance tiers (`[importance.<repo>]`) and
watched files (`[watched]`). Both are read while filtering or rendering a diff,
and both live in `ErConfig`.

`ErConfig` was a field on `App`, and `TabState` never held one. That was fine
while every config-driven decision happened on `App`, but the file filter did
not: `TabState::visible_files` and `TabState::filtered_reviewed_count` were the
two call sites, and both were `&self` on the tab.

So the rules had nowhere to be read from. The alternatives were considered in
`crates/er-engine/src/CLAUDE.md`'s terms — the engine holds state and the
operations on it, and a front end that needs something the shared state model
does not express gets an explicit field.

## Decision

A tab carries the config it resolves against, copied on the way in.

- `TabState.risk_rules` held this repo's project risk rules, read through
  `ErConfig::risk_rules`, and `TabState.file_kinds` its `FileKindRepoConfig`, by
  the same path. P03 (1.0.0) removed the `[importance.<repo>]` table and its
  `TabState.importance` field; the rules now live in `[risk.<repo>]`.
- `App::push_tab` filled it when a tab opened, keyed by `storage::rules_key`: the
  name a clone's origin gives the repo. A remote-only tab has no clone, so its
  key comes from the PR's repo name, which is the name a clone would get.
- Every path that installs a tab copies the tables, and every config change
  refreshed them on every open tab (`App::sync_repo_rules_to_tabs`). A tab that
  skips both shows counts and filters from someone else's rules, or none.
- `watched_config` is copied onto tabs the same way, so this follows a shape the
  engine already uses. It differs in which tabs get it: watched config is pushed
  to the active tab when that tab's settings change, while the rule tables go to
  every tab, because a filter in a background tab still has to agree with the
  reviewer's rules.

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

## Since the App left

The App and `TabState` left in 1.0.0 (P77), and with them the per-tab copy.
A tab's published content carries its rules key and the committed risk-rules
artifact, read when the tab loads, and the file list resolves `[file_kinds]`
and `[risk]` from the live config each time it is built (`file_rule_tables`
in `crates/er-service/src/diff_resource.rs`). An importance run that commits
new rules reloads the repo's loaded tabs (`reload_risk_rules`), which is the
one sync left.
