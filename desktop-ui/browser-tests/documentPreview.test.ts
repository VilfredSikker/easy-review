import { afterAll, beforeAll, describe, expect, it } from "bun:test";
import { chromium, type Browser, type Page } from "playwright";
import type { DocumentSegment } from "../src/lib/documentPreview";

let browser: Browser;
let page: Page;
let server: ReturnType<typeof Bun.serve>;

beforeAll(async () => {
  const build = await Bun.build({
    entrypoints: [new URL("../src/lib/documentPreview.ts", import.meta.url).pathname],
    target: "browser",
  });
  if (!build.success) throw new Error(String(build.logs));
  const bundle = await build.outputs[0].text();
  server = Bun.serve({ port: 0, fetch(request) {
    if (new URL(request.url).pathname === "/renderer.js") {
      return new Response(bundle, { headers: { "Content-Type": "text/javascript" } });
    }
    return new Response('<script type="module">import {renderDocumentMarkdown, documentMarkdownSegments} from "/renderer.js"; window.renderDocumentMarkdown=renderDocumentMarkdown; window.documentMarkdownSegments=documentMarkdownSegments;</script>', {
      headers: { "Content-Type": "text/html" },
    });
  } });
  browser = await chromium.launch({ headless: true });
  page = await browser.newPage();
  await page.goto(server.url.toString());
  await page.waitForFunction(() => "renderDocumentMarkdown" in window);
}, 20_000);

afterAll(async () => {
  await browser?.close();
  server?.stop(true);
});

async function render(text: string): Promise<string> {
  return page.evaluate((source) => {
    const renderMarkdown = Reflect.get(window, "renderDocumentMarkdown") as (source: string) => string;
    return renderMarkdown(source);
  }, text);
}

describe("document Markdown browser sanitization", () => {
  it("renders GFM tables, tasks, nested lists, fenced code and strikethrough", async () => {
    const html = await render("# Title\n\n- [x] done\n- [ ] pending\n  - nested\n\n| a | b |\n| --- | --- |\n| one | two |\n\n~~old~~\n\n```js\nconst x = '<script>';\n```");
    for (const expected of ["<h1>Title</h1>", "☑", "☐", "<table>", "<del>old</del>", "<pre><code>", "&lt;script&gt;"]) {
      expect(html).toContain(expected);
    }
    expect(html).not.toContain("<input");
  });

  it("removes executable HTML, forms, styles, frames and embedded SVG", async () => {
    const html = await render('<script>alert(1)</script><iframe src="https://example.com"></iframe><form><input autofocus></form><svg onload="alert(1)"></svg><p style="color:red" onclick="alert(1)">safe</p><img src="https://example.com/a.png" onerror="alert(1)">');
    for (const forbidden of ["<script", "<iframe", "<form", "<input", "<svg", "style=", "onclick=", "onerror="]) {
      expect(html).not.toContain(forbidden);
    }
    expect(html).toContain("<p>safe</p>");
  });

  it("renders only HTTP(S) external links and HTTPS images", async () => {
    const html = await render('[site](http://example.com) [secure](https://example.com) [repo](./docs/guide.md) [anchor](#section) [unsafe](javascript:alert%281%29)\n\n![remote](https://example.com/image.png) ![insecure](http://example.com/image.png) ![local](./image.png) ![inline](data:image/png;base64,eA==)');
    expect(html.match(/<a /g)?.length).toBe(2);
    expect(html.match(/<img /g)?.length).toBe(1);
    expect(html).toContain('rel="noopener noreferrer"');
    expect(html).toContain('referrerpolicy="no-referrer"');
    for (const label of ["repo (./docs/guide.md)", "anchor (#section)", "insecure (http://example.com/image.png)", "local (./image.png)"]) {
      expect(html).toContain(label);
    }
  });

  it("applies the same target restrictions to raw HTML", async () => {
    const html = await render('<a href="./local.md">local</a><img src="http://example.com/a.png" alt="insecure"><img src="//example.com/a.png" alt="relative"><a href="javascript:alert(1)">unsafe</a>');
    expect(html).not.toContain("<a ");
    expect(html).not.toContain("<img ");
    expect(html).toContain("local (./local.md)");
    expect(html).toContain("insecure (http://example.com/a.png)");
    expect(html).toContain("relative (//example.com/a.png)");
  });
});

async function segments(text: string): Promise<DocumentSegment[]> {
  return page.evaluate((source) => {
    const split = Reflect.get(window, "documentMarkdownSegments") as (source: string) => DocumentSegment[];
    return split(source);
  }, text);
}

describe("document Markdown mermaid segments", () => {
  it("splits top-level mermaid fences out of the surrounding Markdown", async () => {
    const result = await segments("# Flow\n\nSee [docs][ref].\n\n```Mermaid title\nflowchart TB\n  A --> B\n```\n\n```js\nconst x = 1;\n```\n\n[ref]: https://example.com/docs");
    expect(result.map((segment) => segment.kind)).toEqual(["html", "mermaid", "html"]);
    expect(result[1]).toEqual({ kind: "mermaid", source: "flowchart TB\n  A --> B" });
    const [before, , after] = result.map((segment) => segment.kind === "html" ? segment.html : "");
    expect(before).toContain('<a href="https://example.com/docs"');
    expect(after).toContain("<pre><code");
  });

  it("keeps nested fences as code and sanitizes every HTML segment", async () => {
    const result = await segments("- item\n\n  ```mermaid\n  graph LR\n  ```\n\n```mermaid\ngraph LR\n```\n\n<script>alert(1)</script><img src=x onerror=alert(1)>");
    expect(result.map((segment) => segment.kind)).toEqual(["html", "mermaid", "html"]);
    const html = result.flatMap((segment) => segment.kind === "html" ? [segment.html] : []).join("");
    expect(html).toContain("<pre><code>graph LR\n</code></pre>");
    for (const forbidden of ["<script", "onerror=", "<img"]) expect(html).not.toContain(forbidden);
  });
});
