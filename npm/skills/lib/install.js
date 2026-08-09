"use strict";

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const readline = require("node:readline/promises");
const { stdin: input, stdout: output } = require("node:process");
const { syncSkills, SKILL_DIRS, OUT_ROOT } = require("../scripts/sync-skills.js");

const AGENTS = [
  {
    id: "cursor",
    label: "Cursor",
    skillsDir: (root) => path.join(root, ".cursor", "skills"),
    tryHint: 'In Cursor, ask the agent to use er-review or say "ER review" on a PR.',
  },
  {
    id: "claude-code",
    aliases: ["claude"],
    label: "Claude Code",
    skillsDir: (root) => path.join(root, ".claude", "skills"),
    tryHint: "In Claude Code, invoke er-review or ask for an Easy Review triage.",
  },
  {
    id: "codex",
    label: "Codex",
    skillsDir: (root) => path.join(root, ".codex", "skills"),
    tryHint: "In Codex, mention er-review or ask for an Easy Review on a PR.",
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
  if (wanted.has("all") || wanted.has("a")) {
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
  const parts = skillFilter
    .split(",")
    .map((part) => part.trim())
    .filter(Boolean);
  if (parts.length === 1 && parts[0] === "all") {
    return [...SKILL_DIRS];
  }
  const picked = [];
  for (const part of parts) {
    if (!SKILL_DIRS.includes(part)) {
      throw new Error(
        `unknown skill: ${part}. Use one of ${SKILL_DIRS.join(", ")} or *`,
      );
    }
    if (!picked.includes(part)) {
      picked.push(part);
    }
  }
  return picked;
}

function installRoot(global) {
  return global ? os.homedir() : process.cwd();
}

function skillExists(destRoot, skillId) {
  const dest = path.join(destRoot, skillId);
  return fs.existsSync(dest) && fs.statSync(dest).isDirectory();
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

function skillCatalog() {
  return listSkills().map((row) => ({
    id: row.name,
    label: `${row.name} — ${row.description}`,
  }));
}

async function ask(rl, question, fallback, auto) {
  if (auto) {
    console.log(`${question} → ${fallback}`);
    return fallback;
  }
  try {
    const hint = fallback ? ` [${fallback}]` : "";
    const answer = (await rl.question(`${question}${hint}: `)).trim();
    return answer || fallback;
  } catch {
    return fallback;
  }
}

function parseIndexedPick(pick, count, { allValue = "a" } = {}) {
  const normalized = pick.trim().toLowerCase();
  if (normalized === allValue || normalized === "all") {
    return null;
  }
  const idxs = pick
    .split(/[,\s]+/)
    .map((x) => Number(x.trim()))
    .filter((n) => n >= 1 && n <= count);
  return idxs;
}

function printMcpHints() {
  console.log("\n--- Easy Review MCP ---");
  console.log("ER skills use er-mcp for triage, review uploads, tours, and PR comments.");
  console.log("Install the MCP server:\n");
  console.log("  npx -y easy-review-mcp\n");
  console.log("Cursor (~/.cursor/mcp.json):");
  console.log(
    JSON.stringify(
      {
        mcpServers: {
          "easy-review": {
            command: "npx",
            args: ["-y", "easy-review-mcp"],
          },
        },
      },
      null,
      2,
    ),
  );
  console.log("\nClaude Code:");
  console.log("  claude mcp add --scope user easy-review -- npx -y easy-review-mcp\n");
}

function installToTargets(targets, skills, { force = false, overwrite = false } = {}) {
  const installed = [];
  let skipped = 0;

  for (const { agent, dir } of targets) {
    console.log(`\n${agent.label} → ${dir}`);
    for (const skillId of skills) {
      if (skillExists(dir, skillId) && !overwrite && !force) {
        console.log(`  · ${skillId} — already present, skipped`);
        skipped += 1;
        continue;
      }
      const dest = copySkill(skillId, dir);
      console.log(`  ✓ ${skillId} → ${dest}`);
      installed.push({ agent: agent.id, skill: skillId, dest });
    }
  }

  return { installed, skipped };
}

async function runInstall(options = {}) {
  const bundleDir = ensureBundle();
  const global = options.global !== false;
  const root = installRoot(global);
  const auto = options.yes === true || !process.stdin.isTTY;
  const rl = auto ? null : readline.createInterface({ input, output });

  console.log("\nEasy Review skills installer\n");
  console.log("Installs er-* agent skills for PR review, guides, and MCP workflows.\n");

  let selectedAgents;
  if (options.agent) {
    selectedAgents = resolveAgents(options.agent);
    console.log(`Agents: ${selectedAgents.map((a) => a.label).join(", ")}`);
  } else if (!auto) {
    console.log("Which agent(s) to install for?");
    AGENTS.forEach((agent, index) => {
      console.log(`  ${index + 1}) ${agent.label} — ${agent.skillsDir(root)}`);
    });
    console.log("  a) All (default)");
    const pick = await ask(rl, "Enter numbers (e.g. 1,3) or a", "a", false);
    if (pick === "a" || pick === "all") {
      selectedAgents = [...AGENTS];
    } else {
      const idxs = parseIndexedPick(pick, AGENTS.length);
      selectedAgents = idxs && idxs.length > 0 ? idxs.map((n) => AGENTS[n - 1]) : [...AGENTS];
    }
  } else {
    selectedAgents = [...AGENTS];
    console.log("Agents: all (Cursor, Claude Code, Codex)");
  }

  const catalog = skillCatalog();
  let selectedSkills;
  if (options.skill) {
    selectedSkills = resolveSkills(options.skill);
    console.log(`Skills: ${selectedSkills.join(", ")}`);
  } else if (!auto) {
    console.log("\nWhich skills to install?");
    catalog.forEach((skill, index) => {
      console.log(`  ${index + 1}) ${skill.label}`);
    });
    console.log("  a) All (default)");
    const pick = await ask(rl, "Enter numbers (e.g. 1,3) or a", "a", false);
    if (pick === "a" || pick === "all") {
      selectedSkills = catalog.map((skill) => skill.id);
    } else {
      const idxs = parseIndexedPick(pick, catalog.length);
      selectedSkills =
        idxs && idxs.length > 0
          ? idxs.map((n) => catalog[n - 1].id)
          : catalog.map((skill) => skill.id);
    }
  } else {
    selectedSkills = catalog.map((skill) => skill.id);
    console.log(`Skills: all (${selectedSkills.join(", ")})`);
  }

  const targets = [];
  for (const agent of selectedAgents) {
    const defaultDir = agent.skillsDir(root);
    const dir =
      selectedAgents.length === 1
        ? await ask(rl, `Install directory (${agent.label})`, defaultDir, auto)
        : auto
          ? defaultDir
          : await ask(rl, `Install directory (${agent.label})`, defaultDir, false);
    fs.mkdirSync(dir, { recursive: true });
    targets.push({ agent, dir });
  }

  printMcpHints();

  const existingPairs = [];
  for (const { agent, dir } of targets) {
    for (const skillId of selectedSkills) {
      if (skillExists(dir, skillId)) {
        existingPairs.push(`${agent.id}:${skillId}`);
      }
    }
  }

  let overwrite = options.force === true;
  if (existingPairs.length > 0 && !overwrite) {
    console.log(`\nAlready installed: ${existingPairs.join(", ")}`);
    if (auto) {
      console.log("Skipping existing (pass --force to overwrite).");
    } else {
      const ans = await ask(rl, "Overwrite existing skills? (y/N)", "N", false);
      overwrite = /^y(es)?$/i.test(ans);
    }
  }

  console.log("\nInstalling…");
  const { installed, skipped } = installToTargets(targets, selectedSkills, {
    force: options.force === true,
    overwrite,
  });

  if (installed.length === 0 && skipped > 0) {
    console.log("\nNothing new installed (all selected skills already present).");
    console.log("Re-run with --force to overwrite.\n");
    rl?.close();
    return { bundleDir, installed, skipped, skills: selectedSkills };
  }

  if (installed.length === 0) {
    throw new Error("no skills installed");
  }

  console.log("\nDone.");
  if (skipped > 0) {
    console.log(`Installed ${installed.length}, skipped ${skipped} existing (use --force to overwrite).`);
  }
  for (const { agent } of targets) {
    console.log(`Try (${agent.label}): ${agent.tryHint}`);
  }
  console.log("Docs: https://vilfredsikker.github.io/easy-review/guide/mcp.html\n");

  rl?.close();
  return { bundleDir, installed, skipped, skills: selectedSkills };
}

/** Non-interactive install for tests and scripted use. */
function install(options = {}) {
  const bundleDir = ensureBundle();
  const global = options.global !== false;
  const root = installRoot(global);
  const agents = resolveAgents(options.agent);
  const skills = resolveSkills(options.skill);
  const targets = agents.map((agent) => ({
    agent,
    dir: agent.skillsDir(root),
  }));
  for (const target of targets) {
    fs.mkdirSync(target.dir, { recursive: true });
  }
  const { installed } = installToTargets(targets, skills, {
    force: options.force === true,
    overwrite: options.force === true,
  });
  if (installed.length === 0) {
    throw new Error("no skills installed");
  }
  return { bundleDir, installed, skills };
}

module.exports = {
  AGENTS,
  ask,
  ensureBundle,
  install,
  installRoot,
  installToTargets,
  listSkills,
  parseIndexedPick,
  resolveAgents,
  resolveSkills,
  runInstall,
  skillCatalog,
  skillExists,
  skillsBundleDir,
  SKILL_DIRS,
};
