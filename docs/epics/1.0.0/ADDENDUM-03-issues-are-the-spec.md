# Addendum 03: the issue body is the spec

Recorded 2026-11-06, when planning for phases 0–2 closed.

## Order of precedence

For each task in phases 0–2, its GitHub issue body is the specification. Where they disagree, a later source wins over an earlier one:

1. [PLAN.md](./PLAN.md) entry
2. [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md) and [ADDENDUM-02-issue-decisions.md](./ADDENDUM-02-issue-decisions.md)
3. **the issue body**
4. a settled comment on the issue's design issue

The issue bodies were checked against the code at the `release/v1.0.0` tip. The plan corrections they accepted are not copied back into PLAN.md. Each one is a `Plan correction (accepted)` line in its issue. They include:

- the P04 fallback for unresolved storage;
- which comment creators post in P05;
- P06's split, with desktop-ui landing first;
- the lane and scope changes in P08–P13, P16, P21, P22 and P24.

PLAN.md stays the record of phases 3–6 and of the original reasoning.

## Design issues

Ten issues labelled `design` hold a decision that a task issue is blocked on: #401–#410. Each is settled like this:

1. An agent with the `model:opus` label, or the owner, reads the issue and posts two comments. The first starts `**Decision:**` and gives the chosen option with its reason. The second starts `**Signatures:**` and gives the types, function signatures and file layout the task issue needs.
2. The owner approves the decision, or answers it themselves.
3. The design issue is closed. That clears the native "blocked by" link on its task issue.

A design issue changes no code. When a decision contradicts a step already written in its task issue, the person settling it edits that task issue in the same pass.

## What "assignable" means

An issue is assignable when it has a `ready` review verdict, it has no open blocker, and no assigned issue holds any of its lanes.

`agent-ready` marks a complete spec, not permission to start. #376 (P05) is the one exception: it keeps no `agent-ready` until #403 is settled, because most of its steps depend on that decision.
