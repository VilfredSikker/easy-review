import { describe, expect, it } from "bun:test";
import {
  DONE_WINDOW_MS,
  activeJobCount,
  aggregateJobs,
  jobsForTab,
} from "./runningJobs";
import type { ArenaRunSummary } from "./types/arena";
import type { BackgroundTaskSnapshot, TabSummary } from "./types";

const NOW = 1_000_000;

const tab = (p: Partial<TabSummary>): TabSummary => ({
  idx: 0,
  label: "t",
  kind: "local_branch",
  branch: "main",
  pr_number: null,
  remote: null,
  repo_root: "/r",
  is_active: false,
  change_token: "",
  ...p,
});

const task = (p: Partial<BackgroundTaskSnapshot>): BackgroundTaskSnapshot => ({
  id: "1",
  kind: "review",
  label: "Review",
  target_label: "",
  scope: "branch",
  repo_root: "/r",
  branch_label: "main",
  status: "running",
  started_at_ms: NOW - 1000,
  ...p,
});

const run = (p: Partial<ArenaRunSummary>): ArenaRunSummary => ({
  id: "a1",
  branch_ref: "main",
  status: "queued",
  created_at: new Date(NOW - 5000).toISOString(),
  reviewer_count: 2,
  finding_count: 0,
  ...p,
});

describe("aggregateJobs", () => {
  const tabs = [
    tab({ idx: 0, branch: "main" }),
    tab({ idx: 1, branch: "feat", pr_number: 7, remote: "o/r", kind: "remote_pr" }),
  ];

  it("merges the three sources", () => {
    const jobs = aggregateJobs({
      backgroundTasks: [task({})],
      agentCommands: [{ name: "triage", status: "running" }],
      arenaRuns: [run({ status: { running: { round: 1 } } })],
      tabs,
      activeTabIdx: 0,
      nowMs: NOW,
    });
    expect(jobs.map((j) => j.source).sort()).toEqual(["agent_command", "arena", "background_task"]);
    expect(activeJobCount(jobs)).toBe(3);
  });

  it("keeps finished jobs only inside the done window", () => {
    const fresh = task({ id: "f", status: "done", finished_at_ms: NOW - (DONE_WINDOW_MS - 1) });
    const old = task({ id: "o", status: "done", finished_at_ms: NOW - DONE_WINDOW_MS });
    const jobs = aggregateJobs({ backgroundTasks: [fresh, old], tabs, activeTabIdx: 0, nowMs: NOW });
    expect(jobs.map((j) => j.id)).toEqual(["task:f"]);
  });

  it("uses the recorded finish time for agent commands, else drops them", () => {
    const base = { agentCommands: [{ name: "c", status: "done" }], tabs, activeTabIdx: 0, nowMs: NOW };
    expect(aggregateJobs(base)).toHaveLength(0);
    expect(
      aggregateJobs({ ...base, commandDoneAtMs: new Map([["c", NOW - 100]]) }),
    ).toHaveLength(1);
  });

  it("maps tasks to tabs by PR, then branch, then repo", () => {
    const jobs = aggregateJobs({
      backgroundTasks: [
        task({ id: "pr", pr_number: 7, remote_repo: "O/R" }),
        task({ id: "br", branch_label: "main" }),
        task({ id: "repo", branch_label: "zzz" }),
        task({ id: "none", repo_root: "/elsewhere" }),
      ],
      tabs,
      activeTabIdx: 0,
      nowMs: NOW,
    });
    const idx = Object.fromEntries(jobs.map((j) => [j.id, j.tabIdx]));
    expect(idx).toEqual({ "task:pr": 1, "task:br": 0, "task:repo": 0, "task:none": null });
  });

  it("maps arena runs by branch and attributes commands to the active tab", () => {
    const jobs = aggregateJobs({
      agentCommands: [{ name: "x", status: "running" }],
      arenaRuns: [run({ branch_ref: "feat" })],
      tabs,
      activeTabIdx: 0,
      nowMs: NOW,
    });
    expect(jobsForTab(jobs, 1).map((j) => j.source)).toEqual(["arena"]);
    expect(jobsForTab(jobs, 0).map((j) => j.source)).toEqual(["agent_command"]);
  });

  it("classifies arena terminal states", () => {
    const done = aggregateJobs({
      arenaRuns: [
        run({ id: "c", status: "complete", completed_at: new Date(NOW - 10).toISOString() }),
        run({ id: "x", status: "cancelled", completed_at: new Date(NOW - 10).toISOString() }),
      ],
      tabs,
      activeTabIdx: 0,
      nowMs: NOW,
    });
    expect(done.map((j) => j.status).sort()).toEqual(["done", "failed"]);
    expect(activeJobCount(done)).toBe(0);
  });
});
