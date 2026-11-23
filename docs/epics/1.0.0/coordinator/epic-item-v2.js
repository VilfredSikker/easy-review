export const meta = {
  name: 'epic-item-v2',
  description: 'Build, finish or review one epic 1.0.0 phase-5 item: independent review, refute must-fixes, fix, return merge-ready PRs',
  whenToUse: 'One run per ADDENDUM-06 item; mode build|finish|review; the coordinator merges between runs',
  phases: [
    { title: 'Build', detail: 'opus builder implements or finishes the item and opens PR(s); skipped in review mode' },
    { title: 'Review', detail: 'independent opus reviewer per PR' },
    { title: 'Refute', detail: 'two opus skeptics per must-fix' },
    { title: 'Fix', detail: 'opus fixer applies surviving must-fixes' },
    { title: 'Delta', detail: 'opus delta review, up to 3 rounds' },
  ],
}

const A = args
const REPO = 'VilfredSikker/easy-review'

const RULES = `
SHARED RULES (epic 1.0.0 of ${REPO}; follow exactly)
- Spec: docs/epics/1.0.0/AGENT_BRIEF.md, the ADDENDUM-06 entry for this item (docs/epics/1.0.0/ADDENDUM-06-phase-5.md; read its decisions A1-A18, lanes and risks too), and issue #${A.issue} INCLUDING ALL COMMENTS: \`gh issue view ${A.issue} --repo ${REPO} --comments\`. The comments hold the approved step plan, the coordinator's approval conditions, amendments and decisions; where they differ from the addendum, the latest coordinator comment wins.
- Item directory: ~/.claude-worktrees/er-v1/<dir>. Never cd into or edit another item's directory. A NEW item directory is a local clone, never a git worktree, so every local git operation works with the sandbox ON: \`git clone --quiet /Users/vilfredsikker/Projects/easy-review ~/.claude-worktrees/er-v1/<dir>\`, then in it \`git remote set-url origin https://github.com/VilfredSikker/easy-review.git\`, \`git fetch origin\`, \`git checkout -b <branch> origin/<base>\`, and symlink desktop-ui/node_modules from /Users/vilfredsikker/Projects/easy-review/desktop-ui/node_modules if the item touches desktop-ui. If the directory already exists and its \`.git\` is a FILE, it is an older git worktree: there, git commands that write (add, commit, checkout, merge, stash) need the sandbox off and go through \`~/.claude/scripts/epic-run.sh <dir> git …\`.
- Scratch files: other agents share the session scratchpad, so write every script, log and draft ONLY under its subfolder named after your item directory (e.g. scratchpad/<dir>/), never at the scratchpad root, and never reuse another item's file. Every test run and plant gets a timeout (e.g. \`timeout 1200\`), so a hung test cannot hold the machine lock.
- Sandbox: keep it ON by default. Edits, reads, builds that need no test run, git in a clone, and \`git fetch\` (the repo is public) all work inside it and need no approval. Turn it off ONLY for: (1) test suites and heavy gates, through epic-run.sh plus locked.sh as below; (2) a push, written exactly \`cd /Users/vilfredsikker/.claude-worktrees/er-v1/<dir> && git push origin epic/<branch>\` (absolute path, no \`~\`) (\`git -C\` breaks the pre-approval); (3) \`gh\` commands (gh cannot read its login inside the sandbox), each as a single command; (4) git writes in an older worktree, through epic-run.sh. Never turn the sandbox off for a whole chained command.
- Every other sandbox-disabled command (anything but the push and a single gh command) starts with \`~/.claude/scripts/epic-run.sh [--home <dir>] <worktree> <command…>\` (pre-approved, so it never waits on a permission prompt). It cds into the worktree, sets CARGO_HOME, RUSTUP_HOME and the toolchain PATH (cargo 1.98.0, just 1.46.0), and gives the command a fresh temp HOME, which it prints to stderr as \`epic-run: HOME=<dir>\`; pass \`--home <dir>\` to reuse one. \`gh\` and \`git\` commands keep the real HOME on their own. It takes one program plus arguments: for pipes or several steps write a script under the scratchpad and run \`epic-run.sh <worktree> bash <script>\`. Never start a sandbox-disabled command with \`source\`, \`export\`, \`cd … &&\` or a variable assignment: those wait on a prompt that nobody answers overnight. Plain single git/gh commands (git fetch/add/commit/checkout/merge, \`git push origin epic/…\`, gh pr create/edit/comment, gh issue comment) are also pre-approved on their own.
- Heavy checks (tests, clippy, fmt, builds) ONLY through the lock, one at a time, foreground with a timeout: \`~/.claude/scripts/epic-run.sh <worktree> ~/.claude/scripts/locked.sh <worktree> just test\`. Run test suites with the sandbox disabled (process-kill and FSEvents tests fail inside it). After a test run, confirm the temp HOME has no \`Library/Application Support/easy-review/v1/app\` (reuse that HOME with \`--home\` to check). Go fastest first: changed files, then affected tests, then the full suite.
- NEVER read or write the owner's real data: ~/Library/Application Support/easy-review (config.toml, agents.toml, projects.json).
- Acceptance as the item touches: just fmt-check; just clippy (must be 0 diagnostics); just test; just test-desktop; just test-all (er-store, new crates, features); just test-v1 (flag path); just build-service-headless; just build-mcp (engine features or deps); just docs-check (docs); just lint-ui and just test-ui (desktop-ui).
- Planted breaks: every test added for a done-when item must fail against a planted break. COMMIT YOUR WORK FIRST, then apply and revert each plant in ONE command (\`git checkout -- <file>\` reverts to HEAD, so uncommitted edits would be lost). Paste the failing output in the PR. Never leave a plant in the tree.
- Stop line: 1,500 added-plus-changed lines after rename detection. A whole-file move behind a one-line \`pub use\` shim counts as a rename when the new file matches the old apart from paths and \`use\` lines (show \`git diff --no-index --stat\` of old-at-merge-base vs new with use/path lines filtered). Over the line: split into stacked PRs, each opened against its parent branch. No waivers.
- Git: short commit messages that say why; group related files; never \`git add .\`; NEVER force-push; a pushed branch takes merges only (merge origin/release/v1.0.0 in, never rebase). Add NO attribution or session lines to commit messages. Commits are normally signed through 1Password; if signing fails because 1Password is locked, the owner has authorized committing WITHOUT signing: retry with \`git -c commit.gpgsign=false commit …\` and note "some commits unsigned (1Password locked)" in the PR body. Do not change any git config file.
- PR: title starts with [${A.id}] (or [${A.id}a]/[${A.id}b] for split parts); body sections The Problem, How we fix, Design choices (if relevant), How to manually test; plus "Part of #${A.issue}", the plant outputs, the gates run with results, and the stop-line measurement. Add NO attribution or session link to PR descriptions or comments.
- Prose style (PR bodies, comments, commit messages): never use the words "load-bearing", "mint(ed)" or "rung", nor the "not X, but Y" sentence shape.
- If a plan premise turns out false, or a decision outside the approved plan is needed, STOP and return status "decision" with the question and your recommended option. Do not decide product questions yourself.
- Do NOT merge any PR. The coordinator merges.
- Do not write report files (the harness refuses them). Put everything in your structured return value: findings first, compact.
`

const BUILD_SCHEMA = {
  type: 'object',
  properties: {
    status: { type: 'string', enum: ['ready', 'blocked', 'decision'] },
    prs: { type: 'array', items: { type: 'object', properties: {
      number: { type: 'integer' }, branch: { type: 'string' }, base: { type: 'string' },
      head: { type: 'string' }, counted_lines: { type: 'integer' }, worktree: { type: 'string' },
    }, required: ['number', 'branch', 'base', 'head'] } },
    gates: { type: 'string' },
    plants: { type: 'string' },
    notes: { type: 'string' },
    blocker: { type: 'string' },
  },
  required: ['status', 'prs', 'notes'],
}
const REVIEW_SCHEMA = {
  type: 'object',
  properties: {
    verdict: { type: 'string', enum: ['merge', 'fix-then-merge', 'block'] },
    must_fix: { type: 'array', items: { type: 'object', properties: {
      id: { type: 'string' }, title: { type: 'string' }, detail: { type: 'string' }, evidence: { type: 'string' },
    }, required: ['id', 'title', 'detail', 'evidence'] } },
    notes: { type: 'string' },
    gates: { type: 'string' },
    comment_url: { type: 'string' },
  },
  required: ['verdict', 'must_fix', 'notes'],
}
const REFUTE_SCHEMA = {
  type: 'object',
  properties: { refuted: { type: 'boolean' }, reason: { type: 'string' } },
  required: ['refuted', 'reason'],
}
const FIX_SCHEMA = {
  type: 'object',
  properties: {
    status: { type: 'string', enum: ['pushed', 'blocked', 'decision'] },
    head: { type: 'string' }, fixed: { type: 'array', items: { type: 'string' } },
    plants: { type: 'string' }, notes: { type: 'string' }, blocker: { type: 'string' },
  },
  required: ['status', 'notes'],
}

// ---- Build (skipped in review mode) ----
let build
if (A.mode === 'review') {
  build = { status: 'ready', prs: A.prs, notes: 'review mode: existing PRs', gates: '', plants: '' }
  log(`${A.id}: review mode on ${A.prs.map(p => '#' + p.number).join(', ')}`)
} else {
  phase('Build')
  build = await agent(`You are the builder for epic item ${A.id} (issue #${A.issue}): ${A.title}.
${A.mode === 'finish'
  ? `FINISH existing work. State left by a stopped builder:\n${A.handoff}\nReuse the existing worktree(s) and branch(es) named there; first inspect \`git status\`, \`git log @{u}..\` and the open PRs so you continue from exactly where it stopped (if any uncommitted change looks like a leftover planted break rather than real work, revert just that change).`
  : `BUILD from scratch on a new branch ${A.branch} in a new worktree ~/.claude-worktrees/er-v1/${A.dir}, from freshly fetched ${A.base}.`}
Item-specific instructions from the coordinator:
${A.extra}
${RULES}
Deliver: the item complete per its approved plan, every done-when test proven by a plant, the acceptance gates green, PR(s) opened (against ${A.base}, or stacked if split; include any PR already open for this item). Return the structured result: status, prs in stack order (number, branch, base, head SHA, counted lines, worktree), gates, plants, notes (anything unfinished or uncertain), blocker.`,
    { label: `build:${A.id}`, phase: 'Build', model: 'opus', schema: BUILD_SCHEMA })
}

if (!build || build.status !== 'ready' || !build.prs || !build.prs.length) {
  log(`build did not reach ready: ${build ? build.status : 'null'}`)
  return { id: A.id, stage: 'build', build }
}
log(`${A.id}: ${build.prs.length} PR(s): ${build.prs.map(p => '#' + p.number).join(', ')}`)

// ---- Review loop per PR, in stack order (a fix to a lower PR merges up) ----
const results = []
for (let k = 0; k < build.prs.length; k++) {
  const pr = build.prs[k]
  const children = build.prs.slice(k + 1).map(p => p.branch)
  let head = pr.head
  let prevReviewed = null
  let review = null
  const history = []
  for (let round = 1; round <= 3; round++) {
    const isDelta = prevReviewed !== null
    phase(isDelta ? 'Delta' : 'Review')
    review = await agent(`You are an independent ${isDelta ? 'DELTA ' : ''}reviewer for PR #${pr.number} in ${REPO} (epic item ${A.id}, issue #${A.issue}): ${A.title}. Branch ${pr.branch}, base ${pr.base}, head ${head}${isDelta ? '' : ' (fetch first; if the branch has moved past this head, review the latest head and say so)'}.
${isDelta
  ? `Review ONLY the delta ${prevReviewed}..${head}. It must close these must-fixes from the last round, each with plant evidence:\n${JSON.stringify(history[history.length - 1].kept, null, 1)}\nCheck nothing else changed beyond them plus any release merge.`
  : `Review the whole diff against ${pr.base} (use the merge base). Do not trust the PR body's claims: re-read the code, re-apply every planted break yourself and confirm each fails, and run the gates the item requires.${A.prior_review ? `\nEarlier review context (a previous review round was interrupted; verify these items are now closed, but review independently):\n${A.prior_review}` : ''}`}
Spec checklist from the coordinator:
${A.review_focus}
${RULES}
Work in a fresh detached review worktree ~/.claude-worktrees/er-v1/review-${pr.number} at the PR head; remove it when done. Must-fixes are ONLY real defects: a done-when item missing or unproven, a spec, approval-condition or decision violation, a correctness bug, a behaviour change on the flag-off path, a real-data risk, a test that passes vacuously, an inaccurate claim in the PR body, or the stop line exceeded. Everything else goes in notes. Post the verdict as a PR comment (\`gh pr comment\`), with must-fixes numbered. Return the structured verdict.`,
      { label: `${isDelta ? 'delta' : 'review'}:#${pr.number}:r${round}`, phase: isDelta ? 'Delta' : 'Review', model: 'opus', schema: REVIEW_SCHEMA })
    if (!review) { log(`review null for #${pr.number}`); break }
    const mf = review.must_fix || []
    if (review.verdict === 'merge' || mf.length === 0) { history.push({ round, verdict: review.verdict, kept: [] }); break }

    // ---- Refute each must-fix with two skeptics ----
    const judged = await parallel(mf.map(m => () =>
      parallel([0, 1].map(i => () => agent(`Skeptic ${i + 1}: try to REFUTE this must-fix raised by a reviewer on PR #${pr.number} in ${REPO} (head ${head}, base ${pr.base}, issue #${A.issue}).
Must-fix ${m.id}: ${m.title}
Detail: ${m.detail}
Reviewer evidence: ${m.evidence}
Check it against the actual code and the approved plan and decisions on issue #${A.issue} (read its comments). refuted=true ONLY with concrete evidence that the finding is wrong (the code already handles it, the claimed behaviour does not occur, or the plan explicitly allows it). If it holds or you are unsure, refuted=false. Read-only: use \`git show\`/\`git grep\` on the fetched head; you may build or run a single test in a scratch detached worktree under ~/.claude-worktrees/er-v1/refute-${pr.number}-${m.id}-${i} (remove it after; worktree commands need the sandbox off). Heavy checks through ~/.claude/scripts/locked.sh. Return a compact reason.`,
        { label: `refute:#${pr.number}:${m.id}:${i + 1}`, phase: 'Refute', model: 'opus', schema: REFUTE_SCHEMA })))
      .then(vs => ({ m, refutes: vs.filter(Boolean).filter(v => v.refuted).map(v => v.reason) }))))
    const kept = judged.filter(Boolean).filter(j => j.refutes.length < 2).map(j => j.m)
    const dropped = judged.filter(Boolean).filter(j => j.refutes.length >= 2)
    if (dropped.length) log(`#${pr.number}: dropped ${dropped.length} refuted must-fix(es): ${dropped.map(d => d.m.id).join(', ')}`)
    history.push({ round, verdict: review.verdict, kept, dropped: dropped.map(d => ({ id: d.m.id, title: d.m.title, why: d.refutes })) })
    if (!kept.length) { review = Object.assign({}, review, { verdict: 'merge', must_fix: [] }); break }
    if (round === 3) { log(`#${pr.number}: still has must-fixes after 3 rounds`); break }

    // ---- Fix ----
    phase('Fix')
    const fix = await agent(`You are the fixer for PR #${pr.number} in ${REPO} (epic item ${A.id}, issue #${A.issue}). Branch ${pr.branch} at ${head}; work in its worktree (${pr.worktree || 'find it with `git worktree list`; if none exists, add one for ' + pr.branch}).
Apply these verified must-fixes, each with a test proven by a plant where a test applies:
${JSON.stringify(kept, null, 1)}
${dropped.length ? `These were raised but refuted; do NOT apply them:\n${JSON.stringify(dropped.map(d => ({ id: d.m.id, title: d.m.title, why: d.refutes })), null, 1)}` : ''}
Correct the PR body wherever a must-fix concerns its claims. Merge origin/release/v1.0.0 in if the branch is behind (a normal merge). Commit as normal commits and push. ${children.length ? `Then merge ${pr.branch} into each stacked child branch, in order (${children.join(', ')}), and push each (normal merges, no force).` : ''}
${RULES}
Return status, the new head SHA of ${pr.branch}, which must-fix ids you fixed, plant outputs and notes.`,
      { label: `fix:#${pr.number}:r${round}`, phase: 'Fix', model: 'opus', schema: FIX_SCHEMA })
    if (!fix || fix.status !== 'pushed' || !fix.head) { log(`fix did not push for #${pr.number}`); history.push({ round, fix }); break }
    prevReviewed = head
    head = fix.head
  }
  results.push({ number: pr.number, branch: pr.branch, base: pr.base, head, verdict: review ? review.verdict : null, must_fix_open: review ? review.must_fix : null, history })
}

return { id: A.id, build: { prs: build.prs, gates: build.gates, notes: build.notes }, results }
