import type { Command } from "$lib/protocol/generated/Command";
import type { PaintedView } from "$lib/protocol/generated/PaintedView";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { ResourceStore } from "./resourceStore";

type Args = Record<string, unknown>;

function text(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function pick(args: Args, camel: string, snake: string): unknown {
  return args[camel] ?? args[snake];
}

/**
 * The wire command for a card ask on `tab`; null for any other command. An
 * AI-assisted finding reply carries the view `annotations(tab)` named, so a
 * tab that shows another view since refuses it.
 */
export function aiAsk(command: string, a: Args, tab: TabKey, view: PaintedView | null): Command | null {
  const threadId = text(pick(a, "threadId", "thread_id"));
  const findingId = text(pick(a, "findingId", "finding_id"));
  switch (command) {
    case "ask_ai":
      return threadId === null ? null : { kind: "ask_ai", tab, threadId, prompt: text(a.prompt) ?? "" };
    case "validate_with_ai":
      return { kind: "validate_with_ai", tab, threadId, findingId };
    case "elaborate_with_ai":
      return threadId === null ? null : { kind: "elaborate_with_ai", tab, threadId };
    case "reply_to_finding":
      if (pick(a, "aiAssist", "ai_assist") !== true || findingId === null || view === null) return null;
      return { kind: "ask_ai_on_finding", tab, view, findingId, body: text(a.body) ?? "", clientId: text(a.id) };
    default:
      return null;
  }
}

interface AiAsksDeps {
  store: Pick<ResourceStore, "get" | "send" | "whenAt">;
  /** The registry's active tab, the one the page shows. */
  activeKey: () => TabKey | null;
}

/**
 * The card asks: each goes through `er_send` keyed to the
 * active tab, and resolves once every key the `Ack` names is held, so the
 * reply being written already shows from `jobs`; the answer comes with the
 * push of `annotations(tab)`.
 */
export class AiAsks {
  constructor(private readonly deps: AiAsksDeps) {}

  route(command: string, args: Args = {}): Promise<void> | null {
    const tab = this.deps.activeKey();
    if (tab === null) return null;
    const wire = aiAsk(command, args, tab, this.view(tab));
    return wire ? this.send(wire) : null;
  }

  private view(tab: TabKey): PaintedView | null {
    const held = this.deps.store.get({ kind: "annotations", tab })?.value;
    return held?.kind === "annotations" ? held.view : null;
  }

  private async send(command: Command): Promise<void> {
    let ack;
    try {
      ack = await this.deps.store.send(command);
    } catch (e) {
      throw new Error((e as { message?: string }).message ?? String(e), { cause: e });
    }
    await Promise.all(ack.changed.map(({ key, version }) => this.deps.store.whenAt(key, version)));
  }
}
