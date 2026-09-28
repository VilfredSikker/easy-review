/// What a keydown in the command palette asks for. Every intent but `none`
/// means the palette consumed the key, so the caller prevents the default.
export type PaletteKeyIntent =
  | "none"
  | "swallow"
  | "back"
  /** Escape outside the search field: the palette backs out of a submenu or closes. */
  | "leave"
  | "clear-query"
  | "blur-search"
  | "open-selected-submenu"
  | "open-selected"
  | "next"
  | "prev"
  | "focus-search"
  | "letter"
  | "reviewer-down"
  | "reviewer-up"
  | "reviewer-toggle"
  | "reviewer-run";

export interface PaletteKeyContext {
  /** The reviewer picker owns the keyboard instead of the list. */
  reviewersView: boolean;
  searchFocused: boolean;
  /** The search query has non-whitespace text. */
  hasQuery: boolean;
  inSubmenu: boolean;
  /** `paletteQuickActionKey` for this key. */
  quickAction: "search" | "letter" | null;
}

type PaletteKey = { key: string; metaKey: boolean; ctrlKey: boolean; altKey: boolean };

const REVIEWER_KEYS: Record<string, PaletteKeyIntent> = {
  Escape: "back",
  ArrowDown: "reviewer-down",
  ArrowUp: "reviewer-up",
  " ": "reviewer-toggle",
  Enter: "reviewer-run",
};

const LIST_KEYS: Record<string, PaletteKeyIntent> = {
  ArrowDown: "next",
  ArrowUp: "prev",
  Enter: "open-selected",
};

function escapeIntent(ctx: PaletteKeyContext): PaletteKeyIntent {
  if (ctx.searchFocused) return ctx.hasQuery ? "clear-query" : "blur-search";
  return "leave";
}

/// Arrow keys that walk the submenu tree; they belong to the search field
/// while it has focus.
function treeIntent(e: PaletteKey, ctx: PaletteKeyContext): PaletteKeyIntent | null {
  if (ctx.searchFocused) return null;
  if (e.key === "ArrowLeft" && ctx.inSubmenu) return "back";
  if (e.key === "ArrowRight") return "open-selected-submenu";
  return null;
}

function quickIntent(e: PaletteKey, ctx: PaletteKeyContext): PaletteKeyIntent {
  const action = ctx.quickAction;
  if (action === "search") return "focus-search";
  if (action === "letter") return "letter";
  const unchorded = !e.metaKey && !e.ctrlKey && !e.altKey;
  // Outside the search field, stray typing must not reach the page behind.
  if (!ctx.searchFocused && unchorded && (e.key.length === 1 || e.key === "Backspace")) {
    return "swallow";
  }
  return "none";
}

export function paletteKeyIntent(e: PaletteKey, ctx: PaletteKeyContext): PaletteKeyIntent {
  if (ctx.reviewersView) return Object.hasOwn(REVIEWER_KEYS, e.key) ? REVIEWER_KEYS[e.key] : "none";
  if (e.key === "Escape") return escapeIntent(ctx);
  const tree = treeIntent(e, ctx);
  if (tree) return tree;
  if (Object.hasOwn(LIST_KEYS, e.key)) return LIST_KEYS[e.key];
  return quickIntent(e, ctx);
}

/// Single-letter keybinds for the items in view, lower-cased; a later item
/// with the same letter wins.
export function keybindIndex<T extends { kbd?: string }>(items: readonly T[]): Map<string, T> {
  const m = new Map<string, T>();
  for (const item of items) {
    if (item.kbd) m.set(item.kbd.toLowerCase(), item);
  }
  return m;
}

/// Description for the "Validate / re-anchor" entry: what it would re-anchor,
/// or why it cannot run.
export function validateDescription(opts: {
  inScope: boolean;
  scopeDescription: string;
  hasReviewJson: boolean;
  eligibleCommentCount: number;
}): string {
  const { inScope, scopeDescription, hasReviewJson, eligibleCommentCount } = opts;
  if (!inScope) return scopeDescription;
  const hasComments = eligibleCommentCount > 0;
  if (!hasReviewJson && !hasComments) return "Run General review or add GitHub comments first";
  if (hasReviewJson && hasComments) return `Re-anchor review + ${eligibleCommentCount} comment(s)`;
  if (hasComments) return `Re-anchor ${eligibleCommentCount} GitHub comment(s)`;
  return "Re-anchor AI review findings";
}

export function providerDescription(p: { models: readonly unknown[]; is_selected: boolean }): string {
  const n = p.models.length;
  if (n > 0) return `${n} model${n === 1 ? "" : "s"}${p.is_selected ? " · active" : ""}`;
  return p.is_selected ? "active" : "no model presets";
}
