import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { TaskJob } from "$lib/protocol/generated/TaskJob";
import type { Invoke, ResourceClient } from "$lib/protocol/client";
import type { AgentLogEntry, AppSnapshot, BackgroundTaskSnapshot } from "$lib/types";
import { JOBS } from "./annotationsAdapter";
import type { ResourceStore } from "./resourceStore";

type Args = Record<string, unknown>;

/** Runs `fn` every `ms` until the returned stop is called. */
export type Every = (fn: () => void, ms: number) => () => void;

const every: Every = (fn, ms) => {
  const handle = setInterval(fn, ms);
  return () => clearInterval(handle);
};

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

/** The wire command for a task command; null for any other command. */
export function taskCommand(command: string, a: Args): Command | null {
  switch (command) {
    case "cancel_queued_review":
      return { kind: "cancel_queued_task", id: text(a.id) };
    case "cancel_running_review":
      return { kind: "cancel_running_task", id: text(a.id) };
    default:
      return null;
  }
}

/** A `jobs` task row as the page's task shape. */
function asSnapshot(task: TaskJob): BackgroundTaskSnapshot {
  return { ...task, recent_log: [] };
}

interface TasksAdapterDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "send" | "whenAt">;
  client: Pick<ResourceClient, "fetch">;
  /** The registry's active tab, the one the page shows. */
  activeKey: () => TabKey | null;
  invoke: Invoke;
}

/**
 * The background tasks. With the flag off the App's snapshot lists them and
 * the Tauri commands stop them. With it on, `jobs` lists the task host's
 * tasks beside the App's, and a host task is stopped through `er_send` and
 * its output read through `task_log`.
 */
export class TasksAdapter {
  constructor(private readonly deps: TasksAdapterDeps) {}

  /** The tasks `jobs` lists; null with the flag off or before it answers. */
  tasks(): BackgroundTaskSnapshot[] | null {
    if (!this.deps.flagOn) return null;
    const held = this.deps.store.get(JOBS)?.value;
    return held?.kind === "jobs" ? held.tasks.map(asSnapshot) : null;
  }

  /** `raw` with the host's tasks from `jobs` after the App's. */
  overlay(raw: AppSnapshot): AppSnapshot {
    const tasks = this.tasks();
    return tasks === null ? raw : { ...raw, background_tasks: [...(raw.background_tasks ?? []), ...tasks] };
  }

  /** Whether `jobs` lists `id` as one of the host's tasks. */
  private onHost(id: string): boolean {
    return (this.tasks() ?? []).some((task) => task.id === id);
  }

  /** With the flag on, a stop of a host task goes through `er_send`; null for anything else. */
  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn) return null;
    const wire = taskCommand(command, args);
    if (wire === null || !this.onHost(text(args.id))) return null;
    return this.send(wire);
  }

  /** A task's output lines, oldest first. */
  async log(id: string): Promise<AgentLogEntry[]> {
    if (!this.onHost(id)) return this.deps.invoke<AgentLogEntry[]>("get_background_task_log", { taskId: id });
    const [entry] = await this.deps.client.fetch([{ kind: "task_log", id }]);
    const value: Resource | undefined = entry?.value;
    return value?.kind === "task_log" ? value.lines : [];
  }

  /**
   * Keeps a task panel's log current: every second while the task runs, and
   * once on open for a host task, whose `jobs` row carries no tail. An App
   * task is first read a second in, and a finished one never, as before the
   * host existed. Returns the stop.
   */
  follow(id: string, live: boolean, show: (lines: AgentLogEntry[]) => void, tick: Every = every): () => void {
    const onOpen = this.onHost(id);
    if (!live && !onOpen) return () => {};
    let cancelled = false;
    const read = async () => {
      try {
        const lines = await this.log(id);
        if (!cancelled && (live || lines.length > 0)) show(lines);
      } catch {
        // A pruned task stops answering; the snapshot tail stays on screen.
      }
    };
    if (onOpen) void read();
    const stop = live ? tick(() => void read(), 1000) : () => {};
    return () => {
      cancelled = true;
      stop();
    };
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
