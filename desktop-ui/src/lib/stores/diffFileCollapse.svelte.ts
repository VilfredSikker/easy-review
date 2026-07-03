import { untrack } from "svelte";

/** Per-file diff body collapse in the flat diff view (keyed by file path). */
let collapsed = $state<ReadonlySet<string>>(new Set());
/** Bumped on every mutation so derived models reliably re-run. */
let revision = $state(0);

function commit(next: ReadonlySet<string>) {
  collapsed = next;
  revision++;
}

function isCollapsed(filePath: string): boolean {
  return collapsed.has(filePath);
}

// Every mutator below reads `collapsed` (as a no-op guard, or to clone it)
// before writing it. Called from inside a $derived/$effect without untrack,
// that read-then-write makes the caller depend on the very state it's
// mutating, which can reschedule the caller and wedge Svelte's reactive
// flush (chevrons/checkboxes/sticky headers stop updating until a remount).
// Guarding here — once — makes every mutator safe regardless of caller
// context, instead of requiring each call site to remember to untrack it.

function toggle(filePath: string) {
  untrack(() => {
    const next = new Set(collapsed);
    if (next.has(filePath)) next.delete(filePath);
    else next.add(filePath);
    commit(next);
  });
}

function collapse(filePath: string) {
  untrack(() => {
    if (collapsed.has(filePath)) return;
    const next = new Set(collapsed);
    next.add(filePath);
    commit(next);
  });
}

function expand(filePath: string) {
  untrack(() => {
    if (!collapsed.has(filePath)) return;
    const next = new Set(collapsed);
    next.delete(filePath);
    commit(next);
  });
}

function collapseAll(paths: readonly string[]) {
  untrack(() => {
    if (paths.length === 0) return;
    const next = new Set(collapsed);
    for (const p of paths) next.add(p);
    commit(next);
  });
}

function expandAll() {
  untrack(() => {
    if (collapsed.size === 0) return;
    commit(new Set());
  });
}

function clear() {
  untrack(() => {
    if (collapsed.size === 0) return;
    commit(new Set());
  });
}

export const diffFileCollapse = {
  get collapsed() {
    return collapsed;
  },
  get revision() {
    return revision;
  },
  isCollapsed,
  toggle,
  collapse,
  expand,
  collapseAll,
  expandAll,
  clear,
};
