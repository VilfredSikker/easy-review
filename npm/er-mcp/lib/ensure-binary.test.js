"use strict";

/**
 * Resolution order of `ensureBinary`, and the bound on how far the entrypoint
 * re-spawns itself. The PATH cases pin the machine's own PATH to temp dirs, so
 * a real `er-mcp` on the developer's PATH cannot decide the outcome.
 */

const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");
const Module = require("node:module");
const { execFileSync, spawnSync } = require("node:child_process");
const { describe, it, after } = require("node:test");
const assert = require("node:assert/strict");

const { ensureBinary } = require("./ensure-binary.js");
const { platformPackage } = require("./platform.js");

const LAUNCHER = path.resolve(__dirname, "..", "bin", "er-mcp.js");
const PLATFORM_PKG = platformPackage();
// `which` only exists on the hosts the platform packages cover.
const SKIP = !PLATFORM_PKG || process.platform === "win32";

const tmpDirs = [];

function tmpDir() {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "er-mcp-test-"));
  tmpDirs.push(dir);
  return dir;
}

after(() => {
  for (const dir of tmpDirs) {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

function clearBinaryEnv() {
  delete process.env.ER_MCP_PATH;
  delete process.env.ER_MCP_BINARY;
}

/** Point `${PLATFORM_PKG}/package.json` at dir, or at nothing when dir is null. */
function stubPlatformPackage(dir) {
  const original = Module._resolveFilename;
  Module._resolveFilename = function (request, ...rest) {
    if (request === `${PLATFORM_PKG}/package.json`) {
      if (dir) return path.join(dir, "package.json");
      throw new Error(`Cannot find module '${request}'`);
    }
    return original.call(this, request, ...rest);
  };
  return () => {
    Module._resolveFilename = original;
  };
}

/** A temp platform package holding an er-mcp file, as npm installs one. */
function fakePlatformPackage() {
  const dir = tmpDir();
  fs.writeFileSync(
    path.join(dir, "package.json"),
    JSON.stringify({ name: PLATFORM_PKG, version: "0.0.0" }),
  );
  const binary = path.join(dir, "er-mcp");
  fs.writeFileSync(binary, "#!/bin/sh\n");
  return { dir, binary };
}

/** PATH of the given dirs plus the directory `which` itself lives in. */
function pathOf(...dirs) {
  const which = execFileSync("which", ["which"], { encoding: "utf8" }).trim();
  return [...dirs, path.dirname(which)].join(path.delimiter);
}

/** A bin entry shaped like the one pnpm writes: a script naming the launcher. */
function writeShim(dir) {
  const shim = path.join(dir, "er-mcp");
  fs.writeFileSync(shim, `#!/bin/sh\nexec node "${LAUNCHER}" "$@"\n`, {
    mode: 0o755,
  });
  return shim;
}

/** An executable named `er-mcp` that is not this package's launcher. */
function writeRealErMcp(dir) {
  const binary = path.join(dir, "er-mcp");
  fs.writeFileSync(binary, "#!/bin/sh\nexit 0\n", { mode: 0o755 });
  return binary;
}

/** Run fn with PATH set to the given dirs. */
async function onPath(dirs, fn) {
  const saved = process.env.PATH;
  process.env.PATH = pathOf(...dirs);
  try {
    return await fn();
  } finally {
    process.env.PATH = saved;
  }
}

describe("ensureBinary", () => {
  it(
    "returns the platform package's binary when the package is installed",
    { skip: !PLATFORM_PKG },
    async () => {
      clearBinaryEnv();
      const { dir, binary } = fakePlatformPackage();

      const restore = stubPlatformPackage(dir);
      try {
        // Also covers the defensive chmod: the file is written without +x.
        assert.equal(await ensureBinary(), binary);
      } finally {
        restore();
      }
    },
  );

  it(
    "names the binary when the package is installed but unusable",
    { skip: SKIP },
    async () => {
      clearBinaryEnv();
      const { dir } = fakePlatformPackage();
      fs.rmSync(path.join(dir, "er-mcp"));

      const restore = stubPlatformPackage(dir);
      try {
        await assert.rejects(
          onPath([tmpDir()], ensureBinary),
          new RegExp(`the ${PLATFORM_PKG} package is installed but .* is missing`),
        );
      } finally {
        restore();
      }
    },
  );

  it(
    "prefers an er-mcp on PATH to an installed but unusable package",
    { skip: SKIP },
    async () => {
      clearBinaryEnv();
      const { dir } = fakePlatformPackage();
      fs.rmSync(path.join(dir, "er-mcp"));
      const expected = writeRealErMcp(tmpDir());

      const restore = stubPlatformPackage(dir);
      try {
        assert.equal(
          await onPath([path.dirname(expected)], ensureBinary),
          expected,
        );
      } finally {
        restore();
      }
    },
  );

  it(
    "prefers the platform package to an er-mcp on PATH",
    { skip: SKIP },
    async () => {
      clearBinaryEnv();
      const { dir, binary } = fakePlatformPackage();
      const onPathBinary = writeRealErMcp(tmpDir());

      const restore = stubPlatformPackage(dir);
      try {
        assert.equal(
          await onPath([path.dirname(onPathBinary)], ensureBinary),
          binary,
        );
      } finally {
        restore();
      }
    },
  );

  it(
    "prefers ER_MCP_PATH to the platform package",
    { skip: !PLATFORM_PKG },
    async () => {
      clearBinaryEnv();
      const { dir } = fakePlatformPackage();
      const envBinary = path.join(tmpDir(), "er-mcp");
      fs.writeFileSync(envBinary, "#!/bin/sh\n", { mode: 0o755 });
      process.env.ER_MCP_PATH = envBinary;

      const restore = stubPlatformPackage(dir);
      try {
        assert.equal(await ensureBinary(), envBinary);
      } finally {
        restore();
        clearBinaryEnv();
      }
    },
  );

  it("returns an er-mcp found on PATH", { skip: SKIP }, async () => {
    clearBinaryEnv();
    const expected = writeRealErMcp(tmpDir());

    const restore = stubPlatformPackage(null);
    try {
      assert.equal(
        await onPath([path.dirname(expected)], ensureBinary),
        expected,
      );
    } finally {
      restore();
    }
  });

  it("does not take this launcher's own er-mcp shim", { skip: SKIP }, async () => {
    clearBinaryEnv();
    const dir = tmpDir();
    // What npm links: both bin names of this package, at ./bin/er-mcp.js.
    fs.symlinkSync(LAUNCHER, path.join(dir, "er-mcp"));

    const restore = stubPlatformPackage(null);
    try {
      await assert.rejects(
        onPath([dir], ensureBinary),
        new RegExp(`the ${PLATFORM_PKG} package is not installed`),
      );
    } finally {
      restore();
    }
  });

  it(
    "passes over this launcher's shim on PATH for a real binary behind it",
    { skip: SKIP },
    async () => {
      clearBinaryEnv();
      const shim = writeShim(tmpDir());
      const expected = writeRealErMcp(tmpDir());

      const restore = stubPlatformPackage(null);
      try {
        assert.equal(
          await onPath([path.dirname(shim), path.dirname(expected)], ensureBinary),
          expected,
        );
      } finally {
        restore();
      }
    },
  );

  it(
    "does not read a PATH candidate past the shim scan limit",
    { skip: SKIP },
    async () => {
      clearBinaryEnv();
      const dir = tmpDir();
      const binary = path.join(dir, "er-mcp");
      // Larger than any shim, and naming the launcher entry, so only the size
      // gate keeps it from being mistaken for one.
      fs.writeFileSync(binary, `#!/bin/sh\n# ${LAUNCHER}\n${"#".repeat(20 * 1024)}\n`, {
        mode: 0o755,
      });

      const restore = stubPlatformPackage(null);
      try {
        assert.equal(await onPath([dir], ensureBinary), binary);
      } finally {
        restore();
      }
    },
  );

  it("stops when it is already two launchers deep", () => {
    // The entrypoint's bound, whatever shim shape sent it back here.
    const result = spawnSync(process.execPath, [LAUNCHER], {
      encoding: "utf8",
      input: "",
      env: { ...process.env, ER_MCP_LAUNCH_DEPTH: "2" },
      timeout: 15000,
    });

    assert.equal(result.status, 1);
    assert.match(result.stderr, /resolving back to this launcher/);
  });

  it("returns ER_MCP_BINARY when it is executable", async () => {
    clearBinaryEnv();
    const file = path.join(tmpDir(), "er-mcp");
    fs.writeFileSync(file, "#!/bin/sh\n", { mode: 0o755 });
    process.env.ER_MCP_BINARY = file;

    try {
      assert.equal(await ensureBinary(), file);
    } finally {
      clearBinaryEnv();
    }
  });

  it("finds er-mcp when which takes no -a", { skip: SKIP }, async () => {
    clearBinaryEnv();
    const expected = writeRealErMcp(tmpDir());
    const realWhich = execFileSync("which", ["which"], { encoding: "utf8" }).trim();
    const stubDir = tmpDir();
    // What BusyBox `which` does with -a: fail, while the plain call works.
    fs.writeFileSync(
      path.join(stubDir, "which"),
      `#!/bin/sh\ncase "$1" in -a) exit 1;; esac\nexec "${realWhich}" "$@"\n`,
      { mode: 0o755 },
    );

    const restore = stubPlatformPackage(null);
    try {
      assert.equal(
        await onPath([stubDir, path.dirname(expected)], ensureBinary),
        expected,
      );
    } finally {
      restore();
    }
  });

  it("rejects an ER_MCP_PATH that is this launcher", { skip: SKIP }, async () => {
    clearBinaryEnv();
    const shim = path.join(tmpDir(), "er-mcp");
    fs.symlinkSync(LAUNCHER, shim);
    process.env.ER_MCP_PATH = shim;

    try {
      await assert.rejects(ensureBinary(), /resolves to this launcher/);
    } finally {
      clearBinaryEnv();
    }
  });

  it("rejects an ER_MCP_PATH that is not executable", async () => {
    clearBinaryEnv();
    const file = path.join(tmpDir(), "er-mcp");
    fs.writeFileSync(file, "");
    process.env.ER_MCP_PATH = file;

    try {
      await assert.rejects(ensureBinary(), /not executable/);
    } finally {
      clearBinaryEnv();
    }
  });

  it("returns ER_MCP_PATH when it is executable", async () => {
    clearBinaryEnv();
    const file = path.join(tmpDir(), "er-mcp");
    fs.writeFileSync(file, "#!/bin/sh\n", { mode: 0o755 });
    process.env.ER_MCP_PATH = file;

    try {
      assert.equal(await ensureBinary(), file);
    } finally {
      clearBinaryEnv();
    }
  });
});
