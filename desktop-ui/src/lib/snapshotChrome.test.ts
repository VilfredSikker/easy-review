import { describe, expect, it } from "vitest";
import {
  canChromeMerge,
  canChromeMergeTakingNextAi,
  isStaleSnapshotGeneration,
  mergeChromeSnapshot,
  shouldDeferChromeIdentityChange,
  snapshotViewIdentity,
  snapshotViewParts,
} from "./snapshotChrome";
import type { AiSnapshot, AppSnapshot, TabSummary, UiAnnotation } from "./types";

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
    file_risks: [],
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
    remote: null,
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
  it("joins the same fields snapshotViewParts exposes", () => {
    const a = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1427", pr_number: 1427, is_active: true })],
      pr: { ...pr1427 },
    });
    const p = snapshotViewParts(a);
    expect(snapshotViewIdentity(a)).toBe(
      `${p.active_tab}|${p.repo_root}|${p.pr_number ?? ""}|${p.branch}|${p.mode}`,
    );
  });

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

  it("keys off the tab PR, not a mismatched github.number", () => {
    const a = snap({
      active_tab: 0,
      github: {
        owner: "o",
        repo: "r",
        number: 1473,
        url: "https://github.com/o/r/pull/1473",
        state: "OPEN",
        is_draft: false,
        title: "t",
        body: "",
        author: "u",
        head_ref: "h",
        base_ref: "main",
        review_decision: null,
        mergeable: null,
        labels: [],
        checks: [],
        comments_count: 0,
        reviews_count: 0,
        recent_comments: [],
        recent_reviews: [],
        last_updated: null,
        is_authored_by_me: false,
      },
      tabs: [tab({ idx: 0, label: "pr-1469", pr_number: 1469, is_active: true })],
    });
    expect(snapshotViewIdentity(a)).toContain("|1469|");
    expect(snapshotViewIdentity(a)).not.toContain("|1473|");
  });

  it("does not change when github.number arrives on a tab with no PR", () => {
    const tabs = [tab({ idx: 0, label: "feat", kind: "working", pr_number: null, is_active: true })];
    const before = snap({ active_tab: 0, tabs, github: undefined });
    const after = snap({
      active_tab: 0,
      tabs,
      github: {
        owner: "o",
        repo: "r",
        number: 1473,
        url: "https://github.com/o/r/pull/1473",
        state: "OPEN",
        is_draft: false,
        title: "t",
        body: "",
        author: "u",
        head_ref: "h",
        base_ref: "main",
        review_decision: null,
        mergeable: null,
        labels: [],
        checks: [],
        comments_count: 0,
        reviews_count: 0,
        recent_comments: [],
        recent_reviews: [],
        last_updated: null,
        is_authored_by_me: false,
      },
    });
    expect(snapshotViewIdentity(before)).toBe(snapshotViewIdentity(after));
  });

  it("does not change when snapshot.branch fills in on a remote PR tab", () => {
    const tabs = [
      tab({ idx: 0, label: "discovery#1425", pr_number: 1425, is_active: true, branch: null }),
    ];
    const before = snap({ active_tab: 0, tabs, branch: "", base: "" });
    const after = snap({
      active_tab: 0,
      tabs,
      branch: "feat/from-fork",
      base: "main",
    });
    expect(snapshotViewIdentity(before)).toBe(snapshotViewIdentity(after));
    expect(
      canChromeMerge(before, after, { chromeOnly: true, contentChanged: false }),
    ).toBe(true);
    expect(
      shouldDeferChromeIdentityChange(before, after, {
        chromeOnly: true,
        contentChanged: false,
      }),
    ).toBe(false);
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
      notification: { message: "sidebar updated", seq: 1, long: false },
    });
    const merged = mergeChromeSnapshot(prev, next, "prev");
    expect(merged.ai.high).toBe(3);
    expect(merged.ai.summary_markdown).toBe("keep me");
    expect(merged.notification?.message).toBe("sidebar updated");
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
      notification: { message: "opened 1434", seq: 2, long: false },
    });
    const merged = mergeChromeSnapshot(prev, next, "next");
    expect(merged.ai.high).toBe(0);
    expect(merged.ai.summary_markdown).toBeNull();
    expect(merged.pr?.number).toBe(1434);
    expect(merged.notification?.message).toBe("opened 1434");
  });

  it("takes next annotations and browser when identity crosses PRs", () => {
    const prevAnn: UiAnnotation[] = [
      {
        id: "old",
        url: "https://old",
        selector: null,
        box_x: 0,
        box_y: 0,
        box_w: 1,
        box_h: 1,
        viewport_w: 1,
        viewport_h: 1,
        text: "old",
        timestamp: "",
        author: "",
        screenshot_path: null,
        stale: false,
      },
    ];
    const nextAnn: UiAnnotation[] = [];
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1427", pr_number: 1427, is_active: true })],
      ui_annotations: prevAnn,
      browser: { url: "https://old.example", layout: "split", split_ratio: 0.4, annotate_mode: false, show_tooltips: false },
    });
    const next = snap({
      active_tab: 1,
      tabs: [tab({ idx: 1, label: "pr-1434", pr_number: 1434, is_active: true })],
      ui_annotations: nextAnn,
      browser: { url: "https://new.example", layout: "hidden", split_ratio: 0.45, annotate_mode: false, show_tooltips: false },
    });
    const merged = mergeChromeSnapshot(prev, next, "next");
    expect(merged.ui_annotations).toBe(nextAnn);
    expect(merged.browser?.url).toBe("https://new.example");
  });

  it("keeps previous annotations/browser when aiSource is prev", () => {
    const prevAnn: UiAnnotation[] = [
      {
        id: "keep",
        url: "https://keep",
        selector: null,
        box_x: 0,
        box_y: 0,
        box_w: 1,
        box_h: 1,
        viewport_w: 1,
        viewport_h: 1,
        text: "keep",
        timestamp: "",
        author: "",
        screenshot_path: null,
        stale: false,
      },
    ];
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      ui_annotations: prevAnn,
      browser: { url: "https://old.example", layout: "split", split_ratio: 0.4, annotate_mode: false, show_tooltips: false },
    });
    const next = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1434", pr_number: 1434, is_active: true })],
      ui_annotations: [],
      browser: { url: "", layout: "hidden", split_ratio: 0.45, annotate_mode: false, show_tooltips: false },
    });
    const merged = mergeChromeSnapshot(prev, next, "prev");
    expect(merged.ui_annotations).toBe(prevAnn);
    expect(merged.browser?.url).toBe("https://old.example");
  });

  it("fills empty prev branch/base from next chrome", () => {
    const tabs = [tab({ idx: 0, label: "discovery#1425", pr_number: 1425, is_active: true })];
    const prev = snap({
      active_tab: 0,
      tabs,
      branch: "",
      base: "",
    });
    const next = snap({
      active_tab: 0,
      tabs,
      branch: "feat/from-fork",
      base: "main",
      notification: { message: "gh status landed", seq: 3, long: false },
    });
    const merged = mergeChromeSnapshot(prev, next, "prev");
    expect(merged.branch).toBe("feat/from-fork");
    expect(merged.base).toBe("main");
    expect(merged.notification?.message).toBe("gh status landed");
  });

  it("keeps a populated prev branch when next chrome is empty", () => {
    const tabs = [tab({ idx: 0, label: "feat", kind: "local_branch", is_active: true })];
    const prev = snap({
      active_tab: 0,
      tabs,
      branch: "feat/keep",
      base: "origin/main",
    });
    const next = snap({
      active_tab: 0,
      tabs,
      branch: "",
      base: "",
    });
    const merged = mergeChromeSnapshot(prev, next, "prev");
    expect(merged.branch).toBe("feat/keep");
    expect(merged.base).toBe("origin/main");
  });

  it("keeps per-file and per-commit fields from prev content", () => {
    const tabs = [tab({ idx: 0, label: "feat", kind: "working", is_active: true })];
    const prev = snap({
      active_tab: 0,
      tabs,
      files: [
        {
          path: "src/new.ts",
          status: "renamed",
          old_path: "src/old.ts",
          similarity: 92,
          kind: "test",
          additions: 1,
          deletions: 1,
          reviewed: false,
          compacted: false,
          risk: null,
          finding_count: 0,
          comment_count: 0,
          question_count: 0,
          hunks: [],
          source_index: 0,
          cache_key: "k",
        },
      ],
      commits: [
        {
          sha: "abc",
          title: "t",
          author: "a",
          committed_at: "2026-01-01T00:00:00Z",
          additions: 3,
          deletions: 1,
          unpushed: true,
        },
      ],
    });
    // A chrome-only stub carries no files or commits.
    const next = snap({ active_tab: 0, tabs, files: [], commits: [] });
    const merged = mergeChromeSnapshot(prev, next, "prev");
    expect(merged.files[0]).toMatchObject({ old_path: "src/old.ts", similarity: 92, kind: "test" });
    expect(merged.commits?.[0]).toMatchObject({ additions: 3, deletions: 1, unpushed: true });
  });

  it("keeps prev scope stats over a chrome-only stub's zeros", () => {
    const tabs = [tab({ idx: 0, label: "feat", kind: "working", is_active: true })];
    const prev = snap({
      active_tab: 0,
      tabs,
      unstaged_stat: { files: 2, additions: 10, deletions: 4 },
      staged_stat: { files: 1, additions: 3, deletions: 0 },
    });
    const next = snap({
      active_tab: 0,
      tabs,
      unstaged_stat: { files: 0, additions: 0, deletions: 0 },
      staged_stat: { files: 0, additions: 0, deletions: 0 },
    });
    const merged = mergeChromeSnapshot(prev, next, "prev");
    expect(merged.unstaged_stat).toEqual({ files: 2, additions: 10, deletions: 4 });
    expect(merged.staged_stat).toEqual({ files: 1, additions: 3, deletions: 0 });
  });

  it("keeps the local-branch divergence a chrome-only stub omits", () => {
    const tabs = [tab({ idx: 0, label: "feat", kind: "working", pr_number: 7, is_active: true })];
    const divergence = { pr_head_oid: "abc", ahead: 1, unstaged_files: 2 };
    const prev = snap({ active_tab: 0, tabs, local_branch_divergence: divergence });
    const next = snap({ active_tab: 0, tabs });
    expect(mergeChromeSnapshot(prev, next, "prev").local_branch_divergence).toEqual(divergence);
  });

  it("drops the local-branch divergence across a view change", () => {
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "feat", kind: "working", pr_number: 7, is_active: true })],
      local_branch_divergence: { pr_head_oid: "abc", ahead: 1, unstaged_files: 2 },
    });
    const next = snap({
      active_tab: 1,
      tabs: [tab({ idx: 1, label: "pr-8", pr_number: 8, is_active: true })],
    });
    expect(mergeChromeSnapshot(prev, next, "next").local_branch_divergence).toBeUndefined();
  });

  it("takes the per-tab stale flags from next chrome", () => {
    const prev = snap({
      active_tab: 0,
      tabs: [
        tab({ idx: 0, label: "feat", kind: "working", is_active: true }),
        tab({ idx: 1, label: "pr-8", pr_number: 8, stale: false }),
      ],
    });
    const next = snap({
      active_tab: 0,
      tabs: [
        tab({ idx: 0, label: "feat", kind: "working", is_active: true }),
        tab({ idx: 1, label: "pr-8", pr_number: 8, stale: true }),
      ],
    });
    expect(mergeChromeSnapshot(prev, next, "prev").tabs[1].stale).toBe(true);
  });
});

describe("shouldDeferChromeIdentityChange", () => {
  it("defers chrome-only polls when the view identity changed", () => {
    const prev = snap({
      active_tab: 0,
      tabs: [tab({ idx: 0, label: "pr-1427", pr_number: 1427, is_active: true })],
      pr: { ...pr1427 },
    });
    const next = snap({
      active_tab: 1,
      tabs: [tab({ idx: 1, label: "pr-1434", pr_number: 1434, is_active: true })],
      pr: { ...pr1434 },
    });
    expect(
      shouldDeferChromeIdentityChange(prev, next, {
        chromeOnly: true,
        contentChanged: false,
      }),
    ).toBe(true);
    expect(
      shouldDeferChromeIdentityChange(prev, next, {
        chromeOnly: false,
        contentChanged: false,
      }),
    ).toBe(false);
  });
});

it("chrome merges preserve the content preview context", () => {
  const prev = snap({ preview_context_key: "held-document-source" });
  const next = snap({ preview_context_key: "chrome-stub" });
  expect(mergeChromeSnapshot(prev, next).preview_context_key).toBe("held-document-source");
});
