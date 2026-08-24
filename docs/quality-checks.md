# Quality Checks

Easy Review's code-quality tooling: the **CRAP metric** (Change Risk
Anti-Patterns), the **negative-test convention**, and **mutation testing**.
These are dev-process tools — they keep *this* codebase's risk low.

## CRAP metric

CRAP combines a function's cyclomatic complexity with its unit-test coverage
into a single risk score. It was introduced by Savoia & Evans (2007) and made
famous by NDepend — see
[CRAP Metric Is a Thing And It Tells You About Risk in Your Code](https://blog.ndepend.com/crap-metric-thing-tells-risk-code/).

```text
CRAP(m) = CC(m)² × (1 − cov(m)/100)³ + CC(m)
```

- `CC(m)` — cyclomatic complexity (number of paths through the function).
- `cov(m)` — test coverage in percent.
- Scores **above 30** (the default threshold) mark a function as risky to
  change. The minimum score is 1.0.
- At 100% coverage CRAP collapses to CC — the quadratic term vanishes, so
  complexity itself still counts.
- Above CC ≈ 30 no amount of coverage keeps the score under the threshold:
  the function is too complex to certify as clean regardless of tests.

Golden values (also pinned in `crates/er-crap`'s unit tests; note the blog's
"CC=6 → 37" worked example is a typo — the formula yields 42):

| CC | Coverage | CRAP  | Crappy? |
|----|----------|-------|---------|
| 1  | 100%     | 1.0   | no  |
| 6  | 0%       | 42.0  | yes |
| 10 | 0%       | 110.0 | yes |
| 10 | 42%      | ≈29.5 | no  |
| 10 | 41%      | ≈30.5 | yes |
| 25 | 80%      | 30.0  | no (boundary) |
| 25 | 79%      | ≈30.8 | yes |
| 31 | 100%     | 31.0  | yes (unfixable) |

### Cyclomatic complexity definition

`crates/er-crap` scores each function with a `syn` AST walker using a
McCabe-classic definition (documented in `complexity.rs`):

- base 1;
- `+1` per `if` / `else if` / `if let` (each condition, including
  `else if` chains);
- `+1` per `for`, `while` / `while let`, and `loop`;
- `+1` per `match` arm (including `_`);
- `+1` per short-circuit `&&` / `||`;
- `?`, `async`, and `unsafe` blocks add nothing;
- decisions inside closures count toward the enclosing function; a nested
  `fn` item gets its own entry.

### Running it

Prereqs (once): `cargo binstall cargo-llvm-cov` and
`rustup component add llvm-tools-preview`.

```bash
just crap          # coverage (er-engine + er-tui) + gate: exits 1 on CRAPpy functions
just crap-report   # same, but never fails (informational)
just crap-test     # the tool's own tests, incl. negative gate fixtures
```

Under the hood: `cargo llvm-cov -p er-engine -p er-tui --lcov` produces
`lcov.info`, and `cargo run -p er-crap -- --lcov lcov.info` scores every
function in those two crates. Functions with **no coverage data score 0%**
(pessimistic). `tests/`, `benches/`, `examples/`, `target/`, and
`.git/` are never analyzed. The `--path` flag is repeatable — the recipes
scope it to the crates `llvm-cov` measured so untouched crates (desktop,
mcp, tools) don't score pessimistic 0%.

Gate status:

- `just crap` runs the gate locally (`--fail-above`) and **will exit 1
  today** — the coverage-based baseline is not yet under the threshold. It is
  the "make it pass" command for the cleanup effort.
- CI's `crap` job is **report-only** (`--summary`, no `--fail-above`):
  it gates the tooling itself (`cargo test -p er-crap`) and publishes the
  report, but never fails on CRAPpy functions. Flip `--fail-above` on in
  `.github/workflows/ci.yml` once the baseline is under the threshold.

## Negative tests

"A way to add negative tests" = tests that assert **failure behavior**, so a
suite proves things break when they should:

- **Rust error paths** — `Result`-returning tests with
  `assert!(matches!(res, Err(..)))` or `assert!(run(&opts).is_err())`;
  `#[should_panic]` for panics. Example:
  `crates/er-crap/tests/errors.rs` (missing LCOV file, malformed records,
  missing source path).
- **Gate fixtures — "bad input must fail the gate"** — a fixture that is
  known-bad must make the tool exit 1; a clean fixture must exit 0. Example:
  `crates/er-crap/tests/gate_crappy.rs` vs `gate_clean.rs` with the
  `.rs`/`.lcov` fixtures in `crates/er-crap/tests/fixtures/`.
- **TS** — `expect(() => f(badInput)).toThrow()` (desktop-ui uses
  vitest-style imports under `bun test`).

Keep fixtures hermetic: hand-written `.lcov` files with documented line
numbers (the tests assert exact coverage percents, so editing a fixture
without updating its `.lcov` fails loudly — that is the point).

## Mutation testing

Mutation testing injects small bugs ("mutants") into the code and re-runs
the test suite: a surviving mutant means a weak test. **Mutation score** =
fraction of mutants killed.

```bash
cargo binstall cargo-mutants   # once
just mutants                   # mutate er-engine; HTML report in mutants.out/
```

The scheduled CI workflow (`.github/workflows/mutants.yml`, weekly +
manual dispatch) runs `cargo mutants -p er-engine` and uploads the report
artifact. It is informational today; ratchet a minimum mutation score in
once the baseline is known (target ≥ 50%).

## Alternatives

[cargo-crap](https://github.com/minikin/cargo-crap) is a mature external CRAP
tool for Rust (baselines, SARIF, GitHub annotations). We implemented
`crates/er-crap` in-repo so every layer is unit-testable and CI needs no
third-party binary; cargo-crap remains an option if richer reporting is
wanted later.
