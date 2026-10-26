# easy-review-mcp

npx launcher for the Easy Review MCP server (`er-mcp`).

npm installs the prebuilt `er-mcp` binary for your platform as an optional
dependency (`easy-review-mcp-<platform>`), and the launcher execs it with
inherited stdio. Nothing is downloaded at run time.

## Quick start

Wire into an MCP client (do not expect a useful interactive CLI — the server
speaks JSON-RPC on stdin/stdout and waits for a client):

```bash
npx -y easy-review-mcp
```

Running that in a bare terminal prints a short setup hint and exits.

### Cursor (`~/.cursor/mcp.json`)

```json
{
  "mcpServers": {
    "easy-review": {
      "command": "npx",
      "args": ["-y", "easy-review-mcp"]
    }
  }
}
```

### Claude Code

```bash
claude mcp add --scope user easy-review -- npx -y easy-review-mcp
```

### Codex

```bash
codex mcp add easy-review -- npx -y easy-review-mcp
```

### OpenCode

```json
{
  "$schema": "https://opencode.ai/config.json",
  "mcp": {
    "easy-review": {
      "type": "local",
      "command": ["npx", "-y", "easy-review-mcp"],
      "enabled": true
    }
  }
}
```

Write that to `~/.config/opencode/opencode.json` (or project `opencode.json`).

## Overrides

| Env | Meaning |
|-----|---------|
| `ER_MCP_PATH` / `ER_MCP_BINARY` | Use this binary instead of the platform package |

If the launcher reports that `easy-review-mcp-<platform>` is not installed, the
install skipped optional dependencies. Reinstall without `--omit=optional`, or
point Claude at a source-built binary:

```bash
cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp
claude mcp add --scope user easy-review -- "$(command -v er-mcp)"
```

## Supported platforms

- macOS arm64 / x64
- Linux x64

## Version lockstep

`npm/er-mcp/package.json` `version`, its `optionalDependencies` pins, and each
platform package's `version` must match the Cargo workspace version and the
GitHub release tag (`vX.Y.Z`). Release CI publishes the platform packages from
the tag; it checks the versions and stops on a mismatch.

## Develop

```bash
cd npm/er-mcp
npm test
node bin/er-mcp.js   # needs the platform package, ER_MCP_PATH, or er-mcp on PATH
```
