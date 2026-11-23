import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const srcDir = join(here, "../..");
const read = (path: string) => readFileSync(join(srcDir, path), "utf8");

const READERS = [
  "lib/components/LeftSidebar.svelte",
  "lib/components/InboxView.svelte",
  "lib/components/InboxPanel.svelte",
  "lib/components/BranchCard.svelte",
  "lib/components/EmptyState.svelte",
  "lib/components/settings/SettingsPage.svelte",
];

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

describe("projects source scans", () => {
  it("the sidebar readers take their projects from the adapter, not the snapshot", () => {
    for (const reader of READERS) {
      const text = read(reader);
      expect({ reader, reads: text.match(/snapshot\??\.projects\b/g) ?? [] }).toEqual({ reader, reads: [] });
      expect(text).toContain("projectsResource.projects");
    }
  });

  it("the sidebar sends the project commands through the adapter", () => {
    const sidebar = read("lib/components/LeftSidebar.svelte");
    const direct = sidebar.match(
      /(cmd|invoke)(<[^>]*>)?\(\s*["'](reorder_projects|delete_project|add_tracked_branch|remove_tracked_branch|list_available_branches)["']/g,
    );
    expect(direct ?? []).toEqual([]);
  });

  it("the PR writes and the review-settings change reach the backend only through the adapter", () => {
    const direct =
      /(cmd|invoke)(<[^>]*>)?\(\s*["'](refresh_pr_list|refresh_project_pr_list|save_pr|unsave_pr|dismiss_remote_pr|undismiss_remote_pr|patch_project_review_settings)["']/;
    expect(sources().filter((f) => direct.test(stripped(f)))).toEqual([]);
  });

  // The projects adapter, the worktrees adapter and the PR rows answer these.
  // Without it the helpers would read the page view's empty copies.
  it("the sidebar wiring installs the adapters as the sidebar reads", () => {
    const wiring = stripped(join("lib", "stores", "projectsResource.svelte.ts"));
    expect(wiring).toMatch(/^readSidebarFrom\(\{/m);
    // Each read, inside the block it installs.
    const reads = wiring.slice(wiring.indexOf("readSidebarFrom({")).split("});")[0];
    expect(reads).toMatch(/\bprojects:\s*\(\)\s*=>\s*projectsResource\.projects\s*,/);
    expect(reads).toMatch(/\bworktrees:\s*\(\)\s*=>\s*worktreesResource\.worktrees\s*,/);
    expect(reads).toMatch(
      /\bdetectedPrNumber:\s*\(snapshot,\s*tab\)\s*=>\s*detectPrNumber\(tab,\s*snapshot\.branch,\s*projectsResource\.heldPrs\)\s*,/,
    );
    expect(read("main.ts")).toContain('import "$lib/stores/projectsResource.svelte";');
  });

  it("the page reads the projects, the worktrees and the detected PR through the sidebar reads", () => {
    const field = /\b(snapshot|snap)\??\.(projects|worktrees|detected_pr_number)\b/;
    // The default of the reads, which the wiring replaces.
    const allowed = [join("lib", "sidebarReads.ts")];
    expect(sources().filter((f) => !allowed.includes(f) && field.test(stripped(f)))).toEqual([]);
  });

  // A project id is safe to keep, but these modules also hold resource
  // versions and the tab rows that carry wire keys; none of them persists.
  it("no module that reads the projects adapter touches localStorage or sessionStorage", () => {
    const files = readdirSync(srcDir, { recursive: true, encoding: "utf8" }).filter(
      (f) => /\.(ts|svelte)$/.test(f) && !/\.test\.ts$/.test(f) && !f.includes("protocol/generated/"),
    );
    const holds = /from\s+["'][^"']*\/(projectsAdapter|projectsResource\.svelte)["']/;
    const holders = files.filter((f) => holds.test(read(f)));
    for (const reader of READERS) expect(holders).toContain(join(...reader.split("/")));
    expect(holders.filter((f) => /\b(localStorage|sessionStorage)\b/.test(read(f)))).toEqual([]);
  });
});
