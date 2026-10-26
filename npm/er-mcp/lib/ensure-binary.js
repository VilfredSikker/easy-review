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

async function runWhich(cmd, args) {
  try {
    const { stdout } = await execFileAsync(cmd, args);
    return stdout
      .split(/\r?\n/)
      .map((s) => s.trim())
      .filter(Boolean);
  } catch {
    return [];
  }
}

async function whichOnPath(name) {
  if (process.platform === "win32") {
    return runWhich("where", [name]);
  }
  // `which` needs -a to list every match; more than one `er-mcp` is normal — a
  // package manager's bin shim plus a cargo install. BusyBox `which` has no -a
  // and fails outright, so fall back to the single-match form it does accept.
  const all = await runWhich("which", ["-a", name]);
  return all.length > 0 ? all : runWhich("which", [name]);
}

/** An executable regular file — `X_OK` alone also holds for a directory. */
async function pathLooksExecutable(file) {
  try {
    const stat = await fsp.stat(file);
    if (!stat.isFile()) return false;
    await fsp.access(file, fs.constants.X_OK);
    return true;
  } catch {
    return false;
  }
}

/**
 * Locate the binary shipped by the platform-specific optional dependency
 * (installed by npm at install time via os/cpu filtering). Resolving the
 * package's package.json finds it wherever npm put it.
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

/** The bin file npm links for this package — the path in package.json `bin`. */
const LAUNCHER_ENTRY = path.basename(
  // eslint-disable-next-line global-require
  require("../package.json").bin["er-mcp"],
);

/** Enough of a shim to name the file it runs; real binaries are far larger. */
const SHIM_SCAN_LIMIT = 16 * 1024;

/**
 * Is this PATH candidate the launcher itself rather than a real er-mcp binary?
 *
 * npm links every bin name a package declares at the same entrypoint, and this
 * package declares both `easy-review-mcp` and `er-mcp` at ./bin/er-mcp.js, so a
 * PATH search for `er-mcp` finds this script — and spawning it would re-enter
 * this same resolution instead of starting a server. pnpm writes that bin entry
 * as a script naming the target file rather than as a symlink, which is why the
 * resolved file is read as well.
 */
function isLauncherItself(file) {
  let real;
  try {
    real = fs.realpathSync(file);
  } catch {
    return false;
  }
  if (
    path.basename(real) === LAUNCHER_ENTRY ||
    real.startsWith(path.resolve(__dirname, "..") + path.sep)
  ) {
    return true;
  }
  try {
    if (fs.statSync(real).size > SHIM_SCAN_LIMIT) {
      return false;
    }
    return fs.readFileSync(real, "utf8").includes(LAUNCHER_ENTRY);
  } catch {
    return false;
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
 * er-mcp ships only through its npm platform packages, so there is nothing to
 * download: a missing package is reported with how to get one.
 */
async function ensureBinary() {
  const envPath = process.env.ER_MCP_PATH || process.env.ER_MCP_BINARY;
  if (envPath) {
    if (isLauncherItself(envPath)) {
      throw new Error(`ER_MCP_PATH is set but resolves to this launcher: ${envPath}`);
    }
    if (!(await pathLooksExecutable(envPath))) {
      throw new Error(`ER_MCP_PATH is set but not executable: ${envPath}`);
    }
    return envPath;
  }

  const pkg = platformPackage();
  const fromPkg = pkg && platformPackageBinary(pkg);
  let unusable = null;
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
    unusable = fromPkg;
  }

  for (const candidate of await whichOnPath("er-mcp")) {
    // Skip this launcher and keep looking: it can sit in front of a real binary.
    if (isLauncherItself(candidate)) continue;
    if (await pathLooksExecutable(candidate)) {
      return candidate;
    }
  }

  if (unusable) {
    throw new Error(
      `the ${pkg} package is installed but ${unusable} is missing or not executable. ` +
        `Reinstall it, set ER_MCP_PATH, or build from source: ${SOURCE_INSTALL}`,
    );
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
