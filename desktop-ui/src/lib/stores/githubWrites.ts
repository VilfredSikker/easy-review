import type { Command } from "$lib/protocol/generated/Command";
import type { PrAction } from "$lib/protocol/generated/PrAction";
import type { PrTarget } from "$lib/protocol/generated/PrTarget";
import type { ReviewLine } from "$lib/protocol/generated/ReviewLine";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { ResourceStore } from "./resourceStore";

type Args = Record<string, unknown>;

/** The GitHub senders `er_send` carries with the flag on. */
const ROUTED: ReadonlySet<string> = new Set([
  "pull_github_comments",
  "post_github_pr_comment",
  "post_github_line_comment",
  "submit_github_pr_decision",
  "submit_pending_review",
  "run_github_pr_action",
]);

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** The composer's line target as the wire names it. */
export function reviewLine(target: unknown): ReviewLine {
  const t = (target ?? {}) as Args;
  const lineEnd = t.lineNumEnd;
  return {
    file: text(t.file),
    lineNum: typeof t.lineNum === "number" ? t.lineNum : 0,
    lineNumEnd: typeof lineEnd === "number" ? lineEnd : null,
    side: typeof t.side === "string" ? t.side : null,
  };
}

/** The wire command for a GitHub sender on `tab`; null for any other command. */
export function githubWrite(command: string, a: Args, tab: TabKey): Command | null {
  switch (command) {
    case "pull_github_comments":
      return { kind: "pull_github_comments", tab, force: a.force === true };
    case "post_github_pr_comment":
      return { kind: "post_github_pr_comment", tab, body: text(a.body) };
    case "post_github_line_comment":
      return { kind: "post_github_line_comment", tab, target: reviewLine(a.target), body: text(a.text) };
    case "submit_github_pr_decision":
      return { kind: "submit_github_pr_decision", tab, mode: text(a.mode), summary: text(a.summary) };
    case "submit_pending_review":
      return { kind: "submit_pending_review", tab, mode: text(a.mode), summary: text(a.summary) };
    case "run_github_pr_action":
      return { kind: "run_github_pr_action", tab, pr: a.pr as PrTarget, action: a.action as PrAction };
    default:
      return null;
  }
}

interface GithubWritesDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "send" | "whenAt">;
  /** The registry's active tab, whose PR the page shows. */
  activeKey: () => TabKey | null;
}

/**
 * The GitHub senders with the flag on: each goes through `er_send` keyed to
 * the active tab, so the server reads that tab's PR from the registry and
 * its content, and resolves once every key the `Ack` names is held.
 */
export class GithubWrites {
  constructor(private readonly deps: GithubWritesDeps) {}

  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn || !ROUTED.has(command)) return null;
    const tab = this.deps.activeKey();
    if (tab === null) return null;
    const wire = githubWrite(command, args, tab);
    return wire ? this.send(wire) : null;
  }

  async send(command: Command): Promise<void> {
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      throw new Error((e as { message?: string }).message ?? String(e), { cause: e });
    }
    await Promise.all(ack.changed.map(({ key, version }) => this.deps.store.whenAt(key, version)));
  }
}
