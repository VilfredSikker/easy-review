# Mutations go through the command wrapper that ingests the returned snapshot

Desktop mutations go through `app.cmd` (`desktop-ui/src/lib/stores/app.svelte.ts`), which invokes the command, ingests the returned snapshot, and routes a failure to the error banner, the toast, and the frontend log. Raw `invoke` stays for results that must not replace the live snapshot: a non-snapshot return (an export string, a provider list, a terminal write), or a snapshot the caller ingests itself. The rule is written down because the obvious tidy-up — moving every remaining raw `invoke` onto the wrapper — would, for that second group, replace the live view with the snapshot its caller was already handling.

## Consequences

- Each exception has to be named. A raw `invoke` of a command that returns `AppSnapshot`, with no ingest anywhere in the caller, is the failure this record exists to prevent: the write lands on the backend and the UI keeps painting the old state until an unrelated revision event arrives.
- `app.cmd` also carries the tab-change ordering, the loading flags, and the optimistic paths for the reviewed and review-comment commands. A raw `invoke` that looks equivalent to the wrapped call skips all of them.
