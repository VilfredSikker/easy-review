import type { AppSnapshot, PrInfo, ProjectSnapshot } from "./types";

function firstNonEmpty(...vals: Array<string | null | undefined>): string {
  for (const v of vals) {
    if (v != null && v.trim() !== "") return v;
  }
  return "";
}

function prsInProject(project: ProjectSnapshot): PrInfo[] {
  return [
    ...(project.saved_prs ?? []),
    ...(project.my_prs ?? []),
    ...(project.prs_to_review ?? []),
    ...(project.recent_prs ?? []),
    ...(project.recently_merged ?? []),
  ];
}

function findPrInProjects(
  projects: ProjectSnapshot[] | undefined,
  prNumber: number,
  remoteSlug?: string | null,
): PrInfo | null {
  if (!projects) return null;
  const slug = remoteSlug?.trim();
  if (slug) {
    const own = projects.find(
      (p) => p.remote?.toLowerCase() === slug.toLowerCase(),
    );
    const hit = own ? prsInProject(own).find((p) => p.number === prNumber) : undefined;
    if (hit) return hit;
  }
  for (const project of projects) {
    const hit = prsInProject(project).find((p) => p.number === prNumber);
    if (hit) return hit;
  }
  return null;
}

export interface ContextTitle {
  /** Repo name for the leading breadcrumb segment; empty when unknown. */
  repo: string;
  prNumber: number | null;
  /** PR title, or the tab label / branch when the view is not a PR. */
  title: string;
}

/** Breadcrumb "repo / #N title" for the top bar. Same fallback order as
 *  `resolveContextIdentity`: remote PR stubs often carry no `snapshot.pr`, so
 *  the title falls through to GitHub status, the sidebar PR lists, then the
 *  tab label and branch, so the bar never renders empty on a loaded tab. */
export function resolveContextTitle(
  snapshot: AppSnapshot | null | undefined,
): ContextTitle {
  if (!snapshot) return { repo: "", prNumber: null, title: "" };

  const tab =
    snapshot.tabs.find((t) => t.is_active) ??
    (typeof snapshot.active_tab === "number"
      ? snapshot.tabs[snapshot.active_tab]
      : undefined);
  const prNumber =
    tab?.pr_number ??
    snapshot.pr?.number ??
    snapshot.detected_pr_number ??
    snapshot.github?.number ??
    null;
  const repo = repoName(snapshot, tab?.remote);

  if (prNumber == null) {
    return { repo, prNumber, title: firstNonEmpty(tab?.label, snapshot.branch) };
  }
  const listed = findPrInProjects(snapshot.projects, prNumber, tab?.remote);
  const title = firstNonEmpty(
    snapshot.pr?.title,
    snapshot.github?.title,
    listed?.title,
    tab?.label,
    snapshot.branch,
  );
  return { repo, prNumber, title };
}

function repoName(
  snapshot: AppSnapshot,
  tabRemote: string | null | undefined,
): string {
  const gh = snapshot.github;
  const slug = firstNonEmpty(tabRemote, gh ? `${gh.owner}/${gh.repo}` : "");
  if (slug) return slug.split("/").pop() ?? "";
  return firstNonEmpty(snapshot.projects?.find((p) => p.is_active)?.name);
}

/** Branch title + base ref for the context bar. Empty snapshot.branch/base
 *  is common on remote PR stubs; fall back to GitHub status, PR card, then
 *  the sidebar PR lists. */
export function resolveContextIdentity(
  snapshot: AppSnapshot | null | undefined,
): { branch: string; base: string } {
  if (!snapshot) return { branch: "", base: "" };

  const tab =
    snapshot.tabs.find((t) => t.is_active) ??
    (typeof snapshot.active_tab === "number"
      ? snapshot.tabs[snapshot.active_tab]
      : undefined);
  const prNumber =
    tab?.pr_number ??
    snapshot.pr?.number ??
    snapshot.detected_pr_number ??
    snapshot.github?.number ??
    null;
  const listed =
    prNumber != null
      ? findPrInProjects(snapshot.projects, prNumber, tab?.remote)
      : null;

  return {
    branch: firstNonEmpty(
      snapshot.branch,
      tab?.branch,
      snapshot.github?.head_ref,
      snapshot.pr?.head,
      listed?.head_ref,
    ),
    base: firstNonEmpty(
      snapshot.base,
      snapshot.github?.base_ref,
      snapshot.pr?.base,
      listed?.base_ref,
    ),
  };
}
