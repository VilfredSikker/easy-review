# Review flow project

**Status:** proposed
**Started:** 2026-10-17
**Anchors in the task files verified at:** commit `c96dc0f12`

This folder hands a set of changes to er's AI review to an agent with no prior
context. Read this README once, then pick a task from **Tasks**. Each task file
is self-contained and says what to build, where, how to verify it, and when to
stop and ask.

## In plain terms

- **The problem.** er can run many kinds of AI review (general, expert lenses,
  arena, arbiter, triage), but a person has to pick one, start it, and read
  everything it produces. Repeated reviews start from scratch each time, nothing
  records what happened to a finding after a person saw it, and the reviewer's
  attention is spread over the whole diff instead of the parts only a person can
  judge.
- **Why it matters.** er exists for the case where an agent writes code faster
  than a person can read it. Anthropic's Claude Code team published how they
  handle the same problem at scale. Merged PRs more than doubled while the share
  that had to be reverted fell by more than 40 percent. Most of what they do maps
  onto parts er already has.
- **The fix.** Make one Review action the default: a cheap triage pass picks how
  deep to go, then several finder agents run in parallel, a verifier checks each
  finding against the code, and the survivors are ranked and tagged. Around that,
  make repeated reviews settle down instead of looping, record what happened to
  each finding, and point the person at the decisions machines are bad at.
- **TL;DR.** Turn er's separate review buttons into one automatic,
  verify-before-show review flow, and spend the person's attention only where it
  is needed.

## Sources

- *How the Claude Code team does code review*, claude.dev, 2026-10-15.
  https://claude.dev/blog/solving-code-review-at-anthropic/
- Code Review for Claude Code docs: https://code.claude.com/docs/en/code-review
  and its sub-pages `code-review-pull-requests`, `code-review-customize`,
  `code-review-local`.

Each task file's `Idea:` line refers to this numbered list, which is the order
the ideas were first collected in. Task numbers (the order to build in) differ.

1. Pin approvals to the head the reviewer saw → task 01
2. Record an outcome when a finding is dismissed → task 03
3. Review has to converge → task 08
4. Read a REVIEW.md from the repo → task 05
5. Pre-existing findings → task 06
6. Arena round 1 gets the general review's rules → task 02
7. People needed: a label for the whole PR → task 12
8. Show more evidence → task 13
9. A teaching pass for code outside the diff → task 16
10. Resolve GitHub threads from er → task 04
11. Eval set and human-time metrics → task 19
12. Review flow: triage starts Find → Verify → Rank → task 07
13. Spread the flow across providers → task 11
14. Build the review screen around where a person decides → task 15
15. Every finding gets answered, and can go to a fixing agent → task 09
16. Security as its own stage → task 10
17. Review existing code outside any PR → task 18
18. Fix repeated findings at the source → task 17
19. Review while the agent writes → task 14

What the sources say, in brief:

- **Find → Verify → Rank → Post.** "Several agents look for bugs at the same
  time. Then a separate group votes on each finding, and it only gets posted if
  enough of them agree it's real."
- **Severity tags.** Important (fix before merge), Nit (worth fixing, not
  blocking), Pre-existing (a real bug the PR did not introduce), "so a bug the
  PR didn't introduce doesn't look like the author's fault."
- **Depths** (the docs call them review levels). Quick, Standard, Deep. "Deep is worth it where one missed bug costs
  more than the extra review time."
- **Review has to converge.** "An agent author and an agent reviewer can keep
  each other busy indefinitely." Fixes: a cap of three rounds, diff-only
  re-review, flag the bug class, no repeats.
- **Where a person decides.** Should we build it, risky code, architecture,
  reading the evidence, owning the outcome. Checks run everywhere else.
- **Measure noise** by how people respond to each finding: fixed, wrong,
  intentional, didn't matter.

## Decisions already made

These were decided by the project owner on 2026-10-17. Do not reopen them in a
task; if a task seems to need a different answer, stop and ask.

- **One Review action is the default.** It runs triage, then the depth triage
  picks (task 07).
- **Trigger: one click.** Nothing runs on PR refresh. Re-runs after new commits
  are offered to the person, not started automatically.
- **Deep asks first.** Quick and Standard start straight away. Deep shows an
  estimated cost and waits for a click, like the arena's cost confirmation today.
- **The old General, Expert and Arena entries move under an Advanced group** in
  the AI Hub (TUI and desktop). They stay available.
- **Pre-existing findings are wanted** (task 06).
- **The flow should use several providers** where more than one is installed
  (task 11).
- **No bot approvals.** er never approves or merges on its own. Auto-approve and
  auto-merge were cut on purpose in an earlier plan.

## Terms this project proposes

Each is proposed in the task named and lands in `CONTEXT.md` only when that
task's design step is approved. Until then, use them in these docs as written.

- **Review flow** (task 07): the one-click action that runs triage and then
  Find → Verify → Rank.
- **Depth** (task 07): Quick, Standard or Deep. `CONTEXT.md` lists "level" under
  *Avoid*, so do not call it a level.
- **People needed** (task 12): the PR-level label none / one / two / unknown.
  `CONTEXT.md` already uses "Tier" for file importance.
- **Lesson** and **Subject** (task 16): a teaching pass over a path or a
  question, and what it covers.
- **Sweep** (task 18): a review of existing code outside any PR.

## Found while writing these tasks

- `spawn_background_importance`
  (`crates/er-engine/src/app/state/comments.rs`, near line 2869 at
  `c96dc0f12`) starts its agent with Write and Edit allowed, though its doc
  comment says it has no write path; only the prompt wording holds it back.
  This is a bug outside this project and should be fixed on `main` on its own.

## How er's review works today (mental model)

- Review prompts are compiled into `crates/er-engine/src/ai/prompts.rs` (ADR
  0023). er spawns an agent CLI (default `claude --print`; also Codex, Cursor,
  OpenCode, from `crates/er-engine/src/ai_hub_catalog.toml`). The agent writes
  JSON sidecars; er reads them back.
- Sidecars live in managed storage, one set per **view bucket** (ADR 0003, 0004).
  Both the TUI and the desktop read them.
- A **Finding** is AI-owned: a person reads and acts, never edits it (ADR 0009).
  It carries severity, confidence, lens, category, evidence and an anchor.
- **Triage** is a cheap first pass that recommends where to look; it is not a
  review. The **arena** runs several reviewers, a voting round and an
  **arbiter** (ADR 0024). The **arbiter**'s verdicts are an overlay beside the
  findings (ADR 0035). The **seeded run** is expert lenses followed by dedupe and
  an arbiter-only pass.
- Agent spawns go through a queue with a slot cap, default 3 (ADR 0021).
- er-mcp (`crates/er-mcp`) does not spawn agents: the calling agent is the
  reviewer and uploads sidecars.

Words in bold are defined in `CONTEXT.md` at the repo root. Use its vocabulary
and avoid the synonyms it lists.

## Tasks

Ranked by impact for the effort, with dependencies respected. Numbers are the
recommended order.

- **Impact:** H = changes how much a person has to read or how far they can
  trust the result; M = clear improvement in one part of the flow; L = nice to
  have.
- **Complexity:** how hard the design is (new state, ADR needed, open
  questions).
- **Size:** S = one module, at most a small UI touch; M = engine plus one or
  both front ends; L = a new engine subsystem plus both front ends; XL = engine,
  both front ends, prompts and storage.

| # | Task | Impact | Complexity | Size | Depends on | Status |
|---|---|---|---|---|---|---|
| 01 | [Pin approvals to the reviewed head](tasks/01-pin-approvals-to-reviewed-head.md) | M | L | S | — | ready-for-agent |
| 02 | [Arena round 1 gets the general review's rules](tasks/02-arena-round1-rules.md) | M | L | S | — | ready-for-agent |
| 03 | [Record an outcome when a finding is dismissed](tasks/03-finding-outcomes.md) | H | L | M | — | ready-for-agent |
| 04 | [Resolve GitHub threads from er](tasks/04-resolve-github-threads.md) | M | L | S | — | ready-for-agent |
| 05 | [Read REVIEW.md from the base branch](tasks/05-review-md.md) | H | M | S | ADR | needs-design |
| 06 | [Pre-existing findings](tasks/06-pre-existing-tag.md) | M | M | M | — | needs-design |
| 07 | [Review flow: triage → Find → Verify → Rank](tasks/07-default-review-flow.md) | H | H | XL | 02 | needs-design |
| 08 | [Review has to converge](tasks/08-review-convergence.md) | H | H | L | 03, 06 | needs-design |
| 09 | [Every finding answered, plus a fix action](tasks/09-answer-every-finding.md) | H | M | M | 03, 08 | ready-for-agent |
| 10 | [Security as its own stage](tasks/10-security-stage.md) | M | L | S | 07 | ready-for-agent |
| 11 | [Spread the flow across providers](tasks/11-multi-provider-flow.md) | M | M | S | 07 | ready-for-agent |
| 12 | [People needed: a PR-level label](tasks/12-pr-risk-tier.md) | M | M | M | — | ready-for-agent |
| 13 | [More evidence: CI logs, PR template check](tasks/13-evidence-panel.md) | M | M | M | — | ready-for-agent |
| 14 | [Review while the agent writes](tasks/14-live-review.md) | H | H | L | 07, 08 | needs-design |
| 15 | [Screen led by where a person decides](tasks/15-decision-led-screen.md) | H | H | XL | 01, 07, 12, 13 | needs-design |
| 16 | [Teaching pass for a path or a question](tasks/16-teaching-pass-scope.md) | M | M | M | — | needs-design |
| 17 | [Fix repeated findings at the source](tasks/17-fix-at-source.md) | M | M | M | 03, 05 | needs-design |
| 18 | [Review existing code outside a PR](tasks/18-review-existing-code.md) | L | M | M | 06, 16 | needs-design |
| 19 | [Eval set and human-time metrics](tasks/19-evals-and-metrics.md) | H | H | XL | 03 | needs-design |

Tasks 09, 10 and 11 are `ready-for-agent` once the tasks they depend on are
done; check the Depends on column before picking one up. The column lists hard
dependencies only; each task's own `Depends on:` line adds optional ones in
parentheses (for example, 07 uses 05 and 06 when they have landed).

Before starting a `needs-design` task, read
[open-decisions.md](open-decisions.md): it collects every question the owner
has to answer, by task.

Why the order:

- 01, 02 and 04 are small fixes to code that exists. Do them first.
- 03 ranks high for what it unlocks (08, 09, 17, 19) more than for itself.
- 05 is small in code but needs an ADR first, against ADR 0005 (global-only
  config) and ADR 0023 (self-contained prompts).
- 07 is the largest piece, but most of its stages exist (seeded run, arena,
  triage). The work is the engine pipeline, chaining, cost confirmation and
  bringing it to the TUI.
- 10 and 11 are small only once 07 exists.
- 14 and 15 decide where er goes next, but are large and build on most of what
  comes before them.
- 19 is last: it needs months of recorded outcomes before an eval set means
  anything.

Tasks marked `Status: needs-design` start with a design or ADR step that a
person must approve before any code is written. Stop after that step and ask.

## Project-wide rules

These apply to every task. A task file may add to them, never relax them.

- **Read first:** the repo root `CLAUDE.md`, `CONTEXT.md`, and
  `docs/agents/writing-docs.md` before editing any doc.
- **Branching.** Bug fixes go to `main`. Everything else goes to the current
  release branch. Resolve it, never hardcode it:

  ```bash
  git fetch origin --quiet
  git for-each-ref --format='%(refname:short)' 'refs/remotes/origin/release/v*' \
    | sort -V | tail -1
  ```

  Task 01 is the only task that may count as a bug fix (an approval landing on
  commits nobody reviewed). Everything else is a feature.
- **Heavy checks run one at a time.** Run tests, clippy, lint and format through
  `~/.claude/scripts/locked.sh <dir> <command…>`, which holds a machine-wide
  lock. Go fastest first: the changed crate's tests, then affected tests, then
  the full gate.
- **Lint only tightens** (ADR 0038). `just clippy` and `just lint-ui` must exit 0.
  Never grow a baseline; fix the violation or scope an `#[expect(lint, reason =
  …)]` / `// eslint-disable-next-line rule -- why` to the one site.
- **No task numbers in code.** `just comments-check` rejects inline comments that
  cite a plan step or tracker number. Write the why in the comment, never
  "task 07".
- **Tests prove fixes.** A behaviour change gets a test that fails before and
  passes after. Never weaken, skip or delete a test to go green. List every test
  added or changed in your final report.
- **Both front ends.** er has a TUI (`crates/er-tui`) and a desktop app
  (`crates/er-desktop` + `desktop-ui`). Engine state is the UI contract (ADR
  0006). A feature lands in the engine first; a task says which front ends it
  covers.
- **ADRs.** Add one under `docs/adr/` (next free number) when a decision is hard
  to reverse, would surprise a reader, or came from a real trade-off. Tasks 03,
  05, 06, 07, 08, 12, 14, 15, 16, 18 and 19 each add one. Take the next free
  number when you write it, and renumber if another task's ADR landed first.
  Run `just docs-check` after editing docs.
- **Vocabulary.** A new concept gets a definition in `CONTEXT.md`. Do not reuse a
  word `CONTEXT.md` lists under *Avoid*.
- **Commits.** Short messages that say why. Group related files; never
  `git add .`.

## Tracking

Each task file carries a `Status:` line (`ready-for-agent`, `needs-design`,
`in-progress`, `done`, `wontfix`). Update it when you start and finish. Append
notes and decisions at the bottom of the task file under `## Comments`, dated.
