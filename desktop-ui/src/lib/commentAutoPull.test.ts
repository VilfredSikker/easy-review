import { describe, expect, it } from "vitest";
import { githubCommentsAutoPullKey } from "./commentAutoPull";
import type { AppSnapshot, TabSummary } from "./types";

function tab(partial: Partial<TabSummary> = {}): TabSummary {
  return {
    idx: 0,
    label: "feat",
    kind: "local_branch",
    branch: "feat",
    pr_number: 12,
    remote: "acme/widgets",
    repo_root: "/repo",
    is_active: true,
    change_token: "abc",
    ...partial,
  };
}

function snap(
  extra: Partial<Pick<AppSnapshot, "branch" | "pr" | "github" | "tabs" | "active_tab">> = {},
): Pick<AppSnapshot, "branch" | "pr" | "github" | "tabs" | "active_tab"> {
  return {
    branch: "feat",
    pr: { number: 12 } as AppSnapshot["pr"],
    github: {
      owner: "acme",
      repo: "widgets",
      number: 12,
    } as AppSnapshot["github"],
    tabs: [tab()],
    active_tab: 0,
    ...extra,
  };
}

describe("githubCommentsAutoPullKey", () => {
  it("is stable whether the live GitHub status cache is present", () => {
    const withGithub = githubCommentsAutoPullKey(snap());
    const withoutGithub = githubCommentsAutoPullKey(snap({ github: null }));
    expect(withGithub).toBe("acme/widgets:feat:12");
    expect(withoutGithub).toBe(withGithub);
  });

  it("returns null when there is no PR", () => {
    expect(
      githubCommentsAutoPullKey(
        snap({
          pr: null,
          github: null,
          tabs: [tab({ pr_number: null })],
        }),
      ),
    ).toBeNull();
  });
});
