<script lang="ts">
  import { app } from "$lib/stores/app.svelte";
  import { commandPalette } from "$lib/stores/commandPalette.svelte";
  import ModalShell from "$lib/components/ui/ModalShell.svelte";
  import type { BackgroundTaskSnapshot, InboxItemSnapshot, ProjectSnapshot, PrInfo } from "$lib/types";
  import { invoke } from "@tauri-apps/api/core";
  import { tick } from "svelte";

  interface PinnedItem {
    id: string;
    title: string;
    age: string;
  }

  interface Props {
    /** When true, render the narrow icon-only rail (mock lines 239–266). */
    collapsed?: boolean;
    /** Override pinned items (currently no snapshot field — story-time injection). */
    pinnedOverride?: PinnedItem[];
  }
  const { collapsed = false, pinnedOverride }: Props = $props();

  const snapshot = $derived(app.snapshot);
  const worktrees = $derived(snapshot?.worktrees ?? []);
  const projects = $derived<ProjectSnapshot[]>(snapshot?.projects ?? []);
  const inboxItems = $derived<InboxItemSnapshot[]>(snapshot?.inbox_items ?? []);
  const inboxUnreadCount = $derived<number>(snapshot?.inbox_unread_count ?? 0);
  const inboxLastRefreshMs = $derived<number>(snapshot?.inbox_last_refresh_ms ?? 0);
  const loadingPrList = $derived(snapshot?.bg_loading?.pr_list ?? false);
  const activeTab = $derived(snapshot?.tabs?.find((t) => t.is_active) ?? null);

  function projectFromPath(path: string): string {
    const segments = path.split("/").filter(Boolean);
    return segments[segments.length - 2] ?? segments[segments.length - 1] ?? "project";
  }

  const currentWorktree = $derived(worktrees.find((w) => w.is_current));
  const fallbackProjectName = $derived(
    currentWorktree ? projectFromPath(currentWorktree.path) : "current",
  );

  const pinned = $derived<PinnedItem[]>(pinnedOverride ?? []);
  const inboxVisible = $derived(
    [...inboxItems].sort((a, b) => {
      const aUnread = a.read_at_ms == null ? 0 : 1;
      const bUnread = b.read_at_ms == null ? 0 : 1;
      if (aUnread !== bUnread) return aUnread - bUnread;
      return b.created_at_ms - a.created_at_ms;
    }).slice(0, 20),
  );
  const latestInboxMessage = $derived(inboxVisible[0] ?? null);

  let inboxPopoverOpen = $state(false);
  let inboxFilter = $state<"all" | "unread" | "read">("all");
  let inboxProjectFilter = $state<"all" | string>("all");
  let selectedInboxMessage = $state<InboxItemSnapshot | null>(null);
  let expandedProject = $state<string | null>(null);
  let pendingBranchKey = $state<string | null>(null);
  let pendingPrKey = $state<string | null>(null);
  let triagingPrKey = $state<string | null>(null);
  let triagingBranchKey = $state<string | null>(null);
  let prRevealCountByProject = $state<Record<string, number>>({});
  let prSavedRevealCountByProject = $state<Record<string, number>>({});
  let prRecentRevealCountByProject = $state<Record<string, number>>({});
  let sidebarSearch = $state("");
  // Per-project 3-dot menu open state
  let projectMenuOpen = $state<string | null>(null);
  interface MenuAnchor {
    right: number;
    top: number;
    bottom: number;
  }
  let projectMenuAnchor = $state<MenuAnchor | null>(null);
  let projectMenuFlip = $state(false);
  let branchPickerAnchor = $state<MenuAnchor | null>(null);
  let branchPickerFlip = $state(false);

  const PROJECT_MENU_EST_HEIGHT = 200;
  const BRANCH_PICKER_EST_HEIGHT = 256;
  /** Settings footer + gap — keep menus above it when flipping. */
  const SIDEBAR_FOOTER_RESERVE_PX = 52;
  // Per-project sync loading state
  let syncingProject = $state<string | null>(null);
  // Per-project pending delete state
  let pendingDeleteProjectId = $state<string | null>(null);
  let pendingDeleteTimer: ReturnType<typeof setTimeout> | null = null;

  const sidebarSearchNeedle = $derived(sidebarSearch.trim().toLowerCase());
  const searchActive = $derived(sidebarSearchNeedle.length > 0);
  const matchesSearch = (v: string): boolean => v.toLowerCase().includes(sidebarSearchNeedle);
  const filteredProjects = $derived(
    !sidebarSearchNeedle
      ? projects
      : projects.filter((project) => {
          if (matchesSearch(project.name)) return true;
          if (project.local_branches.some((br) => matchesSearch(br.name))) return true;
          if (project.saved_prs?.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          if (project.my_prs.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          if (project.prs_to_review.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          if (project.recent_prs?.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          if (project.recently_merged.some((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))) return true;
          return false;
        }),
  );

  // Branch-picker state for the project 3-dot menu "New" item.
  let addingTo = $state<string | null>(null);
  let availableBranches = $state<string[]>([]);
  let pickerLoading = $state(false);

  function anchorFromButton(btn: HTMLElement, estimatedMenuHeight: number): { anchor: MenuAnchor; flip: boolean } {
    const rect = btn.getBoundingClientRect();
    const spaceBelow = window.innerHeight - rect.bottom - SIDEBAR_FOOTER_RESERVE_PX;
    return {
      anchor: {
        right: window.innerWidth - rect.right,
        top: rect.top,
        bottom: rect.bottom,
      },
      flip: spaceBelow < estimatedMenuHeight,
    };
  }

  function floatingMenuStyle(anchor: MenuAnchor, flip: boolean): string {
    const gap = 4;
    if (flip) {
      return `right:${anchor.right}px;bottom:${window.innerHeight - anchor.top + gap}px`;
    }
    return `right:${anchor.right}px;top:${anchor.bottom + gap}px`;
  }

  async function openBranchPicker(
    projectId: string,
    anchor?: MenuAnchor | null,
    flip = false,
  ) {
    if (addingTo === projectId) {
      addingTo = null;
      branchPickerAnchor = null;
      return;
    }
    addingTo = projectId;
    branchPickerAnchor = anchor ?? null;
    branchPickerFlip = flip;
    pickerLoading = true;
    try {
      availableBranches = await invoke<string[]>("list_available_branches", { projectId });
    } catch (err) {
      app.pushLog("error", "list_available_branches", String(err));
      availableBranches = [];
    } finally {
      pickerLoading = false;
    }
  }

  async function pickBranch(projectId: string, name: string) {
    await app.cmd("add_tracked_branch", { projectId, name });
    addingTo = null;
  }

  function closeBranchPicker() {
    addingTo = null;
    branchPickerAnchor = null;
  }


  function openInboxPopover() {
    inboxPopoverOpen = true;
  }

  function closeInboxPopover() {
    inboxPopoverOpen = false;
  }

  function openInboxMessageModal(item: InboxItemSnapshot) {
    selectedInboxMessage = item;
    app.cmd("mark_inbox_item_read", { id: item.id });
  }

  function closeInboxMessageModal() {
    selectedInboxMessage = null;
  }

  function projectBadge(p: ProjectSnapshot): number {
    return p.local_branches.length + p.my_prs.length + p.prs_to_review.length + (p.recent_prs?.length ?? 0) + p.recently_merged.length;
  }

  function formatCacheAge(ms?: number | null): string {
    if (ms == null || ms < 0) return "";
    const mins = Math.floor(ms / 60_000);
    if (mins < 1) return "<1m";
    if (mins < 60) return `${mins}m`;
    const hrs = Math.floor(mins / 60);
    return `${hrs}h`;
  }

  function formatInboxUpdated(ms: number): string {
    if (!ms || ms <= 0) return "never";
    const delta = Date.now() - ms;
    if (delta < 60_000) return "just now";
    const mins = Math.floor(delta / 60_000);
    if (mins < 60) return `${mins}m ago`;
    const hrs = Math.floor(mins / 60);
    return `${hrs}h ago`;
  }

  function prIconColor(pr: PrInfo): string {
    if (pr.state === "MERGED") return "text-purple-400";
    if (pr.state === "CLOSED") return "text-del-fg";
    if (pr.review_decision === "CHANGES_REQUESTED") return "text-del-fg";
    if (pr.review_decision === "APPROVED") return "text-add-fg";
    if (pr.is_draft) return "text-muted";
    // Ready for review (open, not draft, no decision yet)
    return "text-fg-3";
  }

  /** Map inbox item kind/severity to an SVG path + color class for the icon. */
  interface InboxKindMeta { color: string; path: string; viewBox?: string }
  function inboxKindMeta(item: InboxItemSnapshot): InboxKindMeta {
    // Try kind-based mapping first
    switch (item.kind) {
      case "pr_merged":
      case "merged":
        return { color: "text-periwinkle", path: "M18 6 6 18M6 6l12 12M6 3v6h6" }; // git-merge simplified
      case "ci_failed":
      case "ci-fail":
      case "check_failed":
        return { color: "text-del-fg", path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6" };
      case "review_requested":
      case "review":
        return { color: "text-accent", path: "M1 12s4-8 11-8 11 8 11 8-4 8-11 8-11-8-11-8z M12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z" };
      case "new_comment":
      case "comment":
        return { color: "text-comment", path: "M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z" };
      case "mention":
        return { color: "text-amber-300", path: "M16 8a6 6 0 0 1-12 0 6 6 0 0 1 12 0zM16 8c0 3.3 1.7 6 4 6M20 8v4M20 8a8 8 0 1 0-8 8" };
    }
    // Fall back to severity
    switch (item.severity) {
      case "error":   return { color: "text-del-fg",    path: "M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20zM15 9l-6 6M9 9l6 6" };
      case "warning": return { color: "text-amber-300", path: "M10.29 3.86L1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0zM12 9v4M12 17h.01" };
      default:        return { color: "text-muted",     path: "M18 8h1a4 4 0 0 1 0 8h-1M2 8h16v9a4 4 0 0 1-4 4H6a4 4 0 0 1-4-4V8zM6 1v3M10 1v3M14 1v3" };
    }
  }

  /** Top 2 unread items for the in-rail teaser. */
  const inboxTeaser = $derived(
    [...inboxItems]
      .sort((a, b) => {
        const aUnread = a.read_at_ms == null ? 0 : 1;
        const bUnread = b.read_at_ms == null ? 0 : 1;
        if (aUnread !== bUnread) return aUnread - bUnread;
        return b.created_at_ms - a.created_at_ms;
      })
      .slice(0, 2),
  );

  /** Resolve which project an inbox item belongs to (explicit id or repo/remote match). */
  function inboxItemProjectId(item: InboxItemSnapshot): string | null {
    if (item.target.project_id) return item.target.project_id;
    const root = item.target.repo_root;
    if (root) {
      const match = projects.find((p) => p.root_path && p.root_path === root);
      if (match) return match.id;
    }
    const remote = item.target.remote;
    if (remote) {
      const match = projects.find((p) => p.remote && p.remote === remote);
      if (match) return match.id;
    }
    return null;
  }

  /** Projects that have at least one inbox item, sorted by name. */
  const inboxProjectOptions = $derived(
    projects
      .filter((p) => inboxItems.some((item) => inboxItemProjectId(item) === p.id))
      .sort((a, b) => a.name.localeCompare(b.name)),
  );

  $effect(() => {
    if (
      inboxProjectFilter !== "all" &&
      !inboxProjectOptions.some((p) => p.id === inboxProjectFilter)
    ) {
      inboxProjectFilter = "all";
    }
  });

  /** Inbox items filtered by the selected project. */
  const inboxByProject = $derived(
    inboxProjectFilter === "all"
      ? inboxVisible
      : inboxVisible.filter((i) => inboxItemProjectId(i) === inboxProjectFilter),
  );

  /** Inbox items filtered by the popover tab selection. */
  const inboxFiltered = $derived(
    inboxByProject.filter((i) => {
      if (inboxFilter === "unread") return i.read_at_ms == null;
      if (inboxFilter === "read") return i.read_at_ms != null;
      return true;
    }),
  );
  const inboxUnreadCountAll = $derived(inboxByProject.filter((i) => i.read_at_ms == null).length);

  const hasExpandedProject = $derived(
    !sidebarSearchNeedle && expandedProject !== null,
  );

  function isProjectOpen(p: ProjectSnapshot): boolean {
    if (sidebarSearchNeedle) return true;
    return expandedProject === p.id;
  }

  function collapseAllProjects() {
    expandedProject = null;
  }

  function visibleBranches(project: ProjectSnapshot) {
    return searchActive ? project.local_branches.filter((br) => matchesSearch(br.name)) : project.local_branches;
  }

  function visibleSavedPrs(project: ProjectSnapshot) {
    const list = project.saved_prs ?? [];
    return searchActive
      ? list.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : list;
  }

  function visibleRecentPrs(project: ProjectSnapshot) {
    const list = project.recent_prs ?? [];
    return searchActive
      ? list.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : list;
  }

  function visibleMyPrs(project: ProjectSnapshot) {
    return searchActive
      ? project.my_prs.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : project.my_prs;
  }

  // item 6: exclude CLOSED PRs from To Review
  function visibleToReviewPrs(project: ProjectSnapshot) {
    const base = project.prs_to_review.filter((pr) => pr.state === "OPEN");
    return searchActive
      ? base.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : base;
  }

  function visibleRecentlyMergedPrs(project: ProjectSnapshot) {
    return searchActive
      ? project.recently_merged.filter((pr) => matchesSearch(pr.title) || String(pr.number).includes(sidebarSearchNeedle))
      : project.recently_merged;
  }

  function toggleProject(p: ProjectSnapshot) {
    expandedProject = expandedProject === p.id ? null : p.id;
  }

  async function deleteProject(project: ProjectSnapshot) {
    if (pendingDeleteProjectId !== project.id) {
      pendingDeleteProjectId = project.id;
      if (pendingDeleteTimer) clearTimeout(pendingDeleteTimer);
      pendingDeleteTimer = setTimeout(() => {
        pendingDeleteProjectId = null;
        pendingDeleteTimer = null;
      }, 3000);
      return;
    }
    if (pendingDeleteTimer) {
      clearTimeout(pendingDeleteTimer);
      pendingDeleteTimer = null;
    }
    pendingDeleteProjectId = null;
    projectMenuOpen = null;
    addingTo = null;
    await app.cmd("delete_project", { projectId: project.id });
  }

  async function syncProject(project: ProjectSnapshot) {
    projectMenuOpen = null;
    syncingProject = project.id;
    try {
      await app.cmd("refresh_project_pr_list", { projectId: project.id });
    } finally {
      syncingProject = null;
    }
  }

  function isBranchTriageRunning(project: ProjectSnapshot, branchName: string): boolean {
    return (snapshot?.background_tasks ?? []).some(
      (t: BackgroundTaskSnapshot) =>
        t.kind === "triage" &&
        t.status === "running" &&
        t.repo_root === project.root_path &&
        t.branch_label === branchName &&
        (t.pr_number == null || t.pr_number === undefined),
    );
  }

  function isPrTriageRunning(project: ProjectSnapshot, prNumber: number): boolean {
    return (snapshot?.background_tasks ?? []).some(
      (t: BackgroundTaskSnapshot) =>
        t.kind === "triage" &&
        t.status === "running" &&
        t.pr_number === prNumber &&
        (t.remote_repo === project.remote || t.repo_root === project.root_path),
    );
  }

  async function runPrTriage(project: ProjectSnapshot, pr: PrInfo, e: MouseEvent) {
    e.stopPropagation();
    const key = `${project.id}:${pr.number}`;
    if (triagingPrKey === key) return;
    triagingPrKey = key;
    try {
      await app.cmd("run_pr_triage", { projectId: project.id, prNumber: pr.number });
    } finally {
      if (triagingPrKey === key) triagingPrKey = null;
    }
  }

  async function runBranchTriage(project: ProjectSnapshot, name: string, e: MouseEvent) {
    e.stopPropagation();
    const key = `${project.id}:${name}`;
    if (triagingBranchKey === key) return;
    triagingBranchKey = key;
    try {
      await app.cmd("run_branch_triage", { projectId: project.id, branch: name });
    } finally {
      if (triagingBranchKey === key) triagingBranchKey = null;
    }
  }

  function openProjectMenu(projectId: string, e: MouseEvent) {
    e.stopPropagation();
    if (projectMenuOpen === projectId) {
      closeProjectMenu();
      return;
    }
    const btn = e.currentTarget as HTMLElement;
    const { anchor, flip } = anchorFromButton(btn, PROJECT_MENU_EST_HEIGHT);
    projectMenuAnchor = anchor;
    projectMenuFlip = flip;
    projectMenuOpen = projectId;
  }

  function closeProjectMenu() {
    projectMenuOpen = null;
    projectMenuAnchor = null;
  }

  function branchRowAction(projectId: string, name: string, e: MouseEvent) {
    e.stopPropagation();
    app.cmd("remove_tracked_branch", { projectId, name });
  }

  /** Plain click replaces the active tab. Cmd/Ctrl-click or middle-click opens
   * a new tab. (Inverse of the previous behavior — power users use modifiers
   * when they want to keep the current tab around.) */
  function shouldReplaceTab(e: MouseEvent): boolean {
    return !(e.metaKey || e.ctrlKey || e.button === 1);
  }

  function nextAnimationFrame(): Promise<void> {
    return new Promise((resolve) => {
      if (typeof requestAnimationFrame === "function") {
        requestAnimationFrame(() => resolve());
      } else {
        setTimeout(resolve, 0);
      }
    });
  }

  async function yieldForPendingPaint() {
    await tick();
    await nextAnimationFrame();
  }

  async function openBranch(projectId: string, name: string, e: MouseEvent) {
    const branchKey = `${projectId}:${name}`;
    if (pendingBranchKey === branchKey) return;
    pendingBranchKey = branchKey;
    try {
      await yieldForPendingPaint();
      await app.cmd("open_local_branch", {
        projectId,
        name,
        replace: shouldReplaceTab(e),
      });
    } finally {
      if (pendingBranchKey === branchKey) pendingBranchKey = null;
    }
  }

  function remoteParts(project: ProjectSnapshot): { owner: string; repo: string } | null {
    const remote = project.remote?.trim();
    if (!remote) return null;
    const withoutScheme = remote
      .replace(/^https?:\/\/github\.com\//, "")
      .replace(/\.git$/, "")
      .replace(/^\/+|\/+$/g, "");
    const [owner, repo] = withoutScheme.split("/");
    if (!owner || !repo) return null;
    return { owner, repo };
  }

  async function openPr(project: ProjectSnapshot, prNumber: number, _headRef: string, e: MouseEvent, hint?: PrInfo) {
    const projectId = project.id;
    const prKey = `${projectId}:${prNumber}`;
    if (pendingPrKey === prKey) return;
    pendingPrKey = prKey;
    // Clear any pending hover-prefetch timer for this PR — the click supersedes it.
    cancelPrPrefetch(projectId, prNumber);
    try {
      await yieldForPendingPaint();
      if (project.remote_only) {
        const parts = remoteParts(project);
        if (!parts) return;
        await app.cmd("open_remote_pr", {
          owner: parts.owner,
          repo: parts.repo,
          number: prNumber,
          replace: shouldReplaceTab(e),
        });
      } else {
        await app.cmd("open_pr_review", {
          projectId,
          prNumber,
          replace: shouldReplaceTab(e),
          hint: hint ? buildPrHint(hint) : undefined,
        });
      }
    } finally {
      if (pendingPrKey === prKey) pendingPrKey = null;
    }
  }

  // ── PR hover-prefetch ──
  // After a short debounce on hover, kick a background `prefetch_pr_open` to
  // warm the diff cache so the click feels instant. If the cursor leaves
  // before the debounce fires, the timer is cleared and no fetch starts.
  const PR_HOVER_PREFETCH_DELAY_MS = 150;
  const prPrefetchTimers = new Map<string, ReturnType<typeof setTimeout>>();

  function buildPrHint(pr: PrInfo): {
    baseRef: string;
    headRef: string;
    headOid: string;
    updatedAt: string;
    title: string;
    author: string;
  } | undefined {
    if (!pr.base_ref?.trim() || !pr.head_ref?.trim() || !pr.head_oid?.trim()) {
      return undefined;
    }
    return {
      baseRef: pr.base_ref,
      headRef: pr.head_ref,
      headOid: pr.head_oid,
      updatedAt: pr.updated_at,
      title: pr.title,
      author: pr.author,
    };
  }

  function schedulePrPrefetch(projectId: string, pr: PrInfo) {
    // No useful hint to send → skip; the open path falls back to the slow
    // synchronous gh-pr-view round-trip anyway.
    if (!pr.head_oid || !pr.base_ref) return;
    const key = `${projectId}:${pr.number}`;
    if (prPrefetchTimers.has(key)) return;
    const timer = setTimeout(() => {
      prPrefetchTimers.delete(key);
      // Bypass app.cmd() — that assigns the return value to app.snapshot, and
      // prefetch_pr_open returns () which would null out the snapshot and
      // render the empty page. Fire-and-forget invoke is correct here.
      invoke("prefetch_pr_open", {
        projectId,
        prNumber: pr.number,
        hint: buildPrHint(pr),
      }).catch(() => {
        // Background fetch — failure is logged in Rust, nothing to do here.
      });
    }, PR_HOVER_PREFETCH_DELAY_MS);
    prPrefetchTimers.set(key, timer);
  }

  function cancelPrPrefetch(projectId: string, prNumber: number) {
    const key = `${projectId}:${prNumber}`;
    const timer = prPrefetchTimers.get(key);
    if (timer !== undefined) {
      clearTimeout(timer);
      prPrefetchTimers.delete(key);
    }
  }

  function revealCount(map: Record<string, number>, projectId: string): number {
    return map[projectId] ?? 5;
  }

  function revealMore(map: Record<string, number>, projectId: string): Record<string, number> {
    return { ...map, [projectId]: revealCount(map, projectId) + 5 };
  }

</script>

{#if collapsed}
  <!-- Collapsed rail -->
  <aside class="w-11 bg-surface border-r border-hairline shrink-0 flex flex-col items-center py-3 gap-2 transition-[width] duration-200">
    <button
      onclick={() => app.togglePanel("left")}
      title="Expand sidebar"
      aria-label="Expand left sidebar"
      class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><polyline points="9 18 15 12 9 6"/></svg>
    </button>
    <button
      onclick={() => (app.showEmptyState = true)}
      title="New review"
      aria-label="New review"
      class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
    </button>
    <button title="Search (⌘K)" aria-label="Search" onclick={() => commandPalette.show()} class="w-7 h-7 rounded hover:bg-hover flex items-center justify-center text-fg-3">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
    </button>
    <div class="h-px w-5 bg-hairline my-1"></div>
    <button title={fallbackProjectName} aria-label={fallbackProjectName} class="w-7 h-7 rounded bg-hover flex items-center justify-center text-accent">
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
    </button>
    <div class="mt-auto">
      <button title="Settings" aria-label="Settings" onclick={() => app.setMainView("settings")} class="w-7 h-7 rounded bg-accent flex items-center justify-center text-black text-[10px] font-bold">er</button>
    </div>
  </aside>
{:else}

<aside class="w-60 bg-surface border-r border-hairline shrink-0 flex flex-col h-full overflow-hidden transition-[width] duration-200">
  <!-- Scrollable content; Settings footer is fixed at bottom outside this wrapper. -->
  <div class="flex-1 overflow-y-auto min-h-0">
  <!-- Top actions -->
  <div class="px-2 pt-2 pb-2 space-y-0.5">
    <button
      onclick={() => (app.showEmptyState = true)}
      class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-[12px] text-fg-2"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 5v14M5 12h14"/></svg>
      <span>New review</span>
    </button>
    <button
      onclick={() => { document.querySelector<HTMLInputElement>('[data-left-sidebar-search-input]')?.focus(); }}
      class="w-full flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-[12px] text-fg-3"
    >
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><circle cx="11" cy="11" r="7"/><path d="m21 21-4.3-4.3"/></svg>
      <span>Search</span>
      <span class="kbd ml-auto">⌘P</span>
    </button>
    <div class="px-2 pt-1">
      <input
        data-left-sidebar-search-input
        value={sidebarSearch}
        oninput={(e) => (sidebarSearch = (e.currentTarget as HTMLInputElement).value)}
        class="w-full bg-surface border border-hairline rounded-md px-2 py-1.5 text-[12px] text-fg-2 placeholder:text-muted outline-none"
        placeholder="Search projects, branches, PRs…"
      />
    </div>
  </div>

  {#if pinned.length > 0}
    <div class="px-2 pt-2 pb-2">
      <div class="text-[10px] font-semibold uppercase tracking-[0.06em] text-muted mb-1 px-2">Pinned</div>
      <div class="space-y-0.5">
        {#each pinned as item (item.id)}
          <div class="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-[12px] text-fg-2">
            <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-accent"><path d="M12 17v5M9 10.76V19l3 2 3-2v-8.24"/><path d="M3 7l9-5 9 5"/></svg>
            <span class="truncate">{item.title}</span>
            <span class="font-mono text-[10px] text-muted ml-auto">{item.age}</span>
          </div>
        {/each}
      </div>
    </div>
  {/if}

  <!-- Inbox -->
  <div class="px-2 pt-3 pb-1">
    <!-- Section eyebrow — 10px uppercase 0.06em font-semibold text-muted -->
    <div class="flex items-center px-2 mb-1.5">
      <button
        type="button"
        onclick={openInboxPopover}
        class="text-[10px] font-semibold uppercase tracking-[0.06em] text-muted hover:text-fg-2 transition-colors"
      >
        Inbox
      </button>
      {#if inboxUnreadCount > 0}
        <span class="ml-1.5 text-[9px] font-mono bg-ink-700 text-accent px-1 rounded-full">{inboxUnreadCount}</span>
      {/if}
      <button
        type="button"
        onclick={() => app.cmd("refresh_notifications")}
        class="ml-auto text-[10px] text-muted hover:text-fg transition-colors"
        title="Refresh notifications"
        aria-label="Refresh notifications"
      >↻</button>
    </div>

    <!-- Top 2 unread items inline teaser -->
    <div class="space-y-0.5">
      {#if inboxTeaser.length === 0}
        <div class="px-2 py-1 text-[12px] text-muted">No notifications</div>
      {:else}
        {#each inboxTeaser as item (item.id)}
          {@const meta = inboxKindMeta(item)}
          {@const isUnread = item.read_at_ms == null}
          <button
            type="button"
            onclick={openInboxPopover}
            class="w-full text-left flex items-start gap-2 px-2 py-1.5 rounded-md hover:bg-hover relative group"
          >
            <!-- Unread orange tick on left edge -->
            {#if isUnread}
              <span class="absolute left-0 top-2 bottom-2 w-0.5 bg-accent rounded-r-sm"></span>
            {/if}
            <!-- Kind icon -->
            <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 mt-0.5 {meta.color}">
              <path d={meta.path} />
            </svg>
            <div class="min-w-0 flex-1">
              <div class="text-[12px] {isUnread ? 'font-medium text-fg-2' : 'text-fg-3'} truncate leading-tight">{item.title}</div>
              {#if item.body}
                <div class="text-[11px] text-muted truncate mt-0.5">{item.body}</div>
              {/if}
            </div>
            <!-- Age derived from created_at_ms -->
            <span class="text-[10px] text-muted shrink-0 mt-0.5">
              {#if Date.now() - item.created_at_ms < 60_000}now{:else if Date.now() - item.created_at_ms < 3_600_000}{Math.floor((Date.now() - item.created_at_ms) / 60_000)}m{:else}{Math.floor((Date.now() - item.created_at_ms) / 3_600_000)}h{/if}
            </span>
          </button>
        {/each}
        {#if inboxUnreadCount > 2}
          <button
            type="button"
            onclick={openInboxPopover}
            class="w-full text-left flex items-center gap-1.5 px-2 py-1 text-[11px] text-muted hover:text-fg-2 transition-colors"
          >
            <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M6 9l6 6 6-6"/></svg>
            See {inboxUnreadCount - 2} more
          </button>
        {/if}
      {/if}
    </div>
  </div>

  {#if inboxPopoverOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="fixed inset-0 z-[200]" onclick={closeInboxPopover}></div>
    <!-- Inbox popover — roomier layout with segmented filter tabs -->
    <div
      class="absolute left-2 top-28 z-[201] w-80 rounded-lg border border-border bg-ink-800 shadow-xl flex flex-col overflow-hidden"
      style="max-height: calc(100vh - 120px);"
    >
      <!-- Header: tray icon + Inbox + updated + close -->
      <div class="px-3 pt-2.5 pb-2 border-b border-hairline flex flex-col gap-2">
        <div class="flex items-center gap-1.5">
          <!-- Tray icon (orange) -->
          <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="text-accent shrink-0">
            <path d="M22 12h-6l-2 3h-4l-2-3H2"/>
            <path d="M5.45 5.11L2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.45-6.89A2 2 0 0 0 16.76 4H7.24a2 2 0 0 0-1.79 1.11z"/>
          </svg>
          <span class="text-[12px] font-semibold text-fg">Inbox</span>
          <span class="text-[11px] text-muted">· Updated {formatInboxUpdated(inboxLastRefreshMs)}</span>
          <div class="flex-1"></div>
          <button
            type="button"
            onclick={closeInboxPopover}
            title="Close"
            aria-label="Close inbox"
            class="w-5 h-5 rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover"
          >
            <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5"><path d="M18 6 6 18M6 6l12 12"/></svg>
          </button>
        </div>
        <!-- Segmented filter row -->
        <div class="flex items-center gap-2">
          <div class="inline-flex bg-surface border border-hairline rounded p-0.5">
            <button
              type="button"
              onclick={() => (inboxFilter = "all")}
              class="h-[22px] px-2 rounded-sm text-[11px] font-medium flex items-center gap-1 {inboxFilter === 'all' ? 'bg-hover text-fg' : 'text-muted hover:text-fg-3'}"
            >All <span class="{inboxFilter === 'all' ? 'text-muted' : 'text-muted'} ml-0.5">{inboxByProject.length}</span></button>
            <button
              type="button"
              onclick={() => (inboxFilter = "unread")}
              class="h-[22px] px-2 rounded-sm text-[11px] font-medium flex items-center gap-1 {inboxFilter === 'unread' ? 'bg-hover text-fg' : 'text-muted hover:text-fg-3'}"
            >Unread <span class="{inboxFilter === 'unread' ? 'text-accent' : 'text-muted'} ml-0.5">{inboxUnreadCountAll}</span></button>
            <button
              type="button"
              onclick={() => (inboxFilter = "read")}
              class="h-[22px] px-2 rounded-sm text-[11px] font-medium {inboxFilter === 'read' ? 'bg-hover text-fg' : 'text-muted hover:text-fg-3'}"
            >Read</button>
          </div>
          <div class="flex-1"></div>
          <button
            type="button"
            onclick={() => { app.cmd("mark_all_inbox_read"); }}
            title="Mark all read"
            aria-label="Mark all read"
            class="w-6 h-6 rounded flex items-center justify-center text-periwinkle hover:text-fg hover:bg-hover"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 7 17l-3-3"/><path d="m22 10-7.5 7.5L13 16"/></svg>
          </button>
          <button
            type="button"
            onclick={() => { app.cmd("clear_read_inbox_items"); }}
            title="Clear read"
            aria-label="Clear read"
            class="w-6 h-6 rounded flex items-center justify-center text-periwinkle hover:text-fg hover:bg-hover"
          >
            <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M3 6h18M19 6l-1 14H6L5 6M10 11v6M14 11v6M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/></svg>
          </button>
        </div>
        {#if inboxProjectOptions.length > 0}
          <div class="flex items-center gap-2">
            <label for="inbox-project-filter" class="text-[11px] text-muted shrink-0">Project</label>
            <select
              id="inbox-project-filter"
              bind:value={inboxProjectFilter}
              class="flex-1 min-w-0 bg-surface border border-hairline rounded px-2 py-1 text-[11px] text-fg outline-none"
            >
              <option value="all">All</option>
              {#each inboxProjectOptions as project (project.id)}
                <option value={project.id}>{project.name}</option>
              {/each}
            </select>
          </div>
        {/if}
      </div>
      <!-- Item list -->
      <div class="flex-1 overflow-y-auto p-1">
        {#if inboxFiltered.length === 0}
          <div class="px-3 py-6 text-center text-[12px] text-muted">No items</div>
        {:else}
          {#each inboxFiltered as item (item.id)}
            {@const meta = inboxKindMeta(item)}
            {@const isUnread = item.read_at_ms == null}
            <button
              type="button"
              onclick={() => openInboxMessageModal(item)}
              class="w-full text-left flex items-start gap-[10px] px-[10px] py-2 rounded-md hover:bg-hover relative"
            >
              <!-- Unread orange left tick -->
              {#if isUnread}
                <span class="absolute left-0 top-3 bottom-3 w-0.5 bg-accent rounded-r-sm"></span>
              {/if}
              <!-- Kind icon -->
              <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 mt-0.5 {meta.color}">
                <path d={meta.path} />
              </svg>
              <div class="flex-1 min-w-0">
                <div class="text-[12px] {isUnread ? 'font-medium text-fg-2' : 'text-fg-3'} truncate leading-snug">{item.title}</div>
                {#if item.body}
                  <div class="text-[11px] text-muted truncate mt-0.5">{item.body}</div>
                {/if}
              </div>
              <span class="text-[10px] text-muted shrink-0 mt-0.5 whitespace-nowrap">
                {#if Date.now() - item.created_at_ms < 60_000}now{:else if Date.now() - item.created_at_ms < 3_600_000}{Math.floor((Date.now() - item.created_at_ms) / 60_000)}m{:else}{Math.floor((Date.now() - item.created_at_ms) / 3_600_000)}h{/if}
              </span>
            </button>
          {/each}
        {/if}
      </div>
    </div>
  {/if}

  {#if selectedInboxMessage}
    <ModalShell
      open={true}
      ariaLabel={selectedInboxMessage.title}
      onClose={closeInboxMessageModal}
      backdropClass="fixed inset-0 z-[250] bg-black/60"
      panelClass="fixed left-1/2 top-1/2 z-[251] w-full max-w-2xl -translate-x-1/2 -translate-y-1/2 rounded-lg border border-border bg-surface shadow-xl outline-none"
    >
      <div class="px-4 py-3 border-b border-hairline flex items-center gap-2">
        <span class={selectedInboxMessage.severity === "error" ? "text-del-fg" : selectedInboxMessage.severity === "warning" ? "text-amber-300" : "text-muted"}>●</span>
        <div class="text-sm text-fg-1 truncate">{selectedInboxMessage.title}</div>
        <button class="ml-auto text-muted hover:text-fg px-2" onclick={closeInboxMessageModal}>×</button>
      </div>
      <div class="px-4 py-3 text-sm text-fg-2 whitespace-pre-wrap break-words max-h-[50vh] overflow-y-auto">
        {selectedInboxMessage.body || "(No message body)"}
      </div>
      <div class="px-4 py-3 border-t border-hairline flex items-center justify-end gap-2">
        <button class="px-3 py-1.5 rounded border border-border text-sm text-fg-2 hover:bg-hover" onclick={closeInboxMessageModal}>Close</button>
        <button
          class="px-3 py-1.5 rounded bg-accent text-black text-sm hover:opacity-90"
          onclick={() => {
            if (!selectedInboxMessage) return;
            app.cmd("open_inbox_item", { id: selectedInboxMessage.id });
            closeInboxMessageModal();
          }}
        >
          Open target
        </button>
      </div>
    </ModalShell>
  {/if}

  <!-- Projects -->
  <div class="px-2 pt-3 pb-2">
    <!-- Section eyebrow — 10px uppercase 0.06em font-semibold text-muted -->
    <div class="flex items-center px-2 mb-1.5 gap-1">
      <span class="text-[10px] font-semibold uppercase tracking-[0.06em] text-muted">Projects</span>
      <div class="ml-auto flex items-center gap-1">
        <button
          type="button"
          onclick={collapseAllProjects}
          disabled={!hasExpandedProject}
          title="Collapse all projects"
          aria-label="Collapse all projects"
          class="text-muted hover:text-fg disabled:opacity-30 disabled:cursor-not-allowed"
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="m7 15 5 5 5-5"/>
            <path d="m7 9 5-5 5 5"/>
          </svg>
        </button>
        <button
          type="button"
          onclick={() => app.cmd("open_worktree", {})}
          title="Add project"
          aria-label="Add project"
          class="text-muted hover:text-fg"
        >
          <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
            <path d="M12 5v14M5 12h14"/>
          </svg>
        </button>
        <button
          type="button"
          onclick={() => app.cmd("refresh_pr_list")}
          disabled={loadingPrList}
          title="Refresh PR list"
          aria-label="Refresh PR list"
          class="text-muted hover:text-fg disabled:opacity-40 disabled:cursor-not-allowed"
        >
        <svg
          width="11"
          height="11"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          stroke-width="2.5"
          class={loadingPrList ? "animate-spin" : ""}
        >
          <path d="M21 12a9 9 0 1 1-9-9 9 9 0 0 1 7.8 4.5"/>
          <polyline points="21 3 21 8 16 8"/>
        </svg>
        </button>
      </div>
    </div>
    <div class="space-y-0.5">
      {#if filteredProjects.length > 0}
        {#each filteredProjects as project (project.id)}
          {@const badge = projectBadge(project)}
          {@const open = isProjectOpen(project)}
          <!-- Project header row: chevron + folder + name + badge + 3-dot menu -->
          <div class="group relative">
            <div class="flex items-center">
              <button
                type="button"
                onclick={() => toggleProject(project)}
                class="flex-1 flex items-center gap-1.5 px-2 py-1.5 rounded-md hover:bg-hover text-[12px] text-left {project.is_active ? 'text-fg-2' : 'text-fg-3'} min-w-0"
              >
                <!-- Chevron caret: down when expanded, right when collapsed — 10px fg-muted -->
                <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="shrink-0 text-muted transition-transform {open ? '' : '-rotate-90'}">
                  <path d="M6 9l6 6 6-6"/>
                </svg>
                <!-- Folder icon — 12px text-fg-3 -->
                <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 text-fg-3">
                  <path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/>
                </svg>
                <span class="truncate font-medium">{project.name}</span>
                {#if badge > 0}
                  <span class="font-mono text-[10px] text-muted ml-auto shrink-0">{badge}</span>
                {/if}
              </button>
              <!-- 3-dot more menu button — revealed on row hover -->
              <button
                type="button"
                onclick={(e) => openProjectMenu(project.id, e)}
                title="More options"
                aria-label="More options for {project.name}"
                class="w-6 h-6 rounded flex items-center justify-center text-muted hover:text-fg hover:bg-hover opacity-0 group-hover:opacity-100 transition-opacity shrink-0 mr-1"
              >
                <!-- 3-dot icon -->
                <svg width="13" height="13" viewBox="0 0 24 24" fill="currentColor" class="shrink-0">
                  <circle cx="12" cy="5" r="1.5"/>
                  <circle cx="12" cy="12" r="1.5"/>
                  <circle cx="12" cy="19" r="1.5"/>
                </svg>
              </button>
            </div>

            <!-- 3-dot dropdown menu — matches new-tab menu pattern -->
            {#if projectMenuOpen === project.id && projectMenuAnchor}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="fixed inset-0 z-40" onclick={closeProjectMenu}></div>
              <div
                class="fixed z-50 bg-ink-800 border border-hairline rounded shadow-xl w-36 py-1"
                style={floatingMenuStyle(projectMenuAnchor, projectMenuFlip)}
              >
                {#if !project.remote_only}
                  <button
                    type="button"
                    onclick={() => {
                      const anchor = projectMenuAnchor;
                      const flip = projectMenuFlip;
                      closeProjectMenu();
                      void openBranchPicker(project.id, anchor, flip);
                    }}
                    class="w-full text-left px-3 py-1.5 text-[12px] text-ink-100 hover:bg-ink-700 flex items-center gap-2"
                  >
                    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 text-muted"><path d="M12 5v14M5 12h14"/></svg>
                    New
                  </button>
                {/if}
                <button
                  type="button"
                  onclick={() => syncProject(project)}
                  disabled={syncingProject === project.id}
                  class="w-full text-left px-3 py-1.5 text-[12px] text-ink-100 hover:bg-ink-700 flex items-center gap-2 disabled:opacity-50 disabled:cursor-not-allowed"
                >
                  {#if syncingProject === project.id}
                    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="shrink-0 text-muted animate-spin"><path d="M21 12a9 9 0 1 1-9-9 9 9 0 0 1 7.8 4.5"/><polyline points="21 3 21 8 16 8"/></svg>
                  {:else}
                    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 text-muted"><path d="M21 12a9 9 0 1 1-9-9 9 9 0 0 1 7.8 4.5"/><polyline points="21 3 21 8 16 8"/></svg>
                  {/if}
                  Sync
                </button>
                <div class="h-px bg-hairline my-1"></div>
                <button
                  type="button"
                  onclick={() => deleteProject(project)}
                  class="w-full text-left px-3 py-1.5 text-[12px] flex items-center gap-2 {pendingDeleteProjectId === project.id ? 'text-del-fg font-semibold hover:bg-ink-700' : 'text-ink-100 hover:bg-ink-700'}"
                >
                  <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" class="shrink-0 {pendingDeleteProjectId === project.id ? 'text-del-fg' : 'text-muted'}"><path d="M18 6 6 18M6 6l12 12"/></svg>
                  {pendingDeleteProjectId === project.id ? "Click again to confirm" : "Delete"}
                </button>
              </div>
            {/if}

            <!-- Branch picker (opened from 3-dot menu → New) -->
            {#if addingTo === project.id && branchPickerAnchor}
              <!-- svelte-ignore a11y_click_events_have_key_events -->
              <!-- svelte-ignore a11y_no_static_element_interactions -->
              <div class="fixed inset-0 z-[150]" onclick={closeBranchPicker}></div>
              <div
                class="fixed z-[151] w-56 max-h-64 overflow-y-auto rounded-md border border-border bg-card shadow-xl py-1"
                style={floatingMenuStyle(branchPickerAnchor, branchPickerFlip)}
              >
                {#if pickerLoading}
                  <div class="px-3 py-2 text-xs text-muted">Loading…</div>
                {:else if availableBranches.length === 0}
                  <div class="px-3 py-2 text-xs text-muted">No other local branches</div>
                {:else}
                  {#each availableBranches as name (name)}
                    <button
                      type="button"
                      onclick={() => pickBranch(project.id, name)}
                      class="w-full text-left px-3 py-1.5 text-[12px] text-fg-2 hover:bg-hover truncate"
                      title={name}
                    >{name}</button>
                  {/each}
                {/if}
              </div>
            {/if}
          </div>

          {#if open}
            <div class="ml-4 pl-3 border-l border-hairline space-y-0.5">
              {#if project.pr_cache_stale}
                <div class="flex items-center gap-1.5 px-2 py-1">
                  <span class="text-[9px] uppercase tracking-wider text-amber-400">Stale</span>
                  {#if project.pr_cache_age_ms != null}
                    <span class="text-[9px] text-muted">{formatCacheAge(project.pr_cache_age_ms)} old</span>
                  {/if}
                  {#if loadingPrList}
                    <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/></svg>
                  {/if}
                </div>
              {/if}
              {#if visibleBranches(project).length > 0}
                <!-- Sub-head: 9px uppercase 0.07em font-semibold text-muted/70 (dimmer than eyebrow) -->
                <div class="text-[9px] font-semibold uppercase tracking-[0.07em] text-muted/70 px-2 pt-3 pb-1">Tracked</div>
                {#each visibleBranches(project) as br (br.name)}
                  {@const isActiveView = activeTab?.branch === br.name && activeTab?.repo_root === project.root_path}
                  {@const branchPending = pendingBranchKey === `${project.id}:${br.name}`}
                  {@const branchTriaging = triagingBranchKey === `${project.id}:${br.name}` || isBranchTriageRunning(project, br.name)}
                  <div class="group relative flex items-center">
                    <!-- Orange left tick for active branch row -->
                    {#if isActiveView}
                      <span class="absolute left-0 top-1.5 bottom-1.5 w-0.5 bg-accent rounded-r-sm z-10 pointer-events-none"></span>
                    {/if}
                    <button
                      type="button"
                      title={br.name}
                      onclick={(e) => openBranch(project.id, br.name, e)}
                      onauxclick={(e) => { if (e.button === 1) openBranch(project.id, br.name, e); }}
                      class="w-full flex items-center gap-2 px-2 py-1 rounded-md text-[12px] text-left pr-6 {(isActiveView || branchPending) ? 'bg-hover text-fg font-medium' : 'text-fg-3 hover:bg-hover'}"
                    >
                      {#if isActiveView}
                        <span class="w-1.5 h-1.5 rounded-full {br.is_merged ? 'bg-purple-400' : 'bg-accent'} shrink-0"></span>
                      {:else}
                        <span class="w-1.5 h-1.5 rounded-full {br.is_merged ? 'bg-purple-400' : 'bg-ink-500'} shrink-0"></span>
                      {/if}
                      <div class="flex flex-col min-w-0 flex-1">
                        <span class="truncate">{br.name}</span>
                        {#if br.worktree_path != null}
                          <span class="truncate text-[10px] text-muted font-mono">{br.worktree_path.split("/").slice(-2).join("/")}</span>
                        {/if}
                      </div>
                      {#if branchPending || branchTriaging}
                        <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0 ml-auto">
                          <path d="M21 12a9 9 0 1 1-3-6.7L21 8"/>
                          <path d="M21 3v5h-5"/>
                        </svg>
                      {/if}
                    </button>
                    <span class="absolute right-1 flex items-center gap-0.5 {isActiveView || br.is_current ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'} transition-opacity">
                      {#if project.remote}
                        <button
                          type="button"
                          onclick={(e) => runBranchTriage(project, br.name, e)}
                          disabled={branchTriaging}
                          title="Run triage on this branch"
                          aria-label="Run triage on branch {br.name}"
                          class="w-4 h-4 rounded flex items-center justify-center text-muted hover:text-cyan-400 hover:bg-ink-600 disabled:opacity-50"
                        >
                          {#if branchTriaging}
                            <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="animate-spin"><path d="M21 12a9 9 0 1 1-9-9 9 9 0 0 1 7.8 4.5"/><polyline points="21 3 21 8 16 8"/></svg>
                          {:else}
                            <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/></svg>
                          {/if}
                        </button>
                      {/if}
                      {#if !br.is_current && !isActiveView}
                        <button
                          type="button"
                          onclick={(e) => branchRowAction(project.id, br.name, e)}
                          title="Remove from view"
                          aria-label="Remove branch {br.name} from view"
                          class="w-4 h-4 rounded flex items-center justify-center text-muted hover:text-del-fg hover:bg-ink-600"
                        >
                          <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M18 6 6 18M6 6l12 12"/></svg>
                        </button>
                      {/if}
                    </span>
                  </div>
                {/each}
              {/if}

              {#snippet prRow(pr: PrInfo)}
                {@const isActivePr = (activeTab?.kind === "remote_pr" && activeTab.pr_number === pr.number && project.is_active) ||
                  (activeTab?.kind === "local_branch" && activeTab.branch === pr.head_ref && activeTab.repo_root === project.root_path)}
                {@const prPending = pendingPrKey === `${project.id}:${pr.number}`}
                {@const prTriaging = triagingPrKey === `${project.id}:${pr.number}` || isPrTriageRunning(project, pr.number)}
                <div class="group relative flex items-center">
                  {#if pr.cached}
                    <!-- Sits in the button's left padding — no layout shift for uncached rows. -->
                    <span
                      class="absolute left-0.5 top-1/2 -translate-y-1/2 w-1 h-1 rounded-full bg-add-fg z-10"
                      title="Cached — instant checkout"
                    ></span>
                  {/if}
                  <button
                    type="button"
                    title="{pr.title} #{pr.number}"
                    onclick={(e) => openPr(project, pr.number, pr.head_ref, e, pr)}
                    onauxclick={(e) => { if (e.button === 1) openPr(project, pr.number, pr.head_ref, e, pr); }}
                    onmouseenter={() => { if (!project.remote_only) schedulePrPrefetch(project.id, pr); }}
                    onmouseleave={() => { if (!project.remote_only) cancelPrPrefetch(project.id, pr.number); }}
                    class="w-full flex items-center gap-2 px-2 py-1 rounded-md text-left pr-6 {(isActivePr || prPending) ? 'bg-accent/15 text-fg font-medium' : 'hover:bg-hover text-fg-3'}"
                  >
                    <svg width="11" height="11" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="{prIconColor(pr)} shrink-0">
                      <line x1="6" y1="3" x2="6" y2="15"/>
                      <circle cx="18" cy="6" r="3"/>
                      <circle cx="6" cy="18" r="3"/>
                      <path d="M18 9a9 9 0 0 1-9 9"/>
                    </svg>
                    <span class="truncate text-[12px]">{pr.title}</span>
                    {#if prPending || prTriaging}
                      <svg width="10" height="10" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0">
                        <path d="M21 12a9 9 0 1 1-3-6.7L21 8"/>
                        <path d="M21 3v5h-5"/>
                      </svg>
                    {/if}
                    <span class="shrink-0 text-[10px] mono text-muted">#{pr.number}</span>
                  </button>
                  {#if project.remote}
                    <span class="absolute right-1 flex items-center {isActivePr ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'} transition-opacity">
                      <button
                        type="button"
                        onclick={(e) => runPrTriage(project, pr, e)}
                        disabled={prTriaging}
                        title="Run triage on this PR"
                        aria-label="Run triage on PR #{pr.number}"
                        class="w-4 h-4 rounded flex items-center justify-center text-muted hover:text-cyan-400 hover:bg-ink-600 disabled:opacity-50"
                      >
                        <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M12 2v4M12 18v4M4.93 4.93l2.83 2.83M16.24 16.24l2.83 2.83M2 12h4M18 12h4M4.93 19.07l2.83-2.83M16.24 7.76l2.83-2.83"/></svg>
                      </button>
                    </span>
                  {/if}
                </div>
              {/snippet}

              {#snippet prSectionLabel(text: string)}
                <!-- Sub-heads: 9px uppercase 0.07em font-semibold text-muted/70 (dimmer than eyebrow) -->
                <div class="flex items-center gap-1.5 px-2 pt-3 pb-1">
                  <span class="text-[9px] font-semibold uppercase tracking-[0.07em] text-muted/70">{text}</span>
                  {#if loadingPrList}
                    <svg width="9" height="9" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5" class="text-muted animate-spin shrink-0"><path d="M21 12a9 9 0 1 1-3-6.7L21 8"/><path d="M21 3v5h-5"/></svg>
                  {/if}
                </div>
              {/snippet}

              {#if visibleSavedPrs(project).length > 0}
                {@render prSectionLabel("Saved")}
                {@const savedVisible = visibleSavedPrs(project).slice(0, revealCount(prSavedRevealCountByProject, project.id))}
                {#each savedVisible as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
                {#if visibleSavedPrs(project).length > savedVisible.length}
                  <button
                    type="button"
                    onclick={() => (prSavedRevealCountByProject = revealMore(prSavedRevealCountByProject, project.id))}
                    class="w-full text-left px-2 py-1 rounded-md text-[12px] text-fg-3 hover:bg-hover"
                  >
                    Show more
                  </button>
                {/if}
              {/if}

              {#if visibleMyPrs(project).length > 0 || (loadingPrList && project.my_prs?.length === 0 && !searchActive)}
                {@render prSectionLabel("My PRs")}
                {#each visibleMyPrs(project) as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
              {/if}

              {#if visibleToReviewPrs(project).length > 0 || (loadingPrList && project.prs_to_review?.length === 0 && !searchActive)}
                {@render prSectionLabel("To Review")}
                {@const toReviewVisible = visibleToReviewPrs(project).slice(0, revealCount(prRevealCountByProject, project.id))}
                {#each toReviewVisible as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
                {#if visibleToReviewPrs(project).length > toReviewVisible.length}
                  <button
                    type="button"
                    onclick={() => (prRevealCountByProject = revealMore(prRevealCountByProject, project.id))}
                    class="w-full text-left px-2 py-1 rounded-md text-[12px] text-fg-3 hover:bg-hover"
                  >
                    Show more
                  </button>
                {/if}
              {/if}

              {#if visibleRecentPrs(project).length > 0}
                {@render prSectionLabel("Recent")}
                {@const recentVisible = visibleRecentPrs(project).slice(0, revealCount(prRecentRevealCountByProject, project.id))}
                {#each recentVisible as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
                {#if visibleRecentPrs(project).length > recentVisible.length}
                  <button
                    type="button"
                    onclick={() => (prRecentRevealCountByProject = revealMore(prRecentRevealCountByProject, project.id))}
                    class="w-full text-left px-2 py-1 rounded-md text-[12px] text-fg-3 hover:bg-hover"
                  >
                    Show more
                  </button>
                {/if}
              {/if}

              {#if visibleRecentlyMergedPrs(project).length > 0 || (loadingPrList && project.recently_merged?.length === 0 && !searchActive)}
                {@render prSectionLabel("Recently Merged")}
                {#each visibleRecentlyMergedPrs(project) as pr (pr.number)}
                  {@render prRow(pr)}
                {/each}
              {/if}
            </div>
          {/if}
        {/each}
      {:else if projects.length > 0 && sidebarSearchNeedle}
        <div class="px-2 py-2 text-[12px] text-muted">No matching projects, branches, or PRs.</div>
      {:else}
        <!-- Fallback: legacy single project derived from worktrees -->
        <div class="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-hover text-[12px] text-fg-2">
          <svg width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"><path d="M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z"/></svg>
          <span class="truncate">{fallbackProjectName}</span>
          {#if worktrees.length > 1}
            <span class="font-mono text-[10px] text-muted ml-auto">{worktrees.length}</span>
          {/if}
        </div>
      {/if}
    </div>
  </div>
  </div>

  <!-- Footer: er + Settings — fixed at bottom by being a sibling of the flex-1 scroll area. -->
  <button
    onclick={() => app.setMainView("settings")}
    class="border-t border-hairline p-3 flex items-center gap-2 text-[12px] text-fg-3 shrink-0 hover:bg-hover text-left"
  >
    <div class="w-6 h-6 rounded-md bg-accent flex items-center justify-center text-black text-xs font-bold">er</div>
    <span>Settings</span>
  </button>
</aside>
{/if}

