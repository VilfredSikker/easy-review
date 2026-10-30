import type { AiSnapshot, AppSnapshot, TabSummary, UiAnnotation } from "./types";

/**
 * The one TS mirror of `ExportOpts` in `crates/er-engine/src/export.rs`
 * (camelCase over IPC). The `include*Ids` arrays are per-category allow-lists:
 * undefined exports the whole category.
 */
export type ExportOpts = {
  includeComments: boolean;
  includeQuestions: boolean;
  includeNotes: boolean;
  includeFindings: boolean;
  includeAnnotations: boolean;
  includeChecklist: boolean;
  onlyUnresolved: boolean;
  includeCommentIds?: string[];
  includeQuestionIds?: string[];
  includeNoteIds?: string[];
  includeFindingIds?: string[];
};

export type ExportCategory = "comments" | "questions" | "notes" | "findings" | "annotations" | "checklist";
/** The categories whose items can be left out one by one. Annotations and the
 *  checklist have no per-item allow-list in the engine. */
export type PickableCategory = "comments" | "questions" | "notes" | "findings";

export const EXPORT_CATEGORIES: readonly { key: ExportCategory; label: string }[] = [
  { key: "comments", label: "Comments" },
  { key: "questions", label: "Questions" },
  { key: "notes", label: "Notes" },
  { key: "findings", label: "AI findings" },
  { key: "annotations", label: "UI annotations" },
  { key: "checklist", label: "Review checklist" },
];

export const PICKABLE_CATEGORIES: readonly PickableCategory[] = ["comments", "questions", "notes", "findings"];

export function isPickable(category: ExportCategory): category is PickableCategory {
  return (PICKABLE_CATEGORIES as readonly string[]).includes(category);
}

export type ExportPrefs = {
  include: Record<ExportCategory, boolean>;
  onlyUnresolved: boolean;
};

export const DEFAULT_EXPORT_PREFS: ExportPrefs = {
  include: { comments: true, questions: true, notes: true, findings: true, annotations: true, checklist: true },
  onlyUnresolved: false,
};

/** Reads stored prefs, keeping defaults for anything missing or malformed so a
 *  pref written by an older build never blanks a category. */
export function parseExportPrefs(raw: string | null): ExportPrefs {
  const prefs: ExportPrefs = { include: { ...DEFAULT_EXPORT_PREFS.include }, onlyUnresolved: false };
  if (!raw) return prefs;
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return prefs;
  }
  if (!parsed || typeof parsed !== "object") return prefs;
  const obj = parsed as { include?: Record<string, unknown>; onlyUnresolved?: unknown };
  for (const { key } of EXPORT_CATEGORIES) {
    const v = obj.include?.[key];
    if (typeof v === "boolean") prefs.include[key] = v;
  }
  if (typeof obj.onlyUnresolved === "boolean") prefs.onlyUnresolved = obj.onlyUnresolved;
  return prefs;
}

export type ExportItem = { id: string; file: string; line: number | null; label: string; resolved: boolean };
export type ExportItems = Record<PickableCategory, ExportItem[]>;

function oneLine(text: string): string {
  return text.replace(/\s+/g, " ").trim();
}

/**
 * Every item the engine could export, per pickable category. Resolved and
 * arbiter-dropped findings arrive in their own snapshot lists but the engine
 * still exports them unless `onlyUnresolved` is on, so they belong here too.
 */
export function collectExportItems(ai: AiSnapshot | null | undefined): ExportItems {
  const threads = ai?.threads ?? [];
  const thread = (kind: "comment" | "question" | "note") =>
    threads
      .filter((t) => t.kind === kind)
      .map((t) => ({ id: t.id, file: t.file, line: t.line > 0 ? t.line : null, label: oneLine(t.root.body_markdown ?? ""), resolved: t.resolved }));
  const seen = new Set<string>();
  const findings: ExportItem[] = [];
  for (const [list, dropped] of [[ai?.findings ?? [], false], [ai?.resolved_findings ?? [], false], [ai?.dropped_findings ?? [], true]] as const) {
    for (const f of list) {
      if (seen.has(f.id)) continue;
      seen.add(f.id);
      findings.push({ id: f.id, file: f.file, line: f.line, label: f.title, resolved: f.resolved || dropped || f.confidence === "dropped" });
    }
  }
  return { comments: thread("comment"), questions: thread("question"), notes: thread("note"), findings };
}

/** The items a category row lists: resolved ones vanish under Only unresolved,
 *  matching what the engine leaves out. */
export function visibleItems(items: ExportItem[], onlyUnresolved: boolean): ExportItem[] {
  return onlyUnresolved ? items.filter((i) => !i.resolved) : items;
}

function allowList(items: ExportItem[], excluded: ReadonlySet<string>): string[] | undefined {
  if (!items.some((i) => excluded.has(i.id))) return undefined;
  return items.filter((i) => !excluded.has(i.id)).map((i) => i.id);
}

export function buildExportOpts(prefs: ExportPrefs, items: ExportItems, excluded: ReadonlySet<string>): ExportOpts {
  return {
    includeComments: prefs.include.comments,
    includeQuestions: prefs.include.questions,
    includeNotes: prefs.include.notes,
    includeFindings: prefs.include.findings,
    includeAnnotations: prefs.include.annotations,
    includeChecklist: prefs.include.checklist,
    onlyUnresolved: prefs.onlyUnresolved,
    includeCommentIds: allowList(items.comments, excluded),
    includeQuestionIds: allowList(items.questions, excluded),
    includeNoteIds: allowList(items.notes, excluded),
    includeFindingIds: allowList(items.findings, excluded),
  };
}

/**
 * Items the export will carry: picked annotations on the files, plus UI
 * annotations. The checklist is a summary of the review, not an item, so it
 * adds nothing — the same count the prototype shows.
 */
export function exportItemCount(
  prefs: ExportPrefs,
  items: ExportItems,
  excluded: ReadonlySet<string>,
  uiAnnotations: readonly UiAnnotation[],
): number {
  let n = 0;
  for (const key of PICKABLE_CATEGORIES) {
    if (!prefs.include[key]) continue;
    n += visibleItems(items[key], prefs.onlyUnresolved).filter((i) => !excluded.has(i.id)).length;
  }
  if (prefs.include.annotations) {
    // The engine reads stale annotations as resolved under Only unresolved.
    n += uiAnnotations.filter((a) => !(prefs.onlyUnresolved && a.stale)).length;
  }
  return n;
}

/** Item ids are only unique within one review, so exclusions are kept per tab.
 *  `idx` is not identity: closing a tab compacts the indices. */
export function exportTabKey(snapshot: AppSnapshot | null | undefined): string {
  const tab: TabSummary | undefined =
    snapshot?.tabs.find((t) => t.is_active) ?? snapshot?.tabs.find((t) => t.idx === snapshot.active_tab);
  if (!tab) return "";
  return JSON.stringify([tab.repo_root, tab.remote, tab.kind, tab.pr_number == null ? tab.branch : null, tab.pr_number]);
}

export function markdownLineCount(md: string): number {
  return md === "" ? 0 : md.split("\n").length;
}

export function previewSize(md: string): string {
  const bytes = new TextEncoder().encode(md).length;
  return `${markdownLineCount(md)} lines · ${(bytes / 1024).toFixed(1)} KB`;
}

function escapeHtml(s: string): string {
  return s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");
}

/**
 * Lightweight highlight of the raw export source: headings, quotes, checked
 * checklist items, inline code and bold. The text is escaped first, so the
 * result is safe for `{@html}` whatever the annotations contain.
 */
export function highlightMarkdown(md: string): string {
  return escapeHtml(md)
    .split("\n")
    .map((l) => {
      if (/^#{1,6} /.test(l)) return `<span class="mh">${l}</span>`;
      if (/^&gt;/.test(l)) return `<span class="mq">${l}</span>`;
      if (/^- \[x\]/i.test(l)) return `<span class="mc">${l}</span>`;
      return l;
    })
    .join("\n")
    .replace(/`([^`\n]+)`/g, '<span class="mcode">`$1`</span>')
    .replace(/\*\*([^*\n]+)\*\*/g, "<b>**$1**</b>");
}

/** `name:line` for an item row, with long basenames cut so the label keeps room. */
export function itemLocation(item: ExportItem): string {
  const name = item.file.split("/").pop() ?? item.file;
  const short = name.length > 16 ? `${name.slice(0, 14)}…` : name;
  return `${short}:${item.line ?? "—"}`;
}
