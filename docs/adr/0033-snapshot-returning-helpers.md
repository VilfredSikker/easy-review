# A snapshot-returning helper says something about the next poll, not only this one

Two helpers build a command's reply, and the difference is invisible in the reply itself. `snap_from_confirmed` sets the `last_sent_*` revision markers to the current values, so the next poll compares equal and answers `poll_skip` — right when the frontend already holds exactly this content. `snap_from_command` deliberately leaves those markers misaligned (`wrapping_add(1)`), forcing the next poll to emit a full content snapshot — right when the command changed something the frontend has not seen.

Which helper a command returns from is a decision with a cost in both directions, and it is spread across roughly sixty return sites. Using the confirming helper on a view-changing command loses the forced resend, so the frontend keeps rendering pre-change content until something unrelated invalidates it. Using the invalidating helper after an optimistic write adds a redundant full rebuild of a view the frontend already painted.

## Considered Options

**One helper, and let the poll figure it out.** Rejected: the poll's comparison is exactly what cannot tell the two apart. The revision markers are the only signal, so someone has to set them deliberately at the point the command knows which case it is.

## Consequences

- A new command picks a helper by asking whether the frontend already has this content, not by copying the neighbouring command.
- The choice is not observable from the response body, so a mistake here does not fail a test that asserts on the returned snapshot — it shows up as a stale view or a wasted rebuild, later.
