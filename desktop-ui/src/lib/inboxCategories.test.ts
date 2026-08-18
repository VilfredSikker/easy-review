import { describe, expect, it } from "bun:test";
import type { InboxItemSnapshot, ProjectSnapshot } from "$lib/types";
import {
  formatInboxAge,
  groupInboxItems,
  inboxCategoryChips,
  inboxItemCategory,
  inboxItemProjectId,
  inboxKindMeta,
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

  it("falls back to other when category is missing", () => {
    expect(inboxItemCategory(item({ id: "1", kind: "pr_comment" }))).toBe("other");
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
