import { SvelteMap } from "svelte/reactivity";
import type { Command } from "$lib/protocol/generated/Command";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { TabReviewed } from "$lib/protocol/generated/TabReviewed";
import type { AppSnapshot } from "$lib/types";
import { RECORDED_MODES } from "./diffAdapter";
import type { ResourceStore } from "./resourceStore";

export function reviewedKey(tab: TabKey): ResourceKey {
  return { kind: "reviewed", tab };
}

type Args = Record<string, unknown>;

interface ReviewedAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "send" | "whenAt">;
  /** The registry's active tab, the one the page paints. */
  activeKey: () => TabKey | null;
}

/** A tab key is a number, so the first `|` ends it. */
function pendingId(tab: TabKey, path: string): string {
  return `${tab}|${path}`;
}

/**
 * The active tab's reviewed marks and counts: the snapshot's with the flag
 * off, `reviewed(tab)`'s with it on. The server counts (A8); a click's
 * checkmark shows at once and stays pending until its `Ack`'s versions are
 * held, and a refused write takes it back. A write waits for `reviewed(tab)`:
 * until the page paints the active tab's marks, the rows it shows can be the
 * tab the registry left, so a click there would land on the wrong tab. In the views the App loads
 * (History, Conflicts, Hidden) the count stays the snapshot's.
 */
export class ReviewedAdapter {
  /** The checkmarks painted and not yet acknowledged; `click` tells one click's paint from a later one's. */
  private readonly pending = new SvelteMap<string, { reviewed: boolean; click: object }>();

  constructor(private readonly deps: ReviewedAdapterDeps) {}

  /** The active tab's marks as the resource holds them; null with the flag off or before the first fetch. */
  held(): TabReviewed | null {
    if (!this.deps.flagOn) return null;
    const key = this.deps.activeKey();
    if (key === null) return null;
    const value = this.deps.store.get(reviewedKey(key))?.value;
    return value?.kind === "reviewed" ? value : null;
  }

  /** `snap` with the marks, the count and the pending checkmarks in place of the App's. */
  overlay(snap: AppSnapshot): AppSnapshot {
    const tab = this.deps.activeKey();
    const held = this.held();
    if (tab === null || (!held && this.pending.size === 0)) return snap;
    const marked = held && !RECORDED_MODES.has(held.view.mode) ? new Set(held.paths) : null;
    const files = snap.files.map((file) => {
      const reviewed = this.pending.get(pendingId(tab, file.path))?.reviewed ?? marked?.has(file.path) ?? file.reviewed;
      return reviewed === file.reviewed ? file : { ...file, reviewed };
    });
    return marked && held ? { ...snap, files, reviewed_count: held.reviewed_count } : { ...snap, files };
  }

  /**
   * With the flag on, a reviewed write goes through `er_send` keyed to the
   * active tab, and a bulk review also names the view `reviewed(tab)` was
   * read in; null for any other command and for every command with the flag off.
   */
  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn) return null;
    const tab = this.deps.activeKey();
    if (tab === null) return null;
    switch (command) {
      case "mark_reviewed":
      case "unmark_reviewed":
        return this.mark(tab, String(args.path ?? ""), command === "mark_reviewed");
      case "toggle_reviewed":
        return this.mark(tab, String(args.path ?? ""), null);
      case "bulk_review_pillar":
      case "unbulk_review_pillar":
        return this.bulk(tab, String(args.pillarId ?? args.pillar_id ?? ""), command === "bulk_review_pillar");
      default:
        return null;
    }
  }

  /** Whether `path` shows reviewed on the active tab, its pending mark first. */
  private shows(tab: TabKey, path: string): boolean {
    const pending = this.pending.get(pendingId(tab, path));
    if (pending) return pending.reviewed;
    return this.held()?.paths.includes(path) ?? false;
  }

  /** Paints the checkmark, sends the write, and drops the paint once the `Ack`'s versions are held or the write is refused. */
  private async mark(tab: TabKey, path: string, wanted: boolean | null): Promise<void> {
    if (!path) return;
    if (!this.held()) throw new Error("The reviewed marks have not loaded yet");
    const reviewed = wanted ?? !this.shows(tab, path);
    if (wanted !== null && this.shows(tab, path) === wanted) return;
    const id = pendingId(tab, path);
    const click = {};
    this.pending.set(id, { reviewed, click });
    const command: Command =
      wanted === null ? { kind: "toggle_reviewed", tab, path } : { kind: "mark_reviewed", tab, path, reviewed };
    try {
      await this.send(command);
    } finally {
      if (this.pending.get(id)?.click === click) this.pending.delete(id);
    }
  }

  private bulk(tab: TabKey, pillarId: string, reviewed: boolean): Promise<void> {
    const view = this.held()?.view;
    if (!view) return Promise.reject(new Error("The reviewed marks have not loaded yet"));
    return this.send({ kind: "bulk_review_pillar", tab, view, pillarId, reviewed });
  }

  /** Sends a wire command and resolves once every key its `Ack` names is held at that version. */
  private async send(command: Command): Promise<void> {
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      throw new Error((e as { message?: string }).message ?? String(e), { cause: e });
    }
    await Promise.all(ack.changed.map(({ key, version }) => this.deps.store.whenAt(key, version)));
  }
}
