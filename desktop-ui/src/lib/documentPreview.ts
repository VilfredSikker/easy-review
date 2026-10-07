import DOMPurify from "dompurify";
import { Marked, type Token, type Tokens } from "marked";

export type DocumentPreviewState =
  | { status: "loading" }
  | { status: "ready"; text: string }
  | { status: "error"; message: string };

export function documentPreviewKind(path: string): "markdown" | "text" | null {
  if (!path.includes(".")) return null;
  const extension = path.slice(path.lastIndexOf(".") + 1).toLowerCase();
  if (["md", "markdown", "mdown", "mkd", "mkdn"].includes(extension)) return "markdown";
  return ["txt", "text"].includes(extension) ? "text" : null;
}

function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (character) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  })[character] ?? character);
}

function externalUrl(value: string, image = false): boolean {
  if (!/^https?:\/\//i.test(value)) return false;
  try {
    const protocol = new URL(value).protocol;
    return protocol === "https:" || (!image && protocol === "http:");
  } catch {
    return false;
  }
}

const parser = new Marked({
  gfm: true,
  async: false,
  renderer: {
    link({ href, title, tokens }) {
      const label = this.parser.parseInline(tokens);
      if (!externalUrl(href)) return `${label} (${escapeHtml(href)})`;
      return `<a href="${escapeHtml(href)}"${title ? ` title="${escapeHtml(title)}"` : ""}>${label}</a>`;
    },
    image({ href, text, title }) {
      if (!externalUrl(href, true)) return `${escapeHtml(text || "Image")} (${escapeHtml(href)})`;
      return `<img src="${escapeHtml(href)}" alt="${escapeHtml(text)}"${title ? ` title="${escapeHtml(title)}"` : ""}>`;
    },
    // Task lists retain their status without introducing interactive form controls.
    checkbox({ checked }) {
      return checked ? "☑ " : "☐ ";
    },
  },
});

export type DocumentSegment =
  | { kind: "html"; html: string }
  | { kind: "mermaid"; source: string };

function isMermaidFence(token: Token): token is Tokens.Code {
  return token.type === "code" && (token as Tokens.Code).lang?.trim().split(/\s+/)[0]?.toLowerCase() === "mermaid";
}

/**
 * Split a document at its top-level ```mermaid fences so the preview can draw
 * them as diagrams. Fences nested in lists or quotes stay code blocks.
 */
export function documentMarkdownSegments(text: string): DocumentSegment[] {
  if (typeof document === "undefined") return [{ kind: "html", html: escapeHtml(text) }];
  const segments: DocumentSegment[] = [];
  let pending: Token[] = [];
  const flush = () => {
    if (pending.length === 0) return;
    // Inline tokens, reference links included, are resolved during lexing.
    const html = sanitizeDocumentHtml(parser.parser(pending));
    if (html.trim()) segments.push({ kind: "html", html });
    pending = [];
  };
  for (const token of parser.lexer(text)) {
    if (isMermaidFence(token)) {
      flush();
      segments.push({ kind: "mermaid", source: token.text });
    } else {
      pending.push(token);
    }
  }
  flush();
  return segments;
}

/** One top-level markdown block and the 1-based source lines it came from. */
export interface DocumentBlock {
  startLine: number;
  endLine: number;
  segment: DocumentSegment;
}

function countNewlines(text: string): number {
  let count = 0;
  for (let i = text.indexOf("\n"); i !== -1; i = text.indexOf("\n", i + 1)) count++;
  return count;
}

/**
 * Render each top-level block on its own, tagged with its source line range,
 * so the side-by-side view can place it beside the raw lines it came from.
 * Line numbers come from the tokens' `raw` text, which marked guarantees
 * concatenates back to the input.
 */
export function documentBlocks(text: string): DocumentBlock[] {
  const tokens = parser.lexer(text);
  const blocks: DocumentBlock[] = [];
  let line = 1;
  for (const token of tokens) {
    const startLine = line;
    line += countNewlines(token.raw);
    // Blank lines and link definitions render nothing.
    if (token.type === "space" || token.type === "def") continue;
    const endLine = startLine + countNewlines(token.raw.trimEnd());
    if (isMermaidFence(token)) {
      blocks.push({ startLine, endLine, segment: { kind: "mermaid", source: token.text } });
      continue;
    }
    const html = typeof document === "undefined"
      ? escapeHtml(token.raw)
      : sanitizeDocumentHtml(parser.parser(Object.assign([token], { links: tokens.links })));
    if (html.trim()) blocks.push({ startLine, endLine, segment: { kind: "html", html } });
  }
  return blocks;
}

/**
 * Split a plain-text document into paragraphs at blank lines, tagged with
 * their source lines like `documentBlocks`, so a highlight in the preview
 * maps back to a line.
 */
export function documentTextBlocks(text: string): DocumentBlock[] {
  const blocks: DocumentBlock[] = [];
  const lines = text.split("\n");
  let start = 0;
  for (let i = 0; i <= lines.length; i++) {
    if (i < lines.length && lines[i].trim() !== "") continue;
    if (i > start) {
      const html = `<pre class="document-text">${escapeHtml(lines.slice(start, i).join("\n"))}</pre>`;
      blocks.push({ startLine: start + 1, endLine: i, segment: { kind: "html", html } });
    }
    start = i + 1;
  }
  return blocks;
}

export function renderDocumentMarkdown(text: string): string {
  if (typeof document === "undefined") return escapeHtml(text);
  return sanitizeDocumentHtml(parser.parse(text, { async: false }));
}

/** Sanitize first, then constrain even raw HTML links and images to external URLs. */
function sanitizeDocumentHtml(markup: string): string {
  const fragment = DOMPurify.sanitize(markup, {
    ALLOWED_TAGS: [
      "p", "br", "hr", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote",
      "ul", "ol", "li", "pre", "code", "strong", "em", "del", "s", "a", "img",
      "table", "thead", "tbody", "tr", "th", "td", "details", "summary", "kbd", "sup", "sub",
    ],
    ALLOWED_ATTR: ["href", "src", "alt", "title", "start", "colspan", "rowspan", "align"],
    ALLOW_DATA_ATTR: false,
    ALLOW_ARIA_ATTR: false,
    RETURN_DOM_FRAGMENT: true,
  });
  for (const anchor of fragment.querySelectorAll("a")) {
    const href = anchor.getAttribute("href");
    if (!href || !externalUrl(href)) {
      anchor.replaceWith(document.createTextNode(`${anchor.textContent || "Link"}${href ? ` (${href})` : ""}`));
    } else {
      anchor.setAttribute("rel", "noopener noreferrer");
      anchor.setAttribute("target", "_blank");
    }
  }
  for (const image of fragment.querySelectorAll("img")) {
    const src = image.getAttribute("src");
    if (!src || !externalUrl(src, true)) {
      image.replaceWith(document.createTextNode(`${image.getAttribute("alt") || "Image"}${src ? ` (${src})` : ""}`));
    } else {
      image.setAttribute("referrerpolicy", "no-referrer");
    }
  }
  const container = document.createElement("div");
  container.append(fragment);
  return container.innerHTML;
}
