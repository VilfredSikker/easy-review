import { describe, expect, it } from "bun:test";
import type { ConfigHubField } from "$lib/types";
import { groupAllOn, groupInboxFields, groupPatches, inboxOnCount } from "./inboxGroups";

const bool = (key: string, value: boolean, description = "Show when x"): ConfigHubField => ({
  kind: "bool",
  key,
  label: key.split(".").pop() ?? key,
  description,
  value,
});

const fields: ConfigHubField[] = [
  { kind: "section", title: "Inbox" },
  bool("inbox.show.review_requested", true, "Show when someone asks you to review a PR"),
  bool("inbox.show.ci_failed", true),
  bool("inbox.show.brand_new_kind", false),
  { kind: "section", title: "Inbox notifications" },
  bool("inbox.notify.review_requested", false, "Notify when someone asks you to review a PR"),
  bool("inbox.notify.ci_failed", true),
  bool("display.line_numbers", true),
];

describe("inbox groups", () => {
  it("pairs each kind's show and notify switch in one row", () => {
    const groups = groupInboxFields(fields);
    const reviews = groups.find((g) => g.title === "Reviews");
    expect(reviews?.rows).toHaveLength(1);
    const row = reviews!.rows[0];
    expect(row.show?.key).toBe("inbox.show.review_requested");
    expect(row.notify?.key).toBe("inbox.notify.review_requested");
    expect(row.description).toBe("Someone asks you to review a PR");
  });

  it("never hides a kind the grouping does not know", () => {
    const groups = groupInboxFields(fields);
    expect(groups.at(-1)?.title).toBe("Other");
    expect(groups.at(-1)?.rows.map((r) => r.kind)).toEqual(["brand_new_kind"]);
  });

  it("drops empty groups and ignores non-inbox keys", () => {
    const titles = groupInboxFields(fields).map((g) => g.title);
    expect(titles).toEqual(["Reviews", "CI & GitHub", "Other"]);
  });

  it("counts switches that are on", () => {
    expect(inboxOnCount(groupInboxFields(fields))).toEqual({ on: 3, total: 5 });
  });

  it("All turns on what is off, None turns off what is on, and only sends changes", () => {
    const reviews = groupInboxFields(fields).find((g) => g.title === "Reviews")!;
    expect(groupAllOn(reviews)).toBe(false);
    expect(groupPatches(reviews, true)).toEqual([["inbox.notify.review_requested", true]]);
    const ci = groupInboxFields(fields).find((g) => g.title === "CI & GitHub")!;
    expect(groupAllOn(ci)).toBe(true);
    expect(groupPatches(ci, false)).toEqual([
      ["inbox.show.ci_failed", false],
      ["inbox.notify.ci_failed", false],
    ]);
  });
});
