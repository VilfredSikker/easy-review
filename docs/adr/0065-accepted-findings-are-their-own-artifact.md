# Accepted findings are their own artifact

## Context

ADR 0024 had acceptance merge a run's kept findings into `review.json` and
stamp `accepted_at`. That made the general review a write target for the arena:
acceptance re-read and rewrote the whole file, re-stamped its `diff_hash` with
the run's (so a review from another generation came to look like it matched the
current diff), and needed an archive and a byte-level rollback around the
merge. ADR 0035 records the fourth-writer problem this added.

## Decision

`ai::accepted_findings` stores `AcceptedFindings` at `accepted-findings.json` in
the bucket scope: kind `accepted-findings`, version 1. Each row keeps the
review id the import produced (`arena-{run}-{finding}`), the file, the run id
and diff hash, the acceptance time, the host-derived providers, and the
finding. `Finding.providers` does not serialize, so the row keeps providers in
its own field.

Acceptance appends or replaces rows under one scope lock and takes a snapshot
first. The run's own record (`accepted_at`, `accepted_finding_ids`) is saved
after the artifact, and a failed save rolls the artifact back through the
recovery API (ADR 0062). `review.json` is not read, written or archived by
acceptance, and its diff hash is left alone.

The projection appends the rows after the general review's own rows and before
the expert rows, which is where the import used to leave them, so ids and order
are unchanged for the front ends. Every row's providers are cleared at the
provenance stage and restored from the artifact row through the same lookup
that reads run records.

## Consequences

**Good.** The review loses a writer, and acceptance can no longer mark a review
from another generation as matching the current diff. The artifact is the
durable record: it outlives the run directory.

**Costs.** Rows accepted before this change live inside `review.json`; P34b
deletes the run-file read that restored their providers, so pre-release buckets
show those rows without provider pills (ADR 0066). The artifact spans runs and
generations, so it carries no single `diff_hash`.
