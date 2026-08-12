/**
 * Lazy-loaded mermaid renderer with a small SVG cache.
 *
 * Mermaid is a heavy dependency (~2 MB), so it is loaded on first use via
 * dynamic import and configured with `securityLevel: "strict"` (diagram source
 * is AI-generated — no click handlers, links, or raw HTML). Renders are cached
 * by `theme::source` so re-polls and modal open/close don't re-render.
 */

import type { MermaidConfig } from "mermaid";
import type { AppTheme } from "./themes";

type Mermaid = typeof import("mermaid").default;

let mermaidPromise: Promise<Mermaid> | null = null;
let initializedTheme: string | null = null;
let renderSeq = 0;

const CACHE_LIMIT = 40;
const svgCache = new Map<string, string>();

function cacheSet(key: string, svg: string) {
  if (svgCache.has(key)) svgCache.delete(key);
  svgCache.set(key, svg);
  while (svgCache.size > CACHE_LIMIT) {
    const oldest = svgCache.keys().next().value;
    if (oldest === undefined) break;
    svgCache.delete(oldest);
  }
}

async function getMermaid(): Promise<Mermaid> {
  if (!mermaidPromise) {
    mermaidPromise = import("mermaid").then((m) => m.default);
  }
  return mermaidPromise;
}

/** Mermaid `themeVariables` derived from an app theme (pure — exported for tests). */
export function mermaidThemeVariables(t: AppTheme): MermaidConfig["themeVariables"] {
  return {
    background: t.surface,
    primaryColor: t.panel,
    primaryTextColor: t.textBright,
    primaryBorderColor: t.border,
    lineColor: t.textMuted,
    secondaryColor: t.panel,
    tertiaryColor: t.surface,
    textColor: t.textBright,
    mainBkg: t.panel,
    nodeBorder: t.border,
    clusterBkg: "transparent",
    clusterBorder: t.border,
    edgeLabelBackground: t.surface,
    actorBkg: t.panel,
    actorBorder: t.border,
    actorTextColor: t.textBright,
    actorLineColor: t.textMuted,
    signalColor: t.textMuted,
    signalTextColor: t.textBright,
    labelBoxBkgColor: t.surface,
    labelBoxBorderColor: t.border,
    labelTextColor: t.textBright,
    loopTextColor: t.textBright,
    noteBkgColor: t.panel,
    noteBorderColor: t.border,
    noteTextColor: t.text,
    activationBorderColor: t.accent,
    sequenceNumberColor: t.onAccent,
  };
}

/** (Re-)initialize mermaid for the given app theme. Idempotent per theme name. */
async function ensureInitialized(theme: AppTheme): Promise<Mermaid> {
  const mermaid = await getMermaid();
  if (initializedTheme === theme.name) return mermaid;

  const config: MermaidConfig = {
    startOnLoad: false,
    securityLevel: "strict",
    theme: "base",
    // Mermaid injects an error graphic into the DOM on parse failure by
    // default; the component renders its own error state instead.
    suppressErrorRendering: true,
    fontFamily: "inherit",
    flowchart: { htmlLabels: false, curve: "basis" },
    themeVariables: mermaidThemeVariables(theme),
  };
  mermaid.initialize(config);
  initializedTheme = theme.name;
  return mermaid;
}

/**
 * Render mermaid source to an SVG string, themed to the given app theme.
 * Throws on parse errors — callers show their own error state.
 */
export async function renderMermaid(source: string, theme: AppTheme): Promise<string> {
  const cacheKey = `${theme.name}::${source}`;
  const cached = svgCache.get(cacheKey);
  if (cached) return cached;

  const mermaid = await ensureInitialized(theme);
  // mermaid.render needs a unique id per call; it appends temp nodes to the
  // document and removes them itself.
  const { svg } = await mermaid.render(`er-mmd-${renderSeq++}`, source);
  cacheSet(cacheKey, svg);
  return svg;
}
