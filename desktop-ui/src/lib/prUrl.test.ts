import { describe, expect, it } from "vitest";
import { parseGithubSlug, resolveActivePrUrl } from "./prUrl";
import type { AppSnapshot } from "./types";

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
      has_review_json: false,
      eligible_comment_count: 0,
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

describe("parseGithubSlug", () => {
  it("parses HTTPS remotes", () => {
    expect(parseGithubSlug("https://github.com/org/repo.git")).toBe("org/repo");
  });

  it("parses SSH remotes", () => {
    expect(parseGithubSlug("git@github.com:org/repo.git")).toBe("org/repo");
  });

  it("returns null for non-GitHub remotes", () => {
    expect(parseGithubSlug("git@gitlab.com:org/repo.git")).toBeNull();
  });
});

describe("resolveActivePrUrl", () => {
  it("prefers github.url over pr.url", () => {
    const snap = minimalSnapshot({
      github: {
        owner: "a",
        repo: "b",
        number: 1,
        url: "https://github.com/a/b/pull/1",
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
      pr: {
        number: 2,
        title: "other",
        state: "open",
        base: "main",
        head: "h",
        url: "https://github.com/a/b/pull/2",
        author: "u",
      },
    });
    expect(resolveActivePrUrl(snap)).toBe("https://github.com/a/b/pull/1");
  });

  it("falls back to pr.url", () => {
    const snap = minimalSnapshot({
      pr: {
        number: 99,
        title: "t",
        state: "open",
        base: "main",
        head: "h",
        url: "https://github.com/x/y/pull/99",
        author: "u",
      },
    });
    expect(resolveActivePrUrl(snap)).toBe("https://github.com/x/y/pull/99");
  });

  it("builds URL from project remote and tab pr_number", () => {
    const snap = minimalSnapshot({
      projects: [
        {
          id: "p1",
          name: "proj",
          root_path: "/tmp",
          remote: "git@github.com:org/repo.git",
          is_active: true,
          local_branches: [],
          auto_branches: [],
          saved_prs: [],
          my_prs: [],
          prs_to_review: [],
          recent_prs: [],
          recently_merged: [],
        },
      ],
      tabs: [
        {
          idx: 0,
          label: "feat",
          kind: "working",
          branch: "feat",
          pr_number: 42,
          repo_root: "/tmp",
          is_active: true,
        },
      ],
      active_tab: 0,
    });
    expect(resolveActivePrUrl(snap)).toBe("https://github.com/org/repo/pull/42");
  });

  it("returns null when no PR context exists", () => {
    expect(resolveActivePrUrl(minimalSnapshot({}))).toBeNull();
  });
});
