# A durable workflow record owns flow state

## Context

The review flow kept its state in memory: the planned tasks, their progress, the
captured diff and the bucket lease all died with the process. Only the arena
persisted its run, and admission read that persisted `run.json` status as a live
producer, so a crash mid-run left a `Running` status that blocked the bucket
until a later reconcile rewrote it as `Failed`. Nothing knew which stages had
finished, so nothing could offer to finish them.

ADR 0044 recorded that orchestration stays in memory because durable flow state
was not needed to resume safely. That held while a crash was simply a failed
review. Once a Deep run spans several jobs and cost confirmation, a crash that
loses completed stages wastes the work and leaves the user unsure what ran. The
coordinator's phase-3 decision reverses it: mark the run interrupted, offer a
re-run of the unfinished stages, never resume automatically.

## Decision

Each bucket gets one `workflow.json`, the Enveloped store kind `workflow`. It is
host-written through `store.update_durable`, one record per bucket, because
admission already allows one workflow per bucket. Its stages mirror the flow's
planned task rows by the same stable ids (`triage`, `finding:<lens>`,
`reviewer:<id>:round:<n>`, `verifying:cross`, `verifying:arbiter`), plus
`panel` and `arbiter` stages for a run started from the arena launcher (ADR
0061). Every stage stores a status (`pending`, `running`, `complete`, `failed`,
`interrupted`), the artifact it wrote and the diff generation it ran against.

The running workflow holds the bucket's `er-store` lease for its life; the
record names the holder (pid and a per-process label). Admission asks the lease
file, not a persisted arena status, so two processes cannot both own a bucket
and a dead one is detectable. On load, a zero-timeout lease probe decides a
`running` stage: a free lease means the owner died, so the stage becomes
`interrupted`; a held lease means another process owns it and it stays
`running`. Loading never spawns.

A re-run is offered for a record with `interrupted` or `failed` stages: stages
already `complete` at the same diff generation stay, the rest run again, Deep
round-1 reviewer jobs re-run per job, cross-voting is atomic over all of its
inputs, and a changed diff generation starts the workflow over. A user cancel
leaves unfinished stages `interrupted` with outcome `cancelled` and offers no
re-run on load.

## Consequences

**Good.** A crash leaves a readable record of what finished, and the next load
can finish the rest. Admission is cross-process and self-healing: a stale
`running` stage can no longer block a bucket forever. The record and the UI's
progress rows share task ids, so the durable view and the live view cannot
drift apart.

**Costs.** Until the arena is rebuilt on the job runner, a Deep run has two
records (the workflow record and the arena's `run.json`), and the workflow
record is the authority on resume while `run.json` still holds the findings.
Stage transitions now write a host file under the store lock. ADR 0044's
in-memory policy is reversed for orchestration state; its behaviour decisions —
depths, confirmation, guarded acceptance — stand.
