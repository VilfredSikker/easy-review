import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "../..");
const read = (path: string) => readFileSync(join(srcDir, path), "utf8");

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
      /from\s+["'][^"']*\/tabsAdapter["']/.test(text);
    const holders = files.filter((f) => holdsWireKeys(read(f)));
    expect(holders).toContain(join("lib", "stores", "tabsAdapter.ts"));
    expect(holders.filter((f) => /\b(localStorage|sessionStorage)\b/.test(read(f)))).toEqual([]);
  });
});
