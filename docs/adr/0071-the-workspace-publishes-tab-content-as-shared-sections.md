# The Workspace publishes tab content as Arc-shared sections

## Context

ADR 0067 makes the Workspace actor the one writer of a repo's state, and
readers get an immutable view per revision. Until now the view carried the PR
targets, the arena runs and the review flows, but no open tab's content: the
files, the reviewed marks and the AI state lived only in the desktop's
`TabState`. The flag-on desktop cannot build a snapshot from the service until
the service holds them.

The actor publishes a new view on every arena tick, and a publish clones the
whole view. A diff of thousands of files, and the AI state assembled over it,
cannot be deep-copied at that rate.

## Decision

The Workspace publishes each open tab's content as `Arc`-shared sections with
their own revisions, so a publish copies pointers and a reader rebuilds only
the sections that moved.

- `WorkspaceView.tabs` maps each `TabKey` to its `TabContent`: the resolved
  tab, the context a command on it carries, and three sections, the diff, the
  reviewed marks and the AI state. Each section stamps the revision that last
  replaced it. A section is replaced, never changed in place, so equal
  revisions mean equal content.
- A load runs as an effect, by the same engine steps a `TabState` refresh
  runs, and the actor applies it. The actor never makes a key: the caller
  gives one, so the keys can come from the tab list today and from the
  front end's own tabs later.
- A write effect hands the sidecar or the reviewed map it saved back to the
  actor beside its reply, and the actor patches every tab that reads that
  bucket, duplicate tabs included. The public reply shapes do not change.
- A load is applied only over the content it started from. A write that
  landed while it ran wins, and the load answers that the tab moved.
- A lazy parse replaces the parsed files' `Arc`s and shares the rest.

## Consequences

- An arena tick copies one pointer per tab. A reader that compares section
  revisions skips the sections an event did not touch.
- `AiState` must be `Sync` and `Clone` to sit behind an `Arc`, which is why
  its comment index is a `OnceLock` and a clone drops it.
- `WorkspaceView` compares and prints by key and section revision, not by
  content, since the AI state has no equality of its own.
- Sidecars written outside the Workspace, by `er-mcp` or an agent, reach the
  content only on the next load.
- The service's load and `TabState`'s refresh are two sequences over one set
  of engine steps. A parity test over the four diff modes keeps them in step.
