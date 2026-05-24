/** Visual identity for a file row — extension monogram or named icon slot. */
export interface FileTypeIconSpec {
  /** Shown on the generic monogram fallback (e.g. "TS", "RS"). */
  label: string;
  /** Tailwind classes for monogram fallback (bg + text). */
  monogramClass: string;
  /**
   * Named built-in SVG in `FileTypeIcon.svelte`. When set, monogram is ignored
   * except as accessible label.
   */
  icon?:
    | "typescript"
    | "javascript"
    | "svelte"
    | "rust"
    | "go"
    | "python"
    | "ruby"
    | "json"
    | "markdown"
    | "css"
    | "html"
    | "yaml"
    | "toml"
    | "shell"
    | "docker"
    | "image"
    | "lock"
    | "config"
    | "generic";
}

const EXT_RULES: { match: string[]; spec: FileTypeIconSpec }[] = [
  {
    match: ["ts", "tsx", "mts", "cts"],
    spec: { label: "TS", monogramClass: "bg-sky-500/25 text-sky-300", icon: "typescript" },
  },
  {
    match: ["js", "jsx", "mjs", "cjs"],
    spec: { label: "JS", monogramClass: "bg-amber-500/25 text-amber-200", icon: "javascript" },
  },
  {
    match: ["svelte"],
    spec: { label: "SV", monogramClass: "bg-orange-500/25 text-orange-200", icon: "svelte" },
  },
  {
    match: ["rs"],
    spec: { label: "RS", monogramClass: "bg-orange-600/25 text-orange-300", icon: "rust" },
  },
  {
    match: ["go"],
    spec: { label: "GO", monogramClass: "bg-cyan-500/25 text-cyan-200", icon: "go" },
  },
  {
    match: ["py", "pyi", "pyw"],
    spec: { label: "PY", monogramClass: "bg-blue-500/25 text-blue-200", icon: "python" },
  },
  {
    match: ["rb", "erb"],
    spec: { label: "RB", monogramClass: "bg-red-500/25 text-red-300", icon: "ruby" },
  },
  {
    match: ["json", "jsonc"],
    spec: { label: "{}", monogramClass: "bg-yellow-500/20 text-yellow-200", icon: "json" },
  },
  {
    match: ["md", "mdx"],
    spec: { label: "MD", monogramClass: "bg-slate-400/20 text-slate-200", icon: "markdown" },
  },
  {
    match: ["css", "scss", "sass", "less"],
    spec: { label: "CSS", monogramClass: "bg-pink-500/25 text-pink-200", icon: "css" },
  },
  {
    match: ["html", "htm"],
    spec: { label: "HT", monogramClass: "bg-orange-500/20 text-orange-200", icon: "html" },
  },
  {
    match: ["yaml", "yml"],
    spec: { label: "YML", monogramClass: "bg-violet-500/25 text-violet-200", icon: "yaml" },
  },
  {
    match: ["toml"],
    spec: { label: "TOML", monogramClass: "bg-neutral-500/25 text-neutral-300", icon: "toml" },
  },
  {
    match: ["sh", "bash", "zsh", "fish"],
    spec: { label: "SH", monogramClass: "bg-emerald-500/25 text-emerald-200", icon: "shell" },
  },
  {
    match: ["png", "jpg", "jpeg", "gif", "webp", "svg", "ico"],
    spec: { label: "IMG", monogramClass: "bg-purple-500/25 text-purple-200", icon: "image" },
  },
  {
    match: ["lock"],
    spec: { label: "LK", monogramClass: "bg-muted/30 text-muted", icon: "lock" },
  },
];

const BASENAME_RULES: { match: string; spec: FileTypeIconSpec }[] = [
  {
    match: "dockerfile",
    spec: { label: "DK", monogramClass: "bg-sky-600/25 text-sky-200", icon: "docker" },
  },
  {
    match: "package.json",
    spec: { label: "npm", monogramClass: "bg-emerald-500/20 text-emerald-200", icon: "json" },
  },
  {
    match: "cargo.toml",
    spec: { label: "crate", monogramClass: "bg-orange-600/20 text-orange-200", icon: "toml" },
  },
  {
    match: ".gitignore",
    spec: { label: "git", monogramClass: "bg-orange-500/20 text-orange-200", icon: "config" },
  },
];

function extensionOf(path: string): string {
  const base = path.split("/").pop() ?? path;
  const dot = base.lastIndexOf(".");
  if (dot <= 0) return "";
  return base.slice(dot + 1).toLowerCase();
}

function basenameOf(path: string): string {
  return (path.split("/").pop() ?? path).toLowerCase();
}

export function fileTypeIcon(path: string): FileTypeIconSpec {
  const base = basenameOf(path);
  for (const rule of BASENAME_RULES) {
    if (base === rule.match || base.startsWith(rule.match)) return rule.spec;
  }
  const ext = extensionOf(path);
  for (const rule of EXT_RULES) {
    if (rule.match.includes(ext)) return rule.spec;
  }
  if (ext) {
    const label = ext.length <= 3 ? ext.toUpperCase() : ext.slice(0, 3).toUpperCase();
    return { label, monogramClass: "bg-panel text-fg-3", icon: "generic" };
  }
  return { label: "·", monogramClass: "bg-panel text-fg-3", icon: "generic" };
}
