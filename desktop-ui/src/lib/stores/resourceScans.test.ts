import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "../..");
const read = (path: string) => readFileSync(join(srcDir, path), "utf8");
const APP_STORE = join("lib", "stores", "app.svelte.ts");
const APP_ROOT = "App.svelte";

describe("resource source scans", () => {
  it("the tab strip reads its tabs from the adapter, not the snapshot", () => {
    const strip = read("lib/components/TabStrip.svelte");
    expect(strip.match(/snapshot\??\.(tabs|active_tab)\b/g) ?? []).toEqual([]);
    expect(strip).toContain("tabsResource.tabs");
  });

  it("the page starts the resource store and the App's snapshot reader beside it", () => {
    const root = read(APP_ROOT);
    expect(root).toMatch(/void resources\.start\(\);\s*legacySnapshot\.start\(\);/);
    expect(root).toMatch(/resources\.stop\(\);\s*legacySnapshot\.stop\(\);/);
  });

  // Tab keys count from 1 in every backend process, so a stored key can name
  // a different tab after a restart.
  it("no module that holds a wire key touches localStorage or sessionStorage", () => {
    const files = readdirSync(srcDir, { recursive: true, encoding: "utf8" }).filter(
      (f) => /\.(ts|svelte)$/.test(f) && !/\.test\.ts$/.test(f) && !f.includes("protocol/generated/"),
    );
    const importsResourceStore = /from\s+["'][^"']*\/resourceStore["']/;
    const importsTabs = /from\s+["'][^"']*\/(tabsAdapter|tabsResource\.svelte)["']/;
    const clauses = [
      /\bTabKey\b/,
      /from\s+["'][^"']*\/protocol\//,
      /\bTABS_KEY\b/,
      importsResourceStore,
      importsTabs,
    ];
    // App.svelte imports the store only to start and stop it, and the tabs
    // adapter only to read its rows, which carry no key, so neither import
    // counts there.
    const clausesFor = (f: string) =>
      f === APP_ROOT ? clauses.filter((c) => c !== importsTabs && c !== importsResourceStore) : clauses;
    const holders = files.filter((f) => clauses.some((c) => c.test(read(f))));
    expect(holders).toContain(join("lib", "stores", "tabsAdapter.ts"));
    expect(holders).toContain(APP_ROOT);
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
    expect(sources().filter((f) => field.test(stripped(f)))).toEqual([]);
  });

  it("the picker, update check and panel mirror call the old commands only from their adapters", () => {
    const old = /(invoke|cmd)(<[^>]*>)?\(\s*"(list_ai_providers|list_ai_reviewers|set_ai_selection|set_ai_effort|check_app_update|toggle_panel)"/;
    expect(sources().filter((f) => old.test(stripped(f)))).toEqual([]);
  });

  // The page runs on the resources alone. The one reader of the App's
  // snapshot serves only the fields no resource carries yet, and nothing
  // reads the store's old snapshot or the build flag the old page branched on.
  it("no module reads app.snapshot, and only the App's snapshot reader asks the App for one", () => {
    const LEGACY = join("lib", "stores", "legacySnapshot.svelte.ts");
    expect(sources().filter((f) => /\bapp\.(snapshot|appSnapshot)\b|\.overlaySnapshot\(/.test(stripped(f)))).toEqual([]);
    const asksTheApp = /["'](get_snapshot|poll)["']|["']er:\/\/revision["']/;
    expect(sources().filter((f) => asksTheApp.test(stripped(f)))).toEqual([LEGACY]);
    expect(sources().filter((f) => /v1ServiceFlag|VITE_ER_V1_SERVICE|\bflagOn\b/.test(stripped(f)))).toEqual([]);
  });

  it("app.svelte.ts holds no resource store, and persists view prefs only", () => {
    const code = read(APP_STORE)
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/[^\n]*/g, "");
    expect(code).not.toMatch(/\bresources\b|resourceStore/);
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
  // The tab rows and the browser state, which the tabs adapter and the browser
  // view read with the flag off.
  it("the page reads the tabs and the browser through their adapters, not the snapshot", () => {
    const field = /\b(snapshot|snap)\??\.(tabs|active_tab|browser)\b/;
    // The adapters' flag-off reads, the app store, the snapshot plumbing, and
    // the helpers that name which tab a snapshot's content belongs to: keying
    // those on the registry while the content still comes from the App would
    // file a draft, a preview or a request under the wrong tab for a round trip.
    const allowed = [
      join("lib", "stores", "tabsAdapter.ts"),
      join("lib", "stores", "browserView.ts"),
      APP_STORE,
      join("lib", "snapshotChrome.ts"),
      join("lib", "tabSnapshotCache.ts"),
      join("lib", "contextIdentity.ts"),
      join("lib", "stores", "reviewPageModel.ts"),
      join("lib", "prUrl.ts"),
      join("lib", "sidebarReads.ts"),
      join("lib", "annotationDraftCache.ts"),
      join("lib", "documentPreviewCache.ts"),
      join("lib", "imagePreview.ts"),
      join("lib", "commentAutoPull.ts"),
      join("lib", "riskRequests.ts"),
      join("lib", "exportOptions.ts"),
      join("lib", "components", "overview", "overviewModel.ts"),
      join("lib", "components", "FlatDiffView.svelte"),
    ];
    expect(sources().filter((f) => !allowed.includes(f) && field.test(stripped(f)))).toEqual([]);
  });

  it("the tab commands and the opens reach the backend only through app.cmd and the prefetch seam", () => {
    const old =
      /\binvoke(<[^>]*>)?\(\s*"(open_remote_pr|open_pr_url|open_worktree|open_local_branch|open_pr_review|open_pr_branch|prefetch_pr_open|prefetch_remote_pr_open|open_project_branch|new_tab|close_tab|select_tab|reorder_tabs|update_tab_browser|cycle_tab_browser_layout|set_active_project|open_inbox_item|prefetch_inbox_item)"/;
    expect(sources().filter((f) => old.test(stripped(f)))).toEqual([]);
    const prOpen = stripped(join("lib", "prOpen.ts"));
    expect(prOpen).toMatch(/invokeFn: InvokeFn = \(command, args\) => prefetchSend\(command, args\)/);
  });

  it("app.cmd hands a command to the first route that takes it, the tabs adapter's among them", () => {
    const store = stripped(APP_STORE);
    const start = store.indexOf("  async cmd(");
    const body = store.slice(start, store.indexOf("\n  }\n", start));
    expect(body).toMatch(/await \(this\.routed\(command, args\) \?\? invoke\(command, args\)\);/);
    const wiring = stripped(join("lib", "stores", "tabsResource.svelte.ts"));
    expect(wiring).toMatch(/app\.routeCmds\(\(command, args\) => tabsResource\.route\(command, args\)\);\s*routePrefetches\(/);
  });

  // The page view runs the diff overlay, which reads the active tab and its
  // PR head through these adapters, so a read of the page view in either
  // recurses until the stack overflows.
  it("the adapters the diff overlay reads never read the page view", () => {
    const block = (f: string, ctor: string) => {
      const code = stripped(join("lib", "stores", f));
      const start = code.indexOf(`new ${ctor}({`);
      return code.slice(start, code.indexOf("});", start));
    };
    for (const [f, ctor] of [
      ["tabsResource.svelte.ts", "TabsAdapter"],
      ["diffResource.svelte.ts", "DiffAdapter"],
    ]) {
      expect(block(f, ctor)).not.toMatch(/pageView/);
    }
    expect(block("tabsResource.svelte.ts", "TabsAdapter")).toMatch(/rows: \(\) => legacySnapshot\.tabRows,/);
    // Every dot, the active branch view's too, reads diff(tab), never the App's stale mark.
    expect(block("tabsResource.svelte.ts", "TabsAdapter")).not.toMatch(/diff_stale/);
  });

  // The key handler's modes are the frontend's own; the App keeps input_mode
  // for the TUI (D20), and no resource carries it.
  it("no module reads input_mode off the snapshot", () => {
    const field = /\.input_mode\b|\binput_mode:\s*(prev|next|snap|snapshot)\b/;
    expect(sources().filter((f) => f !== join("lib", "pageTypes.ts") && field.test(stripped(f)))).toEqual([]);
  });

  it("App.svelte reads only the adapter's rows, and persists its two panel sizes only", () => {
    const code = read(APP_ROOT)
      .replace(/\/\*[\s\S]*?\*\//g, "")
      .replace(/\/\/[^\n]*/g, "");
    expect(new Set(code.match(/\btabsResource\b(\.\w+)?/g))).toEqual(
      new Set(["tabsResource", "tabsResource.svelte", "tabsResource.tabs", "tabsResource.active"]),
    );
    const keys = [...code.matchAll(/\blocalStorage\.(?:getItem|setItem)\(\s*([^,)\s]+)/g)].map((m) => m[1]);
    expect(new Set(keys)).toEqual(new Set(["RIGHT_PANEL_STORAGE_KEY", "DRAWER_STORAGE_KEY"]));
    expect(code.replace(/\blocalStorage\.(getItem|setItem)\(/g, "")).not.toMatch(/\b(localStorage|sessionStorage)\b/);
  });

  // With the flag on a live run's progress reaches the arena page through
  // `jobs`; followRunProgress's own test drives the feed it is handed, so these
  // hold App.svelte to calling it and its default feed to the module's flag.
  it("App.svelte follows a run's progress through followRunProgress and listens for no er://revision", () => {
    expect(stripped(APP_ROOT)).toMatch(/followRunProgress\(\(\) => arena\.onRevision\(\)\)/);
    // Read unstripped: the comment strip would cut the event name at its "//".
    expect(read(APP_ROOT)).not.toContain("er://revision");
  });

  it("the review and screenshot reads go through the annotation reads, which fetch them with the flag on", () => {
    const old = /invoke(?:<[^>]*>)?\(\s*"(read_review_json|list_review_revisions|read_annotation_screenshot)"/;
    const callers = sources().filter((f) => old.test(stripped(f)));
    expect(callers).toEqual([join("lib", "stores", "annotationReads.ts")]);
    const reads = stripped(join("lib", "stores", "annotationReads.ts"));
    for (const kind of ["review_json", "review_revisions", "annotation_screenshot"]) {
      expect(reads).toContain(`kind: "${kind}"`);
    }
    // The tab comes from the adapter, which reads no view: the App's mode goes stale with the flag on.
    expect(reads.match(/annotationsResource\.fetchedTab\(\)/g)).toHaveLength(2);
    expect(reads).not.toMatch(/\bapp\b|\.mode\b/);
  });

  it("DiffView joins its hunks with the threads of annotations(tab)", () => {
    const store = stripped(join("lib", "stores", "diffResource.svelte.ts"));
    expect(store).toMatch(/threads: \(\) => annotationsResource\.threads\(\) \?\? \[\],/);
  });

  it("with the flag on the previews and the path list read the tab's content through er_fetch", () => {
    const wiring = read("lib/stores/diffResource.svelte.ts");
    expect(wiring).toMatch(/diffPreview\.route\([\s\S]*?kind: "file_preview"/);
    expect(wiring).toMatch(/imagePreview\.route\([\s\S]*?kind: "image_preview"/);
    expect(wiring).toMatch(/kind: "diff_paths"/);
    expect(read("lib/components/AiReviewFilesModal.svelte")).not.toContain('"list_diff_paths"');
  });

  it("a jump to a thread or a finding fetches the target file's hunks before it scrolls", () => {
    const dom = read("lib/dom.ts");
    for (const [from, scroll] of [["navigateToThread", "scrollToThread"], ["navigateToFinding", "scrollToFinding"]]) {
      const start = dom.indexOf(`export async function ${from}(`);
      const body = dom.slice(start, dom.indexOf("\n}\n", start));
      expect(body).toMatch(new RegExp(`await loadStub\\(fileSnap\\);[\\s\\S]*diffNav\\.${scroll}\\(`));
    }
  });
});
