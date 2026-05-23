/** Adjust Shiki token colors for readability on diff add/del backgrounds. */
const SPAN_COLOR_REMAP: Record<string, string> = {
  // one-dark-pro comment tones → readable gray on dark diff bg
  "#7f848e": "#a7b1ba",
  "#5c6370": "#a7b1ba",
  // one-dark-pro string green on add-bg → lighter teal
  "#98c379": "#d4f0e4",
  // Legacy OneHalfDark / Ocean fallbacks
  "#4f5b66": "#a7b1ba",
  "#343d46": "#a7b1ba",
  "#65737e": "#a7b1ba",
  "#6b6b6b": "#a7b1ba",
  "#5e5e5e": "#a7b1ba",
  "#99c794": "#d4f0e4",
  "#a3be8c": "#d4f0e4",
};

export function remapSpanColor(color: string): string {
  return color ? (SPAN_COLOR_REMAP[color.toLowerCase()] ?? color) : color;
}
