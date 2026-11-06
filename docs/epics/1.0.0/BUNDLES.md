# Bundles: how to hand out epic 1.0.0

Filled-in prompts for every bundle: [PROMPTS.md](./PROMPTS.md). Coordinator handoff: [COORDINATOR.md](./COORDINATOR.md).

A bundle is a set of issues that one agent session works through in order. The issues in a bundle share lanes or context, and each one needs the one before it merged. One session per bundle keeps that context and never runs two issues on the same lane.

Every output comes back to the orchestrating session for an Opus review before it counts ([ADDENDUM-03](./ADDENDUM-03-issues-are-the-spec.md)). A PR is reviewed before merge, and a design decision before you approve and close it.

## Wave 1: start now

These seven bundles run at the same time. The two task bundles hold different lanes, and design bundles hold no lane.

| Bundle | Prompt | Issues, in order | Lanes |
| --- | --- | --- | --- |
| **T1 config** | task | #373 P02 → #374 P03 → #411 P25 | C |
| **T2 deletions** | task | #372 P01 → #375 P04 | E, F |
| **D1 GitHub writes** | design | #403 P05-design | - |
| **D2 git port** | design | #407 P07-design | - |
| **D3 GitHub port** | design | #408 → #409 → #410 (P10, P10, P11 designs) | - |
| **D4 store format** | design | #401 P14-design | - |
| **D5 projection** | design | #402 P20-design | - |

## Wave 2: when its blockers close

Start a bundle when its first issue has no open blocker. The ones that become ready first are listed first.

| Bundle | Prompt | Issues, in order | Lanes | Waits for |
| --- | --- | --- | --- | --- |
| **T3 comment posting** | task | #376 P05 → #377 P06 | E, F | #375, #403 |
| **D6 store follow-ups** | design | #404 → #405 → #406 (P21, P16, P23 designs) | - | #402, #401, #381 |
| **T4 git port** | task | #378 P07 → #379 P08 → #380 P09 | C, then E/F, then C/F | #374, #407, #411, then #377 |
| **T5 GitHub port** | task | #381 P10 → #382 P11 → #383 P12 → #384 P13 | C/E, then E, then C/F, then F | #378, #408–#410 |
| **T6 store core** | task | #385 P14 → #386 P15 → #394 P23 | C, then C/E, then C | #381, #401, #375, #406 |
| **T7 store migrations** | task | #387 P16 → #388 P17 → #389 P18 → #390 P19 | C/E/F, then C/E | #377, #382, #386, #405 |
| **T8 findings** | task | #391 P20 → #392 P21 | E, then E/F | #389, #402, #404 |
| **T9 app records** | task | #393 P22 | F | #384, #386 |
| **T10 MCP uploads** | task | #395 P24 | C | #390, #394 |

Two lanes collide in wave 2. T5's #383 and #384 hold lane F while T3 and T7 do. T6 and T7 both hold lane C. When two bundles want the same lane, the one whose next issue unblocks more work goes first; GitHub's "blocked by" lists show which. Let the other session wait.

## Task prompt

Replace `<BUNDLE>` and `<ISSUES>`, for example `T1 config` and `#373, #374, #411`.

```text
You are implementing bundle <BUNDLE> of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: <ISSUES>.

Each GitHub issue is your full specification. Read docs/epics/1.0.0/AGENT_BRIEF.md on branch release/v1.0.0 first, then the issue. Where they disagree with PLAN.md, the issue wins. Where an issue's design issue has a settled **Decision:** comment, that comment wins.

For each issue in order:
1. Check that its "blocked by" issues are all closed (gh issue view <n> -R VilfredSikker/easy-review, and the blocked-by list on GitHub). If any is open, stop and say which.
2. Comment on the issue that you are starting.
3. Create a worktree from origin/release/v1.0.0 at ~/.claude-worktrees/er-v1/<plan id in lowercase, e.g. p02>, on the branch the issue names. Work only there.
4. Do the steps. Add the tests the issue names; each one must fail before your change and pass after it. Run every heavy check through ~/.claude/scripts/locked.sh, as the brief says.
5. If a "Stop and ask if" condition happens, stop and comment on the issue with the evidence. Do not work around it.
6. Push, open the PR against release/v1.0.0 with "[Pnn]" at the start of the title and "Part of #<issue>" as the body's first line, and post the brief's report as a comment on the issue.
7. Stop. Do not merge, and do not start the next issue. Tell me the PR number. I will reply "continue" once it is reviewed and merged.

Never merge, never push to release/v1.0.0, and never weaken or skip a test to get green.
```

## Design prompt

Replace `<BUNDLE>` and `<ISSUES>`, for example `D3 GitHub port` and `#408, #409, #410`.

```text
You are settling design bundle <BUNDLE> of epic 1.0.0 in VilfredSikker/easy-review. Your design issues, in order: <ISSUES>.

A design issue changes no code. Read docs/epics/1.0.0/ADDENDUM-03-issues-are-the-spec.md and AGENT_BRIEF.md on branch release/v1.0.0. Then, for each issue, read it in full, the task issue it blocks, and the code it cites. Read code only in /Users/vilfredsikker/.claude-worktrees/er-v1/epic-docs, which is checked out at release/v1.0.0; do not edit, commit or switch branches there.

For each design issue in order:
1. Check that its "blocked by" issues are all closed. If any is open, stop and say which.
2. Verify the options and the recommendation against the code. If the code shows the recommendation is wrong, say so and pick another option, with evidence (path:line).
3. Post a comment starting "**Decision:**": the chosen option, the reason, and the options rejected and why. Keep it under 300 words.
4. Post a comment starting "**Signatures:**": the exact Rust types, function signatures, file paths and on-disk formats the task issue needs, so an implementer can copy them. End it with a list headed "Task issue changes": every step or test in the blocked task issue that this decision changes, with the new wording.
5. Do not edit the task issue, do not close the design issue, and do not write code or commit. Tell me the issue numbers and stop. Each decision is reviewed, then approved by the owner, and the task issue is updated after that.

Never run tests, builds, clippy or lint. Read code with git grep, rg and file reads only.
```

## After a review

- **PR approved:** merge it, close its issue by hand (a PR into a release branch does not close it), then tell that bundle's session "continue".
- **Decision approved:** apply its "Task issue changes" to the task issue, and close the design issue. The task bundle that was waiting on it can start.
- **Changes requested:** send the review findings to the same session, so it keeps its context.
