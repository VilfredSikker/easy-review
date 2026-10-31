import { describe, expect, it } from "bun:test";
import type { AppSnapshot, BackgroundTaskSnapshot, CheckSummary, ThreadSnapshot, TourSnapshot } from "$lib/types";
import { githubStatusOpen, richSnapshot, tabsWorkingActive } from "$lib/stories/fixtures";
import {
  asideFileGroups,
  checkRows,
  formatDuration,
  generatorStates,
  reviewFlowCommand,
  langTag,
  latestArenaRun,
  overviewHeader,
  prStatus,
} from "./overviewModel";
import { threadsByAuthor } from "$lib/components/rpanel/commentsAuthor";

const onPr: AppSnapshot = {
  ...richSnapshot,
  pr: null,
  github: githubStatusOpen,
  tabs: [{ ...tabsWorkingActive[0], pr_number: githubStatusOpen.number }],
};

describe("overviewHeader", () => {
  it("reads a PR's title, author, refs, commits and opening time from GitHub", () => {
    expect(overviewHeader(onPr)).toEqual({
      title: githubStatusOpen.title,
      prNumber: 123,
      author: "sune",
      base: "main",
      head: "show-experiment-params",
      commits: 2,
      openedAt: githubStatusOpen.created_at ?? null,
    });
  });

  it("leaves out what only a PR has on a local branch", () => {
    const local: AppSnapshot = {
      ...richSnapshot,
      pr: null,
      github: null,
      tabs: [{ ...tabsWorkingActive[0], pr_number: null }],
    };
    const h = overviewHeader(local);
    expect(h.prNumber).toBeNull();
    expect(h.author).toBeNull();
    expect(h.openedAt).toBeNull();
    expect(h.title).toBe("show-experiment-params");
    expect(h.commits).toBe(richSnapshot.commits?.length ?? -1);
  });

  it("is empty without a snapshot", () => {
    expect(overviewHeader(null).title).toBe("");
  });
});

describe("prStatus", () => {
  it("names the state, then the merge box's verdict", () => {
    const clean = { ...githubStatusOpen, merge_state_status: "CLEAN", review_decision: "APPROVED" };
    expect(prStatus(clean)).toEqual({ label: "Open · ready to merge", tone: "ok" });
  });

  it("says Draft, Merged and Closed on their own", () => {
    expect(prStatus({ ...githubStatusOpen, is_draft: true }).label).toBe("Draft");
    expect(prStatus({ ...githubStatusOpen, state: "MERGED" }).label).toBe("Merged");
    expect(prStatus({ ...githubStatusOpen, state: "CLOSED" }).label).toBe("Closed");
  });
});

describe("checks", () => {
  it("formats durations at a glance's precision", () => {
    expect(formatDuration(41)).toBe("41s");
    expect(formatDuration(112)).toBe("1m 52s");
    expect(formatDuration(128)).toBe("2m 08s");
    expect(formatDuration(3780)).toBe("1h 3m");
  });

  it("puts failures first and shows duration, status text or pending", () => {
    const check = (over: Partial<CheckSummary>): CheckSummary => ({
      name: "c",
      status: "COMPLETED",
      conclusion: "SUCCESS",
      url: null,
      ...over,
    });
    const rows = checkRows([
      check({ name: "test", duration_secs: 112 }),
      check({ name: "preview", description: "Ready" }),
      check({ name: "cleanup", status: "PENDING", conclusion: "" }),
      check({ name: "lint", conclusion: "fail", duration_secs: 9 }),
    ]);
    expect(rows.map((r) => [r.name, r.state, r.detail])).toEqual([
      ["lint", "fail", "9s"],
      ["cleanup", "pend", "pending"],
      ["test", "ok", "1m 52s"],
      ["preview", "ok", "Ready"],
    ]);
  });
});

describe("reviewFlowCommand", () => {
  const base = { review_bucket_er_dir: "/er", review_flow: null } as Pick<AppSnapshot, "review_flow" | "review_bucket_er_dir">;
  const flow = (state: string) => ({ ...base, review_flow: { state, draining: false } as unknown as AppSnapshot["review_flow"] });

  it("starts the review flow, the same review the palette runs", () => {
    expect(reviewFlowCommand(base, "branch")).toEqual({ command: "start_review_flow", args: { scope: "branch", erDir: "/er" } });
  });
  it("confirms a Deep estimate that is waiting", () => {
    expect(reviewFlowCommand(flow("awaiting_confirmation"), "branch")).toEqual({ command: "confirm_review_flow", args: { erDir: "/er" } });
  });
  it("does nothing while a flow runs or with no scope", () => {
    expect(reviewFlowCommand(flow("finding"), "branch")).toBeNull();
    expect(reviewFlowCommand(base, null)).toBeNull();
  });
});

describe("generatorStates", () => {
  const task = (over: Partial<BackgroundTaskSnapshot>): BackgroundTaskSnapshot => ({
    id: "t",
    kind: "triage",
    label: "",
    target_label: "",
    scope: "branch",
    repo_root: tabsWorkingActive[0].repo_root,
    branch_label: richSnapshot.branch,
    status: "running",
    started_at_ms: 0,
    ...over,
  });

  it("spins only for a run on this tab's branch", () => {
    const snap: AppSnapshot = {
      ...richSnapshot,
      background_tasks: [task({ kind: "expert:security" }), task({ kind: "tour", branch_label: "other" })],
    };
    const busy = Object.fromEntries(generatorStates(snap).map((g) => [g.id, g.busy]));
    expect(busy).toEqual({ triage: false, review: true, tour: false });
  });

  it("dates each artifact, and a tour only when there is one", () => {
    const tour = { available: false, createdAt: "2026-10-30T09:00:00Z" } as TourSnapshot;
    const snap: AppSnapshot = {
      ...richSnapshot,
      ai: { ...richSnapshot.ai, review_created_at: "2026-10-30T08:00:00Z", triage: null },
      tour,
    };
    expect(generatorStates(snap).map((g) => g.at)).toEqual([null, "2026-10-30T08:00:00Z", null]);
  });
});

describe("latestArenaRun", () => {
  it("counts runs and dates the newest", () => {
    const run = (created_at: string) =>
      ({ id: created_at, branch_ref: "b", status: "done", created_at, reviewer_count: 2, finding_count: 0 }) as const;
    expect(
      latestArenaRun({ arena_runs: [run("2026-10-30T08:00:00Z"), run("2026-10-30T09:00:00Z")] as never }),
    ).toEqual({ count: 2, at: "2026-10-30T09:00:00Z" });
    expect(latestArenaRun({ arena_runs: [] })).toBeNull();
  });
});

describe("asideFileGroups", () => {
  it("is one untitled group without a tour", () => {
    const groups = asideFileGroups({ files: richSnapshot.files, tour: undefined });
    expect(groups).toHaveLength(1);
    expect(groups[0].title).toBeNull();
    expect(groups[0].files).toHaveLength(richSnapshot.files.length);
  });

  it("groups by pillar with per-pillar totals, and drops empty pillars", () => {
    const [a, b] = richSnapshot.files;
    const tour = {
      available: true,
      pillars: [
        { id: "p1", title: "First", files: [{ path: a.path, reason: "", findingIds: [] }] },
        { id: "p2", title: "Empty", files: [] },
      ],
    } as unknown as TourSnapshot;
    const groups = asideFileGroups({ files: [a, b], tour });
    expect(groups.map((g) => [g.title, g.additions, g.files.length])).toEqual([
      ["First", a.additions, 1],
      ["Other files", b.additions, 1],
    ]);
  });

  it("tags files by extension", () => {
    expect(langTag("db/schema.sql")).toBe("SQL");
    expect(langTag("src/App.svelte")).toBe("SVEL");
    expect(langTag("Makefile")).toBe("");
    expect(langTag(".env")).toBe("");
  });
});

describe("threadsByAuthor", () => {
  const msg = (author: string, kind: "human" | "ai" = "human") => ({
    id: author,
    author,
    kind,
    timestamp: "",
    body_markdown: "",
  });
  const thread = (id: string, kind: ThreadSnapshot["kind"], root: string, replies: ReturnType<typeof msg>[] = []) => ({
    id,
    kind,
    root: msg(root),
    replies,
  });
  const threads = [
    thread("a", "comment", "martin"),
    thread("b", "comment", "sune", [msg("martin")]),
    thread("c", "question", "martin"),
    thread("d", "comment", "sune", [msg("martin", "ai")]),
    thread("e", "comment", "copilot-pull-request-reviewer[bot]"),
  ];

  it("keeps GitHub threads the reviewer started or replied in", () => {
    expect(threadsByAuthor(threads, "martin").map((t) => t.id)).toEqual(["a", "b"]);
  });

  it("matches GitHub's several names for one account", () => {
    expect(threadsByAuthor(threads, "Copilot").map((t) => t.id)).toEqual(["e"]);
  });

  it("passes everything through without an author", () => {
    expect(threadsByAuthor(threads, null)).toHaveLength(threads.length);
  });
});
