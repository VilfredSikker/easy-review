import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "SettingsPage.svelte"),
  "utf8",
);

describe("SettingsPage file importance", () => {
  it("reads the resolved files off the settings payload", () => {
    expect(src).toContain("importanceFiles = res.settings.importanceFiles ?? [];");
    expect(src).toContain("{#if importanceFiles.length > 0}");
  });

  it("windows the rows through the tested helper rather than in the template", () => {
    expect(src).toContain("importanceFileWindow(importanceFiles)");
    expect(src).toContain("{#each importanceWindow.shown as file (file.path)}");
    // The count of what was left out is drawn, not assumed away.
    expect(src).toContain("{#if importanceWindow.hidden > 0}");
  });

  it("answers why a file has its tier, not only what the tier is", () => {
    // The rule key is the answer to the question the card exists for; a row
    // showing the tier alone would be the declared table read back.
    expect(src).toContain("matchedRuleLabel(file)");
    expect(src).toContain("{file.tier}");
  });

  it("says plainly when no rule matched", () => {
    expect(src).toContain("no rule matched");
  });
});
