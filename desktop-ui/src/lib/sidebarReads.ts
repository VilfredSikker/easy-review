import type { AppSnapshot, ProjectSnapshot, TabSummary, WorktreeSnapshot } from "./pageTypes";

/** What `detectedPrNumber` derives the active branch's PR from. */
export type DetectSnapshot = Pick<AppSnapshot, "tabs" | "active_tab" | "branch" | "detected_pr_number">;

/**
 * Where the page's pure helpers read the sidebar's projects, the active tab's
 * worktrees and the PR its branch has. The default reads the view's own
 * fields, which the unit tests set; the sidebar's wiring installs the
 * adapters.
 */
export interface SidebarReads {
  projects(snapshot: Pick<AppSnapshot, "projects">): ProjectSnapshot[];
  worktrees(snapshot: Pick<AppSnapshot, "worktrees">): WorktreeSnapshot[];
  /** `tab` is the snapshot's active tab, the one its branch belongs to. */
  detectedPrNumber(snapshot: DetectSnapshot, tab: TabSummary | undefined): number | null;
  /** Whether the projects have arrived; the view's own field always has. */
  projectsLoaded?(): boolean;
}

const fromSnapshot: SidebarReads = {
  projects: (snapshot) => snapshot.projects ?? [],
  worktrees: (snapshot) => snapshot.worktrees ?? [],
  detectedPrNumber: (snapshot) => snapshot.detected_pr_number ?? null,
};

let reads: SidebarReads = fromSnapshot;

/** The sidebar's wiring reads from its adapters. */
export function readSidebarFrom(next: SidebarReads): void {
  reads = next;
}

/** Empty while there is no snapshot. */
export function sidebarProjects(snapshot: Pick<AppSnapshot, "projects"> | null | undefined): ProjectSnapshot[] {
  return snapshot ? reads.projects(snapshot) : [];
}

/** Whether the sidebar's projects have arrived, so an empty list means none. */
export function sidebarProjectsLoaded(): boolean {
  return reads.projectsLoaded?.() ?? true;
}

/** Empty while there is no snapshot. */
export function sidebarWorktrees(snapshot: Pick<AppSnapshot, "worktrees"> | null | undefined): WorktreeSnapshot[] {
  return snapshot ? reads.worktrees(snapshot) : [];
}

/** The PR the active tab's branch has (`snapshot.rs`'s `detected_pr_number`). */
export function detectedPrNumber(snapshot: DetectSnapshot): number | null {
  const tab = snapshot.tabs.find((t) => t.is_active) ?? snapshot.tabs[snapshot.active_tab];
  return reads.detectedPrNumber(snapshot, tab);
}
