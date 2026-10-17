# Browser feature refactor

Status: planned. No task has started.
Written 2026-10-17 against branch `browser-implementation` at `c96dc0f12`.

This folder holds everything needed to pick the work up cold: the reasoning, the
evidence, and one handover file per task. Start here, then open the task you are
assigned. Every task file is self-contained; this page holds only what all tasks
share.

## In plain terms

- **The problem.** The desktop app has an in-app browser so a reviewer can use the
  app an AI just changed, and click on part of it to leave a note for the agent.
  Today that browser forgets its page whenever a dialog opens, can skip navigation
  between local servers on different ports, sends page messages to the wrong tab,
  and runs its annotation script inside the page itself — where the page's own
  JavaScript can forge notes. Screenshots never reach the agent.
- **Why it matters.** Notes go to coding agents. A note attached to the wrong page,
  or written by the page instead of the reviewer, sends the agent the wrong way.
- **The fix.** Keep the native Tauri webview, but give each review tab a stable
  browser identity owned by the backend; keep pages alive when dialogs open; move
  the annotation script into a script world the page cannot reach; draw the note
  editor in the app instead of the page; capture a cropped screenshot of the
  selected element; and hand notes plus images to agents in one prompt format,
  also served through er-mcp.
- **TL;DR.** Rebuild the review browser so pages survive normal use and every note
  an agent receives was written by the reviewer, about the right element, with a
  picture.

## Mission and definition of done

Ship the rebuilt browser as the only browser path on macOS. Windows and Linux keep
building and say plainly which capabilities they lack.

- [ ] Opening any modal and closing it returns the same page with form text,
      scroll position and history intact (task T09 acceptance).
- [ ] Navigating between `localhost:5173` and `localhost:3000` on the same path
      changes the page (T07).
- [ ] A page script cannot create, change or submit an annotation (T10, T11).
- [ ] A saved annotation carries a cropped screenshot whose outline matches the
      target at Retina and non-default zoom (T13).
- [ ] An MCP client receives an annotation's prompt text and its image through
      er-mcp (T15).
- [ ] The old renderer, the in-page composer, the proxy fallback and the
      `browser-remote` capability are deleted (T16).

**Risk: high.** The work changes a trust boundary (what a web page can send the
app), replaces stored annotation data, and depends on native WebKit behavior that
has not been proven yet. Read the global stop rules below before every task.

## Decisions already made (do not reopen)

Made by the project owner on 2026-10-17. If a task seems to need a different
answer, stop and ask; do not change course on your own.

1. **Keep Tauri and the native child webview.** No Electron, no custom Chromium.
2. **macOS first.** Other platforms keep building and report missing capabilities.
3. **No rollback or downgrade support.** The only user is the project owner.
4. **No v1 migration.** v2 annotations go to a new file,
   `browser-annotations.json`. The old `ui-annotations.json` and its
   `screenshots/` folder are ignored and left on disk.
5. **Popups open in the external browser.** User-activated `window.open` and
   `target=_blank` go to the default browser; popups without user activation are
   denied. In-app popups are deferred.
6. **Agent handoff = one prompt format + an er-mcp tool.** Copy prompt and agent
   actions use the format in [the spec](reference/spec.md#persistence-and-agent-handoff);
   er-mcp serves text plus images. No portable bundle export.
7. **Confirmed bugs are fixed inside the phase that replaces their code**, not as
   separate fixes on `main`.
8. **Sending to an agent is always a reviewer action.** Saving an annotation never
   starts or messages an agent.

## Tasks

Do tasks in order unless the dependency column says otherwise. "Proof" tasks
answer a yes/no question about native behavior; a "no" stops the tasks that
depend on it until the owner picks another approach.

| ID | Task | Depends on | Label | Size |
| --- | --- | --- | --- | --- |
| T01 | [Fixture pages and native probe harness](tasks/T01-fixtures-and-probe.md) | — | ready-for-agent | 1–2 d |
| T02 | [Proof: park a live page under modals](tasks/T02-proof-parking.md) | T01 | ready-for-human | 1–2 d |
| T03 | [Proof: annotation runtime in a WebKit content world](tasks/T03-proof-content-world.md) | T01 | ready-for-human | 1–2 d |
| T04 | [Proof: host editor surface above the page](tasks/T04-proof-editor-surface.md) | T01 | ready-for-human | 1 d |
| T05 | [Proof: navigation events, snapshot, data store; write the ADR](tasks/T05-proof-navigation-capture-adr.md) | T02–T04 | ready-for-human | 1–2 d |
| T06 | [Stable review-tab and browser identity](tasks/T06-stable-browser-identity.md) | T05 | ready-for-agent | 2 d |
| T07 | [Backend navigation state and address rules](tasks/T07-navigation-state.md) | T06 | ready-for-agent | 2–3 d |
| T08 | [Message envelope and sender validation](tasks/T08-message-envelope.md) | T06 | ready-for-agent | 1–2 d |
| T09 | [Non-destructive presentation and overlay leases](tasks/T09-presentation-leases.md) | T06, T02 | ready-for-agent | 3–4 d |
| T10 | [Page runtime in the content world](tasks/T10-page-runtime.md) | T03, T08 | ready-for-agent | 3–4 d |
| T11 | [Host editor and host-owned drafts](tasks/T11-host-editor-drafts.md) | T04, T10, T12 | ready-for-agent | 2–3 d |
| T12 | [Annotation storage v2](tasks/T12-storage-v2.md) | — | ready-for-agent | 1–2 d |
| T13 | [Screenshot capture pipeline](tasks/T13-screenshot-capture.md) | T05, T10, T11, T12 | ready-for-agent | 2–3 d |
| T14 | [Agent prompt format, copy and export](tasks/T14-prompt-format.md) | T12 | ready-for-agent | 1–2 d |
| T15 | [er-mcp browser-annotations tool](tasks/T15-mcp-tool.md) | T12, T14 | ready-for-agent | 1–2 d |
| T16 | [Switch over and delete the old path](tasks/T16-switch-over.md) | all | ready-for-agent | 1–2 d |

The sizes sum to 24–38 working days, about 5–8 weeks for one developer. T12 and
T14 do not touch native code and can run in parallel with the proof tasks.
T10 and T11 merge together in one PR.

When you start a task, change its `Status:` line to `in-progress`; when its
verification passes, set `done` and add the commit hash. Update the table label
here only if the owner changes it.

## Reference material

- [reference/spec.md](reference/spec.md) — the full design, with reasons. Task
  files quote the parts they need; read the spec when a task's "Why" is not enough.
- [reference/evidence.md](reference/evidence.md) — what was verified in this code
  base and in ChatGPT 26.1007, with file and line references.
- `reference/chatgpt-report-{main,preload,renderer}.md` — the full static reading
  of ChatGPT's browser, including every runtime message type and the exact prompt
  strings. Line numbers refer to prettier-formatted copies; the evidence file
  says how to recreate them.

## Shared orientation

- **Branch.** Feature work lands on the current release branch, not `main`.
  Resolve it before you open a PR:
  ```bash
  git fetch origin --quiet
  git for-each-ref --format='%(refname:short)' 'refs/remotes/origin/release/v*' | sort -V | tail -1
  ```
  Build each task on a branch from `browser-implementation` (or from the previous
  task's branch when they depend on each other).
- **Checks.** Heavy checks never run in parallel. Wrap every one in the machine
  lock: `~/.claude/scripts/locked.sh <repo-root> <command…>`.
  - Desktop Rust tests: `cargo test -p er-desktop <filter>`
  - Engine tests: `./scripts/er-tui.sh test -p er-engine <filter>`
  - MCP tests: `cargo test -p er-mcp <filter>`
  - Frontend: `cd desktop-ui && bun run test <file>`, `bun run check`, `bun run lint`
  - Lint gates: `just clippy`, `just lint-ui` (ADR 0038: fix violations; never
    grow a baseline)
  - Docs: `just docs-check`
- **Real app.** Proof tasks and acceptance checks need the Tauri app in a real
  window: `./scripts/tauri-dev.sh`. Storybook and Playwright do not count as proof.
- **Docs to read first.** `CLAUDE.md`, `CONTEXT.md`, and the ADRs each task names.

## Global gotchas

- The native child webview draws **above** the shell's HTML. No CSS `z-index`
  can put shell UI over it. That is why modals currently destroy it
  (`crates/er-desktop/src/browser_webview.rs`, `browser_suspend_for_overlay`).
- Tauri's `initialization_script` and `Webview::eval` both run in the **page's**
  script world. Anything they install, page code can reach.
- The engine's `App` mutex is global. Slow native work (capture, disk writes)
  runs outside it (ADR 0015).
- The desktop is push-driven (ADR 0011). Do not shorten the 30 s safety timer to
  deliver browser state; emit an event instead.
- Config is global-only (ADR 0005). Do not add repo-local browser settings.
- `objc2-web-kit` (0.3, the version Wry uses — `Cargo.lock` has 0.3.2) gates
  every class behind a Cargo feature. Enable the ones you use, for example
  `WKWebView`, `WKWebViewConfiguration`, `WKUserContentController`,
  `WKContentWorld`, `WKUserScript`, `WKScriptMessage`, `WKScriptMessageHandler`,
  `WKFrameInfo`, `WKSnapshotConfiguration`, `WKBackForwardList`; a missing feature
  shows up as "no method named …" or an unresolved import.
- Code comments must not cite task IDs, plan steps or review findings;
  `just comments-check` rejects them. Say why the code is shaped that way instead.
- `.scratch/` is gitignored. Everything a task needs is in this folder.

## Global stop rules

Stop and ask the owner, recording what you found in the task file, when:

- a proof task's answer is "no" or "only sometimes";
- a file, symbol or line a task cites does not exist or does not match;
- a step would touch something the task lists as out of scope;
- a verification fails in a way the task's "If it fails" does not cover;
- you would need to weaken, skip or delete a test to get green.
