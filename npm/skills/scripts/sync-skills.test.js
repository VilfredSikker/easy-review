"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { syncSkills, SKILL_DIRS, OUT_ROOT } = require("../scripts/sync-skills.js");

// Repo checkout only — `crates/` is not part of the published package.
const EXPORT_RS = path.join(__dirname, "..", "..", "..", "crates", "er-engine", "src", "export.rs");

describe("sync-skills", () => {
  it("copies all ER skills with inlined shared docs", () => {
    const { count } = syncSkills();
    assert.equal(count, SKILL_DIRS.length);
    for (const name of SKILL_DIRS) {
      const file = path.join(OUT_ROOT, name, "SKILL.md");
      assert.ok(fs.existsSync(file), `missing ${file}`);
      const text = fs.readFileSync(file, "utf8");
      assert.ok(!text.includes("../_shared/"), `${name} still has _shared links`);
      if (name === "er-queue") {
        assert.ok(text.includes("MCP server"), `${name} missing prerequisites`);
        assert.ok(text.includes("pr_resolve") || text.includes("ref"), `${name} missing ref docs`);
      } else if (name === "er-review" || name === "er-low-hanging-fruit") {
        assert.ok(text.includes("pr_resolve"), `${name} missing ref resolution`);
      } else {
        assert.ok(text.includes("pr_resolve") || text.includes("ref"), `${name} missing ref docs`);
      }
    }
  });

  // The handling rules live in the skill and in the markdown export preamble.
  // An agent may read either one, so the two must not drift.
  it("keeps er-respond handling rules identical to the export preamble", (t) => {
    if (!fs.existsSync(EXPORT_RS)) {
      t.skip("crates/ not present (published package)");
      return;
    }
    syncSkills();
    const rust = fs.readFileSync(EXPORT_RS, "utf8");
    const literal = rust.match(/pub const HANDLING_RULES: &str = "\\\n([\s\S]*?)\n";/);
    assert.ok(literal, "HANDLING_RULES literal not found in export.rs");
    const skill = fs.readFileSync(path.join(OUT_ROOT, "er-respond", "SKILL.md"), "utf8");
    for (const line of literal[1].split("\n").filter((l) => l.trim())) {
      assert.ok(skill.includes(line), `er-respond is missing export rule: ${line}`);
    }
  });
});
