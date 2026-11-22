import { SvelteMap } from "svelte/reactivity";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { AppSnapshot, FileSnapshot } from "$lib/types";
import { jumpToUnreviewed, nextFile, nextHunk, prevFile, prevHunk } from "$lib/navigation";

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
  private readonly picked = new SvelteMap<TabKey, { path: string; hunk: number }>();

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
    const files = snap.files;
    const selected = files[snap.selected_file];
    const at = (sourceIndex: number) => files.find((f) => f.source_index === sourceIndex);
    const pick = (file: FileSnapshot | undefined, hunk = 0) => {
      if (file) this.picked.set(tab, { path: file.path, hunk });
    };
    const visible = files.map((f) => f.source_index);
    const current = selected?.source_index ?? -1;
    const hunk = snap.current_hunk ?? 0;
    switch (command) {
      case "select_file":
        pick(at(Number(args.idx)));
        break;
      case "next_file":
      case "prev_file": {
        const move = (command === "next_file" ? nextFile : prevFile)(current, null, visible, []);
        if (move.kind === "file" || move.kind === "snap") pick(at(move.idx));
        break;
      }
      case "next_hunk":
      case "prev_hunk": {
        const to = command === "next_hunk" ? nextHunk(hunk, selected?.hunks.length ?? 0) : prevHunk(hunk);
        if (to !== null) pick(selected, to);
        break;
      }
      default: {
        const to = jumpToUnreviewed(
          current,
          files.map((f) => [f.source_index, f.reviewed] as const),
        );
        if (to !== null) pick(at(to));
      }
    }
    return Promise.resolve();
  }
}
