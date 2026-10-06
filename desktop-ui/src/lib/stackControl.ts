import type { StackSnapshot } from "./types";

/**
 * Pure view-model for the BranchCard stack control.
 *
 * Kept out of the Svelte component so the ordering, the `n / size` badge and the
 * row labels can be tested directly — the same split the TUI uses (plain rows in
 * the engine, rendering in the UI).
 */

/** One rendered dropdown row: a stack layer, or the trunk that closes the list. */
export interface StackRow {
  /** Branch name — the row's primary text. */
  branch: string;
  /** Right-aligned PR reference (`#42`), empty for a layer without a PR. */
  pr_ref: string;
  /** Muted trailing state text (`open`, `merged · needs rebase`, `trunk`). */
  state: string;
  /** True for the layer this tab is viewing. */
  is_current: boolean;
  /** True when clicking the row can switch the view to it. */
  selectable: boolean;
  /** PR number to open, when the row maps to one. */
  pr_number: number | null;
  needs_rebase: boolean;
}

/**
 * Rows for the dropdown: layers top-of-stack first, then the trunk.
 *
 * The trunk is appended here rather than included in the wire payload so that
 * `position` / `size` stay about stack layers only — the trunk isn't a layer and
 * isn't part of the `n / size` count.
 */
export function stackRows(stack: StackSnapshot | null | undefined): StackRow[] {
  if (!stack || stack.layers.length === 0) return [];

  const rows: StackRow[] = stack.layers.map((layer) => ({
    branch: layer.branch,
    pr_ref: layer.pr_number === null ? "" : `#${layer.pr_number}`,
    state: layer.state,
    is_current: layer.is_current,
    // Switchable only when there is a PR to open (`open_pr_branch` needs a PR
    // number) and it isn't the layer already on screen.
    selectable: layer.enabled && layer.pr_number !== null && !layer.is_current,
    pr_number: layer.pr_number,
    needs_rebase: layer.needs_rebase,
  }));

  // The trunk is usually not one of the wire layers; append it only when it
  // isn't already listed (some `gh stack` versions include it), so the `{#each}`
  // keyed by branch can't collide.
  if (stack.trunk && !rows.some((row) => row.branch === stack.trunk)) {
    rows.push({
      branch: stack.trunk,
      pr_ref: "",
      state: "trunk",
      is_current: false,
      selectable: false,
      pr_number: null,
      needs_rebase: false,
    });
  }

  return rows;
}

/**
 * The `n / size` badge, or `null` when there's no position to show (no stack, or
 * the current branch isn't one of its layers).
 */
export function stackBadge(stack: StackSnapshot | null | undefined): string | null {
  if (!stack || stack.position === null || stack.size === 0) return null;
  return `${stack.position} / ${stack.size}`;
}

/**
 * Short label for the control's collapsed button: the badge when we have one,
 * otherwise a placeholder for a lookup that hasn't landed (or failed).
 */
export function stackSummary(stack: StackSnapshot | null | undefined): string | null {
  if (!stack) return null;
  if (stack.loading) return "Reading…";
  return stackBadge(stack) ?? "Stack";
}

/** Tooltip/aria text explaining the control's state. */
export function stackTitle(stack: StackSnapshot | null | undefined): string {
  if (!stack) return "Stacked PRs";
  if (stack.unavailable) return `Stacked PRs — ${stack.unavailable}`;
  const badge = stackBadge(stack);
  if (!badge) return "Stacked PRs";
  const current = stack.layers.find((layer) => layer.is_current);
  return current
    ? `Stack layer ${badge} — viewing ${current.branch}`
    : `Stack layer ${badge}`;
}

/** Whether a lookup has yet to say whether this branch belongs to a stack. */
export function stackUnknown(stack: StackSnapshot | null | undefined): boolean {
  return !stack || (stack.unavailable === null && stack.layers.length === 0);
}

/**
 * Whether the control should render at all.
 *
 * `isPr` is true when the viewed branch has a PR. A stack layer always does, so
 * a plain branch header stays quiet: the placeholder that lets the user trigger
 * the lazy first lookup only appears where a stack could plausibly exist. Once a
 * lookup lands, the layer count (`isPr` or not) decides.
 */
export function shouldShowStackControl(
  stack: StackSnapshot | null | undefined,
  isPr = true,
): boolean {
  if (!stack) return false;
  // A definitive "not in a stack" (or missing extension) has nothing to show —
  // hiding the control keeps the branch header quiet. A *failed* lookup keeps it
  // so the reason and the refresh button stay reachable.
  if (stack.unavailable) return stack.retryable;
  if (stack.layers.length > 0) return true;
  return isPr;
}

/** Whether a row can be switched to, and the PR number to open for it. */
export function selectablePrNumber(row: StackRow): number | null {
  return row.selectable ? row.pr_number : null;
}

/** Tooltip for a dropdown row — says what clicking it does, or why it can't. */
export function stackRowTitle(row: StackRow): string {
  if (row.selectable && row.pr_number !== null) {
    return `Review PR #${row.pr_number} (${row.branch})`;
  }
  if (row.is_current) return "Currently viewing";
  if (row.state === "trunk") return "Trunk the stack is based on";
  return "No PR yet";
}

/** Width of the stack dropdown, in px (`w-64`). */
export const STACK_MENU_WIDTH = 256;

/** Gap kept between the dropdown and the window edge, in px. */
const STACK_MENU_EDGE_GAP = 8;

/**
 * Where the stack dropdown sits, in viewport coordinates. It is drawn `fixed`
 * because the right panel clips overflow: anchored inside it, a menu wider than
 * the space left of the button was cut off at the panel's edge. Right-aligned
 * under the button, then kept inside the window.
 */
export function stackMenuPosition(
  button: { right: number; bottom: number },
  viewportWidth: number,
): { top: number; left: number } {
  const maxLeft = viewportWidth - STACK_MENU_WIDTH - STACK_MENU_EDGE_GAP;
  const left = Math.max(STACK_MENU_EDGE_GAP, Math.min(button.right - STACK_MENU_WIDTH, maxLeft));
  return { top: button.bottom + 4, left };
}
