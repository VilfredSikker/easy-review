"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const pkg = require("../package.json");

describe("package.json", () => {
  it("ships a dedicated bin and no skills dependency", () => {
    assert.equal(pkg.bin["easy-review-skills"], "bin/easy-review-skills.js");
    assert.equal(pkg.bin.skills, undefined);
    assert.equal(pkg.dependencies, undefined);
  });
});
