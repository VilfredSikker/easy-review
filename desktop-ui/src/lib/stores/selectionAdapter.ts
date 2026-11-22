import { SvelteMap } from "svelte/reactivity";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { AppSnapshot, FileSnapshot } from "$lib/types";
import { jumpToUnreviewed, nextFile, nextHunk, prevFile, prevHunk, type FileMove } from "$lib/navigation";

type Args = Record<string, unknown>;

interface SelectionAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  activeKey: () => TabKey | null;
  /** Whether the page draws the active tab from `diff(tab)`; the views the App loads keep its selection. */
  drawsFromDiff: () => boolean;
  /** The snapshot the page shows, with the diff and the marks laid over it. */
  shown: () => AppSnapshot | null;
}

/** The moves the old commands made on the App, by command name. */
const MOVES = new Set(["select_file", "next_file", "prev_file", "next_hunk", "prev_hunk", "jump_to_unreviewed"]);

/**
 * The active tab's selected file and hunk. With the flag on desktop-ui owns
 * them (A10, ADR 0068): a selection or a move is a function over `diff(tab)`
 * and `reviewed(tab)` and makes no IPC call, and the overlay hands the page
 * the selection as the snapshot named it, a position in the shown list. With
 * the flag off, and in the views the App loads, the App's stays.
 */
export class SelectionAdapter {
  /** Each tab's selection by path, so a list that reorders or filters keeps it. */
  private readonly picked = new SvelteMap<TabKey, Picked>();

  constructor(private readonly deps: SelectionAdapterDeps) {}

  private owns(): TabKey | null {
    if (!this.deps.flagOn || !this.deps.drawsFromDiff()) return null;
    return this.deps.activeKey();
  }

  /** `snap` with the page's selection in place of the App's. */
  overlay(snap: AppSnapshot): AppSnapshot {
    const tab = this.owns();
    if (tab === null) return snap;
    const picked = this.picked.get(tab);
    const pos = picked ? snap.files.findIndex((f) => f.path === picked.path) : -1;
    return { ...snap, selected_file: Math.max(0, pos), current_hunk: pos === -1 ? 0 : (picked?.hunk ?? 0) };
  }

  /** A selection or a move, run in the page; null with the flag off, in a view the App loads, or for any other command. */
  route(command: string, args: Args = {}): Promise<void> | null {
    const tab = this.owns();
    const snap = this.deps.shown();
    if (tab === null || !snap || !MOVES.has(command)) return null;
    const to = target(command, args, snap);
    if (to) this.picked.set(tab, to);
    return Promise.resolve();
  }
}

type Picked = { path: string; hunk: number };

/** Where a selection or a move lands on `snap`, by the engine's rules; null when it stays. */
function target(command: string, args: Args, snap: AppSnapshot): Picked | null {
  const files = snap.files;
  const selected: FileSnapshot | undefined = files[snap.selected_file];
  const fileAt = (sourceIndex: number | null): Picked | null => {
    const file = files.find((f) => f.source_index === sourceIndex);
    return file ? { path: file.path, hunk: 0 } : null;
  };
  const landed = (move: FileMove) => (move.kind === "file" || move.kind === "snap" ? fileAt(move.idx) : null);
  const inSelected = (hunk: number | null) => (selected && hunk !== null ? { path: selected.path, hunk } : null);
  const current = selected?.source_index ?? -1;
  const hunk = snap.current_hunk ?? 0;
  const visible = files.map((f) => f.source_index);
  switch (command) {
    case "select_file":
      return fileAt(Number(args.idx));
    case "next_file":
      return landed(nextFile(current, null, visible, []));
    case "prev_file":
      return landed(prevFile(current, null, visible, []));
    case "next_hunk":
      return inSelected(nextHunk(hunk, selected?.hunks.length ?? 0));
    case "prev_hunk":
      return inSelected(prevHunk(hunk));
    default:
      return fileAt(jumpToUnreviewed(current, files.map((f) => [f.source_index, f.reviewed] as const)));
  }
}
