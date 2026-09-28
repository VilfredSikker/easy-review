export type MarkdownNode =
  | { t: "p"; v: string }
  | { t: "h"; l: number; v: string }
  | { t: "ul"; items: string[] }
  | { t: "ol"; items: string[] }
  | { t: "bq"; v: string }
  | { t: "code"; lang: string; v: string }
  | { t: "table"; align: CellAlign[]; header: string[]; rows: string[][] };

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

type CellAlign = "left" | "center" | "right" | null;

function cellAlign(left: boolean, right: boolean): CellAlign {
  if (left && right) return "center";
  if (right) return "right";
  if (left) return "left";
  return null;
}

/** A delimiter row is the second line of a GFM table, e.g. `| --- | :--: |`. */
function parseDelimiterRow(line: string): CellAlign[] | null {
  if (!line.includes("-")) return null;
  const cells = splitRow(line);
  const align: CellAlign[] = [];
  for (const cell of cells) {
    const m = cell.match(/^(:?)-+(:?)$/);
    if (!m) return null;
    align.push(cellAlign(m[1] === ":", m[2] === ":"));
  }
  return align.length ? align : null;
}

/** A block parsed at `lines[i]`, and the index of the first line after it. */
type BlockParse = { node: MarkdownNode; next: number } | null;

function parseHeading(lines: string[], i: number): BlockParse {
  const hm = lines[i].match(/^(#{1,6})\s+(.*)$/);
  if (!hm) return null;
  return { node: { t: "h", l: hm[1].length, v: hm[2] }, next: i + 1 };
}

function parseFence(lines: string[], start: number): BlockParse {
  const cm = lines[start].match(/^```(\w+)?\s*$/);
  if (!cm) return null;
  const lang = cm[1] ?? "";
  let i = start + 1;
  const code: string[] = [];
  while (i < lines.length && !lines[i].startsWith("```")) code.push(lines[i++]);
  if (i < lines.length) i++;
  return { node: { t: "code", lang, v: code.join("\n") }, next: i };
}

// GFM table: a row containing a pipe followed by a delimiter row whose
// column count matches the header (else it's prose above a `---` rule).
function parseTable(lines: string[], start: number): BlockParse {
  const line = lines[start];
  if (!line.includes("|") || start + 1 >= lines.length) return null;
  const align = parseDelimiterRow(lines[start + 1]);
  const header = align ? splitRow(line) : [];
  if (!align || align.length !== header.length) return null;
  let i = start + 2;
  const rows: string[][] = [];
  while (i < lines.length && lines[i].trim() && lines[i].includes("|")) {
    rows.push(splitRow(lines[i]));
    i++;
  }
  return { node: { t: "table", align, header, rows }, next: i };
}

function parseQuote(lines: string[], start: number): BlockParse {
  if (!lines[start].startsWith("> ")) return null;
  let i = start;
  const q: string[] = [];
  while (i < lines.length && lines[i].startsWith("> ")) q.push(lines[i++].slice(2));
  return { node: { t: "bq", v: q.join("\n") }, next: i };
}

function parseList(lines: string[], start: number, t: "ul" | "ol", item: RegExp): BlockParse {
  let i = start;
  const items: string[] = [];
  while (i < lines.length) {
    const m = lines[i].match(item);
    if (!m) break;
    items.push(m[1]);
    i++;
  }
  if (!items.length) return null;
  return { node: { t, items }, next: i };
}

function parseParagraph(lines: string[], start: number): { node: MarkdownNode; next: number } {
  const p: string[] = [lines[start]];
  let i = start + 1;
  while (i < lines.length && lines[i].trim()) {
    if (/^(#{1,6})\s+/.test(lines[i]) || /^```/.test(lines[i])) break;
    p.push(lines[i++]);
  }
  return { node: { t: "p", v: p.join("\n") }, next: i };
}

const BLOCK_PARSERS: ((lines: string[], i: number) => BlockParse)[] = [
  parseHeading,
  parseFence,
  parseTable,
  parseQuote,
  (lines, i) => parseList(lines, i, "ul", /^\s*[-*]\s+(.+)$/),
  (lines, i) => parseList(lines, i, "ol", /^\s*\d+\.\s+(.+)$/),
];

function parseBlock(lines: string[], i: number): { node: MarkdownNode; next: number } {
  for (const parse of BLOCK_PARSERS) {
    const block = parse(lines, i);
    if (block) return block;
  }
  return parseParagraph(lines, i);
}

export function parseMarkdown(md: string): MarkdownNode[] {
  const lines = md.replace(/\r\n/g, "\n").split("\n");
  const out: MarkdownNode[] = [];
  let i = 0;
  while (i < lines.length) {
    if (!lines[i].trim()) {
      i++;
      continue;
    }
    const block = parseBlock(lines, i);
    out.push(block.node);
    i = block.next;
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
