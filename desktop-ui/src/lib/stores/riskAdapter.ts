import { SvelteMap } from "svelte/reactivity";
import type { Invoke } from "$lib/protocol/client";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { RiskTarget } from "$lib/riskRequests";
import type { RiskFileSnapshot, RiskFilesView, RiskRuleSnapshot, RiskRulesView, RiskTier } from "$lib/types";
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

const targetId = (target: RiskTarget) => `${target.repoRoot}\n${target.remoteRepo ?? ""}`;

interface RiskAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "show" | "send" | "whenAt">;
  invoke: Invoke;
}

/**
 * The risk rules Settings and the Risks tab show, behind one call in both
 * builds. With the flag off it asks the two old commands; with it on it sends
 * `load_risk`, whose `Ack` names the repo's rules key, and reads
 * `risk(key)`. A failed load rejects with its text.
 */
export class RiskAdapter {
  private readonly shown = new Set<string>();
  /** The `risk(key)` and version each target's last load read, by target. */
  private readonly read = new SvelteMap<string, { key: string; version: number }>();

  constructor(private readonly deps: RiskAdapterDeps) {}

  async load(target: RiskTarget, paths: string[]): Promise<RiskView> {
    try {
      if (!this.deps.flagOn) {
        const [view, resolved] = await Promise.all([
          this.deps.invoke<RiskRulesView>("get_risk_rules", { ...target }),
          this.deps.invoke<RiskFilesView>("get_risk_files", { ...target, paths }),
        ]);
        return { repo: view.repo, default: view.default, rules: resolved.rules, files: resolved.files };
      }
      return await this.loadResource(target, paths);
    } catch (e) {
      throw errorText(e);
    }
  }

  /** The rules alone, with the paths they claim counted: what the Risks tab lists. */
  async loadRules(target: RiskTarget, paths: string[]): Promise<RiskRuleSnapshot[]> {
    if (this.deps.flagOn) return (await this.load(target, paths)).rules;
    try {
      return (await this.deps.invoke<RiskFilesView>("get_risk_files", { ...target, paths })).rules;
    } catch (e) {
      throw errorText(e);
    }
  }

  /**
   * Whether a push moved `target`'s `risk(key)` past the version its last load
   * read, as an importance run's reload does; the Risks tab then loads again.
   * Reactive. Always false with the flag off and before a load.
   */
  pushed(target: RiskTarget): boolean {
    const read = this.read.get(targetId(target));
    if (!read) return false;
    const held = this.deps.store.get(riskKey(read.key));
    return held !== undefined && held.version > read.version;
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
    this.read.set(targetId(target), { key: ack.created ?? "", version: moved?.version ?? 0 });
    if (moved) await this.deps.store.whenAt(key, moved.version);
    const held = this.deps.store.get(key)?.value;
    if (held?.kind !== "risk") throw new Error(`no risk rules arrived for ${target.repoRoot}`);
    return held;
  }
}
