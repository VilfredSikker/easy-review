# Git is a port with a CLI adapter and a test fake

Every git call is a subprocess (ADR 0001), but callers reached git through engine free functions, so tests needed real repositories and the engine and front ends could drift. `er-git` now owns the subprocess code behind a `Git` trait: `CliGit` runs the binary, `FakeGit` answers from a script, and a cloneable `Ports` carries `Arc<dyn Git>` into `App`, `TabState` and their workers. Every constructor takes it, so a missed site fails to compile, and there is no global.

The trait mirrors the spawning functions one to one. A typed API or a narrow first slice would either change every caller at once or leave a second way to reach git after the migration. The wide trait keeps the move mechanical; `er-core` can narrow it in phase 4.

## Considered Options

- A typed API (`RepoRoot`, `DiffSpec`, `Oid`) waits for `er-core`.
- A narrow trait leaves the remaining free functions callable.
- A process-wide `OnceLock<Arc<dyn Git>>` removes constructor churn and rules out per-test fakes.

## Consequences

- `FakeGit` ships in normal builds because the test constructors are production code.
- A new git command is a new trait method, added by the PR that needs it.
- Trait-backed free functions become crate-private once the migration lands, so callers cannot bypass the port.
