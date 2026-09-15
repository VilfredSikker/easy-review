# Arbiter verdicts are a load-time overlay with content-addressed ids

## Context

An Arbiter judges findings produced by other reviewers — merging duplicates,
regrading confidence, dropping claims it cannot substantiate. Its output has to be
stored somewhere and matched back to the findings it judged.

The obvious place is `review.json`, because that is what the Arena already does:
`import_arena_findings_to_review` (`crates/er-engine/src/arena/import.rs`)
upserts accepted findings into it.

Two facts made that the wrong choice here.

**`review.json` already has three writers.** The general review writes it wholesale.
Finding validation reads, modifies and rewrites it
(`crates/er-engine/src/ai/prompts.rs:1248-1260`, `:1292-1302`). Scoped runs overwrite
it with findings for a subset of paths — which is why `ai/scoped_merge.rs` exists,
snapshotting to `.prev.json` and merging on exit. The arena import also overwrites
`review.diff_hash` with its run's hash, so importing into a fresher review silently
re-stamps it as matching a different diff. Adding a fourth writer compounds a problem
that already needed a workaround.

**Expert findings use positional ids.** The expert prompt assigns `sec-1`, `sec-2`
and so on (`ai/prompts.rs:381`). Re-run the security expert and `sec-1` is a
different finding wearing the same name. Any store of verdicts keyed on those ids
would attach a stale judgement to a new claim — silently, and in the direction of
false confidence.

## Decision

Arbiter verdicts are written to their own `arbiter.json` sidecar in the active view
bucket, and merged into the review at **load** time — the same overlay pattern
`merge_experts_into_review` (`ai/experts.rs`) already uses for expert sidecars.
Nothing but the arbiter writes that file, and the arbiter writes nothing else.

Findings are content-addressed for the purpose of matching. `arena/identity.rs`
already provides `finding_id` as `sha1(file + nearest_function + canonical_text)`,
stable across runs and tested for it; `Finding` carries no `nearest_function`, so the
variant used here is `sha1(file + canonical(title))` over the existing
`canonical_finding_text` normaliser.

A finding whose claim is unchanged keeps its id and therefore its verdict. A finding
whose claim changed gets a new id, and its old verdict is orphaned rather than
misapplied.

## Consequences

**Good.** `review.json` keeps its existing three writers and gains no more. Re-running
the arbiter is idempotent. Each expert's original claim survives intact beside the
arbiter's ruling, which is what makes the disagreement case legible. Verdicts inherit
per-finding staleness for free, because a verdict is attached to a finding and the
finding knows whether its lines moved.

**Costs.** Two sidecars must be read and merged where one file would have sufficed,
and every consumer of findings must go through the overlay rather than reading
`review.json` directly — a consumer that forgets will silently show ungraded
findings. Content-addressing also means an expert rewording an identical issue
produces a new id and loses its verdict; `canonical_finding_text` normalises
whitespace and case, but not paraphrase.

**Rejected: rewriting `experts/<id>.json` in place with verdicts.** It destroys the
original expert claim, which is exactly what you need to keep for the case where the
arbiter and the expert disagree.
