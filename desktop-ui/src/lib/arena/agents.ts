/** Display metadata for specialized reviewers (`list_ai_reviewers` kinds). */

export interface AgentCatalogEntry {
  kind: string;
  label: string;
  description: string;
  color: string;
  /** Emoji glyph for agent cards (not ArenaIcons). */
  glyph: string;
}

const COLORS: Record<string, string> = {
  general: "#ff7a2b",
  professor: "#9b87f5",
  "expert:security": "#ff6b6b",
  "expert:performance": "#7f87ff",
  "expert:reliability": "#5fd970",
  "expert:testing": "#ffc457",
  "expert:api": "#4ec9a4",
  "expert:patterns": "#ff7a2b",
  "expert:simplifying": "#9b87f5",
  "expert:mentorship": "#4ec9a4",
};

const GLYPHS: Record<string, string> = {
  general: "✦",
  professor: "🎓",
  "expert:security": "🛡",
  "expert:performance": "⚡",
  "expert:reliability": "✓",
  "expert:testing": "🧪",
  "expert:api": "🔌",
  "expert:patterns": "🔍",
  "expert:simplifying": "✂",
  "expert:mentorship": "🤝",
};

export function agentCatalogEntry(
  kind: string,
  label: string,
  description: string,
): AgentCatalogEntry {
  return {
    kind,
    label,
    description,
    color: COLORS[kind] ?? "#8089a0",
    glyph: GLYPHS[kind] ?? "✦",
  };
}
