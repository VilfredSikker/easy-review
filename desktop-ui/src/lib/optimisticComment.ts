import { snapshotViewIdentity } from "./snapshotChrome";
import type { AppSnapshot, FileSnapshot, ThreadSnapshot } from "./types";

export const ADD_THREAD_COMMANDS = [
  "add_comment",
  "add_question",
  "add_note",
] as const;

export type AddThreadCommand = (typeof ADD_THREAD_COMMANDS)[number];

export interface ParsedAddThread {
  file: string;
  hunkIdx: number;
  lineNum: number | null;
  lineNumEnd: number | null;
  text: string;
  side: "LEFT" | "RIGHT";
}

export interface OptimisticThread {
  id: string;
  hunkIdx: number;
  filePath: string;
  thread: ThreadSnapshot;
  viewIdentity: string;
}

let seq = 0;

export function isAddThreadCommand(command: string): command is AddThreadCommand {
  return (ADD_THREAD_COMMANDS as readonly string[]).includes(command);
}

export function nextOptimisticId(now = Date.now()): string {
  seq += 1;
  return `opt-${now}-${seq}`;
}

function asNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "") {
    const n = Number(value);
    if (Number.isFinite(n)) return n;
  }
  return null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function pick(args: Record<string, unknown>, camel: string, snake: string): unknown {
  return args[camel] ?? args[snake];
}

function threadKind(command: AddThreadCommand): ThreadSnapshot["kind"] {
  if (command === "add_comment") return "comment";
  if (command === "add_note") return "note";
  return "question";
}

function parseSide(value: unknown): "LEFT" | "RIGHT" {
  const raw = asString(value)?.toUpperCase();
  if (raw === "LEFT" || raw === "OLD") return "LEFT";
  return "RIGHT";
}

export function parseAddThreadArgs(
  command: string,
  args: Record<string, unknown>,
): ParsedAddThread | null {
  if (!isAddThreadCommand(command)) return null;
  const text = asString(args.text)?.trim() ?? "";
  if (!text) return null;
  return {
    file: asString(args.file)?.trim() ?? "",
    hunkIdx: asNumber(pick(args, "hunkIdx", "hunk_idx")) ?? 0,
    lineNum: asNumber(pick(args, "lineNum", "line_num")),
    lineNumEnd: asNumber(pick(args, "lineNumEnd", "line_num_end")),
    text,
    side: parseSide(args.side),
  };
}

export function buildOptimisticThread(
  command: AddThreadCommand,
  parsed: ParsedAddThread,
  viewIdentity: string,
  filePath: string,
  opts?: { nowIso?: string; id?: string },
): OptimisticThread {
  const id = opts?.id ?? nextOptimisticId();
  const timestamp = opts?.nowIso ?? new Date().toISOString();
  const line = parsed.lineNum ?? 0;
  const thread: ThreadSnapshot = {
    id,
    kind: threadKind(command),
    file: filePath,
    line,
    line_end: parsed.lineNumEnd,
    side: parsed.side,
    source: "local",
    synced: false,
    stale: false,
    resolved: false,
    root: {
      id,
      author: "you",
      kind: "you",
      timestamp,
      body_markdown: parsed.text,
    },
    replies: [],
    promoted_to: null,
  };
  return { id, hunkIdx: parsed.hunkIdx, filePath, thread, viewIdentity };
}

function fileForPending(snap: AppSnapshot, pending: OptimisticThread): FileSnapshot | undefined {
  return snap.files.find((f) => f.path === pending.filePath);
}

function bumpAiCounts(snap: AppSnapshot, kind: ThreadSnapshot["kind"], delta: number): void {
  const clamp = (n: number) => Math.max(0, n + delta);
  if (kind === "comment") {
    snap.ai.comments = clamp(snap.ai.comments);
    snap.ai.local_comment_count = clamp(snap.ai.local_comment_count);
    snap.ai.unpushed = clamp(snap.ai.unpushed);
  } else if (kind === "question") {
    snap.ai.questions = clamp(snap.ai.questions);
  } else {
    snap.ai.notes = clamp(snap.ai.notes);
  }
}

function bumpFileCounts(file: FileSnapshot, kind: ThreadSnapshot["kind"], delta: number): void {
  const clamp = (n: number) => Math.max(0, n + delta);
  if (kind === "comment") file.comment_count = clamp(file.comment_count);
  else if (kind === "question") file.question_count = clamp(file.question_count);
}

export function applyOptimisticThread(snap: AppSnapshot, pending: OptimisticThread): void {
  const already = snap.ai.threads.some((t) => t.id === pending.id);
  if (!already) {
    snap.ai.threads = [...snap.ai.threads, pending.thread];
    bumpAiCounts(snap, pending.thread.kind, 1);
  }

  const file = fileForPending(snap, pending);
  if (!file) return;
  if (!already) bumpFileCounts(file, pending.thread.kind, 1);

  const hunk = file.hunks[pending.hunkIdx];
  if (hunk && !hunk.threads.some((t) => t.id === pending.id)) {
    hunk.threads = [...hunk.threads, pending.thread];
  }
}

export function removeOptimisticThread(snap: AppSnapshot, pending: OptimisticThread): void {
  const existed = snap.ai.threads.some((t) => t.id === pending.id);
  if (existed) {
    snap.ai.threads = snap.ai.threads.filter((t) => t.id !== pending.id);
    bumpAiCounts(snap, pending.thread.kind, -1);
  }

  const file = fileForPending(snap, pending);
  if (!file) return;
  if (existed) bumpFileCounts(file, pending.thread.kind, -1);

  const hunk = file.hunks[pending.hunkIdx];
  if (hunk) {
    hunk.threads = hunk.threads.filter((t) => t.id !== pending.id);
  }
}

export function reapplyOptimisticThreads(
  snap: AppSnapshot,
  pending: OptimisticThread[],
): void {
  const identity = snapshotViewIdentity(snap);
  for (const p of pending) {
    if (p.viewIdentity !== identity) continue;
    if (snap.ai.threads.some((t) => t.id === p.id)) continue;
    applyOptimisticThread(snap, p);
  }
}
