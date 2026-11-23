import type { ResourceClient } from "$lib/protocol/client";
import type { Branch } from "$lib/protocol/generated/Branch";
import type { Branches } from "$lib/protocol/generated/Branches";
import type { Command } from "$lib/protocol/generated/Command";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { Project } from "$lib/protocol/generated/Project";
import type { ProjectReviewSettingsPatch } from "$lib/protocol/generated/ProjectReviewSettingsPatch";
import type { Prs } from "$lib/protocol/generated/Prs";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { AppSnapshot, BranchInfo, ProjectSnapshot, TabSummary } from "$lib/pageTypes";
import { projectPrFields, prsKey } from "./prsAdapter";
import { resourceKeyId, type ResourceStore } from "./resourceStore";

export const PROJECTS_KEY: ResourceKey = { kind: "projects" };

/** The review settings one edit changes; the rest stay as they are. */
export type ReviewSettingsChange = Partial<ProjectReviewSettingsPatch>;

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
 * The resource's rows in its order: each project's branches from its
 * `branches` resource and its PR lists from its remote's `prs`, each empty
 * until that resource arrives. `nowMs` ages the PR lists.
 */
export function projectsFromResource(
  wire: readonly Project[],
  branches: (project: string) => Branches | undefined,
  prs: (remote: string) => Prs | undefined,
  active: TabSummary | null,
  nowMs: number,
): ProjectSnapshot[] {
  return wire.map((project) => {
    const isActive = isActiveProject(project, active);
    const held = branches(project.id);
    const { local, auto } = held ? branchRows(held, active, isActive) : { local: [], auto: [] };
    const row = { id: project.id, remote: project.remote, remote_only: project.remoteOnly };
    return {
      id: project.id,
      name: project.name,
      root_path: project.rootPath,
      remote: project.remote,
      remote_only: project.remoteOnly,
      is_active: isActive,
      local_branches: local,
      auto_branches: auto,
      ...projectPrFields(row, project.remote ? prs(project.remote) : undefined, nowMs),
      auto_triage_max_diff_kb: project.autoTriageMaxDiffKb,
      review_ignore_globs: project.reviewIgnoreGlobs,
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
  /** A plain command call: the old path's list read, and its review-settings change, which `command` could drop. */
  invoke: <T>(name: string, args: Record<string, unknown>) => Promise<T>;
  report: (command: string, e: ErError) => void;
  /** Applies the snapshot an old command answered (`app.ingestCommandSnapshot`); flag off only. */
  ingest: (snapshot: AppSnapshot) => void;
  /** The clock the PR lists are aged by. */
  now?: () => number;
  /** Told how long a flag-on write took from the call until its `Ack`'s versions were held. */
  profile?: (command: string, ms: number) => void;
  sleep?: (ms: number) => Promise<void>;
}

/**
 * The sidebar's projects, read from `app.snapshot` with the flag off and
 * from the `projects` and `branches` resources with it on, behind one read
 * API; and the project commands, sent through the old commands or `er_send`.
 */
export class ProjectsAdapter {
  private readonly branchHolds = new Map<string, () => void>();
  private readonly prHolds = new Map<string, () => void>();

  constructor(private readonly deps: ProjectsAdapterDeps) {
    if (deps.flagOn) deps.store.show(PROJECTS_KEY);
  }

  /** Until the `projects` resource first arrives, the flag-on page shows the snapshot's rows. */
  get projects(): ProjectSnapshot[] {
    const held = this.deps.flagOn ? this.deps.store.get(PROJECTS_KEY)?.value : undefined;
    if (held?.kind !== "projects") return this.deps.snapshot()?.projects ?? [];
    const branches = (project: string) => {
      const value = this.deps.store.get(branchesKey(project))?.value;
      return value?.kind === "branches" ? value : undefined;
    };
    const now = (this.deps.now ?? Date.now)();
    return projectsFromResource(held.projects, branches, (remote) => this.prs(remote), this.deps.activeTab(), now);
  }

  /** The ids the `projects` resource lists; empty until it arrives or with the flag off. */
  get projectIds(): string[] {
    const held = this.deps.store.get(PROJECTS_KEY)?.value;
    return held?.kind === "projects" ? held.projects.map((p) => p.id) : [];
  }

  /** Each remote the listed projects name, once, as the PR cache keys it. */
  get remotes(): string[] {
    const held = this.deps.store.get(PROJECTS_KEY)?.value;
    if (held?.kind !== "projects") return [];
    return [...new Set(held.projects.flatMap((p) => (p.remote ? [p.remote] : [])))];
  }

  /** The `prs` values held, in the order the projects name their remotes. */
  get heldPrs(): Prs[] {
    return this.remotes.flatMap((remote) => {
      const value = this.prs(remote);
      return value ? [value] : [];
    });
  }

  /** Shows the PR lists of exactly these remotes, releasing the rest. */
  showPrs(remotes: readonly string[]): void {
    if (!this.deps.flagOn) return;
    showExactly(this.prHolds, remotes, (remote) => this.deps.store.show(prsKey(remote)));
  }

  /** Shows the branches of exactly these projects, releasing the rest. */
  showBranches(ids: readonly string[]): void {
    if (!this.deps.flagOn) return;
    showExactly(this.branchHolds, ids, (id) => this.deps.store.show(branchesKey(id)));
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

  patchReviewSettings(projectId: string, change: ReviewSettingsChange): Promise<void> {
    const patch: ProjectReviewSettingsPatch = {
      autoTriageMaxDiffKb: null,
      reviewIgnoreGlobAdd: null,
      reviewIgnoreGlobRemove: null,
      ...change,
    };
    if (!this.deps.flagOn) return this.invokeAndIngest("patch_project_review_settings", { projectId, patch });
    return this.write(
      "patch_project_review_settings",
      { projectId, patch },
      { kind: "patch_project_review_settings", project: projectId, patch },
    );
  }

  refreshPrList(): Promise<void> {
    return this.write("refresh_pr_list", {}, { kind: "refresh_pr_list" });
  }

  refreshProjectPrList(projectId: string): Promise<void> {
    return this.write("refresh_project_pr_list", { projectId }, { kind: "refresh_project_pr_list", project: projectId });
  }

  /**
   * Flag off, the old command paints the pin at once. Flag on, nothing
   * paints: the server keeps the 50-pin cap, and the `prs` push shows it.
   */
  savePr(projectId: string, prNumber: number, title: string): Promise<void> {
    return this.write(
      "save_pr",
      { projectId, prNumber, title },
      { kind: "save_pr", project: projectId, number: prNumber, title },
    );
  }

  unsavePr(projectId: string, prNumber: number): Promise<void> {
    return this.write("unsave_pr", { projectId, prNumber }, { kind: "unsave_pr", project: projectId, number: prNumber });
  }

  /** As `savePr`: flag on, the server's dismissed filter shows in the `prs` push. */
  dismissPr(projectId: string, prNumber: number): Promise<void> {
    return this.write(
      "dismiss_remote_pr",
      { projectId, prNumber },
      { kind: "dismiss_pr", project: projectId, number: prNumber },
    );
  }

  undismissPr(projectId: string, prNumber: number): Promise<void> {
    return this.write(
      "undismiss_remote_pr",
      { projectId, prNumber },
      { kind: "undismiss_pr", project: projectId, number: prNumber },
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
    const started = performance.now();
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
    this.deps.profile?.(name, performance.now() - started);
  }

  /**
   * The old command called directly and its snapshot applied, as SettingsPage
   * did before. `command` (`app.cmd`) drops any non-tab command while a tab
   * switch is in flight, with no toast, which would lose a review-settings edit.
   */
  private async invokeAndIngest(name: string, args: Record<string, unknown>): Promise<void> {
    try {
      this.deps.ingest(await this.deps.invoke<AppSnapshot>(name, args));
    } catch (e) {
      // The old command rejects with a string; the toast reads as it did.
      this.deps.report(name, { kind: "internal", message: String(e) });
    }
  }

  private prs(remote: string): Prs | undefined {
    const value = this.deps.store.get(prsKey(remote))?.value;
    return value?.kind === "prs" ? value : undefined;
  }

  private isShown(key: ResourceKey): boolean {
    if (resourceKeyId(key) === resourceKeyId(PROJECTS_KEY)) return true;
    if (key.kind === "prs") return this.prHolds.has(key.remote);
    return key.kind === "branches" && this.branchHolds.has(key.project);
  }
}

/** Holds exactly `wanted` in `holds`, releasing the rest and showing what is new. */
function showExactly(holds: Map<string, () => void>, wanted: readonly string[], show: (id: string) => () => void): void {
  const keep = new Set(wanted);
  for (const [id, release] of holds) {
    if (keep.has(id)) continue;
    release();
    holds.delete(id);
  }
  for (const id of keep) {
    if (!holds.has(id)) holds.set(id, show(id));
  }
}
