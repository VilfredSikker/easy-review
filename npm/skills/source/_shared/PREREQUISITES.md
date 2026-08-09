# Easy Review MCP — prerequisites

- MCP server **`easy-review`** connected (`npx -y easy-review-mcp` or `er-mcp` on PATH).
- Authenticated **`gh`** (`gh auth status`).
- Optional: Easy Review projects in `~/.config/er/projects.json` (Desktop writes this).

Managed PR storage (read `diff-tmp` only; never write here directly):

- macOS: `~/Library/Application Support/easy-review/repos/<owner-repo>/prs/pr-<N>/`
- Linux: `~/.local/share/easy-review/repos/<owner-repo>/prs/pr-<N>/`

Always upload sidecars through **`pr_upload`**, not by writing into the managed directory.
