import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "../..");
const read = (path: string) => readFileSync(join(srcDir, path), "utf8");
const ADAPTER = join("lib", "stores", "inboxAdapter.ts");
const OPTIMISTIC = join("lib", "optimisticLocal.ts");

const sources = () =>
  readdirSync(srcDir, { recursive: true, encoding: "utf8" }).filter(
    (f) =>
      /\.(ts|svelte)$/.test(f) &&
      !/\.test\.ts$/.test(f) &&
      !f.includes("protocol/generated/") &&
      !f.startsWith(join("lib", "stories")),
  );
const stripped = (f: string) =>
  read(f)
    .replace(/\/\*[\s\S]*?\*\//g, "")
    .replace(/\/\/[^\n]*/g, "");

describe("inbox source scans", () => {
  // With the flag on the server counts the unread items (A8: the rule moved
  // to the server). The flag-off paint recounts the snapshot it painted; the
  // per-PR thread badges count threads, which no resource carries.
  it("no module counts the unread items but the flag-off paint", () => {
    const counts = /\b(read_at_ms|readAtMs)\s*(==|===)\s*null\s*\)\s*\.length\b|\bunread(Count|_count)\s*[+-]=/;
    expect(sources().filter((f) => f !== OPTIMISTIC && counts.test(stripped(f)))).toEqual([]);
    const adapter = stripped(ADAPTER);
    const getter = adapter.slice(adapter.indexOf("get unreadCount()"), adapter.indexOf("get lastRefreshMs()"));
    expect(getter).toMatch(/return this\.held\(\)\?\.unreadCount \?\? 0;/);
    expect(adapter).not.toMatch(/\b(read_at_ms|readAtMs)\s*(==|!=)/);
    expect(stripped(join("lib", "commandPaletteMenus.ts"))).toMatch(/\$\{inboxResource\.unreadCount\} unread/);
  });

  it("the page reads the inbox through the adapter, not the snapshot", () => {
    const field = /\.(inbox_items|inbox_unread_count|inbox_last_refresh_ms)\b/;
    // The adapter's flag-off reads, the flag-off paints, and the snapshot
    // plumbing that keeps the live inbox when a cached diff paints.
    const allowed = [ADAPTER, OPTIMISTIC, join("lib", "tabSnapshotCache.ts")];
    expect(sources().filter((f) => !allowed.includes(f) && field.test(stripped(f)))).toEqual([]);
    for (const reader of ["InboxView.svelte", "InboxPanel.svelte", "LeftSidebar.svelte"]) {
      expect({ reader, reads: stripped(join("lib", "components", reader)).includes("inboxResource.items") }).toEqual({
        reader,
        reads: true,
      });
    }
  });

  it("the inbox commands reach the backend only through the adapter and the tabs route", () => {
    const direct =
      /(cmd|invoke|sendPrefetch)(<[^>]*>)?\(\s*["'](mark_inbox_item_read|mark_inbox_items_read|mark_all_inbox_read|clear_read_inbox_items|clear_inbox_items|refresh_notifications|test_native_notification)["']/;
    expect(sources().filter((f) => f !== ADAPTER && direct.test(stripped(f)))).toEqual([]);
  });

  // With the flag on no global op paints, so nothing waits for the chrome
  // merge an old command's snapshot confirmed.
  it("with the flag on the app store merges no global confirm", () => {
    const store = stripped(join("lib", "stores", "app.svelte.ts"));
    const start = store.indexOf("private mergeGlobalConfirm(");
    const body = store.slice(start, store.indexOf("const current = this.snapshot;", start));
    expect(body).toMatch(/if \(v1ServiceRequested\(\)\) return;/);
  });
});
