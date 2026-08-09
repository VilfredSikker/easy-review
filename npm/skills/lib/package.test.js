"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const pkg = require("../package.json");

describe("package.json", () => {
  it("exposes a skills bin so bunx @easy-review/skills runs our CLI", () => {
    assert.equal(typeof pkg.bin, "object");
    assert.equal(pkg.bin.skills, "bin/easy-review-skills.js");
    assert.equal(pkg.dependencies["skills-installer"], "npm:skills@^1.5.22");
    assert.equal(pkg.dependencies.skills, undefined);
  });
});
