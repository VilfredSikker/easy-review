import type { FlowAcceptanceOrigin } from "$lib/arenaFlowAcceptance";
import type { AppSnapshot } from "$lib/types";
import type { ArenaRunSnapshot } from "$lib/types/arena";

/** Quick and Standard expose seeded verification, which retains its own
 * read-only history path. Only Deep owns explicit flow acceptance. */
export function captureHistoryFlowOrigin(
  snapshot: Pick<AppSnapshot, "review_flow" | "review_bucket_er_dir"> | null,
): FlowAcceptanceOrigin | null {
  const flow = snapshot?.review_flow;
  if (flow?.depth !== "deep" || !flow.arena_run_id || !snapshot?.review_bucket_er_dir) return null;
  return { erDir: snapshot.review_bucket_er_dir, runId: flow.arena_run_id };
}

/** Capture the visible bucket before loading history. Only its exact flow run
 * may recover an acceptance origin. Historical runs remain viewable. */
export async function openArenaHistory(
  runId: string,
  currentFlow: FlowAcceptanceOrigin | null,
  readers: {
    loadArena: (runId: string) => Promise<ArenaRunSnapshot>;
    loadFlow: (origin: FlowAcceptanceOrigin) => Promise<ArenaRunSnapshot>;
  },
): Promise<{ snapshot: ArenaRunSnapshot; flowOrigin: FlowAcceptanceOrigin | null }> {
  const flowOrigin = currentFlow?.runId === runId ? { ...currentFlow } : null;
  const snapshot = flowOrigin
    ? await readers.loadFlow(flowOrigin)
    : await readers.loadArena(runId);
  if (snapshot.run.id !== runId) throw new Error("Arena history returned a different run");
  return { snapshot, flowOrigin };
}
