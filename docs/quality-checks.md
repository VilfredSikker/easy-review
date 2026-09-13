# Quality Checks

Dev-process tooling that keeps this codebase's own risk low: the **CRAP
metric** (`crates/er-crap`), the **negative-test convention**, and **mutation
testing**. None of them grades the codebase in CI — see
[ADR 0028](./adr/0028-crap-gate-is-local.md).

## CRAP metric

CRAP combines a function's cyclomatic complexity with its unit-test coverage
into one risk score (Savoia & Evans 2007; popularized by NDepend — see
[CRAP Metric Is a Thing And It Tells You About Risk in Your Code](https://blog.ndepend.com/crap-metric-thing-tells-risk-code/)).

```text
CRAP(m) = CC(m)² × (1 − cov(m)/100)³ + CC(m)
```

- `CC(m)` — cyclomatic complexity (paths through the function).
- `cov(m)` — test coverage in percent.
- Scores **above 30** (the default threshold) mark a function as risky to
  change; 30 exactly still passes. The minimum score is 1.0.
- At 100% coverage CRAP collapses to CC — the quadratic term vanishes, so
  complexity alone still counts. Above CC = 30 no amount of coverage brings a
  function under the threshold: it is too complex to certify clean.

Golden values, pinned in `crates/er-crap`'s unit tests. The blog's "CC=6 → 37"
worked example is a typo; the formula yields 42.

| CC | Coverage | CRAP  | Crappy? |
|----|----------|-------|---------|
| 1  | 100%     | 1.0   | no  |
| 6  | 0%       | 42.0  | yes |
| 10 | 0%       | 110.0 | yes |
| 10 | 42%      | ≈29.5 | no  |
| 10 | 41%      | ≈30.5 | yes |
| 25 | 80%      | 30.0  | no (boundary) |
| 25 | 79%      | ≈30.8 | yes |
| 30 | 100%     | 30.0  | no (the ceiling) |
| 31 | 100%     | 31.0  | yes (no coverage level fixes it) |

The analyzer is a `syn` AST walker, McCabe-classic and documented in
`crates/er-crap/src/complexity.rs`. Three of its rules are not guessable from
the metric: `match` arms count, including `_`; a closure's decisions count
toward the enclosing function, while a nested `fn` item gets its own entry and
leaks nothing outward; `?`, `async`, and `unsafe` add nothing.

### Running it

```bash
just crap          # coverage (er-engine + er-tui) + gate: exits 1 on CRAPpy functions
just crap-report   # same run, never fails
just crap-test     # the tool's own tests, incl. the negative gate fixtures
```

`--path` is scoped to the two crates `llvm-cov` measured. That scoping carries
the result: a function with no coverage data scores **0%** (pessimistic,
matching cargo-crap's default), so an unmeasured crate floods the report with
fake risk. Test, bench and example code is excluded from the walk for the same
reason.

The failing gate is `just crap` locally: the baseline is not under the threshold
yet. CI's `crap` job runs `cargo test -p er-crap` and publishes a `--summary`
report that never fails on the threshold — see
[ADR 0028](./adr/0028-crap-gate-is-local.md).

## Negative tests

Tests that assert **failure behaviour**, so the suite proves things break when
they should:

- **Gate fixtures** — known-bad input must fail the gate (exit 1), clean input
  must pass (exit 0). This is the canonical shape for this tooling;
  `crates/er-crap/tests/` holds both, each a `.rs` + `.lcov` pair.
- **Rust error paths** — `Result`-returning tests with
  `assert!(matches!(res, Err(..)))` or `assert!(run(&opts).is_err())`;
  `#[should_panic]` where the failure is a panic.
- **TS** — `expect(...).rejects.toThrow()` under `bun test` (desktop-ui imports
  from `"bun:test"`, not vitest).

Keep fixtures hermetic: hand-written `.lcov` files whose line numbers the tests
assert exact coverage percents against, so editing a fixture without updating
its `.lcov` fails loudly. That is the point.

## Mutation testing

Mutation testing injects small bugs ("mutants") into the code and re-runs the
suite: a surviving mutant means a weak test. **Mutation score** = fraction of
mutants killed.

```bash
cargo binstall cargo-mutants   # once
just mutants                   # mutate er-engine; HTML report in mutants.out/
just mutants-stale [days]      # age of the last run; exits 1 when stale (default 30)
```

**Deliberately not scheduled** — a run recompiles the engine many times, so it
is expensive and stays on demand. `just mutants` stamps the run date into
`quality/mutants-last-run.txt`; `just mutants-stale` reports the age and exits 1
once the log is stale, or has never been stamped. That is the cue to run it
again. The score is informational; ratchet a minimum once a baseline is known
(target ≥ 50%).

Only exits **0** (every mutant caught) and **2** (mutants survived) stamp the
log. The recipe earns that distinction by testing the status inside an `if`,
which `set -e` exempts: cargo-mutants exits 2 when mutants survive — the usual
outcome, and the reason a baseline is wanted — while the justfile runs every
recipe line under `bash -eu`, so a bare invocation would abort before the stamp
ever ran. Anything else (a compile failure, a missing install) leaves the log
alone rather than recording a run that never completed. Survivors still fail the
recipe, so a freshly stamped log does not mean the run came back clean.

## Alternatives

[cargo-crap](https://github.com/minikin/cargo-crap) is a mature external CRAP
tool for Rust (baselines, SARIF, GitHub annotations). `crates/er-crap` is
in-repo so every layer is unit-testable and CI needs no third-party binary;
cargo-crap stays an option if richer reporting is wanted later.
