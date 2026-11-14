# The Workspace actor and the crate graph

## Context

The desktop's open commands resolve a PR, fetch its state through `gh`, and
place a tab while holding `Arc<Mutex<App>>`. That lock is the one every
snapshot and every other command needs, so a slow `gh` call on one PR stalls
the whole UI. The TUI does the same work through its own code path, so the two
front ends drift.

Phase 4 fixes both: the work moves into `er-service`, which both front ends
call, and `er-service` owns the state for one repository. P38 added the crate
skeleton; P40 builds the actor inside it.

## Decision

**One actor per repo/worktree.** `WorkspaceHandle` carries a command sender, a
subscriber list and the published view. The actor thread owns the state and
processes one message at a time.

**Commands carry every parameter the effect needs.** `OpenTarget` names a PR
URL, an owner/repo/number, or a project id, and always includes the repo root.
The desktop resolves app-scoped records — the project list — on its side of the
seam before sending, so the service does not read the store in P40. Those
records move to the service in P45, not now.

**Effects run off-actor through `WorkspaceDeps`.** The actor takes
`WorkspaceDeps`, not `Ports`, so a new dependency (P45's shared app registry)
is added to one struct. Effect code calls `deps.ports()`; a thread-local guard
panics if that call happens on an actor thread. One subprocess call on the
actor thread would serialize every other command behind it, so the failure is
made loud at the call site. A source-scan test holds the loop to it too.

**The read path is a published view, not a query.** The actor builds the next
`WorkspaceView`, bumps its revision, and swaps an `Arc` behind an `RwLock`.
Readers clone the current `Arc` and never wait on the actor. A `Query` command
answered on the actor thread was rejected: it puts every frame of every reader
in the same queue as the subprocess work, and one slow effect then delays the
next paint.

**Replies are bounded.** `send_and_wait` waits `REPLY_TIMEOUT` (30s) and
returns `Timeout`; the actor keeps running. A caller that needs a tighter bound
passes its own deadline.

**This grows ADR 0015's rule.** ADR 0015 sends blocking commands to a
`run_blocking` worker. On the flag path the wait is explicit: `send` returns a
reply `Receiver`, so a command that blocks on it inside `run_blocking` must
bound the wait — `send_and_wait`, which carries `REPLY_TIMEOUT`, or its own
`recv_timeout`. An unbounded `recv()` would hold a worker for as long as the
actor lives and turn a dead actor into a frozen command.

**The crate graph stays headless.** `er-service` depends on `er-engine` with
`default-features = false` and has no front end in its graph. `v1-service` is
an optional feature on `er-desktop` and `er-tui`; the default build is
unchanged. D21 applies: the flag-on desktop does not render from the view until
P47, so the routed open commands acknowledge the command and return an empty
snapshot. `open_local_branch` and `open_worktree` stay on the old path; they are
noted on #538.

## Consequences

**Good.** An open command no longer takes the App lock, so a slow `gh` call
cannot stall snapshots. Both front ends will call one implementation, and the
actor's state has one writer.

**Costs.** One effect thread per command in P40; a pool can come later if the
thread cost shows up. The flag-on desktop is visibly incomplete until P47
(recorded in D21). Each repo root keeps one live actor for the process's
lifetime; there is no eviction yet.
