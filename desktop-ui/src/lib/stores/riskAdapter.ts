import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { RiskTarget } from "$lib/riskRequests";
import type { RiskFileSnapshot, RiskRuleSnapshot, RiskTier } from "$lib/pageTypes";
import type { ResourceStore } from "./resourceStore";
import { errorText } from "./settingsAdapter";

/** One repo's risk rules with the caller's paths counted and resolved. */
export interface RiskView {
  /** The rules key a project rule edit names. */
  repo: string;
  default: RiskTier;
  rules: RiskRuleSnapshot[];
  files: RiskFileSnapshot[];
}

export const riskKey = (key: string): ResourceKey => ({ kind: "risk", key });

interface RiskAdapterDeps {
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
}

/**
 * The risk rules Settings and the Risks tab show: it sends `load_risk`,
 * whose `Ack` names the repo's rules key, and reads `risk(key)`. A failed
 * load rejects with its text.
 */
export class RiskAdapter {
  private readonly shown = new Set<string>();

  constructor(private readonly deps: RiskAdapterDeps) {}

  async load(target: RiskTarget, paths: string[]): Promise<RiskView> {
    try {
      return await this.loadResource(target, paths);
    } catch (e) {
      throw errorText(e);
    }
  }

  /** The rules alone, with the paths they claim counted: what the Risks tab lists. */
  async loadRules(target: RiskTarget, paths: string[]): Promise<RiskRuleSnapshot[]> {
    return (await this.load(target, paths)).rules;
  }

  private async loadResource(target: RiskTarget, paths: string[]): Promise<RiskView> {
    const ack = await this.deps.store.send({ kind: "load_risk", ...target, paths });
    const key = riskKey(ack.created ?? "");
    // A repo's key stays shown once read, so a later push keeps it fresh.
    if (!this.shown.has(ack.created ?? "")) {
      this.shown.add(ack.created ?? "");
      this.deps.store.show(key);
    }
    const moved = ack.changed.find((c) => c.key.kind === "risk");
    if (moved) await this.deps.store.whenAt(key, moved.version);
    const held = this.deps.store.get(key)?.value;
    if (held?.kind !== "risk") throw new Error(`no risk rules arrived for ${target.repoRoot}`);
    return held;
  }
}
