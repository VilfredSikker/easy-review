"use strict";

const { describe, it, beforeEach } = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

const {
  PLUGIN_ROOT,
  SAFE_PATH,
  makeTmp,
  makeMockHerdr,
  makeMockEr,
  runBash,
  readLog,
  readErLog,
} = require("./helpers.js");

describe("lib/json-field.sh", () => {
  it("extracts string fields from JSON blobs", () => {
    const json = '{"workspace_id":"ws-new","tab_id":"tab-9"}';
    const out = runBash("test/fixtures/run-json-field.sh", {
      JSON_BLOB: json,
    });
    assert.equal(out.status, 0);
    assert.equal(out.stdout.trim(), "ws-new|tab-9");
  });
});

describe("open.sh", () => {
  let tmp;
  let binDir;
  let erLog;

  beforeEach(() => {
    tmp = makeTmp();
    binDir = path.join(tmp, "bin");
    fs.mkdirSync(binDir);
    erLog = path.join(tmp, "er.log");
    makeMockEr(binDir, erLog);
  });

  it("prints install guidance when er is missing", () => {
    const emptyBin = path.join(tmp, "empty-bin");
    fs.mkdirSync(emptyBin);
    const out = runBash("open.sh", {
      PATH: `${emptyBin}:${SAFE_PATH}`,
      HOME: tmp,
    });
    assert.equal(out.status, 0);
    assert.match(out.stderr, /not installed/i);
    assert.match(out.stderr, /cargo tui-install/i);
  });

  it("execs er when it is on PATH", () => {
    const out = runBash("open.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HOME: tmp,
    });
    assert.equal(out.status, 0);
    const er = readErLog(erLog);
    assert.equal(er.args, "er");
    assert.equal(er.cwd, PLUGIN_ROOT);
  });

  it("execs er --remote when HERDR_PLUGIN_CLICKED_URL is set", () => {
    const url = "https://github.com/acme/repo/pull/42";
    const out = runBash("open.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HOME: tmp,
      HERDR_PLUGIN_CLICKED_URL: url,
    });
    assert.equal(out.status, 0);
    assert.equal(readErLog(erLog).args, `er --remote ${url}`);
  });

  it("cds to the workspace checkout before execing er", () => {
    // Herdr starts plugin panes in the plugin directory. Without an explicit
    // cd, `er` walks up to the plugin's own git repo (easy-review on
    // release/herdr-plugin when the plugin is linked from this tree).
    const reviewDir = path.join(tmp, "show-deprecation-banner-for-plates");
    fs.mkdirSync(reviewDir);
    const out = runBash("open.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HOME: tmp,
      HERDR_PLUGIN_ROOT: PLUGIN_ROOT,
      HERDR_PLUGIN_CONTEXT_JSON: JSON.stringify({
        workspace_id: "w-plates",
        workspace_cwd: reviewDir,
      }),
    });
    assert.equal(out.status, 0);
    const er = readErLog(erLog);
    assert.equal(er.args, "er");
    assert.equal(fs.realpathSync(er.cwd), fs.realpathSync(reviewDir));
  });

  it("prefers worktree checkout_path over focused-pane cwd", () => {
    const reviewDir = path.join(tmp, "checkout");
    const paneDir = path.join(tmp, "plugin-lookalike");
    fs.mkdirSync(reviewDir);
    fs.mkdirSync(paneDir);
    const out = runBash("open.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HOME: tmp,
      HERDR_PLUGIN_CONTEXT_JSON: JSON.stringify({
        workspace_cwd: paneDir,
        focused_pane_cwd: paneDir,
        worktree: { checkout_path: reviewDir },
      }),
    });
    assert.equal(out.status, 0);
    assert.equal(
      fs.realpathSync(readErLog(erLog).cwd),
      fs.realpathSync(reviewDir),
    );
  });

  it("stays put when the resolved cwd does not exist", () => {
    const out = runBash("open.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HOME: tmp,
      HERDR_PLUGIN_CONTEXT_JSON: JSON.stringify({
        workspace_cwd: path.join(tmp, "missing"),
      }),
    });
    assert.equal(out.status, 0);
    assert.equal(readErLog(erLog).cwd, PLUGIN_ROOT);
  });
});

describe("open-branch.sh", () => {
  let tmp;
  let binDir;
  let herdrLog;

  beforeEach(() => {
    tmp = makeTmp();
    binDir = path.join(tmp, "bin");
    fs.mkdirSync(binDir);
    herdrLog = path.join(tmp, "herdr.log");
    makeMockHerdr(binDir, herdrLog);
  });

  it("opens the review tab for a workspace action with focus", () => {
    const out = runBash("open-branch.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HERDR_BIN_PATH: path.join(binDir, "herdr"),
      HERDR_WORKSPACE_ID: "ws-manual",
    });
    assert.equal(out.status, 0);
    const log = readLog(herdrLog);
    assert.match(log, /plugin pane open --plugin easy-review --entrypoint review --placement tab --focus --workspace ws-manual/);
    assert.match(log, /tab rename tab-review-1 Review/);
  });

  it("uses event workspace and --no-focus for worktree.created hooks", () => {
    const out = runBash("open-branch.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HERDR_BIN_PATH: path.join(binDir, "herdr"),
      HERDR_WORKSPACE_ID: "ws-stale",
      HERDR_PLUGIN_EVENT: "worktree.created",
      HERDR_PLUGIN_EVENT_JSON: '{"workspace_id":"ws-new","worktree_path":"/tmp/wt"}',
    });
    assert.equal(out.status, 0);
    const log = readLog(herdrLog);
    assert.match(log, /--no-focus --workspace ws-new/);
    assert.doesNotMatch(log, /--workspace ws-stale/);
  });
});

describe("open-pr.sh", () => {
  let tmp;
  let binDir;
  let herdrLog;
  let erLog;

  beforeEach(() => {
    tmp = makeTmp();
    binDir = path.join(tmp, "bin");
    fs.mkdirSync(binDir);
    herdrLog = path.join(tmp, "herdr.log");
    erLog = path.join(tmp, "er.log");
    makeMockHerdr(binDir, herdrLog);
    makeMockEr(binDir, erLog);
  });

  it("exits cleanly when the clicked URL is missing", () => {
    const out = runBash("open-pr.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HERDR_BIN_PATH: path.join(binDir, "herdr"),
    });
    assert.equal(out.status, 0);
    assert.match(out.stderr, /missing HERDR_PLUGIN_CLICKED_URL/);
    assert.equal(readLog(herdrLog), "");
    assert.equal(readLog(erLog), "");
  });

  it("opens the review pane with the PR URL env for open.sh", () => {
    const url = "https://github.com/acme/repo/pull/42";
    const out = runBash("open-pr.sh", {
      PATH: `${binDir}:/usr/bin:/bin`,
      HERDR_BIN_PATH: path.join(binDir, "herdr"),
      HERDR_PLUGIN_CLICKED_URL: url,
    });
    assert.equal(out.status, 0);
    const herdr = readLog(herdrLog);
    assert.match(
      herdr,
      /plugin pane open --plugin easy-review --entrypoint review --placement tab --focus --env HERDR_PLUGIN_CLICKED_URL=https:\/\/github\.com\/acme\/repo\/pull\/42/,
    );
    assert.match(herdr, /tab rename tab-review-1 Review/);
    // Action must not exec er itself — the pane (open.sh) owns the TUI.
    assert.equal(readLog(erLog), "");
  });

  it("still opens the pane when er is not on PATH", () => {
    const emptyBin = path.join(tmp, "empty-bin");
    fs.mkdirSync(emptyBin);
    const out = runBash("open-pr.sh", {
      PATH: `${emptyBin}:${SAFE_PATH}`,
      HERDR_BIN_PATH: path.join(binDir, "herdr"),
      HERDR_PLUGIN_CLICKED_URL: "https://github.com/acme/repo/pull/7",
    });
    assert.equal(out.status, 0);
    assert.match(readLog(herdrLog), /plugin pane open/);
    assert.equal(readLog(erLog), "");
  });
});
