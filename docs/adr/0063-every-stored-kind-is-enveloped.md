# Every stored kind is enveloped

## Context

The store carried two modes into the 1.0.0 work. Agent-written kinds were
stored as bare payload JSON, because agent prompts describe the payload schema
(ADR 0031) and the host wrote what the agent wrote; host-only kinds were
enveloped. Keeping both meant every read path carried a legacy branch, every
kind declared a `WAS_BARE_BEFORE` marker, and a reader could not tell a pre-1.0
file from a direct agent write.

The producers changed. The runner stages agent output and commits it through a
typed store write, er-mcp upload parses the bare bundle in memory and commits
enveloped artifacts, and the recovery API (ADR 0062) restores bytes only
through snapshots. No producer needs a bare file on disk any more.

## Decision

One storage format: a stored artifact is always an envelope. The `Mode`
enum, `Artifact::MODE`, `WAS_BARE_BEFORE` and the legacy parse are gone;
`envelope::parse` requires `kind` and `payload` and rejects a file with
neither, with a malformed reason naming the missing envelope.

A bare file is rejected rather than migrated on read. It has no kind field, so
a migration would have to trust the path to choose one. The registry matches
paths because that is all an unenveloped file offers, but a store read no
longer needs to bet on it. A rejected file stays on disk untouched, so the
user can inspect it; regenerating it or re-uploading through er-mcp replaces
it. A typed write over an unreadable file proceeds and replaces it (only
`NewerVersion` refuses), which is what lets the update flows recover a file
someone edited by hand.

## Consequences

**Good.** Every read has one shape and one version rule. Read paths drop their
per-kind legacy arm, and one table test asserts each of the fifteen production
kinds is written with its own kind and payload.

**Costs.** A pre-1.0 sidecar on disk does not survive the upgrade: it reads as
malformed until it is regenerated or re-uploaded. The rejection message says
so.
