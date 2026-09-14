# Review writes paint immediately and are re-applied until confirmed

A review mutation — a comment, a reply, a resolve — paints into the frontend store as soon as the user acts, is re-applied over every incoming snapshot until the backend's own copy of the change arrives, and rolls back only if the call itself fails. Waiting for the confirming snapshot was rejected because an IPC round trip is slow enough that a click with no visible effect reads as a frozen app. Paints held this way are marked unconfirmed, and the last confirmed snapshot is kept as the point to revert to.

## Consequences

The optimistic chain is serialized, so a reply targets the id its parent created a moment earlier. Writes are gated off a view that is switching, and a blocked paint states its reason instead of swallowing the action as a silent no-op — an unexplained no-op is indistinguishable from a bug.
