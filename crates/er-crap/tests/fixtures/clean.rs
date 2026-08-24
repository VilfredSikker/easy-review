//! Clean fixture: simple functions, fully covered.
//! Used by the positive gate test (gate_clean.rs).
//! Keep `clean.lcov` line numbers in sync when editing this file.

/// Trivial function — CC = 1.
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

/// One decision — CC = 2.
pub fn sign(x: i32) -> &'static str {
    if x >= 0 {
        "non-negative"
    } else {
        "negative"
    }
}

/// Two conditions (if / else-if) — CC = 3.
pub fn describe(n: i32) -> &'static str {
    if n == 0 {
        "zero"
    } else if n > 0 {
        "positive"
    } else {
        "negative"
    }
}

/// Three match arms — CC = 4.
pub fn pick(kind: u8) -> &'static str {
    match kind {
        0 => "a",
        1 => "b",
        _ => "c",
    }
}

/// One short-circuit — CC = 2.
pub fn within(lo: i32, x: i32, hi: i32) -> bool {
    x >= lo && x <= hi
}
