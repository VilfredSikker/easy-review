import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const src = readFileSync(join(here, "RulerBase.svelte"), "utf8");
const style = src.slice(src.indexOf("<style>"));

describe("RulerBase popover", () => {
  it("opens on hover or keyboard focus, never on the focus a click leaves behind", () => {
    expect(style).not.toContain(":focus-within");
    expect(style).toMatch(
      /\.ruler:hover \.rpop,\s*\.ruler:focus-visible \.rpop,\s*\.ruler:has\(:focus-visible\) \.rpop \{/,
    );
  });

  it("closes after a row is clicked", () => {
    const goto = src.slice(src.indexOf("function goto"), src.indexOf("function onKeydown"));
    expect(goto).toContain("onGoto(path);");
    expect(goto).toContain(".blur();");
    expect(src).toContain("onclick={(e) => goto(e, it.path)}");
  });

  it("closes on Escape", () => {
    expect(src).toContain('if (e.key !== "Escape") return;');
    expect(src).toContain("onkeydown={onKeydown}");
  });
});

function rule(selector: string): string {
  const start = style.indexOf(`${selector} {`);
  expect(start).toBeGreaterThanOrEqual(0);
  return style.slice(start, style.indexOf("}", start));
}

describe("RulerBase placement", () => {
  it("takes its own strip in the host's row instead of overlaying the code", () => {
    expect(src).toContain('<div class="ruler-gutter"');
    const gutter = rule(".ruler-gutter");
    expect(gutter).toContain("flex: none;");
    expect(gutter).toMatch(/width: \d+px;/);
    expect(gutter).not.toContain("position: absolute");
    expect(rule(".ruler")).not.toContain("position: absolute");
  });

  it("stacks over the sticky file header (z-30) but under app menus (z-40)", () => {
    const z = Number(/z-index: (\d+);/.exec(rule(".ruler-gutter"))?.[1]);
    expect(z).toBeGreaterThan(30);
    expect(z).toBeLessThan(40);
  });

  it("keeps the popover inside the strip's height", () => {
    expect(rule(".rpop")).toContain("calc(100% - 16px)");
  });
});
