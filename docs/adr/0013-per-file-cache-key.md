# Cache keys are per file

Each `FileSnapshot` carries a `cache_key`: a content hash of that file's own hunks. Highlight caches and render blocks key on it (highlight cache also on theme and path) rather than on a hash of the whole diff. The whole-diff key meant any edit anywhere re-keyed every file, so one keystroke in one file discarded the highlight cache for all of them — the caches went cold precisely when the review was most active.

## Consequences

- The frontend render-block cache keys by path, not by `FileSnapshot` object identity, so it survives snapshot replacement. Keying on identity would silently undo this, since every poll rebuilds the snapshot objects.
- A file's key changes only when its own hunks change. Anything derived per file can be reused across a diff-wide refresh, but anything aggregate (counts, filters, whole-diff hashing for staleness) still needs the diff-level hash.
