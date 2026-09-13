# Diagram agents emit JSON on stdout and the host writes the file

A diagram agent runs read-only — its allowlist is `Read` plus grep/rg/git read
commands — and emits the diagram JSON on stdout. When host-write is set, the host
alone parses that output and atomically writes `diagrams/<id>.json`
(`persist_diagram_from_agent_stdout` in `ai/diagrams.rs`); the agent is never
given a write path. The prompt carries untrusted diff content, so an agent with
`Write`/`Edit` is a prompt-injection write primitive holding the process's own
permissions. Confining the write to the host makes that structural rather than a
matter of prompt discipline.

## Considered Options

Granting the agent `Write`, scoped to the target path by prompt wording. Rejected:
the scope would live in the same prompt an injected diff can override, and the tool
allowlist is the one part an agent cannot argue with.

## Consequences

- The destination stays host-owned: the prompt does name the output path (so the
  agent knows what it is producing), but the agent has no write access to it, so
  moving where diagrams live stays a host-only change.
- Anything an agent must persist durably goes through the same
  parse-validate-write stdout path. Adding `Write` to a diagram spawn removes this
  guarantee. The prompt's `---DIAGRAM_JSON---` delimiter instructions are
  convenience only — a malformed payload writes nothing.
