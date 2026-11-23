import { resourceClient } from "$lib/protocol/client";
import { profileLog } from "$lib/profileLog";
import { readSidebarFrom } from "$lib/sidebarReads";
import { app } from "./app.svelte";
import { ProjectsAdapter } from "./projectsAdapter";
import { detectPrNumber } from "./prsAdapter";
import { resources } from "./resourceStore";
import { tabsResource } from "./tabsResource.svelte";
import { WorktreesAdapter } from "./worktreesAdapter";

/** How often the PR lists' age is read again, so "N min old" and the stale flag follow time. */
const CLOCK_MS = 30_000;
let now = $state(Date.now());
setInterval(() => (now = Date.now()), CLOCK_MS);

const activeTab = () => tabsResource.tabs.find((tab) => tab.is_active) ?? null;

/** The sidebar's projects, from the sidebar resources. */
export const projectsResource = new ProjectsAdapter({
  store: resources,
  client: resourceClient(),
  activeTab,
  report: (command, e) => app.showToast("error", `${command}: ${e.message}`),
  now: () => now,
  profile: (command, ms) => profileLog("sidebar_write_painted", { command, ms: Math.round(ms) }),
});

/** The active tab's worktrees, from `worktrees(root)`. */
export const worktreesResource = new WorktreesAdapter({
  store: resources,
  activeTab,
});

// Each listed project's branches and each named remote's PR lists are shown
// while the list names them, and the active tab's worktrees while it is active.
$effect.root(() => {
  $effect(() => projectsResource.showBranches(projectsResource.projectIds));
  $effect(() => projectsResource.showPrs(projectsResource.remotes));
  $effect(() => worktreesResource.follow());
});

// The page's helpers read the sidebar from these adapters.
readSidebarFrom({
  projects: () => projectsResource.projects,
  worktrees: () => worktreesResource.worktrees,
  detectedPrNumber: (snapshot, tab) => detectPrNumber(tab, snapshot.branch, projectsResource.heldPrs),
  projectsLoaded: () => projectsResource.loaded,
});
