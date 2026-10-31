# A Tour switch reuses the loaded diff

The Tour regroups the diff of the view it was entered from, so switching between the two used to re-run `git diff` (or `gh pr diff`, a network round trip), re-parse it and re-hash it, only to show bytes already in memory. A switch between the Tour and its source view now keeps the loaded diff when nothing that decides the fetch has moved: the route the refresh would take, the base and head refs, the PR, the checkout, the head oid, and the diff hash. Any refresh clears that record first, so a refresh that failed halfway vouches for nothing.

The alternative was to keep refreshing on every switch so a switch could never show stale content. It was not taken because the switch was never what kept the diff current: the watcher refreshes whichever mode is on screen, and Sync re-fetches refs. Content that moved under unchanged refs — an edit in the working tree — reaches the diff the same way whether the reader switches views or not.

## Consequences

- A mode switch is not a refresh. Something that changes the diff without moving one of the recorded inputs and without a watcher event has to refresh itself; a switch will not cover for it.
- Two modes share a diff only when they would run the same fetch. A checked-out PR's Branch view is the working tree while its Tour is the PR's parity diff, so that pair still refreshes.
- A pending background preload disables reuse: it may be newer than what is loaded.
