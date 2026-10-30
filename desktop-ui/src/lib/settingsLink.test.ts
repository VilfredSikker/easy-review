import { describe, expect, test } from "bun:test";
import { isSettingsSection, resolveSettingsTarget, settingsSectionDomId } from "./settingsLink";

describe("settings deep links", () => {
  test("a section lands on the tab that holds it", () => {
    expect(resolveSettingsTarget("risk-rules")).toEqual({ tab: "review", section: "risk-rules" });
    expect(resolveSettingsTarget("concurrency")).toEqual({ tab: "ai", section: "concurrency" });
    expect(resolveSettingsTarget("uninstall")).toEqual({ tab: "general", section: "uninstall" });
  });

  test("a tab opens at its top", () => {
    expect(resolveSettingsTarget("projects")).toEqual({ tab: "projects", section: null });
  });

  test("a plain open names no target", () => {
    expect(resolveSettingsTarget(undefined)).toBeNull();
    expect(resolveSettingsTarget(null)).toBeNull();
  });

  test("only known sections count, not inherited keys", () => {
    expect(isSettingsSection("risk-rules")).toBe(true);
    expect(isSettingsSection("toString")).toBe(false);
  });

  test("the DOM id matches the one the section renders under", () => {
    expect(settingsSectionDomId("uninstall")).toBe("settings-uninstall");
  });
});
