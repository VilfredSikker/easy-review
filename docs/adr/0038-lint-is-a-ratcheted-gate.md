# Lint is a ratcheted gate, in two tiers

Lint had to become a gate without a big-bang cleanup. A global `--max-warnings N` lets one file's cleanup pay for another file's mess, and it never tightens. So both halves of the repo count violations per file and per rule against a committed baseline. A count may not rise, and a count that falls fails the gate until the baseline is pruned, so a fix cannot leave slack for the next change to spend.

Two tiers, on both sides:

- **error:** correctness, idiom, and this repo's written conventions. New code must be clean. Disable a single site inline with a reason.
- **warn:** maintainability signals (nesting, parameter count, unwrap/non-null assertions, unrelated groups sharing a file). These are budgeted: a file may not gain one unless the growth is accepted in a reviewed diff.

## The two halves differ where their tools differ

- **desktop-ui (ESLint).** Pre-existing errors live in `eslint-suppressions.json`, ESLint's own bulk-suppression format, so plain `eslint .` and editors agree with the gate. Warnings live in `eslint-warning-budget.json`, because bulk suppression covers errors only.
- **Rust (clippy).** Clippy has no bulk suppression. The error tier therefore landed at zero: it was fixed, or given a site-scoped `#[expect(lint, reason = …)]`. A real problem with many sites (`unwrap_used`, `string_slice`, allows without a reason) is budgeted in the warn tier instead. `er-lint-gate` replaces `-D warnings`: every diagnostic fails except the lints listed under `[workspace.metadata.lint-gate]`, which are ratcheted against `clippy-warning-budget.json`.

## Choices that would surprise a reader

- **Rules were chosen by probing this codebase.** Each candidate ran over the tree and was judged on its hits. A rule with no hits is on as a free guard. A rule whose hits were an idiom it misreads is off, with the reason next to it in the config. The Svelte 5 dependency read (`x;` inside `$effect`), `onclick={() => (x = y)}`, and `new Promise((r) => requestAnimationFrame(r))` are why `@typescript-eslint/no-unused-expressions`, `no-return-assign` and `no-promise-executor-return` are off. Clippy's pedantic group is on as a group, and its many-hit idiom lints are allowed one by one.
- **There is no line-count cap on either side.** A length cap makes people and agents split a coherent file just to get under it. The frontend uses two cohesion rules instead, which flag a module or class whose code falls into groups that never reference each other. Calibrated on this tree, `module-cohesion` at more than two groups of 60+ lines flags only `LeftSidebar.svelte`. At one group it also flagged three pure modules whose second group was a legitimate sibling. `class-cohesion` flags no store class at any threshold tried. On the Rust side, `er-lint-gate` runs the same LCOM4 measure over inherent `impl` blocks. At more than one group of 40+ lines it flags one impl, where file navigation and tour navigation share `TabState` without touching each other. It also flagged the MCP server's `#[tool_router]` impl until impls under an attribute macro were skipped: like a trait impl, that impl is a set of independent handlers by design. Module-level cohesion is not built for Rust. It needs name resolution across `use` renames and macros, which a `syn` walk cannot do reliably.
- **A key-listener convention stays unenforced.** A rule against global `keydown` listeners outside `keyboard.ts` flagged modal-local Escape handlers far more often than real shortcuts, so it was dropped.
- **A snake_case key passed to `app.cmd` or `invoke` is an error.** Tauri binds JS args in camelCase, and a snake_case key silently arrives as `None`. The rule found a live case on its first run: custom diagrams were always rejected as missing their prompt.

## Consequences

- **Stale enforcement is local-only for Rust.** The budget is counted on macOS, and platform-gated code reads differently on CI's Linux runner. CI therefore passes `--no-stale`. A change that fixes a budgeted site without pruning passes CI, and fails the next local `just clippy`. Linux-only code with a violation fails CI outright, because no macOS run can accept it into the budget.
- **An incomplete clippy run cannot write the budget.** Pedantic is at deny, so one error stops dependent crates from being linted. Their counts would read zero. The gate refuses `--prune` and `--accept-warnings` until every crate is linted.
- **The toolchain is pinned, in `rust-toolchain.toml` and every `dtolnay/rust-toolchain` step.** Pedantic is at deny and the budget holds exact counts, so an unpinned stable would turn CI red whenever a release adds or changes a lint. The cost is that upgrades are manual: bump both, run `just clippy`, then fix or accept what the new clippy reports.
- **CI still runs neither `svelte-check` nor the frontend unit tests.** The desktop-ui job runs the lint gate and the gate's own tests only.
