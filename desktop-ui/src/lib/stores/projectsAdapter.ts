import type { ResourceClient } from "$lib/protocol/client";
import type { Branch } from "$lib/protocol/generated/Branch";
import type { Branches } from "$lib/protocol/generated/Branches";
import type { Command } from "$lib/protocol/generated/Command";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { Project } from "$lib/protocol/generated/Project";
import type { ProjectReviewSettingsPatch } from "$lib/protocol/generated/ProjectReviewSettingsPatch";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { AppSnapshot, BranchInfo, ProjectSnapshot, TabSummary } from "$lib/types";
import { resourceKeyId, type ResourceStore } from "./resourceStore";

export const PROJECTS_KEY: ResourceKey = { kind: "projects" };

export function branchesKey(project: string): ResourceKey {
  return { kind: "branches", project };
}

/** A GitHub remote as a lower-case `owner/repo`, whichever form it was written in. */
export function remoteKey(remote: string | null | undefined): string | null {
  const trimmed = remote?.trim().replace(/\.git$/i, "").replace(/\/+$/, "");
  if (!trimmed) return null;
  const url = /github\.com[:/]+([^/]+)\/([^/]+)$/i.exec(trimmed);
  return (url ? `${url[1]}/${url[2]}` : trimmed).toLowerCase();
}

/**
 * Whether the active tab shows `project`: a local project by its checkout
 * root, one known only by its remote by that remote.
 */
export function isActiveProject(project: Project, active: TabSummary | null): boolean {
  if (!active) return false;
  if (!project.remoteOnly) return project.rootPath === active.repo_root;
  const remote = remoteKey(active.remote);
  return remote !== null && remote === remoteKey(project.remote);
}

/**
 * The branch rows with `is_current` derived: the branch the active tab views
 * for the project it shows, and the checkout's branch for every other one.
 */
export function branchRows(
  branches: Branches,
  active: TabSummary | null,
  isActive: boolean,
): { local: BranchInfo[]; auto: BranchInfo[] } {
  const current = (isActive ? active?.branch : null) ?? branches.currentBranch;
  const row = (branch: Branch): BranchInfo => ({
    name: branch.name,
    upstream: branch.upstream,
    is_current: branch.name === current,
    is_merged: branch.isMerged,
    worktree_path: branch.worktreePath,
    pr_number: branch.prNumber,
  });
  return { local: branches.localBranches.map(row), auto: branches.autoBranches.map(row) };
}

/**
 * The resource's rows in its order. The PR lists still come from the
 * snapshot's row with the same id until they get their own resource, and a
 * project's branches from that row until its `branches` resource arrives.
 */
export function projectsFromResource(
  wire: readonly Project[],
  branches: (project: string) => Branches | undefined,
  rows: readonly ProjectSnapshot[],
  active: TabSummary | null,
): ProjectSnapshot[] {
  const byId = new Map(rows.map((row) => [row.id, row]));
  return wire.map((project) => {
    const row = byId.get(project.id);
    const isActive = isActiveProject(project, active);
    const held = branches(project.id);
    const { local, auto } = held
      ? branchRows(held, active, isActive)
      : { local: row?.local_branches ?? [], auto: row?.auto_branches ?? [] };
    return {
      id: project.id,
      name: project.name,
      root_path: project.rootPath,
      remote: project.remote,
      remote_only: project.remoteOnly,
      is_active: isActive,
      local_branches: local,
      auto_branches: auto,
      saved_prs: row?.saved_prs ?? [],
      my_prs: row?.my_prs ?? [],
      prs_to_review: row?.prs_to_review ?? [],
      recent_prs: row?.recent_prs ?? [],
      recently_merged: row?.recently_merged ?? [],
      pr_cache_stale: row?.pr_cache_stale,
      pr_cache_age_ms: row?.pr_cache_age_ms,
      auto_triage_max_diff_kb: project.autoTriageMaxDiffKb,
      review_ignore_globs: project.reviewIgnoreGlobs,
      dismissed_prs: row?.dismissed_prs ?? [],
    };
  });
}

/** How long a write waits for the resources its `Ack` names to be fetched. */
const ACK_FETCH_WAIT_MS = 2_000;

interface ProjectsAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show" | "send">;
  client: Pick<ResourceClient, "fetch">;
  snapshot: () => Pick<AppSnapshot, "projects"> | null;
  /** The active row of the tab strip. */
  activeTab: () => TabSummary | null;
  /** The old command path, which answers a snapshot; used with the flag off. */
  command: (name: string, args: Record<string, unknown>) => Promise<void>;
  /** A plain command call, for the old path's one command that answers a list. */
  invoke: <T>(name: string, args: Record<string, unknown>) => Promise<T>;
  report: (command: string, e: ErError) => void;
  sleep?: (ms: number) => Promise<void>;
}

/**
 * The sidebar's projects, read from `app.snapshot` with the flag off and
 * from the `projects` and `branches` resources with it on, behind one read
 * API; and the project commands, sent through the old commands or `er_send`.
 */
export class ProjectsAdapter {
  private readonly branchHolds = new Map<string, () => void>();

  constructor(private readonly deps: ProjectsAdapterDeps) {
    if (deps.flagOn) deps.store.show(PROJECTS_KEY);
  }

  get projects(): ProjectSnapshot[] {
    const rows = this.deps.snapshot()?.projects ?? [];
    if (!this.deps.flagOn) return rows;
    const held = this.deps.store.get(PROJECTS_KEY)?.value;
    if (held?.kind !== "projects") return rows;
    const branches = (project: string) => {
      const value = this.deps.store.get(branchesKey(project))?.value;
      return value?.kind === "branches" ? value : undefined;
    };
    return projectsFromResource(held.projects, branches, rows, this.deps.activeTab());
  }

  /** The ids the `projects` resource lists; empty until it arrives or with the flag off. */
  get projectIds(): string[] {
    const held = this.deps.store.get(PROJECTS_KEY)?.value;
    return held?.kind === "projects" ? held.projects.map((p) => p.id) : [];
  }

  /** Shows the branches of exactly these projects, releasing the rest. */
  showBranches(ids: readonly string[]): void {
    if (!this.deps.flagOn) return;
    const wanted = new Set(ids);
    for (const [id, release] of this.branchHolds) {
      if (wanted.has(id)) continue;
      release();
      this.branchHolds.delete(id);
    }
    for (const id of wanted) {
      if (!this.branchHolds.has(id)) this.branchHolds.set(id, this.deps.store.show(branchesKey(id)));
    }
  }

  reorder(orderedIds: string[]): Promise<void> {
    return this.write("reorder_projects", { orderedIds }, { kind: "reorder_projects", ordered: orderedIds });
  }

  deleteProject(projectId: string): Promise<void> {
    return this.write("delete_project", { projectId }, { kind: "delete_project", project: projectId });
  }

  addTrackedBranch(projectId: string, name: string): Promise<void> {
    return this.write(
      "add_tracked_branch",
      { projectId, name },
      { kind: "add_tracked_branch", project: projectId, name },
    );
  }

  removeTrackedBranch(projectId: string, name: string): Promise<void> {
    return this.write(
      "remove_tracked_branch",
      { projectId, name },
      { kind: "remove_tracked_branch", project: projectId, name },
    );
  }

  patchReviewSettings(projectId: string, patch: ProjectReviewSettingsPatch): Promise<void> {
    return this.write(
      "patch_project_review_settings",
      { projectId, patch },
      { kind: "patch_project_review_settings", project: projectId, patch },
    );
  }

  /** Local branches neither checked out nor tracked; read once, never shown. */
  async availableBranches(projectId: string): Promise<string[]> {
    if (!this.deps.flagOn) {
      return this.deps.invoke<string[]>("list_available_branches", { projectId });
    }
    const [entry] = await this.deps.client.fetch([{ kind: "available_branches", project: projectId }]);
    return entry?.value.kind === "available_branches" ? entry.value.branches : [];
  }

  /**
   * Flag off, the old command, which repaints from the snapshot it answers.
   * Flag on, `er_send`; it resolves once each resource the `Ack` names is
   * held at that version, so the caller's pending paint gives way to the
   * new rows and not to the old ones.
   */
  private async write(name: string, args: Record<string, unknown>, command: Command): Promise<void> {
    if (!this.deps.flagOn) return this.deps.command(name, args);
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      this.deps.report(name, e as ErError);
      return;
    }
    const sleep = this.deps.sleep ?? ((ms: number) => new Promise<void>((r) => setTimeout(r, ms)));
    const due = ack.changed.filter(({ key }) => this.isShown(key));
    const deadline = Date.now() + ACK_FETCH_WAIT_MS;
    while (due.some(({ key, version }) => (this.deps.store.get(key)?.version ?? -1) < version)) {
      if (Date.now() > deadline) return;
      await sleep(16);
    }
  }

  private isShown(key: ResourceKey): boolean {
    if (resourceKeyId(key) === resourceKeyId(PROJECTS_KEY)) return true;
    return key.kind === "branches" && this.branchHolds.has(key.project);
  }
}
