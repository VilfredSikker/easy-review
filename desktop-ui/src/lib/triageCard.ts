import type { TriagePriorityFileSnapshot, TriageSnapshot } from "$lib/types";
import type { PillTone } from "$lib/components/ui/Pill.svelte";

/**
 * What the triage card shows before anything is opened, and what its one
 * button does: the pills, the one-line teasers of its collapsed rows, and the
 * verdict-to-command mapping. Everything here is a pure read of the snapshot,
 * so the card's first impression is testable without mounting it.
 */

const VERDICT_LABELS: Record<string, string> = {
  general: "General review",
  expert: "Expert review",
  arena: "Arena debate",
  professor: "Professor",
  skip: "Skip deep review",
};

export function verdictLabel(primary: string): string {
  return VERDICT_LABELS[primary] ?? primary;
}

export type TriageGuard = NonNullable<TriageSnapshot["guard"]>;

/** The guard, only when the agent cited where it is checked (ADR 0039). */
export function evidencedGuard(triage: TriageSnapshot): TriageGuard | null {
  const guard = triage.guard;
  return guard && guard.evidence ? guard : null;
}

export interface TriagePill {
  label: string;
  tone: PillTone;
  /** Hover text; the pill itself stays one or two words. */
  title?: string;
}

const RISK_TONE: Record<string, PillTone> = {
  high: "risk-high",
  medium: "risk-med",
  low: "risk-low",
};

/** `isolated` is the good case and `broad` the one to slow down for; `contained` is the norm. */
const REACH_TONE: Record<string, PillTone> = {
  broad: "warning",
  contained: "neutral",
  isolated: "success",
};

/**
 * The pills in reading order: how risky, how far it reaches, whether a guard
 * keeps it off, how sure the agent is, and how big the diff is. A signal the
 * triage did not record gets no pill.
 */
export function summaryPills(triage: TriageSnapshot): TriagePill[] {
  const pills: TriagePill[] = [];

  if (triage.approx_risk) {
    pills.push({
      label: `${triage.approx_risk} risk`,
      tone: RISK_TONE[triage.approx_risk] ?? "neutral",
      title: "Approximate risk of the whole diff",
    });
  }

  const reach = triage.reach ?? "unknown";
  if (reach !== "unknown") {
    pills.push({
      label: `${reach} reach`,
      tone: REACH_TONE[reach] ?? "neutral",
      title: "How much existing code the change touches",
    });
  }

  const guard = evidencedGuard(triage);
  if (guard) {
    const what = guard.name ? `${guard.kind} ${guard.name}` : guard.kind;
    pills.push({
      label: "guarded",
      tone: "success",
      title: `${what} · ${guard.evidence}`,
    });
  }

  if (triage.confidence) {
    pills.push({
      label: `${triage.confidence} confidence`,
      tone: "neutral",
      title: "How sure the triage is of its verdict",
    });
  }

  const files = triage.files_changed;
  if (files > 0) {
    pills.push({ label: `${files} ${files === 1 ? "file" : "files"}`, tone: "neutral" });
  }

  return pills;
}

/**
 * What the follow-up button does for a verdict. `arena` opens the launcher;
 * `skip` and an unknown verdict do nothing.
 */
export type FollowUp =
  | { kind: "command"; command: string; args: Record<string, unknown> }
  | { kind: "arena" };

export function followUpFor(triage: TriageSnapshot, scope: string): FollowUp | null {
  switch (triage.verdict_primary) {
    case "general":
      return { kind: "command", command: "run_ai_review", args: { scope } };
    case "expert": {
      // An expert verdict with no named expert still needs a reviewer; security
      // is the lens the triage prompt weights most.
      const kinds =
        triage.experts.length > 0
          ? triage.experts.map((id) => `expert:${id}`)
          : ["expert:security"];
      return {
        kind: "command",
        command: "run_ai_scoped_review",
        args: { scope, paths: [], reviewerKinds: kinds },
      };
    }
    case "professor":
      return {
        kind: "command",
        command: "run_ai_professor_review",
        args: { scope, focusPrompt: null },
      };
    case "arena":
      return { kind: "arena" };
    default:
      return null;
  }
}

/** Whether the reach row has anything to show beyond its pill. */
export function hasReachDetail(triage: TriageSnapshot): boolean {
  if ((triage.reach ?? "unknown") === "unknown") return false;
  return (
    Boolean(triage.reach_reason) ||
    (triage.touch_points ?? []).length > 0 ||
    evidencedGuard(triage) !== null
  );
}

/** The reach row's teaser: the reason, or the first touch point when the agent gave none. */
export function reachPreview(triage: TriageSnapshot): string {
  return previewLine(triage.reach_reason ?? "") || (triage.touch_points ?? [])[0] || "";
}

const HORIZONTAL_RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const CODE_FENCE = /^\s*(`{3,}|~{3,})/;
const BLOCK_PREFIX = /^\s*(#{1,6}\s+|[-*+]\s+|\d+\.\s+|>\s*)+/;

/**
 * The first line of a markdown text with its markup stripped, for a collapsed
 * row's teaser. Rules and fences are skipped, since they carry no words.
 * Underscores stay: `snake_case` identifiers are common in these texts and
 * emphasis with underscores is not.
 */
export function previewLine(text: string): string {
  for (const raw of text.split("\n")) {
    if (HORIZONTAL_RULE.test(raw) || CODE_FENCE.test(raw)) continue;
    const line = stripInline(raw);
    if (line) return line;
  }
  return "";
}

/** A private-use character marks a held code span; it never occurs in agent text. */
const HOLD = String.fromCharCode(0xe000);

function stripInline(raw: string): string {
  // Code spans keep their text verbatim: `src/**/*.ts` must not lose its stars.
  const code: string[] = [];
  const held = raw.replace(/`([^`]*)`/g, (_, span: string) => `${HOLD}${code.push(span) - 1}${HOLD}`);
  const stripped = held
    .replace(BLOCK_PREFIX, "")
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .replace(/[`*]/g, "");
  // Every held span is HOLD index HOLD, so the odd parts of a split are indices.
  return stripped
    .split(HOLD)
    .map((part, i) => (i % 2 === 1 ? (code[Number(part)] ?? "") : part))
    .join("")
    .trim();
}

export function basename(path: string): string {
  const i = path.lastIndexOf("/");
  return i === -1 ? path : path.slice(i + 1);
}

/** The top priority file and why, as the Priority files row's teaser. */
export function filesPreview(files: TriagePriorityFileSnapshot[]): string {
  const top = files[0];
  if (!top) return "";
  return top.reason ? `${basename(top.path)} · ${top.reason}` : top.path;
}

const PRIORITY_DOT: Record<string, string> = {
  high: "bg-risk-high",
  medium: "bg-risk-med",
  low: "bg-risk-low",
};

/** The dot before a priority file; `info` and anything unexpected read as muted. */
export function priorityDotClass(risk: string): string {
  return PRIORITY_DOT[risk] ?? "bg-muted";
}
