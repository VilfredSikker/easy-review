# The desktop protocol is versioned resources

_Supersedes ADRs 0011, 0012, 0016, 0017, 0018, 0033 and 0034 once the
flag-off desktop path is deleted; until then that path runs under them._

## Context

The desktop renders from one `AppSnapshot` the backend rebuilds from the App
on every change. Freshness rides on revision counters derived from snapshot
fields (ADR 0034), the payload is cut down by remembering what the frontend
already holds (ADR 0012), and a write paints locally until a confirming
snapshot lands (ADR 0017). Each of those exists to make one whole-app answer
cheap enough to send often. With the Workspace actor (ADR 0067) and the app
registry (ADR 0070) owning the state, the backend can say which piece moved.

## Decision

The desktop reads **resources**: named pieces of published state, each with
one writer that stamps a version when it replaces the content.

- **Keys and versions.** A `ResourceKey` names one resource (`tabs`, and per
  tab its diff, reviewed marks, annotations and PR). Its version is the
  revision its writer stamped, so equal versions mean equal content and no
  hash decides freshness. A tab is named on the wire by a `TabKey` number,
  resolved by lookup in the registry's published list, never turned into a new
  engine key.
- **Three commands and one event.** `er_send(command) -> Ack`,
  `er_fetch(keys) -> Versioned<Resource>[]` (batched) and
  `er_versions(keys) -> ResourceVersion[]`. The backend pushes
  `er://resources` with the keys whose version moved, coalesced. Every failure
  is an `ErError` whose `kind` the frontend branches on.
- **The frontend refetches by version.** A store holds each shown key at the
  version it fetched and refetches only when a push, an `Ack` or a version
  check names a higher one. The client refetches what an `Ack` names as soon
  as it arrives, so a click's repaint never waits on the push's coalescing. It
  asks `er_versions` once after its listener attaches, since a push sent
  before that is lost, and again on a 30 s safety net.
- **Hunks are content-addressed.** A file's hunks are keyed by the hash of its
  own lines and never change, so they carry no version; the frontend caches
  them by hash. A request for a hash the diff has moved past answers `stale`,
  and the client refetches the diff.
- **Types come from Rust.** The wire structs live in `er-protocol`, a leaf
  crate; `ts-rs` writes their TypeScript, and the drift check holds the
  generated files to it. The three commands are typed by hand in one client
  module.
- **Wire keys are never stored.** Tab keys count from 1 in each backend
  process, so a key kept across a restart can name a different tab. A source
  scan keeps desktop-ui's wire-key modules away from `localStorage` and
  `sessionStorage`.

**The painted id still crosses the seam as `client_id`.** This is ADR 0017's
rule, unchanged: the id a front end painted is an explicit parameter of the
write (`client_id`), and the composer carries no state for it. The writer
stores it verbatim when it carries the kind's `q-`/`n-`/`c-` prefix and
generates a fresh id otherwise, so a stale id from another kind cannot land in
the wrong sidecar. A command that writes twice — a finding reply that first
creates its thread root — passes the id to the first write only; the
follow-up reply gets a fresh one. That keeps the confirming resource's row on
the painted key.

## Considered Options

- **Keep the snapshot and shrink it further.** Every shrink so far (deltas,
  split revisions, derived invalidation) added a rule both ends must agree on,
  and the snapshot still reads the App, which the service is replacing.
- **Push the content itself.** A push per change would send diffs nobody is
  looking at; a key and a version are enough to let the page decide.
- **A fingerprint per resource.** A hash computed by the reader can disagree
  with the writer's idea of a change; the writer's own stamp cannot.

## Consequences

- Until the flag-off path is deleted, each ported store is an adapter with
  one read API, fed from `app.snapshot` with `v1-service` off and from
  `er_fetch` with it on, and its tests run both modes. The snapshot's `poll`
  keeps running with the flag on for the stores not yet ported.
- A store that looks stale points at a writer that did not stamp a version,
  or a relay that did not hand it on. Shortening the safety net hides that.
