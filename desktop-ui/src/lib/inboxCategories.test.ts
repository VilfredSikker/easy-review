import { describe, expect, it } from "bun:test";
import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/pageTypes";
import { formatInboxAge, inboxItemCategory, inboxItemProjectId, inboxKindMeta } from "./inboxCategories";

function item(
  partial: Partial<InboxItemSnapshot> & Pick<InboxItemSnapshot, "id" | "kind">,
): InboxItemSnapshot {
  return {
    severity: "info",
    title: partial.title ?? partial.kind,
    body: "",
    source: "github",
    target: {},
    created_at_ms: 0,
    read_at_ms: null,
    dedupe_key: partial.id,
    ...partial,
  };
}

describe("inboxItemCategory", () => {
  it("uses snapshot category when known", () => {
    expect(inboxItemCategory(item({ id: "1", kind: "x", category: "approved" }))).toBe(
      "approved",
    );
  });

  it("falls back to kind when category is missing", () => {
    expect(inboxItemCategory(item({ id: "1", kind: "pr_comment" }))).toBe("pr_comment");
    expect(inboxItemCategory(item({ id: "2", kind: "mystery" }))).toBe("other");
  });
});

describe("inboxItemProjectId", () => {
  const projects: ProjectSnapshot[] = [
    {
      id: "p1",
      name: "one",
      root_path: "/repos/one",
      remote: "org/one",
      is_active: true,
      local_branches: [],
      auto_branches: [],
      saved_prs: [],
      my_prs: [],
      prs_to_review: [],
      recent_prs: [],
      recently_merged: [],
    },
  ];

  it("prefers explicit project id", () => {
    expect(
      inboxItemProjectId(
        item({ id: "1", kind: "x", target: { project_id: "p1" } }),
        projects,
      ),
    ).toBe("p1");
  });

  it("matches remote when id is missing", () => {
    expect(
      inboxItemProjectId(
        item({ id: "1", kind: "x", target: { remote: "org/one" } }),
        projects,
      ),
    ).toBe("p1");
  });

  it("ignores a stored id whose project is gone and matches the remote instead", () => {
    // Items written against a phantom project rooted at a crate subdirectory:
    // the id names nothing, the subdirectory matches no row, and the remote
    // comes back from GitHub in a different case than the project stores.
    expect(
      inboxItemProjectId(
        item({
          id: "1",
          kind: "x",
          target: {
            project_id: "er-desktop",
            repo_root: "/repos/one/crates/er-desktop",
            remote: "Org/One",
          },
        }),
        projects,
      ),
    ).toBe("p1");
  });

  it("returns null when a stored id is gone and nothing else matches", () => {
    expect(
      inboxItemProjectId(
        item({
          id: "1",
          kind: "x",
          target: { project_id: "er-desktop", remote: "other/repo" },
        }),
        projects,
      ),
    ).toBeNull();
  });
});

describe("inboxKindMeta", () => {
  it("styles known review kinds instead of falling back to briefcase", () => {
    expect(inboxKindMeta(item({ id: "1", kind: "pr_review_approved" })).color).toBe(
      "text-add-fg",
    );
    expect(
      inboxKindMeta(item({ id: "2", kind: "pr_review_changes_requested" })).color,
    ).toBe("text-warning");
    expect(inboxKindMeta(item({ id: "3", kind: "pr_review_received" })).color).toBe(
      "text-accent",
    );
    expect(inboxKindMeta(item({ id: "4", kind: "pr_comment_reply" })).color).toBe(
      "text-comment",
    );
  });
});

describe("formatInboxAge", () => {
  it("formats now, minutes, and hours", () => {
    const now = 10_000_000;
    expect(formatInboxAge(now - 10_000, now)).toBe("now");
    expect(formatInboxAge(now - 120_000, now)).toBe("2m");
    expect(formatInboxAge(now - 7_200_000, now)).toBe("2h");
  });
});

