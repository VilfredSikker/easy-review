#!/usr/bin/env node
"use strict";

/**
 * npx entrypoint for Easy Review MCP.
 * Resolves (or downloads) the native `er-mcp` binary, then execs it with
 * inherited stdio so MCP clients can speak the protocol over stdin/stdout.
 */

const { spawn } = require("node:child_process");
const { ensureBinary } = require("../lib/ensure-binary.js");

function printTtyUsage() {
  console.error(`easy-review MCP server (stdio JSON-RPC).

It waits for an MCP client on stdin — running it alone in a terminal looks idle.
Point your client at it instead, for example:

  Cursor  (~/.cursor/mcp.json):
    {
      "mcpServers": {
        "easy-review": {
          "command": "npx",
          "args": ["-y", "easy-review-mcp"]
        }
      }
    }

  Claude Code:
    claude mcp add --scope user easy-review -- npx -y easy-review-mcp

  Codex:
    codex mcp add easy-review -- npx -y easy-review-mcp

Docs: https://vilfredsikker.github.io/easy-review/guide/mcp.html`);
}

async function main() {
  const args = process.argv.slice(2);
  if (args.includes("--help") || args.includes("-h")) {
    printTtyUsage();
    return;
  }
  if (args.includes("--version") || args.includes("-V")) {
    // eslint-disable-next-line global-require
    console.error(`easy-review-mcp ${require("../package.json").version}`);
    return;
  }

  // Interactive terminal (both ends are TTYs) — don't hang silently.
  // MCP clients use pipes for stdin/stdout, so this stays false for them.
  if (process.stdin.isTTY && process.stdout.isTTY) {
    printTtyUsage();
    return;
  }

  const binary = await ensureBinary();
  const child = spawn(binary, args, {
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
