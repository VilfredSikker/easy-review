"use strict";

const fs = require("node:fs");
const fsp = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { execFile } = require("node:child_process");
const { promisify } = require("node:util");
const { pipeline } = require("node:stream/promises");
const { createWriteStream } = require("node:fs");
const {
  downloadUrl,
  rustTarget,
  releaseTag,
  platformPackage,
} = require("./platform.js");

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
 * Locate the binary shipped by the platform-specific optional dependency
 * (installed by npm at install time via os/cpu filtering). Resolving the
 * package.json is robust across Node's `exports` rules.
 */
function platformPackageBinary() {
  const pkg = platformPackage();
  if (!pkg) return null;
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
 * 2. Platform optional-dependency package (no download — installed by npm)
 * 3. Cached download for this package version
 * 4. `er-mcp` on PATH
 * 5. Download from GitHub Releases (v<package.version>) — hardened fallback
 */
async function ensureBinary() {
  const envPath = process.env.ER_MCP_PATH || process.env.ER_MCP_BINARY;
  if (envPath) {
    if (!(await pathLooksExecutable(envPath))) {
      throw new Error(`ER_MCP_PATH is set but not executable: ${envPath}`);
    }
    return envPath;
  }

  const fromPkg = platformPackageBinary();
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

  const version = packageVersion();
  const cached = cachedBinaryPath(version);
  if (await pathLooksExecutable(cached)) {
    await clearMacQuarantine(cached);
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
  await downloadToFile(url, archivePath);

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
  await clearMacQuarantine(destBinary);
  await assertBinaryRuns(destBinary);

  // Best-effort cleanup of the archive to save disk.
  await fsp.unlink(archivePath).catch(() => {});

  process.stderr.write(`easy-review-mcp: installed ${destBinary}\n`);
  return destBinary;
}

/**
 * Download a URL to a file. Prefers curl (reliable redirect handling, retries,
 * and a hard timeout — node's fetch() can stall indefinitely on the GitHub
 * release redirect); falls back to fetch() with an abort-based timeout when
 * curl is unavailable.
 */
async function downloadToFile(url, destPath) {
  const tmp = `${destPath}.partial`;

  try {
    await execFileAsync(
      "curl",
      [
        "-fSL",
        "--retry",
        "3",
        "--retry-delay",
        "1",
        "--max-time",
        "120",
        "-A",
        "easy-review-mcp-npm",
        "-o",
        tmp,
        url,
      ],
      { maxBuffer: 1024 * 1024 },
    );
    await fsp.rename(tmp, destPath);
    return;
  } catch (err) {
    if (err && err.code !== "ENOENT") {
      // curl exists but failed (network/HTTP) — clean up and fall through to fetch.
      await fsp.unlink(tmp).catch(() => {});
    }
  }

  const ac = new AbortController();
  const timer = setTimeout(() => ac.abort(), 120000);
  try {
    const res = await fetch(url, {
      redirect: "follow",
      signal: ac.signal,
      headers: { "User-Agent": "easy-review-mcp-npm" },
    });
    if (!res.ok) {
      throw new Error(
        `download failed (${res.status}) for ${url}. ` +
          `A GitHub Release with the er-mcp asset is required. ` +
          `Or: cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp`,
      );
    }
    await pipeline(res.body, createWriteStream(tmp));
    await fsp.rename(tmp, destPath);
  } catch (err) {
    await fsp.unlink(tmp).catch(() => {});
    throw new Error(
      `failed to download ${url}: ${err.message}. ` +
        `Install from source: cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp`,
    );
  } finally {
    clearTimeout(timer);
  }
}

/** Gatekeeper blocks unsigned GitHub-downloaded binaries until quarantine is cleared. */
async function clearMacQuarantine(file) {
  if (process.platform !== "darwin") return;
  try {
    await execFileAsync("xattr", ["-dr", "com.apple.quarantine", file]);
  } catch {
    // Not quarantined, or xattr unavailable — ignore.
  }
}

async function assertBinaryRuns(file) {
  try {
    // Empty stdin → server exits after initialize wait fails; we only care it execs.
    await execFileAsync(file, [], {
      timeout: 3000,
      input: "",
      maxBuffer: 1024,
    });
  } catch (err) {
    const msg = err && (err.message || String(err));
    const code = err && err.code;
    if (code === "ETIMEDOUT") {
      // Still running = binary executed; fine for smoke check.
      return;
    }
    // ConnectionClosed / non-zero exit after stdin EOF is expected.
    if (err && typeof err.status === "number") {
      return;
    }
    throw new Error(
      `downloaded binary failed to run (${code || "error"}): ${msg}. ` +
        `On macOS try: xattr -dr com.apple.quarantine ${file}`,
    );
  }
}

module.exports = {
  packageVersion,
  cacheDir,
  cachedBinaryPath,
  ensureBinary,
  downloadBinary,
};
