import { describe, expect, it } from "bun:test";
import { documentPreviewKind, renderDocumentMarkdown } from "./documentPreview";

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
