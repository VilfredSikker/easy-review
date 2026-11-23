import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ReviewFlowAction } from "$lib/protocol/generated/ReviewFlowAction";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { Invoke, ResourceClient } from "$lib/protocol/client";
import type { AppSnapshot, ReviewFlowSnapshot } from "$lib/types";
import type { ArenaRunSnapshot } from "$lib/types/arena";
import { JOBS } from "./annotationsAdapter";
import type { ResourceStore } from "./resourceStore";

type Args = Record<string, unknown>;

interface ReviewFlowAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "send">;
  client: Pick<ResourceClient, "fetch">;
  /** The registry's active tab, the one the page paints. */
  activeKey: () => TabKey | null;
  invoke: Invoke;
}

/** The flow commands the page sends, by the Tauri command each replaces. */
function action(command: string, args: Args): ReviewFlowAction | null {
  switch (command) {
    case "start_review_flow":
      return { kind: "start", scope: String(args.scope ?? "") };
    case "confirm_review_flow":
      return { kind: "confirm" };
    case "decline_review_flow":
      return { kind: "decline" };
    case "rerun_review_flow":
      return { kind: "rerun" };
    case "cancel_review_flow":
      return { kind: "cancel" };
    case "run_standard_review_flow":
      return { kind: "run_standard" };
    case "flow_accept_findings":
      return {
        kind: "accept_findings",
        runId: String(args.runId ?? ""),
        findingIds: (args.findingIds as string[] | undefined) ?? [],
      };
    default:
      return null;
  }
}

/**
 * The review flows' controls and reads: the Tauri commands and the App's
 * snapshot with the flag off, and with it on `er_send` keyed to the active
 * tab and the bucket it painted, with each flow read from `jobs`.
 */
export class ReviewFlowAdapter {
  constructor(private readonly deps: ReviewFlowAdapterDeps) {}

  /** The flow `jobs` holds for `erDir`; null with the flag off or for none. */
  flowFor(erDir: string | null | undefined): ReviewFlowSnapshot | null {
    if (!this.deps.flagOn || !erDir) return null;
    const held = this.deps.store.get(JOBS)?.value;
    if (held?.kind !== "jobs") return null;
    const job = held.review_flows.find((flow) => flow.er_dir === erDir);
    return job ? (JSON.parse(job.json) as ReviewFlowSnapshot) : null;
  }

  /** `raw` with the painted bucket's flow from `jobs`, which the App's snapshot no longer feeds with the flag on. */
  overlay(raw: AppSnapshot): AppSnapshot {
    return { ...raw, review_flow: this.flowFor(raw.review_bucket_er_dir) };
  }

  /**
   * With the flag on, a flow control goes through `er_send` keyed to the
   * active tab and the bucket the page painted; null for any other command
   * and for every command with the flag off.
   */
  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn) return null;
    const flowAction = action(command, args);
    const tab = this.deps.activeKey();
    if (flowAction === null || tab === null) return null;
    const sent: Command = { kind: "review_flow", tab, erDir: String(args.erDir ?? ""), action: flowAction };
    return this.deps.store.send(sent).then(() => undefined);
  }

  /** A flow's run, read from the bucket the flow captured. */
  async getRun(erDir: string, runId: string): Promise<ArenaRunSnapshot> {
    if (!this.deps.flagOn) return this.deps.invoke<ArenaRunSnapshot>("flow_get_run", { erDir, runId });
    const tab = this.deps.activeKey();
    if (tab === null) throw new Error("No tab is open");
    const [entry] = await this.deps.client.fetch([{ kind: "review_flow_run", tab, er_dir: erDir, run_id: runId }]);
    const value: Resource | undefined = entry?.value;
    if (value?.kind !== "review_flow_run") throw new Error("The flow's run did not answer");
    return JSON.parse(value.json) as ArenaRunSnapshot;
  }
}
