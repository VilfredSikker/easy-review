import { describe, expect, it } from "vitest";
import { resolveContextIdentity } from "./contextIdentity";
import type { AppSnapshot, GithubStatusSnapshot, PrInfo, ProjectSnapshot, TabSummary } from "./types";

function githubSnap(overrides: Partial<GithubStatusSnapshot> = {}): GithubStatusSnapshot {
  return {
    owner: "reshapebiotech",
    repo: "discovery",
    number: 1425,
    url: "https://github.com/reshapebiotech/discovery/pull/1425",
    state: "OPEN",
    is_draft: false,
    title: "t",
    body: "",
    author: "u",
    head_ref: "feat/from-fork",
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
    ...overrides,
  };
}

function tab(partial: Partial<TabSummary> = {}): TabSummary {
  return {
    idx: 0,
    label: "discovery#1425",
    kind: "remote_pr",
    branch: null,
    pr_number: 1425,
    remote: "reshapebiotech/discovery",
    repo_root: "/repo",
    is_active: true,
    change_token: "t",
    ...partial,
  };
}

function prInfo(partial: Partial<PrInfo> = {}): PrInfo {
  return {
    number: 1425,
    title: "t",
    head_ref: "feat/listed",
    state: "OPEN",
    is_draft: false,
    author: "u",
    assignees: [],
    reviewers: [],
    checks_state: null,
    review_decision: null,
    merged_at: null,
    approved_by_me: false,
    base_ref: "develop",
    head_oid: "abc",
    updated_at: "",
    ...partial,
  };
}

function snap(partial: Partial<AppSnapshot> = {}): AppSnapshot {
  return {
    mode: "pr",
    branch: "",
    base: "",
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
      notes: 0,
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
    theme: "graphite",
    watch_active: false,
    watch_status: { active: false, branch: null, root_path: null },
    worktrees: [],
    projects: [],
    local_branch: null,
    notification: null,
    tabs: [tab()],
    active_tab: 0,
    bg_loading: {
      pr_list: false,
      gh_status: false,
      gh_comments: false,
      tab_diff: false,
    },
    ...partial,
  };
}

describe("resolveContextIdentity", () => {
  it("uses snapshot.branch and snapshot.base when present", () => {
    const id = resolveContextIdentity(
      snap({ branch: "feat/local", base: "origin/main" }),
    );
    expect(id).toEqual({ branch: "feat/local", base: "origin/main" });
  });

  it("falls back to github head/base when snapshot identity is empty", () => {
    const id = resolveContextIdentity(snap({ github: githubSnap() }));
    expect(id).toEqual({ branch: "feat/from-fork", base: "main" });
  });

  it("falls back to the PR card when github is missing", () => {
    const id = resolveContextIdentity(
      snap({
        pr: {
          number: 1425,
          title: "t",
          state: "OPEN",
          base: "main",
          head: "feat/pr-card",
          url: "",
          author: "",
        },
      }),
    );
    expect(id).toEqual({ branch: "feat/pr-card", base: "main" });
  });

  it("falls back to sidebar PR lists", () => {
    const project: ProjectSnapshot = {
      id: "discovery",
      name: "discovery",
      root_path: "",
      remote: "reshapebiotech/discovery",
      is_active: true,
      local_branches: [],
      auto_branches: [],
      saved_prs: [],
      my_prs: [],
      prs_to_review: [],
      recent_prs: [prInfo()],
      recently_merged: [],
    };
    const id = resolveContextIdentity(snap({ projects: [project] }));
    expect(id).toEqual({ branch: "feat/listed", base: "develop" });
  });

  it("prefers the PR list for the tab's own remote slug", () => {
    const own: ProjectSnapshot = {
      id: "discovery",
      name: "discovery",
      root_path: "",
      remote: "reshapebiotech/discovery",
      is_active: true,
      local_branches: [],
      auto_branches: [],
      saved_prs: [],
      my_prs: [],
      prs_to_review: [],
      recent_prs: [prInfo({ head_ref: "feat/own", base_ref: "main" })],
      recently_merged: [],
    };
    const other: ProjectSnapshot = {
      ...own,
      id: "other",
      name: "other",
      remote: "other/repo",
      recent_prs: [prInfo({ head_ref: "feat/other", base_ref: "develop" })],
    };
    const id = resolveContextIdentity(
      snap({
        projects: [other, own],
        tabs: [tab({ remote: "reshapebiotech/discovery" })],
      }),
    );
    expect(id).toEqual({ branch: "feat/own", base: "main" });
  });

  it("treats whitespace-only snapshot fields as empty", () => {
    const id = resolveContextIdentity(
      snap({ branch: "   ", base: "", github: githubSnap() }),
    );
    expect(id).toEqual({ branch: "feat/from-fork", base: "main" });
  });
});
