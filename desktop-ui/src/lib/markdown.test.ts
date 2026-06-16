import { describe, expect, it } from "bun:test";
import { parseMarkdown, renderInline, type MarkdownNode } from "./markdown";

function tableOf(nodes: MarkdownNode[]) {
  const t = nodes.find((n) => n.t === "table");
  if (!t || t.t !== "table") throw new Error("no table node");
  return t;
}

describe("parseMarkdown tables", () => {
  it("parses a basic GFM table", () => {
    const md = ["| Term | Meaning |", "| --- | --- |", "| type | A keyword |", "| as const | Seals an array |"].join("\n");
    const t = tableOf(parseMarkdown(md));
    expect(t.header).toEqual(["Term", "Meaning"]);
    expect(t.rows).toEqual([
      ["type", "A keyword"],
      ["as const", "Seals an array"],
    ]);
  });

  it("reads alignment from the delimiter row", () => {
    const md = ["| L | C | R |", "| :--- | :--: | ---: |", "| a | b | c |"].join("\n");
    const t = tableOf(parseMarkdown(md));
    expect(t.align).toEqual(["left", "center", "right"]);
  });

  it("supports tables without outer pipes", () => {
    const md = ["a | b", "--- | ---", "1 | 2"].join("\n");
    const t = tableOf(parseMarkdown(md));
    expect(t.header).toEqual(["a", "b"]);
    expect(t.rows).toEqual([["1", "2"]]);
  });

  it("honors escaped pipes inside cells", () => {
    const md = ["| a | b |", "| --- | --- |", "| x \\| y | z |"].join("\n");
    const t = tableOf(parseMarkdown(md));
    expect(t.rows[0]).toEqual(["x | y", "z"]);
  });

  it("does not treat a pipe line without a delimiter row as a table", () => {
    const md = "this | is not | a table";
    const nodes = parseMarkdown(md);
    expect(nodes.every((n) => n.t !== "table")).toBe(true);
    expect(nodes[0]).toEqual({ t: "p", v: "this | is not | a table" });
  });

  it("does not treat prose above a dashed rule as a table (column count must match)", () => {
    const md = ["a | b", "-----", "c | d"].join("\n");
    const nodes = parseMarkdown(md);
    expect(nodes.every((n) => n.t !== "table")).toBe(true);
  });

  it("ends the table at a blank line and resumes normal parsing", () => {
    const md = ["| a | b |", "| --- | --- |", "| 1 | 2 |", "", "after"].join("\n");
    const nodes = parseMarkdown(md);
    const t = tableOf(nodes);
    expect(t.rows).toEqual([["1", "2"]]);
    expect(nodes[nodes.length - 1]).toEqual({ t: "p", v: "after" });
  });

  it("renders inline markup inside cells", () => {
    expect(renderInline("**bold** and `code`")).toBe("<strong>bold</strong> and <code>code</code>");
  });

  it("escapes quotes so a link URL cannot break out of the href attribute", () => {
    const html = renderInline('[hi](https://e.com"onmouseover="alert(1))');
    expect(html).not.toContain('"onmouseover="');
    expect(html).toContain("&quot;onmouseover=");
  });
});
