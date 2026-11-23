import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { Invoke } from "$lib/protocol/client";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { ReviewFlowAction } from "$lib/protocol/generated/ReviewFlowAction";
import type { AppSnapshot } from "$lib/pageTypes";
import { JOBS } from "./annotationsAdapter";
import { ReviewFlowAdapter } from "./reviewFlowAdapter";

const TAB = 7;
const flow = (state: string) => JSON.stringify({ state, depth: "deep", awaiting_accept: 0 });
const JOBS_HELD: Resource = {
  kind: "jobs",
  ai_replies: [],
  arena_runs: [],
  review_flows: [
    { er_dir: "/a", json: flow("finding") },
    { er_dir: "/b", json: flow("awaiting_confirmation") },
  ],
};

/** An adapter over a backend that records what it was asked. */
function adapter(flagOn: boolean) {
  const sent: Command[] = [];
  const fetched: ResourceKey[] = [];
  const invoked: [string, Record<string, unknown>][] = [];
  const invoke: Invoke = async <T,>(command: string, args: Record<string, unknown>) => {
    invoked.push([command, args]);
    return { run: { id: "run-old" } } as T;
  };
  const flows = new ReviewFlowAdapter({
    flagOn,
    activeKey: () => TAB,
    invoke,
    store: {
      get: (key: ResourceKey) => (key.kind === JOBS.kind ? { key, version: 3, value: JOBS_HELD } : undefined) as never,
      send: async (command: Command) => {
        sent.push(command);
        return { changed: [], created: null } satisfies Ack;
      },
    },
    client: {
      fetch: async (keys: ResourceKey[]) => {
        fetched.push(...keys);
        const value: Resource = { kind: "review_flow_run", json: JSON.stringify({ run: { id: "run-1" } }) };
        return keys.map((key) => ({ key, version: 0, value }));
      },
    },
  });
  return { flows, sent, fetched, invoked };
}

const painted = (bucket: string) =>
  ({ review_bucket_er_dir: bucket, review_flow: { state: "done" } }) as unknown as AppSnapshot;

describe("ReviewFlowAdapter", () => {
  it("leaves the flag-off controls and run reads on the Tauri commands", async () => {
    const { flows, sent, invoked } = adapter(false);
    expect(flows.route("start_review_flow", { scope: "current", erDir: "/a" })).toBeNull();
    expect(flows.flowFor("/a")).toBeNull();
    await flows.getRun("/a", "run-1");
    expect(invoked).toEqual([["flow_get_run", { erDir: "/a", runId: "run-1" }]]);
    expect(sent).toEqual([]);
  });

  it("reads the painted bucket's flow from jobs, and none for a bucket jobs does not hold", () => {
    const { flows } = adapter(true);
    expect(flows.overlay(painted("/b")).review_flow?.state).toBe("awaiting_confirmation");
    expect(flows.overlay(painted("/a")).review_flow?.state).toBe("finding");
    expect(flows.overlay(painted("/c")).review_flow).toBeNull();
  });

  it("sends each control keyed to the active tab and the painted bucket", async () => {
    const { flows, sent } = adapter(true);
    const controls: [string, Record<string, unknown>, ReviewFlowAction][] = [
      ["start_review_flow", { scope: "current", erDir: "/a" }, { kind: "start", scope: "current" }],
      ["confirm_review_flow", { erDir: "/b" }, { kind: "confirm" }],
      ["decline_review_flow", { erDir: "/b" }, { kind: "decline" }],
      ["rerun_review_flow", { erDir: "/b" }, { kind: "rerun" }],
      ["cancel_review_flow", { erDir: "/b" }, { kind: "cancel" }],
      ["run_standard_review_flow", { erDir: "/b" }, { kind: "run_standard" }],
      [
        "flow_accept_findings",
        { erDir: "/b", runId: "run-1", findingIds: ["f-1"] },
        { kind: "accept_findings", runId: "run-1", findingIds: ["f-1"] },
      ],
    ];
    for (const [command, args] of controls) await flows.route(command, args);
    expect(flows.route("arena_start", {})).toBeNull();
    expect(sent).toEqual(
      controls.map(([, args, action]) => ({ kind: "review_flow", tab: TAB, erDir: String(args.erDir), action })),
    );
  });

  it("fetches a flow's run by its captured bucket", async () => {
    const { flows, fetched, invoked } = adapter(true);
    const run = await flows.getRun("/b", "run-1");
    expect(run.run.id).toBe("run-1");
    expect(fetched).toEqual([{ kind: "review_flow_run", tab: TAB, er_dir: "/b", run_id: "run-1" }]);
    expect(invoked).toEqual([]);
  });

  it("with the flag on the page reads each flow from jobs and sends the controls through er_send", () => {
    const here = dirname(fileURLToPath(import.meta.url));
    const read = (name: string) => readFileSync(join(here, name), "utf8");
    expect(read("reviewFlowResource.svelte.ts")).toMatch(/^\/\/[^\n]*\nimport "\.\/diffResource\.svelte";/);
    expect(read("reviewFlowResource.svelte.ts")).toMatch(
      /if \(flagOn\) \{\s*app\.overlaySnapshot\(\(raw\) => reviewFlowAdapter\.overlay\(raw\)\);\s*app\.routeCmds\(\(command, args\) => reviewFlowAdapter\.route\(command, args\)\);/,
    );
    expect(read("arena.svelte.ts")).not.toContain('"flow_get_run"');
  });
});
