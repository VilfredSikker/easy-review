# er-store — typed artifact storage

Every sidecar file the engine, TUI and desktop keep goes through this crate. A
caller builds an `ArtifactKey` from a `Scope` and a slot, then calls
`ArtifactStore`; the store owns the bucket path, the temp-file rename, the size
guard and the version check. Never join a store path by hand: the engine's
equivalence test pins `Scope` to `storage::view_bucket_dir` and
`storage::pr_bucket_dir` until every caller migrates.
[ADR 0056](../../docs/adr/0056-er-store-format.md).

## One format

Every stored file is the `{kind, version, diff_hash, producer, written_at,
payload}` envelope, and only a store write produces it. An agent still sees the
bare payload schema its prompt describes: it hands that payload over as staged
output or an er-mcp upload, and the host commits it through a typed write. A
file with no envelope reads as malformed and stays on disk untouched until a
typed write replaces it. ADR 0063.

## A new kind

Implement `Artifact` for the payload type, set `KIND`, `VERSION`, `FILE` and
the slot, and register it with `Registry::register`. Do not build the path by
hand and do not write the file with `std::fs`.

## Traps

- A newer file is not overwritten. `read` reports `NewerVersion`; `write`
  refuses it. Adding an optional payload field needs no version bump; bump
  only when an older binary cannot read the new payload, and add a `migrate`.
- A new enum variant needs a version bump or an `#[serde(other)]` arm; without
  one, an older binary reads the new file as a malformed payload.
- `write` does not fsync. `write_durable` does, for the kinds whose loss
  matters.
- `read` never compares `diff_hash`. Staleness is the caller's job; agent output
  is checked at ingest.

The store locks with `std::fs::File::lock` / `try_lock`. `<scope>/.lock` guards
writes and `<scope>/.lease` is the workflow lease; both are on the registry
ignore list. A dead holder's OS lock is released, so the next acquire succeeds;
the `.lock`/`.lease` file and its holder JSON stay behind. The store root is
local app data; NFS and SMB give weaker flock guarantees and are not supported.

Windows uses `LockFileEx` through the same std calls. No CI job compiles or runs
the Windows lock code, and the Windows dead-holder test is unverified. A failed
acquirer on Windows may not be able to read the holder JSON from the locked
file; the error still names the bucket.

`ARTIFACT_LOCK` in `crates/er-engine/src/ai/prepared_diff.rs` is separate: it
serialises `diff-tmp` and `diff-annotated` inside one process. They are agent
inputs, not store artifacts.

The lock binds er store writers. Agents that rewrite artifacts in place and the
er-import skill do not take it. The guard orders writers, not readers: a reader
can see part of a multi-file commit.
