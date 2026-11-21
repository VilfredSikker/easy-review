import { describe, expect, it } from "bun:test";
import { resourceClient, type Invoke } from "$lib/protocol/client";
import type { FileRow } from "$lib/protocol/generated/FileRow";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { TabDiff } from "$lib/protocol/generated/TabDiff";
import type { FileSnapshot } from "$lib/types";
import { DiffAdapter, diffKey, prStale, tabIsStale, wireDiffCommand, type DiffSlice } from "./diffAdapter";
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
    load_error: null,
    ...overrides,
  };
}

function file(path: string): FileSnapshot {
  return { ...fileRow(path), hunks: [], delta_key: "", hunks_omitted: false } as unknown as FileSnapshot;
}

/** The App's slice: a History view's selected commit, which only the snapshot carries. */
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

function adapter(store: ResourceStore, opts: { flagOn?: boolean; snap?: DiffSlice | null; head?: string } = {}) {
  return new DiffAdapter({
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

  it("shows the App's slice in a view the workspace only records", async () => {
    // The tab switched to History: the content records it, and the App loads
    // the selected commit's files, which only the snapshot carries.
    const { store } = backend([{ version: 5, value: { kind: "diff", ...tabDiff({ mode: "history" }) } }]);
    store.show(diffKey(KEY));
    await settle();
    const view = adapter(store, { snap: snapshot() }).read();
    expect(view.mode).toBe("history");
    expect(view.files.map((f) => f.path)).toEqual(["history/commit.rs"]);
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

  it("answers a failed load as its error with no rows", async () => {
    const failed = tabDiff({ files: [], total_count: 0, load_error: "not a repository" });
    const { store } = backend([{ version: 2, value: { kind: "diff", ...failed } }]);
    store.show(diffKey(KEY));
    await settle();
    const diff = adapter(store);
    expect(diff.loadError).toBe("not a repository");
    expect(diff.files).toEqual([]);
  });

  it("sends the diff commands on the active tab, and leaves the views the App loads to their command", async () => {
    const { sent, store } = backend([{ version: 1, value: { kind: "diff", ...tabDiff() } }]);
    const diff = adapter(store);
    expect(diff.route("set_mode", { mode: "history" })).toBeNull();
    expect(diff.route("next_file", {})).toBeNull();
    await diff.route("set_mode", { mode: "pr_diff", prNumber: 4 });
    await diff.route("set_filter", { query: "*.rs" });
    await diff.route("clear_filter");
    await diff.route("force_refresh_diff");
    // A click painted on another tab does nothing.
    await diff.route("set_mode", { mode: "staged", tabIdx: 1 });
    expect(sent).toEqual([
      { kind: "set_diff_mode", tab: KEY, mode: "pr_diff", prNumber: 4 },
      { kind: "set_filter", tab: KEY, query: "*.rs" },
      { kind: "set_filter", tab: KEY, query: "" },
      { kind: "refresh_diff", tab: KEY, sync: true },
    ]);
    expect(adapter(store, { flagOn: false }).route("refresh_diff")).toBeNull();
  });

  it("maps each of the eight commands", () => {
    const wire = (command: string, args: Record<string, unknown> = {}) => wireDiffCommand(command, args, KEY)?.kind;
    expect([
      wire("set_mode", { mode: "staged" }),
      wire("change_base", { branch: "dev" }),
      wire("set_filter", { query: "*.rs" }),
      wire("clear_filter"),
      wire("refresh_diff"),
      wire("force_refresh_diff"),
      wire("sync_pr", { projectId: "p", prNumber: 3 }),
      wire("sync_branch", { projectId: "p", branch: "b" }),
    ]).toEqual([
      "set_diff_mode",
      "change_base",
      "set_filter",
      "set_filter",
      "refresh_diff",
      "refresh_diff",
      "sync_pr",
      "sync_branch",
    ]);
  });
});

describe("tabIsStale", () => {
  it("marks an inactive PR tab once its PR list's head moves past the head its diff was built from", async () => {
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
    expect(tabIsStale(store, tab, true)).toBe(false);
  });
});
