import { describe, expect, it } from "bun:test";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import { AiAsks } from "./aiAsks";

const TAB = 4;
const VIEW = { repo_root: "/repo", pr_number: null, branch: "feature", mode: "branch" };

function asks(flagOn: boolean, ack: Ack = { changed: [], created: null }) {
  const sent: Command[] = [];
  const held: string[] = [];
  const store = {
    get: () => ({ version: 1, value: { kind: "annotations", view: VIEW } }) as never,
    send: async (command: Command) => {
      sent.push(command);
      return ack;
    },
    whenAt: async (key: unknown, version: number) => {
      held.push(`${JSON.stringify(key)}@${version}`);
    },
  };
  return { sent, held, asks: new AiAsks({ flagOn, store, activeKey: () => TAB }) };
}

describe("the card asks", () => {
  it("send each ask keyed to the active tab with the flag on, and wait for the keys the Ack names", async () => {
    const ack: Ack = { changed: [{ key: { kind: "jobs" }, version: 3 }], created: null };
    const { sent, held, asks: ai } = asks(true, ack);
    await ai.route("ask_ai", { threadId: "q-1", prompt: "" });
    await ai.route("validate_with_ai", { threadId: "q-1", findingId: null });
    await ai.route("validate_with_ai", { threadId: null, findingId: "f-1" });
    await ai.route("elaborate_with_ai", { threadId: "n-1" });
    await ai.route("reply_to_finding", { findingId: "f-1", body: "Is it?", aiAssist: true });
    expect(sent).toEqual([
      { kind: "ask_ai", tab: TAB, threadId: "q-1", prompt: "" },
      { kind: "validate_with_ai", tab: TAB, threadId: "q-1", findingId: null },
      { kind: "validate_with_ai", tab: TAB, threadId: null, findingId: "f-1" },
      { kind: "elaborate_with_ai", tab: TAB, threadId: "n-1" },
      { kind: "ask_ai_on_finding", tab: TAB, view: VIEW, findingId: "f-1", body: "Is it?", clientId: null },
    ] as Command[]);
    expect(held).toEqual(Array(5).fill('{"kind":"jobs"}@3'));
  });

  it("leave a plain finding reply and other commands to their own routes", () => {
    const { asks: ai } = asks(true);
    expect(ai.route("reply_to_finding", { findingId: "f-1", body: "agreed", aiAssist: false })).toBeNull();
    expect(ai.route("add_question", {})).toBeNull();
  });
});
