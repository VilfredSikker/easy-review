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
