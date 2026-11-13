# Importance is a repo-scoped RiskRules artifact

## Context

ADR 0048 had the importance job merge its table into `[risk.<repo>]` in
`config.toml`. That let a job write the config file, made a generated table
indistinguishable from one the user wrote, and left no way to replace the
generated part without replacing the user's overrides with it. The phase-3
store now has typed, enveloped kinds and a repo scope, so the generated table
can live beside the repo instead of in the user's file.

## Decision

The importance worker commits a `RiskRulesArtifact` (file `risk-rules.json`,
kind `risk-rules`, enveloped) under `Scope::Repo`, at
`repos/<repo>/risk-rules.json`. The worker validates the proposal and refuses
one naming another repo; it writes only through the store. No job writes
`config.toml`.

Reads merge the artifact under `[risk.<repo>]`: a user rule wins for any key it
names, and the user's default wins when it set one; the artifact fills the
gaps. `[user_risk]` stays a separate table, consulted after the project table.
The merged table is what `RiskRules.project` holds, so filters, the header,
Settings and change facts read one path and need no new precedence rule.

Settings edits keep writing `[risk]`, which is now the user's overlay. There is
no editor for the artifact. A table an earlier importance run merged into
`[risk]` stays there and counts as user config; no migration removes it,
because an old agent write and a user edit are indistinguishable on disk, and
the user may have edited it since.

This supersedes ADR 0048's merge-into-config step. Its rule system, its
precedence within a table and its per-repo keying stand.

## Consequences

- A later run replaces the generated table without touching the user's config,
  and a user override survives every run.
- The artifact is readable by any process through the store, like the other
  kinds; `config.toml` no longer changes when importance runs.
- Existing `[risk]` entries written by the old job keep winning until the user
  clears them.
