import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { Command } from "$lib/protocol/generated/Command";
import type { InboxItem } from "$lib/protocol/generated/InboxItem";
import type { ErError } from "$lib/protocol/generated/ErError";
import type { InboxItemSnapshot } from "$lib/types";
import { INBOX_KEY, InboxAdapter, inboxItemFromWire } from "./inboxAdapter";
import { ResourceStore } from "./resourceStore";

function wireItem(id: string, readAtMs: number | null): InboxItem {
  return {
    id,
    kind: "ci_failed",
    category: "ci",
    severity: "warning",
    title: `title ${id}`,
    body: `body ${id}`,
    source: "github",
    target: { projectId: "p", repoRoot: "/src/p", remote: "acme/p", prNumber: 7, branch: null, url: null },
    createdAtMs: 5,
    readAtMs,
    dedupeKey: `dedupe-${id}`,
  };
}

/** The snapshot's inbox, which the flag-off page reads. */
function snapshotItem(id: string): InboxItemSnapshot {
  return { ...inboxItemFromWire(wireItem(id, null)), read_at_ms: null };
}

/**
 * The server's inbox: it applies each write and counts the unread items
 * itself. `unreadCount` is deliberately not the number of unread items, so a
 * page that counted them would show another number.
 */
function backend() {
  const calls: { command: string; args: Record<string, unknown> }[] = [];
  let version = 3;
  let items = [wireItem("a", null), wireItem("b", null), wireItem("c", 9)];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push({ command, args });
    if (command === "er_fetch") {
      // After a send the fetch answers a task later, as IPC does.
      if (version > 3) await new Promise((r) => setTimeout(r, 5));
      const keys = args.keys as { kind: string }[];
      return keys.map((key) => ({
        key,
        version,
        value: { kind: "inbox", items, unreadCount: 40 + version, lastRefreshMs: 1_000 + version },
      })) as T;
    }
    if (command === "er_send") {
      const sent = args.command as Command;
      if (sent.kind === "test_native_notification") {
        const refusal: ErError = { kind: "refused", message: "Could not send a native notification." };
        // The backend rejects with a plain ErError object.
        throw refusal;
      }
      version += 1;
      if (sent.kind === "mark_inbox_items_read") {
        items = items.map((i) => (sent.ids.includes(i.id) ? { ...i, readAtMs: 50 } : i));
      } else if (sent.kind === "clear_inbox_items") {
        items = items.filter((i) => !(sent.ids.includes(i.id) && i.readAtMs !== null));
      } else {
        return { changed: [], created: null } as T;
      }
      return { changed: [{ key: INBOX_KEY, version }], created: null } as T;
    }
    throw Object.assign(new Error(`unexpected ${command}`), { kind: "unsupported" });
  };
  const client = resourceClient(run, async () => () => {});
  return { calls, store: new ResourceStore(client, 60_000) };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

function adapter(flagOn: boolean) {
  const { calls, store } = backend();
  const snapshot = {
    inbox_items: [snapshotItem("a"), snapshotItem("b")],
    inbox_unread_count: 2,
    inbox_last_refresh_ms: 77,
  };
  const old: { name: string; args: Record<string, unknown> }[] = [];
  const reported: { command: string; message: string }[] = [];
  const inbox = new InboxAdapter({
    flagOn,
    store,
    snapshot: () => snapshot,
    command: async (name, args) => {
      old.push({ name, args });
    },
    invoke: async <T,>(name: string, args: Record<string, unknown>) => {
      old.push({ name, args });
      return undefined as T;
    },
    report: (command, e) => {
      reported.push({ command, message: e.message });
    },
  });
  return { calls, old, reported, inbox, snapshot };
}

const sends = (calls: { command: string; args: Record<string, unknown> }[]) =>
  calls.filter((c) => c.command === "er_send").map((c) => c.args.command);

describe("InboxAdapter", () => {
  it("reads app.snapshot with the flag off and sends the old commands", async () => {
    const { calls, old, inbox, snapshot } = adapter(false);
    await settle();
    expect(inbox.items).toBe(snapshot.inbox_items);
    expect(inbox.unreadCount).toBe(2);
    expect(inbox.lastRefreshMs).toBe(77);
    await inbox.markItemsRead(["a"]);
    await inbox.clearItems(["b"]);
    await inbox.refresh();
    await inbox.testNotification();
    expect(old).toEqual([
      { name: "mark_inbox_items_read", args: { ids: ["a"] } },
      { name: "clear_inbox_items", args: { ids: ["b"] } },
      { name: "refresh_notifications", args: {} },
      { name: "test_native_notification", args: {} },
    ]);
    expect(calls).toEqual([]);
  });

  it("reads the items, the server's unread count and the last refresh from inbox with the flag on", async () => {
    const { inbox } = adapter(true);
    expect(inbox.items).toEqual([]);
    expect(inbox.unreadCount).toBe(0);
    await settle();
    expect(inbox.items.map((i) => [i.id, i.read_at_ms])).toEqual([
      ["a", null],
      ["b", null],
      ["c", 9],
    ]);
    expect(inbox.items[0].target).toEqual({
      project_id: "p",
      repo_root: "/src/p",
      remote: "acme/p",
      pr_number: 7,
      branch: null,
      url: null,
    });
    expect(inbox.unreadCount).toBe(43);
    expect(inbox.lastRefreshMs).toBe(1_003);
  });

  it("with the flag on a read paints nothing and shows once the inbox push lands", async () => {
    const { calls, old, inbox, snapshot } = adapter(true);
    await settle();
    const before = snapshot.inbox_items;
    await inbox.markItemsRead(["a"]);
    expect(sends(calls)).toEqual([{ kind: "mark_inbox_items_read", ids: ["a"] }]);
    expect(old).toEqual([]);
    // No paint on the snapshot: the read and the count come from the server.
    expect(snapshot.inbox_items).toBe(before);
    expect(snapshot.inbox_items.map((i) => i.read_at_ms)).toEqual([null, null]);
    expect(inbox.items.map((i) => [i.id, i.read_at_ms])).toEqual([
      ["a", 50],
      ["b", null],
      ["c", 9],
    ]);
    expect(inbox.unreadCount).toBe(44);
  });

  it("with the flag on a clear paints nothing and shows once the inbox push lands", async () => {
    const { calls, old, inbox } = adapter(true);
    await settle();
    await inbox.clearItems(["b", "c"]);
    expect(sends(calls)).toEqual([{ kind: "clear_inbox_items", ids: ["b", "c"] }]);
    expect(old).toEqual([]);
    expect(inbox.items.map((i) => i.id)).toEqual(["a", "b"]);
  });

  it("with the flag on sends the refresh and the test notification through er_send", async () => {
    const { calls, old, inbox } = adapter(true);
    await inbox.refresh();
    const refused = await inbox.testNotification().then(
      () => null,
      (e: unknown) => e,
    );
    expect(sends(calls)).toEqual([{ kind: "refresh_notifications" }, { kind: "test_native_notification" }]);
    expect(old).toEqual([]);
    // The page's toast reads the message, which is the old command's text.
    expect((refused as Error).message).toBe("Could not send a native notification.");
  });
});
