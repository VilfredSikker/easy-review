# easy-review-mcp

npx launcher for the Easy Review MCP server (`er-mcp`).

On first run it downloads the matching platform binary from the GitHub Release
that matches this package version (`vX.Y.Z`), caches it under
`~/.cache/easy-review/er-mcp/` (or `~/Library/Caches/…` on macOS), and execs it
with inherited stdio.

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

## Overrides

| Env | Meaning |
|-----|---------|
| `ER_MCP_PATH` / `ER_MCP_BINARY` | Use this binary instead of downloading |
| `XDG_CACHE_HOME` | Cache root (Linux/default) |

If no release asset exists yet, install from source:

```bash
cargo install --git https://github.com/VilfredSikker/easy-review --locked er-mcp
```

## Supported platforms

- macOS arm64 / x64
- Linux x64

## Version lockstep

`npm/er-mcp/package.json` `version` must match the Cargo workspace version and
the GitHub release tag (`vX.Y.Z`). Release CI publishes `er-mcp-<triple>.tar.gz`
assets consumed by this launcher.

## Develop

```bash
cd npm/er-mcp
npm test
node bin/er-mcp.js   # needs binary via PATH, ER_MCP_PATH, or a published release
```
