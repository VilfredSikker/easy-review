import type { PrRow } from "$lib/protocol/generated/PrRow";
import type { Prs } from "$lib/protocol/generated/Prs";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { PrInfo, ProjectSnapshot, TabSummary } from "$lib/pageTypes";
import { JOBS } from "./annotationsAdapter";
import type { ResourceStore } from "./resourceStore";

/** Keyed by the exact `Project.remote` string the PR cache keys on, never normalised. */
export function prsKey(remote: string): ResourceKey {
  return { kind: "prs", remote };
}

/** Whether a refresh of the sidebar's PR lists runs, as `jobs` holds it. */
export function prListRefreshing(store: Pick<ResourceStore, "get">): boolean {
  const held = store.get(JOBS)?.value;
  return held?.kind === "jobs" && held.pr_list_refresh;
}

/** After this long the sidebar marks a remote's PR list stale (`snapshot.rs`, 10 min). */
export const PR_CACHE_TTL_MS = 10 * 60 * 1000;

export function prInfo(row: PrRow): PrInfo {
  return {
    number: row.number,
    title: row.title,
    head_ref: row.headRef,
    state: row.state as PrInfo["state"],
    is_draft: row.isDraft,
    author: row.author,
    assignees: row.assignees,
    reviewers: row.reviewers,
    checks_state: row.checksState as PrInfo["checks_state"],
    review_decision: row.reviewDecision as PrInfo["review_decision"],
    merged_at: row.mergedAt,
    approved_by_me: row.approvedByMe,
    base_ref: row.baseRef,
    head_oid: row.headOid,
    updated_at: row.updatedAt,
  };
}

/**
 * How old a remote's PR list is, as the snapshot computed it: a project with
 * a remote and no fetch time yet is stale; one with no remote, or known only
 * by its remote, is neither stale nor aged. The age is derived here and not
 * carried in the value, so the resource's version never moves with time.
 */
export function prCacheAge(
  project: Pick<ProjectSnapshot, "remote" | "remote_only">,
  fetchedAtMs: number | null,
  nowMs: number,
): { pr_cache_stale: boolean; pr_cache_age_ms: number | null } {
  if (project.remote_only || !project.remote) return { pr_cache_stale: false, pr_cache_age_ms: null };
  if (fetchedAtMs === null) return { pr_cache_stale: true, pr_cache_age_ms: null };
  const age = Math.max(0, nowMs - fetchedAtMs);
  return { pr_cache_stale: age > PR_CACHE_TTL_MS, pr_cache_age_ms: age };
}

export type ProjectPrFields = Pick<
  ProjectSnapshot,
  | "saved_prs"
  | "my_prs"
  | "prs_to_review"
  | "recent_prs"
  | "recently_merged"
  | "dismissed_prs"
  | "pr_cache_stale"
  | "pr_cache_age_ms"
>;

/**
 * A project's PR lists from its remote's `prs` value, each id resolved to
 * its row. Empty until that value arrives; the server already left the
 * dismissed numbers out of mine, to review and recently merged.
 */
export function projectPrFields(
  project: Pick<ProjectSnapshot, "id" | "remote" | "remote_only">,
  prs: Prs | undefined,
  nowMs: number,
): ProjectPrFields {
  const lists = prs?.projects.find((entry) => entry.project === project.id);
  const byNumber = new Map((prs?.rows ?? []).map((row) => [row.number, row]));
  const rows = (ids: readonly number[] | undefined): PrInfo[] =>
    (ids ?? []).flatMap((n) => {
      const row = byNumber.get(n);
      return row ? [prInfo(row)] : [];
    });
  const age = prs
    ? prCacheAge(project, prs.fetchedAtMs, nowMs)
    : { pr_cache_stale: false, pr_cache_age_ms: null };
  return {
    saved_prs: rows(lists?.saved),
    my_prs: rows(lists?.mine),
    prs_to_review: rows(lists?.toReview),
    recent_prs: rows(lists?.recent),
    recently_merged: rows(lists?.recentlyMerged),
    dismissed_prs: [...(lists?.dismissed ?? [])],
    ...age,
  };
}

/**
 * The PR the active tab's branch has, as the snapshot's `detected_pr_number`
 * found it in the PR cache: the tab's own PR; none on a remote PR tab;
 * otherwise the first open row whose head is the viewed branch, else the
 * first row of any state, dismissed ones included. A title-only row has no
 * head and never matches.
 */
export function detectPrNumber(
  tab: Pick<TabSummary, "kind" | "pr_number"> | undefined,
  branch: string | null | undefined,
  remotes: Iterable<Prs>,
): number | null {
  if (tab?.pr_number != null) return tab.pr_number;
  if (tab?.kind === "remote_pr" || !branch) return null;
  let first: number | null = null;
  for (const prs of remotes) {
    for (const row of prs.rows) {
      if (row.headRef !== branch) continue;
      if (row.state === "OPEN") return row.number;
      first ??= row.number;
    }
  }
  return first;
}
