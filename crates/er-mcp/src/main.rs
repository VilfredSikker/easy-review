//! Easy Review MCP — triage open PRs from Cursor / Claude / other MCP clients.
//!
//! Transport: stdio (JSON-RPC). Logging goes to stderr so it does not corrupt the protocol.

mod projects;
mod server;

use anyhow::Result;
use rmcp::{transport::stdio, ServiceExt};
use std::io::IsTerminal;
use tracing_subscriber::EnvFilter;

fn print_tty_usage() {
    eprintln!(
        "\
easy-review MCP server (stdio JSON-RPC).

It waits for an MCP client on stdin — running it alone in a terminal looks idle.
Point your client at it instead, for example:

  Cursor  (~/.cursor/mcp.json):
    {{
      \"mcpServers\": {{
        \"easy-review\": {{
          \"command\": \"npx\",
          \"args\": [\"-y\", \"easy-review-mcp\"]
        }}
      }}
    }}

  Claude Code:
    claude mcp add --scope user easy-review -- npx -y easy-review-mcp

  Codex:
    codex mcp add easy-review -- npx -y easy-review-mcp

  OpenCode (~/.config/opencode/opencode.json):
    {{
      \"$schema\": \"https://opencode.ai/config.json\",
      \"mcp\": {{
        \"easy-review\": {{
          \"type\": \"local\",
          \"command\": [\"npx\", \"-y\", \"easy-review-mcp\"],
          \"enabled\": true
        }}
      }}
    }}

Docs: https://vilfredsikker.github.io/easy-review/guide/mcp.html"
    );
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_tty_usage();
        return Ok(());
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        eprintln!("er-mcp {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    // Interactive terminal (stdin+stdout TTYs) — explain instead of hanging.
    // MCP clients attach pipes, so this stays false for Claude/Cursor/Codex/OpenCode.
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        print_tty_usage();
        return Ok(());
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    let service = server::ErMcp::new()
        .serve(stdio())
        .await
        .inspect_err(|e| tracing::error!("serving error: {e:?}"))?;
    service.waiting().await?;
    Ok(())
}
