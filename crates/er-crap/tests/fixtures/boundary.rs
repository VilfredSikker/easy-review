//! Boundary fixture: CC = 5 at 0% coverage scores EXACTLY 30 (not crappy).
//! Pins the strict \`>\` threshold semantics through the full pipeline:
//! a score of 30.0 must NOT fail the gate.

/// Four short-circuit operators (`&&`/`||`) — CC = 4 + 1 = 5. CRAP = 25 + 5 = 30.0 exactly.
pub fn boundary(a: bool, b: bool, c: bool, d: bool, e: bool) -> bool {
    a && b || c && d || e
}
