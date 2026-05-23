const EXT_TO_LANG: Record<string, string> = {
  ts: "typescript",
  tsx: "tsx",
  mts: "typescript",
  cts: "typescript",
  js: "javascript",
  jsx: "jsx",
  mjs: "javascript",
  cjs: "javascript",
  rs: "rust",
  py: "python",
  pyw: "python",
  svelte: "svelte",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  json: "json",
  jsonc: "jsonc",
  toml: "toml",
  yaml: "yaml",
  yml: "yaml",
  md: "markdown",
  mdx: "mdx",
  sh: "bash",
  bash: "bash",
  zsh: "bash",
  sql: "sql",
  go: "go",
};

/** Map a file path to a Shiki language id, or `null` when highlighting should be skipped. */
export function langForPath(path: string): string | null {
  const base = path.split(/[/\\]/).pop() ?? path;
  const dot = base.lastIndexOf(".");
  if (dot <= 0 || dot === base.length - 1) return null;
  const ext = base.slice(dot + 1).toLowerCase();
  return EXT_TO_LANG[ext] ?? null;
}
