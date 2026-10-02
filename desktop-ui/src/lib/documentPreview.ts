import DOMPurify from "dompurify";
import { Marked } from "marked";

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

/** Sanitize first, then constrain even raw HTML links and images to external URLs. */
export function renderDocumentMarkdown(text: string): string {
  if (typeof document === "undefined") return escapeHtml(text);
  const fragment = DOMPurify.sanitize(parser.parse(text, { async: false }), {
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
