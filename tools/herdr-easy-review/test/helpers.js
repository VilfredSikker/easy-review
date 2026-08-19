"use strict";

const { spawnSync } = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const PLUGIN_ROOT = path.resolve(__dirname, "..");
const SAFE_PATH = "/usr/bin:/bin:/usr/local/bin";

function makeTmp() {
  return fs.mkdtempSync(path.join(os.tmpdir(), "er-herdr-test-"));
}

function writeExecutable(file, body) {
  fs.writeFileSync(file, body, { mode: 0o755 });
}

function makeMockHerdr(binDir, logFile) {
  const script = path.join(binDir, "herdr");
  writeExecutable(
    script,
    `#!/usr/bin/env bash
set -euo pipefail
printf '%s\\n' "$*" >> "${logFile}"
if [[ "$1" == "plugin" && "$2" == "pane" && "$3" == "open" ]]; then
  printf '%s\\n' '{"result":{"tab_id":"tab-review-1"}}'
fi
exit 0
`,
  );
  return script;
}

function makeMockEr(binDir, logFile) {
  const script = path.join(binDir, "er");
  writeExecutable(
    script,
    `#!/usr/bin/env bash
set -euo pipefail
printf 'cwd=%s\\n' "$PWD" >> "${logFile}"
printf 'er %s\\n' "$*" >> "${logFile}"
exit 0
`,
  );
  return script;
}

function runBash(scriptName, env = {}, opts = {}) {
  const result = spawnSync("bash", [path.join(PLUGIN_ROOT, scriptName)], {
    env: { ...process.env, ...env },
    encoding: "utf8",
    cwd: opts.cwd ?? PLUGIN_ROOT,
  });
  return {
    stdout: result.stdout ?? "",
    stderr: result.stderr ?? "",
    status: result.status ?? 1,
  };
}

function readLog(logFile) {
  return fs.existsSync(logFile) ? fs.readFileSync(logFile, "utf8").trim() : "";
}

function readErLog(logFile) {
  const lines = readLog(logFile).split("\n").filter(Boolean);
  const cwdLine = lines.find((line) => line.startsWith("cwd="));
  const cmdLine = lines.find((line) => line.startsWith("er"));
  return {
    cwd: cwdLine ? cwdLine.slice("cwd=".length) : "",
    args: cmdLine ?? "",
  };
}

module.exports = {
  PLUGIN_ROOT,
  SAFE_PATH,
  makeTmp,
  makeMockHerdr,
  makeMockEr,
  runBash,
  readLog,
  readErLog,
};
