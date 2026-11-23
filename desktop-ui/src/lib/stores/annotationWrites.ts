import { SvelteMap } from "svelte/reactivity";
import type { Command } from "$lib/protocol/generated/Command";
import type { LineTarget } from "$lib/protocol/generated/LineTarget";
import type { PaintedView } from "$lib/protocol/generated/PaintedView";
import type { TabKey } from "$lib/protocol/generated/TabKey";
import {
  buildOptimisticThreadForKind,
  isAddThreadCommand,
  nextOptimisticId,
  parseAddThreadArgs,
  type ParsedAddThread,
} from "$lib/optimisticComment";
import { profileLog } from "$lib/profileLog";
import type { AiSnapshot, FlatFinding, ThreadMessage, ThreadSnapshot } from "$lib/types";
import type { ResourceStore } from "./resourceStore";

type Args = Record<string, unknown>;

/**
 * What a write the server has not acknowledged yet shows: the threads it
 * adds, the replies it appends and the threads it takes away. Each row keeps
 * the id the write sends as its `clientId`, so the confirmed row is the same.
 */
export interface PendingPaint {
  threads: ThreadSnapshot[];
  replies: { parentId: string; reply: ThreadMessage }[];
  hidden: string[];
}

/**
 * `ai` with the pending writes laid over it: their threads after the
 * server's, their replies after a thread's last, the threads they take away
 * left out. A row the server already holds is not drawn twice.
 */
export function withPending(ai: AiSnapshot, paints: readonly PendingPaint[]): AiSnapshot {
  if (paints.length === 0) return ai;
  const hidden = new Set(paints.flatMap((paint) => paint.hidden));
  const held = new Set(ai.threads.map((thread) => thread.id));
  const replies = paints.flatMap((paint) => paint.replies);
  const threads = ai.threads
    .filter((thread) => !hidden.has(thread.id))
    .map((thread) => {
      const added = replies
        .filter(({ parentId, reply }) => parentId === thread.id && !thread.replies.some((r) => r.id === reply.id))
        .map(({ reply }) => reply);
      return added.length === 0 ? thread : { ...thread, replies: [...thread.replies, ...added] };
    });
  const created = paints.flatMap((paint) => paint.threads).filter((thread) => !held.has(thread.id));
  return { ...ai, threads: [...threads, ...created] };
}

/** The id prefix each sidecar keeps; a client id with another prefix is replaced on the server. */
const ID_PREFIX: Record<ThreadSnapshot["kind"], string> = { comment: "c", note: "n", question: "q" };

/** The annotation writes `er_send` carries with the flag on. */
const ROUTED: ReadonlySet<string> = new Set([
  "add_question",
  "add_note",
  "add_comment",
  "add_document_thread",
  "reply_to_thread",
  "update_thread_message",
  "delete_thread",
  "resolve_thread",
  "promote_to_note",
  "promote_to_comment",
  "promote_finding_to_comment",
  "reply_to_finding",
]);

function text(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function num(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function pick(args: Args, camel: string, snake: string): unknown {
  return args[camel] ?? args[snake];
}

function you(id: string, body: string): ThreadMessage {
  const timestamp = new Date().toISOString();
  const message: ThreadMessage = { id, author: "you", kind: "you", timestamp, body_markdown: body };
  return { ...message, origin: "thread_reply", editable: true, deletable: true };
}

/** `id` when it carries the sidecar's prefix, else a fresh one that does. */
function clientId(id: unknown, kind: ThreadSnapshot["kind"]): string {
  const prefix = ID_PREFIX[kind];
  const given = text(id);
  return given?.startsWith(`${prefix}-`) ? given : nextOptimisticId({ prefix });
}

function lineTarget(parsed: Pick<ParsedAddThread, "file" | "hunkIdx" | "lineNum" | "lineNumEnd">): LineTarget {
  return { file: parsed.file, hunkIdx: parsed.hunkIdx, lineNum: parsed.lineNum, lineNumEnd: parsed.lineNumEnd };
}

function findFinding(ai: AiSnapshot | null, id: string): FlatFinding | undefined {
  const lists = [ai?.findings, ai?.resolved_findings, ai?.dropped_findings];
  return lists.flatMap((list) => list ?? []).find((finding) => finding.id === id);
}

interface Write {
  command: Command;
  paint?: PendingPaint;
}

const NO_PAINT = { threads: [], replies: [], hidden: [] };

/** A thread the page paints for a create, with `id` as its row id. */
function paintedThread(kind: ThreadSnapshot["kind"], parsed: ParsedAddThread, id: string): ThreadSnapshot {
  return buildOptimisticThreadForKind(kind, parsed, "", parsed.file, { id }).thread;
}

function addThread(command: string, a: Args, tab: TabKey, view: PaintedView): Write | null {
  if (!isAddThreadCommand(command)) return null;
  const parsed = parseAddThreadArgs(command, a);
  if (!parsed) return null;
  const kind = command === "add_note" ? "note" : "question";
  const id = clientId(a.id, kind);
  const fields = { tab, view, target: lineTarget(parsed), body: parsed.text, side: text(a.side), clientId: id };
  const wire: Command = kind === "note" ? { kind: "add_note", ...fields } : { kind: "add_question", ...fields };
  return { command: wire, paint: { ...NO_PAINT, threads: [paintedThread(kind, parsed, id)] } };
}

function replyToThread(a: Args, tab: TabKey, view: PaintedView, ai: AiSnapshot | null): Write | null {
  const parentId = text(pick(a, "parentId", "parent_id"));
  const body = text(a.text)?.trim() ?? "";
  if (!parentId || !body) return null;
  const parent = ai?.threads.find((thread) => thread.id === parentId);
  const id = parent ? clientId(null, parent.kind) : null;
  const command: Command = { kind: "reply_to_thread", tab, view, parentId, body, clientId: id };
  if (!id) return { command };
  return { command, paint: { ...NO_PAINT, replies: [{ parentId, reply: you(id, body) }] } };
}

/** A question turned into a note: the question goes and the note shows until the server's row replaces it. */
function promoteToNote(a: Args, tab: TabKey, view: PaintedView, ai: AiSnapshot | null): Write | null {
  const id = text(a.id);
  if (!id) return null;
  const body = text(a.body);
  const command: Command = { kind: "promote_to_note", tab, view, id, body };
  const source = ai?.threads.find((thread) => thread.id === id);
  if (!source) return { command };
  const parsed: ParsedAddThread = {
    file: source.file,
    hunkIdx: source.hunk_index ?? 0,
    lineNum: source.line,
    lineNumEnd: source.line_end ?? null,
    text: body?.trim() || source.root.body_markdown,
    side: source.side === "LEFT" ? "LEFT" : "RIGHT",
  };
  const note = paintedThread("note", parsed, nextOptimisticId({ prefix: "n" }));
  return { command, paint: { ...NO_PAINT, threads: [note], hidden: [id] } };
}

/** A reply to a finding: appended to its thread, or a new comment thread when it has none. */
function replyToFinding(a: Args, tab: TabKey, view: PaintedView, ai: AiSnapshot | null): Write | null {
  const findingId = text(pick(a, "findingId", "finding_id"));
  const body = text(a.body)?.trim() ?? "";
  if (!findingId || !body) return null;
  const id = clientId(null, "comment");
  const command: Command = { kind: "reply_to_finding", tab, view, findingId, body, clientId: id };
  const finding = findFinding(ai, findingId);
  if (!finding) return { command };
  const thread = finding.thread_id ? ai?.threads.find((t) => t.id === finding.thread_id) : undefined;
  if (thread) return { command, paint: { ...NO_PAINT, replies: [{ parentId: thread.id, reply: you(id, body) }] } };
  const parsed: ParsedAddThread = {
    file: finding.file,
    hunkIdx: finding.hunk_index ?? 0,
    lineNum: finding.line,
    lineNumEnd: null,
    text: body,
    side: "RIGHT",
  };
  return { command, paint: { ...NO_PAINT, threads: [paintedThread("comment", parsed, id)] } };
}

/** A GitHub comment on a diff line; it paints nothing until the push brings it. */
function addComment(a: Args, tab: TabKey, view: PaintedView): Write | null {
  // A comment takes a question's line arguments.
  const parsed = parseAddThreadArgs("add_question", a);
  if (!parsed) return null;
  const target = lineTarget(parsed);
  return { command: { kind: "add_comment", tab, view, target, body: parsed.text, side: text(a.side), clientId: text(a.id) } };
}

/** A thread on a previewed document line, checked against the preview key it was picked in. */
function addDocumentThread(a: Args, tab: TabKey): Write | null {
  const line = num(pick(a, "lineNum", "line_num"));
  if (line === null) return null;
  const lineEnd = num(pick(a, "lineNumEnd", "line_num_end"));
  const path = text(a.file) ?? "";
  const previewKey = text(pick(a, "previewKey", "preview_key")) ?? "";
  const threadKind = text(a.kind) ?? "";
  return { command: { kind: "add_document_thread", tab, path, threadKind, line, lineEnd, body: text(a.text) ?? "", previewKey } };
}

/** The wire command for an annotation write on `tab`, painted in `view`, and what it paints until acknowledged. */
export function annotationWrite(
  command: string,
  a: Args,
  tab: TabKey,
  view: PaintedView,
  ai: AiSnapshot | null,
): Write | null {
  const id = text(a.id) ?? "";
  const body = text(a.body);
  switch (command) {
    case "add_question":
    case "add_note":
      return addThread(command, a, tab, view);
    case "add_comment":
      return addComment(a, tab, view);
    case "add_document_thread":
      return addDocumentThread(a, tab);
    case "reply_to_thread":
      return replyToThread(a, tab, view, ai);
    case "update_thread_message":
      return { command: { kind: "update_thread_message", tab, view, id, body: body ?? "" } };
    case "delete_thread":
      return { command: { kind: "delete_thread", tab, view, id } };
    case "resolve_thread":
      return { command: { kind: "resolve_thread", tab, view, id } };
    case "promote_to_note":
      return promoteToNote(a, tab, view, ai);
    case "promote_to_comment":
      return { command: { kind: "promote_to_comment", tab, view, id, body } };
    case "promote_finding_to_comment": {
      const findingId = text(pick(a, "findingId", "finding_id")) ?? "";
      return { command: { kind: "promote_finding_to_comment", tab, view, findingId, body } };
    }
    case "reply_to_finding":
      return replyToFinding(a, tab, view, ai);
    default:
      return null;
  }
}

interface AnnotationWritesDeps {
  /** Whether the backend serves resources (the `v1-service` build). */
  flagOn: boolean;
  store: Pick<ResourceStore, "get" | "send" | "whenAt">;
  /** The registry's active tab, the one the page paints. */
  activeKey: () => TabKey | null;
  /** The AI slice the page shows, where a write finds the thread or finding it names. */
  shownAi: () => AiSnapshot | null;
}

/**
 * The annotation writes with the flag on: each goes through `er_send` keyed
 * to the active tab and the view `annotations(tab)` named, so the server
 * refuses one painted on a view the tab left. A create shows its row at once
 * and keeps it pending until every key the `Ack` names is held; the other
 * writes paint nothing, and the push shows the result. An AI-assisted finding
 * reply keeps its old route.
 */
export class AnnotationWrites {
  private readonly pending = new SvelteMap<string, PendingPaint>();

  constructor(private readonly deps: AnnotationWritesDeps) {}

  /** The paints of the writes not yet acknowledged, oldest first. */
  paints(): PendingPaint[] {
    return [...this.pending.values()];
  }

  route(command: string, args: Args = {}): Promise<void> | null {
    if (!this.deps.flagOn || !ROUTED.has(command)) return null;
    if (command === "reply_to_finding" && pick(args, "aiAssist", "ai_assist") === true) return null;
    const tab = this.deps.activeKey();
    if (tab === null) return null;
    return this.write(command, args, tab);
  }

  private view(tab: TabKey): PaintedView | null {
    const held = this.deps.store.get({ kind: "annotations", tab })?.value;
    return held?.kind === "annotations" ? held.view : null;
  }

  private async write(command: string, args: Args, tab: TabKey): Promise<void> {
    const view = this.view(tab);
    if (!view) throw new Error("The annotations have not loaded yet");
    const write = annotationWrite(command, args, tab, view, this.deps.shownAi());
    if (!write) return;
    const started = performance.now();
    const token = nextOptimisticId({ prefix: "write" });
    if (write.paint) this.pending.set(token, write.paint);
    try {
      let ack;
      try {
        ack = await this.deps.store.send(write.command);
      } catch (e) {
        throw new Error((e as { message?: string }).message ?? String(e), { cause: e });
      }
      await Promise.all(ack.changed.map(({ key, version }) => this.deps.store.whenAt(key, version)));
      profileLog("annotation_write_painted", { command, ms: Math.round(performance.now() - started) });
    } finally {
      this.pending.delete(token);
    }
  }
}
