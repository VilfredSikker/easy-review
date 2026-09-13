# A stub tab's first diff load is deferred off the app lock

Opening or activating a tab that has never loaded a diff returns the stub immediately, with `loading.tab_diff` set, and hands the real refresh to a worker thread (`kick_deferred_tab_refresh`). The loaded diff reaches the frontend through the ordinary revision-event poll, as a later poll — never as that command's own result.

Running the refresh inline would hold the `App` lock across a `git diff` and a full parse, serializing every other command behind it. The cost of not doing so is a split state machine: a command that deliberately returns an incomplete snapshot, and a second path that fills it in, which is why the frontend shows "Loading diff…" rather than an empty pane it would have to interpret.

## Consequences

- The frontend must treat "empty `files`" on a just-opened tab as *loading*, not as *no changes*. The two are distinguishable only by `loading.tab_diff`.
- Any new path that activates a tab inherits this: return the stub and schedule the refresh, rather than refreshing under the lock because it is simpler at the call site.
- The loaded diff arriving through the poll means a dropped revision emit delays it. ADR 0011's 30-second fallback is what eventually delivers it, which is one more reason not to shorten or remove that timer.
