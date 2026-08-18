import { describe, expect, it } from "vitest";
import {
  commentAutoPullKey,
  githubStatusForActiveTab,
  parseGithubSlug,
  resolveActivePrNumber,
  resolveActivePrUrl,
} from "./prUrl";
import type { AppSnapshot, GithubStatusSnapshot, TabSummary } from "./types";

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
      notes: 0,
      unpushed: 0,
      threads: [],
      findings: [],
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

function githubSnap(number: number, url: string): GithubStatusSnapshot {
  return {
    owner: "a",
    repo: "b",
    number,
    url,
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

describe("resolveActivePrNumber", () => {
  it("prefers the active tab PR over the current worktree", () => {
    const snap = minimalSnapshot({
      detected_pr_number: 1473,
      github: githubSnap(1473, "https://github.com/org/repo/pull/1473"),
      worktrees: [
        {
          path: "/repo",
          branch: "feat",
          is_current: true,
          is_pr: true,
          pr_number: 1473,
          is_merged: false,
          remote: "git@github.com:org/repo.git",
        },
      ],
      tabs: [
        tab({
          idx: 0,
          label: "discovery#1469",
          pr_number: 1469,
          remote: "org/discovery",
          is_active: true,
        }),
      ],
      active_tab: 0,
    });
    expect(resolveActivePrNumber(snap)).toBe(1469);
  });

  it("falls back to detected_pr_number when the tab has no PR", () => {
    const snap = minimalSnapshot({
      detected_pr_number: 42,
      worktrees: [
        {
          path: "/repo",
          branch: "feat",
          is_current: true,
          is_pr: true,
          pr_number: 99,
          is_merged: false,
          remote: null,
        },
      ],
      tabs: [tab({ idx: 0, label: "feat", kind: "working", pr_number: null, is_active: true })],
      active_tab: 0,
    });
    expect(resolveActivePrNumber(snap)).toBe(42);
  });

  it("uses the worktree PR only when the tab has no PR of its own", () => {
    const snap = minimalSnapshot({
      worktrees: [
        {
          path: "/repo",
          branch: "feat",
          is_current: true,
          is_pr: true,
          pr_number: 7,
          is_merged: false,
          remote: null,
        },
      ],
      tabs: [tab({ idx: 0, label: "feat", kind: "working", pr_number: null, is_active: true })],
      active_tab: 0,
    });
    expect(resolveActivePrNumber(snap)).toBe(7);
  });
});

describe("githubStatusForActiveTab", () => {
  it("drops github status for a different PR than the tab", () => {
    const snap = minimalSnapshot({
      github: githubSnap(1473, "https://github.com/org/repo/pull/1473"),
      tabs: [
        tab({ idx: 0, label: "pr-1469", pr_number: 1469, is_active: true }),
      ],
      active_tab: 0,
    });
    expect(githubStatusForActiveTab(snap)).toBeNull();
  });

  it("keeps github status when it matches the tab PR", () => {
    const gh = githubSnap(1469, "https://github.com/org/repo/pull/1469");
    const snap = minimalSnapshot({
      github: gh,
      tabs: [
        tab({ idx: 0, label: "pr-1469", pr_number: 1469, is_active: true }),
      ],
      active_tab: 0,
    });
    expect(githubStatusForActiveTab(snap)?.number).toBe(1469);
  });
});

describe("commentAutoPullKey", () => {
  it("keys off the tab PR, not github.number or the worktree", () => {
    const snap = minimalSnapshot({
      github: githubSnap(1473, "https://github.com/org/repo/pull/1473"),
      worktrees: [
        {
          path: "/repo",
          branch: "feat",
          is_current: true,
          is_pr: true,
          pr_number: 1473,
          is_merged: false,
          remote: "org/repo",
        },
      ],
      tabs: [
        tab({
          idx: 1,
          label: "discovery#1469",
          pr_number: 1469,
          remote: "org/discovery",
          repo_root: "/repo",
          is_active: true,
        }),
      ],
      active_tab: 1,
    });
    expect(commentAutoPullKey(snap)).toBe("1:org/discovery:1469");
  });

  it("is stable whether the live GitHub status cache is present", () => {
    const withGithub = commentAutoPullKey(
      minimalSnapshot({
        github: githubSnap(1469, "https://github.com/org/discovery/pull/1469"),
        tabs: [
          tab({
            idx: 0,
            label: "discovery#1469",
            pr_number: 1469,
            remote: "org/discovery",
            is_active: true,
          }),
        ],
        active_tab: 0,
      }),
    );
    const withoutGithub = commentAutoPullKey(
      minimalSnapshot({
        github: null,
        tabs: [
          tab({
            idx: 0,
            label: "discovery#1469",
            pr_number: 1469,
            remote: "org/discovery",
            is_active: true,
          }),
        ],
        active_tab: 0,
      }),
    );
    expect(withGithub).toBe("0:org/discovery:1469");
    expect(withoutGithub).toBe(withGithub);
  });
});

describe("resolveActivePrUrl", () => {
  it("prefers github.url when it matches the resolved PR", () => {
    const snap = minimalSnapshot({
      github: githubSnap(2, "https://github.com/a/b/pull/2"),
      pr: {
        number: 2,
        title: "other",
        state: "open",
        base: "main",
        head: "h",
        url: "https://github.com/a/b/pull/2-pr",
        author: "u",
      },
    });
    expect(resolveActivePrUrl(snap)).toBe("https://github.com/a/b/pull/2");
  });

  it("ignores github.url for a different PR than the tab", () => {
    const snap = minimalSnapshot({
      github: githubSnap(1473, "https://github.com/org/repo/pull/1473"),
      tabs: [
        tab({
          idx: 0,
          label: "discovery#1469",
          pr_number: 1469,
          remote: "git@github.com:org/discovery.git",
          is_active: true,
        }),
      ],
      active_tab: 0,
    });
    expect(resolveActivePrUrl(snap)).toBe("https://github.com/org/discovery/pull/1469");
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

  it("builds URL from tab remote first, falling back to project remote", () => {
    const snap = minimalSnapshot({
      projects: [
        {
          id: "p1",
          name: "proj",
          root_path: "/tmp",
          remote: "git@github.com:wrong/repo.git",
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
          remote: "git@github.com:org/repo.git",
          repo_root: "/tmp",
          is_active: true,
          change_token: "",
        },
      ],
      active_tab: 0,
    });
    expect(resolveActivePrUrl(snap)).toBe("https://github.com/org/repo/pull/42");
  });

  it("falls back to the current worktree's remote when the tab has none", () => {
    const snap = minimalSnapshot({
      projects: [
        {
          id: "p1",
          name: "proj",
          root_path: "/tmp",
          remote: "git@github.com:wrong/repo.git",
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
      worktrees: [
        {
          path: "/tmp/wt",
          branch: "feat",
          is_current: true,
          is_pr: false,
          pr_number: 42,
          is_merged: false,
          remote: "git@github.com:org/repo.git",
        },
      ],
      tabs: [
        {
          idx: 0,
          label: "feat",
          kind: "working",
          branch: "feat",
          pr_number: null,
          remote: null,
          repo_root: "/tmp",
          is_active: true,
          change_token: "",
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
