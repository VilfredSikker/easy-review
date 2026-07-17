#!/usr/bin/env node
"use strict";

/**
 * npx entrypoint for Easy Review MCP.
 * Resolves (or downloads) the native `er-mcp` binary, then execs it with
 * inherited stdio so MCP clients can speak the protocol over stdin/stdout.
 */

const { spawn } = require("node:child_process");
const { ensureBinary } = require("../lib/ensure-binary.js");

async function main() {
  const binary = await ensureBinary();
  const child = spawn(binary, process.argv.slice(2), {
    stdio: "inherit",
    env: process.env,
  });

  const forward = (signal) => {
    if (child.pid) {
      try {
        child.kill(signal);
      } catch {
        // ignore
      }
    }
  };
  process.on("SIGINT", () => forward("SIGINT"));
  process.on("SIGTERM", () => forward("SIGTERM"));

  child.on("error", (err) => {
    console.error(`easy-review-mcp: failed to start ${binary}: ${err.message}`);
    process.exit(1);
  });
  child.on("exit", (code, signal) => {
    if (signal) {
      process.kill(process.pid, signal);
      return;
    }
    process.exit(code ?? 1);
  });
}

main().catch((err) => {
  console.error(`easy-review-mcp: ${err.message || err}`);
  process.exit(1);
});
