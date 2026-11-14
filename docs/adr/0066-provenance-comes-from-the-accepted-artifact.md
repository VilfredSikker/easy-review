# Provenance comes from the accepted-findings artifact

## Context

P34a moved accepted findings into their own artifact, but the projection still
restored their provider pills through a read of `arena/<run>/run.json`
(`accepted_provenance_for_rows`). That read made a load touch arena files, kept
a process-wide cache with an invalidation call from acceptance, and only
applied when the run's diff hash equalled the review's. The arena import is
gone with P34b; the run read was the last tie between loading review state and
the arena's history files.

## Decision

The projection restores providers from `ProjectionInputs.accepted` alone: it
clears every row's provider list, then copies `providers` from the artifact row
with the same review id. `ai/accepted_provenance.rs` and its run cache are
deleted, `FindingsProjection.provenance_diagnostics` goes with the read that
produced it, and the projection still does no disk I/O.

An old bucket, from before P34a, can hold imported rows inside `review.json`
and name them in run files without an artifact. Those rows stay visible with
what `review.json` holds: text, severity, anchors, `raised_by` and `lens`. They
do not get provider pills or the run's lens/raiser normalization. This affects
pre-release buckets only: 1.0.0 carries no old data except the one-time
project-list import (D15). The run directory keeps the record until it is
deleted, so a migration is still possible later if one is ever wanted.

## Consequences

**Good.** A load no longer reads arena files, acceptance no longer invalidates
a cache, and one artifact row is the single source for an accepted finding's
provenance.

**Costs.** Pre-release buckets show accepted rows without provider pills.
Re-accepting is not offered for them because the run's own record still says
accepted; the rows themselves stay visible.
