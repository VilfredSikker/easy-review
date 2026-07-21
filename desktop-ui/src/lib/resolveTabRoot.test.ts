import { describe, expect, it } from "vitest";
import { resolveTabRoot } from "./resolveTabRoot";
import type { AppSnapshot, TabSummary } from "./types";

const tab = (overrides: Partial<TabSummary> = {}): TabSummary => ({
  idx: 0,
  label: "feat",
  kind: "local_branch",
  branch: "feat",
  pr_number: null,
  repo_root: "/Users/me/Projects/discovery",
  is_active: true,
  change_token: "",
  ...overrides,
});

function minimalSnapshot(overrides: Partial<AppSnapshot>): AppSnapshot {
  return {
    mode: "branch",
    branch: "feat",
    base: "main",
    input_mode: "normal",
    files: [],
    selected_file: 0,
    current_hunk: null,
    filter: null,
    reviewed_count: 0,
    total_count: 0,
    ai: {
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
      unpushed: 0,
      threads: [],
      findings: [],
      file_risks: [],
      has_review_json: false,
      eligible_comment_count: 0,
      triage: null,
    },
    pr: null,
    panels: { left: true, tree: true, right: true },
    theme: "dark",
    watch_active: false,
    watch_status: { active: false, branch: null, root_path: null },
    worktrees: [],
    projects: [],
    local_branch: null,
    notification: null,
    tabs: [],
    active_tab: 0,
    bg_loading: { pr_list: false, gh_status: false, gh_comments: false },
    ...overrides,
  };
}

describe("resolveTabRoot", () => {
  it("prefers the linked worktree path for the viewed branch", () => {
    const snapshot = minimalSnapshot({
      branch: "vilfred+dev-6490-1-frontend-and-backend-mock",
      worktrees: [
        {
          path: "/Users/me/Projects/discovery",
          branch: "main",
          is_current: true,
          is_pr: false,
          pr_number: null,
          is_merged: false,
        },
        {
          path: "/Users/me/Projects/discovery/.claude/worktrees/vilfred+dev-6490-1-frontend-and-backend-mock",
          branch: "vilfred+dev-6490-1-frontend-and-backend-mock",
          is_current: false,
          is_pr: true,
          pr_number: 6490,
          is_merged: false,
        },
      ],
    });
    const active = tab({
      branch: "vilfred+dev-6490-1-frontend-and-backend-mock",
      repo_root: "/Users/me/Projects/discovery",
    });

    expect(resolveTabRoot(snapshot, active)).toBe(
      "/Users/me/Projects/discovery/.claude/worktrees/vilfred+dev-6490-1-frontend-and-backend-mock",
    );
  });

  it("falls back to repo_root when no worktree matches", () => {
    const snapshot = minimalSnapshot({ branch: "feat", worktrees: [] });
    expect(resolveTabRoot(snapshot, tab())).toBe("/Users/me/Projects/discovery");
  });

  it("returns empty string for remote-only tabs", () => {
    const snapshot = minimalSnapshot({ branch: "dependabot/foo", worktrees: [] });
    const active = tab({
      kind: "remote_pr",
      branch: null,
      repo_root: "",
    });
    expect(resolveTabRoot(snapshot, active)).toBe("");
  });
});
