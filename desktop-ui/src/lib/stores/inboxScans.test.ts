import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "../..");
const read = (path: string) => readFileSync(join(srcDir, path), "utf8");
const ADAPTER = join("lib", "stores", "inboxAdapter.ts");

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
  // The server counts the unread items (A8: the rule moved to the server).
  // The per-PR thread badges count threads, which no resource carries. Only these
  // modules read an item's read state, and no component is among them. The
  // item ids they hand out (a thread's unread ids, the read item ids) may not
  // be counted anywhere either. It stays a text scan: a count spread over
  // several statements could still pass.
  const READ_STATE = [
    join("lib", "pageTypes.ts"), // declares the snapshot field
    ADAPTER, // maps the wire's readAtMs to it
    join("lib", "inboxThreads.ts"), // a thread's unread ids
  ];
  it("no module counts the unread items", () => {
    const readState = /\b(read_at_ms|readAtMs)\b/;
    expect(sources().filter((f) => !READ_STATE.includes(f) && readState.test(stripped(f)))).toEqual([]);
    // In the modules that read it, no statement that names the read state
    // also counts, and no module counts a thread's unread ids beyond testing
    // them for none.
    const counting = /\.length\b|\.size\b|\+=|\+\+|\breduce\(/;
    const counted = READ_STATE.flatMap((f) =>
      stripped(f)
        .split(/[;{}]/)
        .filter((statement) => readState.test(statement) && counting.test(statement))
        .map((statement) => `${f}: ${statement.trim()}`),
    );
    expect(counted).toEqual([]);
    const unreadIds = /\bunreadIds\)*\.length\b(?!\s*(===|!==|==|!=|>)\s*0\b)/;
    expect(sources().filter((f) => unreadIds.test(stripped(f)))).toEqual([]);
    const readIds = /\breadThreadItemIds\([^;\n]*?\)\)*\s*\.(length|size)\b/;
    expect(sources().filter((f) => readIds.test(stripped(f)))).toEqual([]);
    const adapter = stripped(ADAPTER);
    const getter = adapter.slice(adapter.indexOf("get unreadCount()"), adapter.indexOf("get lastRefreshMs()"));
    expect(getter).toMatch(/return this\.held\(\)\?\.unreadCount \?\? 0;/);
    expect(adapter).not.toMatch(/\b(read_at_ms|readAtMs)\s*(==|!=)/);
    expect(stripped(join("lib", "commandPaletteMenus.ts"))).toMatch(/\$\{inboxResource\.unreadCount\} unread/);
  });

  it("the page reads the inbox through the adapter, not the snapshot", () => {
    const field = /\.(inbox_items|inbox_unread_count|inbox_last_refresh_ms)\b/;
    expect(sources().filter((f) => field.test(stripped(f)))).toEqual([]);
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

});
