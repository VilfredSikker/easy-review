/** Registry entry for a syntax-highlighting theme (Shiki bundled name or custom JSON). */
export interface SyntaxTheme {
  id: string;
  label: string;
  shikiName: string;
  customJson?: Record<string, unknown>;
}

export const SYNTAX_THEMES: SyntaxTheme[] = [
  {
    id: "one-dark-pro",
    label: "One Dark Pro",
    shikiName: "one-dark-pro",
  },
  {
    id: "one-light",
    label: "One Light",
    shikiName: "one-light",
  },
  {
    id: "tokyo-night",
    label: "Tokyo Night",
    shikiName: "tokyo-night",
  },
  {
    id: "github-dark-high-contrast",
    label: "GitHub Dark High Contrast",
    shikiName: "github-dark-high-contrast",
  },
  {
    id: "github-light-high-contrast",
    label: "GitHub Light High Contrast",
    shikiName: "github-light-high-contrast",
  },
];

export const DEFAULT_SYNTAX_THEME_ID = "one-dark-pro";

export function syntaxThemeById(id: string): SyntaxTheme {
  return SYNTAX_THEMES.find((t) => t.id === id) ?? SYNTAX_THEMES[0];
}
