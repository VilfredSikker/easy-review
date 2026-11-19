import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "../..");
const read = (path: string) => readFileSync(join(srcDir, path), "utf8");
const APP_STORE = join("lib", "stores", "app.svelte.ts");

describe("resource source scans", () => {
  it("the tab strip reads its tabs from the adapter, not the snapshot", () => {
    const strip = read("lib/components/TabStrip.svelte");
    expect(strip.match(/snapshot\??\.(tabs|active_tab)\b/g) ?? []).toEqual([]);
    expect(strip).toContain("tabsResource.tabs");
  });

  it("starts the resource store under the flag and keeps the snapshot poll", () => {
    const store = read("lib/stores/app.svelte.ts");
    const start = store.indexOf("  startPolling(");
    const body = store.slice(start, store.indexOf("\n  stopPolling()", start));
    expect(body).toContain('invoke<PollResponse>("poll")');
    expect(body).toMatch(/if \(v1ServiceRequested\(\)\) void resources\.start\(\);/);
  });

  // Tab keys count from 1 in every backend process, so a stored key can name
  // a different tab after a restart.
  it("no module that holds a wire key touches localStorage or sessionStorage", () => {
    const files = readdirSync(srcDir, { recursive: true, encoding: "utf8" }).filter(
      (f) => /\.(ts|svelte)$/.test(f) && !/\.test\.ts$/.test(f) && !f.includes("protocol/generated/"),
    );
    const holdsWireKeys = (text: string) =>
      /\bTabKey\b/.test(text) ||
      /from\s+["'][^"']*\/protocol\//.test(text) ||
      /\bTABS_KEY\b/.test(text) ||
      /from\s+["'][^"']*\/resourceStore["']/.test(text) ||
      /from\s+["'][^"']*\/(tabsAdapter|tabsResource\.svelte)["']/.test(text);
    const holders = files.filter((f) => holdsWireKeys(read(f)));
    expect(holders).toContain(join("lib", "stores", "tabsAdapter.ts"));
    expect(holders).toContain(APP_STORE);
    // app.svelte.ts persists view prefs; the next test scans it in place of this one.
    expect(
      holders.filter((f) => f !== APP_STORE && /\b(localStorage|sessionStorage)\b/.test(read(f))),
    ).toEqual([]);
  });

  it("app.svelte.ts only starts and stops the resource store, and persists view prefs only", () => {
    const code = read(APP_STORE)
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/[^\n]*/g, "");
    // Bound by its import and used only to start and stop, the store gives
    // this module no value to hold.
    expect(code.match(/\bresources\b(\.\w+)?/g)).toEqual([
      "resources",
      "resources.start",
      "resources.stop",
    ]);
    expect(code).toMatch(/import \{ resources \} from "\.\/resourceStore";/);
    const otherStorage = code
      .replace(/\btypeof localStorage\b/g, "")
      .replace(/\blocalStorage\.(getItem|setItem)\(/g, "");
    expect(otherStorage).not.toMatch(/\b(localStorage|sessionStorage)\b/);
    const keys = [
      ...code.matchAll(/\b(?:localStorage\.(?:getItem|setItem)|loadFlag|saveFlag)\(\s*([^,)\s]+)/g),
    ].map((m) => m[1]);
    expect(new Set(keys)).toEqual(
      new Set([
        "DIFF_VIEW_MODE_KEY",
        "COMPACT_LINES_KEY",
        "WRAP_LINES_KEY",
        "COMMENT_VISIBILITY_KEY",
        "HIDE_REVIEWED_KEY",
        "LINE_NUMBERS_KEY",
        "MARK_LINES_KEY",
      ]),
    );
  });
});
