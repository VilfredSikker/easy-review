# The panel runs every reviewer as a job

## Context

The arena's reviewer rounds were a second AI platform beside the review flow:
the adapter owned the spawn, argv, retry and decoder, the registry owned the
child list and a pid-only kill, and progress went through its own
`progress.jsonl`. A Deep flow drove it as a foreign system, so a cancel could
leave a reviewer's grandchildren running and a supervisor panic left the run
looking live. The phase-3 design makes the review flow and the arena one
workflow engine with composable stages, and it measures round two at no extra
cost: whether the first cross round changes the set of claims two reviewers
back.

## Decision

The Panel is the reviewer stage of a Deep run, and every reviewer in it is an
ordinary `JobSpec` on `er-agent::JobRunner`. The panel runs round 1 and each
cross round as a bounded worker pool, one job per reviewer, so the slot pool,
the process group, the deadline and the final-text decode belong to the
runner. Cancellation is bridged from the run's shared stop flag to the job
handles: killing a handle signals the process group, so a reviewer's
grandchildren die with it. A failed spawn fails that reviewer with its reason;
nothing waits in a queue no one drains.

The panel owns no arena-registry entry and no shared `runs` mutex. It writes
one durable artifact, `panel-run.json` (store kind `panel-run`), holding the
reviewers, their round-1 outcomes and vote counts, and the round-two
measurement: `quorum()` over the round-1 proposals compared with `quorum()`
over the first cross round's ballots, with both stored on the record. The
comparison reads votes the panel already parsed, so it adds no arbiter call,
and the default round count is unchanged until the owner decides from the
data.

In the workflow record, a launcher run's single `arena` stage becomes `panel`
and `arbiter` stages; a one-round run, which never calls the arbiter, gets only
the panel stage. A run's record is closed by a drop guard, so a panicking
supervisor cannot leave its stages running. The launcher's execution and the
seeded arbiter stay on the arena path until the remaining arena calls move onto
the runner; this decision covers the panel's execution and the record's
vocabulary, not the launcher's.

## Consequences

**Good.** A Deep cancel reaches every process its reviewers spawned, and a
reviewer that cannot spawn fails instead of hanging. Reviewer rounds report
through the same job runtime as the finders and the card. The round-two
measurement costs nothing and is ready to inform the round-count decision.

**Costs.** Until the launcher and the seeded arbiter move onto the runner, the
arena adapter and its retry loop remain for those paths, so a Deep panel
reviewer has no retry while a launcher reviewer does. `panel-run.json` is
written at bucket scope, so it describes the bucket's latest panel, as
`workflow.json` does for the latest workflow.
