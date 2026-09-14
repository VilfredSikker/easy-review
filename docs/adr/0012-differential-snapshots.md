# Snapshots omit hunks the frontend already holds

Hunk lines dominate snapshot payloads, so the backend remembers, per view, the `delta_key` of the content it last delivered for each file (`SentFilesState`, keyed by a view token covering tab, mode, branch and filter). A file whose content the frontend already holds ships with `hunks_omitted = true` and no hunk payload, and the frontend splices the previous snapshot's hunks back in. Omitted files consume none of the IPC line budget (`SNAPSHOT_DIFF_LINE_BUDGET`), so the budget throttles only what actually changed.

## Considered Options

**Trust the frontend's cache and send only the files that changed.** Simpler — no key bookkeeping, no re-fetch path — but a cache miss is silent and renders hunks that no longer exist. The downgrade turns that same miss into a lazy stub the viewport loader re-fetches, so recovery is automatic.

## Consequences

- `delta_key` covers inline comment threads as well as hunk lines, so commenting on a file resends that file's hunks. That is deliberate: the frontend never holds a thread set the backend does not know about.
- Mis-recording is a performance bug, never a correctness one. Recording content that was never delivered costs a stub re-fetch; failing to record costs a redundant resend. Keep it that way — anything handing file content to the frontend should call `record_sent_file`, and a from-scratch rebuild must reset the map (`get_snapshot`), or every file downgrades and re-fetches one at a time.
- Memory is capped at eight view tokens (`SENT_FILES_VIEW_CAP`), evicted oldest-first, so a view switched away from long enough resends its files in full.
