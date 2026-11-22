import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { FileRow } from "$lib/protocol/generated/FileRow";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabDiff } from "$lib/protocol/generated/TabDiff";
import type { Hunk } from "$lib/protocol/generated/Hunk";
import type { AppSnapshot, FileSnapshot, ThreadSnapshot } from "$lib/types";
import { DiffAdapter, diffKey, prStale, tabIsStale, wireDiffCommand, type DiffSlice, type HunkSource } from "./diffAdapter";
import { ResourceStore } from "./resourceStore";

const settle = () => new Promise((r) => setTimeout(r, 0));

function fileRow(path: string, sourceIndex = 0): FileRow {
  const counts = { additions: 1, deletions: 1, finding_count: 0, comment_count: 0, question_count: 0 };
  const flags = { reviewed: false, compacted: false, is_lazy_stub: false };
  const risk = { risk: null, risk_tier: "ai", risk_rule: null };
  const keys = { source_index: sourceIndex, cache_key: "k", preview_key: "" };
  return { path, status: "modified", old_path: null, similarity: null, kind: "production", ...counts, ...flags, ...risk, ...keys };
}

const zero = { files: 0, additions: 0, deletions: 0 };

function tabDiff(overrides: Partial<TabDiff> = {}): TabDiff {
  return {
    mode: "branch",
    branch: "feature",
    base: "main",
    files: [fileRow("src/a.rs"), fileRow("src/b.rs", 1)],
    total_count: 2,
    diff_stats: { total: zero, code: zero },
    filter: null,
    filter_suggestions: [],
    preview_context_key: "ctx",
    review_bucket_er_dir: "/er/bucket",
    local_branch: null,
    local_branch_checked_out: false,
    unstaged_stat: { files: 3, additions: 4, deletions: 5 },
    staged_stat: zero,
    built_from_head_oid: null,
    pr_diff_head_oid: null,
    base_behind: null,
    local_branch_divergence: null,
    load_error: null,
    commits: [],
    selected_commit_sha: null,
    ...overrides,
  };
}

function file(path: string): FileSnapshot {
  return { ...fileRow(path), hunks: [], delta_key: "", hunks_omitted: false } as unknown as FileSnapshot;
}

/** The App's slice: a History view's selected commit. */
function snapshot(overrides: Partial<DiffSlice> = {}): DiffSlice {
  const files = [file("history/commit.rs")];
  return { ...tabDiff({ total_count: 1 }), files, mode: "history", ...overrides } as unknown as DiffSlice;
}

/** A backend whose `er_fetch` answers each diff key from `answers`, in order. */
function backend(answers: { version: number; value: Resource }[]) {
  const calls: string[] = [];
  const sent: unknown[] = [];
  const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
    calls.push(command);
    if (command === "er_fetch") {
      const keys = (args.keys ?? []) as ResourceKey[];
      const answer = answers.length > 1 ? answers.shift() : answers[0];
      return keys.map((key) => ({ key, ...answer })) as T;
    }
    if (command === "er_send") {
      sent.push(args.command);
      return { changed: [], created: null } as T;
    }
    throw Object.assign(new Error(`unexpected ${command}`), { kind: "unsupported" });
  };
  return { calls, sent, store: new ResourceStore(resourceClient(run, async () => () => {}), 60_000) };
}

const KEY = 7;

function adapter(
  store: ResourceStore,
  opts: { flagOn?: boolean; snap?: DiffSlice | null; head?: string; hunks?: HunkSource } = {},
) {
  return new DiffAdapter({
    hunks: opts.hunks,
    flagOn: opts.flagOn ?? true,
    store,
    snapshot: () => opts.snap ?? null,
    activeKey: () => KEY,
    keyAt: (idx) => (idx === 0 ? KEY : 99),
    prHead: () => (opts.head ? { head_oid: opts.head, head_ref: "feature", base_ref: "main" } : null),
  });
}

describe("DiffAdapter", () => {
  it("reads the snapshot with the flag off and never fetches", async () => {
    const { calls, store } = backend([{ version: 1, value: { kind: "diff", ...tabDiff() } }]);
    const diff = adapter(store, { flagOn: false, snap: snapshot({ mode: "branch" }) });
    await settle();
    expect(diff.files.map((f) => f.path)).toEqual(["history/commit.rs"]);
    expect(calls).toEqual([]);
  });

  it("reads the file list, counts and scope stats from diff(tab) with the flag on", async () => {
    const { store } = backend([{ version: 4, value: { kind: "diff", ...tabDiff() } }]);
    store.show(diffKey(KEY));
    await settle();
    const view = adapter(store, { snap: snapshot({ mode: "branch" }) }).read();
    expect(view.files.map((f) => [f.path, f.source_index])).toEqual([
      ["src/a.rs", 0],
      ["src/b.rs", 1],
    ]);
    expect([view.mode, view.base, view.total_count, view.unstaged_stat.files]).toEqual(["branch", "main", 2, 3]);
    expect(view.review_bucket_er_dir).toBe("/er/bucket");
  });

  it("reads History's commit files, commits and selection from diff(tab), and sends its commands", async () => {
    const commits = [{ sha: "c2", title: "t", author: "a", committed_at: "", additions: 1, deletions: null, unpushed: true }];
    const history = tabDiff({ mode: "history", files: [fileRow("src/c.rs")], commits, selected_commit_sha: "c2" });
    const { sent, store } = backend([{ version: 5, value: { kind: "diff", ...history } }]);
    store.show(diffKey(KEY));
    await settle();
    const diff = adapter(store, { snap: snapshot() });
    const view = diff.read();
    expect(view.mode).toBe("history");
    expect(view.files.map((f) => f.path)).toEqual(["src/c.rs"]);
    expect([view.commits, view.selected_commit_sha]).toEqual([commits, "c2"]);
    await diff.route("select_commit", { sha: "c1" });
    await diff.route("change_base", { branch: "dev" });
    await diff.route("exit_history");
    expect(sent).toEqual([
      { kind: "select_commit", tab: KEY, sha: "c1" },
      { kind: "change_base", tab: KEY, base: "dev" },
      { kind: "exit_history", tab: KEY },
    ]);
  });

  it("refetches the version-0 empty diff when the first push for it arrives", async () => {
    const { store } = backend([
      { version: 0, value: { kind: "diff", ...tabDiff({ files: [], total_count: 0 }) } },
      { version: 3, value: { kind: "diff", ...tabDiff() } },
    ]);
    store.show(diffKey(KEY));
    await settle();
    const diff = adapter(store);
    expect(diff.files).toEqual([]);
    store.applyChanges([{ key: diffKey(KEY), version: 3 }]);
    await settle();
    expect(diff.files.map((f) => f.path)).toEqual(["src/a.rs", "src/b.rs"]);
  });

  it("marks a PR diff stale when the PR list's head moves past the head it was built from", async () => {
    const { store } = backend([{ version: 2, value: { kind: "diff", ...tabDiff({ built_from_head_oid: "h1" }) } }]);
    store.show(diffKey(KEY));
    await settle();
    expect(adapter(store, { head: "h1" }).read().diff_stale).toBeUndefined();
    const moved = adapter(store, { head: "h2" }).read().diff_stale;
    expect([moved?.kind, moved?.built_from_oid, moved?.latest_oid]).toEqual(["pr_head", "h1", "h2"]);
    expect(prStale(null, "h2")).toBeUndefined();
  });

  it("marks a branch view stale from diff(tab) once origin's tip of its base moves, never from the snapshot", async () => {
    const behind = { built_from_oid: "b1", latest_oid: "b2", commits_behind: 3 };
    const { store } = backend([{ version: 2, value: { kind: "diff", ...tabDiff({ base: "origin/main", base_behind: behind }) } }]);
    store.show(diffKey(KEY));
    await settle();
    expect(adapter(store).read().diff_stale).toEqual({
      kind: "base",
      message: "main has new commits on origin — Sync to refresh",
      built_from_oid: "b1",
      latest_oid: "b2",
      commits_behind: 3,
    });
    const { store: fresh } = backend([{ version: 2, value: { kind: "diff", ...tabDiff() } }]);
    fresh.show(diffKey(KEY));
    await settle();
    const snapStale = { kind: "base", message: "", built_from_oid: "x", latest_oid: "y" } as const;
    expect(adapter(fresh, { snap: snapshot({ mode: "branch", diff_stale: snapStale }) }).read().diff_stale).toBeUndefined();
  });

  it("reads the local branch's divergence from diff(tab), never from the snapshot", async () => {
    const divergence = { pr_head_oid: "p1", ahead: 2, unstaged_files: 1 };
    const { store } = backend([{ version: 2, value: { kind: "diff", ...tabDiff({ local_branch_divergence: divergence }) } }]);
    store.show(diffKey(KEY));
    await settle();
    const fromSnapshot = { pr_head_oid: "old", ahead: 9, unstaged_files: 9 };
    const view = adapter(store, { snap: snapshot({ mode: "branch", local_branch_divergence: fromSnapshot }) }).read();
    expect(view.local_branch_divergence).toEqual(divergence);
  });

  it("answers a failed load as its error with no rows", async () => {
    const failed = tabDiff({ files: [], total_count: 0, load_error: "not a repository" });
    const { store } = backend([{ version: 2, value: { kind: "diff", ...failed } }]);
    store.show(diffKey(KEY));
    await settle();
    const diff = adapter(store);
    expect(diff.loadError).toBe("not a repository");
    expect(diff.files).toEqual([]);
  });

  it("sends the diff commands on the active tab, every view included", async () => {
    const { sent, store } = backend([{ version: 1, value: { kind: "diff", ...tabDiff() } }]);
    const diff = adapter(store);
    await diff.route("set_mode", { mode: "history" });
    expect(diff.route("next_file", {})).toBeNull();
    await diff.route("set_mode", { mode: "pr_diff", prNumber: 4 });
    await diff.route("set_filter", { query: "*.rs" });
    await diff.route("clear_filter");
    await diff.route("force_refresh_diff");
    // A click painted on another tab does nothing.
    await diff.route("set_mode", { mode: "staged", tabIdx: 1 });
    expect(sent).toEqual([
      { kind: "set_diff_mode", tab: KEY, mode: "history", prNumber: null },
      { kind: "set_diff_mode", tab: KEY, mode: "pr_diff", prNumber: 4 },
      { kind: "set_filter", tab: KEY, query: "*.rs" },
      { kind: "set_filter", tab: KEY, query: "" },
      { kind: "refresh_diff", tab: KEY, sync: true },
    ]);
    expect(adapter(store, { flagOn: false }).route("refresh_diff")).toBeNull();
  });

  it("maps each of the ten commands", () => {
    const cases: [string, Record<string, unknown>, string][] = [
      ["set_mode", { mode: "staged" }, "set_diff_mode"],
      ["change_base", { branch: "dev" }, "change_base"],
      ["set_filter", { query: "*.rs" }, "set_filter"],
      ["clear_filter", {}, "set_filter"],
      ["refresh_diff", {}, "refresh_diff"],
      ["force_refresh_diff", {}, "refresh_diff"],
      ["sync_pr", { projectId: "p", prNumber: 3 }, "sync_pr"],
      ["sync_branch", { projectId: "p", branch: "b" }, "sync_branch"],
      ["select_commit", { sha: "c1" }, "select_commit"],
      ["exit_history", {}, "exit_history"],
    ];
    for (const [command, args, kind] of cases) expect(wireDiffCommand(command, args, KEY)?.kind).toBe(kind);
  });
});

describe("the stale marks with the flag on", () => {
  it("reads no snapshot stale mark or divergence", () => {
    const source = readFileSync(join(dirname(fileURLToPath(import.meta.url)), "diffAdapter.ts"), "utf8");
    const start = source.indexOf("export function sliceFromResource(");
    const body = source.slice(start, source.indexOf("\n}\n", start));
    expect(start).toBeGreaterThan(-1);
    expect(body).not.toMatch(/snapshot|snap\b/);
    expect(source).not.toMatch(/snap(shot)?\??\.(diff_stale|local_branch_divergence)/);
  });
});

describe("tabIsStale", () => {
  it("marks a PR tab, active or not, once its PR list's head moves past the head its diff was built from", async () => {
    let head = "h1";
    const tab = { key: KEY, pr_number: 9, remote: "acme/w" };
    const run: Invoke = async <T,>(command: string, args: Record<string, unknown>): Promise<T> => {
      const keys = (args.keys ?? []) as ResourceKey[];
      if (command !== "er_fetch") throw new Error(command);
      return keys.map((key) =>
        key.kind === "prs"
          ? { key, version: head === "h1" ? 1 : 2, value: { kind: "prs", rows: [{ number: 9, headOid: head }], fetchedAtMs: null, projects: [] } }
          : { key, version: 1, value: { kind: "diff", ...tabDiff({ built_from_head_oid: "h1" }) } },
      ) as T;
    };
    const store = new ResourceStore(resourceClient(run, async () => () => {}), 60_000);
    store.show(diffKey(KEY));
    store.show({ kind: "prs", remote: "acme/w" });
    await settle();
    expect(tabIsStale(store, tab, false)).toBe(false);
    head = "h2";
    store.applyChanges([{ key: { kind: "prs", remote: "acme/w" }, version: 2 }]);
    await settle();
    expect(tabIsStale(store, tab, false)).toBe(true);
    expect(tabIsStale(store, tab, true)).toBe(true);
  });

  it("marks only the active branch view, once its diff(tab) is behind origin's base", async () => {
    const tab = { key: KEY, pr_number: null, remote: null };
    const behind = { built_from_oid: "b1", latest_oid: "b2", commits_behind: 1 };
    const { store } = backend([{ version: 1, value: { kind: "diff", ...tabDiff({ base_behind: behind }) } }]);
    const { store: fresh } = backend([{ version: 1, value: { kind: "diff", ...tabDiff() } }]);
    store.show(diffKey(KEY));
    fresh.show(diffKey(KEY));
    await settle();
    expect(tabIsStale(store, tab, true)).toBe(true);
    expect(tabIsStale(store, tab, false)).toBe(false);
    expect(tabIsStale(fresh, tab, true)).toBe(false);
  });
});

describe("the hunks by hash", () => {
  const HUNKS: Hunk[] = [{ header: "@@ -1,2 +1,2 @@", old_start: 1, old_count: 2, new_start: 1, new_count: 2, lines: [] }];
  const root = { id: "q1", author: "You", kind: "you" as const, timestamp: "", body_markdown: "why?" };
  const question: ThreadSnapshot = {
    id: "q1", kind: "question", file: "src/a.rs", line: 1, side: "RIGHT", source: "local",
    stale: false, resolved: false, root, replies: [], promoted_to: null,
  };
  /** The App's snapshot: its own hunks for a.rs, which the page no longer draws. */
  const raw = { files: [{ ...file("src/a.rs"), hunks: [{ ...HUNKS[0], header: "@@ app @@", threads: [] }] }], ai: { threads: [question] } } as unknown as AppSnapshot;
  const rows = [{ ...fileRow("src/a.rs"), cache_key: "ha" }, { ...fileRow("src/b.rs", 1), cache_key: "hb" }];
  const source = (held: Record<string, Hunk[]>): HunkSource => ({ get: (hash) => held[hash], threads: () => [question] });

  it("draws each file's hunks by the hash its row names, with the threads joined on, and lists a missing one as a stub", async () => {
    const { store } = backend([{ version: 4, value: { kind: "diff", ...tabDiff({ files: rows }) } }]);
    store.show(diffKey(KEY));
    await settle();
    const shown = adapter(store, { hunks: source({ ha: HUNKS }) }).overlay(raw);
    const [a, b] = shown.files;
    expect([a.path, a.is_lazy_stub, a.hunks.map((h) => h.header), a.hunks[0].threads.map((t) => t.id)]).toEqual(["src/a.rs", false, ["@@ -1,2 +1,2 @@"], ["q1"]]);
    expect([b.path, b.is_lazy_stub, b.hunks]).toEqual(["src/b.rs", true, []]);
  });

  it("draws a History commit's hunks by hash too", async () => {
    const { store } = backend([{ version: 4, value: { kind: "diff", ...tabDiff({ mode: "history", files: rows }) } }]);
    store.show(diffKey(KEY));
    await settle();
    const diff = adapter(store, { hunks: source({ ha: HUNKS }) });
    const [a] = diff.overlay(raw).files;
    expect([a.path, a.hunks.map((h) => h.header)]).toEqual(["src/a.rs", ["@@ -1,2 +1,2 @@"]]);
    expect(diff.hashed()).toBe(true);
  });

  it("sends the compaction toggle with its file and direction, in every view", async () => {
    const { sent, store } = backend([{ version: 1, value: { kind: "diff", ...tabDiff() } }]);
    await adapter(store).route("toggle_compacted", { path: "Cargo.lock", compacted: false });
    expect(sent).toEqual([{ kind: "set_file_compacted", tab: KEY, path: "Cargo.lock", compacted: false }]);
    expect(wireDiffCommand("toggle_compacted", {}, KEY)).toBeUndefined();
    const hidden = backend([{ version: 2, value: { kind: "diff", ...tabDiff({ mode: "hidden" }) } }]);
    hidden.store.show(diffKey(KEY));
    await settle();
    await adapter(hidden.store).route("toggle_compacted", { path: "Cargo.lock", compacted: true });
    expect(hidden.sent).toEqual([{ kind: "set_file_compacted", tab: KEY, path: "Cargo.lock", compacted: true }]);
  });
});
