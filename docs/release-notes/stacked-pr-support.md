# Stacked PRs in the TUI open hub and desktop branch card

## In plain terms

- **What changed.** In a repo whose branch belongs to a
  [stack](https://github.com/github/gh-stack), pressing `o` in the terminal now
  lists the whole stack: one row per layer, in order from the top of the stack
  down to the trunk, each with its branch name and PR number. In the desktop app
  the branch header grows a dropdown showing which layer you're on (`2 / 4`) that
  switches the view to another layer.
- **TL;DR.** Stacked PRs are visible in both UIs — `o` in the terminal, the
  branch header's stack dropdown on the desktop.

## Desktop: the branch header stack control

The **Branch card** in the right panel shows a stack control beside the branch
name whenever the viewed branch is a local checkout that belongs to a stack:

- The collapsed button reads **`2 / 4`** — your layer's position counting from the
  top of the stack — alongside the total layer count (a layer without a PR yet
  still counts).
- Opening it lists every layer, top-of-stack first: branch name, state (`open`,
  `merged`, `queued`, `needs rebase`) and PR number. **Your layer is highlighted**
  (accent branch name + check mark) and the trunk closes the list.
- **Picking another layer opens that PR for review**, replacing the current view —
  the same thing the terminal's `o` → Enter does. Hold
  <kbd>Cmd</kbd>/<kbd>Ctrl</kbd> to open it in a new tab instead.
- Layers with no PR yet are listed but not selectable, and the header's refresh
  button re-runs `gh stack view` after a push or rebase.
- The lookup is **lazy** — first time the control is opened, never on tab load —
  and cached per tab. It runs off the main thread, so `gh` can't freeze the
  window.
- Until that first lookup, a branch with a PR shows a quiet **Stack** placeholder
  so the control can be opened at all. Once a lookup lands, a branch that isn't
  in a stack (or a machine without the extension) drops the control, so the
  header stays quiet; branches without a PR never show it.
- `gh stack view` reads the **checked-out** branch, so the control only appears
  when the branch the tab is showing is actually checked out: a remote-PR tab, or
  a local PR view whose head lives only in a fetched ref, has nothing to read and
  shows no control.

## What changed

The **open hub** (`o`) gains a `── Stack ──` section above the current-PR
section:

```
OPEN (Enter=review, b=browser, Esc=close)
── Navigate ──
  Browse folders
  Switch worktree
  ...
── Stack ──
▶ feat/ui                        [#43]  open
  feat/api                       [#42]  open · needs rebase · current
  feat/auth                      [#41]  merged
  main                                  trunk
── Current PR ──
  Open PR in browser
```

- **Ordered top-of-stack first**, trunk last, matching how a stack merges. Your
  own layer is marked `current`.
- **Branch name and PR number** lead each row; the layer's state follows
  (`open`, `merged`, `queued`, and `needs rebase`).
- **`Enter` opens that PR for review** through the same path as **Open remote
  PR**, so each layer is reviewed as its own PR rather than as the whole stack.
- **`b` opens the selected layer on GitHub.** A layer with no PR yet still shows
  its branch, just not selectable.
- **`Refresh stack`** re-runs the lookup.

## How it works

The list comes from the `gh stack` extension (`github/gh-stack`), which owns
stack membership, ordering, and per-layer state:

```bash
gh extension install github/gh-stack
```

`er` reads `gh stack view --json` and reverses its trunk-first output. The
command talks to GitHub, so it never runs on the UI thread: the hub opens
instantly with a `Loading stacked PRs…` row, a worker thread runs the lookup, and
the rows replace it in place — keeping your cursor where it was. Results are
cached per tab, so reopening `o` is instant.

## Graceful paths

None of these is an error state:

- **Extension not installed** → the row reads `gh stack extension not installed`.
- **Branch not in a stack** → the row carries `gh`'s own explanation
  (`current branch "x" is not part of a stack`).
- **Remote-PR tabs** → the section says it isn't available, since a remote tab's
  checkout isn't the PR's branch.
- **Branch not checked out** → a local PR/branch view whose head lives only in a
  fetched ref says the branch isn't checked out, rather than showing the stack of
  whatever the working tree happens to have.

A `gh stack view` that fails for some other reason (no `gh`, auth, network) is a
real error: it's logged with the repo and branch, and the control stays put
showing the reason so you can retry.
