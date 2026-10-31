import { describe, expect, test } from "bun:test";
import {
  askAiTitle,
  authorLabel,
  elaborateTitle,
  kindStyle,
  lineRangeLabel,
  sendToAgentButton,
  threadActions,
  type ThreadActionInput,
} from "./annotationActions";

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
  resolved: false,
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
    expect(a.overflow).toEqual(["copy", "promoteNote", "resolve", "edit"]);
  });

  test("Promote is hidden once promoted and on document anchors", () => {
    expect(threadActions({ ...base, kind: "note", promoted: true }).primary).not.toContain("promote");
    const doc = threadActions({ ...base, kind: "question", documentAnchored: true });
    expect(doc.primary).not.toContain("promote");
    expect(doc.overflow).toContain("promoteNote");
  });

  test("an open note can be sent to an agent; nothing else can", () => {
    expect(threadActions({ ...base, kind: "note" }).primary).toEqual([
      "askAi",
      "elaborate",
      "reply",
      "sendToAgent",
      "promote",
      "delete",
    ]);
    expect(threadActions({ ...base, kind: "question" }).primary).not.toContain("sendToAgent");
    expect(threadActions(base).primary).not.toContain("sendToAgent");
    expect(threadActions({ ...base, kind: "note", resolved: true }).primary).not.toContain("sendToAgent");
  });

  test("a note is never pushed and has no promote-to-note", () => {
    const a = threadActions({ ...base, kind: "note" });
    expect(a.overflow).toEqual(["copy", "resolve", "edit", "validate"]);
  });

  test("edit only when the root is editable", () => {
    expect(threadActions({ ...base, rootEditable: false }).overflow).not.toContain("edit");
  });

  test("an open question or note can be resolved", () => {
    for (const kind of ["question", "note"] as const) {
      expect(threadActions({ ...base, kind }).overflow).toContain("resolve");
    }
  });

  test("a resolved question or note keeps Reply and Delete", () => {
    for (const kind of ["question", "note"] as const) {
      const a = threadActions({ ...base, kind, resolved: true });
      expect(a.primary).toEqual(["reply"]);
      expect(a.overflow).toContain("delete");
      expect(a.overflow).not.toContain("resolve");
      expect([...a.primary, ...a.overflow]).not.toContain("promote");
    }
  });

  test("a resolved comment keeps Reply and Delete but not Resolve", () => {
    const a = threadActions({ ...base, resolved: true });
    expect(a.primary).toEqual(["reply"]);
    expect(a.overflow).toContain("delete");
  });
});

describe("sendToAgentButton", () => {
  test("is disabled on a remote PR tab with the reason", () => {
    const b = sendToAgentButton({ remotePr: true, running: false });
    expect(b.disabled).toBe(true);
    expect(b.title).toContain("remote PR");
  });

  test("waits while an agent already runs in the tab", () => {
    const b = sendToAgentButton({ remotePr: false, running: true });
    expect(b.disabled).toBe(true);
    expect(b.label).toBe("Agent working…");
  });

  test("says it takes an agent slot when enabled", () => {
    const b = sendToAgentButton({ remotePr: false, running: false });
    expect(b.disabled).toBe(false);
    expect(b.title).toContain("agent slot");
  });
});

describe("AI action tooltips", () => {
  test("Elaborate says it rewrites rather than answers, per kind", () => {
    for (const kind of ["question", "note"] as const) {
      expect(elaborateTitle(kind)).toMatch(/^Rewrite/);
      expect(elaborateTitle(kind)).toContain("does not answer");
    }
    expect(elaborateTitle("note")).toContain("instruction");
  });

  test("Ask AI answers locally", () => {
    expect(askAiTitle("question")).toContain("answer");
    expect(askAiTitle("note")).toContain("not posted to GitHub");
  });
});
