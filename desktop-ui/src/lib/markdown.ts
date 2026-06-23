export type MarkdownNode =
  | { t: "p"; v: string }
  | { t: "h"; l: number; v: string }
  | { t: "ul"; items: string[] }
  | { t: "ol"; items: string[] }
  | { t: "bq"; v: string }
  | { t: "code"; lang: string; v: string }
  | { t: "table"; align: ("left" | "center" | "right" | null)[]; header: string[]; rows: string[][] };

/** Split a GFM table row into its cells, honoring escaped pipes (`\|`). */
function splitRow(line: string): string[] {
  let s = line.trim();
  if (s.startsWith("|")) s = s.slice(1);
  if (s.endsWith("|")) s = s.slice(0, -1);
  const cells: string[] = [];
  let cur = "";
  for (let i = 0; i < s.length; i++) {
    const ch = s[i];
    if (ch === "\\" && s[i + 1] === "|") {
      cur += "|";
      i++;
    } else if (ch === "|") {
      cells.push(cur.trim());
      cur = "";
    } else {
      cur += ch;
    }
  }
  cells.push(cur.trim());
  return cells;
}

/** A delimiter row is the second line of a GFM table, e.g. `| --- | :--: |`. */
function parseDelimiterRow(line: string): ("left" | "center" | "right" | null)[] | null {
  if (!line.includes("-")) return null;
  const cells = splitRow(line);
  const align: ("left" | "center" | "right" | null)[] = [];
  for (const cell of cells) {
    const m = cell.match(/^(:?)-+(:?)$/);
    if (!m) return null;
    const left = m[1] === ":";
    const right = m[2] === ":";
    align.push(left && right ? "center" : right ? "right" : left ? "left" : null);
  }
  return align.length ? align : null;
}

export function parseMarkdown(md: string): MarkdownNode[] {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const out: MarkdownNode[] = [];
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }
    const hm = line.match(/^(#{1,6})\s+(.*)$/);
    if (hm) {
      out.push({ t: "h", l: hm[1].length, v: hm[2] });
      i++;
      continue;
    }
    const cm = line.match(/^```(\w+)?\s*$/);
    if (cm) {
      const lang = cm[1] ?? "";
      i++;
      const code: string[] = [];
      while (i < lines.length && !lines[i].startsWith("```")) code.push(lines[i++]);
      if (i < lines.length) i++;
      out.push({ t: "code", lang, v: code.join("\n") });
      continue;
    }
    // GFM table: a row containing a pipe followed by a delimiter row whose
    // column count matches the header (else it's prose above a `---` rule).
    if (line.includes("|") && i + 1 < lines.length) {
      const align = parseDelimiterRow(lines[i + 1]);
      const header = align ? splitRow(line) : [];
      if (align && align.length === header.length) {
        i += 2;
        const rows: string[][] = [];
        while (i < lines.length && lines[i].trim() && lines[i].includes("|")) {
          rows.push(splitRow(lines[i]));
          i++;
        }
        out.push({ t: "table", align, header, rows });
        continue;
      }
    }
    if (line.startsWith("> ")) {
      const q: string[] = [];
      while (i < lines.length && lines[i].startsWith("> ")) q.push(lines[i++].slice(2));
      out.push({ t: "bq", v: q.join("\n") });
      continue;
    }
    const um = line.match(/^\s*[-*]\s+(.+)$/);
    if (um) {
      const items: string[] = [];
      while (i < lines.length) {
        const m = lines[i].match(/^\s*[-*]\s+(.+)$/);
        if (!m) break;
        items.push(m[1]);
        i++;
      }
      out.push({ t: "ul", items });
      continue;
    }
    const om = line.match(/^\s*\d+\.\s+(.+)$/);
    if (om) {
      const items: string[] = [];
      while (i < lines.length) {
        const m = lines[i].match(/^\s*\d+\.\s+(.+)$/);
        if (!m) break;
        items.push(m[1]);
        i++;
      }
      out.push({ t: "ol", items });
      continue;
    }
    const p: string[] = [line];
    i++;
    while (i < lines.length && lines[i].trim()) {
      if (/^(#{1,6})\s+/.test(lines[i]) || /^```/.test(lines[i])) break;
      p.push(lines[i++]);
    }
    out.push({ t: "p", v: p.join("\n") });
  }
  return out;
}

/**
 * Wrap bare http(s) URLs in anchors. Runs last, over already-generated HTML,
 * so it must skip URLs that are part of a tag we emitted: the preceding char
 * must not be `"` (an href value), `>` (anchor text / a code span), `=` (an
 * attribute), or a word char (mid-token). `^` covers the start of the string.
 */
function linkifyUrls(html: string): string {
  return html.replace(/(^|[^"=>\w])(https?:\/\/[^\s<]+)/g, (_full, pre: string, rawUrl: string) => {
    let url = rawUrl;
    let trail = "";
    // Peel trailing characters that are unlikely to belong to the URL:
    // sentence punctuation always, and a closing paren only when unbalanced
    // (so URLs that legitimately contain `(...)` survive).
    for (;;) {
      const punct = url.match(/[.,;:!?]$/);
      if (punct) {
        // Never strip the `;` that terminates an HTML entity (e.g. `&amp;`,
        // `&gt;`, `&#39;`) produced by escaping — doing so corrupts the URL.
        if (punct[0] === ";" && /&(?:#x?)?\w+;$/.test(url)) break;
        trail = url.slice(-1) + trail;
        url = url.slice(0, -1);
        continue;
      }
      const opens = (url.match(/\(/g) ?? []).length;
      const closes = (url.match(/\)/g) ?? []).length;
      if (url.endsWith(")") && closes > opens) {
        trail = ")" + trail;
        url = url.slice(0, -1);
        continue;
      }
      break;
    }
    return `${pre}<a href="${url}" rel="noreferrer">${url}</a>${trail}`;
  });
}

/** Render inline markdown (bold, italic, code, links, bare URLs) to safe HTML. */
export function renderInline(s: string): string {
  const escaped = s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
  return linkifyUrls(
    escaped
      .replace(/`([^`]+)`/g, "<code>$1</code>")
      .replace(/\*\*([^*]+)\*\*/g, "<strong>$1</strong>")
      .replace(/\*([^*]+)\*/g, "<em>$1</em>")
      .replace(/\[([^\]]+)\]\((https?:\/\/[^\s)]+)\)/g, '<a href="$2" rel="noreferrer">$1</a>'),
  );
}
