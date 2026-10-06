import { describe, expect, it } from "bun:test";
import { documentBlocks, documentPreviewKind, renderDocumentMarkdown } from "./documentPreview";

describe("document preview kind", () => {
  it("matches all supported extensions without case sensitivity", () => {
    for (const extension of ["md", "markdown", "mdown", "mkd", "mkdn"]) {
      expect(documentPreviewKind(`docs/file.${extension.toUpperCase()}`)).toBe("markdown");
    }
    expect(documentPreviewKind("notes.TXT")).toBe("text");
    expect(documentPreviewKind("notes.text")).toBe("text");
    for (const path of ["README", "file.html", "file.svg", "file.md.ts", "md"]) {
      expect(documentPreviewKind(path)).toBeNull();
    }
  });

  it("fails closed outside a browser DOM", () => {
    expect(renderDocumentMarkdown('<script>alert("x")</script>')).toBe("&lt;script&gt;alert(&quot;x&quot;)&lt;/script&gt;");
  });
});

describe("document blocks", () => {
  const ranges = (text: string) => documentBlocks(text).map((b) => [b.startLine, b.endLine, b.segment.kind]);

  it("gives each top-level block its source lines, skipping blank lines", () => {
    expect(ranges("# Title\n\nFirst line\nsecond line\n\n- one\n- two\n\n  continued\n")).toEqual([
      [1, 1, "html"],
      [3, 4, "html"],
      [6, 9, "html"],
    ]);
  });

  it("keeps later lines exact after link definitions, html and mermaid", () => {
    const text = "Para [a][r]\n\n[r]: https://x.y\n\n<div>\nhi\n</div>\n\n```mermaid\ngraph TD\n```\nlast";
    expect(ranges(text)).toEqual([
      [1, 1, "html"],
      [5, 7, "html"],
      [9, 11, "mermaid"],
      [12, 12, "html"],
    ]);
  });

  it("returns nothing for an empty document", () => {
    expect(documentBlocks("")).toEqual([]);
  });
});
