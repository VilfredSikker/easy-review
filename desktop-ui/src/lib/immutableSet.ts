// Copy-on-write Set helpers for `$state` that is replaced, never mutated in
// place. Assigning a fresh Set is what triggers the update, so these stay
// plain Sets rather than SvelteSets.

/** Copy of `set` with `value` added when absent, removed when present. */
export function toggled<T>(set: ReadonlySet<T>, value: T): Set<T> {
  const next = new Set(set);
  if (next.has(value)) next.delete(value);
  else next.add(value);
  return next;
}

/** Copy of `set` with every one of `values` added. */
export function withAll<T>(set: ReadonlySet<T>, values: Iterable<T>): Set<T> {
  const next = new Set(set);
  for (const v of values) next.add(v);
  return next;
}

/** Copy of `set` with every one of `values` removed. */
export function without<T>(set: ReadonlySet<T>, values: Iterable<T>): Set<T> {
  const next = new Set(set);
  for (const v of values) next.delete(v);
  return next;
}
