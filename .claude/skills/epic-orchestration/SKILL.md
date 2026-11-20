---
name: epic-orchestration
description: Easy Review's copy of epic orchestration, used in this repository in place of the agentic-toolkit plugin's general copy. Run a large multi-PR epic with one orchestrator and delegated implementer and reviewer agents. The epic's working state lives on the Feature Board (the epic task, its sub-tasks, epicDocs and epicLog). Covers the plan, lanes, the agent brief, the derived status, the build log, the review and merge loop, and how to resume. Use when the owner asks to plan and deliver a body of work as many PRs on a release branch, to resume an epic in progress (look for an epic task on the Feature Board), or when the board's "Run as workflow" starts one.
---

# Epic orchestration

One orchestrator plans, delegates, reviews and merges. Implementer agents each build one PR, and reviewer agents each review one PR. The epic's working state lives on the Feature Board, so any session can pick the epic up cold. ADRs stay in the repo. Everything that differs per repository (the epic branch, the checks, the lanes, the local services) is in "This repository" at the end.

## State: the epic on the Feature Board

The board is https://claude.ai/artifact/JfHRDzatazCcfG8dVrw1jd. Read and write it only with the ArtifactData tool (`get`, `query`, `set`, `update`, `batch`), with that `url`. Pin every write to an existing doc with the `if_version` you last read; batches hold up to 50 writes. Anyone with board access may read the state, so keep secrets and customer data out of it. The data contract is FB-179; the user-wide `feature-board` skill has the board's general conventions.

| What | Where on the board | Purpose |
| --- | --- | --- |
| The epic | `tasks/<epic id>`, `kind: "epic"` | `spec`: the plan's verified scope, target design and risks. `release`: the release branch. `decisions`, `questions`/`answers`: owner decisions; where a decision and the plan disagree, the decision wins. |
| One sub-task per planned PR | `tasks/<epic id>--<plan id in lowercase>` (`er-90--p07`), `parentId` = epic id, `origin: "subtask"` | `planId` (`P07`), `lane`, `model`, `dependsOn`, `stackOn`, `spec` (the plan entry: scope, out of scope, acceptance commands, risks, size), then `status`, `branch`, `prUrl`, `ci` as the work moves. |
| Deferred work | ordinary follow-up tasks | What DEFERRED.md used to hold. |
| Long text | `epicDocs/<epic id>--<name>` | `brief` (the agent brief), `resume` ("How to resume"), `release-notes`, `owner-actions`, `plan-notes` (anything in the plan that is neither the epic spec nor a sub-task), `addendum-<slug>` only for a long owner write-up that does not fit `decisions`. Every change bumps `version` and writes an `epicDocVersions` copy in the same batch. |
| Build log | `epicLog/<epic id>--<YYYYMMDDHHMMSS>-<4 letters>` | Append-only, one doc per entry: `kind` note, decision, lesson, status or owner_action. Never edit or delete an entry. |

**Status is derived, never stored.** There is no status file. Read it from the sub-tasks with `scripts/board.py status`: ⬜ planned (backlog, explored, todo, spec_review, planned) · 🟡 in progress (in_progress, no PR) · 🔵 in review (code_review, in_review, merge_ready) · 🔧 fixing (in_progress with a PR, or CI failing) · ✅ merged (done) · ⛔ blocked (on_hold, `blockedBy` or `pipelineStop`). The board page shows the same thing to the owner.

**Only the orchestrator writes the board.** Implementer and reviewer agents never call ArtifactData. Before delegating, export what they need to a local scratch folder outside the repo (never committed), and name that folder in the prompt:

1. Read the epic, its sub-tasks, its epicDocs and the latest epicLog entries with ArtifactData and `out_dir` set to `<scratch>/board`.
2. `python3 -I <skill>/scripts/board.py export <scratch>/board <scratch>/epic` writes `BRIEF.md`, `PLAN.md` (spec, decisions and the derived status table), `RESUME.md`, `LOG.md` (newest first), one `PR-<planId>.md` per sub-task and `<name>.md` for any other doc.
3. Re-export when the brief, a decision or a spec changes, and tell running agents with SendMessage.

Agents report back. Any spec text or follow-up they produce goes in their report, and the orchestrator writes it to the board.

Start the brief from `templates/AGENT_BRIEF.md`.

## Lifecycle

1. **Audit and decide.** If the owner has not decided the big forks, ask now. A table of decisions with a recommended default for each works well. Record the answers in the epic's `decisions`, and a `decision` entry in the log.
2. **Epic branch.** Create the epic's branch as "This repository" says, from the branch the owner names, and push it. Every epic PR targets it, never `main`. Set the epic's `release` to it. Below, `<release>` means that branch.
3. **Plan, using the Plan agent on Opus.** Hand it the audit, the decisions and the constraints. It must re-verify every finding against the current branch tip, since code moves on between the audit and the plan.
   Each PR in the plan gets:
   - an id (`P01`; lowercase `p01` in branch, worktree and sub-task ids), a branch (`epic/p01-<slug>`) and a lane
   - a model: Sonnet for simple, mechanical or well-specified work; Opus for complex work, security, money, data access or design calls
   - dependencies, scope, out of scope, acceptance commands, risks and a rough size
   Bound each PR by scope: one responsibility a reviewer can hold in mind and the owner can test on its own. There is no line limit, and a size estimate is a soft guide only. Split a PR when it does two things that could land separately, along that seam, never to reach a number of lines.
4. **Lanes.** Group PRs that can run at the same time, each in its own worktree. A lane that needs a shared local service (a database, a port) is serial. Sequence PRs that edit the same hot files. "This repository" names its serial lanes and hot files.
5. **Put the plan on the board** before any delegation, in batches: the epic's `spec`, one sub-task per PR, the `brief` and `resume` docs (`scripts/board.py doc`), any `plan-notes` and `owner-actions`, and a first `status` log entry. Deferred items become follow-up tasks.
6. **The delegate, review and merge loop**, below.
7. **Finish.** Write a cutover or runbook PR when the epic needs one, write `release-notes`, and open the epic branch's PR into its base, as "This repository" says, with any "do not merge until …" banner the owner actions require.

## The loop

For each PR that is ready to start:

1. **Worktree.** Create `<worktrees>/<id>` outside the repo, where `<repo>` is the main checkout and `<worktrees>` is a directory outside it on this machine:

   ```
   git -C <repo> fetch origin <release>
   git -C <repo> worktree add -b epic/<id>-<slug> <worktrees>/<id> origin/<release>   # <id> in lowercase, e.g. p01
   ```

   Keep worktrees outside the repo, so the repository's own checks never scan another agent's copy. Set the sub-task to `in_progress` with its `branch` and `agentStatus: "Implementing"`. When it is the epic's first sub-task to start, set the epic to `in_progress` in the same batch.
   Take a run id for the sub-task with `scripts/board.py runid <sub-task id>`, and start the brief of every agent you give this sub-task (implementer and reviewers) with the line `run: <run id>`, so its tokens can be counted.

2. **Implementer.** Use the Agent tool with `model` set to the PR's model, in the background.
   - The prompt is short: the PR id, the worktree path, the export folder ("read BRIEF.md and PR-<id>.md there"), whether the agent is in the DB lane, and anything the plan gets wrong.
   - The agent pushes and opens the PR, but does not merge. Write its `prUrl` and set the sub-task to `code_review` with `agentStatus: "PR open for review"`.

3. **Reviewer.** Use the Agent tool once the PR is open: Opus for DB, security or money PRs, Sonnet for the rest. Give it the same export folder. The reviewer:
   - checks the diff against the plan entry and the repo rules;
   - posts one GitHub review with event `COMMENT` (it cannot approve its own repo's PR);
   - reports a verdict of `merge`, `fix-then-merge` or `rework`.
   Tell the reviewer what to look at hardest, for example "can the override ever turn a flag on in production?".

4. **Fix.** Send the must-fix items to the same implementer with SendMessage. It keeps its context. Set the sub-task back to `in_progress` with `agentStatus: "Addressing feedback"` (it shows as 🔧).
   - For a small fix, check the fix diff yourself instead of running a second full review.
   - When a reviewer finds a real problem next to the PR's scope that the PR makes worse, fold it into the PR.

5. **Merge** with a merge commit (no squash), pinned to the head SHA you reviewed so the merge fails if the branch moved since. Then:
   - set the sub-task `done` with `resolution: "completed"` and `agentRun: <run id>`, append a `status` log entry (`scripts/board.py log`), and set its run (`scripts/board.py run`, below), in one batch; when it was the epic's last open sub-task, set the epic `done` with `resolution: "completed"` in the same batch;
   - remove the worktree;
   - make sure the next branch in that lane merges the release branch before it continues.

6. **Report to the owner** in a line or two per event. Ask only when a decision is genuinely theirs, or when something needs their accounts or dashboards.

### Work found mid-epic (FB-181)

When an implementer or reviewer reports extra work, the orchestrator decides where it goes:

- **The epic needs it** (a missing piece, or a planned PR split in two): create a new sub-task of the epic: `origin: "subtask"`, `parentId` the epic, a `planId` and `lane`, the epic's `session` and the `ignore_pipeline` tag. Pick it up in the next round.
- **Anything else** (scope creep, nice-to-haves, work after merge): create a normal follow-up task: `origin: "follow_up"`, `parentId` the sub-task it came from, status `explored` when you can judge its size and feasibility, else `backlog`, no `ignore_pipeline` and no `session`. The pipeline picks it up later.

## Run as workflow

The board's "Run as workflow" starts an epic in a Claude Code session: the epic and its open sub-tasks carry `ignore_pipeline` and `session` with `mode: "workflow"`. Then:

- Keep `session.updatedAt` fresh on the epic and on each sub-task you work, at least once per stage, or the board shows the session as idle in Needs you.
- Write one run per sub-task, once, when you are done with it (merged, handed back, or stopped): `scripts/board.py run <run id> <sub-task id> <project> done|failed "<result>" "<summary: 3 to 5 short sentences>" <stages> <session id>` writes it with the tokens of every agent whose brief started with `run: <run id>`, and prints its path and doc id: `set` it with ArtifactData (`file_path`), or add it to the batch you are writing. Its tokens count toward the board's token budget, so the pipeline holds its own starts while the epic spends the usage limit. Your own tokens as orchestrator go uncounted. Set `agentStatus` in writes you already make; never add a write only for it.
- Once a sub-task's PR is open and reviewed, you may hand it back to the pipeline: take the tags off (`ignore_pipeline` stays when `session.addedTag` is false) and set `session: null`. The pipeline then handles CI fixes, auto-merge and anything after. Hand back the epic itself when every sub-task is handed back.

## Rules for running agents in parallel

- **Machine load.** Heavy checks (the ones "This repository" lists) run one at a time across the machine: every agent runs them through `~/.claude/scripts/locked.sh <worktree> <command…>` when that script exists, in the foreground, and names it in every brief. A timeout that happens only under load, in a file that passes alone, is not a failure. Report the file.
- **Never `pkill` a pattern.** Stop only your own PIDs; other agents run the same tools.
- **Serial lanes are single-owner.** Only the agent named as holding a serial lane may reset or migrate its shared service.
- **CI.** "This repository" says which PRs run CI. Where epic PRs run none, all verification is local, and workflow changes stay untested until they reach a branch that runs them, so reviewers must read the YAML closely.
- **External accounts.** Anything that needs one (staging, Sentry, Turnstile, a production reset) is coded behind env vars, so it does nothing until configured. The owner's steps go in an "Owner actions" section of the PR, in the `owner-actions` doc and as an `owner_action` log entry.
- **Systemic test flakiness.** Make it its own PR, such as PX1, instead of letting every PR report "coverage unconfirmed".
- **Decisions that arrive mid-epic.** Record them in the epic's `decisions` (an `addendum-<slug>` doc only when long), add a `decision` log entry, re-export, and send them to any running planner or implementer with SendMessage.
- **Model names.** Keep model names out of PR text and code. The only exception is the `Co-Authored-By` trailer the session's attribution asks for. Keep agent ids out of user-facing text.

## Resuming an epic

1. **Read the board.** Find the epic task. Read its `resume` doc, then the latest epicLog entries (query `epic` == the id, ordered by `at` descending), then the sub-tasks. `scripts/board.py status` gives the table.
2. **Bring up the local services** "This repository" names, if any.
3. **Check the open PRs.** Run `git fetch origin` and list the open PRs with the release branch as base, and compare them with the sub-tasks. A sub-task at 🟡 or 🔧 with no live agent needs a new implementer. Give it the PR id, its branch and the last known state from the log. A sub-task at 🔵 with no live reviewer needs a new reviewer: read any review already posted on the PR first.
4. **Export** for the agents you start, and **continue the loop.**

Update the `resume` doc whenever the way to pick the epic up changes.

## The board helper

`scripts/board.py` (python3, standard library, run with `python3 -I`) does the file work around ArtifactData calls, since a script cannot call ArtifactData itself. It writes nothing to the board. Its output goes to the system temp folder, or to `BOARD_OUT` when set.

```
board.py export <saved-dir> <out-folder>     # agent files from docs saved with out_dir
board.py status <saved-dir>                  # the derived status table
board.py log <epic id> <project> <kind> "<text>" [P07=merged:931:note ...]
board.py doc <epic id> <project> <name> <markdown file> <current version or 0> "<summary>" [if_version]
board.py runid <sub-task id>                  # a run id for the sub-task's agent briefs
board.py run <run id> <sub-task id> <project> <done|failed> "<result>" "<summary>" [stages] [session id]
```

- `log` writes one epicLog doc and prints its path and doc id: `set` it with ArtifactData (`file_path`), or add it to a batch. Items are `planId=status:pr:note`, where status is planned, in_progress, in_review, fixing, merged or blocked; use `-` for no PR or no note. Items go with kind `status` only.
- `doc` writes a `writes` array that sets the epicDocs doc (its `version` bumped) and its epicDocVersions copy; pass it to ArtifactData `batch`. `<current version>` is the doc's own `version` field; `if_version` is the database version you read, needed when the doc exists.
- `run` writes one runs doc (`kind: "session"`, `trigger: "session"`) with `metrics.tokens` summed over the subagent transcripts whose first message starts with `run: <run id>`; with none it writes `tokens: null` and says why in `metrics.note`.
- Test it with `python3 -I scripts/test_board.py`.

## This repository

The board project id is `easy-review` (the `<project>` argument to `board.py`).

- **Epic branch.** `release/vX.Y.Z`, cut from the branch the owner names (root CLAUDE.md, "Branching & release"). Sub-task PRs target it; the epic ends with its PR into `main` and release notes. `release/v1.0.0` runs its own epic from `docs/epics/1.0.0/`; don't run it with this skill unless the owner says so.
- **CI.** `ci.yml` runs on pull requests into `main`, `staging` and `release/**`, so epic PRs get CI. Wait for it before merging; a red check is the PR's to fix.
- **Checks.** `just ci` before the PR (format, clippy, UI lint, docs check, comments check, tests, headless engine build); `just test` while working. Lint only tightens (ADR 0038): never grow a baseline.
- **Worktrees** stay outside the repo. Each has its own cargo target dirs, which are large: remove a worktree as soon as its PR merges, and watch disk space.
- **Serial lanes.** None by default. Sequence PRs that touch `er-engine`'s public types or the shared theme tokens (`crates/er-tui/src/ui/themes.rs`, `desktop-ui/src/lib/themes.ts`).
- **Repo rules** agents must follow: root `CLAUDE.md`, `CONTEXT.md` for naming, the ADRs touching their area, and `docs/agents/writing-docs.md` before editing a doc.
