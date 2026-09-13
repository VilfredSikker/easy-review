# Terminal output streams over events, not the snapshot

Terminal sessions are not snapshot state and do not bump the desktop revision; their output reaches the frontend only through terminal events (`terminal-output`, closed by `terminal-exit`). Output arrives at byte rate, so routing it through the poll-and-revision contract would either flood the wire with rebuilt snapshots or drop output between polls. A PTY is also an OS resource that has to be killed and reaped, which does not belong in a state model that is rebuilt and diffed on every poll.

## Consequences

- A terminal's contents cannot be restored from a snapshot. Whatever the frontend did not buffer from the event stream is gone on reload.
- Terminal activity never triggers a revision, so a poll-based refresh will not pick it up. Code that waits on the revision event to observe a terminal session is waiting on the wrong signal.
