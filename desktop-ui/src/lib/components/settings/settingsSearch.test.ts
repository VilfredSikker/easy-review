import { describe, expect, it } from "bun:test";
import { readFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import type { AiProviderInfo, ConfigHubField, ProjectSnapshot } from "$lib/types";
import type { RiskRuleRow } from "./riskRules";
import { AI_ROWS, GENERAL_ROWS, PROJECT_ROWS, buildSearchIndex, tabMatchCounts, type SearchInputs } from "./settingsSearch";

const general: ConfigHubField[] = [
  { kind: "section", title: "Appearance" },
  {
    kind: "cycle",
    key: "display.theme",
    label: "Theme",
    description: "Engine theme blurb",
    options: ["graphite", "paper"],
    value: "graphite",
  },
  { kind: "section", title: "Agent" },
  {
    kind: "text",
    key: "agent.command",
    label: "Command",
    description: "Agent executable",
    placeholder: "",
    value: "claude",
    strict: true,
  },
  {
    kind: "cycle",
    key: "ai_hub.max_concurrent_reviews",
    label: "Max parallel reviews",
    description: "Engine concurrency blurb",
    options: ["1", "2", "3"],
    value: "2",
  },
  { kind: "bool", key: "features.brand_new", label: "Brand new flag", description: "Something added later", value: true },
];

const terminal: ConfigHubField[] = [
  { kind: "section", title: "Display" },
  { kind: "bool", key: "display.wrap_lines", label: "Wrap lines", description: "Wrap long lines", value: false },
  { kind: "section", title: "Views" },
  {
    kind: "bool",
    key: "features.view_branch",
    label: "Branch",
    description: "Show branch diff mode (TUI tab 1)",
    value: true,
  },
];

const provider = {
  id: "p",
  label: "P",
  is_selected: true,
  models: [{ id: "m", label: "M", is_selected: true }],
} as unknown as AiProviderInfo;

const project = (over: Partial<ProjectSnapshot>): ProjectSnapshot =>
  ({ id: "x", name: "easy-review", root_path: "/r", remote: null, is_active: true, ...over }) as ProjectSnapshot;

const rule = {
  source: "user",
  pattern: "db/**",
  tier: "high",
  label: "Migrations",
  fileCount: 0,
  readable: true,
} as unknown as RiskRuleRow;

const counts = (query: string, inputs: Partial<SearchInputs> = {}) =>
  tabMatchCounts(buildSearchIndex({ general, terminal, ...inputs }), query);

describe("settings search index", () => {
  it("counts matches per tab so the nav can point at another tab", () => {
    expect(counts("wrap").terminal).toBe(2);
    expect(counts("wrap").general).toBe(0);
    expect(counts("parallel").ai).toBe(1);
    expect(counts("brand new").general).toBe(1);
    expect(counts("").ai).toBe(0);
  });

  it("indexes the description a row shows, not the engine's", () => {
    expect(counts("branch diff mode").terminal).toBe(0);
    expect(counts("Key 1").terminal).toBe(1);
    expect(counts("View modes").terminal).toBe(1);
    expect(counts("engine theme blurb").general).toBe(0);
    expect(counts("engine concurrency blurb").ai).toBe(0);
    expect(counts("running at once").ai).toBe(1);
  });

  it("indexes a row's controls only where the row renders", () => {
    expect(counts("send test").general).toBe(1);
    expect(counts("send test notification").general).toBe(0);
    expect(counts("refresh models", { providers: [provider] }).ai).toBe(1);
    expect(counts("refresh models").ai).toBe(0);
  });

  it("indexes the project rows the card shows for that project", () => {
    expect(counts("ignore in ai reviews", { project: project({}) }).projects).toBe(0);
    expect(counts("size limit", { project: project({}) }).projects).toBe(0);
    expect(counts("size limit", { project: project({ remote: "o/r" }) }).projects).toBe(1);
    expect(counts("run triage", { project: project({ remote: "o/r" }) }).projects).toBe(0);
    expect(counts("run triage", { project: project({ remote: "o/r", auto_triage: true }) }).projects).toBe(1);
    expect(counts("*.lock", { project: project({ review_ignore_globs: ["*.lock"] }) }).projects).toBe(1);
  });

  it("indexes risk rules from the table the section shows", () => {
    expect(counts("migrations", { riskRules: [rule] }).review).toBe(1);
    expect(counts("changed files").review).toBe(0);
    expect(counts("no rules declared").review).toBe(1);
    expect(counts("no rules declared", { riskRules: [rule] }).review).toBe(0);
  });
});

describe("settings rows render the indexed copy", () => {
  const dir = dirname(fileURLToPath(import.meta.url));
  const sources = readdirSync(dir)
    .filter((f) => f.endsWith(".svelte"))
    .map((f) => readFileSync(join(dir, f), "utf8"))
    .join("\n");

  // The index is only as true as the rows that spread this copy; a row that
  // words itself inline would drift from what the nav counts.
  const tables = { GENERAL_ROWS, AI_ROWS, PROJECT_ROWS };
  for (const [table, rows] of Object.entries(tables)) {
    for (const key of Object.keys(rows)) {
      it(`${table}.${key} is spread onto a rendered row`, () => {
        expect(sources).toContain(`{...${table}.${key}}`);
      });
    }
  }

  it("risk rows and every description override reach a rendered row", () => {
    expect(sources).toContain("{...copy.empty}");
    expect(sources).toContain("{...copy.help}");
    for (const name of [
      "WATCHED_DESCRIPTIONS",
      "COMMAND_DESCRIPTIONS",
      "AGENT_DESCRIPTIONS",
      "CONCURRENCY_DESCRIPTIONS",
      "VIEW_KEY_DESCRIPTIONS",
    ]) {
      expect(sources).toContain(`descriptions={${name}}`);
    }
  });
});
