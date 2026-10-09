import { describe, expect, it } from "bun:test";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const src = readFileSync(
  join(dirname(fileURLToPath(import.meta.url)), "Disclosure.svelte"),
  "utf8",
);

/** The source of an `{#if …}` block, from its opening to its `{/if}`. */
function ifBlock(opening: string): string {
  const start = src.indexOf(opening);
  expect(start).toBeGreaterThan(-1);
  const end = src.indexOf("{/if}", start);
  expect(end).toBeGreaterThan(start);
  return src.slice(start, end);
}

describe("Disclosure", () => {
  it("starts collapsed, lets the parent bind the state, and announces it", () => {
    expect(src).toContain("open = $bindable(false)");
    expect(src).toContain("aria-expanded={open}");
    expect(src).toContain("onclick={() => (open = !open)}");
  });

  it("renders the body only inside the open block", () => {
    expect(ifBlock("{#if open}")).toContain("{@render children()}");
    // Rendered once, so "inside the block" is also "nowhere else".
    expect(src.split("{@render children()}").length - 1).toBe(1);
  });

  it("shows the teaser only while collapsed, and keeps it out of the button's name", () => {
    const teaser = ifBlock("{#if !open && preview}");
    expect(teaser).toContain("{preview}");
    expect(teaser).toContain('aria-hidden="true"');
  });
});
