# Review checklist: enriched epic 1.0.0 issues

For the reviewer of each issue rewritten under [HANDOFF-issue-enrichment.md](./HANDOFF-issue-enrichment.md). The question for every issue: **could a smaller model that has read only this issue and [AGENT_BRIEF.md](./AGENT_BRIEF.md) do the work without making a decision the issue did not make?**

Check the code at `origin/release/v1.0.0`, not `main`.

## Per issue

1. **Pointers are true.** Spot-check at least three `path:line` references, and every call-site count. A wrong pointer fails the issue.
2. **Nothing is missed.** Run your own `git grep` for the symbols being changed or deleted. Any site the issue does not list is a finding.
3. **Steps are mechanical.** Each step names the edit. A step that says "decide", "choose" or "design", or that hides a choice behind "update accordingly", either becomes concrete or moves to a design issue.
4. **Tests prove the change.** Each test names the behaviour it pins and would fail against today's code (or, for a deletion, the surviving behaviour is covered). "Add tests" without names fails.
5. **Done when is observable.** At least one criterion is behaviour a user or a caller sees, not only a command passing.
6. **Stop-and-ask is specific.** The conditions are ones the implementer can actually detect.
7. **Scope matches the plan.** Compare with the `Plan entry` block and with [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md). Anything added or dropped needs a `Plan correction` section explaining it.
8. **Shorthand is spelled out.** No unexplained D-numbers, lane names or plan terms.
9. **Size is honest.** Estimated above about 1,500 changed lines means the issue says how to split it.
10. **The model label fits.** `model:sonnet` only if every judgement call has been settled or moved behind a design issue.

## Design issues

- The question is one decision, not a bundle.
- The options are real and the costs are concrete. The recommendation follows from the constraints.
- The implementation issue is blocked by it (native dependency), and its dependent steps say `(after #<design issue>)`.
- Persistence formats (P14), gh writes (P05, P10, P11, P16) and process control (P23) get the closest read: these are hard to change after 1.0.0 ships.

## Plan corrections

Settle each one: accept it and update [PLAN.md](./PLAN.md) through an addendum, or reject it with a reason in an issue comment. Do not leave one open.

## Outcome

Comment on each issue with one verdict:

- **`ready`:** it meets the bar. Add the `agent-ready` label if it is not there.
- **`fix`:** list what to change. Remove `agent-ready` until it is fixed.
- **`design-first`:** a decision is still hidden in the steps. Name it.

Close nothing. Assignment and work start only after a `ready` verdict.
