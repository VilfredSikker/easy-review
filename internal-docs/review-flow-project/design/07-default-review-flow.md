# Default review flow

## Product shape

One click starts triage for the current diff. Pull request refresh does not
start a review. Triage chooses how much review to run and which lenses should
look. The existing actions remain available under Advanced. Professor routes
to Quick and leaves a suggestion to run Professor afterward. This keeps the
default path short while preserving the deliberate, more specific actions.
MCP flow integration and new rank labels are outside this scope.

| Triage verdict | Depth | Finders | Verification |
| --- | --- | --- | --- |
| skip | none | none | none |
| general | Quick | general | seeded arbiter |
| professor | Quick | general | seeded arbiter, then suggest Professor |
| expert | Standard | general and registered experts | seeded arbiter |
| arena | Deep | general and registered experts | Deep arena |

Unknown expert IDs are removed. Mentorship is not a finder. Duplicate IDs are
removed while preserving order, with general first. Expert and arena verdicts
with no usable expert fall back to the security lens. Quick and Standard use
the successful current-run finders as seeded arbiter input. The arbiter writes
its verdict overlay and leaves producer findings alone. Deep asks for cost
confirmation before launching its arena. A completed Deep run waits for the
reviewer to accept results; completion does not import them.

Ignore filtering is part of flow input and lands in Step 8. It runs once before
the diff is hashed, so triage, finders, seeded verification, and Deep all read
the same filtered bytes.

## Execution contract

The flow is in-memory state owned by the engine. It is not a persisted job and
does not resume after process restart. Each flow captures its bucket, refs,
scope, input bytes, configuration, and exact task IDs at start. A tab switch
does not redirect its work. All stages use one prepared generation whose hash
names the captured bytes.

The App advances a flow while polling background tasks. It accepts a completion
only for the exact task ID it launched, once, and only when the producer exited
successfully and left a valid fresh sidecar with the expected hash and producer
identity. Quick and Standard verify only successful current-run lenses. A
matching hash alone is not enough to claim that this flow produced a sidecar.

A bucket lease prevents app-managed preparation, deletion, import, or competing
review work from changing the inputs while the flow owns them. The lease is
released only after owned queued or running work stops and the arena registry
entry is released, even when the visible flow is already failed or cancelled.
Terminal flow state and bucket availability are separate. Cancellation targets
owned task IDs and the captured arena. A late completion may release ownership,
but cannot change a terminal result. The lease coordinates this App process; it
does not lock out an editor, an external agent, or a second `er` process.

Quick and Standard use seeded verification because it grades existing producer
claims without replacing them. Deep uses a real arena because its reviewers
run together under the confirmed plan. Deep acceptance remains a separate,
explicit action. These paths preserve the difference between reviewing claims
and importing accepted arena results.

## Delivery order

Step 1 is independent on `main`; Step 2 is independent on the current release
branch. Step 3 records the docs. Steps 4 and 5 follow Step 3 and can proceed in
parallel. Step 6 needs both; Step 7 needs Steps 2 and 6. Step 8 follows Step 6,
then Step 9 follows Step 8. Steps 10 and 11 independently follow Step 7; Step
12 follows Step 6. Step 13 needs Steps 10, 11, and 12. Step 14 follows Step 4;
Step 15 needs Steps 7 and 14; Step 16 needs Steps 13 and 15. Each code step
remains its own reviewable change.
