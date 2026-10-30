import { describe, expect, it } from "bun:test";
import {
  claimedBy,
  draftProblem,
  fileWindow,
  removeRuleEdit,
  ruleLanded,
  setRuleEdit,
  tierLabel,
  type RiskRuleDraft,
  type RiskRuleRow,
} from "./riskRules";

const rule = (pattern: string, source: "project" | "user" = "project", tier = "high"): RiskRuleRow => ({
  pattern,
  tier,
  readable: true,
  label: null,
  source,
  fileCount: 0,
});

const draft = (over: Partial<RiskRuleDraft> = {}): RiskRuleDraft => ({
  source: "project",
  pattern: "db/**",
  tier: "high",
  label: "",
  ...over,
});

describe("risk rule drafts", () => {
  it("refuses an empty pattern and a project rule with no repo", () => {
    expect(draftProblem(draft({ pattern: "  " }), "er", [])).toContain("pattern");
    expect(draftProblem(draft(), "", [])).toContain("repo");
    // A user rule applies everywhere, so it needs no repo.
    expect(draftProblem(draft({ source: "user" }), "", [])).toBeNull();
  });

  it("refuses a duplicate in the same table but not in the other one", () => {
    const rules = [rule("db/**", "project")];
    expect(draftProblem(draft(), "er", rules)).toContain("already");
    expect(draftProblem(draft({ source: "user" }), "er", rules)).toBeNull();
    // Editing a rule in place is not a clash with itself.
    expect(draftProblem(draft(), "er", rules, "db/**")).toBeNull();
  });

  it("writes the edit the engine's RiskRuleEdit reads", () => {
    const edit = JSON.parse(setRuleEdit(draft({ pattern: " db/** ", label: " Migrations " }), "er"));
    expect(edit).toEqual({
      source: "project",
      repo: "er",
      pattern: "db/**",
      tier: "high",
      label: "Migrations",
      replaces: null,
    });
  });

  it("names the old key only on a real rename", () => {
    expect(JSON.parse(setRuleEdit(draft(), "er", "db/**")).replaces).toBeNull();
    expect(JSON.parse(setRuleEdit(draft(), "er", "migrations/**")).replaces).toBe("migrations/**");
  });

  it("removes a rule by sending no tier", () => {
    const edit = JSON.parse(removeRuleEdit(rule("*.lock", "user"), "er"));
    expect(edit).toEqual({ source: "user", repo: "er", pattern: "*.lock" });
    expect("tier" in edit).toBe(false);
  });

  it("detects a write the engine refused", () => {
    expect(ruleLanded([rule("db/**")], draft())).toBe(true);
    expect(ruleLanded([], draft())).toBe(false);
    expect(ruleLanded([rule("db/**", "project", "low")], draft())).toBe(false);
  });
});

describe("risk file rows", () => {
  it("counts what it leaves out", () => {
    const files = Array.from({ length: 45 }, (_, i) => i);
    expect(fileWindow(files)).toEqual({ shown: files.slice(0, 40), hidden: 5 });
    expect(fileWindow([1, 2])).toEqual({ shown: [1, 2], hidden: 0 });
  });

  it("says plainly when no rule matched", () => {
    expect(claimedBy({ path: "a.rs", tier: "ai", rule: null })).toBe("no rule");
    expect(claimedBy({ path: "a.rs", tier: "high", rule: { key: "*.rs", label: null, source: "user" } })).toBe("*.rs");
  });

  it("spells the ai tier as AI", () => {
    expect(tierLabel("ai")).toBe("AI");
    expect(tierLabel("med")).toBe("med");
  });
});
