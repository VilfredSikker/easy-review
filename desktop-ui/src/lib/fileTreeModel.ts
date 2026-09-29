import { findingPassesTrust } from "$lib/diffAnnotations";
import type { AiSnapshot, Confidence } from "$lib/types";

/// Findings the confidence gate is holding back, counted per file. `minTrust`
/// resolves the reader's gate from the engine's default.
export function hiddenFindingCounts(
  ai: Pick<AiSnapshot, "findings" | "min_trust_default"> | null | undefined,
  minTrust: (fallback: Confidence) => Confidence,
): Map<string, number> {
  const counts = new Map<string, number>();
  if (!ai) return counts;
  const gate = minTrust(ai.min_trust_default);
  for (const finding of ai.findings) {
    if (findingPassesTrust(finding, gate)) continue;
    counts.set(finding.file, (counts.get(finding.file) ?? 0) + 1);
  }
  return counts;
}

/// A fresh selection with `path` flipped; the input is never mutated, because
/// the parent owns it and decides whether to accept the change.
export function toggledPath(selected: ReadonlySet<string> | undefined, path: string): Set<string> {
  const next = new Set(selected ?? []);
  if (next.has(path)) next.delete(path);
  else next.add(path);
  return next;
}

/// A fresh selection with every path in `paths` removed when `allSelected`,
/// otherwise added.
export function toggledPaths(
  selected: ReadonlySet<string> | undefined,
  paths: string[],
  allSelected: boolean,
): Set<string> {
  const next = new Set(selected ?? []);
  for (const p of paths) {
    if (allSelected) next.delete(p);
    else next.add(p);
  }
  return next;
}

export interface ExtChip { label: string; color: string }

const EXT_CHIPS: Record<string, ExtChip> = {
  ts: { label: "TS", color: "var(--color-action)" },
  tsx: { label: "TSX", color: "var(--color-action)" },
  js: { label: "JS", color: "var(--color-warning)" },
  jsx: { label: "JSX", color: "var(--color-warning)" },
  svelte: { label: "SV", color: "var(--color-accent)" },
  css: { label: "CSS", color: "var(--color-periwinkle)" },
  scss: { label: "SCS", color: "var(--color-periwinkle)" },
  rs: { label: "RS", color: "var(--color-emphasis)" },
  md: { label: "MD", color: "var(--color-fg-3)" },
  json: { label: "JSON", color: "var(--color-fg-3)" },
  toml: { label: "TOML", color: "var(--color-fg-3)" },
  yaml: { label: "YML", color: "var(--color-fg-3)" },
  yml: { label: "YML", color: "var(--color-fg-3)" },
  html: { label: "HTML", color: "var(--color-emphasis)" },
  py: { label: "PY", color: "var(--color-success)" },
  go: { label: "GO", color: "var(--color-info)" },
  sh: { label: "SH", color: "var(--color-fg-3)" },
  bash: { label: "SH", color: "var(--color-fg-3)" },
};

export function extChip(ext: string): ExtChip {
  if (Object.hasOwn(EXT_CHIPS, ext)) return EXT_CHIPS[ext];
  return { label: ext ? ext.toUpperCase().slice(0, 3) : "·", color: "var(--color-muted)" };
}
