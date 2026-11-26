# MCP calls er-service's headless functions

## Context

er-mcp served its tools from five engine modules that no other crate used
(PR resolution, the review queue, the sidecar summary, diagram uploads, and
the review feedback reader and writers), and reached `gh` and git through
handles it built itself. Phase 6 moves every front end onto er-service
(ADDENDUM-08 decisions 6 and 8). MCP is a short-lived stdio server with no
tabs, no live view and no event stream, so the workspace actor has nothing
to hold for it.

## Decision

The five modules live in er-service's `headless` module
(`crates/er-service/src/headless.rs`) as plain functions over the store and
a `Ports` value. MCP calls them from its own runtime and starts no actor.
`headless` holds what MCP calls and what those calls need, nothing more; a
scan in `crates/er-service/tests/headless.rs` fails on a function no MCP
path reaches. The server builds one `Ports` and passes it to every call, so
each `gh` and git call goes through it; `crates/er-mcp/tests/ports.rs` fails
on a handle built anywhere else.

Question and note replies and resolves have one writer there,
`write_thread_reply` and `resolve_thread_row`, which MCP and the workspace
effects both call. The reply's place is a parameter: the effect passes the
anchor it re-read from the diff it fetched, and MCP, which fetches nothing,
has the writer copy the parent row. Each caller's rows stay as they were
before the two writers merged; `roots_only` keeps MCP from resolving a reply.

## Considered Options

- **An actor per MCP call.** It would load a workspace view MCP never reads
  and add a thread hop to each tool, for no state worth keeping between
  calls.
- **One reply row for both callers.** The writer would copy the parent row
  for the desktop and the TUI too, which changes where their replies sit
  (side, range, finding link). Ruled out on #774: phase 6 changes nothing a
  user sees.
- **Leave the modules in er-engine.** er-mcp would keep an er-engine
  dependency only for them, outside the manifest rule (decision 6).

## Consequences

- MCP's tools can be tested with scripted ports: the queue's PR list joined
  the `GitHub` trait for this.
- The contract goldens in `crates/er-mcp/tests/contract/goldens/` hold what
  the tools return across the move.
- A later headless caller (the post-1.0.0 CLI) reuses these functions; it
  adds only what it calls.
