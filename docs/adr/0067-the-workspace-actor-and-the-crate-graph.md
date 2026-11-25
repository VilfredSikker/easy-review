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

**This grows ADR 0015's rule.** ADR 0015 moves a blocking command body onto
`tauri::async_runtime::spawn_blocking`. Waiting on an actor is one such block,
and it is explicit: `send` returns a reply `Receiver`, so a body that blocks on
it must bound the wait — `send_and_wait`, which carries `REPLY_TIMEOUT`, or its
own `recv_timeout`. An unbounded `recv()` would hold a worker for as long as the
actor lives and turn a dead actor into a frozen command.

**The crate graph stays headless.** `er-service` depends on `er-engine` with
`default-features = false` and has no front end in its graph. Both front ends
run on it.
An open goes through `er_send` like every other command: its `Ack` names the
resources it moved, and the page refetches `tabs` and the new tab's content
from the published view (ADR 0072).

## Consequences

**Good.** An open takes no lock that a read needs, so a slow `gh` call
cannot stall the page. Both front ends will call one implementation, and the
actor's state has one writer.

**Costs.** One effect thread per command in P40; a pool can come later if the
thread cost shows up. Each repo root keeps one live actor for the process's
lifetime; there is no eviction yet.
