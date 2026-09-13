# Diagram agents emit JSON on stdout and the host writes the file

A diagram agent runs read-only and emits the diagram JSON on stdout. When
host-write is set, the host alone parses that output and atomically writes
`diagrams/<id>.json` (`persist_diagram_from_agent_stdout` in `ai/diagrams.rs`).
The prompt carries untrusted diff content, so an agent with `Write`/`Edit` is a
prompt-injection write primitive holding the process's own permissions.
Confining the write to the host makes that structural rather than a matter of
prompt discipline.

Read-only has to be enforced per CLI family, because the providers disagree about
where write control lives:

| Family | How the write is denied |
|---|---|
| Claude | `--allowedTools` allowlist — `Read` plus grep/rg/git read commands |
| OpenCode | `OPENCODE_PERMISSION` env — denies `edit` and `bash` |
| Codex | `--sandbox read-only` |
| Cursor | `--mode plan`, its read-only mode, with `--force` dropped |

`apply_readonly_spawn` in `config/mod.rs` does the last two. Claude and OpenCode
are deliberately untouched: each already carries its own mechanism, and a second
one here would be a divergent source of truth.

The trap is `--add-dir`. It is documented as adding a directory that is
**writable**, so under a workspace sandbox it reopens exactly what the setting
above closes. A read-only run therefore withholds it. Claude and OpenCode still
receive it, since their own mechanisms deny writes and they need the directory to
reach the prepared diff; Codex and Cursor do not, and both were confirmed to read
outside their working directory with no `--add-dir` at all.

## Considered Options

**Granting the agent `Write`, scoped to the target path by prompt wording.**
Rejected: the scope would live in the same prompt an injected diff can override,
and the tool allowlist is the one part an agent cannot argue with.

**Leaning on the sandbox alone for every family.** Rejected as the only
mechanism: Claude's allowlist is finer-grained than a sandbox, and dropping it
would also drop the read commands the diagram prompt depends on.

## Consequences

- The destination stays host-owned: the prompt names the output path so the agent
  knows what it is producing, but the agent cannot write to it, so moving where
  diagrams live stays a host-only change.
- Anything an agent must persist durably goes through the same
  parse-validate-write stdout path. The prompt's `---DIAGRAM_JSON---` delimiter
  instructions are convenience only — a malformed payload writes nothing.
- **A new provider family does not inherit this.** `apply_readonly_spawn` names
  Claude, Codex, Cursor and OpenCode explicitly and leaves anything else alone,
  because its command-line contract is not known here. A family added to the
  catalog needs its own read-only enforcement before a diagram run is safe on it.
