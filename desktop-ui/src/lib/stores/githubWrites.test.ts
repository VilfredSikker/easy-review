import { describe, expect, it } from "bun:test";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import { GithubWrites, githubWrite } from "./githubWrites";
import { annotationWrite } from "./annotationWrites";

const TAB = 4;
const VIEW = { repo_root: "/repo", pr_number: 7, branch: "", mode: "pr" };

function writes(ack: Ack = { changed: [], created: null }) {
  const sent: Command[] = [];
  const held: string[] = [];
  const store = {
    send: async (command: Command) => {
      sent.push(command);
      return ack;
    },
    whenAt: async (key: unknown, version: number) => {
      held.push(`${JSON.stringify(key)}@${version}`);
    },
  };
  return { sent, held, github: new GithubWrites({ store, activeKey: () => TAB }) };
}

describe("the GitHub senders", () => {
  it("send each command keyed to the active tab and wait for the keys the Ack names", async () => {
    const ack: Ack = { changed: [{ key: { kind: "annotations", tab: TAB }, version: 9 }], created: null };
    const { sent, held, github } = writes(ack);
    const target = { file: "src/a.rs", lineNum: 3, lineNumEnd: null, side: "RIGHT" };
    const pr = { owner: "acme", repo: "widgets", number: 7 };
    const action = { kind: "close" };
    await github.route("pull_github_comments", { force: true });
    await github.route("post_github_pr_comment", { body: "ship it" });
    await github.route("post_github_line_comment", { target, text: "here" });
    await github.route("submit_pending_review", { mode: "APPROVE", summary: "ok" });
    await github.route("run_github_pr_action", { pr, action });
    expect(sent).toEqual([
      { kind: "pull_github_comments", tab: TAB, force: true },
      { kind: "post_github_pr_comment", tab: TAB, body: "ship it" },
      { kind: "post_github_line_comment", tab: TAB, target, body: "here" },
      { kind: "submit_pending_review", tab: TAB, mode: "APPROVE", summary: "ok" },
      { kind: "run_github_pr_action", tab: TAB, pr, action },
    ] as Command[]);
    expect(held).toHaveLength(5);
  });

  it("leave other commands to their own routes", () => {
    expect(writes().github.route("open_url_in_browser", {})).toBeNull();
    expect(githubWrite("reply_to_thread", {}, TAB)).toBeNull();
  });

  it("send a reply on a GitHub thread as an annotation write, with the painted view and no paint", () => {
    const write = annotationWrite("reply_to_github_thread", { parentId: "c-1", text: "thanks", id: "c-2" }, TAB, VIEW, null);
    expect(write).toEqual({
      command: { kind: "reply_to_github_thread", tab: TAB, view: VIEW, parentId: "c-1", body: "thanks", clientId: "c-2" },
    });
  });
});
