import { describe, expect, it } from "vitest";
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { isPaletteSearchFocused, paletteQuickActionKey } from "./commandPaletteKeys";

function key(
  key: string,
  mods: { metaKey?: boolean; ctrlKey?: boolean; altKey?: boolean } = {},
) {
  return {
    key,
    metaKey: mods.metaKey ?? false,
    ctrlKey: mods.ctrlKey ?? false,
    altKey: mods.altKey ?? false,
  };
}

describe("isPaletteSearchFocused", () => {
  it("is true only for the palette search input", () => {
    expect(isPaletteSearchFocused({ dataset: { paletteSearch: "true" } } as EventTarget)).toBe(true);
    expect(isPaletteSearchFocused({ dataset: {} } as EventTarget)).toBe(false);
    expect(isPaletteSearchFocused(null)).toBe(false);
  });
});

describe("paletteQuickActionKey", () => {
  it("returns null while search is focused so letters type into the filter", () => {
    expect(paletteQuickActionKey(key("a"), true)).toBeNull();
    expect(paletteQuickActionKey(key("/"), true)).toBeNull();
    expect(paletteQuickActionKey(key("s"), true)).toBeNull();
  });

  it("treats / as focus-search when search is not focused", () => {
    expect(paletteQuickActionKey(key("/"), false)).toBe("search");
  });

  it("treats a–z as letter jumps when search is not focused", () => {
    expect(paletteQuickActionKey(key("s"), false)).toBe("letter");
    expect(paletteQuickActionKey(key("A"), false)).toBe("letter");
  });

  it("ignores chords and non-letter keys", () => {
    expect(paletteQuickActionKey(key("s", { metaKey: true }), false)).toBeNull();
    expect(paletteQuickActionKey(key("s", { ctrlKey: true }), false)).toBeNull();
    expect(paletteQuickActionKey(key("s", { altKey: true }), false)).toBeNull();
    expect(paletteQuickActionKey(key("1"), false)).toBeNull();
    expect(paletteQuickActionKey(key("Enter"), false)).toBeNull();
    expect(paletteQuickActionKey(key("ArrowDown"), false)).toBeNull();
  });
});

describe("CommandPalette.svelte search focus", () => {
  const src = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "components", "CommandPalette.svelte"),
    "utf8",
  );
  const keyboard = readFileSync(
    join(dirname(fileURLToPath(import.meta.url)), "stores", "keyboard.ts"),
    "utf8",
  );

  it("does not autofocus the search input", () => {
    expect(src).not.toContain('focusSelector="input"');
    expect(src).toContain('id: "focus-search"');
    expect(src).toContain('kbd: "/"');
    expect(src).toContain("paletteQuickActionKey");
    expect(src).toContain("isPaletteSearchFocused");
    expect(src).toContain('data-palette-search="true"');
  });

  it("lets the palette own Escape while it is open", () => {
    expect(keyboard).toContain("commandPalette.open");
    const esc = keyboard.slice(keyboard.indexOf('if (e.key === "Escape")'));
    expect(esc.indexOf("commandPalette.open")).toBeLessThan(esc.indexOf("overlay.dismissTopModal"));
  });
});
