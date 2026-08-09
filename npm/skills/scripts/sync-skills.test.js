"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { syncSkills, SKILL_DIRS, OUT_ROOT } = require("../scripts/sync-skills.js");

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
});
