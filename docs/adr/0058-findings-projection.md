# Findings are a load-time projection, not a mutation

## Context

`load_ai_state` built the findings a user sees by mutating the parsed
`review.json` in four places: expert rows, professor rows, accepted-run
provenance and the arbiter overlay. Their order carried meaning — provenance
clears every row's `providers`, so it must run before the arbiter, which writes
providers back from a verdict — and each stage lived in the module that owned
its sidecar. P21 needs one seam to add validation and scoped re-review
overlays; four in-place merges do not give it one.

## Decision

`ai::projection::build_findings_projection` is the one place the visible
findings are composed. It takes the parsed general review, the loaded expert
files, the professor file, the captured arbiter overlay and a provenance
lookup, and returns `FindingsProjection`: the review, the arbiter effect, the
graded keys, the matched verdicts and provenance diagnostics. `AiState.review`
keeps its `ErReview` type, so no reader or front end changes.

The stages run in the order the loader used: backfill lenses; append the
accepted-findings artifact's rows (ADR 0065); append accepted experts in
ascending `expert_id`; merge the professor; clear agent-written providers and
restore accepted rows' provenance; overlay the arbiter last. The provenance
step is a lookup closure, not a disk read inside the module, so the projection
imports no `std::fs` and tests bind it to a fixed map.

Scoped re-review overlays replace their paths in `ProjectionInputs` before the
first stage, and validation sits between provenance and the arbiter. A row
added after provenance would keep agent-written providers and miss the arena
restore; the arbiter stays last so it grades the final rows.

## Consequences

- Expert order is ascending `expert_id` instead of `read_dir` order, which had
  no defined value; the arena already sorted experts this way.
- One function to extend for scoped overlays and validation, and the golden
  test pins the per-file order and ids against the pre-refactor behaviour.
- The projection is composed on every load. It was composed then too; the
  difference is that the composition is now in one place and testable.
