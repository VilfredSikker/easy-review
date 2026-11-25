# The settings table is the schema, and app-scoped state has one owner

## Context

The Settings page renders from a hand-declared table of rows
(`settings_fields_grouped`). The table had drifted from the config types: some
leaves had no row and no setter, so the page could not edit them, and nothing
told the author of a new config field that it needed one.

Phase 4 also serves settings through `er-service`. Workspace actors are keyed
by repo root (ADR 0067), but the config is global: one `config.toml` and one
`agents.toml` per user. Something on the service side has to own it.

## Decision

**The hand-declared table stays the schema.** No `schemars::JsonSchema`
derives on the config types. The page needs current values, patch keys, widget
kinds, section order, dynamic option lists and list rows in one payload. A JSON
Schema carries none of the values or patch keys and has no standard spelling
for the rest, so a derive would add a private extension dialect and a
schema-to-row translator, and the table would still supply the values: two
descriptions of every leaf plus a translator to keep in step. The labels are UI
copy; the config doc comments are written for code readers and would leak
internal notes onto the page. The TUI has its own table, so a derive would
leave a third description beside the two.

**A fixture makes the classification compulsory.** What a derive would buy is
a reminder that a new field needs a decision. The coverage test does that: its
fixtures build `ErConfig` and `AgentsConfig` with every field named and no
`..Default::default()`, so a new field does not compile there until it is set.
Once set, its leaf must belong to exactly one of three partitions: a table row,
a bespoke editor's root (providers and models, risk rules, project packages),
or a named non-form leaf. A second test saves the fixture and checks the
`agents.toml`/`config.toml` split leaf by leaf.

**App-scoped state has one service-level owner.** The config belongs to one
settings actor, held at a fixed key in the desktop's `ServiceState` and never
chosen from the active tab. Workspace actors own per-repo state only. P45's
`AppRegistry` follows the same rule for tabs and caches. Per-repo actors each
holding a config copy would race their saves to one file, and a desktop command
that picks the actor from the active tab had to read the tab under the App
lock, which is the wait the service exists to remove.

**Settings follow ADR 0067's read path.** The settings actor publishes a
versioned view (the rows, the provider lists, and one risk-rules view per
rules key); writes are commands. A risk-rules view's own version rises only
when that repo's rules change. Resolving changed paths against it runs on the
caller's side until the service holds the diff. The view shows what is on disk:
a save the file refuses undoes the edit, and any write staged after it, in the
actor's config.

**`apply_config_field` stays the one writer of a config field**, in the
engine's config module, and the settings actor calls it.

## Consequences

**Good.** A config field cannot be added without deciding how Settings shows
it. The settings commands save through one owner, so their saves are ordered.
Settings reads never wait on the actor.

**Costs.** Labels and descriptions are still written by hand, and the TUI's
table is not covered by the fixture. A client that wants a machine-readable
schema for validation would need a derive added beside the table; the wire
rows would not change. Side effects the App ran after a config write
(refreshing watched files, resyncing the AI selection) left with it. The
palette's `set_ai_model`, `set_ai_selection` and `set_ai_effort` save through
the actor too. A write is published before its save lands, so a reader can
see an edit the file then refuses; the actor rolls it back when the refusal
arrives, and the view matches the disk again.
