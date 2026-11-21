import type { Command } from "$lib/protocol/generated/Command";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { Inbox } from "$lib/protocol/generated/Inbox";
import type { InboxItem } from "$lib/protocol/generated/InboxItem";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { AppSnapshot, InboxItemSnapshot } from "$lib/types";
import type { ResourceStore } from "./resourceStore";

export const INBOX_KEY: ResourceKey = { kind: "inbox" };

/** One wire item in the shape the inbox helpers read. */
export function inboxItemFromWire(item: InboxItem): InboxItemSnapshot {
  const target = item.target;
  return {
    id: item.id,
    kind: item.kind,
    category: item.category,
    severity: item.severity,
    title: item.title,
    body: item.body,
    source: item.source,
    target: {
      project_id: target.projectId,
      repo_root: target.repoRoot,
      remote: target.remote,
      pr_number: target.prNumber,
      branch: target.branch,
      url: target.url,
    },
    created_at_ms: item.createdAtMs,
    read_at_ms: item.readAtMs,
    dedupe_key: item.dedupeKey,
  };
}

type InboxSlice = Pick<AppSnapshot, "inbox_items" | "inbox_unread_count" | "inbox_last_refresh_ms">;

interface InboxAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
  snapshot: () => InboxSlice | null;
  /** The old command path (`app.cmd`), which paints the reads and clears at once; flag off only. */
  command: (name: string, args: Record<string, unknown>) => Promise<void>;
  /** A plain command call: the old test notification, which answers no snapshot. */
  invoke: <T>(name: string, args: Record<string, unknown>) => Promise<T>;
  report: (command: string, e: ErError) => void;
  /** Told how long a flag-on write took from the call until its `Ack`'s versions were held. */
  profile?: (command: string, ms: number) => void;
}

/**
 * The inbox, read from `app.snapshot` with the flag off and from the `inbox`
 * resource with it on, behind one read API; and the inbox commands, sent
 * through the old commands or `er_send`. With the flag on nothing paints: the
 * server applies each write and counts the unread items, and the `inbox`
 * push shows both (A8). The opens go through the tabs adapter's route.
 */
export class InboxAdapter {
  private mapped: { from: Inbox; items: InboxItemSnapshot[] } | null = null;

  constructor(private readonly deps: InboxAdapterDeps) {
    if (deps.flagOn) deps.store.show(INBOX_KEY);
  }

  /** Newest first, hidden kinds left out. With the flag on, empty until `inbox` arrives. */
  get items(): InboxItemSnapshot[] {
    if (!this.deps.flagOn) return this.deps.snapshot()?.inbox_items ?? [];
    const held = this.held();
    if (!held) return [];
    if (this.mapped?.from !== held) this.mapped = { from: held, items: held.items.map(inboxItemFromWire) };
    return this.mapped.items;
  }

  /** The unread items, as the server counts them. */
  get unreadCount(): number {
    if (!this.deps.flagOn) return this.deps.snapshot()?.inbox_unread_count ?? 0;
    return this.held()?.unreadCount ?? 0;
  }

  /** When the PR sweep last ran; 0 before the first. */
  get lastRefreshMs(): number {
    if (!this.deps.flagOn) return this.deps.snapshot()?.inbox_last_refresh_ms ?? 0;
    return this.held()?.lastRefreshMs ?? 0;
  }

  markItemsRead(ids: string[]): Promise<void> {
    return this.write("mark_inbox_items_read", { ids }, { kind: "mark_inbox_items_read", ids });
  }

  /** Removes the named items that are read; unread ones stay. */
  clearItems(ids: string[]): Promise<void> {
    return this.write("clear_inbox_items", { ids }, { kind: "clear_inbox_items", ids });
  }

  refresh(): Promise<void> {
    return this.write("refresh_notifications", {}, { kind: "refresh_notifications" });
  }

  /** Rejects when the system would not show one: with the reason text flag off, an `Error` carrying it flag on. */
  async testNotification(): Promise<void> {
    if (!this.deps.flagOn) {
      await this.deps.invoke<void>("test_native_notification", {});
      return;
    }
    try {
      await this.deps.store.send({ kind: "test_native_notification" });
    } catch (e) {
      throw new Error((e as ErError).message, { cause: e });
    }
  }

  /**
   * Flag off, the old command, which paints the read or the clear at once.
   * Flag on, `er_send`; it resolves once each resource the `Ack` names is
   * held at that version.
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
    await Promise.all(ack.changed.map(({ key, version }) => this.deps.store.whenAt(key, version)));
    this.deps.profile?.(name, performance.now() - started);
  }

  private held(): Inbox | null {
    const value = this.deps.store.get(INBOX_KEY)?.value;
    return value?.kind === "inbox" ? value : null;
  }
}
