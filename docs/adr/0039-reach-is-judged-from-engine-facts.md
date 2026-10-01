# Reach is judged by the agent from facts the engine computes

## Context

A diff's risk depends on more than whether its code is right. A feature built
entirely as new code — new endpoints, new tables, new services, new UI — can only
break itself, and if a feature flag or permission keeps it off, it cannot break
anything until someone turns it on. The same number of lines spread across shared
modules can break the whole product. `CONTEXT.md` calls this **reach**.

Before this, the only risk signals were the agent's own words (`ErFileReview.risk`,
`TriageDiffStats.approx_risk`), formed from a skim of the diff. Nothing told the
agent which files were new and which were edits, and the triage prompt asked for
"blast radius" with no inputs to judge it from.

Three ways to produce reach were weighed:

- **Prompt change only.** Cheapest, but the agent estimates new-versus-edited from a
  skim, and the result stays a sentence nothing can render or sort.
- **A score computed in the engine.** New-file ratio, importance tiers and
  reverse-import counts, with no model call. ADR 0036 already measured
  reverse-import counting and rejected it on accuracy: module-root files (`mod.rs`,
  `index.ts`) score as foundational. And a **guard** cannot be detected
  mechanically across languages and flag libraries.
- **Engine facts, agent judgement.** The engine computes what it knows exactly; the
  agent judges what needs reading the code.

## Decision

Engine facts, agent judgement.

Every review-shaped command prepares its diff through `ensure_review_inputs`, which
writes `change-facts.md` next to `diff-tmp`. The file lists:

- the new code files;
- the existing code files that are edited, renamed or deleted;
- each file's **file kind**, after the repo's `[file_kinds]` overrides;
- each edited file's declared **importance**. When the repo declares no table, the
  file says "undeclared", which is distinct from a table's default tier.

The facts are rewritten on every call, because they depend on the rule tables as
well as the diff.

Triage records a `reach` block in `triage.json`:

- `level`: `isolated`, `contained` or `broad`;
- `reason`;
- `touch_points`: the edits through which new code is wired into existing code;
- an optional `guard`.

The general review folds reach into each file's `risk_reason`. That needs no schema
change.

A guard counts only with `evidence`: the `path:line` where it is checked. Both front
ends drop a guard claim without evidence. The prompt tells the agent the same thing:
a guess must not lower risk.

## Consequences

**Good.**

- Every agent, whether in-app or an external one through the MCP kit, reads the same
  counts. The file reaches agents that the static kit prompts cannot carry facts to.
- Reach works without an importance table and gets sharper with one.
- Wiring edits are named. Ten new files plus one line in a central router lists that
  line as a touch point.

**Costs.**

- Reach is still a model's call. Two triage runs can disagree, and a confident wrong
  guard claim with fabricated evidence is possible. Evidence gives the reviewer one
  line to open and check.
- `triage.json` gained an optional block. Old files load with reach `unknown`. A
  malformed block degrades to `unknown` instead of failing the load, because
  `load_triage_review` drops the whole file on any parse error.

**Relation to ADR 0036's cut flag tier.** ADR 0036 dropped "flag-guarded" as an
*importance* tier, because being behind a flag says nothing about how much code
depends on a file. Here the guard is a property of one diff, judged at review time
with evidence.
