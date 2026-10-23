export interface FlowAcceptanceOrigin {
  erDir: string;
  runId: string;
}

export type ArenaAcceptanceRoute =
  | { kind: "flow"; erDir: string; runId: string; findingIds?: string[] }
  | { kind: "manual"; request: { run_id: string; finding_ids?: string[] } };

/** Route acceptance from the run's captured origin, independent of active tab. */
export function arenaAcceptanceRoute(
  origin: FlowAcceptanceOrigin | null,
  runId: string,
  findingIds?: string[],
): ArenaAcceptanceRoute {
  if (origin) {
    if (origin.runId !== runId) {
      throw new Error("Arena acceptance run does not match the captured flow run");
    }
    return { kind: "flow", erDir: origin.erDir, runId, findingIds: findingIds ? [...new Set(findingIds)] : undefined };
  }
  return { kind: "manual", request: { run_id: runId, finding_ids: findingIds } };
}

export interface FlowAcceptanceReceipt {
  run: {
    id: string;
    findings: { id: string; accepted_at?: string }[];
  };
}

/** Invoke a bucket-bound acceptance, then count only requested IDs recorded as
 * accepted in that same run's persisted receipt. */
export async function acceptFlowFindings(
  origin: FlowAcceptanceOrigin,
  runId: string,
  findingIds: string[],
  invokeCommand: (route: Extract<ArenaAcceptanceRoute, { kind: "flow" }>) => Promise<void>,
  readReceipt: (origin: FlowAcceptanceOrigin) => Promise<FlowAcceptanceReceipt>,
): Promise<number> {
  const route = arenaAcceptanceRoute(origin, runId, findingIds);
  if (route.kind !== "flow" || !route.findingIds?.length) {
    throw new Error("Flow acceptance requires explicit finding IDs");
  }
  await invokeCommand(route);
  const receipt = await readReceipt(origin);
  if (receipt.run.id !== runId) {
    throw new Error("Flow acceptance receipt returned a different run");
  }
  const accepted = new Set(
    receipt.run.findings.filter((finding) => finding.accepted_at).map((finding) => finding.id),
  );
  return route.findingIds.filter((id) => accepted.has(id)).length;
}
