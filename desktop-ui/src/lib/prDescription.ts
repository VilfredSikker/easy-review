import DOMPurify from "dompurify";
import { Marked, type Tokens } from "marked";
import { langForPath } from "./extToLang";
import { githubRefUrl, parseGithubRef, type GithubRef } from "./prLinks";

/** The repo a bare `#12` in the body belongs to. */
export interface DescriptionContext {
  owner?: string;
  repo?: string;
}

export type AlertVariant = "note" | "tip" | "important" | "warning" | "caution";

export const ALERT_TITLE: Record<AlertVariant, string> = {
  note: "Note",
  tip: "Tip",
  important: "Important",
  warning: "Warning",
  caution: "Caution",
};

const ALERT_GLYPH: Record<AlertVariant, string> = {
  note: "ⓘ",
  tip: "✦",
  important: "!",
  warning: "⚠",
  caution: "⛔",
};

function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c] ?? c);
}

/**
 * How an image in the body is loaded. GitHub's own attachment hosts can sit
 * behind the viewer's session (private repos), so they go through the
 * backend's authenticated fetch; a plain `<img>` would 404 or leak nothing
 * useful. Other https images load directly. Keep in step with `route` in
 * crates/er-desktop/src/github_image.rs.
 */
export function imageRoute(src: string): "github" | "direct" | "blocked" {
  let url: URL;
  try {
    url = new URL(src);
  } catch {
    return "blocked";
  }
  if (url.protocol !== "https:" || url.username || url.password || url.port) return "blocked";
  const host = url.hostname;
  if (host === "github.com") {
    const parts = url.pathname.split("/").filter(Boolean);
    const attachment = parts[0] === "user-attachments" && parts[1] === "assets";
    return attachment || parts[2] === "assets" ? "github" : "direct";
  }
  if (
    host === "raw.githubusercontent.com" ||
    host === "user-images.githubusercontent.com" ||
    host === "private-user-images.githubusercontent.com"
  ) {
    return "github";
  }
  return "direct";
}

// The workers' Shiki bundle; a fence tagged with anything else stays plain.
const SHIKI_LANGS = new Set([
  "typescript", "tsx", "javascript", "jsx", "rust", "python", "svelte", "html", "css", "json",
  "toml", "yaml", "markdown", "bash", "sql", "go", "mdx", "scss", "jsonc",
]);
const FENCE_ALIASES: Record<string, string> = { shell: "bash", console: "bash", rs: "rust", golang: "go" };

/** The Shiki language for a fence's info string (`ts`, `rust`, `sh`…), or null. */
export function fenceLanguage(info: string | null | undefined): string | null {
  const tag = info?.trim().split(/\s+/)[0]?.toLowerCase();
  if (!tag) return null;
  if (SHIKI_LANGS.has(tag)) return tag;
  if (FENCE_ALIASES[tag]) return FENCE_ALIASES[tag];
  return langForPath(`fence.${tag}`);
}

let context: DescriptionContext = {};

interface AlertToken extends Tokens.Generic {
  variant: AlertVariant;
  tokens: Tokens.Generic[];
}

const ALERT_RE = /^ {0,3}> ?\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\][ \t]*(?:\n|$)((?: {0,3}>[^\n]*(?:\n|$))*)/i;
const MENTION_RE = /^@([A-Za-z0-9](?:[A-Za-z0-9]|-(?=[A-Za-z0-9])){0,38})(?:\/([A-Za-z0-9][\w.-]*))?(?![\w@])/;
const REF_RE = /^(?:([\w.-]+)\/([\w.-]+))?#(\d+)(?!\w)/;

const parser = new Marked({
  gfm: true,
  async: false,
  extensions: [
    {
      // GitHub's `> [!WARNING]` callouts; a plain blockquote otherwise.
      name: "alert",
      level: "block",
      start: (src) => /^ {0,3}> ?\[!/m.exec(src)?.index,
      tokenizer(src) {
        const m = ALERT_RE.exec(src);
        if (!m) return undefined;
        const body = m[2].replace(/^ {0,3}> ?/gm, "");
        return { type: "alert", raw: m[0], variant: m[1].toLowerCase(), tokens: this.lexer.blockTokens(body, []) };
      },
      renderer(token) {
        const { variant, tokens } = token as AlertToken;
        return `<div class="md-alert md-alert-${variant}"><p class="md-alert-title">${ALERT_GLYPH[variant]} ${ALERT_TITLE[variant]}</p>${this.parser.parse(tokens)}</div>\n`;
      },
    },
    {
      name: "mention",
      level: "inline",
      start(src) {
        const m = /(^|[^\w`/@.])@[A-Za-z0-9]/.exec(src);
        return m ? m.index + m[1].length : undefined;
      },
      tokenizer(src) {
        // A link inside link text would nest anchors.
        if (this.lexer.state.inLink) return undefined;
        const m = MENTION_RE.exec(src);
        if (!m) return undefined;
        return { type: "mention", raw: m[0], login: m[1], team: m[2] ?? null };
      },
      renderer(token) {
        const { login, team } = token as Tokens.Generic & { login: string; team: string | null };
        const href = team
          ? `https://github.com/orgs/${encodeURIComponent(login)}/teams/${encodeURIComponent(team)}`
          : `https://github.com/${encodeURIComponent(login)}`;
        return `<a class="md-mention" href="${escapeHtml(href)}">@${escapeHtml(team ? `${login}/${team}` : login)}</a>`;
      },
    },
    {
      // `#12` and `owner/repo#12`, linked the way GitHub links them.
      name: "ghref",
      level: "inline",
      start(src) {
        const m = /(^|[\s(,;[])((?:[\w.-]+\/[\w.-]+)?#\d)/.exec(src);
        return m ? m.index + m[1].length : undefined;
      },
      tokenizer(src) {
        if (this.lexer.state.inLink) return undefined;
        const m = REF_RE.exec(src);
        if (!m) return undefined;
        const ref = parseGithubRef(m[0], context);
        if (!ref) return undefined;
        return { type: "ghref", raw: m[0], ref, text: m[0] };
      },
      renderer(token) {
        const { ref, text } = token as Tokens.Generic & { ref: GithubRef; text: string };
        return `<a class="md-ref" href="${escapeHtml(githubRefUrl(ref))}">${escapeHtml(text)}</a>`;
      },
    },
  ],
  renderer: {
    checkbox({ checked }) {
      return `<span class="md-tb" aria-label="${checked ? "done" : "to do"}">${checked ? "✓" : ""}</span>`;
    },
    list(token) {
      if (!token.items.some((item) => item.task)) return false;
      const items = token.items.map((item) => this.listitem(item)).join("");
      return `<ul class="md-tasks">\n${items}</ul>\n`;
    },
    listitem(item) {
      if (!item.task) return false;
      const box = this.checkbox({ checked: !!item.checked });
      const body = this.parser.parse(item.tokens, !!item.loose);
      return `<li class="md-task${item.checked ? " md-done" : ""}">${box}<span>${body}</span></li>\n`;
    },
    table(token) {
      const header = `<tr>${token.header.map((cell) => this.tablecell(cell)).join("")}</tr>`;
      const rows = token.rows.map((row) => `<tr>${row.map((cell) => this.tablecell(cell)).join("")}</tr>`).join("");
      // The wrapper scrolls; a wide table never widens the page.
      return `<div class="md-table-wrap"><table><thead>${header}</thead>${rows ? `<tbody>${rows}</tbody>` : ""}</table></div>\n`;
    },
    image({ href, text, title }) {
      return `<img class="md-img" src="${escapeHtml(href)}" alt="${escapeHtml(text)}"${title ? ` title="${escapeHtml(title)}"` : ""}>`;
    },
  },
});

/** The body as GitHub-flavoured HTML, before sanitizing. Exposed for tests. */
export function prMarkdownHtml(body: string, ctx: DescriptionContext = {}): string {
  context = ctx;
  try {
    return parser.parse(body, { async: false });
  } finally {
    context = {};
  }
}

const ALLOWED_TAGS = [
  "p", "br", "hr", "h1", "h2", "h3", "h4", "h5", "h6", "blockquote", "div", "span",
  "ul", "ol", "li", "pre", "code", "strong", "b", "em", "i", "del", "s", "ins", "mark", "a", "img", "picture", "source",
  "table", "thead", "tbody", "tr", "th", "td", "details", "summary", "kbd", "sup", "sub",
];
const ALLOWED_ATTR = ["href", "src", "srcset", "media", "alt", "title", "start", "colspan", "rowspan", "align", "class", "width", "height", "open", "aria-label"];
const CLASS_RE = /^(md-[a-z-]+|language-[\w+#.-]+)$/;

function httpUrl(value: string | null): value is string {
  if (!value) return false;
  try {
    const p = new URL(value).protocol;
    return p === "https:" || p === "http:";
  } catch {
    return false;
  }
}

/**
 * The ref a link opens as a tab, or null for a plain external link. Pull URLs
 * and the refs the `ghref` extension drew (a `#12` may be a PR) qualify; an
 * issue URL does not, since only PRs open as review tabs.
 */
function tabRef(anchor: Element, href: string): GithubRef | null {
  const ref = parseGithubRef(href);
  if (!ref) return null;
  if (anchor.classList.contains("md-ref")) return { ...ref, kind: "unknown" };
  return ref.kind === "pull" ? ref : null;
}

/** A click on a description link that opens a tab, as the component acts on it. */
export interface RefClick {
  ref: GithubRef;
  /** The link's GitHub URL, for the browser. */
  url: string;
  /** A plain left click opens a tab; a modified or middle click opens the browser. */
  plain: boolean;
}

/** The tab ref a click or auxclick landed on, or null when it is not on one. */
export function refClickTarget(e: MouseEvent): RefClick | null {
  if (e.button > 1) return null;
  const anchor = (e.target as Element | null)?.closest?.("a[data-er-ref]") as HTMLElement | null | undefined;
  if (!anchor) return null;
  const parsed = parseGithubRef(anchor.dataset.erRef ?? "");
  const url = anchor.dataset.erHref ?? null;
  if (!parsed || !httpUrl(url)) return null;
  const ref: GithubRef = { ...parsed, kind: anchor.dataset.erRefKind === "pull" ? "pull" : "unknown" };
  const plain = e.button === 0 && !e.metaKey && !e.ctrlKey && !e.shiftKey && !e.altKey;
  return { ref, url, plain };
}

/**
 * Render a PR description: GitHub-flavoured markdown, then DOMPurify, then the
 * attributes the component acts on. Those `data-er-*` attributes are set only
 * after sanitizing, from values this function checked, so a body cannot plant
 * them. Outside a DOM (tests, SSR) it fails closed to escaped text.
 */
export function renderPrDescription(body: string, ctx: DescriptionContext = {}): string {
  if (typeof document === "undefined") return escapeHtml(body);
  const fragment = DOMPurify.sanitize(prMarkdownHtml(body, ctx), {
    ALLOWED_TAGS,
    ALLOWED_ATTR,
    ALLOW_DATA_ATTR: false,
    RETURN_DOM_FRAGMENT: true,
  });

  for (const el of fragment.querySelectorAll("[class]")) {
    const kept = [...el.classList].filter((c) => CLASS_RE.test(c));
    if (kept.length) el.setAttribute("class", kept.join(" "));
    else el.removeAttribute("class");
  }

  for (const anchor of fragment.querySelectorAll("a")) {
    const href = anchor.getAttribute("href");
    if (!httpUrl(href)) {
      anchor.replaceWith(document.createTextNode(anchor.textContent || ""));
      continue;
    }
    anchor.setAttribute("rel", "noopener noreferrer");
    anchor.setAttribute("target", "_blank");
    const ref = tabRef(anchor, href);
    if (!ref) continue;
    // The app's external-link guard (a document capture listener) sends every
    // `a[href]` to the system browser before this component sees the click, so
    // a link that should open a tab carries its URL in data attributes instead.
    anchor.removeAttribute("href");
    anchor.removeAttribute("target");
    anchor.dataset.erRef = `${ref.owner}/${ref.repo}#${ref.number}`;
    anchor.dataset.erRefKind = ref.kind;
    anchor.dataset.erHref = href;
    anchor.setAttribute("role", "link");
    anchor.setAttribute("tabindex", "0");
  }

  // <picture> sources would let the browser pick an unchecked URL.
  for (const source of fragment.querySelectorAll("source")) source.remove();

  for (const image of fragment.querySelectorAll("img")) {
    const src = image.getAttribute("src") ?? "";
    image.removeAttribute("srcset");
    const route = imageRoute(src);
    if (route === "blocked") {
      image.replaceWith(document.createTextNode(image.getAttribute("alt") || "Image"));
      continue;
    }
    image.classList.add("md-img");
    image.setAttribute("loading", "lazy");
    image.setAttribute("decoding", "async");
    image.setAttribute("referrerpolicy", "no-referrer");
    if (route === "github") {
      image.removeAttribute("src");
      image.dataset.erSrc = src;
      image.classList.add("md-img-pending");
    }
  }

  const container = document.createElement("div");
  container.append(fragment);
  return container.innerHTML;
}
