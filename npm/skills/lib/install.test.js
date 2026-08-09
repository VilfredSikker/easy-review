"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const {
  install,
  installRoot,
  listSkills,
  parseIndexedPick,
  resolveAgents,
  resolveSkills,
  skillExists,
  SKILL_DIRS,
} = require("./install.js");

describe("install", () => {
  it("lists bundled skills after sync", () => {
    const rows = listSkills();
    assert.equal(rows.length, SKILL_DIRS.length);
    assert.ok(rows.some((r) => r.name === "er-review" && r.description.includes("ER review")));
  });

  it("resolves agent aliases", () => {
    const agents = resolveAgents("claude");
    assert.equal(agents.length, 1);
    assert.equal(agents[0].id, "claude-code");
  });

  it("parses indexed picks", () => {
    assert.equal(parseIndexedPick("a", 3), null);
    assert.deepEqual(parseIndexedPick("1,3", 3), [1, 3]);
  });

  it("copies skills into agent directories", () => {
    const root = fs.mkdtempSync(path.join(os.tmpdir(), "er-skills-install-"));
    const prev = process.cwd();
    try {
      process.chdir(root);
      const { installed } = install({
        global: false,
        agent: "cursor",
        skill: "er-review",
        force: true,
      });
      assert.equal(installed.length, 1);
      const dest = path.join(root, ".cursor", "skills", "er-review", "SKILL.md");
      assert.ok(fs.existsSync(dest));
      assert.ok(skillExists(path.join(root, ".cursor", "skills"), "er-review"));
      assert.ok(fs.readFileSync(dest, "utf8").includes("er-review"));
    } finally {
      process.chdir(prev);
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  it("uses the home directory for global installs", () => {
    assert.equal(installRoot(true), os.homedir());
  });
});
