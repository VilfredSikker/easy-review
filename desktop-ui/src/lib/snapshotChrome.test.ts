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
