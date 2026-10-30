import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const read = (name: string) => readFileSync(join(dirname(fileURLToPath(import.meta.url)), name), "utf8");
const page = read("SettingsPage.svelte");
const risk = read("RiskRulesSection.svelte");

describe("SettingsPage risk rules", () => {
  it("reads the merged rule table off the settings payload", () => {
    expect(page).toContain("riskRules = settings.riskRules ?? [];");
    expect(page).toContain("riskFiles = settings.riskFiles ?? [];");
  });

  it("writes rules through the engine's one patch key", () => {
    expect(page).toContain("send(RISK_RULE_PATCH_KEY, edit)");
  });

  it("checks a write landed, since the engine refuses a bad edit silently", () => {
    expect(risk).toContain("ruleLanded(after, draft)");
  });

  it("windows the changed files through the tested helper and counts the rest", () => {
    expect(risk).toContain("fileWindow(files)");
    expect(risk).toContain("{#if fileRows.hidden > 0}");
  });

  it("answers why a file has its tier, not only what the tier is", () => {
    expect(risk).toContain("claimedBy(file)");
    expect(risk).toContain("no rule matched");
  });
});

describe("SettingsPage scope", () => {
  it("says config is global and lives in managed app data (ADR 0005)", () => {
    expect(page).toContain("No repo-local config.");
  });
});

describe("SettingsPage review fixes", () => {
  it("closes the add-rule form once a rule lands", () => {
    expect(risk).toContain("editing = null;\n      adding = false;\n      return null;");
  });

  it("acts on the uninstall deep link once", () => {
    expect(page).toContain("onuninstallfocused={() => (focusUninstall = false)}");
  });
});
