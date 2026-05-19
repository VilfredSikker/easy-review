/**
 * Shared color helper for agent log output lines.
 * Used by AgentOutputCard and RunningAgentPanel.
 */
export function sourceColor(source: string): string {
  if (source === "stderr") return "text-del-fg/80";
  if (source === "status") return "text-ink-400 italic";
  return "text-ink-200";
}
