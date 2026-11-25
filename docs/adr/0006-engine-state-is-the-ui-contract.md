# The engine state model is the contract between the TUI and the desktop app

_Superseded by ADR 0068._

The engine no longer holds a front-end state model: the App left in 1.0.0 (P77), both front ends read the sections the service publishes (ADR 0071), and view state with one owner lives in its front end (ADR 0068). The rest records the decision as it stood.

`er-engine` holds the application state, and both `er-tui` and `er-desktop` link it, so every change to that state model lands in two front ends at once. When a front end needs something the model does not express, adapt it by adding an explicit field or by handling the translation in the front end — never by widening or repurposing an existing field so it carries a second meaning. A repurposed field compiles for both front ends, so nothing flags it, and the damage surfaces only at runtime in whichever front end the change was not written for — normally the terminal, whose keys and render paths are the ones nobody re-ran. The desktop's translation already has a home: the resources `er-service` publishes over the wire types in `er-protocol` (ADR 0072), so a desktop-only need belongs there rather than in the shared model.

## Considered Options

- **Overload an existing field to satisfy the desktop** — reuse a flag, loosen a type, or let one field mean different things depending on the mode. The smallest diff, and the one to expect a future contributor to reach for. It survives review because it type-checks.

## Consequences

Engine fields that only one front end reads are deliberate. They look like dead weight during a later cleanup, and removing them as unused re-opens the overload path.
