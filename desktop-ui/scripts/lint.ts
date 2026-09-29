/**
 * Lint gate: ESLint plus two ratcheted baselines (docs/adr/0038-lint-is-a-ratcheted-gate.md).
 *
 * - Errors: pre-existing violations live in `eslint-suppressions.json` (ESLint's
 *   own bulk-suppressions format, so `eslint --suppress-rule` and
 *   `eslint --prune-suppressions` keep working). New errors fail.
 * - Warnings: per-file, per-rule counts live in `eslint-warning-budget.json`.
 *   A file may not gain warnings of a rule it is budgeted for, and a file with
 *   no budget may not have any.
 *
 * Both baselines only shrink. When a count drops below its baseline the check
 * fails until the baseline is pruned, so a fix can't leave slack that the next
 * change silently spends.
 *
 * Usage:
 *   bun run lint                  # check (CI gate)
 *   bun run lint:prune            # shrink both baselines to the current counts
 *   bun run lint -- --accept-warnings
 *                                 # rewrite the warning budget to the current
 *                                 # counts, growth included (the diff is reviewed)
 *   bun run lint -- --no-cache    # skip .tmp/.eslintcache
 *
 * Exit codes: 0 = clean; 1 = new violations or a stale baseline.
 */
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { relative, resolve } from "node:path";
import { ESLint } from "eslint";

export type Baseline = Record<string, Record<string, { count: number }>>;

type Severity = 1 | 2;

interface Violation {
  file: string;
  rule: string;
  line: number;
  column: number;
  message: string;
}

export interface RatchetResult {
  /** Violations in (file, rule) pairs that exceed their baseline. */
  over: { file: string; rule: string; count: number; allowed: number }[];
  /** Baseline entries whose current count is lower: the baseline must be pruned. */
  stale: { file: string; rule: string; count: number; allowed: number }[];
  /** The baseline shrunk to current counts (never grown). */
  pruned: Baseline;
  /** The current counts, growth included. */
  current: Baseline;
}

/** Count violations per (file, rule). */
export function countByFileRule(violations: readonly Pick<Violation, "file" | "rule">[]): Baseline {
  const out: Baseline = {};
  for (const { file, rule } of violations) {
    const perFile = (out[file] ??= {});
    perFile[rule] = { count: (perFile[rule]?.count ?? 0) + 1 };
  }
  return sortBaseline(out);
}

/** Compare current counts against a baseline. Pure: no IO. */
export function ratchet(current: Baseline, baseline: Baseline): RatchetResult {
  const over: RatchetResult["over"] = [];
  const stale: RatchetResult["stale"] = [];
  const pruned: Baseline = {};

  const files = new Set([...Object.keys(current), ...Object.keys(baseline)]);
  for (const file of files) {
    const rules = new Set([
      ...Object.keys(current[file] ?? {}),
      ...Object.keys(baseline[file] ?? {}),
    ]);
    for (const rule of rules) {
      const count = current[file]?.[rule]?.count ?? 0;
      const allowed = baseline[file]?.[rule]?.count ?? 0;
      if (count > allowed) over.push({ file, rule, count, allowed });
      if (count < allowed) stale.push({ file, rule, count, allowed });
      const keep = Math.min(count, allowed);
      if (keep > 0) (pruned[file] ??= {})[rule] = { count: keep };
    }
  }
  return { over, stale, pruned: sortBaseline(pruned), current };
}

function sortBaseline(b: Baseline): Baseline {
  const out: Baseline = {};
  for (const file of Object.keys(b).sort()) {
    out[file] = {};
    for (const rule of Object.keys(b[file]).sort()) out[file][rule] = b[file][rule];
  }
  return out;
}

function readBaseline(path: string): Baseline {
  return existsSync(path) ? (JSON.parse(readFileSync(path, "utf8")) as Baseline) : {};
}

function writeBaseline(path: string, b: Baseline): void {
  writeFileSync(path, `${JSON.stringify(b, null, "\t")}\n`);
}

function collect(results: ESLint.LintResult[], cwd: string, severity: Severity): Violation[] {
  const out: Violation[] = [];
  for (const r of results) {
    const file = relative(cwd, r.filePath).split("\\").join("/");
    for (const m of r.messages) {
      if (m.severity !== severity) continue;
      out.push({
        file,
        rule: m.ruleId ?? "(fatal)",
        line: m.line,
        column: m.column,
        message: m.message,
      });
    }
  }
  return out;
}

function describe(kind: string, entries: RatchetResult["stale"]): string[] {
  return entries.map((e) => `  ${e.file}  ${e.rule}  ${e.count} (baseline ${e.allowed}) ${kind}`);
}

async function main(argv: string[]): Promise<number> {
  const cwd = process.cwd();
  const suppressionsPath = resolve(cwd, "eslint-suppressions.json");
  const budgetPath = resolve(cwd, "eslint-warning-budget.json");
  const prune = argv.includes("--prune");
  const acceptWarnings = argv.includes("--accept-warnings");

  const eslint = new ESLint({
    cwd,
    cache: !argv.includes("--no-cache"),
    cacheLocation: ".tmp/.eslintcache",
    cacheStrategy: "content",
  });
  const results = await eslint.lintFiles(["."]);

  const errors = collect(results, cwd, 2);
  const warnings = collect(results, cwd, 1);
  const errorCheck = ratchet(countByFileRule(errors), readBaseline(suppressionsPath));
  const warningCheck = ratchet(countByFileRule(warnings), readBaseline(budgetPath));

  if (prune) writeBaseline(suppressionsPath, errorCheck.pruned);
  if (acceptWarnings) writeBaseline(budgetPath, warningCheck.current);
  else if (prune) writeBaseline(budgetPath, warningCheck.pruned);

  // Print the violations that fail: every message of an over-budget
  // (file, rule) pair, since a count can't say which one is new.
  const failing = new Set([
    ...errorCheck.over.map((o) => `2|${o.file}|${o.rule}`),
    ...(acceptWarnings ? [] : warningCheck.over.map((o) => `1|${o.file}|${o.rule}`)),
  ]);
  const shown = results
    .map((r) => {
      const file = relative(cwd, r.filePath).split("\\").join("/");
      const messages = r.messages.filter((m) =>
        failing.has(`${m.severity}|${file}|${m.ruleId ?? "(fatal)"}`),
      );
      return {
        ...r,
        messages,
        errorCount: messages.filter((m) => m.severity === 2).length,
        warningCount: messages.filter((m) => m.severity === 1).length,
        fixableErrorCount: messages.filter((m) => m.severity === 2 && m.fix).length,
        fixableWarningCount: messages.filter((m) => m.severity === 1 && m.fix).length,
        suppressedMessages: [],
      };
    })
    .filter((r) => r.messages.length > 0);
  const formatter = await eslint.loadFormatter("stylish");
  const printed = await formatter.format(shown);
  if (printed) console.log(printed);

  const problems: string[] = [];
  if (errorCheck.over.length) {
    problems.push(
      "New lint errors (above). Fix them, or disable one inline with a reason:",
      "  // eslint-disable-next-line <rule> -- <why>",
    );
  }
  if (warningCheck.over.length && !acceptWarnings) {
    problems.push(
      "Files gained maintainability warnings (above). Simplify, or if the growth is",
      "justified run `bun run lint -- --accept-warnings` and commit eslint-warning-budget.json.",
    );
  }
  if (!prune) {
    const stale = [
      ...describe("error", errorCheck.stale),
      ...(acceptWarnings ? [] : describe("warning", warningCheck.stale)),
    ];
    if (stale.length) {
      problems.push(
        "Lint baseline is stale: these counts went down. Run `bun run lint:prune` and commit the result.",
        ...stale,
      );
    }
  }

  const suppressed = errors.length - errorCheck.over.reduce((n, o) => n + o.count, 0);
  console.log(
    `eslint: ${results.length} files, ${suppressed} baselined errors, ${warnings.length} budgeted warnings.`,
  );
  if (problems.length) {
    console.error(`\n${problems.join("\n")}`);
    return 1;
  }
  return 0;
}

if (import.meta.main) {
  process.exit(await main(process.argv.slice(2)));
}
