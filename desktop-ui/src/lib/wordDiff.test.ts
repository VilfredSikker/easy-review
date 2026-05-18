import { describe, expect, it, beforeEach } from "bun:test";
import { wordDiff, _clearWordDiffCache } from "./wordDiff";

beforeEach(() => {
  _clearWordDiffCache();
});

function joinChanged(spans: { text: string; changed: boolean }[]): string {
  return spans.filter((s) => s.changed).map((s) => s.text).join("");
}

function joinAll(spans: { text: string; changed: boolean }[]): string {
  return spans.map((s) => s.text).join("");
}

describe("wordDiff", () => {
  it("identical lines → all spans unchanged", () => {
    const r = wordDiff("const foo = 1;", "const foo = 1;");
    expect(r.old.every((s) => !s.changed)).toBe(true);
    expect(r.new.every((s) => !s.changed)).toBe(true);
    expect(joinAll(r.old)).toBe("const foo = 1;");
    expect(joinAll(r.new)).toBe("const foo = 1;");
  });

  it("word swap → only the swapped word marked changed on each side", () => {
    const r = wordDiff("const foo = 1;", "const bar = 1;");
    // Round-trip: every span's text concatenates back to the original.
    expect(joinAll(r.old)).toBe("const foo = 1;");
    expect(joinAll(r.new)).toBe("const bar = 1;");
    // Only the differing token is flagged.
    expect(joinChanged(r.old)).toBe("foo");
    expect(joinChanged(r.new)).toBe("bar");
  });

  it("empty old → everything on new side is changed", () => {
    const r = wordDiff("", "hello world");
    expect(r.old).toEqual([]);
    expect(r.new.length).toBeGreaterThan(0);
    expect(r.new.every((s) => s.changed)).toBe(true);
    expect(joinAll(r.new)).toBe("hello world");
  });

  it("empty new → everything on old side is changed", () => {
    const r = wordDiff("hello world", "");
    expect(r.new).toEqual([]);
    expect(r.old.length).toBeGreaterThan(0);
    expect(r.old.every((s) => s.changed)).toBe(true);
    expect(joinAll(r.old)).toBe("hello world");
  });

  it("whitespace-only change is marked", () => {
    const r = wordDiff("a  b", "a b");
    expect(joinAll(r.old)).toBe("a  b");
    expect(joinAll(r.new)).toBe("a b");
    // The whitespace token differs → at least one side has a changed span.
    const anyChanged = r.old.some((s) => s.changed) || r.new.some((s) => s.changed);
    expect(anyChanged).toBe(true);
  });

  it("string literal change marks only the literal", () => {
    const r = wordDiff('return "hello";', 'return "world";');
    expect(joinAll(r.old)).toBe('return "hello";');
    expect(joinAll(r.new)).toBe('return "world";');
    expect(joinChanged(r.old)).toBe('"hello";');
    expect(joinChanged(r.new)).toBe('"world";');
  });
});
