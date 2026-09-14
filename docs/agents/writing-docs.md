# Writing docs

How to write and maintain this repo's documentation. Read this before adding
or editing a doc that an agent will load.

## The rule

**The code carries what and how. Documents carry why and decisions.**

A sentence that restates what the code does is noise: the code already says it,
and the restatement is the part that rots. This is measured, not stylistic. An
audit of the internal docs checked 1,009 factual claims and found **385 wrong or
partly wrong — 39%**. The failures clustered in enumerations, file tables,
module inventories and type lists. The rationale sections held up.

A second failure mode had no fix by editing at all: docs cited things that never
existed or had been deleted — a `config.rs` that was a directory, an `er-api`
crate that was a deferred plan, a `compute_poll_revision` that was never
written. Nothing regenerates a sentence like that, so it decays into confident
fiction.

## Where each kind of thing goes

| What you have | Where it goes |
|---|---|
| A word this project uses in a specific way | `CONTEXT.md` — a definition, nothing else |
| A decision with a reason | `docs/adr/<next-number>-<slug>.md` |
| A rule that breaks something if an agent gets it wrong | the relevant `CLAUDE.md` or `agent.md`, under Traps |
| A boundary between modules, or between the engine and a front end | the module's own doc |
| Build, test, release mechanics | `docs/DEVELOPMENT.md` |
| A pointer for tools other than Claude Code | `AGENTS.md` |

## What not to write

Do not put any of these in an agent-facing doc:

- **File tables.** The tree shows the files. A hand-maintained list of them is
  wrong within a release.
- **Module inventories.** Same failure, larger blast radius — the old engine doc
  listed a `config.rs` that had become a directory.
- **Type, field and enum listings.** Readable from the definition, and they drift
  the moment a field is added. If a type needs explaining, explain *why* it is
  shaped that way, in an ADR.
- **Function-by-function walkthroughs.**
- **Numbers with no home in code.** The old docs carried a 100ms poll (it is
  50ms), a 500-line compaction threshold (it is 2000), and a 5,000-line lazy
  parse trigger (it is a byte count). Each was right once.

If a fact only exists so someone can look it up, it belongs in the code, or in a
reference doc that is generated from the code — never in prose an agent reads.

## Adding an ADR

Add one when **all three** hold:

1. **Hard to reverse** — changing your mind later costs something real.
2. **Surprising without context** — a future reader will ask "why is it like
   this?".
3. **A real trade-off** — there were genuine alternatives and you picked one for
   a reason.

If any of the three is missing, do not write an ADR. A mere implementation
choice is not a decision worth recording.

Format is in `.agents/skills/domain-modeling/ADR-FORMAT.md`. The short version:
a title, one to three sentences of context/decision/why, and optional
`## Considered Options` or `## Consequences` **only** when they carry something
new. Most of ours are ten lines. A padded ADR is worse than a short one because
it buries the reason.

**The reason is the point.** If you cannot state why the decision was made, say
so in the ADR rather than inventing a plausible-sounding rationale — an
unsourced "why" is worse than a recorded gap, because it makes a bad decision
look justified.

## Citing code

Cite a file or symbol only when the decision *is* about that file. Every citation
is a thing that can rot, so keep them few and stable:

- Cite the **module**, not the function, unless the function is the decision.
- Prefer a path over a line number. Line numbers rot fastest of all.
- A citation that no longer resolves is a doc bug, and `just docs-check` finds
  it.

## User-facing docs are a different genre

`README.md` and `docs/guide/` are for people using `er`, not for agents working
on it. How-to and reference are correct there — a user looking up a keybinding
needs the keybinding. The rule above governs docs an agent loads to change the
code.

The two genres still share one obligation: **do not state a fact you have not
checked against the code.** User-facing pages carrying the same stale claims are
still wrong, they just mislead a different reader.

## Checking your work

```bash
just docs-check
```

It runs as part of `just lint` and `just ci`, and verifies:

- every file and symbol cited by an agent-facing doc still resolves;
- every relative markdown link resolves;
- every `ADR NNNN` reference names an ADR that exists;
- every link in `docs/guide/`, the landing page and `README.md` goes somewhere.

Run it after editing any doc. It is the mechanical half of this guide — it
catches a citation that was true when written and quietly stopped being true,
which is the failure this repo has actually suffered from. What it cannot catch
is a sentence that is wrong from the start, or a fact that went missing; those
still need a reader.
