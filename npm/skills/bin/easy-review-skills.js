#!/usr/bin/env node
"use strict";

const { install, listSkills, SKILL_DIRS } = require("../lib/install.js");

function printUsage() {
  console.error(`@easy-review/skills — install Easy Review agent skills for Cursor, Claude Code, Codex, etc.

Usage:
  bunx @easy-review/skills              Install all ER skills globally (default)
  bunx @easy-review/skills --list       List bundled skills
  bunx @easy-review/skills -s er-review Install one skill

Options:
  -s, --skill <name>   Skill to install (${SKILL_DIRS.join(", ")}, or *)
  -g, --global         Install globally (default)
  -p, --project        Install project-local instead
  -a, --agent <name>   Target agent (cursor, claude-code, codex, …)
  -y, --yes            Skip prompts (default)
  -h, --help           Show this help
  -V, --version        Show version

Also install the MCP server:
  bunx easy-review-mcp   (or npx -y easy-review-mcp)

Docs: https://vilfredsikker.github.io/easy-review/guide/mcp.html`);
}

function parseArgs(argv) {
  const opts = { global: true, yes: true };
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
      case "-s":
      case "--skill":
        opts.skill = argv[++i];
        break;
      case "-a":
      case "--agent":
        opts.agent = argv[++i];
        break;
      case "--copy":
        opts.copy = true;
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

function main() {
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
    install(opts);
  } catch (err) {
    console.error(`@easy-review/skills: ${err.message || err}`);
    process.exit(typeof err.code === "number" ? err.code : 1);
  }
}

main();
