import type { Command } from "$lib/protocol/generated/Command";
import type { Resource } from "$lib/protocol/generated/Resource";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import type { TaskJob } from "$lib/protocol/generated/TaskJob";
import type { ResourceClient } from "$lib/protocol/client";
import type { AgentCommandStatus, AgentLogEntry, AppSnapshot, BackgroundTaskSnapshot } from "$lib/pageTypes";
import { viewTaskTarget } from "$lib/viewIdentity";
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

/**
 * Whether `task` targets the tab `view` paints, by the App's rule
 * (`matches_target`): repo, PR, remote and branch label, in any bucket.
 */
function onPaintedTab(task: TaskJob, view: AppSnapshot): boolean {
  const target = viewTaskTarget(view);
  return (
    target !== null &&
    task.repo_root === target.repo_root &&
    task.pr_number === target.pr_number &&
    task.remote_repo === target.remote_repo &&
    task.branch_label === target.branch_label
  );
}

/** A `jobs` task row as the page's task shape. */
function asSnapshot(task: TaskJob): BackgroundTaskSnapshot {
  return { ...task, recent_log: [] };
}

interface TasksAdapterDeps {
  store: Pick<ResourceStore, "get" | "send" | "whenAt">;
  client: Pick<ResourceClient, "fetch">;
  /** The registry's active tab, the one the page shows. */
  activeKey: () => TabKey | null;
  /** Each host task's output lines as last read, by task id. */
  logs: Map<string, AgentLogEntry[]>;
}

/** The most lines the active tab's agent log shows. */
const LOG_LINES = 200;

/**
 * The background tasks. `jobs` lists the task host's tasks, a task is
 * stopped through `er_send`, and its output is read through `task_log`.
 */
export class TasksAdapter {
  /** The status each host task had when its log was last read. */
  private readonly readAt = new Map<string, string>();

  constructor(private readonly deps: TasksAdapterDeps) {}

  /** The task rows `jobs` lists; null before it answers. */
  private rows(): TaskJob[] | null {
    const held = this.deps.store.get(JOBS)?.value;
    return held?.kind === "jobs" ? held.tasks : null;
  }

  /** The tasks `jobs` lists; null before it answers. */
  tasks(): BackgroundTaskSnapshot[] | null {
    return this.rows()?.map(asSnapshot) ?? null;
  }

  /**
   * `raw` with the tasks from `jobs`, and the tasks on the painted bucket as
   * the tab's agent commands and in its agent log, one command per kind.
   */
  overlay(raw: AppSnapshot): AppSnapshot {
    const rows = this.rows();
    if (rows === null) return raw;
    const painted = rows.filter((task) => onPaintedTab(task, raw));
    const commands: AgentCommandStatus[] = [];
    for (const task of painted) {
      if (!commands.some((c) => c.name === task.kind)) {
        commands.push({ name: task.kind, status: task.status, error: task.error ?? null });
      }
    }
    const log = painted.flatMap((task) => this.deps.logs.get(task.id) ?? []);
    return {
      ...raw,
      background_tasks: rows.map(asSnapshot),
      agent_commands: commands,
      agent_log: log.slice(-LOG_LINES),
    };
  }

  /**
   * Reads again the log of each host task on the tab `view` paints that
   * runs, or that finished since its log was last read, into `logs`, and
   * drops the logs of tasks `jobs` no longer lists.
   */
  async refreshLogs(view: AppSnapshot | null): Promise<void> {
    const rows = this.rows();
    if (rows === null) return;
    const listed = new Set(rows.map((task) => task.id));
    for (const id of [...this.deps.logs.keys()]) if (!listed.has(id)) this.deps.logs.delete(id);
    for (const id of [...this.readAt.keys()]) if (!listed.has(id)) this.readAt.delete(id);
    if (!view) return;
    const painted = rows.filter((task) => onPaintedTab(task, view));
    await Promise.all(
      painted
        .filter((task) => task.status === "running" || this.readAt.get(task.id) !== task.status)
        .map(async (task) => {
          this.deps.logs.set(task.id, await this.log(task.id));
          this.readAt.set(task.id, task.status);
        }),
    );
  }

  /**
   * A run goes through `er_send` keyed to the active tab, and so does a
   * stop; null for anything else. A run on a tab with no tab open is
   * refused, since no backend command runs it.
   */
  route(command: string, args: Args = {}): Promise<void> | null {
    const tab = this.deps.activeKey();
    const run = runCommand(command, args, tab);
    // The host's started or queued notice is the run's one message.
    if (run !== null) return this.send(run);
    // Any key answers whether the command is a run on a tab.
    if (tab === null && runCommand(command, args, 0) !== null) {
      return Promise.reject(new Error(`${command}: no tab is open`));
    }
    const wire = taskCommand(command, args);
    return wire === null ? null : this.send(wire);
  }

  /** A task's output lines, oldest first. */
  async log(id: string): Promise<AgentLogEntry[]> {
    const [entry] = await this.deps.client.fetch([{ kind: "task_log", id }]);
    const value: Resource | undefined = entry?.value;
    return value?.kind === "task_log" ? value.lines : [];
  }

  /**
   * Keeps a task panel's log current: once on open, since a `jobs` row
   * carries no tail, then every second while the task runs. Returns the
   * stop.
   */
  follow(id: string, live: boolean, show: (lines: AgentLogEntry[]) => void, tick: Every = every): () => void {
    let cancelled = false;
    const read = async () => {
      try {
        const lines = await this.log(id);
        if (!cancelled && (live || lines.length > 0)) show(lines);
      } catch {
        // A pruned task stops answering; the snapshot tail stays on screen.
      }
    };
    void read();
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
