import {
  prepareFileTreeInput,
  themeToTreeStyles,
  type FileTreePreparedInput,
  type FileTreeRowDecoration,
  type FileTreeRowDecorationContext,
  type GitStatus,
  type GitStatusEntry,
} from "@pierre/trees";
import type { FileSnapshot } from "$lib/types";

/** Mutable bag read by `renderRowDecoration` — updated on each snapshot refresh. */
export interface TreesDecorationState {
  byPath: Map<string, FileSnapshot>;
}

export function filePaths(files: readonly FileSnapshot[]): string[] {
  return files.map((f) => f.path);
}

export function prepareTreesInput(files: readonly FileSnapshot[]): FileTreePreparedInput {
  return prepareFileTreeInput(filePaths(files), { flattenEmptyDirectories: true });
}

export function createDecorationState(files: readonly FileSnapshot[]): TreesDecorationState {
  return { byPath: new Map(files.map((f) => [f.path, f])) };
}

function fileStatusToGit(status: FileSnapshot["status"]): GitStatus {
  switch (status) {
    case "added":
      return "added";
    case "deleted":
      return "deleted";
    case "renamed":
      return "renamed";
    case "copied":
      return "added";
    case "unmerged":
      return "modified";
    case "modified":
    default:
      return "modified";
  }
}

export function toGitStatusEntries(files: readonly FileSnapshot[]): GitStatusEntry[] {
  return files.map((f) => ({ path: f.path, status: fileStatusToGit(f.status) }));
}

/** Secondary labels: +/− counts, findings, lazy stub (Git status uses built-in lane). */
export function rowDecoration(
  state: TreesDecorationState,
  context: FileTreeRowDecorationContext,
): FileTreeRowDecoration | null {
  if (context.item.kind !== "file") return null;
  const file = state.byPath.get(context.item.path);
  if (!file) return null;

  if (file.reviewed) {
    return { text: "reviewed", title: "Marked reviewed in easy-review" };
  }
  if (file.is_lazy_stub) {
    return { text: "···", title: "Hunks not loaded yet" };
  }

  const parts: string[] = [];
  if (file.finding_count > 0) parts.push(String(file.finding_count));
  if (file.comment_count > 0) parts.push(`💬${file.comment_count}`);
  if (file.additions > 0) parts.push(`+${file.additions}`);
  if (file.deletions > 0) parts.push(`−${file.deletions}`);
  if (parts.length === 0) return null;

  return {
    text: parts.join(" "),
    title: `${file.path}: +${file.additions} −${file.deletions}`,
  };
}

/** Host styles aligned with er desktop dark surfaces. */
export function erTreesHostStyles(): Record<string, string> {
  return {
    height: "100%",
    width: "100%",
    minHeight: "0",
    borderRadius: "0",
    ...themeToTreeStyles({
      type: "dark",
      bg: "#16181d",
      fg: "#c8ccd4",
      colors: {
        "gitDecoration.addedResourceForeground": "#7fd99a",
        "gitDecoration.modifiedResourceForeground": "#e5c07b",
        "gitDecoration.deletedResourceForeground": "#f07178",
        "gitDecoration.ignoredResourceForeground": "#6b7280",
        "gitDecoration.untrackedResourceForeground": "#73b8f0",
      },
    }),
  };
}

/** Read-only FileTree options shared by the spike (no rename / DnD / context menu). */
export function readOnlyTreesOptions(
  files: readonly FileSnapshot[],
  decorationState: TreesDecorationState,
  onSelectionChange?: (path: string | null) => void,
) {
  const paths = filePaths(files);
  return {
    paths,
    preparedInput: prepareTreesInput(files),
    flattenEmptyDirectories: true,
    initialExpansion: 2,
    density: "compact" as const,
    icons: "complete" as const,
    gitStatus: toGitStatusEntries(files),
    search: true,
    fileTreeSearchMode: "hide-non-matches" as const,
    composition: { contextMenu: { enabled: false } },
    onSelectionChange: (selected: readonly string[]) => {
      onSelectionChange?.(selected[0] ?? null);
    },
    renderRowDecoration: (ctx: FileTreeRowDecorationContext) => rowDecoration(decorationState, ctx),
  };
}
