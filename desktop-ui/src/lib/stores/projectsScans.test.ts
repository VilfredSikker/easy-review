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

  // With the flag on the projects adapter, the worktrees adapter and the PR
  // rows answer these; the snapshot's copies are read only on the flag-off path.
  it("the page reads the projects, the worktrees and the detected PR through the sidebar reads", () => {
    const field = /\b(snapshot|snap)\??\.(projects|worktrees|detected_pr_number)\b/;
    // The flag-off default of the reads, and the flag-off paints of the optimistic layer.
    const allowed = [join("lib", "sidebarReads.ts"), join("lib", "optimisticLocal.ts")];
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
