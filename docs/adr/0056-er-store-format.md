# The store format

The first `er-store` shape is fixed for 1.0.0: the envelope, the two storage
modes and the version rule. Later PRs add kinds; the wrapper and the version
rules stay stable until 2.0.

## Envelope

A host-only kind is stored as `{kind, version, diff_hash, producer, written_at,
payload}`. A borrowed struct writes the fields in that order, so a
re-serialization cannot reorder them. `version` is the payload type's schema
version; the wrapper has no separate format version. Unknown wrapper keys are
ignored, and a missing or mistyped `diff_hash`, `producer` or `written_at`
reads as absent. Wrapper changes are additive until 2.0.

## Bare kinds

An agent-written kind is the payload JSON by itself, with the payload's own
`version` and `diff_hash` at the top level. The host writes the same bytes an
agent would, so an agent that reads and rewrites the file in place keeps
working. The mode exists because agent prompts describe the bare payload
schema (ADR 0031); phase 3 removes it when the runner commits agent output.

## Versions

A file newer than this build reads as `NewerVersion`, and `write` refuses to
replace it, so an older binary cannot erase a newer one's file. An older
version goes through `Artifact::migrate`; with no migration it reads as
`UnknownVersion`. An Enveloped kind accepts a legacy bare file only when the
kind declares `WAS_BARE_BEFORE`.

Adding an optional payload field needs no bump. A new enum variant does: an
older binary has no arm for it and reads the file as malformed, unless the
variant is covered by `#[serde(other)]`.

## Diff hashes

The store never compares `diff_hash` on read or write. A stale file loads and
the caller marks it; agent output is checked where it enters the bucket. The
alternative, rejecting a stale file in `read`, would hide every review whose
diff moved before the next write.
