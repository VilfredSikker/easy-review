# Ready-to-paste prompts for epic 1.0.0

One prompt per bundle from [BUNDLES.md](./BUNDLES.md). Paste each into a fresh agent session. Start wave 2 bundles only when their first issue has no open blocker.

## Wave 1

### T1 config

```text
You are implementing bundle T1 config of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #373, #374, #411.

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

### T2 deletions

```text
You are implementing bundle T2 deletions of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #372, #375.

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

### D1 GitHub writes

```text
You are settling design bundle D1 GitHub writes of epic 1.0.0 in VilfredSikker/easy-review. Your design issues, in order: #403.

A design issue changes no code. Read docs/epics/1.0.0/ADDENDUM-03-issues-are-the-spec.md and AGENT_BRIEF.md on branch release/v1.0.0. Then, for each issue, read it in full, the task issue it blocks, and the code it cites. Read code only in /Users/vilfredsikker/.claude-worktrees/er-v1/epic-docs, which is checked out at release/v1.0.0; do not edit, commit or switch branches there.

For each design issue in order:
1. Check that its "blocked by" issues are all closed. If any is open, stop and say which.
2. Verify the options and the recommendation against the code. If the code shows the recommendation is wrong, say so and pick another option, with evidence (path:line).
3. Post a comment starting "**Decision:**": the chosen option, the reason, and the options rejected and why. Keep it under 300 words.
4. Post a comment starting "**Signatures:**": the exact Rust types, function signatures, file paths and on-disk formats the task issue needs, so an implementer can copy them. End it with a list headed "Task issue changes": every step or test in the blocked task issue that this decision changes, with the new wording.
5. Do not edit the task issue, do not close the design issue, and do not write code or commit. Tell me the issue numbers and stop. Each decision is reviewed, then approved by the owner, and the task issue is updated after that.

Never run tests, builds, clippy or lint. Read code with git grep, rg and file reads only.
```

### D2 git port

```text
You are settling design bundle D2 git port of epic 1.0.0 in VilfredSikker/easy-review. Your design issues, in order: #407.

A design issue changes no code. Read docs/epics/1.0.0/ADDENDUM-03-issues-are-the-spec.md and AGENT_BRIEF.md on branch release/v1.0.0. Then, for each issue, read it in full, the task issue it blocks, and the code it cites. Read code only in /Users/vilfredsikker/.claude-worktrees/er-v1/epic-docs, which is checked out at release/v1.0.0; do not edit, commit or switch branches there.

For each design issue in order:
1. Check that its "blocked by" issues are all closed. If any is open, stop and say which.
2. Verify the options and the recommendation against the code. If the code shows the recommendation is wrong, say so and pick another option, with evidence (path:line).
3. Post a comment starting "**Decision:**": the chosen option, the reason, and the options rejected and why. Keep it under 300 words.
4. Post a comment starting "**Signatures:**": the exact Rust types, function signatures, file paths and on-disk formats the task issue needs, so an implementer can copy them. End it with a list headed "Task issue changes": every step or test in the blocked task issue that this decision changes, with the new wording.
5. Do not edit the task issue, do not close the design issue, and do not write code or commit. Tell me the issue numbers and stop. Each decision is reviewed, then approved by the owner, and the task issue is updated after that.

Never run tests, builds, clippy or lint. Read code with git grep, rg and file reads only.
```

### D3 GitHub port

```text
You are settling design bundle D3 GitHub port of epic 1.0.0 in VilfredSikker/easy-review. Your design issues, in order: #408, #409, #410.

A design issue changes no code. Read docs/epics/1.0.0/ADDENDUM-03-issues-are-the-spec.md and AGENT_BRIEF.md on branch release/v1.0.0. Then, for each issue, read it in full, the task issue it blocks, and the code it cites. Read code only in /Users/vilfredsikker/.claude-worktrees/er-v1/epic-docs, which is checked out at release/v1.0.0; do not edit, commit or switch branches there.

For each design issue in order:
1. Check that its "blocked by" issues are all closed. If any is open, stop and say which.
2. Verify the options and the recommendation against the code. If the code shows the recommendation is wrong, say so and pick another option, with evidence (path:line).
3. Post a comment starting "**Decision:**": the chosen option, the reason, and the options rejected and why. Keep it under 300 words.
4. Post a comment starting "**Signatures:**": the exact Rust types, function signatures, file paths and on-disk formats the task issue needs, so an implementer can copy them. End it with a list headed "Task issue changes": every step or test in the blocked task issue that this decision changes, with the new wording.
5. Do not edit the task issue, do not close the design issue, and do not write code or commit. Tell me the issue numbers and stop. Each decision is reviewed, then approved by the owner, and the task issue is updated after that.

Never run tests, builds, clippy or lint. Read code with git grep, rg and file reads only.
```

### D4 store format

```text
You are settling design bundle D4 store format of epic 1.0.0 in VilfredSikker/easy-review. Your design issues, in order: #401.

A design issue changes no code. Read docs/epics/1.0.0/ADDENDUM-03-issues-are-the-spec.md and AGENT_BRIEF.md on branch release/v1.0.0. Then, for each issue, read it in full, the task issue it blocks, and the code it cites. Read code only in /Users/vilfredsikker/.claude-worktrees/er-v1/epic-docs, which is checked out at release/v1.0.0; do not edit, commit or switch branches there.

For each design issue in order:
1. Check that its "blocked by" issues are all closed. If any is open, stop and say which.
2. Verify the options and the recommendation against the code. If the code shows the recommendation is wrong, say so and pick another option, with evidence (path:line).
3. Post a comment starting "**Decision:**": the chosen option, the reason, and the options rejected and why. Keep it under 300 words.
4. Post a comment starting "**Signatures:**": the exact Rust types, function signatures, file paths and on-disk formats the task issue needs, so an implementer can copy them. End it with a list headed "Task issue changes": every step or test in the blocked task issue that this decision changes, with the new wording.
5. Do not edit the task issue, do not close the design issue, and do not write code or commit. Tell me the issue numbers and stop. Each decision is reviewed, then approved by the owner, and the task issue is updated after that.

Never run tests, builds, clippy or lint. Read code with git grep, rg and file reads only.
```

### D5 projection

```text
You are settling design bundle D5 projection of epic 1.0.0 in VilfredSikker/easy-review. Your design issues, in order: #402.

A design issue changes no code. Read docs/epics/1.0.0/ADDENDUM-03-issues-are-the-spec.md and AGENT_BRIEF.md on branch release/v1.0.0. Then, for each issue, read it in full, the task issue it blocks, and the code it cites. Read code only in /Users/vilfredsikker/.claude-worktrees/er-v1/epic-docs, which is checked out at release/v1.0.0; do not edit, commit or switch branches there.

For each design issue in order:
1. Check that its "blocked by" issues are all closed. If any is open, stop and say which.
2. Verify the options and the recommendation against the code. If the code shows the recommendation is wrong, say so and pick another option, with evidence (path:line).
3. Post a comment starting "**Decision:**": the chosen option, the reason, and the options rejected and why. Keep it under 300 words.
4. Post a comment starting "**Signatures:**": the exact Rust types, function signatures, file paths and on-disk formats the task issue needs, so an implementer can copy them. End it with a list headed "Task issue changes": every step or test in the blocked task issue that this decision changes, with the new wording.
5. Do not edit the task issue, do not close the design issue, and do not write code or commit. Tell me the issue numbers and stop. Each decision is reviewed, then approved by the owner, and the task issue is updated after that.

Never run tests, builds, clippy or lint. Read code with git grep, rg and file reads only.
```

## Wave 2

### T3 comment posting

```text
You are implementing bundle T3 comment posting of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #376, #377.

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

### D6 store follow-ups

```text
You are settling design bundle D6 store follow-ups of epic 1.0.0 in VilfredSikker/easy-review. Your design issues, in order: #404, #405, #406.

A design issue changes no code. Read docs/epics/1.0.0/ADDENDUM-03-issues-are-the-spec.md and AGENT_BRIEF.md on branch release/v1.0.0. Then, for each issue, read it in full, the task issue it blocks, and the code it cites. Read code only in /Users/vilfredsikker/.claude-worktrees/er-v1/epic-docs, which is checked out at release/v1.0.0; do not edit, commit or switch branches there.

For each design issue in order:
1. Check that its "blocked by" issues are all closed. If any is open, stop and say which.
2. Verify the options and the recommendation against the code. If the code shows the recommendation is wrong, say so and pick another option, with evidence (path:line).
3. Post a comment starting "**Decision:**": the chosen option, the reason, and the options rejected and why. Keep it under 300 words.
4. Post a comment starting "**Signatures:**": the exact Rust types, function signatures, file paths and on-disk formats the task issue needs, so an implementer can copy them. End it with a list headed "Task issue changes": every step or test in the blocked task issue that this decision changes, with the new wording.
5. Do not edit the task issue, do not close the design issue, and do not write code or commit. Tell me the issue numbers and stop. Each decision is reviewed, then approved by the owner, and the task issue is updated after that.

Never run tests, builds, clippy or lint. Read code with git grep, rg and file reads only.
```

### T4 git port

```text
You are implementing bundle T4 git port of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #378, #379, #380.

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

### T5 GitHub port

```text
You are implementing bundle T5 GitHub port of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #381, #382, #383, #384.

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

### T6 store core

```text
You are implementing bundle T6 store core of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #385, #386, #394.

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

### T7 store migrations

```text
You are implementing bundle T7 store migrations of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #387, #388, #389, #390.

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

### T8 findings

```text
You are implementing bundle T8 findings of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #391, #392.

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

### T9 app records

```text
You are implementing bundle T9 app records of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #393.

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

### T10 MCP uploads

```text
You are implementing bundle T10 MCP uploads of epic 1.0.0 in VilfredSikker/easy-review. Your issues, in order: #395.

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
