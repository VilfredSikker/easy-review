import type { CheckSummary } from "$lib/types";

export interface CheckCounts {
  pass: number;
  fail: number;
  pending: number;
  total: number;
}

/**
 * Tally CI checks. `gh pr checks` reports conclusions as `SUCCESS`/`FAILURE`
 * or as its `pass`/`fail` buckets depending on the source, so both spellings
 * count. The card's check row and the merge box headline share this, so the
 * two never disagree on how many checks fail.
 */
export function countChecks(checks: CheckSummary[]): CheckCounts {
  let pass = 0;
  let fail = 0;
  let pending = 0;
  for (const c of checks) {
    if (c.status === "PENDING") pending += 1;
    else if (c.conclusion === "SUCCESS" || c.conclusion === "pass") pass += 1;
    else if (c.conclusion === "FAILURE" || c.conclusion === "fail") fail += 1;
  }
  return { pass, fail, pending, total: checks.length };
}
