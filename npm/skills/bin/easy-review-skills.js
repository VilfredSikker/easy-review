#!/usr/bin/env node
"use strict";

const { runInstall, listSkills, SKILL_DIRS } = require("../lib/install.js");

function printUsage() {
  console.error(`@easy-review/skills — install Easy Review agent skills for Cursor, Claude Code, Codex

Usage:
  npx @easy-review/skills                 Interactive wizard (default in a TTY)
  npx @easy-review/skills --yes           All agents, all skills, default paths
  npx @easy-review/skills --list          List bundled skills
  npx @easy-review/skills -s er-review    Install one skill (with --yes)

Options:
  -s, --skill <names>  Skill id(s), comma-separated, or * (default: all)
  -g, --global         Install under home dir agent paths (default)
  -p, --project        Install under project-local agent paths
  -a, --agent <ids>    Agent ids: cursor, claude-code, codex, all
  -y, --yes            Non-interactive defaults
  -i, --interactive    Force wizard prompts (useful under bunx)
  -f, --force          Overwrite existing skill folders
  -h, --help           Show this help
  -V, --version        Show version

Run from home if you're inside the monorepo package dir:
  cd ~ && npx -y @easy-review/skills

Pair with the MCP server:
  npx -y easy-review-mcp

Docs: https://vilfredsikker.github.io/easy-review/guide/mcp.html`);
}

function parseArgs(argv) {
  const opts = { global: true, yes: false };
  for (let i = 0; i < argv.length; i += 1) {
    const arg = argv[i];
    switch (arg) {
      case "-h":
      case "--help":
        opts.help = true;
        break;
      case "-V":
      case "--version":
        opts.version = true;
        break;
      case "-l":
      case "--list":
        opts.list = true;
        break;
      case "-g":
      case "--global":
        opts.global = true;
        break;
      case "-p":
      case "--project":
        opts.global = false;
        break;
      case "-y":
      case "--yes":
        opts.yes = true;
        break;
      case "-i":
      case "--interactive":
        opts.interactive = true;
        break;
      case "-f":
      case "--force":
        opts.force = true;
        break;
      case "-s":
      case "--skill":
        opts.skill = argv[++i];
        break;
      case "-a":
      case "--agent":
        opts.agent = argv[++i];
        break;
      default:
        if (arg.startsWith("-")) {
          throw new Error(`unknown option: ${arg}`);
        }
        opts.skill = arg;
    }
  }
  return opts;
}

async function main() {
  let opts;
  try {
    opts = parseArgs(process.argv.slice(2));
  } catch (err) {
    console.error(err.message);
    printUsage();
    process.exit(1);
  }

  if (opts.help) {
    printUsage();
    return;
  }
  if (opts.version) {
    // eslint-disable-next-line global-require
    console.log(require("../package.json").version);
    return;
  }
  if (opts.list) {
    for (const row of listSkills()) {
      console.log(`${row.name}\n  ${row.description}\n`);
    }
    return;
  }

  try {
    await runInstall(opts);
  } catch (err) {
    console.error(`@easy-review/skills: ${err.message || err}`);
    process.exit(typeof err.code === "number" ? err.code : 1);
  }
}

main().catch((err) => {
  console.error(err instanceof Error ? err.message : err);
  process.exit(1);
});
