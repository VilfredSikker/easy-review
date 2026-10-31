import type { AiProviderInfo, ConfigHubField, ProjectSnapshot, SettingsTab } from "$lib/types";
import { groupInboxFields } from "./inboxGroups";
import { tierLabel, type RiskFileRow, type RiskRuleRow } from "./riskRules";
import {
  fieldMap,
  matchesQuery,
  optionLabel,
  pickFields,
  sectionsOf,
  unclaimedSections,
  type ConfigValueField,
} from "./settingsModel";

/*
 * The search runs twice: each row filters itself on the text it renders, and
 * the nav counts matches on tabs that are not mounted. Both read the copy and
 * layouts below, so the nav never points at a tab whose rows hide the match.
 */

/** What one row shows: its label, its description, and the controls it
 *  carries that a reader might search for (a button's label). */
export interface RowCopy {
  label?: string;
  description?: string;
  keywords?: string[];
}

export function rowTerms(row: RowCopy): string[] {
  return [row.label, row.description, ...(row.keywords ?? [])].filter((t): t is string => !!t);
}

export type DescriptionOverrides = Record<string, string>;

export const WATCHED_DESCRIPTIONS: DescriptionOverrides = {
  "watched.diff_mode":
    "How to diff files under watched paths: by content, or against a snapshot taken when you open the tab.",
};

export const COMMAND_KEYS = [
  "commands.summary",
  "commands.test",
  "commands.lint",
  "commands.typecheck",
  "commands.security",
  "summary.push_to_pr",
] as const;

export const COMMAND_DESCRIPTIONS: DescriptionOverrides = {
  "summary.push_to_pr": "Write the review summary into the PR description.",
};

export const AGENT_KEYS = ["agent.command", "agent.args", "agent.effort"] as const;

export const AGENT_DESCRIPTIONS: DescriptionOverrides = {
  "agent.command": "The CLI that runs review agents.",
  "agent.args": "Arguments passed to it. {prompt} is replaced by the task.",
  "agent.effort": "Effort the agent CLI is asked for, separate from the default model's.",
};

export const CONCURRENCY_KEYS = [
  "ai_hub.max_concurrent_reviews",
  "ai_hub.max_concurrent_arena_reviews",
  "ai_hub.max_concurrent_agents",
] as const;

export const CONCURRENCY_DESCRIPTIONS: DescriptionOverrides = {
  "ai_hub.max_concurrent_reviews": "Review agents running at once across tabs. Extra reviews queue.",
  "ai_hub.max_concurrent_arena_reviews": "Reviewers an arena runs side by side, on a cap of their own.",
  "ai_hub.max_concurrent_agents":
    "The slot cap across both. It covers review agents only; other subprocesses take no slot.",
};

// The engine titles these for the TUI's own config hub; the page uses the
// prototype's sentence-case names and names each view by the key that opens it.
export const TERMINAL_TITLES: Record<string, string> = { Views: "View modes", "Key Hints": "Key hints" };

export const VIEW_KEY_DESCRIPTIONS: DescriptionOverrides = {
  "features.view_branch": "Key 1",
  "features.view_unstaged": "Key 2",
  "features.view_staged": "Key 3",
  "features.view_history": "Key 4",
  "features.view_conflicts": "Key 5",
  "features.view_hidden": "Key 6",
};

/** The text FieldRows renders for one engine field. A list-add row shows only
 *  its input, so its label is that row's keyword. */
export function fieldRowCopy(field: ConfigValueField, descriptions: DescriptionOverrides = {}): RowCopy {
  if (field.kind === "listAdd") return { keywords: [field.label] };
  if (field.kind === "listEntry") return { label: field.label };
  return { label: field.label, description: descriptions[field.key] ?? field.description };
}

const fieldsTerms = (fields: ConfigValueField[], descriptions: DescriptionOverrides = {}) =>
  fields.flatMap((f) => rowTerms(fieldRowCopy(f, descriptions)));

export const GENERAL_ROWS = {
  theme: { label: "Theme", description: "One token set for the desktop app and the TUI." },
  testNotification: {
    label: "Test notification",
    description: "Send a system notification to check that er has permission to show one.",
    keywords: ["Send test"],
  },
  uninstall: {
    label: "Uninstall Easy Review",
    description:
      "Removes config, review data, legacy cache and installed apps. Your repos are untouched; only Easy Review's own files are deleted.",
    keywords: ["Show what will be removed"],
  },
} satisfies Record<string, RowCopy>;

export const AI_ROWS = {
  noProviders: {
    label: "No providers configured",
    description: "Add a provider to pick the model standalone actions use. Settings are saved to managed app data.",
    keywords: ["Edit providers"],
  },
  provider: { label: "Provider", description: "Used by standalone actions: Ask AI, Elaborate, Professor." },
  model: { label: "Model" },
  effort: { label: "Effort / reasoning", description: "Auto uses the provider default." },
  discovery: {
    label: "Model discovery",
    description: "List models from provider CLIs, in the app and the terminal. Refresh to fetch the list now.",
    keywords: ["Refresh models"],
  },
  providers: {
    label: "Providers",
    description: "Add, edit or remove providers and their models.",
    keywords: ["Edit providers"],
  },
} satisfies Record<string, RowCopy>;

/** The model groups editor lays out cards rather than rows, so its section
 *  matches on these. */
export const MODEL_GROUP_KEYWORDS = ["Fast", "Standard", "Advanced", "Add model"];

export function riskRows(defaultTier: string) {
  return {
    empty: {
      label: "No rules declared",
      description: `Every file reads as ${tierLabel(defaultTier)} until a rule claims it. Add one, or let an agent propose a table for this repo.`,
    },
    help: {
      description: `Most specific wins: exact path, then glob, then file type. This repo's rules come before yours, and anything unclaimed reads as ${tierLabel(defaultTier)}.`,
      keywords: ["Add rule", "Propose rules"],
    },
  } satisfies Record<string, RowCopy>;
}

/** The rule table and the changed-files list sit outside any row. */
export function riskKeywords(rules: RiskRuleRow[], files: RiskFileRow[]): string[] {
  return [
    ...rules.flatMap((r) => [r.pattern, r.label ?? ""]),
    ...(files.length > 0 ? ["Changed files"] : []),
  ].filter(Boolean);
}

export const PROJECT_ROWS = {
  location: { label: "Location" },
  autoTriage: { label: "Auto-triage", description: "Triage new and updated open PRs while the app is open." },
  runTriage: { label: "Run triage", description: "Which PRs start a triage run." },
  ownPrs: { label: "Include my own PRs", description: "Also triage PRs you opened." },
  sizeLimit: {
    label: "Size limit",
    description: "Skip triage when the filtered diff is larger than this. Empty means no limit.",
  },
  ignore: {
    label: "Review ignore patterns",
    description: "Paths reviewers never see, in triage and full reviews. REVIEW.md on the base branch can add more.",
  },
} satisfies Record<string, RowCopy>;

/** The rows ProjectSettingsCard shows for `project`, under the same conditions. */
export function projectRows(project: ProjectSnapshot): RowCopy[] {
  const rows: RowCopy[] = [PROJECT_ROWS.location];
  if (project.remote) {
    rows.push(PROJECT_ROWS.autoTriage);
    if (project.auto_triage) rows.push(PROJECT_ROWS.runTriage, PROJECT_ROWS.ownPrs);
    rows.push(PROJECT_ROWS.sizeLimit);
  }
  rows.push({ ...PROJECT_ROWS.ignore, keywords: project.review_ignore_globs ?? [] });
  return rows;
}

/** Searchable text per tab, as the rows and section titles render it. */
export type SettingsSearchIndex = Record<SettingsTab, string[]>;

export interface SearchInputs {
  general: ConfigHubField[];
  terminal: ConfigHubField[];
  providers?: AiProviderInfo[];
  /** The project the Projects tab opens on. */
  project?: ProjectSnapshot | null;
  riskRules?: RiskRuleRow[];
  riskFiles?: RiskFileRow[];
  riskDefault?: string;
}

function generalTerms(general: ConfigHubField[]): string[] {
  const byKey = fieldMap(general);
  const theme = byKey.get("display.theme");
  const watched = general.flatMap((f) => (f.kind !== "section" && f.key.startsWith("watched.") ? [f] : []));
  const inbox = groupInboxFields(general);
  return [
    ...(theme?.kind === "cycle"
      ? ["Appearance", ...rowTerms({ ...GENERAL_ROWS.theme, keywords: theme.options.map(optionLabel) })]
      : []),
    ...(watched.length > 0 ? ["Watched paths", ...fieldsTerms(watched, WATCHED_DESCRIPTIONS)] : []),
    "Inbox",
    ...rowTerms(GENERAL_ROWS.testNotification),
    ...inbox.flatMap((g) => [g.title, ...g.rows.flatMap((r) => [r.label, r.description])]),
    ...unclaimedSections(general).flatMap((s) => [s.title ?? "Other", ...fieldsTerms(s.fields)]),
    "Danger zone",
    ...rowTerms(GENERAL_ROWS.uninstall),
  ];
}

function aiTerms(general: ConfigHubField[], providers: AiProviderInfo[]): string[] {
  const byKey = fieldMap(general);
  const selected = providers.find((p) => p.is_selected);
  const modelRows =
    providers.length === 0
      ? [AI_ROWS.noProviders]
      : [
          AI_ROWS.provider,
          ...(selected && selected.models.length > 0 ? [AI_ROWS.model] : []),
          AI_ROWS.effort,
          AI_ROWS.discovery,
          AI_ROWS.providers,
        ];
  const agent = pickFields(byKey, AGENT_KEYS);
  const concurrency = pickFields(byKey, CONCURRENCY_KEYS);
  return [
    "Default model",
    ...modelRows.flatMap(rowTerms),
    "Model groups",
    ...MODEL_GROUP_KEYWORDS,
    ...(agent.length > 0 ? ["Agent", ...fieldsTerms(agent, AGENT_DESCRIPTIONS)] : []),
    ...(concurrency.length > 0 ? ["Concurrency", ...fieldsTerms(concurrency, CONCURRENCY_DESCRIPTIONS)] : []),
  ];
}

function reviewTerms(inputs: SearchInputs): string[] {
  const commands = pickFields(fieldMap(inputs.general), COMMAND_KEYS);
  const rules = inputs.riskRules ?? [];
  const risk = riskRows(inputs.riskDefault ?? "ai");
  return [
    ...(commands.length > 0 ? ["Review commands", ...fieldsTerms(commands, COMMAND_DESCRIPTIONS)] : []),
    "Risk rules",
    ...(rules.length > 0 ? [] : rowTerms(risk.empty)),
    ...rowTerms(risk.help),
    ...riskKeywords(rules, inputs.riskFiles ?? []),
  ];
}

function terminalTerms(terminal: ConfigHubField[]): string[] {
  return sectionsOf(terminal).flatMap((s) => [
    s.title ? (TERMINAL_TITLES[s.title] ?? s.title) : "Other",
    ...fieldsTerms(s.fields, VIEW_KEY_DESCRIPTIONS),
  ]);
}

export function buildSearchIndex(inputs: SearchInputs): SettingsSearchIndex {
  const project = inputs.project;
  return {
    general: generalTerms(inputs.general),
    ai: aiTerms(inputs.general, inputs.providers ?? []),
    review: reviewTerms(inputs),
    projects: project ? [project.name, ...projectRows(project).flatMap(rowTerms)] : [],
    terminal: terminalTerms(inputs.terminal),
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
