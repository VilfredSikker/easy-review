import type { GhCommentSummary } from "./types";

/** A PR or issue on GitHub, as a `#123` or URL in a PR body names it. */
export interface GithubRef {
  owner: string;
  repo: string;
  number: number;
  /** `pull` and `issue` come from a URL; a bare `#123` could be either. */
  kind: "pull" | "issue" | "unknown";
}

export type PrLinkKind = "preview" | "linear" | "pull" | "issue" | "ref" | "actions" | "sentry" | "generic";

/** What a row's buttons do. The first one is the row's main button. */
export type PrLinkAction = "browser-split" | "open-issue" | "open-tab" | "view-log" | "open" | "copy";

/** A deploy preview's state, as the bot comment that posted it reports it. */
export type DeployStatus = "ready" | "building" | "failed" | "canceled" | "ignored";

export interface PrLink {
  /** Normalised URL: one row per link however often it is repeated. */
  key: string;
  kind: PrLinkKind;
  url: string;
  title: string;
  detail: string;
  /** Short tag at the row's end: the status for a preview, else the link's type. */
  chip: string;
  status: DeployStatus | null;
  /** Where the link was found; the description outranks a bot. */
  source: "description" | "bot";
  ref: GithubRef | null;
  actions: PrLinkAction[];
}

export interface PrLinkSources {
  body: string;
  comments?: readonly GhCommentSummary[];
  /** The PR being reviewed: links back to itself are not listed. */
  owner?: string;
  repo?: string;
  number?: number;
}

const GITHUB_HOST = "github.com";

function parseUrl(raw: string): URL | null {
  try {
    const url = new URL(raw);
    return url.protocol === "https:" || url.protocol === "http:" ? url : null;
  } catch {
    return null;
  }
}

/** `owner/repo#12`, `#12` (resolved against the PR's repo) or a GitHub pull/issue URL. */
export function parseGithubRef(text: string, base?: { owner?: string; repo?: string }): GithubRef | null {
  const short = /^(?:([\w.-]+)\/([\w.-]+))?#(\d+)$/.exec(text.trim());
  if (short) {
    const owner = short[1] ?? base?.owner;
    const repo = short[2] ?? base?.repo;
    if (!owner || !repo) return null;
    return { owner, repo, number: Number(short[3]), kind: "unknown" };
  }
  const url = parseUrl(text.trim());
  if (!url || url.hostname !== GITHUB_HOST) return null;
  const [owner, repo, section, number] = url.pathname.split("/").filter(Boolean);
  if (!owner || !repo || !/^\d+$/.test(number ?? "")) return null;
  if (section === "pull") return { owner, repo, number: Number(number), kind: "pull" };
  if (section === "issues") return { owner, repo, number: Number(number), kind: "issue" };
  return null;
}

/** `https://github.com/o/r/issues/12` redirects to the PR when 12 is one, so it suits a bare `#12`. */
export function githubRefUrl(ref: GithubRef): string {
  return `https://${GITHUB_HOST}/${ref.owner}/${ref.repo}/${ref.kind === "pull" ? "pull" : "issues"}/${ref.number}`;
}

/** How a ref is opened: as a review tab, or on GitHub in the system browser. */
export interface RefOpeners {
  /** Opens the PR as a review tab; rejects when it cannot (an issue, no access). */
  openPr: (ref: GithubRef) => Promise<void>;
  openUrl: (url: string) => Promise<void>;
}

/**
 * Open a PR in a review tab, or its page on GitHub when it is not one. An issue
 * goes straight to the browser: `open_remote_pr` only caches PRs. A bare `#12`
 * may be either, so it tries the tab and falls back to the GitHub page.
 */
export async function openGithubRef(
  ref: GithubRef,
  openers: RefOpeners,
  url: string = githubRefUrl(ref),
): Promise<"tab" | "browser"> {
  if (ref.kind !== "issue") {
    try {
      await openers.openPr(ref);
      return "tab";
    } catch {
      // Not a PR this app can open; GitHub can still show it.
    }
  }
  await openers.openUrl(url);
  return "browser";
}

/** A bot's login: GitHub apps end in `[bot]`; some post as plain users. */
export function isBotAuthor(author: string): boolean {
  const login = author.toLowerCase();
  return login.endsWith("[bot]") || /^(vercel|netlify|linear|sentry-io|codecov|github-actions)$/.test(login);
}

// Text a link inside never navigates anywhere on GitHub: code, and the HTML
// comments PR templates leave behind.
function stripNonProse(md: string): string {
  return md
    .replace(/<!--[\s\S]*?-->/g, " ")
    .replace(/(^|\n)(`{3,}|~{3,})[^\n]*\n[\s\S]*?(\n\2[^\n]*|$)/g, "$1")
    .replace(/`[^`\n]+`/g, " ");
}

interface Found {
  url: string;
  label: string | null;
  /** The source line, for a bot's status column. */
  line: string;
  /** Inside `![…](…)` or an `<img>`: a picture, not a link. */
  image: boolean;
}

const URL_RE = /https?:\/\/[^\s<>"'`|\]]+/g;
const MD_LINK_RE = /(!?)\[([^\]\n]*)\]\((https?:\/\/[^\s)]+)(?:\s+"[^"]*")?\)/g;

function trimUrl(url: string): string {
  // Sentence punctuation and an unbalanced closing paren are not part of the URL.
  let out = url.replace(/[.,;:!?*_]+$/, "");
  while (out.endsWith(")") && (out.match(/\(/g)?.length ?? 0) < (out.match(/\)/g)?.length ?? 0)) {
    out = out.slice(0, -1);
  }
  return out;
}

function findUrls(md: string): Found[] {
  const found: Found[] = [];
  for (const line of stripNonProse(md).split("\n")) {
    const labelled = new Map<string, { label: string; image: boolean }>();
    for (const m of line.matchAll(MD_LINK_RE)) {
      labelled.set(m[3], { label: m[2].replace(/[*_`]/g, "").trim(), image: m[1] === "!" });
    }
    const images = new Set([...line.matchAll(/<img[^>]*\ssrc=["']([^"']+)["']/gi)].map((m) => m[1]));
    for (const m of line.matchAll(URL_RE)) {
      const url = trimUrl(m[0]);
      const link = labelled.get(url);
      found.push({
        url,
        label: link && !link.image && link.label && link.label !== url ? link.label : null,
        line,
        image: (link?.image ?? false) || images.has(url),
      });
    }
  }
  return found;
}

const STATUS_WORDS: [RegExp, DeployStatus][] = [
  [/\b(error|failed|failure)\b|❌/i, "failed"],
  [/\b(building|processing|queued|pending|in progress|deploying)\b|🔄|👷|⏳/i, "building"],
  [/\bcancel(?:l)?ed\b/i, "canceled"],
  [/\bignored\b/i, "ignored"],
  [/\b(ready|deployed|ready!|succeeded)\b|✅/i, "ready"],
];

/** The status a bot reports next to a preview: its table row first, then the comment's headline. */
export function deployStatus(line: string, comment: string): DeployStatus | null {
  for (const text of [line, comment]) {
    for (const [re, status] of STATUS_WORDS) if (re.test(text)) return status;
  }
  return null;
}

const STATUS_CHIP: Record<DeployStatus, string> = {
  ready: "Ready",
  building: "Building",
  failed: "Failed",
  canceled: "Canceled",
  ignored: "Ignored",
};

function slugTitle(slug: string | undefined): string {
  if (!slug) return "";
  const words = decodeURIComponent(slug).replace(/-/g, " ").trim();
  return words ? words[0].toUpperCase() + words.slice(1) : "";
}

// Hosts a link points at only to draw something: badges, avatars, the image CDN.
const NOISE_HOSTS = /(^|\.)(shields\.io|githubusercontent\.com|camo\.githubusercontent\.com|vercel\.live|vercel\.com|netlify\.com|app\.netlify\.com|codecov\.io)$/;

function isNoise(url: URL): boolean {
  if (NOISE_HOSTS.test(url.hostname)) return true;
  if (url.hostname !== GITHUB_HOST) return false;
  const [first, , section] = url.pathname.split("/").filter(Boolean);
  return first === "user-attachments" || first === "apps" || section === "assets";
}

interface ClassifyOpts {
  label?: string | null;
  /** The source line, for a bot's status column. */
  line?: string;
  /** The whole bot comment, for a status only its headline states. */
  comment?: string;
  source: "description" | "bot";
  self?: PrLinkSources;
}

/** What every row shares; each classifier fills in the rest. */
type LinkBase = Pick<PrLink, "key" | "url" | "status" | "source" | "ref">;
/** A row, `null` to drop the link outright, or `undefined` when not this kind. */
type Classifier = (url: URL, base: LinkBase, opts: ClassifyOpts) => PrLink | null | undefined;

const previewLink: Classifier = (url, base, opts) => {
  const host = url.hostname;
  const vercel = host.endsWith(".vercel.app");
  if (!vercel && !host.endsWith(".netlify.app")) return undefined;
  const sub = host.split(".")[0];
  // Vercel names the project in bold in its table; the subdomain is the fallback.
  const project = vercel
    ? (/\*\*([^*]+)\*\*/.exec(opts.line ?? "")?.[1] ?? sub.split("-git-")[0])
    : (sub.split("--")[1] ?? sub);
  const status = opts.source === "bot" ? deployStatus(opts.line ?? "", opts.comment ?? "") : null;
  const vendor = vercel ? "Vercel" : "Netlify";
  return {
    ...base,
    kind: "preview",
    title: `${vendor} preview · ${project}`,
    detail: host,
    chip: status ? STATUS_CHIP[status] : vendor,
    status,
    actions: ["browser-split", "copy"],
  };
};

const linearLink: Classifier = (url, base, opts) => {
  if (url.hostname !== "linear.app") return undefined;
  const [, section, issueKey, slug] = url.pathname.split("/").filter(Boolean);
  if (section !== "issue" || !issueKey) return undefined;
  const key = issueKey.toUpperCase();
  const name = opts.label && opts.label.toUpperCase() !== key ? opts.label : slugTitle(slug);
  return {
    ...base,
    key: `linear:${key}`,
    kind: "linear",
    title: name ? `${key} · ${name}` : key,
    detail: "Linear issue",
    chip: "Linear",
    actions: ["open-issue", "copy"],
  };
};

const githubRefLink: Classifier = (url, base, opts) => {
  if (url.hostname !== GITHUB_HOST) return undefined;
  const ref = parseGithubRef(url.href);
  if (!ref) return undefined;
  const self = opts.self;
  const sameRepo = self?.owner === ref.owner && self?.repo === ref.repo;
  if (sameRepo && self?.number === ref.number) return null;
  const where = sameRepo ? "" : `${ref.owner}/${ref.repo} · `;
  const shared = {
    ...base,
    key: `gh:${ref.owner}/${ref.repo}#${ref.number}`,
    ref,
    title: opts.label ? `#${ref.number} ${opts.label}` : `#${ref.number}`,
  };
  if (ref.kind === "pull") {
    return { ...shared, kind: "pull", detail: `${where}pull request`, chip: "PR", actions: ["open-tab", "copy"] };
  }
  return { ...shared, kind: "issue", detail: `${where}issue`, chip: "Issue", actions: ["open-issue", "copy"] };
};

const actionsLink: Classifier = (url, base, opts) => {
  if (url.hostname !== GITHUB_HOST) return undefined;
  const run = /^\/([^/]+)\/([^/]+)\/actions\/runs\/(\d+)(?:\/job\/(\d+))?/.exec(url.pathname);
  if (!run) return undefined;
  const job = run[4] ? ` · job ${run[4]}` : "";
  return {
    ...base,
    kind: "actions",
    title: opts.label ?? `Actions run ${run[3]}${job}`,
    detail: `${run[1]}/${run[2]} · GitHub Actions`,
    chip: "CI",
    actions: ["view-log", "copy"],
  };
};

const sentryLink: Classifier = (url, base, opts) => {
  const host = url.hostname;
  if (host !== "sentry.io" && !host.endsWith(".sentry.io")) return undefined;
  const issue = /\/issues\/(\d+)/.exec(url.pathname)?.[1];
  const org = host === "sentry.io" ? /\/organizations\/([^/]+)/.exec(url.pathname)?.[1] : host.split(".")[0];
  return {
    ...base,
    kind: "sentry",
    title: opts.label ?? (issue ? `Sentry issue ${issue}` : "Sentry"),
    detail: org ? `Sentry · ${org}` : "Sentry",
    chip: "Sentry",
    actions: ["open", "copy"],
  };
};

// Bots only add links of a known kind: their footers are full of docs and settings links.
const genericLink: Classifier = (url, base, opts) => {
  if (opts.source === "bot") return null;
  return {
    ...base,
    kind: "generic",
    title: opts.label ?? `${url.hostname}${url.pathname.replace(/\/+$/, "")}`,
    detail: url.hostname,
    chip: "Link",
    actions: ["open", "copy"],
  };
};

const CLASSIFIERS: readonly Classifier[] = [previewLink, linearLink, githubRefLink, actionsLink, sentryLink, genericLink];

/** One row for a URL, or null when it is not worth a row. The first classifier that knows the URL decides. */
export function classifyLink(raw: string, opts: ClassifyOpts): PrLink | null {
  const url = parseUrl(raw);
  if (!url || isNoise(url)) return null;
  const base: LinkBase = {
    key: `${url.origin}${url.pathname.replace(/\/+$/, "")}${url.search}`,
    url: raw,
    status: null,
    source: opts.source,
    ref: null,
  };
  for (const classify of CLASSIFIERS) {
    const link = classify(url, base, opts);
    if (link !== undefined) return link;
  }
  return null;
}

// Linear's magic words: a bare key only links an issue after one of these.
const LINEAR_KEY_RE = /\b(?:fix(?:es|ed)?|close[sd]?|resolve[sd]?|refs?|part of|related to|contributes to)\s*:?\s+([A-Z][A-Z0-9]{1,9}-\d+)\b/gi;

/** Bare `DEV-298` keys after a magic word, upper-cased and de-duplicated. */
export function linearKeys(md: string): string[] {
  const keys = new Set<string>();
  for (const m of stripNonProse(md).matchAll(LINEAR_KEY_RE)) {
    if (m[1] === m[1].toUpperCase()) keys.add(m[1]);
  }
  return [...keys];
}

const REF_RE = /(^|[\s(,;])(?:([\w.-]+)\/([\w.-]+))?#(\d+)\b/g;

/** `#12` and `owner/repo#12` in prose, skipping headings' `#`, code and HTML entities. */
export function bareRefs(md: string, base: { owner?: string; repo?: string }): GithubRef[] {
  const refs: GithubRef[] = [];
  for (const m of stripNonProse(md).replace(URL_RE, " ").matchAll(REF_RE)) {
    const ref = parseGithubRef(`${m[2] ? `${m[2]}/${m[3]}` : ""}#${m[4]}`, base);
    if (ref) refs.push(ref);
  }
  return refs;
}

const KIND_ORDER: Record<PrLinkKind, number> = {
  preview: 0, linear: 1, pull: 2, issue: 3, ref: 4, actions: 5, sentry: 6, generic: 7,
};

/**
 * The Links section: every link in the description, plus the typed links bots
 * comment (deploy previews with their status, Linear issues, CI runs, Sentry).
 * A link both mention keeps the description's label and the bot's status.
 */
export function extractPrLinks(src: PrLinkSources): PrLink[] {
  const rows = new Map<string, PrLink>();
  const add = (link: PrLink | null) => {
    if (!link) return;
    const prev = rows.get(link.key);
    if (!prev) {
      rows.set(link.key, link);
      return;
    }
    const status = prev.status ?? link.status;
    rows.set(link.key, {
      ...prev,
      status,
      chip: status && prev.kind === "preview" ? STATUS_CHIP[status] : prev.chip,
    });
  };

  const linearWorkspace = new Set<string>();
  const scan = (md: string, source: "description" | "bot", comment = "") => {
    for (const f of findUrls(md)) {
      if (f.image) continue;
      const url = parseUrl(f.url);
      if (url?.hostname === "linear.app") {
        const ws = url.pathname.split("/").filter(Boolean)[0];
        if (ws) linearWorkspace.add(ws);
      }
      add(classifyLink(f.url, { label: f.label, line: f.line, comment, source, self: src }));
    }
  };

  scan(src.body, "description");
  for (const c of src.comments ?? []) {
    if (isBotAuthor(c.author)) scan(c.body, "bot", c.body);
  }

  for (const key of linearKeys(src.body)) {
    if (rows.has(`linear:${key}`)) continue;
    const ws = linearWorkspace.size === 1 ? [...linearWorkspace][0] : null;
    if (!ws) continue;
    add(classifyLink(`https://linear.app/${ws}/issue/${key}`, { source: "description", self: src }));
  }

  for (const ref of bareRefs(src.body, src)) {
    if (src.number === ref.number && src.owner === ref.owner && src.repo === ref.repo) continue;
    const key = `gh:${ref.owner}/${ref.repo}#${ref.number}`;
    if (rows.has(key)) continue;
    const sameRepo = src.owner === ref.owner && src.repo === ref.repo;
    rows.set(key, {
      key,
      kind: "ref",
      url: githubRefUrl(ref),
      title: `${sameRepo ? "" : `${ref.owner}/${ref.repo}`}#${ref.number}`,
      detail: sameRepo ? "pull request or issue" : `${ref.owner}/${ref.repo} · pull request or issue`,
      chip: "Ref",
      status: null,
      source: "description",
      ref,
      actions: ["open-tab", "open", "copy"],
    });
  }

  return [...rows.values()]
    .map((link, i) => ({ link, i }))
    .sort((a, b) => KIND_ORDER[a.link.kind] - KIND_ORDER[b.link.kind] || a.i - b.i)
    .map(({ link }) => link);
}

/** Label for an action button. */
export const ACTION_LABEL: Record<PrLinkAction, string> = {
  "browser-split": "Open in browser split",
  "open-issue": "Open issue",
  "open-tab": "Open in tab",
  "view-log": "View log",
  open: "Open",
  copy: "Copy link",
};
