# The complexity gate runs locally, not in CI

The CRAP metric (`crates/er-crap`) derives half its score from coverage, so a threshold only means something once the suite measures the code under it. CI's `crap` job therefore invokes the tool with `--summary` and never passes `--fail-above`: it reports, and every pull request stays green. The failing gate is the local `just crap` recipe, which passes `--fail-above` and fails where the person who can lower the number is already looking at the function.

## Consequences

- Nothing in CI fails on the threshold, so a CRAP regression merges unless someone runs `just crap` locally — which costs a full `cargo llvm-cov` build, so it is a deliberate step, not a reflex. Read the CI report as a trend, not a check.
- The job still fails on `cargo test -p er-crap`, which covers the formula, the complexity analyzer, and the gate's positive and negative fixtures. That is the tool being tested, not the codebase being graded — do not switch it off thinking it is redundant with the report step.
