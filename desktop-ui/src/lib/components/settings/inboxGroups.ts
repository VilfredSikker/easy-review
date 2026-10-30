import type { ConfigFieldValue, ConfigHubField } from "$lib/types";

type BoolField = Extract<ConfigHubField, { kind: "bool" }>;

/**
 * Which card each inbox kind sits in. The engine's catalog is flat; the
 * grouping is a reading aid for this page only, so it lives here rather than in
 * the shared catalog the TUI also reads.
 */
const KIND_GROUPS: { title: string; kinds: string[] }[] = [
  {
    title: "Reviews",
    kinds: ["review_requested", "pr_review_received", "pr_review_changes_requested", "pr_review_approved"],
  },
  { title: "Conversation", kinds: ["pr_comment", "pr_comment_reply", "mention"] },
  { title: "CI & GitHub", kinds: ["ci_failed", "pr_merged", "pr_closed", "github_refresh_failed"] },
  { title: "AI", kinds: ["ai_review_done", "ai_review_failed", "ai_triage_done", "ai_triage_failed"] },
];

const OTHER_GROUP = "Other";

/** One inbox kind: whether it is listed in the inbox, and whether it raises a
 * system notification. The engine keeps the two apart, so the page does too. */
export interface InboxKindRow {
  kind: string;
  label: string;
  description: string;
  show: BoolField | null;
  notify: BoolField | null;
}

export interface InboxGroup {
  title: string;
  rows: InboxKindRow[];
}

/**
 * Group the engine's `inbox.show.*` / `inbox.notify.*` fields by kind. A kind
 * this table does not name goes to a trailing "Other" group, so a kind the
 * engine adds later still gets a switch.
 */
export function groupInboxFields(fields: ConfigHubField[]): InboxGroup[] {
  const rows = new Map<string, InboxKindRow>();
  for (const field of fields) {
    if (field.kind !== "bool") continue;
    const match = /^inbox\.(show|notify)\.(.+)$/.exec(field.key);
    if (!match) continue;
    const [, which, kind] = match;
    const row = rows.get(kind) ?? { kind, label: field.label, description: "", show: null, notify: null };
    if (which === "show") row.show = field;
    else row.notify = field;
    // The engine words each description for its own switch ("Show when…",
    // "Notify when…"); the row names the event once, so strip the verb.
    if (!row.description) {
      const event = field.description.replace(/^(Show|Notify) when /, "");
      row.description = event.charAt(0).toUpperCase() + event.slice(1);
    }
    rows.set(kind, row);
  }
  const placed = new Set<string>();
  const groups: InboxGroup[] = KIND_GROUPS.map((g) => ({
    title: g.title,
    rows: g.kinds.flatMap((kind) => {
      const row = rows.get(kind);
      if (!row) return [];
      placed.add(kind);
      return [row];
    }),
  })).filter((g) => g.rows.length > 0);
  const rest = [...rows.values()].filter((row) => !placed.has(row.kind));
  if (rest.length > 0) groups.push({ title: OTHER_GROUP, rows: rest });
  return groups;
}

function switchesOf(rows: InboxKindRow[]): BoolField[] {
  return rows.flatMap((row) => [row.show, row.notify].filter((f): f is BoolField => f !== null));
}

export function inboxOnCount(groups: InboxGroup[]): { on: number; total: number } {
  const all = switchesOf(groups.flatMap((g) => g.rows));
  return { on: all.filter((f) => f.value).length, total: all.length };
}

/** Whether the group's All/None button turns everything on (some switch is off)
 * or everything off (every switch is on). */
export function groupAllOn(group: InboxGroup): boolean {
  return switchesOf(group.rows).every((f) => f.value);
}

/**
 * The patches that bring every switch in the group to `value`. Only switches
 * that differ are sent: each patch is a config write and a snapshot rebuild.
 */
export function groupPatches(group: InboxGroup, value: boolean): [string, ConfigFieldValue][] {
  return switchesOf(group.rows)
    .filter((f) => f.value !== value)
    .map((f) => [f.key, value]);
}
