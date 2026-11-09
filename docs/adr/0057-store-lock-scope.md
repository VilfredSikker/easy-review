# The store lock covers store writers

Every store write takes an OS advisory lock on `<scope>/.lock`, and a workflow
can hold a lease on `<scope>/.lease` across many writes. The lock binds `er`
store writers only. Agents that rewrite artifacts in place and the er-import
skill do not take it, so it cannot stop them; the tmp-file rename keeps one
write atomic, and the lock exists so two `er` processes cannot interleave a
read-modify-write or a pair of related writes. Readers take no lock: the guard
orders writers, not readers, so a reader can see part of a multi-file commit.

A lease and a write lock are separate files, so a held lease does not block a
store write. The OS releases a dead holder's lock, so a crashed process leaves
the bucket writable and only its holder JSON stays behind. The store root is
local app data; NFS and SMB give weaker `flock` guarantees and are not
supported. Windows uses `LockFileEx` through the same std calls; no CI job runs
it.
