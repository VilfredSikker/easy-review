import { describe, expect, it } from "vitest";
import {
  canChromeMerge,
  canChromeMergeTakingNextAi,
  isStaleSnapshotGeneration,
  mergeChromeSnapshot,
  snapshotViewIdentity,
} from "./snapshotChrome";
import type { AiSnapshot, AppSnapshot, TabSummary } from "./types";

function emptyAi(overrides: Partial<AiSnapshot> = {}): AiSnapshot {
  return {
    fresh: true,
    stale_reason: null,
    summary_markdown: null,
    agent_summaries: {},
    high: 0,
    med: 0,
    low: 0,
    local_comment_count: 0,
    github_comment_count: 0,
    comments: 0,
    questions: 0,
    notes: 0,
    unpushed: 0,
    threads: [],
    findings: [],
    has_review_json: false,
    eligible_comment_count: 0,
    triage: null,
    ...overrides,
  };
}

function tab(partial: Partial<TabSummary> & Pick<TabSummary, "idx" | "label">): TabSummary {
  return {
    kind: "remote_pr",
    branch: null,
    pr_number: null,
    repo_root: "/repo",
    is_active: false,
    change_token: "t",
    ...partial,
  };
}

function snap(partial: Partial<AppSnapshot> & { active_tab: number; tabs: TabSummary[] }): AppSnapshot {
  return {
    mode: "pr",
    branch: "main",
    base: "main",
    input_mode: "normal",
    files: [],
    selected_file: 0,
    current_hunk: null,
    filter: null,
    reviewed_count: 0,
    total_count: 0,
    ai: emptyAi(),
    pr: null,
    panels: { left: true, tree: true, right: true },
    theme: "graphite",
    watch_active: false,
    watch_status: {
      active: false,
      branch: null,
      root_path: null,
    },
    worktrees: [],
    projects: [],
    local_branch: null,
    notification: null,
    bg_loading: {
      pr_list: false,
      gh_status: false,
      gh_comments: false,
      tab_diff: false,
    },
    ...partial,
  };
}

const pr1427 = {
  number: 1427,
  title: "a",
  state: "OPEN",
  base: "main",
  head: "feat",
  url: "",
  author: "",
} as const;

const pr1434 = {
  number: 1434,
  title: "b",
  state: "OPEN",
  base: "main",
  head: "feat",
  url: "",
  author: "",
} as const;

describe("snapshotViewIdentity", () => {
  it("changes when PR number changes", () => {
    const a = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1427", pr_number: 1427, is_active: true })],
      pr: { ...pr1427 },
    });
    const b = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      pr: { ...pr1434 },
    });
    expect(snapshotViewIdentity(a)).not.toBe(snapshotViewIdentity(b));
  });
});

describe("isStaleSnapshotGeneration", () => {
  it("discards polls captured before ingestCommandSnapshot", () => {
    expect(isStaleSnapshotGeneration(1, 2)).toBe(true);
    expect(isStaleSnapshotGeneration(2, 2)).toBe(false);
  });
});

describe("canChromeMerge", () => {
  it("allows merge when identity matches and chrome_only", () => {
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      pr: { ...pr1434 },
      ai: emptyAi({ high: 3 }),
    });
    const next = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      pr: { ...pr1434 },
      ai: emptyAi(),
    });
    expect(
      canChromeMerge(prev, next, { chromeOnly: true, contentChanged: false }),
    ).toBe(true);
    expect(
      canChromeMergeTakingNextAi(prev, next, {
        chromeOnly: true,
        contentChanged: false,
      }),
    ).toBe(false);
  });

  it("rejects same-ai merge when PR identity differs", () => {
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1427", pr_number: 1427, is_active: true })],
      pr: { ...pr1427 },
      ai: emptyAi({ high: 7 }),
    });
    const next = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      pr: { ...pr1434 },
      ai: emptyAi(),
    });
    expect(
      canChromeMerge(prev, next, { chromeOnly: true, contentChanged: false }),
    ).toBe(false);
    expect(
      canChromeMergeTakingNextAi(prev, next, {
        chromeOnly: true,
        contentChanged: false,
      }),
    ).toBe(true);
  });
});

describe("mergeChromeSnapshot", () => {
  it("keeps previous ai when aiSource is prev", () => {
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      ai: emptyAi({ high: 3, summary_markdown: "keep me" }),
    });
    const next = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      ai: emptyAi(),
      notification: "sidebar updated",
    });
    const merged = mergeChromeSnapshot(prev, next, "prev");
    expect(merged.ai.high).toBe(3);
    expect(merged.ai.summary_markdown).toBe("keep me");
    expect(merged.notification).toBe("sidebar updated");
  });

  it("takes next.ai and next.pr when identity crosses PRs", () => {
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1427", pr_number: 1427, is_active: true })],
      pr: { ...pr1427 },
      ai: emptyAi({ high: 7, summary_markdown: "old pr" }),
    });
    const next = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      pr: { ...pr1434 },
      ai: emptyAi({ high: 0, fresh: true }),
      notification: "opened 1434",
    });
    const merged = mergeChromeSnapshot(prev, next, "next");
    expect(merged.ai.high).toBe(0);
    expect(merged.ai.summary_markdown).toBeNull();
    expect(merged.pr?.number).toBe(1434);
    expect(merged.notification).toBe("opened 1434");
  });
});
