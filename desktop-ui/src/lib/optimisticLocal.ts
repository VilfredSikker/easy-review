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
  FindingResponseSnapshot,
  FlatFinding,
  InboxItemSnapshot,
  PrInfo,
  ProjectSnapshot,
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
  // Finding-thread actions: view-scoped, same contract as dismiss_finding.
  "remove_finding_thread",
  "promote_finding_to_comment",
  "delete_finding_response",
  "update_finding_response",
  "reply_to_finding",
  // App-wide chrome (global ops, see `isGlobalOp`): they survive tab switches
  // and are confirmed by a chrome-only snapshot merged onto the current view.
  "mark_inbox_item_read",
  "mark_inbox_items_read",
  "mark_all_inbox_read",
  "clear_read_inbox_items",
  "clear_inbox_items",
  "save_pr",
  "unsave_pr",
  "dismiss_remote_pr",
  "undismiss_remote_pr",
] as const;

export type OptimisticCommand = (typeof OPTIMISTIC_COMMANDS)[number];

export function isOptimisticCommand(command: string): command is OptimisticCommand {
  return (OPTIMISTIC_COMMANDS as readonly string[]).includes(command);
}

/** Sidebar PR lists the backend filters `dismissed_prs` out of. */
export type DismissablePrList = "my_prs" | "prs_to_review" | "recently_merged";
const DISMISSABLE_PR_LISTS: readonly DismissablePrList[] = [
  "my_prs",
  "prs_to_review",
  "recently_merged",
];

/** Backend keeps at most this many saved PRs per project (`projects.rs`). */
export const MAX_SAVED_PRS = 50;

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
    }
  | {
      type: "remove-finding-thread";
      id: string;
      viewIdentity: string;
      findingId: string;
      threads: OptimisticThread[];
      prevThreadId: string | null;
    }
  | {
      type: "promote-finding";
      id: string;
      viewIdentity: string;
      finding: FlatFinding;
      threads: OptimisticThread[];
      created: OptimisticThread;
    }
  | {
      type: "delete-finding-response";
      id: string;
      viewIdentity: string;
      findingId: string;
      response: FindingResponseSnapshot;
      index: number;
    }
  | {
      type: "edit-finding-response";
      id: string;
      viewIdentity: string;
      findingId: string;
      responseId: string;
      prevBody: string;
      nextBody: string;
    }
  | {
      type: "reply-finding";
      id: string;
      viewIdentity: string;
      findingId: string;
      target:
        /** Append to the finding's existing thread. */
        | { kind: "existing"; parentId: string; reply: ThreadMessage }
        /** No thread yet: the backend creates one, so paint it. */
        | { kind: "new"; created: OptimisticThread; prevThreadId: string | null }
        /** AI-assisted: the backend adds its own "…thinking" row, nothing to paint. */
        | { kind: "ai" };
    }
  | {
      type: "inbox-read";
      id: string;
      viewIdentity: string;
      nowMs: number;
      /** Visible items that were unread when painted. Reapply only touches these. */
      ids: string[];
    }
  | {
      type: "inbox-clear";
      id: string;
      viewIdentity: string;
      /** Visible read items dropped when painted. */
      removed: InboxItemSnapshot[];
    }
  | {
      type: "saved-pr";
      id: string;
      viewIdentity: string;
      projectId: string;
      prNumber: number;
      target: boolean;
      pr: PrInfo;
      /** Position in `saved_prs` when painted, -1 when absent. */
      prevIndex: number;
    }
  | {
      type: "dismissed-pr";
      id: string;
      viewIdentity: string;
      projectId: string;
      prNumber: number;
      target: boolean;
      wasDismissed: boolean;
      /** Rows the paint removed from the sidebar lists (dismiss only). */
      removed: { list: DismissablePrList; index: number; pr: PrInfo }[];
    };

const GLOBAL_OP_TYPES: ReadonlySet<OptimisticOp["type"]> = new Set<OptimisticOp["type"]>([
  "inbox-read",
  "inbox-clear",
  "saved-pr",
  "dismissed-pr",
]);

/**
 * Global ops mutate app-wide chrome (inbox, project pins) rather than the
 * active review view. They reapply on every snapshot regardless of view
 * identity, never roll back because the view changed, and are confirmed by a
 * chrome-only snapshot merged onto whatever view is showing.
 */
export function isGlobalOp(op: OptimisticOp): boolean {
  return GLOBAL_OP_TYPES.has(op.type);
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function asNumber(value: unknown): number | null {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string" && value.trim() !== "") {
    const n = Number(value);
    if (Number.isFinite(n)) return n;
  }
  return null;
}

function asStringArray(value: unknown): string[] | null {
  if (!Array.isArray(value)) return null;
  return value.filter((v): v is string => typeof v === "string");
}

function pick(args: Record<string, unknown>, camel: string, snake: string): unknown {
  return args[camel] ?? args[snake];
}

function findProject(snap: AppSnapshot, projectId: string): ProjectSnapshot | undefined {
  return (snap.projects ?? []).find((p) => p.id === projectId);
}

function findPrInProject(project: ProjectSnapshot, prNumber: number): PrInfo | undefined {
  const lists: (PrInfo[] | undefined)[] = [
    project.saved_prs,
    project.my_prs,
    project.prs_to_review,
    project.recent_prs,
    project.recently_merged,
  ];
  for (const list of lists) {
    const hit = list?.find((p) => p.number === prNumber);
    if (hit) return hit;
  }
  return undefined;
}

/**
 * Stub for a PR the snapshot does not know yet, mirroring the backend's
 * `minimal_pr_info` (empty strings, not `null`s). The backend drops saved
 * entries with an empty title, so never produce one.
 */
export function minimalPrInfo(number: number, title: string): PrInfo {
  return {
    number,
    title: title.trim() || `PR #${number}`,
    head_ref: "",
    // The wire format really is "" for a stub; the union type is what real rows carry.
    state: "" as PrInfo["state"],
    is_draft: false,
    author: "",
    assignees: [],
    reviewers: [],
    checks_state: null,
    review_decision: null,
    merged_at: null,
    approved_by_me: false,
    base_ref: "",
    head_oid: "",
    updated_at: "",
  };
}

/** The backend derives `inbox_unread_count` from the visible list; do the same. */
function recountInboxUnread(snap: AppSnapshot): void {
  snap.inbox_unread_count = (snap.inbox_items ?? []).filter((i) => i.read_at_ms == null).length;
}

function findFinding(snap: AppSnapshot, findingId: string): FlatFinding | undefined {
  return snap.ai.findings.find((f) => f.id === findingId);
}

function findingThreads(
  snap: AppSnapshot,
  finding: FlatFinding,
  viewIdentity: string,
): OptimisticThread[] {
  if (!finding.thread_id) return [];
  const root = locateRoot(snap, finding.thread_id);
  if (!root) return [];
  return [pendingFromThread(root.thread, root.filePath, root.hunkIdx, viewIdentity)];
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
    if (!threadId || !root || root.thread.resolved) return null;
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

  if (command === "remove_finding_thread") {
    const findingId = asString(pick(args, "findingId", "finding_id"));
    const finding = findingId ? findFinding(snap, findingId) : undefined;
    if (!findingId || !finding) return null;
    // Built even when no thread is located: the paint is then a no-op but the
    // backend still drops note threads the snapshot never surfaces.
    return {
      type: "remove-finding-thread",
      id,
      viewIdentity,
      findingId,
      threads: findingThreads(snap, finding, viewIdentity),
      prevThreadId: finding.thread_id,
    };
  }

  if (command === "promote_finding_to_comment") {
    const findingId = asString(pick(args, "findingId", "finding_id"));
    const finding = findingId ? findFinding(snap, findingId) : undefined;
    if (!findingId || !finding) return null;
    const body = asString(args.body)?.trim() || finding.message_markdown || finding.title;
    const created = buildOptimisticThread(
      "add_comment",
      {
        file: finding.file,
        hunkIdx: finding.hunk_index ?? 0,
        lineNum: finding.line,
        lineNumEnd: null,
        text: body,
        side: "RIGHT",
      },
      viewIdentity,
      finding.file,
      { nowIso, id: opts?.id },
    );
    return {
      type: "promote-finding",
      id: created.id,
      viewIdentity,
      finding: { ...finding },
      threads: findingThreads(snap, finding, viewIdentity),
      created,
    };
  }

  if (command === "delete_finding_response") {
    const findingId = asString(pick(args, "findingId", "finding_id"));
    const responseId = asString(pick(args, "responseId", "response_id"));
    // "" is the pending "…thinking" row — nothing to delete.
    if (!findingId || !responseId) return null;
    const finding = findFinding(snap, findingId);
    const responses = finding?.responses ?? [];
    const index = responses.findIndex((r) => r.id === responseId);
    if (!finding || index < 0) return null;
    return {
      type: "delete-finding-response",
      id,
      viewIdentity,
      findingId,
      response: { ...responses[index] },
      index,
    };
  }

  if (command === "update_finding_response") {
    const findingId = asString(pick(args, "findingId", "finding_id"));
    const responseId = asString(pick(args, "responseId", "response_id"));
    const nextBody = asString(args.body)?.trim() ?? "";
    if (!findingId || !responseId || !nextBody) return null;
    const response = findFinding(snap, findingId)?.responses?.find((r) => r.id === responseId);
    if (!response) return null;
    return {
      type: "edit-finding-response",
      id,
      viewIdentity,
      findingId,
      responseId,
      prevBody: response.body_markdown,
      nextBody,
    };
  }

  if (command === "reply_to_finding") {
    const findingId = asString(pick(args, "findingId", "finding_id"));
    const finding = findingId ? findFinding(snap, findingId) : undefined;
    if (!findingId || !finding) return null;
    if (pick(args, "aiAssist", "ai_assist") === true) {
      return { type: "reply-finding", id, viewIdentity, findingId, target: { kind: "ai" } };
    }
    const text = asString(args.body)?.trim() ?? "";
    if (!text) return null;
    const root = finding.thread_id ? locateRoot(snap, finding.thread_id) : null;
    if (root) {
      return {
        type: "reply-finding",
        id,
        viewIdentity,
        findingId,
        target: {
          kind: "existing",
          parentId: root.thread.id,
          reply: youMessage(id, text, nowIso),
        },
      };
    }
    const created = buildOptimisticThread(
      "add_comment",
      {
        file: finding.file,
        hunkIdx: finding.hunk_index ?? 0,
        lineNum: finding.line,
        lineNumEnd: null,
        text,
        side: "RIGHT",
      },
      viewIdentity,
      finding.file,
      { nowIso, id: opts?.id },
    );
    return {
      type: "reply-finding",
      id: created.id,
      viewIdentity,
      findingId,
      target: { kind: "new", created, prevThreadId: finding.thread_id },
    };
  }

  if (
    command === "mark_inbox_item_read" ||
    command === "mark_inbox_items_read" ||
    command === "mark_all_inbox_read"
  ) {
    const unread = (snap.inbox_items ?? [])
      .filter((i) => i.read_at_ms == null)
      .map((i) => i.id);
    let ids = unread;
    if (command !== "mark_all_inbox_read") {
      const wanted = new Set(
        command === "mark_inbox_item_read"
          ? [asString(args.id) ?? ""]
          : (asStringArray(args.ids) ?? []),
      );
      ids = unread.filter((i) => wanted.has(i));
      // Nothing visible changes: skip the IPC too, the backend would be a no-op.
      if (ids.length === 0) return null;
    }
    // mark_all always runs: hidden-kind items are not in `inbox_items` but the
    // backend still marks them.
    return {
      type: "inbox-read",
      id,
      viewIdentity,
      nowMs: Date.parse(nowIso) || Date.now(),
      ids,
    };
  }

  if (command === "clear_read_inbox_items" || command === "clear_inbox_items") {
    // Mirrors `InboxState::clear_read_items`: unread items stay even when listed.
    const read = (snap.inbox_items ?? []).filter((i) => i.read_at_ms != null);
    let removed = read;
    if (command === "clear_inbox_items") {
      const wanted = new Set(asStringArray(args.ids) ?? []);
      removed = read.filter((i) => wanted.has(i.id));
      if (removed.length === 0) return null;
    }
    return { type: "inbox-clear", id, viewIdentity, removed: removed.map((i) => ({ ...i })) };
  }

  if (command === "save_pr" || command === "unsave_pr") {
    const projectId = asString(pick(args, "projectId", "project_id"));
    const prNumber = asNumber(pick(args, "prNumber", "pr_number"));
    if (!projectId || prNumber === null) return null;
    const project = findProject(snap, projectId);
    if (!project) return null;
    const prevIndex = (project.saved_prs ?? []).findIndex((p) => p.number === prNumber);
    const pr =
      findPrInProject(project, prNumber) ?? minimalPrInfo(prNumber, asString(args.title) ?? "");
    return {
      type: "saved-pr",
      id,
      viewIdentity,
      projectId,
      prNumber,
      target: command === "save_pr",
      pr: { ...pr },
      prevIndex,
    };
  }

  if (command === "dismiss_remote_pr" || command === "undismiss_remote_pr") {
    const projectId = asString(pick(args, "projectId", "project_id"));
    const prNumber = asNumber(pick(args, "prNumber", "pr_number"));
    if (!projectId || prNumber === null) return null;
    const project = findProject(snap, projectId);
    if (!project) return null;
    const target = command === "dismiss_remote_pr";
    const removed: { list: DismissablePrList; index: number; pr: PrInfo }[] = [];
    if (target) {
      for (const list of DISMISSABLE_PR_LISTS) {
        const rows = project[list] ?? [];
        const index = rows.findIndex((p) => p.number === prNumber);
        if (index >= 0) removed.push({ list, index, pr: { ...rows[index] } });
      }
    }
    return {
      type: "dismissed-pr",
      id,
      viewIdentity,
      projectId,
      prNumber,
      target,
      wasDismissed: (project.dismissed_prs ?? []).includes(prNumber),
      removed,
    };
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
    case "remove-finding-thread": {
      for (const thread of op.threads) removeOptimisticThread(snap, thread);
      const finding = findFinding(snap, op.findingId);
      if (finding) finding.thread_id = null;
      return;
    }
    case "promote-finding":
      if (snap.ai.findings.some((f) => f.id === op.finding.id)) {
        snap.ai.findings = snap.ai.findings.filter((f) => f.id !== op.finding.id);
        bumpFindingCounts(snap, op.finding, -1);
      }
      for (const thread of op.threads) removeOptimisticThread(snap, thread);
      applyOptimisticThread(snap, op.created);
      return;
    case "delete-finding-response": {
      const finding = findFinding(snap, op.findingId);
      if (!finding?.responses) return;
      finding.responses = finding.responses.filter((r) => r.id !== op.response.id);
      return;
    }
    case "edit-finding-response": {
      const finding = findFinding(snap, op.findingId);
      if (!finding?.responses) return;
      finding.responses = finding.responses.map((r) =>
        r.id === op.responseId ? { ...r, body_markdown: op.nextBody } : r,
      );
      return;
    }
    case "reply-finding": {
      const target = op.target;
      if (target.kind === "ai") return;
      if (target.kind === "existing") {
        forEachThreadCopy(snap, target.parentId, (t) => {
          if (!t.replies.some((r) => r.id === target.reply.id)) {
            t.replies = [...t.replies, target.reply];
          }
        });
        return;
      }
      applyOptimisticThread(snap, target.created);
      const finding = findFinding(snap, op.findingId);
      if (finding) finding.thread_id = target.created.id;
      return;
    }
    case "inbox-read": {
      const items = snap.inbox_items;
      if (!items) return;
      const wanted = new Set(op.ids);
      for (const item of items) {
        if (wanted.has(item.id) && item.read_at_ms == null) item.read_at_ms = op.nowMs;
      }
      recountInboxUnread(snap);
      return;
    }
    case "inbox-clear": {
      if (!snap.inbox_items) return;
      const wanted = new Set(op.removed.map((i) => i.id));
      snap.inbox_items = snap.inbox_items.filter(
        (i) => !(wanted.has(i.id) && i.read_at_ms != null),
      );
      recountInboxUnread(snap);
      return;
    }
    case "saved-pr": {
      const project = findProject(snap, op.projectId);
      if (!project) return;
      const list = project.saved_prs ?? [];
      if (!op.target) {
        project.saved_prs = list.filter((p) => p.number !== op.prNumber);
        return;
      }
      const at = list.findIndex((p) => p.number === op.prNumber);
      if (at === 0) return;
      // Prefer the snapshot's own row: once the backend has confirmed, it is
      // the hydrated one, and the stub must not replace it on a reapply.
      const keep = at >= 0 ? list[at] : op.pr;
      project.saved_prs = [keep, ...list.filter((p) => p.number !== op.prNumber)].slice(
        0,
        MAX_SAVED_PRS,
      );
      return;
    }
    case "dismissed-pr": {
      const project = findProject(snap, op.projectId);
      if (!project) return;
      const dismissed = new Set(project.dismissed_prs ?? []);
      if (op.target) {
        dismissed.add(op.prNumber);
        for (const list of DISMISSABLE_PR_LISTS) {
          project[list] = (project[list] ?? []).filter((p) => p.number !== op.prNumber);
        }
      } else {
        // The row itself is not in the snapshot; it returns with the poll.
        dismissed.delete(op.prNumber);
      }
      project.dismissed_prs = [...dismissed];
      return;
    }
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
    case "remove-finding-thread": {
      for (const thread of op.threads) applyOptimisticThread(snap, thread);
      const finding = findFinding(snap, op.findingId);
      if (finding) finding.thread_id = op.prevThreadId;
      return;
    }
    case "promote-finding":
      removeOptimisticThread(snap, op.created);
      for (const thread of op.threads) applyOptimisticThread(snap, thread);
      if (!snap.ai.findings.some((f) => f.id === op.finding.id)) {
        snap.ai.findings = [...snap.ai.findings, op.finding];
        bumpFindingCounts(snap, op.finding, 1);
      }
      return;
    case "delete-finding-response": {
      const finding = findFinding(snap, op.findingId);
      if (!finding) return;
      const list = finding.responses ?? [];
      if (list.some((r) => r.id === op.response.id)) return;
      const next = [...list];
      next.splice(Math.min(op.index, next.length), 0, op.response);
      finding.responses = next;
      return;
    }
    case "edit-finding-response": {
      const finding = findFinding(snap, op.findingId);
      if (!finding?.responses) return;
      finding.responses = finding.responses.map((r) =>
        r.id === op.responseId ? { ...r, body_markdown: op.prevBody } : r,
      );
      return;
    }
    case "reply-finding": {
      const target = op.target;
      if (target.kind === "ai") return;
      if (target.kind === "existing") {
        forEachThreadCopy(snap, target.parentId, (t) => {
          t.replies = t.replies.filter((r) => r.id !== target.reply.id);
        });
        return;
      }
      removeOptimisticThread(snap, target.created);
      const finding = findFinding(snap, op.findingId);
      if (finding) finding.thread_id = target.prevThreadId;
      return;
    }
    case "inbox-read": {
      const items = snap.inbox_items;
      if (!items) return;
      const wanted = new Set(op.ids);
      for (const item of items) {
        if (wanted.has(item.id)) item.read_at_ms = null;
      }
      recountInboxUnread(snap);
      return;
    }
    case "inbox-clear": {
      const list = snap.inbox_items ?? [];
      const present = new Set(list.map((i) => i.id));
      snap.inbox_items = [...list, ...op.removed.filter((i) => !present.has(i.id))];
      recountInboxUnread(snap);
      return;
    }
    case "saved-pr": {
      const project = findProject(snap, op.projectId);
      if (!project) return;
      const without = (project.saved_prs ?? []).filter((p) => p.number !== op.prNumber);
      if (op.target || op.prevIndex < 0) {
        project.saved_prs = without;
        return;
      }
      without.splice(Math.min(op.prevIndex, without.length), 0, op.pr);
      project.saved_prs = without;
      return;
    }
    case "dismissed-pr": {
      const project = findProject(snap, op.projectId);
      if (!project) return;
      const dismissed = new Set(project.dismissed_prs ?? []);
      if (op.target) {
        if (!op.wasDismissed) dismissed.delete(op.prNumber);
        for (const row of op.removed) {
          const rows = (project[row.list] ?? []).filter((p) => p.number !== row.pr.number);
          rows.splice(Math.min(row.index, rows.length), 0, row.pr);
          project[row.list] = rows;
        }
      } else if (op.wasDismissed) {
        dismissed.add(op.prNumber);
      }
      project.dismissed_prs = [...dismissed];
      return;
    }
  }
}

export function reapplyOptimisticOps(snap: AppSnapshot, ops: OptimisticOp[]): void {
  const identity = snapshotViewIdentity(snap);
  for (const op of ops) {
    if (!isGlobalOp(op) && op.viewIdentity !== identity) continue;
    applyOptimisticOp(snap, op);
  }
}

export function optimisticInvokeArgs(
  command: string,
  args: Record<string, unknown>,
  op: OptimisticOp,
  view?: SnapshotViewParts,
): Record<string, unknown> {
  // Global handlers take no `view`: the write is not scoped to a review view.
  const withView = view && !isGlobalOp(op) ? { ...args, view } : { ...args };
  if (op.type === "add-thread") return { ...withView, id: op.pending.id };
  if (op.type === "add-annotation") return { ...withView, id: op.annotation.id };
  // The backend reuses the painted id (comment_id_override) so the confirm
  // lands on the same thread/reply instead of swapping ids under the cursor.
  if (op.type === "reply-finding" && op.target.kind !== "ai") return { ...withView, id: op.id };
  return withView;
}
