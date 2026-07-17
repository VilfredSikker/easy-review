"use strict";

const fs = require("node:fs");
const fsp = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const { pipeline } = require("node:stream/promises");
const { createWriteStream } = require("node:fs");
const { downloadUrl, rustTarget, releaseTag } = require("./platform.js");

const execFileAsync = promisify(execFile);

function packageVersion() {
  // Keep in lockstep with Cargo workspace version / GitHub release tag.
  // eslint-disable-next-line import/no-dynamic-require, global-require
  return require("../package.json").version;
}

function cacheDir(version = packageVersion()) {
  const base =
    process.env.XDG_CACHE_HOME ||
    (process.platform === "darwin"
      ? path.join(os.homedir(), "Library", "Caches")
      : path.join(os.homedir(), ".cache"));
  return path.join(base, "easy-review", "er-mcp", releaseTag(version));
}

function cachedBinaryPath(version = packageVersion()) {
  return path.join(cacheDir(version), "er-mcp");
}

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
 * Resolve the native er-mcp binary.
 *
 * Order:
 * 1. ER_MCP_PATH / ER_MCP_BINARY env
 * 2. Cached download for this package version
 * 3. `er-mcp` on PATH
 * 4. Download from GitHub Releases (v<package.version>)
 */
async function ensureBinary() {
  const envPath = process.env.ER_MCP_PATH || process.env.ER_MCP_BINARY;
  if (envPath) {
    if (!(await pathLooksExecutable(envPath))) {
      throw new Error(`ER_MCP_PATH is set but not executable: ${envPath}`);
    }
    return envPath;
  }

  const version = packageVersion();
  const cached = cachedBinaryPath(version);
  if (await pathLooksExecutable(cached)) {
    return cached;
  }

  const onPath = await whichOnPath("er-mcp");
  if (onPath && (await pathLooksExecutable(onPath))) {
    return onPath;
  }

  return downloadBinary(version);
}

async function downloadBinary(version = packageVersion()) {
  const target = rustTarget();
  const url = downloadUrl(version);
  const dir = cacheDir(version);
  await fsp.mkdir(dir, { recursive: true });

  const archivePath = path.join(dir, `er-mcp-${target}.tar.gz`);
  const destBinary = path.join(dir, "er-mcp");

  process.stderr.write(`easy-review-mcp: downloading ${url}\n`);

  let res;
  try {
    res = await fetch(url, {
      redirect: "follow",
      headers: { "User-Agent": "easy-review-mcp-npm" },
    });
  } catch (err) {
    throw new Error(
      `failed to download ${url}: ${err.message}. ` +
        `Install from source: cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp`,
    );
  }

  if (!res.ok) {
    throw new Error(
      `download failed (${res.status}) for ${url}. ` +
        `A GitHub Release ${releaseTag(version)} with er-mcp-${target}.tar.gz is required. ` +
        `Or: cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp`,
    );
  }

  const tmp = `${archivePath}.partial`;
  await pipeline(res.body, createWriteStream(tmp));
  await fsp.rename(tmp, archivePath);

  try {
    await execFileAsync("tar", ["-xzf", archivePath, "-C", dir]);
  } catch (err) {
    throw new Error(`failed to extract ${archivePath}: ${err.message}`);
  }

  // tarball contains a top-level `er-mcp` binary
  if (!(await pathLooksExecutable(destBinary))) {
    // Some archives nest the binary; search one level.
    const entries = await fsp.readdir(dir);
    const found = entries.find((e) => e === "er-mcp" || e.endsWith("/er-mcp"));
    if (!found) {
      throw new Error(
        `archive extracted but er-mcp binary missing in ${dir} (contents: ${entries.join(", ")})`,
      );
    }
  }

  await fsp.chmod(destBinary, 0o755);
  // Best-effort cleanup of the archive to save disk.
  await fsp.unlink(archivePath).catch(() => {});

  process.stderr.write(`easy-review-mcp: installed ${destBinary}\n`);
  return destBinary;
}

module.exports = {
  packageVersion,
  cacheDir,
  cachedBinaryPath,
  ensureBinary,
  downloadBinary,
};
