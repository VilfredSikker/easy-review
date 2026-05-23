import { createBundledHighlighter } from "shiki/core";
import { createOnigurumaEngine } from "shiki/engine/oniguruma";
import type { HighlighterGeneric, ThemeInput } from "shiki";
import type { SpanSnapshot } from "./types";

const THEME_NAME = "one-dark-pro";

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
  [THEME_NAME]: () => import("@shikijs/themes/one-dark-pro"),
} as const;

type BundledLangId = keyof typeof bundledLangs;

const createHighlighter = createBundledHighlighter({
  langs: bundledLangs,
  themes: bundledThemes,
  engine: () => createOnigurumaEngine(import("shiki/wasm")),
});

type WorkerHighlighter = HighlighterGeneric<BundledLangId, typeof THEME_NAME>;

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
      themes: [THEME_NAME],
      langs: [
        "typescript",
        "tsx",
        "javascript",
        "jsx",
        "rust",
        "python",
        "svelte",
        "html",
        "css",
        "json",
        "toml",
        "yaml",
        "markdown",
        "bash",
        "sql",
        "go",
      ],
    }).then((hl) => {
      highlighter = hl;
      loadedThemes.add(THEME_NAME);
      for (const lang of [
        "typescript",
        "tsx",
        "javascript",
        "jsx",
        "rust",
        "python",
        "svelte",
        "html",
        "css",
        "json",
        "toml",
        "yaml",
        "markdown",
        "bash",
        "sql",
        "go",
      ] as const) {
        loadedLangs.add(lang);
      }
      return hl;
    });
  }
  return initPromise;
}

async function ensureTheme(
  hl: WorkerHighlighter,
  themeName: string,
  themeJson?: Record<string, unknown>,
) {
  const key = themeJson ? `custom:${themeName}` : themeName;
  if (loadedThemes.has(key)) return;
  if (themeJson) {
    await hl.loadTheme(themeJson as ThemeInput);
  } else if (themeName !== THEME_NAME) {
    await hl.loadTheme(themeName as typeof THEME_NAME);
  }
  loadedThemes.add(key);
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
  themeName: typeof THEME_NAME,
): SpanSnapshot[][] {
  const { tokens } = hl.codeToTokens(code, { lang, theme: themeName });
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
      await ensureTheme(hl, themeName, themeJson);
      const langReady = await ensureLang(hl, lang);
      if (!langReady || !loadedLangs.has(lang as BundledLangId)) {
        const empty = lines.map(() => [] as SpanSnapshot[]);
        self.postMessage({ kind: "highlight", id, spans: empty } satisfies HighlightWorkerResponse);
        return;
      }

      const code = lines.join("\n");
      const spans = tokensToSpans(hl, code, lang as BundledLangId, THEME_NAME);
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
