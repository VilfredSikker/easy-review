"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const { PLUGIN_ROOT } = require("./helpers.js");

const manifestPath = path.join(PLUGIN_ROOT, "herdr-plugin.toml");
const manifest = fs.readFileSync(manifestPath, "utf8");

function field(key) {
  const match = manifest.match(new RegExp(`^${key}\\s*=\\s*"([^"]+)"`, "m"));
  return match?.[1] ?? null;
}

describe("herdr-plugin.toml", () => {
  it("declares required package metadata", () => {
    assert.equal(field("id"), "easy-review");
    assert.equal(field("name"), "Easy Review");
    assert.equal(field("version"), "0.1.0");
    assert.equal(field("min_herdr_version"), "0.7.0");
    assert.match(manifest, /platforms\s*=\s*\["linux", "macos", "windows"\]/);
  });

  it("declares the review pane entrypoint", () => {
    assert.match(manifest, /\[\[panes\]\][\s\S]*id\s*=\s*"review"/);
    assert.match(manifest, /command\s*=\s*\["bash", "open\.sh"\]/);
  });

  it("wires worktree.created to open-branch.sh", () => {
    assert.match(manifest, /\[\[events\]\][\s\S]*on\s*=\s*"worktree\.created"/);
    assert.match(manifest, /command\s*=\s*\["bash", "open-branch\.sh"\]/);
  });

  it("registers a GitHub PR link handler", () => {
    assert.match(manifest, /\[\[link_handlers\]\][\s\S]*id\s*=\s*"github-pr"/);
    assert.ok(manifest.includes("pull/[0-9]+"));
    assert.match(manifest, /action\s*=\s*"review-pr"/);
  });

  it("exposes branch and PR review actions", () => {
    assert.match(manifest, /id\s*=\s*"review-pr"[\s\S]*command\s*=\s*\["bash", "open-pr\.sh"\]/);
    assert.match(manifest, /id\s*=\s*"review-branch"[\s\S]*command\s*=\s*\["bash", "open-branch\.sh"\]/);
  });
});
