# AI runs in the user's agent CLI, over the sidecar files

`er` does not reason about code and holds no provider client. Every AI action
builds a prompt and spawns the agent CLI the user configured (`ai_hub.providers`:
a command, its args, and the family whose argument conventions it follows —
Claude, Codex, Cursor, OpenCode, or an explicit override). The agent's only
interface with `er` is files. The parent prepares the diff, writes
`diff-tmp`/`diff-annotated` atomically under a lock, and pins that SHA-256 into
the prompt as the `diff_hash` the agent must record; the agent writes its sidecar
into the tab's view bucket; `er` renders what it finds there. The reviewer's
questions and notes are the return half of the same loop — the actions that
answer them spawn an agent and rewrite the store in place, with the previous
revision left beside it as `.prev.json`.

The user is already authenticated in whichever CLI they use, so integration costs
nothing to store, scope, refresh, or leak — the argument that keeps `gh` out of
the engine (`docs/adr/0026-gh-cli-not-http-api.md`). It also keeps `er` neutral
between providers: adding one is a catalog entry.

## Considered Options

**Call provider APIs directly**, with a key in config and an HTTP client in the
engine. Rejected: it puts a credential in the config file, adds an authentication
lifecycle of our own (refresh, scope, revoke) and a model-selection UI to build,
and turns the provider into a build-time decision instead of a command the user
already runs.

## Consequences

- A new AI capability is a prompt plus a spawn path. Prompts are compiled in, so
  a prompt and its caller ship and are edited as one unit
  (`docs/adr/0023-self-contained-agent-prompts.md`).
- The exit status is the only success signal from a spawn. The stricter check —
  every artifact the contract names rewritten, with a matching hash — was written
  once, as `ArtifactBaseline::capture` / `validate` in
  `crates/er-engine/src/agent_runtime.rs`, and had no caller outside its own
  tests. It is not the only such check in the tree: `sidecar_upload` parses each
  sidecar and compares its `diff_hash` before writing, but that guards the upload
  path, not a spawn. A sidecar whose stored hash disagrees with the current diff
  is marked stale rather than refused (`docs/adr/0010-staleness-is-two-conditions.md`).
- The agent runs with the process's own permissions. Where a prompt carries
  untrusted diff content, the write stays with the host and the agent's output
  arrives on stdout (`docs/adr/0022-host-written-diagram-sidecars.md`).
- Sidecar paths resolve through `crates/er-engine/src/storage.rs`, never by hand
  (`docs/adr/0003-managed-review-storage.md`).
