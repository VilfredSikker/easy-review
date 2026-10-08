"use strict";

const { describe, it } = require("node:test");
const assert = require("node:assert/strict");
const { platformPackage } = require("./platform.js");
const { missingBinaryMessage } = require("./ensure-binary.js");

describe("platform", () => {
  it("maps host to platform package name", () => {
    assert.equal(platformPackage("darwin", "arm64"), "easy-review-mcp-darwin-arm64");
    assert.equal(platformPackage("darwin", "x64"), "easy-review-mcp-darwin-x64");
    assert.equal(platformPackage("linux", "x64"), "easy-review-mcp-linux-x64");
    assert.equal(platformPackage("win32", "x64"), null);
  });
});

describe("missing binary", () => {
  it("names the optional package and how to get it back", () => {
    const msg = missingBinaryMessage("easy-review-mcp-linux-x64");
    assert.match(msg, /easy-review-mcp-linux-x64/);
    assert.match(msg, /--omit=optional/);
    assert.match(msg, /ER_MCP_PATH/);
    assert.match(msg, /cargo install/);
  });

  it("lists supported hosts when there is no package for this one", () => {
    const msg = missingBinaryMessage(null, "win32-x64");
    assert.match(msg, /win32-x64/);
    assert.match(msg, /darwin-arm64, darwin-x64, linux-x64/);
    assert.match(msg, /cargo install/);
  });
});
