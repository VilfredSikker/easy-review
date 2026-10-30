import type { ConfigHubField, SettingsTab } from "$lib/types";

export interface SettingsTabMeta {
  id: SettingsTab;
  label: string;
  blurb: string;
  /** Mono glyph in the left nav. */
  icon: string;
}

export const SETTINGS_TABS: SettingsTabMeta[] = [
  {
    id: "general",
    label: "General",
    blurb: "Appearance, watched paths and inbox. Shared by the app and the terminal.",
    icon: "◐",
  },
  {
    id: "ai",
    label: "AI",
    blurb: "Default provider and model, the agent command, and how many agents run at once.",
    icon: "✦",
  },
  {
    id: "review",
    label: "Review",
    blurb: "Commands the review runs, and the risk rules that rank changed files.",
    icon: "◎",
  },
  {
    id: "projects",
    label: "Projects",
    blurb: "Per-project triage, size limits and review ignore patterns.",
    icon: "▤",
  },
  { id: "terminal", label: "Terminal", blurb: "TUI (er) only: view modes, display and key hints.", icon: "›_" },
];

export type ConfigValueField = Exclude<ConfigHubField, { kind: "section" }>;

export interface FieldSection {
  title: string | null;
  fields: ConfigValueField[];
}

/** Split the engine's flat field list at its section markers. */
export function sectionsOf(fields: ConfigHubField[]): FieldSection[] {
  const out: FieldSection[] = [];
  let current: FieldSection = { title: null, fields: [] };
  for (const field of fields) {
    if (field.kind === "section") {
      if (current.fields.length > 0) out.push(current);
      current = { title: field.title, fields: [] };
    } else {
      current.fields.push(field);
    }
  }
  if (current.fields.length > 0) out.push(current);
  return out;
}

/** Look up fields by key, for the tabs that lay their rows out by hand. */
export function fieldMap(fields: ConfigHubField[]): Map<string, ConfigValueField> {
  const out = new Map<string, ConfigValueField>();
  for (const field of fields) {
    if (field.kind !== "section") out.set(field.key, field);
  }
  return out;
}

/** The fields in `keys` order that the engine actually sent. A key the engine
 * dropped disappears rather than rendering an empty row. */
export function pickFields(map: Map<string, ConfigValueField>, keys: readonly string[]): ConfigValueField[] {
  return keys.flatMap((key) => {
    const field = map.get(key);
    return field ? [field] : [];
  });
}

/** Keys the hand-built General, AI and Review layouts place themselves. */
export const CLAIMED_KEY_PREFIXES = [
  "display.theme",
  "commands.",
  "summary.push_to_pr",
  "agent.",
  "ai_hub.max_concurrent_",
  "features.model_discovery",
  "watched.",
  "inbox.",
] as const;

function isClaimed(key: string): boolean {
  return CLAIMED_KEY_PREFIXES.some((prefix) => key === prefix || key.startsWith(prefix));
}

/**
 * The engine's general sections minus every field a hand-built layout already
 * shows. General renders what is left, so a section or key the engine adds
 * later lands somewhere visible instead of in no tab at all.
 */
export function unclaimedSections(fields: ConfigHubField[]): FieldSection[] {
  return sectionsOf(fields)
    .map((section) => ({ ...section, fields: section.fields.filter((f) => !isClaimed(f.key)) }))
    .filter((section) => section.fields.length > 0);
}

/** A cycle whose every option is an integer reads as a count, so it gets a stepper. */
export function isNumericCycle(options: string[]): boolean {
  return options.length > 1 && options.every((o) => /^-?\d+$/.test(o));
}

/**
 * The option one step from `value`, or null at either end. Steps along the
 * engine's own option list so its range (1..=16 for concurrency and tab width)
 * stays the only one.
 */
export function stepOption(options: string[], value: string, delta: 1 | -1): string | null {
  const index = options.indexOf(value);
  if (index < 0) return options[0] ?? null;
  const next = options[index + delta];
  return next ?? null;
}

/** Whether any of `texts` contains the query, case-insensitively. An empty
 * query matches everything. */
export function matchesQuery(query: string, ...texts: (string | null | undefined)[]): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  return texts.some((t) => !!t && t.toLowerCase().includes(q));
}

/** Searchable text per tab: engine fields plus the rows each tab builds by hand. */
export type SettingsSearchIndex = Record<SettingsTab, string[]>;

const HAND_BUILT_TERMS: SettingsSearchIndex = {
  general: ["Theme", "Appearance", "Send test notification", "Uninstall"],
  ai: [
    "Provider",
    "Model",
    "Effort",
    "Refresh models",
    "Edit providers",
    "Model groups",
    "Fast",
    "Standard",
    "Advanced",
    "Concurrency",
  ],
  review: ["Review commands", "Risk rules", "Propose rules", "Changed files"],
  projects: ["Location", "Run triage", "Size limit", "Review ignore patterns", "Ignore in AI reviews"],
  terminal: [],
};

function fieldTerms(fields: ConfigHubField[]): string[] {
  return fields.flatMap((f) => {
    if (f.kind === "section") return [f.title];
    if (f.kind === "bool" || f.kind === "cycle" || f.kind === "text") return [f.label, f.description];
    return [f.label];
  });
}

export function buildSearchIndex(general: ConfigHubField[], terminal: ConfigHubField[]): SettingsSearchIndex {
  const sections = sectionsOf(general);
  const termsIn = (prefixes: string[]) =>
    fieldTerms(
      sections.flatMap((s) => s.fields).filter((f) => prefixes.some((p) => f.key.startsWith(p))),
    );
  return {
    general: [
      ...HAND_BUILT_TERMS.general,
      ...termsIn(["display.", "watched.", "inbox."]),
      ...fieldTerms(unclaimedSections(general).flatMap((s) => s.fields)),
    ],
    ai: [...HAND_BUILT_TERMS.ai, ...termsIn(["agent.", "ai_hub.", "features.model_discovery"])],
    review: [...HAND_BUILT_TERMS.review, ...termsIn(["commands.", "summary."])],
    projects: HAND_BUILT_TERMS.projects,
    terminal: fieldTerms(terminal),
  };
}

/** How many indexed terms on each tab match, so the nav can point at the tab
 * holding a setting the open tab does not. */
export function tabMatchCounts(index: SettingsSearchIndex, query: string): Record<SettingsTab, number> {
  const count = (terms: string[]) => (query.trim() ? terms.filter((t) => matchesQuery(query, t)).length : 0);
  return {
    general: count(index.general),
    ai: count(index.ai),
    review: count(index.review),
    projects: count(index.projects),
    terminal: count(index.terminal),
  };
}

/** Display text for an engine option such as `new-and-push` or `contrast-dark`. */
export function optionLabel(option: string): string {
  if (option === "xhigh") return "XHigh";
  return option
    .split("-")
    .map((part) => part.charAt(0).toUpperCase() + part.slice(1))
    .join(" ");
}

/** The page's text input: mono, row height, accent focus ring. */
export const INPUT_CLASS =
  "h-7 px-[9px] rounded-[7px] border border-border bg-bg text-fg text-xs font-mono outline-none min-w-0 flex-1 placeholder:text-muted focus:border-accent/60";
