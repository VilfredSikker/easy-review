import type { ThreadSnapshot } from "./types";

/** Buttons shown on the card itself; everything else lives in the overflow menu. */
export type PrimaryAction = "askAi" | "elaborate" | "reply" | "sendToAgent" | "resolve" | "promote" | "delete";
export type OverflowAction = "copy" | "push" | "promoteNote" | "resolve" | "edit" | "validate" | "delete";

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
  resolved: boolean;
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
 * the overflow menu. A resolved thread keeps Reply, and Delete moves to the
 * overflow so it stays reachable.
 */
export function threadActions(t: ThreadActionInput): ThreadActions {
  return { primary: primaryActions(t), overflow: overflowActions(t) };
}

function primaryActions(t: ThreadActionInput): PrimaryAction[] {
  if (t.resolved) return ["reply"];
  if (t.kind === "comment") return ["reply", "resolve"];
  // Promote-to-comment needs a diff line for GitHub to accept (ADR 0041).
  const promote: PrimaryAction[] = t.promoted || t.documentAnchored ? [] : ["promote"];
  // A note is an instruction for a coding agent, so it can be handed to one.
  const agent: PrimaryAction[] = t.kind === "note" ? ["sendToAgent"] : [];
  return ["askAi", "elaborate", "reply", ...agent, ...promote, "delete"];
}

function overflowActions(t: ThreadActionInput): OverflowAction[] {
  const isLocal = t.kind !== "comment";
  const open = !t.resolved;
  const overflow: OverflowAction[] = ["copy"];
  if (!isLocal && !t.synced) overflow.push("push");
  if (t.kind === "question" && open) overflow.push("promoteNote");
  if (isLocal && open) overflow.push("resolve");
  if (t.rootEditable) overflow.push("edit");
  if (t.kind !== "question") overflow.push("validate");
  if (!isLocal || !open) overflow.push("delete");
  return overflow;
}

/** Tracked agent-command name the backend runs Send to agent under. */
export const SEND_TO_AGENT_COMMAND = "send-to-agent";

export interface AgentButton {
  label: string;
  title: string;
  disabled: boolean;
}

/**
 * Send to agent edits the working tree, so on a remote PR tab, which has none
 * and must not get one (ADR 0020), the button stays visible but disabled with
 * the reason. The backend runs one at a time per tab, so it also waits while
 * one runs.
 */
export function sendToAgentButton(o: { remotePr: boolean; running: boolean }): AgentButton {
  if (o.remotePr) {
    return {
      label: "Send to agent",
      title: "Off on a remote PR tab: the agent edits a working tree. Check the PR out locally to use it.",
      disabled: true,
    };
  }
  if (o.running) {
    return { label: "Agent working…", title: "An agent is already carrying out a note in this tab", disabled: true };
  }
  return {
    label: "Send to agent",
    title: "Hand this note to your agent CLI to carry out in the working tree (takes an agent slot)",
    disabled: false,
  };
}

/**
 * Elaborate rewrites the card so it stands on its own; it does not answer. The
 * tooltip says so per kind, so it is not mistaken for Ask AI.
 */
export function elaborateTitle(kind: ThreadSnapshot["kind"]): string {
  return kind === "note"
    ? "Rewrite this note into a self-contained instruction an agent can act on (local reply, does not answer)"
    : "Rewrite this question so it stands on its own, naming the code it is about (local reply, does not answer)";
}

/** One-click Ask AI answers with the kind's own prompt. */
export function askAiTitle(kind: ThreadSnapshot["kind"]): string {
  return kind === "note"
    ? "Ask AI whether this change is right and what it touches (local reply, not posted to GitHub)"
    : "Ask AI to answer this question from the code (local reply, not posted to GitHub)";
}
