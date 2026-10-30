import { describe, expect, test } from "bun:test";
import { authorLabel, kindStyle, lineRangeLabel, threadActions, type ThreadActionInput } from "./annotationActions";

describe("card labels", () => {
  test("lineRangeLabel", () => {
    expect(lineRangeLabel(0)).toBeNull();
    expect(lineRangeLabel(12)).toBe("L12");
    expect(lineRangeLabel(12, 12)).toBe("L12");
    expect(lineRangeLabel(12, 15)).toBe("L12–15");
  });

  test("authorLabel and kindStyle", () => {
    expect(authorLabel("you", "me")).toBe("You");
    expect(authorLabel("ai", "x")).toBe("Claude");
    expect(authorLabel("human", "octocat")).toBe("octocat");
    expect(kindStyle("note").label).toBe("Note");
  });
});

const base: ThreadActionInput = {
  kind: "comment",
  synced: false,
  promoted: false,
  documentAnchored: false,
  rootEditable: true,
};

describe("threadActions", () => {
  test("a comment keeps a short row and moves the rest to overflow", () => {
    const a = threadActions(base);
    expect(a.primary).toEqual(["reply", "resolve"]);
    expect(a.overflow).toEqual(["copy", "push", "edit", "validate", "delete"]);
  });

  test("a synced comment has nothing to push", () => {
    expect(threadActions({ ...base, synced: true }).overflow).not.toContain("push");
  });

  test("a question carries the AI actions and Promote", () => {
    const a = threadActions({ ...base, kind: "question" });
    expect(a.primary).toEqual(["askAi", "elaborate", "reply", "promote", "delete"]);
    expect(a.overflow).toEqual(["copy", "promoteNote", "edit"]);
  });

  test("Promote is hidden once promoted and on document anchors", () => {
    expect(threadActions({ ...base, kind: "note", promoted: true }).primary).not.toContain("promote");
    const doc = threadActions({ ...base, kind: "question", documentAnchored: true });
    expect(doc.primary).not.toContain("promote");
    expect(doc.overflow).toContain("promoteNote");
  });

  test("a note is never pushed and has no promote-to-note", () => {
    const a = threadActions({ ...base, kind: "note" });
    expect(a.overflow).toEqual(["copy", "edit", "validate"]);
  });

  test("edit only when the root is editable", () => {
    expect(threadActions({ ...base, rootEditable: false }).overflow).not.toContain("edit");
  });
});
