"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const { listSkills, SKILL_DIRS } = require("./install.js");

describe("install", () => {
  it("lists bundled skills after sync", () => {
    const rows = listSkills();
    assert.equal(rows.length, SKILL_DIRS.length);
    assert.ok(rows.some((r) => r.name === "er-review" && r.description.includes("ER review")));
  });
});
