import type { PrInfo } from "./types";

export type StackedPrRow = { pr: PrInfo; depth: number };

/** Trunk names a stack is based on. A PR *from* one (a main → production
 * release PR) is not a stack layer, or every main-based PR would nest under it. */
const TRUNK_BRANCHES = new Set(["main", "master", "develop", "dev"]);

/**
 * Order PRs so stacked layers sit together: a PR whose base branch is another
 * listed PR's head is that PR's child, rendered right after it one level
 * deeper. Built from the refs already in the PR list, so it costs no `gh`
 * call. Unstacked PRs and stack roots keep their original relative order.
 */
export function orderPrsByStack(prs: PrInfo[]): StackedPrRow[] {
  const byHead = new Map<string, PrInfo>();
  for (const pr of prs) {
    if (pr.head_ref && !TRUNK_BRANCHES.has(pr.head_ref) && !byHead.has(pr.head_ref)) {
      byHead.set(pr.head_ref, pr);
    }
  }
  const parentOf = (pr: PrInfo): PrInfo | undefined => {
    const parent = pr.base_ref ? byHead.get(pr.base_ref) : undefined;
    return parent && parent.number !== pr.number ? parent : undefined;
  };
  const children = new Map<number, PrInfo[]>();
  for (const pr of prs) {
    const parent = parentOf(pr);
    if (!parent) continue;
    const list = children.get(parent.number) ?? [];
    list.push(pr);
    children.set(parent.number, list);
  }

  const rows: StackedPrRow[] = [];
  const seen = new Set<number>();
  const visit = (pr: PrInfo, depth: number) => {
    if (seen.has(pr.number)) return;
    seen.add(pr.number);
    rows.push({ pr, depth });
    for (const child of children.get(pr.number) ?? []) visit(child, depth + 1);
  };
  for (const pr of prs) {
    if (!parentOf(pr)) visit(pr, 0);
  }
  // A base/head cycle has no root; list its members flat rather than drop them.
  for (const pr of prs) visit(pr, 0);
  return rows;
}
