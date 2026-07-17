"use strict";

/**
 * Map process.platform / process.arch to the Rust target triples used in
 * GitHub Release assets (see .github/workflows/release.yml).
 */

const TARGETS = {
  "darwin-arm64": "aarch64-apple-darwin",
  "darwin-x64": "x86_64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu",
};

function rustTarget(platform = process.platform, arch = process.arch) {
  const key = `${platform}-${arch}`;
  const target = TARGETS[key];
  if (!target) {
    const supported = Object.keys(TARGETS).join(", ");
    throw new Error(
      `unsupported platform ${key}. Supported: ${supported}. ` +
        `Build from source: cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp`,
    );
  }
  return target;
}

function assetName(version, target = rustTarget()) {
  const v = String(version).replace(/^v/, "");
  return `er-mcp-${target}.tar.gz`;
}

function releaseTag(version) {
  const v = String(version).replace(/^v/, "");
  return `v${v}`;
}

function downloadUrl(version, owner = "VilfredSikker", repo = "easy-review") {
  const tag = releaseTag(version);
  const target = rustTarget();
  const asset = assetName(version, target);
  return `https://github.com/${owner}/${repo}/releases/download/${tag}/${asset}`;
}

// npm optional-dependency package that carries the prebuilt binary for each host.
// Keys match `${process.platform}-${process.arch}`; the launcher resolves the
// matching package (installed by npm via os/cpu filtering) before any download.
const PLATFORM_PACKAGES = {
  "darwin-arm64": "easy-review-mcp-darwin-arm64",
  "darwin-x64": "easy-review-mcp-darwin-x64",
  "linux-x64": "easy-review-mcp-linux-x64",
};

function platformPackage(platform = process.platform, arch = process.arch) {
  return PLATFORM_PACKAGES[`${platform}-${arch}`] || null;
}

module.exports = {
  TARGETS,
  rustTarget,
  assetName,
  releaseTag,
  downloadUrl,
  PLATFORM_PACKAGES,
  platformPackage,
};
