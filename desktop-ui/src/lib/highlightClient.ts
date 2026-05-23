import HighlightWorker from "./highlightWorker?worker";
import type { SyntaxTheme } from "./syntaxThemes";
import type { SpanSnapshot } from "./types";
import type {
  HighlightWorkerRequest,
  HighlightWorkerResponse,
} from "./highlightWorker";

let worker: Worker | null = null;
let nextId = 1;
const pending = new Map<
  number,
  {
    kind: "init" | "highlight";
    resolve: (value: SpanSnapshot[][] | void) => void;
    reject: (err: Error) => void;
  }
>();

function getWorker(): Worker {
  if (!worker) {
    worker = new HighlightWorker();
    worker.onmessage = (event: MessageEvent<HighlightWorkerResponse>) => {
      const msg = event.data;
      const entry = pending.get(msg.id);
      if (!entry) return;
      pending.delete(msg.id);
      if (msg.kind === "init") {
        entry.resolve();
      } else {
        entry.resolve(msg.spans);
      }
    };
    worker.onerror = (event) => {
      const err = new Error(event.message || "highlight worker error");
      for (const [, entry] of pending) entry.reject(err);
      pending.clear();
      worker?.terminate();
      worker = null;
    };
  }
  return worker;
}

function postRequest(
  request: HighlightWorkerRequest,
  kind: "init" | "highlight",
): Promise<SpanSnapshot[][] | void> {
  return new Promise((resolve, reject) => {
    pending.set(request.id, {
      kind,
      resolve: resolve as (value: SpanSnapshot[][] | void) => void,
      reject,
    });
    getWorker().postMessage(request);
  });
}

/** Prime Shiki in the worker so the first visible file highlights faster. */
export function warmHighlightWorker(theme: SyntaxTheme): void {
  const id = nextId++;
  const request: HighlightWorkerRequest = {
    kind: "init",
    id,
    themeName: theme.shikiName,
    themeJson: theme.customJson,
  };
  void postRequest(request, "init").catch(() => {});
}

export function highlightLines(
  lines: string[],
  lang: string | null,
  theme: SyntaxTheme,
): Promise<SpanSnapshot[][]> {
  if (lines.length === 0) return Promise.resolve([]);

  const id = nextId++;
  const request: HighlightWorkerRequest = {
    kind: "highlight",
    id,
    lang,
    themeName: theme.shikiName,
    themeJson: theme.customJson,
    lines,
  };

  return postRequest(request, "highlight") as Promise<SpanSnapshot[][]>;
}
