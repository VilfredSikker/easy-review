# Recovery writes snapshots instead of caller bytes

## Context

Moving the review-flow sidecars onto the artifact store left one gap: an
acceptance can archive `review.json` and `arbiter.json`, replace them, then
fail and have to put the old bytes back exactly. The typed write would
reserialize the value, which is not byte-for-byte for a file an agent wrote,
and a malformed file must be restorable even though the store would refuse to
parse it. A plain `write_bytes(path, bytes)` closes the gap and opens a hole:
any caller can put arbitrary bytes at a registered kind's path, skipping the
version, envelope and shape rules the store exists to enforce.

## Decision

The store's recovery API takes handles, not caller bytes:
`snapshot()` reads an artifact's raw bytes into an `ArtifactSnapshot` whose
constructor is private to `er-store`, and `restore_if_unchanged()` writes a
snapshot back only when the file still holds the bytes another snapshot
captured. A computed replacement goes through `write_typed_if_unchanged()`,
which serializes a typed value and returns the snapshot it wrote, so a
subsequent rollback compares against those exact bytes. Archive and restore use
`BackupSnapshot`: `archive_to_dir()` moves an artifact into a backup directory
and `restore_backup()` copies those bytes back, never over an existing file.
Backups mirror the artifact's relative path, and a backup directory may not be
reused.

Every operation canonicalizes the scope and rejects a symlink below it before
reading or writing, so a validation read cannot be pointed outside the bucket.
The compare-and-swap operations hold the scope write lock across the read and
the write.

The `er::store_bytes` lint-gate rule fails a public function in `er-store` that
takes a written byte buffer together with a path selector. A byte parameter
named `expected` is the comparison side of a compare-and-swap and does not
count. That rule is what keeps a future `write_bytes` from quietly returning.

## Consequences

**Good.** Recovery can restore bytes the store would call malformed, and no
public API can place arbitrary bytes at an artifact path. The rollback paths
gain the read-then-write lock they lacked.

**Costs.** A snapshot holds the file in memory, so every recovery read is
bounded by the store's size cap. A caller that legitimately needs new bytes at
an artifact path must go through the typed write; if it cannot, that is a new
decision, not a helper.
