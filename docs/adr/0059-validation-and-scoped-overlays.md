# Validation and scoped re-reviews are their own artifacts

## Context

A scoped re-review re-checks a subset of files and must not speak for the rest
of the diff, yet the review, professor and expert files each hold one complete
result. The previous answer was to snapshot the full file, let the agent
overwrite it with rows for the selected paths, and merge on exit. That gave
every scoped run write access to the full review, made a crash mid-run lose the
snapshot, and left the validate pass with no durable record of what it decided:
it rewrote `review.json` field by field, so a second pass could not tell its own
earlier outcomes from the agent's original text.

## Decision

A scoped run writes only `scoped/general.json`, `scoped/professor.json` or
`scoped/experts/<id>.json`. These register as the Bare store kinds
`scoped-general`, `scoped-professor` and `scoped-expert`, and the validate pass
as `validation`, because agents write them and the prompts name their files
(ADR 0031). A scoped run never opens the full lens file. On a successful
run the host stamps the overlay with the selected paths and the base file's
`diff_hash` and `created_at` as captured at spawn; the loader replaces the
overlay's paths in the lens before projection. An overlay applies only while its
captured identity still matches the base, so a full run that lands first makes
the old overlay inert without needing to delete it, and a second scoped run
merges the first run's paths from the bytes captured at spawn.

The validate pass writes `validation.json` instead of rewriting `review.json`,
and is applied in the projection between provenance and the arbiter. Its
outcomes are keyed by the finding's identity from before any validation pass,
and each outcome names its file. When a scoped general run re-produces a path,
the host drops that path's outcomes: the rows they described no longer exist.
A validation matching the current diff and the review's generation moves the
review's hash to the current one before the expert and professor filters run,
so a validate refreshes a stale review and old-generation experts drop exactly
as they did when the pass rewrote `review.json`. A full run of a lens clears
that lens's overlay and, for general, the validation and the file manifest —
but only when the run succeeds, in the bucket it was spawned into.

## Consequences

**Good.** One writer per artifact. A scoped run cannot corrupt the full review
and a crash leaves the previous state readable. Validation has a history it can
accumulate across passes, and the host can tell which rows it still describes.

**Costs.** Load now applies three overlays (scoped, validation, arbiter) in a
fixed order, and a shifted finding is re-keyed before the arbiter grades it, so
its verdict is orphaned (ADR 0035 accepts the same trade for moved rows). The
scoped run's own `diff_hash` is the hash of the filtered diff; the merged review
keeps the base's hash and identity, so a scoped re-review never claims the whole
diff is fresh.
