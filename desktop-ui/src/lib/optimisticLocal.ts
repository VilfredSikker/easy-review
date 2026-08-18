import {
  applyOptimisticThread,
  buildOptimisticThread,
  isAddThreadCommand,
  nextOptimisticId,
  parseAddThreadArgs,
  removeOptimisticThread,
  type OptimisticThread,
} from "./optimisticComment";
import { snapshotViewIdentity, type SnapshotViewParts } from "./snapshotChrome";
import type {
  AppSnapshot,
  FlatFinding,
  ThreadMessage,
  ThreadSnapshot,
  UiAnnotation,
} from "./types";

export const OPTIMISTIC_COMMANDS = [
  "add_comment",
  "add_question",
  "add_note",
  "reply_to_thread",
  "resolve_thread",
  "delete_thread",
  "update_thread_message",
  "dismiss_finding",
  "promote_to_comment",
  "promote_to_note",
  "bulk_review_pillar",
  "unbulk_review_pillar",
  "add_ui_annotation",
  "delete_ui_annotation",
] as const;

export type OptimisticCommand = (typeof OPTIMISTIC_COMMANDS)[number];

export function isOptimisticCommand(command: string): command is OptimisticCommand {
  return (OPTIMISTIC_COMMANDS as readonly string[]).includes(command);
}

export type OptimisticOp =
  | { type: "add-thread"; id: string; viewIdentity: string; pending: OptimisticThread }
  | {
      type: "reply";
      id: string;
      viewIdentity: string;
      parentId: string;
      reply: ThreadMessage;
    }
  | { type: "resolve"; id: string; viewIdentity: string; threadId: string; prevResolved: boolean }
  | {
      type: "delete-root";
      id: string;
      viewIdentity: string;
      pending: OptimisticThread;
    }
  | {
      type: "delete-reply";
      id: string;
      viewIdentity: string;
      parentId: string;
      reply: ThreadMessage;
      index: number;
    }
  | {
      type: "edit";
      id: string;
      viewIdentity: string;
      threadId: string;
      messageId: string;
      prevBody: string;
      nextBody: string;
    }
  | {
      type: "dismiss-finding";
      id: string;
      viewIdentity: string;
      finding: FlatFinding;
      threads: OptimisticThread[];
    }
  | {
      type: "promote";
      id: string;
      viewIdentity: string;
      source: OptimisticThread;
      created: OptimisticThread;
    }
  | {
      type: "bulk-reviewed";
      id: string;
      viewIdentity: string;
      pillarId: string;
      target: boolean;
      files: { path: string; was: boolean }[];
    }
  | {
      type: "add-annotation";
      id: string;
      viewIdentity: string;
      annotation: UiAnnotation;
    }
  | {
      type: "delete-annotation";
      id: string;
      viewIdentity: string;
      annotation: UiAnnotation;
    };

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function pick(args: Record<string, unknown>, camel: string, snake: string): unknown {
  return args[camel] ?? args[snake];
}

function forEachThreadCopy(
  snap: AppSnapshot,
  threadId: string,
  fn: (thread: ThreadSnapshot) => void,
): void {
  const seen = new Set<ThreadSnapshot>();
  const visit = (t: ThreadSnapshot) => {
    if (t.id !== threadId || seen.has(t)) return;
    seen.add(t);
    fn(t);
  };
  for (const t of snap.ai.threads) visit(t);
  for (const file of snap.files) {
    for (const hunk of file.hunks) {
      for (const t of hunk.threads) visit(t);
    }
  }
}

function locateRoot(
  snap: AppSnapshot,
  threadId: string,
): { thread: ThreadSnapshot; filePath: string; hunkIdx: number } | null {
  for (const file of snap.files) {
    for (let i = 0; i < file.hunks.length; i++) {
      const thread = file.hunks[i].threads.find((t) => t.id === threadId);
      if (thread) return { thread, filePath: file.path, hunkIdx: i };
    }
  }
  const inAi = snap.ai.threads.find((t) => t.id === threadId);
  if (!inAi) return null;
  return { thread: inAi, filePath: inAi.file, hunkIdx: 0 };
}

function locateReply(
  snap: AppSnapshot,
  replyId: string,
): { parent: ThreadSnapshot; reply: ThreadMessage; index: number } | null {
  const search = (threads: ThreadSnapshot[]) => {
    for (const t of threads) {
      const index = t.replies.findIndex((r) => r.id === replyId);
      if (index >= 0) return { parent: t, reply: t.replies[index], index };
    }
    return null;
  };
  return search(snap.ai.threads) ?? search(snap.files.flatMap((f) => f.hunks.flatMap((h) => h.threads)));
}

function locateMessage(
  snap: AppSnapshot,
  messageId: string,
): { thread: ThreadSnapshot; isRoot: boolean } | null {
  const search = (threads: ThreadSnapshot[]) => {
    for (const t of threads) {
      if (t.id === messageId || t.root.id === messageId) return { thread: t, isRoot: true };
      if (t.replies.some((r) => r.id === messageId)) return { thread: t, isRoot: false };
    }
    return null;
  };
  return search(snap.ai.threads) ?? search(snap.files.flatMap((f) => f.hunks.flatMap((h) => h.threads)));
}

function pendingFromThread(
  thread: ThreadSnapshot,
  filePath: string,
  hunkIdx: number,
  viewIdentity: string,
): OptimisticThread {
  return { id: thread.id, hunkIdx, filePath, thread, viewIdentity };
}

function bumpFindingCounts(snap: AppSnapshot, finding: FlatFinding, delta: number): void {
  const clamp = (n: number) => Math.max(0, n + delta);
  if (finding.severity === "high") snap.ai.high = clamp(snap.ai.high);
  else if (finding.severity === "med") snap.ai.med = clamp(snap.ai.med);
  else snap.ai.low = clamp(snap.ai.low);
  const file = snap.files.find((f) => f.path === finding.file);
  if (file) file.finding_count = clamp(file.finding_count);
}

function pillarPaths(snap: AppSnapshot, pillarId: string): string[] {
  const pillar = snap.tour?.pillars.find((p) => p.id === pillarId);
  if (!pillar) return [];
  const paths: string[] = [];
  for (const f of pillar.files) {
    paths.push(f.path);
    for (const rel of f.related ?? []) paths.push(rel.path);
  }
  return paths;
}

function syncPillarReviewedCount(
  snap: AppSnapshot,
  pillarId: string,
  files: { path: string; was: boolean }[],
): void {
  const pillar = snap.tour?.pillars.find((p) => p.id === pillarId);
  if (!pillar) return;
  pillar.reviewedCount = files.filter((row) => {
    const file = snap.files.find((f) => f.path === row.path);
    return file?.reviewed === true;
  }).length;
}

function youMessage(id: string, text: string, nowIso: string): ThreadMessage {
  return {
    id,
    author: "you",
    kind: "you",
    timestamp: nowIso,
    body_markdown: text,
    origin: "thread_reply",
    editable: true,
    deletable: true,
  };
}

export function buildOptimisticOp(
  command: string,
  args: Record<string, unknown>,
  snap: AppSnapshot,
  opts?: { nowIso?: string; id?: string },
): OptimisticOp | null {
  if (!isOptimisticCommand(command)) return null;
  const viewIdentity = snapshotViewIdentity(snap);
  const nowIso = opts?.nowIso ?? new Date().toISOString();
  const id =
    opts?.id ??
    nextOptimisticId(command === "add_ui_annotation" ? { prefix: "ui" } : undefined);

  if (isAddThreadCommand(command)) {
    const parsed = parseAddThreadArgs(command, args);
    if (!parsed) return null;
    const filePath = parsed.file || snap.files[snap.selected_file]?.path || "";
    const pending = buildOptimisticThread(command, parsed, viewIdentity, filePath, {
      nowIso,
      id: opts?.id,
    });
    return { type: "add-thread", id: pending.id, viewIdentity, pending };
  }

  if (command === "reply_to_thread") {
    const parentId = asString(pick(args, "parentId", "parent_id"));
    const text = asString(args.text)?.trim() ?? "";
    if (!parentId || !text || !locateRoot(snap, parentId)) return null;
    return {
      type: "reply",
      id,
      viewIdentity,
      parentId,
      reply: youMessage(id, text, nowIso),
    };
  }

  if (command === "resolve_thread") {
    const threadId = asString(args.id);
    const root = threadId ? locateRoot(snap, threadId) : null;
    if (!root || root.thread.resolved) return null;
    return { type: "resolve", id, viewIdentity, threadId, prevResolved: root.thread.resolved };
  }

  if (command === "delete_thread") {
    const targetId = asString(args.id);
    if (!targetId) return null;
    const root = locateRoot(snap, targetId);
    if (root) {
      return {
        type: "delete-root",
        id,
        viewIdentity,
        pending: pendingFromThread(root.thread, root.filePath, root.hunkIdx, viewIdentity),
      };
    }
    const reply = locateReply(snap, targetId);
    if (!reply) return null;
    return {
      type: "delete-reply",
      id,
      viewIdentity,
      parentId: reply.parent.id,
      reply: { ...reply.reply },
      index: reply.index,
    };
  }

  if (command === "update_thread_message") {
    const messageId = asString(args.id);
    const nextBody = asString(args.body)?.trim() ?? "";
    if (!messageId || !nextBody) return null;
    const found = locateMessage(snap, messageId);
    if (!found) return null;
    const prevBody = found.isRoot
      ? found.thread.root.body_markdown
      : (found.thread.replies.find((r) => r.id === messageId)?.body_markdown ?? "");
    return {
      type: "edit",
      id,
      viewIdentity,
      threadId: found.thread.id,
      messageId,
      prevBody,
      nextBody,
    };
  }

  if (command === "dismiss_finding") {
    const findingId = asString(pick(args, "findingId", "finding_id"));
    if (!findingId) return null;
    const finding = snap.ai.findings.find((f) => f.id === findingId);
    if (!finding) return null;
    const threads: OptimisticThread[] = [];
    if (finding.thread_id) {
      const root = locateRoot(snap, finding.thread_id);
      if (root) {
        threads.push(pendingFromThread(root.thread, root.filePath, root.hunkIdx, viewIdentity));
      }
    }
    return { type: "dismiss-finding", id, viewIdentity, finding: { ...finding }, threads };
  }

  if (command === "promote_to_comment" || command === "promote_to_note") {
    const sourceId = asString(args.id);
    if (!sourceId) return null;
    const root = locateRoot(snap, sourceId);
    if (!root) return null;
    const body =
      asString(args.body)?.trim() ||
      root.thread.root.body_markdown;
    const destCommand = command === "promote_to_comment" ? "add_comment" : "add_note";
    const created = buildOptimisticThread(
      destCommand,
      {
        file: root.filePath,
        hunkIdx: root.hunkIdx,
        lineNum: root.thread.line,
        lineNumEnd: root.thread.line_end ?? null,
        text: body,
        side: root.thread.side === "LEFT" ? "LEFT" : "RIGHT",
      },
      viewIdentity,
      root.filePath,
      { nowIso, id: opts?.id },
    );
    return {
      type: "promote",
      id: created.id,
      viewIdentity,
      source: pendingFromThread(root.thread, root.filePath, root.hunkIdx, viewIdentity),
      created,
    };
  }

  if (command === "bulk_review_pillar" || command === "unbulk_review_pillar") {
    const pillarId = asString(pick(args, "pillarId", "pillar_id"));
    if (!pillarId) return null;
    const target = command === "bulk_review_pillar";
    const paths = pillarPaths(snap, pillarId);
    if (paths.length === 0) return null;
    const files = paths
      .map((path) => {
        const file = snap.files.find((f) => f.path === path);
        if (!file) return null;
        return { path, was: file.reviewed };
      })
      .filter((row): row is { path: string; was: boolean } => row !== null);
    if (files.length === 0) return null;
    return { type: "bulk-reviewed", id, viewIdentity, pillarId, target, files };
  }

  if (command === "add_ui_annotation") {
    const url = asString(args.url) ?? "";
    const text = asString(args.text)?.trim() ?? "";
    const bbox = args.bbox;
    if (!url || !text || !Array.isArray(bbox) || bbox.length !== 4) return null;
    const viewport = Array.isArray(args.viewport) ? args.viewport : [0, 0];
    const annotation: UiAnnotation = {
      id,
      url,
      selector: asString(args.selector),
      box_x: Number(bbox[0]) || 0,
      box_y: Number(bbox[1]) || 0,
      box_w: Number(bbox[2]) || 0,
      box_h: Number(bbox[3]) || 0,
      viewport_w: Number(viewport[0]) || 0,
      viewport_h: Number(viewport[1]) || 0,
      text,
      timestamp: nowIso,
      author: "you",
      screenshot_path: null,
      stale: false,
      element_context: asString(pick(args, "elementContext", "element_context")),
      dom_context: (args.domContext ?? args.dom_context ?? null) as UiAnnotation["dom_context"],
    };
    return { type: "add-annotation", id, viewIdentity, annotation };
  }

  if (command === "delete_ui_annotation") {
    const annId = asString(args.id);
    if (!annId) return null;
    const annotation = (snap.ui_annotations ?? []).find((a) => a.id === annId);
    if (!annotation) return null;
    return { type: "delete-annotation", id, viewIdentity, annotation: { ...annotation } };
  }

  return null;
}

export function applyOptimisticOp(snap: AppSnapshot, op: OptimisticOp): void {
  switch (op.type) {
    case "add-thread":
      applyOptimisticThread(snap, op.pending);
      return;
    case "reply":
      forEachThreadCopy(snap, op.parentId, (t) => {
        if (!t.replies.some((r) => r.id === op.reply.id)) {
          t.replies = [...t.replies, op.reply];
        }
      });
      return;
    case "resolve":
      forEachThreadCopy(snap, op.threadId, (t) => {
        t.resolved = true;
      });
      return;
    case "delete-root":
      removeOptimisticThread(snap, op.pending);
      return;
    case "delete-reply":
      forEachThreadCopy(snap, op.parentId, (t) => {
        t.replies = t.replies.filter((r) => r.id !== op.reply.id);
      });
      return;
    case "edit":
      forEachThreadCopy(snap, op.threadId, (t) => {
        if (t.id === op.messageId || t.root.id === op.messageId) {
          t.root = { ...t.root, body_markdown: op.nextBody };
        }
        t.replies = t.replies.map((r) =>
          r.id === op.messageId ? { ...r, body_markdown: op.nextBody } : r,
        );
      });
      return;
    case "dismiss-finding":
      if (snap.ai.findings.some((f) => f.id === op.finding.id)) {
        snap.ai.findings = snap.ai.findings.filter((f) => f.id !== op.finding.id);
        bumpFindingCounts(snap, op.finding, -1);
      }
      for (const thread of op.threads) {
        removeOptimisticThread(snap, thread);
      }
      return;
    case "promote":
      removeOptimisticThread(snap, op.source);
      applyOptimisticThread(snap, op.created);
      return;
    case "bulk-reviewed": {
      let delta = 0;
      for (const row of op.files) {
        const file = snap.files.find((f) => f.path === row.path);
        if (!file || file.reviewed === op.target) continue;
        file.reviewed = op.target;
        delta += op.target ? 1 : -1;
      }
      snap.reviewed_count = Math.max(0, snap.reviewed_count + delta);
      syncPillarReviewedCount(snap, op.pillarId, op.files);
      return;
    }
    case "add-annotation": {
      const list = snap.ui_annotations ?? [];
      if (!list.some((a) => a.id === op.annotation.id)) {
        snap.ui_annotations = [...list, op.annotation];
      }
      return;
    }
    case "delete-annotation":
      snap.ui_annotations = (snap.ui_annotations ?? []).filter(
        (a) => a.id !== op.annotation.id,
      );
      return;
  }
}

export function rollbackOptimisticOp(snap: AppSnapshot, op: OptimisticOp): void {
  switch (op.type) {
    case "add-thread":
      removeOptimisticThread(snap, op.pending);
      return;
    case "reply":
      forEachThreadCopy(snap, op.parentId, (t) => {
        t.replies = t.replies.filter((r) => r.id !== op.reply.id);
      });
      return;
    case "resolve":
      forEachThreadCopy(snap, op.threadId, (t) => {
        t.resolved = op.prevResolved;
      });
      return;
    case "delete-root":
      applyOptimisticThread(snap, op.pending);
      return;
    case "delete-reply":
      forEachThreadCopy(snap, op.parentId, (t) => {
        if (t.replies.some((r) => r.id === op.reply.id)) return;
        const next = [...t.replies];
        next.splice(Math.min(op.index, next.length), 0, op.reply);
        t.replies = next;
      });
      return;
    case "edit":
      forEachThreadCopy(snap, op.threadId, (t) => {
        if (t.id === op.messageId || t.root.id === op.messageId) {
          t.root = { ...t.root, body_markdown: op.prevBody };
        }
        t.replies = t.replies.map((r) =>
          r.id === op.messageId ? { ...r, body_markdown: op.prevBody } : r,
        );
      });
      return;
    case "dismiss-finding":
      if (!snap.ai.findings.some((f) => f.id === op.finding.id)) {
        snap.ai.findings = [...snap.ai.findings, op.finding];
        bumpFindingCounts(snap, op.finding, 1);
      }
      for (const thread of op.threads) {
        applyOptimisticThread(snap, thread);
      }
      return;
    case "promote":
      removeOptimisticThread(snap, op.created);
      applyOptimisticThread(snap, op.source);
      return;
    case "bulk-reviewed": {
      let delta = 0;
      for (const row of op.files) {
        const file = snap.files.find((f) => f.path === row.path);
        if (!file || file.reviewed === row.was) continue;
        file.reviewed = row.was;
        delta += row.was ? 1 : -1;
      }
      snap.reviewed_count = Math.max(0, snap.reviewed_count + delta);
      syncPillarReviewedCount(snap, op.pillarId, op.files);
      return;
    }
    case "add-annotation":
      snap.ui_annotations = (snap.ui_annotations ?? []).filter(
        (a) => a.id !== op.annotation.id,
      );
      return;
    case "delete-annotation": {
      const list = snap.ui_annotations ?? [];
      if (!list.some((a) => a.id === op.annotation.id)) {
        snap.ui_annotations = [...list, op.annotation];
      }
      return;
    }
  }
}

export function reapplyOptimisticOps(snap: AppSnapshot, ops: OptimisticOp[]): void {
  const identity = snapshotViewIdentity(snap);
  for (const op of ops) {
    if (op.viewIdentity !== identity) continue;
    applyOptimisticOp(snap, op);
  }
}

export function optimisticInvokeArgs(
  command: string,
  args: Record<string, unknown>,
  op: OptimisticOp,
  view?: SnapshotViewParts,
): Record<string, unknown> {
  const withView = view ? { ...args, view } : { ...args };
  if (op.type === "add-thread") return { ...withView, id: op.pending.id };
  if (op.type === "add-annotation") return { ...withView, id: op.annotation.id };
  return withView;
}
