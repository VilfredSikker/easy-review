import { describe, expect, it } from "bun:test";
import { fenceLanguage, imageRoute, prMarkdownHtml, renderPrDescription } from "./prDescription";

const ctx = { owner: "acme", repo: "web" };

describe("prMarkdownHtml", () => {
  it("turns GitHub alerts into callouts and leaves plain quotes alone", () => {
    const html = prMarkdownHtml("> [!WARNING]\n> Run `just reseed` first.\n\n> plain", ctx);
    expect(html).toContain('<div class="md-alert md-alert-warning"><p class="md-alert-title">⚠ Warning</p><p>Run <code>just reseed</code> first.</p>');
    expect(html).toContain("<blockquote>\n<p>plain</p>");
    for (const kind of ["NOTE", "TIP", "IMPORTANT", "CAUTION"]) {
      expect(prMarkdownHtml(`> [!${kind}]\n> x`)).toContain(`md-alert-${kind.toLowerCase()}`);
    }
  });

  it("draws task lists without form controls", () => {
    const html = prMarkdownHtml("- [x] done\n- [ ] todo");
    expect(html).toContain('<ul class="md-tasks">');
    expect(html).toContain('<li class="md-task md-done"><span class="md-tb" aria-label="done">✓</span><span>done</span></li>');
    expect(html).toContain('<li class="md-task"><span class="md-tb" aria-label="to do"></span><span>todo</span></li>');
    expect(prMarkdownHtml("- plain")).toBe("<ul>\n<li>plain</li>\n</ul>\n");
    expect(html).not.toContain("<input");
  });

  it("wraps tables so a wide one scrolls on its own", () => {
    expect(prMarkdownHtml("| a | b |\n|---|---|\n| 1 | 2 |")).toMatch(/^<div class="md-table-wrap"><table><thead><tr><th>a<\/th>/);
  });

  it("links mentions to profiles and refs to the repo", () => {
    const html = prMarkdownHtml("Ping @martin and @acme/core about #1758 and other/lib#3, not a@b.dev", ctx);
    expect(html).toContain('<a class="md-mention" href="https://github.com/martin">@martin</a>');
    expect(html).toContain('<a class="md-mention" href="https://github.com/orgs/acme/teams/core">@acme/core</a>');
    expect(html).toContain('<a class="md-ref" href="https://github.com/acme/web/issues/1758">#1758</a>');
    expect(html).toContain('<a class="md-ref" href="https://github.com/other/lib/issues/3">other/lib#3</a>');
    expect(html).not.toContain("github.com/b.dev");
  });

  it("never nests a mention or ref inside a written link", () => {
    expect(prMarkdownHtml("[see #12 by @martin](https://example.com)", ctx)).toBe('<p><a href="https://example.com">see #12 by @martin</a></p>\n');
  });

  it("leaves #refs alone without a repo, and in code", () => {
    expect(prMarkdownHtml("see #12")).not.toContain("md-ref");
    expect(prMarkdownHtml("`#12` and\n\n```\n@martin #12\n```", ctx)).not.toMatch(/md-(ref|mention)/);
  });

  it("keeps fenced code's language for highlighting and passes <details> through", () => {
    expect(prMarkdownHtml("```ts\nconst a = 1;\n```")).toContain('<pre><code class="language-ts">const a = 1;');
    expect(prMarkdownHtml("<details><summary>Why</summary>\n\nBecause.\n</details>")).toContain("<details><summary>Why</summary>");
  });

  it("marks images so the component can route and zoom them", () => {
    expect(prMarkdownHtml("![shot](https://github.com/user-attachments/assets/abc)")).toContain(
      '<img class="md-img" src="https://github.com/user-attachments/assets/abc" alt="shot">',
    );
  });
});

describe("imageRoute", () => {
  it("sends GitHub attachment hosts through the authenticated backend fetch", () => {
    for (const src of [
      "https://github.com/user-attachments/assets/1b2c",
      "https://github.com/acme/web/assets/1/abc",
      "https://private-user-images.githubusercontent.com/1/2.png?jwt=x",
      "https://user-images.githubusercontent.com/1/2.png",
      "https://raw.githubusercontent.com/acme/web/main/a.png",
    ]) {
      expect(imageRoute(src)).toBe("github");
    }
  });

  it("loads other https images directly and refuses the rest", () => {
    expect(imageRoute("https://img.shields.io/badge/a-b-green")).toBe("direct");
    expect(imageRoute("http://example.com/a.png")).toBe("blocked");
    expect(imageRoute("data:image/png;base64,AAAA")).toBe("blocked");
    expect(imageRoute("javascript:alert(1)")).toBe("blocked");
    expect(imageRoute("https://user:pw@example.com/a.png")).toBe("blocked");
  });
});

describe("fenceLanguage", () => {
  it("maps fence tags onto the highlighter's languages", () => {
    expect(fenceLanguage("rust")).toBe("rust");
    expect(fenceLanguage("ts")).toBe("typescript");
    expect(fenceLanguage("sh")).toBe("bash");
    expect(fenceLanguage("shell")).toBe("bash");
    expect(fenceLanguage("SQL extra")).toBe("sql");
    expect(fenceLanguage("brainfuck")).toBeNull();
    expect(fenceLanguage("")).toBeNull();
  });
});

describe("renderPrDescription", () => {
  it("fails closed outside a browser DOM", () => {
    expect(renderPrDescription('<img src=x onerror="alert(1)">')).toBe("&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
  });
});
