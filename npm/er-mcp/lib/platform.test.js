"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const {
  rustTarget,
  assetName,
  releaseTag,
  downloadUrl,
  TARGETS,
} = require("./platform.js");

describe("platform", () => {
  it("maps common host triples", () => {
    assert.equal(rustTarget("darwin", "arm64"), "aarch64-apple-darwin");
    assert.equal(rustTarget("darwin", "x64"), "x86_64-apple-darwin");
    assert.equal(rustTarget("linux", "x64"), "x86_64-unknown-linux-gnu");
  });

  it("rejects unsupported platforms", () => {
    assert.throws(() => rustTarget("win32", "x64"), /unsupported platform/);
  });

  it("builds release asset names", () => {
    assert.equal(
      assetName("0.4.3", "x86_64-unknown-linux-gnu"),
      "er-mcp-x86_64-unknown-linux-gnu.tar.gz",
    );
    assert.equal(releaseTag("0.4.3"), "v0.4.3");
    assert.equal(releaseTag("v0.4.3"), "v0.4.3");
  });

  it("builds github download urls", () => {
    // Force linux mapping via direct asset helpers; downloadUrl uses process.platform.
    assert.ok(Object.keys(TARGETS).length >= 3);
    const url = downloadUrl("0.4.3");
    assert.match(url, /^https:\/\/github.com\/VilfredSikker\/easy-review\/releases\/download\/v0\.4\.3\/er-mcp-.+\.tar\.gz$/);
  });
});
