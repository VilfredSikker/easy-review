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
    const importsResourceStore = /from\s+["'][^"']*\/resourceStore["']/;
    const clauses = [
      /\bTabKey\b/,
      /from\s+["'][^"']*\/protocol\//,
      /\bTABS_KEY\b/,
      importsResourceStore,
      /from\s+["'][^"']*\/(tabsAdapter|tabsResource\.svelte)["']/,
    ];
    // app.svelte.ts imports the store only to start and stop it (the next test
    // holds it to that), so for it alone the store import does not count.
    const clausesFor = (f: string) =>
      f === APP_STORE ? clauses.filter((c) => c !== importsResourceStore) : clauses;
    const holders = files.filter((f) => clauses.some((c) => c.test(read(f))));
    expect(holders).toContain(join("lib", "stores", "tabsAdapter.ts"));
    expect(holders).toContain(APP_STORE);
    expect(
      files.filter((f) => {
        const text = read(f);
        return (
          clausesFor(f).some((c) => c.test(text)) &&
          /\b(localStorage|sessionStorage)\b/.test(text)
        );
      }),
    ).toEqual([]);
  });

  // The chrome's snapshot fields, which the session adapter reads with the flag off.
  const CHROME_FIELDS =
    "theme|features|display|arena_enabled|config_load_error|active_ai_label|active_ai_provider_label|active_ai_model_label|active_ai_effort|app_version|watch_active|watch_status|panels|notification";
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

  it("the chrome reads the session adapter, not the snapshot", () => {
    const field = new RegExp(`\\b(snapshot|snap)\\??\\.(${CHROME_FIELDS})\\b`);
    // The adapter's flag-off reads, the snapshot plumbing that copies the
    // fields between snapshots, and the flag-off toast in the app store.
    const allowed = [
      join("lib", "stores", "sessionAdapter.ts"),
      join("lib", "tabSnapshotCache.ts"),
      join("lib", "snapshotChrome.ts"),
      APP_STORE,
    ];
    expect(sources().filter((f) => !allowed.includes(f) && field.test(stripped(f)))).toEqual([]);
  });

  it("the picker, update check and panel mirror call the old commands only from their adapters", () => {
    const old = /(invoke|cmd)(<[^>]*>)?\(\s*"(list_ai_providers|list_ai_reviewers|set_ai_selection|set_ai_effort|check_app_update|toggle_panel)"/;
    const allowed = [join("lib", "stores", "sessionAdapter.ts"), join("lib", "stores", "panelMirror.ts")];
    expect(sources().filter((f) => !allowed.includes(f) && old.test(stripped(f)))).toEqual([]);
  });

  it("with the flag off Settings invokes the picks and the palette sends them through app.cmd", () => {
    expect(stripped(join("lib", "components", "CommandPalette.svelte"))).toMatch(
      /\.setAiSelection\(providerId, modelId, effort, "app"\)/,
    );
    expect(stripped(join("lib", "components", "settings", "AiSettings.svelte"))).not.toMatch(/setAi\w+\([^)]*"app"/);
  });

  it("with the flag on the app store shows no snapshot notification, which the notices carry", () => {
    const store = stripped(APP_STORE);
    const start = store.indexOf("private syncSnapshotToast(");
    const body = store.slice(start, store.indexOf("snapshot?.notification", start));
    expect(body).toMatch(/if \(v1ServiceRequested\(\)\) return false;/);
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
