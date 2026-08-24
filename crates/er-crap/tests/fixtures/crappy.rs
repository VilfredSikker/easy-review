//! Crappy fixture: complex, barely-tested functions.
//! Used by the negative gate test (gate_crappy.rs).
//! Keep `crappy.lcov` line numbers in sync when editing this file.

/// Nine decision points — CC = 9. At 20% coverage CRAP = 81 × 0.8³ + 9 ≈ 50.5.
pub fn classify(input: &str, flag: bool, n: usize) -> &'static str {
    let mut out = "unknown";
    if input.is_empty() {
        out = "empty";
    } else if flag && n > 0 {
        out = "flagged";
    } else if n > 100 || n == 42 {
        out = "big";
    } else {
        for _ in 0..n {
            out = "looped";
        }
    }
    while flag && out == "unknown" {
        out = "done";
    }
    out
}

/// Eleven decision points — CC = 11. At 0% coverage CRAP = 121 + 11 = 132.
pub fn evaluate(a: i32, b: i32) -> i32 {
    let mut total = 0;
    for i in 0..a {
        if i % 2 == 0 {
            total += i;
        } else if i % 3 == 0 {
            total -= i;
        } else if i % 5 == 0 && b > 0 {
            total += b;
        }
        match i % 4 {
            0 => total += 1,
            1 => total += 2,
            _ => total += 3,
        }
    }
    while total > 100 && b > 10 {
        total /= 2;
    }
    total
}
