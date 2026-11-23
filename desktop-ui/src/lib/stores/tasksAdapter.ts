import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { TaskJob } from "$lib/protocol/generated/TaskJob";
import type { Invoke, ResourceClient } from "$lib/protocol/client";
import type { AgentCommandStatus, AgentLogEntry, AppSnapshot, BackgroundTaskSnapshot } from "$lib/types";
import { JOBS } from "./annotationsAdapter";
import type { ResourceStore } from "./resourceStore";

type Args = Record<string, unknown>;

function text(value: unknown): string {
  return typeof value === "string" ? value : "";
}

function nullable(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function list(value: unknown): string[] {
  return Array.isArray(value) ? value.filter((v): v is string => typeof v === "string") : [];
}

/** A scoped run of `reviewers` on `tab`. */
function scoped(tab: TabKey, a: Args, paths: string[], reviewers: string[]): Command {
  return {
    kind: "run_scoped_review",
    tab,
    scope: text(a.scope),
    paths,
    reviewers,
    focusPrompt: nullable(a.focusPrompt ?? a.focus_prompt),
  };
}

/** The wire command for a run on `tab`, or for a sidebar triage, which names no tab; null for any other command. */
export function runCommand(command: string, a: Args, tab: TabKey | null): Command | null {
  if (command === "run_pr_triage") {
    return { kind: "run_pr_triage", projectId: text(a.projectId), prNumber: Number(a.prNumber) };
  }
  if (command === "run_branch_triage") {
    return { kind: "run_branch_triage", projectId: text(a.projectId), branch: text(a.branch) };
  }
  if (tab === null) return null;
  switch (command) {
    case "run_ai_review":
      return { kind: "run_review", tab, scope: text(a.scope) };
    case "generate_tour":
      return { kind: "generate_tour", tab };
    case "run_importance_agent":
      return { kind: "run_importance_agent", tab };
    case "generate_diagram":
      return { kind: "generate_diagram", tab, diagramKind: text(a.kind), customPrompt: nullable(a.customPrompt) };
    case "run_ai_expert_review":
      return { kind: "run_expert_review", tab, scope: text(a.scope), expertId: text(a.expertId) };
    case "run_ai_professor_review":
      return scoped(tab, a, [], ["professor"]);
    case "run_ai_triage_review":
      return scoped(tab, a, [], ["triage"]);
    case "run_ai_review_files":
      return scoped(tab, a, list(a.paths), ["general"]);
    case "run_ai_validate":
      return { kind: "run_validate", tab, scope: text(a.scope) };
    case "send_note_to_agent":
      return { kind: "send_note_to_agent", tab, noteId: text(a.noteId) };
    case "run_ai_scoped_review":
      return scoped(tab, a, list(a.paths), list(a.reviewerKinds));
    default:
      return null;
  }
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
  /** Each host task's output lines as last read, by task id. */
  logs: Map<string, AgentLogEntry[]>;
}

/** The most lines the active tab's agent log shows, as the App's snapshot caps it. */
const LOG_LINES = 200;

/**
 * The background tasks. With the flag off the App's snapshot lists them and
 * the Tauri commands stop them. With it on, `jobs` lists the task host's
 * tasks beside the App's, and a host task is stopped through `er_send` and
 * its output read through `task_log`.
 */
export class TasksAdapter {
  /** The status each host task had when its log was last read. */
  private readonly readAt = new Map<string, string>();

  constructor(private readonly deps: TasksAdapterDeps) {}

  /** The task rows `jobs` lists; null with the flag off or before it answers. */
  private rows(): TaskJob[] | null {
    if (!this.deps.flagOn) return null;
    const held = this.deps.store.get(JOBS)?.value;
    return held?.kind === "jobs" ? held.tasks : null;
  }

  /** The tasks `jobs` lists; null with the flag off or before it answers. */
  tasks(): BackgroundTaskSnapshot[] | null {
    return this.rows()?.map(asSnapshot) ?? null;
  }

  /**
   * `raw` with the host's tasks from `jobs` after the App's, and the tasks
   * on the painted bucket among the tab's agent commands and in its agent
   * log, as the App's snapshot merges its own.
   */
  overlay(raw: AppSnapshot): AppSnapshot {
    const rows = this.rows();
    if (rows === null) return raw;
    const tasks = rows.map(asSnapshot);
    const painted = rows.filter((task) => task.er_dir === raw.review_bucket_er_dir);
    const commands: AgentCommandStatus[] = [...(raw.agent_commands ?? [])];
    for (const task of painted) {
      if (!commands.some((c) => c.name === task.kind)) {
        commands.push({ name: task.kind, status: task.status, error: task.error ?? null });
      }
    }
    const log = [...(raw.agent_log ?? []), ...painted.flatMap((task) => this.deps.logs.get(task.id) ?? [])];
    return {
      ...raw,
      background_tasks: [...(raw.background_tasks ?? []), ...tasks],
      agent_commands: commands,
      agent_log: log.slice(-LOG_LINES),
    };
  }

  /**
   * Reads again the log of each host task on `bucket` that runs, or that
   * finished since its log was last read, into `logs`.
   */
  async refreshLogs(bucket: string | null | undefined): Promise<void> {
    if (!bucket) return;
    const painted = (this.rows() ?? []).filter((task) => task.er_dir === bucket);
    await Promise.all(
      painted
        .filter((task) => task.status === "running" || this.readAt.get(task.id) !== task.status)
        .map(async (task) => {
          this.deps.logs.set(task.id, await this.log(task.id));
          this.readAt.set(task.id, task.status);
        }),
    );
  }

  /** Whether `jobs` lists `id` as one of the host's tasks. */
  private onHost(id: string): boolean {
    return (this.tasks() ?? []).some((task) => task.id === id);
  }

  /**
   * With the flag on, a run goes through `er_send` keyed to the active tab,
   * and so does a stop of a host task; null for anything else.
   */
  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn) return null;
    const run = runCommand(command, args, this.deps.activeKey());
    if (run !== null) return this.send(run);
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
