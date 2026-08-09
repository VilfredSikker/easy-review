"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const { syncSkills, SKILL_DIRS, OUT_ROOT } = require("../scripts/sync-skills.js");

const AGENTS = [
  {
    id: "cursor",
    label: "Cursor",
    skillsDir: (root) => path.join(root, ".cursor", "skills"),
  },
  {
    id: "claude-code",
    aliases: ["claude"],
    label: "Claude Code",
    skillsDir: (root) => path.join(root, ".claude", "skills"),
  },
  {
    id: "codex",
    label: "Codex",
    skillsDir: (root) => path.join(root, ".codex", "skills"),
  },
];

function skillsBundleDir() {
  return OUT_ROOT;
}

function ensureBundle() {
  const bundle = skillsBundleDir();
  const missing = SKILL_DIRS.some(
    (name) => !fs.existsSync(path.join(bundle, name, "SKILL.md")),
  );
  if (missing) {
    syncSkills();
  }
  return bundle;
}

function resolveAgents(agentFilter) {
  if (!agentFilter) {
    return [...AGENTS];
  }
  const wanted = new Set(
    agentFilter
      .split(",")
      .map((part) => part.trim().toLowerCase())
      .filter(Boolean),
  );
  if (wanted.has("all")) {
    return [...AGENTS];
  }
  const picked = [];
  for (const agent of AGENTS) {
    const ids = [agent.id, ...(agent.aliases ?? [])];
    if (ids.some((id) => wanted.has(id)) && !picked.some((row) => row.id === agent.id)) {
      picked.push(agent);
    }
  }
  if (picked.length === 0) {
    throw new Error(
      `unknown agent(s): ${agentFilter}. Use cursor, claude-code, codex, or all`,
    );
  }
  return picked;
}

function resolveSkills(skillFilter) {
  if (!skillFilter || skillFilter === "*") {
    return [...SKILL_DIRS];
  }
  if (!SKILL_DIRS.includes(skillFilter)) {
    throw new Error(
      `unknown skill: ${skillFilter}. Use one of ${SKILL_DIRS.join(", ")} or *`,
    );
  }
  return [skillFilter];
}

function installRoot(global) {
  return global ? os.homedir() : process.cwd();
}

function copySkill(skillId, destRoot) {
  const src = path.join(skillsBundleDir(), skillId);
  const dest = path.join(destRoot, skillId);
  const skillFile = path.join(src, "SKILL.md");
  if (!fs.existsSync(skillFile)) {
    throw new Error(`skill pack missing: ${skillId} (expected ${skillFile})`);
  }
  fs.mkdirSync(dest, { recursive: true });
  fs.copyFileSync(skillFile, path.join(dest, "SKILL.md"));
  return dest;
}

function listSkills() {
  ensureBundle();
  return SKILL_DIRS.map((name) => {
    const file = path.join(skillsBundleDir(), name, "SKILL.md");
    const text = fs.readFileSync(file, "utf8");
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
  const global = options.global !== false;
  const root = installRoot(global);
  const agents = resolveAgents(options.agent);
  const skills = resolveSkills(options.skill);
  const installed = [];

  for (const agent of agents) {
    const destRoot = agent.skillsDir(root);
    fs.mkdirSync(destRoot, { recursive: true });
    for (const skillId of skills) {
      const dest = copySkill(skillId, destRoot);
      installed.push({ agent: agent.id, skill: skillId, dest });
    }
  }

  if (installed.length === 0) {
    throw new Error("no skills installed");
  }

  console.log(`\nEasy Review skills installed from ${bundleDir}\n`);
  for (const row of installed) {
    console.log(`  ✓ ${row.skill} → ${row.dest} (${row.agent})`);
  }
  console.log("\nPair with the MCP server: npx -y easy-review-mcp");
  console.log("Docs: https://vilfredsikker.github.io/easy-review/guide/mcp.html\n");

  return { bundleDir, installed, skills };
}

module.exports = {
  AGENTS,
  ensureBundle,
  install,
  installRoot,
  listSkills,
  resolveAgents,
  resolveSkills,
  skillsBundleDir,
  SKILL_DIRS,
};
