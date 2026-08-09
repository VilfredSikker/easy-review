"use strict";

const { spawnSync } = require("node:child_process");
const path = require("node:path");
const { syncSkills, SKILL_DIRS, OUT_ROOT } = require("../scripts/sync-skills.js");

function skillsBundleDir() {
  return OUT_ROOT;
}

function ensureBundle() {
  const bundle = skillsBundleDir();
  const missing = SKILL_DIRS.some(
    (name) => !require("node:fs").existsSync(path.join(bundle, name, "SKILL.md")),
  );
  if (missing) {
    syncSkills();
  }
  return bundle;
}

function resolveSkillsCli() {
  try {
    return require.resolve("skills/bin/cli.mjs");
  } catch {
    return null;
  }
}

function runSkillsAdd(bundleDir, options) {
  const cli = resolveSkillsCli();
  const skill = options.skill ?? "*";
  const args = ["add", bundleDir, "-s", skill, "-y"];
  if (options.global !== false) {
    args.push("-g");
  }
  if (options.agent) {
    args.push("-a", options.agent);
  }
  if (options.copy) {
    args.push("--copy");
  }

  const env = { ...process.env };

  if (cli) {
    const result = spawnSync(process.execPath, [cli, ...args], {
      stdio: "inherit",
      env,
    });
    if (result.error) {
      throw result.error;
    }
    return result.status ?? 1;
  }

  const result = spawnSync("npx", ["-y", "skills@1", ...args], {
    stdio: "inherit",
    env,
  });
  if (result.error) {
    throw result.error;
  }
  return result.status ?? 1;
}

function listSkills() {
  ensureBundle();
  return SKILL_DIRS.map((name) => {
    const file = path.join(skillsBundleDir(), name, "SKILL.md");
    const text = require("node:fs").readFileSync(file, "utf8");
    const desc = text.match(/^description:\s*>?\s*\n((?:\s+.+\n?)+)/m);
    const description = desc
      ? desc[1]
          .split("\n")
          .map((l) => l.trim())
          .filter(Boolean)
          .join(" ")
      : "";
    return { name, description };
  });
}

function install(options = {}) {
  const bundleDir = ensureBundle();
  const code = runSkillsAdd(bundleDir, options);
  if (code !== 0) {
    const err = new Error(`skills add failed with exit code ${code}`);
    err.code = code;
    throw err;
  }
  return { bundleDir, skill: options.skill ?? "*" };
}

module.exports = {
  ensureBundle,
  install,
  listSkills,
  skillsBundleDir,
  SKILL_DIRS,
};
