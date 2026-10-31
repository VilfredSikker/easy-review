import { describe, expect, it } from "bun:test";
import { onExternalLinkClick } from "./openExternalUrl";
import { fenceLanguage, imageRoute, prMarkdownHtml, refClickTarget, renderPrDescription } from "./prDescription";
import { LINEAR_COMMENT_BODY, VERCEL_COMMENT_BODY, VERCEL_HTML_COMMENT_BODY } from "./stories/botCommentFixtures";

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

describe("bot comments", () => {
  it("hides Vercel's state line and keeps its table and skipped-deploys fold", () => {
    const html = prMarkdownHtml(VERCEL_COMMENT_BODY);
    expect(html).not.toContain("[vc]");
    expect(html).not.toContain("eyJpc01vbm9yZXBv");
    expect(html).toContain("<strong>The latest updates on your projects</strong>");
    expect(html).toContain('<div class="md-table-wrap"><table>');
    expect(html).toContain("<details><summary>2 Skipped Deployments</summary>");
  });

  it("drops HTML comments, block and inline, and keeps the HTML around them", () => {
    const html = prMarkdownHtml(LINEAR_COMMENT_BODY);
    expect(html).not.toContain("<!--");
    expect(html).not.toContain("linear-linkback");
    expect(html).toContain('<p><a href="https://linear.app/acme/issue/DEV-298/');
    expect(prMarkdownHtml("a <!-- hidden --> b")).toBe("<p>a  b</p>\n");
    expect(prMarkdownHtml("<!-- never closed\nstill hidden")).not.toContain("hidden");
    expect(prMarkdownHtml(VERCEL_HTML_COMMENT_BODY)).not.toContain("vercel-deployment-summary");
  });

  it("leaves a comment written inside code alone", () => {
    expect(prMarkdownHtml("`<!-- x -->`")).toContain("<code>&lt;!-- x --&gt;</code>");
  });
});

describe("renderPrDescription", () => {
  it("fails closed outside a browser DOM", () => {
    expect(renderPrDescription('<img src=x onerror="alert(1)">')).toBe("&lt;img src=x onerror=&quot;alert(1)&quot;&gt;");
  });
});

describe("ref links under the app's external-link guard", () => {
  // Stand-ins for the anchors renderPrDescription emits: a ref link keeps its
  // URL in data attributes, an ordinary link keeps its href.
  class FakeAnchor {
    dataset: Record<string, string>;
    private href: string | null;
    constructor(href: string | null, dataset: Record<string, string> = {}) {
      this.href = href;
      this.dataset = dataset;
    }
    getAttribute(name: string) {
      return name === "href" ? this.href : null;
    }
    closest(selector: string) {
      if (selector === "a[href]") return this.href === null ? null : this;
      if (selector === "a[data-er-ref]") return this.dataset.erRef === undefined ? null : this;
      return null;
    }
  }

  function click(target: FakeAnchor, init: Partial<MouseEvent> = {}) {
    let prevented = false;
    return {
      button: 0,
      metaKey: false,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      ...init,
      target,
      get defaultPrevented() {
        return prevented;
      },
      preventDefault() {
        prevented = true;
      },
      stopPropagation() {},
    } as unknown as MouseEvent;
  }

  async function withGuardGlobals(run: (opened: string[]) => Promise<void>) {
    const saved = { window: globalThis.window, HTMLAnchorElement: globalThis.HTMLAnchorElement };
    const opened: string[] = [];
    Object.assign(globalThis, {
      HTMLAnchorElement: FakeAnchor,
      window: { location: { href: "http://localhost/" }, open: (url: string) => opened.push(url) },
    });
    try {
      await run(opened);
    } finally {
      Object.assign(globalThis, saved);
    }
  }

  const refAnchor = () =>
    new FakeAnchor(null, { erRef: "acme/web#12", erRefKind: "unknown", erHref: "https://github.com/acme/web/issues/12" });

  it("lets a ref click past the capture guard to the description's handler", async () => {
    await withGuardGlobals(async (opened) => {
      const e = click(refAnchor());
      onExternalLinkClick(e);
      expect(e.defaultPrevented).toBe(false);
      expect(opened).toEqual([]);
      expect(refClickTarget(e)).toEqual({
        ref: { owner: "acme", repo: "web", number: 12, kind: "unknown" },
        url: "https://github.com/acme/web/issues/12",
        plain: true,
      });
    });
  });

  it("sends modified and middle clicks on a ref to the browser", async () => {
    await withGuardGlobals(async () => {
      expect(refClickTarget(click(refAnchor(), { metaKey: true }))?.plain).toBe(false);
      expect(refClickTarget(click(refAnchor(), { button: 1 }))?.plain).toBe(false);
      expect(refClickTarget(click(refAnchor(), { button: 2 }))).toBeNull();
    });
  });

  it("still hands ordinary links to the guard", async () => {
    await withGuardGlobals(async (opened) => {
      const e = click(new FakeAnchor("https://github.com/acme/web/issues/7"));
      onExternalLinkClick(e);
      expect(e.defaultPrevented).toBe(true);
      expect(refClickTarget(e)).toBeNull();
      await new Promise((r) => setTimeout(r, 0));
      expect(opened).toEqual(["https://github.com/acme/web/issues/7"]);
    });
  });
});
