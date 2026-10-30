import type { ThreadSnapshot } from "./types";

/** Buttons shown on the card itself; everything else lives in the overflow menu. */
export type PrimaryAction = "askAi" | "elaborate" | "reply" | "resolve" | "promote" | "delete";
export type OverflowAction = "copy" | "push" | "promoteNote" | "edit" | "validate" | "delete";

export interface OverflowItem {
  id: string;
  label: string;
  title?: string;
  disabled?: boolean;
  danger?: boolean;
  onselect: () => void;
}

export interface ThreadActionInput {
  kind: ThreadSnapshot["kind"];
  synced: boolean;
  /** Already promoted to a GitHub comment. */
  promoted: boolean;
  /** Anchored on a document line outside the diff (ADR 0041). */
  documentAnchored: boolean;
  /** The root message is the reviewer's own, so it can be edited. */
  rootEditable: boolean;
}

export interface ThreadActions {
  primary: PrimaryAction[];
  overflow: OverflowAction[];
}

const KIND_STYLE: Record<ThreadSnapshot["kind"], { label: string; color: string }> = {
  comment: { label: "Comment", color: "var(--color-comment)" },
  question: { label: "Question", color: "var(--color-question)" },
  note: { label: "Note", color: "var(--color-note)" },
};

/** Chip label and token for a kind (ADR 0030): question purple, comment blue, note amber. */
export const kindStyle = (kind: ThreadSnapshot["kind"]) => KIND_STYLE[kind];

/** "L12", "L12–15", or null for a file-level thread. */
export function lineRangeLabel(line: number, lineEnd?: number | null): string | null {
  if (line <= 0) return null;
  return lineEnd && lineEnd > line ? `L${line}–${lineEnd}` : `L${line}`;
}

/** How a message's author reads on the card. */
export function authorLabel(kind: "you" | "human" | "ai", author: string): string {
  if (kind === "you") return "You";
  return kind === "ai" ? "Claude" : author;
}

/**
 * Which actions a thread card offers. A comment is public, so its row stays
 * short (Reply, Resolve). Questions and notes are private and carry the AI
 * actions and Promote. Nothing is dropped: the less common actions move into
 * the overflow menu.
 */
export function threadActions(t: ThreadActionInput): ThreadActions {
  const isLocal = t.kind !== "comment";
  const primary: PrimaryAction[] = isLocal ? ["askAi", "elaborate", "reply"] : ["reply", "resolve"];
  // Promote-to-comment needs a diff line for GitHub to accept (ADR 0041).
  if (isLocal && !t.promoted && !t.documentAnchored) primary.push("promote");
  if (isLocal) primary.push("delete");

  const overflow: OverflowAction[] = ["copy"];
  if (!isLocal && !t.synced) overflow.push("push");
  if (t.kind === "question") overflow.push("promoteNote");
  if (t.rootEditable) overflow.push("edit");
  if (t.kind !== "question") overflow.push("validate");
  if (!isLocal) overflow.push("delete");
  return { primary, overflow };
}
