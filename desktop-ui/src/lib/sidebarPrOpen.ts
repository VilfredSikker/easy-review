import { remoteParts, type TabOpenRequest } from "./prOpen";
import { longAge } from "./relativeTime";
import type { PrInfo, ProjectSnapshot, TabSummary } from "./types";

/** The pull-request glyph the sidebar rows draw, as one path for the modal. */
const PR_ICON_PATH = "M6 3v12M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM18 9a9 9 0 0 1-9 9";

/**
 * The tab already reviewing this PR, if any. Asking "new tab or here?" about a
 * PR that is open is a question with no good answer, so the sidebar switches
 * to it instead. A remote PR tab views a repo other than the clone it sits in,
 * so the remote decides whenever both sides know one.
 */
export function findPrTab(
  project: Pick<ProjectSnapshot, "root_path" | "remote">,
  prNumber: number,
  tabs: readonly TabSummary[],
): TabSummary | null {
  const parts = remoteParts(project);
  const slug = parts ? `${parts.owner}/${parts.repo}`.toLowerCase() : "";
  return (
    tabs.find((t) => {
      if (t.pr_number !== prNumber) return false;
      if (slug && t.remote) return t.remote.toLowerCase() === slug;
      return t.repo_root === project.root_path;
    }) ?? null
  );
}

/** What the tab-open modal shows for a PR picked in the sidebar. */
export function sidebarPrOpenRequest(
  project: Pick<ProjectSnapshot, "id" | "name">,
  pr: Pick<PrInfo, "number" | "title" | "author" | "updated_at" | "state" | "is_draft">,
  now: number = Date.now(),
): TabOpenRequest {
  let state = "Pull request";
  if (pr.state === "MERGED") state = "Merged pull request";
  else if (pr.is_draft) state = "Draft pull request";
  const age = pr.updated_at ? longAge(pr.updated_at, now) : "";
  const context = [state, pr.author, age ? `updated ${age}` : ""].filter(Boolean).join(" · ");
  return {
    key: `${project.id}:${pr.number}`,
    title: pr.title,
    ref: `${project.name} #${pr.number}`,
    context,
    iconPath: PR_ICON_PATH,
    tone: pr.state === "MERGED" ? "periwinkle" : "add-fg",
    quote: null,
  };
}
