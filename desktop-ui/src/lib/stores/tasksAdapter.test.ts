import { describe, expect, it } from "bun:test";
import type { Invoke } from "$lib/protocol/client";
import type { Ack } from "$lib/protocol/generated/Ack";
import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { ResourceKey } from "$lib/protocol/generated/ResourceKey";
import type { AppSnapshot, BackgroundTaskSnapshot } from "$lib/types";
import { JOBS } from "./annotationsAdapter";
import { TasksAdapter } from "./tasksAdapter";

const TAB = 4;
const TASK = {
  id: "review:1",
  kind: "review",
  label: "Review",
  target_label: "feature",
  scope: "branch",
  repo_root: "/repo",
  branch_label: "feature",
  pr_number: null,
  remote_repo: null,
  er_dir: "/bucket",
  status: "running",
  error: null,
  started_at_ms: 1,
  finished_at_ms: null,
};
const HELD: Resource = { kind: "jobs", ai_replies: [], arena_runs: [], review_flows: [], tasks: [TASK], triage: [] };
const APP_TASK = { ...TASK, id: "triage:9", kind: "triage" } as BackgroundTaskSnapshot;

function adapter(flagOn: boolean) {
  const sent: Command[] = [];
  const fetched: ResourceKey[] = [];
  const invoked: string[] = [];
  const awaited: string[] = [];
  const invoke: Invoke = async <T,>(command: string) => {
    invoked.push(command);
    return [{ command_name: "triage", source: "stdout", text: "app" }] as T;
  };
  const tasks = new TasksAdapter({
    flagOn,
    activeKey: () => TAB,
    invoke,
    store: {
      get: (key: ResourceKey) => (key.kind === JOBS.kind ? { key, version: 2, value: HELD } : undefined) as never,
      send: async (command: Command) => {
        sent.push(command);
        return { changed: [{ key: JOBS, version: 3 }], created: null } satisfies Ack;
      },
      whenAt: async (key: ResourceKey, version: number) => {
        awaited.push(`${key.kind}@${version}`);
      },
    },
    client: {
      fetch: async (keys: ResourceKey[]) => {
        fetched.push(...keys);
        const value: Resource = { kind: "task_log", lines: [{ command_name: "review", source: "stdout", text: "host" }] };
        return keys.map((key) => ({ key, version: 0, value }));
      },
    },
  });
  return { tasks, sent, fetched, invoked, awaited };
}

describe("TasksAdapter", () => {
  it("lists the host's tasks from jobs after the App's with the flag on", () => {
    const raw = { background_tasks: [APP_TASK] } as unknown as AppSnapshot;
    expect(adapter(true).tasks.overlay(raw).background_tasks?.map((t) => t.id)).toEqual(["triage:9", "review:1"]);
    expect(adapter(false).tasks.overlay(raw)).toBe(raw);
  });

  it("stops a host task through er_send and leaves the App's to its command", async () => {
    const { tasks, sent, awaited } = adapter(true);
    await tasks.route("cancel_running_review", { id: "review:1" });
    expect(sent).toEqual([{ kind: "cancel_running_task", id: "review:1" }]);
    expect(awaited).toEqual(["jobs@3"]);
    expect(tasks.route("cancel_queued_review", { id: "triage:9" })).toBeNull();
    expect(adapter(false).tasks.route("cancel_running_review", { id: "review:1" })).toBeNull();
  });

  it("reads a host task's log through task_log and an App task's through its command", async () => {
    const { tasks, fetched, invoked } = adapter(true);
    expect((await tasks.log("review:1")).map((l) => l.text)).toEqual(["host"]);
    expect(fetched).toEqual([{ kind: "task_log", id: "review:1" }]);
    expect((await tasks.log("triage:9")).map((l) => l.text)).toEqual(["app"]);
    expect(invoked).toEqual(["get_background_task_log"]);
  });
});
