"use strict";

const fs = require("node:fs");
const fsp = require("node:fs/promises");
const path = require("node:path");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const { PLATFORM_PACKAGES, platformPackage } = require("./platform.js");

const execFileAsync = promisify(execFile);

const SOURCE_INSTALL =
  "cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp";

async function whichOnPath(name) {
  const cmd = process.platform === "win32" ? "where" : "which";
  try {
    const { stdout } = await execFileAsync(cmd, [name]);
    const first = stdout.split(/\r?\n/).map((s) => s.trim()).find(Boolean);
    return first || null;
  } catch {
    return null;
  }
}

async function pathLooksExecutable(file) {
  try {
    await fsp.access(file, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Locate the binary shipped by the platform-specific optional dependency
 * (installed by npm at install time via os/cpu filtering). Resolving the
 * package.json is robust across Node's `exports` rules.
 */
function platformPackageBinary(pkg) {
  try {
    // eslint-disable-next-line import/no-dynamic-require, global-require
    const pkgJson = require.resolve(`${pkg}/package.json`);
    return path.join(path.dirname(pkgJson), "er-mcp");
  } catch {
    return null;
  }
}

/**
 * Resolve the native er-mcp binary.
 *
 * Order:
 * 1. ER_MCP_PATH / ER_MCP_BINARY env
 * 2. Platform optional-dependency package (installed by npm)
 * 3. `er-mcp` on PATH
 *
 * Releases no longer carry er-mcp archives, so there is nothing to download:
 * a missing platform package is reported with how to get one.
 */
async function ensureBinary() {
  const envPath = process.env.ER_MCP_PATH || process.env.ER_MCP_BINARY;
  if (envPath) {
    if (!(await pathLooksExecutable(envPath))) {
      throw new Error(`ER_MCP_PATH is set but not executable: ${envPath}`);
    }
    return envPath;
  }

  const pkg = platformPackage();
  const fromPkg = pkg && platformPackageBinary(pkg);
  if (fromPkg) {
    // npm may not preserve the exec bit in every install path; set it defensively.
    try {
      await fsp.chmod(fromPkg, 0o755);
    } catch {
      // already correct, or read-only store — ignore.
    }
    if (await pathLooksExecutable(fromPkg)) {
      return fromPkg;
    }
  }

  const onPath = await whichOnPath("er-mcp");
  if (onPath && (await pathLooksExecutable(onPath))) {
    return onPath;
  }

  throw new Error(missingBinaryMessage(pkg));
}

function missingBinaryMessage(pkg, host = `${process.platform}-${process.arch}`) {
  if (!pkg) {
    const supported = Object.keys(PLATFORM_PACKAGES).join(", ");
    return (
      `no prebuilt er-mcp for ${host} (supported: ${supported}). ` +
      `Build from source: ${SOURCE_INSTALL}`
    );
  }
  return (
    `the ${pkg} package is not installed. It is an optional dependency, so an ` +
    `install that skips optional dependencies leaves it out. Reinstall without ` +
    `--omit=optional / --no-optional, set ER_MCP_PATH, or build from source: ${SOURCE_INSTALL}`
  );
}

module.exports = {
  ensureBinary,
  missingBinaryMessage,
};
