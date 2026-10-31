// The palette's non-AI menus: Actions, Navigate, View & Layout, PR and Files.
// The AI menu stays in CommandPalette.svelte because it drives the palette's
// own provider picker and reviewer view.

import { invoke } from "@tauri-apps/api/core";
import { app } from "$lib/stores/app.svelte";
import { arena } from "$lib/stores/arena.svelte";
import { browser } from "$lib/stores/browser.svelte";
import { diffNav } from "$lib/stores/diffNav.svelte";
import { exportOptions } from "$lib/stores/exportOptions.svelte";
import { openPrUrlModal } from "$lib/stores/prUrlModal.svelte";
import { reviewPage, type ReviewPage } from "$lib/stores/reviewPage.svelte";
import { terminal } from "$lib/stores/terminal.svelte";
import { copyToClipboard } from "$lib/clipboard";
import { resolveContextIdentity } from "$lib/contextIdentity";
import { resolveActivePrNumber, resolveActivePrUrl } from "$lib/prUrl";
import { resolveTabRoot } from "$lib/resolveTabRoot";
import { sourceToggleState } from "$lib/sourceToggle";
import { recentPrs, type PaletteGroup } from "$lib/commandPaletteView";
import {
  copyBranchName,
  copyPrLink,
  copyWorktreePath,
  openListedPr,
  openPrLink,
  revealWorktree,
} from "$lib/paletteQuickActions";
import type { AppSnapshot, TabSummary } from "$lib/types";

export interface CommandItem {
  id: string;
  label: string;
  description?: string;
  group: PaletteGroup;
  kbd?: string;
  /** Action to run when the row is activated (leaf items). */
  run: () => void;
  /** Files rows show the basename and keep the full path for matching. */
  file?: { path: string; additions: number; deletions: number };
  /** Sub-items shown in a second-level list when this item is activated. */
  submenuItems?: CommandItem[];
  /** Render a custom submenu body instead of a list (currently: reviewer picker). */
  view?: "reviewers";
}

export interface PaletteRunners {
  /** Close, let the palette paint away, then run an IPC-heavy action. */
  dismissAndRun: (fn: () => unknown) => void;
  /** Close and run instant local UI. */
  dismissLocal: (fn: () => void) => void;
}

function activeTabOf(snapshot: AppSnapshot | null): TabSummary | undefined {
  const tabs = snapshot?.tabs ?? [];
  return tabs.find((t) => t.is_active) ?? tabs[snapshot?.active_tab ?? 0];
}

function openExportReviewView() {
  app.setMainView("export-review");
  if (browser.layout === "fullscreen") void browser.setLayout("hidden");
}

function exportItems(r: PaletteRunners): CommandItem[] {
  return [
    {
      id: "quick-copy-export",
      label: "Quick copy export",
      description: "markdown with your saved options",
      group: "Actions",
      kbd: "c",
      run: () => r.dismissAndRun(() => exportOptions.quickCopy()),
    },
    {
      id: "export-review-copy",
      label: "Export review",
      description: "choose what to include",
      group: "Actions",
      kbd: "⌘⇧E",
      run: () => r.dismissLocal(openExportReviewView),
    },
    {
      id: "export-review-file",
      label: "Export review to file",
      description: "Write markdown export of the current review",
      group: "Actions",
      run: () => r.dismissAndRun(() => app.cmd("export_to_agent")),
    },
  ];
}

/** Copy branch / worktree path / reveal: the branch context bar's quick actions. */
function checkoutItems(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  const branch = resolveContextIdentity(snapshot).branch;
  // Remote PR tabs have no local checkout, so there is no path to offer.
  const worktree = resolveTabRoot(snapshot, activeTabOf(snapshot)) || null;
  const items: CommandItem[] = [];
  if (branch) {
    items.push({
      id: "copy-branch",
      label: "Copy branch name",
      description: branch,
      group: "Actions",
      kbd: "b",
      run: () => r.dismissAndRun(() => copyBranchName(branch)),
    });
  }
  if (worktree) {
    items.push(
      {
        id: "copy-worktree-path",
        label: "Copy worktree path",
        description: worktree,
        group: "Actions",
        kbd: "w",
        run: () => r.dismissAndRun(() => copyWorktreePath(worktree)),
      },
      {
        id: "reveal-worktree",
        label: "Reveal worktree in Finder",
        group: "Actions",
        kbd: "f",
        run: () => r.dismissAndRun(() => revealWorktree(worktree)),
      },
    );
  }
  return items;
}

function logItems(r: PaletteRunners): CommandItem[] {
  return [
    {
      id: "copy-logs",
      label: `Copy logs to clipboard (${app.logs.length})`,
      description: "All captured errors & warnings since launch",
      group: "Actions",
      run: () => r.dismissLocal(() => {
        const text = app.dumpLogs() || "(no logs)";
        void copyToClipboard(text)
          .then(() => app.pushLog("info", "clipboard", `Copied ${text.length} chars`))
          .catch(() => {});
      }),
    },
    {
      id: "clear-logs",
      label: "Clear logs",
      group: "Actions",
      run: () => r.dismissLocal(() => app.clearLogs()),
    },
  ];
}

export function actionsMenu(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  return [
    {
      id: "open-in-vscode",
      label: "Open in VS Code",
      description: "Open selected file at current hunk (local checkout only)",
      group: "Actions",
      kbd: "e",
      run: () => r.dismissAndRun(() =>
        invoke<{ kind: string; target: string }>("open_in_vscode").then((res) => {
          if (res.kind === "needs_checkout") app.showToast("info", res.target);
        }).catch((e) => app.showToast("error", `VS Code: ${e}`)),
      ),
    },
    ...exportItems(r),
    ...checkoutItems(snapshot, r),
    {
      id: "open-settings",
      label: "Open settings",
      group: "Actions",
      kbd: ",",
      run: () => r.dismissLocal(() => app.setMainView("settings")),
    },
    ...logItems(r),
  ];
}

const PAGES: { page: ReviewPage; key: string; label: string }[] = [
  { page: "overview", key: "1", label: "Go to Overview" },
  { page: "tour", key: "2", label: "Go to Tour" },
  { page: "diff", key: "3", label: "Go to Diff" },
];

/** Only pages the shell can draw are listed: an unavailable page's key does nothing. */
function pageItems(r: PaletteRunners): CommandItem[] {
  return PAGES.filter(({ page }) => reviewPage.available(page)).map(({ page, key, label }) => ({
    id: `go-${page}`,
    label,
    description: reviewPage.current === page ? "current" : undefined,
    group: "Navigate" as const,
    kbd: key,
    run: () => r.dismissAndRun(() => reviewPage.show(page)),
  }));
}

export function navigateMenu(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  return [
    ...pageItems(r),
    // Outside the Tour `]` toggles the right panel, so the row would lie there.
    ...(reviewPage.current === "tour"
      ? [
          {
            id: "next-pillar",
            label: "Next pillar",
            group: "Navigate" as const,
            kbd: "]",
            run: () => r.dismissLocal(() => void diffNav.scrollToNextPillar()),
          },
        ]
      : []),
    {
      id: "open-inbox",
      label: "Open inbox",
      description: `${snapshot?.inbox_unread_count ?? 0} unread`,
      group: "Navigate",
      kbd: "i",
      run: () => r.dismissLocal(() => app.setMainView("inbox")),
    },
    {
      id: "refresh",
      label: "Refresh diff",
      group: "Navigate",
      kbd: "R",
      run: () => r.dismissAndRun(() => app.cmd("refresh_diff")),
    },
    {
      id: "force-refresh",
      label: "Force refresh diff",
      description: "Re-fetch PR head and base from remote",
      group: "Navigate",
      kbd: "⌘R",
      run: () => r.dismissAndRun(() => app.cmd("force_refresh_diff")),
    },
    {
      id: "toggle-terminal",
      label: terminal.open ? "Hide terminal" : "Show terminal",
      description: "bottom drawer at the tab's repo root",
      group: "Navigate",
      kbd: "`",
      run: () => r.dismissLocal(() => terminal.toggle()),
    },
  ];
}

/**
 * The Local Branch | PR Diff switch as one row naming the side it goes to.
 * A tour attached to the PR diff counts as PR, as in the toggle (ADR 0025).
 */
function sourceItem(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem | null {
  const tab = activeTabOf(snapshot);
  if (!snapshot || !tab) return null;
  const prNumber = resolveActivePrNumber(snapshot);
  const toggle = sourceToggleState({
    prNumber,
    tabKind: tab.kind,
    localBranchCheckedOut: snapshot.local_branch_checked_out,
  });
  const prActive = snapshot.mode === "pr" || (snapshot.mode === "tour" && snapshot.tour?.scope === "pr");
  const available = prActive ? toggle.localAvailable : toggle.prAvailable;
  const reason = prActive ? toggle.localReason : toggle.prReason;
  return {
    id: "switch-source",
    label: prActive ? "Switch to Local Branch" : "Switch to PR Diff",
    description: reason ?? undefined,
    group: "View & Layout",
    kbd: "l",
    run: () => {
      if (!available) return;
      r.dismissAndRun(() =>
        app.cmd("set_mode", prActive ? { mode: "branch" } : { mode: "pr_diff", prNumber }),
      );
    },
  };
}

export function viewMenu(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  const items: CommandItem[] = [
    {
      id: "toggle-diff-view-mode",
      label: "Toggle diff view (unified/split)",
      description: `Currently: ${app.diffViewMode}`,
      group: "View & Layout",
      kbd: "d",
      run: () => r.dismissLocal(() => app.toggleDiffViewMode()),
    },
    {
      id: "open-browser-view",
      label: browser.open ? "Cycle browser layout" : "Open browser (split)",
      description: "Per-tab embedded browser — ⌘B cycles hidden → split → fullscreen",
      group: "View & Layout",
      kbd: "⌘B",
      run: () => r.dismissLocal(() => {
        void (browser.open ? browser.cycleLayout() : browser.setLayout("split"));
      }),
    },
    {
      id: "toggle-left",
      label: "Toggle left panel",
      group: "View & Layout",
      kbd: "[",
      run: () => r.dismissLocal(() => app.togglePanel("left")),
    },
    {
      id: "toggle-tree",
      label: "Toggle file tree",
      group: "View & Layout",
      kbd: "\\",
      run: () => r.dismissLocal(() => app.togglePanel("tree")),
    },
    {
      id: "toggle-right",
      label: "Toggle right panel",
      description: "findings and risks",
      group: "View & Layout",
      kbd: "]",
      run: () => r.dismissLocal(() => app.togglePanel("right")),
    },
  ];
  const source = sourceItem(snapshot, r);
  if (source) items.push(source);
  return items;
}

function prLinkItems(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  const url = resolveActivePrUrl(snapshot);
  if (!url) return [];
  const prNumber = resolveActivePrNumber(snapshot);
  const which = prNumber != null ? `#${prNumber}` : "this PR";
  return [
    {
      id: "copy-pr-link",
      label: "Copy PR link",
      description: which,
      group: "PR",
      kbd: "c",
      run: () => r.dismissAndRun(() => copyPrLink(url)),
    },
    {
      id: "open-pr-link",
      label: "Open PR on GitHub",
      description: `${which} in the browser`,
      group: "PR",
      kbd: "o",
      run: () => r.dismissAndRun(() => openPrLink(url)),
    },
  ];
}

function recentPrItems(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  const current = {
    projectId: snapshot?.projects?.find((p) => p.is_active)?.id ?? null,
    prNumber: resolveActivePrNumber(snapshot),
  };
  return recentPrs(snapshot?.projects, current).map(({ project, pr }) => ({
    id: `recent-pr-${project.id}-${pr.number}`,
    label: `#${pr.number} ${pr.title}`,
    description: project.name,
    group: "PR" as const,
    run: () => r.dismissAndRun(() => openListedPr(project, pr)),
  }));
}

export function prMenu(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  return [
    {
      id: "open-pr-url",
      label: "Open PR by URL",
      description: "Paste a GitHub PR link to open it",
      group: "PR",
      kbd: "⌘⇧O",
      run: () => r.dismissLocal(() => openPrUrlModal()),
    },
    ...prLinkItems(snapshot, r),
    {
      id: "pr-open-arena",
      label: "Open AI review arena",
      description: "several reviewers, judged against each other",
      group: "PR",
      kbd: "a",
      run: () => r.dismissLocal(() => arena.openLauncher()),
    },
    ...recentPrItems(snapshot, r),
  ];
}

export function filesMenu(snapshot: AppSnapshot | null, r: PaletteRunners): CommandItem[] {
  return (snapshot?.files ?? []).map((file) => ({
    id: `file-${file.path}`,
    label: file.path,
    group: "Files in this diff" as const,
    file: { path: file.path, additions: file.additions, deletions: file.deletions },
    run: () => r.dismissAndRun(() => {
      // Mirror FileTree.jumpToFile: select + scroll the flat diff to the file.
      void diffNav.scrollToFile(file.path);
      return app.cmd("select_file", { idx: file.source_index });
    }),
  }));
}

