# Review writes paint immediately and are re-applied until confirmed

A review mutation — a comment, a reply, a resolve — paints into the frontend store as soon as the user acts, is re-applied over every incoming snapshot until the backend's own copy of the change arrives, and rolls back only if the call itself fails. Waiting for the confirming snapshot was rejected because an IPC round trip is slow enough that a click with no visible effect reads as a frozen app. Paints held this way are marked unconfirmed, and the last confirmed snapshot is kept as the point to revert to.

## Consequences

The optimistic chain is serialized, so a reply targets the id its parent created a moment earlier. Writes are gated off a view that is switching, and a blocked paint states its reason instead of swallowing the action as a silent no-op — an unexplained no-op is indistinguishable from a bug.

## The painted id crosses the seam as `client_id`

The id a front end painted is an explicit parameter of the write (`client_id`), not state carried through the composer. The writer stores it verbatim when it carries the kind's `q-`/`n-`/`c-` prefix and generates a fresh id otherwise, so a stale id from another kind cannot land in the wrong sidecar. A command that writes twice — a finding reply that first creates its thread root — passes the id to the first write only; the follow-up reply gets a fresh one. That keeps the confirming snapshot's row on the painted key, which is what the re-apply in ADR 0017 matches on.
