import { describe, expect, it } from "bun:test";
import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/types";
import {
  applyInboxFilters,
  formatInboxAge,
  groupInboxItems,
  inboxCategoryChips,
  inboxItemCategory,
  inboxItemProjectId,
  inboxKindMeta,
  inboxReadIds,
  inboxUnreadIds,
  sortInboxItems,
} from "./inboxCategories";

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

describe("groupInboxItems", () => {
  it("groups by taxonomy order and omits empty categories", () => {
    const grouped = groupInboxItems([
      item({ id: "ci", kind: "ci_failed", category: "ci" }),
      item({ id: "c1", kind: "pr_comment", category: "pr_comment" }),
      item({ id: "a1", kind: "pr_review_approved", category: "approved" }),
      item({ id: "c2", kind: "pr_comment", category: "pr_comment" }),
    ]);
    expect(grouped.map((g) => g.category)).toEqual(["pr_comment", "approved", "ci"]);
    expect(grouped[0].label).toBe("Comment on your PR");
    expect(grouped[0].items.map((i) => i.id)).toEqual(["c1", "c2"]);
  });
});

describe("inboxCategoryChips", () => {
  it("returns chips with unread counts for present categories", () => {
    const chips = inboxCategoryChips([
      item({ id: "1", kind: "pr_comment", category: "pr_comment" }),
      item({
        id: "2",
        kind: "pr_comment",
        category: "pr_comment",
        read_at_ms: 10,
      }),
      item({ id: "3", kind: "pr_review_received", category: "review_received" }),
    ]);
    expect(chips.map((c) => c.category)).toEqual(["pr_comment", "review_received"]);
    expect(chips[0]).toEqual({
      category: "pr_comment",
      label: "Comment on your PR",
      total: 2,
      unread: 1,
    });
    expect(chips[1].unread).toBe(1);
  });
});

describe("sortInboxItems", () => {
  it("puts unread first then newest", () => {
    const sorted = sortInboxItems([
      item({ id: "old-unread", kind: "ci_failed", category: "ci", created_at_ms: 1 }),
      item({
        id: "new-read",
        kind: "pr_merged",
        category: "lifecycle",
        created_at_ms: 9,
        read_at_ms: 10,
      }),
      item({ id: "new-unread", kind: "pr_comment", category: "pr_comment", created_at_ms: 8 }),
    ]);
    expect(sorted.map((i) => i.id)).toEqual(["new-unread", "old-unread", "new-read"]);
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

describe("applyInboxFilters", () => {
  it("filters the full list without capping it", () => {
    const items = Array.from({ length: 25 }, (_, i) =>
      item({
        id: `n${i}`,
        kind: "pr_comment",
        category: "pr_comment",
        created_at_ms: i,
      }),
    );
    items.push(
      item({
        id: "ci-old",
        kind: "ci_failed",
        category: "ci",
        created_at_ms: 0,
      }),
    );
    const filtered = applyInboxFilters(items, {
      projects: [],
      projectId: "all",
      read: "all",
      category: "ci",
    });
    expect(filtered.map((i) => i.id)).toEqual(["ci-old"]);
  });

  it("keeps every matching item so the popover can scroll the full inbox", () => {
    const items = Array.from({ length: 40 }, (_, i) =>
      item({
        id: `m${i}`,
        kind: "pr_merged",
        category: "lifecycle",
        created_at_ms: i,
        read_at_ms: i % 2 === 0 ? 10 : null,
      }),
    );
    const filtered = applyInboxFilters(items, {
      projects: [],
      projectId: "all",
      read: "all",
      category: "lifecycle",
    });
    expect(filtered).toHaveLength(40);
    const grouped = groupInboxItems(filtered);
    expect(grouped).toHaveLength(1);
    expect(grouped[0].items).toHaveLength(40);
  });
});

describe("inboxUnreadIds / inboxReadIds", () => {
  it("splits ids for group mark-read and clear-read", () => {
    const items = [
      item({ id: "u1", kind: "pr_comment", category: "pr_comment" }),
      item({
        id: "r1",
        kind: "pr_merged",
        category: "lifecycle",
        read_at_ms: 10,
      }),
      item({ id: "u2", kind: "pr_merged", category: "lifecycle" }),
    ];
    expect(inboxUnreadIds(items)).toEqual(["u1", "u2"]);
    expect(inboxReadIds(items)).toEqual(["r1"]);
    expect(inboxUnreadIds([])).toEqual([]);
    expect(inboxReadIds([])).toEqual([]);
  });

  it("keeps a single-category filter as one group with those ids", () => {
    const items = [
      item({ id: "u", kind: "pr_merged", category: "lifecycle" }),
      item({
        id: "r",
        kind: "pr_merged",
        category: "lifecycle",
        read_at_ms: 1,
      }),
      item({ id: "other", kind: "pr_comment", category: "pr_comment" }),
    ];
    const filtered = applyInboxFilters(items, {
      projects: [],
      projectId: "all",
      read: "all",
      category: "lifecycle",
    });
    const grouped = groupInboxItems(filtered);
    expect(grouped).toHaveLength(1);
    expect(grouped[0].category).toBe("lifecycle");
    expect(inboxUnreadIds(grouped[0].items)).toEqual(["u"]);
    expect(inboxReadIds(grouped[0].items)).toEqual(["r"]);
  });
});
