import { describe, expect, it } from "bun:test";
import type { Invoke } from "$lib/protocol/client";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { AppSnapshot } from "$lib/pageTypes";
import { JOBS } from "./annotationsAdapter";
import { ArenaAdapter } from "./arenaAdapter";

const TAB = 7;
const FAKE = { provider_id: "fake", model_id: "fake-1" };
const RUNNING: Resource = {
  kind: "jobs",
  ai_replies: [],
  arena_runs: [
    {
      run_id: "run-1",
      er_dir: "/bucket",
      status: { running: { round: 1 } },
      progress: { round: 1, total_rounds: 2, phase: "", thinking: ["fake-1"], done: [] },
    },
    { run_id: "run-0", er_dir: "/bucket", status: "complete", progress: { round: 2, total_rounds: 2, phase: "", thinking: [], done: [] } },
  ],
  review_flows: [],
};

/** An adapter over a backend that records what it was asked and answers `created`. */
function adapter(created: string | null = null) {
  const moved: string[] = [];
  const sent: Command[] = [];
  const fetched: ResourceKey[] = [];
  const invoked: [string, Record<string, unknown>][] = [];
  const invoke: Invoke = async <T,>(command: string, args: Record<string, unknown>) => {
    invoked.push([command, args]);
    return (command === "arena_start" ? "run-old" : null) as T;
  };
  const run = new ArenaAdapter({
    activeKey: () => TAB,
    invoke,
    runsMoved: () => moved.push(sent.at(-1)?.kind ?? ""),
    store: {
      get: (key: ResourceKey) => (key.kind === JOBS.kind ? { key, version: 3, value: RUNNING } : undefined) as never,
      send: async (command: Command) => {
        sent.push(command);
        return { changed: [], created } satisfies Ack;
      },
    },
    client: {
      fetch: async (keys: ResourceKey[]) => {
        fetched.push(...keys);
        const value: Resource = { kind: "arena_run", json: JSON.stringify({ run: { id: "run-1" } }) };
        return keys.map((key) => ({ key, version: 0, value }));
      },
    },
  });
  return { run, sent, fetched, invoked, moved };
}

describe("ArenaAdapter", () => {
  it("has the run list read again after a start, an import and a delete, which nothing pushes", async () => {
    const { run, moved } = adapter("run-1");
    await run.start({ scope: "branch", reviewers: [FAKE] });
    await run.acceptFindings("run-1");
    await run.delete("run-1");
    await run.cancel("run-1");
    expect(moved).toEqual(["arena_start", "arena_accept_findings", "arena_delete"]);
  });

  it("sends a batch start keyed to the active tab and reads its run ids, one per line", async () => {
    const { run, sent } = adapter("run-a\nrun-b");
    const ids = await run.start({
      scope: "branch",
      groups: [{ agent_kind: "security", models: [FAKE] }],
      confirm: true,
    });
    expect(ids).toEqual(["run-a", "run-b"]);
    expect(sent[0]).toMatchObject({
      kind: "arena_start",
      tab: TAB,
      confirm: true,
      plan: { scope: "branch", groups: [{ agent_kind: "security", models: [{ ...FAKE, agent_kind: null }] }] },
    });
  });

  it("reads the accepted count from the Ack and a run from its JSON text", async () => {
    const { run, fetched } = adapter("3");
    expect(await run.acceptFindings("run-1", ["f-1"])).toBe(3);
    expect((await run.get("run-1")).run.id).toBe("run-1");
    expect(fetched).toEqual([{ kind: "arena_run", tab: TAB, run_id: "run-1" }]);
  });

  it("reads a live run's progress from jobs and lists only the live runs as background runs", async () => {
    const { run, invoked } = adapter();
    expect((await run.progress("run-1")).thinking).toEqual(["fake-1"]);
    const shown = run.overlay({ active_arena_run: "stale", arena_runs: [], background_arena_runs: [] } as unknown as AppSnapshot, []);
    expect(shown.background_arena_runs?.map((s) => s.id)).toEqual(["run-1"]);
    expect(shown.active_arena_run).toBeNull();
    expect(invoked).toEqual([]);
  });
});
