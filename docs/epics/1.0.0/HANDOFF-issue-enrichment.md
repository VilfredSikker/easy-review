# Handoff: make the epic 1.0.0 issues ready for a smaller model

You are rewriting GitHub issues, not code. Do not edit any file in the repo, commit, push, open a PR, or run tests, builds, clippy or lint. You only read code and edit issues on `VilfredSikker/easy-review` with `gh`.

## Why

Issues #372–#395 (P01–P24) are the plan entries copied as they are. They are too terse for an agent that has not read the plan. Five things are missing from each: line pointers, concrete tests, behaviour that counts as done, conditions to stop and ask, and the context hidden behind shorthand like "D5" or "lane". After your pass, an agent that reads only the issue and [AGENT_BRIEF.md](./AGENT_BRIEF.md) must be able to do the work without guessing.

A reviewer checks each issue you finish against [REVIEW-issue-enrichment.md](./REVIEW-issue-enrichment.md). Read that file too: it says what a passing issue looks like.

## Read first

1. [PLAN.md](./PLAN.md): "Verified starting point", "Lint gate facts", "Lanes", and the entry for each issue you work on.
2. [ADDENDUM-01-decisions.md](./ADDENDUM-01-decisions.md): decisions D1–D10. These win over the plan.
3. [AGENT_BRIEF.md](./AGENT_BRIEF.md): what the implementer already gets. Do not repeat it in issues.
4. `CONTEXT.md` at the repo root, for names.

Read the code at `origin/release/v1.0.0`. Use `git -C <checkout> show origin/release/v1.0.0:<path>` or a worktree of that branch, never `main`. The two differ.

## Order

Work in issue order, #372 to #395. Issues later in a chain depend on choices made earlier (P08 and P09 use what P07 decides), so a later issue may cite an earlier one's design section.

## What each issue becomes

Keep the first two quote lines of the current body (`> Part of #371 …` and `> Read AGENT_BRIEF …`) exactly as they are. Replace everything below them with these sections, in this order:

```markdown
## Goal
One or two sentences: what is true after this PR that is not true now, and why it matters for 1.0.0.

## Context
Spell out every piece of plan shorthand the task depends on (D5, lane, allow-list, ingest mode, ADR numbers).
Each ADR the implementer must read, with one line saying why.

## Current code
Each file and symbol the change touches, as `path:line` at origin/release/v1.0.0, with one line on what it does today.
Include call sites found with `git grep`, with a count when there are many.

## Steps
A numbered list. Each step is one edit an agent can check off, in an order that compiles at each point where possible.

## Tests
Each test to add or change: file, test name, the behaviour it pins, and why it fails before the change and passes after it.
For deletions: which existing tests are removed with the code, which are changed, and which remaining test proves the surviving behaviour.

## Done when
Observable behaviour, not just commands. Then the acceptance commands from the plan entry.

## Out of scope
From the plan entry, plus anything you found that a naive implementer would be tempted to touch.

## Stop and ask if
Concrete conditions under which the implementer must stop and comment on the issue instead of guessing.
For example: "a `Legacy` arm also handles a live variant", "more than N call sites", "the diff passes 1,500 lines".

## Size
Lines changed, estimated from what you found.
```

Keep the original plan entry text for reference at the bottom, inside a `<details><summary>Plan entry</summary>` block.

## Design-first issues

These leave a design decision open, and a smaller model must not make that decision: #376 P05, #378 P07, #381 P10, #382 P11, #385 P14, #387 P16, #391 P20, #392 P21, #394 P23.

For each of them, also:

1. **Create a design issue.** Title it `[Pnn-design] <decision>`. Give it labels `epic:1.0.0`, `phase:<n>`, `model:opus` and `design`, and the `v1.0.0` milestone. Make it a sub-issue of #371. Its body has these sections:
   - **Question:** the decision, one paragraph.
   - **Constraints:** from the plan, the ADRs and the code, with `path:line`.
   - **Options:** two or three, each with its cost.
   - **Recommendation:** yours, with the reason.
   - **Output:** what the design issue produces, a comment on the issue settling the decision, plus any type signatures or file layout the implementation issue needs.
2. **Block the implementation issue on the design issue**, with GitHub's native dependency.
3. **In the implementation issue**, write the Steps that do not depend on the decision in full. Mark the ones that do with `(after #<design issue>)`, and say what the implementer reads from the design issue's settled comment.

The decisions to cover:

- **P05:** how a retried create avoids posting a duplicate comment.
- **P07:** the method list of the `Git` trait, and how the handle reaches `App`.
- **P10:** the `GhError` classification rules and the fixtures they need, and the shape of the TTL cache.
- **P11:** merging two caches whose TTLs differ per call.
- **P14:** the envelope and key layout, which users keep from 1.0.0 on.
- **P16:** the `PrivateAnnotation` and `GithubComment` types, and the `promote` boundary.
- **P20:** the projection's ordering and ids, plus the golden test that pins them.
- **P21:** the prompt change and the real review run that validates it.
- **P23:** lock semantics on `flock` and `LockFileEx` when the lock's holder has crashed.

Create the label `design` once if it does not exist: `gh label create design -R VilfredSikker/easy-review -c 0e8a16 -d "Decision to settle before implementation" -f`.

## Model labels

Once an issue's design is split out, change its `model:` label to match what remains:

- `model:sonnet` when every remaining step is mechanical and each decision is settled or blocked behind a design issue.
- `model:opus` when it still involves judgement: persistence formats, gh writes or process control.

Say in your report which labels you changed.

## When the plan is wrong

If the code contradicts the plan entry (a symbol has moved, a caller exists that the plan says does not, a file is not where it says), do not rewrite the scope to fit. Add a `## Plan correction` section at the top of the issue with the evidence (`path:line`) and your proposed fix, and list it in your report. The reviewer decides.

## Editing

Write the new body to a file and use `gh issue edit <n> -R VilfredSikker/easy-review --body-file <file>`. Do not change titles, except to add a design issue's title, and do not change assignees, milestones or the existing blocked-by links. Add a comment on the issue: `Enriched for handoff; ready for review.`

## Report

Write your report to a file and give its path in your reply. Include:

- the issues done, with the design issues created (number, and which implementation issue each blocks)
- plan corrections found
- label changes
- issues you could not make ready, and why
