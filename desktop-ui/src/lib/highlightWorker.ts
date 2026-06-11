import { createBundledHighlighter } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";
import type { HighlighterGeneric, ThemeInput } from "shiki";
import type { SpanSnapshot } from "./types";

const DEFAULT_THEME = "one-dark-pro";

const bundledLangs = {
  typescript: () => import("@shikijs/langs/typescript"),
  tsx: () => import("@shikijs/langs/tsx"),
  javascript: () => import("@shikijs/langs/javascript"),
  jsx: () => import("@shikijs/langs/jsx"),
  rust: () => import("@shikijs/langs/rust"),
  python: () => import("@shikijs/langs/python"),
  svelte: () => import("@shikijs/langs/svelte"),
  html: () => import("@shikijs/langs/html"),
  css: () => import("@shikijs/langs/css"),
  json: () => import("@shikijs/langs/json"),
  toml: () => import("@shikijs/langs/toml"),
  yaml: () => import("@shikijs/langs/yaml"),
  markdown: () => import("@shikijs/langs/markdown"),
  bash: () => import("@shikijs/langs/bash"),
  sql: () => import("@shikijs/langs/sql"),
  go: () => import("@shikijs/langs/go"),
  mdx: () => import("@shikijs/langs/mdx"),
  scss: () => import("@shikijs/langs/scss"),
  jsonc: () => import("@shikijs/langs/jsonc"),
} as const;

const bundledThemes = {
  "one-dark-pro": () => import("@shikijs/themes/one-dark-pro"),
  "one-light": () => import("@shikijs/themes/one-light"),
  "tokyo-night": () => import("@shikijs/themes/tokyo-night"),
  "github-dark-high-contrast": () =>
    import("@shikijs/themes/github-dark-high-contrast"),
} as const;

type BundledLangId = keyof typeof bundledLangs;
type BundledThemeId = keyof typeof bundledThemes;

const createHighlighter = createBundledHighlighter({
  langs: bundledLangs,
  themes: bundledThemes,
  engine: () => createJavaScriptRegexEngine(),
});

type WorkerHighlighter = HighlighterGeneric<BundledLangId, BundledThemeId>;

export type HighlightWorkerRequest =
  | {
      kind: "init";
      id: number;
      themeName: string;
      themeJson?: Record<string, unknown>;
    }
  | {
      kind: "highlight";
      id: number;
      lang: string | null;
      themeName: string;
      themeJson?: Record<string, unknown>;
      lines: string[];
    };

export type HighlightWorkerResponse =
  | { kind: "init"; id: number }
  | { kind: "highlight"; id: number; spans: SpanSnapshot[][] };

let highlighter: WorkerHighlighter | null = null;
const loadedThemes = new Set<string>();
const loadedLangs = new Set<BundledLangId>();
let initPromise: Promise<WorkerHighlighter> | null = null;

async function ensureHighlighter(): Promise<WorkerHighlighter> {
  if (highlighter) return highlighter;
  if (!initPromise) {
    initPromise = createHighlighter({
      themes: [DEFAULT_THEME],
      langs: [],
    }).then((hl) => {
      highlighter = hl;
      loadedThemes.add(DEFAULT_THEME);
      return hl;
    });
  }
  return initPromise;
}

/** Load the requested theme and return the name to tokenize with (falls back to the default). */
async function ensureTheme(
  hl: WorkerHighlighter,
  themeName: string,
  themeJson?: Record<string, unknown>,
): Promise<string> {
  if (themeJson) {
    const key = `custom:${themeName}`;
    if (!loadedThemes.has(key)) {
      await hl.loadTheme(themeJson as ThemeInput);
      loadedThemes.add(key);
    }
    return (themeJson.name as string | undefined) ?? themeName;
  }
  const name: BundledThemeId =
    themeName in bundledThemes ? (themeName as BundledThemeId) : DEFAULT_THEME;
  if (!loadedThemes.has(name)) {
    await hl.loadTheme(name);
    loadedThemes.add(name);
  }
  return name;
}

async function ensureLang(hl: WorkerHighlighter, lang: string): Promise<boolean> {
  if (loadedLangs.has(lang as BundledLangId)) return true;
  if (!(lang in bundledLangs)) {
    console.warn(`[highlightWorker] unknown lang ${lang}`);
    return false;
  }
  try {
    await hl.loadLanguage(lang as BundledLangId);
    loadedLangs.add(lang as BundledLangId);
    return true;
  } catch (err) {
    console.warn(`[highlightWorker] failed to load lang ${lang}`, err);
    return false;
  }
}

function tokensToSpans(
  hl: WorkerHighlighter,
  code: string,
  lang: BundledLangId,
  themeName: string,
): SpanSnapshot[][] {
  const { tokens } = hl.codeToTokens(code, {
    lang,
    theme: themeName as BundledThemeId,
  });
  return tokens.map((line) =>
    line.map((token) => ({
      text: token.content,
      color: token.color ?? "",
    })),
  );
}

let workChain: Promise<void> = Promise.resolve();

function enqueue(task: () => Promise<void>): void {
  workChain = workChain.then(task).catch((err) => {
    console.error("[highlightWorker]", err);
  });
}

self.onmessage = (event: MessageEvent<HighlightWorkerRequest>) => {
  const msg = event.data;
  enqueue(async () => {
    try {
      if (msg.kind === "init") {
        const hl = await ensureHighlighter();
        await ensureTheme(hl, msg.themeName, msg.themeJson);
        self.postMessage({ kind: "init", id: msg.id } satisfies HighlightWorkerResponse);
        return;
      }

      const { id, lang, themeName, themeJson, lines } = msg;
      if (!lang || lines.length === 0) {
        const empty = lines.map(() => [] as SpanSnapshot[]);
        self.postMessage({ kind: "highlight", id, spans: empty } satisfies HighlightWorkerResponse);
        return;
      }

      const hl = await ensureHighlighter();
      const resolvedTheme = await ensureTheme(hl, themeName, themeJson);
      const langReady = await ensureLang(hl, lang);
      if (!langReady || !loadedLangs.has(lang as BundledLangId)) {
        const empty = lines.map(() => [] as SpanSnapshot[]);
        self.postMessage({ kind: "highlight", id, spans: empty } satisfies HighlightWorkerResponse);
        return;
      }

      const code = lines.join("\n");
      const spans = tokensToSpans(hl, code, lang as BundledLangId, resolvedTheme);
      self.postMessage({ kind: "highlight", id, spans } satisfies HighlightWorkerResponse);
    } catch (err) {
      console.error("[highlightWorker]", err);
      if (msg.kind === "init") {
        self.postMessage({ kind: "init", id: msg.id } satisfies HighlightWorkerResponse);
      } else {
        const empty = msg.lines.map(() => [] as SpanSnapshot[]);
        self.postMessage({
          kind: "highlight",
          id: msg.id,
          spans: empty,
        } satisfies HighlightWorkerResponse);
      }
    }
  });
};
