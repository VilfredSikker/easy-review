"use strict";

// npm optional-dependency package that carries the prebuilt binary for each host.
// Keys match `${process.platform}-${process.arch}`; npm installs only the one
// matching the host, via each package's os/cpu fields.
const PLATFORM_PACKAGES = {
  "darwin-arm64": "easy-review-mcp-darwin-arm64",
  "darwin-x64": "easy-review-mcp-darwin-x64",
  "linux-x64": "easy-review-mcp-linux-x64",
};

function platformPackage(platform = process.platform, arch = process.arch) {
  return PLATFORM_PACKAGES[`${platform}-${arch}`] || null;
}

module.exports = {
  PLATFORM_PACKAGES,
  platformPackage,
};
