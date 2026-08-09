#!/usr/bin/env node
"use strict";

/**
 * Copy Easy Review skill sources from npm/skills/source/ into the bundled
 * npm/skills/skills/ tree and inline source/_shared so installed SKILL.md
 * files are self-contained.
 */

const fs = require("node:fs");
const path = require("node:path");

const PKG_ROOT = path.join(__dirname, "..");
const SOURCE_ROOT = path.join(PKG_ROOT, "source");
const OUT_ROOT = path.join(PKG_ROOT, "skills");

const SKILL_DIRS = [
  "er-review",
  "er-guide",
  "er-queue",
  "er-low-hanging-fruit",
  "er-get-feedback",
  "er-respond",
  "er-saved",
];

function readShared(name) {
  const file = path.join(SOURCE_ROOT, "_shared", name);
  if (!fs.existsSync(file)) {
    throw new Error(`missing shared doc: ${file}`);
  }
  return fs
    .readFileSync(file, "utf8")
    .trim()
    .replace(/^#[^\n]+\n+/, "");
}

function sharedBlockFor(skillMarkdown) {
  const parts = [];
  if (skillMarkdown.includes("PREREQUISITES.md")) {
    parts.push(readShared("PREREQUISITES.md"));
  }
  if (skillMarkdown.includes("REF_RESOLUTION.md")) {
    parts.push(readShared("REF_RESOLUTION.md"));
  }
  return parts.join("\n\n");
}

function inlineShared(skillMarkdown) {
  const block = sharedBlockFor(skillMarkdown);
  const lines = [];
  for (const line of skillMarkdown.split("\n")) {
    if (!line.includes("../_shared/")) {
      lines.push(line);
      continue;
    }
    const prefix = line.replace(/See \[`[^`]+`\]\([^)]+\)(?: and \[`[^`]+`\]\([^)]+\))?\.?/g, "").trim();
    if (prefix) {
      lines.push(prefix);
    }
  }
  const without = lines.join("\n");
  const match = without.match(/^(---\n[\s\S]*?\n---\n\n#[^\n]+\n\n)/);
  if (match && block) {
    return without.replace(match[1], `${match[1]}${block}\n\n`);
  }
  if (block) {
    return `${without}\n\n${block}\n`;
  }
  return without;
}

function syncSkills() {
  if (!fs.existsSync(SOURCE_ROOT)) {
    throw new Error(`skill sources not found: ${SOURCE_ROOT}`);
  }

  fs.rmSync(OUT_ROOT, { recursive: true, force: true });
  fs.mkdirSync(OUT_ROOT, { recursive: true });

  for (const dir of SKILL_DIRS) {
    const srcDir = path.join(SOURCE_ROOT, dir);
    const srcFile = path.join(srcDir, "SKILL.md");
    if (!fs.existsSync(srcFile)) {
      throw new Error(`missing skill: ${srcFile}`);
    }
    const outDir = path.join(OUT_ROOT, dir);
    fs.mkdirSync(outDir, { recursive: true });
    const raw = fs.readFileSync(srcFile, "utf8");
    const inlined = inlineShared(raw);
    fs.writeFileSync(path.join(outDir, "SKILL.md"), inlined, "utf8");
  }

  return { count: SKILL_DIRS.length, out: OUT_ROOT };
}

if (require.main === module) {
  try {
    const result = syncSkills();
    console.error(`synced ${result.count} skills → ${result.out}`);
  } catch (err) {
    console.error(`sync-skills: ${err.message || err}`);
    process.exit(1);
  }
}

module.exports = { syncSkills, SKILL_DIRS, OUT_ROOT };
