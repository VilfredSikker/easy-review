import { describe, expect, test } from "bun:test";
import { paletteQuickActionKey } from "./commandPaletteKeys";
import {
  keybindIndex,
  paletteKeyIntent,
  providerDescription,
  validateDescription,
  type PaletteKeyContext as FullContext,
} from "./commandPaletteModel";

const key = (k: string, mods: Partial<{ metaKey: boolean; ctrlKey: boolean; altKey: boolean }> = {}) => ({
  key: k,
  metaKey: false,
  ctrlKey: false,
  altKey: false,
  ...mods,
});

type PaletteKeyContext = Omit<FullContext, "quickAction">;

/// Feeds the real quick-action classifier, as the palette does.
const intent = (e: ReturnType<typeof key>, ctx: PaletteKeyContext) =>
  paletteKeyIntent(e, { ...ctx, quickAction: paletteQuickActionKey(e, ctx.searchFocused) });

const root: PaletteKeyContext = { reviewersView: false, searchFocused: false, hasQuery: false, inSubmenu: false };
const search: PaletteKeyContext = { ...root, searchFocused: true };
const submenu: PaletteKeyContext = { ...root, inSubmenu: true };
const reviewers: PaletteKeyContext = { ...root, reviewersView: true, inSubmenu: true };

describe("paletteKeyIntent", () => {
  test("reviewer picker owns its keys and ignores the rest", () => {
    expect(intent(key("Escape"), reviewers)).toBe("back");
    expect(intent(key("ArrowDown"), reviewers)).toBe("reviewer-down");
    expect(intent(key("ArrowUp"), reviewers)).toBe("reviewer-up");
    expect(intent(key(" "), reviewers)).toBe("reviewer-toggle");
    expect(intent(key("Enter"), reviewers)).toBe("reviewer-run");
    expect(intent(key("a"), reviewers)).toBe("none");
    expect(intent(key("constructor"), reviewers)).toBe("none");
  });

  test("Escape clears the query, then blurs search, then leaves the view", () => {
    expect(intent(key("Escape"), { ...search, hasQuery: true })).toBe("clear-query");
    expect(intent(key("Escape"), search)).toBe("blur-search");
    expect(intent(key("Escape"), submenu)).toBe("leave");
    expect(intent(key("Escape"), root)).toBe("leave");
  });

  test("left/right walk the submenu tree only outside the search field", () => {
    expect(intent(key("ArrowLeft"), submenu)).toBe("back");
    expect(intent(key("ArrowLeft"), root)).toBe("none");
    expect(intent(key("ArrowRight"), root)).toBe("open-selected-submenu");
    expect(intent(key("ArrowLeft"), { ...search, inSubmenu: true })).toBe("none");
    expect(intent(key("ArrowRight"), search)).toBe("none");
  });

  test("up/down/Enter navigate the list, with or without search focus", () => {
    for (const ctx of [root, search]) {
      expect(intent(key("ArrowDown"), ctx)).toBe("next");
      expect(intent(key("ArrowUp"), ctx)).toBe("prev");
      expect(intent(key("Enter"), ctx)).toBe("open-selected");
    }
  });

  test("quick keys fire outside the search field; stray typing is swallowed", () => {
    expect(intent(key("/"), root)).toBe("focus-search");
    expect(intent(key("t"), root)).toBe("letter");
    expect(intent(key("1"), root)).toBe("swallow");
    expect(intent(key(" "), root)).toBe("swallow");
    expect(intent(key("Backspace"), root)).toBe("swallow");
    expect(intent(key("Tab"), root)).toBe("none");
  });

  test("chorded keys and typing in the search field pass through", () => {
    expect(intent(key("t", { metaKey: true }), root)).toBe("none");
    expect(intent(key("1", { ctrlKey: true }), root)).toBe("none");
    expect(intent(key("t"), search)).toBe("none");
    expect(intent(key("/"), search)).toBe("none");
    expect(intent(key("Backspace"), search)).toBe("none");
  });
});

describe("keybindIndex", () => {
  test("lower-cases keys, skips items without one, and the last duplicate wins", () => {
    const m = keybindIndex([{ id: 1, kbd: "T" }, { id: 2 }, { id: 3, kbd: "t" }, { id: 4, kbd: "r" }]);
    expect([...m.keys()]).toEqual(["t", "r"]);
    expect(m.get("t")?.id).toBe(3);
  });
});

describe("validateDescription", () => {
  const base = { inScope: true, scopeDescription: "Not in scope", hasReviewJson: false, eligibleCommentCount: 0 };

  test("out of scope shows the scope description", () => {
    expect(validateDescription({ ...base, inScope: false, hasReviewJson: true })).toBe("Not in scope");
  });

  test("nothing to validate", () => {
    expect(validateDescription(base)).toBe("Run General review or add GitHub comments first");
  });

  test("review and comments", () => {
    expect(validateDescription({ ...base, hasReviewJson: true, eligibleCommentCount: 2 })).toBe(
      "Re-anchor review + 2 comment(s)",
    );
  });

  test("comments only", () => {
    expect(validateDescription({ ...base, eligibleCommentCount: 3 })).toBe("Re-anchor 3 GitHub comment(s)");
  });

  test("review only", () => {
    expect(validateDescription({ ...base, hasReviewJson: true })).toBe("Re-anchor AI review findings");
  });
});

describe("providerDescription", () => {
  test("counts models and marks the active provider", () => {
    expect(providerDescription({ models: [1], is_selected: false })).toBe("1 model");
    expect(providerDescription({ models: [1, 2], is_selected: true })).toBe("2 models · active");
    expect(providerDescription({ models: [], is_selected: true })).toBe("active");
    expect(providerDescription({ models: [], is_selected: false })).toBe("no model presets");
  });
});
