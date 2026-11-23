import { describe, expect, it } from "bun:test";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import type { PaintedView } from "$lib/protocol/generated/PaintedView";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { Versioned } from "$lib/protocol/generated/Versioned";
import { isOptimisticCommand } from "$lib/optimisticLocal";
import type { AiSnapshot, FlatFinding, ThreadSnapshot } from "$lib/types";
import { AnnotationWrites, withPending } from "./annotationWrites";

const TAB = 4;
const VIEW: PaintedView = { repo_root: "/repo", pr_number: null, branch: "", mode: "branch" };

function thread(id: string, kind: ThreadSnapshot["kind"] = "question"): ThreadSnapshot {
  const root = { id, author: "you", kind: "you" as const, timestamp: "", body_markdown: "why?" };
  return {
    id, kind, file: "src/a.rs", line: 2, side: "RIGHT", source: "local", stale: false, resolved: false,
    root, replies: [], promoted_to: null, github_id: null,
  };
}

function finding(threadId: string | null): FlatFinding {
  return { id: "f-1", file: "src/a.rs", line: 3, hunk_index: 0, thread_id: threadId, responses: [] } as unknown as FlatFinding;
}

function ai(threads: ThreadSnapshot[], findings: FlatFinding[] = []): AiSnapshot {
  return { threads, findings, resolved_findings: [], dropped_findings: [] } as unknown as AiSnapshot;
}

/** A store whose sends and version waits resolve when the test says. */
function harness(shown: AiSnapshot) {
  const sent: Command[] = [];
  let answer: (ack: Ack) => void = () => {};
  let refuse: (e: unknown) => void = () => {};
  let held: () => void = () => {};
  const value: Resource = { kind: "annotations", view: VIEW } as unknown as Resource;
  const store = {
    get: (key: ResourceKey): Versioned<Resource> | undefined =>
      key.kind === "annotations" && key.tab === TAB ? { key, version: 1, value } : undefined,
    send: (command: Command) => {
      sent.push(command);
      return new Promise<Ack>((resolve, reject) => {
        answer = resolve;
        refuse = reject;
      });
    },
    whenAt: () => new Promise<void>((resolve) => (held = resolve)),
  };
  const writes = new AnnotationWrites({ flagOn: true, store, activeKey: () => TAB, shownAi: () => shown });
  const ack: Ack = { changed: [{ key: { kind: "annotations", tab: TAB }, version: 2 }], created: null };
  const settle = async () => {
    answer(ack);
    await Promise.resolve();
    await Promise.resolve();
    held();
  };
  return { sent, writes, settle, refuse: (e: unknown) => refuse(e) };
}

const tick = () => new Promise((resolve) => setTimeout(resolve, 0));

describe("the annotation writes with the flag on", () => {
  it("show a question until acknowledged, on the row the server confirms by its client id", async () => {
    const { sent, writes, settle } = harness(ai([]));
    const done = writes.route("add_question", { file: "src/a.rs", hunkIdx: 0, lineNum: 2, text: "why?" });

    const painted = writes.paints().flatMap((paint) => paint.threads);
    expect(painted.map((t) => t.kind)).toEqual(["question"]);
    const command = sent[0] as Extract<Command, { kind: "add_question" }>;
    expect(command.clientId).toBe(painted[0].id);
    expect(command.view).toEqual(VIEW);
    await settle();
    await done;
    expect(writes.paints()).toEqual([]);
  });

  it("draw a confirmed row once while its paint is still pending", () => {
    const pending = { threads: [thread("q-1")], replies: [], hidden: [] };
    const shown = withPending(ai([thread("q-1")]), [pending]);
    expect(shown.threads.map((t) => t.id)).toEqual(["q-1"]);
  });

  it("paint nothing for a resolve, a delete or an edit, which leave the optimistic path", async () => {
    const { sent, writes, settle } = harness(ai([thread("q-1")]));
    for (const [command, args] of [
      ["resolve_thread", { id: "q-1" }],
      ["delete_thread", { id: "q-1" }],
      ["update_thread_message", { id: "q-1", body: "edited" }],
    ] as const) {
      expect(isOptimisticCommand(command, true), command).toBe(false);
      const done = writes.route(command, args);
      expect(writes.paints(), command).toEqual([]);
      await settle();
      await done;
    }
    expect(sent.map((c) => c.kind)).toEqual(["resolve_thread", "delete_thread", "update_thread_message"]);
  });

  it("paint nothing for the finding, checklist, triage and diagram writes, each sent as its command", async () => {
    const { sent, writes, settle } = harness(ai([], [finding(null)]));
    const finding_ = { findingId: "f-1" };
    const response = { findingId: "f-1", responseId: "r-1" };
    for (const [command, args] of [
      ["dismiss_finding", finding_],
      ["resolve_finding", finding_],
      ["unresolve_finding", finding_],
      ["remove_finding_thread", finding_],
      ["update_finding_response", { ...response, body: "edited" }],
      ["delete_finding_response", response],
      ["toggle_checklist_item", { index: 0 }],
      ["delete_review_artifact", { kind: "triage" }],
      ["delete_diagram", { id: "flows" }],
    ] as const) {
      expect(isOptimisticCommand(command, true), command).toBe(false);
      const done = writes.route(command, args);
      expect(writes.paints(), command).toEqual([]);
      await settle();
      await done;
    }
    expect(sent.map((c) => c.kind)).toEqual([
      "dismiss_finding",
      "set_finding_resolved",
      "set_finding_resolved",
      "remove_finding_thread",
      "update_finding_response",
      "delete_finding_response",
      "toggle_checklist_item",
      "delete_review_artifact",
      "delete_diagram",
    ]);
    expect(sent.map((c) => ("resolved" in c ? c.resolved : null)).filter((r) => r !== null)).toEqual([true, false]);
  });

  it("show a reply to a finding with no thread as a new thread, under the id the server takes", async () => {
    const { sent, writes, settle } = harness(ai([], [finding(null)]));
    const done = writes.route("reply_to_finding", { findingId: "f-1", body: "agreed", aiAssist: false });

    const painted = writes.paints().flatMap((paint) => paint.threads);
    expect(painted.map((t) => [t.kind, t.root.body_markdown])).toEqual([["comment", "agreed"]]);
    const command = sent[0] as Extract<Command, { kind: "reply_to_finding" }>;
    expect(command.clientId).toBe(painted[0].id);
    await settle();
    await done;
    expect(writes.paints()).toEqual([]);
  });

  it("hide a promoted question and show its note until acknowledged", async () => {
    const shown = ai([thread("q-1")]);
    const { sent, writes, settle } = harness(shown);
    const done = writes.route("promote_to_note", { id: "q-1" });

    expect(sent.map((c) => c.kind)).toEqual(["promote_to_note"]);
    const drawn = withPending(shown, writes.paints()).threads;
    expect(drawn.map((t) => [t.kind, t.root.body_markdown])).toEqual([["note", "why?"]]);
    expect(drawn[0].id.startsWith("n-")).toBe(true);
    await settle();
    await done;
    expect(writes.paints()).toEqual([]);
  });

  it("append a reply to a finding's thread under the id the server takes", async () => {
    const shown = ai([thread("c-1", "comment")], [finding("c-1")]);
    const { sent, writes, settle } = harness(shown);
    const done = writes.route("reply_to_finding", { findingId: "f-1", body: "agreed", aiAssist: false });

    const command = sent[0] as Extract<Command, { kind: "reply_to_finding" }>;
    expect(writes.paints().flatMap((paint) => paint.threads)).toEqual([]);
    const replies = writes.paints().flatMap((paint) => paint.replies);
    expect(replies.map(({ parentId, reply }) => [parentId, reply.id])).toEqual([["c-1", command.clientId]]);
    const drawn = withPending(shown, writes.paints()).threads;
    expect(drawn.map((t) => [t.id, t.replies.map((r) => r.body_markdown)])).toEqual([["c-1", ["agreed"]]]);
    await settle();
    await done;
    expect(writes.paints()).toEqual([]);
  });

  it("leave an AI-assisted finding reply on its old route", () => {
    const { writes } = harness(ai([], [finding(null)]));
    expect(writes.route("reply_to_finding", { findingId: "f-1", body: "why?", aiAssist: true })).toBeNull();
    expect(writes.route("reply_to_finding", { findingId: "f-1", body: "why?", aiAssist: false })).not.toBeNull();
  });

  it("take a refused write's paint back", async () => {
    const { writes, refuse } = harness(ai([thread("q-1")]));
    const done = writes.route("reply_to_thread", { parentId: "q-1", text: "and?" });
    expect(writes.paints().flatMap((paint) => paint.replies).length).toBe(1);
    refuse({ kind: "refused", message: "The active view changed" });
    await expect(done).rejects.toThrow("The active view changed");
    await tick();
    expect(writes.paints()).toEqual([]);
  });
});
